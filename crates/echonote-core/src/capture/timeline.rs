//! One track's path from a capture source to the 16 kHz timeline: downmix to mono, resample
//! with rubato, and place every callback buffer at its capture time.
//!
//! Timeline sample `n` is `n / 16000` seconds after the session start on the [`now_ns`]
//! clock. Each [`Stamp`] says when a buffer was captured; the timeline compares that with
//! where the buffer would land:
//!
//! - more than `gap_s` behind (a late start, a stalled or switched device, a dropped buffer):
//!   insert silence so the buffer lands at its capture time;
//! - a smaller error, either way (clock drift between devices): steer it away over
//!   `horizon_s` by nudging the resampling ratio by at most `max_correction`. Samples are never
//!   dropped or duplicated, so the steady error is the device drift times `horizon_s` (5 ms at
//!   500 ppm).
//!
//! [`now_ns`]: super::ring::now_ns

use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Async, FixedAsync, Resampler, SincInterpolationParameters, WindowFunction};

use super::SourceFormat;
use super::ring::Stamp;
use crate::SAMPLE_RATE;

const OUT_RATE: f64 = SAMPLE_RATE as f64;
/// Weight of each stamp in the smoothed error (stamps come every 10–20 ms).
const ERROR_ALPHA: f64 = 0.05;

#[derive(Clone, Copy, Debug)]
pub struct TimelineConfig {
    /// Fill with silence when the timeline is this far behind the capture clock.
    pub gap_s: f64,
    /// Longest silence inserted at once; a longer jump (the machine slept) moves the anchor.
    pub max_fill_s: f64,
    /// Steer the remaining error away over about this long.
    pub horizon_s: f64,
    /// Largest relative rate change used for steering.
    pub max_correction: f64,
}

