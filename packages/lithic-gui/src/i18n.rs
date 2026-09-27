//! Short names for looking up translated text. Every key passed to these
//! functions must exist in `gui.ftl`; a test checks the sources for that.

use lithic_locale::{Value, locale};

pub fn t(id: &str) -> String {
  locale().get(id)
}

pub fn t1<'a>(id: &str, key: &str, value: impl Into<Value<'a>>) -> String {
  locale().get_args(id, &[(key, value.into())])
}

pub fn t2<'a>(
  id: &str,
  k1: &str,
  v1: impl Into<Value<'a>>,
  k2: &str,
  v2: impl Into<Value<'a>>,
) -> String {
  locale().get_args(id, &[(k1, v1.into()), (k2, v2.into())])
}

pub fn tn(id: &str, args: &[(&str, Value<'_>)]) -> String {
  locale().get_args(id, args)
}

#[cfg(test)]
#[expect(
  clippy::unwrap_used,
  reason = "test setup and assertions intentionally fail on error"
)]
mod tests {
  use std::{fs, path::Path};

  /// Collects the string literal that starts each `t(`, `t1(`, `t2(` or
  /// `tn(` call, even when rustfmt put it on the next line.
  fn keys_in(source: &str) -> Vec<String> {
    let mut keys = Vec::new();
    for call in ["t(", "t1(", "t2(", "tn("] {
      let mut rest = source;
      while let Some(pos) = rest.find(call) {
        let before = rest[..pos].chars().next_back();
        rest = &rest[pos + call.len()..];
        if before.is_some_and(|c| c.is_alphanumeric() || c == '_') {
          continue;
        }
        let Some(literal) = rest.trim_start().strip_prefix('"') else {
          continue;
        };
        if let Some(end) = literal.find('"') {
          keys.push(literal[..end].to_string());
        }
      }
    }
    keys
  }

  fn sources(dir: &Path, out: &mut Vec<(String, String)>) {
    for entry in fs::read_dir(dir).unwrap().flatten() {
      let path = entry.path();
      if path.is_dir() {
        sources(&path, out);
      } else if path.extension().is_some_and(|e| e == "rs")
        && !path.ends_with("i18n.rs")
      {
        out.push((
          path.display().to_string(),
          fs::read_to_string(&path).unwrap(),
        ));
      }
    }
  }

  #[test]
  fn every_used_key_is_translated() {
    let mut files = Vec::new();
    sources(
      &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
      &mut files,
    );
    let loc = lithic_locale::locale();
    let mut missing = Vec::new();
    let mut total = 0;
    for (file, text) in &files {
      for key in keys_in(text) {
        total += 1;
        if !loc.has(&key) {
          missing.push(format!("{key} ({file})"));
        }
      }
    }
    assert!(total > 50, "found only {total} keys, the scan is broken");
    assert!(
      missing.is_empty(),
      "missing translations:\n{}",
      missing.join("\n")
    );
  }

  #[test]
  fn scanner_finds_keys() {
    assert_eq!(
      keys_in(
        "t(\"a-b\") + t1(\"c\", \"n\", 1) + fmt(\"x\") + t2(\n   \"d\", \
         \"a\", 1) + t(name)"
      ),
      ["a-b", "c", "d"]
    );
  }
}
