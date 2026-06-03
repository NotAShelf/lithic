use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use iced::widget::{column, container, row, rule, text};
use iced::{Element, Fill, Subscription, Task, Theme};
use native_theme_iced::{from_preset, from_system};

use lithic_core::api::structs::ModApi;
use lithic_core::sync::structs::ModSyncInfo;
use lithic_core::version::filter::{VersionFilter, minor_version};
use lithic_locale::{Localizer, ids};

use crate::ops::{InstanceFormData, SettingsData, SharedGameInstallProgress};
use crate::views::browse::{BrowseView, SortBy};
use crate::views::game_versions::GameVersionsView;
use crate::views::installed::{InstalledTab, InstalledView};
use crate::views::instances::InstancesView;
use crate::views::settings::SettingsView;
use crate::views::settings::{InitialPageOption, ThemeModeOption};
use crate::views::{browse, game_versions, installed, instances, settings};

#[derive(Debug, Clone, PartialEq)]
pub enum View {
   Browse,
   Installed,
   Instances,
   GameVersions,
   Settings,
}

#[derive(Debug, Clone)]
pub enum Message {
   // Navigation
   Navigate(View),

   // Installed: mods
   InstalledLoaded(Result<HashMap<String, ModSyncInfo>, String>),
   SyncMods,
   SyncDone(Result<HashMap<String, ModSyncInfo>, String>),
   UpdateAll,
   UpdateDone(Result<(), String>),
   RequestDelete(String),
   CancelDelete,
   DeleteMod(String),
   DeleteDone(Result<String, String>),
   InstalledSearchChanged(String),
   ToggleInstalledDetail(String),

   // Installed: tabs + modpacks
   InstalledTabChanged(InstalledTab),
   PacksLoaded(Result<(Vec<String>, Vec<String>), String>),
   EnablePack(String),
   DisablePack(String),
   PackOpDone(Result<String, String>),
   ShowCreatePackForm(bool),
   CreatePackName(String),
   CreatePackId(String),
   CreatePackVersion(String),
   CreatePackSubmit,
   CreatePackDone(Result<String, String>),

   // Browse
   BrowseLoaded(Result<Vec<ModApi>, String>),
   BrowseRefresh,
   BrowseRefreshed(Result<Vec<ModApi>, String>),
   BrowseQueryChanged(String),
   BrowseSearch,
   BrowseSortChanged(SortBy),
   BrowseSortToggle,
   BrowseNextPage,
   BrowsePrevPage,
   InstallMod(String),
   AddModToActiveInstance(String),
   InstallDone(String, Result<String, String>),
   AddModToActiveInstanceDone(String, Result<String, String>),
   ToggleFavorite(String),
   ToggleFavoritesFilter,
   ExportFavorites,
   ExportDone(Result<String, String>),
   FavoritesLoaded(Result<HashSet<String>, String>),
   ToggleBrowseDetail(String),
   BrowseVersionFilterChanged(VersionFilter),
   BrowseVersionFilterLoaded(Result<Vec<ModApi>, String>),
   GameVersionsLoaded(Result<Vec<String>, String>),

   // Instances + launcher
   InstancesLoaded(Result<Vec<lithic_core::instance::InstanceConfig>, String>),
   ActiveInstanceLoaded(Result<Option<lithic_core::instance::InstanceConfig>, String>),
   ReloadInstances,
   InstanceFormId(String),
   InstanceDefaultsLoaded((String, String)),
   InstanceFormName(String),
   InstanceFormDataDir(String),
   InstanceFormModsDir(String),
   InstanceFormGameVersionId(String),
   InstanceFormStartParams(String),
   InstanceFormEnvVars(String),
   InstanceModSearchChanged(String),
   OpenInstanceModPicker,
   CloseInstanceModPicker,
   InstanceModSortChanged(SortBy),
   InstanceModSortToggle,
   InstanceModNextPage,
   InstanceModPrevPage,
   ToggleInstanceSelectedMod(String),
   PickInstanceDataDir,
   PickInstanceDataDirDone(Result<String, String>),
   PickInstanceModsDir,
   PickInstanceModsDirDone(Result<String, String>),
   EditInstance(String),
   ClearInstanceForm,
   SaveInstance,
   SelectInstance(String),
   DeleteInstance(String),
   InstanceOpDone(Result<(), String>),
   LaunchActiveInstance,
   LaunchDone(Result<(), String>),

   // Installed game versions
   InstalledGameVersionsLoaded(Result<Vec<lithic_core::instance::GameVersionInstall>, String>),
   ReloadGameVersions,
   GameVersionFormId(String),
   GameVersionFormVersion(String),
   GameVersionFormPath(String),
   GameVersionInstallId(String),
   GameVersionInstallVersion(String),
   GameVersionInstallDir(String),
   PickGameVersionPath,
   PickGameVersionPathDone(Result<String, String>),
   PickGameVersionInstallDir,
   PickGameVersionInstallDirDone(Result<String, String>),
   SaveGameVersion,
   InstallGameVersion,
   PollGameInstallProgress,
   ToggleGameInstallLogs,
   RefreshNativeTheme,
   InstallGameVersionDone(Result<String, String>),
   DeleteGameVersion(String),
   GameVersionOpDone(Result<(), String>),

   // Status clearing
   ClearBrowseStatus,
   ClearInstalledStatus,
   ClearSettingsStatus,

   // Settings
   SettingsLoaded(Result<SettingsData, String>),
   ThemePresetsLoaded(Result<Vec<String>, String>),
   SettingModDir(String),
   SettingGameDownloadDir(String),
   SettingGameVersion(String),
   SettingZipMods(bool),
   SettingBackupMods(bool),
   SettingBackupModsDir(String),
   SettingNotifyUnzipped(bool),
   SettingCheckUpdates(bool),
   SettingShowExecTime(bool),
   SettingModpackDir(String),
   SettingThemeMode(ThemeModeOption),
   SettingThemePreset(String),
   SettingInitialPage(InitialPageOption),
   SaveSettings,
   SettingsSaved(Result<(), String>),
}

pub struct App {
   pub current_view: View,
   pub mod_dir: PathBuf,
   pub installed: InstalledView,
   pub browse: BrowseView,
   pub instances: InstancesView,
   pub game_versions: GameVersionsView,
   pub settings: SettingsView,
   pub game_install_progress: SharedGameInstallProgress,
   pub theme: Theme,
   pub loc: Arc<Localizer>,
   /// Cached translated navigation labels (computed once at startup).
   pub nav_labels: [String; 5],
}

