//! CI regression gates on frozen synthetic fixtures. Everything is deterministic: tone bursts
//! stand in for speech, the engine's fake model for ASR, an energy detector for the VAD, and
//! a fixed decode-cost model for the worker clock. Queue, latency and decode-work numbers are
//! therefore the same on every machine and gate the pipeline logic (segmentation, scheduling,
//! crosstalk) without models or evaluation audio.
//!
//! `--write-baseline` records the current values with a 10% margin; commit the file only when
//! a change is intended.

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use echonote_core::SAMPLE_RATE;
use echonote_core::crosstalk::{CrosstalkConfig, Reference, Verdict};
use echonote_core::engine::{DecoderFactory, FakeModel};
use echonote_core::pipeline::{EnergyDetector, Pipeline, PipelineConfig, SpeechDetector};
use serde::{Deserialize, Serialize};

use crate::live::score::check_invariants;
use crate::live::{Cost, FinalSegment, PreviewMode, Sim, Track};

const SR: f64 = SAMPLE_RATE as f64;
const WINDOW: usize = 320;
const OWN_HZ: f64 = 220.0;
const OTHER_HZ: f64 = 440.0;
/// A word: 0.32 s of tone, then 0.08 s of silence.
const WORD_S: f64 = 0.32;
const WORD_GAP_S: f64 = 0.08;

