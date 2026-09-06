# Positioning, Stacking, and Tooltips

The framework uses C++23. VSS and C++ settings write to the same set of style properties. C++ instance settings take precedence over style sheets, and cloned components retain these settings.

## Common APIs

```cpp
auto content = column(text("Save the current document"))
    .position(Position::Absolute)
    .left(Inset::percentage(50))
    .bottom(Inset::calc(100, 10)) // calc(100% + 10px)
    .z_index(1000)
    .white_space(WhiteSpace::Nowrap)
    .visibility(Visibility::Hidden)
    .pointer_events(PointerEvents::None)
    .opacity(0.0f);

VisualTransform offset;
offset.translate_x_percent = -50;
offset.translate_y = 4;
content.transform(offset);
```

These methods are defined on the `Widget` base class. They use C++23 explicit object parameters to preserve the derived component type and its lvalue/rvalue category. Component authors do not need to duplicate the positioning setters.

`left/right/top/bottom` accept an `Inset`. A number represents logical pixels; `Inset{}` or `Inset::Auto{}` represents `auto`. `z_index(ZIndex{})` restores `auto`, which differs from `z_index(0)`. `inset(Spacing<Inset>(...))` sets all four sides at once, using the C++ `Spacing` argument order: left, top, right, bottom. The VSS `inset` shorthand uses the CSS order: top, right, bottom, left.

Transitions use the existing types:

```cpp
content.transition_property(TransitionPropertyList::make({styles::Opacity::index()}))
       .transition_duration(StyleTimeList::make({0.15f}))
       .transition_timing_function(EasingList::make({Easing::ease()}));
```

See [`examples/tooltip.cpp`](../examples/tooltip.cpp) for a complete hover style and window example. The Tooltip background and padding are applied to the Column container, while Text handles the text itself.

## Layout and Stacking Semantics

| Property | Behavior |
| --- | --- |
| `position: static` | Normal layout; side offsets are ignored. |
| `relative` | Retains its original layout space, then offsets itself and its subtree. |
| `absolute` | Removed from normal layout and positioned relative to the padding box of the nearest positioned or transformed ancestor; otherwise, the viewport is used. |
| `fixed` | Removed from normal layout and positioned relative to the viewport by default; scrolling does not change its position. A transformed ancestor establishes a new containing block. |
| `sticky` | Retains its normal layout space and sticks to specified edges inside the nearest clipped scroll container, constrained by its parent bounds. |
| `left/right/top/bottom` | Accept `auto`, `px`, `%`, unitless zero, and additive or subtractive `calc()` expressions using these units, including nested parentheses. |
| `inset` | CSS shorthand for one to four positioning values; later declarations for individual sides override them independently. |
| `z-index` | `auto` or a signed 32-bit integer. Items at the same level retain declaration order. Positioned components and direct layout children of Row/Column may set their stacking level. |
| `visibility` | `hidden/collapse` retains layout but does not paint or receive pointer hits; descendants can explicitly restore `visible`. |
| `pointer-events` | `none` prevents the widget from becoming a pointer target; descendants can explicitly restore `auto`. Normal event bubbling is not blocked. |
| `white-space` | `nowrap/pre` disables automatic wrapping; `normal/pre-wrap` retains the existing automatic wrapping behavior. |

Absolute and fixed children do not contribute to container size, gaps, or Fill/Flex allocation. Setting both opposing sides stretches the child when the corresponding size is `auto`. Percentage insets refer to the containing block, while percentage translations refer to the widget's own border box.

An explicit integer z-index, fixed or sticky positioning, a non-`none` transform, or opacity below 1 creates a stacking context. The context participates in outer sorting as a unit, so a high z-index inside it cannot escape the outer context. Ordinary ancestors and `position: relative; z-index: auto` do not trap high-level descendants inside their subtrees. Painting and hit testing share the same order; hit testing proceeds in reverse paint order.

`visibility` transitions use CSS's special interpolation between visible and hidden: content is visible throughout a fade-in and becomes hidden only when a fade-out finishes. `opacity: 0` alone does not disable hit testing; combine it with `visibility` or `pointer-events` when needed.

## Implementation and Boundaries

- `widget_positioning.cpp` handles post-layout positioning, `widget_paint.cpp` handles stacking, painting, and hit testing, and `widget_geometry.h` centralizes transform and containing-block logic.
- Row and Column share `linear_layout.cpp`. Positioned children are excluded from the normal layout interface by `LayoutContext`; components with named slots can use `flow_index()` to map registration indices.
- Sorting is cached on the tree without changing declared child order. Painting reuses ancestor state and path buffers. A `LayoutContext` that does not use out-of-flow positioning allocates no filtered list. `Inset` consists of two floats, and `VisualTransform` remains within `PropertyValue`'s 32-byte inline storage.
- Normal paint invalidation first compares a snapshot of stacking properties. Color and opacity animations reuse the sort order when stacking relationships do not change. Creating or removing a transform recomputes containing blocks, while changing the translation of an existing transform still invalidates painting only.
- This is a native UI layout system, not a complete browser CSS engine. It does not currently provide DOM inline formatting, RTL or vertical layout, CSS whitespace collapsing, every CSS length unit, or multiplication and division in `calc()`. When all insets are `auto`, the start of the parent's content area is used as the fallback position.
- `translate`/`translateX`/`translateY`, `scale`/`scaleX`/`scaleY`, `rotate`, and combinations expressible through translation, rotation, and scaling are supported and composed in CSS order. Combinations requiring skew or cross-axis percentage coefficients report a parse error. 3D transforms are not supported.
- Opacity uses alpha modulation on native paint commands. Browser-style offscreen group-opacity compositing for overlapping descendants is not yet provided. In this framework, `visibility: collapse` behaves the same as `hidden`.

Semantic references: [CSS Positioned Layout](https://www.w3.org/TR/css-position-3/) and [CSS painting order](https://www.w3.org/TR/CSS22/zindex.html).

## Verification

```powershell
xmake f -m debug -y
xmake
xmake test -v
```

`positioning_selftest` covers the complete tooltip VSS, transitions, percentages, nested stacking, negative levels, hit testing, clipping, fixed/sticky scrolling, transparent components, and input slots. Other self-tests cover the existing layout, style, animation, input, resource, and painting paths.
