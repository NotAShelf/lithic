use std::collections::HashMap;
use std::fmt;
use std::sync::LazyLock;

use iced::alignment::Horizontal;
use iced::widget::{
   button, column, container, image, pick_list, row, scrollable, space, svg, text, text_input, toggler,
   tooltip,
};
use iced::{Center, Color, Element, Fill, Length, Task, Theme};
use lithic_core::http::Http;
use lithic_core::moddb::{self, ModDetails, ModSummary, Query, Sort};
use lithic_core::mods::resolve::{Target, pick_release};
use lithic_core::mods::{InstallOptions, ModRef};
use lithic_core::{Freshness, version};

use super::format_count;
use crate::app::{Message as AppMessage, OpKind, Outcome, Shared, Summary};
use crate::i18n::{t, t1, t2};
use crate::style::{self, Tone};
use crate::task::blocking;
use crate::widget;

const PAGE: usize = 40;
const INSTALL_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M12 3v12m0 0 4-4m-4 4-4-4M4 17v3a1 1 0 0 0 1 1h14a1 1 0 0 0 1-1v-3"/></svg>"#;
static INSTALL_HANDLE: LazyLock<svg::Handle> = LazyLock::new(|| svg::Handle::from_memory(INSTALL_SVG));

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
   pub(crate) id: String,
   pub(crate) name: String,
}

impl fmt::Display for TargetChoice {
   fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
      f.write_str(&self.name)
   }
}

#[derive(Debug, Clone)]
enum Logo {
   Loading,
   Ready(image::Handle),
   Failed,
}

#[derive(Debug)]
struct Details {
   summary: ModSummary,
   data: Option<Result<ModDetails, String>>,
   description: String,
}

/// `None` fetches all mods; `Some` restricts releases to a major.minor game version.
type PoolKey = Option<(u64, u64)>;

#[derive(Debug)]
pub struct State {
   target: Option<String>,
   query: String,
   sort: SortChoice,
   compatible: bool,
   favorites_only: bool,
   pool: Option<Result<Vec<ModSummary>, String>>,
   pool_key: Option<PoolKey>,
   request: u64,
   results: Vec<usize>,
   shown: usize,
   installed: HashMap<String, String>,
   logos: HashMap<String, Logo>,
   details: Option<Details>,
}

