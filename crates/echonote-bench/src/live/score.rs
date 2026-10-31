use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use super::sim::{FinalSegment, Track};
use crate::metrics::{edit_distance, lcs_len, round3};
use crate::text::tokens;

/// 20 ms energy frames at 16 kHz.
const FRAME: usize = 320;
const FRAME_MS: u64 = 20;
/// Search mic lag behind sys output from 0 to 200 ms.
const MAX_LAG_FRAMES: usize = 10;
const OVERLAP_PAD_MS: u64 = 300;

#[derive(Deserialize, Clone)]
pub struct RefSegment {
    pub track: String,
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

pub fn load_refs(path: &Path) -> Result<Vec<RefSegment>> {
    let raw = std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    Ok(serde_json::from_str(&raw)?)
}

#[derive(Serialize)]
pub struct TrackAccuracy {
    pub track: String,
    pub ref_tokens: usize,
    pub errors: usize,
    pub error_rate: f64,
}

pub fn accuracy(
    finals: &[FinalSegment],
    refs: &[RefSegment],
    track: &str,
    cn: bool,
) -> TrackAccuracy {
    let hyp = finals.iter().filter(|segment| segment.track == track);
    score_track(hyp, refs, track, cn)
}

fn score_track<'a>(
    hyp: impl Iterator<Item = &'a FinalSegment>,
    refs: &[RefSegment],
    track: &str,
    cn: bool,
) -> TrackAccuracy {
    let mut hyp: Vec<&FinalSegment> = hyp.collect();
    hyp.sort_by_key(|segment| segment.start_ms);
    let mut reference: Vec<&RefSegment> = refs
        .iter()
        .filter(|segment| segment.track == track)
        .collect();
    reference.sort_by_key(|segment| segment.start_ms);
    let reference: Vec<String> = reference
        .iter()
        .flat_map(|segment| tokens(&segment.text, cn))
        .collect();
    let hypothesis: Vec<String> = hyp
        .iter()
        .flat_map(|segment| tokens(&segment.text, cn))
        .collect();
    let errors = edit_distance(&reference, &hypothesis);
    TrackAccuracy {
        track: track.into(),
        ref_tokens: reference.len(),
        errors,
        error_rate: round3(errors as f64 / reference.len().max(1) as f64 * 1000.0) / 1000.0,
    }
}

#[derive(Serialize)]
pub struct Invariants {
    pub ok: bool,
    pub violations: Vec<String>,
}

/// Per track, finals must be non-empty, ordered by start and must not overlap.
pub fn check_invariants(finals: &[FinalSegment], tracks: &[&str]) -> Invariants {
    let mut violations = Vec::new();
    for name in tracks {
        let mut previous: Option<&FinalSegment> = None;
        for segment in finals.iter().filter(|segment| segment.track == *name) {
            if segment.end_ms <= segment.start_ms {
                violations.push(format!("{name}: empty segment at {} ms", segment.start_ms));
            }
            if let Some(previous) = previous {
                if segment.start_ms < previous.start_ms {
                    violations.push(format!(
                        "{name}: {} ms starts before {} ms",
                        segment.start_ms, previous.start_ms
                    ));
                } else if segment.start_ms < previous.end_ms {
                    violations.push(format!(
                        "{name}: {} ms overlaps previous end {} ms",
                        segment.start_ms, previous.end_ms
                    ));
                }
            }
            previous = Some(segment);
        }
    }
    violations.truncate(20);
    Invariants {
        ok: violations.is_empty(),
        violations,
    }
}

#[derive(Serialize)]
pub struct RuleScore {
    pub rule: &'static str,
    pub flagged: usize,
    pub true_positive: usize,
    pub precision: f64,
    pub recall: f64,
    pub mic_error_rate_after: f64,
}

#[derive(Serialize)]
pub struct DedupReport {
    pub mic_segments: usize,
    pub true_bleed_segments: usize,
    pub mic_error_rate_before: f64,
    pub rules: Vec<RuleScore>,
}

type Rule = fn(&MicFeature) -> bool;

/// Echo-detection features of one mic final against the sys track.
#[derive(Serialize)]
pub struct MicFeature {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
    /// Share of mic tokens found, in order, in sys finals overlapping the segment.
    pub text_sim: f64,
    /// Best Pearson correlation of 20 ms log energy with sys lagged 0 to 200 ms.
    pub energy_corr: f64,
}

pub fn mic_features(
    finals: &[FinalSegment],
    mic: &Track,
    sys: &Track,
    cn: bool,
) -> Vec<MicFeature> {
    let mic_energy = log_energy(&mic.samples);
    let sys_energy = log_energy(&sys.samples);
    finals
        .iter()
        .filter(|segment| segment.track == mic.name)
        .map(|segment| {
            let mic_tokens = tokens(&segment.text, cn);
            let sys_tokens: Vec<String> = finals
                .iter()
                .filter(|other| {
                    other.track == sys.name
                        && overlaps(segment, other.start_ms, other.end_ms, OVERLAP_PAD_MS)
                })
                .flat_map(|other| tokens(&other.text, cn))
                .collect();
            let text_sim = if mic_tokens.is_empty() {
                0.0
            } else {
                lcs_len(&mic_tokens, &sys_tokens) as f64 / mic_tokens.len() as f64
            };
            let first = (segment.start_ms / FRAME_MS) as usize;
            let last = ((segment.end_ms / FRAME_MS) as usize).min(mic_energy.len());
            MicFeature {
                start_ms: segment.start_ms,
                end_ms: segment.end_ms,
                text: segment.text.clone(),
                text_sim: round3(text_sim),
                energy_corr: round3(best_lagged_correlation(
                    &mic_energy,
                    &sys_energy,
                    first,
                    last,
                )),
            }
        })
        .collect()
}

