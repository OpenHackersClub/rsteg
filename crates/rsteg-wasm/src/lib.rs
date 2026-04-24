//! Browser-WASM façade for `rsteg`.
//!
//! Scope (spec 10 §"Roadmap placement"):
//! - `rsteg_alloc` / `rsteg_free` — linear-memory allocator shim used by JS.
//! - `rsteg_extract` — extract + AEAD open.
//! - `rsteg_embed` — embed + AEAD seal. Needs OS entropy; on wasm32 this
//!   comes through the custom `register_custom_getrandom!` backend in
//!   `src/rng.rs`, which forwards to a JS-provided `rsteg_fill_random`
//!   import (no `wasm-bindgen`).
//!
//! `rsteg_capacity` is still deliberately absent — it needs a new
//! `FormatAdapter::capacity` method in `rsteg-core` (not yet shipped).
//!
//! Per spec 02 rule 5, the unsafe code in this crate is authorized and
//! narrowly scoped — the `ffi` module below. Every `unsafe` block carries
//! a `// SAFETY:` comment.
//!
//! ABI: every entrypoint returns `u32` (0 on success, non-zero
//! `WasmError` code on failure). Output buffer pointer + length are
//! delivered via caller-provided `*mut *mut u8` and `*mut usize`
//! out-parameters that stay untouched on error. This keeps the shape
//! identical on wasm32 (32-bit pointers) and on 64-bit host targets
//! where the same code runs for the integration test — no clever bit
//! packing, no platform-dependent truncation.
//!
//! The `extern "C"` entrypoints are defined on every target (not just
//! wasm32) so host tests can exercise the ABI via `unsafe` calls without
//! needing a wasm toolchain. Shipping on a non-wasm32 target is a user
//! error, not a soundness hole.

#![deny(unsafe_code)]

use rsteg_core::PayloadHeader;

mod error;
#[cfg(feature = "crypto")]
mod rng;

pub use error::{translate_err, WasmError};

/// Hard cap on any single input buffer. 256 MiB. Prevents the `Vec::reserve`
/// panic path deep inside an adapter when JS uploads an oversized file.
pub const MAX_INPUT_BYTES: usize = 256 * 1024 * 1024;

/// Four-character codes carried on the ABI. Read big-endian so `b"PNG1"`
/// serializes as `0x504E_4731` and reads naturally in a JS debugger.
pub mod fourcc {
    pub const PNG: u32 = u32::from_be_bytes(*b"PNG1");
    pub const BMP: u32 = u32::from_be_bytes(*b"BMP1");
    pub const WAV: u32 = u32::from_be_bytes(*b"WAV1");
}

/// Success return code for every entrypoint. Any non-zero value is a
/// `WasmError` discriminant.
pub const OK: u32 = 0;

// ---------------------------------------------------------------------------
// Host-side safe-Rust core. Both the FFI shim and the host tests go through
// these functions — the shim is just a thin unsafe adapter.
// ---------------------------------------------------------------------------

/// Extract the payload body from a stego carrier.
///
/// Tries the adapter's default linear scheme first, then the permuted
/// scheme with a password-derived seed — mirrors `rsteg-cli` extract.
/// If the recovered header has `FLAG_ENCRYPTED`, the body is AEAD-opened
/// using `XChaCha20Argon2id` with the supplied password; otherwise it is
/// returned plaintext. Every AEAD failure collapses into
/// `WasmError::BadPassphrase` per the spec 06 indistinguishability rule.
pub fn extract_payload(
    stego: &[u8],
    password: Option<&[u8]>,
    format_fourcc: u32,
) -> Result<Vec<u8>, WasmError> {
    if stego.len() > MAX_INPUT_BYTES {
        return Err(WasmError::InputTooLarge);
    }
    if let Some(p) = password {
        if p.len() > MAX_INPUT_BYTES {
            return Err(WasmError::InputTooLarge);
        }
    }

    let framed = try_extract_framed(stego, password, format_fourcc)?;
    finalize(&framed, password)
}

