//! Sharing an instance as a modpack.
//!
//! A pack is a zip archive:
//!
//! ```text
//! lithic-pack.json     the manifest, see [`Manifest`]
//! mods/<file>          mods that cannot be fetched from the ModDB
//! overrides/<path>     files copied into the data directory (ModConfig, ...)
//! ```
//!
//! Packs written by lithic 1.x (a `modinfo.json` whose dependencies list the
//! mods) can be imported as well.

use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tokio::task::spawn_blocking;
use zip::result::ZipError;
use zip::write::SimpleFileOptions;

use crate::error::{Error, IoContext, Result};
use crate::instance::{Instance, NewInstance};
use crate::mods::{InstallOptions, ModRef, Report};
use crate::{Lithic, modinfo, version};

pub const MANIFEST_FILE: &str = "lithic-pack.json";
pub const FORMAT: u32 = 1;

/// Folders in the data directory that hold mod settings and may be shipped
/// with a pack.
pub const CONFIG_DIRS: [&str; 1] = ["ModConfig"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
   pub format: u32,
   pub name: String,
   #[serde(default, skip_serializing_if = "Option::is_none")]
   pub description: Option<String>,
   #[serde(default, skip_serializing_if = "Option::is_none")]
   pub game_version: Option<String>,
   pub mods: Vec<PackMod>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackMod {
   pub id: String,
   pub version: String,
   /// Set for mods fetched from the `ModDB` on import.
   #[serde(default, skip_serializing_if = "Option::is_none")]
   pub moddb_id: Option<i64>,
   /// Set for mods shipped inside the pack under `mods/`.
   #[serde(default, skip_serializing_if = "Option::is_none")]
   pub file: Option<String>,
   #[serde(default = "yes", skip_serializing_if = "is_true")]
   pub enabled: bool,
}

const fn yes() -> bool {
   true
}

#[expect(
   clippy::trivially_copy_pass_by_ref,
   reason = "serde skip_serializing_if requires a borrowed field"
)]
const fn is_true(b: &bool) -> bool {
   *b
}

#[derive(Debug, Clone, Default)]
pub struct ExportOptions {
   pub description: Option<String>,
   /// Ship mod settings from the data directory.
   pub include_config: bool,
   /// Ship every mod file, even those on the `ModDB`. Makes the pack larger but
   /// installable offline.
   pub bundle_all: bool,
}

#[derive(Debug)]
pub struct ImportResult {
   pub instance: Instance,
   pub install: Report,
   pub bundled: usize,
}

impl Lithic {
   /// Writes `instance` as a pack to `dest`. Checks the `ModDB` for each mod to
   /// decide whether to reference it or ship its file.
   /// # Errors
   /// Returns an error if mods cannot be read, a lookup fails, or the pack cannot be written.
   pub async fn export_pack(
      &self,
      instance: &Instance,
      dest: &Path,
      opts: &ExportOptions,
   ) -> Result<Manifest> {
      let installed = self.installed_mods(instance)?;
      let mut entries: Vec<(PackMod, Option<PathBuf>)> = Vec::new();

      for m in &installed {
         let on_moddb = if opts.bundle_all || !m.info.has_metadata {
            None
         } else {
            let lookup = m
               .lock
               .moddb_id
               .map_or_else(|| m.mod_id().to_string(), |id| id.to_string());
            match self.moddb.mod_details(&lookup).await {
               Ok(details) if details.release(&m.info.version).is_some() => Some(details.id),
               Ok(_) | Err(Error::NotFound { .. }) => None,
               Err(e) => return Err(e),
            }
         };
         let entry = PackMod {
            id: m.mod_id().to_string(),
            version: m.info.version.clone(),
            moddb_id: on_moddb,
            file: on_moddb.is_none().then(|| m.file_name.clone()),
            enabled: m.enabled,
         };
         entries.push((entry, on_moddb.is_none().then(|| m.path.clone())));
      }

      let manifest = Manifest {
         format: FORMAT,
         name: instance.name.clone(),
         description: opts.description.clone(),
         game_version: instance.game_version.clone(),
         mods: entries.iter().map(|(e, _)| e.clone()).collect(),
      };

      let data_dir = instance.data_dir();
      let include_config = opts.include_config;
      let dest = dest.to_path_buf();
      let manifest_out = manifest.clone();
      spawn_blocking(move || {
         write_pack(
            &dest,
            &manifest_out,
            &entries,
            include_config.then_some(&data_dir),
         )
      })
      .await
      .map_err(|e| Error::invalid(format!("writing the pack failed: {e}")))??;
      Ok(manifest)
   }

