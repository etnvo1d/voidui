//! Parley owns shaping, wrapping and horizontal geometry. Display line boxes
//! use a compact row adapter; no glyph or caret arrays are copied.
use super::paragraph_inline::InlineContext;
use super::paragraph_rows::ParagraphRows;
use super::source::TextSource;
use crate::*;
use parley::{Affinity, Cluster, Cursor, Layout, PositionedLayoutItem, Selection};
use std::ops::Range;
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TextBrush {
    pub color: Hsla,
    /// Keep the run foreground when a paragraph paint color is supplied.
    pub color_is_explicit: bool,
    pub background: Option<Hsla>,
    pub underline: Option<UnderlineStyle>,
    pub strikethrough: Option<StrikethroughStyle>,
}
impl From<&TextRun> for TextBrush {
    fn from(r: &TextRun) -> Self {
        Self {
            color: r.color,
            color_is_explicit: r.color_is_explicit,
            background: r.background_color,
            underline: r.underline,
            strikethrough: r.strikethrough,
        }
    }
}
impl TextBrush {
    /// Resolve the normal foreground. Selection colors are applied separately.
    pub fn foreground(&self, inherited: Option<Hsla>) -> Hsla {
        if self.color_is_explicit {
            self.color
        } else {
            inherited.unwrap_or(self.color)
        }
    }
}
/// A source U+FFFC replaced by an inline layout box. Source indices remain UTF-8.
#[derive(Debug, Clone, PartialEq)]
pub struct InlineTextBox {
    pub id: u64,
    pub index: usize,
    pub width: f32,
    pub height: f32,
    /// Distance from the top edge to the baseline for `Baseline` alignment.
    pub baseline: f32,
    pub align: InlineAlignment,
    /// Visual relative offset in parent-font em units; excluded from line sizing.
    pub offset_em: f32,
}
#[derive(Clone)]
pub struct Paragraph {
    layout: Layout<TextBrush>,
    source: TextSource,
    width: Option<f32>,
    clamp: Option<usize>,
    measured_width: f32,
    measured_height: f32,
    content_widths: parley::ContentWidths,
    rows: Option<Box<ParagraphRows>>,
    objects: Vec<InlineTextBox>,
    indent: f32,
}
impl TextSystem {
    pub fn shape_paragraph(
        &self,
        text: SharedString,
        runs: &[TextRun],
        size: f32,
        height: f32,
        width: Option<f32>,
        clamp: Option<usize>,
    ) -> Result<Paragraph> {
        self.shape_paragraph_impl(text, runs, size, height, width, clamp, &[], 0.0, None)
    }
    /// Shape an inline formatting context with a CSS font strut on every row.
    /// Pass the container's typography separately from its content styles so
    /// removing the last text glyph does not discard the baseline. Supported
    /// parent-relative object alignments are resolved before source replacement.
    pub fn shape_inline_paragraph(
        &self,
        text: SharedString,
        runs: &[TextRun],
        style: InlineTextStyle<'_>,
        width: Option<f32>,
        clamp: Option<usize>,
        objects: &[InlineTextBox],
        indent: f32,
    ) -> Result<Paragraph> {
        self.shape_paragraph_impl(
            text,
            runs,
            style.font_size,
            style.line_height,
            width,
            clamp,
            objects,
            indent,
            Some(style),
        )
    }

