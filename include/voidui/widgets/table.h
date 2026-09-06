#pragma once

#include <cstdint>
#include <memory>
#include <optional>
#include <string>
#include <type_traits>
#include <utility>
#include <vector>

#include "voidui/core/border.h"
#include "voidui/core/context.h"
#include "voidui/core/style.h"
#include "voidui/core/widget.h"
#include "voidui/widgets/text.h"

/// A CSS table.
///
/// The parts are the ones CSS names, because that is what makes a stylesheet
/// written for a table look like a stylesheet:
///
///   table > caption | colgroup > col | thead | tbody | tfoot > tr > td | th
///
/// and they are separate widget types rather than one widget with a mode
/// flag, so `th`, `td` and `tr` are selectors and not conventions:
///
///   table  { border-collapse: collapse; table-layout: fixed; }
///   th, td { padding: 8 12; border-width: 1; border-color: #d8e0eb; }
///   th     { background: #f1f5f9; text-align: left; }
///   tr:nth-child(even) { background: #fafbfc; }
///   tbody tr:hover     { background: #eff6ff; }
///
/// One table is one layout. A column's width cannot be settled by a row and a
/// row's height cannot be settled by a cell, so `Table::layout` measures and
/// places the whole grid itself through LayoutContext::constrain_node, and the
/// sections, rows and cells beneath it are handed the geometry it worked out.
/// That is also why they are ordinary nodes and not an internal shadow tree:
/// they take part in the cascade, in hit testing and in `:hover` exactly like
/// any other widget.

namespace voidui {

class Table;

// -- Cells -------------------------------------------------------------------

/// A data cell.
///
/// A cell stacks its children vertically inside its padding, then aligns the
/// block: horizontally by the inherited `text-align`, vertically by its own
/// `vertical-align`. `baseline` lines the first text baselines of a row's
/// cells up against each other; the row does that part, because it is the only
/// participant that can see all of them.
class TableCell : public Widget {
public:
  VOIDUI_STYLE_SCOPE(TableCell, "td")

  TableCell() = default;

  explicit TableCell(std::string content) {
    children_.push_back(std::make_unique<Text>(std::move(content)));
  }

  explicit TableCell(const char *content)
      : TableCell(std::string(content ? content : "")) {}

  template <WidgetClass... Children>
    requires(sizeof...(Children) > 0)
  explicit TableCell(Children &&...children) {
    (children_.push_back(transfer_widget(std::forward<Children>(children))),
     ...);
  }

  explicit TableCell(std::vector<std::unique_ptr<Widget>> children)
      : children_(std::move(children)) {}

  VOIDUI_WIDGET_SIZE_STYLE

#define VOIDUI_TABLE_PART_STYLE(name, property)                                \
  template <typename Self>                                                     \
  Self &&name(this Self &&self, styles::property::Value value) {               \
    self.template set_style<styles::property>(std::move(value));               \
    return std::forward<Self>(self);                                           \
  }
  VOIDUI_TABLE_PART_STYLE(background, Background)
  VOIDUI_TABLE_PART_STYLE(color, Foreground)
  VOIDUI_TABLE_PART_STYLE(padding, Padding)
  VOIDUI_TABLE_PART_STYLE(font_size, FontSize)
  VOIDUI_TABLE_PART_STYLE(font_family, FontFamily)
  VOIDUI_TABLE_PART_STYLE(font_weight, FontWeight)
  VOIDUI_TABLE_PART_STYLE(line_height, LineHeight)
  VOIDUI_TABLE_PART_STYLE(text_align, TextAlign)
  VOIDUI_TABLE_PART_STYLE(vertical_align, VerticalAlign)
  VOIDUI_TABLE_PART_STYLE(border_radius, BorderRadius)
  VOIDUI_TABLE_PART_STYLE(border_width, BorderWidth)
  VOIDUI_TABLE_PART_STYLE(border_color, BorderColor)
  VOIDUI_TABLE_PART_STYLE(box_shadow, BoxShadow)
  VOIDUI_TABLE_PART_STYLE(cursor, Cursor)
#undef VOIDUI_TABLE_PART_STYLE

