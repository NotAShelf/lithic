//! Instances: a game version, a data directory the game runs against, and the
//! mods inside it.
//!
//! Layout of `<data>/instances/<id>/`:
//!
//! ```text
//! instance.toml     settings, see [`Instance`]
//! data/             passed to the game as --dataPath (unless data_dir is set)
//! data/Mods/        mods the game loads
//! disabled-mods/    mods switched off in lithic
//! mods.json         what lithic installed, pins; see crate::mods::lock
//! logs/             output of each launch
//! ```

use std::{
  collections::BTreeMap,
  fs,
  io::ErrorKind,
  path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::{
  Lithic,
  error::{Error, IoContext, Kind, Result},
  fsutil::{self, now_ms},
  paths::expand_home,
};

pub const INSTANCE_FILE: &str = "instance.toml";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Instance {
  /// Directory name under `instances/`. Not stored in the file.
  #[serde(skip)]
  pub id:  String,
  /// The instance directory. Not stored in the file.
  #[serde(skip)]
  pub dir: PathBuf,

  pub name:         String,
  /// Game version this instance plays, such as `1.21.5`. Mods are chosen to
  /// match it and it selects the game build used for launching.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub game_version: Option<String>,
  /// A data directory outside the instance, for example the stock launcher's
  /// `VintagestoryData`. Lithic never deletes it.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub data_dir:     Option<PathBuf>,
  /// An extra mod folder outside the data directory, passed to the game with
  /// `--addModPath`. When set, lithic manages mods there instead of in
  /// `<data>/Mods`.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub mods_dir:     Option<PathBuf>,
  /// Account uid to launch with. Falls back to the active account.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub account:      Option<String>,
  #[serde(default)]
  pub launch:       LaunchOptions,
  #[serde(default)]
  pub stats:        Stats,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LaunchOptions {
  /// Extra arguments for the game, one per entry.
  pub args:    Vec<String>,
  pub env:     BTreeMap<String, String>,
  /// Program the game is started through, such as `gamemoderun` or
  /// `prime-run`, with its own arguments.
  pub wrapper: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Stats {
  pub created_at:     i64,
  pub last_played_at: Option<i64>,
  pub play_time_ms:   i64,
}

impl Instance {
  #[must_use]
  pub fn data_dir(&self) -> PathBuf {
    self
      .data_dir
      .clone()
      .unwrap_or_else(|| self.dir.join("data"))
  }

  /// Where the game looks for this instance's mods and where lithic installs
  /// them.
  #[must_use]
  pub fn mods_dir(&self) -> PathBuf {
    self
      .mods_dir
      .clone()
      .unwrap_or_else(|| self.data_dir().join("Mods"))
  }

  #[must_use]
  pub fn disabled_mods_dir(&self) -> PathBuf {
    self.dir.join("disabled-mods")
  }

  #[must_use]
  pub fn lock_file(&self) -> PathBuf {
    self.dir.join("mods.json")
  }

  #[must_use]
  pub fn logs_dir(&self) -> PathBuf {
    self.dir.join("logs")
  }

  /// Log directory the game itself writes to.
  #[must_use]
  pub fn game_logs_dir(&self) -> PathBuf {
    self.data_dir().join("Logs")
  }

  #[must_use]
  pub const fn has_external_data(&self) -> bool {
    self.data_dir.is_some()
  }

  fn file(&self) -> PathBuf {
    self.dir.join(INSTANCE_FILE)
  }
}

#[derive(Debug, Clone, Default)]
pub struct NewInstance {
  pub name:         String,
  /// Derived from the name when not given.
  pub id:           Option<String>,
  pub game_version: Option<String>,
  pub data_dir:     Option<PathBuf>,
  pub mods_dir:     Option<PathBuf>,
}

/// Result of scanning the instances directory. Instances whose file cannot be
/// read are reported instead of hidden.
#[derive(Debug, Default)]
pub struct Listing {
  pub instances: Vec<Instance>,
  pub broken:    Vec<(String, Error)>,
}

impl Lithic {
  /// # Errors
  /// Returns an error if the instances directory or one of its entries cannot
  /// be read.
  pub fn list_instances(&self) -> Result<Listing> {
    let root = self.paths.instances_dir();
    let mut listing = Listing::default();
    let entries = match fs::read_dir(&root) {
      Ok(entries) => entries,
      Err(e) if e.kind() == ErrorKind::NotFound => return Ok(listing),
      Err(e) => return Err(Error::io(&root, e)),
    };
    for entry in entries {
      let entry = entry.at(&root)?;
      let dir = entry.path();
      if !dir.join(INSTANCE_FILE).is_file() {
        continue;
      }
      let id = entry.file_name().to_string_lossy().into_owned();
      match load(&id, &dir) {
        Ok(instance) => listing.instances.push(instance),
        Err(e) => listing.broken.push((id, e)),
      }
    }
    listing.instances.sort_by(|a, b| {
      b.stats
        .last_played_at
        .cmp(&a.stats.last_played_at)
        .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(listing)
  }

  /// # Errors
  /// Returns an error if the id is invalid, the instance is missing, or its
  /// file cannot be read or parsed.
  pub fn instance(&self, id: &str) -> Result<Instance> {
    let dir = self.paths.instance_dir(id);
    if !valid_id(id) || !dir.join(INSTANCE_FILE).is_file() {
      return Err(Error::not_found(Kind::Instance, id));
    }
    load(id, &dir)
  }

  /// The instance named by `id`, or the active one when `id` is `None`.
  ///
  /// # Errors
  /// Returns an error if no instance is selected, settings cannot be read, or
  /// the selected instance cannot be loaded.
  pub fn resolve_instance(&self, id: Option<&str>) -> Result<Instance> {
    if let Some(id) = id {
      return self.instance(id);
    }
    self.settings()?.active_instance.map_or_else(
      || {
        Err(Error::invalid(
          "no instance selected; pass one explicitly or select one first",
        ))
      },
      |active| self.instance(&active),
    )
  }

  /// # Errors
  /// Returns an error if settings or the active instance's file cannot be read
  /// or parsed.
  pub fn active_instance(&self) -> Result<Option<Instance>> {
    let Some(id) = self.settings()?.active_instance else {
      return Ok(None);
    };
    match self.instance(&id) {
      Ok(instance) => Ok(Some(instance)),
      Err(Error::NotFound { .. }) => Ok(None),
      Err(e) => Err(e),
    }
  }

  /// # Errors
  /// Returns an error if the given instance cannot be loaded or settings cannot
  /// be saved.
  pub fn set_active_instance(&self, id: Option<&str>) -> Result<()> {
    if let Some(id) = id {
      self.instance(id)?;
    }
    self.update_settings(|s| s.active_instance = id.map(ToString::to_string))
  }

  /// # Errors
  /// Returns an error for an empty name, an invalid or occupied id, shared data
  /// directories, or a failure to create and save the instance.
  pub fn create_instance(&self, new: NewInstance) -> Result<Instance> {
    let name = new.name.trim().to_string();
    if name.is_empty() {
      return Err(Error::invalid("an instance needs a name"));
    }
    let root = self.paths.instances_dir();
    fs::create_dir_all(&root).at(&root)?;

    let id = if let Some(id) = new.id {
      if !valid_id(&id) {
        return Err(Error::invalid(format!(
          "`{id}` is not a valid instance id; use lowercase letters, digits \
           and dashes"
        )));
      }
      if root.join(&id).exists() {
        return Err(Error::AlreadyExists {
          kind: Kind::Instance,
          id,
        });
      }
      id
    } else {
      let base = match fsutil::slugify(&name) {
        s if s.is_empty() => "instance".to_string(),
        s => s,
      };
      fsutil::unique_name(&base, |c| root.join(c).exists())
    };

    let data_dir = new.data_dir.map(expand_home);
    let mods_dir = new.mods_dir.map(expand_home);
    if let Some(dir) = &data_dir {
      self.ensure_data_dir_unshared(dir, None)?;
    }

    let dir = root.join(&id);
    let instance = Instance {
      id,
      dir: dir.clone(),
      name,
      game_version: new.game_version.filter(|v| !v.trim().is_empty()),
      data_dir,
      mods_dir,
      account: None,
      launch: LaunchOptions::default(),
      stats: Stats {
        created_at: now_ms(),
        ..Stats::default()
      },
    };

    fs::create_dir_all(&dir).at(&dir)?;
    let result = (|| {
      let mods = instance.mods_dir();
      fs::create_dir_all(&mods).at(&mods)?;
      fsutil::write_toml(&instance.file(), &instance)
    })();
    if let Err(e) = result {
      let _ = fsutil::remove_path(&dir);
      return Err(e);
    }
    Ok(instance)
  }

  /// Applies `f` to the instance as it is on disk right now.
  ///
  /// # Errors
  /// Returns errors from loading or locking the instance, from `f`, from an
  /// empty name or shared data directory, or from saving the updated file.
  pub fn update_instance<R>(
    &self,
    id: &str,
    f: impl FnOnce(&mut Instance) -> Result<R>,
  ) -> Result<R> {
    let current = self.instance(id)?;
    let _lock = fsutil::FileLock::acquire(&current.file())?;
    let mut instance = load(id, &current.dir)?;
    let before = instance.clone();
    let out = f(&mut instance)?;

    instance.name = instance.name.trim().to_string();
    if instance.name.is_empty() {
      return Err(Error::invalid("an instance needs a name"));
    }
    instance.game_version = instance
      .game_version
      .take()
      .filter(|v| !v.trim().is_empty());
    if instance.data_dir != before.data_dir
      && let Some(dir) = &instance.data_dir
    {
      self.ensure_data_dir_unshared(dir, Some(id))?;
    }
    instance.id = before.id;
    instance.dir = before.dir;
    fsutil::write_toml(&instance.file(), &instance)?;
    Ok(out)
  }

  /// Copies an instance: its settings and everything in its data directory
  /// except logs and caches. Saves are copied only when `with_saves` is set.
  /// The copy always gets its own data directory.
  ///
  /// # Errors
  /// Returns an error if the source cannot be loaded, the destination cannot
  /// be created, or its files and settings cannot be copied.
  pub fn clone_instance(
    &self,
    id: &str,
    name: &str,
    with_saves: bool,
  ) -> Result<Instance> {
    let source = self.instance(id)?;
    let mut copy = self.create_instance(NewInstance {
      name: name.to_string(),
      game_version: source.game_version.clone(),
      ..NewInstance::default()
    })?;

    let result = (|| {
      let skip = |rel: &Path| {
        let first = rel
          .components()
          .next()
          .map(|c| c.as_os_str().to_string_lossy().into_owned());
        match first.as_deref() {
          Some("Logs" | "Cache" | "Backups" | "Mods") => true,
          Some("Saves") => !with_saves,
          _ => false,
        }
      };
      let from = source.data_dir();
      if from.is_dir() {
        fsutil::copy_dir(&from, &copy.data_dir(), &skip)?;
      }
      let mods = source.mods_dir();
      if mods.is_dir() {
        fsutil::copy_dir(&mods, &copy.mods_dir(), &|_| false)?;
      }
      let disabled = source.disabled_mods_dir();
      if disabled.is_dir() {
        fsutil::copy_dir(&disabled, &copy.disabled_mods_dir(), &|_| false)?;
      }
      if source.lock_file().is_file() {
        fs::copy(source.lock_file(), copy.lock_file())
          .at(source.lock_file())?;
      }
      self.update_instance(&copy.id, |c| {
        c.account.clone_from(&source.account);
        c.launch.clone_from(&source.launch);
        Ok(())
      })
    })();
    if let Err(e) = result {
      let _ = fsutil::remove_path(&copy.dir);
      return Err(e);
    }
    copy = self.instance(&copy.id)?;
    Ok(copy)
  }

  /// Deletes an instance directory. A `data_dir` or `mods_dir` outside the
  /// instance is left alone.
  ///
  /// # Errors
  /// Returns an error if the instance cannot be loaded or removed, or settings
  /// cannot be updated after deletion.
  pub fn delete_instance(&self, id: &str) -> Result<()> {
    let instance = self.instance(id)?;
    fsutil::remove_path(&instance.dir)?;
    self.update_settings(|s| {
      if s.active_instance.as_deref() == Some(id) {
        s.active_instance = None;
      }
    })
  }

  /// Adds elapsed play time and stamps the last-played time.
  ///
  /// # Errors
  /// Returns an error if the instance cannot be loaded, locked, or saved.
  pub fn record_play_session(
    &self,
    id: &str,
    started_ms: i64,
    ended_ms: i64,
  ) -> Result<()> {
    self.update_instance(id, |i| {
      i.stats.last_played_at = Some(ended_ms.max(started_ms));
      i.stats.play_time_ms += (ended_ms - started_ms).max(0);
      Ok(())
    })
  }

  fn ensure_data_dir_unshared(
    &self,
    dir: &Path,
    except: Option<&str>,
  ) -> Result<()> {
    let wanted = normalize(dir);
    for other in self.list_instances()?.instances {
      if Some(other.id.as_str()) == except {
        continue;
      }
      if normalize(&other.data_dir()) == wanted {
        return Err(Error::InUse {
          what:  dir.display().to_string(),
          users: vec![other.name],
        });
      }
    }
    Ok(())
  }
}

fn load(id: &str, dir: &Path) -> Result<Instance> {
  let file = dir.join(INSTANCE_FILE);
  let mut instance: Instance = fsutil::read_toml(&file)?
    .ok_or_else(|| Error::not_found(Kind::Instance, id))?;
  instance.id = id.to_string();
  instance.dir = dir.to_path_buf();
  Ok(instance)
}

#[must_use]
pub fn valid_id(id: &str) -> bool {
  !id.is_empty()
    && id.len() <= 64
    && id.chars().all(|c| {
      c.is_ascii_lowercase()
        || c.is_ascii_digit()
        || c == '-'
        || c == '_'
        || c == '.'
    })
    && !id.starts_with('.')
}

fn normalize(path: &Path) -> PathBuf {
  fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
#[expect(
  clippy::unwrap_used,
  reason = "test setup and assertions intentionally fail on error"
)]
mod tests {
  use super::*;
  use crate::Paths;

  fn lithic() -> (tempfile::TempDir, Lithic) {
    let dir = tempfile::tempdir().unwrap();
    let lithic = Lithic::new(Paths::rooted(dir.path())).unwrap();
    (dir, lithic)
  }

  #[test]
  fn create_derives_unique_ids() {
    let (_d, l) = lithic();
    let a = l
      .create_instance(NewInstance {
        name: "My Pack".into(),
        ..Default::default()
      })
      .unwrap();
    let b = l
      .create_instance(NewInstance {
        name: "My Pack".into(),
        ..Default::default()
      })
      .unwrap();
    assert_eq!(a.id, "my-pack");
    assert_eq!(b.id, "my-pack-2");
    assert!(a.mods_dir().is_dir());
    assert_eq!(l.list_instances().unwrap().instances.len(), 2);
  }

  #[test]
  fn explicit_id_collision_is_an_error() {
    let (_d, l) = lithic();
    let new = || {
      NewInstance {
        name: "x".into(),
        id: Some("x".into()),
        ..Default::default()
      }
    };
    l.create_instance(new()).unwrap();
    assert!(matches!(
      l.create_instance(new()),
      Err(Error::AlreadyExists { .. })
    ));
    assert!(
      l.create_instance(NewInstance {
        name: "y".into(),
        id: Some("../y".into()),
        ..Default::default()
      })
      .is_err()
    );
  }

  #[test]
  fn update_rereads_and_keeps_other_fields() {
    let (_d, l) = lithic();
    let i = l
      .create_instance(NewInstance {
        name: "a".into(),
        ..Default::default()
      })
      .unwrap();
    l.record_play_session(&i.id, 1000, 61_000).unwrap();
    l.update_instance(&i.id, |x| {
      x.name = "renamed".into();
      Ok(())
    })
    .unwrap();
    let got = l.instance(&i.id).unwrap();
    assert_eq!(got.name, "renamed");
    assert_eq!(got.stats.play_time_ms, 60_000);
    assert_eq!(got.stats.last_played_at, Some(61_000));
  }

  #[test]
  fn shared_data_dir_is_refused() {
    let (d, l) = lithic();
    let shared = d.path().join("shared");
    l.create_instance(NewInstance {
      name: "a".into(),
      data_dir: Some(shared.clone()),
      ..Default::default()
    })
    .unwrap();
    let err = l
      .create_instance(NewInstance {
        name: "b".into(),
        data_dir: Some(shared),
        ..Default::default()
      })
      .unwrap_err();
    assert!(matches!(err, Error::InUse { .. }));
  }

  #[test]
  fn delete_keeps_external_data_and_clears_active() {
    let (d, l) = lithic();
    let external = d.path().join("VintagestoryData");
    fs::create_dir_all(external.join("Saves")).unwrap();
    let i = l
      .create_instance(NewInstance {
        name: "adopted".into(),
        data_dir: Some(external.clone()),
        ..Default::default()
      })
      .unwrap();
    l.set_active_instance(Some(&i.id)).unwrap();
    l.delete_instance(&i.id).unwrap();
    assert!(external.join("Saves").is_dir());
    assert!(!i.dir.exists());
    assert_eq!(l.settings().unwrap().active_instance, None);
  }

  #[test]
  fn clone_copies_mods_and_optionally_saves() {
    let (_d, l) = lithic();
    let src = l
      .create_instance(NewInstance {
        name: "src".into(),
        game_version: Some("1.21.5".into()),
        ..Default::default()
      })
      .unwrap();
    fs::write(src.mods_dir().join("a.zip"), "zip").unwrap();
    fs::create_dir_all(src.data_dir().join("Saves")).unwrap();
    fs::write(src.data_dir().join("Saves/w.vcdbs"), "w").unwrap();
    fs::create_dir_all(src.data_dir().join("Logs")).unwrap();
    fs::write(src.data_dir().join("clientsettings.json"), "{}").unwrap();

    let without = l.clone_instance(&src.id, "copy", false).unwrap();
    assert!(without.mods_dir().join("a.zip").is_file());
    assert!(without.data_dir().join("clientsettings.json").is_file());
    assert!(!without.data_dir().join("Saves").exists());
    assert!(!without.data_dir().join("Logs").exists());
    assert_eq!(without.game_version.as_deref(), Some("1.21.5"));

    let with = l.clone_instance(&src.id, "copy", true).unwrap();
    assert!(with.data_dir().join("Saves/w.vcdbs").is_file());
  }
}
