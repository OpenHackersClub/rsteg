## Testing

Test-driven all the way. Red → green → refactor per embedder, per scheme, per crypto primitive.

### Levels

```
  /  soak         \   minutes → hours (local + scheduled, not CI-blocking)
 /   fuzz         \   continuous (cargo-fuzz), nightly CI corpus run
/  compat/KAT      \  minutes (real steghide files, RFC vectors)
/  integration     \  seconds (end-to-end embed/extract per format)
/__  unit  _________\ milliseconds (pure functions, header parsing)
```

### TDD workflow — vertical slice first (revised)

Earlier drafts ordered phase 1 as: build `rsteg-core` fully first, then layer adapters. Reviewer flagged this as anti-TDD (framework-first, test-after). Revised order puts the first failing round-trip test *before* any core infrastructure beyond what it forces:

1. **Red**: write `rsteg-bmp/tests/roundtrip_empty.rs` — generates a tiny random BMP with our in-house `Prng`, calls `BMP_ADAPTER.embed(cover, framed, opts)`, then `extract_into(stego, opts, &mut out)`, asserts `out == framed`. Fails to compile first (no `rsteg-core` traits yet, no `BmpAdapter`).
2. **Green**: add the minimum `rsteg-core` surface the test needs — `FormatAdapter` trait, `Error::HeaderMissing`, `PayloadHeader::encode/decode`, `BitWriter` — and the minimum `BmpAdapter` that parses a 24-bit uncompressed BMP and LSB-embeds. Test passes.
3. **Refactor**: extract the bit iterator into `rsteg-core::bits`, tidy.
4. Repeat for `roundtrip_single_byte`, then densities, then WAV, then PNG, then crypto, then permuted, then compat-steghide.

Components that have no test driver in phase 1 (e.g., steghide detector) get built when their first integration test is written, never before.

### Unit tests

- Colocated with code: `#[cfg(test)] mod tests { ... }` at the bottom of each `rsteg-core` source file.
- Cover: bit packers, CRC32 (slicing-by-8 KATs), PayloadHeader encode/decode, capacity math, BMP row stride, WAV chunk walker, splitmix64 determinism, Argon2id against RFC 9106, XChaCha20-Poly1305 against RFC-adjacent vectors.
- Port-contract tests (if any) live in `rsteg-core/tests/` — tests that verify a trait's documented contract via a mock impl. Adapter-integration tests live in the adapter crate's `tests/` directory.

### Integration tests — per-format minimum set (expanded from reviewer findings)

Each format crate ships these tests at minimum:

| # | Test | Notes |
|---|------|-------|
| 1  | Round-trip 0-byte payload | header only |
| 2  | Round-trip 1-byte payload | |
| 3  | Round-trip at exact capacity | |
| 4  | Capacity + 1 byte → `PayloadTooLarge` before output written | |
| 5  | Round-trip at densities Low, Moderate | |
| 6  | Round-trip at Aggressive3, Aggressive4 with `allow_aggressive = true` | |
| 7  | Density=Aggressive3 with `allow_aggressive = false` → `AggressiveDensityNotAllowed` | |
| 8  | Round-trip linear scheme without encryption | |
| 9  | Round-trip permuted scheme with passphrase + encryption | |
| 10 | Tamper test: flip one stego bit, expect `BadPassphrase` (encrypted) or `BodyCrcMismatch` (plaintext) | |
| 11 | Cross-format refusal: feed PNG to BmpAdapter → `FormatUnrecognized` | |
| 12 | Preservation of out-of-band regions (BMP palette gap, WAV LIST chunk, PNG ancillary chunks) byte-for-byte | |
| 13 | Unexpected passphrase: extract plaintext file with `--password` → `UnexpectedPassphrase` | |
| 14 | Passphrase required: extract encrypted file without `--password` → `PassphraseRequired` | |
| 15 | Minimum carrier: too-small file → `PayloadTooLarge { needed: 32, available: 0 }` | |
| 16 | Alignment edge cases (BMP 1-pixel row, WAV odd data-length + pad, PNG 1×N image) | |
| 17 | `--no-header` raw round-trip with explicit `--length` | |
| 18 | Per-variant: 24-bit + 32-bit BMP, 8-bit + 16-bit WAV, RGB + RGBA PNG | |
| 19 | In-place embed (BMP, WAV only) yields byte-identical output to `embed_into`-over-clone | |
| 20 | Uniform-alpha skip: 32-bit BMP with all-0xFF alpha preserves alpha bytes unchanged | |

