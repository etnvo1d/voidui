#include "voidui/paint/image_cache.h"

#include <SDL3/SDL_stdinc.h>

#include <atomic>
#include <cassert>
#include <cstdio>
#include <list>
#include <mutex>
#include <unordered_map>
#include <utility>

namespace voidui {

namespace {

/// Set VOIDUI_LOG_IMAGE=1 to see every load: what was asked for, what came
/// back, and how big it turned out.
///
/// The image path has several places a picture can fail to appear -- nothing
/// mounted, a read that came back empty, bytes no decoder claims, a decoder
/// that claimed them and could not finish -- and from the outside all of them
/// look the same: an empty box. This is how it says which. It is also how a
/// caller finds out it is decoding a photograph at full size for a thumbnail,
/// which costs nothing visible and a great deal of memory.
bool logging_enabled() {
  static const bool enabled = [] {
    const char *env = SDL_getenv("VOIDUI_LOG_IMAGE");
    return env && SDL_strcmp(env, "0") != 0;
  }();
  return enabled;
}

std::uint64_t hash_bytes(std::string_view text) {
  // FNV-1a. Not cryptographic and does not need to be: it identifies a URI
  // inside one process's cache, and a collision costs a wrong picture rather
  // than anything worse -- which is why the full key mixes in the decode size
  // too, and why `ImageSource::bytes` makes the caller supply its own identity
  // instead of guessing one.
  std::uint64_t hash = 1469598103934665603ull;
  for (const char c : text) {
    hash ^= static_cast<unsigned char>(c);
    hash *= 1099511628211ull;
  }
  return hash;
}

std::uint64_t mix(std::uint64_t seed, std::uint64_t value) {
  seed ^= value + 0x9e3779b97f4a7c15ull + (seed << 6) + (seed >> 2);
  return seed;
}

ImageError to_image_error(ResourceError error) {
  switch (error) {
  case ResourceError::BadUri:
  case ResourceError::NotMounted:
  case ResourceError::NotFound:
  case ResourceError::Unreadable:
  case ResourceError::Denied:
    return ImageError::NotFound;
  }
  return ImageError::NotFound;
}

/// A load's cancellation, shared by the stages it runs in.
///
/// A load is two jobs on two lanes -- read, then decode -- and the entry that
/// owns it lives on the UI thread. Threading the second job's handle back
/// through the UI thread just to store it would cost an event-loop turn per
/// image, so the two stages share this instead: the worker installs its own
/// handle here, and abandonment is one atomic either side can see.
struct LoadJob {
  std::atomic<bool> abandoned{false};
  std::mutex mutex;
  async::JobHandle handle;

  void adopt(async::JobHandle next) {
    std::lock_guard lock(mutex);
    handle = std::move(next);
    if (abandoned.load(std::memory_order_acquire))
      handle.cancel();
  }

  void abandon() {
    abandoned.store(true, std::memory_order_release);
    std::lock_guard lock(mutex);
    handle.cancel();
  }

