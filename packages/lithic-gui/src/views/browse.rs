use std::collections::{HashMap, HashSet};

use human_format::Formatter;
use iced::widget::{Column, button, column, container, pick_list, row, scrollable, text, text_input};
use iced::{Alignment, Color, Element, Fill};
use lithic_core::api::structs::ModApi;
use lithic_core::version::filter::VersionFilter;
use lithic_locale::{Localizer, ids};

use crate::app::Message;
use crate::widgets::{
   active_tab_style, card_style, danger_btn_style, ghost_btn_style, primary_btn_style, status_element,
};

#[derive(Debug, Clone, PartialEq, Default)]
pub enum SortBy {
   #[default]
   Downloads,
   Follows,
   Trending,
   Name,
}

impl SortBy {
   pub fn loc_label<'a>(&self, loc: &'a Localizer) -> std::borrow::Cow<'a, str> {
      match self {
         Self::Downloads => loc.get(ids::BROWSE_SORT_DOWNLOADS),
         Self::Follows => loc.get(ids::BROWSE_SORT_FOLLOWS),
         Self::Trending => loc.get(ids::BROWSE_SORT_TRENDING),
         Self::Name => loc.get(ids::BROWSE_SORT_NAME),
      }
   }
}

pub const PAGE_SIZE: usize = 30;

#[derive(Debug, Clone)]
pub struct BrowseView {
   pub query: String,
   pub all_mods: Vec<ModApi>,
   pub full_mods: Vec<ModApi>,
   pub filtered: Vec<ModApi>,
   pub loading: bool,
   pub status: Option<String>,
   pub sort_by: SortBy,
   pub sort_desc: bool,
   pub page: usize,
   pub favorites: HashSet<String>,
   pub show_favorites_only: bool,
   pub installing: HashSet<String>,
   pub installed_mods: HashMap<String, String>,
   pub confirm_delete: Option<String>,
   pub expanded_mod: Option<String>,
   pub version_filter: VersionFilter,
   pub available_minor_versions: Vec<String>,
}

impl Default for BrowseView {
   fn default() -> Self {
      Self {
         query: String::new(),
         all_mods: Vec::new(),
         full_mods: Vec::new(),
         filtered: Vec::new(),
         loading: false,
         status: None,
         sort_by: SortBy::Downloads,
         sort_desc: true,
         page: 0,
         favorites: HashSet::new(),
         show_favorites_only: false,
         installing: HashSet::new(),
         installed_mods: HashMap::new(),
         confirm_delete: None,
         expanded_mod: None,
         version_filter: VersionFilter::Any,
         available_minor_versions: Vec::new(),
      }
   }
}

impl BrowseView {
   pub fn total_pages(&self) -> usize {
      (self.filtered.len() + PAGE_SIZE - 1) / PAGE_SIZE
   }

   pub fn current_page_mods(&self) -> &[ModApi] {
      let start = self.page * PAGE_SIZE;
      let end = (start + PAGE_SIZE).min(self.filtered.len());
      &self.filtered[start..end]
   }
}

