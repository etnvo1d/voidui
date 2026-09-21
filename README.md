# voidui

A Rust UI library with Taffy layout, retained div/text widgets, and a Winit/WGPU
desktop runtime. Static windows sleep between changes instead of drawing continuously.

```sh
cargo run --example hello
cargo run --example file_tree -- .
cargo run --example input
cargo test --workspace --all-features --locked
```

The feature guides below describe the focused application examples. Headless
performance workloads and their commands live in [benches](benches/README.md).
The duplicate default binary and the separate `layout` and `state` examples have
been removed. Use `hello`, `cargo test --test layout`, and `file_tree` respectively.


- [Function components and state](docs/state.md)
- [Async resources and scoped tasks](docs/tasks.md)
- [Unified events and drag capture](docs/events.md)
- [Native windows and performance](docs/window.md)
- [CSS selectors and optional hot reload](docs/css.md)
- [Stacking, top layer and tooltips](docs/overlays.md)
- [Transitions, color spaces, gradients and shadows](docs/effects.md)
- [Fluent styles and inheritance](docs/style.md)
- [Tailwind-style classes and Rust utility methods](docs/tailwind.md)
- [CSS layout](docs/layout.md)
- [Scrolling and scrollbar placement](docs/scrolling.md)
- [Sticky positioning and 2D transforms](docs/spatial.md)
- [Inputs and reusable editing](docs/input.md)
- [Text selection and highlight CSS](docs/selection.md)
- [Parley text backend and migration](docs/parley.md)
- [Images and inline SVG](docs/media.md)
- [Text widgets](docs/text.md)
- [GPUI-derived rendering backend](crates/voidui_gpui_wgpu/README.md)

CI runs the workspace tests on macOS and checks all targets on Linux and Windows.
Native window, input, and GPU behavior still require platform-specific validation. See the window documentation
for implemented adaptations, source references, tests, and remaining limitations.

## Rich text

Use `rich_text(span("Hello ").child(span("world").bold()))` for one inline
formatting context and `rich_editor(&Editor::from_rich(...))` for editable rich
content. See [Rich text foundations](docs/rich-text.md) and
`cargo run --example rich_text` for transactions, style inheritance, IME, and undo.

The editor supports projection extensions, hidden/folded source, inline image/widget
views, source-backed compound blocks and viewport layout. See
[the editor framework](docs/editor-framework.md) and run
`cargo run --example editor_extensions`.

### Component composition

See [Composing reusable components](docs/composition.md) for named properties,
reusable children, and scoped controllers with ordinary Rust builder methods.
Run `cargo run --example composition` for the sidebar interaction example.
