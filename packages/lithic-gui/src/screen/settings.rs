use std::{
  fmt,
  path::{Path, PathBuf},
};

use iced::{
  Center,
  Element,
  Fill,
  Task,
  Theme,
  widget::{
    button,
    column,
    container,
    responsive,
    row,
    rule,
    scrollable,
    space,
    text,
    toggler,
  },
};
use lithic_core::{Settings, settings::GuiSettings};
use lithic_icons::{self as icon, Icon};
use native_theme_iced::{Theme as NativeTheme, from_preset};

use crate::{
  app::{Message as AppMessage, Page, Shared},
  i18n::t,
  style,
  task::{blocking, pick_folder},
  theme,
  widget,
};

const CONTROL_WIDTH: f32 = 220.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeMode {
  System,
  Light,
  Dark,
  Preset,
}

impl ThemeMode {
  const ALL: [Self; 4] = [Self::System, Self::Light, Self::Dark, Self::Preset];

  const fn key(self) -> &'static str {
    match self {
      Self::System => "system",
      Self::Light => "light",
      Self::Dark => "dark",
      Self::Preset => "preset",
    }
  }

  fn from_key(key: &str) -> Self {
    Self::ALL
      .into_iter()
      .find(|m| m.key() == key)
      .unwrap_or(Self::System)
  }
}

impl fmt::Display for ThemeMode {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.write_str(&match self {
      Self::System => t("theme-system"),
      Self::Light => t("theme-light"),
      Self::Dark => t("theme-dark"),
      Self::Preset => t("theme-preset"),
    })
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartPage {
  Instances,
  Browse,
  Games,
  Accounts,
  Settings,
}

impl StartPage {
  const ALL: [Self; 5] = [
    Self::Instances,
    Self::Browse,
    Self::Games,
    Self::Accounts,
    Self::Settings,
  ];

  const fn key(self) -> &'static str {
    match self {
      Self::Instances => "instances",
      Self::Browse => "browse",
      Self::Games => "games",
      Self::Accounts => "accounts",
      Self::Settings => "settings",
    }
  }

  fn from_key(key: &str) -> Self {
    match Page::from_setting(key) {
      Page::Browse => Self::Browse,
      Page::Games => Self::Games,
      Page::Accounts => Self::Accounts,
      Page::Settings => Self::Settings,
      _ => Self::Instances,
    }
  }
}

impl fmt::Display for StartPage {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.write_str(&match self {
      Self::Instances => t("nav-instances"),
      Self::Browse => t("nav-browse"),
      Self::Games => t("nav-games"),
      Self::Accounts => t("nav-accounts"),
      Self::Settings => t("nav-settings"),
    })
  }
}