  VOIDUI_FLUENT_METHOD(border, (Border value),
                       set_style<styles::BorderRadius>(value.get_radius());
                       set_style<styles::BorderWidth>(value.get_width());
                       set_style<styles::BorderColor>(value.get_brush());)

  /// Zero is read as one, as HTML reads `colspan="0"`'s sibling cases: a cell
  /// always occupies at least the track it starts in.
  VOIDUI_FLUENT_METHOD(colspan, (std::uint16_t value),
                       colspan_ = value ? value : 1;)
  VOIDUI_FLUENT_METHOD(rowspan, (std::uint16_t value),
                       rowspan_ = value ? value : 1;)

  template <WidgetClass T> TableCell &add(T &&child) & {
    children_.push_back(transfer_widget(std::forward<T>(child)));
    return *this;
  }
  template <WidgetClass T> TableCell &&add(T &&child) && {
    children_.push_back(transfer_widget(std::forward<T>(child)));
    return std::move(*this);
  }
  TableCell &add(std::unique_ptr<Widget> child) & {
    children_.push_back(std::move(child));
    return *this;
  }
  TableCell &&add(std::unique_ptr<Widget> child) && {
    children_.push_back(std::move(child));
    return std::move(*this);
  }

  std::uint16_t column_span() const { return colspan_; }
  std::uint16_t row_span() const { return rowspan_; }

  /// The border-box width the last measurement wanted, before the constraint
  /// clamped it. Column sizing reads this: a probe that came back clamped to
  /// the probe width would say nothing about the content behind it.
  float measured_width() const { return measured_width_; }
  float measured_height() const { return measured_height_; }

  std::unique_ptr<Widget> clone() const override;
  void register_children(Registrar &registrar) override;
  Size<float> layout(Constraints constraints, LayoutContext &ctx) override;
  void draw(const DrawContext &ctx, Painter &painter) override;
  EventResult on_event(Event &) override { return EventResult::Unhandled; }
  std::optional<float> first_baseline() const override { return baseline_; }
  std::shared_ptr<const StyleSheet> default_stylesheet() const override;

protected:
  void copy_cell_fields_(TableCell &copy) const;

private:
  friend class Table;

  std::vector<std::unique_ptr<Widget>> children_;
  std::uint16_t colspan_ = 1;
  std::uint16_t rowspan_ = 1;

  // Written by the table between passes; read back by the next layout of this
  // cell. None of it survives into a clone: it describes a node in a grid.
  float measured_width_ = 0.0f;
  float measured_height_ = 0.0f;
  float baseline_shift_ = 0.0f;
  std::optional<float> baseline_;
  bool collapsed_ = false;
  bool empty_ = true;
};

/// A header cell. Identical to `td` in behaviour and a separate type in the
/// cascade, exactly as HTML has it.
class TableHeaderCell : public TableCell {
public:
  VOIDUI_STYLE_SCOPE(TableHeaderCell, "th")

  using TableCell::TableCell;

  std::unique_ptr<Widget> clone() const override;
  std::shared_ptr<const StyleSheet> default_stylesheet() const override;
};

// -- Rows and sections -------------------------------------------------------

/// A row.
///
/// Inside a table its geometry is the table's to decide, so its own layout
/// only accepts what it is given. Used on its own -- which is a mistake, but
/// one worth surviving -- it falls back to laying its cells out side by side.
class TableRow : public Widget {
public:
  VOIDUI_STYLE_SCOPE(TableRow, "tr")

  TableRow() = default;

  template <WidgetClass... Cells>
    requires(sizeof...(Cells) > 0)
  explicit TableRow(Cells &&...cells) {
    (children_.push_back(transfer_widget(std::forward<Cells>(cells))), ...);
  }

  explicit TableRow(std::vector<std::unique_ptr<Widget>> cells)
      : children_(std::move(cells)) {}

