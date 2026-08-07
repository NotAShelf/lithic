mod account;
mod browse;
mod desktop;
mod game;
mod instance;
mod launch;
mod mods;
mod pack;
mod settings;

use lithic_core::Freshness;

use crate::Ctx;
use crate::args::{Command, ModsCommand};
use crate::ui::{Failure, Result};

pub async fn run(ctx: &Ctx, command: Command) -> Result {
   match command {
      Command::Instance(cmd) => instance::run(ctx, cmd).await,
      Command::Mods(cmd) => mods::run(ctx, cmd).await,
      Command::List(args) => mods::run(ctx, ModsCommand::List(args)).await,
      Command::Install(args) => mods::run(ctx, ModsCommand::Install(args)).await,
      Command::Update(args) => mods::run(ctx, ModsCommand::Update(args)).await,
      Command::Remove(args) => mods::run(ctx, ModsCommand::Remove(args)).await,
      Command::Search(args) => browse::search(ctx, args).await,
      Command::Info(args) => browse::info(ctx, args).await,
      Command::Game(cmd) => game::run(ctx, cmd).await,
      Command::Launch(args) => launch::launch(ctx, args).await,
      Command::Logs(args) => launch::logs(ctx, &args),
      Command::Account(cmd) => account::run(ctx, cmd).await,
      Command::Pack(cmd) => pack::run(ctx, cmd).await,
      Command::Settings(cmd) => settings::run(ctx, cmd),
      Command::DesktopEntry => desktop::install(ctx),
      Command::Completions { .. } => Ok(()),
   }
}

/// Turns `latest` into the newest stable game version and strips a leading
/// `v` from anything else.
pub(crate) async fn resolve_game_version(ctx: &Ctx, input: &str) -> Result<String> {
   let input = input.trim();
   if input.eq_ignore_ascii_case("latest") {
      let manifest = ctx.lithic.game_manifest(Freshness::Cached).await?;
      return manifest
         .latest_stable()
         .map(|r| r.version.clone())
         .ok_or_else(|| Failure("the game release list is empty".to_string()));
   }
   Ok(input.trim_start_matches(['v', 'V']).to_string())
}

/// Warns when no build of `version` is installed, since launching would fail.
pub(crate) fn warn_if_not_installed(ctx: &Ctx, version: &str) {
   if ctx.lithic.game_install(version).is_err() {
      ctx.ui.warn(format!(
         "game version {version} is not installed; run `lithic game install {version}` before launching"
      ));
   }
}
