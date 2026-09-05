#include "voidui/core/component.h"
#include "voidui/widgets/button.h"
#include "voidui/widgets/column.h"
#include "voidui/widgets/input.h"
#include "voidui/widgets/modal.h"
#include "voidui/widgets/tooltip.h"

#include <cmath>
#include <cstdio>
#include <optional>

using namespace voidui;
using namespace std::chrono_literals;
namespace {
int failures = 0;
void check(bool condition, const char *message) {
  if (!condition) {
    ++failures;
    std::fprintf(stderr, "FAIL: %s\n", message);
  }
}
Node *find(Node *node, std::string_view id) {
  if (node->widget->style_id() == id)
    return node;
  for (auto &child : node->children)
    if (auto *n = find(child.get(), id))
      return n;
  for (auto &child : node->internal_children)
    if (auto *n = find(child.get(), id))
      return n;
  return nullptr;
}
Node *get(WidgetTree &tree, std::string_view id) {
  return find(tree.root(), id);
}
void open(WidgetTree &tree, std::string_view id, bool value) {
  static_cast<Overlay *>(get(tree, id)->widget.get())->open(value);
  tree.request_layout();
}
DisplayList frame(WidgetTree &tree, double time = 1) {
  tree.advance_animations(time);
  if (tree.needs_layout())
    tree.layout({640, 480});
  DisplayList list;
  Painter painter(list, {640, 480});
  tree.render(painter);
  return list;
}
Point<float> point(Node *node) {
  return {node->global_pos.x + node->size.width * 0.5f,
          node->global_pos.y + node->size.height * 0.5f};
}
void press(WidgetTree &tree, Point<float> pos) {
  MouseMovedEvent move(pos);
  tree.process_event(move);
  MousePressedEvent down(MouseButton::Left, pos);
  tree.process_event(down);
  MouseReleasedEvent up(MouseButton::Left, pos);
  tree.process_event(up);
}
void key(WidgetTree &tree, Keycode code, bool shift = false) {
  KeyPressedEvent down(
      code, KeyModifiers{shift ? KeyModifiers::Shift : std::uint8_t{0}});
  tree.process_event(down);
  KeyReleasedEvent up(code);
  tree.process_event(up);
}
bool focused(WidgetTree &tree, std::string_view id) {
  return get(tree, id)->status.is_focused();
}
bool paints(const DisplayList &list, Node *node) {
  return std::any_of(
      list.commands().begin(), list.commands().end(), [&](const auto &c) {
        return std::abs(c.rect.origin.x - node->global_pos.x) < .01f &&
               std::abs(c.rect.origin.y - node->global_pos.y) < .01f &&
               std::abs(c.rect.size.width - node->size.width) < .01f &&
               std::abs(c.rect.size.height - node->size.height) < .01f;
      });
}
} // namespace

