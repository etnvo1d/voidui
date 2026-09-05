#pragma once

#include "voidui/core/pixel_snap.h"
#include "voidui/core/widget_tree.h"

namespace voidui::detail {

inline const ComputedStyle &node_style(const Node &node) {
  static const ComputedStyle empty;
  return node.style_node.computed ? *node.style_node.computed : empty;
}

inline Transform node_transform(const Node &node, float scale) {
  const auto transform = snap_translation_to_pixel(
      node_style(node).get<styles::Transform>().matrix(node.size), scale);
  if (transform.is_translation())
    return transform;
  const float x = node.global_pos.x + node.size.width * 0.5f;
  const float y = node.global_pos.y + node.size.height * 0.5f;
  return Transform::translate(x, y).concat(transform).concat(
      Transform::translate(-x, -y));
}

inline const Node *positioned_containing_node(const Node &node) {
  const Position position =
      node_style(*layout_box_of(&node)).get<styles::Position>();
  for (const Node *ancestor = node.parent; ancestor;
       ancestor = ancestor->parent) {
    if (ancestor->style_node.is_transparent)
      continue;
    const auto &style = node_style(*ancestor);
    if (ancestor->widget->overlay_options())
      return position == Position::Fixed &&
                     !style.get<styles::Transform>().establishes_containing_block()
                 ? nullptr : ancestor;
    if (style.get<styles::Transform>().establishes_containing_block() ||
        (position != Position::Fixed &&
         style.get<styles::Position>() != Position::Static))
      return ancestor;
  }
  return nullptr;
}

inline bool untransform_point(const Node &node, Point<float> &point,
                              float scale) {
  const Transform transform = node_transform(node, scale);
  if (transform.is_identity())
    return true;
  Transform inverse;
  if (!transform.inverse(inverse))
    return false;
  point = inverse.apply(point);
  return true;
}

inline bool point_in_node_space(const Node &node, Point<float> &point,
                                float scale) {
  return (!node.parent || node.widget->overlay_options() ||
          point_in_node_space(*node.parent, point, scale)) &&
         untransform_point(node, point, scale);
}

inline Transform window_transform(const Node &node, float scale) {
  const Transform local = node_transform(node, scale);
  return node.parent && !node.widget->overlay_options()
             ? window_transform(*node.parent, scale).concat(local)
             : local;
}
} // namespace voidui::detail
