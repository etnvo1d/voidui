# Function components and state

Use `#[component]` on an ordinary synchronous function. Calls create deferred
component descriptions; the body first runs when the description is mounted.

```rust
use voidui::{component, state, div, text, IntoElement};

#[component]
fn counter(title: impl Into<String>, step: usize) -> impl IntoElement {
    let count = state(|| 0usize);
    div().tag("button")
        .child(text(format!("{}: {}", title.into(), count.get())))
        .on_click(move || count.update(|value| *value += step))
}

let view = div()
    .child(counter("Left", 1).key("left"))
    .child(counter("Right", 10).key("right"));
```

`Application::window` accepts these descriptions directly. Run the recursive file
tree example with `cargo run --example file_tree`. It uses immutable List/Read inputs,
keyed expansion state, and per-row selection subscriptions.

For optional named properties, reusable children, and scoped controllers, see
[Composing reusable components](composition.md).

## Inputs and ownership

Declare large immutable inputs as `Read<T>` or `List<T>`. Callers pass ordinary
values, vectors, arrays, or existing snapshots. Normalization happens once when
creating the description; rendering clones only the handles. T need not be Clone.
Callbacks accept ordinary closures without Callback::new at the call site.

```rust
use voidui::{component, Read, List, Callback, div, text, IntoElement};
struct Record { title: String }
#[component]
fn record(value: Read<Record>, on_open: Callback<()>) -> impl IntoElement {
    div().child(text(value.title.clone())).on_click(move || on_open.call(()))
}
#[component]
fn records(values: List<Record>) -> impl IntoElement {
    div().children(values.iter().map(|value| record(value, |()| {})))
}
let view = records(vec![Record { title: "Report".into() }]);
```

`Read<T>` dereferences to T and owns an immutable snapshot. `List<T>::iter()` yields
owned item snapshots, so items can be passed directly to retained children. An
individual item can outlive its original list without retaining the other items.
List construction allocates storage per item and one shared list; iteration does
not allocate or clone the domain objects. Read/List use Arc internally and are
Send + Sync when their contents are; workers can return ordinary Vec<T> or List<T>.

Passing `&snapshot`, `&list`, or `&callback` to a correspondingly declared component
parameter shares it immediately, without retaining the reference itself. Read<String>
also accepts a non-static &str and copies it once into owned storage. Type aliases
keep their ordinary Rust signatures; spell Read/List/Callback/AsyncCallback in a
parameter declaration to request the macro's automatic input conversion.
Named component identity uses the normalized input types: changing a call from a
plain value to a snapshot, or from a closure to Callback, does not reset its state.

Other parameters preserve normal Rust value semantics: they must be Clone + 'static
and are cloned before each execution. Prefer Read<T> over String/Vec/large structs
when those copies would be expensive. Generics, const generics, mut parameters,
explicit return types, and omitted return types are supported. The macro does not
silently rewrite a plain domain type into a shared type.

```compile_fail
use voidui::{component, text};
struct Owned(String);
#[component]
fn label(value: Owned) { text(value.0) } // Use Read<Owned> for a non-Clone input.
```

A component description cannot retain a borrow of the caller's stack. Plain &str
parameters accept static strings. Read<String> provides an owned alternative.

```compile_fail
use voidui::{component, text};
#[component]
fn label(value: &str) { text(value) }
let title = String::from("temporary");
let description = label(title.as_str());
```

The closure constructor still supports non-Clone captures borrowed during rendering:

```rust
use voidui::{component, text};
struct Model { title: String }
let model = Model { title: "Owned model".into() };
let view = component(move || text(model.title.clone()));
```

Use `capture!(model, callback => async move || { ... })` when multiple closures need
owned copies of shared handles. It clones only the explicitly listed names, once
when creating the closure. State, Store, Selection and Resource handles are Copy,
so ordinary move closures can use them without preparatory clones.

### Skipping equal component inputs

Use `#[component(memo)]` for a component whose inputs implement PartialEq. Equal
inputs skip parent-driven execution. Its own subscribed state changes still execute
it. New callback identities and changed wrapper modifiers also invalidate the skip.

Read/List compare snapshot identity; State/Store/Selection compare owner identity;
Callback/AsyncCallback compare callable identity. These checks do not scan model
contents. Plain values use their PartialEq implementation. Interior mutations of
Read<T> are not observable: mutable UI data belongs in tracked state.

Moving a closure into a local variable does not stabilize its callback identity:
the variable is recreated whenever the component executes. Use `Callback::default()`
for an omitted action. Retain a real callback in state and clone its handle when
passing it to memoized children:

