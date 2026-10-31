//! The store thread: the one writer connection lives here and runs closures sent to it, in
//! order. UI queries open their own read-only connections and never wait for it.

use std::path::{Path, PathBuf};
use std::sync::mpsc;

use crate::store::{Store, StoreError};

type Task = Box<dyn FnOnce(&mut Store) + Send>;

#[derive(Clone)]
pub struct StoreHandle {
    tx: mpsc::Sender<Task>,
    path: PathBuf,
}

impl StoreHandle {
    /// Open (and migrate) the library at `path` and start its writer thread.
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        let mut store = Store::open(path)?;
        let (tx, rx) = mpsc::channel::<Task>();
        std::thread::Builder::new()
            .name("store".into())
            .spawn(move || {
                for task in rx {
                    task(&mut store);
                }
            })
            .map_err(StoreError::Io)?;
        Ok(Self {
            tx,
            path: path.to_owned(),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Run `task` on the writer and wait for its result.
    pub fn call<T: Send + 'static>(
        &self,
        task: impl FnOnce(&mut Store) -> Result<T, StoreError> + Send + 'static,
    ) -> Result<T, StoreError> {
        let (done, result) = mpsc::sync_channel(1);
        self.tx
            .send(Box::new(move |store| {
                let _ = done.send(task(store));
            }))
            .map_err(|_| StoreError::Closed)?;
        result.recv().map_err(|_| StoreError::Closed)?
    }

    /// Run `task` on the writer without waiting.
    pub fn send(&self, task: impl FnOnce(&mut Store) + Send + 'static) {
        let _ = self.tx.send(Box::new(task));
    }

    /// A read-only connection for queries.
    pub fn reader(&self) -> Result<Store, StoreError> {
        Store::open_read_only(&self.path)
    }
}
