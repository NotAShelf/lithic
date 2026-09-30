//! Starting the game for an instance and watching it until it exits.

use std::{
  cmp::Reverse,
  fs,
  io::{ErrorKind, Read, Seek, SeekFrom},
  path::{Path, PathBuf},
  process::Stdio,
  sync::Arc,
  time::{SystemTime, UNIX_EPOCH},
};

use tokio::{process::Command, sync::Notify};

use crate::{
  Lithic,
  auth::Account,
  error::{Error, IoContext, Kind, Result},
  fsutil::{self, now_ms},
  game,
  instance::Instance,
};

const KEPT_LOGS: usize = 10;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchSpec {
  pub program: PathBuf,
  pub args:    Vec<String>,
  pub cwd:     PathBuf,
  pub env:     Vec<(String, String)>,
}

impl LaunchSpec {
  /// Shell-quoted launch command for display.
  #[must_use]
  pub fn command_line(&self) -> String {
    let mut line =
      shell_words::quote(&self.program.to_string_lossy()).into_owned();
    for arg in &self.args {
      line.push(' ');
      line.push_str(&shell_words::quote(arg));
    }
    line
  }
}

/// A running game.
#[derive(Debug, Clone)]
pub struct Session {
  pub instance_id: String,
  pub pid:         Option<u32>,
  pub started_ms:  i64,
  /// Game output (stdout and stderr) for this launch.
  pub log_path:    PathBuf,
  stop:            Arc<Notify>,
}

impl Session {
  /// Asks the game to stop. The waiter returned by [`Lithic::launch`] kills
  /// the process and reports its exit.
  pub fn stop(&self) {
    self.stop.notify_one();
  }
}

#[derive(Debug, Clone)]
pub struct Exit {
  pub code:         Option<i32>,
  pub success:      bool,
  pub stopped:      bool,
  pub duration_ms:  i64,
  pub log_path:     PathBuf,
  /// Last lines of the output, for showing why a launch failed.
  pub tail:         Vec<String>,
  /// The game's crash report, if it wrote one during this session.
  pub crash_report: Option<PathBuf>,
}

impl Lithic {
  /// Builds the launch command without starting the game.
  ///
  /// # Errors
  /// Returns an error if no game version is set, its install cannot be loaded,
  /// or the install contains no game executable.
  pub fn launch_spec(&self, instance: &Instance) -> Result<LaunchSpec> {
    let version = instance.game_version.as_deref().ok_or_else(|| {
      Error::invalid(format!("{} has no game version set", instance.name))
    })?;
    let install = self.game_install(version).map_err(|e| {
      match e {
        Error::NotFound { .. } => {
          Error::NotFound {
            kind: Kind::GameVersion,
            id:   format!(
              "{version} (install it or add an existing install first)"
            ),
          }
        },
        e => e,
      }
    })?;
    let (exe, base_args) =
      game::find_executable(&install.path).ok_or_else(|| {
        Error::invalid(format!(
          "no Vintage Story executable in {}",
          install.path.display()
        ))
      })?;

    let mut args = base_args;
    args.push(format!("--dataPath={}", instance.data_dir().display()));
    if let Some(mods) = &instance.mods_dir {
      args.push(format!("--addModPath={}", mods.display()));
    }
    args.extend(instance.launch.args.iter().cloned());

    let (program, args) = match instance.launch.wrapper.split_first() {
      Some((wrapper, wrapper_args)) => {
        let mut all = wrapper_args.to_vec();
        all.push(exe.to_string_lossy().into_owned());
        all.extend(args);
        (PathBuf::from(wrapper), all)
      },
      None => (exe, args),
    };

    let mut env = Vec::new();
    // The stock run.sh points fontconfig at the bundled configuration.
    let fonts = install.path.join("fonts.conf");
    if fonts.is_file() {
      env.push((
        "FONTCONFIG_FILE".to_string(),
        fonts.to_string_lossy().into_owned(),
      ));
    }
    env.extend(
      instance
        .launch
        .env
        .iter()
        .map(|(k, v)| (k.clone(), v.clone())),
    );

    Ok(LaunchSpec {
      program,
      args,
      cwd: install.path,
      env,
    })
  }

