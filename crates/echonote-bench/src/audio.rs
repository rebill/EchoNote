use std::path::Path;

use anyhow::{Context, Result};
use echonote_core::SAMPLE_RATE;
use sherpa_onnx::LinearResampler;

/// Load a WAV file as 16 kHz mono f32 samples.
pub fn load_wav(path: &Path) -> Result<Vec<f32>> {
    let mut reader =
        hound::WavReader::open(path).with_context(|| format!("open {}", path.display()))?;
    let spec = reader.spec();
    let channels = spec.channels as usize;
    let interleaved: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().collect::<Result<_, _>>()?,
        hound::SampleFormat::Int => {
            let scale = 1.0 / (1_i64 << (spec.bits_per_sample - 1)) as f32;
            reader
                .samples::<i32>()
                .map(|sample| sample.map(|value| value as f32 * scale))
                .collect::<Result<_, _>>()?
        }
    };
    let mono: Vec<f32> = if channels == 1 {
        interleaved
    } else {
        interleaved
            .chunks_exact(channels)
            .map(|frame| frame.iter().sum::<f32>() / channels as f32)
            .collect()
    };
    if spec.sample_rate == SAMPLE_RATE {
        return Ok(mono);
    }
    let resampler = LinearResampler::create(spec.sample_rate as i32, SAMPLE_RATE as i32)
        .context("create resampler")?;
    Ok(resampler.resample(&mono, true))
}

pub fn seconds(samples: usize) -> f64 {
    samples as f64 / SAMPLE_RATE as f64
}
