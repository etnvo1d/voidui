//! One logical document selection, with DOM-order endpoints and independently
//! filtered painted fragments. No glyph shaping or layout occurs while dragging.
#![doc = include_str!("../../docs/selection.md")]
use super::{
    geometry::{Point, Rect},
    widget::WidgetId,
    widget_tree::WidgetTree,
};
use crate::style::{
    layer::Visibility,
    selection::{SelectionColors, UserSelect},
};
use slotmap::SecondaryMap;

mod interaction;
mod navigation;
pub use navigation::SelectionMove;
use std::{cmp::Ordering, collections::HashSet, ops::Range};

/// Rust text offsets are UTF-8 byte boundaries. Child offsets address the logical
/// DOM children, including a widget's own text as its first virtual text child.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionPoint {
    Text { node: WidgetId, byte: usize },
    Children { node: WidgetId, index: usize },
}
impl SelectionPoint {
    pub fn text(node: WidgetId, byte: usize) -> Self {
        Self::Text { node, byte }
    }
    pub fn children(node: WidgetId, index: usize) -> Self {
        Self::Children { node, index }
    }
    pub fn node(self) -> WidgetId {
        match self {
            Self::Text { node, .. } | Self::Children { node, .. } => node,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    pub anchor: SelectionPoint,
    pub focus: SelectionPoint,
}
impl Selection {
    pub fn is_collapsed(self) -> bool {
        self.anchor == self.focus
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionDirection {
    Forward,
    Backward,
    Directionless,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionGranularity {
    Grapheme,
    Word,
    Paragraph,
}
#[derive(Debug, Clone, Copy)]
struct Drag {
    seed: Selection,
    origin: SelectionPoint,
    granularity: SelectionGranularity,
    // Keep the starting cursor over auto-styled areas. Selection endpoints can
    // snap from container padding to text without the pointer hitting text.
    cursor: crate::style::selection::Cursor,
}
#[derive(Clone, Copy)]
struct NodeSpan {
    start: usize,
    end: usize,
    text: Option<usize>,
}
#[derive(Default)]
struct DocumentIndex {
    spans: SecondaryMap<WidgetId, NodeSpan>,
    texts: Vec<WidgetId>,
}
impl DocumentIndex {
    /// Text spans are monotonically ordered, including a widget's own virtual
    /// text child. A short selection need not scan every text node in the document.
    fn intersecting(&self, tree: &WidgetTree, start: usize, end: usize) -> Range<usize> {
        let first = self.texts.partition_point(|id| {
            self.spans[*id].text.unwrap()
                + tree.nodes[*id]
                    .widget
                    .as_ref()
                    .unwrap()
                    .text_content()
                    .unwrap()
                    .len()
                < start
        });
        let last = self
            .texts
            .partition_point(|id| self.spans[*id].text.unwrap() <= end);
        first..last
    }
    fn rebuild(&mut self, tree: &WidgetTree) {
        self.spans.clear();
        self.texts.clear();
        let mut cursor = 0;
        fn visit(index: &mut DocumentIndex, tree: &WidgetTree, id: WidgetId, cursor: &mut usize) {
            let start = *cursor;
            *cursor += 1;
            let text = tree.nodes[id]
                .widget
                .as_ref()
                .and_then(|w| w.text_content())
                .map(|s| {
                    let start = *cursor;
                    *cursor += s.len() + 2;
                    index.texts.push(id);
                    start
                });
            for child in &tree.nodes[id].children {
                visit(index, tree, *child, cursor);
                *cursor += 1;
            }
            let end = *cursor;
            *cursor += 1;
            index.spans.insert(id, NodeSpan { start, end, text });
        }
        if let Some(root) = tree.root() {
            visit(self, tree, root, &mut cursor);
        }
    }
    fn offset(&self, tree: &WidgetTree, p: SelectionPoint) -> Option<usize> {
        let span = *self.spans.get(p.node())?;
        match p {
            SelectionPoint::Text { node, byte } => {
                let text = tree.nodes.get(node)?.widget.as_ref()?.text_content()?;
                (byte <= text.len() && text.is_char_boundary(byte))
                    .then(|| span.text.unwrap() + byte)
            }
            SelectionPoint::Children { node, index } => {
                if index == 0 {
                    return Some(span.start);
                }
                let own = usize::from(span.text.is_some());
                if index == 1 && own == 1 {
                    return Some(
                        span.text.unwrap()
                            + tree.nodes[node].widget.as_ref()?.text_content()?.len()
                            + 1,
                    );
                }
                let child = *tree.nodes[node].children.get(index.checked_sub(own + 1)?)?;
                Some(self.spans[child].end + 1)
            }
        }
    }
}

#[derive(Default)]
pub(crate) struct SelectionState {
    range: Option<Selection>,
    // Parley retains visual affinity and the preferred x for vertical movement.
    // Document endpoints remain source UTF-8 offsets, independent of paragraphs.
    local_cursor: Option<(WidgetId, voidui_gpui_wgpu::parley::Selection)>,
    drag: Option<Drag>,
    programmatic: bool,
    index: DocumentIndex,
    index_dirty: bool,
    fragments: SecondaryMap<WidgetId, Range<usize>>,
    fragments_dirty: bool,
    revision: u64,
    pub(crate) paint_dirty: bool,
    pending_change: bool,
    start_handler: Option<Box<dyn FnMut(Selection) -> bool>>,
}
impl SelectionState {
    pub(crate) fn invalidate(&mut self, structure: bool) {
        if structure {
            self.local_cursor = None;
        }
        self.index_dirty |= structure;
        self.fragments_dirty = true;
        self.paint_dirty |= self.range.is_some();
    }
    fn set(&mut self, range: Option<Selection>, programmatic: bool) -> bool {
        if self.range == range && self.programmatic == programmatic {
            return false;
        }
        self.local_cursor = None;
        self.range = range;
        self.programmatic = programmatic;
        self.fragments_dirty = true;
        self.paint_dirty = true;
        self.pending_change = true;
        self.revision += 1;
        true
    }
    fn set_user(&mut self, range: Selection) -> Option<bool> {
        if self.range.is_none_or(|old| old.is_collapsed())
            && !range.is_collapsed()
            && self
                .start_handler
                .as_mut()
                .is_some_and(|handler| !handler(range))
        {
            self.drag = None;
            return None;
        }
        Some(self.set(Some(range), false))
    }
    pub(crate) fn clear(&mut self) {
        self.set(None, false);
        self.drag = None;
        self.invalidate(true);
    }
}

impl WidgetTree {
    fn ensure_selection_index(&self) {
        let mut state = self.selection.borrow_mut();
        if state.index_dirty || state.index.spans.len() != self.nodes.len() {
            state.index.rebuild(self);
            state.index_dirty = false;
            state.fragments_dirty = true;
        }
    }
    /// Document-level cancellable selectstart counterpart. Return false to deny
    /// a new non-collapsed user selection; programmatic ranges are unaffected.
    pub fn on_select_start(&mut self, handler: impl FnMut(Selection) -> bool + 'static) {
        self.selection.get_mut().start_handler = Some(Box::new(handler));
    }
    /// Consume a coalesced boundary-change notification. Some(None) means cleared;
    /// None means no change since the previous call. No polling task is spawned.
    pub fn take_selection_change(&self) -> Option<Option<Selection>> {
        let mut state = self.selection.borrow_mut();
        std::mem::take(&mut state.pending_change).then_some(state.range)
    }
    pub fn selection(&self) -> Option<Selection> {
        self.selection.borrow().range
    }
    /// Visual affinity matters at wrap/bidi boundaries even when the byte offset
    /// does not change. Programmatic text points default to downstream affinity.
    pub fn selection_focus_affinity(&self) -> Option<voidui_gpui_wgpu::parley::Affinity> {
        let state = self.selection.borrow();
        let range = state.range?;
        if !matches!(range.focus, SelectionPoint::Text { .. }) {
            return None;
        }
        Some(
            state
                .local_cursor
                .map(|(_, s)| s.focus().affinity())
                .unwrap_or(voidui_gpui_wgpu::parley::Affinity::Downstream),
        )
    }
    pub fn selection_revision(&self) -> u64 {
        self.selection.borrow().revision
    }
    pub fn selection_direction(&self) -> SelectionDirection {
        self.ensure_selection_index();
        let state = self.selection.borrow();
        let Some(range) = state.range else {
            return SelectionDirection::Directionless;
        };
        match state
            .index
            .offset(self, range.anchor)
            .cmp(&state.index.offset(self, range.focus))
        {
            Ordering::Less => SelectionDirection::Forward,
            Ordering::Greater => SelectionDirection::Backward,
            Ordering::Equal => SelectionDirection::Directionless,
        }
    }
    /// Programmatic ranges are not restricted by user-select, which is a UI
    /// convenience rather than copy protection. Points must be valid text/child boundaries.
    pub fn set_selection(
        &mut self,
        anchor: SelectionPoint,
        focus: SelectionPoint,
    ) -> anyhow::Result<bool> {
        self.ensure_selection_index();
        {
            let state = self.selection.borrow();
            anyhow::ensure!(
                state.index.offset(self, anchor).is_some()
                    && state.index.offset(self, focus).is_some(),
                "invalid selection boundary"
            );
        }
        let state = self.selection.get_mut();
        state.drag = None;
        Ok(state.set(Some(Selection { anchor, focus }), true))
    }
    pub fn clear_selection(&mut self) -> bool {
        let state = self.selection.get_mut();
        state.drag = None;
        state.set(None, false)
    }
    pub fn selection_is_dragging(&self) -> bool {
        self.selection.borrow().drag.is_some()
    }
    pub fn end_selection_drag(&mut self) {
        self.selection.get_mut().drag = None;
    }
    fn contents(&self, id: WidgetId) -> Selection {
        let own = usize::from(
            self.nodes[id]
                .widget
                .as_ref()
                .is_some_and(|w| w.text_content().is_some()),
        );
        Selection {
            anchor: SelectionPoint::children(id, 0),
            focus: SelectionPoint::children(id, self.nodes[id].children.len() + own),
        }
    }
    fn before(&self, id: WidgetId) -> SelectionPoint {
        let Some(parent) = self.nodes[id].parent else {
            return SelectionPoint::children(id, 0);
        };
        let own = usize::from(
            self.nodes[parent]
                .widget
                .as_ref()
                .is_some_and(|w| w.text_content().is_some()),
        );
        SelectionPoint::children(
            parent,
            own + self.nodes[parent]
                .children
                .iter()
                .position(|child| *child == id)
                .unwrap(),
        )
    }
    fn after(&self, id: WidgetId) -> SelectionPoint {
        match self.before(id) {
            SelectionPoint::Children { node, index } if node != id => {
                SelectionPoint::children(node, index + 1)
            }
            _ => self.contents(id).focus,
        }
    }
    fn within(&self, point: SelectionPoint, root: WidgetId) -> bool {
        self.is_descendant_or_self(point.node(), root)
    }
    fn used_select(&self, id: WidgetId) -> UserSelect {
        if self.is_inert(id) {
            UserSelect::None
        } else {
            self.nodes[id].computed.selection.used_user_select
        }
    }
    fn selection_visible(&self, id: WidgetId) -> bool {
        self.is_rendered(id)
            && self.nodes[id].computed.layer.visibility == Visibility::Visible
            && !self.is_inert(id)
    }

    fn normalize_user_selection(
        &self,
        index: &DocumentIndex,
        origin: SelectionPoint,
        mut range: Selection,
    ) -> Selection {
        let forward = index.offset(self, range.anchor) <= index.offset(self, range.focus);
        // Containment is a boundary constraint, not ordinary property inheritance.
        let mut current = Some(origin.node());
        while let Some(id) = current {
            if self.used_select(id) == UserSelect::Contain && !self.within(range.focus, id) {
                let bounds = self.contents(id);
                range.focus = if forward { bounds.focus } else { bounds.anchor };
                break;
            }
            current = self.nodes[id].parent;
        }
        if let Some(modal) = self.active_modal()
            && !self.within(range.focus, modal)
        {
            range.focus = if forward {
                self.contents(modal).focus
            } else {
                self.contents(modal).anchor
            };
        }
        current = Some(range.focus.node());
        while let Some(id) = current {
            let value = self.used_select(id);
            if value == UserSelect::Contain && !self.within(origin, id) {
                range.focus = if forward {
                    self.before(id)
                } else {
                    self.after(id)
                };
                break;
            }
            current = self.nodes[id].parent;
        }
        // `all` expands atomically, except when both endpoints are inside an
        // explicitly non-all descendant. Non-selectable fragments are filtered later.
        let mut atomics = HashSet::new();
        for point in [range.anchor, range.focus] {
            let mut current = Some(point.node());
            let mut override_root = None;
            while let Some(id) = current {
                if self.used_select(id) != UserSelect::All {
                    override_root = Some(id);
                } else if !override_root.is_some_and(|root| {
                    self.within(range.anchor, root) && self.within(range.focus, root)
                }) {
                    atomics.insert(id);
                }
                current = self.nodes[id].parent;
            }
        }
        let mut start = if forward { range.anchor } else { range.focus };
        let mut end = if forward { range.focus } else { range.anchor };
        for id in atomics {
            let all = self.contents(id);
            if index.offset(self, all.anchor) < index.offset(self, start) {
                start = all.anchor;
            }
            if index.offset(self, all.focus) > index.offset(self, end) {
                end = all.focus;
            }
        }
        if forward {
            Selection {
                anchor: start,
                focus: end,
            }
        } else {
            Selection {
                anchor: end,
                focus: start,
            }
        }
    }
    fn ensure_selection_fragments(&self) {
        self.ensure_selection_index();
        let mut state = self.selection.borrow_mut();
        if !state.fragments_dirty {
            return;
        }
        state.fragments.clear();
        state.fragments_dirty = false;
        let Some(range) = state.range.filter(|r| !r.is_collapsed()) else {
            return;
        };
        let (Some(a), Some(b)) = (
            state.index.offset(self, range.anchor),
            state.index.offset(self, range.focus),
        ) else {
            return;
        };
        let (start, end) = (a.min(b), a.max(b));
        for i in state.index.intersecting(self, start, end) {
            let id = state.index.texts[i];
            if !self.selection_visible(id)
                || (!state.programmatic && self.used_select(id) == UserSelect::None)
            {
                continue;
            }
            let text = self.nodes[id]
                .widget
                .as_ref()
                .unwrap()
                .text_content()
                .unwrap();
            let base = state.index.spans[id].text.unwrap();
            let from = start.saturating_sub(base).min(text.len());
            let to = end.saturating_sub(base).min(text.len());
            if from < to {
                state.fragments.insert(id, from..to);
            }
        }
    }
    /// Return the painted/copyable text in DOM order. Native text widgets are
    /// block leaves: a boundary between leaves inserts a newline; soft wraps do not.
    pub fn selected_text(&self) -> String {
        self.ensure_selection_fragments();
        let state = self.selection.borrow();
        let Some(range) = state.range else {
            return String::new();
        };
        let (Some(a), Some(b)) = (
            state.index.offset(self, range.anchor),
            state.index.offset(self, range.focus),
        ) else {
            return String::new();
        };
        let (start, end) = (a.min(b), a.max(b));
        let mut result = String::new();
        let mut previous_end = None;
        for id in &state.index.texts[state.index.intersecting(self, start, end)] {
            if !self.selection_visible(*id)
                || (!state.programmatic && self.used_select(*id) == UserSelect::None)
            {
                continue;
            }
            let text = self.nodes[*id]
                .widget
                .as_ref()
                .unwrap()
                .text_content()
                .unwrap();
            let base = state.index.spans[*id].text.unwrap();
            if previous_end.is_some_and(|last| start <= last && end >= base) {
                result.push('\n');
            }
            if let Some(range) = state.fragments.get(*id) {
                result.push_str(&text[range.clone()].replace("\r\n", "\n"));
            }
            previous_end = Some(base + text.len());
        }
        result
    }
    pub fn selected_range(&self, id: WidgetId) -> Option<Range<usize>> {
        self.ensure_selection_fragments();
        self.selection.borrow().fragments.get(id).cloned()
    }
    pub(crate) fn selection_paint(&self, id: WidgetId) -> Option<super::text::TextSelectionPaint> {
        if self
            .selection
            .borrow()
            .range
            .is_none_or(|r| r.is_collapsed())
        {
            return None;
        }
        let range = self.selected_range(id)?;
        let node = &self.nodes[id];
        let highlight = node
            .highlight
            .as_ref()
            .map(|h| h.computed)
            .unwrap_or_default();
        Some(super::text::TextSelectionPaint {
            range,
            colors: highlight.colors(node.computed.text.color, self.selection_colors),
        })
    }
    /// Set the UA paired colors. Author ::selection colors still take precedence.
    pub fn set_selection_colors(&mut self, colors: SelectionColors) {
        if self.selection_colors != colors {
            self.selection_colors = colors;
            self.selection.get_mut().paint_dirty = true;
        }
    }

    /// User select-all is limited by a containing selection scope or active modal.
    pub fn select_all(&mut self) -> bool {
        self.ensure_selection_index();
        let mut scope = self.active_modal().or(self.root());
        if let Some(selection) = self.selection() {
            let mut current = Some(selection.focus.node());
            while let Some(id) = current {
                if self.used_select(id) == UserSelect::Contain {
                    scope = Some(id);
                    break;
                }
                current = self.nodes[id].parent;
            }
        }
        let Some(scope) = scope else {
            return false;
        };
        let range = self.contents(scope);
        let state = self.selection.get_mut();
        state.drag = None;
        state.set_user(range).unwrap_or(false)
    }
}

impl WidgetTree {
    pub(crate) fn selection_before_insert(&mut self, parent: WidgetId, index: usize) {
        let state = self.selection.get_mut();
        if let Some(mut range) = state.range {
            let adjust = |point: &mut SelectionPoint| {
                if let SelectionPoint::Children {
                    node,
                    index: offset,
                } = point
                    && *node == parent
                    && *offset > index
                {
                    *offset += 1;
                }
            };
            adjust(&mut range.anchor);
            adjust(&mut range.focus);
            state.set(Some(range), state.programmatic);
        }
        state.invalidate(true);
    }
    pub(crate) fn selection_before_remove(&mut self, id: WidgetId) {
        if self.selection().is_none() {
            self.selection.get_mut().invalidate(true);
            return;
        }
        let Some(parent) = self.nodes[id].parent else {
            self.selection.get_mut().clear();
            return;
        };
        let boundary = self.before(id);
        let SelectionPoint::Children { index, .. } = boundary else {
            unreachable!()
        };
        self.selection_before_remove_at(id, parent, index);
    }
    /// A bulk sibling removal supplies original indices in descending order.
    /// This preserves live boundaries without searching the child list per node.
    pub(crate) fn selection_before_remove_at(
        &mut self,
        id: WidgetId,
        parent: WidgetId,
        index: usize,
    ) {
        let boundary = SelectionPoint::children(parent, index);
        let old = self.selection();
        let adjusted = old.map(|range| {
            let adjust = |point: SelectionPoint| {
                if self.within(point, id) {
                    boundary
                } else {
                    match point {
                        SelectionPoint::Children {
                            node,
                            index: offset,
                        } if node == parent && offset > index => {
                            SelectionPoint::children(node, offset - 1)
                        }
                        _ => point,
                    }
                }
            };
            Selection {
                anchor: adjust(range.anchor),
                focus: adjust(range.focus),
            }
        });
        let state = self.selection.get_mut();
        state.drag = None;
        state.set(adjusted, state.programmatic);
        state.invalidate(true);
    }
    /// Replace character data and update live selection boundaries before changing
    /// layout. UI offsets stay UTF-8/grapheme-safe; programmatic points may be any char boundary.
    pub fn replace_text(
        &mut self,
        id: WidgetId,
        range: Range<usize>,
        replacement: &str,
    ) -> anyhow::Result<bool> {
        let widget = self
            .nodes
            .get_mut(id)
            .and_then(|n| n.widget.as_mut())
            .ok_or_else(|| anyhow::anyhow!("stale text element"))?;
        let text = widget
            .text_content()
            .ok_or_else(|| anyhow::anyhow!("element has no text"))?;
        anyhow::ensure!(
            range.start <= range.end
                && range.end <= text.len()
                && text.is_char_boundary(range.start)
                && text.is_char_boundary(range.end),
            "invalid text replacement range"
        );
        if &text[range.clone()] == replacement {
            return Ok(false);
        }
        let content = format!(
            "{}{}{}",
            &text[..range.start],
            replacement,
            &text[range.end..]
        );
        anyhow::ensure!(
            widget.set_text_content(content.into()),
            "widget does not support replacing text"
        );
        self.selection_after_text_change(id, range, replacement.len());
        Ok(true)
    }
    /// Share boundary adjustment with declarative text replacement. The next
    /// widget already owns its text, so reconciliation need not copy it again.
    pub(crate) fn selection_after_text_change(
        &mut self,
        id: WidgetId,
        range: Range<usize>,
        replacement_len: usize,
    ) {
        let state = self.selection.get_mut();
        if let Some(mut selection) = state.range {
            let adjust = |point: &mut SelectionPoint| {
                if let SelectionPoint::Text { node, byte } = point
                    && *node == id
                {
                    if *byte > range.end {
                        *byte = *byte - (range.end - range.start) + replacement_len;
                    } else if *byte > range.start {
                        *byte = range.start;
                    }
                }
            };
            adjust(&mut selection.anchor);
            adjust(&mut selection.focus);
            state.set(Some(selection), state.programmatic);
        }
        state.drag = None;
        state.invalidate(true);
        self.layout_ready = false;
        self.css_dirty = true;
    }
    pub(crate) fn take_selection_paint_dirty(&mut self) -> bool {
        std::mem::take(&mut self.selection.get_mut().paint_dirty)
    }
}

/// Native gesture policy. CSS does not prescribe double-click timing or distance;
/// applications can adjust these UA defaults without affecting selection rules.
#[derive(Debug, Clone, Copy)]
pub struct SelectionOptions {
    pub multi_click_interval: std::time::Duration,
    pub multi_click_distance: f32,
    pub colors: SelectionColors,
}
impl Default for SelectionOptions {
    fn default() -> Self {
        Self {
            multi_click_interval: std::time::Duration::from_millis(500),
            multi_click_distance: 4.0,
            colors: Default::default(),
        }
    }
}
#[derive(Default)]
pub(crate) struct SelectionInput {
    last_click: Option<(std::time::Instant, Point<f32>, u8)>,
}
impl SelectionInput {
    pub fn clicks(
        &mut self,
        now: std::time::Instant,
        point: Point<f32>,
        options: SelectionOptions,
    ) -> u8 {
        let count = self
            .last_click
            .filter(|(time, previous, _)| {
                now.saturating_duration_since(*time) <= options.multi_click_interval
                    && (point.x - previous.x).hypot(point.y - previous.y)
                        <= options.multi_click_distance
            })
            .map(|(_, _, count)| (count + 1).min(3))
            .unwrap_or(1);
        self.last_click = Some((now, point, count));
        count
    }
}