/// The theme for the current settings. Without a known system preference,
/// Lithic starts dark.
pub fn resolve_theme(gui: &GuiSettings, system: Option<&Theme>) -> Theme {
  let system_dark = system.is_none_or(|t| t.extended_palette().is_dark);
  match ThemeMode::from_key(&gui.theme_mode) {
    ThemeMode::Light => theme::lithic(false),
    ThemeMode::Dark => theme::lithic(true),
    ThemeMode::Preset => {
      from_preset(&gui.theme_preset, system_dark)
        .map_or_else(|_| theme::lithic(system_dark), |(theme, _)| theme)
    },
    ThemeMode::System => theme::lithic(system_dark),
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Section {
  #[default]
  General,
  Mods,
  Storage,
}

impl Section {
  const ALL: [Self; 3] = [Self::General, Self::Mods, Self::Storage];

  fn label(self) -> String {
    match self {
      Self::General => t("settings-general"),
      Self::Mods => t("settings-mods"),
      Self::Storage => t("settings-storage"),
    }
  }

  const fn icon(self) -> Icon {
    match self {
      Self::General => Icon::Settings,
      Self::Mods => Icon::Instances,
      Self::Storage => Icon::Folder,
    }
  }
}

#[derive(Debug, Clone)]
pub enum Change {
  ThemeMode(ThemeMode),
  Preset(String),
  StartPage(StartPage),
  AllowPrerelease(bool),
  Concurrency(usize),
  Backups(bool),
  BackupsKeep(usize),
  BackupDir(Option<PathBuf>),
  GameDir(Option<PathBuf>),
}

impl Change {
  fn apply(&self, s: &mut Settings) {
    match self {
      Self::ThemeMode(mode) => s.gui.theme_mode = mode.key().to_string(),
      Self::Preset(name) => s.gui.theme_preset.clone_from(name),
      Self::StartPage(page) => s.gui.initial_page = page.key().to_string(),
      Self::AllowPrerelease(on) => s.mods.allow_prerelease = *on,
      Self::Concurrency(n) => s.mods.concurrency = *n,
      Self::Backups(on) => s.backups.enabled = *on,
      Self::BackupsKeep(n) => s.backups.keep = *n,
      Self::BackupDir(dir) => s.backups.dir.clone_from(dir),
      Self::GameDir(dir) => s.game.install_dir.clone_from(dir),
    }
  }
}

#[derive(Debug, Clone)]
pub enum Message {
  Section(Section),
  Change(Change),
  PickBackupDir,
  PickGameDir,
  Saved(Result<Box<Settings>, String>),
  Open(PathBuf),
}

#[derive(Debug, Default)]
pub struct State {
  section: Section,
}

impl State {
  pub fn update(
    &mut self,
    message: Message,
    shared: &mut Shared,
  ) -> Task<AppMessage> {
    match message {
      Message::Section(section) => {
        self.section = section;
        Task::none()
      },
      Message::Change(change) => {
        change.apply(&mut shared.settings);
        let lithic = shared.lithic.clone();
        Task::perform(
          blocking(move || {
            lithic.update_settings(|s| change.apply(s))?;
            lithic.settings().map(Box::new)
          }),
          |r| AppMessage::Settings(Message::Saved(r)),
        )
      },
      Message::PickBackupDir => {
        Task::future(pick_folder(t("settings-backups-dir"))).and_then(|d| {
          Task::done(AppMessage::Settings(Message::Change(Change::BackupDir(
            Some(d),
          ))))
        })
      },
      Message::PickGameDir => {
        Task::future(pick_folder(t("settings-game-dir"))).and_then(|d| {
          Task::done(AppMessage::Settings(Message::Change(Change::GameDir(
            Some(d),
          ))))
        })
      },
      Message::Saved(Ok(settings)) => {
        shared.settings = *settings;
        Task::done(AppMessage::CheckSystemTheme)
      },
      Message::Saved(Err(e)) => {
        Task::batch([
          shared.toasts.error(t("settings-save-failed"), Some(e)),
          Task::done(AppMessage::Reload),
        ])
      },
      Message::Open(path) => {
        Task::done(AppMessage::Open(path.display().to_string()))
      },
    }
  }

  pub fn view<'a>(&'a self, shared: &'a Shared) -> Element<'a, Message> {
    let nav = column(Section::ALL.into_iter().map(|section| {
      let active = self.section == section;
      button(
        row![
          icon::colored(section.icon(), 16.0, move |theme| {
            let p = theme.extended_palette();
            if active {
              p.primary.base.color
            } else {
              p.background.base.text
            }
          }),
          text(section.label()).size(14),
        ]
        .spacing(10)
        .align_y(Center),
      )
      .width(Fill)
      .padding([7, 12])
      .style(style::nav(active))
      .on_press(Message::Section(section))
      .into()
    }))
    .spacing(4)
    .width(190);

    let section = self.section;
    // Rows stay readable on wide windows instead of pushing controls far
    // from their labels.
    let pane = responsive(move |size| {
      let rows = match section {
        Section::General => general(shared),
        Section::Mods => mods(shared),
        Section::Storage => storage(shared),
      };
      container(scrollable(
        column![
          text(section.label()).size(18).font(widget::bold()),
          widget::divided(rows),
        ]
        .spacing(8)
        .padding([0, 8]),
      ))
      .width(size.width.min(880.0))
      .into()
    });

    widget::page(
      t("nav-settings"),
      space(),
      row![nav, rule::vertical(1).style(style::divider), pane]
        .spacing(20)
        .height(Fill),
    )
  }
}

fn general(shared: &Shared) -> Vec<Element<'_, Message>> {
  let s = &shared.settings;
  let mode = ThemeMode::from_key(&s.gui.theme_mode);
  let mut rows = vec![widget::setting_row(
    t("settings-theme"),
    None,
    widget::select(ThemeMode::ALL, Some(mode), |m| {
      Message::Change(Change::ThemeMode(m))
    })
    .width(CONTROL_WIDTH),
  )];
  if mode == ThemeMode::Preset {
    let presets: Vec<String> = NativeTheme::list_presets()
      .iter()
      .map(|p| p.key.to_string())
      .collect();
    let selected = presets.iter().find(|p| **p == s.gui.theme_preset).cloned();
    rows.push(widget::setting_row(
      t("settings-theme-preset"),
      None,
      widget::select(presets, selected, |p| Message::Change(Change::Preset(p)))
        .placeholder(t("settings-theme-preset-pick"))
        .width(CONTROL_WIDTH),
    ));
  }
  rows.push(widget::setting_row(
    t("settings-start-page"),
    None,
    widget::select(
      StartPage::ALL,
      Some(StartPage::from_key(&s.gui.initial_page)),
      |p| Message::Change(Change::StartPage(p)),
    )
    .width(CONTROL_WIDTH),
  ));
  rows
}

fn mods(shared: &Shared) -> Vec<Element<'_, Message>> {
  let s = &shared.settings;
  let counts: Vec<usize> = (1..=16).collect();
  let mut rows = vec![
    widget::setting_row(
      t("settings-prerelease"),
      Some(t("settings-prerelease-hint")),
      toggler(s.mods.allow_prerelease)
        .on_toggle(|on| Message::Change(Change::AllowPrerelease(on))),
    ),
    widget::setting_row(
      t("settings-concurrency"),
      None,
      widget::select(counts, Some(s.mods.concurrency), |n| {
        Message::Change(Change::Concurrency(n))
      })
      .width(CONTROL_WIDTH),
    ),
    widget::setting_row(
      t("settings-backups"),
      None,
      toggler(s.backups.enabled)
        .on_toggle(|on| Message::Change(Change::Backups(on))),
    ),
  ];
  if s.backups.enabled {
    let keeps: Vec<usize> = (1..=10).collect();
    let dir = s
      .backups
      .dir
      .clone()
      .unwrap_or_else(|| shared.lithic.paths.backups_dir());
    rows.push(widget::setting_row(
      t("settings-backups-keep"),
      None,
      widget::select(keeps, Some(s.backups.keep), |n| {
        Message::Change(Change::BackupsKeep(n))
      })
      .width(CONTROL_WIDTH),
    ));
    rows.push(folder_row(
      t("settings-backups-dir"),
      None,
      &dir,
      Message::PickBackupDir,
      s.backups
        .dir
        .is_some()
        .then_some(Message::Change(Change::BackupDir(None))),
    ));
  }
  rows
}

