use crate::consts::FILE_MODINFO_JSON;
use crate::utils::notice;
use comfy_table::{Attribute, Color};
use thiserror::Error;
use tracing::{debug, error, info, warn};

#[allow(dead_code)]
#[derive(Debug, Error)]
pub enum LithicError {
   #[error("Api Error: {context}: {source}")]
   ApiError {
      context: String,
      #[source]
      source: reqwest::Error,
   },
   #[error("Download Error: {0}")]
   DownloadError(String),
   #[error("{context}: {source}")]
   IoError {
      context: String,
      #[source]
      source: std::io::Error,
   },
   #[error("Parse Error: {0}")]
   UrlParseError(url::ParseError),
   #[error("Version Parse Error: {context}: {source}")]
   VersionError {
      context: String,
      #[source]
      source: semver::Error,
   },
   #[error("No Version Found: {0}")]
   NoVersionFound(String),
   #[error("JsonParseError: {context}: {source}")]
   JsonError {
      context: String,
      #[source]
      source: serde_json5::Error,
   },
   #[error("{0}")]
   SimpleError(String),
   #[error("Expected .zip, found folder. Did you forget to zip your mod? {0}")]
   ModNotZipped(String),
   #[error("ZipError: {context}: {source}")]
   ZipError {
      context: String,
      #[source]
      source: async_zip::error::ZipError,
   },
   #[error("Config File Error: {0}")]
   ConfigFileError(String),
   #[error(
      "Malformed {FILE_MODINFO_JSON} discovered for {0}: Please contact the mod author. Lithic cannot process this mod."
   )]
   MalformedModInfoJson(String),
   #[error("{context}: {source}")]
   TomlError {
      context: String,
      #[source]
      source: toml::de::Error,
   },
   /// The VS API returned HTTP 200 but the JSON payload contains a non-`"200"`
   /// `statuscode` field. The API uses this convention to signal logical errors
   /// (e.g. `"410"` for deprecated endpoints) while still returning 200.
   #[error("API returned status {status_code} for {endpoint}{}", reason.as_deref().map_or(String::new(), |r| format!(": {r}")))]
   ApiStatusError {
      endpoint: String,
      status_code: String,
      reason: Option<String>,
   },
}

impl From<std::io::Error> for LithicError {
   fn from(e: std::io::Error) -> Self {
      LithicError::IoError {
         source: e,
         context: String::new(),
      }
   }
}

impl From<url::ParseError> for LithicError {
   fn from(e: url::ParseError) -> Self {
      LithicError::UrlParseError(e)
   }
}

/// Helper function for Results that discards Ok() value if not needed.
pub fn handle_err_result<T>(
   result: Result<T, LithicError>,
   context: &str,
   nice_error: bool,
   msg_fn: ErrorMsgFn,
) {
   if let Err(e) = result {
      let color = match msg_fn {
         ErrorMsgFn::Debug => {
            if !nice_error {
               debug!("{context}: {e}")
            }
            Color::DarkYellow
         }
         ErrorMsgFn::Error => {
            if !nice_error {
               error!("{context}: {e}")
            }
            Color::Red
         }
         ErrorMsgFn::Info => {
            if !nice_error {
               info!("{context}: {e}")
            }
            Color::Blue
         }
         ErrorMsgFn::Warn => {
            if !nice_error {
               warn!("{context}: {e}")
            }
            Color::Yellow
         }
      };

      if nice_error {
         notice(format!("{context}: {e}"), Some(color), vec![Attribute::Bold]);
      }
   }
}

#[allow(dead_code)]
pub enum ErrorMsgFn {
   Info,
   Debug,
   Warn,
   Error,
}
