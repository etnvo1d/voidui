#include "voidui/core/positioning.h"
#include "voidui/core/style/value.h"

#include <charconv>
#include <cmath>

namespace voidui {
namespace {

template <class T, std::size_t N>
bool keyword(std::string_view text, T &out,
             const std::pair<std::string_view, T> (&values)[N]) {
  text = style_trim(text);
  for (const auto &[name, value] : values) {
    if (text == name) {
      out = value;
      return true;
    }
  }
  return false;
}

// Linear length/percentage sums need only two floats, including nested calc().
// Unsupported units and malformed expressions fail instead of losing units.
class InsetParser {
public:
  explicit InsetParser(std::string_view text) : text_(text) {}
  bool parse(Inset &out) {
    Inset value(0.0f);
    if (!term(value, 0) || !style_trim(text_).empty())
      return false;
    out = value;
    return true;
  }

private:
  bool term(Inset &out, unsigned depth) {
    if (depth > 32)
      return false;
    text_ = style_trim(text_);
    if (text_.empty())
      return false;
    if (text_.starts_with("calc(")) {
      text_.remove_prefix(5);
      return sum(out, depth + 1);
    }
    if (depth && text_.starts_with('(')) {
      text_.remove_prefix(1);
      return sum(out, depth + 1);
    }
    bool positive = text_.starts_with('+');
    if (positive)
      text_.remove_prefix(1);
    if (positive && (text_.starts_with('+') || text_.starts_with('-')))
      return false;
    float number;
    const auto result =
        std::from_chars(text_.data(), text_.data() + text_.size(), number);
    if (result.ec != std::errc{} || !std::isfinite(number))
      return false;
    text_.remove_prefix(static_cast<std::size_t>(result.ptr - text_.data()));
    if (text_.starts_with("px")) {
      out = number;
      text_.remove_prefix(2);
    } else if (text_.starts_with('%')) {
      out = Inset::percentage(number);
      text_.remove_prefix(1);
    } else if (number == 0.0f) {
      out = 0.0f;
    } else {
      return false;
    }
    return true;
  }
  bool sum(Inset &out, unsigned depth) {
    if (!term(out, depth))
      return false;
    for (;;) {
      const auto rest = style_trim(text_);
      if (rest.starts_with(')')) {
        text_ = rest.substr(1);
        return std::isfinite(out.pixels) && std::isfinite(out.percent);
      }
      // CSS requires whitespace on both sides of binary + and -.
      if (rest.size() == text_.size() || rest.size() < 2 ||
          (rest[0] != '+' && rest[0] != '-') ||
          (rest[1] != ' ' && rest[1] != '\t' && rest[1] != '\n' &&
           rest[1] != '\r'))
        return false;
      const float sign = rest[0] == '+' ? 1.0f : -1.0f;
      text_ = rest.substr(1);
      Inset rhs;
      if (!term(rhs, depth))
        return false;
      out.pixels += sign * rhs.pixels;
      out.percent += sign * rhs.percent;
    }
  }
  std::string_view text_;
};

} // namespace

bool parse_style_value(std::string_view text, Position &out) {
  return keyword(text, out,
                 {{"static", Position::Static},
                  {"relative", Position::Relative},
                  {"absolute", Position::Absolute},
                  {"fixed", Position::Fixed},
                  {"sticky", Position::Sticky}});
}
bool parse_style_value(std::string_view text, Visibility &out) {
  return keyword(text, out,
                 {{"visible", Visibility::Visible},
                  {"hidden", Visibility::Hidden},
                  {"collapse", Visibility::Collapse}});
}
bool parse_style_value(std::string_view text, PointerEvents &out) {
  return keyword(
      text, out,
      {{"auto", PointerEvents::Auto}, {"none", PointerEvents::None}});
}
bool parse_style_value(std::string_view text, WhiteSpace &out) {
  return keyword(text, out,
                 {{"normal", WhiteSpace::Normal},
                  {"nowrap", WhiteSpace::Nowrap},
                  {"pre", WhiteSpace::Pre},
                  {"pre-wrap", WhiteSpace::PreWrap}});
}
bool parse_style_value(std::string_view text, Inset &out) {
  text = style_trim(text);
  if (text == "auto") {
    out = {};
    return true;
  }
  return InsetParser(text).parse(out);
}
bool parse_style_value(std::string_view text, ZIndex &out) {
  text = style_trim(text);
  if (text.empty())
    return false;
  if (text == "auto") {
    out = {};
    return true;
  }
  if (text.starts_with('+')) {
    text.remove_prefix(1);
    if (text.starts_with('+') || text.starts_with('-'))
      return false;
  }
  int value;
  const auto result =
      std::from_chars(text.data(), text.data() + text.size(), value);
  if (result.ec != std::errc{} || result.ptr != text.data() + text.size())
    return false;
  out = value;
  return true;
}
std::uint64_t style_value_hash(const Inset &value) {
  const float pixels = value.pixels == 0.0f ? 0.0f : value.pixels;
  const float percent = value.percent == 0.0f ? 0.0f : value.percent;
  return style_hash_combine(style_hash_bytes(&pixels, sizeof(pixels)),
                            style_hash_bytes(&percent, sizeof(percent)));
}
std::uint64_t style_value_hash(const ZIndex &value) {
  return style_hash_combine(static_cast<std::uint64_t>(value.value),
                            value.automatic);
}
bool interpolate_style_value(const Inset &a, const Inset &b, float t,
                             Inset &out) {
  if (a.is_auto() || b.is_auto())
    return false;
  out = {a.pixels + (b.pixels - a.pixels) * t,
         a.percent + (b.percent - a.percent) * t};
  return true;
}
Visibility interpolate_style_value(Visibility a, Visibility b, float t) {
  if (a == Visibility::Visible || b == Visibility::Visible)
    return t <= 0.0f ? a : t >= 1.0f ? b : Visibility::Visible;
  return t < 0.5f ? a : b;
}

} // namespace voidui
