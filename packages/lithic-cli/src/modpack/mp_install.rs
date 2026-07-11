use std::path::Path;
// mp_install calls normal install, except it gathers a list of all the required mods before hand
// and sets the install location to be the modpack specific folder

use crate::commands::install::install_missing_deps;
use crate::commands::sync::sync;
use comfy_table::{Attribute, Color};
use lithic_core::aliases::{ModID, ModVersion};
use lithic_core::api::client::ApiClient;
use lithic_core::api::download::download_requested_mods;
use lithic_core::api::structs::ModInfo;
use lithic_core::config::manager::{Package, get_config};
use lithic_core::consts::FILE_MODINFO_JSON;
use lithic_core::errors::LithicError;
use lithic_core::installer::manager::{Install, Installed, install_manager};
use lithic_core::utils::{command_output, display_table, elapsed_footer, notice};
use lithic_core::utils::{extract_all_mods_metadata, extract_zip_metadata};
use lithic_core::version::manager::{
   parse_download_url_from_version, parse_latest_version, parse_pinned_version,
};
use std::collections::HashMap;
use std::fs;
use std::process::exit;
use yansi::Paint;

use std::time::Instant;
use tracing::{debug, error, info, warn};

pub fn check_if_mp_enabled(mp_id: &str, array: &[String]) {
   if array.iter().any(|m| m.eq_ignore_ascii_case(mp_id)) {
      notice(
         format!(
            "{} {}",
            mp_id, "is currently enabled!. Disable it first then try again. "
         ),
         Some(Color::Yellow),
         vec![],
      );
      exit(1);
   }
}

/// Install a modpack and its dependencies.
///
/// # Errors
///
/// Returns an error if metadata cannot be loaded, dependencies cannot be
/// installed, or config cannot be saved.
pub async fn mp_install(mp_id: ModID, mp_version: Option<ModVersion>) -> Result<String, LithicError> {
   let start_time = Instant::now();
   // installing the modpack with this function will do the following:
   // Save the modpack.zip (the modpack from the mods website) to modpacks/packs
   // Once the modpack is installed, it will download all the mods associated with the modpack
   // to the location [modpacks/installed/modpack_id/*]
   let config = get_config().read().await;

   check_if_mp_enabled(mp_id.as_ref(), &config.modpacks.enabled);

   let packs_path = Path::new(&config.modpacks.modpack_dir).join("mypacks");
   let local_packs = extract_all_mods_metadata(&packs_path, false)
      .await
      .unwrap_or_default();
   let found_local_packs = local_packs
      .iter()
      .find(|(_, info)| info.mod_id.eq_ignore_ascii_case(&mp_id));

   let client = ApiClient::new();
   let installed_dir = Path::new(&config.modpacks.modpack_dir).join("installed");
   let packs_dir = Path::new(&config.modpacks.modpack_dir).join("packs");

   let modpack = if let Some((modpack_filename, local_info)) = found_local_packs {
      info!("local_packs: {:#?}", local_packs);
      Installed {
         mod_id: mp_id.clone(),
         mod_name: local_info.name.clone().into(),
         installed_file_path: Some(
            Path::new(&config.modpacks.modpack_dir)
               .join("mypacks")
               .join(modpack_filename),
         ),
         old_file_path: None,
         install_version: local_info.version.clone().unwrap_or("0.1.0".into()),
         success: true, // it's always true since its local
      }
   } else {
      let mod_info = client.fetch_mod(&mp_id).await?;
      info!("mod_info fetched.. Doing modpack installation..");

      let (version, download_url, _, _) = if let Some(pin_version) = mp_version {
         let pkg = Package {
            mod_id: mp_id.to_string(),
            pinned_version: Some(pin_version),
         };
         parse_pinned_version(&mod_info.mod_json.releases, &pkg, String::new())
      } else {
         debug!("Parsing latest version..");
         parse_latest_version(&mod_info.mod_json.releases)
      };

      info!("version: {}, download_url {}", version, download_url);

      let install_modpack = Install {
         mod_id: mp_id.clone(),
         mod_name: mod_info.mod_json.name.clone().unwrap_or_default().into(),
         version_to_install: version,
         download_url,
         current_file_path: None,
      };

      notice(
         format!("Downloading Modpack {mp_id}..."),
         Some(Color::Green),
         vec![],
      );
      let Some(modpack) = download_requested_mods(&packs_dir, &mut vec![install_modpack], &client, None)
         .await?
         .into_iter()
         .next()
      else {
         return Err(LithicError::SimpleError("Modpack download failure..".into()));
      };
      modpack
   };

   if let Some(modpack_packs_path) = modpack.installed_file_path {
      // Modpack is just a normal mod but we treat it differently when used with modpack
      let modpack_info = extract_zip_metadata::<ModInfo>(&modpack_packs_path, FILE_MODINFO_JSON).await.inspect_err(|_| {
            notice(format!("The requested modpack has a malformed {FILE_MODINFO_JSON} file and Lithic is unable to parse it."), Some(Color::Red), vec![Attribute::Bold]);
        })?;

      // do another check if the IDs are different, user might have installed using the numerical ID
      if !modpack_info.mod_id.eq_ignore_ascii_case(&mp_id) {
         check_if_mp_enabled(modpack_info.mod_id.as_ref(), &config.modpacks.enabled);
      }

      // The modpack is installed to the correct place, install all dependencies
      let modpack_mod_path = installed_dir.join(&modpack_info.mod_id);

      if !modpack_mod_path.exists() {
         info!("Created {modpack_mod_path:?}");
         fs::create_dir_all(&modpack_mod_path)?;
      }

      // grab the mod ids from the modpack
      let mods = modpack_info.dependencies.keys().cloned().collect();
      let mod_pkgs: Vec<Package> = modpack_info
         .dependencies
         .iter()
         .map(|(id, version)| Package {
            mod_id: id.to_string(),
            pinned_version: Some(version.clone()),
         })
         .collect();

      info!("MODS: {mods:?}");
      let deps = client.fetch_mods_parallel(mods).await?;

      let install_mp_mods: Vec<Install> = deps
         .iter()
         .filter_map(|(mod_id, mod_api)| {
            // grab the mod from the modpack so we can actually download the correct version
            if let Some((mp_mod_id, mp_mod_version)) = modpack_info
               .dependencies
               .iter()
               .find(|(dep_mod_id, _)| dep_mod_id.eq(&mod_id))
            {
               let download_url =
                  match parse_download_url_from_version(&mod_api.mod_json.releases, mp_mod_version) {
                     Ok(download_url) => download_url,
                     Err(e) => {
                        warn!("Lithic can't download {}: {}", mp_mod_id.red(), e.red());
                        return None;
                     }
                  };

               Some(Install {
                  mod_id: mod_id.clone(),
                  mod_name: mod_api.mod_json.name.clone().unwrap_or_default().into(),
                  version_to_install: mp_mod_version.clone(),
                  download_url,
                  current_file_path: None,
               })
            } else {
               None
            }
         })
         .collect();

      debug!("Need to download {install_mp_mods:#?}");

      let installed = install_manager(&modpack_mod_path, install_mp_mods, HashMap::new()).await?;

      // Mod saved successfully, add it to the disabled mods so we know its installed

      debug!("Successfully installed {installed:#?}");

      sync(packs_dir, false, vec![]).await?;
      sync(modpack_mod_path, false, mod_pkgs).await?;

      display_table(
         vec![command_output(
            "Successfully installed Modpack:",
            modpack.mod_name,
         )],
         None,
      );
      elapsed_footer(start_time, "Modpack Install");

      return Ok(modpack_info.mod_id.to_string());
   }

   Err(LithicError::SimpleError(
      "Unable to find installed modpack".into(),
   ))
}

