//! Installs, dependency resolution, updates and pins against a local stand-in
//! for the ModDB.

use std::collections::HashMap;
use std::io::Write;
use std::sync::{Arc, Mutex};

use lithic_core::instance::NewInstance;
use lithic_core::moddb::ModDb;
use lithic_core::mods::{InstallOptions, ModRef, Reason};
use lithic_core::{Cancel, Error, Lithic, Paths};
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

#[derive(Default)]
struct Fake {
   mods: HashMap<String, serde_json::Value>,
   files: HashMap<String, Vec<u8>>,
}

type Shared = Arc<Mutex<Fake>>;

fn mod_zip(modinfo: &serde_json::Value) -> Vec<u8> {
   let mut buf = std::io::Cursor::new(Vec::new());
   {
      let mut w = zip::ZipWriter::new(&mut buf);
      w.start_file("modinfo.json", zip::write::SimpleFileOptions::default())
         .unwrap();
      w.write_all(modinfo.to_string().as_bytes()).unwrap();
      w.finish().unwrap();
   }
   buf.into_inner()
}

/// Publishes a release: its zip becomes downloadable and it is added to the
/// mod's release list.
fn publish(
   fake: &Shared,
   base: &str,
   moddb_id: i64,
   mod_id: &str,
   version: &str,
   tags: &[&str],
   deps: serde_json::Value,
) {
   let mut f = fake.lock().unwrap();
   let file = format!("{mod_id}-{version}.zip");
   f.files.insert(
      file.clone(),
      mod_zip(
         &json!({"modid": mod_id, "name": mod_id.to_uppercase(), "version": version, "dependencies": deps}),
      ),
   );
   let release = json!({
      "releaseid": moddb_id * 100 + i64::try_from(f.files.len()).unwrap(),
      "mainfile": format!("{base}/files/{file}?dl={file}"),
      "filename": file,
      "tags": tags,
      "modidstr": mod_id,
      "modversion": version,
   });
   let entry = f.mods.entry(mod_id.to_string()).or_insert_with(
      || json!({"modid": moddb_id, "name": mod_id.to_uppercase(), "urlalias": mod_id, "releases": []}),
   );
   entry["releases"].as_array_mut().unwrap().insert(0, release);
   let entry = entry.clone();
   f.mods.insert(moddb_id.to_string(), entry);
}

