use std::{
  collections::HashMap,
  env,
  fmt::{self, Display, Formatter},
  io::{IsTerminal, Write, stderr, stdin, stdout},
  result::Result as StdResult,
  sync::{Arc, Mutex, PoisonError},
  time::Duration,
};

use comfy_table::{ContentArrangement, Table, presets::UTF8_FULL_CONDENSED};
use indicatif::{
  MultiProgress,
  ProgressBar,
  ProgressDrawTarget,
  ProgressStyle,
};
use lithic_core::{Error, Event, Reporter, Step};
use serde::Serialize;

use crate::{
  args::{ColorChoice, Global},
  style::TableStyle,
};

/// An error that has already been explained to the user, or a message that
/// explains it. Either way the process exits with status 1.
#[derive(Debug)]
pub struct Failure(pub String);

impl Display for Failure {
  fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
    f.write_str(&self.0)
  }
}

impl From<Error> for Failure {
  fn from(e: Error) -> Self {
    Self(e.to_string())
  }
}

pub type Result<T = ()> = StdResult<T, Failure>;

pub fn fail<T>(message: impl Into<String>) -> Result<T> {
  Err(Failure(message.into()))
}

enum Interaction {
  Terminal,
  Unavailable,
}

pub struct Ui {
  pub json:       bool,
  pub yes:        bool,
  pub quiet:      bool,
  color:          ColorChoice,
  interaction:    Interaction,
  table_settings: toml::Table,
}

impl Ui {
  pub fn new(global: &Global, table_settings: toml::Table) -> Self {
    let stdout_tty = stdout().is_terminal();
    let color = match global.color {
      ColorChoice::Always => ColorChoice::Always,
      ColorChoice::Never => ColorChoice::Never,
      ColorChoice::Auto => {
        if stdout_tty && env::var_os("NO_COLOR").is_none_or(|v| v.is_empty()) {
          ColorChoice::Always
        } else {
          ColorChoice::Never
        }
      },
    };
    Self {
      json: global.json,
      yes: global.yes,
      quiet: global.quiet,
      color,
      interaction: if stdin().is_terminal() && stderr().is_terminal() {
        Interaction::Terminal
      } else {
        Interaction::Unavailable
      },
      table_settings,
    }
  }

