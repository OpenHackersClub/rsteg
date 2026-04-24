## Crypto

**Implementation status:**

- `rsteg-crypto-aead` — **shipped** in phase 1 (see `crates/rsteg-crypto-aead/`). Wire fourcc `b"XCA1"`, exported as `rsteg_crypto_aead::FOURCC`. Implements the `xchacha20-argon2id` scheme described below.
- `rsteg-compat-steghide` — **TBD (phase 2).** Spec'd below; no crate under `crates/` yet. The "Compatibility: steghide" section from ### onwards should be read as the target design, not shipped API.

The earlier design called for a single `rsteg-crypto` crate with two Cargo features. Phase 1 landed the AEAD path as a standalone crate (`rsteg-crypto-aead`); `compat-steghide` will ship as a separate crate (`rsteg-compat-steghide`) rather than a feature — matching the spec-01 "one adapter per crate" rule. Shared concerns (`sha2`, `zeroize`, `getrandom`) will come through workspace deps when the second crate lands.

### Default scheme: `xchacha20-argon2id` (`rsteg-crypto` feature `aead`)

**Primitives:**
- KDF: **Argon2id** (RFC 9106), parameters:
  - `m_cost = 65536` (64 MiB)
  - `t_cost = 3`
  - `p_cost = 1`
  - Output length: 32 bytes (256-bit key)
  - Salt: 16 bytes, OS entropy.
- AEAD: **XChaCha20-Poly1305** (IETF variant, 256-bit key, 192-bit nonce, 128-bit tag). Nonce is 24 bytes from OS entropy. The extended nonce eliminates practical risk of nonce-collision across many encryptions with the same key (birthday-bound on 192 bits is effectively unreachable).

**Crates:** `argon2`, `chacha20poly1305` (with `XChaCha20Poly1305`), `sha2`, `getrandom`, `zeroize` (all RustCrypto except `getrandom`). See spec 02.

**Fourcc:** `b"XCA1"` (XChaCha20-Poly1305 + Argon2id v1). Future KDF or AEAD changes bump the trailing digit.

**Ciphertext layout** (output of `seal`, input to `open`):
```
offset  size  field
0       1     kdf_id            (2 = Argon2id; 1 = PBKDF2-HMAC-SHA256 fallback, see below)
1       1     kdf_version       (1 = OWASP 2023 params listed above)
2       16    salt              OS RNG
18      24    nonce             OS RNG
42      N     ciphertext        plaintext_len == ciphertext_len for XChaCha20
42+N    16    poly1305_tag
```

Overhead: **58 bytes** per payload (ciphertext + scheme metadata), independent of payload size.

**AAD scope for `seal`/`open`**:
```
aad = outer_payload_header (32 bytes) || inner_crypto_header (bytes 0..42 above)
    = 74 bytes total
```

This binds **everything** the attacker could tamper with (version, flags, fourcc, density, scheme, body_len, kdf params, salt, nonce) into the Poly1305 tag. Any modification fails tag verification.

**Key derivation:**
```
key = Argon2id(passphrase, salt, m=65536, t=3, p=1, out_len=32)
```
Key material lives in a `zeroize::Zeroizing<[u8; 32]>` and is zeroed on drop.

