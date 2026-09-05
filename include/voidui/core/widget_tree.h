#pragma once

#include <cassert>
#include <limits>
#include <memory>
#include <string>
#include <thread>

#include "voidui/core/cursor.h"
#include "voidui/core/state.h"
#include "voidui/core/style.h"
#include "voidui/core/widget.h"

namespace voidui {

struct Node {
  std::unique_ptr<Widget> widget;
  std::unique_ptr<detail::ComponentRuntime> component_runtime;
  Node *parent = nullptr;
  std::vector<std::unique_ptr<Node>> children;
  std::vector<std::unique_ptr<Node>> internal_children;

  WidgetStatus status;
  Point<float> global_pos{0.0f, 0.0f};
  Size<float> size{0.0f, 0.0f};

  /// The matching-relevant projection of this node. Kept as a separate struct
  /// so the style system stays testable without widgets, painting or a window.
  StyleNode style_node;

  /// Non-empty when this node is an internal child its component exposed under
  /// `host::part(name)`.
  std::string part;
};

inline const Node *layout_box_of(const Node *node) {
  while (node && node->style_node.is_transparent && !node->children.empty())
    node = node->children[0].get();
  return node;
}

inline void translate_subtree(Node &node, Point<float> delta) {
  node.global_pos.x += delta.x;
  node.global_pos.y += delta.y;
  for (auto &child : node.children)
    translate_subtree(*child, delta);
  for (auto &child : node.internal_children)
    translate_subtree(*child, delta);
}

class WidgetTree {
public:
  WidgetTree();
  explicit WidgetTree(std::unique_ptr<Widget> root);
  ~WidgetTree();

  WidgetTree(const WidgetTree &) = delete;
  WidgetTree &operator=(const WidgetTree &) = delete;

  /// A handle background work can hold and invalidate through once it finishes.
  ///
  /// Handed out rather than letting a caller keep a `WidgetTree *`, because the
  /// two things that make such a pointer safe -- the tree still existing, and
  /// the hop back onto the UI thread -- are exactly what a worker cannot check
  /// for itself. See Invalidator.
  Invalidator invalidator();

  void build(std::unique_ptr<Widget> root);

  // -- Styling ---------------------------------------------------------------

  /// Replaces the active stylesheet and re-resolves. Cheap enough to call from
  /// a hot reload.
  void set_stylesheet(std::shared_ptr<const StyleSheet> sheet);

  /// Replaces the active theme and re-resolves. No rule is re-parsed: theme
  /// switching only rebinds tokens.
  void set_theme(std::shared_ptr<const Theme> theme);

  StyleResolver &style_resolver() { return resolver_; }

  Node *root() { return root_.get(); }
  const Node *root() const { return root_.get(); }

  /// Re-runs matching over the whole tree. Called automatically on build, on a
  /// stylesheet change and on a theme change.
  void restyle();

  /// Constant-time invalidation used by the window scheduler and future
  /// animation drivers. Layout includes paint because geometry changes alter
  /// every following draw command.
  void request_paint();
  void request_layout();
  bool needs_layout() const { return invalidation_ == Invalidation::Layout; }
  bool needs_paint() const { return invalidation_ != Invalidation::None; }

  /// Asks for a repaint at a moment, rather than on the very next frame.
  ///
  /// A style animation runs every frame and holds the scheduler awake by
  /// requesting paint from `render`. A caret that blinks twice a second must
  /// not: it would keep a window at its full frame rate for the sake of two
  /// state changes. Earliest deadline wins, and it is armed again by whoever
  /// drew it, so a stale one cannot pin the scheduler.
  void request_paint_at(double when_seconds);

  /// The earliest armed deadline, or infinity when nothing is waiting. The
  /// window scheduler waits on this instead of blocking indefinitely.
  double next_wake_time() const {
    return selection_dragging_ ? std::min(next_wake_, selection_scroll_next_)
                               : next_wake_;
  }

