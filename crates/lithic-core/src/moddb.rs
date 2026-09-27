//! Client for the Vintage Story `ModDB` (`mods.vintagestory.at/api`).
//!
//! The API answers HTTP 200 even for errors and reports the real outcome in a
//! `statuscode` string inside the body, so every response is checked for that
//! before it is decoded. Many fields are nullable or change type between
//! endpoints; the wire types below accept all shapes seen in live responses.

use std::{path::Path, result, time::Duration};

use html2text::{config::with_decorator, render::TrivialDecorator};
use serde::{
  Deserialize,
  Serialize,
  de::{DeserializeOwned, Deserializer},
};
use serde_json::Value;

use crate::{
  error::{Error, Kind, Result},
  fsutil::{self, now_ms},
  http::{Http, form_encode},
  version,
};

pub const API_BASE: &str = "https://mods.vintagestory.at/api";
pub const SITE_BASE: &str = "https://mods.vintagestory.at";

/// One entry of `/api/mods`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModSummary {
  #[serde(rename = "modid", default, deserialize_with = "int")]
  pub id:              i64,
  #[serde(rename = "assetid", default, deserialize_with = "int")]
  pub asset_id:        i64,
  #[serde(default, deserialize_with = "int")]
  pub downloads:       i64,
  #[serde(default, deserialize_with = "int")]
  pub follows:         i64,
  #[serde(rename = "trendingpoints", default, deserialize_with = "int")]
  pub trending_points: i64,
  #[serde(default, deserialize_with = "int")]
  pub comments:        i64,
  #[serde(default, deserialize_with = "text")]
  pub name:            String,
  #[serde(default, deserialize_with = "opt_text")]
  pub summary:         Option<String>,
  #[serde(rename = "modidstrs", default, deserialize_with = "strings")]
  pub mod_ids:         Vec<String>,
  #[serde(default, deserialize_with = "text")]
  pub author:          String,
  #[serde(rename = "urlalias", default, deserialize_with = "opt_text")]
  pub url_alias:       Option<String>,
  #[serde(default, deserialize_with = "opt_text")]
  pub side:            Option<String>,
  #[serde(rename = "type", default, deserialize_with = "text")]
  pub kind:            String,
  #[serde(default, deserialize_with = "opt_text")]
  pub logo:            Option<String>,
  #[serde(default, deserialize_with = "strings")]
  pub tags:            Vec<String>,
  #[serde(rename = "lastreleased", default, deserialize_with = "opt_text")]
  pub last_released:   Option<String>,
}

impl ModSummary {
  #[must_use]
  pub fn page_url(&self) -> String {
    page_url(self.id, self.url_alias.as_deref())
  }

  #[must_use]
  pub fn has_mod_id(&self, mod_id: &str) -> bool {
    self.mod_ids.iter().any(|m| m.eq_ignore_ascii_case(mod_id))
  }
}

/// `/api/mod/{id}`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModDetails {
  #[serde(rename = "modid", default, deserialize_with = "int")]
  pub id:            i64,
  #[serde(rename = "assetid", default, deserialize_with = "int")]
  pub asset_id:      i64,
  #[serde(default, deserialize_with = "text")]
  pub name:          String,
  /// HTML.
  #[serde(default, deserialize_with = "text")]
  pub text:          String,
  #[serde(default, deserialize_with = "text")]
  pub author:        String,
  #[serde(rename = "urlalias", default, deserialize_with = "opt_text")]
  pub url_alias:     Option<String>,
  #[serde(rename = "logofile", default, deserialize_with = "opt_text")]
  pub logo:          Option<String>,
  #[serde(rename = "homepageurl", default, deserialize_with = "opt_text")]
  pub homepage_url:  Option<String>,
  #[serde(rename = "sourcecodeurl", default, deserialize_with = "opt_text")]
  pub source_url:    Option<String>,
  #[serde(rename = "trailervideourl", default, deserialize_with = "opt_text")]
  pub trailer_url:   Option<String>,
  #[serde(rename = "issuetrackerurl", default, deserialize_with = "opt_text")]
  pub issues_url:    Option<String>,
  #[serde(rename = "wikiurl", default, deserialize_with = "opt_text")]
  pub wiki_url:      Option<String>,
  #[serde(default, deserialize_with = "int")]
  pub downloads:     i64,
  #[serde(default, deserialize_with = "int")]
  pub follows:       i64,
  #[serde(default, deserialize_with = "int")]
  pub comments:      i64,
  #[serde(default, deserialize_with = "opt_text")]
  pub side:          Option<String>,
  #[serde(rename = "type", default, deserialize_with = "text")]
  pub kind:          String,
  #[serde(default, deserialize_with = "opt_text")]
  pub created:       Option<String>,
  #[serde(rename = "lastreleased", default, deserialize_with = "opt_text")]
  pub last_released: Option<String>,
  #[serde(default, deserialize_with = "strings")]
  pub tags:          Vec<String>,
  #[serde(default, deserialize_with = "list")]
  pub releases:      Vec<Release>,
  #[serde(default, deserialize_with = "list")]
  pub screenshots:   Vec<Screenshot>,
}

