//! One-time migration from lithic 1.x.
use std::{
  collections::{BTreeMap, BTreeSet},
  fmt,
  fs,
  path::{Component, Path, PathBuf},
};

use crate::{
  Lithic,
  auth::{Account, Accounts},
  error::{Error, IoContext, Result},
  fsutil::{self, now_ms},
  game::{self, Install},
  instance::{self, Instance, LaunchOptions, Stats},
  mods::ModLock,
  paths::expand_home,
  settings::Settings,
};

pub const BACKUP_NAME: &str = "config.toml.v1";

const DROPPED_KEYS: [&str; 6] = [
  "zip_mod_files",
  "notify_of_unzipped_mods",
  "show_execution_time",
  "check_for_updates",
  "update_default_windows_loc",
  "sync_latest_game_version_file_every",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Note {
  Instance {
    id:   String,
    name: String,
  },
  InstanceRenamed {
    from: String,
    to:   String,
  },
  AdoptedModFolder {
    id:  String,
    dir: PathBuf,
  },
  UnknownGameVersion {
    instance:  String,
    reference: String,
  },
  GameInstall {
    version: String,
    path:    PathBuf,
  },
  GameInstallMissing {
    version: String,
    path:    PathBuf,
  },
  DuplicateGameInstall {
    version: String,
    path:    PathBuf,
  },
  SymlinkReplaced {
    instance: String,
    file:     String,
  },
  BrokenSymlinkRemoved {
    instance: String,
    file:     String,
  },
  Pins {
    count: usize,
  },
  Accounts {
    count: usize,
  },
  Favorites {
    count: usize,
  },
  Dropped {
    key: String,
  },
  Removed {
    path: PathBuf,
  },
  OldPacks {
    dir: PathBuf,
  },
  UntrackedFolder {
    dir: PathBuf,
  },
}

impl fmt::Display for Note {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      Self::Instance { id, name } => {
        write!(f, "instance \"{name}\" migrated as `{id}`")
      },
      Self::InstanceRenamed { from, to } => {
        write!(f, "instance id `{from}` is now `{to}`")
      },
      Self::AdoptedModFolder { id, dir } => {
        write!(f, "mod folder {} is now instance `{id}`", dir.display())
      },
      Self::UnknownGameVersion {
        instance,
        reference,
      } => {
        write!(
          f,
          "instance `{instance}` pointed at unknown game version \
           `{reference}`; set one before launching"
        )
      },
      Self::GameInstall { version, path } => {
        write!(f, "game {version} at {}", path.display())
      },
      Self::GameInstallMissing { version, path } => {
        write!(
          f,
          "game {version} at {} no longer exists; kept so you can fix or \
           remove it",
          path.display()
        )
      },
      Self::DuplicateGameInstall { version, path } => {
        write!(
          f,
          "second install of game {version} at {} was left out",
          path.display()
        )
      },
      Self::SymlinkReplaced { instance, file } => {
        write!(
          f,
          "modpack link {file} in `{instance}` replaced with a copy of the mod"
        )
      },
      Self::BrokenSymlinkRemoved { instance, file } => {
        write!(f, "broken modpack link {file} in `{instance}` removed")
      },
      Self::Pins { count } => {
        write!(f, "{count} version pin(s) applied to every instance")
      },
      Self::Accounts { count } => write!(f, "{count} account(s) migrated"),
      Self::Favorites { count } => {
        write!(f, "{count} favourite mod(s) migrated")
      },
      Self::Dropped { key } => {
        write!(f, "setting `{key}` no longer exists and was dropped")
      },
      Self::Removed { path } => write!(f, "removed {}", path.display()),
      Self::OldPacks { dir } => {
        write!(
          f,
          "modpacks from 1.x are still in {}; import them with `lithic pack \
           import`",
          dir.display()
        )
      },
      Self::UntrackedFolder { dir } => {
        write!(
          f,
          "{} belongs to no instance in the old configuration and was left \
           alone; keep it with `lithic instance adopt {}` or delete it",
          dir.display(),
          dir.join("data").display()
        )
      },
    }
  }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
  pub backup: PathBuf,
  pub notes:  Vec<Note>,
}

type Table = toml::Table;

fn get_str(t: &Table, key: &str) -> Option<String> {
  match t.get(key)? {
    toml::Value::String(s) => {
      Some(s.trim().to_string()).filter(|s| !s.is_empty())
    },
    toml::Value::Integer(i) => Some(i.to_string()),
    _ => None,
  }
}

fn get_bool(t: &Table, key: &str) -> Option<bool> {
  t.get(key)?.as_bool()
}

