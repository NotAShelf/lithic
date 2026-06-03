use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

use futures::future::join_all;
use lithic_core::api::client::ApiClient;
use lithic_core::api::client::{VSExecutabletype, VSOSType, VSWinInstallerType};
use lithic_core::api::structs::{ModApi, ModInfo, ModsSearchFile};
use lithic_core::config::manager::{Config, get_config};
use lithic_core::consts::{FILE_GAME_VERSION_SYNC, FILE_LITHIC_SYNC, FILE_MOD_SEARCH_SYNC};
use lithic_core::installer::manager::{Install, install_manager};
use lithic_core::instance::{GameVersionInstall, GameVersionInstallEvent, InstanceConfig};
use lithic_core::search::SearchQuery;
use lithic_core::sync::structs::{GameVersionSync, LithicSyncJson, ModSyncInfo};
use lithic_core::utils::{
   extract_all_mods_metadata, get_current_time, parse_json_file, prettify, write_json_file,
};
use lithic_core::version::filter::{VersionFilter, minor_versions_at_least, unique_minor_versions};
use lithic_core::version::manager::parse_latest_version;
use lithic_locale::locale;

const FAVORITES_FILE: &str = "lithic-gui-favorites.json";

fn err(e: impl ToString) -> String {
   e.to_string()
}

/// Convenience: localized message body for a fluent id with no args.
fn loc_msg(id: &str) -> String {
   locale().get(id).into_owned()
}

#[derive(Debug, Clone, Default)]
pub struct SettingsData {
   pub mod_dir: String,
   pub game_download_dir: String,
   pub pinned_game_version: String,
   pub zip_mod_files: bool,
   pub backup_mods: bool,
   pub backup_mods_dir: String,
   pub notify_of_unzipped_mods: bool,
   pub check_for_updates: bool,
   pub show_execution_time: bool,
   pub modpack_dir: String,
   pub theme_mode: String,
   pub theme_preset: String,
   pub initial_page: String,
}

