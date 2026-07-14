use crate::aliases::{FileName, ModID, UrlString};
use crate::api::structs::{GameVersions, Mod, Mods};
use crate::consts::FILE_MODINFO_JSON;
use crate::errors::LithicError;
use clap::ValueEnum;
use futures::{StreamExt, stream};
use indicatif::{ProgressBar, ProgressStyle};
use reqwest::Response;
use std::collections::{HashMap, HashSet};
use std::fmt::Display;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, error, info};
use yansi::Paint;

const API_BASE_URL: &str = "https://mods.vintagestory.at/api";
const VS_CDN_STABLE_RELEASE: &str = "https://cdn.vintagestory.at/gamefiles/stable";
const VS_CDN_UNSTABLE_RELEASE: &str = "https://cdn.vintagestory.at/gamefiles/unstable";
pub const LITHIC_USER_AGENT: &str = concat!(
   env!("CARGO_PKG_NAME"),
   "/",
   env!("CARGO_PKG_VERSION"),
   " (+",
   env!("CARGO_PKG_REPOSITORY"),
   ")"
);

/// Cap on concurrent per-mod fetches when populating the mod database. The
/// Vintage Story API tolerates fan-out but starts rate-limiting beyond this;
/// `fetch_mods_parallel` respects this bound.
pub const MAX_CONCURRENT_MOD_FETCHES: usize = 5;

#[derive(Debug, Clone)]
pub struct ApiClient {
   agent: Arc<reqwest::Client>,
}

impl Default for ApiClient {
   fn default() -> Self {
      Self::new()
   }
}

#[derive(Debug, Clone, ValueEnum)]
pub enum VSMirrorType {
   Stable,
   Unstable,
}

#[derive(Debug, Clone, ValueEnum)]
pub enum VSExecutabletype {
   Server,
   Client,
}

#[allow(clippy::upper_case_acronyms)]
#[derive(Clone, ValueEnum, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VSOSType {
   Linux,
   #[serde(alias = "macos")]
   OSX,
   Windows,
}

impl Display for VSOSType {
   fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
      match self {
         VSOSType::Linux => write!(f, "linux"),
         VSOSType::OSX => write!(f, "osx"),
         VSOSType::Windows => write!(f, "windows"),
      }
   }
}

impl VSOSType {
   /// Best-effort detection of the host OS. Returns `None` on platforms we
   /// don't support (e.g. BSDs); callers decide whether that's fatal.
   pub fn host() -> Option<Self> {
      match std::env::consts::OS {
         "linux" => Some(Self::Linux),
         "macos" => Some(Self::OSX),
         "windows" => Some(Self::Windows),
         _ => None,
      }
   }
}

#[derive(Debug, Clone, ValueEnum)]
pub enum VSWinInstallerType {
   Install,
   Update,
}

impl ApiClient {
   /// Construct a client with Lithic's default `reqwest` configuration
   /// (20s request timeout, Lithic user-agent).
   pub fn new() -> Self {
      Self {
         // `reqwest::Client::builder().build()` only fails if TLS backend
         // initialisation fails, which is a process-wide configuration error
         // — treat it as a programmer-visible invariant rather than a
         // recoverable runtime case.
         agent: Arc::new(
            reqwest::Client::builder()
               .timeout(Duration::from_secs(20))
               .user_agent(LITHIC_USER_AGENT)
               .build()
               .expect("reqwest::Client builder invariant: default TLS backend available"),
         ),
      }
   }

   /// Inject a pre-built `reqwest::Client`. Intended for tests / integration
   /// scenarios that want to plug in a custom transport (mock server, fixed
   /// proxy, alternate timeout, ...).
   pub fn with_agent(agent: Arc<reqwest::Client>) -> Self {
      Self { agent }
   }

   pub fn api_uri(endpoint: &str) -> String {
      format!("{API_BASE_URL}/{endpoint}")
   }

   /// Validate the API's own `statuscode` envelope. The VS API returns HTTP 200
   /// even for logical errors (e.g. `"410"` for deprecated endpoints) and the
   /// real outcome is conveyed by the `statuscode` field in the JSON body.
   ///
   /// Accepts `""` as a non-error to remain compatible with payloads that omit
   /// the field entirely (`serde(default)` produces an empty string).
   fn check_api_status(status_code: &str, endpoint: &str, reason: Option<&str>) -> Result<(), LithicError> {
      if status_code.is_empty() || status_code == "200" {
         return Ok(());
      }
      Err(LithicError::ApiStatusError {
         endpoint: endpoint.to_string(),
         status_code: status_code.to_string(),
         reason: reason.map(str::to_string),
      })
   }

   pub async fn fetch_all_mods(&self) -> Result<Mods, LithicError> {
      let response = self
         .agent
         .get(Self::api_uri("mods"))
         .send()
         .await
         .and_then(Response::error_for_status)
         .map_err(|e| LithicError::ApiError {
            context: "fetch_all_mods (get): ".to_string(),
            source: e,
         })?;

      let mods = response.json::<Mods>().await.map_err(|e| LithicError::ApiError {
         context: "fetch_all_mods (json): ".to_string(),
         source: e,
      })?;
      Self::check_api_status(&mods.status_code, "mods", None)?;
      Ok(mods)
   }

