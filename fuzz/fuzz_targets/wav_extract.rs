#![no_main]

//! Fuzz target: arbitrary bytes -> `WavAdapter::extract`.

use libfuzzer_sys::fuzz_target;
use rsteg_core::{ExtractOpts, FormatAdapter};
use rsteg_wav::WAV_ADAPTER;

fuzz_target!(|data: &[u8]| {
    let opts = ExtractOpts::default();
    let _ = WAV_ADAPTER.extract(data, &opts);
});
