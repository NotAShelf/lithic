use crate::api::client::{ApiClient, VSExecutabletype, VSMirrorType, VSOSType, VSWinInstallerType};
use crate::config::manager::{Config, get_config};
use crate::errors::LithicError;
use crate::traits::string_ext::StrLowerExt;
use crate::utils::sorted_game_versions;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum GameVersionSource {
   #[default]
   Manual,
   LithicDownload,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GameVersionInstall {
   pub id: String,
   pub version: String,
   pub path: PathBuf,
   #[serde(default)]
   pub source: GameVersionSource,
   /// The OS this install targets. `None` keeps the field optional on disk so
   /// older configs (where `os` was missing) keep loading.
   #[serde(default)]
   pub os: Option<VSOSType>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InstanceConfig {
   pub id: String,
   pub name: String,
   pub data_dir: PathBuf,
   pub mods_dir: PathBuf,
   pub game_version_id: String,
   #[serde(default)]
   pub enabled_modpacks: Vec<String>,
   #[serde(default)]
   pub start_params: String,
   #[serde(default)]
   pub env_vars: String,
   #[serde(default)]
   pub last_played_at: i64,
   #[serde(default)]
   pub total_play_time_ms: i64,
}

#[derive(Debug, Clone)]
pub struct GameVersionInstallOptions {
   pub id: String,
   pub version: String,
   pub install_dir: Option<PathBuf>,
   pub os_type: VSOSType,
   pub exe_type: VSExecutabletype,
   pub windows_installer_type: Option<VSWinInstallerType>,
}

#[derive(Debug, Clone)]
pub enum GameVersionInstallEvent {
   Log(String),
   Progress {
      stage: String,
      downloaded: u64,
      total: Option<u64>,
   },
}

fn now_ms() -> i64 {
   // u128 -> i64: saturate on overflow instead of silently truncating. Current
   // timestamps comfortably fit; this is defensive against clock skew far in
   // the future and against systems whose clock is set before UNIX_EPOCH.
   SystemTime::now()
      .duration_since(UNIX_EPOCH)
      .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
      .unwrap_or(0)
}

pub fn find_executable(version_path: &Path) -> Option<(String, Vec<String>)> {
   #[cfg(target_os = "linux")]
   {
      let native = version_path.join("Vintagestory");
      if native.exists() {
         return Some((native.to_string_lossy().to_string(), Vec::new()));
      }
      let exe = version_path.join("Vintagestory.exe");
      if exe.exists() {
         return Some(("mono".to_string(), vec![exe.to_string_lossy().to_string()]));
      }
   }
   #[cfg(target_os = "windows")]
   {
      let exe = version_path.join("Vintagestory.exe");
      if exe.exists() {
         return Some((exe.to_string_lossy().to_string(), Vec::new()));
      }
   }
   #[cfg(target_os = "macos")]
   {
      let native = version_path.join("Vintagestory");
      if native.exists() {
         return Some((native.to_string_lossy().to_string(), Vec::new()));
      }
   }
   None
}

fn find_executable_dir(path: &Path) -> Option<PathBuf> {
   if find_executable(path).is_some() {
      return Some(path.to_path_buf());
   }
   let entries = std::fs::read_dir(path).ok()?;
   for entry in entries.flatten() {
      let child = entry.path();
      if child.is_dir() && find_executable(&child).is_some() {
         return Some(child);
      }
   }
   None
}

fn parse_env_vars(raw: &str) -> Vec<(String, String)> {
   raw.split(',')
      .filter_map(|entry| {
         let (k, v) = entry.split_once('=')?;
         let key = k.trim().to_string();
         let value = v.trim().to_string();
         if key.is_empty() || value.is_empty() {
            return None;
         }
         Some((key, value))
      })
      .collect()
}

pub async fn ensure_instances_migrated() -> Result<(), String> {
   let mut config = get_config().write().await;
   if !config.instances.is_empty() {
      return Ok(());
   }

   let default = InstanceConfig {
      id: "default".to_string(),
      name: "Default".to_string(),
      data_dir: PathBuf::new(),
      mods_dir: config.mod_dir.clone(),
      game_version_id: String::new(),
      enabled_modpacks: config.modpacks.enabled.clone(),
      start_params: String::new(),
      env_vars: String::new(),
      last_played_at: 0,
      total_play_time_ms: 0,
   };
   config.instances.push(default);
   config.active_instance_id = Some("default".to_string());
   config.save(None).map_err(|e| e.to_string())
}

pub async fn list_instances() -> Result<Vec<InstanceConfig>, String> {
   ensure_instances_migrated().await?;
   let config = get_config().read().await;
   Ok(config.instances.clone())
}

pub async fn add_or_update_instance(instance: InstanceConfig) -> Result<(), String> {
   ensure_instances_migrated().await?;
   let mut config = get_config().write().await;
   if instance.id.trim().is_empty() {
      return Err("Instance id cannot be empty".to_string());
   }
   if instance.mods_dir.as_os_str().is_empty() {
      return Err("Instance mods dir cannot be empty".to_string());
   }
   if instance.name.trim().is_empty() {
      return Err("Instance name cannot be empty".to_string());
   }
   if let Some(i) = config.instances.iter().position(|x| x.id == instance.id) {
      let existing = &config.instances[i];
      let mut updated = instance;
      // Preserve play history metadata across normal edits.
      updated.last_played_at = existing.last_played_at;
      updated.total_play_time_ms = existing.total_play_time_ms;
      config.instances[i] = updated;
   } else {
      config.instances.push(instance);
   }
   config.save(None).map_err(|e| e.to_string())
}

pub async fn remove_instance(id: &str) -> Result<(), String> {
   ensure_instances_migrated().await?;
   let mut config = get_config().write().await;
   config.instances.retain(|x| x.id != id);
   if config.active_instance_id.as_deref() == Some(id) {
      config.active_instance_id = config.instances.first().map(|x| x.id.clone());
   }
   config.save(None).map_err(|e| e.to_string())
}

pub async fn set_active_instance(id: &str) -> Result<(), String> {
   ensure_instances_migrated().await?;
   let mut config = get_config().write().await;
   if !config.instances.iter().any(|x| x.id == id) {
      return Err(format!("Instance not found: {id}"));
   }
   config.active_instance_id = Some(id.to_string());
   if let Some(active) = config.instances.iter().find(|x| x.id == id).cloned() {
      config.mod_dir = active.mods_dir.clone();
      config.modpacks.enabled = active.enabled_modpacks;
   }
   config.save(None).map_err(|e| e.to_string())
}

pub async fn get_active_instance() -> Result<Option<InstanceConfig>, String> {
   ensure_instances_migrated().await?;
   let config = get_config().read().await;
   let Some(active_id) = config.active_instance_id.as_ref() else {
      return Ok(None);
   };
   Ok(config.instances.iter().find(|x| &x.id == active_id).cloned())
}

pub async fn resolve_active_mod_dir() -> Result<PathBuf, String> {
   if let Some(instance) = get_active_instance().await? {
      return Ok(instance.mods_dir);
   }
   let config = get_config().read().await;
   Ok(config.mod_dir.clone())
}

pub async fn list_game_versions() -> Result<Vec<GameVersionInstall>, String> {
   let config = get_config().read().await;
   Ok(config.game_versions.clone())
}

pub async fn add_or_update_game_version(game_version: GameVersionInstall) -> Result<(), String> {
   let mut config = get_config().write().await;
   if game_version.id.trim().is_empty()
      || game_version.version.trim().is_empty()
      || game_version.path.as_os_str().is_empty()
   {
      return Err("Game version id, version, and path are required".to_string());
   }
   if let Some(i) = config.game_versions.iter().position(|x| x.id == game_version.id) {
      config.game_versions[i] = game_version;
   } else {
      config.game_versions.push(game_version);
   }
   config.save(None).map_err(|e| e.to_string())
}

pub async fn remove_game_version(id: &str) -> Result<(), String> {
   let mut config = get_config().write().await;
   if config.instances.iter().any(|x| x.game_version_id == id) {
      return Err("Game version is still referenced by an instance".to_string());
   }
   config.game_versions.retain(|x| x.id != id);
   config.save(None).map_err(|e| e.to_string())
}

async fn download_file<F>(
   client: &ApiClient,
   url: &str,
   save_loc: impl AsRef<Path>,
   progress: &mut F,
) -> Result<(), LithicError>
where
   F: FnMut(GameVersionInstallEvent),
{
   progress(GameVersionInstallEvent::Log(format!("GET {url}")));
   let response = client.head(url).await?;
   let total_size = response
      .headers()
      .get(reqwest::header::CONTENT_LENGTH)
      .and_then(|ct_len| ct_len.to_str().ok())
      .and_then(|ct_len| ct_len.parse::<u64>().ok());

   let mut res = client.get_download(url).await?;
   let mut file = File::create(save_loc).await?;
   let mut downloaded = 0;
   while let Some(chunk) = res
      .chunk()
      .await
      .map_err(|e| LithicError::SimpleError(e.to_string()))?
   {
      file.write_all(&chunk).await?;
      downloaded += chunk.len() as u64;
      progress(GameVersionInstallEvent::Progress {
         stage: "Downloading".to_string(),
         downloaded,
         total: total_size,
      });
   }
   Ok(())
}

fn unpack_tar_gz(archive: &Path, destination: &Path) -> Result<(), String> {
   std::fs::create_dir_all(destination).map_err(|e| e.to_string())?;
   let status = Command::new("tar")
      .arg("-xzf")
      .arg(archive)
      .arg("-C")
      .arg(destination)
      .status()
      .map_err(|e| format!("Failed to run tar: {e}"))?;
   if status.success() {
      Ok(())
   } else {
      Err(format!("tar exited with status: {status}"))
   }
}

pub async fn install_game_version_with_progress<F>(
   opts: GameVersionInstallOptions,
   mut progress: F,
) -> Result<GameVersionInstall, String>
where
   F: FnMut(GameVersionInstallEvent),
{
   let version = opts.version.trim_start_matches('v').to_string();
   progress(GameVersionInstallEvent::Log(format!(
      "Validating Vintage Story version {version}"
   )));
   let game_versions = sorted_game_versions().await.map_err(|e| e.to_string())?;
   let found = game_versions
      .iter()
      .any(|v| v.replace('v', "").eq_ignore_ascii_case(&version));
   if !found {
      return Err(format!("Invalid Vintage Story version: {version}"));
   }

   let mirror = if version.lower_contains("-rc") {
      VSMirrorType::Unstable
   } else {
      VSMirrorType::Stable
   };
   let client = ApiClient::new();
   let (url, filename) = client
      .download_uri(
         &opts.os_type,
         &opts.exe_type,
         &mirror,
         &version,
         opts.windows_installer_type.as_ref(),
      )
      .map_err(|e| e.to_string())?;
   progress(GameVersionInstallEvent::Log(format!(
      "Resolved download URL: {url}"
   )));

   let root = opts
      .install_dir
      .unwrap_or_else(|| Config::data_path().join("game-versions"));
   tokio::fs::create_dir_all(&root)
      .await
      .map_err(|e| e.to_string())?;

   let version_id = if opts.id.trim().is_empty() {
      version.clone()
   } else {
      opts.id
   };
   let archive_path = root.join(&filename);
   progress(GameVersionInstallEvent::Log(format!(
      "Saving artifact to {}",
      archive_path.display()
   )));
   download_file(&client, url.as_ref(), &archive_path, &mut progress)
      .await
      .map_err(|e| e.to_string())?;
   progress(GameVersionInstallEvent::Log("Download complete".to_string()));

   let registered_path = if archive_path
      .file_name()
      .and_then(|n| n.to_str())
      .is_some_and(|n| n.ends_with(".tar.gz"))
   {
      let destination = root.join(&version_id);
      progress(GameVersionInstallEvent::Log(format!(
         "Extracting archive to {}",
         destination.display()
      )));
      progress(GameVersionInstallEvent::Progress {
         stage: "Extracting".to_string(),
         downloaded: 0,
         total: None,
      });
      let archive = archive_path.clone();
      let dest = destination.clone();
      tokio::task::spawn_blocking(move || unpack_tar_gz(&archive, &dest))
         .await
         .map_err(|e| e.to_string())??;
      progress(GameVersionInstallEvent::Progress {
         stage: "Extracting".to_string(),
         downloaded: 1,
         total: Some(1),
      });
      find_executable_dir(&destination).unwrap_or(destination)
   } else {
      progress(GameVersionInstallEvent::Log(
         "Downloaded artifact is not a launchable unpacked game directory.".to_string(),
      ));
      return Err(
            "Downloaded artifact is an installer/archive and cannot be launched directly. Install it manually, then attach the extracted game directory as a version."
                .to_string(),
        );
   };

   let install = GameVersionInstall {
      id: version_id,
      version,
      path: registered_path,
      source: GameVersionSource::LithicDownload,
      os: Some(opts.os_type.clone()),
   };
   add_or_update_game_version(install.clone()).await?;
   progress(GameVersionInstallEvent::Log(format!(
      "Registered {} at {}",
      install.id,
      install.path.display()
   )));
   Ok(install)
}

pub async fn install_game_version(opts: GameVersionInstallOptions) -> Result<GameVersionInstall, String> {
   install_game_version_with_progress(opts, |_: GameVersionInstallEvent| {}).await
}

pub async fn launch_instance(instance_id: Option<String>) -> Result<(), String> {
   ensure_instances_migrated().await?;
   let (active_id, instance, version) = {
      let config = get_config().read().await;
      let active_id = instance_id
         .or_else(|| config.active_instance_id.clone())
         .ok_or_else(|| "No active instance selected".to_string())?;
      let Some(instance) = config.instances.iter().find(|x| x.id == active_id).cloned() else {
         return Err(format!("Instance not found: {active_id}"));
      };
      if instance.data_dir.as_os_str().is_empty() {
         return Err("Instance data_dir is empty".to_string());
      }
      let Some(version) = config
         .game_versions
         .iter()
         .find(|x| x.id == instance.game_version_id)
         .cloned()
      else {
         return Err("Instance has no valid game version reference".to_string());
      };
      (active_id, instance, version)
   };

   let Some((command, mut args)) = find_executable(&version.path) else {
      return Err("Unable to locate Vintage Story executable in selected version path".to_string());
   };

   args.push(format!("--dataPath={}", instance.data_dir.display()));
   args.extend(instance.start_params.split_whitespace().map(ToString::to_string));

   // Mirror the vendor's run.sh: launch from the game directory and use the
   // bundled fonts.conf when present.
   let fonts_conf = version.path.join("fonts.conf");

   // Redirect game output to a log file so startup failures (e.g. a missing
   // .NET runtime) are diagnosable instead of surfacing as a bare exit code.
   let log_path = instance.data_dir.join("lithic-launch.log");
   if let Some(parent) = log_path.parent() {
      tokio::fs::create_dir_all(parent)
         .await
         .map_err(|e| e.to_string())?;
   }
   let stdout_log = File::create(&log_path).await.map_err(|e| e.to_string())?;
   let stderr_log = stdout_log.try_clone().await.map_err(|e| e.to_string())?;

   let start = now_ms();
   let mut cmd = tokio::process::Command::new(command);
   cmd.args(args)
      .current_dir(&version.path)
      .stdout(stdout_log.into_std().await)
      .stderr(stderr_log.into_std().await);
   if fonts_conf.exists() {
      cmd.env("FONTCONFIG_FILE", &fonts_conf);
   }
   for (k, v) in parse_env_vars(&instance.env_vars) {
      cmd.env(k, v);
   }
   let status = cmd.status().await.map_err(|e| e.to_string())?;
   let end = now_ms();

   {
      let mut config = get_config().write().await;
      if let Some(instance) = config.instances.iter_mut().find(|x| x.id == active_id) {
         instance.last_played_at = end;
         instance.total_play_time_ms += (end - start).max(0);
         config.save(None).map_err(|e| e.to_string())?;
      }
   }

   if !status.success() {
      let tail = launch_log_tail(&log_path).await;
      return Err(if tail.is_empty() {
         format!("Game exited with status: {status} (log: {})", log_path.display())
      } else {
         format!(
            "Game exited with status: {status}. {tail} (log: {})",
            log_path.display()
         )
      });
   }
   Ok(())
}

/// Last few non-empty lines of the launch log, for surfacing startup failures.
async fn launch_log_tail(path: &Path) -> String {
   let mut file = match File::open(path).await {
      Ok(file) => file,
      Err(_) => return String::new(),
   };
   let len = file.metadata().await.map(|m| m.len()).unwrap_or(0);
   if file
      .seek(std::io::SeekFrom::Start(len.saturating_sub(4096)))
      .await
      .is_err()
   {
      return String::new();
   }
   let mut buf = Vec::new();
   if file.read_to_end(&mut buf).await.is_err() {
      return String::new();
   }
   let text = String::from_utf8_lossy(&buf);
   let tail: Vec<&str> = text
      .lines()
      .filter(|line| !line.trim().is_empty())
      .rev()
      .take(5)
      .collect();
   tail
      .iter()
      .rev()
      .copied()
      .collect::<Vec<_>>()
      .join(" | ")
      .chars()
      .take(600)
      .collect()
}
