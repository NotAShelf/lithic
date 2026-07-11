use std::path::Path;

use comfy_table::{Attribute, Color};
use tracing::warn;

#[cfg(windows)]
use is_elevated::is_elevated;

use lithic_core::aliases::ModID;
use lithic_core::config::manager::get_config;
use lithic_core::errors::LithicError;
use lithic_core::symlink_manager::SymlinkManager;
use lithic_core::utils::extract_all_mods_metadata;
use lithic_core::utils::notice;
#[cfg(windows)]
use std::process::exit;

/// Disable a modpack by removing its symlinks from `mod_dir`.
///
/// # Errors
///
/// Returns an error if installed metadata cannot be read, symlinks cannot be
/// removed, or config cannot be saved.
pub async fn mp_disable(mpk_id: ModID, mod_dir: impl AsRef<Path>) -> Result<ModID, LithicError> {
   #[cfg(windows)]
   if !is_elevated() {
      notice(
         "In order to disable modpacks, Lithic uses symlinks which require admin permissions on Windows. Please run Lithic with admin rights and try again.",
         Some(Color::Red),
         vec![Attribute::Bold],
      );
      exit(1);
   }

   let config = get_config().read().await;

   let mod_pack_dir = Path::new(&config.modpacks.modpack_dir)
      .join("installed")
      .join(&mpk_id);

   if !mod_pack_dir.exists() {
      return Err(LithicError::SimpleError(
         "Modpack {} doesn't exist. Run 'Lithic modpack list' to view installed modpacks.".into(),
      ));
   }

   if !config
      .modpacks
      .enabled
      .iter()
      .any(|m| m.eq_ignore_ascii_case(mpk_id.as_ref()))
   {
      notice(
         format!(
            "The requested modpack [{}] is not enabled, or you misstyped the ID",
            &mpk_id
         ),
         Some(Color::Yellow),
         vec![Attribute::Bold],
      );
      return Err(LithicError::SimpleError("Modpack is not enabled".into()));
   }

   // check if requested modpack is enabled

   // if it is, get list of mods in that modpack, then remove them from the mod_dir

   let mods_in_pack: Vec<_> = extract_all_mods_metadata(mod_pack_dir, false)
      .await?
      .keys()
      .cloned()
      .collect();

   // iterate through mods in the pack and try to remove the symlink

   for m in mods_in_pack {
      let p = mod_dir.as_ref().join(m);
      if SymlinkManager::exists(&p) {
         SymlinkManager::remove(&p)?;
      } else {
         warn!("Mod {} is no longer linked. Skipping..", p.display());
      }
   }

   Ok(mpk_id)
}