```rust
use voidui::{Callback, component, div, state};

#[component(memo)]
fn action(on_action: Callback<()>) {
    div().on_click(move || on_action.call(()))
}

#[component]
fn counter() {
    let count = state(|| 0usize);
    // Read the live state on activation, rather than capturing an old value.
    let on_action = state(move || Callback::new(move |()| count.update(|n| *n += 1)));
    div().child(count.get().to_string()).child(action(on_action.get()))
}
```

The unannotated closure constructor is not memoized: equality of arbitrary closure
captures cannot be inferred safely. Build repeatable named components for large
lists where parent-driven execution would otherwise be expensive.

## State lifetime and reads

`state(initializer)` allocates one slot on first mount. Later renders reuse that
slot without executing the initializer. Call ordered hooks (`state` and `on_mount`) unconditionally, in the same
order. `task_scope()` is an accessor and consumes no ordered slot. Changed hook counts and types panic in debug and release builds. Swapping
two same-type hooks cannot be detected. Calling `state()` outside a component or
inside another state initializer also panics.

`State<T>` is a pointer-sized Copy handle on the UI thread:

- `with(|value| ...)` borrows without cloning. `get()` clones the current value.
- `update(|value| ...)` mutates in place. `set(value)` replaces it.
- `set_if_changed(value)` requires `PartialEq` and skips equal writes.
- `is_mounted()` checks whether the owner still retains the slot.

Render-time reads subscribe the current component. Writes notify those readers,
including readers in another tree, without automatically rerendering a nonreading
owner. Dependencies are refreshed on every render; conditional **reads** are valid.
Event-time reads do not subscribe. A state shared with children can therefore live
at a stable owner while only the children that read it execute after a write.
Tracking in State<T> is per value. Use Store for key-level subscriptions and
Selection for a single selected item; both are described below.

Unmount drops the stored value even if old callbacks retain handles. Writes through
an unmounted handle do nothing and do not run update closures; reads panic. Lift
state to a surviving parent when it should persist while a branch is collapsed.
State handles are neither Send nor Sync. Local [async tasks](tasks.md) can retain
state handles and update them after an await. Use `spawn_background` or workers for
cross-thread work, then await its result in the local task before writing state.

Writes during component rendering are rejected. Perform writes from callbacks or
host events. User-code panics restore the hook scope, but reconciliation is not a
transactional error boundary: reset the affected tree with `build_root` if the host
catches a render panic.

## Keyed collections and selection

```rust
use voidui::{component, div, store, selection};
let view = component(|| {
    let records = store(|| [(1, String::from("First")), (2, String::from("Second"))]);
    let selected = selection::<usize>();
    div().children(records.keys().into_iter().map(|key| component(move || {
        let title = records.with(&key, |value| value.cloned().unwrap_or_default());
        div().child(title)
            .class(if selected.is_selected(&key) { "selected" } else { "" })
            .on_click(move || selected.select(key))
    }).key(key.to_string())))
});
```

Store accepts ordinary (key, value) pairs. with/get/contains_key subscribe only to
the requested key, including missing keys. insert/remove notify that key's readers;
keys/len subscribe to membership changes, not value replacements. HashMap ordering
is unspecified; sort keys explicitly when order matters. get returns an immutable
Read<V> snapshot, with no V: Clone requirement. Replacing an entry never mutates
previously returned snapshots. update(key, |value| ...) mutates only that entry;
it requires V: Clone and copies V only if an older snapshot is still shared. An
unshared entry is mutated in place. set_if_changed adds an explicit PartialEq check.

Single selection notifies the old and new selected keys. Components reading get()
subscribe to the selected key as a whole. These handles are component-owned and
Copy; snapshots deliberately remain owned and may outlive the originating component.

The keyed APIs avoid scanning every row's selector on each update. Arbitrary field
writes through &mut T cannot reveal which fields changed; State::update therefore
continues to notify the whole value. Model independently updated fields as separate
state slots, or store independently updated records by key.

## Identity and retained widgets

Sibling identity consists of a key (or an exact unkeyed position) and component or
widget type. Give moving siblings stable data keys. Changing the key or type creates
a fresh instance. Duplicate sibling keys are errors, including through `append_child`.
An ID or class does not identify a component.

Components add no CSS, layout, hit-test, or selection node. `.id()`, `.class()`, and
`.add_class()` apply to their rendered root; outer IDs override inner IDs and classes
are combined. Components may return other components, or switch their physical root
type. Surviving wrapper state remains valid in either case.

