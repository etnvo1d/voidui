#pragma once

#include <cstddef>
#include <cstdint>
#include <memory>
#include <string>
#include <string_view>
#include <vector>

#include "voidui/core/async/thread_pool.h"
#include "voidui/core/invalidation.h"
#include "voidui/core/resource.h"
#include "voidui/paint/image_codec.h"

namespace voidui {

namespace detail {
/// One picture at one decode size, defined where the cache is implemented.
struct ImageEntry;
} // namespace detail

/// Where an image's bytes come from.
///
/// A URI covers the three cases that look different from the outside and are
/// the same from here: a file being edited during development, a resource baked
/// into the binary, and something fetched over the network all arrive as bytes
/// through the mount table, and which one a `res://` name resolves to is a
/// deployment decision rather than a widget's business.
///
/// The remaining two cases are the ones a URI cannot express: bytes the
/// application already holds, and an image it has already decoded.
class ImageSource {
public:
  ImageSource() = default;

  static ImageSource uri(ResourceUri uri);

  /// Parses `reference`, resolving it against `base` the way a stylesheet's
  /// `url()` resolves. An unparseable reference yields an empty source, which
  /// loads as a failure rather than throwing at the call site.
  static ImageSource uri(std::string_view reference, const ResourceUri &base = {});

  /// Encoded bytes the caller already has -- an archive it unpacked, a payload
  /// it received. `tag` is the cache identity: two calls with the same tag are
  /// the same picture and are decoded once. Distinct content under one tag is
  /// the one way to make this cache lie, so a hash of the bytes is the safe
  /// choice when nothing better is at hand.
  static ImageSource bytes(Blob encoded, std::uint64_t tag);

  /// Pixels that are already decoded. Nothing is loaded, cached or evicted; the
  /// handle is ready the moment it is acquired. For generated images and for
  /// tests.
  static ImageSource ready(std::shared_ptr<const Image> image);

  bool empty() const { return kind_ == Kind::Empty; }

  /// Stable identity, and half of the cache key. Does not include the decode
  /// size -- the same picture wanted at two sizes is two entries.
  std::uint64_t key() const { return key_; }

  /// For diagnostics. Empty for anything that did not come from a URI.
  std::string display() const;

  const ResourceUri &resource_uri() const { return uri_; }

private:
  enum class Kind : std::uint8_t { Empty, Uri, Bytes, Ready };

  friend class ImageCache;

  Kind kind_ = Kind::Empty;
  std::uint64_t key_ = 0;
  ResourceUri uri_;
  Blob encoded_;
  std::shared_ptr<const Image> ready_;
};

/// What a caller wants loaded, beyond which picture.
struct ImageRequest {
  /// The box the result has to fit, in *pixels* -- a logical size already
  /// multiplied by the display scale. Zero on an axis means unbounded there.
  ///
  /// This is the single largest saving in the image path and the reason the
  /// decode size is part of the cache key rather than a hint: a photograph
  /// shown as a thumbnail should never exist at full size, not even briefly.
  ///
  /// Rounded up to a bucket before it is used, so the decode is at least this
  /// large and usually a little larger -- see `image_size_bucket`. Asking for
  /// 200 and asking for 201 are the same request, which is what keeps a window
  /// being dragged from re-decoding on every frame of the drag.
  int max_width = 0;
  int max_height = 0;

  /// Which pool lane the work runs on. Visible images want Interactive; a
  /// prefetch for rows below the fold wants Background, so it cannot delay one.
  /// Reading the bytes always happens on Blocking regardless -- a file read or
  /// a request must never occupy a CPU worker.
  async::Lane lane = async::Lane::Interactive;

