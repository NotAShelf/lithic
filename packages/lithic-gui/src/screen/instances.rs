use std::{fmt, path::PathBuf};

use iced::{
  Center,
  Element,
  Fill,
  Task,
  widget::{button, column, row, scrollable, text},
};
use lithic_core::{
  Instance,
  game::{launchable, needs_download},
  instance::NewInstance,
  mods::InstallOptions,
  paths::stock_game_data_dirs,
};

use super::{format_duration, format_time};
use crate::{
  app::{Message as AppMessage, OpKind, Outcome, Page, Shared, Summary},
  i18n::{t, t1, t2},
  icon::Icon,
  style::{self, Kind, Tone},
  task::{blocking, pick_folder, pick_pack},
  widget,
};

/// A game version offered when creating an instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameChoice {
  pub version:   String,
  pub installed: bool,
}

impl fmt::Display for GameChoice {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    if self.installed {
      f.write_str(&self.version)
    } else {
      f.write_str(&t1(
        "instances-version-not-installed",
        "version",
        self.version.clone(),
      ))
    }
  }
}

/// Installed versions first, then the stable releases that can be installed.
pub fn game_choices(shared: &Shared) -> Vec<GameChoice> {
  let mut out: Vec<GameChoice> = shared
    .installs
    .iter()
    .map(|i| {
      GameChoice {
        version:   i.version.clone(),
        installed: i.is_available(),
      }
    })
    .collect();
  if let Some(manifest) = &shared.manifest {
    out.extend(
      manifest
        .releases
        .iter()
        .filter(|r| !r.is_prerelease())
        .filter(|r| !shared.installs.iter().any(|i| i.version == r.version))
        .take(12)
        .map(|r| {
          GameChoice {
            version:   r.version.clone(),
            installed: false,
          }
        }),
    );
  }
  out
}

#[derive(Debug, Default)]
struct CreateForm {
  name:       String,
  game:       Option<GameChoice>,
  data_dir:   Option<PathBuf>,
  submitting: bool,
  error:      Option<String>,
}

#[derive(Debug, Default)]
pub struct State {
  create: Option<CreateForm>,
}

#[derive(Debug, Clone)]
pub enum Message {
  OpenCreate,
  OpenAdopt(PathBuf),
  CloseCreate,
  Name(String),
  Game(GameChoice),
  PickDataDir,
  DataDir(Option<PathBuf>),
  Create,
  Created(Result<Box<Instance>, String>),
  Open(String),
  Launch(String),
  Stop(String),
  Import,
  ImportPicked(PathBuf),
}

