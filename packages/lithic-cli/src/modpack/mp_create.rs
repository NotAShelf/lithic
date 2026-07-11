// Modpack creation is a bit involved with all the steps required.
// using --interactive is the best way to do it so lithic can ask you questions as there are alot of flags by default

// only a few are required, so a minimal modpack can be created pretty easily

use crate::commands::arg_structs::modpack_args::MPCreateArgs;
use crate::commands::search::parse_search_file;
use lithic_core::aliases::{ModID, ModVersion};
use lithic_core::api::structs::{ModInfo, StringOrInt};
use lithic_core::config::manager::get_config;
use lithic_core::consts::FILE_MODINFO_JSON;
use lithic_core::errors::LithicError;
use lithic_core::symlink_manager::SymlinkManager;
use lithic_core::utils::{extract_all_mods_metadata, find_mod_id};
use lithic_core::version::manager::parse_version;
use semver::Version;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::fs;
use tracing::{info, warn};
use yansi::Paint;

/// Convert CLI modpack creation arguments into mod metadata.
///
/// # Errors
///
/// Currently does not fail, but returns `Result` to match callers that compose
/// argument collection with fallible modpack preparation.
pub fn collect_mp_create_args(args: &MPCreateArgs) -> Result<ModInfo, LithicError> {
   Ok(ModInfo {
      name: args.name.clone(),
      mod_type: StringOrInt::default(),
      mod_id: args.mpk_id.clone().into(),
      version: Some(args.mpk_version.clone().into()),
      network_version: None,
      texture_size: None,
      description: args.description.clone(),
      website: args.website.clone(),
      authors: vec![args.author.clone().unwrap_or_default()],
      contributors: vec![],
      side: None,
      required_on_client: None,
      required_on_server: None,
      dependencies: HashMap::default(),
   })
}

// When a modpack is created, the mods by default, will be moved to their own folder in modpacks/installed/mynewpack
// If those mods were created from a symlink,

#[allow(clippy::fn_params_excessive_bools)]
/// Create a local modpack archive and installation directory.
///
/// # Errors
///
/// Returns an error if metadata cannot be loaded, files cannot be copied or
/// moved, or the modpack archive cannot be built.
pub async fn mp_create(
   mod_dir: impl AsRef<Path> + Copy,
   mod_pack: &mut ModInfo,
   save_location: Option<impl AsRef<Path>>,
   copy_mods: bool,
   ignore_modpacks: bool,
) -> Result<(PathBuf, PathBuf), LithicError> {
   let config = get_config().write().await;
   let modpack_dir = config.modpacks.modpack_dir.clone();
   drop(config);

   let mods_search_data = parse_search_file().await?.mods;

   let base_modpack_dir = Path::new(&modpack_dir);

   // We DO want to ignore all the symlinks when creating a new modpack
   let all_mods = extract_all_mods_metadata(mod_dir, ignore_modpacks).await?;
   let mp_mods: HashMap<ModID, ModVersion> = all_mods.iter().filter_map(|(mod_filename, mod_info)| {
        let mod_id: ModID = if mod_info.mod_id.is_empty() {
            find_mod_id(&mod_info.name, mod_filename, &mods_search_data).unwrap_or_default().into()
        } else {
            mod_info.mod_id.clone()
        };

        if mod_id.is_empty() {
            warn!("{} {} {} {} {}","Mod".yellow(), mod_filename.magenta(),
                "was not included in this modpack because Lithic was unable to locate a valid modid. It was either omitted or the mod has a malformed".yellow(), FILE_MODINFO_JSON.magenta(), "file".yellow());
            return None;
        }

        let version = parse_version(&mod_info.version.clone().unwrap_or("0.0.0".into()))
            .unwrap_or(Version::new(0,0,0));

        Some((mod_id, version.to_string().into()))
    }).collect();

   mod_pack.dependencies = mp_mods;

   let save_location = if let Some(save_path) = save_location {
      Path::new(save_path.as_ref()).to_path_buf()
   } else {
      base_modpack_dir.join("mypacks")
   };

   let mod_zip_save_path = mod_pack
      .build_modpack(save_location, mod_pack.mod_id.to_string().into())
      .await?;

   // copy or move the mods into a new location
   // first create the new directory

   let mod_install_dir = base_modpack_dir.join("installed").join(&mod_pack.mod_id);

   if !mod_install_dir.exists() {
      fs::create_dir_all(&mod_install_dir)
         .await
         .map_err(|e| LithicError::SimpleError(e.to_string()))?;
   }

   let mut mod_dir_entry = tokio::fs::read_dir(&mod_dir)
      .await
      .map_err(|e| LithicError::SimpleError(e.to_string()))?;

   // copy or move all the mods into the new directory

   while let Ok(Some(entry)) = mod_dir_entry.next_entry().await {
      if SymlinkManager::exists(entry.path()) && ignore_modpacks {
         continue;
      }

      let target = mod_install_dir.join(entry.file_name());

      // if the file is a symlink and ignore_modpacks = false, copy the file.
      if (SymlinkManager::exists(entry.path()) && !ignore_modpacks) || copy_mods {
         info!(
            "Copying {:?} from {} to {}",
            entry.file_name(),
            entry.path().display(),
            target.display()
         );
         tokio::fs::copy(entry.path(), target).await.map_err(|e| {
            LithicError::SimpleError(format!("Failed to copy {}: {}", entry.path().display(), e))
         })?;
      } else {
         info!(
            "Moving {:?} from {} to {}",
            entry.file_name(),
            target.display(),
            entry.path().display()
         );
         tokio::fs::rename(entry.path(), target).await.map_err(|e| {
            LithicError::SimpleError(format!("Failed to move {}: {}", entry.path().display(), e))
         })?;
      }
   }

   // now update the config to place our new modpack into the disabled list
   let mut config = get_config().write().await;

   if !config
      .modpacks
      .disabled
      .iter()
      .any(|m| m.eq_ignore_ascii_case(mod_pack.mod_id.as_ref()))
   {
      config.modpacks.disabled.push(mod_pack.mod_id.to_string());
   }
   config.save(None)?;

   drop(config);

   Ok((mod_zip_save_path, mod_install_dir))
}
