# Composing reusable components

Use ordinary Rust functions and builder methods for configuration, content, and
scoped interaction. No UI template macro is required. Existing positional component
calls continue to work unchanged.

## Named properties

Keep required inputs positional. Add `#[prop(default)]` or
`#[prop(default = expression)]` to an optional input to generate a named setter.
The annotated parameter is omitted from the constructor's argument list.

```rust
use voidui::{component, div, text, Callback, Children, Read};

#[component]
fn panel(
    title: Read<String>,
    #[prop(default = 16.0)] padding: f32,
    #[prop(default)] on_open: Callback<()>,
    children: Children,
) {
    div()
        .padding(padding)
        .child(text(title.to_string()))
        .on_click(move || on_open.call(()))
        .children(children)
}

let view = panel("Files")
    .padding(24.0)
    .on_open(|()| println!("Opened"))
    .class("panel")
    .child(|| text("Project files"));
```

Return types can be omitted on `#[component]` functions. The generated builder
implements `IntoElement`, so `.build()` is optional. Use `.build()` when an API
explicitly requires `ComponentElement`. Property setters, `.key()`, `.id()`,
`.class()`, `.add_class()`, and all event methods preserve the builder type and can
be interleaved. Existing wrapper modifiers still apply to the rendered root.
Property names must not collide with these builder methods.

## Root styles

Component invocations expose the same inherent style methods as `div()` and
`text()`. This includes typed setters, shorthand aliases, and Tailwind presets;
no styling trait import is needed. Named inputs, children, events, and styles
can be interleaved without finishing the builder first:

```rust
use voidui::{Children, component, div, text};

#[component]
fn panel(#[prop(default = 8)] padding: i32, children: Children) {
    div().padding(padding).children(children)
}

let view = panel().w_full().padding(12).bg_gray_50()
    .child(|| text("Content"))
    .p_4().border_r_1().cursor_col_resize();
```

Styles apply to the final rendered widget root, including through transparent
components. They add no CSS/layout node, do not execute the component body early,
and do not change component keys or hook ownership. The same API is available
on positional components and `component(|| ...)` closures.

Each outer invocation overrides only its explicitly styled properties on the
returned root. Unset properties keep the component's own styles. Outer overrides
win over inner component overrides on a shared root. Shorthands and subsequent
longhands follow call order within each invocation. Ordinary stylesheets remain
below inline styles, and stylesheet `!important` declarations remain above them.

Changing or removing an invocation's styles invalidates its memo comparison,
even when its inputs compare equal. State survives matching updates. Removing an
override restores the newly rendered root's own value. Inner state updates and
root-widget replacement retain the invocation's current overrides. Cloned
descriptions have independent style overrides.

An optional component input keeps its setter when its name matches a style
method. In the example above, `.padding(12)` sets the component input, while
`.p_4()` sets the root override. Use `.build()` to select the root style method
explicitly after configuring the component's inputs and children:

```rust
use voidui::{component, div};
#[component]
fn panel(#[prop(default = 8)] padding: i32) { div().padding(padding) }

let view = panel().padding(12).build().padding(20);
// The body receives 12; its rendered root uses 20px padding.
```

The existing `.child(...)` contract is unchanged: use a repeatable closure for
raw widget builders, or pass a component or string directly. Only components
declaring a `Children` input expose child-configuration methods.

## Default inputs

Defaults are evaluated once per constructor call, in declaration order, before
setters run. They may use earlier inputs. A `state(|| initial_value)` initializer
still runs only on mount: changing a `default_open` property does not reset existing
state. Plain properties use their declared Rust type, including numeric inference.
`Read`, `List`, `Callback`, and `AsyncCallback` retain their automatic conversions
at constructors and setters. Type aliases keep ordinary Rust signatures.
`Callback::default()` is an allocation-free, comparable no-op.

Generics, const generics, and `#[component(memo)]` work with named properties.
Memo compares the final normalized inputs, independent of setter order. Required
inputs stay explicit: a named setter is generated only for a defaulted property.

## Reusable children

Declare one `Children` parameter to receive content. It defaults to an empty list
and generates `.child(...)` and `.children(...)` builder methods. It can have any
parameter name, but must be spelled `Children` rather than hidden behind an alias.
A component decides where to place content, for example `div().children(children)`.

```rust
use voidui::{component, div, text, Children};

#[component]
fn card(children: Children) {
    div().class("card").children(children)
}
#[component]
fn heading() { text("Files") }

let view = card()
    .child(heading())
    .child("Select a file")
    .child(|| div().class("details").child("No file selected"));
```

Pass components and text directly. For raw widget trees, supply a repeatable
closure, such as `.child(move || div().child(...))`. Unlike a mounted Widget,
a component description can be reused to construct fresh output. A closure child
runs inside its own component boundary when mounted, with its own hook scope and
access to the receiving container's Context. Neither creating the content list nor
cloning it executes its children. Existing `div().child(widget)` calls are unchanged.

```compile_fail
use voidui::{component, div, Children};
#[component]
fn card(children: Children) { div().children(children) }
let view = card().child(div()); // Use .child(|| div()) for a raw widget tree.
```

`Children::default()` allocates nothing. Nonempty `Children` clones share an immutable list; adding content uses copy-on-write.
`Children::iter()` and iteration over `Children` yield reusable component descriptions.
Use `.key(...)` on component children when reordering a list. Matching children keep
their state through container rerenders. Removed children unmount normally; placing
that content again creates fresh state. Reusing a description in two positions or
trees creates independent hook storage. Captured `Rc` models, callbacks, and mutable
closure captures remain shared: use `state()` for per-placement mutable data.

