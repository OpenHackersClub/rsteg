## Core traits (`rsteg-core`)

`rsteg-core` owns the contracts. `std` + possibly `getrandom` only. `#![deny(unsafe_code)]` with per-module `#[allow(unsafe_code)]` only for the SIMD LSB fast path (documented in spec 02). No file I/O. Slices in, `Vec<u8>` out **or** into a caller-supplied `&mut Vec<u8>` for the zero-alloc hot path. No_std is **out of scope** for v1 (allocator + OS entropy assumptions).

### Design principles

1. **Pure core, mutable outputs at the edge.** Embed/extract are pure functions of input bytes plus OS entropy (crypto only). The `_into` variants let callers reuse buffers.
2. **Closed adapter set at compile time, extensible by re-exporting from downstream crates.** Registry stores `&'static dyn` — no `Box`, no allocation per lookup.
3. **One central `Error` enum facing users, adapter-specific errors wrapped.** Single surface for match-exhaustive error handling without sacrificing locality.
4. **Self-describing stego files.** The 32-byte `PayloadHeader` records everything the extractor needs: scheme, density, crypto fourcc. No "try every combination" on extract.
5. **Authenticated framing when encrypted.** The full `PayloadHeader` is bound into the AEAD AAD. Rollback / downgrade / tamper of any header field fails tag verification.

### Trait surface

