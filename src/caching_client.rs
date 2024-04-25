// Copyright (C) 2024 Daniel Mueller <deso@posteo.net>
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

use std::fs::create_dir_all;
use std::fs::File;
use std::io::copy;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Context as _;
use anyhow::Result;

use tempfile::NamedTempFile;

use crate::log::debug;
use crate::util::format_build_id;
use crate::Client;


/// A debuginfod client that caches data using the file system.
pub struct CachingClient {
  /// The debuginfod client we use for satisfying requests.
  client: Client,
  /// The root directory of the cache.
  cache_dir: PathBuf,
}

impl CachingClient {
  /// Create a new [`CachingClient`] using `cache_dir` as the directory at
  /// which fetched debug info files are cached on the file system.
  pub fn new<P>(client: Client, cache_dir: P) -> Result<Self>
  where
    P: AsRef<Path>,
  {
    let cache_dir = cache_dir.as_ref();
    let () = create_dir_all(cache_dir)
      .with_context(|| format!("failed to create cache directory `{}`", cache_dir.display()))?;

    let slf = Self {
      client,
      cache_dir: cache_dir.to_path_buf(),
    };
    Ok(slf)
  }

  #[inline]
  fn debuginfo_path(&self, build_id: &[u8]) -> PathBuf {
    let build_id = format_build_id(build_id);

    self.cache_dir.join(build_id).join("debuginfo")
  }

  /// Fetch the debug info for the given build ID. Retrieved data is
  /// written to the provided `Write` object.
  pub fn fetch_debug_info(&self, build_id: &[u8]) -> Result<Option<PathBuf>> {
    let path = self.debuginfo_path(build_id);
    if path.try_exists()? {
      debug!("cache hit on `{}`", path.display());
      return Ok(Some(path))
    }

    let mut debug_info = if let Some(debug_info) = self.client.fetch_debug_info(build_id)? {
      debug_info
    } else {
      return Ok(None)
    };

    // It's important that our temporary file is located inside `cache_dir`
    // already, or it may end up on a different device, in which case the
    // `persist` below won't work and we cannot guarantee atomicity.
    let mut tempfile =
      NamedTempFile::new_in(&self.cache_dir).context("failed to create temporary file")?;
    let _count =
      copy(&mut debug_info, &mut tempfile).context("failed to write debug info to file system")?;

    // SANITY: Our path is guaranteed to always have a parent.
    let dir = path.parent().unwrap();
    let () = create_dir_all(dir)
      .with_context(|| format!("failed to create directory `{}`", dir.display()))?;

    let _file = tempfile.persist_noclobber(&path).map_err(|err| {
      let src_path = err.file.path().to_path_buf();
      Result::<File, _>::Err(err)
        .with_context(|| {
          format!(
            "failed to move temporary file `{}` to `{}`",
            src_path.display(),
            path.display()
          )
        })
        .unwrap_err()
    })?;

    Ok(Some(path))
  }
}


#[cfg(test)]
mod tests {
  use super::*;

  use blazesym::symbolize::Elf;
  use blazesym::symbolize::Input;
  use blazesym::symbolize::Source;
  use blazesym::symbolize::Symbolizer;

  use tempfile::tempdir;


  /// Check that we can successfully fetch debug information.
  #[test]
  fn fetch_debug_info() {
    let cache_dir = tempdir().unwrap();
    let urls = ["https://debuginfod.fedoraproject.org/"];
    let client = Client::new(urls).unwrap().unwrap();
    let client = CachingClient::new(client, cache_dir.path()).unwrap();
    // Build ID of `/usr/bin/sleep` on Fedora 38.
    let build_id = [
      0xae, 0xb9, 0xa9, 0x83, 0xac, 0xe1, 0xfb, 0x04, 0x7b, 0x23, 0x41, 0xb1, 0x95, 0x01, 0x65,
      0x44, 0x0f, 0xb2, 0xa8, 0xb9,
    ];
    let path = client.fetch_debug_info(&build_id).unwrap().unwrap();

    let symbolizer = Symbolizer::new();
    let src = Source::from(Elf::new(path));
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
    let cache_dir = tempdir().unwrap();
    let urls = ["https://debuginfod.fedoraproject.org/"];
    let client = Client::new(urls).unwrap().unwrap();
    let client = CachingClient::new(client, cache_dir.path()).unwrap();
    let build_id = [0x00];
    let info = client.fetch_debug_info(&build_id).unwrap();
    assert!(info.is_none());
  }
}
