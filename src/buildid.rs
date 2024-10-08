// Copyright (C) 2024 Daniel Mueller <deso@posteo.net>
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

use std::borrow::Cow;
use std::fmt;

use crate::util::format_build_id;


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

impl<'id> BuildId<'id> {
  /// Create a new `BuildId` from a pre-formatted string.
  #[inline]
  pub fn formatted<B>(build_id: B) -> Self
  where
    B: Into<Cow<'id, str>>,
  {
    Self::Formatted(build_id.into())
  }

  /// Create a new `BuildId` from the "raw" bytes.
  #[inline]
  pub fn raw<B>(build_id: B) -> Self
  where
    B: Into<Cow<'id, [u8]>>,
  {
    Self::RawBytes(build_id.into())
  }

  /// Returns a string representation in hex.
  ///
  /// ```
  /// # use debuginfod::BuildId;
  /// let build_id = BuildId::raw(&[
  ///   0xae, 0xb9, 0xa9, 0x83, 0xac, 0xe1, 0xfb, 0x04, 0x7b, 0x23,
  ///   0x41, 0xb1, 0x95, 0x01, 0x65, 0x44, 0x0f, 0xb2, 0xa8, 0xb9,
  /// ]);
  ///
  /// assert_eq!(build_id.format(), "aeb9a983ace1fb047b2341b1950165440fb2a8b9");
  /// ```
  pub fn format(&self) -> Cow<'_, str> {
    match self {
      Self::RawBytes(bytes) => Cow::Owned(format_build_id(bytes)),
      Self::Formatted(string) => Cow::Borrowed(string),
    }
  }
}

impl fmt::Display for BuildId<'_> {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    write!(f, "{}", self.format())
  }
}


#[cfg(test)]
mod tests {
  use super::*;


  /// Check that we can construct `BuildId` objects as expected.
  #[test]
  fn build_id_construction() {
    let build_id = BuildId::raw(&[0x00]);
    assert!(matches!(build_id, BuildId::RawBytes(Cow::Borrowed(..))));

    let build_id = BuildId::raw(vec![0x00]);
    assert!(matches!(build_id, BuildId::RawBytes(Cow::Owned(..))));

    let build_id = BuildId::formatted("abc");
    assert!(matches!(build_id, BuildId::Formatted(Cow::Borrowed(..))));

    let build_id = BuildId::formatted("abc".to_string());
    assert!(matches!(build_id, BuildId::Formatted(Cow::Owned(..))));
  }

  /// Test the `Display` implementation of the `BuildId` type.
  #[test]
  fn build_id_display() {
    let build_id = BuildId::raw(&[
      0xae, 0xb9, 0xa9, 0x83, 0xac, 0xe1, 0xfb, 0x04, 0x7b, 0x23, 0x41, 0xb1, 0x95, 0x01, 0x65,
      0x44, 0x0f, 0xb2, 0xa8, 0xb9,
    ]);
    assert_eq!(
      build_id.to_string(),
      "aeb9a983ace1fb047b2341b1950165440fb2a8b9"
    );
  }
}
