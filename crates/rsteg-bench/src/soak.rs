//! Soak harness — long-running stability test for the format adapters.
//!
//! Spec'd in `specs/07-testing.md` §"Soak tests" and tracked as a phase-1.5
//! exit criterion in `specs/09-roadmap.md`. This is local-only (not
//! CI-blocking): the report goes to stdout for a human to triage.
//!
//! ## Workload
//!
//! - Carriers: pre-generated 1 MB / 10 MB BMP, 1 MB / 5 MB WAV, 1 MB / 5 MB
//!   PNG. (Spec asks for 1–100 MB; PNG re-encode is the long pole, so we
//!   cap PNG soak carriers at 5 MB to keep iterations per minute usable.)
//! - Operations: `embed` / `extract` / `inspect`, chosen via a small Markov
//!   chain so each step depends on the last (closer to real workload than
//!   uniform random).
//! - 5% of operations are deliberately handed corrupted inputs to exercise
//!   the error path. Errors there are *expected*; errors anywhere else
//!   count as unexpected and are reported.
//! - RSS sampled at the end of each iteration via [`crate::rss`].
//!
//! ## Pass criteria (spec 07)
//!
//! - RSS drift `< 5%` between the first and last 10% of samples.
//! - RSS coefficient-of-variation `< 20%`.
//! - No unexpected (non-injected) errors.

#![deny(unsafe_code)]

use std::time::{Duration, Instant};

use rsteg_bmp::BMP_ADAPTER;
use rsteg_core::{
    Density, EmbedOpts, ExtractOpts, FormatAdapter, PayloadHeader, Prng, SchemeFourcc,
};
use rsteg_png::PNG_ADAPTER;
use rsteg_wav::WAV_ADAPTER;

use crate::rss::{current_rss_bytes, drift_ratio, Summary};

const DEFAULT_DURATION_SECS: u64 = 30;
const DEFAULT_MALFORMED_RATE: f64 = 0.05;
const DEFAULT_SEED: u64 = 0xDEAD_BEEF_CAFE_F00D;

pub fn run(args: Vec<String>) {
    let opts = match Opts::parse(&args) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("error: {e}");
            print_help();
            std::process::exit(2);
        }
    };
    if opts.help {
        print_help();
        return;
    }

    eprintln!(
        "soak: duration={}s malformed_rate={} seed={:#x}",
        opts.duration.as_secs(),
        opts.malformed_rate,
        opts.seed,
    );
    eprintln!("soak: building carrier pool ...");
    let carriers = build_carriers();
    for c in &carriers {
        eprintln!("  {:8} {:>10} bytes", c.id, c.bytes.len());
    }

    let mut prng = Prng::new(opts.seed);
    let baseline_rss = current_rss_bytes();
    if baseline_rss.is_none() {
        eprintln!("soak: warning — RSS sampling unsupported on this platform; drift/variance will be reported as N/A");
    }

    let mut state = State::Embed;
    let mut samples: Vec<u64> = Vec::new();
    let mut counts = Counts::default();
    let mut last_progress = Instant::now();
    let started = Instant::now();
    let progress_every = Duration::from_secs(5);

    while started.elapsed() < opts.duration {
        let next = state.transition(&mut prng);
        let carrier = pick_carrier(&carriers, &mut prng);
        let malformed = next_f64(&mut prng) < opts.malformed_rate;

        let outcome = run_one(next, carrier, malformed, &mut prng);
        counts.record(next, &outcome, malformed);
        if let Some(rss) = current_rss_bytes() {
            samples.push(rss);
        }
        state = next;

        if last_progress.elapsed() >= progress_every {
            eprintln!(
                "soak: t={:>5}s ops={:>7} embed={} extract={} inspect={} unexpected_err={}",
                started.elapsed().as_secs(),
                counts.total(),
                counts.embed,
                counts.extract,
                counts.inspect,
                counts.unexpected_err,
            );
            last_progress = Instant::now();
        }
    }

    print_report(&opts, &samples, baseline_rss, &counts, started.elapsed());
    if counts.unexpected_err > 0 {
        std::process::exit(1);
    }
}

