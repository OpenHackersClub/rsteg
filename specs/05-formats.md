## Formats

Each format is a separate crate implementing `FormatAdapter`. This spec covers: carrier primer, supported variants, embedding schemes, capacity formula, and TDD edge cases.

### Shared concepts

- **Embedding unit**: the smallest addressable chunk we write into. BMP/PNG = one channel byte per pixel; WAV = one PCM value; JPEG = one non-zero AC DCT coefficient (phase 2).
- **Density (d)**: bits modified per embedding unit, encoded as `Density::Low` (d=1), `Density::Moderate` (d=2), `Density::Aggressive3` (d=3), `Density::Aggressive4` (d=4). Default is `Low`. `Aggressive*` requires the CLI flag `--allow-aggressive-density`.
- **Bit ordering**: payload is consumed MSB-first. Deterministic for a given (scheme, density) pair.
- **Header-first (linear scheme) or header-placed (permuted scheme)**:
  - Linear: the 32-byte `PayloadHeader` occupies the first `ceil(256/d)` embedding units.
  - Permuted: header bytes land at passphrase-derived positions via the splitmix64-seeded shuffle (spec 04, `prng`). No contiguous magic at offset 0.
- The permuted scheme is the **default when a passphrase is supplied**. It closes the "rsteg files detectable without passphrase" gap (reviewer finding).

### Scheme defaults per format

| Format | Linear scheme    | Permuted scheme      | Default when password set | Default when no password |
|--------|------------------|----------------------|----------------------------|---------------------------|
| BMP    | `bmp-lsb-linear` | `bmp-lsb-permuted`   | permuted                   | linear                    |
| WAV    | `wav-lsb-linear` | `wav-lsb-permuted`   | permuted                   | linear                    |
| PNG    | `png-lsb-linear` | `png-lsb-permuted`   | permuted                   | linear                    |
| JPEG   | —                | `jpeg-f5` (phase 2)  | `jpeg-f5`                  | `jpeg-f5`                 |

Linear vs permuted is a scheme choice, not a density choice — a permuted scheme still uses the caller's chosen density.

**Why permuted-by-default when encrypted**: our passphrase is already load-bearing for confidentiality via Argon2id + XChaCha20-Poly1305. Reusing it to seed the embedding-position permutation costs nothing at the threat-model level and removes the "detectable magic at offset 0" fingerprint.

---

### BMP (`rsteg-bmp`) — zero deps

**Variants supported in v1:**
- BITMAPFILEHEADER + BITMAPINFOHEADER (DIB header size 40).
- 24-bit uncompressed (`BI_RGB`) — default target.
- 32-bit uncompressed with alpha — supported with uniform-alpha detection (see below).
- Top-down or bottom-up row order.

**Explicit non-goals in v1** (documented in spec 00 as well):
- **Paletted BMP (1/4/8-bit)**: naïve LSB on a palette index changes the palette-entry reference, which — because adjacent palette entries are rarely perceptually similar — produces visually obvious artifacts. Correct embedding requires EzStego-style palette reordering, which is out of scope.
- `BI_RLE` compression, BITMAPV4/V5 headers with color profiles.

**Recognition:** first two bytes `b"BM"`; DIB size field validates.

**Uniform-alpha handling (32-bit):** before embedding, scan the alpha channel. If ≥99% of alpha values are identical (typical: 0xFF opaque, 0x00 unused), the embedder marks alpha as **skip** — embedding uses only R/G/B channels. Rationale: modifying LSB of a channel that's uniformly 0xFF produces an obvious `FF,FE,FF,FE,...` pattern. Decision is recorded via an adapter-specific flag in the inner framing and is round-trip-safe.

**Scheme `bmp-lsb-linear`:**
- Embedding units: every byte of the pixel array (R, G, B, and A if 32-bit and not uniform). Skips header and any row-padding bytes.
- Capacity raw: `(pixel_bytes_usable) * d / 8`. For 24-bit: `width * height * 3 * d / 8`.
- Header reservation: `ceil(32 * 8 / d)` units at the top of the pixel array.

