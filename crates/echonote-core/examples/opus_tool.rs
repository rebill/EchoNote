//! Development tool for recording files (`echonote_core::recording`).
//!
//! cargo run --release -p echonote-core --example opus_tool -- <command>
//!
//! roundtrip <manifest.jsonl> <out_dir> [complexity] [bitrate]
//!     Encode every 16 kHz mono WAV of an eval manifest to Ogg/Opus, decode it back and write
//!     it next to a copy of the manifest, so `echonote-bench eval` can score ASR on recorded
//!     audio. Prints encode and decode speed and the bitrate.
//! probe <file.opus>...
//!     Duration, whether the writer finished, and any torn tail (what crash recovery sees).
//! decode <file.opus> <out.wav> [start_s] [end_s]
//!     Decode a range of the track timeline to a 16-bit WAV.

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail, ensure};
use echonote_core::SAMPLE_RATE;
use echonote_core::recording::{Writer, WriterConfig, decode_range, probe};

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("roundtrip") => roundtrip(args),
        Some("probe") => {
            for path in args {
                let probe = probe(Path::new(&path))?;
                println!(
                    "{path}: {:.3} s, {}, {} of {} bytes valid",
                    probe.samples as f64 / SAMPLE_RATE as f64,
                    if probe.finished {
                        "finished"
                    } else {
                        "unfinished"
                    },
                    probe.valid_len,
                    probe.file_len,
                );
            }
            Ok(())
        }
        Some("decode") => {
            let usage = "usage: opus_tool decode <file.opus> <out.wav> [start_s] [end_s]";
            let input = PathBuf::from(args.next().context(usage)?);
            let output = PathBuf::from(args.next().context(usage)?);
            let mut seconds = args.map(|value| value.parse::<f64>());
            let start = seconds.next().transpose()?.unwrap_or(0.0);
            let end = seconds.next().transpose()?;
            let at = |s: f64| (s * SAMPLE_RATE as f64).round() as u64;
            let samples = decode_range(&input, at(start), end.map_or(u64::MAX, at))?;
            write_wav(&output, &samples)?;
            println!("{} samples to {}", samples.len(), output.display());
            Ok(())
        }
        _ => bail!("usage: opus_tool roundtrip|probe|decode … (see the file header)"),
    }
}

fn read_wav(path: &Path) -> Result<Vec<f32>> {
    let mut wav = hound::WavReader::open(path).with_context(|| format!("{}", path.display()))?;
    let spec = wav.spec();
    ensure!(
        spec.sample_rate == SAMPLE_RATE && spec.channels == 1,
        "{}: need 16 kHz mono",
        path.display()
    );
    Ok(match spec.sample_format {
        hound::SampleFormat::Int => {
            let scale = (1u32 << (spec.bits_per_sample - 1)) as f32;
            wav.samples::<i32>()
                .map(|s| s.map(|s| s as f32 / scale))
                .collect::<Result<_, _>>()?
        }
        hound::SampleFormat::Float => wav.samples::<f32>().collect::<Result<_, _>>()?,
    })
}

fn write_wav(path: &Path, samples: &[f32]) -> Result<()> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut wav = hound::WavWriter::create(path, spec)?;
    for sample in samples {
        wav.write_sample((sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)?;
    }
    wav.finalize()?;
    Ok(())
}

fn roundtrip(mut args: impl Iterator<Item = String>) -> Result<()> {
    let usage = "usage: opus_tool roundtrip <manifest.jsonl> <out_dir> [complexity] [bitrate]";
    let manifest = PathBuf::from(args.next().context(usage)?);
    let out_dir = PathBuf::from(args.next().context(usage)?);
    let defaults = WriterConfig::default();
    let config = WriterConfig {
        complexity: args
            .next()
            .map(|v| v.parse())
            .transpose()?
            .unwrap_or(defaults.complexity),
        bitrate: args
            .next()
            .map(|v| v.parse())
            .transpose()?
            .unwrap_or(defaults.bitrate),
        ..defaults
    };
    let base = manifest.parent().context("manifest has no parent")?;
    std::fs::create_dir_all(out_dir.join("wav"))?;
    let scratch = out_dir.join("scratch.opus");

    let mut out = std::io::BufWriter::new(std::fs::File::create(out_dir.join("manifest.jsonl"))?);
    let (mut audio_s, mut bytes, mut files) = (0.0, 0u64, 0);
    let (mut encode, mut decode) = (Duration::ZERO, Duration::ZERO);
    for line in std::io::BufReader::new(std::fs::File::open(&manifest)?).lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let mut entry: serde_json::Value = serde_json::from_str(&line)?;
        let audio = entry["audio"]
            .as_str()
            .context("entry without audio")?
            .to_owned();
        let samples = read_wav(&base.join(&audio))?;

        let _ = std::fs::remove_file(&scratch);
        let started = Instant::now();
        let mut writer = Writer::create(&scratch, config)?;
        writer.push(&samples)?;
        writer.finish()?;
        encode += started.elapsed();
        bytes += std::fs::metadata(&scratch)?.len();
        let started = Instant::now();
        let decoded = decode_range(&scratch, 0, u64::MAX)?;
        decode += started.elapsed();
        ensure!(decoded.len() == samples.len(), "{audio}: length changed");

        let name = format!("wav/{files:05}.wav");
        write_wav(&out_dir.join(&name), &decoded)?;
        entry["audio"] = name.into();
        writeln!(out, "{entry}")?;
        audio_s += samples.len() as f64 / SAMPLE_RATE as f64;
        files += 1;
    }
    out.flush()?;
    let _ = std::fs::remove_file(&scratch);

    println!(
        "{files} files, {audio_s:.0} s of audio; complexity {}, target {} bit/s\n\
         encode {:.0}x real time ({:.2}% of a core), decode {:.0}x\n\
         {:.1} kbit/s including Ogg overhead ({:.1} MB per hour)",
        config.complexity,
        config.bitrate,
        audio_s / encode.as_secs_f64(),
        100.0 * encode.as_secs_f64() / audio_s,
        audio_s / decode.as_secs_f64(),
        bytes as f64 * 8.0 / audio_s / 1_000.0,
        bytes as f64 / audio_s * 3_600.0 / 1e6,
    );
    Ok(())
}