fn get_int(t: &Table, key: &str) -> Option<i64> {
  match t.get(key)? {
    toml::Value::Integer(i) => Some(*i),
    toml::Value::Float(f) => {
      #[expect(
        clippy::cast_possible_truncation,
        reason = "legacy numeric settings intentionally truncate fractional \
                  values"
      )]
      let value = *f as i64;
      Some(value)
    },
    toml::Value::String(s) => s.trim().parse().ok(),
    _ => None,
  }
}

fn get_tables<'a>(t: &'a Table, key: &str) -> Vec<&'a Table> {
  t.get(key)
    .and_then(toml::Value::as_array)
    .map(|a| a.iter().filter_map(toml::Value::as_table).collect())
    .unwrap_or_default()
}

fn get_path(t: &Table, key: &str) -> Option<PathBuf> {
  get_str(t, key).map(expand_home)
}

/// Parses 1.x `env_vars`: `KEY=value` pairs separated by commas.
fn parse_env(raw: &str) -> BTreeMap<String, String> {
  raw
    .split(',')
    .filter_map(|pair| {
      let (k, v) = pair.split_once('=')?;
      let (k, v) = (k.trim(), v.trim());
      (!k.is_empty()).then(|| (k.to_string(), v.to_string()))
    })
    .collect()
}

fn parse_args(raw: &str) -> Vec<String> {
  shell_words::split(raw).unwrap_or_else(|_| {
    raw.split_whitespace().map(ToString::to_string).collect()
  })
}

struct PlannedInstance {
  id:        String,
  legacy_id: Option<String>,
  instance:  Instance,
}

impl Lithic {
  /// Whether a 1.x configuration is waiting to be migrated.
  #[must_use]
  pub fn needs_migration(&self) -> bool {
    self.paths.legacy_config_file().is_file()
      && !self.paths.settings_file().exists()
  }