  /// Verifies the instance's game session before starting the game. Returns
  /// the session and a future that resolves when the game exits; drive the
  /// future to have play time recorded.
  ///
  /// # Errors
  /// Returns an error if the launch command cannot be built, the account's
  /// session cannot be verified, or the game cannot be started.
  pub async fn launch(
    &self,
    instance: &Instance,
  ) -> Result<(
    Session,
    impl Future<Output = Result<Exit>> + Send + 'static + use<>,
  )> {
    let spec = self.launch_spec(instance)?;
    self.verify_launch_session(instance).await?;
    self.spawn_game(instance, &spec)
  }

  fn spawn_game(
    &self,
    instance: &Instance,
    spec: &LaunchSpec,
  ) -> Result<(
    Session,
    impl Future<Output = Result<Exit>> + Send + 'static + use<>,
  )> {
    fs::create_dir_all(instance.mods_dir()).at(instance.mods_dir())?;
    let data_dir = instance.data_dir();
    // Record the cleanup and arm the guard before any secret is written, so
    // a failure at any later point cannot leave a session lithic does not
    // keep in the game's settings.
    self.start_game_run(&data_dir)?;
    let planned = self
      .accounts()?
      .for_instance(instance.account.as_deref())
      .cloned();
    if let Some(a) = &planned
      && self.is_transient(&a.uid)
    {
      self.track_session_cleanup(&data_dir, &a.uid)?;
    }
    let mut guard = SessionGuard {
      lithic:   self.clone(),
      data_dir: data_dir.clone(),
      account:  planned,
    };
    guard.account = self.inject_account(instance)?;

    let logs = instance.logs_dir();
    fs::create_dir_all(&logs).at(&logs)?;
    prune_logs(&logs);
    let started_ms = now_ms();
    let log_path = logs.join(format!("launch-{started_ms}.log"));
    let out = fs::File::create(&log_path).at(&log_path)?;
    let err = out.try_clone().at(&log_path)?;

    let mut cmd = Command::new(&spec.program);
    cmd
      .args(&spec.args)
      .current_dir(&spec.cwd)
      .envs(spec.env.iter().map(|(k, v)| (k, v)))
      .stdin(Stdio::null())
      .stdout(out)
      .stderr(err)
      .kill_on_drop(false);
    let mut child = cmd.spawn().map_err(|e| {
      match e.kind() {
        ErrorKind::NotFound => {
          Error::invalid(format!(
            "cannot run {}: not found",
            spec.program.display()
          ))
        },
        _ => Error::io(&spec.program, e),
      }
    })?;

    let stop = Arc::new(Notify::new());
    let session = Session {
      instance_id: instance.id.clone(),
      pid: child.id(),
      started_ms,
      log_path: log_path.clone(),
      stop: Arc::clone(&stop),
    };

    let lithic = self.clone();
    let instance_id = instance.id.clone();
    let game_logs = instance.game_logs_dir();
    let waiter = async move {
      let mut stopped = false;
      let status = tokio::select! {
         status = child.wait() => status,
         () = stop.notified() => {
            stopped = true;
            let _ = child.start_kill();
            child.wait().await
         }
      }
      .at(&log_path)?;
      if let Some(account) = guard.account.take()
        && let Err(e) = lithic.finish_game_session(&data_dir, &account)
      {
        tracing::warn!("could not settle the game login: {e}");
      }
      let ended_ms = now_ms();
      if let Err(e) =
        lithic.record_play_session(&instance_id, started_ms, ended_ms)
      {
        tracing::warn!("could not record play time for {instance_id}: {e}");
      }
      let crash = game_logs.join("client-crash.log");
      let crash_report = fs::metadata(&crash)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .filter(|t| i64::try_from(t.as_millis()).unwrap_or(0) >= started_ms)
        .map(|_| crash);
      Ok(Exit {
        code: status.code(),
        success: status.success(),
        stopped,
        duration_ms: ended_ms - started_ms,
        tail: tail(&log_path, 12),
        log_path,
        crash_report,
      })
    };
    Ok((session, waiter))
  }

