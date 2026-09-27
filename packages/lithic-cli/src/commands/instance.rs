use std::path::PathBuf;

use comfy_table::Cell;
use lithic_core::{Error, Instance, Kind, instance::NewInstance, paths};
use serde::Serialize;

use super::{resolve_game_version, warn_if_not_installed};
use crate::{
  Ctx,
  args::{
    InstanceCommand,
    InstanceCreateArgs,
    InstanceEditArgs,
    InstancePathArgs,
  },
  ui::{Failure, Result, Ui, fail, format_duration, format_time},
};

#[derive(Serialize)]
struct InstanceView<'a> {
  #[serde(flatten)]
  instance:          &'a Instance,
  id:                &'a str,
  selected:          bool,
  resolved_data_dir: PathBuf,
  resolved_mods_dir: PathBuf,
  mods:              usize,
}

pub async fn run(ctx: &Ctx, cmd: InstanceCommand) -> Result {
  match cmd {
    InstanceCommand::List => list(ctx),
    InstanceCommand::Show { id } => show(ctx, id.as_deref()),
    InstanceCommand::Create(args) => create(ctx, args).await,
    InstanceCommand::Adopt(args) => {
      let dir = match args.dir {
        Some(dir) => paths::expand_home(dir),
        None => {
          match paths::stock_game_data_dirs().into_iter().next() {
            Some(dir) => dir,
            None => {
              return fail(
                "no VintagestoryData folder found; pass the folder to adopt",
              );
            },
          }
        },
      };
      if !dir.is_dir() {
        return fail(format!("{} is not a folder", dir.display()));
      }
      let game_version = match args.game {
        Some(v) => Some(resolve_game_version(ctx, &v).await?),
        None => None,
      };
      let instance = ctx.lithic.create_instance(NewInstance {
        name: args.name,
        game_version: game_version.clone(),
        data_dir: Some(dir.clone()),
        ..NewInstance::default()
      })?;
      ctx.ui.success(format!(
        "created instance `{}` using {}",
        instance.id,
        dir.display()
      ));
      if game_version.is_none() {
        ctx.ui.status(format!(
          "Set its game version with `lithic instance edit {} --game \
           <version>`",
          instance.id
        ));
      }
      Ok(())
    },
    InstanceCommand::Edit(args) => edit(ctx, args).await,
    InstanceCommand::Clone {
      id,
      name,
      with_saves,
    } => {
      let copy = ctx.lithic.clone_instance(&id, &name, with_saves)?;
      ctx.ui.success(format!("copied `{id}` to `{}`", copy.id));
      Ok(())
    },
    InstanceCommand::Remove { id } => {
      let instance = ctx.lithic.instance(&id)?;
      let question = if instance.has_external_data() {
        format!(
          "Remove instance \"{}\"? Its data folder {} is kept.",
          instance.name,
          instance.data_dir().display()
        )
      } else {
        format!(
          "Remove instance \"{}\" and everything in {}, including saved \
           worlds?",
          instance.name,
          instance.dir.display()
        )
      };
      if !ctx.ui.confirm(&question)? {
        return fail("");
      }
      ctx.lithic.delete_instance(&id)?;
      ctx.ui.success(format!("removed `{id}`"));
      Ok(())
    },
    InstanceCommand::Select { id } => {
      ctx.lithic.set_active_instance(Some(&id))?;
      ctx.ui.success(format!("`{id}` is now selected"));
      Ok(())
    },
    InstanceCommand::Path(args) => path(ctx, &args),
  }
}

