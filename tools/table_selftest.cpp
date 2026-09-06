#include <cmath>
#include <cstdio>
#include <limits>
#include <memory>
#include <string>

#include "voidui/core/widget_tree.h"
#include "voidui/paint/display_list.h"
#include "voidui/paint/painter.h"
#include "voidui/widgets/table.h"

namespace {

using namespace voidui;

/// A cell's content of an exact size, so every expectation below is arithmetic
/// rather than a guess about the machine's fonts.
class FixedBox : public Widget {
public:
  VOIDUI_STYLE_SCOPE(FixedBox, "fixed-box")

  FixedBox() = default;
  FixedBox(float width, float height) {
    set_style<styles::Width>(Length::Fixed{width});
    set_style<styles::Height>(Length::Fixed{height});
  }

  void register_children(Registrar &) override {}
  Size<float> layout(Constraints constraints, LayoutContext &ctx) override {
    return constraints.resolve(ctx.style.layout_size(), Size<float>{});
  }
  void draw(const DrawContext &, Painter &) override {}
  EventResult on_event(Event &) override { return EventResult::Unhandled; }
  std::unique_ptr<Widget> clone() const override {
    return std::make_unique<FixedBox>();
  }
};

/// Content that reports a first baseline, so baseline alignment can be tested
/// without depending on which fonts this machine happens to have.
class BaselineBox : public FixedBox {
public:
  VOIDUI_STYLE_SCOPE(BaselineBox, "baseline-box")

  BaselineBox() = default;
  BaselineBox(float width, float height, float baseline)
      : FixedBox(width, height), baseline_(baseline) {}

  std::optional<float> first_baseline() const override { return baseline_; }
  std::unique_ptr<Widget> clone() const override {
    return std::make_unique<BaselineBox>();
  }

private:
  float baseline_ = 0.0f;
};

bool close(float lhs, float rhs) { return std::abs(lhs - rhs) < 0.01f; }

bool ok = true;

bool expect(bool condition, const char *message) {
  if (!condition) {
    std::fprintf(stderr, "FAIL: %s\n", message);
    ok = false;
  }
  return condition;
}

/// Builds a tree, applies a stylesheet and lays it out at one size.
struct Fixture {
  Fixture(Table declaration, std::string_view vss, float width = 1000.0f,
          float height = 1000.0f)
      : tree(transfer_widget(std::move(declaration))) {
    if (!vss.empty())
      tree.set_stylesheet(StyleParser::parse(vss, "table.selftest.vss").sheet);
    tree.layout(Constraints(width, height));
  }

  const Node *root() const { return tree.root(); }
  const Table &table() const {
    return static_cast<const Table &>(*tree.root()->widget);
  }

