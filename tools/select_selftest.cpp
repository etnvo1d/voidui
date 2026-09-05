#include "voidui/core/component.h"
#include "voidui/widgets/button.h"
#include "voidui/widgets/column.h"
#include "voidui/widgets/scrollable.h"
#include "voidui/widgets/select.h"
#include <cassert>
#include <cmath>
#include <cstdio>
using namespace voidui;
namespace {
int failures = 0;
void check(bool value, const char *message) {
  if (!value) {
    ++failures;
    std::fprintf(stderr, "FAIL: %s\n", message);
  }
}
bool near(float a, float b) { return std::abs(a - b) < 0.01f; }
Node *find(Node *node, std::string_view id) {
  if (node->widget->style_id() == id)
    return node;
  for (auto &c : node->children)
    if (auto *n = find(c.get(), id))
      return n;
  for (auto &c : node->internal_children)
    if (auto *n = find(c.get(), id))
      return n;
  return nullptr;
}
Node *part(Node *node, std::string_view name) {
  for (auto &c : node->internal_children)
    if (c->part == name)
      return c.get();
  return nullptr;
}
DisplayList frame(WidgetTree &tree, Size<float> size = {480, 400}) {
  tree.advance_animations(0);
  if (tree.needs_layout())
    tree.layout({size.width, size.height});
  DisplayList list;
  Painter p(list, size);
  tree.render(p);
  return list;
}
void click(WidgetTree &tree, Point<float> p) {
  MousePressedEvent press(MouseButton::Left, p);
  tree.process_event(press);
  MouseReleasedEvent release(MouseButton::Left, p);
  tree.process_event(release);
}
void key(WidgetTree &tree, Keycode code, KeyModifiers mods = {}) {
  KeyPressedEvent event(code, mods);
  tree.process_event(event);
}
Point<float> point(Node *node) {
  return {node->global_pos.x + 8,
          node->global_pos.y + node->size.height * 0.5f};
}
std::vector<SelectOption> items() {
  return {
      {"a", "Alpha"}, {"b", "Beta", true}, {"c", "Charlie"}, {"d", "Delta"}};
}
Select *control(Node *node) {
  return static_cast<Select *>(node->widget.get());
}
} // namespace
int main() {
  {
    int changes = 0, background = 0;
    auto tree = WidgetTree(transfer_widget(
        column(
            scrollable(column(select(items()).value("a").id("select").on_change(
                           [&](const auto &) { ++changes; })))
                .size({280, 48})
                .id("viewport"),
            button("Background")
                .on_click([&] { ++background; })
                .id("background"),
            button("Next").id("next"))
            .padding(24)
            .gap(12)));
    auto *s = find(tree.root(), "select");
    auto *p = part(s, "picker");
    frame(tree);
    check(control(s)->default_stylesheet()->diagnostics().empty(),
          "default stylesheet has no parse errors");
    check(p->size.height == 0, "closed picker is not measured");
    click(tree, point(s));
    auto list = frame(tree);
    check(control(s)->is_open(), "pointer opens select");
    check(near(p->size.width, s->size.width), "picker matches trigger width");
    check(p->global_pos.y >= s->global_pos.y + s->size.height,
          "picker sits below trigger");
    auto *options = p->children[0].get();
    auto *last = options->children.back().get();
    check(last->global_pos.y > find(tree.root(), "viewport")->global_pos.y + 48,
          "option extends beyond scroll viewport");
    bool unclipped = false;
    for (const auto &cmd : list.commands()) {
      const auto clip = list.clips()[cmd.clip_index].scissor;
      if (clip.origin.y > s->global_pos.y + s->size.height &&
          clip.size.height > 0)
        unclipped = true;
    }
    check(unclipped, "display list retains painting beyond ancestor clip");
    click(tree, point(options->children[1].get()));
    frame(tree);
    check(control(s)->is_open() && control(s)->value() == "a" && changes == 0,
          "disabled option cannot commit");
    click(tree, point(last));
    frame(tree);
    check(control(s)->value() == "d" && changes == 1 && !control(s)->is_open(),
          "option outside viewport commits exactly once");
    check(s->status.is_focused(), "selection retains trigger focus");
    key(tree, Keycode::Space);
    frame(tree);
    key(tree, Keycode::Home);
    frame(tree);
    key(tree, Keycode::Down);
    frame(tree);
    check((options->children[2]->style_node.status & StatusBits::kFocused) != 0,
          "navigation skips disabled item");
    check(control(s)->value() == "d", "browsing does not commit");
    key(tree, Keycode::Escape);
    frame(tree);
    check(!control(s)->is_open() && control(s)->value() == "d",
          "Escape cancels pending navigation");
    key(tree, Keycode::Space);
    frame(tree);
    key(tree, Keycode::Home);
    key(tree, Keycode::Return);
    frame(tree);
    check(control(s)->value() == "a" && changes == 2,
          "Enter commits keyboard selection");
    key(tree, Keycode::Space);
    frame(tree);
    click(tree, {450, 350});
    frame(tree);
    check(!control(s)->is_open() && background == 0,
          "outside click closes without click-through");
    click(tree, point(s));
    frame(tree);
    key(tree, Keycode::Tab);
    frame(tree);
    check(!control(s)->is_open() &&
              find(tree.root(), "background")->status.is_focused(),
          "Tab closes picker and advances once");
    key(tree, Keycode::Tab, {KeyModifiers::Shift});
    key(tree, Keycode::C);
    frame(tree);
    check(control(s)->value() == "c", "closed select supports typeahead");
    key(tree, Keycode::Space);
    frame(tree);
    WindowFocusLostEvent lost;
    tree.process_event(lost);
    frame(tree);
    check(!control(s)->is_open(), "native focus loss dismisses picker");
  }
  {
    std::vector<SelectOption> many;
    for (int i = 0; i < 40; ++i)
      many.push_back({std::to_string(i), "Option " + std::to_string(i)});
    WidgetTree tree(transfer_widget(select(many)
                                        .value("0")
                                        .picker_max_height(150)
                                        .position(Position::Fixed)
                                        .bottom(12)
                                        .right(12)));
    frame(tree);
    auto *s = tree.root();
    auto *p = part(s, "picker");
    click(tree, point(s));
    frame(tree);
    check(p->global_pos.y < s->global_pos.y && p->size.height <= 150,
          "bottom edge flips picker up and limits height");
    key(tree, Keycode::End);
    frame(tree);
    auto *scroll = static_cast<Scrollable *>(p->widget.get());
    check(scroll->scroll_offset().y > 1000,
          "keyboard End reveals last option in long list");
    auto *last = p->children[0]->children.back().get();
    check(last->global_pos.y + last->size.height <=
              p->global_pos.y + p->size.height,
          "active last row is inside visible panel");
    key(tree, Keycode::Return);
    frame(tree);
    check(control(s)->value() == "39", "long-list keyboard selection commits");
    click(tree, point(s));
    frame(tree, {160, 120});
    tree.layout({160, 120});
    frame(tree, {160, 120});
    check(p->size.width <= 144 && p->size.height <= 104,
          "small window constrains popup");
  }
  {
    WidgetTree tree(transfer_widget(
        column(select(items()).disabled(true).id("disabled"),
               select().id("empty"), select(items()).id("normal"))));
    frame(tree);
    auto *disabled = find(tree.root(), "disabled");
    click(tree, point(disabled));
    frame(tree);
    check(!control(disabled)->is_open() && !disabled->status.is_focused(),
          "disabled control does not open or focus");
    key(tree, Keycode::Tab);
    frame(tree);
    auto *empty = find(tree.root(), "empty");
    check(empty->status.is_focused(), "tab skips disabled select");
    key(tree, Keycode::Space);
    frame(tree);
    check(!control(empty)->is_open(),
          "empty select preserves placeholder without opening empty popup");
  }
  {
    WidgetTree tree(transfer_widget(select(items()).value("a")));
    auto parsed = StyleParser::parse(R"vss(
      ::picker(select) { appearance: base-select; }
      select { appearance: none; padding: 9px 15px; }
      select:open { border-width: 3px; }
      select::picker(select) { background: #123456; }
      select:open::picker-icon { color: #112233; }
      select option:checked { padding: 13px; }
      select option:checked::checkmark { color: #654321; }
      select::picker(select):hover { border-width: 2px; }
    )vss");
    check(parsed.diagnostics.empty(), "standard select CSS parses");
    tree.set_stylesheet(parsed.sheet);
    frame(tree);
    auto *s = tree.root();
    auto *p = part(s, "picker");
    check(part(s, "picker-icon")->size.width == 0,
          "appearance none hides default arrow");
    click(tree, point(s));
    frame(tree);
    check(near(s->style_node.computed->get<styles::BorderWidth>(), 3),
          ":open reflects runtime state");
    auto *option = p->children[0]->children[0].get();
    check(near(option->style_node.computed->get<styles::Padding>().left, 13),
          "select option:checked crosses exposed picker boundary");
    auto cpp = Select::option_selector().checked().checkmark().build();
    check(cpp.matches(part(option, "checkmark")->style_node),
          "C++ checked checkmark selector matches same target");
    check(Selectors::of<Select>().open().picker_icon().build().matches(
              part(s, "picker-icon")->style_node),
          "C++ open picker icon selector matches");
    check(!Selectors::of<Select>().picker().hovered().build().matches(
              p->style_node),
          "pseudo-class after picker tests picker, not hovered host");
    auto styled = select(items())
                      .appearance(SelectAppearance::BaseSelect)
                      .color(Color(12, 34, 56))
                      .padding(17)
                      .width(260);
    WidgetTree inline_tree(transfer_widget(std::move(styled)));
    inline_tree.set_stylesheet(parsed.sheet);
    frame(inline_tree);
    check(inline_tree.root()->style_node.computed->get<Select::Appearance>() ==
                  SelectAppearance::BaseSelect &&
              near(inline_tree.root()->size.width, 260),
          "C++ inline styles override VSS");
    key(tree, Keycode::Escape);
    frame(tree);
    check(!Selectors::of<Select>().open().build().matches(s->style_node),
          "dismissal clears :open immediately");
  }
  {
    std::optional<State<int>> count;
    WidgetTree tree(transfer_widget(component([&] {
      auto state = use_state(0);
      count = state;
      return select(items()).id("select").on_change(
          [state](const auto &) { state.set(state.get() + 1); });
    })));
    frame(tree);
    auto *s = find(tree.root(), "select");
    click(tree, point(s));
    frame(tree);
    key(tree, Keycode::End);
    key(tree, Keycode::Return);
    frame(tree);
    s = find(tree.root(), "select");
    check(control(s)->value() == "d" && count->get() == 1,
          "uncontrolled value survives callback reconciliation");
    click(tree, point(s));
    frame(tree);
    count->set(2);
    frame(tree);
    s = find(tree.root(), "select");
    check(control(s)->is_open(),
          "unrelated component rebuild preserves open picker");
    key(tree, Keycode::Home);
    key(tree, Keycode::Return);
    frame(tree);
    check(control(find(tree.root(), "select"))->value() == "a",
          "reconciled callbacks refer to current select");
    auto declared = select(items()).value("c");
    WidgetTree clones(transfer_widget(column(declared, declared)));
    frame(clones);
    click(clones, point(clones.root()->children[0].get()));
    frame(clones);
    check(!control(clones.root()->children[1].get())->is_open(),
          "lvalue clones have independent runtime state");
  }
  {
    std::optional<State<std::string>> value;
    std::optional<State<bool>> disabled;
    WidgetTree tree(transfer_widget(component([&] {
      auto state = use_state(std::string("a"));
      value = state;
      auto off = use_state(false);
      disabled = off;
      return select(items(), state).disabled(off.get()).id("bound");
    })));
    frame(tree);
    auto *s = find(tree.root(), "bound");
    click(tree, point(s));
    frame(tree);
    key(tree, Keycode::End);
    key(tree, Keycode::Return);
    frame(tree);
    check(value->get() == "d", "State binding receives committed value");
    value->set("c");
    frame(tree);
    s = find(tree.root(), "bound");
    check(control(s)->value() == "c",
          "external State value updates displayed selection");
    click(tree, point(s));
    frame(tree);
    disabled->set(true);
    frame(tree);
    check(!control(find(tree.root(), "bound"))->is_open(),
          "disabling during reconciliation closes picker");
  }
  {
    WidgetTree tree(transfer_widget(select(items())
                                        .width(180)
                                        .position(Position::Absolute)
                                        .left(120)
                                        .top(80)));
    frame(tree);
    auto *s = tree.root();
    auto *p = part(s, "picker");
    click(tree, point(s));
    frame(tree);
    const float original = p->size.width;
    auto parsed = StyleParser::parse("select { transform: scale(1.25); }");
    tree.set_stylesheet(parsed.sheet);
    frame(tree);
    check(near(p->size.width, original * 1.25f),
          "paint-only anchor scale updates picker width");
    auto selector = Selectors::of<Select>().open().picker().hovered().build();
    auto roundtrip =
        StyleParser::parse(selector.to_string() + " { color: red; }");
    check(roundtrip.diagnostics.empty() &&
              roundtrip.sheet->rule(0).selector.specificity() ==
                  selector.specificity(),
          "standard pseudo-element specificity survives C++ serialization");
    check(selector.specificity().types == 2 &&
              selector.specificity().classes == 2,
          "standard pseudo-element counts as type, state as class specificity");
  }
  std::printf("select: %d failures\n", failures);
  return failures ? 1 : 0;
}
