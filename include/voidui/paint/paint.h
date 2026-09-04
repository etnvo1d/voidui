#pragma once

#include "voidui/render/brush.h"

namespace voidui {

enum class LineCap {
  Butt,
  Round,
  Square,
};

enum class LineJoin {
  Miter,
  Round,
  Bevel,
};

/// Where a stroke sits relative to the outline it traces.
enum class StrokeAlign {
  Inside,
  Center,
  Outside,
};

/// How a shape is filled. Kept deliberately separate from Pen: `fill_*` and
/// `stroke_*` are independent operations, and a caller that wants both issues
/// both.
struct Paint {
  Brush brush = Color(0, 0, 0);
  float opacity = 1.0f;

  Paint() = default;
  Paint(Brush brush) : brush(std::move(brush)) {}
  Paint(Color color) : brush(color) {}
};

/// How an outline is stroked.
struct Pen {
  float width = 1.0f;
  LineCap cap = LineCap::Butt;
  LineJoin join = LineJoin::Miter;
  float miter_limit = 4.0f;
  StrokeAlign align = StrokeAlign::Center;

  Pen() = default;
  explicit Pen(float width) : width(width) {}
  Pen(float width, StrokeAlign align) : width(width), align(align) {}
};

/// A drop shadow. Rounded-rect shadows are evaluated analytically in the
/// shader; arbitrary paths go through a blurred mask.
///
/// `inset` picks which side of the reference box the shadow lives on, exactly
/// as CSS does. An outer shadow surrounds the box and is knocked out of its
/// interior -- it is never visible through a translucent background. An inner
/// shadow is the complement: it is confined to the box and fades inward from
/// its edge.
struct Shadow {
  Color color = Color(0, 0, 0, 96);
  Point<float> offset{0.0f, 0.0f};
  float blur = 0.0f;   // standard deviation, in logical units
  float spread = 0.0f; // outset applied before blurring
  bool inset = false;

  Shadow() = default;
  Shadow(Color color, Point<float> offset, float blur, float spread = 0.0f,
         bool inset = false)
      : color(color), offset(offset), blur(blur), spread(spread),
        inset(inset) {}
};

} // namespace voidui
