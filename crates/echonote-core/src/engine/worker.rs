//! The single ASR worker thread. It holds at most one model: loaded on first use (or on a
//! preload when recording starts), kept while any client holds it, and unloaded after a
//! configurable idle time otherwise, so a tray-resident app gives the memory back.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::decoder::{Decoder, Model};
use crate::asr::Transcript;
use crate::pipeline::Job;
use crate::recording::decode_range;

/// Where a spilled job's audio is read back from.
#[derive(Clone, Debug)]
pub struct Spill {
    pub path: PathBuf,
    /// File sample of timeline index 0 (recovery decodes a tail, so its timeline starts later).
    pub base: u64,
}

#[derive(Debug)]
pub enum WorkerEvent {
    Loaded {
        model: String,
        seconds: f64,
    },
    LoadFailed {
        model: String,
        error: String,
    },
    Decoded {
        job: Job,
        transcript: Transcript,
        decode_s: f64,
    },
    /// Not decoded because the model failed to load.
    Skipped {
        job: Job,
    },
}

type Reply = Arc<dyn Fn(WorkerEvent) + Send + Sync>;

enum Message {
    Decode {
        job: Job,
        spill: Option<Spill>,
        model: Model,
        reply: Reply,
        cancel: Arc<AtomicBool>,
    },
    Load {
        model: Model,
        reply: Reply,
        cancel: Arc<AtomicBool>,
    },
    Hold(bool),
    /// Free this model now if nobody holds it (the high-accuracy model after a pass).
    Unload(String),
}

/// Handle to the worker thread; the thread exits when every handle and client is gone.
pub struct Worker {
    tx: mpsc::Sender<Message>,
    loaded: Arc<Mutex<Option<String>>>,
}

impl Worker {
    pub fn spawn(idle_unload: Duration) -> Self {
        let (tx, rx) = mpsc::channel();
        let loaded = Arc::new(Mutex::new(None));
        {
            let loaded = loaded.clone();
            std::thread::Builder::new()
                .name("asr-worker".into())
                .spawn(move || run(rx, idle_unload, loaded))
                .expect("spawn the ASR worker");
        }
        Self { tx, loaded }
    }

    /// Free `model` as soon as no client holds it, instead of after the idle time.
    pub fn unload(&self, model: &Model) {
        let _ = self.tx.send(Message::Unload(model.id()));
    }

    /// Id of the model in memory, if any.
    pub fn loaded(&self) -> Option<String> {
        self.loaded.lock().ok().and_then(|loaded| loaded.clone())
    }

    /// A client whose results go to `reply`. While `hold` is set the model stays loaded.
    pub fn client(
        &self,
        model: Model,
        hold: bool,
        reply: impl Fn(WorkerEvent) + Send + Sync + 'static,
    ) -> Client {
        if hold {
            let _ = self.tx.send(Message::Hold(true));
        }
        Client {
            tx: self.tx.clone(),
            model,
            reply: Arc::new(reply),
            cancel: Arc::new(AtomicBool::new(false)),
            hold,
        }
    }
}

/// One user of the worker: a recording session, a recovery or a re-transcription.
pub struct Client {
    tx: mpsc::Sender<Message>,
    model: Model,
    reply: Reply,
    cancel: Arc<AtomicBool>,
    hold: bool,
}

impl Client {
    pub fn model(&self) -> &Model {
        &self.model
    }

    /// Load the model now (answers with `Loaded` or `LoadFailed`, also when already loaded).
    pub fn load(&self) {
        let _ = self.tx.send(Message::Load {
            model: self.model.clone(),
            reply: self.reply.clone(),
            cancel: self.cancel.clone(),
        });
    }

    /// Drop this client's queued work once `cancel` is set (it is also set on drop), so
    /// another client's work need not wait behind it. A decode in progress finishes.
    pub fn cancelled_by(mut self, cancel: Arc<AtomicBool>) -> Self {
        self.cancel = cancel;
        self
    }

    /// Queue a job; a spilled job is read back from `spill` first.
    pub fn decode(&self, job: Job, spill: Option<Spill>) {
        let _ = self.tx.send(Message::Decode {
            job,
            spill,
            model: self.model.clone(),
            reply: self.reply.clone(),
            cancel: self.cancel.clone(),
        });
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        if self.hold {
            let _ = self.tx.send(Message::Hold(false));
        }
    }
}

