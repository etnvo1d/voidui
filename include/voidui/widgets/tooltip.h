#pragma once

#include "voidui/widgets/overlay.h"
#include "voidui/widgets/text.h"

namespace voidui {

/// Wraps one trigger without changing its natural size. The bubble is an
/// internal, non-interactive overlay exposed as tooltip::part(bubble).
class Tooltip : public Widget {
public:
  VOIDUI_STYLE_SCOPE(Tooltip, "tooltip")

  Tooltip(std::unique_ptr<Widget> trigger, std::string explanation)
      : trigger_(std::move(trigger)), explanation_(std::move(explanation)) {}

  VOIDUI_WIDGET_SIZE_STYLE
  VOIDUI_FLUENT_SETTER(placement, placement_, OverlayPlacement)
  VOIDUI_FLUENT_METHOD(delay, (std::chrono::milliseconds value),
                       delay_ = std::max(value, std::chrono::milliseconds{0});)
  VOIDUI_FLUENT_METHOD(gap, (float value), gap_ = std::max(value, 0.0f);)
  VOIDUI_FLUENT_METHOD(max_width, (float value),
                       max_width_ = std::max(value, 0.0f);)

  void register_children(Registrar &registrar) override {
    if (trigger_)
      registrar.take_child(std::move(trigger_));
    if (explanation_.empty())
      return;
    OverlayOptions options;
    options.placement = placement_;
    options.trigger = OverlayTrigger::HoverOrFocus;
    options.interactive = false;
    options.dismiss_on_escape = true;
    options.dismiss_on_press = true;
    options.dismiss_on_scroll = true;
    options.delay = delay_;
    options.gap = gap_;
    options.max_width = max_width_;
    registrar.take_internal_child(
        transfer_widget(overlay(text(explanation_)).options(options)),
        "bubble");
  }

  Size<float> layout(Constraints constraints, LayoutContext &ctx) override {
    const auto &specified = ctx.style.layout_size();
    Constraints child_constraints = constraints;
    if (const auto *width = std::get_if<Length::Fixed>(&specified.width.value))
      child_constraints.max_width = std::clamp(
          width->value, constraints.min_width, constraints.max_width);
    if (const auto *height =
            std::get_if<Length::Fixed>(&specified.height.value))
      child_constraints.max_height = std::clamp(
          height->value, constraints.min_height, constraints.max_height);
    Size<float> size{};
    if (ctx.child_count()) {
      size = ctx.constrain_child(0, child_constraints);
      ctx.place_child(0, {});
    }
    return constraints.resolve(specified, size);
  }
  void draw(const DrawContext &, Painter &) override {}
  EventResult on_event(Event &) override { return EventResult::Unhandled; }
  std::unique_ptr<Widget> clone() const override {
    auto result = std::make_unique<Tooltip>(
        trigger_ ? clone_widget(*trigger_) : nullptr, explanation_);
    result->placement_ = placement_;
    result->delay_ = delay_;
    result->gap_ = gap_;
    result->max_width_ = max_width_;
    return result;
  }
  std::shared_ptr<const StyleSheet> default_stylesheet() const override {
    static const auto defaults =
        StyleParser::parse(R"vss(
      tooltip::part(bubble) {
        padding: 6px 10px;
        background: #18181b;
        color: white;
        font-size: 12px;
        border-radius: 6px;
        user-select: none;
      }
    )vss",
                           "tooltip.default.vss", StyleOrigin::WidgetDefault)
            .sheet;
    return defaults;
  }

private:
  std::unique_ptr<Widget> trigger_;
  std::string explanation_;
  OverlayPlacement placement_ = OverlayPlacement::Top;
  std::chrono::milliseconds delay_{500};
  float gap_ = 8.0f;
  float max_width_ = 320.0f;
};

template <WidgetClass T>
[[nodiscard]] Tooltip tooltip(T &&trigger, std::string explanation) {
  return Tooltip(transfer_widget(std::forward<T>(trigger)),
                 std::move(explanation));
}

} // namespace voidui
