//! Vintage Story accounts: logging in, remembering sessions, and handing the
//! right one to the game at launch.
//!
//! Account names and ids live in `accounts.toml`; the secret session lives in
//! the keyring or a private file (see [`store`]).

use std::{path::Path, sync::MutexGuard};

mod cleanup;
pub mod client;
pub mod clientsettings;
pub mod store;

pub use client::{AuthError, LoginResponse};
use clientsettings::AccountIdentity;
use serde::{Deserialize, Serialize};
pub use store::Session;
use store::SessionStore;

use crate::{
  Lithic,
  error::{Error, Kind, Result},
  fsutil,
  instance::Instance,
};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Account {
  pub uid:        String,
  pub playername: String,
  #[serde(default)]
  pub email:      String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Accounts {
  /// Account used for instances that do not name one.
  pub active:   Option<String>,
  #[serde(rename = "account")]
  pub accounts: Vec<Account>,
}

impl Accounts {
  #[must_use]
  pub fn get(&self, uid: &str) -> Option<&Account> {
    self.accounts.iter().find(|a| a.uid == uid)
  }

  /// The account an instance launches with: its own choice if that account
  /// still exists, otherwise the active one.
  #[must_use]
  pub fn for_instance(
    &self,
    instance_account: Option<&str>,
  ) -> Option<&Account> {
    instance_account
      .and_then(|uid| self.get(uid))
      .or_else(|| self.active.as_deref().and_then(|uid| self.get(uid)))
  }
}

#[derive(Debug, Default)]
pub(crate) struct TransientAccounts {
  active:   Option<String>,
  accounts: Vec<(Account, Session)>,
}

impl Lithic {
  fn sessions(&self) -> SessionStore {
    SessionStore::new(self.paths.sessions_dir())
  }

  fn temporary(&self) -> Result<MutexGuard<'_, TransientAccounts>> {
    self
      .transient
      .lock()
      .map_err(|_| Error::invalid("temporary account state is unavailable"))
  }

  fn session(&self, uid: &str) -> Result<Option<Session>> {
    let temporary = self.temporary()?;
    let session = temporary
      .accounts
      .iter()
      .find(|(account, _)| account.uid == uid)
      .map(|(_, session)| session.clone());
    drop(temporary);
    Ok(session.or_else(|| self.sessions().load(uid)))
  }

  /// Reads saved accounts, returning an empty list if none exist.
  ///
  /// # Errors
  /// Returns an error if the accounts file cannot be read or parsed.
  pub fn accounts(&self) -> Result<Accounts> {
    let mut saved: Accounts =
      fsutil::read_toml(&self.paths.accounts_file())?.unwrap_or_default();
    let temporary = self.temporary()?;
    for (account, _) in &temporary.accounts {
      match saved.accounts.iter_mut().find(|a| a.uid == account.uid) {
        Some(existing) => existing.clone_from(account),
        None => saved.accounts.push(account.clone()),
      }
    }
    if let Some(active) = &temporary.active {
      saved.active = Some(active.clone());
    }
    drop(temporary);
    Ok(saved)
  }

  /// One login attempt. On [`AuthError::TwoFactorRequired`], call again with
  /// `twofa` set to the returned token and the user's code. When `remember`
  /// is false, the account is usable only until this `Lithic` is dropped.
  /// Its session is removed from an instance's game settings when the game
  /// exits, or when lithic stops waiting for it; if the game writes it back
  /// after that, [`Lithic::clean_up_sessions`] removes it at the next start.
  ///
  /// # Errors
  /// Returns an authentication error if login fails, or a storage error if
  /// a remembered account cannot be saved.
  pub async fn login(
    &self,
    email: &str,
    password: &str,
    twofa: Option<(&str, &str)>,
    remember: bool,
  ) -> Result<Account> {
    let response =
      client::gamelogin(&self.http, email, password, twofa).await?;
    if remember {
      self.save_login(email, response)
    } else {
      self.use_login_once(email, response)
    }
  }

  fn login_parts(
    email: &str,
    login: LoginResponse,
  ) -> Result<(Account, Session)> {
    if login.uid.is_empty() {
      return Err(Error::Auth(AuthError::Server(
        "the login succeeded but carried no account id".into(),
      )));
    }
    Ok((
      Account {
        uid:        login.uid,
        playername: login.playername,
        email:      email.trim().to_string(),
      },
      Session {
        sessionkey:       login.sessionkey,
        sessionsignature: login.sessionsignature,
        mptoken:          login.mptoken,
        entitlements:     login.entitlements,
      },
    ))
  }

  fn use_login_once(
    &self,
    email: &str,
    login: LoginResponse,
  ) -> Result<Account> {
    let (account, session) = Self::login_parts(email, login)?;
    let no_active = self.accounts()?.active.is_none();
    let mut temporary = self.temporary()?;
    if let Some((existing, saved)) = temporary
      .accounts
      .iter_mut()
      .find(|(a, _)| a.uid == account.uid)
    {
      *existing = account.clone();
      *saved = session;
    } else {
      temporary.accounts.push((account.clone(), session));
    }
    if no_active {
      temporary.active = Some(account.uid.clone());
    }
    drop(temporary);
    Ok(account)
  }

  /// Stores a successful login. The first account becomes the active one.
  ///
  /// # Errors
  /// Returns an authentication error if the account ID is missing, or a
  /// storage error if the session or accounts file cannot be saved.
  pub fn save_login(
    &self,
    email: &str,
    login: LoginResponse,
  ) -> Result<Account> {
    let (account, session) = Self::login_parts(email, login)?;
    self.sessions().save(&account.uid, &session)?;
    let saved = account.clone();
    fsutil::update_toml(
      &self.paths.accounts_file(),
      move |a: &mut Accounts| {
        match a.accounts.iter_mut().find(|x| x.uid == account.uid) {
          Some(existing) => *existing = account.clone(),
          None => a.accounts.push(account.clone()),
        }
        if a.active.is_none() {
          a.active = Some(account.uid);
        }
        Ok(())
      },
    )?;
    let mut temporary = self.temporary()?;
    temporary.accounts.retain(|(a, _)| a.uid != saved.uid);
    if temporary.active.as_deref() == Some(saved.uid.as_str()) {
      temporary.active = None;
    }
    drop(temporary);
    // Remembered again, so an earlier sign-out must not scrub this session.
    self.untrack_account_cleanup(&saved.uid)?;
    Ok(saved)
  }

  /// Makes an existing account active.
  ///
  /// # Errors
  /// Returns an error if the account is unknown or its registry cannot be
  /// updated.
  pub fn set_active_account(&self, uid: &str) -> Result<()> {
    let mut temporary = self.temporary()?;
    if temporary.accounts.iter().any(|(a, _)| a.uid == uid) {
      temporary.active = Some(uid.to_string());
      return Ok(());
    }
    fsutil::update_toml(&self.paths.accounts_file(), |a: &mut Accounts| {
      if a.get(uid).is_none() {
        return Err(Error::not_found(Kind::Account, uid));
      }
      a.active = Some(uid.to_string());
      Ok(())
    })?;
    temporary.active = None;
    drop(temporary);
    Ok(())
  }

  /// Forgets an account and deletes its session. If it was active, no
  /// account is active afterwards.
  ///
  /// # Errors
  /// Returns an error if the session cannot be deleted or the accounts file
  /// cannot be read or updated.
  pub fn logout(&self, uid: &str) -> Result<()> {
    let saved: Accounts =
      fsutil::read_toml(&self.paths.accounts_file())?.unwrap_or_default();
    if saved.get(uid).is_some() {
      self.sessions().delete(uid)?;
      fsutil::update_toml(&self.paths.accounts_file(), |a: &mut Accounts| {
        a.accounts.retain(|x| x.uid != uid);
        if a.active.as_deref() == Some(uid) {
          a.active = None;
        }
        Ok(())
      })?;
    }
    let mut temporary = self.temporary()?;
    temporary.accounts.retain(|(a, _)| a.uid != uid);
    if temporary.active.as_deref() == Some(uid) {
      temporary.active = None;
    }
    drop(temporary);
    for instance in self.list_instances()?.instances {
      if let Err(e) = self.sign_out_of(&instance.data_dir(), uid) {
        tracing::warn!("could not sign {uid} out of {}: {e}", instance.id);
      }
    }
    Ok(())
  }

  /// Whether `uid` was signed in without being remembered.
  pub(crate) fn is_transient(&self, uid: &str) -> bool {
    self
      .temporary()
      .is_ok_and(|state| state.accounts.iter().any(|(a, _)| a.uid == uid))
  }

  #[must_use]
  pub fn has_session(&self, uid: &str) -> bool {
    self.is_transient(uid) || self.sessions().load(uid).is_some()
  }

  /// Asks Vintage Story whether an account's current session is accepted.
  /// A missing or incomplete session is rejected without a network request.
  ///
  /// # Errors
  /// Returns an error if the auth server cannot verify the session.
  pub async fn check_account_session(&self, uid: &str) -> Result<bool> {
    let Some(session) = self.session(uid)? else {
      return Ok(false);
    };
    if session.sessionkey.is_empty() || session.sessionsignature.is_empty() {
      return Ok(false);
    }
    Ok(client::validate_session(&self.http, uid, &session.sessionkey).await?)
  }

  /// Stops a launch whose account session the auth server has rejected.
  /// Without an account the game shows its own login screen, so that
  /// launch goes ahead.
  ///
  /// # Errors
  ///
  /// Returns an error if the accounts cannot be read, or the account's
  /// session is missing or rejected.
  pub(crate) async fn verify_launch_session(
    &self,
    instance: &Instance,
  ) -> Result<()> {
    let accounts = self.accounts()?;
    let Some(account) = accounts.for_instance(instance.account.as_deref())
    else {
      return Ok(());
    };
    launch_verdict(
      &account.playername,
      self.check_account_session(&account.uid).await,
    )
  }

  /// Writes the instance's account into its `clientsettings.json` before a
  /// launch. Without any account this does nothing and the game shows its
  /// own login screen.
  ///
  /// # Errors
  ///
  /// Returns an error if the accounts file cannot be read, the chosen account
  /// has no session, or the game's settings cannot be updated.
  pub fn inject_account(&self, instance: &Instance) -> Result<Option<Account>> {
    let accounts = self.accounts()?;
    let Some(account) = accounts.for_instance(instance.account.as_deref())
    else {
      return Ok(None);
    };
    let session = self.session(&account.uid)?.ok_or_else(|| {
      Error::Auth(AuthError::InvalidCredentials(format!(
        "no session for {}; log in again",
        account.playername
      )))
    })?;
    clientsettings::inject_account(&instance.data_dir(), &AccountIdentity {
      uid:        &account.uid,
      playername: &account.playername,
      email:      &account.email,
      session:    &session,
    })?;
    Ok(Some(account.clone()))
  }

  /// Keeps a session renewed by the game after login for subsequent launches.
  ///
  /// # Errors
  /// Returns an error if the game's settings cannot be read or the session
  /// cannot be saved.
  pub(crate) fn sync_game_session(
    &self,
    data_dir: &Path,
    account: &Account,
  ) -> Result<()> {
    let Some((uid, mut session)) = clientsettings::game_session(data_dir)?
    else {
      return Ok(());
    };
    if uid != account.uid {
      return Ok(());
    }
    let mut temporary = self.temporary()?;
    if let Some((_, saved)) =
      temporary.accounts.iter_mut().find(|(a, _)| a.uid == uid)
    {
      if session.entitlements.is_empty() {
        session.entitlements.clone_from(&saved.entitlements);
      }
      *saved = session;
      return Ok(());
    }
    drop(temporary);
    let store = self.sessions();
    let Some(saved) = store.load(&uid) else {
      return Ok(());
    };
    if session.entitlements.is_empty() {
      session.entitlements.clone_from(&saved.entitlements);
    }
    if session != saved {
      store.save(&uid, &session)?;
    }
    Ok(())
  }
}

