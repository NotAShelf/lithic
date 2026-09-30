//! The mods of an instance: what is installed, turning mods on and off,
//! removing them, and pinning versions. Installing and updating live in
//! [`install`].

pub mod install;
pub mod lock;
pub mod resolve;

use std::{
  collections::{BTreeMap, BTreeSet},
  fmt,
  fs,
  io::ErrorKind,
  path::{Path, PathBuf},
};

pub use install::{Change, Failure, InstallOptions, Reason, Report, Update};
pub use lock::{LockEntry, ModLock};
use serde::{Deserialize, Serialize};

use crate::{
  Lithic,
  error::{Error, IoContext, Kind, Result},
  fsutil,
  instance::Instance,
  modinfo::{self, Format, ModInfo},
  paths::expand_home,
  version,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstalledMod {
  pub info:      ModInfo,
  pub path:      PathBuf,
  pub file_name: String,
  pub format:    Format,
  pub enabled:   bool,
  /// Why the metadata could not be read, if it could not.
  pub error:     Option<String>,
  pub lock:      LockEntry,
}

impl InstalledMod {
  #[must_use]
  pub fn mod_id(&self) -> &str {
    &self.info.mod_id
  }

  #[must_use]
  pub fn display_name(&self) -> &str {
    if self.info.name.trim().is_empty() {
      &self.file_name
    } else {
      &self.info.name
    }
  }
}

/// A mod as the user names it: `carryon`, `carryon@1.14.3`, a
/// `vintagestorymodinstall://` link, or a `ModDB` page URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModRef {
  pub id:      String,
  pub version: Option<String>,
}

impl ModRef {
  /// # Errors
  /// Returns an error if the reference has no id or contains whitespace in its
  /// id.
  pub fn parse(input: &str) -> Result<Self> {
    let s = input.trim();
    let s = s.strip_prefix("vintagestorymodinstall://").unwrap_or(s);
    let s = s
      .strip_prefix("https://mods.vintagestory.at/")
      .or_else(|| s.strip_prefix("http://mods.vintagestory.at/"))
      .map_or(s, |rest| {
        rest
          .trim_start_matches("show/mod/")
          .split(['?', '#', '/'])
          .next()
          .unwrap_or(rest)
      });
    let s = s.trim_end_matches('/');
    let (id, version) = match s.split_once('@') {
      Some((id, v)) => {
        (
          id.trim(),
          Some(v.trim().to_string()).filter(|v| !v.is_empty()),
        )
      },
      None => (s, None),
    };
    if id.is_empty() || id.contains(char::is_whitespace) {
      return Err(Error::invalid(format!("`{input}` is not a mod id")));
    }
    Ok(Self {
      id: id.to_ascii_lowercase(),
      version,
    })
  }
}

impl fmt::Display for ModRef {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match &self.version {
      Some(v) => write!(f, "{}@{v}", self.id),
      None => f.write_str(&self.id),
    }
  }
}

/// Something wrong with an instance's mod set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Problem {
  MissingDependency {
    mod_id:     String,
    dependency: String,
    required:   String,
  },
  OutdatedDependency {
    mod_id:     String,
    dependency: String,
    required:   String,
    installed:  String,
  },
  DisabledDependency {
    mod_id:     String,
    dependency: String,
  },
  Duplicate {
    mod_id: String,
    files:  Vec<String>,
  },
  Unreadable {
    file:  String,
    error: String,
  },
}

/// Holds `<instance>/.operation.lock` for the duration of a change to an
/// instance's mods, so two installs cannot interleave.
pub(crate) struct OperationLock {
  file: fs::File,
}

impl OperationLock {
  pub(crate) fn try_acquire(instance: &Instance) -> Result<Self> {
    let path = instance.dir.join(".operation.lock");
    let file = fs::OpenOptions::new()
      .create(true)
      .truncate(false)
      .write(true)
      .open(&path)
      .at(&path)?;
    match file.try_lock() {
      Ok(()) => Ok(Self { file }),
      Err(fs::TryLockError::WouldBlock) => {
        Err(Error::Busy(instance.name.clone()))
      },
      Err(fs::TryLockError::Error(e)) => Err(Error::io(&path, e)),
    }
  }
}

