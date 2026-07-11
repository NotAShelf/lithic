use std::env;
use std::ffi::OsStr;
#[cfg(unix)]
use std::path::Path;
use std::path::PathBuf;

#[cfg(windows)]
use std::process::Command;

#[cfg(unix)]
use lithic_core::utils::notice;

#[cfg(unix)]
use comfy_table::{Attribute, Color};

use crate::commands::download::download_file;
use async_zip::tokio::read::fs::ZipFileReader;
use futures::AsyncReadExt;
use lithic_core::api::client::ApiClient;
use lithic_core::errors::LithicError;
use tokio::fs;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use tracing::info;
use uuid::Uuid;
use yansi::Paint;

pub struct LithicUpdater {
   /// This is the name of the binary inside the archive
   pub new_binary_name: String,
   pub current_binary_path: PathBuf,
   /// This will be the temp working dir
   pub temp_dir: PathBuf,
   /// The full path to the download archive
   pub downloaded_path: PathBuf,
}

impl LithicUpdater {
   /// Create updater state in a fresh temporary directory.
   ///
   /// # Errors
   ///
   /// Returns an error if the temp directory cannot be created or the current
   /// executable path cannot be resolved.
   pub async fn new(binary_name: &str) -> Result<Self, LithicError> {
      let temp_dir = env::temp_dir().join(Uuid::new_v4().to_string());
      if !temp_dir.exists() {
         tokio::fs::create_dir_all(&temp_dir).await?;
      }

      info!("Created temp dir {}", temp_dir.display().magenta());

      Ok(Self {
         new_binary_name: binary_name.to_string(),
         temp_dir,
         downloaded_path: PathBuf::default(),
         current_binary_path: env::current_exe()?,
      })
   }

   /// Download and extract an update archive.
   ///
   /// # Errors
   ///
   /// Returns an error if downloading, extracting, or cleanup after extraction
   /// failure fails.
   pub async fn download_archive(
      &mut self,
      archive_name: &str,
      download_url: &str,
      finish_msg: impl AsRef<str>,
   ) -> Result<&LithicUpdater, LithicError> {
      let client = ApiClient::new();
      download_file(
         &client,
         download_url,
         &self.temp_dir.join(archive_name),
         finish_msg,
      )
      .await?;

      self.downloaded_path = self.temp_dir.join(archive_name);

      if let Err(e) = self.extract_binary().await {
         info!(
            "{}: {}",
            "Failed to extract binary; cleaning up temp files".yellow(),
            e.red().bold()
         );
         if let Err(cleanup_err) = fs::remove_dir_all(&self.temp_dir).await {
            info!("Failed to clean update temp directory: {cleanup_err}");
         }
         return Err(e);
      }

      Ok(self)
   }

   /// Extract the target binary from the downloaded archive.
   ///
   /// # Errors
   ///
   /// Returns an error if the archive cannot be read, the binary entry is
   /// missing, or the extracted binary cannot be written.
   pub async fn extract_binary(&self) -> Result<PathBuf, LithicError> {
      info!("Extracting {}", &self.downloaded_path.display());

      let zip = ZipFileReader::new(&self.downloaded_path)
         .await
         .map_err(|e| LithicError::ZipError {
            context: format!(
               "Failed to load zip archive into ZipFileReader: {}",
               &self.downloaded_path.display()
            ),
            source: e,
         })?;

      info!("Looking for binary in archive called: {}", &self.new_binary_name);

      let entry_index = zip
         .file()
         .entries()
         .iter()
         .position(|entry| {
            let filename = entry.filename().as_str().unwrap_or("");
            info!("Current file in archive: {}", filename.magenta());
            filename == self.new_binary_name || filename.ends_with(&format!("/{}", &self.new_binary_name))
         })
         .ok_or_else(|| {
            LithicError::SimpleError(format!("Failed to locate {} in zip", &self.new_binary_name))
         })?;

      // extract the binary
      let mut entry_reader = zip
         .reader_with_entry(entry_index)
         .await
         .map_err(|e| LithicError::ZipError {
            context: format!("Failed to create entry_reader for {}", &self.new_binary_name),
            source: e,
         })?;

      let mut output_file = File::create(&self.temp_dir.join(&self.new_binary_name)).await?;
      let mut buffer = Vec::new();
      entry_reader.read_to_end(&mut buffer).await?;
      output_file.write_all(&buffer).await?;

      info!("{}", "Successfully extracted binary from zip archive".green());

      Ok(self.temp_dir.join(&self.new_binary_name))
   }