**Open flow:**
1. Parse inner_crypto_header. Reject unknown `kdf_id` / `kdf_version` with `Error::KdfParams` — same error for all mismatches (doesn't leak which one).
2. Reconstruct `aad` exactly as the writer did.
3. Derive key from passphrase + salt.
4. Decrypt-and-verify with XChaCha20-Poly1305.
5. **Any authentication failure returns `Error::BadPassphrase`**. No subdivision — "wrong passphrase", "truncated ciphertext", "tampered outer header" all look identical to the caller.

**Fallback (`kdf_id=1`, PBKDF2-HMAC-SHA256@600k)** is retained only so future releases that want a lighter KDF for embedded targets have a slot. It is **not** the default and is not selectable via CLI in phase 1. Reserved for future.

### Randomness

`rsteg-crypto::rng` wraps `getrandom::getrandom` behind a thin function that returns `Err(Error::RngUnavailable)` on failure. `RngUnavailable` is a hard-stop: the CLI prints an explicit message and exits 1. No fallback to any non-entropy source under any condition. A unit test uses `getrandom`'s test-hook to simulate failure and assert we error-out cleanly.

**Nonce-reuse invariant** (spec-mandated, tested):
- Every `seal()` call pulls a fresh 16-byte salt and a fresh 24-byte nonce from `rng`.
- Key caching across salts is forbidden. Implementations must NOT cache an Argon2id-derived key keyed by passphrase alone.
- A test generates `N=1000` `seal()` outputs for the same passphrase + plaintext and asserts all salts and all nonces are unique. (Birthday collision at those sizes is astronomically unlikely; a failure indicates a code bug.)

### Zeroization

- `zeroize::Zeroizing<T>` wraps passphrase buffers and derived keys.
- The AEAD and KDF crates from RustCrypto already zeroize internal state.
- CLI reads passphrases into `Zeroizing<Vec<u8>>` and passes by reference.

### Constant-time requirements

- `chacha20poly1305` uses `subtle::ConstantTimeEq` for tag comparison — verified in upstream source; we pin versions and monitor for regressions in the dependency review.
- Spec 07 includes a timing test: run `extract` against (a) bad header magic, (b) valid header + bad tag, (c) valid header + valid tag + wrong-but-well-formed passphrase-derived key. Median timings for the three must be within 2σ after warmup.

### Test vectors

- **RFC 8439 §2.8.2** adapted for a direct ChaCha20-Poly1305 sanity check (via `XChaCha20` the nonce differs but the core primitive is the same).
- **RFC 9106 Appendix A** Argon2id test vector.
- **Our own fixed-salt/fixed-nonce KATs** for `seal`/`open`: empty payload, 1 B, 1 MiB. Deterministic via a testing constructor that accepts explicit salt + nonce and short-circuits RNG.

### Security posture

- Pre-image resistance: Argon2id@64MiB is the configurable bottleneck. An attacker possessing a stego file and doing dictionary attacks pays 64 MiB × ~400 ms per guess on CPU, meaningful hours-to-days per plausible password even for a large cluster. This is the *right* line of defense for file-at-rest.
- Post-compromise: if the passphrase leaks, everything is lost (as always).
- Misuse-resistance: XChaCha20's 192-bit nonce removes practical reuse concerns even under buggy callers.
- Side channels: constant-time AEAD. Timing invariant on the extract path (see above).
- Rollback/downgrade: all header fields in AAD → not possible without the passphrase.

---

## Compatibility: steghide (`rsteg-crypto` feature `compat-steghide`)

**Scope in phase 1:**
- **Detect** steghide-eligible files across BMP and WAV.
- **Read/decrypt** steghide's BMP and WAV files when the passphrase is supplied.
- **Not write.** We never produce files compatible with steghide.

Phase 2 adds JPEG detection + read once the JPEG adapter lands.

**Fourcc for detection reporting only:** `b"STGH"` — never written into an rsteg-authored header.

### steghide wire format (verified against steghide source v0.5.1)

**Important**: The previous draft of this spec was factually wrong about the KDF. Below is corrected based on `src/MCryptPP.cc`, `src/MHashKeyGen.cc`, `src/EmbData.cc`, `src/PseudoRandomSource.cc`.

**Embedded bitstream layout (pre-encryption):**

```
magic                 25 bits   "shm" (specific bit pattern from steghide's Magic.{h,cc})
version               8 bits
encryption_algorithm  5 bits    index into steghide's algorithm table (default = Rijndael-128 / AES-128)
encryption_mode       3 bits    default = CBC
enc_keysize           5 bits    default = 16 (AES-128)
compression_level     2 bits    0 = off, 1 = best speed, ..., 9 = best size (zlib)
checksum_flag         1 bit     if set, CRC32 present after payload
emb_file_name_nbits   16 bits   length of filename in bits
... filename, compressed/encrypted payload, optional CRC32 ...
```

**Key derivation** (steghide uses **libmhash** `mhash_keygen_ext`):
- Hash algorithm: **MD5** (spec-important: not SHA-256 as previously guessed)
- Scheme: `mhash`'s key-gen produces key + IV from `(passphrase, salt)` via a specific MD5-based stretching defined in libmhash's `keygen.c`. We reproduce this in `rsteg-crypto::compat::mhash_kdf`.
- Salt: steghide's `CryptoFlags.salt` — 16 bytes of constant zero for default settings. Modifiable via CLI flags that most users never touch.

**Encryption:** AES-128-CBC (default). Symmetric mode selected by the 5+3 bit fields above.

**Position permutation (for embedding positions):**
- PRNG: **LCG** (linear congruential generator), not Mersenne Twister. Source: `src/PseudoRandomSource.cc`.
- Seed: 32-bit integer derived from passphrase via `Hash::operator UWORD32()` (a summarizing fold of the passphrase bytes into a `uint32_t`). Exact transform: `src/Hash.cc`.
- LCG constants: defined in `PseudoRandomSource.cc`; we reproduce them bit-exactly.
- Position selection: LCG output × scale factor → next embedding position. Modular-bias corrections are applied per steghide's implementation.

**Our plan:**
1. `rsteg_crypto::compat::steghide_kdf(passphrase) -> (key, iv)` — bit-exact reproduction of libmhash's MD5-based keygen.
2. `rsteg_crypto::compat::steghide_lcg::seed(passphrase) -> u32`, `.next_position(max) -> usize` — bit-exact LCG.
3. `SteghideDetector`:
   - **No passphrase**: run chi-square + sample-pair on carrier LSBs. Report `ChiSquare/Suspected` if LSB distribution is flagged; else `FormatEligible`. Cannot find `shm` magic without the passphrase (permuted).
   - **With passphrase**: seed the LCG, read LSBs in permuted order, look for `shm` magic, parse header, decrypt via `mhash_kdf`, verify CRC32, return plaintext.

**Blocking gates before `compat-steghide` ships:**
1. This spec section must list exact KDF bytes, exact LCG constants — no TBD. Verified in the implementation PR against a round-trip through the real `steghide` binary.
2. Test corpus: at least **5 independently-produced steghide files per format** (BMP + WAV in phase 1, +JPEG in phase 2) with varying dimensions, encryption algorithms, compression levels, and asymmetric plaintexts (1 byte, one block, one block + 1, a long one).
3. A nightly "compat sanity" CI job that generates a fresh steghide file from the `steghide` binary (if present) and round-trips it through rsteg.

### Security notes for steghide-compat

- We ship the compat reader behind its own feature flag so users who don't need it don't pull `aes`, `cbc`, `md-5`, and don't expose themselves to any bug in that code path.
- The compat code is read-only. There is no path from a steghide file into an `rsteg embed` write. Supporting write would obligate us to maintain format bit-compat, which is a maintenance trap.
- `SECURITY.md` (phase 1.5) notes: steghide's choices (AES-128-CBC, MD5-based KDF, no MAC) are known-weak by modern standards. Recovered plaintext should be re-sealed with `xchacha20-argon2id` if ongoing confidentiality matters.

### Crypto scheme registration

`rsteg-core::Registry::cryptos` holds every enabled scheme. Extraction looks up the scheme by `fourcc` from `PayloadHeader.crypto_fourcc`. Unknown fourcc → `Error::CryptoSchemeUnknown`, compiled-out fourcc → `Error::CryptoSchemeDisabled` (different error because the fix differs: "rebuild with feature X" vs "unrecognized scheme"). Note: the steghide-compat reader is **not** selected by crypto_fourcc — steghide files carry no rsteg header — it is selected by the `SteghideDetector` running during `rsteg extract` when the detector scores `Confirmed`.

### Audit posture

- `rsteg-crypto::aead` is glue code. All cryptographic primitives come from audited RustCrypto crates.
- `rsteg-crypto::compat` is a read-only, interop implementation. Every primitive it uses is considered weak for new deployments.
