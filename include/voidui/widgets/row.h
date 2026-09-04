#pragma once

#include <algorithm>
#include <cmath>
#include <limits>
#include <memory>
#include <utility>
#include <vector>

#include "voidui/core/border.h"
#include "voidui/core/context.h"
#include "voidui/core/style.h"
#include "voidui/core/widget.h"

namespace voidui {

/// Lays out its children from left to right.
///
/// Children receive the row's available height. Auto/Fixed children take their
/// natural width first; Fill/Flex children share the remaining width, with
/// Flex values used as weights.
class Row : public Widget {
public:
  VOIDUI_STYLE_SCOPE(Row, "row")

  Row() = default;

  template <WidgetClass... Children>
    requires(sizeof...(Children) > 0)
  explicit Row(Children &&...children) {
    (children_.push_back(transfer_widget(std::forward<Children>(children))),
     ...);
  }

  explicit Row(std::vector<std::unique_ptr<Widget>> children)
      : children_(std::move(children)) {}

  VOIDUI_WIDGET_SIZE_STYLE
  VOIDUI_FLUENT_METHOD(gap, (float value), gap_ = std::max(value, 0.0f);)
  VOIDUI_FLUENT_METHOD(background, (Brush value),
                       set_style<styles::Background>(std::move(value));)
  VOIDUI_FLUENT_METHOD(padding, (Padding value),
                       set_style<styles::Padding>(value);)
  VOIDUI_FLUENT_METHOD(
      border, (Border value),
      set_style<styles::BorderRadius>(value.get_radius());
      set_style<styles::BorderWidth>(value.get_width());
      set_style<styles::BorderColor>(value.get_brush());)

  template <WidgetClass T> Row &add(T &&child) & {
    children_.push_back(transfer_widget(std::forward<T>(child)));
    return *this;
  }

  template <WidgetClass T> Row &&add(T &&child) && {
    children_.push_back(transfer_widget(std::forward<T>(child)));
    return std::move(*this);
  }

  Row &add(std::unique_ptr<Widget> child) & {
    children_.push_back(std::move(child));
    return *this;
  }

  Row &&add(std::unique_ptr<Widget> child) && {
    children_.push_back(std::move(child));
    return std::move(*this);
  }

  std::unique_ptr<Widget> clone() const override {
    std::vector<std::unique_ptr<Widget>> children;
    children.reserve(children_.size());
    for (const auto &child : children_) {
      if (child)
        children.push_back(clone_widget(*child));
    }

    auto copy = std::make_unique<Row>(std::move(children));
    copy->gap_ = gap_;
    return copy;
  }

  void register_children(Registrar &registrar) override {
    for (auto &child : children_) {
      if (child)
        registrar.take_child(std::move(child));
    }
    children_.clear();
  }