#[cfg(test)]
impl Lithic {
  /// Signs `uid` in for this process only, without the auth server.
  pub(crate) fn login_once_for_test(&self, uid: &str) -> Result<Account> {
    self.use_login_once("", LoginResponse {
      uid: uid.into(),
      playername: "Steve".into(),
      sessionkey: "sk".into(),
      sessionsignature: "sig".into(),
      ..LoginResponse::default()
    })
  }
}

/// Whether a session check lets the game start. Only an explicit rejection
/// stops it: singleplayer works without the auth server, and the game
/// reports multiplayer login problems itself.
fn launch_verdict(player: &str, checked: Result<bool>) -> Result<()> {
  match checked {
    Ok(true) => Ok(()),
    Ok(false) => {
      Err(Error::invalid(format!(
        "session for {player} is missing or rejected; log in to Lithic again"
      )))
    },
    Err(Error::Auth(e @ (AuthError::Network(_) | AuthError::Server(_)))) => {
      tracing::warn!("could not verify the session for {player}: {e}");
      Ok(())
    },
    Err(e) => Err(e),
  }
}

#[cfg(test)]
#[expect(
  clippy::unwrap_used,
  reason = "test setup and assertions intentionally fail on error"
)]
mod tests {
  use super::*;

  #[test]
  fn only_a_rejected_session_blocks_launch() {
    assert!(launch_verdict("p", Ok(true)).is_ok());
    assert!(launch_verdict("p", Ok(false)).is_err());
    assert!(
      launch_verdict("p", Err(Error::Auth(AuthError::Network("down".into()))))
        .is_ok(),
      "an unreachable auth server must not block singleplayer"
    );
    assert!(
      launch_verdict("p", Err(Error::Auth(AuthError::Server("503".into()))))
        .is_ok()
    );
    assert!(
      launch_verdict("p", Err(Error::invalid("broken accounts file"))).is_err()
    );
  }

