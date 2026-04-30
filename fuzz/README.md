# rsteg-fuzz

`cargo-fuzz` harness for the format adapters and the `PayloadHeader` decoder.
Spec'd in [`../specs/07-testing.md`](../specs/07-testing.md) §Fuzzing.

This crate is intentionally **outside the main workspace** (`exclude =
["fuzz"]` in the root `Cargo.toml`) — it pins nightly via
`rust-toolchain.toml` and pulls `libfuzzer-sys`, neither of which should
touch routine `cargo build --workspace` runs on stable.

## Targets

| Target          | Surface fuzzed                         |
|-----------------|----------------------------------------|
| `bmp_extract`   | `BMP_ADAPTER.extract` on arbitrary bytes |
| `wav_extract`   | `WAV_ADAPTER.extract` on arbitrary bytes |
| `png_extract`   | `PNG_ADAPTER.extract` on arbitrary bytes — also exercises `miniz_oxide` decode |
| `header_decode` | `PayloadHeader::decode` on arbitrary bytes |

The contract on every target is the same as spec 07 §Fuzzing:

> Expect `Err` or valid extract; never panic / unreachable / OOM.

Phase 2 will add `aead_open`, `roundtrip`, `roundtrip_aead`, `header_malleable`,
`aad_misbind`, and the `steghide_diff` differential target listed in the spec.

## Local run

Requires `cargo-fuzz` (one-time): `cargo install cargo-fuzz`.

```sh
# from the workspace root
cd fuzz

# 60-second smoke
cargo +nightly fuzz run bmp_extract -- -max_total_time=60

# 10-minute run, multi-core
cargo +nightly fuzz run png_extract -- -max_total_time=600 -jobs=4
```

## Seed corpus

`corpus/<target>/seed_*` holds checked-in seeds. `cargo-fuzz` writes any
new coverage-gaining inputs alongside; only `seed_*` is tracked in git
(see `.gitignore`).

When a target finds a crash, the minimised input is committed under
`corpus/regressions/` (see spec 07 §Regression protocol) and a regression
unit test is added in the relevant adapter crate.

## CI

`.github/workflows/fuzz.yml` runs each target nightly for 10 minutes and
opens an issue on crash. The job is non-blocking (it does not gate PR
merges) — phase-1.5 exit criteria require ≥ 1 week of clean nightly runs
before tagging `0.2.0`.
