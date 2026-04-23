//! Bit-level I/O primitives for embedding into LSBs.
//!
//! The writer/reader each maintain a persistent bit accumulator across calls
//! to `write`/`read`. This is essential for densities that don't divide 8
//! (density 3) where successive calls split a payload byte across units — a
//! per-call reset would lose the boundary bits.

/// Writes a sequence of bytes into the low `density` bits of a slice of
/// embedding units. Reads payload bytes MSB-first.
#[derive(Debug)]
pub struct BitWriter<'a> {
    units: &'a mut [u8],
    density: u8,
    unit_cursor: usize,
    bit_buffer: u32,
    bits_in_buffer: u32,
}

impl<'a> BitWriter<'a> {
    /// `density` must be 1..=4; violations are the caller's problem.
    pub fn new(units: &'a mut [u8], density: u8) -> Self {
        debug_assert!((1..=4).contains(&density));
        Self {
            units,
            density,
            unit_cursor: 0,
            bit_buffer: 0,
            bits_in_buffer: 0,
        }
    }

    /// Number of embedding units the writer still has available.
    #[must_use]
    pub fn units_remaining(&self) -> usize {
        self.units.len().saturating_sub(self.unit_cursor)
    }

    /// Number of payload bytes still writable at current density, ignoring
    /// any bits already accumulated in the buffer.
    #[must_use]
    pub fn bytes_capacity(&self) -> usize {
        (self.units_remaining() * usize::from(self.density)) / 8
    }

    /// Write `payload`. Accumulates across calls; call [`BitWriter::flush`]
    /// after the final write to commit any trailing bits.
    pub fn write(&mut self, payload: &[u8]) -> Result<(), ()> {
        let mask = (1u8 << self.density) - 1;
        let bits_per_unit = u32::from(self.density);

        for &byte in payload {
            self.bit_buffer = (self.bit_buffer << 8) | u32::from(byte);
            self.bits_in_buffer += 8;

            while self.bits_in_buffer >= bits_per_unit {
                if self.unit_cursor >= self.units.len() {
                    return Err(());
                }
                let shift = self.bits_in_buffer - bits_per_unit;
                let value = ((self.bit_buffer >> shift) as u8) & mask;
                let unit = &mut self.units[self.unit_cursor];
                *unit = (*unit & !mask) | value;
                self.unit_cursor += 1;
                self.bits_in_buffer -= bits_per_unit;
                self.bit_buffer &= (1u32 << self.bits_in_buffer).wrapping_sub(1);
            }
        }
        Ok(())
    }

    /// Flush trailing bits. Writes one partial unit with the remaining bits
    /// in the high positions of the density-bit field, padding low positions
    /// with zero. Idempotent (no-op when buffer is empty).
    pub fn flush(&mut self) -> Result<(), ()> {
        if self.bits_in_buffer == 0 {
            return Ok(());
        }
        let mask = (1u8 << self.density) - 1;
        let bits_per_unit = u32::from(self.density);
        if self.unit_cursor >= self.units.len() {
            return Err(());
        }
        let pad = bits_per_unit - self.bits_in_buffer;
        let value = ((self.bit_buffer << pad) as u8) & mask;
        let unit = &mut self.units[self.unit_cursor];
        *unit = (*unit & !mask) | value;
        self.unit_cursor += 1;
        self.bits_in_buffer = 0;
        self.bit_buffer = 0;
        Ok(())
    }
}

/// Reads a sequence of bytes from the low `density` bits of a slice of
/// embedding units. Writes extracted bytes MSB-first. Maintains state
/// across `read()` calls.
#[derive(Debug)]
pub struct BitReader<'a> {
    units: &'a [u8],
    density: u8,
    unit_cursor: usize,
    bit_buffer: u32,
    bits_in_buffer: u32,
}

impl<'a> BitReader<'a> {
    pub fn new(units: &'a [u8], density: u8) -> Self {
        debug_assert!((1..=4).contains(&density));
        Self {
            units,
            density,
            unit_cursor: 0,
            bit_buffer: 0,
            bits_in_buffer: 0,
        }
    }

