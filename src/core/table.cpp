#include "voidui/widgets/table.h"

#include <algorithm>
#include <cmath>
#include <limits>

#include "voidui/core/context.h"

namespace voidui {
namespace {

constexpr float kInfinity = std::numeric_limits<float>::infinity();
constexpr std::size_t kNoIndex = static_cast<std::size_t>(-1);

/// A function component is invisible to layout: what it rendered owns the box.
Node *layout_box(Node *node) {
  while (node && node->style_node.is_transparent && !node->children.empty())
    node = node->children[0].get();
  return node;
}

bool participates(const Node *box) {
  return box && box->widget && box->style_node.computed &&
         !box->widget->overlay_options() &&
         !out_of_flow(box->style_node.computed->get<styles::Position>());
}

/// The node to place and measure, paired with the node whose style and widget
/// describe it. The two differ only for a component.
struct FlowChild {
  Node *node = nullptr;
  Node *box = nullptr;
};

std::vector<FlowChild> flow_children(Node &parent) {
  std::vector<FlowChild> result;
  result.reserve(parent.children.size());
  for (auto &child : parent.children) {
    Node *box = layout_box(child.get());
    if (participates(box))
      result.push_back({child.get(), box});
  }
  return result;
}

float border_width_of(const ComputedStyle &style) {
  return std::max(style.get<styles::BorderWidth>(), 0.0f);
}

/// Horizontal padding plus border, which is the difference between a cell's
/// border box and the space its content actually gets.
float horizontal_chrome(const ComputedStyle &style, bool collapsed) {
  const Spacing<float> padding = style.get<styles::Padding>();
  const float border = collapsed ? 0.0f : border_width_of(style);
  return padding.left + padding.right + 2.0f * border;
}

const Length *fixed_or_flexible(const Length &length) {
  return std::holds_alternative<Length::Auto>(length.value) ? nullptr : &length;
}

float flex_weight(const Length &length) {
  if (std::holds_alternative<Length::Fill>(length.value))
    return 1.0f;
  if (const auto *flex = std::get_if<Length::Flex>(&length.value))
    return static_cast<float>(std::max<std::uint16_t>(1, flex->value));
  return 0.0f;
}

// -- The grid ----------------------------------------------------------------

struct GridCell {
  Node *node = nullptr;
  TableCell *cell = nullptr;
  const ComputedStyle *style = nullptr;
  std::size_t row = 0;
  std::size_t column = 0;
  std::uint16_t colspan = 1;
  std::uint16_t rowspan = 1;
  float width = 0.0f;
  float height = 0.0f;
  float min_content = 0.0f;
  float max_content = 0.0f;
};

struct GridRow {
  Node *node = nullptr;
  TableRow *row = nullptr;
  const ComputedStyle *style = nullptr;
  std::size_t section = kNoIndex;
  float y = 0.0f;
  float height = 0.0f;
  /// The lowest first baseline among the cells that asked to be aligned on
  /// it. Zero when the row has none.
  float baseline = 0.0f;
};

struct GridSection {
  Node *node = nullptr;
  const ComputedStyle *style = nullptr;
  std::size_t first_row = 0;
  std::size_t row_count = 0;
};

struct GridColumnBox {
  Node *node = nullptr;
  std::size_t first_column = 0;
  std::size_t column_count = 1;
};

struct GridColumn {
  const ComputedStyle *style = nullptr;
  Length length;
  float min_width = 0.0f;
  float max_width = 0.0f;
  float width = 0.0f;
  float x = 0.0f;
  bool has_length = false;
};

/// The winning contributor to one collapsed border line: the widest wins, and
/// ties keep the first seen, which walks cell, row, section, column, table in
/// that order because that is the order they are offered.
struct CollapsedLine {
  float width = 0.0f;
  Brush brush = Brush(Color::TRANSPARENT);

