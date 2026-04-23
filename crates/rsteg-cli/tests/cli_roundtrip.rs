//! End-to-end CLI round-trip tests. We invoke the binary produced by
//! `cargo build -p rsteg-cli` via `std::process::Command` — ensures the
//! user-facing verbs actually work, not just the library APIs.

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

fn cli_path() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_BIN_EXE_rsteg"));
    // CARGO_BIN_EXE_<name> points at the binary produced for this crate.
    assert!(p.exists(), "rsteg binary missing at {p:?}");
    p.pop();
    p.push("rsteg");
    p
}

fn write_bmp(path: &std::path::Path, w: u32, h: u32) {
    let row = (w as usize * 3 + 3) & !3;
    let pixel_bytes = row * h as usize;
    let file_size = 54 + pixel_bytes;
    let mut out = Vec::new();
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&(file_size as u32).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&54u32.to_le_bytes());
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(w as i32).to_le_bytes());
    out.extend_from_slice(&(h as i32).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&24u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&(pixel_bytes as u32).to_le_bytes());
    out.extend_from_slice(&2835i32.to_le_bytes());
    out.extend_from_slice(&2835i32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    for y in 0..h {
        for x in 0..w {
            out.push(((x * 3 + y) & 0xFF) as u8);
            out.push(((x + y * 3) & 0xFF) as u8);
            out.push(((x * 7 + y * 5) & 0xFF) as u8);
        }
        for _ in (w * 3) as usize..row {
            out.push(0);
        }
    }
    fs::write(path, out).unwrap();
}

fn write_wav(path: &std::path::Path, samples: u32) {
    let bits_per_sample: u16 = 16;
    let channels: u16 = 1;
    let sample_rate: u32 = 22_050;
    let byte_rate = sample_rate * u32::from(channels) * u32::from(bits_per_sample) / 8;
    let block_align = channels * bits_per_sample / 8;
    let data_bytes = samples * u32::from(block_align);
    let mut out = Vec::new();
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
    for i in 0..samples {
        let s = ((i * 17) as i16).wrapping_add(0x0100);
        out.extend_from_slice(&s.to_le_bytes());
    }
    fs::write(path, out).unwrap();
}

fn write_png(path: &std::path::Path, w: u32, h: u32) {
    use miniz_oxide::deflate::compress_to_vec_zlib;
    let mut raw = Vec::new();
    for y in 0..h {
        raw.push(0);
        for x in 0..w {
            raw.push((x & 0xFF) as u8);
            raw.push((y & 0xFF) as u8);
            raw.push(((x + y) & 0xFF) as u8);
        }
    }
    let idat = compress_to_vec_zlib(&raw, 6);
    let mut out = Vec::new();
    out.extend_from_slice(b"\x89PNG\r\n\x1a\n");
    let mut push_chunk = |out: &mut Vec<u8>, ty: &[u8; 4], data: &[u8]| {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let s = out.len();
        out.extend_from_slice(ty);
        out.extend_from_slice(data);
        let crc = rsteg_core::crc32_ieee(&out[s..]);
        out.extend_from_slice(&crc.to_be_bytes());
    };
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.push(8);
    ihdr.push(2);
    ihdr.push(0);
    ihdr.push(0);
    ihdr.push(0);
    push_chunk(&mut out, b"IHDR", &ihdr);
    push_chunk(&mut out, b"IDAT", &idat);
    push_chunk(&mut out, b"IEND", &[]);
    fs::write(path, out).unwrap();
}

