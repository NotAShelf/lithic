use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

use lithic_core::fsutil;

use crate::Ctx;
use crate::ui::{Failure, Result, fail};

pub const TEMPLATE: &str = include_str!("../../lithic.desktop");

/// Installs a desktop entry for the current user and makes lithic the handler
/// for the `ModDB`'s one-click install links.
pub fn install(ctx: &Ctx) -> Result {
   if !cfg!(all(unix, not(target_os = "macos"))) {
      return fail("desktop entries are only used on Linux and BSD desktops");
   }
   let exe = env::current_exe().map_err(|e| Failure(e.to_string()))?;
   let program = window_binary(&exe)?;

   let applications = env::var_os("XDG_DATA_HOME")
      .filter(|v| !v.is_empty())
      .map(PathBuf::from)
      .or_else(|| env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
      .ok_or_else(|| Failure("cannot find your home directory".to_string()))?
      .join("applications");
   let path = applications.join("lithic.desktop");
   fsutil::write_atomic(&path, entry(&program).as_bytes())?;
   ctx.ui.success(format!("wrote {}", path.display()));

   let registered = Command::new("xdg-mime")
      .args([
         "default",
         "lithic.desktop",
         "x-scheme-handler/vintagestorymodinstall",
      ])
      .status()
      .is_ok_and(|s| s.success());
   if registered {
      ctx.ui
         .success("install buttons on mods.vintagestory.at now open lithic");
   } else {
      ctx.ui.warn(
         "could not run xdg-mime; register lithic.desktop for x-scheme-handler/vintagestorymodinstall yourself",
      );
   }
   let _ = Command::new("update-desktop-database")
      .arg(&applications)
      .status();
   Ok(())
}

/// The binary that opens the window. The standalone CLI cannot handle links,
/// so a `lithic` next to it is used instead.
fn window_binary(exe: &Path) -> Result<PathBuf> {
   let stem = exe
      .file_stem()
      .map(|s| s.to_string_lossy().into_owned())
      .unwrap_or_default();
   if stem == "lithic" || stem.ends_with("gui") {
      return Ok(exe.to_path_buf());
   }
   let sibling = exe.with_file_name(if cfg!(windows) { "lithic.exe" } else { "lithic" });
   if sibling.is_file() {
      return Ok(sibling);
   }
   fail(format!(
      "{} cannot open the window; run `lithic desktop-entry` from the full lithic binary",
      exe.display()
   ))
}

/// The template with `Exec` pointing at `program`, quoted as the desktop
/// entry specification requires.
fn entry(program: &Path) -> String {
   let path = program.to_string_lossy();
   let quoted = if path.chars().any(|c| c.is_whitespace() || "\"'\\`$".contains(c)) {
      let escaped: String = path
         .chars()
         .flat_map(|c| match c {
            '"' | '`' | '$' | '\\' => vec!['\\', c],
            c => vec![c],
         })
         .collect();
      format!("\"{escaped}\"")
   } else {
      path.into_owned()
   };
   TEMPLATE.replace("Exec=lithic %u", &format!("Exec={quoted} %u"))
}

#[cfg(test)]
mod tests {
   use super::*;

   #[test]
   fn exec_line_is_quoted_when_needed() {
      assert!(entry(Path::new("/usr/bin/lithic")).contains("Exec=/usr/bin/lithic %u"));
      assert!(entry(Path::new("/opt/my apps/lithic")).contains("Exec=\"/opt/my apps/lithic\" %u"));
      assert!(TEMPLATE.contains("MimeType=x-scheme-handler/vintagestorymodinstall;"));
   }
}
