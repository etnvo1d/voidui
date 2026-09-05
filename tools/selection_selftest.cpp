#include "voidui/core/widget_tree.h"
#include "voidui/paint/display_list.h"
#include "voidui/paint/painter.h"
#include "voidui/widgets/button.h"
#include "voidui/widgets/column.h"
#include "voidui/widgets/row.h"
#include "voidui/widgets/input.h"
#include "voidui/widgets/overlay.h"
#include "voidui/widgets/scrollable.h"
#include "voidui/widgets/sidebar.h"
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

Point<float> text_edge(Node *node, bool end) {
  return {node->global_pos.x + (end ? node->size.width - 0.1f : 0.1f),
          node->global_pos.y + node->size.height * 0.5f};
}

void select(WidgetTree &tree, Point<float> from, Point<float> to) {
  MousePressedEvent press(MouseButton::Left, from);
  tree.process_event(press);
  MouseMovedEvent move(to);
  tree.process_event(move);
  MouseReleasedEvent release(MouseButton::Left, to);
  tree.process_event(release);
}

int highlight_count(WidgetTree &tree) {
  int count = 0;
  for (const auto &command : render(tree, {400.0f, 400.0f}).commands())
    count += is_selection_fill(command) ? 1 : 0;
  return count;
}

Node *find_id(Node *node, std::string_view id) {
  if (node->widget->style_id() == id)
    return node;
  for (auto &child : node->children)
    if (Node *found = find_id(child.get(), id))
      return found;
  for (auto &child : node->internal_children)
    if (Node *found = find_id(child.get(), id))
      return found;
  return nullptr;
}