/// Evaluate rules that drop mic segments which repeat what the sys track just played.
pub fn dedup(
    finals: &[FinalSegment],
    refs: &[RefSegment],
    mic: &Track,
    sys: &Track,
    cn: bool,
) -> DedupReport {
    let mic_finals: Vec<&FinalSegment> = finals
        .iter()
        .filter(|segment| segment.track == mic.name)
        .collect();
    let features = mic_features(finals, mic, sys, cn);
    let in_track = |segment: &FinalSegment, track: &str| -> u64 {
        refs.iter()
            .filter(|reference| reference.track == track)
            .map(|reference| overlap_ms(segment, reference.start_ms, reference.end_ms))
            .sum()
    };
    let is_bleed: Vec<bool> = mic_finals
        .iter()
        .map(|segment| in_track(segment, sys.name) > in_track(segment, mic.name))
        .collect();

    let before = score_track(mic_finals.iter().copied(), refs, mic.name, cn).error_rate;
    let rules: [(&'static str, Rule); 3] = [
        ("text_sim>=0.6", |feature| feature.text_sim >= 0.6),
        ("energy_corr>=0.8", |feature| feature.energy_corr >= 0.8),
        ("text>=0.6|(text>=0.3&energy>=0.6)", |feature| {
            feature.text_sim >= 0.6 || (feature.text_sim >= 0.3 && feature.energy_corr >= 0.6)
        }),
    ];
    let true_bleed = is_bleed.iter().filter(|bleed| **bleed).count();
    let rules = rules
        .iter()
        .map(|(name, rule)| {
            let flagged: Vec<bool> = features.iter().map(rule).collect();
            let flagged_count = flagged.iter().filter(|value| **value).count();
            let true_positive = is_bleed
                .iter()
                .zip(&flagged)
                .filter(|(bleed, flag)| **flag && **bleed)
                .count();
            let kept = mic_finals
                .iter()
                .zip(&flagged)
                .filter(|(_, flag)| !**flag)
                .map(|(segment, _)| *segment);
            RuleScore {
                rule: name,
                flagged: flagged_count,
                true_positive,
                precision: round3(true_positive as f64 / flagged_count.max(1) as f64),
                recall: round3(true_positive as f64 / true_bleed.max(1) as f64),
                mic_error_rate_after: score_track(kept, refs, mic.name, cn).error_rate,
            }
        })
        .collect();
    DedupReport {
        mic_segments: mic_finals.len(),
        true_bleed_segments: true_bleed,
        mic_error_rate_before: before,
        rules,
    }
}

fn overlaps(segment: &FinalSegment, start_ms: u64, end_ms: u64, pad_ms: u64) -> bool {
    start_ms < segment.end_ms + pad_ms && segment.start_ms < end_ms + pad_ms
}

fn overlap_ms(segment: &FinalSegment, start_ms: u64, end_ms: u64) -> u64 {
    segment
        .end_ms
        .min(end_ms)
        .saturating_sub(segment.start_ms.max(start_ms))
}

/// Log RMS energy in dB per 20 ms frame.
fn log_energy(samples: &[f32]) -> Vec<f64> {
    samples
        .chunks(FRAME)
        .map(|frame| {
            let power = frame
                .iter()
                .map(|sample| (*sample as f64).powi(2))
                .sum::<f64>()
                / frame.len() as f64;
            10.0 * (power + 1e-10).log10()
        })
        .collect()
}

/// Highest Pearson correlation between the mic envelope and the sys envelope shifted earlier
/// by 0..=200 ms. Returns 0 when sys is silent over the window.
fn best_lagged_correlation(mic: &[f64], sys: &[f64], first: usize, last: usize) -> f64 {
    const SILENCE_DB: f64 = -50.0;
    let mut best = 0.0f64;
    for lag in 0..=MAX_LAG_FRAMES {
        if first < lag || last <= first + 4 || last > sys.len() + lag {
            continue;
        }
        let left = &mic[first..last];
        let right = &sys[first - lag..last - lag];
        let active = right.iter().filter(|value| **value > SILENCE_DB).count();
        if active * 4 < right.len() {
            continue;
        }
        best = best.max(pearson(left, right));
    }
    round3(best)
}

fn pearson(left: &[f64], right: &[f64]) -> f64 {
    let n = left.len() as f64;
    let (mean_left, mean_right) = (left.iter().sum::<f64>() / n, right.iter().sum::<f64>() / n);
    let (mut covariance, mut var_left, mut var_right) = (0.0, 0.0, 0.0);
    for (a, b) in left.iter().zip(right) {
        covariance += (a - mean_left) * (b - mean_right);
        var_left += (a - mean_left).powi(2);
        var_right += (b - mean_right).powi(2);
    }
    if var_left <= f64::EPSILON || var_right <= f64::EPSILON {
        return 0.0;
    }
    covariance / (var_left.sqrt() * var_right.sqrt())
}
