//! Revision-checked display plans. Source coordinates survive hiding, replacement
//! and inserted views. No projection operation changes document bytes or undo.
use super::{Bias, EditError, Selection, SelectionSet, TextRead, ViewDescription, ViewSpec};
use crate::StyleSpan;
use std::{ops::Range, sync::Arc};

/// Explicit registration identity, or an opaque layout-assigned widget handle.
/// Use a widget key when application identity must survive projection rebuilds.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ViewId(pub u64);
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EditBehavior {
    /// Navigation and deletion treat the represented source range as one unit.
    #[default]
    Atomic,
    /// Reveal the original source whenever a caret/selection touches the range.
    RevealOnSelection,
}
#[derive(Debug, Clone, PartialEq)]
pub enum ReplacementContent {
    Text(String),
    Object(ViewId),
    Widget(ViewSpec),
}
#[derive(Debug, Clone)]
pub struct Replacement {
    /// Explicit identity for text/registered objects. For described widgets the
    /// editor assigns this handle during validation and layout; do not persist it.
    pub id: ViewId,
    pub range: Range<usize>,
    pub content: ReplacementContent,
    pub behavior: EditBehavior,
}
impl Replacement {
    /// Replace source with a lazily mounted view. No ID registry is required.
    /// Equal descriptions at the same source range reuse the existing instance.
    pub fn widget(range: Range<usize>, description: impl ViewDescription) -> Self {
        Self {
            id: ViewId::default(),
            range,
            content: ReplacementContent::Widget(ViewSpec::new(description)),
            behavior: EditBehavior::Atomic,
        }
    }
    /// Preserve widget identity when it moves. Keys must be unique within the
    /// producing extension (or within the base projection). Only valid for widgets.
    pub fn key(mut self, key: impl Into<smol_str::SmolStr>) -> Self {
        let ReplacementContent::Widget(spec) = &mut self.content else {
            panic!("keys require a described widget");
        };
        spec.key = Some(key.into());
        self
    }
    pub fn hide(id: ViewId, range: Range<usize>) -> Self {
        Self::text(id, range, "")
    }
    pub fn text(id: ViewId, range: Range<usize>, text: impl Into<String>) -> Self {
        Self {
            id,
            range,
            content: ReplacementContent::Text(text.into()),
            behavior: EditBehavior::Atomic,
        }
    }
    pub fn object(id: ViewId, range: Range<usize>) -> Self {
        Self {
            id,
            range,
            content: ReplacementContent::Object(id),
            behavior: EditBehavior::Atomic,
        }
    }
    pub fn reveal_on_selection(mut self) -> Self {
        self.behavior = EditBehavior::RevealOnSelection;
        self
    }
}
impl PartialEq for Replacement {
    fn eq(&self, other: &Self) -> bool {
        // Runtime handles are not render inputs. Freshly rebuilt descriptions
        // must compare equal before an editor has assigned or matched their IDs.
        (matches!(self.content, ReplacementContent::Widget(_)) || self.id == other.id)
            && self.range == other.range
            && self.content == other.content
            && self.behavior == other.behavior
    }
}

