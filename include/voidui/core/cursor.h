#pragma once

#include <cstdint>
#include <string_view>

namespace voidui {

/// Platform-independent values accepted by the VSS `cursor` property.
/// A byte is enough for storage in PropertyValue and the platform cursor cache.
enum class CursorShape : std::uint8_t {
  Auto,
  Default,
  None,
  Pointer,
  Text,
  Wait,
  Progress,
  Crosshair,
  Move,
  NotAllowed,
  HorizontalResize,
  VerticalResize,
  NorthwestSoutheastResize,
  NortheastSouthwestResize,
  NorthwestResize,
  NorthResize,
  NortheastResize,
  EastResize,
  SoutheastResize,
  SouthResize,
  SouthwestResize,
  WestResize,
  Count,
};

bool parse_style_value(std::string_view text, CursorShape &out);

} // namespace voidui
