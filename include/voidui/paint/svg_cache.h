#pragma once

#include <cstddef>
#include <cstdint>
#include <memory>
#include <string>
#include <string_view>

#include "voidui/core/async/thread_pool.h"
#include "voidui/core/invalidation.h"
#include "voidui/core/resource.h"
#include "voidui/paint/svg.h"

namespace voidui {

namespace detail {
struct SvgEntry;
} // namespace detail

/// Where a document's markup comes from.
///
/// The same three cases an image has, minus a decode size: a URI resolved
/// through the mount table, markup the application already holds, and a
/// document it has already parsed.
class SvgSource {
public:
  SvgSource() = default;

  static SvgSource uri(ResourceUri uri);

  /// Parses `reference`, resolving it against `base` the way a stylesheet's
  /// `url()` resolves. An unparseable reference yields an empty source, which
  /// loads as a failure rather than throwing at the call site.
  static SvgSource uri(std::string_view reference, const ResourceUri &base = {});

  /// Markup the caller already has -- an icon compiled into the binary as a
  /// string literal, a payload it received. Its identity is the hash of the
  /// text, so the same icon written twice is parsed once.
  static SvgSource markup(std::string text);

  /// A document that is already parsed. Nothing is loaded or cached.
  static SvgSource ready(std::shared_ptr<const SvgDocument> document);

  bool empty() const { return kind_ == Kind::Empty; }

  /// Stable identity, and the cache key.
  std::uint64_t key() const { return key_; }

  /// For diagnostics. Empty for anything that did not come from a URI.
  std::string display() const;

  const ResourceUri &resource_uri() const { return uri_; }

private:
  enum class Kind : std::uint8_t { Empty, Uri, Markup, Ready };

  friend class SvgCache;

  Kind kind_ = Kind::Empty;
  std::uint64_t key_ = 0;
  ResourceUri uri_;
  std::string text_;
  std::shared_ptr<const SvgDocument> ready_;
};

/// One caller's interest in one document.
///
/// Copyable and cheap. While any handle exists the parsed document is alive and
/// shared; when the last one goes the cache decides, against its byte budget,
/// whether to keep it for the next caller.
///
/// UI thread only, like ImageHandle and for the same reason.
class SvgHandle {
public:
  enum class State : std::uint8_t {
    Empty,
    Loading,
    Ready,
    Failed,
  };

  SvgHandle() = default;

  State state() const;
  bool loading() const { return state() == State::Loading; }
  bool ready() const { return state() == State::Ready; }
  bool failed() const { return state() == State::Failed; }

  /// The parsed document, or null unless ready.
  std::shared_ptr<const SvgDocument> document() const;

  /// Why it failed. Empty unless `failed()`.
  std::string_view error() const;

private:
  friend class SvgCache;

  explicit SvgHandle(std::shared_ptr<detail::SvgEntry> entry)
      : entry_(std::move(entry)) {}

  std::shared_ptr<detail::SvgEntry> entry_;
};

/// The process's parsed SVG documents, keyed by source.
///
/// Deduplication is the point, and it matters more here than for images: a
/// parsed document is immutable and size-independent, so every site that shows
/// one icon can share a single object -- one read, one parse, one set of paths,
/// and one entry in the renderer's mask cache. A toolbar of twenty buttons
/// showing four distinct icons holds four documents.
///
/// Loads are not cancelled. Parsing an SVG is a fraction of a millisecond of
/// CPU against a file read that has already happened, so the machinery to stop
/// one mid-flight would cost more than it could ever save; a load whose last
/// waiter has gone finishes, lands in retention, and is dropped from there like
/// anything else.
class SvgCache {
public:
  SvgCache();
  ~SvgCache();

  SvgCache(const SvgCache &) = delete;
  SvgCache &operator=(const SvgCache &) = delete;

  static SvgCache &global();

  /// Starts the load if it is not already running or done, and returns a handle
  /// to it. Cheap to call every layout pass: the common case is a hash lookup.
  SvgHandle acquire(const SvgSource &source, const Invalidator &invalidator = {},
                    async::Lane lane = async::Lane::Interactive);

  /// How many bytes of parsed documents to keep alive with no handles on them.
  /// Lowering it evicts immediately; zero disables retention.
  void set_byte_budget(std::size_t bytes);
  std::size_t byte_budget() const;
  std::size_t bytes_retained() const;

  /// Entries in the table, live and retained.
  std::size_t size() const;

  /// Where markup is read from. Defaults to the global mount table.
  void set_resources(Resources *resources);

  /// Drops everything not currently held. Handles already issued keep working.
  void clear();

private:
  struct State;
  std::unique_ptr<State> state_;
};

} // namespace voidui
