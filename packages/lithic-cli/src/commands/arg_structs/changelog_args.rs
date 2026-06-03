use clap::Args;

#[derive(Args, Debug, Clone)]
pub struct ChangeLogArgs {
   /// Mod id (textual or numeric) whose changelog should be displayed.
   /// Example: `lithic changelog rudiments`.
   pub mod_id: String,

   /// Limit to the N most recent releases. Pass 0 to show every release.
   #[arg(short = 'v', long, default_value_t = 3)]
   pub show_versions: usize,
}
