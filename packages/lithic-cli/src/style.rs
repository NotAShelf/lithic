//! Table colours, configured under `[cli.table]` in settings.toml.
//!
//! The layout is the one lithic 1.x used, so migrated settings keep working:
//!
//! ```toml
//! [cli.table.list.headers]
//! "name.color" = "green"
//! "name.attribute" = "bold"
//!
//! [cli.table.search.cells]
//! "mod_id.color" = "magenta"
//! ```

use clap::ValueEnum;
use comfy_table::{Attribute, Cell, Color};
use toml::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum CellColor {
   Black,
   Red,
   Green,
   Yellow,
   Blue,
   Magenta,
   Cyan,
   White,
   Grey,
   DarkRed,
   DarkGreen,
   DarkYellow,
   DarkBlue,
   DarkMagenta,
   DarkCyan,
   DarkGrey,
   Reset,
}

impl CellColor {
   pub const fn key(self) -> &'static str {
      match self {
         Self::Black => "black",
         Self::Red => "red",
         Self::Green => "green",
         Self::Yellow => "yellow",
         Self::Blue => "blue",
         Self::Magenta => "magenta",
         Self::Cyan => "cyan",
         Self::White => "white",
         Self::Grey => "grey",
         Self::DarkRed => "dark_red",
         Self::DarkGreen => "dark_green",
         Self::DarkYellow => "dark_yellow",
         Self::DarkBlue => "dark_blue",
         Self::DarkMagenta => "dark_magenta",
         Self::DarkCyan => "dark_cyan",
         Self::DarkGrey => "dark_grey",
         Self::Reset => "reset",
      }
   }

   pub fn from_key(key: &str) -> Option<Self> {
      let key = key.trim().to_ascii_lowercase().replace('-', "_");
      Self::value_variants().iter().copied().find(|c| c.key() == key)
   }

   const fn to_comfy(self) -> Color {
      match self {
         Self::Black => Color::Black,
         Self::Red => Color::Red,
         Self::Green => Color::Green,
         Self::Yellow => Color::Yellow,
         Self::Blue => Color::Blue,
         Self::Magenta => Color::Magenta,
         Self::Cyan => Color::Cyan,
         Self::White => Color::White,
         Self::Grey => Color::Grey,
         Self::DarkRed => Color::DarkRed,
         Self::DarkGreen => Color::DarkGreen,
         Self::DarkYellow => Color::DarkYellow,
         Self::DarkBlue => Color::DarkBlue,
         Self::DarkMagenta => Color::DarkMagenta,
         Self::DarkCyan => Color::DarkCyan,
         Self::DarkGrey => Color::DarkGrey,
         Self::Reset => Color::Reset,
      }
   }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum CellAttr {
   Bold,
   Italic,
   Underline,
   Dim,
   Reset,
}

impl CellAttr {
   pub const fn key(self) -> &'static str {
      match self {
         Self::Bold => "bold",
         Self::Italic => "italic",
         Self::Underline => "underline",
         Self::Dim => "dim",
         Self::Reset => "reset",
      }
   }

   pub fn from_key(key: &str) -> Option<Self> {
      let key = key.trim().to_ascii_lowercase();
      // 1.x wrote "nohidden" as its "no attribute" value.
      if key == "nohidden" {
         return Some(Self::Reset);
      }
      Self::value_variants().iter().copied().find(|a| a.key() == key)
   }

   const fn to_comfy(self) -> Attribute {
      match self {
         Self::Bold => Attribute::Bold,
         Self::Italic => Attribute::Italic,
         Self::Underline => Attribute::Underlined,
         Self::Dim => Attribute::Dim,
         Self::Reset => Attribute::NormalIntensity,
      }
   }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Look {
   pub color: Option<CellColor>,
   pub attr: Option<CellAttr>,
}

/// Colours for one table, defaults merged with the user's settings.
#[derive(Debug, Clone)]
pub struct TableStyle<'a> {
   table: &'a toml::Table,
   name: &'static str,
}

impl<'a> TableStyle<'a> {
   pub const fn load(settings: &'a toml::Table, name: &'static str) -> Self {
      Self {
         table: settings,
         name,
      }
   }

   pub fn look(&self, part: &str, column: &str) -> Look {
      let user = self
         .table
         .get(self.name)
         .and_then(|t| t.get(part))
         .and_then(Value::as_table);
      let read = |suffix: &str| {
         user
            .and_then(|t| t.get(&format!("{column}.{suffix}")))
            .and_then(Value::as_str)
      };
      let default = default_look(self.name, part, column);
      Look {
         color: read("color").and_then(CellColor::from_key).or(default.color),
         attr: read("attribute").and_then(CellAttr::from_key).or(default.attr),
      }
   }

   pub fn header(&self, column: &str, text: &str) -> Cell {
      apply(Cell::new(text), self.look("headers", column))
   }

   pub fn cell(&self, column: &str, text: impl ToString) -> Cell {
      apply(Cell::new(text), self.look("cells", column))
   }
}

fn apply(mut cell: Cell, look: Look) -> Cell {
   if let Some(color) = look.color {
      cell = cell.fg(color.to_comfy());
   }
   if let Some(attr) = look.attr {
      cell = cell.add_attribute(attr.to_comfy());
   }
   cell
}

fn default_look(table: &str, part: &str, column: &str) -> Look {
   if part == "headers" {
      return Look {
         color: Some(CellColor::Green),
         attr: Some(CellAttr::Bold),
      };
   }
   let (color, attr) = match (table, column) {
      ("list", "name") => (Some(CellColor::Yellow), None),
      ("list", "version") => (None, Some(CellAttr::Dim)),
      ("list", "update") => (Some(CellColor::Green), Some(CellAttr::Bold)),
      ("list", "problems") => (Some(CellColor::Red), Some(CellAttr::Bold)),
      ("search", "mod_id") => (Some(CellColor::Magenta), Some(CellAttr::Bold)),
      _ => (None, None),
   };
   Look { color, attr }
}

#[cfg(test)]
#[expect(
   clippy::unwrap_used,
   reason = "test setup and assertions intentionally fail on error"
)]
mod tests {
   use super::*;

   #[test]
   fn user_settings_override_defaults() {
      let settings: toml::Table = toml::from_str(
         "[list.headers]\n\"name.color\" = \"dark_red\"\n[list.cells]\n\"name.attribute\" = \"nohidden\"\n",
      )
      .unwrap();
      let style = TableStyle::load(&settings, "list");
      assert_eq!(style.look("headers", "name").color, Some(CellColor::DarkRed));
      assert_eq!(style.look("headers", "name").attr, Some(CellAttr::Bold));
      assert_eq!(style.look("cells", "name").color, Some(CellColor::Yellow));
      assert_eq!(style.look("cells", "name").attr, Some(CellAttr::Reset));
   }

   #[test]
   fn colour_keys_accept_both_spellings() {
      assert_eq!(CellColor::from_key("dark-grey"), Some(CellColor::DarkGrey));
      assert_eq!(CellColor::from_key("DARK_GREY"), Some(CellColor::DarkGrey));
      assert_eq!(CellColor::from_key("cyan"), Some(CellColor::Cyan));
      assert_eq!(CellColor::from_key("mauve"), None);
   }
}
