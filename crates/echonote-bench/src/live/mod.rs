//! Live pipeline simulation in virtual time. The app's `echonote_core::pipeline` is fed in
//! VAD-window ticks; every decode is executed for real and its measured duration advances a
//! single-worker clock, so latency and occupancy reflect this machine without waiting in real
//! time.

mod score;
mod sim;
mod stats;

use std::path::PathBuf;
use std::time::Instant;

use anyhow::{Result, bail};
use echonote_core::asr::{OfflineAsr, StreamingAsr};
use echonote_core::pipeline::{Pipeline, PipelineConfig, SherpaDetector, SpeechDetector};
use echonote_core::vad::{VadKind, VadSpec};
use serde::Serialize;

use crate::audio::{load_wav, seconds};
use crate::metrics::round3;
use crate::model_arg::{ModelArgs, default_streaming_dir, models_dir};
use crate::sys_metrics::{logical_cpus, rss_now_mb, usage};
use sim::{Sim, Track};

/// Per side of a forced cut, so the two finals share 0.4 s that token timestamps split.
const DEFAULT_OVERLAP_S: f64 = 0.2;

#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PreviewMode {
    /// Finals only.
    None,
    /// Re-decode the growing segment with the offline model (strategy S1).
    Redecode,
    /// Streaming zipformer previews plus offline finals (strategy S2).
    Streaming,
}

#[derive(clap::ValueEnum, Clone, Copy, Debug)]
enum VadArg {
    Silero,
    Ten,
}

#[derive(clap::Args)]
pub struct LiveArgs {
    #[command(flatten)]
    model: ModelArgs,
    /// Microphone track (labelled "mic").
    #[arg(long)]
    mic: PathBuf,
    /// Optional system-audio track (labelled "sys").
    #[arg(long)]
    sys: Option<PathBuf>,
    /// JSON array of {"track","start_ms","end_ms","text"} references.
    #[arg(long)]
    refs: Option<PathBuf>,
    #[arg(long, value_enum, default_value = "redecode")]
    preview: PreviewMode,
    /// Minimum seconds between previews, shared by all tracks (raised adaptively).
    #[arg(long, default_value_t = 0.6)]
    preview_interval: f64,
    #[arg(long, value_enum, default_value = "silero")]
    vad: VadArg,
    #[arg(long, default_value_t = 0.5)]
    min_silence: f32,
    /// Seconds of speech after which sherpa ends an utterance at a 0.1 s dip.
    #[arg(long, default_value_t = 10.0)]
    max_speech: f32,
    /// Force a final once an utterance reaches this many seconds (0 disables).
    #[arg(long, default_value_t = 12.0)]
    hard_cap: f64,
    /// Seconds each side of a forced cut decodes past it. Defaults to 0.2 for models with
    /// token timestamps and 0 otherwise.
    #[arg(long)]
    overlap: Option<f64>,
    #[arg(long, default_value_t = 1)]
    streaming_threads: i32,
    #[arg(long)]
    cn_numerals: bool,
    /// JSON map used to fold Traditional characters to Simplified before scoring.
    #[arg(long)]
    t2s: Option<PathBuf>,
    /// Write finals as JSONL.
    #[arg(long)]
    finals_out: Option<PathBuf>,
    #[arg(long)]
    report: Option<PathBuf>,
}

#[derive(Serialize)]
struct LiveReport {
    model: String,
    threads: i32,
    preview: PreviewMode,
    vad: String,
    max_speech_s: f32,
    hard_cap_s: f64,
    overlap_s: f64,
    tracks: usize,
    audio_s: f64,
    load_s: f64,
    wall_s: f64,
    #[serde(flatten)]
    stats: stats::StatsSummary,
    cpu_s_per_audio_s: f64,
    machine_cpu_share: f64,
    peak_rss_mb: f64,
    rss_after_load_mb: f64,
    rss_end_mb: f64,
    logical_cpus: usize,
    accuracy: Vec<score::TrackAccuracy>,
    dedup: Option<score::DedupReport>,
    /// Dual-track runs: per mic final echo features for manual inspection.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    mic_features: Vec<score::MicFeature>,
    invariants: score::Invariants,
}

