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
        }
    }
}

impl std::error::Error for Error {}