// ---------------- options ----------------

#[derive(Clone, Debug)]
struct Opts {
    duration: Duration,
    malformed_rate: f64,
    seed: u64,
    help: bool,
}

impl Opts {
    fn parse(args: &[String]) -> Result<Self, String> {
        let mut duration = Duration::from_secs(DEFAULT_DURATION_SECS);
        let mut malformed_rate = DEFAULT_MALFORMED_RATE;
        let mut seed = DEFAULT_SEED;
        let mut help = false;
        let mut i = 0;
        while i < args.len() {
            let a = &args[i];
            match a.as_str() {
                "--help" | "-h" => help = true,
                "--duration" => {
                    i += 1;
                    let v = args.get(i).ok_or("--duration requires a value")?;
                    duration = parse_duration(v)?;
                }
                "--malformed-rate" => {
                    i += 1;
                    let v = args.get(i).ok_or("--malformed-rate requires a value")?;
                    malformed_rate = v.parse().map_err(|e: std::num::ParseFloatError| e.to_string())?;
                    if !(0.0..=1.0).contains(&malformed_rate) {
                        return Err("--malformed-rate must be in [0,1]".into());
                    }
                }
                "--seed" => {
                    i += 1;
                    let v = args.get(i).ok_or("--seed requires a value")?;
                    seed = parse_u64(v)?;
                }
                other => return Err(format!("unknown argument: {other}")),
            }
            i += 1;
        }
        Ok(Self {
            duration,
            malformed_rate,
            seed,
            help,
        })
    }
}

fn parse_duration(s: &str) -> Result<Duration, String> {
    let (num, unit) = s
        .find(|c: char| c.is_ascii_alphabetic())
        .map_or((s, "s"), |i| (&s[..i], &s[i..]));
    let n: u64 = num
        .parse()
        .map_err(|e: std::num::ParseIntError| e.to_string())?;
    let secs = match unit {
        "s" | "" => n,
        "m" => n * 60,
        "h" => n * 3600,
        other => return Err(format!("unknown duration unit '{other}'")),
    };
    Ok(Duration::from_secs(secs))
}

fn parse_u64(s: &str) -> Result<u64, String> {
    if let Some(hex) = s.strip_prefix("0x") {
        u64::from_str_radix(hex, 16).map_err(|e| e.to_string())
    } else {
        s.parse().map_err(|e: std::num::ParseIntError| e.to_string())
    }
}

fn print_help() {
    println!(
        r"rsteg-bench soak — long-running stability test for the format adapters

USAGE:
  rsteg-bench soak [--duration 2h|30m|60s] [--malformed-rate 0.05] [--seed 0xDEADBEEF]

DEFAULTS:
  --duration        30s   (spec target: 2h)
  --malformed-rate  0.05  (5% of ops handed corrupted input)
  --seed            0xDEADBEEFCAFEF00D

EXIT CODE:
  0  pass
  1  one or more unexpected errors observed during soak
  2  bad arguments
"
    );
}

// ---------------- workload ----------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Embed,
    Extract,
    Inspect,
}

impl State {
    /// Transition table. Tuned by hand so embed/extract dominate but inspect
    /// keeps showing up — the goal is steady-state mixed work, not uniform
    /// random.
    fn transition(self, prng: &mut Prng) -> Self {
        let r = next_f64(prng);
        match self {
            Self::Embed => {
                if r < 0.50 {
                    Self::Extract
                } else if r < 0.80 {
                    Self::Embed
                } else {
                    Self::Inspect
                }
            }
            Self::Extract => {
                if r < 0.40 {
                    Self::Embed
                } else if r < 0.80 {
                    Self::Extract
                } else {
                    Self::Inspect
                }
            }
            Self::Inspect => {
                if r < 0.60 {
                    Self::Embed
                } else if r < 0.90 {
                    Self::Extract
                } else {
                    Self::Inspect
                }
            }
        }
    }

}