fn storage(shared: &Shared) -> Vec<Element<'_, Message>> {
  let s = &shared.settings;
  let paths = &shared.lithic.paths;
  let game_dir = s
    .game
    .install_dir
    .clone()
    .unwrap_or_else(|| paths.game_dir());
  vec![
    folder_row(
      t("settings-game-dir"),
      Some(t("settings-game-dir-hint")),
      &game_dir,
      Message::PickGameDir,
      s.game
        .install_dir
        .is_some()
        .then_some(Message::Change(Change::GameDir(None))),
    ),
    path_row(t("settings-path-config"), paths.config.clone()),
    path_row(t("settings-path-data"), paths.data.clone()),
    path_row(t("settings-path-cache"), paths.cache.clone()),
  ]
}

fn path_text<'a>(path: &Path) -> Element<'a, Message> {
  container(
    text(path.display().to_string())
      .size(13)
      .style(style::muted)
      .wrapping(text::Wrapping::WordOrGlyph),
  )
  .max_width(560)
  .into()
}

fn folder_row<'a>(
  label: String,
  hint: Option<String>,
  path: &Path,
  pick: Message,
  reset: Option<Message>,
) -> Element<'a, Message> {
  let mut control = row![
    path_text(path),
    widget::icon_button(
      Icon::Folder,
      t("common-open-folder"),
      Some(Message::Open(path.to_path_buf()))
    ),
    widget::secondary(t("common-change"), Some(pick)),
  ]
  .spacing(8)
  .align_y(Center);
  if let Some(reset) = reset {
    control = control.push(widget::icon_button(
      Icon::Close,
      t("common-reset"),
      Some(reset),
    ));
  }
  widget::setting_row(label, hint, control)
}

fn path_row<'a>(label: String, path: PathBuf) -> Element<'a, Message> {
  widget::setting_row(
    label,
    None,
    row![
      path_text(&path),
      widget::icon_button(
        Icon::Folder,
        t("common-open-folder"),
        Some(Message::Open(path))
      ),
    ]
    .spacing(8)
    .align_y(Center),
  )
}

#[cfg(test)]
#[expect(
  clippy::unwrap_used,
  reason = "test setup and assertions intentionally fail on error"
)]
mod tests {
  use lithic_core::{Lithic, Paths};

  use super::*;

  #[test]
  fn sections_switch() {
    let dir = tempfile::tempdir().unwrap();
    let mut shared =
      Shared::new(Lithic::new(Paths::rooted(dir.path())).unwrap());
    let mut state = State::default();
    assert_eq!(state.section, Section::General);
    let _ = state.update(Message::Section(Section::Storage), &mut shared);
    assert_eq!(state.section, Section::Storage);
  }
}
