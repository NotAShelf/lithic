use crate::updater::github_api_args::GithubReleases;
use crate::updater::self_update::LithicUpdater;
use comfy_table::presets::UTF8_HORIZONTAL_ONLY;
use comfy_table::{Attribute, CellAlignment, Color};
use lithic_core::api::client::LITHIC_USER_AGENT;
use lithic_core::errors::LithicError;
use lithic_core::utils::{CellData, LithicMessage, command_output, display_table, lithic_message, notice};
use lithic_core::version::manager::parse_version;
use reqwest::Client;
use reqwest::header::ACCEPT;
use std::env;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, info};

// this url shows all releases for lithic published to github
const GITHUB_LITHIC_URI: &str = "https://api.github.com/repos/Tekunogosu/Lithic/releases";

pub struct GithubApi {
   agent: Arc<reqwest::Client>,
}

impl GithubApi {
   /// Create a GitHub API client.
   ///
   /// # Panics
   ///
   /// Panics only if constructing the static reqwest client configuration fails.
   #[must_use]
   pub fn new() -> Self {
      Self {
         agent: Arc::new(
            Client::builder()
               .timeout(Duration::from_secs(20))
               .user_agent(LITHIC_USER_AGENT)
               .build()
               .expect("Failed to build Github API client"),
         ),
      }
   }

   #[must_use]
   pub fn api_url(endpoint: &str) -> String {
      format!("{GITHUB_LITHIC_URI}/{endpoint}")
   }

   /// Fetch the latest Lithic GitHub release metadata.
   ///
   /// # Errors
   ///
   /// Returns an error if the request fails or the response cannot be parsed.
   pub async fn get_latest_release(&self) -> Result<GithubReleases, LithicError> {
      let uri = Self::api_url("latest");
      info!("URL: {}", &uri);
      let response = self
         .agent
         .get(uri)
         .header(ACCEPT, "application/vnd.github+json")
         .send()
         .await
         .and_then(reqwest::Response::error_for_status)
         .map_err(|e| LithicError::SimpleError(format!("get_latest_release: {e}")))?;

      let text = response
         .text()
         .await
         .map_err(|e| LithicError::SimpleError(format!("get_latest_release: txt {e}")))?;

      debug!("get_latest_release: txt: {text}");

      let parsed: GithubReleases = serde_json::from_str(&text)
         .map_err(|e| LithicError::SimpleError(format!("get_latest_release: (json) {e}")))?;

      Ok(parsed)
   }
}

impl Default for GithubApi {
   fn default() -> Self {
      Self::new()
   }
}

/// Check whether a newer Lithic release is available.
///
/// # Errors
///
/// Returns an error if release metadata cannot be fetched or either version
/// string cannot be parsed.
pub async fn check_for_update(hide_message: bool, hide_is_updated_msg: bool) -> Result<bool, LithicError> {
   let client = GithubApi::new();

   let latest_release = client.get_latest_release().await?;

   let latest_version = parse_version(latest_release.tag_name.as_str())?;
   let current_version = parse_version(env!("CARGO_PKG_VERSION"))?;

   let has_update = latest_version > current_version;

   if !hide_message {
      if has_update {
         lithic_message(LithicMessage {
            header: Some(CellData::new(
               "New Lithic Version Available!".into(),
               Some(Color::Green),
               vec![Attribute::Bold],
               Some(CellAlignment::Center),
            )),
            message: vec![
               CellData::new(
                  format!("Version: {latest_version} is now available!"),
                  Some(Color::Green),
                  vec![Attribute::Bold],
                  Some(CellAlignment::Center),
               ),
               CellData::new(
                  "You can update Lithic with the following command: ".into(),
                  Some(Color::Yellow),
                  vec![],
                  Some(CellAlignment::Center),
               ),
               CellData::new(
                  "./Lithic self --update".into(),
                  Some(Color::Magenta),
                  vec![Attribute::Bold],
                  Some(CellAlignment::Center),
               ),
               CellData::default(),
               CellData::new(
                  "You can disable this message with ./Lithic config set --disable-update-message".into(),
                  Some(Color::Cyan),
                  vec![Attribute::Italic, Attribute::Dim],
                  Some(CellAlignment::Center),
               ),
            ],
         });
      } else if !hide_is_updated_msg {
         display_table(
            vec![command_output(
               "Lithic is up-to-date!",
               format!("v{current_version}"),
            )],
            Some(UTF8_HORIZONTAL_ONLY),
         );
      }
   }

   info!("Current Version: {current_version}, latest version {latest_version}, has-update: {has_update}");

   Ok(has_update)
}