  /// Log files for an instance, newest first: lithic's launch logs and the
  /// game's own logs.
  #[must_use]
  pub fn log_files(&self, instance: &Instance) -> Vec<PathBuf> {
    let mut files: Vec<(SystemTime, PathBuf)> =
      [instance.logs_dir(), instance.game_logs_dir()]
        .iter()
        .filter_map(|dir| fs::read_dir(dir).ok())
        .flatten()
        .flatten()
        .filter(|e| {
          e.path()
            .extension()
            .is_some_and(|x| x.eq_ignore_ascii_case("log") || x == "txt")
        })
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    files.sort_by_key(|f| Reverse(f.0));
    files.into_iter().map(|(_, p)| p).collect()
  }
}

/// Reads at most the last `max_bytes` of a file as text.
///
/// # Errors
/// Returns an error if the file cannot be opened, inspected, sought, or read.
pub fn read_tail(path: &Path, max_bytes: u64) -> Result<String> {
  let mut file = fs::File::open(path).at(path)?;
  let len = file.metadata().at(path)?.len();
  let start = len.saturating_sub(max_bytes);
  file.seek(SeekFrom::Start(start)).at(path)?;
  let mut buf = Vec::new();
  file.read_to_end(&mut buf).at(path)?;
  let mut text = String::from_utf8_lossy(&buf).into_owned();
  if start > 0
    && let Some(end) = text.find('\n')
  {
    text.drain(..=end);
  }
  Ok(text)
}

/// The last `lines` non-empty lines of a file.
#[must_use]
pub fn tail(path: &Path, lines: usize) -> Vec<String> {
  let Ok(text) = read_tail(path, 16 * 1024) else {
    return Vec::new();
  };
  let mut out: Vec<String> = text
    .lines()
    .rev()
    .filter(|l| !l.trim().is_empty())
    .take(lines)
    .map(ToString::to_string)
    .collect();
  out.reverse();
  out
}

/// Removes the launch's session from the game settings if lithic does not
/// keep it and nobody saw the game exit: the launch failed, or the waiter
/// was dropped because lithic quit while the game kept running.
struct SessionGuard {
  lithic:   Lithic,
  data_dir: PathBuf,
  /// Taken once the game's exit has been handled.
  account:  Option<Account>,
}

impl Drop for SessionGuard {
  fn drop(&mut self) {
    if let Some(account) = self.account.take()
      && let Err(e) = self.lithic.abandon_game_session(&self.data_dir, &account)
    {
      tracing::warn!("could not remove the game login: {e}");
    }
  }
}

fn prune_logs(dir: &Path) {
  let Ok(entries) = fs::read_dir(dir) else {
    return;
  };
  let mut logs: Vec<PathBuf> = entries
    .flatten()
    .map(|e| e.path())
    .filter(|p| fsutil::file_name_string(p).starts_with("launch-"))
    .collect();
  if logs.len() < KEPT_LOGS {
    return;
  }
  logs.sort();
  for old in &logs[..logs.len() + 1 - KEPT_LOGS] {
    let _ = fs::remove_file(old);
  }
}

#[cfg(test)]
#[expect(
  clippy::unwrap_used,
  reason = "test setup and assertions intentionally fail on error"
)]
mod tests {
  #[cfg(unix)] use std::time::Duration;

  #[cfg(unix)] use tokio::time::timeout;

  use super::*;
  use crate::{Paths, auth::LoginResponse, instance::NewInstance};

  fn setup() -> (tempfile::TempDir, Lithic, Instance) {
    let d = tempfile::tempdir().unwrap();
    let l = Lithic::new(Paths::rooted(d.path())).unwrap();
    let game = d.path().join("game-1.21.5");
    fs::create_dir_all(&game).unwrap();
    let exe = if cfg!(windows) {
      "Vintagestory.exe"
    } else {
      "Vintagestory"
    };
    fs::write(game.join(exe), "").unwrap();
    fs::write(game.join("fonts.conf"), "").unwrap();
    l.add_game_install("1.21.5", &game).unwrap();
    let i = l
      .create_instance(NewInstance {
        name: "play".into(),
        game_version: Some("1.21.5".into()),
        ..Default::default()
      })
      .unwrap();
    (d, l, i)
  }

