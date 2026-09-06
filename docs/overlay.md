# Overlays, Modals, and Tooltips

## Application Developers

```cpp
#include "voidui/core/component.h"
#include "voidui/widgets/button.h"
#include "voidui/widgets/column.h"
#include "voidui/widgets/modal.h"
#include "voidui/widgets/tooltip.h"

using namespace voidui;

auto view = component([] {
  auto showing = use_state(false);
  return column(
      button("Open").on_click([showing] { showing.set(true); }),
      modal(column(
          tooltip(button("Help"), "This tooltip belongs to the current dialog"),
          button("Close").on_click([showing] { showing.set(false); })))
          .open(showing)
          .width(420)
          .close_on_outside_press(true));
});
```

A `Modal` is closed and centered by default, has a backdrop, and closes on Escape. By default, clicking the backdrop only blocks input. `.open(State<bool>)` binds both the visible value and the close callback: when Escape, an outside click, or a parent overlay closes the modal, the state is automatically set to false. A state-controlled close button can call `set(false)` directly. A Modal may also be declared only while it is open; removing the declaration automatically restores focus and cleans up the overlay.

Modals, Overlays, and Tooltips can be declared inside a Modal without configuring a z-index. Every dialog in `examples/modal.cpp` can open another layer, with no fixed nesting limit.

Tooltip usage:

```cpp
tooltip(button("Save"), "Save the current document")
    .placement(OverlayPlacement::Top)
    .delay(std::chrono::milliseconds{500})
    .max_width(240);
```

By default, a tooltip appears after a 500 ms hover delay and immediately when its trigger receives focus. The bubble does not accept pointer input. Leaving the trigger area, losing window focus, scrolling, pressing the mouse, or pressing Escape closes it. If focus remains within the trigger, the tooltip can stay visible after the pointer leaves. After an explicit dismissal, the current trigger state must end and begin again before it can reopen; a new focus activation can also show it. An empty string does not create a bubble.

Automatic focus restoration after a Modal closes does not trigger a tooltip. Hovering again still uses the delay, and moving the pointer away hides it. Focusing it again with Tab restores normal focus-tooltip behavior.

```css
modal { padding: 24px; border-radius: 12px; background: white; }
tooltip::part(bubble) { background: #1d4ed8; color: white; }
```

Configure the Modal backdrop with `.backdrop(Color(0, 0, 0, 96))`.

## Component Authors

`Overlay` is the non-modal entry point to the same mechanism. It is open and interactive by default and anchors to the nearest non-transparent parent node. Place it next to a button or register it through `take_internal_child`; ordinary parent layouts automatically exclude the Overlay. A function component may return an Overlay directly.

```cpp
auto anchor = column(
    button("Menu"),
    overlay(column(button("First item"), button("Second item")))
        .open(menu_state)
        .placement(OverlayPlacement::Bottom)
        .close_on_escape(true)
        .close_on_outside_press(true));
```

A standalone Widget can override `overlay_options()` to return its own `OverlayOptions`; it does not need to inherit from Overlay. The tree sends an `OverlayDismissedEvent` directly to the overlay root for reasons including `Escape`, `OutsidePress`, `Press`, `Scroll`, `OwnerClosed`, `ModalOpened`, and `WindowFocusLost`. This is a notification, not a cancellable close request. A Modal that requires explicit user action can disable the Escape and outside-click policies, then control its state through application buttons.

Overlay and Modal can also be bound manually:

```cpp
modal(content)
    .open(showing.get())
    .on_close([showing](OverlayDismissReason reason) {
      showing.set(false);
    });
```

`.on_close(...)` replaces the existing callback, including one installed by `.open(State<bool>)`. When using both, synchronize the state in the custom callback. Programmatically setting the state to false does not notify that layer again, but it sends `OwnerClosed` to attached overlays so child component state stays synchronized. Callbacks are dispatched after the tree completes the close, allowing components to update through State. Removing a component does not invoke callbacks on destroyed components or retain invalid focus and overlay pointers.

An unbound `.open(true)` remains closed after a policy dismisses it. To reopen it, the framework must observe false and then true. After directly changing parameters on a mounted Widget, call the tree's `request_layout()`. Declarative updates through State are recommended.

## Order, Ownership, and Modal Scope

