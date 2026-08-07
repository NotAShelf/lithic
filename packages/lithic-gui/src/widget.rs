//! Small building blocks shared by the screens.

use iced::widget::{button, center, column, container, mouse_area, opaque, row, space, stack, text};
use iced::{Center, Element, Fill, Font, font};

use crate::i18n::t;
use crate::style::{self, Tone};

pub fn bold() -> Font {
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

pub fn loading<'a, M: 'a>(label: impl text::IntoFragment<'a>) -> Element<'a, M> {
   center(text(label).style(style::muted)).padding(32).into()
}

/// A labelled form control with an optional hint below it.
pub fn field<'a, M: 'a>(
   label: impl text::IntoFragment<'a>,
   control: impl Into<Element<'a, M>>,
   hint: Option<String>,
) -> Element<'a, M> {
   let mut col = column![text(label).size(13).font(bold()), control.into()].spacing(6);
   if let Some(hint) = hint {
      col = col.push(text(hint).size(12).style(style::muted));
   }
   col.into()
}

pub fn badge<'a, M: 'a>(label: impl text::IntoFragment<'a>, tone: Tone) -> Element<'a, M> {
   container(text(label).size(11))
      .padding([2, 8])
      .style(style::badge(tone))
      .into()
}

pub fn notice<'a, M: 'a>(content: impl Into<Element<'a, M>>, tone: Tone) -> Element<'a, M> {
   container(content)
      .padding([10, 14])
      .width(Fill)
      .style(style::notice(tone))
      .into()
}

pub fn card<'a, M: 'a>(content: impl Into<Element<'a, M>>) -> container::Container<'a, M> {
   container(content).padding(16).width(Fill).style(style::card)
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
      opaque(mouse_area(center(opaque(dialog)).style(style::backdrop)).on_press(on_dismiss))
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
   confirm_label: impl text::IntoFragment<'a>,
   danger: bool,
   on_confirm: M,
   on_cancel: M,
) -> Element<'a, M> {
   let confirm = button(text(confirm_label))
      .padding([8, 16])
      .style(if danger { button::danger } else { button::primary })
      .on_press(on_confirm);
   dialog(
      title,
      text(body),
      row![
         button(text(t("common-cancel")))
            .padding([8, 16])
            .style(button::secondary)
            .on_press(on_cancel),
         confirm,
      ]
      .spacing(8),
      440.0,
   )
}

/// A primary action button that shows `busy_label` and does nothing while
/// `busy` is set.
pub fn action<'a, M: Clone + 'a>(
   label: String,
   busy_label: String,
   busy: bool,
   on_press: Option<M>,
) -> Element<'a, M> {
   let label = if busy { busy_label } else { label };
   button(text(label))
      .padding([8, 16])
      .style(button::primary)
      .on_press_maybe(if busy { None } else { on_press })
      .into()
}

pub fn secondary<'a, M: Clone + 'a>(label: String, on_press: Option<M>) -> Element<'a, M> {
   button(text(label))
      .padding([8, 16])
      .style(button::secondary)
      .on_press_maybe(on_press)
      .into()
}

pub fn link<'a, M: Clone + 'a>(label: String, on_press: M) -> Element<'a, M> {
   button(text(label).size(13))
      .padding([2, 4])
      .style(button::text)
      .on_press(on_press)
      .into()
}
