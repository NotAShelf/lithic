use std::process::ExitCode;

fn main() -> ExitCode {
   let args: Vec<std::ffi::OsString> = std::env::args_os().collect();
   let bin_name = args
      .first()
      .and_then(|a| std::path::Path::new(a).file_stem())
      .map(|s| s.to_string_lossy().into_owned())
      .unwrap_or_default();
   // No arguments, the GUI binary name or --gui opens the window.
   let gui = args.len() == 1
      || bin_name.ends_with("gui")
      || args.iter().skip(1).any(|a| a == "--gui");
   if gui {
      return match lithic_gui::run() {
         Ok(()) => ExitCode::SUCCESS,
         Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
         }
      };
   }
   lithic_cli::run_with(args)
}
