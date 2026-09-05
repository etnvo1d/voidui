#include "voidui/core/component.h"
#include "voidui/widgets/button.h"
#include "voidui/widgets/column.h"
#include "voidui/widgets/input.h"
#include "voidui/widgets/scrollable.h"
#include "voidui/widgets/tooltip.h"

#include <cmath>
#include <cstdio>
#include <optional>

using namespace voidui;
using namespace std::chrono_literals;
namespace {
int failures = 0;
void check(bool value, const char *message) {
  if (!value) {
    ++failures;
    std::fprintf(stderr, "FAIL: %s\n", message);
  }
}
bool near(float a, float b) { return std::abs(a - b) < 0.01f; }
struct Log {
  std::vector<int> drawn;
  int presses = 0;
  int last_press = 0;
  int measures[8]{};
};
class Probe : public Widget {
public:
  Probe(int id, Log &log, Size<float> size)
      : id_(id), log_(&log), size_(size) {}
  void register_children(Registrar &) override {}
  Size<float> layout(Constraints c, LayoutContext &) override {
    ++log_->measures[id_];
    return {std::clamp(size_.width, c.min_width, c.max_width),
            std::clamp(size_.height, c.min_height, c.max_height)};
  }
  void draw(const DrawContext &ctx, Painter &painter) override {
    log_->drawn.push_back(id_);
    painter.fill_rect(ctx.bounds, Paint(Color(10 * id_, 100, 100)));
  }
  bool focusable() const override { return true; }
  EventResult on_event(Event &e) override {
    if (e.type() == EventType::MousePressed) {
      ++log_->presses;
      log_->last_press = id_;
      return EventResult::Handled;
    }
    return EventResult::Unhandled;
  }
  std::unique_ptr<Widget> clone() const override {
    return std::make_unique<Probe>(id_, *log_, size_);
  }

private:
  int id_;
  Log *log_;
  Size<float> size_;
};
DisplayList render(WidgetTree &tree, Log &log, double time = 0) {
  tree.advance_animations(time);
  if (tree.needs_layout())
    tree.layout({300, 200});
  log.drawn.clear();
  DisplayList list;
  Painter painter(list, {300, 200});
  tree.render(painter);
  return list;
}
void move(WidgetTree &tree, Point<float> point) {
  MouseMovedEvent event(point);
  tree.process_event(event);
}
void press(WidgetTree &tree, Point<float> point) {
  MousePressedEvent event(MouseButton::Left, point);
  tree.process_event(event);
  MouseReleasedEvent release(MouseButton::Left, point);
  tree.process_event(release);
}
void sheet(WidgetTree &tree, std::string_view source) {
  auto parsed = StyleParser::parse(source, "overlay.test.vss");
  check(parsed.diagnostics.empty(), "test stylesheet parses");
  tree.set_stylesheet(parsed.sheet);
}
Node *find(Node *node, std::string_view id) {
  if (node->widget->style_id() == id)
    return node;
  for (auto &child : node->children)
    if (auto *found = find(child.get(), id))
      return found;
  for (auto &child : node->internal_children)
    if (auto *found = find(child.get(), id))
      return found;
  return nullptr;
}
bool drawn(const Log &log, int id) {
  return std::find(log.drawn.begin(), log.drawn.end(), id) != log.drawn.end();
}
} // namespace