   #[cfg(unix)]
   /// Replace the current Unix binary with the extracted update.
   ///
   /// # Errors
   ///
   /// Returns an error if permissions cannot be copied, staging or replacing
   /// the executable fails, or cleanup fails.
   pub async fn update(&self) -> Result<(), LithicError> {
      // Keep the replacement executable's mode consistent with the binary the
      // user launched, including non-default executable permissions.
      self.set_new_perms().await?;

      let staged_binary = staged_binary_path(&self.current_binary_path);
      fs::copy(self.temp_dir.join(&self.new_binary_name), &staged_binary).await?;

      // The staged file lives next to the running executable, so this rename
      // is an atomic replacement on Unix and never leaves the old binary
      // missing if copying the update fails.
      fs::rename(&staged_binary, &self.current_binary_path).await?;

      notice("Update successful!", Some(Color::Green), vec![Attribute::Bold]);
      fs::remove_file(&self.downloaded_path).await?;
      fs::remove_file(self.temp_dir.join(&self.new_binary_name)).await?;
      fs::remove_dir(&self.temp_dir).await?;
      Ok(())
   }

   #[cfg(unix)]
   /// Copies the current binary's permissions onto the extracted replacement.
   ///
   /// # Errors
   ///
   /// Returns an error if permissions cannot be read from the existing binary
   /// or applied to the extracted binary.
   pub async fn set_new_perms(&self) -> Result<(), LithicError> {
      info!("Copying permissions from current binary to extracted update");
      let perms = fs::metadata(&self.current_binary_path).await?.permissions();
      fs::set_permissions(&self.temp_dir.join(&self.new_binary_name), perms).await?;

      info!("Permissions copied from current binary to new one");
      Ok(())
   }

   #[cfg(windows)]
   pub async fn create_update_script(&self) -> Result<&LithicUpdater, LithicError> {
      let script_path = self.temp_dir.join("update.bat");

      let exe_backup = &self.make_backup().await?;

      // load update script and replace placeholders
      let template = include_str!("windows_updater.bat");
      let exe_name_path = self.get_current_binary_filename()?;
      let Some(exe_name) = exe_name_path.to_str() else {
         return Err(LithicError::SimpleError(format!(
            "current binary filename is not valid UTF-8: {}",
            exe_name_path.display()
         )));
      };
      let script_content = template
         .replace("{EXE_NAME}", exe_name)
         .replace(
            "{CURRENT_EXE}",
            self.current_binary_path.to_string_lossy().as_ref(),
         )
         .replace("{BACKUP_PATH}", exe_backup.to_string_lossy().as_ref())
         .replace(
            "{NEW_BINARY}",
            self
               .temp_dir
               .join(&self.new_binary_name)
               .to_string_lossy()
               .as_ref(),
         );

      fs::write(&script_path, &script_content).await?;

      Ok(self)
   }

   #[cfg(windows)]
   pub fn execute_update_bat(&self) -> Result<(), LithicError> {
      info!("Exiting Lithic and executing update bat");

      // start the update script in background
      Command::new("cmd")
         .args([
            "/C",
            "start",
            "/MIN",
            self.temp_dir.join("update.bat").to_string_lossy().as_ref(),
         ])
         .spawn()
         .map_err(|e| {
            LithicError::SimpleError(format!(
               "{}: {}",
               "Failed to spawn windows update process".yellow(),
               e.to_string().red().bold()
            ))
         })?;

      // exit so the update script can be updated
      std::process::exit(0);
   }

   /// Copy the current binary into the updater temp directory.
   ///
   /// # Errors
   ///
   /// Returns an error if the current binary name cannot be resolved or the
   /// backup copy fails.
   pub async fn make_backup(&self) -> Result<PathBuf, LithicError> {
      let backup_path = &self
         .temp_dir
         .join(self.get_current_binary_filename()?)
         .with_added_extension("backup");

      fs::copy(&self.current_binary_path, &backup_path).await?;

      Ok(backup_path.clone())
   }

   fn get_current_binary_filename(&self) -> Result<&OsStr, LithicError> {
      self
         .current_binary_path
         .file_name()
         .ok_or_else(|| LithicError::SimpleError("Unable to get file name from current exe path".into()))
   }
}

#[cfg(unix)]
fn staged_binary_path(current_binary_path: &Path) -> PathBuf {
   current_binary_path.with_added_extension("lithic-update")
}

#[cfg(test)]
mod tests {
   #[cfg(unix)]
   #[test]
   fn staged_binary_keeps_the_running_binary_name() {
      let current = std::path::Path::new("/tmp/lithic.bin");
      assert_eq!(
         super::staged_binary_path(current),
         std::path::PathBuf::from("/tmp/lithic.bin.lithic-update")
      );
   }
}
