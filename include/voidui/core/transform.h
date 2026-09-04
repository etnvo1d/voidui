#pragma once

#include <algorithm>
#include <cmath>

#include "voidui/core/geometry.h"

namespace voidui {

/// A 2D affine transform, stored as the two rows that matter:
///
///     | a c e |
///     | b d f |
///     | 0 0 1 |
struct Transform {
  float a = 1.0f, b = 0.0f;
  float c = 0.0f, d = 1.0f;
  float e = 0.0f, f = 0.0f;

  constexpr Transform() = default;
  constexpr Transform(float a, float b, float c, float d, float e, float f)
      : a(a), b(b), c(c), d(d), e(e), f(f) {}

  static constexpr Transform translate(float x, float y) {
    return Transform(1.0f, 0.0f, 0.0f, 1.0f, x, y);
  }

  static constexpr Transform scale(float x, float y) {
    return Transform(x, 0.0f, 0.0f, y, 0.0f, 0.0f);
  }

  static Transform rotate(float radians) {
    const float s = std::sin(radians);
    const float c = std::cos(radians);
    return Transform(c, s, -s, c, 0.0f, 0.0f);
  }

  /// `this` applied after `inner`, i.e. the transform of a child whose parent
  /// carries `*this`.
  constexpr Transform concat(const Transform &inner) const {
    return Transform(a * inner.a + c * inner.b, b * inner.a + d * inner.b,
                     a * inner.c + c * inner.d, b * inner.c + d * inner.d,
                     a * inner.e + c * inner.f + e,
                     b * inner.e + d * inner.f + f);
  }

  constexpr Point<float> apply(Point<float> p) const {
    return Point<float>(a * p.x + c * p.y + e, b * p.x + d * p.y + f);
  }

  /// Direction vectors ignore translation.
  constexpr Point<float> apply_vector(Point<float> v) const {
    return Point<float>(a * v.x + c * v.y, b * v.x + d * v.y);
  }

  /// Computes the inverse without allocating or promoting to a general matrix.
  /// Returns false for a collapsed transform such as scale(0).
  bool inverse(Transform &out) const {
    const float determinant = a * d - b * c;
    if (std::abs(determinant) < 1e-8f)
      return false;
    const float reciprocal = 1.0f / determinant;
    out = Transform(d * reciprocal, -b * reciprocal, -c * reciprocal,
                    a * reciprocal, (c * f - d * e) * reciprocal,
                    (b * e - a * f) * reciprocal);
    return true;
  }

  /// True when the transform maps axis-aligned rectangles to axis-aligned
  /// rectangles, which is what lets a clip become a plain scissor rect and a
  /// rounded rect stay on the analytic fast path.
  bool is_axis_aligned() const {
    const float eps = 1e-5f;
    const bool upright = std::abs(b) < eps && std::abs(c) < eps;
    const bool quarter_turn = std::abs(a) < eps && std::abs(d) < eps;
    return upright || quarter_turn;
  }

  /// True when the linear part is identity, so the transform only moves
  /// geometry: no scale, no rotation, no skew.
  ///
  /// This, not `is_identity()`, is the property that decides whether something
  /// can stay crisp. A translation maps the device pixel grid onto itself, so
  /// anything that was aligned to the grid can be aligned again afterwards by
  /// rounding one offset -- glyph bitmaps keep landing texel-on-pixel and box
  /// edges keep landing on pixel boundaries. A scale or a rotation genuinely
  /// leaves the grid and has to resample.
  bool is_translation() const {
    const float eps = 1e-6f;
    return std::abs(a - 1.0f) < eps && std::abs(b) < eps && std::abs(c) < eps &&
           std::abs(d - 1.0f) < eps;
  }

  /// The offset a translation carries. Meaningless unless `is_translation()`.
  constexpr Point<float> translation() const { return Point<float>(e, f); }

  bool is_identity() const {
    const float eps = 1e-6f;
    return is_translation() && std::abs(e) < eps && std::abs(f) < eps;
  }

  /// Uniform scale factor, used to pick a flattening tolerance and to convert
  /// stroke widths into device pixels.
  float approximate_scale() const { return std::sqrt(std::abs(a * d - b * c)); }

  /// Axis-aligned bounding box of the transformed rectangle.
  Rect<float> map_bounds(Rect<float> r) const {
    const Point<float> corners[4]{
        apply(Point<float>(r.origin.x, r.origin.y)),
        apply(Point<float>(r.origin.x + r.size.width, r.origin.y)),
        apply(Point<float>(r.origin.x + r.size.width,
                           r.origin.y + r.size.height)),
        apply(Point<float>(r.origin.x, r.origin.y + r.size.height)),
    };

    float min_x = corners[0].x, max_x = corners[0].x;
    float min_y = corners[0].y, max_y = corners[0].y;

    for (int i = 1; i < 4; ++i) {
      min_x = std::min(min_x, corners[i].x);
      max_x = std::max(max_x, corners[i].x);
      min_y = std::min(min_y, corners[i].y);
      max_y = std::max(max_y, corners[i].y);
    }

    return Rect<float>(min_x, min_y, max_x - min_x, max_y - min_y);
  }
};

} // namespace voidui
