//! Version strings as they appear on the `ModDB` and in `modinfo.json`.
//!
//! Mod authors are inconsistent (`v1.2`, `1.0.0.4`, `2.0.0-dev.20`), so
//! parsing is lenient and anything unparsable falls back to comparing text.

use std::cmp::Ordering;

use semver::Version;

#[must_use]
pub fn parse(s: &str) -> Option<Version> {
   let s = s.trim();
   let s = s.strip_prefix(['v', 'V']).unwrap_or(s);
   lenient_semver::parse(s).ok()
}

#[must_use]
pub fn compare(a: &str, b: &str) -> Ordering {
   match (parse(a), parse(b)) {
      (Some(x), Some(y)) => x.cmp(&y),
      (Some(_), None) => Ordering::Greater,
      (None, Some(_)) => Ordering::Less,
      (None, None) => a.cmp(b),
   }
}

/// `-dev`, `-pre`, `-rc` and friends.
#[must_use]
pub fn is_prerelease(s: &str) -> bool {
   match parse(s) {
      Some(v) => !v.pre.is_empty(),
      None => s.contains('-'),
   }
}

/// `(major, minor)` of a version, e.g. `1.21.5-rc.2` gives `(1, 21)`.
#[must_use]
pub fn minor(s: &str) -> Option<(u64, u64)> {
   if let Some(v) = parse(s) {
      return Some((v.major, v.minor));
   }
   let s = s.trim().trim_start_matches(['v', 'V']);
   let mut parts = s.split('.');
   let major = parts.next()?.parse().ok()?;
   let minor = parts.next()?.split('-').next()?.parse().ok()?;
   Some((major, minor))
}

/// Whether `installed` meets a `modinfo.json` dependency requirement. The game
/// reads the requirement as a minimum version; `*` or empty accepts anything.
#[must_use]
pub fn satisfies(installed: &str, required: &str) -> bool {
   let required = required.trim();
   if required.is_empty() || required == "*" {
      return true;
   }
   compare(installed, required) != Ordering::Less
}

/// How well a release's declared game versions fit a target game version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Fit {
   /// Declares other game versions only.
   None,
   /// Declares no game versions at all.
   Unknown,
   /// Declares a different patch of the same major.minor.
   SameMinor,
   /// Declares the target version itself.
   Exact,
}

#[must_use]
pub fn fit(tags: &[String], game_version: &str) -> Fit {
   let tags: Vec<&str> = tags.iter().map(|t| t.trim()).filter(|t| !t.is_empty()).collect();
   if tags.is_empty() {
      return Fit::Unknown;
   }
   let target = game_version.trim().trim_start_matches(['v', 'V']);
   if tags
      .iter()
      .any(|t| t.trim_start_matches(['v', 'V']).eq_ignore_ascii_case(target))
   {
      return Fit::Exact;
   }
   let target_minor = minor(target);
   if target_minor.is_some() && tags.iter().any(|t| minor(t) == target_minor) {
      return Fit::SameMinor;
   }
   Fit::None
}

#[cfg(test)]
mod tests {
   use super::*;

   #[test]
   fn lenient_parsing() {
      assert!(parse("v1.2").is_some());
      assert!(parse("1.0.0.4").is_some());
      assert!(parse("2.0.0-dev.20").is_some());
      assert_eq!(compare("1.10.0", "1.9.9"), Ordering::Greater);
      assert_eq!(compare("2.0.0-dev.24", "2.0.0-dev.20"), Ordering::Greater);
      assert_eq!(compare("2.0.0", "2.0.0-rc.1"), Ordering::Greater);
      assert_eq!(compare("V1.2.0", "1.2.0"), Ordering::Equal);
   }

   #[test]
   fn prerelease_detection() {
      assert!(is_prerelease("1.0.0-dev.3"));
      assert!(is_prerelease("1.22.0-rc.10"));
      assert!(!is_prerelease("1.22.7"));
   }

   #[test]
   fn minor_extraction() {
      assert_eq!(minor("1.21.5-rc.2"), Some((1, 21)));
      assert_eq!(minor("1.19"), Some((1, 19)));
      assert_eq!(minor("v1.18.15"), Some((1, 18)));
      assert_eq!(minor("nonsense"), None);
   }

   #[test]
   fn dependency_requirement_is_a_minimum() {
      assert!(satisfies("1.2.0", "1.1.0"));
      assert!(satisfies("1.1.0", "1.1.0"));
      assert!(!satisfies("1.0.9", "1.1.0"));
      assert!(satisfies("0.0.1", "*"));
      assert!(satisfies("0.0.1", ""));
   }

   #[test]
   fn release_fit() {
      let tags = |t: &[&str]| t.iter().map(ToString::to_string).collect::<Vec<_>>();
      assert_eq!(fit(&tags(&["1.22.0", "1.22.6"]), "1.22.6"), Fit::Exact);
      assert_eq!(fit(&tags(&["1.22.0", "1.22.6"]), "1.22.7"), Fit::SameMinor);
      assert_eq!(fit(&tags(&["1.21.0"]), "1.22.7"), Fit::None);
      assert_eq!(fit(&tags(&[""]), "1.22.7"), Fit::Unknown);
      assert!(Fit::Exact > Fit::SameMinor && Fit::SameMinor > Fit::Unknown && Fit::Unknown > Fit::None);
   }
}
