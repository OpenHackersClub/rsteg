## Phase-0 Review Notes

The initial spec set was reviewed by 5 agents in parallel (security, architecture, performance, steganography-domain, testing). This document records the top findings that changed the spec and the ones that did not, so future readers understand why decisions were made.

### Adopted (spec changed)

#### Crypto / security
- **Argon2id replaces PBKDF2** as the default KDF. PBKDF2@600k is GPU/ASIC-attackable; Argon2id is memory-hard and the OWASP 2023 default. Cost: +1 dep (`argon2`). Spec 06.
- **XChaCha20-Poly1305 replaces ChaCha20-Poly1305.** 192-bit nonce eliminates birthday-collision risk and matches stegano-rs's posture. Same dep (`chacha20poly1305`). Spec 06.
- **Full `PayloadHeader` bound into AEAD AAD.** Prevents version-rollback, crypto-downgrade, and body-length tampering. Spec 04, 06.
- **Single `BadPassphrase` error for all encrypted-path failures.** Prior `BodyCrcMismatch` vs `BadPassphrase` split was an oracle. CRC is zero (and unused) when encrypted. Spec 04.
- **`getrandom` crate allowlisted.** Hand-rolled OS RNG shim was a foot-gun (unsafe FFI; early-boot `EAGAIN`; short reads). Spec 02, 06.
- **`zeroize` crate allowlisted.** Hand-rolled zeroize in safe Rust is unsound (optimizer elides dead stores). Spec 02, 06.
- **`--password-insecure-arg` opt-in** for CI secret-injection workflows; default remains argv-strict. Spec 03.
- **`termios`/`SetConsoleMode` echo suppression** for stdin passphrase reads. Spec 03.
- **`O_NOFOLLOW` + mode-bits check** for `file:PATH` passphrase source. Spec 03.

#### Architecture / API
- **Registry stores `&'static dyn`** (not `Box<dyn>`). Zero allocation, devirtualization opportunity. Spec 01, 04.
- **`rsteg-crypto` is one crate** (was split into `-aead` + `-compat-steghide`). Two features; shared `sha2`/`zeroize`/`getrandom`. Spec 01.
- **`embed_into` / `extract_into` hot-path methods.** Caller-supplied `&mut Vec<u8>` avoids per-call allocation. Spec 04.
- **`embed_in_place` for BMP/WAV.** These formats don't require re-encoding; we should not clone the carrier. Spec 04, 05.
- **`PayloadHeader` grown to 32 bytes** with `scheme_fourcc` + `density` + more reserved space. Makes extract self-describing; we don't brute-force density × scheme. Spec 04.
- **`deny(unsafe_code)` replaces `forbid(unsafe_code)`** with documented per-module opt-outs for SIMD. Enables hand-vectorized LSB loop. Spec 02.
- **`Error::Adapter` carries a boxed `source`** instead of a stringly-typed detail. Spec 04.
- **`Error::{PassphraseRequired, UnexpectedPassphrase, AggressiveDensityNotAllowed, RngUnavailable}`** added. Spec 04.

#### Formats / domain
- **Permuted LSB moved to phase 1** (was phase 3). It's ~100 LOC and closes the "rsteg files detectable at offset 0" gap. Default when `--password` is supplied. Spec 05, 09.
- **Density capped at `Low | Moderate` by default.** Density 3/4 requires `--allow-aggressive-density`. Spec 05 (review: d≥2 already fails chi-square on natural images; d≥3 is visibly distorted).
- **PNG filter choice preserved per-row.** Earlier "filter 0 for determinism" created a detection fingerprint. Spec 05.
- **8-bit WAV added to phase 1** (was 16-bit only). Feature parity with steghide. Spec 05.
- **Paletted BMP explicitly non-goal.** Naïve LSB on palette indices is visually broken. Spec 00, 05.
- **Uniform-alpha detection for 32-bit BMP / RGBA PNG.** Embedding LSB in an all-0xFF alpha channel is a chi-square giveaway; skip alpha when it's uniform. Spec 05.
- **JPEG phase-2 scheme is F5, not jsteg-LSB.** Jsteg has been broken by chi-square since 1999. Spec 05, 09.
- **Graph-matching (`jpeg-dct-graph`) moved from vague phase-3 backlog to phase-2-late / early-phase-3.** It's why steghide still exists. Spec 09.
- **Multi-file payload support in phase 1** via a simple manifest. stegano-rs has this; steghide doesn't. Spec 03.

