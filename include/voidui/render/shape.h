#pragma once

#include "voidui/core/border.h"
#include "voidui/core/geometry.h"

namespace voidui {

struct Quad {
  Point<float> top_left;
  Point<float> top_right;
  Point<float> bottom_right;
  Point<float> bottom_left;
  Radius radius;
};

} // namespace voidui