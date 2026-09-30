use std::path::Path;

use lithic_core::{fsutil::file_name_string, launch};
use serde::Serialize;

use crate::{
  Ctx,
  args::{LaunchArgs, LogsArgs},
  ui::{Result, Ui, fail, format_duration},
};

#[expect(
  clippy::print_stdout,
  clippy::print_stderr,
  reason = "dry-run commands go to stdout and game crash diagnostics go to \
            stderr"
)]
pub async fn launch(ctx: &Ctx, args: LaunchArgs) -> Result {
  let instance = ctx.instance(args.id.as_deref())?;

  if args.dry_run {
    let spec = ctx.lithic.launch_spec(&instance)?;
    if ctx.ui.json {
      #[derive(Serialize)]
      struct Out<'a> {
        program: &'a Path,
        args:    &'a [String],
        cwd:     &'a Path,
        env:     &'a [(String, String)],
      }
      return Ui::print_json(&Out {
        program: &spec.program,
        args:    &spec.args,
        cwd:     &spec.cwd,
        env:     &spec.env,
      });
    }
    for (k, v) in &spec.env {
      println!("{k}={v}");
    }
    println!("cd {}", spec.cwd.display());
    println!("{}", spec.command_line());
    return Ok(());
  }

  let (session, waiter) = ctx.lithic.launch(&instance).await?;
  let pid = session
    .pid
    .map(|p| format!(" (pid {p})"))
    .unwrap_or_default();
  if args.detach {
    ctx.ui.success(format!(
      "started {}{pid}; output goes to {}",
      instance.name,
      session.log_path.display()
    ));
    return Ok(());
  }
  ctx.ui.status(format!(
    "Started {}{pid}. Waiting for the game to close.",
    instance.name
  ));

  let exit = waiter.await?;
  if exit.success || exit.stopped {
    ctx.ui.success(format!(
      "game closed after {}",
      format_duration(exit.duration_ms)
    ));
    return Ok(());
  }
  let code = exit
    .code
    .map_or_else(|| "a signal".to_string(), |c| format!("code {c}"));
  ctx.ui.error(format!(
    "the game exited with {code} after {}",
    format_duration(exit.duration_ms)
  ));
  for line in &exit.tail {
    eprintln!("  {line}");
  }
  if let Some(crash) = &exit.crash_report {
    eprintln!("Crash report: {}", crash.display());
  }
  eprintln!("Full output: {}", exit.log_path.display());
  fail("")
}

#[expect(
  clippy::print_stdout,
  reason = "log paths and contents are CLI output"
)]
pub fn logs(ctx: &Ctx, args: &LogsArgs) -> Result {
  let instance = ctx.instance(args.id.as_deref())?;
  let files = ctx.lithic.log_files(&instance);

  if args.list {
    if ctx.ui.json {
      return Ui::print_json(&files);
    }
    for f in &files {
      println!("{}", f.display());
    }
    return Ok(());
  }

  let file = if args.game {
    Some(instance.game_logs_dir().join("client-main.log"))
      .filter(|p| p.is_file())
  } else {
    files
      .iter()
      .find(|f| file_name_string(f).starts_with("launch-"))
      .cloned()
  };
  let Some(file) = file else {
    return fail(format!("{} has no logs yet", instance.name));
  };
  let text = launch::read_tail(&file, 256 * 1024)?;
  let start = text.lines().count().saturating_sub(args.lines);
  ctx.ui.status(format!("==> {} <==", file.display()));
  for line in text.lines().skip(start) {
    println!("{line}");
  }
  Ok(())
}
