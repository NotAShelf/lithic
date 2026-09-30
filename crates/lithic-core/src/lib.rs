//! Core operations for instances, mods, game builds, and accounts.

pub mod auth;
pub mod error;
pub mod fsutil;
pub mod game;
pub mod http;
pub mod instance;
pub mod launch;
pub mod migrate;
pub mod moddb;
pub mod modinfo;
pub mod mods;
pub mod pack;
pub mod paths;
pub mod progress;
pub mod settings;
pub mod version;

use std::sync::{Arc, Mutex};

pub use error::{Error, Kind, Result};
use http::Http;
pub use instance::Instance;
use moddb::{ModDb, ModIndex};
pub use paths::Paths;
pub use progress::{Cancel, Event, Reporter, Step};
pub use settings::Settings;

/// Entry point to all operations. Cheap to clone.
#[derive(Debug, Clone)]
pub struct Lithic {
  pub paths: Paths,
  pub http:  Http,
  pub moddb: ModDb,
  /// Sign-ins that last only as long as this process.
  transient: Arc<Mutex<auth::TransientAccounts>>,
}

/// Whether to use cached remote data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Freshness {
  /// Use the cache if it is younger than the configured maximum age.
  Cached,
  /// Always fetch.
  Refresh,
}

impl Lithic {
  /// Creates the HTTP and `ModDB` clients for these paths.
  ///
  /// # Errors
  ///
  /// Returns an error if the HTTP client cannot be built.
  pub fn new(paths: Paths) -> Result<Self> {
    let http = Http::new()?;
    Ok(Self {
      moddb: ModDb::new(http.clone()),
      paths,
      http,
      transient: Arc::new(Mutex::new(auth::TransientAccounts::default())),
    })
  }

  /// Creates a handle using the configured directories, and removes game
  /// sessions an earlier run could not (see [`Self::clean_up_sessions`]).
  ///
  /// # Errors
  ///
  /// Returns an error if the directories cannot be resolved or the HTTP
  /// client cannot be built.
  pub fn from_env() -> Result<Self> {
    let lithic = Self::new(Paths::from_env()?)?;
    if let Err(e) = lithic.clean_up_sessions() {
      tracing::warn!("could not remove leftover game logins: {e}");
    }
    Ok(lithic)
  }

  /// Reads settings, using defaults if no settings file exists.
  ///
  /// # Errors
  ///
  /// Returns an error if the settings file cannot be read or parsed.
  pub fn settings(&self) -> Result<Settings> {
    Ok(fsutil::read_toml(&self.paths.settings_file())?.unwrap_or_default())
  }

  /// Re-reads the settings under a file lock before applying `f`.
  ///
  /// # Errors
  ///
  /// Returns an error if locking, reading, or writing the settings fails.
  pub fn update_settings<R>(
    &self,
    f: impl FnOnce(&mut Settings) -> R,
  ) -> Result<R> {
    fsutil::update_toml(&self.paths.settings_file(), |s: &mut Settings| {
      Ok(f(s))
    })
  }

  /// Returns the `ModDB` mod list, using stale cached data if a refresh fails.
  ///
  /// # Errors
  ///
  /// Returns an error if settings cannot be read, or if the request fails
  /// without a usable cache.
  pub async fn mod_index(&self, freshness: Freshness) -> Result<ModIndex> {
    let path = self.paths.mod_index_file();
    let max_age = f64::from(self.settings()?.mods.index_max_age_hours);
    let cached = match ModIndex::load_cached(&path) {
      Some(index)
        if freshness == Freshness::Cached && index.age_hours() < max_age =>
      {
        return Ok(index);
      },
      cached => cached,
    };
    match self.moddb.mods(&moddb::Query::default()).await {
      Ok(mods) => {
        let index = ModIndex {
          fetched_at: fsutil::now_ms(),
          mods,
        };
        if let Err(e) = index.save(&path) {
          tracing::warn!("could not cache the mod index: {e}");
        }
        Ok(index)
      },
      Err(e) => {
        match cached {
          Some(index) => {
            tracing::warn!("using a stale mod index, refresh failed: {e}");
            Ok(index)
          },
          None => Err(e),
        }
      },
    }
  }
}