#### steghide compat — factual corrections
- **KDF**: steghide uses libmhash's MD5-based `mhash_keygen_ext`, not SHA-256. Our earlier draft was factually wrong. Added `md-5` to allowlist. Spec 06.
- **PRNG**: steghide uses an LCG (not Mersenne Twister, not libmcrypt). LCG constants verified in `src/PseudoRandomSource.cc`. Spec 06.
- **Compat ship gate**: before merge, spec 06 must have no TBDs on the steghide protocol; `≥ 5 fixtures per format` must exist; `compat-sanity` CI job (installs real steghide) must pass. Spec 06, 07, 09.

#### Testing / CI
- **Phase 1 work order reversed**: first failing test is `rsteg-bmp::tests::roundtrip_empty`; `rsteg-core` grows to satisfy it. Prevents framework-first test-after-the-fact drift. Spec 09.
- **Minimum test set per format expanded from 9 to 20 tests** (new: passphrase-on-plaintext, plaintext-without-passphrase, density out-of-range, minimum-carrier, alignment corners, in-place equivalence, uniform-alpha skip). Spec 07.
- **Hand-rolled shrinker** for the in-house PRNG harness (~100 LOC). Recovers most of what `proptest` shrinking gives. Spec 07.
- **Regression inputs stored inline** (minimized) rather than as RNG seeds. Caps suite growth. Spec 07.
- **CI: `test-minimal`, Windows `test-default`, macOS-arm `test-default`, Miri on `rsteg-core`, `compat-sanity` nightly, `mutants` nightly** added. Spec 07.
- **Differential fuzz target `steghide_diff`** when steghide binary is available. Spec 07.
- **`aad_misbind`, `header_malleable`, `roundtrip_aead`, `cli_argv`** fuzz targets added. Spec 07.
- **Timing-channel test** (BadPassphrase wrong-pass vs tampered-tag vs bad-magic must be within 2σ). Spec 04, 07.
- **Bench threshold: CPU p95 > 20%** (was wall-clock p50 > 10%). Stable on shared CI runners. Spec 08.
- **Bench baseline: rolling median of last 20 base-branch runs.** Spec 08.

#### Benchmarking
- **Subprocess and in-process numbers in separate tables**; README headline sourced from subprocess table (fair to both sides). Spec 08.
- **p50 + p95 reported**; tail matters for interactive tools. Spec 08.
- **Peak RSS only reported for subprocess runs** (`ru_maxrss` is lifetime peak; unreliable in-process). Spec 08.
- **Real-photo PNG corpus** in addition to synthetic random; the synthetic data is adversarial for DEFLATE and distorts user-facing numbers. Spec 08.

### Adopted (feature or flag addition)

- **`png-fast` feature** — off by default. Swaps `miniz_oxide` for `libdeflater` (C FFI). 2–3× PNG compress speed at the cost of the pure-Rust posture. Spec 02, 05.
- **CRC32 slicing-by-8** (8 KB table, ~4× faster than the 1 KB table-per-byte variant). Spec 04.
- **SHA-256 hardware-acceleration** (`sha2/asm` feature) wired for Apple Silicon + `sha-ni` x86. Spec 06.

### Not adopted (considered and rejected)

- **Merge BMP + WAV into one crate.** Suggested by reviewer; we keep them separate so fuzzing and feature-flag minimization (one-format builds) stay clean. Shared code lives in `rsteg-core::bits`.
- **Switch `Vec<Box<dyn>>` registry to a closed `enum`.** An enum closes the door on third-party format adapters using `rsteg-core`. We took the `&'static dyn` midpoint — zero allocation, still extensible.
- **Add a facade `rsteg` crate** re-exporting all adapters. Defer to phase 1.5 based on user feedback.
- **Unsafe CLI literal-password short-form** (e.g. `--password=secret`). Kept only the explicit `--password-insecure-arg` variant with a deliberately ugly name.
- **Parallelize embed path with rayon.** Single-threaded is faster for lowest peak RSS and crossover point (>50 ms embed) is above what we'll see after the perf-review optimizations.

### Known deferred items (not blockers)

- `jpeg-dct-graph` may land phase-2-late or early-phase-3 depending on scope.
- `png-chunk-text` — ancillary-chunk embedding, phase 3.
- Additional formats (TIFF, GIF EzStego, AU) — phase 3 backlog.
- `no_std` / WASM / mobile — out of scope unless user demand surfaces.

### Sign-off

Spec revision completed 2026-04-23 against all BLOCKING/CRITICAL/HIGH/MAJOR findings from the review team. The spec is considered ready for phase 1 implementation. MEDIUM/MINOR/LOW/NIT items either addressed or scheduled for phase 1.5/2. Implementation will reference this document when questions arise about why a particular decision was made.