impl Default for State {
   fn default() -> Self {
      Self {
         target: None,
         query: String::new(),
         sort: SortChoice(Sort::Downloads),
         compatible: true,
         favorites_only: false,
         pool: None,
         pool_key: None,
         request: 0,
         results: Vec::new(),
         shown: PAGE,
         installed: HashMap::new(),
         logos: HashMap::new(),
         details: None,
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
   Refresh,
   PoolLoaded(u64, PoolKey, Result<Vec<ModSummary>, String>),
   Installed(Option<String>, Result<HashMap<String, String>, String>),
   Logo(String, Option<Vec<u8>>),
   More,
   Favorite(String),
   FavoriteSaved(Result<(), String>),
   Install(String, Option<String>),
   OpenDetails(usize),
   DetailsLoaded(i64, Result<Box<ModDetails>, String>),
   CloseDetails,
   Open(String),
}

pub fn enter(state: &State, shared: &Shared) -> Task<AppMessage> {
   let key = state.wanted_key(shared);
   let mut tasks = vec![refresh_installed(state, shared)];
   if state.pool_key != Some(key) || matches!(state.pool, Some(Err(_))) {
      tasks.push(Task::done(AppMessage::Browse(Message::Refresh)));
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
   let target = Some(id.clone());
   Task::perform(
      blocking(move || {
         let instance = lithic.instance(&id)?;
         Ok(lithic
            .installed_mods(&instance)?
            .into_iter()
            .map(|m| (m.info.mod_id, m.info.version))
            .collect())
      }),
      move |r| AppMessage::Browse(Message::Installed(target, r)),
   )
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
      let sort = if self.query.trim().is_empty() && self.sort.0 == Sort::Relevance {
         Sort::Downloads
      } else {
         self.sort.0
      };
      let favorites = &shared.settings.gui.favorites;
      self.results = moddb::rank(pool, &self.query, sort)
         .into_iter()
         .filter(|&i| !pool[i].mod_ids.is_empty())
         .filter(|&i| !self.favorites_only || pool[i].mod_ids.iter().any(|id| favorites.contains(id)))
         .collect();
      self.shown = PAGE;
   }

   fn load_logos(&mut self, shared: &Shared) -> Task<AppMessage> {
      let Some(Ok(pool)) = &self.pool else {
         return Task::none();
      };
      let wanted: Vec<String> = self
         .results
         .iter()
         .take(self.shown)
         .filter_map(|&i| pool[i].logo.clone())
         .filter(|url| !self.logos.contains_key(url))
         .collect();
      let mut tasks = Vec::new();
      for url in wanted {
         self.logos.insert(url.clone(), Logo::Loading);
         tasks.push(logo_task(shared.lithic.http.clone(), url));
      }
      Task::batch(tasks)
   }

   pub fn update(&mut self, message: Message, shared: &mut Shared) -> Task<AppMessage> {
      match message {
         Message::Target(choice) => {
            self.set_target(Some(choice.id), shared);
            return enter(self, shared);
         }
         Message::Query(q) => {
            self.query = q;
            self.rerank(shared);
            return self.load_logos(shared);
         }
         Message::Sort(s) => {
            self.sort = s;
            self.rerank(shared);
            return self.load_logos(shared);
         }
         Message::Compatible(on) => {
            self.compatible = on;
            return enter(self, shared);
         }
         Message::FavoritesOnly(on) => {
            self.favorites_only = on;
            self.rerank(shared);
            return self.load_logos(shared);
         }
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
                     }
                     Some(minor) => match lithic.moddb.game_versions().await {
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
                        }
                        Err(e) => Err(e),
                     },
                  };
                  result.map_err(|e| e.to_string())
               },
               move |r| AppMessage::Browse(Message::PoolLoaded(request, key, r)),
            );
         }
         Message::PoolLoaded(request, key, result) => {
            if request != self.request {
               return Task::none();
            }
            self.pool_key = Some(key);
            self.pool = Some(result);
            self.rerank(shared);
            return self.load_logos(shared);
         }
         Message::Installed(target, result) => {
            if target.as_deref() != self.target_id(shared) {
               return Task::none();
            }
            match result {
               Ok(map) => self.installed = map,
               Err(e) => return shared.toasts.error(t("browse-installed-failed"), Some(e)),
            }
         }
         Message::Logo(url, bytes) => {
            let logo = bytes.map_or(Logo::Failed, |b| Logo::Ready(image::Handle::from_bytes(b)));
            self.logos.insert(url, logo);
         }
         Message::More => {
            self.shown += PAGE;
            return self.load_logos(shared);
         }
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
               |r| AppMessage::Browse(Message::FavoriteSaved(r)),
            );
         }
         Message::FavoriteSaved(Ok(())) => {}
         Message::FavoriteSaved(Err(e)) => return shared.toasts.error(t("browse-favorite-failed"), Some(e)),
         Message::Install(mod_id, version) => {
            let Some(target) = self.target_id(shared).map(ToString::to_string) else {
               return shared.toasts.warning(t("browse-no-instance"));
            };
            let pin = version.is_some();
            let refs = vec![ModRef { id: mod_id, version }];
            return shared.start_op(
               target.clone(),
               OpKind::Install,
               move |lithic, reporter, cancel| async move {
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
               },
            );
         }
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
               move |r| AppMessage::Browse(Message::DetailsLoaded(id, r)),
            );
         }
         Message::DetailsLoaded(id, result) => {
            if let Some(d) = &mut self.details
               && d.summary.id == id
            {
               if let Ok(details) = &result {
                  d.description = moddb::html_to_plain(&details.text);
               }
               d.data = Some(result.map(|b| *b));
            }
         }
         Message::CloseDetails => self.details = None,
         Message::Open(url) => return Task::done(AppMessage::Open(url)),
      }
      Task::none()
   }

   pub fn view<'a>(&'a self, shared: &'a Shared) -> Element<'a, Message> {
      let targets: Vec<TargetChoice> = shared
         .instances
         .iter()
         .map(|i| TargetChoice {
            id: i.id.clone(),
            name: i.name.clone(),
         })
         .collect();
      let target = self
         .target_id(shared)
         .and_then(|id| targets.iter().find(|c| c.id == id).cloned());
      let game = self.target_game(shared);
      let compatible_label = match game.and_then(version::minor) {
         Some((a, b)) => t1("browse-compatible-with", "version", format!("{a}.{b}")),
         None => t("browse-compatible"),
      };

      let controls = column![
         row![
            text_input(&t("browse-search"), &self.query)
               .on_input(Message::Query)
               .padding([9, 12])
               .width(Fill),
            pick_list(SortChoice::ALL, Some(self.sort), Message::Sort).padding([9, 12]),
            button(text(t("common-refresh")))
               .padding([9, 12])
               .style(style::nav(false))
               .on_press(Message::Refresh),
         ]
         .spacing(8)
         .align_y(Center),
         row![
            text(t("browse-install-into")).size(13),
            pick_list(targets, target, Message::Target).placeholder(t("browse-pick-instance")),
         ]
         .spacing(12)
         .align_y(Center),
         row![
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
         ]
         .spacing(24)
         .align_y(Center),
      ]
      .spacing(10);

      let busy = self.target_id(shared).and_then(|id| shared.busy.get(id));
      let list: Element<Message> = match &self.pool {
         None => widget::loading(t("browse-loading")),
         Some(Err(e)) => widget::empty(
            t("browse-failed"),
            e.clone(),
            Some(widget::secondary(t("common-retry"), Some(Message::Refresh))),
         ),
         Some(Ok(_)) if self.results.is_empty() => {
            widget::empty(t("browse-no-results"), t("browse-no-results-body"), None)
         }
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
                  space().width(128),
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
         }
      };

      let mut body = column![controls].spacing(12);
      if shared.instances.is_empty() {
         body = body.push(widget::notice(text(t("browse-no-instances")), Tone::Warn));
      }
      if let Some(b) = busy {
         body = body.push(widget::notice(text(b.label()), Tone::Neutral));
      }
      if let Some(Ok(_)) = &self.pool {
         body = body.push(
            text(t1("browse-result-count", "count", self.results.len()))
               .size(12)
               .style(style::muted),
         );
      }
      body = body.push(list);

      let page = widget::page(t("nav-browse"), space(), body);
      match &self.details {
         Some(d) => widget::modal(
            page,
            self.details_view(d, shared, busy.is_some()),
            Message::CloseDetails,
         ),
         None => page,
      }
   }

   fn logo<'a>(&'a self, url: Option<&'a String>, size: f32) -> Element<'a, Message> {
      match url.and_then(|u| self.logos.get(u)) {
         Some(Logo::Ready(handle)) => image(handle.clone()).width(size).height(size).into(),
         _ => container(space())
            .width(size)
            .height(size)
            .style(style::badge(Tone::Neutral))
            .into(),
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
      let favorite = m
         .mod_ids
         .iter()
         .any(|id| shared.settings.gui.favorites.contains(id));

      let mut meta = t1("browse-by", "author", m.author.clone());
      if favorite {
         meta.push_str("  |  ");
         meta.push_str(&t("browse-favorite"));
      }
      if let Some(version) = installed {
         meta.push_str("  |  ");
         meta.push_str(&t1("browse-installed", "version", version.clone()));
      }
      if let Some(side) = m.side.as_deref().filter(|s| *s != "both") {
         meta.push_str("  |  ");
         meta.push_str(&t1("browse-side", "side", side.to_string()));
      }

      let can_install = !busy && self.target_id(shared).is_some();
      let action: Element<Message> = match installed {
         Some(_) => space().width(36).height(36).into(),
         None => tooltip(
            button(
               svg((*INSTALL_HANDLE).clone())
                  .width(20)
                  .height(20)
                  .style(move |theme: &Theme, _| {
                     let p = theme.extended_palette();
                     let text = p.background.base.text;
                     let color = if can_install {
                        text
                     } else {
                        let bg = p.background.weakest.color;
                        Color::from_rgb(
                           bg.r.mul_add(0.65, text.r * 0.35),
                           bg.g.mul_add(0.65, text.g * 0.35),
                           bg.b.mul_add(0.65, text.b * 0.35),
                        )
                     };
                     svg::Style { color: Some(color) }
                  }),
            )
            .width(36)
            .height(36)
            .padding(8)
            .style(style::nav(false))
            .on_press_maybe(can_install.then(|| Message::Install(mod_id.clone(), None))),
            text(t("browse-install")).size(13),
            tooltip::Position::Top,
         )
         .style(container::rounded_box)
         .into(),
      };

      container(
         row![
            self.logo(m.logo.as_ref(), 40.0),
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
               button(text(t("browse-details")).size(13))
                  .width(Length::Fixed(64.0))
                  .padding([8, 4])
                  .style(button::text)
                  .on_press(Message::OpenDetails(index)),
               action,
            ]
            .spacing(8)
            .align_y(Center)
            .width(Length::Fixed(128.0)),
         ]
         .spacing(12)
         .align_y(Center),
      )
      .padding([8, 12])
      .width(Fill)
      .style(style::list_row)
      .into()
   }

   fn details_view<'a>(&'a self, d: &'a Details, shared: &'a Shared, busy: bool) -> Element<'a, Message> {
      let m = &d.summary;
      let mod_id = m.mod_ids.first().cloned().unwrap_or_default();
      let favorite = m
         .mod_ids
         .iter()
         .any(|id| shared.settings.gui.favorites.contains(id));
      let header = row![
         self.logo(m.logo.as_ref(), 72.0),
         column![
            text(&m.name).size(22).font(widget::bold()),
            text(t1("browse-by", "author", m.author.clone()))
               .size(13)
               .style(style::muted),
            row![widget::link(t("browse-open-page"), Message::Open(m.page_url())),].spacing(4),
         ]
         .spacing(4)
         .width(Fill),
         button(text(if favorite {
            t("browse-unfavorite")
         } else {
            t("browse-favorite")
         }))
         .style(button::secondary)
         .on_press(Message::Favorite(mod_id.clone())),
      ]
      .spacing(14)
      .align_y(Center);

      let body: Element<Message> = match &d.data {
         None => widget::loading(t("loading")),
         Some(Err(e)) => widget::notice(text(e.clone()), Tone::Bad),
         Some(Ok(details)) => {
            let target = Target {
               game_version: self.target_game(shared).map(ToString::to_string),
               allow_prerelease: shared.settings.mods.allow_prerelease,
            };
            let recommended = pick_release(details, &target, None);
            let verdict: Element<Message> = match &recommended {
               Ok(r) => row![
                  text(t1(
                     "browse-would-install",
                     "version",
                     r.version.clone().unwrap_or_default()
                  ))
                  .width(Fill),
                  button(text(t("browse-install")))
                     .padding([6, 14])
                     .style(button::primary)
                     .on_press_maybe(
                        (!busy && self.target_id(shared).is_some())
                           .then(|| Message::Install(mod_id.clone(), None)),
                     ),
               ]
               .align_y(Center)
               .into(),
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
                  button(text(t("browse-install-version")).size(12))
                     .style(button::text)
                     .on_press_maybe(
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
               text(t("browse-releases")).font(widget::bold()),
               releases,
               text(t("browse-pin-hint")).size(12).style(style::muted),
            ]
            .spacing(12)
            .into()
         }
      };

      container(
         column![
            header,
            scrollable(body).height(Length::Fixed(460.0)),
            row![
               space::horizontal(),
               widget::secondary(t("common-close"), Some(Message::CloseDetails))
            ],
         ]
         .spacing(16),
      )
      .padding(24)
      .width(760)
      .style(style::dialog)
      .into()
   }
}

