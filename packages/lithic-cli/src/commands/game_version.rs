use crate::commands::arg_structs::game_version_args::{
   GameVersionCommands, GameVersionSourceArg, GameVersionSubCommands,
};
use lithic_core::api::client::VSOSType;
use lithic_core::instance::{GameVersionInstall, GameVersionInstallOptions, GameVersionSource};
use std::path::PathBuf;

fn map_source(source: GameVersionSourceArg) -> GameVersionSource {
   match source {
      GameVersionSourceArg::Manual => GameVersionSource::Manual,
      GameVersionSourceArg::LithicDownload => GameVersionSource::LithicDownload,
   }
}

/// Dispatch game-version subcommands.
///
/// # Errors
///
/// Returns an error if loading, saving, deleting, or installing game-version
/// entries fails.
pub async fn parse_game_version_commands(commands: &GameVersionCommands) -> Result<(), String> {
   match &commands.subcommand {
      GameVersionSubCommands::List => {
         let versions = lithic_core::instance::list_game_versions().await?;
         for v in versions {
            println!("{}\t{}\t{}\t{:?}", v.id, v.version, v.path.display(), v.source);
         }
         Ok(())
      }
      GameVersionSubCommands::Add(args) => {
         lithic_core::instance::add_or_update_game_version(GameVersionInstall {
            id: args.id.clone(),
            version: args.version.clone(),
            path: PathBuf::from(&args.path),
            source: map_source(args.source),
            os: VSOSType::host(),
         })
         .await
      }
      GameVersionSubCommands::Install(args) => {
         let installed = lithic_core::instance::install_game_version(GameVersionInstallOptions {
            id: args.id.clone().unwrap_or_else(|| args.version.clone()),
            version: args.version.clone(),
            install_dir: args.install_dir.clone().map(PathBuf::from),
            os_type: args.os_type.clone(),
            exe_type: args.exe_type.clone(),
            windows_installer_type: args.windows_installer_type.clone(),
         })
         .await?;
         println!(
            "installed {}\t{}\t{}",
            installed.id,
            installed.version,
            installed.path.display()
         );
         Ok(())
      }
      GameVersionSubCommands::Remove(args) => lithic_core::instance::remove_game_version(&args.id).await,
   }
}
