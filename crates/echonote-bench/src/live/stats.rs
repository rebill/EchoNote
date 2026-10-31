use serde::Serialize;

use crate::metrics::{percentile, round3};

#[derive(Default)]
pub struct Stats {
    pub vad_s: f64,
    /// Seconds of audio passed to the offline model, for the machine-independent work ratio.
    pub decoded_audio_s: f64,
    pub final_decode_s: f64,
    pub final_decodes: usize,
    pub preview_decode_s: f64,
    pub preview_decodes: usize,
    pub streaming_s: f64,
    pub preview_updates: usize,
    pub dropped_previews: usize,
    pub forced_splits: usize,
    pub max_queue: usize,
    pub drain_s: f64,
    pub final_latency_s: Vec<f64>,
    pub queue_wait_s: Vec<f64>,
    pub preview_lag_s: Vec<f64>,
    pub first_preview_s: Vec<f64>,
}

#[derive(Serialize)]
pub struct Distribution {
    pub count: usize,
    pub p50_ms: Option<f64>,
    pub p95_ms: Option<f64>,
    pub max_ms: Option<f64>,
}

fn distribution(values: &[f64]) -> Distribution {
    let ms = |quantile: f64| percentile(values, quantile).map(|value| round3(value * 1000.0));
    Distribution {
        count: values.len(),
        p50_ms: ms(0.5),
        p95_ms: ms(0.95),
        max_ms: ms(1.0),
    }
}

/// Machine-independent counters plus measured costs, normalized per second of audio.
#[derive(Serialize)]
pub struct StatsSummary {
    pub final_decodes: usize,
    pub preview_decodes: usize,
    pub preview_updates: usize,
    pub dropped_previews: usize,
    /// Finals emitted by the hard cap instead of a VAD pause.
    pub forced_splits: usize,
    pub decodes_per_audio_min: f64,
    /// Audio seconds decoded by the offline model per second of input (re-decode amplification).
    pub decoded_audio_per_audio_s: f64,
    pub max_final_queue: usize,
    /// Single-worker occupancy: offline decode wall time per second of audio.
    pub worker_busy_per_audio_s: f64,
    pub streaming_busy_per_audio_s: f64,
    pub vad_busy_per_audio_s: f64,
    pub drain_after_stop_ms: f64,
    pub final_latency: Distribution,
    pub final_queue_wait: Distribution,
    pub preview_lag: Distribution,
    pub first_preview_latency: Distribution,
}

impl Stats {
    pub fn summary(&self, audio_s: f64) -> StatsSummary {
        let per_audio = |value: f64| round3(value / audio_s * 1000.0) / 1000.0;
        StatsSummary {
            final_decodes: self.final_decodes,
            preview_decodes: self.preview_decodes,
            preview_updates: self.preview_updates,
            dropped_previews: self.dropped_previews,
            forced_splits: self.forced_splits,
            decodes_per_audio_min: round3(
                (self.final_decodes + self.preview_decodes) as f64 / audio_s * 60.0,
            ),
            decoded_audio_per_audio_s: per_audio(self.decoded_audio_s),
            max_final_queue: self.max_queue,
            worker_busy_per_audio_s: per_audio(self.final_decode_s + self.preview_decode_s),
            streaming_busy_per_audio_s: per_audio(self.streaming_s),
            vad_busy_per_audio_s: per_audio(self.vad_s),
            drain_after_stop_ms: round3(self.drain_s * 1000.0),
            final_latency: distribution(&self.final_latency_s),
            final_queue_wait: distribution(&self.queue_wait_s),
            preview_lag: distribution(&self.preview_lag_s),
            first_preview_latency: distribution(&self.first_preview_s),
        }
    }
}
