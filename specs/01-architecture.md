## Architecture

### Principles

- **Hexagonal**: `rsteg-core` defines ports (traits); format and crypto crates are adapters. The CLI is one entrypoint; the library API is another.
- **No runtime reflection / dynamic loading**: plugins are compile-time Cargo features. Aligns with the supply-chain goal.
- **Minimal deps in core**: `rsteg-core` depends only on `std` (optionally `getrandom` if RNG ever moves in; currently stays in `rsteg-crypto`). Every other crate in the workspace takes deps from the reviewed allowlist in spec 02.
- **Additive features**: enabling a feature never changes the behavior of another. Turning off `jpeg` must not affect PNG output bit-for-bit. CLI default behavior is derived from CLI args alone, not from compile-time feature flags (see spec 03 on `--crypto` default).

### Workspace layout

```
rsteg/
  Cargo.toml                  # [workspace] only — no package
  crates/
    rsteg-core/               # Traits, Error, Registry, PayloadHeader, bits, crc32, prng
    rsteg-png/                # feature gate: png
    rsteg-bmp/                # feature gate: bmp
    rsteg-wav/                # feature gate: wav
    rsteg-jpeg/               # feature gate: jpeg        (phase 2)
    rsteg-crypto/             # feature gates: aead, compat-steghide
    rsteg-cli/                # binary; aggregates features
    rsteg-bench/              # dev-only harness (publish = false)
  fuzz/                       # cargo-fuzz targets, dev-only
  specs/                      # design docs (this folder)
  corpus/                     # test carrier fixtures (small)
```

Note: `rsteg-crypto-aead` + `rsteg-compat-steghide` are folded into a single `rsteg-crypto` crate with two features. The two implementations share `sha2`, `getrandom`, `zeroize`, and significant utility code.

`rsteg-bmp` and `rsteg-wav` stay separate crates (they share the `rsteg-core::bits` iterator, not code in each other). This keeps fuzzing and feature-flag minimization honest.

### Dependency graph

```
rsteg-cli
  ├── rsteg-core
  ├── rsteg-png            (feat: png)
  ├── rsteg-bmp            (feat: bmp)
  ├── rsteg-wav            (feat: wav)
  ├── rsteg-jpeg           (feat: jpeg)
  └── rsteg-crypto         (feat: crypto, compat-steghide)

rsteg-png, rsteg-bmp, rsteg-wav, rsteg-jpeg → rsteg-core
rsteg-crypto                                → rsteg-core
```

No crate depends sideways on another adapter crate. Adapters compose only through `rsteg-core` traits.

### Plugin model

Compile-time registry. `rsteg-core` defines `Registry` (spec 04) holding `&'static dyn` trait object references — no heap allocation.

Each adapter crate exposes a `pub static` singleton:

```rust
// in rsteg-bmp/src/lib.rs
pub static BMP_ADAPTER: BmpAdapter = BmpAdapter;
```

`rsteg-cli` builds the registry once at startup:

```rust
fn build_registry() -> Registry {
    let mut r = Registry::default();
    #[cfg(feature = "png")] r.formats.push(&rsteg_png::PNG_ADAPTER);
    #[cfg(feature = "bmp")] r.formats.push(&rsteg_bmp::BMP_ADAPTER);
    #[cfg(feature = "wav")] r.formats.push(&rsteg_wav::WAV_ADAPTER);
    #[cfg(feature = "jpeg")] r.formats.push(&rsteg_jpeg::JPEG_ADAPTER);
    #[cfg(feature = "compat-steghide")] r.detectors.push(&rsteg_crypto::compat::STEGHIDE_DETECTOR);
    #[cfg(feature = "crypto")] r.cryptos.push(&rsteg_crypto::aead::XCHACHA20_ARGON2);
    r
}
```

No `inventory`, no `linkme`, no proc-macro registries. As the adapter set grows past ~6, we will introduce a small `macro_rules!` `register_formats!(png, bmp, wav, jpeg)` in `rsteg-cli` to reduce merge-conflict surface. This is a no-dep change when it lands.

Library users skip the registry and call adapters directly:

```rust
use rsteg_bmp::BMP_ADAPTER;
use rsteg_core::{EmbedOpts, Density};

let stego = BMP_ADAPTER.embed(&cover_bytes, &framed, &EmbedOpts {
    scheme: None,
    density: Density::Low,
})?;
```

### Feature matrix

| Feature | Default | Pulls crate | Adds transitive deps |
|---------|---------|-------------|----------------------|
| `png`              | yes | `rsteg-png`         | `miniz_oxide` (~1) |
| `bmp`              | yes | `rsteg-bmp`         | 0 |
| `wav`              | yes | `rsteg-wav`         | 0 |
| `jpeg`             | no  | `rsteg-jpeg`        | `zune-jpeg` (~1) |
| `crypto`           | yes | `rsteg-crypto`      | RustCrypto (~6 with argon2, chacha20poly1305, sha2, zeroize) + `getrandom` |
| `compat-steghide`  | yes | `rsteg-crypto`      | additional `aes`, `cbc`, `md-5` (~3) |
| `png-fast`         | no  | `rsteg-png`         | `libdeflater` (C FFI, opt-in only) |

Budgets (enforced in CI):
- Default features: ≤ **30 total transitive crates**.
- Minimal `--no-default-features --features png`: ≤ **2**.
- `--all-features` excluding `png-fast`: ≤ **35**.

### Entry points

- **Binary**: `rsteg` (from `rsteg-cli`). Spec 03 defines its surface.
- **Library**: `rsteg-core` + any adapter crates. No facade aggregation crate in phase 1; users pick what they need. Revisit in phase 1.5 if users report import fatigue.

### What lives in `rsteg-core`

- Trait definitions (spec 04).
- `Error` enum.
- `PayloadHeader` — 32-byte framing.
- `Capacity` struct.
- Bit iterator (`bits::BitReader`, `bits::BitWriter`, scalar + SIMD fast path in `bits::simd`).
- CRC-32/IEEE, slicing-by-8 (8 KB static table).
- SplitMix64 PRNG (`prng`) for test determinism and passphrase-seeded permutation.
- No file I/O. Everything is `&[u8]` in, `Vec<u8>` out or `&mut Vec<u8>` out for the zero-alloc path.

### `no_std`

Out of scope for v1. Rationale: OS entropy, allocator, `std::io::Read/Write` for the CLI all assume `std`. Revisit if an embedded use case materializes.
