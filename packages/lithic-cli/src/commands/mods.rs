use std::collections::BTreeSet;

use lithic_core::mods::{self, InstallOptions, InstalledMod, ModRef, Problem, Reason, Report};
use lithic_core::{Cancel, Instance};
use serde::Serialize;

use crate::Ctx;
use crate::args::{ModsCommand, ModsInstallArgs, ModsListArgs, ModsRemoveArgs, ModsUpdateArgs};
use crate::ui::{Result, fail};

pub async fn run(ctx: &Ctx, cmd: ModsCommand) -> Result {
   match cmd {
      ModsCommand::List(args) => list(ctx, &args),
      ModsCommand::Install(args) => install(ctx, args).await,
      ModsCommand::Update(args) => update(ctx, args).await,
      ModsCommand::Remove(args) => remove(ctx, &args),
      ModsCommand::Enable { mods } => set_enabled(ctx, &mods, true),
      ModsCommand::Disable { mods } => set_enabled(ctx, &mods, false),
      ModsCommand::Pin { id, version } => {
         let instance = ctx.instance(None)?;
         let version = match version {
            Some(v) => v,
            None => {
               let installed = ctx.lithic.installed_mods(&instance)?;
               match installed.iter().find(|m| m.mod_id().eq_ignore_ascii_case(&id)) {
                  Some(m) if !m.info.version.is_empty() => m.info.version.clone(),
                  _ => return fail(format!("{id} is not installed; give the version to pin")),
               }
            }
         };
         ctx.lithic.set_mod_pin(&instance, &id, Some(&version))?;
         ctx.ui
            .success(format!("{id} stays on {version} in {}", instance.name));
         Ok(())
      }
      ModsCommand::Unpin { id } => {
         let instance = ctx.instance(None)?;
         ctx.lithic.set_mod_pin(&instance, &id, None)?;
         ctx.ui
            .success(format!("{id} is no longer pinned in {}", instance.name));
         Ok(())
      }
      ModsCommand::Check => check(ctx),
   }
}

fn list(ctx: &Ctx, args: &ModsListArgs) -> Result {
   let instance = ctx.instance(None)?;
   let installed = ctx.lithic.installed_mods(&instance)?;
   let problems = mods::problems(&installed);
   if ctx.ui.json {
      #[derive(Serialize)]
      struct Out<'a> {
         instance: &'a str,
         mods: &'a [InstalledMod],
         problems: &'a [Problem],
      }
      return ctx.ui.print_json(&Out {
         instance: &instance.id,
         mods: &installed,
         problems: &problems,
      });
   }
   if installed.is_empty() {
      ctx.ui.status(format!(
         "{} has no mods. Find some with `lithic search <words>`.",
         instance.name
      ));
      return Ok(());
   }

   let style = ctx.ui.style("list");
   let mut header = vec![
      style.header("name", "Name"),
      style.header("mod_id", "Id"),
      style.header("version", "Version"),
      style.header("state", "State"),
   ];
   if args.files {
      header.push(style.header("filename", "File"));
   }
   let mut table = ctx.ui.table();
   table.set_header(header);
   for m in &installed {
      let mut state = vec![if m.enabled { "on" } else { "off" }.to_string()];
      if let Some(pin) = &m.lock.pin {
         state.push(format!("pinned {pin}"));
      }
      if m.lock.dependency {
         state.push("dependency".into());
      }
      if m.error.is_some() {
         state.push("unreadable".into());
      }
      let mut row = vec![
         style.cell("name", m.display_name()),
         style.cell("mod_id", m.mod_id()),
         style.cell(
            "version",
            if m.info.version.is_empty() {
               "?"
            } else {
               &m.info.version
            },
         ),
         style.cell("state", state.join(", ")),
      ];
      if args.files {
         row.push(style.cell("filename", &m.file_name));
      }
      table.add_row(row);
   }
   ctx.ui.print_table(&table);
   let enabled = installed.iter().filter(|m| m.enabled).count();
   ctx.ui.status(format!(
      "{} mods in {}, {enabled} enabled",
      installed.len(),
      instance.name
   ));
   if !problems.is_empty() {
      ctx.ui.warn(format!(
         "{} problem(s) found; run `lithic mods check` for details",
         problems.len()
      ));
   }
   Ok(())
}