  void offer(float candidate, const Brush &candidate_brush) {
    if (candidate <= width)
      return;
    width = candidate;
    brush = candidate_brush;
  }
};

/// Spreads `extra` over `weights`, giving the remainder to the last taker so
/// the parts add back up to the whole. Column and row sizing both need this,
/// and a table that is one pixel wider than its columns is a visible bug.
void distribute(float extra, const std::vector<float> &weights,
                std::vector<float> &out) {
  float total = 0.0f;
  for (float weight : weights)
    total += weight;
  if (extra <= 0.0f || total <= 0.0f)
    return;
  float given = 0.0f;
  std::size_t last = kNoIndex;
  for (std::size_t i = 0; i < weights.size(); ++i) {
    if (weights[i] <= 0.0f)
      continue;
    last = i;
  }
  for (std::size_t i = 0; i < weights.size(); ++i) {
    if (weights[i] <= 0.0f)
      continue;
    const float share =
        i == last ? extra - given : extra * weights[i] / total;
    out[i] += share;
    given += share;
  }
}

} // namespace

// -- TableCell ---------------------------------------------------------------

void TableCell::copy_cell_fields_(TableCell &copy) const {
  copy.colspan_ = colspan_;
  copy.rowspan_ = rowspan_;
  copy.children_.reserve(children_.size());
  for (const auto &child : children_)
    if (child)
      copy.children_.push_back(clone_widget(*child));
}

std::unique_ptr<Widget> TableCell::clone() const {
  auto copy = std::make_unique<TableCell>();
  copy_cell_fields_(*copy);
  return copy;
}

std::unique_ptr<Widget> TableHeaderCell::clone() const {
  auto copy = std::make_unique<TableHeaderCell>();
  copy_cell_fields_(*copy);
  return copy;
}

void TableCell::register_children(Registrar &registrar) {
  for (auto &child : children_)
    if (child)
      registrar.take_child(std::move(child));
  children_.clear();
}

Size<float> TableCell::layout(Constraints constraints, LayoutContext &ctx) {
  const Spacing<float> padding = ctx.style.get<styles::Padding>();
  const float border = collapsed_ ? 0.0f : border_width_of(ctx.style);
  const Spacing<float> chrome = padding + Spacing<float>(border);
  const Constraints inner = Constraints(constraints).shrink(chrome);

  const std::size_t count = ctx.child_count();
  empty_ = count == 0;

  std::vector<Size<float>> sizes(count);
  float content_width = 0.0f;
  float content_height = 0.0f;
  for (std::size_t i = 0; i < count; ++i) {
    sizes[i] = ctx.constrain_child(
        i, Constraints(0.0f, inner.max_width, 0.0f, kInfinity));
    content_width = std::max(content_width, sizes[i].width);
    content_height += sizes[i].height;
  }

  // Reported before the constraint clamps anything: column sizing probes a
  // cell with a deliberately impossible width to learn its minimum, and a
  // clamped answer would only repeat the probe back.
  measured_width_ = content_width + chrome.left + chrome.right;
  measured_height_ = content_height + chrome.top + chrome.bottom;

  const Size<float> resolved = constraints.resolve(
      ctx.style.layout_size(), {measured_width_, measured_height_});

  const float free_height =
      std::max(resolved.height - measured_height_, 0.0f);
  float y = chrome.top;
  switch (ctx.style.get<styles::VerticalAlign>()) {
  case VerticalAlign::Middle:
    y += free_height * 0.5f;
    break;
  case VerticalAlign::Bottom:
    y += free_height;
    break;
  case VerticalAlign::Baseline:
    // The row hands back the distance this cell has to drop so that its first
    // baseline meets its neighbours'. Zero for a row with nothing to align to.
    y += std::min(baseline_shift_, free_height);
    break;
  case VerticalAlign::Top:
    break;
  }

  const float content_box = std::max(
      resolved.width - chrome.left - chrome.right, 0.0f);
  const TextAlign align = ctx.style.get<styles::TextAlign>();

  baseline_.reset();
  for (std::size_t i = 0; i < count; ++i) {
    // Content narrower than the cell is aligned as a block, rather than
    // stretched: `text-align: right` in a cell should move a button as well as
    // a paragraph, and stretching the button would move nothing.
    const float slack = std::max(content_box - sizes[i].width, 0.0f);
    float x = chrome.left;
    if (align == TextAlign::Center)
      x += slack * 0.5f;
    else if (align == TextAlign::Right)
      x += slack;
    ctx.place_child(i, {x, y});
    if (i == 0) {
      const Node *box = layout_box_of(&ctx.child_node(i));
      if (box && box->widget)
        if (const std::optional<float> baseline = box->widget->first_baseline())
          baseline_ = y + *baseline;
    }
    y += sizes[i].height;
  }

  return resolved;
}

void TableCell::draw(const DrawContext &ctx, Painter &painter) {
  // `empty-cells: hide` is about a cell with nothing in it, and CSS says it
  // has no effect once the borders are collapsed -- there is no border of this
  // cell's own left to hide.
  if (empty_ && !collapsed_ &&
      ctx.style.get<styles::EmptyCells>() == EmptyCells::Hide)
    return;

  const Radius radius = ctx.style.get<styles::BorderRadius>();
  painter.fill_rrect(ctx.bounds, radius,
                     Paint(ctx.style.get<styles::Background>()));
  if (collapsed_)
    return;
  const float border = border_width_of(ctx.style);
  if (border > 0.0f)
    painter.stroke_rrect(ctx.bounds, radius,
                         Paint(ctx.style.get<styles::BorderColor>()),
                         Pen(border, StrokeAlign::Inside));
}

std::shared_ptr<const StyleSheet> TableCell::default_stylesheet() const {
  static const auto sheet =
      StyleParser::parse(R"vss(
    td { padding: 6px 10px; }
  )vss",
                         "table.td.default.vss", StyleOrigin::WidgetDefault)
          .sheet;
  return sheet;
}

std::shared_ptr<const StyleSheet> TableHeaderCell::default_stylesheet() const {
  static const auto sheet =
      StyleParser::parse(R"vss(
    th { padding: 6px 10px; font-weight: 600; }
  )vss",
                         "table.th.default.vss", StyleOrigin::WidgetDefault)
          .sheet;
  return sheet;
}

// -- TableRow ----------------------------------------------------------------

std::unique_ptr<Widget> TableRow::clone() const {
  std::vector<std::unique_ptr<Widget>> cells;
  cells.reserve(children_.size());
  for (const auto &child : children_)
    if (child)
      cells.push_back(clone_widget(*child));
  return std::make_unique<TableRow>(std::move(cells));
}

void TableRow::register_children(Registrar &registrar) {
  for (auto &child : children_)
    if (child)
      registrar.take_child(std::move(child));
  children_.clear();
}

Size<float> TableRow::layout(Constraints constraints, LayoutContext &ctx) {
  if (driven_) {
    // The table has already measured and placed every cell in this row. All
    // that is left is to accept the box it was given.
    driven_ = false;
    return constraints.resolve(ctx.style.layout_size(),
                               {constraints.min_width, constraints.min_height});
  }
  // A row outside a table has no columns to line up with, so it behaves like
  // an ordinary horizontal stack rather than disappearing.
  return detail::layout_linear(constraints, ctx, 0.0f, false);
}

void TableRow::draw(const DrawContext &ctx, Painter &painter) {
  const Radius radius = ctx.style.get<styles::BorderRadius>();
  painter.fill_rrect(ctx.bounds, radius,
                     Paint(ctx.style.get<styles::Background>()));
  if (collapsed_)
    return;
  const float border = border_width_of(ctx.style);
  if (border > 0.0f)
    painter.stroke_rrect(ctx.bounds, radius,
                         Paint(ctx.style.get<styles::BorderColor>()),
                         Pen(border, StrokeAlign::Inside));
}

// -- TableSection ------------------------------------------------------------

