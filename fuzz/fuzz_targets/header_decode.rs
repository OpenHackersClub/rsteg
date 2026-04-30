#![no_main]

//! Fuzz target: arbitrary bytes -> `PayloadHeader::decode`.
//!
//! Tiny surface but the highest-leverage fuzz target — every extract path
//! funnels through this decoder, so a panic here would propagate to every
//! adapter.

use libfuzzer_sys::fuzz_target;
use rsteg_core::PayloadHeader;

fuzz_target!(|data: &[u8]| {
    let _ = PayloadHeader::decode(data);
});