  /// Woken when the load finishes. Safe to leave empty for a fire-and-forget
  /// prefetch, which wants the pixels cached and no frame drawn for them.
  Invalidator invalidator;
};

/// Rounds a decode extent up to a shared bucket.
///
/// Without this the decode size is a continuous function of the layout box, and
/// a window being dragged wider asks for a different size -- and therefore a
/// different cache entry, and a fresh decode -- on every frame of the drag.
/// Rounding up rather than to nearest keeps the result at least as large as
/// asked for, so bucketing never costs resolution.
int image_size_bucket(int pixels);

/// One caller's interest in one image.
///
/// Copyable and cheap. While any handle to an entry exists the entry is live:
/// its pixels are held, and a load still running is not cancelled. When the
/// last one goes the entry falls back to the cache's own budget, which decides
/// whether to keep the pixels for the next caller or let them go.
///
/// UI thread only, including copying and destroying one. Holding an image is a
/// statement about what is on screen, and the cache's bookkeeping is single
/// threaded for the same reason the widget tree is. A worker that needs pixels
/// should be handed the `std::shared_ptr<const Image>`, which is free to travel.
class ImageHandle {
public:
  enum class State : std::uint8_t {
    Empty,   ///< nothing was asked for
    Loading, ///< bytes are being read, or pixels decoded
    Ready,
    Failed,
  };

  ImageHandle() = default;
  ImageHandle(const ImageHandle &other);
  ImageHandle(ImageHandle &&other) noexcept;
  ImageHandle &operator=(const ImageHandle &other);
  ImageHandle &operator=(ImageHandle &&other) noexcept;
  ~ImageHandle();

  State state() const;
  bool loading() const { return state() == State::Loading; }
  bool ready() const { return state() == State::Ready; }
  bool failed() const { return state() == State::Failed; }

  /// The decoded pixels, or null unless ready.
  std::shared_ptr<const Image> image() const;

  /// Why it failed. Meaningless unless `failed()`.
  ImageError error() const;

private:
  friend class ImageCache;

  /// Takes the reference with it, so no call site can construct a handle and
  /// forget to count it.
  explicit ImageHandle(std::shared_ptr<detail::ImageEntry> entry);

  std::shared_ptr<detail::ImageEntry> entry_;
};

/// The process's decoded images, keyed by picture and decode size.
///
/// Three things live here that a widget must not do for itself.
///
/// Deduplication is the first and the one that matters in a list. A hundred
/// rows showing one avatar ask a hundred times; the first acquires, the rest
/// attach to the load already running. Without it the file is read a hundred
/// times, decoded a hundred times, and -- because the renderer's GPU cache is
/// keyed by `Image::id()`, which is per object -- uploaded a hundred times too.
///
/// Retention is the second. A row scrolled just off the top of a list will very
/// likely be back, so its pixels are kept after its last handle goes, up to a
/// byte budget, and the oldest are dropped when that budget is reached.
///
/// Cancellation is the third. A list scrolled quickly asks for far more images
/// than it ends up showing, and a load nobody is waiting for any more should
/// stop before it decodes rather than after.
class ImageCache {
public:
  ImageCache();
  ~ImageCache();

  ImageCache(const ImageCache &) = delete;
  ImageCache &operator=(const ImageCache &) = delete;

  static ImageCache &global();

  /// Starts the load if it is not already running or done, and returns a handle
  /// to it. Cheap to call every layout pass: the common case is a hash lookup.
  ///
  /// UI thread only. The handle's state changes only at the event loop's
  /// hand-off point, so a state read during layout stays true for that whole
  /// pass and cannot flip between measuring a widget and drawing it.
  ImageHandle acquire(const ImageSource &source, const ImageRequest &request);

  /// Loads without holding on to the result: the pixels land in the cache and
  /// are subject to its budget like anything else. For rows below the fold.
  void prefetch(const ImageSource &source, const ImageRequest &request);

  /// How much decoded imagery to keep alive with no handles on it. Lowering it
  /// evicts immediately. Zero disables retention, so pixels live exactly as
  /// long as someone is holding them.
  void set_byte_budget(std::size_t bytes);
  std::size_t byte_budget() const;

  /// Held by entries with no handles left -- what retention is currently
  /// costing. Pixels a widget is holding are its own cost and are not counted.
  std::size_t bytes_retained() const;

  /// Entries in the table, live and retained.
  std::size_t size() const;

  /// Where bytes are read from and what decodes them. Defaults to the global
  /// mount table and the global codec list; an application with isolation
  /// requirements -- tests, a plugin host -- can point a cache of its own
  /// somewhere else.
  void set_resources(Resources *resources);
  void set_codecs(const ImageCodecs *codecs);

  /// Drops everything not currently held. Handles already issued keep working.
  void clear();

private:
  friend struct detail::ImageEntry;

  struct State;
  std::unique_ptr<State> state_;
};

} // namespace voidui
