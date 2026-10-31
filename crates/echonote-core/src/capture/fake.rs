//! Sources for tests, benchmarks and development: in-memory samples or a WAV file, played into
//! a ring by a feeder thread in 10 ms buffers like a device callback.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use super::ring::{RingConsumer, now_ns, ring};
use super::{AudioSource, CaptureError, RING_SECONDS, SourceFormat, Stamp};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pace {
    /// One buffer every 10 ms of the shared clock, as a device delivers them.
    Real,
    /// As fast as the consumer drains the ring; nothing is dropped. Stamps still say where
    /// each buffer belongs in time, so the timeline places them exactly.
    Fast,
}

pub struct FakeSource {
    consumer: RingConsumer,
    format: SourceFormat,
    name: String,
    stop: Arc<AtomicBool>,
    done: Arc<AtomicBool>,
    failed: Arc<AtomicBool>,
    feeder: Option<JoinHandle<()>>,
}

impl FakeSource {
    /// Play interleaved `samples` from now on. `fail_after_s` makes the source report a
    /// device failure after that much audio, like an unplugged headset.
    pub fn start(
        name: impl Into<String>,
        format: SourceFormat,
        samples: Vec<f32>,
        pace: Pace,
        fail_after_s: Option<f64>,
    ) -> Self {
        let (mut producer, consumer) = ring(format.rate, format.channels, RING_SECONDS);
        let stop = Arc::new(AtomicBool::new(false));
        let done = Arc::new(AtomicBool::new(false));
        let failed = Arc::new(AtomicBool::new(false));
        let start_ns = now_ns();
        let feeder = {
            let (stop, done, failed) = (stop.clone(), done.clone(), failed.clone());
            let rate = u64::from(format.rate);
            let channels = format.channels.max(1);
            let block = (format.rate as usize / 100).max(1);
            let limit = fail_after_s.map_or(usize::MAX, |s| (s * format.rate as f64) as usize);
            std::thread::spawn(move || {
                let mut frame = 0usize;
                for chunk in samples.chunks(block * channels) {
                    if stop.load(Ordering::Relaxed) {
                        break;
                    }
                    if frame >= limit {
                        failed.store(true, Ordering::Release);
                        break;
                    }
                    let at_ns = start_ns + frame as u64 * 1_000_000_000 / rate;
                    match pace {
                        Pace::Real => {
                            let wait = at_ns.saturating_sub(now_ns());
                            std::thread::sleep(Duration::from_nanos(wait));
                        }
                        Pace::Fast => {
                            while !producer.has_room(chunk.len() / channels) {
                                if stop.load(Ordering::Relaxed) {
                                    return;
                                }
                                std::thread::sleep(Duration::from_millis(1));
                            }
                        }
                    }
                    producer.push(chunk, at_ns);
                    frame += chunk.len() / channels;
                }
                done.store(true, Ordering::Release);
            })
        };
        Self {
            consumer,
            format,
            name: name.into(),
            stop,
            done,
            failed,
            feeder: Some(feeder),
        }
    }

    /// Play a WAV file (any rate, channels and sample format).
    pub fn from_wav(path: &Path, pace: Pace) -> Result<Self, CaptureError> {
        let (format, samples) = read_wav(path)?;
        let name = format!("file {}", path.display());
        Ok(Self::start(name, format, samples, pace, None))
    }
}

/// Interleaved samples of a WAV file as f32.
pub fn read_wav(path: &Path) -> Result<(SourceFormat, Vec<f32>), CaptureError> {
    let mut reader = hound::WavReader::open(path)?;
    let spec = reader.spec();
    let samples = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().collect::<Result<_, _>>()?,
        hound::SampleFormat::Int => {
            let scale = (1u64 << (spec.bits_per_sample - 1)) as f32;
            reader
                .samples::<i32>()
                .map(|sample| sample.map(|value| value as f32 / scale))
                .collect::<Result<_, _>>()?
        }
    };
    let format = SourceFormat {
        rate: spec.sample_rate,
        channels: usize::from(spec.channels.max(1)),
    };
    Ok((format, samples))
}

impl AudioSource for FakeSource {
    fn format(&self) -> SourceFormat {
        self.format
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn drain(&mut self, samples: &mut Vec<f32>, stamps: &mut Vec<Stamp>) {
        self.consumer.drain(samples, stamps);
    }

    fn failed(&self) -> bool {
        self.failed.load(Ordering::Acquire)
    }

    fn finished(&self) -> bool {
        // `done` is set after the last push, so an empty ring afterwards means all was drained.
        self.done.load(Ordering::Acquire) && self.consumer.is_empty()
    }

    fn dropped(&self) -> u64 {
        self.consumer.dropped()
    }
}

impl Drop for FakeSource {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(feeder) = self.feeder.take() {
            let _ = feeder.join();
        }
    }
}
