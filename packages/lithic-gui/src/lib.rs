mod app;
mod i18n;
mod notify;
mod screen;
mod style;
mod task;
mod theme;
mod widget;

use std::{env, io};

use lithic_core::{Lithic, mods::ModRef};
use tracing_subscriber::EnvFilter;

/// The scheme the `ModDB`'s one-click install buttons use.
pub const LINK_SCHEME: &str = "vintagestorymodinstall://";

/// Opens the window, reading an optional mod-install link from the arguments.
///
/// # Errors
/// Returns an error if Lithic cannot initialize its directories or the window
/// fails to run.
pub fn run() -> Result<(), String> {
  let link = env::args().nth(1).filter(|a| a.starts_with(LINK_SCHEME));
  run_with_link(link)
}

/// Opens the window. A `vintagestorymodinstall://` link asks which instance
/// to install that mod into.
///
/// # Errors
/// Returns an error if Lithic cannot initialize its directories or the window
/// fails to run.
pub fn run_with_link(link: Option<String>) -> Result<(), String> {
  let _ = tracing_subscriber::fmt()
    .with_env_filter(
      EnvFilter::try_from_env("LITHIC_LOG")
        .unwrap_or_else(|_| EnvFilter::new("warn")),
    )
    .with_writer(io::stderr)
    .try_init();
  let link = link.and_then(|l| {
    match ModRef::parse(&l) {
      Ok(r) => Some(r),
      Err(e) => {
        tracing::warn!("ignoring {l}: {e}");
        None
      },
    }
  });
  let lithic = Lithic::from_env().map_err(|e| e.to_string())?;
  app::run(lithic, link).map_err(|e| e.to_string())
}
