mod app;
mod i18n;
mod notify;
mod screen;
mod style;
mod task;
mod widget;

use tracing_subscriber::EnvFilter;

pub fn run() -> Result<(), String> {
   let _ = tracing_subscriber::fmt()
      .with_env_filter(EnvFilter::try_from_env("LITHIC_LOG").unwrap_or_else(|_| EnvFilter::new("warn")))
      .with_writer(std::io::stderr)
      .try_init();
   let lithic = lithic_core::Lithic::from_env().map_err(|e| e.to_string())?;
   app::run(lithic).map_err(|e| e.to_string())
}