  /// Migrates a 1.x configuration if there is one. Returns `None` when there
  /// was nothing to do. A config file that does not parse at all is reported
  /// as [`Error::Corrupt`] and left untouched.
  ///
  /// # Errors
  /// Returns an error if the legacy configuration cannot be read, parsed, or
  /// locked, or if migrated files cannot be copied, written, or renamed.
  pub fn migrate(&self) -> Result<Option<Report>> {
    if !self.needs_migration() {
      return Ok(None);
    }
    let legacy_file = self.paths.legacy_config_file();
    let _lock = fsutil::FileLock::acquire(&legacy_file)?;
    if !self.needs_migration() {
      return Ok(None);
    }

    let text = fs::read_to_string(&legacy_file).at(&legacy_file)?;
    let old: Table = toml::from_str(&text).map_err(|e| {
      Error::Corrupt {
        path:    legacy_file.clone(),
        message: format!("{e}; fix or move the file and start lithic again"),
      }
    })?;

    let mut notes = Vec::new();
    let mut settings = Settings::default();

    let installs = Self::plan_game_installs(&old, &mut notes);
    let versions_by_id: BTreeMap<String, String> =
      get_tables(&old, "game_versions")
        .into_iter()
        .filter_map(|g| Some((get_str(g, "id")?, get_str(g, "version")?)))
        .collect();

    let planned = self.plan_instances(&old, &versions_by_id, &mut notes);

    for install in &installs {
      self.register(install.clone())?;
    }

    for p in &planned {
      self.write_instance(p)?;
    }

    // Symlinked modpack mods become real files.
    for p in &planned {
      resolve_symlinks(&p.instance, &mut notes)?;
    }

    // Pins from [[pkg]] apply to every instance.
    let pins: BTreeMap<String, String> = get_tables(&old, "pkg")
      .into_iter()
      .filter_map(|p| {
        Some((
          get_str(p, "mod_id")?.to_ascii_lowercase(),
          get_str(p, "pinned_version")?,
        ))
      })
      .collect();
    if !pins.is_empty() {
      for p in &planned {
        fsutil::update_json(&p.instance.lock_file(), |lock: &mut ModLock| {
          for (id, version) in &pins {
            lock.mods.entry(id.clone()).or_default().pin =
              Some(version.clone());
          }
          Ok(())
        })?;
      }
      notes.push(Note::Pins { count: pins.len() });
    }

    let accounts: Vec<Account> = get_tables(&old, "accounts")
      .into_iter()
      .filter_map(|a| {
        Some(Account {
          uid:        get_str(a, "uid")?,
          playername: get_str(a, "playername").unwrap_or_default(),
          email:      get_str(a, "email").unwrap_or_default(),
        })
      })
      .collect();
    if !accounts.is_empty() {
      let active = get_str(&old, "active_account_uid")
        .filter(|uid| accounts.iter().any(|a| &a.uid == uid));
      notes.push(Note::Accounts {
        count: accounts.len(),
      });
      fsutil::update_toml(&self.paths.accounts_file(), |a: &mut Accounts| {
        for account in &accounts {
          if a.get(&account.uid).is_none() {
            a.accounts.push(account.clone());
          }
        }
        if a.active.is_none() {
          a.active.clone_from(&active);
        }
        Ok(())
      })?;
    }

    let active_legacy = get_str(&old, "active_instance_id");
    settings.active_instance = active_legacy.and_then(|legacy| {
      planned
        .iter()
        .find(|p| p.legacy_id.as_deref() == Some(legacy.as_str()))
        .map(|p| p.id.clone())
    });
    if let Some(enabled) = get_bool(&old, "backup_mods") {
      settings.backups.enabled = enabled;
    }
    if let Some(dir) = get_path(&old, "backup_mods_dir") {
      settings.backups.dir = Some(dir);
    }
    settings.game.download_dir = get_path(&old, "game_download_dir");
    if let Some(hours) = get_int(&old, "sync_mod_search_file_every") {
      settings.mods.index_max_age_hours =
        u32::try_from(hours.clamp(1, 24 * 30)).unwrap_or(24);
    }
    if let Some(table) = old.get("table").and_then(toml::Value::as_table) {
      settings.cli.table = table.clone();
    }
    if let Some(mode) = get_str(&old, "theme_mode") {
      settings.gui.theme_mode = mode;
    }
    if let Some(preset) = get_str(&old, "theme_preset") {
      settings.gui.theme_preset = preset;
    }
    if let Some(page) = get_str(&old, "initial_page") {
      settings.gui.initial_page = page;
    }
    let favorites_file = self.paths.config.join("lithic-gui-favorites.json");
    if let Ok(text) = fs::read_to_string(&favorites_file)
      && let Ok(list) = serde_json::from_str::<BTreeSet<String>>(&text)
    {
      notes.push(Note::Favorites { count: list.len() });
      settings.gui.favorites = list;
      let _ = fs::remove_file(&favorites_file);
    }
    for key in DROPPED_KEYS {
      if old.contains_key(key) {
        notes.push(Note::Dropped {
          key: key.to_string(),
        });
      }
    }

    for cache in ["game-versions.json", "mod-search.json"] {
      let path = self.paths.config.join(cache);
      if path.is_file() && fs::remove_file(&path).is_ok() {
        notes.push(Note::Removed { path });
      }
    }
    for p in &planned {
      let sync = p.instance.mods_dir().join("lithic-sync.json");
      if sync.is_file() && fs::remove_file(&sync).is_ok() {
        notes.push(Note::Removed { path: sync });
      }
    }
    let old_game_dir = self.paths.data.join("game-versions");
    if let Ok(entries) = fs::read_dir(&old_game_dir) {
      for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let is_archive = name.starts_with("vs_")
          && [".tar.gz", ".exe", ".zip"]
            .iter()
            .any(|ext| name.ends_with(ext));
        if is_archive
          && entry.path().is_file()
          && fs::remove_file(entry.path()).is_ok()
        {
          notes.push(Note::Removed { path: entry.path() });
        }
      }
    }
    let modpacks = old
      .get("modpacks")
      .and_then(toml::Value::as_table)
      .and_then(|m| get_path(m, "modpack_dir"))
      .unwrap_or_else(|| self.paths.data.join("modpacks"));
    if modpacks.join("mypacks").is_dir() || modpacks.join("packs").is_dir() {
      notes.push(Note::OldPacks { dir: modpacks });
    }
    if let Ok(entries) = fs::read_dir(self.paths.instances_dir()) {
      for entry in entries.flatten() {
        let dir = entry.path();
        if dir.is_dir() && !dir.join(instance::INSTANCE_FILE).exists() {
          notes.push(Note::UntrackedFolder { dir });
        }
      }
    }

