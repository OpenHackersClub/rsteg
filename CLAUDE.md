# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project status: phase 1 (shipped)

`rsteg` is a Rust steganography tool (library + CLI) targeting feature parity with `steghide` and `stegano-rs`. Phase 1 is implemented: core, bmp, wav, png, crypto-aead (XChaCha20-Poly1305 + Argon2id), cli, bench, plus `rsteg-site-build` which renders the static landing page from source READMEs. Phase 2 is **spec'd but not yet coded** — `rsteg-compat-steghide` (spec 06) and `rsteg-jpeg` (spec 05) have no crate under `crates/` yet, and spec 04's `Detector`, `Registry`, and `Capacity` trait surface is partially implemented (the registry is wired compile-time in `rsteg-cli::build_registry`, but the full detection framework from spec 04 §"Detector" is TBD). The browser-WASM façade (`rsteg-wasm`, spec 10) is in-progress on its own feature branch.

Read `specs/README.md` first, then the numbered specs in order. When code and spec disagree, treat the code as current and open a PR that updates the spec; annotate unimplemented spec sections with `<!-- TBD (phase 2) -->` so future readers know what's pending.

Key constraints that shape every decision:

- **Supply-chain minimization.** Every direct and transitive dep is justified in [`specs/02-dependency-policy.md`](specs/02-dependency-policy.md). Current reality (`cargo tree -p rsteg-cli`): ~35 transitive crates with default features (Argon2id + XChaCha20-Poly1305 stack dominates), 1 with `--no-default-features --features png`. The written spec-02 budget is what the project is managed against; adding a new direct dep requires a PR that updates that spec and re-runs the counts. Drift past ~40 needs a conscious decision.
- **Plugin architecture via Cargo features, not dynamic loading.** Each format (PNG / BMP / WAV / JPEG) and crypto scheme is a separate crate gated by a feature. `rsteg-core` has zero runtime deps.
- **`#![forbid(unsafe_code)]`** in every first-party crate.
- **No proc-macro deps anywhere.** No `serde_derive`, `thiserror`, `async-trait`, `clap` (derive). Hand-rolled `Display`, `lexopt` for CLI.
- **TDD discipline.** Red-green-refactor per feature. See [`specs/07-testing.md`](specs/07-testing.md). The commit cadence is: test commit (red) → implementation commit (green) → optional refactor commit.

## Workspace layout (current)

```
rsteg/
  Cargo.toml                  # [workspace]
  crates/
    rsteg-core/               # Traits, Error, PayloadHeader, Prng — std only
    rsteg-png/                # feat: png             (dep: miniz_oxide)
    rsteg-bmp/                # feat: bmp             (zero deps)
    rsteg-wav/                # feat: wav             (zero deps)
    rsteg-crypto-aead/        # feat: crypto-aead     (RustCrypto chacha20poly1305 + argon2 + zeroize)
    rsteg-cli/                # binary, uses lexopt — registers adapters at build-time
    rsteg-bench/              # dev-only subprocess comparison harness, publish = false
    rsteg-site-build/         # dev-only: render README.md fragments into public/*.html
    # --- spec'd but not yet coded ---
    rsteg-jpeg/                # feat: jpeg            (phase 2; spec 05)
    rsteg-compat-steghide/     # feat: compat-steghide (phase 2, read-only; spec 06)
  corpus/                     # test fixtures (incl. real steghide files)
  specs/                      # design docs — authoritative
  public/                     # static site served by Cloudflare Pages
  sample/                     # demo input images (Munch cover + payload)
  bench/                      # bench reports
```

Implementation order per [`specs/09-roadmap.md`](specs/09-roadmap.md): core → bmp → wav → crypto-aead → png → cli → bench (✅ all shipped) → compat-steghide → jpeg (phase 2).

## Commands

These are the canonical commands for the current (phase-1) workspace.

