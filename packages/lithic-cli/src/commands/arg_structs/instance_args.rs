use clap::{Args, Subcommand};

#[derive(Args, Debug, Clone)]
pub struct InstanceCommands {
   #[command(subcommand)]
   pub subcommand: InstanceSubCommands,
}

#[derive(Subcommand, Debug, Clone)]
pub enum InstanceSubCommands {
   /// List every configured instance with its id, name, and mods directory.
   #[command(about = "List configured instances")]
   List,
   /// Show the currently active instance (the one `launch` will start).
   #[command(about = "Show the active instance")]
   Show,
   /// Mark an instance as active. Affects every subsequent mod operation.
   #[command(about = "Select the active instance by id")]
   Select(InstanceSelectArgs),
   /// Create a new instance, or update an existing one with the same id.
   #[command(about = "Create or update an instance")]
   Upsert(InstanceUpsertArgs),
   /// Remove an instance from Lithic's config. Files on disk are not touched.
   #[command(about = "Delete an instance from Lithic")]
   Delete(InstanceDeleteArgs),
}

#[derive(Args, Debug, Clone)]
pub struct InstanceSelectArgs {
   /// ID of the instance to make active.
   pub id: String,
}

#[derive(Args, Debug, Clone)]
pub struct InstanceDeleteArgs {
   /// ID of the instance to remove from Lithic.
   pub id: String,
}

#[derive(Args, Debug, Clone)]
pub struct InstanceUpsertArgs {
   /// Stable identifier of the instance. Reuse to update an existing instance.
   pub id: String,
   /// Human-readable name shown in listings.
   #[arg(long)]
   pub name: String,
   /// Directory containing this instance's mods (typically `<data>/Mods`).
   #[arg(long)]
   pub mods_dir: String,
   /// Vintage Story data directory for this instance (saves, configs, etc.).
   #[arg(long, default_value = "")]
   pub data_dir: String,
   /// `id` of a registered game version (see `lithic game-version list`).
   #[arg(long, default_value = "")]
   pub game_version_id: String,
   /// Extra command-line arguments appended to the game invocation.
   #[arg(long, default_value = "")]
   pub start_params: String,
   /// Environment variables for the launched process, comma-separated.
   /// Format: `KEY=VALUE,OTHER=VAL2`.
   #[arg(long, default_value = "")]
   pub env_vars: String,
}
