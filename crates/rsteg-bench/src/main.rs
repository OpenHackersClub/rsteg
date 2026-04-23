//! `rsteg-bench` — benchmark harness for the rsteg-cli binary against
//! steghide and stegano-cli. Dev-only (publish = false).
//!
//! Per specs/08-benchmarking.md:
//! - Wall-clock + CPU time (user+sys) + peak RSS per subprocess invocation.
//! - 3 warmups, 11 measured runs → median / p95 / MAD.
//! - Output: JSON and Markdown table.
//!
//! This is the Table A (subprocess) comparison. Table B (in-process) is a
//! future add. The claim in the README is sourced from Table A.

#![deny(unsafe_code)]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

mod corpus;
mod rusage;

use rusage::rusage_delta;

const WARMUP: usize = 3;
const MEASURED: usize = 11;

fn main() {
    let mut parser = lexopt::Parser::from_env();
    let mut case_filter: Option<String> = None;
    let mut format: String = "markdown".into();
    let mut tools: Vec<String> = Vec::new();
    let mut out_path: Option<PathBuf> = None;
    let mut all = false;
    while let Some(a) = parser.next().unwrap_or(None) {
        use lexopt::prelude::*;
        match a {
            Long("case") => case_filter = Some(parser.value().unwrap().into_string().unwrap()),
            Long("tool") => tools.push(parser.value().unwrap().into_string().unwrap()),
            Long("format") => format = parser.value().unwrap().into_string().unwrap(),
            Long("all") => all = true,
            Long("out") => out_path = Some(parser.value().unwrap().into()),
            Long("help") | Short('h') => {
                print_help();
                return;
            }
            _ => {}
        }
    }

    let cases = corpus::all_cases();
    let cases: Vec<_> = cases
        .into_iter()
        .filter(|c| all || case_filter.as_deref().map_or(true, |f| c.id == f))
        .collect();

    let tool_filter: Vec<Tool> = if tools.is_empty() {
        all_tools()
    } else {
        tools.iter().filter_map(|t| Tool::from_id(t)).collect()
    };

    let corpus_root = std::env::temp_dir().join("rsteg-bench-corpus");
    std::fs::create_dir_all(&corpus_root).unwrap();

    let mut all_runs: Vec<RunReport> = Vec::new();
    for case in &cases {
        let cover_path = corpus_root.join(format!("{}.{}", case.id, case.ext));
        let cover_bytes = case.build_cover();
        std::fs::write(&cover_path, &cover_bytes).unwrap();

        for &payload_size in case.payload_sizes {
            let payload_path = corpus_root.join(format!("payload_{payload_size}.bin"));
            let payload = payload_seed(payload_size);
            std::fs::write(&payload_path, &payload).unwrap();

            for &tool in &tool_filter {
                if !tool.supports(case.format) {
                    continue;
                }
                for op in [Op::Embed, Op::Extract] {
                    if let Some(r) = run_bench(tool, op, case, &cover_path, &payload_path, payload_size) {
                        println!(
                            "  {:10} {:8} {:5} p50={:>7.1}ms p95={:>7.1}ms cpu={:>7.1}ms rss={:>4}MB",
                            case.id,
                            op.id(),
                            tool.id(),
                            ns_to_ms(r.wall_p50),
                            ns_to_ms(r.wall_p95),
                            ns_to_ms(r.cpu_p50),
                            r.peak_rss / (1024 * 1024),
                        );
                        all_runs.push(r);
                    }
                }
            }
        }
    }

    let report = match format.as_str() {
        "json" => serialize_json(&all_runs),
        "markdown" | _ => render_markdown(&all_runs),
    };

    if let Some(p) = out_path {
        std::fs::write(&p, &report).unwrap();
        eprintln!("wrote {} bytes to {:?}", report.len(), p);
    } else {
        std::io::stdout().write_all(report.as_bytes()).unwrap();
    }
}

fn print_help() {
    println!(
        r#"rsteg-bench — bench harness
USAGE:
  rsteg-bench [--all | --case ID] [--tool rsteg|steghide|stegano] ...
              [--format markdown|json] [--out PATH]"#
    );
}