// See `FileLock`: closing alone can leave the lock held by a forked child.
impl Drop for OperationLock {
  fn drop(&mut self) {
    let _ = self.file.unlock();
  }
}

impl Lithic {
  /// # Errors
  /// Returns an error when the instance's lock file cannot be read.
  pub fn mod_lock(&self, instance: &Instance) -> Result<ModLock> {
    Ok(fsutil::read_json(&instance.lock_file())?.unwrap_or_default())
  }

  pub(crate) fn update_mod_lock<R>(
    instance: &Instance,
    f: impl FnOnce(&mut ModLock) -> Result<R>,
  ) -> Result<R> {
    fsutil::update_json(&instance.lock_file(), |lock: &mut ModLock| {
      let out = f(lock)?;
      lock.mods.retain(|_, entry| !entry.is_empty());
      Ok(out)
    })
  }

  /// Every mod in the instance, enabled or not, sorted by name.
  /// # Errors
  /// Returns an error if the lock file or a mod directory cannot be read.
  pub fn installed_mods(
    &self,
    instance: &Instance,
  ) -> Result<Vec<InstalledMod>> {
    let lock = self.mod_lock(instance)?;
    let mut mods = scan_dir(&instance.mods_dir(), true, &lock)?;
    mods.extend(scan_dir(&instance.disabled_mods_dir(), false, &lock)?);
    mods.sort_by(|a, b| {
      a.display_name()
        .to_lowercase()
        .cmp(&b.display_name().to_lowercase())
        .then_with(|| a.file_name.cmp(&b.file_name))
    });
    Ok(mods)
  }

  /// Moves a mod into or out of the disabled folder.
  /// # Errors
  /// Returns an error when the mod is absent or a file operation fails.
  pub fn set_mod_enabled(
    &self,
    instance: &Instance,
    mod_id: &str,
    enabled: bool,
  ) -> Result<()> {
    let _op = OperationLock::try_acquire(instance)?;
    let mods = self.installed_mods(instance)?;
    let targets: Vec<&InstalledMod> = mods
      .iter()
      .filter(|m| {
        m.mod_id().eq_ignore_ascii_case(mod_id) || m.file_name == mod_id
      })
      .collect();
    if targets.is_empty() {
      return Err(Error::not_found(Kind::Mod, mod_id));
    }
    let dest_dir = if enabled {
      instance.mods_dir()
    } else {
      instance.disabled_mods_dir()
    };
    for m in targets.into_iter().filter(|m| m.enabled != enabled) {
      let dest = free_path(&dest_dir, &m.file_name);
      fsutil::move_path(&m.path, &dest)?;
    }
    Ok(())
  }

  /// Pins a mod to a version (`Some`) or releases the pin (`None`). A pin can
  /// be set for a mod that is not installed yet.
  /// # Errors
  /// Returns an error if the lock file cannot be updated.
  pub fn set_mod_pin(
    &self,
    instance: &Instance,
    mod_id: &str,
    pin: Option<&str>,
  ) -> Result<()> {
    let key = mod_id.to_ascii_lowercase();
    Self::update_mod_lock(instance, |lock| {
      lock.mods.entry(key).or_default().pin = pin.map(ToString::to_string);
      Ok(())
    })
  }

