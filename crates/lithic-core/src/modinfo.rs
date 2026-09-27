//! Reading mod metadata from the files the game loads: `.zip` archives and
//! folders with a `modinfo.json`, `.cs` source mods, and `.dll` mods.

use std::{
  collections::BTreeMap,
  fs::{self, File},
  io::Read,
  path::{Path, PathBuf},
  result,
};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::error::{Error, IoContext, Result};

pub const MODINFO_FILE: &str = "modinfo.json";

/// Dependencies every mod may declare on the base game. They are never
/// installed from the `ModDB`.
pub const GAME_MOD_IDS: [&str; 3] = ["game", "survival", "creative"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
  Zip,
  Folder,
  Cs,
  Dll,
}

impl Format {
  #[must_use]
  pub fn of(path: &Path) -> Option<Self> {
    if path.is_dir() {
      return Some(Self::Folder);
    }
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
      "zip" => Some(Self::Zip),
      "cs" => Some(Self::Cs),
      "dll" => Some(Self::Dll),
      _ => None,
    }
  }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModInfo {
  /// Lowercased, as the game compares mod ids case-insensitively.
  pub mod_id:       String,
  pub name:         String,
  pub version:      String,
  pub kind:         String,
  pub side:         Option<String>,
  pub description:  Option<String>,
  pub authors:      Vec<String>,
  pub website:      Option<String>,
  /// Lowercased mod id to minimum version (`*` or empty for any).
  pub dependencies: BTreeMap<String, String>,
  /// False when nothing was read from the file and the id was guessed from
  /// its name.
  pub has_metadata: bool,
}

impl ModInfo {
  /// Dependencies on other mods, leaving out the base game.
  pub fn mod_dependencies(&self) -> impl Iterator<Item = (&String, &String)> {
    self.dependencies.iter().filter(|(id, _)| !is_game_mod(id))
  }
}

#[must_use]
pub fn is_game_mod(mod_id: &str) -> bool {
  GAME_MOD_IDS.iter().any(|g| g.eq_ignore_ascii_case(mod_id))
}

/// The id the game derives from a mod name when `modid` is missing: lowercase
/// ASCII letters and digits only.
pub fn mod_id_from_name(name: &str) -> String {
  name
    .chars()
    .filter(char::is_ascii_alphanumeric)
    .flat_map(char::to_lowercase)
    .collect()
}

/// Reads metadata for one mod file or folder.
/// # Errors
/// Returns an error for an unsupported mod file, unreadable metadata, or
/// invalid JSON.
pub fn read(path: &Path) -> Result<ModInfo> {
  let format = Format::of(path).ok_or_else(|| {
    Error::invalid(format!("{} is not a mod", path.display()))
  })?;
  match format {
    Format::Zip => read_zip(path),
    Format::Folder => {
      let file =
        find_case_insensitive(path, MODINFO_FILE).ok_or_else(|| {
          Error::parse(path.display().to_string(), "folder has no modinfo.json")
        })?;
      let text = fs::read_to_string(&file).at(&file)?;
      parse_json(&text).map_err(|e| Error::parse(file.display().to_string(), e))
    },
    Format::Cs => {
      let text = fs::read_to_string(path).at(path)?;
      Ok(parse_cs(&text).unwrap_or_else(|| guessed(path)))
    },
    Format::Dll => Ok(guessed(path)),
  }
}

/// Reads `modinfo.json` from the root of an archive. Archives without one
/// (code mods that carry their metadata in a `.dll` attribute) fall back to
/// an id guessed from the file name.
/// # Errors
/// Returns an error if the archive cannot be opened or its metadata is invalid.
pub fn read_zip(path: &Path) -> Result<ModInfo> {
  let file = File::open(path).at(path)?;
  let mut archive = zip::ZipArchive::new(file)
    .map_err(|e| Error::parse(path.display().to_string(), e))?;

  let index = (0..archive.len()).find(|&i| {
    archive.name_for_index(i).is_some_and(|n| {
      n.trim_start_matches("./")
        .eq_ignore_ascii_case(MODINFO_FILE)
    })
  });
  let Some(index) = index else {
    return Ok(guessed(path));
  };

  let mut entry = archive
    .by_index(index)
    .map_err(|e| Error::parse(path.display().to_string(), e))?;
  let mut bytes = Vec::new();
  entry.read_to_end(&mut bytes).at(path)?;
  let text = String::from_utf8_lossy(&bytes);
  parse_json(&text).map_err(|e| {
    Error::parse(format!("{} in {}", MODINFO_FILE, path.display()), e)
  })
}

