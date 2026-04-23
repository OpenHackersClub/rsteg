## Overview

### What rsteg is

A command-line tool and Rust library for hiding and extracting payloads inside media files (steganography). Rust-native, minimal supply-chain surface, plugin-based so users only compile in the formats and crypto they need.

### Goals

1. **Feature capability parity** with [`steghide`](https://www.kali.org/tools/steghide/) and [`stegano-rs`](https://github.com/steganogram/stegano-rs): embed, extract, capacity reporting, passphrase-protected payloads, multi-file payloads, multiple carrier formats.
2. **Faster and lower peak memory** than both reference tools on a standard corpus, measured by a reproducible benchmark harness (spec 08). Claim sourced from the subprocess-comparison table (fair to both sides).
3. **Minimal supply chain**: every direct and transitive dependency is justified, reviewed, and gated by CI (spec 02).
4. **Plugin architecture**: formats and crypto schemes are separate crates behind Cargo features. A user who only needs PNG-LSB compiles ~1 transitive dep.
5. **Clean CLI** (spec 03) — not a steghide-clone syntax.
6. **Read steghide-produced files** where the underlying format is supported (spec 06): detection on all formats; full read for steghide's BMP/WAV in phase 1, JPEG in phase 2. **Never write** steghide-compatible files.
7. **Modern crypto**: XChaCha20-Poly1305 AEAD with Argon2id KDF by default. Passphrase-seeded permuted embedding by default when a password is supplied — closes the "magic bytes visible at offset 0" detection gap.
8. **Test-driven development** — every embedder ships with a failing round-trip test before the embedder itself.

### Non-goals

- Binary-compatible *writing* of steghide files. We detect and read them for interop; we don't write them. New files use our own scheme.
- Network protocols, covert channels over wire, or live-stream steganography.
- GUI. CLI + library only.
- Dynamic plugin loading (`.so` / `.dll`). Plugins are compile-time Cargo features — loading unsigned native code at runtime is the exact supply-chain risk we're avoiding.
- Image/audio editing beyond what's needed to embed/extract. We do not re-encode, resize, or transcode carriers.
- Paletted BMP embedding. Naïve LSB on palette indices produces visible artifacts; correct embedding requires EzStego-style palette reordering, out of scope.
- AU / Sun audio format. Steghide supports it; we don't, by design.
- `no_std`. Out of scope for v1 (OS entropy + allocator assumptions). Revisit if embedded demand materializes.

### What we don't guarantee

- **Presence concealment against all adversaries.** When a passphrase is supplied, rsteg uses the permuted scheme — our files look ~statistically like an unmodified carrier to chi-square and sample-pair analysis. Without a passphrase (linear scheme), the `RSTG` magic bytes are at a deterministic position and any third party can tell rsteg was used. This is documented in `SECURITY.md` at phase 1.5.
- **Detection-resistance parity with steghide's graph-matching on JPEG.** Phase 2 ships F5 (meaningfully better than jsteg-style LSB); graph-matching comes in phase 2 late or phase 3.

### Comparison targets

| Tool | Formats | Crypto | Notes |
|------|---------|--------|-------|
| steghide | JPEG, BMP, WAV, AU | AES-128-CBC + libmhash MD5-based KDF, LCG permutation | C++, graph-matching DCT embedding, last release 2003 |
| stegano-rs | PNG, WAV | XChaCha20-Poly1305 + Argon2id | Rust, LSB only, actively maintained |
| **rsteg phase 1** | PNG, BMP, WAV | XChaCha20-Poly1305 + Argon2id, permuted LSB by default, steghide-compat read (BMP/WAV) | Rust, plugin arch, minimal deps |
| **rsteg phase 2** | + JPEG (F5) | + steghide-compat read for JPEG; graph-matching JPEG | |

### Success criteria for phase 1 ship

- All three phase-1 formats pass the 20-test minimum set per scheme (spec 07).
- Timing-channel invariant test passes.
- CLI verbs (`embed`, `extract`, `inspect`, `list`) work end-to-end, multi-file manifests included.
- Benchmark harness publishes Table A (subprocess comparison) showing rsteg-cli ≤ steghide and ≤ stegano-cli on CPU p95 + peak RSS on all corpus cases.
- `cargo tree --no-default-features --features png` shows ≤ 2 transitive deps.
- `cargo tree` with default features shows ≤ 30 transitive deps.
- steghide-produced BMP and WAV files are readable given the correct passphrase; ≥ 5 fixtures per format in `corpus/steghide/`; nightly `compat-sanity` CI job green.
