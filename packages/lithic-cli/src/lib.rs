#![warn(clippy::perf, clippy::pedantic)]
#![warn(clippy::manual_string_new)]
#![allow(
   clippy::redundant_closure_for_method_calls,
   clippy::struct_field_names,
   clippy::doc_markdown,
   clippy::unnecessary_wraps,
   clippy::collapsible_if,
   clippy::unnecessary_to_owned,
   clippy::implicit_clone,
   clippy::too_many_lines,
   clippy::cmp_owned,
   clippy::needless_borrows_for_generic_args,
   clippy::ignored_unit_patterns,
   clippy::needless_borrow,
   clippy::unnecessary_semicolon
)]

pub mod cli_commands;
pub mod commands;
pub mod logging;
pub mod modpack;
pub mod updater;

#[cfg(windows)]
pub mod windows_funcs;

use crate::cli_commands::{Cli, Commands, ShellType};
use crate::commands::changelog::changelog as changelog_cmd;
use crate::commands::config::parse_config_args;
use crate::commands::delete::{delete_all, delete_cmd};
use crate::commands::download::download;
use crate::commands::game_version::parse_game_version_commands;
use crate::commands::info::info;
use crate::commands::install::{install_cmd, install_missing_deps};
use crate::commands::instance::parse_instance_commands;
use crate::commands::launch::launch as launch_game;
use crate::commands::list::cmd_list;
use crate::commands::search::search;
use crate::commands::sync::{daily_file_syncs, game_version_sync};
use crate::logging::{VerboseLevel, init_logging};
use crate::modpack::modpack_commands::parse_modpack_commands;
use crate::updater::update_manager;
use crate::updater::update_manager::check_for_update;
use clap::{CommandFactory, FromArgMatches};
use clap_complete::{Shell, generate};
use color_eyre::Result;
use comfy_table::{Attribute, Color};
use commands::sync::sync;
use commands::update::update_mods;
use dirs::home_dir;
use lithic_core::aliases::ModID;
use lithic_core::config::manager::{get_config, init_config};
use lithic_core::errors::{ErrorMsgFn, handle_err_result};
use lithic_core::options::LithicOptions;
use lithic_core::traits::string_ext::StrLowerExt;
use lithic_core::utils::{elapsed_footer, notice};
use lithic_core::utils::{get_expanded_path, sorted_game_versions};
use std::fs::File;
use std::io::{self, Write, stdin};
use std::path::{Path, PathBuf};
use std::process::{Command, exit};
use std::time::Instant;
use tracing::{debug, error, info, warn};
use yansi::Paint;

