//! Core operations for instances, mods, and game builds.

pub mod error;
pub mod fsutil;
pub mod game;
pub mod http;
pub mod instance;
pub mod launch;
pub mod moddb;
pub mod modinfo;
pub mod mods;
pub mod paths;
pub mod progress;
pub mod settings;
pub mod version;

pub use error::{Error, Kind, Result};
pub use instance::Instance;
pub use paths::Paths;
pub use progress::{Cancel, Event, Reporter, Step};
pub use settings::Settings;

use http::Http;
use moddb::{ModDb, ModIndex};

/// Entry point to all operations. Cheap to clone.
#[derive(Debug, Clone)]
pub struct Lithic {
   pub paths: Paths,
   pub http: Http,
   pub moddb: ModDb,
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
   pub fn new(paths: Paths) -> Result<Self> {
      let http = Http::new()?;
      Ok(Self {
         moddb: ModDb::new(http.clone()),
         paths,
         http,
      })
   }

   pub fn from_env() -> Result<Self> {
      Self::new(Paths::from_env()?)
   }

   pub fn settings(&self) -> Result<Settings> {
      Ok(fsutil::read_toml(&self.paths.settings_file())?.unwrap_or_default())
   }

   /// Applies `f` to the settings on disk, re-reading them first so changes
   /// made by another lithic process in the meantime are kept.
   pub fn update_settings<R>(&self, f: impl FnOnce(&mut Settings) -> R) -> Result<R> {
      fsutil::update_toml(&self.paths.settings_file(), |s: &mut Settings| Ok(f(s)))
   }

   /// The ModDB mod list. A failed refresh falls back to a stale cache when
   /// there is one.
   pub async fn mod_index(&self, freshness: Freshness) -> Result<ModIndex> {
      let path = self.paths.mod_index_file();
      let max_age = f64::from(self.settings()?.mods.index_max_age_hours);
      let cached = match ModIndex::load_cached(&path) {
         Some(index) if freshness == Freshness::Cached && index.age_hours() < max_age => return Ok(index),
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
         }
         Err(e) => match cached {
            Some(index) => {
               tracing::warn!("using a stale mod index, refresh failed: {e}");
               Ok(index)
            }
            None => Err(e),
         },
      }
   }
}
