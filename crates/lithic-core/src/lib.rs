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
    self.mod_index_for(None, freshness).await
  }

  /// Like [`Self::mod_index`], but with `minor` set only lists mods that have
  /// a release for that major.minor game version. Each version is cached
  /// separately, with the same maximum age.
  ///
  /// # Errors
  ///
  /// Returns an error if settings cannot be read, or if the request fails
  /// without a usable cache.
  pub async fn mod_index_for(
    &self,
    minor: Option<(u64, u64)>,
    freshness: Freshness,
  ) -> Result<ModIndex> {
    let path = minor.map_or_else(
      || self.paths.mod_index_file(),
      |m| self.paths.mod_index_file_for(m),
    );
    let max_age = f64::from(self.settings()?.mods.index_max_age_hours);
    let cached = match ModIndex::load_cached(&path) {
      Some(index)
        if freshness == Freshness::Cached && index.age_hours() < max_age =>
      {
        return Ok(index);
      },
      cached => cached,
    };
    let fetched = match minor {
      None => self.moddb.mods(&moddb::Query::default()).await,
      Some(minor) => {
        match self.moddb.game_versions().await {
          Ok(tags) => {
            let game_versions = tags
              .iter()
              .filter(|tag| version::minor(&tag.name) == Some(minor))
              .map(|tag| tag.tag_id)
              .collect();
            self
              .moddb
              .mods(&moddb::Query {
                game_versions,
                ..moddb::Query::default()
              })
              .await
          },
          Err(e) => Err(e),
        }
      },
    };
    match fetched {
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

#[cfg(test)]
#[expect(
  clippy::unwrap_used,
  reason = "test setup and assertions intentionally fail on error"
)]
mod tests {
  use super::*;

  #[tokio::test]
  async fn version_lists_are_cached_per_version() {
    let dir = tempfile::tempdir().unwrap();
    let mut lithic = Lithic::new(Paths::rooted(dir.path())).unwrap();
    // Nothing listens here, so any request fails.
    lithic.moddb = ModDb::with_base(lithic.http.clone(), "http://127.0.0.1:9");
    let index = |name: &str, fetched_at| {
      ModIndex {
        fetched_at,
        mods: vec![moddb::ModSummary {
          name: name.into(),
          ..moddb::ModSummary::default()
        }],
      }
    };
    index("fresh", fsutil::now_ms())
      .save(&lithic.paths.mod_index_file_for((1, 21)))
      .unwrap();
    let got = lithic
      .mod_index_for(Some((1, 21)), Freshness::Cached)
      .await
      .unwrap();
    assert_eq!(got.mods[0].name, "fresh");

    index("stale", 0)
      .save(&lithic.paths.mod_index_file_for((1, 20)))
      .unwrap();
    let got = lithic
      .mod_index_for(Some((1, 20)), Freshness::Cached)
      .await
      .unwrap();
    assert_eq!(got.mods[0].name, "stale", "a failed refresh falls back");
    assert!(
      lithic.mod_index_for(None, Freshness::Cached).await.is_err(),
      "each version has its own cache"
    );
  }
}