Test harness refuses to `#[cfg(any(...))]`-guard these — every test runs in every feature configuration that has the format enabled. Tests against disabled-feature combinations appear via the `test-minimal` CI matrix row (below).

### Property-style tests (no `proptest` dep)

`rsteg-core::testing` (dev-only, `#[cfg(test)]`) provides:

```rust
pub struct Prng(u64);  // splitmix64, ~10 lines
impl Prng { pub fn new(seed: u64) -> Self; pub fn u64(&mut self) -> u64; ... }

pub fn random_payload(prng: &mut Prng, len: usize) -> Vec<u8>;
pub fn random_bmp24(prng: &mut Prng, w: u32, h: u32) -> Vec<u8>;
pub fn random_wav16(prng: &mut Prng, samples: u32, channels: u8) -> Vec<u8>;
pub fn random_png_rgba(prng: &mut Prng, w: u32, h: u32) -> Vec<u8>;
```

**Each integration test runs over seeds `0..64`** (stable, reproducible). Failures cause:
1. The failing **input** (minimized via a hand-rolled shrinker, below) is encoded **inline in the test** as a literal `&'static [u8]`, with a comment noting the original seed and fix PR.
2. The seed is NOT kept as a permanent parameter — regression cases are pinned as minimized inputs, not seeds. This caps test-suite growth.

**Hand-rolled shrinker** in `rsteg-core::testing::shrink`:
- Start with the failing `(carrier, payload, opts)` tuple.
- Try: halve carrier dimensions, halve payload length, zero suffix bytes, drop to smallest valid BMP/WAV/PNG.
- Iterate until the test stops failing, log the minimal input.
- ~100 LOC. Closes most of the gap vs proptest's shrinking.

### Compat / known-answer tests

- `corpus/rfc/` — RFC 8439 (ChaCha20-Poly1305) and RFC 9106 (Argon2id) fixed vectors. Checked in as Rust literals.
- `corpus/steghide/` — **≥ 5 files per format** (BMP + WAV phase 1, +JPEG phase 2), each with a `.spec.toml` describing passphrase and expected plaintext hex. Files generated from the actual `steghide` binary with varying:
  - Dimensions / durations
  - Payload length (1 byte, one cipher block, one block + 1, medium, large)
  - Asymmetric plaintexts to catch permutation off-by-one bugs (symmetric payloads can paper over them)
  - Encryption algorithms (at least: default Rijndael-128-CBC, and one non-default e.g. Blowfish to prove our algorithm dispatch works)
  - Compression on/off
- `corpus/png/` — PNGs from multiple producers (libpng reference, ImageMagick, oxipng, browser-exported).
- Total corpus budget: 2 MB. Larger carriers generated on-the-fly.

### Fuzzing

`fuzz/` directory with `cargo-fuzz` + `libfuzzer-sys` (dev-only dep). Targets:

| Target | Purpose |
|--------|---------|
| `bmp_extract`      | Arbitrary bytes → `BmpAdapter::extract`. Expect `Err` or valid extract; never panic / unreachable / OOM. |
| `wav_extract`      | Same for WAV. |
| `png_decode`       | Fuzz the PNG parser path. |
| `jpeg_decode`      | Phase 2. |
| `aead_open`        | Arbitrary ciphertexts against a fixed passphrase. |
| `header_parse`     | `PayloadHeader::decode` on arbitrary bytes. |
| `roundtrip`        | Random bytes as payload × random valid carrier; assert `extract(embed(x)) == x`. |
| `roundtrip_aead`   | Round-trip + encryption. Wrong-passphrase branch must only return `BadPassphrase` — test asserts. |
| `header_malleable` | Embed, then mutate 1–N stego bytes, re-extract. Must never return a Ok-but-wrong plaintext; must panic/return-Ok only. |
| `aad_misbind`      | Embed encrypted, flip one byte in `PayloadHeader` before extract. Must return `BadPassphrase`. |
| `cli_argv`         | Feed arbitrary argv to the lexopt-based parser. Only outcomes: `Ok(ParsedCommand)` or typed `ArgError`. |
| `steghide_diff`    | When `steghide` binary is on PATH: feed same random BMP/WAV + passphrase through both `rsteg` (compat-read) and `steghide`. Assert output equality. |

