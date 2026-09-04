#include <cmath>
#include <cstdio>
#include <memory>

#include "voidui/core/component.h"
#include "voidui/core/widget_tree.h"
#include "voidui/paint/display_list.h"
#include "voidui/paint/painter.h"
#include "voidui/widgets/column.h"
#include "voidui/widgets/row.h"
#include "voidui/widgets/text.h"

namespace {

class FixedBox : public voidui::Widget {
public:
  VOIDUI_STYLE_SCOPE(FixedBox, "fixed-box")

  FixedBox() = default;

  FixedBox(float width, float height) {
    set_size_({voidui::Length::Fixed{width}, voidui::Length::Fixed{height}});
  }

  explicit FixedBox(voidui::Size<voidui::Length> size) {
    set_size_(std::move(size));
  }

  void register_children(voidui::Registrar &) override {}

  voidui::Size<float> layout(voidui::Constraints constraints,
                             voidui::LayoutContext &ctx) override {
    return constraints.resolve(ctx.style.layout_size(), voidui::Size<float>{});
  }

  void draw(const voidui::DrawContext &, voidui::Painter &) override {}

  voidui::EventResult on_event(voidui::Event &) override {
    return voidui::EventResult::Unhandled;
  }

  std::unique_ptr<voidui::Widget> clone() const override {
    return std::make_unique<FixedBox>();
  }

private:
  void set_size_(voidui::Size<voidui::Length> size) {
    set_style<voidui::styles::Width>(std::move(size.width));
    set_style<voidui::styles::Height>(std::move(size.height));
  }
};

class FrameRequester : public FixedBox {
public:
  FrameRequester() : FixedBox(10.0f, 10.0f) {}

  void draw(const voidui::DrawContext &ctx, voidui::Painter &) override {
    ctx.request_frame();
  }

  std::unique_ptr<voidui::Widget> clone() const override {
    return std::make_unique<FrameRequester>();
  }
};

bool close(float lhs, float rhs) { return std::abs(lhs - rhs) < 0.001f; }

bool expect(bool condition, const char *message) {
  if (!condition)
    std::fprintf(stderr, "FAIL: %s\n", message);
  return condition;
}

} // namespace

