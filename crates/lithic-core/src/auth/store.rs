//! Secret session storage. Sessions go to the OS keyring when the
//! `os-keyring` feature is on and a keyring is reachable, otherwise to a file
//! only the user can read.

#[cfg(all(feature = "os-keyring", not(test)))]
use std::result;
use std::{fmt, fs, path::PathBuf};

#[cfg(all(feature = "os-keyring", not(test)))]
use keyring::v1::{Entry, Error as KeyringError};
use serde::{Deserialize, Serialize};

use crate::{
  error::{Error, Result},
  fsutil,
};

#[cfg(all(feature = "os-keyring", not(test)))]
const KEYRING_SERVICE: &str = "lithic-vintagestory";

/// What the game needs to treat an account as signed in. Never logged.
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
  pub sessionkey:       String,
  pub sessionsignature: String,
  pub mptoken:          String,
  #[serde(default)]
  pub entitlements:     String,
}

impl fmt::Debug for Session {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.write_str("Session { .. }")
  }
}

pub(crate) struct SessionStore {
  dir: PathBuf,
}

impl SessionStore {
  pub(crate) fn new(dir: impl Into<PathBuf>) -> Self {
    Self { dir: dir.into() }
  }

  fn file(&self, uid: &str) -> PathBuf {
    // The uid comes from the auth server; it must never act as a path.
    let safe: String = uid
      .chars()
      .map(|c| {
        if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
          c
        } else {
          '_'
        }
      })
      .collect();
    self.dir.join(format!("{safe}.json"))
  }

  pub(crate) fn save(&self, uid: &str, session: &Session) -> Result<()> {
    let json =
      serde_json::to_string(session).map_err(|e| Error::parse("session", e))?;
    #[cfg(all(feature = "os-keyring", not(test)))]
    match keyring_set(uid, &json) {
      Ok(()) => {
        let _ = fsutil::remove_path(&self.file(uid));
        return Ok(());
      },
      Err(e) => {
        tracing::warn!(
          "keyring unavailable, storing the session in a private file: {e}"
        )
      },
    }
    fsutil::write_atomic_with_mode(
      &self.file(uid),
      json.as_bytes(),
      Some(0o600),
    )
  }

  pub(crate) fn load(&self, uid: &str) -> Option<Session> {
    #[cfg(all(feature = "os-keyring", not(test)))]
    if let Some(json) = keyring_get(uid)
      && let Ok(session) = serde_json::from_str(&json)
    {
      return Some(session);
    }
    let text = fs::read_to_string(self.file(uid)).ok()?;
    serde_json::from_str(&text).ok()
  }

  /// Removes the session everywhere it might be. Fails if a copy could not
  /// be removed, so a logout never silently leaves a secret behind.
  pub(crate) fn delete(&self, uid: &str) -> Result<()> {
    #[cfg(all(feature = "os-keyring", not(test)))]
    keyring_delete(uid)?;
    fsutil::remove_path(&self.file(uid))
  }

  #[cfg(test)]
  pub(crate) fn file_path(&self, uid: &str) -> PathBuf {
    self.file(uid)
  }
}

#[cfg(all(feature = "os-keyring", not(test)))]
fn keyring_entry(uid: &str) -> result::Result<Entry, KeyringError> {
  Entry::new(KEYRING_SERVICE, uid)
}

#[cfg(all(feature = "os-keyring", not(test)))]
fn keyring_set(uid: &str, json: &str) -> result::Result<(), KeyringError> {
  keyring_entry(uid)?.set_password(json)
}

#[cfg(all(feature = "os-keyring", not(test)))]
fn keyring_get(uid: &str) -> Option<String> {
  keyring_entry(uid).ok()?.get_password().ok()
}

#[cfg(all(feature = "os-keyring", not(test)))]
fn keyring_delete(uid: &str) -> Result<()> {
  let Ok(entry) = keyring_entry(uid) else {
    return Ok(());
  };
  match entry.delete_credential() {
    // No keyring at all means nothing was ever stored there.
    Ok(())
    | Err(
      KeyringError::NoEntry
      | KeyringError::NoStorageAccess(_)
      | KeyringError::PlatformFailure(_)
      | KeyringError::NoDefaultStore,
    ) => Ok(()),
    Err(e) => {
      Err(Error::invalid(format!(
        "could not remove the session from the keyring: {e}"
      )))
    },
  }
}

#[cfg(test)]
mod tests {
  use std::path::Path;

  use super::*;

  #[test]
  fn session_file_cannot_escape() {
    let store = SessionStore::new("/sessions");
    let path = store.file_path("../../etc/passwd");
    assert_eq!(path.parent(), Some(Path::new("/sessions")));
  }

  #[test]
  fn debug_hides_secrets() {
    let s = Session {
      sessionkey: "secret".into(),
      ..Session::default()
    };
    assert!(!format!("{s:?}").contains("secret"));
  }
}