    // settings.toml marks the migration as done, so it is written last.
    fsutil::write_toml(&self.paths.settings_file(), &settings)?;
    let backup = self.paths.config.join(BACKUP_NAME);
    fs::rename(&legacy_file, &backup).at(&backup)?;
    let _ = fs::remove_file(fsutil::FileLock::path_for(&legacy_file));
    Ok(Some(Report { backup, notes }))
  }

  fn plan_game_installs(old: &Table, notes: &mut Vec<Note>) -> Vec<Install> {
    let mut out: Vec<Install> = Vec::new();
    for g in get_tables(old, "game_versions") {
      let (Some(version), Some(path)) =
        (get_str(g, "version"), get_path(g, "path"))
      else {
        continue;
      };
      let version = version.trim_start_matches(['v', 'V']).to_string();
      if out.iter().any(|i| i.version.eq_ignore_ascii_case(&version)) {
        notes.push(Note::DuplicateGameInstall { version, path });
        continue;
      }
      let located = game::locate_game_dir(&path);
      match &located {
        Some(p) => {
          notes.push(Note::GameInstall {
            version: version.clone(),
            path:    p.clone(),
          });
        },
        None => {
          notes.push(Note::GameInstallMissing {
            version: version.clone(),
            path:    path.clone(),
          });
        },
      }
      out.push(Install {
        version,
        path: located.unwrap_or(path),
        managed: get_str(g, "source").as_deref() == Some("lithic_download"),
      });
    }
    out
  }

  fn plan_instances(
    &self,
    old: &Table,
    versions_by_id: &BTreeMap<String, String>,
    notes: &mut Vec<Note>,
  ) -> Vec<PlannedInstance> {
    fn unique(taken: &mut BTreeSet<String>, base: &str) -> String {
      let base = if base.is_empty() { "instance" } else { base };
      let id = fsutil::unique_name(base, |c| taken.contains(c));
      taken.insert(id.clone());
      id
    }
    let pinned_game = get_str(old, "pinned_game_version");
    let mut taken: BTreeSet<String> = BTreeSet::new();
    let mut out = Vec::new();

    let legacy = get_tables(old, "instances");
    for t in &legacy {
      let Some(legacy_id) = get_str(t, "id") else {
        continue;
      };
      let name = get_str(t, "name").unwrap_or_else(|| legacy_id.clone());
      let legacy_data = get_path(t, "data_dir");
      let legacy_mods = get_path(t, "mods_dir");

      // An instance whose data already sits in `instances/<name>/data`
      // keeps that folder, and with it that name as its id.
      let home = used_data_dir(legacy_data.as_ref(), legacy_mods.as_ref())
        .and_then(|d| self.managed_instance_name(&d))
        .filter(|n| instance::valid_id(n) && !taken.contains(n));
      let base = match &home {
        Some(n) => n.clone(),
        None if instance::valid_id(&legacy_id) => legacy_id.clone(),
        None => fsutil::slugify(&legacy_id),
      };
      let id = unique(&mut taken, &base);
      if id != legacy_id {
        notes.push(Note::InstanceRenamed {
          from: legacy_id.clone(),
          to:   id.clone(),
        });
      }

      let dir = self.paths.instance_dir(&id);
      let legacy_home = self
        .paths
        .instance_dir(home.as_deref().unwrap_or(&legacy_id));
      let (data_dir, mods_dir) =
        placement(&legacy_home, &dir, legacy_data.as_ref(), legacy_mods);

      let game_version = get_str(t, "game_version_id").map_or_else(
        || pinned_game.clone(),
        |reference| {
          versions_by_id.get(&reference).map_or_else(
            || {
              notes.push(Note::UnknownGameVersion {
                instance: id.clone(),
                reference,
              });
              pinned_game.clone()
            },
            |v| Some(v.trim_start_matches(['v', 'V']).to_string()),
          )
        },
      );

      let instance = Instance {
        id: id.clone(),
        dir,
        name: name.clone(),
        game_version,
        data_dir,
        mods_dir,
        account: get_str(t, "account_uid"),
        launch: LaunchOptions {
          args:    get_str(t, "start_params")
            .map(|s| parse_args(&s))
            .unwrap_or_default(),
          env:     get_str(t, "env_vars")
            .map(|s| parse_env(&s))
            .unwrap_or_default(),
          wrapper: Vec::new(),
        },
        stats: Stats {
          created_at:     now_ms(),
          last_played_at: get_int(t, "last_played_at").filter(|t| *t > 0),
          play_time_ms:   get_int(t, "total_play_time_ms").unwrap_or(0).max(0),
        },
      };
      notes.push(Note::Instance {
        id: id.clone(),
        name,
      });
      out.push(PlannedInstance {
        id,
        legacy_id: Some(legacy_id),
        instance,
      });
    }

    // 1.x without instances managed a single mod folder directly.
    if legacy.is_empty()
      && let Some(mod_dir) = get_path(old, "mod_dir")
      && mod_dir.is_dir()
    {
      let id = unique(&mut taken, "default");
      let dir = self.paths.instance_dir(&id);
      let (data_dir, mods_dir) =
        placement(&dir, &dir, None, Some(mod_dir.clone()));
      notes.push(Note::AdoptedModFolder {
        id:  id.clone(),
        dir: mod_dir,
      });
      out.push(PlannedInstance {
        id:        id.clone(),
        legacy_id: None,
        instance:  Instance {
          id,
          dir,
          name: "Default".to_string(),
          game_version: pinned_game,
          data_dir,
          mods_dir,
          account: None,
          launch: LaunchOptions::default(),
          stats: Stats {
            created_at: now_ms(),
            ..Stats::default()
          },
        },
      });
    }
    out
  }

  fn write_instance(&self, p: &PlannedInstance) -> Result<()> {
    if let Some(legacy_id) = &p.legacy_id
      && legacy_id != &p.id
      && is_plain_name(legacy_id)
    {
      let old_dir = self.paths.instance_dir(legacy_id);
      if old_dir.is_dir()
        && !p.instance.dir.exists()
        && !old_dir.join(instance::INSTANCE_FILE).exists()
      {
        fsutil::move_path(&old_dir, &p.instance.dir)?;
      }
    }
    fs::create_dir_all(&p.instance.dir).at(&p.instance.dir)?;
    let mods = p.instance.mods_dir();
    fs::create_dir_all(&mods).at(&mods)?;
    fsutil::write_toml(
      &p.instance.dir.join(instance::INSTANCE_FILE),
      &p.instance,
    )
  }
}

