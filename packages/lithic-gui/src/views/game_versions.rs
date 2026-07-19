use iced::widget::{
   button, column, container, pick_list, progress_bar, row, scrollable, text, text_editor, text_input,
};
use iced::{Alignment, Element, Fill, Font};
use lithic_core::instance::GameVersionInstall;
use lithic_locale::{Localizer, ids};

use crate::app::Message;
use crate::ops::GameInstallProgress;
use crate::widgets::{card_style, danger_btn_style, ghost_btn_style, primary_btn_style, status_element};

#[derive(Debug, Clone, Default)]
pub struct GameVersionsView {
   pub versions: Vec<GameVersionInstall>,
   pub available_versions: Vec<String>,
   pub loading: bool,
   pub installing: bool,
   pub status: Option<String>,
   pub form_id: String,
   pub form_version: String,
   pub form_path: String,
   pub install_id: String,
   pub install_version: String,
   pub install_dir: String,
   pub install_banner: GameInstallProgress,
   pub show_install_logs: bool,
}

pub fn view<'a>(
   state: &'a GameVersionsView,
   log_content: &'a text_editor::Content,
   loc: &'a Localizer,
) -> Element<'a, Message> {
   let header = row![
      text(loc.get("game-versions-title")).size(22).width(Fill),
      button(text(loc.get("game-versions-reload")))
         .on_press(Message::ReloadGameVersions)
         .style(ghost_btn_style),
   ]
   .spacing(8)
   .align_y(Alignment::Center);

   let install_banner: Element<'_, Message> = if state.install_banner.active || state.install_banner.done {
      let title = if let Some(error) = &state.install_banner.error {
         loc.get_with("game-versions-banner-title-error", "error", error.to_string())
            .into_owned()
      } else if state.install_banner.done {
         loc.get("game-versions-banner-title-complete").into_owned()
      } else {
         // Percent carries its own leading space so the pattern renders
         // cleanly while the stage has no percentage yet.
         let percent_text = state
            .install_banner
            .percent
            .map(|p| format!(" {p}%"))
            .unwrap_or_default();
         loc.get_with2(
            "game-versions-banner-title-progress",
            "stage",
            state.install_banner.stage.to_string(),
            "percent",
            percent_text,
         )
         .into_owned()
      };

      let bar_value = state.install_banner.percent.unwrap_or(0) as f32 / 100.0;
      let logs: Element<'_, Message> = if state.show_install_logs {
         // Read-only text editor: selectable + Ctrl+C copyable, edits are
         // dropped in the `GameInstallLogAction` handler.
         text_editor(log_content)
            .on_action(Message::GameInstallLogAction)
            .font(Font::MONOSPACE)
            .size(11)
            .height(150)
            .into()
      } else {
         iced::widget::Space::new().into()
      };

      let copy_logs: Element<'_, Message> =
         if state.show_install_logs && !state.install_banner.logs.is_empty() {
            button(text(loc.get("game-versions-copy-logs")))
               .on_press(Message::CopyGameInstallLogs)
               .style(ghost_btn_style)
               .into()
         } else {
            iced::widget::Space::new().into()
         };

      container(
         column![
            row![
               text(title).size(14).width(Fill),
               copy_logs,
               button(if state.show_install_logs {
                  text(loc.get("game-versions-hide-logs"))
               } else {
                  text(loc.get("game-versions-view-logs"))
               })
               .on_press(Message::ToggleGameInstallLogs)
               .style(ghost_btn_style),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
            progress_bar(0.0..=1.0, bar_value),
            logs,
         ]
         .spacing(8),
      )
      .padding(12)
      .style(card_style)
      .into()
   } else {
      iced::widget::Space::new().into()
   };

   let form = container(
      column![
         text(loc.get("game-versions-attach-title")).size(14),
         text_input(
            loc.get("game-versions-form-id-placeholder").as_ref(),
            &state.form_id
         )
         .on_input(Message::GameVersionFormId),
         text_input(
            loc.get("game-versions-form-version-placeholder").as_ref(),
            &state.form_version
         )
         .on_input(Message::GameVersionFormVersion),
         row![
            text_input(
               loc.get("game-versions-form-path-placeholder").as_ref(),
               &state.form_path
            )
            .on_input(Message::GameVersionFormPath)
            .width(Fill),
            button(text(loc.get("game-versions-browse")))
               .on_press(Message::PickGameVersionPath)
               .style(ghost_btn_style),
         ]
         .spacing(6),
         button(text(loc.get(ids::GAME_VERSIONS_SAVE)))
            .on_press(Message::SaveGameVersion)
            .style(primary_btn_style),
      ]
      .spacing(6),
   )
   .padding(12)
   .style(card_style);

   // Version selection: dropdown of known versions when the version list is
   // available (defaults to latest stable); free-text fallback when offline
   // or the list failed to load.
   let version_field: Element<'_, Message> = if state.available_versions.is_empty() {
      text_input(
         loc.get("game-versions-install-version-placeholder").as_ref(),
         &state.install_version,
      )
      .on_input(Message::GameVersionInstallVersion)
      .into()
   } else {
      let selected = if state.install_version.is_empty() {
         None
      } else {
         Some(state.install_version.clone())
      };
      pick_list(
         state.available_versions.as_slice(),
         selected,
         Message::GameVersionInstallVersion,
      )
      .placeholder(loc.get("game-versions-install-version-placeholder").as_ref())
      .width(Fill)
      .into()
   };

   let can_install = !state.installing && !state.install_version.trim().is_empty();
   let install_button: Element<'_, Message> = if state.installing {
      button(text(loc.get("game-versions-installing-label")))
         .style(ghost_btn_style)
         .into()
   } else {
      button(text(loc.get("game-versions-download-install")))
         .on_press_maybe(can_install.then_some(Message::InstallGameVersion))
         .style(primary_btn_style)
         .into()
   };

   let install_form = container(
      column![
         text(loc.get("game-versions-install-title")).size(14),
         text_input(
            loc.get("game-versions-install-id-placeholder").as_ref(),
            &state.install_id
         )
         .on_input(Message::GameVersionInstallId),
         version_field,
         row![
            text_input(
               loc.get("game-versions-install-dir-placeholder").as_ref(),
               &state.install_dir
            )
            .on_input(Message::GameVersionInstallDir)
            .width(Fill),
            button(text(loc.get("game-versions-browse")))
               .on_press(Message::PickGameVersionInstallDir)
               .style(ghost_btn_style),
         ]
         .spacing(6),
         install_button,
      ]
      .spacing(6),
   )
   .padding(12)
   .style(card_style);

   let mut rows: Vec<Element<'_, Message>> = Vec::new();
   for gv in &state.versions {
      rows.push(
         container(
            row![
               column![
                  text(format!("{} ({})", gv.version, gv.id)).size(14),
                  text(gv.path.display().to_string()).size(12),
               ]
               .spacing(4)
               .width(Fill),
               button(text(loc.get(ids::GAME_VERSIONS_DELETE)))
                  .on_press(Message::DeleteGameVersion(gv.id.clone()))
                  .style(danger_btn_style),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
         )
         .padding(10)
         .style(card_style)
         .into(),
      );
   }

   let body: Element<'_, Message> = if state.loading {
      container(text(loc.get(ids::GAME_VERSIONS_LOADING)))
         .center(Fill)
         .into()
   } else {
      scrollable(column(rows).spacing(6)).height(Fill).into()
   };

   column![
      header,
      {
         if state.install_banner.active || state.install_banner.done {
            iced::widget::Space::new().height(16).into()
         } else {
            status_element(state.status.as_deref())
         }
      },
      install_banner,
      install_form,
      form,
      body
   ]
   .spacing(10)
   .padding(16)
   .height(Fill)
   .into()
}
