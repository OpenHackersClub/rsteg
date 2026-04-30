# Changelog

All notable changes to `rsteg` are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

The workspace ships under a single version line — every publishable crate
is bumped together (see [`specs/09-roadmap.md`](specs/09-roadmap.md) §"Versioning & release").

## [Unreleased]

### Added

- `CHANGELOG.md` (this file).
- **`fuzz/`** — `cargo-fuzz` scaffold for the format adapters and the
  `PayloadHeader` decoder. Targets: `bmp_extract`, `wav_extract`,
  `png_extract`, `header_decode`. Seed corpus checked in for each target.
  Non-blocking nightly CI workflow (`.github/workflows/fuzz.yml`) runs
  every target for 10 minutes. Phase-1.5 deliverable per
  [`specs/07-testing.md`](specs/07-testing.md) §Fuzzing and
  [`specs/09-roadmap.md`](specs/09-roadmap.md) §Phase 1.5.

## [0.1.0] — Phase 1 foundations

First public release. Shippable steganography for BMP, WAV, and PNG carriers
with authenticated encryption.

### Added

- **`rsteg-core`** — `FormatAdapter` and `CryptoScheme` traits, `Error` enum,
  32-byte `PayloadHeader` framing (`RSTG` magic + version + flags + crypto
  fourcc + scheme fourcc + density + body length + CRC32 + reserved),
  `BitWriter`, `Density`, `splitmix64` PRNG with `Blake2b`-keyed seeding.
  Zero runtime deps, `#![forbid(unsafe_code)]`.
- **`rsteg-bmp`** — 24-bit uncompressed LSB embedding for BMP, both linear
  (`bmp-lsb-linear`) and permuted (`bmp-lsb-permuted`) schemes.
- **`rsteg-wav`** — LSB embedding for 8-bit and 16-bit PCM WAV, linear
  (`wav-lsb-linear`) and permuted (`wav-lsb-permuted`) schemes.
- **`rsteg-png`** — LSB embedding for PNG truecolor / truecolor-alpha,
  preserving the source image's per-row filter on re-encode. Single dep:
  `miniz_oxide` (DEFLATE) with `default-features = false`.
- **`rsteg-crypto-aead`** — `XChaCha20Argon2id` AEAD scheme. XChaCha20-Poly1305
  body with an Argon2id KDF (`m=64 MiB, t=3, p=1`), fourcc `b"XCA1"`. The
  full 32-byte `PayloadHeader` is bound into AEAD associated data — every
  field is tamper-evident, not just the body. AEAD failure paths collapse
  to a single `BadPassphrase` error per spec 06.
- **`rsteg-cli`** — `rsteg` binary with `embed` / `extract` / `inspect` /
  `list` / `version` verbs. `lexopt`-based argument parsing (no proc-macro
  CLI deps). Multi-payload manifest support via `--payload`.
- **`rsteg-bench`** — subprocess-comparison harness vs `steghide` and
  `stegano-cli`. JSON and Markdown report output. Phase-1 numbers show
  rsteg-cli winning by 4–125× vs `steghide` and 10–25× vs `stegano-cli`
  on shared cases — see [`bench/README.md`](bench/README.md).
- **`rsteg-site-build`** — dev-only renderer that splices marked sections
  from `README.md` and `bench/README.md` into the static landing page,
  with a CI gate that fails when the rendered HTML drifts from source.
- **`rsteg-wasm`** — browser-WASM façade (extract + embed) for
  `wasm32-unknown-unknown` per [`specs/10-browser-wasm.md`](specs/10-browser-wasm.md).
  Hand-rolled `extern "C"` ABI; no `wasm-bindgen`. CI gate enforces the
  release-`.wasm` size budget. Ships on its own version line ahead of
  `0.1.0` for the rest of the workspace.
- **CI** — `cargo build --workspace --all-targets --locked` and
  `cargo test --workspace --locked` matrix on `ubuntu-latest` and
  `macos-latest`; site-content sync gate; publish dry-run for `rsteg-core`;
  Cloudflare Pages deploy + per-PR previews.
- **Release pipeline** — hand-rolled `release.yml` (~85 LOC) that builds
  prebuilt `rsteg-cli` binaries on tagged releases. See
  [`RELEASING.md`](RELEASING.md).
- **Specs** — 11 spec documents under [`specs/`](specs/) covering
  architecture, dependency policy, CLI surface, core traits, formats,
  crypto, testing, benchmarking, roadmap, browser-WASM, plus
  `REVIEW_NOTES.md`.

### Security

- Default crypto path is XChaCha20-Poly1305 + Argon2id — modern AEAD with
  a memory-hard KDF.
- `compat-steghide` (the read-only steghide-interop crate spec'd in
  [`specs/06-crypto.md`](specs/06-crypto.md)) is **not yet shipped** and
  will arrive in phase 2. It will use steghide's MD5-based KDF and is
  documented as detection + read only — never write.
- Linear LSB schemes do not conceal the *presence* of an embedded payload
  against a chi-square attacker. Use the permuted scheme + a passphrase
  (or the AEAD path) when presence-detection resistance matters. A formal
  threat model is owed in `SECURITY.md` (phase 1.5).

### Known gaps tracked for follow-up

- Fuzz scaffold landed post-`0.1.0` (see `[Unreleased]`); ≥ 1 week of clean nightly runs is a phase-1.5 exit criterion.
- No 2-hour soak run on file. Phase 1.5 deliverable.
- No Windows runner in CI matrix. Phase 1.5 follow-up if a downstream
  user reports breakage.
- `compat-steghide` and `jpeg` adapters are spec'd (specs 05 and 06) but
  have no crate under `crates/`. Phase 2.

[Unreleased]: https://github.com/OpenHackersClub/rsteg/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/OpenHackersClub/rsteg/releases/tag/v0.1.0
