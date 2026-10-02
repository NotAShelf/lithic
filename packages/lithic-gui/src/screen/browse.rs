use std::{
  collections::{BTreeSet, HashMap},
  fmt,
};

use iced::{
  Center,
  Element,
  Fill,
  Length,
  Task,
  alignment::Horizontal,
  widget::{
    button,
    checkbox,
    column,
    container,
    image,
    row,
    scrollable,
    space,
    text,
    toggler,
  },
};
use lithic_core::{
  Cancel,
  Freshness,
  http::Http,
  moddb::{self, ModDetails, ModSummary, Query, Sort},
  mods::{
    InstallOptions,
    ModRef,
    Update,
    resolve::{Target, pick_release},
  },
  version,
};
use lithic_icons::{self as icon, Icon};

use super::format_count;
use crate::{
  app::{Message as AppMessage, OpKind, Outcome, Shared, Summary},
  i18n::{t, t1, t2},
  style::{self, Kind, Tone},
  task::blocking,
  widget,
};

const PAGE: usize = 40;
const ACTIONS_WIDTH: f32 = 104.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SortChoice(Sort);

impl SortChoice {
  const ALL: [Self; 6] = [
    Self(Sort::Relevance),
    Self(Sort::Downloads),
    Self(Sort::Trending),
    Self(Sort::Updated),
    Self(Sort::Follows),
    Self(Sort::Name),
  ];
}

impl fmt::Display for SortChoice {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.write_str(&match self.0 {
      Sort::Relevance => t("browse-sort-relevance"),
      Sort::Downloads => t("browse-sort-downloads"),
      Sort::Trending => t("browse-sort-trending"),
      Sort::Updated => t("browse-sort-updated"),
      Sort::Follows => t("browse-sort-follows"),
      Sort::Name => t("browse-sort-name"),
    })
  }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetChoice {
  pub(crate) id:   String,
  pub(crate) name: String,
}

impl fmt::Display for TargetChoice {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.write_str(&self.name)
  }
}

/// A mod logo, cached in [`Shared`] so every browse view reuses downloads.
#[derive(Debug, Clone)]
pub enum Logo {
  Loading,
  Ready(image::Handle),
  Failed,
}

#[derive(Debug)]
struct Details {
  summary:     ModSummary,
  data:        Option<Result<ModDetails, String>>,
  description: String,
}

/// `None` fetches all mods; `Some` restricts releases to a major.minor game
/// version.
type PoolKey = Option<(u64, u64)>;

#[derive(Debug)]
#[expect(
  clippy::struct_excessive_bools,
  reason = "the filters are independent toggles the user flips"
)]
pub struct State {
  /// Wraps this view's messages, so it can live on its own page or inside
  /// an instance.
  wrap:           fn(Message) -> AppMessage,
  /// Installs always go into `target`; the instance picker is hidden.
  locked:         bool,
  target:         Option<String>,
  query:          String,
  sort:           SortChoice,
  compatible:     bool,
  favorites_only: bool,
  hide_installed: bool,
  pool:           Option<Result<Vec<ModSummary>, String>>,
  pool_key:       Option<PoolKey>,
  request:        u64,
  results:        Vec<usize>,
  shown:          usize,
  installed:      HashMap<String, String>,
  updates:        HashMap<String, Update>,
  /// Mod ids picked to install in one go.
  selected:       BTreeSet<String>,
  details:        Option<Details>,
}

impl Default for State {
  fn default() -> Self {
    Self {
      wrap:           AppMessage::Browse,
      locked:         false,
      target:         None,
      query:          String::new(),
      sort:           SortChoice(Sort::Downloads),
      compatible:     true,
      favorites_only: false,
      hide_installed: false,
      pool:           None,
      pool_key:       None,
      request:        0,
      results:        Vec::new(),
      shown:          PAGE,
      installed:      HashMap::new(),
      updates:        HashMap::new(),
      selected:       BTreeSet::new(),
      details:        None,
    }
  }
}

#[derive(Debug, Clone)]
pub enum Message {
  Target(TargetChoice),
  Query(String),
  Sort(SortChoice),
  Compatible(bool),
  FavoritesOnly(bool),
  HideInstalled(bool),
  Refresh,
  PoolLoaded(u64, PoolKey, Result<Vec<ModSummary>, String>),
  Installed(Option<String>, Result<HashMap<String, String>, String>),
  Updates(Option<String>, Result<HashMap<String, Update>, String>),
  Logo(String, Option<Vec<u8>>),
  More,
  Favorite(String),
  FavoriteSaved(Result<(), String>),
  Update(String),
  Install(String, Option<String>),
  Select(String, bool),
  ClearSelection,
  InstallSelected,
  OpenDetails(usize),
  DetailsLoaded(i64, Result<Box<ModDetails>, String>),
  CloseDetails,
  Open(String),
}

