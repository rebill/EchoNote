//! Replay a dual-track recording (`mic16.wav`, `sys16.wav`) through the engine with the real
//! SenseVoice model and Silero VAD, and print the stored transcript.
//!
//! Usage: engine_replay <dir> [real|fast]
//! Models come from `$ECHONOTE_DEV_DATA/models` (`scripts/dev/fetch_assets.py`).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use echonote_core::asr::ModelKind;
use echonote_core::capture::{AudioSource, FakeSource, Pace, read_wav};
use echonote_core::engine::{
    Engine, EngineConfig, EngineEvent, OpenSource, SessionOptions, SherpaModel,
};
use echonote_core::store::Crosstalk;
use echonote_core::vad::{VadKind, VadSpec};

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let dir = PathBuf::from(
        args.next()
            .context("usage: engine_replay <dir> [real|fast]")?,
    );
    let pace = match args.next().as_deref() {
        Some("fast") => Pace::Fast,
        _ => Pace::Real,
    };
    let dev = std::env::var_os("ECHONOTE_DEV_DATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let home = std::env::var_os("HOME").unwrap_or_default();
            PathBuf::from(home).join(".cache/echonote-dev")
        });
    let models = dev.join("models");
    let model = SherpaModel::new(
        ModelKind::SenseVoice,
        models.join("sherpa-onnx-sense-voice-zh-en-ja-ko-yue-int8-2025-09-09"),
    );
    let vad = VadSpec::meeting(VadKind::Silero, models.join("vad/silero_vad.onnx"));
    let data = std::env::temp_dir().join(format!("echonote-replay-{}", std::process::id()));
    let config = EngineConfig::new(data.clone(), Arc::new(model), EngineConfig::sherpa_vad(vad));
    let (engine, events) = Engine::open(config)?;

    let open = |name: &str| -> Result<OpenSource> {
        let (format, samples) = read_wav(&dir.join(name))?;
        let mut samples = Some(samples);
        let label = name.to_owned();
        Ok(Box::new(move || {
            let audio = samples.take().unwrap_or_default();
            Ok(
                Box::new(FakeSource::start(label.clone(), format, audio, pace, None))
                    as Box<dyn AudioSource>,
            )
        }))
    };
    let started = Instant::now();
    let meeting = engine.start(SessionOptions {
        title: "replay".into(),
        language: None,
        mic: Some(open("mic16.wav")?),
        sys: Some(open("sys16.wav")?),
    })?;
    loop {
        match events.recv_timeout(Duration::from_secs(600))? {
            EngineEvent::Segment { segment, .. } => {
                let speaker = if segment.track.as_str() == "mic" {
                    "我  "
                } else {
                    "对方"
                };
                let mark = match segment.crosstalk {
                    Some(Crosstalk::Duplicate) => "[串音] ",
                    Some(Crosstalk::Trim) => "[裁剪] ",
                    None => "",
                };
                println!(
                    "{:6.2}s {speaker} {mark}{}",
                    segment.start_ms as f64 / 1_000.0,
                    segment.text
                );
            }
            EngineEvent::Notice { notice, .. } => eprintln!("notice: {notice:?}"),
            EngineEvent::Finished { stats, .. } => {
                eprintln!(
                    "finished in {:.1} s: {stats:?}",
                    started.elapsed().as_secs_f64()
                );
                break;
            }
            _ => {}
        }
    }
    engine.stop()?;
    let reader = engine.store().reader()?;
    eprintln!("meeting {meeting}: {:?}", reader.meeting(meeting)?);
    drop(engine);
    let _ = std::fs::remove_dir_all(&data);
    Ok(())
}
