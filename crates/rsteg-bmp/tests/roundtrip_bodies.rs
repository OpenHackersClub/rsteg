//! Body-bearing round-trips, capacity limits, density sweep, and tamper check.
//! Covers items 2, 3, 4, 5, 10 (plaintext variant), 15 of the minimum test set.

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
    for _ in 0..height {
        for _ in 0..row_bytes {
            out.push(fill);
        }
        for _ in row_bytes..stride {
            out.push(0);
        }
    }
    out
}

fn roundtrip_with(body: &[u8], density: Density) {
    let cover = make_bmp24(64, 64, 0x80);
    let header = PayloadHeader::plain(SchemeFourcc::BMP_LSB_LINEAR, density, body);
    let framed = header.encode_with(body);
    let opts = EmbedOpts {
        scheme: Some("bmp-lsb-linear"),
        density,
        seed: None,
    };
    let stego = BMP_ADAPTER.embed(&cover, &framed, &opts).unwrap();
    assert_eq!(stego.len(), cover.len());
    let ex = BMP_ADAPTER
        .extract(
            &stego,
            &ExtractOpts {
                scheme: Some("bmp-lsb-linear"),
                density: Some(density),
                skip_header: false,
                raw_bit_count: None,
                seed: None,
            },
        )
        .unwrap();
    assert_eq!(ex, framed);
}

#[test]
fn single_byte_payload_density_low() {
    roundtrip_with(&[0xA5], Density::Low);
}

#[test]
fn mid_size_payload_density_low() {
    let body: Vec<u8> = (0..=255u8).collect();
    roundtrip_with(&body, Density::Low);
}

#[test]
fn density_moderate_roundtrips() {
    let body = b"density-2 roundtrip at moderate".to_vec();
    roundtrip_with(&body, Density::Moderate);
}

#[test]
fn density_aggressive3_roundtrips() {
    let body = vec![0xDE; 300];
    roundtrip_with(&body, Density::Aggressive3);
}

#[test]
fn density_aggressive4_roundtrips() {
    let body = vec![0xEF; 600];
    roundtrip_with(&body, Density::Aggressive4);
}

#[test]
fn over_capacity_reports_payload_too_large() {
    // 4×4 BMP @ d=1 has only 48 pixel bytes = 6 bytes of raw capacity.
    // Header alone (32 bytes) exceeds that.
    let cover = make_bmp24(4, 4, 0);
    let header = PayloadHeader::plain(SchemeFourcc::BMP_LSB_LINEAR, Density::Low, &[]);
    let framed = header.encode_with(&[]);
    let opts = EmbedOpts {
        scheme: Some("bmp-lsb-linear"),
        density: Density::Low,
        seed: None,
    };
    let err = BMP_ADAPTER.embed(&cover, &framed, &opts).unwrap_err();
    assert!(matches!(err, Error::PayloadTooLarge { .. }), "got {err:?}");
}

#[test]
fn cross_format_refused() {
    // Feed a non-BMP blob (PNG signature) to the BMP adapter.
    let png_sig = b"\x89PNG\r\n\x1a\nand more junk";
    let err = BMP_ADAPTER.recognize(png_sig);
    assert!(!err);

    let framed = PayloadHeader::plain(SchemeFourcc::BMP_LSB_LINEAR, Density::Low, &[])
        .encode_with(&[]);
    let opts = EmbedOpts {
        scheme: Some("bmp-lsb-linear"),
        density: Density::Low,
        seed: None,
    };
    let err = BMP_ADAPTER.embed(png_sig, &framed, &opts).unwrap_err();
    assert!(matches!(err, Error::FormatUnrecognized));
}

#[test]
fn plaintext_tamper_trips_body_crc() {
    let cover = make_bmp24(64, 64, 0x7F);
    let body = b"tamper-with-me-please".to_vec();
    let header = PayloadHeader::plain(SchemeFourcc::BMP_LSB_LINEAR, Density::Low, &body);
    let framed = header.encode_with(&body);
    let opts = EmbedOpts {
        scheme: Some("bmp-lsb-linear"),
        density: Density::Low,
        seed: None,
    };
    let mut stego = BMP_ADAPTER.embed(&cover, &framed, &opts).unwrap();

    // Flip a bit in a pixel byte that lives *after* the header region.
    let pixel_start = 54;
    let target = pixel_start + (32 * 8) + 10;
    stego[target] ^= 0x01;

    let err = BMP_ADAPTER
        .extract(
            &stego,
            &ExtractOpts {
                scheme: Some("bmp-lsb-linear"),
                density: Some(Density::Low),
                skip_header: false,
                raw_bit_count: None,
                seed: None,
            },
        )
        .unwrap_err();
    assert!(matches!(err, Error::BodyCrcMismatch), "got {err:?}");
}

#[test]
fn out_of_band_region_preserved() {
    let cover = make_bmp24(32, 32, 0x40);
    let body = b"hello world".to_vec();
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
    // BITMAPFILEHEADER + DIB header (offset 0..54) are out-of-band.
    assert_eq!(&stego[..54], &cover[..54]);
}