Corpus seeds come from `corpus/`. Targets run via `cargo fuzz run <name>` locally; a GH Actions nightly job (non-blocking) runs each for 10 min. Any crash → auto-filed issue + minimized input committed under `corpus/regressions/` (size-capped; old entries aged out).

### Timing-side-channel test

In `rsteg-crypto/tests/timing.rs`: run `extract` 200 times each against
1. File with bad header magic
2. File with valid header but tampered tag
3. File with valid header + valid tag but wrong passphrase

Assert that the median wall-clock times are within 2σ of each other. Warmup discarded. Failure = an oracle exists somewhere in the extract path.

### Soak tests (not CI-blocking)

Local-only harness in `rsteg-bench`:
- Loop for 2 hours mixing embed/extract/inspect in a Markov-ish pattern.
- Carrier sizes vary randomly 1 MB–100 MB.
- 5% of inputs are deliberately malformed to exercise error paths (catches allocation-in-error-path leaks).
- Report: RSS drift AND RSS variance (sawtooth at steady drift=0% is still a bug).
- Exit criteria: < 5% drift, < 20% variance, no unexplained deallocations.

### CI matrix (GitHub Actions)

| Job | Features | Platforms | Purpose |
|-----|----------|-----------|---------|
| `build-default`   | default                                | linux+mac+win                 | Canonical build |
| `build-minimal`   | `--no-default-features --features png` | linux                         | Dep-count gate |
| `build-all`       | `--all-features`                       | linux                         | Everything compiles together |
| `test-default`    | default                                | linux+mac+win+macos-14 (arm)  | `cargo test --workspace` |
| `test-minimal`    | `--no-default-features --features png,crypto` | linux                  | Catches "feature A only passes because B is on" bugs |
| `fuzz-smoke`      | (fuzz)                                 | linux                         | 60s per target; fails on crash |
| `miri`            | default                                | linux                         | `cargo miri test -p rsteg-core` — catches unsafe that sneaks in via deps |
| `deny`            | —                                      | linux                         | `cargo deny check` |
| `tree-budget`     | default                                | linux                         | Asserts transitive dep counts |
| `compat`          | default                                | linux                         | Runs against `corpus/steghide/` |
| `compat-sanity`   | default                                | linux (nightly)               | Installs real `steghide`, generates fresh file, round-trips |
| `msrv`            | default                                | linux                         | Builds on pinned MSRV Rust |
| `mutants`         | default                                | linux (nightly, non-blocking) | `cargo-mutants` on `rsteg-core` |

### Bench-as-test thresholds

- CPU time (not wall clock) for comparisons — stable on GHA shared runners.
- Compare against **rolling median of last 20 base-branch runs**.
- Threshold: p95 regression **> 20%** fails the PR (overridable with `bench-ok` label; overrides logged).
- For rigorous local comparisons, use self-hosted runner (nightly only).

### Coverage policy (narrative + CI-enforced)

- **Every `pub fn` in `rsteg-core`** has at least one unit test. CI lint: `rg "pub fn" rsteg-core/src | filter out macros | assert each name appears in a #[test]`.
- **Every `Error` variant** is produced by at least one test. CI lint: a Rust test `error_coverage.rs` walks the `Error` enum (via a helper `fn variants() -> &'static [&'static str]`) and checks each variant name has a covering test file via `include_str!`. ~30 LOC.
- **Every embedding scheme** has all 20 tests in the minimum set.

### Test discipline guardrails

- **Test determinism under feature flags**: every `#[test]` in an integration-test file must start the file with `#![cfg(feature = "…")]` — otherwise it won't compile with that feature off.
- **No shared mutable state**: tests may not read/write under `target/` except via a `tempfile`-equivalent per-test temp dir. Benchmark corpus cache under `target/rsteg-bench-corpus/` is gated by file locks.
- **Git hook (pre-commit)**: if any `src/*.rs` or `crates/*/src/*.rs` file is staged, at least one `*_test.rs` or `tests/*.rs` file must be in the same commit, unless the commit message starts with `refactor:` or `chore:`. Prevents silent implementation-without-test drift.
- **Commit message convention**:
  - `test(bmp): round-trip empty payload` (red, test alone)
  - `feat(bmp): implement 24-bit LSB embed` (green, test + impl)
  - `refactor(bmp): share bit iterator with rsteg-core` (refactor, no behavior change)

### Regression protocol

Any reported bug gets a failing test before the fix. The fix commit references the test file and line. Minimized input is committed inline (not as a seed); stale regression inputs (non-triggering after 6 months) can be pruned.
