#include <cassert>
#include <cmath>
#include <cstdio>
#include "voidui/core/component.h"
#include "voidui/widgets/button.h"
#include "voidui/widgets/column.h"
#include "voidui/widgets/sidebar.h"
#include "voidui/widgets/modal.h"

using namespace voidui;
namespace {
bool near(float a, float b) { return std::abs(a - b) < 0.01f; }
class Box : public Widget {
public:
  inline static int draws = 0;
  void register_children(Registrar &) override {}
  Size<float> layout(Constraints c, LayoutContext &ctx) override {
    return c.resolve(ctx.style.layout_size(), {100, 80});
  }
  void draw(const DrawContext &ctx, Painter &p) override {
    ++draws;
    p.fill_rect(ctx.bounds, Paint(Color(100, 120, 140)));
  }
  EventResult on_event(Event &) override { return EventResult::Unhandled; }
  std::unique_ptr<Widget> clone() const override { return std::make_unique<Box>(); }
};
Sidebar *view(WidgetTree &tree) {
  return static_cast<Sidebar *>(const_cast<Node *>(layout_box_of(tree.root()))->widget.get());
}
Point<float> grip(WidgetTree &tree) {
  auto *node = layout_box_of(tree.root());
  auto r = view(tree)->handle_bounds();
  return {node->global_pos.x + r.origin.x + r.size.width / 2,
          node->global_pos.y + r.origin.y + r.size.height / 2};
}
void press(WidgetTree &tree, Point<float> p) {
  MousePressedEvent e(MouseButton::Left, p); tree.process_event(e);
}
void move(WidgetTree &tree, Point<float> p) {
  MouseMovedEvent e(p); tree.process_event(e);
}
void release(WidgetTree &tree, Point<float> p) {
  MouseReleasedEvent e(MouseButton::Left, p); tree.process_event(e);
}
void key(WidgetTree &tree, Keycode code) {
  KeyPressedEvent e(code); tree.process_event(e);
}
auto make_view() {
  return sidebar(Box(), Box()).extent(200).limits(100, 400)
      .drag_mode(SidebarDragMode::Elastic).drag_threshold(40).collapse_threshold(30);
}
}

