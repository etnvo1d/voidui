#![doc = include_str!("../../docs/input.md")]
//! Text controls share one editor view. Box geometry follows CSS and intrinsic
//! rows/columns; typing changes retained text, never the control's intrinsic size.
use crate::{
    core::{
        context::{DrawContext, LayoutContext},
        element::ElementProps,
        event::EventResult,
        geometry::{Point, Rect},
        input::{InputContext, InputEvent, KeyInput, PointerPhase, TextInputClient},
        layout::{self, LayoutInput, LayoutOutput, layout_leaf},
        updates::WidgetInvalidator,
        widget::{Widget, WidgetBuilder, WidgetUpdate},
    },
    editing::{
        Bias, EditKind, Editor, EditorLayout, EditorState, LayoutOptions, Motion, Selection,
        SelectionSet,
    },
    render::{self, Painter, TextSystem},
    style::{
        selection::{Cursor, SelectionColors},
        style::Style,
        text::TextStyle,
    },
};
use std::{
    cell::{OnceCell, RefCell},
    collections::BTreeMap,
    ops::Range,
    rc::Rc,
    sync::Arc,
    time::{Duration, Instant},
};
use taffy::util::ResolveOrZero;
use winit::keyboard::{Key, NamedKey, SmolStr};

mod autoscroll;
mod client;
mod embedded;
mod keymap;
mod view;
pub use keymap::KeyContext;
use view::{clamp_scroll, display_boundary, display_selection, display_slice};

type KeyHandler = Rc<dyn Fn(&KeyInput, KeyContext<'_>) -> bool>;
type ChangeHandler = Rc<dyn Fn(&Editor)>;
#[derive(Debug, Clone, Copy)]
pub struct EditorOptions {
    pub columns: usize,
    pub rows: usize,
    pub accept_tab: bool,
    /// No repeating task exists: only the focused, drawable caret schedules a wakeup.
    pub blink_interval: Duration,
}
impl Default for EditorOptions {
    fn default() -> Self {
        Self {
            columns: 20,
            rows: 2,
            accept_tab: false,
            blink_interval: Duration::from_millis(500),
        }
    }
}
#[derive(Default)]
struct ViewLayout {
    selection_drag: Option<Box<autoscroll::SelectionDrag>>,
    text: EditorLayout,
    placeholder: EditorLayout,
    options: Option<LayoutOptions>,
    source: Option<(u64, u64, u64, Option<u64>)>,
    // Only projection-enabled pointer gestures allocate this snapshot. Selection
    // changes must not move the text being targeted by the same gesture.
    pointer_projection: Option<Box<(u64, crate::editing::Projection)>>,
    font_revision: u64,
    system: Option<Arc<TextSystem>>,
    scroll: Point<f32>,
    // Automatic reveal has been handled for this generation, including when a
    // pointer selection deliberately preserves the current viewport.
    reveal_generation: Option<u64>,
    placeholder_key: Option<(String, LayoutOptions)>,
}

/// Ordinary controls bind a String state. An explicit Editor is optional and
/// exposes advanced document/selection/command access without value snapshots.
#[derive(Clone)]
pub enum InputValue {
    State(crate::State<String>),
    Editor(Editor),
}
impl From<crate::State<String>> for InputValue {
    fn from(value: crate::State<String>) -> Self {
        Self::State(value)
    }
}
impl From<&crate::State<String>> for InputValue {
    fn from(value: &crate::State<String>) -> Self {
        Self::State(*value)
    }
}
impl From<Editor> for InputValue {
    fn from(value: Editor) -> Self {
        Self::Editor(value)
    }
}
impl From<&Editor> for InputValue {
    fn from(value: &Editor) -> Self {
        Self::Editor(value.clone())
    }
}
impl InputValue {
    fn same_source(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::State(a), Self::State(b)) => a.same_state(b),
            (Self::Editor(a), Self::Editor(b)) => a.same_session(b),
            _ => false,
        }
    }
}

pub struct TextEdit {
    source: InputValue,
    editor: OnceCell<Editor>,
    state_subscription: Option<crate::core::state::StateSubscription>,
    multiline: bool,
    options: EditorOptions,
    rows: usize,
    columns: usize,
    placeholder: String,
    readonly: bool,
    disabled: bool,
    style: TextStyle,
    placeholder_style: TextStyle,
    selection_colors: SelectionColors,
    view: RefCell<ViewLayout>,
    views: crate::editing::EditorViews,
    extensions: crate::editing::EditorExtensions,
    extension_host: RefCell<crate::editing::extensions::ExtensionHost>,
    viewport_options: crate::editing::ViewportOptions,

