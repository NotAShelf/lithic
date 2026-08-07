use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

/// Where lithic keeps its files.
///
/// `config` holds settings, `data` holds instances and game
/// builds, `cache` holds things that can be re-downloaded at any time. Each
/// root can be overridden with `LITHIC_CONFIG_DIR`, `LITHIC_DATA_DIR` and
/// `LITHIC_CACHE_DIR`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
   pub config: PathBuf,
   pub data: PathBuf,
   pub cache: PathBuf,
}

impl Paths {
   pub fn from_env() -> Result<Self> {
      let pick = |var: &str, base: Option<PathBuf>| -> Result<PathBuf> {
         if let Some(dir) = std::env::var_os(var).filter(|v| !v.is_empty()) {
            return Ok(PathBuf::from(dir));
         }
         base
            .map(|b| b.join("lithic"))
            .ok_or_else(|| Error::invalid(format!("cannot determine a default directory; set {var}")))
      };

      Ok(Self {
         config: pick("LITHIC_CONFIG_DIR", dirs::config_dir())?,
         data: pick("LITHIC_DATA_DIR", dirs::data_dir())?,
         cache: pick("LITHIC_CACHE_DIR", dirs::cache_dir())?,
      })
   }

   /// All three roots under one directory. Used by tests and portable setups.
   pub fn rooted(root: impl AsRef<Path>) -> Self {
      let root = root.as_ref();
      Self {
         config: root.join("config"),
         data: root.join("data"),
         cache: root.join("cache"),
      }
   }

   pub fn settings_file(&self) -> PathBuf {
      self.config.join("settings.toml")
   }

   pub fn instances_dir(&self) -> PathBuf {
      self.data.join("instances")
   }

   pub fn instance_dir(&self, id: &str) -> PathBuf {
      self.instances_dir().join(id)
   }

   pub fn game_dir(&self) -> PathBuf {
      self.data.join("game")
   }

   pub fn game_registry_file(&self) -> PathBuf {
      self.game_dir().join("installs.toml")
   }

   pub fn backups_dir(&self) -> PathBuf {
      self.data.join("backups")
   }

   pub fn downloads_dir(&self) -> PathBuf {
      self.cache.join("downloads")
   }

   pub fn game_manifest_file(&self) -> PathBuf {
      self.cache.join("game-manifest.json")
   }
}

/// Expands a leading `~` to the home directory.
pub fn expand_home(path: impl AsRef<Path>) -> PathBuf {
   let path = path.as_ref();
   let Ok(rest) = path.strip_prefix("~") else {
      return path.to_path_buf();
   };
   match dirs::home_dir() {
      Some(home) => home.join(rest),
      None => path.to_path_buf(),
   }
}

/// Data directories the stock game launcher uses, in the order they are
/// checked. Only existing directories are returned.
pub fn stock_game_data_dirs() -> Vec<PathBuf> {
   let mut candidates = Vec::new();
   if let Some(config) = dirs::config_dir() {
      candidates.push(config.join("VintagestoryData"));
   }
   if let Some(home) = dirs::home_dir() {
      candidates.push(home.join(".config").join("VintagestoryData"));
      candidates.push(
         home
            .join(".var")
            .join("app")
            .join("at.vintagestory.VintageStory")
            .join("config")
            .join("VintagestoryData"),
      );
   }
   let mut seen = Vec::new();
   for dir in candidates {
      if dir.is_dir() && !seen.contains(&dir) {
         seen.push(dir);
      }
   }
   seen
}

#[cfg(test)]
mod tests {
   use super::*;

   #[test]
   fn tilde_expands_to_home() {
      let home = dirs::home_dir().unwrap();
      assert_eq!(expand_home("~/x/y"), home.join("x/y"));
      assert_eq!(expand_home("~"), home);
      assert_eq!(expand_home("/abs/path"), PathBuf::from("/abs/path"));
      assert_eq!(expand_home("rel/~"), PathBuf::from("rel/~"));
   }
}
