use std::path::{Path, PathBuf};

use iced::{
  Center,
  Element,
  Fill,
  Task,
  widget::{
    button,
    column,
    pick_list,
    progress_bar,
    row,
    scrollable,
    space,
    text,
    text_input,
    toggler,
  },
};
use lithic_core::{
  game::{Install, Platform, find_executable},
  version,
};

use crate::{
  app::{Message as AppMessage, OpKind, Outcome, Shared},
  i18n::{t, t1},
  style::{self, Tone},
  task::{blocking, pick_folder},
  widget,
};

#[derive(Debug, Default)]
pub struct State {
  show_unstable:  bool,
  selected:       Option<String>,
  add_version:    String,
  add_path:       Option<PathBuf>,
  adding:         bool,
  confirm_remove: Option<Install>,
}

#[derive(Debug, Clone)]
pub enum Message {
  ShowUnstable(bool),
  Select(String),
  Install,
  Cancel(String),
  RetryList,
  AddVersion(String),
  PickAddPath,
  AddPath(PathBuf),
  Add,
  Added(Result<Install, String>),
  AskRemove(Install),
  CancelRemove,
  Remove,
  Removed(Result<(), String>),
  Open(PathBuf),
}

pub fn busy_key(version: &str) -> String {
  format!("game:{version}")
}

impl State {
  pub fn update(
    &mut self,
    message: Message,
    shared: &mut Shared,
  ) -> Task<AppMessage> {
    match message {
      Message::ShowUnstable(on) => self.show_unstable = on,
      Message::Select(v) => self.selected = Some(v),
      Message::Install => {
        let Some(version) = self.selected.take() else {
          return Task::none();
        };
        return shared.start_op(
          busy_key(&version),
          OpKind::GameInstall,
          move |lithic, reporter, cancel| {
            async move {
              lithic
                .install_game(&version, &reporter, &cancel)
                .await
                .map(|i| Outcome::GameInstalled(i.version))
                .map_err(|e| e.to_string())
            }
          },
        );
      },
      Message::Cancel(version) => {
        return Task::done(AppMessage::CancelOp(busy_key(&version)));
      },
      Message::RetryList => return Task::done(AppMessage::ReloadManifest),
      Message::AddVersion(v) => self.add_version = v,
      Message::PickAddPath => {
        return Task::future(pick_folder(t("games-add-pick")))
          .and_then(|p| Task::done(AppMessage::Games(Message::AddPath(p))));
      },
      Message::AddPath(p) => {
        if self.add_version.trim().is_empty()
          && let Some(guess) = guess_version(&p)
        {
          self.add_version = guess;
        }
        self.add_path = Some(p);
      },
      Message::Add => {
        let (Some(path), version) =
          (self.add_path.clone(), self.add_version.trim().to_string())
        else {
          return Task::none();
        };
        if version.is_empty() || self.adding {
          return Task::none();
        }
        self.adding = true;
        let lithic = shared.lithic.clone();
        return Task::perform(
          blocking(move || lithic.add_game_install(&version, &path)),
          |r| AppMessage::Games(Message::Added(r)),
        );
      },
      Message::Added(result) => {
        self.adding = false;
        return match result {
          Ok(install) => {
            self.add_path = None;
            self.add_version.clear();
            Task::batch([
              shared.toasts.success(t1(
                "games-added",
                "version",
                install.version,
              )),
              Task::done(AppMessage::Reload),
            ])
          },
          Err(e) => shared.toasts.error(t("games-add-failed"), Some(e)),
        };
      },
      Message::AskRemove(install) => self.confirm_remove = Some(install),
      Message::CancelRemove => self.confirm_remove = None,
      Message::Remove => {
        let Some(install) = self.confirm_remove.take() else {
          return Task::none();
        };
        let lithic = shared.lithic.clone();
        return Task::perform(
          blocking(move || lithic.remove_game_install(&install.version)),
          |r| AppMessage::Games(Message::Removed(r)),
        );
      },
      Message::Removed(Ok(())) => return Task::done(AppMessage::Reload),
      Message::Removed(Err(e)) => {
        return shared.toasts.error(t("games-remove-failed"), Some(e));
      },
      Message::Open(path) => {
        return Task::done(AppMessage::Open(path.display().to_string()));
      },
    }
    Task::none()
  }

