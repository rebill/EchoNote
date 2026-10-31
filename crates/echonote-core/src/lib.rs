//! EchoNote engine: audio pipeline, VAD, ASR scheduling, storage and LLM integration.
//!
//! This crate has no dependency on Tauri so every component can be unit tested and
//! benchmarked headless.

pub mod asr;
pub mod capture;
pub mod vad;

/// Sample rate used by every model and by the internal audio timeline.
pub const SAMPLE_RATE: u32 = 16_000;

/// Convert a sample count on the internal timeline to milliseconds.
pub fn samples_to_ms(samples: u64) -> u64 {
    samples * 1_000 / SAMPLE_RATE as u64
}