impl Default for TimelineConfig {
    fn default() -> Self {
        Self {
            gap_s: 0.05,
            max_fill_s: 10.0,
            horizon_s: 10.0,
            max_correction: 0.005,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TimelineStats {
    /// Samples of silence inserted for gaps.
    pub gap_samples: u64,
    /// Smoothed alignment error in ms; positive means behind the capture clock.
    pub error_ms: f64,
    /// Rate correction currently applied, in parts per million.
    pub correction_ppm: f64,
    /// Clock jumps longer than `max_fill_s` that moved the anchor instead of being filled.
    pub jumps: u64,
}

pub struct Timeline {
    config: TimelineConfig,
    /// Clock value of timeline sample 0; signed because padding moves it earlier.
    start_ns: i64,
    format: SourceFormat,
    resampler: Async<f32>,
    chunk: usize,
    /// Mono source frames waiting for a full resampler chunk.
    input: Vec<f32>,
    scratch: Vec<f32>,
    /// Interleaved samples drained from the ring that no stamp has described yet.
    pending: Vec<f32>,
    /// Source frames fed so far in the current stream's numbering.
    consumed: u64,
    /// Timeline position of the next source frame to enter the resampler.
    position: f64,
    /// Samples handed out so far.
    emitted: u64,
    /// Resampler start-up delay still to discard.
    skip: usize,
    correction: f64,
    error: Option<f64>,
    stats: TimelineStats,
}

impl Timeline {
    pub fn new(format: SourceFormat, start_ns: u64, config: TimelineConfig) -> Self {
        let chunk = (format.rate as usize / 100).max(16);
        let resampler = build_resampler(format.rate, chunk, config.max_correction);
        let skip = resampler.output_delay();
        Self {
            config,
            start_ns: start_ns as i64,
            format,
            scratch: vec![0.0; resampler.output_frames_max()],
            resampler,
            chunk,
            input: Vec::with_capacity(chunk * 2),
            pending: Vec::new(),
            consumed: 0,
            position: 0.0,
            emitted: 0,
            skip,
            correction: 1.0,
            error: None,
            stats: TimelineStats::default(),
        }
    }

    pub fn format(&self) -> SourceFormat {
        self.format
    }

    /// Samples handed out so far: the track's length on the timeline.
    pub fn emitted(&self) -> u64 {
        self.emitted
    }

    pub fn stats(&self) -> TimelineStats {
        self.stats
    }

    /// Timeline sample for a clock value.
    pub fn sample_at(&self, at_ns: u64) -> f64 {
        (at_ns as f64 - self.start_ns as f64) * 1e-9 * OUT_RATE
    }

    /// Move the clock origin later, e.g. by the length of a pause, so the time spent paused
    /// does not count as a gap.
    pub fn shift(&mut self, ns: u64) {
        self.start_ns += ns as i64;
    }

    /// Flush and append `samples` of silence that stand for no capture time (the clock origin
    /// moves earlier by as much). Used before a pause so the segmenter sees the end of speech.
    pub fn pad(&mut self, samples: usize, out: &mut Vec<f32>) {
        self.flush(out);
        out.extend(std::iter::repeat_n(0.0, samples));
        self.emitted += samples as u64;
        self.position = self.emitted as f64;
        self.start_ns -= (samples as f64 / OUT_RATE * 1e9) as i64;
    }

    /// The source delivered nothing for a while (a silent loopback, a stalled device): emit
    /// silence up to `until_ns` so later stages keep moving. A late buffer from before
    /// `until_ns` then lands a little late instead of being lost.
    pub fn fill_to(&mut self, until_ns: u64, out: &mut Vec<f32>) {
        let target = self.sample_at(until_ns);
        if target <= self.position + self.chunk as f64 {
            return;
        }
        self.flush(out);
        let samples = (target - self.position).floor().max(0.0) as usize;
        out.extend(std::iter::repeat_n(0.0, samples));
        self.emitted += samples as u64;
        self.position = self.emitted as f64;
        self.stats.gap_samples += samples as u64;
    }

    /// Add drained samples and their stamps; resampled audio is appended to `out`.
    pub fn push(&mut self, samples: &[f32], stamps: &[Stamp], out: &mut Vec<f32>) {
        self.pending.extend_from_slice(samples);
        let channels = self.format.channels;
        let pending = std::mem::take(&mut self.pending);
        let mut used = 0;
        for stamp in stamps {
            if stamp.frame > self.consumed {
                // A buffer the ring had no room for: its frames are missing.
                self.feed_silence((stamp.frame - self.consumed) as usize, out);
                self.consumed = stamp.frame;
            }
            let overlap = (self.consumed - stamp.frame).min(u64::from(stamp.frames));
            let at_ns = stamp.at_ns + overlap * 1_000_000_000 / u64::from(self.format.rate);
            self.observe(at_ns, out);
            let wanted = (u64::from(stamp.frames) - overlap) as usize * channels;
            let take = wanted.min(pending.len() - used);
            self.feed_interleaved(&pending[used..used + take], out);
            used += take;
            self.consumed += (take / channels) as u64;
        }
        self.pending = pending;
        self.pending.drain(..used);
    }

    /// The source ended or is being replaced: feed what is left without a stamp, so a new
    /// stream can start at frame 0.
    pub fn end_source(&mut self, out: &mut Vec<f32>) {
        let pending = std::mem::take(&mut self.pending);
        self.feed_interleaved(&pending, out);
        self.consumed = 0;
    }

    /// Switch to a new source. A different format flushes and rebuilds the resampler; the
    /// first stamp of the new source fills the gap since the old one stopped.
    pub fn set_source(&mut self, format: SourceFormat, out: &mut Vec<f32>) {
        self.end_source(out);
        if format != self.format {
            self.flush(out);
            self.format = format;
            self.chunk = (format.rate as usize / 100).max(16);
            self.resampler = build_resampler(format.rate, self.chunk, self.config.max_correction);
            self.scratch = vec![0.0; self.resampler.output_frames_max()];
            self.skip = self.resampler.output_delay();
            self.correction = 1.0;
        }
    }

    /// Emit everything fed so far (the resampler holds a few ms), then continue from there.
    pub fn flush(&mut self, out: &mut Vec<f32>) {
        let target = self.position.round().max(0.0) as u64;
        let mut rounds = 0;
        while self.emitted < target && rounds < 1_000 {
            let pad = self.chunk - self.input.len() % self.chunk;
            self.input.extend(std::iter::repeat_n(0.0, pad));
            self.process(out, Some(target));
            rounds += 1;
        }
        self.input.clear();
        self.resampler.reset();
        self.skip = self.resampler.output_delay();
        self.position = self.emitted as f64;
    }

    fn ratio(&self) -> f64 {
        OUT_RATE / f64::from(self.format.rate) * self.correction
    }

    /// Compare where the next frame lands with its capture time; fill or steer.
    fn observe(&mut self, at_ns: u64, out: &mut Vec<f32>) {
        let mut error = self.sample_at(at_ns) - self.position;
        let gap = self.config.gap_s * OUT_RATE;
        if error > gap {
            let max_fill = self.config.max_fill_s * OUT_RATE;
            if error > max_fill {
                let excess = error - max_fill;
                self.start_ns += (excess / OUT_RATE * 1e9) as i64;
                self.stats.jumps += 1;
                error = max_fill;
            }
            let frames = (error / self.ratio()).round() as usize;
            let before = self.position;
            self.feed_silence(frames, out);
            self.stats.gap_samples += (self.position - before).round() as u64;
            error = self.sample_at(at_ns) - self.position;
            self.error = Some(error);
        }
        let smoothed = match self.error {
            Some(previous) => previous + ERROR_ALPHA * (error - previous),
            None => error,
        };
        self.error = Some(smoothed);
        let max = self.config.max_correction;
        let correction =
            (1.0 + smoothed / (self.config.horizon_s * OUT_RATE)).clamp(1.0 - max, 1.0 + max);
        if (correction - self.correction).abs() > 1e-6 {
            self.correction = correction;
            if let Some(adjustable) = self.resampler.as_adjustable() {
                let _ = adjustable.set_resample_ratio_relative(correction, true);
            }
        }
        self.stats.error_ms = smoothed / OUT_RATE * 1_000.0;
        self.stats.correction_ppm = (self.correction - 1.0) * 1e6;
    }

    fn feed_interleaved(&mut self, data: &[f32], out: &mut Vec<f32>) {
        let channels = self.format.channels;
        let frames = data.len() / channels;
        if frames == 0 {
            return;
        }
        if channels == 1 {
            self.input.extend_from_slice(&data[..frames]);
        } else {
            let scale = 1.0 / channels as f32;
            self.input.extend(
                data.chunks_exact(channels)
                    .map(|frame| frame.iter().sum::<f32>() * scale),
            );
        }
        self.position += frames as f64 * self.ratio();
        self.process(out, None);
    }

    fn feed_silence(&mut self, frames: usize, out: &mut Vec<f32>) {
        let mut left = frames;
        while left > 0 {
            let take = left.min(self.chunk * 16);
            self.input.extend(std::iter::repeat_n(0.0, take));
            self.position += take as f64 * self.ratio();
            self.process(out, None);
            left -= take;
        }
    }

    /// Resample every full chunk of `input`; stop emitting at `limit` (used by `flush`).
    fn process(&mut self, out: &mut Vec<f32>, limit: Option<u64>) {
        let mut offset = 0;
        while self.input.len() - offset >= self.chunk {
            let input =
                InterleavedSlice::new(&self.input[offset..offset + self.chunk], 1, self.chunk)
                    .expect("chunk-sized input");
            let frames = self.resampler.output_frames_next();
            let mut output =
                InterleavedSlice::new_mut(&mut self.scratch[..frames], 1, frames).expect("output");
            let (_, written) = self
                .resampler
                .process_into_buffer(&input, &mut output, None)
                .expect("buffers sized by the resampler");
            offset += self.chunk;
            let skip = self.skip.min(written);
            self.skip -= skip;
            let mut produced = &self.scratch[skip..written];
            if let Some(limit) = limit {
                let room = limit.saturating_sub(self.emitted) as usize;
                produced = &produced[..produced.len().min(room)];
            }
            out.extend_from_slice(produced);
            self.emitted += produced.len() as u64;
        }
        self.input.drain(..offset);
    }
}

fn build_resampler(rate: u32, chunk: usize, max_correction: f64) -> Async<f32> {
    let params = SincInterpolationParameters::new(128, WindowFunction::BlackmanHarris2);
    Async::<f32>::new_sinc(
        OUT_RATE / f64::from(rate),
        1.0 + 2.0 * max_correction,
        &params,
        chunk,
        1,
        FixedAsync::Input,
    )
    .expect("valid resampler parameters")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: u32 = 48_000;
    const BLOCK: usize = 480;

    /// Stereo 48 kHz with an impulse at stream frame `click`, fed in 10 ms buffers whose
    /// capture times come from `clock(frame)` (seconds). `skip` drops the buffers covering
    /// those frames, as a full ring does.
    fn run(
        seconds: f64,
        click: usize,
        clock: impl Fn(usize) -> f64,
        skip: std::ops::Range<usize>,
    ) -> (Vec<f32>, Timeline) {
        let format = SourceFormat {
            rate: SRC,
            channels: 2,
        };
        let mut timeline = Timeline::new(format, 1_000_000_000, TimelineConfig::default());
        let mut out = Vec::new();
        let total = (seconds * SRC as f64) as usize;
        for start in (0..total).step_by(BLOCK) {
            if skip.contains(&start) {
                continue;
            }
            let mut buffer = vec![0.0f32; BLOCK * 2];
            if (start..start + BLOCK).contains(&click) {
                buffer[(click - start) * 2] = 1.0;
                buffer[(click - start) * 2 + 1] = 1.0;
            }
            let stamp = Stamp {
                frame: start as u64,
                frames: BLOCK as u32,
                at_ns: 1_000_000_000 + (clock(start) * 1e9) as u64,
            };
            timeline.push(&buffer, &[stamp], &mut out);
        }
        timeline.flush(&mut out);
        (out, timeline)
    }

    fn peak(samples: &[f32]) -> usize {
        samples
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
            .unwrap()
            .0
    }

    fn real_time(frame: usize) -> f64 {
        frame as f64 / SRC as f64
    }

    #[test]
    fn exact_stamps_keep_length_and_position() {
        let (out, timeline) = run(2.0, 48_000, real_time, 0..0);
        assert!((out.len() as i64 - 32_000).abs() <= 1, "{}", out.len());
        assert!(
            (peak(&out) as i64 - 16_000).abs() <= 1,
            "peak at {}",
            peak(&out)
        );
        // Downmixed, then low-passed: the impulse keeps most of its energy.
        assert!(out[peak(&out)] > 0.25, "{}", out[peak(&out)]);
        assert_eq!(timeline.stats().gap_samples, 0);
    }

    #[test]
    fn a_late_start_is_filled_with_silence() {
        let (out, timeline) = run(2.0, 48_000, |f| real_time(f) + 0.3, 0..0);
        assert!(
            (peak(&out) as i64 - 20_800).abs() <= 2,
            "peak at {}",
            peak(&out)
        );
        assert!(out[..4_700].iter().all(|x| x.abs() < 1e-6));
        assert!((timeline.stats().gap_samples as i64 - 4_800).abs() <= 2);
    }

    #[test]
    fn dropped_buffers_and_stalls_keep_later_audio_in_place() {
        // A full ring dropped 100 ms; frame numbers still advance.
        let (out, _) = run(2.0, 72_000, real_time, 24_000..28_800);
        assert!(
            (peak(&out) as i64 - 24_000).abs() <= 2,
            "peak at {}",
            peak(&out)
        );
        // The device stalled for 200 ms at 1.0 s: frames continue, capture time jumps.
        let stalled = |f: usize| real_time(f) + if f >= 48_000 { 0.2 } else { 0.0 };
        let (out, timeline) = run(2.0, 72_000, stalled, 0..0);
        assert!(
            (peak(&out) as i64 - 27_200).abs() <= 2,
            "peak at {}",
            peak(&out)
        );
        assert!((timeline.stats().gap_samples as i64 - 3_200).abs() <= 2);
    }

    #[test]
    fn clock_drift_is_steered_within_20_ms() {
        for ppm in [-1_500.0, -500.0, 500.0, 1_500.0] {
            // The device delivers `rate * (1 + ppm)` frames per second of host time.
            let clock = move |f: usize| real_time(f) / (1.0 + ppm * 1e-6);
            let (out, timeline) = run(120.0, 0, clock, 0..0);
            let host_s = 120.0 / (1.0 + ppm * 1e-6);
            let error_ms = (host_s * 16_000.0 - out.len() as f64) / 16.0;
            assert!(error_ms.abs() < 20.0, "{ppm} ppm: {error_ms:.1} ms");
            assert!(timeline.stats().gap_samples == 0, "{ppm} ppm filled a gap");
            // Without steering the error would be ppm * 120 s.
            assert!(error_ms.abs() < (ppm * 1e-6 * 120.0 * 1_000.0f64).abs() / 2.0);
        }
    }

    #[test]
    fn a_new_source_with_another_format_continues_the_timeline() {
        let mut timeline = Timeline::new(
            SourceFormat {
                rate: 48_000,
                channels: 1,
            },
            0,
            TimelineConfig::default(),
        );
        let mut out = Vec::new();
        for i in 0..100u64 {
            let stamp = Stamp {
                frame: i * 480,
                frames: 480,
                at_ns: i * 10_000_000,
            };
            timeline.push(&[0.1; 480], &[stamp], &mut out);
        }
        // Headphones plugged in at 1.0 s: 44.1 kHz stereo, first buffer 100 ms later.
        let format = SourceFormat {
            rate: 44_100,
            channels: 2,
        };
        timeline.set_source(format, &mut out);
        assert!((out.len() as i64 - 16_000).abs() <= 1, "{}", out.len());
        for i in 0..100u64 {
            let stamp = Stamp {
                frame: i * 441,
                frames: 441,
                at_ns: 1_100_000_000 + i * 10_000_000,
            };
            timeline.push(&[0.2; 882], &[stamp], &mut out);
        }
        timeline.flush(&mut out);
        assert!((out.len() as i64 - 33_600).abs() <= 2, "{}", out.len());
        assert!((out[25_000] - 0.2).abs() < 0.01 && out[16_800].abs() < 0.01);
    }

    #[test]
    fn padding_and_idle_fill_keep_the_clock_mapping() {
        let format = SourceFormat {
            rate: 16_000,
            channels: 1,
        };
        let mut timeline = Timeline::new(format, 0, TimelineConfig::default());
        let mut out = Vec::new();
        let push = |timeline: &mut Timeline, out: &mut Vec<f32>, from: u64, at_ns: u64| {
            for i in 0..50u64 {
                let stamp = Stamp {
                    frame: from + i * 160,
                    frames: 160,
                    at_ns: at_ns + i * 10_000_000,
                };
                timeline.push(&[0.1; 160], &[stamp], out);
            }
        };
        push(&mut timeline, &mut out, 0, 0);
        // Pause at 0.5 s with 0.25 s of padding, resume 10 s later.
        timeline.pad(4_000, &mut out);
        assert_eq!(out.len(), 12_000);
        timeline.shift(10_000_000_000);
        timeline.set_source(format, &mut out);
        push(&mut timeline, &mut out, 0, 10_500_000_000);
        timeline.flush(&mut out);
        assert!((out.len() as i64 - 20_000).abs() <= 2, "{}", out.len());
        assert_eq!(timeline.stats().gap_samples, 0);
        // Nothing arrives for 2 s: idle fill catches up, a late buffer is not lost.
        timeline.fill_to(13_000_000_000, &mut out);
        assert!((out.len() as i64 - 52_000).abs() <= 2, "{}", out.len());
    }
}
