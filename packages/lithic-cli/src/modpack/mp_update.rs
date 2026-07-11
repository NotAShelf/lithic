use crate::commands::arg_structs::modpack_args::MPUpdateArgs;
use crate::commands::sync::{get_sync_data, sync};
use crate::commands::update::update_mods;
use crate::modpack::mp_install::check_if_mp_enabled;
use comfy_table::Color;
use lithic_core::api::client::ApiClient;
use lithic_core::api::download::download_requested_mods;
use lithic_core::api::structs::ModInfo;
use lithic_core::config::manager::{Package, get_config};
use lithic_core::consts::FILE_MODINFO_JSON;
use lithic_core::errors::LithicError;
use lithic_core::installer::manager::Install;
use lithic_core::sync::structs::ModSyncInfo;
use lithic_core::utils::notice;
use lithic_core::utils::{delete_file, extract_zip_metadata};
use std::collections::HashMap;
use std::path::Path;
use tracing::{debug, error, info};
use yansi::Paint;

/// Update an installed modpack archive and dependency set.
///
/// # Errors
///
/// Returns an error if sync data cannot be loaded, the modpack cannot be
/// fetched, or files cannot be replaced.
pub async fn mp_update(args: MPUpdateArgs) -> Result<(), LithicError> {
   let config = get_config().read().await;

   // Make sure the modpack isn't enabled or we'll have orphaned symlinks
   check_if_mp_enabled(&args.mpk_id, &config.modpacks.enabled);

   let modpack_base_dir = Path::new(&config.modpacks.modpack_dir);
   let pack_dir = modpack_base_dir.join("packs");

   let modpack_sync_file = get_sync_data(&pack_dir, false).await?;

   let modpack_sync_file: HashMap<String, ModSyncInfo> = modpack_sync_file.lithic_sync;

   // check if the requested modpack is in the sync file
   if !modpack_sync_file.contains_key(&args.mpk_id) {
      notice(
         format!(
            "{} doesn't appear to be installed. Use Lithic modpack install {} to download the modpack",
            &args.mpk_id, &args.mpk_id
         ),
         Some(Color::Yellow),
         vec![],
      );
      return Err(LithicError::SimpleError(
         "Modpack not installed, nothing to update".into(),
      ));
   }
   let Some(modpack_info) = modpack_sync_file.get(&args.mpk_id) else {
      return Err(LithicError::SimpleError(
         "Unable to retrieve modpack info from sync file".into(),
      ));
   };

   // get the modinfo.json file for the modpack and check it against the sync file
   if modpack_info
      .installed_version
      .eq_ignore_ascii_case(&modpack_info.latest_known_version)
   {
      notice(
         format!("Modpack {} is already up to date!", &args.mpk_id),
         Some(Color::Green),
         vec![],
      );
      return Ok(());
   }

   // we know its not up-to-date, download the latest version and save it to the packs folder, deleting the old version.. unless the are named the same
   let mp_file_path = pack_dir.join(&modpack_info.file_name);
   let client = ApiClient::new();
   // we already have the latest download URL, use that
   let m_install = Install {
      mod_id: args.mpk_id.clone().into(),
      mod_name: modpack_info.mod_name.clone().into(),
      version_to_install: modpack_info.latest_known_version.clone(),
      download_url: modpack_info.latest_download_url.clone().into(),
      current_file_path: Some(mp_file_path.clone()),
   };

   debug!("{} {:#?}", "m_install".green(), m_install.blue());

   let installed = match download_requested_mods(&pack_dir, &mut vec![m_install], &client, None).await {
      Ok(i) => {
         // Exactly one modpack is processed per call. If the downloader
         // returned anything else, refuse rather than panic.
         let Some(first) = i.first().cloned() else {
            return Err(LithicError::SimpleError(format!(
               "modpack update returned no installed file for {}",
               &args.mpk_id
            )));
         };
         if !first.installed_file_path.eq(&Some(mp_file_path.clone())) {
            info!("Deleting old modpack file {}", mp_file_path.display());
            delete_file(&mp_file_path).await?;
         }
         first
      }
      Err(e) => return Err(e),
   };

   let Some(updated_mp_filepath) = &installed.installed_file_path else {
      return Err(LithicError::SimpleError(format!(
         "Unable to get updated file path for {}",
         &args.mpk_id
      )));
   };

   let mp_mod_pkgs: Vec<Package> = extract_zip_metadata::<ModInfo>(&updated_mp_filepath, FILE_MODINFO_JSON)
      .await?
      .dependencies
      .iter()
      .map(|(mod_id, mod_version)| Package {
         mod_id: mod_id.to_string(),
         pinned_version: Some(mod_version.clone()),
      })
      .collect();

   let mp_install_dir = modpack_base_dir.join("installed").join(&args.mpk_id);
   sync(&mp_install_dir, true, &mp_mod_pkgs).await?;

   match update_mods(&mp_install_dir, &[], false).await {
      Ok(()) => {
         sync(&mp_install_dir, false, &mp_mod_pkgs).await?;
      }
      Err(e) => {
         error!("{}", e.to_string());
      }
   }

   Ok(())
}
