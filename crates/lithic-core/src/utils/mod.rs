use crate::aliases::{ModFileName, ModID, ModVersion};
use crate::api::structs::{ModApi, ModInfo};
use crate::config::manager::Config;
use crate::config::manager::get_config;
use crate::config::structs::{CellAttr, CellColor};
use crate::consts::{FILE_GAME_VERSION_SYNC, FILE_LITHIC_SYNC, FILE_MODINFO_JSON};
use crate::errors::LithicError;
use crate::installer::manager::{Install, Installed};
use crate::symlink_manager::SymlinkManager;
use crate::sync::structs::{GameVersionSync, ModSyncInfo};
use crate::version::manager::parse_version;
use async_zip::tokio::read::fs::ZipFileReader;
use chrono::{DateTime, Duration, NaiveDateTime, Utc};
use comfy_table::ContentArrangement::Dynamic;
use comfy_table::modifiers::UTF8_ROUND_CORNERS;
use comfy_table::presets::{UTF8_BORDERS_ONLY, UTF8_FULL_CONDENSED, UTF8_HORIZONTAL_ONLY};
use comfy_table::{Attribute, Cell, CellAlignment, Color, ContentArrangement, Row, Table};
use dirs::home_dir;
use futures::{StreamExt, stream};
use serde_json::to_string_pretty;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;
use yansi::Paint;

use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tracing::{debug, info, warn};

pub fn get_current_time() -> String {
   let datetime: DateTime<Utc> = Utc::now();
   datetime.format("%Y-%m-%d %H:%M").to_string()
}

pub fn timestamp_older_than(num_hours: i64, timestamp: &str) -> Result<bool, LithicError> {
   let naive_dt = NaiveDateTime::parse_from_str(timestamp, "%Y-%m-%d %H:%M")
      .map_err(|e| LithicError::SimpleError(format!("Failed to parse timestamp '{timestamp}': {e}")))?;
   let now = Utc::now().naive_utc();
   let duration = now.signed_duration_since(naive_dt);

   Ok(duration > Duration::hours(num_hours))
}

// if the path contains ~/, which is short for /home/<user>, then expand it, otherwise just return
// the path,
// TODO: Need handle windows default
pub fn get_expanded_path(dir: impl AsRef<Path>) -> PathBuf {
   let dir = dir.as_ref();
   if dir.starts_with("~/")
      && let Some(home) = home_dir()
      && let Ok(rel) = dir.strip_prefix("~")
   {
      return home.join(rel);
   }

   dir.to_path_buf()
}

pub async fn extract_zip_metadata<T>(entry: impl AsRef<Path>, inner_file: &str) -> Result<T, LithicError>
where
   T: for<'de> serde::Deserialize<'de>,
{
   let entry = entry.as_ref();
   // This function doesn't need async as it's doing synchronous file operations
   if entry.is_dir() {
      return Err(LithicError::ModNotZipped(entry.display().to_string()));
   }
   if entry.extension().is_some_and(|x| !x.eq_ignore_ascii_case("zip")) {
      return Err(LithicError::SimpleError(format!(
         "Skipping non-zip file: {}",
         entry.display()
      )));
   }

   let archive = ZipFileReader::new(entry)
      .await
      .map_err(|e| LithicError::ZipError {
         context: format!("Failed to open zip archive {:?}: {}", entry.file_name(), e),
         source: e,
      })?;

   // Locate the file we want
   let entry_index = archive
      .file()
      .entries()
      .iter()
      .position(|e| {
          e.filename()
             .as_str()
             .is_ok_and(|s| s.eq_ignore_ascii_case(inner_file))
      })
      .ok_or_else(|| LithicError::ZipError {
         context: format!("Failed to find {} in {:?}", inner_file, entry.file_name()),
         source: async_zip::error::ZipError::UnableToLocateEOCDR,
      })?;

   let mut entry_reader =
      archive
         .reader_with_entry(entry_index)
         .await
         .map_err(|e| LithicError::ZipError {
            context: format!("Failed to read {} in {:?}", inner_file, entry.file_name()),
            source: e,
         })?;

   // read the content of the file inner_file
   let mut mod_info_contents = String::new();
   entry_reader
      .read_to_string_checked(&mut mod_info_contents)
      .await
      .map_err(|e| LithicError::ZipError {
         context: format!("Failed to read {} in {:?}", inner_file, entry.file_name()),
         source: e,
      })?;

   let mod_info = if inner_file.to_lowercase().ends_with(".json") {
      serde_json5::from_str::<T>(&mod_info_contents).map_err(|e: serde_json5::Error| {
         LithicError::JsonError {
            context: format!(
               "Failed to parse json in {}",
               entry.file_name().unwrap_or_default().to_string_lossy()
            ),
            source: e,
         }
      })?
   } else if inner_file.to_lowercase().ends_with(".toml") {
      toml::from_str::<T>(&mod_info_contents).map_err(|e| LithicError::TomlError {
         context: format!(
            "Failed to parse toml in {}",
            entry.file_name().unwrap_or_default().to_string_lossy()
         ),
         source: e,
      })?
   } else {
      return Err(LithicError::SimpleError(format!(
         "Unsupported file format {inner_file}"
      )));
   };

   Ok(mod_info)
}

