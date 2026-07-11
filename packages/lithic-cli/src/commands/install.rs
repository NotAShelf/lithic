use crate::commands::sync::get_sync_data;
use comfy_table::presets::UTF8_HORIZONTAL_ONLY;
use lithic_core::aliases::{ModID, ModVersion};
use lithic_core::api::client::ApiClient;
use lithic_core::config::manager::{Package, get_config};
use lithic_core::errors::LithicError;
use lithic_core::errors::LithicError::SimpleError;
use lithic_core::installer::manager::{Install, install_manager};
use lithic_core::utils::{command_output, display_installation_results, display_table};
use lithic_core::utils::{extract_all_mods_metadata, gather_missing_dependencies, split_modid_version};
use lithic_core::version::manager::{parse_latest_version, parse_pinned_version};
use std::collections::HashMap;
use std::path::Path;
use tracing::{debug, info};

// Report if trying install a mod that already exists
// Use -f to force an installation
// add way to set the version you want to download
/// Install the requested mods into `mod_dir`.
///
/// # Errors
///
/// Returns an error if sync data cannot be loaded, mod metadata cannot be
/// fetched, or installation fails.
pub async fn install_cmd(
   mod_dir: impl AsRef<Path>,
   mods_requested: Vec<ModID>,
   _force: bool,
) -> Result<(), LithicError> {
   let mod_dir = mod_dir.as_ref();
   info!("install_cmd: {mods_requested:?}");

   display_table(
      vec![command_output(
         "Installing..",
         mods_requested
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", "),
      )],
      Some(UTF8_HORIZONTAL_ONLY),
   );

   // do this first as we need to strip the @ if it exists
   let mod_map: HashMap<ModID, Option<ModVersion>> = mods_requested.iter().map(split_modid_version).collect();

   // get sync data
   let sync_data = get_sync_data(mod_dir, true).await?;

   let config = get_config().read().await;

   let installed_mods = sync_data.lithic_sync.clone();

   let client = ApiClient::new();

   // get the download urls for all requested mods
   let result = client
      .fetch_mods_parallel(mod_map.keys().cloned().collect())
      .await?;

   if result.is_empty() {
      return Err(SimpleError(format!("Invalid modid {mods_requested:?}")));
   }

   let mods_requested: Vec<Install> = result
      .into_iter()
      .map(|(mod_id, mod_info)| {
         let pin_ver = if let Some(mod_version) = mod_map.get(&mod_id) {
            if mod_version.is_some() {
               mod_version.clone()
            } else if let Some(package) = config
               .pkg
               .iter()
               .find(|package| package.mod_id.eq_ignore_ascii_case(mod_id.as_ref()))
            {
               package.pinned_version.clone()
            } else {
               None
            }
         } else {
            None
         };

         let pkg = Package {
            mod_id: mod_id.to_string(),
            pinned_version: pin_ver.clone(),
         };

         info!("pkg: {:?}", pkg);

         let pinned_game_ver = &config.pinned_game_version;

         let (version, download_url, _, _) =
            parse_pinned_version(&mod_info.mod_json.releases, &pkg, pinned_game_ver);

         Install {
            mod_id: mod_id.clone(),
            mod_name: mod_info.mod_json.name.unwrap_or_default().into(),
            version_to_install: version,
            download_url,
            current_file_path: None,
         }
      })
      .collect();

   info!("Mods requested {:?}", mods_requested);

   let mods_processed = install_manager(mod_dir, mods_requested.clone(), installed_mods).await?;

   display_installation_results(mods_processed);

   Ok(())
}

/// mod_dir_for_req is where the mods_requested will be searched for
/// all dependencies will be installed to dep_install_path
///
/// # Errors
///
/// Returns an error if installed metadata or sync data cannot be loaded, or if
/// dependency installation fails.
pub async fn install_missing_deps<V: AsRef<[ModID]>>(
   mod_dir_for_req: impl AsRef<Path>,
   mods_requested: V,
   dep_install_path: impl AsRef<Path>,
) -> Result<(), LithicError> {
   let (mod_dir, mods_requested) = (mod_dir_for_req.as_ref(), mods_requested.as_ref());
   // get all installed mod info
   // retrieve all dependencies
   // send missing ones to install_manager()

   let installed_mods = extract_all_mods_metadata(mod_dir, true).await?;
   // silence the sync message because it happens too much during installation.
   let sync_data = get_sync_data(mod_dir, true).await?.lithic_sync.clone();

   let mods_map: HashMap<ModID, Option<ModVersion>> =
      mods_requested.iter().map(split_modid_version).collect();
   let mods_id_vec: Vec<ModID> = mods_map.keys().cloned().collect();

   info!("install_missing_deps: mods_id_vec: {:?}", mods_id_vec);

   // if there are reports of slowness is this section .values().par_bridge()...flat_map_iter() could be used to speed it up
   // this is prob not an issue even with a lot of mods as the data is all in memory at this point
   let mut missing_deps: Vec<Install> =
      gather_missing_dependencies(&installed_mods, &mods_id_vec, &sync_data);

   let client = ApiClient::new();

   // get the final list of mods we know need to be installed
   let md_ids: Vec<ModID> = missing_deps.iter().map(|i| i.mod_id.clone()).collect();
   info!("md_ids: {:?}", md_ids);
   // get download_urls
   let result = client.fetch_mods_parallel(md_ids.clone()).await?;
   info!("result: {:?}", result);

   if result.is_empty() {
      info!("No missing deps to download..");
      return Ok(());
   }

   for mod_info in &mut missing_deps {
      if let Some(data) = result.get(&mod_info.mod_id) {
         mod_info.mod_name = data.mod_json.name.clone().unwrap_or_default().into();
         let (version, download_url, _, _) = parse_latest_version(&data.mod_json.releases);
         mod_info.download_url = download_url;
         mod_info.version_to_install = version;
      }
   }

   debug!("deps: {:?}", missing_deps);

   let mods_processed = install_manager(dep_install_path, missing_deps, sync_data).await?;

   info!("mods_processed {:#?}", mods_processed);

   display_installation_results(mods_processed);

   Ok(())
}
