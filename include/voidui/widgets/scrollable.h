#pragma once

#include <algorithm>
#include <cmath>
#include <limits>
#include <memory>
#include <string_view>
#include <utility>

#include "voidui/core/border.h"
#include "voidui/core/context.h"
#include "voidui/core/style.h"
#include "voidui/core/widget.h"

namespace voidui {

enum class ScrollAxis { Vertical, Horizontal, Both };

/// Controls scrollbar visibility without disabling scrolling on that axis.
enum class ScrollbarVisibility { Auto, Always, Never };

inline bool parse_style_value(std::string_view text, ScrollbarVisibility &out) {
  text = style_trim(text);
  if (text == "auto") {
    out = ScrollbarVisibility::Auto;
    return true;
  }
  if (text == "always" || text == "scroll") {
    out = ScrollbarVisibility::Always;
    return true;
  }
  if (text == "never" || text == "hidden") {
    out = ScrollbarVisibility::Never;
    return true;
  }
  return false;
}

/// A clipped viewport that scrolls one child vertically, horizontally, or on
/// both axes. Mouse wheels, track clicks, and draggable thumbs are supported.
class Scrollable : public Widget {
public:
  VOIDUI_STYLE_SCOPE(Scrollable, "scrollable")

  VOIDUI_STYLE_PROPERTY(Scrollable, HorizontalScrollbar, ScrollbarVisibility,
                        "horizontal-scrollbar", NotInherited, Layout,
                        ScrollbarVisibility::Auto);
  VOIDUI_STYLE_PROPERTY(Scrollable, VerticalScrollbar, ScrollbarVisibility,
                        "vertical-scrollbar", NotInherited, Layout,
                        ScrollbarVisibility::Auto);
  VOIDUI_STYLE_PROPERTY(Scrollable, ScrollbarTrack, Brush, "scrollbar-track",
                        NotInherited, Paint, Color(0, 0, 0, 24));
  VOIDUI_STYLE_PROPERTY(Scrollable, ScrollbarThumb, Brush, "scrollbar-thumb",
                        NotInherited, Paint, Color(0, 0, 0, 96));
  VOIDUI_STYLE_PROPERTY(Scrollable, ScrollbarThickness, float,
                        "scrollbar-thickness", NotInherited, Layout, 10.0f);
  VOIDUI_STYLE_PROPERTY(Scrollable, ScrollbarMinThumbSize, float,
                        "scrollbar-min-thumb-size", NotInherited, Layout,
                        24.0f);
  VOIDUI_STYLE_PROPERTY(Scrollable, ScrollbarHitSlop, float,
                        "scrollbar-hit-slop", NotInherited, Layout, 4.0f);

  Scrollable() = default;

  template <WidgetClass T>
  explicit Scrollable(T &&content)
      : content_(transfer_widget(std::forward<T>(content))) {}

  explicit Scrollable(std::unique_ptr<Widget> content)
      : content_(std::move(content)) {}

  VOIDUI_WIDGET_SIZE_STYLE
  VOIDUI_FLUENT_SETTER(axis, axis_, ScrollAxis)
  VOIDUI_FLUENT_METHOD(horizontal_scrollbar, (ScrollbarVisibility value),
                       set_style<HorizontalScrollbar>(value);)
  VOIDUI_FLUENT_METHOD(vertical_scrollbar, (ScrollbarVisibility value),
                       set_style<VerticalScrollbar>(value);)
  VOIDUI_FLUENT_METHOD(scrollbar_track, (Brush value),
                       set_style<ScrollbarTrack>(std::move(value));)
  VOIDUI_FLUENT_METHOD(scrollbar_thumb, (Brush value),
                       set_style<ScrollbarThumb>(std::move(value));)
  VOIDUI_FLUENT_METHOD(scrollbar_thickness, (float value),
                       set_style<ScrollbarThickness>(std::max(value, 0.0f));)
  VOIDUI_FLUENT_METHOD(scrollbar_min_thumb_size, (float value),
                       set_style<ScrollbarMinThumbSize>(std::max(value, 0.0f));)
  VOIDUI_FLUENT_METHOD(scrollbar_hit_slop, (float value),
                       set_style<ScrollbarHitSlop>(std::max(value, 0.0f));)
  VOIDUI_FLUENT_METHOD(scroll_step, (float value),
                       scroll_step_ = std::max(value, 0.0f);)
  VOIDUI_FLUENT_METHOD(scroll_to, (Point<float> value),
                       scroll_offset_ = {std::max(value.x, 0.0f),
                                         std::max(value.y, 0.0f)};
                       scroll_offset_set_ = true;)