fn list(ctx: &Ctx) -> Result {
  let listing = ctx.lithic.list_instances()?;
  let selected = ctx.lithic.settings()?.active_instance;
  for (id, error) in &listing.broken {
    ctx
      .ui
      .warn(format!("instance `{id}` could not be read: {error}"));
  }

  let views: Vec<InstanceView> = listing
    .instances
    .iter()
    .map(|i| view(ctx, i, selected.as_deref()))
    .collect();
  if ctx.ui.json {
    return Ui::print_json(&views);
  }
  if views.is_empty() {
    ctx.ui.status(
      "No instances yet. Create one with `lithic instance create <name> \
       --game latest`.",
    );
    return Ok(());
  }

  let mut table = ctx.ui.table();
  table.set_header(vec![
    "",
    "Id",
    "Name",
    "Game",
    "Mods",
    "Last played",
    "Played",
  ]);
  for v in &views {
    let i = v.instance;
    table.add_row(vec![
      Cell::new(if v.selected { "*" } else { "" }),
      Cell::new(v.id),
      Cell::new(&i.name),
      Cell::new(i.game_version.as_deref().unwrap_or("-")),
      Cell::new(v.mods),
      Cell::new(
        i.stats
          .last_played_at
          .map_or_else(|| "never".into(), format_time),
      ),
      Cell::new(format_duration(i.stats.play_time_ms)),
    ]);
  }
  Ui::print_table(&table);
  Ok(())
}

fn view<'a>(
  ctx: &Ctx,
  instance: &'a Instance,
  selected: Option<&str>,
) -> InstanceView<'a> {
  InstanceView {
    instance,
    id: &instance.id,
    selected: selected == Some(instance.id.as_str()),
    resolved_data_dir: instance.data_dir(),
    resolved_mods_dir: instance.mods_dir(),
    mods: ctx.lithic.installed_mods(instance).map_or(0, |m| m.len()),
  }
}

fn show(ctx: &Ctx, id: Option<&str>) -> Result {
  let instance = ctx.instance(id)?;
  let selected = ctx.lithic.settings()?.active_instance;
  let v = view(ctx, &instance, selected.as_deref());
  if ctx.ui.json {
    return Ui::print_json(&v);
  }

  let accounts = ctx.lithic.accounts()?;
  let account = accounts
    .for_instance(instance.account.as_deref())
    .map_or_else(
      || "none, the game will ask you to log in".into(),
      |a| {
        let how = if instance.account.as_deref() == Some(a.uid.as_str()) {
          ""
        } else {
          " (active account)"
        };
        format!("{}{how}", a.playername)
      },
    );
  let game = instance.game_version.as_ref().map_or_else(
    || "not set".to_string(),
    |v| {
      match ctx.lithic.game_install(v) {
        Ok(install) => format!("{v} ({})", install.path.display()),
        Err(_) => format!("{v} (not installed)"),
      }
    },
  );

  let mut rows: Vec<(&str, String)> = vec![
    ("Id", instance.id.clone()),
    ("Name", instance.name.clone()),
    ("Selected", if v.selected { "yes" } else { "no" }.into()),
    ("Game", game),
    ("Account", account),
    ("Data folder", instance.data_dir().display().to_string()),
    ("Mods folder", instance.mods_dir().display().to_string()),
    ("Mods", v.mods.to_string()),
    (
      "Last played",
      instance
        .stats
        .last_played_at
        .map_or_else(|| "never".into(), format_time),
    ),
    ("Play time", format_duration(instance.stats.play_time_ms)),
  ];
  if !instance.launch.args.is_empty() {
    rows.push(("Game arguments", instance.launch.args.join(" ")));
  }
  if !instance.launch.wrapper.is_empty() {
    rows.push(("Wrapper", instance.launch.wrapper.join(" ")));
  }
  for (k, val) in &instance.launch.env {
    rows.push(("Environment", format!("{k}={val}")));
  }

  let mut table = ctx.ui.table();
  for (k, val) in rows {
    table.add_row(vec![Cell::new(k), Cell::new(val)]);
  }
  Ui::print_table(&table);
  Ok(())
}