int main() {
  {
    Log log;
    auto view =
        column(scrollable(column(Probe(1, log, {40, 20}),
                                 overlay(Probe(2, log, {80, 30})).id("popup")))
                   .size({60, 35}),
               Probe(3, log, {300, 200})
                   .position(Position::Fixed)
                   .left(0)
                   .top(0)
                   .z_index(100000))
            .padding(40)
            .opacity(0.5f);
    WidgetTree tree(transfer_widget(std::move(view)));
    auto list = render(tree, log);
    const Node *popup = find(tree.root(), "popup");
    const Node *anchor = popup->parent;
    check(near(anchor->size.width, 40) && near(anchor->size.height, 20),
          "overlay contributes neither size nor gap to normal flow");
    check(log.drawn.back() == 2,
          "overlay paints above the highest ordinary z-index");
    const auto &command = list.commands().back();
    const auto clip = list.clips()[command.clip_index].scissor;
    check(near(clip.origin.x, popup->global_pos.x) &&
              near(clip.origin.y, popup->global_pos.y) &&
              near(clip.size.width, 80) && near(clip.size.height, 30),
          "overlay content uses its own clip beyond the scroll viewport");
    check(near(command.opacity, 1),
          "ancestor opacity does not dim the overlay");
    press(tree, {popup->global_pos.x + 5, popup->global_pos.y + 20});
    check(log.last_press == 2,
          "overlay is hittable outside the scroll viewport");
    static_cast<Overlay *>(find(tree.root(), "popup")->widget.get())
        ->interactive(false);
    press(tree, {popup->global_pos.x + 5, popup->global_pos.y + 20});
    check(log.last_press == 3,
          "noninteractive overlay passes hits to ordinary content");
  }
  {
    Log log;
    WidgetTree tree(
        transfer_widget(column(Probe(1, log, {40, 20}),
                               overlay(Probe(2, log, {80, 30})).id("popup"))
                            .id("anchor")
                            .position(Position::Absolute)
                            .left(80)
                            .top(70)));
    sheet(tree, "#anchor { transform: scale(2) translateX(10px); }");
    auto list = render(tree, log);
    const Node *popup = find(tree.root(), "popup");
    check(
        near(popup->global_pos.x, 80) && near(popup->global_pos.y, 108),
        "anchor uses transformed window bounds without scaling popup content");
    check(list.commands().back().transform.is_identity(),
          "popup does not inherit the anchor's transform");
    press(tree, {popup->global_pos.x + 5, popup->global_pos.y + 5});
    check(log.last_press == 2,
          "transformed anchor paint and hit testing agree");
    sheet(tree, "#anchor { transform: translateX(35px); }");
    render(tree, log);
    check(near(popup->global_pos.x, 95),
          "popup follows paint-only anchor movement");
  }
  {
    Log log;
    WidgetTree tree(
        transfer_widget(column(Probe(1, log, {40, 20}),
                               overlay(Probe(2, log, {100, 40})).id("popup"))
                            .position(Position::Fixed)
                            .right(0)
                            .bottom(0)));
    render(tree, log);
    const Node *popup = find(tree.root(), "popup");
    check(near(popup->global_pos.x, 192) && near(popup->global_pos.y, 132),
          "bottom placement flips upward and clamps to right window edge");
    tree.layout({80, 60});
    DisplayList list;
    Painter painter(list, {80, 60});
    tree.render(painter);
    check(popup->size.width <= 64 && popup->size.height <= 44,
          "resizing constrains overlay measurement to available viewport");
  }
  {
    Log log;
    OverlayOptions options;
    options.trigger = OverlayTrigger::HoverOrFocus;
    options.interactive = false;
    options.delay = 500ms;
    options.dismiss_on_escape = options.dismiss_on_scroll = true;
    WidgetTree tree(transfer_widget(
        column(Probe(1, log, {40, 20}),
               overlay(Probe(2, log, {80, 30})).options(options))
            .position(Position::Absolute)
            .left(40)
            .top(40)));
    render(tree, log, 10);
    check(log.measures[2] == 0 && !drawn(log, 2),
          "hidden overlays do not measure or draw their content");
    move(tree, {45, 45});
    render(tree, log, 10);
    check(near(static_cast<float>(tree.next_wake_time()), 10.5f),
          "hover arms a single deadline");
    check(!tree.needs_paint(), "waiting for tooltip does not poll frames");
    render(tree, log, 10.49);
    check(!drawn(log, 2), "tooltip remains hidden before deadline");
    render(tree, log, 10.5);
    check(drawn(log, 2) && log.measures[2] == 1,
          "deadline opens and measures once");
    render(tree, log, 10.6);
    check(log.measures[2] == 1 && !tree.needs_paint(),
          "steady visible overlay reuses layout and allows idle sleep");
    KeyPressedEvent escape(Keycode::Escape);
    tree.process_event(escape);
    render(tree, log, 10.7);
    check(!drawn(log, 2),
          "Escape dismisses without reopening while still hovered");
    move(tree, {200, 150});
    render(tree, log, 10.8);
    move(tree, {45, 45});
    render(tree, log, 11.3);
    check(drawn(log, 2), "leaving and returning rearms tooltip");
    MouseScrolledEvent wheel(0, -1, {45, 45});
    tree.process_event(wheel);
    render(tree, log, 11.4);
    check(!drawn(log, 2), "scroll dismisses tooltip");
  }
  {
    Log log;
    WidgetTree tree(transfer_widget(
        column(
            tooltip(Probe(1, log, {60, 24}), "A useful explanation").id("tip"))
            .padding(40)));
    const auto hidden_commands = render(tree, log).commands().size();
    Node *tip = find(tree.root(), "tip");
    check(near(tip->size.width, 60) && near(tip->size.height, 24),
          "Tooltip preserves trigger natural dimensions");
    auto *bubble = tip->internal_children[0].get();
    check(bubble->part == "bubble" && bubble->widget->overlay_options() &&
              !bubble->widget->overlay_options()->interactive,
          "Tooltip exposes its noninteractive overlay as a styled part");
    check(near(bubble->style_node.computed->get<styles::Padding>().left, 10),
          "Tooltip installs default bubble styling");
    move(tree, {45, 45});
    render(tree, log, 0.1);
    press(tree, {45, 45});
    auto list = render(tree, log, 0.1);
    check(list.commands().size() > hidden_commands,
          "focus reveals Tooltip without hover delay");
    WindowFocusLostEvent lost;
    tree.process_event(lost);
    list = render(tree, log, 0.2);
    check(list.commands().size() == hidden_commands,
          "window focus loss dismisses tooltip and clears focused trigger");
    move(tree, {45, 45});
    render(tree, log, 1);
    MouseLeftEvent left;
    tree.process_event(left);
    list = render(tree, log, 1.1);
    check(list.commands().size() == hidden_commands,
          "leaving the native window cancels hover tooltip");
    sheet(tree, "tooltip::part(bubble) { padding: 11px 17px; }");
    render(tree, log, 1.2);
    check(near(bubble->style_node.computed->get<styles::Padding>().left, 17),
          "application style overrides Tooltip bubble defaults");
  }
  {
    Log log;
    auto make = [&](float offset) {
      return scrollable(column(column(Probe(1, log, {40, 20}),
                                      overlay(Probe(2, log, {80, 30}))),
                               column().height(300)))
          .size({100, 40})
          .scroll_to({0, offset});
    };
    WidgetTree tree(transfer_widget(make(0)));
    render(tree, log);
    check(drawn(log, 2), "visible anchor shows popup");
    tree.build(transfer_widget(make(100)));
    render(tree, log);
    check(!drawn(log, 2), "fully clipped anchor hides its overlay");
  }
  {
    Log log;
    WidgetTree tree(transfer_widget(
        column(Probe(1, log, {40, 20}),
               overlay(column(Probe(2, log, {80, 30}),
                              overlay(Probe(3, log, {90, 25})).id("nested")))
                   .id("outer"))
            .padding(40)));
    render(tree, log);
    check(log.drawn == std::vector<int>({1, 2, 3}),
          "nested overlays each paint once in order");
    static_cast<Overlay *>(find(tree.root(), "outer")->widget.get())
        ->open(false);
    render(tree, log);
    check(!drawn(log, 2) && !drawn(log, 3),
          "closing outer overlay hides nested overlays");
  }
  {
    Log log;
    std::optional<State<bool>> visible;
    WidgetTree tree(transfer_widget(component([&] {
      auto state = use_state(true);
      visible = state;
      auto content = column(Probe(1, log, {40, 20}));
      if (state.get())
        content.add(overlay(Probe(2, log, {80, 30})).key("popup"));
      return content;
    })));
    render(tree, log);
    check(drawn(log, 2), "component can declare an overlay");
    visible->set(false);
    render(tree, log);
    check(!drawn(log, 2),
          "reconciliation removes overlay without stale paint pointers");
    visible->set(true);
    render(tree, log);
    check(drawn(log, 2), "reconciliation can create a fresh overlay");
    tree.build(nullptr);
    render(tree, log);
    check(log.drawn.empty(), "destroying tree clears overlay ownership");
    tree.build(transfer_widget(
        column(Probe(1, log, {40, 20}), overlay(Probe(2, log, {80, 30})))));
    render(tree, log);
    check(drawn(log, 2), "empty tree can be rebuilt with overlays");
  }
  {
    Log log;
    WidgetTree tree(transfer_widget(
        column(
            Probe(1, log, {40, 20}), component([&] {
              return overlay(
                         column(
                             input().value("Editable").width(120).id("editor"),
                             Probe(2, log, {12, 10})
                                 .position(Position::Absolute)
                                 .right(0)
                                 .bottom(0)
                                 .id("corner"),
                             Probe(3, log, {10, 10})
                                 .position(Position::Fixed)
                                 .left(260)
                                 .top(170)
                                 .id("fixed")))
                  .id("popup");
            }))
            .padding(40)
            .id("anchor")));
    sheet(tree, "#anchor { transform: translateX(15px); }");
    render(tree, log);
    Node *popup = find(tree.root(), "popup");
    Node *corner = find(tree.root(), "corner");
    check(near(corner->global_pos.x + corner->size.width,
               popup->global_pos.x + popup->size.width) &&
              near(corner->global_pos.y + corner->size.height,
                   popup->global_pos.y + popup->size.height),
          "absolute descendants use overlay as their containing block");
    Node *editor = find(tree.root(), "editor");
    press(tree, {editor->global_pos.x + 5, editor->global_pos.y + 5});
    check(tree.wants_text_input() && tree.text_input_area().has_value(),
          "interactive portal hosts a focused text input through a transparent "
          "component");
    const auto local_area =
        editor->widget->text_input_area({editor->global_pos, editor->size});
    check(
        near(tree.text_input_area()->rect.origin.x, local_area->rect.origin.x),
        "IME geometry excludes transforms above the portal boundary");
    Node *fixed = find(tree.root(), "fixed");
    check(near(fixed->global_pos.x, 260) && near(fixed->global_pos.y, 170),
          "viewport-fixed overlay descendant starts in window coordinates");
    sheet(tree, "#anchor { transform: translateX(30px); }");
    render(tree, log);
    check(near(fixed->global_pos.x, 260) && near(fixed->global_pos.y, 170),
          "moving popup leaves viewport-fixed descendants in place");
    static_cast<Overlay *>(popup->widget.get())->open(false);
    render(tree, log);
    check(!tree.wants_text_input(),
          "closing popup deactivates its input client");
    TextInputEvent input_event("should not arrive");
    tree.process_event(input_event);
    check(editor->widget->selection_text() == "Editable",
          "hidden popup cannot receive text events");
  }
  std::printf("overlay: %d failures\n", failures);
  return failures ? 1 : 0;
}
