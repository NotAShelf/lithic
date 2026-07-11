use comfy_table::presets::UTF8_HORIZONTAL_ONLY;
use comfy_table::{Attribute, Color};
use std::path::Path;

#[cfg(windows)]
use is_elevated::is_elevated;

use lithic_core::aliases::ModID;
use lithic_core::config::manager::get_config;
use lithic_core::errors::LithicError;
use lithic_core::symlink_manager::SymlinkManager;
use lithic_core::utils::extract_all_mods_metadata;
use lithic_core::utils::{CellData, display_table, notice};
#[cfg(windows)]
use std::process::exit;

/// Enable a modpack by linking its installed mods into `mod_dir`.
///
/// # Errors
///
/// Returns an error if the modpack cannot be found, symlinks cannot be created,
/// or config cannot be saved.
pub async fn mp_enable(mpk_id: ModID, mod_dir: impl AsRef<Path>, force: bool) -> Result<String, LithicError> {
   #[cfg(windows)]
   if !is_elevated() {
      notice(
         "In order to enable modpacks, Lithic uses symlinks which require admin permissions on Windows. Please run Lithic with admins right and try again.",
         Some(Color::Red),
         vec![Attribute::Bold],
      );
      exit(1);
   }

   let config = get_config().read().await;
   let mod_pack_install_dir = Path::new(&config.modpacks.modpack_dir).join("installed");
   let full_dir_with_mpk_id = mod_pack_install_dir.join(&mpk_id);

   if !full_dir_with_mpk_id.exists() {
      return Err(LithicError::SimpleError(format!(
         "Modpack {} doesn't exist in {}. Run 'Lithic modpack list' or 'Lithic modpack local list' to view your installed modpacks.",
         &mpk_id,
         &mod_pack_install_dir.display()
      )));
   }

   // check if a modpack already exists
   // if so, notify the user and tell them to either disable the current one OR use modpack enable -f to force the use and warn about using multiple

   // Is it already enabled?
   if config
      .modpacks
      .enabled
      .iter()
      .any(|m| m.eq_ignore_ascii_case(mpk_id.as_ref()))
   {
      notice(
         format!(
            "Modpack: [{}] is already enabled. Did you mean to disable it?",
            &mpk_id
         ),
         Some(Color::Yellow),
         vec![Attribute::Bold],
      );
      return Err(LithicError::SimpleError("Modpack already enabled".into()));
   }

   // Is it even installed??
   if !config
      .modpacks
      .disabled
      .iter()
      .any(|m| m.eq_ignore_ascii_case(mpk_id.as_ref()))
   {
      notice(
         format!(
            "Modpack: [{}] is not installed! Use [Lithic modpack install {}] to install it first.",
            &mpk_id, &mpk_id
         ),
         Some(Color::Yellow),
         vec![Attribute::Bold],
      );
      return Err(LithicError::SimpleError(
         "Modpack needs to be installed first".into(),
      ));
   }

   // Is anything else enabled?
   if !config.modpacks.enabled.is_empty() && !force {
      display_table(
         vec![(
            CellData::new(
               "You already have the following modpack(s) enabled: ".into(),
               Some(Color::Yellow),
               vec![],
               None,
            ),
            CellData::new(
               config.modpacks.enabled.join(","),
               Some(Color::Magenta),
               vec![],
               None,
            ),
         )],
         Some(UTF8_HORIZONTAL_ONLY),
      );

      notice(
         "Run this command again with --force to enable it anyway..",
         Some(Color::Yellow),
         vec![],
      );
      return Err(LithicError::SimpleError(format!(
         "Modpacks already enabled {}",
         config.modpacks.enabled.join(",")
      )));
   }

   // we know that the modpack is installed and IS NOT enabled
   // lets enable it

   // get list of mods for the modpack
   // create symlinks is the Mods folder
   // return the modpack id that was enabled

   let mod_list: Vec<_> = extract_all_mods_metadata(&full_dir_with_mpk_id, false)
      .await?
      .keys()
      .cloned()
      .collect();

   for m in mod_list {
      let target = &full_dir_with_mpk_id.join(&m);
      let link = mod_dir.as_ref().join(&m);
      SymlinkManager::create(target, link).await?;
   }

   Ok(mpk_id.to_string())
}
