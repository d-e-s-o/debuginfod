// Copyright (C) 2024-2025 Daniel Mueller <deso@posteo.net>
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

use std::convert::Infallible;
use std::env;
use std::io::Read;

use anyhow::anyhow;
use anyhow::Context as _;
use anyhow::Error;
use anyhow::Result;

use http::StatusCode;
use url::Url;

use crate::log::debug;
use crate::log::warn;
use crate::util::split_env_var_contents;
use crate::BuildId;
use crate::HttpClient;
use crate::HttpClientError;
use crate::Readable;

/// A successful response from a debuginfod server.
#[derive(Debug)]
pub struct Response<'url, R> {
  /// A reader for the data the debuginfod server returned.
  pub data: R,
  /// The url of the server that had the found debug info.
  pub server_url: &'url str,
}

/// Creates a new `DebugInfoResponse`.
impl<'url, R: Read> Response<'url, R> {
  fn new(data: R, server_url: &'url str) -> Self {
    Self { data, server_url }
  }
}

/// A client for interacting with (one or more) `debuginfod` servers.
#[derive(Debug)]
pub struct Client {
  /// A list of base URLs of services speaking the debuginfod
  /// protocol, in decreasing order of importance.
  base_urls: Vec<Url>,
  /// The HTTP client we use for satisfying requests.
  client: Box<dyn HttpClient + Send + Sync>,
}

impl Client {
  /// Create a new `ClientBuilder` object.
  pub fn builder() -> ClientBuilder {
    ClientBuilder::default()
  }

  /// Fetch the debug info for the given build ID.
  ///
  /// If debug info data is found for the provided build ID, it can be read
  /// from the response's `data` field.
  ///
  /// HTTP errors returned by a subset of servers at the base URLs provided
  /// during construction will be ignored if and only if one of them returned
  /// data successfully.
  pub fn fetch_debug_info(
    &self,
    build_id: &BuildId,
  ) -> Result<Option<Response<'_, impl Readable>>> {
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

      let result = self.client.get(url.as_str());
      match result {
        Ok(response) => return Ok(Some(Response::new(response, base_url.as_str()))),
        Err(HttpClientError::StatusCode(StatusCode::NOT_FOUND)) => continue,
        Err(HttpClientError::StatusCode(s)) => {
          warn!(
            "failed to retrieve debug info from `{url}`{}",
            s.canonical_reason()
              .map(|s| format!(" {s}"))
              .unwrap_or_default()
          );
          server_err = server_err.or_else(|| Some(status_to_error(s)));
          continue
        },
        Err(err) => {
          warn!("failed to issue GET request `{url}`: {err}");
          // `anyhow` only lets us add context to `Result`s, not to errors.
          // So temporarily wrap in a `Result`.
          let err = Err::<Infallible, _>(err)
            .with_context(|| format!("failed to issue request to `{url}`"))
            .unwrap_err();
          issue_err = issue_err.or_else(|| Some(err));
          continue
        },
      };
    }

    if let Some(err) = server_err.or(issue_err) {
      Err(err).with_context(|| format!("failed to fetch debug info for build ID `{build_id}`"))
    } else {
      Ok(None)
    }
  }
}

/// A builder for `Client` objects. Create via `Client::builder()`.
#[derive(Debug, Default)]
pub struct ClientBuilder<C = ()> {
  /// The HTTP client we use for satisfying requests.
  client: C,
}

impl ClientBuilder<()> {
  /// Set the HTTP client to use for requests.
  pub fn http_client<C>(self, client: C) -> ClientBuilder<C>
  where
    C: HttpClient + 'static,
  {
    ClientBuilder { client }
  }
}

impl<C> ClientBuilder<C>
where
  C: HttpClient + Send + Sync + 'static,
{
  /// Build a new `Client` able to speak the debuginfod protocol.
  ///
  /// The provided `base_urls` is a list of URLs in decreasing order of
  /// importance. `Ok(None)` will be returned if this list is empty. If
  /// any of the URLs could not be parsed, an error will be emitted.
  pub fn build<'url, U>(self, base_urls: U) -> Result<Option<Client>>
  where
    U: IntoIterator<Item = &'url str>,
  {
    let base_urls = base_urls
      .into_iter()
      .map(|url| Url::parse(url.trim()).with_context(|| format!("failed to parse URL `{url}`")))
      .collect::<Result<Vec<_>>>()?;

    if base_urls.is_empty() {
      return Ok(None);
    }
    debug!("using debuginfod URLs: {base_urls:#?}");

    let slf = Client {
      base_urls,
      client: Box::new(self.client),
    };
    Ok(Some(slf))
  }

  /// Build a new `Client` object with URLs parsed from the
  /// `DEBUGINFOD_URLS` environment variable.
  ///
  /// If `DEBUGINFOD_URLS` is not present or empty, `Ok(None)` will be
  /// returned. If the variable contents could not be parsed, an error
  /// will be emitted.
  pub fn build_from_env(self) -> Result<Option<Client>> {
    let urls_str = if let Some(urls_str) = env::var_os("DEBUGINFOD_URLS") {
      urls_str
    } else {
      return Ok(None);
    };

    let urls_str = urls_str
      .to_str()
      .context("DEBUGINFOD_URLS does not contain valid Unicode")?;
    let urls = split_env_var_contents(urls_str);
    self.build(urls)
  }
}


#[cfg(test)]
mod tests {
  use super::*;

  use std::borrow::Cow;
  use std::fmt::Debug;
  use std::io::copy;
  use std::io::Error as IoError;
  use std::io::ErrorKind;

  use blazesym::symbolize::source::Elf;
  use blazesym::symbolize::source::Source;
  use blazesym::symbolize::Input;
  use blazesym::symbolize::Symbolizer;