   /// Fetches mods compatible with the given MAJOR.MINOR game version (e.g. "1.20").
   pub async fn fetch_mods_with_gameversion(&self, version: &str) -> Result<Mods, LithicError> {
      let response = self
         .agent
         .get(Self::api_uri(&format!("mods?gameversion={version}")))
         .send()
         .await
         .and_then(Response::error_for_status)
         .map_err(|e| LithicError::ApiError {
            context: format!("fetch_mods_with_gameversion ({version}): "),
            source: e,
         })?;

      let mods = response.json::<Mods>().await.map_err(|e| LithicError::ApiError {
         context: format!("fetch_mods_with_gameversion (json) ({version}): "),
         source: e,
      })?;
      Self::check_api_status(&mods.status_code, &format!("mods?gameversion={version}"), None)?;
      Ok(mods)
   }

   pub async fn fetch_mod(&self, mod_id: impl AsRef<str>) -> Result<Mod, LithicError> {
      let mod_id = mod_id.as_ref();
      if mod_id.is_empty() {
         error!("Mod id is empty {}", mod_id);
         return Err(LithicError::MalformedModInfoJson(
            "The mod id received was empty.. unable to download whatever mod this is.".to_string(),
         ));
      }

      info!("{} {}", "Fetching mod: ".bright_green(), mod_id.bright_yellow());

      let response = self
         .agent
         .get(Self::api_uri(&format!("mod/{mod_id}")))
         .send()
         .await
         .and_then(Response::error_for_status)
         .map_err(|e| LithicError::ApiError {
            context: format!("fetch_mod (get) [{mod_id}]",),
            source: e,
         })?;

      let headers = response.headers().clone();
      let status_code = response.status();

      info!(
         "fetch_mod ({}): Status Code: {}",
         mod_id.magenta(),
         status_code.magenta()
      );
      info!(
         "fetch_mod ({}): Headers: {:?}",
         mod_id.magenta(),
         headers.bright_blue()
      );

      let text = response
         .text()
         .await
         .map_err(|e| LithicError::SimpleError(e.to_string()))?;

      let parsed: Mod = serde_json::from_str(&text).map_err(|e| LithicError::SimpleError(e.to_string()))?;
      debug!("Parsed {:?}", parsed);
      Self::check_api_status(&parsed.status_code, &format!("mod/{mod_id}"), None)?;

      Ok(parsed)
   }

   pub async fn fetch_mods_parallel(&self, mod_list: Vec<ModID>) -> Result<HashMap<ModID, Mod>, LithicError> {
      let valid_ids: Vec<ModID> = mod_list.into_iter().filter(|m| {
            if m.is_empty() {
               error!("\n\r\tModID is empty or missing mod_id. Please contact the author to correct their malformed {FILE_MODINFO_JSON}.\n\r\tWithout the mod id, Lithic will be unable to manage this mod.");
               false
            } else {
                true
            }
        }).collect();

      if valid_ids.is_empty() {
         return Ok(HashMap::new());
      }

      // progress bar for completed calls
      let pb = ProgressBar::new(valid_ids.len() as u64);
      pb.set_style(
            ProgressStyle::default_bar()
                .template("{spinner:.green} [{elapsed_precise:.cyan}] [{bar:.cyan/grey:40}] {pos:.green}/{len:.cyan} {msg:.yellow}")
                .expect("static indicatif template invariant: literal is well-formed")
                .progress_chars("█▒░")
        );
      pb.set_message("Fetching mods...");

      let results = stream::iter(valid_ids)
         .map(|mod_id| {
            let client = self.clone();
            let pb = pb.clone();
            async move {
               info!("ModID: {mod_id}");
               let result = client
                  .fetch_mod(&mod_id)
                  .await
                  .map(|the_mod| (mod_id.clone(), the_mod));
               match result {
                  Ok((mod_id, mod_info)) => {
                     pb.set_message(mod_id.to_string());
                     pb.inc(1);
                     Some((mod_id, mod_info))
                  }
                  Err(e) => {
                     error!("{mod_id} {e}");
                     pb.set_message(format!("Failed: {}", mod_id.to_string().red()));
                     pb.inc(1);
                     None
                  }
               }
            }
         })
         .buffer_unordered(MAX_CONCURRENT_MOD_FETCHES)
         .filter_map(futures::future::ready)
         .collect::<HashMap<_, _>>()
         .await;

      pb.finish_with_message("Fetch Complete");
      Ok(results)
   }

