//! CRLF preprocessing preserves source byte positions. Ordinary LF-only text
//! stays shared and allocates no normalized string or offset table.
use crate::{SharedString, TextRun};
use std::borrow::Cow;
use unicode_segmentation::GraphemeCursor;
#[derive(Clone)]
pub(super) struct TextSource {
    original: SharedString,
    normalized: Option<Box<Normalized>>,
}
#[derive(Clone)]
struct Normalized {
    text: SharedString,
    removed: Vec<usize>,
}
impl TextSource {
    #[cfg(test)]
    pub fn new(original: SharedString) -> Self {
        Self::with_objects(original, &[])
    }
    pub fn with_objects(original: SharedString, objects: &[crate::InlineTextBox]) -> Self {
        let normalized = (!objects.is_empty() || original.contains('\r')).then(|| {
            let mut text = String::with_capacity(original.len());
            let mut removed = Vec::new();
            let mut chars = original.char_indices().peekable();
            while let Some((i, ch)) = chars.next() {
                if objects.binary_search_by_key(&i, |b| b.index).is_ok() {
                    removed.extend(i..i + ch.len_utf8());
                } else if ch == '\r' {
                    if chars.peek().is_some_and(|(_, next)| *next == '\n') {
                        removed.push(i);
                    } else {
                        text.push('\n');
                    }
                } else {
                    text.push(ch);
                }
            }
            Box::new(Normalized {
                text: text.into(),
                removed,
            })
        });
        Self {
            original,
            normalized,
        }
    }
    pub fn shared_original(&self) -> SharedString {
        self.original.clone()
    }
    pub fn original(&self) -> &str {
        &self.original
    }
    pub fn text(&self) -> &str {
        self.normalized
            .as_ref()
            .map(|s| s.text.as_str())
            .unwrap_or(&self.original)
    }
    pub fn to_layout(&self, index: usize) -> usize {
        index
            - self
                .normalized
                .as_ref()
                .map(|n| n.removed.partition_point(|p| *p < index))
                .unwrap_or(0)
    }
    pub fn to_source(&self, index: usize) -> usize {
        index
            + self
                .normalized
                .as_ref()
                .map(|n| {
                    // Removed CR bytes have normalized positions raw_position - rank.
                    let mut low = 0;
                    let mut high = n.removed.len();
                    while low < high {
                        let mid = (low + high) / 2;
                        if n.removed[mid] - mid < index {
                            low = mid + 1;
                        } else {
                            high = mid;
                        }
                    }
                    low
                })
                .unwrap_or(0)
    }
    pub fn runs<'a>(&self, runs: &'a [TextRun]) -> Cow<'a, [TextRun]> {
        if self.normalized.is_none() {
            return Cow::Borrowed(runs);
        }
        let mut start = 0;
        Cow::Owned(
            runs.iter()
                .map(|r| {
                    let end = start + r.len;
                    let mut run = r.clone();
                    run.len = self.to_layout(end) - self.to_layout(start);
                    start = end;
                    run
                })
                .collect(),
        )
    }
    /// Shaping clusters may split a grapheme when its font lacks a combining
    /// mark or ZWJ sequence. UI carets still obey UAX #29, without a cell array.
    pub fn snap(&self, index: usize, toward_end: bool) -> usize {
        let text = self.text();
        let index = index.min(text.len());
        let mut cursor = GraphemeCursor::new(index, text.len(), true);
        if cursor
            .is_boundary(text, 0)
            .expect("complete text supplies grapheme context")
        {
            return index;
        }
        if toward_end {
            cursor.next_boundary(text, 0).unwrap().unwrap_or(text.len())
        } else {
            cursor.prev_boundary(text, 0).unwrap().unwrap_or(0)
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn crlf_offsets_round_trip_at_grapheme_boundaries() {
        let s = TextSource::new("ab\r\n\r\nc\r".into());
        assert_eq!(s.text(), "ab\n\nc\n");
        for i in [0, 1, 2, 4, 6, 7, 8] {
            assert_eq!(s.to_source(s.to_layout(i)), i);
        }
        assert_eq!(s.to_layout(3), 2);
    }
}
