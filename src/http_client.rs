// Copyright (C) 2025 Arvid Norlander <VorpalBlade@users.noreply.github.com>
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

use std::error::Error;
use std::fmt::Debug;
use std::fmt::Display;
use std::fmt::Formatter;
use std::fmt::Result as FmtResult;
use std::io::Read;

use http::StatusCode;

/// An error that occurred while performing an HTTP request.
#[derive(Debug)]
#[non_exhaustive]
pub enum HttpClientError {
  /// The server responded with a non-success status code.
  StatusCode(StatusCode),
  /// The provided URL was not a well formed URL.
  InvalidUrl(Box<dyn Error + Send + Sync>),
  /// Some other error occurred.
  Other(Box<dyn Error + Send + Sync>),
}

impl Display for HttpClientError {
  fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
    match self {
      Self::StatusCode(code) => {
        write!(
          f,
          "HTTP request failed with status code {code} {}",
          code.canonical_reason().unwrap_or("")
        )
      },
      Self::Other(err) => write!(f, "HTTP client error: {err}"),
      Self::InvalidUrl(error) => {
        write!(f, "Invalid URL: {error}")
      },
    }
  }
}

impl Error for HttpClientError {
  fn source(&self) -> Option<&(dyn Error + 'static)> {
    match self {
      Self::StatusCode(_) => None,
      Self::InvalidUrl(err) | Self::Other(err) => Some(&**err),
    }
  }
}

/// A trait representing HTTP client capable of performing blocking GET
/// requests, used to download debug information from `debuginfod` servers.
pub trait HttpClient: Debug {
  /// Perform a blocking HTTP GET request to the specified URL.
  fn get(&self, url: &str) -> Result<Box<dyn Read>, HttpClientError>;
}
