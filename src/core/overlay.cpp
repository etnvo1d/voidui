#include "voidui/core/overlay.h"
#include "core/widget_geometry.h"

#include <algorithm>
#include <cmath>

namespace voidui {
namespace {
const Node *anchor_of(const Node &node) {
  const Node *parent = node.parent;
  while (parent && parent->style_node.is_transparent)
    parent = parent->parent;
  return parent;
}

Rect<float> intersect(Rect<float> a, Rect<float> b) {
  const float x = std::max(a.origin.x, b.origin.x);
  const float y = std::max(a.origin.y, b.origin.y);
  return {x, y,
          std::max(0.0f, std::min(a.origin.x + a.size.width,
                                  b.origin.x + b.size.width) -
                             x),
          std::max(0.0f, std::min(a.origin.y + a.size.height,
                                  b.origin.y + b.size.height) -
                             y)};
}

Point<float> place(Rect<float> anchor, Size<float> size, OverlayPlacement side,
                   float gap) {
  const float x = anchor.origin.x, y = anchor.origin.y;
  const float w = anchor.size.width, h = anchor.size.height;
  switch (side) {
  case OverlayPlacement::Center:
    return {x + (w - size.width) * 0.5f, y + (h - size.height) * 0.5f};
  case OverlayPlacement::Top:
    return {x + (w - size.width) * 0.5f, y - gap - size.height};
  case OverlayPlacement::Bottom:
    return {x + (w - size.width) * 0.5f, y + h + gap};
  case OverlayPlacement::Left:
    return {x - gap - size.width, y + (h - size.height) * 0.5f};
  case OverlayPlacement::Right:
    return {x + w + gap, y + (h - size.height) * 0.5f};
  }
  return {};
}

OverlayPlacement opposite(OverlayPlacement side) {
  switch (side) {
  case OverlayPlacement::Center:
    return side;
  case OverlayPlacement::Top:
    return OverlayPlacement::Bottom;
  case OverlayPlacement::Bottom:
    return OverlayPlacement::Top;
  case OverlayPlacement::Left:
    return OverlayPlacement::Right;
  case OverlayPlacement::Right:
    return OverlayPlacement::Left;
  }
  return side;
}

void move_overlay(Node &node, Point<float> delta) {
  node.global_pos.x += delta.x;
  node.global_pos.y += delta.y;
  const auto move_child = [&](auto &child) {
    const Node *box = layout_box_of(child.get());
    if (box->widget->overlay_options() ||
        (detail::node_style(*box).get<styles::Position>() == Position::Fixed &&
         !detail::positioned_containing_node(*box)))
      return;
    move_overlay(*child, delta);
  };
  for (auto &child : node.children)
    move_child(child);
  for (auto &child : node.internal_children)
    move_child(child);
}
} // namespace

void WidgetTree::rebuild_overlays_() {
  std::vector<OverlayEntry> next;
  next.reserve(overlays_.size());
  // Preserve runtime state through keyed reorder without quadratic searches
  // or a persistent hash table. This runs only when the tree structure changes.
  std::sort(overlays_.begin(), overlays_.end(),
            [](const auto &a, const auto &b) {
              return std::less<Node *>{}(a.node, b.node);
            });
  const auto collect = [&](auto &&self, Node *node, Node *owner) -> void {
    if (node->widget->overlay_options()) {
      const auto old =
          std::lower_bound(overlays_.begin(), overlays_.end(), node,
                           [](const auto &entry, Node *key) {
                             return std::less<Node *>{}(entry.node, key);
                           });
      next.push_back(old == overlays_.end() || old->node != node
                         ? OverlayEntry{node}
                         : *old);
      next.back().declared_owner = owner;
      if (!next.back().visible)
        next.back().owner = owner;
      owner = node;
    }
    for (auto &child : node->children)
      self(self, child.get(), owner);
    for (auto &child : node->internal_children)
      self(self, child.get(), owner);
  };
  if (root_)
    collect(collect, root_.get(), nullptr);
  overlays_.swap(next);
  overlay_stack_dirty_ = true;
}

void WidgetTree::layout_overlays_() {
  for (auto &entry : overlays_)
    entry.layout_dirty = true;
  update_overlays_();
}

void WidgetTree::deactivate_overlay_(Node *node) {
  if (hovered_node_ && is_inside_(hovered_node_, node))
    set_hovered_(nullptr);
  if (focused_node_ && is_inside_(focused_node_, node))
    set_focus_(nullptr);
  if (selection_node_ && is_inside_(selection_node_, node))
    clear_selection_();
  if (active_node_ && is_inside_(active_node_, node)) {
    active_node_->status.set_active(false);
    active_node_->style_node.status = active_node_->status.bits() | active_node_->widget->style_status();
    invalidate_(resolver_.resolve_subtree(active_node_->style_node, true));
    active_node_ = nullptr;
  }
  for (auto &[button, target] : mouse_down_widgets_)
    if (target && is_inside_(target, node))
      target = nullptr;
}

void WidgetTree::update_overlays_() {
  rebuild_overlay_stack_();
  for (auto &entry : overlays_) {
    Node &node = *entry.node;
    const auto &options = *node.widget->overlay_options();
    const Node *anchor =
        options.modal || options.placement == OverlayPlacement::Center
            ? nullptr
            : anchor_of(node);
    const bool focused = focus_triggers_hints_ && anchor && focused_node_ &&
                         is_inside_(focused_node_, const_cast<Node *>(anchor));
    const bool requested =
        options.open && (options.trigger == OverlayTrigger::Manual ||
                         (anchor && (anchor->status.is_hovered() || focused)));
    if (!requested) {
      entry.suppressed = false;
      entry.show_at = std::numeric_limits<double>::infinity();
    } else if (!entry.requested) {
      entry.show_at =
          frame_time_ +
          (focused ? 0.0
                   : std::max(
                         0.0,
                         std::chrono::duration<double>(options.delay).count()));
    }
    entry.requested = requested;
    // A new focus activation can reveal a tooltip even when the same mouse
    // press cancelled a pending hover. Escape remains suppressed until exit.
    if (focused && !entry.focused &&
        options.trigger == OverlayTrigger::HoverOrFocus)
      entry.suppressed = false;
    entry.focused = focused;
    if (focused)
      entry.show_at = std::min(entry.show_at, frame_time_);
    if (!requested) {
      if (entry.visible)
        close_overlay_(&node, OverlayDismissReason::OwnerClosed, false);
      entry.suppressed = false;
      entry.owner = entry.declared_owner;
      continue;
    }
    if (entry.suppressed)
      continue;
    const Rect<float> viewport{{}, viewport_};
    Rect<float> bounds =
        anchor ? detail::window_transform(*anchor, device_scale_)
                     .map_bounds({anchor->global_pos, anchor->size})
               : viewport;
    Rect<float> visible_bounds = intersect(bounds, viewport);
    bool eligible = detail::node_style(node).get<styles::Visibility>() ==
                    Visibility::Visible;
    if (entry.owner) {
      const auto owner =
          std::find_if(overlays_.begin(), overlays_.end(),
                       [&](const auto &e) { return e.node == entry.owner; });
      if (owner == overlays_.end() || !owner->visible) {
        if (entry.visible)
          close_overlay_(&node, OverlayDismissReason::OwnerClosed, true);
        eligible = false;
      }
    }
    // A clipped-away anchor must not leave a floating bubble behind. Visual
    // bounds also account for transforms and nested portal boundaries.
    for (const Node *parent = anchor ? anchor : node.parent; parent;
         parent = parent->parent) {
      eligible &= detail::node_style(*parent).get<styles::Visibility>() ==
                  Visibility::Visible;
      eligible &= parent->widget->children_visible();
      if (anchor && parent != anchor && parent->widget->clips_children())
        visible_bounds = intersect(
            visible_bounds, detail::window_transform(*parent, device_scale_)
                                .map_bounds(parent->widget->children_clip(
                                    {parent->global_pos, parent->size})));
      if (parent->widget->overlay_options()) {
        const auto enclosing =
            std::find_if(overlays_.begin(), overlays_.end(),
                         [parent](const auto &e) { return e.node == parent; });
        eligible &= enclosing != overlays_.end() && enclosing->visible;
        break;
      }
    }
    eligible &= visible_bounds.size.width > 0 && visible_bounds.size.height > 0;
    // Inactive modal scopes cannot start delayed hints or bring background
    // popups above the current dialog. Explicit modals may open a new scope.
    if (!options.modal && top_modal_ &&
        !belongs_to_overlay_(&node, top_modal_) &&
        !belongs_to_overlay_(top_modal_, &node)) {
      if (entry.visible || (entry.requested && !entry.suppressed))
        close_overlay_(&node, OverlayDismissReason::ModalOpened, true);
      eligible = false;
    }
    const bool visible = requested && eligible && !entry.suppressed &&
                         frame_time_ >= entry.show_at;
    if (visible != entry.visible) {
      if (!visible) {
        // A declaration closes silently; its descendants receive OwnerClosed.
        close_overlay_(&node, OverlayDismissReason::OwnerClosed, false);
        if (!requested)
          entry.suppressed = false;
      } else {
        entry.visible = true;
        entry.sequence = ++overlay_sequence_;
        overlay_stack_dirty_ = true;
        if (options.modal) {
          if (!entry.declared_owner)
            entry.owner = top_modal_;
          entry.return_focus = focused_node_;
          top_modal_ = &node;
          modal_focus_dirty_ = true;
          for (auto &other : overlays_) {
            if (&other != &entry && other.requested &&
                !other.node->widget->overlay_options()->modal &&
                !belongs_to_overlay_(other.node, &node) &&
                !belongs_to_overlay_(&node, other.node))
              close_overlay_(other.node, OverlayDismissReason::ModalOpened,
                             true);
          }
        }
      }
      request_paint();
    }
    if (requested && eligible && !entry.suppressed && !visible)
      request_paint_at(entry.show_at);
    if (!visible) {
      if (!requested)
        entry.owner = entry.declared_owner;
      continue;
    }

    const float px =
        std::clamp(options.viewport_padding, 0.0f, viewport_.width * 0.5f);
    const float py =
        std::clamp(options.viewport_padding, 0.0f, viewport_.height * 0.5f);
    const Rect<float> available{px, py, viewport_.width - 2 * px,
                                viewport_.height - 2 * py};
    {
      const float max_width = std::min(std::max(options.max_width, 0.0f),
                                        available.size.width);
      float max_height = available.size.height;
      if (options.constrain_to_anchor_side &&
          (options.placement == OverlayPlacement::Bottom ||
           options.placement == OverlayPlacement::Top)) {
        const float above = bounds.origin.y - py - options.gap;
        const float below = viewport_.height - py - bounds.origin.y -
                            bounds.size.height - options.gap;
        max_height = std::clamp(std::max(above, below), 0.0f, max_height);
      }
      const float width = options.match_anchor_width
                              ? std::clamp(bounds.size.width, 0.0f, max_width)
                              : max_width;
      const float min_width = options.match_anchor_width ? width : 0.0f;
      if (entry.measure_limit.width != width || entry.measure_limit.height != max_height ||
          entry.measure_min_width != min_width) {
        entry.layout_dirty = true;
        entry.measure_limit = {width, max_height};
        entry.measure_min_width = min_width;
      }
      if (entry.layout_dirty)
        layout_node_(&node, {min_width, width, 0.0f, max_height});
    }
    const auto preferred =
        place(bounds, node.size, options.placement, options.gap);
    const auto flipped =
        place(bounds, node.size, opposite(options.placement), options.gap);
    const auto overflow = [&](Point<float> point) {
      return std::max(available.origin.x - point.x, 0.0f) +
             std::max(available.origin.y - point.y, 0.0f) +
             std::max(point.x + node.size.width - available.origin.x -
                          available.size.width,
                      0.0f) +
             std::max(point.y + node.size.height - available.origin.y -
                          available.size.height,
                      0.0f);
    };
    auto target = overflow(flipped) < overflow(preferred) ? flipped : preferred;
    target.x = std::clamp(target.x, px,
                          std::max(px, viewport_.width - px - node.size.width));
    target.y = std::clamp(
        target.y, py, std::max(py, viewport_.height - py - node.size.height));
    if (target.x != node.global_pos.x || target.y != node.global_pos.y)
      move_overlay(
          node, {target.x - node.global_pos.x, target.y - node.global_pos.y});
    if (entry.layout_dirty) {
      position_subtree_(&node, true);
      entry.layout_dirty = false;
    }
  }
  rebuild_overlay_stack_();
  sync_modal_focus_();
}
} // namespace voidui