  VOIDUI_FLUENT_METHOD(background, (Brush value),
                       set_style<styles::Background>(std::move(value));)
  VOIDUI_FLUENT_METHOD(padding, (Padding value),
                       set_style<styles::Padding>(value);)
  VOIDUI_FLUENT_METHOD(
      border, (Border value),
      set_style<styles::BorderRadius>(value.get_radius());
      set_style<styles::BorderWidth>(value.get_width());
      set_style<styles::BorderColor>(value.get_brush());)

  Point<float> scroll_offset() const { return scroll_offset_; }
  Size<float> content_size() const { return content_size_; }
  Size<float> viewport_size() const { return viewport_size_; }
  Point<float> max_scroll_offset() const { return max_scroll_; }
  Rect<float> horizontal_scrollbar_thumb() const { return horizontal_thumb_; }
  Rect<float> vertical_scrollbar_thumb() const { return vertical_thumb_; }
  bool horizontal_scrollbar_visible() const { return show_horizontal_; }
  bool vertical_scrollbar_visible() const { return show_vertical_; }

  std::unique_ptr<Widget> clone() const override {
    auto copy = std::make_unique<Scrollable>(content_ ? clone_widget(*content_)
                                                      : nullptr);
    copy->axis_ = axis_;
    copy->scroll_offset_ = scroll_offset_;
    copy->scroll_offset_set_ = scroll_offset_set_;
    copy->scroll_step_ = scroll_step_;
    return copy;
  }

  void inherit_runtime(const Widget &previous) override {
    const auto &scrollable = static_cast<const Scrollable &>(previous);
    if (!scroll_offset_set_)
      scroll_offset_ = scrollable.scroll_offset_;
    max_scroll_ = scrollable.max_scroll_;
    content_size_ = scrollable.content_size_;
    viewport_origin_ = scrollable.viewport_origin_;
    viewport_size_ = scrollable.viewport_size_;
    show_horizontal_ = scrollable.show_horizontal_;
    show_vertical_ = scrollable.show_vertical_;
    has_layout_ = scrollable.has_layout_;
    scrollbar_thickness_ = scrollable.scrollbar_thickness_;
    min_thumb_size_ = scrollable.min_thumb_size_;
    hit_slop_ = scrollable.hit_slop_;
    horizontal_track_ = scrollable.horizontal_track_;
    horizontal_thumb_ = scrollable.horizontal_thumb_;
    vertical_track_ = scrollable.vertical_track_;
    vertical_thumb_ = scrollable.vertical_thumb_;
    last_global_pos_ = scrollable.last_global_pos_;
    drag_axis_ = scrollable.drag_axis_;
    drag_start_pointer_ = scrollable.drag_start_pointer_;
    drag_start_offset_ = scrollable.drag_start_offset_;
    pointer_interaction_ = scrollable.pointer_interaction_;
  }

  void register_children(Registrar &registrar) override {
    if (content_)
      registrar.take_child(std::move(content_));
  }