#[allow(clippy::too_many_lines)]
pub async fn run() -> Result<()> {
   let cmd = Cli::command();
   let cli = Cli::from_arg_matches(&cmd.get_matches()).unwrap_or_else(|_| {
      error!("Error attempting to parse CLI arguments: ");
      exit(1)
   });

   let verbosity = if cli.debug {
      VerboseLevel::Debug
   } else if cli.verbose {
      VerboseLevel::Verbose
   } else {
      VerboseLevel::Default
   };
   init_logging(&verbosity);

   handle_err_result(init_config(), "init_config: ", false, ErrorMsgFn::Debug);

   if cli.verbose {
      info!("Verbose logging enabled");
   }

   if cli.debug {
      debug!("Debug logging enabled");
   }

   #[cfg(windows)]
   {
      if !matches!(cli.command, Commands::Config { .. }) {
         let update_windows_default_loc = {
            let config = get_config().read().await;
            config.update_default_windows_loc
         };

         if update_windows_default_loc {
            match windows_funcs::check_old_default_windows().await {
               Ok(_) => {}
               Err(e) => {
                  error!("Error attempting to update default mod path {}", e);
               }
            }
         }
      }
   }

   let mod_opts: LithicOptions = LithicOptions::default();
   let mut mod_dir = lithic_core::instance::resolve_active_mod_dir()
      .await
      .unwrap_or_else(|_| PathBuf::new());
   if mod_dir.as_os_str().is_empty() {
      mod_dir = mod_opts.get_mod_path().await;
   }
   if cli.mods_dir.is_some() {
      mod_dir = get_expanded_path(PathBuf::from(cli.mods_dir.clone().unwrap_or(String::new())));
      if !mod_dir.exists() {
         notice(
            "The directory you specified is not valid. Check your input for typos and try again.",
            Some(Color::Yellow),
            vec![Attribute::Bold],
         );
         exit(1);
      }
   }

   if !matches!(&cli.command, Commands::LithicSelf(_)) {
      let config = get_config().read().await;
      let _ = check_for_update(config.check_for_updates, true).await;
   }

   if cli.with_mpk.is_some() {
      let config = get_config().read().await;
      mod_dir = Path::new(&config.modpacks.modpack_dir)
         .join("installed")
         .join(cli.with_mpk.clone().unwrap_or(String::new()));
      if !mod_dir.exists() {
         notice(
            "The modpack you specified isn't installed. Double check your spelling and try again.",
            Some(Color::Yellow),
            vec![Attribute::Bold],
         );
         exit(1);
      }
   }

   info!("Operating on mods dir: {:?}", mod_dir);
   match &cli.command {
      Commands::Sync(args) => {
         if args.sync_search_db {
            handle_err_result(
               daily_file_syncs(args.sync_search_db).await,
               "Failed calling sync_search_db",
               true,
               ErrorMsgFn::Error,
            );
         } else if args.sync_game_versions {
            handle_err_result(
               game_version_sync(args.sync_game_versions).await,
               "Failed calling sync_game_version",
               true,
               ErrorMsgFn::Error,
            );
         } else {
            handle_sync_call(&mod_dir, false).await;
         }
      }
      Commands::List(args) => {
         if args.game_versions.is_some() {
            let Ok(sorted_versions) = sorted_game_versions().await else {
               warn!("Unable to get game versions. Run \"lithic sync\" and try again.");
               return Ok(());
            };
            let filter_by = &args.game_versions.clone().unwrap_or("1.20".into());

            let versions: Vec<String> = sorted_versions
               .into_iter()
               .filter(|v| v.lower_contains(filter_by))
               .collect();
            notice(
               format!("[{}]", versions.join("], [").as_str()),
               Some(Color::Yellow),
               vec![],
            );
         } else {
            handle_err_result(
               cmd_list(
                  &mod_dir,
                  args.updates,
                  args.pinned,
                  false,
                  false,
                  args.export_args.columns.clone(),
                  args.export_args.export_as.clone(),
                  args.export_args.file_path.clone(),
               )
               .await,
               "Failed to display list",
               true,
               ErrorMsgFn::Error,
            );
         }
      }
      Commands::Update(args) => {
         let mod_ids: Vec<ModID> = args.mod_ids.iter().cloned().map(Into::into).collect();
         match update_mods(&mod_dir, mod_ids, args.keep_old_files).await {
            Ok(()) => {
               handle_sync_call(&mod_dir, false).await;
            }
            Err(e) => {
               warn!("{}\n\r", e.to_string().red().bold());
               exit(1);
            }
         }
      }
      Commands::Download(args) => {
         handle_err_result(download(args).await, "Failed download:", true, ErrorMsgFn::Error);
      }
      Commands::Install(args) => {
         let start_time = Instant::now();
         let config = get_config().read().await;

         if !args.mod_ids.is_empty() {
            let mod_ids: Vec<ModID> = args.mod_ids.iter().cloned().map(Into::into).collect();
            match install_cmd(&mod_dir, mod_ids, args.missing_dependencies).await {
               Ok(()) => {
                  handle_sync_call(&mod_dir, false).await;
               }
               Err(e) => {
                  error!("{}", e);
               }
            }
         }

         if args.missing_dependencies {
            let mod_ids: Vec<ModID> = args.mod_ids.iter().cloned().map(Into::into).collect();
            match install_missing_deps(&mod_dir, mod_ids, &mod_dir).await {
               Ok(()) => {
                  handle_sync_call(&mod_dir, false).await;
               }
               Err(e) => {
                  error!("{}", e);
               }
            }
         }

         if config.show_execution_time {
            elapsed_footer(start_time, "Install");
         }

         #[cfg(unix)]
         if args.wait {
            println!("Press enter to exit...");
            stdin().read_line(&mut String::new()).unwrap();
         }
      }
      Commands::Config(config_cmd) => {
         parse_config_args(config_cmd).await;
      }
      Commands::Misc {
         gen_auto_complete: Some(shell),
         ..
      } => {
         generate_completion(shell.clone());
      }
      #[cfg(unix)]
      Commands::Misc {
         one_click_setup: true,
         ..
      } => {
         one_click_setup();
      }
      Commands::Misc { .. } => {}
      Commands::Info(args) => {
         handle_err_result(info(args).await, "Failed to call Info:", true, ErrorMsgFn::Info);
      }
      Commands::Search(args) => {
         handle_err_result(search(args).await, "Search failed:", true, ErrorMsgFn::Error);
      }
      Commands::Modpack(cmds) => {
         parse_modpack_commands(cmds, &mod_dir).await;
      }
      Commands::LithicSelf(args) => {
         if args.check_updates {
            handle_err_result(
               check_for_update(false, false).await,
               "Update check failed:",
               true,
               ErrorMsgFn::Error,
            );
         }

         if args.update {
            handle_err_result(
               update_manager::self_update_binary(args.force).await,
               "Lithic update failed",
               true,
               ErrorMsgFn::Error,
            );
         }
      }

      Commands::Delete(args) => {
         if !args.mod_id.is_empty() {
            handle_err_result(
               delete_cmd(
                  &mod_dir,
                  args.mod_id.iter().cloned().map(Into::into).collect(),
                  args.mod_backups,
               )
               .await,
               "Unable to delete mod(s)",
               true,
               ErrorMsgFn::Error,
            );
         }

         if args.all.is_some() {
            if let Some(which) = &args.all {
               handle_err_result(
                  delete_all(&mod_dir, which).await,
                  &format!("Unable to delete all mod(s) in {}", mod_dir.display()),
                  true,
                  ErrorMsgFn::Error,
               );
            }
         }
      }
      Commands::Instance(commands) => {
         if let Err(e) = parse_instance_commands(commands).await {
            error!("Instance command failed: {e}");
            exit(1);
         }
      }
      Commands::GameVersion(commands) => {
         if let Err(e) = parse_game_version_commands(commands).await {
            error!("Game-version command failed: {e}");
            exit(1);
         }
      }
      Commands::Launch(args) => {
         if let Err(e) = launch_game(args).await {
            error!("Launch failed: {e}");
            exit(1);
         }
      }
      Commands::Changelog(args) => {
         handle_err_result(
            changelog_cmd(args).await,
            "Changelog failed:",
            true,
            ErrorMsgFn::Error,
         );
      }
   }
   Ok(())
}

