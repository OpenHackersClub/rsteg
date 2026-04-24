//! Deterministic corpus generators. Per spec 08 table §"Corpus", each case
//! has dimensions and a set of payload sizes. We stay on the smaller end
//! so an entire bench run completes in a few minutes locally.

use crate::Format;

pub struct Case {
    pub id: &'static str,
    pub ext: &'static str,
    pub format: Format,
    pub payload_sizes: &'static [usize],
    /// Seed-derived cover builder.
    build: fn() -> Vec<u8>,
}

impl Case {
    pub fn build_cover(&self) -> Vec<u8> {
        (self.build)()
    }
}

pub fn all_cases() -> Vec<Case> {
    vec![
        Case {
            id: "bmp-tiny",
            ext: "bmp",
            format: Format::Bmp,
            payload_sizes: &[16, 1024, 4096],
            build: || make_bmp24(128, 128),
        },
        Case {
            id: "bmp-small",
            ext: "bmp",
            format: Format::Bmp,
            payload_sizes: &[1024, 10_240, 65_536],
            build: || make_bmp24(512, 512),
        },
        Case {
            id: "wav-short",
            ext: "wav",
            format: Format::Wav,
            payload_sizes: &[1024, 20_480],
            build: || make_wav16(44_100, 5 * 44_100, 2),
        },
        Case {
            id: "png-synth-small",
            ext: "png",
            format: Format::Png,
            payload_sizes: &[1024, 10_240],
            build: || make_png_rgb(256, 256),
        },
    ]
}

fn make_bmp24(width: u32, height: u32) -> Vec<u8> {
    let row = (width as usize * 3 + 3) & !3;
    let pixel_bytes = row * height as usize;
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
    let mut s: u64 = 0x7257_E4;
    for _ in 0..height {
        for _ in 0..width {
            s = s.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let z = (s ^ (s >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            let b = z.to_le_bytes();
            out.push(b[0]);
            out.push(b[1]);
            out.push(b[2]);
        }
        for _ in (width * 3) as usize..row {
            out.push(0);
        }
    }
    out
}

fn make_wav16(sample_rate: u32, samples: u32, channels: u16) -> Vec<u8> {
    let bits_per_sample: u16 = 16;
    let byte_rate = sample_rate * u32::from(channels) * u32::from(bits_per_sample) / 8;
    let block_align = channels * bits_per_sample / 8;
    let data_bytes = samples * u32::from(block_align);
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
    let mut s: u64 = 0x7257_E4;
    for _ in 0..samples {
        for _ in 0..channels {
            s = s.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let z = (s ^ (s >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            let sample = (z as u16) as i16;
            out.extend_from_slice(&sample.to_le_bytes());
        }
    }
    out
}

fn make_png_rgb(width: u32, height: u32) -> Vec<u8> {
    use miniz_oxide::deflate::compress_to_vec_zlib;
    let mut s: u64 = 0x7257_E4;
    let mut raw = Vec::with_capacity((1 + width as usize * 3) * height as usize);
    for _ in 0..height {
        raw.push(0);
        for _ in 0..width {
            s = s.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let z = (s ^ (s >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            let b = z.to_le_bytes();
            raw.push(b[0]);
            raw.push(b[1]);
            raw.push(b[2]);
        }
    }
    let idat = compress_to_vec_zlib(&raw, 6);

    let mut out = Vec::new();
    out.extend_from_slice(b"\x89PNG\r\n\x1a\n");
    let mut chunk = |out: &mut Vec<u8>, ty: &[u8; 4], data: &[u8]| {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let start = out.len();
        out.extend_from_slice(ty);
        out.extend_from_slice(data);
        let crc = rsteg_core::crc32_ieee(&out[start..]);
        out.extend_from_slice(&crc.to_be_bytes());
    };
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.push(8);
    ihdr.push(2);
    ihdr.push(0);
    ihdr.push(0);
    ihdr.push(0);
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &idat);
    chunk(&mut out, b"IEND", &[]);
    out
}
