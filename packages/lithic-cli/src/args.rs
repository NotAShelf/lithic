use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

#[derive(Debug, Parser)]
#[command(
   name = "lithic",
   version,
   about = "Manage Vintage Story instances, game versions and mods",
   long_about = "Manage Vintage Story instances, game versions and mods.\n\n\
      Commands that work on mods act on the selected instance (`lithic instance select`) \
      unless --instance names another one."
)]
pub struct Cli {
   #[command(flatten)]
   pub global: Global,

   #[command(subcommand)]
   pub command: Command,
}

#[derive(Debug, Args)]
pub struct Global {
   /// Instance to act on instead of the selected one
   #[arg(short, long, global = true, value_name = "ID")]
   pub instance: Option<String>,

   /// Print machine-readable JSON on stdout
   #[arg(long, global = true)]
   pub json: bool,

   /// Answer yes to confirmation prompts
   #[arg(short, long, global = true)]
   pub yes: bool,

   /// Show more log output; repeat for more detail
   #[arg(short, long, global = true, action = clap::ArgAction::Count)]
   pub verbose: u8,

   /// Only print errors
   #[arg(short, long, global = true, conflicts_with = "verbose")]
   pub quiet: bool,

   /// When to use colours
   #[arg(long, global = true, value_enum, default_value_t = ColorChoice::Auto)]
   pub color: ColorChoice,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ColorChoice {
   Auto,
   Always,
   Never,
}

#[derive(Debug, Subcommand)]
pub enum Command {
   /// Create, change and remove instances
   #[command(subcommand, visible_alias = "i")]
   Instance(InstanceCommand),

   /// Manage the mods of an instance
   #[command(subcommand, visible_alias = "m")]
   Mods(ModsCommand),

   /// List the mods of an instance (same as `mods list`)
   List(ModsListArgs),

   /// Install mods (same as `mods install`)
   Install(ModsInstallArgs),

   /// Update mods (same as `mods update`)
   Update(ModsUpdateArgs),

   /// Remove mods (same as `mods remove`)
   Remove(ModsRemoveArgs),

   /// Search the ModDB
   Search(SearchArgs),

   /// Show a mod's details from the ModDB
   Info(InfoArgs),

   /// Install and manage game versions
   #[command(subcommand, visible_alias = "g")]
   Game(GameCommand),

   /// Start the game for an instance
   Launch(LaunchArgs),

   /// Show an instance's logs
   Logs(LogsArgs),

   /// Log in to Vintage Story accounts
   #[command(subcommand)]
   Account(AccountCommand),

   /// Export and import modpacks
   #[command(subcommand)]
   Pack(PackCommand),

   /// Show and change settings
   #[command(subcommand, visible_alias = "config")]
   Settings(SettingsCommand),

   /// Print a shell completion script
   Completions {
      #[arg(value_enum)]
      shell: clap_complete::Shell,
   },
}

#[derive(Debug, Subcommand)]
pub enum InstanceCommand {
   /// List instances
   #[command(visible_alias = "ls")]
   List,

   /// Show one instance
   Show { id: Option<String> },

   /// Create an instance
   Create(InstanceCreateArgs),

   /// Turn an existing game data folder (such as the stock launcher's
   /// VintagestoryData) into an instance without moving it
   Adopt(InstanceAdoptArgs),

   /// Change an instance's settings
   Edit(InstanceEditArgs),

   /// Copy an instance, its mods and its settings
   Clone {
      id: String,
      name: String,
      /// Copy saved worlds too
      #[arg(long)]
      with_saves: bool,
   },

   /// Delete an instance and its files (external data folders are kept)
   #[command(visible_alias = "rm")]
   Remove { id: String },

   /// Make an instance the one commands act on by default
   Select { id: String },

