//! Optional vertical geometry for mixed absolute heights. Native glyphs, clusters,
//! horizontal layout and Unicode analysis stay in Parley; only row metrics live here.
use super::paragraph_inline::InlineContext;
use crate::{TextBrush, TextRun};
use parley::{Layout, LineMetrics};

#[derive(Clone, Copy)]
pub(super) struct HeightSpan {
    pub end: usize,
    pub height: f32,
}

#[derive(Clone, Copy)]
struct RowMetrics {
    top: f32,
    height: f32,
    baseline: f32,
    ascent: f32,
    descent: f32,
}

#[derive(Clone)]
pub(super) struct ParagraphRows {
    spans: Box<[HeightSpan]>,
    rows: Vec<RowMetrics>,
    native_height: f32,
    inline: Option<InlineContext>,
}

impl ParagraphRows {
    pub fn for_inline(
        runs: &[TextRun],
        height: f32,
        objects: &[crate::InlineTextBox],
        context: InlineContext,
    ) -> Self {
        let mut rows = Self::new(runs, height).unwrap_or_else(|| Self {
            spans: vec![HeightSpan {
                end: runs.iter().map(|run| run.len).sum(),
                height: runs
                    .iter()
                    .find(|run| run.len > 0)
                    .and_then(|run| run.line_height)
                    .unwrap_or(height),
            }]
            .into_boxed_slice(),
            rows: Vec::new(),
            native_height: height,
            inline: None,
        });
        rows.native_height = rows.native_height.max(height).max(
            objects
                .iter()
                .map(|object| object.height)
                .fold(0.0, f32::max),
        );
        rows.inline = Some(context);
        rows
    }

    /// Runs have already been validated and normalized. Adjacent equal heights
    /// share one span; uniform paragraphs allocate neither spans nor row metrics.
    pub fn new(runs: &[TextRun], default_height: f32) -> Option<Self> {
        let first = runs
            .iter()
            .find(|run| run.len > 0)?
            .line_height
            .unwrap_or(default_height);
        if runs
            .iter()
            .filter(|run| run.len > 0)
            .all(|run| run.line_height.unwrap_or(default_height) == first)
        {
            return None;
        }
        let mut spans: Vec<HeightSpan> = Vec::new();
        let mut end = 0;
        let mut native_height = first;
        for run in runs.iter().filter(|run| run.len > 0) {
            end += run.len;
            let height = run.line_height.unwrap_or(default_height);
            native_height = native_height.max(height);
            if let Some(last) = spans.last_mut().filter(|last| last.height == height) {
                last.end = end;
            } else {
                spans.push(HeightSpan { end, height });
            }
        }
        Some(Self {
            spans: spans.into_boxed_slice(),
            rows: Vec::new(),
            native_height,
            inline: None,
        })
    }

    pub fn native_height(&self) -> f32 {
        self.native_height
    }

    /// Keep native rows disjoint even if every requested height is smaller than
    /// the font's ink extent. Parley's selection geometry locates rows by y, so a
    /// native overlap can select the wrong row before we translate its rectangle.
    /// Only line breaking is repeated; no font resolution or shaping is repeated.
    pub fn separate_native_rows(&self, layout: &mut Layout<TextBrush>, width: Option<f32>) {
        let mut bottom = f32::NEG_INFINITY;
        let mut extent = self.native_height;
        let mut overlap = false;
        for line in layout.lines() {
            let m = line.metrics();
            overlap |= m.block_min_coord < bottom;
            bottom = m.block_max_coord;
            extent = extent.max(m.ascent + m.descent);
        }
        if !overlap {
            return;
        }
        // A native row is centered at y + native_height/2 and has extent no
        // greater than `extent`. Separate centers by that extent, with one ULP
        // of room at each step to avoid overlapping bounds after f32 rounding.
        let mut breaker = layout.break_lines();
        breaker
            .state_mut()
            .set_layout_max_advance(width.unwrap_or(f32::MAX));
        breaker
            .state_mut()
            .set_line_max_advance(width.unwrap_or(f32::MAX));
        let mut y = f64::from((extent - self.native_height) * 0.5);
        loop {
            breaker.state_mut().set_line_y(y);
            if breaker.break_next().is_none() {
                break;
            }
            y = f64::from(((y + f64::from(extent)) as f32).next_up());
        }
        breaker.finish();
    }