  VOIDUI_WIDGET_SIZE_STYLE
  VOIDUI_FLUENT_METHOD(background, (Brush value),
                       set_style<styles::Background>(std::move(value));)
  VOIDUI_FLUENT_METHOD(color, (Brush value),
                       set_style<styles::Foreground>(std::move(value));)
  VOIDUI_FLUENT_METHOD(border, (Border value),
                       set_style<styles::BorderRadius>(value.get_radius());
                       set_style<styles::BorderWidth>(value.get_width());
                       set_style<styles::BorderColor>(value.get_brush());)

  template <WidgetClass T> TableRow &add(T &&cell) & {
    children_.push_back(transfer_widget(std::forward<T>(cell)));
    return *this;
  }
  template <WidgetClass T> TableRow &&add(T &&cell) && {
    children_.push_back(transfer_widget(std::forward<T>(cell)));
    return std::move(*this);
  }
  TableRow &add(std::unique_ptr<Widget> cell) & {
    children_.push_back(std::move(cell));
    return *this;
  }
  TableRow &&add(std::unique_ptr<Widget> cell) && {
    children_.push_back(std::move(cell));
    return std::move(*this);
  }

  std::unique_ptr<Widget> clone() const override;
  void register_children(Registrar &registrar) override;
  Size<float> layout(Constraints constraints, LayoutContext &ctx) override;
  void draw(const DrawContext &ctx, Painter &painter) override;
  EventResult on_event(Event &) override { return EventResult::Unhandled; }

private:
  friend class Table;

  std::vector<std::unique_ptr<Widget>> children_;
  bool driven_ = false;
  bool collapsed_ = false;
};

enum class TableSectionKind : std::uint8_t { Head, Body, Foot };

/// The shared behaviour of `thead`, `tbody` and `tfoot`. Never instantiated
/// on its own: the three are separate types so that they are separate
/// selectors and so that the table can order them the way CSS does, head
/// first and foot last, whatever order they were written in.
class TableSection : public Widget {
public:
  TableSection() = default;

  template <WidgetClass... Rows>
    requires(sizeof...(Rows) > 0)
  explicit TableSection(Rows &&...rows) {
    (children_.push_back(transfer_widget(std::forward<Rows>(rows))), ...);
  }

  explicit TableSection(std::vector<std::unique_ptr<Widget>> rows)
      : children_(std::move(rows)) {}

  virtual TableSectionKind section_kind() const = 0;

  VOIDUI_WIDGET_SIZE_STYLE
  VOIDUI_FLUENT_METHOD(background, (Brush value),
                       set_style<styles::Background>(std::move(value));)
  VOIDUI_FLUENT_METHOD(color, (Brush value),
                       set_style<styles::Foreground>(std::move(value));)
  VOIDUI_FLUENT_METHOD(border, (Border value),
                       set_style<styles::BorderRadius>(value.get_radius());
                       set_style<styles::BorderWidth>(value.get_width());
                       set_style<styles::BorderColor>(value.get_brush());)

  template <WidgetClass T> TableSection &add(T &&row) & {
    children_.push_back(transfer_widget(std::forward<T>(row)));
    return *this;
  }
  template <WidgetClass T> TableSection &&add(T &&row) && {
    children_.push_back(transfer_widget(std::forward<T>(row)));
    return std::move(*this);
  }
  TableSection &add(std::unique_ptr<Widget> row) & {
    children_.push_back(std::move(row));
    return *this;
  }
  TableSection &&add(std::unique_ptr<Widget> row) && {
    children_.push_back(std::move(row));
    return std::move(*this);
  }

  void register_children(Registrar &registrar) override;
  Size<float> layout(Constraints constraints, LayoutContext &ctx) override;
  void draw(const DrawContext &ctx, Painter &painter) override;
  EventResult on_event(Event &) override { return EventResult::Unhandled; }

protected:
  std::vector<std::unique_ptr<Widget>> take_children_() const;

private:
  friend class Table;

