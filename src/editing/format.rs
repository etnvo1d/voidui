//! Inline formatting patches and canonical span operations used by transactions.
use super::{Edit, EditError};
use crate::{
    core::rich_text::{InlineStyle, StyleSpan},
    render::{Font, FontStyle, FontWeight, Hsla, SharedString, StrikethroughStyle, UnderlineStyle},
};
use std::{collections::BTreeMap, ops::Range, sync::Arc};

/// Each field independently keeps (`None`), sets (`Some(Some(value))`), or
/// clears (`Some(None)`) an override. Clearing resumes the view's base style.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StylePatch {
    pub font: Option<Option<Font>>,
    pub font_weight: Option<Option<FontWeight>>,
    pub font_style: Option<Option<FontStyle>>,
    pub metadata: Option<Option<Arc<BTreeMap<SharedString, SharedString>>>>,
    pub font_size: Option<Option<f32>>,
    pub line_height: Option<Option<f32>>,
    pub color: Option<Option<Hsla>>,
    pub background: Option<Option<Hsla>>,
    pub underline: Option<Option<UnderlineStyle>>,
    pub strikethrough: Option<Option<StrikethroughStyle>>,
}
impl StylePatch {
    pub fn new() -> Self {
        Self::default()
    }
    /// Remove all explicit overrides so text inherits its surrounding style.
    pub fn clear() -> Self {
        Self::replace(InlineStyle::default())
    }
    /// False explicitly overrides inherited bold with normal weight.
    pub fn bold(mut self, enabled: bool) -> Self {
        self.font_weight = Some(Some(if enabled {
            FontWeight::BOLD
        } else {
            FontWeight::NORMAL
        }));
        self
    }
    /// False explicitly overrides inherited italic with normal posture.
    pub fn italic(mut self, enabled: bool) -> Self {
        self.font_style = Some(Some(if enabled {
            FontStyle::Italic
        } else {
            FontStyle::Normal
        }));
        self
    }
    pub fn color(mut self, color: impl Into<Hsla>) -> Self {
        self.color = Some(Some(color.into()));
        self
    }
    /// Values are validated atomically when applying the transaction.
    pub fn font_size(mut self, size: f32) -> Self {
        self.font_size = Some(Some(size));
        self
    }
    /// Values are validated atomically when applying the transaction.
    pub fn line_height(mut self, height: f32) -> Self {
        self.line_height = Some(Some(height));
        self
    }

    /// Replace all overrides, including clearing fields absent from `style`.
    pub fn replace(style: InlineStyle) -> Self {
        Self {
            font: Some(style.font),
            font_weight: Some(style.font_weight),
            font_style: Some(style.font_style),
            metadata: Some(style.metadata),
            font_size: Some(style.font_size),
            line_height: Some(style.line_height),
            color: Some(style.color),
            background: Some(style.background),
            underline: Some(style.underline),
            strikethrough: Some(style.strikethrough),
        }
    }
    pub(crate) fn apply(&self, style: &mut InlineStyle) {
        macro_rules! field {
            ($($name:ident),*) => {$(if let Some(value) = &self.$name {
                style.$name = value.clone();
            })*};
        }
        field!(
            font,
            font_weight,
            font_style,
            metadata,
            font_size,
            line_height,
            color,
            background,
            underline,
            strikethrough
        );
    }
}

/// A format operation in the resulting document's UTF-8 coordinates.
/// Operations run in order after all text replacements, just like output selection.
#[derive(Debug, Clone, PartialEq)]
pub struct Format {
    pub range: Range<usize>,
    pub patch: StylePatch,
}

/// An exact style replacement in the resulting revision. Default style clears
/// overrides. These deltas also describe formatting-only undo and redo.
#[derive(Debug, Clone, PartialEq)]
pub struct StyleChange {
    pub range: Range<usize>,
    pub style: InlineStyle,
}

pub(crate) fn validate_style(style: &InlineStyle) -> Result<(), EditError> {
    style
        .validate()
        .map_err(|_| EditError::Rejected("invalid inline style"))
}

pub(crate) fn push(spans: &mut Vec<StyleSpan>, range: Range<usize>, style: InlineStyle) {
    if range.is_empty() || style == InlineStyle::default() {
        return;
    }
    if let Some(last) = spans.last_mut() {
        if last.range.end == range.start && last.style == style {
            last.range.end = range.end;
            return;
        }
    }
    spans.push(StyleSpan { range, style });
}

