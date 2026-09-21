//! Widget-independent retained Parley paragraphs. Width changes rebreak existing
//! glyph data; color/alignment/selection never reshape text.
#![doc = include_str!("../../docs/parley.md")]
use crate::core::geometry::{Point, Rect, Size};
use std::sync::Arc;
use voidui_gpui_wgpu::{
    Font, Hsla, Painter, Paragraph, Result, SharedString, TextAlign, TextLayoutCache, TextSystem,
    parley,
};
pub struct Text {
    content: SharedString,
    // Convenience measure/paint callers allocate this only when requested.
    // Widgets keep their own paragraph and never carry a second inline cache.
    cached: Option<Box<PreparedText>>,
}
pub struct PreparedText {
    paragraph: Arc<Paragraph>,
    options: TextLayoutOptions,
    system: Arc<TextSystem>,
    font_revision: u64,
    // Cache intrinsic numbers, not extra glyph layouts, across viewport changes.
    intrinsic: [Option<TextMeasurement>; 2],
    // Uniform cache keys cannot describe explicit inline typography.
    runs: Option<Arc<[voidui_gpui_wgpu::TextRun]>>,
}
#[derive(Clone, Copy)]
pub(crate) struct TextMeasurement {
    pub size: Size<f32>,
    pub first: Option<f32>,
    pub last: Option<f32>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct TextLayoutOptions {
    pub font: Font,
    pub color: Hsla,
    pub font_size: f32,
    pub line_height: f32,
    pub wrap_width: Option<f32>,
    /// Maximum visible rows. Hidden rows never paint or participate in mouse hits.
    pub line_clamp: Option<usize>,
}
impl Text {
    pub fn new(content: impl Into<SharedString>) -> Self {
        Self {
            content: content.into(),
            cached: None,
        }
    }
    pub fn content(&self) -> &SharedString {
        &self.content
    }
    pub fn set_content(&mut self, content: impl Into<SharedString>) {
        let content = content.into();
        if self.content != content {
            self.content = content;
            self.cached = None;
        }
    }
    pub fn shape(
        &self,
        cache: &TextLayoutCache,
        options: &TextLayoutOptions,
    ) -> Result<PreparedText> {
        let paragraph = cache.prepare(
            self.content.clone(),
            &options.font,
            options.font_size,
            options.line_height,
            options.wrap_width,
            options.line_clamp,
        )?;
        Ok(PreparedText {
            paragraph,
            options: options.clone(),
            system: cache.system().clone(),
            font_revision: cache.font_revision(),
            intrinsic: [None, None],
            runs: None,
        })
    }
    pub fn min_content_width(&self, cache: &TextLayoutCache, options: &TextLayoutOptions) -> f32 {
        self.shape(cache, options)
            .expect("failed to shape text")
            .min_content_width()
    }
    pub fn measure(
        &mut self,
        cache: &TextLayoutCache,
        options: &TextLayoutOptions,
    ) -> Result<Size<f32>> {
        self.cached = None;
        self.cached = Some(Box::new(self.shape(cache, options)?));
        Ok(self.cached.as_ref().unwrap().size())
    }
    pub fn paint(
        &self,
        painter: &mut Painter<'_>,
        bounds: Rect<f32>,
        align: TextAlign,
    ) -> Result<()> {
        self.cached
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Text::measure must succeed before Text::paint"))?
            .paint(painter, bounds, align)
    }
}
impl PreparedText {
    /// Shape one inline formatting context. All spans share Unicode analysis,
    /// line breaking, selection geometry, and a common baseline on each row.
    pub fn shape_rich(
        cache: &TextLayoutCache,
        content: &crate::core::rich_text::RichText,
        options: &TextLayoutOptions,
    ) -> Result<Self> {
        if content.spans().is_empty() {
            return Text::new(content.shared_text()).shape(cache, options);
        }
        let runs: Arc<[_]> = crate::core::rich_text::resolve_runs(
            content.text(),
            content.spans(),
            0..content.text().len(),
            &options.font,
        )
        .into();
        let paragraph = cache.prepare_runs(
            content.shared_text(),
            runs.clone(),
            options.font_size,
            options.line_height,
            options.wrap_width,
            options.line_clamp,
        )?;
        Ok(Self {
            paragraph,
            options: options.clone(),
            system: cache.system().clone(),
            font_revision: cache.font_revision(),
            intrinsic: [None, None],
            runs: Some(runs),
        })
    }

