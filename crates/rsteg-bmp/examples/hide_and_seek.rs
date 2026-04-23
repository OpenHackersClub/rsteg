//! Demonstrates the current `rsteg-bmp` + `rsteg-core` library API.
//!
//! Run with: `cargo run -p rsteg-bmp --example hide_and_seek`
//!
//! - Generates a 256×256 24-bit BMP cover in memory.
//! - Hides a secret message at density 1 (linear LSB).
//! - Extracts it back and verifies round-trip + CRC.
//! - Shows byte-level diff between cover and stego, and the impact of
//!   embedding at higher densities.

use rsteg_bmp::BMP_ADAPTER;
use rsteg_core::{
    Density, EmbedOpts, ExtractOpts, FormatAdapter, PayloadHeader, SchemeFourcc,
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
        for x in 0..(width * 3) {
            // soft gradient so LSB diffs are visible as a pattern, not noise
            out.push(fill.wrapping_add((x ^ y) as u8));
        }
        for _ in (width * 3) as usize..stride {
            out.push(0);
        }
    }
    out
}

fn diff_count(a: &[u8], b: &[u8]) -> usize {
    a.iter().zip(b).filter(|(x, y)| x != y).count()
}

fn run(density: Density, secret: &[u8]) {
    let cover = make_bmp24(256, 256, 0x40);
    let cover_pixel_bytes = cover.len() - 54;
    let header = PayloadHeader::plain(SchemeFourcc::BMP_LSB_LINEAR, density, secret);
    let framed = header.encode_with(secret);
    let opts = EmbedOpts {
        scheme: Some("bmp-lsb-linear"),
        density,
        seed: None,
    };

    let stego = BMP_ADAPTER.embed(&cover, &framed, &opts).unwrap();
    let ex_opts = ExtractOpts {
        scheme: Some("bmp-lsb-linear"),
        density: Some(density),
        skip_header: false,
        raw_bit_count: None,
        seed: None,
    };
    let extracted = BMP_ADAPTER.extract(&stego, &ex_opts).unwrap();

    assert_eq!(extracted, framed, "round-trip broke");

    let total_bytes_touched = diff_count(&cover, &stego);
    let pct = (total_bytes_touched as f64) / (cover_pixel_bytes as f64) * 100.0;
    println!(
        "  density={}  framed={:>3}B  diff={:>5} bytes ({:5.2}% of pixels)",
        density.bits(),
        framed.len(),
        total_bytes_touched,
        pct
    );
}

fn main() {
    let secret = b"the mitochondria is the powerhouse of the cell";

    println!("rsteg-bmp hide-and-seek demo");
    println!("  cover: 256x256 24-bit BMP (196,662 bytes total, 196,608 pixel bytes)");
    println!("  secret: {:?} ({} bytes)", std::str::from_utf8(secret).unwrap(), secret.len());
    println!();
    println!("Embedding the secret + 32-byte header at each density:");

    for d in [Density::Low, Density::Moderate, Density::Aggressive3, Density::Aggressive4] {
        run(d, secret);
    }

    println!();
    println!("Tamper check — flip one post-header pixel byte and re-extract:");

    let cover = make_bmp24(256, 256, 0x40);
    let header = PayloadHeader::plain(SchemeFourcc::BMP_LSB_LINEAR, Density::Low, secret);
    let framed = header.encode_with(secret);
    let opts = EmbedOpts {
        scheme: Some("bmp-lsb-linear"),
        density: Density::Low,
        seed: None,
    };
    let mut stego = BMP_ADAPTER.embed(&cover, &framed, &opts).unwrap();
    // At density 1, one pixel byte = one embedding unit.
    // Header occupies units 0..256; body (46 B = 368 bits) occupies units 256..624.
    // Flip LSB of unit 400, which is inside the body region.
    stego[54 + 400] ^= 0x01;
    let ex_opts = ExtractOpts {
        scheme: Some("bmp-lsb-linear"),
        density: Some(Density::Low),
        skip_header: false,
        raw_bit_count: None,
        seed: None,
    };
    match BMP_ADAPTER.extract(&stego, &ex_opts) {
        Ok(_) => println!("  UNEXPECTED: extract succeeded despite tampering"),
        Err(e) => println!("  extract rejected: {e}"),
    }
}