  #[test]
  fn spec_includes_data_path_args_and_env() {
    let (_d, l, i) = setup();
    l.update_instance(&i.id, |x| {
      x.launch.args = vec!["--connect".into(), "my server:42000".into()];
      x.launch.env.insert("A".into(), "1".into());
      Ok(())
    })
    .unwrap();
    let i = l.instance(&i.id).unwrap();
    let spec = l.launch_spec(&i).unwrap();
    assert!(spec.program.ends_with(if cfg!(windows) {
      "Vintagestory.exe"
    } else {
      "Vintagestory"
    }));
    assert_eq!(
      spec.args[0],
      format!("--dataPath={}", i.data_dir().display())
    );
    assert_eq!(&spec.args[1..], ["--connect", "my server:42000"]);
    assert!(spec.env.iter().any(|(k, _)| k == "FONTCONFIG_FILE"));
    assert!(spec.env.contains(&("A".to_string(), "1".to_string())));
    assert!(spec.command_line().contains("'my server:42000'"));
  }

  #[test]
  fn wrapper_and_extra_mod_path() {
    let (d, l, i) = setup();
    let extra = d.path().join("extra-mods");
    l.update_instance(&i.id, |x| {
      x.launch.wrapper = vec!["gamemoderun".into()];
      x.mods_dir = Some(extra.clone());
      Ok(())
    })
    .unwrap();
    let spec = l.launch_spec(&l.instance(&i.id).unwrap()).unwrap();
    assert_eq!(spec.program, PathBuf::from("gamemoderun"));
    assert!(spec.args[0].ends_with(if cfg!(windows) {
      "Vintagestory.exe"
    } else {
      "Vintagestory"
    }));
    assert!(
      spec
        .args
        .contains(&format!("--addModPath={}", extra.display()))
    );
  }

  #[test]
  fn missing_game_version_is_explained() {
    let (_d, l, i) = setup();
    l.update_instance(&i.id, |x| {
      x.game_version = Some("9.9.9".into());
      Ok(())
    })
    .unwrap();
    let err = l.launch_spec(&l.instance(&i.id).unwrap()).unwrap_err();
    assert!(err.to_string().contains("9.9.9"));
  }

