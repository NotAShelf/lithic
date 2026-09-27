use std::{env, ffi::OsString, path::Path, process::ExitCode};

#[expect(
  clippy::print_stderr,
  reason = "startup failures must reach the invoking terminal"
)]
fn main() -> ExitCode {
  let args: Vec<OsString> = env::args_os().collect();
  let bin_name = args
    .first()
    .and_then(|a| Path::new(a).file_stem())
    .map(|s| s.to_string_lossy().into_owned())
    .unwrap_or_default();
  let link = args
    .get(1)
    .map(|a| a.to_string_lossy().into_owned())
    .filter(|a| a.starts_with(lithic_gui::LINK_SCHEME));

  // Started without arguments (as from a desktop entry), as `lithic-gui`,
  // with --gui, or by the browser with a one-click install link: open the
  // window. Anything else is a CLI invocation.
  let gui = args.len() == 1
    || link.is_some()
    || bin_name.ends_with("gui")
    || args.iter().skip(1).any(|a| a == "--gui");
  if gui {
    return match lithic_gui::run_with_link(link) {
      Ok(()) => ExitCode::SUCCESS,
      Err(e) => {
        eprintln!("error: {e}");
        ExitCode::FAILURE
      },
    };
  }
  lithic_cli::run_with(args)
}