pub fn enter(state: &State, shared: &Shared) -> Task<AppMessage> {
  let key = state.wanted_key(shared);
  let mut tasks = vec![refresh_installed(state, shared)];
  if state.pool_key != Some(key) || matches!(state.pool, Some(Err(_))) {
    tasks.push(Task::done((state.wrap)(Message::Refresh)));
  }
  Task::batch(tasks)
}

pub fn refresh_installed(state: &State, shared: &Shared) -> Task<AppMessage> {
  let Some(id) = state
    .target
    .clone()
    .or_else(|| shared.default_instance().map(|i| i.id.clone()))
  else {
    return Task::none();
  };
  let lithic = shared.lithic.clone();
  let reader = lithic.clone();
  let installed_target = Some(id.clone());
  let update_id = id.clone();
  let updates_target = Some(id.clone());
  let update_reader = lithic.clone();
  let wrap = state.wrap;
  Task::batch([
    Task::perform(
      blocking(move || {
        let instance = reader.instance(&id)?;
        Ok(
          reader
            .installed_mods(&instance)?
            .into_iter()
            .map(|m| (m.info.mod_id, m.info.version))
            .collect(),
        )
      }),
      move |r| wrap(Message::Installed(installed_target, r)),
    ),
    Task::perform(
      async move {
        let instance =
          blocking(move || update_reader.instance(&update_id)).await?;
        let updates = lithic
          .check_updates(&instance, &Cancel::new())
          .await
          .map_err(|e| e.to_string())?
          .into_iter()
          .filter(|u| {
            u.release
              .version
              .as_deref()
              .is_some_and(|v| version::compare(v, &u.installed).is_gt())
          })
          .map(|u| (u.mod_id.clone(), u))
          .collect();
        Ok(updates)
      },
      move |r| wrap(Message::Updates(updates_target, r)),
    ),
  ])
}

impl State {
  /// Keeps the target pointing at an instance that exists.
  pub fn sync_target(&mut self, shared: &Shared) {
    if self
      .target
      .as_deref()
      .is_some_and(|id| shared.instance(id).is_none())
    {
      self.target = None;
    }
  }

  pub fn set_target(&mut self, id: Option<String>, shared: &Shared) {
    self.target = id;
    self.sync_target(shared);
    self.installed.clear();
    self.updates.clear();
  }

  fn target_id<'a>(&'a self, shared: &'a Shared) -> Option<&'a str> {
    self
      .target
      .as_deref()
      .or_else(|| shared.default_instance().map(|i| i.id.as_str()))
  }

