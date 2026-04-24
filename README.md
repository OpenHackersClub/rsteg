# rsteg

A minimal-dependency Rust steganography toolkit — library, CLI, and eventual
parity with [steghide](https://www.kali.org/tools/steghide/) and
[stegano-rs](https://github.com/steganogram/stegano-rs).

- Live demo & feature tracker: <https://rsteg.pages.dev>
- Algorithm explainers: <https://rsteg.pages.dev/algo>
- Specs (architecture, formats, crypto, benchmarking): [`specs/`](./specs)
- Benchmark report: [`bench/README.md`](./bench/README.md)

<!-- site:begin intro -->
## What is steganography?

Cryptography hides *what* a message says. Steganography hides *that there is
a message at all*. rsteg takes a carrier file (image or audio) and tweaks
its least-significant bits so they spell out a hidden payload. A copy of the
carrier that doesn't use rsteg is indistinguishable from a copy that does —
unless you know the scheme.
<!-- site:end intro -->

<!-- site:begin lsb-basics -->
## How LSB embedding works

For each byte of the cover, we overwrite the *density* low bits with one
bit of payload. At density 1, the top 7 bits of a pixel are untouched — a
pixel value of `0xA3 = 10100011` becomes `0xA2 = 10100010` to carry a `0`
bit. That's a 1/256 change, invisible to the eye. At density 4 you get
four times the capacity and the change is ±7 (still perceptually tiny but
statistically detectable).
<!-- site:end lsb-basics -->

## Install & run

<!-- site:begin install -->
```sh
# Build the CLI with default features (png + bmp + wav + crypto + compat-steghide).
cargo build --workspace --release

# Embed a payload into a BMP cover with a password-derived AEAD key.
cargo run -p rsteg-cli --release -- embed \
  --in cover.bmp --payload secret.txt --out stego.bmp --password -

# Extract it again.
cargo run -p rsteg-cli --release -- extract \
  --in stego.bmp --out recovered.txt --password -
```
<!-- site:end install -->

See [`CLAUDE.md`](./CLAUDE.md) for the full set of workspace commands
including the minimal `--no-default-features --features png` build
(≤ 2 transitive crates) and the bench harness invocation.

## Design principles

- **Supply-chain minimum.** Every direct and transitive dep is justified
  in [`specs/02-dependency-policy.md`](./specs/02-dependency-policy.md).
  Budget: ≤ 25 transitive crates with default features; ≤ 2 with
  `--no-default-features --features png`.
- **Plugin architecture via Cargo features.** Each format (PNG / BMP /
  WAV / JPEG) and crypto scheme is a separate crate gated by a feature.
  `rsteg-core` has zero runtime deps.
- **`#![forbid(unsafe_code)]`** in every first-party crate.
- **No proc-macro deps anywhere.** Hand-rolled `Display`, `lexopt` for CLI.
- **TDD.** Red-green-refactor per feature. Tests drive the design.

## Benchmarks

rsteg is faster than both reference tools on every case it shares with
them — 4–125× vs `steghide`, 10–25× vs `stegano-cli` on PNG/WAV. Full
numbers and methodology: [`bench/README.md`](./bench/README.md).

## License

MIT OR Apache-2.0
