use crate::commands::sync::get_sync_data;
use comfy_table::{Attribute, Color};
use lithic_core::aliases::ModID;
use lithic_core::config::manager::get_config;
use lithic_core::errors::LithicError;
use lithic_core::installer::manager::{Install, Installed, install_manager};
use lithic_core::sync::structs::ModSyncInfo;
use lithic_core::utils::{backup_older_files, remove_older_files, split_modid_version};
use lithic_core::utils::{display_installation_results, elapsed_footer, notice};
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::time::Instant;
use tracing::debug;
use yansi::Paint;

#[allow(clippy::map_entry)]
/// Update installed mods in `mod_dir`.
///
/// # Errors
///
/// Returns an error if sync data cannot be loaded, update metadata cannot be
/// resolved, files cannot be backed up or removed, or installation fails.
pub async fn update_mods<V: AsRef<[ModID]>>(
   mod_dir: impl AsRef<Path>,
   update_mod_ids: V,
   keep_old_files: bool,
) -> Result<(), LithicError> {
   let (mod_dir, update_mod_ids) = (mod_dir.as_ref(), update_mod_ids.as_ref());
   let start_time = Instant::now();
   let config = get_config().read().await;
   let sync_data = get_sync_data(&PathBuf::from(mod_dir), false).await?;

   notice(
      "Updating mods...",
      Option::from(Color::Yellow),
      vec![Attribute::Bold],
   );
   // filter out anything that is a symlink. This means it's a modpack file we don't want to update.
   let sync_data = sync_data
      .lithic_sync
      .into_iter() // Consume and transform
      .filter_map(|(mod_id, sync_info)| {
         // filter out any symlinks and return a modid that has the version stripped from it
         if sync_info.is_symlink {
            None
         } else {
            Some((split_modid_version(mod_id).0, sync_info))
         }
      })
      .collect();

   let mut mods_to_check_update: HashMap<ModID, ModSyncInfo> = HashMap::new();
   let mut updates_exist = false;

   if update_mod_ids.is_empty() {
      mods_to_check_update.clone_from(&sync_data);
      updates_exist = true;
   } else {
      for typed_mod_id in update_mod_ids {
         let mod_sync_data = &sync_data;
         // user typed in a valid typed_mod_id so violet is happy now
         let typed_mod_id: ModID = typed_mod_id.to_lowercase().into();
         if mod_sync_data.contains_key(&typed_mod_id) {
            mods_to_check_update
               .entry(typed_mod_id.clone())
               .or_insert(mod_sync_data[&typed_mod_id].clone());
            updates_exist = true;
         } else {
            println!("{} is not a valid mod_id!", &typed_mod_id.red());
         }
      }
   }

   if !updates_exist {
      return Err(LithicError::SimpleError(String::from(
         "No valid update ids or the mod dir is empty..\n\r",
      )));
   }

   let all_installed_mods: HashMap<String, ModSyncInfo> = mods_to_check_update
      .iter()
      .map(|(k, v)| (k.to_string(), v.clone()))
      .collect();
   debug!("all_installed_mods: {:#?}", all_installed_mods);

   let final_mod_update_list: Vec<Install> = mods_to_check_update
      .into_iter()
      .filter_map(|(mod_id, mod_sync_info)| {
         // if mod_id is present in the [[pkg]] section of the config, check if we are allowed to update the mod
         if mod_sync_info.latest_known_version != mod_sync_info.installed_version && !mod_id.is_empty() {
            Some(Install {
               mod_id: mod_id.to_lowercase().into(),
               mod_name: mod_sync_info.mod_name.clone().into(),
               version_to_install: mod_sync_info.latest_known_version.clone(),
               download_url: mod_sync_info.latest_download_url.clone().into(),
               current_file_path: Some(mod_dir.join(mod_sync_info.file_name)),
            })
         } else {
            None
         }
      })
      .collect();

   debug!("final_mod_update_list: {:#?}", final_mod_update_list);

   let mods_processed: Vec<Installed> =
      install_manager(mod_dir, final_mod_update_list.clone(), all_installed_mods).await?;

   if config.backup_mods {
      backup_older_files(&mods_processed).await?;
   }

   if !keep_old_files {
      remove_older_files(&mods_processed).await?;
   }

   display_installation_results(mods_processed);

   if config.show_execution_time {
      elapsed_footer(start_time, "Update");
   }

   Ok(())
}
