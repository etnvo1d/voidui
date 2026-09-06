#pragma once

#include <cstddef>
#include <cstdint>
#include <memory>
#include <span>
#include <string>
#include <string_view>
#include <vector>

namespace voidui {

enum class TextAlign : std::uint8_t { Left, Center, Right };
inline bool parse_style_value(std::string_view text, TextAlign &out) {
  const auto begin = text.find_first_not_of(" \t\r\n");
  if (begin == std::string_view::npos) return false;
  text = text.substr(begin, text.find_last_not_of(" \t\r\n") - begin + 1);
  if (text == "left") out = TextAlign::Left;
  else if (text == "center") out = TextAlign::Center;
  else if (text == "right") out = TextAlign::Right;
  else return false;
  return true;
}

/// CSS-compatible font weights. Numeric VSS values from 1 through 1000 are
/// also accepted and are represented by casting them to this enum.
enum class FontWeight : std::uint16_t {
  Thin = 100,
  ExtraLight = 200,
  Light = 300,
  Normal = 400,
  Medium = 500,
  SemiBold = 600,
  Bold = 700,
  ExtraBold = 800,
  Black = 900,
};

constexpr std::uint16_t font_weight_value(FontWeight weight) {
  return static_cast<std::uint16_t>(weight);
}

/// Reads `normal`, `bold`, the other CSS weight keywords, or a numeric weight
/// in the inclusive range 1..1000.
bool parse_style_value(std::string_view text, FontWeight &out);

/// The value of `font-family`: the families to try, most preferred first.
///
/// Names, not a loaded face. Which face a name resolves to depends on
/// `font-size`, `font-weight` and the locale, and those cascade independently
/// of this one -- a rule that changes only the size must not have to know which
/// family won. Resolution happens once, in the widget, with all four in hand.
///
/// Generic names (`system-ui`, `sans-serif`, `monospace`, ...) are kept as
/// written and handed to the font layer, which is the only part that knows what
/// they mean on this platform.
///
/// The list is shared and immutable, so copying one costs a refcount bump --
/// which matters, because an inherited property is copied into every node of a
/// subtree.
class FontFamilyList {
public:
  FontFamilyList() = default;

  /// Empty names are dropped; a list left with nothing in it is the empty list,
  /// which the font layer reads as "the platform's UI font".
  static FontFamilyList of(std::vector<std::string> families);

  bool empty() const { return !data_; }
  std::size_t size() const { return data_ ? data_->families.size() : 0; }

  std::span<const std::string> families() const {
    return data_ ? std::span<const std::string>(data_->families)
                 : std::span<const std::string>();
  }

  /// The first family, or an empty string when the list is empty.
  const std::string &primary() const;

  /// Canonical CSS text, for diagnostics and round-tripping.
  std::string to_string() const;

  bool operator==(const FontFamilyList &other) const;
  std::uint64_t hash() const { return data_ ? data_->hash : 0; }

private:
  struct Data {
    std::vector<std::string> families;
    std::uint64_t hash = 0;
  };

  std::shared_ptr<const Data> data_;
};

/// Reads a comma-separated family list: `"Inter", Helvetica Neue, sans-serif`.
///
/// Quoted names are taken verbatim; unquoted ones are a run of identifiers,
/// whose internal runs of whitespace fold to one space, exactly as CSS reads
/// them. One malformed name invalidates the whole declaration, again as in CSS,
/// so a typo leaves the inherited family standing rather than half-applying.
bool parse_style_value(std::string_view text, FontFamilyList &out);

inline std::uint64_t style_value_hash(const FontFamilyList &list) {
  return list.hash();
}

} // namespace voidui
