# CSS keyframe animations

Define named stages with `@keyframes` and attach them to a widget with the
`animation` shorthand or its eight longhands:

```css
@keyframes pulse {
    from, to { background-color: #2563eb; }
    50% { background-color: #93c5fd; }
}

.indicator {
    animation: pulse 1.2s ease-in-out infinite;
}
```

Names are case-sensitive. Frame selectors accept `from`, `to`, percentages and
comma-separated offsets. Declarations at equal offsets combine in source order;
a later rule with the same name replaces the earlier named rule. Missing start
or end values use the widget's underlying computed style. Important declarations
inside keyframes are ignored.

## Playback controls

| Property | Initial value | Accepted values |
| --- | --- | --- |
| `animation-name` | `none` | Named keyframes, quoted names, or `none` |
| `animation-duration` | `0s` | Nonnegative time |
| `animation-delay` | `0s` | Time, including negative values |
| `animation-timing-function` | `ease` | The easing functions supported by transitions |
| `animation-iteration-count` | `1` | Nonnegative number, including fractions, or `infinite` |
| `animation-direction` | `normal` | `normal`, `reverse`, `alternate`, `alternate-reverse` |
| `animation-fill-mode` | `none` | `none`, `forwards`, `backwards`, `both` |
| `animation-play-state` | `running` | `running`, `paused` |

The first time in the shorthand is the duration; the second is the delay.
Each comma-separated entry resets omitted controls to their initial values.
Shorter longhand lists repeat to match the name list. CSS-wide keywords reset or
inherit all shorthand controls together.

Use `backwards` to apply the starting sample during a positive delay and
`forwards` to retain the final sample after completion. Direction and fractional
iteration counts determine those samples. A negative delay starts playback partway
through its timeline. Pausing preserves elapsed time; resuming continues from it.
Paused and completed animations do not request presentation frames.

Removing a name cancels that animation. Reordering names, changing timing controls
or replacing a stylesheet preserves elapsed time for matching names. Repeated
names match from right to left. Hiding a widget with `display: none` cancels its
playback; showing it again starts a new timeline.

## Rust styling

Use `StyleBuilder::animation` with `voidui::style::animation::AnimationStyle` to
replace all controls, or call the corresponding `animation_*` setters separately.
Duration and delay values use seconds. An empty name list disables playback.
`AnimationName::from("pulse")` selects a stylesheet's named rule; the typed API
does not create keyframes. The direction, fill and play-state enums are available
in the same module. Set iteration count to `f64::INFINITY` for an infinite loop.

## Cascade and rendering

Later animation entries take precedence when they affect the same property.
Important authored declarations take precedence over keyframe values. An active
transition retains priority over a keyframe effect on the same property until
the transition finishes. A frame-level `animation-timing-function` applies to the
interval beginning at that frame.

Keyframes share the transition engine's supported value types: colors, transforms,
shadows, typography, numeric layout lengths and related numeric properties.
Incompatible supported values change at the interval midpoint. Properties without
a supported value representation do not produce animation tracks. See
[CSS effects](effects.md) for the value types and rendering limitations.

Paint-only changes reuse layout; geometry and typography changes invalidate the
corresponding layout inputs. This native widget subset does not dispatch DOM
animation events or implement animation composition modes or scroll timelines.
