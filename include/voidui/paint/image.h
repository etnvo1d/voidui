#pragma once

#include <cstddef>
#include <cstdint>
#include <memory>
#include <span>
#include <utility>
#include <vector>

#include "voidui/core/resource.h"

namespace voidui {

/// How an image's colour channels relate to its alpha.
///
/// The renderer composites premultiplied colour, so straight pixels have to be
/// converted before they reach a texture. Where that conversion happens is a
/// performance decision, not a correctness one: a decoder running on a worker
/// can hand over pixels that are already premultiplied, and the upload path
/// then stages them as they stand. Pixels built by hand -- a test, a generated
/// pattern -- are far easier to write straight, so both arrive as the same type
/// and the format says which conversion, if any, is still owed.
enum class PixelFormat : std::uint8_t {
  Rgba8Straight,
  Rgba8Premultiplied,
};

/// A CPU-side RGBA8 image.
///
/// Deliberately device-free: the renderer uploads and caches the pixels the
/// first time an image is drawn, keyed by `id()`. That keeps image creation off
/// the render thread's critical path and out of the public API's dependencies.
///
/// `id()` is per-object, not per-content, and the renderer's GPU-side cache is
/// keyed by it. Two Images decoded from the same bytes are therefore two
/// uploads; keeping one object per distinct picture is the loader's job.
class Image {
public:
  /// `pixels` is straight (non-premultiplied) RGBA8, row-major, tightly packed.
  static std::shared_ptr<Image> from_rgba8(std::vector<std::uint8_t> pixels, int width,
                                           int height);

  /// The general form. `pixels` is row-major and tightly packed at four bytes
  /// per texel; the Blob may own its storage, borrow static bytes, or hold a
  /// slice of something refcounted, so a decoder can hand over the buffer it
  /// filled without a copy.
  ///
  /// `source_width` and `source_height` are what the picture measures at full
  /// resolution, which is not the same thing as what was decoded -- see
  /// `source_width()`. Zero means they are the same.
  static std::shared_ptr<Image> from_pixels(Blob pixels, int width, int height,
                                            PixelFormat format,
                                            int source_width = 0,
                                            int source_height = 0);

  int width() const { return width_; }
  int height() const { return height_; }

  /// What the picture measures at full resolution.
  ///
  /// Usually the same as `width()`, and deliberately not always. A decoder
  /// asked to fit a thumbnail box returns a small image, and the difference
  /// between the two is the whole saving -- but the picture's own proportions
  /// and its natural size are still properties of the source, not of whatever
  /// box happened to be on screen when it was first asked for. A widget sizing
  /// itself to its content, or drawing at natural scale, has to read these; a
  /// widget mapping a crop into texture coordinates has to read both and divide.
  int source_width() const { return source_width_; }
  int source_height() const { return source_height_; }

  /// Whether the pixels are the full picture rather than a reduction of it.
  bool is_full_resolution() const {
    return width_ == source_width_ && height_ == source_height_;
  }
  PixelFormat format() const { return format_; }
  std::uint64_t id() const { return id_; }

  /// Tightly packed RGBA8, `width * height * 4` bytes.
  std::span<const std::uint8_t> pixels() const {
    return {reinterpret_cast<const std::uint8_t *>(pixels_.data()), pixels_.size()};
  }

  /// The same bytes, with their keep-alive. An upload that outlives the call
  /// recording it -- a texture copy queued now and flushed once there is a
  /// command buffer -- holds this rather than copying the pixels.
  const Blob &storage() const { return pixels_; }

  /// What holding this image costs in CPU memory.
  std::size_t byte_size() const { return pixels_.size(); }

private:
  Image(Blob pixels, int width, int height, int source_width, int source_height,
        PixelFormat format, std::uint64_t id)
      : pixels_(std::move(pixels)), width_(width), height_(height),
        source_width_(source_width), source_height_(source_height),
        format_(format), id_(id) {}

  Blob pixels_;
  int width_ = 0;
  int height_ = 0;
  int source_width_ = 0;
  int source_height_ = 0;
  PixelFormat format_ = PixelFormat::Rgba8Straight;
  std::uint64_t id_ = 0;
};

} // namespace voidui