/// Decides `data_dir` and `mods_dir` for a migrated instance. `None` means
/// the default location inside the instance directory.
///
/// 1.x created instances at `<data>/instances/<id>/data` with mods in its
/// `Mods` folder, which is exactly the 2.0 default. The automatic "default"
/// instance of 1.x had no data dir and pointed at the stock launcher's mod
/// folder; that folder's parent is adopted as the data directory.
fn placement(
  legacy_instance_dir: &Path,
  new_instance_dir: &Path,
  data: Option<&PathBuf>,
  mods: Option<PathBuf>,
) -> (Option<PathBuf>, Option<PathBuf>) {
  let default_legacy = legacy_instance_dir.join("data");
  let data_dir = used_data_dir(data, mods.as_ref())
    .filter(|d| *d != default_legacy && *d != new_instance_dir.join("data"));
  let effective_data = data_dir
    .clone()
    .unwrap_or_else(|| new_instance_dir.join("data"));
  let mods_dir = mods.filter(|m| {
    let expected_new = effective_data.join("Mods");
    let expected_old = default_legacy.join("Mods");
    *m != expected_new && *m != expected_old
  });
  (data_dir, mods_dir)
}

/// The data folder a 1.x instance actually ran with: its `data_dir`, or for
/// the automatic instance without one, the folder holding its `Mods`.
fn used_data_dir(
  data: Option<&PathBuf>,
  mods: Option<&PathBuf>,
) -> Option<PathBuf> {
  data.cloned().or_else(|| {
    mods
      .filter(|m| m.file_name().is_some_and(|n| n == "Mods"))
      .and_then(|m| m.parent())
      .map(Path::to_path_buf)
  })
}

impl Lithic {
  /// `<name>` when `data` is `<data>/instances/<name>/data`.
  fn managed_instance_name(&self, data: &Path) -> Option<String> {
    if data.file_name()? != "data" {
      return None;
    }
    let dir = data.parent()?;
    (dir.parent()? == self.paths.instances_dir())
      .then(|| fsutil::file_name_string(dir))
  }
}

/// A single path component, so joining it cannot leave the parent directory.
fn is_plain_name(name: &str) -> bool {
  let mut components = Path::new(name).components();
  matches!(components.next(), Some(Component::Normal(_)))
    && components.next().is_none()
}

fn resolve_symlinks(instance: &Instance, notes: &mut Vec<Note>) -> Result<()> {
  let dir = instance.mods_dir();
  let Ok(entries) = fs::read_dir(&dir) else {
    return Ok(());
  };
  for entry in entries.flatten() {
    let path = entry.path();
    let Ok(meta) = fs::symlink_metadata(&path) else {
      continue;
    };
    if !meta.file_type().is_symlink() {
      continue;
    }
    let file = entry.file_name().to_string_lossy().into_owned();
    if let Ok(target) = fs::canonicalize(&path) {
      let tmp = dir.join(format!(".{file}.migrating"));
      if target.is_dir() {
        fsutil::copy_dir(&target, &tmp, &|_| false)?;
      } else {
        fs::copy(&target, &tmp).at(&target)?;
      }
      fs::remove_file(&path).at(&path)?;
      fs::rename(&tmp, &path).at(&path)?;
      notes.push(Note::SymlinkReplaced {
        instance: instance.id.clone(),
        file,
      });
    } else {
      fs::remove_file(&path).at(&path)?;
      notes.push(Note::BrokenSymlinkRemoved {
        instance: instance.id.clone(),
        file,
      });
    }
  }
  Ok(())
}