fn clear_after(msg: Message) -> Task<Message> {
   Task::perform(
      async { tokio::time::sleep(Duration::from_secs(4)).await },
      move |_| msg,
   )
}

impl App {
   pub fn new() -> (Self, Task<Message>) {
      // Config init failure is recoverable: we'll run with in-memory defaults
      // until the user fixes the underlying issue. Log it so the cause shows
      // up under `RUST_LOG`, rather than swallowing it silently.
      if let Err(e) = lithic_core::config::manager::init_config() {
         tracing::error!("failed to initialise config (continuing with defaults): {e}");
      }
      let loc = Arc::new(Localizer::new_english());
      let nav_labels = [
         loc.get(ids::NAV_BROWSE).into_owned(),
         loc.get(ids::NAV_INSTALLED).into_owned(),
         loc.get(ids::NAV_INSTANCES).into_owned(),
         loc.get(ids::NAV_GAME_VERSIONS).into_owned(),
         loc.get(ids::NAV_SETTINGS).into_owned(),
      ];
      let app = App {
         current_view: View::Browse,
         mod_dir: PathBuf::new(),
         installed: InstalledView {
            loading: true,
            ..Default::default()
         },
         browse: BrowseView {
            loading: true,
            ..Default::default()
         },
         instances: InstancesView {
            loading: true,
            ..Default::default()
         },
         game_versions: GameVersionsView {
            loading: true,
            ..Default::default()
         },
         settings: SettingsView::default(),
         game_install_progress: crate::ops::new_game_install_progress(),
         theme: detect_native_theme(),
         loc,
         nav_labels,
      };
      let task = Task::batch([
         Task::perform(crate::ops::load_installed(), Message::InstalledLoaded),
         Task::perform(crate::ops::load_settings(), Message::SettingsLoaded),
         Task::perform(crate::ops::load_theme_presets(), Message::ThemePresetsLoaded),
         Task::perform(crate::ops::load_browse(), Message::BrowseLoaded),
         Task::perform(crate::ops::load_favorites(), Message::FavoritesLoaded),
         Task::perform(crate::ops::load_game_versions(), Message::GameVersionsLoaded),
         Task::perform(crate::ops::load_instances(), Message::InstancesLoaded),
         Task::perform(crate::ops::load_active_instance(), Message::ActiveInstanceLoaded),
         Task::perform(
            crate::ops::load_game_version_installs(),
            Message::InstalledGameVersionsLoaded,
         ),
      ]);
      (app, task)
   }

   pub fn title(&self) -> String {
      self.loc.get(ids::APP_TITLE).into_owned()
   }

