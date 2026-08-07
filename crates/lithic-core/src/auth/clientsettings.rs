//! Writing an account session into the game's `clientsettings.json`.
//!
//! The game keeps login state under `stringSettings` in the file at its data
//! path. The account fields are merged into whatever is there; everything else
//! in the file is preserved. A file that exists but does not parse is left
//! alone and reported.

use std::path::Path;

use serde_json::{Map, Value};

use super::store::Session;
use crate::error::{Error, IoContext, Result};
use crate::fsutil;

pub const CLIENTSETTINGS_FILE: &str = "clientsettings.json";

pub struct AccountIdentity<'a> {
   pub uid: &'a str,
   pub playername: &'a str,
   pub session: &'a Session,
}

pub fn inject_account(data_dir: &Path, account: &AccountIdentity<'_>) -> Result<()> {
   std::fs::create_dir_all(data_dir).at(data_dir)?;
   let path = data_dir.join(CLIENTSETTINGS_FILE);
   let mut root = read_root(&path)?;
   let settings = string_settings(&mut root);

   let fields = [
      ("useridentifier", account.uid),
      ("playername", account.playername),
      ("sessionkey", account.session.sessionkey.as_str()),
      ("sessionsignature", account.session.sessionsignature.as_str()),
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

   let json = serde_json::to_vec_pretty(&root).map_err(|e| Error::parse(CLIENTSETTINGS_FILE, e))?;
   fsutil::write_atomic(&path, &json)
}

fn read_root(path: &Path) -> Result<Value> {
   match std::fs::read_to_string(path) {
      Ok(text) => match serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}')) {
         Ok(value) if value.is_object() => Ok(value),
         Ok(_) => Err(Error::Corrupt {
            path: path.to_path_buf(),
            message: "not a JSON object".to_string(),
         }),
         Err(e) => Err(Error::Corrupt {
            path: path.to_path_buf(),
            message: e.to_string(),
         }),
      },
      Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Value::Object(Map::new())),
      Err(e) => Err(Error::io(path, e)),
   }
}

fn string_settings(root: &mut Value) -> &mut Map<String, Value> {
   let Value::Object(obj) = root else {
      unreachable!("read_root only returns objects");
   };
   let slot = obj
      .entry("stringSettings")
      .or_insert_with(|| Value::Object(Map::new()));
   if !slot.is_object() {
      *slot = Value::Object(Map::new());
   }
   match slot {
      Value::Object(map) => map,
      _ => unreachable!("replaced with an object above"),
   }
}

#[cfg(test)]
mod tests {
   use super::*;

   fn session() -> Session {
      Session {
         sessionkey: "sk".into(),
         sessionsignature: "sig".into(),
         mptoken: "mt".into(),
         entitlements: String::new(),
      }
   }

   #[test]
   fn creates_file_when_absent() {
      let dir = tempfile::tempdir().unwrap();
      let s = session();
      inject_account(
         dir.path(),
         &AccountIdentity {
            uid: "u1",
            playername: "Steve",
            session: &s,
         },
      )
      .unwrap();
      let v: Value =
         serde_json::from_str(&std::fs::read_to_string(dir.path().join(CLIENTSETTINGS_FILE)).unwrap())
            .unwrap();
      assert_eq!(v["stringSettings"]["useridentifier"], "u1");
      assert_eq!(v["stringSettings"]["sessionkey"], "sk");
      assert!(v["stringSettings"].get("entitlements").is_none());
   }

   #[test]
   fn keeps_unrelated_settings() {
      let dir = tempfile::tempdir().unwrap();
      std::fs::write(
         dir.path().join(CLIENTSETTINGS_FILE),
         r#"{"stringSettings":{"language":"en","entitlements":"x"},"intSettings":{"masterVolume":50}}"#,
      )
      .unwrap();
      let s = session();
      inject_account(
         dir.path(),
         &AccountIdentity {
            uid: "u2",
            playername: "Alex",
            session: &s,
         },
      )
      .unwrap();
      let v: Value =
         serde_json::from_str(&std::fs::read_to_string(dir.path().join(CLIENTSETTINGS_FILE)).unwrap())
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
      std::fs::write(&path, "{ broken").unwrap();
      let s = session();
      let err = inject_account(
         dir.path(),
         &AccountIdentity {
            uid: "u",
            playername: "p",
            session: &s,
         },
      )
      .unwrap_err();
      assert!(matches!(err, Error::Corrupt { .. }));
      assert_eq!(std::fs::read_to_string(&path).unwrap(), "{ broken");
   }
}
