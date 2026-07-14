#[cfg(unix)]
use tokio::fs::symlink;

#[cfg(windows)]
use tokio::fs::{symlink_dir, symlink_file};

use crate::errors::LithicError;
use std::fs;
use std::path::Path;

pub struct SymlinkManager;

impl SymlinkManager {
   /// Create a symbolic link at `link` pointing to `target`.
   ///
   /// On Windows the syscall is type-specific (`symlink_dir` vs.
   /// `symlink_file`). We read the target's metadata once and act on that
   /// snapshot to avoid a TOCTOU window between an `is_dir()` probe and the
   /// subsequent syscall. A failure to read metadata is surfaced rather than
   /// silently treated as "file".
   pub async fn create(target: impl AsRef<Path>, link: impl AsRef<Path>) -> Result<(), LithicError> {
      let (target, link) = (target.as_ref(), link.as_ref());
      #[cfg(unix)]
      symlink(target, link)
         .await
         .map_err(|e| LithicError::SimpleError(e.to_string()))?;

      #[cfg(windows)]
      {
         let meta = tokio::fs::metadata(target).await.map_err(|e| {
            LithicError::SimpleError(format!("cannot stat symlink target {}: {e}", target.display()))
         })?;
         let res = if meta.is_dir() {
            symlink_dir(target, link).await
         } else {
            symlink_file(target, link).await
         };
         res.map_err(|e| LithicError::SimpleError(e.to_string()))?;
      }

      Ok(())
   }

   pub fn remove(path: impl AsRef<Path>) -> Result<(), LithicError> {
      fs::remove_file(path.as_ref()).map_err(|e| LithicError::SimpleError(e.to_string()))?;

      Ok(())
   }

   /// Checks if `path` is a symlink
   pub fn exists(path: impl AsRef<Path>) -> bool {
      path.as_ref().is_symlink()
   }
}