   #[allow(clippy::too_many_lines)]
   pub fn update(&mut self, message: Message) -> Task<Message> {
      match message {
         // --- Navigation ---
         Message::Navigate(v) => {
            self.current_view = v.clone();
            match v {
               View::Installed
                  if self.installed.tab == InstalledTab::Modpacks
                     && self.installed.packs.is_empty()
                     && self.installed.enabled_packs.is_empty() =>
               {
                  self.installed.loading = true;
                  Task::perform(crate::ops::load_packs(), Message::PacksLoaded)
               }
               _ => Task::none(),
            }
         }

         // --- Installed: mods ---
         Message::InstalledLoaded(Ok(mods)) => {
            self.browse.installed_mods = mods
               .iter()
               .map(|(id, info)| (id.clone(), info.file_name.to_string()))
               .collect();
            self.installed.mods = mods.into_values().collect();
            self.installed.mods.sort_by(|a, b| a.mod_name.cmp(&b.mod_name));
            self.installed.loading = false;
            Task::none()
         }
         Message::InstalledLoaded(Err(e)) => {
            self.installed.status = Some(
               self
                  .loc
                  .get_with("status-error", "error", e.to_string())
                  .into_owned(),
            );
            self.installed.loading = false;
            clear_after(Message::ClearInstalledStatus)
         }
         Message::SyncMods => {
            self.installed.loading = true;
            self.installed.status = Some(self.loc.get(ids::STATUS_SYNCING).into_owned());
            let mod_dir = self.mod_dir.clone();
            Task::perform(crate::ops::sync_mods(mod_dir), Message::SyncDone)
         }
         Message::SyncDone(Ok(mods)) => {
            self.browse.installed_mods = mods
               .iter()
               .map(|(id, info)| (id.clone(), info.file_name.to_string()))
               .collect();
            self.installed.mods = mods.into_values().collect();
            self.installed.mods.sort_by(|a, b| a.mod_name.cmp(&b.mod_name));
            self.installed.loading = false;
            self.installed.status = Some(self.loc.get(ids::STATUS_SYNC_COMPLETE).into_owned());
            clear_after(Message::ClearInstalledStatus)
         }
         Message::SyncDone(Err(e)) => {
            self.installed.loading = false;
            self.installed.status = Some(
               self
                  .loc
                  .get_with(ids::STATUS_SYNC_FAILED, "error", e.to_string())
                  .into_owned(),
            );
            clear_after(Message::ClearInstalledStatus)
         }
         Message::UpdateAll => {
            self.installed.loading = true;
            self.installed.status = Some(self.loc.get(ids::STATUS_UPDATING).into_owned());
            let mod_dir = self.mod_dir.clone();
            Task::perform(crate::ops::update_all(mod_dir), Message::UpdateDone)
         }
         Message::UpdateDone(Ok(())) => {
            self.installed.loading = false;
            self.installed.status = Some(self.loc.get(ids::STATUS_UPDATE_COMPLETE).into_owned());
            let mod_dir = self.mod_dir.clone();
            Task::batch([
               Task::perform(crate::ops::load_installed_from(mod_dir), Message::InstalledLoaded),
               clear_after(Message::ClearInstalledStatus),
            ])
         }
         Message::UpdateDone(Err(e)) => {
            self.installed.loading = false;
            self.installed.status = Some(
               self
                  .loc
                  .get_with(ids::STATUS_UPDATE_FAILED, "error", e.to_string())
                  .into_owned(),
            );
            clear_after(Message::ClearInstalledStatus)
         }
         Message::RequestDelete(file_name) => {
            self.browse.confirm_delete = Some(file_name.clone());
            self.installed.confirm_delete = Some(file_name);
            Task::none()
         }
         Message::CancelDelete => {
            self.browse.confirm_delete = None;
            self.installed.confirm_delete = None;
            Task::none()
         }
         Message::DeleteMod(file_name) => {
            self.browse.confirm_delete = None;
            self.installed.confirm_delete = None;
            let mod_dir = self.mod_dir.clone();
            Task::perform(crate::ops::delete_mod(mod_dir, file_name), Message::DeleteDone)
         }
         Message::DeleteDone(Ok(file_name)) => {
            self.installed.mods.retain(|m| m.file_name != file_name);
            self.browse.installed_mods.retain(|_, f| f != &file_name);
            self.installed.status = Some(
               self
                  .loc
                  .get_with(ids::STATUS_DELETED, "name", file_name)
                  .into_owned(),
            );
            let mod_dir = self.mod_dir.clone();
            Task::batch([
               Task::perform(crate::ops::load_installed_from(mod_dir), Message::InstalledLoaded),
               clear_after(Message::ClearInstalledStatus),
            ])
         }
         Message::DeleteDone(Err(e)) => {
            self.installed.status = Some(
               self
                  .loc
                  .get_with(ids::STATUS_DELETE_FAILED, "error", e.to_string())
                  .into_owned(),
            );
            clear_after(Message::ClearInstalledStatus)
         }
         Message::InstalledSearchChanged(q) => {
            self.installed.search = q;
            Task::none()
         }
         Message::ToggleInstalledDetail(key) => {
            if self.installed.expanded_mod.as_deref() == Some(key.as_str()) {
               self.installed.expanded_mod = None;
            } else {
               self.installed.expanded_mod = Some(key);
            }
            Task::none()
         }

         // --- Installed: tabs + modpacks ---
         Message::InstalledTabChanged(tab) => {
            self.installed.tab = tab.clone();
            if tab == InstalledTab::Modpacks
               && self.installed.packs.is_empty()
               && self.installed.enabled_packs.is_empty()
            {
               self.installed.loading = true;
               Task::perform(crate::ops::load_packs(), Message::PacksLoaded)
            } else {
               Task::none()
            }
         }
         Message::PacksLoaded(Ok((packs, enabled))) => {
            self.installed.packs = packs;
            self.installed.enabled_packs = enabled;
            self.installed.loading = false;
            Task::none()
         }
         Message::PacksLoaded(Err(e)) => {
            self.installed.status = Some(
               self
                  .loc
                  .get_with("status-error", "error", e.to_string())
                  .into_owned(),
            );
            self.installed.loading = false;
            clear_after(Message::ClearInstalledStatus)
         }
         Message::EnablePack(id) => Task::perform(crate::ops::enable_pack(id), Message::PackOpDone),
         Message::DisablePack(id) => Task::perform(crate::ops::disable_pack(id), Message::PackOpDone),
         Message::PackOpDone(Ok(msg)) => {
            self.installed.status = Some(msg);
            Task::batch([
               Task::perform(crate::ops::load_packs(), Message::PacksLoaded),
               clear_after(Message::ClearInstalledStatus),
            ])
         }
         Message::PackOpDone(Err(e)) => {
            self.installed.status = Some(
               self
                  .loc
                  .get_with("status-error", "error", e.to_string())
                  .into_owned(),
            );
            clear_after(Message::ClearInstalledStatus)
         }
         Message::ShowCreatePackForm(show) => {
            self.installed.show_create_form = show;
            Task::none()
         }
         Message::CreatePackName(v) => {
            self.installed.create_name = v;
            Task::none()
         }
         Message::CreatePackId(v) => {
            self.installed.create_id = v;
            Task::none()
         }
         Message::CreatePackVersion(v) => {
            self.installed.create_version = v;
            Task::none()
         }
         Message::CreatePackSubmit => {
            let mod_dir = self.mod_dir.clone();
            let name = self.installed.create_name.clone();
            let id = self.installed.create_id.clone();
            let version = self.installed.create_version.clone();
            self.installed.loading = true;
            self.installed.show_create_form = false;
            Task::perform(
               crate::ops::create_pack(mod_dir, name, id, version),
               Message::CreatePackDone,
            )
         }
         Message::CreatePackDone(Ok(msg)) => {
            self.installed.loading = false;
            self.installed.status = Some(msg);
            self.installed.create_name.clear();
            self.installed.create_id.clear();
            self.installed.create_version.clear();
            Task::batch([
               Task::perform(crate::ops::load_packs(), Message::PacksLoaded),
               clear_after(Message::ClearInstalledStatus),
            ])
         }
         Message::CreatePackDone(Err(e)) => {
            self.installed.loading = false;
            self.installed.status = Some(
               self
                  .loc
                  .get_with("status-create-failed", "error", e.to_string())
                  .into_owned(),
            );
            clear_after(Message::ClearInstalledStatus)
         }

         // --- Browse ---
         Message::BrowseLoaded(Ok(mods)) => {
            self.instances.available_mods = mods.clone();
            self.browse.full_mods = mods.clone();
            self.browse.all_mods = mods;
            self.browse.loading = false;
            apply_browse_filter(&mut self.browse);
            Task::none()
         }
         Message::BrowseLoaded(Err(e)) => {
            self.browse.status = Some(
               self
                  .loc
                  .get_with("status-browse-load-failed", "error", e.to_string())
                  .into_owned(),
            );
            self.browse.loading = false;
            clear_after(Message::ClearBrowseStatus)
         }
         Message::BrowseRefresh => {
            self.browse.loading = true;
            self.browse.status = Some(self.loc.get(ids::STATUS_REFRESHING).into_owned());
            Task::perform(crate::ops::refresh_browse(), Message::BrowseRefreshed)
         }
         Message::BrowseRefreshed(Ok(mods)) => {
            self.browse.full_mods = mods.clone();
            self.browse.status = Some(self.loc.get(ids::STATUS_REFRESHED).into_owned());
            if matches!(self.browse.version_filter, VersionFilter::Any) {
               self.browse.all_mods = mods;
               self.browse.loading = false;
               apply_browse_filter(&mut self.browse);
               clear_after(Message::ClearBrowseStatus)
            } else {
               let filter = self.browse.version_filter.clone();
               let versions = self.browse.available_minor_versions.clone();
               Task::batch([
                  Task::perform(
                     crate::ops::fetch_versioned_browse(filter, versions),
                     Message::BrowseVersionFilterLoaded,
                  ),
                  clear_after(Message::ClearBrowseStatus),
               ])
            }
         }
         Message::BrowseRefreshed(Err(e)) => {
            self.browse.loading = false;
            self.browse.status = Some(
               self
                  .loc
                  .get_with(ids::STATUS_REFRESH_FAILED, "error", e.to_string())
                  .into_owned(),
            );
            clear_after(Message::ClearBrowseStatus)
         }
         Message::BrowseQueryChanged(q) => {
            self.browse.query = q;
            apply_browse_filter(&mut self.browse);
            Task::none()
         }
         Message::BrowseSearch => {
            apply_browse_filter(&mut self.browse);
            Task::none()
         }
         Message::BrowseSortChanged(sort) => {
            self.browse.sort_by = sort;
            apply_browse_filter(&mut self.browse);
            Task::none()
         }
         Message::BrowseSortToggle => {
            self.browse.sort_desc = !self.browse.sort_desc;
            apply_browse_filter(&mut self.browse);
            Task::none()
         }
         Message::BrowseNextPage => {
            if self.browse.page + 1 < self.browse.total_pages() {
               self.browse.page += 1;
            }
            Task::none()
         }
         Message::BrowsePrevPage => {
            if self.browse.page > 0 {
               self.browse.page -= 1;
            }
            Task::none()
         }
         Message::InstallMod(mod_id) => {
            self.browse.installing.insert(mod_id.clone());
            let mod_dir = self.mod_dir.clone();
            let id = mod_id.clone();
            Task::perform(crate::ops::install_mod(mod_dir, mod_id), move |r| {
               Message::InstallDone(id, r)
            })
         }
         Message::AddModToActiveInstance(mod_id) => {
            self.browse.installing.insert(mod_id.clone());
            let id = mod_id.clone();
            Task::perform(crate::ops::install_mod_to_active_instance(mod_id), move |r| {
               Message::AddModToActiveInstanceDone(id, r)
            })
         }
         Message::InstallDone(mod_id, Ok(name)) => {
            self.browse.installing.remove(&mod_id);
            self.browse.status = Some(
               self
                  .loc
                  .get_with(ids::STATUS_INSTALLED, "name", name)
                  .into_owned(),
            );
            let mod_dir = self.mod_dir.clone();
            Task::batch([
               Task::perform(crate::ops::load_installed_from(mod_dir), Message::InstalledLoaded),
               clear_after(Message::ClearBrowseStatus),
            ])
         }
         Message::InstallDone(mod_id, Err(e)) => {
            self.browse.installing.remove(&mod_id);
            self.browse.status = Some(
               self
                  .loc
                  .get_with(ids::STATUS_INSTALL_FAILED, "error", e.to_string())
                  .into_owned(),
            );
            clear_after(Message::ClearBrowseStatus)
         }
         Message::AddModToActiveInstanceDone(mod_id, Ok(name)) => {
            self.browse.installing.remove(&mod_id);
            self.browse.status = Some(
               self
                  .loc
                  .get_with("status-added-to-instance", "name", name)
                  .into_owned(),
            );
            let mod_dir = self.mod_dir.clone();
            Task::batch([
               Task::perform(crate::ops::load_installed_from(mod_dir), Message::InstalledLoaded),
               clear_after(Message::ClearBrowseStatus),
            ])
         }
         Message::AddModToActiveInstanceDone(mod_id, Err(e)) => {
            self.browse.installing.remove(&mod_id);
            self.browse.status = Some(
               self
                  .loc
                  .get_with("status-add-to-instance-failed", "error", e.to_string())
                  .into_owned(),
            );
            clear_after(Message::ClearBrowseStatus)
         }
         Message::ToggleFavorite(key) => {
            if self.browse.favorites.contains(&key) {
               self.browse.favorites.remove(&key);
            } else {
               self.browse.favorites.insert(key);
            }
            let favs = self.browse.favorites.clone();
            apply_browse_filter(&mut self.browse);
            Task::perform(crate::ops::save_favorites(favs), |_| {
               Message::ExportDone(Ok(String::new()))
            })
         }
         Message::ToggleFavoritesFilter => {
            self.browse.show_favorites_only = !self.browse.show_favorites_only;
            apply_browse_filter(&mut self.browse);
            Task::none()
         }
         Message::ExportFavorites => {
            let favs = self.browse.favorites.clone();
            Task::perform(crate::ops::export_favorites(favs), Message::ExportDone)
         }
         Message::ExportDone(Ok(path)) => {
            if !path.is_empty() {
               self.browse.status = Some(
                  self
                     .loc
                     .get_with("status-exported-favorites", "path", path)
                     .into_owned(),
               );
               return clear_after(Message::ClearBrowseStatus);
            }
            Task::none()
         }
         Message::ExportDone(Err(e)) => {
            self.browse.status = Some(
               self
                  .loc
                  .get_with("status-export-failed", "error", e.to_string())
                  .into_owned(),
            );
            clear_after(Message::ClearBrowseStatus)
         }
         Message::FavoritesLoaded(Ok(favs)) => {
            self.browse.favorites = favs;
            apply_browse_filter(&mut self.browse);
            Task::none()
         }
         Message::FavoritesLoaded(Err(_)) => Task::none(),
         Message::ToggleBrowseDetail(key) => {
            if self.browse.expanded_mod.as_deref() == Some(key.as_str()) {
               self.browse.expanded_mod = None;
            } else {
               self.browse.expanded_mod = Some(key);
            }
            Task::none()
         }

         // --- Status clearing ---
         Message::ClearBrowseStatus => {
            self.browse.status = None;
            Task::none()
         }
         Message::ClearInstalledStatus => {
            self.installed.status = None;
            Task::none()
         }
         Message::ClearSettingsStatus => {
            self.settings.status = None;
            Task::none()
         }

         // --- Settings ---
         Message::SettingsLoaded(Ok(data)) => {
            self.settings.mod_dir = data.mod_dir.clone();
            self.settings.game_download_dir = data.game_download_dir;
            self.settings.pinned_game_version = data.pinned_game_version.clone();
            self.settings.zip_mod_files = data.zip_mod_files;
            self.settings.backup_mods = data.backup_mods;
            self.settings.backup_mods_dir = data.backup_mods_dir;
            self.settings.notify_of_unzipped_mods = data.notify_of_unzipped_mods;
            self.settings.check_for_updates = data.check_for_updates;
            self.settings.show_execution_time = data.show_execution_time;
            self.settings.modpack_dir = data.modpack_dir;
            self.settings.theme_mode = ThemeModeOption::from_config(&data.theme_mode);
            self.settings.theme_preset = data.theme_preset;
            self.settings.initial_page = InitialPageOption::from_config(&data.initial_page);
            self.settings.dirty = false;
            self.mod_dir = PathBuf::from(data.mod_dir);
            self.current_view = view_from_initial_page(self.settings.initial_page);
            self.theme = theme_from_mode(self.settings.theme_mode, self.settings.theme_preset.as_str());
            if let Some(minor) = minor_version(&data.pinned_game_version) {
               self.browse.version_filter = VersionFilter::Exact(minor);
               if !self.browse.available_minor_versions.is_empty() {
                  let filter = self.browse.version_filter.clone();
                  let versions = self.browse.available_minor_versions.clone();
                  self.browse.loading = true;
                  return Task::perform(
                     crate::ops::fetch_versioned_browse(filter, versions),
                     Message::BrowseVersionFilterLoaded,
                  );
               }
            }
            Task::none()
         }
         Message::SettingsLoaded(Err(e)) => {
            tracing::error!("Settings load error: {e}");
            Task::none()
         }
         Message::ThemePresetsLoaded(Ok(presets)) => {
            self.settings.available_theme_presets = presets;
            Task::none()
         }
         Message::ThemePresetsLoaded(Err(e)) => {
            tracing::error!("Theme preset load error: {e}");
            Task::none()
         }
         Message::SettingModDir(v) => {
            self.settings.mod_dir = v;
            self.settings.dirty = true;
            Task::none()
         }
         Message::SettingGameDownloadDir(v) => {
            self.settings.game_download_dir = v;
            self.settings.dirty = true;
            Task::none()
         }
         Message::SettingGameVersion(v) => {
            self.settings.pinned_game_version = v;
            self.settings.dirty = true;
            Task::none()
         }
         Message::SettingZipMods(v) => {
            self.settings.zip_mod_files = v;
            self.settings.dirty = true;
            Task::none()
         }
         Message::SettingBackupMods(v) => {
            self.settings.backup_mods = v;
            self.settings.dirty = true;
            Task::none()
         }
         Message::SettingBackupModsDir(v) => {
            self.settings.backup_mods_dir = v;
            self.settings.dirty = true;
            Task::none()
         }
         Message::SettingNotifyUnzipped(v) => {
            self.settings.notify_of_unzipped_mods = v;
            self.settings.dirty = true;
            Task::none()
         }
         Message::SettingCheckUpdates(v) => {
            self.settings.check_for_updates = v;
            self.settings.dirty = true;
            Task::none()
         }
         Message::SettingShowExecTime(v) => {
            self.settings.show_execution_time = v;
            self.settings.dirty = true;
            Task::none()
         }
         Message::SettingModpackDir(v) => {
            self.settings.modpack_dir = v;
            self.settings.dirty = true;
            Task::none()
         }
         Message::SettingThemeMode(v) => {
            self.settings.theme_mode = v;
            self.settings.dirty = true;
            self.theme = theme_from_mode(v, self.settings.theme_preset.as_str());
            Task::none()
         }
         Message::SettingThemePreset(v) => {
            self.settings.theme_preset = v;
            self.settings.dirty = true;
            self.theme = theme_from_mode(self.settings.theme_mode, self.settings.theme_preset.as_str());
            Task::none()
         }
         Message::SettingInitialPage(v) => {
            self.settings.initial_page = v;
            self.settings.dirty = true;
            Task::none()
         }
         Message::SaveSettings => {
            let data = SettingsData {
               mod_dir: self.settings.mod_dir.clone(),
               game_download_dir: self.settings.game_download_dir.clone(),
               pinned_game_version: self.settings.pinned_game_version.clone(),
               zip_mod_files: self.settings.zip_mod_files,
               backup_mods: self.settings.backup_mods,
               backup_mods_dir: self.settings.backup_mods_dir.clone(),
               notify_of_unzipped_mods: self.settings.notify_of_unzipped_mods,
               check_for_updates: self.settings.check_for_updates,
               show_execution_time: self.settings.show_execution_time,
               modpack_dir: self.settings.modpack_dir.clone(),
               theme_mode: self.settings.theme_mode.as_config().to_string(),
               theme_preset: self.settings.theme_preset.clone(),
               initial_page: self.settings.initial_page.as_config().to_string(),
            };
            Task::perform(crate::ops::save_settings(data), Message::SettingsSaved)
         }
         Message::SettingsSaved(Ok(())) => {
            self.settings.dirty = false;
            self.settings.status = Some(self.loc.get("status-settings-saved").into_owned());
            self.mod_dir = PathBuf::from(&self.settings.mod_dir);
            self.theme = theme_from_mode(self.settings.theme_mode, self.settings.theme_preset.as_str());
            let new_filter = minor_version(&self.settings.pinned_game_version)
               .map(VersionFilter::Exact)
               .unwrap_or(VersionFilter::Any);
            if new_filter != self.browse.version_filter {
               self.browse.version_filter = new_filter.clone();
               match new_filter {
                  VersionFilter::Any => {
                     self.browse.all_mods = self.browse.full_mods.clone();
                     apply_browse_filter(&mut self.browse);
                  }
                  _ if !self.browse.available_minor_versions.is_empty() => {
                     self.browse.loading = true;
                     let versions = self.browse.available_minor_versions.clone();
                     return Task::batch([
                        Task::perform(
                           crate::ops::fetch_versioned_browse(new_filter, versions),
                           Message::BrowseVersionFilterLoaded,
                        ),
                        clear_after(Message::ClearSettingsStatus),
                     ]);
                  }
                  _ => {}
               }
            }
            clear_after(Message::ClearSettingsStatus)
         }
         Message::SettingsSaved(Err(e)) => {
            self.settings.status = Some(
               self
                  .loc
                  .get_with(ids::STATUS_SAVE_FAILED, "error", e.to_string())
                  .into_owned(),
            );
            clear_after(Message::ClearSettingsStatus)
         }

         // --- Version filtering ---
         Message::GameVersionsLoaded(Ok(versions)) => {
            self.browse.available_minor_versions = versions;
            if !matches!(self.browse.version_filter, VersionFilter::Any) {
               let filter = self.browse.version_filter.clone();
               let vers = self.browse.available_minor_versions.clone();
               self.browse.loading = true;
               Task::perform(
                  crate::ops::fetch_versioned_browse(filter, vers),
                  Message::BrowseVersionFilterLoaded,
               )
            } else {
               Task::none()
            }
         }
         Message::GameVersionsLoaded(Err(_)) => Task::none(),
         Message::BrowseVersionFilterChanged(filter) => {
            self.browse.version_filter = filter.clone();
            match filter {
               VersionFilter::Any => {
                  self.browse.all_mods = self.browse.full_mods.clone();
                  apply_browse_filter(&mut self.browse);
                  Task::none()
               }
               _ => {
                  self.browse.loading = true;
                  let versions = self.browse.available_minor_versions.clone();
                  Task::perform(
                     crate::ops::fetch_versioned_browse(filter, versions),
                     Message::BrowseVersionFilterLoaded,
                  )
               }
            }
         }
         Message::BrowseVersionFilterLoaded(Ok(mods)) => {
            self.browse.all_mods = mods;
            self.browse.loading = false;
            apply_browse_filter(&mut self.browse);
            Task::none()
         }
         Message::BrowseVersionFilterLoaded(Err(e)) => {
            self.browse.loading = false;
            self.browse.status = Some(
               self
                  .loc
                  .get_with("status-version-filter-failed", "error", e.to_string())
                  .into_owned(),
            );
            clear_after(Message::ClearBrowseStatus)
         }

         // --- Instances + launcher ---
         Message::ReloadInstances => {
            self.instances.loading = true;
            Task::batch([
               Task::perform(crate::ops::load_instances(), Message::InstancesLoaded),
               Task::perform(crate::ops::load_active_instance(), Message::ActiveInstanceLoaded),
            ])
         }
         Message::InstancesLoaded(Ok(instances)) => {
            self.instances.instances = instances;
            self.instances.loading = false;
            Task::none()
         }
         Message::InstancesLoaded(Err(e)) => {
            self.instances.status = Some(
               self
                  .loc
                  .get_with("status-instances-load-failed", "error", e.to_string())
                  .into_owned(),
            );
            self.instances.loading = false;
            Task::none()
         }
         Message::ActiveInstanceLoaded(Ok(active)) => {
            if let Some(active) = active {
               self.instances.active_instance_id = active.id.clone();
               self.mod_dir = active.mods_dir;
            } else {
               self.instances.active_instance_id.clear();
            }
            Task::none()
         }
         Message::ActiveInstanceLoaded(Err(e)) => {
            self.instances.status = Some(
               self
                  .loc
                  .get_with("status-active-instance-load-failed", "error", e.to_string())
                  .into_owned(),
            );
            Task::none()
         }
         Message::InstanceFormId(v) => {
            self.instances.form_id = v.clone();
            if self.instances.form_data_dir.trim().is_empty()
               || self.instances.form_mods_dir.trim().is_empty()
            {
               Task::perform(
                  crate::ops::default_instance_paths(v),
                  Message::InstanceDefaultsLoaded,
               )
            } else {
               Task::none()
            }
         }
         Message::InstanceDefaultsLoaded((data_dir, mods_dir)) => {
            if self.instances.form_data_dir.trim().is_empty() {
               self.instances.form_data_dir = data_dir;
            }
            if self.instances.form_mods_dir.trim().is_empty() {
               self.instances.form_mods_dir = mods_dir;
            }
            Task::none()
         }
         Message::InstanceFormName(v) => {
            self.instances.form_name = v;
            Task::none()
         }
         Message::InstanceFormDataDir(v) => {
            self.instances.form_data_dir = v;
            Task::none()
         }
         Message::InstanceFormModsDir(v) => {
            self.instances.form_mods_dir = v;
            Task::none()
         }
         Message::InstanceFormGameVersionId(v) => {
            self.instances.form_game_version_id = v;
            Task::none()
         }
         Message::InstanceFormStartParams(v) => {
            self.instances.form_start_params = v;
            Task::none()
         }
         Message::InstanceFormEnvVars(v) => {
            self.instances.form_env_vars = v;
            Task::none()
         }
         Message::InstanceModSearchChanged(v) => {
            self.instances.mod_search = v;
            self.instances.mod_page = 0;
            Task::none()
         }
         Message::OpenInstanceModPicker => {
            self.instances.show_mod_picker = true;
            Task::none()
         }
         Message::CloseInstanceModPicker => {
            self.instances.show_mod_picker = false;
            Task::none()
         }
         Message::InstanceModSortChanged(sort) => {
            self.instances.mod_sort_by = sort;
            self.instances.mod_page = 0;
            Task::none()
         }
         Message::InstanceModSortToggle => {
            self.instances.mod_sort_desc = !self.instances.mod_sort_desc;
            Task::none()
         }
         Message::InstanceModNextPage => {
            self.instances.mod_page += 1;
            Task::none()
         }
         Message::InstanceModPrevPage => {
            if self.instances.mod_page > 0 {
               self.instances.mod_page -= 1;
            }
            Task::none()
         }
         Message::ToggleInstanceSelectedMod(id) => {
            if self.instances.selected_mod_ids.contains(&id) {
               self.instances.selected_mod_ids.retain(|m| m != &id);
            } else {
               self.instances.selected_mod_ids.push(id);
               self.instances.selected_mod_ids.sort();
            }
            Task::none()
         }
         Message::PickInstanceDataDir => {
            Task::perform(crate::ops::pick_folder(), Message::PickInstanceDataDirDone)
         }
         Message::PickInstanceDataDirDone(Ok(path)) => {
            self.instances.form_data_dir = path.clone();
            if self.instances.form_mods_dir.trim().is_empty() {
               self.instances.form_mods_dir = PathBuf::from(&path).join("Mods").to_string_lossy().to_string();
            }
            Task::none()
         }
         Message::PickInstanceDataDirDone(Err(e)) => {
            self.instances.status = Some(
               self
                  .loc
                  .get_with("status-folder-picker-failed", "error", e.to_string())
                  .into_owned(),
            );
            Task::none()
         }
         Message::PickInstanceModsDir => {
            Task::perform(crate::ops::pick_folder(), Message::PickInstanceModsDirDone)
         }
         Message::PickInstanceModsDirDone(Ok(path)) => {
            self.instances.form_mods_dir = path;
            Task::none()
         }
         Message::PickInstanceModsDirDone(Err(e)) => {
            self.instances.status = Some(
               self
                  .loc
                  .get_with("status-folder-picker-failed", "error", e.to_string())
                  .into_owned(),
            );
            Task::none()
         }
         Message::EditInstance(id) => {
            if let Some(inst) = self.instances.instances.iter().find(|i| i.id == id) {
               self.instances.form_id = inst.id.clone();
               self.instances.form_name = inst.name.clone();
               self.instances.form_data_dir = inst.data_dir.to_string_lossy().to_string();
               self.instances.form_mods_dir = inst.mods_dir.to_string_lossy().to_string();
               self.instances.form_game_version_id = inst.game_version_id.clone();
               self.instances.form_start_params = inst.start_params.clone();
               self.instances.form_env_vars = inst.env_vars.clone();
            }
            Task::none()
         }
         Message::ClearInstanceForm => {
            self.instances.form_id.clear();
            self.instances.form_name.clear();
            self.instances.form_data_dir.clear();
            self.instances.form_mods_dir.clear();
            self.instances.form_game_version_id.clear();
            self.instances.form_start_params.clear();
            self.instances.form_env_vars.clear();
            self.instances.selected_mod_ids.clear();
            self.instances.mod_search.clear();
            Task::none()
         }
         Message::SaveInstance => Task::perform(
            crate::ops::upsert_instance(InstanceFormData {
               id: self.instances.form_id.clone(),
               name: self.instances.form_name.clone(),
               data_dir: self.instances.form_data_dir.clone(),
               mods_dir: self.instances.form_mods_dir.clone(),
               game_version_id: self.instances.form_game_version_id.clone(),
               start_params: self.instances.form_start_params.clone(),
               env_vars: self.instances.form_env_vars.clone(),
               selected_mod_ids: self.instances.selected_mod_ids.clone(),
            }),
            Message::InstanceOpDone,
         ),
         Message::SelectInstance(id) => {
            Task::perform(crate::ops::set_active_instance(id), Message::InstanceOpDone)
         }
         Message::DeleteInstance(id) => {
            Task::perform(crate::ops::delete_instance(id), Message::InstanceOpDone)
         }
         Message::InstanceOpDone(Ok(())) => {
            self.instances.status = Some(self.loc.get("status-instance-op-complete").into_owned());
            Task::batch([
               Task::perform(crate::ops::load_instances(), Message::InstancesLoaded),
               Task::perform(crate::ops::load_active_instance(), Message::ActiveInstanceLoaded),
               Task::perform(crate::ops::load_installed(), Message::InstalledLoaded),
            ])
         }
         Message::InstanceOpDone(Err(e)) => {
            self.instances.status = Some(
               self
                  .loc
                  .get_with("status-instance-op-failed", "error", e.to_string())
                  .into_owned(),
            );
            Task::none()
         }
         Message::LaunchActiveInstance => {
            self.instances.status = Some(self.loc.get(ids::STATUS_LAUNCHING).into_owned());
            Task::perform(crate::ops::launch_active_instance(), Message::LaunchDone)
         }
         Message::LaunchDone(Ok(())) => {
            self.instances.status = Some(self.loc.get("status-game-exit-success").into_owned());
            Task::none()
         }
         Message::LaunchDone(Err(e)) => {
            self.instances.status = Some(
               self
                  .loc
                  .get_with(ids::STATUS_LAUNCH_FAILED, "error", e.to_string())
                  .into_owned(),
            );
            Task::none()
         }

         // --- Installed game versions ---
         Message::ReloadGameVersions => {
            self.game_versions.loading = true;
            Task::perform(
               crate::ops::load_game_version_installs(),
               Message::InstalledGameVersionsLoaded,
            )
         }
         Message::InstalledGameVersionsLoaded(Ok(versions)) => {
            self.instances.game_versions = versions.clone();
            self.game_versions.versions = versions;
            self.game_versions.loading = false;
            Task::none()
         }
         Message::InstalledGameVersionsLoaded(Err(e)) => {
            self.game_versions.status = Some(
               self
                  .loc
                  .get_with("status-game-versions-load-failed", "error", e.to_string())
                  .into_owned(),
            );
            self.game_versions.loading = false;
            Task::none()
         }
         Message::GameVersionFormId(v) => {
            self.game_versions.form_id = v;
            Task::none()
         }
         Message::GameVersionFormVersion(v) => {
            self.game_versions.form_version = v;
            Task::none()
         }
         Message::GameVersionFormPath(v) => {
            self.game_versions.form_path = v;
            Task::none()
         }
         Message::GameVersionInstallId(v) => {
            self.game_versions.install_id = v;
            Task::none()
         }
         Message::GameVersionInstallVersion(v) => {
            self.game_versions.install_version = v;
            Task::none()
         }
         Message::GameVersionInstallDir(v) => {
            self.game_versions.install_dir = v;
            Task::none()
         }
         Message::PickGameVersionPath => {
            Task::perform(crate::ops::pick_folder(), Message::PickGameVersionPathDone)
         }
         Message::PickGameVersionPathDone(Ok(path)) => {
            self.game_versions.form_path = path;
            Task::none()
         }
         Message::PickGameVersionPathDone(Err(e)) => {
            self.game_versions.status = Some(
               self
                  .loc
                  .get_with("status-folder-picker-failed", "error", e.to_string())
                  .into_owned(),
            );
            Task::none()
         }
         Message::PickGameVersionInstallDir => {
            Task::perform(crate::ops::pick_folder(), Message::PickGameVersionInstallDirDone)
         }
         Message::PickGameVersionInstallDirDone(Ok(path)) => {
            self.game_versions.install_dir = path;
            Task::none()
         }
         Message::PickGameVersionInstallDirDone(Err(e)) => {
            self.game_versions.status = Some(
               self
                  .loc
                  .get_with("status-folder-picker-failed", "error", e.to_string())
                  .into_owned(),
            );
            Task::none()
         }
         Message::SaveGameVersion => Task::perform(
            crate::ops::upsert_game_version_install(
               self.game_versions.form_id.clone(),
               self.game_versions.form_version.clone(),
               self.game_versions.form_path.clone(),
            ),
            Message::GameVersionOpDone,
         ),
         Message::InstallGameVersion => {
            self.game_versions.installing = true;
            self.game_versions.status = Some(self.loc.get("status-installing-vs").into_owned());
            let id = if self.game_versions.install_id.trim().is_empty() {
               self.game_versions.install_version.clone()
            } else {
               self.game_versions.install_id.clone()
            };
            Task::perform(
               crate::ops::install_game_version(
                  id,
                  self.game_versions.install_version.clone(),
                  self.game_versions.install_dir.clone(),
                  self.game_install_progress.clone(),
               ),
               Message::InstallGameVersionDone,
            )
         }
         Message::PollGameInstallProgress => {
            if let Ok(progress) = self.game_install_progress.lock() {
               self.game_versions.install_banner = progress.clone();
            }
            Task::none()
         }
         Message::RefreshNativeTheme => {
            if self.settings.theme_mode == ThemeModeOption::System {
               self.theme = detect_native_theme();
            }
            Task::none()
         }
         Message::ToggleGameInstallLogs => {
            self.game_versions.show_install_logs = !self.game_versions.show_install_logs;
            Task::none()
         }
         Message::InstallGameVersionDone(Ok(msg)) => {
            self.game_versions.installing = false;
            self.game_versions.status = Some(msg);
            Task::perform(
               crate::ops::load_game_version_installs(),
               Message::InstalledGameVersionsLoaded,
            )
         }
         Message::InstallGameVersionDone(Err(e)) => {
            self.game_versions.installing = false;
            self.game_versions.status = Some(
               self
                  .loc
                  .get_with(ids::STATUS_INSTALL_FAILED, "error", e.to_string())
                  .into_owned(),
            );
            if let Ok(mut progress) = self.game_install_progress.lock() {
               progress.active = false;
               progress.done = true;
               progress.error = Some(e.clone());
               progress.logs.push(
                  self
                     .loc
                     .get_with(ids::STATUS_INSTALL_FAILED, "error", e.to_string())
                     .into_owned(),
               );
            }
            Task::none()
         }
         Message::DeleteGameVersion(id) => Task::perform(
            crate::ops::delete_game_version_install(id),
            Message::GameVersionOpDone,
         ),
         Message::GameVersionOpDone(Ok(())) => {
            self.game_versions.status = Some(self.loc.get("status-game-version-op-complete").into_owned());
            Task::perform(
               crate::ops::load_game_version_installs(),
               Message::InstalledGameVersionsLoaded,
            )
         }
         Message::GameVersionOpDone(Err(e)) => {
            self.game_versions.status = Some(
               self
                  .loc
                  .get_with("status-game-version-op-failed", "error", e.to_string())
                  .into_owned(),
            );
            Task::none()
         }
      }
   }

