use crate::aliases::{ModFileName, ModID, ModName, ModVersion};
use crate::consts::FILE_LITHIC_SYNC;
use crate::errors::LithicError;
use crate::utils::{get_current_time, prettify, write_atomic_async};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::default::Default;
use std::io::ErrorKind;
use std::path::Path;
use tracing::debug;

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct LithicSyncJson {
   #[serde(rename = "LithicSync")]
   pub lithic_sync: HashMap<String, ModSyncInfo>,
   pub last_sync: String,
}

impl Default for LithicSyncJson {
   fn default() -> Self {
      LithicSyncJson {
         lithic_sync: HashMap::default(),
         last_sync: get_current_time(),
      }
   }
}

impl LithicSyncJson {
   /// Atomically persist the sync index to `file_location`.
   pub async fn save(&self, file_location: impl AsRef<Path>) -> Result<(), LithicError> {
      debug!("Attempting to save {:?}", self);
      let json = prettify(self, "Sync")?;
      write_atomic_async(file_location.as_ref(), json.as_bytes()).await
   }
}

/// Remove a mod file and its sync entry with compensation if deletion fails.
///
/// The sync entry is saved first so a successful file deletion never leaves a
/// stale entry behind. If deletion fails, the original index is restored.
///
/// # Errors
///
/// Returns an error if the sync index cannot be read or saved, file deletion
/// fails, or restoring the index after a failed deletion fails.
pub async fn remove_mod_and_sync(mod_dir: impl AsRef<Path>, file_name: &str) -> Result<(), LithicError> {
   let mod_dir = mod_dir.as_ref();
   let sync_file = mod_dir.join(FILE_LITHIC_SYNC);
   let original = if sync_file.exists() {
      let mut sync = crate::utils::parse_json_file::<LithicSyncJson>(&sync_file).await?;
      let original = sync.clone();
      sync.lithic_sync.retain(|_, info| info.file_name != file_name);
      sync.save(&sync_file).await?;
      Some(original)
   } else {
      None
   };

   match tokio::fs::remove_file(mod_dir.join(file_name)).await {
      Ok(()) => Ok(()),
      Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
      Err(delete_error) => {
         if let Some(original) = original
            && let Err(restore_error) = original.save(&sync_file).await
         {
            return Err(LithicError::SimpleError(format!(
               "failed to delete {} ({delete_error}); restoring sync index also failed: {restore_error}",
               mod_dir.join(file_name).display()
            )));
         }
         Err(LithicError::IoError {
            context: format!("failed to delete {}", mod_dir.join(file_name).display()),
            source: delete_error,
         })
      }
   }
}

#[derive(Deserialize, Serialize, Debug)]
pub struct ModIDSync {
   pub all_mods: HashMap<ModName, ModIDSyncData>,
   pub last_sync: String,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct ModIDSyncData {
   pub mod_id: ModID,
   pub modid_strs: Vec<String>,
}

#[derive(Deserialize, Serialize, Debug, Clone, Default)]
pub struct ModSyncInfo {
   pub file_name: ModFileName,
   pub mod_name: String,
   pub asset_id: i64,
   pub installed_version: ModVersion,
   pub latest_known_version: ModVersion,
   pub latest_download_url: String,
   pub game_versions: Vec<String>,
   pub latest_changelog: String,

   #[serde(default)]
   pub is_symlink: bool,
}

#[derive(Deserialize, Serialize, Debug, Clone, Default)]
pub struct GameVersionSync {
   pub game_versions: Vec<String>,
   pub last_sync: String,
}

impl GameVersionSync {
   pub fn new() -> GameVersionSync {
      Self::default()
   }
}

#[cfg(test)]
mod tests {
   use super::*;

   #[tokio::test]
   async fn remove_mod_and_sync_removes_both_entries() {
      let dir = std::env::temp_dir().join(format!("lithic-sync-test-{}", std::process::id()));
      tokio::fs::create_dir_all(&dir).await.unwrap();
      let file_name = "example.zip";
      tokio::fs::write(dir.join(file_name), b"mod").await.unwrap();

      let mut sync = LithicSyncJson::default();
      sync.lithic_sync.insert(
         "example".to_string(),
         ModSyncInfo {
            file_name: file_name.into(),
            ..ModSyncInfo::default()
         },
      );
      sync.save(dir.join(FILE_LITHIC_SYNC)).await.unwrap();

      remove_mod_and_sync(&dir, file_name).await.unwrap();

      assert!(!dir.join(file_name).exists());
      let saved = crate::utils::parse_json_file::<LithicSyncJson>(dir.join(FILE_LITHIC_SYNC))
         .await
         .unwrap();
      assert!(saved.lithic_sync.is_empty());
      tokio::fs::remove_dir_all(dir).await.unwrap();
   }
}
