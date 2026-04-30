#![no_main]

//! Fuzz target: arbitrary bytes -> `PngAdapter::extract`.
//!
//! Exercises the full PNG parse + defilter + extract pipeline. The decode
//! path runs against `miniz_oxide`, so this also fuzzes our use of that
//! crate's public surface.

use libfuzzer_sys::fuzz_target;
use rsteg_core::{ExtractOpts, FormatAdapter};
use rsteg_png::PNG_ADAPTER;

fuzz_target!(|data: &[u8]| {
    let opts = ExtractOpts::default();
    let _ = PNG_ADAPTER.extract(data, &opts);
});