  /// Removes mods by id (or file name). With `orphans`, dependencies that
  /// only the removed mods needed go too. Returns what was removed.
  /// # Errors
  /// Returns an error if a mod is absent, cannot be moved or backed up, or the
  /// lock cannot be updated.
  pub fn remove_mods(
    &self,
    instance: &Instance,
    ids: &[String],
    orphans: bool,
  ) -> Result<Vec<InstalledMod>> {
    let _op = OperationLock::try_acquire(instance)?;
    let settings = self.settings()?;
    let mods = self.installed_mods(instance)?;

    let wanted: BTreeSet<String> =
      ids.iter().map(|i| i.to_ascii_lowercase()).collect();
    let mut removing: Vec<&InstalledMod> = mods
      .iter()
      .filter(|m| {
        wanted.contains(m.mod_id())
          || wanted.contains(&m.file_name.to_ascii_lowercase())
      })
      .collect();
    for id in &wanted {
      if !removing
        .iter()
        .any(|m| m.mod_id() == id || m.file_name.eq_ignore_ascii_case(id))
      {
        return Err(Error::not_found(Kind::Mod, id.clone()));
      }
    }

    if orphans {
      loop {
        let gone: BTreeSet<&str> =
          removing.iter().map(|m| m.mod_id()).collect();
        let still_needed: BTreeSet<&str> = mods
          .iter()
          .filter(|m| !gone.contains(m.mod_id()))
          .flat_map(|m| m.info.mod_dependencies().map(|(d, _)| d.as_str()))
          .collect();
        let extra: Vec<&InstalledMod> = mods
          .iter()
          .filter(|m| {
            m.lock.dependency
              && !gone.contains(m.mod_id())
              && !still_needed.contains(m.mod_id())
              && removing
                .iter()
                .any(|r| r.info.dependencies.contains_key(m.mod_id()))
          })
          .collect();
        if extra.is_empty() {
          break;
        }
        removing.extend(extra);
      }
    }

    // The lock is updated after each file so it never describes a mod that
    // is already gone, even if a later removal fails.
    for m in &removing {
      if settings.backups.enabled {
        self.backup_mod(instance, m)?;
      }
      fsutil::remove_path(&m.path)?;
      Self::update_mod_lock(instance, |lock| {
        if let Some(entry) = lock.mods.get_mut(m.mod_id()) {
          *entry = LockEntry {
            pin: entry.pin.take(),
            ..LockEntry::default()
          };
        }
        Ok(())
      })?;
    }
    Ok(removing.into_iter().cloned().collect())
  }

  /// Installed mods that depend on `mod_id`.
  /// # Errors
  /// Returns an error if the installed mods cannot be read.
  pub fn dependents(
    &self,
    instance: &Instance,
    mod_id: &str,
  ) -> Result<Vec<InstalledMod>> {
    let key = mod_id.to_ascii_lowercase();
    Ok(
      self
        .installed_mods(instance)?
        .into_iter()
        .filter(|m| m.info.dependencies.contains_key(&key))
        .collect(),
    )
  }

  /// Copies a mod's file aside before it is replaced or removed, keeping the
  /// newest `backups.keep` copies per mod.
  pub(crate) fn backup_mod(
    &self,
    instance: &Instance,
    m: &InstalledMod,
  ) -> Result<()> {
    let settings = self.settings()?;
    let root = settings
      .backups
      .dir
      .map_or_else(|| self.paths.backups_dir(), expand_home);
    let dir = root
      .join(&instance.id)
      .join(fsutil::sanitize_file_name(m.mod_id()));
    fs::create_dir_all(&dir).at(&dir)?;
    let stamp = fsutil::now_ms();
    let dest = dir.join(format!("{stamp}-{}", m.file_name));
    if m.path.is_dir() {
      fsutil::copy_dir(&m.path, &dest, &|_| false)?;
    } else {
      fs::copy(&m.path, &dest).at(&m.path)?;
    }

    let mut existing: Vec<PathBuf> = fs::read_dir(&dir)
      .at(&dir)?
      .flatten()
      .map(|e| e.path())
      .collect();
    existing.sort();
    let keep = settings.backups.keep.max(1);
    if existing.len() > keep {
      for old in &existing[..existing.len() - keep] {
        fsutil::remove_path(old)?;
      }
    }
    Ok(())
  }
}