impl ModDetails {
  #[must_use]
  pub fn page_url(&self) -> String {
    page_url(self.id, self.url_alias.as_deref())
  }

  /// The mod id string, taken from the newest release that states one.
  #[must_use]
  pub fn mod_id(&self) -> Option<&str> {
    self.releases.iter().find_map(|r| r.mod_id.as_deref())
  }

  #[must_use]
  pub fn release(&self, mod_version: &str) -> Option<&Release> {
    self.releases.iter().find(|r| {
      r.version
        .as_deref()
        .is_some_and(|v| version::compare(v, mod_version).is_eq())
    })
  }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Release {
  #[serde(rename = "releaseid", default, deserialize_with = "int")]
  pub id:        i64,
  #[serde(rename = "mainfile", default, deserialize_with = "opt_text")]
  pub url:       Option<String>,
  #[serde(default, deserialize_with = "opt_text")]
  pub filename:  Option<String>,
  #[serde(rename = "fileid", default, deserialize_with = "int")]
  pub file_id:   i64,
  #[serde(default, deserialize_with = "int")]
  pub downloads: i64,
  /// Game versions this release declares support for.
  #[serde(default, deserialize_with = "strings")]
  pub tags:      Vec<String>,
  #[serde(rename = "modidstr", default, deserialize_with = "opt_text")]
  pub mod_id:    Option<String>,
  #[serde(rename = "modversion", default, deserialize_with = "opt_text")]
  pub version:   Option<String>,
  #[serde(default, deserialize_with = "opt_text")]
  pub created:   Option<String>,
  /// HTML.
  #[serde(default, deserialize_with = "opt_text")]
  pub changelog: Option<String>,
}

impl Release {
  /// The name to save the download under. The `mainfile` URL carries a CDN
  /// hash in its path, so the clean `filename` field (or the `dl` query
  /// parameter) is preferred.
  pub fn file_name(&self) -> String {
    let from_query = self.url.as_deref().and_then(|u| {
      let query = u.split_once('?')?.1;
      query
        .split('&')
        .find_map(|kv| kv.strip_prefix("dl="))
        .map(percent_decode)
    });
    let from_path = self
      .url
      .as_deref()
      .map(|u| u.split('?').next().unwrap_or(u))
      .and_then(|u| u.rsplit('/').next())
      .map(percent_decode);
    let raw = self
      .filename
      .clone()
      .filter(|f| !f.is_empty())
      .or(from_query)
      .or(from_path)
      .unwrap_or_else(|| format!("release-{}.zip", self.id));
    fsutil::sanitize_file_name(&raw)
  }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Screenshot {
  #[serde(rename = "mainfile", default, deserialize_with = "opt_text")]
  pub url:       Option<String>,
  #[serde(
    rename = "thumbnailfilename",
    default,
    deserialize_with = "opt_text"
  )]
  pub thumbnail: Option<String>,
  #[serde(default, deserialize_with = "opt_text")]
  pub filename:  Option<String>,
}

/// A game version as the `ModDB` tags it. `tag_id` is what the mod list filter
/// expects.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameVersionTag {
  pub tag_id: i64,
  pub name:   String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tag {
  pub tag_id: i64,
  pub name:   String,
}

#[must_use]
pub fn page_url(id: i64, alias: Option<&str>) -> String {
  alias.filter(|a| !a.is_empty()).map_or_else(
    || format!("{SITE_BASE}/show/mod/{id}"),
    |alias| format!("{SITE_BASE}/{alias}"),
  )
}

/// Server-side filters for `/api/mods`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Query {
  pub text:          Option<String>,
  /// Tag ids from [`ModDb::game_versions`]. A mod matches if any of its
  /// releases declares any of these.
  pub game_versions: Vec<i64>,
  pub tags:          Vec<i64>,
}

