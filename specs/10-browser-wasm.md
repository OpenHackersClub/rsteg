## Browser (WASM) target

Runs `rsteg` embed/extract entirely in the browser as a `wasm32-unknown-unknown` module. The CLI is not part of this target. Supersedes the "Web/WASM targets" deferred entry in [`specs/09-roadmap.md`](09-roadmap.md); roadmap section is updated in the same PR as the first implementation commit.

### Goals

1. **In-browser parity with the library.** PNG / BMP / WAV embed + extract, XChaCha20-Poly1305 AEAD with Argon2id KDF, steghide-compat *read* for BMP / WAV. Same bytes in, same bytes out as the native library — cross-validated by a shared fixture corpus.
2. **Zero trusted-server dependency.** Payloads, passphrases, and cover media never leave the user's machine. This is the core reason to ship a WASM build at all.
3. **Supply-chain minimization unchanged.** The WASM build reuses the existing crate graph. No bundler plugin registry, no npm-lockfile blowup, no framework coupling.
4. **No proc-macro deps.** The workspace-wide proc-macro ban (spec 02 rule 3) still applies. JS interop is hand-rolled `extern "C"` over `wasm32-unknown-unknown` — not `wasm-bindgen`.

### Non-goals

- **Cloudflare Workers, Node.js, Deno.** A separate spec covers server-side WASM if/when demand appears. The `rsteg-wasm` façade is designed to be reusable there, but this spec only commits to browsers.
- **Streaming APIs.** Cover files are read fully into memory JS-side, passed to WASM as a single `Uint8Array`. Matches the library's `&[u8]` in / `Vec<u8>` out shape.
- **Web Worker auto-spawning.** The crate exposes synchronous functions. Running them off the main thread is the caller's responsibility — we document it but don't ship a worker wrapper.
- **Framework integrations.** No React / Vue / Svelte glue. A plain ES module is the deliverable.
- **Cover-file preview / diffing UI.** Out of scope — that's the `public/` demo's job, not the library's.
- **Paletted BMP, JPEG.** Inherited from spec 00. JPEG arrives when phase 2 lands and this spec gets an amendment.

### Crate layout

One new crate, no changes to existing crates:

```
crates/
  rsteg-wasm/        # feature gate: wasm
    src/lib.rs       # extern "C" entrypoints + a tiny alloc shim
    Cargo.toml       # cdylib, deps = workspace adapters (feature-gated)
    README.md        # JS glue example (hand-written, ~50 lines)
    pkg/             # built artifact: rsteg.wasm + rsteg.js (generated, gitignored)
```

`rsteg-wasm` aggregates the same adapters `rsteg-cli` does, through the same Cargo features:

```toml
# crates/rsteg-wasm/Cargo.toml (sketch)
[lib]
crate-type = ["cdylib"]

[features]
default = ["png", "bmp", "wav", "crypto", "compat-steghide"]
png             = ["rsteg-png"]
bmp             = ["rsteg-bmp"]
wav             = ["rsteg-wav"]
crypto          = ["rsteg-crypto/aead", "getrandom/js"]
compat-steghide = ["rsteg-crypto/compat-steghide"]

[dependencies]
rsteg-core   = { path = "../rsteg-core" }
rsteg-png    = { path = "../rsteg-png",    optional = true }
rsteg-bmp    = { path = "../rsteg-bmp",    optional = true }
rsteg-wav    = { path = "../rsteg-wav",    optional = true }
rsteg-crypto = { path = "../rsteg-crypto", optional = true }
getrandom    = { version = "0.2", optional = true, default-features = false }
```

No crate depends on `rsteg-wasm`. It is a leaf, like `rsteg-cli`.

### ABI — hand-rolled, no `wasm-bindgen`

Four `extern "C"` entrypoints cover the surface. All take / return pointer + length pairs into linear memory; JS drives allocation through two exported shim functions.

```rust
// Allocator shims — caller allocates input buffers in WASM memory, then
// hands us the pointer. Output buffers are allocated by Rust and freed
// by a follow-up call from JS.
#[no_mangle] pub extern "C" fn rsteg_alloc(len: usize) -> *mut u8;
#[no_mangle] pub extern "C" fn rsteg_free(ptr: *mut u8, len: usize);

// Capacity — read-only probe, returns a packed u64 (bytes_capacity).
#[no_mangle] pub extern "C" fn rsteg_capacity(
    cover_ptr: *const u8, cover_len: usize,
    format_fourcc: u32,
) -> u64;

// Embed / extract — return an opaque handle encoding (ptr, len, err_code)
// into a single u64 (upper 32 = len-or-error, lower 32 = ptr). JS reads it,
// copies the bytes out, then calls rsteg_free.
#[no_mangle] pub extern "C" fn rsteg_embed(
    cover_ptr: *const u8, cover_len: usize,
    payload_ptr: *const u8, payload_len: usize,
    password_ptr: *const u8, password_len: usize,   // may be (null, 0)
    format_fourcc: u32, density: u8,
) -> u64;

#[no_mangle] pub extern "C" fn rsteg_extract(
    stego_ptr: *const u8, stego_len: usize,
    password_ptr: *const u8, password_len: usize,
    format_fourcc: u32,
) -> u64;
```

