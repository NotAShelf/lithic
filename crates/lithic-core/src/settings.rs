use std::collections::BTreeSet;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Contents of `settings.toml`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
   pub active_instance: Option<String>,
   pub mods: ModSettings,
   pub backups: BackupSettings,
   pub game: GameSettings,
   pub gui: GuiSettings,
   pub cli: CliSettings,
   /// Keys this version does not know about. Kept so that editing settings
   /// with an older lithic does not drop what a newer one wrote.
   #[serde(flatten)]
   pub extra: toml::Table,
}

impl Default for Settings {
   fn default() -> Self {
      Self {
         active_instance: None,
         mods: ModSettings::default(),
         backups: BackupSettings::default(),
         game: GameSettings::default(),
         gui: GuiSettings::default(),
         cli: CliSettings::default(),
         extra: toml::Table::new(),
      }
   }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ModSettings {
   /// Offer `-dev`, `-pre` and `-rc` mod releases when a stable one exists.
   pub allow_prerelease: bool,
   /// How old the cached `ModDB` index may get before it is fetched again.
   pub index_max_age_hours: u32,
   /// Parallel downloads and API requests.
   pub concurrency: usize,
}

impl Default for ModSettings {
   fn default() -> Self {
      Self {
         allow_prerelease: false,
         index_max_age_hours: 6,
         concurrency: 6,
      }
   }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct BackupSettings {
   /// Copy a mod's old file aside before replacing or removing it.
   pub enabled: bool,
   /// Defaults to `<data>/backups`.
   pub dir: Option<PathBuf>,
   /// Old files kept per mod.
   pub keep: usize,
}

impl Default for BackupSettings {
   fn default() -> Self {
      Self {
         enabled: false,
         dir: None,
         keep: 3,
      }
   }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct GameSettings {
   /// Where downloaded game builds are unpacked. Defaults to `<data>/game`.
   pub install_dir: Option<PathBuf>,
   /// Where `lithic game download` saves archives. Defaults to the user's
   /// download directory.
   pub download_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct GuiSettings {
   /// `system`, `light`, `dark`, or `preset`.
   pub theme_mode: String,
   pub theme_preset: String,
   pub initial_page: String,
   pub favorites: BTreeSet<String>,
}

impl Default for GuiSettings {
   fn default() -> Self {
      Self {
         theme_mode: "system".to_string(),
         theme_preset: String::new(),
         initial_page: "instances".to_string(),
         favorites: BTreeSet::new(),
      }
   }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CliSettings {
   /// Per-column table colours. Interpreted by the CLI only.
   pub table: toml::Table,
}

#[cfg(test)]
#[expect(
   clippy::unwrap_used,
   reason = "test setup and assertions intentionally fail on error"
)]
mod tests {
   use super::*;

   #[test]
   fn empty_file_gives_defaults() {
      let s: Settings = toml::from_str("").unwrap();
      assert_eq!(s, Settings::default());
   }

   #[test]
   fn unknown_keys_survive_a_round_trip() {
      let text =
         "future_flag = true\n[future_section]\nx = 1\n[mods]\nconcurrency = 2\nnew_mod_key = \"y\"\n";
      let s: Settings = toml::from_str(text).unwrap();
      assert_eq!(s.mods.concurrency, 2);
      let out = toml::to_string(&s).unwrap();
      let back: Settings = toml::from_str(&out).unwrap();
      assert_eq!(back.extra.get("future_flag"), Some(&toml::Value::Boolean(true)));
      assert!(back.extra.contains_key("future_section"));
   }
}
