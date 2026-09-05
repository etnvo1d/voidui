#include "core/widget_geometry.h"
#include "voidui/core/widget_tree.h"
#include "voidui/core/overlay.h"

#include <algorithm>

namespace voidui {
namespace {
using detail::node_style;
using detail::node_transform;

bool z_applies(const Node &node) {
  if (node_style(node).get<styles::Position>() != Position::Static)
    return true;
  const Node *parent = node.parent;
  while (parent && parent->style_node.is_transparent)
    parent = parent->parent;
  return parent && parent->widget->is_flex_container();
}

int stack_level(const Node &node) {
  const auto z = node_style(node).get<styles::ZIndex>();
  return z_applies(node) && !z.automatic ? z.value : 0;
}

bool stacking_context(const Node &node) {
  if (node.style_node.is_transparent)
    return false;
  const auto &style = node_style(node);
  const auto position = style.get<styles::Position>();
  return (z_applies(node) && !style.get<styles::ZIndex>().automatic) ||
         position == Position::Fixed || position == Position::Sticky ||
         style.get<styles::Transform>().establishes_containing_block() ||
         style.get<styles::Opacity>() < 1.0f;
}

bool participates(const Node &node) {
  return !node.style_node.is_transparent &&
         (node_style(node).get<styles::Position>() != Position::Static ||
          stacking_context(node));
}

template <class Fn> void children(Node *node, Fn &&fn) {
  for (auto &child : node->children)
    fn(child.get());
  for (auto &child : node->internal_children)
    fn(child.get());
}

// Follow containing blocks rather than DOM ancestry for clipping. This also
// handles an absolute box nested inside a viewport-fixed subtree.
bool clips_target(const Node &ancestor, const Node &target) {
  if (!ancestor.widget->clips_children())
    return false;
  const Node *node = &target;
  while (node && node != &ancestor) {
    if (node->widget->overlay_options())
      return false;
    if (!node->style_node.is_transparent &&
        out_of_flow(node_style(*node).get<styles::Position>()))
      node = detail::positioned_containing_node(*node);
    else
      node = node->parent;
  }
  return node == &ancestor;
}

bool hit_path(const Node &node, const Node &target, Point<float> &point,
              float scale) {
  if (node.parent && !node.widget->overlay_options() &&
      !hit_path(*node.parent, target, point, scale))
    return false;
  if (!detail::untransform_point(node, point, scale))
    return false;
  return &node == &target || !clips_target(node, target) ||
         node.widget->children_clip({node.global_pos, node.size})
             .contains(point);
}
} // namespace

void WidgetTree::update_paint_order_() {
  if (!paint_order_dirty_)
    return;
  if (!paint_structure_dirty_) {
    const bool unchanged = std::all_of(
        paint_order_.begin(), paint_order_.end(), [](const PaintEntry &entry) {
          return entry.foreground ||
                 (entry.level == stack_level(*entry.node) &&
                  entry.context == stacking_context(*entry.node) &&
                  entry.participant == participates(*entry.node));
        });
    if (unchanged) {
      paint_order_dirty_ = false;
      return;
    }
  }
  paint_order_.clear();
  if (paint_structure_dirty_)
    rebuild_overlays_();
  // A normal ancestor does not imprison positioned descendants. Only an
  // actual stacking context is atomic in its parent's ordered participant list.
  const auto normal = [&](auto &&self, Node *node) -> void {
    paint_order_.push_back({node, 0, false});
    children(node, [&](Node *child) {
      if (!child->widget->overlay_options() && !participates(*child))
        self(self, child);
    });
    paint_order_.push_back({node, 0, true});
  };
  const auto context = [&](auto &&self, Node *root) -> void {
    std::vector<Node *> participants;
    const auto collect = [&](auto &&walk, Node *parent) -> void {
      children(parent, [&](Node *child) {
        if (child->widget->overlay_options())
          return;
        if (participates(*child))
          participants.push_back(child);
        if (!stacking_context(*child))
          walk(walk, child);
      });
    };
    collect(collect, root);
    std::stable_sort(
        participants.begin(), participants.end(),
        [](Node *a, Node *b) { return stack_level(*a) < stack_level(*b); });
    paint_order_.push_back({root, 0, false});
    const auto paint_participant = [&](Node *node) {
      if (stacking_context(*node))
        self(self, node);
      else
        normal(normal, node);
    };
    auto it = participants.begin();
    for (; it != participants.end() && stack_level(**it) < 0; ++it)
      paint_participant(*it);
    children(root, [&](Node *child) {
      if (!child->widget->overlay_options() && !participates(*child))
        normal(normal, child);
    });
    for (; it != participants.end(); ++it)
      paint_participant(*it);
    paint_order_.push_back({root, 0, true});
  };
  if (root_ && !root_->widget->overlay_options())
    context(context, root_.get());
  for (std::size_t i = 0; i < overlays_.size(); ++i) {
    overlays_[i].paint_begin = paint_order_.size();
    context(context, overlays_[i].node);
    overlays_[i].paint_end = paint_order_.size();
  }
  for (auto &entry : paint_order_) {
    if (entry.foreground)
      continue;
    entry.level = stack_level(*entry.node);
    entry.context = stacking_context(*entry.node);
    entry.participant = participates(*entry.node);
  }
  paint_order_dirty_ = false;
  paint_structure_dirty_ = false;
}

void WidgetTree::render_ordered_(Painter &painter) {
  update_paint_order_();
  const auto normal_end = overlays_.empty() ? paint_order_.size() : overlays_[0].paint_begin;
  draw_range_(painter, 0, normal_end);
  for (const auto index : overlay_stack_) {
    const auto &entry = overlays_[index];
    const auto &options = *entry.node->widget->overlay_options();
    if (options.modal) {
      painter.save();
      painter.fill_rect({{}, viewport_}, Paint(options.backdrop));
      painter.restore();
    }
    draw_range_(painter, entry.paint_begin, entry.paint_end);
  }
}

void WidgetTree::draw_range_(Painter &painter, std::size_t begin, std::size_t end) {
  paint_path_.clear();
  for (std::size_t index = begin; index < end; ++index) {
    const auto &entry = paint_order_[index];
    next_paint_path_.clear();
    for (const Node *node = entry.node; node; node = node->parent) {
      next_paint_path_.push_back(
          {node, node->parent && !node->widget->overlay_options() &&
                     clips_target(*node->parent, *entry.node)});
      if (node->widget->overlay_options())
        break;
    }
    std::reverse(next_paint_path_.begin(), next_paint_path_.end());
    std::size_t shared = 0;
    while (shared < paint_path_.size() && shared < next_paint_path_.size() &&
           paint_path_[shared].node == next_paint_path_[shared].node &&
           paint_path_[shared].parent_clip ==
               next_paint_path_[shared].parent_clip)
      ++shared;
    for (std::size_t i = paint_path_.size(); i > shared; --i)
      painter.restore();
    for (std::size_t i = shared; i < next_paint_path_.size(); ++i) {
      painter.save();
      const auto scope = next_paint_path_[i];
      const Node &node = *scope.node;
      if (scope.parent_clip) {
        const Node &parent = *node.parent;
        painter.clip_rect(
            parent.widget->children_clip({parent.global_pos, parent.size}));
      }
      painter.transform(node_transform(node, device_scale_));
      painter.opacity(node_style(node).get<styles::Opacity>());
    }
    paint_path_.swap(next_paint_path_);
    painter.save();
    draw_node_(entry.node, painter, entry.foreground);
    painter.restore();
  }
  for (std::size_t i = paint_path_.size(); i > 0; --i)
    painter.restore();
  paint_path_.clear();
}

Node *WidgetTree::hit_test_(Node *, Point<float> point) {
  update_paint_order_();
  if (!Rect<float>{{}, viewport_}.contains(point)) return nullptr;
  for (auto it = overlay_stack_.rbegin(); it != overlay_stack_.rend(); ++it) {
    const auto &entry = overlays_[*it];
    const auto &options = *entry.node->widget->overlay_options();
    if (options.interactive && input_allowed_(entry.node)) {
      if (auto *hit = hit_range_(point, entry.paint_begin, entry.paint_end)) return hit;
    }
    if (options.modal) return nullptr; // backdrop is an input barrier
  }
  return hit_range_(point, 0, overlays_.empty() ? paint_order_.size() : overlays_[0].paint_begin);
}

Node *WidgetTree::hit_range_(Point<float> point, std::size_t begin, std::size_t end) {
  for (std::size_t index = end; index > begin;) {
    const auto &entry = paint_order_[--index];
    Node *node = entry.node;
    const auto &style = node_style(*node);
    if (node->style_node.is_transparent ||
        !input_allowed_(node) ||
        style.get<styles::Visibility>() != Visibility::Visible ||
        style.get<styles::PointerEvents>() == PointerEvents::None)
      continue;
    Point<float> local = point;
    if (!hit_path(*node, *node, local, device_scale_))
      continue;
    const Rect<float> bounds{node->global_pos, node->size};
    if (entry.foreground ? node->widget->foreground_hit_test(local, bounds)
                       : bounds.contains(local))
      return node;
  }
  return nullptr;
}
} // namespace voidui