impl State {
  pub fn update(
    &mut self,
    message: Message,
    shared: &mut Shared,
  ) -> Task<AppMessage> {
    match message {
      Message::OpenCreate => {
        let game = game_choices(shared).into_iter().next();
        self.create = Some(CreateForm {
          game,
          ..CreateForm::default()
        });
      },
      Message::OpenAdopt(dir) => {
        self.create = Some(CreateForm {
          name: "Vintage Story".to_string(),
          game: game_choices(shared).into_iter().find(|c| c.installed),
          data_dir: Some(dir),
          ..CreateForm::default()
        });
      },
      Message::CloseCreate => self.create = None,
      Message::Name(v) => {
        if let Some(f) = &mut self.create {
          f.name = v;
        }
      },
      Message::Game(g) => {
        if let Some(f) = &mut self.create {
          f.game = Some(g);
        }
      },
      Message::PickDataDir => {
        return Task::future(pick_folder(t("instances-pick-data"))).and_then(
          |d| Task::done(AppMessage::Instances(Message::DataDir(Some(d)))),
        );
      },
      Message::DataDir(d) => {
        if let Some(f) = &mut self.create {
          f.data_dir = d;
        }
      },
      Message::Create => {
        let Some(form) = &mut self.create else {
          return Task::none();
        };
        if form.submitting || form.name.trim().is_empty() {
          return Task::none();
        }
        form.submitting = true;
        form.error = None;
        let lithic = shared.lithic.clone();
        let new = NewInstance {
          name: form.name.trim().to_string(),
          game_version: form.game.as_ref().map(|g| g.version.clone()),
          data_dir: form.data_dir.clone(),
          ..NewInstance::default()
        };
        return Task::perform(
          blocking(move || {
            let instance = lithic.create_instance(new)?;
            if lithic.settings()?.active_instance.is_none() {
              lithic.set_active_instance(Some(&instance.id))?;
            }
            Ok(Box::new(instance))
          }),
          |r| AppMessage::Instances(Message::Created(r)),
        );
      },
      Message::Created(Ok(instance)) => {
        let needs_game = self
          .create
          .as_ref()
          .and_then(|f| f.game.as_ref())
          .filter(|g| !g.installed)
          .map(|g| g.version.clone());
        self.create = None;
        let mut tasks = vec![
          Task::done(AppMessage::Reload),
          Task::done(AppMessage::Navigate(Page::Instance(instance.id))),
        ];
        if let Some(version) = needs_game {
          tasks.push(shared.start_op(
            super::game::busy_key(&version),
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
          ));
        }
        return Task::batch(tasks);
      },
      Message::Created(Err(e)) => {
        if let Some(f) = &mut self.create {
          f.submitting = false;
          f.error = Some(e);
        }
      },
      Message::Open(id) => {
        return Task::done(AppMessage::Navigate(Page::Instance(id)));
      },
      Message::Launch(id) => return Task::done(AppMessage::Launch(id)),
      Message::Stop(id) => return Task::done(AppMessage::Stop(id)),
      Message::Import => {
        return Task::future(pick_pack(t("instances-import-pick"))).and_then(
          |p| Task::done(AppMessage::Instances(Message::ImportPicked(p))),
        );
      },
      Message::ImportPicked(path) => {
        let key = format!("import:{}", path.display());
        return shared.start_op(
          key,
          OpKind::Import,
          move |lithic, reporter, cancel| {
            async move {
              let opts = InstallOptions {
                reporter,
                cancel,
                ..InstallOptions::default()
              };
              lithic
                .import_pack(&path, None, &opts)
                .await
                .map(|r| {
                  Outcome::Imported {
                    instance: r.instance.id,
                    summary:  Summary::from(&r.install),
                  }
                })
                .map_err(|e| e.to_string())
            }
          },
        );
      },
    }
    Task::none()
  }

  pub fn view<'a>(&'a self, shared: &'a Shared) -> Element<'a, Message> {
    let actions = row![
      widget::secondary_icon(
        Icon::Import,
        t("instances-import"),
        Some(Message::Import)
      ),
      widget::primary_icon(
        Icon::Plus,
        t("instances-new"),
        Some(Message::OpenCreate)
      ),
    ]
    .spacing(8)
    .align_y(Center);

    let importing: Vec<Element<Message>> = shared
      .busy
      .iter()
      .filter(|(k, _)| k.starts_with("import:"))
      .map(|(_, b)| {
        widget::notice(
          text(format!("{} {}", t("instances-importing"), b.label())),
          Tone::Neutral,
        )
      })
      .collect();

    let mut list = column(importing).spacing(10);
    for (id, error) in &shared.broken {
      list = list.push(widget::notice(
        text(t2(
          "instances-broken",
          "id",
          id.clone(),
          "error",
          error.clone(),
        )),
        Tone::Bad,
      ));
    }

    let content: Element<Message> = if !shared.loaded {
      widget::loading(t("loading"))
    } else if shared.instances.is_empty() {
      let adopt = stock_game_data_dirs().into_iter().next();
      let mut buttons = row![widget::primary_icon(
        Icon::Plus,
        t("instances-new"),
        Some(Message::OpenCreate)
      )]
      .spacing(8);
      if let Some(dir) = adopt {
        buttons = buttons.push(widget::secondary(
          t("instances-adopt"),
          Some(Message::OpenAdopt(dir)),
        ));
      }
      widget::empty(
        t("instances-empty-title"),
        t("instances-empty-body"),
        Some(buttons.into()),
      )
    } else {
      for instance in &shared.instances {
        list = list.push(instance_row(instance, shared));
      }
      scrollable(list).height(Fill).into()
    };

    let page = widget::page(t("nav-instances"), actions, content);
    match &self.create {
      Some(form) => {
        widget::modal(page, create_dialog(form, shared), Message::CloseCreate)
      },
      None => page,
    }
  }
}

