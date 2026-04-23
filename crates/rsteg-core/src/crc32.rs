//! CRC-32/IEEE (poly 0xEDB88320), single-byte table-driven.
//!
//! Spec 04 calls for slicing-by-8 (8 KB table, ~4× faster). We start with the
//! 1 KB single-byte table — minimum viable. The slicing-by-8 variant ships in
//! the refactor pass when a perf test demands it.

const POLY: u32 = 0xEDB8_8320;

static TABLE: [u32; 256] = {
    let mut t = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 == 1 { POLY ^ (c >> 1) } else { c >> 1 };
            k += 1;
        }
        t[i] = c;
        i += 1;
    }
    t
};

#[must_use]
pub fn crc32_ieee(bytes: &[u8]) -> u32 {
    let mut c = 0xFFFF_FFFF_u32;
    for &b in bytes {
        c = TABLE[((c ^ u32::from(b)) & 0xFF) as usize] ^ (c >> 8);
    }
    c ^ 0xFFFF_FFFF
}

#[cfg(test)]
mod tests {
    use super::crc32_ieee;

    #[test]
    fn empty_input_is_zero() {
        assert_eq!(crc32_ieee(&[]), 0);
    }

    #[test]
    fn known_test_vector_123456789() {
        assert_eq!(crc32_ieee(b"123456789"), 0xCBF4_3926);
    }
}
