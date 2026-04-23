//! First TDD red test per specs/09-roadmap.md.
//!
//! A round-trip embed/extract with an empty payload (header-only) against a
//! minimal valid 24-bit BMP must produce `framed == extracted`.

use rsteg_bmp::BMP_ADAPTER;
use rsteg_core::{
    Density, EmbedOpts, ExtractOpts, FormatAdapter, PayloadHeader, SchemeFourcc,
};

/// Generate a minimal uncompressed 24-bit BMP of the given dimensions with a
/// pixel payload of the given byte (row-padded to 4-byte boundaries).
fn make_bmp24(width: u32, height: u32, fill: u8) -> Vec<u8> {
    let row_bytes = (width as usize) * 3;
    let stride = (row_bytes + 3) & !3; // 4-byte aligned row
    let pixel_bytes = stride * (height as usize);
    let file_size = 54 + pixel_bytes;

    let mut out = Vec::with_capacity(file_size);
    // BITMAPFILEHEADER (14 bytes)
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&(file_size as u32).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // reserved
    out.extend_from_slice(&54u32.to_le_bytes()); // pixel offset

    // BITMAPINFOHEADER (40 bytes)
    out.extend_from_slice(&40u32.to_le_bytes()); // header size
    out.extend_from_slice(&(width as i32).to_le_bytes());
    out.extend_from_slice(&(height as i32).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // planes
    out.extend_from_slice(&24u16.to_le_bytes()); // bpp
    out.extend_from_slice(&0u32.to_le_bytes()); // BI_RGB
    out.extend_from_slice(&(pixel_bytes as u32).to_le_bytes());
    out.extend_from_slice(&2835i32.to_le_bytes()); // 72 DPI x
    out.extend_from_slice(&2835i32.to_le_bytes()); // 72 DPI y
    out.extend_from_slice(&0u32.to_le_bytes()); // palette colors
    out.extend_from_slice(&0u32.to_le_bytes()); // important colors

    // Pixel data: fill byte repeated; row-padding zeros.
    for _ in 0..height {
        for _ in 0..row_bytes {
            out.push(fill);
        }
        for _ in row_bytes..stride {
            out.push(0);
        }
    }
    assert_eq!(out.len(), file_size);
    out
}

#[test]
fn roundtrip_empty_payload_bmp24_linear_density_low() {
    // Cover: 64×64 solid-gray BMP — well above the 32-byte header capacity.
    let cover = make_bmp24(64, 64, 0x80);

    // Build a plaintext (unencrypted) framed payload with zero body bytes.
    let header = PayloadHeader::plain(SchemeFourcc::BMP_LSB_LINEAR, Density::Low, &[]);
    let framed = header.encode_with(&[]);
    assert_eq!(framed.len(), PayloadHeader::SIZE);

    let opts = EmbedOpts {
        scheme: Some("bmp-lsb-linear"),
        density: Density::Low,
    };
    let stego = BMP_ADAPTER
        .embed(&cover, &framed, &opts)
        .expect("embed should succeed");

    assert_eq!(stego.len(), cover.len(), "BMP output length must match cover");
    // Header + DIB header (54 bytes) must be byte-identical to cover.
    assert_eq!(&stego[..54], &cover[..54]);

    let extract_opts = ExtractOpts {
        scheme: Some("bmp-lsb-linear"),
        density: Some(Density::Low),
        skip_header: false,
        raw_bit_count: None,
    };
    let extracted = BMP_ADAPTER
        .extract(&stego, &extract_opts)
        .expect("extract should succeed");

    assert_eq!(
        extracted, framed,
        "extracted framed bytes must equal the embedded framed bytes"
    );
}