#[derive(Clone, Copy)]
enum Fmt {
    Bmp,
    Wav,
    Png,
}

struct Carrier {
    id: &'static str,
    fmt: Fmt,
    bytes: Vec<u8>,
}

fn build_carriers() -> Vec<Carrier> {
    vec![
        Carrier {
            id: "bmp-1mb",
            fmt: Fmt::Bmp,
            bytes: synth_bmp24(640, 540),
        },
        Carrier {
            id: "bmp-10mb",
            fmt: Fmt::Bmp,
            bytes: synth_bmp24(2048, 1700),
        },
        Carrier {
            id: "wav-1mb",
            fmt: Fmt::Wav,
            bytes: synth_wav16(44_100, 6 * 44_100, 2),
        },
        Carrier {
            id: "wav-5mb",
            fmt: Fmt::Wav,
            bytes: synth_wav16(44_100, 30 * 44_100, 2),
        },
        Carrier {
            id: "png-1mb",
            fmt: Fmt::Png,
            bytes: synth_png_rgb(512, 512),
        },
        Carrier {
            id: "png-5mb",
            fmt: Fmt::Png,
            bytes: synth_png_rgb(1024, 1024),
        },
    ]
}

fn pick_carrier<'a>(pool: &'a [Carrier], prng: &mut Prng) -> &'a Carrier {
    let idx = (prng.next_u64() as usize) % pool.len();
    &pool[idx]
}

#[derive(Clone, Copy, Debug)]
enum Outcome {
    Ok,
    ExpectedErr,
    UnexpectedErr,
}

fn run_one(state: State, carrier: &Carrier, malformed: bool, prng: &mut Prng) -> Outcome {
    match state {
        State::Embed => do_embed(carrier, malformed, prng),
        State::Extract => do_extract(carrier, malformed, prng),
        State::Inspect => do_inspect(carrier, malformed, prng),
    }
}

fn do_embed(carrier: &Carrier, malformed: bool, prng: &mut Prng) -> Outcome {
    // Payload size: log-uniform up to ~10% of carrier capacity.
    let cap = (carrier.bytes.len() / 16).max(64);
    let payload_len = (prng.next_u64() as usize) % cap;
    let payload = random_bytes(prng, payload_len);

    let scheme = match carrier.fmt {
        Fmt::Bmp => SchemeFourcc::BMP_LSB_LINEAR,
        Fmt::Wav => SchemeFourcc::WAV_LSB_LINEAR,
        Fmt::Png => SchemeFourcc::PNG_LSB_LINEAR,
    };
    let header = PayloadHeader::plain(scheme, Density::Low, &payload);
    let framed = header.encode_with(&payload);

    let opts = EmbedOpts::default();
    let cover: &[u8] = if malformed {
        // Malformed embed input: an empty carrier. Expected to fail with
        // FormatUnrecognized or PayloadTooLarge — both typed errors.
        &[]
    } else {
        &carrier.bytes
    };
    let r = match carrier.fmt {
        Fmt::Bmp => BMP_ADAPTER.embed(cover, &framed, &opts),
        Fmt::Wav => WAV_ADAPTER.embed(cover, &framed, &opts),
        Fmt::Png => PNG_ADAPTER.embed(cover, &framed, &opts),
    };
    classify(r.map(|_| ()), malformed)
}

