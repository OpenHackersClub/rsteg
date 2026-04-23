## Roadmap

Phased delivery with explicit exit criteria. Each phase ends with a git tag and a published bench report.

### Phase 0 — Specification (done)

**Deliverable:** `specs/` directory + `REVIEW_NOTES.md` capturing the initial review team's findings and our responses.

**Exit criteria:**
- 10 specs written, reviewed, revised to address BLOCKING/CRITICAL/HIGH/MAJOR findings from the 5-agent review team.
- Open questions either answered or marked `# TBD (issue #N)`.

### Phase 1 — Foundations (target: shippable steganography for PNG/BMP/WAV)

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
- All 20 integration tests per adapter pass.
- Timing-channel test passes (spec 04 invariant).
- `corpus/steghide/` BMP and WAV files (≥ 5 each) decode correctly.
- `cargo tree` counts within budgets (spec 02).
- CI green across entire matrix (spec 07) including Windows and macOS-arm `test-default`.
- Bench report: rsteg-cli wins or ties both `steghide` and `stegano-cli` on CPU p95 + peak RSS on all subprocess-table cases in `corpus/bench/`. Losses documented in README.
- `CHANGELOG.md` written; `0.1.0` tagged.
- `CLAUDE.md` updated with actual workspace commands.

### Phase 1.5 — Hardening

**Deliverable:** fuzz corpus stabilized; 1+ week of nightly fuzz runs clean; soak test passed; `SECURITY.md` published.

**Exit criteria:**
- `fuzz/corpus/` has ≥ 100 MB of seeds per target, captured from CI fuzz runs.
- No open panics, no open crashes, no known OOMs on any fuzz target.
- 2-hour soak on BMP and WAV + PNG shows RSS drift < 5% and variance < 20%.
- `SECURITY.md`: threat model, what we guarantee (confidentiality via AEAD), what we don't (presence concealment in linear schemes, steghide-compat is read-only and uses weak primitives), reporting process.
- `0.2.0` tagged.

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
