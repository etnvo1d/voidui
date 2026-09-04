#pragma once

#include <cstddef>
#include <cstdint>
#include <memory>
#include <string>

#include "voidui/core/resource.h"

namespace voidui {

/// How the network provider behaves.
struct HttpOptions {
  /// Sent as `User-Agent`. Servers do differentiate on it, and an application
  /// that says nothing looks like something worth blocking.
  std::string user_agent = "VoidUI/1.0";

  /// Milliseconds. Applied to the connection and to each read separately, so a
  /// server that accepts and then stalls is caught by the same number.
  int timeout_ms = 15000;

  /// A response larger than this is refused as it arrives, rather than
  /// buffered and then judged. Untrusted bytes should never be able to decide
  /// how much memory the process spends.
  std::size_t max_bytes = 32ull * 1024 * 1024;

  /// Requests in flight at once. Beyond this, callers queue.
  ///
  /// A list flung past two hundred rows would otherwise open two hundred
  /// connections -- which is slower than six, not faster, because the bandwidth
  /// is the same and every one of them now competes for it.
  int max_concurrent = 6;

  /// Where fetched bytes are kept between runs. Empty disables the disk cache,
  /// which is what a test wants and what an application almost never does.
  ///
  /// The *encoded* bytes are stored, not decoded pixels: a JPEG is a tenth the
  /// size of its pixels, and the decode size a widget wants is a property of
  /// this run's window rather than of the file.
  std::string cache_directory;

  /// A ceiling on that directory, enforced oldest-first when it is exceeded.
  std::size_t cache_bytes = 256ull * 1024 * 1024;

  /// How long a cached response is served without asking the server again.
  ///
  /// A floor, not the whole policy: a response carrying `no-store` is never
  /// written, and one carrying an entity tag is revalidated with it rather than
  /// refetched, so a stale hit costs a 304 and no body.
  int min_fresh_seconds = 300;
};

/// Fetches `http:` and `https:` URIs, caching what it gets.
///
/// Deliberately synchronous. It is only ever called from the pool's blocking
/// lane -- `ImageCache` reads there, and so should anything else that names a
/// URL -- so an asynchronous interface here would buy nothing and cost every
/// caller a state machine. Blocking a blocking-lane worker is what that lane is
/// for.
///
/// Null on a platform without an implementation, in which case a network URI
/// stays unreachable rather than silently unresolved.
std::shared_ptr<ResourceProvider> http_provider(HttpOptions options = {});

/// A path under the user's cache directory, for `HttpOptions::cache_directory`.
/// Empty when the platform will not say where that is.
std::string default_cache_directory(std::string_view application);

} // namespace voidui
