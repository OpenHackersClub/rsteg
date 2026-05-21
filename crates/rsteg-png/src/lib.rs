//! PNG format adapter for rsteg.
//!
//! V1 scope (per `specs/05-formats.md`):
//! - Color types 2 (RGB) and 6 (RGBA), bit depth 8, no Adam7.
//! - Filter bytes preserved per row (cover's original filter choice is
//!   reapplied on re-encode — avoids the "all filter-0" fingerprint).
//! - Multiple IDAT chunks concatenated on read; single IDAT on write.
//! - Ancillary chunks (tEXt, pHYs, etc.) preserved verbatim.

#![deny(unsafe_code)]

use rsteg_core::{
    prng::{shuffle, Prng},
    BitReader, BitWriter, Density, EmbedOpts, Error, ExtractOpts, FormatAdapter, PayloadHeader,
};

pub static PNG_ADAPTER: PngAdapter = PngAdapter;

#[derive(Debug, Default, Clone, Copy)]
pub struct PngAdapter;

const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1A, b'\n'];

/// Parsed view of an 8-bit non-interlaced RGB/RGBA PNG.
#[derive(Debug)]
struct PngState {
    width: u32,
    height: u32,
    color_type: u8, // 2=RGB, 6=RGBA
    /// Decompressed + defiltered raw pixel bytes (w*h*channels).
    pixels: Vec<u8>,
    /// Filter byte for each row (0..=4).
    row_filters: Vec<u8>,
    /// Chunks preceding IDAT, excluding IHDR. Stored as (type, data).
    pre_chunks: Vec<([u8; 4], Vec<u8>)>,
    /// Chunks following the final IDAT, excluding IEND. Stored as (type, data).
    post_chunks: Vec<([u8; 4], Vec<u8>)>,
}

impl PngState {
    fn channels(&self) -> usize {
        match self.color_type {
            2 => 3,
            6 => 4,
            _ => unreachable!(),
        }
    }
}

