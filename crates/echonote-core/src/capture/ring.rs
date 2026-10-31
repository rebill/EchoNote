//! Lock-free hand-off from an audio callback to the capture thread.
//!
//! Each callback pushes its whole buffer into a sample ring and one [`Stamp`] (where the
//! buffer starts in the source stream and when it was captured) into a small stamp ring. Both
//! are rtrb SPSC rings allocated up front: the callback does no allocation, takes no lock and
//! makes no system call. When the sample ring is full the buffer is dropped whole and only
//! counted; the stamps of later buffers show the gap, and the timeline fills it with silence.

use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use rtrb::{Consumer, Producer, RingBuffer};

/// Where a callback buffer starts in its stream and when its first frame was captured.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stamp {
    /// Index of the buffer's first frame in the source stream (dropped buffers included).
    pub frame: u64,
    /// Frames in the buffer.
    pub frames: u32,
    /// Capture time of the first frame, [`now_ns`] clock.
    pub at_ns: u64,
}

static EPOCH: OnceLock<Instant> = OnceLock::new();

/// Fix the clock origin. Call before any audio callback can run, so the callbacks only read
/// an initialized value.
pub fn init_clock() {
    EPOCH.get_or_init(Instant::now);
}

/// Monotonic nanoseconds since [`init_clock`]: the clock every capture timestamp uses.
/// Reading it is real-time safe (`mach_absolute_time` on macOS, QPC on Windows).
pub fn now_ns() -> u64 {
    let epoch = *EPOCH.get_or_init(Instant::now);
    epoch.elapsed().as_nanos() as u64
}

/// The callback side.
pub struct RingProducer {
    samples: Producer<f32>,
    stamps: Producer<Stamp>,
    channels: usize,
    frame: u64,
    dropped: Arc<AtomicU64>,
}

impl RingProducer {
    /// Push one callback buffer of interleaved samples captured at `at_ns`.
    pub fn push(&mut self, data: &[f32], at_ns: u64) {
        self.push_iter(data.len(), data.iter().copied(), at_ns);
    }

    /// Push `len` interleaved samples from an iterator (for sample-format conversion without
    /// a temporary buffer).
    pub fn push_iter(&mut self, len: usize, data: impl Iterator<Item = f32>, at_ns: u64) {
        let frames = (len / self.channels.max(1)) as u64;
        if frames == 0 {
            return;
        }
        let stamp = Stamp {
            frame: self.frame,
            frames: frames as u32,
            at_ns,
        };
        self.frame += frames;
        let len = frames as usize * self.channels;
        if self.stamps.slots() == 0 {
            self.dropped.fetch_add(frames, Ordering::Relaxed);
            return;
        }
        match self.samples.write_chunk_uninit(len) {
            Ok(chunk) => {
                chunk.fill_from_iter(data.take(len));
                let _ = self.stamps.push(stamp);
            }
            Err(_) => {
                self.dropped.fetch_add(frames, Ordering::Relaxed);
            }
        }
    }

    /// Whether a buffer of `frames` would fit now (for sources that can wait instead of drop).
    pub fn has_room(&self, frames: usize) -> bool {
        self.stamps.slots() > 0 && self.samples.slots() >= frames * self.channels
    }
}

/// The capture-thread side.
pub struct RingConsumer {
    samples: Consumer<f32>,
    stamps: Consumer<Stamp>,
    dropped: Arc<AtomicU64>,
}

impl RingConsumer {
    /// Move everything available into `samples` and `stamps`. Stamps are read first, so every
    /// returned stamp's samples are in `samples` (now or from an earlier call); samples whose
    /// stamp is not visible yet may also be returned and are described by a later call.
    pub fn drain(&mut self, samples: &mut Vec<f32>, stamps: &mut Vec<Stamp>) {
        while let Ok(stamp) = self.stamps.pop() {
            stamps.push(stamp);
        }
        let available = self.samples.slots();
        if available == 0 {
            return;
        }
        if let Ok(chunk) = self.samples.read_chunk(available) {
            let (first, second) = chunk.as_slices();
            samples.extend_from_slice(first);
            samples.extend_from_slice(second);
            chunk.commit_all();
        }
    }

    /// Frames dropped because the ring was full.
    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    pub fn is_empty(&self) -> bool {
        self.stamps.is_empty() && self.samples.is_empty()
    }
}

/// A ring for `seconds` of audio at `rate` and `channels`, plus room for the stamps of one
/// callback per millisecond over the same time.
pub fn ring(rate: u32, channels: usize, seconds: f64) -> (RingProducer, RingConsumer) {
    let samples = ((rate as f64 * seconds) as usize * channels.max(1)).max(1 << 12);
    let stamps = ((seconds * 1_000.0) as usize).max(256);
    let (sample_tx, sample_rx) = RingBuffer::new(samples);
    let (stamp_tx, stamp_rx) = RingBuffer::new(stamps);
    let dropped = Arc::new(AtomicU64::new(0));
    (
        RingProducer {
            samples: sample_tx,
            stamps: stamp_tx,
            channels: channels.max(1),
            frame: 0,
            dropped: dropped.clone(),
        },
        RingConsumer {
            samples: sample_rx,
            stamps: stamp_rx,
            dropped,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_rings_drop_whole_buffers_and_keep_frame_numbers() {
        let (mut tx, mut rx) = ring(1_000, 2, 8.0); // 16,000 samples
        let buffer = vec![0.5f32; 2 * 3_000];
        tx.push(&buffer, 1);
        tx.push(&buffer, 2);
        tx.push(&buffer, 3); // does not fit
        let (mut samples, mut stamps) = (Vec::new(), Vec::new());
        rx.drain(&mut samples, &mut stamps);
        assert_eq!(samples.len(), 12_000);
        assert_eq!(rx.dropped(), 3_000);
        assert_eq!(
            stamps,
            vec![
                Stamp {
                    frame: 0,
                    frames: 3_000,
                    at_ns: 1
                },
                Stamp {
                    frame: 3_000,
                    frames: 3_000,
                    at_ns: 2
                },
            ]
        );
        tx.push(&buffer[..20], 4);
        let (mut samples, mut stamps) = (Vec::new(), Vec::new());
        rx.drain(&mut samples, &mut stamps);
        assert_eq!(stamps[0].frame, 9_000, "the dropped buffer still counts");
        assert_eq!(samples.len(), 20);
    }
}