/// Finds problems that would stop the game from loading a mod set.
pub fn problems(mods: &[InstalledMod]) -> Vec<Problem> {
  let mut out = Vec::new();

  let mut by_id: BTreeMap<&str, Vec<&InstalledMod>> = BTreeMap::new();
  for m in mods {
    by_id.entry(m.mod_id()).or_default().push(m);
  }

  for m in mods {
    if let Some(error) = &m.error {
      out.push(Problem::Unreadable {
        file:  m.file_name.clone(),
        error: error.clone(),
      });
    }
  }

  for (id, copies) in &by_id {
    let enabled: Vec<&&InstalledMod> =
      copies.iter().filter(|m| m.enabled).collect();
    if enabled.len() > 1 {
      out.push(Problem::Duplicate {
        mod_id: (*id).to_string(),
        files:  enabled.iter().map(|m| m.file_name.clone()).collect(),
      });
    }
  }

  for m in mods.iter().filter(|m| m.enabled) {
    for (dep, required) in m.info.mod_dependencies() {
      let candidates = by_id
        .get(dep.as_str())
        .map(Vec::as_slice)
        .unwrap_or_default();
      let enabled = candidates.iter().find(|c| c.enabled);
      match (enabled, candidates.is_empty()) {
        (Some(found), _) => {
          if found.info.has_metadata
            && !version::satisfies(&found.info.version, required)
          {
            out.push(Problem::OutdatedDependency {
              mod_id:     m.mod_id().to_string(),
              dependency: dep.clone(),
              required:   required.clone(),
              installed:  found.info.version.clone(),
            });
          }
        },
        (None, false) => {
          out.push(Problem::DisabledDependency {
            mod_id:     m.mod_id().to_string(),
            dependency: dep.clone(),
          })
        },
        (None, true) => {
          out.push(Problem::MissingDependency {
            mod_id:     m.mod_id().to_string(),
            dependency: dep.clone(),
            required:   required.clone(),
          })
        },
      }
    }
  }
  out
}

fn scan_dir(
  dir: &Path,
  enabled: bool,
  lock: &ModLock,
) -> Result<Vec<InstalledMod>> {
  let entries = match fs::read_dir(dir) {
    Ok(entries) => entries,
    Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
    Err(e) => return Err(Error::io(dir, e)),
  };
  let mut out = Vec::new();
  for entry in entries {
    let entry = entry.at(dir)?;
    let path = entry.path();
    let file_name = entry.file_name().to_string_lossy().into_owned();
    if file_name.starts_with('.') {
      continue;
    }
    let Some(format) = Format::of(&path) else {
      continue;
    };
    let (info, error) = match modinfo::read(&path) {
      Ok(info) => (info, None),
      Err(e) => {
        (
          ModInfo {
            mod_id: modinfo::mod_id_from_name(
              path
                .file_stem()
                .map(|s| s.to_string_lossy())
                .unwrap_or_default()
                .as_ref(),
            ),
            name: file_name.clone(),
            ..ModInfo::default()
          },
          Some(e.to_string()),
        )
      },
    };
    let lock_entry = lock.mods.get(&info.mod_id).cloned().unwrap_or_default();
    out.push(InstalledMod {
      info,
      path,
      file_name,
      format,
      enabled,
      error,
      lock: lock_entry,
    });
  }
  Ok(out)
}

/// `dir/name`, or `dir/name (2)` and so on if that is taken.
pub(crate) fn free_path(dir: &Path, name: &str) -> PathBuf {
  let candidate = dir.join(name);
  if !candidate.exists() {
    return candidate;
  }
  let (stem, ext) = match name.rsplit_once('.') {
    Some((s, e)) if !s.is_empty() => (s, format!(".{e}")),
    _ => (name, String::new()),
  };
  (2..=i32::MAX)
    .map(|n| dir.join(format!("{stem} ({n}){ext}")))
    .find(|p| !p.exists())
    .unwrap_or(candidate)
}

#[cfg(test)]
#[expect(
  clippy::unwrap_used,
  reason = "test setup and assertions intentionally fail on error"
)]
pub(crate) mod tests {
  use std::{io::Write, thread, time::Duration};

  use zip::write::SimpleFileOptions;

  use super::*;
  use crate::{Paths, instance::NewInstance};

