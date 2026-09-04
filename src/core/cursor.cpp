#include "voidui/core/cursor.h"

#include "voidui/core/style/value.h"

namespace voidui {

bool parse_style_value(std::string_view text, CursorShape &out) {
  text = style_trim(text);
  if (text == "auto")
    out = CursorShape::Auto;
  else if (text == "default")
    out = CursorShape::Default;
  else if (text == "none")
    out = CursorShape::None;
  else if (text == "pointer")
    out = CursorShape::Pointer;
  else if (text == "text" || text == "vertical-text")
    out = CursorShape::Text;
  else if (text == "wait")
    out = CursorShape::Wait;
  else if (text == "progress")
    out = CursorShape::Progress;
  else if (text == "crosshair")
    out = CursorShape::Crosshair;
  else if (text == "move" || text == "all-scroll")
    out = CursorShape::Move;
  else if (text == "not-allowed" || text == "no-drop")
    out = CursorShape::NotAllowed;
  else if (text == "ew-resize" || text == "col-resize")
    out = CursorShape::HorizontalResize;
  else if (text == "ns-resize" || text == "row-resize")
    out = CursorShape::VerticalResize;
  else if (text == "nwse-resize")
    out = CursorShape::NorthwestSoutheastResize;
  else if (text == "nesw-resize")
    out = CursorShape::NortheastSouthwestResize;
  else if (text == "nw-resize")
    out = CursorShape::NorthwestResize;
  else if (text == "n-resize")
    out = CursorShape::NorthResize;
  else if (text == "ne-resize")
    out = CursorShape::NortheastResize;
  else if (text == "e-resize")
    out = CursorShape::EastResize;
  else if (text == "se-resize")
    out = CursorShape::SoutheastResize;
  else if (text == "s-resize")
    out = CursorShape::SouthResize;
  else if (text == "sw-resize")
    out = CursorShape::SouthwestResize;
  else if (text == "w-resize")
    out = CursorShape::WestResize;
  else
    return false;
  return true;
}

} // namespace voidui
