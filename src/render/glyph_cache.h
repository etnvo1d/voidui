#pragma once

#include <cstdint>
#include <unordered_map>

#include "render/atlas.h"
#include "voidui/paint/font.h"

namespace voidui {

/// Rasterised glyphs, packed into the shared atlas.
///
/// Glyphs are rendered at their final device size -- not scaled from some
/// canonical size and not stored as distance fields -- because at UI sizes the
/// difference between a hinted 13px bitmap and a resampled one is the
/// difference between crisp text and mush.
class GlyphCache {
public:
  /// Horizontal subpixel positions a glyph is rasterised at. Quantising to
  /// quarters keeps runs evenly spaced without multiplying the cache fourfold
  /// for no visible gain.
  static constexpr int kSubpixelSteps = 4;

  struct Entry {
    AtlasSlot slot;
    int left = 0;   ///< device px from the pen to the bitmap's left edge
    int top = 0;    ///< device px from the baseline up to the bitmap's top edge
    int width = 0;
    int height = 0;

    /// Colour glyphs (emoji) carry their own premultiplied RGBA and live on the
    /// colour page; everything else is coverage on the R8 page.
    bool color = false;
  };

  GlyphCache() = default;

  /// Which shared page ran out of room, when that is why placement failed.
  ///
  /// The caller owns both pages and is the only one that can recycle them --
  /// and it cannot do so the moment it hears about it, because instances
  /// pointing into the page are already queued for this frame. So the failure
  /// is reported rather than acted on.
  enum class PageFull : std::uint8_t { None, Coverage, Color };

  /// Returns null when the glyph could not be rasterised or the page is full.
  /// A blank glyph (a space) yields a zero-sized entry rather than null, so it
  /// is not retried every frame; so does a glyph too large for an empty page,
  /// which no amount of recycling would ever make room for.
  ///
  /// `color` may be null; a colour glyph then simply fails to place rather than
  /// forcing the caller to allocate a page it may never otherwise need.
  const Entry *get(const Font &font, std::uint32_t glyph_id, int device_size,
                   int subpixel, Atlas &coverage, Atlas *color,
                   PageFull *page_full = nullptr);

  void clear() { entries_.clear(); }

private:
  std::unordered_map<std::uint64_t, Entry> entries_;
};

} // namespace voidui
