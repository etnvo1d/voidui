#include "voidui/widgets/input.h"

#include "voidui/core/component.h"
#include "voidui/core/pixel_snap.h"
#include "voidui/core/widget_tree.h"
#include "voidui/paint/display_list.h"
#include "voidui/paint/painter.h"
#include "voidui/widgets/button.h"
#include "voidui/widgets/column.h"
#include "voidui/widgets/text.h"

#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <limits>
#include <string_view>
#include <variant>

namespace {

/// The scale this machine actually runs at, and the one where a caret built in
/// logical units comes apart: one logical pixel is 1.25 device pixels, and the
/// renderer rounds a rectangle's two edges independently.
constexpr float kScale = 1.25f;

/// A colour no other part of a default input paints, so a caret can be picked
/// out of a display list by its brush alone.
const voidui::Color kCaretInk(255, 0, 255);

void check(bool condition, const char *message) {
  if (condition)
    return;
  std::fprintf(stderr, "input self-test failed: %s\n", message);
  std::exit(1);
}

void focus(voidui::WidgetTree &tree, voidui::Point<float> point) {
  voidui::MousePressedEvent down(voidui::MouseButton::Left, point);
  tree.process_event(down);
  voidui::MouseReleasedEvent up(voidui::MouseButton::Left, point);
  tree.process_event(up);
}

/// Paints an already laid-out tree into a display list.
bool rendered_caret(voidui::WidgetTree &tree, voidui::Size<float> viewport,
                    voidui::Rect<float> &out) {
  using namespace voidui;
  DisplayList list;
  Painter painter(list, viewport);
  tree.render(painter);

  for (const DrawCommand &command : list.commands()) {
    if (command.kind != CommandKind::FillRRect)
      continue;
    const Color *color = std::get_if<Color>(&command.brush);
    if (!color || !(*color == kCaretInk))
      continue;
    out = command.transform.map_bounds(command.rect);
    return true;
  }
  return false;
}

/// Lays out and paints a tree into a display list, the way a window would.
bool painted_caret(voidui::WidgetTree &tree, voidui::Size<float> viewport,
                   voidui::Rect<float> &out) {
  tree.layout({viewport.width, viewport.height});
  return rendered_caret(tree, viewport, out);
}

bool paints_layout_text(voidui::WidgetTree &tree, voidui::Size<float> viewport,
                        std::string_view expected) {
  using namespace voidui;
  tree.layout({viewport.width, viewport.height});
  DisplayList list;
  Painter painter(list, viewport);
  tree.render(painter);
  for (const DrawCommand &command : list.commands())
    if (command.kind == CommandKind::Glyphs && command.text_layout &&
        command.text_layout->text() == expected)
      return true;
  return false;
}

/// The width the renderer will end up rasterising, in device pixels. It snaps
/// the two edges apart, so this is the only number that says whether a caret
/// is really the hairline it was asked to be.
float device_width(voidui::Rect<float> rect) {
  return voidui::round_half_up((rect.origin.x + rect.size.width) * kScale) -
         voidui::round_half_up(rect.origin.x * kScale);
}

bool near(float a, float b, float tolerance = 0.01f) {
  return std::fabs(a - b) <= tolerance;
}

} // namespace

