use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;

use iced::widget::{
   button, checkbox, column, pick_list, progress_bar, row, scrollable, space, text, text_editor, text_input,
   toggler,
};
use iced::{Center, Element, Fill, Task};
use lithic_core::launch::read_tail;
use lithic_core::mods::{self, InstallOptions, InstalledMod, ModRef, Problem, Update};
use lithic_core::{Cancel, Instance};

use super::instances::{GameChoice, game_choices};
use super::{describe_problem, format_duration, format_time, installable_fix};
use crate::app::{Message as AppMessage, OpKind, Outcome, Page, Shared, Summary};
use crate::i18n::{t, t1, t2};
use crate::style::{self, Tone};
use crate::task::{blocking, pick_folder};
use crate::widget;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
   Mods,
   Logs,
   Settings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Folder {
   Instance,
   Data,
   Mods,
   Logs,
}

#[derive(Debug)]
enum Confirm {
   RemoveMod {
      mod_id: String,
      name: String,
      dependents: Vec<String>,
   },
   Delete,
   Clone {
      name: String,
      with_saves: bool,
   },
}

#[derive(Debug, Default)]
struct Form {
   name: String,
   game: Option<GameChoice>,
   args: String,
   env: text_editor::Content,
   wrapper: String,
   mods_dir: Option<PathBuf>,
   dirty: bool,
   saving: bool,
   error: Option<String>,
}

impl Form {
   fn from(instance: &Instance, shared: &Shared) -> Self {
      let game = instance.game_version.as_ref().map(|v| GameChoice {
         version: v.clone(),
         installed: shared.installs.iter().any(|i| i.version == *v),
      });
      let env = instance
         .launch
         .env
         .iter()
         .map(|(k, v)| format!("{k}={v}"))
         .collect::<Vec<_>>()
         .join("\n");
      Self {
         name: instance.name.clone(),
         game,
         args: shell_words::join(&instance.launch.args),
         env: text_editor::Content::with_text(&env),
         wrapper: shell_words::join(&instance.launch.wrapper),
         mods_dir: instance.mods_dir.clone(),
         dirty: false,
         saving: false,
         error: None,
      }
   }

   /// Checks the form and turns it into the values to store.
   fn parse(&self) -> Result<Parsed, String> {
      if self.name.trim().is_empty() {
         return Err(t("instance-name-required"));
      }
      let args =
         shell_words::split(&self.args).map_err(|e| t1("instance-bad-args", "error", e.to_string()))?;
      let wrapper =
         shell_words::split(&self.wrapper).map_err(|e| t1("instance-bad-wrapper", "error", e.to_string()))?;
      let mut env = BTreeMap::new();
      for line in self.env.text().lines().map(str::trim).filter(|l| !l.is_empty()) {
         let Some((k, v)) = line.split_once('=') else {
            return Err(t1("instance-bad-env", "line", line.to_string()));
         };
         env.insert(k.trim().to_string(), v.to_string());
      }
      Ok(Parsed {
         name: self.name.trim().to_string(),
         game: self.game.as_ref().map(|g| g.version.clone()),
         args,
         env,
         wrapper,
         mods_dir: self.mods_dir.clone(),
      })
   }
}

struct Parsed {
   name: String,
   game: Option<String>,
   args: Vec<String>,
   env: BTreeMap<String, String>,
   wrapper: Vec<String>,
   mods_dir: Option<PathBuf>,
}

#[derive(Debug, Default)]
struct Logs {
   files: Vec<PathBuf>,
   selected: Option<PathBuf>,
   content: text_editor::Content,
   follow: bool,
}

#[derive(Debug)]
pub struct State {
   pub id: String,
   tab: Tab,
   mods: Option<Result<Vec<InstalledMod>, String>>,
   problems: Vec<Problem>,
   filter: String,
   install_input: String,
   updates: Option<Vec<Update>>,
   checking: bool,
   confirm: Option<Confirm>,
   form: Form,
   logs: Logs,
}

#[derive(Debug, Clone)]
pub enum Message {
   Back,
   Tab(Tab),
   ModsLoaded(Result<Vec<InstalledMod>, String>),
   Filter(String),
   InstallInput(String),
   InstallRequested,
   Toggle(String, bool),
   Changed(Result<(), String>),
   CheckUpdates,
   UpdatesChecked(Result<Vec<Update>, String>),
   UpdateOne(String),
   UpdateAll,
   Install(String),
   AskRemove(String),
   Pin(String, Option<String>),
   Launch,
   Stop,
   Select,
   Open(Folder),
   Cancel,

   AskDelete,
   AskClone,
   CloneName(String),
   CloneSaves(bool),
   CloseConfirm,
   Confirmed,
   Removed(Result<Vec<String>, String>),
   Deleted(Result<(), String>),
   Cloned(Result<Instance, String>),

   FormName(String),
   FormGame(GameChoice),
   FormArgs(String),
   FormEnv(text_editor::Action),
   FormWrapper(String),
   FormPickMods,
   FormModsDir(Option<PathBuf>),
   Save,
   Saved(Result<(), String>),
   Revert,