pub fn run(args: LiveArgs) -> Result<()> {
    let spec = args.model.spec()?;
    if let Some(path) = &args.t2s {
        crate::text::enable_t2s(path)?;
    }
    let (kind, file) = match args.vad {
        VadArg::Silero => (VadKind::Silero, "silero_vad.onnx"),
        VadArg::Ten => (VadKind::Ten, "ten-vad.onnx"),
    };
    let mut vad_spec = VadSpec::meeting(kind, models_dir().join("vad").join(file));
    vad_spec.min_silence_s = args.min_silence;
    vad_spec.max_speech_s = args.max_speech;
    let overlap_s = match args.overlap {
        Some(overlap) if overlap > 0.0 && !spec.kind.has_token_timestamps() => {
            bail!("--overlap needs a model with token timestamps")
        }
        Some(overlap) => overlap,
        None if spec.kind.has_token_timestamps() => DEFAULT_OVERLAP_S,
        None => 0.0,
    };

    let mut inputs = vec![("mic", load_wav(&args.mic)?)];
    if let Some(sys) = &args.sys {
        inputs.push(("sys", load_wav(sys)?));
    }
    let window = vad_spec.window_size();
    let total = inputs
        .iter()
        .map(|(_, samples)| samples.len())
        .max()
        .unwrap_or(0)
        / window
        * window;

    let load_started = Instant::now();
    let asr = OfflineAsr::load(&spec)?;
    let streaming = match args.preview {
        PreviewMode::Streaming => Some(StreamingAsr::load_zipformer(
            &default_streaming_dir(),
            args.streaming_threads,
        )?),
        _ => None,
    };
    let mut tracks = Vec::new();
    let mut detectors: Vec<Box<dyn SpeechDetector>> = Vec::new();
    for (name, mut samples) in inputs {
        samples.resize(total, 0.0);
        tracks.push(Track { name, samples });
        detectors.push(Box::new(SherpaDetector::new(&vad_spec)?));
    }
    let load_s = load_started.elapsed().as_secs_f64();
    let rss_after_load_mb = rss_now_mb();

    let names: Vec<&'static str> = tracks.iter().map(|track| track.name).collect();
    let config = PipelineConfig {
        previews: args.preview == PreviewMode::Redecode,
        preview_interval_s: args.preview_interval,
        // Through speakers the mic mostly hears the other side again.
        priority_track: names.iter().position(|name| *name == "sys"),
        ..PipelineConfig::meeting(args.hard_cap, overlap_s)
    };
    let mut sim = Sim::new(
        Pipeline::new(config, detectors),
        &asr,
        streaming.as_ref(),
        args.preview,
        names.clone(),
        vad_spec.min_speech_s as f64,
    );
    let cpu_before = usage().cpu_s;
    let wall_started = Instant::now();
    let mut tick_end = window;
    while tick_end <= total {
        sim.tick(&tracks, tick_end, window);
        tick_end += window;
    }
    sim.finish(total);
    let wall_s = wall_started.elapsed().as_secs_f64();
    let cpu_s = usage().cpu_s - cpu_before;

    let audio_s = seconds(total);
    let refs = args
        .refs
        .as_deref()
        .map(score::load_refs)
        .transpose()?
        .unwrap_or_default();
    let accuracy = if refs.is_empty() {
        Vec::new()
    } else {
        names
            .iter()
            .map(|name| score::accuracy(&sim.finals, &refs, name, args.cn_numerals))
            .collect()
    };
    let dedup = match (&tracks[..], refs.is_empty()) {
        ([mic, sys], false) => Some(score::dedup(&sim.finals, &refs, mic, sys, args.cn_numerals)),
        _ => None,
    };
    let mic_features = match &tracks[..] {
        [mic, sys] => score::mic_features(&sim.finals, mic, sys, args.cn_numerals),
        _ => Vec::new(),
    };
    if let Some(path) = &args.finals_out {
        let lines: Vec<String> = sim
            .finals
            .iter()
            .map(serde_json::to_string)
            .collect::<Result<_, _>>()?;
        std::fs::write(path, lines.join("\n") + "\n")?;
    }
    let report = LiveReport {
        model: spec.kind.name().into(),
        threads: spec.num_threads,
        preview: args.preview,
        vad: format!("{:?}", args.vad).to_lowercase(),
        max_speech_s: args.max_speech,
        hard_cap_s: args.hard_cap,
        overlap_s,
        tracks: tracks.len(),
        audio_s: round3(audio_s),
        load_s: round3(load_s),
        wall_s: round3(wall_s),
        stats: sim.stats.summary(audio_s),
        cpu_s_per_audio_s: round3(cpu_s / audio_s * 1000.0) / 1000.0,
        machine_cpu_share: round3(cpu_s / audio_s / logical_cpus() as f64 * 1000.0) / 1000.0,
        peak_rss_mb: round3(usage().peak_rss_mb),
        rss_after_load_mb: round3(rss_after_load_mb),
        rss_end_mb: round3(rss_now_mb()),
        logical_cpus: logical_cpus(),
        accuracy,
        dedup,
        mic_features,
        invariants: score::check_invariants(&sim.finals, &names),
    };
    let json = serde_json::to_string_pretty(&report)?;
    println!("{json}");
    if let Some(path) = args.report {
        std::fs::write(path, json + "\n")?;
    }
    Ok(())
}
