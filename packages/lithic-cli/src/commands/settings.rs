use std::path::PathBuf;

use comfy_table::Cell;
use lithic_core::Settings;
use lithic_core::paths::expand_home;

use crate::Ctx;
use crate::args::{SettingsCommand, TableCommand, TableName, TablePart};
use crate::style::TableStyle;
use crate::ui::{Result, fail};

/// Settings that can be read and changed from the command line.
const KEYS: &[(&str, &str)] = &[
   (
      "mods.allow_prerelease",
      "offer -dev, -pre and -rc mod releases (true/false)",
   ),
   (
      "mods.index_max_age_hours",
      "hours before the ModDB mod list is fetched again",
   ),
   ("mods.concurrency", "parallel downloads"),
   (
      "backups.enabled",
      "keep a copy of mods before replacing or removing them (true/false)",
   ),
   ("backups.dir", "where backups go"),
   ("backups.keep", "backups kept per mod"),
   ("game.install_dir", "where game versions are installed"),
   ("game.download_dir", "where `lithic game download` saves files"),
   ("gui.theme_mode", "system, light, dark or preset"),
   ("gui.theme_preset", "theme used when theme_mode is preset"),
   ("gui.initial_page", "page the GUI opens on"),
];

pub fn run(ctx: &Ctx, cmd: SettingsCommand) -> Result {
   match cmd {
      SettingsCommand::Show => {
         let settings = ctx.lithic.settings()?;
         if ctx.ui.json {
            return ctx.ui.print_json(&settings);
         }
         let mut table = ctx.ui.table();
         table.set_header(vec!["Key", "Value", "Meaning"]);
         for (key, meaning) in KEYS {
            table.add_row(vec![
               Cell::new(key),
               Cell::new(get(&settings, key)),
               Cell::new(meaning),
            ]);
         }
         ctx.ui.print_table(&table);
         Ok(())
      }
      SettingsCommand::Get { key } => {
         check_key(&key)?;
         println!("{}", get(&ctx.lithic.settings()?, &key));
         Ok(())
      }
      SettingsCommand::Set { key, value } => {
         check_key(&key)?;
         let mut error = None;
         ctx.lithic.update_settings(|s| {
            if let Err(e) = set(s, &key, Some(&value)) {
               error = Some(e);
            }
         })?;
         match error {
            Some(e) => fail(e),
            None => {
               ctx.ui
                  .success(format!("{key} = {}", get(&ctx.lithic.settings()?, &key)));
               Ok(())
            }
         }
      }
      SettingsCommand::Unset { key } => {
         check_key(&key)?;
         ctx.lithic.update_settings(|s| {
            let _ = set(s, &key, None);
         })?;
         ctx.ui
            .success(format!("{key} reset to {}", get(&ctx.lithic.settings()?, &key)));
         Ok(())
      }
      SettingsCommand::Paths => {
         let p = &ctx.lithic.paths;
         let settings = ctx.lithic.settings()?;
         let rows = [
            ("Settings", p.settings_file()),
            ("Instances", p.instances_dir()),
            (
               "Game versions",
               settings
                  .game
                  .install_dir
                  .map(expand_home)
                  .unwrap_or_else(|| p.game_dir()),
            ),
            (
               "Backups",
               settings
                  .backups
                  .dir
                  .map(expand_home)
                  .unwrap_or_else(|| p.backups_dir()),
            ),
            ("Cache", p.cache.clone()),
         ];
         if ctx.ui.json {
            let map: std::collections::BTreeMap<&str, &PathBuf> = rows.iter().map(|(k, v)| (*k, v)).collect();
            return ctx.ui.print_json(&map);
         }
         let mut table = ctx.ui.table();
         for (k, v) in rows {
            table.add_row(vec![Cell::new(k), Cell::new(v.display())]);
         }
         ctx.ui.print_table(&table);
         Ok(())
      }
      SettingsCommand::Table(cmd) => table(ctx, cmd),
   }
}