   /// Reads a pack's manifest without importing it.
   /// # Errors
   /// Returns an error if the archive or manifest is invalid, unreadable, or unsupported.
   pub fn read_pack(&self, path: &Path) -> Result<Manifest> {
      let mut archive = open_zip(path)?;
      read_manifest(&mut archive, path)
   }

   /// Creates a new instance from a pack. `name` overrides the pack's name.
   /// # Errors
   /// Returns an error if the pack cannot be read, the instance cannot be created, or installation fails.
   pub async fn import_pack(
      &self,
      path: &Path,
      name: Option<&str>,
      opts: &InstallOptions,
   ) -> Result<ImportResult> {
      let manifest = self.read_pack(path)?;
      let instance = self.create_instance(NewInstance {
         name: name.map_or_else(|| manifest.name.clone(), ToString::to_string),
         game_version: manifest.game_version.clone(),
         ..NewInstance::default()
      })?;

      let result = async {
         let archive_path = path.to_path_buf();
         let mods_dir = instance.mods_dir();
         let disabled_dir = instance.disabled_mods_dir();
         let data_dir = instance.data_dir();
         let bundled_mods: BTreeMap<String, bool> = manifest
            .mods
            .iter()
            .filter_map(|m| Some((m.file.clone()?, m.enabled)))
            .collect();
         let bundled = spawn_blocking(move || {
            unpack_files(&archive_path, &mods_dir, &disabled_dir, &data_dir, &bundled_mods)
         })
         .await
         .map_err(|e| Error::invalid(format!("reading the pack failed: {e}")))??;

         let refs: Vec<ModRef> = manifest
            .mods
            .iter()
            .filter(|m| m.file.is_none())
            .map(|m| ModRef {
               id: m.moddb_id.map_or_else(|| m.id.clone(), |id| id.to_string()),
               version: Some(m.version.clone()).filter(|v| !v.is_empty()),
            })
            .collect();
         let install = if refs.is_empty() {
            Report::default()
         } else {
            let opts = InstallOptions {
               pin_versions: false,
               dependencies: true,
               ..opts.clone()
            };
            self.install_mods(&instance, &refs, &opts).await?
         };

         for m in manifest.mods.iter().filter(|m| !m.enabled && m.file.is_none()) {
            let _ = self.set_mod_enabled(&instance, &m.id, false);
         }
         Ok::<_, Error>((install, bundled))
      }
      .await;

      match result {
         Ok((install, bundled)) => Ok(ImportResult {
            instance: self.instance(&instance.id)?,
            install,
            bundled,
         }),
         Err(e) => {
            let _ = self.delete_instance(&instance.id);
            Err(e)
         }
      }
   }
}

fn open_zip(path: &Path) -> Result<zip::ZipArchive<fs::File>> {
   let file = fs::File::open(path).at(path)?;
   zip::ZipArchive::new(file).map_err(|e| Error::parse(path.display().to_string(), e))
}

fn read_entry(archive: &mut zip::ZipArchive<fs::File>, name: &str, path: &Path) -> Result<Option<String>> {
   let index = (0..archive.len()).find(|&i| {
      archive
         .name_for_index(i)
         .is_some_and(|n| n.eq_ignore_ascii_case(name))
   });
   let Some(index) = index else { return Ok(None) };
   let mut entry = archive
      .by_index(index)
      .map_err(|e| Error::parse(path.display().to_string(), e))?;
   let mut bytes = Vec::new();
   entry.read_to_end(&mut bytes).at(path)?;
   Ok(Some(String::from_utf8_lossy(&bytes).into_owned()))
}

fn read_manifest(archive: &mut zip::ZipArchive<fs::File>, path: &Path) -> Result<Manifest> {
   if let Some(text) = read_entry(archive, MANIFEST_FILE, path)? {
      let manifest: Manifest = serde_json::from_str(&text).map_err(|e| Error::parse(MANIFEST_FILE, e))?;
      if manifest.format > FORMAT {
         return Err(Error::Unsupported(format!(
            "this pack needs a newer lithic (pack format {}, supported {FORMAT})",
            manifest.format
         )));
      }
      return Ok(manifest);
   }

   // lithic 1.x packs: a modinfo.json whose dependencies are the pack's mods.
   if let Some(text) = read_entry(archive, modinfo::MODINFO_FILE, path)? {
      let info = modinfo::parse_json(&text).map_err(|e| Error::parse(modinfo::MODINFO_FILE, e))?;
      let game_version = info
         .dependencies
         .get("game")
         .filter(|v| version::parse(v).is_some())
         .cloned();
      return Ok(Manifest {
         format: FORMAT,
         name: if info.name.is_empty() {
            info.mod_id.clone()
         } else {
            info.name.clone()
         },
         description: info.description.clone(),
         game_version,
         mods: info
            .mod_dependencies()
            .map(|(id, v)| PackMod {
               id: id.clone(),
               version: if v == "*" { String::new() } else { v.clone() },
               moddb_id: None,
               file: None,
               enabled: true,
            })
            .collect(),
      });
   }
   Err(Error::invalid(format!("{} is not a lithic pack", path.display())))
}

