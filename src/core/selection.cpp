#include "voidui/core/selection.h"

#include "voidui/core/style/value.h"

namespace voidui {

bool parse_style_value(std::string_view text, UserSelect &out) {
  text = style_trim(text);
  if (text == "auto")
    out = UserSelect::Auto;
  else if (text == "none")
    out = UserSelect::None;
  else if (text == "text")
    out = UserSelect::Text;
  else if (text == "all")
    out = UserSelect::All;
  else
    return false;
  return true;
}

} // namespace voidui
