use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// `mods.json` in an instance directory: what lithic knows about each mod
/// beyond what the files themselves say.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ModLock {
  /// Keyed by lowercased mod id.
  pub mods: BTreeMap<String, LockEntry>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LockEntry {
  /// File name lithic installed.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub file:         Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub version:      Option<String>,
  /// Numeric `ModDB` id.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub moddb_id:     Option<i64>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub release_id:   Option<i64>,
  /// Version to stay on; updates skip this mod.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub pin:          Option<String>,
  /// Installed only because another mod needs it.
  #[serde(skip_serializing_if = "std::ops::Not::not")]
  pub dependency:   bool,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub installed_at: Option<i64>,
}

impl LockEntry {
  /// An entry that carries nothing but a pin can outlive the mod itself, so
  /// a pin set before installing still applies.
  #[must_use]
  pub const fn is_empty(&self) -> bool {
    self.pin.is_none() && self.file.is_none() && self.moddb_id.is_none()
  }
}
