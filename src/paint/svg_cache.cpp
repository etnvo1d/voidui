#include "voidui/paint/svg_cache.h"

#include <deque>
#include <unordered_map>
#include <utility>
#include <vector>

namespace voidui {

namespace {

std::uint64_t hash_bytes(std::string_view text) {
  // FNV-1a, as elsewhere in the caches. It identifies a document inside one
  // process; a collision costs a wrong picture rather than anything worse.
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

std::string_view describe(ResourceError error) { return to_string(error); }

} // namespace

// -- The entry ----------------------------------------------------------------

/// One document.
///
/// Every field is UI-thread-only. A worker never touches an entry: it produces
/// a result and posts it, and the posted task -- which runs on the UI thread at
/// the event loop's hand-off point -- writes the outcome down.
struct detail::SvgEntry {
  std::uint64_t key = 0;
  std::string name;

  SvgHandle::State state = SvgHandle::State::Loading;
  std::shared_ptr<const SvgDocument> document;
  std::string error;
  std::size_t bytes = 0;

  std::vector<Invalidator> waiters;
};

SvgHandle::State SvgHandle::state() const {
  return entry_ ? entry_->state : State::Empty;
}

std::shared_ptr<const SvgDocument> SvgHandle::document() const {
  return entry_ ? entry_->document : nullptr;
}

std::string_view SvgHandle::error() const {
  return entry_ ? std::string_view(entry_->error) : std::string_view();
}

// -- SvgSource ----------------------------------------------------------------

SvgSource SvgSource::uri(ResourceUri uri) {
  SvgSource source;
  if (uri.empty() || uri.path().empty())
    return source;

  source.kind_ = Kind::Uri;
  source.key_ = mix(hash_bytes(uri.to_string()), 1);
  source.uri_ = std::move(uri);
  return source;
}

SvgSource SvgSource::uri(std::string_view reference, const ResourceUri &base) {
  const ResourceResult<ResourceUri> resolved =
      base.empty() ? ResourceUri::parse(reference) : base.resolve(reference);
  if (!resolved)
    return SvgSource();
  return uri(*resolved);
}

SvgSource SvgSource::markup(std::string text) {
  SvgSource source;
  if (text.empty())
    return source;

  source.kind_ = Kind::Markup;
  source.key_ = mix(hash_bytes(text), 2);
  source.text_ = std::move(text);
  return source;
}

SvgSource SvgSource::ready(std::shared_ptr<const SvgDocument> document) {
  SvgSource source;
  if (!document)
    return source;

  source.kind_ = Kind::Ready;
  source.key_ = mix(reinterpret_cast<std::uintptr_t>(document.get()), 3);
  source.ready_ = std::move(document);
  return source;
}

std::string SvgSource::display() const {
  return kind_ == Kind::Uri ? uri_.display() : std::string();
}

// -- SvgCache -----------------------------------------------------------------

using Entry = detail::SvgEntry;

struct SvgCache::State {
  /// Weak, so an entry lives exactly as long as somebody is holding it or the
  /// retention ring is. There is no separate holder count and no eviction of a
  /// document that is on screen -- the shared_ptr already says both things.
  std::unordered_map<std::uint64_t, std::weak_ptr<Entry>> entries;

  /// The most recently released documents, kept in case they are wanted again.
  /// A scrolling list drops and re-acquires the same handful of icons
  /// constantly, and re-reading a file for one of them would be a needless
  /// round trip through the blocking lane.
  std::deque<std::shared_ptr<Entry>> retained;
  std::size_t retained_bytes = 0;
  std::size_t budget = 1u << 20;

  Resources *resources = nullptr;

  /// Expired weak entries are cleared when their key is asked for again, which
  /// covers everything that is actually in use. A key nobody ever asks for
  /// again would otherwise keep a table slot forever, so the table is swept
  /// once it has grown well past what is live.
  std::size_t sweep_at = 64;

  Resources &resource_table() {
    return resources ? *resources : Resources::global();
  }

  void retain(std::shared_ptr<Entry> entry) {
    if (budget == 0)
      return;
    retained_bytes += entry->bytes;
    retained.push_back(std::move(entry));
    trim();
  }

  void trim() {
    while (retained_bytes > budget && !retained.empty()) {
      retained_bytes -= retained.front()->bytes;
      retained.pop_front();
    }
  }

