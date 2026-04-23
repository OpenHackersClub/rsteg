## Benchmarking

Goal: be **fastest** and **lowest peak memory** of the three tools on a standard corpus, with a reproducible harness anyone can run.

No `criterion` (transitive-dep weight). Custom harness in `rsteg-bench/` (dev-only, `publish = false`).

### What we measure

Per (tool × format × payload-size × operation):

1. **Wall clock**, ns, median + MAD of N runs after W warmup runs.
2. **CPU time** (user + sys), via `getrusage(RUSAGE_SELF)` on Linux/macOS, `GetProcessTimes` on Windows. **This is the primary metric for comparisons on shared CI runners** — it's much more stable than wall-clock.
3. **Peak resident set size (RSS)**, via `getrusage.ru_maxrss` **for subprocess runs only**. See methodology note below on why in-process peak is unreliable.
4. **Output size delta**, bytes. Stego size minus cover size.
5. **Percentiles**: p50 and p95 reported. Tail (p95) matters more than median for an interactive tool.
6. **Carrier visual quality** (phase 3): PSNR between cover and stego for image formats.

### Comparison layout (revised from earlier draft)

Reviewer flag: earlier layout mixed in-process and subprocess tools in the same table, which is apples-to-oranges (subprocess adds `fork+exec` + dynamic linker + CRT init). Revised:

**Table A — subprocess comparison** (real user experience):
- `rsteg-cli` (our binary via `std::process::Command`)
- `steghide` (via PATH)
- `stegano-cli` if available

**Table B — in-process comparison** (library overhead characterization):
- `rsteg` (library direct)
- `stegano-core` (dev dep)

The README headline claim is sourced from Table A. Table B is published alongside as a diagnostic, never cited in isolation.

### Corpus

Checked in under `corpus/bench/` — small templates; bench harness generates the full size carriers deterministically via splitmix64 (seed `0x7257_E4`) and caches to `target/rsteg-bench-corpus/`.

| Id | Format | Dimensions / duration | Size | Payload sizes |
|----|--------|------------------------|------|----------------|
| `bmp-tiny`     | BMP 24-bit   | 128×128    | 48 KB  | 16 B, 1 KB, 5 KB |
| `bmp-small`    | BMP 24-bit   | 512×512    | 786 KB | 1 KB, 10 KB, 90 KB |
| `bmp-large`    | BMP 24-bit   | 2048×2048  | 12 MB  | 1 KB, 100 KB, 1 MB |
| `wav-short`    | WAV 16-stereo 44.1 kHz | 5 s   | 882 KB | 1 KB, 50 KB |
| `wav-long`     | WAV 16-stereo 44.1 kHz | 60 s  | 10 MB  | 1 KB, 500 KB |
| `wav-8bit`     | WAV 8-mono 44.1 kHz    | 30 s  | 1.3 MB | 1 KB, 100 KB |
| `png-synthetic-small` | PNG RGBA synthetic  | 512×512    | ~1 MB  | 1 KB, 50 KB |
| `png-synthetic-large` | PNG RGBA synthetic  | 2048×2048  | ~16 MB | 100 KB, 1 MB |
| `png-photo-small`     | PNG RGB from real photo | 512×512 | ~400 KB | 1 KB, 20 KB |
| `png-photo-large`     | PNG RGB from real photo | 2048×2048 | ~5 MB | 50 KB, 500 KB |
| `jpeg-small`   | JPEG q=85    | 512×512    | ~60 KB | 1 KB (phase 2) |

**Real-photo PNGs** added per reviewer finding: synthetic random pixel data is adversarial for DEFLATE (incompressible), which makes `miniz_oxide` look slower and distorts user-facing numbers. The real photos are a CC0/public-domain checked-in fixture (small, ~200 KB base) scaled up deterministically.

### Operations

