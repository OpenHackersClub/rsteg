//! Body-bearing round-trips, density sweep, RGBA variant, tamper, cross-format
//! refusal, and ancillary-chunk preservation for PNG.
//!
//! Covers items 2, 3, 4, 5, 6, 10 (plaintext), 11, 12, 15, 18 (RGB/RGBA half)
//! of the minimum set in `specs/07-testing.md`.

use rsteg_core::{
    Density, EmbedOpts, Error, ExtractOpts, FormatAdapter, PayloadHeader, SchemeFourcc,
};
use rsteg_png::PNG_ADAPTER;

fn write_chunk(out: &mut Vec<u8>, ty: [u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let crc_start = out.len();
    out.extend_from_slice(&ty);
    out.extend_from_slice(data);
    let crc = rsteg_core::crc32_ieee(&out[crc_start..]);
    out.extend_from_slice(&crc.to_be_bytes());
}

/// Minimal 8-bit PNG builder. `color_type` = 2 (RGB) or 6 (RGBA).
fn make_png(width: u32, height: u32, color_type: u8, fill: [u8; 4], extras: &[([u8; 4], Vec<u8>)]) -> Vec<u8> {
    use miniz_oxide::deflate::compress_to_vec_zlib;
    let channels = if color_type == 2 { 3usize } else { 4 };
    let mut raw = Vec::with_capacity((1 + width as usize * channels) * height as usize);
    for _ in 0..height {
        raw.push(0);
        for _ in 0..width {
            raw.extend_from_slice(&fill[..channels]);
        }
    }
    let idat = compress_to_vec_zlib(&raw, 6);
    let mut out = Vec::new();
    out.extend_from_slice(b"\x89PNG\r\n\x1a\n");
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.push(8);
    ihdr.push(color_type);
    ihdr.push(0);
    ihdr.push(0);
    ihdr.push(0);
    write_chunk(&mut out, *b"IHDR", &ihdr);
    for (ty, data) in extras {
        write_chunk(&mut out, *ty, data);
    }
    write_chunk(&mut out, *b"IDAT", &idat);
    write_chunk(&mut out, *b"IEND", &[]);
    out
}

fn roundtrip_with(cover: &[u8], body: &[u8], density: Density) {
    let header = PayloadHeader::plain(SchemeFourcc::PNG_LSB_LINEAR, density, body);
    let framed = header.encode_with(body);
    let stego = PNG_ADAPTER
        .embed(
            cover,
            &framed,
            &EmbedOpts {
                scheme: Some("png-lsb-linear"),
                density,
            seed: None,
    },
        )
        .unwrap();
    let extracted = PNG_ADAPTER
        .extract(
            &stego,
            &ExtractOpts {
                scheme: Some("png-lsb-linear"),
                density: Some(density),
                skip_header: false,
                raw_bit_count: None,
            seed: None,
    },
        )
        .unwrap();
    assert_eq!(extracted, framed);
}

#[test]
fn single_byte_rgb_density_low() {
    let cover = make_png(32, 32, 2, [0x40, 0x80, 0xC0, 0xFF], &[]);
    roundtrip_with(&cover, &[0xA5], Density::Low);
}

#[test]
fn mid_size_rgb_density_low() {
    let cover = make_png(64, 64, 2, [0x20, 0x30, 0x40, 0xFF], &[]);
    let body: Vec<u8> = (0..=255u8).collect();
    roundtrip_with(&cover, &body, Density::Low);
}

#[test]
fn rgba_density_low() {
    let cover = make_png(32, 32, 6, [0x40, 0x80, 0xC0, 0x40], &[]);
    roundtrip_with(&cover, b"rgba works too", Density::Low);
}

#[test]
fn density_moderate() {
    let cover = make_png(32, 32, 2, [0x10, 0x20, 0x30, 0xFF], &[]);
    roundtrip_with(&cover, b"density 2", Density::Moderate);
}

#[test]
fn density_aggressive3() {
    let cover = make_png(64, 64, 2, [0x10, 0x20, 0x30, 0xFF], &[]);
    let body = vec![0xCCu8; 400];
    roundtrip_with(&cover, &body, Density::Aggressive3);
}

#[test]
fn density_aggressive4() {
    let cover = make_png(64, 64, 2, [0x10, 0x20, 0x30, 0xFF], &[]);
    let body = vec![0xEFu8; 500];
    roundtrip_with(&cover, &body, Density::Aggressive4);
}

#[test]
fn over_capacity_reports_payload_too_large() {
    // 2×2 RGB = 12 pixel bytes = 12 bits at d=1 = 1 byte of capacity.
    // Header (32 B) exceeds that.
    let cover = make_png(2, 2, 2, [0, 0, 0, 0xFF], &[]);
    let framed = PayloadHeader::plain(SchemeFourcc::PNG_LSB_LINEAR, Density::Low, &[])
        .encode_with(&[]);
    let err = PNG_ADAPTER
        .embed(
            &cover,
            &framed,
            &EmbedOpts {
                scheme: Some("png-lsb-linear"),
                density: Density::Low,
            seed: None,
    },
        )
        .unwrap_err();
    assert!(matches!(err, Error::PayloadTooLarge { .. }), "got {err:?}");
}

#[test]
fn cross_format_refused() {
    let bmp_like = b"BMsome-other-bytes";
    assert!(!PNG_ADAPTER.recognize(bmp_like));
    let framed = PayloadHeader::plain(SchemeFourcc::PNG_LSB_LINEAR, Density::Low, &[])
        .encode_with(&[]);
    let err = PNG_ADAPTER
        .embed(
            bmp_like,
            &framed,
            &EmbedOpts {
                scheme: Some("png-lsb-linear"),
                density: Density::Low,
            seed: None,
    },
        )
        .unwrap_err();
    assert!(matches!(err, Error::FormatUnrecognized));
}

#[test]
fn plaintext_tamper_trips_body_crc() {
    let cover = make_png(64, 64, 2, [0x40, 0x80, 0xC0, 0xFF], &[]);
    let body = b"tamper-me".to_vec();
    let header = PayloadHeader::plain(SchemeFourcc::PNG_LSB_LINEAR, Density::Low, &body);
    let framed = header.encode_with(&body);
    let mut stego = PNG_ADAPTER
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

    // Mutate a byte deep inside the IDAT zlib stream so the decoded pixel
    // bytes differ from the embedded ones. Any byte change inside IDAT is
    // likely to scramble far more than one LSB, but it still hits the
    // scheme-level body-CRC path we want to test.
    let idat_offset = stego.len() / 2;
    stego[idat_offset] ^= 0x01;

    match PNG_ADAPTER.extract(
        &stego,
        &ExtractOpts {
            scheme: Some("png-lsb-linear"),
            density: Some(Density::Low),
            skip_header: false,
            raw_bit_count: None,
        seed: None,
    },
    ) {
        // Various error paths are acceptable; the guarantee is Err(not Ok(same bytes)).
        Err(_) => {}
        Ok(out) => assert_ne!(out, framed, "tampered PNG must not yield original framed"),
    }
}

#[test]
fn ancillary_chunks_preserved_verbatim() {
    // Inject a tEXt chunk before IDAT. Must round-trip byte-identically.
    let text = b"SoftwarersrstegPNG_test";
    let cover = make_png(32, 32, 2, [0x40, 0x80, 0xC0, 0xFF], &[(*b"tEXt", text.to_vec())]);

    // Record the original tEXt chunk bytes verbatim (length+type+data+CRC).
    let text_start = find_chunk(&cover, *b"tEXt").expect("tEXt in cover");
    let text_len = 12 + text.len();
    let text_original = cover[text_start..text_start + text_len].to_vec();

    let body = b"keep tEXt".to_vec();
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

    let text_in_stego = find_chunk(&stego, *b"tEXt").expect("tEXt in stego");
    assert_eq!(
        &stego[text_in_stego..text_in_stego + text_len],
        text_original.as_slice(),
    );
}

/// Find the starting byte offset of the first chunk with the given type.
fn find_chunk(bytes: &[u8], ty: [u8; 4]) -> Option<usize> {
    let mut cursor = 8usize;
    while cursor + 8 <= bytes.len() {
        let len = u32::from_be_bytes(bytes[cursor..cursor + 4].try_into().unwrap()) as usize;
        let chunk_ty: [u8; 4] = bytes[cursor + 4..cursor + 8].try_into().unwrap();
        if chunk_ty == ty {
            return Some(cursor);
        }
        cursor = cursor + 8 + len + 4;
    }
    None
}