    fn shape_paragraph_impl(
        &self,
        text: SharedString,
        runs: &[TextRun],
        size: f32,
        height: f32,
        width: Option<f32>,
        clamp: Option<usize>,
        objects: &[InlineTextBox],
        indent: f32,
        style: Option<InlineTextStyle<'_>>,
    ) -> Result<Paragraph> {
        anyhow::ensure!(indent.is_finite(), "invalid paragraph indent");
        let mut objects = objects.to_vec();
        objects.sort_by_key(|b| b.index);
        let mut previous = None;
        let mut object_ids = std::collections::HashSet::new();
        for b in &objects {
            anyhow::ensure!(
                b.width.is_finite()
                    && b.width >= 0.0
                    && b.height.is_finite()
                    && b.height > 0.0
                    && b.baseline.is_finite()
                    && b.offset_em.is_finite()
                    && text
                        .get(b.index..)
                        .is_some_and(|s| s.starts_with('\u{fffc}'))
                    && previous != Some(b.index)
                    && object_ids.insert(b.id),
                "invalid inline box"
            );
            previous = Some(b.index);
        }
        anyhow::ensure!(
            size.is_finite()
                && size > 0.0
                && height.is_finite()
                && height > 0.0
                && width.is_none_or(|w| w.is_finite() && w >= 0.0),
            "invalid text layout dimensions"
        );
        let mut end = 0usize;
        for run in runs {
            anyhow::ensure!(
                run.font_size.is_none_or(|v| v.is_finite() && v > 0.0)
                    && run.line_height.is_none_or(|v| v.is_finite() && v > 0.0),
                "invalid text run dimensions"
            );
            end = end
                .checked_add(run.len)
                .ok_or_else(|| anyhow::anyhow!("text run length overflow"))?;
            anyhow::ensure!(
                end <= text.len() && text.is_char_boundary(end),
                "invalid UTF-8 text run"
            );
        }
        anyhow::ensure!(
            end == text.len(),
            "text runs must cover the entire paragraph"
        );
        let inline = if !text.is_empty() && clamp != Some(0) {
            style
                .map(|style| InlineContext::new(self, style, runs, &mut objects))
                .transpose()?
        } else {
            None
        };
        let source = TextSource::with_objects(text, &objects);
        let boxes: Vec<_> = objects
            .iter()
            .map(|b| parley::InlineBox {
                id: b.id,
                index: source.to_layout(b.index),
                width: b.width,
                height: b.height,
                kind: parley::InlineBoxKind::InFlow,
            })
            .collect();
        let runs = source.runs(runs);
        let rows = if clamp == Some(0) {
            None
        } else {
            match inline {
                Some(context) => Some(Box::new(ParagraphRows::for_inline(
                    &runs, height, &objects, context,
                ))),
                None => ParagraphRows::new(&runs, height).map(Box::new),
            }
        };
        let mut layout = if source.original().is_empty() || clamp == Some(0) {
            Layout::new()
        } else {
            self.backend.build_with_boxes(
                source.text(),
                &runs,
                size,
                height,
                rows.as_ref().map(|rows| rows.native_height()),
                &boxes,
            )?
        };
        layout.set_text_indent(indent, parley::IndentOptions::default());
        let content_widths = layout.calculate_content_widths();
        let mut paragraph = Paragraph {
            layout,
            source,
            width: None,
            clamp,
            content_widths,
            measured_width: 0.0,
            measured_height: 0.0,
            rows,
            objects,
            indent,
        };
        paragraph.reflow(width);
        anyhow::ensure!(
            paragraph.source.original().is_empty()
                || clamp == Some(0)
                || !paragraph.objects.is_empty()
                || paragraph.layout.lines().any(|l| l.runs().next().is_some()),
            "no fonts available for nonempty text"
        );
        Ok(paragraph)
    }
}
impl Paragraph {
    /// Display bounds for each visible embedded box, including row translation.
    pub fn inline_boxes(&self, width: f32, align: TextAlign) -> Vec<(u64, usize, Bounds<f32>)> {
        let mut out = Vec::new();
        for (row, line) in self.layout.lines().take(self.line_count()).enumerate() {
            let baseline = self.line_metrics(row).baseline;
            for item in line.items() {
                if let PositionedLayoutItem::InlineBox(b) = item {
                    if let Some(source) = self.objects.iter().find(|o| o.id == b.id) {
                        out.push((
                            b.id,
                            source.index,
                            Bounds::new(
                                point(
                                    b.x + self.offset(row, width, align),
                                    baseline - source.baseline
                                        + self
                                            .rows
                                            .as_ref()
                                            .map_or(0.0, |rows| rows.object_offset(b.id)),
                                ),
                                size(b.width, source.height),
                            ),
                        ));
                    }
                }
            }
        }
        out
    }
    /// Native shaping, byte ranges, clusters and horizontal geometry.
    /// For mixed heights, native y coordinates use isolated layout bands;
    /// use `row_bounds`, `caret_bounds`, baselines and selection APIs for display.
    pub fn layout(&self) -> &Layout<TextBrush> {
        &self.layout
    }
    pub fn source(&self) -> &str {
        self.source.original()
    }
    pub fn layout_text(&self) -> &str {
        self.source.text()
    }
    pub fn source_index(&self, index: usize) -> usize {
        self.objects
            .iter()
            .filter(|b| self.source.to_layout(b.index) == index)
            .map(|b| b.index + '\u{fffc}'.len_utf8())
            .max()
            .unwrap_or_else(|| self.source.to_source(index))
    }
    pub fn layout_index(&self, index: usize) -> usize {
        self.source.to_layout(index)
    }
    pub fn snap_cursor(&self, cursor: Cursor) -> Cursor {
        let index = self
            .source
            .snap(cursor.index(), cursor.affinity() == Affinity::Upstream);
        Cursor::from_byte_index(&self.layout, index, cursor.affinity())
    }
    pub fn line_count(&self) -> usize {
        self.layout.len().min(self.clamp.unwrap_or(usize::MAX))
    }
    pub(crate) fn shared_source(&self) -> SharedString {
        self.source.shared_original()
    }
    pub fn width(&self) -> f32 {
        self.measured_width
    }
    /// Total advance of the visible lines, including a trailing empty line.
    /// Native caret/selection bounds can extend beyond this box for tight leading.
    pub fn height(&self) -> f32 {
        self.measured_height
    }
    pub fn first_baseline(&self) -> Option<f32> {
        self.layout
            .get(0)
            .filter(|_| self.line_count() > 0)
            .map(|_| self.line_metrics(0).baseline)
    }
    pub fn last_baseline(&self) -> Option<f32> {
        self.layout
            .get(self.line_count().saturating_sub(1))
            .filter(|_| self.line_count() > 0)
            .map(|_| self.line_metrics(self.line_count() - 1).baseline)
    }
    pub fn content_widths(&self) -> parley::ContentWidths {
        self.content_widths
    }
    pub fn wrap_width(&self) -> Option<f32> {
        self.width
    }
    /// Change only the available width. A paragraph's row clamp is set at construction.
    /// Panics if width is negative or non-finite, matching fluent style validation.
    pub fn reflow(&mut self, width: Option<f32>) {
        assert!(
            width.is_none_or(|w| w.is_finite() && w >= 0.0),
            "wrap_width must be finite and nonnegative"
        );
        // Editors revisit retained blocks on every edit. An unchanged width
        // must not walk rows, rebuild metrics or allocate per-block storage.
        if self.width == width
            && (!self.layout.is_empty()
                || self.source.original().is_empty()
                || self.clamp == Some(0))
        {
            return;
        }
        if self.clamp != Some(0)
            && !self.source.original().is_empty()
            && (self.width != width || self.layout.is_empty())
        {
            self.layout.break_all_lines(width);
            if let Some(rows) = &self.rows {
                rows.separate_native_rows(&mut self.layout, width);
            }
            self.layout
                .align(parley::Alignment::Left, parley::AlignmentOptions::default());
        }
        self.width = width;
        let count = self.line_count();
        // Native text-only paragraphs without an adapter need no row allocation.
        self.measured_height = if let Some(rows) = &mut self.rows {
            rows.rebuild(&self.layout, count)
        } else {
            self.layout
                .lines()
                .take(self.line_count())
                .map(|line| f64::from(line.metrics().line_height))
                .sum::<f64>() as f32
        };
        self.measured_width = self
            .layout
            .lines()
            .take(self.line_count())
            .map(|l| l.metrics().advance)
            .fold(0.0, f32::max)
            .min(width.unwrap_or(f32::INFINITY));
    }
    fn line_metrics(&self, row: usize) -> parley::LineMetrics {
        let line = self.layout.get(row).expect("visible paragraph row");
        self.rows
            .as_ref()
            .map_or(*line.metrics(), |rows| rows.metrics(row, line.metrics()))
    }
    fn offset(&self, row: usize, width: f32, align: TextAlign) -> f32 {
        let advance = self
            .layout
            .get(row)
            .map(|l| l.metrics().advance - l.metrics().trailing_whitespace)
            .unwrap_or(0.0);
        match align {
            TextAlign::Left => 0.0,
            TextAlign::Center => (width - advance) * 0.5,
            TextAlign::Right => width - advance,
        }
    }
    fn row_at_y(&self, y: f32) -> Option<usize> {
        if self.line_count() == 0 {
            return None;
        }
        let y = y.clamp(0.0, self.height().next_down().max(0.0));
        Some(if let Some(rows) = &self.rows {
            rows.row_at(y)
        } else {
            Cluster::from_point(&self.layout, 0.0, y)
                .map(|(cluster, _)| cluster.path().line_index())
                .unwrap_or(self.layout.len().saturating_sub(1))
                .min(self.line_count() - 1)
        })
    }
    fn local_point(&self, p: Point<f32>, width: f32, align: TextAlign) -> Option<Point<f32>> {
        let row = self.row_at_y(p.y)?;
        let y = if self.rows.is_some() {
            let line = self.layout.get(row)?;
            let m = line.metrics();
            // All points in a display row map to its native band center. Native
            // Unicode/caret queries only need x once the correct row is chosen.
            (m.block_min_coord + m.block_max_coord) * 0.5
        } else {
            p.y.clamp(0.0, self.height().next_down().max(0.0))
        };
        Some(point(p.x - self.offset(row, width, align), y))
    }
    /// Native row of a cursor from this paragraph, respecting wrap affinity and bidi.
    /// Includes clamped-away rows; compare with `line_count()` before navigating.
    /// Returns zero for an empty layout. Source offsets must first use `layout_index`.
    pub fn cursor_row(&self, cursor: Cursor) -> usize {
        // Follow Cursor::geometry's visual-cluster choice, including soft-wrap
        // affinity, bidi boundaries and the empty row after a final newline.
        let cluster = match cursor.visual_clusters(&self.layout) {
            [Some(left), Some(right)] if left.is_end_of_line() => {
                if left.is_soft_line_break()
                    && ((left.is_rtl() && cursor.affinity() == Affinity::Downstream)
                        || (!left.is_rtl() && cursor.affinity() == Affinity::Upstream))
                {
                    Some(left)
                } else {
                    Some(right)
                }
            }
            [Some(left), None] if left.is_hard_line_break() => None,
            [Some(left), _] => Some(left),
            [_, right] => right,
        };
        cluster
            .map(|c| c.path().line_index())
            .unwrap_or(self.layout.len().saturating_sub(1))
    }
    pub fn cursor_at(&self, p: Point<f32>, width: f32, align: TextAlign) -> Option<Cursor> {
        let p = self.local_point(p, width, align)?;
        Some(self.snap_cursor(Cursor::from_point(&self.layout, p.x, p.y)))
    }
    /// Source-aware pointer hit testing, including both sides of inline objects.
    /// Native cursors cannot distinguish an object's two source edges because
    /// its placeholder is removed before shaping. Compare their visual caret
    /// positions as well, including points just outside the object's ink box.
    pub fn source_cursor_at(
        &self,
        p: Point<f32>,
        width: f32,
        align: TextAlign,
    ) -> Option<(usize, Affinity)> {
        let cursor = self.cursor_at(p, width, align)?;
        let row = self.row_at_y(p.y)?;
        let x = p.x - self.offset(row, width, align);
        Some(self.source_hit_on_row(cursor, row, x))
    }

