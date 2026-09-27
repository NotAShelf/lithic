use std::{
  ffi::OsString,
  fs::{self, File, OpenOptions},
  io::{ErrorKind, Write},
  path::{Path, PathBuf},
  process,
  time::{SystemTime, UNIX_EPOCH},
};

use serde::{Serialize, de::DeserializeOwned};

use crate::error::{Error, IoContext, Result};

/// Milliseconds since the Unix epoch.
#[must_use]
pub fn now_ms() -> i64 {
  SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

/// Writes `bytes` to a sibling temp file and renames it over `path`, so readers
/// never see a half-written file.
///
/// # Errors
/// Returns an I/O error if the temp file cannot be written or renamed.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
  write_atomic_with_mode(path, bytes, None)
}

/// Like [`write_atomic`], but the file is created with `mode` on Unix. The
/// mode is applied at creation, so the contents are never readable by others,
/// not even briefly.
///
/// # Errors
/// Returns an I/O error if the directory or temp file cannot be created,
/// written, synced, or renamed.
pub fn write_atomic_with_mode(
  path: &Path,
  bytes: &[u8],
  mode: Option<u32>,
) -> Result<()> {
  let parent = path.parent().unwrap_or_else(|| Path::new("."));
  fs::create_dir_all(parent).at(parent)?;

  let mut tmp_name = path.file_name().unwrap_or_default().to_os_string();
  tmp_name.push(format!(".{}.tmp", process::id()));
  let tmp = parent.join(tmp_name);

  let mut options = OpenOptions::new();
  options.write(true).create(true).truncate(true);
  #[cfg(unix)]
  if let Some(mode) = mode {
    use std::os::unix::fs::OpenOptionsExt;
    options.mode(mode);
  }
  #[cfg(not(unix))]
  let _ = mode;

  let result = (|| {
    let mut file = options.open(&tmp).at(&tmp)?;
    file.write_all(bytes).at(&tmp)?;
    file.sync_all().at(&tmp)?;
    fs::rename(&tmp, path).at(path)
  })();
  if result.is_err() {
    let _ = fs::remove_file(&tmp);
  }
  result
}

/// Reads a TOML file. A missing file is `Ok(None)`; a file that exists but
/// does not parse is [`Error::Corrupt`] and is never overwritten by callers.
///
/// # Errors
/// Returns an I/O error if the file cannot be read, or [`Error::Corrupt`]
/// if the existing TOML cannot be parsed.
pub fn read_toml<T: DeserializeOwned>(path: &Path) -> Result<Option<T>> {
  let text = match fs::read_to_string(path) {
    Ok(text) => text,
    Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
    Err(e) => return Err(Error::io(path, e)),
  };
  toml::from_str(&text).map(Some).map_err(|e| {
    Error::Corrupt {
      path:    path.to_path_buf(),
      message: e.to_string(),
    }
  })
}

/// Writes a value as TOML, replacing the file atomically.
///
/// # Errors
/// Returns an error if serialization or writing fails.
pub fn write_toml<T: Serialize>(path: &Path, value: &T) -> Result<()> {
  let text = toml::to_string_pretty(value)
    .map_err(|e| Error::parse(path.display().to_string(), e))?;
  write_atomic(path, text.as_bytes())
}

/// Reads a JSON file, returning `None` if it does not exist.
///
/// # Errors
/// Returns an I/O error if the file cannot be read, or [`Error::Corrupt`]
/// if the existing JSON cannot be parsed.
pub fn read_json<T: DeserializeOwned>(path: &Path) -> Result<Option<T>> {
  let bytes = match fs::read(path) {
    Ok(bytes) => bytes,
    Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
    Err(e) => return Err(Error::io(path, e)),
  };
  serde_json::from_slice(&bytes).map(Some).map_err(|e| {
    Error::Corrupt {
      path:    path.to_path_buf(),
      message: e.to_string(),
    }
  })
}

/// Writes a value as JSON, replacing the file atomically.
///
/// # Errors
/// Returns an error if serialization or writing fails.
pub fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
  let bytes = serde_json::to_vec_pretty(value)
    .map_err(|e| Error::parse(path.display().to_string(), e))?;
  write_atomic(path, &bytes)
}

/// Holds an exclusive advisory lock on `<path>.lock` until dropped. Every
/// read-modify-write of a shared file goes through one of these, so the GUI and
/// CLI can run at the same time without losing each other's changes.
pub struct FileLock {
  _file: File,
}

impl FileLock {
  /// # Errors
  /// Returns an I/O error if the lock file cannot be created or locked.
  pub fn acquire(path: &Path) -> Result<Self> {
    let lock_path = Self::path_for(path);
    if let Some(parent) = lock_path.parent() {
      fs::create_dir_all(parent).at(parent)?;
    }
    let file = OpenOptions::new()
      .create(true)
      .truncate(false)
      .write(true)
      .open(&lock_path)
      .at(&lock_path)?;
    file.lock().at(&lock_path)?;
    Ok(Self { _file: file })
  }