void sidebar_selection(SidebarPlacement placement) {
  // Keep the layout and button rows from examples/sidebar.cpp: the panel's
  // last text overlaps the vertical band occupied by the main content's buttons.
  auto panel = column(
      text("WORKSPACE").font_size(14), text("Team HQ").font_size(28),
      button("Overview"), button("Projects"), button("Documents"),
      text("Drag the panel edge.\nShort pulls keep the size.").id("panel-text"),
      button("Close panel")).padding(24).gap(16);
  const std::string keyboard =
      "Keyboard: Tab to the edge. Arrows resize; Enter toggles; Escape cancels a drag.";
  const std::string bottom = "Expanded size: 260 px\n" + keyboard;
  auto content = column(
      text("A sidebar on any edge").font_size(30).id("heading"),
      text("Pull past 48 px to reveal or resize. Pull inward past the minimum to collapse."),
      text("After opening, catch up to the new edge to widen. Reverse to shrink."),
      row(button("Left"), button("Right"), button("Top"), button("Bottom")).gap(8),
      row(button("Close"), button("Drag: elastic")).gap(8),
      row(button("Layout: docked").id("layout-button"),
          button("Closed: edge only")).gap(8),
      text("Expanded size: 260 px").id("size"), text(keyboard).id("keyboard"))
      .padding(32).gap(20).id("main");
  WidgetTree tree(transfer_widget(
      sidebar(scrollable(std::move(panel)), scrollable(std::move(content)))
          .placement(placement).open(true).extent(260).limits(150, 480)));
  tree.layout({1080.0f, 720.0f});
  Node *last = find_id(tree.root(), "keyboard");
  Node *size = find_id(tree.root(), "size");
  Node *button_node = find_id(tree.root(), "layout-button");
  const auto bottom_point = text_edge(last, true);
  MousePressedEvent press(MouseButton::Left, bottom_point);
  tree.process_event(press);
  MouseMovedEvent initial(text_edge(size, false));
  tree.process_event(initial);
  check(tree.selected_text() == bottom, "sidebar: upward drag selects the bottom two texts");

  const auto button_point = text_edge(button_node, false);
  MouseMovedEvent over_button(button_point);
  tree.process_event(over_button);
  check(tree.selected_text() == bottom,
        "sidebar: passing over Layout: docked keeps the main selection, excluding the panel");
  MouseMovedEvent gap({button_point.x,
                       button_node->global_pos.y + button_node->size.height + 5});
  tree.process_event(gap);
  check(tree.selected_text() == bottom,
        "sidebar: the gap below a button resolves within the main column");

  Node *main = find_id(tree.root(), "main");
  MouseMovedEvent padding({main->global_pos.x + main->size.width - 1, button_point.y});
  tree.process_event(padding);
  check(tree.selected_text() == bottom,
        "sidebar: content padding cannot redirect selection into the panel");
  MouseMovedEvent up(text_edge(find_id(tree.root(), "heading"), false));
  tree.process_event(up);
  const auto main_selection = tree.selected_text();
  check(main_selection.starts_with("A sidebar on any edge\n") &&
            main_selection.ends_with(bottom) &&
            main_selection.find("Team HQ") == std::string::npos,
        "sidebar: continuing upwards selects the main content through the button rows");

  Node *panel_text = find_id(tree.root(), "panel-text");
  MouseMovedEvent across(text_edge(panel_text, true));
  tree.process_event(across);
  check(tree.selected_text().find("Team HQ") != std::string::npos,
        "sidebar: explicitly dragging onto panel text still crosses containers");
  tree.process_event(over_button);
  check(tree.selected_text() == bottom,
        "sidebar: returning from the panel to the button restores the main range");
  MouseReleasedEvent release(MouseButton::Left, button_point);
  tree.process_event(release);
  check(tree.selected_text() == bottom,
        "sidebar: releasing over the button preserves the bottom selection");
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
  MouseMovedEvent outside_drag({399.0f, 99.0f});
  tree.process_event(outside_drag);
  check(tree.get_current_cursor_shape() == CursorShape::Text,
        "selection capture keeps the I-beam outside the text");
  MouseLeftEvent leave;
  tree.process_event(leave);
  check(tree.get_current_cursor_shape() == CursorShape::Text,
        "selection capture keeps the I-beam when the pointer leaves the window");
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

  WidgetTree document(transfer_widget(column(
      text("VoidNote"), text("Hello, VoidUI!"), text("last"))));
  document.layout({400.0f, 400.0f});
  Node *first = document.root()->children[0].get();
  Node *second = document.root()->children[1].get();
  Node *third = document.root()->children[2].get();
  document.process_event(select_all);
  check(document.selected_text() == "VoidNote\nHello, VoidUI!\nlast",
        "Ctrl+A also works before any pointer selection exists");
  const auto builds_before_document = TextLayout::builds_performed();
  select(document, text_edge(first, false), text_edge(second, true));
  check(document.selected_text() == "VoidNote\nHello, VoidUI!",
        "column siblings form one selection and copy with a line break");
  check(highlight_count(document) == 2,
        "each text node in the document range paints its highlight");
  select(document, text_edge(second, true), text_edge(first, false));
  check(document.selected_text() == "VoidNote\nHello, VoidUI!",
        "reverse dragging copies in declaration order");
  select(document, text_edge(first, false), text_edge(third, true));
  check(highlight_count(document) == 3 &&
            document.selected_text() == "VoidNote\nHello, VoidUI!\nlast",
        "a drag that jumps across a middle node includes all its text");
  check(TextLayout::builds_performed() == builds_before_document,
        "cross-node dragging and painting reuse shaped text layouts");

  const auto caret = [&](Node *n, std::uint32_t offset) {
    for (float x = 0; x < n->size.width; x += 0.1f) {
      const auto point = Point<float>{n->global_pos.x + x, text_edge(n, false).y};
      if (n->widget->selection_hit_test(point, {n->global_pos, n->size}) == offset)
        return point;
    }
    check(false, "requested caret exists");
    return text_edge(n, false);
  };
  select(document, caret(first, 4), caret(second, 5));
  check(document.selected_text() == "Note\nHello",
        "only the selected portions of the two endpoint nodes are copied");
  MousePressedEvent word_press(MouseButton::Left, caret(second, 2), 2);
  document.process_event(word_press);
  MouseMovedEvent word_move(caret(third, 2));
  document.process_event(word_move);
  check(document.selected_text() == "Hello, VoidUI!\nlast",
        "double-click dragging extends across nodes by whole words");
  MouseMovedEvent word_reverse(caret(first, 2));
  document.process_event(word_reverse);
  check(document.selected_text() == "VoidNote\nHello",
        "backward word dragging retains the complete original word");
  MouseReleasedEvent word_release(MouseButton::Left, caret(second, 2));
  document.process_event(word_release);
  check(document.selected_text() == "Hello",
        "returning a word drag to its anchor preserves the original word");
  MousePressedEvent anchor(MouseButton::Left, caret(second, 5));
  document.process_event(anchor);
  MouseMovedEvent forward(text_edge(third, true));
  document.process_event(forward);
  MouseMovedEvent backward(text_edge(first, false));
  document.process_event(backward);
  check(document.selected_text() == "VoidNote\nHello",
        "reversing direction during a drag retains the original anchor");
  MouseReleasedEvent finish(MouseButton::Left, caret(second, 5));
  document.process_event(finish);
  check(document.selected_text().empty(),
        "returning to the anchor collapses the whole cross-node range");
  document.process_event(select_all);
  check(document.selected_text() == "VoidNote\nHello, VoidUI!\nlast",
        "Ctrl+A selects all selectable text in the document");
  MousePressedEvent release_only(MouseButton::Left, text_edge(first, false));
  document.process_event(release_only);
  MouseReleasedEvent far_release(MouseButton::Left, text_edge(second, true));
  document.process_event(far_release);
  check(document.selected_text() == "VoidNote\nHello, VoidUI!",
        "release updates the cross-node endpoint without a prior move");
  select(document, text_edge(first, false), {0.0f, 399.0f});
  check(document.selected_text() == "VoidNote\nHello, VoidUI!\nlast",
        "dragging below the document includes the final line at any x");

  WidgetTree exclusions(transfer_widget(column(
      text("start"), text("skip").add_class("no-select"),
      column(text("hidden"), text("included").add_class("selectable"))
          .add_class("no-select"),
      text("invisible").visibility(Visibility::Hidden),
      input("editable"), button("button"), text("end"))));
  exclusions.set_stylesheet(StyleParser::parse(
      ".no-select { user-select: none; } .selectable { user-select: text; }").sheet);
  exclusions.layout({400.0f, 400.0f});
  select(exclusions, text_edge(exclusions.root()->children.front().get(), false),
         text_edge(exclusions.root()->children.back().get(), true));
  check(exclusions.selected_text() == "start\nincluded\nend" &&
            highlight_count(exclusions) == 3,
        "ranges skip none, hidden text, inputs and buttons, honoring explicit text overrides");
  Node *editor = exclusions.root()->children[4].get();
  select(exclusions, text_edge(editor, false),
         text_edge(exclusions.root()->children.back().get(), true));
  exclusions.process_event(select_all);
  check(exclusions.selected_text() == "editable",
        "input selection and Ctrl+A remain confined to the editor");

  WidgetTree atomic(transfer_widget(column(text("before"),
      column(text("one"), text("two")).add_class("atomic"),
      text("after"))));
  atomic.set_stylesheet(StyleParser::parse(".atomic { user-select: all; }").sheet);
  atomic.layout({400.0f, 400.0f});
  Node *one = atomic.root()->children[1]->children[0].get();
  Node *two = atomic.root()->children[1]->children[1].get();
  select(atomic, text_edge(two, true), text_edge(two, true));
  check(atomic.selected_text() == "one\ntwo",
        "user-select:all selects the complete ancestor group on click");
  select(atomic, text_edge(atomic.root()->children[0].get(), false), caret(one, 1));
  check(atomic.selected_text() == "before\none\ntwo",
        "entering user-select:all includes the entire group");
  select(atomic, text_edge(two, true), text_edge(atomic.root()->children[2].get(), true));
  check(atomic.selected_text() == "one\ntwo\nafter",
        "a selection starting in user-select:all can extend beyond it");

  WidgetTree inline_text(transfer_widget(row(text("hello "), text("world"))));
  inline_text.layout({400.0f, 400.0f});
  select(inline_text, text_edge(inline_text.root()->children.front().get(), false),
         text_edge(inline_text.root()->children.back().get(), true));
  check(inline_text.selected_text() == "hello world",
        "same-row text is concatenated without an artificial newline");

  WidgetTree nested(transfer_widget(column(
      column(text("中文")), column(text("第一行\n第二行"))).id("translated")));
  nested.set_stylesheet(StyleParser::parse(
      "#translated { transform: translate(30px, 40px); }").sheet);
  nested.layout({400.0f, 400.0f});
  auto nested_from = text_edge(nested.root()->children[0]->children[0].get(), false);
  auto nested_to = text_edge(nested.root()->children[1]->children[0].get(), true);
  // The final text has two lines: finish at the right edge of its last line.
  nested_to.y += nested.root()->children[1]->children[0]->size.height * 0.25f;
  select(nested, {nested_from.x + 30, nested_from.y + 40},
         {nested_to.x + 30, nested_to.y + 40});
  check(nested.selected_text() == "中文\n第一行\n第二行" &&
            highlight_count(nested) == 3,
        "nested transformed text preserves UTF-8 and internal line breaks");

  WidgetTree portals(transfer_widget(column(text("page"),
      overlay(column(text("popup first"), text("popup second")))
          .open(true).close_on_outside_press(false))));
  portals.layout({400.0f, 400.0f});
  portals.process_event(select_all);
  check(portals.selected_text() == "page",
        "page selection excludes text belonging to an independent portal");
  Node *portal = portals.root()->children[1].get();
  select(portals, text_edge(portal->children[0]->children[0].get(), false),
         text_edge(portal->children[0]->children[1].get(), true));
  portals.process_event(select_all);
  check(portals.selected_text() == "popup first\npopup second",
        "a portal owns its own cross-node selection and Ctrl+A scope");
  static_cast<Overlay *>(portal->widget.get())->open(false);
  portals.request_layout();
  portals.layout({400.0f, 400.0f});
  render(portals, {400.0f, 400.0f});
  check(portals.selected_text().empty(),
        "closing a selected portal discards its document range");
  document.set_stylesheet(StyleParser::parse("text { user-select: none; }").sheet);
  check(document.selected_text().empty() && highlight_count(document) == 0,
        "changing selection eligibility clears a stale range");
  inline_text.build(transfer_widget(text("replacement")));
  inline_text.layout({400.0f, 400.0f});
  check(inline_text.selected_text().empty(),
        "rebuilding selected nodes clears their pointers safely");

  sidebar_selection(SidebarPlacement::Left);
  sidebar_selection(SidebarPlacement::Right);

  std::printf("\n%s (%d failure%s)\n", failures ? "FAILED" : "PASSED", failures,
              failures == 1 ? "" : "s");
  return failures == 0 ? 0 : 1;
}