  std::vector<std::unique_ptr<Widget>> children_;
  bool driven_ = false;
  bool collapsed_ = false;
};

#define VOIDUI_TABLE_SECTION(ClassName, ScopeName, Kind)                       \
  class ClassName : public TableSection {                                      \
  public:                                                                      \
    VOIDUI_STYLE_SCOPE(ClassName, ScopeName)                                   \
    using TableSection::TableSection;                                          \
    TableSectionKind section_kind() const override {                           \
      return TableSectionKind::Kind;                                           \
    }                                                                          \
    std::unique_ptr<Widget> clone() const override {                           \
      return std::make_unique<ClassName>(take_children_());                    \
    }                                                                          \
  }

VOIDUI_TABLE_SECTION(TableHead, "thead", Head);
VOIDUI_TABLE_SECTION(TableBody, "tbody", Body);
VOIDUI_TABLE_SECTION(TableFoot, "tfoot", Foot);
#undef VOIDUI_TABLE_SECTION

// -- Caption and columns -----------------------------------------------------

/// The table's caption, placed above or below the grid by `caption-side`.
class TableCaption : public Widget {
public:
  VOIDUI_STYLE_SCOPE(TableCaption, "caption")

  TableCaption() = default;

  explicit TableCaption(std::string content) {
    children_.push_back(std::make_unique<Text>(std::move(content)));
  }

  explicit TableCaption(const char *content)
      : TableCaption(std::string(content ? content : "")) {}

  template <WidgetClass... Children>
    requires(sizeof...(Children) > 0)
  explicit TableCaption(Children &&...children) {
    (children_.push_back(transfer_widget(std::forward<Children>(children))),
     ...);
  }

  explicit TableCaption(std::vector<std::unique_ptr<Widget>> children)
      : children_(std::move(children)) {}

  VOIDUI_WIDGET_SIZE_STYLE
  VOIDUI_FLUENT_METHOD(background, (Brush value),
                       set_style<styles::Background>(std::move(value));)
  VOIDUI_FLUENT_METHOD(color, (Brush value),
                       set_style<styles::Foreground>(std::move(value));)
  VOIDUI_FLUENT_METHOD(padding, (Padding value),
                       set_style<styles::Padding>(value);)
  VOIDUI_FLUENT_METHOD(caption_side, (CaptionSide value),
                       set_style<styles::CaptionSide>(value);)

  std::unique_ptr<Widget> clone() const override;
  void register_children(Registrar &registrar) override;
  Size<float> layout(Constraints constraints, LayoutContext &ctx) override;
  void draw(const DrawContext &ctx, Painter &painter) override;
  EventResult on_event(Event &) override { return EventResult::Unhandled; }

private:
  std::vector<std::unique_ptr<Widget>> children_;
};

/// One column. It renders nothing but its own background and border, which is
/// exactly what CSS gives a `col`, and it carries the width the column sizing
/// algorithm should use.
class TableColumn : public Widget {
public:
  VOIDUI_STYLE_SCOPE(TableColumn, "col")

  TableColumn() = default;

  VOIDUI_WIDGET_SIZE_STYLE
  VOIDUI_FLUENT_METHOD(background, (Brush value),
                       set_style<styles::Background>(std::move(value));)
  VOIDUI_FLUENT_METHOD(border, (Border value),
                       set_style<styles::BorderRadius>(value.get_radius());
                       set_style<styles::BorderWidth>(value.get_width());
                       set_style<styles::BorderColor>(value.get_brush());)
  /// How many columns this one description covers, as HTML's `span` does.
  VOIDUI_FLUENT_METHOD(span, (std::uint16_t value), span_ = value ? value : 1;)

  std::uint16_t column_span() const { return span_; }

  std::unique_ptr<Widget> clone() const override;
  void register_children(Registrar &) override {}
  Size<float> layout(Constraints constraints, LayoutContext &ctx) override;
  void draw(const DrawContext &ctx, Painter &painter) override;
  EventResult on_event(Event &) override { return EventResult::Unhandled; }
  std::shared_ptr<const StyleSheet> default_stylesheet() const override;

private:
  std::uint16_t span_ = 1;
};

/// A group of columns. Paints behind the columns it spans.
class TableColumnGroup : public Widget {
public:
  VOIDUI_STYLE_SCOPE(TableColumnGroup, "colgroup")

