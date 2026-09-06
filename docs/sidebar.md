# General-Purpose Sidebar

`#include "voidui/widgets/sidebar.h"`

`sidebar(panel, content)` manages two viewports: a sidebar and the main content. It fills its parent by default, can be placed in any container, and can be nested to combine multiple directions. Either slot accepts an ordinary control, a container, or a function component. The internal viewports clip automatically; wrap long content in `scrollable(...)`.

```cpp
auto page = component([] {
  auto showing = use_state(true);
  auto panel_size = use_state(260.0f);

  return sidebar(
      column(text("Tool panel"),
             button("Close").on_click([showing] { showing.set(false); })),
      column(button("Toggle sidebar").on_click([showing] {
               showing.set(!showing.get());
             }),
             text("Main content")))
      .placement(SidebarPlacement::Right)
      .open(showing)
      .extent(panel_size)
      .limits(160, 480)
      .drag_mode(SidebarDragMode::ElasticOpenClose)
      .drag_threshold(48)
      .collapse_threshold(40);
});
```

The example also requires `component.h`, `button.h`, and `column.h`. Both `open(State<bool>)` and `extent(State<float>)` are two-way bindings: dragging and keyboard operations write back to the state, while calling `set(...)` from an external button or another component updates the sidebar. `on_open_change` and `on_extent_change` add listeners without replacing state bindings. They fire only when interaction changes the corresponding value; external declarative updates do not fire them in return.

## Direction and Layout

| API | Behavior |
| --- | --- |
| `placement(Left / Right / Top / Bottom)` | Defaults to Left. Vertical sidebars resize their width; top and bottom sidebars resize their height. |
| `mode(SidebarMode::Docked)` | The default. Occupies layout space, so the main content adjusts with its size. |
| `mode(SidebarMode::Overlay)` | Keeps the main content at its original size and draws the sidebar over it; does not create a modal backdrop. |
| `extent(float)` | Defaults to 280. The expanded size, excluding the drag handle. |
| `limits(min, max)` | Defaults to 160–560. The lower and upper bounds for the expanded size. |
| `collapsed_extent(float)` | Defaults to 0. Values such as 56 retain a narrow rail; the value cannot exceed the minimum expanded size. |
| `handle_size(float)` | Defaults to 8. The thickness of the drag handle; it remains present while closed so the sidebar can be pulled open again. |
| `size / width / height / margin` | Size and outer margin of the entire component. Adjust the sidebar width or height with `extent`. |

Child component state is retained while the sidebar is closed. `collapsed_extent(0)` excludes hidden content from pointer input and Tab focus. A narrow rail displays a clipped viewport of the same content. When separate icon-only and full-navigation content is needed, switch inside the panel component according to the bound `showing` state. If window space becomes insufficient, the visible size is compressed temporarily and returns to its original value when space is restored, without rewriting the persistent size state.

## Dragging and Keyboard Input

`SidebarDragMode` provides four presets:

- `Immediate` (default): opening, resizing, and closing do not wait for an elastic threshold or produce a resistance curve. Shrinking inward to the minimum expanded size closes the sidebar immediately. After reaching the maximum size, reversing direction follows immediately without accumulating overshoot distance.
- `Elastic`: the effective drag distance must reach `drag_threshold` before opening or resizing starts. Any distance beyond the threshold contributes to continuous resizing. Releasing before the threshold leaves the size unchanged. With `.edge_visible(true)`, the bending edge provides resistance feedback and springs back over approximately 250 ms after release.
- `ElasticOpenClose`: only opening and closing are elastic. Once opened, catching up to the new boundary begins continuous resizing without waiting for a second threshold; reversing direction within the boundary gap also starts shrinking immediately. Pressing and dragging an already open sidebar follows immediately. After shrinking to the minimum size, it enters the elastic closing phase.
- `Disabled`: removes the drag handle and its keyboard focus. The sidebar can still be toggled through state and buttons.

The three phases can also be overridden separately. Each accepts `SidebarDragBehavior::Immediate` or `SidebarDragBehavior::Elastic`:

| API | Controls |
| --- | --- |
| `open_behavior(...)` | Whether a closed sidebar waits for `drag_threshold` and produces elastic feedback. |
| `resize_behavior(...)` | Whether an open sidebar waits for `drag_threshold` before resizing, and whether it provides resistance beyond the maximum size. |
| `collapse_behavior(...)` | Whether, after reaching the minimum size, the sidebar must be dragged through `collapse_threshold` before closing, with resistance feedback. |

Phases without an override use the `drag_mode` preset. Explicit overrides are independent of setter call order. `Disabled` always disables dragging. For example, to use elasticity only while opening and follow the pointer directly otherwise:

```cpp
sidebar(panel, content)
    .drag_mode(SidebarDragMode::Immediate)
    .open_behavior(SidebarDragBehavior::Elastic);
```

To disable elasticity completely, use only `.drag_mode(SidebarDragMode::Immediate)`; the individual thresholds do not need to be set to zero. `edge_visible` still controls only line visibility, not interactive elasticity.

