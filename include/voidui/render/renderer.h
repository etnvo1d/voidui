#pragma once

#include "voidui/core/color.h"
#include "voidui/paint/display_list.h"

namespace voidui {

/// Turns a recorded frame into pixels.
///
/// The display list arrives whole rather than through per-primitive calls, so
/// the backend is free to reorder, batch and fuse commands. In particular an
/// adjacent fill/stroke pair over the same geometry collapses into one shading
/// pass -- without that, the two coverages get combined by the fixed-function
/// blender, which tints thin borders wherever their coverage is partial.
class Renderer {
public:
  virtual ~Renderer() = default;

  virtual void resize(int pixel_width, int pixel_height, float logical_width,
                      float logical_height, float display_scale) = 0;

  virtual void render(const DisplayList &list, Color clear_color) = 0;
};

} // namespace voidui
