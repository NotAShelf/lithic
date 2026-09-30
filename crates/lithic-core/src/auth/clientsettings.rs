//! Writing an account session into the game's `clientsettings.json`.
//!
//! The game keeps login state under `stringSettings` in the file at its data
//! path. The account fields are merged into whatever is there; everything else
//! in the file is preserved. A file that exists but does not parse is left
//! alone and reported.

use std::{fs, io::ErrorKind, mem, path::Path};

use serde_json::{Map, Value};

use super::store::Session;
use crate::{
  error::{Error, IoContext, Result},
  fsutil,
};

pub const CLIENTSETTINGS_FILE: &str = "clientsettings.json";

pub struct AccountIdentity<'a> {
  pub uid:        &'a str,
  pub playername: &'a str,
  pub email:      &'a str,
  pub session:    &'a Session,
}

/// Writes an account's session into the game data directory.
///
/// # Errors
/// Returns an error if the directory or settings file cannot be read or
/// written, or if existing settings are not a JSON object.
pub fn inject_account(
  data_dir: &Path,
  account: &AccountIdentity<'_>,
) -> Result<()> {
  fs::create_dir_all(data_dir).at(data_dir)?;
  let path = data_dir.join(CLIENTSETTINGS_FILE);
  let mut root = read_root(&path)?;
  let slot = root.entry("stringSettings").or_insert(Value::Null);
  let mut settings = match mem::take(slot) {
    Value::Object(settings) => settings,
    _ => Map::new(),
  };

  let fields = [
    ("playeruid", account.uid),
    ("playername", account.playername),
    ("useremail", account.email),
    ("sessionkey", account.session.sessionkey.as_str()),
    (
      "sessionsignature",
      account.session.sessionsignature.as_str(),
    ),
    ("mptoken", account.session.mptoken.as_str()),
    ("entitlements", account.session.entitlements.as_str()),
  ];
  for (key, value) in fields {
    // An empty entitlements string would wipe what the game already knows.
    if key == "entitlements" && value.is_empty() {
      continue;
    }
    settings.insert(key.to_string(), Value::String(value.to_string()));
  }
  *slot = Value::Object(settings);

  let json = serde_json::to_vec_pretty(&root)
    .map_err(|e| Error::parse(CLIENTSETTINGS_FILE, e))?;
  fsutil::write_atomic(&path, &json)
}

/// What [`clear_session`] found in the game's settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cleared {
  /// The targeted session was there and is gone; carries its key.
  Removed(String),
  /// No session at all.
  Clean,
  /// A session that is not the target: another account, or a newer login
  /// of the same one. Left as it is.
  Other,
}

/// Removes `uid`'s session secrets from the game data directory when
/// `is_target` accepts its key, leaving everything else in the file alone.
///
/// # Errors
/// Returns an error if the settings file cannot be read, parsed or written.
pub fn clear_session(
  data_dir: &Path,
  uid: &str,
  is_target: impl Fn(&str) -> bool,
) -> Result<Cleared> {
  let path = data_dir.join(CLIENTSETTINGS_FILE);
  if !path.is_file() {
    return Ok(Cleared::Clean);
  }
  let mut root = read_root(&path)?;
  let Some(Value::Object(settings)) = root.get_mut("stringSettings") else {
    return Ok(Cleared::Clean);
  };
  let key = settings
    .get("sessionkey")
    .and_then(Value::as_str)
    .unwrap_or_default()
    .to_string();
  if key.is_empty() {
    return Ok(Cleared::Clean);
  }
  if settings.get("playeruid").and_then(Value::as_str) != Some(uid)
    || !is_target(&key)
  {
    return Ok(Cleared::Other);
  }
  for field in ["sessionkey", "sessionsignature", "mptoken"] {
    settings.remove(field);
  }
  let json = serde_json::to_vec_pretty(&root)
    .map_err(|e| Error::parse(CLIENTSETTINGS_FILE, e))?;
  fsutil::write_atomic(&path, &json)?;
  Ok(Cleared::Removed(key))
}

/// Reads a complete session saved by the game after login.
///
/// # Errors
/// Returns an error if the game's settings cannot be read or parsed.
pub(crate) fn game_session(
  data_dir: &Path,
) -> Result<Option<(String, Session)>> {
  let root = read_root(&data_dir.join(CLIENTSETTINGS_FILE))?;
  let Some(Value::Object(settings)) = root.get("stringSettings") else {
    return Ok(None);
  };
  let field = |key| {
    settings
      .get(key)
      .and_then(Value::as_str)
      .unwrap_or_default()
  };
  let uid = field("playeruid");
  let sessionkey = field("sessionkey");
  let sessionsignature = field("sessionsignature");
  let mptoken = field("mptoken");
  if uid.is_empty() || sessionkey.is_empty() || sessionsignature.is_empty() {
    return Ok(None);
  }
  Ok(Some((uid.to_string(), Session {
    sessionkey:       sessionkey.to_string(),
    sessionsignature: sessionsignature.to_string(),
    mptoken:          mptoken.to_string(),
    entitlements:     field("entitlements").to_string(),
  })))
}