  #[test]
  fn tail_skips_blank_lines() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("l.log");
    fs::write(&p, "a\n\nb\nc\n\n").unwrap();
    assert_eq!(tail(&p, 2), ["b", "c"]);
  }

  #[test]
  fn old_launch_logs_are_pruned() {
    let d = tempfile::tempdir().unwrap();
    for n in 0..12 {
      fs::write(d.path().join(format!("launch-{n:03}.log")), "").unwrap();
    }
    prune_logs(d.path());
    assert_eq!(fs::read_dir(d.path()).unwrap().count(), KEPT_LOGS - 1);
  }

  #[tokio::test]
  async fn launch_without_account_leaves_login_to_the_game() {
    let (_d, l, i) = setup();
    l.verify_launch_session(&i).await.unwrap();
  }

  #[cfg(unix)]
  #[tokio::test]
  async fn launch_runs_waits_and_records_play_time() {
    use std::os::unix::fs::PermissionsExt;
    let (_d, l, i) = setup();
    let exe = l.game_install("1.21.5").unwrap().path.join("Vintagestory");
    fs::write(&exe, "#!/bin/sh\necho started \"$1\"\nexit 3\n").unwrap();
    fs::set_permissions(&exe, fs::Permissions::from_mode(0o755)).unwrap();

    let (session, waiter) =
      l.spawn_game(&i, &l.launch_spec(&i).unwrap()).unwrap();
    let exit = waiter.await.unwrap();
    assert_eq!(exit.code, Some(3));
    assert!(!exit.success);
    assert!(exit.tail[0].starts_with("started --dataPath="));
    assert_eq!(exit.log_path, session.log_path);
    assert!(l.instance(&i.id).unwrap().stats.last_played_at.is_some());
  }

  #[cfg(unix)]
  #[tokio::test]
  async fn game_login_survives_the_next_launch() {
    use std::os::unix::fs::PermissionsExt;

    let (_d, l, i) = setup();
    l.save_login("sample@example.invalid", LoginResponse {
      uid: "uid".into(),
      playername: "Player".into(),
      sessionkey: "old".into(),
      sessionsignature: "old-signature".into(),
      mptoken: "old-mp".into(),
      ..Default::default()
    })
    .unwrap();
    let exe = l.game_install("1.21.5").unwrap().path.join("Vintagestory");
    fs::write(
      &exe,
      "#!/bin/sh\ncat > \"${1#--dataPath=}/clientsettings.json\" \
       <<'EOF'\n{\"stringSettings\":{\"playeruid\":\"uid\",\"playername\":\"\
       Player\",\"sessionkey\":\"new\",\"sessionsignature\":\"new-signature\",\
       \"mptoken\":null}}\nEOF\n",
    )
    .unwrap();
    fs::set_permissions(&exe, fs::Permissions::from_mode(0o755)).unwrap();

    let (_, waiter) = l.spawn_game(&i, &l.launch_spec(&i).unwrap()).unwrap();
    assert!(waiter.await.unwrap().success);
    let next = l
      .create_instance(NewInstance {
        name: "next".into(),
        ..Default::default()
      })
      .unwrap();
    l.inject_account(&next).unwrap();
    let settings: serde_json::Value = serde_json::from_slice(
      &fs::read(next.data_dir().join("clientsettings.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(settings["stringSettings"]["sessionkey"], "new");
  }

  #[cfg(unix)]
  #[tokio::test]
  async fn stop_kills_the_game() {
    use std::os::unix::fs::PermissionsExt;
    let (_d, l, i) = setup();
    let exe = l.game_install("1.21.5").unwrap().path.join("Vintagestory");
    fs::write(&exe, "#!/bin/sh\nexec sleep 30\n").unwrap();
    fs::set_permissions(&exe, fs::Permissions::from_mode(0o755)).unwrap();
    let (session, waiter) =
      l.spawn_game(&i, &l.launch_spec(&i).unwrap()).unwrap();
    let handle = tokio::spawn(waiter);
    session.stop();
    let exit = timeout(Duration::from_secs(10), handle)
      .await
      .unwrap()
      .unwrap()
      .unwrap();
    assert!(exit.stopped);
  }

  #[test]
  fn no_secret_is_written_when_its_cleanup_cannot_be_recorded() {
    let (_d, l, i) = setup();
    l.login_once_for_test("u1").unwrap();
    // A directory where the cleanup list should be makes recording it fail.
    fs::create_dir_all(l.paths.session_cleanup_file()).unwrap();

    assert!(l.spawn_game(&i, &l.launch_spec(&i).unwrap()).is_err());
    let settings = i.data_dir().join("clientsettings.json");
    assert!(
      fs::read_to_string(&settings).map_or(true, |s| !s.contains("sessionkey"))
    );
  }

  #[cfg(unix)]
  #[tokio::test]
  async fn process_only_login_leaves_no_session_when_lithic_stops_waiting() {
    use std::os::unix::fs::PermissionsExt;
    let (_d, l, i) = setup();
    let exe = l.game_install("1.21.5").unwrap().path.join("Vintagestory");
    fs::write(&exe, "#!/bin/sh\nexec sleep 2\n").unwrap();
    fs::set_permissions(&exe, fs::Permissions::from_mode(0o755)).unwrap();
    l.login_once_for_test("u1").unwrap();

    let (_session, waiter) =
      l.spawn_game(&i, &l.launch_spec(&i).unwrap()).unwrap();
    let settings = i.data_dir().join("clientsettings.json");
    assert!(
      fs::read_to_string(&settings)
        .unwrap()
        .contains("sessionkey")
    );

    drop(waiter);
    assert!(
      !fs::read_to_string(&settings)
        .unwrap()
        .contains("sessionkey"),
      "quitting lithic must not leave the session behind"
    );
    assert!(
      fs::read_to_string(l.paths.session_cleanup_file())
        .unwrap()
        .contains("u1"),
      "the next start checks again in case the game writes it back"
    );
  }
}
