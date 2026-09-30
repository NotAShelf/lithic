//! Removing sessions from game settings once lithic no longer wants them
//! there: process-only logins, and accounts signed out of lithic.
//!
//! A running game keeps the session it started with, may renew it, and
//! writes it back as it exits. Such a write-back belongs to the old login
//! whatever its key, so the unit of cleanup is the game run, not the key:
//!
//! - When lithic sees the run exit, it removes any session for the account.
//! - When lithic stops watching first, or the account is signed out, the
//!   settings file is recorded in `session-cleanup.toml`, and every start
//!   removes any session for that account from it.
//! - An entry ends when lithic launches that instance again, since sessions
//!   from then on belong to the new run, for example a login through the game's
//!   own screen. It also ends when the account is remembered again or the data
//!   folder is gone.
//!
//! A game started outside lithic cannot be told apart from the old run, so
//! a login made that way is removed until lithic next launches the instance.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{Account, Accounts, clientsettings};
use crate::{Lithic, error::Result, fsutil};

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct Pending {
  #[serde(rename = "entry")]
  entries: Vec<Entry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Entry {
  uid:      String,
  data_dir: PathBuf,
}

/// Removes any session for `uid` from `data_dir`.
fn scrub(data_dir: &Path, uid: &str) -> Result<()> {
  clientsettings::clear_session(data_dir, uid, |_| true).map(drop)
}

impl Lithic {
  /// Records that no session for `uid` may stay in `data_dir`.
  ///
  /// # Errors
  /// Returns an error if the cleanup list cannot be updated.
  pub(crate) fn track_session_cleanup(
    &self,
    data_dir: &Path,
    uid: &str,
  ) -> Result<()> {
    let entry = Entry {
      uid:      uid.to_string(),
      data_dir: data_dir.to_path_buf(),
    };
    fsutil::update_toml(
      &self.paths.session_cleanup_file(),
      |p: &mut Pending| {
        if !p.entries.contains(&entry) {
          p.entries.push(entry);
        }
        Ok(())
      },
    )
  }

  fn untrack_session_cleanup(
    &self,
    keep: impl Fn(&Entry) -> bool,
  ) -> Result<()> {
    let path = self.paths.session_cleanup_file();
    if !path.is_file() {
      return Ok(());
    }
    fsutil::update_toml(&path, |p: &mut Pending| {
      p.entries.retain(|e| keep(e));
      Ok(())
    })
  }

  /// Stops cleaning up after `uid` everywhere, because it was remembered
  /// again and its session now belongs in the game's settings.
  ///
  /// # Errors
  /// Returns an error if the cleanup list cannot be updated.
  pub(crate) fn untrack_account_cleanup(&self, uid: &str) -> Result<()> {
    self.untrack_session_cleanup(|e| e.uid != uid)
  }

  /// Marks the start of a new game run in `data_dir`. Sessions written from
  /// now on belong to that run, so earlier cleanup entries for the folder
  /// end after one last scrub.
  ///
  /// # Errors
  /// Returns an error if the game's settings or the cleanup list cannot be
  /// read or written.
  pub(crate) fn start_game_run(&self, data_dir: &Path) -> Result<()> {
    let path = self.paths.session_cleanup_file();
    let pending: Option<Pending> = fsutil::read_toml(&path)?;
    let Some(pending) = pending else {
      return Ok(());
    };
    for e in pending.entries.iter().filter(|e| e.data_dir == data_dir) {
      scrub(&e.data_dir, &e.uid)?;
    }
    self.untrack_session_cleanup(|e| e.data_dir != data_dir)
  }

  /// Removes every recorded session, and ends entries whose data folder is
  /// gone (see the module notes).
  ///
  /// # Errors
  /// Returns an error if the cleanup list cannot be read or updated.
  pub fn clean_up_sessions(&self) -> Result<()> {
    let path = self.paths.session_cleanup_file();
    let pending: Option<Pending> = fsutil::read_toml(&path)?;
    if pending.is_none_or(|p| p.entries.is_empty()) {
      return Ok(());
    }
    fsutil::update_toml(&path, |p: &mut Pending| {
      p.entries.retain(|e| e.data_dir.is_dir());
      for e in &p.entries {
        if let Err(err) = scrub(&e.data_dir, &e.uid) {
          tracing::warn!(
            "could not remove a session from {}: {err}",
            e.data_dir.display()
          );
        }
      }
      Ok(())
    })
  }

  /// Removes `uid`'s session from `data_dir` on sign-out, and keeps watching
  /// in case a running game writes it back.
  ///
  /// # Errors
  /// Returns an error if the game's settings or the cleanup list cannot be
  /// read or written.
  pub(crate) fn sign_out_of(&self, data_dir: &Path, uid: &str) -> Result<()> {
    if let clientsettings::Cleared::Removed(_) =
      clientsettings::clear_session(data_dir, uid, |_| true)?
    {
      self.track_session_cleanup(data_dir, uid)?;
    }
    Ok(())
  }

  /// Whether `uid` is signed in and remembered, so its session belongs in
  /// the game's settings.
  fn keeps_session(&self, uid: &str) -> Result<bool> {
    let saved: Accounts =
      fsutil::read_toml(&self.paths.accounts_file())?.unwrap_or_default();
    Ok(saved.get(uid).is_some() && !self.is_transient(uid))
  }

  /// Settles the session in a game's settings after lithic sees the game
  /// exit. A remembered account keeps the session the game renewed.
  /// Otherwise the login was process-only or signed out while the game ran,
  /// and whatever session the run wrote back, renewed or not, is removed.
  ///
  /// # Errors
  /// Returns an error if the accounts, the game's settings or the cleanup
  /// list cannot be read or written.
  pub(crate) fn finish_game_session(
    &self,
    data_dir: &Path,
    account: &Account,
  ) -> Result<()> {
    if self.keeps_session(&account.uid)? {
      return self.sync_game_session(data_dir, account);
    }
    if self.is_transient(&account.uid) {
      // Later launches in this process start from the renewed session.
      self.sync_game_session(data_dir, account)?;
    }
    scrub(data_dir, &account.uid)?;
    self.untrack_session_cleanup(|e| {
      e.data_dir != data_dir || e.uid != account.uid
    })
  }

  /// Removes the session lithic does not keep when it stops watching a game
  /// that may still be running, and records the file so the next start
  /// removes whatever the game writes back on exit.
  ///
  /// # Errors
  /// Returns an error if the accounts, the game's settings or the cleanup
  /// list cannot be read or written.
  pub(crate) fn abandon_game_session(
    &self,
    data_dir: &Path,
    account: &Account,
  ) -> Result<()> {
    if self.keeps_session(&account.uid)? {
      return Ok(());
    }
    scrub(data_dir, &account.uid)?;
    self.track_session_cleanup(data_dir, &account.uid)
  }
}

#[cfg(test)]
#[expect(
  clippy::unwrap_used,
  reason = "test setup and assertions intentionally fail on error"
)]
mod tests {
  use std::fs;