// ---------------- tools ----------------

#[derive(Clone, Copy, PartialEq)]
enum Tool {
    Rsteg,
    Steghide,
    Stegano,
}

impl Tool {
    fn id(self) -> &'static str {
        match self {
            Self::Rsteg => "rsteg",
            Self::Steghide => "steghide",
            Self::Stegano => "stegano",
        }
    }
    fn from_id(s: &str) -> Option<Self> {
        Some(match s {
            "rsteg" | "rsteg-cli" => Self::Rsteg,
            "steghide" => Self::Steghide,
            "stegano" | "stegano-cli" => Self::Stegano,
            _ => return None,
        })
    }
    fn supports(self, fmt: Format) -> bool {
        match self {
            Self::Rsteg => true,
            // steghide: JPEG/BMP/WAV/AU; no PNG.
            Self::Steghide => matches!(fmt, Format::Bmp | Format::Wav),
            // stegano-cli (as installed): PNG + WAV.
            Self::Stegano => matches!(fmt, Format::Png | Format::Wav),
        }
    }
    fn available(self) -> bool {
        match self {
            Self::Rsteg => rsteg_binary_path().exists(),
            Self::Steghide => which("steghide").is_some(),
            Self::Stegano => which("stegano").is_some(),
        }
    }
}

fn all_tools() -> Vec<Tool> {
    [Tool::Rsteg, Tool::Steghide, Tool::Stegano]
        .into_iter()
        .filter(|t| t.available())
        .collect()
}

fn which(cmd: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let p = dir.join(cmd);
        if p.exists() {
            return Some(p);
        }
    }
    None
}

fn rsteg_binary_path() -> PathBuf {
    // Prefer the release build if it exists; fall back to debug.
    let here = std::env::current_exe().unwrap();
    let root = here
        .ancestors()
        .find(|p| p.join("target").exists())
        .unwrap_or(Path::new("."))
        .to_path_buf();
    let rel = root.join("target/release/rsteg");
    if rel.exists() {
        return rel;
    }
    root.join("target/debug/rsteg")
}

// ---------------- cases ----------------

#[derive(Clone, Copy, PartialEq)]
pub enum Format {
    Bmp,
    Wav,
    Png,
}

impl Format {
    fn id(self) -> &'static str {
        match self {
            Self::Bmp => "bmp",
            Self::Wav => "wav",
            Self::Png => "png",
        }
    }
}

// ---------------- run ----------------

#[derive(Clone, Copy)]
enum Op {
    Embed,
    Extract,
}
impl Op {
    fn id(self) -> &'static str {
        match self {
            Self::Embed => "embed",
            Self::Extract => "extract",
        }
    }
}

#[derive(Clone)]
struct RunReport {
    case: String,
    operation: String,
    payload_size: usize,
    tool: String,
    wall_p50: u128,
    wall_p95: u128,
    wall_mad: u128,
    cpu_p50: u128,
    peak_rss: u64,
    output_size_delta: i64,
}

