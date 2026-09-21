//! Derived presentation is separate from editable document formatting. Keeping
//! syntax colors out of the document also keeps them out of typing and history.
use super::{EditError, format};
use crate::core::rich_text::StyleSpan;

#[derive(Debug)]
pub(super) struct Highlights {
    pub spans: Vec<StyleSpan>,
    pub generation: u64,
}

pub(super) fn canonicalize_source(
    source: &(impl super::TextRead + ?Sized),
    spans: impl IntoIterator<Item = StyleSpan>,
) -> Result<Vec<StyleSpan>, EditError> {
    let mut spans: Vec<_> = spans.into_iter().collect();
    spans.sort_by_key(|s| (s.range.start, s.range.end));
    let mut end = 0;
    let mut out = Vec::with_capacity(spans.len());
    for s in spans {
        if s.range.start < end
            || s.range.start > s.range.end
            || !source.is_char_boundary(s.range.start)
            || !source.is_char_boundary(s.range.end)
        {
            return Err(EditError::Rejected("invalid highlight spans"));
        }
        format::validate_style(&s.style)?;
        end = s.range.end;
        format::push(&mut out, s.range, s.style);
    }
    Ok(out)
}

/// Merge two canonical interval streams in O(document spans + highlights).
/// Only explicitly set highlight fields override committed formatting; removing
/// a highlight exposes the document style without an inverse edit or snapshot.
pub(crate) fn overlay(base: &[StyleSpan], highlights: &[StyleSpan]) -> Vec<StyleSpan> {
    let mut left = base
        .iter()
        .flat_map(|s| [s.range.start, s.range.end])
        .peekable();
    let mut right = highlights
        .iter()
        .flat_map(|s| [s.range.start, s.range.end])
        .peekable();
    let mut result = Vec::new();
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
            while i < base.len() && base[i].range.end <= start {
                i += 1;
            }
            while j < highlights.len() && highlights[j].range.end <= start {
                j += 1;
            }
            let style = base
                .get(i)
                .filter(|s| s.range.start <= start)
                .map(|s| s.style.clone())
                .unwrap_or_default();
            let style =
                if let Some(highlight) = highlights.get(j).filter(|s| s.range.start <= start) {
                    style.overlay(&highlight.style)
                } else {
                    style
                };
            format::push(&mut result, start..end, style);
        }
        previous = Some(end);
    }
    result
}
