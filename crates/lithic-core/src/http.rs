use std::fs::File;
use std::path::Path;
use std::time::Duration;

use md5::{Digest, Md5};
use reqwest::header::CONTENT_TYPE;
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tokio::time::sleep;

use crate::error::{Error, IoContext, Result};
use crate::progress::{Cancel, Event, Reporter};

pub const USER_AGENT: &str = concat!(
   env!("CARGO_PKG_NAME"),
   "/",
   env!("CARGO_PKG_VERSION"),
   " (+",
   env!("CARGO_PKG_REPOSITORY"),
   ")"
);

const ATTEMPTS: u32 = 3;

/// HTTP clients shared by everything that talks to the network.
#[derive(Debug, Clone)]
pub struct Http {
   api: reqwest::Client,
   /// No overall timeout: a 600 MB game archive cannot finish inside the API
   /// timeout, and reqwest applies that timeout to the body as well.
   download: reqwest::Client,
}

pub struct Download<'a> {
   pub label: &'a str,
   pub md5: Option<&'a str>,
   pub reporter: &'a Reporter,
   pub cancel: &'a Cancel,
}

impl Http {
   /// Creates the shared API and download clients.
   ///
   /// # Errors
   /// Returns an error if TLS or HTTP client initialization fails.
   pub fn new() -> Result<Self> {
      let build = |b: reqwest::ClientBuilder| {
         b.user_agent(USER_AGENT)
            .connect_timeout(Duration::from_secs(20))
            .build()
            .map_err(|e| Error::invalid(format!("cannot set up HTTP client: {e}")))
      };
      Ok(Self {
         api: build(reqwest::Client::builder().timeout(Duration::from_secs(30)))?,
         download: build(reqwest::Client::builder().read_timeout(Duration::from_mins(1)))?,
      })
   }

   /// Gets a UTF-8 response body, retrying transient failures.
   ///
   /// # Errors
   /// Returns an error if the request, response status, or body read fails.
   pub async fn get_text(&self, url: &str) -> Result<String> {
      retry(|| async {
         let response = self
            .api
            .get(url)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(|e| Error::http(url, e))?;
         response.text().await.map_err(|e| Error::http(url, e))
      })
      .await
   }

   /// POSTs an `application/x-www-form-urlencoded` body once, without
   /// retrying, and returns the status and body whatever the status is.
   ///
   /// # Errors
   /// Returns an error if sending the request or reading its response fails.
   pub async fn post_form(&self, url: &str, pairs: &[(&str, &str)]) -> Result<(u16, String)> {
      let body = pairs
         .iter()
         .map(|(k, v)| format!("{}={}", form_encode(k), form_encode(v)))
         .collect::<Vec<_>>()
         .join("&");
      let response = self
         .api
         .post(url)
         .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
         .body(body)
         .send()
         .await
         .map_err(|e| Error::http(url, e))?;
      let status = response.status().as_u16();
      let text = response.text().await.map_err(|e| Error::http(url, e))?;
      Ok((status, text))
   }

   /// Gets a response body as bytes, retrying transient failures.
   ///
   /// # Errors
   /// Returns an error if the request, response status, or body read fails.
   pub async fn get_bytes(&self, url: &str) -> Result<Vec<u8>> {
      retry(|| async {
         let response = self
            .download
            .get(url)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(|e| Error::http(url, e))?;
         Ok(response.bytes().await.map_err(|e| Error::http(url, e))?.to_vec())
      })
      .await
   }