  pub fn write_mod_zip(dir: &Path, file: &str, modinfo: &str) -> PathBuf {
    fs::create_dir_all(dir).unwrap();
    let path = dir.join(file);
    let mut w = zip::ZipWriter::new(fs::File::create(&path).unwrap());
    w.start_file("modinfo.json", SimpleFileOptions::default())
      .unwrap();
    w.write_all(modinfo.as_bytes()).unwrap();
    w.finish().unwrap();
    path
  }

  pub fn setup() -> (tempfile::TempDir, Lithic, Instance) {
    let dir = tempfile::tempdir().unwrap();
    let lithic = Lithic::new(Paths::rooted(dir.path())).unwrap();
    let instance = lithic
      .create_instance(NewInstance {
        name: "test".into(),
        game_version: Some("1.21.5".into()),
        ..Default::default()
      })
      .unwrap();
    (dir, lithic, instance)
  }

  #[test]
  fn mod_ref_forms() {
    let r = ModRef::parse("CarryOn@1.14.3").unwrap();
    assert_eq!(
      (r.id.as_str(), r.version.as_deref()),
      ("carryon", Some("1.14.3"))
    );
    assert_eq!(
      ModRef::parse("vintagestorymodinstall://carryon@1.0")
        .unwrap()
        .version
        .as_deref(),
      Some("1.0")
    );
    assert_eq!(
      ModRef::parse("https://mods.vintagestory.at/carryon")
        .unwrap()
        .id,
      "carryon"
    );
    assert_eq!(
      ModRef::parse("https://mods.vintagestory.at/show/mod/890#tab")
        .unwrap()
        .id,
      "890"
    );
    assert!(ModRef::parse("  ").is_err());
    assert!(ModRef::parse("two words").is_err());
  }

  #[test]
  fn scan_reads_enabled_and_disabled() {
    let (_d, l, i) = setup();
    write_mod_zip(
      &i.mods_dir(),
      "a.zip",
      r#"{"modid":"alpha","name":"Alpha","version":"1.0.0"}"#,
    );
    write_mod_zip(
      &i.disabled_mods_dir(),
      "b.zip",
      r#"{"modid":"beta","name":"Beta","version":"2.0.0"}"#,
    );
    fs::write(i.mods_dir().join("broken.zip"), "not a zip").unwrap();
    fs::write(i.mods_dir().join("notes.txt"), "ignored").unwrap();
    let mods = l.installed_mods(&i).unwrap();
    assert_eq!(mods.len(), 3);
    let beta = mods.iter().find(|m| m.mod_id() == "beta").unwrap();
    assert!(!beta.enabled);
    let broken = mods.iter().find(|m| m.file_name == "broken.zip").unwrap();
    assert!(broken.error.is_some());
  }

  #[test]
  fn enable_disable_moves_files() {
    let (_d, l, i) = setup();
    write_mod_zip(
      &i.mods_dir(),
      "a.zip",
      r#"{"modid":"alpha","version":"1.0.0"}"#,
    );
    l.set_mod_enabled(&i, "alpha", false).unwrap();
    assert!(i.disabled_mods_dir().join("a.zip").is_file());
    assert!(!i.mods_dir().join("a.zip").exists());
    l.set_mod_enabled(&i, "ALPHA", true).unwrap();
    assert!(i.mods_dir().join("a.zip").is_file());
    assert!(matches!(
      l.set_mod_enabled(&i, "nope", true),
      Err(Error::NotFound { .. })
    ));
  }

