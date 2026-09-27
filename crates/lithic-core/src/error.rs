use std::{
  fmt,
  io,
  path::{Path, PathBuf},
  result,
};

use crate::auth::AuthError;

pub type Result<T, E = Error> = result::Result<T, E>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
  Instance,
  GameVersion,
  Mod,
  Release,
  Account,
}

impl fmt::Display for Kind {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.write_str(match self {
      Self::Instance => "instance",
      Self::GameVersion => "game version",
      Self::Mod => "mod",
      Self::Release => "release",
      Self::Account => "account",
    })
  }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
  #[error("{}: {source}", path.display())]
  Io {
    path:   PathBuf,
    #[source]
    source: io::Error,
  },

  #[error("request to {url} failed: {source}")]
  Http {
    url:    String,
    #[source]
    source: reqwest::Error,
  },

  #[error("{endpoint} answered with status {status}")]
  Api { endpoint: String, status: String },

  #[error("could not parse {what}: {message}")]
  Parse { what: String, message: String },

  #[error("{} could not be read and was left untouched: {message}", path.display())]
  Corrupt { path: PathBuf, message: String },

  #[error("{kind} not found: {id}")]
  NotFound { kind: Kind, id: String },

  #[error("{kind} already exists: {id}")]
  AlreadyExists { kind: Kind, id: String },

  #[error("{what} is still used by {}", users.join(", "))]
  InUse { what: String, users: Vec<String> },

  #[error("no release of {mod_id} works with game version {game_version}")]
  NoCompatibleRelease {
    mod_id:       String,
    game_version: String,
  },

  #[error("checksum mismatch for {file}: expected {expected}, got {actual}")]
  Checksum {
    file:     String,
    expected: String,
    actual:   String,
  },

  #[error("{0}")]
  Invalid(String),

  #[error("{0}")]
  Unsupported(String),

  #[error("the operation was cancelled")]
  Cancelled,

  #[error(
    "{0} is being changed by another lithic operation, try again when it \
     finishes"
  )]
  Busy(String),

  #[error("instance {0} is running")]
  Running(String),

  #[error(transparent)]
  Auth(#[from] AuthError),
}

impl Error {
  pub fn io(path: impl Into<PathBuf>, source: io::Error) -> Self {
    Self::Io {
      path: path.into(),
      source,
    }
  }

  pub fn http(url: impl Into<String>, source: reqwest::Error) -> Self {
    Self::Http {
      url: url.into(),
      source,
    }
  }

  pub fn parse(what: impl Into<String>, message: impl fmt::Display) -> Self {
    Self::Parse {
      what:    what.into(),
      message: message.to_string(),
    }
  }

  pub fn not_found(kind: Kind, id: impl Into<String>) -> Self {
    Self::NotFound {
      kind,
      id: id.into(),
    }
  }

  pub fn invalid(message: impl Into<String>) -> Self {
    Self::Invalid(message.into())
  }

  /// True for failures that retrying later could fix: timeouts, dropped
  /// connections, 5xx answers.
  #[must_use]
  pub fn is_transient(&self) -> bool {
    match self {
      Self::Http { source, .. } => {
        source.is_timeout()
          || source.is_connect()
          || source.is_request()
          || source.status().is_some_and(|s| s.is_server_error())
      },
      _ => false,
    }
  }
}

/// Attaches the offending path to `std::io::Error`s.
pub trait IoContext<T> {
  /// # Errors
  /// Returns [`Error::Io`] with the supplied path when the I/O operation fails.
  fn at(self, path: impl AsRef<Path>) -> Result<T>;
}

impl<T> IoContext<T> for io::Result<T> {
  fn at(self, path: impl AsRef<Path>) -> Result<T> {
    self.map_err(|e| Error::io(path.as_ref(), e))
  }
}