  pub const fn style(&self, table: &'static str) -> TableStyle<'_> {
    TableStyle::load(&self.table_settings, table)
  }

  pub fn table(&self) -> Table {
    let mut table = Table::new();
    table
      .load_style(UTF8_FULL_CONDENSED.with_rounded_corners())
      .set_content_arrangement(ContentArrangement::Dynamic);
    if matches!(self.color, ColorChoice::Always) {
      table.enforce_styling();
    } else {
      table.force_no_tty();
    }
    table
  }

  #[expect(clippy::print_stdout, reason = "formatted tables are CLI output")]
  pub fn print_table(table: &Table) {
    println!("{table}");
  }

  #[expect(clippy::print_stdout, reason = "serialized JSON is CLI output")]
  pub fn print_json<T: Serialize + ?Sized>(value: &T) -> Result {
    let text = serde_json::to_string_pretty(value)
      .map_err(|e| Failure(e.to_string()))?;
    println!("{text}");
    Ok(())
  }

  /// A progress or result line on stderr, so stdout stays clean for data.
  #[expect(
    clippy::print_stderr,
    reason = "CLI status goes to stderr to keep stdout clean for data"
  )]
  pub fn status(&self, message: impl AsRef<str>) {
    if !self.quiet {
      eprintln!("{}", message.as_ref());
    }
  }

  #[expect(clippy::print_stderr, reason = "CLI warnings go to stderr")]
  pub fn warn(&self, message: impl AsRef<str>) {
    eprintln!("{} {}", self.paint("warning:", "33;1"), message.as_ref());
  }

  #[expect(clippy::print_stderr, reason = "CLI errors go to stderr")]
  pub fn error(&self, message: impl AsRef<str>) {
    eprintln!("{} {}", self.paint("error:", "31;1"), message.as_ref());
  }

  #[expect(clippy::print_stderr, reason = "CLI success notices go to stderr")]
  pub fn success(&self, message: impl AsRef<str>) {
    if !self.quiet {
      eprintln!("{} {}", self.paint("ok", "32;1"), message.as_ref());
    }
  }

  pub fn paint(&self, text: &str, sgr: &str) -> String {
    if matches!(self.color, ColorChoice::Always) {
      format!("\x1b[{sgr}m{text}\x1b[0m")
    } else {
      text.to_string()
    }
  }

  /// Asks before a destructive step. Without a terminal to ask on, only
  /// `--yes` lets it go ahead.
  #[expect(
    clippy::print_stderr,
    reason = "interactive confirmation prompts go to stderr"
  )]
  pub fn confirm(&self, question: &str) -> Result<bool> {
    if self.yes {
      return Ok(true);
    }
    if matches!(self.interaction, Interaction::Unavailable) {
      return fail(format!(
        "{question} Pass --yes to confirm when not running interactively."
      ));
    }
    eprint!("{question} [y/N] ");
    let _ = stderr().flush();
    let mut answer = String::new();
    stdin()
      .read_line(&mut answer)
      .map_err(|e| Failure(e.to_string()))?;
    Ok(matches!(
      answer.trim().to_ascii_lowercase().as_str(),
      "y" | "yes"
    ))
  }

  #[expect(
    clippy::print_stderr,
    reason = "interactive input prompts go to stderr"
  )]
  pub fn prompt(&self, question: &str) -> Result<String> {
    if matches!(self.interaction, Interaction::Unavailable) {
      return fail(format!(
        "{question} needs a terminal; pass the value as an option instead"
      ));
    }
    eprint!("{question}: ");
    let _ = stderr().flush();
    let mut answer = String::new();
    stdin()
      .read_line(&mut answer)
      .map_err(|e| Failure(e.to_string()))?;
    Ok(answer.trim().to_string())
  }

  pub fn prompt_secret(&self, question: &str) -> Result<String> {
    if matches!(self.interaction, Interaction::Unavailable) {
      return fail(format!(
        "{question} needs a terminal; use --password-stdin instead"
      ));
    }
    rpassword::prompt_password(format!("{question}: "))
      .map_err(|e| Failure(e.to_string()))
  }

  /// A reporter that draws progress bars on stderr, or does nothing when
  /// output is not a terminal or is meant for scripts.
  #[expect(
    clippy::literal_string_with_formatting_args,
    reason = "indicatif parses these placeholders as progress templates"
  )]
  pub fn progress(&self) -> Progress {
    if self.quiet || self.json || !stderr().is_terminal() {
      return Progress {
        reporter: Reporter::none(),
        multi:    None,
      };
    }
    let multi = MultiProgress::with_draw_target(ProgressDrawTarget::stderr());
    let spinner = multi.add(ProgressBar::new_spinner());
    spinner.set_style(
      ProgressStyle::with_template("{spinner} {msg}")
        .unwrap_or_else(|_| ProgressStyle::default_spinner()),
    );
    spinner.enable_steady_tick(Duration::from_millis(120));
    let bars: Arc<Mutex<HashMap<String, ProgressBar>>> = Arc::default();
    let bar_style = ProgressStyle::with_template(
      "  {msg:30!} {bar:30} {bytes:>10}/{total_bytes:10} {bytes_per_sec:>12}",
    )
    .unwrap_or_else(|_| ProgressStyle::default_bar());

    let m = multi.clone();
    let reporter = Reporter::new(move |event| {
      match event {
        Event::Step(step) => spinner.set_message(step_text(step)),
        Event::Log(line) => spinner.set_message(line),
        Event::Transfer { label, done, total } => {
          let mut bars = bars.lock().unwrap_or_else(PoisonError::into_inner);
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
          drop(bars);
        },
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
  multi:        Option<MultiProgress>,
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
  reason = "human-readable counts intentionally round to one decimal after \
            scaling"
)]
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