async fn serve(fake: Shared) -> String {
   let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
   let base = format!("http://{}", listener.local_addr().unwrap());
   tokio::spawn(async move {
      loop {
         let Ok((mut sock, _)) = listener.accept().await else {
            return;
         };
         let fake = fake.clone();
         tokio::spawn(async move {
            let mut buf = vec![0u8; 8192];
            let mut len = 0;
            while !buf[..len].windows(4).any(|w| w == b"\r\n\r\n") {
               let n = sock.read(&mut buf[len..]).await.unwrap_or(0);
               if n == 0 {
                  return;
               }
               len += n;
            }
            let request = String::from_utf8_lossy(&buf[..len]).to_string();
            let path = request
               .split_whitespace()
               .nth(1)
               .unwrap_or("/")
               .split('?')
               .next()
               .unwrap_or("/")
               .to_string();
            let (status, body, kind) = {
               let f = fake.lock().unwrap();
               if let Some(id) = path.strip_prefix("/mod/") {
                  match f.mods.get(&id.to_lowercase()) {
                     Some(m) => (
                        "200 OK",
                        json!({"statuscode": "200", "mod": m}).to_string().into_bytes(),
                        "application/json",
                     ),
                     None => ("200 OK", br#"{"statuscode":"404"}"#.to_vec(), "application/json"),
                  }
               } else if let Some(name) = path.strip_prefix("/files/") {
                  match f.files.get(name) {
                     Some(bytes) => ("200 OK", bytes.clone(), "application/zip"),
                     None => ("404 Not Found", Vec::new(), "text/plain"),
                  }
               } else {
                  ("404 Not Found", Vec::new(), "text/plain")
               }
            };
            let head = format!(
               "HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
               body.len()
            );
            let _ = sock.write_all(head.as_bytes()).await;
            let _ = sock.write_all(&body).await;
         });
      }
   });
   base
}

async fn setup() -> (tempfile::TempDir, Lithic, lithic_core::Instance, Shared, String) {
   let dir = tempfile::tempdir().unwrap();
   let fake: Shared = Arc::default();
   let base = serve(fake.clone()).await;
   let mut lithic = Lithic::new(Paths::rooted(dir.path())).unwrap();
   lithic.moddb = ModDb::with_base(lithic.http.clone(), &base);
   let instance = lithic
      .create_instance(NewInstance {
         name: "Test".into(),
         game_version: Some("1.21.5".into()),
         ..Default::default()
      })
      .unwrap();
   (dir, lithic, instance, fake, base)
}

fn opts() -> InstallOptions {
   InstallOptions {
      dependencies: true,
      pin_versions: true,
      ..Default::default()
   }
}

#[tokio::test]
async fn install_resolves_dependencies_for_the_game_version() {
   let (_d, l, i, fake, base) = setup().await;
   publish(
      &fake,
      &base,
      1,
      "app",
      "1.0.0",
      &["1.21.0", "1.21.4"],
      json!({"lib": "1.0.0", "game": "1.21.0"}),
   );
   publish(
      &fake,
      &base,
      1,
      "app",
      "2.0.0",
      &["1.22.0"],
      json!({"lib": "1.0.0"}),
   );
   publish(&fake, &base, 2, "lib", "1.1.0", &["1.21.3"], json!({}));
   publish(&fake, &base, 2, "lib", "1.2.0-dev.1", &["1.21.5"], json!({}));

   let report = l
      .install_mods(&i, &[ModRef::parse("app").unwrap()], &opts())
      .await
      .unwrap();
   assert!(report.is_success(), "{:?}", report.failures);
   let mut got: Vec<(String, String, Reason)> = report
      .changes
      .iter()
      .map(|c| (c.mod_id.clone(), c.to.clone(), c.reason.clone()))
      .collect();
   got.sort_by(|a, b| a.0.cmp(&b.0));
   assert_eq!(
      got,
      [
         ("app".to_string(), "1.0.0".to_string(), Reason::Requested),
         (
            "lib".to_string(),
            "1.1.0".to_string(),
            Reason::Dependency {
               of: "app".to_string()
            }
         ),
      ]
   );
   assert!(i.mods_dir().join("app-1.0.0.zip").is_file());
   assert!(i.mods_dir().join("lib-1.1.0.zip").is_file());

   let lock = l.mod_lock(&i).unwrap();
   assert!(lock.mods["lib"].dependency);
   assert!(!lock.mods["app"].dependency);
   assert_eq!(lock.mods["app"].moddb_id, Some(1));
   assert!(lithic_core::mods::problems(&l.installed_mods(&i).unwrap()).is_empty());

   let again = l
      .install_mods(&i, &[ModRef::parse("app").unwrap()], &opts())
      .await
      .unwrap();
   assert!(again.changes.is_empty());
   assert_eq!(again.unchanged, ["app"]);
}

#[tokio::test]
async fn updates_replace_files_and_respect_pins() {
   let (_d, l, i, fake, base) = setup().await;
   l.update_settings(|s| s.backups.enabled = true).unwrap();
   publish(&fake, &base, 1, "app", "1.0.0", &["1.21.5"], json!({}));
   publish(&fake, &base, 2, "other", "1.0.0", &["1.21.5"], json!({}));
   l.install_mods(
      &i,
      &[
         ModRef::parse("app").unwrap(),
         ModRef::parse("other@1.0.0").unwrap(),
      ],
      &opts(),
   )
   .await
   .unwrap();
   assert_eq!(
      l.mod_lock(&i).unwrap().mods["other"].pin.as_deref(),
      Some("1.0.0")
   );

   publish(&fake, &base, 1, "app", "1.1.0", &["1.21.5"], json!({}));
   publish(&fake, &base, 2, "other", "1.1.0", &["1.21.5"], json!({}));

   let updates = l.check_updates(&i, &Cancel::new()).await.unwrap();
   assert_eq!(updates.len(), 1, "the pinned mod stays put");
   assert_eq!(updates[0].mod_id, "app");
   assert_eq!(updates[0].release.version.as_deref(), Some("1.1.0"));

   let report = l.update_mods(&i, &updates, &opts()).await.unwrap();
   assert_eq!(report.changes[0].from.as_deref(), Some("1.0.0"));
   assert!(i.mods_dir().join("app-1.1.0.zip").is_file());
   assert!(!i.mods_dir().join("app-1.0.0.zip").exists());
   let backups = l.paths.backups_dir().join(&i.id).join("app");
   assert_eq!(std::fs::read_dir(backups).unwrap().count(), 1);

   l.set_mod_pin(&i, "other", None).unwrap();
   let updates = l.check_updates(&i, &Cancel::new()).await.unwrap();
   assert_eq!(updates.len(), 1);
   assert_eq!(updates[0].mod_id, "other");
}

#[tokio::test]
async fn disabled_mods_stay_disabled_when_updated() {
   let (_d, l, i, fake, base) = setup().await;
   publish(&fake, &base, 1, "app", "1.0.0", &["1.21.5"], json!({}));
   l.install_mods(&i, &[ModRef::parse("app").unwrap()], &opts())
      .await
      .unwrap();
   l.set_mod_enabled(&i, "app", false).unwrap();
   publish(&fake, &base, 1, "app", "1.1.0", &["1.21.5"], json!({}));
   let updates = l.check_updates(&i, &Cancel::new()).await.unwrap();
   l.update_mods(&i, &updates, &opts()).await.unwrap();
   assert!(i.disabled_mods_dir().join("app-1.1.0.zip").is_file());
   assert!(!i.mods_dir().join("app-1.1.0.zip").exists());
}

#[tokio::test]
async fn failures_are_reported_per_mod() {
   let (_d, l, i, fake, base) = setup().await;
   publish(
      &fake,
      &base,
      1,
      "app",
      "1.0.0",
      &["1.21.5"],
      json!({"missinglib": "*"}),
   );
   publish(&fake, &base, 3, "old", "1.0.0", &["1.19.0"], json!({}));

   let report = l
      .install_mods(
         &i,
         &[
            ModRef::parse("app").unwrap(),
            ModRef::parse("old").unwrap(),
            ModRef::parse("nope").unwrap(),
         ],
         &opts(),
      )
      .await
      .unwrap();
   assert_eq!(report.changes.len(), 1);
   assert!(i.mods_dir().join("app-1.0.0.zip").is_file());

   let failure = |id: &str| report.failures.iter().find(|f| f.mod_id == id).unwrap();
   assert!(matches!(failure("old").error, Error::NoCompatibleRelease { .. }));
   assert!(matches!(failure("nope").error, Error::NotFound { .. }));
   assert_eq!(failure("missinglib").needed_by.as_deref(), Some("app"));
}

#[tokio::test]
async fn changes_are_refused_while_another_operation_runs() {
   let (_d, l, i, fake, base) = setup().await;
   publish(&fake, &base, 1, "app", "1.0.0", &["1.21.5"], json!({}));
   let held = std::fs::OpenOptions::new()
      .create(true)
      .truncate(false)
      .write(true)
      .open(i.dir.join(".operation.lock"))
      .unwrap();
   held.lock().unwrap();

   let result = l
      .install_mods(&i, &[ModRef::parse("app").unwrap()], &opts())
      .await;
   assert!(matches!(result, Err(Error::Busy(_))));
   assert!(matches!(l.set_mod_enabled(&i, "app", false), Err(Error::Busy(_))));

   drop(held);
   assert!(
      l.install_mods(&i, &[ModRef::parse("app").unwrap()], &opts())
         .await
         .unwrap()
         .is_success()
   );
}

#[cfg(unix)]
#[tokio::test]
async fn a_failed_update_keeps_the_old_mod() {
   use std::os::unix::fs::PermissionsExt;
   let (_d, l, i, fake, base) = setup().await;
   publish(&fake, &base, 1, "app", "1.0.0", &["1.21.5"], json!({}));
   l.install_mods(&i, &[ModRef::parse("app").unwrap()], &opts())
      .await
      .unwrap();
   publish(&fake, &base, 1, "app", "1.1.0", &["1.21.5"], json!({}));
   let updates = l.check_updates(&i, &Cancel::new()).await.unwrap();

   let mods = i.mods_dir();
   std::fs::set_permissions(&mods, std::fs::Permissions::from_mode(0o555)).unwrap();
   let report = l.update_mods(&i, &updates, &opts()).await.unwrap();
   std::fs::set_permissions(&mods, std::fs::Permissions::from_mode(0o755)).unwrap();

   assert_eq!(report.failures.len(), 1);
   assert!(report.changes.is_empty());
   assert!(
      mods.join("app-1.0.0.zip").is_file(),
      "the working version is still there"
   );
   assert_eq!(
      l.mod_lock(&i).unwrap().mods["app"].version.as_deref(),
      Some("1.0.0")
   );
}
