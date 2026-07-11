use crate::commands::arg_structs::launch_args::LaunchArgs;

/// Launch a configured Vintage Story instance.
///
/// # Errors
///
/// Returns an error if no matching instance or game install can be resolved, or
/// if starting the process fails.
pub async fn launch(args: &LaunchArgs) -> Result<(), String> {
   lithic_core::instance::launch_instance(args.instance.clone()).await
}
