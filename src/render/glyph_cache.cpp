#include "render/glyph_cache.h"

#include "paint/font_impl.h"

#include FT_OUTLINE_H

#include <vector>

namespace voidui {

namespace {

/// Disjoint bit fields, OR-ed rather than XOR-ed.
///
///     subpixel    bits  0..1   (4 positions)
///     device_size bits  2..15  (14 bits, sizes to 16383)
///     glyph_id    bits 16..35  (20 bits; the format caps glyph ids at 65536)
///     font_id     bits 36..63  (28 bits of a process-wide counter)
///
/// The previous packing XOR-ed overlapping shifts, so `device_size << 4` ran
/// into `glyph_id << 12` once a size reached 256 -- a heading at 128 logical px
/// on a 2x display -- and silently drew a different glyph.
std::uint64_t cache_key(std::uint64_t font_id, std::uint32_t glyph_id, int device_size,
                        int subpixel) {
  return (font_id << 36) | (static_cast<std::uint64_t>(glyph_id & 0xFFFFFu) << 16) |
         (static_cast<std::uint64_t>(device_size & 0x3FFF) << 2) |
         static_cast<std::uint64_t>(subpixel & 0x3);
}

} // namespace

const GlyphCache::Entry *GlyphCache::get(const Font &font, std::uint32_t glyph_id,
                                         int device_size, int subpixel, Atlas &coverage,
                                         Atlas *color, PageFull *page_full) {
  if (device_size <= 0)
    return nullptr;

  const std::uint64_t key = cache_key(font.id(), glyph_id, device_size, subpixel);
  if (auto it = entries_.find(key); it != entries_.end())
    return &it->second;

  FontImpl *impl = font.impl();
  if (!impl || !impl->data || !impl->data->face)
    return nullptr;

  FontData &data = *impl->data;

  // The face is shared between every size drawn from this file, so the pixel
  // size it currently carries is tracked alongside it.
  if (data.ft_pixel_size != device_size) {
    if (FT_Set_Pixel_Sizes(data.face, 0, static_cast<FT_UInt>(device_size)) != 0)
      return nullptr;
    data.ft_pixel_size = device_size;
  }

  // FT_LOAD_COLOR makes FreeType composite a COLR/CPAL glyph's layers, or
  // decode a CBDT strike, into a premultiplied BGRA bitmap. Faces without
  // colour tables are unaffected and still render as coverage.
  const FT_Int32 flags =
      FT_LOAD_DEFAULT | (font.has_color_glyphs() ? FT_LOAD_COLOR : FT_LOAD_NO_BITMAP);

  if (FT_Load_Glyph(data.face, glyph_id, flags) != 0)
    return nullptr;

  FT_GlyphSlot slot = data.face->glyph;

  // Nudge the outline before rendering so the same glyph can land on a
  // fractional pen position without being resampled.
  if (slot->format == FT_GLYPH_FORMAT_OUTLINE && subpixel != 0) {
    const FT_Pos delta = static_cast<FT_Pos>(subpixel) * 64 / kSubpixelSteps;
    FT_Outline_Translate(&slot->outline, delta, 0);
  }

  if (slot->format != FT_GLYPH_FORMAT_BITMAP &&
      FT_Render_Glyph(slot, FT_RENDER_MODE_NORMAL) != 0)
    return nullptr;

  Entry entry;
  entry.left = slot->bitmap_left;
  entry.top = slot->bitmap_top;
  entry.width = static_cast<int>(slot->bitmap.width);
  entry.height = static_cast<int>(slot->bitmap.rows);
  entry.color = slot->bitmap.pixel_mode == FT_PIXEL_MODE_BGRA;

  if (entry.width > 0 && entry.height > 0) {
    Atlas *page = entry.color ? color : &coverage;
    if (!page)
      return nullptr;

    // Bigger than an empty page will ever be. Cache it as unrenderable so the
    // next frame does not pay FreeType for it again, and do not report a full
    // page: recycling would drop every glyph on it and still not fit this one.
    if (!page->can_fit(entry.width, entry.height)) {
      entry.width = 0;
      entry.height = 0;
      return &entries_.emplace(key, entry).first->second;
    }

    entry.slot = page->allocate(entry.width, entry.height);
    if (!entry.slot.valid) {
      if (page_full)
        *page_full = entry.color ? PageFull::Color : PageFull::Coverage;
      return nullptr;
    }

    if (entry.color) {
      // FreeType hands back premultiplied BGRA; the atlas stores RGBA.
      std::vector<std::uint8_t> rgba(static_cast<std::size_t>(entry.width) * entry.height *
                                     4);
      for (int y = 0; y < entry.height; ++y) {
        const std::uint8_t *src = slot->bitmap.buffer + static_cast<std::size_t>(y) *
                                                            slot->bitmap.pitch;
        std::uint8_t *dst = rgba.data() + static_cast<std::size_t>(y) * entry.width * 4;
        for (int x = 0; x < entry.width; ++x) {
          dst[x * 4 + 0] = src[x * 4 + 2];
          dst[x * 4 + 1] = src[x * 4 + 1];
          dst[x * 4 + 2] = src[x * 4 + 0];
          dst[x * 4 + 3] = src[x * 4 + 3];
        }
      }
      page->stage(entry.slot, rgba.data(), entry.width * 4);
    } else {
      page->stage(entry.slot, slot->bitmap.buffer, slot->bitmap.pitch);
    }
  }

  return &entries_.emplace(key, entry).first->second;
}

} // namespace voidui
