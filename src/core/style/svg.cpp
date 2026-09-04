#include "voidui/core/style/svg.h"

#include <cstdlib>
#include <string>
#include <vector>

namespace voidui {

namespace {

bool is_space(char c) {
  return c == ' ' || c == '\t' || c == '\n' || c == '\r' || c == '\f' ||
         c == '\v';
}

char lower(char c) {
  return (c >= 'A' && c <= 'Z') ? static_cast<char>(c - 'A' + 'a') : c;
}

/// ASCII case-insensitive keyword comparison.
///
/// CSS keywords are case-insensitive and SVG's own attribute vocabulary is
/// written in camel case -- `currentColor`, `nonzero`, `non-scaling-stroke` --
/// so a byte-for-byte match would reject the spelling the specification itself
/// uses in its examples.
bool keyword_is(std::string_view text, std::string_view keyword) {
  if (text.size() != keyword.size())
    return false;
  for (std::size_t i = 0; i < text.size(); ++i)
    if (lower(text[i]) != keyword[i])
      return false;
  return true;
}

/// Splits on whitespace and commas together.
///
/// SVG's numeric lists treat the two interchangeably: `4 2`, `4,2` and `4, 2`
/// are one list, and so is `4,,2` -- an empty run between separators is not an
/// entry.
std::vector<std::string_view> split_list(std::string_view text) {
  std::vector<std::string_view> parts;
  std::size_t start = std::string_view::npos;
  for (std::size_t i = 0; i < text.size(); ++i) {
    const bool separator = is_space(text[i]) || text[i] == ',';
    if (separator) {
      if (start != std::string_view::npos) {
        parts.push_back(text.substr(start, i - start));
        start = std::string_view::npos;
      }
    } else if (start == std::string_view::npos) {
      start = i;
    }
  }
  if (start != std::string_view::npos)
    parts.push_back(text.substr(start));
  return parts;
}

} // namespace

bool parse_style_value(std::string_view text, SvgPaint &out) {
  text = style_trim(text);
  if (text.empty())
    return false;

  if (keyword_is(text, "none")) {
    out = SvgPaint::none();
    return true;
  }
  if (keyword_is(text, "currentcolor")) {
    out = SvgPaint::current_color();
    return true;
  }

  // A fragment reference is meaningful inside a document and meaningless in a
  // rule, which has nothing to resolve it against. Reported as unreadable so
  // the author gets a diagnostic naming the property, rather than a silent
  // black shape.
  if (text.size() >= 4 && keyword_is(text.substr(0, 4), "url("))
    return false;

  Color color;
  if (!parse_style_value(text, color))
    return false;
  out = SvgPaint::solid(color);
  return true;
}

bool parse_style_value(std::string_view text, SvgDashArray &out) {
  text = style_trim(text);
  if (text.empty())
    return false;

  if (keyword_is(text, "none")) {
    out = SvgDashArray{};
    return true;
  }

  const std::vector<std::string_view> parts = split_list(text);
  if (parts.empty() || parts.size() > kMaxSvgDashes)
    return false;

  SvgDashArray dashes;
  for (const std::string_view part : parts) {
    float value = 0.0f;
    if (!parse_style_value(part, value) || !(value >= 0.0f))
      return false;
    dashes.lengths[dashes.count++] = value;
  }

  out = dashes;
  return true;
}

bool parse_style_value(std::string_view text, SvgPaintOrder &out) {
  text = style_trim(text);
  if (text.empty())
    return false;

  if (keyword_is(text, "normal")) {
    out = SvgPaintOrder::FillStroke;
    return true;
  }

  // Only the relative order of fill and stroke can change anything here, so the
  // list is read for which of the two comes first and validated for the rest.
  // Markers are accepted and ignored -- they are part of the grammar, and a
  // rule that mentions them should not fail to parse over a feature the
  // renderer does not draw.
  bool saw_fill = false;
  bool saw_stroke = false;
  SvgPaintOrder order = SvgPaintOrder::FillStroke;
  for (const std::string_view word : split_list(text)) {
    if (keyword_is(word, "fill")) {
      if (saw_fill)
        return false;
      saw_fill = true;
    } else if (keyword_is(word, "stroke")) {
      if (saw_stroke)
        return false;
      if (!saw_fill)
        order = SvgPaintOrder::StrokeFill;
      saw_stroke = true;
    } else if (!keyword_is(word, "markers")) {
      return false;
    }
  }

  if (!saw_fill && !saw_stroke)
    return false;

  out = order;
  return true;
}

bool parse_style_value(std::string_view text, SvgVectorEffect &out) {
  text = style_trim(text);
  if (keyword_is(text, "none")) {
    out = SvgVectorEffect::None;
    return true;
  }
  if (keyword_is(text, "non-scaling-stroke")) {
    out = SvgVectorEffect::NonScalingStroke;
    return true;
  }
  return false;
}

bool parse_style_value(std::string_view text, LineCap &out) {
  text = style_trim(text);
  if (keyword_is(text, "butt")) {
    out = LineCap::Butt;
    return true;
  }
  if (keyword_is(text, "round")) {
    out = LineCap::Round;
    return true;
  }
  if (keyword_is(text, "square")) {
    out = LineCap::Square;
    return true;
  }
  return false;
}

bool parse_style_value(std::string_view text, LineJoin &out) {
  text = style_trim(text);
  if (keyword_is(text, "miter")) {
    out = LineJoin::Miter;
    return true;
  }
  if (keyword_is(text, "round")) {
    out = LineJoin::Round;
    return true;
  }
  if (keyword_is(text, "bevel")) {
    out = LineJoin::Bevel;
    return true;
  }

  // SVG 2 adds two joins this renderer does not distinguish. Falling back to
  // the nearest one it does draw is better than refusing the declaration and
  // leaving the join at whatever the cascade had underneath.
  if (keyword_is(text, "miter-clip")) {
    out = LineJoin::Miter;
    return true;
  }
  if (keyword_is(text, "arcs")) {
    out = LineJoin::Round;
    return true;
  }
  return false;
}

bool parse_style_value(std::string_view text, FillRule &out) {
  text = style_trim(text);
  if (keyword_is(text, "nonzero")) {
    out = FillRule::NonZero;
    return true;
  }
  if (keyword_is(text, "evenodd")) {
    out = FillRule::EvenOdd;
    return true;
  }
  return false;
}

} // namespace voidui