/// Download and install the latest Lithic binary for this platform.
///
/// # Errors
///
/// Returns an error if update metadata cannot be loaded, the platform is not
/// supported, the archive cannot be downloaded, or the binary swap fails.
pub async fn self_update_binary(force_update: bool) -> Result<(), LithicError> {
   // get latest release based in arch
   // download it to a temp dir
   // unzip the file
   // copy the current binary to tmp dir and put the new binary in its place, with the same name and permissions
   // if file swap failed, revert changes.. move old binary back in place, clean up tmp download
   // if success, print message about success

   let github_client = GithubApi::new();
   let latest_release = github_client.get_latest_release().await?;
   //
   let latest_version = parse_version(latest_release.tag_name.as_str())?;
   //
   // // if we want to force the update, set the version to 0.0.0 so its always out of date.
   // // it's a hack.. but im lazy :3
   let current_version = if force_update {
      info!("Forcing update");
      parse_version("0.0.0")?
   } else {
      parse_version(env!("CARGO_PKG_VERSION"))?
   };

   info!("Force update: {force_update}");

   if !force_update && !check_for_update(true, true).await? {
      info!("Lithic already up-to-date!");
      notice(
         format!("Already on latest version: {current_version}"),
         Some(Color::Green),
         vec![Attribute::Bold],
      );
      return Ok(());
   }

   info!("Update found, current version: {current_version}, new version: {latest_version}");

   let platform_bin_name = get_platform_bin_name()?;

   let archive_name = format!("{}.zip", &platform_bin_name);

   let Some(download_url) = latest_release
      .assets
      .iter()
      .find(|a| a.name == archive_name)
      .map(|a| &a.browser_download_url)
   else {
      return Err(LithicError::SimpleError("Failed to get download url".into()));
   };

   let new_binary_name: String = if cfg!(windows) {
      "lithic.exe".into()
   } else {
      "lithic".into()
   };

   info!("new_binary_name: {new_binary_name}");

   #[cfg(windows)]
   match LithicUpdater::new(&new_binary_name)
      .await?
      .download_archive(&archive_name, download_url, "Latest version downloaded...")
      .await?
      .create_update_script()
      .await?
      .execute_update_bat()
   {
      Ok(()) => {}
      Err(e) => {
         return Err(LithicError::SimpleError(format!(
            "Failed execute windows update script: {e}"
         )));
      }
   }

   #[cfg(unix)]
   LithicUpdater::new(&new_binary_name)
      .await?
      .download_archive(&archive_name, download_url, "Latest version downloaded...")
      .await?
      .update()
      .await?;

   Ok(())
}

/// Return the release asset basename for the host platform. Unknown
/// `(os, arch)` combinations produce a `LithicError` rather than aborting:
/// users on exotic platforms can still run the rest of the binary, they just
/// cannot self-update.
///
/// # Errors
///
/// Returns an error when the current OS and architecture do not map to a
/// published Lithic release asset.
pub fn get_platform_bin_name() -> Result<String, LithicError> {
   let os = std::env::consts::OS;
   let arch = std::env::consts::ARCH;

   match (os, arch) {
      ("linux", "x86_64") => Ok("lithic-linux-x86_64".into()),
      ("linux", "aarch64") => Ok("lithic-linux-aarch64".into()),
      ("macos", "x86_64") => Ok("lithic-macos-x86_64".into()),
      ("macos", "aarch64") => Ok("lithic-macos-aarch64".into()),
      ("windows", "x86_64") => Ok("lithic-windows-x86_64".into()),
      (os, arch) => Err(LithicError::SimpleError(format!(
         "Self-update not supported on {os}/{arch}. \
          Please open a GitHub issue with your platform and the desired target."
      ))),
   }
}