   /// Print one of an instance's folders
   Path(InstancePathArgs),
}

#[derive(Debug, Args)]
pub struct InstanceCreateArgs {
   pub name: String,
   /// Folder name; derived from the name when omitted
   #[arg(long)]
   pub id: Option<String>,
   /// Game version, such as 1.21.5 or `latest`
   #[arg(long, value_name = "VERSION")]
   pub game: Option<String>,
   /// Use this data folder instead of one inside the instance
   #[arg(long, value_name = "DIR")]
   pub data_dir: Option<PathBuf>,
   /// Also load mods from this folder and install into it
   #[arg(long, value_name = "DIR")]
   pub mods_dir: Option<PathBuf>,
   /// Make the new instance the selected one
   #[arg(long)]
   pub select: bool,
}

#[derive(Debug, Args)]
pub struct InstanceAdoptArgs {
   /// Data folder to adopt; the stock launcher's is used when omitted
   pub dir: Option<PathBuf>,
   #[arg(long, default_value = "Vintage Story")]
   pub name: String,
   #[arg(long, value_name = "VERSION")]
   pub game: Option<String>,
}

#[derive(Debug, Args)]
pub struct InstanceEditArgs {
   /// Instance to edit; defaults to the selected one
   pub id: Option<String>,
   #[arg(long)]
   pub name: Option<String>,
   /// Game version, such as 1.21.5 or `latest`
   #[arg(long, value_name = "VERSION", conflicts_with = "no_game")]
   pub game: Option<String>,
   #[arg(long)]
   pub no_game: bool,
   /// Account uid or player name to launch with
   #[arg(long, conflicts_with = "no_account")]
   pub account: Option<String>,
   /// Launch with the active account
   #[arg(long)]
   pub no_account: bool,
   #[arg(long, value_name = "DIR", conflicts_with = "no_mods_dir")]
   pub mods_dir: Option<PathBuf>,
   #[arg(long)]
   pub no_mods_dir: bool,
   /// Game arguments, parsed like a shell command line; replaces the current ones
   #[arg(long, value_name = "ARGS", allow_hyphen_values = true)]
   pub args: Option<String>,
   /// Set an environment variable for the game
   #[arg(long, value_name = "KEY=VALUE")]
   pub env: Vec<String>,
   /// Remove an environment variable
   #[arg(long, value_name = "KEY")]
   pub unset_env: Vec<String>,
   /// Program to start the game through, such as `gamemoderun`; empty to clear
   #[arg(long, value_name = "COMMAND", allow_hyphen_values = true)]
   pub wrapper: Option<String>,
}

#[derive(Debug, Args)]
pub struct InstancePathArgs {
   pub id: Option<String>,
   #[arg(long, group = "which")]
   pub data: bool,
   #[arg(long, group = "which")]
   pub mods: bool,
   #[arg(long, group = "which")]
   pub logs: bool,
}

#[derive(Debug, Subcommand)]
pub enum ModsCommand {
   /// List installed mods
   #[command(visible_alias = "ls")]
   List(ModsListArgs),
   /// Install mods from the ModDB
   #[command(visible_alias = "add")]
   Install(ModsInstallArgs),
   /// Update mods, or list available updates with --check
   Update(ModsUpdateArgs),
   /// Remove mods
   #[command(visible_alias = "rm")]
   Remove(ModsRemoveArgs),
   /// Turn mods on
   Enable { mods: Vec<String> },
   /// Turn mods off without removing them
   Disable { mods: Vec<String> },
   /// Keep a mod on a version; updates skip it
   Pin {
      #[arg(value_name = "MOD")]
      id: String,
      /// Defaults to the installed version
      version: Option<String>,
   },
   /// Let updates move a mod again
   Unpin {
      #[arg(value_name = "MOD")]
      id: String,
   },
   /// Report missing, outdated, duplicate and unreadable mods
   Check,
}

#[derive(Debug, Args)]
pub struct ModsListArgs {
   /// Also show the file name of each mod
   #[arg(long)]
   pub files: bool,
}

#[derive(Debug, Args)]
pub struct ModsInstallArgs {
   /// Mod ids, id@version, ModDB links or vintagestorymodinstall:// links
   #[arg(required = true, value_name = "MOD")]
   pub mods: Vec<String>,
   /// Do not install missing dependencies
   #[arg(long)]
   pub no_deps: bool,
   /// Download again even if the version is already installed
   #[arg(long)]
   pub reinstall: bool,
}

#[derive(Debug, Args)]
pub struct ModsUpdateArgs {
   /// Only these mods; all when omitted
   #[arg(value_name = "MOD")]
   pub mods: Vec<String>,
   /// Only list what would be updated
   #[arg(long, visible_alias = "dry-run")]
   pub check: bool,
}

#[derive(Debug, Args)]
pub struct ModsRemoveArgs {
   #[arg(required = true, value_name = "MOD")]
   pub mods: Vec<String>,
   /// Keep dependencies that nothing else needs
   #[arg(long)]
   pub keep_deps: bool,
}

#[derive(Debug, Args)]
pub struct SearchArgs {
   pub query: Vec<String>,
   #[arg(long, value_enum, default_value_t = SortArg::Relevance)]
   pub sort: SortArg,
   #[arg(long, default_value_t = 25)]
   pub limit: usize,
   /// Only mods with a release for this game version (the instance's by
   /// default when one is selected)
   #[arg(long, value_name = "VERSION", conflicts_with = "any_version")]
   pub game: Option<String>,
   /// Do not filter by game version
   #[arg(long)]
   pub any_version: bool,
   /// Fetch a fresh mod list instead of the cached one
   #[arg(long)]
   pub refresh: bool,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum SortArg {
   Relevance,
   Downloads,
   Follows,
   Trending,
   Updated,
   Name,
}

#[derive(Debug, Args)]
pub struct InfoArgs {
   #[arg(value_name = "MOD")]
   pub id: String,
   /// List every release
   #[arg(long)]
   pub releases: bool,
   /// Show the changelog of the newest releases
   #[arg(long)]
   pub changelog: bool,
}

#[derive(Debug, Subcommand)]
pub enum GameCommand {
   /// Installed game versions
   #[command(visible_alias = "ls")]
   List,
   /// Versions that can be installed
   Available {
      /// Include release candidates and pre-releases
      #[arg(long)]
      unstable: bool,
      #[arg(long, default_value_t = 20)]
      limit: usize,
   },
   /// Download and install a version for this computer
   Install {
      /// A version such as 1.21.5, or `latest`
      version: String,
   },
   /// Register a game folder you installed yourself
   Add { version: String, path: PathBuf },
   /// Forget a version; files are deleted only if lithic installed it
   #[command(visible_alias = "rm")]
   Remove { version: String },
   /// Download an archive or installer without installing it
   Download {
      version: String,
      #[arg(long, value_enum)]
      platform: Option<PlatformArg>,
      /// Defaults to the download folder from settings
      #[arg(long, value_name = "DIR")]
      dir: Option<PathBuf>,
   },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum PlatformArg {
   Linux,
   Windows,
   MacX64,
   MacArm64,
   LinuxServer,
   WindowsServer,
}

#[derive(Debug, Args)]
pub struct LaunchArgs {
   /// Instance to start; defaults to the selected one
   pub id: Option<String>,
   /// Return right away instead of waiting for the game to exit (play time
   /// is not recorded)
   #[arg(long)]
   pub detach: bool,
   /// Print the command instead of running it
   #[arg(long)]
   pub dry_run: bool,
}

#[derive(Debug, Args)]
pub struct LogsArgs {
   pub id: Option<String>,
   /// Lines to show
   #[arg(short = 'n', long, default_value_t = 60)]
   pub lines: usize,
   /// Show the game's client-main.log instead of lithic's launch output
   #[arg(long)]
   pub game: bool,
   /// List log files instead of printing one
   #[arg(long)]
   pub list: bool,
}

#[derive(Debug, Subcommand)]
pub enum AccountCommand {
   /// Log in; prompts for the password and a two-factor code if needed
   Login {
      #[arg(long)]
      email: Option<String>,
      /// Read the password from standard input
      #[arg(long)]
      password_stdin: bool,
      /// Two-factor code, if you already have one
      #[arg(long, value_name = "CODE")]
      code: Option<String>,
   },
   /// Known accounts
   #[command(visible_alias = "ls")]
   List,
   /// Use this account for instances that do not name one
   Switch { account: String },
   /// Forget an account and its session
   Logout { account: String },
}

#[derive(Debug, Subcommand)]
pub enum PackCommand {
   /// Write an instance to a pack file
   Export {
      /// Instance; defaults to the selected one
      id: Option<String>,
      #[arg(short, long, value_name = "FILE")]
      output: Option<PathBuf>,
      /// Include mod settings (ModConfig)
      #[arg(long)]
      config: bool,
      /// Put every mod file in the pack so it installs without the ModDB
      #[arg(long)]
      bundle_all: bool,
      #[arg(long)]
      description: Option<String>,
   },
   /// Create a new instance from a pack file
   Import {
      file: PathBuf,
      #[arg(long)]
      name: Option<String>,
   },
   /// Show what a pack contains
   Show { file: PathBuf },
}

#[derive(Debug, Subcommand)]
pub enum SettingsCommand {
   /// All settings
   Show,
   /// One setting, by dotted key such as mods.allow_prerelease
   Get { key: String },
   /// Change a setting
   Set { key: String, value: String },
   /// Reset a setting to its default
   Unset { key: String },
   /// Where lithic keeps its files
   Paths,
   /// Colours of the `list` and `search` tables
   #[command(subcommand)]
   Table(TableCommand),
}

#[derive(Debug, Subcommand)]
pub enum TableCommand {
   /// Current table colours
   Show,
   /// Colour a column
   Set {
      #[arg(value_enum)]
      table: TableName,
      #[arg(value_enum)]
      part: TablePart,
      column: String,
      #[arg(long, value_enum)]
      color: Option<crate::style::CellColor>,
      #[arg(long, value_enum)]
      attribute: Option<crate::style::CellAttr>,
   },
   /// Go back to the default colours
   Reset {
      #[arg(value_enum)]
      table: Option<TableName>,
   },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum TableName {
   List,
   Search,
}

impl TableName {
   pub fn key(self) -> &'static str {
      match self {
         TableName::List => "list",
         TableName::Search => "search",
      }
   }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum TablePart {
   Headers,
   Cells,
}

impl TablePart {
   pub fn key(self) -> &'static str {
      match self {
         TablePart::Headers => "headers",
         TablePart::Cells => "cells",
      }
   }
}

#[cfg(test)]
mod tests {
   use super::*;
   use clap::CommandFactory;

   #[test]
   fn definition_is_consistent() {
      Cli::command().debug_assert();
   }

   #[test]
   fn global_flags_work_after_subcommands() {
      let cli = Cli::try_parse_from(["lithic", "list", "-v", "--json", "-i", "x"]).unwrap();
      assert_eq!(cli.global.verbose, 1);
      assert!(cli.global.json);
      assert_eq!(cli.global.instance.as_deref(), Some("x"));
   }

   #[test]
   fn hyphenated_game_args_are_accepted() {
      let cli = Cli::try_parse_from(["lithic", "instance", "edit", "--args", "-x --connect host"]).unwrap();
      let Command::Instance(InstanceCommand::Edit(edit)) = cli.command else {
         panic!("wrong command");
      };
      assert_eq!(edit.args.as_deref(), Some("-x --connect host"));
   }
}
