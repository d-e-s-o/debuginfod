// Copyright (C) 2024 Daniel Mueller <deso@posteo.net>
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

use std::env;
use std::io::Read;

use anyhow::anyhow;
use anyhow::Context as _;
use anyhow::Error;
use anyhow::Result;

use reqwest::blocking::Client as HttpClient;
use reqwest::blocking::Request;
use reqwest::Method;
use reqwest::StatusCode;
use reqwest::Url;

use crate::log::debug;
use crate::log::warn;
use crate::util::format_build_id;
use crate::util::parse_urls;


/// A client for interacting with (one or more) `debuginfod` servers.
#[derive(Debug)]
pub struct Client {
  /// A list of base URLs of services speaking the debuginfod
  /// protocol, in decreasing order of importance.
  base_urls: Vec<Url>,
  /// The HTTP client we use for satisfying requests.
  client: HttpClient,
}

impl Client {
  /// Create a new `Client` able to speak the debuginfod protocol.
  ///
  /// The provided `base_urls` is a list of URLs in decreasing order of
  /// importance.
  ///
  /// # Panics
  /// The function panics if `base_urls` is empty.
  pub fn new(base_urls: Vec<Url>) -> Self {
    assert!(
      !base_urls.is_empty(),
      "provided base URLs must not be empty"
    );
    debug!("using debuginfod URLs: {base_urls:#?}");

    let client = HttpClient::new();
    Self { base_urls, client }
  }

  /// Returns a vector of URLs based on format of the
  /// `DEBUGINFOD_URLS` environment variable value, which is either a
  /// comma separated or space separated list of URLs.
  pub fn from_env() -> Result<Self> {
    let urls_str =
      env::var_os("DEBUGINFOD_URLS").context("DEBUGINFOD_URLS variable is not present")?;
    let urls_str = urls_str
      .to_str()
      .context("DEBUGINFOD_URLS does not contain valid Unicode")?;
    let urls = parse_urls(urls_str)?;
    let slf = Self::new(urls);
    Ok(slf)
  }

  /// Fetch the debug info for the given build ID.
  ///
  /// If debug info data is found for the provided build ID, it can be read
  /// from the given [`Read`] object.
  ///
  /// HTTP errors returned by a subset of servers at the base URLs provided
  /// during construction will be ignored if and only if one of them returned
  /// data successfully.
  pub fn fetch_debug_info(&self, build_id: &[u8]) -> Result<Option<impl Read>> {
    fn status_to_error(status: StatusCode) -> Error {
      let reason = status
        .canonical_reason()
        .map(|reason| format!(" ({reason})"))
        .unwrap_or_default();

      anyhow!("request failed with HTTP status {status}{reason}")
    }

    let build_id = format_build_id(build_id);
    let mut issue_err = None;
    let mut server_err = None;

    // The endpoint we contact is `/buildid/<BUILDID>/debuginfo`.
    for base_url in &self.base_urls {
      let mut url = base_url.clone();
      let () = url.set_path(&format!("buildid/{build_id}/debuginfo"));
      debug!("making GET request to {url}");

      let result = self
        .client
        .execute(Request::new(Method::GET, url.clone()))
        .context("failed to issue request to `{url}`");
      let response = match result {
        Ok(response) => response,
        Err(err) => {
          warn!("failed to issue GET request `{url}`: {err}");
          issue_err = issue_err.or_else(|| Some(err));
          continue
        },
      };

      match response.status() {
        s if s.is_success() => return Ok(Some(response)),
        s if s == StatusCode::NOT_FOUND => continue,
        s => {
          warn!(
            "failed to retrieve debug info from `{url}`{}",
            s.canonical_reason()
              .map(|s| format!(" {s}"))
              .unwrap_or_default()
          );
          server_err = server_err.or_else(|| Some(status_to_error(s)));
          continue
        },
      }
    }

    if let Some(err) = server_err.or(issue_err) {
      Err(err).context("failed to fetch debug info for build ID `{build_id}`")
    } else {
      Ok(None)
    }
  }
}


#[cfg(test)]
mod tests {
  use super::*;

  use std::io::copy;

  use blazesym::symbolize::Elf;
  use blazesym::symbolize::Input;
  use blazesym::symbolize::Source;
  use blazesym::symbolize::Symbolizer;

  use tempfile::NamedTempFile;


  /// Make sure that we fail `Client` construction when no base URLs are
  /// provided.
  #[test]
  #[should_panic(expected = "provided base URLs must not be empty")]
  fn empty_base_urls() {
    let _result = Client::new(vec![]);
  }

  /// Check that we can successfully fetch debug information.
  #[test]
  fn fetch_debug_info() {
    let urls = vec![Url::parse("https://debuginfod.fedoraproject.org/").unwrap()];
    let client = Client::new(urls);
    // Build ID of `/usr/bin/sleep` on Fedora 38.
    let build_id = [
      0xae, 0xb9, 0xa9, 0x83, 0xac, 0xe1, 0xfb, 0x04, 0x7b, 0x23, 0x41, 0xb1, 0x95, 0x01, 0x65,
      0x44, 0x0f, 0xb2, 0xa8, 0xb9,
    ];
    let mut info = client.fetch_debug_info(&build_id).unwrap().unwrap();

    let mut file = NamedTempFile::new().unwrap();
    let bytes = copy(&mut info, &mut file).unwrap();
    assert_eq!(bytes, 112216);

    let symbolizer = Symbolizer::new();
    let src = Source::from(Elf::new(file.path()));
    let sym = symbolizer
      .symbolize_single(&src, Input::VirtOffset(0x2d70))
      .unwrap()
      .into_sym()
      .unwrap();
    assert_eq!(sym.name, "usage");
  }

  /// Check that we fail to find debug information for an invalid build
  /// ID.
  #[test]
  fn fetch_debug_info_not_found() {
    let urls = vec![Url::parse("https://debuginfod.fedoraproject.org/").unwrap()];
    let client = Client::new(urls);
    let build_id = [0x00];
    let info = client.fetch_debug_info(&build_id).unwrap();
    assert!(info.is_none());
  }
}
