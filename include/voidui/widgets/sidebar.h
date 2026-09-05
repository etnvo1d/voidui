#pragma once

#include <algorithm>
#include <cmath>
#include <functional>
#include <memory>
#include <optional>

#include "voidui/core/context.h"
#include "voidui/core/state.h"

namespace voidui {

enum class SidebarPlacement { Left, Right, Top, Bottom };
enum class SidebarMode { Docked, Overlay };
enum class SidebarDragMode { Disabled, Immediate, Elastic, ElasticOpenClose };
enum class SidebarDragBehavior { Immediate, Elastic };

/// A panel and a main-content viewport. Nest sidebars to combine edges.
/// Extents are logical pixels, excluding the drag handle. State bindings are
/// bidirectional; constant declarations act as defaults until changed.
class Sidebar : public Widget {
public:
  VOIDUI_STYLE_SCOPE(Sidebar, "sidebar")
  VOIDUI_STYLE_PROPERTY(Sidebar, HandleColor, Brush, "handle-color",
                        NotInherited, Paint, Color(110, 120, 130, 100));
  VOIDUI_STYLE_PROPERTY(Sidebar, ElasticColor, Brush, "elastic-color",
                        NotInherited, Paint, Color(30, 195, 190));

  template <WidgetClass Panel, WidgetClass Content>
  Sidebar(Panel &&panel, Content &&content)
      : Sidebar(transfer_widget(std::forward<Panel>(panel)),
                transfer_widget(std::forward<Content>(content))) {}
  Sidebar(std::unique_ptr<Widget> panel, std::unique_ptr<Widget> content);

  VOIDUI_WIDGET_SIZE_STYLE
  VOIDUI_FLUENT_SETTER(placement, placement_, SidebarPlacement)
  VOIDUI_FLUENT_SETTER(mode, mode_, SidebarMode)
  VOIDUI_FLUENT_SETTER(drag_mode, drag_mode_, SidebarDragMode)
  /// Optional per-phase overrides of drag_mode's preset. Disabled still wins.
  VOIDUI_FLUENT_SETTER(open_behavior, open_behavior_, SidebarDragBehavior)
  VOIDUI_FLUENT_SETTER(resize_behavior, resize_behavior_, SidebarDragBehavior)
  VOIDUI_FLUENT_SETTER(collapse_behavior, collapse_behavior_, SidebarDragBehavior)
  /// Debug aid: show both the resting edge and the elastic curve. This only
  /// affects painting; the invisible drag target remains usable by default.
  VOIDUI_FLUENT_SETTER(edge_visible, edge_visible_, bool)
  VOIDUI_FLUENT_METHOD(open, (bool value),
                       open_ = declared_open_ = value; open_explicit_ = true;)
  VOIDUI_FLUENT_METHOD(open, (State<bool> state),
                       open_ = declared_open_ = state.get();
                       open_explicit_ = true; open_binding_ = state;)
  VOIDUI_FLUENT_METHOD(extent, (float value),
                       extent_ = declared_extent_ = finite_nonnegative_(value);
                       extent_explicit_ = true;)
  VOIDUI_FLUENT_METHOD(extent, (State<float> state),
                       extent_ = declared_extent_ = finite_nonnegative_(state.get());
                       extent_explicit_ = true; extent_binding_ = state;)
  VOIDUI_FLUENT_METHOD(limits, (float minimum, float maximum),
                       min_extent_ = finite_nonnegative_(minimum);
                       max_extent_ = std::max(min_extent_, finite_nonnegative_(maximum));)
  VOIDUI_FLUENT_METHOD(collapsed_extent, (float value),
                       collapsed_extent_ = finite_nonnegative_(value);)
  VOIDUI_FLUENT_METHOD(handle_size, (float value),
                       handle_size_ = finite_nonnegative_(value);)
  VOIDUI_FLUENT_METHOD(drag_threshold, (float value),
                       drag_threshold_ = finite_nonnegative_(value);)
  VOIDUI_FLUENT_METHOD(collapse_threshold, (float value),
                       collapse_threshold_ = finite_nonnegative_(value);)
  VOIDUI_FLUENT_METHOD(background, (Brush value),
                       set_style<styles::Background>(std::move(value));)
  VOIDUI_FLUENT_METHOD(handle_color, (Brush value),
                       set_style<HandleColor>(std::move(value));)
  VOIDUI_FLUENT_METHOD(elastic_color, (Brush value),
                       set_style<ElasticColor>(std::move(value));)
  VOIDUI_FLUENT_SETTER(on_open_change, on_open_change_, std::function<void(bool)>)
  VOIDUI_FLUENT_SETTER(on_extent_change, on_extent_change_, std::function<void(float)>)