   SelectLog(PathBuf),
   LogLoaded(Result<(Vec<PathBuf>, Option<PathBuf>, String), String>),
   RefreshLog,
   LogAction(text_editor::Action),
   Follow(bool),
}

pub fn load_mods(shared: &Shared, id: &str) -> Task<AppMessage> {
   let lithic = shared.lithic.clone();
   let id = id.to_string();
   Task::perform(
      blocking(move || {
         let instance = lithic.instance(&id)?;
         lithic.installed_mods(&instance)
      }),
      |r| AppMessage::Instance(Message::ModsLoaded(r)),
   )
}

impl State {
   pub fn new(id: &str, shared: &Shared) -> Self {
      let form = shared
         .instance(id)
         .map(|i| Form::from(i, shared))
         .unwrap_or_default();
      Self {
         id: id.to_string(),
         tab: Tab::Mods,
         mods: None,
         problems: Vec::new(),
         filter: String::new(),
         install_input: String::new(),
         updates: None,
         checking: false,
         confirm: None,
         form,
         logs: Logs {
            follow: true,
            ..Logs::default()
         },
      }
   }

   pub fn follows_log(&self) -> bool {
      self.tab == Tab::Logs && self.logs.follow
   }

   /// Called when a background operation on this instance finishes.
   pub fn after_op(&mut self, shared: &Shared) -> Task<AppMessage> {
      self.updates = None;
      load_mods(shared, &self.id)
   }

   pub fn refresh_logs(&self, shared: &Shared) -> Task<AppMessage> {
      let lithic = shared.lithic.clone();
      let id = self.id.clone();
      let wanted = self.logs.selected.clone();
      Task::perform(
         blocking(move || {
            let instance = lithic.instance(&id)?;
            let files = lithic.log_files(&instance);
            let selected = wanted
               .filter(|w| files.contains(w))
               .or_else(|| files.first().cloned());
            let text = match &selected {
               Some(f) => read_tail(f, 512 * 1024)?,
               None => String::new(),
            };
            Ok((files, selected, text))
         }),
         |r| AppMessage::Instance(Message::LogLoaded(r)),
      )
   }

   fn start_install(&self, shared: &mut Shared, refs: Vec<ModRef>, kind: OpKind) -> Task<AppMessage> {
      let id = self.id.clone();
      shared.start_op(id.clone(), kind, move |lithic, reporter, cancel| async move {
         let instance = lithic.instance(&id).map_err(|e| e.to_string())?;
         let opts = InstallOptions {
            dependencies: true,
            pin_versions: true,
            reporter,
            cancel,
            ..InstallOptions::default()
         };
         lithic
            .install_mods(&instance, &refs, &opts)
            .await
            .map(|r| Outcome::Mods(Summary::from(&r)))
            .map_err(|e| e.to_string())
      })
   }

   fn start_update(&self, shared: &mut Shared, updates: Vec<Update>) -> Task<AppMessage> {
      let id = self.id.clone();
      shared.start_op(
         id.clone(),
         OpKind::Update,
         move |lithic, reporter, cancel| async move {
            let instance = lithic.instance(&id).map_err(|e| e.to_string())?;
            let opts = InstallOptions {
               dependencies: true,
               reporter,
               cancel,
               ..InstallOptions::default()
            };
            lithic
               .update_mods(&instance, &updates, &opts)
               .await
               .map(|r| Outcome::Mods(Summary::from(&r)))
               .map_err(|e| e.to_string())
         },
      )
   }

   fn blocking_change<F>(&self, shared: &Shared, f: F) -> Task<AppMessage>
   where
      F: FnOnce(&lithic_core::Lithic, &Instance) -> lithic_core::Result<()> + Send + 'static,
   {
      let lithic = shared.lithic.clone();
      let id = self.id.clone();
      Task::perform(
         blocking(move || {
            let instance = lithic.instance(&id)?;
            f(&lithic, &instance)
         }),
         |r| AppMessage::Instance(Message::Changed(r)),
      )
   }

