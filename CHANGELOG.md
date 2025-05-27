Unreleased
----------
- Exported `Response` type from rate to make it nameable in user code
- Switched from `openssl` to `rustls` backend for `reqwest` client


0.2.0
-----
- Introduced `BuildId` enum and adjusted `fetch_debug_info` methods to
  work with it
- Introduced `Response` type to include additional meta data from
  `Client::fetch_debug_info` method


0.1.1
-----
- Added `CacheClient::from_env` constructor
- Added `Debug` impl for `CachingClient`


0.1.0
-----
- Initial release