    pub fn rebuild(&mut self, layout: &Layout<TextBrush>, count: usize) -> f32 {
        let mut span_index = 0;
        let mut top = 0.0_f64;
        // Keep capacity across resize probes; shrinking and then growing a
        // paragraph does not repeatedly allocate its row metrics.
        self.rows.clear();
        self.rows.extend(layout.lines().take(count).map(|line| {
            let range = line.text_range();
            // Empty final rows inherit the last surviving source byte's style.
            // Nonempty rows include separators and trailing whitespace in their
            // source intersections, even when those bytes produce no glyphs.
            while span_index + 1 < self.spans.len() && self.spans[span_index].end <= range.start {
                span_index += 1;
            }
            let mut height = self.spans[span_index].height;
            let mut next = span_index;
            while next + 1 < self.spans.len() && self.spans[next].end < range.end {
                next += 1;
                height = height.max(self.spans[next].height);
            }
            let native = line.metrics();
            let (ascent, descent, over) = if let Some(inline) = &self.inline {
                let extents = inline.line(&line, &self.spans);
                height = extents.over + extents.under;
                (extents.ascent, extents.descent, extents.over)
            } else {
                let leading = height - native.ascent - native.descent;
                (native.ascent, native.descent, native.ascent + leading * 0.5)
            };
            let row = RowMetrics {
                top: top as f32,
                height,
                baseline: top as f32 + over,
                ascent,
                descent,
            };
            top += f64::from(height);
            row
        }));
        top as f32
    }

    pub fn metrics(&self, row: usize, native: &LineMetrics) -> LineMetrics {
        let row = self.rows[row];
        let leading = row.height - (row.ascent + row.descent);
        LineMetrics {
            line_height: row.height,
            ascent: row.ascent,
            descent: row.descent,
            leading,
            baseline: row.baseline,
            // Selection/caret geometry covers the union of the line box and
            // typographic content, including overflow under negative leading.
            block_min_coord: row.top.min(row.baseline - row.ascent),
            block_max_coord: (row.top + row.height).max(row.baseline + row.descent),
            ..*native
        }
    }

    pub fn object_offset(&self, id: u64) -> f32 {
        self.inline
            .as_ref()
            .map_or(0.0, |inline| inline.object_offset(id))
    }

    pub fn row_at(&self, y: f32) -> usize {
        // Hit testing uses nonoverlapping line advances. Ink can overflow those
        // advances with tight leading, but must not make row choice ambiguous.
        self.rows
            .partition_point(|row| row.top <= y)
            .saturating_sub(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn uniform_heights_need_no_adapter_and_adjacent_heights_are_coalesced() {
        let run = |len, line_height| TextRun {
            len,
            line_height,
            ..Default::default()
        };
        assert!(ParagraphRows::new(&[run(2, None), run(3, Some(24.0))], 24.0).is_none());
        assert!(ParagraphRows::new(&[run(2, Some(32.0)), run(0, Some(64.0))], 24.0).is_none());
        let rows = ParagraphRows::new(
            &[run(2, None), run(2, Some(24.0)), run(1, Some(48.0))],
            24.0,
        )
        .unwrap();
        assert_eq!(rows.spans.len(), 2);
        assert_eq!(rows.spans[0].end, 4);
        assert_eq!(rows.native_height(), 48.0);
        assert!(rows.rows.is_empty());
    }

    #[test]
    fn row_storage_reuses_capacity_across_resize_probes() {
        use crate::{ParleyTextSystem, TextSystem, font};
        use std::{borrow::Cow, sync::Arc};
        let backend = ParleyTextSystem::new_without_system_fonts("IBM Plex Sans");
        backend
            .add_fonts(vec![Cow::Borrowed(include_bytes!(
                "../../tests/fonts/IBMPlexSans-Regular.ttf"
            ))])
            .unwrap();
        let system = TextSystem::new(Arc::new(backend));
        let text = "one two three four five six";
        let runs = [
            TextRun {
                len: 4,
                font: font("IBM Plex Sans"),
                line_height: Some(32.0),
                ..Default::default()
            },
            TextRun {
                len: text.len() - 4,
                font: font("IBM Plex Sans"),
                ..Default::default()
            },
        ];
        let mut paragraph = system
            .shape_paragraph(text.into(), &runs, 16.0, 24.0, Some(30.0), None)
            .unwrap();
        let mut rows = ParagraphRows::new(&runs, 24.0).unwrap();
        rows.rebuild(paragraph.layout(), paragraph.line_count());
        let ptr = rows.rows.as_ptr();
        let capacity = rows.rows.capacity();
        for width in [500.0, 100.0, 30.0] {
            paragraph.reflow(Some(width));
            rows.rebuild(paragraph.layout(), paragraph.line_count());
            assert_eq!(rows.rows.as_ptr(), ptr);
            assert_eq!(rows.rows.capacity(), capacity);
        }
    }
}