fn write_pack(
   dest: &Path,
   manifest: &Manifest,
   entries: &[(PackMod, Option<PathBuf>)],
   config_from: Option<&PathBuf>,
) -> Result<()> {
   let mut part_name = dest.file_name().unwrap_or_default().to_os_string();
   part_name.push(".part");
   let part = dest.with_file_name(part_name);
   if let Some(parent) = dest.parent() {
      fs::create_dir_all(parent).at(parent)?;
   }

   let result = (|| {
      let mut zip = zip::ZipWriter::new(fs::File::create(&part).at(&part)?);
      let options = SimpleFileOptions::default();
      let zip_err = |e: ZipError| Error::invalid(format!("writing {}: {e}", dest.display()));

      let json = serde_json::to_vec_pretty(manifest).map_err(|e| Error::parse(MANIFEST_FILE, e))?;
      zip.start_file(MANIFEST_FILE, options).map_err(zip_err)?;
      zip.write_all(&json).at(&part)?;

      for (entry, file) in entries {
         let (Some(name), Some(file)) = (&entry.file, file) else {
            continue;
         };
         add_path(&mut zip, file, &format!("mods/{name}"), options)?;
      }
      if let Some(data) = config_from {
         for dir in CONFIG_DIRS {
            let from = data.join(dir);
            if from.is_dir() {
               add_path(&mut zip, &from, &format!("overrides/{dir}"), options)?;
            }
         }
      }
      zip.finish().map_err(zip_err)?;
      Ok(())
   })();

   match result {
      Ok(()) => fs::rename(&part, dest).at(dest),
      Err(e) => {
         let _ = fs::remove_file(&part);
         Err(e)
      }
   }
}

fn add_path(
   zip: &mut zip::ZipWriter<fs::File>,
   path: &Path,
   name: &str,
   options: SimpleFileOptions,
) -> Result<()> {
   let zip_err = |e: ZipError| Error::invalid(format!("adding {name}: {e}"));
   if path.is_dir() {
      for entry in fs::read_dir(path).at(path)? {
         let entry = entry.at(path)?;
         let child = format!("{name}/{}", entry.file_name().to_string_lossy());
         add_path(zip, &entry.path(), &child, options)?;
      }
      return Ok(());
   }
   zip.start_file(name, options).map_err(zip_err)?;
   let mut file = fs::File::open(path).at(path)?;
   io::copy(&mut file, zip).at(path)?;
   Ok(())
}

/// Extracts bundled mods and overrides. Entry names are checked so nothing
/// can be written outside the target directories.
fn unpack_files(
   archive_path: &Path,
   mods_dir: &Path,
   disabled_dir: &Path,
   data_dir: &Path,
   bundled: &BTreeMap<String, bool>,
) -> Result<usize> {
   let mut archive = open_zip(archive_path)?;
   let mut count = 0;
   let mut folder_mods: BTreeMap<String, bool> = BTreeMap::new();
   for i in 0..archive.len() {
      let mut entry = archive
         .by_index(i)
         .map_err(|e| Error::parse(archive_path.display().to_string(), e))?;
      let Some(rel) = entry.enclosed_name() else {
         continue;
      };
      if entry.is_dir() {
         continue;
      }
      let mut parts = rel.components();
      let top = parts.next().map(|c| c.as_os_str().to_string_lossy().into_owned());
      let rest: PathBuf = parts.collect();
      let target = match top.as_deref() {
         Some("mods") => {
            let first = rest
               .components()
               .next()
               .map(|c| c.as_os_str().to_string_lossy().into_owned())
               .unwrap_or_default();
            let Some(&enabled) = bundled.get(&first) else {
               continue;
            };
            if rest.components().count() == 1 {
               count += 1;
            } else {
               folder_mods.insert(first, enabled);
            }
            if enabled {
               mods_dir.join(&rest)
            } else {
               disabled_dir.join(&rest)
            }
         }
         Some("overrides") if !rest.as_os_str().is_empty() => data_dir.join(&rest),
         _ => continue,
      };
      if let Some(parent) = target.parent() {
         fs::create_dir_all(parent).at(parent)?;
      }
      let mut out = fs::File::create(&target).at(&target)?;
      io::copy(&mut entry, &mut out).at(&target)?;
   }
   Ok(count + folder_mods.len())
}