  /// `.<name>.lock` next to `path`.
  #[must_use]
  pub fn path_for(path: &Path) -> PathBuf {
    let mut name = OsString::from(".");
    name.push(path.file_name().unwrap_or_default());
    name.push(".lock");
    path.with_file_name(name)
  }
}

/// Re-reads `path`, applies `f`, and writes the result back, all while holding
/// the file's lock. A missing file starts from `T::default()`.
///
/// # Errors
/// Returns an error if acquiring the lock, reading or writing the file, or
/// applying `f` fails. Invalid existing TOML is left untouched.
pub fn update_toml<T, R>(
  path: &Path,
  f: impl FnOnce(&mut T) -> Result<R>,
) -> Result<R>
where
  T: DeserializeOwned + Serialize + Default,
{
  let _lock = FileLock::acquire(path)?;
  let mut value: T = read_toml(path)?.unwrap_or_default();
  let out = f(&mut value)?;
  write_toml(path, &value)?;
  Ok(out)
}

/// [`update_toml`] for JSON files.
///
/// # Errors
/// Returns an error if acquiring the lock, reading or writing the file, or
/// applying `f` fails. Invalid existing JSON is left untouched.
pub fn update_json<T, R>(
  path: &Path,
  f: impl FnOnce(&mut T) -> Result<R>,
) -> Result<R>
where
  T: DeserializeOwned + Serialize + Default,
{
  let _lock = FileLock::acquire(path)?;
  let mut value: T = read_json(path)?.unwrap_or_default();
  let out = f(&mut value)?;
  write_json(path, &value)?;
  Ok(out)
}

/// Recursively copies `from` into `to`. Entries for which `skip` returns true
/// (given the path relative to `from`) are left out. Symlinks are copied as
/// the files they point to.
///
/// # Errors
/// Returns an I/O error if a directory cannot be created or an entry cannot
/// be read or copied.
pub fn copy_dir(
  from: &Path,
  to: &Path,
  skip: &dyn Fn(&Path) -> bool,
) -> Result<()> {
  fn walk(
    root: &Path,
    dir: &Path,
    to: &Path,
    skip: &dyn Fn(&Path) -> bool,
  ) -> Result<()> {
    fs::create_dir_all(to).at(to)?;
    for entry in fs::read_dir(dir).at(dir)? {
      let entry = entry.at(dir)?;
      let src = entry.path();
      let rel = src.strip_prefix(root).unwrap_or(&src);
      if skip(rel) {
        continue;
      }
      let dst = to.join(entry.file_name());
      if src.is_dir() {
        walk(root, &src, &dst, skip)?;
      } else {
        fs::copy(&src, &dst).at(&src)?;
      }
    }
    Ok(())
  }
  walk(from, from, to, skip)
}

/// Removes a file, directory tree, or symlink. Missing paths are fine.
///
/// # Errors
/// Returns an I/O error if the path cannot be inspected or removed.
pub fn remove_path(path: &Path) -> Result<()> {
  let meta = match fs::symlink_metadata(path) {
    Ok(meta) => meta,
    Err(e) if e.kind() == ErrorKind::NotFound => return Ok(()),
    Err(e) => return Err(Error::io(path, e)),
  };
  if meta.is_dir() {
    fs::remove_dir_all(path).at(path)
  } else {
    fs::remove_file(path).at(path)
  }
}

/// Moves `from` to `to`. Copies and removes the source only when the paths
/// are on different filesystems; other rename failures leave it untouched.
///
/// # Errors
/// Returns an I/O error if the destination cannot be created, or if renaming,
/// copying across filesystems, or removing the source fails.
pub fn move_path(from: &Path, to: &Path) -> Result<()> {
  if let Some(parent) = to.parent() {
    fs::create_dir_all(parent).at(parent)?;
  }
  match fs::rename(from, to) {
    Ok(()) => Ok(()),
    Err(e) if e.kind() == ErrorKind::CrossesDevices && from.is_dir() => {
      copy_dir(from, to, &|_| false)?;
      remove_path(from)
    },
    Err(e) if e.kind() == ErrorKind::CrossesDevices => {
      fs::copy(from, to).at(from)?;
      remove_path(from)
    },
    Err(e) => Err(Error::io(to, e)),
  }
}

/// Turns free text into an identifier safe for use as a directory name:
/// lowercase ASCII letters, digits and single dashes.
pub fn slugify(text: &str) -> String {
  let mut out = String::new();
  let mut dash = false;
  for c in text.chars().flat_map(char::to_lowercase) {
    if c.is_ascii_alphanumeric() {
      out.push(c);
      dash = false;
    } else if !dash && !out.is_empty() {
      out.push('-');
      dash = true;
    }
  }
  while out.ends_with('-') {
    out.pop();
  }
  out
}