**Scheme `bmp-lsb-permuted`:**
- Same embedding units; position order is `SplitMix64(Blake2b(passphrase || salt)[..8])` → Fisher-Yates shuffle of the embedding-unit index list. Salt comes from the outer crypto (when encrypted; it's the same 16 bytes that feed Argon2id). For an unencrypted-but-permuted use (rare), a passphrase is still required; error otherwise.
- The writer emits header bytes first in the permuted sequence, then body bytes. The reader reproduces the permutation and reads in the same order.

**In-place embed:** `BmpAdapter::supports_embed_in_place() = true`. `embed_in_place(&mut [u8], framed, opts)` modifies the carrier buffer directly — no re-encoding, no copy. CLI uses this path when the user allows overwriting the input file and the output path equals the input path.

**Edge cases / tests:**
- Row padding to 4-byte boundary preserved bit-for-bit.
- Top-down vs bottom-up round-trip.
- 32-bit uniform-alpha skip decision is stable: encode, decode, re-encode yields the same bytes.
- Gap between `bfOffBits` and the pixel data (extra palette bytes) is preserved untouched.
- Minimum carrier: 1×1 BMP fails capacity (header doesn't fit) — verified as `Error::PayloadTooLarge { needed: 32, available: 0 }`.

---

### WAV (`rsteg-wav`) — zero deps

**Variants supported in v1:**
- RIFF/WAVE with PCM `fmt ` chunk (format code 1).
- **16-bit signed little-endian** samples.
- **8-bit unsigned** samples (added for feature parity with steghide; reviewer finding).
- 1 or 2 channels.
- 8 kHz – 192 kHz sample rate.

**Out of scope v1:** 24-bit packed, 32-bit float, extensible formats, ADPCM, RIFX. AU format: non-goal (declared in spec 00).

**Recognition:** `b"RIFF" …… b"WAVE"` and a `fmt ` chunk with format == 1 and bits_per_sample ∈ {8, 16}.

**Scheme `wav-lsb-linear` / `wav-lsb-permuted`:**
- Embedding units: each PCM value (16-bit sample or 8-bit sample — *per channel*). For stereo 44.1 kHz: 88,200 PCM values per second.
- Capacity raw: `pcm_values * d / 8` bytes.
- Chunks other than `fmt ` and `data` are preserved verbatim.

**Terminology note:** spec uses "PCM value" (one number per channel) rather than "sample" (which in audio usually means a frame). Avoids a 2× off-by-one trap on stereo capacity.

**In-place embed:** supported, same as BMP.

**Edge cases:**
- Non-`fmt `/`data` chunks (`LIST`, `INFO`, `JUNK`) round-trip.
- RIFF length field may be a lie (streaming captures often leave it 0xFFFFFFFF). Accept it; reject only if `data` chunk claims more bytes than available.
- Odd-length `data` chunk pad byte preserved.
- 8-bit: sample bytes are unsigned; LSB still straightforward.

---

### PNG (`rsteg-png`) — dep: `miniz_oxide`

**Variants supported in v1:**
- Color types 2 (RGB) and 6 (RGBA); bit depth 8.
- Interlace method 0 (no Adam7) in v1.
- Filters 0–4 (None, Sub, Up, Average, Paeth) on read.

**Out of scope v1:** paletted (type 3), grayscale (types 0, 4), bit depth 16, Adam7 interlacing, APNG.

**Recognition:** 8-byte PNG signature `89 50 4E 47 0D 0A 1A 0A`, IHDR chunk parses, color type in {2, 6}, bit depth 8.

**Filter-choice decision (revised from earlier draft):**
Re-encode with **per-row original filter choices preserved**. The earlier draft chose filter 0 for determinism — reviewers flagged this as a detection fingerprint (a PNG with all filter-0 rows is statistically unusual for natural photos). Preserving the original filter per row keeps the encoder-signature profile of the cover intact. Cost: modest (needs to remember filter bytes from the decode pass).

**Scheme `png-lsb-linear` / `png-lsb-permuted`:**
- Decode pipeline: parse chunks → concatenate IDAT payloads (streaming into `miniz_oxide` with a preallocated output sized from IHDR) → defilter rows (in-place where possible) → raw pixel bytes.
- Embedding units: every pixel byte (R, G, B, and A if RGBA and not uniform-alpha — same rule as BMP).
- Encode pipeline: re-apply original per-row filter → zlib-deflate (level 6 default; bench determines whether level 1 ships as default — see spec 08) → single IDAT → preserve ancillary chunks (tEXt, pHYs, etc.).
- Capacity raw: `width * height * channels_used * d / 8` bytes, where `channels_used` is 3 (RGB) or 4 (RGBA with varying alpha) or 3 (RGBA with uniform alpha).

**In-place embed:** not supported. PNG re-encodes; no way to patch bytes without recompressing.

**Pipeline RSS optimization** (reviewer finding): the naive decode-concat-inflate-defilter-copy-embed-refilter-deflate flow has ~3–4× peak RSS vs the raw pixel buffer. Revised flow targets ~2× peak:
1. Concat IDATs streaming into a preallocated inflate output.
2. Defilter in-place.
3. Embed in-place (via `embed_into` with `out` = defiltered buffer).
4. Refilter in-place (buffer layout stays the same — filter byte per row is updated, data bytes rewritten per filter).
5. Deflate with preallocated output Vec.

**Edge cases:**
- Multiple IDAT chunks on read are concatenated in order.
- Ancillary chunks preserved.
- CRC32 on every chunk recomputed on write.
- Zero-length IDAT: rejected as `Error::Malformed`.

---

### JPEG (`rsteg-jpeg`) — phase 2, dep: `zune-jpeg`

Full spec deferred to phase 2. Baseline decision changed from the earlier draft: **we ship F5 (matrix encoding), not jsteg-style DCT-LSB.**

- **Scheme `jpeg-f5`**: F5 algorithm (Westfeld 2001). Matrix encoding embeds `k` message bits per `2^k - 1` DCT-coefficient group using a parity function; ±1 coefficient adjustment (not LSB replacement). Dramatically lower detectability than jsteg.
- We do **not** ship `jpeg-dct-lsb` (jsteg-style) — it's been fully broken by chi-square since 1999. Shipping it would give users no improvement over existing broken tools and would mislead them into thinking their JPEG stego is safe.
- Graph-matching (`jpeg-dct-graph`) planned for phase 2 or early phase 3 — it's the core reason steghide still exists in 2026 and we want real feature parity, not parity-on-paper.

---

### Scheme ids (summary table)

| Scheme id              | fourcc | Crate         | Phase |
|------------------------|--------|----------------|-------|
| `bmp-lsb-linear`       | `BLSL` | rsteg-bmp      | 1     |
| `bmp-lsb-permuted`     | `BLSP` | rsteg-bmp      | 1     |
| `wav-lsb-linear`       | `WLSL` | rsteg-wav      | 1     |
| `wav-lsb-permuted`     | `WLSP` | rsteg-wav      | 1     |
| `png-lsb-linear`       | `PLSL` | rsteg-png      | 1     |
| `png-lsb-permuted`     | `PLSP` | rsteg-png      | 1     |
| `jpeg-f5`              | `JF5A` | rsteg-jpeg     | 2     |
| `jpeg-dct-graph`       | `JDCG` | rsteg-jpeg     | 2 or 3 |
| `png-chunk-text`       | `PCTX` | rsteg-png      | 3     |

All phase-1 schemes use the same `PayloadHeader` framing. Detection across formats: read the first (or permuted-order) 256 bits, look for `b"RSTG"` magic. With passphrase → Confirmed via AEAD. Without passphrase → chi-square hint only (because permuted mode scatters the magic).

### Capacity quick-reference

Values are **raw** (pre-header). Usable = raw − 32 (plaintext) or raw − 32 − 58 = raw − 90 (AEAD).

| Format | Carrier example | d=1 raw capacity | d=1 usable AEAD |
|--------|-----------------|-------------------|------------------|
| BMP 24-bit 512×512     | 786,432 pixel bytes  | 98,304 bytes | 98,214 bytes |
| WAV 16-bit stereo 44.1 kHz 60s | 5.29M PCM values | 661,500 bytes | 661,410 bytes |
| WAV 8-bit mono 44.1 kHz 60s    | 2.65M PCM values | 330,750 bytes | 330,660 bytes |
| PNG RGBA 512×512 uniform-α     | 786,432 usable bytes | 98,304 bytes | 98,214 bytes |
| PNG RGBA 512×512 varying-α     | 1,048,576 usable bytes | 131,072 bytes | 130,982 bytes |

A capacity test in spec 07 asserts each value in this table.