   pub fn update(&mut self, message: Message, shared: &mut Shared) -> Task<AppMessage> {
      match message {
         Message::Back => return Task::done(AppMessage::Navigate(Page::Instances)),
         Message::Tab(tab) => {
            self.tab = tab;
            if tab == Tab::Logs {
               return self.refresh_logs(shared);
            }
            if tab == Tab::Settings
               && !self.form.dirty
               && let Some(i) = shared.instance(&self.id)
            {
               self.form = Form::from(i, shared);
            }
         }
         Message::ModsLoaded(result) => {
            self.problems = result.as_ref().map(|m| mods::problems(m)).unwrap_or_default();
            self.mods = Some(result);
         }
         Message::Filter(f) => self.filter = f,
         Message::Toggle(mod_id, enabled) => {
            return self.blocking_change(shared, move |l, i| l.set_mod_enabled(i, &mod_id, enabled));
         }
         Message::Changed(Ok(())) => return load_mods(shared, &self.id),
         Message::Changed(Err(e)) => {
            return Task::batch([
               shared.toasts.error(t("instance-change-failed"), Some(e)),
               load_mods(shared, &self.id),
            ]);
         }
         Message::CheckUpdates => {
            if self.checking {
               return Task::none();
            }
            self.checking = true;
            let lithic = shared.lithic.clone();
            let id = self.id.clone();
            return Task::perform(
               async move {
                  let instance = lithic.instance(&id).map_err(|e| e.to_string())?;
                  lithic
                     .check_updates(&instance, &Cancel::new())
                     .await
                     .map_err(|e| e.to_string())
               },
               |r| AppMessage::Instance(Message::UpdatesChecked(r)),
            );
         }
         Message::UpdatesChecked(result) => {
            self.checking = false;
            match result {
               Ok(updates) => {
                  let toast = if updates.is_empty() {
                     shared.toasts.info(t("instance-up-to-date"))
                  } else {
                     Task::none()
                  };
                  self.updates = Some(updates);
                  return toast;
               }
               Err(e) => return shared.toasts.error(t("instance-update-check-failed"), Some(e)),
            }
         }
         Message::UpdateOne(mod_id) => {
            let chosen: Vec<Update> = self
               .updates
               .iter()
               .flatten()
               .filter(|u| u.mod_id == mod_id)
               .cloned()
               .collect();
            if !chosen.is_empty() {
               return self.start_update(shared, chosen);
            }
         }
         Message::UpdateAll => {
            if let Some(updates) = self.updates.clone().filter(|u| !u.is_empty()) {
               return self.start_update(shared, updates);
            }
         }
         Message::Install(mod_id) => {
            let refs = vec![ModRef {
               id: mod_id,
               version: None,
            }];
            return self.start_install(shared, refs, OpKind::Install);
         }
         Message::AskRemove(mod_id) => {
            if let Some(Ok(installed)) = &self.mods {
               let name = installed
                  .iter()
                  .find(|m| m.mod_id() == mod_id)
                  .map_or(mod_id.clone(), |m| m.display_name().to_string());
               let dependents = installed
                  .iter()
                  .filter(|m| m.mod_id() != mod_id && m.info.dependencies.contains_key(&mod_id))
                  .map(|m| m.display_name().to_string())
                  .collect();
               self.confirm = Some(Confirm::RemoveMod {
                  mod_id,
                  name,
                  dependents,
               });
            }
         }
         Message::Pin(mod_id, version) => {
            return self.blocking_change(shared, move |l, i| l.set_mod_pin(i, &mod_id, version.as_deref()));
         }
         Message::InstallInput(value) => self.install_input = value,
         Message::InstallRequested => {
            let reference = match ModRef::parse(self.install_input.trim()) {
               Ok(reference) => reference,
               Err(error) => return shared.toasts.error(t("instance-change-failed"), Some(error.to_string())),
            };
            self.install_input.clear();
            return self.start_install(shared, vec![reference], OpKind::Install);
         }
         Message::Launch => return Task::done(AppMessage::Launch(self.id.clone())),
         Message::Stop => return Task::done(AppMessage::Stop(self.id.clone())),
         Message::Cancel => return Task::done(AppMessage::CancelOp(self.id.clone())),
         Message::Select => {
            let lithic = shared.lithic.clone();
            let id = self.id.clone();
            return Task::perform(
               blocking(move || lithic.set_active_instance(Some(&id))),
               |r| match r {
                  Ok(()) => AppMessage::Reload,
                  Err(e) => AppMessage::Instance(Message::Changed(Err(e))),
               },
            );
         }
         Message::Open(folder) => {
            if let Some(i) = shared.instance(&self.id) {
               let path = match folder {
                  Folder::Instance => i.dir.clone(),
                  Folder::Data => i.data_dir(),
                  Folder::Mods => i.mods_dir(),
                  Folder::Logs => i.game_logs_dir(),
               };
               let _ = std::fs::create_dir_all(&path);
               return Task::done(AppMessage::Open(path.display().to_string()));
            }
         }

         Message::AskDelete => self.confirm = Some(Confirm::Delete),
         Message::AskClone => {
            let name = shared
               .instance(&self.id)
               .map(|i| t1("instance-copy-name", "name", i.name.clone()))
               .unwrap_or_default();
            self.confirm = Some(Confirm::Clone {
               name,
               with_saves: false,
            });
         }
         Message::CloneName(v) => {
            if let Some(Confirm::Clone { name, .. }) = &mut self.confirm {
               *name = v;
            }
         }
         Message::CloneSaves(on) => {
            if let Some(Confirm::Clone { with_saves, .. }) = &mut self.confirm {
               *with_saves = on;
            }
         }
         Message::CloseConfirm => self.confirm = None,
         Message::Confirmed => return self.confirmed(shared),
         Message::Removed(Ok(removed)) => {
            return Task::batch([
               shared
                  .toasts
                  .success(t1("instance-removed-mods", "names", removed.join(", "))),
               load_mods(shared, &self.id),
               Task::done(AppMessage::Reload),
            ]);
         }
         Message::Removed(Err(e)) => return shared.toasts.error(t("instance-remove-failed"), Some(e)),
         Message::Deleted(Ok(())) => {
            return Task::batch([
               Task::done(AppMessage::Reload),
               Task::done(AppMessage::Navigate(Page::Instances)),
            ]);
         }
         Message::Deleted(Err(e)) => return shared.toasts.error(t("instance-delete-failed"), Some(e)),
         Message::Cloned(Ok(copy)) => {
            return Task::batch([
               shared
                  .toasts
                  .success(t1("instance-cloned", "name", copy.name.clone())),
               Task::done(AppMessage::Reload),
               Task::done(AppMessage::Navigate(Page::Instance(copy.id))),
            ]);
         }
         Message::Cloned(Err(e)) => return shared.toasts.error(t("instance-clone-failed"), Some(e)),
         Message::FormName(v) => self.edit(|f| f.name = v),
         Message::FormGame(g) => self.edit(|f| f.game = Some(g)),
         Message::FormArgs(v) => self.edit(|f| f.args = v),
         Message::FormWrapper(v) => self.edit(|f| f.wrapper = v),
         Message::FormEnv(action) => {
            let edits = action.is_edit();
            self.form.env.perform(action);
            if edits {
               self.edit(|_| {});
            }
         }
         Message::FormPickMods => {
            return Task::future(pick_folder(t("instance-pick-mods")))
               .and_then(|d| Task::done(AppMessage::Instance(Message::FormModsDir(Some(d)))));
         }
         Message::FormModsDir(dir) => self.edit(|f| f.mods_dir = dir),
         Message::Revert => {
            if let Some(i) = shared.instance(&self.id) {
               self.form = Form::from(i, shared);
            }
         }
         Message::Save => {
            if self.form.saving {
               return Task::none();
            }
            let parsed = match self.form.parse() {
               Ok(p) => p,
               Err(e) => {
                  self.form.error = Some(e);
                  return Task::none();
               }
            };
            self.form.saving = true;
            let lithic = shared.lithic.clone();
            let id = self.id.clone();
            return Task::perform(
               blocking(move || {
                  lithic.update_instance(&id, |i| {
                     i.name = parsed.name;
                     i.game_version = parsed.game;
                     i.launch.args = parsed.args;
                     i.launch.env = parsed.env;
                     i.launch.wrapper = parsed.wrapper;
                     i.mods_dir = parsed.mods_dir;
                     Ok(())
                  })
               }),
               |r| AppMessage::Instance(Message::Saved(r)),
            );
         }
         Message::Saved(result) => {
            self.form.saving = false;
            match result {
               Ok(()) => {
                  self.form.dirty = false;
                  self.form.error = None;
                  return Task::batch([
                     shared.toasts.success(t("instance-saved")),
                     Task::done(AppMessage::Reload),
                     load_mods(shared, &self.id),
                  ]);
               }
               Err(e) => self.form.error = Some(e),
            }
         }

         Message::SelectLog(path) => {
            self.logs.selected = Some(path);
            return self.refresh_logs(shared);
         }
         Message::RefreshLog => return self.refresh_logs(shared),
         Message::LogLoaded(Ok((files, selected, text))) => {
            self.logs.files = files;
            self.logs.selected = selected;
            if self.logs.content.text() != text {
               self.logs.content = text_editor::Content::with_text(&text);
               if self.logs.follow {
                  self
                     .logs
                     .content
                     .perform(text_editor::Action::Move(text_editor::Motion::DocumentEnd));
               }
            }
         }
         Message::LogLoaded(Err(e)) => return shared.toasts.error(t("instance-log-failed"), Some(e)),
         Message::LogAction(action) => {
            if !action.is_edit() {
               self.logs.content.perform(action);
            }
         }
         Message::Follow(on) => self.logs.follow = on,
      }
      Task::none()
   }

