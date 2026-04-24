## Browser (WASM) target

Runs `rsteg` embed/extract entirely in the browser as a `wasm32-unknown-unknown` module. The CLI is not part of this target. Supersedes the "Web/WASM targets" deferred entry in [`specs/09-roadmap.md`](09-roadmap.md); roadmap section is updated in the same PR as the first implementation commit.

### Goals

1. **In-browser parity with the library.** PNG / BMP / WAV embed + extract, XChaCha20-Poly1305 AEAD with Argon2id KDF, steghide-compat *read* for BMP / WAV. Same bytes in, same bytes out as the native library — cross-validated by a shared fixture corpus.
2. **Zero trusted-server dependency.** Payloads, passphrases, and cover media never leave the user's machine. This is the core reason to ship a WASM build at all.
3. **Supply-chain minimization unchanged.** The WASM build reuses the existing crate graph. No bundler plugin registry, no npm-lockfile blowup, no framework coupling.
4. **No proc-macro deps.** The workspace-wide proc-macro ban (spec 02 rule 3) still applies. JS interop is hand-rolled `extern "C"` over `wasm32-unknown-unknown` — **not** `wasm-bindgen`, and **not** `getrandom`'s `js` / `wasm_js` features (both pull `wasm-bindgen` transitively). OS entropy on wasm32 comes through a `register_custom_getrandom!` (`macro_rules!`, not proc-macro) backend that forwards to a JS-provided `rsteg_fill_random(ptr, len)` import.

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

`rsteg-wasm` aggregates the same adapters `rsteg-cli` does. The real crate graph today is `rsteg-crypto-aead` (single crate, no feature-split into `aead` + `compat-steghide` yet — see spec 01 §"Workspace layout" note). `compat-steghide` arrives when `rsteg-compat-steghide` lands; the `compat` feature below is reserved but unimplemented.

```toml
# crates/rsteg-wasm/Cargo.toml (sketch)
[lib]
crate-type = ["cdylib", "rlib"]   # rlib so host-side tests can link the ABI shim

[features]
default = ["png", "bmp", "wav", "crypto"]
png     = ["rsteg-png"]
bmp     = ["rsteg-bmp"]
wav     = ["rsteg-wav"]
crypto  = ["rsteg-crypto-aead"]
# compat = ["rsteg-compat-steghide"]  # reserved — lands with the compat crate

[dependencies]
rsteg-core        = { path = "../rsteg-core" }
rsteg-png         = { path = "../rsteg-png",         optional = true }
rsteg-bmp         = { path = "../rsteg-bmp",         optional = true }
rsteg-wav         = { path = "../rsteg-wav",         optional = true }
rsteg-crypto-aead = { path = "../rsteg-crypto-aead", optional = true }

[profile.release]
panic = "abort"   # required — unwinding across extern "C" to wasm32 is UB
lto   = true
opt-level = "z"
```

No crate depends on `rsteg-wasm`. It is a leaf, like `rsteg-cli`.

### ABI — hand-rolled, no `wasm-bindgen`

**Target gate.** `rsteg-wasm` is wasm32-only for shipping. The entrypoints are `#[cfg(target_arch = "wasm32")]`; on any other target the crate exposes a parallel host-side wrapper (same logic, safe Rust, no `#[no_mangle]`) so `cargo test -p rsteg-wasm` runs on the developer's machine. A `compile_error!` catches anyone who tries to ship the `cdylib` to a non-wasm32 triple.

**Authorized unsafe.** The alloc shim and the `extern "C"` bodies need `#[allow(unsafe_code)]` at module level. Per spec 02 rule 5, `crates/rsteg-wasm/src/ffi.rs` is added to the authorized-unsafe list, every unsafe block carries a `// SAFETY:` comment, and the module is fuzzed alongside the format adapters.

**Input cap.** A hard `MAX_CARRIER_BYTES = 256 MiB` check at each entrypoint returns a dedicated error code (`ERR_INPUT_TOO_LARGE`) before any allocation. Prevents a `memory.grow` failure deep in `Vec::reserve` → panic → abort.