Three concerns are managed separately: the component tree owns widgets and styles, the active overlay stack determines paint order, and the topmost Modal limits the interactive scope.

- Ordinary content is painted first, followed by active overlays in **open order**. Reopening an overlay moves it to the top. If one update opens several layers, they are pushed in declaration traversal order. Component rebuilds, key reordering, and style recalculation do not change the established open order. A z-index on ordinary content cannot cross the overlay boundary.
- Each Modal's backdrop is painted immediately beneath that Modal, covering earlier layers. Closing a nested dialog reveals the preceding layer instead of sharing a single backdrop that always sits below every dialog.
- Content declared inside an overlay automatically belongs to it. If a Modal with no declared overlay parent opens while another Modal is active, it belongs to the active Modal; sibling declarations therefore still close one layer at a time correctly. Ordinary Tooltips and menus should be declared inside their owning Modal and cannot move in front from a background scope.
- Opening a new Modal closes temporary overlays that do not belong to it and cancels pending background Tooltips. If the Modal was already inside a menu, that menu remains as its owner beneath it.
- Clicks and wheel input cannot pass through the topmost Modal backdrop, and keyboard events cannot bubble outside its scope. Interactive attached Overlays can still receive input; the entire subtree of a non-interactive bubble is excluded.
- Escape handles only the topmost dismissible overlay and stops at a Modal that disallows Escape. An outside click closes at most one interactive overlay and consumes both the press and its paired release, preventing the newly exposed background control from being activated by the same action.
- When a Modal opens, it focuses the first focusable control. Tab and Shift+Tab cycle within the current scope. The modal input boundary remains even if there are no focusable descendants. Button supports activation with Enter and Space. Closing a Modal restores the focus held before it opened; if that target was removed or is no longer interactive, focus falls back to the current Modal.
- Closing an owner also closes its attached overlays and releases their hover, text selection, focus, and mouse capture. Removing any layer likewise clears runtime state and pending notifications.

## Positioning and Clipping

Overlays retain their original style inheritance, parts, and component lifecycle. They paint in window logical coordinates without inheriting outer clipping, transforms, or composited opacity. Their own style transforms and opacity still apply. Anchor positioning accounts for ancestor transforms and updates with layout, window size, and paint transforms.

An anchored Overlay prefers the requested direction, flips when the opposite direction would overflow less, and is then constrained to the window. Modal and `Center` placement use the window rectangle and do not disappear when a trigger button scrolls out of view. An ordinary anchored overlay closes when its anchor becomes completely invisible. Visibility checks for rotated clips use their bounding rectangles.

Overlay content is clipped to its own bounds; nested overlays escape that clip again. Use Scrollable inside long menus or dialogs. Window constraints limit the measured size but do not create scrollbars automatically. Menu height is not currently allocated automatically from the remaining space on either side of the anchor.

## Performance and Verification

The implementation creates no extra native windows, offscreen textures, or second ownership tree, and still produces a single DisplayList. Ordinary Widget, Node, and PaintEntry objects gain no per-overlay instance fields. Only overlay nodes have sparse runtime state, while the active stack stores array indices. Active indices are resorted only on push, pop, or structural changes. Painting and hit testing reuse each overlay's cached paint range, so toggling an overlay does not rebuild the complete paint order.

Untriggered or suppressed overlays skip geometry computation, and hidden content is not measured before it first opens. Stable visible overlays reuse layout and translate their content only when placement changes. Delays reuse a one-shot wake-up mechanism; waiting or stable visibility does not request continuous frames. Early cancellation may leave at most one stale timer wake-up. With the defaults, component authors do not need to manage timers, layer numbers, or focus-restoration handles.

`overlay_selftest` verifies clipping, transforms, hit testing, placement, delays, and lifecycle. `modal_selftest` verifies open order, nested input scope, focus cycling and restoration, state binding, backdrop clicks, cancellation of background tooltips, sibling declarations, key reordering, and reference cleanup after removal.

`overlay_allocation_selftest` opens 32 Modal layers simultaneously, warms up, then reuses Painter and DisplayList to generate 100 frames. It verifies zero C++ `operator new` allocations, no repaint requests in the stable state, and no leftover paint commands after closing every layer. The test does not cover allocations inside GPU drivers or allocations during initial layout and opening.
