#include "core/widget_geometry.h"
#include "voidui/core/overlay.h"

namespace voidui {

bool WidgetTree::belongs_to_overlay_(const Node *node,
                                     const Node *owner) const {
  if (!node || !owner)
    return false;
  for (; node;) {
    if (node == owner)
      return true;
    if (node->widget->overlay_options()) {
      const auto entry =
          std::find_if(overlays_.begin(), overlays_.end(),
                       [node](const auto &e) { return e.node == node; });
      if (entry != overlays_.end() && entry->owner) {
        node = entry->owner;
        continue;
      }
    }
    node = node->parent;
  }
  return false;
}

bool WidgetTree::input_allowed_(const Node *node) const {
  for (const Node *parent = node ? node->parent : nullptr; parent; parent = parent->parent)
    if (!parent->widget->children_visible())
      return false;
  if (overlays_.empty())
    return node != nullptr;
  if (!node || (top_modal_ && !belongs_to_overlay_(node, top_modal_)))
    return false;
  for (const Node *ancestor = node; ancestor; ancestor = ancestor->parent) {
    if (detail::node_style(*ancestor).get<styles::Visibility>() !=
        Visibility::Visible)
      return false;
    if (const auto *options = ancestor->widget->overlay_options()) {
      const auto entry = std::find_if(
          overlays_.begin(), overlays_.end(),
          [ancestor](const auto &e) { return e.node == ancestor; });
      if (entry == overlays_.end() || !entry->visible || !options->interactive)
        return false;
    }
  }
  return true;
}

void WidgetTree::rebuild_overlay_stack_() {
  if (!overlay_stack_dirty_)
    return;
  overlay_stack_.clear();
  for (std::size_t i = 0; i < overlays_.size(); ++i)
    if (overlays_[i].visible)
      overlay_stack_.push_back(i);
  std::sort(overlay_stack_.begin(), overlay_stack_.end(), [&](auto a, auto b) {
    return overlays_[a].sequence < overlays_[b].sequence;
  });
  Node *modal = nullptr;
  for (auto i : overlay_stack_)
    if (overlays_[i].node->widget->overlay_options()->modal)
      modal = overlays_[i].node;
  if (modal != top_modal_)
    modal_focus_dirty_ = true;
  top_modal_ = modal;
  overlay_stack_dirty_ = false;
}

void WidgetTree::close_overlay_(Node *node, OverlayDismissReason reason,
                                bool notify) {
  const auto target =
      std::find_if(overlays_.begin(), overlays_.end(),
                   [node](const auto &entry) { return entry.node == node; });
  if (target == overlays_.end())
    return;
  if (!target->visible && (!target->requested || target->suppressed))
    return;
  if (top_modal_ && belongs_to_overlay_(top_modal_, node)) {
    restore_focus_ = target->return_focus;
    modal_focus_dirty_ = true;
  }
  // Ownership is retained until the next activation so sibling dialogs also
  // close with their opener. User callbacks run after the whole group closes.
  for (auto it = overlays_.rbegin(); it != overlays_.rend(); ++it) {
    if (!belongs_to_overlay_(it->node, node))
      continue;
    const bool was_open = it->visible || (it->requested && !it->suppressed);
    it->suppressed = true;
    it->visible = false;
    if (!was_open)
      continue;
    deactivate_overlay_(it->node);
    if (it->node != node || notify)
      overlay_notifications_.push_back(
          {it->node,
           it->node == node ? reason : OverlayDismissReason::OwnerClosed});
  }
  overlay_stack_dirty_ = true;
  rebuild_overlay_stack_();
  request_paint();
}

void WidgetTree::forget_overlays_(Node *node) {
  // Called before node memory is released, including during reconciliation.
  for (const auto &entry : overlays_)
    if (is_inside_(entry.node, node))
      close_overlay_(entry.node, OverlayDismissReason::OwnerClosed, false);
  for (auto &entry : overlays_) {
    if (entry.return_focus && is_inside_(entry.return_focus, node))
      entry.return_focus = nullptr;
    if (entry.owner && is_inside_(entry.owner, node))
      entry.owner = entry.declared_owner;
    if (entry.owner && is_inside_(entry.owner, node))
      entry.owner = nullptr;
  }
  if (restore_focus_ && is_inside_(restore_focus_, node))
    restore_focus_ = nullptr;
  std::erase_if(overlay_notifications_, [&](const auto &entry) {
    return is_inside_(entry.node, node);
  });
  std::erase_if(overlays_, [&](const auto &entry) {
    return is_inside_(entry.node, node);
  });
  overlay_stack_dirty_ = true;
  rebuild_overlay_stack_();
}

void WidgetTree::flush_overlay_notifications_() {
  // Pop before calling user code: a callback may rebuild the tree. Removal
  // scrubs pending pointers, and no references into the vector cross callbacks.
  while (!overlay_notifications_.empty()) {
    const auto notification = overlay_notifications_.back();
    overlay_notifications_.pop_back();
    OverlayDismissedEvent event(notification.reason);
    notification.node->widget->on_event(event);
    invalidate_(event.invalidation());
    if (event.style_requested()) restyle();
  }
}

void WidgetTree::move_focus_(bool backward) {
  Node *first = nullptr, *last = nullptr, *previous = nullptr, *next = nullptr;
  bool passed = false;
  const auto walk = [&](auto &&self, Node *node) -> void {
    if (node->widget->focusable() && input_allowed_(node)) {
      if (!first)
        first = node;
      last = node;
      if (node == focused_node_)
        passed = true;
      else if (!passed)
        previous = node;
      else if (!next)
        next = node;
    }
    for (auto &child : node->children)
      self(self, child.get());
    for (auto &child : node->internal_children)
      self(self, child.get());
  };
  if (root_)
    walk(walk, root_.get());
  set_focus_(backward ? (previous ? previous : last) : (next ? next : first));
}

void WidgetTree::sync_modal_focus_() {
  if (!modal_focus_dirty_)
    return;
  modal_focus_dirty_ = false;
  if (hovered_node_ && !input_allowed_(hovered_node_))
    set_hovered_(nullptr);
  if ((selection_node_ && !input_allowed_(selection_node_)) ||
      (selection_focus_node_ && !input_allowed_(selection_focus_node_)))
    clear_selection_();
  if (active_node_ && !input_allowed_(active_node_)) {
    active_node_->status.set_active(false);
    active_node_->style_node.status = active_node_->status.bits() | active_node_->widget->style_status();
    invalidate_(resolver_.resolve_subtree(active_node_->style_node, true));
    active_node_ = nullptr;
  }
  for (auto &[button, target] : mouse_down_widgets_)
    if (target && !input_allowed_(target))
      target = nullptr;
  Node *restore = std::exchange(restore_focus_, nullptr);
  if (restore && restore->widget->focusable() && input_allowed_(restore)) {
    set_focus_(restore, false);
  } else if (!focused_node_ || !input_allowed_(focused_node_)) {
    set_focus_(nullptr);
    if (top_modal_)
      move_focus_(false);
  }
}

bool WidgetTree::dismiss_overlays_(Event &event) {
  const auto type = event.type();
  if (type == EventType::KeyPressed) {
    if (static_cast<KeyPressedEvent &>(event).keycode() != Keycode::Escape)
      return false;
    for (auto it = overlay_stack_.rbegin(); it != overlay_stack_.rend(); ++it) {
      const auto &entry = overlays_[*it];
      const auto &options = *entry.node->widget->overlay_options();
      if (options.dismiss_on_escape) {
        close_overlay_(entry.node, OverlayDismissReason::Escape, true);
        sync_modal_focus_();
        return true;
      }
      if (options.modal)
        return true;
    }
    // Escape also cancels a delayed hint when no visible dismissible layer
    // handled it. Pending hints have no stack position yet.
    for (auto it = overlays_.rbegin(); it != overlays_.rend(); ++it) {
      if (it->requested && !it->visible && !it->suppressed &&
          it->node->widget->overlay_options()->dismiss_on_escape) {
        close_overlay_(it->node, OverlayDismissReason::Escape, true);
        return true;
      }
    }
    return false;
  }
  if (type != EventType::MousePressed && type != EventType::MouseScrolled &&
      type != EventType::WindowFocusLost)
    return false;
  const Node *hit =
      type == EventType::WindowFocusLost
          ? nullptr
          : hit_test_(root_.get(), static_cast<MouseEvent &>(event).get_pos());
  // A pointer press dismisses at most one interactive entry and is consumed,
  // so the same press/release cannot activate a newly exposed background
  // button. Hints are noninteractive and may all dismiss without eating the
  // press.
  for (std::size_t position = overlay_stack_.size(); position > 0;) {
    const auto index = overlay_stack_[--position];
    const auto &entry = overlays_[index];
    Node *node = entry.node;
    const auto options = *node->widget->overlay_options();
    const bool outside = !belongs_to_overlay_(hit, node);
    const bool dismiss =
        (type == EventType::WindowFocusLost &&
         (options.trigger == OverlayTrigger::HoverOrFocus || options.dismiss_on_focus_loss)) ||
        (type == EventType::MouseScrolled && options.dismiss_on_scroll) ||
        (type == EventType::MousePressed &&
         (options.dismiss_on_press ||
          (outside && options.dismiss_on_outside_press)));
    if (dismiss) {
      close_overlay_(
          node,
          type == EventType::MouseScrolled ? OverlayDismissReason::Scroll
          : type == EventType::WindowFocusLost
              ? OverlayDismissReason::WindowFocusLost
          : options.dismiss_on_press ? OverlayDismissReason::Press
                                     : OverlayDismissReason::OutsidePress,
          true);
      if (options.interactive && type == EventType::MousePressed) {
        // Cancel any older capture before consuming this new press.
        mouse_down_widgets_.clear();
        sync_modal_focus_();
        return true;
      }
      position = std::min(position, overlay_stack_.size());
    } else if (options.modal && type != EventType::WindowFocusLost) {
      break;
    }
  }
  // Cancel pending hints as well; they are deliberately absent from the stack.
  for (auto &entry : overlays_) {
    if (entry.visible || !entry.requested || entry.suppressed)
      continue;
    const auto &options = *entry.node->widget->overlay_options();
    if ((type == EventType::MousePressed && options.dismiss_on_press) ||
        (type == EventType::MouseScrolled && options.dismiss_on_scroll) ||
        (type == EventType::WindowFocusLost &&
         (options.trigger == OverlayTrigger::HoverOrFocus || options.dismiss_on_focus_loss)))
      close_overlay_(entry.node,
                     type == EventType::MouseScrolled
                         ? OverlayDismissReason::Scroll
                     : type == EventType::WindowFocusLost
                         ? OverlayDismissReason::WindowFocusLost
                         : OverlayDismissReason::Press,
                     true);
  }
  return false;
}
} // namespace voidui
