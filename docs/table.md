# Table

`#include "voidui/widgets/table.h"`

A table consists of the same named parts as a CSS table. Each part is an independent widget type, so `th`, `td`, and `tr` are selectors rather than conventions:

```
table > caption | colgroup > col | thead | tbody | tfoot > tr > td | th
```

```cpp
table(thead(tr(th("Package"), th("Owner"), th("Coverage"))),
      tbody(tr(td("voidui-core"), td("Ada"), td("98%")),
            tr(td("style-resolver"), td("Lin"), td("94%"))))
    .border_collapse(BorderCollapse::Collapse)
    .width(Length::Fill{})
    .add_class("releases");
```

For data-driven tables, use the fluent builder. Strings are wrapped in `td` automatically, and other widgets can be supplied directly:

```cpp
auto grid = table().headers({"Key", "Value"}).table_layout(TableLayout::Fixed);
for (const auto &[key, value] : rows)
  grid.row(key, value);                       // Append to the implicit tbody
grid.row("Icon", svg("res://check.svg"));    // A non-td widget is wrapped in td
```

`tr_of("Alice", "30")` follows the same rules and produces a row directly. Run `xmake run voidui_example_table` to view the example.

## Layout

A table is laid out as one unit. Column widths cannot be decided by rows, and row heights cannot be decided by cells independently. `Table::layout` therefore measures and places the entire grid through `LayoutContext::constrain_node`, then supplies the resulting geometry to sections, rows, and cells. They remain ordinary nodes: they participate in stacking, hit testing, and `:hover` just like any other widget.

- **Column widths (`table-layout: auto`)**: each cell's maximum-content width is measured first. A second minimum-content measurement is performed only if the total does not fit. Spanning cells are handled in ascending span order and add only the amount that their covered columns cannot already provide. A fixed `width` on a `<col>` or cell competes as that column's requested size. Columns with `width: fill` or `flex` absorb extra space first.
- **Column widths (`table-layout: fixed`)**: content is not measured at all. Widths come from `<col>` elements first, then from cells in the first row. Remaining space is divided equally among columns without a width.
- **Row heights**: each row uses the maximum height of its cells, with a fixed `height` on the row as a lower bound. A row-spanning cell stretches its covered rows only when their combined height is insufficient, distributing the extra amount in proportion to their existing heights. When the table itself has a definite height, extra space is likewise distributed across rows. This lets `height: fill` expand the table body instead of leaving blank space.
- **Section order**: regardless of source order, `thead` appears first, `tbody` sections in the middle, and `tfoot` last, matching CSS. A `tr` written directly under `table` is treated as part of an anonymous `tbody`.
- **Caption**: placed above or below the grid according to `caption-side`, with the same width as the grid.
- A cell stacks its children vertically, then aligns the group as a whole: horizontally according to inherited `text-align`, and vertically according to its own `vertical-align`. When content is narrower than the cell, it is translated rather than stretched, so `text-align: right` moves a button just as it moves a paragraph.

## Border Model

With `border-collapse: separate` (the default), every cell owns its border and cells are separated by `border-spacing`. Row and section backgrounds show through the gaps.

With `border-collapse: collapse`, adjacent borders merge into shared grid lines owned by the table:

- Each line uses the maximum `border-width` among its adjacent candidates and the color of the winner. Cells, rows, sections, `<col>` elements, and the table participate in the conflict in that order.
- Line width occupies layout space, and cells fit strictly between the lines, so a cell's `border-width` is no longer included in its own chrome.
- Lines are painted in `draw_foreground`, after all child nodes. They do not cover anything because layout already reserved their bands.
- The table's own padding and border yield to the outermost grid lines, as required by CSS. `border-radius` does not affect the grid lines in this mode, matching browser behavior.

## VSS and C++ Styles

All table-related CSS properties use the same property registry and declaration macros as `background`, so VSS and fluent calls set the same values.

