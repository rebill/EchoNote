//! Cuts one track into finals: at pauses reported by the detector, and at a hard length cap
//! for continuous speech. Positions are sample indices on the track's 16 kHz timeline.

/// Segmenter settings in samples.
#[derive(Clone, Debug)]
pub struct SegmenterConfig {
    /// Decoded audio of one final never exceeds this (plus `overlap`).
    pub hard_cap: usize,
    /// A forced cut goes at the quietest frame within this distance before the cap.
    pub split_search: usize,
    pub split_frame: usize,
    /// Audio shared by both sides of a forced cut; their tokens are split at the cut by
    /// timestamp. Zero for models without token timestamps.
    pub overlap: usize,
    /// Audio kept after the detected end of speech.
    pub tail: usize,
}

impl SegmenterConfig {
    pub fn meeting(hard_cap_s: f64, overlap_s: f64) -> Self {
        let rate = crate::SAMPLE_RATE as f64;
        Self {
            hard_cap: if hard_cap_s > 0.0 {
                (hard_cap_s * rate) as usize
            } else {
                usize::MAX
            },
            split_search: (2.0 * rate) as usize,
            split_frame: (0.02 * rate) as usize,
            overlap: (overlap_s * rate) as usize,
            tail: (0.1 * rate) as usize,
        }
    }
}

/// A finished piece of speech. `span` is what this final owns on the timeline (finals of a
/// track tile it without overlap); `audio` is what gets decoded and contains `span`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cut {
    pub span_start: usize,
    pub span_end: usize,
    pub audio_start: usize,
    pub audio_end: usize,
    /// True when the hard cap ended it rather than a pause.
    pub forced: bool,
}

/// The utterance currently being spoken.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Region {
    pub span_start: usize,
    pub audio_start: usize,
    /// Continues the previous final after a forced cut.
    pub continued: bool,
}

#[derive(Debug)]
pub struct Segmenter {
    config: SegmenterConfig,
    region: Option<Region>,
    /// End of the last emitted span; nothing before it is emitted again.
    committed: usize,
}

impl Segmenter {
    pub fn new(config: SegmenterConfig) -> Self {
        Self {
            config,
            region: None,
            committed: 0,
        }
    }

    pub fn region(&self) -> Option<Region> {
        self.region
    }

    pub fn committed(&self) -> usize {
        self.committed
    }

    /// Process the detector's answer for the window ending at `fed`. `audio(start, end)`
    /// returns samples in `[start, end)`; it is only called inside the current region.
    pub fn on_window(
        &mut self,
        fed: usize,
        speech: bool,
        onset_lag: usize,
        release_lag: usize,
        audio: impl Fn(usize, usize) -> Vec<f32>,
    ) -> Option<Cut> {
        match (self.region, speech) {
            (None, true) => {
                let start = fed.saturating_sub(onset_lag).max(self.committed);
                self.region = Some(Region {
                    span_start: start,
                    audio_start: start,
                    continued: false,
                });
                None
            }
            (Some(region), false) => {
                self.region = None;
                let end = (fed.saturating_sub(release_lag) + self.config.tail).min(fed);
                if end <= region.span_start {
                    return None;
                }
                Some(self.emit(region, end, end, false))
            }
            (Some(region), true) if fed - region.audio_start >= self.config.hard_cap => {
                let lowest = region.span_start + 2 * self.config.overlap + self.config.split_frame;
                let from = fed.saturating_sub(self.config.split_search).max(lowest);
                let cut_at =
                    quietest(&audio(from, fed), from, self.config.split_frame).unwrap_or(fed);
                let audio_end = (cut_at + self.config.overlap).min(fed);
                let cut = self.emit(region, cut_at, audio_end, true);
                self.region = Some(Region {
                    span_start: cut_at,
                    audio_start: cut_at
                        .saturating_sub(self.config.overlap)
                        .max(region.span_start),
                    continued: true,
                });
                Some(cut)
            }
            _ => None,
        }
    }

    /// Close an open region at end of input.
    pub fn finish(&mut self, total: usize) -> Option<Cut> {
        let region = self.region.take()?;
        (total > region.span_start).then(|| self.emit(region, total, total, false))
    }

    fn emit(&mut self, region: Region, span_end: usize, audio_end: usize, forced: bool) -> Cut {
        self.committed = span_end;
        Cut {
            span_start: region.span_start,
            span_end,
            audio_start: region.audio_start,
            audio_end,
            forced,
        }
    }
}

/// Midpoint of the lowest-energy frame in `samples`, which start at timeline index `start`.
fn quietest(samples: &[f32], start: usize, frame: usize) -> Option<usize> {
    samples
        .chunks_exact(frame)
        .enumerate()
        .map(|(index, chunk)| {
            (
                chunk.iter().map(|sample| sample * sample).sum::<f32>(),
                index,
            )
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, index)| start + index * frame + frame / 2)
}