  Size<float> layout(Constraints constraints, LayoutContext &ctx) override {
    const float border_width =
        std::max(ctx.style.get<styles::BorderWidth>(), 0.0f);
    const Spacing<float> chrome =
        ctx.style.get<styles::Padding>() + Spacing<float>(border_width);
    const float horizontal_chrome = chrome.left + chrome.right;
    const float vertical_chrome = chrome.top + chrome.bottom;
    const Size<Length> &size = ctx.style.layout_size();
    const float thickness = std::max(ctx.style.get<ScrollbarThickness>(), 0.0f);
    const ScrollbarVisibility horizontal_policy =
        ctx.style.get<HorizontalScrollbar>();
    const ScrollbarVisibility vertical_policy =
        ctx.style.get<VerticalScrollbar>();

    const bool scroll_horizontal = scrolls_horizontal_();
    const bool scroll_vertical = scrolls_vertical_();
    show_horizontal_ =
        scroll_horizontal &&
        (horizontal_policy == ScrollbarVisibility::Always ||
         (has_layout_ && horizontal_policy == ScrollbarVisibility::Auto &&
          show_horizontal_));
    show_vertical_ =
        scroll_vertical &&
        (vertical_policy == ScrollbarVisibility::Always ||
         (has_layout_ && vertical_policy == ScrollbarVisibility::Auto &&
          show_vertical_));
    hit_slop_ = std::max(ctx.style.get<ScrollbarHitSlop>(), 0.0f);

    const float available_outer_width = available_axis_(
        size.width, constraints.min_width, constraints.max_width);
    const float available_outer_height = available_axis_(
        size.height, constraints.min_height, constraints.max_height);

    Size<float> measured = measure_child_(
        ctx,
        subtract_chrome_(available_outer_width,
                         horizontal_chrome +
                             (show_vertical_ ? thickness : 0.0f)),
        subtract_chrome_(available_outer_height,
                         vertical_chrome +
                             (show_horizontal_ ? thickness : 0.0f)));
    Size<float> result;
    bool visibility_changed = false;

    // Auto bars affect the opposite axis. A vertical bar can make horizontal
    // content overflow and vice versa, so resolve the small two-state system
    // until both visibility and the measured content stop changing.
    for (int pass = 0; pass < 6; ++pass) {
      const Size<float> intrinsic{measured.width + horizontal_chrome +
                                      (show_vertical_ ? thickness : 0.0f),
                                  measured.height + vertical_chrome +
                                      (show_horizontal_ ? thickness : 0.0f)};
      result = constraints.resolve(size, intrinsic);

      const Size<float> viewport{
          subtract_chrome_(result.width,
                           horizontal_chrome +
                               (show_vertical_ ? thickness : 0.0f)),
          subtract_chrome_(result.height,
                           vertical_chrome +
                               (show_horizontal_ ? thickness : 0.0f))};
      if (visibility_changed)
        measured = measure_child_(ctx, viewport.width, viewport.height);

      const bool next_horizontal =
          scroll_horizontal &&
          (horizontal_policy == ScrollbarVisibility::Always ||
           (horizontal_policy == ScrollbarVisibility::Auto &&
            measured.width > viewport.width + kOverflowEpsilon));
      const bool next_vertical =
          scroll_vertical &&
          (vertical_policy == ScrollbarVisibility::Always ||
           (vertical_policy == ScrollbarVisibility::Auto &&
            measured.height > viewport.height + kOverflowEpsilon));
      if (next_horizontal == show_horizontal_ &&
          next_vertical == show_vertical_)
        break;

      show_horizontal_ = next_horizontal;
      show_vertical_ = next_vertical;
      visibility_changed = true;
    }

    const Size<float> intrinsic{measured.width + horizontal_chrome +
                                    (show_vertical_ ? thickness : 0.0f),
                                measured.height + vertical_chrome +
                                    (show_horizontal_ ? thickness : 0.0f)};
    result = constraints.resolve(size, intrinsic);
    viewport_origin_ = {chrome.left, chrome.top};
    viewport_size_ = {
        subtract_chrome_(result.width, horizontal_chrome +
                                           (show_vertical_ ? thickness : 0.0f)),
        subtract_chrome_(result.height,
                         vertical_chrome +
                             (show_horizontal_ ? thickness : 0.0f))};
    content_size_ = measured;
    has_layout_ = true;

    max_scroll_ = {
        scroll_horizontal
            ? std::max(content_size_.width - viewport_size_.width, 0.0f)
            : 0.0f,
        scroll_vertical
            ? std::max(content_size_.height - viewport_size_.height, 0.0f)
            : 0.0f};
    clamp_offset_();
    scrollbar_thickness_ = thickness;
    min_thumb_size_ = std::max(ctx.style.get<ScrollbarMinThumbSize>(), 0.0f);
    update_scrollbar_geometry_();

    if (ctx.child_count() > 0) {
      const Spacing<MarginValue> &margin = ctx.child_margin(0);
      const std::size_t horizontal_auto_count =
          static_cast<std::size_t>(margin.left.is_auto()) +
          static_cast<std::size_t>(margin.right.is_auto());
      const std::size_t vertical_auto_count =
          static_cast<std::size_t>(margin.top.is_auto()) +
          static_cast<std::size_t>(margin.bottom.is_auto());
      const float horizontal_free =
          std::max(viewport_size_.width - measured.width, 0.0f);
      const float vertical_free =
          std::max(viewport_size_.height - measured.height, 0.0f);
      Spacing<float> auto_margin;
      if (margin.left.is_auto())
        auto_margin.left =
            horizontal_free / static_cast<float>(horizontal_auto_count);
      if (margin.right.is_auto())
        auto_margin.right =
            horizontal_free / static_cast<float>(horizontal_auto_count);
      if (margin.top.is_auto())
        auto_margin.top =
            vertical_free / static_cast<float>(vertical_auto_count);
      if (margin.bottom.is_auto())
        auto_margin.bottom =
            vertical_free / static_cast<float>(vertical_auto_count);
      ctx.place_child(0,
                      Point<float>(viewport_origin_.x - scroll_offset_.x,
                                   viewport_origin_.y - scroll_offset_.y),
                      auto_margin);
    }

    return result;
  }

