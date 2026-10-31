use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::time::Instant;

use anyhow::{Context, Result};
use echonote_core::asr::OfflineAsr;
use serde::{Deserialize, Serialize};

use crate::audio::{load_wav, seconds};
use crate::metrics::{edit_distance, percentile, round3};
use crate::model_arg::ModelArgs;
use crate::sys_metrics::{logical_cpus, rss_now_mb, usage};
use crate::text::tokens;

#[derive(clap::Args)]
pub struct SmokeArgs {
    #[command(flatten)]
    pub model: ModelArgs,
    pub wav: PathBuf,
}

pub fn smoke(args: SmokeArgs) -> Result<()> {
    let spec = args.model.spec()?;
    let samples = load_wav(&args.wav)?;
    let load_started = Instant::now();
    let asr = OfflineAsr::load(&spec)?;
    let load_s = load_started.elapsed().as_secs_f64();
    let decode_started = Instant::now();
    let text = asr.decode(&samples);
    let decode_s = decode_started.elapsed().as_secs_f64();
    let audio_s = seconds(samples.len());
    println!("{text}");
    eprintln!(
        "model={} threads={} load={load_s:.3}s audio={audio_s:.2}s decode={decode_s:.3}s rtf={:.4} peak_rss={:.0}MB",
        spec.kind.name(),
        spec.num_threads,
        decode_s / audio_s,
        usage().peak_rss_mb
    );
    Ok(())
}

#[derive(clap::Args)]
pub struct EvalArgs {
    #[command(flatten)]
    pub model: ModelArgs,
    /// JSONL manifest with {"id", "audio", "text"}; audio paths are relative to the manifest.
    #[arg(long)]
    pub manifest: PathBuf,
    /// Only decode the first N utterances.
    #[arg(long)]
    pub limit: Option<usize>,
    /// Rewrite Arabic digits as Chinese numerals before scoring.
    #[arg(long)]
    pub cn_numerals: bool,
    /// JSON map used to fold Traditional characters to Simplified before scoring.
    #[arg(long)]
    pub t2s: Option<PathBuf>,
    /// Write per-utterance hypotheses as JSONL.
    #[arg(long)]
    pub hyp_out: Option<PathBuf>,
    /// Write the summary report as JSON.
    #[arg(long)]
    pub report: Option<PathBuf>,
}

#[derive(Deserialize)]
struct Utterance {
    id: String,
    audio: PathBuf,
    text: String,
}

#[derive(Serialize)]
struct Hypothesis<'a> {
    id: &'a str,
    reference: &'a str,
    hypothesis: &'a str,
    errors: usize,
    ref_tokens: usize,
    decode_ms: f64,
}

#[derive(Serialize)]
pub struct EvalReport {
    pub model: String,
    pub threads: i32,
    pub manifest: String,
    pub utterances: usize,
    pub audio_s: f64,
    pub ref_tokens: usize,
    pub errors: usize,
    pub error_rate: f64,
    pub load_s: f64,
    pub decode_s: f64,
    pub rtf: f64,
    pub decode_ms_p50: f64,
    pub decode_ms_p95: f64,
    pub cpu_s_per_audio_s: f64,
    pub machine_cpu_share: f64,
    pub peak_rss_mb: f64,
    pub rss_after_load_mb: f64,
    pub rss_end_mb: f64,
    pub logical_cpus: usize,
}

pub fn run(args: EvalArgs) -> Result<()> {
    let spec = args.model.spec()?;
    if let Some(path) = &args.t2s {
        crate::text::enable_t2s(path)?;
    }
    let base = args
        .manifest
        .parent()
        .map(PathBuf::from)
        .unwrap_or_default();
    let reader = BufReader::new(
        File::open(&args.manifest).with_context(|| format!("open {}", args.manifest.display()))?,
    );
    let mut utterances = Vec::new();
    for line in reader.lines() {
        let line = line?;
        if !line.trim().is_empty() {
            utterances.push(serde_json::from_str::<Utterance>(&line)?);
        }
    }
    if let Some(limit) = args.limit {
        utterances.truncate(limit);
    }

    let load_started = Instant::now();
    let asr = OfflineAsr::load(&spec)?;
    let load_s = load_started.elapsed().as_secs_f64();
    let rss_after_load_mb = rss_now_mb();
    let cpu_before = usage().cpu_s;

    let mut hyp_out = args.hyp_out.as_ref().map(File::create).transpose()?;
    let (mut audio_s, mut decode_s, mut errors, mut ref_tokens) = (0.0, 0.0, 0usize, 0usize);
    let mut decode_ms = Vec::with_capacity(utterances.len());
    for (index, utterance) in utterances.iter().enumerate() {
        let samples = load_wav(&base.join(&utterance.audio))?;
        let started = Instant::now();
        let hypothesis = asr.decode(&samples);
        let elapsed = started.elapsed().as_secs_f64();
        let reference_tokens = tokens(&utterance.text, args.cn_numerals);
        let hypothesis_tokens = tokens(&hypothesis, args.cn_numerals);
        let utterance_errors = edit_distance(&reference_tokens, &hypothesis_tokens);

        audio_s += seconds(samples.len());
        decode_s += elapsed;
        errors += utterance_errors;
        ref_tokens += reference_tokens.len();
        decode_ms.push(elapsed * 1000.0);
        if let Some(out) = hyp_out.as_mut() {
            let record = Hypothesis {
                id: &utterance.id,
                reference: &utterance.text,
                hypothesis: &hypothesis,
                errors: utterance_errors,
                ref_tokens: reference_tokens.len(),
                decode_ms: round3(elapsed * 1000.0),
            };
            writeln!(out, "{}", serde_json::to_string(&record)?)?;
        }
        if (index + 1) % 100 == 0 {
            eprintln!(
                "  {}/{} error_rate={:.4}",
                index + 1,
                utterances.len(),
                errors as f64 / ref_tokens.max(1) as f64
            );
        }
    }

    let cpu_s = usage().cpu_s - cpu_before;
    let report = EvalReport {
        model: spec.kind.name().into(),
        threads: spec.num_threads,
        manifest: args.manifest.display().to_string(),
        utterances: utterances.len(),
        audio_s: round3(audio_s),
        ref_tokens,
        errors,
        error_rate: round3(errors as f64 / ref_tokens.max(1) as f64 * 100.0) / 100.0,
        load_s: round3(load_s),
        decode_s: round3(decode_s),
        rtf: round3(decode_s / audio_s * 1000.0) / 1000.0,
        decode_ms_p50: round3(percentile(&decode_ms, 0.5).unwrap_or(0.0)),
        decode_ms_p95: round3(percentile(&decode_ms, 0.95).unwrap_or(0.0)),
        cpu_s_per_audio_s: round3(cpu_s / audio_s * 1000.0) / 1000.0,
        machine_cpu_share: round3(cpu_s / audio_s / logical_cpus() as f64 * 1000.0) / 1000.0,
        peak_rss_mb: round3(usage().peak_rss_mb),
        rss_after_load_mb: round3(rss_after_load_mb),
        rss_end_mb: round3(rss_now_mb()),
        logical_cpus: logical_cpus(),
    };
    let json = serde_json::to_string_pretty(&report)?;
    println!("{json}");
    if let Some(path) = args.report {
        std::fs::write(path, json + "\n")?;
    }
    Ok(())
}
