#include <cstdio>
#include <memory>
#include <optional>

#include "voidui/core/component.h"
#include "voidui/core/context.h"
#include "voidui/core/style/parser.h"
#include "voidui/core/widget_tree.h"
#include "voidui/widgets/row.h"

namespace {

class Probe : public voidui::Widget {
public:
  VOIDUI_STYLE_SCOPE(Probe, "probe")

  Probe(int id, int value) : id_(id), value_(value) {}

  void register_children(voidui::Registrar &) override {}

  voidui::Size<float> layout(voidui::Constraints constraints,
                             voidui::LayoutContext &ctx) override {
    return constraints.resolve(ctx.style.layout_size(), {10.0f, 10.0f});
  }

  void draw(const voidui::DrawContext &, voidui::Painter &) override {}

  voidui::EventResult on_event(voidui::Event &) override {
    return voidui::EventResult::Unhandled;
  }

  std::unique_ptr<voidui::Widget> clone() const override {
    return std::make_unique<Probe>(id_, value_);
  }

  int id() const { return id_; }
  int value() const { return value_; }

private:
  int id_;
  int value_;
};

auto counter(int id, std::optional<voidui::State<int>> &state) {
  return voidui::component([id, &state] {
           auto value = voidui::use_state(id * 10);
           state = value;
           return Probe(id, value.get());
         })
      .key(id);
}

bool expect(bool condition, const char *message) {
  if (!condition)
    std::fprintf(stderr, "FAIL: %s\n", message);
  return condition;
}

Probe *probe(voidui::Node *component) {
  return static_cast<Probe *>(component->children[0]->widget.get());
}

} // namespace

int main() {
  std::optional<voidui::State<bool>> reversed;
  std::optional<voidui::State<int>> first;
  std::optional<voidui::State<int>> second;

  auto view = voidui::component([&] {
    auto reverse = voidui::use_state(false);
    reversed = reverse;
    if (reverse.get())
      return voidui::row(counter(2, second), counter(1, first));
    return voidui::row(counter(1, first), counter(2, second));
  });

  voidui::WidgetTree tree(voidui::transfer_widget(std::move(view)));
  tree.layout({200.0f, 100.0f});

  voidui::Node *row = tree.root()->children[0].get();
  voidui::Node *first_component = row->children[0].get();
  voidui::Node *second_component = row->children[1].get();

  bool ok = true;
  ok &= expect(probe(first_component)->value() == 10,
               "first component starts with its own state");
  ok &= expect(probe(second_component)->value() == 20,
               "second component starts with its own state");

  auto styled = voidui::StyleParser::parse("row > probe { padding: 7; }",
                                           "component_selftest.vss");
  tree.set_stylesheet(styled.sheet);
  const auto &padding =
      first_component->children[0]
          ->style_node.computed->get<voidui::styles::Padding>();
  ok &= expect(padding.top == 7.0f,
               "components are transparent to child selectors");

  first->set(11);
  ok &= expect(tree.needs_layout(), "state updates request layout");
  tree.layout({200.0f, 100.0f});
  ok &= expect(row->children[0].get() == first_component,
               "state updates preserve the component node");
  ok &= expect(probe(first_component)->value() == 11,
               "state updates rebuild the component output");
  ok &= expect(probe(second_component)->value() == 20,
               "sibling state remains isolated");

  reversed->set(true);
  tree.layout({200.0f, 100.0f});
  ok &= expect(row->children[0].get() == second_component &&
                   row->children[1].get() == first_component,
               "keys preserve component identity while reordering");
  ok &= expect(probe(row->children[1].get())->value() == 11,
               "keyed components keep state after reordering");

  first->set(12);
  reversed->set(false);
  tree.layout({200.0f, 100.0f});
  ok &= expect(row->children[0].get() == first_component &&
                   probe(first_component)->value() == 12,
               "nested dirty components reconcile through their dirty parent");

  return ok ? 0 : 1;
}