struct Loaded {
    id: String,
    decoder: Box<dyn Decoder>,
}

fn run(rx: mpsc::Receiver<Message>, idle_unload: Duration, shown: Arc<Mutex<Option<String>>>) {
    let mut loaded: Option<Loaded> = None;
    let mut failed: Option<String> = None;
    let mut holds = 0usize;
    let mut last_used = Instant::now();
    let set_shown = |id: Option<String>| {
        if let Ok(mut shown) = shown.lock() {
            *shown = id;
        }
    };
    loop {
        let message = if loaded.is_some() && holds == 0 {
            let idle = last_used.elapsed();
            match rx.recv_timeout(idle_unload.saturating_sub(idle)) {
                Ok(message) => message,
                Err(RecvTimeoutError::Timeout) => {
                    loaded = None;
                    set_shown(None);
                    continue;
                }
                Err(RecvTimeoutError::Disconnected) => return,
            }
        } else {
            match rx.recv() {
                Ok(message) => message,
                Err(_) => return,
            }
        };
        last_used = Instant::now();
        match message {
            Message::Hold(true) => holds += 1,
            Message::Hold(false) => holds = holds.saturating_sub(1),
            Message::Unload(id) => {
                if holds == 0 && loaded.as_ref().is_some_and(|current| current.id == id) {
                    loaded = None;
                    set_shown(None);
                }
            }
            Message::Load {
                model,
                reply,
                cancel,
            } => {
                if cancel.load(Ordering::Relaxed) {
                    continue;
                }
                failed = None;
                ensure(&mut loaded, &mut failed, &model, &reply, true, &set_shown);
            }
            Message::Decode {
                mut job,
                spill,
                model,
                reply,
                cancel,
            } => {
                if cancel.load(Ordering::Relaxed) {
                    continue;
                }
                if !ensure(&mut loaded, &mut failed, &model, &reply, false, &set_shown) {
                    // Give the job back undecoded so its audio is not counted as transcribed.
                    reply(WorkerEvent::Skipped { job });
                    continue;
                }
                let started = Instant::now();
                if job.is_spilled()
                    && let Some(spill) = &spill
                {
                    let (start, end) = job.range();
                    job.samples = decode_range(
                        &spill.path,
                        spill.base + start as u64,
                        spill.base + end as u64,
                    )
                    .unwrap_or_default();
                }
                let decoder = &loaded.as_ref().expect("ensured").decoder;
                let transcript = if job.samples.is_empty() {
                    Transcript::default()
                } else {
                    decoder.decode(&job.samples)
                };
                let decode_s = started.elapsed().as_secs_f64();
                reply(WorkerEvent::Decoded {
                    job,
                    transcript,
                    decode_s,
                });
                last_used = Instant::now();
            }
        }
    }
}

/// Make `model` the loaded one. Returns whether it is loaded; `announce` replies `Loaded`
/// even when it already was.
fn ensure(
    loaded: &mut Option<Loaded>,
    failed: &mut Option<String>,
    model: &Model,
    reply: &Reply,
    announce: bool,
    shown: &impl Fn(Option<String>),
) -> bool {
    let id = model.id();
    if loaded.as_ref().is_some_and(|current| current.id == id) {
        if announce {
            reply(WorkerEvent::Loaded {
                model: model.name(),
                seconds: 0.0,
            });
        }
        return true;
    }
    if failed.as_deref() == Some(id.as_str()) {
        return false;
    }
    // Only one model in memory: drop the old one before loading.
    *loaded = None;
    shown(None);
    let started = Instant::now();
    match model.load() {
        Ok(decoder) => {
            *loaded = Some(Loaded {
                id: id.clone(),
                decoder,
            });
            shown(Some(id));
            reply(WorkerEvent::Loaded {
                model: model.name(),
                seconds: started.elapsed().as_secs_f64(),
            });
            true
        }
        Err(error) => {
            *failed = Some(id);
            reply(WorkerEvent::LoadFailed {
                model: model.name(),
                error: error.to_string(),
            });
            false
        }
    }
}
