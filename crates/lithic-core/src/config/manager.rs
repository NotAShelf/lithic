use crate::aliases::ModVersion;
use crate::config::structs::Tables;
use crate::errors::LithicError;
use crate::instance::{GameVersionInstall, InstanceConfig};
use crate::options::LithicOptions;
use crate::utils::{CellData, LithicMessage, lithic_message};
use chrono::Local;
use comfy_table::{Attribute, CellAlignment, Color};
use dirs::home_dir;
use serde::{Deserialize, Serialize};
use std::fs;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::path::PathBuf;
use std::sync::OnceLock;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};
use yansi::Paint;

#[derive(Deserialize, Serialize, Debug)]
#[allow(clippy::struct_excessive_bools)]
#[serde(default)]
pub struct Config {
   /// this sets the default mod dir so you don't have to type -m everytime
   #[serde(default)]
   pub mod_dir: PathBuf,
   // this tells lithic which versions of the game to download mods for.
   // It will download mods up to this version and not over
   #[serde(default)]
   pub pinned_game_version: String,
   // automatically zips mod folders that are unzipped during the sync process
   #[serde(default)]
   pub zip_mod_files: bool,
   // create a backup of each mod before its updated.
   #[serde(default)]
   pub backup_mods: bool,

   // location for the mod backups
   // default ~/.config/lithic/backups
   #[serde(default)]
   pub backup_mods_dir: PathBuf,

   #[cfg(windows)]
   pub update_default_windows_loc: bool,

   // Shows the "<operation> completed: " text after a command finishes
   #[serde(default)]
   pub show_execution_time: bool,

   #[serde(default)]
   pub notify_of_unzipped_mods: bool,

   #[serde(default)]
   pub game_download_dir: PathBuf,

   #[serde(default)]
   pub check_for_updates: bool,

   #[serde(default)]
   pub modpacks: ModPacks,

   #[serde(default)]
   pub pkg: Vec<Package>,

   #[serde(default = "default_sync_time")]
   pub sync_latest_game_version_file_every: i64,

   #[serde(default = "default_sync_time")]
   pub sync_mod_search_file_every: i64,

   #[serde(default)]
   pub table: Tables,

   #[serde(default)]
   pub instances: Vec<InstanceConfig>,

   #[serde(default)]
   pub active_instance_id: Option<String>,

   #[serde(default)]
   pub game_versions: Vec<GameVersionInstall>,

   #[serde(default = "default_theme_mode")]
   pub theme_mode: String,

   #[serde(default)]
   pub theme_preset: String,

   #[serde(default = "default_initial_page")]
   pub initial_page: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ModPacks {
   #[serde(default)]
   pub modpack_dir: PathBuf,
   #[serde(default)]
   pub enabled: Vec<String>,
   #[serde(default)]
   pub disabled: Vec<String>,
}

// Manually set the default since we need the default modpack_dir to be set to something specific
// Otherwise its set to a blank string which will make modpacks installs fail.
impl Default for ModPacks {
   fn default() -> Self {
      Self {
         modpack_dir: Config::data_path().join("modpacks"),
         enabled: vec![],
         disabled: vec![],
      }
   }
}

#[derive(Deserialize, Serialize, Debug, Clone, Default)]
pub struct Package {
   pub mod_id: String,
   #[serde(default)]
   pub pinned_version: Option<ModVersion>,
}

fn default_sync_time() -> i64 {
   24
}

fn default_theme_mode() -> String {
   "system".to_string()
}

fn default_initial_page() -> String {
   "browse".to_string()
}

impl Config {
   pub fn get_path() -> PathBuf {
      if cfg!(target_os = "windows") {
         if let Some(w_path) = std::env::var_os("APPDATA") {
            PathBuf::from(w_path).join("lithic")
         } else {
            PathBuf::from("../..").join("lithic")
         }
      } else if let Some(config_dir) = dirs::config_dir() {
         config_dir.join("lithic")
      } else if let Some(u_path) = home_dir() {
         u_path.join(".config").join("lithic")
      } else {
         PathBuf::from("../..").join("lithic")
      }
   }