std::vector<std::unique_ptr<Widget>> TableSection::take_children_() const {
  std::vector<std::unique_ptr<Widget>> rows;
  rows.reserve(children_.size());
  for (const auto &child : children_)
    if (child)
      rows.push_back(clone_widget(*child));
  return rows;
}

void TableSection::register_children(Registrar &registrar) {
  for (auto &child : children_)
    if (child)
      registrar.take_child(std::move(child));
  children_.clear();
}

Size<float> TableSection::layout(Constraints constraints, LayoutContext &ctx) {
  if (driven_) {
    driven_ = false;
    return constraints.resolve(ctx.style.layout_size(),
                               {constraints.min_width, constraints.min_height});
  }
  return detail::layout_linear(constraints, ctx, 0.0f, true);
}

void TableSection::draw(const DrawContext &ctx, Painter &painter) {
  const Radius radius = ctx.style.get<styles::BorderRadius>();
  painter.fill_rrect(ctx.bounds, radius,
                     Paint(ctx.style.get<styles::Background>()));
  if (collapsed_)
    return;
  const float border = border_width_of(ctx.style);
  if (border > 0.0f)
    painter.stroke_rrect(ctx.bounds, radius,
                         Paint(ctx.style.get<styles::BorderColor>()),
                         Pen(border, StrokeAlign::Inside));
}

// -- TableCaption ------------------------------------------------------------

std::unique_ptr<Widget> TableCaption::clone() const {
  std::vector<std::unique_ptr<Widget>> children;
  children.reserve(children_.size());
  for (const auto &child : children_)
    if (child)
      children.push_back(clone_widget(*child));
  return std::make_unique<TableCaption>(std::move(children));
}

void TableCaption::register_children(Registrar &registrar) {
  for (auto &child : children_)
    if (child)
      registrar.take_child(std::move(child));
  children_.clear();
}

Size<float> TableCaption::layout(Constraints constraints, LayoutContext &ctx) {
  return detail::layout_linear(constraints, ctx, 0.0f, true);
}

void TableCaption::draw(const DrawContext &ctx, Painter &painter) {
  detail::draw_container_box(ctx, painter);
}

// -- TableColumn / TableColumnGroup ------------------------------------------

std::unique_ptr<Widget> TableColumn::clone() const {
  auto copy = std::make_unique<TableColumn>();
  copy->span_ = span_;
  return copy;
}

Size<float> TableColumn::layout(Constraints constraints, LayoutContext &ctx) {
  // Placed by the table over the tracks it describes; it never sizes itself.
  return constraints.resolve(ctx.style.layout_size(),
                             {constraints.min_width, constraints.min_height});
}

void TableColumn::draw(const DrawContext &ctx, Painter &painter) {
  detail::draw_container_box(ctx, painter);
}

std::shared_ptr<const StyleSheet> TableColumn::default_stylesheet() const {
  static const auto sheet =
      StyleParser::parse(R"vss(
    col { pointer-events: none; }
  )vss",
                         "table.col.default.vss", StyleOrigin::WidgetDefault)
          .sheet;
  return sheet;
}

std::unique_ptr<Widget> TableColumnGroup::clone() const {
  std::vector<std::unique_ptr<Widget>> columns;
  columns.reserve(children_.size());
  for (const auto &child : children_)
    if (child)
      columns.push_back(clone_widget(*child));
  return std::make_unique<TableColumnGroup>(std::move(columns));
}

void TableColumnGroup::register_children(Registrar &registrar) {
  for (auto &child : children_)
    if (child)
      registrar.take_child(std::move(child));
  children_.clear();
}

Size<float> TableColumnGroup::layout(Constraints constraints,
                                     LayoutContext &ctx) {
  return constraints.resolve(ctx.style.layout_size(),
                             {constraints.min_width, constraints.min_height});
}

void TableColumnGroup::draw(const DrawContext &ctx, Painter &painter) {
  detail::draw_container_box(ctx, painter);
}

std::shared_ptr<const StyleSheet>
TableColumnGroup::default_stylesheet() const {
  static const auto sheet =
      StyleParser::parse(R"vss(
    colgroup { pointer-events: none; }
  )vss",
                         "table.colgroup.default.vss",
                         StyleOrigin::WidgetDefault)
          .sheet;
  return sheet;
}

// -- Table -------------------------------------------------------------------

std::unique_ptr<Widget> Table::clone() const {
  std::vector<std::unique_ptr<Widget>> children;
  children.reserve(children_.size());
  for (const auto &child : children_)
    if (child)
      children.push_back(clone_widget(*child));
  return std::make_unique<Table>(std::move(children));
}

void Table::register_children(Registrar &registrar) {
  for (auto &child : children_)
    if (child)
      registrar.take_child(std::move(child));
  children_.clear();
}

void Table::inherit_runtime(const Widget &previous) {
  const auto &table = static_cast<const Table &>(previous);
  // Only so that a paint arriving before the next layout has something to
  // draw. The next layout overwrites all of it.
  column_widths_ = table.column_widths_;
  row_heights_ = table.row_heights_;
  collapsed_lines_ = table.collapsed_lines_;
  collapsed_ = table.collapsed_;
}

void Table::add_header_row_(std::vector<std::string> labels) {
  std::vector<std::unique_ptr<Widget>> cells;
  cells.reserve(labels.size());
  for (std::string &label : labels)
    cells.push_back(std::make_unique<TableHeaderCell>(std::move(label)));
  auto head = std::make_unique<TableHead>();
  head->add(std::make_unique<TableRow>(std::move(cells)));
  children_.push_back(std::move(head));
}

void Table::append_body_row_(std::unique_ptr<Widget> row) {
  // The last body written wins, so `.row()` after an explicit `tbody` extends
  // that one instead of opening a second.
  for (auto it = children_.rbegin(); it != children_.rend(); ++it) {
    if (auto *body = dynamic_cast<TableBody *>(it->get())) {
      body->add(std::move(row));
      return;
    }
  }
  auto body = std::make_unique<TableBody>();
  body->add(std::move(row));
  children_.push_back(std::move(body));
}

