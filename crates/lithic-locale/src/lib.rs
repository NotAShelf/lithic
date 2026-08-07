//! Fluent translations for the lithic GUI.
//!
//! ```rust
//! use lithic_locale::locale;
//!
//! assert_eq!(locale().get("nav-instances"), "Instances");
//! ```

use std::sync::OnceLock;

use fluent::concurrent::FluentBundle;
use fluent::{FluentArgs, FluentResource, FluentValue};
use unic_langid::LanguageIdentifier;

pub use fluent::FluentValue as Value;

/// The English sources, one per file.
pub const ENGLISH: &[&str] = &[include_str!("../locales/en/gui.ftl")];

static GLOBAL: OnceLock<Localizer> = OnceLock::new();

/// The process-wide localizer, English unless [`set_locale`] ran first.
pub fn locale() -> &'static Localizer {
   GLOBAL.get_or_init(Localizer::english)
}

/// Replaces the global localizer. Returns `false` if [`locale`] was already
/// used, in which case nothing changes.
pub fn set_locale(localizer: Localizer) -> bool {
   GLOBAL.set(localizer).is_ok()
}

pub struct Localizer {
   bundle: FluentBundle<FluentResource>,
}

impl Localizer {
   pub fn from_sources(lang: &str, sources: &[&str]) -> Result<Self, String> {
      let lang: LanguageIdentifier = lang.parse().map_err(|e| format!("invalid language tag: {e}"))?;
      let mut bundle = FluentBundle::new_concurrent(vec![lang]);
      // Isolation marks around placeables show up as boxes in the GUI's text
      // renderer, and the UI does not mix text directions.
      bundle.set_use_isolating(false);
      for src in sources {
         let resource = FluentResource::try_new((*src).to_string())
            .map_err(|(_, errors)| format!("FTL parse errors: {errors:?}"))?;
         bundle
            .add_resource(resource)
            .map_err(|errors| format!("duplicate FTL messages: {errors:?}"))?;
      }
      Ok(Self { bundle })
   }

   pub fn english() -> Self {
      Self::from_sources("en-US", ENGLISH).expect("the bundled English messages are valid")
   }

   pub fn has(&self, id: &str) -> bool {
      self.bundle.has_message(id)
   }

   /// The message `id`, or `id` itself when it is missing.
   pub fn get(&self, id: &str) -> String {
      self.format(id, None)
   }

   pub fn get_args(&self, id: &str, args: &[(&str, FluentValue<'_>)]) -> String {
      let mut fluent_args = FluentArgs::new();
      for (key, value) in args {
         fluent_args.set(*key, value.clone());
      }
      self.format(id, Some(&fluent_args))
   }

   fn format(&self, id: &str, args: Option<&FluentArgs<'_>>) -> String {
      let Some(pattern) = self.bundle.get_message(id).and_then(|m| m.value()) else {
         return id.to_string();
      };
      let mut errors = Vec::new();
      self
         .bundle
         .format_pattern(pattern, args, &mut errors)
         .into_owned()
   }
}

#[cfg(test)]
mod tests {
   use super::*;

   #[test]
   fn english_loads() {
      let loc = Localizer::english();
      assert_eq!(loc.get("nav-instances"), "Instances");
      assert_eq!(loc.get("no-such-message"), "no-such-message");
   }

   #[test]
   fn placeables_have_no_isolation_marks() {
      let loc = Localizer::english();
      let text = loc.get_args("mods-count", &[("count", 3.into())]);
      assert!(
         !text.contains('\u{2068}') && !text.contains('\u{2069}'),
         "{text:?}"
      );
      assert!(text.contains('3'));
   }
}
