//! State shared by commands, the tray, the shortcut and the event forwarder.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use echonote_core::engine::Engine;
use echonote_core::glossary::{Entry, Glossary};
use echonote_core::store::{Store, StoreError};

use crate::ai::config::AiSettings;
use crate::ai::jobs::Jobs;
use crate::dev::Dev;
use crate::error::{CommandError, CommandResult};
use crate::live::Forwarder;
use crate::secrets::Secrets;
use crate::settings::Settings;

pub struct AppState {
    /// The engine, or why it could not open (the library is from a newer version, the data
    /// directory is not writable).
    pub engine: Result<Arc<Engine>, CommandError>,
    pub data_dir: PathBuf,
    /// The `models/` directories searched, the data directory's first (downloads go there).
    pub model_roots: Vec<PathBuf>,
    /// Model downloads in progress, by manifest id.
    pub downloads: Mutex<HashMap<String, echonote_models::Cancel>>,
    pub settings: Mutex<Settings>,
    pub live: Arc<Mutex<Forwarder>>,
    pub dev: Dev,
    /// Set by Quit: the next exit request is not turned into "keep running in the tray".
    pub quitting: AtomicBool,
    /// An error from a tray or shortcut action, for the window that opens to show it.
    pub pending_error: Mutex<Option<CommandError>>,
    /// Why the global shortcut could not be registered.
    pub shortcut_error: Mutex<Option<String>>,
    /// LLM providers and AI options.
    ai: Mutex<AiSettings>,
    pub secrets: Secrets,
    pub jobs: Jobs,
    /// Replacement rules for displayed and exported text; reloaded when edited.
    glossary: Mutex<Arc<Glossary>>,
    reader: Mutex<Option<Store>>,
}

pub fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl AppState {
    pub fn new(
        engine: Result<Arc<Engine>, CommandError>,
        data_dir: PathBuf,
        model_roots: Vec<PathBuf>,
        settings: Settings,
        ai: AiSettings,
        dev: Dev,
    ) -> Self {
        // A smoke run must not open a keychain prompt or leave entries behind.
        let secrets = if dev.smoke.is_some() {
            Secrets::memory()
        } else {
            Secrets::Keychain
        };
        Self {
            ai: Mutex::new(ai),
            secrets,
            jobs: Jobs::default(),
            engine,
            data_dir,
            model_roots,
            downloads: Mutex::default(),
            settings: Mutex::new(settings),
            live: Arc::new(Mutex::new(Forwarder::default())),
            dev,
            quitting: AtomicBool::new(false),
            pending_error: Mutex::new(None),
            shortcut_error: Mutex::new(None),
            glossary: Mutex::new(Arc::default()),
            reader: Mutex::new(None),
        }
    }

    pub fn engine(&self) -> CommandResult<&Arc<Engine>> {
        self.engine.as_ref().map_err(Clone::clone)
    }

    /// Where the live model and the VAD are now (a repair download may have added them).
    pub fn models(&self) -> crate::models::Located {
        crate::models::locate(&self.model_roots)
    }

    pub fn settings(&self) -> Settings {
        lock(&self.settings).clone()
    }

    pub fn ai(&self) -> AiSettings {
        lock(&self.ai).clone()
    }

    /// Change the AI settings and save them; nothing changes when `change` or the save fails.
    pub fn update_ai<T>(
        &self,
        change: impl FnOnce(&mut AiSettings) -> CommandResult<T>,
    ) -> CommandResult<T> {
        let mut current = lock(&self.ai);
        let mut next = current.clone();
        let value = change(&mut next)?;
        if next != *current {
            let json = next.to_json();
            self.engine()?
                .store()
                .call(move |store| store.set_setting(crate::ai::config::KEY, &json))?;
            *current = next;
        }
        Ok(value)
    }

    pub fn glossary(&self) -> Arc<Glossary> {
        lock(&self.glossary).clone()
    }

    /// Read the glossary from the library; returns its entries.
    pub fn reload_glossary(&self) -> CommandResult<Vec<Entry>> {
        let entries = self.read(|store| store.glossary())?;
        *lock(&self.glossary) = Arc::new(Glossary::new(&entries));
        Ok(entries)
    }

    /// Run `query` on the shared read-only connection, reopened after a database error.
    pub fn read<T>(&self, query: impl FnOnce(&Store) -> Result<T, StoreError>) -> CommandResult<T> {
        let engine = self.engine()?;
        let mut reader = lock(&self.reader);
        if reader.is_none() {
            *reader = Some(engine.store().reader()?);
        }
        let store = reader.as_ref().expect("opened above");
        query(store).map_err(|error| {
            if matches!(error, StoreError::Sqlite(_)) {
                *reader = None;
            }
            error.into()
        })
    }
}
