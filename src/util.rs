// Copyright (C) 2024 Daniel Mueller <deso@posteo.net>
// SPDX-License-Identifier: (Apache-2.0 OR MIT)


pub(crate) fn format_build_id(build_id: &[u8]) -> String {
  build_id
    .iter()
    .fold(String::with_capacity(build_id.len() * 2), |mut s, b| {
      let () = s.push_str(&format!("{b:02x}"));
      s
    })
}


pub(crate) fn split_env_var_contents(urls_str: &str) -> impl Iterator<Item = &str> {
  urls_str
    .split([',', ' '])
    .map(|s| s.trim())
    .filter(|s| !s.is_empty())
    .map(|url| url.trim())
}


#[cfg(test)]
mod tests {
  use super::*;


  /// Make sure that we can properly "stringify" a build ID.
  #[test]
  fn build_id_formatting() {
    let bytes = [
      165, 120, 253, 173, 168, 51, 14, 181, 3, 35, 210, 155, 210, 77, 246, 177, 168, 59, 252, 5,
    ];
    let expected = "a578fdada8330eb50323d29bd24df6b1a83bfc05";

    let build_id = format_build_id(&bytes);
    assert_eq!(build_id, expected);
  }

  /// Check that we can properly parse a space separated list of URLs.
  #[test]
  fn split_space_separated_urls() {
    let urls_str = "https://debug.infod https://de.bug.info.d";
    let urls = split_env_var_contents(urls_str).collect::<Vec<_>>();
    assert_eq!(urls, vec!["https://debug.infod", "https://de.bug.info.d",],);

    // Note the trailing space.
    let urls_str = "https://debug.infod ";
    let urls = split_env_var_contents(urls_str).collect::<Vec<_>>();
    assert_eq!(urls, vec!["https://debug.infod"]);
  }

  /// Check that we can properly parse a comma separated list of URLs.
  #[test]
  fn parse_comma_separated_urls() {
    let urls_str = "https://debug.infod,https://de.bug.info.d";
    let urls = split_env_var_contents(urls_str).collect::<Vec<_>>();
    assert_eq!(urls, vec!["https://debug.infod", "https://de.bug.info.d",],);
  }

  /// Check that we can properly parse a comma separated list of URLs.
  #[test]
  fn parse_no_valid_urls() {
    let urls = split_env_var_contents("").collect::<Vec<_>>();
    assert_eq!(urls, Vec::<&str>::new());
  }
}
