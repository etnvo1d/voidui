// Stand-in for platforms whose network provider is not written yet.
//
// macOS would use NSURLSession, whose own URL cache already implements the
// freshness and revalidation rules the Windows backend spells out by hand;
// Linux would use libcurl, which does not, and would want the sidecar cache
// carried over. Both fit ResourceProvider unchanged, because the interface a
// fetch needs is the interface a file read needs.
//
// Returning null rather than a provider that always fails is deliberate:
// `Resources` reports NotMounted for a network URI when nothing is installed,
// which is the truthful answer and distinguishable from a request that ran and
// came back empty.

#include "voidui/core/http.h"

namespace voidui {

std::shared_ptr<ResourceProvider> http_provider(HttpOptions) { return nullptr; }

std::string default_cache_directory(std::string_view) { return {}; }

} // namespace voidui
