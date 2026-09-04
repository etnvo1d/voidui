#include "voidui/core/style/easing.h"

#include <algorithm>
#include <cmath>
#include <cstdlib>

namespace voidui {
namespace {

bool is_space(char c) {
  return c == ' ' || c == '\t' || c == '\n' || c == '\r' || c == '\f' ||
         c == '\v';
}

std::vector<std::string_view> split_arguments(std::string_view text) {
  std::vector<std::string_view> result;
  int depth = 0;
  std::size_t begin = 0;
  for (std::size_t i = 0; i < text.size(); ++i) {
    if (text[i] == '(')
      ++depth;
    else if (text[i] == ')')
      depth = std::max(depth - 1, 0);
    else if (text[i] == ',' && depth == 0) {
      result.push_back(style_trim(text.substr(begin, i - begin)));
      begin = i + 1;
    }
  }
  result.push_back(style_trim(text.substr(begin)));
  return result;
}

std::vector<std::string_view> split_words(std::string_view text) {
  std::vector<std::string_view> result;
  std::size_t begin = std::string_view::npos;
  for (std::size_t i = 0; i < text.size(); ++i) {
    if (is_space(text[i])) {
      if (begin != std::string_view::npos) {
        result.push_back(text.substr(begin, i - begin));
        begin = std::string_view::npos;
      }
    } else if (begin == std::string_view::npos) {
      begin = i;
    }
  }
  if (begin != std::string_view::npos)
    result.push_back(text.substr(begin));
  return result;
}

/// `name(arguments)`. False when `text` is not exactly one function call.
bool read_call(std::string_view text, std::string_view &name,
               std::string_view &arguments) {
  const std::size_t open = text.find('(');
  if (open == std::string_view::npos || text.back() != ')')
    return false;
  name = style_trim(text.substr(0, open));
  arguments = text.substr(open + 1, text.size() - open - 2);
  return !name.empty();
}

bool parse_percentage(std::string_view text, float &fraction) {
  if (!text.ends_with('%'))
    return false;
  text.remove_suffix(1);
  float value = 0.0f;
  if (!parse_style_value(text, value) || !std::isfinite(value))
    return false;
  fraction = value * 0.01f;
  return true;
}

bool parse_integer(std::string_view text, long &out) {
  const std::string copy(text);
  char *end = nullptr;
  const long value = std::strtol(copy.c_str(), &end, 10);
  if (end == copy.c_str() || *end != '\0')
    return false;
  out = value;
  return true;
}

bool parse_step_position(std::string_view text, StepPosition &position) {
  if (text == "jump-start" || text == "start")
    position = StepPosition::JumpStart;
  else if (text == "jump-end" || text == "end")
    position = StepPosition::JumpEnd;
  else if (text == "jump-none")
    position = StepPosition::JumpNone;
  else if (text == "jump-both")
    position = StepPosition::JumpBoth;
  else
    return false;
  return true;
}

/// Fills in the inputs css-easing-2 leaves implicit and enforces the
/// monotonicity it requires, so evaluation never has to check either.
void normalise_linear_points(std::vector<LinearEasingPoint> &points,
                             const std::vector<bool> &specified) {
  if (points.empty())
    return;
  if (!specified.front())
    points.front().input = 0.0f;
  if (!specified.back())
    points.back().input = 1.0f;

  // Runs of unspecified inputs spread evenly between their nearest specified
  // neighbours, which are now guaranteed to exist at both ends.
  std::size_t anchor = 0;
  for (std::size_t i = 1; i < points.size(); ++i) {
    if (!specified[i] && i + 1 != points.size())
      continue;
    const std::size_t span = i - anchor;
    for (std::size_t step = 1; step < span; ++step)
      points[anchor + step].input =
          points[anchor].input +
          (points[i].input - points[anchor].input) *
              (static_cast<float>(step) / static_cast<float>(span));
    anchor = i;
  }

  for (std::size_t i = 1; i < points.size(); ++i)
    points[i].input = std::max(points[i].input, points[i - 1].input);
}

bool parse_linear_points(std::string_view arguments, Easing &out) {
  std::vector<LinearEasingPoint> points;
  std::vector<bool> specified;
  for (std::string_view item : split_arguments(arguments)) {
    const std::vector<std::string_view> words = split_words(item);
    if (words.empty() || words.size() > 3)
      return false;

    float output = 0.0f;
    if (!parse_style_value(words[0], output) || !std::isfinite(output))
      return false;

    if (words.size() == 1) {
      points.push_back({0.0f, output});
      specified.push_back(false);
      continue;
    }
    // `0.5 20% 80%` is shorthand for two points sharing one output, which is
    // how css-easing-2 writes a flat segment.
    for (std::size_t i = 1; i < words.size(); ++i) {
      float input = 0.0f;
      if (!parse_percentage(words[i], input))
        return false;
      points.push_back({input, output});
      specified.push_back(true);
    }
  }
  // One control point has no segment to interpolate across, which css-easing-2
  // treats as invalid rather than as a constant.
  if (points.size() < 2)
    return false;

  normalise_linear_points(points, specified);
  out = Easing::linear_points(points);
  return true;
}

/// Newton-Raphson with a bisection fallback, the standard solver for the
/// cubic's x coordinate. Newton converges in two or three steps for the
/// curves stylesheets actually use; bisection only runs for the pathological
/// ones where the derivative vanishes.
float solve_bezier_t(float ax, float bx, float cx, float x) {
  constexpr float kEpsilon = 1e-6f;

  const auto sample = [&](float t) { return ((ax * t + bx) * t + cx) * t; };
  const auto slope = [&](float t) { return (3.0f * ax * t + 2.0f * bx) * t + cx; };

  float t = x;
  for (int i = 0; i < 8; ++i) {
    const float error = sample(t) - x;
    if (std::abs(error) < kEpsilon)
      return t;
    const float derivative = slope(t);
    if (std::abs(derivative) < 1e-6f)
      break;
    t -= error / derivative;
  }

  float low = 0.0f;
  float high = 1.0f;
  t = std::clamp(x, low, high);
  for (int i = 0; i < 32 && low < high; ++i) {
    const float value = sample(t);
    if (std::abs(value - x) < kEpsilon)
      return t;
    if (x > value)
      low = t;
    else
      high = t;
    const float next = low + (high - low) * 0.5f;
    if (next == t)
      break;
    t = next;
  }
  return t;
}

} // namespace

LinearEasingTable &LinearEasingTable::instance() {
  static LinearEasingTable table;
  return table;
}

const std::vector<LinearEasingPoint> *
LinearEasingTable::intern(const std::vector<LinearEasingPoint> &points) {
  if (points.empty())
    return nullptr;
  std::lock_guard<std::mutex> guard(mutex_);
  // Linear scan: a stylesheet holds a handful of distinct `linear()` curves,
  // and interning happens at parse time, never per frame.
  for (const std::vector<LinearEasingPoint> &existing : lists_)
    if (existing == points)
      return &existing;
  lists_.push_back(points);
  return &lists_.back();
}

Easing Easing::linear_points(const std::vector<LinearEasingPoint> &points) {
  if (points.size() < 2)
    return Easing::linear();
  Easing result;
  result.kind_ = Kind::Points;
  result.points_ = LinearEasingTable::instance().intern(points);
  result.x1_ = result.y1_ = 0.0f;
  result.x2_ = result.y2_ = 1.0f;
  if (!result.points_)
    return Easing::linear();
  return result;
}

float Easing::evaluate_(float progress) const {
  const float x = std::clamp(progress, 0.0f, 1.0f);

  switch (kind_) {
  case Kind::Linear:
    return x;

  case Kind::CubicBezier: {
    // A curve whose control points sit on the diagonal is the identity, and
    // cubic-bezier(0, 0, 1, 1) is how `linear` reaches this path through a
    // stylesheet. Worth one comparison to skip the solve.
    if (x1_ == y1_ && x2_ == y2_)
      return x;
    if (x <= 0.0f || x >= 1.0f)
      return x;

    const float cx = 3.0f * x1_;
    const float bx = 3.0f * (x2_ - x1_) - cx;
    const float ax = 1.0f - cx - bx;
    const float t = solve_bezier_t(ax, bx, cx, x);

    const float cy = 3.0f * y1_;
    const float by = 3.0f * (y2_ - y1_) - cy;
    const float ay = 1.0f - cy - by;
    return ((ay * t + by) * t + cy) * t;
  }

  case Kind::Steps: {
    const float count = static_cast<float>(step_count_);
    float jumps = count;
    if (position_ == StepPosition::JumpNone)
      jumps = count - 1.0f;
    else if (position_ == StepPosition::JumpBoth)
      jumps = count + 1.0f;
    if (jumps <= 0.0f)
      return x;

    float current = std::floor(x * count);
    if (position_ == StepPosition::JumpStart ||
        position_ == StepPosition::JumpBoth)
      current += 1.0f;
    return std::clamp(current, 0.0f, jumps) / jumps;
  }

  case Kind::Points: {
    const std::vector<LinearEasingPoint> &points = *points_;
    if (points.size() < 2)
      return x;
    if (x <= points.front().input)
      return points.front().output;
    if (x >= points.back().input)
      return points.back().output;

    // Small lists, scanned forward: a binary search would cost more in
    // branches than the four or five comparisons it saves.
    for (std::size_t i = 1; i < points.size(); ++i) {
      if (x > points[i].input)
        continue;
      const float span = points[i].input - points[i - 1].input;
      if (span <= 0.0f)
        return points[i].output;
      const float local = (x - points[i - 1].input) / span;
      return points[i - 1].output +
             (points[i].output - points[i - 1].output) * local;
    }
    return points.back().output;
  }
  }
  return x;
}

bool parse_style_value(std::string_view text, Easing &out) {
  text = style_trim(text);
  if (text.empty())
    return false;

  if (text == "linear") {
    out = Easing::linear();
    return true;
  }
  if (text == "ease") {
    out = Easing::ease();
    return true;
  }
  if (text == "ease-in") {
    out = Easing::ease_in();
    return true;
  }
  if (text == "ease-out") {
    out = Easing::ease_out();
    return true;
  }
  if (text == "ease-in-out") {
    out = Easing::ease_in_out();
    return true;
  }
  if (text == "step-start") {
    out = Easing::step_start();
    return true;
  }
  if (text == "step-end") {
    out = Easing::step_end();
    return true;
  }

  std::string_view name;
  std::string_view arguments;
  if (!read_call(text, name, arguments))
    return false;

  if (name == "cubic-bezier") {
    const std::vector<std::string_view> values = split_arguments(arguments);
    if (values.size() != 4)
      return false;
    float control[4] = {0.0f, 0.0f, 0.0f, 0.0f};
    for (std::size_t i = 0; i < 4; ++i)
      if (!parse_style_value(values[i], control[i]) ||
          !std::isfinite(control[i]))
        return false;
    // The x coordinates are the curve's parameterisation and must stay in
    // range for the solve to be single-valued; y may overshoot freely.
    if (control[0] < 0.0f || control[0] > 1.0f || control[2] < 0.0f ||
        control[2] > 1.0f)
      return false;
    out = Easing::cubic_bezier(control[0], control[1], control[2], control[3]);
    return true;
  }

  if (name == "steps") {
    const std::vector<std::string_view> values = split_arguments(arguments);
    if (values.empty() || values.size() > 2)
      return false;
    long count = 0;
    if (!parse_integer(values[0], count) || count <= 0 || count > 65535)
      return false;
    StepPosition position = StepPosition::JumpEnd;
    if (values.size() == 2 && !parse_step_position(values[1], position))
      return false;
    // jump-none spends a step on each endpoint, so one step leaves nothing to
    // interpolate across and the declaration is invalid.
    if (position == StepPosition::JumpNone && count < 2)
      return false;
    out = Easing::steps(static_cast<std::uint16_t>(count), position);
    return true;
  }

  if (name == "linear")
    return parse_linear_points(arguments, out);

  return false;
}

std::uint64_t style_value_hash(const Easing &easing) {
  const auto hash_float = [](float value) {
    const float canonical = value == 0.0f ? 0.0f : value;
    return style_hash_bytes(&canonical, sizeof(canonical));
  };
  std::uint64_t seed = static_cast<std::uint64_t>(easing.kind());
  seed = style_hash_combine(seed,
                            static_cast<std::uint64_t>(easing.step_position()));
  seed = style_hash_combine(seed, easing.step_count());
  // Interning makes the address the identity of the control points, so two
  // curves with the same content hash and compare the same.
  seed = style_hash_combine(seed, reinterpret_cast<std::uintptr_t>(
                                      easing.control_points()));
  seed = style_hash_combine(seed, hash_float(easing.x1()));
  seed = style_hash_combine(seed, hash_float(easing.y1()));
  seed = style_hash_combine(seed, hash_float(easing.x2()));
  return style_hash_combine(seed, hash_float(easing.y2()));
}

} // namespace voidui