pub(crate) fn slice(spans: &[StyleSpan], range: Range<usize>) -> Vec<StyleSpan> {
    let first = spans.partition_point(|s| s.range.end <= range.start);
    spans[first..]
        .iter()
        .take_while(|s| s.range.start < range.end)
        .filter_map(|s| {
            let start = range.start.max(s.range.start);
            let end = range.end.min(s.range.end);
            (start < end).then(|| StyleSpan {
                range: start - range.start..end - range.start,
                style: s.style.clone(),
            })
        })
        .collect()
}

pub(crate) fn style_at(spans: &[StyleSpan], position: usize, before: bool) -> InlineStyle {
    let index = spans.partition_point(|s| {
        if before && position > 0 {
            s.range.end < position
        } else {
            s.range.end <= position
        }
    });
    spans
        .get(index)
        .filter(|s| {
            if before && position > 0 {
                s.range.start < position
            } else {
                s.range.start <= position
            }
        })
        .map(|s| s.style.clone())
        .unwrap_or_default()
}

/// Copy surviving source intervals with a monotonic cursor. A span crossing
/// several edits is visited only for its surviving pieces; no temporary slices.
fn copy_segment(
    out: &mut Vec<StyleSpan>,
    spans: &[StyleSpan],
    index: &mut usize,
    range: Range<usize>,
    destination: usize,
) {
    while *index < spans.len() && spans[*index].range.end <= range.start {
        *index += 1;
    }
    while let Some(span) = spans.get(*index) {
        if span.range.start >= range.end {
            break;
        }
        let start = span.range.start.max(range.start);
        let end = span.range.end.min(range.end);
        if start < end {
            push(
                out,
                destination + start - range.start..destination + end - range.start,
                span.style.clone(),
            );
        }
        if span.range.end > range.end {
            break;
        }
        *index += 1;
    }
}

/// Preserve only surviving text's styles. Insertions start without overrides;
/// exact output deltas subsequently supply their formatting. O(spans + edits).
pub(crate) fn map_spans(spans: &[StyleSpan], edits: &[Edit]) -> Vec<StyleSpan> {
    if spans.is_empty() {
        return Vec::new();
    }
    let mut result = Vec::with_capacity(spans.len());
    let (mut old, mut new, mut index) = (0, 0, 0);
    for edit in edits {
        copy_segment(&mut result, spans, &mut index, old..edit.range.start, new);
        new += edit.range.start - old + edit.insert.len();
        old = edit.range.end;
    }
    let end = spans.last().map_or(old, |s| s.range.end.max(old));
    copy_segment(&mut result, spans, &mut index, old..end, new);
    result
}

/// Emit just a patched interval, retaining independent fields in spans and gaps.
fn patch_segment(
    out: &mut Vec<StyleSpan>,
    spans: &[StyleSpan],
    index: &mut usize,
    range: Range<usize>,
    patch: &StylePatch,
) {
    let mut position = range.start;
    while position < range.end {
        while *index < spans.len() && spans[*index].range.end <= position {
            *index += 1;
        }
        let mut style = InlineStyle::default();
        let end = if let Some(span) = spans.get(*index) {
            if span.range.start <= position {
                style = span.style.clone();
                span.range.end.min(range.end)
            } else {
                span.range.start.min(range.end)
            }
        } else {
            range.end
        };
        patch.apply(&mut style);
        push(out, position..end, style);
        position = end;
    }
}

/// Patch one interval. Prefix, affected intervals, and suffix are emitted in
/// order without sorting boundaries or revisiting earlier spans.
pub(crate) fn patch(
    spans: &[StyleSpan],
    range: Range<usize>,
    patch: &StylePatch,
) -> Vec<StyleSpan> {
    if range.is_empty() {
        return spans.to_vec();
    }
    let mut result = Vec::with_capacity(spans.len());
    let mut index = 0;
    copy_segment(&mut result, spans, &mut index, 0..range.start, 0);
    patch_segment(&mut result, spans, &mut index, range.clone(), patch);
    let end = spans
        .last()
        .map_or(range.end, |s| s.range.end.max(range.end));
    copy_segment(&mut result, spans, &mut index, range.end..end, range.end);
    result
}