   pub fn view(&self) -> Element<'_, Message> {
      let loc = &self.loc;
      let nav_items: &[(&str, View)] = &[
         (&self.nav_labels[0], View::Browse),
         (&self.nav_labels[1], View::Installed),
         (&self.nav_labels[2], View::Instances),
         (&self.nav_labels[3], View::GameVersions),
         (&self.nav_labels[4], View::Settings),
      ];

      let nav_buttons: Vec<Element<'_, Message>> = nav_items
         .iter()
         .map(|(label, view)| {
            crate::widgets::nav_button(label, self.current_view == *view, Message::Navigate(view.clone()))
         })
         .collect();

      let sidebar = container(column![
         container(text(loc.get("app-brand")).size(17)).padding(iced::Padding {
            top: 16.0,
            right: 16.0,
            bottom: 12.0,
            left: 20.0
         }),
         rule::horizontal(1),
         column(nav_buttons).spacing(2).padding([4, 0]),
         iced::widget::space::vertical(),
         rule::horizontal(1),
         container(text(loc.get("app-version")).size(11).color(iced::Color {
            r: 0.40,
            g: 0.40,
            b: 0.40,
            a: 1.0
         }))
         .padding([8, 16]),
      ])
      .style(|_theme: &iced::Theme| iced::widget::container::Style {
         background: Some(
            iced::Color {
               r: 1.0,
               g: 1.0,
               b: 1.0,
               a: 0.02,
            }
            .into(),
         ),
         ..Default::default()
      })
      .width(160)
      .height(Fill);

