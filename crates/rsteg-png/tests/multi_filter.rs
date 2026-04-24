//! Covers the "preserve per-row filter choices" invariant from spec 05.
//!
//! Builds a PNG with a mix of filter bytes (None/Sub/Up/Average/Paeth)
//! across rows, embeds a payload, re-decodes, and asserts both the
//! payload round-trips *and* each row's filter byte matches the cover.

use rsteg_core::{Density, EmbedOpts, ExtractOpts, FormatAdapter, PayloadHeader, SchemeFourcc};
use rsteg_png::PNG_ADAPTER;

fn write_chunk(out: &mut Vec<u8>, ty: [u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let crc_start = out.len();
    out.extend_from_slice(&ty);
    out.extend_from_slice(data);
    let crc = rsteg_core::crc32_ieee(&out[crc_start..]);
    out.extend_from_slice(&crc.to_be_bytes());
}

/// Make an RGB8 PNG where row `y` uses filter `filters[y % filters.len()]`.
fn make_png_with_filters(width: u32, height: u32, filters: &[u8]) -> Vec<u8> {
    use miniz_oxide::deflate::compress_to_vec_zlib;
    let channels = 3usize;
    let stride = width as usize * channels;
    let mut raw = Vec::with_capacity((1 + stride) * height as usize);

    // Synthesize pixels with a slight gradient so filters other than None
    // actually produce non-zero filtered bytes (testing the filter code).
    let mut pixels = vec![0u8; stride * height as usize];
    for y in 0..height as usize {
        for x in 0..width as usize {
            let i = y * stride + x * channels;
            pixels[i] = (x as u8).wrapping_mul(3);
            pixels[i + 1] = (y as u8).wrapping_mul(5);
            pixels[i + 2] = ((x + y) as u8).wrapping_mul(7);
        }
    }

    for y in 0..height as usize {
        let filter = filters[y % filters.len()];
        raw.push(filter);
        let curr = &pixels[y * stride..(y + 1) * stride];
        let prev: &[u8] = if y == 0 { &[] } else { &pixels[(y - 1) * stride..y * stride] };
        for i in 0..stride {
            let a = if i >= channels { curr[i - channels] } else { 0 };
            let b = if prev.is_empty() { 0 } else { prev[i] };
            let c = if prev.is_empty() || i < channels { 0 } else { prev[i - channels] };
            let filtered = match filter {
                0 => curr[i],
                1 => curr[i].wrapping_sub(a),
                2 => curr[i].wrapping_sub(b),
                3 => curr[i].wrapping_sub(((u16::from(a) + u16::from(b)) / 2) as u8),
                4 => {
                    let p = i32::from(a) + i32::from(b) - i32::from(c);
                    let pa = (p - i32::from(a)).unsigned_abs();
                    let pb = (p - i32::from(b)).unsigned_abs();
                    let pc = (p - i32::from(c)).unsigned_abs();
                    let pred = if pa <= pb && pa <= pc { a } else if pb <= pc { b } else { c };
                    curr[i].wrapping_sub(pred)
                }
                _ => unreachable!(),
            };
            raw.push(filtered);
        }
    }

    let idat = compress_to_vec_zlib(&raw, 6);
    let mut out = Vec::new();
    out.extend_from_slice(b"\x89PNG\r\n\x1a\n");
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.push(8);
    ihdr.push(2); // RGB
    ihdr.push(0);
    ihdr.push(0);
    ihdr.push(0);
    write_chunk(&mut out, *b"IHDR", &ihdr);
    write_chunk(&mut out, *b"IDAT", &idat);
    write_chunk(&mut out, *b"IEND", &[]);
    out
}

/// Re-decode a PNG, returning the original filter byte per row.
fn extract_row_filters(png: &[u8]) -> Vec<u8> {
    use miniz_oxide::inflate::decompress_to_vec_zlib;
    // Minimal parse: walk chunks, grab IDAT, inflate, read filter bytes.
    let mut cursor = 8usize;
    let mut width = 0u32;
    let mut height = 0u32;
    let mut color_type = 0u8;
    let mut idats: Vec<u8> = Vec::new();
    while cursor + 12 <= png.len() {
        let len = u32::from_be_bytes(png[cursor..cursor + 4].try_into().unwrap()) as usize;
        let ty: [u8; 4] = png[cursor + 4..cursor + 8].try_into().unwrap();
        let data_end = cursor + 8 + len;
        match &ty {
            b"IHDR" => {
                width = u32::from_be_bytes(png[cursor + 8..cursor + 12].try_into().unwrap());
                height = u32::from_be_bytes(png[cursor + 12..cursor + 16].try_into().unwrap());
                color_type = png[cursor + 17];
            }
            b"IDAT" => idats.extend_from_slice(&png[cursor + 8..data_end]),
            b"IEND" => break,
            _ => {}
        }
        cursor = data_end + 4;
    }
    let raw = decompress_to_vec_zlib(&idats).expect("inflate");
    let channels = if color_type == 2 { 3 } else { 4 };
    let stride = width as usize * channels;
    (0..height as usize)
        .map(|y| raw[y * (stride + 1)])
        .collect()
}

#[test]
fn preserves_per_row_filter_choice() {
    let cover = make_png_with_filters(32, 16, &[0, 1, 2, 3, 4]);
    let cover_filters = extract_row_filters(&cover);
    assert!(cover_filters.iter().any(|&f| f == 4), "cover should have at least one Paeth row");

    let body = b"filter-preservation-test".to_vec();
    let framed = PayloadHeader::plain(SchemeFourcc::PNG_LSB_LINEAR, Density::Low, &body)
        .encode_with(&body);
    let stego = PNG_ADAPTER
        .embed(
            &cover,
            &framed,
            &EmbedOpts {
                scheme: Some("png-lsb-linear"),
                density: Density::Low,
            seed: None,
    },
        )
        .unwrap();

    let stego_filters = extract_row_filters(&stego);
    assert_eq!(cover_filters, stego_filters, "row filter choices must be preserved");

    // And the payload still round-trips.
    let extracted = PNG_ADAPTER
        .extract(
            &stego,
            &ExtractOpts {
                scheme: Some("png-lsb-linear"),
                density: Some(Density::Low),
                skip_header: false,
                raw_bit_count: None,
            seed: None,
    },
        )
        .unwrap();
    assert_eq!(extracted, framed);
}
