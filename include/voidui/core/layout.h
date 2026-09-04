#pragma once

#include <algorithm>
#include <cmath>
#include <limits>
#include <variant>

#include "geometry.h"

namespace voidui {

struct Length {
  struct Auto {};
  struct Fixed {
    float value;
  };
  struct Fill {};
  struct Flex {
    uint16_t value;
  };

  using Value = std::variant<Auto, Fixed, Fill, Flex>;

  Value value = Auto{};

  Length() = default;
  Length(Auto v) : value(v) {}
  Length(Fixed v) : value(v) {}
  Length(Fill v) : value(v) {}
  Length(Flex v) : value(v) {}
  Length(float fixed) : value(Fixed{fixed}) {}
};

/// One CSS margin edge. An infinity sentinel keeps the value at four bytes
/// while preserving the distinction between `auto` and every finite fixed
/// margin.
struct MarginValue {
  struct Auto {};

  constexpr MarginValue() = default;
  constexpr MarginValue(float fixed) : value_(fixed) {}
  constexpr MarginValue(Auto)
      : value_(std::numeric_limits<float>::infinity()) {}

  constexpr bool is_auto() const {
    return value_ == std::numeric_limits<float>::infinity();
  }
  constexpr float fixed_or_zero() const { return is_auto() ? 0.0f : value_; }

  constexpr bool operator==(const MarginValue &other) const {
    return (is_auto() && other.is_auto()) || value_ == other.value_;
  }

private:
  float value_ = 0.0f;
};

static_assert(sizeof(MarginValue) == sizeof(float));

inline Spacing<float> resolve_fixed_margin(const Spacing<MarginValue> &margin) {
  return {margin.left.fixed_or_zero(), margin.top.fixed_or_zero(),
          margin.right.fixed_or_zero(), margin.bottom.fixed_or_zero()};
}

struct Margin {
  using Auto = MarginValue::Auto;

  MarginValue top{};
  MarginValue right{};
  MarginValue bottom{};
  MarginValue left{};

  Margin() = default;
  Margin(Auto uniform) : Margin(MarginValue(uniform)) {}
  Margin(MarginValue uniform)
      : top(uniform), right(uniform), bottom(uniform), left(uniform) {}
  Margin(MarginValue vertical, MarginValue horizontal)
      : top(vertical), right(horizontal), bottom(vertical), left(horizontal) {}
  Margin(MarginValue top, MarginValue right, MarginValue bottom,
         MarginValue left)
      : top(top), right(right), bottom(bottom), left(left) {}
};

struct Constraints {
  float min_width;
  float max_width;
  float min_height;
  float max_height;

  constexpr Constraints(float min_width, float max_width, float min_height,
                        float max_height)
      : min_width(min_width), max_width(max_width), min_height(min_height),
        max_height(max_height) {}

  constexpr Constraints(float max_width, float max_height)
      : min_width(0.0f), max_width(max_width), min_height(0.0f),
        max_height(max_height) {}

  static Constraints unconstrained() {
    return {0.0f, std::numeric_limits<float>::infinity(), 0.0f,
            std::numeric_limits<float>::infinity()};
  }

  Constraints &constrain_width(Length width) {
    if (const auto *fixed = std::get_if<Length::Fixed>(&width.value)) {
      float w = std::clamp(fixed->value, min_width, max_width);
      min_width = w;
      max_width = w;
    }
    return *this;
  }

  Constraints &constrain_height(Length height) {
    if (const auto *fixed = std::get_if<Length::Fixed>(&height.value)) {
      float h = std::clamp(fixed->value, min_height, max_height);
      min_height = h;
      max_height = h;
    }
    return *this;
  }

  Constraints shrink(Spacing<float> padding) {
    return Constraints{
        std::max(min_width - padding.left - padding.right, 0.0f),
        std::max(max_width - padding.left - padding.right, 0.0f),
        std::max(min_height - padding.top - padding.bottom, 0.0f),
        std::max(max_height - padding.top - padding.bottom, 0.0f),
    };
  }

  Size<float> resolve(Size<Length> size, Size<float> intrinsic_size) {
    float width, height;

    if (const auto *fixed_w = std::get_if<Length::Fixed>(&size.width.value)) {
      width = std::clamp(fixed_w->value, min_width, max_width);
    } else if (std::isfinite(max_width) &&
               (std::get_if<Length::Fill>(&size.width.value) ||
                std::get_if<Length::Flex>(&size.width.value))) {
      width = max_width;
    } else {
      width = std::clamp(intrinsic_size.width, min_width, max_width);
    }

    if (const auto *fixed_h = std::get_if<Length::Fixed>(&size.height.value)) {
      height = std::clamp(fixed_h->value, min_height, max_height);
    } else if (std::isfinite(max_height) &&
               (std::get_if<Length::Fill>(&size.height.value) ||
                std::get_if<Length::Flex>(&size.height.value))) {
      height = max_height;
    } else {
      height = std::clamp(intrinsic_size.height, min_height, max_height);
    }

    return Size(width, height);
  }
};

} // namespace voidui
