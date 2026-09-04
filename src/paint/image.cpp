#include "voidui/paint/image.h"

#include <atomic>

namespace voidui {

namespace {

std::uint64_t next_image_id() {
  static std::atomic<std::uint64_t> counter{1};
  return counter.fetch_add(1, std::memory_order_relaxed);
}

} // namespace

std::shared_ptr<Image> Image::from_pixels(Blob pixels, int width, int height,
                                          PixelFormat format, int source_width,
                                          int source_height) {
  if (width <= 0 || height <= 0)
    return nullptr;

  const std::size_t expected = static_cast<std::size_t>(width) * height * 4;
  if (pixels.size() < expected)
    return nullptr;

  // Defaulting to the decoded size keeps every caller that does not scale --
  // a generated pattern, a test fixture -- from having to say so twice.
  if (source_width <= 0)
    source_width = width;
  if (source_height <= 0)
    source_height = height;

  return std::shared_ptr<Image>(new Image(std::move(pixels), width, height,
                                          source_width, source_height, format,
                                          next_image_id()));
}

std::shared_ptr<Image> Image::from_rgba8(std::vector<std::uint8_t> pixels, int width,
                                         int height) {
  if (width <= 0 || height <= 0)
    return nullptr;

  const std::size_t expected = static_cast<std::size_t>(width) * height * 4;
  if (pixels.size() < expected)
    return nullptr;

  pixels.resize(expected);
  return from_pixels(Blob::own(std::move(pixels)), width, height,
                     PixelFormat::Rgba8Straight);
}

} // namespace voidui