fn read_root(path: &Path) -> Result<Map<String, Value>> {
  match fs::read_to_string(path) {
    Ok(text) => {
      match serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}')) {
        Ok(Value::Object(obj)) => Ok(obj),
        Ok(_) => {
          Err(Error::Corrupt {
            path:    path.to_path_buf(),
            message: "not a JSON object".to_string(),
          })
        },
        Err(e) => {
          Err(Error::Corrupt {
            path:    path.to_path_buf(),
            message: e.to_string(),
          })
        },
      }
    },
    Err(e) if e.kind() == ErrorKind::NotFound => Ok(Map::new()),
    Err(e) => Err(Error::io(path, e)),
  }
}

#[cfg(test)]
#[expect(
  clippy::unwrap_used,
  reason = "test setup and assertions intentionally fail on error"
)]
mod tests {
  use super::*;

  fn session() -> Session {
    Session {
      sessionkey:       "sk".into(),
      sessionsignature: "sig".into(),
      mptoken:          "mt".into(),
      entitlements:     String::new(),
    }
  }

  #[test]
  fn creates_file_when_absent() {
    let dir = tempfile::tempdir().unwrap();
    let s = session();
    inject_account(dir.path(), &AccountIdentity {
      uid:        "u1",
      playername: "Steve",
      email:      "steve@example.test",
      session:    &s,
    })
    .unwrap();
    let v: Value = serde_json::from_str(
      &fs::read_to_string(dir.path().join(CLIENTSETTINGS_FILE)).unwrap(),
    )
    .unwrap();
    assert_eq!(v["stringSettings"]["playeruid"], "u1");
    assert_eq!(v["stringSettings"]["useremail"], "steve@example.test");
    assert_eq!(v["stringSettings"]["sessionkey"], "sk");
    assert!(v["stringSettings"].get("entitlements").is_none());
  }

  #[test]
  fn clearing_removes_only_that_accounts_secrets() {
    let dir = tempfile::tempdir().unwrap();
    let s = session();
    inject_account(dir.path(), &AccountIdentity {
      uid:        "u1",
      playername: "Steve",
      email:      "",
      session:    &s,
    })
    .unwrap();
    let any = |_: &str| true;
    assert_eq!(
      clear_session(dir.path(), "someone-else", any).unwrap(),
      Cleared::Other
    );
    assert_eq!(
      clear_session(dir.path(), "u1", |k| k == "older").unwrap(),
      Cleared::Other,
      "a newer login of the same account stays"
    );
    let read = || {
      serde_json::from_str::<Value>(
        &fs::read_to_string(dir.path().join(CLIENTSETTINGS_FILE)).unwrap(),
      )
      .unwrap()
    };
    assert_eq!(read()["stringSettings"]["sessionkey"], "sk");

    assert_eq!(
      clear_session(dir.path(), "u1", |k| k == "sk").unwrap(),
      Cleared::Removed("sk".into())
    );
    let v = read();
    assert!(v["stringSettings"].get("sessionkey").is_none());
    assert!(v["stringSettings"].get("sessionsignature").is_none());
    assert_eq!(v["stringSettings"]["playername"], "Steve");
    assert_eq!(
      clear_session(dir.path(), "u1", any).unwrap(),
      Cleared::Clean
    );

    let empty = tempfile::tempdir().unwrap();
    assert_eq!(
      clear_session(empty.path(), "u1", any).unwrap(),
      Cleared::Clean
    );
  }

  #[test]
  fn keeps_unrelated_settings() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
         dir.path().join(CLIENTSETTINGS_FILE),
         r#"{"stringSettings":{"language":"en","entitlements":"x"},"intSettings":{"masterVolume":50}}"#,
      )
      .unwrap();
    let s = session();
    inject_account(dir.path(), &AccountIdentity {
      uid:        "u2",
      playername: "Alex",
      email:      "alex@example.test",
      session:    &s,
    })
    .unwrap();
    let v: Value = serde_json::from_str(
      &fs::read_to_string(dir.path().join(CLIENTSETTINGS_FILE)).unwrap(),
    )
    .unwrap();
    assert_eq!(v["stringSettings"]["language"], "en");
    assert_eq!(v["stringSettings"]["entitlements"], "x");
    assert_eq!(v["stringSettings"]["playername"], "Alex");
    assert_eq!(v["intSettings"]["masterVolume"], 50);
  }

  #[test]
  fn unreadable_file_is_not_overwritten() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(CLIENTSETTINGS_FILE);
    fs::write(&path, "{ broken").unwrap();
    let s = session();
    let err = inject_account(dir.path(), &AccountIdentity {
      uid:        "u",
      playername: "p",
      email:      "p@example.test",
      session:    &s,
    })
    .unwrap_err();
    assert!(matches!(err, Error::Corrupt { .. }));
    assert_eq!(fs::read_to_string(&path).unwrap(), "{ broken");
  }
}
