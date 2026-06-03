use std::fmt::{Display, Formatter};

use iced::widget::{button, checkbox, column, pick_list, row, scrollable, text, text_input};
use iced::{Alignment, Color, Element, Fill};
use lithic_core::version::filter::minor_version;
use lithic_locale::{Localizer, ids, locale};

use crate::app::Message;
use crate::widgets::{section_card, section_label, status_element};

#[derive(Debug, Default, Clone)]
pub struct SettingsView {
   pub mod_dir: String,
   pub game_download_dir: String,
   pub pinned_game_version: String,
   pub zip_mod_files: bool,
   pub backup_mods: bool,
   pub backup_mods_dir: String,
   pub notify_of_unzipped_mods: bool,
   pub check_for_updates: bool,
   pub show_execution_time: bool,
   pub modpack_dir: String,
   pub theme_mode: ThemeModeOption,
   pub theme_preset: String,
   pub available_theme_presets: Vec<String>,
   pub initial_page: InitialPageOption,
   pub dirty: bool,
   pub status: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemeModeOption {
   #[default]
   System,
   Light,
   Dark,
   Preset,
}

impl ThemeModeOption {
   pub const ALL: [Self; 4] = [Self::System, Self::Light, Self::Dark, Self::Preset];

   pub fn from_config(value: &str) -> Self {
      match value {
         "light" => Self::Light,
         "dark" => Self::Dark,
         "preset" => Self::Preset,
         _ => Self::System,
      }
   }

   pub fn as_config(self) -> &'static str {
      match self {
         Self::System => "system",
         Self::Light => "light",
         Self::Dark => "dark",
         Self::Preset => "preset",
      }
   }
}

impl Display for ThemeModeOption {
   fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
      // iced's `pick_list` renders dropdown items via this `Display` impl, so
      // route the visible label through the global localizer instead of hard-
      // coding English.
      let id = match self {
         Self::System => "settings-theme-system",
         Self::Light => "settings-theme-light",
         Self::Dark => "settings-theme-dark",
         Self::Preset => "settings-theme-preset-mode",
      };
      f.write_str(&locale().get(id))
   }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InitialPageOption {
   #[default]
   Browse,
   Installed,
   Instances,
   GameVersions,
   Settings,
}

impl InitialPageOption {
   pub const ALL: [Self; 5] = [
      Self::Browse,
      Self::Installed,
      Self::Instances,
      Self::GameVersions,
      Self::Settings,
   ];

   pub fn from_config(value: &str) -> Self {
      match value {
         "installed" => Self::Installed,
         "instances" => Self::Instances,
         "game_versions" => Self::GameVersions,
         "settings" => Self::Settings,
         _ => Self::Browse,
      }
   }

   pub fn as_config(self) -> &'static str {
      match self {
         Self::Browse => "browse",
         Self::Installed => "installed",
         Self::Instances => "instances",
         Self::GameVersions => "game_versions",
         Self::Settings => "settings",
      }
   }
}

impl Display for InitialPageOption {
   fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
      // Reuse the nav-* keys so the startup-page picker matches the sidebar
      // exactly, in any future locale.
      let id = match self {
         Self::Browse => ids::NAV_BROWSE,
         Self::Installed => ids::NAV_INSTALLED,
         Self::Instances => ids::NAV_INSTANCES,
         Self::GameVersions => ids::NAV_GAME_VERSIONS,
         Self::Settings => ids::NAV_SETTINGS,
      };
      f.write_str(&locale().get(id))
   }
}