pub fn view<'a>(state: &'a BrowseView, loc: &'a Localizer) -> Element<'a, Message> {
   let export_btn: Element<'_, Message> = if !state.favorites.is_empty() {
      button(text(loc.get(ids::BROWSE_EXPORT_FAVORITES)).size(13))
         .on_press(Message::ExportFavorites)
         .style(ghost_btn_style)
         .into()
   } else {
      iced::widget::Space::new().into()
   };

   let refresh_btn = button(text(format!("↺ {}", loc.get(ids::BROWSE_REFRESH))).size(13))
      .on_press(Message::BrowseRefresh)
      .style(ghost_btn_style);

   let header = row![
      text(loc.get("browse-title")).size(22).width(Fill),
      export_btn,
      refresh_btn,
   ]
   .spacing(8)
   .align_y(Alignment::Center);

   let search_bar = row![
      text_input(loc.get(ids::BROWSE_SEARCH_PLACEHOLDER).as_ref(), &state.query)
         .on_input(Message::BrowseQueryChanged)
         .on_submit(Message::BrowseSearch)
         .width(Fill),
      button(text(loc.get("browse-search"))).on_press(Message::BrowseSearch),
   ]
   .spacing(8)
   .align_y(Alignment::Center);

   let version_controls = {
      let all_versions = loc.get("browse-all-versions").into_owned();
      let version_options: Vec<String> = std::iter::once(all_versions.clone())
         .chain(state.available_minor_versions.iter().cloned())
         .collect();

      let selected_label = match &state.version_filter {
         VersionFilter::Any => all_versions.clone(),
         VersionFilter::Exact(v) | VersionFilter::AtLeast(v) => v.clone(),
      };

      let current_is_at_least = matches!(state.version_filter, VersionFilter::AtLeast(_));
      let vlist = pick_list(version_options, Some(selected_label), move |s: String| {
         if s == all_versions {
            Message::BrowseVersionFilterChanged(VersionFilter::Any)
         } else if current_is_at_least {
            Message::BrowseVersionFilterChanged(VersionFilter::AtLeast(s))
         } else {
            Message::BrowseVersionFilterChanged(VersionFilter::Exact(s))
         }
      })
      .width(140);

      let mode_btn: Element<'_, Message> = match &state.version_filter {
         VersionFilter::Any => iced::widget::Space::new().into(),
         VersionFilter::Exact(v) => button(text("=").size(12))
            .on_press(Message::BrowseVersionFilterChanged(VersionFilter::AtLeast(
               v.clone(),
            )))
            .padding([5, 8])
            .style(active_tab_style)
            .into(),
         VersionFilter::AtLeast(v) => button(text("≥").size(12))
            .on_press(Message::BrowseVersionFilterChanged(VersionFilter::Exact(
               v.clone(),
            )))
            .padding([5, 8])
            .style(active_tab_style)
            .into(),
      };

      row![
         text(loc.get("browse-version-label")).size(12).color(Color {
            r: 0.55,
            g: 0.55,
            b: 0.55,
            a: 1.0,
         }),
         vlist,
         mode_btn,
      ]
      .spacing(6)
      .align_y(Alignment::Center)
   };

   let sort_controls = {
      let sorts = [SortBy::Downloads, SortBy::Follows, SortBy::Trending, SortBy::Name];
      let sort_btns = sorts
         .into_iter()
         .map(|s| sort_btn(s, &state.sort_by, state.sort_desc, loc));

      let fav_btn: Element<'_, Message> = {
         if state.show_favorites_only {
            let label = format!("★ {}", loc.get("browse-favorites-enabled"));
            button(text(label).size(12))
               .on_press(Message::ToggleFavoritesFilter)
               .padding([5, 10])
               .style(active_tab_style)
               .into()
         } else {
            let label = format!("☆ {}", loc.get("browse-favorites-disabled"));
            button(text(label).size(12))
               .on_press(Message::ToggleFavoritesFilter)
               .padding([5, 10])
               .style(ghost_btn_style)
               .into()
         }
      };

      let count_label = if state.loading {
         String::new()
      } else {
         match &state.version_filter {
            VersionFilter::Any => loc
               .get_with("browse-mod-count", "count", state.filtered.len().to_string())
               .into_owned(),
            VersionFilter::Exact(v) => loc
               .get_with2(
                  "browse-mod-count-version",
                  "count",
                  state.filtered.len().to_string(),
                  "version",
                  v.clone(),
               )
               .into_owned(),
            VersionFilter::AtLeast(v) => loc
               .get_with2(
                  "browse-mod-count-version-ge",
                  "count",
                  state.filtered.len().to_string(),
                  "version",
                  v.clone(),
               )
               .into_owned(),
         }
      };

      row(sort_btns.collect::<Vec<_>>())
         .push(fav_btn)
         .push(iced::widget::Space::new().width(Fill))
         .push(text(count_label).size(12).color(Color {
            r: 0.55,
            g: 0.55,
            b: 0.55,
            a: 1.0,
         }))
         .spacing(4)
         .align_y(Alignment::Center)
   };

   let body: Element<'_, Message> = if state.loading {
      container(text(loc.get("browse-fetching")).size(15))
         .center(Fill)
         .height(Fill)
         .into()
   } else if state.filtered.is_empty() {
      let msg = if state.show_favorites_only && state.favorites.is_empty() {
         loc.get("browse-no-favorites").into_owned()
      } else if !matches!(state.version_filter, VersionFilter::Any) && state.query.is_empty() {
         // `label()` returns `None` only for `Any`, which is excluded above.
         let label = state.version_filter.label().unwrap_or_default();
         loc.get_with("browse-no-results-version", "version", label).into_owned()
      } else if !state.query.is_empty() {
         loc.get("browse-no-query-results").into_owned()
      } else {
         loc.get(ids::BROWSE_NO_RESULTS).into_owned()
      };
      container(text(msg).size(14)).center(Fill).height(Fill).into()
   } else {
      let page_mods = state.current_page_mods();
      let rows: Vec<Element<'_, Message>> = page_mods
         .iter()
         .map(|m| {
            let key = mod_key(m);
            let favorited = state.favorites.contains(key.as_str());
            let installing = state.installing.contains(&key);
            let installed_file = state.installed_mods.get(&key).cloned();
            let pending_delete =
               state.confirm_delete.is_some() && state.confirm_delete.as_deref() == installed_file.as_deref();
            let expanded = state.expanded_mod.as_deref() == Some(key.as_str());
            browse_row(
               m,
               favorited,
               installing,
               installed_file,
               pending_delete,
               expanded,
               loc,
            )
         })
         .collect();

      let total = state.total_pages();
      let page_bar = row![
         button(text(format!("← {}", loc.get("browse-prev"))).size(12))
            .on_press_maybe(if state.page > 0 {
               Some(Message::BrowsePrevPage)
            } else {
               None
            })
            .style(ghost_btn_style),
         text(loc.get_with2(
            "browse-page",
            "current",
            (state.page + 1).to_string(),
            "total",
            total.max(1).to_string()
         ))
         .size(13),
         button(text(format!("{} →", loc.get("browse-next"))).size(12))
            .on_press_maybe(if state.page + 1 < total {
               Some(Message::BrowseNextPage)
            } else {
               None
            })
            .style(ghost_btn_style),
      ]
      .spacing(8)
      .align_y(Alignment::Center);

      column![
         scrollable(Column::with_children(rows).spacing(6)).height(Fill),
         page_bar,
      ]
      .spacing(8)
      .height(Fill)
      .into()
   };

   column![
      header,
      search_bar,
      version_controls,
      sort_controls,
      status_element(state.status.as_deref()),
      body,
   ]
   .spacing(10)
   .padding(16)
   .width(Fill)
   .height(Fill)
   .into()
}