fn check_key(key: &str) -> Result {
   if KEYS.iter().any(|(k, _)| *k == key) {
      Ok(())
   } else {
      let known: Vec<&str> = KEYS.iter().map(|(k, _)| *k).collect();
      fail(format!(
         "unknown setting `{key}`; known settings: {}",
         known.join(", ")
      ))
   }
}

fn get(s: &Settings, key: &str) -> String {
   let path = |p: &Option<PathBuf>| p.as_ref().map(|p| p.display().to_string()).unwrap_or_default();
   match key {
      "mods.allow_prerelease" => s.mods.allow_prerelease.to_string(),
      "mods.index_max_age_hours" => s.mods.index_max_age_hours.to_string(),
      "mods.concurrency" => s.mods.concurrency.to_string(),
      "backups.enabled" => s.backups.enabled.to_string(),
      "backups.dir" => path(&s.backups.dir),
      "backups.keep" => s.backups.keep.to_string(),
      "game.install_dir" => path(&s.game.install_dir),
      "game.download_dir" => path(&s.game.download_dir),
      "gui.theme_mode" => s.gui.theme_mode.clone(),
      "gui.theme_preset" => s.gui.theme_preset.clone(),
      "gui.initial_page" => s.gui.initial_page.clone(),
      _ => String::new(),
   }
}

/// Sets `key` from text, or back to its default when `value` is `None`.
fn set(s: &mut Settings, key: &str, value: Option<&str>) -> std::result::Result<(), String> {
   let d = Settings::default();
   let flag = |v: Option<&str>, default: bool| -> std::result::Result<bool, String> {
      match v.map(|v| v.trim().to_ascii_lowercase()) {
         None => Ok(default),
         Some(v) if ["true", "yes", "on", "1"].contains(&v.as_str()) => Ok(true),
         Some(v) if ["false", "no", "off", "0"].contains(&v.as_str()) => Ok(false),
         Some(v) => Err(format!("`{v}` is not true or false")),
      }
   };
   let number = |v: Option<&str>, default: u64, min: u64| -> std::result::Result<u64, String> {
      match v {
         None => Ok(default),
         Some(v) => match v.trim().parse::<u64>() {
            Ok(n) if n >= min => Ok(n),
            _ => Err(format!("`{v}` must be a whole number of at least {min}")),
         },
      }
   };
   let dir = |v: Option<&str>| v.filter(|v| !v.trim().is_empty()).map(|v| expand_home(v.trim()));
   let text =
      |v: Option<&str>, default: &str| v.map_or_else(|| default.to_string(), |v| v.trim().to_string());

   match key {
      "mods.allow_prerelease" => s.mods.allow_prerelease = flag(value, d.mods.allow_prerelease)?,
      "mods.index_max_age_hours" => {
         let n = number(value, u64::from(d.mods.index_max_age_hours), 1)?;
         s.mods.index_max_age_hours = u32::try_from(n).map_err(|_| "too large".to_string())?;
      }
      "mods.concurrency" => {
         let n = number(value, d.mods.concurrency as u64, 1)?;
         s.mods.concurrency = usize::try_from(n.min(32)).unwrap_or(32);
      }
      "backups.enabled" => s.backups.enabled = flag(value, d.backups.enabled)?,
      "backups.dir" => s.backups.dir = dir(value),
      "backups.keep" => {
         let n = number(value, d.backups.keep as u64, 1)?;
         s.backups.keep = usize::try_from(n).unwrap_or(usize::MAX);
      }
      "game.install_dir" => s.game.install_dir = dir(value),
      "game.download_dir" => s.game.download_dir = dir(value),
      "gui.theme_mode" => {
         let mode = text(value, &d.gui.theme_mode).to_ascii_lowercase();
         if !["system", "light", "dark", "preset"].contains(&mode.as_str()) {
            return Err(format!("`{mode}` is not system, light, dark or preset"));
         }
         s.gui.theme_mode = mode;
      }
      "gui.theme_preset" => s.gui.theme_preset = text(value, &d.gui.theme_preset),
      "gui.initial_page" => s.gui.initial_page = text(value, &d.gui.initial_page),
      _ => return Err(format!("unknown setting `{key}`")),
   }
   Ok(())
}

