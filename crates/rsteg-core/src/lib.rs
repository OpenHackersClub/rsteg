//! rsteg-core — traits, framing, and errors.
//!
//! This crate grows vertically from TDD consumers. It starts with only what
//! the first failing test (`rsteg-bmp/tests/roundtrip_empty.rs`) needs.

#![deny(unsafe_code)]

mod bits;
mod crc32;
mod error;
mod header;

pub use bits::{BitReader, BitWriter};
pub use crc32::crc32_ieee;
pub use error::Error;
pub use header::{PayloadHeader, SchemeFourcc};

/// Embedding density — bits written per embedding unit.
///
/// `Low` and `Moderate` are public defaults. `Aggressive3` and `Aggressive4`
/// require the caller to have explicitly opted in (CLI: `--allow-aggressive-density`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Density {
    Low,
    Moderate,
    Aggressive3,
    Aggressive4,
}

impl Density {
    #[must_use]
    pub const fn bits(self) -> u8 {
        match self {
            Self::Low => 1,
            Self::Moderate => 2,
            Self::Aggressive3 => 3,
            Self::Aggressive4 => 4,
        }
    }

    /// Convert the raw bit count (1..=4) stored in `PayloadHeader.density`.
    #[must_use]
    pub const fn from_bits(b: u8) -> Option<Self> {
        match b {
            1 => Some(Self::Low),
            2 => Some(Self::Moderate),
            3 => Some(Self::Aggressive3),
            4 => Some(Self::Aggressive4),
            _ => None,
        }
    }
}

/// Embedding options.
#[derive(Clone, Debug)]
pub struct EmbedOpts {
    /// Scheme id (e.g. `"bmp-lsb-linear"`). `None` picks the adapter default.
    pub scheme: Option<&'static str>,
    pub density: Density,
}

/// Extraction options.
#[derive(Clone, Debug)]
pub struct ExtractOpts {
    pub scheme: Option<&'static str>,
    /// `None` means read density from the embedded header.
    pub density: Option<Density>,
    /// `true` in `--no-header` raw mode.
    pub skip_header: bool,
    /// Required when `skip_header` is `true`.
    pub raw_bit_count: Option<u64>,
}

/// Trait implemented by each carrier-format adapter.
pub trait FormatAdapter: Send + Sync + 'static {
    fn id(&self) -> &'static str;

    /// Cheap sniff: does this adapter recognize the bytes?
    fn recognize(&self, bytes: &[u8]) -> bool;

    /// Embed `framed` (already-encoded `PayloadHeader` + body) into `carrier`.
    ///
    /// The default implementation calls [`FormatAdapter::embed_into`].
    fn embed(
        &self,
        carrier: &[u8],
        framed: &[u8],
        opts: &EmbedOpts,
    ) -> Result<Vec<u8>, Error> {
        let mut out = Vec::with_capacity(carrier.len());
        self.embed_into(carrier, framed, opts, &mut out)?;
        Ok(out)
    }

    /// Extract framed bytes (`PayloadHeader` + body) from `stego`.
    ///
    /// The default implementation calls [`FormatAdapter::extract_into`].
    fn extract(&self, stego: &[u8], opts: &ExtractOpts) -> Result<Vec<u8>, Error> {
        let mut out = Vec::new();
        self.extract_into(stego, opts, &mut out)?;
        Ok(out)
    }

    /// Zero-alloc-when-possible embed. Writes into `out`, reusing capacity.
    fn embed_into(
        &self,
        carrier: &[u8],
        framed: &[u8],
        opts: &EmbedOpts,
        out: &mut Vec<u8>,
    ) -> Result<(), Error>;

    /// Zero-alloc-when-possible extract.
    fn extract_into(
        &self,
        stego: &[u8],
        opts: &ExtractOpts,
        out: &mut Vec<u8>,
    ) -> Result<(), Error>;
}