  bool is_open() const { return open_; }
  float expanded_extent() const { return extent_; }
  float visible_extent() const { return visible_extent_; }
  bool is_dragging() const { return dragging_; }
  float elastic_offset() const { return elastic_offset_; }
  Rect<float> panel_bounds() const { return panel_rect_; }
  Rect<float> content_bounds() const { return content_rect_; }
  Rect<float> handle_bounds() const { return handle_rect_; }

  std::unique_ptr<Widget> clone() const override;
  void inherit_runtime(const Widget &previous) override;
  void register_children(Registrar &registrar) override;
  Size<float> layout(Constraints constraints, LayoutContext &ctx) override;
  void draw(const DrawContext &ctx, Painter &painter) override;
  void draw_foreground(const DrawContext &ctx, Painter &painter) override;
  EventResult on_event(Event &) override { return EventResult::Unhandled; }
  bool clips_children() const override { return true; }

private:
  static float finite_nonnegative_(float value) {
    return std::isfinite(value) ? std::max(value, 0.0f) : 0.0f;
  }
  bool horizontal_() const;
  float direction_() const;
  float axis_(Point<float> point) const;
  bool elastic_(const std::optional<SidebarDragBehavior> &behavior,
                bool resizing = false) const;
  EventResult handle_event_(Event &event);
  void arm_drag_(float pointer, float visible_extent);
  void move_drag_(Point<float> point, Event &event);
  void change_(bool open, float extent, Event &event);
  void finish_drag_(Event &event);

  std::unique_ptr<Widget> panel_, content_;
  std::shared_ptr<bool> panel_visible_ = std::make_shared<bool>(true);
  SidebarPlacement placement_ = SidebarPlacement::Left;
  SidebarMode mode_ = SidebarMode::Docked;
  SidebarDragMode drag_mode_ = SidebarDragMode::Immediate;
  std::optional<SidebarDragBehavior> open_behavior_, resize_behavior_, collapse_behavior_;
  bool edge_visible_ = false;
  bool open_ = true, declared_open_ = true, open_explicit_ = false;
  float extent_ = 280.0f, declared_extent_ = 280.0f;
  bool extent_explicit_ = false;
  float min_extent_ = 160.0f, max_extent_ = 560.0f;
  float collapsed_extent_ = 0.0f, handle_size_ = 8.0f;
  float drag_threshold_ = 48.0f, collapse_threshold_ = 48.0f;
  std::optional<State<bool>> open_binding_;
  std::optional<State<float>> extent_binding_;
  std::function<void(bool)> on_open_change_;
  std::function<void(float)> on_extent_change_;

  Rect<float> panel_rect_, content_rect_, handle_rect_;
  float visible_extent_ = 0.0f, available_extent_ = 560.0f;
  bool dragging_ = false, drag_start_open_ = true;
  float drag_restore_extent_ = 280.0f;
  // Opening/closing jumps the edge away from the captured pointer. Pending
  // motion must catch that edge in its own direction before loading a threshold.
  enum class DragPhase { Pending, Resizing };
  DragPhase drag_phase_ = DragPhase::Pending;
  int drag_direction_ = 0;
  float drag_last_pointer_ = 0.0f, drag_direction_origin_ = 0.0f;
  float drag_edge_anchor_ = 0.0f, drag_edge_offset_ = 0.0f;
  float drag_resize_extent_ = 0.0f, drag_reopen_extent_ = 280.0f;
  float elastic_offset_ = 0.0f, release_offset_ = 0.0f;
  std::optional<double> release_time_;
};

template <WidgetClass Panel, WidgetClass Content>
[[nodiscard]] Sidebar sidebar(Panel &&panel, Content &&content) {
  return Sidebar(std::forward<Panel>(panel), std::forward<Content>(content));
}

} // namespace voidui
