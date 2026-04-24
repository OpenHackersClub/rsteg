//! Permuted-scheme round-trips.
//!
//! `bmp-lsb-permuted` scatters header and body bytes over the carrier via a
//! splitmix64-seeded Fisher–Yates shuffle of the embedding-unit index list.
//! The `PayloadHeader.scheme_fourcc` is `BLSP`; `flags.permuted = 1`.
//!
//! Covers items 9, 10 (plaintext variant), 12 of the minimum test set in
//! `specs/07-testing.md` for the permuted scheme.

use rsteg_bmp::BMP_ADAPTER;
use rsteg_core::{
    Density, EmbedOpts, Error, ExtractOpts, FormatAdapter, PayloadHeader, SchemeFourcc,
};

fn make_bmp24(width: u32, height: u32, fill: u8) -> Vec<u8> {
    let row_bytes = (width as usize) * 3;
    let stride = (row_bytes + 3) & !3;
    let pixel_bytes = stride * (height as usize);
    let file_size = 54 + pixel_bytes;
    let mut out = Vec::with_capacity(file_size);
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&(file_size as u32).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&54u32.to_le_bytes());
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(width as i32).to_le_bytes());
    out.extend_from_slice(&(height as i32).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&24u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&(pixel_bytes as u32).to_le_bytes());
    out.extend_from_slice(&2835i32.to_le_bytes());
    out.extend_from_slice(&2835i32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    for y in 0..height {
        for x in 0..row_bytes {
            out.push(fill.wrapping_add((x ^ y as usize) as u8));
        }
        for _ in row_bytes..stride {
            out.push(0);
        }
    }
    out
}

fn roundtrip_permuted(body: &[u8], density: Density, seed: u64) {
    let cover = make_bmp24(64, 64, 0x40);
    // Permuted header: flags.permuted = 1, scheme_fourcc = BLSP.
    let mut header = PayloadHeader::plain(SchemeFourcc::BMP_LSB_PERMUTED, density, body);
    header.flags |= PayloadHeader::FLAG_PERMUTED;
    let framed = header.encode_with(body);

    let stego = BMP_ADAPTER
        .embed(
            &cover,
            &framed,
            &EmbedOpts {
                scheme: Some("bmp-lsb-permuted"),
                density,
                seed: Some(seed),
            },
        )
        .unwrap();
    assert_eq!(stego.len(), cover.len());

    let extracted = BMP_ADAPTER
        .extract(
            &stego,
            &ExtractOpts {
                scheme: Some("bmp-lsb-permuted"),
                density: Some(density),
                skip_header: false,
                raw_bit_count: None,
                seed: Some(seed),
            },
        )
        .unwrap();
    assert_eq!(extracted, framed);
}

#[test]
fn empty_payload_roundtrips_under_permuted_density_low() {
    roundtrip_permuted(&[], Density::Low, 0xDEADBEEF);
}

#[test]
fn single_byte_payload_roundtrips_permuted() {
    roundtrip_permuted(&[0xA5], Density::Low, 0xCAFEBABE);
}

#[test]
fn mid_size_payload_permuted_density_low() {
    let body: Vec<u8> = (0..=199u8).collect();
    roundtrip_permuted(&body, Density::Low, 42);
}

#[test]
fn permuted_density_moderate_roundtrips() {
    roundtrip_permuted(b"permuted density-2", Density::Moderate, 0x12345);
}

#[test]
fn permuted_density_aggressive4_roundtrips() {
    let body = vec![0xCCu8; 400];
    roundtrip_permuted(&body, Density::Aggressive4, 0x77777);
}

/// Extracting with a different seed must NOT return the framed bytes.
///
/// It might return `Err(...)` (usually `HeaderBadMagic` because the permutation
/// reorders the magic elsewhere) or it might return `Ok(some_bytes)` where the
/// bytes decode to garbage. What it must NOT do is return `Ok(framed)`.
#[test]
fn wrong_seed_does_not_recover_plaintext() {
    let body = b"very secret message that must not leak".to_vec();
    let cover = make_bmp24(64, 64, 0x40);
    let mut header = PayloadHeader::plain(SchemeFourcc::BMP_LSB_PERMUTED, Density::Low, &body);
    header.flags |= PayloadHeader::FLAG_PERMUTED;
    let framed = header.encode_with(&body);

    let stego = BMP_ADAPTER
        .embed(
            &cover,
            &framed,
            &EmbedOpts {
                scheme: Some("bmp-lsb-permuted"),
                density: Density::Low,
                seed: Some(0xAAAAAAAA),
            },
        )
        .unwrap();

    match BMP_ADAPTER.extract(
        &stego,
        &ExtractOpts {
            scheme: Some("bmp-lsb-permuted"),
            density: Some(Density::Low),
            skip_header: false,
            raw_bit_count: None,
            seed: Some(0xBBBBBBBB),
        },
    ) {
        Ok(out) => assert_ne!(out, framed, "wrong seed must not recover plaintext"),
        Err(_) => {}
    }
}