/// Try linear, then permuted — matches `rsteg-cli::run_extract`.
fn try_extract_framed(
    stego: &[u8],
    password: Option<&[u8]>,
    format_fourcc: u32,
) -> Result<Vec<u8>, WasmError> {
    use rsteg_core::{ExtractOpts, FormatAdapter};

    let (adapter, linear, permuted): (&dyn FormatAdapter, &'static str, Option<&'static str>) =
        match format_fourcc {
            #[cfg(feature = "bmp")]
            fourcc::BMP => (
                &rsteg_bmp::BMP_ADAPTER,
                "bmp-lsb-linear",
                Some("bmp-lsb-permuted"),
            ),
            #[cfg(feature = "wav")]
            fourcc::WAV => (
                &rsteg_wav::WAV_ADAPTER,
                "wav-lsb-linear",
                Some("wav-lsb-permuted"),
            ),
            #[cfg(feature = "png")]
            fourcc::PNG => (&rsteg_png::PNG_ADAPTER, "png-lsb-linear", None),
            _ => return Err(WasmError::UnknownFormat),
        };

    if !adapter.recognize(stego) {
        return Err(WasmError::FormatMismatch);
    }

    // Attempt 1: linear (no seed).
    let linear_opts = ExtractOpts {
        scheme: Some(linear),
        density: None,
        skip_header: false,
        raw_bit_count: None,
        seed: None,
    };
    if let Ok(framed) = adapter.extract(stego, &linear_opts) {
        if PayloadHeader::decode(&framed).is_ok() {
            return Ok(framed);
        }
    }

    // Attempt 2: permuted (seed derived from password, if any).
    if let Some(permuted_id) = permuted {
        let seed = fold_u64(password.unwrap_or(&[]));
        let permuted_opts = ExtractOpts {
            scheme: Some(permuted_id),
            density: None,
            skip_header: false,
            raw_bit_count: None,
            seed: Some(seed),
        };
        if let Ok(framed) = adapter.extract(stego, &permuted_opts) {
            if PayloadHeader::decode(&framed).is_ok() {
                return Ok(framed);
            }
        }
    }

    Err(WasmError::HeaderMissing)
}

fn finalize(framed: &[u8], password: Option<&[u8]>) -> Result<Vec<u8>, WasmError> {
    let header = PayloadHeader::decode(framed).map_err(|e| translate_err(&e))?;
    let encrypted = (header.flags & PayloadHeader::FLAG_ENCRYPTED) != 0;

    match (encrypted, password) {
        (true, None) => Err(WasmError::PassphraseRequired),
        (false, Some(_)) => Err(WasmError::UnexpectedPassphrase),
        (false, None) => Ok(framed[PayloadHeader::SIZE..].to_vec()),
        (true, Some(pw)) => {
            #[cfg(feature = "crypto")]
            {
                use rsteg_core::CryptoScheme;
                let aad = header.encode();
                let scheme = rsteg_crypto_aead::XChaCha20Argon2id::new();
                scheme
                    .open(&framed[PayloadHeader::SIZE..], pw, &aad)
                    .map_err(|e| translate_err(&e))
            }
            #[cfg(not(feature = "crypto"))]
            {
                let _ = pw;
                Err(WasmError::CryptoDisabled)
            }
        }
    }
}

/// FNV-style fold, matches `rsteg-cli::fold_u64`. Not cryptographic —
/// seed secrecy is the AEAD's job, not the PRNG's.
fn fold_u64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x100_0000_01B3);
    }
    h
}