  void draw(const DrawContext &ctx, Painter &painter) override {
    last_global_pos_ = ctx.bounds.origin;
    const Radius radius = ctx.style.get<styles::BorderRadius>();
    painter.fill_rrect(ctx.bounds, radius,
                       Paint(ctx.style.get<styles::Background>()));

    const float border_width = ctx.style.get<styles::BorderWidth>();
    if (border_width > 0.0f) {
      painter.stroke_rrect(ctx.bounds, radius,
                           Paint(ctx.style.get<styles::BorderColor>()),
                           Pen(border_width, StrokeAlign::Inside));
    }
  }

  void draw_foreground(const DrawContext &ctx, Painter &painter) override {
    last_global_pos_ = ctx.bounds.origin;
    const Paint track(ctx.style.get<ScrollbarTrack>());
    const Paint thumb(ctx.style.get<ScrollbarThumb>());
    const float radius =
        std::max(ctx.style.get<ScrollbarThickness>(), 0.0f) * 0.5f;

    if (show_horizontal_) {
      painter.fill_rrect(to_global_(horizontal_track_, ctx.bounds.origin),
                         Radius(radius), track);
      painter.fill_rrect(to_global_(horizontal_thumb_, ctx.bounds.origin),
                         Radius(radius), thumb);
    }
    if (show_vertical_) {
      painter.fill_rrect(to_global_(vertical_track_, ctx.bounds.origin),
                         Radius(radius), track);
      painter.fill_rrect(to_global_(vertical_thumb_, ctx.bounds.origin),
                         Radius(radius), thumb);
    }
    if (show_horizontal_ && show_vertical_) {
      painter.fill_rect(
          Rect<float>(ctx.bounds.origin.x + vertical_track_.origin.x,
                      ctx.bounds.origin.y + horizontal_track_.origin.y,
                      vertical_track_.size.width,
                      horizontal_track_.size.height),
          track);
    }
  }

  EventResult on_event(Event &event) override {
    if (auto result = event.dispatch<MouseScrolledEvent>(
            [&](MouseScrolledEvent &e) { return on_scroll_(e); });
        result == EventResult::Handled)
      return result;

    if (auto result = event.dispatch<MousePressedEvent>(
            [&](MousePressedEvent &e) { return on_press_(e); });
        result == EventResult::Handled)
      return result;

    if (auto result = event.dispatch<MouseMovedEvent>(
            [&](MouseMovedEvent &e) { return on_move_(e); });
        result == EventResult::Handled)
      return result;

    return event.dispatch<MouseReleasedEvent>(
        [&](MouseReleasedEvent &e) { return on_release_(e); });
  }

