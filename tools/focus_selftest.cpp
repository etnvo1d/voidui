#include "voidui/core/widget_tree.h"
#include "voidui/widgets/button.h"
#include "voidui/widgets/column.h"
#include "voidui/widgets/input.h"

#include <cstdio>
#include <cstdlib>
#include <vector>

using namespace voidui;

namespace {
void check(bool condition, const char *message) {
  if (!condition) {
    std::fprintf(stderr, "focus self-test failed: %s\n", message);
    std::exit(1);
  }
}

struct TabState {
  bool consume = false;
  std::vector<KeyModifiers> presses;
  int releases = 0;

  EventResult receive(Event &event) {
    if (event.type() == EventType::KeyReleased &&
        static_cast<KeyReleasedEvent &>(event).keycode() == Keycode::Tab)
      ++releases;
    return event.dispatch<KeyPressedEvent>([&](KeyPressedEvent &key) {
      if (key.keycode() != Keycode::Tab)
        return EventResult::Unhandled;
      presses.push_back(key.modifiers());
      key.request_layout();
      return consume ? EventResult::Handled : EventResult::Unhandled;
    });
  }
};

class Editor : public Input {
public:
  TabState tabs;
  EventResult on_event(Event &event) override {
    if (tabs.receive(event) == EventResult::Handled) {
      // Model an editor changing its text and caret while consuming Tab.
      TextInputEvent indent("\t");
      Input::on_event(indent);
      return EventResult::Handled;
    }
    return Input::on_event(event);
  }
};

class Parent : public Column {
public:
  TabState tabs;
  EventResult on_event(Event &event) override { return tabs.receive(event); }
};

void tab(WidgetTree &tree, KeyModifiers modifiers = {}) {
  KeyPressedEvent down(Keycode::Tab, modifiers);
  tree.process_event(down);
  KeyReleasedEvent up(Keycode::Tab, modifiers);
  tree.process_event(up);
}
} // namespace

int main() {
  for (bool shift : {false, true}) {
    for (int handler : {0, 1, 2}) {
      auto parent = std::make_unique<Parent>();
      auto *ancestor = parent.get();
      auto editor = std::make_unique<Editor>();
      auto *control = editor.get();
      parent->add(button("before"));
      parent->add(std::move(editor));
      parent->add(button("after"));
      WidgetTree tree(std::move(parent));
      tree.layout({400, 200});
      Node *editor_node = tree.root()->children[1].get();

      tab(tree);
      check(tree.root()->children[0]->status.is_focused(),
            "Tab with no focus enters the first control");
      tab(tree);
      check(editor_node->status.is_focused(), "Tab enters a custom editor");
      ancestor->tabs.presses.clear();
      control->tabs.consume = handler == 1;
      ancestor->tabs.consume = handler == 2;
      tree.layout({400, 200});
      check(!tree.needs_layout(), "layout request is cleared before dispatch");

      tab(tree, {shift ? KeyModifiers::Shift : std::uint8_t{0}});
      check(control->tabs.presses.size() == 1 &&
                control->tabs.presses[0].shift() == shift,
            "editor receives Tab exactly once with its Shift modifier");
      check(ancestor->tabs.presses.size() == (handler == 1 ? 0u : 1u),
            "unhandled Tab bubbles to the ancestor exactly once");
      check(tree.needs_layout(), "Tab preserves widget invalidation requests");
      if (handler == 0) {
        check(tree.root()->children[shift ? 0 : 2]->status.is_focused(),
              "unhandled Tab traverses focus in the requested direction");
        tab(tree, {shift ? KeyModifiers::Shift : std::uint8_t{0}});
        check(tree.root()->children[shift ? 2 : 0]->status.is_focused(),
              "default focus traversal still wraps");
      } else {
        check(editor_node->status.is_focused(), "handled Tab retains focus");
        // One release arrived when Tab entered the editor, one for this key.
        check(control->tabs.releases == 2, "editor receives handled Tab release");
        if (handler == 1) {
          check(control->value() == "\t", "editor can insert indentation");
          TextInputEvent text("x");
          tree.process_event(text);
          check(control->value() == "\tx", "typing uses the updated caret");
        }
      }
    }
  }

  for (auto modifier : {KeyModifiers::Control, KeyModifiers::Alt,
                        KeyModifiers::Gui}) {
    WidgetTree tree(transfer_widget(column(Editor{}, button("next"))));
    tree.layout({400, 200});
    tab(tree, {KeyModifiers::Shift});
    check(tree.root()->children[1]->status.is_focused(),
          "Shift+Tab with no focus enters the last control");
    tab(tree);
    auto *editor = static_cast<Editor *>(tree.root()->children[0]->widget.get());
    tab(tree, {modifier});
    check(tree.root()->children[0]->status.is_focused() &&
              editor->tabs.presses.size() == 1 &&
              editor->tabs.presses[0].bits == modifier,
          "modified Tab is delivered without default focus traversal");
  }
  std::puts("focus self-test passed");
}
