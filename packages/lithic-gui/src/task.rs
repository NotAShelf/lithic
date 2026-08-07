//! Running core calls off the UI thread, and talking to the desktop.

use std::path::PathBuf;

/// Runs a synchronous core call on the blocking pool. Errors become text,
/// since messages must be cloneable and core errors are not.
pub async fn blocking<T, F>(f: F) -> Result<T, String>
where
   F: FnOnce() -> lithic_core::Result<T> + Send + 'static,
   T: Send + 'static,
{
   match tokio::task::spawn_blocking(f).await {
      Ok(result) => result.map_err(|e| e.to_string()),
      Err(e) => Err(format!("internal error: {e}")),
   }
}

pub async fn pick_folder(title: String) -> Option<PathBuf> {
   rfd::AsyncFileDialog::new()
      .set_title(title)
      .pick_folder()
      .await
      .map(|h| h.path().to_path_buf())
}

pub async fn pick_pack(title: String) -> Option<PathBuf> {
   rfd::AsyncFileDialog::new()
      .set_title(title)
      .add_filter("Lithic pack", &["zip"])
      .pick_file()
      .await
      .map(|h| h.path().to_path_buf())
}

pub async fn save_pack(title: String, name: String) -> Option<PathBuf> {
   rfd::AsyncFileDialog::new()
      .set_title(title)
      .set_file_name(name)
      .add_filter("Lithic pack", &["zip"])
      .save_file()
      .await
      .map(|h| h.path().to_path_buf())
}

/// Opens a folder in the file manager or a link in the browser.
pub fn open(target: &str) -> Result<(), String> {
   opener::open(target).map_err(|e| e.to_string())
}
