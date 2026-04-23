//! 32-byte `PayloadHeader` framing. See `specs/04-core-traits.md`.
//!
//! Layout (big-endian ints):
//! ```text
//! 0..4    magic b"RSTG"
//! 4       version (1)
//! 5       flags    (bit0 encrypted, bit1 compressed, bit2 permuted, bit3..7 reserved-zero)
//! 6..10   crypto_fourcc
//! 10..14  scheme_fourcc
//! 14      density (1..=4)
//! 15      reserved (must be zero)
//! 16..20  body_len u32 BE
//! 20..24  body_crc32 u32 BE  (zero when flags.encrypted = 1)
//! 24..32  reserved (all zero)
//! ```

use crate::{crc32::crc32_ieee, Density, Error};

/// Strongly-typed FourCC identifying an embedding scheme.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SchemeFourcc(pub [u8; 4]);

impl SchemeFourcc {
    pub const ZERO: Self = Self([0; 4]);
    pub const BMP_LSB_LINEAR: Self = Self(*b"BLSL");
    pub const BMP_LSB_PERMUTED: Self = Self(*b"BLSP");
    pub const WAV_LSB_LINEAR: Self = Self(*b"WLSL");
    pub const WAV_LSB_PERMUTED: Self = Self(*b"WLSP");
    pub const PNG_LSB_LINEAR: Self = Self(*b"PLSL");
    pub const PNG_LSB_PERMUTED: Self = Self(*b"PLSP");
}

/// 32-byte fixed-layout payload header. See module docs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PayloadHeader {
    pub version: u8,
    pub flags: u8,
    pub crypto_fourcc: [u8; 4],
    pub scheme_fourcc: SchemeFourcc,
    pub density: u8,
    pub body_len: u32,
    pub body_crc32: u32,
}

impl PayloadHeader {
    pub const SIZE: usize = 32;
    pub const MAGIC: [u8; 4] = *b"RSTG";

    pub const FLAG_ENCRYPTED: u8 = 1 << 0;
    pub const FLAG_COMPRESSED: u8 = 1 << 1;
    pub const FLAG_PERMUTED: u8 = 1 << 2;

    /// Build a header for an unencrypted, uncompressed payload.
    ///
    /// `body_crc32` is computed from `body`.
    #[must_use]
    pub fn plain(scheme: SchemeFourcc, density: Density, body: &[u8]) -> Self {
        Self {
            version: 1,
            flags: 0,
            crypto_fourcc: [0; 4],
            scheme_fourcc: scheme,
            density: density.bits(),
            body_len: body.len() as u32,
            body_crc32: crc32_ieee(body),
        }
    }

    /// Encode the 32-byte wire form.
    #[must_use]
    pub fn encode(&self) -> [u8; Self::SIZE] {
        let mut out = [0u8; Self::SIZE];
        out[0..4].copy_from_slice(&Self::MAGIC);
        out[4] = self.version;
        out[5] = self.flags;
        out[6..10].copy_from_slice(&self.crypto_fourcc);
        out[10..14].copy_from_slice(&self.scheme_fourcc.0);
        out[14] = self.density;
        out[15] = 0;
        out[16..20].copy_from_slice(&self.body_len.to_be_bytes());
        out[20..24].copy_from_slice(&self.body_crc32.to_be_bytes());
        // 24..32 stay zero.
        out
    }

    /// Encode `header || body` into a fresh `Vec`.
    #[must_use]
    pub fn encode_with(&self, body: &[u8]) -> Vec<u8> {
        let mut v = Vec::with_capacity(Self::SIZE + body.len());
        v.extend_from_slice(&self.encode());
        v.extend_from_slice(body);
        v
    }

    /// Decode from bytes. Returns an error for bad magic, unknown version, or
    /// non-zero reserved fields.
    pub fn decode(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() < Self::SIZE {
            return Err(Error::HeaderMissing);
        }
        if bytes[0..4] != Self::MAGIC {
            return Err(Error::HeaderBadMagic);
        }
        let version = bytes[4];
        if version != 1 {
            return Err(Error::HeaderBadVersion(version));
        }
        let flags = bytes[5];
        // Reject unknown flag bits + reserved bytes.
        let known_flags =
            Self::FLAG_ENCRYPTED | Self::FLAG_COMPRESSED | Self::FLAG_PERMUTED;
        if (flags & !known_flags) != 0 {
            return Err(Error::HeaderReservedBitsSet);
        }
        if bytes[15] != 0 {
            return Err(Error::HeaderReservedBitsSet);
        }
        if bytes[24..32].iter().any(|&b| b != 0) {
            return Err(Error::HeaderReservedBitsSet);
        }

        let mut crypto_fourcc = [0u8; 4];
        crypto_fourcc.copy_from_slice(&bytes[6..10]);
        let mut scheme = [0u8; 4];
        scheme.copy_from_slice(&bytes[10..14]);
        let density = bytes[14];
        if !(1..=4).contains(&density) {
            return Err(Error::DensityOutOfRange(density));
        }
        let body_len = u32::from_be_bytes(bytes[16..20].try_into().unwrap());
        let body_crc32 = u32::from_be_bytes(bytes[20..24].try_into().unwrap());

        Ok(Self {
            version,
            flags,
            crypto_fourcc,
            scheme_fourcc: SchemeFourcc(scheme),
            density,
            body_len,
            body_crc32,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{PayloadHeader, SchemeFourcc};
    use crate::Density;

    #[test]
    fn plain_empty_roundtrips() {
        let h = PayloadHeader::plain(SchemeFourcc::BMP_LSB_LINEAR, Density::Low, &[]);
        let bytes = h.encode();
        let h2 = PayloadHeader::decode(&bytes).unwrap();
        assert_eq!(h, h2);
        assert_eq!(h2.body_len, 0);
        assert_eq!(h2.density, 1);
    }

    #[test]
    fn bad_magic_detected() {
        let mut bytes = [0u8; PayloadHeader::SIZE];
        bytes[0..4].copy_from_slice(b"XXXX");
        bytes[4] = 1;
        bytes[14] = 1;
        let err = PayloadHeader::decode(&bytes).unwrap_err();
        assert!(matches!(err, crate::Error::HeaderBadMagic));
    }

    #[test]
    fn reserved_bit_rejected() {
        let h = PayloadHeader::plain(SchemeFourcc::BMP_LSB_LINEAR, Density::Low, &[]);
        let mut bytes = h.encode();
        bytes[24] = 0x01; // trip reserved_2
        let err = PayloadHeader::decode(&bytes).unwrap_err();
        assert!(matches!(err, crate::Error::HeaderReservedBitsSet));
    }

    #[test]
    fn body_crc_computed() {
        let h = PayloadHeader::plain(SchemeFourcc::BMP_LSB_LINEAR, Density::Low, b"abc");
        let h2 = PayloadHeader::decode(&h.encode()).unwrap();
        assert_eq!(h2.body_len, 3);
        assert_eq!(h2.body_crc32, crate::crc32_ieee(b"abc"));
    }
}
