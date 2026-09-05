#pragma once

#include "voidui/core/color.h"
#include <chrono>

namespace voidui {

enum class OverlayPlacement { Top, Bottom, Left, Right, Center };
enum class OverlayTrigger { Manual, HoverOrFocus };
enum class OverlayDismissReason {
  Escape,
  OutsidePress,
  Press,
  Scroll,
  OwnerClosed,
  ModalOpened,
  WindowFocusLost
};

/// An overlay is owned and styled in the ordinary widget tree, but measured
/// outside normal flow and painted in window coordinates. Its nearest
/// non-transparent parent is its anchor. Newly opened entries paint on top.
struct OverlayOptions {
  OverlayPlacement placement = OverlayPlacement::Bottom;
  OverlayTrigger trigger = OverlayTrigger::Manual;
  bool open = true;
  bool interactive = true;
  bool modal = false;
  bool dismiss_on_escape = false;
  bool dismiss_on_outside_press = false;
  bool dismiss_on_press = false;
  bool dismiss_on_scroll = false;
  bool match_anchor_width = false;
  bool constrain_to_anchor_side = false;
  bool dismiss_on_focus_loss = false;
  float gap = 8.0f;
  float viewport_padding = 8.0f;
  float max_width = 320.0f;
  std::chrono::milliseconds delay{0};
  Color backdrop = Color(0, 0, 0, 96);
};

} // namespace voidui
