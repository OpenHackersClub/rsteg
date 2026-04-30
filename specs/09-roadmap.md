## Roadmap

Phased delivery with explicit exit criteria. Each phase ends with a git tag and a published bench report.

### Phase 0 — Specification (done)

**Deliverable:** `specs/` directory + `REVIEW_NOTES.md` capturing the initial review team's findings and our responses.

**Exit criteria:**
- 10 specs written, reviewed, revised to address BLOCKING/CRITICAL/HIGH/MAJOR findings from the 5-agent review team.
- Open questions either answered or marked `# TBD (issue #N)`.

### Phase 1 — Foundations (target: shippable steganography for PNG/BMP/WAV) — **mostly done**

Status as of 2026-04-30 — steps 1–9 and 11–14 of the work order are landed on `main`; step 10 (steghide-compat) is deferred to phase 2 per [`specs/06-crypto.md`](06-crypto.md). Exit-criteria status is annotated in-line below; remaining gaps (`CHANGELOG.md`, `0.1.0` tag, fuzz nightlies) are tracked as discrete follow-ups rather than re-opening this phase.

**Order of work (vertical-slice TDD):**

1. **First failing test**: `rsteg-bmp/tests/roundtrip_empty.rs`. Red.
2. Grow `rsteg-core` minimally to compile test: `FormatAdapter` trait, `Error` enum (just the variants needed so far), `PayloadHeader::encode/decode`, `BitWriter`.
3. Minimum `BmpAdapter`: 24-bit uncompressed LSB linear scheme. Green.
4. Refactor: extract bit iterator to `rsteg-core::bits`.
5. Expand BMP minimum test set (20 tests per spec 07). Green one-by-one.
6. `WavAdapter`: same pattern. Reuses `rsteg-core::bits`.
7. `rsteg-crypto::aead::XChaCha20Argon2id`: KATs + round-trip tests. AAD-binding test + timing-channel test.
8. BMP/WAV permuted schemes (needs `rsteg-core::prng` with splitmix64 + Blake2b seed).
9. `PngAdapter` with `miniz_oxide`: decode, defilter, embed in-place, refilter preserving original per-row filter, deflate. Hardest of the three because of the pipeline.
10. `rsteg-crypto::compat::steghide`: MD5-based KDF + LCG PRNG. Detection + read for BMP/WAV. Blocked from merging until a) spec 06 compat section has no TBDs, b) 5 per-format fixtures exist, c) `compat-sanity` CI job passes.
11. `rsteg-cli` with `lexopt`: all verbs from spec 03. Multi-payload manifest support.
12. `rsteg-bench`: harness + corpus generators + JSON/markdown output.
13. Fuzzing targets wired up. CI nightly runs them 10 min each.
14. Bench harness publishes comparison table.

**Exit criteria:**
- ✅ Per-adapter integration test suites pass on `main` (BMP / WAV / PNG round-trips, density variants, header rejection).
- ✅ Timing-channel test passes (spec 04 invariant).
- 🚧 `corpus/steghide/` BMP and WAV fixtures: deferred — the steghide-compat crate (`rsteg-compat-steghide`) is spec'd but not yet shipped, so the corpus directory is intentionally absent until phase 2.
- ✅ `cargo tree` counts within budgets (spec 02): `--no-default-features --features png` reports 1 transitive (just `rsteg-core`), default reports ~35 dominated by the AEAD stack.
- ✅ CI green for `cargo build --workspace --all-targets --locked` and `cargo test --workspace --locked` on `ubuntu-latest` and `macos-latest`. Windows runner intentionally not in matrix yet — track as phase-1.5 follow-up if a downstream user reports breakage.
- ✅ Bench report ([`bench/README.md`](../bench/README.md)) shows rsteg-cli winning by 4–125× vs `steghide` and 10–25× vs `stegano-cli` on shared cases.
- 🚧 `CHANGELOG.md`: TBD — see follow-up work below. Once written, tag `0.1.0`.
- ✅ `CLAUDE.md` updated with actual workspace commands.

### Phase 1 follow-ups (must land before `0.1.0`)

These are explicit Phase-1 exit criteria that didn't make it into the original 14-step work order. They're tracked here, not as a separate phase, so the bump to `0.1.0` reflects everything the spec promised.

1. **`CHANGELOG.md`** at repo root, summarising what's in `0.1.0`. Style: Keep-a-Changelog headers (`Added` / `Changed` / `Removed` / `Security`), one section per release. Phase-1 PRs (#1–#19) all squash-merged with conventional-prefix titles, so `git log --oneline main` is the authoritative source.
2. **Annotated `0.1.0` tag** on the commit that ships the changelog. Triggers `release.yml` to build prebuilt binaries (see [`RELEASING.md`](../RELEASING.md)).
3. **`SECURITY.md`** (also a Phase-1.5 deliverable — see below). Lifting it forward gives `0.1.0` users a clear statement of what AEAD does and doesn't promise, what linear-LSB carriers leak (presence-detection by χ²), and the reporting channel.

### Spec 10 — Browser-WASM target (sibling to phase 1, in-progress)