   pub fn data_path() -> PathBuf {
      if cfg!(target_os = "windows") {
         Self::get_path()
      } else if let Some(data_dir) = dirs::data_dir() {
         data_dir.join("lithic")
      } else if let Some(u_path) = home_dir() {
         u_path.join(".local").join("share").join("lithic")
      } else {
         Self::get_path()
      }
   }
}

impl Default for Config {
   fn default() -> Self {
      // let backup_mods_dir = get_expanded_path(PathBuf::from(CONFIG_DEFAULT_DIR).join("mod_backups"));
      let backup_mods_dir = Self::data_path().join("mod_backups");
      let modpack_dir = Self::data_path().join("modpacks");

      match Self::setup_modpack_dir("modpacks") {
         Ok(_) => {}
         Err(e) => {
            warn!("Failed to setup modpack dir: {}", e);
         }
      }

      info!("modpack_dir {}", modpack_dir.display());

      Self {
         mod_dir: LithicOptions::default().mod_dir.unwrap_or_default(),
         pinned_game_version: String::new(), // if its empty then get the latest
         zip_mod_files: false,
         backup_mods: false,
         backup_mods_dir,
         show_execution_time: true,
         notify_of_unzipped_mods: false,
         game_download_dir: dirs::download_dir().unwrap_or_default(),
         sync_latest_game_version_file_every: 24,
         sync_mod_search_file_every: 24,
         pkg: Vec::default(),
         table: Tables::with_defaults(),
         modpacks: ModPacks::default(),
         check_for_updates: true,
         instances: Vec::new(),
         active_instance_id: None,
         game_versions: Vec::new(),
         theme_mode: default_theme_mode(),
         theme_preset: String::new(),
         initial_page: default_initial_page(),

         #[cfg(windows)]
         update_default_windows_loc: true,
      }
   }
}

impl Config {
   pub fn new() -> Result<Config, LithicError> {
      let config_path = Self::get_path();

      info!("config_path: {}", config_path.display());

      if !config_path.exists() {
         fs::create_dir_all(&config_path)
            .map_err(|e| LithicError::ConfigFileError(format!("Failed to create config directory: {e}")))?;
      }

      let config_file_path = config_path.join("config.toml");

      if !config_file_path.exists() {
         let default_config = Self::default();
         let toml_content = toml::to_string_pretty(&default_config)
            .map_err(|e| LithicError::ConfigFileError(format!("Failed to serialize default config: {e}")))?;

         crate::utils::write_atomic_sync(&config_file_path, toml_content.as_bytes()).map_err(|e| {
            LithicError::ConfigFileError(format!(
               "Failed to write default config file {}: {e}",
               config_file_path.display()
            ))
         })?;

         println!(
            "{} {}",
            "Successfully created config file: ".green(),
            config_file_path.display().to_string().bright_yellow()
         );
         return Ok(default_config);
      }

      // make sure the modpack_dir is setup early
      Self::setup_modpack_dir("modpacks")?;

      // if config exists load and parse it
      let mut file = File::open(&config_file_path)
         .map_err(|e| LithicError::ConfigFileError(format!("Failed to open config file: {e}")))?;

      let mut contents = String::new();
      file
         .read_to_string(&mut contents)
         .map_err(|e| LithicError::ConfigFileError(format!("Failed to read config file: {e}")))?;

      match toml::from_str::<Config>(&contents) {
         Ok(config) => Ok(config),
         Err(e) => {
            backup_config(&config_file_path, Some(e.to_string()))?;

            // write the default
            let config = Config::default();
            config.save(Option::from(Config::get_path()))?;

            Ok(config)
         }
      }
   }

   pub fn setup_modpack_dir(modpack_dir: impl AsRef<Path>) -> Result<(), LithicError> {
      let modpack_dir = Self::data_path().join(modpack_dir);
      // create the modpack directory if it hasn't been created
      debug!("Checking if {} exists", modpack_dir.to_string_lossy());
      if !&modpack_dir.exists() {
         info!("Created modpack directory");

         for dir in ["installed", "packs", "mypacks"] {
            info!("creating modpacks/{dir}");
            let d = &modpack_dir.join(dir);
            fs::create_dir_all(d).map_err(|e| {
               LithicError::SimpleError(format!("Failed to create {}: {}", d.to_string_lossy(), e))
            })?;
         }
      }

      Ok(())
   }

