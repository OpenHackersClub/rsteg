//! First TDD red test for WAV (spec 09 step 6, roadmap phase 1).
//!
//! Round-trip an empty (header-only) payload through a minimal PCM 16-bit mono WAV.

use rsteg_core::{Density, EmbedOpts, ExtractOpts, FormatAdapter, PayloadHeader, SchemeFourcc};
use rsteg_wav::WAV_ADAPTER;

/// Build a minimal uncompressed PCM WAV (RIFF/WAVE) with `samples` 16-bit signed
/// little-endian mono samples set to `fill`.
fn make_wav16_mono(sample_rate: u32, samples: u32, fill: i16) -> Vec<u8> {
    let bits_per_sample: u16 = 16;
    let channels: u16 = 1;
    let byte_rate: u32 = sample_rate * u32::from(channels) * u32::from(bits_per_sample) / 8;
    let block_align: u16 = channels * bits_per_sample / 8;
    let data_bytes: u32 = samples * u32::from(block_align);

    let mut out = Vec::with_capacity(44 + data_bytes as usize);
    // RIFF header
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36u32 + data_bytes).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    // fmt chunk
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes()); // PCM fmt chunk size
    out.extend_from_slice(&1u16.to_le_bytes()); // format = PCM
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&block_align.to_le_bytes());
    out.extend_from_slice(&bits_per_sample.to_le_bytes());
    // data chunk
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_bytes.to_le_bytes());
    for _ in 0..samples {
        out.extend_from_slice(&fill.to_le_bytes());
    }
    out
}

#[test]
fn roundtrip_empty_payload_wav16_mono_linear_density_low() {
    let cover = make_wav16_mono(44_100, 2_048, 0x0100);

    let header = PayloadHeader::plain(SchemeFourcc::WAV_LSB_LINEAR, Density::Low, &[]);
    let framed = header.encode_with(&[]);
    assert_eq!(framed.len(), PayloadHeader::SIZE);

    let opts = EmbedOpts {
        scheme: Some("wav-lsb-linear"),
        density: Density::Low,
    };
    let stego = WAV_ADAPTER
        .embed(&cover, &framed, &opts)
        .expect("embed should succeed");

    assert_eq!(
        stego.len(),
        cover.len(),
        "WAV output length must match cover"
    );
    // RIFF + fmt header (first 44 bytes) is out-of-band — must be preserved.
    assert_eq!(&stego[..44], &cover[..44]);

    let extracted = WAV_ADAPTER
        .extract(
            &stego,
            &ExtractOpts {
                scheme: Some("wav-lsb-linear"),
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