  TableColumnGroup() = default;

  template <WidgetClass... Columns>
    requires(sizeof...(Columns) > 0)
  explicit TableColumnGroup(Columns &&...columns) {
    (children_.push_back(transfer_widget(std::forward<Columns>(columns))),
     ...);
  }

  explicit TableColumnGroup(std::vector<std::unique_ptr<Widget>> columns)
      : children_(std::move(columns)) {}

  VOIDUI_FLUENT_METHOD(background, (Brush value),
                       set_style<styles::Background>(std::move(value));)
  VOIDUI_FLUENT_METHOD(border, (Border value),
                       set_style<styles::BorderRadius>(value.get_radius());
                       set_style<styles::BorderWidth>(value.get_width());
                       set_style<styles::BorderColor>(value.get_brush());)

  TableColumnGroup &add(std::unique_ptr<Widget> column) & {
    children_.push_back(std::move(column));
    return *this;
  }
  TableColumnGroup &&add(std::unique_ptr<Widget> column) && {
    children_.push_back(std::move(column));
    return std::move(*this);
  }

  std::unique_ptr<Widget> clone() const override;
  void register_children(Registrar &registrar) override;
  Size<float> layout(Constraints constraints, LayoutContext &ctx) override;
  void draw(const DrawContext &ctx, Painter &painter) override;
  EventResult on_event(Event &) override { return EventResult::Unhandled; }
  std::shared_ptr<const StyleSheet> default_stylesheet() const override;

private:
  std::vector<std::unique_ptr<Widget>> children_;
};

namespace detail {

/// One collapsed border, in the table's local coordinates. Under
/// `border-collapse: collapse` the shared line belongs to the table, not to
/// either of the two cells it separates, so the table reserves the space for
/// it during layout and paints it afterwards -- over nothing, because nothing
/// else was allowed into that strip.
struct TableGridLine {
  Rect<float> rect;
  Brush brush = Brush(Color::TRANSPARENT);
};

/// Wraps whatever a fluent row builder was handed: a string becomes a `td`
/// holding a paragraph, a cell is taken as it is, any other widget is put
/// inside a `td`.
inline std::unique_ptr<Widget> table_cell_of(std::string content) {
  return std::make_unique<TableCell>(std::move(content));
}

inline std::unique_ptr<Widget> table_cell_of(const char *content) {
  return std::make_unique<TableCell>(std::string(content ? content : ""));
}

template <WidgetClass T> std::unique_ptr<Widget> table_cell_of(T &&widget) {
  if constexpr (std::is_base_of_v<TableCell, std::remove_cvref_t<T>>) {
    return transfer_widget(std::forward<T>(widget));
  } else {
    return std::make_unique<TableCell>(
        transfer_widget(std::forward<T>(widget)));
  }
}

} // namespace detail

// -- The table ---------------------------------------------------------------

/// A table.
///
/// `table-layout`, `border-collapse`, `border-spacing`, `caption-side` and
/// `empty-cells` are read off this node and govern the whole grid; the first
/// four inherit, so they can also be written on an ancestor. Everything else
/// -- padding, borders, backgrounds, fonts, `text-align`, `vertical-align` --
/// is set on the part it belongs to, from VSS or from the chain.
class Table : public Widget {
public:
  VOIDUI_STYLE_SCOPE(Table, "table")

  Table() = default;

  template <WidgetClass... Children>
    requires(sizeof...(Children) > 0)
  explicit Table(Children &&...children) {
    (children_.push_back(transfer_widget(std::forward<Children>(children))),
     ...);
  }

  explicit Table(std::vector<std::unique_ptr<Widget>> children)
      : children_(std::move(children)) {}