fn parse_png(bytes: &[u8]) -> Result<PngState, Error> {
    if bytes.len() < 8 || bytes[..8] != PNG_SIGNATURE {
        return Err(Error::FormatUnrecognized);
    }

    let mut cursor = 8usize;
    let mut ihdr: Option<[u8; 13]> = None;
    let mut idat_cat: Vec<u8> = Vec::new();
    let mut pre_chunks: Vec<([u8; 4], Vec<u8>)> = Vec::new();
    let mut post_chunks: Vec<([u8; 4], Vec<u8>)> = Vec::new();
    let mut seen_idat = false;
    let mut saw_iend = false;

    while cursor + 12 <= bytes.len() {
        let len = u32::from_be_bytes(bytes[cursor..cursor + 4].try_into().unwrap()) as usize;
        let ty: [u8; 4] = bytes[cursor + 4..cursor + 8].try_into().unwrap();
        let data_start = cursor + 8;
        let data_end = data_start.checked_add(len).ok_or(Error::Malformed {
            at: "png",
            detail: "chunk length overflow",
        })?;
        if data_end + 4 > bytes.len() {
            return Err(Error::Malformed {
                at: "png",
                detail: "chunk extends past end of file",
            });
        }
        let data = &bytes[data_start..data_end];
        // CRC is bytes[data_end..data_end+4]; we trust upstream for now.

        match &ty {
            b"IHDR" => {
                if ihdr.is_some() {
                    return Err(Error::Malformed {
                        at: "png",
                        detail: "duplicate IHDR",
                    });
                }
                if len != 13 {
                    return Err(Error::Malformed {
                        at: "png",
                        detail: "IHDR must be 13 bytes",
                    });
                }
                let mut h = [0u8; 13];
                h.copy_from_slice(data);
                ihdr = Some(h);
            }
            b"IDAT" => {
                idat_cat.extend_from_slice(data);
                seen_idat = true;
            }
            b"IEND" => {
                saw_iend = true;
                break;
            }
            _ => {
                if seen_idat {
                    post_chunks.push((ty, data.to_vec()));
                } else {
                    pre_chunks.push((ty, data.to_vec()));
                }
            }
        }

        cursor = data_end + 4;
    }

    if !saw_iend {
        return Err(Error::Malformed {
            at: "png",
            detail: "missing IEND",
        });
    }
    let ihdr = ihdr.ok_or(Error::Malformed {
        at: "png",
        detail: "missing IHDR",
    })?;
    if idat_cat.is_empty() {
        return Err(Error::Malformed {
            at: "png",
            detail: "missing IDAT",
        });
    }

    let width = u32::from_be_bytes(ihdr[0..4].try_into().unwrap());
    let height = u32::from_be_bytes(ihdr[4..8].try_into().unwrap());
    let bit_depth = ihdr[8];
    let color_type = ihdr[9];
    let compression = ihdr[10];
    let filter_method = ihdr[11];
    let interlace = ihdr[12];

    if bit_depth != 8 {
        return Err(Error::FormatUnsupported {
            id: "png",
            reason: "only 8-bit depth is supported in phase 1",
        });
    }
    if color_type != 2 && color_type != 6 {
        return Err(Error::FormatUnsupported {
            id: "png",
            reason: "only RGB (type 2) and RGBA (type 6) are supported in phase 1",
        });
    }
    if compression != 0 || filter_method != 0 {
        return Err(Error::Malformed {
            at: "png",
            detail: "non-zero compression or filter method",
        });
    }
    if interlace != 0 {
        return Err(Error::FormatUnsupported {
            id: "png",
            reason: "Adam7 interlacing not supported in phase 1",
        });
    }

    // Decompress IDAT via zlib.
    let raw =
        miniz_oxide::inflate::decompress_to_vec_zlib(&idat_cat).map_err(|_| Error::Malformed {
            at: "png",
            detail: "IDAT zlib inflate failed",
        })?;

    let channels = if color_type == 2 { 3 } else { 4 } as usize;
    let stride = width as usize * channels;
    let expected_len = (1 + stride) * height as usize;
    if raw.len() != expected_len {
        return Err(Error::Malformed {
            at: "png",
            detail: "decompressed length does not match header",
        });
    }

    let (pixels, row_filters) = defilter(&raw, width, height, channels)?;

    Ok(PngState {
        width,
        height,
        color_type,
        pixels,
        row_filters,
        pre_chunks,
        post_chunks,
    })
}

/// Undo per-row PNG filtering; return the raw pixel bytes (w*h*channels)
/// and the original filter byte for each row.
fn defilter(
    raw: &[u8],
    width: u32,
    height: u32,
    channels: usize,
) -> Result<(Vec<u8>, Vec<u8>), Error> {
    let stride = width as usize * channels;
    let mut filters = Vec::with_capacity(height as usize);
    let mut pixels = vec![0u8; stride * height as usize];

    for y in 0..height as usize {
        let raw_row = &raw[y * (stride + 1)..(y + 1) * (stride + 1)];
        let filter = raw_row[0];
        let src = &raw_row[1..];
        let (prev_row, curr_row) = pixels.split_at_mut(y * stride);
        let prev_row: &[u8] = if y == 0 {
            &[]
        } else {
            &prev_row[(y - 1) * stride..y * stride]
        };
        let row = &mut curr_row[..stride];
        match filter {
            0 => row.copy_from_slice(src),
            1 => {
                // Sub: Recon = Filt + Recon(a)
                for i in 0..stride {
                    let a = if i >= channels { row[i - channels] } else { 0 };
                    row[i] = src[i].wrapping_add(a);
                }
            }
            2 => {
                // Up: Recon = Filt + Recon(b)
                for i in 0..stride {
                    let b = if prev_row.is_empty() { 0 } else { prev_row[i] };
                    row[i] = src[i].wrapping_add(b);
                }
            }
            3 => {
                // Average: Recon = Filt + floor((Recon(a) + Recon(b)) / 2)
                for i in 0..stride {
                    let a = if i >= channels { row[i - channels] } else { 0 };
                    let b = if prev_row.is_empty() { 0 } else { prev_row[i] };
                    row[i] = src[i].wrapping_add(((u16::from(a) + u16::from(b)) / 2) as u8);
                }
            }
            4 => {
                // Paeth
                for i in 0..stride {
                    let a = if i >= channels { row[i - channels] } else { 0 };
                    let b = if prev_row.is_empty() { 0 } else { prev_row[i] };
                    let c = if prev_row.is_empty() || i < channels {
                        0
                    } else {
                        prev_row[i - channels]
                    };
                    row[i] = src[i].wrapping_add(paeth_predictor(a, b, c));
                }
            }
            _ => {
                return Err(Error::Malformed {
                    at: "png",
                    detail: "unknown row filter",
                })
            }
        }
        filters.push(filter);
    }
    Ok((pixels, filters))
}