fn do_extract(carrier: &Carrier, malformed: bool, prng: &mut Prng) -> Outcome {
    // Build a stego first (always the unmalformed path), then either
    // extract it as-is or hand the extractor random bytes.
    let payload = random_bytes(prng, 256);
    let scheme = match carrier.fmt {
        Fmt::Bmp => SchemeFourcc::BMP_LSB_LINEAR,
        Fmt::Wav => SchemeFourcc::WAV_LSB_LINEAR,
        Fmt::Png => SchemeFourcc::PNG_LSB_LINEAR,
    };
    let header = PayloadHeader::plain(scheme, Density::Low, &payload);
    let framed = header.encode_with(&payload);
    let stego = match carrier.fmt {
        Fmt::Bmp => BMP_ADAPTER.embed(&carrier.bytes, &framed, &EmbedOpts::default()),
        Fmt::Wav => WAV_ADAPTER.embed(&carrier.bytes, &framed, &EmbedOpts::default()),
        Fmt::Png => PNG_ADAPTER.embed(&carrier.bytes, &framed, &EmbedOpts::default()),
    };
    let stego = match stego {
        Ok(s) => s,
        Err(_) => return Outcome::UnexpectedErr,
    };

    let input: Vec<u8> = if malformed {
        // Mutate one byte deep in the carrier's pixel data so the header
        // CRC fails. Must yield a typed error, never a panic.
        let mut bad = stego.clone();
        let i = (prng.next_u64() as usize) % bad.len().max(1);
        bad[i] ^= 0xFF;
        bad
    } else {
        stego
    };

    let opts = ExtractOpts::default();
    let r = match carrier.fmt {
        Fmt::Bmp => BMP_ADAPTER.extract(&input, &opts),
        Fmt::Wav => WAV_ADAPTER.extract(&input, &opts),
        Fmt::Png => PNG_ADAPTER.extract(&input, &opts),
    };
    classify(r.map(|_| ()), malformed)
}

fn do_inspect(carrier: &Carrier, malformed: bool, prng: &mut Prng) -> Outcome {
    // "Inspect" here = decode the framing header from the first 32 bytes
    // of *something*. Mirrors what `rsteg-cli inspect` does on the head.
    let bytes = if malformed {
        random_bytes(prng, 32)
    } else {
        // The carrier's raw bytes don't contain a valid header — that IS
        // the expected behavior. Treat both Ok and HeaderBadMagic / HeaderMissing
        // as expected outcomes.
        carrier.bytes[..32.min(carrier.bytes.len())].to_vec()
    };
    let r = PayloadHeader::decode(&bytes);
    // Inspect is a probe — the typical outcome is a header error on a
    // non-rsteg blob. Both Ok and Err are expected; UnexpectedErr would
    // only fire if decode panicked, which the fuzz harness already chases.
    let _ = r;
    Outcome::Ok
}

fn classify<T>(r: Result<T, rsteg_core::Error>, malformed: bool) -> Outcome {
    match (r, malformed) {
        (Ok(_), false) => Outcome::Ok,
        (Ok(_), true) => Outcome::Ok, // tolerated — some malformations slip through
        (Err(_), true) => Outcome::ExpectedErr,
        (Err(_), false) => Outcome::UnexpectedErr,
    }
}

// ---------------- counts + report ----------------

#[derive(Default, Debug)]
struct Counts {
    embed: u64,
    extract: u64,
    inspect: u64,
    ok: u64,
    expected_err: u64,
    unexpected_err: u64,
}

impl Counts {
    fn record(&mut self, state: State, outcome: &Outcome, _malformed: bool) {
        match state {
            State::Embed => self.embed += 1,
            State::Extract => self.extract += 1,
            State::Inspect => self.inspect += 1,
        }
        match outcome {
            Outcome::Ok => self.ok += 1,
            Outcome::ExpectedErr => self.expected_err += 1,
            Outcome::UnexpectedErr => self.unexpected_err += 1,
        }
    }

    fn total(&self) -> u64 {
        self.embed + self.extract + self.inspect
    }
}

