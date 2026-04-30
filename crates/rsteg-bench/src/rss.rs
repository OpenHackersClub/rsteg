//! Cross-platform peak-RSS sampler for the soak harness.
//!
//! No `unsafe`, no `libc` dep — keeps `rsteg-bench` aligned with the
//! workspace `#![deny(unsafe_code)]` invariant.
//!
//! - **Linux**: read `/proc/self/statm`, multiply resident pages × page size.
//! - **macOS**: shell out to `ps -o rss= -p $$` (KB → bytes). Slow per call
//!   (~10–30 ms) but this harness samples on the order of seconds, not
//!   microseconds — accuracy matters more than throughput.
//! - **Other**: returns 0. The soak report flags this so a non-zero RSS
//!   isn't silently invented from thin air.

#![deny(unsafe_code)]

use std::process::Command;

/// Resident set size in bytes for the current process.
///
/// Returns `None` on platforms with no implementation. The soak driver
/// surfaces that in the final report — we don't fabricate a number.
#[must_use]
pub fn current_rss_bytes() -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        return read_linux_statm();
    }
    #[cfg(target_os = "macos")]
    {
        return read_macos_ps();
    }
    #[allow(unreachable_code)]
    None
}

#[cfg(target_os = "linux")]
fn read_linux_statm() -> Option<u64> {
    let raw = std::fs::read_to_string("/proc/self/statm").ok()?;
    // Fields: size resident shared text lib data dt — all in pages.
    let resident_pages: u64 = raw.split_whitespace().nth(1)?.parse().ok()?;
    // 4 KiB page size is the de facto Linux default. We don't shell out to
    // `getconf PAGESIZE` to avoid spawning a process per sample on the hot
    // path; an oddball page size would skew the numbers proportionally
    // and the drift/variance ratios stay valid regardless.
    Some(resident_pages * 4096)
}

#[cfg(target_os = "macos")]
fn read_macos_ps() -> Option<u64> {
    let pid = std::process::id().to_string();
    let out = Command::new("ps")
        .args(["-o", "rss=", "-p", &pid])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let kb: u64 = std::str::from_utf8(&out.stdout)
        .ok()?
        .trim()
        .parse()
        .ok()?;
    Some(kb * 1024)
}

/// Min / mean / max / stddev / coefficient-of-variation summary.
#[derive(Clone, Debug)]
pub struct Summary {
    pub n: usize,
    pub min: u64,
    pub max: u64,
    pub mean: f64,
    pub stddev: f64,
    /// Coefficient of variation = stddev / mean (unitless).
    pub cv: f64,
}

impl Summary {
    #[must_use]
    pub fn from(samples: &[u64]) -> Self {
        if samples.is_empty() {
            return Self {
                n: 0,
                min: 0,
                max: 0,
                mean: 0.0,
                stddev: 0.0,
                cv: 0.0,
            };
        }
        let n = samples.len();
        let mut min = u64::MAX;
        let mut max = 0u64;
        let mut sum: u128 = 0;
        for &s in samples {
            if s < min {
                min = s;
            }
            if s > max {
                max = s;
            }
            sum += u128::from(s);
        }
        #[allow(clippy::cast_precision_loss)]
        let mean = (sum as f64) / (n as f64);
        let var: f64 = samples
            .iter()
            .map(|&s| {
                let d = (s as f64) - mean;
                d * d
            })
            .sum::<f64>()
            / (n as f64);
        let stddev = var.sqrt();
        let cv = if mean > 0.0 { stddev / mean } else { 0.0 };
        Self {
            n,
            min,
            max,
            mean,
            stddev,
            cv,
        }
    }
}

/// Drift between the first and last 10% of the sample window.
///
/// `(tail_mean - head_mean) / head_mean` — positive values mean RSS grew
/// over the run, the canonical leak signature.
#[must_use]
pub fn drift_ratio(samples: &[u64]) -> Option<f64> {
    if samples.len() < 20 {
        return None;
    }
    let edge = samples.len() / 10;
    let head = &samples[..edge];
    let tail = &samples[samples.len() - edge..];
    let head_mean = Summary::from(head).mean;
    let tail_mean = Summary::from(tail).mean;
    if head_mean <= 0.0 {
        return None;
    }
    Some((tail_mean - head_mean) / head_mean)
}

#[cfg(test)]
mod tests {
    use super::{drift_ratio, Summary};

    #[test]
    fn summary_handles_empty() {
        let s = Summary::from(&[]);
        assert_eq!(s.n, 0);
        assert_eq!(s.cv, 0.0);
    }

    #[test]
    fn summary_basic_stats() {
        let s = Summary::from(&[100, 200, 300]);
        assert_eq!(s.n, 3);
        assert_eq!(s.min, 100);
        assert_eq!(s.max, 300);
        assert!((s.mean - 200.0).abs() < 1e-9);
        // Population stddev of {100,200,300} = sqrt((10000+0+10000)/3) ≈ 81.65
        assert!((s.stddev - 81.649_658_092_772_6).abs() < 1e-6);
    }

    #[test]
    fn drift_returns_none_for_short_series() {
        assert!(drift_ratio(&[1, 2, 3]).is_none());
    }

    #[test]
    fn drift_positive_when_tail_grows() {
        // 100 samples — head 10 averages 100, tail 10 averages 200.
        let mut v = vec![100u64; 50];
        v.extend(vec![200u64; 50]);
        let d = drift_ratio(&v).unwrap();
        assert!((d - 1.0).abs() < 1e-9, "drift was {d}");
    }
}