fn paeth_predictor(a: u8, b: u8, c: u8) -> u8 {
    let p = i32::from(a) + i32::from(b) - i32::from(c);
    let pa = (p - i32::from(a)).unsigned_abs();
    let pb = (p - i32::from(b)).unsigned_abs();
    let pc = (p - i32::from(c)).unsigned_abs();
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

/// Reapply per-row filtering using the stored filter choices. Counterpart of
/// `defilter`. Output layout: `[filter, row_bytes, filter, row_bytes, ...]`.
fn refilter(pixels: &[u8], width: u32, height: u32, channels: usize, filters: &[u8]) -> Vec<u8> {
    let stride = width as usize * channels;
    let mut out = Vec::with_capacity((1 + stride) * height as usize);

    for y in 0..height as usize {
        let curr = &pixels[y * stride..(y + 1) * stride];
        let prev: &[u8] = if y == 0 {
            &[]
        } else {
            &pixels[(y - 1) * stride..y * stride]
        };
        let filter = filters.get(y).copied().unwrap_or(0);
        out.push(filter);
        match filter {
            0 => out.extend_from_slice(curr),
            1 => {
                for i in 0..stride {
                    let a = if i >= channels { curr[i - channels] } else { 0 };
                    out.push(curr[i].wrapping_sub(a));
                }
            }
            2 => {
                for i in 0..stride {
                    let b = if prev.is_empty() { 0 } else { prev[i] };
                    out.push(curr[i].wrapping_sub(b));
                }
            }
            3 => {
                for i in 0..stride {
                    let a = if i >= channels { curr[i - channels] } else { 0 };
                    let b = if prev.is_empty() { 0 } else { prev[i] };
                    out.push(curr[i].wrapping_sub(((u16::from(a) + u16::from(b)) / 2) as u8));
                }
            }
            4 => {
                for i in 0..stride {
                    let a = if i >= channels { curr[i - channels] } else { 0 };
                    let b = if prev.is_empty() { 0 } else { prev[i] };
                    let c = if prev.is_empty() || i < channels {
                        0
                    } else {
                        prev[i - channels]
                    };
                    out.push(curr[i].wrapping_sub(paeth_predictor(a, b, c)));
                }
            }
            _ => unreachable!("defilter would have rejected"),
        }
    }
    out
}

/// Re-encode a parsed PngState back to PNG bytes.
fn encode_png(state: &PngState) -> Vec<u8> {
    let channels = state.channels();
    let raw = refilter(
        &state.pixels,
        state.width,
        state.height,
        channels,
        &state.row_filters,
    );
    let idat_data = miniz_oxide::deflate::compress_to_vec_zlib(&raw, 6);

    let mut out = Vec::with_capacity(PNG_SIGNATURE.len() + idat_data.len() + 64);
    out.extend_from_slice(&PNG_SIGNATURE);

    // IHDR
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&state.width.to_be_bytes());
    ihdr.extend_from_slice(&state.height.to_be_bytes());
    ihdr.push(8);
    ihdr.push(state.color_type);
    ihdr.push(0);
    ihdr.push(0);
    ihdr.push(0);
    write_chunk(&mut out, *b"IHDR", &ihdr);

    for (ty, data) in &state.pre_chunks {
        write_chunk(&mut out, *ty, data);
    }

    write_chunk(&mut out, *b"IDAT", &idat_data);

    for (ty, data) in &state.post_chunks {
        write_chunk(&mut out, *ty, data);
    }

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

/// Every pixel byte is an embedding unit. No alpha-uniform skip yet
/// (phase 1.5 / spec 05 "Uniform-alpha handling").
fn embedding_indices(state: &PngState) -> Vec<usize> {
    (0..state.pixels.len()).collect()
}

/// Build the embedding-unit index list; apply the PRNG shuffle for permuted.
///
/// Mirrors the `indices_for` helper in `rsteg-bmp` / `rsteg-wav`. Same seed
/// → same permutation, so the reader reproduces the writer's order exactly.
fn indices_for(state: &PngState, permuted_seed: Option<u64>) -> Vec<usize> {
    let mut idxs = embedding_indices(state);
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
    let mut state = parse_png(carrier)?;
    let idxs = indices_for(&state, permuted_seed);

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

    // Scratch — the pixel bytes in embedding order (file order for linear,
    // shuffled for permuted). The BitWriter then fills them linearly.
    let mut scratch: Vec<u8> = idxs.iter().map(|&i| state.pixels[i]).collect();
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
        state.pixels[idx] = scratch[n];
    }

    let encoded = encode_png(&state);
    out.clear();
    out.extend_from_slice(&encoded);
    Ok(())
}