/// Parses `modinfo.json` text. Accepts JSON5 (comments, trailing commas), a
/// leading byte-order mark, and any capitalisation of keys.
/// # Errors
/// Returns an error when the input is invalid JSON or lacks both a mod id and
/// name.
pub fn parse_json(text: &str) -> result::Result<ModInfo, String> {
  let text = text.trim_start_matches('\u{feff}');
  let value: Value = serde_json5::from_str(text).map_err(|e| e.to_string())?;
  let Value::Object(raw) = value else {
    return Err("expected a JSON object".to_string());
  };
  let map: Map<String, Value> = raw
    .into_iter()
    .map(|(k, v)| (k.to_ascii_lowercase(), v))
    .collect();

  let name = string(map.get("name")).unwrap_or_default();
  let mod_id = string(map.get("modid"))
    .map(|s| s.to_ascii_lowercase())
    .filter(|s| !s.is_empty())
    .unwrap_or_else(|| mod_id_from_name(&name));
  if mod_id.is_empty() {
    return Err("neither modid nor name is set".to_string());
  }

  let dependencies = match map.get("dependencies") {
    Some(Value::Object(deps)) => {
      deps
        .iter()
        .map(|(k, v)| {
          (k.to_ascii_lowercase(), string(Some(v)).unwrap_or_default())
        })
        .collect()
    },
    _ => BTreeMap::new(),
  };

  let authors = match map.get("authors") {
    Some(Value::Array(items)) => {
      items.iter().filter_map(|v| string(Some(v))).collect()
    },
    Some(v) => string(Some(v)).into_iter().collect(),
    None => Vec::new(),
  };

  Ok(ModInfo {
    mod_id,
    name,
    version: string(map.get("version")).unwrap_or_default(),
    kind: string(map.get("type"))
      .unwrap_or_else(|| "code".to_string())
      .to_ascii_lowercase(),
    side: string(map.get("side")),
    description: string(map.get("description")),
    authors,
    website: string(map.get("website")),
    dependencies,
    has_metadata: true,
  })
}

/// Pulls `[assembly: ModInfo(...)]` and `[assembly: ModDependency(...)]` out of
/// a source mod. Returns `None` when there is no `ModInfo` attribute.
#[must_use]
pub fn parse_cs(source: &str) -> Option<ModInfo> {
  let args = attribute_args(source, "ModInfo").into_iter().next()?;
  let (positional, named) = split_args(&args);

  let name = positional.first().cloned().unwrap_or_default();
  let mod_id = positional
    .get(1)
    .map_or_else(|| mod_id_from_name(&name), |s| s.to_ascii_lowercase());
  if mod_id.is_empty() {
    return None;
  }

  let mut dependencies = BTreeMap::new();
  for dep in attribute_args(source, "ModDependency") {
    let (pos, _) = split_args(&dep);
    if let Some(id) = pos.first() {
      dependencies.insert(
        id.to_ascii_lowercase(),
        pos.get(1).cloned().unwrap_or_default(),
      );
    }
  }

  Some(ModInfo {
    mod_id,
    name,
    version: named.get("version").cloned().unwrap_or_default(),
    kind: "code".to_string(),
    side: named.get("side").cloned(),
    description: named.get("description").cloned(),
    authors: named
      .get("authors")
      .map(|a| vec![a.clone()])
      .unwrap_or_default(),
    website: named.get("website").cloned(),
    dependencies,
    has_metadata: true,
  })
}

