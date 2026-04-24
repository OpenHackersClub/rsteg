//! BMP format adapter for rsteg.
//!
//! V1 scope: 24-bit uncompressed (BI_RGB) with BITMAPINFOHEADER (size 40),
//! linear LSB embedding. See `specs/05-formats.md`.

#![deny(unsafe_code)]

use rsteg_core::{
    prng::{shuffle, Prng},
    BitReader, BitWriter, Density, EmbedOpts, Error, ExtractOpts, FormatAdapter, PayloadHeader,
};

pub static BMP_ADAPTER: BmpAdapter = BmpAdapter;

#[derive(Debug, Default, Clone, Copy)]
pub struct BmpAdapter;

/// Location of the pixel array inside a BMP file.
#[derive(Debug)]
struct BmpLayout {
    /// Byte offset of the pixel array.
    pixel_offset: usize,
    /// Byte length of the pixel array.
    pixel_len: usize,
    /// Width in pixels (from DIB header).
    _width: u32,
    /// Height in pixels, absolute value of the signed DIB field.
    _height: u32,
    /// Bits per pixel.
    _bpp: u16,
}

fn parse_bmp(bytes: &[u8]) -> Result<BmpLayout, Error> {
    if bytes.len() < 54 || &bytes[0..2] != b"BM" {
        return Err(Error::FormatUnrecognized);
    }
    let pixel_offset = u32::from_le_bytes(bytes[10..14].try_into().unwrap()) as usize;
    let dib_size = u32::from_le_bytes(bytes[14..18].try_into().unwrap());
    if dib_size != 40 {
        return Err(Error::FormatUnsupported {
            id: "bmp",
            reason: "only BITMAPINFOHEADER (size 40) is supported in v1",
        });
    }
    let width = u32::from_le_bytes(bytes[18..22].try_into().unwrap());
    let height_signed = i32::from_le_bytes(bytes[22..26].try_into().unwrap());
    let height = height_signed.unsigned_abs();
    let bpp = u16::from_le_bytes(bytes[28..30].try_into().unwrap());
    let compression = u32::from_le_bytes(bytes[30..34].try_into().unwrap());

    if bpp != 24 {
        return Err(Error::FormatUnsupported {
            id: "bmp",
            reason: "only 24-bit BMP is supported in phase 1",
        });
    }
    if compression != 0 {
        return Err(Error::FormatUnsupported {
            id: "bmp",
            reason: "only uncompressed BI_RGB is supported in phase 1",
        });
    }

    let row_bytes = (width as usize) * 3;
    let stride = (row_bytes + 3) & !3;
    let pixel_len = stride * (height as usize);

    if pixel_offset
        .checked_add(pixel_len)
        .is_none_or(|end| end > bytes.len())
    {
        return Err(Error::Malformed {
            at: "bmp",
            detail: "pixel data extends past end of file",
        });
    }

    Ok(BmpLayout {
        pixel_offset,
        pixel_len,
        _width: width,
        _height: height,
        _bpp: bpp,
    })
}

/// For a 24-bit BMP with `width` columns and a given row `stride`, return the
/// list of byte indices inside the pixel region that are *pixel* bytes (R/G/B),
/// skipping the row-padding bytes at the end of each row.
fn pixel_byte_indices(width: u32, height: u32, stride: usize) -> Vec<usize> {
    let row_bytes = (width as usize) * 3;
    let mut out = Vec::with_capacity(row_bytes * (height as usize));
    for row in 0..(height as usize) {
        let base = row * stride;
        for col in 0..row_bytes {
            out.push(base + col);
        }
    }
    out
}

/// Build the list of pixel-byte offsets; apply the PRNG shuffle for permuted.
fn indices_for(
    width: u32,
    height: u32,
    stride: usize,
    permuted_seed: Option<u64>,
) -> Vec<usize> {
    let mut idxs = pixel_byte_indices(width, height, stride);
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
    let layout = parse_bmp(carrier)?;
    let width = u32::from_le_bytes(carrier[18..22].try_into().unwrap());
    let height_signed = i32::from_le_bytes(carrier[22..26].try_into().unwrap());
    let height = height_signed.unsigned_abs();
    let row_bytes = (width as usize) * 3;
    let stride = (row_bytes + 3) & !3;

    let pixel_slice_start = layout.pixel_offset;

    let idxs = indices_for(width, height, stride, permuted_seed);
    let needed_units = framed
        .len()
        .checked_mul(8)
        .and_then(|b| b.checked_add(usize::from(density.bits()) - 1))
        .map(|b| b / usize::from(density.bits()))
        .ok_or(Error::PayloadTooLarge {
            needed: u64::MAX,
            available: idxs.len() as u64,
        })?;
    if needed_units > idxs.len() {
        let available_bytes = ((idxs.len() as u64) * u64::from(density.bits())) / 8;
        return Err(Error::PayloadTooLarge {
            needed: framed.len() as u64,
            available: available_bytes,
        });
    }

    out.clear();
    out.extend_from_slice(carrier);

    // Scratch collects the pixel bytes at positions idxs[0], idxs[1], ... in
    // that order, then the BitWriter fills them linearly. For the linear
    // scheme idxs is already in file order; for permuted it's a shuffle of
    // the same list.
    let mut scratch: Vec<u8> = idxs
        .iter()
        .map(|&i| out[pixel_slice_start + i])
        .collect();

    let avail_bytes = (scratch.len() as u64 * u64::from(density.bits())) / 8;
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
        out[pixel_slice_start + idx] = scratch[n];
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
    let layout = parse_bmp(stego)?;
    let width = u32::from_le_bytes(stego[18..22].try_into().unwrap());
    let height_signed = i32::from_le_bytes(stego[22..26].try_into().unwrap());
    let height = height_signed.unsigned_abs();
    let row_bytes = (width as usize) * 3;
    let stride = (row_bytes + 3) & !3;

    let idxs = indices_for(width, height, stride, permuted_seed);
    let scratch: Vec<u8> = idxs
        .iter()
        .map(|&i| stego[layout.pixel_offset + i])
        .collect();

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

impl FormatAdapter for BmpAdapter {
    fn id(&self) -> &'static str {
        "bmp"
    }

    fn recognize(&self, bytes: &[u8]) -> bool {
        bytes.len() >= 2 && &bytes[0..2] == b"BM"
    }

    fn embed_into(
        &self,
        carrier: &[u8],
        framed: &[u8],
        opts: &EmbedOpts,
        out: &mut Vec<u8>,
    ) -> Result<(), Error> {
        match opts.scheme {
            None | Some("bmp-lsb-linear") => embed_scheme(carrier, framed, opts.density, None, out),
            Some("bmp-lsb-permuted") => {
                let seed = opts.seed.ok_or(Error::PermutationSeedRequired)?;
                embed_scheme(carrier, framed, opts.density, Some(seed), out)
            }
            Some(_) => Err(Error::FormatUnsupported {
                id: "bmp",
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
            None | Some("bmp-lsb-linear") => extract_scheme(stego, density, None, out),
            Some("bmp-lsb-permuted") => {
                let seed = opts.seed.ok_or(Error::PermutationSeedRequired)?;
                extract_scheme(stego, density, Some(seed), out)
            }
            Some(_) => Err(Error::FormatUnsupported {
                id: "bmp",
                reason: "unknown scheme",
            }),
        }
    }
}
