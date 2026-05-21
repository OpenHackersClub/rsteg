//! Permuted-scheme round-trips for `rsteg-png`.
//!
//! `png-lsb-permuted` scatters header and body bytes over the pixel bytes via
//! a splitmix64-seeded Fisher–Yates shuffle of the embedding-unit index list —
//! same pattern as `bmp-lsb-permuted` / `wav-lsb-permuted`. The
//! `PayloadHeader.scheme_fourcc` is `PLSP`; `flags.permuted = 1`.
//!
//! Closes the format-parity gap noted in `specs/05-formats.md` §PNG.

use rsteg_core::{
    Density, EmbedOpts, Error, ExtractOpts, FormatAdapter, PayloadHeader, SchemeFourcc,
};
use rsteg_png::PNG_ADAPTER;

/// Build a minimal 8-bit RGB PNG with filter byte 0 (None) on every row.
/// Identical fixture-builder to `roundtrip_empty.rs` — duplicated here so the
/// permuted test file stands on its own.
fn make_png_rgb8(width: u32, height: u32, fill: [u8; 3]) -> Vec<u8> {
    use miniz_oxide::deflate::compress_to_vec_zlib;

    let mut raw = Vec::with_capacity((1 + (width as usize) * 3) * (height as usize));
    for y in 0..height {
        raw.push(0); // filter: None
        for x in 0..width {
            // Vary pixels so post-embed re-deflate isn't a degenerate constant.
            raw.push(fill[0].wrapping_add((x ^ y) as u8));
            raw.push(fill[1].wrapping_add((x.wrapping_mul(3) ^ y) as u8));
            raw.push(fill[2].wrapping_add((x ^ y.wrapping_mul(5)) as u8));
        }
    }
    let idat_data = compress_to_vec_zlib(&raw, 6);

    let mut out = Vec::new();
    out.extend_from_slice(b"\x89PNG\r\n\x1a\n");

    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.push(8); // bit depth
    ihdr.push(2); // color type RGB
    ihdr.push(0); // compression
    ihdr.push(0); // filter method
    ihdr.push(0); // interlace
    write_chunk(&mut out, *b"IHDR", &ihdr);
    write_chunk(&mut out, *b"IDAT", &idat_data);
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

fn roundtrip_permuted(body: &[u8], density: Density, seed: u64) {
    let cover = make_png_rgb8(64, 64, [0x40, 0x80, 0xC0]);
    let mut header = PayloadHeader::plain(SchemeFourcc::PNG_LSB_PERMUTED, density, body);
    header.flags |= PayloadHeader::FLAG_PERMUTED;
    let framed = header.encode_with(body);

    let stego = PNG_ADAPTER
        .embed(
            &cover,
            &framed,
            &EmbedOpts {
                scheme: Some("png-lsb-permuted"),
                density,
                seed: Some(seed),
            },
        )
        .expect("permuted embed should succeed");
    assert_eq!(&stego[..8], b"\x89PNG\r\n\x1a\n", "signature preserved");

    let extracted = PNG_ADAPTER
        .extract(
            &stego,
            &ExtractOpts {
                scheme: Some("png-lsb-permuted"),
                density: Some(density),
                skip_header: false,
                raw_bit_count: None,
                seed: Some(seed),
            },
        )
        .expect("permuted extract should succeed");
    assert_eq!(extracted, framed);
}

#[test]
fn empty_payload_roundtrips_permuted_density_low() {
    roundtrip_permuted(&[], Density::Low, 0xDEADBEEF);
}

#[test]
fn mid_size_payload_permuted_density_low() {
    let body: Vec<u8> = (0..=199u8).collect();
    roundtrip_permuted(&body, Density::Low, 42);
}

#[test]
fn permuted_density_moderate_roundtrips() {
    roundtrip_permuted(b"permuted png density-2", Density::Moderate, 0x12345);
}

#[test]
fn permuted_density_aggressive4_roundtrips() {
    let body = vec![0xCCu8; 300];
    roundtrip_permuted(&body, Density::Aggressive4, 0x77777);
}

/// Permuted-without-seed must error with `PermutationSeedRequired` on embed
/// and extract — same contract as BMP/WAV.
#[test]
fn permuted_requires_seed_on_embed() {
    let cover = make_png_rgb8(16, 16, [0, 0, 0]);
    let framed =
        PayloadHeader::plain(SchemeFourcc::PNG_LSB_PERMUTED, Density::Low, &[]).encode_with(&[]);
    let err = PNG_ADAPTER
        .embed(
            &cover,
            &framed,
            &EmbedOpts {
                scheme: Some("png-lsb-permuted"),
                density: Density::Low,
                seed: None,
            },
        )
        .unwrap_err();
    assert!(matches!(err, Error::PermutationSeedRequired), "got {err:?}");
}

#[test]
fn permuted_requires_seed_on_extract() {
    let cover = make_png_rgb8(32, 32, [0x10, 0x20, 0x30]);
    let mut header = PayloadHeader::plain(SchemeFourcc::PNG_LSB_PERMUTED, Density::Low, &[]);
    header.flags |= PayloadHeader::FLAG_PERMUTED;
    let framed = header.encode_with(&[]);
    let stego = PNG_ADAPTER
        .embed(
            &cover,
            &framed,
            &EmbedOpts {
                scheme: Some("png-lsb-permuted"),
                density: Density::Low,
                seed: Some(1),
            },
        )
        .unwrap();
    let err = PNG_ADAPTER
        .extract(
            &stego,
            &ExtractOpts {
                scheme: Some("png-lsb-permuted"),
                density: Some(Density::Low),
                skip_header: false,
                raw_bit_count: None,
                seed: None,
            },
        )
        .unwrap_err();
    assert!(matches!(err, Error::PermutationSeedRequired), "got {err:?}");
}

/// Linear-produced stego must not decode as permuted (wrong order → garbage
/// magic). Mirrors the BMP analogue.
#[test]
fn linear_stego_fails_under_permuted_extract() {
    let cover = make_png_rgb8(64, 64, [0x40, 0x80, 0xC0]);
    let body = b"linear-produced".to_vec();
    let header = PayloadHeader::plain(SchemeFourcc::PNG_LSB_LINEAR, Density::Low, &body);
    let framed = header.encode_with(&body);
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
    match PNG_ADAPTER.extract(
        &stego,
        &ExtractOpts {
            scheme: Some("png-lsb-permuted"),
            density: Some(Density::Low),
            skip_header: false,
            raw_bit_count: None,
            seed: Some(1),
        },
    ) {
        Ok(out) => assert_ne!(
            out, framed,
            "permuted must not recover linear-produced plaintext"
        ),
        Err(_) => {}
    }
}
