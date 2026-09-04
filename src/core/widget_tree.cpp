#include "voidui/core/widget_tree.h"

#include "voidui/core/component.h"
#include "voidui/core/context.h"
#include "voidui/core/pixel_snap.h"

#include <SDL3/SDL_clipboard.h>

#include <algorithm>
#include <functional>
#include <string>
#include <typeindex>
#include <unordered_map>
#include <unordered_set>
#include <vector>

namespace voidui {
namespace {

/// The node's resolved style, or an empty one for a tree that has not been
/// resolved yet. Reading an unset property from the empty style still returns
/// the property's declared default, so a widget never sees a null.
const ComputedStyle &style_of(const Node &node) {
  static const ComputedStyle empty;
  return node.style_node.computed ? *node.style_node.computed : empty;
}

/// The padding box: the border box pulled in by the border it draws inside.
/// Degenerate boxes collapse to zero rather than turning inside out.
Rect<float> deflate_rect(const Rect<float> &rect, float amount) {
  if (amount <= 0.0f)
    return rect;
  const float width = std::max(rect.size.width - amount * 2.0f, 0.0f);
  const float height = std::max(rect.size.height - amount * 2.0f, 0.0f);
  return Rect<float>(rect.origin.x + amount, rect.origin.y + amount, width,
                     height);
}

/// The matching inner radius. CSS shrinks each corner by the border width and
/// lets it square off once the border is thicker than the radius.
Radius deflate_radius(const Radius &radius, float amount) {
  if (amount <= 0.0f)
    return radius;
  const auto shrink = [amount](float value) {
    return std::max(value - amount, 0.0f);
  };
  return Radius(shrink(radius.left_top), shrink(radius.right_top),
                shrink(radius.right_bottom), shrink(radius.left_bottom));
}

const Node *layout_box_of(const Node *node) {
  while (node && node->style_node.is_transparent && !node->children.empty())
    node = node->children[0].get();
  return node;
}

void translate_subtree(Node &node, Point<float> delta) {
  node.global_pos.x += delta.x;
  node.global_pos.y += delta.y;
  for (auto &child : node.children)
    translate_subtree(*child, delta);
  for (auto &child : node.internal_children)
    translate_subtree(*child, delta);
}

bool untransform_point(const Node &node, Point<float> &point,
                       float device_scale) {
  const VisualTransform &visual = style_of(node).get<styles::Transform>();
  // The exact matrix rendering used, snapping included, or a press would land
  // on a different pixel than the one the user aimed at.
  const Transform visual_transform =
      snap_translation_to_pixel(visual.matrix(), device_scale);
  if (visual_transform.is_identity())
    return true;

  const Rect<float> bounds{node.global_pos, node.size};
  const Point<float> center{bounds.origin.x + bounds.size.width * 0.5f,
                            bounds.origin.y + bounds.size.height * 0.5f};
  // A translation already commutes with the centring, and routing it through
  // the pair anyway computes `(c + t) - c` -- which for a one-pixel shift next
  // to a three-figure coordinate cancels away most of the bits that mattered.
  const Transform around_center =
      visual_transform.is_translation()
          ? visual_transform
          : Transform::translate(center.x, center.y)
                .concat(visual_transform)
                .concat(Transform::translate(-center.x, -center.y));
  Transform inverse;
  if (!around_center.inverse(inverse))
    return false;
  point = inverse.apply(point);
  return true;
}

bool point_in_node_space(const Node &node, Point<float> &point,
                         float device_scale) {
  if (node.parent && !point_in_node_space(*node.parent, point, device_scale))
    return false;
  return untransform_point(node, point, device_scale);
}

} // namespace

WidgetTree::WidgetTree()
    : token_(std::make_shared<detail::TreeToken>(detail::TreeToken{this})) {}

WidgetTree::WidgetTree(std::unique_ptr<Widget> root) : WidgetTree() {
  build(std::move(root));
}

WidgetTree::~WidgetTree() {
  // Anything still holding an Invalidator now finds a null tree behind it and
  // does nothing, rather than a dangling one.
  token_->tree = nullptr;
}

Invalidator WidgetTree::invalidator() {
  return Invalidator(token_, async::current_ui_dispatcher());
}

bool Invalidator::post_(Invalidation what) const {
  if (!token_)
    return false;

  // The check lands inside the posted task on purpose. Testing liveness here
  // would prove only that the tree was alive on some worker thread a moment
  // ago; testing it there proves it on the one thread that destroys trees.
  return dispatcher_.post([token = token_, what] {
    if (!token->tree)
      return;
    if (what == Invalidation::Layout)
      token->tree->request_layout();
    else
      token->tree->request_paint();
  });
}

bool Invalidator::request_layout() const { return post_(Invalidation::Layout); }

bool Invalidator::request_paint() const { return post_(Invalidation::Paint); }

void WidgetTree::refresh_style_node_(Node *node) {
  // The style projection of this widget. Interning the names once here is what
  // lets matching compare integers instead of strings.
  StyleNode &style_node = node->style_node;
  style_node.type = std::type_index(typeid(*node->widget));
  style_node.id = AtomTable::instance().intern(node->widget->style_id());
  style_node.classes.clear();
  for (const std::string &name : node->widget->style_classes())
    style_node.classes.push_back(AtomTable::instance().intern(name));
  std::sort(style_node.classes.begin(), style_node.classes.end());
  style_node.classes.erase(
      std::unique(style_node.classes.begin(), style_node.classes.end()),
      style_node.classes.end());
  resolver_.add_default_stylesheet(node->widget->default_stylesheet());
  style_node.inline_declaration = node->widget->inline_style();
  style_node.status = node->status.bits();
  style_node.is_transparent =
      dynamic_cast<ComponentBase *>(node->widget.get()) != nullptr;
}

std::unique_ptr<Node> WidgetTree::build_node_(std::unique_ptr<Widget> widget) {
  std::unique_ptr<Node> node = std::make_unique<Node>();
  node->widget = std::move(widget);
  refresh_style_node_(node.get());

  if (auto *component = dynamic_cast<ComponentBase *>(node->widget.get())) {
    node->component_runtime = std::make_unique<detail::ComponentRuntime>();
    node->component_runtime->tree_ = this;
    node->component_runtime->node_ = node.get();
    std::unique_ptr<Node> child =
        build_node_(component->render(*node->component_runtime));
    child->parent = node.get();
    node->children.push_back(std::move(child));
    return node;
  }

  std::vector<std::unique_ptr<Widget>> children, internal_children;
  std::vector<std::string> internal_parts;
  Registrar registrar(children, internal_children, internal_parts);
  node->widget->register_children(registrar);

  std::vector<std::unique_ptr<Node>> children_nodes;
  std::vector<std::unique_ptr<Node>> internal_children_nodes;

  for (auto &child : children) {
    if (!child)
      continue;
    std::unique_ptr<Node> child_node = build_node_(std::move(child));
    child_node->parent = node.get();
    children_nodes.push_back(std::move(child_node));
  }

  for (std::size_t i = 0; i < internal_children.size(); ++i) {
    if (!internal_children[i])
      continue;
    std::unique_ptr<Node> child_node =
        build_node_(std::move(internal_children[i]));
    child_node->parent = node.get();
    child_node->style_node.is_internal = true;
    if (i < internal_parts.size() && !internal_parts[i].empty()) {
      child_node->part = internal_parts[i];
      child_node->style_node.part =
          AtomTable::instance().intern(child_node->part);
    }
    internal_children_nodes.push_back(std::move(child_node));
  }

  node->children = std::move(children_nodes);
  node->internal_children = std::move(internal_children_nodes);

  return node;
}

void WidgetTree::discard_node_(Node *node) {
  resolver_.forget_animations(node->style_node);
  if (selection_node_ && is_inside_(selection_node_, node)) {
    selection_node_ = nullptr;
    selection_anchor_ = selection_focus_ = 0;
    selection_dragging_ = false;
  }
  if (hovered_node_ && is_inside_(hovered_node_, node))
    hovered_node_ = nullptr;
  if (active_node_ && is_inside_(active_node_, node))
    active_node_ = nullptr;
  if (focused_node_ && is_inside_(focused_node_, node)) {
    focused_node_->widget->text_input_stopped();
    focused_node_ = nullptr;
  }
  for (auto &[button, target] : mouse_down_widgets_)
    if (target && is_inside_(target, node))
      target = nullptr;
}

std::unique_ptr<Node>
WidgetTree::reconcile_node_(std::unique_ptr<Node> node,
                            std::unique_ptr<Widget> declaration) {
  const WidgetKey *old_key = node->widget->reconciliation_key();
  const WidgetKey *new_key = declaration->reconciliation_key();
  const bool same_key =
      (!old_key && !new_key) || (old_key && new_key && *old_key == *new_key);
  const bool same_type = std::type_index(typeid(*node->widget)) ==
                         std::type_index(typeid(*declaration));

  if (!same_key || !same_type) {
    Node *parent = node->parent;
    discard_node_(node.get());
    node = build_node_(std::move(declaration));
    node->parent = parent;
    return node;
  }

  declaration->inherit_runtime(*node->widget);
  node->widget = std::move(declaration);
  refresh_style_node_(node.get());

  if (dynamic_cast<ComponentBase *>(node->widget.get())) {
    render_component_(node.get());
    return node;
  }

  std::vector<std::unique_ptr<Widget>> children, internal_children;
  std::vector<std::string> internal_parts;
  Registrar registrar(children, internal_children, internal_parts);
  node->widget->register_children(registrar);

  reconcile_children_(node->children, std::move(children), node.get(), false);
  reconcile_children_(node->internal_children, std::move(internal_children),
                      node.get(), true, &internal_parts);
  return node;
}

void WidgetTree::reconcile_children_(
    std::vector<std::unique_ptr<Node>> &nodes,
    std::vector<std::unique_ptr<Widget>> declarations, Node *parent,
    bool internal, const std::vector<std::string> *parts) {
  auto attach = [&](std::unique_ptr<Node> child,
                    std::size_t declaration_index) {
    child->parent = parent;
    child->style_node.is_internal = internal;
    child->part =
        internal && parts ? (*parts)[declaration_index] : std::string{};
    child->style_node.part = AtomTable::instance().intern(child->part);
    return child;
  };

  bool has_keys = false;
  for (const auto &node : nodes)
    has_keys |= node->widget->reconciliation_key() != nullptr;
  for (const auto &declaration : declarations)
    has_keys |= declaration->reconciliation_key() != nullptr;

  std::vector<std::unique_ptr<Node>> result;
  result.reserve(declarations.size());

  if (!has_keys) {
    const std::size_t common = std::min(nodes.size(), declarations.size());
    for (std::size_t i = 0; i < common; ++i)
      result.push_back(attach(
          reconcile_node_(std::move(nodes[i]), std::move(declarations[i])), i));
    for (std::size_t i = common; i < declarations.size(); ++i)
      result.push_back(attach(build_node_(std::move(declarations[i])), i));
    for (std::size_t i = common; i < nodes.size(); ++i)
      discard_node_(nodes[i].get());

    nodes = std::move(result);
    return;
  }

  struct KeyHash {
    std::size_t operator()(const WidgetKey *key) const { return key->hash(); }
  };
  struct KeyEqual {
    bool operator()(const WidgetKey *left, const WidgetKey *right) const {
      return *left == *right;
    }
  };
  std::unordered_map<const WidgetKey *, std::size_t, KeyHash, KeyEqual> keyed;
  keyed.reserve(nodes.size());
  for (std::size_t i = 0; i < nodes.size(); ++i)
    if (const WidgetKey *key = nodes[i]->widget->reconciliation_key())
      keyed.emplace(key, i);

  for (std::size_t i = 0; i < declarations.size(); ++i) {
    std::size_t old_index = nodes.size();
    if (const WidgetKey *key = declarations[i]->reconciliation_key()) {
      auto found = keyed.find(key);
      if (found != keyed.end()) {
        old_index = found->second;
        keyed.erase(found);
      }
    } else if (i < nodes.size() && nodes[i] &&
               !nodes[i]->widget->reconciliation_key()) {
      old_index = i;
    }

    std::unique_ptr<Node> child =
        old_index < nodes.size() ? reconcile_node_(std::move(nodes[old_index]),
                                                   std::move(declarations[i]))
                                 : build_node_(std::move(declarations[i]));
    result.push_back(attach(std::move(child), i));
  }

  for (auto &node : nodes)
    if (node)
      discard_node_(node.get());
  nodes = std::move(result);
}

void WidgetTree::render_component_(Node *node) {
  auto *component = static_cast<ComponentBase *>(node->widget.get());
  node->component_runtime->queued_ = false;

  std::vector<std::unique_ptr<Widget>> declaration;
  declaration.push_back(component->render(*node->component_runtime));
  reconcile_children_(node->children, std::move(declaration), node, false);
}

void WidgetTree::queue_component_(Node *node) {
  dirty_components_.push_back(node);
  request_layout();
}

void WidgetTree::flush_component_updates_() {
  if (dirty_components_.empty())
    return;

  std::vector<Node *> dirty = std::move(dirty_components_);
  dirty_components_.clear();
  const std::less<Node *> less;
  std::sort(dirty.begin(), dirty.end(), less);
  std::vector<Node *> roots;
  roots.reserve(dirty.size());

  for (Node *node : dirty) {
    node->component_runtime->queued_ = false;
    Node *ancestor = node->parent;
    while (ancestor &&
           !std::binary_search(dirty.begin(), dirty.end(), ancestor, less))
      ancestor = ancestor->parent;
    if (!ancestor)
      roots.push_back(node);
  }

  for (Node *node : roots) {
    render_component_(node);
    link_style_tree_(node);
    style_rebuild_blooms(node->style_node);
    invalidate_(resolver_.resolve_subtree(node->style_node, true));
  }
}

Size<float> WidgetTree::layout_node_(Node *node, Constraints constraints) {
  if (!node || !node->widget)
    return Size(0.0f, 0.0f);

  Widget *widget = node->widget.get();
  const Spacing<float> margin =
      resolve_fixed_margin(style_of(*node).layout_margin());
  LayoutContext ctx{
      node->status,
      style_of(*node),
      node->global_pos,
      node->children,
      node->internal_children,
      [this](Node *n, Constraints c) { return this->layout_node_(n, c); },
      device_scale_,
      invalidator()};
  // Widgets own their border box. The tree owns the outer margin box so every
  // widget, including third-party ones, gets identical margin behavior.
  Size<float> size = widget->layout(constraints.shrink(margin), ctx);
  node->size = size;
  return {size.width + margin.left + margin.right,
          size.height + margin.top + margin.bottom};
}

void WidgetTree::process_event(Event &e) {
  if (!root_)
    return;

  // Key events take a direct branch and leave. Pointer motion, including a
  // selection drag, therefore pays no RTTI checks for keyboard shortcuts.
  if (e.type() == EventType::KeyPressed) {
    auto &event = static_cast<KeyPressedEvent &>(e);
    if (!handle_selection_key_(event))
      bubble_event_(focused_node_, event);
    sync_focused_selection_();
    invalidate_(event.invalidation());
    return;
  }
  if (e.type() == EventType::KeyReleased) {
    bubble_event_(focused_node_, e);
    invalidate_(e.invalidation());
    return;
  }
  if (e.type() == EventType::TextEditing || e.type() == EventType::TextInput) {
    bubble_event_(focused_node_, e);
    sync_focused_selection_();
    invalidate_(e.invalidation());
    return;
  }

  e.dispatch<MouseMovedEvent>([&](MouseMovedEvent &e) {
    if (selection_dragging_ && selection_node_) {
      Widget *target = selection_node_->widget.get();
      Point<float> point = e.get_pos();
      if (point_in_node_space(*selection_node_, point, device_scale_)) {
        set_selection_(
            selection_node_, selection_anchor_,
            target->selection_hit_test(
                point, {selection_node_->global_pos, selection_node_->size}));
      }
    }

    Node *hit = hit_test_(root_.get(), e.get_pos());

    if (hit == hovered_node_) {
      // Hover styling is already current, but pointer motion is still an input
      // event. Returning before dispatch used to swallow every drag update
      // while the pointer remained over the same widget.
      bubble_event_(active_node_ ? active_node_ : hit, e);
      return EventResult::Unhandled;
    }

    // Clear last hit path
    for (Node *node = hovered_node_; node; node = node->parent) {
      node->status.set_hovered(false);
    }

    // Tag current hit path
    for (Node *node = hit; node; node = node->parent) {
      node->status.set_hovered(true);
    }

    // Only the two paths changed status, so everything above the node where
    // they meet is untouched. Re-resolving from that meeting point covers
    // every affected node in one pass -- and it must be a subtree pass, not a
    // per-node one, because a descendant selector like `button:hover .label`
    // can start matching further down while the hovered node itself is
    // unchanged.
    std::unordered_set<Node *> previous;
    for (Node *node = hovered_node_; node; node = node->parent) {
      node->style_node.status = node->status.bits();
      previous.insert(node);
    }
    Node *meeting_point = nullptr;
    for (Node *node = hit; node; node = node->parent) {
      node->style_node.status = node->status.bits();
      if (!meeting_point && previous.count(node) != 0)
        meeting_point = node;
    }
    if (!meeting_point)
      meeting_point = root_.get();

    if (meeting_point) {
      invalidate_(resolver_.resolve_subtree(meeting_point->style_node,
                                            /*force_subtree=*/true));
    }
    // Widget authors may still draw directly from DrawContext::status, so a
    // status edge needs one repaint even when its VSS only changes `cursor`.
    request_paint();

    // A widget that handled the left press owns pointer motion until release.
    // Scrollbar thumbs depend on this when the pointer leaves the thumb (or
    // even the viewport) during a drag.
    bubble_event_(active_node_ ? active_node_ : hit, e);

    hovered_node_ = hit;
    return EventResult::Unhandled;
  });

  e.dispatch<MousePressedEvent>([&](MousePressedEvent &e) {
    Node *hit = hit_test_(root_.get(), e.get_pos());

    if (e.button() == MouseButton::Left) {
      Node *selection = selectable_text_(hit);
      if (!selection) {
        clear_selection_();
      } else {
        Widget *target = selection->widget.get();
        const UserSelect policy =
            style_of(*selection).get<styles::UserSelect>();
        Point<float> point = e.get_pos();
        if (point_in_node_space(*selection, point, device_scale_)) {
          const std::uint32_t offset = target->selection_hit_test(
              point, {selection->global_pos, selection->size});
          if (policy == UserSelect::All || e.click_count() >= 3) {
            set_selection_(
                selection, 0,
                static_cast<std::uint32_t>(target->selection_text().size()));
            selection_dragging_ = false;
          } else if (e.click_count() == 2) {
            const auto [begin, end] = target->selection_word_at(offset);
            set_selection_(selection, begin, end);
            selection_dragging_ = false;
          } else {
            set_selection_(selection, offset, offset);
            selection_dragging_ = true;
          }
        }
      }
    }

    Node *receiver = bubble_event_(hit, e);

    if (receiver)
      mouse_down_widgets_[e.button()] = receiver;

    if (receiver && e.button() == MouseButton::Left) {
      active_node_ = receiver;
      active_node_->status.set_active(true);
      receiver->style_node.status = receiver->status.bits();
      invalidate_(resolver_.resolve_subtree(receiver->style_node, true));
      request_paint();
    }

    if (e.button() == MouseButton::Left || e.button() == MouseButton::Right) {
      Node *focusable = bubble_focusable_(hit);
      set_focus_(focusable);
    }

    return receiver ? EventResult::Handled : EventResult::Unhandled;
  });

  e.dispatch<MouseReleasedEvent>([&](MouseReleasedEvent &e) {
    if (e.button() == MouseButton::Left && selection_dragging_ &&
        selection_node_) {
      Widget *target = selection_node_->widget.get();
      Point<float> point = e.get_pos();
      if (point_in_node_space(*selection_node_, point, device_scale_)) {
        set_selection_(
            selection_node_, selection_anchor_,
            target->selection_hit_test(
                point, {selection_node_->global_pos, selection_node_->size}));
      }
      selection_dragging_ = false;
    }
    Node *hit = hit_test_(root_.get(), e.get_pos());
    Node *pressed_target = mouse_down_widgets_[e.button()];
    Node *receiver = bubble_event_(pressed_target ? pressed_target : hit, e);
    mouse_down_widgets_[e.button()] = nullptr;
    if (active_node_ && e.button() == MouseButton::Left) {
      active_node_->status.set_active(false);
      active_node_->style_node.status = active_node_->status.bits();
      invalidate_(resolver_.resolve_subtree(active_node_->style_node, true));
      request_paint();
      active_node_ = nullptr;
    }

    if (pressed_target && is_inside_(hit, pressed_target)) {
      MouseClickedEvent click_event(e.button(), e.get_pos());
      bubble_event_(pressed_target, click_event);
      invalidate_(click_event.invalidation());
    }
    mouse_down_widgets_[e.button()] = nullptr;
    return receiver || pressed_target ? EventResult::Handled
                                      : EventResult::Unhandled;
  });

  e.dispatch<MouseScrolledEvent>([&](MouseScrolledEvent &e) {
    Node *hit = hit_test_(root_.get(), e.get_pos());
    Node *receiver = bubble_event_(hit, e);
    return receiver ? EventResult::Handled : EventResult::Unhandled;
  });

  invalidate_(e.invalidation());
}

CursorShape WidgetTree::get_current_cursor_shape() const {
  if (!hovered_node_)
    return CursorShape::Default;

  // Cursor is inherited during style resolution, so the deepest hovered node
  // already carries the effective value. This replaces an ancestor walk and a
  // virtual call per level with one compact computed-style lookup.
  const ComputedStyle &style = style_of(*hovered_node_);
  const CursorShape cursor = style.get<styles::Cursor>();
  if (cursor != CursorShape::Auto)
    return cursor;
  return hovered_node_->widget->supports_text_selection() &&
                 style.get<styles::UserSelect>() != UserSelect::None
             ? CursorShape::Text
             : CursorShape::Default;
}

Node *WidgetTree::hit_test_(Node *node, Point<float> point) {
  if (!root_)
    return nullptr;

  Rect<float> bounds{node->global_pos, node->size};

  // Rendering composes the visual transform around the widget center and
  // applies it to the whole subtree. Walk the exact inverse here and pass the
  // mapped point down, so hit testing follows translated, scaled and rotated
  // widgets without storing another matrix on every Node.
  if (!untransform_point(*node, point, device_scale_))
    return nullptr;

  if (!bounds.contains(point))
    return nullptr;

  // Foreground controls drawn above clipped content, such as scrollbars, get
  // first refusal before the child underneath them.
  if (node->widget->foreground_hit_test(point, bounds))
    return node;

  const bool test_children =
      !node->widget->clips_children() ||
      node->widget->children_clip(bounds).contains(point);
  if (test_children) {
    for (auto it = node->internal_children.rbegin();
         it != node->internal_children.rend(); ++it) {
      if (Node *hit = hit_test_(it->get(), point))
        return hit;
    }
    for (auto it = node->children.rbegin(); it != node->children.rend(); ++it) {
      if (Node *hit = hit_test_(it->get(), point))
        return hit;
    }
  }

  return node;
}

void WidgetTree::render_node_(Node *node, Painter &painter) {
  if (!node)
    return;
  const ComputedStyle &style = style_of(*node);
  // Built on the stack from what the node already owns: no allocation and no
  // refcount traffic on the per-frame path.
  const bool has_selection =
      node == selection_node_ && selection_anchor_ != selection_focus_ &&
      style.get<styles::UserSelect>() != UserSelect::None;
  const DrawContext ctx{Rect<float>{node->global_pos, node->size},
                        node->status,
                        style,
                        *this,
                        std::min(selection_anchor_, selection_focus_),
                        std::max(selection_anchor_, selection_focus_),
                        has_selection};

  painter.save();
  const VisualTransform &visual = style.get<styles::Transform>();
  // Snapped where it enters the tree, not where each item leaves it. Every
  // descendant rounds its own final position onto the device grid, and that
  // rounding only carries a shift through unchanged when the shift is a whole
  // number of device pixels -- so a fractional one takes the subtree apart.
  const Transform transform =
      snap_translation_to_pixel(visual.matrix(), device_scale_);
  if (transform.is_translation()) {
    // Applied on its own. A translation already commutes with the centring
    // below, and routing it through the pair anyway computes `(c + t) - c`:
    // for a one-pixel shift beside a three-figure coordinate that cancellation
    // costs most of the bits, and what comes back out is no longer the whole
    // device pixel that was snapped -- which was the entire point.
    if (!transform.is_identity())
      painter.transform(transform);
  } else {
    const Point<float> center{
        ctx.bounds.origin.x + ctx.bounds.size.width * 0.5f,
        ctx.bounds.origin.y + ctx.bounds.size.height * 0.5f};
    painter.translate(center.x, center.y);
    painter.transform(transform);
    painter.translate(-center.x, -center.y);
  }
  painter.opacity(style.get<styles::Opacity>());

  // CSS paints the first shadow in the list on top of the ones after it, so
  // the list is walked backwards. Outer shadows go under the box; inner ones
  // are confined to it and go over its background.
  const ShadowList &shadows = style.get<styles::BoxShadow>();
  const Radius radius = style.get<styles::BorderRadius>();
  for (std::size_t i = shadows.size(); i-- > 0;) {
    if (!shadows[i].inset && shadows[i].color.a != 0.0f)
      painter.draw_shadow(ctx.bounds, radius, shadows[i]);
  }

  node->widget->draw(ctx, painter);

  // An inner shadow is clipped to the padding box, which keeps it off the
  // border the widget just drew. It lands over the widget's own content rather
  // than under it, which CSS has the other way round -- background and content
  // arrive here as one indivisible `draw`. Only a shadow reaching far enough
  // inward to touch the content notices.
  if (!shadows.empty()) {
    const float border = style.get<styles::BorderWidth>();
    const Rect<float> padding_box = deflate_rect(ctx.bounds, border);
    const Radius padding_radius = deflate_radius(radius, border);
    for (std::size_t i = shadows.size(); i-- > 0;) {
      if (shadows[i].inset && shadows[i].color.a != 0.0f)
        painter.draw_shadow(padding_box, padding_radius, shadows[i]);
    }
  }

  if (node->widget->clips_children()) {
    painter.save();
    painter.clip_rect(node->widget->children_clip(ctx.bounds));
    for (auto &child : node->children)
      render_node_(child.get(), painter);
    for (auto &child : node->internal_children)
      render_node_(child.get(), painter);
    painter.restore();
  } else {
    for (auto &child : node->children)
      render_node_(child.get(), painter);
    for (auto &child : node->internal_children)
      render_node_(child.get(), painter);
  }

  node->widget->draw_foreground(ctx, painter);
  painter.restore();
}

Node *WidgetTree::bubble_event_(Node *target, Event &e) {
  for (Node *node = target; node; node = node->parent) {
    if (node->widget->on_event(e) == EventResult::Handled)
      return node;
  }

  return nullptr;
}

bool WidgetTree::is_inside_(Node *node, Node *ancestor) const {
  for (; node; node = node->parent) {
    if (node == ancestor)
      return true;
  }
  return false;
}

Node *WidgetTree::selectable_text_(Node *target) const {
  for (Node *node = target; node; node = node->parent) {
    if (node->widget->accepts_text_input())
      return node;
    if (node->widget->focusable())
      return nullptr;
  }
  for (Node *node = target; node; node = node->parent) {
    if (style_of(*node).get<styles::UserSelect>() == UserSelect::None)
      return nullptr;
    if (node->widget->supports_text_selection())
      return node;
  }
  return nullptr;
}

void WidgetTree::set_selection_(Node *node, std::uint32_t anchor,
                                std::uint32_t focus) {
  const std::size_t size = node->widget->selection_text().size();
  anchor = static_cast<std::uint32_t>(std::min<std::size_t>(anchor, size));
  focus = static_cast<std::uint32_t>(std::min<std::size_t>(focus, size));
  if (selection_node_ == node && selection_anchor_ == anchor &&
      selection_focus_ == focus)
    return;
  selection_node_ = node;
  selection_anchor_ = anchor;
  selection_focus_ = focus;
  node->widget->selection_changed(anchor, focus);
  request_paint();
}

void WidgetTree::clear_selection_() {
  if (!selection_node_)
    return;
  if (selection_node_->widget->accepts_text_input())
    selection_node_->widget->selection_changed(selection_focus_,
                                               selection_focus_);
  selection_node_ = nullptr;
  selection_anchor_ = selection_focus_ = 0;
  selection_dragging_ = false;
  request_paint();
}

void WidgetTree::sync_focused_selection_() {
  if (!focused_node_ || !focused_node_->widget->accepts_text_input())
    return;
  const auto [anchor, focus] = focused_node_->widget->text_selection();
  set_selection_(focused_node_, anchor, focus);
}

bool WidgetTree::wants_text_input() const {
  return focused_node_ && focused_node_->widget->accepts_text_input();
}

const Node *WidgetTree::text_input_client() const {
  return wants_text_input() ? focused_node_ : nullptr;
}

std::optional<TextInputArea> WidgetTree::text_input_area() const {
  if (!focused_node_ || !focused_node_->widget->accepts_text_input())
    return std::nullopt;

  std::optional<TextInputArea> area = focused_node_->widget->text_input_area(
      {focused_node_->global_pos, focused_node_->size});
  if (!area)
    return std::nullopt;

  // Rendering applies each visual transform to the complete subtree. Native
  // IME UI lives outside that renderer, so reproduce the same root-to-leaf
  // transform here and hand the backend the caret's visual position.
  std::vector<const Node *> path;
  for (const Node *node = focused_node_; node; node = node->parent)
    path.push_back(node);

  Transform transform;
  for (auto it = path.rbegin(); it != path.rend(); ++it) {
    const Node &node = **it;
    const Transform visual = snap_translation_to_pixel(
        style_of(node).get<styles::Transform>().matrix(), device_scale_);
    Transform around_center = visual;
    if (!visual.is_translation()) {
      const Point<float> center{node.global_pos.x + node.size.width * 0.5f,
                                node.global_pos.y + node.size.height * 0.5f};
      around_center = Transform::translate(center.x, center.y)
                          .concat(visual)
                          .concat(Transform::translate(-center.x, -center.y));
    }
    transform = transform.concat(around_center);
  }

  const Point<float> cursor{area->rect.origin.x + area->cursor,
                            area->rect.origin.y +
                                area->rect.size.height * 0.5f};
  const Rect<float> transformed = transform.map_bounds(area->rect);
  area->cursor = transform.apply(cursor).x - transformed.origin.x;
  area->rect = transformed;
  return area;
}

bool WidgetTree::handle_selection_key_(KeyPressedEvent &event) {
  if (!selection_node_)
    return false;

  if (style_of(*selection_node_).get<styles::UserSelect>() ==
      UserSelect::None) {
    clear_selection_();
    return false;
  }

  if (event.keycode() == Keycode::Escape) {
    clear_selection_();
    return true;
  }
  const bool dedicated_copy = event.keycode() == Keycode::Copy;
  if (!event.modifiers().primary() && !dedicated_copy)
    return false;

  Widget *target = selection_node_->widget.get();
  const std::string_view text = target->selection_text();

  if (event.keycode() == Keycode::A) {
    set_selection_(selection_node_, 0, static_cast<std::uint32_t>(text.size()));
    return true;
  }
  if (event.keycode() != Keycode::C && event.keycode() != Keycode::Copy)
    return false;

  const std::size_t begin = std::min<std::size_t>(
      std::min(selection_anchor_, selection_focus_), text.size());
  const std::size_t end = std::min<std::size_t>(
      std::max(selection_anchor_, selection_focus_), text.size());
  if (begin == end)
    return true;

  // SDL requires a null-terminated string. Copying allocates only when the
  // user explicitly copies; selection, dragging and painting remain allocation
  // free.
  const std::string selected(text.substr(begin, end - begin));
  SDL_SetClipboardText(selected.c_str());
  return true;
}

/// Find first ancestor that is focusable.
Node *WidgetTree::bubble_focusable_(Node *target) {
  for (; target; target = target->parent) {
    if (target->widget->focusable())
      return target;
  }
  return nullptr;
}

void WidgetTree::set_focus_(Node *node) {
  if (node == focused_node_)
    return;

  Node *prev = focused_node_;
  focused_node_ = node;

  if (prev) {
    prev->widget->text_input_stopped();
    prev->status.set_focused(false);
    prev->style_node.status = prev->status.bits();
    invalidate_(resolver_.resolve_subtree(prev->style_node, true));
  }

  if (focused_node_) {
    focused_node_->status.set_focused(true);
    focused_node_->style_node.status = focused_node_->status.bits();
    invalidate_(resolver_.resolve_subtree(focused_node_->style_node, true));
  }
  request_layout();
}

void WidgetTree::link_style_tree_(Node *node) {
  StyleNode &style_node = node->style_node;
  style_node.children.clear();
  style_node.parent = node->parent ? &node->parent->style_node : nullptr;

  // Internal children are part of the style tree -- they inherit, and
  // ::part() reaches them -- so both lists are linked. What keeps them
  // private is the boundary check in the matcher, not their absence here.
  for (auto &child : node->children) {
    style_node.children.push_back(&child->style_node);
    link_style_tree_(child.get());
  }
  for (auto &child : node->internal_children) {
    style_node.children.push_back(&child->style_node);
    link_style_tree_(child.get());
  }
}

void WidgetTree::restyle() {
  if (!root_)
    return;
  style_rebuild_blooms(root_->style_node);
  invalidate_(resolver_.resolve_tree(root_->style_node));
}

void WidgetTree::invalidate_(Invalidation invalidation) {
  invalidation_ = max_invalidation(invalidation_, invalidation);
}

void WidgetTree::request_paint() {
  assert_owner_thread_();
  invalidate_(Invalidation::Paint);
}

void WidgetTree::request_layout() {
  assert_owner_thread_();
  invalidation_ = Invalidation::Layout;
}

void WidgetTree::request_paint_at(double when_seconds) {
  assert_owner_thread_();
  next_wake_ = std::min(next_wake_, when_seconds);
}

bool WidgetTree::advance_animations(double now_seconds) {
  frame_time_ = now_seconds;
  // A deadline fires once and is disarmed here rather than in `render`: the
  // widget that wants the next one arms it again while it paints, so a caret
  // that stops blinking -- because it lost focus, or its field was removed --
  // takes its deadline with it and the window goes back to sleep.
  if (now_seconds >= next_wake_) {
    next_wake_ = std::numeric_limits<double>::infinity();
    request_paint();
  }
  invalidate_(resolver_.advance_animations(now_seconds));
  return resolver_.has_active_animations();
}

void WidgetTree::set_stylesheet(std::shared_ptr<const StyleSheet> sheet) {
  resolver_.set_stylesheet(std::move(sheet));
  restyle();
}

void WidgetTree::set_theme(std::shared_ptr<const Theme> theme) {
  resolver_.set_theme(std::move(theme));
  restyle();
}

void WidgetTree::build(std::unique_ptr<Widget> root) {
  dirty_components_.clear();
  if (root_)
    discard_node_(root_.get());
  root_ = build_node_(std::move(root));
  if (root_) {
    link_style_tree_(root_.get());
    restyle();
  }
  request_layout();
}

void WidgetTree::render(Painter &painter) {
  if (!root_)
    return;
  // Consume the current paint request before drawing. A widget can request the
  // next animation frame from DrawContext and that new request then survives.
  if (invalidation_ == Invalidation::Paint)
    invalidation_ = Invalidation::None;
  render_node_(root_.get(), painter);
  // Keep the scheduler awake only while motion exists. The next frame samples
  // all active nodes before drawing; once the dense active list empties this
  // request disappears and SDL returns to a blocking wait.
  if (resolver_.has_active_animations())
    request_paint();
}

void WidgetTree::set_device_scale(float scale) {
  const float next = scale > 0.0f ? scale : 1.0f;
  if (device_scale_ == next)
    return;
  device_scale_ = next;
  request_layout();
}

void WidgetTree::layout(Constraints constraints) {
  if (!root_)
    return;
  flush_component_updates_();
  const Node *box = layout_box_of(root_.get());
  const Spacing<MarginValue> &specified_margin = style_of(*box).layout_margin();
  const Spacing<float> margin = resolve_fixed_margin(specified_margin);
  root_->global_pos = root_.get() == box ? Point<float>{margin.left, margin.top}
                                         : Point<float>{};
  layout_node_(root_.get(), constraints);

  if (std::isfinite(constraints.max_width) &&
      (specified_margin.left.is_auto() || specified_margin.right.is_auto())) {
    const float outer_width = box->size.width + margin.left + margin.right;
    const float remaining = std::max(constraints.max_width - outer_width, 0.0f);
    const float left =
        specified_margin.left.is_auto()
            ? remaining /
                  static_cast<float>(specified_margin.right.is_auto() ? 2 : 1)
            : 0.0f;
    if (left > 0.0f)
      translate_subtree(*root_, {left, 0.0f});
  }
  invalidation_ = Invalidation::Paint;
}

} // namespace voidui