   fn edit(&mut self, f: impl FnOnce(&mut Form)) {
      f(&mut self.form);
      self.form.dirty = true;
      self.form.error = None;
   }

   fn confirmed(&mut self, shared: &mut Shared) -> Task<AppMessage> {
      let Some(confirm) = self.confirm.take() else {
         return Task::none();
      };
      let lithic = shared.lithic.clone();
      let id = self.id.clone();
      match confirm {
         Confirm::RemoveMod { mod_id, .. } => Task::perform(
            blocking(move || {
               let instance = lithic.instance(&id)?;
               let removed = lithic.remove_mods(&instance, &[mod_id], true)?;
               Ok(removed.iter().map(|m| m.display_name().to_string()).collect())
            }),
            |r| AppMessage::Instance(Message::Removed(r)),
         ),
         Confirm::Delete => {
            if shared.is_running(&id) || shared.busy.contains_key(&id) {
               return shared.toasts.warning(t("instance-delete-busy"));
            }
            Task::perform(blocking(move || lithic.delete_instance(&id)), |r| {
               AppMessage::Instance(Message::Deleted(r))
            })
         }
         Confirm::Clone { name, with_saves } => {
            if name.trim().is_empty() {
               self.confirm = Some(Confirm::Clone { name, with_saves });
               return Task::none();
            }
            Task::perform(
               blocking(move || lithic.clone_instance(&id, &name, with_saves)),
               |r| AppMessage::Instance(Message::Cloned(r)),
            )
         }
      }
   }