  bool cancelled() const { return abandoned.load(std::memory_order_acquire); }
};

} // namespace

int image_size_bucket(int pixels) {
  if (pixels <= 0)
    return 0;

  // Below the first step every size shares one bucket: an icon list asking for
  // 17, 20 and 24 pixels wants one entry, not three.
  if (pixels <= 32)
    return 32;

  // Coarser above 1024, where a bucket's worth of extra pixels is a rounding
  // error against the decode it saves.
  const int step = pixels <= 1024 ? 64 : 256;
  return ((pixels + step - 1) / step) * step;
}

// -- ImageSource --------------------------------------------------------------

ImageSource ImageSource::uri(ResourceUri uri) {
  ImageSource source;
  // A URI with no path parses -- an empty reference becomes a `file:` URI
  // naming nothing -- and is still not a picture. Caught here rather than left
  // to fail at read time, so a widget given an empty string reports a bad
  // source rather than spending a pool job discovering it.
  if (uri.empty() || uri.path().empty())
    return source;

  source.kind_ = Kind::Uri;
  source.key_ = hash_bytes(uri.to_string());
  source.uri_ = std::move(uri);
  return source;
}

ImageSource ImageSource::uri(std::string_view reference, const ResourceUri &base) {
  ResourceResult<ResourceUri> parsed =
      base.empty() ? ResourceUri::parse(reference) : base.resolve(reference);
  if (!parsed)
    return ImageSource();
  return ImageSource::uri(std::move(*parsed));
}

ImageSource ImageSource::bytes(Blob encoded, std::uint64_t tag) {
  ImageSource source;
  if (encoded.empty())
    return source;

  source.kind_ = Kind::Bytes;
  // Mixed with a constant so a byte source and a URI source cannot collide on a
  // caller's tag that happens to equal some URI's hash.
  source.key_ = mix(0x8b2f1c3d5e7a9014ull, tag);
  source.encoded_ = std::move(encoded);
  return source;
}

ImageSource ImageSource::ready(std::shared_ptr<const Image> image) {
  ImageSource source;
  if (!image)
    return source;

  source.kind_ = Kind::Ready;
  source.key_ = mix(0x3f5b8d1907c2e64full, image->id());
  source.ready_ = std::move(image);
  return source;
}

std::string ImageSource::display() const {
  return kind_ == Kind::Uri ? uri_.display() : std::string();
}

// -- the entry ----------------------------------------------------------------

/// One picture at one decode size.
///
/// Every field is UI-thread-only. A worker never touches an entry: it produces
/// a result and posts it, and the posted task -- which runs on the UI thread,
/// at the event loop's hand-off point -- is what writes the outcome down. That
/// is why none of this is atomic and none of it is locked.
struct detail::ImageEntry {
  std::uint64_t key = 0;

  /// Kept only so a failure can name itself. An empty box on screen says
  /// nothing about which picture it was, and that is the one thing a reader
  /// needs in order to act.
  std::string name;
  ImageHandle::State state = ImageHandle::State::Loading;

  std::shared_ptr<const Image> image;
  ImageError error = ImageError::Unsupported;
  std::size_t bytes = 0;

  /// Handles pointing here. Not the shared_ptr's own count: an in-flight load
  /// holds a reference too, and "the load is still running" must not read as
  /// "somebody is still waiting for it".
  int holders = 0;

  std::shared_ptr<LoadJob> job;

  /// Woken once, when the load lands. Cleared then -- a widget that wants to
  /// hear about the next load acquires again, and one that has gone away is not
  /// still in the list.
  std::vector<Invalidator> waiters;

  /// Null once the entry is unreachable from its cache: either the cache is
  /// gone, or this is a detached entry that was never in the table.
  ImageCache::State *cache = nullptr;

  /// Where this sits in the retention list, valid only while `holders == 0`
  /// and the state is Ready.
  std::list<ImageEntry *>::iterator retained{};
  bool is_retained = false;
};

using Entry = detail::ImageEntry;

struct ImageCache::State {
  std::unordered_map<std::uint64_t, std::shared_ptr<Entry>> entries;

  /// Ready entries nobody holds, oldest first. Splicing keeps both ends O(1).
  std::list<Entry *> retained;
  std::size_t retained_bytes = 0;
  std::size_t budget = 64ull * 1024 * 1024;

  Resources *resources = nullptr;
  const ImageCodecs *codecs = nullptr;

  Resources &resource_table() const {
    return resources ? *resources : Resources::global();
  }

  const ImageCodecs &codec_list() const {
    return codecs ? *codecs : ImageCodecs::global();
  }

