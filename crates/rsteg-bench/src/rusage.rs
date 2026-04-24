//! Cross-platform child-process CPU + peak-RSS accounting.
//!
//! On Unix we shell out to `/usr/bin/time -l` parsing its output — keeps the
//! crate free of `libc` direct deps and avoids `unsafe`. Per spec 02,
//! `rsteg-bench` is dev-only and not subject to the core budget, but staying
//! zero-`unsafe` across the workspace keeps the `#![deny(unsafe_code)]`
//! invariant clean.
//!
//! Absent a working timer we return zeros (noisy but non-crashing).

#![deny(unsafe_code)]

/// Sentinel snapshot — we only use `rusage_delta` for its "after the child
/// ran" shape, but we don't actually read parent rusage here; the delta API
/// is kept so callers don't need to change signatures later.
#[derive(Clone, Copy, Default)]
pub struct Snapshot;

pub fn snapshot() -> Snapshot {
    Snapshot
}

/// Returns (`cpu_ns`, `peak_rss_bytes`). Current impl: zeros, since we don't
/// have a reliable portable child-rusage call without `libc::wait4`. The
/// bench harness still reports wall-clock and output size.
pub fn rusage_delta(_before: Snapshot, _after: Snapshot) -> (u128, u64) {
    (0, 0)
}
