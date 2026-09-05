#include "voidui/core/component.h"
#include "voidui/core/context.h"
#include "voidui/widgets/column.h"
#include "voidui/widgets/input.h"
#include "voidui/widgets/row.h"
#include "voidui/widgets/scrollable.h"
#include "voidui/widgets/text.h"

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
struct Log {
  std::vector<int> drawn;
  int pressed = -1;
};
class Probe : public Widget {
public:
  Probe(int id, Log &log, float width = 20, float height = 10)
      : id_(id), log_(&log), intrinsic_(width, height) {}
  VOIDUI_WIDGET_SIZE_STYLE
  VOIDUI_FLUENT_SETTER(clip, clip_, bool)
  template <WidgetClass T> Probe &&add(T &&child) && {
    children_.push_back(transfer_widget(std::forward<T>(child)));
    return std::move(*this);
  }
  void register_children(Registrar &registrar) override {
    for (auto &child : children_)
      registrar.take_child(std::move(child));
    children_.clear();
  }
  Size<float> layout(Constraints constraints, LayoutContext &ctx) override {
    for (size_t i = 0; i < ctx.child_count(); ++i) {
      ctx.constrain_child(i, {1000, 1000});
      ctx.place_child(i, {});
    }
    return constraints.resolve(ctx.style.layout_size(), intrinsic_);
  }
  void draw(const DrawContext &ctx, Painter &painter) override {
    log_->drawn.push_back(id_);
    painter.fill_rect(ctx.bounds, Paint(Color(100, 100, 100)));
  }
  EventResult on_event(Event &e) override {
    if (e.type() != EventType::MousePressed)
      return EventResult::Unhandled;
    log_->pressed = id_;
    return EventResult::Handled;
  }
  bool clips_children() const override { return clip_; }
  std::unique_ptr<Widget> clone() const override {
    auto result = std::make_unique<Probe>(id_, *log_, intrinsic_.width,
                                          intrinsic_.height);
    result->clip_ = clip_;
    for (const auto &child : children_)
      result->children_.push_back(clone_widget(*child));
    return result;
  }

private:
  int id_;
  Log *log_;
  Size<float> intrinsic_;
  bool clip_ = false;
  std::vector<std::unique_ptr<Widget>> children_;
};
DisplayList render(WidgetTree &tree, Log &log) {
  log.drawn.clear();
  DisplayList list;
  Painter painter(list, {400, 300});
  tree.render(painter);
  return list;
}
int press(WidgetTree &tree, Log &log, Point<float> point) {
  log.pressed = -1;
  MousePressedEvent event(MouseButton::Left, point);
  tree.process_event(event);
  MouseReleasedEvent release(MouseButton::Left, point);
  tree.process_event(release);
  return log.pressed;
}
void sheet(WidgetTree &tree, std::string_view source) {
  auto result = StyleParser::parse(source, "test.vss");
  check(result.diagnostics.empty(), "stylesheet parses without diagnostics");
  tree.set_stylesheet(result.sheet);
}
} // namespace

