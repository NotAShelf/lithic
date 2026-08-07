use iced::widget::{button, column, row, scrollable, space, text, text_input};
use iced::{Center, Element, Fill, Task};
use lithic_core::Error;
use lithic_core::auth::{Account, AuthError};

use crate::app::{Message as AppMessage, Shared};
use crate::i18n::{t, t1};
use crate::style::{self, Tone};
use crate::task::blocking;
use crate::widget;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Step {
   Credentials,
   Code { token: String },
}

#[derive(Debug)]
pub struct State {
   email: String,
   password: String,
   code: String,
   step: Step,
   submitting: bool,
   error: Option<String>,
   confirm_logout: Option<Account>,
}

impl Default for State {
   fn default() -> Self {
      Self {
         email: String::new(),
         password: String::new(),
         code: String::new(),
         step: Step::Credentials,
         submitting: false,
         error: None,
         confirm_logout: None,
      }
   }
}

#[derive(Debug, Clone)]
pub enum LoginResult {
   Done(Account),
   NeedsCode(String),
   Failed(String),
}

#[derive(Debug, Clone)]
pub enum Message {
   Email(String),
   Password(String),
   Code(String),
   Submit,
   Back,
   Result(LoginResult),
   MakeActive(String),
   AskLogout(Account),
   CancelLogout,
   Logout,
   Done(Result<(), String>),
}

impl State {
   pub fn update(&mut self, message: Message, shared: &mut Shared) -> Task<AppMessage> {
      match message {
         Message::Email(v) => self.email = v,
         Message::Password(v) => self.password = v,
         Message::Code(v) => self.code = v.chars().filter(char::is_ascii_digit).take(8).collect(),
         Message::Submit => {
            if self.submitting || !self.can_submit() {
               return Task::none();
            }
            self.submitting = true;
            self.error = None;
            let lithic = shared.lithic.clone();
            let (email, password) = (self.email.trim().to_string(), self.password.clone());
            let twofa = match &self.step {
               Step::Code { token } => Some((token.clone(), self.code.clone())),
               Step::Credentials => None,
            };
            return Task::perform(
               async move {
                  let twofa = twofa.as_ref().map(|(tk, c)| (tk.as_str(), c.as_str()));
                  match lithic.login(&email, &password, twofa).await {
                     Ok(account) => LoginResult::Done(account),
                     Err(Error::Auth(AuthError::TwoFactorRequired { prelogintoken })) => {
                        LoginResult::NeedsCode(prelogintoken)
                     }
                     Err(e) => LoginResult::Failed(e.to_string()),
                  }
               },
               |r| AppMessage::Accounts(Message::Result(r)),
            );
         }
         Message::Back => {
            self.step = Step::Credentials;
            self.code.clear();
            self.error = None;
         }
         Message::Result(result) => {
            self.submitting = false;
            match result {
               LoginResult::Done(account) => {
                  *self = Self::default();
                  return Task::batch([
                     shared
                        .toasts
                        .success(t1("accounts-logged-in", "name", account.playername)),
                     Task::done(AppMessage::Reload),
                  ]);
               }
               LoginResult::NeedsCode(token) => {
                  self.step = Step::Code { token };
                  self.code.clear();
               }
               LoginResult::Failed(e) => self.error = Some(e),
            }
         }
         Message::MakeActive(uid) => {
            let lithic = shared.lithic.clone();
            return Task::perform(blocking(move || lithic.set_active_account(&uid)), |r| {
               AppMessage::Accounts(Message::Done(r))
            });
         }
         Message::AskLogout(account) => self.confirm_logout = Some(account),
         Message::CancelLogout => self.confirm_logout = None,
         Message::Logout => {
            let Some(account) = self.confirm_logout.take() else {
               return Task::none();
            };
            let lithic = shared.lithic.clone();
            return Task::perform(blocking(move || lithic.logout(&account.uid)), |r| {
               AppMessage::Accounts(Message::Done(r))
            });
         }
         Message::Done(Ok(())) => return Task::done(AppMessage::Reload),
         Message::Done(Err(e)) => return shared.toasts.error(t("accounts-change-failed"), Some(e)),
      }
      Task::none()
   }

   fn can_submit(&self) -> bool {
      match self.step {
         Step::Credentials => !self.email.trim().is_empty() && !self.password.is_empty(),
         Step::Code { .. } => self.code.len() >= 6,
      }
   }