  #[test]
  fn remove_with_orphans_and_pins_survive() {
    let (_d, l, i) = setup();
    write_mod_zip(
      &i.mods_dir(),
      "a.zip",
      r#"{"modid":"app","version":"1.0.0","dependencies":{"lib":"1.0.0","game":""}}"#,
    );
    write_mod_zip(
      &i.mods_dir(),
      "l.zip",
      r#"{"modid":"lib","version":"1.0.0"}"#,
    );
    write_mod_zip(
      &i.mods_dir(),
      "o.zip",
      r#"{"modid":"other","version":"1.0.0","dependencies":{"shared":""}}"#,
    );
    write_mod_zip(
      &i.mods_dir(),
      "s.zip",
      r#"{"modid":"shared","version":"1.0.0"}"#,
    );
    Lithic::update_mod_lock(&i, |lock| {
      lock.mods.insert("lib".into(), LockEntry {
        dependency: true,
        file: Some("l.zip".into()),
        ..Default::default()
      });
      lock.mods.insert("shared".into(), LockEntry {
        dependency: true,
        file: Some("s.zip".into()),
        ..Default::default()
      });
      lock.mods.insert("app".into(), LockEntry {
        pin: Some("1.0.0".into()),
        file: Some("a.zip".into()),
        ..Default::default()
      });
      Ok(())
    })
    .unwrap();

    let removed = l.remove_mods(&i, &["app".into()], true).unwrap();
    let ids: BTreeSet<&str> =
      removed.iter().map(InstalledMod::mod_id).collect();
    assert_eq!(ids, BTreeSet::from(["app", "lib"]));
    assert!(i.mods_dir().join("s.zip").is_file());

    let lock = l.mod_lock(&i).unwrap();
    assert_eq!(
      lock.mods.get("app").and_then(|e| e.pin.as_deref()),
      Some("1.0.0")
    );
    assert!(!lock.mods.contains_key("lib"));
  }

  #[test]
  fn problem_detection() {
    let (_d, l, i) = setup();
    write_mod_zip(
      &i.mods_dir(),
      "a.zip",
      r#"{"modid":"app","version":"1.0.0","dependencies":{"lib":"2.0.0","gone":"*","off":""}}"#,
    );
    write_mod_zip(
      &i.mods_dir(),
      "l.zip",
      r#"{"modid":"lib","version":"1.0.0"}"#,
    );
    write_mod_zip(
      &i.mods_dir(),
      "l2.zip",
      r#"{"modid":"lib","version":"1.0.0"}"#,
    );
    write_mod_zip(
      &i.disabled_mods_dir(),
      "off.zip",
      r#"{"modid":"off","version":"1.0.0"}"#,
    );
    let found = problems(&l.installed_mods(&i).unwrap());
    assert!(
         found
            .iter()
            .any(|p| matches!(p, Problem::MissingDependency { dependency, .. } if dependency == "gone"))
      );
    assert!(
         found
            .iter()
            .any(|p| matches!(p, Problem::OutdatedDependency { dependency, .. } if dependency == "lib"))
      );
    assert!(
         found
            .iter()
            .any(|p| matches!(p, Problem::DisabledDependency { dependency, .. } if dependency == "off"))
      );
    assert!(found.iter().any(
      |p| matches!(p, Problem::Duplicate { mod_id, .. } if mod_id == "lib")
    ));
  }

  #[test]
  fn backups_are_pruned() {
    let (_d, l, i) = setup();
    l.update_settings(|s| {
      s.backups.enabled = true;
      s.backups.keep = 2;
    })
    .unwrap();
    let path = write_mod_zip(
      &i.mods_dir(),
      "a.zip",
      r#"{"modid":"app","version":"1.0.0"}"#,
    );
    let m = l.installed_mods(&i).unwrap().remove(0);
    for _ in 0..4 {
      l.backup_mod(&i, &m).unwrap();
      thread::sleep(Duration::from_millis(3));
    }
    let dir = l.paths.backups_dir().join(&i.id).join("app");
    assert_eq!(fs::read_dir(dir).unwrap().count(), 2);
    assert!(path.is_file());
  }

  #[test]
  fn free_path_avoids_collisions() {
    let d = tempfile::tempdir().unwrap();
    fs::write(d.path().join("m.zip"), "").unwrap();
    assert_eq!(free_path(d.path(), "m.zip"), d.path().join("m (2).zip"));
    assert_eq!(free_path(d.path(), "n.zip"), d.path().join("n.zip"));
  }
}
