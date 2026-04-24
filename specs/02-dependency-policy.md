## Dependency Policy

Supply-chain minimization is a first-class requirement. Every dependency (direct or transitive) is a trust decision. This spec defines the rules and allowlist.

### Rules

1. **Zero runtime deps in `rsteg-core`.** `std` only. If `rsteg-core` ever needs something, the need goes into a new feature crate instead. (Exception: `rsteg-core` may use `getrandom` if and only if the library-level API that needs entropy is routed through `rsteg-core::rng`; otherwise keep RNG in `rsteg-crypto`. See spec 06.)
2. **Every direct dep is allowlisted** in this spec before it enters `Cargo.toml`. Adding a dep is a spec change.
3. **No proc-macro deps anywhere in the workspace.** Proc macros execute arbitrary code at compile time. This rules out `serde_derive`, `thiserror`, `async-trait`, `tokio-macros`, etc. Hand-implement `Display`/`Debug`/`Error` as needed.
4. **No build scripts** (`build.rs`) in our crates. `cargo deny` gates transitive `build.rs` to an allowlisted set (see allowlist).
5. **`#![deny(unsafe_code)]` by default**, with narrow, documented per-module `#[allow(unsafe_code)]` opt-outs. `forbid` is stricter than we can live with once we want SIMD intrinsics or OS FFI. Every `unsafe` block carries a `// SAFETY:` comment explaining the invariant and is covered by fuzzing. Modules currently authorized to `#[allow(unsafe_code)]`:
   - `rsteg-core::bits::simd` — SIMD intrinsics for the LSB loop (x86_64 BMI2 `_pdep_u64`, aarch64 NEON).
   - `rsteg-core::rng` — only if we end up needing our own `libc::getrandom` fallback; prefer the allowlisted `getrandom` crate first.
   A PR adding a new `#[allow(unsafe_code)]` module must list it here in the same change.
6. **No async runtime.** `tokio`, `async-std`, `smol` are banned. CLI is blocking I/O; library is pure functions over `&[u8]` (plus AEAD `seal` which reads OS entropy).
7. **No network-capable deps.** No `reqwest`, `hyper`, `url`, `h2`. We read local files only.
8. **Pinned versions.** `Cargo.lock` is checked in for the workspace (including library crates, against the usual convention — we treat the lockfile as a reviewed artifact).
9. **MSRV** is the current stable Rust minus two releases, re-baselined quarterly. No nightly-only features outside `fuzz/`.
10. **License allowlist**: MIT, Apache-2.0, BSD-2-Clause, BSD-3-Clause, ISC, Zlib. Copyleft (GPL, LGPL, MPL) is rejected because of the possibility of library users needing to re-distribute.

### Direct dependency allowlist

Each entry lists the crate, the feature that gates it, why no in-house replacement, and the supply-chain-weight signal we rely on. "Weight" = approximate LOC + transitive-dep count; lower is safer.

| Crate | Feature | Why we can't replace | Weight |
|-------|---------|----------------------|--------|
| `getrandom`        | `crypto`          | OS-entropy shim across Linux/macOS/Windows/BSD; hand-rolling involves per-platform `unsafe extern` FFI that silently breaks in sandboxed or early-boot environments. Rust-random org maintains it; most crypto libs pull it transitively already. | ~700 LOC, 0 runtime deps on mainstream platforms |
| `zeroize`          | `crypto`          | Secure zeroing in safe Rust is impossible without compiler fences. Zeroize is ~200 LOC, single-file readable, audited, used by every RustCrypto crate. Hand-rolling a "volatile write loop" in safe Rust is a false mitigation (the optimizer elides it). | ~200 LOC, 0 transitive deps |
| `argon2`           | `crypto`          | Memory-hard KDF. PBKDF2-SHA256 is GPU/ASIC-attackable at any iteration count; Argon2id is the OWASP 2023 default. Pulls `blake2` (pure Rust), `password-hash`. | ~4 transitive deps |
| `chacha20poly1305` | `crypto`          | AEAD with constant-time guarantees. We use the `XChaCha20Poly1305` path — 192-bit nonce, misuse-resistant. | RustCrypto; 4–5 transitive |
| `sha2`             | `crypto`, `compat-steghide` | PBKDF2 fallback and compat KDF building block. With the `asm` feature: hardware-accelerated on Apple Silicon and `sha-ni`-capable x86. | RustCrypto; 2 transitive |
| `aes`              | `compat-steghide` | AES-128-CBC for steghide's default cipher. Constant-time. | RustCrypto; 2 transitive |
| `cbc`              | `compat-steghide` | CBC mode wrapper around `aes`. | RustCrypto; 0 extra |
| `md-5`             | `compat-steghide` | steghide's `mhash` KDF uses MD5 (verified against steghide source). Not for new crypto; read-only compat only. | RustCrypto; 2 transitive |
| `miniz_oxide`      | `png`             | PNG requires DEFLATE; 400+ LOC of pure Rust zlib is out of scope for in-house. Used by `rustc`, `flate2`, `image`. | 0 runtime deps |
| `zune-jpeg`        | `jpeg`            | JPEG Huffman + IDCT decode ~3k LOC. Audited, fuzzed, zero deps. Phase 2 only. | 0–1 transitive |
| `lexopt`           | CLI binary only   | Arg parser, 800 LOC zero deps. Replaces `clap` and its ~15 deps. | 0 transitive |

