use clap::{Args, Subcommand, ValueEnum};
use lithic_core::api::client::{VSExecutabletype, VSOSType, VSWinInstallerType};

#[derive(Args, Debug, Clone)]
pub struct GameVersionCommands {
   #[command(subcommand)]
   pub subcommand: GameVersionSubCommands,
}

#[derive(Subcommand, Debug, Clone)]
pub enum GameVersionSubCommands {
   /// List every game version known to Lithic (installed and manually attached).
   #[command(about = "List installed and attached game versions")]
   List,
   /// Register a pre-existing Vintage Story install with Lithic without downloading anything.
   #[command(about = "Attach an existing game install to Lithic")]
   Add(GameVersionAddArgs),
   /// Download Vintage Story from the official CDN and register the result.
   #[command(about = "Download and install a Vintage Story version")]
   Install(GameVersionInstallArgs),
   /// Forget a registered version. The on-disk install is left untouched.
   #[command(about = "Unregister a game version from Lithic")]
   Remove(GameVersionRemoveArgs),
}

#[derive(Args, Debug, Clone)]
pub struct GameVersionAddArgs {
   /// Short identifier used to refer to this install in other commands.
   pub id: String,
   /// Version label (e.g. `1.20.0`). Used for filtering and display only.
   #[arg(long)]
   pub version: String,
   /// Absolute path to the existing Vintage Story install directory.
   #[arg(long)]
   pub path: String,
   /// How this install was obtained. Affects what Lithic is allowed to delete.
   #[arg(long, value_enum, default_value = "manual")]
   pub source: GameVersionSourceArg,
}

#[derive(Args, Debug, Clone)]
pub struct GameVersionRemoveArgs {
   /// ID of the game version to unregister.
   pub id: String,
}

#[derive(Args, Debug, Clone)]
pub struct GameVersionInstallArgs {
   /// Optional identifier to register the install under. Defaults to `version`.
   #[arg(long)]
   pub id: Option<String>,
   /// Version to download from the Vintage Story CDN (e.g. `1.20.0`).
   #[arg(long)]
   pub version: String,
   /// Directory to install into. Defaults to Lithic's managed games directory.
   #[arg(long)]
   pub install_dir: Option<String>,
   #[arg(short, long, value_name = "OS", default_value_t = os_default())]
   pub os_type: VSOSType,
   #[arg(short = 't', long = "type", value_name = "TYPE", default_value = "client")]
   pub exe_type: VSExecutabletype,
   #[arg(short, long, default_value = "install")]
   pub windows_installer_type: Option<VSWinInstallerType>,
}

#[derive(ValueEnum, Debug, Clone, Copy)]
pub enum GameVersionSourceArg {
   Manual,
   LithicDownload,
}

fn os_default() -> VSOSType {
   #[cfg(target_os = "macos")]
   return VSOSType::OSX;
   #[cfg(target_os = "windows")]
   return VSOSType::Windows;
   #[cfg(target_os = "linux")]
   VSOSType::Linux
}