  fn target_game<'a>(&self, shared: &'a Shared) -> Option<&'a str> {
    self
      .target_id(shared)
      .and_then(|id| shared.instance(id))
      .and_then(|i| i.game_version.as_deref())
  }

  fn wanted_key(&self, shared: &Shared) -> PoolKey {
    if self.compatible {
      self.target_game(shared).and_then(version::minor)
    } else {
      None
    }
  }

  fn rerank(&mut self, shared: &Shared) {
    let Some(Ok(pool)) = &self.pool else {
      self.results.clear();
      return;
    };
    let sort = if self.query.trim().is_empty() && self.sort.0 == Sort::Relevance
    {
      Sort::Downloads
    } else {
      self.sort.0
    };
    let favorites = &shared.settings.gui.favorites;
    self.results = moddb::rank(pool, &self.query, sort)
      .into_iter()
      .filter(|&i| !pool[i].mod_ids.is_empty())
      .filter(|&i| {
        !self.favorites_only
          || pool[i].mod_ids.iter().any(|id| favorites.contains(id))
      })
      .filter(|&i| {
        !self.hide_installed
          || !pool[i]
            .mod_ids
            .iter()
            .any(|id| self.installed.contains_key(id))
      })
      .collect();
  }

  /// Reranks for a new search or filter, starting again from the first page.
  /// Refreshes that keep the search, such as after an install, call
  /// [`Self::rerank`] instead so the list does not shrink under the user.
  fn refilter(&mut self, shared: &mut Shared) -> Task<AppMessage> {
    self.rerank(shared);
    self.shown = PAGE;
    self.load_logos(shared)
  }

  fn install(
    &self,
    shared: &mut Shared,
    refs: Vec<ModRef>,
    pin: bool,
  ) -> Task<AppMessage> {
    let Some(target) = self.target_id(shared).map(ToString::to_string) else {
      return shared.toasts.warning(t("browse-no-instance"));
    };
    shared.start_op(
      target.clone(),
      OpKind::Install,
      move |lithic, reporter, cancel| {
        async move {
          let instance = lithic.instance(&target).map_err(|e| e.to_string())?;
          let opts = InstallOptions {
            dependencies: true,
            pin_versions: pin,
            reporter,
            cancel,
            ..InstallOptions::default()
          };
          lithic
            .install_mods(&instance, &refs, &opts)
            .await
            .map(|r| Outcome::Mods(Summary::from(&r)))
            .map_err(|e| e.to_string())
        }
      },
    )
  }

  fn load_logos(&self, shared: &mut Shared) -> Task<AppMessage> {
    let Some(Ok(pool)) = &self.pool else {
      return Task::none();
    };
    let wanted: Vec<String> = self
      .results
      .iter()
      .take(self.shown)
      .filter_map(|&i| pool[i].logo.clone())
      .filter(|url| !shared.logos.contains_key(url))
      .collect();
    let mut tasks = Vec::new();
    for url in wanted {
      shared.logos.insert(url.clone(), Logo::Loading);
      tasks.push(logo_task(shared.lithic.http.clone(), url, self.wrap));
    }
    Task::batch(tasks)
  }

  /// A browse view inside an instance page that installs only into `id`.
  pub fn for_instance(id: &str, wrap: fn(Message) -> AppMessage) -> Self {
    Self {
      wrap,
      locked: true,
      target: Some(id.to_string()),
      ..Self::default()
    }
  }

  pub fn update(
    &mut self,
    message: Message,
    shared: &mut Shared,
  ) -> Task<AppMessage> {
    let wrap = self.wrap;
    match message {
      Message::Target(choice) => {
        if self.locked {
          return Task::none();
        }
        self.set_target(Some(choice.id), shared);
        return enter(self, shared);
      },
      Message::Query(q) => {
        self.query = q;
        return self.refilter(shared);
      },
      Message::Sort(s) => {
        self.sort = s;
        return self.refilter(shared);
      },
      Message::Compatible(on) => {
        self.compatible = on;
        return enter(self, shared);
      },
      Message::FavoritesOnly(on) => {
        self.favorites_only = on;
        return self.refilter(shared);
      },
      Message::HideInstalled(on) => {
        self.hide_installed = on;
        return self.refilter(shared);
      },
      Message::Refresh => {
        self.request += 1;
        let request = self.request;
        let key = self.wanted_key(shared);
        let force = self.pool_key == Some(key);
        self.pool = None;
        let lithic = shared.lithic.clone();
        return Task::perform(
          async move {
            let result = match key {
              None => {
                let freshness = if force {
                  Freshness::Refresh
                } else {
                  Freshness::Cached
                };
                lithic.mod_index(freshness).await.map(|index| index.mods)
              },
              Some(minor) => {
                match lithic.moddb.game_versions().await {
                  Ok(tags) => {
                    let ids: Vec<i64> = tags
                      .iter()
                      .filter(|tag| version::minor(&tag.name) == Some(minor))
                      .map(|tag| tag.tag_id)
                      .collect();
                    lithic
                      .moddb
                      .mods(&Query {
                        game_versions: ids,
                        ..Query::default()
                      })
                      .await
                  },
                  Err(e) => Err(e),
                }
              },
            };
            result.map_err(|e| e.to_string())
          },
          move |r| wrap(Message::PoolLoaded(request, key, r)),
        );
      },
      Message::PoolLoaded(request, key, result) => {
        if request != self.request {
          return Task::none();
        }
        self.pool_key = Some(key);
        self.pool = Some(result);
        return self.refilter(shared);
      },
      Message::Installed(target, result) => {
        if target.as_deref() != self.target_id(shared) {
          return Task::none();
        }
        match result {
          Ok(installed) => {
            self.installed = installed;
            self.updates.retain(|id, update| {
              self.installed.get(id) == Some(&update.installed)
            });
            self.selected.retain(|id| !self.installed.contains_key(id));
            self.rerank(shared);
            return self.load_logos(shared);
          },
          Err(e) => {
            return shared.toasts.error(t("browse-installed-failed"), Some(e));
          },
        }
      },
      Message::Updates(target, result) => {
        if target.as_deref() != self.target_id(shared) {
          return Task::none();
        }
        match result {
          Ok(updates) => self.updates = updates,
          Err(e) => {
            return shared
              .toasts
              .error(t("instance-update-check-failed"), Some(e));
          },
        }
      },
      Message::Logo(url, bytes) => {
        let logo = bytes
          .map_or(Logo::Failed, |b| Logo::Ready(image::Handle::from_bytes(b)));
        shared.logos.insert(url, logo);
      },
      Message::More => {
        self.shown += PAGE;
        return self.load_logos(shared);
      },
      Message::Favorite(mod_id) => {
        let favorites = &mut shared.settings.gui.favorites;
        let adding = !favorites.contains(&mod_id);
        if adding {
          favorites.insert(mod_id.clone());
        } else {
          favorites.remove(&mod_id);
        }
        if self.favorites_only {
          self.rerank(shared);
        }
        let lithic = shared.lithic.clone();
        return Task::perform(
          blocking(move || {
            lithic.update_settings(|s| {
              if adding {
                s.gui.favorites.insert(mod_id);
              } else {
                s.gui.favorites.remove(&mod_id);
              }
            })
          }),
          move |r| wrap(Message::FavoriteSaved(r)),
        );
      },
      Message::FavoriteSaved(Ok(())) => {},
      Message::FavoriteSaved(Err(e)) => {
        return shared.toasts.error(t("browse-favorite-failed"), Some(e));
      },
      Message::Install(mod_id, version) => {
        let pin = version.is_some();
        let refs = vec![ModRef {
          id: mod_id,
          version,
        }];
        return self.install(shared, refs, pin);
      },
      Message::Select(mod_id, on) => {
        if on {
          self.selected.insert(mod_id);
        } else {
          self.selected.remove(&mod_id);
        }
      },
      Message::ClearSelection => self.selected.clear(),
      Message::InstallSelected => {
        let refs = self
          .selected
          .iter()
          .map(|id| {
            ModRef {
              id:      id.clone(),
              version: None,
            }
          })
          .collect::<Vec<_>>();
        if refs.is_empty() {
          return Task::none();
        }
        return self.install(shared, refs, false);
      },
      Message::Update(mod_id) => {
        let Some(target) = self.target_id(shared).map(ToString::to_string)
        else {
          return shared.toasts.warning(t("browse-no-instance"));
        };
        let Some(update) = self.updates.get(&mod_id).cloned() else {
          return Task::none();
        };
        return shared.start_op(
          target.clone(),
          OpKind::Update,
          move |lithic, reporter, cancel| {
            async move {
              let instance =
                lithic.instance(&target).map_err(|e| e.to_string())?;
              let opts = InstallOptions {
                dependencies: true,
                reporter,
                cancel,
                ..InstallOptions::default()
              };
              lithic
                .update_mods(&instance, &[update], &opts)
                .await
                .map(|r| Outcome::Mods(Summary::from(&r)))
                .map_err(|e| e.to_string())
            }
          },
        );
      },
      Message::OpenDetails(index) => {
        let Some(Ok(pool)) = &self.pool else {
          return Task::none();
        };
        let Some(summary) = pool.get(index).cloned() else {
          return Task::none();
        };
        let id = summary.id;
        self.details = Some(Details {
          summary,
          data: None,
          description: String::new(),
        });
        let lithic = shared.lithic.clone();
        return Task::perform(
          async move {
            lithic
              .moddb
              .mod_details(&id.to_string())
              .await
              .map(Box::new)
              .map_err(|e| e.to_string())
          },
          move |r| wrap(Message::DetailsLoaded(id, r)),
        );
      },
      Message::DetailsLoaded(id, result) => {
        if let Some(d) = &mut self.details
          && d.summary.id == id
        {
          if let Ok(details) = &result {
            d.description = moddb::html_to_plain(&details.text);
          }
          d.data = Some(result.map(|b| *b));
        }
      },
      Message::CloseDetails => self.details = None,
      Message::Open(url) => return Task::done(AppMessage::Open(url)),
    }
    Task::none()
  }

  /// The Browse page, with the instance to install into in its header.
  pub fn view<'a>(&'a self, shared: &'a Shared) -> Element<'a, Message> {
    let targets: Vec<TargetChoice> = shared
      .instances
      .iter()
      .map(|i| {
        TargetChoice {
          id:   i.id.clone(),
          name: i.name.clone(),
        }
      })
      .collect();
    let target = self
      .target_id(shared)
      .and_then(|id| targets.iter().find(|c| c.id == id).cloned());
    let picker: Element<Message> = if targets.is_empty() {
      space().into()
    } else {
      widget::tip(
        row![
          icon::icon(Icon::Instances, 18.0),
          widget::select(targets, target, Message::Target)
            .placeholder(t("browse-pick-instance"))
            .padding([7, 12]),
        ]
        .spacing(8)
        .align_y(Center),
        t("browse-install-into"),
      )
    };
    let page = widget::page(t("nav-browse"), picker, self.content(shared));
    widget::layered(
      page,
      self.details(shared).map(|d| (d, Message::CloseDetails)),
    )
  }

  /// Search controls and results, without a page frame.
  pub fn content<'a>(&'a self, shared: &'a Shared) -> Element<'a, Message> {
    let game = self.target_game(shared);
    let compatible_label = match game.and_then(version::minor) {
      Some((a, b)) => {
        t1("browse-compatible-with", "version", format!("{a}.{b}"))
      },
      None => t("browse-compatible"),
    };

    let mut filters = row![
      toggler(self.compatible && game.is_some())
        .label(compatible_label)
        .on_toggle_maybe(
          game
            .is_some()
            .then_some(Message::Compatible as fn(bool) -> Message)
        ),
      toggler(self.favorites_only)
        .label(t("browse-favorites-only"))
        .on_toggle(Message::FavoritesOnly),
      toggler(self.hide_installed)
        .label(t("browse-hide-installed"))
        .on_toggle(Message::HideInstalled),
      space::horizontal(),
    ]
    .spacing(24)
    .align_y(Center);
    if let Some(Ok(_)) = &self.pool {
      filters = filters.push(
        text(t1("browse-result-count", "count", self.results.len()))
          .size(12)
          .style(style::muted),
      );
    }
    let mut controls = column![
      row![
        widget::input(&t("browse-search"), &self.query)
          .on_input(Message::Query)
          .padding([9, 12])
          .width(Fill),
        widget::select(SortChoice::ALL, Some(self.sort), Message::Sort)
          .padding([9, 12]),
        widget::icon_button(
          Icon::Refresh,
          t("common-refresh"),
          Some(Message::Refresh)
        ),
      ]
      .spacing(8)
      .align_y(Center),
      filters,
    ]
    .spacing(10);

    let busy = self.target_id(shared).and_then(|id| shared.busy.get(id));
    let list: Element<Message> = match &self.pool {
      None => widget::loading(t("browse-loading")),
      Some(Err(e)) => {
        widget::empty(
          t("browse-failed"),
          e.clone(),
          Some(widget::secondary(t("common-retry"), Some(Message::Refresh))),
        )
      },
      Some(Ok(_)) if self.results.is_empty() => {
        widget::empty(t("browse-no-results"), t("browse-no-results-body"), None)
      },
      Some(Ok(pool)) => {
        let rows = self
          .results
          .iter()
          .take(self.shown)
          .map(|&i| self.mod_row(i, &pool[i], shared, busy.is_some()));
        let heading = container(
          row![
            text(t("browse-mod-heading")).width(Fill),
            text(t("browse-downloads-heading")).width(76),
            space().width(ACTIONS_WIDTH),
          ]
          .spacing(12),
        )
        .padding([4, 12]);
        let mut col = column![heading, column(rows).spacing(0)];
        if self.results.len() > self.shown {
          col = col.push(
            container(widget::secondary(
              t1("browse-more", "count", self.results.len() - self.shown),
              Some(Message::More),
            ))
            .center_x(Fill),
          );
        }
        scrollable(col).height(Fill).into()
      },
    };

    // Notices go at the end of `controls` rather than before the list, so
    // the list keeps its tree position and scroll offset when they appear.
    if shared.instances.is_empty() {
      controls = controls
        .push(widget::notice(text(t("browse-no-instances")), Tone::Warn));
    }
    if let Some(b) = busy
      && !self.locked
    {
      controls = controls.push(widget::notice(text(b.label()), Tone::Neutral));
    }
    let mut body = column![controls, list].spacing(12).height(Fill);
    if !self.selected.is_empty() {
      body = body.push(self.selection_bar(shared, busy.is_some()));
    }
    body.into()
  }

  fn selection_bar<'a>(
    &'a self,
    shared: &'a Shared,
    busy: bool,
  ) -> Element<'a, Message> {
    widget::card(
      row![
        text(t1("browse-selected", "count", self.selected.len())).width(Fill),
        widget::ghost(
          t("browse-clear-selection"),
          Some(Message::ClearSelection)
        ),
        widget::primary_icon(
          Icon::Download,
          t("browse-install-selected"),
          (!busy && self.target_id(shared).is_some())
            .then_some(Message::InstallSelected),
        ),
      ]
      .spacing(8)
      .align_y(Center),
    )
    .padding([8, 12])
    .into()
  }

  /// The open mod's details, to lay over whatever holds this view.
  pub fn details<'a>(
    &'a self,
    shared: &'a Shared,
  ) -> Option<Element<'a, Message>> {
    let busy = self
      .target_id(shared)
      .is_some_and(|id| shared.busy.contains_key(id));
    self
      .details
      .as_ref()
      .map(|d| self.details_view(d, shared, busy))
  }

  fn logo<'a>(
    shared: &'a Shared,
    url: Option<&'a String>,
    size: f32,
  ) -> Element<'a, Message> {
    match url.and_then(|u| shared.logos.get(u)) {
      Some(Logo::Ready(handle)) => {
        image(handle.clone()).width(size).height(size).into()
      },
      _ => {
        container(space())
          .width(size)
          .height(size)
          .style(style::placeholder)
          .into()
      },
    }
  }

  fn mod_row<'a>(
    &'a self,
    index: usize,
    m: &'a ModSummary,
    shared: &'a Shared,
    busy: bool,
  ) -> Element<'a, Message> {
    let mod_id = m.mod_ids.first().cloned().unwrap_or_default();
    let installed = m.mod_ids.iter().find_map(|id| self.installed.get(id));
    let update = m.mod_ids.iter().find_map(|id| self.updates.get(id));
    let favorite = m
      .mod_ids
      .iter()
      .any(|id| shared.settings.gui.favorites.contains(id));

    let mut meta = t1("browse-by", "author", m.author.clone());
    if let Some(version) = installed {
      meta.push_str("  |  ");
      meta.push_str(&t1("browse-installed", "version", version.clone()));
      if update.is_some() {
        meta.push_str("  |  ");
        meta.push_str(&t("browse-update-available"));
      }
    }
    if let Some(side) = m.side.as_deref().filter(|s| *s != "both") {
      meta.push_str("  |  ");
      meta.push_str(&t1("browse-side", "side", side.to_string()));
    }

    let can_act = !busy
      && self.target_id(shared).is_some()
      && (installed.is_none() || update.is_some());
    let label = match (installed, update) {
      (Some(_), Some(u)) => {
        t1(
          "browse-update-to",
          "version",
          u.release.version.clone().unwrap_or_default(),
        )
      },
      (Some(_), None) => t("mods-already-installed"),
      (None, _) => t("browse-install"),
    };
    let action = widget::icon_button(
      if update.is_some() {
        Icon::Update
      } else {
        Icon::Download
      },
      label,
      can_act.then(|| {
        update.map_or_else(
          || Message::Install(mod_id.clone(), None),
          |u| Message::Update(u.mod_id.clone()),
        )
      }),
    );
    let favorite_action = widget::tip(
      button(icon::colored(
        if favorite {
          Icon::StarFilled
        } else {
          Icon::Star
        },
        18.0,
        move |theme| {
          let p = theme.extended_palette();
          if favorite {
            p.warning.base.color
          } else {
            p.background.base.text
          }
        },
      ))
      .padding(7)
      .style(style::btn(Kind::Ghost))
      .on_press(Message::Favorite(mod_id.clone())),
      t(if favorite {
        "browse-unfavorite"
      } else {
        "browse-favorite"
      }),
    );

    let selected = self.selected.contains(&mod_id);
    let select = widget::tip(
      checkbox(selected).on_toggle_maybe(
        (installed.is_none() || selected)
          .then_some(move |on| Message::Select(mod_id.clone(), on)),
      ),
      t(if selected {
        "browse-unselect"
      } else {
        "browse-select"
      }),
    );

    container(
      row![
        select,
        Self::logo(shared, m.logo.as_ref(), 40.0),
        column![
          text(&m.name).size(15).font(widget::bold()),
          text(meta).size(12).style(style::muted),
          text(m.summary.as_deref().unwrap_or_default()).size(13),
        ]
        .spacing(2)
        .width(Fill),
        text(format_count(m.downloads))
          .size(12)
          .style(style::muted)
          .width(76)
          .align_x(Horizontal::Right),
        row![
          widget::icon_button(
            Icon::Info,
            t("browse-details"),
            Some(Message::OpenDetails(index))
          ),
          action,
          favorite_action,
        ]
        .spacing(4)
        .align_y(Center)
        .width(Length::Fixed(ACTIONS_WIDTH)),
      ]
      .spacing(12)
      .align_y(Center),
    )
    .padding([8, 12])
    .width(Fill)
    .style(style::list_row)
    .into()
  }

  fn details_view<'a>(
    &'a self,
    d: &'a Details,
    shared: &'a Shared,
    busy: bool,
  ) -> Element<'a, Message> {
    let m = &d.summary;
    let mod_id = m.mod_ids.first().cloned().unwrap_or_default();
    let header = row![
      Self::logo(shared, m.logo.as_ref(), 72.0),
      column![
        text(&m.name).size(22).font(widget::bold()),
        text(t1("browse-by", "author", m.author.clone()))
          .size(13)
          .style(style::muted),
        row![widget::link(
          t("browse-open-page"),
          Message::Open(m.page_url())
        ),]
        .spacing(4),
      ]
      .spacing(4)
      .width(Fill),
    ]
    .spacing(14)
    .align_y(Center);

    let body: Element<Message> = match &d.data {
      None => widget::loading(t("loading")),
      Some(Err(e)) => widget::notice(text(e.clone()), Tone::Bad),
      Some(Ok(details)) => {
        let target = Target {
          game_version:     self.target_game(shared).map(ToString::to_string),
          allow_prerelease: shared.settings.mods.allow_prerelease,
        };
        let installed = m.mod_ids.iter().find_map(|id| self.installed.get(id));
        let update = m.mod_ids.iter().find_map(|id| self.updates.get(id));
        let recommended = pick_release(details, &target, None);
        let verdict: Element<Message> = match &recommended {
          Ok(r) => {
            row![
              text(match installed {
                Some(v) if update.is_some() =>
                  format!(
                    "{}  |  {}",
                    t1("browse-installed", "version", v.clone()),
                    t("browse-update-available"),
                  ),
                Some(v) => t1("browse-installed", "version", v.clone()),
                None =>
                  t1(
                    "browse-would-install",
                    "version",
                    r.version.clone().unwrap_or_default(),
                  ),
              })
              .width(Fill),
              widget::primary_icon(
                if update.is_some() {
                  Icon::Update
                } else {
                  Icon::Download
                },
                if update.is_some() {
                  t("instance-update")
                } else if installed.is_some() {
                  t("mods-already-installed")
                } else {
                  t("browse-install")
                },
                (!busy
                  && self.target_id(shared).is_some()
                  && (installed.is_none() || update.is_some()))
                .then(|| {
                  update.map_or_else(
                    || Message::Install(mod_id.clone(), None),
                    |u| Message::Update(u.mod_id.clone()),
                  )
                }),
              ),
            ]
            .align_y(Center)
            .into()
          },
          Err(e) => text(e.to_string()).into(),
        };

        let mut links = row![].spacing(4);
        for (label, url) in [
          (t("browse-source"), &details.source_url),
          (t("browse-issues"), &details.issues_url),
          (t("browse-wiki"), &details.wiki_url),
          (t("browse-homepage"), &details.homepage_url),
        ] {
          if let Some(url) = url {
            links = links.push(widget::link(label, Message::Open(url.clone())));
          }
        }

        let releases = column(details.releases.iter().take(12).map(|r| {
          let v = r.version.clone().unwrap_or_default();
          let fits = target
            .game_version
            .as_deref()
            .map(|g| version::fit(&r.tags, g) >= version::Fit::SameMinor);
          let tone = match fits {
            Some(true) => Tone::Good,
            Some(false) => Tone::Bad,
            None => Tone::Neutral,
          };
          row![
            text(v.clone()).width(140),
            widget::badge(tag_range(&r.tags), tone),
            text(r.created.clone().unwrap_or_default())
              .size(12)
              .style(style::muted)
              .width(Fill),
            widget::icon_button(
              Icon::Download,
              t("browse-install-version"),
              (!busy && self.target_id(shared).is_some() && !v.is_empty())
                .then(|| Message::Install(mod_id.clone(), Some(v.clone()))),
            ),
          ]
          .spacing(8)
          .align_y(Center)
          .into()
        }))
        .spacing(6);

        column![
          widget::notice(
            verdict,
            if recommended.is_ok() {
              Tone::Neutral
            } else {
              Tone::Warn
            }
          ),
          links,
          text(d.description.clone()).size(13),
          row![
            text(t("browse-releases")).font(widget::bold()),
            widget::hint(t("browse-pin-hint")),
          ]
          .spacing(6)
          .align_y(Center),
          releases,
        ]
        .spacing(12)
        .into()
      },
    };

    container(
      column![header, scrollable(body).height(Length::Fixed(460.0)), row![
        space::horizontal(),
        widget::secondary(t("common-close"), Some(Message::CloseDetails))
      ],]
      .spacing(16),
    )
    .padding(24)
    .width(760)
    .style(style::dialog)
    .into()
  }
}

