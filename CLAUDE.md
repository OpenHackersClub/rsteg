# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project status: phase 0 (specification)

`rsteg` is a Rust steganography tool (library + CLI) targeting feature parity with `steghide` and `stegano-rs`. As of this commit there is **no Rust code yet** — the canonical source of truth is `specs/`. Always read `specs/README.md` first, then the numbered specs in order.

Key constraints that shape every decision:

- **Supply-chain minimization.** Every direct and transitive dep is justified in [`specs/02-dependency-policy.md`](specs/02-dependency-policy.md). Budget: ≤ 25 transitive crates with default features; ≤ 2 with `--no-default-features --features png`. Adding a dep requires updating that spec in the same PR.
- **Plugin architecture via Cargo features, not dynamic loading.** Each format (PNG / BMP / WAV / JPEG) and crypto scheme is a separate crate gated by a feature. `rsteg-core` has zero runtime deps.
- **`#![forbid(unsafe_code)]`** in every first-party crate.
- **No proc-macro deps anywhere.** No `serde_derive`, `thiserror`, `async-trait`, `clap` (derive). Hand-rolled `Display`, `lexopt` for CLI.
- **TDD discipline.** Red-green-refactor per feature. See [`specs/07-testing.md`](specs/07-testing.md). The commit cadence is: test commit (red) → implementation commit (green) → optional refactor commit.

## Workspace layout (planned — not yet scaffolded)

```
rsteg/
  Cargo.toml                  # [workspace] only
  crates/
    rsteg-core/               # Traits, Error, Registry, PayloadHeader — std only
    rsteg-png/                # feat: png             (dep: miniz_oxide)
    rsteg-bmp/                # feat: bmp             (zero deps)
    rsteg-wav/                # feat: wav             (zero deps)
    rsteg-jpeg/               # feat: jpeg            (phase 2)
    rsteg-crypto-aead/        # feat: crypto          (RustCrypto chacha20poly1305 + pbkdf2 + sha2)
    rsteg-compat-steghide/    # feat: compat-steghide (RustCrypto aes + cbc + sha2, read-only)
    rsteg-cli/                # binary, uses lexopt
    rsteg-bench/              # dev-only harness, publish = false
  fuzz/                       # cargo-fuzz, dev-only
  corpus/                     # test fixtures (incl. real steghide files)
  specs/                      # design docs
```

Implementation order per [`specs/09-roadmap.md`](specs/09-roadmap.md): core → bmp → wav → crypto-aead → png → compat-steghide → cli → bench.

## Commands

Once the workspace exists, these are the canonical commands:

```sh
# Build / test default features (png+bmp+wav+crypto+compat-steghide)
cargo build --workspace
cargo test  --workspace

# Minimal build — proves dep-budget claim
cargo build --no-default-features --features png
cargo tree  --no-default-features --features png --prefix none | sort -u | wc -l   # must be ≤ 2

# All features incl. jpeg
cargo build --all-features
cargo test  --all-features

# Single integration test
cargo test -p rsteg-bmp --test roundtrip

# Single unit test by name
cargo test -p rsteg-core header::tests::decode_rejects_bad_magic

# Supply-chain gate
cargo deny check

# Fuzzing (requires `cargo install cargo-fuzz`)
cd fuzz && cargo fuzz run bmp_extract

# Bench harness
cargo run -p rsteg-bench --release -- run --case bmp-small --tool all
cargo run -p rsteg-bench --release -- run --all --format markdown

# CLI smoke
cargo run -p rsteg-cli --release -- embed --in cover.bmp --payload secret.txt --out stego.bmp --password -
cargo run -p rsteg-cli --release -- extract --in stego.bmp --out recovered.txt --password -
```

Until the workspace is scaffolded, those commands will fail. If you're starting fresh, the first task in `TaskList` is "Scaffold workspace".

## Architecture cheat-sheet

- `rsteg-core` defines `FormatAdapter`, `Detector`, `CryptoScheme` traits. Every other crate is an adapter.
- Adapters are registered compile-time in `rsteg-cli::build_registry()` via `#[cfg(feature = "…")]` blocks. No `inventory`/`linkme` dep.
- All embed/extract I/O is `&[u8]` in, `Vec<u8>` out. Core is pure; the CLI does all file I/O.
- 32-byte `PayloadHeader` (`RSTG` magic + version + flags + crypto fourcc + scheme fourcc + density + body len + CRC32 + reserved) frames every write. See `crates/rsteg-core/src/header.rs` for the exact offsets. Detection on extract = looking for the magic in the first `160/density` embedding units.
- Crypto default: `aead-chacha20` (ChaCha20-Poly1305 + PBKDF2-HMAC-SHA256 @ 600k iters). `compat-steghide` is detection + read only, never write.

See [`specs/01-architecture.md`](specs/01-architecture.md) and [`specs/04-core-traits.md`](specs/04-core-traits.md) for the full picture.

## When specs and code disagree

Specs are authoritative. If you find the code diverging, update the spec in the same PR as the code change — never leave them out of sync. If the spec is wrong and you can't fix both in one PR, open an issue and annotate the spec with `# TBD (issue #N)`.

## Git workflow

- Feature branches only. Never push to `main`.
- Commit messages follow conventional prefixes: `test(bmp): …`, `feat(bmp): …`, `refactor(bmp): …`, `docs(spec-07): …`, `chore: …`.
- Spec edits travel with the code change that implements them.
