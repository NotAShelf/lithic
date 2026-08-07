// Without this, Windows opens a console window next to the GUI.
#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() -> std::process::ExitCode {
   match lithic_gui::run() {
      Ok(()) => std::process::ExitCode::SUCCESS,
      Err(e) => {
         eprintln!("error: {e}");
         std::process::ExitCode::FAILURE
      }
   }
}