    subscription: Option<Rc<crate::editing::handle::Observer>>,
    constraint: Option<Rc<crate::editing::EditConstraint>>,
    invalidator: Option<WidgetInvalidator>,
    active: bool,
    caret_visible: bool,
    blink_at: Option<Instant>,
    drag_position: Option<Point<f32>>,
    key_handler: Option<KeyHandler>,
    on_change: Option<ChangeHandler>,
    on_submit: Option<ChangeHandler>,
}
impl TextEdit {
    pub fn new(value: impl Into<InputValue>, multiline: bool) -> Self {
        let source = value.into();
        Self {
            source,
            editor: OnceCell::new(),
            state_subscription: None,
            multiline,
            options: Default::default(),
            rows: EditorOptions::default().rows,
            columns: EditorOptions::default().columns,
            placeholder: String::new(),
            readonly: false,
            disabled: false,
            style: Default::default(),
            placeholder_style: Default::default(),
            selection_colors: Default::default(),
            view: Default::default(),
            views: Default::default(),
            extensions: Default::default(),
            extension_host: Default::default(),
            viewport_options: Default::default(),
            subscription: None,
            constraint: None,
            invalidator: None,
            active: false,
            caret_visible: true,
            blink_at: None,
            drag_position: None,
            key_handler: None,
            on_change: None,
            on_submit: None,
        }
    }
    pub fn editor(&self) -> &Editor {
        self.editor.get_or_init(|| match &self.source {
            InputValue::Editor(editor) => {
                assert!(
                    self.multiline
                        || editor
                            .with(|s| !s.text().contains(['\r', '\n', '\u{2028}', '\u{2029}'])),
                    "input requires a single-line Editor document"
                );
                editor.clone()
            }
            InputValue::State(state) => {
                Editor::new(state.with_untracked(|text| self.normalized(text)))
            }
        })
    }
    fn synchronize_value(&mut self) -> crate::core::updates::Invalidation {
        let InputValue::State(state) = &self.source else {
            return Default::default();
        };
        if !state.is_mounted() {
            return Default::default();
        }
        let old_empty = self
            .editor()
            .with(|s| s.document().is_empty() && s.composition().is_none());
        state.with_untracked(|text| {
            self.editor().synchronize(|editor| {
                // Echoes of committed input leave selection, history and IME intact.
                if text == editor.text() {
                    return;
                }
                let normalized = self.normalized(text);
                if normalized == editor.text() {
                    return;
                }
                editor.cancel_composition();
                let head = editor.selections().primary().head.min(normalized.len());
                let mut head = head;
                while !normalized.is_char_boundary(head) {
                    head -= 1;
                }
                let mut transaction = crate::editing::Transaction::new(
                    editor.revision(),
                    [crate::editing::Edit::new(
                        0..editor.text().len(),
                        normalized,
                    )],
                );
                transaction.selection = Some(SelectionSet::single(Selection::caret(head)));
                editor
                    .transact(transaction)
                    .expect("normalized bound text must satisfy input constraints");
                editor.clear_history();
            });
        });
        let new_empty = self
            .editor()
            .with(|s| s.document().is_empty() && s.composition().is_none());
        crate::core::updates::Invalidation {
            style: old_empty != new_empty,
            layout: false,
        }
    }
    fn publish_value(&self) {
        if let InputValue::State(state) = &self.source {
            if state.is_mounted() {
                let equal = state.with_untracked(|text| self.editor().with(|s| s.text() == text));
                if !equal {
                    state.set_if_changed(self.editor().text());
                }
            }
        }
    }
    fn repaint(&self) {
        if let Some(invalidator) = &self.invalidator {
            invalidator.repaint();
        }
    }
    fn end_pointer_gesture(&mut self) {
        self.view.borrow_mut().selection_drag = None;
        self.view.borrow().text.cancel_view_pointer();
        self.end_text_gesture();
    }
    fn end_text_gesture(&mut self) {
        self.drag_position = None;
        let mut view = self.view.borrow_mut();
        if view.pointer_projection.take().is_some() {
            // Selection may already match the prepared key; force the deferred
            // projection update even when release changes no document state.
            view.source = None;
            drop(view);
            self.repaint();
        }
    }
    fn reset_caret(&mut self, now: Instant) {
        self.caret_visible = true;
        self.blink_at = self
            .blink_enabled()
            .then_some(now + self.options.blink_interval);
        self.repaint();
    }
    fn blink_enabled(&self) -> bool {
        self.active
            && !self.disabled
            && self.view.borrow().text.focused_view().is_none()
            && self.style.caret_animation
            && !self.options.blink_interval.is_zero()
            && self
                .editor()
                .with(|s| s.composition().is_none() && s.selections().iter().any(|s| s.is_caret()))
    }
    fn normalized(&self, text: &str) -> String {
        if self.multiline {
            text.replace("\r\n", "\n").replace('\r', "\n")
        } else {
            text.chars()
                .filter(|ch| !matches!(ch, '\r' | '\n' | '\u{2028}' | '\u{2029}'))
                .collect()
        }
    }
}

