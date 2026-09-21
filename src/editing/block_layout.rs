//! Custom block arrangement uses the editor's own text measurement and source
//! ranges. Cells are not independent editors: selection, undo and IME stay shared.
use super::TextRead;
use crate::core::geometry::{Rect, Size};
use std::ops::Range;
#[derive(Debug, Clone)]
pub struct TextCell {
    pub source: Range<usize>,
    pub bounds: Rect<f32>,
}
#[derive(Debug, Clone, Default)]
pub struct BlockArrangement {
    /// Re-run the provider as the viewport/required source changes.
    pub viewport_dependent: bool,
    pub size: Size<f32>,
    pub cells: Vec<TextCell>,
    pub rules: Vec<(Rect<f32>, crate::render::Hsla)>,
    /// Optional decoration and interaction layer behind the source-backed cells.
    /// Its bounds are the arrangement size; unhandled events reach source text.
    /// It shares embedded-view focus, capture and mounting, without replacing text.
    pub decoration: Option<super::ViewSpec>,
}
impl BlockArrangement {
    /// Attach a retained view whose inputs include the geometry from this layout.
    /// The block owns its size; the view's measured size does not resize the block.
    pub fn with_decoration(mut self, view: impl super::ViewDescription) -> Self {
        self.decoration = Some(super::ViewSpec::new(view));
        self
    }
}
pub trait BlockMeasure {
    /// Previous cell geometry during transient IME display updates. Providers
    /// can retain widths and minimum heights while preedit candidates change.
    fn composition_cells(&self) -> Option<&[TextCell]> {
        None
    }

    fn source(&self) -> &dyn TextRead;
    fn available_width(&self) -> f32;
    /// Block-local viewport. Providers can emit only visible children while
    /// reporting the full block extent for very large compound documents.
    fn viewport(&self) -> Option<Rect<f32>>;
    fn required_position(&self) -> Option<usize> {
        None
    }
    fn measure_text(
        &mut self,
        source: Range<usize>,
        width: f32,
    ) -> crate::render::Result<Size<f32>>;
}
pub trait BlockLayout {
    fn layout(
        &self,
        source: Range<usize>,
        measure: &mut dyn BlockMeasure,
    ) -> crate::render::Result<BlockArrangement>;
}
/// Source-backed table layout. Rows/cells are supplied by a schema or parser;
/// this provider supplies geometry without owning document content or history.
#[derive(Debug, Clone)]
pub struct GridBlock {
    pub rows: Vec<Vec<Range<usize>>>,
    pub column_weights: Vec<f32>,
    pub padding: f32,
    pub gap: f32,
    pub rule: Option<(f32, crate::render::Hsla)>,
}
impl BlockLayout for GridBlock {
    fn layout(
        &self,
        source: Range<usize>,
        m: &mut dyn BlockMeasure,
    ) -> crate::render::Result<BlockArrangement> {
        anyhow::ensure!(
            self.padding.is_finite()
                && self.padding >= 0.0
                && self.gap.is_finite()
                && self.gap >= 0.0
                && !self.column_weights.is_empty()
                && self
                    .column_weights
                    .iter()
                    .all(|w| w.is_finite() && *w > 0.0),
            "invalid grid layout"
        );
        let columns = self.column_weights.len();
        let total: f32 = self.column_weights.iter().sum();
        let width = m.available_width();
        let space = (width - self.gap * (columns.saturating_sub(1)) as f32).max(0.0);
        let widths: Vec<_> = self
            .column_weights
            .iter()
            .map(|w| space * w / total)
            .collect();
        let mut out = BlockArrangement::default();
        let mut y = 0.0;
        for row in &self.rows {
            anyhow::ensure!(row.len() == columns, "grid row must match column count");
            let mut sizes = Vec::with_capacity(columns);
            let mut height = 0.0f32;
            for (cell, w) in row.iter().zip(&widths) {
                anyhow::ensure!(
                    cell.start >= source.start && cell.end <= source.end,
                    "grid cell is outside block source"
                );
                let size = m.measure_text(cell.clone(), (w - 2.0 * self.padding).max(0.0))?;
                height = height.max(size.height + 2.0 * self.padding);
                sizes.push(size);
            }
            let mut x = 0.0;
            for ((cell, w), _size) in row.iter().zip(&widths).zip(sizes) {
                out.cells.push(TextCell {
                    source: cell.clone(),
                    bounds: Rect::from_xywh(
                        x + self.padding,
                        y + self.padding,
                        (w - 2.0 * self.padding).max(0.0),
                        (height - 2.0 * self.padding).max(0.0),
                    ),
                });
                if let Some((thickness, color)) = self.rule {
                    anyhow::ensure!(
                        thickness.is_finite() && thickness >= 0.0,
                        "invalid table rule"
                    );
                    out.rules
                        .push((Rect::from_xywh(x, y, thickness, height), color));
                    out.rules
                        .push((Rect::from_xywh(x, y, *w, thickness), color));
                }
                x += w + self.gap;
            }
            y += height + self.gap;
        }
        out.size = Size::new(width, (y - self.gap).max(0.0));
        Ok(out)
    }
}
