#pragma once

#include <functional>
#include <memory>
#include <vector>

#include "voidui/core/style.h"
#include "voidui/core/widget_tree.h"

namespace voidui {
class LayoutContext {
public:
  using LayoutChildFn =
      std::function<Size<float>(Node *node, Constraints constraints)>;

  LayoutContext(WidgetStatus status, const ComputedStyle &style,
                Point<float> global_pos,
                std::vector<std::unique_ptr<Node>> &children,
                std::vector<std::unique_ptr<Node>> &internal_children,
                LayoutChildFn layout_fn, float device_scale = 1.0f,
                Invalidator invalidator = {})
      : status(status), style(style), global_pos_(global_pos),
        children_(children), internal_children_(internal_children),
        layout_fn_(layout_fn), device_scale_(device_scale),
        invalidator_(std::move(invalidator)) {}

  LayoutContext(const LayoutContext &other) = delete;
  LayoutContext &operator=(const LayoutContext &) = delete;
  LayoutContext(LayoutContext &&other) = delete;
  LayoutContext &operator=(LayoutContext &&) noexcept = default;

  WidgetStatus status;

  /// The node's resolved style. Padding, borders and anything else that moves
  /// a box comes from here, so layout and painting read one source.
  const ComputedStyle &style;

  Point<float> global_pos() const { return global_pos_; }

  /// Device pixels per logical unit for the surface being laid out.
  ///
  /// Almost nothing needs this: layout is in logical units by design. Text is
  /// the exception, because a line box that is a fractional number of device
  /// pixels tall makes stacked lines round unevenly against each other.
  float device_scale() const { return device_scale_; }

  /// A handle for work this pass starts and a later frame finishes.
  ///
  /// Measurement is where a widget first knows how much room it has, so it is
  /// where a load sized to that room has to begin -- and therefore where the
  /// means to ask for another frame has to be available. Copyable and safe to
  /// hand to a worker thread; see Invalidator.
  const Invalidator &invalidator() const { return invalidator_; }

  size_t child_count() const {
    return children_.size() + internal_children_.size();
  }

  const Size<Length> &child_layout_size(size_t index) const {
    const Node *node = layout_node_(index);
    return node->style_node.computed->layout_size();
  }

  /// Margin belongs to the rendered root of a transparent component, just as
  /// width and height do. Containers use this only when distributing flex
  /// space; ordinary measurement already returns the complete margin box.
  const Spacing<MarginValue> &child_margin(size_t index) const {
    return layout_node_(index)->style_node.computed->layout_margin();
  }

  Size<float> constrain_child(size_t index, Constraints constraints) {
    return layout_fn_(child_at_(index), constraints);
  }

  template <typename Fn> std::vector<Size<float>> constrain_children(Fn &&fn) {
    std::vector<Size<float>> children_sizes;

    for (size_t i = 0; i < children_.size(); i++) {
      Constraints constraints = fn(i);
      Size<float> size = constrain_child(i, constraints);
      children_sizes.push_back(size);
    }

    return children_sizes;
  }

  void place_child(size_t index, Point<float> local_pos) {
    place_child(index, local_pos, {});
  }

  /// Places a child after a container has distributed space to its auto
  /// margins. Fixed margins remain owned by the tree's normal box path.
  void place_child(size_t index, Point<float> local_pos,
                   Spacing<float> auto_margin) {
    Node *child = child_at_(index);
    const Spacing<float> margin =
        resolve_fixed_margin(child->style_node.computed->layout_margin());
    const Point<float> new_pos{
        local_pos.x + margin.left + auto_margin.left + global_pos_.x,
        local_pos.y + margin.top + auto_margin.top + global_pos_.y};
    const Point<float> delta{new_pos.x - child->global_pos.x,
                             new_pos.y - child->global_pos.y};
    translate_subtree_(*child, delta);
  }

private:
  const Node *layout_node_(size_t index) const {
    const Node *node = child_at_(index);
    // Function components are layout-transparent. Their rendered root owns the
    // box properties that the parent container must classify.
    while (node->style_node.is_transparent)
      node = node->children[0].get();
    return node;
  }

  Node *child_at_(size_t index) const {
    if (index < children_.size())
      return children_[index].get();
    return internal_children_[index - children_.size()].get();
  }

  static void translate_subtree_(Node &node, Point<float> delta) {
    node.global_pos.x += delta.x;
    node.global_pos.y += delta.y;
    for (auto &child : node.children)
      translate_subtree_(*child, delta);
    for (auto &child : node.internal_children)
      translate_subtree_(*child, delta);
  }

  std::vector<std::unique_ptr<Node>> &children_;
  std::vector<std::unique_ptr<Node>> &internal_children_;
  LayoutChildFn layout_fn_;
  Point<float> global_pos_;
  float device_scale_ = 1.0f;
  Invalidator invalidator_;
};

/// Everything a widget needs in order to paint itself.
///
/// Passed by const reference and holding a reference to the node's resolved
/// style, so a frame of drawing performs no allocation and no refcount
/// traffic: the style object is owned by the style tree for as long as the
/// node lives, and the widget only reads it.
///
/// The style deliberately does not live on the widget. A widget is cloned,
/// moved and passed around before it ever becomes a node, while a computed
/// style only means anything for a node in a tree; keeping it here is what
/// makes that impossible to get wrong.
struct DrawContext {
  Rect<float> bounds;
  WidgetStatus status;
  const ComputedStyle &style;
  WidgetTree &tree;
  std::uint32_t selection_begin = 0;
  std::uint32_t selection_end = 0;
  bool has_selection = false;

  /// Keeps a paint animation running without forcing layout. An animation that
  /// changes geometry must request layout instead.
  void request_frame() const { tree.request_paint(); }
  void request_layout() const { tree.request_layout(); }

  /// This frame's timestamp, on the monotonic clock that drives the animator.
  /// Time enters the tree at one point, so a widget cannot sample a clock of
  /// its own and disagree with the frame it is painting into.
  double now() const { return tree.frame_time(); }

  /// Asks for the next frame at a moment instead of immediately. A caret that
  /// toggles twice a second wants two frames a second, not sixty; requesting
  /// them this way lets the window sleep in between.
  void request_frame_at(double when_seconds) const {
    tree.request_paint_at(when_seconds);
  }
};
} // namespace voidui
