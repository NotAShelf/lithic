mod args;
mod commands;
mod style;
mod ui;

use std::{
  env,
  ffi::OsString,
  io::{stderr, stdout},
  process::ExitCode,
};

use args::{Cli, Command, Global};
use clap::{CommandFactory, Parser};
use lithic_core::{Error, Instance, Lithic};
use tokio::runtime::Builder;
use tracing_subscriber::EnvFilter;
use ui::{Result, Ui};

/// Runs the CLI with the process arguments and returns its exit status.
#[must_use]
pub fn run() -> ExitCode {
  run_with(env::args_os())
}

/// Runs the CLI with the supplied arguments and returns its exit status.
#[must_use]
#[expect(
  clippy::print_stderr,
  reason = "runtime startup failure is a CLI error diagnostic"
)]
pub fn run_with<I, T>(args: I) -> ExitCode
where
  I: IntoIterator<Item = T>,
  T: Into<OsString> + Clone,
{
  let cli = match Cli::try_parse_from(args) {
    Ok(cli) => cli,
    Err(e) => {
      let _ = e.print();
      return if e.use_stderr() {
        ExitCode::from(2)
      } else {
        ExitCode::SUCCESS
      };
    },
  };
  init_logging(&cli.global);

  let runtime = match Builder::new_multi_thread().enable_all().build() {
    Ok(rt) => rt,
    Err(e) => {
      eprintln!("error: cannot start the async runtime: {e}");
      return ExitCode::FAILURE;
    },
  };
  runtime.block_on(dispatch(cli))
}

fn init_logging(global: &Global) {
  let level = match (global.quiet, global.verbose) {
    (true, _) => "error",
    (false, 0) => "warn",
    (false, 1) => "info",
    (false, _) => "debug",
  };
  let filter = EnvFilter::try_from_env("LITHIC_LOG").unwrap_or_else(|_| {
    EnvFilter::new(format!("warn,lithic_core={level},lithic_cli={level}"))
  });
  let _ = tracing_subscriber::fmt()
    .with_env_filter(filter)
    .with_writer(stderr)
    .with_target(false)
    .without_time()
    .try_init();
}

pub(crate) struct Ctx {
  pub lithic: Lithic,
  pub ui:     Ui,
  instance:   Option<String>,
}

impl Ctx {
  /// The instance named on the command line (positional first, then
  /// `--instance`), or the selected one.
  pub fn instance(&self, positional: Option<&str>) -> Result<Instance> {
    let id = positional.or(self.instance.as_deref());
    self.lithic.resolve_instance(id).map_err(|e| {
      match e {
        Error::Invalid(_) if id.is_none() => {
          ui::Failure(
            "no instance selected; create one with `lithic instance create`, \
             select one with `lithic instance select <id>`, or pass --instance"
              .to_string(),
          )
        },
        e => e.into(),
      }
    })
  }
}

async fn dispatch(cli: Cli) -> ExitCode {
  if let Command::Completions { shell } = cli.command {
    let mut cmd = Cli::command();
    clap_complete::generate(shell, &mut cmd, "lithic", &mut stdout());
    return ExitCode::SUCCESS;
  }

  let fallback_ui = Ui::new(&cli.global, toml::Table::new());
  let lithic = match Lithic::from_env() {
    Ok(l) => l,
    Err(e) => {
      fallback_ui.error(e.to_string());
      return ExitCode::FAILURE;
    },
  };

  match lithic.migrate() {
    Ok(Some(report)) => {
      fallback_ui
        .status("Moved your lithic 1.x configuration to the new layout:");
      for note in &report.notes {
        fallback_ui.status(format!("  {note}"));
      }
      fallback_ui.status(format!(
        "The old file is kept at {}",
        report.backup.display()
      ));
    },
    Ok(None) => {},
    Err(e) => {
      fallback_ui.error(format!(
        "could not migrate the lithic 1.x configuration: {e}"
      ));
      return ExitCode::FAILURE;
    },
  }

  let settings = match lithic.settings() {
    Ok(s) => s,
    Err(e) => {
      fallback_ui.error(e.to_string());
      return ExitCode::FAILURE;
    },
  };
  let ctx = Ctx {
    ui: Ui::new(&cli.global, settings.cli.table),
    lithic,
    instance: cli.global.instance,
  };

  match commands::run(&ctx, cli.command).await {
    Ok(()) => ExitCode::SUCCESS,
    Err(failure) => {
      if !failure.0.is_empty() {
        ctx.ui.error(&failure.0);
      }
      ExitCode::FAILURE
    },
  }
}
