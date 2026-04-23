//! WAV (RIFF/PCM) format adapter for rsteg.
//!
//! V1 scope (per `specs/05-formats.md`):
//! - RIFF/WAVE, `fmt ` format code 1 (PCM).
//! - 16-bit signed LE, or 8-bit unsigned.
//! - 1 or 2 channels, 8 kHz–192 kHz.
//!
//! Embedding unit = one PCM value (spec terminology — *per channel*, not "frame").
//! For 16-bit LE, we LSB-modify the low byte of each pair; for 8-bit, the sample
//! byte itself. Chunks other than `fmt ` and `data` are preserved verbatim.

#![deny(unsafe_code)]

use rsteg_core::{
    prng::{shuffle, Prng},
    BitReader, BitWriter, Density, EmbedOpts, Error, ExtractOpts, FormatAdapter, PayloadHeader,
};

pub static WAV_ADAPTER: WavAdapter = WavAdapter;

#[derive(Debug, Default, Clone, Copy)]
pub struct WavAdapter;

/// Layout of the parts of a WAV file we care about.
#[derive(Debug)]
struct WavLayout {
    /// Byte offset of the first PCM sample inside the `data` chunk.
    data_offset: usize,
    /// Declared size of the `data` chunk in bytes.
    data_len: usize,
    /// Bits per sample: 8 or 16.
    bits_per_sample: u16,
}

fn parse_wav(bytes: &[u8]) -> Result<WavLayout, Error> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(Error::FormatUnrecognized);
    }

    // RIFF size field can be a lie (spec 05 §WAV). Don't trust it for bounds;
    // walk chunks against the actual buffer length instead.
    let mut cursor = 12usize;
    let mut fmt_offset: Option<usize> = None;
    let mut data_slot: Option<(usize, usize)> = None;

    while cursor + 8 <= bytes.len() {
        let id: [u8; 4] = bytes[cursor..cursor + 4].try_into().unwrap();
        let size = u32::from_le_bytes(bytes[cursor + 4..cursor + 8].try_into().unwrap()) as usize;
        let payload_start = cursor + 8;
        let payload_end = payload_start.checked_add(size).ok_or(Error::Malformed {
            at: "wav",
            detail: "chunk size overflows",
        })?;
        if payload_end > bytes.len() {
            // `data` may legitimately be truncated (spec notes streaming captures
            // leave size = 0xFFFFFFFF). For non-`data` chunks this is malformed.
            if &id == b"data" {
                // Fall back to "take what's actually present" so we don't trip on
                // truncated / oversized declarations.
                data_slot = Some((payload_start, bytes.len() - payload_start));
                break;
            }
            return Err(Error::Malformed {
                at: "wav",
                detail: "chunk extends past end of file",
            });
        }

        if &id == b"fmt " {
            fmt_offset = Some(payload_start);
        } else if &id == b"data" {
            data_slot = Some((payload_start, size));
        }

        // Chunks are word-aligned: odd-size chunks carry a 1-byte pad.
        let advance = size + (size & 1);
        cursor = payload_start
            .checked_add(advance)
            .ok_or(Error::Malformed {
                at: "wav",
                detail: "chunk advance overflows",
            })?;
    }

    let fmt_offset = fmt_offset.ok_or(Error::Malformed {
        at: "wav",
        detail: "no fmt  chunk found",
    })?;
    let (data_offset, data_len) = data_slot.ok_or(Error::Malformed {
        at: "wav",
        detail: "no data chunk found",
    })?;

    if fmt_offset + 16 > bytes.len() {
        return Err(Error::Malformed {
            at: "wav",
            detail: "fmt chunk truncated",
        });
    }
    let format_code = u16::from_le_bytes(bytes[fmt_offset..fmt_offset + 2].try_into().unwrap());
    let channels = u16::from_le_bytes(bytes[fmt_offset + 2..fmt_offset + 4].try_into().unwrap());
    let bits_per_sample =
        u16::from_le_bytes(bytes[fmt_offset + 14..fmt_offset + 16].try_into().unwrap());

    if format_code != 1 {
        return Err(Error::FormatUnsupported {
            id: "wav",
            reason: "only PCM (format code 1) is supported in phase 1",
        });
    }
    if !(1..=2).contains(&channels) {
        return Err(Error::FormatUnsupported {
            id: "wav",
            reason: "only mono or stereo WAV is supported in phase 1",
        });
    }
    if bits_per_sample != 8 && bits_per_sample != 16 {
        return Err(Error::FormatUnsupported {
            id: "wav",
            reason: "only 8-bit or 16-bit PCM is supported in phase 1",
        });
    }

    Ok(WavLayout {
        data_offset,
        data_len,
        bits_per_sample,
    })
}

/// Byte offsets within the file at which each embedding-unit byte lives.
///
/// For 16-bit LE samples, that's the low byte of each pair (even offsets inside
/// the `data` chunk). For 8-bit, every byte.
fn embedding_byte_indices(layout: &WavLayout) -> Vec<usize> {
    match layout.bits_per_sample {
        8 => (0..layout.data_len)
            .map(|i| layout.data_offset + i)
            .collect(),
        16 => (0..layout.data_len / 2)
            .map(|i| layout.data_offset + i * 2)
            .collect(),
        _ => unreachable!("parse_wav rejects other bit depths"),
    }
}

