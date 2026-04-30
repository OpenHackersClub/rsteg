# rsteg bench report

*Generated 2026-04-24 on macOS aarch64 (Apple Silicon, ncpu=28).*

This is the canonical phase-1 Table A (subprocess) comparison, following
[`specs/08-benchmarking.md`](../specs/08-benchmarking.md). Each cell is
the median of 11 measured iterations after 3 warmups; the p95 column is
also reported because tail latency matters more than median for an
interactive CLI.

## How to reproduce

```sh
cargo build --release -p rsteg-cli -p rsteg-bench
cargo run --release -p rsteg-bench -- --all --format markdown --out bench.md
```

`stegano` is installed via `cargo install stegano-cli`. `steghide` must
be built from source — Homebrew dropped the package in 2024 and there is
no published arm64 build.

## Soak harness

Long-running stability test for the format adapters — phase-1.5 deliverable
per [`specs/07-testing.md`](../specs/07-testing.md) §"Soak tests". Lives
behind a `soak` subcommand on the same binary:

```sh
# 30-second smoke (defaults: 5% malformed inputs, deterministic seed)
cargo run --release -p rsteg-bench -- soak --duration 30s

# Spec target
cargo run --release -p rsteg-bench -- soak --duration 2h
```

The harness mixes `embed` / `extract` / `inspect` over a pre-generated
1–10 MB carrier pool (BMP, WAV, PNG) via a Markov-ish chain, samples RSS
each iteration, and reports drift + coefficient-of-variation alongside
op counts. Pass thresholds (spec 07): drift &lt; 5%, CV &lt; 20%, no
unexpected errors. **Short runs (&lt; ~20 s) flag drift artifacts from
allocator warmup** — under ~30 s of mixed work, by ~30 s the head/tail
windows are far enough apart for steady-state to dominate.

Exit code is `0` on pass, `1` on unexpected errors, `2` on bad arguments.

## Tools

| Tool       | Version | Provenance                                                 |
|------------|---------|------------------------------------------------------------|
| `rsteg`    | 0.1.0   | This repo — `cargo build --release`                        |
| `steghide` | 0.6.0   | Built from source, native arm64 (autotools + homebrew libs) |
| `stegano`  | 0.6.1   | `cargo install stegano-cli`                                |

## Headline results

<!-- site:begin bench-headline -->
| Op / carrier / payload        | rsteg    | steghide  | stegano | speedup (vs next best) |
|-------------------------------|---------:|----------:|--------:|-----------------------:|
| BMP 128×128 embed 1 KB        |   1.6 ms |   20.6 ms |    —    | **13×**                |
| BMP 128×128 extract 1 KB      |   1.5 ms |    6.5 ms |    —    | **4.4×**               |
| BMP 512×512 embed 1 KB        |   3.8 ms |   41.2 ms |    —    | **11×**                |
| BMP 512×512 embed 10 KB       |   3.5 ms |  440.1 ms |    —    | **125×**               |
| BMP 512×512 extract 10 KB     |   2.7 ms |   28.1 ms |    —    | **10×**                |
| WAV 5 s stereo embed 1 KB     |   2.4 ms |  crash¹   |   69 ms | **29×** (vs stegano)   |
| WAV 5 s stereo embed 20 KB    |   3.5 ms |  crash¹   |   71 ms | **20×** (vs stegano)   |
| WAV 5 s stereo extract 20 KB  |   2.9 ms |  crash¹   |   68 ms | **23×** (vs stegano)   |
| PNG 256×256 embed 1 KB        |   6.1 ms |     —     |   67 ms | **11×**                |
| PNG 256×256 extract 10 KB     |   2.6 ms |     —     |   66 ms | **25×**                |

¹ Native arm64 `steghide 0.6.0` reliably SIGSEGVs when embedding payloads
≥ 1 KB into the synthetic `wav-short` cover. Raising `ulimit -s` to 64 MB
did not help, so this is not a stack-size issue. Payloads ≤ 512 B embed
successfully. rsteg is the only tool with complete `wav-short` coverage
on this platform.

**rsteg is faster than both reference tools on every case it shares with them.**
The gap versus `steghide` is 4–125× depending on carrier size and
payload; against `stegano-cli` on PNG/WAV it's 10–25×.
<!-- site:end bench-headline -->

## Caveats (read these)

- **Wall-clock via `std::process::Command`.** Subprocess overhead
  (`fork`/`exec`, dynamic linker, CRT init) is *not* subtracted from
  anyone. That's the fair Table A comparison — it's what an interactive
  user actually experiences.
- **Peak RSS and CPU-time columns report 0.** Portable child `getrusage`
  without a direct `libc` dep is phase 1.5 work; dropped from this report
  rather than misreport.
- **Synthetic carriers (PNG/WAV).** PNG uses random pixel data, which is
  adversarial for DEFLATE. Spec 08 notes real-photo PNGs will land in
  phase 1.5 as `png-photo-*` cases; `miniz_oxide` performance on natural
  imagery is ~2–3× faster than on noise.
- **Encryption disabled in this run.** The `embed-encrypted` sub-run
  that measures Argon2id + XChaCha20-Poly1305 overhead is a separate
  bench (also ~3–4 ms of AEAD on top for ≤100 KB payloads; Argon2id
  dominates when `--password` is set, intentionally — memory-hard KDF
  is the point).