Error codes reuse `rsteg_core::Error` discriminants, surfaced through the upper 32 bits when the low 32 are `0`. A `rsteg_last_error_message(buf, cap) -> usize` provides a diagnostic string when the caller wants it — rare path, not on the hot loop.

The JS wrapper (`pkg/rsteg.js`) is ~50 lines of hand-written ES module code: `WebAssembly.instantiateStreaming`, two helpers (`toWasm(u8)` / `fromWasm(handle)`), and one async init. No bundler step is required. Shipped as a single `.wasm` + `.js` pair.

**Why not `wasm-bindgen`.** `#[wasm_bindgen]` is a proc macro and would violate spec 02 rule 3. It also pulls ~60 transitive crates on default features, blowing the budget. The hand-rolled ABI above is ~80 LOC Rust plus ~50 LOC JS — well within the "priced in" cost the dep policy accepts.

### Randomness

`getrandom` with the `js` feature targets `wasm32-unknown-unknown` via `crypto.getRandomValues`. This is the only dep change: adding `"js"` to the `getrandom` feature list when `rsteg-wasm/crypto` is on. Already allowlisted in spec 02; no new crate enters the graph.

`getrandom` 0.2 and 0.3 both support the browser, with different feature-flag names (`js` vs `wasm_js`). Pinning is spec 02's job; whichever major is current at implementation time is fine, as long as the lockfile shows one `getrandom`.

### Argon2id in the browser

Budget: default **t=3, m=65536, p=1** (spec 06). A conservative benchmark on mid-range 2024 laptops is **300–800 ms**. Acceptable with a progress indicator; unacceptable on the main thread for a UI that also needs to stay responsive.

Guidance we publish (in `rsteg-wasm/README.md`, not changes to spec 06):
- Call `rsteg_embed` / `rsteg_extract` from a Web Worker for anything above trivial payloads.
- Keep the default cost parameters. Lowering them for "mobile performance" weakens the KDF and is out of scope — users who want speed can skip the passphrase and accept linear-mode embedding, same tradeoff as the CLI.
- PBKDF2 is not an option here. spec 06 rejected it for native; same answer for WASM.

### Bundle size

Target — all default features, release build, `wasm-opt -Oz`, brotli-compressed:

| Component | Contribution |
|-----------|--------------|
| `rsteg-core` (bits, header, crc, prng, splitmix)        | ~20 KB |
| `rsteg-bmp` + `rsteg-wav`                                | ~10 KB |
| `rsteg-png` + `miniz_oxide`                              | ~80 KB |
| `rsteg-crypto` (chacha20poly1305 + argon2 + sha2)        | ~120 KB |
| `rsteg-crypto::compat` (aes + cbc + md-5)                | ~40 KB |
| Hand-rolled ABI + alloc shim                             | ~3 KB  |
| **Total (brotli-compressed)**                            | **~200–260 KB** |

CI gate: `ls -la rsteg-wasm/pkg/rsteg.wasm` after `wasm-opt -Oz` must be ≤ **500 KB uncompressed, ≤ 260 KB brotli-compressed**. Failing the budget blocks merge. Identical mechanism to the `cargo tree` count budget in spec 02.

### Build commands

```sh
# One-shot build of the default-feature bundle.
cargo build -p rsteg-wasm --target wasm32-unknown-unknown --release

# Strip + optimize. wasm-opt comes from binaryen; installed in CI, optional locally.
wasm-opt -Oz -o crates/rsteg-wasm/pkg/rsteg.wasm \
  target/wasm32-unknown-unknown/release/rsteg_wasm.wasm

# Minimal PNG-only bundle — mirrors the "≤ 2 transitive deps" claim from spec 02.
cargo build -p rsteg-wasm --target wasm32-unknown-unknown --release \
  --no-default-features --features png
```

No `wasm-pack`, no `trunk`, no `wasm-bindgen-cli`. A single `cargo build` + `wasm-opt` shell step.

### Testing

The WASM build cannot regress the library's semantics. Tests go in three places:

1. **Shared fixtures.** The existing `corpus/` directory is the source of truth. The WASM test runner loads the same BMP / WAV / PNG fixtures and asserts byte-for-byte identical output vs the native library reference hashes recorded in `corpus/hashes.json`. This is how we prove parity.
2. **Browser smoke — headless.** `tests/wasm-smoke/` runs under [`wasmtime`](https://wasmtime.dev/) for the pure-compute paths (no `crypto.getRandomValues`, so encryption smoke stays native). Avoids pulling a browser harness into CI just for CI hello-world.
3. **Browser smoke — real browser.** A separate nightly CI job loads the artifact into headless Chromium via Playwright (already used by `public/`), runs the landing-page Munch demo end-to-end (see below), plus 4 other canonical fixtures, and asserts hashes. Not on every PR — the native test suite plus the wasmtime smoke are the fast gate.

`cargo test -p rsteg-wasm` on the host still compiles — the crate is a `cdylib`, but the same lib can be tested as a host rlib under `#[cfg(test)]`. Fixture equivalence tests run there.

### Live demo — upgrade the existing Munch example

The landing page already ships a "Scream hidden in Starry Night" demo (see `public/index.html` section `#stego-demo`), but today it's **static**: three pre-baked artifacts served from `/sample/`:

- `/sample/munch_starry.png` — 1.96 MB PNG cover (Starry Night).
- `/sample/munch_starry_stego.png` — 2.07 MB stego output, payload already embedded.
- `/sample/munch_scream.jpg` — 321 KB extracted payload (Scream), shown inside `<details>`.

The WASM milestone **replaces the static reveal with a live extract**. No new demo, no new fixtures, no new copy:

1. The page loads `/sample/munch_starry_stego.png` into a `Uint8Array` (same URL, same bytes).
2. On user click of the `<summary>` "Reveal the payload" toggle, the page prompts for the passphrase (the one `rsteg-bench` uses to produce the fixture — published in `sample/README.md`).
3. The existing ES-module init loads `/pkg/rsteg.wasm`, calls `rsteg_extract`, gets the JPG bytes back.
4. The extracted bytes are fed to a `Blob` + `URL.createObjectURL` and swapped into the same `<img class="payload-reveal">` the `<details>` block already contains.
5. The page verifies the extracted SHA-1 matches the `10570e48…` prefix already displayed in the copy (`index.html:254`). Mismatch → visible error, CI fails.

This proves four things at once to any visitor:

- The WASM build is real.
- Extract parity — the artifact that came out of the native CLI during page build comes out of the WASM runtime in-browser, byte-identical.
- Zero server involvement — the fixture is a static asset, the crypto happens client-side.
- The supply-chain story holds up where it's hardest — in a browser, with no bundler.

An **embed** path on the same demo (cover + payload + passphrase → new stego PNG) is an optional extension; skipped if it blows the 260 KB bundle gate or the page-load budget. Extract alone is the phase-2 exit criterion because it exercises every crate on the hot path (`rsteg-core` framing, `rsteg-png`, `rsteg-crypto` AEAD + KDF) without needing `crypto.getRandomValues` at all — the RNG path is reserved for the optional embed upgrade.

The `<pre class="demo-cmd">` block at `index.html:258` stays as-is: it documents the CLI invocation that produced the stego fixture, and the WASM demo matches its result. No copy change needed there.

### Security notes (for `SECURITY.md` at phase 1.5)

- **Passphrase in the browser.** Read from an `<input type="password">` by the app; passed into WASM as bytes; zeroized inside Rust via `zeroize` (same as native). After the WASM call returns, the JS-side string is still recoverable from memory until GC — we document this and recommend clearing the input field. Not a regression vs any other in-browser crypto tool.
- **Side-channels.** `constant_time_eq` + `zeroize` paths still work under WASM. No new side-channel surface; JIT timing attacks on WASM are out of our threat model.
- **Subresource integrity.** The `.wasm` and `.js` artifacts are published with `integrity=` SRI hashes in the demo page. If the library is consumed externally, users are expected to pin the hash.

### Roadmap placement

- Not phase 1. Phase 1 is native-library-and-CIL correctness.
- **Phase 2 candidate**, concurrent with JPEG work. Requires phase 1 to be stable (the WASM build consumes the same crates; no point targeting a moving library).
- Exit criteria:
  1. Parity corpus passes (all native fixtures byte-identical).
  2. Bundle size under the 260 KB brotli gate.
  3. Existing Munch demo on the landing page extracts `/sample/munch_scream.jpg` live from `/sample/munch_starry_stego.png` in the browser — SHA-1 matches the prefix already printed in the copy, CI asserts both bytes and hash.
  4. `cargo tree -p rsteg-wasm --target wasm32-unknown-unknown` adds zero new direct deps beyond the `getrandom/js` feature flip.

Update [`specs/09-roadmap.md`](09-roadmap.md) → "Web/WASM targets" row removed from "Deferred / likely-never", moved under phase 2 scope, in the same PR as the first `rsteg-wasm` commit.