/// Display geometry belongs to the block provider, not text style metadata.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ParagraphStyle {
    /// Base font of the whole source line, including empty or concealed lines.
    /// Inline font overrides inherit from this font rather than the editor font.
    pub font: Option<crate::render::Font>,
    /// Base size in logical pixels. Unset typography inherits from the editor.
    pub font_size: Option<f32>,
    /// Base line height in logical pixels, shared by every wrapped row.
    /// Taller inline content can still expand a row around the shared baseline.
    pub line_height: Option<f32>,
    pub align: Option<crate::render::TextAlign>,
    pub inset_left: f32,
    pub inset_right: f32,
    pub space_before: f32,
    pub space_after: f32,
    /// Additional first-line indent. Negative values create hanging indentation.
    pub first_line_indent: f32,
    pub background: Option<crate::render::Hsla>,
    pub leading_rule: Option<(f32, crate::render::Hsla)>,
}
impl ParagraphStyle {
    pub fn validate(&self) -> Result<(), EditError> {
        crate::InlineStyle {
            font: self.font.clone(),
            font_size: self.font_size,
            line_height: self.line_height,
            ..Default::default()
        }
        .validate()
        .map_err(|_| EditError::Rejected("invalid paragraph typography"))?;
        if [
            self.inset_left,
            self.inset_right,
            self.space_before,
            self.space_after,
        ]
        .iter()
        .any(|v| !v.is_finite() || *v < 0.0)
            || !self.first_line_indent.is_finite()
            || self
                .leading_rule
                .is_some_and(|(w, _)| !w.is_finite() || w < 0.0)
        {
            return Err(EditError::Rejected("invalid block geometry"));
        }
        for color in [self.background, self.leading_rule.map(|(_, c)| c)]
            .into_iter()
            .flatten()
        {
            if ![color.h, color.s, color.l, color.a]
                .into_iter()
                .all(f32::is_finite)
            {
                return Err(EditError::Rejected("invalid block color"));
            }
        }
        Ok(())
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct BlockStyle {
    pub range: Range<usize>,
    pub style: ParagraphStyle,
}
/// A custom block replaces a source range with a described or registered view.
/// It participates in the same viewport cache as text.
#[derive(Debug, Clone)]
pub struct BlockView {
    pub id: ViewId,
    pub range: Range<usize>,
    pub widget: Option<ViewSpec>,
    /// Source-backed blocks keep their arrangement while IME edits their cells.
    pub source_backed: bool,
}
impl BlockView {
    /// A block backed by an explicit `EditorViews` registration.
    pub fn new(id: ViewId, range: Range<usize>) -> Self {
        Self {
            id,
            range,
            widget: None,
            source_backed: false,
        }
    }
    /// Preserve this source-backed layout during composition. Opaque widgets
    /// should use the default behavior so their source is available to the IME.
    pub fn source_backed(mut self) -> Self {
        self.source_backed = true;
        self
    }
    /// A block backed by a self-contained, comparable view description.
    pub fn widget(range: Range<usize>, description: impl ViewDescription) -> Self {
        Self {
            id: ViewId::default(),
            range,
            widget: Some(ViewSpec::new(description)),
            source_backed: false,
        }
    }
    /// Preserve this widget across moves within its producing extension.
    pub fn key(mut self, key: impl Into<smol_str::SmolStr>) -> Self {
        self.widget
            .as_mut()
            .expect("keys require a described widget")
            .key = Some(key.into());
        self
    }
}
impl PartialEq for BlockView {
    fn eq(&self, other: &Self) -> bool {
        (self.widget.is_some() || self.id == other.id)
            && self.range == other.range
            && self.widget == other.widget
            && self.source_backed == other.source_backed
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Projection {
    pub styles: Vec<StyleSpan>,
    pub replacements: Vec<Replacement>,
    pub paragraphs: Vec<BlockStyle>,
    pub blocks: Vec<BlockView>,
}
impl Projection {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn replace(mut self, r: Replacement) -> Self {
        self.replacements.push(r);
        self
    }
    pub fn style(mut self, s: StyleSpan) -> Self {
        // Fluent style layers compose independent fields and normalize overlaps.
        // Raw public span arrays still require canonical disjoint intervals.
        self.styles = super::highlights::overlay(&self.styles, std::slice::from_ref(&s));
        self
    }
    pub fn paragraph(mut self, range: Range<usize>, style: ParagraphStyle) -> Self {
        self.paragraphs.push(BlockStyle { range, style });
        self
    }
    pub fn block(mut self, id: ViewId, range: Range<usize>) -> Self {
        self.blocks.push(BlockView::new(id, range));
        self
    }
    /// Add a described block, optionally carrying a stable key.
    pub fn block_view(mut self, block: BlockView) -> Self {
        self.blocks.push(block);
        self
    }
    pub fn validate(&mut self, source: &(impl TextRead + ?Sized)) -> Result<(), EditError> {
        fn check(source: &(impl TextRead + ?Sized), r: &Range<usize>) -> Result<(), EditError> {
            if r.start > r.end
                || !source.is_char_boundary(r.start)
                || !source.is_char_boundary(r.end)
            {
                Err(EditError::InvalidRange(r.clone()))
            } else {
                Ok(())
            }
        }
        self.replacements
            .sort_by_key(|r| (r.range.start, r.range.end));
        self.styles.sort_by_key(|s| (s.range.start, s.range.end));
        self.paragraphs
            .sort_by_key(|s| (s.range.start, s.range.end));
        self.blocks.sort_by_key(|b| (b.range.start, b.range.end));
        let mut ids = std::collections::HashSet::new();
        let mut end = 0;
        for r in &self.replacements {
            check(source, &r.range)?;
            if matches!(r.content,ReplacementContent::Object(id) if id!=r.id) {
                return Err(EditError::Rejected(
                    "object identity must match replacement identity",
                ));
            }
            if r.range.start < end
                || (!matches!(r.content, ReplacementContent::Widget(_)) && !ids.insert(r.id))
            {
                return Err(EditError::Rejected("conflicting projection replacements"));
            }
            end = r.range.end;
        }
        end = 0;
        for s in &self.styles {
            check(source, &s.range)?;
            s.style
                .validate()
                .map_err(|_| EditError::Rejected("invalid projected style"))?;
            if s.range.start < end {
                return Err(EditError::Rejected("overlapping projected styles"));
            }
            end = s.range.end;
        }
        for s in &self.paragraphs {
            check(source, &s.range)?;
            s.style.validate()?;
        }
        end = 0;
        for b in &self.blocks {
            check(source, &b.range)?;
            if b.range.is_empty()
                || b.range.start < end
                || (b.widget.is_none() && !ids.insert(b.id))
            {
                return Err(EditError::Rejected("conflicting block views"));
            }
            end = b.range.end;
            if self.replacements.iter().any(|r| {
                r.range.start < b.range.end
                    && r.range.end > b.range.start
                    && !(r.range.start >= b.range.start && r.range.end <= b.range.end)
            }) {
                return Err(EditError::Rejected(
                    "inline replacement overlaps block view",
                ));
            }
        }
        // Give public projection queries collision-free local handles. Layout
        // subsequently matches these descriptions to the previous document view.
        self.match_widgets(None, None, false, false)?;
        Ok(())
    }
    pub fn active(&self, selections: &SelectionSet, composition: Option<Range<usize>>) -> Self {
        let mut out = self.clone();
        out.replacements.retain(|r| {
            let intersects =
                |range: Range<usize>| range.start <= r.range.end && range.end >= r.range.start;
            // Boundary insertions do not replace the neighboring object. Keep
            // structural replacements such as a cell break at the IME boundary.
            !composition.as_ref().is_some_and(|c| {
                if c.is_empty() {
                    r.range.start < c.start && c.start < r.range.end
                } else {
                    r.range.start < c.end && c.start < r.range.end
                }
            }) && (r.behavior != EditBehavior::RevealOnSelection
                || selections.is_structured()
                || !selections.iter().any(|s| intersects(s.text_range())))
        });
        out
    }
    pub(crate) fn compose(&mut self, range: Range<usize>, length: usize) {
        let map = |p: usize, bias: Bias| {
            if p < range.start || (p == range.start && bias == Bias::Before) {
                p
            } else if p >= range.end {
                p - range.len() + length
            } else {
                range.start + if bias == Bias::After { length } else { 0 }
            }
        };
        self.blocks.retain(|b| {
            b.source_backed || b.range.end <= range.start || b.range.start >= range.end
        });
        for r in &mut self.replacements {
            r.range = if r.range.is_empty() {
                let position = map(r.range.start, Bias::After);
                position..position
            } else {
                map(r.range.start, Bias::After)..map(r.range.end, Bias::Before)
            };
        }
        for b in &mut self.blocks {
            b.range = map(b.range.start, Bias::Before)..map(b.range.end, Bias::After);
        }
        for p in &mut self.paragraphs {
            p.range = map(p.range.start, Bias::Before)..map(p.range.end, Bias::After);
        }
        let edits = [super::Edit::new(range, " ".repeat(length))];
        self.styles = super::format::map_spans(&self.styles, &edits);
    }
    /// Look up a source line's container style after validating the projection.
    pub fn paragraph_style(&self, byte: usize) -> ParagraphStyle {
        // Validation sorts starts. Skip later decorations before searching for
        // the last containing range; long decorated documents need local lookup.
        let end = self.paragraphs.partition_point(|s| s.range.start <= byte);
        self.paragraphs[..end]
            .iter()
            .rev()
            .find(|s| s.range.contains(&byte) || s.range.is_empty() && s.range.start == byte)
            .map(|s| s.style.clone())
            .unwrap_or_default()
    }
    /// Only the requested layout fragment is materialized. A replacement may
    /// cross hard breaks; callers group that source range into one layout block.
    /// Call `validate` before querying an authored projection to sort ranges and
    /// assign collision-free widget handles. EditorState and EditorLayout do this
    /// automatically when accepting a projection.
    pub fn project(
        &self,
        source: &(impl TextRead + ?Sized),
        range: Range<usize>,
        base: &[StyleSpan],
    ) -> Result<ProjectedText, EditError> {
        if range.start > range.end
            || !source.is_char_boundary(range.start)
            || !source.is_char_boundary(range.end)
        {
            return Err(EditError::InvalidRange(range));
        }
        let mut out = ProjectedText {
            source: range.clone(),
            text: String::new(),
            spans: Vec::new(),
            map: PositionMap::default(),
            objects: Vec::new(),
        };
        let first = self
            .replacements
            .partition_point(|r| r.range.end < range.start);
        let mut cursor = range.start;
        for r in &self.replacements[first..] {
            if r.range.start > range.end {
                break;
            }
            if r.range.start < range.start || r.range.end > range.end {
                continue;
            }
            if cursor < r.range.start {
                out.copy(source, cursor..r.range.start, base)?;
            }
            let start = out.text.len();
            match &r.content {
                ReplacementContent::Text(text) => out.text.push_str(text),
                ReplacementContent::Object(_) | ReplacementContent::Widget(_) => {
                    out.text.push('\u{fffc}');
                    out.objects.push(ProjectedObject {
                        id: r.id,
                        range: start..out.text.len(),
                        source: r.range.clone(),
                    });
                }
            }
            let end = out.text.len();
            if start < end {
                let style = base
                    .iter()
                    .find(|s| s.range.contains(&r.range.start))
                    .map(|s| s.style.clone())
                    .unwrap_or_default();
                super::format::push(&mut out.spans, start..end, style);
            }
            out.map.segments.push(MapSegment {
                source: r.range.clone(),
                display: start..end,
                copied: false,
            });
            cursor = r.range.end;
        }
        if cursor < range.end {
            out.copy(source, cursor..range.end, base)?;
        }
        out.map.source = range;
        out.map.display_len = out.text.len();
        Ok(out)
    }
    /// Expand a deletion to whole atomic replacements. Hidden source must not
    /// leave half a marker or half an embedded object after a visual deletion.
    pub fn atomic_range(&self, mut range: Range<usize>) -> Range<usize> {
        for r in &self.replacements {
            if r.range.start < range.end && r.range.end > range.start {
                range.start = range.start.min(r.range.start);
                range.end = range.end.max(r.range.end);
            }
        }
        range
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectedObject {
    pub id: ViewId,
    pub range: Range<usize>,
    pub source: Range<usize>,
}
#[derive(Debug, Clone)]
pub struct ProjectedText {
    pub source: Range<usize>,
    pub text: String,
    pub spans: Vec<StyleSpan>,
    pub map: PositionMap,
    pub objects: Vec<ProjectedObject>,
}
impl ProjectedText {
    pub(crate) fn empty(source: Range<usize>) -> Self {
        Self {
            source: source.clone(),
            text: String::new(),
            spans: Vec::new(),
            objects: Vec::new(),
            map: PositionMap {
                source,
                display_len: 0,
                segments: Vec::new(),
            },
        }
    }

    fn copy(
        &mut self,
        source: &(impl TextRead + ?Sized),
        range: Range<usize>,
        base: &[StyleSpan],
    ) -> Result<(), EditError> {
        let start = self.text.len();
        self.text.push_str(
            &source
                .read(range.clone())
                .ok_or_else(|| EditError::InvalidRange(range.clone()))?,
        );
        let end = self.text.len();
        let first = base.partition_point(|s| s.range.end <= range.start);
        for s in &base[first..] {
            if s.range.start >= range.end {
                break;
            }
            let a = s.range.start.max(range.start);
            let b = s.range.end.min(range.end);
            if a < b {
                super::format::push(
                    &mut self.spans,
                    start + a - range.start..start + b - range.start,
                    s.style.clone(),
                );
            }
        }
        self.map.segments.push(MapSegment {
            source: range,
            display: start..end,
            copied: true,
        });
        Ok(())
    }
}
#[derive(Debug, Clone, Default)]
pub struct PositionMap {
    source: Range<usize>,
    display_len: usize,
    segments: Vec<MapSegment>,
}
#[derive(Debug, Clone)]
struct MapSegment {
    source: Range<usize>,
    display: Range<usize>,
    copied: bool,
}
impl PositionMap {
    pub fn to_display(&self, byte: usize, bias: Bias) -> usize {
        map(
            &self.segments,
            byte,
            bias,
            false,
            self.source.start,
            self.display_len,
        )
    }
    pub fn to_source(&self, byte: usize, bias: Bias) -> usize {
        map(&self.segments, byte, bias, true, 0, self.source.end)
    }
    pub fn selection_to_source(&self, s: Selection) -> Selection {
        Selection {
            anchor: self.to_source(
                s.anchor,
                if s.anchor <= s.head {
                    Bias::Before
                } else {
                    Bias::After
                },
            ),
            head: self.to_source(s.head, s.affinity),
            ..s
        }
    }
}
fn map(
    segments: &[MapSegment],
    byte: usize,
    bias: Bias,
    reverse: bool,
    origin: usize,
    end: usize,
) -> usize {
    let mut result = if reverse {
        segments.first().map_or(end, |s| s.source.start)
    } else {
        0
    };
    for s in segments {
        let (from, to) = if reverse {
            (&s.display, &s.source)
        } else {
            (&s.source, &s.display)
        };
        if byte < from.start {
            return result;
        }
        if byte == from.start && from.is_empty() {
            if bias == Bias::Before {
                return to.start;
            }
            result = to.end;
            continue;
        }
        if byte == from.start {
            return to.start;
        }
        if byte < from.end {
            if s.copied {
                return to.start + byte.saturating_sub(from.start);
            }
            return if bias == Bias::Before {
                to.start
            } else {
                to.end
            };
        }
        result = to.end;
    }
    if byte < origin { 0 } else { result.min(end) }
}

/// Immutable parser input. Background work never borrows the live editor.
#[derive(Debug, Clone)]
pub struct ProjectionSnapshot {
    /// True only for a transient display snapshot containing IME preedit.
    pub composing: bool,
    pub document_id: u64,
    pub revision: u64,
    pub change: Option<Arc<super::Change>>,
    pub text: super::TextSnapshot,
    pub spans: Arc<[StyleSpan]>,
    pub selections: SelectionSet,
}

#[cfg(test)]
mod composition_tests {
    use super::*;
    #[test]
    fn source_backed_blocks_survive_preedit_and_map_both_edges() {
        for range in [0..0, 3..3, 2..5] {
            let mut projection = Projection::new()
                .block_view(BlockView::new(ViewId(1), 0..10).source_backed())
                .block(ViewId(2), 12..16);
            projection.compose(range.clone(), 6);
            assert_eq!(projection.blocks.len(), 2);
            assert_eq!(projection.blocks[0].range, 0..10 - range.len() + 6);
            assert_eq!(
                projection.blocks[1].range,
                12 - range.len() + 6..16 - range.len() + 6
            );
        }
        let mut opaque = Projection::new().block(ViewId(1), 0..10);
        opaque.compose(3..3, 6);
        assert!(opaque.blocks.is_empty());
    }
    #[test]
    fn preedit_at_replacement_boundaries_keeps_breaks_and_empty_annotations() {
        let plan = Projection::new()
            .replace(Replacement::text(ViewId(1), 1..5, "\n"))
            .replace(Replacement::text(ViewId(2), 5..5, "hint"));
        let mut active = plan.active(&SelectionSet::single(Selection::caret(5)), Some(5..5));
        active.compose(5..5, 6);
        assert_eq!(active.replacements[0].range, 1..5);
        assert_eq!(active.replacements[1].range, 11..11);
    }
    #[test]
    fn structured_selection_does_not_reveal_source_and_survives_mapping() {
        let selection = SelectionSet::structured(super::super::Selection::caret(2));
        let projection =
            Projection::new().replace(Replacement::hide(ViewId(1), 1..4).reveal_on_selection());
        assert_eq!(projection.active(&selection, None).replacements.len(), 1);
        assert!(
            projection
                .active(
                    &SelectionSet::single(super::super::Selection::caret(2)),
                    None
                )
                .replacements
                .is_empty()
        );
        assert!(
            selection
                .map(&super::super::ChangeSet::default())
                .is_structured()
        );
    }
}
