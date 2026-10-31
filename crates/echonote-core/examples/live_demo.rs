//! Live transcription demo in the terminal: the microphone is "我", system audio is "对方".
//!
//! Drives `echonote_core::pipeline` the way the app will: per-track Silero VAD with a 12 s
//! hard cap, one SenseVoice worker serving both tracks (finals first, one preview slot shared
//! by both tracks with system audio first). Mic finals are compared with system-audio tokens
//! of the same moment (`echonote_core::crosstalk`): bleed is hidden, and bleed inside the
//! user's own sentence is trimmed.
//!
//! macOS: run through `scripts/dev/live_demo.sh`. Writes mic.wav, sys.wav (16 kHz) and
//! transcript.jsonl to `<out_dir>` and stops when `<out_dir>/stop` appears. Windows loopback
//! sends nothing while the output is silent, so system-audio timestamps drift there.

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn main() {
    eprintln!("live_demo supports macOS and Windows only");
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn main() -> anyhow::Result<()> {
    demo::run()
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
mod demo {
    use std::collections::VecDeque;
    use std::fs::File;
    use std::io::{BufWriter, Write};
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
    use std::time::{Duration, Instant};

    use anyhow::{Context, Result};
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use echonote_core::asr::{ModelKind, OfflineAsr, OfflineModelSpec, Transcript};
    use echonote_core::crosstalk::{CrosstalkConfig, Reference, Verdict, Word};
    use echonote_core::pipeline::{
        Job, Output, Pipeline, PipelineConfig, SherpaDetector, SpeechDetector, TimedToken,
    };
    use echonote_core::vad::{VadKind, VadSpec};
    use rtrb::{Consumer, RingBuffer};
    use serde_json::json;
    use sherpa_onnx::LinearResampler;

    const SR: usize = 16_000;
    const RING: usize = 1 << 22;
    const HARD_CAP_S: f64 = 12.0;
    /// SenseVoice has token timestamps, so forced cuts can overlap.
    const OVERLAP_S: f64 = 0.2;
    /// Capture is drained this often; decoder results wake the loop sooner.
    const TICK: Duration = Duration::from_millis(30);
    /// How long a mic final may wait for the system track to confirm or rule out crosstalk.
    const MIC_WAIT: Duration = Duration::from_secs(3);
    /// System tokens are kept this long for mic finals still waiting in the queue.
    const REFERENCE_S: f64 = 120.0;
    const MIC: usize = 0;
    const SYS: usize = 1;

    /// One capture source: an audio callback feeding a lock-free ring.
    struct Source {
        _guard: Box<dyn std::any::Any>,
        samples: Consumer<f32>,
        dropped: Arc<AtomicU64>,
        rate: u32,
        channels: usize,
        name: String,
    }

    /// Real-time safe push: no allocation, no lock; overflow is counted, not blocked on.
    fn ring() -> (
        impl FnMut(&[f32]) + Send + 'static,
        Consumer<f32>,
        Arc<AtomicU64>,
    ) {
        let (mut producer, consumer) = RingBuffer::new(RING);
        let dropped = Arc::new(AtomicU64::new(0));
        let counter = dropped.clone();
        let push = move |data: &[f32]| match producer.write_chunk_uninit(data.len()) {
            Ok(chunk) => {
                chunk.fill_from_iter(data.iter().copied());
            }
            Err(_) => {
                counter.fetch_add(data.len() as u64, Ordering::Relaxed);
            }
        };
        (push, consumer, dropped)
    }

    fn start_mic(host: &cpal::Host) -> Result<Source> {
        let device = host
            .default_input_device()
            .context("no default input device")?;
        let config = device.default_input_config()?;
        anyhow::ensure!(
            config.sample_format() == cpal::SampleFormat::F32,
            "expected f32 mic samples, device default is {:?}",
            config.sample_format()
        );
        let (mut push, samples, dropped) = ring();
        let stream = device.build_input_stream(
            config.config(),
            move |data: &[f32], _| push(data),
            |error| eprintln!("mic stream error: {error}"),
            None,
        )?;
        stream.play()?;
        Ok(Source {
            _guard: Box::new(stream),
            samples,
            dropped,
            rate: config.sample_rate(),
            channels: config.channels() as usize,
            name: device
                .description()
                .map(|description| description.to_string())
                .unwrap_or_default(),
        })
    }
    #[cfg(target_os = "macos")]
    fn start_system(_host: &cpal::Host) -> Result<Source> {
        use echonote_core::capture::macos_tap::SystemTap;
        let (mut push, samples, dropped) = ring();
        let tap = SystemTap::start(move |data, _host_ns| push(data))?;
        let format = tap.format();
        Ok(Source {
            _guard: Box::new(tap),
            samples,
            dropped,
            rate: format.sample_rate as u32,
            channels: format.channels.max(1) as usize,
            name: "Core Audio process tap".into(),
        })
    }

    #[cfg(target_os = "windows")]
    fn start_system(host: &cpal::Host) -> Result<Source> {
        let device = host
            .default_output_device()
            .context("no default output device")?;
        let config = device.default_output_config()?;
        anyhow::ensure!(
            config.sample_format() == cpal::SampleFormat::F32,
            "expected f32 loopback samples, device default is {:?}",
            config.sample_format()
        );
        let (mut push, samples, dropped) = ring();
        let stream = device.build_input_stream(
            config.config(),
            move |data: &[f32], _| push(data),
            |error| eprintln!("loopback stream error: {error}"),
            None,
        )?;
        stream.play()?;
        Ok(Source {
            _guard: Box::new(stream),
            samples,
            dropped,
            rate: config.sample_rate(),
            channels: config.channels() as usize,
            name: format!(
                "WASAPI loopback: {}",
                device
                    .description()
                    .map(|description| description.to_string())
                    .unwrap_or_default()
            ),
        })
    }

    /// Development source: plays a recorded WAV into the ring at real-time pace
    /// (`ECHONOTE_DEMO_REPLAY=<dir with mic16.wav and sys16.wav>`).
    fn replay(path: &Path) -> Result<Source> {
        let mut reader =
            hound::WavReader::open(path).with_context(|| format!("open {}", path.display()))?;
        let spec = reader.spec();
        let samples: Vec<f32> = match spec.sample_format {
            hound::SampleFormat::Float => reader.samples::<f32>().collect::<Result<_, _>>()?,
            hound::SampleFormat::Int => {
                let scale = (1u32 << (spec.bits_per_sample - 1)) as f32;
                reader
                    .samples::<i32>()
                    .map(|sample| sample.map(|value| value as f32 / scale))
                    .collect::<Result<_, _>>()?
            }
        };
        let (mut push, consumer, dropped) = ring();
        let block = spec.sample_rate as usize / 100 * spec.channels as usize;
        std::thread::spawn(move || {
            let started = Instant::now();
            for (index, chunk) in samples.chunks(block).enumerate() {
                let due = started + Duration::from_millis(10 * index as u64);
                std::thread::sleep(due.saturating_duration_since(Instant::now()));
                push(chunk);
            }
        });
        Ok(Source {
            _guard: Box::new(()),
            samples: consumer,
            dropped,
            rate: spec.sample_rate,
            channels: spec.channels as usize,
            name: format!("replay {}", path.display()),
        })
    }

    enum Event {
        Loaded(f64),
        LoadFailed(String),
        Decoded {
            job: Job,
            transcript: Transcript,
            decode_s: f64,
        },
    }

    /// The single decoder thread. The pipeline lives on the main thread and hands it one job
    /// at a time; the job travels back with its transcript.
    fn worker(spec: OfflineModelSpec, jobs: Receiver<Job>, events: Sender<Event>) {
        let started = Instant::now();
        let asr = match OfflineAsr::load(&spec) {
            Ok(asr) => asr,
            Err(error) => {
                let _ = events.send(Event::LoadFailed(error.to_string()));
                return;
            }
        };
        let _ = events.send(Event::Loaded(started.elapsed().as_secs_f64()));
        for job in jobs {
            let started = Instant::now();
            let transcript = asr.decode_transcript(&job.samples);
            let decode_s = started.elapsed().as_secs_f64();
            let event = Event::Decoded {
                job,
                transcript,
                decode_s,
            };
            if events.send(event).is_err() {
                return;
            }
        }
    }

    /// One capture track on its own 16 kHz sample clock.
    struct Track {
        source: Source,
        resampler: Option<LinearResampler>,
        /// Samples received so far.
        received: usize,
        /// Session time of sample 0, set when the first samples arrive.
        offset_s: Option<f64>,
        /// Latest preview of the current utterance.
        preview: String,
        preview_utterance: u64,
        wav: hound::WavWriter<BufWriter<File>>,
    }

    impl Track {
        fn new(source: Source, wav: &Path) -> Result<Self> {
            let resampler = if source.rate as usize == SR {
                None
            } else {
                Some(
                    LinearResampler::create(source.rate as i32, SR as i32)
                        .context("create resampler")?,
                )
            };
            let wav_spec = hound::WavSpec {
                channels: 1,
                sample_rate: SR as u32,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            };
            Ok(Self {
                source,
                resampler,
                received: 0,
                offset_s: None,
                preview: String::new(),
                preview_utterance: 0,
                wav: hound::WavWriter::create(wav, wav_spec)?,
            })
        }

        fn seconds_at(&self, sample: usize) -> f64 {
            self.offset_s.unwrap_or(0.0) + sample as f64 / SR as f64
        }

        /// Drain the ring, downmix and resample to 16 kHz.
        fn pull(&mut self, session: Instant) -> Vec<f32> {
            let channels = self.source.channels;
            let available = self.source.samples.slots() / channels * channels;
            if available == 0 {
                return Vec::new();
            }
            let Ok(chunk) = self.source.samples.read_chunk(available) else {
                return Vec::new();
            };
            let (first, second) = chunk.as_slices();
            let interleaved: Vec<f32> = first.iter().chain(second).copied().collect();
            chunk.commit_all();
            let mono: Vec<f32> = if channels == 1 {
                interleaved
            } else {
                interleaved
                    .chunks_exact(channels)
                    .map(|frame| frame.iter().sum::<f32>() / channels as f32)
                    .collect()
            };
            if self.offset_s.is_none() {
                let captured = Duration::from_secs_f64(mono.len() as f64 / self.source.rate as f64);
                let elapsed = session.elapsed().saturating_sub(captured);
                self.offset_s = Some(elapsed.as_secs_f64());
            }
            match &self.resampler {
                Some(resampler) => resampler.resample(&mono, false),
                None => mono,
            }
        }

        /// Save 16 kHz samples and count them.
        fn save(&mut self, samples: &[f32]) -> Result<()> {
            for sample in samples {
                self.wav
                    .write_sample((sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)?;
            }
            self.received += samples.len();
            Ok(())
        }
    }

    /// The pipeline and its tracks, owned by the main thread.
    struct Engine {
        pipeline: Pipeline,
        tracks: [Track; 2],
        jobs: Sender<Job>,
        /// System-audio tokens for the crosstalk check.
        reference: Reference,
        started: Instant,
        loaded: bool,
        busy: bool,
    }

    impl Engine {
        fn now_s(&self) -> f64 {
            self.started.elapsed().as_secs_f64()
        }

        /// Move captured audio into the pipeline.
        fn pull(&mut self) -> Result<()> {
            for index in [MIC, SYS] {
                let samples = self.tracks[index].pull(self.started);
                self.push(index, &samples)?;
            }
            Ok(())
        }

        fn push(&mut self, index: usize, samples: &[f32]) -> Result<()> {
            if samples.is_empty() {
                return Ok(());
            }
            self.tracks[index].save(samples)?;
            let now_s = self.now_s();
            self.pipeline.push(index, samples, now_s);
            let utterance = self.pipeline.utterance(index);
            let track = &mut self.tracks[index];
            if track.preview_utterance != utterance {
                track.preview_utterance = utterance;
                track.preview.clear();
                if index == SYS {
                    self.reference.clear_preview();
                }
            }
            if index == SYS {
                let sys = &self.tracks[SYS];
                self.reference
                    .prune_before(sys.seconds_at(sys.received) - REFERENCE_S);
            }
            Ok(())
        }

        /// Tokens of a track on the session clock.
        fn words(&self, track: usize, tokens: Vec<TimedToken>) -> Vec<Word> {
            tokens
                .into_iter()
                .map(|token| Word {
                    at_s: self.tracks[track].seconds_at(token.at),
                    text: token.text,
                })
                .collect()
        }

        /// Give the idle decoder the next job. Nothing is sent while the model loads, so
        /// finals wait in the pipeline and previews are built from the newest audio later.
        fn dispatch(&mut self) {
            if !self.loaded || self.busy {
                return;
            }
            let now_s = self.now_s();
            if let Some(job) = self.pipeline.next_job(now_s) {
                self.busy = self.jobs.send(job).is_ok();
            }
        }

        fn sys_now(&self) -> SysNow {
            SysNow {
                finals_pending: self.pipeline.pending_finals_of(SYS) > 0,
                region_start_s: self
                    .pipeline
                    .region(SYS)
                    .map(|region| self.tracks[SYS].seconds_at(region.span_start)),
            }
        }

        /// End of capture: flush the resamplers, close open utterances, decode what is left.
        fn finish(&mut self, events: &Receiver<Event>, session: &mut Session) -> Result<()> {
            self.pull()?;
            for index in [MIC, SYS] {
                let tail = match &self.tracks[index].resampler {
                    Some(resampler) => resampler.resample(&[], true),
                    None => Vec::new(),
                };
                self.push(index, &tail)?;
            }
            let now_s = self.now_s();
            self.pipeline.finish(now_s);
            while self.pipeline.pending_finals() > 0 {
                self.dispatch();
                let event = events.recv().context("decoder stopped")?;
                handle(event, self, session)?;
            }
            Ok(())
        }
    }

    /// SenseVoice can emit `<|zh|><|NEUTRAL|>`-style tags; keep only the text.
    fn strip_tags(text: &str) -> String {
        let mut out = String::with_capacity(text.len());
        let mut rest = text;
        while let Some(open) = rest.find("<|") {
            out.push_str(&rest[..open]);
            match rest[open..].find("|>") {
                Some(close) => rest = &rest[open + close + 2..],
                None => {
                    rest = "";
                }
            }
        }
        out.push_str(rest);
        out.trim().to_owned()
    }
    struct Line {
        track: usize,
        start_s: f64,
        end_s: f64,
        text: String,
        words: Vec<Word>,
        latency_s: f64,
        arrived: Instant,
    }

    /// Terminal output and transcript file. Styled lines and the one-line status go straight to
    /// the terminal device (`ECHONOTE_DEMO_TTY`); stdout gets the same lines as plain text.
    struct Session {
        tty: Option<File>,
        columns: usize,
        transcript: BufWriter<File>,
        status: String,
        pending_mic: VecDeque<Line>,
        latencies: Vec<f64>,
        lines: [usize; 2],
        crosstalk: usize,
        trimmed: usize,
        busy_s: f64,
    }

    /// Terminal columns used by a string: wide (CJK and other non-ASCII) characters count two.
    fn columns(text: &str) -> usize {
        text.chars()
            .map(|ch| if ch.is_ascii() { 1 } else { 2 })
            .sum()
    }

    /// The end of `text` that fits in `budget` columns.
    fn tail_fit(text: &str, budget: usize) -> String {
        let mut used = 0;
        let mut kept: Vec<char> = Vec::new();
        for ch in text.chars().rev() {
            used += if ch.is_ascii() { 1 } else { 2 };
            if used > budget {
                break;
            }
            kept.push(ch);
        }
        kept.iter().rev().collect()
    }
    /// Whether system tokens may still arrive, for the crosstalk check.
    struct SysNow {
        finals_pending: bool,
        /// Session time where the system utterance in progress started.
        region_start_s: Option<f64>,
    }

    impl Session {
        /// Print a finished line above the status line.
        fn print(&mut self, styled: &str, plain: &str) {
            let mut stdout = std::io::stdout();
            let _ = writeln!(stdout, "{plain}");
            let _ = stdout.flush();
            if let Some(tty) = self.tty.as_mut() {
                let _ = write!(tty, "\r\x1b[2K{styled}\n\x1b[2m{}\x1b[0m", self.status);
                let _ = tty.flush();
            }
        }

        fn draw_status(&mut self) {
            if let Some(tty) = self.tty.as_mut() {
                let _ = write!(tty, "\r\x1b[2K\x1b[2m{}\x1b[0m", self.status);
                let _ = tty.flush();
            }
        }

        fn say(&mut self, message: &str) {
            self.print(message, message);
        }

        fn emit(&mut self, mut line: Line, verdict: Verdict) -> Result<()> {
            let hidden = verdict == Verdict::Duplicate;
            let raw_text = match verdict {
                Verdict::Trim { text } => Some(std::mem::replace(&mut line.text, text)),
                _ => None,
            };
            let stamp = format!(
                "{:02}:{:02}",
                line.start_s as u64 / 60,
                line.start_s as u64 % 60
            );
            let speaker = if line.track == MIC { "我  " } else { "对方" };
            let mark = match (hidden, &raw_text) {
                (true, _) => "[外放串音] ",
                (false, Some(_)) => "[已去除串音] ",
                (false, None) => "",
            };
            let plain = format!("{stamp} {speaker} {mark}{}", line.text);
            // Hidden crosstalk is only counted on the status line; transcript.jsonl keeps it.
            if !hidden {
                let color = if line.track == MIC { 36 } else { 33 };
                let styled = format!("{stamp} \x1b[{color}m{speaker}\x1b[0m {}", line.text);
                self.print(&styled, &plain);
            } else {
                let mut stdout = std::io::stdout();
                let _ = writeln!(stdout, "{plain}");
                let _ = stdout.flush();
            }
            let crosstalk = match (hidden, &raw_text) {
                (true, _) => "duplicate",
                (false, Some(_)) => "trim",
                (false, None) => "keep",
            };
            let record = json!({
                "track": if line.track == MIC { "mic" } else { "sys" },
                "start_s": (line.start_s * 1000.0).round() / 1000.0,
                "end_s": (line.end_s * 1000.0).round() / 1000.0,
                "text": line.text,
                "crosstalk": (line.track == MIC).then_some(crosstalk),
                "raw_text": raw_text,
                "latency_s": (line.latency_s * 1000.0).round() / 1000.0,
            });
            writeln!(self.transcript, "{record}")?;
            self.transcript.flush()?;
            self.lines[line.track] += 1;
            self.crosstalk += usize::from(hidden);
            self.trimmed += usize::from(raw_text.is_some());
            self.latencies.push(line.latency_s);
            self.draw_status();
            Ok(())
        }

        /// Emit mic finals once the system track can confirm or rule out crosstalk.
        fn release_mic(&mut self, reference: &Reference, sys: &SysNow, force: bool) -> Result<()> {
            let config = CrosstalkConfig::default();
            while let Some(line) = self.pending_mic.front() {
                let verdict = reference.judge(&line.words, &config).verdict;
                // System tokens of this moment may still arrive: a queued final, or the
                // utterance in progress if it started before this line ended.
                let may_grow = sys.finals_pending
                    || sys
                        .region_start_s
                        .is_some_and(|start| start <= line.end_s + config.max_lag_s);
                if verdict != Verdict::Duplicate
                    && may_grow
                    && !force
                    && line.arrived.elapsed() < MIC_WAIT
                {
                    return Ok(());
                }
                let line = self.pending_mic.pop_front().unwrap();
                self.emit(line, verdict)?;
            }
            Ok(())
        }

        /// One status line that never wraps: the system track's preview while it speaks (the
        /// mic then mostly hears the speakers), otherwise the mic preview.
        fn update_status(&mut self, loaded: bool, sys_speaking: bool, previews: [&str; 2]) {
            let hidden = if self.crosstalk + self.trimmed > 0 {
                format!(
                    "  （外放串音：隐藏 {} 句，裁剪 {} 句）",
                    self.crosstalk, self.trimmed
                )
            } else {
                String::new()
            };
            let (label, preview) = if sys_speaking {
                ("对方", previews[SYS])
            } else {
                ("我", previews[MIC])
            };
            let status = if !loaded {
                "正在加载模型…".to_owned()
            } else if preview.is_empty() {
                let idle = if sys_speaking {
                    "● 对方说话中"
                } else {
                    "● 录音中"
                };
                format!("{idle}{hidden}")
            } else {
                let fixed = format!("● {label}：…{hidden}");
                let budget = self.columns.saturating_sub(columns(&fixed) + 2);
                format!("● {label}：…{}{hidden}", tail_fit(preview, budget))
            };
            if status != self.status {
                self.status = status;
                self.draw_status();
            }
        }
    }

    fn data_dir() -> PathBuf {
        if let Some(path) = std::env::var_os("ECHONOTE_DEV_DATA") {
            return PathBuf::from(path);
        }
        let home = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .unwrap_or_default();
        PathBuf::from(home).join(".cache").join("echonote-dev")
    }

    /// Apply one decoder event.
    fn handle(event: Event, engine: &mut Engine, session: &mut Session) -> Result<()> {
        let (job, transcript, decode_s) = match event {
            Event::Loaded(seconds) => {
                engine.loaded = true;
                session.say(&format!(
                    "模型加载完成（{seconds:.1} s），开始转录。按 Ctrl+C 结束。\n"
                ));
                return Ok(());
            }
            Event::LoadFailed(error) => anyhow::bail!("加载模型失败：{error}"),
            Event::Decoded {
                job,
                transcript,
                decode_s,
            } => (job, transcript, decode_s),
        };
        engine.busy = false;
        session.busy_s += decode_s;
        match engine.pipeline.complete(job, transcript, decode_s) {
            Some(Output::Preview {
                track,
                text,
                tokens,
                ..
            }) => {
                engine.tracks[track].preview = strip_tags(&text);
                if track == SYS {
                    let words = engine.words(SYS, tokens);
                    engine.reference.set_preview(&words);
                }
            }
            Some(Output::Final {
                track,
                cut,
                text,
                tokens,
            }) => {
                let text = strip_tags(&text);
                if text.is_empty() {
                    return Ok(());
                }
                let words = engine.words(track, tokens);
                if track == SYS {
                    engine.reference.add_final(&words);
                }
                let current = &engine.tracks[track];
                let end_s = current.seconds_at(cut.span_end);
                let line = Line {
                    track,
                    start_s: current.seconds_at(cut.span_start),
                    end_s,
                    text,
                    words,
                    latency_s: (current.seconds_at(current.received) - end_s).max(0.0),
                    arrived: Instant::now(),
                };
                if track == SYS {
                    session.emit(line, Verdict::Keep)?;
                } else {
                    session.pending_mic.push_back(line);
                }
            }
            // The utterance ended or was cut while its preview decoded.
            None => {}
        }
        Ok(())
    }

    pub fn run() -> Result<()> {
        let mut args = std::env::args().skip(1);
        let out_dir = PathBuf::from(
            args.next()
                .context("usage: live_demo <out_dir> [max_minutes]")?,
        );
        let max_minutes: u64 = args
            .next()
            .map(|value| value.parse())
            .transpose()?
            .unwrap_or(120);
        std::fs::create_dir_all(&out_dir)?;
        let models = data_dir().join("models");
        let model_dir = models.join("sherpa-onnx-sense-voice-zh-en-ja-ko-yue-int8-2025-09-09");
        anyhow::ensure!(
            model_dir.is_dir(),
            "{} not found; run scripts/dev/fetch_assets.py",
            model_dir.display()
        );

        // Start loading the model first; audio is buffered while it loads.
        let spec = OfflineModelSpec::new(ModelKind::SenseVoice, model_dir);
        let (jobs, job_queue) = mpsc::channel();
        let (events, received) = mpsc::channel();
        let decoder = std::thread::spawn(move || worker(spec, job_queue, events));
        let tty = match std::env::var_os("ECHONOTE_DEMO_TTY") {
            Some(path) => match std::fs::OpenOptions::new().write(true).open(&path) {
                Ok(file) => {
                    println!("@tty");
                    Some(file)
                }
                Err(error) => {
                    eprintln!("cannot write to {}: {error}", PathBuf::from(path).display());
                    None
                }
            },
            None => None,
        };
        let columns = std::env::var("ECHONOTE_DEMO_COLUMNS")
            .ok()
            .and_then(|value| value.trim().parse::<usize>().ok())
            .unwrap_or(100)
            .clamp(40, 160);
        let mut session = Session {
            tty,
            columns,
            transcript: BufWriter::new(File::create(out_dir.join("transcript.jsonl"))?),
            status: String::new(),
            pending_mic: VecDeque::new(),
            latencies: Vec::new(),
            lines: [0, 0],
            crosstalk: 0,
            trimmed: 0,
            busy_s: 0.0,
        };
        let started = Instant::now();
        let (mic, system) = match std::env::var_os("ECHONOTE_DEMO_REPLAY") {
            Some(dir) => {
                let dir = PathBuf::from(dir);
                (
                    replay(&dir.join("mic16.wav"))?,
                    replay(&dir.join("sys16.wav"))?,
                )
            }
            None => {
                let host = cpal::default_host();
                (
                    start_mic(&host)?,
                    start_system(&host).context("start system audio")?,
                )
            }
        };
        session.say(&format!("麦克风：{}（{} Hz）", mic.name, mic.rate));
        session.say(&format!("系统音频：{}（{} Hz）", system.name, system.rate));
        let vad_spec =
            VadSpec::meeting(VadKind::Silero, models.join("vad").join("silero_vad.onnx"));
        let detectors: Vec<Box<dyn SpeechDetector>> = vec![
            Box::new(SherpaDetector::new(&vad_spec)?),
            Box::new(SherpaDetector::new(&vad_spec)?),
        ];
        let config = PipelineConfig {
            // Through speakers the mic mostly hears the other side again.
            priority_track: Some(SYS),
            ..PipelineConfig::meeting(HARD_CAP_S, OVERLAP_S)
        };
        let mut engine = Engine {
            pipeline: Pipeline::new(config, detectors),
            tracks: [
                Track::new(mic, &out_dir.join("mic.wav"))?,
                Track::new(system, &out_dir.join("sys.wav"))?,
            ],
            jobs,
            reference: Reference::default(),
            started,
            loaded: false,
            busy: false,
        };

        let stop_file = out_dir.join("stop");
        let deadline = started + Duration::from_secs(max_minutes * 60);
        session.update_status(false, false, ["", ""]);
        while !stop_file.exists() && Instant::now() < deadline {
            match received.recv_timeout(TICK) {
                Ok(event) => handle(event, &mut engine, &mut session)?,
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => anyhow::bail!("decoder stopped"),
            }
            while let Ok(event) = received.try_recv() {
                handle(event, &mut engine, &mut session)?;
            }
            engine.pull()?;
            engine.dispatch();
            session.release_mic(&engine.reference, &engine.sys_now(), false)?;
            let sys_speaking = engine.pipeline.region(SYS).is_some();
            let [mic, sys] = &engine.tracks;
            session.update_status(engine.loaded, sys_speaking, [&mic.preview, &sys.preview]);
        }

        session.status = "正在转写剩余音频…".to_owned();
        session.draw_status();
        engine.finish(&received, &mut session)?;
        session.release_mic(&engine.reference, &engine.sys_now(), true)?;
        let Engine { tracks, jobs, .. } = engine;
        drop(jobs);
        let _ = decoder.join();
        let audio_s = tracks
            .iter()
            .map(|track| track.received as f64 / SR as f64)
            .fold(0.0, f64::max);
        let mut latencies = session.latencies.clone();
        latencies.sort_by(f64::total_cmp);
        let pick = |q: f64| {
            latencies
                .get(((latencies.len() as f64 * q).ceil() as usize).saturating_sub(1))
                .copied()
                .unwrap_or(0.0)
        };
        let dropped: u64 = tracks
            .iter()
            .map(|track| track.source.dropped.load(Ordering::Relaxed))
            .sum();
        let summary = format!(
            "\n录音 {:.0} s：我 {} 句（隐藏外放串音 {} 句，裁剪 {} 句），对方 {} 句。定稿延迟 p50 {:.2} s，p95 {:.2} s；\
             解码占用 {:.2}（两轨合计，1.0 表示解码线程满载）；丢弃样本 {}。\n文件：{}",
            audio_s,
            session.lines[MIC],
            session.crosstalk,
            session.trimmed,
            session.lines[SYS],
            pick(0.5),
            pick(0.95),
            session.busy_s / audio_s.max(1.0),
            dropped,
            out_dir.display(),
        );
        session.status.clear();
        session.say(&summary);
        for track in tracks {
            track.wav.finalize()?;
        }
        Ok(())
    }
}