  bool clips_children() const override { return true; }

  std::optional<Rect<float>> selection_scroll_viewport(Rect<float> bounds) const override {
    return children_clip(bounds);
  }

  Invalidation selection_scroll_by(Point<float> delta) override {
    const auto before = scroll_offset_;
    scroll_offset_.x += delta.x;
    scroll_offset_.y += delta.y;
    clamp_offset_();
    update_scrollbar_geometry_();
    return before.x != scroll_offset_.x || before.y != scroll_offset_.y
               ? Invalidation::Layout : Invalidation::None;
  }

  Rect<float> children_clip(Rect<float> bounds) const override {
    return Rect<float>(bounds.origin.x + viewport_origin_.x,
                       bounds.origin.y + viewport_origin_.y,
                       viewport_size_.width, viewport_size_.height);
  }

  bool foreground_hit_test(Point<float> point,
                           Rect<float> bounds) const override {
    if (show_horizontal_ &&
        to_global_(expanded_hit_rect_(horizontal_track_, hit_slop_, true),
                   bounds.origin)
            .contains(point))
      return true;
    return show_vertical_ &&
           to_global_(expanded_hit_rect_(vertical_track_, hit_slop_, false),
                      bounds.origin)
               .contains(point);
  }

private:
  enum class DragAxis { None, Horizontal, Vertical };

  static constexpr float kOverflowEpsilon = 0.01f;

  bool scrolls_horizontal_() const {
    return axis_ == ScrollAxis::Horizontal || axis_ == ScrollAxis::Both;
  }

  bool scrolls_vertical_() const {
    return axis_ == ScrollAxis::Vertical || axis_ == ScrollAxis::Both;
  }

  Size<float> measure_child_(LayoutContext &ctx, float viewport_width,
                             float viewport_height) const {
    if (ctx.child_count() == 0)
      return {};

    return ctx.constrain_child(
        0, Constraints{
               0.0f,
               scrolls_horizontal_() ? std::numeric_limits<float>::infinity()
                                     : viewport_width,
               0.0f,
               scrolls_vertical_() ? std::numeric_limits<float>::infinity()
                                   : viewport_height});
  }

  static float available_axis_(const Length &length, float min, float max) {
    if (const auto *fixed = std::get_if<Length::Fixed>(&length.value))
      return std::clamp(fixed->value, min, max);
    return max;
  }

  static float subtract_chrome_(float available, float chrome) {
    return std::isfinite(available) ? std::max(available - chrome, 0.0f)
                                    : available;
  }

  static bool close_(float lhs, float rhs) {
    return std::abs(lhs - rhs) < kOverflowEpsilon;
  }

  void clamp_offset_() {
    scroll_offset_.x = std::clamp(scroll_offset_.x, 0.0f, max_scroll_.x);
    scroll_offset_.y = std::clamp(scroll_offset_.y, 0.0f, max_scroll_.y);
  }

  static Rect<float> to_global_(Rect<float> local, Point<float> origin) {
    local.origin.x += origin.x;
    local.origin.y += origin.y;
    return local;
  }

  static Rect<float> expanded_hit_rect_(Rect<float> rect, float amount,
                                        bool horizontal) {
    if (horizontal) {
      rect.origin.y -= amount;
      rect.size.height += amount * 2.0f;
    } else {
      rect.origin.x -= amount;
      rect.size.width += amount * 2.0f;
    }
    return rect;
  }

