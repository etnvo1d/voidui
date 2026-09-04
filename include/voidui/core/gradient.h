#pragma once

#include <array>
#include <cstdint>
#include <initializer_list>
#include <memory>
#include <span>
#include <utility>

#include "voidui/core/color.h"
#include "voidui/core/geometry.h"

namespace voidui {

enum class GradientStopUnit : std::uint8_t {
  Fraction,
  Length,
  Unspecified,
};

/// One color on a gradient line. Fractions use the CSS 0..1 gradient-line
/// space; lengths are logical pixels and unspecified positions are resolved by
/// the CSS color-stop fixup algorithm once the painted box is known.
struct GradientStop {
  Color color;
  float position = 0.0f;
  GradientStopUnit unit = GradientStopUnit::Fraction;

  constexpr GradientStop(Color color, float fraction)
      : color(color), position(fraction), unit(GradientStopUnit::Fraction) {}

  constexpr explicit GradientStop(Color color)
      : color(color), unit(GradientStopUnit::Unspecified) {}

  static constexpr GradientStop length(Color color, float logical_pixels) {
    GradientStop stop(color, logical_pixels);
    stop.unit = GradientStopUnit::Length;
    return stop;
  }
};

class LinearGradient {
public:
  enum class Geometry : std::uint8_t { Points, CssAngle, CssDirection };

  enum class Direction : std::uint8_t {
    Top,
    TopRight,
    Right,
    BottomRight,
    Bottom,
    BottomLeft,
    Left,
    TopLeft,
  };

  /// `start` and `end` are in the painted shape's local 0..1 coordinate
  /// space. The defaults match CSS `to bottom`.
  LinearGradient(Color start_color, Color end_color, Point<float> start,
                 Point<float> end);

  LinearGradient(Color start_color, Color end_color)
      : LinearGradient(start_color, end_color, Point<float>(0.5f, 0.0f),
                       Point<float>(0.5f, 1.0f)) {}

  LinearGradient(Point<float> start, Point<float> end,
                 std::span<const GradientStop> stops);

  LinearGradient(Point<float> start, Point<float> end,
                 std::initializer_list<GradientStop> stops)
      : LinearGradient(
            start, end,
            std::span<const GradientStop>(stops.begin(), stops.size())) {}

  static LinearGradient css_angle(float angle_radians,
                                  std::span<const GradientStop> stops);
  static LinearGradient css_angle(float angle_radians,
                                  std::initializer_list<GradientStop> stops) {
    return css_angle(angle_radians, std::span<const GradientStop>(
                                        stops.begin(), stops.size()));
  }
  static LinearGradient css_direction(Direction direction,
                                      std::span<const GradientStop> stops);
  static LinearGradient
  css_direction(Direction direction,
                std::initializer_list<GradientStop> stops) {
    return css_direction(
        direction, std::span<const GradientStop>(stops.begin(), stops.size()));
  }

  Color start_color() const;
  Color end_color() const;

  /// The gradient with a different `<color-interpolation-method>`. Stops are
  /// immutable and shared, so the copy is one small allocation at parse time
  /// and nothing at all afterwards.
  LinearGradient with_interpolation(ColorInterpolationMethod method) const;

  /// As written -- `specified` is false until an author says `in <space>`.
  ColorInterpolationMethod interpolation() const;

  /// The method with the space folded in: sRGB while every stop is a legacy
  /// colour, Oklab as soon as one is not, exactly as a two-colour mix decides
  /// it.
  ColorInterpolationMethod effective_interpolation() const;

  Point<float> start() const;
  Point<float> end() const;
  Geometry geometry() const;
  float angle() const;
  Direction direction() const;
  std::span<const GradientStop> stops() const;

  /// Resolves CSS angles and side/corner keywords against the actual gradient
  /// box. Returned points remain in local 0..1 coordinates.
  std::pair<Point<float>, Point<float>> axis_for(Size<float> box) const;

  /// Writes colors and used 0..1 positions after CSS stop fixup. The output
  /// spans must each hold at least kMaxGradientStops elements.
  std::size_t resolve_stops(Size<float> box, std::span<Color> colors,
                            std::span<float> positions) const;

  bool operator==(const LinearGradient &other) const;

private:
  struct Data;
  static std::shared_ptr<Data> make_data_(std::span<const GradientStop> stops);
  explicit LinearGradient(std::shared_ptr<const Data> data)
      : data_(std::move(data)) {}

  std::shared_ptr<const Data> data_;
};

inline constexpr std::size_t kMaxGradientStops = 8;

/// A compact conic gradient. The immutable stops are shared between animated
/// copies, so advancing the angle is allocation-free and copies only a shared
/// pointer plus one float.
///
/// Stop positions are fractions of a full turn. Unlike the linear case the
/// last stop does not sit at 1: it closes back onto the first, so a run of
/// unspecified stops is spread over `count` arcs rather than `count - 1` and
/// the seam a spinner ring would otherwise show never appears. A stop given as
/// a length is treated as unspecified, a turn having no extent to measure.
class ConicGradient {
public:
  ConicGradient() = default;
  ConicGradient(float angle_radians, std::span<const GradientStop> stops);
  ConicGradient(float angle_radians, std::initializer_list<GradientStop> stops)
      : ConicGradient(
            angle_radians,
            std::span<const GradientStop>(stops.begin(), stops.size())) {}

  /// Evenly spaced around the turn. `GradientStop` has no implicit conversion
  /// from `Color`, so this stays unambiguous with the overloads above.
  ConicGradient(float angle_radians, std::span<const Color> colors);
  ConicGradient(float angle_radians, std::initializer_list<Color> colors)
      : ConicGradient(angle_radians,
                      std::span<const Color>(colors.begin(), colors.size())) {}

  float angle() const { return angle_; }
  std::span<const GradientStop> stops() const;

  ConicGradient with_interpolation(ColorInterpolationMethod method) const;
  ColorInterpolationMethod interpolation() const;
  ColorInterpolationMethod effective_interpolation() const;

  ConicGradient with_angle(float angle_radians) const {
    ConicGradient result = *this;
    result.angle_ = angle_radians;
    return result;
  }

  /// Writes colors and used 0..1 turn positions after stop fixup. The output
  /// spans must each hold at least kMaxGradientStops elements.
  std::size_t resolve_stops(std::span<Color> colors,
                            std::span<float> positions) const;

  bool operator==(const ConicGradient &other) const;

private:
  struct Stops {
    std::array<GradientStop, kMaxGradientStops> stops{
        GradientStop(Color::TRANSPARENT), GradientStop(Color::TRANSPARENT),
        GradientStop(Color::TRANSPARENT), GradientStop(Color::TRANSPARENT),
        GradientStop(Color::TRANSPARENT), GradientStop(Color::TRANSPARENT),
        GradientStop(Color::TRANSPARENT), GradientStop(Color::TRANSPARENT)};
    std::uint8_t count = 0;
    ColorInterpolationMethod interpolation;
  };

  std::shared_ptr<const Stops> stops_;
  float angle_ = 0.0f;
};

} // namespace voidui