```sh
# Build / test default features (png + bmp + wav + crypto-aead)
cargo build --workspace
cargo test  --workspace

# Minimal build — proves the ≤ 2 transitive claim for a bmp-less, png-only,
# crypto-free binary (currently reports 1 — just rsteg-core).
cargo build -p rsteg-cli --no-default-features --features png
cargo tree  -p rsteg-cli --no-default-features --features png --prefix none | sort -u | wc -l

# Default-feature transitive count (currently ~35, bounded by the crypto stack).
cargo tree -p rsteg-cli --prefix none | sort -u | wc -l

# All features (compat-steghide and jpeg are spec'd; features gated but crates TBD).
cargo build --all-features
cargo test  --all-features

# Single integration test
cargo test -p rsteg-bmp --test roundtrip

# Single unit test by name
cargo test -p rsteg-core header::tests::decode_rejects_bad_magic

# Supply-chain gate
cargo deny check

# Fuzzing (requires `cargo install cargo-fuzz` + nightly toolchain).
# fuzz/ is its own cargo-fuzz workspace, excluded from the main one.
# Targets: bmp_extract, wav_extract, png_extract, header_decode.
cd fuzz && cargo +nightly fuzz run bmp_extract -- -max_total_time=60

# Bench harness (subprocess comparison vs steghide + stegano-cli)
cargo run -p rsteg-bench --release -- run --case bmp-small --tool all
cargo run -p rsteg-bench --release -- run --all --format markdown

# Soak harness — long-running stability test (phase-1.5 deliverable).
# Mixes embed/extract/inspect over a 1-10 MB carrier pool with 5% malformed
# inputs; reports RSS drift + coefficient-of-variation. Spec target: 2h.
cargo run -p rsteg-bench --release -- soak --duration 30s
cargo run -p rsteg-bench --release -- soak --duration 2h

# CLI smoke
cargo run -p rsteg-cli --release -- embed --in cover.bmp --payload secret.txt --out stego.bmp --password -
cargo run -p rsteg-cli --release -- extract --in stego.bmp --out recovered.txt --password -

# Site: re-render public/*.html from README.md + bench/README.md (idempotent).
cargo run -p rsteg-site-build

# Preview the deployed site locally (matches Cloudflare Pages byte-for-byte).
python3 -m http.server --directory public 8787
```

## Architecture cheat-sheet

- `rsteg-core` defines `FormatAdapter` and `CryptoScheme` traits (spec 04's `Detector` / `Registry` / `Capacity` are spec'd but only partially surfaced in code — the registry lives as compile-time wiring in `rsteg-cli::build_registry`, not as a standalone `Registry` type). Every format and crypto scheme is an adapter crate.
- Adapters are registered compile-time in `rsteg-cli::build_registry()` via `#[cfg(feature = "…")]` blocks. No `inventory`/`linkme` dep.
- All embed/extract I/O is `&[u8]` in, `Vec<u8>` out. Core is pure; the CLI does all file I/O.
- 32-byte `PayloadHeader` (`RSTG` magic + version + flags + crypto fourcc + scheme fourcc + density + body len + CRC32 + reserved) frames every write. See `crates/rsteg-core/src/header.rs` for the exact offsets. Detection on extract = looking for the magic in the first `160/density` embedding units.
- Crypto default: `aead-chacha20` — XChaCha20-Poly1305 + Argon2id (m=64 MiB, t=3, p=1) — fourcc `b"XCA1"`, exposed as `rsteg_crypto_aead::FOURCC`. Earlier spec drafts mentioned PBKDF2-HMAC-SHA256 @ 600k iters; code switched to Argon2id during phase 1 (see `crates/rsteg-crypto-aead/src/lib.rs:3`). `compat-steghide` is spec'd as detection + read only (never write) but the crate is not yet implemented.

See [`specs/01-architecture.md`](specs/01-architecture.md) and [`specs/04-core-traits.md`](specs/04-core-traits.md) for the full picture.

## When specs and code disagree

Specs are authoritative. If you find the code diverging, update the spec in the same PR as the code change — never leave them out of sync. If the spec is wrong and you can't fix both in one PR, open an issue and annotate the spec with `# TBD (issue #N)`.

## Git workflow

- Feature branches only. Never push to `main`.
- Commit messages follow conventional prefixes: `test(bmp): …`, `feat(bmp): …`, `refactor(bmp): …`, `docs(spec-07): …`, `chore: …`.
- Spec edits travel with the code change that implements them.
