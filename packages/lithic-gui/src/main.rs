// Without this, Windows opens a console window next to the GUI.
#![cfg_attr(windows, windows_subsystem = "windows")]

use std::process::ExitCode;

#[expect(
   clippy::print_stderr,
   reason = "startup failures must reach the invoking terminal"
)]
fn main() -> ExitCode {
   match lithic_gui::run() {
      Ok(()) => ExitCode::SUCCESS,
      Err(e) => {
         eprintln!("error: {e}");
         ExitCode::FAILURE
      }
   }
}
