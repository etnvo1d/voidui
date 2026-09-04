#pragma once

#include <cstdint>
#include <vector>

#include "rhi/device.h"

namespace voidui {

/// A rectangle reserved inside an atlas texture, in texels.
struct AtlasSlot {
  int x = 0;
  int y = 0;
  int width = 0;
  int height = 0;
  bool valid = false;
};

/// A single texture that many small images share.
///
/// Allocation is a shelf packer: rectangles are placed left to right along rows
/// whose height is set by the first entry on them. It wastes a little vertical
/// space compared to a skyline packer, but glyphs and path masks arrive in
/// similar sizes, and the packing cost stays O(rows).
class Atlas {
public:
  /// A texel of padding around every entry keeps bilinear taps from bleeding
  /// across slot boundaries.
  static constexpr int kGutter = 1;

  static std::unique_ptr<Atlas> create(rhi::Device &device, int size,
                                       rhi::TextureFormat format,
                                       int bytes_per_texel, rhi::Filter filter);
  ~Atlas();

  Atlas(const Atlas &) = delete;
  Atlas &operator=(const Atlas &) = delete;

  /// Whether an entry of this size could ever be placed, on a page as empty as
  /// this one will ever be.
  ///
  /// Callers need this to tell "the page is full, recycling it would help" from
  /// "this will never fit, recycling would throw away everything and still
  /// fail". Answering a failed `allocate` with a recycle in the second case is
  /// how the renderer used to wedge itself.
  bool can_fit(int width, int height) const {
    return width > 0 && height > 0 && width + kGutter <= size_ &&
           height + kGutter <= size_;
  }

  /// Reserves space with a one-texel gutter so bilinear sampling of one entry
  /// never picks up its neighbour. Returns an invalid slot when full.
  AtlasSlot allocate(int width, int height);

  /// Queues a slot's pixels for upload. All pending uploads are flushed in one
  /// copy pass by `flush`.
  void stage(const AtlasSlot &slot, const std::uint8_t *pixels, int pitch);

  /// Uploads every staged region. Safe to call with nothing pending.
  bool flush();

  /// Drops all allocations. Callers must forget any slots they still hold.
  void clear();

  rhi::Texture *texture() const { return texture_.get(); }
  rhi::Sampler *sampler() const { return sampler_.get(); }

  /// Bilinear regardless of the page's usual filter. An entry drawn under a
  /// transform no longer lands texel-on-pixel, and point-sampling a coverage
  /// bitmap through a rotation is visibly ragged.
  rhi::Sampler *smooth_sampler() const { return smooth_sampler_.get(); }

  int size() const { return size_; }

  /// Normalised texture coordinates for a slot.
  void uv_for(const AtlasSlot &slot, float out[4]) const {
    const float inv = 1.0f / static_cast<float>(size_);
    out[0] = static_cast<float>(slot.x) * inv;
    out[1] = static_cast<float>(slot.y) * inv;
    out[2] = static_cast<float>(slot.x + slot.width) * inv;
    out[3] = static_cast<float>(slot.y + slot.height) * inv;
  }

private:
  struct Shelf {
    int y = 0;
    int height = 0;
    int used = 0;
  };

  struct Pending {
    AtlasSlot slot;
    std::size_t offset = 0;
    // The staged block covers the entry plus its gutter, clamped to the page.
    int width = 0;
    int height = 0;
  };

  Atlas(rhi::Device &device, std::unique_ptr<rhi::Texture> texture,
        std::unique_ptr<rhi::Sampler> sampler,
        std::unique_ptr<rhi::Sampler> smooth_sampler, int size,
        int bytes_per_texel);

  rhi::Device &device_;
  std::unique_ptr<rhi::Texture> texture_;
  std::unique_ptr<rhi::Sampler> sampler_;
  std::unique_ptr<rhi::Sampler> smooth_sampler_;

  int size_ = 0;
  int bytes_per_texel_ = 1;
  std::vector<Shelf> shelves_;

  std::vector<std::uint8_t> scratch_;
  std::vector<Pending> pending_;
};

} // namespace voidui