    /// Direct native data for custom widgets. There is no secondary glyph layout.
    pub fn paragraph(&self) -> &Paragraph {
        &self.paragraph
    }
    pub fn source(&self) -> &str {
        self.paragraph.source()
    }
    pub fn size(&self) -> Size<f32> {
        Size::new(self.paragraph.width(), self.paragraph.height())
    }
    pub fn first_baseline(&self) -> Option<f32> {
        self.paragraph.first_baseline()
    }
    pub fn last_baseline(&self) -> Option<f32> {
        self.paragraph.last_baseline()
    }
    pub(crate) fn measurement(&self) -> TextMeasurement {
        TextMeasurement {
            size: self.size(),
            first: self.first_baseline(),
            last: self.last_baseline(),
        }
    }
    pub(crate) fn intrinsic(&mut self, minimum: bool) -> TextMeasurement {
        let slot = usize::from(minimum);
        if let Some(m) = self.intrinsic[slot] {
            return m;
        }
        let old = self.wrap_width();
        let width = minimum.then(|| self.min_content_width());
        self.reflow(width);
        let m = self.measurement();
        self.reflow(old);
        self.intrinsic[slot] = Some(m);
        m
    }
    pub(crate) fn share(&mut self, cache: &TextLayoutCache) {
        if let Some(runs) = &self.runs {
            cache.intern_runs(
                &mut self.paragraph,
                runs.clone(),
                self.options.font_size,
                self.options.line_height,
                self.options.line_clamp,
            );
            return;
        }
        cache.intern(
            &mut self.paragraph,
            &self.options.font,
            self.options.font_size,
            self.options.line_height,
            self.options.line_clamp,
        );
    }
    pub fn min_content_width(&self) -> f32 {
        self.paragraph.content_widths().min
    }
    pub fn wrap_width(&self) -> Option<f32> {
        self.options.wrap_width
    }
    pub fn matches(&self, cache: &TextLayoutCache, options: &TextLayoutOptions) -> bool {
        self.options.font == options.font
            && self.options.font_size == options.font_size
            && self.options.line_height == options.line_height
            && self.options.line_clamp == options.line_clamp
            && self.font_revision == cache.font_revision()
            && cache.same_system(&self.system)
    }
    /// Copy-on-write only when an identical paragraph is shared with another
    /// widget. Weak cache entries do not force a second glyph-data allocation.
    pub fn reflow(&mut self, width: Option<f32>) {
        if self.options.wrap_width != width {
            Arc::make_mut(&mut self.paragraph).reflow(width);
            self.options.wrap_width = width;
        }
    }
    pub fn paint(
        &self,
        painter: &mut Painter<'_>,
        bounds: Rect<f32>,
        align: TextAlign,
    ) -> Result<()> {
        self.paint_with_color(painter, bounds, align, self.options.color)
    }
    pub fn paint_with_color(
        &self,
        painter: &mut Painter<'_>,
        bounds: Rect<f32>,
        align: TextAlign,
        color: Hsla,
    ) -> Result<()> {
        self.paragraph
            .paint(painter, native(bounds), align, Some(color), None)
    }
    pub fn paint_selection(
        &self,
        painter: &mut Painter<'_>,
        bounds: Rect<f32>,
        align: TextAlign,
        color: Hsla,
        selection: &TextSelectionPaint,
    ) -> Result<()> {
        self.paragraph.paint(
            painter,
            native(bounds),
            align,
            Some(color),
            Some((
                selection.range.clone(),
                selection.colors.color.into(),
                selection.colors.background.into(),
            )),
        )
    }
    pub fn cursor_at(&self, p: Point<f32>, width: f32, align: TextAlign) -> Option<parley::Cursor> {
        self.paragraph
            .cursor_at(voidui_gpui_wgpu::point(p.x, p.y), width, align)
    }
    pub fn hit_position(&self, p: Point<f32>, width: f32, align: TextAlign) -> Option<usize> {
        self.cursor_at(p, width, align)
            .map(|c| self.paragraph.source_index(c.index()))
    }
    pub fn character_at(&self, p: Point<f32>, width: f32, align: TextAlign) -> Option<usize> {
        self.paragraph
            .character_at(voidui_gpui_wgpu::point(p.x, p.y), width, align)
    }
    pub fn selection_unit(
        &self,
        p: Point<f32>,
        width: f32,
        align: TextAlign,
        word: bool,
    ) -> Option<std::ops::Range<usize>> {
        self.paragraph
            .selection_unit(voidui_gpui_wgpu::point(p.x, p.y), width, align, word)
    }
    pub fn caret_position(&self, byte: usize, width: f32, align: TextAlign) -> Option<Point<f32>> {
        self.caret_with_affinity(byte, parley::Affinity::Downstream, width, align)
    }
    pub fn caret_with_affinity(
        &self,
        byte: usize,
        affinity: parley::Affinity,
        width: f32,
        align: TextAlign,
    ) -> Option<Point<f32>> {
        self.paragraph
            .caret_position(byte, affinity, width, align)
            .map(|p| Point::new(p.x, p.y))
    }
    pub fn row_range(&self, byte: usize) -> Option<std::ops::Range<usize>> {
        self.paragraph.row_range(byte)
    }
    pub fn selection_rectangles(
        &self,
        range: std::ops::Range<usize>,
        width: f32,
        align: TextAlign,
    ) -> Vec<(usize, Rect<f32>)> {
        self.paragraph
            .selection_rectangles(range, width, align)
            .into_iter()
            .map(|(row, b)| {
                (
                    row,
                    Rect::from_xywh(b.origin.x, b.origin.y, b.size.width, b.size.height),
                )
            })
            .collect()
    }
}
fn native(b: Rect<f32>) -> voidui_gpui_wgpu::Bounds<f32> {
    voidui_gpui_wgpu::Bounds::new(
        voidui_gpui_wgpu::point(b.origin.x, b.origin.y),
        voidui_gpui_wgpu::size(b.size.width, b.size.height),
    )
}
#[derive(Debug, Clone)]
pub struct TextSelectionPaint {
    pub range: std::ops::Range<usize>,
    pub colors: crate::style::selection::SelectionColors,
}
