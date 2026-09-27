pub mod accounts;
pub mod browse;
pub mod game;
pub mod instance;
pub mod instances;
pub mod settings;

use lithic_core::mods::Problem;

use crate::i18n::{t2, tn};

pub fn format_time(ms: i64) -> String {
   use jiff::{Timestamp, tz::TimeZone};

   Timestamp::from_millisecond(ms)
      .map(|ts| {
         ts.to_zoned(TimeZone::system())
            .strftime("%Y-%m-%d %H:%M")
            .to_string()
      })
      .unwrap_or_default()
}

pub fn format_duration(ms: i64) -> String {
   let minutes = ms.max(0) / 60_000;
   match (minutes / 60, minutes % 60) {
      (0, m) => format!("{m}m"),
      (h, m) => format!("{h}h {m:02}m"),
   }
}

#[expect(
   clippy::cast_precision_loss,
   reason = "abbreviated counts deliberately round to one decimal place"
)]
pub fn format_count(n: i64) -> String {
   match n {
      n if n >= 1_000_000 => format!("{:.1}M", n as f64 / 1_000_000.0),
      n if n >= 10_000 => format!("{}k", n / 1000),
      n if n >= 1_000 => format!("{:.1}k", n as f64 / 1000.0),
      n => n.to_string(),
   }
}

pub fn describe_problem(p: &Problem) -> String {
   match p {
      Problem::MissingDependency {
         mod_id,
         dependency,
         required,
      } if required.is_empty() || required == "*" => t2(
         "problem-missing",
         "mod",
         mod_id.clone(),
         "dep",
         dependency.clone(),
      ),
      Problem::MissingDependency {
         mod_id,
         dependency,
         required,
      } => tn(
         "problem-missing-version",
         &[
            ("mod", mod_id.clone().into()),
            ("dep", dependency.clone().into()),
            ("version", required.clone().into()),
         ],
      ),
      Problem::OutdatedDependency {
         mod_id,
         dependency,
         required,
         installed,
      } => tn(
         "problem-outdated",
         &[
            ("mod", mod_id.clone().into()),
            ("dep", dependency.clone().into()),
            ("version", required.clone().into()),
            ("installed", installed.clone().into()),
         ],
      ),
      Problem::DisabledDependency { mod_id, dependency } => t2(
         "problem-disabled",
         "mod",
         mod_id.clone(),
         "dep",
         dependency.clone(),
      ),
      Problem::Duplicate { mod_id, files } => t2(
         "problem-duplicate",
         "mod",
         mod_id.clone(),
         "files",
         files.join(", "),
      ),
      Problem::Unreadable { file, error } => {
         t2("problem-unreadable", "file", file.clone(), "error", error.clone())
      }
   }
}

/// The dependency a problem is about, if installing it would fix it.
pub fn installable_fix(p: &Problem) -> Option<&str> {
   match p {
      Problem::MissingDependency { dependency, .. } | Problem::OutdatedDependency { dependency, .. } => {
         Some(dependency)
      }
      _ => None,
   }
}

#[cfg(test)]
mod tests {
   use super::*;

   #[test]
   fn formatting() {
      assert_eq!(format_duration(125 * 60_000), "2h 05m");
      assert_eq!(format_count(8706), "8.7k");
   }
}