  void update_scrollbar_geometry_() {
    horizontal_track_ = {};
    horizontal_thumb_ = {};
    vertical_track_ = {};
    vertical_thumb_ = {};
    const float thickness = scrollbar_thickness_;
    const float min_thumb = min_thumb_size_;

    if (show_horizontal_) {
      horizontal_track_ = Rect<float>(
          viewport_origin_.x, viewport_origin_.y + viewport_size_.height,
          viewport_size_.width, thickness);
      const float track = horizontal_track_.size.width;
      const float ratio = content_size_.width > 0.0f
                              ? viewport_size_.width / content_size_.width
                              : 1.0f;
      const float thumb =
          std::clamp(track * ratio, std::min(min_thumb, track), track);
      const float travel = std::max(track - thumb, 0.0f);
      const float position = max_scroll_.x > 0.0f
                                 ? travel * scroll_offset_.x / max_scroll_.x
                                 : 0.0f;
      horizontal_thumb_ =
          Rect<float>(horizontal_track_.origin.x + position,
                      horizontal_track_.origin.y, thumb, thickness);
    }

    if (show_vertical_) {
      vertical_track_ =
          Rect<float>(viewport_origin_.x + viewport_size_.width,
                      viewport_origin_.y, thickness, viewport_size_.height);
      const float track = vertical_track_.size.height;
      const float ratio = content_size_.height > 0.0f
                              ? viewport_size_.height / content_size_.height
                              : 1.0f;
      const float thumb =
          std::clamp(track * ratio, std::min(min_thumb, track), track);
      const float travel = std::max(track - thumb, 0.0f);
      const float position = max_scroll_.y > 0.0f
                                 ? travel * scroll_offset_.y / max_scroll_.y
                                 : 0.0f;
      vertical_thumb_ =
          Rect<float>(vertical_track_.origin.x,
                      vertical_track_.origin.y + position, thickness, thumb);
    }
  }

  void jump_horizontal_thumb_to_(float pointer) {
    const float travel =
        horizontal_track_.size.width - horizontal_thumb_.size.width;
    if (travel > 0.0f && max_scroll_.x > 0.0f) {
      const float thumb_position =
          std::clamp(pointer - horizontal_track_.origin.x -
                         horizontal_thumb_.size.width * 0.5f,
                     0.0f, travel);
      scroll_offset_.x = thumb_position * max_scroll_.x / travel;
    }
    clamp_offset_();
    update_scrollbar_geometry_();
  }

  void jump_vertical_thumb_to_(float pointer) {
    const float travel =
        vertical_track_.size.height - vertical_thumb_.size.height;
    if (travel > 0.0f && max_scroll_.y > 0.0f) {
      const float thumb_position =
          std::clamp(pointer - vertical_track_.origin.y -
                         vertical_thumb_.size.height * 0.5f,
                     0.0f, travel);
      scroll_offset_.y = thumb_position * max_scroll_.y / travel;
    }
    clamp_offset_();
    update_scrollbar_geometry_();
  }

  EventResult on_scroll_(MouseScrolledEvent &event) {
    const Point<float> before = scroll_offset_;
    if (scrolls_vertical_() && max_scroll_.y > 0.0f)
      scroll_offset_.y -= event.y_offset() * scroll_step_;
    if (scrolls_horizontal_() && max_scroll_.x > 0.0f) {
      float delta = event.x_offset();
      if (delta == 0.0f && (!scrolls_vertical_() || max_scroll_.y <= 0.0f))
        delta = event.y_offset();
      scroll_offset_.x -= delta * scroll_step_;
    }
    clamp_offset_();
    update_scrollbar_geometry_();
    if (close_(before.x, scroll_offset_.x) &&
        close_(before.y, scroll_offset_.y))
      return EventResult::Unhandled;
    event.request_layout();
    return EventResult::Handled;
  }