impl Query {
  fn to_url(&self, base: &str) -> String {
    let mut params: Vec<String> = Vec::new();
    if let Some(text) = self
      .text
      .as_deref()
      .map(str::trim)
      .filter(|t| !t.is_empty())
    {
      params.push(format!("text={}", form_encode(text)));
    }
    params.extend(
      self
        .game_versions
        .iter()
        .map(|id| format!("gameversions[]={id}")),
    );
    params.extend(self.tags.iter().map(|id| format!("tagids[]={id}")));
    if params.is_empty() {
      format!("{base}/mods")
    } else {
      format!("{base}/mods?{}", params.join("&"))
    }
  }
}

#[derive(Debug, Clone)]
pub struct ModDb {
  http: Http,
  base: String,
}

impl ModDb {
  #[must_use]
  pub fn new(http: Http) -> Self {
    Self::with_base(http, API_BASE)
  }

  /// A client for another API root, such as a mirror or a test server.
  pub fn with_base(http: Http, base: impl Into<String>) -> Self {
    Self {
      http,
      base: base.into().trim_end_matches('/').to_string(),
    }
  }

  /// # Errors
  /// Returns an error if the request fails or the response cannot be decoded.
  pub async fn mods(&self, query: &Query) -> Result<Vec<ModSummary>> {
    let body = self.get("mods", &query.to_url(&self.base)).await?;
    decode_field(body, "mods")
  }

  /// Looks a mod up by numeric id, url alias, or mod id string.
  ///
  /// # Errors
  /// Returns an error for an empty id, an unsuccessful request, or an invalid
  /// response.
  pub async fn mod_details(&self, id: &str) -> Result<ModDetails> {
    let id = id.trim();
    if id.is_empty() {
      return Err(Error::invalid("empty mod id"));
    }
    let url = format!("{}/mod/{}", self.base, form_encode(id));
    match self.get(&format!("mod/{id}"), &url).await {
      Err(Error::Api { status, .. }) if status == "404" => {
        Err(Error::not_found(Kind::Mod, id))
      },
      Err(e) => Err(e),
      Ok(body) => decode_field(body, "mod"),
    }
  }

  /// # Errors
  /// Returns an error if the request fails or the version list is invalid.
  pub async fn game_versions(&self) -> Result<Vec<GameVersionTag>> {
    #[derive(Deserialize)]
    struct Raw {
      #[serde(rename = "tagid", deserialize_with = "int")]
      tag_id: i64,
      #[serde(default, deserialize_with = "text")]
      name:   String,
    }
    let body = self
      .get("gameversions", &format!("{}/gameversions", self.base))
      .await?;
    let raw: Vec<Raw> = decode_field(body, "gameversions")?;
    let mut out: Vec<GameVersionTag> = raw
      .into_iter()
      .map(|r| {
        GameVersionTag {
          tag_id: r.tag_id,
          name:   r.name,
        }
      })
      .collect();
    out.sort_by(|a, b| version::compare(&b.name, &a.name));
    Ok(out)
  }

  /// # Errors
  /// Returns an error if the request fails or the tag list is invalid.
  pub async fn tags(&self) -> Result<Vec<Tag>> {
    #[derive(Deserialize)]
    struct Raw {
      #[serde(rename = "tagid", deserialize_with = "int")]
      tag_id: i64,
      #[serde(default, deserialize_with = "text")]
      name:   String,
    }
    let body = self.get("tags", &format!("{}/tags", self.base)).await?;
    let raw: Vec<Raw> = decode_field(body, "tags")?;
    Ok(
      raw
        .into_iter()
        .map(|r| {
          Tag {
            tag_id: r.tag_id,
            name:   r.name,
          }
        })
        .collect(),
    )
  }

  async fn get(&self, endpoint: &str, url: &str) -> Result<Value> {
    let text = self.http.get_text(url).await?;
    let body: Value = serde_json::from_str(&text).map_err(|_| {
      let snippet: String = text.chars().take(120).collect();
      Error::parse(format!("ModDB {endpoint}"), format!("not JSON: {snippet}"))
    })?;
    let status = body.get("statuscode").map(|s| {
      match s {
        Value::String(s) => s.clone(),
        other => other.to_string(),
      }
    });
    match status.as_deref() {
      None | Some("200" | "") => Ok(body),
      Some(status) => {
        Err(Error::Api {
          endpoint: endpoint.to_string(),
          status:   status.to_string(),
        })
      },
    }
  }
}