  use reqwest::blocking::Client as ReqwestBlockingClient;

  use tempfile::NamedTempFile;

  use test_fork::fork;

  use crate::Readable;


  /// Make sure that we fail `Client` construction when no base URLs are
  /// provided.
  #[test]
  fn no_valid_urls() {
    let client = Client::builder()
      .http_client(ReqwestBlockingClient::new())
      .build([])
      .unwrap();
    assert!(client.is_none());

    let _err = Client::builder()
      .http_client(ReqwestBlockingClient::new())
      .build(["!#&*(@&!"])
      .unwrap_err();
  }

  /// Check that the creation of a `Client` object from information
  /// provided in the environment works as it should.
  #[fork]
  #[test]
  fn from_env_creation() {
    // SAFETY: `test-fork` ensures that we are in a single-threaded
    //         context.
    let () = unsafe { env::remove_var("DEBUGINFOD_URLS") };
    let result = Client::builder()
      .http_client(ReqwestBlockingClient::new())
      .build_from_env()
      .unwrap();
    assert!(result.is_none(), "{result:?}");

    let urls = "https://debug.infod https://de.bug.info.d";
    // SAFETY: `test-fork` ensures that we are in a single-threaded
    //         context.
    let () = unsafe { env::set_var("DEBUGINFOD_URLS", urls) };
    let client = Client::builder()
      .http_client(ReqwestBlockingClient::new())
      .build_from_env()
      .unwrap()
      .unwrap();
    assert_eq!(client.base_urls.len(), 2);
  }

  /// Check that we can successfully fetch debug information.
  #[test]
  fn fetch_debug_info() {
    let urls = ["https://debuginfod.fedoraproject.org/"];
    let client = Client::builder()
      .http_client(ReqwestBlockingClient::new())
      .build(urls)
      .unwrap()
      .unwrap();
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
    let client = Client::builder()
      .http_client(ReqwestBlockingClient::new())
      .build(urls)
      .unwrap()
      .unwrap();
    let build_id = BuildId::RawBytes(Cow::Borrowed(&[0x00]));
    let info = client.fetch_debug_info(&build_id).unwrap();
    assert!(info.is_none());
  }

  #[derive(Debug)]
  struct DummyHttpClient(fn(&str) -> Result<Box<dyn Readable>, HttpClientError>);

  impl HttpClient for DummyHttpClient {
    fn get(&self, url: &str) -> Result<Box<dyn Readable>, HttpClientError> {
      (self.0)(url)
    }
  }

  /// Check replacing the http client with a dummy implementation.
  #[test]
  fn custom_http_client_generic_error() {
    let urls = ["https://debuginfod.fedoraproject.org/"];
    let http_client = DummyHttpClient(|url| {
      Err(HttpClientError::Other(Box::new(IoError::new(
        ErrorKind::Other,
        format!("DummyHttpClient cannot fetch {url}"),
      ))))
    });
    let client = Client::builder()
      .http_client(http_client)
      .build(urls)
      .unwrap()
      .unwrap();
    let build_id = BuildId::RawBytes(Cow::Borrowed(&[0x00]));
    let err = client.fetch_debug_info(&build_id).unwrap_err();
    assert!(err
      .root_cause()
      .to_string()
      .contains("DummyHttpClient cannot fetch"));
  }

  /// Check replacing the http client with a dummy implementation.
  #[test]
  fn custom_http_client_status_code_404() {
    let urls = ["https://debuginfod.fedoraproject.org/"];
    let http_client =
      DummyHttpClient(|_url| Err(HttpClientError::StatusCode(StatusCode::NOT_FOUND)));
    let client = Client::builder()
      .http_client(http_client)
      .build(urls)
      .unwrap()
      .unwrap();
    let build_id = BuildId::RawBytes(Cow::Borrowed(&[0x00]));
    let res = client.fetch_debug_info(&build_id);
    assert!(res.unwrap().is_none());
  }


  /// Check replacing the http client with a dummy implementation.
  #[test]
  fn custom_http_client_found_second() {
    let urls = [
      "https://debuginfod.fedoraproject.org/",
      "https://debuginfod.archlinux.org/",
    ];
    let http_client = DummyHttpClient(|url: &str| {
      if url.contains("debuginfod.archlinux.org") {
        let data: &[u8] = b"Debug info!";
        return Ok(Box::new(data));
      }
      Err(HttpClientError::StatusCode(StatusCode::NOT_FOUND))
    });
    let client = Client::builder()
      .http_client(http_client)
      .build(urls)
      .unwrap()
      .unwrap();
    let build_id = BuildId::RawBytes(Cow::Borrowed(&[0x00]));
    let mut info = client.fetch_debug_info(&build_id).unwrap().unwrap();
    assert_eq!(info.server_url, "https://debuginfod.archlinux.org/");

    let mut buf = String::new();
    info.data.read_to_string(&mut buf).unwrap();
    assert_eq!(buf, "Debug info!");
  }

  // Check replacing the http client with a dummy implementation, other status
  // code
  #[test]
  fn custom_http_client_status_code_other() {
    let urls = ["https://debuginfod.fedoraproject.org/"];
    let http_client =
      DummyHttpClient(|_url| Err(HttpClientError::StatusCode(StatusCode::IM_A_TEAPOT)));
    let client = Client::builder()
      .http_client(http_client)
      .build(urls)
      .unwrap()
      .unwrap();
    let build_id = BuildId::RawBytes(Cow::Borrowed(&[0x00]));
    let err = client.fetch_debug_info(&build_id).unwrap_err();
    assert!(err
      .root_cause()
      .to_string()
      .contains("request failed with HTTP status 418"));
  }
}