    /// Visual Home/End positions, including objects at a row's leading/trailing
    /// edge. Source-aware navigation must not skip removed object placeholders.
    pub fn source_row_edge(&self, row: usize, end: bool) -> Option<(usize, Affinity)> {
        if row >= self.line_count() {
            return None;
        }
        let line = self.layout.get(row)?;
        let m = line.metrics();
        let x = m.inline_min_coord + m.offset + if end { m.advance } else { 0.0 };
        let cursor = self.snap_cursor(Cursor::from_point(
            &self.layout,
            x,
            (m.block_min_coord + m.block_max_coord) * 0.5,
        ));
        Some(self.source_hit_on_row(cursor, row, x))
    }

    fn source_hit_on_row(&self, cursor: Cursor, row: usize, x: f32) -> (usize, Affinity) {
        let mut result = (self.source_index(cursor.index()), cursor.affinity());
        if self.objects.is_empty() {
            return result;
        }
        let mut distance = if self.cursor_row(cursor) == row {
            (cursor.geometry(&self.layout, 1.0).x0 as f32 - x).abs()
        } else {
            f32::INFINITY
        };
        for item in self.layout.get(row).expect("visible paragraph row").items() {
            if let PositionedLayoutItem::InlineBox(b) = item
                && let Some(source) = self.objects.iter().find(|object| object.id == b.id)
            {
                for (edge, byte, affinity) in [
                    (b.x, source.index, Affinity::Downstream),
                    (
                        b.x + b.width,
                        source.index + '\u{fffc}'.len_utf8(),
                        Affinity::Upstream,
                    ),
                ] {
                    let next = (edge - x).abs();
                    if next <= distance {
                        distance = next;
                        result = (byte, affinity);
                    }
                }
            }
        }
        result
    }
    pub fn character_at(&self, p: Point<f32>, width: f32, align: TextAlign) -> Option<usize> {
        let p = self.local_point(p, width, align)?;
        Cluster::from_point(&self.layout, p.x, p.y).map(|(c, _)| {
            self.source
                .to_source(self.source.snap(c.text_range().start, false))
        })
    }
    /// Word/paragraph gestures use the same Unicode analysis as layout.
    pub fn selection_unit(
        &self,
        p: Point<f32>,
        width: f32,
        align: TextAlign,
        word: bool,
    ) -> Option<Range<usize>> {
        let p = self.local_point(p, width, align)?;
        let selection = if word {
            Selection::word_from_point(&self.layout, p.x, p.y)
        } else {
            Selection::hard_line_from_point(&self.layout, p.x, p.y)
        };
        let mut range = selection.text_range();
        // Triple-click selects the paragraph terminator as well. Parley's
        // hard-line editor selection stops just before it.
        if !word
            && let Some(ch) = self
                .source
                .text()
                .get(range.end..)
                .and_then(|s| s.chars().next())
                .filter(|ch| matches!(ch, '\n' | '\u{2028}' | '\u{2029}'))
        {
            range.end += ch.len_utf8();
        }

        Some(
            self.source.to_source(self.source.snap(range.start, false))
                ..self.source.to_source(self.source.snap(range.end, true)),
        )
    }
    pub fn caret_position(
        &self,
        index: usize,
        affinity: Affinity,
        width: f32,
        align: TextAlign,
    ) -> Option<Point<f32>> {
        self.caret_bounds(index, affinity, width, align)
            .map(|bounds| bounds.origin)
    }
    /// A one-logical-pixel caret with the display row's vertical geometry.
    /// Coordinates are paragraph-local and horizontally aligned within `width`.
    /// Returns `None` for invalid source offsets, empty or clamped-away rows.
    pub fn caret_bounds(
        &self,
        index: usize,
        affinity: Affinity,
        width: f32,
        align: TextAlign,
    ) -> Option<Bounds<f32>> {
        if index > self.source.original().len()
            || !self.source.original().is_char_boundary(index)
            || self.line_count() == 0
        {
            return None;
        }
        let cursor = Cursor::from_byte_index(&self.layout, self.source.to_layout(index), affinity);
        let rect = cursor.geometry(&self.layout, 1.0);
        let row = self.cursor_row(cursor);
        if row >= self.line_count() {
            return None;
        }
        let metrics = self.line_metrics(row);
        Some(Bounds::new(
            point(
                rect.x0 as f32 + self.offset(row, width, align),
                metrics.block_min_coord,
            ),
            size(
                (rect.x1 - rect.x0) as f32,
                metrics.block_max_coord - metrics.block_min_coord,
            ),
        ))
    }
    /// Authoritative display-row bounds for vertical navigation and scrolling.
    /// Includes trailing whitespace and the same vertical extent as carets.
    pub fn row_bounds(&self, row: usize, width: f32, align: TextAlign) -> Option<Bounds<f32>> {
        if row >= self.line_count() {
            return None;
        }
        let metrics = self.line_metrics(row);
        Some(Bounds::new(
            point(self.offset(row, width, align), metrics.block_min_coord),
            size(
                metrics.advance,
                metrics.block_max_coord - metrics.block_min_coord,
            ),
        ))
    }
    pub fn selection_rectangles(
        &self,
        range: Range<usize>,
        width: f32,
        align: TextAlign,
    ) -> Vec<(usize, Bounds<f32>)> {
        if range.is_empty() {
            return Vec::new();
        }
        let selection = Selection::new(
            Cursor::from_byte_index(
                &self.layout,
                self.source.snap(self.source.to_layout(range.start), false),
                Affinity::Downstream,
            ),
            Cursor::from_byte_index(
                &self.layout,
                self.source.snap(self.source.to_layout(range.end), true),
                Affinity::Upstream,
            ),
        );
        let mut result = Vec::new();
        selection.geometry_with(&self.layout, |r, row| {
            if row < self.line_count() {
                let metrics = self.line_metrics(row);
                result.push((
                    row,
                    Bounds::new(
                        point(
                            r.x0 as f32 + self.offset(row, width, align),
                            metrics.block_min_coord,
                        ),
                        size(
                            (r.x1 - r.x0) as f32,
                            metrics.block_max_coord - metrics.block_min_coord,
                        ),
                    ),
                ));
            }
        });
        result
    }
    pub fn row_range(&self, index: usize) -> Option<Range<usize>> {
        let index = self.source.to_layout(index);
        self.layout
            .lines()
            .take(self.line_count())
            .find(|l| l.text_range().contains(&index))
            .or_else(|| self.layout.lines().take(self.line_count()).next_back())
            .map(|line| {
                let mut range = line.text_range();
                // End navigation stops before an explicit line terminator;
                // paragraph selection intentionally includes that terminator.
                if line.break_reason() == parley::BreakReason::Explicit {
                    if let Some(ch) = self.source.text()[..range.end]
                        .chars()
                        .next_back()
                        .filter(|ch| matches!(ch, '\n' | '\u{2028}' | '\u{2029}'))
                    {
                        range.end -= ch.len_utf8();
                    }
                }
                self.source.to_source(range.start)..self.source.to_source(range.end)
            })
    }
    /// Paint shaped glyphs with the first line's baseline at `origin`.
    /// Use this for externally positioned text such as formula display lists.
    /// Only the caller's clip applies: glyph ink may extend beyond its advance
    /// and line height (large operators, accents and italic overhangs). This
    /// paints foreground glyphs only, without paragraph backgrounds, selection
    /// or decorations. Font fallback and color emoji share the ordinary atlas.
    pub fn paint_glyphs(
        &self,
        painter: &mut Painter<'_>,
        origin: Point<f32>,
        color: Option<Hsla>,
    ) -> Result<()> {
        let Some(baseline) = self.first_baseline() else {
            return Ok(());
        };
        let backend = painter.text_system().backend.clone();
        for (row, line) in self.layout.lines().take(self.line_count()).enumerate() {
            let dy =
                origin.y + self.line_metrics(row).baseline - baseline - line.metrics().baseline;
            for item in line.items() {
                let PositionedLayoutItem::GlyphRun(run) = item else {
                    continue;
                };
                let (font, emoji) = backend.raster.register(run.run());
                let ink = run.style().brush.foreground(color);
                if ink.a <= 0.0 {
                    continue;
                }
                for glyph in run.positioned_glyphs() {
                    let position = point(px(origin.x + glyph.x), px(dy + glyph.y));
                    if emoji {
                        painter.paint_emoji(
                            position,
                            font,
                            GlyphId(glyph.id),
                            px(run.run().font_size()),
                        )?;
                    } else {
                        painter.paint_glyph(
                            position,
                            font,
                            GlyphId(glyph.id),
                            px(run.run().font_size()),
                            ink,
                        )?;
                    }
                }
            }
        }
        Ok(())
    }