    /// How many bytes could still be assembled from remaining unit data
    /// plus bits already in the buffer.
    #[must_use]
    pub fn bytes_available(&self) -> usize {
        let units_bits = (self.units.len() - self.unit_cursor) * usize::from(self.density);
        let total_bits = units_bits + self.bits_in_buffer as usize;
        total_bits / 8
    }

    /// Read exactly `out.len()` bytes, filling `out`. `Err` on exhaustion.
    pub fn read(&mut self, out: &mut [u8]) -> Result<(), ()> {
        if out.len() > self.bytes_available() {
            return Err(());
        }
        let mask = (1u8 << self.density) - 1;
        let bits_per_unit = u32::from(self.density);
        let mut out_cursor = 0;

        while out_cursor < out.len() {
            while self.bits_in_buffer < 8 {
                let unit_value = u32::from(self.units[self.unit_cursor] & mask);
                self.bit_buffer = (self.bit_buffer << bits_per_unit) | unit_value;
                self.bits_in_buffer += bits_per_unit;
                self.unit_cursor += 1;
            }
            let shift = self.bits_in_buffer - 8;
            out[out_cursor] = (self.bit_buffer >> shift) as u8;
            self.bits_in_buffer -= 8;
            self.bit_buffer &= (1u32 << self.bits_in_buffer).wrapping_sub(1);
            out_cursor += 1;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{BitReader, BitWriter};

    #[test]
    fn write_then_read_density_1_roundtrips() {
        let payload: &[u8] = b"hello";
        let mut units = vec![0u8; payload.len() * 8];
        let mut w = BitWriter::new(&mut units, 1);
        w.write(payload).unwrap();
        w.flush().unwrap();

        let mut out = vec![0u8; payload.len()];
        BitReader::new(&units, 1).read(&mut out).unwrap();
        assert_eq!(out, payload);
    }

    #[test]
    fn write_preserves_upper_bits() {
        let mut units = vec![0xFFu8; 8];
        let mut w = BitWriter::new(&mut units, 1);
        w.write(&[0x00]).unwrap();
        w.flush().unwrap();
        for u in units {
            assert_eq!(u & 0xFE, 0xFE, "upper bits clobbered");
            assert_eq!(u & 0x01, 0x00, "LSB not set to payload bit");
        }
    }

    #[test]
    fn density_4_roundtrips() {
        let payload = &[0xDE, 0xAD, 0xBE, 0xEF];
        let mut units = vec![0u8; payload.len() * 2];
        let mut w = BitWriter::new(&mut units, 4);
        w.write(payload).unwrap();
        w.flush().unwrap();

        let mut out = vec![0u8; payload.len()];
        BitReader::new(&units, 4).read(&mut out).unwrap();
        assert_eq!(&out, payload);
    }

    /// Writing in two chunks must give the same units as one call.
    #[test]
    fn write_state_survives_split_at_density_3() {
        let payload: Vec<u8> = (0..33u8).collect();
        let units_needed = (payload.len() * 8 + 2) / 3;

        let mut one = vec![0u8; units_needed];
        let mut w = BitWriter::new(&mut one, 3);
        w.write(&payload).unwrap();
        w.flush().unwrap();

        let mut split = vec![0u8; units_needed];
        let mut w2 = BitWriter::new(&mut split, 3);
        w2.write(&payload[..10]).unwrap();
        w2.write(&payload[10..]).unwrap();
        w2.flush().unwrap();

        assert_eq!(one, split);
    }

    /// Reading in two chunks must yield the same output as one call.
    #[test]
    fn read_state_survives_split_at_density_3() {
        let payload: Vec<u8> = (0..33u8).collect();
        let units_needed = (payload.len() * 8 + 2) / 3;
        let mut units = vec![0u8; units_needed];
        let mut w = BitWriter::new(&mut units, 3);
        w.write(&payload).unwrap();
        w.flush().unwrap();

        let mut r = BitReader::new(&units, 3);
        let mut a = [0u8; 10];
        let mut b = vec![0u8; 23];
        r.read(&mut a).unwrap();
        r.read(&mut b).unwrap();

        assert_eq!(&a[..], &payload[..10]);
        assert_eq!(&b[..], &payload[10..]);
    }
}
