#include "voidui/core/widget_tree.h"
#include "voidui/paint/display_list.h"
#include "voidui/paint/painter.h"
#include "voidui/widgets/button.h"
#include "voidui/widgets/text.h"

#include <cstdio>
#include <variant>

using namespace voidui;

namespace {

int failures = 0;

void check(bool condition, const char *message) {
  std::printf("  %s  %s\n", condition ? "ok  " : "FAIL", message);
  failures += condition ? 0 : 1;
}

bool is_selection_fill(const DrawCommand &command) {
  if (command.kind != CommandKind::FillRRect)
    return false;
  const Color *color = std::get_if<Color>(&command.brush);
  return color && color->to_rgba8() == Rgba8{51, 144, 255, 128};
}

DisplayList render(WidgetTree &tree, Size<float> viewport) {
  DisplayList list;
  Painter painter(list, viewport);
  tree.render(painter);
  return list;
}

} // namespace

int main() {
  std::puts("selection self-test");

  WidgetTree tree(transfer_widget(text("hello world")));
  tree.layout(Constraints(400.0f, 100.0f));
  Node *node = tree.root();
  check(node && node->size.width > 0.0f && node->size.height > 0.0f,
        "text produces selectable geometry");
  if (!node)
    return 1;

  const std::uint64_t builds_before_drag = TextLayout::builds_performed();
  const float y = node->global_pos.y + node->size.height * 0.5f;
  MousePressedEvent press(MouseButton::Left, {node->global_pos.x + 0.1f, y});
  tree.process_event(press);
  MouseMovedEvent drag({node->global_pos.x + node->size.width - 0.1f, y});
  tree.process_event(drag);
  check(tree.get_current_cursor_shape() == CursorShape::Text,
        "cursor: auto resolves selectable text to an I-beam");
  MouseReleasedEvent release(MouseButton::Left, drag.get_pos());
  tree.process_event(release);

  DisplayList selected = render(tree, {400.0f, 100.0f});
  check(!selected.commands().empty() &&
            is_selection_fill(selected.commands().front()),
        "drag selection paints its highlight before glyphs");
  check(TextLayout::builds_performed() == builds_before_drag,
        "pointer selection never reshapes text");

  KeyModifiers primary;
  primary.bits = KeyModifiers::Control;
  KeyPressedEvent select_all(Keycode::A, primary);
  tree.process_event(select_all);
  DisplayList all = render(tree, {400.0f, 100.0f});
  check(!all.commands().empty() && is_selection_fill(all.commands().front()) &&
            all.commands().front().rect.size.width > node->size.width * 0.9f,
        "Ctrl+A selects the complete text range");

  const auto word = node->widget->selection_word_at(2);
  check(word.first == 0 && word.second == 5,
        "double-click word boundaries use UTF-8 byte ranges");

  WidgetTree multiline(transfer_widget(text("first\nsecond")));
  auto multiline_style =
      StyleParser::parse("text { line-height: 40px; font-weight: bold; }");
  multiline.set_stylesheet(multiline_style.sheet);
  multiline.layout(Constraints(400.0f, 100.0f));
  Node *multiline_node = multiline.root();
  const auto &multiline_layout =
      static_cast<Text *>(multiline_node->widget.get())->text_layout();
  check(multiline_layout && multiline_layout->size().height == 80.0f,
        "line-height controls every line box");
  check(multiline_layout &&
            multiline_layout->fonts()->weight() == FontWeight::Bold,
        "font-weight selects the requested font stack");
  MousePressedEvent multiline_press(
      MouseButton::Left,
      {multiline_node->global_pos.x + 0.1f,
       multiline_node->global_pos.y + multiline_node->size.height * 0.25f});
  multiline.process_event(multiline_press);
  MouseMovedEvent multiline_drag(
      {multiline_node->global_pos.x + multiline_node->size.width - 0.1f,
       multiline_node->global_pos.y + multiline_node->size.height * 0.75f});
  multiline.process_event(multiline_drag);
  DisplayList multiline_list = render(multiline, {400.0f, 100.0f});
  int selected_lines = 0;
  for (const DrawCommand &command : multiline_list.commands())
    selected_lines += is_selection_fill(command) ? 1 : 0;
  check(selected_lines == 2,
        "a multi-line range emits one allocation-free highlight per line");

  WidgetTree unicode(transfer_widget(text("中文 test")));
  unicode.layout(Constraints(400.0f, 100.0f));
  const auto ideograph = unicode.root()->widget->selection_word_at(0);
  check(ideograph.first == 0 && ideograph.second == 3,
        "word selection keeps UTF-8 codepoint boundaries");

  WidgetTree all_tree(transfer_widget(text("select all")));
  auto all_style = StyleParser::parse("text { user-select: all; }");
  all_tree.set_stylesheet(all_style.sheet);
  all_tree.layout(Constraints(400.0f, 100.0f));
  Node *all_node = all_tree.root();
  MousePressedEvent all_press(
      MouseButton::Left,
      {all_node->global_pos.x + all_node->size.width * 0.5f,
       all_node->global_pos.y + all_node->size.height * 0.5f});
  all_tree.process_event(all_press);
  DisplayList all_click = render(all_tree, {400.0f, 100.0f});
  check(!all_click.commands().empty() &&
            is_selection_fill(all_click.commands().front()),
        "user-select: all selects the whole widget on one click");

  WidgetTree button_tree(transfer_widget(button("not selectable")));
  button_tree.layout(Constraints(400.0f, 100.0f));
  Node *label = button_tree.root()->children.front().get();
  MousePressedEvent button_press(
      MouseButton::Left, {label->global_pos.x + 1.0f,
                          label->global_pos.y + label->size.height * 0.5f});
  button_tree.process_event(button_press);
  MouseMovedEvent button_drag(
      {label->global_pos.x + label->size.width - 1.0f,
       label->global_pos.y + label->size.height * 0.5f});
  button_tree.process_event(button_drag);
  check(button_tree.get_current_cursor_shape() == CursorShape::Pointer,
        "button cursor inherits through its non-selectable label");
  DisplayList button_list = render(button_tree, {400.0f, 100.0f});
  bool button_has_selection = false;
  for (const DrawCommand &command : button_list.commands())
    button_has_selection |= is_selection_fill(command);
  check(!button_has_selection,
        "button's inherited user-select: none protects its label");

  std::printf("\n%s (%d failure%s)\n", failures ? "FAILED" : "PASSED", failures,
              failures == 1 ? "" : "s");
  return failures == 0 ? 0 : 1;
}