  void sweep() {
    if (entries.size() < sweep_at)
      return;
    for (auto it = entries.begin(); it != entries.end();)
      it = it->second.expired() ? entries.erase(it) : std::next(it);
    sweep_at = entries.size() * 2 + 64;
  }
};

SvgCache::SvgCache() : state_(std::make_unique<State>()) {}
SvgCache::~SvgCache() = default;

SvgCache &SvgCache::global() {
  static SvgCache cache;
  return cache;
}

void SvgCache::set_byte_budget(std::size_t bytes) {
  state_->budget = bytes;
  state_->trim();
}

std::size_t SvgCache::byte_budget() const { return state_->budget; }

std::size_t SvgCache::bytes_retained() const { return state_->retained_bytes; }

std::size_t SvgCache::size() const { return state_->entries.size(); }

void SvgCache::set_resources(Resources *resources) {
  state_->resources = resources;
}

void SvgCache::clear() {
  state_->retained.clear();
  state_->retained_bytes = 0;
  state_->sweep_at = 0;
  state_->sweep();
}

namespace {

/// Writes a finished load down and wakes whoever asked for it.
///
/// Free rather than a member because it outlives the call that started it: by
/// the time the parse finishes, the widget that asked may be gone. Nothing here
/// reads the cache -- the entry is reached through the shared_ptr the job
/// carried.
void publish(const std::shared_ptr<Entry> &entry, SvgDocument::Result result) {
  if (entry->state != SvgHandle::State::Loading)
    return;

  if (result.document) {
    entry->state = SvgHandle::State::Ready;
    entry->bytes = result.document->byte_size();
    entry->document = std::move(result.document);
  } else {
    entry->state = SvgHandle::State::Failed;
    entry->error = std::move(result.error);
  }

  const std::vector<Invalidator> waiters = std::move(entry->waiters);
  entry->waiters.clear();
  for (const Invalidator &waiter : waiters)
    waiter.request_layout();
}

void add_waiter(Entry &entry, const Invalidator &invalidator) {
  if (!invalidator)
    return;

  // One wake-up per tree, not per widget: a list of a hundred rows waiting on
  // one icon should queue one frame between them.
  for (const Invalidator &existing : entry.waiters)
    if (existing.tree_key() == invalidator.tree_key())
      return;
  entry.waiters.push_back(invalidator);
}

} // namespace

SvgHandle SvgCache::acquire(const SvgSource &source,
                            const Invalidator &invalidator, async::Lane lane) {
  if (source.empty()) {
    auto entry = std::make_shared<Entry>();
    entry->state = SvgHandle::State::Failed;
    entry->error = "no source";
    return SvgHandle(entry);
  }

  // Already parsed: a detached entry, never in the table and never retained.
  if (source.kind_ == SvgSource::Kind::Ready) {
    auto entry = std::make_shared<Entry>();
    entry->state = SvgHandle::State::Ready;
    entry->document = source.ready_;
    entry->bytes = entry->document->byte_size();
    return SvgHandle(entry);
  }

  state_->sweep();

  if (auto it = state_->entries.find(source.key());
      it != state_->entries.end()) {
    if (std::shared_ptr<Entry> entry = it->second.lock()) {
      if (entry->state == SvgHandle::State::Loading)
        add_waiter(*entry, invalidator);
      return SvgHandle(std::move(entry));
    }
    state_->entries.erase(it);
  }

  auto entry = std::make_shared<Entry>();
  entry->key = source.key();
  entry->name = source.display();
  if (entry->name.empty())
    entry->name = "<markup>";
  entry->state = SvgHandle::State::Loading;
  state_->entries.emplace(source.key(), entry);

  // Markup the caller already holds needs no read, and parsing it is fast
  // enough that a round trip through the pool would cost more latency than it
  // saves work. It resolves before this call returns, so a widget built from a
  // string literal draws on its very first frame.
  if (source.kind_ == SvgSource::Kind::Markup) {
    publish(entry, SvgDocument::parse(source.text_));
    state_->retain(entry);
    return SvgHandle(entry);
  }

  add_waiter(*entry, invalidator);

  const async::UiDispatcher dispatcher = async::current_ui_dispatcher();
  SvgCache::State *cache = state_.get();
  Resources *resources = &state_->resource_table();
  ResourceUri uri = source.uri_;

  // Reading and parsing are two jobs on two lanes for the reason images split
  // them: a file read or a request may block for a long time and must never
  // occupy a CPU worker that visible work is queued behind.
  (void)async::ThreadPool::shared().submit(
      async::Lane::Blocking,
      [entry, dispatcher, cache, resources, uri = std::move(uri),
       lane]() mutable {
        ResourceResult<Blob> opened = resources->open(uri);
        if (!opened) {
          std::string error(describe(opened.error()));
          (void)dispatcher.post([entry, cache, error = std::move(error)]() mutable {
            SvgDocument::Result failure;
            failure.error = std::move(error);
            publish(entry, std::move(failure));
            cache->retain(entry);
          });
          return;
        }

        (void)async::ThreadPool::shared().submit(
            lane, [entry, dispatcher, cache, blob = std::move(*opened)]() mutable {
              SvgDocument::Result result = SvgDocument::parse(blob.text());

              // The markup is the second-largest thing this load holds and
              // nothing needs it once the shapes exist.
              blob = Blob();

              (void)dispatcher.post(
                  [entry, cache, result = std::move(result)]() mutable {
                    publish(entry, std::move(result));
                    cache->retain(entry);
                  });
            });
      });

  return SvgHandle(entry);
}

} // namespace voidui
