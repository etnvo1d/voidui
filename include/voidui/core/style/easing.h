#pragma once

#include <cstdint>
#include <deque>
#include <mutex>
#include <string_view>
#include <type_traits>
#include <vector>

#include "voidui/core/style/value.h"

namespace voidui {

/// Which end of a step interval the jump happens on, per css-easing-1.
///
///   steps(4, jump-start)  first jump at 0,   last at 3/4  ("start")
///   steps(4, jump-end)    first jump at 1/4, last at 1    ("end", the default)
///   steps(4, jump-none)   holds both 0 and 1, three jumps in between
///   steps(4, jump-both)   jumps immediately and again at 1
enum class StepPosition : std::uint8_t {
  JumpStart,
  JumpEnd,
  JumpNone,
  JumpBoth,
};

/// One control point of a `linear()` easing function.
struct LinearEasingPoint {
  float input = 0.0f;
  float output = 0.0f;

  friend bool operator==(const LinearEasingPoint &,
                         const LinearEasingPoint &) = default;
};

/// Interned control-point lists for `linear()`.
///
/// The lists are the only part of an easing function that cannot fit in a
/// register, and a stylesheet contains a handful of distinct ones at most.
/// Interning happens once, while parsing; the address it hands back is stable
/// for the life of the process, so evaluating the easing on a later frame is
/// a dereference and never takes the lock. A deque, not a vector, is what
/// makes that address stable as the table grows.
class LinearEasingTable {
public:
  static LinearEasingTable &instance();

  /// Null for an empty list. Identical content always interns to the same
  /// entry, so a hot reload of the same file does not grow the table, and two
  /// equal curves compare equal by pointer.
  const std::vector<LinearEasingPoint> *
  intern(const std::vector<LinearEasingPoint> &points);

private:
  mutable std::mutex mutex_;
  std::deque<std::vector<LinearEasingPoint>> lists_;
};

/// A CSS easing function, by value.
///
/// Thirty-two bytes, trivially copyable, and free of indirection for every
/// form but `linear()`, which holds a pointer into an interned table.
/// Evaluation is a handful of multiplies -- no lookup table is built, no lock
/// is taken and no state is kept between frames -- so a node can hold one per
/// animating property without the animator owning anything extra.
class Easing {
public:
  enum class Kind : std::uint8_t {
    Linear,
    CubicBezier,
    Steps,
    /// `linear()` with explicit control points.
    Points,
  };

  /// `ease`, the CSS initial value of both timing-function properties.
  constexpr Easing() noexcept = default;

  static constexpr Easing linear() noexcept {
    Easing result;
    result.kind_ = Kind::Linear;
    result.x1_ = result.y1_ = 0.0f;
    result.x2_ = result.y2_ = 1.0f;
    return result;
  }

  static constexpr Easing cubic_bezier(float x1, float y1, float x2,
                                       float y2) noexcept {
    Easing result;
    result.kind_ = Kind::CubicBezier;
    result.x1_ = x1;
    result.y1_ = y1;
    result.x2_ = x2;
    result.y2_ = y2;
    return result;
  }

  static constexpr Easing steps(std::uint16_t count,
                                StepPosition position) noexcept {
    Easing result;
    result.kind_ = Kind::Steps;
    result.position_ = position;
    result.step_count_ = count;
    result.x1_ = result.y1_ = 0.0f;
    result.x2_ = result.y2_ = 1.0f;
    return result;
  }

  /// Interns `points` and returns the matching `linear()` function. A list of
  /// fewer than two points has no segment to interpolate across and
  /// degenerates to `linear`.
  static Easing linear_points(const std::vector<LinearEasingPoint> &points);

  static constexpr Easing ease() noexcept {
    return cubic_bezier(0.25f, 0.1f, 0.25f, 1.0f);
  }
  static constexpr Easing ease_in() noexcept {
    return cubic_bezier(0.42f, 0.0f, 1.0f, 1.0f);
  }
  static constexpr Easing ease_out() noexcept {
    return cubic_bezier(0.0f, 0.0f, 0.58f, 1.0f);
  }
  static constexpr Easing ease_in_out() noexcept {
    return cubic_bezier(0.42f, 0.0f, 0.58f, 1.0f);
  }
  static constexpr Easing step_start() noexcept {
    return steps(1, StepPosition::JumpStart);
  }
  static constexpr Easing step_end() noexcept {
    return steps(1, StepPosition::JumpEnd);
  }

  constexpr Kind kind() const noexcept { return kind_; }
  constexpr StepPosition step_position() const noexcept { return position_; }
  constexpr std::uint16_t step_count() const noexcept { return step_count_; }
  /// The interned control points of a `linear()` easing, null for every other
  /// kind. Stable for the life of the process.
  constexpr const std::vector<LinearEasingPoint> *control_points() const
      noexcept {
    return points_;
  }

  /// The four control coordinates of a cubic-bezier easing. Meaningless for
  /// the other kinds, where they are pinned to the identity curve so that two
  /// equal functions always compare equal.
  constexpr float x1() const noexcept { return x1_; }
  constexpr float y1() const noexcept { return y1_; }
  constexpr float x2() const noexcept { return x2_; }
  constexpr float y2() const noexcept { return y2_; }

  /// Maps input progress to output progress. `progress` is clamped to [0, 1];
  /// the output is not, because a cubic-bezier may legitimately overshoot.
  float operator()(float progress) const {
    // The linear case is the one a scroll-driven or continuously sampled
    // property is most likely to use, so it stays inline and branch-free.
    if (kind_ == Kind::Linear)
      return progress < 0.0f ? 0.0f : (progress > 1.0f ? 1.0f : progress);
    return evaluate_(progress);
  }

  friend constexpr bool operator==(const Easing &, const Easing &) = default;

private:
  float evaluate_(float progress) const;

  Kind kind_ = Kind::CubicBezier;
  StepPosition position_ = StepPosition::JumpEnd;
  std::uint16_t step_count_ = 1;
  const std::vector<LinearEasingPoint> *points_ = nullptr;
  float x1_ = 0.25f;
  float y1_ = 0.1f;
  float x2_ = 0.25f;
  float y2_ = 1.0f;
};

static_assert(std::is_trivially_copyable_v<Easing>);

/// Reads any of the CSS `<easing-function>` productions:
///
///   linear | ease | ease-in | ease-out | ease-in-out | step-start | step-end
///   cubic-bezier(<number>, <number>, <number>, <number>)
///   steps(<integer> [, <step-position>]?)
///   linear(<number> [<percentage> <percentage>?]? , ...)
bool parse_style_value(std::string_view text, Easing &out);

inline bool style_value_equals(const Easing &a, const Easing &b) {
  return a == b;
}

/// Hashed field by field rather than as bytes: Easing has padding after its
/// two leading enums, whose contents are not guaranteed to be anything.
std::uint64_t style_value_hash(const Easing &easing);

} // namespace voidui
