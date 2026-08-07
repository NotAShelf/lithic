use std::collections::HashMap;
use std::io::{IsTerminal, Write};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use comfy_table::presets::UTF8_FULL_CONDENSED;
use comfy_table::{ContentArrangement, Table};
use indicatif::{MultiProgress, ProgressBar, ProgressDrawTarget, ProgressStyle};
use lithic_core::{Event, Reporter, Step};
use serde::Serialize;

use crate::args::{ColorChoice, Global};
use crate::style::TableStyle;

/// An error that has already been explained to the user, or a message that
/// explains it. Either way the process exits with status 1.
#[derive(Debug)]
pub struct Failure(pub String);

impl std::fmt::Display for Failure {
   fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
      f.write_str(&self.0)
   }
}

impl From<lithic_core::Error> for Failure {
   fn from(e: lithic_core::Error) -> Self {
      Failure(e.to_string())
   }
}

pub type Result<T = ()> = std::result::Result<T, Failure>;

pub fn fail<T>(message: impl Into<String>) -> Result<T> {
   Err(Failure(message.into()))
}

pub struct Ui {
   pub json: bool,
   pub yes: bool,
   pub quiet: bool,
   color: bool,
   interactive: bool,
   table_settings: toml::Table,
}

impl Ui {
   pub fn new(global: &Global, table_settings: toml::Table) -> Self {
      let stdout_tty = std::io::stdout().is_terminal();
      let color = match global.color {
         ColorChoice::Always => true,
         ColorChoice::Never => false,
         ColorChoice::Auto => stdout_tty && std::env::var_os("NO_COLOR").is_none_or(|v| v.is_empty()),
      };
      Self {
         json: global.json,
         yes: global.yes,
         quiet: global.quiet,
         color,
         interactive: std::io::stdin().is_terminal() && std::io::stderr().is_terminal(),
         table_settings,
      }
   }