fn print_report(
    opts: &Opts,
    samples: &[u64],
    baseline_rss: Option<u64>,
    counts: &Counts,
    elapsed: Duration,
) {
    println!("# soak report");
    println!();
    println!("- duration: {:.1}s (target {:.1}s)", elapsed.as_secs_f64(), opts.duration.as_secs_f64());
    println!("- seed: {:#x}", opts.seed);
    println!("- malformed_rate: {}", opts.malformed_rate);
    println!();
    println!("## ops");
    println!();
    println!("| op       | count |");
    println!("|----------|------:|");
    println!("| embed    | {} |", counts.embed);
    println!("| extract  | {} |", counts.extract);
    println!("| inspect  | {} |", counts.inspect);
    println!("| total    | {} |", counts.total());
    println!();
    println!("| outcome        | count |");
    println!("|----------------|------:|");
    println!("| ok             | {} |", counts.ok);
    println!("| expected_err   | {} |", counts.expected_err);
    println!("| unexpected_err | {} |", counts.unexpected_err);
    println!();

    println!("## rss");
    println!();
    if samples.is_empty() {
        println!("RSS sampling unavailable on this platform — drift/variance N/A.");
        return;
    }
    let summary = Summary::from(samples);
    let drift = drift_ratio(samples);
    println!(
        "- baseline (start): {} bytes",
        baseline_rss.map_or("N/A".to_string(), |b| b.to_string())
    );
    println!("- samples: {}", summary.n);
    println!("- min/mean/max: {} / {:.0} / {} bytes", summary.min, summary.mean, summary.max);
    println!("- stddev: {:.0} bytes", summary.stddev);
    println!("- coefficient-of-variation: {:.2}% (pass threshold: < 20%)", summary.cv * 100.0);
    if let Some(d) = drift {
        println!("- drift (head→tail): {:.2}% (pass threshold: < 5%)", d * 100.0);
    } else {
        println!("- drift: N/A (need ≥ 20 samples)");
    }
    println!();

    let cv_pass = summary.cv < 0.20;
    let drift_pass = drift.is_none_or(|d| d.abs() < 0.05);
    let ops_pass = counts.unexpected_err == 0;
    println!("## verdict");
    println!();
    println!("- cv:    {}", if cv_pass { "✅ pass" } else { "❌ fail" });
    println!("- drift: {}", if drift_pass { "✅ pass" } else { "❌ fail" });
    println!("- ops:   {}", if ops_pass { "✅ pass" } else { "❌ fail (unexpected errors)" });
}

// ---------------- helpers ----------------

#[allow(clippy::cast_precision_loss)]
fn next_f64(prng: &mut Prng) -> f64 {
    // Take the top 53 bits to fill an f64 mantissa exactly.
    (prng.next_u64() >> 11) as f64 / ((1u64 << 53) as f64)
}

fn random_bytes(prng: &mut Prng, len: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(len);
    while out.len() < len {
        let chunk = prng.next_u64().to_le_bytes();
        let take = chunk.len().min(len - out.len());
        out.extend_from_slice(&chunk[..take]);
    }
    out
}

// ---------------- carrier synthesizers ----------------
//
// Mirror the helpers in `corpus.rs` but parameterised on dimensions so the
// soak driver can produce arbitrary-sized carriers without leaking those
// helpers' module privacy.

