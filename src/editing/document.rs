//! Versioned UTF-8 text and atomic edits. Offsets always address the input
//! revision; callers never need to adjust later edits after an earlier insertion.
use super::format::{self, Format, StyleChange};
use super::{BufferOptions, TextBuffer, TextRead, TextSnapshot};
use crate::core::rich_text::{InlineStyle, RichText, StyleSpan};
use std::ops::Range;

#[derive(Debug, Clone, PartialEq)]
pub struct Edit {
    pub range: Range<usize>,
    pub insert: String,
    // None inherits insertion-point formatting; Some (including empty) is exact.
    spans: Option<Vec<StyleSpan>>,
}
impl Edit {
    pub fn new(range: Range<usize>, insert: impl Into<String>) -> Self {
        Self {
            range,
            insert: insert.into(),
            spans: None,
        }
    }
    /// Insert an exact rich fragment. Unstyled gaps explicitly use base typography.
    pub fn rich(range: Range<usize>, content: impl Into<RichText>) -> Self {
        let content = content.into();
        Self {
            range,
            insert: content.text().into(),
            spans: Some(content.spans().to_vec()),
        }
    }
    /// Insert with an exact style, including explicitly unformatted text.
    /// Public style fields are validated when the transaction is submitted.
    pub fn styled(range: Range<usize>, text: impl Into<String>, style: InlineStyle) -> Self {
        let insert = text.into();
        let len = insert.len();
        let spans = if style == InlineStyle::default() {
            Vec::new()
        } else {
            vec![StyleSpan {
                range: 0..len,
                style,
            }]
        };
        Self {
            range,
            insert,
            spans: Some(spans),
        }
    }
    pub fn spans(&self) -> Option<&[StyleSpan]> {
        self.spans.as_deref()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Bias {
    Before,
    #[default]
    After,
}

/// A validated, sorted set of changes. Also maps decorations and selections
/// across a transaction. Positions inside replaced text map to the chosen edge.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ChangeSet {
    edits: Vec<Edit>,
    styles: Vec<StyleChange>,
}
impl ChangeSet {
    pub fn edits(&self) -> &[Edit] {
        &self.edits
    }
    /// Exact style deltas use output coordinates; text edits use input coordinates.
    pub fn style_changes(&self) -> &[StyleChange] {
        &self.styles
    }
    pub fn is_empty(&self) -> bool {
        self.edits.is_empty() && self.styles.is_empty()
    }
    pub fn map(&self, position: usize, bias: Bias) -> usize {
        let mut removed = 0;
        let mut added = 0;
        for edit in &self.edits {
            if position < edit.range.start {
                break;
            }
            // Undoing adjacent deletions can produce several insertions at the
            // same input position. After must pass all of them, not just the first.
            if position == edit.range.start && edit.range.is_empty() {
                if bias == Bias::Before {
                    return position - removed + added;
                }
                added += edit.insert.len();
                continue;
            }
            // The end of a nonempty replacement belongs to the following text.
            if position < edit.range.end {
                return edit.range.start - removed
                    + added
                    + if bias == Bias::After {
                        edit.insert.len()
                    } else {
                        0
                    };
            }
            removed += edit.range.len();
            added += edit.insert.len();
        }
        position - removed + added
    }
    pub fn map_range(&self, range: Range<usize>) -> Range<usize> {
        self.map(range.start, Bias::Before)..self.map(range.end, Bias::After)
    }
    pub(crate) fn retained_bytes(&self) -> usize {
        self.edits.capacity() * std::mem::size_of::<Edit>()
            + self
                .edits
                .iter()
                .map(|e| e.insert.capacity())
                .sum::<usize>()
            + self.styles.capacity() * std::mem::size_of::<StyleChange>()
            + self
                .styles
                .iter()
                .map(|s| format::retained_style_bytes(&s.style))
                .sum::<usize>()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditError {
    StaleRevision { expected: u64, actual: u64 },
    InvalidRange(Range<usize>),
    OverlappingEdits,
    InvalidSelection,
    CompositionActive,
    Rejected(&'static str),
}
impl std::fmt::Display for EditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for EditError {}

/// Compact contiguous storage suits short controls without allocating a rope
/// per input. Storage is private so a chunked implementation can be introduced
/// without changing transactions. Middle edits currently move the trailing bytes.
#[derive(Debug)]
pub struct Document {
    text: TextBuffer,
    spans: Option<std::sync::Arc<[StyleSpan]>>,
    id: u64,
    revision: u64,
}
impl Document {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: TextBuffer::new(text.into(), BufferOptions::default()),
            spans: None,
            id: next_document_id(),
            revision: 0,
        }
    }
    pub fn with_buffer_options(text: impl Into<String>, options: BufferOptions) -> Self {
        Self {
            text: TextBuffer::new(text.into(), options),
            spans: None,
            id: next_document_id(),
            revision: 0,
        }
    }
    pub fn from_rich(content: impl Into<RichText>) -> Self {
        let content = content.into();
        Self {
            text: TextBuffer::new(content.text().into(), BufferOptions::default()),
            spans: (!content.spans().is_empty()).then(|| std::sync::Arc::from(content.spans())),
            id: next_document_id(),
            revision: 0,
        }
    }
    pub fn id(&self) -> u64 {
        self.id
    }
    pub fn style_snapshot(&self) -> std::sync::Arc<[StyleSpan]> {
        static EMPTY: std::sync::OnceLock<std::sync::Arc<[StyleSpan]>> = std::sync::OnceLock::new();
        self.spans
            .clone()
            .unwrap_or_else(|| EMPTY.get_or_init(|| std::sync::Arc::from([])).clone())
    }
    pub fn snapshot(&self) -> TextSnapshot {
        self.text.snapshot()
    }
    pub fn buffer(&self) -> &TextBuffer {
        &self.text
    }
    pub fn read(&self, range: Range<usize>) -> Option<std::borrow::Cow<'_, str>> {
        self.text.read(range)
    }
    pub fn is_char_boundary(&self, p: usize) -> bool {
        self.text.is_char_boundary(p)
    }
    pub fn spans(&self) -> &[StyleSpan] {
        self.spans.as_deref().unwrap_or(&[])
    }
    /// Explicit snapshot; ordinary reads borrow `text()` and `spans()` instead.
    pub fn rich_text(&self) -> RichText {
        self.rich_slice(0..self.len())
            .expect("document spans are validated")
    }
    pub fn rich_slice(&self, range: Range<usize>) -> Option<RichText> {
        RichText::from_spans(
            self.text.read(range.clone())?.as_ref(),
            format::slice(self.spans(), range),
        )
        .ok()
    }
    /// Before selects the preceding character's style at a boundary. At zero,
    /// both biases select the following character; gaps have no overrides.
    pub fn style_at(&self, position: usize, bias: Bias) -> Option<InlineStyle> {
        self.text
            .is_char_boundary(position)
            .then(|| format::style_at(self.spans(), position, bias == Bias::Before))
    }
    pub fn text(&self) -> &str {
        self.text.as_str()
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn len(&self) -> usize {
        self.text.len()
    }
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }
    pub fn slice(&self, range: Range<usize>) -> Option<&str> {
        self.text.as_str().get(range)
    }
    pub fn capacity(&self) -> usize {
        self.text.capacity()
    }

