#pragma once

#include <cstdint>
#include <vector>

#include "voidui/core/transform.h"
#include "voidui/paint/paint.h"
#include "voidui/paint/path.h"

namespace voidui {

/// A single-channel coverage bitmap in device pixels.
struct Mask {
  int width = 0;
  int height = 0;
  int origin_x = 0; ///< device x of pixel column 0
  int origin_y = 0; ///< device y of pixel row 0
  std::vector<std::uint8_t> pixels;

  bool empty() const { return width <= 0 || height <= 0; }
};

/// Rasterises a path into an exact-area coverage mask.
///
/// Coverage is accumulated as signed area per pixel and prefix-summed along
/// each row, which yields the true fractional winding number at every pixel --
/// 256 levels of antialiasing, correct for self-intersecting contours, and
/// identical in quality to what a font rasteriser produces. That is the reason
/// paths take this route rather than being triangulated and multisampled: 4x
/// MSAA would offer five levels where this offers 256.
Mask rasterize_path(const Path &path, const Transform &to_device, FillRule rule);

/// Converts a stroke into the region it covers, so the same fill rasteriser can
/// draw it. Joins, caps and the stroke alignment are all resolved here.
Path stroke_to_fill(const Path &path, const Pen &pen, float tolerance);

// `flatten_path` is declared in <voidui/paint/path.h> -- dashing needs it too,
// and one flattener is enough. It is implemented here.

} // namespace voidui