| Property | Values | Inherited | Read by |
| --- | --- | --- | --- |
| `table-layout` | `auto` \| `fixed` | No | `table` |
| `border-collapse` | `separate` \| `collapse` | Yes | `table` |
| `border-spacing` | `<h>` or `<h> <v>` | Yes | `table` |
| `caption-side` | `top` \| `bottom` | Yes | `caption` (inherited from `table`) |
| `empty-cells` | `show` \| `hide` | Yes | `td` / `th` |
| `vertical-align` | `baseline` \| `top` \| `middle` \| `bottom` | No | `td` / `th` |
| `text-align` | `left` \| `center` \| `right` | Yes | Any widget |

```css
table  { border-collapse: collapse; table-layout: fixed; border-radius: 12px; }
th, td { padding: 10px 14px; border-width: 1px; border-color: #e7ecf3; }
thead th { background: #f1f5f9; text-align: left; font-weight: 600; }
tbody tr:nth-child(even) { background: #fafbfd; }
tbody tr:hover           { background: #eff6ff; }
tbody tr:last-child td   { border-color: #d8e0eb; }
.numeric { text-align: right; }
caption  { caption-side: bottom; color: #64748b; font-size: 12px; }
```

Fluent calls write to the same inline style layer and correspond directly to the properties above:

```cpp
table(...)
    .table_layout(TableLayout::Fixed)
    .border_collapse(BorderCollapse::Collapse)
    .border_spacing(BorderSpacing(8.0f, 6.0f))
    .caption_side(CaptionSide::Bottom)
    .empty_cells(EmptyCells::Hide)
    .border(Border::solid(1.0f, Color(0xd8, 0xe0, 0xeb)))
    .padding({10.0f});

td("Total").colspan(3).vertical_align(VerticalAlign::Middle).text_align(TextAlign::Right);
th("Description").rowspan(2);
col().width(Length::Fill{}).background(Color(250, 251, 253));
```

### `vertical-align: baseline`

`baseline` aligns the first-line baselines of cells in the same row, so cells with different font sizes do not each align to the top. It is the CSS initial value and the default for `td` and `th` in this library. The implementation uses `Widget::first_baseline()`: a cell asks its content for the first-line baseline, the row selects the lowest one, and the remaining cells are shifted downward. Content that cannot provide a baseline, such as content with no text, is treated as `top`.

### Structural Pseudo-classes

Tables add `:nth-child()`, `:nth-last-child()`, `:first-child`, `:last-child`, and `:only-child`. The syntax implements the complete `An+B` form, including `odd` and `even`:

```css
tbody tr:nth-child(even)    { background: #fafbfd; }
tbody tr:nth-child(3n+1)    { border-width: 1px; }
tbody tr:last-child td      { border-width: 0px; }
tbody td:nth-last-child(2)  { text-align: right; }
```

The corresponding C++ APIs are `SelectorBuilder::nth_child(a, b)`, `nth_last_child(a, b)`, `first_child()`, `last_child()`, and `only_child()`. `Table::striped_row_selector()` is a ready-made form of `tr:nth-child(2n)`.

The widget tree maintains indices when its structure changes and counts only siblings in the light tree. Components are transparent and pass their index to the root they render, so a row implemented as a function component still counts as one row for `:nth-child()`. A component's internal children have index 0 and cannot match any structural pseudo-class; `::part()` is the mechanism for crossing that boundary.

## Additional Notes

- `col` and `colgroup` paint nothing except backgrounds and borders. The table places them over the columns they describe, above the table background and below row backgrounds, matching CSS paint order. They default to `pointer-events: none` and do not intercept cell clicks.
- `empty-cells: hide` applies only to cells with no child nodes. A cell containing a Text widget with an empty string is not considered empty. As required by CSS, the property has no effect when borders are collapsed.
- Rows do not need to fill every column; missing cells remain empty. Later rows automatically skip positions occupied by row-spanning cells.
- The table itself does not scroll. Wrap it in `scrollable(...)` when necessary. Table headers are not sticky.
- When used outside a Table, `TableRow` and `TableSection` fall back to ordinary horizontal and vertical stacks rather than disappearing.
