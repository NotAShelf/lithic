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
    pick_list,
    responsive,
    row,
    scrollable,
    space,
    text,
    toggler,
  },
};
use lithic_core::{Settings, settings::GuiSettings};
use native_theme_iced::{Theme as NativeTheme, from_preset};

use crate::{
  app::{Message as AppMessage, Page, Shared},
  i18n::t,
  style,
  task::{blocking, pick_folder},
  widget,
};

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

/// The theme for the current settings. `None` lets iced follow the system
/// light or dark preference.
pub fn resolve_theme(
  gui: &GuiSettings,
  system: Option<&Theme>,
) -> Option<Theme> {
  match ThemeMode::from_key(&gui.theme_mode) {
    ThemeMode::Light => Some(Theme::Light),
    ThemeMode::Dark => Some(Theme::Dark),
    ThemeMode::Preset => {
      let dark = system.is_none_or(|t| t.extended_palette().is_dark);
      from_preset(&gui.theme_preset, dark)
        .map(|(theme, _)| theme)
        .ok()
        .or_else(|| system.cloned())
    },
    ThemeMode::System => system.cloned(),
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
  Change(Change),
  PickBackupDir,
  PickGameDir,
  Saved(Result<Box<Settings>, String>),
  Open(PathBuf),
}

pub fn update(message: Message, shared: &mut Shared) -> Task<AppMessage> {
  match message {
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

pub fn view(shared: &Shared) -> Element<'_, Message> {
  widget::page(
    t("nav-settings"),
    space(),
    responsive(move |size| {
      scrollable(body(shared, size.width)).height(Fill).into()
    }),
  )
}

fn body(shared: &Shared, width: f32) -> Element<'_, Message> {
  let compact_paths = width < 620.0;
  let s = &shared.settings;
  let change = |c: Change| Message::Change(c);
  let mode = ThemeMode::from_key(&s.gui.theme_mode);

  let mut appearance = column![widget::field(
    t("settings-theme"),
    pick_list(ThemeMode::ALL, Some(mode), move |m| {
      change(Change::ThemeMode(m))
    }),
    None
  )]
  .spacing(16);
  if mode == ThemeMode::Preset {
    let presets: Vec<String> = NativeTheme::list_presets()
      .iter()
      .map(|p| p.key.to_string())
      .collect();
    let selected = presets.iter().find(|p| **p == s.gui.theme_preset).cloned();
    appearance = appearance.push(widget::field(
      t("settings-theme-preset"),
      pick_list(presets, selected, move |p| change(Change::Preset(p)))
        .placeholder(t("settings-theme-preset-pick")),
      None,
    ));
  }
  appearance = appearance.push(widget::field(
    t("settings-start-page"),
    pick_list(
      StartPage::ALL,
      Some(StartPage::from_key(&s.gui.initial_page)),
      move |p| change(Change::StartPage(p)),
    ),
    None,
  ));

  let counts: Vec<usize> = (1..=16).collect();
  let mut mods = column![
    toggler(s.mods.allow_prerelease)
      .label(t("settings-prerelease"))
      .on_toggle(move |on| change(Change::AllowPrerelease(on))),
    text(t("settings-prerelease-hint"))
      .size(12)
      .style(style::muted),
    widget::field(
      t("settings-concurrency"),
      pick_list(counts, Some(s.mods.concurrency), move |n| {
        change(Change::Concurrency(n))
      }),
      None
    ),
    toggler(s.backups.enabled)
      .label(t("settings-backups"))
      .on_toggle(move |on| change(Change::Backups(on))),
    text(t("settings-backups-hint"))
      .size(12)
      .style(style::muted),
  ]
  .spacing(10);
  if s.backups.enabled {
    let keeps: Vec<usize> = (1..=10).collect();
    let dir = s
      .backups
      .dir
      .clone()
      .unwrap_or_else(|| shared.lithic.paths.backups_dir());
    mods = mods
      .push(widget::field(
        t("settings-backups-keep"),
        pick_list(keeps, Some(s.backups.keep), move |n| {
          change(Change::BackupsKeep(n))
        }),
        None,
      ))
      .push(widget::field(
        t("settings-backups-dir"),
        folder_row(
          &dir,
          Message::PickBackupDir,
          s.backups
            .dir
            .is_some()
            .then_some(change(Change::BackupDir(None))),
          compact_paths,
        ),
        None,
      ));
  }

  let game_dir = s
    .game
    .install_dir
    .clone()
    .unwrap_or_else(|| shared.lithic.paths.game_dir());
  let storage = column![
    widget::field(
      t("settings-game-dir"),
      folder_row(
        &game_dir,
        Message::PickGameDir,
        s.game
          .install_dir
          .is_some()
          .then_some(change(Change::GameDir(None))),
        compact_paths,
      ),
      Some(t("settings-game-dir-hint")),
    ),
    path_row(
      t("settings-path-config"),
      shared.lithic.paths.config.clone(),
      compact_paths
    ),
    path_row(
      t("settings-path-data"),
      shared.lithic.paths.data.clone(),
      compact_paths
    ),
    path_row(
      t("settings-path-cache"),
      shared.lithic.paths.cache.clone(),
      compact_paths
    ),
  ]
  .spacing(12);

  let appearance = section(t("settings-appearance"), appearance);
  let mods = section(t("settings-mods"), mods);
  let storage = section(t("settings-storage"), storage);

  if width >= 1050.0 {
    row![
      column![appearance, mods].spacing(16).width(Fill),
      column![storage].width(Fill),
    ]
    .spacing(16)
    .into()
  } else {
    column![appearance, mods, storage].spacing(16).into()
  }
}

fn section<'a>(
  title: String,
  content: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
  widget::card(
    column![text(title).size(16).font(widget::bold()), content.into()]
      .spacing(14),
  )
  .into()
}

fn folder_row<'a>(
  path: &Path,
  pick: Message,
  reset: Option<Message>,
  compact: bool,
) -> Element<'a, Message> {
  let path = text(path.display().to_string())
    .size(13)
    .wrapping(text::Wrapping::WordOrGlyph)
    .width(Fill);
  let change = button(text(t("common-change")).size(13))
    .style(button::secondary)
    .on_press(pick);
  let mut actions = row![change].spacing(8).align_y(Center);
  if let Some(reset) = reset {
    actions = actions.push(
      button(text(t("common-reset")).size(13))
        .style(button::text)
        .on_press(reset),
    );
  }
  if compact {
    column![path, actions].spacing(8).into()
  } else {
    row![path, actions].spacing(8).align_y(Center).into()
  }
}

fn path_row<'a>(
  label: String,
  path: PathBuf,
  compact: bool,
) -> Element<'a, Message> {
  let value = text(path.display().to_string())
    .size(13)
    .style(style::muted)
    .wrapping(text::Wrapping::WordOrGlyph)
    .width(Fill);
  let open = button(text(t("common-open")).size(13))
    .style(button::text)
    .on_press(Message::Open(path));
  if compact {
    column![
      row![
        text(label)
          .size(13)
          .wrapping(text::Wrapping::WordOrGlyph)
          .width(Fill),
        open
      ]
      .spacing(8)
      .align_y(Center),
      value,
    ]
    .spacing(4)
    .into()
  } else {
    row![text(label).size(13).width(160), value, open]
      .spacing(8)
      .align_y(Center)
      .into()
  }
}