pub async fn extract_all_mods_metadata(
   mod_dir: impl AsRef<Path>,
   ignore_symlink: bool,
) -> Result<HashMap<ModFileName, ModInfo>, LithicError> {
   let mod_dir = mod_dir.as_ref();
   let mut dir = tokio::fs::read_dir(mod_dir)
      .await
      .map_err(|e| LithicError::IoError {
         context: format!("Can't read mod_dir: {}", mod_dir.to_string_lossy()),
         source: e,
      })?;
   let mut entries = Vec::new();

   while let Some(entry) = dir.next_entry().await? {
      entries.push(entry);
   }

   let concurrent_limit = num_cpus::get();

   let config = get_config().read().await;
   // Create a local copy of the data needed so we can drop the config
   let notif_unzipped_mods = config.notify_of_unzipped_mods;
   drop(config); // manually drop the config, we don't actually need it anymore

   let results: Vec<(ModFileName, ModInfo)> = stream::iter(entries)
      // This is to ignore modpack mods when using normal lithic commands while a modpack is enabled
      .filter(|e| futures::future::ready(!(ignore_symlink && SymlinkManager::exists(e.path()))))
      .map(|entry| async move {
         let filename: ModFileName = entry.file_name().to_string_lossy().to_string().into();
         extract_zip_metadata::<ModInfo>(&entry.path(), FILE_MODINFO_JSON)
            .await
            .map(|mod_info| (filename, mod_info))
            .inspect_err(|e| {
               if matches!(e, LithicError::ModNotZipped(_)) && notif_unzipped_mods {
                  println!("{}", e.to_string().yellow());
               } else {
                  debug!("{}", e.to_string().yellow());
               }
            })
            .ok()
      })
      .buffer_unordered(concurrent_limit)
      .filter_map(futures::future::ready)
      .collect()
      .await;

   Ok(results.into_iter().collect())
}

// TODO: Decide if this function is needed
#[allow(dead_code)]
pub async fn verify_zip_file(file_path: impl AsRef<Path>) -> Result<(), LithicError> {
   // Open and verify the zip file integrity
   let file_path = file_path.as_ref();

   let archive = ZipFileReader::new(file_path)
      .await
      .map_err(|e| LithicError::ZipError {
         context: format!("Invalid zip file: {}", file_path.to_string_lossy()),
         source: e,
      })?;

   // Check that the archive contains at least one file
   if archive.file().entries().is_empty() {
      return Err(LithicError::SimpleError(format!(
         "Zip file is empty: {}",
         file_path.to_string_lossy()
      )));
   }

   Ok(())
}

pub async fn delete_file(file: impl AsRef<Path>) -> Result<(), LithicError> {
   let file = file.as_ref();
   debug!("Trying to delete {}", file.display());
   if file.is_dir() {
      return Err(LithicError::SimpleError(format!(
         "Expected a file, found a directory: {}",
         file.display()
      )));
   }

   if file.exists() {
      tokio::fs::remove_file(file)
         .await
         .map_err(|e| LithicError::IoError {
            context: format!(
               "Failed attempting to delete {}",
               file.file_name().unwrap_or_default().to_string_lossy()
            ),
            source: e,
         })
   } else {
      Err(LithicError::SimpleError(format!(
         "File {} does not exist",
         file.display()
      )))
   }
}

