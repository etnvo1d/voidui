#include <cmath>
#include <cstdio>
#include <memory>

#include "voidui/core/widget_tree.h"
#include "voidui/paint/display_list.h"
#include "voidui/paint/painter.h"
#include "voidui/widgets/scrollable.h"

namespace {

class FixedBox : public voidui::Widget {
public:
  inline static int layout_calls = 0;

  FixedBox() = default;

  FixedBox(float width, float height) {
    set_style<voidui::styles::Width>(voidui::Length::Fixed{width});
    set_style<voidui::styles::Height>(voidui::Length::Fixed{height});
  }

  void register_children(voidui::Registrar &) override {}

  voidui::Size<float> layout(voidui::Constraints constraints,
                             voidui::LayoutContext &ctx) override {
    ++layout_calls;
    return constraints.resolve(ctx.style.layout_size(), {});
  }

  void draw(const voidui::DrawContext &ctx, voidui::Painter &painter) override {
    painter.fill_rect(ctx.bounds, voidui::Paint(voidui::Color(255, 0, 0)));
  }

  voidui::EventResult on_event(voidui::Event &) override {
    return voidui::EventResult::Unhandled;
  }

  std::unique_ptr<voidui::Widget> clone() const override {
    return std::make_unique<FixedBox>();
  }
};

bool close(float lhs, float rhs) { return std::abs(lhs - rhs) < 0.02f; }

bool expect(bool condition, const char *message) {
  if (!condition)
    std::fprintf(stderr, "FAIL: %s\n", message);
  return condition;
}

voidui::Size<voidui::Length> fixed_size(float width, float height) {
  return {voidui::Length::Fixed{width}, voidui::Length::Fixed{height}};
}

} // namespace

