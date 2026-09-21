//! CSS line boxes for text and embedded objects in one formatting context.
//! Parley keeps ownership of shaping and horizontal line breaking.
use super::paragraph_rows::HeightSpan;
use crate::{Font, InlineTextBox, Result, TextBrush, TextRun, TextSystem};

/// Parent typography of an inline formatting context. Its zero-width strut
/// contributes to every line, including lines containing only embedded objects.
#[derive(Clone, Copy, Debug)]
pub struct InlineTextStyle<'a> {
    pub font: &'a Font,
    pub font_size: f32,
    pub line_height: f32,
}

/// Parent-relative CSS `vertical-align` values supported by embedded views.
/// Alignment is resolved from the surrounding text style, without depending on
/// whether adjacent text happens to produce glyphs on this line.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InlineAlignment {
    /// Use the view's baseline; views without one normally use their bottom edge.
    #[default]
    Baseline,
    /// Put the box's center half the parent's x-height above the text baseline.
    Middle,
    /// Align the top edge with the top of the parent's font content area.
    TextTop,
    /// Align the bottom edge with the bottom of the parent's font content area.
    TextBottom,
}

#[derive(Clone, Copy)]
pub(super) struct LineExtents {
    pub over: f32,
    pub under: f32,
    pub ascent: f32,
    pub descent: f32,
}
impl LineExtents {
    fn text(metrics: &parley::RunMetrics, height: f32) -> Self {
        // CSS 2.2 §10.8.1: distribute leading before combining inline boxes.
        // Centering the final union instead would move baselines when a small
        // widget or another font is added. Negative leading is intentional.
        let half_leading = (height - metrics.ascent - metrics.descent) * 0.5;
        Self {
            over: metrics.ascent + half_leading,
            under: metrics.descent + half_leading,
            ascent: metrics.ascent,
            descent: metrics.descent,
        }
    }
    fn include(&mut self, other: Self) {
        self.over = self.over.max(other.over);
        self.under = self.under.max(other.under);
        self.ascent = self.ascent.max(other.ascent);
        self.descent = self.descent.max(other.descent);
    }
}

#[derive(Clone)]
struct ObjectLayout {
    id: u64,
    extents: LineExtents,
    offset_y: f32,
}

#[derive(Clone)]
pub(super) struct InlineContext {
    strut: LineExtents,
    objects: Vec<ObjectLayout>,
}
impl InlineContext {
    /// Resolve objects before removing their source placeholders: an object-only
    /// styled span still supplies its parent's font size and line height.
    pub fn new(
        system: &TextSystem,
        style: InlineTextStyle<'_>,
        runs: &[TextRun],
        objects: &mut [InlineTextBox],
    ) -> Result<Self> {
        let metrics = system.backend.line_metrics(style.font, style.font_size)?;
        let strut = LineExtents::text(&metrics, style.line_height);
        let mut resolved = Vec::with_capacity(objects.len());
        let mut run_index = 0;
        let mut end = runs.first().map_or(0, |run| run.len);
        for object in objects {
            while run_index + 1 < runs.len() && end <= object.index {
                run_index += 1;
                end += runs[run_index].len;
            }
            let parent = runs.get(run_index);
            let size = parent
                .and_then(|run| run.font_size)
                .unwrap_or(style.font_size);
            let height = parent
                .and_then(|run| run.line_height)
                .unwrap_or(style.line_height);
            let font = parent.map_or(style.font, |run| &run.font);
            let metrics = system.backend.line_metrics(font, size)?;
            object.baseline = match object.align {
                InlineAlignment::Baseline => object.baseline,
                // CSS Values defines 0.5em as the fallback for an unavailable
                // x-height. Fonts that supply it always use their actual metric.
                InlineAlignment::Middle => {
                    (object.height + metrics.x_height.unwrap_or(size * 0.5)) * 0.5
                }
                InlineAlignment::TextTop => metrics.ascent,
                InlineAlignment::TextBottom => object.height - metrics.descent,
            };
            // The parent span has its own line-height even without visible text.
            let mut extents = LineExtents::text(&metrics, height);
            extents.include(LineExtents {
                over: object.baseline,
                under: object.height - object.baseline,
                ascent: object.baseline,
                descent: object.height - object.baseline,
            });
            // Relative positioning happens after line sizing. Keeping the
            // displacement separate prevents visual tweaks from moving text.
            let offset_y = object.offset_em * size;
            anyhow::ensure!(offset_y.is_finite(), "invalid inline object offset");
            resolved.push(ObjectLayout {
                id: object.id,
                extents,
                offset_y,
            });
        }
        resolved.sort_by_key(|object| object.id);
        Ok(Self {
            strut,
            objects: resolved,
        })
    }

    fn object(&self, id: u64) -> Option<&ObjectLayout> {
        self.objects
            .binary_search_by_key(&id, |object| object.id)
            .ok()
            .map(|index| &self.objects[index])
    }

    pub fn object_offset(&self, id: u64) -> f32 {
        self.object(id).map_or(0.0, |object| object.offset_y)
    }

    pub fn line(&self, line: &parley::Line<'_, TextBrush>, heights: &[HeightSpan]) -> LineExtents {
        let mut extents = self.strut;
        for run in line.runs() {
            let range = run.text_range();
            let mut index = heights
                .partition_point(|span| span.end <= range.start)
                .min(heights.len() - 1);
            loop {
                // Requested heights come from source spans. Parley 0.11.1 may
                // merge line-height-only changes or assign the next run's height.
                extents.include(LineExtents::text(run.metrics(), heights[index].height));
                if index + 1 == heights.len() || heights[index].end >= range.end {
                    break;
                }
                index += 1;
            }
        }
        for item in line.items() {
            if let parley::PositionedLayoutItem::InlineBox(object) = item
                && let Some(object) = self.object(object.id)
            {
                extents.include(object.extents);
            }
        }
        extents
    }
}