   pub fn style(&self, table: &'static str) -> TableStyle<'_> {
      TableStyle::load(&self.table_settings, table)
   }

   pub fn table(&self) -> Table {
      let mut table = Table::new();
      table
         .load_style(UTF8_FULL_CONDENSED.with_rounded_corners())
         .set_content_arrangement(ContentArrangement::Dynamic);
      if self.color {
         table.enforce_styling();
      } else {
         table.force_no_tty();
      }
      table
   }

   pub fn print_table(&self, table: &Table) {
      println!("{table}");
   }

   pub fn print_json<T: Serialize + ?Sized>(&self, value: &T) -> Result {
      let text = serde_json::to_string_pretty(value).map_err(|e| Failure(e.to_string()))?;
      println!("{text}");
      Ok(())
   }

   /// A progress or result line on stderr, so stdout stays clean for data.
   pub fn status(&self, message: impl AsRef<str>) {
      if !self.quiet {
         eprintln!("{}", message.as_ref());
      }
   }

   pub fn warn(&self, message: impl AsRef<str>) {
      eprintln!("{} {}", self.paint("warning:", "33;1"), message.as_ref());
   }

   pub fn error(&self, message: impl AsRef<str>) {
      eprintln!("{} {}", self.paint("error:", "31;1"), message.as_ref());
   }

   pub fn success(&self, message: impl AsRef<str>) {
      if !self.quiet {
         eprintln!("{} {}", self.paint("ok", "32;1"), message.as_ref());
      }
   }

   pub fn paint(&self, text: &str, sgr: &str) -> String {
      if self.color {
         format!("\x1b[{sgr}m{text}\x1b[0m")
      } else {
         text.to_string()
      }
   }

   /// Asks before a destructive step. Without a terminal to ask on, only
   /// `--yes` lets it go ahead.
   pub fn confirm(&self, question: &str) -> Result<bool> {
      if self.yes {
         return Ok(true);
      }
      if !self.interactive {
         return fail(format!(
            "{question} Pass --yes to confirm when not running interactively."
         ));
      }
      eprint!("{question} [y/N] ");
      let _ = std::io::stderr().flush();
      let mut answer = String::new();
      std::io::stdin()
         .read_line(&mut answer)
         .map_err(|e| Failure(e.to_string()))?;
      Ok(matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes"))
   }

   pub fn prompt(&self, question: &str) -> Result<String> {
      if !self.interactive {
         return fail(format!(
            "{question} needs a terminal; pass the value as an option instead"
         ));
      }
      eprint!("{question}: ");
      let _ = std::io::stderr().flush();
      let mut answer = String::new();
      std::io::stdin()
         .read_line(&mut answer)
         .map_err(|e| Failure(e.to_string()))?;
      Ok(answer.trim().to_string())
   }

   /// A reporter that draws progress bars on stderr, or does nothing when
   /// output is not a terminal or is meant for scripts.
   pub fn progress(&self) -> Progress {
      if self.quiet || self.json || !std::io::stderr().is_terminal() {
         return Progress {
            reporter: Reporter::none(),
            multi: None,
         };
      }
      let multi = MultiProgress::with_draw_target(ProgressDrawTarget::stderr());
      let spinner = multi.add(ProgressBar::new_spinner());
      spinner.set_style(
         ProgressStyle::with_template("{spinner} {msg}").unwrap_or(ProgressStyle::default_spinner()),
      );
      spinner.enable_steady_tick(Duration::from_millis(120));
      let bars: Arc<Mutex<HashMap<String, ProgressBar>>> = Arc::default();
      let bar_style = ProgressStyle::with_template(
         "  {msg:30!} {bar:30} {bytes:>10}/{total_bytes:10} {bytes_per_sec:>12}",
      )
      .unwrap_or(ProgressStyle::default_bar());

      let m = multi.clone();
      let reporter = Reporter::new(move |event| match event {
         Event::Step(step) => spinner.set_message(step_text(step)),
         Event::Log(line) => spinner.set_message(line),
         Event::Transfer { label, done, total } => {
            let mut bars = bars.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            let bar = bars.entry(label.clone()).or_insert_with(|| {
               let bar = m.add(ProgressBar::new(total.unwrap_or(0)));
               bar.set_style(bar_style.clone());
               bar.set_message(label.clone());
               bar
            });
            if let Some(total) = total {
               bar.set_length(total);
            }
            bar.set_position(done);
            if total.is_some_and(|t| done >= t) {
               bar.finish_and_clear();
            }
         }
      });
      Progress {
         reporter,
         multi: Some(multi),
      }
   }
}

pub struct Progress {
   pub reporter: Reporter,
   multi: Option<MultiProgress>,
}

impl Drop for Progress {
   fn drop(&mut self) {
      if let Some(multi) = &self.multi {
         let _ = multi.clear();
      }
   }
}

fn step_text(step: Step) -> String {
   match step {
      Step::Resolving => "Resolving",
      Step::Downloading => "Downloading",
      Step::Verifying => "Verifying",
      Step::Extracting => "Extracting",
      Step::Installing => "Installing",
      Step::Cleaning => "Cleaning up",
   }
   .to_string()
}

pub fn format_time(ms: i64) -> String {
   use chrono::TimeZone;
   chrono::Local
      .timestamp_millis_opt(ms)
      .single()
      .map(|t| t.format("%Y-%m-%d %H:%M").to_string())
      .unwrap_or_default()
}

pub fn format_duration(ms: i64) -> String {
   let minutes = ms.max(0) / 60_000;
   match (minutes / 60, minutes % 60) {
      (0, m) => format!("{m}m"),
      (h, m) => format!("{h}h {m:02}m"),
   }
}

pub fn format_count(n: i64) -> String {
   match n {
      n if n >= 1_000_000 => format!("{:.1}M", n as f64 / 1_000_000.0),
      n if n >= 10_000 => format!("{}k", n / 1000),
      n if n >= 1_000 => format!("{:.1}k", n as f64 / 1000.0),
      n => n.to_string(),
   }
}

#[cfg(test)]
mod tests {
   use super::*;

   #[test]
   fn durations_and_counts() {
      assert_eq!(format_duration(59_000), "0m");
      assert_eq!(format_duration(3_600_000 + 5 * 60_000), "1h 05m");
      assert_eq!(format_count(999), "999");
      assert_eq!(format_count(1_500), "1.5k");
      assert_eq!(format_count(45_710), "45k");
      assert_eq!(format_count(1_067_466), "1.1M");
   }
}