int main() {
  Inset inset;
  check(parse_style_value("calc(100% + 10px)", inset) &&
            near(inset.resolve(80), 90),
        "calc retains percentages until layout");
  check(parse_style_value("calc(50% - (2px + 3px))", inset) &&
            near(inset.resolve(100), 45),
        "nested sums resolve");
  for (auto invalid : {"12", "1em", "nan", "calc(1px+ 2px)", "calc(1px +)",
                       "10%junk", "+-1px", ""})
    check(!parse_style_value(invalid, inset),
          "invalid inset does not become a pixel value");
  ZIndex z;
  check(parse_style_value("-2147483648", z) &&
            z.value == std::numeric_limits<int>::min(),
        "full negative integer range");
  for (auto invalid : {"1.2", "1px", "2147483648", "--1", "+-1", ""})
    check(!parse_style_value(invalid, z),
          "z-index accepts only integers or auto");
  VisualTransform visual;
  check(parse_style_value("translate(-50%, 4px)", visual) &&
            near(visual.matrix({80, 20}).e, -40),
        "translate percent uses own box");
  check(!parse_style_value("translate(auto, 4px)", visual),
        "translate rejects auto");
  check(parse_style_value("scale(2) translateX(10px) ", visual) &&
            near(visual.matrix().e, 20),
        "transform composition follows CSS function order");
  check(parse_style_value("translateX(10px) translateX(5px)", visual) &&
            near(visual.matrix().e, 15),
        "repeated transforms compose instead of overwriting each other");
  check(parse_style_value("scale(2) translate(-50%, 4px)", visual) &&
            near(visual.matrix({80, 20}).e, -80),
        "scaled percentage transforms retain their reference box");
  check(PropertyValue(Inset(0.0f)).hash() == PropertyValue(Inset(-0.0f)).hash(),
        "equal inset zeroes share hashes");
  VisualTransform zero, negative_zero;
  check(PropertyValue(0.0f) == PropertyValue(-0.0f) &&
            PropertyValue(0.0f).hash() == PropertyValue(-0.0f).hash(),
        "scalar style hashes agree with signed-zero equality");
  negative_zero.translate_x = -0.0f;
  check(PropertyValue(zero) == PropertyValue(negative_zero) &&
            PropertyValue(zero).hash() == PropertyValue(negative_zero).hash(),
        "transform hash agrees with equality");
  // Extension registration must never invalidate a descriptor held by a caller.
  const auto *descriptor =
      &PropertyRegistry::instance().describe(styles::Left::index());
  for (int i = 0; i < 512; ++i) {
    PropertyDescriptor extra;
    extra.name = "test.extra-" + std::to_string(i);
    PropertyRegistry::instance().register_property(std::move(extra));
  }
  check(descriptor ==
                &PropertyRegistry::instance().describe(styles::Left::index()) &&
            descriptor->name == "left",
        "property descriptors survive registration");

  Log log;
  WidgetTree flow(transfer_widget(
      row(Probe(1, log).position(Position::Relative).left(7).top(-3),
          Probe(2, log).position(Position::Absolute).left(50), Probe(3, log))
          .gap(5)));
  flow.layout({200, 100});
  check(near(flow.root()->size.width, 45) &&
            near(flow.root()->children[2]->global_pos.x, 25),
        "positioned child contributes no size or gap");
  check(near(flow.root()->children[0]->global_pos.x, 7) &&
            near(flow.root()->children[0]->global_pos.y, -3),
        "relative offset preserves flow space");
  flow.layout({200, 100});
  check(near(flow.root()->children[0]->global_pos.x, 7),
        "relative offsets do not accumulate on relayout");

  WidgetTree tooltip(transfer_widget(
      column(Probe(0, log, 10, 80),
             column(Probe(1, log, 100, 40),
                    column(text("Tooltip label")).add_class("tooltip-content"))
                 .add_class("tooltip")
                 .size({100, 40}))));
  sheet(tooltip, R"(
    .tooltip { position: relative; }
    .tooltip-content {
      position: absolute; left: 50%; bottom: calc(100% + 10px);
      padding: 6px 10px; background: #111; color: white; border-radius: 6px;
      font-size: 12px; white-space: nowrap; opacity: 0; visibility: hidden;
      transform: translate(-50%, 4px);
      transition: opacity 150ms ease, transform 150ms ease, visibility 150ms;
      pointer-events: none; z-index: 1000;
    }
    .tooltip:hover .tooltip-content { opacity: 1; visibility: visible; transform: translate(-50%, 0); }
  )");
  tooltip.layout({400, 300});
  Node *anchor = tooltip.root()->children[1].get();
  Node *tip = anchor->children[1].get();
  check(near(tip->global_pos.x, 50) &&
            near(tip->global_pos.y + tip->size.height, 70),
        "tooltip centers on its containing block and sits ten pixels above it");
  const double start = tooltip.style_resolver().animation_time();
  tooltip.advance_animations(start);
  MouseMovedEvent hover({50, 100});
  tooltip.process_event(hover);
  tooltip.advance_animations(start + 0.075);
  check(tip->style_node.computed->get<styles::Visibility>() ==
                Visibility::Visible &&
            tip->style_node.computed->get<styles::Opacity>() > 0,
        "tooltip becomes visible throughout fade-in");
  tooltip.advance_animations(start + 0.2);
  render(tooltip, log);
  MouseMovedEvent leave({350, 290});
  tooltip.process_event(leave);
  tooltip.advance_animations(start + 0.275);
  check(tip->style_node.computed->get<styles::Visibility>() ==
            Visibility::Visible,
        "visibility remains visible throughout fade-out");
  tooltip.advance_animations(start + 0.36);
  check(tip->style_node.computed->get<styles::Visibility>() ==
            Visibility::Hidden,
        "visibility hides at transition end");

  WidgetTree edges(transfer_widget(Probe(0, log, 100, 80)
                                       .position(Position::Relative)
                                       .add(Probe(1, log)
                                                .position(Position::Absolute)
                                                .left(10)
                                                .right(20)
                                                .top(5)
                                                .bottom(15))));
  edges.layout({300, 200});
  check(near(edges.root()->children[0]->size.width, 70) &&
            near(edges.root()->children[0]->size.height, 60),
        "opposing insets stretch auto dimensions");
  sheet(edges, "* { inset: 1px calc(10% + 2px) 3px 4px; left: 9px; }");
  check(
      near(edges.root()->style_node.computed->get<styles::Left>().resolve(100),
           9) &&
          near(edges.root()->style_node.computed->get<styles::Right>().resolve(
                   100),
               12),
      "inset shorthand and edge cascade");

  WidgetTree stack(transfer_widget(
      Probe(0, log, 100, 100)
          .add(
              Probe(1, log)
                  .position(Position::Relative)
                  .add(Probe(2, log).position(Position::Absolute).z_index(100)))
          .add(Probe(3, log).position(Position::Relative).z_index(1))));
  stack.layout({300, 200});
  render(stack, log);
  check(log.drawn == std::vector<int>({0, 1, 3, 2}),
        "auto ancestor lets high-z descendant escape");
  check(press(stack, log, {5, 5}) == 2, "hit order is inverse paint order");
  stack.root()->children[0]->widget->set_style<styles::ZIndex>(0);
  stack.restyle();
  render(stack, log);
  check(log.drawn == std::vector<int>({0, 1, 2, 3}) &&
            press(stack, log, {5, 5}) == 3,
        "z-index zero traps descendants in a stacking context");
  stack.root()->children[1]->widget->set_style<styles::ZIndex>(-1);
  stack.restyle();
  render(stack, log);
  check(log.drawn == std::vector<int>({0, 3, 1, 2}),
        "negative context paints above context background and below normal "
        "content");
  WidgetTree ties(transfer_widget(
      Probe(0, log)
          .add(Probe(1, log).position(Position::Absolute).z_index(5))
          .add(Probe(2, log).position(Position::Absolute).z_index(5))));
  ties.layout({200, 100});
  render(ties, log);
  check(log.drawn == std::vector<int>({0, 1, 2}) &&
            press(ties, log, {5, 5}) == 2,
        "equal z-index preserves document order");

  WidgetTree overflow(transfer_widget(
      Probe(0, log).add(Probe(1, log).position(Position::Relative).left(50))));
  overflow.layout({200, 100});
  check(press(overflow, log, {55, 5}) == 1,
        "visible overflow outside parent can receive input");
  static_cast<Probe *>(overflow.root()->widget.get())->clip(true);
  overflow.restyle();
  overflow.request_layout();
  overflow.layout({200, 100});
  check(press(overflow, log, {55, 5}) == -1,
        "clipped overflow cannot receive input");

  WidgetTree hidden(
      transfer_widget(Probe(0, log)
                          .visibility(Visibility::Hidden)
                          .add(Probe(1, log)
                                   .visibility(Visibility::Visible)
                                   .pointer_events(PointerEvents::None))));
  hidden.layout({200, 100});
  render(hidden, log);
  check(log.drawn == std::vector<int>({1}) && press(hidden, log, {5, 5}) == -1,
        "visible child overrides hidden ancestor, pointer-events none passes "
        "through");

  WidgetTree fixed(transfer_widget(
      scrollable(
          column(Probe(0, log, 20, 500),
                 Probe(1, log).position(Position::Fixed).right(10).bottom(10)))
          .size({100, 80})));
  fixed.layout({300, 200});
  Node *floating = fixed.root()->children[0]->children[1].get();
  check(near(floating->global_pos.x, 270) && near(floating->global_pos.y, 180),
        "fixed uses viewport instead of scroll content size");
  check(press(fixed, log, {275, 185}) == 1,
        "viewport fixed escapes ancestor scroll clipping");
  MouseScrolledEvent scroll(0, -40, {20, 20});
  fixed.process_event(scroll);
  fixed.layout({300, 200});
  check(near(floating->global_pos.y, 180),
        "fixed does not move with scrolling");
  render(fixed, log);
  VisualTransform identity;
  identity.specified = true;
  fixed.root()->widget->set_style<styles::Transform>(identity);
  fixed.restyle();
  check(fixed.needs_layout(),
        "creating a transform invalidates positioned containing blocks");
  fixed.layout({300, 200});
  check(near(floating->global_pos.x, 70) && near(floating->global_pos.y, 60),
        "identity transform establishes a fixed containing block");
  render(fixed, log);
  identity.translate_y = 4;
  fixed.root()->widget->set_style<styles::Transform>(identity);
  fixed.restyle();
  check(!fixed.needs_layout() && fixed.needs_paint(),
        "moving an existing transform stays paint-only");

  WidgetTree nested_fixed(
      transfer_widget(Probe(0, log, 30, 30)
                          .clip(true)
                          .add(Probe(1, log)
                                   .position(Position::Fixed)
                                   .left(100)
                                   .top(60)
                                   .add(Probe(2, log)
                                            .position(Position::Absolute)
                                            .left(0)
                                            .top(0)))));
  nested_fixed.layout({300, 200});
  auto fixed_list = render(nested_fixed, log);
  check(fixed_list.commands().size() == 3 &&
            fixed_list.clips()[fixed_list.commands().back().clip_index]
                .scissor.contains({105, 65}),
        "fixed subtree draw commands use the same escaped clip as hit testing");
  check(press(nested_fixed, log, {105, 65}) == 2,
        "absolute descendants of a viewport-fixed box escape outer clips too");

  WidgetTree sticky(transfer_widget(
      scrollable(column(Probe(0, log, 60, 20).position(Position::Sticky).top(0),
                        Probe(1, log, 60, 300)))
          .size({100, 80})));
  sticky.layout({300, 200});
  MouseScrolledEvent sticky_scroll(0, -1, {50, 40});
  sticky.process_event(sticky_scroll);
  sticky.layout({300, 200});
  const Node *sticky_box = sticky.root()->children[0]->children[0].get();
  check(near(sticky_box->global_pos.y, 0) &&
            sticky.root()->children[0]->global_pos.y < 0,
        "sticky remains at the scrollport edge while its containing content "
        "scrolls");

  auto api = Probe(7, log)
                 .position(Position::Absolute)
                 .left(Inset::percentage(50))
                 .bottom(Inset::calc(100, 10))
                 .z_index(1000)
                 .opacity(0.5f);
  api.transition_property(
         TransitionPropertyList::make({styles::Opacity::index()}))
      .transition_duration(StyleTimeList::make({0.15f}))
      .transition_timing_function(EasingList::make({Easing::ease()}));
  WidgetTree clone(transfer_widget(api));
  check(clone.root()->style_node.computed->get<styles::Left>() ==
                Inset::percentage(50) &&
            clone.root()->style_node.computed->get<styles::ZIndex>().value ==
                1000,
        "lvalue cloning preserves the fluent positioning declarations");

  WidgetTree transparent(transfer_widget(
      row(Probe(0, log), component([&] {
            return Probe(1, log).position(Position::Absolute).left(80).top(30);
          }),
          Probe(2, log))
          .gap(5)));
  transparent.layout({200, 100});
  check(
      near(transparent.root()->size.width, 45) &&
          near(transparent.root()->children[1]->children[0]->global_pos.x, 80),
      "transparent components preserve out-of-flow root positioning");
  WidgetTree named(transfer_widget(
      input()
          .block_start(text("overlay").position(Position::Absolute))
          .inline_start(text("prefix"))
          .block_end(text("help"))));
  named.layout({300, 200});
  check(named.root()->internal_children[2]->global_pos.y >
            named.root()->internal_children[1]->global_pos.y,
        "out-of-flow named slot does not corrupt sibling slot indices");

  WidgetTree mutated(transfer_widget(Probe(1, log)));
  mutated.layout({200, 100});
  mutated.root()->widget->position(Position::Fixed).left(60).top(30);
  mutated.restyle();
  mutated.layout({200, 100});
  check(near(mutated.root()->global_pos.x, 60) &&
            near(mutated.root()->global_pos.y, 30),
        "restyle picks up the first inline declaration on an existing widget");
  mutated.build(nullptr);
  mutated.layout({200, 100});
  render(mutated, log);
  check(!mutated.root() && !mutated.needs_paint(),
        "clearing a built tree does not dereference a null widget");

  WidgetTree hidden_input(transfer_widget(input("keep")));
  hidden_input.layout({300, 200});
  MousePressedEvent focus_input(MouseButton::Left, {20, 18});
  hidden_input.process_event(focus_input);
  check(hidden_input.wants_text_input(), "input initially receives focus");
  hidden_input.root()->widget->visibility(Visibility::Hidden);
  hidden_input.restyle();
  TextInputEvent hidden_typing("discard");
  hidden_input.process_event(hidden_typing);
  check(!hidden_input.wants_text_input() &&
            static_cast<Input *>(hidden_input.root()->widget.get())->value() ==
                "keep",
        "hidden input stops receiving keyboard text");
  std::printf("positioning: %d failures\n", failures);
  return failures ? 1 : 0;
}
