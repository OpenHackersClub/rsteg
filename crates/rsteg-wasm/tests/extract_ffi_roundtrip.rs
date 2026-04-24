//! Host-side smoke test for the `rsteg-wasm` FFI.
//!
//! Exercises `rsteg_alloc` → JS copy-in → `rsteg_extract` → copy-out →
//! `rsteg_free` against a BMP carrier embedded via the native
//! `rsteg-bmp` adapter. Proves the ABI round-trips without a wasm
//! toolchain — the browser smoke test in `tests/wasm-smoke/` (follow-up
//! PR) replays the same flow inside headless Chromium.
//!
//! This harness stands in for the JS glue, so it necessarily has
//! unsafe pointer code. Spec 02 rule 5 authorizes this test file under
//! the same umbrella as `crates/rsteg-wasm/src/lib.rs::ffi`.
//!
//! Plaintext-only: exercising the encrypted path here would trigger
//! Argon2id at production parameters (t=3, m=64MiB), which is already
//! covered by `rsteg-crypto-aead/tests/*`. The encrypted leg is exercised
//! end-to-end by the browser smoke test.

#![allow(unsafe_code)]

use rsteg_bmp::BMP_ADAPTER;
use rsteg_core::{Density, EmbedOpts, FormatAdapter, PayloadHeader, SchemeFourcc};
use rsteg_wasm::{fourcc, rsteg_alloc, rsteg_extract, rsteg_free, WasmError, OK};

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

fn embed_plaintext_bmp(cover: &[u8], payload: &[u8]) -> Vec<u8> {
    let header = PayloadHeader::plain(SchemeFourcc::BMP_LSB_LINEAR, Density::Low, payload);
    let framed = header.encode_with(payload);
    let opts = EmbedOpts {
        scheme: Some("bmp-lsb-linear"),
        density: Density::Low,
        seed: None,
    };
    BMP_ADAPTER.embed(cover, &framed, &opts).expect("embed")
}

/// Simulates the JS glue: copies `bytes` into a wasm-allocated buffer and
/// returns `(ptr, len)`. Caller must pair with `rsteg_free(ptr, len)`.
fn copy_to_wasm(bytes: &[u8]) -> (*mut u8, usize) {
    let len = bytes.len();
    let ptr = rsteg_alloc(len);
    assert!(!ptr.is_null(), "rsteg_alloc({len}) returned null");
    // Safety: just-allocated writable region of exactly `len` bytes.
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, len);
    }
    (ptr, len)
}

/// Thin wrapper so the test bodies stay readable; the FFI is the real
/// surface under test, this just isolates the unsafe block.
fn call_extract(
    stego_ptr: *const u8,
    stego_len: usize,
    password_ptr: *const u8,
    password_len: usize,
    fourcc: u32,
    out_ptr: *mut *mut u8,
    out_len: *mut usize,
) -> u32 {
    // Safety: every caller in this file constructs the pointer arguments
    // either from `rsteg_alloc` (valid for `len` bytes) or null with zero
    // length; `out_ptr` / `out_len` point to stack storage owned by the
    // caller. The call matches the function's `# Safety` contract.
    unsafe {
        rsteg_extract(
            stego_ptr,
            stego_len,
            password_ptr,
            password_len,
            fourcc,
            out_ptr,
            out_len,
        )
    }
}

fn free(ptr: *mut u8, len: usize) {
    // Safety: callers pass pointer/length pairs returned by `rsteg_alloc`
    // or by a successful `rsteg_extract`, never freed previously.
    unsafe { rsteg_free(ptr, len) };
}

#[test]
fn ffi_extract_roundtrips_plaintext_bmp() {
    let cover = make_bmp24(96, 96, 0x80);
    let payload: &[u8] = b"hello from rsteg-wasm";
    let stego = embed_plaintext_bmp(&cover, payload);

    let (stego_ptr, stego_len) = copy_to_wasm(&stego);
    let mut out_ptr: *mut u8 = std::ptr::null_mut();
    let mut out_len: usize = 0;

    let rc = call_extract(
        stego_ptr,
        stego_len,
        std::ptr::null(),
        0,
        fourcc::BMP,
        &mut out_ptr,
        &mut out_len,
    );
    assert_eq!(rc, OK, "extract returned {rc}");
    assert!(!out_ptr.is_null());
    assert_eq!(out_len, payload.len());

    // Safety: FFI wrote `out_ptr` pointing at `out_len` bytes we own
    // until `rsteg_free`.
    let extracted = unsafe { core::slice::from_raw_parts(out_ptr, out_len).to_vec() };
    assert_eq!(extracted, payload);

    free(out_ptr, out_len);
    free(stego_ptr, stego_len);
}

#[test]
fn ffi_extract_rejects_oversized_input() {
    let fake_ptr = 0x1000 as *const u8;
    let mut out_ptr: *mut u8 = std::ptr::null_mut();
    let mut out_len: usize = 0;
    let rc = call_extract(
        fake_ptr,
        (256 * 1024 * 1024) + 1,
        std::ptr::null(),
        0,
        fourcc::BMP,
        &mut out_ptr,
        &mut out_len,
    );
    assert_eq!(rc, WasmError::InputTooLarge as u32);
    assert!(out_ptr.is_null());
    assert_eq!(out_len, 0);
}

#[test]
fn ffi_extract_rejects_unknown_format() {
    let cover = make_bmp24(16, 16, 0);
    let stego = embed_plaintext_bmp(&cover, b"x");
    let (ptr, len) = copy_to_wasm(&stego);

    let mut out_ptr: *mut u8 = std::ptr::null_mut();
    let mut out_len: usize = 0;
    let rc = call_extract(
        ptr,
        len,
        std::ptr::null(),
        0,
        u32::from_be_bytes(*b"ZZZZ"),
        &mut out_ptr,
        &mut out_len,
    );
    assert_eq!(rc, WasmError::UnknownFormat as u32);

    free(ptr, len);
}

#[test]
fn ffi_extract_rejects_format_mismatch() {
    let cover = make_bmp24(16, 16, 0);
    let stego = embed_plaintext_bmp(&cover, b"x");
    let (ptr, len) = copy_to_wasm(&stego);

    let mut out_ptr: *mut u8 = std::ptr::null_mut();
    let mut out_len: usize = 0;
    let rc = call_extract(
        ptr,
        len,
        std::ptr::null(),
        0,
        fourcc::PNG,
        &mut out_ptr,
        &mut out_len,
    );
    assert_eq!(rc, WasmError::FormatMismatch as u32);

    free(ptr, len);
}

#[test]
fn ffi_extract_rejects_null_with_nonzero_len() {
    let mut out_ptr: *mut u8 = std::ptr::null_mut();
    let mut out_len: usize = 0;
    let rc = call_extract(
        std::ptr::null(),
        42,
        std::ptr::null(),
        0,
        fourcc::BMP,
        &mut out_ptr,
        &mut out_len,
    );
    assert_eq!(rc, WasmError::NullPointer as u32);
}

#[test]
fn ffi_extract_rejects_null_out_params() {
    let cover = make_bmp24(16, 16, 0);
    let stego = embed_plaintext_bmp(&cover, b"x");
    let (ptr, len) = copy_to_wasm(&stego);

    let rc = call_extract(
        ptr,
        len,
        std::ptr::null(),
        0,
        fourcc::BMP,
        std::ptr::null_mut(),
        std::ptr::null_mut(),
    );
    assert_eq!(rc, WasmError::NullPointer as u32);

    free(ptr, len);
}