async fn install(ctx: &Ctx, args: ModsInstallArgs) -> Result {
   let instance = ctx.instance(None)?;
   let refs = args
      .mods
      .iter()
      .map(|m| ModRef::parse(m))
      .collect::<std::result::Result<Vec<_>, _>>()?;
   warn_without_game_version(ctx, &instance);

   let progress = ctx.ui.progress();
   let report = ctx
      .lithic
      .install_mods(
         &instance,
         &refs,
         &InstallOptions {
            dependencies: !args.no_deps,
            reinstall: args.reinstall,
            pin_versions: true,
            reporter: progress.reporter.clone(),
            cancel: Cancel::new(),
         },
      )
      .await;
   drop(progress);
   let report = report?;
   for id in &report.unchanged {
      ctx.ui
         .status(format!("{id} is already installed at the chosen version"));
   }
   print_report(ctx, &instance, &report)
}

async fn update(ctx: &Ctx, args: ModsUpdateArgs) -> Result {
   let instance = ctx.instance(None)?;
   warn_without_game_version(ctx, &instance);
   let progress = ctx.ui.progress();
   progress.reporter.log("Checking the ModDB for updates");
   let mut updates = ctx.lithic.check_updates(&instance, &Cancel::new()).await;
   drop(progress);
   let wanted: BTreeSet<String> = args.mods.iter().map(|m| m.to_ascii_lowercase()).collect();
   if let Ok(list) = &mut updates
      && !wanted.is_empty()
   {
      list.retain(|u| wanted.contains(&u.mod_id));
   }
   let updates = updates?;

   if args.check || updates.is_empty() {
      if ctx.ui.json {
         return ctx.ui.print_json(&updates);
      }
      if updates.is_empty() {
         ctx.ui
            .status(format!("Every mod in {} is up to date.", instance.name));
         return Ok(());
      }
      let style = ctx.ui.style("list");
      let mut table = ctx.ui.table();
      table.set_header(vec![
         style.header("name", "Name"),
         style.header("mod_id", "Id"),
         style.header("version", "Installed"),
         style.header("update", "Available"),
      ]);
      for u in &updates {
         let to = u.release.version.clone().unwrap_or_default();
         table.add_row(vec![
            style.cell("name", &u.name),
            style.cell("mod_id", &u.mod_id),
            style.cell("version", &u.installed),
            style.cell("update", if u.to_pin { format!("{to} (pinned)") } else { to }),
         ]);
      }
      ctx.ui.print_table(&table);
      ctx.ui.status(format!("{} update(s) available", updates.len()));
      return Ok(());
   }

   let progress = ctx.ui.progress();
   let report = ctx
      .lithic
      .update_mods(
         &instance,
         &updates,
         &InstallOptions {
            dependencies: true,
            pin_versions: false,
            reporter: progress.reporter.clone(),
            ..InstallOptions::default()
         },
      )
      .await;
   drop(progress);
   print_report(ctx, &instance, &report?)
}

fn remove(ctx: &Ctx, args: &ModsRemoveArgs) -> Result {
   let instance = ctx.instance(None)?;
   let installed = ctx.lithic.installed_mods(&instance)?;
   let targets: BTreeSet<String> = args.mods.iter().map(|m| m.to_ascii_lowercase()).collect();
   for id in &targets {
      let dependents: Vec<&str> = installed
         .iter()
         .filter(|m| !targets.contains(m.mod_id()) && m.info.dependencies.contains_key(id))
         .map(InstalledMod::display_name)
         .collect();
      if !dependents.is_empty() {
         ctx.ui
            .warn(format!("{} depend(s) on {id}", dependents.join(", ")));
      }
   }
   let names: Vec<&str> = targets.iter().map(String::as_str).collect();
   if !ctx
      .ui
      .confirm(&format!("Remove {} from {}?", names.join(", "), instance.name))?
   {
      return fail("");
   }
   let removed = ctx.lithic.remove_mods(&instance, &args.mods, !args.keep_deps)?;
   if ctx.ui.json {
      return ctx.ui.print_json(&removed);
   }
   for m in &removed {
      let why = if targets.contains(m.mod_id()) {
         ""
      } else {
         " (no longer needed)"
      };
      ctx.ui
         .success(format!("removed {} {}{why}", m.display_name(), m.info.version));
   }
   Ok(())
}