   pub fn view<'a>(&'a self, shared: &'a Shared) -> Element<'a, Message> {
      let Some(instance) = shared.instance(&self.id) else {
         return widget::loading(t("loading"));
      };

      let body: Element<Message> = match self.tab {
         Tab::Mods => self.mods_tab(instance, shared),
         Tab::Logs => self.logs_tab(),
         Tab::Settings => self.settings_tab(instance, shared),
      };

      let content = column![self.header(instance, shared), self.tabs(), body]
         .spacing(16)
         .padding(24)
         .height(Fill);

      match &self.confirm {
         Some(confirm) => widget::modal(
            content,
            self.confirm_view(confirm, instance),
            Message::CloseConfirm,
         ),
         None => content.into(),
      }
   }

   fn header<'a>(&'a self, instance: &'a Instance, shared: &'a Shared) -> Element<'a, Message> {
      let running = shared.running.contains_key(&self.id);
      let launching = shared.launching.contains(&self.id);
      let selected = shared.settings.active_instance.as_deref() == Some(self.id.as_str());
      let game_ok = instance
         .game_version
         .as_deref()
         .is_some_and(|v| shared.installs.iter().any(|i| i.version == v));

      let mut title = row![text(&instance.name).size(26).font(widget::bold())]
         .spacing(10)
         .align_y(Center);
      if selected {
         title = title.push(widget::badge(t("instances-selected"), Tone::Accent));
      }
      if running {
         title = title.push(widget::badge(t("instances-running"), Tone::Good));
      }

      let game = match &instance.game_version {
         Some(v) if game_ok => t1("instances-game", "version", v.clone()),
         Some(v) => t1("instance-game-not-installed", "version", v.clone()),
         None => t("instances-no-game"),
      };
      let played = match instance.stats.last_played_at {
         Some(ms) => t2(
            "instances-played",
            "when",
            format_time(ms),
            "total",
            format_duration(instance.stats.play_time_ms),
         ),
         None => t("instances-never-played"),
      };

      let play: Element<Message> = if running {
         button(text(t("instances-stop")))
            .padding([8, 20])
            .style(button::danger)
            .on_press(Message::Stop)
            .into()
      } else {
         widget::action(
            t("instances-play"),
            t("instances-starting"),
            launching,
            game_ok.then_some(Message::Launch),
         )
      };

      let small =
         |label: String, msg: Message| button(text(label).size(13)).style(button::text).on_press(msg);
      let mut tools = row![
         small(t("instance-open-folder"), Message::Open(Folder::Instance)),
         small(t("instance-clone"), Message::AskClone),
         small(t("instance-delete"), Message::AskDelete),
      ]
      .spacing(2);
      if !selected {
         tools = tools.push(small(t("instance-select"), Message::Select));
      }

      column![
         button(text(t("instance-back")).size(13))
            .style(button::text)
            .padding(0)
            .on_press(Message::Back),
         row![
            column![
               title,
               text(format!("{game}  |  {played}")).size(13).style(style::muted)
            ]
            .spacing(6)
            .width(Fill),
            play,
         ]
         .spacing(12)
         .align_y(Center),
         tools,
      ]
      .spacing(8)
      .into()
   }

   fn tabs(&self) -> Element<'_, Message> {
      let tab = |label: String, tab: Tab| {
         button(text(label))
            .padding([6, 14])
            .style(style::tab(self.tab == tab))
            .on_press(Message::Tab(tab))
      };
      let mods_label = match &self.mods {
         Some(Ok(m)) => t1("instance-tab-mods-count", "count", m.len()),
         _ => t("instance-tab-mods"),
      };
      row![
         tab(mods_label, Tab::Mods),
         tab(t("instance-tab-logs"), Tab::Logs),
         tab(t("instance-tab-settings"), Tab::Settings),
      ]
      .spacing(6)
      .into()
   }

   fn mods_tab<'a>(&'a self, instance: &'a Instance, shared: &'a Shared) -> Element<'a, Message> {
      let busy = shared.busy.get(&self.id);
      let updates = self.updates.as_deref().unwrap_or_default();

      let mut toolbar = row![
         text_input(&t("instance-filter"), &self.filter)
            .on_input(Message::Filter)
            .padding(8)
            .width(260),
         space::horizontal(),
         button(text(t("instance-open-mods")).size(13))
            .style(button::text)
            .on_press(Message::Open(Folder::Mods)),
         text(t("instance-add-mods")).size(13),
         text_input("mod-id", &self.install_input)
            .on_input(Message::InstallInput)
            .on_submit(Message::InstallRequested)
            .padding(8)
            .width(160),
         button(text(t("browse-install")).size(13))
            .style(button::secondary)
            .on_press_maybe(
               (busy.is_none() && !self.install_input.trim().is_empty())
                  .then_some(Message::InstallRequested),
            ),
      ]
      .spacing(8)
      .align_y(Center);
      toolbar = if updates.is_empty() {
         toolbar.push(widget::action(
            t("instance-check-updates"),
            t("instance-checking"),
            self.checking,
            busy.is_none().then_some(Message::CheckUpdates),
         ))
      } else {
         toolbar.push(widget::action(
            t1("instance-update-all", "count", updates.len()),
            t("instance-updating"),
            busy.is_some(),
            Some(Message::UpdateAll),
         ))
      };

      let mut col = column![toolbar].spacing(12);

      if let Some(b) = busy {
         let bar: Element<Message> = match b.fraction() {
            Some(f) => progress_bar(0.0..=1.0, f).girth(6).into(),
            None => space().into(),
         };
         col = col.push(widget::notice(
            column![
               row![
                  text(b.label()).width(Fill),
                  button(text(t("common-cancel")).size(13))
                     .style(button::secondary)
                     .on_press(Message::Cancel),
               ]
               .align_y(Center),
               bar,
            ]
            .spacing(6),
            Tone::Neutral,
         ));
      }

      if instance.game_version.is_none() {
         col = col.push(widget::notice(text(t("instance-no-game-warning")), Tone::Warn));
      }

      if !self.problems.is_empty() {
         let lines = column(self.problems.iter().map(|p| {
            let fix = installable_fix(p).map(|dep| {
               button(text(t("instance-fix-install")).size(12))
                  .style(button::secondary)
                  .on_press_maybe(busy.is_none().then(|| Message::Install(dep.to_string())))
            });
            row![text(describe_problem(p)).size(13).width(Fill)]
               .extend(fix.map(Into::into))
               .align_y(Center)
               .into()
         }))
         .spacing(4);
         col = col.push(widget::notice(
            column![text(t("instance-problems")).font(widget::bold()), lines].spacing(6),
            Tone::Warn,
         ));
      }

      let list: Element<Message> = match &self.mods {
         None => widget::loading(t("loading")),
         Some(Err(e)) => widget::notice(text(e.clone()), Tone::Bad),
         Some(Ok(installed)) if installed.is_empty() => widget::empty(
            t("instance-no-mods-title"),
            t("instance-no-mods-body"),
            None,
         ),
         Some(Ok(installed)) => {
            let needle = self.filter.trim().to_lowercase();
            let rows = installed
               .iter()
               .filter(|m| {
                  needle.is_empty()
                     || m.display_name().to_lowercase().contains(&needle)
                     || m.mod_id().contains(&needle)
               })
               .map(|m| mod_row(m, updates.iter().find(|u| u.mod_id == m.mod_id()), busy.is_some()));
            scrollable(column(rows).spacing(6)).height(Fill).into()
         }
      };
      col.push(list).height(Fill).into()
   }

   fn logs_tab(&self) -> Element<'_, Message> {
      let names: Vec<LogFile> = self.logs.files.iter().cloned().map(LogFile).collect();
      let selected = self.logs.selected.clone().map(LogFile);
      let bar = row![
         pick_list(names, selected, |f: LogFile| Message::SelectLog(f.0))
            .placeholder(t("instance-no-logs"))
            .width(320),
         button(text(t("common-refresh")).size(13))
            .style(button::secondary)
            .on_press(Message::RefreshLog),
         space::horizontal(),
         toggler(self.logs.follow)
            .label(t("instance-follow-log"))
            .on_toggle(Message::Follow),
         button(text(t("common-open-folder")).size(13))
            .style(button::text)
            .on_press(Message::Open(Folder::Logs)),
      ]
      .spacing(8)
      .align_y(Center);

      let view: Element<Message> = if self.logs.files.is_empty() {
         widget::empty(t("instance-no-logs"), t("instance-no-logs-body"), None)
      } else {
         text_editor(&self.logs.content)
            .on_action(Message::LogAction)
            .font(iced::Font::MONOSPACE)
            .size(12)
            .height(Fill)
            .into()
      };
      column![bar, view].spacing(10).height(Fill).into()
   }

   fn settings_tab<'a>(&'a self, instance: &'a Instance, shared: &'a Shared) -> Element<'a, Message> {
      let f = &self.form;
      let mods_label = f
         .mods_dir
         .as_ref()
         .map_or_else(|| t("instance-mods-dir-default"), |p| p.display().to_string());
      let mut mods_row = row![
         text(mods_label).size(13).width(Fill),
         button(text(t("common-choose-folder")).size(13))
            .style(button::secondary)
            .on_press(Message::FormPickMods),
      ]
      .spacing(8)
      .align_y(Center);
      if f.mods_dir.is_some() {
         mods_row = mods_row.push(
            button(text(t("common-reset")).size(13))
               .style(button::text)
               .on_press(Message::FormModsDir(None)),
         );
      }

      let data_hint = if instance.has_external_data() {
         t("instance-data-external")
      } else {
         t("instance-data-internal")
      };

      let mut form = column![
         widget::field(
            t("instances-name"),
            text_input("", &f.name).on_input(Message::FormName).padding(8),
            None
         ),
         widget::field(
            t("instances-game-version"),
            pick_list(game_choices(shared), f.game.clone(), Message::FormGame)
               .placeholder(t("instances-pick-game")),
            None
         ),
         widget::field(
            t("instance-args"),
            text_input("--connect server.example:42420", &f.args)
               .on_input(Message::FormArgs)
               .padding(8),
            Some(t("instance-args-hint"))
         ),
         widget::field(
            t("instance-env"),
            text_editor(&f.env)
               .on_action(Message::FormEnv)
               .height(90)
               .placeholder("KEY=value"),
            Some(t("instance-env-hint"))
         ),
         widget::field(
            t("instance-wrapper"),
            text_input("gamemoderun", &f.wrapper)
               .on_input(Message::FormWrapper)
               .padding(8),
            Some(t("instance-wrapper-hint"))
         ),
         widget::field(
            t("instance-mods-dir"),
            mods_row,
            Some(t("instance-mods-dir-hint"))
         ),
         widget::field(
            t("instance-data-dir"),
            row![
               text(instance.data_dir().display().to_string())
                  .size(13)
                  .width(Fill),
               button(text(t("common-open-folder")).size(13))
                  .style(button::text)
                  .on_press(Message::Open(Folder::Data)),
            ]
            .align_y(Center),
            Some(data_hint)
         ),
      ]
      .spacing(16);
      if let Some(error) = &f.error {
         form = form.push(widget::notice(text(error.clone()), Tone::Bad));
      }
      form = form.push(
         row![
            space::horizontal(),
            widget::secondary(t("common-revert"), f.dirty.then_some(Message::Revert)),
            widget::action(
               t("common-save"),
               t("common-saving"),
               f.saving,
               f.dirty.then_some(Message::Save)
            ),
         ]
         .spacing(8),
      );

      scrollable(widget::card(form.max_width(720))).height(Fill).into()
   }

   fn confirm_view<'a>(&'a self, confirm: &'a Confirm, instance: &'a Instance) -> Element<'a, Message> {
      match confirm {
         Confirm::RemoveMod { name, dependents, .. } => {
            let body = if dependents.is_empty() {
               t1("instance-remove-body", "name", name.clone())
            } else {
               t2(
                  "instance-remove-body-deps",
                  "name",
                  name.clone(),
                  "dependents",
                  dependents.join(", "),
               )
            };
            widget::confirm(
               t("instance-remove-title"),
               body,
               t("common-remove"),
               true,
               Message::Confirmed,
               Message::CloseConfirm,
            )
         }
         Confirm::Delete => {
            let body = if instance.has_external_data() {
               t1(
                  "instance-delete-body-external",
                  "path",
                  instance.data_dir().display().to_string(),
               )
            } else {
               t1("instance-delete-body", "path", instance.dir.display().to_string())
            };
            widget::confirm(
               t1("instance-delete-title", "name", instance.name.clone()),
               body,
               t("instance-delete"),
               true,
               Message::Confirmed,
               Message::CloseConfirm,
            )
         }
         Confirm::Clone { name, with_saves } => widget::dialog(
            t("instance-clone-title"),
            column![
               widget::field(
                  t("instances-name"),
                  text_input("", name)
                     .on_input(Message::CloneName)
                     .on_submit(Message::Confirmed)
                     .padding(8),
                  None
               ),
               checkbox(*with_saves)
                  .label(t("instance-clone-saves"))
                  .on_toggle(Message::CloneSaves),
            ]
            .spacing(12),
            row![
               widget::secondary(t("common-cancel"), Some(Message::CloseConfirm)),
               widget::action(
                  t("instance-clone"),
                  String::new(),
                  false,
                  (!name.trim().is_empty()).then_some(Message::Confirmed)
               ),
            ]
            .spacing(8),
            460.0,
         ),
      }
   }
}