int main() {
  using namespace voidui;

  std::string changed;
  WidgetTree field_tree(std::make_unique<Input>(
      input()
          .placeholder("address")
          .inline_start(text("https://"))
          .inline_end(button("clear"))
          .block_start(text("Website"))
          .block_end(text("Public URL"))
          .on_change([&](const std::string &value) { changed = value; })));
  field_tree.layout({400.0f, 200.0f});

  Node *field_node = field_tree.root();
  check(field_node && field_node->internal_children.size() == 4,
        "all four slots are mounted as internal children");
  check(field_node->internal_children[0]->part == "block-start" &&
            field_node->internal_children[1]->part == "inline-start" &&
            field_node->internal_children[2]->part == "inline-end" &&
            field_node->internal_children[3]->part == "block-end",
        "slot part names are stable");
  check(field_node->internal_children[0]->global_pos.y <
                field_node->internal_children[1]->global_pos.y &&
            field_node->internal_children[3]->global_pos.y >
                field_node->internal_children[1]->global_pos.y,
        "block slots surround the inline row");

  focus(field_tree, {100.0f, 45.0f});
  check(field_tree.wants_text_input(), "a focused input requests text input");
  TextInputEvent insert("voidui.dev");
  field_tree.process_event(insert);
  auto *field = dynamic_cast<Input *>(field_node->widget.get());
  check(field && field->value() == "voidui.dev" && changed == "voidui.dev",
        "committed text updates value and invokes on_change");

  KeyModifiers primary;
  primary.bits = KeyModifiers::Control;
  KeyPressedEvent select_all(Keycode::A, primary);
  field_tree.process_event(select_all);
  TextInputEvent replace("example.com");
  field_tree.process_event(replace);
  check(field->value() == "example.com",
        "typing replaces the focused input selection");

  WidgetTree area_tree(std::make_unique<Textarea>(
      textarea("one").inline_start(text("note")).block_end(text("3"))));
  area_tree.layout({400.0f, 200.0f});
  focus(area_tree, {80.0f, 30.0f});
  KeyPressedEvent end(Keycode::End);
  area_tree.process_event(end);
  KeyPressedEvent newline(Keycode::Return);
  area_tree.process_event(newline);
  TextInputEvent second("two");
  area_tree.process_event(second);
  auto *area = dynamic_cast<Textarea *>(area_tree.root()->widget.get());
  check(area && area->value() == "one\ntwo",
        "textarea accepts newlines and committed text");

  auto bound_component = component([] {
    auto value = use_state(std::string("bound"));
    return input(value);
  });
  WidgetTree bound_tree(
      std::make_unique<decltype(bound_component)>(std::move(bound_component)));
  bound_tree.layout({300.0f, 100.0f});
  focus(bound_tree, {40.0f, 15.0f});
  KeyPressedEvent bound_end(Keycode::End);
  bound_tree.process_event(bound_end);
  TextInputEvent bound_insert(" value");
  bound_tree.process_event(bound_insert);
  bound_tree.layout({300.0f, 100.0f});
  auto *bound =
      dynamic_cast<Input *>(bound_tree.root()->children.front()->widget.get());
  check(bound && bound->value() == "bound value",
        "State<string> binding survives component reconciliation");

  auto styled = StyleParser::parse(R"vss(
    input { inline-gap: 13; caret-color: #ff0000; }
    input::part(inline-start) { color: #123456; }
  )vss");
  check(styled.diagnostics.empty(), "input properties and parts parse in VSS");
  field_tree.set_stylesheet(styled.sheet);
  const Color expected(0x12, 0x34, 0x56);
  const auto &brush = field_node->internal_children[1]
                          ->style_node.computed->get<styles::Foreground>();
  const Color actual = std::get<Color>(brush);
  check(actual.r == expected.r && actual.g == expected.g &&
            actual.b == expected.b && actual.a == expected.a,
        "VSS reaches a named input slot through ::part");

  // -- the caret --------------------------------------------------------------
  //
  // Everything below paints into a real display list at 1.25x. A caret is one
  // rectangle, and every way it used to be wrong -- misplaced, the wrong
  // height, clipped away, a different thickness at every letter, or simply
  // never blinking -- is visible in that rectangle.

  const Size<float> viewport{400.0f, 200.0f};

  // Taller than one line, so the vertical centring is doing real work and a
  // caret placed against the wrong height has room to land in the wrong place.
  WidgetTree caret_tree(
      std::make_unique<Input>(input()
                                  .height(Length::Fixed{60.0f})
                                  .caret_color(kCaretInk)
                                  .caret_blink(0.0f)));
  caret_tree.set_device_scale(kScale);
  caret_tree.layout({viewport.width, viewport.height});
  focus(caret_tree, {60.0f, 30.0f});

  Rect<float> empty_caret;
  check(painted_caret(caret_tree, viewport, empty_caret),
        "a focused empty field paints a caret");

  TextInputEvent letter("A");
  caret_tree.process_event(letter);
  Rect<float> typed_caret;
  check(painted_caret(caret_tree, viewport, typed_caret),
        "a focused field with text paints a caret");

  // The empty field has no text layout to centre against. Measuring the line
  // box instead is what keeps its caret on the same row as the first character
  // rather than starting half way down the box and hanging out of the bottom.
  check(near(empty_caret.origin.y, typed_caret.origin.y) &&
            near(empty_caret.size.height, typed_caret.size.height),
        "an empty field's caret sits exactly where the first character's will");
  check(typed_caret.origin.x > empty_caret.origin.x,
        "the caret advances past the character that was typed");

  // A textarea is many lines tall, so nothing clamps a misplaced caret back
  // into position: an empty one that centred against its own zero-height
  // layout started half way down the box and hung out of the bottom.
  WidgetTree empty_area_tree(std::make_unique<Textarea>(
      textarea().caret_color(kCaretInk).caret_blink(0.0f)));
  empty_area_tree.set_device_scale(kScale);
  empty_area_tree.layout({viewport.width, viewport.height});
  focus(empty_area_tree, {60.0f, 30.0f});
  Rect<float> empty_area_caret;
  check(painted_caret(empty_area_tree, viewport, empty_area_caret),
        "a focused empty textarea paints a caret");

  TextInputEvent area_letter("A");
  empty_area_tree.process_event(area_letter);
  Rect<float> typed_area_caret;
  check(painted_caret(empty_area_tree, viewport, typed_area_caret),
        "a textarea with text paints a caret");
  check(near(empty_area_caret.origin.y, typed_area_caret.origin.y) &&
            near(empty_area_caret.size.height, typed_area_caret.size.height),
        "an empty textarea's caret is on its first line, not down the middle");

  TextInputEvent word("Wij mixed");
  caret_tree.process_event(word);
  check(near(device_width(typed_caret), 1.0f),
        "the caret rasterises one device pixel wide");
  for (int i = 0; i < 9; ++i) {
    Rect<float> caret;
    check(painted_caret(caret_tree, viewport, caret), "caret follows Left");
    check(near(device_width(caret), 1.0f),
          "the caret keeps one device pixel of width at every offset");
    check(near(caret.size.height, typed_caret.size.height),
          "the caret keeps its height at every offset");
    KeyPressedEvent left(Keycode::Left);
    caret_tree.process_event(left);
  }

  // -- the caret stays inside a field it has overflowed -----------------------

  WidgetTree scroll_tree(
      std::make_unique<Input>(input(std::string(120, 'm'))
                                  .width(Length::Fixed{140.0f})
                                  .caret_color(kCaretInk)
                                  .caret_blink(0.0f)));
  scroll_tree.set_device_scale(kScale);
  scroll_tree.layout({viewport.width, viewport.height});
  focus(scroll_tree, {70.0f, 18.0f});
  KeyPressedEvent to_end(Keycode::End);
  scroll_tree.process_event(to_end);

  Rect<float> end_caret;
  check(painted_caret(scroll_tree, viewport, end_caret),
        "the caret at the end of an overflowing field is still painted");

  const auto &scroll_style = *scroll_tree.root()->style_node.computed;
  const auto scroll_padding = scroll_style.get<styles::Padding>();
  const float scroll_border = scroll_style.get<styles::BorderWidth>();
  const float editor_left = scroll_padding.left + scroll_border;
  const float editor_right = 140.0f - scroll_padding.right - scroll_border;
  check(end_caret.origin.x >= editor_left - 0.01f &&
            end_caret.origin.x + end_caret.size.width <= editor_right + 0.01f,
        "the caret at the end of the text is inside the clip, not on its edge");
  check(end_caret.origin.x > editor_left + 50.0f,
        "the field scrolled to follow the caret to the end of the text");

  // Home only asks for a repaint. Scrolling used to happen during layout, so
  // the view stayed where it was and the caret left the field entirely.
  KeyPressedEvent to_home(Keycode::Home);
  scroll_tree.process_event(to_home);
  Rect<float> home_caret;
  check(rendered_caret(scroll_tree, viewport, home_caret),
        "the caret is painted after Home");
  check(near(home_caret.origin.x, editor_left, 1.0f),
        "the view scrolls back with the caret on a plain repaint");

  // -- parent placement does not detach the editor from its input ------------

  // This is the same shape as examples/button.cpp: a transparent component
  // wraps an auto-centred column, and changing the input also changes the
  // width of a sibling. Containers measure children before placing them, so
  // editor geometry must remain local to the input while those placements
  // move its node around.
  auto moving_component = component([] {
    auto value = use_state(std::string());
    return column(input(value)
                      .inline_start(text("Https://"))
                      .caret_color(kCaretInk)
                      .caret_blink(0.0f),
                  text("URL: " + value.get()));
  });
  WidgetTree moving_tree(std::make_unique<decltype(moving_component)>(
      std::move(moving_component)));
  const auto centered =
      StyleParser::parse("column { margin-left: auto; margin-right: auto; }");
  check(centered.diagnostics.empty(), "centred input test style parses");
  moving_tree.set_stylesheet(centered.sheet);
  moving_tree.set_device_scale(kScale);
  moving_tree.layout({viewport.width, viewport.height});

  auto moving_nodes = [&] {
    Node *column_node = moving_tree.root()->children.front().get();
    Node *input_node = column_node->children.front().get();
    Node *prefix_node = input_node->internal_children.front().get();
    return std::array<Node *, 2>{input_node, prefix_node};
  };
  auto expected_editor_left = [&] {
    const auto [input_node, prefix_node] = moving_nodes();
    (void)input_node;
    return prefix_node->global_pos.x + prefix_node->size.width + 8.0f;
  };

  auto [moving_input, moving_prefix] = moving_nodes();
  focus(moving_tree,
        {expected_editor_left() + 2.0f,
         moving_input->global_pos.y + moving_input->size.height * 0.5f});
  Rect<float> initially_placed_caret;
  check(rendered_caret(moving_tree, viewport, initially_placed_caret),
        "an initially centred input paints its caret");
  check(near(initially_placed_caret.origin.x, expected_editor_left(), 1.0f),
        "initial parent placement keeps the caret after inline-start");
  const auto initial_ime = moving_tree.text_input_area();
  check(initial_ime &&
            near(initial_ime->rect.origin.x, expected_editor_left(), 1.0f) &&
            near(initial_ime->cursor, 0.0f, 1.0f),
        "IME candidates start beside the initial caret");

  // Make the bound sibling wider than the input, then delete the selection.
  // The column moves first left and then right again. The text and caret must
  // follow both moves instead of retaining either old global position.
  TextInputEvent long_value(std::string(80, 'W'));
  moving_tree.process_event(long_value);
  Rect<float> long_caret;
  check(painted_caret(moving_tree, viewport, long_caret),
        "the widened component still paints its caret");
  const auto scrolled_ime = moving_tree.text_input_area();
  check(scrolled_ime && scrolled_ime->cursor >= 0.0f &&
            scrolled_ime->cursor <= scrolled_ime->rect.size.width,
        "IME candidates follow a horizontally scrolled caret");
  KeyPressedEvent select_moving_all(Keycode::A, primary);
  moving_tree.process_event(select_moving_all);
  KeyPressedEvent erase_moving(Keycode::Backspace);
  moving_tree.process_event(erase_moving);

  Rect<float> replaced_caret;
  check(painted_caret(moving_tree, viewport, replaced_caret),
        "the input paints its caret after deleting a selection");
  check(near(replaced_caret.origin.x, expected_editor_left(), 1.0f),
        "selection deletion keeps text and caret after inline-start");
  const auto replaced_ime = moving_tree.text_input_area();
  check(replaced_ime &&
            near(replaced_ime->rect.origin.x, expected_editor_left(), 1.0f) &&
            near(replaced_ime->cursor, 0.0f, 1.0f),
        "IME candidates follow the input after selection deletion");

  WidgetTree transformed_ime_tree(std::make_unique<Input>(
      input("ime").caret_color(kCaretInk).caret_blink(0.0f)));
  const auto transformed_ime_style =
      StyleParser::parse("input { transform: translateY(7px); }");
  check(transformed_ime_style.diagnostics.empty(),
        "transformed IME test style parses");
  transformed_ime_tree.set_stylesheet(transformed_ime_style.sheet);
  transformed_ime_tree.set_device_scale(kScale);
  transformed_ime_tree.layout({viewport.width, viewport.height});
  focus(transformed_ime_tree, {30.0f, 25.0f});
  Rect<float> transformed_caret;
  check(painted_caret(transformed_ime_tree, viewport, transformed_caret),
        "a transformed input paints its caret");
  const auto transformed_ime = transformed_ime_tree.text_input_area();
  check(transformed_ime &&
            near(transformed_ime->rect.origin.y, transformed_caret.origin.y,
                 1.0f) &&
            near(transformed_ime->rect.origin.x + transformed_ime->cursor,
                 transformed_caret.origin.x, 1.0f),
        "IME candidates follow the input's visual transform");

  // -- IME composition uses the control's own text renderer -----------------

  WidgetTree composition_tree(std::make_unique<Input>(
      input("old").caret_color(kCaretInk).caret_blink(0.0f)));
  composition_tree.set_device_scale(kScale);
  composition_tree.layout({viewport.width, viewport.height});
  focus(composition_tree, {30.0f, 18.0f});
  KeyPressedEvent select_composition_target(Keycode::A, primary);
  composition_tree.process_event(select_composition_target);

  TextEditingEvent composing("pin yin", 3, 0);
  composition_tree.process_event(composing);
  Rect<float> composition_caret;
  check(painted_caret(composition_tree, viewport, composition_caret),
        "IME composition paints a framework caret");
  check(paints_layout_text(composition_tree, viewport, "pin yin"),
        "IME composition is shaped by the input's TextLayout");
  auto *composition_input =
      dynamic_cast<Input *>(composition_tree.root()->widget.get());
  check(composition_input && composition_input->value() == "old",
        "IME composition stays transient until it is committed");
  const auto composition_ime = composition_tree.text_input_area();
  check(composition_ime && composition_ime->cursor > 0.0f &&
            composition_ime->cursor < composition_ime->rect.size.width,
        "the candidate anchor follows the caret inside composition text");

  TextInputEvent commit_composition("拼音");
  composition_tree.process_event(commit_composition);
  check(composition_input->value() == "拼音",
        "committed IME text replaces the original selection once");

  TextEditingEvent cancelled_composition("cancel", 6, 0);
  composition_tree.process_event(cancelled_composition);
  TextEditingEvent clear_composition("", 0, 0);
  composition_tree.process_event(clear_composition);
  check(composition_input->value() == "拼音",
        "cancelling IME composition leaves the committed value unchanged");

  // -- blinking ---------------------------------------------------------------

  WidgetTree blink_tree(
      std::make_unique<Input>(input("hi").caret_color(kCaretInk)));
  blink_tree.set_device_scale(kScale);
  blink_tree.layout({viewport.width, viewport.height});
  focus(blink_tree, {60.0f, 18.0f});

  auto caret_at = [&](double when) {
    blink_tree.advance_animations(when);
    Rect<float> caret;
    return painted_caret(blink_tree, viewport, caret);
  };

  check(caret_at(100.0), "the caret is solid on the frame it last moved");
  check(blink_tree.next_wake_time() > 100.0 &&
            blink_tree.next_wake_time() <= 100.53 + 0.001,
        "a blinking caret arms the scheduler for its next toggle");
  check(!caret_at(100.6), "the caret is off half a blink later");
  check(caret_at(101.2), "the caret comes back a blink after that");

  TextInputEvent typing("!");
  blink_tree.process_event(typing);
  check(caret_at(101.3),
        "typing restarts the blink solid instead of leaving a gap");

  Rect<float> unused;
  check(painted_caret(caret_tree, viewport, unused) &&
            caret_tree.next_wake_time() ==
                std::numeric_limits<double>::infinity(),
        "caret-blink: 0 paints a steady caret and never wakes the scheduler");

  // -- vertical motion keeps its column ---------------------------------------

  WidgetTree column_tree(std::make_unique<Textarea>(
      textarea("aaaaaaaaaa\nbb\ncccccccccc").caret_color(kCaretInk)));
  column_tree.set_device_scale(kScale);
  column_tree.layout({viewport.width, viewport.height});
  // Past the right end of the first line: a press on the padding belongs to
  // the nearest edge of the text, and used to be ignored outright.
  focus(column_tree, {column_tree.root()->size.width - 2.0f, 22.0f});
  auto *column = dynamic_cast<Textarea *>(column_tree.root()->widget.get());
  check(column && column->text_selection().second == 10,
        "a press past the end of a line puts the caret at that end");

  KeyPressedEvent down_once(Keycode::Down);
  column_tree.process_event(down_once);
  check(column->text_selection().second == 13,
        "Down clamps to the end of a shorter line");
  KeyPressedEvent down_twice(Keycode::Down);
  column_tree.process_event(down_twice);
  check(
      column->text_selection().second == 24,
      "Down returns to the column the run started from, not the short line's");

  KeyPressedEvent up_1(Keycode::Up);
  column_tree.process_event(up_1);
  KeyPressedEvent up_2(Keycode::Up);
  column_tree.process_event(up_2);
  check(column->text_selection().second == 10,
        "Up climbs back to the same column");
  KeyPressedEvent up_3(Keycode::Up);
  column_tree.process_event(up_3);
  check(column->text_selection().second == 0,
        "Up on the first line goes to the start of the text");

  std::puts("input self-test passed");
  return 0;
}
