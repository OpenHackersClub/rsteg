# Security policy

This document covers what `rsteg` protects against, what it explicitly
does *not* protect against, and how to report a vulnerability. Read
[`specs/06-crypto.md`](specs/06-crypto.md) for the cryptographic primitives
and [`specs/04-core-traits.md`](specs/04-core-traits.md) for the embedding
invariants the rest of this document refers to.

## Reporting a vulnerability

**Preferred:** open a private security advisory at
<https://github.com/OpenHackersClub/rsteg/security/advisories/new>.
GitHub routes the report to the maintainers without exposing the issue.

**Alternate:** email `v@fractalbox.dev`.

Please include:
- The affected version (commit hash or release tag).
- A reproducer — input bytes if possible, otherwise a minimal Rust test.
- Whether the issue affects the AEAD path, a format adapter, or the FFI
  layer in `rsteg-wasm`.

I aim to acknowledge within 7 days and ship a fix or a documented
mitigation within 30 days for issues that affect the AEAD or FFI surface.
Format-parser hardening (panic / OOM on malformed input) is treated as
fuzz-discovered and rolled into the regular release cadence.

## What rsteg guarantees (AEAD path)

When a payload is encrypted via the default scheme `xchacha20-argon2id`:

- **Confidentiality.** The body is encrypted with XChaCha20-Poly1305
  (IETF, 256-bit key, 192-bit nonce, 128-bit tag) using a key derived
  from the passphrase via Argon2id (`m=64 MiB, t=3, p=1`, OWASP-2023
  parameters). An attacker without the passphrase learns nothing about
  the plaintext beyond its length (modulo a fixed 58-byte AEAD overhead).
- **Integrity.** The full 32-byte `PayloadHeader` plus the 42-byte
  inner crypto header (kdf_id, kdf_version, salt, nonce) are bound into
  the Poly1305 tag as associated data. Tampering with *any* field —
  format fourcc, density, scheme fourcc, salt, nonce, body length —
  fails authentication.
- **Indistinguishable failure modes.** Wrong passphrase, truncated
  ciphertext, tampered outer header, and out-of-range KDF parameters
  all surface as a single `BadPassphrase` error. The
  `rsteg-crypto-aead::open` implementation has a timing-channel test
  in spec 07.
- **No nonce reuse.** Each `seal()` draws a fresh 16-byte salt and a
  fresh 24-byte nonce from the OS RNG (`getrandom`). Failure to obtain
  entropy is a hard stop; there is no fallback to a non-entropy source.
- **Zeroization on drop.** Passphrases, derived keys, and intermediate
  state live in `zeroize::Zeroizing<...>` wrappers and are wiped when
  they go out of scope.

## What rsteg does NOT guarantee

Steganography is a layered defence; rsteg's threat model is narrow.

### Presence-detection resistance is limited

- **Linear LSB schemes** (`bmp-lsb-linear`, `wav-lsb-linear`,
  `png-lsb-linear`) preserve enough statistical structure that a
  chi-square or RS-analysis attacker can detect the presence of an
  embedded payload from the carrier alone. They do not hide *that* a
  message was hidden — only what it says (when AEAD is also used).
- **Permuted LSB schemes** (`bmp-lsb-permuted`, `wav-lsb-permuted`)
  spread the embedding bits according to a passphrase-derived
  permutation, raising the cost of presence detection but not
  eliminating it.
- For high-stakes presence concealment, no LSB scheme is sufficient.
  A different carrier (JPEG-F5 in phase 2; matrix or graph-matching
  embedding in phase 3) or a different tool is appropriate.

### Side channels

- Timing leaks across the AEAD `open` path are guarded by spec-07's
  timing-channel test. Side channels outside that test (cache, power,
  electromagnetic) are not in scope.
- Embedding throughput is roughly linear in carrier size; that does not
  leak payload-presence on its own, but a passive observer with both
  the cover and the stego file has a direct comparison and rsteg makes
  no attempt to defeat that.

### Compat-steghide (when it ships)

Phase 2 will introduce `rsteg-compat-steghide` for **read-only** decoding
of files written by `steghide`. It uses steghide's MD5-based KDF and
LCG PRNG — both weak by modern standards. The compat crate is for
recovering data already written by steghide; rsteg will never *write*
in steghide's wire format. Files produced by the compat path provide
none of the guarantees in the §"AEAD path" section above.

### Format adapters

- BMP, WAV, PNG, and (eventually) JPEG parsers operate on attacker-
  controlled bytes. `#![forbid(unsafe_code)]` is enforced workspace-wide,
  so memory safety is a function of the standard library and `miniz_oxide`
  (PNG only). Panics on malformed input are bugs and will be fixed; OOM
  via `Vec::reserve` on adversarial inputs is bounded by an explicit
  256 MiB cap in `rsteg-wasm` but is the operating-system's problem in
  the native CLI.
- Fuzz nightlies are listed in [`specs/09-roadmap.md`](specs/09-roadmap.md)
  as a phase-1.5 deliverable. Until they land, treat parser robustness
  on adversarial inputs as best-effort.

### `rsteg-wasm` (browser façade)

- The `extern "C"` ABI is hand-rolled with explicit `// SAFETY:` comments
  per spec-02 rule 5. The unsafe surface is small and reviewable.
- WASM does not magically protect a passphrase from a hostile JS
  environment. If the page hosting the module is compromised, the
  passphrase the user types is compromised. The "no trusted server"
  property buys you the right to choose your hosting environment;
  it does not buy you confidentiality against a malicious hosting
  environment.

## Out of scope (non-goals)

- Hiding the *fact* that `rsteg` was used to write a file (the carrier's
  metadata may give it away; presence concealment is a stronger property
  than rsteg targets).
- Resistance to active warden attacks that re-encode the carrier.
- Forward secrecy. Each message is independently sealed under the
  passphrase; there are no chained or rotating keys.
- Cover-source attribution (proving the cover came from a particular
  camera / encoder).

## Cryptographic agility

The crypto fourcc (`b"XCA1"` for the current scheme) lives in the
`PayloadHeader`. Future schemes get new fourccs; old fourccs continue
to decode for as long as the corresponding scheme crate is in the
workspace. There is no implicit downgrade — a payload sealed under
`XCA1` is rejected by any future scheme that expects different
parameters, and the rejection is indistinguishable from `BadPassphrase`.
