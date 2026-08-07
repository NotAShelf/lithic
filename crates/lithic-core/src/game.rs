//! Game builds: the official release list, installing builds, and the
//! registry of builds instances can launch with.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{Error, IoContext, Kind, Result};
use crate::fsutil::{self, now_ms};
use crate::http::Download;
use crate::progress::{Cancel, Reporter, Step};
use crate::{Freshness, Lithic, version};

pub const MANIFEST_URL: &str = "https://api.vintagestory.at/stable-unstable.json";
const MANIFEST_MAX_AGE_MS: i64 = 6 * 3_600_000;

/// Artifact kinds in the release list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Platform {
   #[serde(rename = "linux")]
   Linux,
   #[serde(rename = "windows")]
   Windows,
   #[serde(rename = "mac-x64")]
   MacX64,
   #[serde(rename = "mac-arm64")]
   MacArm64,
   #[serde(rename = "linuxserver")]
   LinuxServer,
   #[serde(rename = "windowsserver")]
   WindowsServer,
}

impl Platform {
   pub const ALL: [Platform; 6] = [
      Platform::Linux,
      Platform::Windows,
      Platform::MacX64,
      Platform::MacArm64,
      Platform::LinuxServer,
      Platform::WindowsServer,
   ];

   pub fn key(self) -> &'static str {
      match self {
         Platform::Linux => "linux",
         Platform::Windows => "windows",
         Platform::MacX64 => "mac-x64",
         Platform::MacArm64 => "mac-arm64",
         Platform::LinuxServer => "linuxserver",
         Platform::WindowsServer => "windowsserver",
      }
   }

   pub fn from_key(key: &str) -> Option<Self> {
      Self::ALL.into_iter().find(|p| p.key() == key)
   }

   /// The client build for the machine lithic runs on.
   pub fn host_client() -> Option<Self> {
      match (std::env::consts::OS, std::env::consts::ARCH) {
         ("linux", _) => Some(Platform::Linux),
         ("windows", _) => Some(Platform::Windows),
         ("macos", "aarch64") => Some(Platform::MacArm64),
         ("macos", _) => Some(Platform::MacX64),
         _ => None,
      }
   }
}

