use crate::commands::arg_structs::download_args::DownloadArgs;
use comfy_table::{Attribute, Color};
use indicatif::{ProgressBar, ProgressStyle};
use lithic_core::api::client::{ApiClient, VSMirrorType};
use lithic_core::config::manager::get_config;
use lithic_core::errors::LithicError;
use lithic_core::traits::string_ext::StrLowerExt;
use lithic_core::utils::notice;
use lithic_core::utils::sorted_game_versions;
use std::path::{Path, PathBuf};
use std::process::exit;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use tracing::info;

/// Download a Vintage Story archive matching the provided CLI arguments.
///
/// # Errors
///
/// Returns an error if validation fails, version lookup fails, URL construction
/// fails, or the archive cannot be written.
pub async fn download(args: &DownloadArgs) -> Result<(), LithicError> {
   args.validate()?;

   let config = get_config().read().await;
   let download_dir = match &args.save_dir {
      Some(dir) => PathBuf::from(dir),
      None => config.game_download_dir.clone(),
   };

   if !download_dir.is_dir() {
      return Err(LithicError::SimpleError(format!(
         "download directory does not exist or is not a directory: {}",
         download_dir.display()
      )));
   }

   info!("Saving vintage story executable to: {}", download_dir.display());

   let client = ApiClient::new();

   // if true, then use unstable, -rc.x is unstable
   let mirror_type = match &args.game_version.lower_contains("-rc") {
      true => VSMirrorType::Unstable,
      false => VSMirrorType::Stable,
   };

   // verify game version
   let Ok(game_versions) = sorted_game_versions().await else {
      return Err(LithicError::SimpleError(
         "Failed to fetch game versions. Run Lithic sync and try again.".into(),
      ));
   };

   let user_version = args.game_version.replace('v', "");
   if !game_versions
      .iter()
      .any(|game_version| game_version.replace('v', "").eq_ignore_ascii_case(&user_version))
   {
      notice(
         format!(
            "The version you provided [{user_version}] is not valid. The following are all valid versions.."
         ),
         Some(Color::Red),
         vec![Attribute::Bold],
      );
      notice(
         format!("[{}]", game_versions.join("], [").as_str()),
         Some(Color::Magenta),
         vec![],
      );
      exit(1);
   }

   notice(
      format!("Downloading Vintage Story v{user_version}"),
      Some(Color::Yellow),
      vec![Attribute::Bold],
   );

   let (url, filename) = client.download_uri(
      &args.os_type,
      &args.exe_type,
      &mirror_type,
      &user_version,
      Option::from(&args.windows_installer_type),
   )?;

   info!("{url:?}");

   let save_loc = download_dir.join(&filename);
   download_file(&client, &url, &save_loc, "").await?;

   notice(
      format!(
         "Vintage Story has been saved to {}/{}",
         download_dir.display(),
         filename
      ),
      Some(Color::Green),
      vec![Attribute::Bold],
   );

   Ok(())
}

/// Download `url` to `save_loc` while showing a progress bar.
///
/// # Errors
///
/// Returns an error if the request fails, the destination cannot be created, or
/// any response chunk cannot be written.
///
/// # Panics
///
/// Panics only if the static progress-bar template literal is invalid.
pub async fn download_file(
   client: &ApiClient,
   url: &str,
   save_loc: impl AsRef<Path>,
   finish_message: impl AsRef<str>,
) -> Result<(), LithicError> {
   let response = client.head(&url).await?;
   let total_size = response
      .headers()
      .get(reqwest::header::CONTENT_LENGTH)
      .and_then(|ct_len| ct_len.to_str().ok())
      .and_then(|ct_len| ct_len.parse::<u64>().ok())
      .unwrap_or(0);

   let pb = ProgressBar::new(total_size);
   pb.set_style(
        ProgressStyle::with_template("[{elapsed_precise:.yellow}] [{bar:40.green/grey}] [{bytes:.cyan}/{total_bytes:.green}] [{percent:.magenta}%]")
            .expect("static indicatif template invariant: literal is well-formed")
            .progress_chars("#}•")
    );

   let mut res = client.get_request(url).await?;
   let mut file = File::create(save_loc).await?;
   let mut downloaded = 0;

   while let Some(chunk) = res
      .chunk()
      .await
      .map_err(|e| LithicError::SimpleError(e.to_string()))?
   {
      file.write_all(&chunk).await?;
      downloaded += chunk.len() as u64;
      pb.set_position(downloaded);
   }

   pb.finish();

   if !finish_message.as_ref().is_empty() {
      notice(finish_message, Some(Color::Green), vec![Attribute::Bold]);
   }

   Ok(())
}