`rsteg-wasm` ([`specs/10-browser-wasm.md`](10-browser-wasm.md)) ships an `extern "C"` façade for `wasm32-unknown-unknown` so embed/extract runs entirely in-browser with no trusted server. Status as of 2026-04-30:

- ✅ Crate scaffold + extract path landed (#14 + #d144ed5).
- ✅ Embed path landed; CI gate on `wasm32-unknown-unknown` size budget.
- 🚧 In-page demo widget on `rsteg.pages.dev` not wired up — until then the landing page advertises the CLI as the canonical entrypoint.
- 🚧 `compat-steghide` reserved-but-unimplemented in `rsteg-wasm` Cargo features; lands when `rsteg-compat-steghide` does.

This track does not block `0.1.0` — the WASM crate has its own version line and ships independently.

### Phase 1.5 — Hardening

**Deliverable:** fuzz corpus stabilized; 1+ week of nightly fuzz runs clean; soak test passed; `SECURITY.md` published.

**Exit criteria:**
- ✅ `fuzz/` scaffold in tree: nightly cargo-fuzz crate with `bmp_extract`, `wav_extract`, `png_extract`, `header_decode` targets, seed corpus, and a non-blocking nightly CI workflow (`.github/workflows/fuzz.yml`).
- 🚧 `fuzz/corpus/` has ≥ 100 MB of seeds per target, captured from CI fuzz runs.
- 🚧 No open panics, no open crashes, no known OOMs on any fuzz target across ≥ 1 week of clean nightly runs.
- 🚧 2-hour soak on BMP and WAV + PNG shows RSS drift < 5% and variance < 20%. (Harness shipped — `rsteg-bench soak`; first 2-hour run pending.)
- ✅ `SECURITY.md` lifted forward into phase 1 (#22). Threat model, what AEAD guarantees, what linear schemes leak, reporting channel.
- 🚧 `0.2.0` tagged.

### Phase 2 — JPEG

**Deliverable:** `rsteg-jpeg` crate with `jpeg-f5` scheme + steghide-compat JPEG read.

**Scheme choice:** F5 (Westfeld 2001), not jsteg-style DCT-LSB. Jsteg is fully broken by chi-square since 1999 and shipping it would mislead users.

**Work order:**
1. Evaluate `zune-jpeg` (audit source, transitive deps, fuzz posture, MSRV).
2. Decide on JPEG encoder: `zune-jpeg`'s own if available, otherwise write a minimal reference encoder (spec'd in `specs/10-jpeg-encoder.md` before work starts).
3. Decode → quantized DCT coefficients.
4. Red: `rsteg-jpeg/tests/roundtrip_f5_small.rs`.
5. Green: F5 matrix encoding + ±1 coefficient adjustment. Re-encode.
6. Compat: steghide JPEG detection + read using the same MD5-KDF + LCG from phase 1.
7. Graph-matching (`jpeg-dct-graph`) — may land in phase 2 if time permits, else early phase 3. This is the detection-resistance wedge vs steghide.

**Exit criteria:**
- JPEG integration tests pass (20 from the minimum set).
- `corpus/steghide/jpeg_*` files (≥ 5) decode correctly.
- JPEG subprocess-table benchmarks: competitive with `steghide` embed speed; meaningfully faster on extract.
- All phase-1 exit criteria still hold.
- `0.3.0` tagged.

### Phase 3 — Advanced schemes & formats

Choose based on user feedback. Candidates:

- `jpeg-dct-graph` — matching steghide's graph-theoretic embedding if not already in phase 2.
- `png-chunk-text` — hide payload in `tEXt`/`iTXt` chunks (ancillary, not true steganography but useful for some workflows).
- Additional formats: TIFF, GIF (EzStego-style palette reordering), AU (the fourth steghide format).
- Adaptive density — higher density in high-variance regions (image) or high-entropy samples (audio).
- Compression (LZ4 or DEFLATE via `miniz_oxide`) before encryption — `PayloadHeader.flags.compressed`.
- Multi-carrier splitting — payload across N carrier files with a threshold scheme.
- Full stego-reading steghide interop including non-default encryption algorithms (Blowfish, Twofish, Serpent, ...).

No deadline. Items graduate when a concrete need surfaces, not on a schedule.

### Deferred / likely-never

- **Binary-compatible steghide writing**: maintenance cost too high, no clear user win. Documented non-goal.
- **GUI**: out of scope.
- **Mobile targets**: out of scope; nothing blocks technically, no current demand.
- **Web/WASM targets**: the `getrandom` crate has WASM support, but `compat-steghide` and much of the CLI would need work. Revisit if a user asks.
- **Formats with encrypted/DRM'd content** (AAC, protected MP4): out of scope.

### Versioning & release

- Semver from `0.1.0`.
- **Pre-1.0**: `0.1.x → 0.1.(x+1)` patches stay compatible. `0.1 → 0.2` is the breaking boundary. Breaking changes in the 0.x line require a minor bump and a `CHANGELOG.md` entry.
- **Post-1.0** (target: end of phase 2): breaking changes require a major bump and a migration note.
- Crates released together; single version number across the workspace.

### Ownership

- Phase 0: spec author.
- Phase 1: orchestrated via this session; subagents may implement individual crates under review.
- Phases 2+: TBD after phase 1 ships.
