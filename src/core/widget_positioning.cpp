#include "core/widget_geometry.h"

#include <algorithm>
#include <cmath>

namespace voidui {
namespace {
Rect<float> padding_box(const Node &node) {
  const float border =
      std::max(node.style_node.computed->get<styles::BorderWidth>(), 0.0f);
  return {node.global_pos.x + border, node.global_pos.y + border,
          std::max(0.0f, node.size.width - 2 * border),
          std::max(0.0f, node.size.height - 2 * border)};
}

// Returns the margin-box start. Opposing auto margins absorb positive space;
// over-constrained boxes keep the start edge (horizontal writing direction).
float axis_position(float origin, float extent, float size, Inset start,
                    Inset end, MarginValue margin_start, MarginValue margin_end,
                    float fallback) {
  const float ms = margin_start.fixed_or_zero();
  const float me = margin_end.fixed_or_zero();
  if (start.is_auto())
    return end.is_auto() ? fallback
                         : origin + extent - end.resolve(extent) - size - me;
  float result = origin + start.resolve(extent) + ms;
  if (!end.is_auto() && margin_start.is_auto()) {
    const float free =
        extent - start.resolve(extent) - end.resolve(extent) - size - ms - me;
    result += std::max(0.0f, free) / (margin_end.is_auto() ? 2.0f : 1.0f);
  }
  return result;
}
} // namespace

Rect<float> WidgetTree::containing_block_(const Node *node) const {
  if (const Node *ancestor = detail::positioned_containing_node(*node))
    return padding_box(*ancestor);
  return {{0.0f, 0.0f}, viewport_};
}

void WidgetTree::position_subtree_(Node *node, bool positioned) {
  // Portal subtrees are measured and positioned after ordinary content.
  if (node->widget->overlay_options() && !positioned)
    return;
  const Node *box = layout_box_of(node);
  const auto &style = *box->style_node.computed;
  const Position position = style.get<styles::Position>();
  if (!positioned) {
    const Inset left = style.get<styles::Left>(),
                right = style.get<styles::Right>();
    const Inset top = style.get<styles::Top>(),
                bottom = style.get<styles::Bottom>();
    if (out_of_flow(position)) {
      const Rect<float> cb = containing_block_(node);
      const auto &size = style.layout_size();
      const auto &margin = style.layout_margin();
      const auto axis_constraints = [](float extent, Inset start, Inset end,
                                       const Length &length, float margins) {
        float available = std::max(0.0f, extent - start.resolve(extent) -
                                             end.resolve(extent));
        if (const auto *fixed = std::get_if<Length::Fixed>(&length.value))
          available = std::max(available, fixed->value + margins);
        const bool stretch =
            std::holds_alternative<Length::Auto>(length.value) &&
            !start.is_auto() && !end.is_auto();
        return std::pair{stretch ? available : 0.0f, available};
      };
      auto [min_w, max_w] = axis_constraints(
          cb.size.width, left, right, size.width,
          margin.left.fixed_or_zero() + margin.right.fixed_or_zero());
      auto [min_h, max_h] = axis_constraints(
          cb.size.height, top, bottom, size.height,
          margin.top.fixed_or_zero() + margin.bottom.fixed_or_zero());
      if (std::holds_alternative<Length::Auto>(size.width.value) &&
          (left.is_auto() || right.is_auto()) &&
          (style.get<styles::WhiteSpace>() == WhiteSpace::Nowrap ||
           style.get<styles::WhiteSpace>() == WhiteSpace::Pre))
        max_w = std::numeric_limits<float>::infinity();
      // Auto height is intrinsic unless both vertical insets constrain it.
      if (top.is_auto() || bottom.is_auto())
        max_h = std::numeric_limits<float>::infinity();
      layout_node_(node, {min_w, max_w, min_h, max_h});
      const Node *parent_box = node->parent;
      while (parent_box && parent_box->style_node.is_transparent)
        parent_box = parent_box->parent;
      const auto parent_padding = parent_box ? padding_box(*parent_box) : cb;
      const auto padding =
          parent_box ? parent_box->style_node.computed->get<styles::Padding>()
                     : Padding{};
      const Point<float> target{
          axis_position(cb.origin.x, cb.size.width, box->size.width, left,
                        right, margin.left, margin.right,
                        parent_padding.origin.x + padding.left +
                            margin.left.fixed_or_zero()),
          axis_position(cb.origin.y, cb.size.height, box->size.height, top,
                        bottom, margin.top, margin.bottom,
                        parent_padding.origin.y + padding.top +
                            margin.top.fixed_or_zero())};
      translate_subtree(
          *node, {target.x - box->global_pos.x, target.y - box->global_pos.y});
    } else if (position == Position::Relative) {
      const Node *parent = node->parent;
      while (parent && parent->style_node.is_transparent)
        parent = parent->parent;
      Size<float> reference = parent ? padding_box(*parent).size : viewport_;
      if (parent) {
        const auto padding =
            parent->style_node.computed->get<styles::Padding>();
        reference.width =
            std::max(0.0f, reference.width - padding.left - padding.right);
        reference.height =
            std::max(0.0f, reference.height - padding.top - padding.bottom);
      }
      translate_subtree(*node,
                        {left.is_auto() ? -right.resolve(reference.width)
                                        : left.resolve(reference.width),
                         top.is_auto() ? -bottom.resolve(reference.height)
                                       : top.resolve(reference.height)});
    } else if (position == Position::Sticky) {
      const Node *scroll = node->parent;
      while (scroll && !scroll->widget->clips_children())
        scroll = scroll->parent;
      const Rect<float> port = scroll ? scroll->widget->children_clip(
                                            {scroll->global_pos, scroll->size})
                                      : Rect<float>{{0.0f, 0.0f}, viewport_};
      Point<float> target = box->global_pos;
      const auto sticky_axis = [](float value, float size, float origin,
                                  float extent, Inset start, Inset end) {
        if (!end.is_auto())
          value = std::min(value, origin + extent - end.resolve(extent) - size);
        if (!start.is_auto())
          value = std::max(value, origin + start.resolve(extent));
        return value;
      };
      target.x = sticky_axis(target.x, box->size.width, port.origin.x,
                             port.size.width, left, right);
      target.y = sticky_axis(target.y, box->size.height, port.origin.y,
                             port.size.height, top, bottom);
      const Node *parent = node->parent;
      while (parent && parent->style_node.is_transparent)
        parent = parent->parent;
      if (parent) {
        const auto bounds = padding_box(*parent);
        target.x = std::clamp(
            target.x, bounds.origin.x,
            std::max(bounds.origin.x,
                     bounds.origin.x + bounds.size.width - box->size.width));
        target.y = std::clamp(
            target.y, bounds.origin.y,
            std::max(bounds.origin.y,
                     bounds.origin.y + bounds.size.height - box->size.height));
      }
      translate_subtree(
          *node, {target.x - box->global_pos.x, target.y - box->global_pos.y});
    }
  }
  for (auto &child : node->children)
    position_subtree_(child.get(), node->style_node.is_transparent);
  for (auto &child : node->internal_children)
    position_subtree_(child.get());
}
} // namespace voidui
