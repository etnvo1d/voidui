#pragma once

#include <cstdint>
#include <limits>
#include <string_view>

namespace voidui {

enum class Position : std::uint8_t {
  Static,
  Relative,
  Absolute,
  Fixed,
  Sticky
};
enum class Visibility : std::uint8_t { Visible, Hidden, Collapse };
enum class PointerEvents : std::uint8_t { Auto, None };
enum class WhiteSpace : std::uint8_t { Normal, Nowrap, Pre, PreWrap };

/// A CSS inset in logical pixels plus a percentage of its containing block.
/// Percentages remain unresolved until layout; auto occupies no extra storage.
struct Inset {
  struct Auto {};
  float pixels = 0.0f;
  float percent = std::numeric_limits<float>::infinity();

  constexpr Inset() = default;
  constexpr Inset(Auto) {}
  constexpr Inset(float px) : pixels(px), percent(0.0f) {}
  constexpr Inset(float px, float percentage)
      : pixels(px), percent(percentage) {}
  static constexpr Inset percentage(float value) { return {0.0f, value}; }
  static constexpr Inset calc(float percentage, float px) {
    return {px, percentage};
  }
  constexpr bool is_auto() const {
    return percent == std::numeric_limits<float>::infinity();
  }
  constexpr float resolve(float reference) const {
    return is_auto() ? 0.0f : pixels + reference * (percent / 100.0f);
  }
  bool operator==(const Inset &) const = default;
};

struct ZIndex {
  struct Auto {};
  int value = 0;
  bool automatic = true;
  constexpr ZIndex() = default;
  constexpr ZIndex(Auto) {}
  constexpr ZIndex(int index) : value(index), automatic(false) {}
  bool operator==(const ZIndex &) const = default;
};

constexpr bool out_of_flow(Position position) {
  return position == Position::Absolute || position == Position::Fixed;
}

bool parse_style_value(std::string_view, Position &);
bool parse_style_value(std::string_view, Inset &);
bool parse_style_value(std::string_view, ZIndex &);
bool parse_style_value(std::string_view, Visibility &);
bool parse_style_value(std::string_view, PointerEvents &);
bool parse_style_value(std::string_view, WhiteSpace &);
std::uint64_t style_value_hash(const Inset &);
std::uint64_t style_value_hash(const ZIndex &);
bool interpolate_style_value(const Inset &, const Inset &, float, Inset &);
Visibility interpolate_style_value(Visibility, Visibility, float);

} // namespace voidui