  /// The timestamp `advance_animations` was last given. Widgets read it while
  /// painting so that time enters the tree at exactly one point.
  double frame_time() const { return frame_time_; }

  /// Advances the sparse style animation set. `now_seconds` must come from a
  /// monotonic clock; exposing it keeps animation tests deterministic.
  bool advance_animations(double now_seconds);
  bool has_active_animations() const {
    return resolver_.has_active_animations();
  }

  void render(Painter &painter);
  void layout(Constraints constraints);

  /// Device pixels per logical unit. Set this before `layout`, never between
  /// `layout` and `render`: text rounds its line box to this scale at layout
  /// time, and changing it afterwards would paint a layout built for one
  /// display while the renderer projects for another.
  void set_device_scale(float scale);
  void process_event(Event &e);
  /// Selected UTF-8 text in declaration order, with line breaks between rows.
  /// Uses the same range as painting and the Copy/Ctrl+C shortcut.
  std::string selected_text();
  bool wants_text_input() const;
  const Node *text_input_client() const;
  std::optional<TextInputArea> text_input_area() const;
  /// During capture, the dragged widget owns the cursor even outside its
  /// bounds. Otherwise use the current hover target's computed cursor.
  CursorShape get_current_cursor_shape() const;

private:
  friend class detail::ComponentRuntime;

  std::unique_ptr<Node> build_node_(std::unique_ptr<Widget> widget);
  std::unique_ptr<Node> reconcile_node_(std::unique_ptr<Node> node,
                                        std::unique_ptr<Widget> declaration);
  void reconcile_children_(std::vector<std::unique_ptr<Node>> &nodes,
                           std::vector<std::unique_ptr<Widget>> declarations,
                           Node *parent, bool internal,
                           const std::vector<std::string> *parts = nullptr);
  void render_component_(Node *node);
  void flush_component_updates_();
  void queue_component_(Node *node);
  void refresh_style_node_(Node *node);
  void discard_node_(Node *node);
  Size<float> layout_node_(Node *node, Constraints constraints);
  void position_subtree_(Node *node, bool positioned = false);
  Rect<float> containing_block_(const Node *node) const;
  Node *hit_test_(Node *node, Point<float> point);
  void draw_node_(Node *node, Painter &painter, bool foreground);
  void update_paint_order_();
  void render_ordered_(Painter &painter);
  void rebuild_overlays_();
  void layout_overlays_();
  void update_overlays_();
  bool dismiss_overlays_(Event &event);
  void deactivate_overlay_(Node *node);
  void close_overlay_(Node *node, OverlayDismissReason reason, bool notify);
  void rebuild_overlay_stack_();
  void sync_modal_focus_();
  void flush_overlay_notifications_();
  void forget_overlays_(Node *node);
  bool belongs_to_overlay_(const Node *node, const Node *owner) const;
  bool input_allowed_(const Node *node) const;
  void move_focus_(bool backward);
  void draw_range_(Painter &painter, std::size_t begin, std::size_t end);
  Node *hit_range_(Point<float> point, std::size_t begin, std::size_t end);
  Node *bubble_event_(Node *target, Event &e);
  bool is_inside_(Node *node, Node *ancestor) const;
  Node *bubble_focusable_(Node *target);
  void set_focus_(Node *node, bool trigger_hints = true);
  void set_hovered_(Node *node);
  void dispatch_event_(Event &event);
  Node *selectable_text_(Node *target) const;
  void set_selection_(Node *node, std::uint32_t anchor, std::uint32_t focus);
  void extend_selection_(Point<float> point);
  Node *selection_scroll_owner_(Node *pointer_node) const;
  Rect<float> selection_scroll_bounds_(Node *node) const;
  void advance_selection_scroll_(double now_seconds);
  void collect_selection_nodes_();
  void refresh_selection_();
  std::pair<std::uint32_t, std::uint32_t> selection_range_(Node *node) const;
  void clear_selection_();
  bool handle_selection_key_(KeyPressedEvent &event);
  void sync_focused_selection_();
  void invalidate_(Invalidation invalidation);
  void assert_owner_thread_() const noexcept {
#ifndef NDEBUG
    assert(owner_thread_ == std::this_thread::get_id() &&
           "VoidUI trees must be mutated on their owning UI thread");
#endif
  }

private:
  void link_style_tree_(Node *node);