/// Permuted-without-seed must error with `PermutationSeedRequired`.
#[test]
fn permuted_requires_seed_on_embed() {
    let cover = make_bmp24(32, 32, 0);
    let framed = PayloadHeader::plain(SchemeFourcc::BMP_LSB_PERMUTED, Density::Low, &[])
        .encode_with(&[]);
    let err = BMP_ADAPTER
        .embed(
            &cover,
            &framed,
            &EmbedOpts {
                scheme: Some("bmp-lsb-permuted"),
                density: Density::Low,
                seed: None,
            },
        )
        .unwrap_err();
    assert!(matches!(err, Error::PermutationSeedRequired), "got {err:?}");
}

#[test]
fn permuted_requires_seed_on_extract() {
    let cover = make_bmp24(32, 32, 0);
    let mut header = PayloadHeader::plain(SchemeFourcc::BMP_LSB_PERMUTED, Density::Low, &[]);
    header.flags |= PayloadHeader::FLAG_PERMUTED;
    let framed = header.encode_with(&[]);
    let stego = BMP_ADAPTER
        .embed(
            &cover,
            &framed,
            &EmbedOpts {
                scheme: Some("bmp-lsb-permuted"),
                density: Density::Low,
                seed: Some(1),
            },
        )
        .unwrap();
    let err = BMP_ADAPTER
        .extract(
            &stego,
            &ExtractOpts {
                scheme: Some("bmp-lsb-permuted"),
                density: Some(Density::Low),
                skip_header: false,
                raw_bit_count: None,
                seed: None,
            },
        )
        .unwrap_err();
    assert!(matches!(err, Error::PermutationSeedRequired), "got {err:?}");
}

/// The first 54 bytes (BMP header + DIB) must still be byte-identical — we
/// only touch pixel bytes.
#[test]
fn permuted_preserves_bmp_header_region() {
    let cover = make_bmp24(48, 48, 0x40);
    let body = b"preserve me".to_vec();
    let mut header = PayloadHeader::plain(SchemeFourcc::BMP_LSB_PERMUTED, Density::Low, &body);
    header.flags |= PayloadHeader::FLAG_PERMUTED;
    let framed = header.encode_with(&body);
    let stego = BMP_ADAPTER
        .embed(
            &cover,
            &framed,
            &EmbedOpts {
                scheme: Some("bmp-lsb-permuted"),
                density: Density::Low,
                seed: Some(9999),
            },
        )
        .unwrap();
    assert_eq!(&stego[..54], &cover[..54]);
}

/// Linear-produced stego must not decode as permuted (wrong order →
/// garbage magic) and vice versa. Makes sure an `rsteg extract --scheme …`
/// mismatch fails cleanly.
#[test]
fn linear_stego_fails_under_permuted_extract() {
    let cover = make_bmp24(64, 64, 0x40);
    let body = b"linear-produced".to_vec();
    let header = PayloadHeader::plain(SchemeFourcc::BMP_LSB_LINEAR, Density::Low, &body);
    let framed = header.encode_with(&body);
    let stego = BMP_ADAPTER
        .embed(
            &cover,
            &framed,
            &EmbedOpts {
                scheme: Some("bmp-lsb-linear"),
                density: Density::Low,
                seed: None,
            },
        )
        .unwrap();
    // Extract with the permuted scheme and a seed → should not recover the framed bytes.
    match BMP_ADAPTER.extract(
        &stego,
        &ExtractOpts {
            scheme: Some("bmp-lsb-permuted"),
            density: Some(Density::Low),
            skip_header: false,
            raw_bit_count: None,
            seed: Some(1),
        },
    ) {
        Ok(out) => assert_ne!(out, framed, "permuted must not recover linear-produced plaintext"),
        Err(_) => {}
    }
}