fn mod_row<'a>(m: &'a InstalledMod, update: Option<&'a Update>, busy: bool) -> Element<'a, Message> {
   let id = m.mod_id().to_string();
   let mut badges = row![].spacing(6);
   if let Some(pin) = &m.lock.pin {
      badges = badges.push(widget::badge(
         t1("instance-pinned", "version", pin.clone()),
         Tone::Accent,
      ));
   }
   if m.lock.dependency {
      badges = badges.push(widget::badge(t("instance-dependency"), Tone::Neutral));
   }
   if !m.info.has_metadata {
      badges = badges.push(widget::badge(t("instance-no-metadata"), Tone::Warn));
   }
   if m.error.is_some() {
      badges = badges.push(widget::badge(t("instance-unreadable"), Tone::Bad));
   }
   if let Some(u) = update {
      badges = badges.push(widget::badge(
         t1(
            "instance-update-to",
            "version",
            u.release.version.clone().unwrap_or_default(),
         ),
         Tone::Good,
      ));
   }

   let version = if m.info.version.is_empty() {
      "?".to_string()
   } else {
      m.info.version.clone()
   };
   let pin = match &m.lock.pin {
      Some(_) => Some(Message::Pin(id.clone(), None)),
      None => (!m.info.version.is_empty()).then(|| Message::Pin(id.clone(), Some(m.info.version.clone()))),
   };
   let pin_label = if m.lock.pin.is_some() {
      t("instance-unpin")
   } else {
      t("instance-pin")
   };

   let mut actions = row![].spacing(2).align_y(Center);
   if update.is_some() {
      actions = actions.push(
         button(text(t("instance-update")).size(13))
            .style(button::text)
            .on_press_maybe((!busy).then(|| Message::UpdateOne(id.clone()))),
      );
   }
   actions = actions
      .push(
         button(text(pin_label).size(13))
            .style(button::text)
            .on_press_maybe(pin),
      )
      .push(
         button(text(t("common-remove")).size(13))
            .style(button::text)
            .on_press_maybe((!busy).then(|| Message::AskRemove(id.clone()))),
      );

   widget::card(
      row![
         checkbox(m.enabled).on_toggle_maybe((!busy).then(|| {
            let id = id.clone();
            move |on| Message::Toggle(id.clone(), on)
         })),
         column![
            row![text(m.display_name()).font(widget::bold()), badges]
               .spacing(8)
               .align_y(Center),
            text(format!("{} {}  |  {}", m.mod_id(), version, m.file_name))
               .size(12)
               .style(style::muted),
         ]
         .spacing(4)
         .width(Fill),
         actions,
      ]
      .spacing(12)
      .align_y(Center),
   )
   .padding([10, 14])
   .into()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LogFile(PathBuf);

impl fmt::Display for LogFile {
   fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
      let name = self
         .0
         .file_name()
         .map(|n| n.to_string_lossy().into_owned())
         .unwrap_or_default();
      match name.strip_prefix("launch-").and_then(|n| n.strip_suffix(".log")) {
         Some(stamp) => match stamp.parse::<i64>() {
            Ok(ms) => f.write_str(&t1("instance-launch-log", "when", format_time(ms))),
            Err(_) => f.write_str(&name),
         },
         None => f.write_str(&name),
      }
   }
}