  fn accounts() -> Accounts {
    Accounts {
      active:   Some("a".into()),
      accounts: vec![
        Account {
          uid:        "a".into(),
          playername: "A".into(),
          email:      String::new(),
        },
        Account {
          uid:        "b".into(),
          playername: "B".into(),
          email:      String::new(),
        },
      ],
    }
  }

  #[test]
  fn instance_choice_wins_when_valid() {
    assert_eq!(accounts().for_instance(Some("b")).unwrap().uid, "b");
    assert_eq!(accounts().for_instance(Some("gone")).unwrap().uid, "a");
    assert_eq!(accounts().for_instance(None).unwrap().uid, "a");
    let none = Accounts {
      active: None,
      ..accounts()
    };
    assert!(none.for_instance(None).is_none());
  }

  #[test]
  fn save_login_and_logout() {
    let d = tempfile::tempdir().unwrap();
    let l = Lithic::new(crate::Paths::rooted(d.path())).unwrap();
    let login = |uid: &str| {
      LoginResponse {
        uid: uid.into(),
        playername: uid.to_uppercase(),
        sessionkey: "k".into(),
        ..LoginResponse::default()
      }
    };
    l.save_login("x@y", login("one")).unwrap();
    l.save_login("z@y", login("two")).unwrap();
    let a = l.accounts().unwrap();
    assert_eq!(a.active.as_deref(), Some("one"));
    assert_eq!(a.accounts.len(), 2);
    l.logout("one").unwrap();
    let a = l.accounts().unwrap();
    assert_eq!(a.active, None);
    assert_eq!(a.accounts.len(), 1);
  }

  #[test]
  fn unchecked_login_is_available_without_being_remembered() {
    let dir = tempfile::tempdir().unwrap();
    let paths = crate::Paths::rooted(dir.path());
    let lithic = Lithic::new(paths.clone()).unwrap();
    lithic
      .use_login_once("one@example.test", LoginResponse {
        uid: "once".into(),
        playername: "Once".into(),
        sessionkey: "key".into(),
        sessionsignature: "signature".into(),
        ..LoginResponse::default()
      })
      .unwrap();

    let clone = lithic.clone();
    let accounts = clone.accounts().unwrap();
    assert_eq!(accounts.active.as_deref(), Some("once"));
    assert_eq!(accounts.accounts[0].email, "one@example.test");
    assert!(clone.has_session("once"));
    assert_eq!(clone.session("once").unwrap().unwrap().sessionkey, "key");
    assert!(!paths.accounts_file().exists());
    assert!(!lithic.sessions().file_path("once").exists());

    let next_run = Lithic::new(paths).unwrap();
    assert!(next_run.accounts().unwrap().accounts.is_empty());
    assert!(!next_run.has_session("once"));
    clone.logout("once").unwrap();
    assert!(lithic.accounts().unwrap().accounts.is_empty());
  }
}
