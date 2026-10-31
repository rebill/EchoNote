//! Live pipeline simulation in virtual time. Audio is fed in VAD-window ticks; every decode is
//! executed for real and its measured duration advances a single-worker clock, so latency and
//! occupancy reflect this machine without waiting in real time.

mod score;
mod sim;
mod stats;

use std::path::PathBuf;
use std::time::Instant;

use anyhow::Result;
use echonote_core::asr::{OfflineAsr, StreamingAsr};
use echonote_core::vad::{Vad, VadKind, VadSpec};
use serde::Serialize;

use crate::audio::{load_wav, seconds};
use crate::metrics::round3;
use crate::model_arg::{ModelArgs, default_streaming_dir, models_dir};
use crate::sys_metrics::{logical_cpus, rss_now_mb, usage};
use sim::{SR, Sim, Track};

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
    /// Minimum seconds between preview requests per track (raised adaptively).
    #[arg(long, default_value_t = 0.6)]
    preview_interval: f64,
    #[arg(long, value_enum, default_value = "silero")]
    vad: VadArg,
    #[arg(long, default_value_t = 0.5)]
    min_silence: f32,
    #[arg(long, default_value_t = 10.0)]
    max_speech: f32,
    /// Force a final once an utterance reaches this many seconds (0 disables).
    #[arg(long, default_value_t = 12.0)]
    hard_cap: f64,
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
    hard_cap_s: f64,
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
    /// Dual-track runs without refs: per mic final echo features for manual inspection.
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
    for (name, mut samples) in inputs {
        samples.resize(total, 0.0);
        tracks.push(Track::new(name, samples, Vad::create(&vad_spec)?));
    }
    let load_s = load_started.elapsed().as_secs_f64();
    let rss_after_load_mb = rss_now_mb();

    // sherpa-onnx starts a detected segment two windows plus min_speech before detection.
    let preroll = 2 * window + (vad_spec.min_speech_s as f64 * SR) as usize;
    let mut sim = Sim::new(
        &asr,
        streaming.as_ref(),
        args.preview,
        args.preview_interval,
        preroll,
        vad_spec.min_speech_s as f64,
        args.hard_cap,
    );
    let cpu_before = usage().cpu_s;
    let wall_started = Instant::now();
    let mut tick_end = window;
    while tick_end <= total {
        sim.tick(&mut tracks, tick_end, window);
        tick_end += window;
    }
    sim.finish(&mut tracks, total);
    let wall_s = wall_started.elapsed().as_secs_f64();
    let cpu_s = usage().cpu_s - cpu_before;

    let audio_s = seconds(total);
    let refs = args
        .refs
        .as_deref()
        .map(score::load_refs)
        .transpose()?
        .unwrap_or_default();
    let names: Vec<&str> = tracks.iter().map(|track| track.name).collect();
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
    let mic_features = match (&tracks[..], refs.is_empty()) {
        ([mic, sys], true) => score::mic_features(&sim.finals, mic, sys, args.cn_numerals),
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
        hard_cap_s: args.hard_cap,
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