fn sort_btn<'a>(sort: SortBy, current: &'a SortBy, desc: bool, loc: &'a Localizer) -> Element<'a, Message> {
   let active = &sort == current;
   let dir = if active {
      if desc { " ↓" } else { " ↑" }
   } else {
      ""
   };
   let label = format!("{}{}", sort.loc_label(loc), dir);
   let msg = if active {
      Message::BrowseSortToggle
   } else {
      Message::BrowseSortChanged(sort.clone())
   };
   if active {
      button(text(label).size(12))
         .on_press(msg)
         .padding([5, 10])
         .style(active_tab_style)
         .into()
   } else {
      button(text(label).size(12))
         .on_press(msg)
         .padding([5, 10])
         .style(ghost_btn_style)
         .into()
   }
}

fn mod_key(m: &ModApi) -> String {
   m.mod_id_strs
      .first()
      .cloned()
      .unwrap_or_else(|| m.mod_id.to_string())
}

fn browse_row(
   m: &ModApi,
   favorited: bool,
   installing: bool,
   installed_file: Option<String>,
   pending_delete: bool,
   expanded: bool,
   loc: &Localizer,
) -> Element<'static, Message> {
   let name = m.name.clone().unwrap_or_else(|| m.mod_id.to_string());
   let author = m.author.clone().unwrap_or_default();
   let summary = m.summary.clone().unwrap_or_default();
   let tags = m.tags.clone();
   let last_released = m.last_released.clone();
   let mod_id_str = mod_key(m);
   let fav_key = mod_id_str.clone();
   let dl_label = format_count(m.downloads);
   let fol_label = format_count(m.follows);

   let fav_color = if favorited {
      Color {
         r: 1.0,
         g: 0.84,
         b: 0.0,
         a: 1.0,
      }
   } else {
      Color {
         r: 0.45,
         g: 0.45,
         b: 0.45,
         a: 1.0,
      }
   };
   let fav_icon = if favorited { "★" } else { "☆" };

   let action_btn: Element<'static, Message> = if pending_delete {
      if let Some(file_name) = installed_file.clone() {
         let fname2 = file_name.clone();
         row![
            text(loc.get("browse-delete-confirm")).size(12).color(Color {
               r: 0.85,
               g: 0.35,
               b: 0.35,
               a: 1.0
            }),
            button(text(loc.get("browse-yes")).size(12))
               .on_press(Message::DeleteMod(fname2))
               .style(danger_btn_style),
            button(text(loc.get("browse-no")).size(12))
               .on_press(Message::CancelDelete)
               .style(ghost_btn_style),
         ]
         .spacing(6)
         .align_y(Alignment::Center)
         .into()
      } else {
         iced::widget::Space::new().into()
      }
   } else if installing {
      button(text(loc.get("browse-installing")).size(13))
         .style(ghost_btn_style)
         .into()
   } else if let Some(file_name) = installed_file {
      button(text(loc.get("browse-uninstall")).size(13))
         .on_press(Message::RequestDelete(file_name))
         .style(danger_btn_style)
         .into()
   } else {
      row![
         button(text(loc.get(ids::BROWSE_INSTALL)).size(13))
            .on_press(Message::InstallMod(mod_id_str.clone()))
            .style(primary_btn_style),
         button(text(loc.get(ids::BROWSE_ADD_TO_INSTANCE)).size(13))
            .on_press(Message::AddModToActiveInstance(mod_id_str.clone()))
            .style(ghost_btn_style),
      ]
      .spacing(6)
      .into()
   };

   let expand_key = mod_id_str.clone();
   let expand_icon = if expanded { "▼" } else { "▶" };
   let expand_btn = button(text(expand_icon).size(10).color(Color {
      r: 0.50,
      g: 0.50,
      b: 0.50,
      a: 1.0,
   }))
   .on_press(Message::ToggleBrowseDetail(expand_key))
   .style(|_: &iced::Theme, _| iced::widget::button::Style {
      background: None,
      ..Default::default()
   })
   .padding([2, 4]);

   let meta_row = row![
      text(loc.get_with("browse-by-author", "author", author.clone()))
         .size(12)
         .color(Color {
            r: 0.55,
            g: 0.55,
            b: 0.55,
            a: 1.0
         }),
      text("·").size(12).color(Color {
         r: 0.35,
         g: 0.35,
         b: 0.35,
         a: 1.0
      }),
      text(loc.get_with("browse-downloads", "count", dl_label.clone()))
         .size(12)
         .color(Color {
            r: 0.55,
            g: 0.55,
            b: 0.55,
            a: 1.0
         }),
      text("·").size(12).color(Color {
         r: 0.35,
         g: 0.35,
         b: 0.35,
         a: 1.0
      }),
      text(loc.get_with("browse-follows", "count", fol_label.clone()))
         .size(12)
         .color(Color {
            r: 0.55,
            g: 0.55,
            b: 0.55,
            a: 1.0
         }),
   ]
   .spacing(6)
   .align_y(Alignment::Center);

   let main_row: Element<'static, Message> = row![
      button(text(fav_icon).size(16).color(fav_color))
         .on_press(Message::ToggleFavorite(fav_key))
         .style(|_: &iced::Theme, _| iced::widget::button::Style {
            background: None,
            ..Default::default()
         })
         .padding([0, 4]),
      expand_btn,
      column![
         text(name).size(15),
         meta_row,
         text(summary).size(12).color(Color {
            r: 0.65,
            g: 0.65,
            b: 0.65,
            a: 1.0
         }),
      ]
      .spacing(3)
      .width(Fill),
      action_btn,
   ]
   .spacing(8)
   .align_y(Alignment::Center)
   .into();

   let body: Element<'static, Message> = if expanded {
      let tags_str = if tags.is_empty() {
         String::new()
      } else {
         loc.get_with("browse-tags", "tags", tags.join(", ")).into_owned()
      };
      let released_str = last_released
         .map(|d| {
            loc.get_with("browse-released", "date", d.to_string())
               .into_owned()
         })
         .unwrap_or_default();

      let detail_parts: Vec<String> = [tags_str, released_str]
         .into_iter()
         .filter(|s| !s.is_empty())
         .collect();

      if detail_parts.is_empty() {
         main_row
      } else {
         column![
            main_row,
            container(text(detail_parts.join("  ·  ")).size(11).color(Color {
               r: 0.50,
               g: 0.50,
               b: 0.50,
               a: 1.0
            }))
            .padding([4, 12]),
         ]
         .spacing(0)
         .into()
      }
   } else {
      main_row
   };

   container(body).padding([10, 12]).style(card_style).into()
}

fn format_count(n: i64) -> String {
   if n < 1000 {
      return n.to_string();
   }
   Formatter::new().with_separator("").format(n as f64)
}
