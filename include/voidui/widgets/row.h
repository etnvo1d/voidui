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
  bool is_flex_container() const override { return true; }

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
  VOIDUI_FLUENT_METHOD(border, (Border value),
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
    return detail::layout_linear(constraints, ctx, gap_, false);
  }

  void draw(const DrawContext &ctx, Painter &painter) override {
    detail::draw_container_box(ctx, painter);
  }

  EventResult on_event(Event &) override { return EventResult::Unhandled; }

private:
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
