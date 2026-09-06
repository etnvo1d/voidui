#include "voidui/core/style/table.h"

#include <array>

namespace voidui {
namespace {

/// A keyword table beats a chain of comparisons here only in readability, but
/// six properties written six different ways is exactly how a parser grows a
/// value that behaves unlike its neighbours.
template <class T, std::size_t N>
bool parse_keyword(std::string_view text,
                   const std::array<std::pair<std::string_view, T>, N> &table,
                   T &out) {
  text = style_trim(text);
  for (const auto &[keyword, value] : table) {
    if (text == keyword) {
      out = value;
      return true;
    }
  }
  return false;
}

} // namespace

bool parse_style_value(std::string_view text, TableLayout &out) {
  static constexpr std::array<std::pair<std::string_view, TableLayout>, 2>
      table{{{"auto", TableLayout::Auto}, {"fixed", TableLayout::Fixed}}};
  return parse_keyword(text, table, out);
}

bool parse_style_value(std::string_view text, BorderCollapse &out) {
  static constexpr std::array<std::pair<std::string_view, BorderCollapse>, 2>
      table{{{"separate", BorderCollapse::Separate},
             {"collapse", BorderCollapse::Collapse}}};
  return parse_keyword(text, table, out);
}

bool parse_style_value(std::string_view text, CaptionSide &out) {
  static constexpr std::array<std::pair<std::string_view, CaptionSide>, 2>
      table{{{"top", CaptionSide::Top}, {"bottom", CaptionSide::Bottom}}};
  return parse_keyword(text, table, out);
}

bool parse_style_value(std::string_view text, EmptyCells &out) {
  static constexpr std::array<std::pair<std::string_view, EmptyCells>, 2> table{
      {{"show", EmptyCells::Show}, {"hide", EmptyCells::Hide}}};
  return parse_keyword(text, table, out);
}

bool parse_style_value(std::string_view text, VerticalAlign &out) {
  static constexpr std::array<std::pair<std::string_view, VerticalAlign>, 4>
      table{{{"baseline", VerticalAlign::Baseline},
             {"top", VerticalAlign::Top},
             {"middle", VerticalAlign::Middle},
             {"bottom", VerticalAlign::Bottom}}};
  return parse_keyword(text, table, out);
}

bool parse_style_value(std::string_view text, BorderSpacing &out) {
  text = style_trim(text);
  const std::size_t split = text.find_first_of(" \t\r\n");
  float horizontal = 0.0f;
  if (!parse_style_value(text.substr(0, split), horizontal))
    return false;
  if (split == std::string_view::npos) {
    out = BorderSpacing(horizontal);
    return true;
  }
  float vertical = 0.0f;
  const std::string_view rest = style_trim(text.substr(split));
  if (rest.find_first_of(" \t\r\n") != std::string_view::npos ||
      !parse_style_value(rest, vertical))
    return false;
  out = BorderSpacing(horizontal, vertical);
  return true;
}

} // namespace voidui