**Error ABI.** A stable `#[repr(u32)]` `WasmError` enum in `rsteg-wasm` — decoupled from `rsteg_core::Error` so adding a new core variant does not shift wire-format error codes. Return shape: `u64` with the high bit (`1 << 63`) as the success/error sentinel.

- Success: high bit `0`. Lower 32 = pointer (may be `0` for empty output). Bits 32..63 = length.
- Error: high bit `1`. Lower 32 = `WasmError` discriminant. Bits 32..63 = reserved (0).

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

Error codes are the stable `WasmError` `#[repr(u32)]` enum, not `rsteg_core::Error` discriminants. `rsteg-wasm::translate_err(&rsteg_core::Error) -> WasmError` does the mapping (lossy by design — many core variants collapse into one wire code, e.g. every AEAD failure lands on `ERR_BAD_PASSPHRASE` per spec 06's indistinguishability requirement).

**Ownership contract.**

- Input buffers (`cover`, `payload`, `password`): allocated by JS via `rsteg_alloc`, freed by JS via `rsteg_free` after the call returns. Password buffer is zeroized by Rust before any return (success or error) — JS cannot zeroize on its own.
- Output buffer: allocated by Rust (`Vec<u8>::into_raw_parts`), freed by JS via `rsteg_free` after copying the bytes out.
- On panic: the `panic = "abort"` profile turns any panic into a WASM trap. JS observes the trap and must treat every outstanding buffer as leaked — the WASM instance is dead anyway.

`rsteg_last_error_message(buf, cap) -> usize` provides a diagnostic string when the caller wants it — rare path, not on the hot loop.

The JS wrapper (`pkg/rsteg.js`) is ~50 lines of hand-written ES module code: `WebAssembly.instantiateStreaming`, two helpers (`toWasm(u8)` / `fromWasm(handle)`), and one async init. No bundler step is required. Shipped as a single `.wasm` + `.js` pair.

**Why not `wasm-bindgen`.** `#[wasm_bindgen]` is a proc macro and would violate spec 02 rule 3. It also pulls ~60 transitive crates on default features, blowing the budget. The hand-rolled ABI above is ~80 LOC Rust plus ~50 LOC JS — well within the "priced in" cost the dep policy accepts.

### Randomness

`getrandom`'s `js` (0.2) and `wasm_js` (0.3) features both depend on `wasm-bindgen`, which is a proc-macro crate — **not** acceptable. Instead we register a custom backend using `register_custom_getrandom!` (a `macro_rules!` macro, permitted):

```rust
// crates/rsteg-wasm/src/rng.rs  (sketch, wasm32 only)
extern "C" {
    // Imported from JS — the host provides a Uint8Array filled from
    // crypto.getRandomValues and writes it into the given pointer.
    fn rsteg_fill_random(ptr: *mut u8, len: usize) -> i32;
}

fn browser_getrandom(buf: &mut [u8]) -> Result<(), getrandom::Error> {
    // SAFETY: buf is a valid writable slice; JS honors (ptr, len) exactly.
    let rc = unsafe { rsteg_fill_random(buf.as_mut_ptr(), buf.len()) };
    if rc == 0 { Ok(()) } else { Err(getrandom::Error::UNEXPECTED) }
}

getrandom::register_custom_getrandom!(browser_getrandom);
```

The JS glue provides `rsteg_fill_random` by calling `crypto.getRandomValues` on a `Uint8Array` view into WASM memory. ~10 lines of JS. No `wasm-bindgen`, no `js-sys`, no transitive proc-macro blow-up.

`getrandom` itself stays allowlisted in spec 02 as a direct dep of `rsteg-crypto-aead`. No new crate, no new feature flag, no deviation from spec 02 rule 3.

### Argon2id in the browser

Budget: default **t=3, m=65536, p=1** (spec 06). A conservative benchmark on mid-range 2024 laptops is **300–800 ms**. Acceptable with a progress indicator; unacceptable on the main thread for a UI that also needs to stay responsive.

`m=65536` is 64 MiB of scratch memory. Safe on desktop browsers; marginal on iOS Safari (~1 GiB practical cap under memory pressure, silent tab kill if exceeded) and low-end Android. Mitigation:

- Call `rsteg_embed` / `rsteg_extract` from a Web Worker for anything above trivial payloads. The main thread stays responsive even if the Worker OOMs.
- Wrap the Argon2id call site so an allocation failure returns `ERR_KDF_MEMORY` (a dedicated `WasmError` variant) rather than trapping. Lets the demo page tell the user "this device doesn't have enough memory to open this file" instead of a blank tab crash.
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

Table is a **target** for the implementation PR, not a proven fact. The budget gate reevaluates if first-cut measurements land above 320 KB brotli. dlmalloc is the default allocator (~10 KB); `wee_alloc` is explicitly not used (unmaintained, known memory-fragmentation issues).

CI gate: `ls -la rsteg-wasm/pkg/rsteg.wasm` after `wasm-opt -Oz` must be ≤ **500 KB uncompressed, ≤ 260 KB brotli-compressed**. Failing the budget blocks merge. Identical mechanism to the `cargo tree` count budget in spec 02. Pin a minimum `wasm-opt` version in CI (binaryen ≥ 119) so upstream regressions don't land silently.

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

The landing page already ships a "Scream hidden in Starry Night" demo (see `public/index.html` section `#stego-demo`), but today it's **static**: three pre-baked artifacts live on disk at `public/sample/` and are served from `/sample/`:

- `/sample/munch_starry.png` — 1.96 MB PNG cover (Starry Night).
- `/sample/munch_starry_stego.png` — 2.07 MB stego output, payload already embedded.
- `/sample/munch_scream.jpg` — 321 KB extracted payload (Scream), shown inside `<details>`.

The WASM milestone **replaces the static reveal with a live extract**. No new demo, no new fixtures, no new copy. The *exit criterion* for the milestone lives in `tests/wasm-smoke/` (headless Chromium via Playwright), not in `public/index.html` copy — so refactors of the landing page do not break the WASM crate's CI gate. The landing-page upgrade lands as a companion PR once the crate ships.

Flow once both PRs land:

1. The page loads `/sample/munch_starry_stego.png` into a `Uint8Array` (same URL, same bytes).
2. On user click of the `<summary>` "Reveal the payload" toggle, the page prompts for the passphrase. (The passphrase is documented in the PR that bakes the fixtures — not referenced here as a promise; this spec does not require a specific docs location.)
3. The page loads `/pkg/rsteg.wasm`, calls `rsteg_extract`, gets the JPG bytes back.
4. The extracted bytes are fed to a `Blob` + `URL.createObjectURL` and swapped into the same `<img class="payload-reveal">` the `<details>` block already contains.
5. The page verifies the extracted SHA-1 matches the `10570e48…` prefix already displayed in the copy. Mismatch → visible error.

This proves four things at once to any visitor:

- The WASM build is real.
- Extract parity — the artifact that came out of the native CLI during page build comes out of the WASM runtime in-browser, byte-identical.
- Zero server involvement — the fixture is a static asset, the crypto happens client-side.
- The supply-chain story holds up where it's hardest — in a browser, with no bundler.

An **embed** path on the same demo (cover + payload + passphrase → new stego PNG) is an optional extension; skipped if it blows the 260 KB bundle gate or the page-load budget. Extract alone exercises every crate on the hot path (`rsteg-core` framing, `rsteg-png`, `rsteg-crypto-aead` AEAD + KDF) without needing `crypto.getRandomValues` at all — the custom-RNG backend above is only on the embed path.

### Security notes (for `SECURITY.md` at phase 1.5)

- **Passphrase in the browser.** Delivered to WASM as a `Uint8Array` allocated via `rsteg_alloc`, populated byte-by-byte from the input event, zeroized inside Rust via `zeroize` before any entrypoint return. Strongly recommended (documented in `rsteg-wasm/README.md`) to run the call inside a dedicated Web Worker so the main thread's V8/JSC heap never sees the bytes. JS strings immutably retain until GC — passing the passphrase as a `Uint8Array` and clearing it explicitly is the mitigation.
- **Side-channels.** `constant_time_eq` + `zeroize` paths still work under WASM, but WASM does **not** guarantee constant-time execution — V8/SpiderMonkey tier-up compilers can introduce data-dependent branches via inline caches. We preserve the algorithmic property; wall-clock constant-time is best-effort. Timing-attack resistance at the browser-timing-API level is out of our threat model. The spec 07 timing invariant test runs natively; a wasmtime-hosted variant runs nightly with a relaxed threshold.
- **CSP + Trusted Types.** Hosting guidance in `rsteg-wasm/README.md`: minimum CSP is `script-src 'self' 'wasm-unsafe-eval'; worker-src 'self'; connect-src 'self'` (Chrome requires `'wasm-unsafe-eval'` for `WebAssembly.instantiate*`). The hand-rolled JS glue does no `eval` / `innerHTML` / `document.write`, so it is Trusted-Types-compatible without additional work.
- **Subresource integrity.** The `.wasm` and `.js` artifacts are published with `integrity=` SRI hashes on the `<script>` tag. `<link rel="modulepreload">` SRI enforcement for `.wasm` is inconsistent across 2026-vintage browsers; treat SRI on `.js` as the authoritative pin and let the JS glue assert on the WASM streaming instantiation result.

### Roadmap placement

- Not phase 1. Phase 1 is native-library-and-CIL correctness.
- **Phase 2 candidate**, concurrent with JPEG work. Requires phase 1 to be stable (the WASM build consumes the same crates; no point targeting a moving library).

Current implementation status (landed on `spec/browser-wasm`, PR #14):

- ✅ `rsteg-wasm` crate scaffolded with `cdylib + rlib`; FFI entrypoints exposed on every target so host tests can exercise them without a wasm toolchain.
- ✅ `rsteg_alloc` / `rsteg_free` / `rsteg_extract` / `rsteg_embed` with a `u32` status + out-parameter ABI — identical shape on wasm32 and 64-bit host.
- ✅ Custom `register_custom_getrandom!` backend forwarding to a JS-provided `rsteg_fill_random(ptr, len)` import; no `wasm-bindgen`, no `js-sys` in the wasm32 crate graph.
- ✅ wasm32 CI job with binaryen-pinned `wasm-opt -Oz`, size gate (≤ 260 KB brotli / ≤ 500 KB raw), and a "no wasm-bindgen / js-sys in the tree" assertion. First-cut measurement on the `release-wasm` profile: **~41 KB brotli, ~123 KB raw** without wasm-opt — well under the gate.
- ✅ Host-side FFI tests: 11 passing (6 extract, 5 embed).

Still outstanding for the phase-2 tag:

1. **Headless-browser smoke test** in `tests/wasm-smoke/` — extract the Munch fixture (`/sample/munch_starry_stego.png`) in a real browser and assert SHA-1 of the payload matches `10570e48…`. This exercises the JS-provided `rsteg_fill_random` import path that the host tests bypass.
2. **Landing-page demo upgrade** — wire `rsteg_extract` into the `<details>` reveal at `public/index.html#stego-demo`. Companion PR, decoupled from this crate's CI gate so `index.html` refactors don't break the WASM build.
3. **Differential fuzz target** at `fuzz/fuzz_targets/diff_native_vs_wasmtime.rs` — random inputs through native Rust vs the same crate compiled to wasm32 under `wasmtime`, outputs must match bit-for-bit.
4. **Roadmap update** — remove "Web/WASM targets" from `specs/09-roadmap.md`'s "Deferred / likely-never" list, move under phase 2 scope.
