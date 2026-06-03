//! `lithic-locale` – Fluent-based localisation for the Lithic toolchain.
//!
//! # Usage
//!
//! ```rust
//! use lithic_locale::{Localizer, locale};
//!
//! let msg = locale().get("nav-browse");
//! assert_eq!(msg, "Browse");
//! ```

use std::borrow::Cow;
use std::sync::OnceLock;

use fluent::FluentArgs;
use fluent::FluentResource;
use fluent::FluentValue;
use fluent::concurrent::FluentBundle;
use unic_langid::LanguageIdentifier;

// ---------------------------------------------------------------------------
// Global localizer instance
// ---------------------------------------------------------------------------

static GLOBAL_LOCALIZER: OnceLock<Localizer> = OnceLock::new();

/// Returns a reference to the process-wide [`Localizer`].
///
/// The first call initialises the localizer with the built-in English locale.
/// Call [`set_locale`] before the first [`locale`] call to override the locale.
pub fn locale() -> &'static Localizer {
   GLOBAL_LOCALIZER.get_or_init(Localizer::new_english)
}

/// Replace the global localizer.
///
/// Must be called **before** any call to [`locale`]. Returns `false` if the
/// global was already initialised (the replacement is silently ignored).
pub fn set_locale(localizer: Localizer) -> bool {
   GLOBAL_LOCALIZER.set(localizer).is_ok()
}

/// A compiled bundle of Fluent messages for a single locale.
pub struct Localizer {
   bundle: FluentBundle<FluentResource>,
}

impl Localizer {
   /// Build a `Localizer` from raw FTL source strings and a BCP-47 language tag.
   ///
   /// Returns an error string when FTL parsing fails.
   pub fn from_ftl_sources(lang: &str, sources: &[&str]) -> Result<Self, String> {
      let lang_id: LanguageIdentifier = lang.parse().map_err(|e| format!("invalid lang: {e}"))?;
      let mut bundle = FluentBundle::new_concurrent(vec![lang_id]);

      for src in sources {
         let resource = FluentResource::try_new(src.to_string())
            .map_err(|(_, errs)| format!("FTL parse errors: {errs:?}"))?;
         bundle
            .add_resource(resource)
            .map_err(|errs| format!("duplicate FTL entries: {errs:?}"))?;
      }

      Ok(Self { bundle })
   }

   /// Convenience constructor that bundles all built-in English FTL files.
   pub fn new_english() -> Self {
      let sources: &[&str] = &[
         include_str!("../locales/en/help.ftl"),
         include_str!("../locales/en/errors.ftl"),
         include_str!("../locales/en/gui.ftl"),
      ];
      Self::from_ftl_sources("en-US", sources).expect("built-in English FTL must be valid")
   }

   /// Look up a Fluent message by its ID without arguments.
   ///
   /// Returns the translated string, or the message ID itself as a fallback.
   pub fn get(&self, id: &str) -> Cow<'static, str> {
      self.get_with_args(id, None)
   }

   /// Look up a Fluent message with optional [`FluentArgs`].
   pub fn get_with_args<'a>(&'a self, id: &str, args: Option<&'a FluentArgs<'_>>) -> Cow<'static, str> {
      let Some(msg) = self.bundle.get_message(id) else {
         return Cow::Owned(id.to_string());
      };
      let Some(pattern) = msg.value() else {
         return Cow::Owned(id.to_string());
      };

      let mut errors = vec![];
      let result = self.bundle.format_pattern(pattern, args, &mut errors);
      Cow::Owned(result.into_owned())
   }

   /// Convenience: format a message with a single named argument.
   pub fn get_with<'a>(
      &'a self,
      id: &str,
      key: &'a str,
      value: impl Into<FluentValue<'a>>,
   ) -> Cow<'static, str> {
      let mut args = FluentArgs::new();
      args.set(key, value);
      self.get_with_args(id, Some(&args))
   }

   /// Convenience: format a message with two named arguments.
   pub fn get_with2<'a>(
      &'a self,
      id: &str,
      key1: &'a str,
      value1: impl Into<FluentValue<'a>>,
      key2: &'a str,
      value2: impl Into<FluentValue<'a>>,
   ) -> Cow<'static, str> {
      let mut args = FluentArgs::new();
      args.set(key1, value1);
      args.set(key2, value2);
      self.get_with_args(id, Some(&args))
   }

   /// Convenience: format a message with three named arguments.
   pub fn get_with3<'a>(
      &'a self,
      id: &str,
      values: [(&'a str, FluentValue<'a>); 3],
   ) -> Cow<'static, str> {
      let mut args = FluentArgs::new();
      for (key, value) in values {
         args.set(key, value);
      }
      self.get_with_args(id, Some(&args))
   }
}

/// Module containing all known message IDs as constants, to avoid stringly-typed usage.
pub mod ids {
   // Navigation
   pub const NAV_BROWSE: &str = "nav-browse";
   pub const NAV_INSTALLED: &str = "nav-installed";
   pub const NAV_INSTANCES: &str = "nav-instances";
   pub const NAV_GAME_VERSIONS: &str = "nav-game-versions";
   pub const NAV_SETTINGS: &str = "nav-settings";

   // App
   pub const APP_TITLE: &str = "app-title";

