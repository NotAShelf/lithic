//! Vintage Story accounts: logging in, remembering sessions, and handing the
//! right one to the game at launch.
//!
//! Account names and ids live in `accounts.toml`; the secret session lives in
//! the keyring or a private file (see [`store`]).

pub mod client;
pub mod clientsettings;
pub mod store;

use serde::{Deserialize, Serialize};

pub use client::{AuthError, LoginResponse};
pub use store::Session;

use crate::Lithic;
use crate::error::{Error, Kind, Result};
use crate::fsutil;
use crate::instance::Instance;
use clientsettings::AccountIdentity;
use store::SessionStore;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Account {
   pub uid: String,
   pub playername: String,
   #[serde(default)]
   pub email: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Accounts {
   /// Account used for instances that do not name one.
   pub active: Option<String>,
   #[serde(rename = "account")]
   pub accounts: Vec<Account>,
}

impl Accounts {
   pub fn get(&self, uid: &str) -> Option<&Account> {
      self.accounts.iter().find(|a| a.uid == uid)
   }

   /// The account an instance launches with: its own choice if that account
   /// still exists, otherwise the active one.
   pub fn for_instance(&self, instance_account: Option<&str>) -> Option<&Account> {
      instance_account
         .and_then(|uid| self.get(uid))
         .or_else(|| self.active.as_deref().and_then(|uid| self.get(uid)))
   }
}

impl Lithic {
   fn sessions(&self) -> SessionStore {
      SessionStore::new(self.paths.sessions_dir())
   }

   pub fn accounts(&self) -> Result<Accounts> {
      Ok(fsutil::read_toml(&self.paths.accounts_file())?.unwrap_or_default())
   }

   /// One login attempt. On [`AuthError::TwoFactorRequired`], call again with
   /// `twofa` set to the returned token and the user's code.
   pub async fn login(&self, email: &str, password: &str, twofa: Option<(&str, &str)>) -> Result<Account> {
      let response = client::gamelogin(&self.http, email, password, twofa).await?;
      self.save_login(email, response)
   }

   /// Stores a successful login. The first account becomes the active one.
   pub fn save_login(&self, email: &str, login: LoginResponse) -> Result<Account> {
      if login.uid.is_empty() {
         return Err(Error::Auth(AuthError::Server(
            "the login succeeded but carried no account id".into(),
         )));
      }
      let session = Session {
         sessionkey: login.sessionkey,
         sessionsignature: login.sessionsignature,
         mptoken: login.mptoken,
         entitlements: login.entitlements,
      };
      self.sessions().save(&login.uid, &session)?;

      let account = Account {
         uid: login.uid,
         playername: login.playername,
         email: email.trim().to_string(),
      };
      let saved = account.clone();
      fsutil::update_toml(&self.paths.accounts_file(), move |a: &mut Accounts| {
         match a.accounts.iter_mut().find(|x| x.uid == account.uid) {
            Some(existing) => *existing = account.clone(),
            None => a.accounts.push(account.clone()),
         }
         if a.active.is_none() {
            a.active = Some(account.uid);
         }
         Ok(())
      })?;
      Ok(saved)
   }

   pub fn set_active_account(&self, uid: &str) -> Result<()> {
      fsutil::update_toml(&self.paths.accounts_file(), |a: &mut Accounts| {
         if a.get(uid).is_none() {
            return Err(Error::not_found(Kind::Account, uid));
         }
         a.active = Some(uid.to_string());
         Ok(())
      })
   }

   /// Forgets an account and deletes its session. If it was active, no
   /// account is active afterwards.
   pub fn logout(&self, uid: &str) -> Result<()> {
      self.sessions().delete(uid)?;
      fsutil::update_toml(&self.paths.accounts_file(), |a: &mut Accounts| {
         a.accounts.retain(|x| x.uid != uid);
         if a.active.as_deref() == Some(uid) {
            a.active = None;
         }
         Ok(())
      })
   }

   pub fn has_session(&self, uid: &str) -> bool {
      self.sessions().load(uid).is_some()
   }

   /// Writes the instance's account into its `clientsettings.json` before a
   /// launch. Without any account this does nothing and the game shows its
   /// own login screen.
   pub fn inject_account(&self, instance: &Instance) -> Result<Option<Account>> {
      let accounts = self.accounts()?;
      let Some(account) = accounts.for_instance(instance.account.as_deref()) else {
         return Ok(None);
      };
      let session = self.sessions().load(&account.uid).ok_or_else(|| {
         Error::Auth(AuthError::InvalidCredentials(format!(
            "no stored session for {}; log in again",
            account.playername
         )))
      })?;
      clientsettings::inject_account(
         &instance.data_dir(),
         &AccountIdentity {
            uid: &account.uid,
            playername: &account.playername,
            session: &session,
         },
      )?;
      Ok(Some(account.clone()))
   }
}

#[cfg(test)]
mod tests {
   use super::*;

   fn accounts() -> Accounts {
      Accounts {
         active: Some("a".into()),
         accounts: vec![
            Account {
               uid: "a".into(),
               playername: "A".into(),
               email: String::new(),
            },
            Account {
               uid: "b".into(),
               playername: "B".into(),
               email: String::new(),
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
      let login = |uid: &str| LoginResponse {
         uid: uid.into(),
         playername: uid.to_uppercase(),
         sessionkey: "k".into(),
         ..LoginResponse::default()
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
}
