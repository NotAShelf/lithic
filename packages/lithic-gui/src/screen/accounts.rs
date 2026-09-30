use std::{collections::HashMap, mem};

use iced::{
  Center,
  Element,
  Fill,
  Task,
  widget::{column, container, row, scrollable, space, text},
};
use lithic_core::{
  Error,
  auth::{Account, AuthError},
};

use crate::{
  app::{Message as AppMessage, Shared},
  i18n::{t, t1},
  icon::Icon,
  style::{self, Tone},
  task::blocking,
  widget,
};

#[derive(Debug, Clone, PartialEq, Eq)]
enum Step {
  Credentials,
  Code { token: String },
}

#[derive(Debug, Clone)]
enum Check {
  Checking,
  Valid,
  Rejected,
  Failed(String),
}

#[derive(Debug)]
pub struct State {
  adding:         bool,
  email:          String,
  password:       String,
  code:           String,
  step:           Step,
  submitting:     bool,
  error:          Option<String>,
  checks:         HashMap<String, Check>,
  confirm_logout: Option<Account>,
}

impl Default for State {
  fn default() -> Self {
    Self {
      adding:         false,
      email:          String::new(),
      password:       String::new(),
      code:           String::new(),
      step:           Step::Credentials,
      submitting:     false,
      error:          None,
      checks:         HashMap::new(),
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
  Add(Option<String>),
  CloseAdd,
  Email(String),
  Password(String),
  Code(String),
  Submit,
  Back,
  Result(LoginResult),
  Checked(String, Result<bool, String>),
  MakeActive(String),
  AskLogout(Account),
  CancelLogout,
  Logout,
  Done(Result<(), String>),
}

impl State {
  /// Checks every stored session that is not being checked already.
  pub fn enter(&mut self, shared: &Shared) -> Task<AppMessage> {
    Task::batch(
      shared
        .accounts
        .accounts
        .iter()
        .filter(|a| shared.sessions.contains(&a.uid))
        .map(|a| self.check(a.uid.clone(), shared)),
    )
  }

  /// Whether the last check found `uid`'s session rejected.
  pub fn rejected(&self, uid: &str) -> bool {
    matches!(self.checks.get(uid), Some(Check::Rejected))
  }

  /// Checks the active account, so the sidebar can tell whether it still
  /// works before the Accounts page is opened.
  pub fn check_active(&mut self, shared: &Shared) -> Task<AppMessage> {
    match shared.accounts.active.clone() {
      Some(uid) if shared.sessions.contains(&uid) => self.check(uid, shared),
      _ => Task::none(),
    }
  }

  fn check(&mut self, uid: String, shared: &Shared) -> Task<AppMessage> {
    if matches!(self.checks.get(&uid), Some(Check::Checking)) {
      return Task::none();
    }
    self.checks.insert(uid.clone(), Check::Checking);
    let lithic = shared.lithic.clone();
    Task::perform(
      async move {
        let result = lithic
          .check_account_session(&uid)
          .await
          .map_err(|e| e.to_string());
        (uid, result)
      },
      |(uid, result)| AppMessage::Accounts(Message::Checked(uid, result)),
    )
  }

  pub fn update(
    &mut self,
    message: Message,
    shared: &mut Shared,
  ) -> Task<AppMessage> {
    match message {
      Message::Add(email) => {
        self.adding = true;
        self.step = Step::Credentials;
        self.email = email.unwrap_or_default();
        self.password.clear();
        self.code.clear();
        self.error = None;
      },
      Message::CloseAdd => {
        if !self.submitting {
          self.adding = false;
          self.password.clear();
        }
      },
      Message::Email(v) => self.email = v,
      Message::Password(v) => self.password = v,
      Message::Code(v) => {
        self.code = v.chars().filter(char::is_ascii_digit).take(8).collect();
      },
      Message::Submit => {
        if self.submitting || !self.can_submit() {
          return Task::none();
        }
        self.submitting = true;
        self.error = None;
        let lithic = shared.lithic.clone();
        let (email, password) =
          (self.email.trim().to_string(), self.password.clone());
        let twofa = match &self.step {
          Step::Code { token } => Some((token.clone(), self.code.clone())),
          Step::Credentials => None,
        };
        return Task::perform(
          async move {
            let twofa = twofa.as_ref().map(|(tk, c)| (tk.as_str(), c.as_str()));
            match lithic.login(&email, &password, twofa, true).await {
              Ok(account) => LoginResult::Done(account),
              Err(Error::Auth(AuthError::TwoFactorRequired {
                prelogintoken,
              })) => LoginResult::NeedsCode(prelogintoken),
              Err(e) => LoginResult::Failed(e.to_string()),
            }
          },
          |r| AppMessage::Accounts(Message::Result(r)),
        );
      },
      Message::Back => {
        self.step = Step::Credentials;
        self.code.clear();
        self.error = None;
      },
      Message::Result(result) => {
        self.submitting = false;
        match result {
          LoginResult::Done(account) => {
            let checks = mem::take(&mut self.checks);
            *self = Self::default();
            self.checks = checks;
            self.checks.insert(account.uid.clone(), Check::Valid);
            return Task::batch([
              shared.toasts.success(t1(
                "accounts-logged-in",
                "name",
                account.playername,
              )),
              Task::done(AppMessage::Reload),
            ]);
          },
          LoginResult::NeedsCode(token) => {
            self.step = Step::Code { token };
            self.code.clear();
          },
          LoginResult::Failed(e) => self.error = Some(e),
        }
      },
      Message::Checked(uid, result) => {
        if matches!(self.checks.get(&uid), Some(Check::Checking))
          && shared.accounts.get(&uid).is_some()
        {
          let status = match result {
            Ok(true) => Check::Valid,
            Ok(false) => Check::Rejected,
            Err(e) => Check::Failed(e),
          };
          self.checks.insert(uid, status);
        }
      },
      Message::MakeActive(uid) => {
        let lithic = shared.lithic.clone();
        return Task::perform(
          blocking(move || lithic.set_active_account(&uid)),
          |r| AppMessage::Accounts(Message::Done(r)),
        );
      },
      Message::AskLogout(account) => self.confirm_logout = Some(account),
      Message::CancelLogout => self.confirm_logout = None,
      Message::Logout => {
        let Some(account) = self.confirm_logout.take() else {
          return Task::none();
        };
        self.checks.remove(&account.uid);
        let lithic = shared.lithic.clone();
        return Task::perform(
          blocking(move || lithic.logout(&account.uid)),
          |r| AppMessage::Accounts(Message::Done(r)),
        );
      },
      Message::Done(Ok(())) => return Task::done(AppMessage::Reload),
      Message::Done(Err(e)) => {
        return shared.toasts.error(t("accounts-change-failed"), Some(e));
      },
    }
    Task::none()
  }

  fn can_submit(&self) -> bool {
    match self.step {
      Step::Credentials => {
        !self.email.trim().is_empty() && !self.password.is_empty()
      },
      Step::Code { .. } => self.code.len() >= 6,
    }
  }

  fn account_row<'a>(
    &'a self,
    account: &'a Account,
    shared: &'a Shared,
  ) -> Element<'a, Message> {
    let uid = &account.uid;
    let active = shared.accounts.active.as_deref() == Some(uid.as_str());
    let (tone, status) = match self.checks.get(uid) {
      Some(Check::Checking) => (Tone::Neutral, t("accounts-checking")),
      Some(Check::Valid) => (Tone::Good, t("accounts-valid")),
      Some(Check::Rejected) => (Tone::Bad, t("accounts-rejected")),
      Some(Check::Failed(e)) => {
        (Tone::Warn, t1("accounts-check-failed", "error", e.clone()))
      },
      None if shared.sessions.contains(uid) => {
        (Tone::Neutral, t("accounts-checking"))
      },
      None => (Tone::Bad, t("accounts-session-missing")),
    };
    let needs_login = tone == Tone::Bad;

    let dot = widget::tip(
      container(space().width(10).height(10)).style(style::dot(tone)),
      status,
    );
    let mut identity =
      row![dot, text(&account.playername).size(15).font(widget::bold()),]
        .spacing(12)
        .align_y(Center);
    if !account.email.is_empty() {
      identity =
        identity.push(text(&account.email).size(13).style(style::muted));
    }
    if active {
      identity =
        identity.push(widget::badge(t("accounts-active"), Tone::Accent));
    }

    let mut actions = row![].spacing(4).align_y(Center);
    if needs_login {
      actions = actions.push(widget::icon_button(
        Icon::Refresh,
        t("accounts-sign-in-again"),
        Some(Message::Add(
          Some(account.email.clone()).filter(|e| !e.is_empty()),
        )),
      ));
    }
    if !active {
      actions = actions.push(widget::icon_button(
        Icon::Star,
        t("accounts-make-active"),
        Some(Message::MakeActive(uid.clone())),
      ));
    }
    actions = actions.push(widget::icon_button(
      Icon::LogOut,
      t("accounts-logout"),
      Some(Message::AskLogout(account.clone())),
    ));

    container(
      row![container(identity).width(Fill), actions]
        .spacing(12)
        .align_y(Center),
    )
    .padding([8, 12])
    .into()
  }