```rust
// -- Format adapters -------------------------------------------------------

/// A carrier format (PNG, BMP, WAV, JPEG, ...).
pub trait FormatAdapter: Send + Sync + 'static {
    /// Stable short id (e.g. "png", "bmp"). Matches `--format` values.
    fn id(&self) -> &'static str;

    /// Human-readable name with variant info.
    fn describe(&self, carrier: &[u8]) -> Result<Description, Error>;

    /// Cheap check: can this adapter parse the bytes?
    fn recognize(&self, bytes: &[u8]) -> bool;

    /// Enumerate schemes this adapter implements.
    fn schemes(&self) -> &'static [SchemeMeta];

    /// Maximum payload bytes (excluding PayloadHeader + AEAD overhead) for given opts.
    fn capacity(&self, carrier: &[u8], opts: &EmbedOpts) -> Result<Capacity, Error>;

    // -- Hot path: embed / extract --------------------------------------

    /// Embed `framed` (PayloadHeader + body) into `carrier`. Writes to `out`,
    /// reusing capacity if available. Never panics.
    fn embed_into(
        &self,
        carrier: &[u8],
        framed: &[u8],
        opts: &EmbedOpts,
        out: &mut Vec<u8>,
    ) -> Result<(), Error>;

    /// Extract framed bytes (PayloadHeader + body) from `stego` into `out`.
    /// The caller is responsible for parsing the header and decrypting.
    fn extract_into(
        &self,
        stego: &[u8],
        opts: &ExtractOpts,
        out: &mut Vec<u8>,
    ) -> Result<(), Error>;

    // -- Optional: in-place embed for formats where it's sound ----------

    /// Formats that can modify the carrier bytes without re-encoding
    /// (BMP, WAV) override this. Default: false.
    fn supports_embed_in_place(&self) -> bool { false }

    /// In-place embed. Default implementation returns `FormatUnsupported`.
    fn embed_in_place(
        &self,
        _carrier: &mut [u8],
        _framed: &[u8],
        _opts: &EmbedOpts,
    ) -> Result<(), Error> {
        Err(Error::FormatUnsupported {
            id: self.id(),
            reason: "in-place embed not supported by this format",
        })
    }

    // -- Convenience wrappers --------------------------------------------

    /// Allocate + embed. Default: calls `embed_into` with a fresh `Vec`.
    fn embed(&self, carrier: &[u8], framed: &[u8], opts: &EmbedOpts)
        -> Result<Vec<u8>, Error>
    {
        let mut out = Vec::with_capacity(carrier.len());
        self.embed_into(carrier, framed, opts, &mut out)?;
        Ok(out)
    }

    /// Allocate + extract. Default: calls `extract_into`.
    fn extract(&self, stego: &[u8], opts: &ExtractOpts) -> Result<Vec<u8>, Error> {
        let mut out = Vec::new();
        self.extract_into(stego, opts, &mut out)?;
        Ok(out)
    }
}

// -- Detection ------------------------------------------------------------
//
// **Status:** TBD — this trait is not yet in `rsteg-core`. The `inspect` CLI
// verb (spec 03) currently does a linear-probe magic-bytes check inline in
// `rsteg-cli::run_inspect`; lifting that into the `Detector` framework below
// is phase-1.5/phase-2 work. Until then, treat this section as the target
// shape rather than as-shipped API.

pub trait Detector: Send + Sync + 'static {
    fn id(&self) -> &'static str;
    fn probe(&self, bytes: &[u8], passphrase: Option<&[u8]>) -> DetectorReport;
}

pub struct DetectorReport {
    pub hits: Vec<DetectionHit>,
}

pub struct DetectionHit {
    pub scheme: &'static str,
    pub source: ConfidenceSource,
    pub level:  ConfidenceLevel,
    pub needs_passphrase: bool,
    pub notes: Option<&'static str>,
}

pub enum ConfidenceSource {
    RstegHeader,        // found b"RSTG" magic at the expected position
    SteghideMagic,      // found steghide's "shm" pattern after passphrase-derived permutation
    ChiSquare,          // statistical LSB uniformity test
    EntropyDelta,       // difference in entropy from natural carrier baseline
    FormatEligible,     // this format could contain stego for this scheme; no positive signal
}

pub enum ConfidenceLevel {
    Negative,       // tested and did not find the signal (e.g. chi-square looks clean)
    Inconclusive,
    Suspected,      // statistical or eligibility-based signal
    Confirmed,      // MAC/tag verification succeeded — requires passphrase
}

// -- Crypto ---------------------------------------------------------------

pub trait CryptoScheme: Send + Sync + 'static {
    fn id(&self) -> &'static str;
    fn fourcc(&self) -> [u8; 4];

    /// Produce ciphertext bound to `aad`. Output layout is scheme-defined but
    /// self-contained (salt + nonce + tag + KDF params as needed).
    /// `aad` MUST include the outer PayloadHeader bytes so that downgrade /
    /// rollback attacks on the header fail tag verification.
    fn seal(
        &self,
        plaintext: &[u8],
        passphrase: &[u8],
        aad: &[u8],
    ) -> Result<Vec<u8>, Error>;

    /// Authenticate + decrypt. On any failure (wrong passphrase, truncated
    /// ciphertext, modified aad) returns `Error::BadPassphrase`. No error
    /// variant distinguishes these cases — that is a hard invariant.
    fn open(
        &self,
        ciphertext: &[u8],
        passphrase: &[u8],
        aad: &[u8],
    ) -> Result<Vec<u8>, Error>;
}

// -- Options & metadata ---------------------------------------------------

#[derive(Clone, Debug)]
pub struct EmbedOpts {
    pub scheme: Option<&'static str>,   // None => adapter default
    pub density: Density,
    /// 64-bit PRNG seed for permuted schemes. `None` with a permuted scheme
    /// is a caller bug — the adapter returns `Error::PermutationSeedRequired`.
    /// For linear schemes, `seed` is ignored.
    pub seed: Option<u64>,
}

#[derive(Clone, Debug)]
pub struct ExtractOpts {
    pub scheme: Option<&'static str>,
    pub density: Option<Density>,       // None => read from header
    pub skip_header: bool,              // true for --no-header raw reads
    pub raw_bit_count: Option<u64>,     // required when skip_header = true
    /// Must match the writer's seed for permuted schemes; ignored for linear.
    pub seed: Option<u64>,
}

/// Embedding density. Permitted values: Low(1), Moderate(2), Aggressive(3|4).
/// Construction enforces the cap — you cannot build `Aggressive(5)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Density {
    Low,          // 1 bit per embedding unit (default, statistically safest)
    Moderate,     // 2 bits per embedding unit (capacity/detectability knee)
    Aggressive3,  // 3 bits — opt-in; visible distortion likely
    Aggressive4,  // 4 bits — opt-in; visible distortion likely
}

impl Density {
    pub fn bits(self) -> u8 { match self { Self::Low => 1, Self::Moderate => 2, Self::Aggressive3 => 3, Self::Aggressive4 => 4 } }
}

pub struct SchemeMeta {
    pub id: &'static str,                 // e.g. "bmp-lsb-linear"
    pub fourcc: [u8; 4],                  // stored in PayloadHeader.scheme_fourcc
    pub description: &'static str,
    pub default_density: Density,
    pub supports_permuted: bool,
}

pub struct Description {
    pub id: &'static str,
    pub variant: &'static str,            // interned; static string. No allocation.
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub sample_rate: Option<u32>,
    pub bit_depth: Option<u8>,
    pub channels: Option<u8>,
}