  Size<float> layout(Constraints constraints, LayoutContext &ctx) override {
    const float border_width =
        std::max(ctx.style.get<styles::BorderWidth>(), 0.0f);
    const Spacing<float> chrome =
        ctx.style.get<styles::Padding>() + Spacing<float>(border_width);
    const float horizontal_chrome = chrome.left + chrome.right;
    const float vertical_chrome = chrome.top + chrome.bottom;
    const Size<Length> &size = ctx.style.layout_size();
    const float available_outer_height = available_axis_(
        size.height, constraints.min_height, constraints.max_height);
    const float available_outer_width = available_axis_(
        size.width, constraints.min_width, constraints.max_width);
    const float available_height =
        subtract_chrome_(available_outer_height, vertical_chrome);
    const float available_width =
        subtract_chrome_(available_outer_width, horizontal_chrome);
    std::vector<Size<float>> child_sizes(ctx.child_count());
    std::vector<float> flex_weights(ctx.child_count(), 0.0f);

    float occupied_width = gap_total_(ctx.child_count());
    float total_flex = 0.0f;
    std::size_t horizontal_auto_count = 0;
    for (size_t i = 0; i < ctx.child_count(); ++i) {
      const Spacing<MarginValue> &margin = ctx.child_margin(i);
      horizontal_auto_count += margin.left.is_auto();
      horizontal_auto_count += margin.right.is_auto();
      flex_weights[i] = flex_weight_(ctx.child_layout_size(i).width);
      if (flex_weights[i] > 0.0f) {
        total_flex += flex_weights[i];
        const Spacing<float> fixed_margin = resolve_fixed_margin(margin);
        occupied_width += fixed_margin.left + fixed_margin.right;
        continue;
      }

      child_sizes[i] = ctx.constrain_child(
          i, Constraints{0.0f, std::numeric_limits<float>::infinity(), 0.0f,
                         available_height});
      occupied_width += child_sizes[i].width;
    }

    if (total_flex > 0.0f) {
      const float remaining =
          std::isfinite(available_width)
              ? std::max(available_width - occupied_width, 0.0f)
              : 0.0f;
      for (size_t i = 0; i < ctx.child_count(); ++i) {
        if (flex_weights[i] <= 0.0f)
          continue;
        const float width = remaining * flex_weights[i] / total_flex;
        const Spacing<float> margin = resolve_fixed_margin(ctx.child_margin(i));
        const float outer_width = width + margin.left + margin.right;
        child_sizes[i] = ctx.constrain_child(
            i, Constraints{outer_width, outer_width, 0.0f, available_height});
      }
    }

    Size<float> intrinsic;
    for (size_t i = 0; i < child_sizes.size(); ++i) {
      if (i > 0)
        intrinsic.width += gap_;
      intrinsic.width += child_sizes[i].width;
      intrinsic.height = std::max(intrinsic.height, child_sizes[i].height);
    }

    intrinsic.width += horizontal_chrome;
    intrinsic.height += vertical_chrome;
    const Size<float> row_size = constraints.resolve(size, intrinsic);
    const float content_width =
        std::max(row_size.width - horizontal_chrome, 0.0f);
    const float content_height =
        std::max(row_size.height - vertical_chrome, 0.0f);
    const float horizontal_auto =
        horizontal_auto_count == 0
            ? 0.0f
            : std::max(content_width - (intrinsic.width - horizontal_chrome),
                       0.0f) /
                  static_cast<float>(horizontal_auto_count);

    float x = chrome.left;
    for (size_t i = 0; i < child_sizes.size(); ++i) {
      const Spacing<MarginValue> &margin = ctx.child_margin(i);
      Spacing<float> auto_margin;
      if (margin.left.is_auto())
        auto_margin.left = horizontal_auto;
      if (margin.right.is_auto())
        auto_margin.right = horizontal_auto;

      const std::size_t vertical_auto_count =
          static_cast<std::size_t>(margin.top.is_auto()) +
          static_cast<std::size_t>(margin.bottom.is_auto());
      const float vertical_auto =
          vertical_auto_count == 0
              ? 0.0f
              : std::max(content_height - child_sizes[i].height, 0.0f) /
                    static_cast<float>(vertical_auto_count);
      if (margin.top.is_auto())
        auto_margin.top = vertical_auto;
      if (margin.bottom.is_auto())
        auto_margin.bottom = vertical_auto;

      ctx.place_child(i, Point<float>(x, chrome.top), auto_margin);
      x += child_sizes[i].width + auto_margin.left + auto_margin.right + gap_;
    }

    return row_size;
  }

  void draw(const DrawContext &ctx, Painter &painter) override {
    const Radius radius = ctx.style.get<styles::BorderRadius>();
    painter.fill_rrect(ctx.bounds, radius,
                       Paint(ctx.style.get<styles::Background>()));

    const float border_width = ctx.style.get<styles::BorderWidth>();
    if (border_width > 0.0f) {
      painter.stroke_rrect(ctx.bounds, radius,
                           Paint(ctx.style.get<styles::BorderColor>()),
                           Pen(border_width, StrokeAlign::Inside));
    }
  }

  EventResult on_event(Event &) override { return EventResult::Unhandled; }

private:
  static float available_axis_(const Length &length, float min, float max) {
    if (const auto *fixed = std::get_if<Length::Fixed>(&length.value))
      return std::clamp(fixed->value, min, max);
    return max;
  }

  static float subtract_chrome_(float available, float chrome) {
    return std::isfinite(available) ? std::max(available - chrome, 0.0f)
                                    : available;
  }

  float gap_total_(size_t child_count) const {
    return child_count > 1 ? gap_ * static_cast<float>(child_count - 1) : 0.0f;
  }

  static float flex_weight_(const Length &length) {
    if (std::holds_alternative<Length::Fill>(length.value))
      return 1.0f;
    if (const auto *flex = std::get_if<Length::Flex>(&length.value))
      return static_cast<float>(std::max<std::uint16_t>(flex->value, 1));
    return 0.0f;
  }

  std::vector<std::unique_ptr<Widget>> children_;
  float gap_ = 0.0f;
};

[[nodiscard]] inline Row row() { return Row{}; }

template <WidgetClass... Children>
  requires(sizeof...(Children) > 0)
[[nodiscard]] Row row(Children &&...children) {
  return Row(std::forward<Children>(children)...);
}

} // namespace voidui