fn extract_scheme(
    stego: &[u8],
    density: Density,
    permuted_seed: Option<u64>,
    out: &mut Vec<u8>,
) -> Result<(), Error> {
    let state = parse_png(stego)?;
    let idxs = indices_for(&state, permuted_seed);
    let scratch: Vec<u8> = idxs.iter().map(|&i| state.pixels[i]).collect();

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

impl FormatAdapter for PngAdapter {
    fn id(&self) -> &'static str {
        "png"
    }

    fn recognize(&self, bytes: &[u8]) -> bool {
        bytes.len() >= 8 && bytes[..8] == PNG_SIGNATURE
    }

    fn embed_into(
        &self,
        carrier: &[u8],
        framed: &[u8],
        opts: &EmbedOpts,
        out: &mut Vec<u8>,
    ) -> Result<(), Error> {
        match opts.scheme {
            None | Some("png-lsb-linear") => embed_scheme(carrier, framed, opts.density, None, out),
            Some("png-lsb-permuted") => {
                let seed = opts.seed.ok_or(Error::PermutationSeedRequired)?;
                embed_scheme(carrier, framed, opts.density, Some(seed), out)
            }
            Some(_) => Err(Error::FormatUnsupported {
                id: "png",
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
            None | Some("png-lsb-linear") => extract_scheme(stego, density, None, out),
            Some("png-lsb-permuted") => {
                let seed = opts.seed.ok_or(Error::PermutationSeedRequired)?;
                extract_scheme(stego, density, Some(seed), out)
            }
            Some(_) => Err(Error::FormatUnsupported {
                id: "png",
                reason: "unknown scheme",
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{paeth_predictor, PNG_ADAPTER};
    use rsteg_core::FormatAdapter;

    #[test]
    fn paeth_predictor_matches_spec() {
        // a=1, b=2, c=0: p=3, pa=2, pb=1, pc=3 → pick b=2.
        assert_eq!(paeth_predictor(1, 2, 0), 2);
        // a=b=c=5: p=5, pa=pb=pc=0 → pick a (first tie).
        assert_eq!(paeth_predictor(5, 5, 5), 5);
        assert_eq!(paeth_predictor(0, 0, 0), 0);
        assert_eq!(paeth_predictor(255, 255, 255), 255);
    }

    #[test]
    fn recognize_rejects_non_png() {
        assert!(!PNG_ADAPTER.recognize(b"BM......"));
        assert!(!PNG_ADAPTER.recognize(b"RIFF1234WAVE"));
        assert!(!PNG_ADAPTER.recognize(&[]));
    }
}