A transparent component can return `children.single()`. This requires exactly one
child and adds no layout node. Group multiple siblings in a `div` explicitly; an
empty or multi-child list produces an explanatory panic. Containers that accept
arbitrary lists should use `.children(children)` on their layout root.

## Scoped interaction

Publish a typed controller from an ancestor and resolve it in descendant components.
Intermediate components do not need forwarding parameters.

```rust
use voidui::{component, div, state, text, Children, State,
             provide_context, use_context};

#[derive(Clone, Copy, PartialEq)]
struct SidebarController { open: State<bool> }
impl SidebarController {
    /// Toggle visibility from an event handler.
    fn toggle(self) { self.open.update(|open| *open = !*open); }
}

#[component]
fn sidebar_scope(#[prop(default = true)] default_open: bool, children: Children) {
    let open = state(|| default_open);
    provide_context(SidebarController { open });
    children.single()
}

#[component]
fn iconbar() {
    let sidebar = use_context::<SidebarController>();
    div().tag("button")
        .child("Toggle sidebar")
        .on_click(move || sidebar.toggle())
}

#[component]
fn sidebar() {
    let sidebar = use_context::<SidebarController>();
    div().when(sidebar.open.get(), |view| view.child(text("Files")))
}

#[component]
fn workspace() {
    sidebar_scope().default_open(true).child(|| {
        div().child(iconbar()).child(sidebar())
    })
}
```

`provide_context(value)` requires `Clone + PartialEq + 'static`. Use small domain
controllers containing `State` handles, `Read` snapshots, and `Callback` handles.
Their equality compares identity without reading reactive values. Independently
updated fields should remain separate `State` values. A context value itself is
not an additional state store.

`use_context::<T>()` returns the nearest ancestor's value and reports the missing
Rust type if no provider exists. `try_context::<T>()` returns `Option<T>` instead.
Lookup excludes the current component's own provider. Nested scopes shadow outer
ones, siblings and separate trees are isolated, and no global singleton is created.
The ancestry is logical component ancestry, including transparent wrappers, rather
than the CSS or physical widget tree.

Call these APIs during component rendering, not in events or state initializers.
Capture the returned controller in events. Context access consumes no ordered hook
slot, so conditional lookups and providers are valid. Publish at most once per type
per render; omitting a previously published type removes that provider. Returning
an equal value does not invalidate consumers. Adding, replacing, or removing a
provider reconnects consumers even below memoized intermediates. Missing lookups
are tracked so a newly available provider is observed.

Obtaining a controller tracks only its provider binding. Reading one of its State
handles separately tracks that value. A trigger that only calls `toggle()` does
not rerender on every open/close. Context invalidations discovered during a flush
finish before that flush returns; ordinary writes from unmount destructors retain
the next-batch semantics described in the state guide.

For explicit targeting, declare an optional controller property and resolve it
with `controller.unwrap_or_else(use_context)`. This only looks up context when no
controller was supplied. A controller contains weak State handles: keep its owning
scope mounted for as long as other components need to use it.

## Conditional configuration

All widget builders support `.when(condition, |view| ...)`. The closure executes
only when the condition is true and must return the same builder type. This is
useful for conditional classes, styles, and children without mutable temporary
builders. Removing a child unmounts it; use a visibility/display style when its
state must remain mounted.

## Verification

Run `cargo run --example composition` for a native sidebar demo, or add `--smoke`
to verify closing and reopening it automatically. `tests/component_api.rs` covers
named inputs, content replay, keys, memoization, Context shadowing/replacement,
independent trees, and diagnostics.

### Editor support

Run `python3 scripts/check-style-editor.py` with `rust-analyzer` on PATH to check
completion, hover documentation, and go-to-definition through the actual LSP
server. This includes ordinary widgets, closure components, named component
builders, parameter/style name collisions, and calls after a style setter. The
fixture renames the dependency to `ui` and imports no styling trait.

The check writes its fixture, server log, and results under `target/style-editor`.
Use `--rust-analyzer /path/to/rust-analyzer` to select a server, or `--timeout 300`
for a slow first workspace load. It does not modify editor settings. Keep this
check alongside `cargo test --workspace`: successful compilation alone does not
verify editor name resolution through generated APIs.

Invalid declarations are diagnosed at the parameter:

```compile_fail
use voidui::{component, div};
#[component]
fn invalid(#[prop(default)] id: String) { div() }
// `id` is reserved for the component root modifier.
```

```compile_fail
use voidui::{component, div, Children};
#[component]
fn invalid(left: Children, right: Children) { div() }
// A component accepts one content list; use separate components for named regions.
```

```compile_fail
use voidui::{component, div};
#[component]
fn invalid(#[prop(unknown)] count: usize) { div() }
```

### Retained cost

The logical Context ancestry adds a retained scope per component; its provider map
is allocated only when used. This does not add widget/layout nodes or polling.
On a macOS/aarch64 release run on 2026-09-14 (`state_bench 10000 50000`), the
existing one-usize-state benchmark retained about 531 bytes per component versus
the earlier 459-byte measurement in the state guide. Warmed local updates still
allocated zero bytes, rendered one component, and took about 0.40 microseconds;
keyed store updates took about 0.33 microseconds with zero warmed allocations.
These figures exclude GPU/layout work and are machine-dependent.