fn logo_task(http: Http, url: String) -> Task<AppMessage> {
   let key = url.clone();
   Task::perform(async move { http.get_bytes(&url).await.ok() }, move |bytes| {
      AppMessage::Browse(Message::Logo(key, bytes))
   })
}

/// `1.21.0, 1.21.1, 1.21.2` as `1.21.0 to 1.21.2`.
fn tag_range(tags: &[String]) -> String {
   let mut sorted: Vec<&String> = tags.iter().collect();
   sorted.sort_by(|a, b| version::compare(a, b));
   match sorted.as_slice() {
      [] => t("browse-no-game-versions"),
      [one] => (*one).clone(),
      [first, .., last] => t2(
         "browse-version-range",
         "from",
         (*first).clone(),
         "to",
         (*last).clone(),
      ),
   }
}

#[cfg(test)]
#[expect(
   clippy::unwrap_used,
   reason = "test setup and assertions intentionally fail on error"
)]
mod tests {
   use super::*;
   use lithic_core::{Lithic, Paths};

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
      assert_eq!(s.results[0], 99, "most downloaded first when nothing is typed");
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
   fn version_ranges() {
      let tags = |t: &[&str]| t.iter().map(ToString::to_string).collect::<Vec<_>>();
      assert_eq!(
         tag_range(&tags(&["1.21.2", "1.21.0", "1.21.10"])),
         "1.21.0 to 1.21.10"
      );
      assert_eq!(tag_range(&tags(&["1.20.0"])), "1.20.0");
   }
}
