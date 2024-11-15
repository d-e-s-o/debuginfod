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
use crate::util::split_env_var_contents;
use crate::BuildId;

/// A successful response from a debuginfod server.
#[derive(Debug)]
pub struct Response<'url, R> {
  /// A reader for the data the debuginfod server returned.
  pub data: R,
  /// The url of the server that had the found debug info.
  pub server_url: &'url str,
}

/// Creates a new `DebugInfoResponse`.
impl<'debug_info_response, R: Read> Response<'debug_info_response, R> {
  fn new(data: R, debug_infod_url: &'debug_info_response str) -> Self {
    Response {
      data,
      server_url: debug_infod_url,
    }
  }
}

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
  /// importance. `Ok(None)` will be returned if this list is empty. If
  /// any of the variables could not be parsed, an error will be
  /// emitted.
  pub fn new<'url, U>(base_urls: U) -> Result<Option<Self>>
  where
    U: IntoIterator<Item = &'url str>,
  {
    let base_urls = base_urls
      .into_iter()
      .map(|url| Url::parse(url.trim()).with_context(|| format!("failed to parse URL `{url}`")))
      .collect::<Result<Vec<_>>>()?;

    if base_urls.is_empty() {
      return Ok(None)
    }
    debug!("using debuginfod URLs: {base_urls:#?}");

    let client = HttpClient::new();
    let slf = Self { base_urls, client };
    Ok(Some(slf))
  }

  /// Create a new `Client` object with URLs parsed from the
  /// `DEBUGINFOD_URLS` environment variable.
  ///
  /// If `DEBUGINFOD_URLS` is not present or empty, `Ok(None)` will be
  /// returned. If the variable contents could not be parsed, an error
  /// will be emitted.
  pub fn from_env() -> Result<Option<Self>> {
    let urls_str = if let Some(urls_str) = env::var_os("DEBUGINFOD_URLS") {
      urls_str
    } else {
      return Ok(None)
    };

    let urls_str = urls_str
      .to_str()
      .context("DEBUGINFOD_URLS does not contain valid Unicode")?;
    let urls = split_env_var_contents(urls_str);
    Self::new(urls)
  }

  /// Fetch the debug info for the given build ID.
  ///
  /// If debug info data is found for the provided build ID, it can be read
  /// from the response's `data` field.
  ///
  /// HTTP errors returned by a subset of servers at the base URLs provided
  /// during construction will be ignored if and only if one of them returned
  /// data successfully.
  pub fn fetch_debug_info(&self, build_id: &BuildId) -> Result<Option<Response<impl Read>>> {
    fn status_to_error(status: StatusCode) -> Error {
      let reason = status
        .canonical_reason()
        .map(|reason| format!(" ({reason})"))
        .unwrap_or_default();

      anyhow!("request failed with HTTP status {status}{reason}")
    }

    let build_id = build_id.format();
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
        s if s.is_success() => return Ok(Some(Response::new(response, base_url.as_str()))),
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

  use std::borrow::Cow;
  use std::io::copy;

  use blazesym::symbolize::Elf;
  use blazesym::symbolize::Input;
  use blazesym::symbolize::Source;
  use blazesym::symbolize::Symbolizer;

  use tempfile::NamedTempFile;


  /// Make sure that we fail `Client` construction when no base URLs are
  /// provided.
  #[test]
  fn no_valid_urls() {
    let client = Client::new([]).unwrap();
    assert!(client.is_none());

    let _err = Client::new(["!#&*(@&!"]).unwrap_err();
  }

  /// Check that we can successfully fetch debug information.
  #[test]
  fn fetch_debug_info() {
    let urls = ["https://debuginfod.fedoraproject.org/"];
    let client = Client::new(urls).unwrap().unwrap();
    // Build ID of `/usr/bin/sleep` on Fedora 38, in different representations.
    let build_ids = vec![
      BuildId::RawBytes(Cow::Borrowed(&[
        0xae, 0xb9, 0xa9, 0x83, 0xac, 0xe1, 0xfb, 0x04, 0x7b, 0x23, 0x41, 0xb1, 0x95, 0x01, 0x65,
        0x44, 0x0f, 0xb2, 0xa8, 0xb9,
      ])),
      BuildId::Formatted("aeb9a983ace1fb047b2341b1950165440fb2a8b9".into()),
    ];

    for build_id in build_ids {
      let mut response = client.fetch_debug_info(&build_id).unwrap().unwrap();
      assert_eq!(response.server_url, "https://debuginfod.fedoraproject.org/");

      let mut file = NamedTempFile::new().unwrap();
      let bytes = copy(&mut response.data, &mut file).unwrap();
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
  }

  /// Check that we fail to find debug information for an invalid build
  /// ID.
  #[test]
  fn fetch_debug_info_not_found() {
    let urls = ["https://debuginfod.fedoraproject.org/"];
    let client = Client::new(urls).unwrap().unwrap();
    let build_id = BuildId::RawBytes(Cow::Borrowed(&[0x00]));
    let info = client.fetch_debug_info(&build_id).unwrap();
    assert!(info.is_none());
  }
}