pub struct Capacity {
    pub bytes_raw: u64,                   // pre-header, at given density
    pub bytes_usable_plain: u64,          // raw - sizeof(PayloadHeader)
    pub bytes_usable_aead: u64,           // raw - sizeof(PayloadHeader) - aead_overhead
    pub density: Density,
    pub scheme: &'static str,
}

// -- Registry -------------------------------------------------------------
//
// **Status:** TBD as a type in `rsteg-core`. The registry exists today as
// compile-time wiring in `rsteg-cli::build_registry()` via `#[cfg(feature)]`
// blocks — pulling it into a first-class `Registry` struct would let library
// consumers mint their own without reimplementing the feature-gated assembly.
// Same for `Capacity` above: adapters return capacity via per-adapter helpers
// rather than a shared `Capacity` struct. Phase 2.

#[derive(Default)]
pub struct Registry {
    pub formats:   Vec<&'static dyn FormatAdapter>,
    pub detectors: Vec<&'static dyn Detector>,
    pub cryptos:   Vec<&'static dyn CryptoScheme>,
}

impl Registry {
    pub fn format_for(&self, bytes: &[u8]) -> Option<&'static dyn FormatAdapter> { ... }
    pub fn format_by_id(&self, id: &str) -> Option<&'static dyn FormatAdapter> { ... }
    pub fn crypto_by_id(&self, id: &str) -> Option<&'static dyn CryptoScheme> { ... }
    pub fn crypto_by_fourcc(&self, f: [u8; 4]) -> Option<&'static dyn CryptoScheme> { ... }
}
```

Registry entries are `&'static dyn` pointing at `pub static` singletons in each adapter crate (`pub static BMP_ADAPTER: BmpAdapter = BmpAdapter;`). Zero heap allocation; dispatch is still a virtual call but the compiler can often devirtualize at the library-direct call site.

### `PayloadHeader` — on-carrier framing

**32 bytes. Big-endian integers.** The full header is bound into the AEAD AAD when encrypted, so any modification of any field fails tag verification.

```
offset  size  field              notes
0       4     magic              b"RSTG"
4       1     version            currently 1; readers reject unknown versions
5       1     flags              bit0 encrypted, bit1 compressed, bit2 permuted,
                                 bit3..7 reserved-must-be-zero
6       4     crypto_fourcc      e.g. b"XCA1" (XChaCha20-Poly1305 + Argon2id v1);
                                 b"STGH" (steghide compat read); zeros if !encrypted
10      4     scheme_fourcc      e.g. b"BLSL" (BMP LSB linear),
                                         b"BLSP" (BMP LSB permuted),
                                         b"WLSL"/b"WLSP" (WAV), b"PLSL"/b"PLSP" (PNG).
                                 Full list in `rsteg_core::SchemeFourcc`. Density is
                                 carried by the separate `density` byte, not the fourcc.
14      1     density            1..=4
15      1     reserved_1         must be zero
16      4     body_len           BE u32, bytes of body (ciphertext if encrypted)
20      4     body_crc32         CRC-32/IEEE of body. MUST be zero when encrypted
                                 (Poly1305 provides authentication).
24      8     reserved_2         must be all zero
```

- Readers reject nonzero reserved bytes with `Error::HeaderReservedBitsSet`. Keeps the extensibility channel clean — we can define what those bytes mean in v2 without v1 readers silently ignoring them.
- `body_len` covers ciphertext length when encrypted (not plaintext). Max 4 GiB.
- `scheme_fourcc` exists so extract is fully self-describing: caller doesn't guess density × scheme × crypto.
- **Header occupies the first `(32 * 8 / density_bits)` embedding units** of the carrier. At `density=Low` (1 bit): 256 units (bytes for BMP/WAV). At `density=Aggressive4`: 64 units.
- **Permuted placement**: when `flags.permuted = 1`, header bytes are placed at passphrase-derived positions (not contiguous at the start). The reader, without the passphrase, sees no `RSTG` magic at any deterministic offset — closing the "rsteg files are identifiable without a key" gap.

### `PayloadHeader` API

```rust
impl PayloadHeader {
    pub const SIZE: usize = 32;
    pub const MAGIC: &[u8; 4] = b"RSTG";

    pub fn encode(&self) -> [u8; 32];
    pub fn decode(bytes: &[u8]) -> Result<Self, Error>;
    /// AAD scope for AEAD = the full 32-byte encoded header.
    pub fn aad(&self) -> [u8; 32] { self.encode() }
}
```

### `Error` enum

A single enum in `rsteg-core`. Adapters wrap their own errors into `Error::Adapter`.

