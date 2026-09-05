#include "voidui/widgets/sidebar.h"

namespace voidui {
namespace {

// Slot boundaries clip overflowing content without unmounting its state.
class SidebarViewport : public Widget {
public:
  VOIDUI_STYLE_SCOPE(SidebarViewport, "sidebar-viewport")
  SidebarViewport(std::unique_ptr<Widget> child, std::shared_ptr<bool> enabled)
      : child_(std::move(child)), enabled_(std::move(enabled)) {}
  std::unique_ptr<Widget> clone() const override {
    return std::make_unique<SidebarViewport>(
        child_ ? clone_widget(*child_) : nullptr, enabled_);
  }
  void register_children(Registrar &r) override {
    if (child_) r.take_child(std::move(child_));
  }
  Size<float> layout(Constraints c, LayoutContext &ctx) override {
    Size<float> measured{};
    if (const auto index = ctx.flow_index(0);
        index && (c.min_width != c.max_width || c.min_height != c.max_height))
      measured = ctx.constrain_child(*index, c);
    const auto size = c.resolve(ctx.style.layout_size(), measured);
    if (const auto index = ctx.flow_index(0)) {
      ctx.constrain_child(*index, {size.width, size.width, size.height, size.height});
      ctx.place_child(*index, {});
    }
    return size;
  }
  bool clips_children() const override { return true; }
  bool children_visible() const override { return !enabled_ || *enabled_; }
  void draw(const DrawContext &ctx, Painter &painter) override {
    detail::draw_container_box(ctx, painter);
  }
  EventResult on_event(Event &) override { return EventResult::Unhandled; }
private:
  std::unique_ptr<Widget> child_;
  std::shared_ptr<bool> enabled_;
};

class SidebarHandle : public Widget {
public:
  VOIDUI_STYLE_SCOPE(SidebarHandle, "sidebar-handle")
  explicit SidebarHandle(std::function<EventResult(Event &)> event, bool enabled,
                         bool horizontal)
      : event_(std::move(event)), enabled_(enabled), horizontal_(horizontal) {
    set_style<styles::Cursor>(enabled ? (horizontal ? CursorShape::HorizontalResize
                                                  : CursorShape::VerticalResize)
                                     : CursorShape::Default);
  }
  std::unique_ptr<Widget> clone() const override {
    return std::make_unique<SidebarHandle>(event_, enabled_, horizontal_);
  }
  void register_children(Registrar &) override {}
  Size<float> layout(Constraints c, LayoutContext &ctx) override {
    return c.resolve(ctx.style.layout_size(), {});
  }
  void draw(const DrawContext &ctx, Painter &painter) override {
    if (ctx.status.is_focused())
      painter.fill_rect(ctx.bounds, Paint(Color(30, 195, 190, 65)));
  }
  bool focusable() const override { return enabled_; }
  EventResult on_event(Event &event) override {
    return enabled_ ? event_(event) : EventResult::Unhandled;
  }
private:
  std::function<EventResult(Event &)> event_;
  bool enabled_, horizontal_;
};

Rect<float> axis_rect(bool horizontal, float start, float extent, float cross) {
  return horizontal ? Rect<float>(start, 0, extent, cross)
                    : Rect<float>(0, start, cross, extent);
}
} // namespace

Sidebar::Sidebar(std::unique_ptr<Widget> panel, std::unique_ptr<Widget> content)
    : panel_(std::move(panel)), content_(std::move(content)) {
  set_style<styles::Width>(Length::Fill{});
  set_style<styles::Height>(Length::Fill{});
}

bool Sidebar::horizontal_() const {
  return placement_ == SidebarPlacement::Left || placement_ == SidebarPlacement::Right;
}
float Sidebar::direction_() const {
  return placement_ == SidebarPlacement::Left || placement_ == SidebarPlacement::Top
             ? 1.0f : -1.0f;
}
float Sidebar::axis_(Point<float> p) const { return horizontal_() ? p.x : p.y; }

bool Sidebar::elastic_(const std::optional<SidebarDragBehavior> &behavior,
                       bool resizing) const {
  if (drag_mode_ == SidebarDragMode::Disabled) return false;
  if (behavior) return *behavior == SidebarDragBehavior::Elastic;
  return drag_mode_ == SidebarDragMode::Elastic ||
         (drag_mode_ == SidebarDragMode::ElasticOpenClose && !resizing);
}

std::unique_ptr<Widget> Sidebar::clone() const {
  auto copy = std::make_unique<Sidebar>(panel_ ? clone_widget(*panel_) : nullptr,
                                        content_ ? clone_widget(*content_) : nullptr);
  copy->placement_ = placement_;
  copy->mode_ = mode_;
  copy->drag_mode_ = drag_mode_;
  copy->open_behavior_ = open_behavior_;
  copy->resize_behavior_ = resize_behavior_;
  copy->collapse_behavior_ = collapse_behavior_;
  copy->edge_visible_ = edge_visible_;
  copy->open_ = open_;
  copy->declared_open_ = declared_open_;
  copy->open_explicit_ = open_explicit_;
  copy->extent_ = extent_;
  copy->declared_extent_ = declared_extent_;
  copy->extent_explicit_ = extent_explicit_;
  copy->min_extent_ = min_extent_;
  copy->max_extent_ = max_extent_;
  copy->collapsed_extent_ = collapsed_extent_;
  copy->handle_size_ = handle_size_;
  copy->drag_threshold_ = drag_threshold_;
  copy->collapse_threshold_ = collapse_threshold_;
  copy->open_binding_ = open_binding_;
  copy->extent_binding_ = extent_binding_;
  copy->on_open_change_ = on_open_change_;
  copy->on_extent_change_ = on_extent_change_;
  return copy;
}

void Sidebar::inherit_runtime(const Widget &previous) {
  const auto &old = static_cast<const Sidebar &>(previous);
  if (!open_binding_ && (!open_explicit_ || declared_open_ == old.declared_open_))
    open_ = old.open_;
  if (!extent_binding_ && (!extent_explicit_ || declared_extent_ == old.declared_extent_))
    extent_ = old.extent_;
  panel_rect_ = old.panel_rect_;
  content_rect_ = old.content_rect_;
  handle_rect_ = old.handle_rect_;
  visible_extent_ = old.visible_extent_;
  available_extent_ = old.available_extent_;
  *panel_visible_ = *old.panel_visible_;
  // A binding echo or unrelated rebuild must not interrupt pointer capture.
  // An external state/configuration change deliberately cancels the gesture.
  if (open_ != old.open_ || extent_ != old.extent_ || placement_ != old.placement_ ||
      mode_ != old.mode_ || drag_mode_ != old.drag_mode_ ||
      open_behavior_ != old.open_behavior_ || resize_behavior_ != old.resize_behavior_ ||
      collapse_behavior_ != old.collapse_behavior_ ||
      min_extent_ != old.min_extent_ || max_extent_ != old.max_extent_ ||
      collapsed_extent_ != old.collapsed_extent_ || handle_size_ != old.handle_size_ ||
      drag_threshold_ != old.drag_threshold_ || collapse_threshold_ != old.collapse_threshold_)
    return;
  dragging_ = old.dragging_;
  drag_start_open_ = old.drag_start_open_;
  drag_restore_extent_ = old.drag_restore_extent_;
  drag_phase_ = old.drag_phase_;
  drag_direction_ = old.drag_direction_;
  drag_last_pointer_ = old.drag_last_pointer_;
  drag_direction_origin_ = old.drag_direction_origin_;
  drag_edge_anchor_ = old.drag_edge_anchor_;
  drag_edge_offset_ = old.drag_edge_offset_;
  drag_resize_extent_ = old.drag_resize_extent_;
  drag_reopen_extent_ = old.drag_reopen_extent_;
  elastic_offset_ = old.elastic_offset_;
  release_offset_ = old.release_offset_;
  release_time_ = old.release_time_;
}

void Sidebar::register_children(Registrar &r) {
  // Paint the main content first, so the panel also works in overlay mode.
  r.take_internal_child(std::make_unique<SidebarViewport>(std::move(content_), nullptr), "content");
  r.take_internal_child(std::make_unique<SidebarViewport>(std::move(panel_), panel_visible_), "panel");
  r.take_internal_child(std::make_unique<SidebarHandle>(
      [this](Event &e) { return handle_event_(e); },
      drag_mode_ != SidebarDragMode::Disabled, horizontal_()), "handle");
}

Size<float> Sidebar::layout(Constraints c, LayoutContext &ctx) {
  // With unbounded constraints, measure the main slot for a finite fallback.
  Size<float> intrinsic{};
  if ((!std::isfinite(c.max_width) || !std::isfinite(c.max_height)) && ctx.flow_index(0))
    intrinsic = ctx.constrain_child(*ctx.flow_index(0), c);
  const float desired = open_ ? std::clamp(extent_, min_extent_, max_extent_)
                              : std::min(collapsed_extent_, min_extent_);
  const float grip = drag_mode_ == SidebarDragMode::Disabled ? 0.0f : handle_size_;
  if (horizontal_()) intrinsic.width += desired + grip;
  else intrinsic.height += desired + grip;
  const auto size = c.resolve(ctx.style.layout_size(), intrinsic);
  const float total = horizontal_() ? size.width : size.height;
  const float cross = horizontal_() ? size.height : size.width;
  const float actual_grip = std::min(grip, total);
  available_extent_ = std::max(total - actual_grip, 0.0f);
  visible_extent_ = std::min(desired, available_extent_);
  *panel_visible_ = visible_extent_ > 0.0f;
  const float occupied = visible_extent_ + actual_grip;
  const bool leading = direction_() > 0;
  panel_rect_ = axis_rect(horizontal_(), leading ? 0.0f : total - visible_extent_, visible_extent_, cross);
  handle_rect_ = axis_rect(horizontal_(), leading ? visible_extent_ : total - occupied, actual_grip, cross);
  content_rect_ = mode_ == SidebarMode::Overlay
                      ? Rect<float>({}, size)
                      : axis_rect(horizontal_(), leading ? occupied : 0.0f,
                                  std::max(total - occupied, 0.0f), cross);
  const Rect<float> rects[] = {content_rect_, panel_rect_, handle_rect_};
  for (std::size_t i = 0; i < 3; ++i) {
    if (const auto flow = ctx.flow_index(i)) {
      const auto &rect = rects[i];
      ctx.constrain_child(*flow, {rect.size.width, rect.size.width,
                                  rect.size.height, rect.size.height});
      ctx.place_child(*flow, rect.origin);
    }
  }
  return size;
}

void Sidebar::draw(const DrawContext &ctx, Painter &painter) {
  painter.fill_rect(ctx.bounds, Paint(ctx.style.get<styles::Background>()));
}

void Sidebar::draw_foreground(const DrawContext &ctx, Painter &painter) {
  if (drag_mode_ == SidebarDragMode::Disabled) return;
  if (!edge_visible_) {
    // Hidden feedback should not keep the frame scheduler awake on release.
    release_offset_ = 0.0f;
    release_time_.reset();
    if (!dragging_) elastic_offset_ = 0.0f;
    return;
  }
  painter.clip_rect(ctx.bounds);
  if (!dragging_ && release_offset_ != 0.0f) {
    if (!release_time_) release_time_ = ctx.now();
    const float t = static_cast<float>(std::max(0.0, ctx.now() - *release_time_));
    elastic_offset_ = release_offset_ * std::exp(-24.0f * t);
    if (t >= 0.25f) {
      elastic_offset_ = release_offset_ = 0.0f;
      release_time_.reset();
    } else ctx.request_frame();
  }
  const float center = axis_(handle_rect_.origin) +
                       (horizontal_() ? handle_rect_.size.width : handle_rect_.size.height) * 0.5f;
  const float cross = horizontal_() ? ctx.bounds.size.height : ctx.bounds.size.width;
  const auto point = [&](float axis, float other) {
    return horizontal_() ? Point<float>{ctx.bounds.origin.x + axis, ctx.bounds.origin.y + other}
                         : Point<float>{ctx.bounds.origin.x + other, ctx.bounds.origin.y + axis};
  };
  Path line;
  line.move_to(point(center, 0.0f));
  // 4/3 makes the cubic midpoint displacement equal to elastic_offset_.
  const float bow = direction_() * elastic_offset_ * (4.0f / 3.0f);
  line.cubic_to(point(center + bow, cross / 3.0f),
                point(center + bow, cross * 2.0f / 3.0f), point(center, cross));
  painter.stroke_path(line, Paint(dragging_ || release_offset_ != 0.0f
                                     ? ctx.style.get<ElasticColor>()
                                     : ctx.style.get<HandleColor>()), Pen(2.0f));
}

void Sidebar::change_(bool open, float extent, Event &event) {
  extent = std::clamp(extent, min_extent_, max_extent_);
  const bool open_changed = open != open_;
  const bool extent_changed = extent != extent_;
  if (!open_changed && !extent_changed) return;
  open_ = open;
  extent_ = extent;
  event.request_layout();
  // Copy all handles before invoking user code (which may rebuild the tree).
  auto open_binding = open_binding_;
  auto extent_binding = extent_binding_;
  auto open_callback = on_open_change_;
  auto extent_callback = on_extent_change_;
  if (open_changed && open_binding) open_binding->set(open);
  if (extent_changed && extent_binding) extent_binding->set(extent);
  if (open_changed && open_callback) open_callback(open);
  if (extent_changed && extent_callback) extent_callback(extent);
}

void Sidebar::arm_drag_(float pointer, float visible_extent) {
  drag_phase_ = DragPhase::Pending;
  drag_direction_ = 0;
  drag_last_pointer_ = drag_direction_origin_ = pointer;
  drag_edge_anchor_ = drag_edge_offset_ + visible_extent;
  drag_resize_extent_ = visible_extent;
  elastic_offset_ = release_offset_ = 0.0f;
  release_time_.reset();
}

void Sidebar::move_drag_(Point<float> point, Event &event) {
  // In this coordinate system positive always means expanding, on every edge.
  const float pointer = axis_(point) * direction_();
  const float delta = pointer - drag_last_pointer_;
  if (!std::isfinite(delta) || delta == 0.0f) return;
  const int direction = delta > 0.0f ? 1 : -1;
  if (direction != drag_direction_) {
    drag_direction_origin_ = drag_last_pointer_;
    drag_direction_ = direction;
    // Remember the useful size before shrinking, not the near-zero size just
    // before collapse. Escape has a separate snapshot of the entire gesture.
    if (open_ && direction < 0) drag_reopen_extent_ = extent_;
  }
  drag_last_pointer_ = pointer;
  const bool elastic_resize = elastic_(resize_behavior_, true);
  const bool elastic_collapse = elastic_(collapse_behavior_);
  const float threshold = (open_ ? elastic_resize : elastic_(open_behavior_))
                              ? drag_threshold_ : 0.0f;
  const float limit = std::min(max_extent_, available_extent_);
  const float minimum = std::min(min_extent_, limit);
  elastic_offset_ = 0.0f;
  if (!dragging_) release_offset_ = 0.0f;
  event.request_paint();

  if (drag_phase_ == DragPhase::Pending) {
    // After a snap, approaching an edge consumes only the gap. Moving away
    // from it can load the opposite threshold immediately, from the latest
    // turning point. A reversal never reuses accumulated travel in the other
    // direction, and an unchanged release sample cannot undo the snap.
    const float origin = direction > 0
                             ? std::max(drag_direction_origin_, drag_edge_anchor_)
                             : std::min(drag_direction_origin_, drag_edge_anchor_);
    const float distance = static_cast<float>(direction) * (pointer - origin);
    if (distance <= 0.0f || (!open_ && direction < 0)) return;
    if (distance < threshold) {
      elastic_offset_ = std::clamp(static_cast<float>(direction) * distance * 0.45f,
                                   -80.0f, 80.0f);
      if (!dragging_) release_offset_ = elastic_offset_;
      return;
    }
    if (!open_) {
      const float restored = std::clamp(extent_, min_extent_, max_extent_);
      arm_drag_(pointer, std::min(restored, available_extent_));
      // Opening consumes this sample, including any overshoot. Never turn a
      // single coarse mouse event into opening followed by an unwanted resize.
      change_(true, restored, event);
      return;
    }
    drag_phase_ = DragPhase::Resizing;
    drag_resize_extent_ += static_cast<float>(direction) * (distance - threshold);
  } else {
    // Once picked up, the edge follows both directions without another
    // dead zone. Only a discontinuous open/close starts a new pending phase.
    drag_resize_extent_ += delta;
  }

  const float collapse_distance = elastic_collapse ? collapse_threshold_ : 0.0f;
  if (direction < 0 && drag_resize_extent_ <= minimum - collapse_distance) {
    const float closed = std::min({collapsed_extent_, min_extent_, available_extent_});
    arm_drag_(pointer, closed);
    change_(false, drag_reopen_extent_, event);
    return;
  }
  const float clamped = std::clamp(drag_resize_extent_, minimum, limit);
  // Resistance remains bounded even if the captured pointer leaves the window.
  if ((drag_resize_extent_ < minimum && elastic_collapse) ||
      (drag_resize_extent_ > limit && elastic_resize))
    elastic_offset_ = std::clamp((drag_resize_extent_ - clamped) * 0.35f, -80.0f, 80.0f);
  // An immediate boundary must not accumulate invisible overshoot: reversing
  // at the maximum should resize on the very next pointer sample.
  if (drag_resize_extent_ > limit && !elastic_resize)
    drag_resize_extent_ = limit;
  if (!dragging_) release_offset_ = elastic_offset_;
  change_(true, std::max(clamped, min_extent_), event);
}

void Sidebar::finish_drag_(Event &event) {
  dragging_ = false;
  release_offset_ = elastic_offset_;
  release_time_.reset();
  event.request_paint();
}

EventResult Sidebar::handle_event_(Event &event) {
  if (drag_mode_ == SidebarDragMode::Disabled) return EventResult::Unhandled;
  if (auto *press = dynamic_cast<MousePressedEvent *>(&event)) {
    if (press->button() != MouseButton::Left) return EventResult::Unhandled;
    dragging_ = true;
    drag_start_open_ = open_;
    drag_restore_extent_ = extent_;
    drag_reopen_extent_ = extent_;
    const float pointer = axis_(press->get_pos()) * direction_();
    // Keep the original grab offset within the handle through size jumps.
    drag_edge_offset_ = pointer - visible_extent_;
    arm_drag_(pointer, visible_extent_);
    elastic_offset_ = release_offset_ = 0.0f;
    release_time_.reset();
    event.request_paint();
    return EventResult::Handled;
  }
  if (auto *move = dynamic_cast<MouseMovedEvent *>(&event); move && dragging_) {
    move_drag_(move->get_pos(), event);
    return EventResult::Handled;
  }
  if (auto *release = dynamic_cast<MouseReleasedEvent *>(&event);
      release && release->button() == MouseButton::Left && dragging_) {
    // Finish before invoking change callbacks; the final pointer sample matters.
    finish_drag_(event);
    move_drag_(release->get_pos(), event);
    return EventResult::Handled;
  }
  if (event.type() == EventType::WindowFocusLost && dragging_) {
    finish_drag_(event);
    return EventResult::Handled;
  }
  if (auto *key = dynamic_cast<KeyPressedEvent *>(&event)) {
    if (key->keycode() == Keycode::Escape && dragging_) {
      finish_drag_(event);
      change_(drag_start_open_, drag_restore_extent_, event);
      return EventResult::Handled;
    }
    if (key->keycode() == Keycode::Return || key->keycode() == Keycode::Space) {
      finish_drag_(event);
      change_(!open_, extent_, event);
      return EventResult::Handled;
    }
    float delta = 0;
    if (key->keycode() == (horizontal_() ? Keycode::Left : Keycode::Up)) delta = -16;
    if (key->keycode() == (horizontal_() ? Keycode::Right : Keycode::Down)) delta = 16;
    if (delta != 0) {
      finish_drag_(event);
      delta *= direction_();
      const float target = extent_ + delta;
      change_(open_ ? target >= min_extent_ : delta > 0,
              open_ && target >= min_extent_ ? target : extent_, event);
      return EventResult::Handled;
    }
  }
  return EventResult::Unhandled;
}

} // namespace voidui