   pub async fn fetch_game_versions(&self) -> Result<HashSet<String>, LithicError> {
      let response = self
         .agent
         .get(Self::api_uri("gameversions"))
         .send()
         .await
         .and_then(Response::error_for_status)
         .map_err(|e| LithicError::ApiError {
            context: "Failed during gameversions api call".to_string(),
            source: e,
         })?;

      let text = response.text().await.map_err(|e| LithicError::ApiError {
         context: "Failed parsing game versions api data".to_string(),
         source: e,
      })?;

      let versions: GameVersions =
         serde_json::from_str(&text).map_err(|e| LithicError::SimpleError(e.to_string()))?;
      Self::check_api_status(&versions.status_code, "gameversions", None)?;

      let hash: HashSet<String> = versions
         .game_versions
         .iter()
         .map(|gv| &gv.name)
         .cloned()
         .collect();

      Ok(hash)
   }

   pub async fn get_request(&self, mod_uri: &str) -> Result<Response, LithicError> {
      self
         .agent
         .get(mod_uri)
         .send()
         .await
         .and_then(Response::error_for_status)
         .map_err(|e| LithicError::ApiError {
            context: format!("get_request: {mod_uri}"),
            source: e,
         })
   }

   /// Build the CDN URL and target filename for a Vintage Story release.
   ///
   /// The official CDN names files according to the OS, executable kind, and
   /// (on Windows) installer kind: e.g. `vs_client_linux-x64_1.20.0.tar.gz`,
   /// `vs_install_win-x64_1.20.0.exe`, `vs_server_win-x64_1.20.0.zip`.
   pub fn download_uri(
      &self,
      os_type: &VSOSType,
      exe_type: &VSExecutabletype,
      vsmirror_type: &VSMirrorType,
      game_version: &str,
      win_installer: Option<&VSWinInstallerType>,
   ) -> Result<(UrlString, FileName), LithicError> {
      let filename = build_release_filename(os_type, exe_type, win_installer, game_version);
      let cdn_base = match vsmirror_type {
         VSMirrorType::Stable => VS_CDN_STABLE_RELEASE,
         VSMirrorType::Unstable => VS_CDN_UNSTABLE_RELEASE,
      };
      let url = format!("{cdn_base}/{filename}");
      Ok((url.into(), filename.into()))
   }

   pub async fn head(&self, uri: &str) -> Result<Response, LithicError> {
      self
         .agent
         .head(uri)
         .send()
         .await
         .and_then(Response::error_for_status)
         .map_err(|e| LithicError::ApiError {
            context: format!("Failed calling agent.head({uri})"),
            source: e,
         })
   }
}

/// Construct the CDN basename for a Vintage Story release artifact.
fn build_release_filename(
   os: &VSOSType,
   exe: &VSExecutabletype,
   win_installer: Option<&VSWinInstallerType>,
   game_version: &str,
) -> String {
   let prefix = match (os, exe) {
      (VSOSType::Windows, VSExecutabletype::Server) => "vs_server_win".to_string(),
      (VSOSType::Windows, VSExecutabletype::Client) => {
         let installer = match win_installer.unwrap_or(&VSWinInstallerType::Install) {
            VSWinInstallerType::Install => "install",
            VSWinInstallerType::Update => "update",
         };
         format!("vs_{installer}_win")
      }
      (os, exe) => {
         let etype = match exe {
            VSExecutabletype::Client => "client",
            VSExecutabletype::Server => "server",
         };
         format!("vs_{etype}_{os}")
      }
   };
   let extension = match (os, exe) {
      (VSOSType::Linux | VSOSType::OSX, _) => "tar.gz",
      (VSOSType::Windows, VSExecutabletype::Server) => "zip",
      (VSOSType::Windows, VSExecutabletype::Client) => "exe",
   };
   format!("{prefix}-x64_{game_version}.{extension}")
}

#[cfg(test)]
mod tests {
   use super::*;

   #[test]
   fn linux_client_filename() {
      let name = build_release_filename(&VSOSType::Linux, &VSExecutabletype::Client, None, "1.20.0");
      assert_eq!(name, "vs_client_linux-x64_1.20.0.tar.gz");
   }

   #[test]
   fn windows_client_installer() {
      let name = build_release_filename(
         &VSOSType::Windows,
         &VSExecutabletype::Client,
         Some(&VSWinInstallerType::Install),
         "1.20.0",
      );
      assert_eq!(name, "vs_install_win-x64_1.20.0.exe");
   }

   #[test]
   fn windows_client_update() {
      let name = build_release_filename(
         &VSOSType::Windows,
         &VSExecutabletype::Client,
         Some(&VSWinInstallerType::Update),
         "1.20.0",
      );
      assert_eq!(name, "vs_update_win-x64_1.20.0.exe");
   }

   #[test]
   fn windows_server_archive() {
      let name = build_release_filename(&VSOSType::Windows, &VSExecutabletype::Server, None, "1.20.0");
      assert_eq!(name, "vs_server_win-x64_1.20.0.zip");
   }

   #[test]
   fn osx_client_tarball() {
      let name = build_release_filename(&VSOSType::OSX, &VSExecutabletype::Client, None, "1.20.0");
      assert_eq!(name, "vs_client_osx-x64_1.20.0.tar.gz");
   }
}
