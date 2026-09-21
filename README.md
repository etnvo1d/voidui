# voidui

A Rust UI library with Taffy layout, retained div/text widgets, and a Winit/WGPU
desktop runtime. Static windows sleep between changes instead of drawing continuously.

```sh
cargo run
cargo run --example hello
cargo run --example effects
cargo run --example tailwind
cargo run --example media
cargo run --example overlays
cargo run --example selection
cargo run --example input
cargo run --example state
cargo run --example async_tasks -- README.md
cargo run --example resources -- README.md
cargo run --example events
cargo run --example scrolling
cargo run --example spatial
cargo test --workspace
```

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

macOS rendering is tested locally. Windows and Linux cross-compilation is checked;
platform-specific runtime validation is still required. See the window documentation
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
