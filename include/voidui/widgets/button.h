#pragma once

#include <memory>

#include "voidui/core/context.h"
#include "voidui/core/style.h"
#include "voidui/core/widget.h"
#include "voidui/widgets/text.h"

namespace voidui {

class Button : public Widget {
public:
  VOIDUI_STYLE_SCOPE(Button, "button")

  Button() = default;
  bool focusable() const override { return true; }

  template <WidgetClass T>
  Button(T &&content) : content_(transfer_widget(std::forward<T>(content))) {}

  Button(std::unique_ptr<Widget> content) : content_(std::move(content)) {}

  VOIDUI_WIDGET_SIZE_STYLE

  /// The fluent setters write into the widget's inline style rather than into
  /// fields of their own, so a value set here and a value set in a stylesheet
  /// meet in one cascade -- with this one winning, as an inline style should.
  VOIDUI_FLUENT_METHOD(background, (Brush value),
                       set_style<styles::Background>(std::move(value));)
  VOIDUI_FLUENT_METHOD(padding, (Padding value),
                       set_style<styles::Padding>(value);)
  VOIDUI_FLUENT_METHOD(
      border, (Border value),
      set_style<styles::BorderRadius>(value.get_radius());
      set_style<styles::BorderWidth>(value.get_width());
      set_style<styles::BorderColor>(value.get_brush());)
  VOIDUI_FLUENT_SETTER(on_click, on_click_, std::function<void()>)

  /// Component-owned VSS. It can style the button, its states, and descendants
  /// while remaining below every application stylesheet rule.
  std::shared_ptr<const StyleSheet> default_stylesheet() const override {
    static const std::shared_ptr<const StyleSheet> defaults =
        StyleParser::parse(R"vss(
          button {
            background: #ffffff;
            padding: 8 12;
            border-color: #e5e5e5;
            border-width: 1;
            border-radius: 26;
            cursor: pointer;
            user-select: none;
          }
          button:focus { border-color: #2563eb; }
        )vss",
                           "button.default.vss", StyleOrigin::WidgetDefault)
            .sheet;
    return defaults;
  }

  std::unique_ptr<Widget> clone() const override {
    auto result =
        std::make_unique<Button>(content_ ? clone_widget(*content_) : nullptr);

    result->on_click_ = on_click_;

    return result;
  }

  virtual void register_children(Registrar &registrar) override {
    if (content_) {
      registrar.take_child(std::move(content_));
    }
  }

