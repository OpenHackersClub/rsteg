//! Body-bearing round-trips, density sweep, capacity ceiling, tamper, cross-format
//! refusal, 8-bit path, and out-of-band chunk preservation. Covers items 2, 3, 4,
//! 5, 6, 10 (plaintext), 11, 12, 15, 18 (WAV half) of the minimum test set in
//! `specs/07-testing.md`.

use rsteg_core::{
    Density, EmbedOpts, Error, ExtractOpts, FormatAdapter, PayloadHeader, SchemeFourcc,
};
use rsteg_wav::WAV_ADAPTER;

fn make_wav16(sample_rate: u32, samples: u32, channels: u16, fill: i16) -> Vec<u8> {
    let bits_per_sample: u16 = 16;
    let byte_rate: u32 = sample_rate * u32::from(channels) * u32::from(bits_per_sample) / 8;
    let block_align: u16 = channels * bits_per_sample / 8;
    let data_bytes: u32 = samples * u32::from(block_align);

    let mut out = Vec::with_capacity(44 + data_bytes as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36u32 + data_bytes).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&block_align.to_le_bytes());
    out.extend_from_slice(&bits_per_sample.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_bytes.to_le_bytes());
    for _ in 0..(samples * u32::from(channels)) {
        out.extend_from_slice(&fill.to_le_bytes());
    }
    out
}

fn make_wav8_mono(sample_rate: u32, samples: u32, fill: u8) -> Vec<u8> {
    let bits_per_sample: u16 = 8;
    let channels: u16 = 1;
    let byte_rate: u32 = sample_rate * u32::from(channels) * u32::from(bits_per_sample) / 8;
    let block_align: u16 = channels * bits_per_sample / 8;
    let data_bytes: u32 = samples * u32::from(block_align);

    let mut out = Vec::with_capacity(44 + data_bytes as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36u32 + data_bytes).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&block_align.to_le_bytes());
    out.extend_from_slice(&bits_per_sample.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_bytes.to_le_bytes());
    for _ in 0..samples {
        out.push(fill);
    }
    out
}