Matching widget IDs, focus, selection, and top-layer membership survive updates.
Removed subtrees use the existing lifecycle cleanup. Selection boundaries adjust
when text or children are removed. `Div` and `Text` apply new descriptions in place;
unchanged text retains prepared shaping data. Custom `Widget` implementations can
implement `reconcile` and return `WidgetUpdate::{Unchanged, Changed, Replace}`.
The safe default replaces unknown widget data and invalidates layout and painting.

`on_click` accepts sync or async callbacks and keeps the original builder type;
register it before or after children. Pointer press/release activates an enabled
handler and bubbles to its ancestors. See [Events and dragging](events.md);
focused handlers also accept Enter/Space. Use `.tag("button")` for button focus and
CSS behavior. `WidgetTree::click(id)` provides programmatic/headless activation.

## Scheduling and cost

Writes coalesce into one queue entry per reader. A flush visits queued components
parent-first and skips a child already rendered by its parent. Reexecuting a parent
also reconciles the descendants it returns; #[component(memo)] skips matching
children with equal inputs and no pending state invalidation.
Unrelated branches do not execute. A value-only leaf update does not scan siblings.
A physical root replacement can require moving entries in its parent's child list.

Component-owned state slots use a compact vector. Copy handles resolve through a
UI-thread generational registry containing weak references; unmount removes the
entry, and old handles cannot address reused slots. Registry capacity is reused
until thread exit. No ancestor paths are copied. The common one-reader state has no subscriber allocation; shared state uses
an indexed subscriber table. Component metadata lives in sparse side tables, adding
no fields or layout wrappers to ordinary retained widget nodes. Clean update checks
are constant-time and do not allocate or traverse the tree.

Native windows install an event-driven wakeup and flush component updates before
sleeping, including while minimized. Style/layout work waits for a drawable frame.
No polling timer is installed. Equal output leaves style, layout, and paint caches
clean. Changed geometry still uses the existing full layout pass; CSS invalidation
still uses the existing cascade. Local component execution does not promise local
layout or local selector matching.

Headless hosts can use the same lifecycle explicitly:

```rust
use voidui::{component, state, div, core::widget_tree::WidgetTree};
let mut tree = WidgetTree::new();
tree.build_root(component(|| { let _ = state(|| 0usize); div() }));
tree.flush_updates(); // Also called by update_styles() and layout().
assert_eq!(tree.state_count(), 1);
```

`reconcile_root(new_description)` supplies new root inputs while preserving matching
instances. `build_root` explicitly resets the tree. `has_pending_updates` is a cheap
host check; `set_update_waker` accepts a main-thread callback that should request
work without reentering the tree or writing state. Destructors may enqueue a later
batch during unmount; a flush does not spin until convergence.

## Reproduce validation

```sh
cargo test --workspace
cargo test --doc -p voidui
cargo test --example file_tree
cargo bench --bench state -- 10000 50000
```

The benchmark compares the same DOM with and without one component/usize slot per
row, reports requested retained heap bytes, and measures local updates, clean checks,
batched writes, shared-state fanout, and bulk unmount. It asserts one executed component per local update and zero
allocations for warmed value-only div updates. It excludes CSS, layout, font shaping,
GPU work, allocator metadata, and the caller's handle array. Values vary with machine,
allocator, collection capacity, compiler, and component inputs; use the command above
for measurements on your target.

### Validation of the data API

`tests/data_model.rs` covers non-Clone input ownership, independent list item
lifetimes, normalized component identity, memo invalidation, keyed edits, selection,
and stale Copy handles. `tests/resources.rs` covers resource commit/cancellation
behavior. `cargo run --example resources -- --smoke README.md` checks real file
loading and reloads through a native window.

The state benchmark also asserts that a 10,000-row keyed store edits/renders one
row with zero warmed update allocations. On a macOS/aarch64 release run on
2026-09-13, its keyed update took about 0.34 microseconds. The plain local-state
update retained zero warmed allocations. Requested additional storage per component
with one usize state increased from about 404 to 459 bytes after introducing the
Copy-handle registry and optional memo metadata. This is a measured space tradeoff,
not a claim of reduced memory consumption. Idle task checks still allocate nothing;
these microbenchmarks do not measure process-wide power usage or rendering cost.

## Text input bindings

`input(value)` and `textarea(value)` accept `State<String>` directly. Start an
empty value with `state(String::new)`. Their retained widgets subscribe directly,
so the parent reruns only when it explicitly reads that state. The input keeps
selection, composition and history internally; only committed text is published.
See [Inputs and reusable editing](input.md) for String binding and advanced Editor APIs.
