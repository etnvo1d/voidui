#include "voidui/core/widget_tree.h"
#include "voidui/paint/display_list.h"
#include "voidui/paint/painter.h"
#include "voidui/widgets/column.h"
#include "voidui/widgets/input.h"
#include "voidui/widgets/row.h"
#include "voidui/widgets/scrollable.h"
#include "voidui/widgets/text.h"

#include <cmath>
#include <cstdio>
#include <string>

using namespace voidui;

namespace {
int failures = 0;
void check(bool ok, const char *message) {
  std::printf("%s %s\n", ok ? "ok  " : "FAIL", message);
  failures += !ok;
}
Size<Length> fixed(float w, float h) {
  return {Length::Fixed{w}, Length::Fixed{h}};
}
auto lines() {
  auto result = column();
  for (int i = 0; i < 40; ++i)
    result.add(text("Line " + std::to_string(i) + " selectable content"));
  return result;
}
void frame(WidgetTree &tree, double time) {
  tree.advance_animations(time);
  if (tree.needs_layout()) tree.layout({500.0f, 300.0f});
  DisplayList list;
  Painter painter(list, {500.0f, 300.0f});
  tree.render(painter);
}
void ticks(WidgetTree &tree, double &time, int count) {
  for (int i = 0; i < count; ++i) {
    time += 0.02;
    frame(tree, time);
  }
}
void press(WidgetTree &tree, Point<float> point) {
  MousePressedEvent e(MouseButton::Left, point);
  tree.process_event(e);
}
void move(WidgetTree &tree, Point<float> point) {
  MouseMovedEvent e(point);
  tree.process_event(e);
}
void release(WidgetTree &tree, Point<float> point) {
  MouseReleasedEvent e(MouseButton::Left, point);
  tree.process_event(e);
}

void vertical() {
  WidgetTree tree(transfer_widget(scrollable(lines()).size(fixed(220, 100))));
  double time = 10;
  frame(tree, time);
  auto *scroll = static_cast<Scrollable *>(tree.root()->widget.get());
  press(tree, {2, 5});
  ticks(tree, time, 4);
  check(scroll->scroll_offset().y == 0 && !std::isfinite(tree.next_wake_time()),
        "holding a simple press at an edge does not start scrolling");
  const auto builds = TextLayout::builds_performed();
  move(tree, {120, 98});
  const auto initial = tree.selected_text();
  check(std::isfinite(tree.next_wake_time()), "dragging to an edge arms the window scheduler");
  ticks(tree, time, 12);
  check(scroll->scroll_offset().y > 0 && tree.selected_text().size() > initial.size(),
        "a stationary edge pointer scrolls and extends selection after layout");
  check(tree.selected_text().starts_with("Line 0 selectable content\n"),
        "auto-scroll preserves the selection anchor");
  check(TextLayout::builds_performed() == builds, "scrolling reuses shaped text layouts");
  move(tree, {100, 50});
  const float stopped = scroll->scroll_offset().y;
  ticks(tree, time, 10);
  check(scroll->scroll_offset().y == stopped && !std::isfinite(tree.next_wake_time()),
        "returning to the center stops scrolling and further timer wakeups");
  move(tree, {120, 2});
  ticks(tree, time, 4);
  check(scroll->scroll_offset().y < stopped, "moving to the top edge reverses scrolling");
  move(tree, {120, 130});
  ticks(tree, time, 250);
  check(scroll->scroll_offset().y == scroll->max_scroll_offset().y &&
            !std::isfinite(tree.next_wake_time()),
        "scrolling stops at the content limit without waking forever");
  check(tree.selected_text().find("Line 39") != std::string::npos,
        "selection reaches the newly revealed last line");
  move(tree, {120, -30});
  ticks(tree, time, 4);
  const float before_release = scroll->scroll_offset().y;
  release(tree, {120, -30});
  ticks(tree, time, 10);
  check(scroll->scroll_offset().y == before_release && !std::isfinite(tree.next_wake_time()),
        "release outside the viewport cancels auto-scroll immediately");

  scroll->scroll_to({0, 0}); tree.request_layout(); frame(tree, time);
  press(tree, {2, 5}); move(tree, {120, 98}); ticks(tree, time, 3);
  const float before_focus_loss = scroll->scroll_offset().y;
  WindowFocusLostEvent lost; tree.process_event(lost);
  ticks(tree, time, 5);
  check(scroll->scroll_offset().y == before_focus_loss && tree.selected_text().empty(),
        "window focus loss cancels scrolling and selection");

  scroll->scroll_to({0, 0}); tree.request_layout(); frame(tree, time);
  press(tree, {2, 5}); move(tree, {120, 98}); ticks(tree, time, 2);
  KeyPressedEvent escape(Keycode::Escape); tree.process_event(escape);
  const float before_escape = scroll->scroll_offset().y;
  ticks(tree, time, 5);
  check(scroll->scroll_offset().y == before_escape && !std::isfinite(tree.next_wake_time()),
        "Escape cancels the pending scroll timer");

  scroll->scroll_to({0, 0}); tree.request_layout(); frame(tree, time);
  press(tree, {2, 5}); move(tree, {120, 98}); ticks(tree, time, 2);
  tree.build(transfer_widget(text("replacement")));
  ticks(tree, time, 5);
  check(tree.selected_text().empty() && !std::isfinite(tree.next_wake_time()),
        "removing the selected tree leaves no live auto-scroll work");
}

void horizontal_and_speed() {
  const std::string content(200, 'W');
  WidgetTree tree(transfer_widget(scrollable(text(content))
      .axis(ScrollAxis::Horizontal).size(fixed(180, 65))));
  double time = 10; frame(tree, time);
  auto *scroll = static_cast<Scrollable *>(tree.root()->widget.get());
  press(tree, {2, 5}); move(tree, {158, 10}); ticks(tree, time, 1);
  const float near = scroll->scroll_offset().x;
  move(tree, {210, 10}); ticks(tree, time, 1);
  check(near > 0 && scroll->scroll_offset().x - near > near,
        "horizontal scrolling accelerates as the pointer moves beyond the edge");
  const auto selection = tree.selected_text().size();
  ticks(tree, time, 10);
  check(tree.selected_text().size() > selection && scroll->scroll_offset().y == 0,
        "horizontal selection continues with no mouse motion or vertical drift");
  move(tree, {-20, 10}); ticks(tree, time, 50);
  check(scroll->scroll_offset().x == 0, "left edge scrolling returns to the start");
}

void nested_and_transformed() {
  WidgetTree tree(transfer_widget(scrollable(column(
      scrollable(lines()).size(fixed(220, 180)),
      text("outer trailing content").height(Length::Fixed{250})))
      .size(fixed(240, 100))));
  double time = 10; frame(tree, time);
  auto *outer = static_cast<Scrollable *>(tree.root()->widget.get());
  auto *inner = static_cast<Scrollable *>(tree.root()->children[0]->children[0]->widget.get());
  press(tree, {2, 5}); move(tree, {120, 98}); ticks(tree, time, 5);
  check(inner->scroll_offset().y > 0 && outer->scroll_offset().y == 0,
        "a clipped inner viewport scrolls first at its visible edge");
  inner->scroll_to(inner->max_scroll_offset()); tree.request_layout(); frame(tree, time);
  ticks(tree, time, 4);
  check(outer->scroll_offset().y > 0,
        "the outer viewport takes over when the inner viewport reaches its limit");

  WidgetTree siblings(transfer_widget(row(
      scrollable(lines()).size(fixed(200, 100)),
      scrollable(lines()).size(fixed(200, 100))).id("shifted")));
  siblings.set_stylesheet(StyleParser::parse(
      "#shifted { transform: translate(25px, 30px); }").sheet);
  time = 10; frame(siblings, time);
  auto *left = static_cast<Scrollable *>(siblings.root()->children[0]->widget.get());
  auto *right = static_cast<Scrollable *>(siblings.root()->children[1]->widget.get());
  press(siblings, {27, 35}); move(siblings, {145, 128}); ticks(siblings, time, 5);
  check(left->scroll_offset().y > 0 && right->scroll_offset().y == 0,
        "transformed edge scrolling stays within the current column");
}

void editors() {
  WidgetTree single(transfer_widget(input(std::string(200, 'W')).size(fixed(180, 45))));
  double time = 10; frame(single, time);
  press(single, {15, 20}); move(single, {200, 20});
  const auto first = single.selected_text().size();
  ticks(single, time, 20);
  check(single.selected_text().size() > first,
        "single-line input scrolls its own text while the pointer is stationary");

  std::string content;
  for (int i = 0; i < 40; ++i) content += "editor line " + std::to_string(i) + "\n";
  WidgetTree multi(transfer_widget(scrollable(column(
      textarea(content).size(fixed(220, 90)), text("outside").height(Length::Fixed{250})))
      .size(fixed(240, 150))));
  time = 10; frame(multi, time);
  press(multi, {15, 15}); move(multi, {120, 110});
  const auto initial = multi.selected_text().size();
  ticks(multi, time, 20);
  check(multi.selected_text().size() > initial &&
        static_cast<Scrollable *>(multi.root()->widget.get())->scroll_offset().y == 0 &&
        multi.selected_text().find("outside") == std::string::npos,
        "textarea auto-scroll remains confined to the editor");
}
} // namespace

int main() {
  vertical();
  horizontal_and_speed();
  nested_and_transformed();
  editors();
  std::printf("selection auto-scroll: %d failures\n", failures);
  return failures ? 1 : 0;
}