#[cfg(test)]
mod tests {
   use super::*;
   use lithic_core::instance::NewInstance;

   fn setup() -> (tempfile::TempDir, Shared, String) {
      let dir = tempfile::tempdir().unwrap();
      let lithic = lithic_core::Lithic::new(lithic_core::Paths::rooted(dir.path())).unwrap();
      let instance = lithic
         .create_instance(NewInstance {
            name: "Test".into(),
            game_version: Some("1.21.5".into()),
            ..Default::default()
         })
         .unwrap();
      let mut shared = Shared::new(lithic);
      shared.instances.push(instance.clone());
      (dir, shared, instance.id)
   }

   #[test]
   fn form_round_trip_and_validation() {
      let (_d, shared, id) = setup();
      let mut state = State::new(&id, &shared);
      let mut shared = shared;
      assert!(!state.form.dirty);
      let _ = state.update(Message::FormArgs("--connect 'my host:1'".into()), &mut shared);
      assert!(state.form.dirty);
      let parsed = state.form.parse().ok().unwrap();
      assert_eq!(parsed.args, ["--connect", "my host:1"]);

      let _ = state.update(Message::FormArgs("'open".into()), &mut shared);
      let _ = state.update(Message::Save, &mut shared);
      assert!(state.form.error.is_some(), "bad quoting is reported, not saved");
      assert!(!state.form.saving);

      state.form.env = text_editor::Content::with_text("A=1\nnot a pair");
      state.form.args.clear();
      assert!(state.form.parse().is_err());
      state.form.env = text_editor::Content::with_text("A=1\n\nB = two=2");
      let parsed = state.form.parse().ok().unwrap();
      assert_eq!(parsed.env.get("B").map(String::as_str), Some(" two=2"));
   }

