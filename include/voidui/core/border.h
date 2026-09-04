#pragma once

#include "voidui/render/brush.h"

namespace voidui {

struct Radius {
  float left_top = 0.0f;
  float right_top = 0.0f;
  float right_bottom = 0.0f;
  float left_bottom = 0.0f;

  constexpr Radius(float left_top, float right_top, float right_bottom,
                   float left_bottom)
      : left_top(left_top), right_top(right_top), right_bottom(right_bottom),
        left_bottom(left_bottom) {}

  constexpr Radius(float radius)
      : left_top(radius), right_top(radius), right_bottom(radius),
        left_bottom(radius) {}
};

class Border {
public:
  Border() : radius_(0), width_(0), brush_(Color::TRANSPARENT) {}
  Border(Radius radius, float width, const Brush &brush)
      : radius_(radius), width_(width), brush_(brush) {}

  inline static Border solid(float width, const Brush &brush) {
    Border border;
    border.width_ = width;
    border.brush_ = brush;
    return border;
  }

  float get_width() const { return width_; }
  Radius get_radius() const { return radius_; }
  const Brush &get_brush() const { return brush_; }

private:
  Radius radius_;
  float width_;
  Brush brush_;
};

} // namespace voidui