impl std::fmt::Display for Platform {
   fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
      f.write_str(self.key())
   }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Artifact {
   pub filename: String,
   pub url: String,
   pub md5: Option<String>,
   /// As published, e.g. `590.5 MB`.
   pub size: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Release {
   pub version: String,
   pub artifacts: BTreeMap<Platform, Artifact>,
}

impl Release {
   pub fn is_prerelease(&self) -> bool {
      version::is_prerelease(&self.version)
   }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Manifest {
   pub fetched_at: i64,
   /// Newest first.
   pub releases: Vec<Release>,
}

impl Manifest {
   pub fn parse(json: &str) -> Result<Self> {
      let root: BTreeMap<String, Value> =
         serde_json::from_str(json).map_err(|e| Error::parse("game release list", e))?;
      let mut releases: Vec<Release> = root
         .into_iter()
         .map(|(version, entry)| {
            let artifacts = entry
               .as_object()
               .map(|o| {
                  o.iter()
                     .filter_map(|(key, a)| {
                        let platform = Platform::from_key(key)?;
                        let url = a
                           .pointer("/urls/cdn")
                           .or_else(|| a.pointer("/urls/local"))?
                           .as_str()?;
                        Some((
                           platform,
                           Artifact {
                              filename: a.get("filename")?.as_str()?.to_string(),
                              url: url.to_string(),
                              md5: a.get("md5").and_then(Value::as_str).map(ToString::to_string),
                              size: a.get("filesize").and_then(Value::as_str).map(ToString::to_string),
                           },
                        ))
                     })
                     .collect()
               })
               .unwrap_or_default();
            Release { version, artifacts }
         })
         .collect();
      releases.sort_by(|a, b| version::compare(&b.version, &a.version));
      Ok(Self {
         fetched_at: now_ms(),
         releases,
      })
   }

   pub fn release(&self, version: &str) -> Option<&Release> {
      let v = version.trim().trim_start_matches(['v', 'V']);
      self.releases.iter().find(|r| r.version.eq_ignore_ascii_case(v))
   }

   pub fn latest_stable(&self) -> Option<&Release> {
      self.releases.iter().find(|r| !r.is_prerelease())
   }
}

/// A game build lithic can launch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Install {
   pub version: String,
   pub path: PathBuf,
   /// Installed by lithic, as opposed to an existing install the user added.
   /// Only managed installs have their files deleted on removal.
   #[serde(default)]
   pub managed: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Registry {
   #[serde(default, rename = "install")]
   installs: Vec<Install>,
}

/// The executable inside a game directory, and any arguments needed before
/// the game's own. Builds before 1.18 on Linux and macOS run under mono.
pub fn find_executable(dir: &Path) -> Option<(PathBuf, Vec<String>)> {
   if cfg!(windows) {
      let exe = dir.join("Vintagestory.exe");
      return exe.is_file().then(|| (exe, Vec::new()));
   }
   let native = dir.join("Vintagestory");
   if native.is_file() {
      return Some((native, Vec::new()));
   }
   let exe = dir.join("Vintagestory.exe");
   exe.is_file()
      .then(|| (PathBuf::from("mono"), vec![exe.to_string_lossy().into_owned()]))
}

/// `dir` itself or its only relevant child (`vintagestory/`,
/// `Vintage Story.app/`) if that is where the executable is.
pub fn locate_game_dir(dir: &Path) -> Option<PathBuf> {
   if find_executable(dir).is_some() {
      return Some(dir.to_path_buf());
   }
   fs::read_dir(dir)
      .ok()?
      .flatten()
      .map(|e| e.path())
      .find(|p| p.is_dir() && find_executable(p).is_some())
}

impl Lithic {
   pub async fn game_manifest(&self, freshness: Freshness) -> Result<Manifest> {
      let path = self.paths.game_manifest_file();
      let cached: Option<Manifest> = fsutil::read_json(&path).ok().flatten();
      if freshness == Freshness::Cached
         && let Some(m) = &cached
         && now_ms() - m.fetched_at < MANIFEST_MAX_AGE_MS
      {
         return Ok(m.clone());
      }
      match self
         .http
         .get_text(MANIFEST_URL)
         .await
         .and_then(|t| Manifest::parse(&t))
      {
         Ok(manifest) => {
            if let Err(e) = fsutil::write_json(&path, &manifest) {
               tracing::warn!("could not cache the game release list: {e}");
            }
            Ok(manifest)
         }
         Err(e) => cached.ok_or(e),
      }
   }

   pub fn game_installs(&self) -> Result<Vec<Install>> {
      let registry: Registry = fsutil::read_toml(&self.paths.game_registry_file())?.unwrap_or_default();
      let mut installs = registry.installs;
      installs.sort_by(|a, b| version::compare(&b.version, &a.version));
      Ok(installs)
   }

   pub fn game_install(&self, version: &str) -> Result<Install> {
      let v = version.trim().trim_start_matches(['v', 'V']);
      self
         .game_installs()?
         .into_iter()
         .find(|i| i.version.eq_ignore_ascii_case(v))
         .ok_or_else(|| Error::not_found(Kind::GameVersion, v))
   }

   /// Registers an existing game directory under `version`. Replaces an
   /// earlier registration of the same version.
   pub fn add_game_install(&self, version: &str, path: &Path) -> Result<Install> {
      let version = version.trim().trim_start_matches(['v', 'V']).to_string();
      if version.is_empty() {
         return Err(Error::invalid("a game version is required"));
      }
      let path = crate::paths::expand_home(path);
      let dir = locate_game_dir(&path)
         .ok_or_else(|| Error::invalid(format!("no Vintage Story executable found in {}", path.display())))?;
      let install = Install {
         version,
         path: dir,
         managed: false,
      };
      self.register(install.clone())?;
      Ok(install)
   }

   /// Unregisters a build. Files are deleted only for builds lithic
   /// installed itself. Refuses while an instance uses the version.
   pub fn remove_game_install(&self, version: &str) -> Result<()> {
      let install = self.game_install(version)?;
      let users: Vec<String> = self
         .list_instances()?
         .instances
         .into_iter()
         .filter(|i| {
            i.game_version
               .as_deref()
               .is_some_and(|v| v.eq_ignore_ascii_case(&install.version))
         })
         .map(|i| i.name)
         .collect();
      if !users.is_empty() {
         return Err(Error::InUse {
            what: format!("game version {}", install.version),
            users,
         });
      }
      fsutil::update_toml(&self.paths.game_registry_file(), |r: &mut Registry| {
         r.installs
            .retain(|i| !i.version.eq_ignore_ascii_case(&install.version));
         Ok(())
      })?;
      if install.managed {
         let root = self.game_root()?;
         match install
            .path
            .ancestors()
            .find(|p| p.parent() == Some(root.as_path()))
         {
            Some(top) => fsutil::remove_path(top)?,
            None => {
               // Installed by lithic 1.x into `<data>/game-versions/<id>/`.
               fsutil::remove_path(&install.path)?;
               if let Some(parent) = install.path.parent()
                  && fs::read_dir(parent).is_ok_and(|mut d| d.next().is_none())
               {
                  let _ = fs::remove_dir(parent);
               }
            }
         }
      }
      Ok(())
   }

   /// Downloads, verifies and unpacks a client build for this machine.
   pub async fn install_game(&self, version: &str, reporter: &Reporter, cancel: &Cancel) -> Result<Install> {
      let platform = Platform::host_client()
         .ok_or_else(|| Error::Unsupported("there are no game builds for this operating system".into()))?;
      let manifest = self.game_manifest(Freshness::Cached).await?;
      let manifest = match manifest.release(version) {
         Some(_) => manifest,
         None => self.game_manifest(Freshness::Refresh).await?,
      };
      let release = manifest
         .release(version)
         .ok_or_else(|| Error::not_found(Kind::GameVersion, version))?;
      let artifact = release
         .artifacts
         .get(&platform)
         .ok_or_else(|| Error::Unsupported(format!("version {} has no {platform} build", release.version)))?;

      let root = self.game_root()?;
      let target = root.join(fsutil::sanitize_file_name(&release.version));
      if target.exists() {
         return Err(Error::AlreadyExists {
            kind: Kind::GameVersion,
            id: release.version.clone(),
         });
      }

      reporter.step(Step::Downloading);
      let archive = self.paths.downloads_dir().join(&artifact.filename);
      self
         .http
         .download(
            &artifact.url,
            &archive,
            Download {
               label: &artifact.filename,
               md5: artifact.md5.as_deref(),
               reporter,
               cancel,
            },
         )
         .await?;
      reporter.log(format!("verified {}", artifact.filename));

      // The Windows installer records its directory for its uninstaller, so it
      // installs straight into the final place instead of being renamed.
      let work = match platform {
         Platform::Windows => target.clone(),
         _ => root.join(format!(
            ".{}.partial",
            fsutil::sanitize_file_name(&release.version)
         )),
      };
      let _ = fsutil::remove_path(&work);
      let result = async {
         cancel.check()?;
         match platform {
            Platform::Windows => {
               reporter.step(Step::Installing);
               run_windows_installer(&archive, &work).await?;
            }
            _ => {
               reporter.step(Step::Extracting);
               let (a, w) = (archive.clone(), work.clone());
               tokio::task::spawn_blocking(move || extract_tar_gz(&a, &w))
                  .await
                  .map_err(|e| Error::invalid(format!("extraction failed: {e}")))??;
            }
         }
         let game_dir = locate_game_dir(&work).ok_or_else(|| {
            Error::invalid("the downloaded build does not contain a Vintage Story executable")
         })?;
         let relative = game_dir
            .strip_prefix(&work)
            .unwrap_or(Path::new(""))
            .to_path_buf();
         if work != target {
            fs::rename(&work, &target).at(&target)?;
         }
         Ok(target.join(relative))
      }
      .await;
      let _ = fsutil::remove_path(&archive);
      let path = match result {
         Ok(path) => path,
         Err(e) => {
            let _ = fsutil::remove_path(&work);
            return Err(e);
         }
      };

      let install = Install {
         version: release.version.clone(),
         path,
         managed: true,
      };
      self.register(install.clone())?;
      Ok(install)
   }

   /// Downloads any artifact from the release list into `dir` without
   /// installing it, for example a server build.
   pub async fn download_game(
      &self,
      version: &str,
      platform: Platform,
      dir: &Path,
      reporter: &Reporter,
      cancel: &Cancel,
   ) -> Result<PathBuf> {
      let manifest = self.game_manifest(Freshness::Cached).await?;
      let release = manifest
         .release(version)
         .ok_or_else(|| Error::not_found(Kind::GameVersion, version))?;
      let artifact = release
         .artifacts
         .get(&platform)
         .ok_or_else(|| Error::Unsupported(format!("version {} has no {platform} build", release.version)))?;
      let dest = dir.join(fsutil::sanitize_file_name(&artifact.filename));
      self
         .http
         .download(
            &artifact.url,
            &dest,
            Download {
               label: &artifact.filename,
               md5: artifact.md5.as_deref(),
               reporter,
               cancel,
            },
         )
         .await?;
      Ok(dest)
   }

   pub(crate) fn game_root(&self) -> Result<PathBuf> {
      Ok(self
         .settings()?
         .game
         .install_dir
         .map(crate::paths::expand_home)
         .unwrap_or_else(|| self.paths.game_dir()))
   }

   pub(crate) fn register(&self, install: Install) -> Result<()> {
      fsutil::update_toml(&self.paths.game_registry_file(), |r: &mut Registry| {
         r.installs
            .retain(|i| !i.version.eq_ignore_ascii_case(&install.version));
         r.installs.push(install);
         Ok(())
      })
   }
}

fn extract_tar_gz(archive: &Path, dest: &Path) -> Result<()> {
   fs::create_dir_all(dest).at(dest)?;
   let file = fs::File::open(archive).at(archive)?;
   let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(file));
   tar.set_preserve_permissions(true);
   tar.set_overwrite(true);
   for entry in tar.entries().at(archive)? {
      let mut entry = entry.at(archive)?;
      // unpack_in refuses entries that would land outside `dest`.
      entry.unpack_in(dest).at(dest)?;
   }
   Ok(())
}

async fn run_windows_installer(installer: &Path, dest: &Path) -> Result<()> {
   fs::create_dir_all(dest).at(dest)?;
   // The game ships an Inno Setup installer. These switches install without
   // any prompts, shortcuts or file associations into `dest`, leaving any
   // stock installation alone.
   let status = tokio::process::Command::new(installer)
      .arg("/VERYSILENT")
      .arg("/SUPPRESSMSGBOXES")
      .arg("/NORESTART")
      .arg("/SP-")
      .arg("/NOICONS")
      .arg("/CURRENTUSER")
      .arg("/TASKS=")
      .arg(format!("/DIR={}", dest.display()))
      .status()
      .await
      .at(installer)?;
   if status.success() {
      Ok(())
   } else {
      Err(Error::invalid(format!("the game installer exited with {status}")))
   }
}

#[cfg(test)]
mod tests {
   use super::*;

   const MANIFEST: &str = r#"{
      "1.22.7": {
         "linux": {"filename": "vs_client_linux-x64_1.22.7.tar.gz", "filesize": "590.5 MB",
                   "md5": "ecfa56411b0a2912ebba29839fa98a3f",
                   "urls": {"cdn": "https://cdn.vintagestory.at/gamefiles/stable/vs_client_linux-x64_1.22.7.tar.gz",
                            "local": "https://account.vintagestory.at/files/stable/vs_client_linux-x64_1.22.7.tar.gz"},
                   "latest": 1},
         "mac-arm64": {"filename": "vs_client_osx-arm64_1.22.7.tar.gz", "md5": "aa",
                       "urls": {"cdn": "https://cdn/vs_client_osx-arm64_1.22.7.tar.gz"}},
         "server": {"filename": "ignored", "urls": {"cdn": "x"}}
      },
      "1.22.0-rc.10": {"linux": {"filename": "rc.tar.gz", "urls": {"local": "https://local/rc.tar.gz"}}},
      "1.9.14": {"linux": {"filename": "vs_archive_1.9.14.tar.gz", "urls": {"cdn": "https://cdn/vs_archive_1.9.14.tar.gz"}}}
   }"#;

   #[test]
   fn manifest_parsing_and_order() {
      let m = Manifest::parse(MANIFEST).unwrap();
      let versions: Vec<&str> = m.releases.iter().map(|r| r.version.as_str()).collect();
      assert_eq!(versions, ["1.22.7", "1.22.0-rc.10", "1.9.14"]);
      assert_eq!(m.latest_stable().unwrap().version, "1.22.7");
      let r = m.release("v1.22.7").unwrap();
      assert_eq!(r.artifacts.len(), 2);
      let linux = &r.artifacts[&Platform::Linux];
      assert_eq!(linux.md5.as_deref(), Some("ecfa56411b0a2912ebba29839fa98a3f"));
      assert!(linux.url.starts_with("https://cdn."));
      assert_eq!(
         m.release("1.22.0-rc.10").unwrap().artifacts[&Platform::Linux].url,
         "https://local/rc.tar.gz"
      );
      assert!(m.release("1.22.0-rc.10").unwrap().is_prerelease());
   }

   #[test]
   fn tarball_extracts_and_is_located() {
      let d = tempfile::tempdir().unwrap();
      let archive = d.path().join("vs.tar.gz");
      {
         let gz =
            flate2::write::GzEncoder::new(fs::File::create(&archive).unwrap(), flate2::Compression::fast());
         let mut b = tar::Builder::new(gz);
         let mut header = tar::Header::new_gnu();
         header.set_size(4);
         header.set_mode(0o755);
         header.set_cksum();
         b.append_data(&mut header, "vintagestory/Vintagestory", &b"#!sh"[..])
            .unwrap();
         b.into_inner().unwrap().finish().unwrap();
      }
      let dest = d.path().join("out");
      extract_tar_gz(&archive, &dest).unwrap();
      assert_eq!(locate_game_dir(&dest), Some(dest.join("vintagestory")));
   }

   #[test]
   fn registry_and_removal_rules() {
      let d = tempfile::tempdir().unwrap();
      let l = Lithic::new(crate::Paths::rooted(d.path())).unwrap();
      let external = d.path().join("ext/vintagestory");
      fs::create_dir_all(&external).unwrap();
      fs::write(
         external.join(if cfg!(windows) {
            "Vintagestory.exe"
         } else {
            "Vintagestory"
         }),
         "",
      )
      .unwrap();

      let added = l.add_game_install("v1.21.5", &d.path().join("ext")).unwrap();
      assert_eq!(added.version, "1.21.5");
      assert_eq!(added.path, external);
      assert!(!added.managed);
      assert!(l.add_game_install("1.21.6", d.path()).is_err());

      l.create_instance(crate::instance::NewInstance {
         name: "uses it".into(),
         game_version: Some("1.21.5".into()),
         ..Default::default()
      })
      .unwrap();
      assert!(matches!(
         l.remove_game_install("1.21.5"),
         Err(Error::InUse { .. })
      ));
      l.delete_instance("uses-it").unwrap();
      l.remove_game_install("1.21.5").unwrap();
      assert!(external.is_dir(), "external installs are never deleted");
      assert!(l.game_installs().unwrap().is_empty());
   }
}