   /// Persist the config to `<config_dir>/config.toml`. Writes are atomic
   /// (temp file + rename) so a crash mid-write cannot corrupt the live file.
   pub fn save(&self, config_dir: Option<PathBuf>) -> Result<(), LithicError> {
      ensure_config_can_save(CONFIG_LOAD_ERROR.get().map(String::as_str))?;
      let config_path = config_dir.unwrap_or_else(Self::get_path);
      let config_file_path = config_path.join("config.toml");

      let toml_content = toml::to_string_pretty(self)
         .map_err(|e| LithicError::ConfigFileError(format!("Failed to serialize config: {e}")))?;

      crate::utils::write_atomic_sync(&config_file_path, toml_content.as_bytes()).map_err(|e| {
         LithicError::ConfigFileError(format!(
            "Failed to write config file {}: {e}",
            config_file_path.display()
         ))
      })
   }
}

pub fn backup_config(config_path: impl AsRef<Path>, message: Option<String>) -> Result<(), LithicError> {
   let config_path = config_path.as_ref();
   if config_path.exists() {
      let back_name = format!("toml.bak-{}", Local::now().format("%Y%m%d_%H%M%S"));
      let backup_path = config_path.with_extension(&back_name);

      let h1 = CellData::new(
         "Lithic has discovered an error with your config.toml file".to_string(),
         Some(Color::Magenta),
         vec![Attribute::Bold],
         None,
      );

      let m1 = CellData::new(
         "Your old config has been backed up to the following location:".to_string(),
         Some(Color::Yellow),
         vec![],
         None,
      );

      let m2 = CellData::new(
         format!("{}", config_path.with_extension(&back_name).display()),
         Some(Color::Green),
         vec![Attribute::Bold],
         None,
      );

      let m3 = CellData::new(
          "A new config has been written using default values. You will need to set your configuration options again.".to_string(),
          Some(Color::Yellow),
          vec![],None,
        );

      let m4 = CellData::new(String::new(), None, vec![], None);
      let m5 = CellData::new(
         message.unwrap_or_default(),
         Some(Color::Red),
         vec![Attribute::Bold, Attribute::Italic],
         Some(CellAlignment::Left),
      );

      lithic_message(LithicMessage {
         header: Some(h1),
         message: vec![m1, m2, m3, m4, m5],
      });

      fs::copy(config_path, &backup_path)?;
   }

   Ok(())
}

static CONFIG: OnceLock<RwLock<Config>> = OnceLock::new();
static CONFIG_LOAD_ERROR: OnceLock<String> = OnceLock::new();

fn ensure_config_can_save(load_error: Option<&str>) -> Result<(), LithicError> {
   load_error.map_or(Ok(()), |error| {
      Err(LithicError::ConfigFileError(format!(
         "refusing to overwrite config after a load failure: {error}"
      )))
   })
}

// Initiate the CONFIG in the main file so its ready everywhere else
pub fn init_config() -> Result<(), LithicError> {
   let config = Config::new().inspect_err(|e| {
      let _ = CONFIG_LOAD_ERROR.set(e.to_string());
   })?;

   if CONFIG.set(RwLock::new(config)).is_err() {
      return Err(LithicError::ConfigFileError(
         "Config has already been initialized".to_string(),
      ));
   }

   Ok(())
}

/// Return the process-wide config handle.
///
/// `init_config` should be called at startup so the returned handle reflects
/// the on-disk file. If something goes wrong and `get_config` is reached
/// before initialisation, it falls back to in-memory defaults. If loading
/// failed, [`Config::save`] refuses to overwrite the on-disk configuration.
pub fn get_config() -> &'static RwLock<Config> {
   CONFIG.get_or_init(|| {
      let config = Config::new().unwrap_or_else(|e| {
         let _ = CONFIG_LOAD_ERROR.set(e.to_string());
         tracing::error!("config not initialised and lazy load failed ({e}); using in-memory defaults");
         Config::default()
      });
      RwLock::new(config)
   })
}

#[cfg(test)]
mod tests {
   use super::ensure_config_can_save;

   #[test]
   fn config_save_is_blocked_after_a_load_failure() {
      assert!(ensure_config_can_save(None).is_ok());
      assert!(ensure_config_can_save(Some("invalid TOML")).is_err());
   }
}
