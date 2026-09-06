# Select

`#include "voidui/widgets/select.h"`

A single-value drop-down selector. An option's `value` is its stable identifier, `label` is its display text, and the third field, `disabled`, defaults to `false`. Values should be unique; if a value is duplicated, the first matching option is displayed. When no value is set or no option matches, the placeholder is shown. The first option is not selected automatically.

```cpp
select({{"cn", "China"}, {"us", "United States"}, {"eu", "Europe", true}})
    .placeholder("Select a region")
    .value("cn")
    .width(280)
    .picker_max_height(240)
    .on_change([](const std::string &value) { /* Save the value */ });
```

A `State<std::string>` can be bound inside a function component:

```cpp
return component([] {
  auto region = use_state(std::string("cn"));
  return select({{"cn", "China"}, {"us", "United States"}}, region);
  // Equivalent to select({...}).value(region)
});
```

`.disabled(true)` disables the entire control and removes it from the Tab order. `on_change` fires exactly once when the user commits a different value. A new declaration without an explicit value retains the previous user selection; an explicit `.value(...)` adopts the newly declared value when the component rebuilds. Rebuilding with the same option list preserves the open state. Changing the option list or disabling the control closes it. Select instances passed as lvalues and then cloned each own independent interaction state.

## Interaction and Overlay

- Click, Space, Enter, and Up/Down open the picker. Once open, Up/Down browses options and Home/End jumps to the first or last enabled option. Browsing does not change the selected value.
- Enter, Space, or clicking an option commits it. The selected option displays a checkmark, the browsed option receives background feedback, and focus remains on the trigger after closing.
- Escape, Alt+Up, an outside click, Tab, or loss of window focus closes the picker. Escape cancels browsing, while Tab continues to the next control. Outside clicks use the overlay system's click-through prevention.
- ASCII letters and digits support prefix search. Input within 700 ms forms a prefix, and repeating the first character cycles through matches. When closed, a match is selected immediately; when open, only the browsed option moves. IME search, multiple selection, and optgroups are not currently supported.
- The default width is 240 px and the default maximum height is 280 px. The default theme includes a focus ring, disabled state, rounded corners, shadow, and an arrow that flips vertically. Long labels remain on one line and are clipped so they cannot cover the arrow.
- The picker is an internal overlay, painted and hit-tested independently in window coordinates without inheriting Scrollable clipping. The panel matches the trigger width and is constrained by the window width. It prefers opening downward, flips upward when necessary, and limits its height to the available space. Long lists scroll within the panel, and keyboard browsing automatically scrolls the target option into view. Wheel input over the panel is not passed to an outer Scrollable.

## VSS and C++ Styles

The following subset of CSS syntax is supported. See [CSS Forms](https://www.w3.org/TR/css-forms-1/) and [CSS UI](https://www.w3.org/TR/css-ui-4/) for the related standards. These are the semantics of a custom-painted VoidUI control, not an implementation of the browser's complete form styling specification.

```css
select {
  appearance: base-select;
  width: 280px;
  padding: 10px 12px;
  background: white;
  color: #263244;
  border-width: 1px;
  border-color: #cbd5e1;
  border-radius: 8px;
  font-size: 14px;
}
select:focus, select:open { border-color: #2563eb; }
select:disabled { color: #94a3b8; cursor: not-allowed; }
select::picker(select) { background: white; border-radius: 10px; }
select:open::picker-icon { color: #2563eb; }
select option:checked { background: #eff6ff; }
select option:focus { background: #dbeafe; }
select option:disabled { color: #94a3b8; }
select option:checked::checkmark { color: #2563eb; }
```

`:enabled` is also available for select and option. On an option, `:focus` means the item currently browsed by keyboard or pointer. System focus remains on Select, and options do not add Tab stops. A pseudo-class before a pseudo-element tests the host; one after it tests that part. For example: `select:open::picker-icon` and `select::picker(select):hover`.

`::picker(select)`, `::picker-icon`, and `::checkmark` reuse the named parts `::part(picker)`, `::part(picker-icon)`, and `::part(checkmark)`, respectively. `select::part(label)` styles the trigger text. Option is a public style node, so `.my-select option` can limit its scope; its internal text and graphics remain exposed through parts.

`appearance` supports `auto`, `base-select`, and `none`. The first two use VoidUI's default appearance, while `none` hides the default arrow. It does not enable a native system control or clear explicit backgrounds, borders, or other styles. CSS `content` replacement, anchor-positioning properties, `:not()`, and other form pseudo-classes are not implemented. `picker_max_height()` is component configuration, not a standard CSS property. VSS can set the panel height with `select::picker(select) { height: 220px; }`, but it is still constrained by the configured maximum height and available window space.

The same cascade supports fluent C++ assignments, with inline settings taking precedence over application VSS:

```cpp
auto field = select({{"cn", "China"}, {"us", "United States"}})
    .appearance(SelectAppearance::BaseSelect)
    .background(Color(255, 255, 255))
    .color(Color(38, 50, 68))
    .font_size(14)
    .padding({10, 12})
    .border_radius(8)
    .border_width(1)
    .border_color(Color(203, 213, 225));

auto sheet = std::make_shared<StyleSheet>();
sheet->add(Selectors::of<Select>().open().picker_icon(),
           StyleDeclaration{}.set<styles::Foreground>(Color(37, 99, 235)));
sheet->add(Select::option_selector().checked().checkmark(),
           StyleDeclaration{}.set<styles::Foreground>(Color(13, 148, 136)));
```

C++ selectors provide `.open()`, `.checked()`, `.disabled()`, `.enabled()`, `.picker()`, `.picker_icon()`, and `.checkmark()`. They can also be combined with `.hovered()`, `.focused()`, and `.part(...)`.

## Running

```sh
xmake run voidui_example_select
xmake run voidui_select_selftest
```

The example loads `examples/select.vss` from the project root and demonstrates state binding, long lists, a scrolling viewport, disabled options, and a custom theme.
