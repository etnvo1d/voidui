#pragma once

#include <functional>
#include <utility>

#include "voidui/core/context.h"
#include "voidui/core/style.h"
#include "voidui/core/widget.h"

namespace voidui {

/// A widget that hands its rectangle straight to a callback.
///
/// Useful for exercising or embedding raw Painter drawing without inventing a
/// widget for every shape.
class Canvas : public Widget {
public:
  VOIDUI_STYLE_SCOPE(Canvas, "canvas")

  using DrawFn = std::function<void(Rect<float>, Painter &)>;

  explicit Canvas(DrawFn draw) : draw_(std::move(draw)) {}

  Canvas(DrawFn draw, Size<Length> size) : draw_(std::move(draw)) {
    set_style<styles::Width>(std::move(size.width));
    set_style<styles::Height>(std::move(size.height));
  }

  VOIDUI_WIDGET_SIZE_STYLE

  std::shared_ptr<const StyleSheet> default_stylesheet() const override {
    static const std::shared_ptr<const StyleSheet> defaults =
        StyleParser::parse("canvas { width: fill; height: fill; }",
                           "canvas.default.vss", StyleOrigin::WidgetDefault)
            .sheet;
    return defaults;
  }

  std::unique_ptr<Widget> clone() const override {
    return std::make_unique<Canvas>(draw_);
  }

  void register_children(Registrar &) override {}

  Size<float> layout(Constraints constraints, LayoutContext &ctx) override {
    return constraints.resolve(ctx.style.layout_size(),
                               Size<float>(0.0f, 0.0f));
  }

  void draw(const DrawContext &ctx, Painter &painter) override {
    if (draw_)
      draw_(ctx.bounds, painter);
  }

  EventResult on_event(Event &) override { return EventResult::Unhandled; }

private:
  DrawFn draw_;
};

} // namespace voidui
