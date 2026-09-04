#pragma once

#include <cstdint>
#include <expected>
#include <memory>
#include <span>
#include <string_view>
#include <vector>

#include "voidui/paint/image.h"

namespace voidui {

enum class ImageError {
  Unsupported, ///< no registered decoder recognises these bytes
  Malformed,   ///< a decoder claimed them and could not finish
  TooLarge,    ///< the declared dimensions exceed the caller's limit
  OutOfMemory,
  Cancelled,

  /// The bytes could not be obtained at all: no such resource, nothing mounted
  /// under that prefix, an unreadable file, a refused request. Distinct from
  /// Unsupported because a widget answers them differently -- a missing avatar
  /// is a placeholder, a corrupt one is worth reporting.
  NotFound,
};

std::string_view to_string(ImageError error);

template <class T> using ImageResult = std::expected<T, ImageError>;

/// What a decoder can report without decoding.
///
/// Worth its own call: a list that has to reserve space for a thousand
/// thumbnails needs their aspect ratios, not their pixels, and every format
/// worth supporting puts the dimensions in a header a few dozen bytes long.
struct ImageInfo {
  int width = 0;
  int height = 0;
  bool has_alpha = false;
};

/// What the caller wants out of a decode.
struct DecodeOptions {
  /// The largest result the caller has any use for, in pixels. Zero on either
  /// axis means "no limit on that axis".
  ///
  /// A ceiling, not a demand. Decoding straight to the size that will be
  /// displayed is the single largest saving available here -- a 4000x3000 JPEG
  /// shown at 200x150 is 48 MB decoded natively and 120 KB decoded to fit --
  /// but formats differ in how cheaply they can do it. JPEG scales by whole
  /// DCT ratios and lands on 1/2, 1/4 or 1/8; PNG cannot scale at all without
  /// resampling afterwards. A decoder is therefore free to return something
  /// larger than asked for, and the result's own `width()`/`height()` are
  /// authoritative. What it may never do is return something *smaller*, which
  /// would silently cost the caller resolution it asked to keep.
  int max_width = 0;
  int max_height = 0;

  /// Refuse anything whose declared size exceeds this, before allocating for
  /// it. Untrusted bytes -- and every remote image is untrusted -- can declare
  /// 64000x64000 in a header a hundred bytes long, and the honest answer is to
  /// fail rather than to try.
  int size_limit = 16384;

  /// Premultiply on the worker that decodes, rather than on the thread that
  /// uploads. The renderer composites premultiplied colour either way; doing it
  /// here costs a pass over pixels already in cache, and doing it there costs a
  /// second full-image buffer at upload time.
  bool premultiply = true;
};

/// One image format, or one library's worth of them.
///
/// Decoders are called from worker threads, concurrently and on any thread, so
/// an implementation must hold no mutable state across a call.
class ImageDecoder {
public:
  virtual ~ImageDecoder() = default;

  /// For diagnostics; also what distinguishes two decoders claiming one format.
  virtual std::string_view name() const = 0;

  /// Whether this decoder claims `prefix`, which is the first few bytes of the
  /// data and may be shorter than any real image. Cheap and allocation-free:
  /// it runs once per registered decoder on every decode.
  virtual bool sniff(std::span<const std::byte> prefix) const = 0;

  virtual ImageResult<ImageInfo> probe(std::span<const std::byte> data) const = 0;

  virtual ImageResult<std::shared_ptr<Image>>
  decode(std::span<const std::byte> data, const DecodeOptions &options) const = 0;
};

/// The registered decoders, tried in order.
///
/// Registration is a startup activity and reads are not: a decode happens on a
/// pool thread, several at a time. So the table is published the same way the
/// resource mount table is -- copy on write, readers never lock, a reader
/// already walking the old table stays valid.
class ImageCodecs {
public:
  ImageCodecs();
  ~ImageCodecs();

  ImageCodecs(const ImageCodecs &) = delete;
  ImageCodecs &operator=(const ImageCodecs &) = delete;

  /// The table a default decode uses. Populated on first touch with whatever
  /// this build has: the platform codec where one exists, nothing otherwise.
  static ImageCodecs &global();

  /// Higher priority is tried first; ties keep registration order. Priority is
  /// what lets an application put a specialised decoder -- an AVIF library, a
  /// codec tuned for its own texture format -- ahead of the platform's.
  void add(std::shared_ptr<ImageDecoder> decoder, int priority = 0);

  ImageResult<ImageInfo> probe(std::span<const std::byte> data) const;

  ImageResult<std::shared_ptr<Image>>
  decode(std::span<const std::byte> data, const DecodeOptions &options = {}) const;

  /// The registered decoders, most preferred first. For diagnostics.
  std::vector<std::string_view> decoders() const;

private:
  struct Table;

  std::shared_ptr<const Table> snapshot_() const;

  struct State;
  std::unique_ptr<State> state_;
};

/// The platform's own decoder, or null where this build has none.
///
/// Windows uses WIC, which is the same machinery Explorer's thumbnails go
/// through: PNG, JPEG, GIF, BMP, TIFF, HEIF and -- since Windows 10 -- WebP,
/// with scaled JPEG decoding for free. Registered into `ImageCodecs::global()`
/// automatically; exposed here so a test can drive it directly.
std::shared_ptr<ImageDecoder> platform_image_decoder();

} // namespace voidui