  use super::*;
  use crate::{
    Paths,
    auth::{LoginResponse, store::Session},
  };

  fn write_session(data_dir: &Path, uid: &str, key: &str) {
    clientsettings::inject_account(
      data_dir,
      &clientsettings::AccountIdentity {
        uid,
        playername: "Steve",
        email: "",
        session: &Session {
          sessionkey:       key.into(),
          sessionsignature: "sig".into(),
          mptoken:          String::new(),
          entitlements:     String::new(),
        },
      },
    )
    .unwrap();
  }

  fn session_in(data_dir: &Path) -> Option<String> {
    let text =
      fs::read_to_string(data_dir.join(clientsettings::CLIENTSETTINGS_FILE))
        .unwrap();
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    v["stringSettings"]["sessionkey"].as_str().map(String::from)
  }

  fn pending(l: &Lithic) -> Vec<Entry> {
    fsutil::read_toml::<Pending>(&l.paths.session_cleanup_file())
      .unwrap()
      .unwrap_or_default()
      .entries
  }

  fn login(uid: &str, key: &str) -> LoginResponse {
    LoginResponse {
      uid: uid.into(),
      playername: "Steve".into(),
      sessionkey: key.into(),
      sessionsignature: "sig".into(),
      ..LoginResponse::default()
    }
  }

