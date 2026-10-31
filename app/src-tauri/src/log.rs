//! A small append-only log in `<data dir>/logs/app.log`, rotated at 1 MB with one backup.
//! Only event kinds, error codes and timings go here: never transcript text, titles or keys.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_BYTES: u64 = 1 << 20;

struct Log {
    path: PathBuf,
    file: File,
}

static LOG: Mutex<Option<Log>> = Mutex::new(None);

pub fn init(data_dir: &Path) {
    let dir = data_dir.join("logs");
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let path = dir.join("app.log");
    if let Ok(file) = OpenOptions::new().create(true).append(true).open(&path)
        && let Ok(mut log) = LOG.lock()
    {
        *log = Some(Log { path, file });
    }
}

pub fn write(message: &str) {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |elapsed| elapsed.as_secs_f64());
    if cfg!(debug_assertions) {
        eprintln!("[echonote] {message}");
    }
    let Ok(mut guard) = LOG.lock() else {
        return;
    };
    let Some(log) = guard.as_mut() else {
        return;
    };
    let _ = writeln!(log.file, "{seconds:.3} {message}");
    if log.file.metadata().is_ok_and(|meta| meta.len() > MAX_BYTES) {
        let _ = std::fs::rename(&log.path, log.path.with_extension("log.1"));
        if let Ok(file) = OpenOptions::new().create(true).append(true).open(&log.path) {
            log.file = file;
        }
    }
}

macro_rules! log {
    ($($arg:tt)*) => {
        $crate::log::write(&format!($($arg)*))
    };
}
pub(crate) use log;