fn instance_row<'a>(
  instance: &'a Instance,
  shared: &'a Shared,
) -> Element<'a, Message> {
  let id = instance.id.clone();
  let running = shared.running.contains_key(&id);
  let launching = shared.launching.contains(&id);
  let busy = shared.busy.get(&id);
  let selected =
    shared.settings.active_instance.as_deref() == Some(id.as_str());
  let mods = shared.mod_counts.get(&id).copied().unwrap_or(0);

  let mut details = vec![instance.game_version.as_ref().map_or_else(
    || t("instances-no-game"),
    |v| t1("instances-game", "version", v.clone()),
  )];
  details.push(t1("mods-count", "count", mods));
  details.push(instance.stats.last_played_at.map_or_else(
    || t("instances-never-played"),
    |ms| {
      t2(
        "instances-played",
        "when",
        format_time(ms),
        "total",
        format_duration(instance.stats.play_time_ms),
      )
    },
  ));

  let mut title = row![text(&instance.name).size(17).font(widget::bold())]
    .spacing(8)
    .align_y(Center);
  if selected {
    title = title.push(widget::badge(t("instances-selected"), Tone::Accent));
  }
  if running {
    title = title.push(widget::badge(t("instances-running"), Tone::Good));
  }
  if let Some(b) = busy {
    title = title.push(widget::badge(b.label(), Tone::Warn));
  }
  let missing_game = instance.game_version.as_deref().is_some_and(|v| {
    !shared
      .installs
      .iter()
      .any(|i| i.version == v && i.is_available())
  });
  if let Some(v) = instance.game_version.as_deref()
    && missing_game
    && busy.is_none()
  {
    let released = shared
      .manifest
      .as_ref()
      .is_none_or(|m| m.release(v).is_some());
    title = title.push(match (needs_download(&shared.installs, v), released) {
      (false, _) => widget::badge(t("games-missing"), Tone::Bad),
      (true, true) => widget::badge(t("instances-game-missing"), Tone::Warn),
      (true, false) => {
        widget::badge(t("instances-game-unavailable"), Tone::Bad)
      },
    });
  }

  let play = if running {
    widget::btn(
      Some(Icon::Stop),
      t("instances-stop"),
      Kind::Danger,
      Some(Message::Stop(id.clone())),
    )
  } else if launching || busy.is_some() {
    widget::btn(
      Some(Icon::Play),
      t("instances-starting"),
      Kind::Secondary,
      None,
    )
  } else {
    widget::secondary_icon(
      Icon::Play,
      t("instances-play"),
      instance
        .game_version
        .as_deref()
        .is_some_and(|v| launchable(&shared.installs, v))
        .then(|| Message::Launch(id.clone())),
    )
  };

  button(
    row![
      column![
        title,
        text(details.join("  |  ")).size(13).style(style::muted)
      ]
      .spacing(4)
      .width(Fill),
      play,
    ]
    .spacing(12)
    .align_y(Center),
  )
  .padding([10, 14])
  .width(Fill)
  .style(style::row_card)
  .on_press(Message::Open(id))
  .into()
}