// Helper function to get just installed dependencies by passing empty vec and hashmap to the parts that filter out dependencies
pub fn gather_dependencies(installed_mods: &HashMap<ModFileName, ModInfo>) -> Vec<Install> {
   gather_missing_dependencies(installed_mods, &[], &HashMap::new())
}

pub fn gather_missing_dependencies<V: AsRef<[ModID]>>(
   installed_mods: &HashMap<ModFileName, ModInfo>,
   mods_requested: V,
   sync_data: &HashMap<String, ModSyncInfo>,
) -> Vec<Install> {
   // if there are reports of slowness is this section .values().par_bridge()...flat_map_iter() could be used to speed it up
   // this is prob not an issue even with a lot of mods as the data is all in memory at this point
   let id_vec: Vec<ModID> = sync_data.keys().map(|m| split_modid_version(m).0).collect();

   let mods_requested = mods_requested.as_ref();

   installed_mods
      // .values()
      .iter()
      .filter(|(_, mod_info)| {
         mods_requested.is_empty() || mods_requested.iter().any(|m| m == &mod_info.mod_id)
      })
      .flat_map(|(mod_filename, mod_info)| {
         mod_info
            .dependencies
            .iter()
            .filter_map(|(mod_id, version)| {
               if !mod_id.contains("game")
                  && !mod_id.contains("survival")
                  && !mod_id.contains("creative")
                  && !id_vec.iter().any(|m| m == mod_id)
               {
                  Some(Install {
                     mod_id: mod_id.clone(),
                     mod_name: "".into(),
                     version_to_install: version.clone(),
                     download_url: "".into(),
                     current_file_path: Some(PathBuf::from(mod_filename)),
                  })
               } else {
                  None
               }
            })
            .collect::<Vec<_>>()
            .into_iter()
      })
      .collect()
}

pub async fn parse_json_file<T>(file_path: impl AsRef<Path>) -> Result<T, LithicError>
where
   T: for<'de> serde::Deserialize<'de>,
{
   let file_path = file_path.as_ref();
   let filename = file_path
      .file_name()
      .unwrap_or_default()
      .to_string_lossy()
      .to_string();

   let mut file = File::open(file_path).await.map_err(|e| LithicError::IoError {
      context: format!("Unable to open {filename}"),
      source: e,
   })?;

   let mut file_contents = String::new();
   file
      .read_to_string(&mut file_contents)
      .await
      .map_err(|e| LithicError::IoError {
         context: format!("Failure while reading from file {filename}"),
         source: e,
      })?;

   let json = serde_json5::from_str::<T>(&file_contents).map_err(|e| {
      let sync_error = if filename.eq(FILE_LITHIC_SYNC) {
         format!(
            "{} {} {}",
            "(Run".yellow(),
            "Lithic sync".blue(),
            "to repopulate the sync file and resolve this message)".yellow()
         )
      } else {
         String::new()
      };
      LithicError::JsonError {
         context: format!("Json parsing Error for {filename} {sync_error}"),
         source: e,
      }
   })?;

   Ok(json)
}

pub async fn write_json_file(
   file_path: impl AsRef<Path>,
   json: String,
   config_dir: impl AsRef<Path>,
) -> Result<(), LithicError> {
   let (file_path, config_dir) = (file_path.as_ref(), config_dir.as_ref());
   write_atomic_async(file_path, json.as_bytes())
      .await
      .map_err(|e| match e {
         LithicError::IoError { source, .. } => LithicError::IoError {
            context: format!(
               "Error writing sync mod search file to config dir: {}",
               config_dir.to_string_lossy()
            ),
            source,
         },
         other => other,
      })
}