  virtual Size<float> layout(Constraints constraints,
                             LayoutContext &ctx) override {
    const float border_width =
        std::max(ctx.style.get<styles::BorderWidth>(), 0.0f);
    const Spacing<float> chrome =
        ctx.style.get<styles::Padding>() + Spacing<float>(border_width);
    const float horizontal_chrome = chrome.left + chrome.right;
    const float vertical_chrome = chrome.top + chrome.bottom;
    const Size<Length> &size = ctx.style.layout_size();

    auto available_axis = [](const Length &length, float min,
                             float max) -> float {
      if (const auto *fixed = std::get_if<Length::Fixed>(&length.value)) {
        return std::clamp(fixed->value, min, max);
      }

      return max;
    };

    const float available_width = available_axis(
        size.width, constraints.min_width, constraints.max_width);
    const float available_height = available_axis(
        size.height, constraints.min_height, constraints.max_height);

    const Constraints child_constraints{
        0.0f,
        std::max(available_width - horizontal_chrome, 0.0f),
        0.0f,
        std::max(available_height - vertical_chrome, 0.0f),
    };

    auto child_sizes =
        ctx.constrain_children([&](size_t) { return child_constraints; });

    Size<float> content_size(0.0f, 0.0f);

    for (const auto &child_size : child_sizes) {
      content_size.width = std::max(content_size.width, child_size.width);
      content_size.height = std::max(content_size.height, child_size.height);
    }

    const Size<float> intrinsic_size{
        content_size.width + horizontal_chrome,
        content_size.height + vertical_chrome,
    };
    const Size<float> button_size = constraints.resolve(size, intrinsic_size);
    const Size<float> content_area{
        std::max(button_size.width - horizontal_chrome, 0.0f),
        std::max(button_size.height - vertical_chrome, 0.0f),
    };

    for (size_t i = 0; i < child_sizes.size(); ++i) {
      const auto &child_size = child_sizes[i];
      const Spacing<MarginValue> &margin = ctx.child_margin(i);
      const std::size_t horizontal_auto_count =
          static_cast<std::size_t>(margin.left.is_auto()) +
          static_cast<std::size_t>(margin.right.is_auto());
      const std::size_t vertical_auto_count =
          static_cast<std::size_t>(margin.top.is_auto()) +
          static_cast<std::size_t>(margin.bottom.is_auto());
      const float horizontal_free =
          std::max(content_area.width - child_size.width, 0.0f);
      const float vertical_free =
          std::max(content_area.height - child_size.height, 0.0f);
      Spacing<float> auto_margin;
      if (margin.left.is_auto())
        auto_margin.left =
            horizontal_free / static_cast<float>(horizontal_auto_count);
      if (margin.right.is_auto())
        auto_margin.right =
            horizontal_free / static_cast<float>(horizontal_auto_count);
      if (margin.top.is_auto())
        auto_margin.top =
            vertical_free / static_cast<float>(vertical_auto_count);
      if (margin.bottom.is_auto())
        auto_margin.bottom =
            vertical_free / static_cast<float>(vertical_auto_count);

      const float x =
          chrome.left +
          (horizontal_auto_count == 0 ? horizontal_free * 0.5f : 0.0f);
      const float y =
          chrome.top + (vertical_auto_count == 0 ? vertical_free * 0.5f : 0.0f);
      ctx.place_child(i, Point<float>(x, y), auto_margin);
    }

    return button_size;
  }

  virtual void draw(const DrawContext &ctx, Painter &painter) override {
    const Radius radius = ctx.style.get<styles::BorderRadius>();

    // Hover is no longer a branch in here: `button:hover { background: ... }`
    // in the stylesheet resolves to a different ComputedStyle before this runs.
    painter.fill_rrect(ctx.bounds, radius,
                       Paint(ctx.style.get<styles::Background>()));

    const float border_width = ctx.style.get<styles::BorderWidth>();
    if (border_width > 0.0f) {
      // A border sits inside the silhouette it trims, the way CSS draws one.
      painter.stroke_rrect(ctx.bounds, radius,
                           Paint(ctx.style.get<styles::BorderColor>()),
                           Pen(border_width, StrokeAlign::Inside));
    }
  }

  virtual EventResult on_event(Event &e) {
    if (e.type() == EventType::KeyPressed) {
      const auto &key = static_cast<KeyPressedEvent &>(e);
      if (key.keycode() == Keycode::Return || key.keycode() == Keycode::Space) {
        if (on_click_) on_click_();
        e.request_paint();
        return EventResult::Handled;
      }
    }
    auto pressed = e.dispatch<MousePressedEvent>([](MousePressedEvent &event) {
      return event.button() == MouseButton::Left ? EventResult::Handled
                                                 : EventResult::Unhandled;
    });
    if (pressed == EventResult::Handled)
      return pressed;

    auto released =
        e.dispatch<MouseReleasedEvent>([](MouseReleasedEvent &event) {
          return event.button() == MouseButton::Left ? EventResult::Handled
                                                     : EventResult::Unhandled;
        });
    if (released == EventResult::Handled)
      return released;

    return e.dispatch<MouseClickedEvent>([&](MouseClickedEvent &e) {
      if (e.button() != MouseButton::Left)
        return EventResult::Unhandled;
      if (on_click_) {
        on_click_();
        e.request_paint();
      }
      return EventResult::Handled;
    });
  }

private:
  std::function<void()> on_click_;
  std::unique_ptr<Widget> content_;
};

[[nodiscard]] inline Button button() { return Button{}; }

[[nodiscard]] inline Button button(std::string content) {
  return Button(text(std::move(content)));
}

template <WidgetClass T> [[nodiscard]] Button button(T &&content) {
  return Button(std::forward<T>(content));
}

} // namespace voidui
