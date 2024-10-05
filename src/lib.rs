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

use std::borrow::Cow;
use std::fmt;

use crate::util::format_build_id;

#[cfg(feature = "fs-cache")]
mod caching_client;
mod client;
mod util;

#[cfg(feature = "fs-cache")]
#[cfg_attr(docsrs, doc(cfg(feature = "fs-cache")))]
pub use caching_client::CachingClient;
pub use client::Client;


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

/// The (GNU) build id is a randomly generated string added by most compilers to
/// executables which is used by debuginfod to index them. It's typically stored
/// as a note in ELF files with name `ELF_NOTE_GNU` and type `NT_GNU_BUILD_ID`.
/// Not all runtimes produce it, for example, the Rust compiler does not
/// generate it, and Golang stores its build id in `.note.go.buildid`, but for
/// all intents and purposes the GNU build id is the only relevant one when it
/// comes to debuginfod servers.
#[derive(Debug)]
pub enum BuildId<'id> {
  /// The raw bytes of a build id read off ELF.
  RawBytes(Cow<'id, [u8]>),
  /// Printable build id in hex.
  Formatted(Cow<'id, str>),
}

impl BuildId<'_> {
  /// Returns a string representation in hex.
  pub fn formatted(&self) -> Cow<'_, str> {
    match self {
      BuildId::RawBytes(bytes) => Cow::Owned(format_build_id(bytes)),
      BuildId::Formatted(string) => Cow::Borrowed(string),
    }
  }
}

impl fmt::Display for BuildId<'_> {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    write!(f, "{}", self.formatted())
  }
}
