//! Timing-side-channel test. Per spec 07, the three failure modes of `open`
//! (bad magic / bad tag / wrong passphrase-derived key) must run in
//! comparable wall-clock time so no extract-path oracle leaks which one
//! failed.
//!
//! **Caveat.** Wall-clock on a shared CI runner is noisy. We use a fairly
//! loose threshold (medians within 3× of each other after warmup). That's
//! still tight enough to catch a "short-circuit on magic mismatch" bug,
//! which would make one path ~order-of-magnitude faster than the others.
//!
//! The detached in-place AEAD already does the Poly1305 compare via
//! `subtle::ConstantTimeEq`, so at the cryptographic layer we inherit the
//! constant-time tag comparison. This test guards the surrounding glue —
//! early-outs on header inspection, etc.

use rsteg_core::CryptoScheme;
use rsteg_crypto_aead::XChaCha20Argon2id;
use std::time::Instant;

/// `N` samples per scenario. Higher → tighter bound but slower test.
const N: usize = 64;
/// Warmup runs discarded before measurement.
const WARMUP: usize = 8;

fn median_ns<F: FnMut()>(mut f: F, n: usize, warmup: usize) -> u128 {
    for _ in 0..warmup {
        f();
    }
    let mut samples = Vec::with_capacity(n);
    for _ in 0..n {
        let t = Instant::now();
        f();
        samples.push(t.elapsed().as_nanos());
    }
    samples.sort_unstable();
    samples[samples.len() / 2]
}

#[test]
#[ignore = "timing-sensitive; runs ~1s; not in default test set"]
fn open_timing_invariant_across_failure_modes() {
    let s = XChaCha20Argon2id::new_testing();
    let aad = [0u8; 32];
    let ct_good = s.seal(b"plaintext", b"right-password", &aad).unwrap();

    // Scenario A: unknown KDF id. Fails at the inner-header check.
    let mut ct_bad_magic = ct_good.clone();
    ct_bad_magic[0] = 0x7F;

    // Scenario B: valid header, tampered ciphertext. Fails at AEAD tag compare.
    let mut ct_bad_tag = ct_good.clone();
    let tag_off = ct_bad_tag.len() - 16;
    ct_bad_tag[tag_off] ^= 0x01;

    // Scenario C: valid header + valid tag shape, wrong passphrase. Fails
    // at AEAD tag compare with a different (wrong) key.
    let ct_wrong_pw = ct_good.clone();

    let t_a = median_ns(
        || {
            let _ = s.open(&ct_bad_magic, b"right-password", &aad);
        },
        N,
        WARMUP,
    );
    let t_b = median_ns(
        || {
            let _ = s.open(&ct_bad_tag, b"right-password", &aad);
        },
        N,
        WARMUP,
    );
    let t_c = median_ns(
        || {
            let _ = s.open(&ct_wrong_pw, b"wrong-password", &aad);
        },
        N,
        WARMUP,
    );

    let lo = t_a.min(t_b).min(t_c);
    let hi = t_a.max(t_b).max(t_c);
    // Loose: 3× allows for GC-induced jitter. Tight enough to catch a
    // short-circuit that would make the magic-check path ~10× faster.
    assert!(
        hi <= lo * 3,
        "open timing diverges: bad_magic={}ns bad_tag={}ns wrong_pw={}ns",
        t_a,
        t_b,
        t_c
    );
}