```rust
pub enum Error {
    // -- Format parsing / recognition --
    FormatUnrecognized,
    FormatUnsupported { id: &'static str, reason: &'static str },
    Malformed { at: &'static str, detail: &'static str },

    // -- Capacity --
    PayloadTooLarge { needed: u64, available: u64 },

    // -- Header / extraction --
    HeaderMissing,
    HeaderBadMagic,
    HeaderBadVersion(u8),
    HeaderReservedBitsSet,
    BodyCrcMismatch,                   // only emitted in plaintext mode

    // -- Crypto --
    CryptoSchemeUnknown([u8; 4]),
    CryptoSchemeDisabled { id: &'static str },
    BadPassphrase,                     // sole failure for any encrypted read
    PassphraseRequired,                // file is encrypted, caller supplied none
    UnexpectedPassphrase,              // caller supplied one, file is plaintext
    KdfParams { detail: &'static str },

    // -- RNG / system --
    RngUnavailable,                    // hard stop; never soft-fallback

    // -- Policy --
    DensityOutOfRange(u8),
    AggressiveDensityNotAllowed(Density),

    // -- Adapter-wrapped --
    Adapter {
        adapter: &'static str,
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}
```

Hand-written `Display` + `std::error::Error::source()` chain. No `thiserror`.

**Error-oracle invariants (enforced by tests):**
- For any encrypted file, `extract` only returns `BadPassphrase` on any authentication failure. It must not distinguish "wrong passphrase" from "tampered ciphertext" from "malformed inner-crypto-header".
- Timing of the three failure modes above must be within 2σ on a warmed run (spec 07 test).
- `BodyCrcMismatch` is unreachable in encrypted mode (it's bound to AAD, so tamper fails as `BadPassphrase` before CRC runs).

### Bits & CRC

- **Bit iterator**: `rsteg-core::bits::BitWriter` + `BitReader` operate on `&mut [u8]` at density 1..=4. Scalar default; SIMD fast-path in `bits::simd` (x86_64 BMI2 `_pdep_u64`, aarch64 NEON). The SIMD module is the only place `#[allow(unsafe_code)]` is permitted in `rsteg-core`.
- **CRC-32/IEEE, slicing-by-8.** 8 KB static table in `rsteg-core::crc32`. Hand-implemented, covered by unit tests against known vectors (empty = 0x00000000, "123456789" = 0xCBF43926).
- **PRNG for permutation**: splitmix64 seeded from `Blake2b(passphrase || salt)[0..8]`. Hand-rolled, ~10 lines, in `rsteg-core::prng`.

### Encoding & wire compatibility

- All multi-byte integers big-endian.
- Version is a single byte; a reader must refuse unknown versions rather than try to be forward-compatible.
- The registry guarantees `id()` strings are unique; `fourcc()` values for crypto schemes and `scheme_fourcc` values are unique too. Uniqueness is tested by a `#[test]` in `rsteg-core` that walks a registry built with *all* features on.

### Thread-safety

All trait objects are `Send + Sync + 'static`. Adapters hold no mutable state — they are unit structs or hold immutable config. Concurrency is the caller's problem.

### Library usage sketch

```rust
use rsteg_core::{EmbedOpts, Density, PayloadHeader};
use rsteg_bmp::BMP_ADAPTER;
use rsteg_crypto::XCHACHA20_ARGON2;

fn hide(cover: &[u8], plaintext: &[u8], passphrase: &[u8]) -> Result<Vec<u8>, rsteg_core::Error> {
    let scheme = BMP_ADAPTER.schemes().iter().find(|s| s.id == "bmp-lsb-permuted").unwrap();
    let header = PayloadHeader::encrypted(
        XCHACHA20_ARGON2.fourcc(),
        scheme.fourcc,
        Density::Low,
        /* body_len filled after seal */ 0,
    );
    let body = XCHACHA20_ARGON2.seal(plaintext, passphrase, &header.aad())?;
    let framed = header.with_body_len(body.len() as u32).encode_with(&body);
    BMP_ADAPTER.embed(cover, &framed, &EmbedOpts { scheme: Some(scheme.id), density: Density::Low })
}
```

High-level helper in `rsteg-core` hides the `with_body_len` dance:

```rust
pub fn embed_with(
    registry: &Registry,
    carrier: &[u8],
    payload: &[u8],
    passphrase: Option<&[u8]>,
    opts: &EmbedOpts,
) -> Result<Vec<u8>, Error> { ... }
```
