#pragma once

#include <variant>

#include "voidui/core/color.h"
#include "voidui/core/gradient.h"

namespace voidui {

using Brush = std::variant<Color, LinearGradient, ConicGradient>;

} // namespace voidui