   // Browse
   pub const BROWSE_SEARCH_PLACEHOLDER: &str = "browse-search-placeholder";
   pub const BROWSE_REFRESH: &str = "browse-refresh";
   pub const BROWSE_SORT_DOWNLOADS: &str = "browse-sort-downloads";
   pub const BROWSE_SORT_FOLLOWS: &str = "browse-sort-follows";
   pub const BROWSE_SORT_TRENDING: &str = "browse-sort-trending";
   pub const BROWSE_SORT_NAME: &str = "browse-sort-name";
   pub const BROWSE_INSTALL: &str = "browse-install";
   pub const BROWSE_ADD_TO_INSTANCE: &str = "browse-add-to-instance";
   pub const BROWSE_INSTALLED_BADGE: &str = "browse-installed-badge";
   pub const BROWSE_LOADING: &str = "browse-loading";
   pub const BROWSE_NO_RESULTS: &str = "browse-no-results";
   pub const BROWSE_FAVORITES_ONLY: &str = "browse-favorites-only";
   pub const BROWSE_EXPORT_FAVORITES: &str = "browse-export-favorites";
   pub const BROWSE_PAGE: &str = "browse-page";

   // Installed
   pub const INSTALLED_SYNC: &str = "installed-sync";
   pub const INSTALLED_UPDATE_ALL: &str = "installed-update-all";
   pub const INSTALLED_DELETE: &str = "installed-delete";
   pub const INSTALLED_CONFIRM_DELETE: &str = "installed-confirm-delete";
   pub const INSTALLED_CANCEL: &str = "installed-cancel";
   pub const INSTALLED_NO_MODS: &str = "installed-no-mods";
   pub const INSTALLED_SEARCH: &str = "installed-search";
   pub const INSTALLED_LOADING: &str = "installed-loading";

   // Settings
   pub const SETTINGS_SAVE: &str = "settings-save";
   pub const SETTINGS_MOD_DIR: &str = "settings-mod-dir";
   pub const SETTINGS_GAME_DIR: &str = "settings-game-dir";
   pub const SETTINGS_GAME_VERSION: &str = "settings-game-version";
   pub const SETTINGS_SAVED: &str = "settings-saved";

   // Instances
   pub const INSTANCES_NEW: &str = "instances-new";
   pub const INSTANCES_SAVE: &str = "instances-save";
   pub const INSTANCES_DELETE: &str = "instances-delete";
   pub const INSTANCES_SELECT: &str = "instances-select";
   pub const INSTANCES_LAUNCH: &str = "instances-launch";
   pub const INSTANCES_LOADING: &str = "instances-loading";

   // Game Versions
   pub const GAME_VERSIONS_INSTALL: &str = "game-versions-install";
   pub const GAME_VERSIONS_SAVE: &str = "game-versions-save";
   pub const GAME_VERSIONS_DELETE: &str = "game-versions-delete";
   pub const GAME_VERSIONS_LOADING: &str = "game-versions-loading";

   // Help / CLI
   pub const CMD_CONFIG: &str = "cmd-config";
   pub const CMD_DELETE: &str = "cmd-delete";
   pub const CMD_DOWNLOAD: &str = "cmd-download";
   pub const CMD_HELP: &str = "cmd-help";
   pub const CMD_INFO: &str = "cmd-info";
   pub const CMD_INSTALL: &str = "cmd-install";
   pub const CMD_LIST: &str = "cmd-list";
   pub const CMD_MISC: &str = "cmd-misc";
   pub const CMD_MODPACK: &str = "cmd-modpack";
   pub const CMD_SEARCH: &str = "cmd-search";
   pub const CMD_SELF: &str = "cmd-self";
   pub const CMD_SYNC: &str = "cmd-sync";
   pub const CMD_UPDATE: &str = "cmd-update";
   pub const FLAG_VERBOSE: &str = "flag-verbose";

   // Errors
   pub const ERR_GENERIC: &str = "err-generic";
   pub const ERR_NOT_FOUND: &str = "err-not-found";
   pub const ERR_IO: &str = "err-io";
   pub const ERR_PARSE: &str = "err-parse";
   pub const ERR_NETWORK: &str = "err-network";

   // Status
   pub const STATUS_SYNCING: &str = "status-syncing";
   pub const STATUS_SYNC_COMPLETE: &str = "status-sync-complete";
   pub const STATUS_SYNC_FAILED: &str = "status-sync-failed";
   pub const STATUS_UPDATING: &str = "status-updating";
   pub const STATUS_UPDATE_COMPLETE: &str = "status-update-complete";
   pub const STATUS_UPDATE_FAILED: &str = "status-update-failed";
   pub const STATUS_INSTALLED: &str = "status-installed";
   pub const STATUS_INSTALL_FAILED: &str = "status-install-failed";
   pub const STATUS_DELETED: &str = "status-deleted";
   pub const STATUS_DELETE_FAILED: &str = "status-delete-failed";
   pub const STATUS_REFRESHING: &str = "status-refreshing";
   pub const STATUS_REFRESHED: &str = "status-refreshed";
   pub const STATUS_REFRESH_FAILED: &str = "status-refresh-failed";
   pub const STATUS_LAUNCHING: &str = "status-launching";
   pub const STATUS_LAUNCH_FAILED: &str = "status-launch-failed";
   pub const STATUS_SAVE_FAILED: &str = "status-save-failed";
}

#[cfg(test)]
mod tests {
   use super::*;

   #[test]
   fn english_localizer_loads() {
      let loc = Localizer::new_english();
      assert_eq!(loc.get(ids::NAV_BROWSE), "Browse");
      assert_eq!(loc.get(ids::APP_TITLE), "Lithic - Vintage Story Mod Manager");
   }

   #[test]
   fn global_locale_works() {
      let msg = locale().get(ids::NAV_BROWSE);
      assert_eq!(msg, "Browse");
   }

   #[test]
   fn fallback_on_missing_key() {
      let loc = Localizer::new_english();
      assert_eq!(loc.get("does-not-exist"), "does-not-exist");
   }

   #[test]
   fn get_with_arg() {
      let loc = Localizer::new_english();
      let msg = loc.get_with(ids::STATUS_SYNC_FAILED, "error", "timeout");
      assert!(msg.contains("timeout"), "got: {msg}");
   }
}
