use std::env;

use comfy_table::Cell;
use lithic_core::{
  Cancel,
  Freshness,
  game::{Platform, find_executable},
};

use super::resolve_game_version;
use crate::{
  Ctx,
  args::{GameCommand, PlatformArg},
  ui::{Result, Ui, fail},
};

pub async fn run(ctx: &Ctx, cmd: GameCommand) -> Result {
  match cmd {
    GameCommand::List => list(ctx),
    GameCommand::Available { unstable, limit } => {
      let manifest = ctx.lithic.game_manifest(Freshness::Cached).await?;
      let installed = ctx.lithic.game_installs()?;
      let host = Platform::host_client();
      let releases: Vec<_> = manifest
        .releases
        .iter()
        .filter(|r| unstable || !r.is_prerelease())
        .take(limit)
        .collect();
      if ctx.ui.json {
        return Ui::print_json(&releases);
      }
      let mut table = ctx.ui.table();
      table.set_header(vec!["Version", "Channel", "Size", "Installed"]);
      for r in releases {
        let artifact = host.and_then(|p| r.artifacts.get(&p));
        table.add_row(vec![
          Cell::new(&r.version),
          Cell::new(if r.is_prerelease() {
            "unstable"
          } else {
            "stable"
          }),
          Cell::new(
            artifact
              .and_then(|a| a.size.as_deref())
              .unwrap_or("no build for this system"),
          ),
          Cell::new(if installed.iter().any(|i| i.version == r.version) {
            "yes"
          } else {
            ""
          }),
        ]);
      }
      Ui::print_table(&table);
      Ok(())
    },
    GameCommand::Install { version } => {
      let version = resolve_game_version(ctx, &version).await?;
      let progress = ctx.ui.progress();
      let result = ctx
        .lithic
        .install_game(&version, &progress.reporter, &Cancel::new())
        .await;
      drop(progress);
      let install = result?;
      if ctx.ui.json {
        return Ui::print_json(&install);
      }
      ctx.ui.success(format!(
        "installed Vintage Story {} in {}",
        install.version,
        install.path.display()
      ));
      Ok(())
    },
    GameCommand::Add { version, path } => {
      let install = ctx.lithic.add_game_install(&version, &path)?;
      ctx.ui.success(format!(
        "added Vintage Story {} at {}",
        install.version,
        install.path.display()
      ));
      Ok(())
    },
    GameCommand::Remove { version } => {
      let install = ctx.lithic.game_install(&version)?;
      let question = if install.managed {
        format!(
          "Remove Vintage Story {} and delete {}?",
          install.version,
          install.path.display()
        )
      } else {
        format!(
          "Forget Vintage Story {}? Its files in {} are kept.",
          install.version,
          install.path.display()
        )
      };
      if !ctx.ui.confirm(&question)? {
        return fail("");
      }
      ctx.lithic.remove_game_install(&install.version)?;
      ctx
        .ui
        .success(format!("removed Vintage Story {}", install.version));
      Ok(())
    },
    GameCommand::Download {
      version,
      platform,
      dir,
    } => {
      let version = resolve_game_version(ctx, &version).await?;
      let platform = match platform {
        Some(p) => to_platform(p),
        None => {
          match Platform::host_client() {
            Some(p) => p,
            None => return fail("pick a build with --platform"),
          }
        },
      };
      let settings = ctx.lithic.settings()?;
      let dir = dir
        .or(settings.game.download_dir)
        .or_else(|| env::current_dir().ok())
        .unwrap_or_default();
      let progress = ctx.ui.progress();
      let result = ctx
        .lithic
        .download_game(
          &version,
          platform,
          &dir,
          &progress.reporter,
          &Cancel::new(),
        )
        .await;
      drop(progress);
      let path = result?;
      ctx
        .ui
        .success(format!("downloaded and verified {}", path.display()));
      Ok(())
    },
  }
}

fn list(ctx: &Ctx) -> Result {
  let installs = ctx.lithic.game_installs()?;
  if ctx.ui.json {
    return Ui::print_json(&installs);
  }
  if installs.is_empty() {
    ctx.ui.status(
      "No game versions yet. Install one with `lithic game install latest`.",
    );
    return Ok(());
  }
  let instances = ctx.lithic.list_instances()?.instances;
  let mut table = ctx.ui.table();
  table.set_header(vec!["Version", "Path", "Installed by", "Used by"]);
  for i in &installs {
    let users: Vec<&str> = instances
      .iter()
      .filter(|x| x.game_version.as_deref() == Some(i.version.as_str()))
      .map(|x| x.name.as_str())
      .collect();
    let path = if find_executable(&i.path).is_some() {
      i.path.display().to_string()
    } else {
      format!("{} (missing)", i.path.display())
    };
    table.add_row(vec![
      Cell::new(&i.version),
      Cell::new(path),
      Cell::new(if i.managed { "lithic" } else { "you" }),
      Cell::new(users.join(", ")),
    ]);
  }
  Ui::print_table(&table);
  Ok(())
}

const fn to_platform(p: PlatformArg) -> Platform {
  match p {
    PlatformArg::Linux => Platform::Linux,
    PlatformArg::Windows => Platform::Windows,
    PlatformArg::MacX64 => Platform::MacX64,
    PlatformArg::MacArm64 => Platform::MacArm64,
    PlatformArg::LinuxServer => Platform::LinuxServer,
    PlatformArg::WindowsServer => Platform::WindowsServer,
  }
}