  WidgetTree tree;
};

/// `td, th { padding: 0 }` undoes the widget defaults so the numbers below are
/// only about the table algorithm.
constexpr const char *kBare = "td, th { padding: 0; } caption { padding: 0; }";

const Node *child(const Node *node, std::size_t index) {
  return node && index < node->children.size() ? node->children[index].get()
                                               : nullptr;
}

void test_auto_widths() {
  Fixture fixture(table(tbody(tr(td(FixedBox(40.0f, 20.0f)),
                                 td(FixedBox(60.0f, 30.0f))),
                              tr(td(FixedBox(50.0f, 10.0f)),
                                 td(FixedBox(30.0f, 10.0f))))),
                  kBare);

  const Node *root = fixture.root();
  expect(close(root->size.width, 110.0f),
         "auto columns take the widest cell in each column");
  expect(close(root->size.height, 40.0f), "row heights stack");

  const Node *body = child(root, 0);
  const Node *row0 = child(body, 0);
  const Node *row1 = child(body, 1);
  expect(row0 && row1 && close(row0->size.height, 30.0f),
         "a row is as tall as its tallest cell");
  expect(row1 && close(row1->global_pos.y, 30.0f),
         "the second row starts below the first");
  expect(close(child(row0, 1)->global_pos.x, 50.0f),
         "the second column starts after the first");
  expect(close(child(row0, 0)->size.width, 50.0f),
         "a cell is as wide as its column, not as its content");

  const std::vector<float> &widths = fixture.table().column_widths();
  expect(widths.size() == 2 && close(widths[0], 50.0f) &&
             close(widths[1], 60.0f),
         "the table reports the widths it resolved");
}

void test_border_spacing() {
  Fixture fixture(table(tbody(tr(td(FixedBox(40.0f, 20.0f)),
                                 td(FixedBox(60.0f, 30.0f))),
                              tr(td(FixedBox(50.0f, 10.0f)),
                                 td(FixedBox(30.0f, 10.0f))))),
                  "td { padding: 0; } table { border-spacing: 4 6; }");

  const Node *root = fixture.root();
  expect(close(root->size.width, 110.0f + 12.0f),
         "border-spacing adds a gap on both sides of every column");
  expect(close(root->size.height, 40.0f + 18.0f),
         "border-spacing adds a gap above and below every row");

  const Node *row0 = child(child(root, 0), 0);
  expect(close(child(row0, 0)->global_pos.x, 4.0f),
         "the first column starts after the outer horizontal spacing");
  expect(close(child(row0, 1)->global_pos.x, 58.0f),
         "the second column starts after the inner horizontal spacing");
  expect(close(row0->global_pos.y, 6.0f),
         "the first row starts after the outer vertical spacing");
  expect(close(child(child(root, 0), 1)->global_pos.y, 42.0f),
         "the second row starts after the inner vertical spacing");
}

void test_border_collapse() {
  Fixture fixture(table(tbody(tr(td(FixedBox(50.0f, 30.0f)),
                                 td(FixedBox(60.0f, 30.0f))),
                              tr(td(FixedBox(50.0f, 10.0f)),
                                 td(FixedBox(60.0f, 10.0f))))),
                  "td { padding: 0; border-width: 2; border-color: #000; }"
                  "table { border-collapse: collapse; }");

  const Node *root = fixture.root();
  // Three vertical lines of 2 and three horizontal ones, each shared by the
  // two cells it separates rather than drawn twice.
  expect(close(root->size.width, 110.0f + 6.0f),
         "a collapsed border is counted once between two columns");
  expect(close(root->size.height, 40.0f + 6.0f),
         "a collapsed border is counted once between two rows");

  const Node *row0 = child(child(root, 0), 0);
  expect(close(child(row0, 0)->global_pos.x, 2.0f),
         "the outer collapsed border sits outside the first cell");
  expect(close(child(row0, 1)->global_pos.x, 54.0f),
         "the shared collapsed border sits between the two cells");
  expect(close(child(row0, 0)->size.width, 50.0f),
         "a collapsed cell spends no width of its own on its border");
}

void test_spans() {
  Fixture colspan(table(tbody(tr(td(FixedBox(100.0f, 10.0f)).colspan(2)),
                              tr(td(FixedBox(30.0f, 10.0f)),
                                 td(FixedBox(30.0f, 10.0f))))),
                  kBare);
  expect(close(colspan.root()->size.width, 100.0f),
         "a spanning cell widens the columns it crosses when they are too "
         "narrow");
  const std::vector<float> &widths = colspan.table().column_widths();
  expect(widths.size() == 2 && close(widths[0], 50.0f) &&
             close(widths[1], 50.0f),
         "the extra width is shared between the spanned columns");

  Fixture rowspan(table(tbody(tr(td(FixedBox(20.0f, 60.0f)).rowspan(2),
                                 td(FixedBox(20.0f, 10.0f))),
                              tr(td(FixedBox(20.0f, 10.0f))))),
                  kBare);
  expect(close(rowspan.root()->size.height, 60.0f),
         "a spanning cell lengthens the rows it crosses");
  const Node *tall = child(child(child(rowspan.root(), 0), 0), 0);
  expect(tall && close(tall->size.height, 60.0f),
         "the spanning cell is as tall as the rows it covers");
  const Node *second_row = child(child(rowspan.root(), 0), 1);
  expect(second_row && close(second_row->global_pos.y, 30.0f),
         "the added height is shared between the spanned rows");
}

void test_fixed_layout() {
  Fixture fixture(
      table(colgroup(col().width(Length::Fixed{80.0f}), col()),
            tbody(tr(td(FixedBox(400.0f, 10.0f)), td(FixedBox(10.0f, 10.0f)))))
          .table_layout(TableLayout::Fixed)
          .width(Length::Fixed{200.0f}),
      kBare);

  const std::vector<float> &widths = fixture.table().column_widths();
  expect(widths.size() == 2 && close(widths[0], 80.0f) &&
             close(widths[1], 120.0f),
         "a fixed layout takes the column's width and shares out the rest, "
         "whatever the content wants");
  expect(close(fixture.root()->size.width, 200.0f),
         "a fixed table is exactly as wide as it was told to be");

  // Measured with nothing to divide -- as a row or a scroll viewport does --
  // a fixed table has to answer with its content, not with empty columns.
  Fixture unbounded(table(tbody(tr(td(FixedBox(40.0f, 10.0f)),
                                   td(FixedBox(60.0f, 10.0f)))))
                        .table_layout(TableLayout::Fixed),
                    kBare, std::numeric_limits<float>::infinity(),
                    std::numeric_limits<float>::infinity());
  expect(close(unbounded.root()->size.width, 100.0f),
         "a fixed table with no width to divide falls back to its content");
}

void test_shrink_to_fit() {
  Fixture fixture(table(tbody(tr(td(FixedBox(400.0f, 10.0f)),
                                 td(FixedBox(400.0f, 10.0f))))),
                  kBare, 500.0f, 500.0f);
  expect(close(fixture.root()->size.width, 500.0f),
         "an over-wide auto table shrinks into the space it has");

  Fixture wide(table(tbody(tr(td(FixedBox(40.0f, 10.0f)),
                              td(FixedBox(60.0f, 10.0f))))),
               kBare, 500.0f, 500.0f);
  expect(close(wide.root()->size.width, 100.0f),
         "an auto table that fits stays at its content width");

  Fixture filled(table(tbody(tr(td(FixedBox(40.0f, 10.0f)),
                                td(FixedBox(60.0f, 10.0f)))))
                     .width(Length::Fill{}),
                 kBare, 500.0f, 500.0f);
  expect(close(filled.root()->size.width, 500.0f),
         "width: fill stretches the columns to the whole line");
}

void test_caption_and_section_order() {
  Fixture top(table(caption(FixedBox(10.0f, 12.0f)),
                    tbody(tr(td(FixedBox(20.0f, 10.0f))))),
              kBare);
  expect(close(top.root()->size.height, 22.0f),
         "a caption adds its height to the table");
  expect(close(child(top.root(), 0)->global_pos.y, 0.0f),
         "caption-side: top puts the caption above the grid");
  expect(close(child(child(top.root(), 1), 0)->global_pos.y, 12.0f),
         "the grid starts below a top caption");

  Fixture bottom(table(caption(FixedBox(10.0f, 12.0f)),
                       tbody(tr(td(FixedBox(20.0f, 10.0f))))),
                 "td, th { padding: 0; } caption { padding: 0; }"
                 "table { caption-side: bottom; }");
  expect(close(child(bottom.root(), 0)->global_pos.y, 10.0f),
         "caption-side: bottom puts the caption below the grid");

  // Written foot first, head last: the table still stacks head, body, foot.
  Fixture ordered(table(tfoot(tr(td(FixedBox(20.0f, 10.0f)))),
                        tbody(tr(td(FixedBox(20.0f, 20.0f)))),
                        thead(tr(td(FixedBox(20.0f, 30.0f))))),
                  kBare);
  const Node *foot = child(ordered.root(), 0);
  const Node *body = child(ordered.root(), 1);
  const Node *head = child(ordered.root(), 2);
  expect(head && close(head->global_pos.y, 0.0f), "thead is laid out first");
  expect(body && close(body->global_pos.y, 30.0f), "tbody follows the head");
  expect(foot && close(foot->global_pos.y, 50.0f), "tfoot is laid out last");
}

void test_alignment() {
  Fixture fixture(table(tbody(tr(td(FixedBox(20.0f, 40.0f)),
                                 td(FixedBox(20.0f, 10.0f)).key("middle"),
                                 td(FixedBox(20.0f, 10.0f)).key("bottom"),
                                 td(FixedBox(10.0f, 10.0f)).key("right")))),
                  "td, th { padding: 0; }"
                  "td { vertical-align: top; }"
                  "tr > td:nth-child(2) { vertical-align: middle; }"
                  "tr > td:nth-child(3) { vertical-align: bottom; }"
                  "tr > td:nth-child(4) { text-align: right; width: 30; }");

  const Node *row = child(child(fixture.root(), 0), 0);
  expect(close(child(child(row, 0), 0)->global_pos.y, 0.0f),
         "vertical-align: top keeps the content at the top of the cell");
  expect(close(child(child(row, 1), 0)->global_pos.y, 15.0f),
         "vertical-align: middle centres the content in the cell");
  expect(close(child(child(row, 2), 0)->global_pos.y, 30.0f),
         "vertical-align: bottom drops the content to the bottom");
  const Node *right = child(row, 3);
  expect(right && close(child(right, 0)->global_pos.x,
                        right->global_pos.x + 20.0f),
         "text-align: right aligns a cell's content against its right edge");
}

void test_structural_selectors() {
  const auto sheet = StyleParser::parse(
      "tbody tr:nth-child(even) { background: #101010; }"
      "tbody tr:first-child   { border-width: 1; }"
      "tbody tr:last-child    { border-width: 2; }"
      "tbody tr:nth-child(3n+1) { color: #202020; }",
      "table.stripes.vss");
  expect(sheet.diagnostics.empty(), "the structural selectors all parse");

  auto declaration = table();
  for (int i = 0; i < 6; ++i)
    declaration.add(tbody(tr(td(FixedBox(20.0f, 10.0f)))).key(i));
  // One body, six rows -- the striping question is about rows inside a body.
  auto striped = table(tbody(tr(td(FixedBox(20.0f, 10.0f))),
                             tr(td(FixedBox(20.0f, 10.0f))),
                             tr(td(FixedBox(20.0f, 10.0f))),
                             tr(td(FixedBox(20.0f, 10.0f)))));

  WidgetTree tree(transfer_widget(std::move(striped)));
  tree.set_stylesheet(sheet.sheet);
  tree.layout(Constraints(500.0f, 500.0f));

  const Node *body = child(tree.root(), 0);
  const auto has_background = [&](std::size_t index) {
    return child(body, index)->style_node.computed->has<styles::Background>();
  };
  expect(!has_background(0) && has_background(1) && !has_background(2) &&
             has_background(3),
         ":nth-child(even) matches the second and fourth rows");
  expect(close(child(body, 0)->style_node.computed->get<styles::BorderWidth>(),
               1.0f),
         ":first-child matches the first row");
  expect(close(child(body, 3)->style_node.computed->get<styles::BorderWidth>(),
               2.0f),
         ":last-child matches the last row");
  expect(child(body, 0)->style_node.computed->has<styles::Foreground>() &&
             child(body, 3)->style_node.computed->has<styles::Foreground>() &&
             !child(body, 1)->style_node.computed->has<styles::Foreground>(),
         ":nth-child(3n+1) matches the first and fourth rows");

  const Selector built = Table::striped_row_selector().build();
  expect(built.to_string() == "tr:nth-child(2n)",
         "a structural selector built from C++ prints as CSS");
}

void test_baseline_alignment() {
  Fixture fixture(table(tbody(tr(td(BaselineBox(20.0f, 30.0f, 25.0f)),
                                 td(BaselineBox(20.0f, 20.0f, 10.0f))))),
                  kBare);

  const Node *row = child(child(fixture.root(), 0), 0);
  expect(close(child(child(row, 0), 0)->global_pos.y, 0.0f),
         "the cell with the lowest baseline does not move");
  expect(close(child(child(row, 1), 0)->global_pos.y, 15.0f),
         "a shallower baseline drops to meet the deepest one in the row");
  expect(close(row->size.height, 35.0f),
         "the row is tall enough for the content it pushed down");

  Fixture no_baseline(table(tbody(tr(td(FixedBox(20.0f, 30.0f)),
                                     td(FixedBox(20.0f, 20.0f))))),
                      kBare);
  const Node *plain = child(child(no_baseline.root(), 0), 0);
  expect(close(child(child(plain, 1), 0)->global_pos.y, 0.0f),
         "content with no baseline to report is aligned as if it were top");
}

/// Renders one frame and hands back the commands, so painting decisions --
/// which are not visible in the node geometry -- can be checked too.
DisplayList render(WidgetTree &tree, Size<float> viewport) {
  DisplayList list;
  Painter painter(list, viewport);
  tree.render(painter);
  return list;
}

std::size_t count_fills(const DisplayList &list, Color color) {
  std::size_t found = 0;
  for (const DrawCommand &command : list.commands()) {
    if (command.kind != CommandKind::FillRRect)
      continue;
    if (const Color *fill = std::get_if<Color>(&command.brush))
      found += *fill == color;
  }
  return found;
}

void test_painting() {
  const Color marker(255, 0, 0);
  Fixture hidden(table(tbody(tr(td(), td(FixedBox(20.0f, 10.0f))))),
                 "td { padding: 0; background: #ff0000; width: 20; }"
                 "table { empty-cells: hide; }");
  expect(count_fills(render(hidden.tree, {1000.0f, 1000.0f}), marker) == 1,
         "empty-cells: hide leaves a cell with no content unpainted");

  Fixture shown(table(tbody(tr(td(), td(FixedBox(20.0f, 10.0f))))),
                "td { padding: 0; background: #ff0000; width: 20; }");
  expect(count_fills(render(shown.tree, {1000.0f, 1000.0f}), marker) == 2,
         "empty-cells: show paints it like any other cell");

  const Color line(0, 255, 0);
  Fixture collapsed(table(tbody(tr(td(FixedBox(20.0f, 10.0f)),
                                   td(FixedBox(20.0f, 10.0f))),
                                tr(td(FixedBox(20.0f, 10.0f)),
                                   td(FixedBox(20.0f, 10.0f))))),
                    "td { padding: 0; border-width: 2; border-color: #00ff00; }"
                    "table { border-collapse: collapse; }");
  // Three vertical lines and three horizontal ones, drawn by the table rather
  // than by eight cells each stroking its own box.
  expect(count_fills(render(collapsed.tree, {1000.0f, 1000.0f}), line) == 6,
         "a collapsed table draws one shared line per grid edge");
}

void test_row_builder() {
  auto declaration = table().headers({"Name", "Age"});
  declaration.row("Alice", "30");
  declaration.row("Bob", td(FixedBox(20.0f, 10.0f)));

  Fixture fixture(std::move(declaration), kBare);
  const Node *head = child(fixture.root(), 0);
  const Node *body = child(fixture.root(), 1);
  expect(head && head->children.size() == 1 &&
             head->children[0]->children.size() == 2,
         "headers() builds a thead of two header cells");
  expect(body && body->children.size() == 2,
         "row() appends to one implicit tbody");
  expect(fixture.table().column_widths().size() == 2,
         "the builder rows line up with the header columns");
}

void test_empty_table() {
  Fixture fixture(table(), kBare);
  expect(close(fixture.root()->size.width, 0.0f) &&
             close(fixture.root()->size.height, 0.0f),
         "a table with no rows lays out without incident");

  Fixture caption_only(table(caption(FixedBox(30.0f, 12.0f))), kBare);
  expect(close(caption_only.root()->size.height, 12.0f),
         "a table that is only a caption is as tall as the caption");
}

} // namespace

int main() {
  test_auto_widths();
  test_border_spacing();
  test_border_collapse();
  test_spans();
  test_fixed_layout();
  test_shrink_to_fit();
  test_caption_and_section_order();
  test_alignment();
  test_baseline_alignment();
  test_structural_selectors();
  test_painting();
  test_row_builder();
  test_empty_table();
  return ok ? 0 : 1;
}