/// Embed `payload` into `cover` and return the stego carrier bytes.
///
/// Mirrors `rsteg-cli::run_embed`: when `password` is supplied the body is
/// XChaCha20-Poly1305 sealed with Argon2id KDF and the permuted scheme is
/// preferred (closes the "RSTG magic at offset 0" fingerprint); without a
/// password the output is plaintext-framed at the linear scheme.
///
/// AEAD seal on wasm32 draws 40 bytes of entropy via the custom `getrandom`
/// backend in `src/rng.rs`. On host targets `getrandom`'s built-in OS
/// backend is used.
#[cfg(feature = "crypto")]
pub fn embed_payload(
    cover: &[u8],
    payload: &[u8],
    password: Option<&[u8]>,
    format_fourcc: u32,
) -> Result<Vec<u8>, WasmError> {
    use rsteg_core::{CryptoScheme, Density, EmbedOpts, FormatAdapter, SchemeFourcc};

    if cover.len() > MAX_INPUT_BYTES || payload.len() > MAX_INPUT_BYTES {
        return Err(WasmError::InputTooLarge);
    }
    if let Some(p) = password {
        if p.len() > MAX_INPUT_BYTES {
            return Err(WasmError::InputTooLarge);
        }
    }

    #[allow(clippy::type_complexity)]
    let (adapter, linear_id, linear_fcc, permuted_id, permuted_fcc): (
        &dyn FormatAdapter,
        &'static str,
        SchemeFourcc,
        Option<&'static str>,
        Option<SchemeFourcc>,
    ) = match format_fourcc {
        #[cfg(feature = "bmp")]
        fourcc::BMP => (
            &rsteg_bmp::BMP_ADAPTER,
            "bmp-lsb-linear",
            SchemeFourcc::BMP_LSB_LINEAR,
            Some("bmp-lsb-permuted"),
            Some(SchemeFourcc::BMP_LSB_PERMUTED),
        ),
        #[cfg(feature = "wav")]
        fourcc::WAV => (
            &rsteg_wav::WAV_ADAPTER,
            "wav-lsb-linear",
            SchemeFourcc::WAV_LSB_LINEAR,
            Some("wav-lsb-permuted"),
            Some(SchemeFourcc::WAV_LSB_PERMUTED),
        ),
        #[cfg(feature = "png")]
        fourcc::PNG => (
            // PNG permuted is not in phase 1 — matches rsteg-cli behavior:
            // password-encrypted PNG still uses the linear scheme, and the
            // AEAD alone carries confidentiality.
            &rsteg_png::PNG_ADAPTER,
            "png-lsb-linear",
            SchemeFourcc::PNG_LSB_LINEAR,
            None,
            None,
        ),
        _ => return Err(WasmError::UnknownFormat),
    };

    if !adapter.recognize(cover) {
        return Err(WasmError::FormatMismatch);
    }

    let density = Density::Low;
    let is_encrypted = password.is_some();
    let use_permuted = is_encrypted && permuted_id.is_some();
    let (scheme_id, scheme_fcc) = if use_permuted {
        (permuted_id.unwrap(), permuted_fcc.unwrap())
    } else {
        (linear_id, linear_fcc)
    };

    if is_encrypted {
        // Matches `rsteg-cli::run_embed`: ciphertext is 42-byte inner header
        // + plaintext + 16-byte Poly1305 tag = plaintext + 58.
        const AEAD_OVERHEAD: u32 = 58;

        let mut header = PayloadHeader::plain(scheme_fcc, density, &[]);
        header.flags |= PayloadHeader::FLAG_ENCRYPTED;
        if use_permuted {
            header.flags |= PayloadHeader::FLAG_PERMUTED;
        }
        header.crypto_fourcc = rsteg_crypto_aead::FOURCC;
        let payload_u32 = u32::try_from(payload.len()).map_err(|_| WasmError::InputTooLarge)?;
        header.body_len = payload_u32
            .checked_add(AEAD_OVERHEAD)
            .ok_or(WasmError::InputTooLarge)?;
        header.body_crc32 = 0;

        let aad = header.encode();
        let pw = password.unwrap_or(&[]);
        let scheme = rsteg_crypto_aead::XChaCha20Argon2id::new();
        let ct = scheme.seal(payload, pw, &aad).map_err(|e| translate_err(&e))?;

        let mut framed = Vec::with_capacity(PayloadHeader::SIZE + ct.len());
        framed.extend_from_slice(&aad);
        framed.extend_from_slice(&ct);

        let seed = if use_permuted { Some(fold_u64(pw)) } else { None };
        let opts = EmbedOpts {
            scheme: Some(scheme_id),
            density,
            seed,
        };
        adapter
            .embed(cover, &framed, &opts)
            .map_err(|e| translate_err(&e))
    } else {
        let header = PayloadHeader::plain(scheme_fcc, density, payload);
        let framed = header.encode_with(payload);
        let opts = EmbedOpts {
            scheme: Some(scheme_id),
            density,
            seed: None,
        };
        adapter
            .embed(cover, &framed, &opts)
            .map_err(|e| translate_err(&e))
    }
}

/// Embed path is only available with the `crypto` feature (the header
/// framing crate is brought in by rsteg-core, but encryption depends on
/// rsteg-crypto-aead). Callers that try to embed without `crypto` enabled
/// and without a password could in principle go through a plaintext path,
/// but the ABI surface keeps embed symmetric with extract: both require
/// the full default feature set.
#[cfg(not(feature = "crypto"))]
pub fn embed_payload(
    _cover: &[u8],
    _payload: &[u8],
    _password: Option<&[u8]>,
    _format_fourcc: u32,
) -> Result<Vec<u8>, WasmError> {
    Err(WasmError::CryptoDisabled)
}