   #[test]
   fn remove_confirmation_lists_dependents() {
      let (_d, mut shared, id) = setup();
      let mut state = State::new(&id, &shared);
      let lib = InstalledMod {
         info: lithic_core::modinfo::ModInfo {
            mod_id: "lib".into(),
            name: "Lib".into(),
            ..Default::default()
         },
         path: "/x/lib.zip".into(),
         file_name: "lib.zip".into(),
         format: lithic_core::modinfo::Format::Zip,
         enabled: true,
         error: None,
         lock: Default::default(),
      };
      let mut app = lib.clone();
      app.info.mod_id = "app".into();
      app.info.name = "App".into();
      app.info.dependencies.insert("lib".into(), "*".into());
      let _ = state.update(Message::ModsLoaded(Ok(vec![app, lib])), &mut shared);
      let _ = state.update(Message::AskRemove("lib".into()), &mut shared);
      match &state.confirm {
         Some(Confirm::RemoveMod { dependents, .. }) => assert_eq!(dependents, &["App".to_string()]),
         other => panic!("unexpected {other:?}"),
      }
      let _ = state.update(Message::CloseConfirm, &mut shared);
      assert!(state.confirm.is_none());
   }

   #[test]
   fn log_names_are_readable() {
      assert_eq!(
         LogFile("/x/client-main.log".into()).to_string(),
         "client-main.log"
      );
      assert!(
         LogFile("/x/launch-1700000000000.log".into())
            .to_string()
            .contains("2023")
      );
   }
}
