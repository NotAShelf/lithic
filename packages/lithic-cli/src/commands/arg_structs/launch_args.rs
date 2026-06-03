use clap::Args;

#[derive(Args, Debug, Clone)]
pub struct LaunchArgs {
   /// ID of the instance to launch. Defaults to the active instance when omitted
   /// (see `lithic instance show` / `lithic instance select`).
   #[arg(long)]
   pub instance: Option<String>,
}
