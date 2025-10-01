// Copyright (C) 2025 Arvid Norlander <VorpalBlade@users.noreply.github.com>
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

use http::Method;

use reqwest::blocking::Client as BlockingClient;
use reqwest::blocking::Request;

use crate::HttpClient;
use crate::HttpClientError;
use crate::Readable;

/// Implements the `HttpClient` trait for the `reqwest` crate.
impl HttpClient for BlockingClient {
  /// Perform a blocking HTTP GET request to the specified URL.
  fn get(&self, url: &str) -> Result<Box<dyn Readable>, HttpClientError> {
    let resp = self
      .execute(Request::new(
        Method::GET,
        url
          .try_into()
          .map_err(|err| HttpClientError::InvalidUrl(Box::new(err)))?,
      ))
      .map_err(|err| HttpClientError::Other(Box::new(err)))?;

    let status = resp.status();
    if !status.is_success() {
      return Err(HttpClientError::StatusCode(status));
    }

    Ok(Box::new(resp))
  }
}
