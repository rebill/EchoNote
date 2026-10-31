//! System audio on macOS: the process tap feeding a ring.

use super::macos_tap::SystemTap;
use super::ring::{RingConsumer, now_ns, ring};
use super::{AudioSource, CaptureError, RING_SECONDS, SourceFormat, Stamp};

pub struct SystemSource {
    tap: SystemTap,
    consumer: RingConsumer,
    format: SourceFormat,
}

impl SystemSource {
    pub fn start() -> Result<Self, CaptureError> {
        let mut tap = SystemTap::open()?;
        let native = tap.format();
        let format = SourceFormat {
            rate: native.sample_rate.round() as u32,
            channels: native.channels.max(1) as usize,
        };
        if format.rate == 0 {
            return Err(CaptureError::Format("tap reports 0 Hz".into()));
        }
        let (mut producer, consumer) = ring(format.rate, format.channels, RING_SECONDS);
        tap.start(move |data, age_ns| producer.push(data, now_ns().saturating_sub(age_ns)))?;
        Ok(Self {
            tap,
            consumer,
            format,
        })
    }
}

impl AudioSource for SystemSource {
    fn format(&self) -> SourceFormat {
        self.format
    }

    fn name(&self) -> &str {
        "Core Audio process tap"
    }

    fn drain(&mut self, samples: &mut Vec<f32>, stamps: &mut Vec<Stamp>) {
        self.consumer.drain(samples, stamps);
    }

    fn failed(&self) -> bool {
        self.tap.changed()
    }

    fn dropped(&self) -> u64 {
        self.consumer.dropped()
    }
}
