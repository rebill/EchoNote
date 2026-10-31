//! Microphone capture with cpal in the device's native format.
//!
//! cpal does not follow the system default input device, so with no device chosen the
//! source checks about once a second whether the default changed and then reports itself
//! failed; the session opens a new source and the timeline fills the gap.

use std::cell::Cell;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample};

use super::ring::{RingConsumer, RingProducer, now_ns, ring};
use super::{AudioSource, CaptureError, RING_SECONDS, SourceFormat, Stamp};

const DEFAULT_POLL: Duration = Duration::from_secs(1);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceInfo {
    /// Stable id to store in settings (cpal `DeviceId` display form).
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

/// Input devices of the default host.
pub fn input_devices() -> Result<Vec<DeviceInfo>, CaptureError> {
    let host = cpal::default_host();
    let default = host
        .default_input_device()
        .and_then(|device| device.id().ok())
        .map(|id| id.to_string());
    let devices = host.input_devices().map_err(device_error)?;
    Ok(devices
        .filter_map(|device| {
            let id = device.id().ok()?.to_string();
            Some(DeviceInfo {
                is_default: default.as_deref() == Some(id.as_str()),
                name: device_name(&device),
                id,
            })
        })
        .collect())
}

pub struct MicSource {
    _stream: cpal::Stream,
    consumer: RingConsumer,
    format: SourceFormat,
    name: String,
    /// The default device's id when following the default, else `None`.
    follows: Option<String>,
    host: cpal::Host,
    error: Arc<AtomicBool>,
    checked: Cell<Instant>,
    replaced: Cell<bool>,
}

impl MicSource {
    /// Open `device_id`, or the system default input (followed when it changes).
    pub fn open(device_id: Option<&str>) -> Result<Self, CaptureError> {
        if super::mic_permission() == super::Permission::Denied {
            return Err(CaptureError::Denied);
        }
        let host = cpal::default_host();
        let device = match device_id {
            Some(id) => host
                .input_devices()
                .map_err(device_error)?
                .find(|device| device.id().is_ok_and(|found| found.to_string() == id))
                .ok_or(CaptureError::NoDevice("selected microphone"))?,
            None => host
                .default_input_device()
                .ok_or(CaptureError::NoDevice("input"))?,
        };
        let follows = match device_id {
            Some(_) => None,
            None => Some(device.id().map(|id| id.to_string()).unwrap_or_default()),
        };
        let supported = device.default_input_config().map_err(device_error)?;
        let format = SourceFormat {
            rate: supported.sample_rate(),
            channels: usize::from(supported.channels().max(1)),
        };
        let (producer, consumer) = ring(format.rate, format.channels, RING_SECONDS);
        let error = Arc::new(AtomicBool::new(false));
        let config = supported.config();
        let stream = match supported.sample_format() {
            SampleFormat::F32 => build::<f32>(&device, config, producer, &error),
            SampleFormat::I16 => build::<i16>(&device, config, producer, &error),
            SampleFormat::I32 => build::<i32>(&device, config, producer, &error),
            SampleFormat::U16 => build::<u16>(&device, config, producer, &error),
            other => return Err(CaptureError::Format(format!("{other:?}"))),
        }?;
        stream.play().map_err(device_error)?;
        Ok(Self {
            _stream: stream,
            consumer,
            format,
            name: device_name(&device),
            follows,
            host,
            error,
            checked: Cell::new(Instant::now()),
            replaced: Cell::new(false),
        })
    }
}

fn build<T>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    mut producer: RingProducer,
    error: &Arc<AtomicBool>,
) -> Result<cpal::Stream, CaptureError>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let error = error.clone();
    device
        .build_input_stream(
            config,
            move |data: &[T], info: &cpal::InputCallbackInfo| {
                let timestamp = info.timestamp();
                // Age of the first frame: capture happened before the callback.
                let age = timestamp
                    .callback
                    .saturating_duration_since(timestamp.capture)
                    .as_nanos() as u64;
                let at_ns = now_ns().saturating_sub(age);
                producer.push_iter(
                    data.len(),
                    data.iter()
                        .map(|&sample| <f32 as FromSample<T>>::from_sample_(sample)),
                    at_ns,
                );
            },
            move |_| error.store(true, Ordering::Relaxed),
            None,
        )
        .map_err(device_error)
}

fn device_name(device: &cpal::Device) -> String {
    device
        .description()
        .map(|description| description.to_string())
        .unwrap_or_else(|_| "microphone".into())
}

fn device_error(error: impl std::fmt::Display) -> CaptureError {
    CaptureError::Device(error.to_string())
}

impl AudioSource for MicSource {
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
        if self.error.load(Ordering::Relaxed) || self.replaced.get() {
            return true;
        }
        let Some(followed) = &self.follows else {
            return false;
        };
        if self.checked.get().elapsed() >= DEFAULT_POLL {
            self.checked.set(Instant::now());
            let current = self
                .host
                .default_input_device()
                .and_then(|device| device.id().ok())
                .map(|id| id.to_string());
            if current.as_ref() != Some(followed) {
                self.replaced.set(true);
            }
        }
        self.replaced.get()
    }

    fn dropped(&self) -> u64 {
        self.consumer.dropped()
    }
}