fn decode_field<T: DeserializeOwned>(
  mut body: Value,
  field: &str,
) -> Result<T> {
  let value = body.get_mut(field).map(Value::take).ok_or_else(|| {
    Error::parse("ModDB response", format!("missing `{field}`"))
  })?;
  serde_json::from_value(value)
    .map_err(|e| Error::parse(format!("ModDB `{field}`"), e))
}

/// The full mod list, cached on disk.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ModIndex {
  pub fetched_at: i64,
  pub mods:       Vec<ModSummary>,
}

impl ModIndex {
  #[must_use]
  pub fn age_hours(&self) -> f64 {
    let elapsed = now_ms() - self.fetched_at;
    let hours =
      Duration::from_millis(elapsed.unsigned_abs()).as_secs_f64() / 3600.0;
    if elapsed < 0 { -hours } else { hours }
  }

  #[must_use]
  pub fn load_cached(path: &Path) -> Option<Self> {
    fsutil::read_json(path).ok().flatten()
  }

  /// # Errors
  /// Returns an error if serialization or writing the cache fails.
  pub fn save(&self, path: &Path) -> Result<()> {
    let bytes =
      serde_json::to_vec(self).map_err(|e| Error::parse("mod index", e))?;
    fsutil::write_atomic(path, &bytes)
  }

  /// Finds a mod by its mod id string (what `modinfo.json` calls `modid`).
  #[must_use]
  pub fn by_mod_id(&self, mod_id: &str) -> Option<&ModSummary> {
    self.mods.iter().find(|m| m.has_mod_id(mod_id)).or_else(|| {
      self.mods.iter().find(|m| {
        m.url_alias
          .as_deref()
          .is_some_and(|a| a.eq_ignore_ascii_case(mod_id))
      })
    })
  }

  #[must_use]
  pub fn by_id(&self, id: i64) -> Option<&ModSummary> {
    self.mods.iter().find(|m| m.id == id)
  }

  #[must_use]
  pub fn search(&self, query: &str, sort: Sort) -> Vec<&ModSummary> {
    search(&self.mods, query, sort)
  }
}

#[derive(
  Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Sort {
  /// Best text match first, then downloads.
  #[default]
  Relevance,
  Downloads,
  Follows,
  Trending,
  Updated,
  Name,
}

/// Ranks mods against free text. Every whitespace-separated word must appear in
/// the name, mod id, author, summary, or tags.
#[must_use]
pub fn search<'a>(
  mods: &'a [ModSummary],
  query: &str,
  sort: Sort,
) -> Vec<&'a ModSummary> {
  rank(mods, query, sort)
    .into_iter()
    .map(|i| &mods[i])
    .collect()
}

/// Like [`search`], but returns positions in `mods`.
pub fn rank(mods: &[ModSummary], query: &str, sort: Sort) -> Vec<usize> {
  let words: Vec<String> =
    query.split_whitespace().map(str::to_lowercase).collect();
  let mut hits: Vec<(u32, usize, &ModSummary)> = mods
    .iter()
    .enumerate()
    .filter_map(|(i, m)| {
      if words.is_empty() {
        return Some((0, i, m));
      }
      let name = m.name.to_lowercase();
      let author = m.author.to_lowercase();
      let summary = m.summary.as_deref().unwrap_or_default().to_lowercase();
      let mut score = 0;
      for word in &words {
        let in_id = m
          .mod_ids
          .iter()
          .any(|id| id.to_lowercase().contains(word.as_str()));
        let exact_id = m.mod_ids.iter().any(|id| id.eq_ignore_ascii_case(word));
        let in_tags = m
          .tags
          .iter()
          .any(|t| t.to_lowercase().contains(word.as_str()));
        let word_score = if exact_id {
          50
        } else if name.starts_with(word.as_str()) {
          30
        } else if name.contains(word.as_str()) {
          20
        } else if in_id {
          15
        } else if author.contains(word.as_str()) {
          8
        } else if in_tags {
          5
        } else if summary.contains(word.as_str()) {
          3
        } else {
          return None;
        };
        score += word_score;
      }
      Some((score, i, m))
    })
    .collect();

  hits.sort_by(|(sa, _, a), (sb, _, b)| {
    match sort {
      Sort::Relevance => sb.cmp(sa).then(b.downloads.cmp(&a.downloads)),
      Sort::Downloads => b.downloads.cmp(&a.downloads),
      Sort::Follows => b.follows.cmp(&a.follows),
      Sort::Trending => b.trending_points.cmp(&a.trending_points),
      Sort::Updated => b.last_released.cmp(&a.last_released),
      Sort::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
    }
  });
  hits.into_iter().map(|(_, i, _)| i).collect()
}