/// Multi-selection commands already supply sorted disjoint ranges, so apply
/// their patches in one sweep. Explicit overlapping/unsorted commands preserve
/// caller order and use sequential passes instead of changing patch semantics.
pub(crate) fn apply_formats(mut spans: Vec<StyleSpan>, formats: &[Format]) -> Vec<StyleSpan> {
    if formats.is_empty() {
        return spans;
    }
    if !formats
        .windows(2)
        .all(|pair| pair[0].range.end <= pair[1].range.start)
    {
        for operation in formats {
            spans = patch(&spans, operation.range.clone(), &operation.patch);
        }
        return spans;
    }
    let mut result = Vec::with_capacity(spans.len());
    let (mut index, mut position) = (0, 0);
    for operation in formats {
        copy_segment(
            &mut result,
            &spans,
            &mut index,
            position..operation.range.start,
            position,
        );
        patch_segment(
            &mut result,
            &spans,
            &mut index,
            operation.range.clone(),
            &operation.patch,
        );
        position = operation.range.end;
    }
    let end = spans.last().map_or(position, |s| s.range.end.max(position));
    copy_segment(&mut result, &spans, &mut index, position..end, position);
    result
}

/// Compare ordered interval boundaries in linear time. Only changed portions
/// enter history; default style is an explicit clearing delta.
pub(crate) fn diff(before: &[StyleSpan], after: &[StyleSpan]) -> Vec<StyleChange> {
    let mut left = before
        .iter()
        .flat_map(|s| [s.range.start, s.range.end])
        .peekable();
    let mut right = after
        .iter()
        .flat_map(|s| [s.range.start, s.range.end])
        .peekable();
    let mut result: Vec<StyleChange> = Vec::new();
    let (mut i, mut j) = (0, 0);
    let mut previous = None;
    loop {
        let end = match (left.peek(), right.peek()) {
            (Some(a), Some(b)) => *a.min(b),
            (Some(a), None) => *a,
            (None, Some(b)) => *b,
            (None, None) => break,
        };
        while left.peek() == Some(&end) {
            left.next();
        }
        while right.peek() == Some(&end) {
            right.next();
        }
        if let Some(start) = previous {
            while i < before.len() && before[i].range.end <= start {
                i += 1;
            }
            while j < after.len() && after[j].range.end <= start {
                j += 1;
            }
            let default = InlineStyle::default();
            let old = before
                .get(i)
                .filter(|s| s.range.start <= start)
                .map(|s| &s.style)
                .unwrap_or(&default);
            let new = after
                .get(j)
                .filter(|s| s.range.start <= start)
                .map(|s| &s.style)
                .unwrap_or(&default);
            if old != new {
                if let Some(last) = result
                    .last_mut()
                    .filter(|last| last.range.end == start && last.style == *new)
                {
                    last.range.end = end;
                } else {
                    result.push(StyleChange {
                        range: start..end,
                        style: new.clone(),
                    });
                }
            }
        }
        previous = Some(end);
    }
    result
}

/// Apply disjoint sorted replacements with one sweep. This is shared by rich
/// paste, undo and redo; each source span and delta is visited once.
pub(crate) fn apply_changes(spans: Vec<StyleSpan>, changes: &[StyleChange]) -> Vec<StyleSpan> {
    if changes.is_empty() {
        return spans;
    }
    let mut result = Vec::with_capacity(spans.len().max(changes.len()));
    let (mut index, mut position) = (0, 0);
    for change in changes {
        debug_assert!(position <= change.range.start);
        copy_segment(
            &mut result,
            &spans,
            &mut index,
            position..change.range.start,
            position,
        );
        push(&mut result, change.range.clone(), change.style.clone());
        position = change.range.end;
    }
    let end = spans.last().map_or(position, |s| s.range.end.max(position));
    copy_segment(&mut result, &spans, &mut index, position..end, position);
    result
}

/// Charge shared payloads conservatively per retained style, so a large link or
/// font descriptor cannot bypass the history budget by sharing an Arc.
/// Allocator metadata and outer history-container spare capacity are excluded.
pub(crate) fn retained_style_bytes(style: &InlineStyle) -> usize {
    let mut bytes = 0;
    if let Some(font) = &style.font {
        bytes += font.family.len();
        bytes += font.features.0.capacity() * std::mem::size_of::<(String, u32)>();
        bytes += font
            .features
            .0
            .iter()
            .map(|(name, _)| name.capacity())
            .sum::<usize>();
        if let Some(fallbacks) = &font.fallbacks {
            bytes += fallbacks.0.capacity() * std::mem::size_of::<String>();
            bytes += fallbacks.0.iter().map(String::capacity).sum::<usize>();
        }
    }
    if let Some(metadata) = &style.metadata {
        bytes += std::mem::size_of::<BTreeMap<SharedString, SharedString>>();
        bytes += metadata
            .iter()
            .map(|(key, value)| {
                std::mem::size_of::<(SharedString, SharedString)>() + key.len() + value.len()
            })
            .sum::<usize>();
    }
    bytes
}