#[derive(Debug, Clone, Default)]
pub struct InstanceFormData {
   pub id: String,
   pub name: String,
   pub data_dir: String,
   pub mods_dir: String,
   pub game_version_id: String,
   pub start_params: String,
   pub env_vars: String,
   pub selected_mod_ids: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct GameInstallProgress {
   pub active: bool,
   pub stage: String,
   pub percent: Option<u8>,
   pub logs: Vec<String>,
   pub done: bool,
   pub error: Option<String>,
}

pub type SharedGameInstallProgress = Arc<Mutex<GameInstallProgress>>;

pub fn new_game_install_progress() -> SharedGameInstallProgress {
   Arc::new(Mutex::new(GameInstallProgress::default()))
}

pub async fn load_installed() -> Result<HashMap<String, ModSyncInfo>, String> {
   let mod_dir = lithic_core::instance::resolve_active_mod_dir().await?;
   load_installed_from(mod_dir).await
}

pub async fn load_installed_from(mod_dir: PathBuf) -> Result<HashMap<String, ModSyncInfo>, String> {
   if mod_dir.as_os_str().is_empty() || !mod_dir.exists() {
      return Ok(HashMap::new());
   }
   let sync_file = mod_dir.join(FILE_LITHIC_SYNC);
   if sync_file.exists() {
      let data = parse_json_file::<LithicSyncJson>(&sync_file).await.map_err(err)?;
      return Ok(data.lithic_sync);
   }
   build_basic_installed(&mod_dir).await
}

async fn build_basic_installed(mod_dir: &Path) -> Result<HashMap<String, ModSyncInfo>, String> {
   let installed = extract_all_mods_metadata(mod_dir, true).await.map_err(err)?;
   let map = installed
      .into_iter()
      .map(|(file_name, info)| {
         let key = if info.mod_id.is_empty() {
            file_name.to_string()
         } else {
            info.mod_id.to_string()
         };
         let sync = ModSyncInfo {
            file_name: file_name.clone(),
            mod_name: info.name,
            installed_version: info.version.unwrap_or_default(),
            ..ModSyncInfo::default()
         };
         (key, sync)
      })
      .collect();
   Ok(map)
}

pub async fn sync_mods(mod_dir: PathBuf) -> Result<HashMap<String, ModSyncInfo>, String> {
   if mod_dir.as_os_str().is_empty() || !mod_dir.exists() {
      return Err(loc_msg("ops-mods-dir-missing"));
   }
   let scanned = build_basic_installed(&mod_dir).await?;

   let sync_file = mod_dir.join(FILE_LITHIC_SYNC);
   if !sync_file.exists() {
      return Ok(scanned);
   }

   let mut sync_map = parse_json_file::<LithicSyncJson>(&sync_file)
      .await
      .map(|d| d.lithic_sync)
      .unwrap_or_default();

   for (id, info) in scanned {
      sync_map.entry(id).or_insert(info);
   }

   let sync = LithicSyncJson {
      lithic_sync: sync_map.clone(),
      last_sync: get_current_time(),
   };
   sync.save(&sync_file).await.map_err(err)?;

   Ok(sync_map)
}

pub async fn update_all(mod_dir: PathBuf) -> Result<(), String> {
   if mod_dir.as_os_str().is_empty() || !mod_dir.exists() {
      return Err(loc_msg("ops-mods-dir-not-configured"));
   }
   let sync_file = mod_dir.join(FILE_LITHIC_SYNC);
   if !sync_file.exists() {
      return Err(loc_msg("ops-no-sync-data"));
   }
   let sync_data = parse_json_file::<LithicSyncJson>(&sync_file).await.map_err(err)?;

   let mods_needing_update: Vec<Install> = sync_data
      .lithic_sync
      .iter()
      .filter(|(_, info)| {
         !info.is_symlink
            && !info.latest_known_version.is_empty()
            && info.installed_version != info.latest_known_version
      })
      .map(|(mod_id, info)| Install {
         mod_id: mod_id.clone().into(),
         mod_name: info.mod_name.clone().into(),
         download_url: info.latest_download_url.clone().into(),
         version_to_install: info.latest_known_version.clone(),
         current_file_path: Some(mod_dir.join(&info.file_name)),
      })
      .collect();

   if mods_needing_update.is_empty() {
      return Ok(());
   }

   install_manager(mod_dir, mods_needing_update, sync_data.lithic_sync)
      .await
      .map(|_| ())
      .map_err(err)
}

pub async fn delete_mod(mod_dir: PathBuf, file_name: String) -> Result<String, String> {
   let file_path = mod_dir.join(&file_name);
   if file_path.exists() {
      tokio::fs::remove_file(&file_path).await.map_err(err)?;
   }

   let sync_file = mod_dir.join(FILE_LITHIC_SYNC);
   if sync_file.exists() {
      if let Ok(mut data) = parse_json_file::<LithicSyncJson>(&sync_file).await {
         data.lithic_sync.retain(|_, info| info.file_name != file_name);
         let _ = data.save(&sync_file).await;
      }
   }

   Ok(file_name)
}

pub async fn refresh_browse() -> Result<Vec<ModApi>, String> {
   let path = Config::get_path().join(FILE_MOD_SEARCH_SYNC);
   fetch_and_cache_mods(&path).await
}

pub async fn load_browse() -> Result<Vec<ModApi>, String> {
   let search_file_path = Config::get_path().join(FILE_MOD_SEARCH_SYNC);

   if search_file_path.exists() {
      if let Ok(data) = parse_json_file::<ModsSearchFile>(&search_file_path).await {
         if !data.mods.is_empty() {
            return Ok(data.mods);
         }
      }
   }
   fetch_and_cache_mods(&search_file_path).await
}

async fn fetch_and_cache_mods(path: &Path) -> Result<Vec<ModApi>, String> {
   let client = ApiClient::new();
   let all = client.fetch_all_mods().await.map_err(err)?;
   let file_data = ModsSearchFile {
      mods: all.mods.clone(),
      last_sync: get_current_time(),
   };
   let json = prettify(&file_data, "Mods Search DB").map_err(err)?;
   let config_dir = Config::get_path();
   write_json_file(path, json, &config_dir).await.map_err(err)?;
   Ok(all.mods)
}

pub fn search_mods(all_mods: &[ModApi], query: &str) -> Vec<ModApi> {
   if query.trim().is_empty() {
      return all_mods.to_vec();
   }
   SearchQuery::new()
      .add_text_search(query.to_string())
      .execute(all_mods)
}

pub async fn install_mod(mod_dir: PathBuf, mod_id: String) -> Result<String, String> {
   if mod_dir.as_os_str().is_empty() || !mod_dir.exists() {
      return Err(loc_msg("ops-mods-dir-not-configured-settings"));
   }
   let sync_file = mod_dir.join(FILE_LITHIC_SYNC);
   let mut sync_map: HashMap<String, ModSyncInfo> = if sync_file.exists() {
      parse_json_file::<LithicSyncJson>(&sync_file)
         .await
         .map(|d| d.lithic_sync)
         .unwrap_or_default()
   } else {
      HashMap::new()
   };

   let client = ApiClient::new();
   let mod_info = client.fetch_mod(&mod_id).await.map_err(err)?;
   let mod_name = mod_info.mod_json.name.clone().unwrap_or_else(|| mod_id.clone());

   let (version, download_url, _, _) = parse_latest_version(&mod_info.mod_json.releases);

   let to_install = vec![Install {
      mod_id: mod_id.clone().into(),
      mod_name: mod_name.clone().into(),
      download_url: download_url.clone(),
      version_to_install: version.clone(),
      current_file_path: None,
   }];

   let installed_list = install_manager(mod_dir.clone(), to_install, sync_map.clone())
      .await
      .map_err(err)?;

   for inst in &installed_list {
      let Some(path) = &inst.installed_file_path else {
         continue;
      };
      let file_name = path
         .file_name()
         .and_then(|n| n.to_str())
         .unwrap_or_default()
         .to_string();
      if file_name.is_empty() {
         continue;
      }
      let is_primary = inst.mod_id.as_ref() == mod_id;
      sync_map.insert(
         inst.mod_id.to_string(),
         ModSyncInfo {
            file_name: file_name.into(),
            mod_name: inst.mod_name.to_string(),
            installed_version: if is_primary && inst.install_version.is_empty() {
               version.clone()
            } else {
               inst.install_version.clone()
            },
            latest_known_version: if is_primary {
               version.clone()
            } else {
               inst.install_version.clone()
            },
            latest_download_url: if is_primary {
               download_url.to_string()
            } else {
               String::new()
            },
            ..ModSyncInfo::default()
         },
      );
   }

   let sync = LithicSyncJson {
      lithic_sync: sync_map,
      last_sync: get_current_time(),
   };
   sync.save(&sync_file).await.map_err(err)?;

   Ok(mod_name)
}

pub async fn install_mod_to_active_instance(mod_id: String) -> Result<String, String> {
   let instance = lithic_core::instance::get_active_instance()
      .await?
      .ok_or_else(|| loc_msg("ops-no-active-instance"))?;
   if instance.mods_dir.trim().is_empty() {
      return Err(loc_msg("ops-active-instance-no-mods-dir"));
   }
   tokio::fs::create_dir_all(&instance.mods_dir).await.map_err(err)?;
   install_mod(PathBuf::from(instance.mods_dir), mod_id).await
}

pub async fn load_favorites() -> Result<HashSet<String>, String> {
   let path = Config::get_path().join(FAVORITES_FILE);
   if !path.exists() {
      return Ok(HashSet::new());
   }
   let data = tokio::fs::read_to_string(&path).await.map_err(err)?;
   serde_json::from_str(&data).map_err(err)
}

pub async fn save_favorites(favorites: HashSet<String>) -> Result<(), String> {
   let path = Config::get_path().join(FAVORITES_FILE);
   let data = serde_json::to_string(&favorites).map_err(err)?;
   tokio::fs::write(&path, data).await.map_err(err)
}

pub async fn export_favorites(favorites: HashSet<String>) -> Result<String, String> {
   let path = Config::get_path().join("lithic-favorites-export.txt");
   let mut ids: Vec<&String> = favorites.iter().collect();
   ids.sort();
   let content = ids.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("\n");
   tokio::fs::write(&path, content).await.map_err(err)?;
   Ok(path.display().to_string())
}

pub async fn load_packs() -> Result<(Vec<String>, Vec<String>), String> {
   let config = get_config().read().await;
   let disabled = config.modpacks.disabled.clone();
   let enabled = config.modpacks.enabled.clone();
   Ok((disabled, enabled))
}

pub async fn enable_pack(id: String) -> Result<String, String> {
   let mut config = get_config().write().await;
   if !config.modpacks.enabled.contains(&id) {
      config.modpacks.enabled.push(id.clone());
      config.modpacks.disabled.retain(|m| m != &id);
      config.save(None).map_err(err)?;
   }
   Ok(locale().get_with("ops-pack-enabled", "id", id).into_owned())
}

pub async fn disable_pack(id: String) -> Result<String, String> {
   let mut config = get_config().write().await;
   config.modpacks.enabled.retain(|m| m != &id);
   if !config.modpacks.disabled.contains(&id) {
      config.modpacks.disabled.push(id.clone());
   }
   config.save(None).map_err(err)?;
   Ok(locale().get_with("ops-pack-disabled", "id", id).into_owned())
}

pub async fn create_pack(
   mod_dir: PathBuf,
   name: String,
   pack_id: String,
   version: String,
) -> Result<String, String> {
   let modpack_dir = {
      let config = get_config().read().await;
      config.modpacks.modpack_dir.clone()
   };

   if modpack_dir.is_empty() {
      return Err(loc_msg("ops-modpack-dir-not-configured"));
   }

   let installed = load_installed_from(mod_dir).await?;
   let dependencies: HashMap<String, String> = installed
      .into_iter()
      .filter(|(id, _)| !id.is_empty())
      .map(|(id, info)| (id, info.installed_version.to_string()))
      .collect();

   let save_path = std::path::Path::new(&modpack_dir).join("mypacks");
   tokio::fs::create_dir_all(&save_path).await.map_err(err)?;

   let pack_info = ModInfo {
      name: name.clone(),
      mod_id: pack_id.clone().into(),
      version: if version.is_empty() {
         None
      } else {
         Some(version.into())
      },
      dependencies: dependencies
         .into_iter()
         .map(|(k, v)| (k.into(), v.into()))
         .collect(),
      ..ModInfo::default()
   };

   pack_info
      .build_modpack(save_path.clone(), pack_id.clone().into())
      .await
      .map_err(err)?;

   {
      let mut config = get_config().write().await;
      if !config.modpacks.disabled.contains(&pack_id) {
         config.modpacks.disabled.push(pack_id.clone());
      }
      config.save(None).map_err(err)?;
   }

   Ok(locale()
      .get_with2(
         "ops-pack-created",
         "name",
         name,
         "path",
         save_path.display().to_string(),
      )
      .into_owned())
}

pub async fn load_game_versions() -> Result<Vec<String>, String> {
   let path = Config::get_path().join(FILE_GAME_VERSION_SYNC);

   let raw: Vec<String> = if path.exists() {
      parse_json_file::<GameVersionSync>(&path)
         .await
         .map(|gvs| gvs.game_versions)
         .unwrap_or_default()
   } else {
      let client = ApiClient::new();
      let set = client.fetch_game_versions().await.map_err(err)?;
      let mut sorted: Vec<String> = set.into_iter().collect();
      sorted.sort();
      let gvs = GameVersionSync {
         game_versions: sorted.clone(),
         last_sync: get_current_time(),
      };
      let json = prettify(&gvs, "game versions").map_err(err)?;
      let _ = write_json_file(&path, json, &Config::get_path()).await;
      sorted
   };

   Ok(unique_minor_versions(&raw))
}

pub async fn fetch_versioned_browse(
   filter: VersionFilter,
   all_minor_versions: Vec<String>,
) -> Result<Vec<ModApi>, String> {
   let client = ApiClient::new();

   match &filter {
      VersionFilter::Any => {
         let all = client.fetch_all_mods().await.map_err(err)?;
         Ok(all.mods)
      }
      VersionFilter::Exact(v) => {
         let result = client.fetch_mods_with_gameversion(v).await.map_err(err)?;
         Ok(result.mods)
      }
      VersionFilter::AtLeast(min_v) => {
         let targets: Vec<String> = minor_versions_at_least(&all_minor_versions, min_v)
            .into_iter()
            .cloned()
            .collect();

         if targets.is_empty() {
            return Ok(Vec::new());
         }

         let fetches = targets.into_iter().map(|v| {
            let client = client.clone();
            async move { client.fetch_mods_with_gameversion(&v).await }
         });

         let results = join_all(fetches).await;

         let mut seen = HashSet::new();
         let mut combined = Vec::new();
         for result in results {
            if let Ok(mods_result) = result {
               for m in mods_result.mods {
                  if seen.insert(m.mod_id) {
                     combined.push(m);
                  }
               }
            }
         }
         Ok(combined)
      }
   }
}

pub async fn load_settings() -> Result<SettingsData, String> {
   let config = get_config().read().await;
   Ok(SettingsData {
      mod_dir: config.mod_dir.clone(),
      game_download_dir: config.game_download_dir.clone(),
      pinned_game_version: config.pinned_game_version.clone(),
      zip_mod_files: config.zip_mod_files,
      backup_mods: config.backup_mods,
      backup_mods_dir: config.backup_mods_dir.clone(),
      notify_of_unzipped_mods: config.notify_of_unzipped_mods,
      check_for_updates: config.check_for_updates,
      show_execution_time: config.show_execution_time,
      modpack_dir: config.modpacks.modpack_dir.clone(),
      theme_mode: config.theme_mode.clone(),
      theme_preset: config.theme_preset.clone(),
      initial_page: config.initial_page.clone(),
   })
}

pub async fn save_settings(s: SettingsData) -> Result<(), String> {
   let mut config = get_config().write().await;
   config.mod_dir = s.mod_dir;
   config.game_download_dir = s.game_download_dir;
   config.pinned_game_version = s.pinned_game_version;
   config.zip_mod_files = s.zip_mod_files;
   config.backup_mods = s.backup_mods;
   config.backup_mods_dir = s.backup_mods_dir;
   config.notify_of_unzipped_mods = s.notify_of_unzipped_mods;
   config.check_for_updates = s.check_for_updates;
   config.show_execution_time = s.show_execution_time;
   config.modpacks.modpack_dir = s.modpack_dir;
   config.theme_mode = s.theme_mode;
   config.theme_preset = s.theme_preset;
   config.initial_page = s.initial_page;
   config.save(None).map_err(err)
}

pub async fn load_theme_presets() -> Result<Vec<String>, String> {
   Ok(native_theme::theme::Theme::list_presets()
      .iter()
      .map(|preset| preset.key.to_string())
      .collect())
}

pub async fn load_instances() -> Result<Vec<InstanceConfig>, String> {
   lithic_core::instance::list_instances().await
}

pub async fn load_active_instance() -> Result<Option<InstanceConfig>, String> {
   lithic_core::instance::get_active_instance().await
}

pub async fn upsert_instance(form: InstanceFormData) -> Result<(), String> {
   let id = form.id.trim().to_string();
   if id.is_empty() {
      return Err(loc_msg("ops-instance-id-empty"));
   }
   let name = if form.name.trim().is_empty() {
      id.clone()
   } else {
      form.name.trim().to_string()
   };
   let (default_data_dir, default_mods_dir) = default_instance_paths(id.clone()).await;
   let data_dir = if form.data_dir.trim().is_empty() {
      default_data_dir
   } else {
      form.data_dir
   };
   let mods_dir = if form.mods_dir.trim().is_empty() {
      default_mods_dir
   } else {
      form.mods_dir
   };
   tokio::fs::create_dir_all(&data_dir).await.map_err(err)?;
   tokio::fs::create_dir_all(&mods_dir).await.map_err(err)?;

   for mod_id in &form.selected_mod_ids {
      install_mod(PathBuf::from(&mods_dir), mod_id.clone()).await?;
   }

   lithic_core::instance::add_or_update_instance(InstanceConfig {
      id,
      name,
      data_dir,
      mods_dir,
      game_version_id: form.game_version_id,
      enabled_modpacks: Vec::new(),
      start_params: form.start_params,
      env_vars: form.env_vars,
      last_played_at: 0,
      total_play_time_ms: 0,
   })
   .await
}

pub async fn default_instance_paths(id: String) -> (String, String) {
   let clean_id = id
      .chars()
      .map(|c| {
         if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
            c
         } else {
            '-'
         }
      })
      .collect::<String>();
   let id = if clean_id.trim().is_empty() {
      "new-instance".to_string()
   } else {
      clean_id
   };
   let data_dir = Config::data_path().join("instances").join(id).join("data");
   let mods_dir = data_dir.join("Mods");
   (
      data_dir.to_string_lossy().to_string(),
      mods_dir.to_string_lossy().to_string(),
   )
}