// ---------------------------------------------------------------------------
// FFI shim. Authorized unsafe per spec 02 rule 5. Thin — every entrypoint
// validates its arguments, forwards to the safe-Rust core above, and packs
// the result into the `u64` ABI.
// ---------------------------------------------------------------------------

#[allow(unsafe_code)]
mod ffi {
    use super::{embed_payload, extract_payload, WasmError, MAX_INPUT_BYTES, OK};
    use std::alloc::{alloc, dealloc, Layout};

    /// Allocate `len` bytes in linear memory. Returns a pointer the caller
    /// fills and later hands to one of the entrypoints. Caller is responsible
    /// for pairing every `rsteg_alloc(len)` with exactly one `rsteg_free(ptr, len)`.
    ///
    /// Safe to call — does not dereference user pointers. Kept outside
    /// `unsafe extern "C"` so JS glue can call it like any other function.
    #[no_mangle]
    pub extern "C" fn rsteg_alloc(len: usize) -> *mut u8 {
        if len == 0 || len > MAX_INPUT_BYTES {
            return core::ptr::null_mut();
        }
        let Ok(layout) = Layout::from_size_align(len, 1) else {
            return core::ptr::null_mut();
        };
        // SAFETY: `layout` has non-zero size (checked above) and alignment 1
        // which is always valid. `alloc` may return null under pressure; we
        // propagate that to the caller unchanged.
        unsafe { alloc(layout) }
    }

    /// Free a buffer previously returned by `rsteg_alloc` (or by one of the
    /// extraction entrypoints, which hand back Rust-owned buffers).
    ///
    /// # Safety
    ///
    /// `ptr` and `len` must be the exact pair returned by a prior
    /// `rsteg_alloc` / entrypoint call, and this pair must not have been
    /// freed already. A null `ptr` or zero `len` is a no-op.
    #[no_mangle]
    pub unsafe extern "C" fn rsteg_free(ptr: *mut u8, len: usize) {
        if ptr.is_null() || len == 0 {
            return;
        }
        let Ok(layout) = Layout::from_size_align(len, 1) else {
            return;
        };
        // SAFETY: caller contract — `ptr` and `len` came from a prior
        // `rsteg_alloc` / entrypoint return, and the caller has not freed
        // them previously. Alignment is 1 which matches the alloc layout.
        unsafe { dealloc(ptr, layout) };
    }

    /// Extract and return the payload body.
    ///
    /// On success (return value `OK`), `*out_ptr` holds a pointer to a
    /// Rust-owned buffer of `*out_len` bytes — caller must copy the bytes
    /// out then call `rsteg_free(*out_ptr, *out_len)`. On error (non-zero
    /// return), `out_ptr` and `out_len` are left untouched.
    ///
    /// `password_ptr` may be null with `password_len == 0` to indicate
    /// "no passphrase"; any other shape is a `NullPointer` error.
    ///
    /// # Safety
    ///
    /// `stego_ptr` must point to `stego_len` readable bytes; if
    /// `password_ptr` is non-null it must point to `password_len` readable
    /// bytes; `out_ptr` and `out_len` must each point to writable storage
    /// of their respective pointee sizes. All pointer inputs must remain
    /// live for the duration of the call. On success the memory written
    /// into `*out_ptr` is owned by the caller until `rsteg_free`.
    #[no_mangle]
    pub unsafe extern "C" fn rsteg_extract(
        stego_ptr: *const u8,
        stego_len: usize,
        password_ptr: *const u8,
        password_len: usize,
        format_fourcc: u32,
        out_ptr: *mut *mut u8,
        out_len: *mut usize,
    ) -> u32 {
        if out_ptr.is_null() || out_len.is_null() {
            return WasmError::NullPointer as u32;
        }
        let stego = match as_slice(stego_ptr, stego_len) {
            Ok(s) => s,
            Err(code) => return code as u32,
        };
        let password = if password_ptr.is_null() && password_len == 0 {
            None
        } else {
            match as_slice(password_ptr, password_len) {
                Ok(s) => Some(s),
                Err(code) => return code as u32,
            }
        };

        match extract_payload(stego, password, format_fourcc) {
            Ok(v) => {
                let (ptr, len) = into_raw_parts(v);
                // SAFETY: caller passed non-null `out_ptr` / `out_len`
                // (checked above). Writes obey the out-parameter contract.
                unsafe {
                    *out_ptr = ptr;
                    *out_len = len;
                }
                OK
            }
            Err(e) => e as u32,
        }
    }