      let content: Element<'_, Message> = match &self.current_view {
         View::Browse => browse::view(&self.browse, loc),
         View::Installed => installed::view(&self.installed, &self.settings.pinned_game_version, loc),
         View::Instances => instances::view(&self.instances, loc),
         View::GameVersions => game_versions::view(&self.game_versions, loc),
         View::Settings => settings::view(&self.settings, loc),
      };

      container(row![sidebar, rule::vertical(1), content].spacing(0))
         .width(Fill)
         .height(Fill)
         .into()
   }

   pub fn subscription(&self) -> Subscription<Message> {
      Subscription::batch([
         iced::time::every(Duration::from_millis(250)).map(|_| Message::PollGameInstallProgress),
         iced::time::every(Duration::from_secs(5)).map(|_| Message::RefreshNativeTheme),
      ])
   }
}

fn browse_mod_key(m: &ModApi) -> String {
   m.mod_id_strs
      .first()
      .cloned()
      .unwrap_or_else(|| m.mod_id.to_string())
}

fn apply_browse_filter(browse: &mut BrowseView) {
   let base: Vec<ModApi> = if browse.query.trim().is_empty() {
      browse.all_mods.clone()
   } else {
      crate::ops::search_mods(&browse.all_mods, &browse.query)
   };

   let mut filtered: Vec<ModApi> = if browse.show_favorites_only {
      base
         .into_iter()
         .filter(|m| browse.favorites.contains(&browse_mod_key(m)))
         .collect()
   } else {
      base
   };

   match browse.sort_by {
      SortBy::Downloads => {
         filtered.sort_by(|a, b| {
            if browse.sort_desc {
               b.downloads.cmp(&a.downloads)
            } else {
               a.downloads.cmp(&b.downloads)
            }
         });
      }
      SortBy::Follows => {
         filtered.sort_by(|a, b| {
            if browse.sort_desc {
               b.follows.cmp(&a.follows)
            } else {
               a.follows.cmp(&b.follows)
            }
         });
      }
      SortBy::Trending => {
         filtered.sort_by(|a, b| {
            if browse.sort_desc {
               b.trending_points.cmp(&a.trending_points)
            } else {
               a.trending_points.cmp(&b.trending_points)
            }
         });
      }
      SortBy::Name => {
         filtered.sort_by(|a, b| {
            let an = a.name.as_deref().unwrap_or("");
            let bn = b.name.as_deref().unwrap_or("");
            if browse.sort_desc { bn.cmp(an) } else { an.cmp(bn) }
         });
      }
   }

   browse.filtered = filtered;
   browse.page = 0;
}

pub fn run() -> iced::Result {
   iced::application(App::new, App::update, App::view)
      .title(App::title)
      .theme(|app: &App| app.theme.clone())
      .subscription(App::subscription)
      .window(iced::window::Settings {
         size: iced::Size::new(1100.0, 720.0),
         ..Default::default()
      })
      .run()
}

fn detect_native_theme() -> Theme {
   from_system().map(|(theme, _, _)| theme).unwrap_or(Theme::Dark)
}

fn theme_from_mode(mode: ThemeModeOption, preset: &str) -> Theme {
   match mode {
      ThemeModeOption::System => detect_native_theme(),
      ThemeModeOption::Light => Theme::Light,
      ThemeModeOption::Dark => Theme::Dark,
      ThemeModeOption::Preset => from_preset(preset, true)
         .map(|(theme, _)| theme)
         .unwrap_or_else(|_| detect_native_theme()),
   }
}

fn view_from_initial_page(page: InitialPageOption) -> View {
   match page {
      InitialPageOption::Browse => View::Browse,
      InitialPageOption::Installed => View::Installed,
      InitialPageOption::Instances => View::Instances,
      InitialPageOption::GameVersions => View::GameVersions,
      InitialPageOption::Settings => View::Settings,
   }
}