/// Atomically write `bytes` to `path` by writing to a sibling temp file and
/// renaming over the target. A crash mid-write leaves the prior contents intact
/// rather than a truncated/corrupt file. Synchronous variant for code paths
/// that are not in an async context (config saves at startup).
pub fn write_atomic_sync(path: impl AsRef<Path>, bytes: &[u8]) -> Result<(), LithicError> {
   use std::io::Write;
   let path = path.as_ref();
   let parent = path.parent().unwrap_or_else(|| Path::new("."));
   std::fs::create_dir_all(parent).map_err(|e| LithicError::IoError {
      context: format!("creating parent directory for {}", path.display()),
      source: e,
   })?;
   let tmp = tempfile_path(path);
   {
      let mut f = std::fs::File::create(&tmp).map_err(|e| LithicError::IoError {
         context: format!("creating temp file {}", tmp.display()),
         source: e,
      })?;
      f.write_all(bytes).map_err(|e| LithicError::IoError {
         context: format!("writing temp file {}", tmp.display()),
         source: e,
      })?;
      f.sync_all().map_err(|e| LithicError::IoError {
         context: format!("fsync temp file {}", tmp.display()),
         source: e,
      })?;
   }
   std::fs::rename(&tmp, path).map_err(|e| LithicError::IoError {
      context: format!("rename {} -> {}", tmp.display(), path.display()),
      source: e,
   })
}

/// Async variant of [`write_atomic_sync`].
pub async fn write_atomic_async(path: impl AsRef<Path>, bytes: &[u8]) -> Result<(), LithicError> {
   let path = path.as_ref();
   let parent = path.parent().unwrap_or_else(|| Path::new("."));
   tokio::fs::create_dir_all(parent).await.map_err(|e| LithicError::IoError {
      context: format!("creating parent directory for {}", path.display()),
      source: e,
   })?;
   let tmp = tempfile_path(path);
   {
      let mut f = File::create(&tmp).await.map_err(|e| LithicError::IoError {
         context: format!("creating temp file {}", tmp.display()),
         source: e,
      })?;
      AsyncWriteExt::write_all(&mut f, bytes).await.map_err(|e| LithicError::IoError {
         context: format!("writing temp file {}", tmp.display()),
         source: e,
      })?;
      f.sync_all().await.map_err(|e| LithicError::IoError {
         context: format!("fsync temp file {}", tmp.display()),
         source: e,
      })?;
   }
   tokio::fs::rename(&tmp, path).await.map_err(|e| LithicError::IoError {
      context: format!("rename {} -> {}", tmp.display(), path.display()),
      source: e,
   })
}

fn tempfile_path(target: &Path) -> PathBuf {
   let pid = std::process::id();
   let nanos = std::time::SystemTime::now()
      .duration_since(std::time::UNIX_EPOCH)
      .map(|d| d.as_nanos())
      .unwrap_or(0);
   let mut tmp = target.as_os_str().to_owned();
   tmp.push(format!(".tmp.{pid}.{nanos}"));
   PathBuf::from(tmp)
}

#[cfg(test)]
mod atomic_write_tests {
   use super::write_atomic_sync;

   #[test]
   fn atomic_write_replaces_existing_contents() {
      let path = std::env::temp_dir().join(format!(
         "lithic-atomic-write-test-{}",
         std::process::id()
      ));
      std::fs::write(&path, b"old").unwrap();

      write_atomic_sync(&path, b"new").unwrap();

      assert_eq!(std::fs::read(&path).unwrap(), b"new");
      std::fs::remove_file(path).unwrap();
   }
}

pub async fn sorted_game_versions() -> Result<Vec<String>, LithicError> {
   let version_file_path = Config::get_path().join(FILE_GAME_VERSION_SYNC);

   let mut versions = if version_file_path.exists() {
      parse_json_file::<GameVersionSync>(&version_file_path)
         .await?
         .game_versions
   } else {
      return Err(LithicError::SimpleError(
         "Unable to get latest game version by default, run Lithic sync and try again".into(),
      ));
   };

   versions.sort_by(
      |v1, v2| match (lenient_semver::parse(v1), lenient_semver::parse(v2)) {
         (Ok(a), Ok(b)) => a.cmp(&b),
         (Ok(_), Err(_)) => std::cmp::Ordering::Greater,
         (Err(_), Ok(_)) => std::cmp::Ordering::Less,
         (Err(_), Err(_)) => std::cmp::Ordering::Equal,
      },
   );

   versions.reverse();
   Ok(versions)
}