SelectorBuilder Table::striped_row_selector() {
  return SelectorBuilder::of<TableRow>().nth_child(2, 0);
}

std::shared_ptr<const StyleSheet> Table::default_stylesheet() const {
  static const auto sheet =
      StyleParser::parse(R"vss(
    caption { padding: 4px 0px; text-align: center; }
  )vss",
                         "table.default.vss", StyleOrigin::WidgetDefault)
          .sheet;
  return sheet;
}

Size<float> Table::layout(Constraints constraints, LayoutContext &ctx) {
  collapsed_ =
      ctx.style.get<styles::BorderCollapse>() == BorderCollapse::Collapse;
  const bool fixed_requested =
      ctx.style.get<styles::TableLayout>() == TableLayout::Fixed;
  const BorderSpacing spacing = ctx.style.get<styles::BorderSpacing>();
  const float table_border = border_width_of(ctx.style);
  const Brush table_border_brush = ctx.style.get<styles::BorderColor>();

  // Under `collapse` the table's own border becomes the outermost grid line
  // and its padding goes away, exactly as CSS defines it.
  const Spacing<float> chrome =
      collapsed_ ? Spacing<float>(0.0f)
                 : ctx.style.get<styles::Padding>() +
                       Spacing<float>(table_border);

  // -- Structure ------------------------------------------------------------

  Node *caption_node = nullptr;
  const ComputedStyle *caption_style = nullptr;
  std::vector<GridColumnBox> column_boxes;
  std::vector<GridColumn> columns;
  std::vector<GridSection> sections;
  std::vector<GridRow> rows;
  std::vector<GridCell> cells;

  // Rows are gathered per section kind first, because CSS paints and stacks a
  // table head first and a foot last however the source ordered them.
  struct PendingSection {
    Node *node = nullptr;
    const ComputedStyle *style = nullptr;
    std::vector<FlowChild> rows;
  };
  std::vector<PendingSection> heads, bodies, feet;

  const auto column_box_of = [&](Node *node, std::uint16_t span) {
    const std::size_t first = column_boxes.empty()
                                  ? 0
                                  : column_boxes.back().first_column +
                                        column_boxes.back().column_count;
    column_boxes.push_back({node, first, span});
    while (columns.size() < first + span)
      columns.push_back(GridColumn{});
  };

  for (std::size_t i = 0; i < ctx.child_count(); ++i) {
    Node &child = ctx.child_node(i);
    Node *box = layout_box(&child);
    if (!box)
      continue;
    Widget *widget = box->widget.get();
    const ComputedStyle *style = box->style_node.computed.get();

    if (dynamic_cast<TableCaption *>(widget)) {
      if (!caption_node) {
        caption_node = &child;
        caption_style = style;
      }
      continue;
    }
    if (dynamic_cast<TableColumnGroup *>(widget)) {
      const std::size_t group_first =
          column_boxes.empty() ? 0
                               : column_boxes.back().first_column +
                                     column_boxes.back().column_count;
      const std::size_t group_index = column_boxes.size();
      column_boxes.push_back({&child, group_first, 0});
      std::size_t covered = 0;
      for (const FlowChild &inner : flow_children(*box)) {
        auto *column = dynamic_cast<TableColumn *>(inner.box->widget.get());
        if (!column)
          continue;
        column_box_of(inner.node, column->column_span());
        covered += column->column_span();
      }
      column_boxes[group_index].column_count = std::max<std::size_t>(covered, 1);
      while (columns.size() < group_first + column_boxes[group_index].column_count)
        columns.push_back(GridColumn{});
      continue;
    }
    if (auto *column = dynamic_cast<TableColumn *>(widget)) {
      column_box_of(&child, column->column_span());
      continue;
    }
    if (auto *section = dynamic_cast<TableSection *>(widget)) {
      PendingSection pending{&child, style, flow_children(*box)};
      switch (section->section_kind()) {
      case TableSectionKind::Head:
        heads.push_back(std::move(pending));
        break;
      case TableSectionKind::Body:
        bodies.push_back(std::move(pending));
        break;
      case TableSectionKind::Foot:
        feet.push_back(std::move(pending));
        break;
      }
      continue;
    }
    if (dynamic_cast<TableRow *>(widget)) {
      // A bare row is its own anonymous body, which is what an HTML parser
      // would have built around it.
      bodies.push_back(PendingSection{nullptr, nullptr, {FlowChild{&child, box}}});
      continue;
    }
  }

  // The column descriptions the boxes above pointed at, now that `columns` is
  // as long as it will get from them.
  for (const GridColumnBox &column_box : column_boxes) {
    Node *box = layout_box(column_box.node);
    if (!box || !dynamic_cast<TableColumn *>(box->widget.get()))
      continue;
    const ComputedStyle *style = box->style_node.computed.get();
    for (std::size_t c = column_box.first_column;
         c < column_box.first_column + column_box.column_count &&
         c < columns.size();
         ++c) {
      columns[c].style = style;
      columns[c].length = style->layout_size().width;
      columns[c].has_length = fixed_or_flexible(columns[c].length) != nullptr;
    }
  }

  // -- Cells into tracks ----------------------------------------------------

  // `occupied[row][column]` -- a rowspan reaches into rows that have not been
  // read yet, so the grid has to be filled rather than counted.
  std::vector<std::vector<bool>> occupied;
  const auto reserve_cell = [&](std::size_t row, std::size_t column,
                                std::uint16_t rowspan, std::uint16_t colspan) {
    while (occupied.size() < row + rowspan)
      occupied.emplace_back();
    for (std::size_t r = row; r < row + rowspan; ++r) {
      if (occupied[r].size() < column + colspan)
        occupied[r].resize(column + colspan, false);
      for (std::size_t c = column; c < column + colspan; ++c)
        occupied[r][c] = true;
    }
    while (columns.size() < column + colspan)
      columns.push_back(GridColumn{});
  };

  const auto gather = [&](std::vector<PendingSection> &group) {
    for (PendingSection &pending : group) {
      GridSection section;
      section.node = pending.node;
      section.style = pending.style;
      section.first_row = rows.size();
      for (const FlowChild &row_child : pending.rows) {
        auto *row_widget = dynamic_cast<TableRow *>(row_child.box->widget.get());
        if (!row_widget)
          continue;
        const std::size_t row_index = rows.size();
        GridRow row;
        row.node = row_child.node;
        row.row = row_widget;
        row.style = row_child.box->style_node.computed.get();
        row.section = sections.size();
        rows.push_back(row);

        std::size_t column = 0;
        for (const FlowChild &cell_child : flow_children(*row_child.box)) {
          auto *cell_widget =
              dynamic_cast<TableCell *>(cell_child.box->widget.get());
          if (!cell_widget)
            continue;
          while (row_index < occupied.size() &&
                 column < occupied[row_index].size() &&
                 occupied[row_index][column])
            ++column;
          GridCell cell;
          cell.node = cell_child.node;
          cell.cell = cell_widget;
          cell.style = cell_child.box->style_node.computed.get();
          cell.row = row_index;
          cell.column = column;
          cell.colspan = cell_widget->column_span();
          cell.rowspan = cell_widget->row_span();
          reserve_cell(row_index, column, cell.rowspan, cell.colspan);
          column += cell.colspan;
          cells.push_back(cell);
        }
      }
      section.row_count = rows.size() - section.first_row;
      sections.push_back(section);
    }
  };
  gather(heads);
  gather(bodies);
  gather(feet);

  const std::size_t column_count = columns.size();
  const std::size_t row_count = rows.size();

  // Every part learns whether it owns its border before anything is measured:
  // a collapsed cell has no border of its own, and therefore no border in its
  // chrome either.
  for (GridCell &cell : cells)
    cell.cell->collapsed_ = collapsed_;
  for (GridRow &row : rows)
    row.row->collapsed_ = collapsed_;
  for (GridSection &section : sections) {
    if (!section.node)
      continue;
    if (auto *widget =
            dynamic_cast<TableSection *>(layout_box(section.node)->widget.get()))
      widget->collapsed_ = collapsed_;
  }

  // -- Collapsed grid lines -------------------------------------------------

  std::vector<CollapsedLine> vertical(column_count + 1);
  std::vector<CollapsedLine> horizontal(row_count + 1);
  if (collapsed_) {
    const auto offer_box = [&](const ComputedStyle &style, std::size_t c0,
                               std::size_t c1, std::size_t r0,
                               std::size_t r1) {
      const float width = border_width_of(style);
      if (width <= 0.0f)
        return;
      const Brush brush = style.get<styles::BorderColor>();
      if (c0 <= column_count)
        vertical[c0].offer(width, brush);
      if (c1 <= column_count)
        vertical[c1].offer(width, brush);
      if (r0 <= row_count)
        horizontal[r0].offer(width, brush);
      if (r1 <= row_count)
        horizontal[r1].offer(width, brush);
    };
    for (const GridCell &cell : cells)
      offer_box(*cell.style, cell.column,
                std::min(cell.column + cell.colspan, column_count), cell.row,
                std::min<std::size_t>(cell.row + cell.rowspan, row_count));
    for (std::size_t i = 0; i < rows.size(); ++i)
      if (rows[i].style)
        offer_box(*rows[i].style, 0, column_count, i, i + 1);
    for (const GridSection &section : sections)
      if (section.style && section.row_count)
        offer_box(*section.style, 0, column_count, section.first_row,
                  section.first_row + section.row_count);
    for (std::size_t c = 0; c < column_count; ++c)
      if (columns[c].style)
        offer_box(*columns[c].style, c, c + 1, 0, row_count);
    if (table_border > 0.0f) {
      vertical.front().offer(table_border, table_border_brush);
      vertical.back().offer(table_border, table_border_brush);
      horizontal.front().offer(table_border, table_border_brush);
      horizontal.back().offer(table_border, table_border_brush);
    }
  }

  /// The space between column `index - 1` and column `index`, counting the
  /// outer edges. Separate tables spend `border-spacing` here; collapsed ones
  /// spend the shared border, and nothing else is allowed into the strip.
  const auto column_gap = [&](std::size_t index) {
    return collapsed_ ? vertical[index].width : spacing.horizontal;
  };
  const auto row_gap = [&](std::size_t index) {
    return collapsed_ ? horizontal[index].width : spacing.vertical;
  };

  float gaps_width = 0.0f;
  for (std::size_t c = 0; c <= column_count; ++c)
    gaps_width += column_gap(c);
  float gaps_height = 0.0f;
  for (std::size_t r = 0; r <= row_count; ++r)
    gaps_height += row_gap(r);

  // -- Column widths --------------------------------------------------------

  const float horizontal_chrome_total = chrome.left + chrome.right;
  const Length declared_width = ctx.style.layout_size().width;
  float outer_limit = constraints.max_width;
  if (const auto *fixed = std::get_if<Length::Fixed>(&declared_width.value))
    outer_limit = std::clamp(fixed->value, constraints.min_width,
                             constraints.max_width);
  const bool definite_width =
      std::isfinite(outer_limit) &&
      (std::holds_alternative<Length::Fixed>(declared_width.value) ||
       flex_weight(declared_width) > 0.0f);
  const float available_content =
      std::isfinite(outer_limit)
          ? std::max(outer_limit - horizontal_chrome_total - gaps_width, 0.0f)
          : kInfinity;

  for (std::size_t c = 0; c < column_count; ++c) {
    if (const auto *fixed = std::get_if<Length::Fixed>(&columns[c].length.value)) {
      columns[c].min_width = std::max(columns[c].min_width, fixed->value);
      columns[c].max_width = std::max(columns[c].max_width, fixed->value);
    }
  }

  // A fixed layout divides a width it has been given. Asked how wide it would
  // like to be -- by a row measuring its children, or a scroll viewport -- it
  // has no width to divide, and CSS falls back to the content-driven algorithm
  // rather than to a table of empty columns.
  const bool fixed_layout = fixed_requested && std::isfinite(available_content);

  if (fixed_layout) {
    // `table-layout: fixed` never measures content. Columns without a width of
    // their own take one from the first row, and whatever is left over is
    // shared out equally.
    std::vector<bool> settled(column_count, false);
    for (std::size_t c = 0; c < column_count; ++c)
      settled[c] = std::holds_alternative<Length::Fixed>(columns[c].length.value);
    for (const GridCell &cell : cells) {
      if (cell.row != 0 || cell.colspan != 1 || cell.column >= column_count ||
          settled[cell.column])
        continue;
      const Length &length = cell.style->layout_size().width;
      if (const auto *fixed = std::get_if<Length::Fixed>(&length.value)) {
        columns[cell.column].max_width = fixed->value;
        columns[cell.column].min_width = fixed->value;
        settled[cell.column] = true;
      }
    }
    float assigned = 0.0f;
    std::size_t unsettled = 0;
    for (std::size_t c = 0; c < column_count; ++c) {
      if (settled[c])
        assigned += columns[c].max_width;
      else
        ++unsettled;
    }
    const float leftover =
        std::isfinite(available_content)
            ? std::max(available_content - assigned, 0.0f)
            : 0.0f;
    const float each =
        unsettled ? leftover / static_cast<float>(unsettled) : 0.0f;
    for (std::size_t c = 0; c < column_count; ++c)
      columns[c].width = settled[c] ? columns[c].max_width : each;
  } else {
    // Auto layout. One probe per cell for its widest form; the narrowest form
    // is only worth asking for when the widest does not fit.
    for (GridCell &cell : cells) {
      ctx.constrain_node(*cell.node, Constraints(0.0f, kInfinity, 0.0f,
                                                 kInfinity));
      cell.max_content = cell.cell->measured_width();
      if (const auto *fixed =
              std::get_if<Length::Fixed>(&cell.style->layout_size().width.value))
        cell.max_content = std::max(cell.max_content, fixed->value);
      cell.min_content = cell.max_content;
    }
    for (const GridCell &cell : cells) {
      if (cell.colspan != 1 || cell.column >= column_count)
        continue;
      columns[cell.column].max_width =
          std::max(columns[cell.column].max_width, cell.max_content);
    }
    float sum_max = 0.0f;
    for (const GridColumn &column : columns)
      sum_max += column.max_width;

    const bool needs_minimums =
        std::isfinite(available_content) && sum_max > available_content;
    if (needs_minimums) {
      for (GridCell &cell : cells) {
        const float probe =
            horizontal_chrome(*cell.style, collapsed_) + 1.0f;
        ctx.constrain_node(*cell.node,
                           Constraints(0.0f, probe, 0.0f, kInfinity));
        cell.min_content = cell.cell->measured_width();
        if (const auto *fixed = std::get_if<Length::Fixed>(
                &cell.style->layout_size().width.value))
          cell.min_content = std::max(cell.min_content, fixed->value);
        cell.min_content = std::min(cell.min_content, cell.max_content);
      }
    }
    for (const GridCell &cell : cells) {
      if (cell.colspan != 1 || cell.column >= column_count)
        continue;
      columns[cell.column].min_width =
          std::max(columns[cell.column].min_width, cell.min_content);
    }

    // Spanning cells come last and only add what the columns they cross
    // cannot already supply, which is the rule that keeps `colspan` from
    // inflating a column that a plain cell already sized.
    std::vector<const GridCell *> spanning;
    for (const GridCell &cell : cells)
      if (cell.colspan > 1)
        spanning.push_back(&cell);
    std::sort(spanning.begin(), spanning.end(),
              [](const GridCell *a, const GridCell *b) {
                return a->colspan < b->colspan;
              });
    for (const GridCell *cell : spanning) {
      const std::size_t first = cell->column;
      const std::size_t last = std::min<std::size_t>(
          cell->column + cell->colspan, column_count);
      if (first >= last)
        continue;
      float inner_gaps = 0.0f;
      for (std::size_t c = first + 1; c < last; ++c)
        inner_gaps += column_gap(c);
      const auto spread = [&](float required, float GridColumn::*field) {
        float total = 0.0f;
        std::vector<float> weights(last - first, 0.0f);
        for (std::size_t c = first; c < last; ++c) {
          total += columns[c].*field;
          weights[c - first] = columns[c].max_width;
        }
        const float deficit = required - inner_gaps - total;
        if (deficit <= 0.0f)
          return;
        bool any = false;
        for (float weight : weights)
          any = any || weight > 0.0f;
        if (!any)
          std::fill(weights.begin(), weights.end(), 1.0f);
        std::vector<float> shares(weights.size(), 0.0f);
        distribute(deficit, weights, shares);
        for (std::size_t c = first; c < last; ++c)
          columns[c].*field += shares[c - first];
      };
      spread(cell->max_content, &GridColumn::max_width);
      spread(cell->min_content, &GridColumn::min_width);
    }

    for (GridColumn &column : columns)
      column.max_width = std::max(column.max_width, column.min_width);

    float total_min = 0.0f, total_max = 0.0f;
    for (const GridColumn &column : columns) {
      total_min += column.min_width;
      total_max += column.max_width;
    }

    float target = total_max;
    if (definite_width)
      target = available_content;
    else if (std::isfinite(available_content))
      target = std::clamp(total_max, std::min(total_min, available_content),
                          available_content);

    for (GridColumn &column : columns)
      column.width = column.max_width;

    if (target > total_max) {
      // Flexible columns take the slack first; without any, it is shared out
      // in proportion to what each column already asked for, so a wide column
      // stays wide.
      std::vector<float> weights(column_count, 0.0f);
      bool flexible = false;
      for (std::size_t c = 0; c < column_count; ++c) {
        weights[c] = flex_weight(columns[c].length);
        flexible = flexible || weights[c] > 0.0f;
      }
      if (!flexible)
        for (std::size_t c = 0; c < column_count; ++c)
          weights[c] = columns[c].max_width > 0.0f ? columns[c].max_width : 1.0f;
      std::vector<float> shares(column_count, 0.0f);
      distribute(target - total_max, weights, shares);
      for (std::size_t c = 0; c < column_count; ++c)
        columns[c].width += shares[c];
    } else if (target < total_max) {
      const float shrinkable = total_max - total_min;
      if (shrinkable <= 0.0f || target <= total_min) {
        for (GridColumn &column : columns)
          column.width = column.min_width;
      } else {
        std::vector<float> weights(column_count, 0.0f);
        for (std::size_t c = 0; c < column_count; ++c)
          weights[c] = columns[c].max_width - columns[c].min_width;
        std::vector<float> shares(column_count, 0.0f);
        distribute(total_max - target, weights, shares);
        for (std::size_t c = 0; c < column_count; ++c)
          columns[c].width = columns[c].max_width - shares[c];
      }
    }
  }

  for (GridColumn &column : columns)
    column.width = std::max(column.width, 0.0f);

  float x = chrome.left;
  for (std::size_t c = 0; c < column_count; ++c) {
    x += column_gap(c);
    columns[c].x = x;
    x += columns[c].width;
  }
  const float grid_width = column_count ? x + column_gap(column_count) - chrome.left
                                        : gaps_width;

  // -- Row heights ----------------------------------------------------------

  const auto cell_width = [&](const GridCell &cell) {
    const std::size_t first = cell.column;
    const std::size_t last =
        std::min<std::size_t>(cell.column + cell.colspan, column_count);
    float width = 0.0f;
    for (std::size_t c = first; c < last; ++c) {
      width += columns[c].width;
      if (c > first)
        width += column_gap(c);
    }
    return width;
  };

  for (GridCell &cell : cells) {
    cell.cell->baseline_shift_ = 0.0f;
    cell.width = cell_width(cell);
    const Size<float> size = ctx.constrain_node(
        *cell.node, Constraints(cell.width, cell.width, 0.0f, kInfinity));
    cell.height = size.height;
  }

  // Baseline alignment is a row-wide question: a cell can report where its
  // first baseline sits, but only the row can say how far each one has to drop
  // for them to meet.
  for (GridRow &row : rows)
    row.baseline = 0.0f;
  for (const GridCell &cell : cells) {
    if (cell.rowspan != 1 || cell.row >= row_count)
      continue;
    if (cell.style->get<styles::VerticalAlign>() != VerticalAlign::Baseline)
      continue;
    if (const std::optional<float> baseline = cell.cell->first_baseline())
      rows[cell.row].baseline = std::max(rows[cell.row].baseline, *baseline);
  }
  for (GridCell &cell : cells) {
    if (cell.rowspan != 1 || cell.row >= row_count)
      continue;
    if (cell.style->get<styles::VerticalAlign>() != VerticalAlign::Baseline)
      continue;
    const std::optional<float> baseline = cell.cell->first_baseline();
    if (!baseline)
      continue;
    const float shift = rows[cell.row].baseline - *baseline;
    if (shift <= 0.0f)
      continue;
    cell.cell->baseline_shift_ = shift;
    cell.height += shift;
  }

  for (std::size_t r = 0; r < row_count; ++r) {
    float height = 0.0f;
    if (rows[r].style)
      if (const auto *fixed = std::get_if<Length::Fixed>(
              &rows[r].style->layout_size().height.value))
        height = fixed->value;
    rows[r].height = height;
  }
  for (const GridCell &cell : cells) {
    if (cell.rowspan != 1 || cell.row >= row_count)
      continue;
    rows[cell.row].height = std::max(rows[cell.row].height, cell.height);
  }
  // A cell that spans rows only lengthens them when what they already have is
  // not enough, and then in proportion to what each already holds.
  for (const GridCell &cell : cells) {
    if (cell.rowspan <= 1 || cell.row >= row_count)
      continue;
    const std::size_t first = cell.row;
    const std::size_t last =
        std::min<std::size_t>(cell.row + cell.rowspan, row_count);
    float spanned = 0.0f;
    std::vector<float> weights(last - first, 0.0f);
    for (std::size_t r = first; r < last; ++r) {
      spanned += rows[r].height;
      weights[r - first] = rows[r].height;
      if (r > first)
        spanned += row_gap(r);
    }
    const float deficit = cell.height - spanned;
    if (deficit <= 0.0f)
      continue;
    bool any = false;
    for (float weight : weights)
      any = any || weight > 0.0f;
    if (!any)
      std::fill(weights.begin(), weights.end(), 1.0f);
    std::vector<float> shares(weights.size(), 0.0f);
    distribute(deficit, weights, shares);
    for (std::size_t r = first; r < last; ++r)
      rows[r].height += shares[r - first];
  }

  // -- The caption ----------------------------------------------------------

  const CaptionSide caption_side =
      caption_style ? caption_style->get<styles::CaptionSide>()
                    : ctx.style.get<styles::CaptionSide>();
  float caption_height = 0.0f;
  if (caption_node) {
    const Size<float> size = ctx.constrain_node(
        *caption_node, Constraints(grid_width, grid_width, 0.0f, kInfinity));
    caption_height = size.height;
  }

  // -- The table box --------------------------------------------------------

  float grid_height = gaps_height;
  for (const GridRow &row : rows)
    grid_height += row.height;

  const Size<float> intrinsic{grid_width + chrome.left + chrome.right,
                              grid_height + caption_height + chrome.top +
                                  chrome.bottom};
  const Size<float> resolved =
      constraints.resolve(ctx.style.layout_size(), intrinsic);

  // A definite height is shared out over the rows, which is what makes
  // `height: fill` on a table stretch its body rather than leave a gap.
  const float slack = resolved.height - intrinsic.height;
  if (slack > 0.0f && row_count > 0) {
    std::vector<float> weights(row_count, 0.0f);
    for (std::size_t r = 0; r < row_count; ++r)
      weights[r] = rows[r].height > 0.0f ? rows[r].height : 1.0f;
    std::vector<float> shares(row_count, 0.0f);
    distribute(slack, weights, shares);
    for (std::size_t r = 0; r < row_count; ++r)
      rows[r].height += shares[r];
    grid_height += slack;
  }

  const float grid_top =
      chrome.top + (caption_side == CaptionSide::Top ? caption_height : 0.0f);

  float y = grid_top;
  for (std::size_t r = 0; r < row_count; ++r) {
    y += row_gap(r);
    rows[r].y = y;
    y += rows[r].height;
  }

  // -- Placement ------------------------------------------------------------

  if (caption_node) {
    ctx.place_node(*caption_node,
                   {chrome.left, caption_side == CaptionSide::Top
                                     ? chrome.top
                                     : grid_top + grid_height});
  }

  // Columns first: CSS paints a column's background above the table's and
  // below every row's, and declaration order already puts them there.
  for (const GridColumnBox &column_box : column_boxes) {
    if (!column_box.node || column_box.first_column >= column_count)
      continue;
    const std::size_t last = std::min(
        column_box.first_column + column_box.column_count, column_count);
    float width = 0.0f;
    for (std::size_t c = column_box.first_column; c < last; ++c) {
      width += columns[c].width;
      if (c > column_box.first_column)
        width += column_gap(c);
    }
    ctx.constrain_node(*column_box.node,
                       Constraints(width, width, grid_height, grid_height));
    ctx.place_node(*column_box.node,
                   {columns[column_box.first_column].x, grid_top});
  }

  const float row_left = chrome.left + column_gap(0);
  const float row_width = std::max(grid_width - column_gap(0) -
                                       column_gap(column_count),
                                   0.0f);

  for (std::size_t s = 0; s < sections.size(); ++s) {
    GridSection &section = sections[s];
    if (!section.node || section.row_count == 0)
      continue;
    const GridRow &first = rows[section.first_row];
    const GridRow &last = rows[section.first_row + section.row_count - 1];
    const float height = last.y + last.height - first.y;
    Node *section_box = layout_box(section.node);
    if (auto *widget =
            dynamic_cast<TableSection *>(section_box->widget.get()))
      widget->driven_ = true;
    ctx.constrain_node(*section.node,
                       Constraints(row_width, row_width, height, height));
    ctx.place_node(*section.node, {row_left, first.y});
  }

  for (GridRow &row : rows) {
    row.row->driven_ = true;
    ctx.constrain_node(*row.node, Constraints(row_width, row_width, row.height,
                                              row.height));
    ctx.place_node(*row.node, {row_left, row.y});
  }

  for (GridCell &cell : cells) {
    const std::size_t last_row =
        std::min<std::size_t>(cell.row + cell.rowspan, row_count);
    float height = 0.0f;
    for (std::size_t r = cell.row; r < last_row; ++r) {
      height += rows[r].height;
      if (r > cell.row)
        height += row_gap(r);
    }
    ctx.constrain_node(*cell.node, Constraints(cell.width, cell.width, height,
                                               height));
    ctx.place_node(*cell.node,
                   {cell.column < column_count ? columns[cell.column].x
                                               : chrome.left,
                    cell.row < row_count ? rows[cell.row].y : grid_top});
  }

  // -- What painting needs --------------------------------------------------

  column_widths_.clear();
  column_widths_.reserve(column_count);
  for (const GridColumn &column : columns)
    column_widths_.push_back(column.width);
  row_heights_.clear();
  row_heights_.reserve(row_count);
  for (const GridRow &row : rows)
    row_heights_.push_back(row.height);

  collapsed_lines_.clear();
  if (collapsed_ && column_count && row_count) {
    const float left = chrome.left;
    const float right = left + grid_width;
    const float top = grid_top;
    const float bottom = top + grid_height;
    for (std::size_t c = 0; c <= column_count; ++c) {
      const CollapsedLine &line = vertical[c];
      if (line.width <= 0.0f)
        continue;
      const float line_x = c < column_count
                               ? columns[c].x - line.width
                               : right - line.width;
      collapsed_lines_.push_back(
          {Rect<float>(line_x, top, line.width, bottom - top), line.brush});
    }
    for (std::size_t r = 0; r <= row_count; ++r) {
      const CollapsedLine &line = horizontal[r];
      if (line.width <= 0.0f)
        continue;
      const float line_y =
          r < row_count ? rows[r].y - line.width : bottom - line.width;
      collapsed_lines_.push_back(
          {Rect<float>(left, line_y, right - left, line.width), line.brush});
    }
  }

  return resolved;
}

void Table::draw(const DrawContext &ctx, Painter &painter) {
  const Radius radius = ctx.style.get<styles::BorderRadius>();
  painter.fill_rrect(ctx.bounds, radius,
                     Paint(ctx.style.get<styles::Background>()));
  if (collapsed_)
    return;
  const float border = border_width_of(ctx.style);
  if (border > 0.0f)
    painter.stroke_rrect(ctx.bounds, radius,
                         Paint(ctx.style.get<styles::BorderColor>()),
                         Pen(border, StrokeAlign::Inside));
}

void Table::draw_foreground(const DrawContext &ctx, Painter &painter) {
  // Collapsed borders are drawn after the cells because they belong to the
  // table, not to either neighbour -- and they cover nothing, because layout
  // reserved the strip they occupy.
  for (const detail::TableGridLine &line : collapsed_lines_) {
    const Rect<float> rect{line.rect.origin.x + ctx.bounds.origin.x,
                           line.rect.origin.y + ctx.bounds.origin.y,
                           line.rect.size.width, line.rect.size.height};
    painter.fill_rect(rect, Paint(line.brush));
  }
}

} // namespace voidui
