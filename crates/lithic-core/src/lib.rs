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
use moddb::ModDb;

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
}