async fn handle_sync_call(mod_dir: impl AsRef<Path>, quiet: bool) {
   match sync(mod_dir.as_ref(), quiet, vec![]).await {
      Ok(_) => {}
      Err(e) => {
         error!("{}", e.to_string().red().bold());
         exit(1);
      }
   }
}

fn generate_completion(shell: ShellType) {
   let mut cmd = Cli::command();
   let shell: Shell = shell.into();

   generate(shell, &mut cmd, "lithic", &mut io::stdout());

   println!("\n# Completion script generated. To use it:");
   match shell {
      Shell::Bash => {
         println!("# Save the above output to ~/.local/share/bash-completion/completions/lithic");
         println!(
            "# Or run: lithic misc --gen-auto-complete bash > ~/.local/share/bash-completion/completions/lithic"
         );
      }
      Shell::Zsh => {
         println!("# Save the above output to ~/.zsh/completion/_lithic");
         println!("# Or run: lithic misc --gen-auto-complete zsh > ~/.zsh/completion/_lithic");
         println!("# Then add to your .zshrc: fpath=(~/.zsh/completion $fpath)");
      }
      Shell::Fish => {
         println!("# Save the above output to ~/.config/fish/completions/lithic.fish");
         println!("# Or run: lithic misc --gen-auto-complete fish > ~/.config/fish/completions/lithic.fish");
      }
      Shell::PowerShell => {
         println!("# Save the above output to a file and source it in your PowerShell profile");
         println!("# Or run: lithic misc --gen-auto-complete powershell > lithic.ps1");
      }
      _ => {}
   }
}

// Thanks coolcoder613 for the 1-click install setup!
#[cfg(unix)]
fn one_click_setup() {
   let exe_path = match std::env::current_exe() {
      Ok(exe_path) => exe_path,
      Err(e) => {
         error!("Unable to get Lithic executable path: {e}");
         exit(1);
      }
   };

   let text = include_str!("lithic.desktop").replace("{LITHIC_PATH}", &exe_path.to_string_lossy());

   let lithic_desktop_path = if let Some(home) = home_dir() {
      home.join(".local/share/applications/lithic.desktop")
   } else {
      error!("Unable to access your home directory. Check your permissions and try again. ");
      exit(1);
   };

   match File::create(lithic_desktop_path) {
      Ok(mut desktop_file) => match desktop_file.write_all(text.as_bytes()) {
         Ok(()) => {
            let _ = Command::new("xdg-mime")
               .arg("default")
               .arg("lithic.desktop")
               .arg("x-scheme-handler/vintagestorymodinstall")
               .status();
            notice(
               "Desktop file for 1-click mod install created successfully.",
               Some(Color::Green),
               vec![],
            );
         }
         Err(e) => {
            error!("Failed to write to desktop file: {}", e);
         }
      },
      Err(e) => {
         error!("Failed to open desktop file: {}", e);
      }
   }
}
