#pragma once

#include <array>
#include <cstdint>

namespace voidui {

template <typename T> struct Point {
  T x{}, y{};
  constexpr Point() = default;
  constexpr Point(T x, T y) : x(x), y(y) {}
};

template <typename T> struct Size {
  T width{}, height{};
  constexpr Size() = default;
  constexpr Size(T width, T height) : width(width), height(height) {}
};

template <typename T> struct Rect {
  Point<T> origin;
  Size<T> size;
  constexpr Rect() = default;
  constexpr Rect(Point<T> origin, Size<T> size) : origin(origin), size(size) {}
  constexpr Rect(T x, T y, T width, T height)
      : origin(x, y), size(width, height) {}

  bool contains(Point<T> point) const {
    const T left = origin.x;
    const T top = origin.y;
    const T right = left + size.width;
    const T bottom = top + size.height;

    return point.x >= left && point.x < right && point.y >= top &&
           point.y < bottom;
  }

  /// LeftTop, RightTop, RightBottom, LeftBottom
  std::array<Point<T>, 4> corners() const {
    return {Point<T>{origin.x, origin.y},
            Point<T>{origin.x + size.width, origin.y},
            Point<T>{origin.x + size.width, origin.y + size.height},
            Point<T>{origin.x, origin.y + size.height}};
  }
};

template <typename T> struct Spacing {
  T left{}, top{}, right{}, bottom{};
  Spacing() = default;
  Spacing(T left, T top, T right, T bottom)
      : left(left), top(top), right(right), bottom(bottom) {}
  Spacing(T horizontal, T vertical)
      : left(horizontal), top(vertical), right(horizontal), bottom(vertical) {}
  Spacing(T uniform)
      : left(uniform), top(uniform), right(uniform), bottom(uniform) {}

  Spacing expand(const Spacing &other) const {
    return Spacing(left + other.left, top + other.top, right + other.right,
                   bottom + other.bottom);
  }
  Spacing operator+(const Spacing &other) const { return expand(other); }

  Spacing shrink(const Spacing &other) const {
    return Spacing(left - other.left, top - other.top, right - other.right,
                   bottom - other.bottom);
  }
  Spacing operator-(const Spacing &other) const { return shrink(other); }
};

} // namespace voidui