fn roundtrip_with(cover: &[u8], body: &[u8], density: Density) {
    let header = PayloadHeader::plain(SchemeFourcc::WAV_LSB_LINEAR, density, body);
    let framed = header.encode_with(body);
    let stego = WAV_ADAPTER
        .embed(
            cover,
            &framed,
            &EmbedOpts {
                scheme: Some("wav-lsb-linear"),
                density,
                seed: None,
            },
        )
        .unwrap();
    assert_eq!(stego.len(), cover.len());
    let extracted = WAV_ADAPTER
        .extract(
            &stego,
            &ExtractOpts {
                scheme: Some("wav-lsb-linear"),
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
fn single_byte_payload_density_low() {
    let cover = make_wav16(44_100, 2_048, 1, 0x0080);
    roundtrip_with(&cover, &[0xA5], Density::Low);
}

#[test]
fn mid_size_payload_density_low() {
    let cover = make_wav16(44_100, 4_096, 1, 0x0100);
    let body: Vec<u8> = (0..=255u8).collect();
    roundtrip_with(&cover, &body, Density::Low);
}

#[test]
fn stereo_carrier_roundtrips() {
    // Stereo doubles the embedding units — confirms the per-channel PCM-value
    // counting in embedding_byte_indices.
    let cover = make_wav16(44_100, 1_024, 2, 0x0040);
    roundtrip_with(&cover, b"stereo works", Density::Low);
}

#[test]
fn density_moderate_roundtrips() {
    let cover = make_wav16(44_100, 2_048, 1, 0x0200);
    roundtrip_with(&cover, b"density-2 wav roundtrip", Density::Moderate);
}

#[test]
fn density_aggressive3_roundtrips() {
    let cover = make_wav16(44_100, 4_096, 1, 0x0400);
    roundtrip_with(&cover, &vec![0xDEu8; 300], Density::Aggressive3);
}

#[test]
fn density_aggressive4_roundtrips() {
    let cover = make_wav16(44_100, 4_096, 1, 0x0800);
    roundtrip_with(&cover, &vec![0xEFu8; 600], Density::Aggressive4);
}

#[test]
fn eight_bit_mono_roundtrips() {
    // 8-bit: every sample byte is an embedding unit.
    let cover = make_wav8_mono(22_050, 2_048, 0x7F);
    roundtrip_with(&cover, b"8-bit unsigned mono", Density::Low);
}

#[test]
fn over_capacity_reports_payload_too_large() {
    // 16-bit mono with 16 samples → 16 embedding units at d=1 = 2 bytes raw.
    // The 32-byte header alone exceeds that.
    let cover = make_wav16(8_000, 16, 1, 0);
    let framed = PayloadHeader::plain(SchemeFourcc::WAV_LSB_LINEAR, Density::Low, &[])
        .encode_with(&[]);
    let err = WAV_ADAPTER
        .embed(
            &cover,
            &framed,
            &EmbedOpts {
                scheme: Some("wav-lsb-linear"),
                density: Density::Low,
                seed: None,
            },
        )
        .unwrap_err();
    assert!(matches!(err, Error::PayloadTooLarge { .. }), "got {err:?}");
}

#[test]
fn cross_format_refused() {
    // Feed a BMP signature to the WAV adapter.
    let bmp_like = b"BMsome-other-bytes-that-are-not-a-wav";
    assert!(!WAV_ADAPTER.recognize(bmp_like));
    let framed = PayloadHeader::plain(SchemeFourcc::WAV_LSB_LINEAR, Density::Low, &[])
        .encode_with(&[]);
    let err = WAV_ADAPTER
        .embed(
            bmp_like,
            &framed,
            &EmbedOpts {
                scheme: Some("wav-lsb-linear"),
                density: Density::Low,
                seed: None,
            },
        )
        .unwrap_err();
    assert!(matches!(err, Error::FormatUnrecognized));
}

#[test]
fn plaintext_tamper_trips_body_crc() {
    let cover = make_wav16(44_100, 4_096, 1, 0x0100);
    let body = b"tamper-me".to_vec();
    let header = PayloadHeader::plain(SchemeFourcc::WAV_LSB_LINEAR, Density::Low, &body);
    let framed = header.encode_with(&body);
    let mut stego = WAV_ADAPTER
        .embed(
            &cover,
            &framed,
            &EmbedOpts {
                scheme: Some("wav-lsb-linear"),
                density: Density::Low,
                seed: None,
            },
        )
        .unwrap();

    // Flip a bit in the low byte of a PCM sample well past the header region.
    // data offset starts at 44; 32 header bytes × 8 bits × 2-bytes-per-sample = 512.
    let target = 44 + 600;
    stego[target] ^= 0x01;

    let err = WAV_ADAPTER
        .extract(
            &stego,
            &ExtractOpts {
                scheme: Some("wav-lsb-linear"),
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
fn ancillary_chunk_preserved_verbatim() {
    // Inject a LIST chunk between fmt and data — non-fmt/-data chunks must be
    // round-tripped byte-for-byte.
    let sample_rate = 22_050u32;
    let samples = 1_024u32;
    let bits_per_sample = 16u16;
    let channels = 1u16;
    let byte_rate = sample_rate * u32::from(channels) * u32::from(bits_per_sample) / 8;
    let block_align = channels * bits_per_sample / 8;
    let data_bytes = samples * u32::from(block_align);

    let list_payload: &[u8] = b"INFOIART\x04\x00\x00\x00me\x00\x00";
    let list_size = list_payload.len() as u32;
    let riff_size = 4 + (8 + 16) + (8 + list_size) + (8 + data_bytes);

    let mut cover = Vec::new();
    cover.extend_from_slice(b"RIFF");
    cover.extend_from_slice(&riff_size.to_le_bytes());
    cover.extend_from_slice(b"WAVE");
    cover.extend_from_slice(b"fmt ");
    cover.extend_from_slice(&16u32.to_le_bytes());
    cover.extend_from_slice(&1u16.to_le_bytes());
    cover.extend_from_slice(&channels.to_le_bytes());
    cover.extend_from_slice(&sample_rate.to_le_bytes());
    cover.extend_from_slice(&byte_rate.to_le_bytes());
    cover.extend_from_slice(&block_align.to_le_bytes());
    cover.extend_from_slice(&bits_per_sample.to_le_bytes());
    cover.extend_from_slice(b"LIST");
    cover.extend_from_slice(&list_size.to_le_bytes());
    cover.extend_from_slice(list_payload);
    cover.extend_from_slice(b"data");
    cover.extend_from_slice(&data_bytes.to_le_bytes());
    for _ in 0..samples {
        cover.extend_from_slice(&0x0100i16.to_le_bytes());
    }

    let list_chunk_start = 12 + 8 + 16;
    let list_chunk_end = list_chunk_start + 8 + list_payload.len();
    let list_before: Vec<u8> = cover[list_chunk_start..list_chunk_end].to_vec();

    let body = b"keep the LIST intact".to_vec();
    let header = PayloadHeader::plain(SchemeFourcc::WAV_LSB_LINEAR, Density::Low, &body);
    let framed = header.encode_with(&body);
    let stego = WAV_ADAPTER
        .embed(
            &cover,
            &framed,
            &EmbedOpts {
                scheme: Some("wav-lsb-linear"),
                density: Density::Low,
                seed: None,
            },
        )
        .unwrap();

    assert_eq!(
        &stego[list_chunk_start..list_chunk_end],
        list_before.as_slice(),
        "LIST chunk must be preserved byte-for-byte"
    );
    // fmt + RIFF + WAVE prefix also preserved.
    assert_eq!(&stego[..list_chunk_start], &cover[..list_chunk_start]);
}