fn tmp_dir(name: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("rsteg-cli-test-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&p);
    fs::create_dir_all(&p).unwrap();
    p
}

fn check_roundtrip(cover_maker: fn(&std::path::Path), fmt_name: &str) {
    let d = tmp_dir(fmt_name);
    let cover = d.join(format!("cover.{fmt_name}"));
    let payload = d.join("secret.txt");
    let stego = d.join(format!("stego.{fmt_name}"));
    let recovered = d.join("recovered.txt");
    cover_maker(&cover);
    fs::write(&payload, b"roundtrip-via-CLI").unwrap();

    let embed = Command::new(cli_path())
        .args(["embed", "--in"])
        .arg(&cover)
        .arg("--payload")
        .arg(&payload)
        .arg("--out")
        .arg(&stego)
        .output()
        .expect("spawn rsteg embed");
    assert!(
        embed.status.success(),
        "embed failed ({:?}): {}",
        embed.status,
        String::from_utf8_lossy(&embed.stderr)
    );

    let extract = Command::new(cli_path())
        .args(["extract", "--in"])
        .arg(&stego)
        .arg("--out")
        .arg(&recovered)
        .output()
        .expect("spawn rsteg extract");
    assert!(
        extract.status.success(),
        "extract failed ({:?}): {}",
        extract.status,
        String::from_utf8_lossy(&extract.stderr)
    );

    let got = fs::read(&recovered).unwrap();
    assert_eq!(got, b"roundtrip-via-CLI");
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn cli_bmp_roundtrip_plaintext() {
    check_roundtrip(|p| write_bmp(p, 64, 64), "bmp");
}

#[test]
fn cli_wav_roundtrip_plaintext() {
    check_roundtrip(|p| write_wav(p, 4_096), "wav");
}

#[test]
fn cli_png_roundtrip_plaintext() {
    check_roundtrip(|p| write_png(p, 64, 64), "png");
}

#[test]
fn cli_bmp_roundtrip_encrypted() {
    let d = tmp_dir("bmp-enc");
    let cover = d.join("cover.bmp");
    let payload = d.join("secret.txt");
    let stego = d.join("stego.bmp");
    let recovered = d.join("recovered.txt");
    write_bmp(&cover, 128, 128);
    fs::write(&payload, b"encrypted-roundtrip-via-CLI").unwrap();

    let password = "hunter2";

    let mut embed = Command::new(cli_path())
        .args(["embed", "--in"])
        .arg(&cover)
        .arg("--payload")
        .arg(&payload)
        .arg("--out")
        .arg(&stego)
        .args(["--password", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    embed.stdin.as_mut().unwrap().write_all(password.as_bytes()).unwrap();
    let embed_out = embed.wait_with_output().unwrap();
    assert!(
        embed_out.status.success(),
        "embed failed: {}",
        String::from_utf8_lossy(&embed_out.stderr)
    );

    let mut extract = Command::new(cli_path())
        .args(["extract", "--in"])
        .arg(&stego)
        .arg("--out")
        .arg(&recovered)
        .args(["--password", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    extract.stdin.as_mut().unwrap().write_all(password.as_bytes()).unwrap();
    let extract_out = extract.wait_with_output().unwrap();
    assert!(
        extract_out.status.success(),
        "extract failed: {}",
        String::from_utf8_lossy(&extract_out.stderr)
    );

    let got = fs::read(&recovered).unwrap();
    assert_eq!(got, b"encrypted-roundtrip-via-CLI");
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn cli_wrong_password_fails() {
    let d = tmp_dir("bmp-bad-pw");
    let cover = d.join("cover.bmp");
    let payload = d.join("secret.txt");
    let stego = d.join("stego.bmp");
    let recovered = d.join("recovered.txt");
    write_bmp(&cover, 128, 128);
    fs::write(&payload, b"secret").unwrap();

    let mut embed = Command::new(cli_path())
        .args(["embed", "--in"])
        .arg(&cover)
        .arg("--payload")
        .arg(&payload)
        .arg("--out")
        .arg(&stego)
        .args(["--password", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    embed.stdin.as_mut().unwrap().write_all(b"right").unwrap();
    embed.wait().unwrap();

    let mut extract = Command::new(cli_path())
        .args(["extract", "--in"])
        .arg(&stego)
        .arg("--out")
        .arg(&recovered)
        .args(["--password", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    extract.stdin.as_mut().unwrap().write_all(b"wrong").unwrap();
    let out = extract.wait_with_output().unwrap();
    assert!(!out.status.success(), "wrong password should not succeed");
    let _ = fs::remove_dir_all(&d);
}
