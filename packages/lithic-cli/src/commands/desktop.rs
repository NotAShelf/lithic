use std::{
  env,
  fs,
  io::ErrorKind,
  path::{Path, PathBuf},
};

use ini_edit::editor::{EditOptions, Editor, SeparatorSpacing};
use lithic_core::fsutil;

use crate::{
  Ctx,
  ui::{Failure, Result, fail},
};

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
    .or_else(|| {
      env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share"))
    })
    .ok_or_else(|| Failure("cannot find your home directory".to_string()))?
    .join("applications");
  let path = applications.join("lithic.desktop");
  fsutil::write_atomic(&path, entry(&program).as_bytes())?;
  ctx.ui.success(format!("wrote {}", path.display()));

  let config = env::var_os("XDG_CONFIG_HOME")
    .filter(|v| !v.is_empty())
    .map(PathBuf::from)
    .or_else(|| env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
    .ok_or_else(|| Failure("cannot find your home directory".to_string()))?
    .join("mimeapps.list");
  let current = match fs::read_to_string(&config) {
    Ok(contents) => contents,
    Err(e) if e.kind() == ErrorKind::NotFound => String::new(),
    Err(e) => return Err(Failure(e.to_string())),
  };
  let editor = Editor::with_edit_options(&current, &EditOptions {
    separator_spacing: SeparatorSpacing::Compact,
  });
  editor
    .section("Default Applications")
    .set("x-scheme-handler/vintagestorymodinstall", "lithic.desktop;");
  fsutil::write_atomic(&config, editor.finish().as_bytes())?;
  ctx
    .ui
    .success("install buttons on mods.vintagestory.at now open lithic");
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
  let sibling = exe.with_file_name(if cfg!(windows) {
    "lithic.exe"
  } else {
    "lithic"
  });
  if sibling.is_file() {
    return Ok(sibling);
  }
  fail(format!(
    "{} cannot open the window; run `lithic desktop-entry` from the full \
     lithic binary",
    exe.display()
  ))
}

/// The template with `Exec` pointing at `program`, quoted as the desktop
/// entry specification requires.
fn entry(program: &Path) -> String {
  let path = program.to_string_lossy();
  let quoted = if path
    .chars()
    .any(|c| c.is_whitespace() || "\"'\\`$".contains(c))
  {
    let escaped: String = path
      .chars()
      .flat_map(|c| {
        match c {
          '"' | '`' | '$' | '\\' => vec!['\\', c],
          c => vec![c],
        }
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
    assert!(
      entry(Path::new("/usr/bin/lithic")).contains("Exec=/usr/bin/lithic %u")
    );
    assert!(
      entry(Path::new("/opt/my apps/lithic"))
        .contains("Exec=\"/opt/my apps/lithic\" %u")
    );
    assert!(
      TEMPLATE.contains("MimeType=x-scheme-handler/vintagestorymodinstall;")
    );
  }
}