/// Strips anything from a server-provided file name that could escape the
/// target directory or upset Windows.
#[must_use]
pub fn sanitize_file_name(name: &str) -> String {
  let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
  let cleaned: String = base
    .chars()
    .map(|c| {
      match c {
        '<' | '>' | ':' | '"' | '|' | '?' | '*' => '_',
        c if c.is_control() => '_',
        c => c,
      }
    })
    .collect();
  let cleaned = cleaned.trim().trim_start_matches('.').to_string();
  if cleaned.is_empty() {
    "download".to_string()
  } else {
    cleaned
  }
}

/// Picks `<base>`, `<base>-2`, `<base>-3` and so on, whichever is not taken.
pub fn unique_name(base: &str, taken: impl Fn(&str) -> bool) -> String {
  if !taken(base) {
    return base.to_string();
  }
  for n in 2..=u64::MAX {
    let candidate = format!("{base}-{n}");
    if !taken(&candidate) {
      return candidate;
    }
  }
  base.to_string()
}

#[must_use]
pub fn file_name_string(path: &Path) -> String {
  path
    .file_name()
    .map(|n| n.to_string_lossy().into_owned())
    .unwrap_or_default()
}

/// Returns `path` if it is absolute, otherwise `base/path`.
#[must_use]
pub fn absolutize(base: &Path, path: &Path) -> PathBuf {
  if path.is_absolute() {
    path.to_path_buf()
  } else {
    base.join(path)
  }
}

#[cfg(test)]
#[expect(
  clippy::unwrap_used,
  reason = "test setup and assertions intentionally fail on error"
)]
mod tests {
  use super::*;

  #[test]
  fn slugify_collapses_and_trims() {
    assert_eq!(slugify("  My Cool  Instance!! "), "my-cool-instance");
    assert_eq!(slugify("1.21 Modded"), "1-21-modded");
    assert_eq!(slugify("___"), "");
    assert_eq!(slugify("Größe"), "gr-e");
  }

  #[test]
  fn sanitize_blocks_traversal() {
    assert_eq!(sanitize_file_name("../../etc/passwd"), "passwd");
    assert_eq!(sanitize_file_name("..\\evil.zip"), "evil.zip");
    assert_eq!(sanitize_file_name("a:b?.zip"), "a_b_.zip");
    assert_eq!(sanitize_file_name(".."), "download");
  }

  #[test]
  fn unique_name_appends_counter() {
    let taken = ["a", "a-2"];
    assert_eq!(unique_name("a", |c| taken.contains(&c)), "a-3");
    assert_eq!(unique_name("b", |c| taken.contains(&c)), "b");
  }

  #[test]
  fn atomic_write_replaces_and_cleans_up() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("f.txt");
    write_atomic(&path, b"old").unwrap();
    write_atomic(&path, b"new").unwrap();
    assert_eq!(fs::read(&path).unwrap(), b"new");
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
  }

  #[cfg(unix)]
  #[test]
  fn mode_is_applied_at_creation() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("secret");
    write_atomic_with_mode(&path, b"x", Some(0o600)).unwrap();
    let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
  }

  #[test]
  fn corrupt_toml_is_reported_not_replaced() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("x.toml");
    fs::write(&path, "this is = = not toml").unwrap();
    let err = update_toml::<toml::Table, ()>(&path, |_| Ok(())).unwrap_err();
    assert!(matches!(err, Error::Corrupt { .. }));
    assert_eq!(fs::read_to_string(&path).unwrap(), "this is = = not toml");
  }

  #[test]
  fn copy_dir_honours_skip() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("src");
    fs::create_dir_all(src.join("keep/inner")).unwrap();
    fs::create_dir_all(src.join("Logs")).unwrap();
    fs::write(src.join("keep/inner/a"), "a").unwrap();
    fs::write(src.join("Logs/l"), "l").unwrap();
    let dst = dir.path().join("dst");
    copy_dir(&src, &dst, &|rel| rel.starts_with("Logs")).unwrap();
    assert!(dst.join("keep/inner/a").exists());
    assert!(!dst.join("Logs").exists());
  }

  #[test]
  fn move_does_not_merge_directories_when_rename_fails() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("src");
    let dst = dir.path().join("dst");
    fs::create_dir(&src).unwrap();
    fs::create_dir(&dst).unwrap();
    fs::write(src.join("source"), "keep").unwrap();
    fs::write(dst.join("existing"), "keep").unwrap();

    assert!(move_path(&src, &dst).is_err());
    assert!(src.join("source").exists());
    assert!(!dst.join("source").exists());
    assert!(dst.join("existing").exists());
  }
}
