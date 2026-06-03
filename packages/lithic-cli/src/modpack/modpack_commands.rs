use crate::commands::arg_structs::modpack_args::{MPLocalSubCommands, ModpackCommands, ModpackSubCommands};
use crate::commands::info::info;
use crate::commands::list::cmd_list;
use crate::handle_sync_call;
use crate::modpack::mp_create::{collect_mp_create_args, mp_create};
use crate::modpack::mp_delete::delete_mpk_cmd;
use crate::modpack::mp_disable::mp_disable;
use crate::modpack::mp_enable::mp_enable;
use crate::modpack::mp_install::{mp_install, mp_install_missing_deps};
use crate::modpack::mp_update::mp_update;
use comfy_table::presets::UTF8_HORIZONTAL_ONLY;
use comfy_table::{Attribute, Color};
use lithic_core::aliases::{ModID, ModVersion};
use lithic_core::config::manager::get_config;
use lithic_core::utils::{command_output, display_table, notice};
use std::path::Path;
use std::process::exit;
use tracing::{error, info, warn};
use yansi::Paint;

pub async fn parse_modpack_commands(commands: &ModpackCommands, mod_dir: impl AsRef<Path>) {
   match &commands.subcommand {
      ModpackSubCommands::Create(args) => {
         let mut parse_args = match collect_mp_create_args(args) {
            Ok(m) => m,
            Err(e) => {
               error!(
                  "(THIS IS A BUG, Please report it) - Failed collecting modpack commands.. {}",
                  e.to_string()
               );
               exit(1);
            }
         };
         match mp_create(
            &mod_dir,
            &mut parse_args,
            args.save_path.clone(),
            args.copy_mods,
            args.ignore_modpacks,
         )
         .await
         {
            Ok((zip_location, mods_location)) => {
               display_table(
                  vec![
                     command_output(
                        "Your modpack has been created and saved to:",
                        zip_location.display().to_string(),
                     ),
                     command_output(
                        "Your modpack mods have been saved to:",
                        mods_location.display().to_string(),
                     ),
                  ],
                  Some(UTF8_HORIZONTAL_ONLY),
               );
            }
            Err(e) => {
               error!("{}", e.to_string().red().bold());
            }
         }
      }
      ModpackSubCommands::Delete(args) => {
         match delete_mpk_cmd(args.mpk_id.clone().into()).await {
            Ok(mpk_id) => {
               // do config write things
               let mut config = get_config().write().await;
               config
                  .modpacks
                  .disabled
                  .retain(|m| !m.eq_ignore_ascii_case(mpk_id.as_ref()));
               config.save(None).unwrap();

               notice(
                  format!("{mpk_id} has been deleted successfully!"),
                  Some(Color::Green),
                  vec![Attribute::Bold],
               );
            }
            Err(e) => {
               notice(e.to_string(), Some(Color::Yellow), vec![Attribute::Bold]);
            }
         }
      }
      ModpackSubCommands::Install(args) => {
         if args.missing_dependencies {
            match mp_install_missing_deps(args.mod_id.clone().into()).await {
               Ok(()) => {}
               Err(e) => {
                  error!("{}", e.to_string().red());
               }
            }
         } else {
            match mp_install(
               args.mod_id.clone().into(),
               args.mod_version.clone().map(ModVersion::from),
            )
            .await
            {
               Ok(installed) => {
                  // We update the config AFTER the installation so we know the lock on the config file is up
                  let mut config = get_config().write().await;
                  if !config.modpacks.disabled.contains(&installed) {
                     config.modpacks.disabled.push(installed);
                     match config.save(None) {
                        Ok(_) => {}
                        Err(e) => {
                           error!("{}", e.red().bold());
                        }
                     }
                  }
               }
               Err(e) => {
                  notice(
                     "Failed to install modpack. Maybe you have the wrong ID?",
                     Some(Color::Red),
                     vec![Attribute::Bold],
                  );
                  // hide the error for cleaner UX
                  info!("{}", e.to_string().red().bold());
               }
            }
         }
      }
      ModpackSubCommands::Enable(args) => {
         match mp_enable(ModID::from(args.mpk_id.clone()), mod_dir, args.force).await {
            Ok(enabled_pack) => {
               let mut config = get_config().write().await;
               config.modpacks.enabled.push(enabled_pack.clone());
               config
                  .modpacks
                  .disabled
                  .retain(|e| !e.eq_ignore_ascii_case(&enabled_pack));
               match config.save(None) {
                  Ok(()) => {
                     notice(
                        format!("Modpack: [{enabled_pack}] has been enabled!"),
                        Some(Color::Green),
                        vec![Attribute::Bold],
                     );
                  }
                  Err(e) => {
                     // If we fail to save, we should remove the symlinks
                     error!("{}", e.to_string().red().bold());
                  }
               }
            }
            Err(e) => {
               warn!(
                  "Failed to enable modpack.. Make sure there are no lingering symlinks in your default mods dir. :{}",
                  e.to_string().red().bold()
               );
            }
         }
      }
      ModpackSubCommands::Disable(args) => {
         match mp_disable(ModID::from(args.mpk_id.clone()), mod_dir).await {
            Ok(disabled_pack) => {
               let mut config = get_config().write().await;
               config
                  .modpacks
                  .enabled
                  .retain(|m| !m.eq_ignore_ascii_case(&disabled_pack));
               config.modpacks.disabled.push(disabled_pack.to_string());
               match config.save(None) {
                  Ok(()) => {
                     notice(
                        format!("Modpack: [{disabled_pack}] has been disabled!"),
                        Some(Color::Green),
                        vec![Attribute::Bold],
                     );
                  }
                  Err(e) => {
                     error!("{}", e.to_string().red().bold());
                  }
               }
            }
            Err(e) => {
               error!("Failed to disable modpack.. :{}", e.to_string().red().bold());
            }
         }
      }
      ModpackSubCommands::List(args) => {
         let config = get_config().read().await;
         let packs_path = Path::new(&config.modpacks.modpack_dir).join("packs");
         match cmd_list(
            &packs_path,
            args.updates,
            false,
            true,
            false,
            args.export_args.columns.clone(),
            args.export_args.export_as.clone(),
            args.export_args.file_path.clone(),
         )
         .await
         {
            Ok(()) => {}
            Err(e) => {
               error!("{}", e.to_string().red().bold());
            }
         }
      }
      ModpackSubCommands::Update(args) => match mp_update(args.clone()).await {
         Ok(()) => {}
         Err(e) => {
            error!("{}", e.to_string().red().bold());
         }
      },
      ModpackSubCommands::Info(args) => match info(args).await {
         Ok(()) => {}
         Err(e) => {
            error!("{}", e.to_string().red().bold());
         }
      },
      ModpackSubCommands::Sync => {
         let config = get_config().read().await;
         let mp_dir = Path::new(&config.modpacks.modpack_dir).join("packs");

         handle_sync_call(&mp_dir, false).await;
      }

      ModpackSubCommands::Local(args) => match &args.subcommands {
         MPLocalSubCommands::List(largs) => {
            let config = get_config().read().await;
            let packs_path = Path::new(&config.modpacks.modpack_dir).join("mypacks");

            match cmd_list(
               &packs_path,
               false,
               false,
               true,
               true,
               largs.output_commands.columns.clone(),
               largs.output_commands.export_as.clone(),
               largs.output_commands.file_path.clone(),
            )
            .await
            {
               Ok(()) => {}
               Err(e) => {
                  error!("{}", e.to_string().red().bold());
               }
            }
         }
         MPLocalSubCommands::Delete(largs) => match delete_mpk_cmd(largs.mpk_id.clone().into()).await {
            Ok(mpk_id) => {
               let mut config = get_config().write().await;
               config
                  .modpacks
                  .disabled
                  .retain(|m| !m.eq_ignore_ascii_case(mpk_id.as_ref()));
               if let Err(e) = config.save(None) {
                  error!("{}", e.to_string().red().bold());
               }
               notice(
                  format!("Local modpack {mpk_id} deleted."),
                  Some(Color::Green),
                  vec![Attribute::Bold],
               );
            }
            Err(e) => {
               notice(e.to_string(), Some(Color::Yellow), vec![Attribute::Bold]);
            }
         },
      },
   }
}
