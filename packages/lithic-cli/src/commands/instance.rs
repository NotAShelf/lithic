use crate::commands::arg_structs::instance_args::{
   InstanceCommands, InstanceSubCommands, InstanceUpsertArgs,
};
use lithic_core::instance::{InstanceConfig, get_active_instance};
use std::path::PathBuf;

fn to_instance(args: &InstanceUpsertArgs) -> InstanceConfig {
   InstanceConfig {
      id: args.id.clone(),
      name: args.name.clone(),
      data_dir: PathBuf::from(&args.data_dir),
      mods_dir: PathBuf::from(&args.mods_dir),
      game_version_id: args.game_version_id.clone(),
      enabled_modpacks: Vec::new(),
      start_params: args.start_params.clone(),
      env_vars: args.env_vars.clone(),
      last_played_at: 0,
      total_play_time_ms: 0,
   }
}

/// Dispatch instance subcommands.
///
/// # Errors
///
/// Returns an error if loading, saving, deleting, activating, or launching an
/// instance fails.
pub async fn parse_instance_commands(commands: &InstanceCommands) -> Result<(), String> {
   match &commands.subcommand {
      InstanceSubCommands::List => {
         let instances = lithic_core::instance::list_instances().await?;
         for inst in instances {
            println!(
               "{}\t{}\t{}\t{}",
               inst.id,
               inst.name,
               inst.mods_dir.display(),
               inst.game_version_id
            );
         }
         Ok(())
      }
      InstanceSubCommands::Show => {
         let active = get_active_instance().await?;
         if let Some(inst) = active {
            println!(
               "active: {}\nname: {}\nmods_dir: {}\ndata_dir: {}\ngame_version_id: {}",
               inst.id,
               inst.name,
               inst.mods_dir.display(),
               inst.data_dir.display(),
               inst.game_version_id
            );
         } else {
            println!("No active instance.");
         }
         Ok(())
      }
      InstanceSubCommands::Select(args) => lithic_core::instance::set_active_instance(&args.id).await,
      InstanceSubCommands::Upsert(args) => {
         lithic_core::instance::add_or_update_instance(to_instance(args)).await
      }
      InstanceSubCommands::Delete(args) => lithic_core::instance::remove_instance(&args.id).await,
   }
}
