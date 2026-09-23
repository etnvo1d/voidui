//! Batch interval normalization with insertion-order, field-wise precedence.
use super::{InlineStyle, StyleSpan};

/// Collect overlapping styles cheaply and normalize once after parsing.
/// Later entries override only the properties they set, including individual
/// metadata keys. Empty spans are ignored; validate source boundaries separately.
#[derive(Default)]
pub struct StyleSpanBuilder {
    spans: Vec<StyleSpan>,
}
impl StyleSpanBuilder {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn push(&mut self, span: StyleSpan) {
        self.spans.push(span);
    }
    pub fn finish(self) -> Vec<StyleSpan> {
        let n = self.spans.len().max(1).next_power_of_two();
        // Leaves retain insertion order. Each ancestor composes its left and
        // right children, so changing one active span costs logarithmic work.
        let mut tree = vec![InlineStyle::default(); n * 2];
        let mut events = Vec::with_capacity(self.spans.len() * 2);
        for (i, span) in self.spans.iter().enumerate() {
            if span.range.start < span.range.end {
                events.push((span.range.start, i, true));
                events.push((span.range.end, i, false));
            }
        }
        events.sort_unstable();
        let mut out: Vec<StyleSpan> = Vec::new();
        let mut previous = 0;
        for (at, index, start) in events {
            if previous < at && tree[1] != InlineStyle::default() {
                if let Some(last) = out
                    .last_mut()
                    .filter(|s| s.range.end == previous && s.style == tree[1])
                {
                    last.range.end = at;
                } else {
                    out.push(StyleSpan::new(previous..at, tree[1].clone()));
                }
            }
            let mut node = n + index;
            tree[node] = if start {
                self.spans[index].style.clone()
            } else {
                InlineStyle::default()
            };
            while node > 1 {
                node /= 2;
                tree[node] = tree[node * 2].overlay(&tree[node * 2 + 1]);
            }
            previous = at;
        }
        out
    }
}
#[cfg(all(test, feature = "editing"))]
mod tests {
    use super::*;
    #[test]
    fn batch_matches_sequential_overlays_including_metadata() {
        for seed in 0..30 {
            let mut batch = StyleSpanBuilder::new();
            let mut expected = Vec::new();
            for i in 0..80 {
                let start = (i * 37 + seed * 11) % 97;
                let end = start + (i * 13 + seed) % 31;
                let style = match i % 4 {
                    0 => InlineStyle::new().bold(),
                    1 => InlineStyle::new().italic().metadata("link", format!("{i}")),
                    2 => InlineStyle::new()
                        .font_size(12. + i as f32)
                        .metadata("title", "value"),
                    _ => InlineStyle::new().metadata("link", "override"),
                };
                let span = StyleSpan::new(start..end, style);
                if start != end {
                    expected =
                        crate::editing::highlights::overlay(&expected, std::slice::from_ref(&span));
                }
                batch.push(span);
            }
            assert_eq!(batch.finish(), expected);
        }
    }
}