/// Install missing dependencies for an already-installed modpack.
///
/// # Errors
///
/// Returns an error if the modpack cannot be found or dependency installation
/// fails.
pub async fn mp_install_missing_deps(mpk_id: ModID) -> Result<(), LithicError> {
   // iterate through all the modpacks and for each one check the dependencies.
   // if the installed/modpack folder is missing, create it and download all deps without checking

   // use a write here as we need to update the config IF the modpack wasn't installed via the modpack install command
   let config = get_config().read().await;

   let modpack_dir = config.modpacks.modpack_dir.clone();
   drop(config);
   let modpack_dir = Path::new(&modpack_dir);
   let packs_dir = modpack_dir.join("packs");

   // check if the modpack is in the packs dir

   let packs_data = extract_all_mods_metadata(&packs_dir, false).await?;

   for (_, pack_info) in packs_data {
      if pack_info.mod_id.eq_ignore_ascii_case(&mpk_id) {
         // check if mpk has a installed/mpk-id folder
         let mpk_mods_dir = modpack_dir.join("installed").join(&mpk_id);
         if !mpk_mods_dir.exists() {
            tokio::fs::create_dir_all(&mpk_mods_dir).await?;
         }

         match install_missing_deps(&packs_dir, vec![mpk_id.clone()], &mpk_mods_dir).await {
            Ok(()) => {
               let mut config = get_config().write().await;
               // update the config file now
               if !config
                  .modpacks
                  .disabled
                  .iter()
                  .any(|m| m.eq_ignore_ascii_case(mpk_id.as_ref()))
               {
                  config.modpacks.disabled.push(mpk_id.to_string());
               }

               config.save(None)?;
               drop(config);

               notice(
                  format!(
                     "The dependencies for {mpk_id} have been installed! You can now enable this modpack."
                  ),
                  Some(Color::Green),
                  vec![Attribute::Bold],
               );
            }
            Err(e) => {
               error!("Failed to install dependencies for {mpk_id}: {e}");
            }
         }
      }
   }

   Ok(())
}
