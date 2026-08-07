//! Table colours, configured under `[cli.table]` in settings.toml.
//!
//! The table layout is configured through CLI settings:
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
   pub fn key(self) -> &'static str {
      match self {
         CellColor::Black => "black",
         CellColor::Red => "red",
         CellColor::Green => "green",
         CellColor::Yellow => "yellow",
         CellColor::Blue => "blue",
         CellColor::Magenta => "magenta",
         CellColor::Cyan => "cyan",
         CellColor::White => "white",
         CellColor::Grey => "grey",
         CellColor::DarkRed => "dark_red",
         CellColor::DarkGreen => "dark_green",
         CellColor::DarkYellow => "dark_yellow",
         CellColor::DarkBlue => "dark_blue",
         CellColor::DarkMagenta => "dark_magenta",
         CellColor::DarkCyan => "dark_cyan",
         CellColor::DarkGrey => "dark_grey",
         CellColor::Reset => "reset",
      }
   }

   pub fn from_key(key: &str) -> Option<Self> {
      let key = key.trim().to_ascii_lowercase().replace('-', "_");
      Self::value_variants().iter().copied().find(|c| c.key() == key)
   }

   fn to_comfy(self) -> Color {
      match self {
         CellColor::Black => Color::Black,
         CellColor::Red => Color::Red,
         CellColor::Green => Color::Green,
         CellColor::Yellow => Color::Yellow,
         CellColor::Blue => Color::Blue,
         CellColor::Magenta => Color::Magenta,
         CellColor::Cyan => Color::Cyan,
         CellColor::White => Color::White,
         CellColor::Grey => Color::Grey,
         CellColor::DarkRed => Color::DarkRed,
         CellColor::DarkGreen => Color::DarkGreen,
         CellColor::DarkYellow => Color::DarkYellow,
         CellColor::DarkBlue => Color::DarkBlue,
         CellColor::DarkMagenta => Color::DarkMagenta,
         CellColor::DarkCyan => Color::DarkCyan,
         CellColor::DarkGrey => Color::DarkGrey,
         CellColor::Reset => Color::Reset,
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
   pub fn key(self) -> &'static str {
      match self {
         CellAttr::Bold => "bold",
         CellAttr::Italic => "italic",
         CellAttr::Underline => "underline",
         CellAttr::Dim => "dim",
         CellAttr::Reset => "reset",
      }
   }

   pub fn from_key(key: &str) -> Option<Self> {
      let key = key.trim().to_ascii_lowercase();
      // 1.x wrote "nohidden" as its "no attribute" value.
      if key == "nohidden" {
         return Some(CellAttr::Reset);
      }
      Self::value_variants().iter().copied().find(|a| a.key() == key)
   }

   fn to_comfy(self) -> Attribute {
      match self {
         CellAttr::Bold => Attribute::Bold,
         CellAttr::Italic => Attribute::Italic,
         CellAttr::Underline => Attribute::Underlined,
         CellAttr::Dim => Attribute::Dim,
         CellAttr::Reset => Attribute::NormalIntensity,
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
   pub fn load(settings: &'a toml::Table, name: &'static str) -> Self {
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
         .and_then(toml::Value::as_table);
      let read = |suffix: &str| {
         user
            .and_then(|t| t.get(&format!("{column}.{suffix}")))
            .and_then(toml::Value::as_str)
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
      apply(Cell::new(text.to_string()), self.look("cells", column))
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
