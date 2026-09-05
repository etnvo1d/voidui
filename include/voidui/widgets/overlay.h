#pragma once

#include "voidui/core/context.h"
#include "voidui/core/overlay.h"

namespace voidui {

/// A portal whose content stays owned by its declaring component. Modal adds
/// defaults to this same implementation; fluent calls preserve concrete types.
class Overlay : public Widget {
public:
  VOIDUI_STYLE_SCOPE(Overlay, "overlay")
  template <WidgetClass T>
  explicit Overlay(T &&content)
      : content_(transfer_widget(std::forward<T>(content))) {}
  explicit Overlay(std::unique_ptr<Widget> content)
      : content_(std::move(content)) {}

#define VOIDUI_OVERLAY_OPTION(name, field, type)                               \
  template <typename Self> Self &&name(this Self &&self, type value) {         \
    self.options_.field = std::move(value);                                    \
    return std::forward<Self>(self);                                           \
  }
  VOIDUI_OVERLAY_OPTION(open, open, bool)
  VOIDUI_OVERLAY_OPTION(placement, placement, OverlayPlacement)
  VOIDUI_OVERLAY_OPTION(interactive, interactive, bool)
  VOIDUI_OVERLAY_OPTION(close_on_escape, dismiss_on_escape, bool)
  VOIDUI_OVERLAY_OPTION(close_on_outside_press, dismiss_on_outside_press, bool)
  VOIDUI_OVERLAY_OPTION(backdrop, backdrop, Color)
#undef VOIDUI_OVERLAY_OPTION
  template <typename Self>
  Self &&options(this Self &&self, OverlayOptions options) {
    self.options_ = std::move(options);
    return std::forward<Self>(self);
  }
  template <typename Self> Self &&open(this Self &&self, State<bool> state) {
    self.options_.open = state.get();
    self.on_close_ = [state](OverlayDismissReason) { state.set(false); };
    return std::forward<Self>(self);
  }
  template <typename Self>
  Self &&on_close(this Self &&self,
                  std::function<void(OverlayDismissReason)> fn) {
    self.on_close_ = std::move(fn);
    return std::forward<Self>(self);
  }
  template <typename Self> Self &&gap(this Self &&self, float value) {
    self.options_.gap = std::max(value, 0.0f);
    return std::forward<Self>(self);
  }
  template <typename Self> Self &&max_width(this Self &&self, float value) {
    self.options_.max_width = std::max(value, 0.0f);
    return std::forward<Self>(self);
  }
#define VOIDUI_OVERLAY_STYLE(name, property)                                   \
  template <typename Self>                                                     \
  Self &&name(this Self &&self, styles::property::Value value) {               \
    self.template set_style<styles::property>(std::move(value));               \
    return std::forward<Self>(self);                                           \
  }
  VOIDUI_OVERLAY_STYLE(width, Width)
  VOIDUI_OVERLAY_STYLE(height, Height)
  VOIDUI_OVERLAY_STYLE(padding, Padding)
  VOIDUI_OVERLAY_STYLE(background, Background)
#undef VOIDUI_OVERLAY_STYLE
  template <typename Self> Self &&size(this Self &&self, Size<Length> value) {
    self.template set_style<styles::Width>(std::move(value.width));
    self.template set_style<styles::Height>(std::move(value.height));
    return std::forward<Self>(self);
  }
  template <typename Self> Self &&margin(this Self &&self, Margin value) {
    self.template set_style<styles::MarginTop>(value.top);
    self.template set_style<styles::MarginRight>(value.right);
    self.template set_style<styles::MarginBottom>(value.bottom);
    self.template set_style<styles::MarginLeft>(value.left);
    return std::forward<Self>(self);
  }

  const OverlayOptions *overlay_options() const override { return &options_; }
  bool clips_children() const override { return true; }
  void register_children(Registrar &registrar) override {
    if (content_)
      registrar.take_child(std::move(content_));
  }
  Size<float> layout(Constraints constraints, LayoutContext &ctx) override {
    return detail::layout_linear(constraints, ctx, 0.0f, true);
  }
  void draw(const DrawContext &ctx, Painter &painter) override {
    detail::draw_container_box(ctx, painter);
  }
  EventResult on_event(Event &event) override {
    if (event.type() != EventType::OverlayDismissed)
      return EventResult::Unhandled;
    // A close callback may replace its own declaration.
    auto callback = on_close_;
    if (callback)
      callback(static_cast<OverlayDismissedEvent &>(event).reason());
    return EventResult::Handled;
  }
  std::unique_ptr<Widget> clone() const override {
    auto result = std::make_unique<Overlay>(clone_content_());
    copy_overlay_to_(*result);
    return result;
  }

protected:
  std::unique_ptr<Widget> clone_content_() const {
    return content_ ? clone_widget(*content_) : nullptr;
  }
  void copy_overlay_to_(Overlay &target) const {
    target.options_ = options_;
    target.on_close_ = on_close_;
  }
  OverlayOptions options_;

private:
  std::unique_ptr<Widget> content_;
  std::function<void(OverlayDismissReason)> on_close_;
};

template <WidgetClass T> [[nodiscard]] Overlay overlay(T &&content) {
  return Overlay(std::forward<T>(content));
}
} // namespace voidui