  void release(Entry *entry);
  void retain(Entry *entry);
  void unretain(Entry *entry);
  void trim();
  void forget(Entry *entry);
};

void ImageCache::State::retain(Entry *entry) {
  if (entry->is_retained)
    return;

  retained.push_back(entry);
  entry->retained = std::prev(retained.end());
  entry->is_retained = true;
  retained_bytes += entry->bytes;
}

void ImageCache::State::unretain(Entry *entry) {
  if (!entry->is_retained)
    return;

  retained.erase(entry->retained);
  entry->is_retained = false;
  retained_bytes -= entry->bytes;
}

void ImageCache::State::forget(Entry *entry) {
  unretain(entry);

  auto it = entries.find(entry->key);
  if (it != entries.end() && it->second.get() == entry) {
    entry->cache = nullptr;
    entries.erase(it);
  }
}

void ImageCache::State::release(Entry *entry) {
  switch (entry->state) {
  case ImageHandle::State::Ready:
    // Kept rather than dropped: a row scrolled just off the top of a list is
    // very likely coming back, and the budget is what decides how much of that
    // bet the application is willing to pay for.
    retain(entry);
    trim();
    break;

  case ImageHandle::State::Loading:
    // Nobody is waiting for this any more. Cancelling before the decode starts
    // is the whole point -- a list flung past a hundred rows should not decode
    // the ninety it never showed.
    if (entry->job)
      entry->job->abandon();
    forget(entry);
    break;

  case ImageHandle::State::Failed:
    // Not remembered. A failure is usually transient -- a request that timed
    // out, a file not written yet -- and caching it would make the retry the
    // user expects impossible.
    forget(entry);
    break;

  case ImageHandle::State::Empty:
    break;
  }
}

void ImageCache::State::trim() {
  while (retained_bytes > budget && !retained.empty()) {
    Entry *oldest = retained.front();
    forget(oldest);
  }
}

// -- ImageHandle --------------------------------------------------------------

namespace {

/// Adds `invalidator` to the entry's wake list, once per tree.
///
/// Both halves of that matter. A list of a thousand rows sharing one avatar
/// needs one wake-up between them, not a thousand -- and a single widget that
/// re-measures on every frame of a two-second network load would otherwise add
/// a hundred and twenty entries for itself alone.
void add_waiter(Entry &entry, const Invalidator &invalidator) {
  if (!invalidator)
    return;

  const void *tree = invalidator.tree_key();
  for (const Invalidator &existing : entry.waiters)
    if (existing.tree_key() == tree)
      return;

  entry.waiters.push_back(invalidator);
}

void hold(const std::shared_ptr<Entry> &entry) {
  if (entry)
    ++entry->holders;
}

void unhold(const std::shared_ptr<Entry> &entry) {
  if (!entry)
    return;

  assert(entry->holders > 0);
  if (--entry->holders > 0)
    return;

  if (entry->cache)
    entry->cache->release(entry.get());
}

} // namespace

ImageHandle::ImageHandle(std::shared_ptr<detail::ImageEntry> entry)
    : entry_(std::move(entry)) {
  hold(entry_);
}

ImageHandle::ImageHandle(const ImageHandle &other) : entry_(other.entry_) {
  hold(entry_);
}

ImageHandle::ImageHandle(ImageHandle &&other) noexcept
    : entry_(std::move(other.entry_)) {
  other.entry_.reset();
}

ImageHandle &ImageHandle::operator=(const ImageHandle &other) {
  if (this == &other)
    return *this;

  // Held before released, so assigning a handle onto another one pointing at
  // the same entry cannot momentarily drop it to zero and evict it.
  std::shared_ptr<Entry> next = other.entry_;
  hold(next);
  unhold(entry_);
  entry_ = std::move(next);
  return *this;
}

ImageHandle &ImageHandle::operator=(ImageHandle &&other) noexcept {
  if (this == &other)
    return *this;

  unhold(entry_);
  entry_ = std::move(other.entry_);
  other.entry_.reset();
  return *this;
}

ImageHandle::~ImageHandle() { unhold(entry_); }

ImageHandle::State ImageHandle::state() const {
  return entry_ ? entry_->state : State::Empty;
}

std::shared_ptr<const Image> ImageHandle::image() const {
  return entry_ && entry_->state == State::Ready ? entry_->image : nullptr;
}

ImageError ImageHandle::error() const {
  return entry_ ? entry_->error : ImageError::NotFound;
}

// -- ImageCache ---------------------------------------------------------------

ImageCache::ImageCache() : state_(std::make_unique<State>()) {}

ImageCache::~ImageCache() {
  // Handles outliving the cache are legal -- a widget torn down after it -- so
  // every entry is told its cache is gone rather than left pointing at freed
  // bookkeeping. A load still running is abandoned on the way out.
  for (auto &[key, entry] : state_->entries) {
    entry->cache = nullptr;
    entry->is_retained = false;
    if (entry->state == ImageHandle::State::Loading && entry->job)
      entry->job->abandon();
  }
}

ImageCache &ImageCache::global() {
  static ImageCache cache;
  return cache;
}

void ImageCache::set_resources(Resources *resources) {
  state_->resources = resources;
}

void ImageCache::set_codecs(const ImageCodecs *codecs) { state_->codecs = codecs; }

void ImageCache::set_byte_budget(std::size_t bytes) {
  state_->budget = bytes;
  state_->trim();
}

std::size_t ImageCache::byte_budget() const { return state_->budget; }

std::size_t ImageCache::bytes_retained() const { return state_->retained_bytes; }

std::size_t ImageCache::size() const { return state_->entries.size(); }

void ImageCache::clear() {
  while (!state_->retained.empty())
    state_->forget(state_->retained.front());
}

namespace {

/// The load, from either end of the two stages.
///
/// Written as a free function taking everything by value because it outlives
/// the call that started it: by the time the decode runs, the widget that asked
/// may be gone and the entry may have been abandoned. Nothing here reads the
/// entry -- it only produces a result and posts it.
void publish(const std::shared_ptr<Entry> &entry,
             ImageResult<std::shared_ptr<Image>> result) {
  if (entry->state != ImageHandle::State::Loading)
    return;

  if (logging_enabled()) {
    if (result.has_value() && *result) {
      const Image &image = **result;
      std::printf("voidui: image %s -> %dx%d from %dx%d, %.1f KB\n",
                  entry->name.c_str(), image.width(), image.height(),
                  image.source_width(), image.source_height(),
                  image.byte_size() / 1024.0);
    } else {
      const std::string_view why =
          to_string(result.has_value() ? ImageError::Malformed : result.error());
      std::printf("voidui: image %s FAILED: %.*s\n", entry->name.c_str(),
                  static_cast<int>(why.size()), why.data());
    }
    std::fflush(stdout);
  }

  if (result.has_value() && *result) {
    entry->state = ImageHandle::State::Ready;
    entry->image = std::move(*result);
    entry->bytes = entry->image->byte_size();
  } else {
    entry->state = ImageHandle::State::Failed;
    entry->error = result.has_value() ? ImageError::Malformed : result.error();
  }

  entry->job.reset();

  // Taken by move first: an invalidator's post can run nothing here, but the
  // list must be empty either way, and a waiter added by a later acquire
  // belongs to the next load rather than this one.
  const std::vector<Invalidator> waiters = std::move(entry->waiters);
  entry->waiters.clear();
  for (const Invalidator &waiter : waiters)
    waiter.request_layout();

  // A result that landed with nobody waiting still has to be accounted for, or
  // a prefetch would sit outside the budget forever.
  if (entry->holders == 0 && entry->cache)
    entry->cache->release(entry.get());
}

} // namespace

ImageHandle ImageCache::acquire(const ImageSource &source,
                                const ImageRequest &request) {
  if (source.empty()) {
    auto entry = std::make_shared<Entry>();
    entry->state = ImageHandle::State::Failed;
    entry->error = ImageError::NotFound;
    return ImageHandle(entry);
  }

  // Already-decoded pixels are not a load and not cache traffic. The entry is
  // detached -- never in the table, never retained, never evicted -- so a
  // generated image costs one allocation and no bookkeeping.
  if (source.kind_ == ImageSource::Kind::Ready) {
    auto entry = std::make_shared<Entry>();
    entry->state = ImageHandle::State::Ready;
    entry->image = source.ready_;
    entry->bytes = source.ready_->byte_size();
    return ImageHandle(entry);
  }

  const int width = image_size_bucket(request.max_width);
  const int height = image_size_bucket(request.max_height);

  std::uint64_t key = source.key();
  key = mix(key, static_cast<std::uint64_t>(width));
  key = mix(key, static_cast<std::uint64_t>(height));

  if (auto it = state_->entries.find(key); it != state_->entries.end()) {
    const std::shared_ptr<Entry> &entry = it->second;

    // Coming back from retention: it was unheld a moment ago and is wanted
    // again, which is exactly the bet retention was placed on.
    state_->unretain(entry.get());

    // A second caller for a load already running attaches to it rather than
    // starting another. This is the line that keeps a list of a hundred rows
    // sharing one avatar down to one read, one decode and one upload.
    if (entry->state == ImageHandle::State::Loading)
      add_waiter(*entry, request.invalidator);

    return ImageHandle(entry);
  }

  auto entry = std::make_shared<Entry>();
  entry->key = key;
  // Only a URI names itself. Bytes the application supplied have no name worth
  // printing, but a log line that says nothing at all is worse than one that
  // says which kind of source it was.
  entry->name = source.display();
  if (entry->name.empty())
    entry->name = "<bytes>";
  entry->state = ImageHandle::State::Loading;
  entry->cache = state_.get();
  entry->job = std::make_shared<LoadJob>();
  add_waiter(*entry, request.invalidator);

  state_->entries.emplace(key, entry);

  DecodeOptions options;
  options.max_width = width;
  options.max_height = height;

  const async::UiDispatcher dispatcher = async::current_ui_dispatcher();
  const std::shared_ptr<LoadJob> job = entry->job;
  const ImageCodecs *codecs = &state_->codec_list();

  // Reading and decoding are two jobs on two lanes on purpose. A file read or a
  // request may block for a long time and must never occupy a CPU worker that
  // a visible decode is queued behind; a decode is CPU-bound and wants the lane
  // the caller chose. Splitting them is also where cancellation gets its teeth:
  // a load abandoned while its bytes are still arriving never decodes at all.
  Blob encoded = source.encoded_;
  ResourceUri uri = source.uri_;
  Resources *resources = &state_->resource_table();
  const bool needs_read = source.kind_ == ImageSource::Kind::Uri;
  const async::Lane decode_lane = request.lane;

  async::JobHandle read = async::ThreadPool::shared().submit_cancellable(
      async::Lane::Blocking,
      [entry, job, dispatcher, codecs, options, encoded = std::move(encoded),
       uri = std::move(uri), resources, needs_read,
       decode_lane](async::CancelToken token) mutable {
        if (token.stop_requested() || job->cancelled())
          return;

        ImageError read_error = ImageError::NotFound;
        if (needs_read) {
          ResourceResult<Blob> opened = resources->open(uri);
          if (!opened) {
            read_error = to_image_error(opened.error());
            encoded = Blob();
          } else {
            encoded = std::move(*opened);
          }
        }

        if (token.stop_requested() || job->cancelled())
          return;

        if (encoded.empty()) {
          (void)dispatcher.post([entry, read_error, job] {
            if (job->cancelled())
              return;
            publish(entry, std::unexpected(read_error));
          });
          return;
        }

        async::JobHandle decode = async::ThreadPool::shared().submit_cancellable(
            decode_lane, [entry, job, dispatcher, codecs, options,
                          encoded = std::move(encoded)](
                             async::CancelToken decode_token) mutable {
              if (decode_token.stop_requested() || job->cancelled())
                return;

              ImageResult<std::shared_ptr<Image>> decoded =
                  codecs->decode(encoded.bytes(), options);

              // Released here rather than at the end of the frame: for a large
              // photograph the encoded bytes are the second-largest thing this
              // load ever holds, and nothing needs them once pixels exist.
              encoded = Blob();

              if (decode_token.stop_requested() || job->cancelled())
                return;

              (void)dispatcher.post(
                  [entry, job, decoded = std::move(decoded)]() mutable {
                    if (job->cancelled())
                      return;
                    publish(entry, std::move(decoded));
                  });
            });

        job->adopt(std::move(decode));
      });

  job->adopt(std::move(read));

  return ImageHandle(entry);
}

void ImageCache::prefetch(const ImageSource &source, const ImageRequest &request) {
  ImageRequest prefetch_request = request;
  // A prefetch has no frame waiting on it, so it neither wakes one nor competes
  // with a decode that does.
  prefetch_request.invalidator = {};
  if (prefetch_request.lane == async::Lane::Interactive)
    prefetch_request.lane = async::Lane::Background;

  // The handle is dropped immediately, which routes the entry straight through
  // `release`: a finished prefetch lands in retention, and one nobody ever
  // wanted is cancelled by the next thing that needs the budget.
  (void)acquire(source, prefetch_request);
}

} // namespace voidui