  VOIDUI_WIDGET_SIZE_STYLE

#define VOIDUI_TABLE_STYLE(name, property)                                     \
  template <typename Self>                                                     \
  Self &&name(this Self &&self, styles::property::Value value) {               \
    self.template set_style<styles::property>(std::move(value));               \
    return std::forward<Self>(self);                                           \
  }
  VOIDUI_TABLE_STYLE(background, Background)
  VOIDUI_TABLE_STYLE(color, Foreground)
  VOIDUI_TABLE_STYLE(padding, Padding)
  VOIDUI_TABLE_STYLE(font_size, FontSize)
  VOIDUI_TABLE_STYLE(font_family, FontFamily)
  VOIDUI_TABLE_STYLE(font_weight, FontWeight)
  VOIDUI_TABLE_STYLE(line_height, LineHeight)
  VOIDUI_TABLE_STYLE(text_align, TextAlign)
  VOIDUI_TABLE_STYLE(border_radius, BorderRadius)
  VOIDUI_TABLE_STYLE(border_width, BorderWidth)
  VOIDUI_TABLE_STYLE(border_color, BorderColor)
  VOIDUI_TABLE_STYLE(box_shadow, BoxShadow)
  VOIDUI_TABLE_STYLE(table_layout, TableLayout)
  VOIDUI_TABLE_STYLE(border_collapse, BorderCollapse)
  VOIDUI_TABLE_STYLE(border_spacing, BorderSpacing)
  VOIDUI_TABLE_STYLE(caption_side, CaptionSide)
  VOIDUI_TABLE_STYLE(empty_cells, EmptyCells)
#undef VOIDUI_TABLE_STYLE

  VOIDUI_FLUENT_METHOD(border, (Border value),
                       set_style<styles::BorderRadius>(value.get_radius());
                       set_style<styles::BorderWidth>(value.get_width());
                       set_style<styles::BorderColor>(value.get_brush());)

  template <WidgetClass T> Table &add(T &&child) & {
    children_.push_back(transfer_widget(std::forward<T>(child)));
    return *this;
  }
  template <WidgetClass T> Table &&add(T &&child) && {
    children_.push_back(transfer_widget(std::forward<T>(child)));
    return std::move(*this);
  }
  Table &add(std::unique_ptr<Widget> child) & {
    children_.push_back(std::move(child));
    return *this;
  }
  Table &&add(std::unique_ptr<Widget> child) && {
    children_.push_back(std::move(child));
    return std::move(*this);
  }

  /// Builds a `thead` of `th` cells from plain strings. The long form -- a
  /// `thead` written out -- stays available and composes with this one.
  VOIDUI_FLUENT_METHOD(headers, (std::vector<std::string> labels),
                       add_header_row_(std::move(labels));)

  VOIDUI_FLUENT_METHOD(caption, (std::string content),
                       children_.insert(
                           children_.begin(),
                           std::make_unique<TableCaption>(std::move(content)));)

  /// Appends one row to the table's implicit `tbody`, taking strings, cells,
  /// or any other widget -- a plain widget is wrapped in a `td` the way an
  /// unwrapped value in an HTML table would not be, because here there is no
  /// parser to correct it.
  template <typename Self, typename... Cells>
    requires(sizeof...(Cells) > 0)
  Self &&row(this Self &&self, Cells &&...cells) {
    std::vector<std::unique_ptr<Widget>> made;
    made.reserve(sizeof...(Cells));
    (made.push_back(detail::table_cell_of(std::forward<Cells>(cells))), ...);
    self.append_body_row_(std::make_unique<TableRow>(std::move(made)));
    return std::forward<Self>(self);
  }

  /// The columns and rows the last layout resolved, in logical pixels. Useful
  /// for tests and for anything that wants to draw beside a table.
  const std::vector<float> &column_widths() const { return column_widths_; }
  const std::vector<float> &row_heights() const { return row_heights_; }

  std::unique_ptr<Widget> clone() const override;
  void register_children(Registrar &registrar) override;
  Size<float> layout(Constraints constraints, LayoutContext &ctx) override;
  void draw(const DrawContext &ctx, Painter &painter) override;
  void draw_foreground(const DrawContext &ctx, Painter &painter) override;
  EventResult on_event(Event &) override { return EventResult::Unhandled; }
  void inherit_runtime(const Widget &previous) override;
  std::shared_ptr<const StyleSheet> default_stylesheet() const override;

