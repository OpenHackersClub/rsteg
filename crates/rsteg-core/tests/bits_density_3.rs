//! Focused test: bit writer/reader round-trip at density 3 for sizes that
//! exercise the flush path.

use rsteg_core::{BitReader, BitWriter};

fn rt(payload: &[u8], density: u8) {
    let units_needed = (payload.len() * 8 + usize::from(density) - 1) / usize::from(density);
    let mut units = vec![0u8; units_needed];
    {
        let mut w = BitWriter::new(&mut units, density);
        w.write(payload).unwrap();
        w.flush().unwrap();
    }

    let mut out = vec![0u8; payload.len()];
    BitReader::new(&units, density).read(&mut out).unwrap();
    assert_eq!(&out, payload, "density={density} len={}", payload.len());
}

#[test]
fn density_3_single_byte() {
    for v in 0u8..=255 {
        rt(&[v], 3);
    }
}

#[test]
fn density_3_two_bytes() {
    rt(&[0xAB, 0xCD], 3);
    rt(&[0xFF, 0x00], 3);
    rt(&[0x00, 0xFF], 3);
}

#[test]
fn density_3_sizes_that_trigger_flush() {
    for len in [1, 2, 3, 4, 5, 7, 10, 31, 32, 33, 332] {
        let payload: Vec<u8> = (0..len).map(|i| (i * 37) as u8 ^ 0x5A).collect();
        rt(&payload, 3);
    }
}