  EventResult on_press_(MousePressedEvent &event) {
    if (event.button() != MouseButton::Left)
      return EventResult::Unhandled;

    const Point<float> point = event.get_pos();
    const Rect<float> horizontal_thumb =
        to_global_(expanded_hit_rect_(horizontal_thumb_, hit_slop_, true),
                   last_global_pos_);
    const Rect<float> vertical_thumb =
        to_global_(expanded_hit_rect_(vertical_thumb_, hit_slop_, false),
                   last_global_pos_);

    if (show_horizontal_ && horizontal_thumb.contains(point)) {
      drag_axis_ = DragAxis::Horizontal;
      drag_start_pointer_ = point.x;
      drag_start_offset_ = scroll_offset_.x;
      pointer_interaction_ = true;
      return EventResult::Handled;
    }
    if (show_vertical_ && vertical_thumb.contains(point)) {
      drag_axis_ = DragAxis::Vertical;
      drag_start_pointer_ = point.y;
      drag_start_offset_ = scroll_offset_.y;
      pointer_interaction_ = true;
      return EventResult::Handled;
    }

    const Rect<float> horizontal_track =
        to_global_(expanded_hit_rect_(horizontal_track_, hit_slop_, true),
                   last_global_pos_);
    if (show_horizontal_ && horizontal_track.contains(point)) {
      jump_horizontal_thumb_to_(point.x - last_global_pos_.x);
      event.request_layout();
      drag_axis_ = DragAxis::Horizontal;
      drag_start_pointer_ = point.x;
      drag_start_offset_ = scroll_offset_.x;
      pointer_interaction_ = true;
      return EventResult::Handled;
    }
    const Rect<float> vertical_track =
        to_global_(expanded_hit_rect_(vertical_track_, hit_slop_, false),
                   last_global_pos_);
    if (show_vertical_ && vertical_track.contains(point)) {
      jump_vertical_thumb_to_(point.y - last_global_pos_.y);
      event.request_layout();
      drag_axis_ = DragAxis::Vertical;
      drag_start_pointer_ = point.y;
      drag_start_offset_ = scroll_offset_.y;
      pointer_interaction_ = true;
      return EventResult::Handled;
    }
    return EventResult::Unhandled;
  }

  EventResult on_move_(MouseMovedEvent &event) {
    if (drag_axis_ == DragAxis::None)
      return EventResult::Unhandled;

    const Point<float> before = scroll_offset_;
    if (drag_axis_ == DragAxis::Horizontal) {
      const float travel =
          horizontal_track_.size.width - horizontal_thumb_.size.width;
      if (travel > 0.0f) {
        scroll_offset_.x =
            drag_start_offset_ +
            (event.get_pos().x - drag_start_pointer_) * max_scroll_.x / travel;
      }
    } else {
      const float travel =
          vertical_track_.size.height - vertical_thumb_.size.height;
      if (travel > 0.0f) {
        scroll_offset_.y =
            drag_start_offset_ +
            (event.get_pos().y - drag_start_pointer_) * max_scroll_.y / travel;
      }
    }
    clamp_offset_();
    update_scrollbar_geometry_();
    if (!close_(before.x, scroll_offset_.x) ||
        !close_(before.y, scroll_offset_.y))
      event.request_layout();
    return EventResult::Handled;
  }

  EventResult on_release_(MouseReleasedEvent &event) {
    if (event.button() != MouseButton::Left || !pointer_interaction_)
      return EventResult::Unhandled;
    drag_axis_ = DragAxis::None;
    pointer_interaction_ = false;
    return EventResult::Handled;
  }

  std::unique_ptr<Widget> content_;
  ScrollAxis axis_ = ScrollAxis::Vertical;
  Point<float> scroll_offset_;
  bool scroll_offset_set_ = false;
  Point<float> max_scroll_;
  float scroll_step_ = 40.0f;

  Size<float> content_size_;
  Point<float> viewport_origin_;
  Size<float> viewport_size_;
  bool show_horizontal_ = false;
  bool show_vertical_ = false;
  bool has_layout_ = false;
  float scrollbar_thickness_ = 0.0f;
  float min_thumb_size_ = 0.0f;
  float hit_slop_ = 0.0f;
  Rect<float> horizontal_track_;
  Rect<float> horizontal_thumb_;
  Rect<float> vertical_track_;
  Rect<float> vertical_thumb_;
  Point<float> last_global_pos_;

  DragAxis drag_axis_ = DragAxis::None;
  float drag_start_pointer_ = 0.0f;
  float drag_start_offset_ = 0.0f;
  bool pointer_interaction_ = false;
};

[[nodiscard]] inline Scrollable scrollable() { return Scrollable{}; }

template <WidgetClass T> [[nodiscard]] Scrollable scrollable(T &&content) {
  return Scrollable(std::forward<T>(content));
}

} // namespace voidui
