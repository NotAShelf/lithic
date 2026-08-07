//! Toast notifications. Errors and warnings stay until dismissed; success and
//! info messages expire on their own. Expiry is keyed by id, so an old timer
//! never removes a newer toast.

use std::time::Duration;

use iced::widget::{button, column, container, row, text};
use iced::{Element, Fill, Task};
use tokio::time::sleep;

use crate::app::Message as AppMessage;
use crate::i18n::t;
use crate::style::{self, Tone};

const EXPIRE_AFTER: Duration = Duration::from_secs(5);
const MAX_SHOWN: usize = 5;

#[derive(Debug, Clone, Copy)]
pub enum Message {
   Dismiss(u64),
   Expire(u64),
   Toggle(u64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
   Error,
   Warning,
   Success,
   Info,
}

impl Severity {
   const fn sticky(self) -> bool {
      matches!(self, Self::Error | Self::Warning)
   }

   const fn tone(self) -> Tone {
      match self {
         Self::Error => Tone::Bad,
         Self::Warning => Tone::Warn,
         Self::Success => Tone::Good,
         Self::Info => Tone::Accent,
      }
   }
}

#[derive(Debug, Clone)]
struct Toast {
   id: u64,
   severity: Severity,
   title: String,
   detail: Option<String>,
   expanded: bool,
}

#[derive(Debug, Default)]
pub struct Notifications {
   items: Vec<Toast>,
   next_id: u64,
}

impl Notifications {
   pub fn push(
      &mut self,
      severity: Severity,
      title: impl Into<String>,
      detail: Option<String>,
   ) -> Task<AppMessage> {
      let title = title.into();
      if self
         .items
         .last()
         .is_some_and(|last| last.severity == severity && last.title == title && last.detail == detail)
      {
         return Task::none();
      }
      let id = self.next_id;
      self.next_id = self.next_id.wrapping_add(1);
      self.items.push(Toast {
         id,
         severity,
         title,
         detail,
         expanded: false,
      });
      while self.items.len() > MAX_SHOWN {
         let oldest = self.items.iter().position(|n| !n.severity.sticky()).unwrap_or(0);
         self.items.remove(oldest);
      }
      if severity.sticky() {
         Task::none()
      } else {
         Task::perform(async { sleep(EXPIRE_AFTER).await }, move |()| {
            AppMessage::Toast(Message::Expire(id))
         })
      }
   }

   pub fn error(&mut self, title: impl Into<String>, detail: Option<String>) -> Task<AppMessage> {
      self.push(Severity::Error, title, detail)
   }

   pub fn warning(&mut self, title: impl Into<String>) -> Task<AppMessage> {
      self.push(Severity::Warning, title, None)
   }

   pub fn success(&mut self, title: impl Into<String>) -> Task<AppMessage> {
      self.push(Severity::Success, title, None)
   }

   pub fn info(&mut self, title: impl Into<String>) -> Task<AppMessage> {
      self.push(Severity::Info, title, None)
   }

   pub fn update(&mut self, message: Message) {
      match message {
         Message::Dismiss(id) | Message::Expire(id) => self.items.retain(|n| n.id != id),
         Message::Toggle(id) => {
            if let Some(n) = self.items.iter_mut().find(|n| n.id == id) {
               n.expanded = !n.expanded;
            }
         }
      }
   }

   pub const fn is_empty(&self) -> bool {
      self.items.is_empty()
   }

   pub fn view(&self) -> Element<'_, Message> {
      let toasts = self.items.iter().map(|n| {
         let mut actions = row![].spacing(4);
         if n.detail.is_some() {
            let label = if n.expanded {
               t("toast-hide-details")
            } else {
               t("toast-show-details")
            };
            actions = actions.push(
               button(text(label).size(12))
                  .style(button::text)
                  .on_press(Message::Toggle(n.id)),
            );
         }
         actions = actions.push(
            button(text(t("toast-dismiss")).size(12))
               .style(button::text)
               .on_press(Message::Dismiss(n.id)),
         );

         let mut body = column![row![text(&n.title).size(14).width(Fill), actions].spacing(8)].spacing(6);
         if n.expanded
            && let Some(detail) = &n.detail
         {
            body = body.push(text(detail).size(12).style(style::muted));
         }
         container(body)
            .padding([10, 12])
            .width(360)
            .style(style::toast(n.severity.tone()))
            .into()
      });
      column(toasts).spacing(8).into()
   }
}

#[cfg(test)]
mod tests {
   use super::*;

   #[test]
   fn duplicates_collapse_and_queue_is_bounded() {
      let mut n = Notifications::default();
      let _ = n.error("same", None);
      let _ = n.error("same", None);
      assert_eq!(n.items.len(), 1);
      for i in 0..10 {
         let _ = n.info(format!("info {i}"));
      }
      assert_eq!(n.items.len(), MAX_SHOWN);
      assert!(
         n.items.iter().any(|t| t.title == "same"),
         "sticky errors outlive info toasts"
      );
   }

   #[test]
   fn expiry_only_removes_its_own_toast() {
      let mut n = Notifications::default();
      let _ = n.info("first");
      let _ = n.info("second");
      n.update(Message::Expire(0));
      assert_eq!(n.items.len(), 1);
      assert_eq!(n.items[0].title, "second");
      n.update(Message::Expire(0));
      assert_eq!(n.items.len(), 1);
   }
}
