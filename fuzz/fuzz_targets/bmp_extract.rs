#![no_main]

//! Fuzz target: arbitrary bytes -> `BmpAdapter::extract`.
//!
//! Per spec 07 §Fuzzing, the only acceptable outcomes are `Ok(_)` (a valid
//! decode) or `Err(_)` (any typed error). Panics, unreachables, and OOMs
//! count as bugs — libfuzzer flags them automatically.

use libfuzzer_sys::fuzz_target;
use rsteg_bmp::BMP_ADAPTER;
use rsteg_core::{ExtractOpts, FormatAdapter};

fuzz_target!(|data: &[u8]| {
    let opts = ExtractOpts::default();
    let _ = BMP_ADAPTER.extract(data, &opts);
});