fn indices_for(layout: &WavLayout, permuted_seed: Option<u64>) -> Vec<usize> {
    let mut idxs = embedding_byte_indices(layout);
    if let Some(seed) = permuted_seed {
        let mut prng = Prng::new(seed);
        shuffle(&mut prng, &mut idxs);
    }
    idxs
}

fn embed_scheme(
    carrier: &[u8],
    framed: &[u8],
    density: Density,
    permuted_seed: Option<u64>,
    out: &mut Vec<u8>,
) -> Result<(), Error> {
    let layout = parse_wav(carrier)?;
    let idxs = indices_for(&layout, permuted_seed);

    let avail_bytes = (idxs.len() as u64 * u64::from(density.bits())) / 8;
    let needed_units = framed
        .len()
        .checked_mul(8)
        .and_then(|b| b.checked_add(usize::from(density.bits()) - 1))
        .map(|b| b / usize::from(density.bits()))
        .ok_or(Error::PayloadTooLarge {
            needed: u64::MAX,
            available: avail_bytes,
        })?;
    if needed_units > idxs.len() {
        return Err(Error::PayloadTooLarge {
            needed: framed.len() as u64,
            available: avail_bytes,
        });
    }

    out.clear();
    out.extend_from_slice(carrier);

    let mut scratch: Vec<u8> = idxs.iter().map(|&i| out[i]).collect();
    {
        let mut writer = BitWriter::new(&mut scratch, density.bits());
        writer.write(framed).map_err(|()| Error::PayloadTooLarge {
            needed: framed.len() as u64,
            available: avail_bytes,
        })?;
        writer.flush().map_err(|()| Error::PayloadTooLarge {
            needed: framed.len() as u64,
            available: avail_bytes,
        })?;
    }
    for (n, &idx) in idxs.iter().enumerate() {
        out[idx] = scratch[n];
    }
    debug_assert_eq!(out.len(), carrier.len());
    Ok(())
}

fn extract_scheme(
    stego: &[u8],
    density: Density,
    permuted_seed: Option<u64>,
    out: &mut Vec<u8>,
) -> Result<(), Error> {
    let layout = parse_wav(stego)?;
    let idxs = indices_for(&layout, permuted_seed);
    let scratch: Vec<u8> = idxs.iter().map(|&i| stego[i]).collect();

    if scratch.len() * usize::from(density.bits()) < PayloadHeader::SIZE * 8 {
        return Err(Error::PayloadTooLarge {
            needed: PayloadHeader::SIZE as u64,
            available: (scratch.len() as u64 * u64::from(density.bits())) / 8,
        });
    }

    let mut reader = BitReader::new(&scratch, density.bits());
    let mut header_bytes = [0u8; PayloadHeader::SIZE];
    reader
        .read(&mut header_bytes)
        .map_err(|()| Error::HeaderMissing)?;
    let header = PayloadHeader::decode(&header_bytes)?;

    let body_len = header.body_len as usize;
    if reader.bytes_available() < body_len {
        return Err(Error::PayloadTooLarge {
            needed: body_len as u64,
            available: reader.bytes_available() as u64,
        });
    }
    out.clear();
    out.extend_from_slice(&header_bytes);
    let body_start = out.len();
    out.resize(body_start + body_len, 0);
    if body_len > 0 {
        reader
            .read(&mut out[body_start..])
            .map_err(|()| Error::HeaderMissing)?;
    }

    if (header.flags & PayloadHeader::FLAG_ENCRYPTED) == 0 && body_len > 0 {
        let actual = rsteg_core::crc32_ieee(&out[body_start..]);
        if actual != header.body_crc32 {
            return Err(Error::BodyCrcMismatch);
        }
    }
    Ok(())
}

impl FormatAdapter for WavAdapter {
    fn id(&self) -> &'static str {
        "wav"
    }

    fn recognize(&self, bytes: &[u8]) -> bool {
        bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WAVE"
    }

    fn embed_into(
        &self,
        carrier: &[u8],
        framed: &[u8],
        opts: &EmbedOpts,
        out: &mut Vec<u8>,
    ) -> Result<(), Error> {
        match opts.scheme {
            None | Some("wav-lsb-linear") => embed_scheme(carrier, framed, opts.density, None, out),
            Some("wav-lsb-permuted") => {
                let seed = opts.seed.ok_or(Error::PermutationSeedRequired)?;
                embed_scheme(carrier, framed, opts.density, Some(seed), out)
            }
            Some(_) => Err(Error::FormatUnsupported {
                id: "wav",
                reason: "unknown scheme",
            }),
        }
    }

    fn extract_into(
        &self,
        stego: &[u8],
        opts: &ExtractOpts,
        out: &mut Vec<u8>,
    ) -> Result<(), Error> {
        let density = opts.density.unwrap_or(Density::Low);
        match opts.scheme {
            None | Some("wav-lsb-linear") => extract_scheme(stego, density, None, out),
            Some("wav-lsb-permuted") => {
                let seed = opts.seed.ok_or(Error::PermutationSeedRequired)?;
                extract_scheme(stego, density, Some(seed), out)
            }
            Some(_) => Err(Error::FormatUnsupported {
                id: "wav",
                reason: "unknown scheme",
            }),
        }
    }
}
