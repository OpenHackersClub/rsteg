//! First TDD red test for PNG. Round-trip an empty (header-only) payload
//! through a tiny RGB8 PNG that uses Filter=None on every row.

use rsteg_core::{Density, EmbedOpts, ExtractOpts, FormatAdapter, PayloadHeader, SchemeFourcc};
use rsteg_png::PNG_ADAPTER;

/// Build a minimal 8-bit RGB PNG of `width`×`height` at `fill` with filter
/// byte 0 (None) on every row, producing a valid PNG the adapter can decode.
fn make_png_rgb8(width: u32, height: u32, fill: [u8; 3]) -> Vec<u8> {
    use miniz_oxide::deflate::compress_to_vec_zlib;

    let mut raw = Vec::with_capacity((1 + (width as usize) * 3) * (height as usize));
    for _ in 0..height {
        raw.push(0); // filter: None
        for _ in 0..width {
            raw.extend_from_slice(&fill);
        }
    }
    let idat_data = compress_to_vec_zlib(&raw, 6);

    let mut out = Vec::new();
    out.extend_from_slice(b"\x89PNG\r\n\x1a\n");

    // IHDR
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.push(8); // bit depth
    ihdr.push(2); // color type RGB
    ihdr.push(0); // compression
    ihdr.push(0); // filter
    ihdr.push(0); // interlace
    write_chunk(&mut out, *b"IHDR", &ihdr);

    // IDAT
    write_chunk(&mut out, *b"IDAT", &idat_data);

    // IEND
    write_chunk(&mut out, *b"IEND", &[]);

    out
}

fn write_chunk(out: &mut Vec<u8>, ty: [u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let crc_start = out.len();
    out.extend_from_slice(&ty);
    out.extend_from_slice(data);
    let crc = rsteg_core::crc32_ieee(&out[crc_start..]);
    out.extend_from_slice(&crc.to_be_bytes());
}

#[test]
fn roundtrip_empty_payload_rgb8_linear_density_low() {
    let cover = make_png_rgb8(64, 64, [0x40, 0x80, 0xC0]);

    let header = PayloadHeader::plain(SchemeFourcc::PNG_LSB_LINEAR, Density::Low, &[]);
    let framed = header.encode_with(&[]);
    assert_eq!(framed.len(), PayloadHeader::SIZE);

    let opts = EmbedOpts {
        scheme: Some("png-lsb-linear"),
        density: Density::Low,
    };
    let stego = PNG_ADAPTER
        .embed(&cover, &framed, &opts)
        .expect("embed should succeed");

    // PNG re-encodes, so file length may differ. The important property
    // is that extract yields the same framed bytes.
    assert!(!stego.is_empty());
    assert_eq!(&stego[..8], b"\x89PNG\r\n\x1a\n", "signature preserved");

    let extracted = PNG_ADAPTER
        .extract(
            &stego,
            &ExtractOpts {
                scheme: Some("png-lsb-linear"),
                density: Some(Density::Low),
                skip_header: false,
                raw_bit_count: None,
            },
        )
        .expect("extract should succeed");

    assert_eq!(
        extracted, framed,
        "extracted framed bytes must equal the embedded framed bytes"
    );
}