`rsteg-bmp` and `rsteg-wav` have zero deps.

### Optional performance feature (off by default)

| Crate | Feature | Trade |
|-------|---------|-------|
| `libdeflater` | `png-fast` | C `libdeflate` via FFI. 2–3× faster PNG compress/decompress than `miniz_oxide`. Pulls a C dep and breaks the pure-Rust posture. Off by default, on-label only for users who need max throughput. |

### Dev-only deps (not shipped)

| Crate | Scope | Purpose |
|-------|-------|---------|
| `libfuzzer-sys`   | `fuzz/`          | cargo-fuzz harness |
| `stegano-core`    | `rsteg-bench/`   | Comparison target in benchmarks |
| `cargo-mutants`   | nightly CI       | Mutation testing (non-blocking) |

Dev deps do not count against the default-build budget but must still be license-compatible for redistribution of dev artifacts (test corpora).

### Budget

Measured on `rsteg-cli` (the shipped binary), *not* the full workspace —
dev-only crates (`rsteg-bench`, `rsteg-site-build`) sit outside
this budget because they never reach a user.

| Config                                               | Target | Current |
|------------------------------------------------------|-------:|--------:|
| Default (`png`, `bmp`, `wav`, `crypto-aead`)         |  ≤ 40  |    ~35  |
| Minimal (`--no-default-features --features png`)     |   ≤ 2  |       1 |
| Planned `jpeg` feature adds                          |   ≤ 3  |     TBD |
| Planned `compat-steghide` feature adds               |   ≤ 5  |     TBD |

Reproduce with:

```sh
cargo tree -p rsteg-cli --prefix none | sort -u | wc -l
cargo tree -p rsteg-cli --no-default-features --features png --prefix none | sort -u | wc -l
```

The default target was widened from the original ≤ 30 to ≤ 40 in phase 1
once the Argon2id + XChaCha20-Poly1305 stack was wired in (`argon2`,
`chacha20poly1305`, `zeroize`, plus their RustCrypto transitives). Going
back under 30 would mean switching to HKDF + PBKDF2 or dropping AEAD for
plain ChaCha20, both of which trade off security properties the spec-06
threat model explicitly requires — so the budget was raised rather than
the crypto weakened.

`png-fast` feature (if introduced) is explicitly out of the budget —
users opt in.

CI fails the build if either of the two `cargo tree` counts exceeds the
target column above.

### Review process

- Adding / upgrading a direct dep requires updating this spec in the same PR.
- Upgrading across a major version: a second reviewer reads the upstream changelog and diffs the source of the changed crate.
- `cargo deny check` runs in CI: advisories, licenses, bans (proc-macro bans, duplicate-version limits), sources.
- `cargo vet` (optional, phase 2) — imports trust chains from well-known orgs (RustCrypto, rust-lang) and flags everything else for local review.

### What this costs

- Every convenience we give up (no `serde`, no `thiserror`, no `anyhow`, no `clap`) is extra code we write and maintain. This is priced in. The policy is not "minimize at all costs" — it is "justify each one and keep the count small."
- Ergonomic loss from the `thiserror` ban is tiny (hand-rolled `Display` impls, ~5 lines per error variant).
- Ergonomic loss from the `clap` ban is moderate — ~30–80 lines of arg parsing per subcommand. Offset by the ~15 transitive deps and the derive macro we avoid.
- Consistency note on "trust signal": we rank deps by **supply-chain weight** (LOC + transitive count + unsafe surface). Single-author crates can meet the bar if the code fits on one screen and the author has a track record. Proc-macro status is a hard no regardless of author.