fn guessed(path: &Path) -> ModInfo {
  let stem = path
    .file_stem()
    .map(|s| s.to_string_lossy().into_owned())
    .unwrap_or_default();
  let base = stem.split(['_', '-', ' ']).next().unwrap_or(&stem);
  ModInfo {
    mod_id: mod_id_from_name(base),
    name: stem.clone(),
    kind: "code".to_string(),
    has_metadata: false,
    ..ModInfo::default()
  }
}

fn string(value: Option<&Value>) -> Option<String> {
  match value? {
    Value::String(s) => Some(s.trim().to_string()),
    Value::Number(n) => Some(n.to_string()),
    Value::Bool(b) => Some(b.to_string()),
    _ => None,
  }
}

fn find_case_insensitive(dir: &Path, name: &str) -> Option<PathBuf> {
  fs::read_dir(dir)
    .ok()?
    .flatten()
    .find(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(name))
    .map(|e| e.path())
}

/// The argument text of every `[assembly: Name(...)]` attribute in `source`.
fn attribute_args(source: &str, name: &str) -> Vec<String> {
  let mut out = Vec::new();
  let mut rest = source;
  while let Some(pos) = rest.find("assembly") {
    rest = &rest[pos + "assembly".len()..];
    let after = rest.trim_start();
    let Some(after) = after.strip_prefix(':') else {
      continue;
    };
    let after = after.trim_start();
    let Some(after) = after.strip_prefix(name) else {
      continue;
    };
    let after = after
      .strip_prefix("Attribute")
      .unwrap_or(after)
      .trim_start();
    let Some(body) = after.strip_prefix('(') else {
      continue;
    };
    if let Some(args) = balanced(body) {
      out.push(args.to_string());
    }
  }
  out
}

/// Text up to the `)` that closes an already-opened `(`, skipping string
/// literals.
fn balanced(body: &str) -> Option<&str> {
  let mut depth = 1;
  let mut in_str = false;
  let mut escaped = false;
  for (i, c) in body.char_indices() {
    if in_str {
      match c {
        _ if escaped => escaped = false,
        '\\' => escaped = true,
        '"' => in_str = false,
        _ => {},
      }
      continue;
    }
    match c {
      '"' => in_str = true,
      '(' | '{' => depth += 1,
      ')' | '}' => {
        depth -= 1;
        if depth == 0 {
          return Some(&body[..i]);
        }
      },
      _ => {},
    }
  }
  None
}

/// Splits attribute arguments into positional string literals and
/// `Name = "value"` pairs (keys lowercased). For array values such as
/// `Authors = new[] { "a", "b" }` the strings are joined with `, `.
fn split_args(args: &str) -> (Vec<String>, BTreeMap<String, String>) {
  let mut positional = Vec::new();
  let mut named = BTreeMap::new();
  for part in split_top_level(args) {
    let part = part.trim();
    if let Some((key, value)) = part.split_once('=')
      && !key.contains('"')
    {
      let strings = literals(value);
      if !strings.is_empty() {
        named.insert(key.trim().to_ascii_lowercase(), strings.join(", "));
      }
    } else if let Some(s) = literals(part).into_iter().next() {
      positional.push(s);
    }
  }
  (positional, named)
}

fn split_top_level(args: &str) -> Vec<&str> {
  let mut parts = Vec::new();
  let mut depth = 0;
  let mut in_str = false;
  let mut escaped = false;
  let mut start = 0;
  for (i, c) in args.char_indices() {
    if in_str {
      match c {
        _ if escaped => escaped = false,
        '\\' => escaped = true,
        '"' => in_str = false,
        _ => {},
      }
      continue;
    }
    match c {
      '"' => in_str = true,
      '(' | '{' | '[' => depth += 1,
      ')' | '}' | ']' => depth -= 1,
      ',' if depth == 0 => {
        parts.push(&args[start..i]);
        start = i + 1;
      },
      _ => {},
    }
  }
  parts.push(&args[start..]);
  parts
}