#[cfg(test)]
#[expect(
  clippy::unwrap_used,
  reason = "test setup and assertions intentionally fail on error"
)]
mod tests {
  #[cfg(unix)] use std::os::unix::fs::symlink;

  use super::*;
  use crate::Paths;

  fn legacy_config(root: &Path) -> String {
    let data = root.join("data");
    let adopted = root.join("VintagestoryData");
    format!(
      r#"
mod_dir = "{adopted}/Mods"
pinned_game_version = "1.20.12"
zip_mod_files = false
backup_mods = true
backup_mods_dir = "{data}/mod_backups"
show_execution_time = true
notify_of_unzipped_mods = false
game_download_dir = "/home/u/Downloads"
check_for_updates = true
sync_latest_game_version_file_every = 24
sync_mod_search_file_every = 12
active_instance_id = "My Instance"
theme_mode = "dark"
theme_preset = "Nord"
initial_page = "installed"
active_account_uid = "uid-1"

[modpacks]
modpack_dir = "{data}/modpacks"
enabled = ["pack1"]
disabled = []

[[pkg]]
mod_id = "CarryOn"
pinned_version = "1.6.0"

[[pkg]]
mod_id = "nopin"

[table.list.headers]
"name.color" = "green"
"name.attribute" = "bold"

[table.search.cells]
"mod_id.color" = "magenta"

[[instances]]
id = "My Instance"
name = "My Instance"
data_dir = "{data}/instances/My Instance/data"
mods_dir = "{data}/instances/My Instance/data/Mods"
game_version_id = "vs121"
enabled_modpacks = []
start_params = "--foo 'with space'"
env_vars = "A=1, B=two"
last_played_at = 1700000000000
total_play_time_ms = 3600000
account_uid = "uid-2"

[[instances]]
id = "default"
name = "Default"
data_dir = ""
mods_dir = "{adopted}/Mods"
game_version_id = "gone"
last_played_at = 0
total_play_time_ms = 0

[[instances]]
id = "custom"
name = "Custom Mods"
data_dir = "{data}/instances/custom/data"
mods_dir = "{root}/elsewhere/mods"
game_version_id = ""

[[game_versions]]
id = "vs121"
version = "1.21.5"
path = "{data}/game-versions/vs121/vintagestory"
source = "lithic_download"
os = "linux"

[[game_versions]]
id = "vs121-copy"
version = "1.21.5"
path = "/nonexistent"
source = "manual"

[[accounts]]
uid = "uid-1"
playername = "One"
email = "one@example.com"

[[accounts]]
uid = "uid-2"
playername = "Two"
"#,
      adopted = adopted.display(),
      data = data.display(),
      root = root.display(),
    )
  }

  fn setup() -> (tempfile::TempDir, Lithic) {
    let d = tempfile::tempdir().unwrap();
    let paths = Paths::rooted(d.path());
    let root = d.path();
    fs::create_dir_all(&paths.config).unwrap();
    fs::write(paths.legacy_config_file(), legacy_config(root)).unwrap();
    fs::write(
      paths.config.join("lithic-gui-favorites.json"),
      r#"["carryon","betterruins"]"#,
    )
    .unwrap();
    fs::write(paths.config.join("mod-search.json"), "{}").unwrap();

    let exe = if cfg!(windows) {
      "Vintagestory.exe"
    } else {
      "Vintagestory"
    };
    let game = paths.data.join("game-versions/vs121/vintagestory");
    fs::create_dir_all(&game).unwrap();
    fs::write(game.join(exe), "").unwrap();
    fs::write(
      paths
        .data
        .join("game-versions/vs_client_linux-x64_1.21.5.tar.gz"),
      "archive",
    )
    .unwrap();

    let inst_mods = paths.data.join("instances/My Instance/data/Mods");
    fs::create_dir_all(&inst_mods).unwrap();
    fs::write(inst_mods.join("real.zip"), "zip").unwrap();
    fs::write(inst_mods.join("lithic-sync.json"), "{}").unwrap();
    fs::create_dir_all(paths.data.join("instances/My Instance/data/Saves"))
      .unwrap();

    let pack_mod = paths.data.join("modpacks/installed/pack1/packmod.zip");
    fs::create_dir_all(pack_mod.parent().unwrap()).unwrap();
    fs::write(&pack_mod, "pack mod").unwrap();
    fs::create_dir_all(paths.data.join("modpacks/mypacks")).unwrap();
    #[cfg(unix)]
    {
      symlink(&pack_mod, inst_mods.join("packmod.zip")).unwrap();
      symlink(root.join("missing.zip"), inst_mods.join("dangling.zip"))
        .unwrap();
    }

    fs::create_dir_all(root.join("VintagestoryData/Mods")).unwrap();
    fs::write(root.join("VintagestoryData/Mods/stock.zip"), "zip").unwrap();

    let lithic = Lithic::new(paths).unwrap();
    (d, lithic)
  }