/// A single-line control. Keep the Editor handle stable across component renders.
pub fn input(value: impl Into<InputValue>) -> WidgetBuilder<TextEdit> {
    control(value.into(), false)
}
/// A multiline control with CSS sizing and soft wrapping enabled by default.
pub fn textarea(value: impl Into<InputValue>) -> WidgetBuilder<TextEdit> {
    control(value.into(), true)
}
/// A rich multiline editor backed by a stable document handle. Formatting,
/// selection, undo, and IME use the same session as `textarea(&editor)`.
/// A String binding cannot carry formatting; keep an `Editor` for rich content.
pub fn rich_editor(editor: &Editor) -> WidgetBuilder<TextEdit> {
    control(InputValue::Editor(editor.clone()), true)
}
fn control(value: InputValue, multiline: bool) -> WidgetBuilder<TextEdit> {
    WidgetBuilder {
        events: Default::default(),
        widget: TextEdit::new(value, multiline),
        props: ElementProps::new(Style::default()),
        children: Vec::new(),
    }
}
impl WidgetBuilder<TextEdit> {
    pub fn views(mut self, views: crate::editing::EditorViews) -> Self {
        self.widget.views = views;
        self
    }
    pub fn extensions(mut self, extensions: crate::editing::EditorExtensions) -> Self {
        self.widget.extensions = extensions;
        self
    }
    pub fn viewport_options(mut self, options: crate::editing::ViewportOptions) -> Self {
        self.widget.viewport_options = options;
        self
    }