pub async fn delete_instance(id: String) -> Result<(), String> {
   lithic_core::instance::remove_instance(&id).await
}

pub async fn set_active_instance(id: String) -> Result<(), String> {
   lithic_core::instance::set_active_instance(&id).await
}

pub async fn launch_active_instance() -> Result<(), String> {
   lithic_core::instance::launch_instance(None).await
}

pub async fn load_game_version_installs() -> Result<Vec<GameVersionInstall>, String> {
   lithic_core::instance::list_game_versions().await
}

pub async fn upsert_game_version_install(id: String, version: String, path: String) -> Result<(), String> {
   lithic_core::instance::add_or_update_game_version(GameVersionInstall {
      id,
      version,
      path,
      source: lithic_core::instance::GameVersionSource::Manual,
      os: std::env::consts::OS.to_string(),
   })
   .await
}

pub async fn delete_game_version_install(id: String) -> Result<(), String> {
   lithic_core::instance::remove_game_version(&id).await
}

fn native_os_type() -> VSOSType {
   #[cfg(target_os = "macos")]
   return VSOSType::OSX;
   #[cfg(target_os = "windows")]
   return VSOSType::Windows;
   #[cfg(target_os = "linux")]
   VSOSType::Linux
}

pub async fn install_game_version(
   id: String,
   version: String,
   install_dir: String,
   progress: SharedGameInstallProgress,
) -> Result<String, String> {
   if let Ok(mut p) = progress.lock() {
      *p = GameInstallProgress {
         active: true,
         stage: loc_msg("ops-game-install-stage-starting"),
         percent: Some(0),
         logs: vec![
            locale()
               .get_with("ops-game-install-log-starting", "version", version.clone())
               .into_owned(),
         ],
         done: false,
         error: None,
      };
   }

   let progress_for_events = progress.clone();
   let installed = lithic_core::instance::install_game_version_with_progress(
      lithic_core::instance::GameVersionInstallOptions {
         id,
         version,
         install_dir: if install_dir.trim().is_empty() {
            None
         } else {
            Some(PathBuf::from(install_dir))
         },
         os_type: native_os_type(),
         exe_type: VSExecutabletype::Client,
         windows_installer_type: Some(VSWinInstallerType::Install),
      },
      move |event| {
         if let Ok(mut p) = progress_for_events.lock() {
            match event {
               GameVersionInstallEvent::Log(line) => {
                  p.logs.push(line);
               }
               GameVersionInstallEvent::Progress {
                  stage,
                  downloaded,
                  total,
               } => {
                  p.stage = stage;
                  p.percent = total.and_then(|t| {
                     if t == 0 {
                        None
                     } else {
                        Some(((downloaded.saturating_mul(100)) / t).min(100) as u8)
                     }
                  });
               }
            }
         }
      },
   )
   .await?;
   if let Ok(mut p) = progress.lock() {
      p.done = true;
      p.active = false;
      p.stage = loc_msg("ops-game-install-stage-complete");
      p.percent = Some(100);
      p.logs.push(
         locale()
            .get_with("ops-game-install-log-finished", "path", installed.path.clone())
            .into_owned(),
      );
   }
   Ok(locale()
      .get_with2(
         "ops-game-install-result",
         "version",
         installed.version,
         "path",
         installed.path,
      )
      .into_owned())
}

pub async fn pick_folder() -> Result<String, String> {
   tokio::task::spawn_blocking(|| {
        let candidates: Vec<(&str, Vec<&str>)> = if cfg!(target_os = "windows") {
            vec![(
                "powershell",
                vec![
                    "-NoProfile",
                    "-Command",
                    "Add-Type -AssemblyName System.Windows.Forms; $d = New-Object System.Windows.Forms.FolderBrowserDialog; if ($d.ShowDialog() -eq 'OK') { $d.SelectedPath }",
                ],
            )]
        } else if cfg!(target_os = "macos") {
            vec![(
                "osascript",
                vec![
                    "-e",
                    "POSIX path of (choose folder with prompt \"Choose a folder\")",
                ],
            )]
        } else {
            vec![
                ("zenity", vec!["--file-selection", "--directory"]),
                ("kdialog", vec!["--getexistingdirectory"]),
            ]
        };

        for (cmd, args) in candidates {
            let Ok(output) = Command::new(cmd).args(args).output() else {
                continue;
            };
            if output.status.success() {
                let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !path.is_empty() {
                    return Ok(path);
                }
            }
        }
        Err(loc_msg("ops-folder-picker-missing"))
    })
    .await
    .map_err(|e| e.to_string())?
}