fn literals(text: &str) -> Vec<String> {
  let mut out = Vec::new();
  let mut chars = text.chars();
  while let Some(c) = chars.next() {
    if c != '"' {
      continue;
    }
    let mut s = String::new();
    let mut escaped = false;
    for c in chars.by_ref() {
      match c {
        _ if escaped => {
          s.push(c);
          escaped = false;
        },
        '\\' => escaped = true,
        '"' => break,
        c => s.push(c),
      }
    }
    out.push(s);
  }
  out
}

#[cfg(test)]
#[expect(
  clippy::unwrap_used,
  reason = "test setup and assertions intentionally fail on error"
)]
mod tests {
  use std::io::Write;

  use zip::write::SimpleFileOptions;

  use super::*;

  #[test]
  fn json_keys_any_case_with_comments() {
    let info = parse_json(
      "\u{feff}{ // comment\n Name: 'Carry On', MODID: 'CarryOn', Version: \
       '1.14.3', type: 'code',\n Dependencies: { Game: '1.22.0', \
       CarryCapacity: '*' }, authors: ['a', 'b'], }",
    )
    .unwrap();
    assert_eq!(info.mod_id, "carryon");
    assert_eq!(info.version, "1.14.3");
    assert_eq!(
      info.dependencies.get("game").map(String::as_str),
      Some("1.22.0")
    );
    assert_eq!(info.mod_dependencies().count(), 1);
    assert_eq!(info.authors, ["a", "b"]);
  }

  #[test]
  fn missing_modid_is_derived_from_name() {
    let info =
      parse_json(r#"{"name": "Better Ruins!", "version": "1.0"}"#).unwrap();
    assert_eq!(info.mod_id, "betterruins");
  }

  #[test]
  fn numeric_fields_are_coerced() {
    let info =
      parse_json(r#"{"modid": "x", "type": 1, "version": 2}"#).unwrap();
    assert_eq!(info.kind, "1");
    assert_eq!(info.version, "2");
  }

  #[test]
  fn only_exact_game_ids_are_skipped() {
    assert!(is_game_mod("Survival"));
    assert!(!is_game_mod("primitivesurvival"));
    assert!(!is_game_mod("tabletopgames"));
  }

  #[test]
  fn cs_attributes() {
    let src = r#"
         using Vintagestory.API.Common;
         [assembly: ModInfo("Hello World", "HelloWorld", Version = "1.2.0",
             Authors = new[] { "someone", "other" }, Side = "Universal")]
         [assembly: ModDependency("game", "1.19.0")]
         [assembly: ModDependency("otherlib")]
      "#;
    let info = parse_cs(src).unwrap();
    assert_eq!(info.mod_id, "helloworld");
    assert_eq!(info.name, "Hello World");
    assert_eq!(info.version, "1.2.0");
    assert_eq!(info.side.as_deref(), Some("Universal"));
    assert_eq!(info.authors, ["someone, other"]);
    assert_eq!(
      info.dependencies.get("otherlib").map(String::as_str),
      Some("")
    );
  }

  #[test]
  fn zip_with_and_without_modinfo() {
    let dir = tempfile::tempdir().unwrap();
    let with = dir.path().join("with.zip");
    let mut w = zip::ZipWriter::new(File::create(&with).unwrap());
    w.start_file("ModInfo.JSON", SimpleFileOptions::default())
      .unwrap();
    w.write_all(br#"{"modid":"withmod","version":"0.1.0"}"#)
      .unwrap();
    w.finish().unwrap();
    assert_eq!(read(&with).unwrap().mod_id, "withmod");

    let without = dir.path().join("SomeLib-1.0.zip");
    let mut w = zip::ZipWriter::new(File::create(&without).unwrap());
    w.start_file("lib.dll", SimpleFileOptions::default())
      .unwrap();
    w.write_all(b"MZ").unwrap();
    w.finish().unwrap();
    let info = read(&without).unwrap();
    assert!(!info.has_metadata);
    assert_eq!(info.mod_id, "somelib");
  }
}
