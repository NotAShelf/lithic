//! Small building blocks shared by the screens.

use std::borrow::Borrow;

use iced::{
  Center,
  Element,
  Fill,
  Font,
  font,
  widget::{
    PickList,
    TextInput,
    button,
    center,
    column,
    container,
    mouse_area,
    opaque,
    pick_list,
    row,
    rule,
    space,
    stack,
    text,
    text_input,
    tooltip,
  },
};
use lithic_icons::{self as icon, Icon};

use crate::{
  i18n::t,
  style::{self, Kind, Tone},
};

pub const fn bold() -> Font {
  Font {
    weight: font::Weight::Bold,
    ..Font::DEFAULT
  }
}

/// A page with a title, optional actions on the right, and its content.
pub fn page<'a, M: 'a>(
  title: impl text::IntoFragment<'a>,
  actions: impl Into<Element<'a, M>>,
  content: impl Into<Element<'a, M>>,
) -> Element<'a, M> {
  column![
    row![
      text(title).size(24).font(bold()),
      space::horizontal(),
      actions.into()
    ]
    .spacing(8)
    .align_y(Center),
    content.into(),
  ]
  .spacing(16)
  .padding(24)
  .height(Fill)
  .into()
}

/// Centered message for lists with nothing to show.
pub fn empty<'a, M: 'a>(
  title: impl text::IntoFragment<'a>,
  body: impl text::IntoFragment<'a>,
  action: Option<Element<'a, M>>,
) -> Element<'a, M> {
  let mut content = column![
    text(title).size(18).font(bold()),
    text(body).style(style::muted).align_x(Center),
  ]
  .spacing(8)
  .align_x(Center)
  .max_width(460);
  if let Some(action) = action {
    content = content.push(space().height(8)).push(action);
  }
  center(content).padding(32).into()
}

pub fn loading<'a, M: 'a>(
  label: impl text::IntoFragment<'a>,
) -> Element<'a, M> {
  center(text(label).style(style::muted)).padding(32).into()
}

/// A text input in the shared control style.
pub fn input<'a, M: Clone + 'a>(
  placeholder: &str,
  value: &str,
) -> TextInput<'a, M> {
  text_input(placeholder, value)
    .padding([7, 10])
    .style(style::input)
}

/// A drop-down in the shared control style.
pub fn select<'a, T, L, V, M>(
  options: L,
  selected: Option<V>,
  on_selected: impl Fn(T) -> M + 'a,
) -> PickList<'a, T, L, V, M>
where
  T: ToString + PartialEq + Clone + 'a,
  L: Borrow<[T]> + 'a,
  V: Borrow<T> + 'a,
  M: Clone + 'a,
{
  pick_list(options, selected, on_selected)
    .padding([7, 10])
    .style(style::select)
}

/// A labelled form control. `hint_text` shows on hover over a help icon.
pub fn field<'a, M: 'a>(
  label: impl text::IntoFragment<'a>,
  control: impl Into<Element<'a, M>>,
  hint_text: Option<String>,
) -> Element<'a, M> {
  let mut heading = row![text(label).size(13).font(bold())]
    .spacing(6)
    .align_y(Center);
  if let Some(h) = hint_text {
    heading = heading.push(hint(h));
  }
  column![heading, control.into()].spacing(6).into()
}

/// A short status next to a name, as colored text.
pub fn badge<'a, M: 'a>(
  label: impl text::IntoFragment<'a>,
  tone: Tone,
) -> Element<'a, M> {
  text(label).size(12).style(style::tone(tone)).into()
}

pub fn notice<'a, M: 'a>(
  content: impl Into<Element<'a, M>>,
  tone: Tone,
) -> Element<'a, M> {
  container(content)
    .padding([10, 14])
    .width(Fill)
    .style(style::notice(tone))
    .into()
}

pub fn card<'a, M: 'a>(
  content: impl Into<Element<'a, M>>,
) -> container::Container<'a, M> {
  container(content)
    .padding(16)
    .width(Fill)
    .style(style::card)
}

/// Lays `dialog` over `base`, dimming it. Clicking the dimmed area sends
/// `on_dismiss`.
pub fn modal<'a, M: Clone + 'a>(
  base: impl Into<Element<'a, M>>,
  dialog: impl Into<Element<'a, M>>,
  on_dismiss: M,
) -> Element<'a, M> {
  stack![
    base.into(),
    opaque(
      mouse_area(center(opaque(dialog)).style(style::backdrop))
        .on_press(on_dismiss)
    )
  ]
  .into()
}

/// The frame all dialogs share.
pub fn dialog<'a, M: 'a>(
  title: impl text::IntoFragment<'a>,
  body: impl Into<Element<'a, M>>,
  buttons: impl Into<Element<'a, M>>,
  width: f32,
) -> Element<'a, M> {
  container(
    column![
      text(title).size(20).font(bold()),
      body.into(),
      row![space::horizontal(), buttons.into()].spacing(8),
    ]
    .spacing(16),
  )
  .padding(24)
  .width(width)
  .style(style::dialog)
  .into()
}