Dragging has two phases: waiting for the threshold and continuous adjustment. Immediate behavior uses a zero threshold. When a closed sidebar is dragged in the opening direction to the threshold, elastic opening uses the configured initial `extent` and ignores drag memory, while immediate opening uses the remembered size. Remaining pointer movement from that event does not enlarge the panel further. Opening or closing jumps the boundary to a new position, so the boundary gap is reevaluated after every jump, independently in both directions:

- For a left sidebar, if the pointer is left of the new boundary, rightward movement first catches up to the boundary. Only distance beyond it counts toward the right-drag threshold. Reversing leftward partway through can measure the left-drag threshold from the latest turning point without returning to the original press position.
- If the pointer is right of the new boundary, the behavior is symmetric: leftward movement first catches up to the boundary, while rightward movement can measure a new threshold from the latest turning point. Crossing the gap to the boundary does not change the size or produce elastic feedback.
- After the threshold in the current direction is reached, continuous adjustment begins and the size changes smoothly from the panel's current size. Reversing direction then follows immediately without waiting for the threshold again. Only another open/close jump resets the waiting phase.

For example, with `Elastic`, a closed left sidebar dragged from 0 with a configured width of 260 and a threshold of 48 (ignoring the grab offset inside the handle) opens to 260 when the pointer reaches 48. Continuing to 200 leaves it at 260. Only movement beyond the boundary at 260 begins counting toward the resize threshold, so at 328 the width is 280. If the pointer instead reverses left at 200, counting starts from 200; at 132 the width is 240 rather than jumping suddenly to the pointer position.

With the same dimensions and `ElasticOpenClose`, the sidebar opens to 260 at 48, remains 260 through 260, and becomes 280 at 280. If the pointer instead reverses at 200 and moves left to 190, the width immediately becomes 250. Every opening from the closed state uses the opening elasticity once.

During continuous adjustment in the closing direction, an elastic close occurs when the target size reaches `minimum expanded size - collapse_threshold`; without elastic closing it occurs at the minimum expanded size. Both modes remember the expanded size from before this shrink began for reopening through immediate dragging, a button, or the keyboard. Elastic opening always ignores this memory. For example, after `.extent(240)` is dragged to 360 and then closed without releasing, another elastic pull still opens it at 240; the same remains true after releasing and starting a new drag. Writes from two-way-bound interaction and ordinary rebuilds do not change the elastic opening size; only an explicit external change to `extent` updates it. The size remains constrained by `limits` and available space.

`collapse_threshold` defaults to 48. When the edge line and the corresponding elastic phase are enabled, dragging beyond a size boundary displays a bounded resistance curve. All four directions follow the same rules, changing only the coordinate axis and opening direction.

Parent component rebuilds do not reset the current phase, boundary, or turning point. Repeated move or release events at the same position do not undo an opening that just completed. External changes to the open state, size, or drag configuration cancel the current gesture. Losing window focus ends dragging and retains the committed size. Escape restores the open state and size from the start of the press, even if the gesture passed through several open/close transitions.

The handle is focusable with Tab and displays a focus background. Left/Right or Up/Down adjusts it by 16 px per press, depending on orientation. Shrinking outward below the minimum closes it; adjusting inward while closed opens it. Enter or Space toggles the open state. Keyboard operations are not affected by mouse-drag thresholds.

## Styling and State

The edge line is hidden by default in both Debug and Release builds. `.edge_visible(true)` displays both the resting edge line and the elastic curve while dragging; `.edge_visible(false)` hides both. Visibility affects only line painting, not the drag region, thresholds, cursor, or keyboard controls. The keyboard focus indicator remains visible. When hidden, the line's spring-back animation does not request continuous animation frames.

For example, show it only in debug builds:

```cpp
auto view = sidebar(panel, content);
#ifndef NDEBUG
view.edge_visible(true);
#endif
```

The interactive example uses this debug configuration. Use `.handle_color(Brush)` and `.elastic_color(Brush)` to set the enabled edge and drag-curve colors. They can also be set in VSS with `sidebar { handle-color: ...; elastic-color: ...; }`. `.background(Brush)` sets the background of the entire component; backgrounds for the panel and main content can be applied directly to the supplied controls.

The internal viewports expose `sidebar::part(panel)`, `sidebar::part(content)`, and `sidebar::part(handle)`. Apply padding, scrolling, and similar layout styles to the supplied slot content; Sidebar controls the geometry of the slots themselves.

Without bindings, the sidebar stores its own interaction state. Unchanged declarations such as `.open(true)` and `.extent(260)` do not overwrite user drag results during parent rebuilds; they are applied only when the declared value actually changes. Use State bindings when any component must explicitly control the open state or size. Retain the component position during rebuilds, or assign a stable `.key(...)`, to preserve slot content and interaction state.

```sh
xmake run voidui_example_sidebar
xmake run voidui_sidebar_selftest
```

The example switches among all four directions, docked and overlay modes, immediate dragging, elastic open/close only, and fully elastic dragging. It also demonstrates retaining a narrow rail when closed and toggling the sidebar with buttons both inside and outside it.
