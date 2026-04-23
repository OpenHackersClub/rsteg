//! WAV (RIFF/PCM) format adapter for rsteg.
//!
//! V1 scope (per `specs/05-formats.md`):
//! - RIFF/WAVE, `fmt ` format code 1 (PCM).
//! - 16-bit signed LE, or 8-bit unsigned.
//! - 1 or 2 channels, 8 kHz–192 kHz.
//!
//! Embedding unit = one PCM value (spec terminology — *per channel*, not "frame").
//! A 16-bit stereo second at 44.1 kHz is 88,200 embedding units.