  #[test]
  fn full_migration() {
    let (d, l) = setup();
    let root = d.path();
    assert!(l.needs_migration());
    let report = l.migrate().unwrap().unwrap();
    assert!(!l.needs_migration());
    assert!(report.backup.is_file());
    assert!(!l.paths.legacy_config_file().exists());

    let listing = l.list_instances().unwrap();
    assert!(listing.broken.is_empty());
    assert_eq!(listing.instances.len(), 3);

    let main = l.instance("my-instance").unwrap();
    assert_eq!(main.name, "My Instance");
    assert_eq!(main.game_version.as_deref(), Some("1.21.5"));
    assert_eq!(main.account.as_deref(), Some("uid-2"));
    assert_eq!(main.launch.args, ["--foo", "with space"]);
    assert_eq!(main.launch.env.get("B").map(String::as_str), Some("two"));
    assert_eq!(main.stats.play_time_ms, 3_600_000);
    assert_eq!(main.stats.last_played_at, Some(1_700_000_000_000));
    assert_eq!(
      main.data_dir, None,
      "the old default location became the new default"
    );
    assert_eq!(main.mods_dir, None);
    assert!(
      main.mods_dir().join("real.zip").is_file(),
      "instance folder moved with its data"
    );
    assert!(main.data_dir().join("Saves").is_dir());
    assert!(!main.mods_dir().join("lithic-sync.json").exists());

    let adopted = l.instance("default").unwrap();
    assert_eq!(adopted.data_dir, Some(root.join("VintagestoryData")));
    assert_eq!(adopted.mods_dir, None);
    assert_eq!(
      adopted.game_version.as_deref(),
      Some("1.20.12"),
      "unknown reference falls back to the pin"
    );

    let custom = l.instance("custom").unwrap();
    assert_eq!(custom.data_dir, None);
    assert_eq!(custom.mods_dir, Some(root.join("elsewhere/mods")));

    let settings = l.settings().unwrap();
    assert_eq!(settings.active_instance.as_deref(), Some("my-instance"));
    assert!(settings.backups.enabled);
    assert_eq!(settings.backups.dir, Some(root.join("data/mod_backups")));
    assert_eq!(
      settings.game.download_dir,
      Some(PathBuf::from("/home/u/Downloads"))
    );
    assert_eq!(settings.mods.index_max_age_hours, 12);
    assert_eq!(settings.gui.theme_mode, "dark");
    assert_eq!(settings.gui.theme_preset, "Nord");
    assert_eq!(settings.gui.initial_page, "installed");
    assert_eq!(settings.gui.favorites.len(), 2);
    assert!(settings.cli.table.contains_key("list"));
    assert!(settings.cli.table.contains_key("search"));

    let installs = l.game_installs().unwrap();
    assert_eq!(installs.len(), 1);
    assert!(installs[0].managed);
    assert!(
      report
        .notes
        .iter()
        .any(|n| matches!(n, Note::DuplicateGameInstall { .. }))
    );
    assert!(
      !l.paths
        .data
        .join("game-versions/vs_client_linux-x64_1.21.5.tar.gz")
        .exists()
    );

    let accounts = l.accounts().unwrap();
    assert_eq!(accounts.accounts.len(), 2);
    assert_eq!(accounts.active.as_deref(), Some("uid-1"));

    for i in &listing.instances {
      let lock = l.mod_lock(i).unwrap();
      assert_eq!(
        lock.mods.get("carryon").and_then(|e| e.pin.as_deref()),
        Some("1.6.0")
      );
      assert!(!lock.mods.contains_key("nopin"));
    }

    for key in ["zip_mod_files", "check_for_updates", "show_execution_time"] {
      assert!(report.notes.contains(&Note::Dropped { key: key.into() }));
    }
    assert!(
      report
        .notes
        .iter()
        .any(|n| matches!(n, Note::InstanceRenamed { .. }))
    );
    assert!(
      report
        .notes
        .iter()
        .any(|n| matches!(n, Note::OldPacks { .. }))
    );
    assert!(!l.paths.config.join("mod-search.json").exists());
    assert!(!l.paths.config.join("lithic-gui-favorites.json").exists());

    #[cfg(unix)]
    {
      let packmod = main.mods_dir().join("packmod.zip");
      assert!(
        !fs::symlink_metadata(&packmod)
          .unwrap()
          .file_type()
          .is_symlink()
      );
      assert_eq!(fs::read_to_string(&packmod).unwrap(), "pack mod");
      assert!(!main.mods_dir().join("dangling.zip").exists());
    }

    for note in &report.notes {
      assert!(!note.to_string().contains('\u{2014}'));
    }

    assert!(l.migrate().unwrap().is_none(), "second run is a no-op");
  }