fn set_enabled(ctx: &Ctx, ids: &[String], enabled: bool) -> Result {
   if ids.is_empty() {
      return fail("name at least one mod");
   }
   let instance = ctx.instance(None)?;
   for id in ids {
      ctx.lithic.set_mod_enabled(&instance, id, enabled)?;
      ctx.ui
         .success(format!("{id} {}", if enabled { "enabled" } else { "disabled" }));
   }
   let problems = mods::problems(&ctx.lithic.installed_mods(&instance)?);
   if problems
      .iter()
      .any(|p| matches!(p, Problem::DisabledDependency { .. }))
   {
      ctx.ui
         .warn("some enabled mods now depend on a disabled one; see `lithic mods check`");
   }
   Ok(())
}

fn check(ctx: &Ctx) -> Result {
   let instance = ctx.instance(None)?;
   let problems = mods::problems(&ctx.lithic.installed_mods(&instance)?);
   if ctx.ui.json {
      ctx.ui.print_json(&problems)?;
   } else if problems.is_empty() {
      ctx.ui.success(format!("no problems found in {}", instance.name));
   } else {
      for p in &problems {
         println!("{}", describe(p));
      }
   }
   if problems.is_empty() {
      Ok(())
   } else {
      fail(format!("{} problem(s) found", problems.len()))
   }
}

fn describe(p: &Problem) -> String {
   match p {
      Problem::MissingDependency {
         mod_id,
         dependency,
         required,
      } => {
         let need = if required.is_empty() || required == "*" {
            String::new()
         } else {
            format!(" {required} or newer")
         };
         format!("{mod_id} needs {dependency}{need}, which is not installed (lithic install {dependency})")
      }
      Problem::OutdatedDependency {
         mod_id,
         dependency,
         required,
         installed,
      } => format!("{mod_id} needs {dependency} {required} or newer, but {installed} is installed"),
      Problem::DisabledDependency { mod_id, dependency } => {
         format!("{mod_id} needs {dependency}, which is disabled (lithic mods enable {dependency})")
      }
      Problem::Duplicate { mod_id, files } => {
         format!("{mod_id} is installed more than once: {}", files.join(", "))
      }
      Problem::Unreadable { file, error } => format!("{file} could not be read: {error}"),
   }
}

fn print_report(ctx: &Ctx, instance: &Instance, report: &Report) -> Result {
   if ctx.ui.json {
      #[derive(Serialize)]
      struct FailureOut<'a> {
         mod_id: &'a str,
         needed_by: Option<&'a str>,
         error: String,
      }
      #[derive(Serialize)]
      struct Out<'a> {
         changes: &'a [mods::Change],
         unchanged: &'a [String],
         failures: Vec<FailureOut<'a>>,
      }
      ctx.ui.print_json(&Out {
         changes: &report.changes,
         unchanged: &report.unchanged,
         failures: report
            .failures
            .iter()
            .map(|f| FailureOut {
               mod_id: &f.mod_id,
               needed_by: f.needed_by.as_deref(),
               error: f.error.to_string(),
            })
            .collect(),
      })?;
   } else {
      for c in &report.changes {
         let what = match (&c.from, &c.reason) {
            (Some(from), _) => format!("updated {} {from} -> {}", c.name, c.to),
            (None, Reason::Dependency { of }) => format!("installed {} {} (needed by {of})", c.name, c.to),
            (None, _) => format!("installed {} {}", c.name, c.to),
         };
         ctx.ui.success(what);
      }
      for (mod_id, dep, required) in &report.unmet {
         ctx.ui.warn(format!(
            "{mod_id} wants {dep} {required} or newer, but no such release fits {}",
            instance.game_version.as_deref().unwrap_or("this instance")
         ));
      }
   }
   for f in &report.failures {
      let context = f
         .needed_by
         .as_ref()
         .map(|of| format!(" (needed by {of})"))
         .unwrap_or_default();
      ctx.ui.error(format!("{}{context}: {}", f.mod_id, f.error));
   }
   if report.failures.is_empty() {
      Ok(())
   } else {
      fail(format!("{} mod(s) could not be installed", report.failures.len()))
   }
}

fn warn_without_game_version(ctx: &Ctx, instance: &Instance) {
   if instance.game_version.is_none() {
      ctx.ui.warn(format!(
         "{} has no game version, so mods are not checked for compatibility",
         instance.name
      ));
   }
}
