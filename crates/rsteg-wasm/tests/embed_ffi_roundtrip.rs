//! Host-side smoke test for the `rsteg_embed` FFI.
//!
//! Plaintext round-trip: embed via FFI → extract via native adapter →
//! compare. Proves the embed path end-to-end without needing a wasm
//! toolchain.
//!
//! Encrypted embed (which exercises the `register_custom_getrandom!`
//! backend via `XChaCha20Argon2id::seal`) is deferred to the browser smoke
//! test — the Argon2id KDF at production parameters (t=3, m=64MiB) adds
//! ~400 ms per test invocation, which is heavy for a CI that runs on
//! every PR. On host targets the built-in `getrandom` backend is used
//! anyway, so a host test wouldn't exercise the custom path.

#![allow(unsafe_code)]

use rsteg_bmp::BMP_ADAPTER;
use rsteg_core::{ExtractOpts, FormatAdapter, PayloadHeader};
use rsteg_wasm::{fourcc, rsteg_alloc, rsteg_embed, rsteg_free, WasmError, OK};

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

fn copy_to_wasm(bytes: &[u8]) -> (*mut u8, usize) {
    let len = bytes.len();
    let ptr = rsteg_alloc(len);
    assert!(!ptr.is_null(), "rsteg_alloc({len}) returned null");
    // Safety: just-allocated writable region of `len` bytes.
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, len);
    }
    (ptr, len)
}

fn free(ptr: *mut u8, len: usize) {
    // Safety: pointer/length pair returned by `rsteg_alloc` or an entrypoint.
    unsafe { rsteg_free(ptr, len) };
}

#[allow(clippy::too_many_arguments)]
fn call_embed(
    cover_ptr: *const u8,
    cover_len: usize,
    payload_ptr: *const u8,
    payload_len: usize,
    password_ptr: *const u8,
    password_len: usize,
    fcc: u32,
    out_ptr: *mut *mut u8,
    out_len: *mut usize,
) -> u32 {
    // Safety: all pointers are either null (matched by the len) or come
    // from `rsteg_alloc`. Out-params point to stack locals owned by the
    // caller. Matches the function's `# Safety` contract.
    unsafe {
        rsteg_embed(
            cover_ptr,
            cover_len,
            payload_ptr,
            payload_len,
            password_ptr,
            password_len,
            fcc,
            out_ptr,
            out_len,
        )
    }
}

#[test]
fn ffi_embed_roundtrips_plaintext_bmp() {
    let cover = make_bmp24(96, 96, 0x80);
    let payload: &[u8] = b"hello from rsteg_embed";

    let (cover_ptr, cover_len) = copy_to_wasm(&cover);
    let (payload_ptr, payload_len) = copy_to_wasm(payload);
    let mut out_ptr: *mut u8 = std::ptr::null_mut();
    let mut out_len: usize = 0;

    let rc = call_embed(
        cover_ptr,
        cover_len,
        payload_ptr,
        payload_len,
        std::ptr::null(),
        0,
        fourcc::BMP,
        &mut out_ptr,
        &mut out_len,
    );
    assert_eq!(rc, OK, "embed returned {rc}");
    assert!(!out_ptr.is_null());
    assert_eq!(out_len, cover.len(), "BMP stego preserves cover length");

    // Safety: out_ptr points to out_len bytes owned by us until rsteg_free.
    let stego = unsafe { core::slice::from_raw_parts(out_ptr, out_len).to_vec() };

    // Round-trip: extract via the native adapter and verify we get back
    // the framed header + payload we embedded.
    let extract_opts = ExtractOpts {
        scheme: Some("bmp-lsb-linear"),
        density: None,
        skip_header: false,
        raw_bit_count: None,
        seed: None,
    };
    let framed = BMP_ADAPTER
        .extract(&stego, &extract_opts)
        .expect("native extract should succeed");
    let header = PayloadHeader::decode(&framed).expect("valid header");
    assert_eq!(
        header.flags & PayloadHeader::FLAG_ENCRYPTED,
        0,
        "plaintext embed must not set FLAG_ENCRYPTED"
    );
    let body = &framed[PayloadHeader::SIZE..PayloadHeader::SIZE + header.body_len as usize];
    assert_eq!(body, payload, "extracted body must equal embedded payload");

    free(out_ptr, out_len);
    free(payload_ptr, payload_len);
    free(cover_ptr, cover_len);
}

#[test]
fn ffi_embed_rejects_unknown_format() {
    let cover = make_bmp24(16, 16, 0);
    let payload = b"x";
    let (cp, cl) = copy_to_wasm(&cover);
    let (pp, pl) = copy_to_wasm(payload);
    let mut op: *mut u8 = std::ptr::null_mut();
    let mut ol: usize = 0;

    let rc = call_embed(
        cp,
        cl,
        pp,
        pl,
        std::ptr::null(),
        0,
        u32::from_be_bytes(*b"ZZZZ"),
        &mut op,
        &mut ol,
    );
    assert_eq!(rc, WasmError::UnknownFormat as u32);

    free(pp, pl);
    free(cp, cl);
}

#[test]
fn ffi_embed_rejects_format_mismatch() {
    let cover = make_bmp24(16, 16, 0);
    let payload = b"x";
    let (cp, cl) = copy_to_wasm(&cover);
    let (pp, pl) = copy_to_wasm(payload);
    let mut op: *mut u8 = std::ptr::null_mut();
    let mut ol: usize = 0;

    // Claim PNG fourcc against BMP bytes.
    let rc = call_embed(
        cp,
        cl,
        pp,
        pl,
        std::ptr::null(),
        0,
        fourcc::PNG,
        &mut op,
        &mut ol,
    );
    assert_eq!(rc, WasmError::FormatMismatch as u32);

    free(pp, pl);
    free(cp, cl);
}

#[test]
fn ffi_embed_rejects_oversized_input() {
    // Lie about cover length; we never dereference the pointer.
    let mut op: *mut u8 = std::ptr::null_mut();
    let mut ol: usize = 0;
    let rc = call_embed(
        0x1000 as *const u8,
        (256 * 1024 * 1024) + 1,
        0x2000 as *const u8,
        8,
        std::ptr::null(),
        0,
        fourcc::BMP,
        &mut op,
        &mut ol,
    );
    assert_eq!(rc, WasmError::InputTooLarge as u32);
}

#[test]
fn ffi_embed_rejects_null_out_params() {
    let cover = make_bmp24(16, 16, 0);
    let payload = b"x";
    let (cp, cl) = copy_to_wasm(&cover);
    let (pp, pl) = copy_to_wasm(payload);

    let rc = call_embed(
        cp,
        cl,
        pp,
        pl,
        std::ptr::null(),
        0,
        fourcc::BMP,
        std::ptr::null_mut(),
        std::ptr::null_mut(),
    );
    assert_eq!(rc, WasmError::NullPointer as u32);

    free(pp, pl);
    free(cp, cl);
}
