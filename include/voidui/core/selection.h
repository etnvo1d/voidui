#pragma once

#include <cstdint>
#include <string_view>

namespace voidui {

/// CSS-compatible policy for read-only text selection.
enum class UserSelect : std::uint8_t {
  Auto,
  None,
  Text,
  All,
};

bool parse_style_value(std::string_view text, UserSelect &out);

} // namespace voidui