    /// Paint optional selection colors over the original glyphs exactly once.
    /// Partial ligatures use disjoint clips, while whole glyphs retain overhang.
    /// `color` overrides only runs whose `color_is_explicit` is false.
    pub fn paint(
        &self,
        painter: &mut Painter<'_>,
        bounds: Bounds<f32>,
        align: TextAlign,
        color: Option<Hsla>,
        selection: Option<(Range<usize>, Hsla, Hsla)>,
    ) -> Result<()> {
        self.paint_selections(
            painter,
            bounds,
            align,
            color,
            selection
                .as_ref()
                .map(|(range, fg, bg)| (std::slice::from_ref(range), *fg, *bg)),
        )
    }
    /// Paint disjoint editor selections without repainting the paragraph glyphs.
    /// Explicit run foregrounds survive `color`; selection foregrounds override both.
    pub fn paint_selections(
        &self,
        painter: &mut Painter<'_>,
        bounds: Bounds<f32>,
        align: TextAlign,
        color: Option<Hsla>,
        selection: Option<(&[Range<usize>], Hsla, Hsla)>,
    ) -> Result<()> {
        if self.line_count() == 0 || bounds.size.width <= 0.0 || bounds.size.height <= 0.0 {
            return Ok(());
        }
        let mut clip = bounds.map(px);
        // Hanging first-line text occupies the paragraph's leading gutter. The
        // outer widget clip still limits it to the editor content box.
        if self.indent < 0.0 {
            clip.origin.x += px(self.indent);
            clip.size.width -= px(self.indent);
        }
        // Offscreen paragraphs still have retained layout, but must not allocate
        // selection rectangles or walk glyphs during a visible scene rebuild.
        if clip.intersect(&painter.content_mask().bounds).is_empty() {
            return Ok(());
        }
        let backend = painter.text_system().backend.clone();
        let mut rectangles: Vec<_> = selection
            .as_ref()
            .into_iter()
            .flat_map(|(ranges, _, _)| {
                ranges
                    .iter()
                    .flat_map(|r| self.selection_rectangles(r.clone(), bounds.size.width, align))
            })
            .collect();
        rectangles
            .sort_by(|(a, x), (b, y)| a.cmp(b).then_with(|| x.origin.x.total_cmp(&y.origin.x)));
        // Distinct logical ranges can cover the same grapheme or ligature. Merge
        // overlapping highlight rectangles before alpha blending and glyph clips.
        let mut count = 0;
        for index in 0..rectangles.len() {
            let (row, rect) = rectangles[index];
            if count > 0 {
                let (last_row, last) = &mut rectangles[count - 1];
                if *last_row == row
                    && last.origin.y == rect.origin.y
                    && last.size.height == rect.size.height
                    && rect.origin.x <= last.origin.x + last.size.width
                {
                    last.size.width = (rect.origin.x + rect.size.width)
                        .max(last.origin.x + last.size.width)
                        - last.origin.x;
                    continue;
                }
            }
            rectangles[count] = (row, rect);
            count += 1;
        }
        rectangles.truncate(count);
        painter.with_clip(clip, |painter| {
            // Run backgrounds precede the highlight overlay. Painting them in
            // the glyph loop would hide ::selection on background-styled runs.
            if self
                .layout
                .styles()
                .iter()
                .any(|s| s.brush.background.is_some_and(|c| c.a > 0.0))
            {
                for (row, line) in self.layout.lines().take(self.line_count()).enumerate() {
                    let metrics = self.line_metrics(row);
                    let dx = bounds.origin.x + self.offset(row, bounds.size.width, align);
                    for item in line.items() {
                        if let PositionedLayoutItem::GlyphRun(run) = item
                            && let Some(bg) = run.style().brush.background.filter(|c| c.a > 0.0)
                        {
                            painter.paint_quad(fill(
                                Bounds::new(
                                    point(
                                        px(dx + run.offset()),
                                        px(bounds.origin.y + metrics.block_min_coord),
                                    ),
                                    size(
                                        px(run.advance()),
                                        px(metrics.block_max_coord - metrics.block_min_coord),
                                    ),
                                ),
                                bg,
                            ));
                        }
                    }
                }
            }

            if let Some((_, _, bg)) = &selection
                && bg.a > 0.0
            {
                for (_, r) in &rectangles {
                    painter.paint_quad(fill(
                        Bounds::new(
                            point(
                                px(bounds.origin.x + r.origin.x),
                                px(bounds.origin.y + r.origin.y),
                            ),
                            r.size.map(px),
                        ),
                        *bg,
                    ));
                }
            }
            for (row, line) in self.layout.lines().take(self.line_count()).enumerate() {
                let metrics = self.line_metrics(row);
                if bounds.origin.y + metrics.block_min_coord
                    > f32::from(painter.content_mask().bounds.bottom())
                    || bounds.origin.y + metrics.block_max_coord
                        < f32::from(painter.content_mask().bounds.top())
                {
                    continue;
                }
                let dx = bounds.origin.x + self.offset(row, bounds.size.width, align);
                let dy = bounds.origin.y + metrics.baseline - line.metrics().baseline;
                for item in line.items() {
                    if let PositionedLayoutItem::GlyphRun(run) = item {
                        let (font_id, emoji) = backend.raster.register(run.run());
                        let brush = &run.style().brush;
                        let fg = brush.foreground(color);
                        for g in run.positioned_glyphs() {
                            let origin = point(px(dx + g.x), px(dy + g.y));
                            let draw = |p: &mut Painter<'_>, c: Hsla| -> Result<()> {
                                if c.a <= 0.0 {
                                    return Ok(());
                                }
                                if emoji {
                                    p.paint_emoji(
                                        origin,
                                        font_id,
                                        GlyphId(g.id),
                                        px(run.run().font_size()),
                                    )
                                } else {
                                    p.paint_glyph(
                                        origin,
                                        font_id,
                                        GlyphId(g.id),
                                        px(run.run().font_size()),
                                        c,
                                    )
                                }
                            };
                            let Some((_, selected, _)) = &selection else {
                                draw(painter, fg)?;
                                continue;
                            };
                            let low = rectangles.partition_point(|(r, _)| *r < row);
                            let high = rectangles.partition_point(|(r, _)| *r <= row);
                            let left = dx + g.x;
                            let right = left + g.advance;
                            if let Some((_, r)) = rectangles[low..high].iter().find(|(_, r)| {
                                left >= bounds.origin.x + r.origin.x - 0.001
                                    && right <= bounds.origin.x + r.origin.x + r.size.width + 0.001
                            }) {
                                let _ = r;
                                draw(painter, *selected)?;
                                continue;
                            }
                            if !rectangles[low..high].iter().any(|(_, r)| {
                                left < bounds.origin.x + r.origin.x + r.size.width
                                    && right > bounds.origin.x + r.origin.x
                            }) {
                                draw(painter, fg)?;
                                continue;
                            }
                            let mask = painter.content_mask().bounds;
                            let mut start = mask.left();
                            for (_, r) in &rectangles[low..high] {
                                let a = px(bounds.origin.x + r.origin.x).max(mask.left());
                                let b = px(bounds.origin.x + r.origin.x + r.size.width)
                                    .min(mask.right());
                                if a >= b {
                                    continue;
                                }
                                if start < a {
                                    painter.with_clip(
                                        Bounds::new(
                                            point(start, mask.top()),
                                            size(a - start, mask.size.height),
                                        ),
                                        |p| draw(p, fg),
                                    )?;
                                }
                                painter.with_clip(
                                    Bounds::new(
                                        point(a, mask.top()),
                                        size(b - a, mask.size.height),
                                    ),
                                    |p| draw(p, *selected),
                                )?;
                                start = b;
                            }
                            if start < mask.right() {
                                painter.with_clip(
                                    Bounds::new(
                                        point(start, mask.top()),
                                        size(mask.right() - start, mask.size.height),
                                    ),
                                    |p| draw(p, fg),
                                )?;
                            }
                        }
                        if let Some(mut u) = brush.underline {
                            u.color = Some(u.color.unwrap_or(fg));
                            painter.paint_underline(
                                point(
                                    px(dx + run.offset()),
                                    px(dy
                                        + run.baseline()
                                        + run.run().metrics().underline_offset.abs()),
                                ),
                                px(run.advance()),
                                &u,
                            );
                        }
                        if let Some(mut s) = brush.strikethrough {
                            s.color = Some(s.color.unwrap_or(fg));
                            painter.paint_strikethrough(
                                point(
                                    px(dx + run.offset()),
                                    px(dy + run.baseline()
                                        - run.run().metrics().strikethrough_offset),
                                ),
                                px(run.advance()),
                                &s,
                            );
                        }
                    }
                }
            }
            Ok(())
        })
    }
}
