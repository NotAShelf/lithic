use std::{
  collections::{HashMap, HashSet},
  fmt::Write as _,
  path::PathBuf,
  pin::pin,
  time::{Duration, Instant},
};

use futures::{
  SinkExt,
  StreamExt,
  channel::mpsc::{Sender, unbounded},
};
use iced::{
  Element,
  Fill,
  Subscription,
  Task,
  Theme,
  stream,
  time,
  widget::{
    button,
    column,
    container,
    pick_list,
    row,
    scrollable,
    space,
    stack,
    text,
  },
  window,
};
use lithic_core::{
  Cancel,
  Error,
  Event,
  Freshness,
  Instance,
  Lithic,
  Reporter,
  Result as CoreResult,
  Settings,
  Step,
  auth::Accounts,
  game::{Install, Manifest},
  launch::{Exit, Session},
  mods::{Change, InstallOptions, ModRef, Report},
};
use native_theme_iced::from_system;
use tokio::task::spawn_blocking;

use crate::{
  i18n::{t, t1, t2},
  notify::{self, Notifications},
  screen::{accounts, browse, game, instance, instances, settings},
  style,
  task::{blocking, open},
  theme,
  widget,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Page {
  Instances,
  Instance(String),
  Browse,
  Games,
  Accounts,
  Settings,
}

impl Page {
  pub fn from_setting(value: &str) -> Self {
    match value {
      "browse" => Self::Browse,
      "games" | "game_versions" => Self::Games,
      "accounts" => Self::Accounts,
      "settings" => Self::Settings,
      _ => Self::Instances,
    }
  }
}

/// A long-running operation, keyed by instance id or `game:<version>`.
#[derive(Debug, Clone)]
pub struct Busy {
  pub kind:     OpKind,
  pub step:     Option<Step>,
  pub transfer: Option<(String, u64, Option<u64>)>,
  pub cancel:   Cancel,
}

impl Busy {
  /// How far the current transfer is, when its size is known.
  #[expect(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    reason = "progress bars display an approximate fraction of byte counts"
  )]
  pub fn fraction(&self) -> Option<f32> {
    let (_, done, total) = self.transfer.as_ref()?;
    let total = (*total)?;
    (total > 0).then(|| (*done as f64 / total as f64) as f32)
  }

  pub fn label(&self) -> String {
    match self.step {
      Some(Step::Resolving) => t("step-resolving"),
      Some(Step::Downloading) => {
        match &self.transfer {
          Some((label, ..)) => {
            t1("step-downloading-item", "item", label.clone())
          },
          None => t("step-downloading"),
        }
      },
      Some(Step::Verifying) => t("step-verifying"),
      Some(Step::Extracting) => t("step-extracting"),
      Some(Step::Installing) => t("step-installing"),
      Some(Step::Cleaning) => t("step-cleaning"),
      None => t("step-starting"),
    }
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpKind {
  Install,
  Update,
  Import,
  Export,
  GameInstall,
}

/// A cloneable digest of a core [`Report`].
#[derive(Debug, Clone, Default)]
pub struct Summary {
  pub changes:   Vec<Change>,
  pub unchanged: Vec<String>,
  pub failures:  Vec<(String, Option<String>, String)>,
}

impl From<&Report> for Summary {
  fn from(r: &Report) -> Self {
    Self {
      changes:   r.changes.clone(),
      unchanged: r.unchanged.clone(),
      failures:  r
        .failures
        .iter()
        .map(|f| (f.mod_id.clone(), f.needed_by.clone(), f.error.to_string()))
        .collect(),
    }
  }
}

#[derive(Debug, Clone)]
pub enum Outcome {
  Mods(Summary),
  Imported { instance: String, summary: Summary },
  GameInstalled(String),
  Exported(PathBuf),
}

/// State every screen can read and change.
pub struct Shared {
  pub lithic:     Lithic,
  pub settings:   Settings,
  pub instances:  Vec<Instance>,
  pub broken:     Vec<(String, String)>,
  pub mod_counts: HashMap<String, usize>,
  pub installs:   Vec<Install>,
  pub accounts:   Accounts,
  /// Accounts with a stored session. Checked while loading, since asking
  /// the keyring can be slow.
  pub sessions:   HashSet<String>,
  pub manifest:   Option<Manifest>,
  pub running:    HashMap<String, Session>,
  pub launching:  HashSet<String>,
  pub busy:       HashMap<String, Busy>,
  pub toasts:     Notifications,
  pub logos:      HashMap<String, browse::Logo>,
  pub loaded:     bool,
}

impl Shared {
  pub fn new(lithic: Lithic) -> Self {
    Self {
      settings: lithic.settings().unwrap_or_default(),
      lithic,
      instances: Vec::new(),
      broken: Vec::new(),
      mod_counts: HashMap::new(),
      installs: Vec::new(),
      accounts: Accounts::default(),
      sessions: HashSet::new(),
      manifest: None,
      running: HashMap::new(),
      launching: HashSet::new(),
      busy: HashMap::new(),
      toasts: Notifications::default(),
      logos: HashMap::new(),
      loaded: false,
    }
  }

  pub fn instance(&self, id: &str) -> Option<&Instance> {
    self.instances.iter().find(|i| i.id == id)
  }

  pub fn is_running(&self, id: &str) -> bool {
    self.running.contains_key(id) || self.launching.contains(id)
  }

  /// The instance mods go into unless the user picks another: the selected
  /// one, else the first.
  pub fn default_instance(&self) -> Option<&Instance> {
    self
      .settings
      .active_instance
      .as_deref()
      .and_then(|id| self.instance(id))
      .or_else(|| self.instances.first())
  }

  /// Starts a background operation that reports progress into `busy[key]`
  /// and ends with [`Message::OpDone`]. Refuses while `key` is busy.
  pub fn start_op<F, Fut>(
    &mut self,
    key: String,
    kind: OpKind,
    work: F,
  ) -> Task<Message>
  where
    F: FnOnce(Lithic, Reporter, Cancel) -> Fut + Send + 'static,
    Fut: Future<Output = Result<Outcome, String>> + Send + 'static,
  {
    if self.busy.contains_key(&key) {
      return self.toasts.warning(t("op-already-running"));
    }
    let cancel = Cancel::new();
    self.busy.insert(key.clone(), Busy {
      kind,
      step: None,
      transfer: None,
      cancel: cancel.clone(),
    });
    let lithic = self.lithic.clone();
    Task::stream(stream::channel(
      32,
      async move |mut out: Sender<Message>| {
        let (tx, mut rx) = unbounded::<Event>();
        let reporter = Reporter::new(move |e| {
          let _ = tx.unbounded_send(e);
        });
        let mut work = pin!(work(lithic, reporter, cancel));
        let mut last_transfer: Option<Instant> = None;
        let result = loop {
          tokio::select! {
             result = &mut work => break result,
             Some(event) = rx.next() => {
                let partial = matches!(&event, Event::Transfer { done, total, .. } if Some(*done) != *total);
                if partial && last_transfer.is_some_and(|last| last.elapsed() < Duration::from_millis(120)) {
                   continue;
                }
                if partial {
                   last_transfer = Some(Instant::now());
                }
                let _ = out.send(Message::OpEvent(key.clone(), event)).await;
             }
          }
        };
        let _ = out.send(Message::OpDone(key, result)).await;
      },
    ))
  }
}

#[derive(Debug, Clone)]
pub struct Snapshot {
  settings:   Settings,
  instances:  Vec<Instance>,
  broken:     Vec<(String, String)>,
  mod_counts: HashMap<String, usize>,
  installs:   Vec<Install>,
  accounts:   Accounts,
  sessions:   HashSet<String>,
}

fn load_snapshot(lithic: &Lithic) -> CoreResult<Snapshot> {
  let listing = lithic.list_instances()?;
  let mod_counts = listing
    .instances
    .iter()
    .map(|i| {
      (
        i.id.clone(),
        lithic.installed_mods(i).map_or(0, |m| m.len()),
      )
    })
    .collect();
  let accounts = lithic.accounts()?;
  let sessions = accounts
    .accounts
    .iter()
    .filter(|a| lithic.has_session(&a.uid))
    .map(|a| a.uid.clone())
    .collect();
  Ok(Snapshot {
    sessions,
    settings: lithic.settings()?,
    broken: listing
      .broken
      .iter()
      .map(|(id, e)| (id.clone(), e.to_string()))
      .collect(),
    instances: listing.instances,
    mod_counts,
    installs: lithic.game_installs()?,
    accounts,
  })
}

#[derive(Debug, Clone)]
enum Dialog {
  Migrated {
    notes:  Vec<String>,
    backup: PathBuf,
  },
  MigrationFailed(String),
  Quit,
  /// A `vintagestorymodinstall://` link lithic was started with.
  Link(ModRef),
}

#[derive(Debug, Clone)]
pub enum Message {
  Navigate(Page),
  /// Opens Browse with mods going into this instance.
  BrowseFor(String),
  Reload,
  Loaded(Result<Box<Snapshot>, String>),
  ReloadManifest,
  ManifestLoaded(Result<Manifest, String>),
  CheckSystemTheme,
  SystemTheme(Option<Theme>),

  Launch(String),
  GameStarted(String, Session),
  GameFailed(String, String),
  GameExited(String, Result<Exit, String>),
  Stop(String),

  OpEvent(String, Event),
  OpDone(String, Result<Outcome, String>),
  CancelOp(String),

  Toast(notify::Message),
  Open(String),
  CloseRequested(window::Id),
  ConfirmQuit,
  CloseDialog,
  LinkTarget(browse::TargetChoice),
  LinkInstall,

  Instances(instances::Message),
  Instance(instance::Message),
  Browse(browse::Message),
  Games(game::Message),
  Accounts(accounts::Message),
  Settings(settings::Message),
}

pub struct App {
  shared:       Shared,
  page:         Page,
  dialog:       Option<Dialog>,
  /// A link waiting for the current dialog to close.
  queued_link:  Option<ModRef>,
  link_target:  Option<String>,
  system_theme: Option<Theme>,
  /// Resolved once per settings or system change; building a preset theme
  /// is too slow to repeat on every frame.
  theme:        Theme,
  instances:    instances::State,
  instance:     Option<instance::State>,
  browse:       browse::State,
  games:        game::State,
  accounts:     accounts::State,
  settings:     settings::State,
}

impl App {
  pub fn new(lithic: Lithic, link: Option<ModRef>) -> (Self, Task<Message>) {
    let mut dialog = match lithic.migrate() {
      Ok(Some(report)) => {
        Some(Dialog::Migrated {
          notes:  report.notes.iter().map(ToString::to_string).collect(),
          backup: report.backup,
        })
      },
      Ok(None) => None,
      Err(e) => Some(Dialog::MigrationFailed(e.to_string())),
    };
    let mut queued_link = None;
    match (&dialog, link) {
      (None, Some(link)) => dialog = Some(Dialog::Link(link)),
      (Some(_), link) => queued_link = link,
      (None, None) => {},
    }
    let shared = Shared::new(lithic);
    let page = Page::from_setting(&shared.settings.gui.initial_page);
    let mut app = Self {
      page,
      dialog,
      queued_link,
      link_target: None,
      system_theme: None,
      theme: theme::lithic(true),
      instances: instances::State::default(),
      instance: None,
      browse: browse::State::default(),
      games: game::State::default(),
      accounts: accounts::State::default(),
      settings: settings::State::default(),
      shared,
    };
    app.refresh_theme();
    let enter = app.enter(&app.page.clone());
    let tasks = Task::batch([
      Task::done(Message::Reload),
      Task::done(Message::ReloadManifest),
      Task::done(Message::CheckSystemTheme),
      enter,
    ]);
    (app, tasks)
  }

  pub fn title(&self) -> String {
    match &self.page {
      Page::Instance(id) => {
        self.shared.instance(id).map_or_else(
          || t("window-title"),
          |i| t1("window-title-instance", "name", i.name.clone()),
        )
      },
      _ => t("window-title"),
    }
  }

  pub fn theme(&self) -> Theme {
    self.theme.clone()
  }

  fn refresh_theme(&mut self) {
    self.theme = settings::resolve_theme(
      &self.shared.settings.gui,
      self.system_theme.as_ref(),
    );
  }

  /// Work to do when a page is shown.
  fn enter(&self, page: &Page) -> Task<Message> {
    match page {
      Page::Browse => browse::enter(&self.browse, &self.shared),
      Page::Instance(id) => instance::load_mods(&self.shared, id),
      _ => Task::none(),
    }
  }

  pub fn update(&mut self, message: Message) -> Task<Message> {
    match message {
      Message::Navigate(page) => {
        if let Page::Instance(id) = &page
          && self.instance.as_ref().is_none_or(|s| &s.id != id)
        {
          self.instance = Some(instance::State::new(id, &self.shared));
        }
        let task = self.enter(&page);
        self.page = page;
        task
      },
      Message::BrowseFor(id) => {
        self.browse.set_target(Some(id), &self.shared);
        self.page = Page::Browse;
        browse::enter(&self.browse, &self.shared)
      },
      Message::Reload => {
        let lithic = self.shared.lithic.clone();
        Task::perform(
          blocking(move || load_snapshot(&lithic).map(Box::new)),
          Message::Loaded,
        )
      },
      Message::Loaded(Ok(snapshot)) => {
        let s = *snapshot;
        self.shared.settings = s.settings;
        self.shared.instances = s.instances;
        self.shared.broken = s.broken;
        self.shared.mod_counts = s.mod_counts;
        self.shared.installs = s.installs;
        self.shared.accounts = s.accounts;
        self.shared.sessions = s.sessions;
        self.refresh_theme();
        let first_load = !self.shared.loaded;
        self.shared.loaded = true;
        if let Page::Instance(id) = &self.page
          && self.shared.instance(id).is_none()
        {
          self.page = Page::Instances;
        }
        self.browse.sync_target(&self.shared);
        if first_load && self.page == Page::Browse {
          return browse::enter(&self.browse, &self.shared);
        }
        Task::none()
      },
      Message::Loaded(Err(e)) => {
        self.shared.toasts.error(t("load-failed"), Some(e))
      },
      Message::ReloadManifest => {
        let lithic = self.shared.lithic.clone();
        Task::perform(
          async move {
            lithic
              .game_manifest(Freshness::Cached)
              .await
              .map_err(|e| e.to_string())
          },
          Message::ManifestLoaded,
        )
      },
      Message::ManifestLoaded(Ok(manifest)) => {
        self.shared.manifest = Some(manifest);
        Task::none()
      },
      Message::ManifestLoaded(Err(e)) => {
        tracing::warn!("game release list unavailable: {e}");
        Task::none()
      },
      Message::CheckSystemTheme => {
        if self.shared.settings.gui.theme_mode != "system"
          && self.shared.settings.gui.theme_mode != "preset"
        {
          return Task::none();
        }
        Task::perform(
          async {
            spawn_blocking(|| from_system().ok().map(|(theme, ..)| theme))
              .await
              .ok()
              .flatten()
          },
          Message::SystemTheme,
        )
      },
      Message::SystemTheme(theme) => {
        if self.system_theme != theme {
          self.system_theme = theme;
          self.refresh_theme();
        }
        Task::none()
      },

      Message::Launch(id) => self.launch(id),
      Message::GameStarted(id, session) => {
        self.shared.launching.remove(&id);
        self.shared.running.insert(id, session);
        Task::none()
      },
      Message::GameFailed(id, error) => {
        self.shared.launching.remove(&id);
        self.shared.toasts.error(t("launch-failed"), Some(error))
      },
      Message::GameExited(id, result) => {
        self.shared.running.remove(&id);
        self.shared.launching.remove(&id);
        let name = self
          .shared
          .instance(&id)
          .map_or_else(|| id.clone(), |i| i.name.clone());
        let toast = match result {
          Ok(exit) if exit.success || exit.stopped => Task::none(),
          Ok(exit) => {
            let mut detail = exit.tail.join("\n");
            if let Some(crash) = &exit.crash_report {
              let _ = write!(
                detail,
                "\n\n{}",
                t1("launch-crash-report", "path", crash.display().to_string())
              );
            }
            let code =
              exit.code.map_or_else(|| "?".to_string(), |c| c.to_string());
            self.shared.toasts.error(
              t2("launch-exited", "name", name, "code", code),
              Some(detail),
            )
          },
          Err(e) => {
            self
              .shared
              .toasts
              .error(t1("launch-lost", "name", name), Some(e))
          },
        };
        let reload_logs = match (&self.page, &mut self.instance) {
          (Page::Instance(open), Some(state)) if *open == id => {
            state.refresh_logs(&self.shared)
          },
          _ => Task::none(),
        };
        Task::batch([toast, Task::done(Message::Reload), reload_logs])
      },
      Message::Stop(id) => {
        if let Some(session) = self.shared.running.get(&id) {
          session.stop();
        }
        Task::none()
      },

      Message::OpEvent(key, event) => {
        if let Some(busy) = self.shared.busy.get_mut(&key) {
          match event {
            Event::Step(step) => {
              busy.step = Some(step);
              if step != Step::Downloading {
                busy.transfer = None;
              }
            },
            Event::Transfer { label, done, total } => {
              busy.step = Some(Step::Downloading);
              busy.transfer = Some((label, done, total));
            },
            Event::Log(_) => {},
          }
        }
        Task::none()
      },
      Message::OpDone(key, result) => self.finish_op(&key, result),
      Message::CancelOp(key) => {
        if let Some(busy) = self.shared.busy.get(&key) {
          busy.cancel.cancel();
        }
        Task::none()
      },

      Message::Toast(m) => {
        self.shared.toasts.update(m);
        Task::none()
      },
      Message::Open(target) => {
        match open(&target) {
          Ok(()) => Task::none(),
          Err(e) => self.shared.toasts.error(t("open-failed"), Some(e)),
        }
      },
      Message::CloseRequested(id) => {
        if self.shared.running.is_empty()
          && self.shared.launching.is_empty()
          && self.shared.busy.is_empty()
        {
          window::close(id)
        } else {
          self.dialog = Some(Dialog::Quit);
          Task::none()
        }
      },
      Message::ConfirmQuit => iced::exit(),
      Message::CloseDialog => {
        self.dialog = self.queued_link.take().map(Dialog::Link);
        Task::none()
      },
      Message::LinkTarget(choice) => {
        self.link_target = Some(choice.id);
        Task::none()
      },
      Message::LinkInstall => {
        let Some(Dialog::Link(link)) = self.dialog.take() else {
          return Task::none();
        };
        let Some(target) = self
          .link_target
          .clone()
          .or_else(|| self.shared.default_instance().map(|i| i.id.clone()))
        else {
          return Task::none();
        };
        self.link_target = None;
        let open =
          Task::done(Message::Navigate(Page::Instance(target.clone())));
        let pin = link.version.is_some();
        let refs = vec![link];
        let op = self.shared.start_op(
          target.clone(),
          OpKind::Install,
          move |lithic, reporter, cancel| {
            async move {
              let instance =
                lithic.instance(&target).map_err(|e| e.to_string())?;
              let opts = InstallOptions {
                dependencies: true,
                pin_versions: pin,
                reporter,
                cancel,
                ..Default::default()
              };
              lithic
                .install_mods(&instance, &refs, &opts)
                .await
                .map(|r| Outcome::Mods(Summary::from(&r)))
                .map_err(|e| e.to_string())
            }
          },
        );
        Task::batch([op, open])
      },

      Message::Instances(m) => self.instances.update(m, &mut self.shared),
      Message::Instance(m) => {
        match &mut self.instance {
          Some(state) => state.update(m, &mut self.shared),
          None => Task::none(),
        }
      },
      Message::Browse(m) => self.browse.update(m, &mut self.shared),
      Message::Games(m) => self.games.update(m, &mut self.shared),
      Message::Accounts(m) => self.accounts.update(m, &mut self.shared),
      Message::Settings(m) => {
        let task = self.settings.update(m, &mut self.shared);
        self.refresh_theme();
        task
      },
    }
  }

  fn launch(&mut self, id: String) -> Task<Message> {
    if self.shared.is_running(&id) {
      return Task::none();
    }
    self.shared.launching.insert(id.clone());
    let lithic = self.shared.lithic.clone();
    Task::stream(stream::channel(4, async move |mut out: Sender<Message>| {
      let started = async {
        let instance = lithic.instance(&id)?;
        lithic.set_active_instance(Some(&id))?;
        lithic.launch(&instance)
      }
      .await;
      match started {
        Ok((session, waiter)) => {
          let _ = out.send(Message::GameStarted(id.clone(), session)).await;
          let exit = waiter.await.map_err(|e| e.to_string());
          let _ = out.send(Message::GameExited(id, exit)).await;
        },
        Err(e) => {
          let _ = out.send(Message::GameFailed(id, e.to_string())).await;
        },
      }
    }))
  }

  fn finish_op(
    &mut self,
    key: &str,
    result: Result<Outcome, String>,
  ) -> Task<Message> {
    let kind = self.shared.busy.remove(key).map(|b| b.kind);
    let name = self
      .shared
      .instance(key)
      .map_or_else(|| key.to_string(), |i| i.name.clone());
    let mut tasks = vec![Task::done(Message::Reload)];

    match result {
      Ok(Outcome::Mods(summary)) => {
        tasks.push(self.summary_toasts(&summary, &name, kind));
      },
      Ok(Outcome::Imported { instance, summary }) => {
        tasks.push(self.summary_toasts(&summary, &name, kind));
        tasks.push(Task::done(Message::Navigate(Page::Instance(instance))));
      },
      Ok(Outcome::GameInstalled(version)) => {
        tasks.push(self.shared.toasts.success(t1(
          "game-installed",
          "version",
          version,
        )));
      },
      Ok(Outcome::Exported(path)) => {
        tasks.push(self.shared.toasts.success(t1(
          "pack-exported",
          "path",
          path.display().to_string(),
        )));
      },
      Err(e) if e == Error::Cancelled.to_string() => {
        tasks.push(self.shared.toasts.info(t("op-cancelled")));
      },
      Err(e) => {
        tasks.push(
          self
            .shared
            .toasts
            .error(t1("op-failed", "name", name), Some(e)),
        );
      },
    }

    if let (Page::Instance(open), Some(state)) =
      (&self.page, &mut self.instance)
      && *open == key
    {
      tasks.push(state.after_op(&self.shared));
    }
    if self.page == Page::Browse {
      tasks.push(browse::refresh_installed(&self.browse, &self.shared));
    }
    Task::batch(tasks)
  }

  fn summary_toasts(
    &mut self,
    summary: &Summary,
    name: &str,
    kind: Option<OpKind>,
  ) -> Task<Message> {
    let mut tasks = Vec::new();
    if !summary.changes.is_empty() {
      let message = match kind {
        Some(OpKind::Update) => {
          t2(
            "mods-updated",
            "count",
            summary.changes.len(),
            "name",
            name.to_string(),
          )
        },
        _ => {
          t2(
            "mods-installed",
            "count",
            summary.changes.len(),
            "name",
            name.to_string(),
          )
        },
      };
      tasks.push(self.shared.toasts.success(message));
    } else if !summary.unchanged.is_empty() && summary.failures.is_empty() {
      tasks.push(self.shared.toasts.info(t("mods-already-installed")));
    }
    if !summary.failures.is_empty() {
      let detail = summary
        .failures
        .iter()
        .map(|(id, by, e)| {
          by.as_ref().map_or_else(
            || format!("{id}: {e}"),
            |by| {
              format!(
                "{id} ({}): {e}",
                t1("mods-needed-by", "name", by.clone())
              )
            },
          )
        })
        .collect::<Vec<_>>()
        .join("\n");
      tasks.push(self.shared.toasts.error(
        t1("mods-some-failed", "count", summary.failures.len()),
        Some(detail),
      ));
    }
    Task::batch(tasks)
  }

  pub fn view(&self) -> Element<'_, Message> {
    let content: Element<Message> = match &self.page {
      Page::Instances => {
        self.instances.view(&self.shared).map(Message::Instances)
      },
      Page::Instance(_) => {
        self.instance.as_ref().map_or_else(
          || widget::loading(t("loading")),
          |state| state.view(&self.shared).map(Message::Instance),
        )
      },
      Page::Browse => self.browse.view(&self.shared).map(Message::Browse),
      Page::Games => self.games.view(&self.shared).map(Message::Games),
      Page::Accounts => self.accounts.view(&self.shared).map(Message::Accounts),
      Page::Settings => self.settings.view(&self.shared).map(Message::Settings),
    };

    let layout =
      row![self.sidebar(), container(content).width(Fill).height(Fill)];
    let with_toasts: Element<Message> = if self.shared.toasts.is_empty() {
      layout.into()
    } else {
      stack![
        layout,
        container(self.shared.toasts.view().map(Message::Toast))
          .padding(16)
          .width(Fill)
          .height(Fill)
          .align_right(Fill)
          .align_bottom(Fill),
      ]
      .into()
    };

    match &self.dialog {
      None => with_toasts,
      Some(dialog) => {
        widget::modal(
          with_toasts,
          self.dialog_view(dialog),
          Message::CloseDialog,
        )
      },
    }
  }

  fn dialog_view<'a>(&'a self, dialog: &'a Dialog) -> Element<'a, Message> {
    match dialog {
      Dialog::Migrated { notes, backup } => {
        let list =
          column(notes.iter().map(|n| text(format!("- {n}")).size(13).into()))
            .spacing(4);
        widget::dialog(
          t("migrated-title"),
          column![
            text(t("migrated-body")),
            container(scrollable(list).height(240))
              .style(style::card)
              .padding(12),
            text(t1("migrated-backup", "path", backup.display().to_string()))
              .size(12)
              .style(style::muted),
          ]
          .spacing(12),
          button(text(t("common-ok")))
            .padding([8, 16])
            .on_press(Message::CloseDialog),
          620.0,
        )
      },
      Dialog::MigrationFailed(error) => {
        widget::dialog(
          t("migration-failed-title"),
          column![
            text(t("migration-failed-body")),
            text(error).size(13).style(style::muted)
          ]
          .spacing(12),
          button(text(t("common-ok")))
            .padding([8, 16])
            .on_press(Message::CloseDialog),
          560.0,
        )
      },
      Dialog::Quit => {
        widget::confirm(
          t("quit-title"),
          t("quit-body"),
          t("quit-confirm"),
          true,
          Message::ConfirmQuit,
          Message::CloseDialog,
        )
      },
      Dialog::Link(link) => {
        let what = link.version.as_ref().map_or_else(
          || t1("link-mod", "mod", link.id.clone()),
          |v| {
            t2(
              "link-mod-version",
              "mod",
              link.id.clone(),
              "version",
              v.clone(),
            )
          },
        );
        if self.shared.instances.is_empty() {
          return widget::dialog(
            t("link-title"),
            column![
              text(what),
              text(t("link-no-instances")).style(style::muted)
            ]
            .spacing(12),
            button(text(t("common-close")))
              .padding([8, 16])
              .on_press(Message::CloseDialog),
            480.0,
          );
        }
        let choices: Vec<browse::TargetChoice> = self
          .shared
          .instances
          .iter()
          .map(|i| {
            browse::TargetChoice {
              id:   i.id.clone(),
              name: i.name.clone(),
            }
          })
          .collect();
        let wanted = self
          .link_target
          .clone()
          .or_else(|| self.shared.default_instance().map(|i| i.id.clone()));
        let selected = choices
          .iter()
          .find(|c| Some(&c.id) == wanted.as_ref())
          .cloned();
        widget::dialog(
          t("link-title"),
          column![
            text(what),
            widget::field(
              t("link-into"),
              pick_list(choices, selected, Message::LinkTarget),
              None
            ),
          ]
          .spacing(12),
          row![
            widget::secondary(t("common-cancel"), Some(Message::CloseDialog)),
            widget::action(
              t("browse-install"),
              String::new(),
              false,
              Some(Message::LinkInstall)
            ),
          ]
          .spacing(8),
          480.0,
        )
      },
    }
  }

  fn sidebar(&self) -> Element<'_, Message> {
    let current = match self.page {
      Page::Instance(_) => Page::Instances,
      ref p => p.clone(),
    };
    let item = |label: String, page: Page| {
      let active = current == page;
      button(text(label).size(15))
        .width(Fill)
        .padding([8, 12])
        .style(style::nav(active))
        .on_press(Message::Navigate(page))
    };

    let account = self
      .shared
      .accounts
      .active
      .as_deref()
      .and_then(|uid| self.shared.accounts.get(uid))
      .map_or_else(
        || t("sidebar-signed-out"),
        |a| t1("sidebar-signed-in", "name", a.playername.clone()),
      );
    let running = self.shared.running.len();

    let mut col = column![
      text("lithic").size(22).font(widget::bold()),
      space().height(12),
      item(t("nav-instances"), Page::Instances),
      item(t("nav-browse"), Page::Browse),
      item(t("nav-games"), Page::Games),
      item(t("nav-accounts"), Page::Accounts),
      item(t("nav-settings"), Page::Settings),
      space::vertical(),
    ]
    .spacing(4);
    if running > 0 {
      col = col.push(text(t1("sidebar-running", "count", running)).size(12));
    }
    col = col.push(text(account).size(12).style(style::muted));
    col = col.push(
      text(format!("v{}", env!("CARGO_PKG_VERSION")))
        .size(11)
        .style(style::muted),
    );

    container(col.padding(16))
      .width(210)
      .height(Fill)
      .style(style::sidebar)
      .into()
  }

  pub fn subscription(&self) -> Subscription<Message> {
    let mut subs = vec![window::close_requests().map(Message::CloseRequested)];
    if matches!(
      self.shared.settings.gui.theme_mode.as_str(),
      "system" | "preset"
    ) {
      subs.push(
        time::every(Duration::from_secs(30)).map(|_| Message::CheckSystemTheme),
      );
    }
    if let (Page::Instance(id), Some(state)) = (&self.page, &self.instance)
      && self.shared.running.contains_key(id)
      && state.follows_log()
    {
      subs.push(
        time::every(Duration::from_secs(2))
          .map(|_| Message::Instance(instance::Message::RefreshLog)),
      );
    }
    Subscription::batch(subs)
  }
}

pub fn run(lithic: Lithic, link: Option<ModRef>) -> iced::Result {
  iced::application(
    move || App::new(lithic.clone(), link.clone()),
    App::update,
    App::view,
  )
  .title(App::title)
  .theme(App::theme)
  .subscription(App::subscription)
  .exit_on_close_request(false)
  .window_size((1180.0, 760.0))
  .run()
}
