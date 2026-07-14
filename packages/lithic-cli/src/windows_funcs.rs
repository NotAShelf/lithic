use crate::commands::delete::iterate_and_move_zip;
use crate::modpack::mp_disable::mp_disable;
use crate::modpack::mp_enable::mp_enable;
use comfy_table::{Attribute, CellAlignment, Color};
use is_elevated::is_elevated;
use lithic_core::aliases::ModID;
use lithic_core::config::manager::get_config;
use lithic_core::errors::LithicError;
use lithic_core::utils::extract_all_mods_metadata;
use lithic_core::utils::{CellData, LithicMessage, lithic_message, notice};
use std::env;
use std::path::Path;
use std::process::exit;
use tracing::{error, info};

pub async fn check_old_default_windows() -> Result<(), LithicError> {
   // check what is currently the default in the config - if it exists.
   // check if there are any mods in the old default location
   // prompt the user if they would like to move the mods to the preferred default location
   // if yes, move the mods and update the config file
   // if no, don't move mods, ask if they would like to silence the check so they are not bugged again in the future

   let config = get_config().read().await;
   let mod_dir = config.mod_dir.clone();
   drop(config);

   if let Some(app_data) = env::var_os("APPDATA") {
      let old_default = Path::new(&app_data).join("Vintagestory").join("Mods");
      if !old_default.exists() {
         // Means game is not installed or user setup their own path, so just exit.
         return Ok(());
      }

      let new_default = Path::new(&app_data).join("VintagestoryData").join("Mods");

      // path is valid, check if any mods exist. This will only look for the presence of .zip mod files
      let mod_metadata = extract_all_mods_metadata(&old_default, false).await?;

      // First check if there are any mods present and prompt the user.
      // if they choose to do the swittch, check for enabled modpacks that need to be disabled
      // move all .zips over.
      // enable any modpacks that were enabled before
      let mut can_proceed = false;
      if !mod_metadata.is_empty() {
         // prompt user if we can proceed
         lithic_message(LithicMessage {
                header: Some(CellData::new("Attention!".into(), Some(Color::Yellow), vec![Attribute::Bold], Some(CellAlignment::Center))),
                message: vec![
                    CellData::new("Currently, you are using the old default location for mods, which is:".into(), Some(Color::Yellow), vec![], Some(CellAlignment::Center)),
                    CellData::new(old_default.to_string_lossy().to_string(), Some(Color::Cyan), vec![Attribute::Bold], Some(CellAlignment::Center)),
                    CellData::blank(),
                    CellData::new("The correct default should be:".into(), Some(Color::Yellow), vec![], Some(CellAlignment::Center)),
                    CellData::new(new_default.to_string_lossy().to_string(), Some(Color::Cyan), vec![], Some(CellAlignment::Center)),
                    CellData::blank(),
                    CellData::new("Lithic can update this location and move your mods. This changes only the default mod location that Lithic uses and WILL NOT affect gameplay.".into(), Some(Color::Yellow), vec![], Some(CellAlignment::Center)),
                    CellData::blank(),
                    CellData::new("Would you like lithic to update this location? Type: Y or N".into(), Some(Color::Green), vec![], Some(CellAlignment::Center)),
                ],
            });

         loop {
            // print!("Would you like Lithic to update your default path? [Y/N]: ");
            let mut input = String::new();
            std::io::stdin()
               .read_line(&mut input)
               .expect("Unable to read input, try again.");
            match input.trim().to_lowercase().as_str() {
               "y" | "yes" => {
                  can_proceed = true;
                  break;
               }
               "n" | "no" => break,
               _ => println!("Please enter 'y' or 'yes', 'n' or 'no'"),
            }
         }

         if !can_proceed {
            println!();
            notice(
               "Ok, Lithic will not update your mod location. To prevent Lithic from checking again, use the following command:",
               Some(Color::Green),
               vec![],
            );
            notice(
               "Lithic config set --update-default-windows-loc false",
               Some(Color::Cyan),
               vec![Attribute::Bold],
            );
            return Ok(());
         }

         // User has given permission to update the location.. do the stuff
         // check for any enabled modpacks and disable them
         let enabled_modpacks: Vec<String> = {
            let config = get_config().read().await;
            config.modpacks.enabled.clone()
         };

         if !enabled_modpacks.is_empty() {
            if !is_elevated() {
               notice(
                  "You have modpacks enabled. Lithic will need admin right to disable, then enable your modpacks.",
                  Some(Color::Yellow),
                  vec![Attribute::Bold],
               );
               exit(0);
            }
            info!("User running with admin right, continuing");

            for mpk_id in &enabled_modpacks {
                match mp_disable(ModID::from(mpk_id.clone()), &old_default).await {
                  Ok(modpack) => {
                     let mut config = get_config().write().await;
                     config
                        .modpacks
                        .enabled
                        .retain(|m| !m.eq_ignore_ascii_case(&modpack));
                      config.modpacks.disabled.push(modpack.to_string());
                     config.save(None)?;
                     info!("disabled {modpack}")
                  }
                  Err(e) => {
                     error!("Lithic was enable to disable your modpack {mpk_id} and cannot continue: {e}");
                     exit(1);
                  }
               }
            }
         }

         // iterate though all the .zips and move them to the new location
         let mut mods = tokio::fs::read_dir(&old_default).await?;

         if let Err(e) = iterate_and_move_zip(&mut mods, &new_default, true).await {
            notice(
               format!(
                  "Lithic ran into errors while attempting to move your mods. You will need to move them manually, then use the following command to reset the default dir to the new one: {e}"
               ),
               Some(Color::Red),
               vec![Attribute::Bold],
            );
            notice("Lithic config del -m:", Some(Color::Cyan), vec![Attribute::Bold]);
            exit(1);
         }

         // files have been moved, update the default location and enable any modpacks

         if Path::new(&mod_dir) == old_default {
            let mut config = get_config().write().await;
             config.mod_dir = new_default.to_path_buf();
            config.update_default_windows_loc = false; // set this to false so we don't try to run the update again
            config.save(None)?;
            info!("Updated mod_dir in config to new path {}", new_default.display());
         }

         for mpk_id in &enabled_modpacks {
             match mp_enable(ModID::from(mpk_id.clone()), &new_default, true).await {
               Ok(modpack) => {
                  let mut config = get_config().write().await;
                  info!("Enabled {modpack}");
                  config.modpacks.enabled.push(modpack.clone());
                  config
                     .modpacks
                     .disabled
                     .retain(|m| !m.eq_ignore_ascii_case(&modpack));
                  config.save(None)?;
                  drop(config);
               }
               Err(e) => {
                  notice(
                     format!(
                        "Lithic was enable to enable your modpack {mpk_id}, try using Lithic enable {mpk_id} instead: {e}"
                     ),
                     Some(Color::Red),
                     vec![Attribute::Bold],
                  );
               }
            }
         }

         notice(
            format!(
               "Your default mod location has been updated! You can now find your mods in {}",
               new_default.display()
            ),
            Some(Color::Green),
            vec![Attribute::Bold],
         );
      }
   } else {
      info!("Unable to read the appdata folder. This should not happen and will cause errors with lithic");
      return Ok(());
   }

   Ok(())
}
