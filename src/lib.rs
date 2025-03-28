// Copyright (C) 2024 Daniel Mueller <deso@posteo.net>
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! A crate for interacting with [`debuginfod`][debuginfod] servers.
//!
//! [debuginfod]: https://sourceware.org/elfutils/Debuginfod.html

#![warn(
  missing_debug_implementations,
  missing_docs,
  clippy::absolute_paths,
  rustdoc::broken_intra_doc_links
)]
#![cfg_attr(docsrs, feature(doc_cfg))]

mod buildid;
#[cfg(feature = "fs-cache")]
mod caching_client;
mod client;
mod util;

pub use buildid::BuildId;
#[cfg(feature = "fs-cache")]
#[cfg_attr(docsrs, doc(cfg(feature = "fs-cache")))]
pub use caching_client::CachingClient;
pub use client::Client;
pub use client::Response;


#[cfg(feature = "tracing")]
#[macro_use]
#[allow(unused_imports)]
mod log {
  pub(crate) use tracing::debug;
  pub(crate) use tracing::error;
  pub(crate) use tracing::info;
  pub(crate) use tracing::instrument;
  pub(crate) use tracing::trace;
  pub(crate) use tracing::warn;
}

#[cfg(not(feature = "tracing"))]
#[macro_use]
#[allow(unused_imports)]
mod log {
  macro_rules! debug {
    ($($args:tt)*) => {{
      if false {
        // Make sure to use `args` to prevent any warnings about
        // unused variables.
        let _args = format_args!($($args)*);
      }
    }};
  }

  pub(crate) use debug;
  pub(crate) use debug as error;
  pub(crate) use debug as info;
  pub(crate) use debug as trace;
  pub(crate) use debug as warn;
}
