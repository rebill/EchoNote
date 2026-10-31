//! M0 Spike B probe: record the default microphone and system audio together, then report
//! callback timing, gaps, levels and clock drift between the two sources. System audio is a
//! Core Audio process tap on macOS and WASAPI loopback of the default output on Windows.
//!
//! macOS: run through `scripts/dev/macos_capture_probe.sh` so the permission prompts are
//! attributed to an app bundle. Windows: `cargo run --release -p echonote-core --example
//! capture_probe -- <out_dir> [seconds]`.

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn main() {
    eprintln!("capture_probe supports macOS and Windows only");
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn main() -> anyhow::Result<()> {
    probe::run()
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
mod probe {
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{Duration, Instant};

    use anyhow::{Context, Result};
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    #[cfg(target_os = "macos")]
    use echonote_core::capture::macos_tap::SystemTap;
    use rtrb::{Consumer, Producer, RingBuffer};
    use serde_json::{Value, json};

    const SAMPLE_RING: usize = 1 << 24;
    const TIMING_RING: usize = 1 << 16;

    struct Sink {
        samples: Producer<f32>,
        /// (arrival ns since t0, interleaved samples in the callback)
        timing: Producer<(u64, u32)>,
        dropped: Arc<AtomicU64>,
        t0: Instant,
    }

    impl Sink {
        /// Real-time safe: no allocation, no lock.
        fn push(&mut self, data: &[f32]) {
            let elapsed = self.t0.elapsed().as_nanos() as u64;
            let _ = self.timing.push((elapsed, data.len() as u32));
            match self.samples.write_chunk_uninit(data.len()) {
                Ok(chunk) => {
                    chunk.fill_from_iter(data.iter().copied());
                }
                Err(_) => {
                    self.dropped.fetch_add(data.len() as u64, Ordering::Relaxed);
                }
            }
        }
    }

    struct Drain {
        samples: Consumer<f32>,
        timing: Consumer<(u64, u32)>,
        all_samples: Vec<f32>,
        all_timing: Vec<(u64, u32)>,
    }

    impl Drain {
        fn pull(&mut self) {
            while let Ok(value) = self.samples.pop() {
                self.all_samples.push(value);
            }
            while let Ok(value) = self.timing.pop() {
                self.all_timing.push(value);
            }
        }
    }

    fn channel(t0: Instant) -> (Sink, Drain, Arc<AtomicU64>) {
        let (samples, sample_rx) = RingBuffer::new(SAMPLE_RING);
        let (timing, timing_rx) = RingBuffer::new(TIMING_RING);
        let dropped = Arc::new(AtomicU64::new(0));
        let drain = Drain {
            samples: sample_rx,
            timing: timing_rx,
            all_samples: Vec::new(),
            all_timing: Vec::new(),
        };
        (
            Sink {
                samples,
                timing,
                dropped: dropped.clone(),
                t0,
            },
            drain,
            dropped,
        )
    }

    /// The system-audio source and what was captured from it.
    struct System {
        guard: Box<dyn std::any::Any>,
        drain: Drain,
        dropped: Arc<AtomicU64>,
        rate: f64,
        channels: u32,
        info: Value,
    }

    #[cfg(target_os = "macos")]
    fn start_system(t0: Instant, _host: &cpal::Host) -> Result<System> {
        let (mut sink, drain, dropped) = channel(t0);
        let tap = SystemTap::start(move |data, _host_ns| sink.push(data))?;
        let format = tap.format();
        Ok(System {
            guard: Box::new(tap),
            drain,
            dropped,
            rate: format.sample_rate,
            channels: format.channels.max(1),
            info: json!({
                "kind": "core-audio-process-tap",
                "format_id": format!("{:08x}", format.format_id),
                "format_flags": format.format_flags,
                "bits_per_channel": format.bits_per_channel,
            }),
        })
    }

    /// WASAPI loopback of the default render device. It delivers no packets while nothing plays,
    /// which shows up as gaps and as frames_duration_s falling behind wall_duration_s.
    #[cfg(target_os = "windows")]
    fn start_system(t0: Instant, host: &cpal::Host) -> Result<System> {
        let device = host
            .default_output_device()
            .context("no default output device")?;
        let config = device.default_output_config()?;
        anyhow::ensure!(
            config.sample_format() == cpal::SampleFormat::F32,
            "probe expects f32 loopback samples, device default is {:?}",
            config.sample_format()
        );
        let (mut sink, drain, dropped) = channel(t0);
        let stream = device.build_input_stream(
            config.config(),
            move |data: &[f32], _| sink.push(data),
            |error| eprintln!("loopback stream error: {error}"),
            None,
        )?;
        stream.play()?;
        Ok(System {
            guard: Box::new(stream),
            drain,
            dropped,
            rate: config.sample_rate() as f64,
            channels: config.channels() as u32,
            info: json!({
                "kind": "wasapi-loopback",
                "device": device.description().map(|description| description.to_string()).unwrap_or_default(),
            }),
        })
    }

    pub fn run() -> Result<()> {
        let mut args = std::env::args().skip(1);
        let out_dir = PathBuf::from(
            args.next()
                .context("usage: capture_probe <out_dir> [seconds]")?,
        );
        let seconds: u64 = args
            .next()
            .map(|value| value.parse())
            .transpose()?
            .unwrap_or(30);
        std::fs::create_dir_all(&out_dir)?;
        let t0 = Instant::now();

        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .context("no default input device")?;
        let mic_config = device.default_input_config()?;
        anyhow::ensure!(
            mic_config.sample_format() == cpal::SampleFormat::F32,
            "probe expects f32 mic samples, device default is {:?}",
            mic_config.sample_format()
        );
        let mic_rate = mic_config.sample_rate() as f64;
        let mic_channels = mic_config.channels() as u32;
        let (mut mic_sink, mut mic_drain, mic_dropped) = channel(t0);
        let stream = device.build_input_stream(
            mic_config.config(),
            move |data: &[f32], _| mic_sink.push(data),
            |error| eprintln!("mic stream error: {error}"),
            None,
        )?;
        stream.play()?;

        let (mut system, system_error) = match start_system(t0, &host) {
            Ok(system) => (Some(system), None),
            Err(error) => (None, Some(format!("{error:#}"))),
        };

        eprintln!("recording {seconds}s; play some audio and speak");
        let deadline = t0 + Duration::from_secs(seconds);
        while Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
            mic_drain.pull();
            if let Some(system) = system.as_mut() {
                system.drain.pull();
            }
        }
        drop(stream);
        mic_drain.pull();
        let mic = summarize(
            &mic_drain,
            mic_rate,
            mic_channels,
            mic_dropped.load(Ordering::Relaxed),
        );
        write_wav(
            &out_dir.join("mic.wav"),
            &mic_drain.all_samples,
            mic_rate,
            mic_channels,
        )?;

        let (sys, system_format) = match system {
            Some(System {
                guard,
                mut drain,
                dropped,
                rate,
                channels,
                info,
            }) => {
                drop(guard);
                drain.pull();
                write_wav(&out_dir.join("sys.wav"), &drain.all_samples, rate, channels)?;
                let format = json!({ "sample_rate": rate, "channels": channels, "source": info });
                (
                    summarize(&drain, rate, channels, dropped.load(Ordering::Relaxed)),
                    format,
                )
            }
            None => (Value::Null, Value::Null),
        };

        let report = json!({
            "seconds": seconds,
            "mic_device": device.description().map(|description| description.to_string()).unwrap_or_default(),
            "mic": mic,
            "system_error": system_error,
            "system_format": system_format,
            "sys": sys,
            "relative_drift_ppm": match (mic["drift_ppm"].as_f64(), sys["drift_ppm"].as_f64()) {
                (Some(a), Some(b)) => json!(round(a - b, 1)),
                _ => Value::Null,
            },
        });
        let text = serde_json::to_string_pretty(&report)?;
        std::fs::write(out_dir.join("report.json"), &text)?;
        println!("{text}");
        Ok(())
    }

    /// Callback statistics plus the sample rate fitted from arrival times against frame counts.
    fn summarize(drain: &Drain, nominal_rate: f64, channels: u32, dropped: u64) -> Value {
        let timing = &drain.all_timing;
        if timing.len() < 10 {
            return json!({ "callbacks": timing.len(), "samples": drain.all_samples.len() });
        }
        let seconds: Vec<f64> = timing.iter().map(|(ns, _)| *ns as f64 / 1e9).collect();
        let mut cumulative = Vec::with_capacity(timing.len());
        let mut total = 0f64;
        for (_, samples) in timing {
            total += (*samples / channels) as f64;
            cumulative.push(total);
        }
        let n = seconds.len() as f64;
        let (mean_t, mean_f) = (
            seconds.iter().sum::<f64>() / n,
            cumulative.iter().sum::<f64>() / n,
        );
        let (mut covariance, mut variance) = (0.0, 0.0);
        for (t, f) in seconds.iter().zip(&cumulative) {
            covariance += (t - mean_t) * (f - mean_f);
            variance += (t - mean_t).powi(2);
        }
        let fitted_rate = covariance / variance;
        let gaps: Vec<f64> = seconds
            .windows(2)
            .map(|pair| (pair[1] - pair[0]) * 1000.0)
            .collect();
        let mean_period_ms = gaps.iter().sum::<f64>() / gaps.len() as f64;
        let max_gap_ms = gaps.iter().copied().fold(0.0, f64::max);
        let late = gaps
            .iter()
            .filter(|gap| **gap > 2.0 * mean_period_ms + 5.0)
            .count();

        let mut offset = 0usize;
        let mut zero_buffers = 0usize;
        for (_, samples) in timing {
            let len = *samples as usize;
            let end = (offset + len).min(drain.all_samples.len());
            if drain.all_samples[offset..end]
                .iter()
                .all(|sample| *sample == 0.0)
            {
                zero_buffers += 1;
            }
            offset = end;
        }
        let power = drain
            .all_samples
            .iter()
            .map(|sample| (*sample as f64).powi(2))
            .sum::<f64>()
            / drain.all_samples.len().max(1) as f64;
        json!({
            "callbacks": timing.len(),
            "frames": total as u64,
            "channels": channels,
            "nominal_rate": nominal_rate,
            "fitted_rate": round(fitted_rate, 2),
            "drift_ppm": round((fitted_rate - nominal_rate) / nominal_rate * 1e6, 1),
            "startup_ms": round(seconds[0] * 1000.0, 1),
            "frames_duration_s": round(total / nominal_rate, 3),
            "wall_duration_s": round(seconds[seconds.len() - 1] - seconds[0], 3),
            "mean_period_ms": round(mean_period_ms, 2),
            "max_gap_ms": round(max_gap_ms, 1),
            "late_callbacks": late,
            "zero_buffers": zero_buffers,
            "rms_dbfs": round(10.0 * (power + 1e-12).log10(), 1),
            "dropped_samples": dropped,
        })
    }

    fn write_wav(path: &std::path::Path, samples: &[f32], rate: f64, channels: u32) -> Result<()> {
        let spec = hound::WavSpec {
            channels: channels as u16,
            sample_rate: rate as u32,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };
        let mut writer = hound::WavWriter::create(path, spec)?;
        for sample in samples {
            writer.write_sample(*sample)?;
        }
        writer.finalize()?;
        Ok(())
    }

    fn round(value: f64, digits: i32) -> f64 {
        let scale = 10f64.powi(digits);
        (value * scale).round() / scale
    }
}
