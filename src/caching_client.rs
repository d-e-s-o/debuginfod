// Copyright (C) 2024 Daniel Mueller <deso@posteo.net>
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

use std::env;
use std::fs::create_dir_all;
use std::fs::File;
use std::io::copy;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Context as _;
use anyhow::Result;

use dirs::cache_dir;
use dirs::home_dir;

use tempfile::NamedTempFile;

use crate::log::debug;
use crate::util::format_build_id;
use crate::Client;


/// A debuginfod client that caches data using the file system.
#[derive(Debug)]
pub struct CachingClient {
  /// The debuginfod client we use for satisfying requests.
  client: Client,
  /// The root directory of the cache.
  cache_dir: PathBuf,
}

impl CachingClient {
  /// Create a new [`CachingClient`] using `cache_dir` as the directory at
  /// which fetched debug info files are cached on the file system.
  ///
  /// # Notes
  /// Unless you have a good reason not to, it is likely best to use the
  /// system's cache directory to share data with other debuginfod aware
  /// programs. Hence, consider using the [`CachingClient::from_env`]
  /// constructor instead.
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

  /// Create a new [`CachingClient`] using the path contained in the
  /// `DEBUGINFOD_CACHE_PATH` environment variable as the directory at
  /// which fetched debug info files are cached on the file system.
  ///
  /// If `DEBUGINFOD_CACHE_PATH` is not present, then if
  /// `XDG_CACHE_HOME` is set `$XDG_CACHE_HOME/debuginfod_client` is
  /// used and if that is unset as well then
  /// `$HOME/.cache/debuginfod_client` will be used.
  pub fn from_env(client: Client) -> Result<Self> {
    let cache_path = env::var_os("DEBUGINFOD_CACHE_PATH")
      .map(PathBuf::from)
      .or_else(|| cache_dir().map(|dir| dir.join("debuginfod_client")))
      .or_else(|| home_dir().map(|dir| dir.join(".cache").join("debuginfod_client")))
      .context("DEBUGINFOD_CACHE_PATH environment variable not found")?;

    Self::new(client, cache_path)
  }

  #[inline]
  fn debuginfo_path(&self, build_id: &[u8]) -> PathBuf {
    let build_id = format_build_id(build_id);

    self.cache_dir.join(build_id).join("debuginfo")
  }

  /// Fetch the debug info for the given build ID.
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
