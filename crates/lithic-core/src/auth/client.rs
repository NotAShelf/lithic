//! Login against `auth.vintagestory.at/v2/gamelogin`.
//!
//! The endpoint takes a form POST and answers HTTP 200 with a JSON object for
//! both success and failure. Failures carry a `reason` (for example
//! `invalidemailorpassword`); success carries the session the game stores in
//! `clientsettings.json`. Field names and types of the success response are
//! read leniently.

use std::{error, fmt};

use serde_json::{Map, Value};

use crate::http::Http;

const GAMELOGIN_URL: &str = "https://auth.vintagestory.at/v2/gamelogin";

#[derive(Debug, Clone)]
pub enum AuthError {
  Network(String),
  /// Wrong email, password or code. Carries the server's reason, if any.
  InvalidCredentials(String),
  /// Retry with the TOTP code and this token.
  TwoFactorRequired {
    prelogintoken: String,
  },
  /// An answer that could not be understood.
  Server(String),
}

impl fmt::Display for AuthError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      Self::Network(e) => write!(f, "could not reach the login server: {e}"),
      Self::InvalidCredentials(reason) if reason.is_empty() => {
        f.write_str("invalid email or password")
      },
      Self::InvalidCredentials(reason) => write!(f, "login rejected: {reason}"),
      Self::TwoFactorRequired { .. } => {
        f.write_str("a two-factor code is required")
      },
      Self::Server(e) => {
        write!(f, "unexpected answer from the login server: {e}")
      },
    }
  }
}

impl error::Error for AuthError {}

#[derive(Debug, Clone, Default)]
pub struct LoginResponse {
  pub uid:              String,
  pub playername:       String,
  pub sessionkey:       String,
  pub sessionsignature: String,
  pub mptoken:          String,
  pub entitlements:     String,
}

/// Pass `twofa` as `Some((prelogintoken, code))` on the second step.
///
/// # Errors
/// Returns [`AuthError::Network`] if the request fails, [`AuthError::Server`]
/// for an invalid server response, or a credential or two-factor error reported
/// by the login server.
pub async fn gamelogin(
  http: &Http,
  email: &str,
  password: &str,
  twofa: Option<(&str, &str)>,
) -> Result<LoginResponse, AuthError> {
  let mut form = vec![("email", email), ("password", password)];
  if let Some((token, code)) = twofa {
    form.push(("prelogintoken", token));
    form.push(("totpcode", code));
  }
  let (status, body) = http
    .post_form(GAMELOGIN_URL, &form)
    .await
    .map_err(|e| AuthError::Network(e.to_string()))?;
  if !(200..300).contains(&status) {
    return Err(AuthError::Server(format!("HTTP {status}")));
  }
  let value: Value = serde_json::from_str(&body).map_err(|_| {
    let snippet: String = body.chars().take(200).collect();
    AuthError::Server(format!("not JSON: {snippet}"))
  })?;
  let Some(obj) = value.as_object() else {
    return Err(AuthError::Server("not a JSON object".to_string()));
  };
  interpret(obj)
}

fn field(obj: &Map<String, Value>, key: &str) -> String {
  match obj.get(key) {
    Some(Value::String(s)) => s.clone(),
    Some(Value::Null) | None => String::new(),
    Some(other) => other.to_string(),
  }
}

fn first_field(obj: &Map<String, Value>, keys: &[&str]) -> String {
  keys
    .iter()
    .map(|k| field(obj, k))
    .find(|v| !v.is_empty())
    .unwrap_or_default()
}

fn interpret(obj: &Map<String, Value>) -> Result<LoginResponse, AuthError> {
  let sessionkey = first_field(obj, &["sessionkey", "sessionKey"]);
  if !sessionkey.is_empty() {
    return Ok(LoginResponse {
      uid: first_field(obj, &["uid", "playeruid", "useridentifier"]),
      playername: first_field(obj, &["playername", "playerName"]),
      sessionkey,
      sessionsignature: first_field(obj, &[
        "sessionsignature",
        "sessionSignature",
      ]),
      mptoken: first_field(obj, &["mptoken", "mpToken"]),
      entitlements: field(obj, "entitlements"),
    });
  }

  let reason = first_field(obj, &["reason", "message"]);
  let prelogintoken = first_field(obj, &["prelogintoken", "preLoginToken"]);
  let lower = reason.to_lowercase();
  let wants_code = ["totp", "2fa", "twofa", "two factor", "secondfactor"]
    .iter()
    .any(|k| lower.contains(k));
  if wants_code && !prelogintoken.is_empty() {
    return Err(AuthError::TwoFactorRequired { prelogintoken });
  }
  if !reason.is_empty() {
    return Err(AuthError::InvalidCredentials(reason));
  }
  let keys: Vec<&str> = obj.keys().map(String::as_str).collect();
  Err(AuthError::Server(format!(
    "no session in the answer (keys: {})",
    keys.join(", ")
  )))
}

#[cfg(test)]
#[expect(
  clippy::unwrap_used,
  clippy::panic,
  reason = "test setup and assertions intentionally fail on error"
)]
mod tests {
  use super::*;

  fn parse(json: &str) -> Result<LoginResponse, AuthError> {
    let value: Value = serde_json::from_str(json).unwrap();
    interpret(value.as_object().unwrap())
  }

  #[test]
  fn successful_login() {
    let out = parse(
         r#"{"valid":1,"uid":"abc","playername":"Steve","sessionkey":"sk","sessionsignature":"sig","mptoken":"mt","entitlements":"singleplayer"}"#,
      )
      .unwrap();
    assert_eq!(
      (out.uid.as_str(), out.playername.as_str()),
      ("abc", "Steve")
    );
    assert_eq!(
      (out.sessionkey.as_str(), out.mptoken.as_str()),
      ("sk", "mt")
    );
  }

  #[test]
  fn non_string_fields_are_coerced() {
    let out =
      parse(r#"{"uid":12345,"sessionkey":"sk","entitlements":["a","b"]}"#)
        .unwrap();
    assert_eq!(out.uid, "12345");
    assert_eq!(out.entitlements, "[\"a\",\"b\"]");
  }

  #[test]
  fn two_factor() {
    match parse(
      r#"{"valid":0,"reason":"requiretotpcode","prelogintoken":"plt-123"}"#,
    ) {
      Err(AuthError::TwoFactorRequired { prelogintoken }) => {
        assert_eq!(prelogintoken, "plt-123")
      },
      other => panic!("expected TwoFactorRequired, got {other:?}"),
    }
    assert!(matches!(
      parse(r#"{"valid":0,"reason":"totp","preLoginToken":"x"}"#),
      Err(AuthError::TwoFactorRequired { .. })
    ));
  }

  #[test]
  fn bad_credentials() {
    match parse(r#"{"valid":0,"reason":"invalidemailorpassword"}"#) {
      Err(AuthError::InvalidCredentials(reason)) => {
        assert_eq!(reason, "invalidemailorpassword")
      },
      other => panic!("expected InvalidCredentials, got {other:?}"),
    }
  }

  #[test]
  fn unknown_shape_names_the_keys() {
    match parse(r#"{"foo":"bar","baz":1}"#) {
      Err(AuthError::Server(msg)) => {
        assert!(msg.contains("foo") && msg.contains("baz"))
      },
      other => panic!("expected Server, got {other:?}"),
    }
  }
}