#[cfg(test)]
#[expect(
   clippy::unwrap_used,
   reason = "test setup and assertions intentionally fail on error"
)]
mod tests {
   use super::*;
   use crate::Paths;
   use crate::mods::tests::{setup, write_mod_zip};
   use tokio::runtime::Runtime;

   #[test]
   fn pack_round_trip_offline() {
      let (d, l, i) = setup();
      write_mod_zip(
         &i.mods_dir(),
         "local.zip",
         r#"{"modid":"localmod","version":"0.1.0"}"#,
      );
      write_mod_zip(
         &i.disabled_mods_dir(),
         "off.zip",
         r#"{"modid":"offmod","version":"1.0.0"}"#,
      );
      fs::create_dir_all(i.data_dir().join("ModConfig")).unwrap();
      fs::write(i.data_dir().join("ModConfig/settings.json"), "{}").unwrap();
      fs::create_dir_all(i.data_dir().join("Saves")).unwrap();

      let dest = d.path().join("out/pack.zip");
      let rt = Runtime::new().unwrap();
      let manifest = rt
         .block_on(l.export_pack(
            &i,
            &dest,
            &ExportOptions {
               include_config: true,
               bundle_all: true,
               ..Default::default()
            },
         ))
         .unwrap();
      assert_eq!(manifest.mods.len(), 2);
      assert_eq!(l.read_pack(&dest).unwrap(), manifest);

      let imported = rt
         .block_on(l.import_pack(&dest, Some("Imported"), &InstallOptions::default()))
         .unwrap();
      let ni = imported.instance;
      assert_eq!(ni.game_version.as_deref(), Some("1.21.5"));
      assert_eq!(imported.bundled, 2);
      assert!(ni.mods_dir().join("local.zip").is_file());
      assert!(ni.disabled_mods_dir().join("off.zip").is_file());
      assert!(ni.data_dir().join("ModConfig/settings.json").is_file());
      assert!(!ni.data_dir().join("Saves").exists());
   }

   #[test]
   fn legacy_pack_manifest() {
      let d = tempfile::tempdir().unwrap();
      let path = write_mod_zip(
         d.path(),
         "old.zip",
         r#"{"name":"My Pack","modid":"mypack","type":"content","dependencies":{"game":"1.19.8","carryon":"1.6.0","anything":"*"}}"#,
      );
      let l = Lithic::new(Paths::rooted(d.path())).unwrap();
      let m = l.read_pack(&path).unwrap();
      assert_eq!(m.name, "My Pack");
      assert_eq!(m.game_version.as_deref(), Some("1.19.8"));
      let ids: Vec<(&str, &str)> = m
         .mods
         .iter()
         .map(|m| (m.id.as_str(), m.version.as_str()))
         .collect();
      assert_eq!(ids, [("anything", ""), ("carryon", "1.6.0")]);
   }

   #[test]
   fn hostile_entry_names_are_ignored() {
      let d = tempfile::tempdir().unwrap();
      let path = d.path().join("evil.zip");
      let mut w = zip::ZipWriter::new(fs::File::create(&path).unwrap());
      let o = SimpleFileOptions::default();
      w.start_file(MANIFEST_FILE, o).unwrap();
      w.write_all(br#"{"format":1,"name":"x","mods":[{"id":"a","version":"1","file":"a.zip"}]}"#)
         .unwrap();
      w.start_file("overrides/../../escape.txt", o).unwrap();
      w.write_all(b"x").unwrap();
      w.start_file("mods/a.zip", o).unwrap();
      w.write_all(b"zip").unwrap();
      w.finish().unwrap();

      let target = d.path().join("inst");
      let bundled = BTreeMap::from([("a.zip".to_string(), true)]);
      let n = unpack_files(
         &path,
         &target.join("Mods"),
         &target.join("off"),
         &target,
         &bundled,
      )
      .unwrap();
      assert_eq!(n, 1);
      assert!(target.join("Mods/a.zip").is_file());
      assert!(!d.path().join("escape.txt").exists());
   }
}