  pub fn view<'a>(&'a self, shared: &'a Shared) -> Element<'a, Message> {
    let installed: Element<Message> = if shared.installs.is_empty() {
      widget::notice(text(t("games-none")), Tone::Neutral)
    } else {
      column(shared.installs.iter().map(|i| install_card(i, shared)))
        .spacing(8)
        .into()
    };

    let running: Vec<Element<Message>> = shared
      .busy
      .iter()
      .filter_map(|(key, busy)| {
        let version = key.strip_prefix("game:")?;
        let bar: Element<Message> = busy.fraction().map_or_else(
          || text(t("games-working")).size(12).style(style::muted).into(),
          |f| progress_bar(0.0..=1.0, f).girth(8).into(),
        );
        Some(
          widget::card(
            column![
              row![
                text(t1("games-installing", "version", version.to_string()))
                  .font(widget::bold())
                  .width(Fill),
                button(text(t("common-cancel")).size(13))
                  .style(button::text)
                  .on_press(Message::Cancel(version.to_string())),
              ]
              .align_y(Center),
              text(busy.label()).size(13).style(style::muted),
              bar,
            ]
            .spacing(8),
          )
          .into(),
        )
      })
      .collect();

    let body = column![
      section_title(t("games-installed")),
      installed,
      column(running).spacing(8),
      section_title(t("games-install-title")),
      self.install_panel(shared),
      section_title(t("games-add-title")),
      self.add_panel(),
    ]
    .spacing(12)
    .max_width(820);

    let page =
      widget::page(t("nav-games"), space(), scrollable(body).height(Fill));
    match &self.confirm_remove {
      Some(install) => {
        let body = if install.managed {
          t1(
            "games-remove-managed",
            "path",
            install.path.display().to_string(),
          )
        } else {
          t1(
            "games-remove-external",
            "path",
            install.path.display().to_string(),
          )
        };
        widget::modal(
          page,
          widget::confirm(
            t1("games-remove-title", "version", install.version.clone()),
            body,
            t("common-remove"),
            install.managed,
            Message::Remove,
            Message::CancelRemove,
          ),
          Message::CancelRemove,
        )
      },
      None => page,
    }
  }

  fn install_panel<'a>(&'a self, shared: &'a Shared) -> Element<'a, Message> {
    let Some(manifest) = &shared.manifest else {
      return widget::card(
        row![
          text(t("games-list-unavailable")).width(Fill),
          button(text(t("common-retry")))
            .style(button::secondary)
            .on_press(Message::RetryList),
        ]
        .align_y(Center),
      )
      .into();
    };
    let Some(platform) = Platform::host_client() else {
      return widget::notice(text(t("games-unsupported-os")), Tone::Warn);
    };
    let versions: Vec<String> = manifest
      .releases
      .iter()
      .filter(|r| self.show_unstable || !r.is_prerelease())
      .filter(|r| r.artifacts.contains_key(&platform))
      .filter(|r| !shared.installs.iter().any(|i| i.version == r.version))
      .map(|r| r.version.clone())
      .collect();
    let size = self.selected.as_ref().and_then(|v| {
      manifest
        .release(v)
        .and_then(|r| r.artifacts.get(&platform))
        .and_then(|a| a.size.clone())
    });
    let busy = self
      .selected
      .as_ref()
      .is_some_and(|v| shared.busy.contains_key(&busy_key(v)));

    widget::card(
      column![
        text(t("games-install-hint")).size(13).style(style::muted),
        row![
          pick_list(versions, self.selected.clone(), Message::Select)
            .placeholder(t("games-pick-version"))
            .width(220),
          widget::action(
            t("games-install"),
            t("games-installing-short"),
            busy,
            self.selected.is_some().then_some(Message::Install)
          ),
          space::horizontal(),
          toggler(self.show_unstable)
            .label(t("games-show-unstable"))
            .on_toggle(Message::ShowUnstable),
        ]
        .spacing(12)
        .align_y(Center),
      ]
      .extend(size.map(|s| {
        text(t1("games-download-size", "size", s))
          .size(12)
          .style(style::muted)
          .into()
      }))
      .spacing(12),
    )
    .into()
  }

