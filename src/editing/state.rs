//! Editing sessions, transient IME preedit, and bounded delta history.
use super::document::boundary_after;
use super::extensions::Anchors;
use super::highlights::{self, Highlights};
use super::{Anchor, AnchorId, Projection, ProjectionSnapshot};
use super::{
    Bias,
    format::{self, Format, StylePatch},
};
use super::{ChangeSet, Document, Edit, EditError, Selection, SelectionSet};
use crate::core::rich_text::{InlineStyle, RichText, StyleSpan};
use std::{
    borrow::Cow,
    collections::VecDeque,
    ops::Range,
    rc::{Rc, Weak},
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EditKind {
    Typing,
    Delete,
    Paste,
    Composition,
    #[default]
    Command,
    Undo,
    Redo,
}
#[derive(Debug, Clone)]
pub struct Transaction {
    pub revision: u64,
    pub edits: Vec<Edit>,
    pub formats: Vec<Format>,
    /// Selection in the resulting document. Omit to map the existing selection.
    pub selection: Option<SelectionSet>,
    pub kind: EditKind,
}
impl Transaction {
    pub fn new(revision: u64, edits: impl IntoIterator<Item = Edit>) -> Self {
        Self {
            revision,
            edits: edits.into_iter().collect(),
            formats: Vec::new(),
            selection: None,
            kind: EditKind::Command,
        }
    }
    /// Add undoable document formatting in output coordinates. Patches run in order.
    /// Use `EditorState::set_highlights` for automatically derived display styles.
    pub fn format(mut self, range: Range<usize>, patch: StylePatch) -> Self {
        self.formats.push(Format { range, patch });
        self
    }
}
#[derive(Debug, Clone)]
pub struct Change {
    pub before_revision: u64,
    pub revision: u64,
    pub changes: ChangeSet,
    pub kind: EditKind,
}
#[derive(Debug, Clone, Copy)]
pub struct HistoryOptions {
    pub max_bytes: usize,
    pub merge_interval: Duration,
}
impl Default for HistoryOptions {
    fn default() -> Self {
        Self {
            max_bytes: 1024 * 1024,
            merge_interval: Duration::from_millis(500),
        }
    }
}
#[derive(Debug)]
struct Record {
    forward: ChangeSet,
    inverse: ChangeSet,
}
#[derive(Debug)]
struct HistoryEntry {
    records: Vec<Record>,
    before: SelectionSet,
    after: SelectionSet,
    before_typing: Option<Box<InlineStyle>>,
    after_typing: Option<Box<InlineStyle>>,
    kind: EditKind,
    time: Instant,
    bytes: usize,
}
impl HistoryEntry {
    /// Visit deltas in application order without allocating an iterator.
    fn changes(&self, redo: bool) -> impl Iterator<Item = &ChangeSet> {
        (0..self.records.len()).map(move |index| {
            let index = if redo {
                index
            } else {
                self.records.len() - index - 1
            };
            let record = &self.records[index];
            if redo {
                &record.forward
            } else {
                &record.inverse
            }
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Composition {
    /// Range in the committed document, unchanged until commit.
    pub range: Range<usize>,
    pub text: String,
    /// UTF-8 offsets within preedit. None asks the view to hide its caret.
    pub cursor: Option<(usize, usize)>,
    /// Input style frozen for the lifetime of this preedit.
    pub style: InlineStyle,
}

/// Keep the returned guard alive while this document invariant is required.
/// Constraints validate transactions before mutation and never run for preedit.
pub type EditConstraint = dyn Fn(&Document, &ChangeSet) -> Result<(), EditError>;
#[derive(Debug, Default)]
struct ExtensionState {
    projection: Option<Box<(u64, Projection)>>,
    anchors: Option<Box<Anchors>>,
    last_change: Option<Box<Change>>,
}
#[derive(Debug)]
pub struct EditorState {
    constraints: Vec<Weak<EditConstraint>>,
    document: Document,
    selection: SelectionSet,
    composition: Option<Box<Composition>>,
    typing_style: Option<Box<InlineStyle>>,
    highlights: Option<Box<Highlights>>,
    extensions: Option<Box<ExtensionState>>,
    generation: u64,
    undo: VecDeque<HistoryEntry>,
    redo: Vec<HistoryEntry>,
    history_bytes: usize,
    history: HistoryOptions,
    history_boundary: bool,
}
impl EditorState {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            constraints: Vec::new(),
            document: Document::new(text),
            selection: Default::default(),
            composition: None,
            typing_style: None,
            highlights: None,
            extensions: None,
            generation: 0,
            undo: VecDeque::new(),
            redo: Vec::new(),
            history_bytes: 0,
            history: Default::default(),
            history_boundary: true,
        }
    }
    pub fn from_document(document: Document) -> Self {
        let mut state = Self::new("");
        state.document = document;
        state
    }
    pub fn from_rich(content: impl Into<RichText>) -> Self {
        let mut state = Self::new("");
        state.document = Document::from_rich(content);
        state
    }
    /// Pending overrides affect subsequent typing and IME commits. None derives
    /// formatting independently at each selection. Moving selection clears them.
    /// This is session state, not a document edit or standalone undo entry.
    pub fn typing_style(&self) -> Option<&InlineStyle> {
        self.typing_style.as_deref()
    }
    pub fn set_typing_style(&mut self, style: Option<InlineStyle>) -> Result<bool, EditError> {
        if self.composition.is_some() {
            return Err(EditError::CompositionActive);
        }
        if let Some(style) = &style {
            format::validate_style(style)?;
        }
        if style.as_ref() == self.typing_style.as_deref() {
            return Ok(false);
        }
        self.typing_style = style.map(Box::new);
        self.break_history_group();
        self.generation += 1;
        Ok(true)
    }
    fn input_style(&self, selection: Selection) -> InlineStyle {
        self.typing_style.as_deref().cloned().unwrap_or_else(|| {
            self.document
                .style_at(
                    selection.text_range().start,
                    if selection.is_caret()
                        && selection.affinity == Bias::After
                        && selection.head > 0
                    {
                        Bias::Before
                    } else {
                        Bias::After
                    },
                )
                .unwrap()
        })
    }
    /// Apply one undoable user-formatting patch to every nonempty selection.
    /// Automatic syntax styles belong in `set_highlights`, not this transaction.
    /// With only carets, update pending input formatting using the primary caret.
    pub fn format_selections(&mut self, patch: StylePatch) -> Result<Option<Change>, EditError> {
        if self.composition.is_some() {
            return Err(EditError::CompositionActive);
        }
        let mut transaction = Transaction::new(self.revision(), []);
        for selection in self.selection.iter().filter(|s| !s.is_caret()) {
            transaction.formats.push(Format {
                range: selection.text_range(),
                patch: patch.clone(),
            });
        }
        if transaction.formats.is_empty() {
            let mut style = self.input_style(self.selection.primary());
            patch.apply(&mut style);
            self.set_typing_style(Some(style))?;
            return Ok(None);
        }
        self.transact(transaction)
    }
    pub fn projection_revision(&self) -> u64 {
        self.extensions
            .as_ref()
            .and_then(|e| e.projection.as_ref())
            .map_or(0, |p| p.0)
    }
    pub fn projection(&self) -> Projection {
        self.extensions
            .as_ref()
            .and_then(|e| e.projection.as_ref())
            .map(|p| p.1.clone())
            .unwrap_or_default()
    }
    /// Publish a display plan for a committed revision. Like highlighting this
    /// never enters history; text changes clear a plan until it is recomputed.
    pub fn set_projection(
        &mut self,
        revision: u64,
        mut projection: Projection,
    ) -> Result<bool, EditError> {
        if revision != self.revision() {
            return Err(EditError::StaleRevision {
                expected: revision,
                actual: self.revision(),
            });
        }
        projection.validate(&self.document)?;
        if self.projection() == projection {
            return Ok(false);
        }
        self.generation += 1;
        self.extensions
            .get_or_insert_with(Default::default)
            .projection = Some(Box::new((self.generation, projection)));
        Ok(true)
    }
    pub fn snapshot(&self) -> ProjectionSnapshot {
        ProjectionSnapshot {
            composing: false,
            document_id: self.document.id(),
            revision: self.revision(),
            change: self.last_change().cloned().map(std::sync::Arc::new),
            text: self.document.snapshot(),
            spans: self.document.style_snapshot(),
            selections: self.selections().clone(),
        }
    }
    /// UI snapshot uses composed source coordinates, while extension analysis
    /// always receives `snapshot()` in committed coordinates.
    pub(crate) fn display_snapshot(&self) -> ProjectionSnapshot {
        let mut snapshot = self.snapshot();
        if let Some(c) = self.composition() {
            // Keep document identity for scroll anchors. The explicit transient
            // flag disables index reuse without turning preedit into a new note.
            snapshot.composing = true;
            snapshot.change = None;
            snapshot.text.replace(c.range.clone(), &c.text);
        }
        if self.highlights.is_some() || self.composition.is_some() {
            snapshot.spans = self.display_spans().into_owned().into();
        }
        snapshot
    }
    pub fn last_change(&self) -> Option<&Change> {
        self.extensions
            .as_ref()
            .and_then(|e| e.last_change.as_deref())
    }
    pub fn create_anchor(&mut self, byte: usize, bias: Bias) -> Result<AnchorId, EditError> {
        if !self.document.is_char_boundary(byte) {
            return Err(EditError::InvalidSelection);
        }
        let anchors = self
            .extensions
            .get_or_insert_with(Default::default)
            .anchors
            .get_or_insert_with(Default::default);
        let id = AnchorId(anchors.next);
        anchors.next = anchors.next.checked_add(1).expect("anchor IDs exhausted");
        anchors.positions.insert(id, Anchor { byte, bias });
        Ok(id)
    }
    pub fn anchor(&self, id: AnchorId) -> Option<Anchor> {
        self.extensions
            .as_ref()?
            .anchors
            .as_ref()?
            .positions
            .get(&id)
            .copied()
    }
    pub fn remove_anchor(&mut self, id: AnchorId) -> bool {
        self.extensions
            .as_mut()
            .and_then(|e| e.anchors.as_mut())
            .is_some_and(|a| a.positions.remove(&id).is_some())
    }
    /// Replace automatic styling for a committed document revision. Highlight
    /// updates never enter history, clear redo, change selection, or split typing
    /// groups. Late asynchronous results are rejected without changing the view.
    /// Spans must not overlap; unset fields expose the committed document style.
    pub fn set_highlights(
        &mut self,
        revision: u64,
        spans: impl IntoIterator<Item = StyleSpan>,
    ) -> Result<bool, EditError> {
        if revision != self.revision() {
            return Err(EditError::StaleRevision {
                expected: revision,
                actual: self.revision(),
            });
        }
        let spans = highlights::canonicalize_source(&self.document, spans)?;
        if spans == self.highlights() {
            return Ok(false);
        }
        self.generation += 1;
        self.highlights = (!spans.is_empty()).then(|| {
            Box::new(Highlights {
                spans,
                generation: self.generation,
            })
        });
        Ok(true)
    }
    /// Remove derived styling, revealing user formatting without a document edit.
    pub fn clear_highlights(&mut self) -> bool {
        if self.highlights.take().is_none() {
            return false;
        }
        self.generation += 1;
        true
    }
    pub fn highlights(&self) -> &[StyleSpan] {
        self.highlights.as_ref().map_or(&[], |h| &h.spans)
    }
    /// Display cache key for derived styles. Selection-only changes leave it fixed.
    pub fn highlight_revision(&self) -> u64 {
        self.highlights.as_ref().map_or(0, |h| h.generation)
    }
    /// Styles in the composed UTF-8 display string. With no highlights or preedit,
    /// this borrows document spans. Derived styles never enter rich-text exports.
    pub fn display_spans(&self) -> Cow<'_, [StyleSpan]> {
        let spans = if let Some(highlights) = &self.highlights {
            Cow::Owned(highlights::overlay(
                self.document.spans(),
                &highlights.spans,
            ))
        } else {
            Cow::Borrowed(self.document.spans())
        };
        let Some(composition) = &self.composition else {
            return spans;
        };
        let edit = Edit::new(composition.range.clone(), &composition.text);
        let mapped = format::map_spans(&spans, &[edit]);
        Cow::Owned(format::patch(
            &mapped,
            composition.range.start..composition.range.start + composition.text.len(),
            &StylePatch::replace(composition.style.clone()),
        ))
    }
    /// Text edits invalidate derived analysis, including undo/redo. Formatting
    /// alone keeps highlight coordinates valid. The caller recomputes highlighting
    /// for the new revision instead of painting stale tokens at mapped positions.
    fn apply_document_change(&mut self, changes: &ChangeSet, kind: EditKind) {
        if !changes.edits().is_empty() {
            self.highlights = None;
            if let Some(extra) = &mut self.extensions {
                extra.projection = None;
            }
        }
        if let Some(anchors) = self.extensions.as_mut().and_then(|e| e.anchors.as_mut()) {
            anchors.map(changes);
        }
        let before_revision = self.revision();
        self.document.apply(changes);
        self.extensions
            .get_or_insert_with(Default::default)
            .last_change = Some(Box::new(Change {
            before_revision,
            revision: self.revision(),
            changes: changes.clone(),
            kind,
        }));
    }
    pub fn constrain(
        &mut self,
        check: impl Fn(&Document, &ChangeSet) -> Result<(), EditError> + 'static,
    ) -> Rc<EditConstraint> {
        let check: Rc<EditConstraint> = Rc::new(check);
        self.constraints.retain(|c| c.strong_count() > 0);
        self.constraints.push(Rc::downgrade(&check));
        check
    }
    pub fn document(&self) -> &Document {
        &self.document
    }
    pub fn text(&self) -> &str {
        self.document.text()
    }
    pub fn revision(&self) -> u64 {
        self.document.revision()
    }
    /// Changes on selection/preedit updates as well as document transactions.
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn selections(&self) -> &SelectionSet {
        &self.selection
    }
    pub fn composition(&self) -> Option<&Composition> {
        self.composition.as_deref()
    }
    pub fn history_bytes(&self) -> usize {
        self.history_bytes
    }
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
    pub fn set_history_options(&mut self, options: HistoryOptions) {
        self.history = options;
        self.trim_history();
        if options.max_bytes == 0 {
            self.undo.shrink_to_fit();
            self.redo.shrink_to_fit();
        }
    }
    pub fn break_history_group(&mut self) {
        self.history_boundary = true;
    }
    pub fn clear_history(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.history_bytes = 0;
        self.history_boundary = true;
    }
    pub fn select(&mut self, selection: SelectionSet) -> Result<bool, EditError> {
        if self.composition.is_some() {
            return Err(EditError::CompositionActive);
        }
        if !selection.valid(&self.document) {
            return Err(EditError::InvalidSelection);
        }
        if selection == self.selection {
            return Ok(false);
        }
        self.selection = selection;
        self.typing_style = None;
        self.history_boundary = true;
        self.generation += 1;
        Ok(true)
    }
    pub fn select_all(&mut self) -> Result<bool, EditError> {
        self.select(SelectionSet::single(Selection::range(
            0,
            self.document.len(),
        )))
    }
    pub fn selected_text(&self) -> String {
        let mut out = String::new();
        for selection in self.selection.iter().filter(|s| !s.is_caret()) {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(self.document.read(selection.text_range()).unwrap().as_ref());
        }
        out
    }
    pub fn replace_selections(
        &mut self,
        text: &str,
        kind: EditKind,
    ) -> Result<Option<Change>, EditError> {
        if self.selection.len() == 1 {
            let range = self.selection.primary().text_range();
            let selection = SelectionSet::single(Selection::caret(range.start + text.len()));
            return self.transact(Transaction {
                revision: self.revision(),
                edits: vec![Edit::styled(
                    range,
                    text,
                    self.input_style(self.selection.primary()),
                )],
                formats: Vec::new(),
                selection: Some(selection),
                kind,
            });
        }
        let mut removed = 0;
        let mut added = 0;
        let mut edits = Vec::with_capacity(self.selection.len());
        let ranges = self.selection.iter().map(|s| {
            let range = s.text_range();
            let head = range.start - removed + added + text.len();
            removed += range.len();
            added += text.len();
            edits.push(Edit::styled(range, text, self.input_style(*s)));
            Selection::caret(head)
        });
        let selection = SelectionSet::new(ranges, self.selection.primary_index())?;
        self.transact(Transaction {
            revision: self.revision(),
            edits,
            formats: Vec::new(),
            selection: Some(selection),
            kind,
        })
    }

    /// Replace all selections with an exact fragment in one history operation.
    pub fn replace_selections_rich(
        &mut self,
        content: &RichText,
        kind: EditKind,
    ) -> Result<Option<Change>, EditError> {
        let mut transaction = Transaction::new(self.revision(), []);
        let (mut removed, mut added) = (0, 0);
        let selections = self.selection.iter().map(|s| {
            let range = s.text_range();
            let head = range.start - removed + added + content.text().len();
            removed += range.len();
            added += content.text().len();
            transaction.edits.push(Edit::rich(range, content.clone()));
            Selection::caret(head)
        });
        transaction.selection = Some(SelectionSet::new(
            selections,
            self.selection.primary_index(),
        )?);
        transaction.kind = kind;
        self.transact(transaction)
    }

    pub fn transact(&mut self, transaction: Transaction) -> Result<Option<Change>, EditError> {
        self.transact_at(transaction, Instant::now())
    }
    pub fn transact_at(
        &mut self,
        transaction: Transaction,
        now: Instant,
    ) -> Result<Option<Change>, EditError> {
        if self.composition.is_some() {
            return Err(EditError::CompositionActive);
        }
        let changes =
            self.document
                .prepare(transaction.revision, transaction.edits, transaction.formats)?;
        for check in self.constraints.iter().filter_map(Weak::upgrade) {
            check(&self.document, &changes)?;
        }
        let selection = transaction.selection.unwrap_or_else(|| {
            // Formatting cannot move a caret or erase its preferred vertical x.
            if changes.edits().is_empty() {
                self.selection.clone()
            } else {
                self.selection.map(&changes)
            }
        });
        // Validate output positions against the virtual edit pieces before writing
        // anything. A malformed transaction cannot partially mutate the document.
        if !selection.iter().all(|s| {
            boundary_after(&self.document, &changes, s.anchor)
                && boundary_after(&self.document, &changes, s.head)
                && s.preferred_x.is_none_or(f32::is_finite)
        }) {
            return Err(EditError::InvalidSelection);
        }
        if changes.is_empty() {
            self.select(selection)?;
            return Ok(None);
        }
        if self.history.max_bytes == 0 {
            let before_revision = self.revision();
            self.apply_document_change(&changes, transaction.kind);
            self.selection = selection;
            self.generation += 1;
            return Ok(Some(Change {
                before_revision,
                revision: self.revision(),
                changes,
                kind: transaction.kind,
            }));
        }
        let inverse = self.document.inverse(&changes);
        let before_revision = self.revision();
        self.apply_document_change(&changes, transaction.kind);
        let before = std::mem::replace(&mut self.selection, selection);
        self.generation += 1;
        let record = Record {
            forward: changes.clone(),
            inverse,
        };
        let bytes = record.forward.retained_bytes()
            + record.inverse.retained_bytes()
            + std::mem::size_of::<Record>();
        self.history_bytes -= self.redo.iter().map(|entry| entry.bytes).sum::<usize>();
        self.redo.clear();
        let merge = !self.history_boundary
            && matches!(transaction.kind, EditKind::Typing | EditKind::Delete)
            && self.undo.back().is_some_and(|entry| {
                entry.kind == transaction.kind
                    && entry.after == before
                    && now.saturating_duration_since(entry.time) <= self.history.merge_interval
            });
        if merge {
            let entry = self.undo.back_mut().unwrap();
            let old_capacity = entry.records.capacity();
            let old_selection_bytes = entry.after.retained_bytes();
            entry.records.push(record);
            entry.after = self.selection.clone();
            entry.time = now;
            let bytes = bytes - std::mem::size_of::<Record>()
                + (entry.records.capacity() - old_capacity) * std::mem::size_of::<Record>();
            entry.bytes = entry.bytes - old_selection_bytes + entry.after.retained_bytes() + bytes;
            self.history_bytes =
                self.history_bytes - old_selection_bytes + entry.after.retained_bytes() + bytes;
        } else {
            let bytes = bytes
                + std::mem::size_of::<HistoryEntry>()
                + before.retained_bytes()
                + self.selection.retained_bytes()
                + self.typing_style.as_deref().map_or(0, |s| {
                    std::mem::size_of::<InlineStyle>() + format::retained_style_bytes(s)
                }) * 2;
            self.undo.push_back(HistoryEntry {
                records: vec![record],
                before,
                before_typing: self.typing_style.clone(),
                after_typing: self.typing_style.clone(),
                after: self.selection.clone(),
                kind: transaction.kind,
                time: now,
                bytes,
            });
            self.history_bytes += bytes;
        }
        self.history_boundary = false;
        self.trim_history();
        Ok(Some(Change {
            before_revision,
            revision: self.revision(),
            changes,
            kind: transaction.kind,
        }))
    }
    fn trim_history(&mut self) {
        while self.history_bytes > self.history.max_bytes {
            let Some(entry) = self.undo.pop_front() else {
                break;
            };
            self.history_bytes -= entry.bytes;
        }
        // Redo is a stack: its first entries are the furthest future edits.
        // Drain a discarded prefix once instead of shifting the vector for each
        // entry. This keeps trimming linear without increasing every session's
        // storage for the ordinary push/pop path.
        let mut discarded = 0;
        for entry in &self.redo {
            if self.history_bytes <= self.history.max_bytes {
                break;
            }
            self.history_bytes -= entry.bytes;
            discarded += 1;
        }
        self.redo.drain(..discarded);
    }
    /// Undo/redo return each delta in execution order so external decorations can
    /// map their positions without diffing whole document snapshots.
    pub fn undo(&mut self) -> Result<Vec<Change>, EditError> {
        self.restore(false)
    }
    pub fn redo(&mut self) -> Result<Vec<Change>, EditError> {
        self.restore(true)
    }
    fn restore(&mut self, redo: bool) -> Result<Vec<Change>, EditError> {
        let entry = if redo {
            self.redo.pop()
        } else {
            self.undo.pop_back()
        };
        let Some(entry) = entry else {
            self.cancel_composition();
            return Ok(Vec::new());
        };
        // A constraint can be attached after history was recorded. Validate the
        // whole group before restoring anything; rejected undo retains its entry.
        let constraints: Vec<_> = self.constraints.iter().filter_map(Weak::upgrade).collect();
        if !constraints.is_empty() {
            let mut probe = self.document.history_probe();
            let valid = entry.changes(redo).try_for_each(|changes| {
                for check in &constraints {
                    check(&probe, changes)?;
                }
                probe.apply(changes);
                Ok::<_, EditError>(())
            });
            if let Err(error) = valid {
                if redo {
                    self.redo.push(entry);
                } else {
                    self.undo.push_back(entry);
                }
                return Err(error);
            }
        }
        self.cancel_composition();
        let mut result = Vec::with_capacity(entry.records.len());
        for changes in entry.changes(redo) {
            let before_revision = self.revision();
            self.apply_document_change(changes, if redo { EditKind::Redo } else { EditKind::Undo });
            result.push(Change {
                before_revision,
                revision: self.revision(),
                changes: changes.clone(),
                kind: if redo { EditKind::Redo } else { EditKind::Undo },
            });
        }
        self.typing_style = if redo {
            entry.after_typing.clone()
        } else {
            entry.before_typing.clone()
        };
        self.selection = if redo {
            entry.after.clone()
        } else {
            entry.before.clone()
        };
        if redo {
            self.undo.push_back(entry);
        } else {
            self.redo.push(entry);
        }
        self.generation += 1;
        self.history_boundary = true;
        Ok(result)
    }
    pub fn set_composition(
        &mut self,
        text: &str,
        cursor: Option<(usize, usize)>,
    ) -> Result<bool, EditError> {
        if let Some((a, b)) = cursor {
            if !text.is_char_boundary(a) || !text.is_char_boundary(b) {
                return Err(EditError::InvalidSelection);
            }
        }
        let composition = Composition {
            range: self.selection.primary().text_range(),
            text: text.into(),
            cursor,
            style: self
                .composition
                .as_ref()
                .map(|c| c.style.clone())
                .unwrap_or_else(|| self.input_style(self.selection.primary())),
        };
        if self.composition.as_deref() == Some(&composition) {
            return Ok(false);
        }
        self.composition = Some(Box::new(composition));
        self.generation += 1;
        Ok(true)
    }
    pub fn cancel_composition(&mut self) -> bool {
        if self.composition.take().is_none() {
            return false;
        }
        self.generation += 1;
        true
    }
    /// Commit an extension-authored transaction while closing native preedit.
    /// The transaction uses committed source coordinates, just like snapshot().
    /// Rejection restores composition and history grouping without losing input.
    pub fn commit_transaction(
        &mut self,
        mut transaction: Transaction,
    ) -> Result<Option<Change>, EditError> {
        transaction.kind = EditKind::Composition;
        self.finish_composition(|state| state.transact(transaction))
    }
    pub fn commit_composition(&mut self, text: &str) -> Result<Option<Change>, EditError> {
        self.finish_composition(|state| state.replace_selections(text, EditKind::Composition))
    }
    fn finish_composition(
        &mut self,
        commit: impl FnOnce(&mut Self) -> Result<Option<Change>, EditError>,
    ) -> Result<Option<Change>, EditError> {
        // Temporarily remove preedit so the same transaction validator can run.
        // Rejection restores preedit without changing generation, history or text.
        let composition = self.composition.take();
        let boundary = self.history_boundary;
        self.break_history_group();
        let result = commit(self);
        if result.is_err() {
            self.composition = composition;
            self.history_boundary = boundary;
        } else {
            if composition.is_some() {
                self.generation += 1;
            }
            self.break_history_group();
        }
        result
    }
}


#[cfg(test)]
mod history_tests {
    use super::*;

    fn three_commands() -> EditorState {
        let mut state = EditorState::new("");
        for text in ["a", "b", "c"] {
            state.replace_selections(text, EditKind::Command).unwrap();
        }
        state
    }

    #[test]
    fn trimming_redo_retains_the_nearest_future_entries() {
        let mut state = three_commands();
        for _ in 0..3 {
            state.undo().unwrap();
        }
        // The stack is [c, b, a]. Keep exactly the two nearest redo entries,
        // using their actual accounting rather than assuming allocator sizes.
        let budget = state
            .redo
            .iter()
            .rev()
            .take(2)
            .map(|entry| entry.bytes)
            .sum();
        state.set_history_options(HistoryOptions {
            max_bytes: budget,
            ..Default::default()
        });
        assert_eq!(state.history_bytes(), budget);
        state.redo().unwrap();
        assert_eq!(state.text(), "a");
        state.redo().unwrap();
        assert_eq!(state.text(), "ab");
        assert!(!state.can_redo());
        state.undo().unwrap();
        state.undo().unwrap();
        assert_eq!(state.text(), "");
    }

    #[test]
    fn trimming_mixed_history_preserves_a_contiguous_chain() {
        let mut state = three_commands();
        state.undo().unwrap();
        let budget = state.history_bytes() - state.undo.front().unwrap().bytes;
        state.set_history_options(HistoryOptions {
            max_bytes: budget,
            ..Default::default()
        });
        state.undo().unwrap();
        assert_eq!(state.text(), "a");
        assert!(!state.can_undo());
        state.redo().unwrap();
        state.redo().unwrap();
        assert_eq!(state.text(), "abc");
        assert!(!state.can_redo());
    }

    #[test]
    fn disabling_history_discards_a_large_redo_stack() {
        let mut state = EditorState::new("");
        for _ in 0..1000 {
            state.replace_selections("x", EditKind::Command).unwrap();
        }
        while state.can_undo() {
            state.undo().unwrap();
        }
        let text = state.text().to_owned();
        state.set_history_options(HistoryOptions {
            max_bytes: 0,
            ..Default::default()
        });
        assert_eq!(state.history_bytes(), 0);
        assert!(!state.can_undo());
        assert!(!state.can_redo());
        assert_eq!(state.text(), text);
        assert_eq!(state.undo.capacity(), 0);
        assert_eq!(state.redo.capacity(), 0);
    }
}