fn logo_task(
  http: Http,
  url: String,
  wrap: fn(Message) -> AppMessage,
) -> Task<AppMessage> {
  let key = url.clone();
  Task::perform(
    async move { http.get_bytes(&url).await.ok() },
    move |bytes| wrap(Message::Logo(key, bytes)),
  )
}

/// `1.21.0, 1.21.1, 1.21.2` as `1.21.0 to 1.21.2`.
fn tag_range(tags: &[String]) -> String {
  let mut sorted: Vec<&String> = tags.iter().collect();
  sorted.sort_by(|a, b| version::compare(a, b));
  match sorted.as_slice() {
    [] => t("browse-no-game-versions"),
    [one] => (*one).clone(),
    [first, .., last] => {
      t2(
        "browse-version-range",
        "from",
        (*first).clone(),
        "to",
        (*last).clone(),
      )
    },
  }
}

#[cfg(test)]
#[expect(
  clippy::unwrap_used,
  reason = "test setup and assertions intentionally fail on error"
)]
mod tests {
  use lithic_core::{Lithic, Paths};

  use super::*;

  fn summary(id: i64, name: &str, modid: &str, downloads: i64) -> ModSummary {
    ModSummary {
      id,
      name: name.into(),
      mod_ids: vec![modid.into()],
      downloads,
      ..ModSummary::default()
    }
  }

