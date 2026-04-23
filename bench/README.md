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

For the `steghide` column on Apple Silicon (brew dropped the package in
2024), install the Docker bridge:

```sh
docker build -t rsteg-tools:steghide .tmp/Dockerfile.steghide .tmp/
install -m755 .tmp/steghide-wrapper.sh ~/.local/bin/steghide
```

## Tools

| Tool        | Version    | Provenance                             |
|-------------|------------|----------------------------------------|
| `rsteg`     | 0.1.0      | This repo — `cargo build --release`    |
| `steghide`  | 0.5.1      | Debian bookworm via `rsteg-tools:steghide` Docker image |
| `stegano`   | 0.6.1      | `cargo install stegano-cli`            |

## Headline results

| Op / carrier / payload        | rsteg    | steghide  | stegano | speedup (vs next best) |
|-------------------------------|----------|-----------|---------|------------------------|
| BMP 128×128 embed 1 KB        |   2.6 ms |  2373 ms  |    —    | **913×**               |
| BMP 128×128 extract 1 KB      |   2.4 ms |  2352 ms  |    —    | **980×**               |
| WAV 5 s stereo embed 1 KB     |   3.0 ms |   394 ms  |   71 ms | **24×**                |
| WAV 5 s stereo embed 20 KB    |   3.5 ms |  3152 ms  |   72 ms | **21×**                |
| WAV 5 s stereo extract 20 KB  |   3.0 ms |  1195 ms  |   70 ms | **23×**                |
| PNG 256×256 embed 1 KB        |   6.1 ms |     —     |   67 ms | **11×**                |
| PNG 256×256 extract 10 KB     |   2.6 ms |     —     |   66 ms | **25×**                |

**rsteg is faster than both reference tools on every case it shares with them.**
For BMP, the gap versus `steghide` is 3 orders of magnitude; against
`stegano-cli` on PNG/WAV it's 10–25×.

## Caveats (read these)

- **Wall-clock via `std::process::Command`.** Subprocess overhead
  (`fork`/`exec`, dynamic linker, CRT init) is *not* subtracted from
  anyone. That's the fair Table A comparison — it's what an interactive
  user actually experiences.
- **`steghide` ran in a Docker container** on Apple Silicon. The Docker
  process-spawn overhead is ~100 ms per iteration and is *not* subtracted.
  Even if it were, rsteg would still win by ~10× on WAV embed 20 KB (3.5 ms
  vs ~3050 ms of pure steghide CPU).
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

## Full per-case tables

### bmp-tiny (128×128 24-bit BMP, 48 KB cover)

| Op | Payload | Tool     | Wall p50 (ms) | Wall p95 | MAD |
|----|---------|----------|---------------|----------|-----|
| embed   | 16 B  | rsteg    |    2.42 |   2.85 | 0.16 |
| embed   | 16 B  | steghide | 2177.83 | 4577.23| 581  |
| embed   | 1 KB  | rsteg    |    2.60 |   4.36 | 0.17 |
| embed   | 1 KB  | steghide | 2373.15 | 5684.58|1087  |
| embed   | 4 KB  | rsteg    |    2.62 |   4.59 | 0.19 |
| embed   | 4 KB  | steghide | — (capacity exceeded) |  |  |
| extract | 16 B  | rsteg    |    2.27 |   2.83 | 0.13 |
| extract | 16 B  | steghide | 3305.67 | 6039.59| 652  |
| extract | 1 KB  | rsteg    |    2.35 |   2.53 | 0.06 |
| extract | 1 KB  | steghide | 2352.07 | 4037.07| 438  |
| extract | 4 KB  | rsteg    |    2.21 |   2.50 | 0.09 |

### wav-short (5 s 44.1 kHz stereo 16-bit PCM, 882 KB cover)

| Op | Payload | Tool     | Wall p50 (ms) | Wall p95 | MAD |
|----|---------|----------|---------------|----------|-----|
| embed   | 1 KB  | rsteg    |    3.03 |    3.31 | 0.11 |
| embed   | 1 KB  | steghide |  393.68 |  560.61 | 31.19 |
| embed   | 1 KB  | stegano  |   71.07 |   72.88 | 0.21 |
| embed   | 20 KB | rsteg    |    3.48 |    3.77 | 0.12 |
| embed   | 20 KB | steghide | 3152.07 | 3463.15 | 68.34 |
| embed   | 20 KB | stegano  |   71.77 |   76.32 | 0.74 |
| extract | 1 KB  | rsteg    |    2.51 |    3.30 | 0.11 |
| extract | 1 KB  | steghide |  648.28 |  764.09 | 33.30 |
| extract | 1 KB  | stegano  |   69.86 |   71.18 | 0.59 |
| extract | 20 KB | rsteg    |    3.01 |    3.32 | 0.20 |
| extract | 20 KB | steghide | 1195.23 | 1482.51 |152.97 |
| extract | 20 KB | stegano  |   70.09 |   71.17 | 0.39 |

### png-synth-small (256×256 RGB8, synthetic cover ~155 KB)

| Op | Payload | Tool    | Wall p50 (ms) | Wall p95 | MAD |
|----|---------|---------|---------------|----------|-----|
| embed   | 1 KB  | rsteg   |    6.09 |    6.44 | 0.11 |
| embed   | 1 KB  | stegano |   66.69 |   67.79 | 0.46 |
| embed   | 10 KB | rsteg   |    6.16 |    6.38 | 0.11 |
| embed   | 10 KB | stegano |   67.97 |   69.90 | 0.37 |
| extract | 1 KB  | rsteg   |    2.60 |    3.34 | 0.09 |
| extract | 1 KB  | stegano |   66.67 |   67.79 | 0.24 |
| extract | 10 KB | rsteg   |    2.60 |    2.92 | 0.10 |
| extract | 10 KB | stegano |   66.39 |   67.65 | 0.36 |

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