/// Returns mod_id as lowercase
pub fn find_mod_id<V: AsRef<[ModApi]>>(
   mod_name: &String,
   mod_filename: &ModFileName,
   mods_search_data: V,
) -> Result<String, LithicError> {
   let mods_search_data = mods_search_data.as_ref();
   info!(
      "{} has an empty mod id, attempting locate mod id...",
      mod_filename
   );
   let res: Vec<ModApi> = mods_search_data
      .iter()
      .filter(|mod_search| match &mod_search.name {
         Some(name) => mod_name.eq_ignore_ascii_case(name),
         None => mod_search.mod_id_strs.contains(mod_name),
      })
      .cloned()
      .collect();

   if res.is_empty() || res.len() > 1 {
      // no mods match
      warn!(
         "Unable to determine the mod_id for {} - {}.\n\r\t Their modinfo.json is malformed and no information provided allowed Lithic to determine it.\n\r\t \
                     Please contact the author to correct their modinfo.json file",
         mod_name.bright_red().bold(),
         mod_filename.bright_red().bold()
      );
      Err(LithicError::SimpleError(format!(
         "Unable to locate mod_id for {mod_name}"
      )))
   } else {
      Ok(res[0].mod_id.to_string().to_lowercase())
   }
}

/// Removes older files after updates
///
/// processed_install: Vec<Installed>
pub async fn remove_older_files(processed_install: &[Installed]) -> Result<(), LithicError> {
   for mod_installed in processed_install {
      if let (Some(old), Some(new)) = (&mod_installed.old_file_path, &mod_installed.installed_file_path) {
         if old == new {
            info!("Old file and new file have the same name, **NOT DELETING**");
         } else {
            info!("Cleaning up mod file for {}", old.display());
            delete_file(old).await?;
         }
      }
   }
   Ok(())
}

pub async fn backup_older_files(processed_install: &[Installed]) -> Result<(), LithicError> {
   let config = get_config().read().await;
   let backup_dir = Path::new(&config.backup_mods_dir);

   if !backup_dir.exists() {
      tokio::fs::create_dir_all(backup_dir).await?;
   }

   for m in processed_install {
      if let Some(old_path) = &m.old_file_path
         && let Some(old_file_name) = old_path.file_name()
      {
         tokio::fs::copy(old_path, backup_dir.join(old_file_name)).await?;
      }
   }

   display_table(
      vec![(
         CellData::new(
            "Updated mods have been backed up to:".into(),
            Some(Color::Green),
            vec![],
            None,
         ),
         CellData::new(
            format!("{}", backup_dir.display()),
            Some(Color::Magenta),
            vec![],
            None,
         ),
      )],
      Some(UTF8_HORIZONTAL_ONLY),
   );

   Ok(())
}

pub fn split_modid_version(mod_id_str: impl AsRef<str>) -> (ModID, Option<ModVersion>) {
   if let Some((modid, version)) = mod_id_str
      .as_ref()
      .strip_prefix("vintagestorymodinstall://")
      .unwrap_or(mod_id_str.as_ref())
      .split_once('@')
   {
      match parse_version(version) {
         Ok(p_ver) => return (modid.to_string().into(), Some(p_ver.to_string().into())),
         Err(e) => {
            warn!(
               "Failed to parse version @{} from {}: {}. Version must be in MAJOR.MINOR.PATCH format.",
               version,
               mod_id_str.as_ref(),
               e
            );
         }
      }
   }

   (mod_id_str.as_ref().to_lowercase().into(), None)
}

pub fn format_for_csv(input: impl AsRef<str>) -> String {
   let input = normalize_whitespace(input.as_ref());
   if input.contains(',') || input.contains('"') {
      format!("\"{}\"", input.replace('"', "\"\""))
   } else {
      input
   }
}