  fn shared() -> (tempfile::TempDir, Shared) {
    let dir = tempfile::tempdir().unwrap();
    let lithic = Lithic::new(Paths::rooted(dir.path())).unwrap();
    (dir, Shared::new(lithic))
  }

  #[test]
  fn stale_pool_responses_are_ignored() {
    let (_d, mut shared) = shared();
    let mut s = State::default();
    let _ = s.update(Message::Refresh, &mut shared);
    let _ = s.update(Message::Refresh, &mut shared);
    let old = vec![summary(1, "Old", "old", 1)];
    let _ = s.update(Message::PoolLoaded(1, None, Ok(old)), &mut shared);
    assert!(s.pool.is_none(), "an older request must not win");
    let new = vec![summary(2, "New", "new", 1)];
    let _ = s.update(Message::PoolLoaded(2, None, Ok(new)), &mut shared);
    assert_eq!(s.results.len(), 1);
  }

  #[test]
  fn search_favorites_and_paging() {
    let (_d, mut shared) = shared();
    let mut s = State::default();
    let pool: Vec<ModSummary> = (0..100)
      .map(|i| summary(i, &format!("Mod {i}"), &format!("m{i}"), i))
      .collect();
    s.request = 1;
    let _ = s.update(Message::PoolLoaded(1, None, Ok(pool)), &mut shared);
    assert_eq!(s.results.len(), 100);
    assert_eq!(
      s.results[0], 99,
      "most downloaded first when nothing is typed"
    );
    let _ = s.update(Message::More, &mut shared);
    assert_eq!(s.shown, PAGE * 2);

    let _ = s.update(Message::Query("mod 42".into()), &mut shared);
    assert_eq!(s.results.len(), 1);
    assert_eq!(s.shown, PAGE);

    let _ = s.update(Message::Query(String::new()), &mut shared);
    let _ = s.update(Message::Favorite("m7".into()), &mut shared);
    let _ = s.update(Message::FavoritesOnly(true), &mut shared);
    assert_eq!(s.results, [7]);
  }

