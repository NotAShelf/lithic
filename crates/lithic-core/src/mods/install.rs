//! Installing and updating mods.
//!
//! Everything is downloaded into a staging directory first. Each download is
//! opened and its `modinfo.json` read, which both verifies the archive and
//! reveals its dependencies; those are resolved the same way until nothing new
//! turns up. Only then are files moved into the instance, old versions backed
//! up and removed, and `mods.json` updated. A failure while resolving leaves
//! the instance untouched.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::{fs, mem};

use futures::stream::{self, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::task::spawn_blocking;

use super::resolve::{Target, pick_release};
use super::{InstalledMod, LockEntry, ModLock, ModRef, OperationLock, free_path};
use crate::error::{Error, Result};
use crate::fsutil::{self, now_ms};
use crate::http::Download;
use crate::instance::Instance;
use crate::moddb::{ModDetails, Release};
use crate::modinfo::{self, ModInfo};
use crate::progress::{Cancel, Reporter, Step};
use crate::{Lithic, version};

#[derive(Debug, Clone, Default)]
pub struct InstallOptions {
   /// Also install missing dependencies. On by default in the frontends.
   pub dependencies: bool,
   /// Download again even if the chosen release is already installed.
   pub reinstall: bool,
   /// Keep mods requested at a specific version on that version.
   pub pin_versions: bool,
   pub reporter: Reporter,
   pub cancel: Cancel,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Reason {
   Requested,
   Dependency { of: String },
   Update,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Change {
   pub mod_id: String,
   pub name: String,
   pub from: Option<String>,
   pub to: String,
   pub file: String,
   pub reason: Reason,
}

#[derive(Debug)]
pub struct Failure {
   pub mod_id: String,
   /// Set when the mod was pulled in by another one.
   pub needed_by: Option<String>,
   pub error: Error,
}

#[derive(Debug, Default)]
pub struct Report {
   pub changes: Vec<Change>,
   /// Requested mods that were already installed at the chosen version.
   pub unchanged: Vec<String>,
   pub failures: Vec<Failure>,
   /// Dependencies whose required version is newer than anything that fits
   /// the instance's game version.
   pub unmet: Vec<(String, String, String)>,
}

impl Report {
   #[must_use]
   pub const fn is_success(&self) -> bool {
      self.failures.is_empty()
   }
}

/// An available update for an installed mod.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Update {
   pub mod_id: String,
   pub name: String,
   pub installed: String,
   pub moddb_id: i64,
   pub release: Release,
   /// A pinned mod whose installed version differs from its pin shows up
   /// here too, even when that means going back.
   pub to_pin: bool,
}

#[derive(Debug, Clone)]
struct Job {
   id: String,
   pin: Option<String>,
   reason: Reason,
   /// Set by `update_mods`, which already knows the release.
   resolved: Option<(i64, String, Release)>,
}

#[derive(Debug)]
struct Staged {
   mod_id: String,
   name: String,
   moddb_id: i64,
   release: Release,
   path: PathBuf,
   info: ModInfo,
   reason: Reason,
   pin: Option<String>,
}

struct StageContext<'a> {
   installed: &'a [InstalledMod],
   lock: &'a ModLock,
   target: &'a Target,
   staging: &'a Path,
   opts: &'a InstallOptions,
}

impl Lithic {
   /// # Errors
   /// Returns an error if the instance is busy, its settings cannot be read, or installation is cancelled.
   pub async fn install_mods(
      &self,
      instance: &Instance,
      refs: &[ModRef],
      opts: &InstallOptions,
   ) -> Result<Report> {
      let jobs = refs
         .iter()
         .map(|r| Job {
            id: r.id.clone(),
            pin: r.version.clone(),
            reason: Reason::Requested,
            resolved: None,
         })
         .collect();
      self.run_jobs(instance, jobs, opts).await
   }

   /// Looks up every installed mod on the `ModDB` and returns those with a
   /// newer compatible release. Mods not on the `ModDB` are skipped.
   /// # Errors
   /// Returns an error if settings or installed mods cannot be read, or cancellation is requested.
   pub async fn check_updates(&self, instance: &Instance, cancel: &Cancel) -> Result<Vec<Update>> {
      let settings = self.settings()?;
      let target = Target {
         game_version: instance.game_version.clone(),
         allow_prerelease: settings.mods.allow_prerelease,
      };
      let mut seen = BTreeSet::new();
      let candidates: Vec<InstalledMod> = self
         .installed_mods(instance)?
         .into_iter()
         .filter(|m| m.info.has_metadata && seen.insert(m.mod_id().to_string()))
         .collect();

      let results: Vec<Result<Option<Update>>> = stream::iter(candidates)
         .map(|m| {
            let target = target.clone();
            async move {
               cancel.check()?;
               let lookup = m
                  .lock
                  .moddb_id
                  .map_or_else(|| m.mod_id().to_string(), |id| id.to_string());
               let details = match self.moddb.mod_details(&lookup).await {
                  Ok(d) => d,
                  Err(Error::NotFound { .. }) => return Ok(None),
                  Err(e) => return Err(e),
               };
               let pin = m.lock.pin.as_deref();
               let Ok(release) = pick_release(&details, &target, pin) else {
                  return Ok(None);
               };
               let latest = release.version.clone().unwrap_or_default();
               let installed = &m.info.version;
               let wanted = match pin {
                  Some(_) => !version::compare(&latest, installed).is_eq(),
                  None => version::compare(&latest, installed).is_gt(),
               };
               Ok(wanted.then(|| Update {
                  mod_id: m.mod_id().to_string(),
                  name: m.display_name().to_string(),
                  installed: installed.clone(),
                  moddb_id: details.id,
                  release: release.clone(),
                  to_pin: pin.is_some(),
               }))
            }
         })
         .buffer_unordered(settings.mods.concurrency.max(1))
         .collect()
         .await;

      let mut updates = Vec::new();
      for result in results {
         match result {
            Ok(Some(u)) => updates.push(u),
            Ok(None) => {}
            Err(Error::Cancelled) => return Err(Error::Cancelled),
            Err(e) => tracing::warn!("update check failed for one mod: {e}"),
         }
      }
      updates.sort_by_key(|u| u.name.to_lowercase());
      Ok(updates)
   }

   /// Applies updates, typically the result of [`Lithic::check_updates`],
   /// possibly filtered by the user.
   /// # Errors
   /// Returns an error if the instance is busy, its settings cannot be read, or installation is cancelled.
   pub async fn update_mods(
      &self,
      instance: &Instance,
      updates: &[Update],
      opts: &InstallOptions,
   ) -> Result<Report> {
      let jobs = updates
         .iter()
         .map(|u| Job {
            id: u.mod_id.clone(),
            pin: None,
            reason: Reason::Update,
            resolved: Some((u.moddb_id, u.name.clone(), u.release.clone())),
         })
         .collect();
      let opts = InstallOptions {
         reinstall: true,
         ..opts.clone()
      };
      self.run_jobs(instance, jobs, &opts).await
   }

   async fn run_jobs(&self, instance: &Instance, jobs: Vec<Job>, opts: &InstallOptions) -> Result<Report> {
      let _op = OperationLock::try_acquire(instance)?;
      let settings = self.settings()?;
      let target = Target {
         game_version: instance.game_version.clone(),
         allow_prerelease: settings.mods.allow_prerelease,
      };
      let installed = self.installed_mods(instance)?;
      let lock = self.mod_lock(instance)?;
      let staging = self
         .paths
         .downloads_dir()
         .join(format!("{}-{}", instance.id, now_ms()));

      let context = StageContext {
         installed: &installed,
         lock: &lock,
         target: &target,
         staging: &staging,
         opts,
      };
      let result = self
         .resolve_and_stage(jobs, &context, settings.mods.concurrency)
         .await;
      let result = match result {
         Ok((staged, mut report)) => {
            opts.cancel.check()?;
            opts.reporter.step(Step::Installing);
            self
               .apply(instance, &installed, staged, &mut report)
               .map(|()| report)
         }
         Err(e) => Err(e),
      };
      opts.reporter.step(Step::Cleaning);
      let _ = fsutil::remove_path(&staging);
      result
   }

   async fn resolve_and_stage(
      &self,
      jobs: Vec<Job>,
      context: &StageContext<'_>,
      concurrency: usize,
   ) -> Result<(Vec<Staged>, Report)> {
      let StageContext { installed, opts, .. } = *context;
      let mut report = Report::default();
      let mut staged: BTreeMap<String, Staged> = BTreeMap::new();
      let mut attempted: BTreeSet<String> = BTreeSet::new();
      let mut queue = jobs;

      let installed_version = |id: &str| -> Option<&InstalledMod> {
         installed
            .iter()
            .find(|m| m.enabled && m.mod_id() == id)
            .or_else(|| installed.iter().find(|m| m.mod_id() == id))
      };

      while !queue.is_empty() {
         opts.cancel.check()?;
         opts.reporter.step(Step::Resolving);
         let wave: Vec<Job> = mem::take(&mut queue)
            .into_iter()
            .filter(|j| attempted.insert(j.id.to_ascii_lowercase()))
            .collect();

         let results: Vec<(Job, Result<Option<Staged>>)> = stream::iter(wave)
            .map(|job| async move {
               let result = self.stage_one(&job, context).await;
               (job, result)
            })
            .buffer_unordered(concurrency.max(1))
            .collect()
            .await;

         for (job, result) in results {
            match result {
               Err(Error::Cancelled) => return Err(Error::Cancelled),
               Err(error) => report.failures.push(Failure {
                  mod_id: job.id.clone(),
                  needed_by: match &job.reason {
                     Reason::Dependency { of } => Some(of.clone()),
                     _ => None,
                  },
                  error,
               }),
               Ok(None) => {
                  if job.reason == Reason::Requested {
                     report.unchanged.push(job.id.clone());
                  }
               }
               Ok(Some(s)) => {
                  attempted.insert(s.mod_id.clone());
                  let dependencies = opts.dependencies.then(|| s.info.mod_dependencies());
                  for (dep, required) in dependencies.into_iter().flatten() {
                     if attempted.contains(dep) || staged.contains_key(dep) {
                        continue;
                     }
                     let have = installed_version(dep);
                     if have.is_some_and(|m| version::satisfies(&m.info.version, required)) {
                        continue;
                     }
                     queue.push(Job {
                        id: dep.clone(),
                        pin: None,
                        reason: Reason::Dependency { of: s.mod_id.clone() },
                        resolved: None,
                     });
                  }
                  staged.insert(s.mod_id.clone(), s);
               }
            }
         }
      }

      for s in staged.values() {
         for (dep, required) in s.info.mod_dependencies() {
            let have = staged
               .get(dep)
               .map(|d| d.info.version.as_str())
               .or_else(|| installed_version(dep).map(|m| m.info.version.as_str()));
            if let Some(have) = have
               && !version::satisfies(have, required)
            {
               report
                  .unmet
                  .push((s.mod_id.clone(), dep.clone(), required.clone()));
            }
         }
      }

      Ok((staged.into_values().collect(), report))
   }

   async fn stage_one(&self, job: &Job, context: &StageContext<'_>) -> Result<Option<Staged>> {
      let StageContext {
         installed,
         lock,
         target,
         staging,
         opts,
      } = *context;
      opts.cancel.check()?;
      let key = job.id.to_ascii_lowercase();
      let (moddb_id, name, release, details_mod_id) = if let Some((id, name, release)) = &job.resolved {
         (*id, name.clone(), release.clone(), release.mod_id.clone())
      } else {
         let details: ModDetails = self.moddb.mod_details(&job.id).await?;
         let lock_pin = lock
            .mods
            .get(&key)
            .or_else(|| {
               details
                  .mod_id()
                  .and_then(|m| lock.mods.get(&m.to_ascii_lowercase()))
            })
            .and_then(|e| e.pin.clone());
         let pin = job.pin.clone().or(lock_pin);
         let release = pick_release(&details, target, pin.as_deref())?.clone();
         (
            details.id,
            details.name.clone(),
            release,
            details.mod_id().map(ToString::to_string),
         )
      };

      let mod_id = release
         .mod_id
         .clone()
         .or(details_mod_id)
         .unwrap_or_else(|| key.clone())
         .to_ascii_lowercase();
      let new_version = release.version.clone().unwrap_or_default();

      if !opts.reinstall
         && let Some(current) = installed.iter().find(|m| m.mod_id() == mod_id)
         && version::compare(&current.info.version, &new_version).is_eq()
      {
         return Ok(None);
      }

      let url = release
         .url
         .clone()
         .ok_or_else(|| Error::invalid(format!("release {new_version} of {mod_id} has no download")))?;
      let dir = staging.join(fsutil::sanitize_file_name(&mod_id));
      let path = dir.join(release.file_name());
      opts.reporter.step(Step::Downloading);
      self
         .http
         .download(
            &url,
            &path,
            Download {
               label: &format!("{mod_id} {new_version}"),
               md5: None,
               reporter: &opts.reporter,
               cancel: &opts.cancel,
            },
         )
         .await?;

      let read_path = path.clone();
      let info = spawn_blocking(move || modinfo::read(&read_path))
         .await
         .map_err(|e| Error::invalid(format!("reading {mod_id} failed: {e}")))??;
      if info.has_metadata && info.mod_id != mod_id {
         tracing::warn!(
            "{mod_id}: the ModDB lists this mod as `{mod_id}` but its modinfo.json says `{}`",
            info.mod_id
         );
      }
      opts.reporter.log(format!("fetched {mod_id} {new_version}"));

      Ok(Some(Staged {
         mod_id: if info.has_metadata {
            info.mod_id.clone()
         } else {
            mod_id
         },
         name,
         moddb_id,
         release,
         path,
         info,
         reason: job.reason.clone(),
         pin: job.pin.clone().filter(|_| opts.pin_versions),
      }))
   }

   fn apply(
      &self,
      instance: &Instance,
      installed: &[InstalledMod],
      staged: Vec<Staged>,
      report: &mut Report,
   ) -> Result<()> {
      let backups = self.settings()?.backups.enabled;

      for s in staged {
         let mod_id = s.mod_id.clone();
         if let Err(error) = self.apply_one(instance, installed, &s, backups, report) {
            report.failures.push(Failure {
               mod_id,
               needed_by: None,
               error,
            });
         }
      }
      Ok(())
   }

   /// Replaces one mod. The new file is moved next to the old one under a
   /// temporary name before anything is deleted, so a failure never leaves
   /// the instance without the mod. The lock is updated right away, so it
   /// matches the disk even if a later mod fails.
   fn apply_one(
      &self,
      instance: &Instance,
      installed: &[InstalledMod],
      s: &Staged,
      backups: bool,
      report: &mut Report,
   ) -> Result<()> {
      let old: Vec<&InstalledMod> = installed.iter().filter(|m| m.mod_id() == s.mod_id).collect();
      let keep_disabled = !old.is_empty() && old.iter().all(|m| !m.enabled);
      let dir = if keep_disabled {
         instance.disabled_mods_dir()
      } else {
         instance.mods_dir()
      };
      let file_name = fsutil::file_name_string(&s.path);
      let incoming = dir.join(format!(".{file_name}.incoming"));
      fsutil::move_path(&s.path, &incoming)?;

      let replaced = (|| {
         for m in &old {
            if backups {
               self.backup_mod(instance, m)?;
            }
            fsutil::remove_path(&m.path)?;
         }
         Ok(())
      })();
      if let Err(e) = replaced {
         let _ = fsutil::remove_path(&incoming);
         return Err(e);
      }
      let dest = free_path(&dir, &file_name);
      fs::rename(&incoming, &dest).map_err(|e| Error::io(&dest, e))?;

      let version = s.release.version.clone().unwrap_or_default();
      report.changes.push(Change {
         mod_id: s.mod_id.clone(),
         name: if s.name.is_empty() {
            s.info.name.clone()
         } else {
            s.name.clone()
         },
         from: old.first().map(|m| m.info.version.clone()),
         to: version.clone(),
         file: fsutil::file_name_string(&dest),
         reason: s.reason.clone(),
      });

      let new = LockEntry {
         file: Some(fsutil::file_name_string(&dest)),
         version: Some(version),
         moddb_id: Some(s.moddb_id),
         release_id: Some(s.release.id),
         pin: s.pin.clone(),
         dependency: matches!(s.reason, Reason::Dependency { .. }),
         installed_at: Some(now_ms()),
      };
      Self::update_mod_lock(instance, |lock| {
         let entry = lock.mods.entry(s.mod_id.clone()).or_default();
         let was_explicit = entry.file.is_some() && !entry.dependency;
         let pin = if s.pin.is_some() {
            new.pin.clone()
         } else {
            entry.pin.take()
         };
         *entry = LockEntry {
            pin,
            dependency: new.dependency && !was_explicit,
            ..new
         };
         Ok(())
      })
   }
}