  #[test]
  fn instance_less_config_adopts_mod_folder() {
    let d = tempfile::tempdir().unwrap();
    let paths = Paths::rooted(d.path());
    fs::create_dir_all(&paths.config).unwrap();
    let stock = d.path().join("VintagestoryData/Mods");
    fs::create_dir_all(&stock).unwrap();
    fs::write(
      paths.legacy_config_file(),
      format!("mod_dir = \"{}\"\n", stock.display()),
    )
    .unwrap();
    let l = Lithic::new(paths).unwrap();
    l.migrate().unwrap().unwrap();
    let i = l.instance("default").unwrap();
    assert_eq!(i.data_dir, Some(d.path().join("VintagestoryData")));
    assert_eq!(i.mods_dir(), stock);
  }

  #[test]
  fn automatic_instance_keeps_its_managed_folder() {
    let d = tempfile::tempdir().unwrap();
    let paths = Paths::rooted(d.path());
    fs::create_dir_all(&paths.config).unwrap();
    let mods = paths.instance_dir("main").join("data/Mods");
    fs::create_dir_all(&mods).unwrap();
    fs::write(mods.join("m.zip"), "zip").unwrap();
    fs::write(
      paths.legacy_config_file(),
      format!(
        "[[instances]]\nid = \"default\"\nname = \"Default\"\ndata_dir = \
         \"\"\nmods_dir = \"{}\"\n",
        mods.display()
      ),
    )
    .unwrap();
    let l = Lithic::new(paths).unwrap();
    let report = l.migrate().unwrap().unwrap();
    let listing = l.list_instances().unwrap();
    assert_eq!(listing.instances.len(), 1);
    let i = &listing.instances[0];
    assert_eq!(i.id, "main");
    assert_eq!(i.name, "Default");
    assert_eq!(i.data_dir, None);
    assert_eq!(i.mods_dir, None);
    assert!(i.mods_dir().join("m.zip").is_file());
    assert!(report.notes.contains(&Note::InstanceRenamed {
      from: "default".into(),
      to:   "main".into(),
    }));
  }

  #[test]
  fn unparsable_config_is_left_alone() {
    let d = tempfile::tempdir().unwrap();
    let paths = Paths::rooted(d.path());
    fs::create_dir_all(&paths.config).unwrap();
    fs::write(paths.legacy_config_file(), "mod_dir = = broken").unwrap();
    let l = Lithic::new(paths).unwrap();
    assert!(matches!(l.migrate(), Err(Error::Corrupt { .. })));
    assert!(l.paths.legacy_config_file().is_file());
    assert!(!l.paths.settings_file().exists());
  }

  #[test]
  fn odd_values_do_not_block_the_rest() {
    let d = tempfile::tempdir().unwrap();
    let paths = Paths::rooted(d.path());
    fs::create_dir_all(&paths.config).unwrap();
    fs::write(
      paths.legacy_config_file(),
      "backup_mods = \"yes\"\ntheme_mode = 5\n[[instances]]\nid = \"a\"\nname \
       = \"A\"\ntotal_play_time_ms = \"12\"\nstart_params = \"--x \
       'unterminated\"\n",
    )
    .unwrap();
    let l = Lithic::new(paths).unwrap();
    l.migrate().unwrap().unwrap();
    let a = l.instance("a").unwrap();
    assert_eq!(a.stats.play_time_ms, 12);
    assert_eq!(a.launch.args, ["--x", "'unterminated"]);
    assert!(!l.settings().unwrap().backups.enabled);
  }
}