async fn create(ctx: &Ctx, args: InstanceCreateArgs) -> Result {
  let game_version = match &args.game {
    Some(v) => Some(resolve_game_version(ctx, v).await?),
    None => None,
  };
  let instance = ctx.lithic.create_instance(NewInstance {
    name:         args.name,
    id:           args.id,
    game_version: game_version.clone(),
    data_dir:     args.data_dir,
    mods_dir:     args.mods_dir,
  })?;
  if args.select || ctx.lithic.settings()?.active_instance.is_none() {
    ctx.lithic.set_active_instance(Some(&instance.id))?;
  }
  if ctx.ui.json {
    return Ui::print_json(&view(ctx, &instance, Some(&instance.id)));
  }
  ctx.ui.success(format!(
    "created instance `{}` in {}",
    instance.id,
    instance.dir.display()
  ));
  match game_version {
    Some(v) => warn_if_not_installed(ctx, &v),
    None => {
      ctx.ui.status(format!(
        "Set its game version with `lithic instance edit {} --game <version>`",
        instance.id
      ))
    },
  }
  Ok(())
}

async fn edit(ctx: &Ctx, args: InstanceEditArgs) -> Result {
  let instance = ctx.instance(args.id.as_deref())?;
  let game = match &args.game {
    Some(v) => Some(resolve_game_version(ctx, v).await?),
    None => None,
  };
  let account = match &args.account {
    Some(wanted) => {
      let accounts = ctx.lithic.accounts()?;
      let found = accounts
        .accounts
        .iter()
        .find(|a| a.uid == *wanted || a.playername.eq_ignore_ascii_case(wanted))
        .ok_or_else(|| Error::not_found(Kind::Account, wanted.clone()))?;
      Some(found.uid.clone())
    },
    None => None,
  };
  let game_args = match &args.args {
    Some(line) => Some(parse_command_line(line)?),
    None => None,
  };
  let wrapper = match &args.wrapper {
    Some(line) => Some(parse_command_line(line)?),
    None => None,
  };
  let mut env_changes = Vec::new();
  for pair in &args.env {
    let Some((k, v)) = pair.split_once('=') else {
      return fail(format!("`{pair}` is not KEY=VALUE"));
    };
    env_changes.push((k.trim().to_string(), v.to_string()));
  }

  ctx.lithic.update_instance(&instance.id, |i| {
    if let Some(name) = &args.name {
      i.name.clone_from(name);
    }
    if let Some(v) = &game {
      i.game_version = Some(v.clone());
    }
    if args.no_game {
      i.game_version = None;
    }
    if let Some(uid) = &account {
      i.account = Some(uid.clone());
    }
    if args.no_account {
      i.account = None;
    }
    if let Some(dir) = &args.mods_dir {
      i.mods_dir = Some(paths::expand_home(dir));
    }
    if args.no_mods_dir {
      i.mods_dir = None;
    }
    if let Some(a) = &game_args {
      i.launch.args.clone_from(a);
    }
    if let Some(w) = &wrapper {
      i.launch.wrapper.clone_from(w);
    }
    for (k, v) in &env_changes {
      i.launch.env.insert(k.clone(), v.clone());
    }
    for k in &args.unset_env {
      i.launch.env.remove(k);
    }
    Ok(())
  })?;
  ctx.ui.success(format!("updated `{}`", instance.id));
  if let Some(v) = game {
    warn_if_not_installed(ctx, &v);
  }
  Ok(())
}

fn parse_command_line(line: &str) -> Result<Vec<String>> {
  shell_words::split(line)
    .map_err(|e| Failure(format!("cannot split `{line}`: {e}")))
}

#[expect(
  clippy::print_stdout,
  reason = "the resolved instance path is CLI output"
)]
fn path(ctx: &Ctx, args: &InstancePathArgs) -> Result {
  let instance = ctx.instance(args.id.as_deref())?;
  let path = if args.data {
    instance.data_dir()
  } else if args.mods {
    instance.mods_dir()
  } else if args.logs {
    instance.logs_dir()
  } else {
    instance.dir
  };
  println!("{}", path.display());
  Ok(())
}
