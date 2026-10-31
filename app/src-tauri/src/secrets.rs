//! API keys and secret request headers, in the macOS Keychain or the Windows Credential
//! Manager: one entry per provider, never in the library or a log. Smoke runs keep them in
//! memory so no system prompt can open.

use std::collections::HashMap;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::error::{CommandError, CommandResult};
use crate::state::lock;

/// Debug builds keep their own entries, so trying a build never touches the release app's.
#[cfg(debug_assertions)]
pub const SERVICE: &str = "com.rebill.echonote.debug";
#[cfg(not(debug_assertions))]
pub const SERVICE: &str = "com.rebill.echonote";

/// What a provider needs to authenticate. Not `Debug`.
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Secret {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub headers: Vec<(String, String)>,
}

impl Secret {
    pub fn is_empty(&self) -> bool {
        self.api_key.is_none() && self.headers.is_empty()
    }
}

pub enum Secrets {
    Keychain,
    Memory(Mutex<HashMap<String, String>>),
}

fn keychain_error(error: keyring::Error) -> CommandError {
    // The platform's message names the failure, never the secret.
    CommandError::new("keychain", error.to_string())
}

impl Secrets {
    pub fn memory() -> Self {
        Self::Memory(Mutex::default())
    }

    fn entry(account: &str) -> CommandResult<keyring::Entry> {
        keyring::Entry::new(SERVICE, account).map_err(keychain_error)
    }

    /// The secret saved for `account`, or none.
    pub fn get(&self, account: &str) -> CommandResult<Secret> {
        let stored = match self {
            Self::Keychain => match Self::entry(account)?.get_password() {
                Ok(value) => Some(value),
                Err(keyring::Error::NoEntry) => None,
                Err(error) => return Err(keychain_error(error)),
            },
            Self::Memory(map) => lock(map).get(account).cloned(),
        };
        // An entry this app did not write (or an older format) counts as no secret.
        Ok(stored
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default())
    }

    /// Save `secret` for `account`; an empty one removes the entry.
    pub fn set(&self, account: &str, secret: &Secret) -> CommandResult<()> {
        if secret.is_empty() {
            return self.delete(account);
        }
        let json = serde_json::to_string(secret).expect("secret serializes");
        match self {
            Self::Keychain => Self::entry(account)?
                .set_password(&json)
                .map_err(keychain_error),
            Self::Memory(map) => {
                lock(map).insert(account.to_owned(), json);
                Ok(())
            }
        }
    }

    pub fn delete(&self, account: &str) -> CommandResult<()> {
        match self {
            Self::Keychain => match Self::entry(account)?.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
                Err(error) => Err(keychain_error(error)),
            },
            Self::Memory(map) => {
                lock(map).remove(account);
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secret() -> Secret {
        Secret {
            api_key: Some("sk-test".into()),
            headers: vec![("X-Org".into(), "acme".into())],
        }
    }

    #[test]
    fn memory_round_trip_and_empty_removes() {
        let secrets = Secrets::memory();
        assert!(secrets.get("provider-1").unwrap().is_empty());
        secrets.set("provider-1", &secret()).unwrap();
        assert!(secrets.get("provider-1").unwrap() == secret());
        secrets.set("provider-1", &Secret::default()).unwrap();
        assert!(secrets.get("provider-1").unwrap().is_empty());
        secrets.delete("provider-1").unwrap();
    }

    /// Writes, reads and deletes one scratch entry in the real keychain. Run by hand:
    /// `cargo test -p echonote-app keychain -- --ignored` (macOS may ask for access).
    #[test]
    #[ignore]
    fn keychain_round_trip() {
        let secrets = Secrets::Keychain;
        let account = format!("test-{}", std::process::id());
        secrets.set(&account, &secret()).unwrap();
        let read = secrets.get(&account);
        secrets.delete(&account).unwrap();
        assert!(read.unwrap() == secret());
        assert!(secrets.get(&account).unwrap().is_empty());
    }
}