  std::unique_ptr<Node> root_;
  Node *hovered_node_ = nullptr;
  Node *active_node_ = nullptr;
  Node *focused_node_ = nullptr;
  // Restoring a modal's opener keeps keyboard focus without reopening its hint.
  bool focus_triggers_hints_ = true;
  Node *selection_node_ = nullptr;
  Node *selection_focus_node_ = nullptr;
  std::uint32_t selection_anchor_ = 0;
  std::uint32_t selection_focus_ = 0;
  bool selection_dragging_ = false;
  Point<float> selection_pointer_{};
  double selection_scroll_next_ = std::numeric_limits<double>::infinity();
  double selection_scroll_last_ = 0.0;
  bool selection_by_word_ = false;
  std::uint32_t selection_word_begin_ = 0;
  std::uint32_t selection_word_end_ = 0;
  // Reused buffers keep document traversal out of individual widget draws.
  std::vector<Node *> selection_nodes_;
  struct SelectionRange {
    Node *node;
    std::uint32_t begin, end;
  };
  std::vector<SelectionRange> selection_ranges_;
  std::unordered_map<MouseButton, Node *> mouse_down_widgets_;
  std::vector<Node *> dirty_components_;
  StyleResolver resolver_;

  /// Handed to every Invalidator this tree issues, and cleared on the way out,
  /// so a load that lands after the window closed finds nothing to poke.
  std::shared_ptr<detail::TreeToken> token_;

  Invalidation invalidation_ = Invalidation::Layout;
  double frame_time_ = 0.0;
  double next_wake_ = std::numeric_limits<double>::infinity();
  float device_scale_ = 1.0f;
  Size<float> viewport_{};
  // Sparse storage: only portal roots need activation state. No handles,
  // secondary ownership tree or per-Node overlay payload are required.
  struct OverlayEntry {
    Node *node;
    Node *owner = nullptr;
    Node *declared_owner = nullptr;
    Node *return_focus = nullptr;
    std::uint64_t sequence = 0;
    std::size_t paint_begin = 0;
    std::size_t paint_end = 0;
    double show_at = std::numeric_limits<double>::infinity();
    bool requested = false;
    bool focused = false;
    bool visible = false;
    bool suppressed = false;
    bool layout_dirty = true;
    Size<float> measure_limit{-1, -1};
    float measure_min_width = -1;
  };
  std::vector<OverlayEntry> overlays_;
  // Indices into declaration-order entries, sorted only on activation edges.
  std::vector<std::size_t> overlay_stack_;
  struct OverlayNotification { Node *node; OverlayDismissReason reason; };
  std::vector<OverlayNotification> overlay_notifications_;
  Node *top_modal_ = nullptr;
  Node *restore_focus_ = nullptr;
  std::uint64_t overlay_sequence_ = 0;
  bool overlay_stack_dirty_ = false;
  bool modal_focus_dirty_ = false;
  std::uint8_t swallowed_releases_ = 0;
  bool swallowed_escape_release_ = false;
  struct PaintEntry {
    Node *node;
    int level = 0;
    bool foreground;
    bool context = false;
    bool participant = false;
  };
  std::vector<PaintEntry> paint_order_;
  struct PaintScope {
    const Node *node;
    bool parent_clip;
  };
  std::vector<PaintScope> paint_path_;
  std::vector<PaintScope> next_paint_path_;
  bool paint_order_dirty_ = true;
  bool paint_structure_dirty_ = true;
#ifndef NDEBUG
  std::thread::id owner_thread_ = std::this_thread::get_id();
#endif
};

} // namespace voidui