/// HTML without any markup, for interfaces that do their own wrapping and
/// styling.
#[must_use]
pub fn html_to_plain(html: &str) -> String {
  with_decorator(TrivialDecorator::new())
    .string_from_read(html.as_bytes(), 10_000)
    .unwrap_or_else(|_| html.to_string())
    .trim()
    .to_string()
}

/// HTML from the `ModDB` (descriptions, changelogs) as text for a terminal,
/// with light Markdown-style markup.
#[must_use]
pub fn html_to_text(html: &str, width: usize) -> String {
  html2text::from_read(html.as_bytes(), width.max(20))
    .unwrap_or_else(|_| html.to_string())
    .trim()
    .to_string()
}

fn percent_decode(s: &str) -> String {
  let bytes = s.as_bytes();
  let mut out = Vec::with_capacity(bytes.len());
  let mut i = 0;
  let hex = |b: u8| (b as char).to_digit(16).and_then(|d| u8::try_from(d).ok());
  while i < bytes.len() {
    match bytes[i] {
      b'%' if i + 2 < bytes.len() => {
        if let (Some(hi), Some(lo)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
          out.push((hi << 4) | lo);
          i += 3;
        } else {
          out.push(b'%');
          i += 1;
        }
      },
      b'+' => {
        out.push(b' ');
        i += 1;
      },
      b => {
        out.push(b);
        i += 1;
      },
    }
  }
  String::from_utf8_lossy(&out).into_owned()
}

fn int<'de, D: Deserializer<'de>>(d: D) -> result::Result<i64, D::Error> {
  Ok(match Value::deserialize(d)? {
    Value::Number(n) => {
      n.as_i64()
        .or_else(|| {
          n.as_f64().map(|f| {
            #[expect(
              clippy::cast_possible_truncation,
              reason = "ModDB numeric fields accept floating point JSON and \
                        truncate to integer"
            )]
            let integer = f as i64;
            integer
          })
        })
        .unwrap_or(0)
    },
    Value::String(s) => s.trim().parse().unwrap_or(0),
    Value::Bool(b) => i64::from(b),
    _ => 0,
  })
}

fn opt_text<'de, D: Deserializer<'de>>(
  d: D,
) -> result::Result<Option<String>, D::Error> {
  Ok(match Value::deserialize(d)? {
    Value::String(s) => Some(s.trim().to_string()).filter(|s| !s.is_empty()),
    Value::Number(n) => Some(n.to_string()),
    Value::Bool(b) => Some(b.to_string()),
    _ => None,
  })
}

fn text<'de, D: Deserializer<'de>>(d: D) -> result::Result<String, D::Error> {
  opt_text(d).map(Option::unwrap_or_default)
}

fn strings<'de, D: Deserializer<'de>>(
  d: D,
) -> result::Result<Vec<String>, D::Error> {
  Ok(match Value::deserialize(d)? {
    Value::Array(items) => {
      items
        .into_iter()
        .filter_map(|v| {
          match v {
            Value::String(s) => Some(s.trim().to_string()),
            Value::Number(n) => Some(n.to_string()),
            _ => None,
          }
        })
        .filter(|s| !s.is_empty())
        .collect()
    },
    _ => Vec::new(),
  })
}

fn list<'de, D, T>(d: D) -> result::Result<Vec<T>, D::Error>
where
  D: Deserializer<'de>,
  T: DeserializeOwned,
{
  Ok(match Value::deserialize(d)? {
    Value::Array(items) => {
      items
        .into_iter()
        .filter_map(|v| serde_json::from_value(v).ok())
        .collect()
    },
    _ => Vec::new(),
  })
}

#[cfg(test)]
#[expect(
  clippy::unwrap_used,
  reason = "test setup and assertions intentionally fail on error"
)]
mod tests {
  use super::*;