int main() {
  // Actual tree input/capture, with all axes and signs and no preceding paint.
  for (const auto edge : {SidebarPlacement::Left, SidebarPlacement::Right,
                          SidebarPlacement::Top, SidebarPlacement::Bottom}) {
    WidgetTree tree(transfer_widget(make_view().placement(edge)));
    tree.layout({800, 600});
    const bool horizontal = edge == SidebarPlacement::Left || edge == SidebarPlacement::Right;
    const bool leading = edge == SidebarPlacement::Left || edge == SidebarPlacement::Top;
    const float sign = leading ? 1.0f : -1.0f;
    auto *s = view(tree);
    assert(near(s->visible_extent(), 200));
    assert(near(horizontal ? s->content_bounds().size.width : s->content_bounds().size.height,
                (horizontal ? 800.0f : 600.0f) - 208));
    const auto start = grip(tree);
    const auto point = [&](float delta) {
      return Point<float>{start.x + (horizontal ? sign * delta : 0),
                          start.y + (horizontal ? 0 : sign * delta)};
    };
    move(tree, start);
    assert(tree.get_current_cursor_shape() == (horizontal ? CursorShape::HorizontalResize : CursorShape::VerticalResize));
    press(tree, start);
    move(tree, point(39));
    assert(near(s->expanded_extent(), 200) && s->elastic_offset() > 0);
    const auto resize_cursor = horizontal ? CursorShape::HorizontalResize
                                         : CursorShape::VerticalResize;
    assert(tree.get_current_cursor_shape() == resize_cursor);
    MouseLeftEvent leave; tree.process_event(leave);
    assert(tree.get_current_cursor_shape() == resize_cursor);
    move(tree, point(80));
    tree.layout({800, 600});
    assert(near(s->expanded_extent(), 240));
    // Continue through relayout, then use the final release coordinate.
    release(tree, point(100));
    tree.layout({800, 600});
    assert(near(s->expanded_extent(), 260) && !s->is_dragging());
    assert(tree.get_current_cursor_shape() == CursorShape::Default);
    const auto second = grip(tree);
    press(tree, second);
    move(tree, {second.x + (horizontal ? -sign * 280 : 0),
                second.y + (horizontal ? 0 : -sign * 280)});
    tree.layout({800, 600});
    assert(!s->is_open() && near(s->visible_extent(), 0));
    WindowFocusLostEvent lost; tree.process_event(lost);
    assert(!s->is_dragging());
    assert(tree.get_current_cursor_shape() == CursorShape::Default);
    const auto closed = grip(tree);
    press(tree, closed);
    release(tree, {closed.x + (horizontal ? sign * 50 : 0),
                   closed.y + (horizontal ? 0 : sign * 50)});
    tree.layout({800, 600});
    assert(s->is_open() && near(s->visible_extent(), 260));

    // Snapping open consumes the opening threshold. The cursor must catch
    // the new edge before it can load another expansion threshold.
    WidgetTree snapped(transfer_widget(make_view().placement(edge).open(false)));
    snapped.layout({800, 600});
    const auto snap_start = grip(snapped);
    const auto snap_point = [&](float extent) {
      return Point<float>{snap_start.x + (horizontal ? sign * extent : 0),
                          snap_start.y + (horizontal ? 0 : sign * extent)};
    };
    const auto sample = [&](float extent) {
      move(snapped, snap_point(extent));
      snapped.layout({800, 600});
    };
    press(snapped, snap_start);
    sample(40); // Exactly the threshold opens to the remembered size.
    assert(view(snapped)->is_open() && near(view(snapped)->visible_extent(), 200));
    sample(40); // A duplicate sample must not undo the snap or resize.
    sample(100);
    sample(199);
    assert(near(view(snapped)->visible_extent(), 200));
    assert(near(view(snapped)->elastic_offset(), 0));
    sample(200);
    sample(239);
    assert(near(view(snapped)->visible_extent(), 200));
    sample(250);
    assert(near(view(snapped)->visible_extent(), 210));
    sample(260);
    sample(255); // Once resizing, reversals follow immediately.
    assert(near(view(snapped)->visible_extent(), 215));
    release(snapped, snap_point(255));
    assert(near(view(snapped)->expanded_extent(), 215));

    // Reversing inside the gap loads contraction from the latest turning
    // point. It must neither wait for the old press nor jump to cursor size.
    WidgetTree reversed(transfer_widget(make_view().placement(edge).open(false)));
    reversed.layout({800, 600});
    press(reversed, grip(reversed));
    const auto reverse_sample = [&](float extent) {
      move(reversed, snap_point(extent));
      reversed.layout({800, 600});
    };
    reverse_sample(40);
    reverse_sample(150);
    reverse_sample(120);
    assert(near(view(reversed)->visible_extent(), 200));
    reverse_sample(110);
    assert(near(view(reversed)->visible_extent(), 200));
    reverse_sample(90);
    assert(near(view(reversed)->visible_extent(), 180));
    reverse_sample(-20); // Reach min - collapse_threshold.
    assert(!view(reversed)->is_open());
    assert(near(view(reversed)->expanded_extent(), 200));
    reverse_sample(1000); // One sparse event opens, without inflating size.
    assert(view(reversed)->is_open() && near(view(reversed)->visible_extent(), 200));
    reverse_sample(980); // Cursor is beyond the new edge: shrinking waits.
    assert(near(view(reversed)->visible_extent(), 200));
    reverse_sample(1020); // Expanding can load from the latest turn instead.
    assert(near(view(reversed)->visible_extent(), 200));
    reverse_sample(1040);
    assert(near(view(reversed)->visible_extent(), 220));
    key(reversed, Keycode::Escape);
    reversed.layout({800, 600});
    assert(!view(reversed)->is_open() && near(view(reversed)->expanded_extent(), 200));
    release(reversed, snap_point(1040));
    assert(!view(reversed)->is_open());

    // Symmetric case: the restored panel ends BEFORE the captured cursor.
    WidgetTree overshot(transfer_widget(make_view().placement(edge).open(false)
                                            .limits(20, 400).extent(60)));
    overshot.layout({800, 600});
    press(overshot, grip(overshot));
    const auto overshot_sample = [&](float extent) {
      move(overshot, snap_point(extent));
      overshot.layout({800, 600});
    };
    overshot_sample(100);
    assert(near(view(overshot)->visible_extent(), 60));
    overshot_sample(80);
    overshot_sample(61);
    assert(near(view(overshot)->visible_extent(), 60));
    assert(near(view(overshot)->elastic_offset(), 0));
    overshot_sample(60);
    overshot_sample(30);
    assert(near(view(overshot)->visible_extent(), 60));
    overshot_sample(20);
    overshot_sample(10);
    assert(near(view(overshot)->visible_extent(), 50));
    release(overshot, snap_point(10));
    assert(near(view(overshot)->expanded_extent(), 50));

    // Opening/closing can be elastic while all ordinary resizing follows
    // immediately, including a fresh press and reversing inside the snap gap.
    for (bool overrides : {false, true}) {
      auto declaration = make_view().placement(edge).open(false)
          .drag_mode(SidebarDragMode::ElasticOpenClose);
      if (overrides) {
        declaration.drag_mode(SidebarDragMode::Immediate)
            .open_behavior(SidebarDragBehavior::Elastic)
            .resize_behavior(SidebarDragBehavior::Immediate)
            .collapse_behavior(SidebarDragBehavior::Elastic);
      }
      WidgetTree hybrid(clone_widget(declaration));
      hybrid.layout({800, 600});
      press(hybrid, grip(hybrid));
      const auto hybrid_sample = [&](float distance) {
        move(hybrid, snap_point(distance));
        hybrid.layout({800, 600});
      };
      hybrid_sample(39);
      assert(!view(hybrid)->is_open() && view(hybrid)->elastic_offset() > 0);
      hybrid_sample(40);
      assert(view(hybrid)->is_open() && near(view(hybrid)->visible_extent(), 200));
      hybrid_sample(150); // Catching the moved edge adds no second resistance.
      hybrid_sample(149); // Reversing inside the gap follows immediately.
      assert(near(view(hybrid)->visible_extent(), 199));
      key(hybrid, Keycode::Escape);
      hybrid.layout({800, 600});
      assert(!view(hybrid)->is_open() && near(view(hybrid)->expanded_extent(), 200));
      press(hybrid, grip(hybrid));
      hybrid_sample(40);
      hybrid_sample(200);
      hybrid_sample(205);
      assert(near(view(hybrid)->visible_extent(), 205));
      assert(near(view(hybrid)->elastic_offset(), 0));
      hybrid_sample(90);
      assert(view(hybrid)->is_open() && near(view(hybrid)->visible_extent(), 100));
      assert(view(hybrid)->elastic_offset() < 0);
      hybrid_sample(70);
      assert(!view(hybrid)->is_open() && near(view(hybrid)->expanded_extent(), 205));
      release(hybrid, snap_point(70));
      assert(!view(hybrid)->is_open());
      press(hybrid, grip(hybrid));
      release(hybrid, snap_point(40));
      hybrid.layout({800, 600});
      assert(view(hybrid)->is_open() && near(view(hybrid)->visible_extent(), 205));
      const auto fresh = grip(hybrid);
      press(hybrid, fresh);
      release(hybrid, {fresh.x + (horizontal ? sign * 5 : 0),
                      fresh.y + (horizontal ? 0 : sign * 5)});
      assert(near(view(hybrid)->expanded_extent(), 210));
    }

    // Immediate means no resistance when opening, resizing, or collapsing.
    // Overshooting the maximum must not delay the next inward movement.
    WidgetTree direct(transfer_widget(make_view().placement(edge).open(false)
                                         .drag_mode(SidebarDragMode::Immediate)));
    direct.layout({800, 600});
    press(direct, grip(direct));
    const auto direct_sample = [&](float distance) {
      move(direct, snap_point(distance));
      direct.layout({800, 600});
      assert(near(view(direct)->elastic_offset(), 0));
    };
    direct_sample(1);
    assert(view(direct)->is_open() && near(view(direct)->visible_extent(), 200));
    direct_sample(450);
    assert(near(view(direct)->visible_extent(), 400));
    direct_sample(449);
    assert(near(view(direct)->visible_extent(), 399));
    direct_sample(150);
    assert(!view(direct)->is_open());
    release(direct, snap_point(150));
  }

  // Every phase can override either preset independently, in any setter order.
  for (auto preset : {SidebarDragMode::Immediate, SidebarDragMode::Elastic}) {
    for (bool opening : {false, true}) {
      for (bool resizing : {false, true}) {
        for (bool collapsing : {false, true}) {
          const auto behavior = [](bool elastic) {
            return elastic ? SidebarDragBehavior::Elastic : SidebarDragBehavior::Immediate;
          };
          auto declaration = make_view().open_behavior(behavior(opening))
              .resize_behavior(behavior(resizing)).collapse_behavior(behavior(collapsing))
              .drag_mode(preset);
          WidgetTree sizing(clone_widget(declaration));
          sizing.layout({800, 600});
          auto start = grip(sizing);
          press(sizing, start);
          release(sizing, {start.x + 5, start.y});
          assert(near(view(sizing)->expanded_extent(), resizing ? 200.0f : 205.0f));
          assert((view(sizing)->elastic_offset() > 0) == resizing);

          WidgetTree closing(clone_widget(declaration));
          closing.layout({800, 600});
          start = grip(closing);
          press(closing, start);
          const float threshold = resizing ? 40.0f : 0.0f;
          move(closing, {start.x - 110 - threshold, start.y});
          assert(view(closing)->is_open() == collapsing);
          assert((view(closing)->elastic_offset() < 0) == collapsing);
          release(closing, {start.x - 130 - threshold, start.y});
          assert(!view(closing)->is_open());
          assert(near(view(closing)->expanded_extent(), 200));

          declaration.open(false);
          WidgetTree revealing(clone_widget(declaration));
          revealing.layout({800, 600});
          start = grip(revealing);
          press(revealing, start);
          release(revealing, {start.x + 5, start.y});
          assert(view(revealing)->is_open() == !opening);
          assert((view(revealing)->elastic_offset() > 0) == opening);
        }
      }
    }
  }

  // Short gestures do not resize; the curve relaxes on the tree's clock.
  WidgetTree short_tree(transfer_widget(make_view().edge_visible(true)));
  short_tree.layout({800, 600});
  auto p = grip(short_tree);
  press(short_tree, p); release(short_tree, {p.x + 20, p.y});
  assert(near(view(short_tree)->expanded_extent(), 200));
  DisplayList list;
  Painter painter(list, {800, 600});
  short_tree.advance_animations(1.0);
  short_tree.render(painter);
  assert(short_tree.needs_paint());
  short_tree.advance_animations(1.3);
  short_tree.render(painter);
  assert(near(view(short_tree)->elastic_offset(), 0));

  // Bound state survives every drag-triggered component reconciliation.
  std::optional<State<bool>> showing;
  std::optional<State<float>> width;
  std::optional<State<int>> unrelated;
  int open_changes = 0;
  WidgetTree bound(transfer_widget(component([&] {
    showing = use_state(true); width = use_state(200.0f); unrelated = use_state(0);
    return make_view().open(*showing).extent(*width)
        .on_open_change([&](bool) { ++open_changes; });
  })));
  bound.layout({800, 600});
  p = grip(bound); press(bound, p);
  move(bound, {p.x + 70, p.y}); bound.layout({800, 600});
  assert(near(width->get(), 230) && view(bound)->is_dragging());
  unrelated->set(1); bound.layout({800, 600});
  assert(bound.get_current_cursor_shape() == CursorShape::HorizontalResize);
  move(bound, {p.x + 90, p.y}); bound.layout({800, 600});
  assert(near(width->get(), 250));
  showing->set(false); bound.layout({800, 600});
  assert(!view(bound)->is_open() && !view(bound)->is_dragging());
  move(bound, {p.x + 150, p.y}); release(bound, {p.x + 150, p.y});
  assert(!showing->get());
  showing->set(true); bound.layout({800, 600});
  assert(near(view(bound)->visible_extent(), 250));
  p = grip(bound); press(bound, p); release(bound, p);
  key(bound, Keycode::Return); bound.layout({800, 600});
  assert(!showing->get() && open_changes == 1);

  // Binding echoes preserve both gap anchors and the latest turning point.
  WidgetTree bound_snap(transfer_widget(component([&] {
    showing = use_state(false); width = use_state(200.0f); unrelated = use_state(0);
    return make_view().open(*showing).extent(*width);
  })));
  bound_snap.layout({800, 600});
  p = grip(bound_snap); press(bound_snap, p);
  move(bound_snap, {p.x + 40, p.y}); bound_snap.layout({800, 600});
  assert(showing->get() && near(width->get(), 200));
  move(bound_snap, {p.x + 150, p.y}); bound_snap.layout({800, 600});
  unrelated->set(1); bound_snap.layout({800, 600});
  assert(near(width->get(), 200));
  move(bound_snap, {p.x + 90, p.y}); bound_snap.layout({800, 600});
  assert(near(width->get(), 180) && view(bound_snap)->is_dragging());
  key(bound_snap, Keycode::Escape); bound_snap.layout({800, 600});
  assert(!showing->get() && near(width->get(), 200));

  // Hybrid binding echoes keep immediate resizing; a phase configuration
  // change cancels capture without allowing a stale release to change size.
  std::optional<State<SidebarDragBehavior>> resize_behavior;
  WidgetTree bound_hybrid(transfer_widget(component([&] {
    showing = use_state(false); width = use_state(200.0f);
    resize_behavior = use_state(SidebarDragBehavior::Immediate);
    return make_view().drag_mode(SidebarDragMode::ElasticOpenClose)
        .resize_behavior(resize_behavior->get()).open(*showing).extent(*width);
  })));
  bound_hybrid.layout({800, 600});
  p = grip(bound_hybrid); press(bound_hybrid, p);
  move(bound_hybrid, {p.x + 40, p.y}); bound_hybrid.layout({800, 600});
  move(bound_hybrid, {p.x + 205, p.y}); bound_hybrid.layout({800, 600});
  assert(showing->get() && near(width->get(), 205) && view(bound_hybrid)->is_dragging());
  move(bound_hybrid, {p.x + 210, p.y}); bound_hybrid.layout({800, 600});
  assert(near(width->get(), 210));
  resize_behavior->set(SidebarDragBehavior::Elastic); bound_hybrid.layout({800, 600});
  assert(!view(bound_hybrid)->is_dragging());
  release(bound_hybrid, {p.x + 250, p.y});
  assert(near(width->get(), 210));

  // Uncontrolled declarations preserve runtime across unrelated renders.
  WidgetTree local(transfer_widget(component([&] {
    unrelated = use_state(0);
    return make_view().open(true);
  })));
  local.layout({800, 600}); p = grip(local); press(local, p);
  move(local, {p.x + 70, p.y}); local.layout({800, 600});
  unrelated->set(2); local.layout({800, 600});
  assert(near(view(local)->expanded_extent(), 230) && view(local)->is_dragging());
  key(local, Keycode::Escape); local.layout({800, 600});
  assert(near(view(local)->expanded_extent(), 200) && !view(local)->is_dragging());

  WidgetTree overlay(transfer_widget(make_view().mode(SidebarMode::Overlay).placement(SidebarPlacement::Right)));
  overlay.layout({800, 600});
  assert(near(view(overlay)->content_bounds().size.width, 800));
  assert(near(view(overlay)->panel_bounds().origin.x, 600));
  WidgetTree tiny(transfer_widget(make_view().extent(300)));
  tiny.layout({30, 20});
  assert(near(view(tiny)->visible_extent(), 22));
  tiny.layout({800, 600});
  assert(near(view(tiny)->visible_extent(), 300));
  WidgetTree rail(transfer_widget(make_view().open(false).collapsed_extent(56)));
  rail.layout({800, 600});
  assert(near(view(rail)->visible_extent(), 56));
  WidgetTree disabled(transfer_widget(make_view().open(false).drag_mode(SidebarDragMode::Disabled)));
  disabled.layout({800, 600});
  assert(near(view(disabled)->content_bounds().size.width, 800));
  WidgetTree immediate(transfer_widget(make_view().drag_mode(SidebarDragMode::Immediate)));
  immediate.layout({800, 600}); p = grip(immediate); press(immediate, p);
  release(immediate, {p.x + 5, p.y});
  assert(near(view(immediate)->expanded_extent(), 205));

  // Closed content remains mounted, but cannot receive keyboard focus.
  int hidden_clicks = 0, visible_clicks = 0;
  WidgetTree focus(transfer_widget(sidebar(
      Button().on_click([&] { ++hidden_clicks; }),
      Button().on_click([&] { ++visible_clicks; })).open(false)));
  focus.layout({800, 600});
  key(focus, Keycode::Tab); key(focus, Keycode::Return);
  assert(visible_clicks == 1 && hidden_clicks == 0);
  key(focus, Keycode::Tab); key(focus, Keycode::Return);
  assert(view(focus)->is_open() && hidden_clicks == 0);

  // Clone preserves configuration and nested slot declarations.
  auto declaration = make_view().placement(SidebarPlacement::Bottom).open(false);
  WidgetTree cloned(clone_widget(declaration));
  cloned.layout({800, 600});
  assert(!view(cloned)->is_open() && near(view(cloned)->handle_bounds().origin.y, 592));
  assert(cloned.root()->internal_children[1]->children.size() == 1);

  // A closed viewport also suppresses descendants escaping normal clipping.
  WidgetTree escaped(transfer_widget(sidebar(
      column(Box().position(Position::Fixed).left(40).top(40),
             modal(Box()).open(true)), Box()).open(false)));
  escaped.layout({800, 600});
  Box::draws = 0;
  DisplayList escaped_list;
  Painter escaped_painter(escaped_list, {800, 600});
  escaped.render(escaped_painter);
  assert(Box::draws == 1);

  WidgetTree unbounded(transfer_widget(make_view()));
  unbounded.layout(Constraints::unconstrained());
  assert(near(unbounded.root()->size.width, 308));
  assert(near(unbounded.root()->size.height, 80));
  std::puts("sidebar_selftest: passed");
}