int main() {
  static_assert(std::same_as<decltype(modal(column()).open(true)), Modal &&>);
  for (bool remove_on_close : {false, true}) {
    for (int close_mode = 0; close_mode < 3; ++close_mode) {
      WidgetTree tree(transfer_widget(component([remove_on_close] {
        auto showing = use_state(false);
        auto page = column(
            tooltip(button("Open dialog").id("opener").on_click([showing] {
              showing.set(true);
            }),
                    "Open help")
                .id("hint"),
            button("Other").id("other"));
        if (!remove_on_close || showing.get())
          page.add(modal(button("Close").id("close").on_click(
                             [showing] { showing.set(false); }))
                       .open(showing)
                       .close_on_outside_press(true));
        return page;
      })));
      frame(tree, 1);
      press(tree, point(get(tree, "opener")));
      frame(tree, 2);
      MouseMovedEvent away({620, 460});
      tree.process_event(away);
      if (close_mode == 0)
        key(tree, Keycode::Escape);
      else if (close_mode == 1)
        press(tree, point(get(tree, "close")));
      else
        press(tree, {620, 460});
      frame(tree, 3);
      auto *bubble = get(tree, "hint")->internal_children[0].get();
      check(focused(tree, "opener"),
            "closing modal still restores opener focus");
      check(!paints(frame(tree, 4), bubble),
            "restored focus must not reopen opener tooltip");
      check(!tree.needs_paint(), "silent focus restoration allows idle sleep");
      MouseMovedEvent hover(point(get(tree, "opener")));
      tree.process_event(hover);
      check(!paints(frame(tree, 4.1), bubble),
            "hover after restoration still observes tooltip delay");
      check(paints(frame(tree, 4.6), bubble),
            "new hover can show tooltip after modal closes");
      tree.process_event(away);
      check(!paints(frame(tree, 5), bubble),
            "tooltip disappears on hover exit despite restored focus");
      key(tree, Keycode::Tab);
      check(focused(tree, "other"),
            "restored focus retains normal Tab position");
      key(tree, Keycode::Tab, true);
      check(paints(frame(tree, 5.1), bubble),
            "deliberate keyboard focus still shows tooltip immediately");
    }
  }
  {
    int page_clicks = 0, a_clicks = 0, b_clicks = 0;
    int a_closed = 0, b_closed = 0, popup_closed = 0;
    auto view = column(
        button("Page").on_click([&] { ++page_clicks; }).id("page"),
        modal(column(button("A first").on_click([&] { ++a_clicks; }).id("a1"),
                     button("A second").id("a2"),
                     overlay(button("Menu"))
                         .open(false)
                         .close_on_escape(true)
                         .on_close([&](auto) { ++popup_closed; })
                         .id("menu"),
                     modal(column(button("B first")
                                      .on_click([&] { ++b_clicks; })
                                      .id("b1"),
                                  button("B second").id("b2")))
                         .width(260)
                         .on_close([&](auto) { ++b_closed; })
                         .id("b")))
            .width(420)
            .on_close([&](auto) { ++a_closed; })
            .id("a"));
    WidgetTree tree(transfer_widget(std::move(view)));
    frame(tree);
    press(tree, point(get(tree, "page")));
    check(focused(tree, "page"), "page trigger can receive focus");
    open(tree, "a", true);
    frame(tree);
    check(focused(tree, "a1"), "opening modal focuses its first control");
    const int before = page_clicks;
    press(tree, point(get(tree, "page")));
    check(page_clicks == before, "modal backdrop prevents background clicks");
    key(tree, Keycode::Tab);
    check(focused(tree, "a1"), "Tab enters active modal after backdrop press");
    key(tree, Keycode::Tab);
    check(focused(tree, "a2"), "Tab advances within active modal");
    key(tree, Keycode::Tab);
    check(focused(tree, "a1"), "Tab wraps and skips closed child modal");
    key(tree, Keycode::Tab, true);
    check(focused(tree, "a2"), "Shift Tab wraps backwards");
    key(tree, Keycode::Tab);
    key(tree, Keycode::Return);
    check(a_clicks == 1, "keyboard activation works on modal buttons");
    open(tree, "b", true);
    frame(tree);
    check(focused(tree, "b1"), "nested modal takes focus");
    key(tree, Keycode::Tab);
    key(tree, Keycode::Tab);
    check(focused(tree, "b1"), "nested modal isolates Tab navigation");
    key(tree, Keycode::Escape);
    frame(tree);
    check(b_closed == 1 && a_closed == 0, "Escape closes only topmost modal");
    check(focused(tree, "a1"), "closing nested modal restores parent focus");
    open(tree, "menu", true);
    frame(tree);
    key(tree, Keycode::Escape);
    frame(tree);
    check(popup_closed == 1 && a_closed == 0,
          "Escape closes menu before its modal");
    key(tree, Keycode::Escape);
    frame(tree);
    check(a_closed == 1 && focused(tree, "page"),
          "closing last modal restores page trigger");
    check(!paints(frame(tree), get(tree, "a")),
          "dismissed true declaration stays closed");
    open(tree, "a", false);
    frame(tree);
    open(tree, "a", true);
    frame(tree);
    check(focused(tree, "a1"), "false-to-true reopens modal");
  }
  {
    // Reverse declaration order must not decide activation order, including
    // after a component reconciliation recreates the declarations.
    int first = 0, second = 0;
    std::optional<State<bool>> rerender;
    WidgetTree tree(transfer_widget(component([&] {
      auto value = use_state(false);
      rerender = value;
      return column(button("Page"),
                    modal(button("Earlier"))
                        .open(false)
                        .on_close([&](auto) { ++first; })
                        .id("first"),
                    modal(button("Later"))
                        .open(false)
                        .on_close([&](auto) { ++second; })
                        .id("second"));
    })));
    frame(tree);
    open(tree, "second", true);
    frame(tree);
    open(tree, "first", true);
    auto list = frame(tree);
    check(paints(list, get(tree, "first")),
          "earlier-declared modal can open last");
    key(tree, Keycode::Escape);
    frame(tree);
    check(first == 1 && second == 0,
          "activation order wins over declaration order");
    key(tree, Keycode::Escape);
    frame(tree);
    check(second == 1, "older sibling modal remains under newer modal");
  }
  {
    int page = 0, closed = 0;
    WidgetTree tree(transfer_widget(
        column(button("Page").on_click([&] { ++page; }).id("page"),
               modal(button("Inside"))
                   .open(true)
                   .close_on_outside_press(true)
                   .on_close([&](auto reason) {
                     check(reason == OverlayDismissReason::OutsidePress,
                           "outside close reports reason");
                     ++closed;
                   })
                   .id("dialog"))));
    frame(tree);
    press(tree, point(get(tree, "page")));
    frame(tree);
    check(closed == 1 && page == 0,
          "outside dismissal consumes press without click-through");
    press(tree, point(get(tree, "page")));
    check(page == 1, "next deliberate press reaches page");
  }
  {
    WidgetTree tree(transfer_widget(
        column(tooltip(button("Page tip"), "Page hint").delay(500ms).id("tip"),
               modal(column(tooltip(button("Modal tip"), "Modal hint")
                                .delay(0ms)
                                .id("inner"),
                            modal(button("Deepest")).id("deep")))
                   .id("dialog"))));
    frame(tree, 10);
    MouseMovedEvent move(point(get(tree, "tip")));
    tree.process_event(move);
    frame(tree, 10.1);
    open(tree, "dialog", true);
    frame(tree, 10.2);
    Node *page_bubble = get(tree, "tip")->internal_children[0].get();
    check(!paints(frame(tree, 11), page_bubble),
          "background delayed tooltip cannot rise over modal");
    Node *inner = get(tree, "inner");
    MouseMovedEvent inner_move(point(inner));
    tree.process_event(inner_move);
    Node *inner_bubble = inner->internal_children[0].get();
    check(paints(frame(tree, 11.1), inner_bubble),
          "modal tooltip paints above its modal");
    open(tree, "deep", true);
    check(!paints(frame(tree, 11.2), inner_bubble),
          "child modal dismisses parent's tooltip");
  }
  {
    std::optional<State<bool>> a, b;
    WidgetTree tree(transfer_widget(component([&] {
      auto first = use_state(false);
      a = first;
      auto second = use_state(false);
      b = second;
      return column(
          button("Page").id("page"),
          modal(column(button("A").id("a1"),
                       modal(button("B").id("b1")).open(second).id("b")))
              .open(first)
              .id("a"));
    })));
    frame(tree);
    a->set(true);
    frame(tree);
    b->set(true);
    frame(tree);
    key(tree, Keycode::Escape);
    frame(tree);
    check(!b->get() && a->get(),
          "State binding synchronizes child close without closing parent");
    b->set(true);
    frame(tree);
    a->set(false);
    frame(tree);
    check(!b->get(), "closing parent synchronizes descendant state");
    a->set(true);
    frame(tree);
    check(!paints(frame(tree), get(tree, "b")),
          "closed child does not unexpectedly reopen");
  }
  {
    std::optional<State<bool>> retain;
    WidgetTree tree(transfer_widget(component([&] {
      auto keep = use_state(true);
      retain = keep;
      auto content = column(button("Page").id("page"));
      if (keep.get())
        content.add(modal(button("Focus")).open(true).id("modal"));
      return content;
    })));
    frame(tree);
    retain->set(false);
    frame(tree);
    key(tree, Keycode::Tab);
    check(focused(tree, "page"),
          "removing focused modal leaves no stale focus or stack pointers");
    tree.build(nullptr);
    frame(tree);
  }
  {
    // An older declaration opened later stays on top through keyed reorder.
    std::optional<State<bool>> a, b, reverse;
    WidgetTree tree(transfer_widget(component([&] {
      auto av = use_state(false);
      a = av;
      auto bv = use_state(false);
      b = bv;
      auto rev = use_state(false);
      reverse = rev;
      auto first = modal(input().id("ai")).open(av).key("a").id("a");
      auto second = modal(input().id("bi")).open(bv).key("b").id("b");
      auto content = column(button("Page").id("page"));
      if (rev.get())
        content.add(std::move(second)).add(std::move(first));
      else
        content.add(std::move(first)).add(std::move(second));
      return content;
    })));
    frame(tree);
    press(tree, point(get(tree, "page")));
    b->set(true);
    frame(tree);
    a->set(true);
    frame(tree);
    reverse->set(true);
    frame(tree);
    check(focused(tree, "ai"),
          "keyed reorder preserves active modal and focused input");
    key(tree, Keycode::Escape);
    frame(tree);
    check(!a->get() && b->get() && focused(tree, "bi"),
          "keyed reorder retains activation order and return focus");
    a->set(true);
    frame(tree);
    b->set(false);
    frame(tree);
    check(!a->get(), "sibling modal is owned by modal active when it opened");
    check(focused(tree, "page"),
          "closing owner restores page across sibling modal scopes");
  }
  {
    // A root modal has no normal-flow box or visible anchor requirement.
    WidgetTree tree(
        transfer_widget(modal(button("Root").id("root-button")).id("root")));
    auto list = frame(tree);
    check(list.commands().empty(), "closed root modal does not draw content");
    open(tree, "root", true);
    list = frame(tree);
    check(focused(tree, "root-button") && paints(list, tree.root()),
          "root modal centers and receives focus on activation");
    key(tree, Keycode::Escape);
    frame(tree);
  }
  {
    // Modal nested in a popup must not dismiss its own owner on activation.
    WidgetTree tree(transfer_widget(
        column(button("Anchor"),
               overlay(column(button("Menu"),
                              modal(button("Inside").id("inside")).id("modal")))
                   .id("popup"))));
    frame(tree);
    open(tree, "modal", true);
    auto list = frame(tree);
    check(paints(list, get(tree, "popup")) && paints(list, get(tree, "modal")),
          "modal opened by nonmodal popup retains its owner beneath backdrop");
    check(focused(tree, "inside"), "popup-owned modal gets isolated focus");
    open(tree, "popup", false);
    frame(tree);
    check(!paints(frame(tree), get(tree, "modal")),
          "closing popup closes its child modal");
  }
  {
    int closed = 0;
    std::optional<State<bool>> remove;
    WidgetTree tree(transfer_widget(component([&] {
      auto value = use_state(false);
      remove = value;
      auto content = column(button("Focus origin").id("origin"));
      if (!value.get())
        content.add(input().id("return-target"));
      content.add(modal(input()).open(false).id("dialog"));
      return content;
    })));
    frame(tree);
    press(tree, point(get(tree, "return-target")));
    open(tree, "dialog", true);
    frame(tree);
    remove->set(true);
    frame(tree);
    key(tree, Keycode::Escape);
    frame(tree);
    key(tree, Keycode::Tab);
    check(focused(tree, "origin"),
          "removed return-focus target is never dereferenced");
  }
  std::printf("modal: %d failures\n", failures);
  return failures ? 1 : 0;
}