/// A yes/no question. `danger` styles the confirm button as destructive.
pub fn confirm<'a, M: Clone + 'a>(
  title: impl text::IntoFragment<'a>,
  body: impl text::IntoFragment<'a>,
  confirm_label: String,
  danger: bool,
  on_confirm: M,
  on_cancel: M,
) -> Element<'a, M> {
  dialog(
    title,
    text(body),
    row![
      btn(None, t("common-cancel"), Kind::Ghost, Some(on_cancel)),
      btn(
        None,
        confirm_label,
        if danger { Kind::Danger } else { Kind::Primary },
        Some(on_confirm)
      ),
    ]
    .spacing(8),
    440.0,
  )
}

/// The button every screen uses. All kinds share one size and shape so they
/// line up next to each other.
pub fn btn<'a, M: Clone + 'a>(
  glyph: Option<Icon>,
  label: String,
  kind: Kind,
  on_press: Option<M>,
) -> Element<'a, M> {
  let enabled = on_press.is_some();
  let mut content = row![].spacing(8).align_y(Center);
  if let Some(glyph) = glyph {
    content = content.push(icon::colored(glyph, 16.0, move |theme| {
      let color = kind.text(theme);
      if enabled {
        color
      } else {
        color.scale_alpha(0.45)
      }
    }));
  }
  button(content.push(text(label).size(14)))
    .padding([7, 12])
    .style(style::btn(kind))
    .on_press_maybe(on_press)
    .into()
}

/// A primary action that shows `busy_label` and does nothing while `busy`
/// is set.
pub fn action<'a, M: Clone + 'a>(
  label: String,
  busy_label: String,
  busy: bool,
  on_press: Option<M>,
) -> Element<'a, M> {
  let label = if busy { busy_label } else { label };
  btn(
    None,
    label,
    Kind::Primary,
    if busy { None } else { on_press },
  )
}

pub fn secondary<'a, M: Clone + 'a>(
  label: String,
  on_press: Option<M>,
) -> Element<'a, M> {
  btn(None, label, Kind::Secondary, on_press)
}

pub fn primary_icon<'a, M: Clone + 'a>(
  glyph: Icon,
  label: String,
  on_press: Option<M>,
) -> Element<'a, M> {
  btn(Some(glyph), label, Kind::Primary, on_press)
}

pub fn secondary_icon<'a, M: Clone + 'a>(
  glyph: Icon,
  label: String,
  on_press: Option<M>,
) -> Element<'a, M> {
  btn(Some(glyph), label, Kind::Secondary, on_press)
}

/// A quiet way out, such as Cancel or Back.
pub fn ghost<'a, M: Clone + 'a>(
  label: String,
  on_press: Option<M>,
) -> Element<'a, M> {
  btn(None, label, Kind::Ghost, on_press)
}

/// Shows `label` in a tooltip while the pointer is over `content`.
pub fn tip<'a, M: 'a>(
  content: impl Into<Element<'a, M>>,
  label: impl text::IntoFragment<'a>,
) -> Element<'a, M> {
  tooltip(
    content,
    container(text(label).size(13))
      .max_width(320)
      .padding([6, 10])
      .style(style::tooltip),
    tooltip::Position::Top,
  )
  .gap(4)
  .into()
}

/// An icon-only action. `label` names it in a tooltip.
pub fn icon_button<'a, M: Clone + 'a>(
  icon: Icon,
  label: String,
  on_press: Option<M>,
) -> Element<'a, M> {
  let enabled = on_press.is_some();
  tip(
    button(icon::colored(icon, 16.0, move |theme| {
      let color = Kind::Ghost.text(theme);
      if enabled {
        color
      } else {
        color.scale_alpha(0.35)
      }
    }))
    .padding(8)
    .style(style::btn(Kind::Ghost))
    .on_press_maybe(on_press),
    label,
  )
}

/// A help icon that explains something in a tooltip.
pub fn hint<'a, M: 'a>(label: String) -> Element<'a, M> {
  tip(
    icon::colored(Icon::Help, 15.0, |theme| {
      theme
        .extended_palette()
        .background
        .base
        .text
        .scale_alpha(0.55)
    }),
    label,
  )
}

/// A setting with its label on the left and its control on the right.
pub fn setting_row<'a, M: 'a>(
  label: impl text::IntoFragment<'a>,
  hint_text: Option<String>,
  control: impl Into<Element<'a, M>>,
) -> Element<'a, M> {
  let mut name = row![text(label).size(14)].spacing(6).align_y(Center);
  if let Some(h) = hint_text {
    name = name.push(hint(h));
  }
  row![container(name).width(Fill), control.into()]
    .spacing(16)
    .align_y(Center)
    .padding([10, 0])
    .into()
}

/// Stacks `items` with thin lines between them.
pub fn divided<'a, M: 'a>(
  items: impl IntoIterator<Item = Element<'a, M>>,
) -> Element<'a, M> {
  let mut col = column![];
  for (i, item) in items.into_iter().enumerate() {
    if i > 0 {
      col = col.push(rule::horizontal(1).style(style::divider));
    }
    col = col.push(item);
  }
  col.into()
}

pub fn link<'a, M: Clone + 'a>(label: String, on_press: M) -> Element<'a, M> {
  button(text(label).size(13))
    .padding([2, 4])
    .style(button::text)
    .on_press(on_press)
    .into()
}