  fn account(uid: &str) -> Account {
    Account {
      uid: uid.into(),
      ..Account::default()
    }
  }

  fn lithic() -> (tempfile::TempDir, Lithic, PathBuf) {
    let d = tempfile::tempdir().unwrap();
    let l = Lithic::new(Paths::rooted(d.path())).unwrap();
    let data = d.path().join("data");
    fs::create_dir_all(&data).unwrap();
    (d, l, data)
  }

  #[test]
  fn restart_while_the_game_runs_keeps_watching_the_file() {
    let (_d, l, data) = lithic();
    write_session(&data, "u1", "old");
    // Lithic quits while the game keeps running.
    l.abandon_game_session(&data, &account("u1")).unwrap();
    assert_eq!(session_in(&data), None);

    // Lithic starts again before the game exits and finds the file clean.
    l.clean_up_sessions().unwrap();
    assert_eq!(pending(&l).len(), 1, "a clean file does not end the entry");

    // The game exits and writes back a session it renewed meanwhile.
    write_session(&data, "u1", "renewed");
    l.clean_up_sessions().unwrap();
    assert_eq!(session_in(&data), None);

    fs::remove_dir_all(&data).unwrap();
    l.clean_up_sessions().unwrap();
    assert!(pending(&l).is_empty(), "a deleted instance is not watched");
  }

  #[test]
  fn renewed_session_written_after_sign_out_is_removed_on_exit() {
    let (_d, l, data) = lithic();
    l.save_login("a@b", login("u1", "old")).unwrap();
    write_session(&data, "u1", "old");
    l.logout("u1").unwrap();
    l.sign_out_of(&data, "u1").unwrap();

    // Watched: the run that was going at sign-out exits with a new key.
    write_session(&data, "u1", "renewed");
    l.finish_game_session(&data, &account("u1")).unwrap();
    assert_eq!(session_in(&data), None);
  }

  #[test]
  fn renewed_session_written_after_sign_out_while_unwatched_is_removed() {
    let (_d, l, data) = lithic();
    l.save_login("a@b", login("u1", "old")).unwrap();
    write_session(&data, "u1", "old");
    l.sign_out_of(&data, "u1").unwrap();

    // Nobody watched the run; it exits with a key renewed during play.
    write_session(&data, "u1", "renewed");
    l.clean_up_sessions().unwrap();
    assert_eq!(session_in(&data), None);
  }

  #[test]
  fn login_in_a_later_run_persists() {
    let (_d, l, data) = lithic();
    l.save_login("a@b", login("u1", "old")).unwrap();
    write_session(&data, "u1", "old");
    l.sign_out_of(&data, "u1").unwrap();

    // Lithic launches the instance again, without an account, and the user
    // signs in through the game's own login screen.
    l.start_game_run(&data).unwrap();
    write_session(&data, "u1", "new");
    l.clean_up_sessions().unwrap();
    assert_eq!(session_in(&data).as_deref(), Some("new"));
    assert!(pending(&l).is_empty());
  }

  #[test]
  fn a_new_run_removes_what_the_old_run_wrote_first() {
    let (_d, l, data) = lithic();
    write_session(&data, "u1", "old");
    l.abandon_game_session(&data, &account("u1")).unwrap();
    // The old run wrote its session back before lithic started the new one.
    write_session(&data, "u1", "old");
    l.start_game_run(&data).unwrap();
    assert_eq!(session_in(&data), None);
  }

  #[test]
  fn signing_in_again_cancels_an_earlier_sign_out() {
    let (_d, l, data) = lithic();
    l.save_login("a@b", login("u1", "sk")).unwrap();
    write_session(&data, "u1", "sk");
    l.track_session_cleanup(&data, "u1").unwrap();

    l.save_login("a@b", login("u1", "sk")).unwrap();
    assert!(pending(&l).is_empty());
    l.clean_up_sessions().unwrap();
    assert_eq!(session_in(&data).as_deref(), Some("sk"));
  }
}