   pub fn view<'a>(&'a self, shared: &'a Shared) -> Element<'a, Message> {
      let accounts = &shared.accounts;
      let list: Element<Message> = if accounts.accounts.is_empty() {
         widget::card(text(t("accounts-none")).style(style::muted)).into()
      } else {
         column(accounts.accounts.iter().map(|a| {
            let active = accounts.active.as_deref() == Some(a.uid.as_str());
            let session_ok = shared.sessions.contains(&a.uid);
            let mut badges = row![].spacing(6);
            if active {
               badges = badges.push(widget::badge(t("accounts-active"), Tone::Accent));
            }
            if !session_ok {
               badges = badges.push(widget::badge(t("accounts-session-missing"), Tone::Bad));
            }
            let make_active = (!active).then(|| {
               button(text(t("accounts-make-active")).size(13))
                  .style(button::secondary)
                  .on_press(Message::MakeActive(a.uid.clone()))
            });
            widget::card(
               row![
                  column![
                     row![text(&a.playername).size(16).font(widget::bold()), badges]
                        .spacing(8)
                        .align_y(Center),
                     text(&a.email).size(13).style(style::muted),
                  ]
                  .spacing(4)
                  .width(Fill),
               ]
               .extend(make_active.map(Into::into))
               .push(
                  button(text(t("accounts-logout")).size(13))
                     .style(button::text)
                     .on_press(Message::AskLogout(a.clone())),
               )
               .spacing(8)
               .align_y(Center),
            )
            .into()
         }))
         .spacing(8)
         .into()
      };

      let locked = self.submitting;
      let form: Element<Message> = match &self.step {
         Step::Credentials => column![
            text(t("accounts-login-title")).size(16).font(widget::bold()),
            text(t("accounts-login-hint")).size(13).style(style::muted),
            widget::field(
               t("accounts-email"),
               text_input("you@example.com", &self.email)
                  .on_input_maybe((!locked).then_some(Message::Email as fn(String) -> Message))
                  .on_submit(Message::Submit)
                  .padding(8),
               None
            ),
            widget::field(
               t("accounts-password"),
               text_input("", &self.password)
                  .secure(true)
                  .on_input_maybe((!locked).then_some(Message::Password as fn(String) -> Message))
                  .on_submit(Message::Submit)
                  .padding(8),
               None
            ),
            row![
               space::horizontal(),
               widget::action(
                  t("accounts-login"),
                  t("accounts-logging-in"),
                  locked,
                  self.can_submit().then_some(Message::Submit)
               )
            ],
         ]
         .spacing(12)
         .into(),
         Step::Code { .. } => column![
            text(t("accounts-code-title")).size(16).font(widget::bold()),
            text(t("accounts-code-hint")).size(13).style(style::muted),
            text_input("123456", &self.code)
               .on_input_maybe((!locked).then_some(Message::Code as fn(String) -> Message))
               .on_submit(Message::Submit)
               .padding(8),
            row![
               widget::secondary(t("common-back"), (!locked).then_some(Message::Back)),
               space::horizontal(),
               widget::action(
                  t("accounts-verify"),
                  t("accounts-logging-in"),
                  locked,
                  self.can_submit().then_some(Message::Submit)
               ),
            ],
         ]
         .spacing(12)
         .into(),
      };
      let mut form_card = column![form].spacing(12);
      if let Some(error) = &self.error {
         form_card = form_card.push(widget::notice(text(error.as_str()), Tone::Bad));
      }

      let body = column![list, widget::card(form_card).max_width(520)].spacing(16);

      let page = widget::page(t("nav-accounts"), space(), scrollable(body).height(Fill));
      match &self.confirm_logout {
         Some(account) => widget::modal(
            page,
            widget::confirm(
               t("accounts-logout-title"),
               t1("accounts-logout-body", "name", account.playername.clone()),
               t("accounts-logout"),
               true,
               Message::Logout,
               Message::CancelLogout,
            ),
            Message::CancelLogout,
         ),
         None => page,
      }
   }
}

#[cfg(test)]
mod tests {
   use super::*;

   #[test]
   fn code_input_keeps_digits_only() {
      let mut s = State::default();
      let dir = tempfile::tempdir().unwrap();
      let mut shared = Shared::new(lithic_core::Lithic::new(lithic_core::Paths::rooted(dir.path())).unwrap());
      let _ = s.update(Message::Code("12a 34-56789".into()), &mut shared);
      assert_eq!(s.code, "12345678");
   }

   #[test]
   fn two_factor_flow() {
      let dir = tempfile::tempdir().unwrap();
      let mut shared = Shared::new(lithic_core::Lithic::new(lithic_core::Paths::rooted(dir.path())).unwrap());
      let mut s = State::default();
      assert!(!s.can_submit());
      let _ = s.update(Message::Email("a@b".into()), &mut shared);
      let _ = s.update(Message::Password("pw".into()), &mut shared);
      assert!(s.can_submit());
      let _ = s.update(Message::Submit, &mut shared);
      assert!(s.submitting);
      let _ = s.update(Message::Submit, &mut shared);

      let _ = s.update(Message::Result(LoginResult::NeedsCode("tok".into())), &mut shared);
      assert!(!s.submitting);
      assert_eq!(s.step, Step::Code { token: "tok".into() });
      assert!(!s.can_submit());
      let _ = s.update(Message::Code("123456".into()), &mut shared);
      assert!(s.can_submit());

      let _ = s.update(Message::Back, &mut shared);
      assert_eq!(s.step, Step::Credentials);
      assert_eq!(s.email, "a@b", "going back keeps the email");

      let _ = s.update(Message::Result(LoginResult::Failed("nope".into())), &mut shared);
      assert_eq!(s.error.as_deref(), Some("nope"));
   }
}