pub fn normalize_whitespace(input: impl AsRef<str>) -> String {
   input.as_ref().split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn html_parse(input: &mut impl AsRef<str>, width: usize) -> Result<String, LithicError> {
   html2text::from_read(&mut input.as_ref().as_bytes(), width)
      .map_err(|_| LithicError::SimpleError("html2txt failed".to_string()))
}

pub fn prettify<T>(data: T, command_type: impl AsRef<str>) -> Result<String, LithicError>
where
   T: serde::Serialize,
{
   to_string_pretty(&data).map_err(|e| LithicError::JsonError {
      context: format!("Failure while making the {} json pretty", command_type.as_ref()),
      source: serde_json5::Error::from(std::io::Error::other(e)),
   })
}

pub struct LithicMessage {
   pub header: Option<CellData>,
   pub message: Vec<CellData>,
}

pub fn lithic_message(lithic_message: LithicMessage) {
   let mut table = Table::new();
   table
      .load_preset(UTF8_BORDERS_ONLY)
      .apply_modifier(UTF8_ROUND_CORNERS)
      .set_content_arrangement(Dynamic);

   if let Some(header_data) = lithic_message.header {
      let mut h_cell = Cell::new(header_data.text);
      if !header_data.attributes.is_empty() {
         h_cell = h_cell.add_attributes(header_data.attributes);
      }
      h_cell = h_cell
         .fg(header_data.color.unwrap_or(Color::Green))
         .set_alignment(header_data.alignment.unwrap_or(CellAlignment::Center));

      let mut row = Row::new();
      row.add_cell(h_cell);

      table.set_header(row);
   }

   let rows: Vec<Row> = lithic_message
      .message
      .iter()
      .map(|message_data| {
         let mut cell = Cell::from(message_data.text.clone());

         if !message_data.attributes.is_empty() {
            for attr in &message_data.attributes {
               cell = cell.add_attribute(*attr);
            }
         }
         cell = cell
            .fg(message_data.color.unwrap_or(Color::Yellow))
            .set_alignment(message_data.alignment.unwrap_or(CellAlignment::Center));

         let mut row = Row::new();
         row.add_cell(cell);
         row
      })
      .collect();

   table.add_rows(rows);

   println!("{table}");
}

pub fn notice(message: impl AsRef<str>, fg_color: Option<Color>, attributes: Vec<Attribute>) {
   let mut table = Table::new();
   table
      .load_preset(UTF8_HORIZONTAL_ONLY)
      .apply_modifier(UTF8_ROUND_CORNERS)
      .set_content_arrangement(Dynamic);

   let mut cell = Cell::new(message.as_ref());

   if let Some(color) = fg_color {
      cell = cell.fg(color);
   }

   if !attributes.is_empty() {
      cell = cell.add_attributes(attributes);
   }

   cell = cell.set_alignment(CellAlignment::Center);

   let mut row = Row::new();
   row.add_cell(cell);

   table.add_row(row);
   println!("{table}");
}

pub fn prep_cell(
   text: impl AsRef<str>,
   color: Option<CellColor>,
   attribute: Option<CellAttr>,
   delimiter: Option<char>,
   alignment: Option<CellAlignment>,
) -> Cell {
   let mut cell = Cell::from(text.as_ref());

   cell = cell.fg(Color::from(color.unwrap_or(CellColor::Reset)));
   cell = cell.add_attribute(Attribute::from(attribute.unwrap_or(CellAttr::NoHidden)));
   cell = cell.set_delimiter(delimiter.unwrap_or(' '));
   cell = cell.set_alignment(alignment.unwrap_or(CellAlignment::Left));

   cell
}

fn fill_table_body(list: &mut [Installed], table: &mut Table, l_color: Color, r_color: Color) {
   list.sort_by(|a, b| a.mod_name.cmp(&b.mod_name));

   for m in list {
      let mut row = Row::new();
      row.add_cell(
         Cell::new(m.mod_name.clone())
            .fg(l_color)
            .set_alignment(CellAlignment::Left),
      );
      row.add_cell(
         Cell::new(m.install_version.clone())
            .fg(r_color)
            .set_alignment(CellAlignment::Left),
      );
      table.add_row(row);
   }
}

pub fn display_installation_results(mods_processed: Vec<Installed>) {
   let (mut successful, mut failed): (Vec<Installed>, Vec<Installed>) =
      mods_processed.into_iter().partition(|m| m.success);

   let mut s_table = Table::new();
   s_table
      .load_preset(UTF8_FULL_CONDENSED)
      .apply_modifier(UTF8_ROUND_CORNERS);
   let mut f_table = s_table.clone();

   if !successful.is_empty() {
      let mut sh_row = Row::new();
      sh_row.add_cell(
         Cell::new("Successfully Installed".to_string())
            .fg(Color::Green)
            .add_attribute(Attribute::Bold)
            .set_alignment(CellAlignment::Center),
      );
      s_table.set_header(sh_row);

      fill_table_body(&mut successful, &mut s_table, Color::Green, Color::Magenta);

      println!("{s_table}");

      display_table(
         vec![command_output(
            "Total mods Installed",
            successful.len().to_string(),
         )],
         None,
      );
   }

   if !failed.is_empty() {
      let mut fh_row = Row::new();
      fh_row.add_cell(
         Cell::new("Failed to Install".to_string())
            .fg(Color::Red)
            .add_attribute(Attribute::Bold)
            .set_alignment(CellAlignment::Center),
      );
      f_table.set_header(fh_row);

      fill_table_body(&mut failed, &mut f_table, Color::Red, Color::Magenta);

      println!("{f_table}");
   }
}

pub fn construct_cell(dt: CellData) -> Cell {
   let mut cell = Cell::new(dt.text);

   if let Some(color) = dt.color {
      cell = cell.fg(color);
   }

   for attr in dt.attributes {
      cell = cell.add_attribute(attr);
   }

   cell
}
pub fn command_output(option: impl AsRef<str>, val: impl AsRef<str>) -> (CellData, CellData) {
   (
      CellData::new(
         option.as_ref().into(),
         Some(Color::Yellow),
         vec![Attribute::Bold],
         None,
      ),
      CellData::new(
         val.as_ref().into(),
         Some(Color::Magenta),
         vec![Attribute::Bold],
         None,
      ),
   )
}

pub fn display_table(row_data: Vec<(CellData, CellData)>, table_style: Option<&str>) {
   let style = table_style.unwrap_or(UTF8_BORDERS_ONLY);
   let mut table = Table::new();
   table
      .load_preset(style)
      .set_content_arrangement(ContentArrangement::Dynamic)
      .apply_modifier(UTF8_ROUND_CORNERS);

   let mut rows: Vec<Row> = Vec::new();

   for (l_col, r_col) in row_data {
      let mut row = Row::new();
      row.add_cell(construct_cell(l_col));
      row.add_cell(construct_cell(r_col));
      rows.push(row);
   }

   table.add_rows(rows);

   println!("{table}");
}

#[derive(Default)]
pub struct CellData {
   pub(crate) text: String,
   pub(crate) attributes: Vec<Attribute>,
   pub(crate) color: Option<Color>,
   pub(crate) alignment: Option<CellAlignment>,
}

impl CellData {
   pub fn new(
      text: String,
      color: Option<Color>,
      attributes: Vec<Attribute>,
      alignment: Option<CellAlignment>,
   ) -> CellData {
      Self {
         text,
         attributes,
         color,
         alignment,
      }
   }

   #[allow(dead_code)]
   pub fn blank() -> CellData {
      Self::new(String::new(), None, vec![], None)
   }
}

pub fn elapsed_footer(start_time: Instant, operation: impl AsRef<str> + std::fmt::Display) {
   let mut table = Table::new();
   table
      .load_preset(UTF8_HORIZONTAL_ONLY)
      .apply_modifier(UTF8_ROUND_CORNERS)
      .set_content_arrangement(Dynamic);

   let elapsed = format!("{:.2}s", start_time.elapsed().as_secs_f64());
   // let out_str = format!("{} {} {}{}", operation.bright_green().bold(),"operation took:".bright_green().bold(), elapsed.bright_purple(), "s".bright_yellow());
   let operation_str = format!("{} {}", operation, "operation completed: ");
   let mut row = Row::new();

   row.add_cell(
      Cell::new(operation_str.as_str())
         .fg(Color::Green)
         .add_attribute(Attribute::Bold),
   );
   row.add_cell(
      Cell::new(elapsed.as_str())
         .fg(Color::Magenta)
         .add_attribute(Attribute::Bold),
   );

   table.add_row(row);

   println!("{table}");
}