#[derive(clap::Args)]
pub struct GateArgs {
    /// Thresholds file.
    #[arg(long, default_value = "crates/echonote-bench/gates.json")]
    thresholds: PathBuf,
    /// Write the measured values to this JSON file.
    #[arg(long)]
    report: Option<PathBuf>,
    /// Replace the thresholds with the measured values plus margins instead of checking.
    #[arg(long)]
    write_baseline: bool,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
struct CostModel {
    base_s: f64,
    per_audio_s: f64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Limit {
    #[serde(skip_serializing_if = "Option::is_none")]
    min: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Thresholds {
    cost_model: CostModel,
    fixtures: BTreeMap<String, BTreeMap<String, Limit>>,
}

/// A track's script: words as `(start_s, hz, amplitude)`.
struct Script {
    seconds: f64,
    mic: Vec<(f64, f64, f32)>,
    sys: Vec<(f64, f64, f32)>,
}

fn sentence(start: f64, words: usize, hz: f64, amplitude: f32) -> Vec<(f64, f64, f32)> {
    (0..words)
        .map(|i| (start + i as f64 * (WORD_S + WORD_GAP_S), hz, amplitude))
        .collect()
}

/// Alternating turns over 3 minutes; the mic hears the speakers 10 dB down, 50 ms late, and
/// every third reply starts before the other side has finished (double talk).
fn meeting_dual() -> Script {
    let mut mic = Vec::new();
    let mut sys = Vec::new();
    let mut at = 1.0;
    let mut turn = 0;
    while at < 170.0 {
        let other = sentence(at, 12, OTHER_HZ, 0.5);
        let end = at + 12.0 * (WORD_S + WORD_GAP_S);
        mic.extend(other.iter().map(|&(t, hz, _)| (t + 0.05, hz, 0.16)));
        sys.extend(other);
        let reply = if turn % 3 == 2 { end - 1.2 } else { end + 1.0 };
        mic.extend(sentence(reply, 8, OWN_HZ, 0.3));
        at = reply + 8.0 * (WORD_S + WORD_GAP_S) + 1.0;
        turn += 1;
    }
    Script {
        seconds: 180.0,
        mic,
        sys,
    }
}

/// 90 s of speech without a pause long enough to end an utterance: hard-cap cuts.
fn continuous() -> Script {
    let words = ((88.0 - 1.0) / (WORD_S + 0.1)) as usize;
    let sys = (0..words)
        .map(|i| (1.0 + i as f64 * (WORD_S + 0.1), OTHER_HZ, 0.5))
        .collect();
    Script {
        seconds: 90.0,
        mic: Vec::new(),
        sys,
    }
}

/// 20 minutes, one short sentence a minute: memory and decode work while mostly idle.
fn long_quiet() -> Script {
    let mut mic = Vec::new();
    let mut sys = Vec::new();
    for minute in 0..20 {
        let at = 5.0 + f64::from(minute) * 60.0;
        if minute % 2 == 0 {
            sys.extend(sentence(at, 6, OTHER_HZ, 0.5));
        } else {
            mic.extend(sentence(at, 6, OWN_HZ, 0.3));
        }
    }
    Script {
        seconds: 1_200.0,
        mic,
        sys,
    }
}

fn render(seconds: f64, words: &[(f64, f64, f32)]) -> Vec<f32> {
    let total = (seconds * SR) as usize / WINDOW * WINDOW;
    let mut out = vec![0.0f32; total];
    for &(start, hz, amplitude) in words {
        let from = (start * SR) as usize;
        let to = (((start + WORD_S) * SR) as usize).min(total);
        for (index, sample) in out.iter_mut().enumerate().take(to).skip(from) {
            let t = index as f64 / SR;
            *sample += amplitude * (2.0 * std::f64::consts::PI * hz * t).sin() as f32;
        }
    }
    out
}

fn detector() -> Box<dyn SpeechDetector> {
    Box::new(EnergyDetector::new(WINDOW, 0.02, 2, 25))
}

fn run_fixture(name: &str, script: &Script, cost: CostModel) -> Result<BTreeMap<String, f64>> {
    let model = FakeModel::default();
    let decoder = model.load()?;
    let tracks = vec![
        Track {
            name: "mic",
            samples: render(script.seconds, &script.mic),
        },
        Track {
            name: "sys",
            samples: render(script.seconds, &script.sys),
        },
    ];
    let names = vec!["mic", "sys"];
    let config = PipelineConfig {
        priority_track: Some(1),
        ..PipelineConfig::meeting(12.0, 0.2)
    };
    let pipeline = Pipeline::new(config, vec![detector(), detector()]);
    let mut sim = Sim::new(
        pipeline,
        decoder.as_ref(),
        Cost::Fixed {
            base_s: cost.base_s,
            per_audio_s: cost.per_audio_s,
        },
        None,
        PreviewMode::Redecode,
        names.clone(),
        2.0 * WINDOW as f64 / SR,
    );
    let total = tracks[0].samples.len();
    let mut tick_end = WINDOW;
    while tick_end <= total {
        sim.tick(&tracks, tick_end, WINDOW);
        tick_end += WINDOW;
    }
    sim.finish(total);
    let audio_s = total as f64 / SR;
    let summary = sim.stats.summary(audio_s);
    let invariants = check_invariants(&sim.finals, &names);
    if !invariants.ok {
        eprintln!("{name}: {:?}", invariants.violations);
    }

    let mut metrics = BTreeMap::new();
    let mut put = |key: &str, value: f64| {
        metrics.insert(key.to_owned(), (value * 1000.0).round() / 1000.0);
    };
    put("invariant_violations", invariants.violations.len() as f64);
    put(
        "decoded_audio_per_audio_s",
        summary.decoded_audio_per_audio_s,
    );
    put("decodes_per_audio_min", summary.decodes_per_audio_min);
    put("max_final_queue", summary.max_final_queue as f64);
    put("forced_splits", summary.forced_splits as f64);
    put(
        "final_latency_p95_ms",
        summary.final_latency.p95_ms.unwrap_or(0.0),
    );
    put("drain_after_stop_ms", summary.drain_after_stop_ms);
    if let Some(sys) = summary.first_preview_latency_by_track.get("sys") {
        put("sys_first_preview_p95_ms", sys.p95_ms.unwrap_or(0.0));
    }
    put("max_held_s", sim.max_held as f64 / SR);
    let lost = lost_words(&sim.finals, "mic", &own_words(script))
        + lost_words(&sim.finals, "sys", &script.sys);
    put("words_lost", lost as f64);
    let longest = sim
        .finals
        .iter()
        .map(|segment| segment.end_ms - segment.start_ms)
        .max()
        .unwrap_or(0);
    put("longest_final_s", longest as f64 / 1000.0);
    if !script.mic.is_empty() && !script.sys.is_empty() {
        let (own_hidden, bleed_finals, bleed_hidden) = crosstalk(&sim.finals, script);
        put("own_finals_hidden", own_hidden as f64);
        if bleed_finals > 0 {
            put(
                "bleed_hidden_share",
                bleed_hidden as f64 / bleed_finals as f64,
            );
        }
    }
    Ok(metrics)
}

/// The user's own words on the mic (bleed copies excluded).
fn own_words(script: &Script) -> Vec<(f64, f64, f32)> {
    script
        .mic
        .iter()
        .copied()
        .filter(|&(_, hz, _)| hz == OWN_HZ)
        .collect()
}

/// Words whose centre lies in no final of the track.
fn lost_words(finals: &[FinalSegment], track: &str, words: &[(f64, f64, f32)]) -> usize {
    words
        .iter()
        .filter(|&&(start, _, _)| {
            let centre = ((start + WORD_S / 2.0) * 1000.0) as u64;
            !finals.iter().any(|segment| {
                segment.track == track && segment.start_ms <= centre && centre < segment.end_ms
            })
        })
        .count()
}

/// Judge every mic final against all system finals: (own finals hidden, finals with only
/// bleed, of those hidden).
fn crosstalk(finals: &[FinalSegment], script: &Script) -> (usize, usize, usize) {
    let mut reference = Reference::default();
    for segment in finals.iter().filter(|segment| segment.track == "sys") {
        reference.add_final(&segment.words);
    }
    let config = CrosstalkConfig::default();
    let own = own_words(script);
    let (mut own_hidden, mut bleed, mut bleed_hidden) = (0, 0, 0);
    for segment in finals.iter().filter(|segment| segment.track == "mic") {
        let hidden = !segment.words.is_empty()
            && reference.judge(&segment.words, &config).verdict == Verdict::Duplicate;
        let has_own = own.iter().any(|&(start, _, _)| {
            let (from, to) = ((start * 1000.0) as u64, ((start + WORD_S) * 1000.0) as u64);
            from < segment.end_ms && segment.start_ms < to
        });
        if has_own {
            own_hidden += usize::from(hidden);
        } else {
            bleed += 1;
            bleed_hidden += usize::from(hidden);
        }
    }
    (own_hidden, bleed, bleed_hidden)
}

type Fixture = (&'static str, fn() -> Script);

const FIXTURES: [Fixture; 3] = [
    ("meeting-dual", meeting_dual),
    ("continuous", continuous),
    ("long-quiet", long_quiet),
];

/// Metrics that must hold exactly; the rest get a 10% margin.
fn baseline_limit(metric: &str, value: f64) -> Limit {
    match metric {
        "invariant_violations" | "words_lost" | "own_finals_hidden" => Limit {
            min: None,
            max: Some(0.0),
        },
        "bleed_hidden_share" => Limit {
            min: Some((value * 900.0).floor() / 1000.0),
            max: None,
        },
        "forced_splits" => Limit {
            min: Some((value * 0.9).floor()),
            max: Some((value * 1.1).ceil()),
        },
        "longest_final_s" => Limit {
            min: None,
            max: Some(12.5),
        },
        _ => Limit {
            min: None,
            max: Some(((value * 1100.0).ceil() + 1.0) / 1000.0),
        },
    }
}

pub fn run(args: GateArgs) -> Result<()> {
    let thresholds: Option<Thresholds> = match std::fs::read_to_string(&args.thresholds) {
        Ok(text) => Some(serde_json::from_str(&text).context("parse thresholds")?),
        Err(_) if args.write_baseline => None,
        Err(error) => {
            return Err(error).with_context(|| format!("read {}", args.thresholds.display()));
        }
    };
    // Roughly SenseVoice with two threads on the 4-core baseline.
    let cost = thresholds.as_ref().map_or(
        CostModel {
            base_s: 0.03,
            per_audio_s: 0.1,
        },
        |thresholds| thresholds.cost_model,
    );
    let mut measured = BTreeMap::new();
    for (name, script) in FIXTURES {
        measured.insert(name.to_owned(), run_fixture(name, &script(), cost)?);
    }
    let json = serde_json::to_string_pretty(&measured)?;
    println!("{json}");
    if let Some(path) = &args.report {
        std::fs::write(path, json + "\n")?;
    }
    if args.write_baseline {
        let fixtures = measured
            .iter()
            .map(|(name, metrics)| {
                let limits = metrics
                    .iter()
                    .map(|(metric, value)| (metric.clone(), baseline_limit(metric, *value)))
                    .collect();
                (name.clone(), limits)
            })
            .collect();
        let thresholds = Thresholds {
            cost_model: cost,
            fixtures,
        };
        std::fs::write(
            &args.thresholds,
            serde_json::to_string_pretty(&thresholds)? + "\n",
        )?;
        eprintln!("wrote {}", args.thresholds.display());
        return Ok(());
    }
    let thresholds = thresholds.expect("read above");
    let mut failures = Vec::new();
    for (fixture, limits) in &thresholds.fixtures {
        let Some(values) = measured.get(fixture) else {
            failures.push(format!("{fixture}: fixture missing"));
            continue;
        };
        for (metric, limit) in limits {
            let Some(&value) = values.get(metric) else {
                failures.push(format!("{fixture}.{metric}: not measured"));
                continue;
            };
            if limit.max.is_some_and(|max| value > max) || limit.min.is_some_and(|min| value < min)
            {
                failures.push(format!("{fixture}.{metric} = {value} outside {limit:?}"));
            }
        }
    }
    if failures.is_empty() {
        eprintln!("all gates pass");
        Ok(())
    } else {
        for failure in &failures {
            eprintln!("FAIL {failure}");
        }
        bail!("{} gate(s) failed", failures.len())
    }
}
