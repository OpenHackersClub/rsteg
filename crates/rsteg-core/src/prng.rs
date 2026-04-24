//! SplitMix64 PRNG + Fisher–Yates shuffle.
//!
//! Used by the *permuted* embedding schemes (`bmp-lsb-permuted`,
//! `wav-lsb-permuted`, …) to scatter header and body bytes over the carrier in
//! a passphrase-derived order. See `specs/04-core-traits.md` §"Bits & CRC" and
//! `specs/05-formats.md` §"Scheme `bmp-lsb-permuted`".
//!
//! The PRNG is deliberately small (~10 LOC) and takes a raw `u64` seed. The
//! blake2-based derivation from `passphrase || salt` lives in `rsteg-crypto`
//! where blake2 is already pulled in transitively via `argon2`. Keeping the
//! seed an opaque `u64` here lets `rsteg-core` stay std-only.

/// 64-bit SplitMix64 PRNG. Tiny, fast, deterministic — not cryptographic.
#[derive(Clone, Debug)]
pub struct Prng {
    state: u64,
}

impl Prng {
    /// Construct with an explicit seed. Two `Prng`s with the same seed produce
    /// the same stream.
    #[must_use]
    pub const fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    /// Advance the state and return the next 64-bit output.
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Return a uniform integer in `0..n`. Uses Lemire's fast-mod trick to
    /// avoid modulo bias on non-power-of-two `n`.
    pub fn bounded(&mut self, n: u64) -> u64 {
        debug_assert!(n > 0);
        let mut x = self.next_u64();
        let mut m = (u128::from(x)) * (u128::from(n));
        let mut l = m as u64;
        if l < n {
            let t = n.wrapping_neg() % n;
            while l < t {
                x = self.next_u64();
                m = (u128::from(x)) * (u128::from(n));
                l = m as u64;
            }
        }
        (m >> 64) as u64
    }
}

/// Fisher–Yates shuffle `indices` in place using `prng`.
///
/// For a slice of length `n`, this produces one of `n!` permutations uniformly
/// (modulo the PRNG's period, which is 2^64 — orders of magnitude more than
/// any realistic carrier).
pub fn shuffle<T>(prng: &mut Prng, indices: &mut [T]) {
    let len = indices.len();
    if len < 2 {
        return;
    }
    // `i` runs from len-1 down to 1; j = prng.bounded(i+1) picks an index in
    // 0..=i and we swap indices[i] ↔ indices[j].
    let mut i = len - 1;
    while i > 0 {
        let j = prng.bounded((i as u64) + 1) as usize;
        indices.swap(i, j);
        i -= 1;
    }
}

#[cfg(test)]
mod tests {
    use super::{shuffle, Prng};

    /// Reference: stream from `seed = 0`, matched against a second
    /// independent run from the same seed. Determinism is the main
    /// contract — every permuted extract relies on it.
    #[test]
    fn splitmix64_is_deterministic() {
        let mut a = Prng::new(0);
        let mut b = Prng::new(0);
        for _ in 0..64 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    /// Different seeds produce different first outputs (with overwhelming
    /// probability — we check across many seeds).
    #[test]
    fn different_seeds_diverge() {
        let a = Prng::new(0).next_u64();
        for s in 1..128 {
            assert_ne!(Prng::new(s).next_u64(), a, "seed {s} collided with 0");
        }
    }

    /// `bounded(n)` stays within `0..n`.
    #[test]
    fn bounded_respects_upper() {
        let mut p = Prng::new(42);
        for _ in 0..10_000 {
            let v = p.bounded(7);
            assert!(v < 7);
        }
    }

    /// Empty / single-element shuffles are no-ops.
    #[test]
    fn shuffle_trivial_lengths() {
        let mut p = Prng::new(1);
        let mut empty: [u32; 0] = [];
        shuffle(&mut p, &mut empty);
        let mut one = [42u32];
        shuffle(&mut p, &mut one);
        assert_eq!(one, [42]);
    }

    /// Shuffle is a permutation (same multiset, typically different order).
    #[test]
    fn shuffle_preserves_multiset() {
        let mut p = Prng::new(1234);
        let mut v: Vec<u32> = (0..64).collect();
        shuffle(&mut p, &mut v);
        let mut sorted = v.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, (0..64).collect::<Vec<_>>());
        // With overwhelming probability, the shuffled order != identity.
        assert_ne!(v, (0..64).collect::<Vec<_>>());
    }

    /// Same seed → same shuffle. Needed for extract to reverse the writer's
    /// permutation exactly.
    #[test]
    fn shuffle_is_deterministic() {
        let n = 128usize;
        let mut a: Vec<u32> = (0..n as u32).collect();
        let mut b: Vec<u32> = (0..n as u32).collect();
        shuffle(&mut Prng::new(0xABCDEF), &mut a);
        shuffle(&mut Prng::new(0xABCDEF), &mut b);
        assert_eq!(a, b);
    }
}
