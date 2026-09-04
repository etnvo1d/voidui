#include "voidui/core/gradient.h"

#include <algorithm>
#include <cmath>
#include <limits>

namespace voidui {
namespace {

constexpr GradientStop kTransparentStops[]{
    GradientStop(Color::TRANSPARENT, 0.0f),
    GradientStop(Color::TRANSPARENT, 1.0f),
};

float finite_or(float value, float fallback) {
  return std::isfinite(value) ? value : fallback;
}

/// The space a stop list mixes in when the author named none. One legacy
/// colour among legacy colours keeps the whole ramp in sRGB, which is what
/// every gradient written before this existed relied on; a single modern
/// colour anywhere moves it to Oklab, matching what a two-colour mix does.
ColorInterpolationMethod fold_interpolation(ColorInterpolationMethod method,
                                            std::span<const GradientStop> stops) {
  if (method.specified)
    return method;
  method.specified = true;
  method.space = ColorInterpolationSpace::Srgb;
  for (const GradientStop &stop : stops) {
    if (stop.color.space != ColorSpace::LegacyRgb) {
      method.space = ColorInterpolationSpace::Oklab;
      break;
    }
  }
  return method;
}

} // namespace

struct LinearGradient::Data {
  std::array<GradientStop, kMaxGradientStops> stops{
      GradientStop(Color::TRANSPARENT, 0.0f),
      GradientStop(Color::TRANSPARENT, 1.0f),
      GradientStop(Color::TRANSPARENT),
      GradientStop(Color::TRANSPARENT),
      GradientStop(Color::TRANSPARENT),
      GradientStop(Color::TRANSPARENT),
      GradientStop(Color::TRANSPARENT),
      GradientStop(Color::TRANSPARENT)};
  Point<float> start{0.5f, 0.0f};
  Point<float> end{0.5f, 1.0f};
  float angle = 0.0f;
  std::uint8_t count = 2;
  Geometry geometry = Geometry::Points;
  Direction direction = Direction::Bottom;
  ColorInterpolationMethod interpolation;
};

std::shared_ptr<LinearGradient::Data>
LinearGradient::make_data_(std::span<const GradientStop> input) {
  auto data = std::make_shared<LinearGradient::Data>();
  const std::span<const GradientStop> source =
      input.empty() ? std::span<const GradientStop>(kTransparentStops) : input;
  data->count = static_cast<std::uint8_t>(
      std::min(source.size(), static_cast<std::size_t>(kMaxGradientStops)));
  std::copy_n(source.begin(), data->count, data->stops.begin());
  for (std::size_t i = 0; i < data->count; ++i) {
    GradientStop &stop = data->stops[i];
    if (stop.unit != GradientStopUnit::Unspecified)
      stop.position = finite_or(stop.position, 0.0f);
  }
  if (data->count == 1) {
    data->stops[1] = data->stops[0];
    data->stops[0].position = 0.0f;
    data->stops[0].unit = GradientStopUnit::Fraction;
    data->stops[1].position = 1.0f;
    data->stops[1].unit = GradientStopUnit::Fraction;
    data->count = 2;
  }
  return data;
}

namespace {

Point<float> direction_vector(LinearGradient::Direction direction,
                              Size<float> box) {
  const float width = std::max(std::abs(box.width), 1e-5f);
  const float height = std::max(std::abs(box.height), 1e-5f);
  Point<float> vector;
  switch (direction) {
  case LinearGradient::Direction::Top:
    vector = {0.0f, -1.0f};
    break;
  case LinearGradient::Direction::TopRight:
    vector = {height, -width};
    break;
  case LinearGradient::Direction::Right:
    vector = {1.0f, 0.0f};
    break;
  case LinearGradient::Direction::BottomRight:
    vector = {height, width};
    break;
  case LinearGradient::Direction::Bottom:
    vector = {0.0f, 1.0f};
    break;
  case LinearGradient::Direction::BottomLeft:
    vector = {-height, width};
    break;
  case LinearGradient::Direction::Left:
    vector = {-1.0f, 0.0f};
    break;
  case LinearGradient::Direction::TopLeft:
    vector = {-height, -width};
    break;
  }
  const float length = std::hypot(vector.x, vector.y);
  return length > 1e-5f ? Point<float>(vector.x / length, vector.y / length)
                        : Point<float>(0.0f, 1.0f);
}

} // namespace

LinearGradient::LinearGradient(Color start_color, Color end_color,
                               Point<float> start, Point<float> end)
    : LinearGradient(
          start, end,
          {GradientStop(start_color, 0.0f), GradientStop(end_color, 1.0f)}) {}

LinearGradient::LinearGradient(Point<float> start, Point<float> end,
                               std::span<const GradientStop> stops) {
  auto data = make_data_(stops);
  data->start = {finite_or(start.x, 0.5f), finite_or(start.y, 0.0f)};
  data->end = {finite_or(end.x, 0.5f), finite_or(end.y, 1.0f)};
  data_ = std::move(data);
}

LinearGradient LinearGradient::css_angle(float angle_radians,
                                         std::span<const GradientStop> stops) {
  auto data = make_data_(stops);
  data->geometry = Geometry::CssAngle;
  data->angle = finite_or(angle_radians, 0.0f);
  return LinearGradient(std::move(data));
}

LinearGradient
LinearGradient::css_direction(Direction direction,
                              std::span<const GradientStop> stops) {
  auto data = make_data_(stops);
  data->geometry = Geometry::CssDirection;
  data->direction = direction;
  return LinearGradient(std::move(data));
}

Color LinearGradient::start_color() const {
  const auto values = stops();
  return values.empty() ? Color::TRANSPARENT : values.front().color;
}

Color LinearGradient::end_color() const {
  const auto values = stops();
  return values.empty() ? Color::TRANSPARENT : values.back().color;
}

LinearGradient
LinearGradient::with_interpolation(ColorInterpolationMethod method) const {
  auto data = data_ ? std::make_shared<Data>(*data_) : std::make_shared<Data>();
  data->interpolation = method;
  return LinearGradient(std::move(data));
}

ColorInterpolationMethod LinearGradient::interpolation() const {
  return data_ ? data_->interpolation : ColorInterpolationMethod{};
}

ColorInterpolationMethod LinearGradient::effective_interpolation() const {
  return fold_interpolation(interpolation(), stops());
}

Point<float> LinearGradient::start() const {
  return data_ ? data_->start : Point<float>(0.5f, 0.0f);
}

Point<float> LinearGradient::end() const {
  return data_ ? data_->end : Point<float>(0.5f, 1.0f);
}

LinearGradient::Geometry LinearGradient::geometry() const {
  return data_ ? data_->geometry : Geometry::Points;
}

float LinearGradient::angle() const { return data_ ? data_->angle : 0.0f; }

LinearGradient::Direction LinearGradient::direction() const {
  return data_ ? data_->direction : Direction::Bottom;
}

std::span<const GradientStop> LinearGradient::stops() const {
  return data_
             ? std::span<const GradientStop>(data_->stops.data(), data_->count)
             : std::span<const GradientStop>();
}

std::pair<Point<float>, Point<float>>
LinearGradient::axis_for(Size<float> box) const {
  if (geometry() == Geometry::Points)
    return {start(), end()};

  const float width = std::max(std::abs(box.width), 1e-5f);
  const float height = std::max(std::abs(box.height), 1e-5f);
  Point<float> direction_value;
  if (geometry() == Geometry::CssAngle) {
    direction_value = {std::sin(angle()), -std::cos(angle())};
  } else {
    direction_value = direction_vector(direction(), box);
  }

  // CSS Images 3: L = |W sin(A)| + |H cos(A)|. The line is centered in the
  // gradient box; normalizing its endpoints keeps the GPU payload box-local.
  const float line_length = std::abs(width * direction_value.x) +
                            std::abs(height * direction_value.y);
  const float x = direction_value.x * line_length / (2.0f * width);
  const float y = direction_value.y * line_length / (2.0f * height);
  return {{0.5f - x, 0.5f - y}, {0.5f + x, 0.5f + y}};
}

std::size_t LinearGradient::resolve_stops(Size<float> box,
                                          std::span<Color> colors,
                                          std::span<float> positions) const {
  const auto values = stops();
  const std::size_t count =
      std::min({values.size(), colors.size(), positions.size()});
  if (count == 0)
    return 0;

  const auto [axis_start, axis_end] = axis_for(box);
  const float line_width = (axis_end.x - axis_start.x) * box.width;
  const float line_height = (axis_end.y - axis_start.y) * box.height;
  const float line_length =
      std::max(std::hypot(line_width, line_height), 1e-5f);
  const float unspecified = std::numeric_limits<float>::quiet_NaN();

  for (std::size_t i = 0; i < count; ++i) {
    colors[i] = values[i].color;
    switch (values[i].unit) {
    case GradientStopUnit::Fraction:
      positions[i] = finite_or(values[i].position, 0.0f);
      break;
    case GradientStopUnit::Length:
      positions[i] = finite_or(values[i].position, 0.0f) / line_length;
      break;
    case GradientStopUnit::Unspecified:
      positions[i] = unspecified;
      break;
    }
  }

  if (std::isnan(positions[0]))
    positions[0] = 0.0f;
  if (std::isnan(positions[count - 1]))
    positions[count - 1] = 1.0f;

  // CSS color-stop fixup: specified stops never move before an earlier stop;
  // each run of missing positions is then distributed evenly.
  float previous = positions[0];
  for (std::size_t i = 1; i < count; ++i) {
    if (std::isnan(positions[i]))
      continue;
    positions[i] = std::max(positions[i], previous);
    previous = positions[i];
  }
  for (std::size_t begin = 0; begin + 1 < count;) {
    std::size_t end = begin + 1;
    while (end < count && std::isnan(positions[end]))
      ++end;
    const float step =
        (positions[end] - positions[begin]) / static_cast<float>(end - begin);
    for (std::size_t i = begin + 1; i < end; ++i)
      positions[i] = positions[begin] + step * static_cast<float>(i - begin);
    begin = end;
  }
  return count;
}

bool LinearGradient::operator==(const LinearGradient &other) const {
  if (geometry() != other.geometry() || start().x != other.start().x ||
      start().y != other.start().y || end().x != other.end().x ||
      end().y != other.end().y || angle() != other.angle() ||
      direction() != other.direction() ||
      !(interpolation() == other.interpolation()))
    return false;
  const auto left = stops();
  const auto right = other.stops();
  if (left.size() != right.size())
    return false;
  for (std::size_t i = 0; i < left.size(); ++i) {
    if (!(left[i].color == right[i].color) ||
        left[i].position != right[i].position || left[i].unit != right[i].unit)
      return false;
  }
  return true;
}

ConicGradient::ConicGradient(float angle_radians,
                             std::span<const GradientStop> input)
    : angle_(angle_radians) {
  auto stops = std::make_shared<Stops>();
  stops->count = static_cast<std::uint8_t>(
      std::min(input.size(), static_cast<std::size_t>(kMaxGradientStops)));
  std::copy_n(input.begin(), stops->count, stops->stops.begin());
  for (std::uint8_t i = 0; i < stops->count; ++i) {
    GradientStop &stop = stops->stops[i];
    if (stop.unit != GradientStopUnit::Unspecified)
      stop.position = finite_or(stop.position, 0.0f);
  }
  stops_ = std::move(stops);
}

ConicGradient::ConicGradient(float angle_radians, std::span<const Color> colors)
    : angle_(angle_radians) {
  auto stops = std::make_shared<Stops>();
  stops->count = static_cast<std::uint8_t>(
      std::min(colors.size(), static_cast<std::size_t>(kMaxGradientStops)));
  for (std::uint8_t i = 0; i < stops->count; ++i)
    stops->stops[i] = GradientStop(colors[i]);
  stops_ = std::move(stops);
}

std::span<const GradientStop> ConicGradient::stops() const {
  return stops_
             ? std::span<const GradientStop>(stops_->stops.data(), stops_->count)
             : std::span<const GradientStop>();
}

ConicGradient
ConicGradient::with_interpolation(ColorInterpolationMethod method) const {
  auto stops = stops_ ? std::make_shared<Stops>(*stops_)
                      : std::make_shared<Stops>();
  stops->interpolation = method;
  ConicGradient result = *this;
  result.stops_ = std::move(stops);
  return result;
}

ColorInterpolationMethod ConicGradient::interpolation() const {
  return stops_ ? stops_->interpolation : ColorInterpolationMethod{};
}

ColorInterpolationMethod ConicGradient::effective_interpolation() const {
  return fold_interpolation(interpolation(), stops());
}

std::size_t ConicGradient::resolve_stops(std::span<Color> colors,
                                         std::span<float> positions) const {
  const auto values = stops();
  const std::size_t count =
      std::min({values.size(), colors.size(), positions.size()});
  if (count == 0)
    return 0;

  const float unspecified = std::numeric_limits<float>::quiet_NaN();
  for (std::size_t i = 0; i < count; ++i) {
    colors[i] = values[i].color;
    positions[i] = values[i].unit == GradientStopUnit::Fraction
                       ? finite_or(values[i].position, 0.0f)
                       : unspecified;
  }

  if (std::isnan(positions[0]))
    positions[0] = 0.0f;

  // Specified stops never move behind an earlier one, as in the linear case.
  float previous = positions[0];
  for (std::size_t i = 1; i < count; ++i) {
    if (std::isnan(positions[i]))
      continue;
    positions[i] = std::max(positions[i], previous);
    previous = positions[i];
  }

  // The arc from the last stop back to the first is a segment like any other,
  // so a trailing run of unspecified stops is distributed against a sentinel
  // one full turn past the first stop instead of being pinned to 1. Three
  // unspecified stops therefore land on thirds and close cleanly, where the
  // linear rule would put them at 0, 0.5 and 1 and seam at zero degrees.
  const float sentinel = positions[0] + 1.0f;
  for (std::size_t begin = 0; begin + 1 < count;) {
    std::size_t end = begin + 1;
    while (end < count && std::isnan(positions[end]))
      ++end;
    const float anchor = end < count ? positions[end] : sentinel;
    const std::size_t span = end < count ? end - begin : count - begin;
    const float step =
        (anchor - positions[begin]) / static_cast<float>(span);
    for (std::size_t i = begin + 1; i < end; ++i)
      positions[i] = positions[begin] + step * static_cast<float>(i - begin);
    begin = end;
  }
  return count;
}

bool ConicGradient::operator==(const ConicGradient &other) const {
  if (angle_ != other.angle_ || !(interpolation() == other.interpolation()))
    return false;
  const auto left = stops();
  const auto right = other.stops();
  if (left.size() != right.size())
    return false;
  for (std::size_t i = 0; i < left.size(); ++i) {
    if (!(left[i].color == right[i].color) ||
        left[i].position != right[i].position || left[i].unit != right[i].unit)
      return false;
  }
  return true;
}

} // namespace voidui
