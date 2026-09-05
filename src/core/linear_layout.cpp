#include "voidui/core/context.h"

#include <algorithm>
#include <cmath>
#include <limits>

namespace voidui::detail {

Size<float> layout_linear(Constraints constraints, LayoutContext &ctx,
                          float gap, bool vertical) {
  // Work in main/cross coordinates. Row and Column share every sizing,
  // flex and auto-margin rule, so fixing one axis cannot leave the other
  // behind.
  const auto oriented = [vertical](auto size) {
    if (vertical)
      std::swap(size.width, size.height);
    return size;
  };
  const auto oriented_margin = [vertical](auto margin) {
    if (vertical) {
      std::swap(margin.left, margin.top);
      std::swap(margin.right, margin.bottom);
    }
    return margin;
  };
  if (vertical) {
    std::swap(constraints.min_width, constraints.min_height);
    std::swap(constraints.max_width, constraints.max_height);
  }
  const auto chrome = oriented_margin(
      ctx.style.get<styles::Padding>() +
      Spacing<float>(std::max(ctx.style.get<styles::BorderWidth>(), 0.0f)));
  const float main_chrome = chrome.left + chrome.right;
  const float cross_chrome = chrome.top + chrome.bottom;
  const auto size = oriented(ctx.style.layout_size());
  const auto available_axis = [](const Length &length, float min, float max,
                                 float chrome) {
    if (const auto *fixed = std::get_if<Length::Fixed>(&length.value))
      max = std::clamp(fixed->value, min, max);
    return std::isfinite(max) ? std::max(0.0f, max - chrome) : max;
  };
  const float available_main = available_axis(
      size.width, constraints.min_width, constraints.max_width, main_chrome);
  const float available_cross =
      available_axis(size.height, constraints.min_height,
                     constraints.max_height, cross_chrome);
  const auto measure = [&](size_t index, float min, float max) {
    return oriented(ctx.constrain_child(
        index, vertical ? Constraints(0, available_cross, min, max)
                        : Constraints(min, max, 0, available_cross)));
  };
  struct Child {
    Size<float> size;
    float flex = 0;
  };
  std::vector<Child> children(ctx.child_count());
  float occupied =
      children.size() > 1 ? gap * static_cast<float>(children.size() - 1) : 0;
  float total_flex = 0;
  size_t main_auto_count = 0;
  for (size_t i = 0; i < children.size(); ++i) {
    const auto margin = oriented_margin(ctx.child_margin(i));
    main_auto_count += margin.left.is_auto() + margin.right.is_auto();
    const Length length = oriented(ctx.child_layout_size(i)).width;
    float weight =
        std::holds_alternative<Length::Fill>(length.value) ? 1.0f : 0.0f;
    if (const auto *flex = std::get_if<Length::Flex>(&length.value))
      weight = static_cast<float>(std::max<std::uint16_t>(1, flex->value));
    children[i].flex = weight;
    total_flex += weight;
    if (weight > 0)
      occupied += margin.left.fixed_or_zero() + margin.right.fixed_or_zero();
    else {
      children[i].size = measure(i, 0, std::numeric_limits<float>::infinity());
      occupied += children[i].size.width;
    }
  }
  if (total_flex > 0) {
    const float remaining = std::isfinite(available_main)
                                ? std::max(available_main - occupied, 0.0f)
                                : 0;
    for (size_t i = 0; i < children.size(); ++i) {
      if (children[i].flex <= 0)
        continue;
      const auto margin = oriented_margin(ctx.child_margin(i));
      const float outer = remaining * children[i].flex / total_flex +
                          margin.left.fixed_or_zero() +
                          margin.right.fixed_or_zero();
      children[i].size = measure(i, outer, outer);
    }
  }
  Size<float> intrinsic;
  for (size_t i = 0; i < children.size(); ++i) {
    intrinsic.width += children[i].size.width + (i ? gap : 0);
    intrinsic.height = std::max(intrinsic.height, children[i].size.height);
  }
  const auto resolved = constraints.resolve(
      size, {intrinsic.width + main_chrome, intrinsic.height + cross_chrome});
  const float main_auto =
      main_auto_count
          ? std::max(resolved.width - main_chrome - intrinsic.width, 0.0f) /
                static_cast<float>(main_auto_count)
          : 0;
  const float content_cross = std::max(resolved.height - cross_chrome, 0.0f);
  float advance = chrome.left;
  for (size_t i = 0; i < children.size(); ++i) {
    const auto margin = oriented_margin(ctx.child_margin(i));
    const auto cross_count = margin.top.is_auto() + margin.bottom.is_auto();
    const float cross_auto =
        cross_count ? std::max(content_cross - children[i].size.height, 0.0f) /
                          static_cast<float>(cross_count)
                    : 0;
    Spacing<float> distributed{margin.left.is_auto() ? main_auto : 0,
                               margin.top.is_auto() ? cross_auto : 0,
                               margin.right.is_auto() ? main_auto : 0,
                               margin.bottom.is_auto() ? cross_auto : 0};
    ctx.place_child(i,
                    vertical ? Point<float>(chrome.top, advance)
                             : Point<float>(advance, chrome.top),
                    oriented_margin(distributed));
    advance +=
        children[i].size.width + distributed.left + distributed.right + gap;
  }
  return oriented(resolved);
}

void draw_container_box(const DrawContext &ctx, Painter &painter) {
  const Radius radius = ctx.style.get<styles::BorderRadius>();
  painter.fill_rrect(ctx.bounds, radius,
                     Paint(ctx.style.get<styles::Background>()));
  const float border = ctx.style.get<styles::BorderWidth>();
  if (border > 0)
    painter.stroke_rrect(ctx.bounds, radius,
                         Paint(ctx.style.get<styles::BorderColor>()),
                         Pen(border, StrokeAlign::Inside));
}
} // namespace voidui::detail
