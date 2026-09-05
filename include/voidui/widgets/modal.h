#pragma once

#include "voidui/widgets/overlay.h"

namespace voidui {

/// A centered, initially closed portal with its own backdrop and focus scope.
/// Bind .open(State<bool>) for automatic close-state synchronization.
class Modal : public Overlay {
public:
  VOIDUI_STYLE_SCOPE(Modal, "modal")
  template <WidgetClass T>
  explicit Modal(T &&content)
      : Modal(transfer_widget(std::forward<T>(content))) {}
  explicit Modal(std::unique_ptr<Widget> content)
      : Overlay(std::move(content)) {
    options_.open = false;
    options_.modal = true;
    options_.placement = OverlayPlacement::Center;
    options_.dismiss_on_escape = true;
    options_.max_width = 560.0f;
    options_.viewport_padding = 24.0f;
  }
  std::unique_ptr<Widget> clone() const override {
    auto result = std::make_unique<Modal>(clone_content_());
    copy_overlay_to_(*result);
    return result;
  }
  std::shared_ptr<const StyleSheet> default_stylesheet() const override {
    static const auto defaults =
        StyleParser::parse(R"vss(
      modal {
        background: white;
        color: #18181b;
        padding: 24px;
        border-radius: 12px;
        box-shadow: 0px 8px 32px #00000030;
      }
    )vss",
                           "modal.default.vss", StyleOrigin::WidgetDefault)
            .sheet;
    return defaults;
  }
};

template <WidgetClass T> [[nodiscard]] Modal modal(T &&content) {
  return Modal(std::forward<T>(content));
}
} // namespace voidui