    /// Embed `payload` into `cover` and return a Rust-owned buffer of
    /// stego-carrier bytes via the same `(out_ptr, out_len)` out-parameter
    /// shape as [`rsteg_extract`].
    ///
    /// When `password_ptr` / `password_len` describe a non-empty slice,
    /// the body is AEAD-sealed using XChaCha20-Poly1305 + Argon2id and the
    /// permuted LSB scheme is used for BMP / WAV (PNG stays linear; the
    /// AEAD alone provides confidentiality). When the password is absent
    /// (`(null, 0)`) the body is embedded plaintext at the linear scheme.
    ///
    /// On wasm32 the seal path draws entropy via the custom `getrandom`
    /// backend (`rsteg_fill_random` JS import). A JS glue that does not
    /// wire up that import will produce `WasmError::RngUnavailable`.
    ///
    /// # Safety
    ///
    /// Same contract as `rsteg_extract`: all non-null pointers must point
    /// to the declared number of readable (or writable, for out-params)
    /// bytes and remain live for the duration of the call. Output buffer
    /// ownership transfers to the caller on success.
    #[no_mangle]
    pub unsafe extern "C" fn rsteg_embed(
        cover_ptr: *const u8,
        cover_len: usize,
        payload_ptr: *const u8,
        payload_len: usize,
        password_ptr: *const u8,
        password_len: usize,
        format_fourcc: u32,
        out_ptr: *mut *mut u8,
        out_len: *mut usize,
    ) -> u32 {
        if out_ptr.is_null() || out_len.is_null() {
            return WasmError::NullPointer as u32;
        }
        let cover = match as_slice(cover_ptr, cover_len) {
            Ok(s) => s,
            Err(code) => return code as u32,
        };
        let payload = match as_slice(payload_ptr, payload_len) {
            Ok(s) => s,
            Err(code) => return code as u32,
        };
        let password = if password_ptr.is_null() && password_len == 0 {
            None
        } else {
            match as_slice(password_ptr, password_len) {
                Ok(s) => Some(s),
                Err(code) => return code as u32,
            }
        };

        match embed_payload(cover, payload, password, format_fourcc) {
            Ok(v) => {
                let (ptr, len) = into_raw_parts(v);
                // SAFETY: both out-params checked non-null above.
                unsafe {
                    *out_ptr = ptr;
                    *out_len = len;
                }
                OK
            }
            Err(e) => e as u32,
        }
    }

    /// Convert `(ptr, len)` into a safe slice, validating nullness and size
    /// caps. An empty-but-non-null buffer is permitted (JS side may pass
    /// `rsteg_alloc(0)` → null; this path handles both shapes consistently).
    fn as_slice<'a>(ptr: *const u8, len: usize) -> Result<&'a [u8], WasmError> {
        if len > MAX_INPUT_BYTES {
            return Err(WasmError::InputTooLarge);
        }
        if len == 0 {
            return Ok(&[]);
        }
        if ptr.is_null() {
            return Err(WasmError::NullPointer);
        }
        // SAFETY: caller contract per `rsteg-wasm/README.md` §"Ownership".
        // `ptr` refers to a JS-side allocation of exactly `len` bytes that
        // the caller promises to keep live for the duration of this call.
        // The slice is bounded to `len` and not retained past return.
        Ok(unsafe { core::slice::from_raw_parts(ptr, len) })
    }

    /// Hand a Rust-owned `Vec<u8>` to the JS caller. Uses `Box::leak` to
    /// detach ownership — the caller must call `rsteg_free(ptr, len)` to
    /// reclaim the memory. An empty vec returns `(null, 0)`.
    fn into_raw_parts(v: Vec<u8>) -> (*mut u8, usize) {
        if v.is_empty() {
            return (core::ptr::null_mut(), 0);
        }
        let boxed = v.into_boxed_slice();
        let len = boxed.len();
        let ptr = Box::leak(boxed).as_mut_ptr();
        (ptr, len)
    }
}

// Re-export the FFI symbols so integration tests can call them as regular
// functions without depending on a linker for `#[no_mangle]`.
pub use ffi::{rsteg_alloc, rsteg_embed, rsteg_extract, rsteg_free};


