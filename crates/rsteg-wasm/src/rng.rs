//! Custom `getrandom` backend for the wasm32 target.
//!
//! Spec 10 §"Randomness". We cannot use `getrandom`'s `js` / `wasm_js`
//! features because they pull `wasm-bindgen` transitively, violating the
//! proc-macro ban (spec 02 rule 3). Instead we register a custom backend
//! with `register_custom_getrandom!` (a `macro_rules!` macro, permitted)
//! that forwards to a JS-provided `rsteg_fill_random(ptr, len)` import.
//!
//! On host targets this module compiles to a no-op: `getrandom`'s built-in
//! OS backend (libc `getrandom(2)` / `CryptGenRandom` / `SecRandomCopyBytes`)
//! wins and the registered custom backend is never called. Keeping the
//! registration compile-in for every target simplifies feature gating and
//! lets host tests exercise the same code paths.

// The JS-import block and its unsafe call site are authorized here per
// spec 02 rule 5. Every unsafe block carries a `// SAFETY:` comment.
#![allow(unsafe_code)]

#[cfg(target_arch = "wasm32")]
extern "C" {
    /// JS-side entropy filler. The browser glue is expected to do:
    ///
    /// ```js
    /// rsteg_fill_random: (ptr, len) => {
    ///   try {
    ///     crypto.getRandomValues(new Uint8Array(wasm.memory.buffer, ptr, len));
    ///     return 0;
    ///   } catch { return 1; }
    /// }
    /// ```
    ///
    /// Returns `0` on success, non-zero on any failure — the exact code is
    /// opaque and currently not plumbed through to the user.
    fn rsteg_fill_random(ptr: *mut u8, len: usize) -> i32;
}

#[cfg(not(target_arch = "wasm32"))]
/// Host stand-in. Never called in practice — `getrandom`'s built-in OS
/// backend wins over custom backends on any target that has one. Marked
/// `unsafe` so the call site below compiles identically under both
/// configurations (the wasm32 branch declares this as an extern C fn,
/// which is always unsafe to invoke).
unsafe fn rsteg_fill_random(_ptr: *mut u8, _len: usize) -> i32 {
    1 // "unexpected" — if this is ever called on host it's a bug
}

fn custom_rng(buf: &mut [u8]) -> Result<(), getrandom::Error> {
    if buf.is_empty() {
        return Ok(());
    }
    // SAFETY: `buf` is a valid writable slice; we pass its exact length
    // to the foreign fn which fills bytes in-place and returns a status.
    // On wasm32 the host JS reads `ptr` / `len` as a view into linear
    // memory. On host the call is a stub that always fails (unused path).
    let rc = unsafe { rsteg_fill_random(buf.as_mut_ptr(), buf.len()) };
    if rc == 0 {
        Ok(())
    } else {
        Err(getrandom::Error::UNEXPECTED)
    }
}

// Register the custom backend. On targets with a built-in backend (linux,
// macos, windows, bsd, …) this is dead code — the built-in wins. On
// wasm32-unknown-unknown it's the only entropy source for the crate.
getrandom::register_custom_getrandom!(custom_rng);