  fn add_panel(&self) -> Element<'_, Message> {
    let path_label = self
      .add_path
      .as_ref()
      .map_or_else(|| t("games-add-no-folder"), |p| p.display().to_string());
    widget::card(
      column![
        text(t("games-add-hint")).size(13).style(style::muted),
        row![
          text(path_label).size(13).width(Fill),
          button(text(t("common-choose-folder")).size(13))
            .style(button::secondary)
            .on_press(Message::PickAddPath),
        ]
        .spacing(8)
        .align_y(Center),
        row![
          text_input(&t("games-add-version"), &self.add_version)
            .on_input(Message::AddVersion)
            .on_submit(Message::Add)
            .padding(8)
            .width(220),
          space::horizontal(),
          widget::action(
            t("games-add"),
            t("games-adding"),
            self.adding,
            (self.add_path.is_some() && !self.add_version.trim().is_empty())
              .then_some(Message::Add)
          ),
        ]
        .spacing(12)
        .align_y(Center),
      ]
      .spacing(12),
    )
    .into()
  }
}

fn section_title<'a>(label: String) -> Element<'a, Message> {
  text(label).size(16).font(widget::bold()).into()
}

fn install_card<'a>(
  install: &'a Install,
  shared: &'a Shared,
) -> Element<'a, Message> {
  let users: Vec<&str> = shared
    .instances
    .iter()
    .filter(|i| i.game_version.as_deref() == Some(install.version.as_str()))
    .map(|i| i.name.as_str())
    .collect();
  let present = find_executable(&install.path).is_some();
  let mut badges = row![widget::badge(
    if install.managed {
      t("games-by-lithic")
    } else {
      t("games-by-you")
    },
    Tone::Neutral
  )]
  .spacing(6);
  if !present {
    badges = badges.push(widget::badge(t("games-missing"), Tone::Bad));
  }
  let used = if users.is_empty() {
    t("games-unused")
  } else {
    t1("games-used-by", "names", users.join(", "))
  };
  widget::card(
    row![
      column![
        row![
          text(format!("Vintage Story {}", install.version))
            .size(16)
            .font(widget::bold()),
          badges
        ]
        .spacing(8)
        .align_y(Center),
        text(install.path.display().to_string())
          .size(12)
          .style(style::muted),
        text(used).size(13),
      ]
      .spacing(4)
      .width(Fill),
      button(text(t("common-open-folder")).size(13))
        .style(button::text)
        .on_press(Message::Open(install.path.clone())),
      button(text(t("common-remove")).size(13))
        .style(button::text)
        .on_press(Message::AskRemove(install.clone())),
    ]
    .spacing(8)
    .align_y(Center),
  )
  .into()
}

/// A version number found in a folder name, such as `vintagestory-1.21.5`.
fn guess_version(path: &Path) -> Option<String> {
  path.ancestors().take(3).find_map(|p| {
    let name = p.file_name()?.to_string_lossy();
    name
      .split(|c: char| {
        !(c.is_ascii_digit() || c == '.' || c == '-' || c.is_ascii_lowercase())
      })
      .flat_map(|part| part.split('_'))
      .find_map(|part| {
        let candidate = part.trim_start_matches(|c: char| !c.is_ascii_digit());
        (version::parse(candidate).is_some() && candidate.contains('.'))
          .then(|| candidate.to_string())
      })
  })
}

#[cfg(test)]
mod tests {
  use std::path::Path;

  use super::guess_version;

  #[test]
  fn version_from_folder_names() {
    assert_eq!(
      guess_version(Path::new("/games/vintagestory-1.21.5")).as_deref(),
      Some("1.21.5")
    );
    assert_eq!(
      guess_version(Path::new("/games/1.20.12/vintagestory")).as_deref(),
      Some("1.20.12")
    );
    assert_eq!(guess_version(Path::new("/games/vintagestory")), None);
  }
}
