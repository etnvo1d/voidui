#include "render/atlas.h"

#include <SDL3/SDL_log.h>

#include <algorithm>
#include <cstring>

namespace voidui {

Atlas::Atlas(rhi::Device &device, std::unique_ptr<rhi::Texture> texture,
             std::unique_ptr<rhi::Sampler> sampler,
             std::unique_ptr<rhi::Sampler> smooth_sampler, int size,
             int bytes_per_texel)
    : device_(device), texture_(std::move(texture)),
      sampler_(std::move(sampler)), smooth_sampler_(std::move(smooth_sampler)),
      size_(size), bytes_per_texel_(bytes_per_texel) {}

Atlas::~Atlas() = default;

std::unique_ptr<Atlas> Atlas::create(rhi::Device &device, int size,
                                     rhi::TextureFormat format,
                                     int bytes_per_texel, rhi::Filter filter) {
  std::unique_ptr<rhi::Texture> texture =
      device.create_texture(static_cast<std::uint32_t>(size),
                            static_cast<std::uint32_t>(size), format);
  if (!texture) {
    SDL_Log("voidui: atlas texture creation failed");
    return nullptr;
  }

  std::unique_ptr<rhi::Sampler> sampler = device.create_sampler(filter);
  std::unique_ptr<rhi::Sampler> smooth =
      device.create_sampler(rhi::Filter::Linear);
  if (!sampler || !smooth) {
    SDL_Log("voidui: atlas sampler creation failed");
    return nullptr;
  }

  return std::unique_ptr<Atlas>(new Atlas(device, std::move(texture),
                                          std::move(sampler), std::move(smooth),
                                          size, bytes_per_texel));
}

AtlasSlot Atlas::allocate(int width, int height) {
  AtlasSlot slot;
  if (!can_fit(width, height))
    return slot;

  const int padded_w = width + kGutter;
  const int padded_h = height + kGutter;

  for (Shelf &shelf : shelves_) {
    // Only reuse a shelf that is tall enough but not wastefully taller.
    if (padded_h > shelf.height || shelf.height > padded_h * 2)
      continue;
    if (shelf.used + padded_w > size_)
      continue;

    slot = {shelf.used, shelf.y, width, height, true};
    shelf.used += padded_w;
    return slot;
  }

  const int next_y =
      shelves_.empty() ? 0 : shelves_.back().y + shelves_.back().height;
  if (next_y + padded_h > size_)
    return slot;

  shelves_.push_back(Shelf{next_y, padded_h, padded_w});
  slot = {0, next_y, width, height, true};
  return slot;
}

void Atlas::stage(const AtlasSlot &slot, const std::uint8_t *pixels,
                  int pitch) {
  if (!slot.valid || !pixels)
    return;

  // The staged block runs one texel past the entry's right and bottom edges,
  // and that extra row and column go out as zeroes.
  //
  // The packer has always reserved that gutter, but nothing ever wrote it: the
  // texels held whatever the texture was created with -- undefined in both D3D
  // and Vulkan -- and, after a page was recycled, the pixels of whatever used
  // to sit there. Bilinear taps at an entry's edge read exactly those texels,
  // so a scaled image picked up a stripe of its old neighbour. An entry's left
  // and top gutter is the right and bottom gutter of the entry before it on the
  // shelf, so clearing two sides here clears all four in practice; at the page
  // border there is no neighbour and the sampler clamps.
  const int width = std::min(slot.width + kGutter, size_ - slot.x);
  const int height = std::min(slot.height + kGutter, size_ - slot.y);

  const std::size_t row_bytes =
      static_cast<std::size_t>(width) * bytes_per_texel_;
  const std::size_t offset = scratch_.size();
  scratch_.resize(offset + row_bytes * static_cast<std::size_t>(height), 0);

  const std::size_t copy_bytes =
      static_cast<std::size_t>(slot.width) * bytes_per_texel_;
  for (int y = 0; y < slot.height; ++y) {
    std::memcpy(scratch_.data() + offset +
                    static_cast<std::size_t>(y) * row_bytes,
                pixels + static_cast<std::size_t>(y) * pitch, copy_bytes);
  }

  pending_.push_back(Pending{slot, offset, width, height});
}

bool Atlas::flush() {
  if (pending_.empty())
    return true;

  std::vector<rhi::TextureUpload> uploads;
  uploads.reserve(pending_.size());
  for (const Pending &item : pending_) {
    uploads.push_back(
        {static_cast<std::uint32_t>(item.slot.x),
         static_cast<std::uint32_t>(item.slot.y),
         static_cast<std::uint32_t>(item.width),
         static_cast<std::uint32_t>(item.height),
         static_cast<std::uint32_t>(item.offset),
         static_cast<std::uint32_t>(item.width * bytes_per_texel_)});
  }

  const bool uploaded = device_.upload_texture(
      *texture_, scratch_.data(), static_cast<std::uint32_t>(scratch_.size()),
      uploads.data(), static_cast<std::uint32_t>(uploads.size()));

  scratch_.clear();
  pending_.clear();
  return uploaded;
}

void Atlas::clear() {
  shelves_.clear();
  scratch_.clear();
  pending_.clear();
}

} // namespace voidui