fn create_dialog<'a>(
  form: &'a CreateForm,
  shared: &'a Shared,
) -> Element<'a, Message> {
  let choices = game_choices(shared);
  let data_label = form
    .data_dir
    .as_ref()
    .map_or_else(|| t("instances-data-default"), |d| d.display().to_string());
  let mut data_row = row![
    text(data_label).size(13).width(Fill),
    widget::secondary_icon(
      Icon::Folder,
      t("common-choose-folder"),
      Some(Message::PickDataDir)
    ),
  ]
  .spacing(8)
  .align_y(Center);
  if form.data_dir.is_some() {
    data_row = data_row.push(widget::icon_button(
      Icon::Close,
      t("common-reset"),
      Some(Message::DataDir(None)),
    ));
  }

  let game_hint = match &form.game {
    Some(g) if !g.installed => {
      Some(t1("instances-will-install", "version", g.version.clone()))
    },
    None if shared.manifest.is_none() => {
      Some(t("instances-releases-unavailable"))
    },
    _ => None,
  };

  let mut body = column![
    widget::field(
      t("instances-name"),
      widget::input(&t("instances-name-placeholder"), &form.name)
        .on_input(Message::Name)
        .on_submit(Message::Create)
        .padding(8),
      None
    ),
    widget::field(
      t("instances-game-version"),
      widget::select(choices, form.game.clone(), Message::Game)
        .placeholder(t("instances-pick-game")),
      game_hint
    ),
    widget::field(
      t("instances-data-folder"),
      data_row,
      Some(t("instances-data-hint"))
    ),
  ]
  .spacing(14);
  if let Some(error) = &form.error {
    body = body.push(widget::notice(text(error.clone()), Tone::Bad));
  }

  widget::dialog(
    t("instances-create-title"),
    body,
    row![
      widget::ghost(t("common-cancel"), Some(Message::CloseCreate)),
      widget::action(
        t("instances-create"),
        t("instances-creating"),
        form.submitting,
        (!form.name.trim().is_empty()).then_some(Message::Create)
      ),
    ]
    .spacing(8),
    520.0,
  )
}

#[cfg(test)]
#[expect(
  clippy::unwrap_used,
  reason = "test setup and assertions intentionally fail on error"
)]
mod tests {
  use std::fs;

  use lithic_core::{
    Lithic,
    Paths,
    game::{Install, Manifest},
  };

  use super::*;

  fn shared() -> (tempfile::TempDir, Shared) {
    let dir = tempfile::tempdir().unwrap();
    let lithic = Lithic::new(Paths::rooted(dir.path())).unwrap();
    (dir, Shared::new(lithic))
  }

  #[test]
  fn create_form_needs_a_name_and_blocks_double_submit() {
    let (_d, mut shared) = shared();
    let mut s = State::default();
    let _ = s.update(Message::OpenCreate, &mut shared);
    let _ = s.update(Message::Create, &mut shared);
    assert!(
      !s.create.as_ref().unwrap().submitting,
      "empty names are not submitted"
    );
    let _ = s.update(Message::Name("Pack".into()), &mut shared);
    let _ = s.update(Message::Create, &mut shared);
    assert!(s.create.as_ref().unwrap().submitting);
    let _ = s.update(Message::Created(Err("boom".into())), &mut shared);
    let form = s.create.as_ref().unwrap();
    assert!(!form.submitting);
    assert_eq!(form.error.as_deref(), Some("boom"));
  }

  #[test]
  fn installed_versions_come_first_and_missing_executables_are_unavailable() {
    let (d, mut shared) = shared();
    let game = d.path().join("game");
    fs::create_dir(&game).unwrap();
    let executable = game.join(if cfg!(windows) {
      "Vintagestory.exe"
    } else {
      "Vintagestory"
    });
    fs::write(&executable, "").unwrap();
    shared.installs.push(Install {
      version: "1.21.5".into(),
      path:    game,
      managed: true,
    });
    let manifest = Manifest::parse(
         r#"{"1.22.7": {"linux": {"filename": "a", "urls": {"cdn": "u"}}},
             "1.21.5": {"linux": {"filename": "b", "urls": {"cdn": "u"}}},
             "1.22.8-rc.1": {"linux": {"filename": "c", "urls": {"cdn": "u"}}}}"#,
      )
      .unwrap();
    shared.manifest = Some(manifest);
    let choices = game_choices(&shared);
    let versions: Vec<(&str, bool)> = choices
      .iter()
      .map(|c| (c.version.as_str(), c.installed))
      .collect();
    assert_eq!(versions, [("1.21.5", true), ("1.22.7", false)]);

    fs::remove_file(executable).unwrap();
    let choices = game_choices(&shared);
    assert_eq!(choices[0].version, "1.21.5");
    assert!(!choices[0].installed);
  }
}
