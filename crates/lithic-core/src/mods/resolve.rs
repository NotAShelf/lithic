//! Choosing which release of a mod to install.

use crate::error::{Error, Kind, Result};
use crate::moddb::{ModDetails, Release};
use crate::version::{self, Fit};

#[derive(Debug, Clone, Default)]
pub struct Target {
   /// The instance's game version. `None` accepts any release.
   pub game_version: Option<String>,
   pub allow_prerelease: bool,
}

/// Picks the release to install.
///
/// With a pin, that exact version is returned whatever it declares. Otherwise
/// the newest release declaring the target game version or another patch of
/// the same minor version wins; releases that declare nothing are the fallback.
/// Prereleases are only considered when allowed or when no stable release fits.
/// # Errors
/// Returns an error when the pin is unavailable or no downloadable release fits the target.
pub fn pick_release<'a>(details: &'a ModDetails, target: &Target, pin: Option<&str>) -> Result<&'a Release> {
   let label = details.mod_id().unwrap_or(&details.name).to_string();
   let usable = || {
      details
         .releases
         .iter()
         .filter(|r| r.url.is_some() && r.version.is_some())
   };

   if let Some(pin) = pin {
      return usable()
         .find(|r| {
            r.version
               .as_deref()
               .is_some_and(|v| version::compare(v, pin).is_eq())
         })
         .ok_or_else(|| Error::not_found(Kind::Release, format!("{label}@{pin}")));
   }

   let fit = |r: &Release| {
      target
         .game_version
         .as_ref()
         .map_or(Fit::Exact, |game| version::fit(&r.tags, game))
   };

   let compatible: Vec<&Release> = usable().filter(|r| fit(r) >= Fit::SameMinor).collect();
   let pool = if compatible.is_empty() {
      usable().filter(|r| fit(r) == Fit::Unknown).collect()
   } else {
      compatible
   };
   if pool.is_empty() {
      return Err(match &target.game_version {
         Some(game) if usable().next().is_some() => Error::NoCompatibleRelease {
            mod_id: label,
            game_version: game.clone(),
         },
         _ => Error::not_found(Kind::Release, label),
      });
   }

   let is_pre = |r: &&Release| r.version.as_deref().is_some_and(version::is_prerelease);
   let stable: Vec<&Release> = pool.iter().copied().filter(|r| !is_pre(r)).collect();
   let pool = if target.allow_prerelease || stable.is_empty() {
      pool
   } else {
      stable
   };

   pool
      .into_iter()
      .max_by(|a, b| {
         version::compare(
            a.version.as_deref().unwrap_or_default(),
            b.version.as_deref().unwrap_or_default(),
         )
         .then(a.id.cmp(&b.id))
      })
      .ok_or_else(|| Error::not_found(Kind::Release, label))
}

#[cfg(test)]
#[expect(
   clippy::unwrap_used,
   reason = "test setup and assertions intentionally fail on error"
)]
mod tests {
   use super::*;

   fn release(id: i64, version: &str, tags: &[&str]) -> Release {
      Release {
         id,
         url: Some(format!("https://x/{id}.zip")),
         version: Some(version.to_string()),
         tags: tags.iter().map(ToString::to_string).collect(),
         mod_id: Some("m".to_string()),
         ..Release::default()
      }
   }

   fn details(releases: Vec<Release>) -> ModDetails {
      ModDetails {
         releases,
         ..ModDetails::default()
      }
   }

   fn target(game: &str) -> Target {
      Target {
         game_version: Some(game.to_string()),
         allow_prerelease: false,
      }
   }

   #[test]
   fn newest_compatible_wins() {
      let d = details(vec![
         release(3, "3.0.0", &["1.22.0"]),
         release(2, "2.1.0", &["1.21.0", "1.21.3"]),
         release(1, "2.0.0", &["1.21.5"]),
      ]);
      assert_eq!(pick_release(&d, &target("1.21.5"), None).unwrap().id, 2);
      assert_eq!(pick_release(&d, &target("1.22.7"), None).unwrap().id, 3);
   }

   #[test]
   fn incompatible_is_an_error_not_a_guess() {
      let d = details(vec![release(1, "1.0.0", &["1.19.0"])]);
      assert!(matches!(
         pick_release(&d, &target("1.22.7"), None),
         Err(Error::NoCompatibleRelease { .. })
      ));
   }

   #[test]
   fn stable_preferred_unless_only_prereleases_fit() {
      let d = details(vec![
         release(2, "2.0.0-dev.14", &["1.22.0"]),
         release(1, "1.9.0", &["1.22.0"]),
      ]);
      assert_eq!(pick_release(&d, &target("1.22.0"), None).unwrap().id, 1);
      let mut t = target("1.22.0");
      t.allow_prerelease = true;
      assert_eq!(pick_release(&d, &t, None).unwrap().id, 2);

      let d = details(vec![release(2, "2.0.0-dev.14", &["1.22.0"])]);
      assert_eq!(pick_release(&d, &target("1.22.0"), None).unwrap().id, 2);
   }

   #[test]
   fn pin_overrides_compatibility() {
      let d = details(vec![
         release(2, "2.0.0", &["1.22.0"]),
         release(1, "1.0.0", &["1.19.0"]),
      ]);
      assert_eq!(pick_release(&d, &target("1.22.0"), Some("1.0.0")).unwrap().id, 1);
      assert!(pick_release(&d, &target("1.22.0"), Some("9.9.9")).is_err());
   }

   #[test]
   fn untagged_releases_are_a_fallback() {
      let d = details(vec![release(1, "1.0.0", &[])]);
      assert_eq!(pick_release(&d, &target("1.22.0"), None).unwrap().id, 1);
   }
}
