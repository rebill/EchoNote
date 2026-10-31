use std::fs::File;
use std::io;
use std::path::Path;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread::JoinHandle;
use std::time::Duration;

/// Flushes recording files to the disk on its own thread, so a power cut loses at most
/// `interval` of audio and the capture thread never blocks on the disk.
pub struct Syncer {
    stop: Sender<()>,
    thread: JoinHandle<io::Result<()>>,
}

impl Syncer {
    /// Sync `files` and the directory holding them now (headers and file names become
    /// durable), every `interval`, and once more on [`stop`](Self::stop).
    pub fn spawn(dir: &Path, files: Vec<File>, interval: Duration) -> io::Result<Self> {
        let (stop, stopped) = mpsc::channel();
        let dir = dir.to_owned();
        let thread = std::thread::Builder::new()
            .name("recording-sync".into())
            .spawn(move || run(&dir, &files, interval, stopped))?;
        Ok(Self { stop, thread })
    }

    /// Final sync after the writers finished; returns the first error seen while recording.
    pub fn stop(self) -> io::Result<()> {
        let _ = self.stop.send(());
        self.thread
            .join()
            .unwrap_or_else(|_| Err(io::Error::other("recording sync thread panicked")))
    }
}

fn run(
    dir: &Path,
    files: &[File],
    interval: Duration,
    stopped: mpsc::Receiver<()>,
) -> io::Result<()> {
    let mut first_error = sync_files(files).err();
    if let Err(error) = sync_dir(dir) {
        first_error.get_or_insert(error);
    }
    loop {
        let last = !matches!(
            stopped.recv_timeout(interval),
            Err(RecvTimeoutError::Timeout)
        );
        if let Err(error) = sync_files(files) {
            first_error.get_or_insert(error);
        }
        if last {
            return first_error.map_or(Ok(()), Err);
        }
    }
}

/// `sync_data` is `fcntl(F_FULLFSYNC)` on macOS (a plain `fsync` there leaves data in the
/// drive cache), `fdatasync` on Linux and `FlushFileBuffers` on Windows.
fn sync_files(files: &[File]) -> io::Result<()> {
    let mut first = Ok(());
    for file in files {
        let synced = file.sync_data();
        if first.is_ok() {
            first = synced;
        }
    }
    first
}

/// Make new entries in `dir` durable. NTFS journals directory changes, and Windows cannot
/// open a directory as a `File`, so this is a no-op there.
pub fn sync_dir(dir: &Path) -> io::Result<()> {
    #[cfg(unix)]
    File::open(dir)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = dir;
    Ok(())
}