  const SUMMARY: &str = r#"{"modid":12083,"assetid":70869,"downloads":2,"follows":0,"trendingpoints":0,
      "comments":0,"name":" Hardcore Water Forked Forked","summary":"Fork","modidstrs":["hardcorewaterforkedforked"],
      "author":"violets","urlalias":null,"side":"both","type":"mod","logo":null,"tags":[""],
      "lastreleased":"2026-09-26 22:04:21"}"#;

  const RELEASE: &str = r#"{"releaseid":52086,
      "mainfile":"https://moddbcdn.vintagestory.at/CarryOn-1.22.0_v1.14_6433d237.zip?dl=CarryOn-1.22.0_v1.14.3.zip",
      "filename":"CarryOn-1.22.0_v1.14.3.zip","fileid":113014,"downloads":45710,
      "tags":["1.22.0","1.22.1"],"modidstr":"carryon","modversion":"1.14.3","created":"2026-08-07 10:22:05",
      "changelog":"<ul><li>x</li></ul>"}"#;

  #[test]
  fn summary_matches_live_shape() {
    let m: ModSummary = serde_json::from_str(SUMMARY).unwrap();
    assert_eq!(m.id, 12083);
    assert_eq!(m.name, "Hardcore Water Forked Forked");
    assert!(m.tags.is_empty());
    assert_eq!(m.url_alias, None);
    assert_eq!(m.page_url(), "https://mods.vintagestory.at/show/mod/12083");
  }

  #[test]
  fn release_matches_live_shape() {
    let r: Release = serde_json::from_str(RELEASE).unwrap();
    assert_eq!(r.id, 52086);
    assert_eq!(r.version.as_deref(), Some("1.14.3"));
    assert_eq!(r.file_name(), "CarryOn-1.22.0_v1.14.3.zip");
  }

  #[test]
  fn release_file_name_fallbacks() {
    let mut r: Release = serde_json::from_str(RELEASE).unwrap();
    r.filename = None;
    assert_eq!(r.file_name(), "CarryOn-1.22.0_v1.14.3.zip");
    r.url = Some("https://cdn/x/My%20Mod_1.0.zip".to_string());
    assert_eq!(r.file_name(), "My Mod_1.0.zip");
    r.url = None;
    assert_eq!(r.file_name(), "release-52086.zip");
  }

  #[test]
  fn odd_release_fields_do_not_break_decoding() {
    let r: Release = serde_json::from_str(
      r#"{"releaseid":"7","filename":123,"fileid":null,"tags":null}"#,
    )
    .unwrap();
    assert_eq!(r.id, 7);
    assert_eq!(r.filename.as_deref(), Some("123"));
    assert_eq!(r.file_id, 0);
  }

  #[test]
  fn search_ranks_and_requires_every_word() {
    let mk = |id: i64, name: &str, modid: &str, downloads: i64| {
      ModSummary {
        id,
        name: name.to_string(),
        mod_ids: vec![modid.to_string()],
        downloads,
        ..ModSummary::default()
      }
    };
    let mods = vec![
      mk(1, "Carry On", "carryon", 100),
      mk(2, "Carry Capacity", "carrycapacity", 500),
      mk(3, "Primitive Survival", "primitivesurvival", 900),
    ];
    let hits = search(&mods, "carry on", Sort::Relevance);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, 1);
    let hits = search(&mods, "carry", Sort::Relevance);
    assert_eq!(hits[0].id, 2);
    assert_eq!(search(&mods, "", Sort::Downloads)[0].id, 3);
  }

  #[test]
  fn query_url() {
    let q = Query {
      text:          Some("carry on".to_string()),
      game_versions: vec![-5, -6],
      tags:          vec![],
    };
    assert_eq!(
      q.to_url(API_BASE),
      format!(
        "{API_BASE}/mods?text=carry+on&gameversions[]=-5&gameversions[]=-6"
      )
    );
  }

  #[test]
  fn plain_html_has_no_markup() {
    let text = html_to_plain(
      "<h2>Team</h2><p><strong>Bold</strong> and <a href=\"https://x\">link</a></p>",
    );
    assert!(text.contains("Team"));
    assert!(text.contains("Bold and link"), "{text:?}");
    assert!(
      !text.contains('#') && !text.contains('*') && !text.contains('['),
      "{text:?}"
    );
  }

  #[test]
  fn percent_round_trip() {
    assert_eq!(percent_decode("a%20b%2Fc"), "a b/c");
    assert_eq!(percent_decode("100%"), "100%");
    assert_eq!(percent_decode("%éé"), "%éé");
  }
}
