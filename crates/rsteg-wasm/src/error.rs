//! Stable `#[repr(u32)]` error codes for the WASM ABI.
//!
//! Decoupled from `rsteg_core::Error` on purpose — adding a new core
//! variant must not shift wire-format codes. Many core variants collapse
//! into one wire code (every AEAD failure → `BadPassphrase` per spec 06's
//! indistinguishability requirement).

use rsteg_core::Error;

/// ABI error codes. **Stable wire contract — never renumber existing variants.**
/// Add new codes only at the end of the list.
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WasmError {
    /// Caller supplied a null pointer with a non-zero length.
    NullPointer = 1,
    /// Input exceeded `MAX_INPUT_BYTES`.
    InputTooLarge = 2,
    /// `format_fourcc` did not match any adapter compiled into this build.
    UnknownFormat = 3,
    /// Format fourcc was recognized but the bytes are not that format.
    FormatMismatch = 4,
    /// Carrier was recognized and decoded but contained no rsteg payload,
    /// or the header magic / version / reserved bits were wrong.
    HeaderMissing = 10,
    /// Carrier bytes were malformed in a way the adapter could not parse.
    Malformed = 11,
    /// File says encrypted but caller supplied no password.
    PassphraseRequired = 20,
    /// File is plaintext but caller supplied a password.
    UnexpectedPassphrase = 21,
    /// Authenticated-encryption open failed. Per spec 06, this is the sole
    /// failure mode for any AEAD variant — wrong passphrase, tampered bytes,
    /// or malformed inner header are indistinguishable here.
    BadPassphrase = 22,
    /// Build does not include the `crypto` feature but the carrier is encrypted.
    CryptoDisabled = 23,
    /// Argon2id could not allocate its scratch memory. Reserved — the
    /// native `argon2` crate currently panics on OOM rather than signalling,
    /// so this surfaces only if the WASM runtime traps and callers retry.
    KdfMemory = 24,
    /// OS / JS entropy source is unavailable. On wasm32 this typically
    /// means the JS glue did not wire up `rsteg_fill_random`, or the host
    /// refused `crypto.getRandomValues` under a restrictive CSP.
    RngUnavailable = 25,
    /// Fallback for any `rsteg_core::Error` variant not otherwise mapped.
    /// Stable code but imprecise; check `rsteg_last_error_message` for detail.
    Other = 255,
}

/// Map a `rsteg_core::Error` into a stable wire code.
///
/// Lossy by design. Per spec 06, every AEAD failure shape must be
/// indistinguishable to the caller — `BadPassphrase`, `BodyCrcMismatch`,
/// and any inner-header tamper all collapse to `WasmError::BadPassphrase`.
#[must_use]
pub fn translate_err(e: &Error) -> WasmError {
    match e {
        Error::FormatUnrecognized | Error::FormatUnsupported { .. } => WasmError::FormatMismatch,
        Error::Malformed { .. } => WasmError::Malformed,
        Error::HeaderMissing
        | Error::HeaderBadMagic
        | Error::HeaderBadVersion(_)
        | Error::HeaderReservedBitsSet => WasmError::HeaderMissing,
        Error::BodyCrcMismatch | Error::BadPassphrase | Error::KdfParams { .. } => {
            WasmError::BadPassphrase
        }
        Error::PassphraseRequired => WasmError::PassphraseRequired,
        Error::UnexpectedPassphrase => WasmError::UnexpectedPassphrase,
        Error::RngUnavailable => WasmError::RngUnavailable,
        Error::PayloadTooLarge { .. } => WasmError::InputTooLarge,
        Error::DensityOutOfRange(_) | Error::PermutationSeedRequired => WasmError::Other,
    }
}