    pub fn placeholder(self, text: impl Into<SmolStr>) -> Self {
        self.attr("placeholder", text)
    }
    pub fn read_only(mut self, readonly: bool) -> Self {
        if readonly {
            self.props.attributes.insert("readonly".into(), "".into());
        } else {
            self.props.attributes.remove("readonly");
        }
        self
    }
    pub fn disabled(mut self, disabled: bool) -> Self {
        if disabled {
            self.props.attributes.insert("disabled".into(), "".into());
        } else {
            self.props.attributes.remove("disabled");
        }
        self
    }
    pub fn rows(self, rows: usize) -> Self {
        self.attr("rows", rows.max(1).to_string())
    }
    pub fn columns(self, columns: usize) -> Self {
        self.attr("cols", columns.max(1).to_string())
    }
    pub fn editor_options(mut self, options: EditorOptions) -> Self {
        self.widget.options = options;
        self
    }
    /// Return true to consume a command before the default keymap. This callback
    /// can retain its own state for modal keymaps without changing the editor core.
    pub fn on_key(mut self, handler: impl Fn(&KeyInput, KeyContext<'_>) -> bool + 'static) -> Self {
        self.widget.key_handler = Some(Rc::new(handler));
        self
    }
    pub fn on_change(mut self, handler: impl Fn(&Editor) + 'static) -> Self {
        self.widget.on_change = Some(Rc::new(handler));
        self
    }
    pub fn on_submit(mut self, handler: impl Fn(&Editor) + 'static) -> Self {
        self.widget.on_submit = Some(Rc::new(handler));
        self
    }
}
impl Widget for TextEdit {
    fn tag_name(&self) -> &'static str {
        if self.multiline { "textarea" } else { "input" }
    }
    fn update_model(&mut self) -> crate::core::updates::Invalidation {
        self.synchronize_value()
    }
    fn attach(&mut self, invalidator: WidgetInvalidator) {
        if let InputValue::State(state) = &self.source {
            if self.state_subscription.is_none() {
                self.state_subscription = Some(state.subscribe_widget(invalidator.clone()));
            }
        }
        if self.subscription.is_none() {
            self.subscription = Some(self.editor().subscribe(invalidator.clone()));
        }
        self.invalidator = Some(invalidator);
        self.extension_host
            .borrow_mut()
            .configure(self.extensions.clone(), self.invalidator.clone());
        self.synchronize_value();
        if !self.multiline && self.constraint.is_none() {
            self.constraint = Some(self.editor().update(|s| {
                s.constrain(|_, changes| {
                    if changes
                        .edits()
                        .iter()
                        .any(|e| e.insert.contains(['\r', '\n', '\u{2028}', '\u{2029}']))
                    {
                        Err(crate::editing::EditError::Rejected(
                            "single-line input rejects line breaks",
                        ))
                    } else {
                        Ok(())
                    }
                })
            }));
        }
    }
    fn default_style(&self) -> Option<Style> {
        let mut style = Style::default();
        let overflow = if self.multiline {
            crate::Overflow::Auto
        } else {
            crate::Overflow::Hidden
        };
        for p in [
            crate::style::scroll::ScrollProperty::OverflowX,
            crate::style::scroll::ScrollProperty::OverflowY,
        ] {
            style.scroll.set(
                p,
                crate::style::scroll::ScrollValue::Overflow(overflow).into(),
            );
        }
        style.layout.min_size.width = layout::LengthPercentageAuto::length(0.0);
        style.cursor = Cursor::Icon(winit::window::CursorIcon::Text).into();
        Some(style)
    }
    fn reconcile(&mut self, next: &dyn Widget) -> WidgetUpdate {
        let Some(next) = (next as &dyn std::any::Any).downcast_ref::<Self>() else {
            return WidgetUpdate::Replace;
        };
        let source_changed = !self.source.same_source(&next.source);
        let changed = source_changed
            || self.multiline != next.multiline
            || self.options.columns != next.options.columns
            || self.options.rows != next.options.rows;
        if source_changed {
            self.end_pointer_gesture();
            self.source = next.source.clone();
            self.editor.take();
            self.state_subscription = None;
            self.subscription = None;
            self.constraint = None;
            *self.view.borrow_mut() = ViewLayout::default();
        }
        if self.multiline != next.multiline {
            self.end_pointer_gesture();
            self.constraint = None;
            self.subscription = None;
            self.editor.take();
            *self.view.borrow_mut() = ViewLayout::default();
        }
        self.multiline = next.multiline;
        if !self.views.same(&next.views) || !self.extensions.same(&next.extensions) {
            self.end_pointer_gesture();
            self.views = next.views.clone();
            self.extensions = next.extensions.clone();
            self.view.borrow_mut().source = None;
            self.extension_host
                .borrow_mut()
                .configure(self.extensions.clone(), self.invalidator.clone());
        }
        self.viewport_options = next.viewport_options;
        self.options = next.options;
        self.key_handler = next.key_handler.clone();
        self.on_change = next.on_change.clone();
        self.on_submit = next.on_submit.clone();
        if changed {
            WidgetUpdate::Changed
        } else {
            WidgetUpdate::Unchanged
        }
    }
    fn text_input(&self) -> Option<&dyn TextInputClient> {
        Some(self)
    }
    fn text_input_mut(&mut self) -> Option<&mut dyn TextInputClient> {
        Some(self)
    }
    fn layout(&mut self, inputs: LayoutInput, cx: LayoutContext<'_, '_>) -> LayoutOutput {
        let style = cx.text_style();
        self.style = style.clone();
        let system = cx.text_layout().system().clone();
        let em = system.resolve_font(&style.font);
        let column = f32::from(
            system
                .ch_advance(em, render::px(style.font_size))
                .unwrap_or(render::px(style.font_size * 0.5)),
        );
        let line_height = style.line_height.resolve(style.font_size);
        let mut out = layout_leaf(cx.layout_style(), inputs, |_, _| layout::Size {
            width: column * self.columns.max(1) as f32,
            height: line_height
                * if self.multiline {
                    self.rows.max(1) as f32
                } else {
                    1.0
                },
        });
        let top = cx
            .layout_style()
            .padding
            .top
            .resolve_or_zero(inputs.parent_size.width, crate::core::layout::resolve_calc)
            + cx.layout_style()
                .border
                .top
                .resolve_or_zero(inputs.parent_size.width, crate::core::layout::resolve_calc);
        out.baselines.first = Some(
            top + f32::from(system.baseline_offset(
                em,
                render::px(style.font_size),
                render::px(line_height),
            )),
        );
        out.baselines.last = out.baselines.first;
        out
    }
    fn draw(&self, painter: &mut Painter<'_>, cx: DrawContext) -> render::Result<()> {
        self.paint_control(painter, cx)
    }
}