- `embed` cold: fresh process (subprocess tools) / fresh allocator (in-process).
- `embed` hot: repeated in the same process (in-process tools only; reported separately).
- `extract` cold / hot.
- `capacity` (our tool only; others don't expose it separately).
- `embed-encrypted`: with `--password` (PBKDF2/Argon2id impact visible; run separately from the non-crypto numbers to isolate steganography-core cost).

### Methodology

- **Warmup**: 3 iterations discarded.
- **Measured**: 11 iterations. Report median, MAD, p95.
- **Process isolation** for subprocess adapters: fresh `std::process::Command` per iteration.
- **Allocator**: default system allocator for fairness. Bench harness explicitly pins to system; does not use a custom allocator.
- **Thermal / load**: harness prints a stderr warning (does not refuse) if load avg > ncpu/2 or if CPU freq scaling is detected; on CI the warning is purely informational. Reviewer flagged the earlier 0.5-load hard-stop as CI-unfriendly.
- **Deterministic inputs**: same payload bytes across tools. Same cover bytes.
- **Encryption disabled by default** in the main run (we measure steganography, not crypto). The `embed-encrypted` / `extract-encrypted` sub-runs measure AEAD overhead separately.

### Peak RSS measurement (revised)

Reviewer flag: `getrusage.ru_maxrss` is lifetime peak of the process. For an in-process benchmark calling `rsteg::embed` 11 times in one process, ru_maxrss returns the max across all 11 calls — indistinguishable from a memory leak and not reflective of per-call peak.

- **Subprocess runs**: `ru_maxrss` after the child exits is the right measurement for that single invocation. Reported.
- **In-process runs**: peak RSS is **not reported** by default. A separate in-process diagnostic mode can sample `/proc/self/status` `VmHWM` every 1 ms via a helper thread — opt-in via `--sample-rss`. Without custom allocator hooks, per-call peak is not portably measurable.

### Output

JSON:
```json
{
  "rsteg_version": "0.1.0",
  "host":   { "os": "darwin", "arch": "aarch64", "cpu": "Apple M2", "ncpu": 8 },
  "runs":   [
    {
      "case": "bmp-small",
      "operation": "embed",
      "payload_size": 10240,
      "tool": "rsteg-cli",
      "mode": "subprocess",
      "wall_ns_p50": 2100000,
      "wall_ns_p95": 2240000,
      "wall_ns_mad": 80000,
      "cpu_ns_p50":  1920000,
      "peak_rss_bytes": 1888256,
      "output_size_delta": 0
    }
  ]
}
```

Markdown summary:
```
## bmp-small — embed 10 KB payload (subprocess)

| Tool        | Wall p50 (ms) | Wall p95 | CPU p50 | Peak RSS (MB) | Output Δ |
|-------------|---------------|----------|---------|---------------|----------|
| rsteg-cli   | 2.10 ±0.08    | 2.24     | 1.92    | 1.8           | 0        |
| stegano-cli | 2.80 ±0.12    | 3.00     | 2.40    | 3.1           | 0        |
| steghide    | 8.40 ±0.31    | 9.10     | 7.20    | 4.8           | 0        |
```

### CI integration

- PR job runs bench-smoke: `bmp-small` + `wav-short` + `png-photo-small` at one payload size each. Budget: 60 seconds total.
- Regressions on CPU-p95 > 20% (revised from earlier 10%, which produces too many false positives on GHA) fail the PR. `bench-ok` label overrides.
- Comparison baseline: **rolling median of the last 20 base-branch runs** (stored in a `bench-history` orphan branch), not a single data point.
- Nightly full-bench job on self-hosted runner when available; falls back to GHA. Results committed as JSON under `bench-history/<date>.json`.
- Plot generated by a tiny `awk` + `gnuplot` script — no web dashboard.

### Parallelism (policy)

**rsteg is single-threaded.** Single-threaded is the right choice for lowest peak RSS (no per-thread stacks, no thread-local arenas) and dominant paths are either memory-bound (BMP/WAV LSB loop) or bottlenecked by upstream single-threaded libraries (`miniz_oxide`). Threshold for reconsidering parallelism: >50 ms embed on a single carrier — we're well under that after the perf-review optimizations (spec 04/05 `embed_into`, `embed_in_place`, slicing-by-8 CRC, SIMD LSB).

### Interpreting results

- Be honest about where we lose. The report includes all runs, not a cherry-picked subset.
- If stegano-rs wins PNG extract, that goes in the README. We fix it in code, not in benchmark selection.
- Subprocess startup overhead is not "our fault" and is not subtracted from competitors. Table A is the fair comparison; if that table shows us winning, that's the claim.

### Non-goals

- No `criterion`. No flamegraph fame. Optimizations ship with a benchmark diff attached to the PR or they don't ship.
- No micro-benchmarks of individual functions in CI. Hot-path profiling uses `samply` or `perf` locally.