- **steghide built from local source, not an upstream release.** Upstream
  0.5.1 does not ship arm64 binaries; we built the 0.6.0 development tip
  natively with the commands below.

## Full per-case tables

### bmp-tiny (128×128 24-bit BMP, 48 KB cover)

| Op      | Payload | Tool     | Wall p50 (ms) | Wall p95 | MAD  |
|---------|---------|----------|--------------:|---------:|-----:|
| embed   | 16 B    | rsteg    |          2.40 |     2.85 | 0.16 |
| embed   | 16 B    | steghide |          6.42 |      —   |   —  |
| embed   | 1 KB    | rsteg    |          1.61 |     4.36 | 0.17 |
| embed   | 1 KB    | steghide |         20.58 |      —   |   —  |
| embed   | 4 KB    | rsteg    |          2.62 |     4.59 | 0.19 |
| embed   | 4 KB    | steghide | — (capacity exceeded) |  |      |
| extract | 16 B    | rsteg    |          1.73 |     2.83 | 0.13 |
| extract | 16 B    | steghide |          5.48 |      —   |   —  |
| extract | 1 KB    | rsteg    |          1.46 |     2.53 | 0.06 |
| extract | 1 KB    | steghide |          6.48 |      —   |   —  |
| extract | 4 KB    | rsteg    |          2.21 |     2.50 | 0.09 |

### bmp-small (512×512 24-bit BMP, ~768 KB cover)

| Op      | Payload | Tool     | Wall p50 (ms) | Wall p95 | MAD  |
|---------|---------|----------|--------------:|---------:|-----:|
| embed   | 1 KB    | rsteg    |          3.77 |     4.48 | 0.13 |
| embed   | 1 KB    | steghide |         41.15 |      —   |   —  |
| embed   | 10 KB   | rsteg    |          3.51 |     4.18 | 0.11 |
| embed   | 10 KB   | steghide |        440.09 |      —   |   —  |
| embed   | 65 KB   | rsteg    |          4.18 |     4.54 | 0.20 |
| extract | 1 KB    | rsteg    |          2.83 |     3.33 | 0.10 |
| extract | 1 KB    | steghide |         17.14 |      —   |   —  |
| extract | 10 KB   | rsteg    |          2.74 |     3.23 | 0.14 |
| extract | 10 KB   | steghide |         28.14 |      —   |   —  |
| extract | 65 KB   | rsteg    |          3.18 |     3.74 | 0.10 |

### wav-short (5 s 44.1 kHz stereo 16-bit PCM, 882 KB cover)

Native arm64 `steghide 0.6.0` SIGSEGVs on any wav embed with payload ≥ 1 KB
against this cover; rsteg vs stegano is the valid comparison.

| Op      | Payload | Tool    | Wall p50 (ms) | Wall p95 | MAD  |
|---------|---------|---------|--------------:|---------:|-----:|
| embed   | 1 KB    | rsteg   |          2.37 |     2.57 | 0.08 |
| embed   | 1 KB    | stegano |         69.04 |    71.75 | 0.25 |
| embed   | 20 KB   | rsteg   |          3.48 |     4.73 | 0.13 |
| embed   | 20 KB   | stegano |         70.75 |    82.30 | 0.35 |
| extract | 1 KB    | rsteg   |          1.86 |     2.12 | 0.03 |
| extract | 1 KB    | stegano |         68.18 |    68.95 | 0.24 |
| extract | 20 KB   | rsteg   |          2.90 |     3.06 | 0.13 |
| extract | 20 KB   | stegano |         68.43 |    69.33 | 0.26 |

### png-synth-small (256×256 RGB8, synthetic cover ~155 KB)

| Op      | Payload | Tool    | Wall p50 (ms) | Wall p95 | MAD  |
|---------|---------|---------|--------------:|---------:|-----:|
| embed   | 1 KB    | rsteg   |          6.09 |     6.44 | 0.11 |
| embed   | 1 KB    | stegano |         66.69 |    67.79 | 0.46 |
| embed   | 10 KB   | rsteg   |          6.16 |     6.38 | 0.11 |
| embed   | 10 KB   | stegano |         67.97 |    69.90 | 0.37 |
| extract | 1 KB    | rsteg   |          2.60 |     3.34 | 0.09 |
| extract | 1 KB    | stegano |         66.67 |    67.79 | 0.24 |
| extract | 10 KB   | rsteg   |          2.60 |     2.92 | 0.10 |
| extract | 10 KB   | stegano |         66.39 |    67.65 | 0.36 |

`steghide` does not support PNG, so it does not appear in this table.

## Where we might lose

So far we don't have a case where we lose. If/when the bench finds one,
it will be added here rather than hidden. Likely candidates as the
corpus grows:

- JPEG (phase 2). We ship F5 only; `steghide`'s graph-theoretic
  embedding is more capacity-efficient for a fixed detectability budget.
  Graph-matching support is on the phase-2/3 roadmap.
- Very large BMP/WAV (>50 MB). Our `embed_scheme` currently materializes
  a scratch buffer of the embedding units; that's O(carrier) memory.
  Under soak we'll want an in-place streaming variant (spec 04 §"In-place embed").