  fn add_dialog(&self) -> Element<'_, Message> {
    let locked = self.submitting;
    let (title, body, buttons): (String, Element<Message>, Element<Message>) =
      match &self.step {
        Step::Credentials => {
          (
            t("accounts-login-title"),
            column![
              widget::field(
                t("accounts-email"),
                widget::input("you@example.com", &self.email)
                  .on_input_maybe(
                    (!locked)
                      .then_some(Message::Email as fn(String) -> Message)
                  )
                  .on_submit(Message::Submit)
                  .padding(8),
                None
              ),
              widget::field(
                t("accounts-password"),
                widget::input("", &self.password)
                  .secure(true)
                  .on_input_maybe(
                    (!locked)
                      .then_some(Message::Password as fn(String) -> Message)
                  )
                  .on_submit(Message::Submit)
                  .padding(8),
                None
              ),
            ]
            .spacing(12)
            .into(),
            row![
              widget::ghost(
                t("common-cancel"),
                (!locked).then_some(Message::CloseAdd)
              ),
              widget::action(
                t("accounts-login"),
                t("accounts-logging-in"),
                locked,
                self.can_submit().then_some(Message::Submit)
              ),
            ]
            .spacing(8)
            .into(),
          )
        },
        Step::Code { .. } => {
          (
            t("accounts-code-title"),
            column![
              text(t("accounts-code-hint")).size(13).style(style::muted),
              widget::input("123456", &self.code)
                .on_input_maybe(
                  (!locked).then_some(Message::Code as fn(String) -> Message)
                )
                .on_submit(Message::Submit)
                .padding(8),
            ]
            .spacing(12)
            .into(),
            row![
              widget::ghost(
                t("common-back"),
                (!locked).then_some(Message::Back)
              ),
              widget::action(
                t("accounts-verify"),
                t("accounts-logging-in"),
                locked,
                self.can_submit().then_some(Message::Submit)
              ),
            ]
            .spacing(8)
            .into(),
          )
        },
      };
    let mut content = column![body].spacing(12);
    if let Some(error) = &self.error {
      content = content.push(widget::notice(text(error.as_str()), Tone::Bad));
    }
    widget::dialog(title, content, buttons, 440.0)
  }

  pub fn view<'a>(&'a self, shared: &'a Shared) -> Element<'a, Message> {
    let add = widget::primary_icon(
      Icon::Plus,
      t("accounts-add"),
      Some(Message::Add(None)),
    );
    let page = if shared.accounts.accounts.is_empty() {
      widget::page(
        t("nav-accounts"),
        space(),
        widget::empty(t("accounts-none"), t("accounts-none-body"), Some(add)),
      )
    } else {
      let list = widget::card(widget::divided(
        shared
          .accounts
          .accounts
          .iter()
          .map(|a| self.account_row(a, shared)),
      ))
      .padding(4);
      widget::page(
        t("nav-accounts"),
        add,
        scrollable(column![list]).height(Fill),
      )
    };

    if self.adding {
      return widget::modal(page, self.add_dialog(), Message::CloseAdd);
    }
    match &self.confirm_logout {
      Some(account) => {
        widget::modal(
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
        )
      },
      None => page,
    }
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

  fn shared() -> (tempfile::TempDir, Shared) {
    let dir = tempfile::tempdir().unwrap();
    let shared = Shared::new(Lithic::new(Paths::rooted(dir.path())).unwrap());
    (dir, shared)
  }

  #[test]
  fn code_input_keeps_digits_only() {
    let (_dir, mut shared) = shared();
    let mut s = State::default();
    let _ = s.update(Message::Code("12a 34-56789".into()), &mut shared);
    assert_eq!(s.code, "12345678");
  }

  #[test]
  fn entering_checks_every_stored_session() {
    let (_dir, mut shared) = shared();
    for uid in ["a", "b"] {
      shared.accounts.accounts.push(Account {
        uid: uid.into(),
        ..Default::default()
      });
    }
    shared.sessions.insert("a".into());
    let mut s = State::default();
    let _ = s.enter(&shared);
    assert!(matches!(s.checks.get("a"), Some(Check::Checking)));
    assert!(!s.checks.contains_key("b"), "no session, nothing to check");

    assert!(!s.rejected("a"));
    let _ = s.update(Message::Checked("a".into(), Ok(false)), &mut shared);
    assert!(s.rejected("a"), "the sidebar must see the rejection");
  }

  #[test]
  fn two_factor_flow() {
    let (_dir, mut shared) = shared();
    let mut s = State::default();
    let _ = s.update(Message::Add(Some("a@b".into())), &mut shared);
    assert!(s.adding);
    assert_eq!(s.email, "a@b", "signing in again keeps the email");
    assert!(!s.can_submit());
    let _ = s.update(Message::Password("pw".into()), &mut shared);
    assert!(s.can_submit());
    let _ = s.update(Message::Submit, &mut shared);
    assert!(s.submitting);
    let _ = s.update(Message::CloseAdd, &mut shared);
    assert!(s.adding, "the dialog stays open while signing in");

    let _ = s.update(
      Message::Result(LoginResult::NeedsCode("tok".into())),
      &mut shared,
    );
    assert!(!s.submitting);
    assert_eq!(s.step, Step::Code {
      token: "tok".into(),
    });
    assert!(!s.can_submit());
    let _ = s.update(Message::Code("123456".into()), &mut shared);
    assert!(s.can_submit());

    let _ = s.update(Message::Back, &mut shared);
    assert_eq!(s.step, Step::Credentials);
    assert_eq!(s.email, "a@b", "going back keeps the email");

    let _ = s.update(
      Message::Result(LoginResult::Failed("nope".into())),
      &mut shared,
    );
    assert_eq!(s.error.as_deref(), Some("nope"));
  }
}