  #[test]
  fn hide_installed_restores_results() {
    let (_dir, mut shared) = shared();
    let mut state = State {
      pool: Some(Ok(vec![
        summary(1, "Installed", "installed", 2),
        summary(2, "Available", "available", 1),
      ])),
      ..State::default()
    };
    state.installed.insert("installed".into(), "1.0".into());
    state.rerank(&shared);
    assert_eq!(state.results, [0, 1]);

    let _ = state.update(Message::HideInstalled(true), &mut shared);
    assert_eq!(state.results, [1]);
    let _ = state.update(Message::HideInstalled(false), &mut shared);
    assert_eq!(state.results, [0, 1]);
  }

  #[test]
  fn installing_keeps_paging_and_prunes_selection() {
    let (_dir, mut shared) = shared();
    let mut s = State::for_instance("main", AppMessage::Browse);
    let pool: Vec<ModSummary> = (0..100)
      .map(|i| summary(i, &format!("Mod {i}"), &format!("m{i}"), i))
      .collect();
    s.request = 1;
    let _ = s.update(Message::PoolLoaded(1, None, Ok(pool)), &mut shared);
    let _ = s.update(Message::More, &mut shared);
    let _ = s.update(Message::Select("m1".into(), true), &mut shared);
    let _ = s.update(Message::Select("m2".into(), true), &mut shared);

    let installed = HashMap::from([("m1".to_string(), "1.0".to_string())]);
    let _ = s.update(
      Message::Installed(Some("main".into()), Ok(installed)),
      &mut shared,
    );
    assert_eq!(s.shown, PAGE * 2, "an install must not collapse the list");
    assert_eq!(s.selected, BTreeSet::from(["m2".to_string()]));

    let _ = s.update(Message::ClearSelection, &mut shared);
    assert!(s.selected.is_empty());
  }

  #[test]
  fn locked_view_keeps_its_instance() {
    let (_dir, mut shared) = shared();
    let mut state = State::for_instance("main", AppMessage::Browse);
    let _ = state.update(
      Message::Target(TargetChoice {
        id:   "other".into(),
        name: "Other".into(),
      }),
      &mut shared,
    );
    assert_eq!(state.target.as_deref(), Some("main"));
  }

  #[test]
  fn version_ranges() {
    let tags =
      |t: &[&str]| t.iter().map(ToString::to_string).collect::<Vec<_>>();
    assert_eq!(
      tag_range(&tags(&["1.21.2", "1.21.0", "1.21.10"])),
      "1.21.0 to 1.21.10"
    );
    assert_eq!(tag_range(&tags(&["1.20.0"])), "1.20.0");
  }
}
