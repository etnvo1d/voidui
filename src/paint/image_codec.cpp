#include "voidui/paint/image_codec.h"

#include <algorithm>
#include <atomic>
#include <mutex>
#include <utility>

namespace voidui {

std::string_view to_string(ImageError error) {
  switch (error) {
  case ImageError::Unsupported:
    return "unsupported image format";
  case ImageError::Malformed:
    return "malformed image data";
  case ImageError::TooLarge:
    return "image exceeds the size limit";
  case ImageError::OutOfMemory:
    return "out of memory decoding image";
  case ImageError::Cancelled:
    return "image decode cancelled";
  case ImageError::NotFound:
    return "image not found";
  }
  return "unknown image error";
}

namespace {

/// Enough for every container header worth sniffing. PNG needs 8, JPEG 3,
/// GIF 6, RIFF/WebP 12, and the ISO base media brands HEIF and AVIF sit at
/// offset 4 of an `ftyp` box.
constexpr std::size_t kSniffBytes = 32;

struct Entry {
  std::shared_ptr<ImageDecoder> decoder;
  int priority = 0;
  std::uint64_t sequence = 0;
};

} // namespace

struct ImageCodecs::Table {
  std::vector<Entry> entries; ///< already ordered, most preferred first
};

struct ImageCodecs::State {
  std::atomic<std::shared_ptr<const Table>> table;
  std::mutex mutex;
  std::uint64_t next_sequence = 1;
};

ImageCodecs::ImageCodecs() : state_(std::make_unique<State>()) {
  state_->table.store(std::make_shared<const Table>());
}

ImageCodecs::~ImageCodecs() = default;

ImageCodecs &ImageCodecs::global() {
  static ImageCodecs codecs;

  // Populated beside the construction rather than inside it, so `global` stays
  // the only thing that knows the default table exists at all. Both run under
  // the same guard, so a decode racing the first call still sees the platform
  // decoder registered.
  static const bool populated = [] {
    if (std::shared_ptr<ImageDecoder> platform = platform_image_decoder())
      codecs.add(std::move(platform));
    return true;
  }();
  (void)populated;

  return codecs;
}

std::shared_ptr<const ImageCodecs::Table> ImageCodecs::snapshot_() const {
  return state_->table.load();
}

void ImageCodecs::add(std::shared_ptr<ImageDecoder> decoder, int priority) {
  if (!decoder)
    return;

  std::lock_guard lock(state_->mutex);

  auto next = std::make_shared<Table>(*snapshot_());
  next->entries.push_back(
      Entry{std::move(decoder), priority, state_->next_sequence++});

  // Stable on the sequence rather than std::sort's unspecified order, so two
  // decoders registered at the same priority keep the order they were added in
  // -- the only thing an application can use to break the tie deliberately.
  std::stable_sort(next->entries.begin(), next->entries.end(),
                   [](const Entry &a, const Entry &b) {
                     if (a.priority != b.priority)
                       return a.priority > b.priority;
                     return a.sequence < b.sequence;
                   });

  state_->table.store(std::shared_ptr<const Table>(std::move(next)));
}

std::vector<std::string_view> ImageCodecs::decoders() const {
  const std::shared_ptr<const Table> table = snapshot_();

  std::vector<std::string_view> names;
  names.reserve(table->entries.size());
  for (const Entry &entry : table->entries)
    names.push_back(entry.decoder->name());
  return names;
}

ImageResult<ImageInfo> ImageCodecs::probe(std::span<const std::byte> data) const {
  if (data.empty())
    return std::unexpected(ImageError::Unsupported);

  const std::shared_ptr<const Table> table = snapshot_();
  const std::span<const std::byte> prefix =
      data.subspan(0, std::min(data.size(), kSniffBytes));

  for (const Entry &entry : table->entries) {
    if (!entry.decoder->sniff(prefix))
      continue;
    return entry.decoder->probe(data);
  }

  return std::unexpected(ImageError::Unsupported);
}

ImageResult<std::shared_ptr<Image>>
ImageCodecs::decode(std::span<const std::byte> data,
                    const DecodeOptions &options) const {
  if (data.empty())
    return std::unexpected(ImageError::Unsupported);

  const std::shared_ptr<const Table> table = snapshot_();
  const std::span<const std::byte> prefix =
      data.subspan(0, std::min(data.size(), kSniffBytes));

  // The first decoder that claims the bytes owns the outcome, including its
  // failure. Falling through to the next one on error would turn a corrupt PNG
  // into an "unsupported format" report and hide which stage actually broke.
  for (const Entry &entry : table->entries) {
    if (!entry.decoder->sniff(prefix))
      continue;
    return entry.decoder->decode(data, options);
  }

  return std::unexpected(ImageError::Unsupported);
}

} // namespace voidui