int main() {
  auto nested_row =
      voidui::row(FixedBox(10.0f, 20.0f), FixedBox(30.0f, 40.0f)).gap(3.0f);
  auto view =
      voidui::column(FixedBox(50.0f, 10.0f), std::move(nested_row)).gap(5.0f);

  voidui::WidgetTree tree(voidui::transfer_widget(std::move(view)));
  bool ok = expect(tree.needs_layout(), "a new tree requests layout");
  tree.layout(voidui::Constraints(200.0f, 200.0f));
  ok &= expect(!tree.needs_layout() && tree.needs_paint(),
               "layout leaves only paint dirty");

  const voidui::Node *column = tree.root();
  ok &= expect(column != nullptr, "column root exists");
  if (!column)
    return 1;

  ok &= expect(close(column->size.width, 50.0f),
               "column width is its widest child");
  ok &= expect(close(column->size.height, 55.0f),
               "column height includes child heights and gap");
  ok &= expect(column->children.size() == 2, "column registers both children");
  if (column->children.size() != 2)
    return 1;

  const voidui::Node *row = column->children[1].get();
  ok &=
      expect(close(row->global_pos.x, 0.0f) && close(row->global_pos.y, 15.0f),
             "column places the row below its first child");
  ok &= expect(close(row->size.width, 43.0f),
               "row width includes child widths and gap");
  ok &=
      expect(close(row->size.height, 40.0f), "row height is its tallest child");
  ok &= expect(row->children.size() == 2, "row registers both children");
  if (row->children.size() != 2)
    return 1;

  ok &= expect(close(row->children[0]->global_pos.x, 0.0f) &&
                   close(row->children[0]->global_pos.y, 15.0f),
               "moving a nested row also moves its first child");
  ok &= expect(close(row->children[1]->global_pos.x, 13.0f) &&
                   close(row->children[1]->global_pos.y, 15.0f),
               "row places its second child after the first child and gap");

  auto flex_view =
      voidui::row(FixedBox(10.0f, 10.0f),
                  FixedBox(voidui::Size<voidui::Length>{
                      voidui::Length::Flex{1}, voidui::Length::Fill{}}),
                  FixedBox(voidui::Size<voidui::Length>{
                      voidui::Length::Flex{2}, voidui::Length::Fill{}}))
          .gap(5.0f)
          .size(voidui::Size<voidui::Length>{voidui::Length::Fixed{100.0f},
                                             voidui::Length::Fixed{20.0f}});
  voidui::WidgetTree flex_tree(voidui::transfer_widget(std::move(flex_view)));
  flex_tree.layout(voidui::Constraints(200.0f, 200.0f));

  const voidui::Node *flex_row = flex_tree.root();
  const float one_share = 80.0f / 3.0f;
  ok &= expect(close(flex_row->children[1]->size.width, one_share),
               "row assigns one share to Flex(1)");
  ok &= expect(close(flex_row->children[2]->size.width, one_share * 2.0f),
               "row assigns two shares to Flex(2)");
  ok &= expect(close(flex_row->children[1]->size.height, 20.0f) &&
                   close(flex_row->children[2]->size.height, 20.0f),
               "Fill stretches children on the cross axis");

  auto decorated_view =
      voidui::row(FixedBox(10.0f, 10.0f), FixedBox(20.0f, 5.0f))
          .gap(5.0f)
          .background(voidui::Color(10, 20, 30))
          .padding(voidui::Padding(2.0f))
          .border(voidui::Border::solid(1.0f, voidui::Color(40, 50, 60)));
  voidui::WidgetTree decorated_tree(
      voidui::transfer_widget(std::move(decorated_view)));
  decorated_tree.layout(voidui::Constraints(200.0f, 200.0f));

  const voidui::Node *decorated_row = decorated_tree.root();
  ok &= expect(close(decorated_row->size.width, 41.0f) &&
                   close(decorated_row->size.height, 16.0f),
               "padding and border contribute to the row's intrinsic size");
  ok &= expect(close(decorated_row->children[0]->global_pos.x, 3.0f) &&
                   close(decorated_row->children[0]->global_pos.y, 3.0f),
               "padding and border inset the first child");
  ok &= expect(close(decorated_row->children[1]->global_pos.x, 18.0f) &&
                   close(decorated_row->children[1]->global_pos.y, 3.0f),
               "decorated row preserves its child gap");

  auto styled_view = voidui::row(FixedBox(10.0f, 10.0f));
  voidui::WidgetTree styled_tree(
      voidui::transfer_widget(std::move(styled_view)));
  auto dimensions = voidui::StyleParser::parse(
      "row { width: 120px; height: 35px; }", "layout.vss");
  ok &= expect(dimensions.diagnostics.empty(),
               "width and height parse as Length properties");
  styled_tree.set_stylesheet(dimensions.sheet);
  styled_tree.layout(voidui::Constraints(200.0f, 200.0f));
  ok &= expect(close(styled_tree.root()->size.width, 120.0f) &&
                   close(styled_tree.root()->size.height, 35.0f),
               "stylesheet dimensions control the resolved box");

  auto inline_view =
      voidui::row(FixedBox(10.0f, 10.0f))
          .size({voidui::Length::Fixed{90.0f}, voidui::Length::Fixed{25.0f}});
  voidui::WidgetTree inline_tree(
      voidui::transfer_widget(std::move(inline_view)));
  inline_tree.set_stylesheet(dimensions.sheet);
  inline_tree.layout(voidui::Constraints(200.0f, 200.0f));
  ok &= expect(close(inline_tree.root()->size.width, 90.0f) &&
                   close(inline_tree.root()->size.height, 25.0f),
               "inline dimensions override application styles");

  auto flexible_component = voidui::component([] {
    return FixedBox(voidui::Size<voidui::Length>{voidui::Length::Flex{1},
                                                 voidui::Length::Fill{}});
  });
  auto component_view =
      voidui::row(FixedBox(10.0f, 10.0f), std::move(flexible_component))
          .size({voidui::Length::Fixed{100.0f}, voidui::Length::Fixed{20.0f}});
  voidui::WidgetTree component_tree(
      voidui::transfer_widget(std::move(component_view)));
  component_tree.layout(voidui::Constraints(200.0f, 200.0f));
  const voidui::Node *component_node = component_tree.root()->children[1].get();
  ok &= expect(close(component_node->size.width, 90.0f) &&
                   close(component_node->size.height, 20.0f),
               "layout sizing passes through transparent components");

  auto first_margin = FixedBox(10.0f, 10.0f);
  first_margin.id("first-margin");
  auto second_margin = FixedBox(20.0f, 5.0f);
  second_margin.id("second-margin");
  auto margin_view =
      voidui::column(std::move(first_margin), std::move(second_margin))
          .gap(2.0f);
  voidui::WidgetTree margin_tree(
      voidui::transfer_widget(std::move(margin_view)));
  auto margins = voidui::StyleParser::parse(R"vss(
    column { margin: 1px 2px 3px 4px; }
    column > fixed-box { margin: 5px 6px 7px 8px; }
    #first-margin { margin-left: 9px; }
  )vss",
                                            "margin.vss");
  ok &= expect(margins.diagnostics.empty(),
               "margin shorthand and longhands parse without diagnostics");
  margin_tree.set_stylesheet(margins.sheet);
  margin_tree.layout(voidui::Constraints(200.0f, 200.0f));

  const voidui::Node *margin_column = margin_tree.root();
  const voidui::Spacing<float> root_margin = voidui::resolve_fixed_margin(
      margin_column->style_node.computed->layout_margin());
  ok &= expect(close(root_margin.top, 1.0f) && close(root_margin.right, 2.0f) &&
                   close(root_margin.bottom, 3.0f) &&
                   close(root_margin.left, 4.0f),
               "four-value margin uses CSS top-right-bottom-left order");
  ok &= expect(close(margin_column->global_pos.x, 4.0f) &&
                   close(margin_column->global_pos.y, 1.0f),
               "root margin offsets the root border box");
  ok &= expect(close(margin_column->size.width, 34.0f) &&
                   close(margin_column->size.height, 41.0f),
               "child margin boxes contribute to parent intrinsic size");

  const voidui::Node *first_margin_node = margin_column->children[0].get();
  const voidui::Node *second_margin_node = margin_column->children[1].get();
  ok &= expect(close(first_margin_node->global_pos.x, 13.0f) &&
                   close(first_margin_node->global_pos.y, 6.0f),
               "a child is placed inside its outer margin box");
  ok &= expect(close(second_margin_node->global_pos.x, 12.0f) &&
                   close(second_margin_node->global_pos.y, 30.0f),
               "column advances by margin box size plus gap");
  ok &= expect(close(first_margin_node->size.width, 10.0f) &&
                   close(first_margin_node->size.height, 10.0f),
               "margin does not change the child's border-box size");

  auto one = FixedBox(1.0f, 1.0f);
  one.id("one");
  auto two = FixedBox(1.0f, 1.0f);
  two.id("two");
  auto three = FixedBox(1.0f, 1.0f);
  three.id("three");
  auto four = FixedBox(1.0f, 1.0f);
  four.id("four");
  voidui::WidgetTree shorthand_tree(voidui::transfer_widget(voidui::row(
      std::move(one), std::move(two), std::move(three), std::move(four))));
  auto shorthand = voidui::StyleParser::parse(R"vss(
    #one { margin: 1px; }
    #two { margin: 1px 2px; }
    #three { margin: 1px 2px 3px; margin-left: 9px; }
    #four { margin-left: 9px; margin: 1px 2px 3px 4px; }
  )vss",
                                              "margin-shorthand.vss");
  ok &= expect(shorthand.diagnostics.empty(),
               "all CSS margin shorthand arities parse");
  shorthand_tree.set_stylesheet(shorthand.sheet);
  shorthand_tree.layout(voidui::Constraints(200.0f, 200.0f));
  const auto &shorthand_children = shorthand_tree.root()->children;
  const voidui::Spacing<float> one_margin = voidui::resolve_fixed_margin(
      shorthand_children[0]->style_node.computed->layout_margin());
  const voidui::Spacing<float> two_margin = voidui::resolve_fixed_margin(
      shorthand_children[1]->style_node.computed->layout_margin());
  const voidui::Spacing<float> three_margin = voidui::resolve_fixed_margin(
      shorthand_children[2]->style_node.computed->layout_margin());
  const voidui::Spacing<float> four_margin = voidui::resolve_fixed_margin(
      shorthand_children[3]->style_node.computed->layout_margin());
  ok &= expect(close(one_margin.left, 1.0f) && close(one_margin.top, 1.0f) &&
                   close(one_margin.right, 1.0f) &&
                   close(one_margin.bottom, 1.0f),
               "one-value margin applies to every side");
  ok &=
      expect(close(two_margin.top, 1.0f) && close(two_margin.bottom, 1.0f) &&
                 close(two_margin.left, 2.0f) && close(two_margin.right, 2.0f),
             "two-value margin maps to vertical and horizontal sides");
  ok &= expect(
      close(three_margin.top, 1.0f) && close(three_margin.right, 2.0f) &&
          close(three_margin.bottom, 3.0f) && close(three_margin.left, 9.0f),
      "a later longhand overrides its shorthand edge");
  ok &= expect(close(four_margin.top, 1.0f) && close(four_margin.right, 2.0f) &&
                   close(four_margin.bottom, 3.0f) &&
                   close(four_margin.left, 4.0f),
               "a later shorthand overrides an earlier longhand");

  auto flex_margin_child = FixedBox(voidui::Size<voidui::Length>{
      voidui::Length::Flex{1}, voidui::Length::Fill{}});
  flex_margin_child.id("flex-margin");
  auto flex_margin_view =
      voidui::row(FixedBox(10.0f, 10.0f), std::move(flex_margin_child))
          .size({voidui::Length::Fixed{100.0f}, voidui::Length::Fixed{20.0f}});
  voidui::WidgetTree flex_margin_tree(
      voidui::transfer_widget(std::move(flex_margin_view)));
  auto flex_margin_style = voidui::StyleParser::parse(
      "#flex-margin { margin: 0 5px; }", "flex-margin.vss");
  flex_margin_tree.set_stylesheet(flex_margin_style.sheet);
  flex_margin_tree.layout(voidui::Constraints(200.0f, 200.0f));
  const voidui::Node *flex_margin_node =
      flex_margin_tree.root()->children[1].get();
  ok &= expect(close(flex_margin_node->size.width, 80.0f) &&
                   close(flex_margin_node->global_pos.x, 15.0f),
               "fixed flex margins are reserved before free-space sharing");

  auto negative_margin = FixedBox(10.0f, 10.0f);
  negative_margin.id("negative-margin");
  voidui::WidgetTree negative_tree(voidui::transfer_widget(
      voidui::row(std::move(negative_margin), FixedBox(10.0f, 10.0f))));
  auto negative_style = voidui::StyleParser::parse(
      "#negative-margin { margin-right: -5px; }", "negative-margin.vss");
  negative_tree.set_stylesheet(negative_style.sheet);
  negative_tree.layout(voidui::Constraints(200.0f, 200.0f));
  ok &= expect(close(negative_tree.root()->size.width, 15.0f) &&
                   close(negative_tree.root()->children[1]->global_pos.x, 5.0f),
               "negative margins reduce flow advance and allow overlap");

  auto inline_margin_view =
      voidui::text("inline margin").margin(voidui::Margin(2.0f, 3.0f));
  voidui::WidgetTree inline_margin_tree(
      voidui::transfer_widget(std::move(inline_margin_view)));
  inline_margin_tree.layout(voidui::Constraints(100.0f, 100.0f));
  ok &= expect(close(inline_margin_tree.root()->global_pos.x, 2.0f) &&
                   close(inline_margin_tree.root()->global_pos.y, 3.0f),
               "the C++ margin setter writes the same inline style edges");

  auto centered_box = FixedBox(20.0f, 10.0f);
  centered_box.id("centered-box");
  auto centered_view =
      voidui::row(std::move(centered_box))
          .size({voidui::Length::Fixed{100.0f}, voidui::Length::Fixed{50.0f}});
  voidui::WidgetTree centered_tree(
      voidui::transfer_widget(std::move(centered_view)));
  auto centered_style = voidui::StyleParser::parse(
      "#centered-box { margin: auto; }", "auto-margin.vss");
  ok &= expect(centered_style.diagnostics.empty(),
               "auto is valid in every margin shorthand slot");
  centered_tree.set_stylesheet(centered_style.sheet);
  centered_tree.layout(voidui::Constraints(200.0f, 200.0f));
  const voidui::Node *centered_node = centered_tree.root()->children[0].get();
  const voidui::Spacing<voidui::MarginValue> &centered_margin =
      centered_node->style_node.computed->layout_margin();
  ok &= expect(
      centered_margin.left.is_auto() && centered_margin.top.is_auto() &&
          centered_margin.right.is_auto() && centered_margin.bottom.is_auto(),
      "margin auto remains distinct in computed style");
  ok &= expect(close(centered_node->global_pos.x, 40.0f) &&
                   close(centered_node->global_pos.y, 20.0f),
               "row distributes auto margins on both axes");

  auto pushed_box = FixedBox(20.0f, 10.0f);
  pushed_box.id("pushed-box");
  auto pushed_view =
      voidui::row(FixedBox(10.0f, 10.0f), std::move(pushed_box))
          .size({voidui::Length::Fixed{100.0f}, voidui::Length::Fixed{20.0f}});
  voidui::WidgetTree pushed_tree(
      voidui::transfer_widget(std::move(pushed_view)));
  auto pushed_style = voidui::StyleParser::parse(
      "#pushed-box { margin-left: auto; }", "auto-margin-longhand.vss");
  ok &= expect(pushed_style.diagnostics.empty(),
               "auto is valid in margin longhands");
  pushed_tree.set_stylesheet(pushed_style.sheet);
  pushed_tree.layout(voidui::Constraints(200.0f, 200.0f));
  ok &= expect(close(pushed_tree.root()->children[1]->global_pos.x, 80.0f),
               "a main-axis auto margin absorbs remaining row space");

  auto auto_margin_component = voidui::component([] {
    auto box = FixedBox(20.0f, 10.0f);
    box.id("auto-component-box");
    return box;
  });
  auto component_margin_view =
      voidui::row(std::move(auto_margin_component))
          .size({voidui::Length::Fixed{100.0f}, voidui::Length::Fixed{20.0f}});
  voidui::WidgetTree component_margin_tree(
      voidui::transfer_widget(std::move(component_margin_view)));
  auto component_margin_style = voidui::StyleParser::parse(
      "#auto-component-box { margin-left: auto; }", "auto-component.vss");
  component_margin_tree.set_stylesheet(component_margin_style.sheet);
  component_margin_tree.layout(voidui::Constraints(200.0f, 200.0f));
  const voidui::Node *component_rendered_box =
      component_margin_tree.root()->children[0]->children[0].get();
  ok &= expect(close(component_rendered_box->global_pos.x, 80.0f),
               "auto margins pass through transparent components");

  auto column_box = FixedBox(20.0f, 10.0f);
  column_box.id("column-box");
  auto auto_column =
      voidui::column(std::move(column_box))
          .size({voidui::Length::Fixed{100.0f}, voidui::Length::Fixed{100.0f}});
  voidui::WidgetTree auto_column_tree(
      voidui::transfer_widget(std::move(auto_column)));
  auto auto_column_style = voidui::StyleParser::parse(
      "#column-box { margin: 5px auto auto; }", "auto-column.vss");
  ok &= expect(auto_column_style.diagnostics.empty(),
               "auto and fixed margins can be mixed in a shorthand");
  auto_column_tree.set_stylesheet(auto_column_style.sheet);
  auto_column_tree.layout(voidui::Constraints(200.0f, 200.0f));
  const voidui::Node *column_box_node =
      auto_column_tree.root()->children[0].get();
  ok &= expect(close(column_box_node->global_pos.x, 40.0f) &&
                   close(column_box_node->global_pos.y, 5.0f),
               "column resolves cross-axis centering and trailing auto space");

  auto root_box = FixedBox(20.0f, 10.0f);
  root_box.id("root-box");
  voidui::WidgetTree auto_root_tree(
      voidui::transfer_widget(std::move(root_box)));
  auto auto_root_style = voidui::StyleParser::parse(
      "#root-box { margin: 0 auto; }", "auto-root.vss");
  auto_root_tree.set_stylesheet(auto_root_style.sheet);
  auto_root_tree.layout(voidui::Constraints(200.0f, 100.0f));
  ok &= expect(close(auto_root_tree.root()->global_pos.x, 90.0f),
               "horizontal auto margins center a fixed-width root box");

  auto overflowing_box = FixedBox(30.0f, 10.0f);
  overflowing_box.id("overflowing-box");
  auto overflowing_view =
      voidui::row(std::move(overflowing_box))
          .size({voidui::Length::Fixed{20.0f}, voidui::Length::Fixed{10.0f}});
  voidui::WidgetTree overflowing_tree(
      voidui::transfer_widget(std::move(overflowing_view)));
  auto overflowing_style = voidui::StyleParser::parse(
      "#overflowing-box { margin: 0 auto; }", "auto-overflow.vss");
  overflowing_tree.set_stylesheet(overflowing_style.sheet);
  overflowing_tree.layout(voidui::Constraints(100.0f, 100.0f));
  ok &= expect(close(overflowing_tree.root()->children[0]->global_pos.x, 0.0f),
               "auto margins resolve to zero when no free space remains");

  voidui::WidgetTree animation_tree(voidui::transfer_widget(FrameRequester{}));
  animation_tree.layout(voidui::Constraints(100.0f, 100.0f));
  voidui::DisplayList animation_list;
  voidui::Painter animation_painter(animation_list,
                                    voidui::Size<float>(100.0f, 100.0f));
  animation_tree.render(animation_painter);
  ok &= expect(animation_tree.needs_paint(),
               "a draw request schedules the next animation frame");

  return ok ? 0 : 1;
}