   /// Streams `url` into `dest`. The body goes to `dest.part` first and is
   /// renamed into place only once it is complete and, when `md5` is given,
   /// matches it. Returns the number of bytes written.
   ///
   /// # Errors
   /// Returns an error if the request, file write or rename, or checksum
   /// verification fails, or the operation is cancelled.
   pub async fn download(&self, url: &str, dest: &Path, opts: Download<'_>) -> Result<u64> {
      let mut part_name = dest.file_name().unwrap_or_default().to_os_string();
      part_name.push(".part");
      let part = dest.with_file_name(part_name);
      if let Some(parent) = dest.parent() {
         fs::create_dir_all(parent).await.at(parent)?;
      }

      let result = retry(|| self.download_once(url, &part, &opts)).await;
      let (size, digest) = match result {
         Ok(v) => v,
         Err(e) => {
            let _ = fs::remove_file(&part).await;
            return Err(e);
         }
      };

      if let Some(expected) = opts.md5
         && !expected.eq_ignore_ascii_case(&digest)
      {
         let _ = fs::remove_file(&part).await;
         return Err(Error::Checksum {
            file: opts.label.to_string(),
            expected: expected.to_ascii_lowercase(),
            actual: digest,
         });
      }

      fs::rename(&part, dest).await.at(dest)?;
      Ok(size)
   }

   async fn download_once(&self, url: &str, part: &Path, opts: &Download<'_>) -> Result<(u64, String)> {
      opts.cancel.check()?;
      let mut response = self
         .download
         .get(url)
         .send()
         .await
         .and_then(reqwest::Response::error_for_status)
         .map_err(|e| Error::http(url, e))?;
      let total = response.content_length();

      let mut file = fs::File::create(part).await.at(part)?;
      let mut hasher = Md5::new();
      let mut done = 0u64;
      opts.reporter.emit(Event::Transfer {
         label: opts.label.to_string(),
         done,
         total,
      });

      while let Some(chunk) = response.chunk().await.map_err(|e| Error::http(url, e))? {
         opts.cancel.check()?;
         file.write_all(&chunk).await.at(part)?;
         hasher.update(&chunk);
         done += chunk.len() as u64;
         opts.reporter.emit(Event::Transfer {
            label: opts.label.to_string(),
            done,
            total,
         });
      }
      file.flush().await.at(part)?;
      file.sync_all().await.at(part)?;

      if let Some(total) = total
         && done != total
      {
         return Err(Error::invalid(format!(
            "download of {} ended early ({done} of {total} bytes)",
            opts.label
         )));
      }

      Ok((done, hex(&hasher.finalize())))
   }
}

async fn retry<T, F, Fut>(mut f: F) -> Result<T>
where
   F: FnMut() -> Fut,
   Fut: Future<Output = Result<T>>,
{
   let mut attempt = 1;
   loop {
      match f().await {
         Ok(v) => return Ok(v),
         Err(e) if attempt < ATTEMPTS && e.is_transient() => {
            tracing::warn!("attempt {attempt} failed, retrying: {e}");
            sleep(Duration::from_millis(500 * 2u64.pow(attempt))).await;
            attempt += 1;
         }
         Err(e) => return Err(e),
      }
   }
}

/// Percent-encodes for query strings and form bodies (space becomes `+`).
#[must_use]
pub fn form_encode(s: &str) -> String {
   let mut out = String::with_capacity(s.len());
   for b in s.bytes() {
      match b {
         b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
         b' ' => out.push('+'),
         _ => {
            const DIGITS: &[u8; 16] = b"0123456789ABCDEF";
            out.push('%');
            out.push(char::from(DIGITS[usize::from(b >> 4)]));
            out.push(char::from(DIGITS[usize::from(b & 0x0f)]));
         }
      }
   }
   out
}

#[must_use]
pub fn hex(bytes: &[u8]) -> String {
   const DIGITS: &[u8; 16] = b"0123456789abcdef";
   let mut out = String::with_capacity(bytes.len() * 2);
   for &b in bytes {
      out.push(char::from(DIGITS[usize::from(b >> 4)]));
      out.push(char::from(DIGITS[usize::from(b & 0x0f)]));
   }
   out
}

/// md5 of a file on disk, as lowercase hex.
///
/// # Errors
/// Returns an I/O error if the file cannot be opened or read.
pub fn md5_file(path: &Path) -> Result<String> {
   use std::io::Read;
   let mut file = File::open(path).at(path)?;
   let mut hasher = Md5::new();
   let mut buf = vec![0u8; 1 << 16];
   loop {
      let n = file.read(&mut buf).at(path)?;
      if n == 0 {
         break;
      }
      hasher.update(&buf[..n]);
   }
   Ok(hex(&hasher.finalize()))
}