fn run_bench(
    tool: Tool,
    op: Op,
    case: &corpus::Case,
    cover: &Path,
    payload: &Path,
    payload_size: usize,
) -> Option<RunReport> {
    // Prepare a single stego so extract has an input.
    let work = std::env::temp_dir().join(format!(
        "rsteg-bench-{}-{}-{}-{}-{}",
        case.id,
        tool.id(),
        op.id(),
        payload_size,
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).ok()?;
    let stego_path = work.join(format!("stego.{}", case.ext));
    let recovered = work.join("recovered.bin");

    if matches!(op, Op::Extract) {
        // Build a stego via the same tool so extract has matching input.
        let cmd = embed_cmd(tool, case, cover, payload, &stego_path)?;
        let _ = run_one(cmd).ok();
        if !stego_path.exists() {
            let _ = std::fs::remove_dir_all(&work);
            return None;
        }
    }

    let mut wall = Vec::with_capacity(MEASURED);
    let mut cpu = Vec::with_capacity(MEASURED);
    let mut peak_rss = 0u64;
    let mut size_delta: i64 = 0;

    for i in 0..(WARMUP + MEASURED) {
        let cmd = match op {
            Op::Embed => embed_cmd(tool, case, cover, payload, &stego_path)?,
            Op::Extract => extract_cmd(tool, case, &stego_path, &recovered)?,
        };
        let r = run_one(cmd).ok()?;
        if i >= WARMUP {
            wall.push(r.wall_ns);
            cpu.push(r.cpu_ns);
            if r.peak_rss > peak_rss {
                peak_rss = r.peak_rss;
            }
        }
        if matches!(op, Op::Embed) && stego_path.exists() {
            let cover_len = std::fs::metadata(cover).ok()?.len() as i64;
            let stego_len = std::fs::metadata(&stego_path).ok()?.len() as i64;
            size_delta = stego_len - cover_len;
        }
    }
    wall.sort_unstable();
    cpu.sort_unstable();
    let wall_p50 = wall[wall.len() / 2];
    let wall_p95 = wall[(wall.len() * 95) / 100];
    let cpu_p50 = cpu[cpu.len() / 2];
    let wall_mad = mad(&wall);

    let _ = std::fs::remove_dir_all(&work);

    Some(RunReport {
        case: case.id.to_string(),
        operation: op.id().to_string(),
        payload_size,
        tool: tool.id().to_string(),
        wall_p50,
        wall_p95,
        wall_mad,
        cpu_p50,
        peak_rss,
        output_size_delta: size_delta,
    })
}

fn embed_cmd(tool: Tool, case: &corpus::Case, cover: &Path, payload: &Path, out: &Path) -> Option<Command> {
    let mut c = match tool {
        Tool::Rsteg => {
            let mut c = Command::new(rsteg_binary_path());
            c.args(["embed", "--in"]).arg(cover)
                .arg("--payload").arg(payload)
                .arg("--out").arg(out)
                .arg("--force");
            c
        }
        Tool::Steghide => {
            let mut c = Command::new("steghide");
            c.args(["embed", "-cf"]).arg(cover)
                .arg("-ef").arg(payload)
                .arg("-sf").arg(out)
                .args(["-p", "bench", "-f", "-Z", "-e", "none"]);
            c
        }
        Tool::Stegano => {
            // stegano hide --in <cover> --data <payload> --out <out>. Stegano
            // prompts for a password if none is given and panics when stdin
            // isn't a terminal, so we always pass `--password bench`.
            if !matches!(case.format, Format::Png | Format::Wav) {
                return None;
            }
            let mut c = Command::new("stegano");
            c.args(["hide", "--in"]).arg(cover)
                .arg("--data").arg(payload)
                .arg("--out").arg(out)
                .args(["--password", "bench"]);
            c
        }
    };
    c.stdout(Stdio::null()).stderr(Stdio::null()).stdin(Stdio::null());
    Some(c)
}

fn extract_cmd(tool: Tool, case: &corpus::Case, stego: &Path, out: &Path) -> Option<Command> {
    let mut c = match tool {
        Tool::Rsteg => {
            let mut c = Command::new(rsteg_binary_path());
            c.args(["extract", "--in"]).arg(stego)
                .arg("--out").arg(out)
                .arg("--force");
            c
        }
        Tool::Steghide => {
            let mut c = Command::new("steghide");
            c.args(["extract", "-sf"]).arg(stego)
                .arg("-xf").arg(out)
                .args(["-p", "bench", "-f"]);
            c
        }
        Tool::Stegano => {
            if !matches!(case.format, Format::Png | Format::Wav) {
                return None;
            }
            // stegano unveil --in <stego> --out <output folder>. Same
            // password contract as `hide`.
            let out_dir = out.parent().unwrap_or(Path::new(".")).to_path_buf();
            let mut c = Command::new("stegano");
            c.args(["unveil", "--in"]).arg(stego)
                .arg("--out").arg(out_dir)
                .args(["--password", "bench"]);
            c
        }
    };
    c.stdout(Stdio::null()).stderr(Stdio::null()).stdin(Stdio::null());
    Some(c)
}

struct OneRun {
    wall_ns: u128,
    cpu_ns: u128,
    peak_rss: u64,
}

fn run_one(mut cmd: Command) -> Result<OneRun, std::io::Error> {
    let before = rusage::snapshot();
    let t = Instant::now();
    let status = cmd.status()?;
    let wall = t.elapsed();
    let after = rusage::snapshot();
    if !status.success() {
        return Err(std::io::Error::other(format!("exit {status:?}")));
    }
    let (cpu_ns, peak_rss) = rusage_delta(before, after);
    Ok(OneRun {
        wall_ns: wall.as_nanos(),
        cpu_ns,
        peak_rss,
    })
}

fn ns_to_ms(ns: u128) -> f64 {
    ns as f64 / 1_000_000.0
}

fn mad(sorted: &[u128]) -> u128 {
    let med = sorted[sorted.len() / 2];
    let mut dev: Vec<u128> = sorted
        .iter()
        .map(|v| if *v >= med { v - med } else { med - v })
        .collect();
    dev.sort_unstable();
    dev[dev.len() / 2]
}

fn payload_seed(n: usize) -> Vec<u8> {
    let mut v = Vec::with_capacity(n);
    let mut s: u64 = 0x7257_E4_CAFE_F00D;
    while v.len() < n {
        s = s.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = s ^ (s >> 30);
        z = z.wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = z ^ (z >> 27);
        z = z.wrapping_mul(0x94D0_49BB_1331_11EB);
        z = z ^ (z >> 31);
        for b in z.to_le_bytes() {
            if v.len() == n {
                break;
            }
            v.push(b);
        }
    }
    v
}

fn render_markdown(runs: &[RunReport]) -> String {
    let mut s = String::new();
    s.push_str("# rsteg bench report\n\n");
    s.push_str(&format!(
        "Platform: {}/{} ncpu={}\n\n",
        std::env::consts::OS,
        std::env::consts::ARCH,
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(0)
    ));
    // Group by (case, operation, payload_size).
    use std::collections::BTreeMap;
    let mut grouped: BTreeMap<(String, String, usize), Vec<&RunReport>> = BTreeMap::new();
    for r in runs {
        grouped
            .entry((r.case.clone(), r.operation.clone(), r.payload_size))
            .or_default()
            .push(r);
    }
    for ((case, op, size), group) in grouped {
        s.push_str(&format!(
            "## {} — {} {}\n\n",
            case,
            op,
            human_size(size),
        ));
        s.push_str("| Tool | Wall p50 (ms) | Wall p95 | Wall MAD | CPU p50 | Peak RSS (MB) | Δ size |\n");
        s.push_str("|------|---------------|----------|----------|---------|---------------|--------|\n");
        for r in &group {
            s.push_str(&format!(
                "| {} | {:.2} | {:.2} | {:.2} | {:.2} | {} | {} |\n",
                r.tool,
                ns_to_ms(r.wall_p50),
                ns_to_ms(r.wall_p95),
                ns_to_ms(r.wall_mad),
                ns_to_ms(r.cpu_p50),
                r.peak_rss / (1024 * 1024),
                r.output_size_delta,
            ));
        }
        s.push('\n');
    }
    s
}

fn human_size(n: usize) -> String {
    if n >= 1_000_000 {
        format!("{:.1} MB", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{} KB", n / 1_000)
    } else {
        format!("{n} B")
    }
}

fn serialize_json(runs: &[RunReport]) -> String {
    let mut s = String::from("{\n  \"runs\": [\n");
    for (i, r) in runs.iter().enumerate() {
        if i > 0 {
            s.push_str(",\n");
        }
        s.push_str(&format!(
            "    {{\"case\":\"{}\",\"operation\":\"{}\",\"payload_size\":{},\"tool\":\"{}\",\"wall_ns_p50\":{},\"wall_ns_p95\":{},\"wall_ns_mad\":{},\"cpu_ns_p50\":{},\"peak_rss_bytes\":{},\"output_size_delta\":{}}}",
            r.case, r.operation, r.payload_size, r.tool,
            r.wall_p50, r.wall_p95, r.wall_mad, r.cpu_p50,
            r.peak_rss, r.output_size_delta,
        ));
    }
    s.push_str("\n  ]\n}\n");
    s
}