    /// Constraints inspect each intermediate history revision without touching
    /// live state. Preserve the revision and copy the text only once.
    pub(crate) fn history_probe(&self) -> Self {
        Self {
            text: self.text.clone(),
            spans: self.spans.clone(),
            id: self.id,
            revision: self.revision,
        }
    }
    pub(crate) fn prepare(
        &self,
        revision: u64,
        mut edits: Vec<Edit>,
        formats: Vec<Format>,
    ) -> Result<ChangeSet, EditError> {
        if revision != self.revision {
            return Err(EditError::StaleRevision {
                expected: revision,
                actual: self.revision,
            });
        }
        edits.sort_by_key(|e| (e.range.start, e.range.end));
        for (i, edit) in edits.iter().enumerate() {
            if edit.range.start > edit.range.end
                || !self.text.is_char_boundary(edit.range.start)
                || !self.text.is_char_boundary(edit.range.end)
            {
                return Err(EditError::InvalidRange(edit.range.clone()));
            }
            if i > 0 {
                let previous = &edits[i - 1];
                if previous.range.end > edit.range.start || previous.range.start == edit.range.start
                {
                    return Err(EditError::OverlappingEdits);
                }
            }
        }
        for edit in &edits {
            if let Some(spans) = &edit.spans {
                crate::core::rich_text::validate_spans(&edit.insert, spans)
                    .map_err(|_| EditError::Rejected("invalid rich insertion"))?;
            }
        }
        if self.spans().is_empty()
            && formats.is_empty()
            && edits.iter().all(|edit| {
                edit.spans
                    .as_ref()
                    .is_none_or(|spans| spans.iter().all(|s| s.style == InlineStyle::default()))
            })
        {
            edits.retain(|e| !self.text.equals(e.range.clone(), &e.insert));
            for edit in &mut edits {
                edit.spans = None;
            }
            return Ok(ChangeSet {
                edits,
                styles: Vec::new(),
            });
        }
        // Build exact inserted intervals before consuming edits. Text-identical
        // rich replacements only change formatting and do not map selections.
        let mut inserted = Vec::new();
        let (mut removed, mut added) = (0, 0);
        for edit in &mut edits {
            let start = edit.range.start - removed + added;
            if edit.spans.is_some() || !self.text.equals(edit.range.clone(), &edit.insert) {
                let spans = edit.spans.take().unwrap_or_else(|| {
                    let inherited = self
                        .style_at(
                            edit.range.start,
                            if edit.range.is_empty() {
                                Bias::Before
                            } else {
                                Bias::After
                            },
                        )
                        .unwrap();
                    let mut spans = Vec::new();
                    format::push(&mut spans, 0..edit.insert.len(), inherited);
                    spans
                });
                let mut cursor = 0;
                for span in spans {
                    if cursor < span.range.start {
                        inserted.push(StyleChange {
                            range: start + cursor..start + span.range.start,
                            style: InlineStyle::default(),
                        });
                    }
                    if !span.range.is_empty() {
                        inserted.push(StyleChange {
                            range: start + span.range.start..start + span.range.end,
                            style: span.style,
                        });
                    }
                    cursor = span.range.end;
                }
                if cursor < edit.insert.len() {
                    inserted.push(StyleChange {
                        range: start + cursor..start + edit.insert.len(),
                        style: InlineStyle::default(),
                    });
                }
            }
            removed += edit.range.len();
            added += edit.insert.len();
        }
        edits.retain(|e| !self.text.equals(e.range.clone(), &e.insert));
        let mut changes = ChangeSet {
            edits,
            styles: Vec::new(),
        };
        let mapped = format::map_spans(self.spans(), &changes.edits);
        let mut styled = format::apply_changes(mapped.clone(), &inserted);
        for operation in &formats {
            if operation.range.start > operation.range.end
                || !boundary_after(self, &changes, operation.range.start)
                || !boundary_after(self, &changes, operation.range.end)
            {
                return Err(EditError::InvalidRange(operation.range.clone()));
            }
            let mut check = InlineStyle::default();
            operation.patch.apply(&mut check);
            format::validate_style(&check)?;
        }
        styled = format::apply_formats(styled, &formats);
        changes.styles = format::diff(&mapped, &styled);
        Ok(changes)
    }
    pub(crate) fn inverse(&self, changes: &ChangeSet) -> ChangeSet {
        let mut removed = 0;
        let mut added = 0;
        let edits: Vec<_> = changes
            .edits
            .iter()
            .map(|edit| {
                let start = edit.range.start - removed + added;
                removed += edit.range.len();
                added += edit.insert.len();
                Edit::new(
                    start..start + edit.insert.len(),
                    self.text.read(edit.range.clone()).unwrap().as_ref(),
                )
            })
            .collect();
        let after = format::apply_changes(
            format::map_spans(self.spans(), &changes.edits),
            &changes.styles,
        );
        let mapped_back = format::map_spans(&after, &edits);
        ChangeSet {
            edits,
            styles: format::diff(&mapped_back, self.spans()),
        }
    }
    pub(crate) fn apply(&mut self, changes: &ChangeSet) {
        if changes.is_empty() {
            return;
        }
        let spans = format::apply_changes(
            format::map_spans(self.spans(), &changes.edits),
            &changes.styles,
        );
        self.spans = (!spans.is_empty()).then(|| spans.into());
        // Descending edits preserve every input coordinate, including adjacent ranges.
        for edit in changes.edits.iter().rev() {
            self.text.replace(edit.range.clone(), &edit.insert);
        }
        self.revision = self
            .revision
            .checked_add(1)
            .expect("document revision exhausted");
    }
}

/// Check output UTF-8 boundaries against virtual edit pieces without copying the
/// document. Both text and formatting transactions validate before mutation.
pub(crate) fn boundary_after(
    text: &(impl TextRead + ?Sized),
    changes: &ChangeSet,
    position: usize,
) -> bool {
    let mut old = 0;
    let mut new = 0;
    for edit in changes.edits() {
        let unchanged = edit.range.start - old;
        if position < new + unchanged {
            return text.is_char_boundary(old + position - new);
        }
        new += unchanged;
        if position <= new + edit.insert.len() {
            return edit.insert.is_char_boundary(position - new);
        }
        new += edit.insert.len();
        old = edit.range.end;
    }
    position
        .checked_sub(new)
        .and_then(|p| old.checked_add(p))
        .is_some_and(|p| text.is_char_boundary(p))
}

impl TextRead for Document {
    fn len(&self) -> usize {
        self.text.len()
    }
    fn is_char_boundary(&self, p: usize) -> bool {
        self.text.is_char_boundary(p)
    }
    fn read(&self, r: Range<usize>) -> Option<std::borrow::Cow<'_, str>> {
        self.text.read(r)
    }
    fn chunk(&self, p: usize) -> (&str, usize) {
        self.text.chunk(p)
    }
}

impl Default for Document {
    fn default() -> Self {
        Self::new("")
    }
}
fn next_document_id() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}