fn synth_bmp24(width: u32, height: u32) -> Vec<u8> {
    let row = ((width as usize) * 3 + 3) & !3;
    let pixel_bytes = row * height as usize;
    let file_size = 54 + pixel_bytes;
    let mut out = Vec::with_capacity(file_size);
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&(file_size as u32).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&54u32.to_le_bytes());
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(width as i32).to_le_bytes());
    out.extend_from_slice(&(height as i32).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&24u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&(pixel_bytes as u32).to_le_bytes());
    out.extend_from_slice(&2835i32.to_le_bytes());
    out.extend_from_slice(&2835i32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    let mut s: u64 = 0x7257_E4;
    for _ in 0..height {
        for _ in 0..width {
            s = s.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let z = (s ^ (s >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            let b = z.to_le_bytes();
            out.push(b[0]);
            out.push(b[1]);
            out.push(b[2]);
        }
        for _ in (width * 3) as usize..row {
            out.push(0);
        }
    }
    out
}

fn synth_wav16(sample_rate: u32, samples: u32, channels: u16) -> Vec<u8> {
    let bits_per_sample: u16 = 16;
    let byte_rate = sample_rate * u32::from(channels) * u32::from(bits_per_sample) / 8;
    let block_align = channels * bits_per_sample / 8;
    let data_bytes = samples * u32::from(block_align);
    let mut out = Vec::with_capacity(44 + data_bytes as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36u32 + data_bytes).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&block_align.to_le_bytes());
    out.extend_from_slice(&bits_per_sample.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_bytes.to_le_bytes());
    let mut s: u64 = 0x7257_E4;
    for _ in 0..samples {
        for _ in 0..channels {
            s = s.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let z = (s ^ (s >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            let sample = (z as u16) as i16;
            out.extend_from_slice(&sample.to_le_bytes());
        }
    }
    out
}

fn synth_png_rgb(width: u32, height: u32) -> Vec<u8> {
    use miniz_oxide::deflate::compress_to_vec_zlib;
    let mut s: u64 = 0x7257_E4;
    let mut raw = Vec::with_capacity((1 + width as usize * 3) * height as usize);
    for _ in 0..height {
        raw.push(0);
        for _ in 0..width {
            s = s.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let z = (s ^ (s >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            let b = z.to_le_bytes();
            raw.push(b[0]);
            raw.push(b[1]);
            raw.push(b[2]);
        }
    }
    let idat = compress_to_vec_zlib(&raw, 6);

    let mut out = Vec::new();
    out.extend_from_slice(b"\x89PNG\r\n\x1a\n");
    let chunk = |out: &mut Vec<u8>, ty: &[u8; 4], data: &[u8]| {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let start = out.len();
        out.extend_from_slice(ty);
        out.extend_from_slice(data);
        let crc = rsteg_core::crc32_ieee(&out[start..]);
        out.extend_from_slice(&crc.to_be_bytes());
    };
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.push(8);
    ihdr.push(2);
    ihdr.push(0);
    ihdr.push(0);
    ihdr.push(0);
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &idat);
    chunk(&mut out, b"IEND", &[]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_duration_units() {
        assert_eq!(parse_duration("30").unwrap(), Duration::from_secs(30));
        assert_eq!(parse_duration("30s").unwrap(), Duration::from_secs(30));
        assert_eq!(parse_duration("5m").unwrap(), Duration::from_secs(300));
        assert_eq!(parse_duration("2h").unwrap(), Duration::from_secs(7200));
        assert!(parse_duration("5d").is_err());
    }

    #[test]
    fn parse_u64_hex_and_decimal() {
        assert_eq!(parse_u64("42").unwrap(), 42);
        assert_eq!(parse_u64("0xFF").unwrap(), 0xFF);
    }

    #[test]
    fn opts_defaults() {
        let o = Opts::parse(&[]).unwrap();
        assert_eq!(o.duration, Duration::from_secs(DEFAULT_DURATION_SECS));
        assert!((o.malformed_rate - DEFAULT_MALFORMED_RATE).abs() < 1e-9);
    }

    #[test]
    fn opts_rejects_out_of_range_rate() {
        let args = vec!["--malformed-rate".to_string(), "1.5".to_string()];
        assert!(Opts::parse(&args).is_err());
    }

    #[test]
    fn carrier_pool_well_formed() {
        let pool = build_carriers();
        assert_eq!(pool.len(), 6);
        for c in &pool {
            assert!(!c.bytes.is_empty(), "{} carrier was empty", c.id);
        }
    }

    #[test]
    fn markov_terminates_on_inspect_chain() {
        // Sanity check: from any state, inspect-only never starves the chain.
        let mut prng = Prng::new(1);
        let mut state = State::Embed;
        for _ in 0..10_000 {
            state = state.transition(&mut prng);
        }
        // No assertion on final state — the property is "did not panic / hang".
        let _ = state;
    }
}