int main() {
  bool ok = true;

  const auto parsed_styles = voidui::StyleParser::parse(R"vss(
    scrollable {
      horizontal-scrollbar: always;
      vertical-scrollbar: never;
      scrollbar-thickness: 12;
      scrollbar-hit-slop: 6;
      scrollbar-track: #10101020;
      scrollbar-thumb: #20202080;
    }
  )vss");
  ok &= expect(parsed_styles.diagnostics.empty(),
               "scrollbar policies and appearance parse from stylesheets");

  auto auto_view = voidui::scrollable(FixedBox(300.0f, 400.0f))
                       .axis(voidui::ScrollAxis::Both)
                       .size(fixed_size(100.0f, 120.0f));
  voidui::WidgetTree auto_tree(voidui::transfer_widget(std::move(auto_view)));
  auto_tree.layout(voidui::Constraints(500.0f, 500.0f));

  auto *auto_scrollable =
      dynamic_cast<voidui::Scrollable *>(auto_tree.root()->widget.get());
  ok &= expect(auto_scrollable != nullptr,
               "scrollable root has its concrete type");
  if (!auto_scrollable)
    return 1;

  ok &= expect(auto_scrollable->horizontal_scrollbar_visible() &&
                   auto_scrollable->vertical_scrollbar_visible(),
               "Auto shows both bars when both axes overflow");
  ok &= expect(close(auto_scrollable->viewport_size().width, 90.0f) &&
                   close(auto_scrollable->viewport_size().height, 110.0f),
               "visible bars reserve space from the viewport");
  ok &= expect(close(auto_scrollable->content_size().width, 300.0f) &&
                   close(auto_scrollable->content_size().height, 400.0f),
               "scrollable measures the full overflowing content");

  voidui::MouseScrolledEvent wheel(0.0f, -1.0f,
                                   voidui::Point<float>(10.0f, 10.0f));
  auto_tree.process_event(wheel);
  ok &= expect(auto_tree.needs_layout(),
               "scrolling requests layout for the translated content");
  auto_tree.layout(voidui::Constraints(500.0f, 500.0f));
  ok &= expect(close(auto_scrollable->scroll_offset().y, 40.0f),
               "mouse wheel advances the vertical offset");
  ok &= expect(close(auto_tree.root()->children[0]->global_pos.y, -40.0f),
               "layout moves content by the current scroll offset");

  voidui::DisplayList list;
  voidui::Painter painter(list, voidui::Size<float>(500.0f, 500.0f));
  auto_tree.render(painter);
  ok &= expect(list.commands().size() >= 2,
               "scrollable and its content both produce draw commands");
  if (list.commands().size() >= 2) {
    const auto &content_clip =
        list.clips()[list.commands()[1].clip_index].scissor;
    ok &= expect(close(content_clip.size.width, 90.0f) &&
                     close(content_clip.size.height, 110.0f),
                 "content draw commands are clipped to the viewport");
  }

  // After the wheel scroll, the thumb begins around y=11. Pointer capture
  // keeps delivering motion to the scrollable while dragging, and the next
  // layout applies the new offset.
  voidui::MousePressedEvent press(voidui::MouseButton::Left,
                                  voidui::Point<float>(95.0f, 20.0f));
  auto_tree.process_event(press);
  voidui::MouseMovedEvent move(voidui::Point<float>(95.0f, 60.0f));
  auto_tree.process_event(move);
  voidui::MouseReleasedEvent release(voidui::MouseButton::Left,
                                     voidui::Point<float>(95.0f, 60.0f));
  auto_tree.process_event(release);
  auto_tree.layout(voidui::Constraints(500.0f, 500.0f));
  ok &= expect(auto_scrollable->scroll_offset().y > 100.0f,
               "vertical thumb dragging changes the scroll offset");

  auto_scrollable->scroll_to(voidui::Point<float>(0.0f, 0.0f));
  auto_tree.layout(voidui::Constraints(500.0f, 500.0f));
  voidui::MousePressedEvent continuous_press(
      voidui::MouseButton::Left, voidui::Point<float>(95.0f, 10.0f));
  auto_tree.process_event(continuous_press);
  voidui::MouseMovedEvent continuous_move_1(voidui::Point<float>(95.0f, 30.0f));
  auto_tree.process_event(continuous_move_1);
  auto_tree.layout(voidui::Constraints(500.0f, 500.0f));
  const float intermediate_1 = auto_scrollable->scroll_offset().y;
  voidui::MouseMovedEvent continuous_move_2(voidui::Point<float>(95.0f, 60.0f));
  auto_tree.process_event(continuous_move_2);
  auto_tree.layout(voidui::Constraints(500.0f, 500.0f));
  const float intermediate_2 = auto_scrollable->scroll_offset().y;
  voidui::MouseReleasedEvent continuous_release(
      voidui::MouseButton::Left, voidui::Point<float>(95.0f, 60.0f));
  auto_tree.process_event(continuous_release);
  ok &=
      expect(intermediate_1 > 0.0f && intermediate_1 < intermediate_2 &&
                 intermediate_2 < 290.0f,
             "successive pointer positions produce continuous middle offsets");

  const float before_slop_drag = auto_scrollable->scroll_offset().y;
  voidui::MousePressedEvent slop_press(voidui::MouseButton::Left,
                                       voidui::Point<float>(87.0f, 100.0f));
  auto_tree.process_event(slop_press);
  voidui::MouseMovedEvent slop_move(voidui::Point<float>(87.0f, 70.0f));
  auto_tree.process_event(slop_move);
  voidui::MouseReleasedEvent slop_release(voidui::MouseButton::Left,
                                          voidui::Point<float>(87.0f, 70.0f));
  auto_tree.process_event(slop_release);
  auto_tree.layout(voidui::Constraints(500.0f, 500.0f));
  ok &= expect(!close(auto_scrollable->scroll_offset().y, before_slop_drag),
               "the expanded track hit area starts a direct drag");

  FixedBox::layout_calls = 0;
  auto_tree.layout(voidui::Constraints(500.0f, 500.0f));
  ok &= expect(FixedBox::layout_calls == 1,
               "stable Auto visibility lays out content only once per frame");

  auto always_view =
      voidui::scrollable(FixedBox(20.0f, 20.0f))
          .axis(voidui::ScrollAxis::Both)
          .horizontal_scrollbar(voidui::ScrollbarVisibility::Always)
          .vertical_scrollbar(voidui::ScrollbarVisibility::Always)
          .size(fixed_size(100.0f, 120.0f));
  voidui::WidgetTree always_tree(
      voidui::transfer_widget(std::move(always_view)));
  always_tree.layout(voidui::Constraints(500.0f, 500.0f));
  const auto *always_scrollable = dynamic_cast<const voidui::Scrollable *>(
      always_tree.root()->widget.get());
  ok &= expect(always_scrollable->horizontal_scrollbar_visible() &&
                   always_scrollable->vertical_scrollbar_visible(),
               "Always displays bars without overflow");

  auto never_view =
      voidui::scrollable(FixedBox(300.0f, 400.0f))
          .axis(voidui::ScrollAxis::Both)
          .horizontal_scrollbar(voidui::ScrollbarVisibility::Never)
          .vertical_scrollbar(voidui::ScrollbarVisibility::Never)
          .size(fixed_size(100.0f, 120.0f));
  voidui::WidgetTree never_tree(voidui::transfer_widget(std::move(never_view)));
  never_tree.layout(voidui::Constraints(500.0f, 500.0f));
  auto *never_scrollable =
      dynamic_cast<voidui::Scrollable *>(never_tree.root()->widget.get());
  ok &= expect(!never_scrollable->horizontal_scrollbar_visible() &&
                   !never_scrollable->vertical_scrollbar_visible(),
               "Never hides bars even when content overflows");
  voidui::MouseScrolledEvent hidden_wheel(-1.0f, -1.0f,
                                          voidui::Point<float>(10.0f, 10.0f));
  never_tree.process_event(hidden_wheel);
  never_tree.layout(voidui::Constraints(500.0f, 500.0f));
  ok &= expect(close(never_scrollable->scroll_offset().x, 40.0f) &&
                   close(never_scrollable->scroll_offset().y, 40.0f),
               "hidden bars do not disable scrolling");

  auto vertical_view = voidui::scrollable(FixedBox(300.0f, 400.0f))
                           .axis(voidui::ScrollAxis::Vertical)
                           .size(fixed_size(100.0f, 120.0f));
  voidui::WidgetTree vertical_tree(
      voidui::transfer_widget(std::move(vertical_view)));
  vertical_tree.layout(voidui::Constraints(500.0f, 500.0f));
  const auto *vertical_scrollable = dynamic_cast<const voidui::Scrollable *>(
      vertical_tree.root()->widget.get());
  ok &= expect(!vertical_scrollable->horizontal_scrollbar_visible() &&
                   vertical_scrollable->vertical_scrollbar_visible(),
               "axis selection disables the unwanted horizontal direction");
  ok &= expect(close(vertical_scrollable->content_size().width, 90.0f),
               "vertical-only content is constrained to the viewport width");

  return ok ? 0 : 1;
}