  /// `table > tbody > tr:nth-child(2n)`, built from C++.
  static SelectorBuilder striped_row_selector();

private:
  void add_header_row_(std::vector<std::string> labels);
  void append_body_row_(std::unique_ptr<Widget> row);

  std::vector<std::unique_ptr<Widget>> children_;

  // Resolved by the last layout; read by painting and by the accessors.
  std::vector<float> column_widths_;
  std::vector<float> row_heights_;
  std::vector<detail::TableGridLine> collapsed_lines_;
  bool collapsed_ = false;
};

// -- Declarations ------------------------------------------------------------

[[nodiscard]] inline Table table() { return Table{}; }

template <WidgetClass... Children>
  requires(sizeof...(Children) > 0)
[[nodiscard]] Table table(Children &&...children) {
  return Table(std::forward<Children>(children)...);
}

[[nodiscard]] inline TableCaption caption(std::string content) {
  return TableCaption(std::move(content));
}

template <WidgetClass... Children>
  requires(sizeof...(Children) > 0)
[[nodiscard]] TableCaption caption(Children &&...children) {
  return TableCaption(std::forward<Children>(children)...);
}

[[nodiscard]] inline TableColumn col() { return TableColumn{}; }

template <WidgetClass... Columns>
  requires(sizeof...(Columns) > 0)
[[nodiscard]] TableColumnGroup colgroup(Columns &&...columns) {
  return TableColumnGroup(std::forward<Columns>(columns)...);
}

[[nodiscard]] inline TableColumnGroup colgroup() { return TableColumnGroup{}; }

#define VOIDUI_TABLE_SECTION_FACTORY(FnName, ClassName)                        \
  [[nodiscard]] inline ClassName FnName() { return ClassName{}; }              \
  template <WidgetClass... Rows>                                               \
    requires(sizeof...(Rows) > 0)                                              \
  [[nodiscard]] ClassName FnName(Rows &&...rows) {                             \
    return ClassName(std::forward<Rows>(rows)...);                             \
  }

VOIDUI_TABLE_SECTION_FACTORY(thead, TableHead)
VOIDUI_TABLE_SECTION_FACTORY(tbody, TableBody)
VOIDUI_TABLE_SECTION_FACTORY(tfoot, TableFoot)
#undef VOIDUI_TABLE_SECTION_FACTORY

[[nodiscard]] inline TableRow tr() { return TableRow{}; }

template <WidgetClass... Cells>
  requires(sizeof...(Cells) > 0)
[[nodiscard]] TableRow tr(Cells &&...cells) {
  return TableRow(std::forward<Cells>(cells)...);
}

/// A row from plain values: `tr_of("Alice", "30", "Berlin")`.
template <typename... Cells>
  requires(sizeof...(Cells) > 0)
[[nodiscard]] TableRow tr_of(Cells &&...cells) {
  std::vector<std::unique_ptr<Widget>> made;
  made.reserve(sizeof...(Cells));
  (made.push_back(detail::table_cell_of(std::forward<Cells>(cells))), ...);
  return TableRow(std::move(made));
}

[[nodiscard]] inline TableCell td() { return TableCell{}; }
[[nodiscard]] inline TableCell td(std::string content) {
  return TableCell(std::move(content));
}

template <WidgetClass... Children>
  requires(sizeof...(Children) > 0)
[[nodiscard]] TableCell td(Children &&...children) {
  return TableCell(std::forward<Children>(children)...);
}

[[nodiscard]] inline TableHeaderCell th() { return TableHeaderCell{}; }
[[nodiscard]] inline TableHeaderCell th(std::string content) {
  return TableHeaderCell(std::move(content));
}

template <WidgetClass... Children>
  requires(sizeof...(Children) > 0)
[[nodiscard]] TableHeaderCell th(Children &&...children) {
  return TableHeaderCell(std::forward<Children>(children)...);
}

} // namespace voidui