pub fn view<'a>(state: &'a SettingsView, loc: &'a Localizer) -> Element<'a, Message> {
   let header = row![text(loc.get("settings-title")).size(22).width(Fill)].align_y(Alignment::Center);

   let paths_card = section_card(
      column![
         column![
            section_label(loc.get("settings-section-mods-dir")),
            text_input(loc.get("settings-mod-dir-placeholder").as_ref(), &state.mod_dir)
               .on_input(Message::SettingModDir),
         ]
         .spacing(6),
         column![
            section_label(loc.get("settings-section-game-download-dir")),
            text_input(
               loc.get("settings-game-dir-placeholder").as_ref(),
               &state.game_download_dir
            )
            .on_input(Message::SettingGameDownloadDir),
         ]
         .spacing(6),
         column![
            section_label(loc.get("settings-section-modpack-dir")),
            text_input(
               loc.get("settings-modpack-dir-placeholder").as_ref(),
               &state.modpack_dir
            )
            .on_input(Message::SettingModpackDir),
         ]
         .spacing(6),
      ]
      .spacing(16),
   );

   let browse_gate_note: Element<'_, Message> = if state.pinned_game_version.is_empty() {
      iced::widget::Space::new().into()
   } else if let Some(minor) = minor_version(&state.pinned_game_version) {
      text(loc.get_with("settings-browse-filter-note", "minor", minor.to_string()))
         .size(11)
         .color(Color {
            r: 0.45,
            g: 0.75,
            b: 0.50,
            a: 1.0,
         })
         .into()
   } else {
      text(loc.get("settings-invalid-version"))
         .size(11)
         .color(Color {
            r: 0.75,
            g: 0.40,
            b: 0.40,
            a: 1.0,
         })
         .into()
   };

   let game_card = section_card(
      column![
         section_label(loc.get("settings-game-version-section")),
         column![
            section_label(loc.get("settings-pinned-version-label")),
            text_input(
               loc.get("settings-pinned-version-placeholder").as_ref(),
               &state.pinned_game_version
            )
            .on_input(Message::SettingGameVersion),
            browse_gate_note,
         ]
         .spacing(6),
      ]
      .spacing(16),
   );

   let updates_card = section_card(
      column![
         section_label(loc.get("settings-updates")),
         checkbox(state.check_for_updates)
            .label(loc.get("settings-check-updates"))
            .on_toggle(Message::SettingCheckUpdates),
      ]
      .spacing(12),
   );

   let app_card = section_card(
      column![
         section_label(loc.get("settings-appearance")),
         row![
            text(loc.get("settings-theme")).size(12).width(100),
            pick_list(
               ThemeModeOption::ALL,
               Some(state.theme_mode),
               Message::SettingThemeMode
            )
            .width(180),
         ]
         .spacing(8)
         .align_y(Alignment::Center),
         row![
            text(loc.get("settings-preset")).size(12).width(100),
            pick_list(
               state.available_theme_presets.as_slice(),
               if state.theme_preset.is_empty() {
                  None
               } else {
                  Some(state.theme_preset.clone())
               },
               Message::SettingThemePreset
            )
            .placeholder(loc.get("settings-preset-placeholder"))
            .width(260),
         ]
         .spacing(8)
         .align_y(Alignment::Center),
         row![
            text(loc.get("settings-startup-page")).size(12).width(100),
            pick_list(
               InitialPageOption::ALL,
               Some(state.initial_page),
               Message::SettingInitialPage
            )
            .width(180),
         ]
         .spacing(8)
         .align_y(Alignment::Center),
      ]
      .spacing(10),
   );

   let files_card = section_card(
      column![
         section_label(loc.get("settings-mod-files")),
         checkbox(state.zip_mod_files)
            .label(loc.get("settings-zip-mods"))
            .on_toggle(Message::SettingZipMods),
         checkbox(state.notify_of_unzipped_mods)
            .label(loc.get("settings-notify-unzipped"))
            .on_toggle(Message::SettingNotifyUnzipped),
         checkbox(state.show_execution_time)
            .label(loc.get("settings-show-exec-time"))
            .on_toggle(Message::SettingShowExecTime),
      ]
      .spacing(12),
   );

   let backup_card = section_card(
      column![
         section_label(loc.get("settings-backup")),
         checkbox(state.backup_mods)
            .label(loc.get("settings-backup-mods"))
            .on_toggle(Message::SettingBackupMods),
         {
            let backup_dir: Element<'_, Message> = if state.backup_mods {
               column![
                  section_label(loc.get("settings-backup-dir")),
                  text_input(
                     loc.get("settings-backup-dir-placeholder").as_ref(),
                     &state.backup_mods_dir
                  )
                  .on_input(Message::SettingBackupModsDir),
               ]
               .spacing(6)
               .into()
            } else {
               iced::widget::Space::new().into()
            };
            backup_dir
         },
      ]
      .spacing(12),
   );

   let save_label = if state.dirty {
      loc.get("settings-save-dirty")
   } else {
      loc.get(ids::SETTINGS_SAVE)
   };
   let save_btn: Element<'_, Message> = if state.dirty {
      button(text(save_label)).on_press(Message::SaveSettings).into()
   } else {
      button(text(save_label)).into()
   };

   let footer = row![iced::widget::Space::new().width(Fill), save_btn].align_y(Alignment::Center);

   scrollable(
      column![
         header,
         status_element(state.status.as_deref()),
         paths_card,
         game_card,
         app_card,
         updates_card,
         files_card,
         backup_card,
         footer,
      ]
      .spacing(12)
      .padding(16)
      .width(Fill),
   )
   .height(Fill)
   .into()
}