fn table(ctx: &Ctx, cmd: TableCommand) -> Result {
   match cmd {
      TableCommand::Show => {
         let settings = ctx.lithic.settings()?;
         if ctx.ui.json {
            return ctx.ui.print_json(&settings.cli.table);
         }
         let columns = [
            (
               "list",
               ["name", "mod_id", "version", "state", "update", "filename"].as_slice(),
            ),
         ];
         let mut out = ctx.ui.table();
         out.set_header(vec!["Table", "Column", "Header", "Cells"]);
         for (name, cols) in columns {
            let style = TableStyle::load(&settings.cli.table, name);
            for col in cols {
               let describe = |part: &str| {
                  let look = style.look(part, col);
                  [look.color.map(|c| c.key()), look.attr.map(|a| a.key())]
                     .into_iter()
                     .flatten()
                     .collect::<Vec<_>>()
                     .join(" ")
               };
               out.add_row(vec![
                  Cell::new(name),
                  style.cell(col, col),
                  Cell::new(describe("headers")),
                  Cell::new(describe("cells")),
               ]);
            }
         }
         ctx.ui.print_table(&out);
         Ok(())
      }
      TableCommand::Set {
         table,
         part,
         column,
         color,
         attribute,
      } => {
         if color.is_none() && attribute.is_none() {
            return fail("give --color, --attribute, or both");
         }
         ctx.lithic.update_settings(|s| {
            let section = section_mut(&mut s.cli.table, table, part);
            if let Some(c) = color {
               section.insert(format!("{column}.color"), toml::Value::String(c.key().into()));
            }
            if let Some(a) = attribute {
               section.insert(format!("{column}.attribute"), toml::Value::String(a.key().into()));
            }
         })?;
         ctx.ui
            .success(format!("updated the {} {} of {column}", table.key(), part.key()));
         Ok(())
      }
      TableCommand::Reset { table } => {
         ctx.lithic.update_settings(|s| match table {
            Some(t) => {
               s.cli.table.remove(t.key());
            }
            None => s.cli.table.clear(),
         })?;
         ctx.ui.success("table colours reset");
         Ok(())
      }
   }
}

fn section_mut(root: &mut toml::Table, table: TableName, part: TablePart) -> &mut toml::Table {
   let t = root
      .entry(table.key())
      .or_insert_with(|| toml::Value::Table(toml::Table::new()));
   if !t.is_table() {
      *t = toml::Value::Table(toml::Table::new());
   }
   let toml::Value::Table(t) = t else {
      unreachable!("replaced with a table above")
   };
   let p = t
      .entry(part.key())
      .or_insert_with(|| toml::Value::Table(toml::Table::new()));
   if !p.is_table() {
      *p = toml::Value::Table(toml::Table::new());
   }
   let toml::Value::Table(p) = p else {
      unreachable!("replaced with a table above")
   };
   p
}

#[cfg(test)]
mod tests {
   use super::*;

   #[test]
   fn every_key_round_trips() {
      for (key, _) in KEYS {
         let mut s = Settings::default();
         let sample = match *key {
            k if k.ends_with("allow_prerelease") || k.ends_with("enabled") => "true",
            k if k.ends_with("_dir") || k.ends_with(".dir") => "/tmp/x",
            "gui.theme_mode" => "dark",
            k if k.starts_with("gui.") => "x",
            _ => "7",
         };
         set(&mut s, key, Some(sample)).unwrap_or_else(|e| panic!("{key}: {e}"));
         assert_eq!(get(&s, key), sample, "{key}");
         set(&mut s, key, None).unwrap();
         assert_eq!(get(&s, key), get(&Settings::default(), key), "{key} reset");
      }
   }

   #[test]
   fn bad_values_are_rejected() {
      let mut s = Settings::default();
      assert!(set(&mut s, "mods.concurrency", Some("0")).is_err());
      assert!(set(&mut s, "backups.enabled", Some("maybe")).is_err());
      assert!(set(&mut s, "gui.theme_mode", Some("neon")).is_err());
   }
}
