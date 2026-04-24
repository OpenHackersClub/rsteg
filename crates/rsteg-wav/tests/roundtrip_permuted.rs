//! Permuted-scheme round-trips for WAV.

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

fn roundtrip_permuted(body: &[u8], density: Density, seed: u64) {
    let cover = make_wav16(44_100, 2_048, 1, 0x0100);
    let mut header = PayloadHeader::plain(SchemeFourcc::WAV_LSB_PERMUTED, density, body);
    header.flags |= PayloadHeader::FLAG_PERMUTED;
    let framed = header.encode_with(body);

    let stego = WAV_ADAPTER
        .embed(
            &cover,
            &framed,
            &EmbedOpts {
                scheme: Some("wav-lsb-permuted"),
                density,
                seed: Some(seed),
            },
        )
        .unwrap();
    assert_eq!(stego.len(), cover.len());

    let extracted = WAV_ADAPTER
        .extract(
            &stego,
            &ExtractOpts {
                scheme: Some("wav-lsb-permuted"),
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
    roundtrip_permuted(&[], Density::Low, 0xDEAD_BEEF);
}

#[test]
fn single_byte_permuted_roundtrips() {
    roundtrip_permuted(&[0xA5], Density::Low, 42);
}

#[test]
fn mid_size_body_permuted_density_moderate() {
    let body: Vec<u8> = (0..=199u8).collect();
    roundtrip_permuted(&body, Density::Moderate, 0xCAFE_F00D);
}

#[test]
fn wrong_seed_does_not_recover_plaintext_wav() {
    let body = b"audio secret".to_vec();
    let cover = make_wav16(44_100, 2_048, 2, 0x0080);
    let mut header = PayloadHeader::plain(SchemeFourcc::WAV_LSB_PERMUTED, Density::Low, &body);
    header.flags |= PayloadHeader::FLAG_PERMUTED;
    let framed = header.encode_with(&body);

    let stego = WAV_ADAPTER
        .embed(
            &cover,
            &framed,
            &EmbedOpts {
                scheme: Some("wav-lsb-permuted"),
                density: Density::Low,
                seed: Some(0xAAAA),
            },
        )
        .unwrap();

    match WAV_ADAPTER.extract(
        &stego,
        &ExtractOpts {
            scheme: Some("wav-lsb-permuted"),
            density: Some(Density::Low),
            skip_header: false,
            raw_bit_count: None,
            seed: Some(0xBBBB),
        },
    ) {
        Ok(out) => assert_ne!(out, framed, "wrong seed must not recover plaintext"),
        Err(_) => {}
    }
}

#[test]
fn permuted_without_seed_errors_wav() {
    let cover = make_wav16(44_100, 2_048, 1, 0x0010);
    let framed = PayloadHeader::plain(SchemeFourcc::WAV_LSB_PERMUTED, Density::Low, &[])
        .encode_with(&[]);
    let err = WAV_ADAPTER
        .embed(
            &cover,
            &framed,
            &EmbedOpts {
                scheme: Some("wav-lsb-permuted"),
                density: Density::Low,
                seed: None,
            },
        )
        .unwrap_err();
    assert!(matches!(err, Error::PermutationSeedRequired), "got {err:?}");
}
