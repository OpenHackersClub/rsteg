//! `rsteg-core` error type.
//!
//! Hand-written `Display` and `std::error::Error` impls — we do not pull
//! `thiserror` (proc-macro ban, spec 02).

use std::fmt;

#[derive(Debug)]
pub enum Error {
    /// Bytes did not look like any known carrier format for this adapter.
    FormatUnrecognized,
    /// Format recognized, but a variant / option is unsupported.
    FormatUnsupported {
        id: &'static str,
        reason: &'static str,
    },
    /// Carrier was recognized but malformed.
    Malformed {
        at: &'static str,
        detail: &'static str,
    },
    /// Payload + framing exceeds carrier capacity.
    PayloadTooLarge { needed: u64, available: u64 },
    /// Expected a `PayloadHeader` but could not find one (e.g. stego not modified).
    HeaderMissing,
    /// Magic bytes at the start of the extracted header were wrong.
    HeaderBadMagic,
    /// Header version field is not one we recognize.
    HeaderBadVersion(u8),
    /// A reserved-must-be-zero field was non-zero — reject to keep the wire
    /// format extensible cleanly.
    HeaderReservedBitsSet,
    /// Plaintext body's CRC32 did not match the header-declared CRC.
    /// Unreachable for encrypted bodies (AEAD catches tampering first).
    BodyCrcMismatch,
    /// Density value outside of 1..=4.
    DensityOutOfRange(u8),
    /// Permuted scheme requested without a PRNG seed in `opts.seed`.
    PermutationSeedRequired,

    // -- Crypto (encoded for completeness; only emitted when a CryptoScheme
    //    impl is registered). Kept in the central enum per spec 04.
    /// AEAD authentication failure. Sole failure mode for any encrypted open —
    /// the extract path must not distinguish "wrong passphrase" from "tampered
    /// ciphertext" from "malformed inner header" (spec 06 §"Open flow"). The
    /// timing test in `rsteg-crypto-aead/tests/timing.rs` asserts this.
    BadPassphrase,
    /// File header says `flags.encrypted = 1` but caller supplied no passphrase.
    PassphraseRequired,
    /// Caller supplied a passphrase but file is plaintext. CLI hints the user
    /// to omit `--password`.
    UnexpectedPassphrase,
    /// Inner crypto header has an unknown KDF id or version. Single variant so
    /// the error surface doesn't leak which field tripped (spec 06).
    KdfParams {
        detail: &'static str,
    },
    /// OS entropy source is unavailable (hard stop, no soft-fallback).
    RngUnavailable,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FormatUnrecognized => f.write_str("carrier format not recognized"),
            Self::FormatUnsupported { id, reason } => {
                write!(f, "format '{id}' is recognized but unsupported: {reason}")
            }
            Self::Malformed { at, detail } => write!(f, "malformed carrier at {at}: {detail}"),
            Self::PayloadTooLarge { needed, available } => write!(
                f,
                "payload too large: need {needed} bytes of capacity, have {available}"
            ),
            Self::HeaderMissing => f.write_str("no rsteg payload header found"),
            Self::HeaderBadMagic => f.write_str("payload header has wrong magic"),
            Self::HeaderBadVersion(v) => write!(f, "unsupported payload header version {v}"),
            Self::HeaderReservedBitsSet => f.write_str("payload header reserved bits set"),
            Self::BodyCrcMismatch => f.write_str("payload body CRC32 mismatch"),
            Self::DensityOutOfRange(d) => write!(f, "density {d} is outside 1..=4"),
            Self::PermutationSeedRequired => {
                f.write_str("permuted scheme requires EmbedOpts/ExtractOpts::seed")
            }
            Self::BadPassphrase => f.write_str("authentication failure (bad passphrase or tampered ciphertext)"),
            Self::PassphraseRequired => f.write_str("file is encrypted; --password is required"),
            Self::UnexpectedPassphrase => {
                f.write_str("file is plaintext; --password was supplied but not needed")
            }
            Self::KdfParams { detail } => write!(f, "unsupported KDF parameters: {detail}"),
            Self::RngUnavailable => f.write_str("OS random number generator unavailable"),
        }
    }
}

impl std::error::Error for Error {}
