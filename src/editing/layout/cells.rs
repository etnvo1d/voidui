//! Source-backed custom block children use the shared text flow and selection.
use super::*;
pub(super) struct CellMeasure<'a> {
    pub(super) composition_cells: Option<&'a [crate::editing::TextCell]>,
    pub(super) source: &'a TextSnapshot,
    pub(super) styles: &'a [StyleSpan],
    pub(super) projection: &'a Projection,
    pub(super) options: &'a LayoutOptions,
    pub(super) views: Rc<RefCell<MountedViews>>,
    pub(super) system: Arc<TextSystem>,
    pub(super) width: f32,
    pub(super) viewport: Option<Rect<f32>>,
    pub(super) required_position: Option<usize>,
    pub(super) cache: BTreeMap<(usize, usize, u32), CellFlow>,
}
impl CellMeasure<'_> {
    pub(super) fn measure(&mut self, range: Range<usize>, width: f32) -> render::Result<Size<f32>> {
        let key = (range.start, range.end, width.to_bits());
        if let Some(c) = self.cache.get(&key) {
            return Ok(c.flow.size());
        }
        let projected = self
            .projection
            .project(self.source, range.clone(), self.styles)?;
        let options = self
            .options
            .for_paragraph(&self.projection.paragraph_style(range.start));
        let runs = crate::core::rich_text::resolve_runs(
            &projected.text,
            &projected.spans,
            0..projected.text.len(),
            &options.font,
        );
        let cache = TextLayoutCache::new(self.system.clone());
        let mut boxes = Vec::new();
        for object in &projected.objects {
            let m = self.views.borrow_mut().measure(object.id, width, &cache)?;
            boxes.push(render::InlineTextBox {
                id: object.id.0,
                index: object.range.start,
                width: m.size.width,
                height: m.size.height,
                baseline: m.baseline,
                align: m.align,
                offset_em: m.offset_em,
            });
        }
        let paragraph = self.system.shape_inline_paragraph(
            projected.text.clone().into(),
            &runs,
            render::InlineTextStyle {
                font: &options.font,
                font_size: options.font_size,
                line_height: options.line_height,
            },
            Some(width),
            None,
            &boxes,
            0.0,
        )?;
        let flow = TextFlow::from_paragraph(
            paragraph,
            LayoutOptions {
                width: Some(width),
                ..options.clone()
            },
            self.system.clone(),
        );
        let size = Size::new(
            flow.size().width,
            flow.size().height.max(options.line_height),
        );
        self.cache.insert(
            key,
            CellFlow {
                source: range,
                bounds: Rect::new(Point::default(), size),
                projected,
                flow,
            },
        );
        Ok(size)
    }
}
impl crate::editing::BlockMeasure for CellMeasure<'_> {
    fn composition_cells(&self) -> Option<&[crate::editing::TextCell]> {
        self.composition_cells
    }
    fn required_position(&self) -> Option<usize> {
        self.required_position
    }
    fn source(&self) -> &dyn TextRead {
        self.source
    }
    fn available_width(&self) -> f32 {
        self.width
    }
    fn viewport(&self) -> Option<Rect<f32>> {
        self.viewport
    }
    fn measure_text(&mut self, r: Range<usize>, w: f32) -> render::Result<Size<f32>> {
        self.measure(r, w)
    }
}
impl Engine {
    pub(super) fn move_in_cells(
        &mut self,
        i: usize,
        selection: Selection,
        motion: Motion,
        width: f32,
        align: TextAlign,
        height: f32,
    ) -> Option<Selection> {
        let b = &self.blocks[i];
        let c = &self.cache[&i];
        let at = c.cells.iter().position(|cell| {
            cell.source.start <= selection.head && selection.head <= cell.source.end
        })?;
        let cell = &c.cells[at];
        if matches!(motion, Motion::DocumentStart | Motion::DocumentEnd) {
            return None;
        }
        let local = Selection {
            anchor: cell
                .projected
                .map
                .to_display(selection.anchor, Bias::Before),
            head: cell
                .projected
                .map
                .to_display(selection.head, selection.affinity),
            ..selection
        };
        let moved = cell.flow.move_selection(
            local,
            motion,
            false,
            cell.bounds.size.width,
            b.style.align.unwrap_or(align),
            height,
        );
        let byte = cell.projected.map.to_source(moved.head, moved.affinity);
        let vertical = matches!(
            motion,
            Motion::Up | Motion::Down | Motion::PageUp | Motion::PageDown
        );
        let forward = matches!(
            motion,
            Motion::Down | Motion::PageDown | Motion::Right | Motion::WordRight
        );
        if !vertical && byte != selection.head {
            return Some(Selection {
                anchor: byte,
                head: byte,
                ..moved
            });
        }
        if vertical {
            let caret = cell.flow.caret(
                local.head,
                local.affinity,
                cell.bounds.size.width,
                b.style.align.unwrap_or(align),
            )?;
            let current = Point::new(
                cell.bounds.origin.x + caret.origin.x,
                cell.bounds.origin.y + caret.origin.y + caret.size.height * 0.5,
            );
            let p = Point::new(
                selection
                    .preferred_x
                    .unwrap_or(current.x + b.style.inset_left - self.source_offset(i).x)
                    - b.style.inset_left
                    + self.source_offset(i).x,
                current.y
                    + if forward {
                        caret.size.height
                    } else {
                        -caret.size.height
                    },
            );
            if p.y >= cell.bounds.origin.y && p.y < cell.bounds.origin.y + cell.bounds.size.height {
                let byte = cell.projected.map.to_source(moved.head, moved.affinity);
                return Some(Selection {
                    anchor: byte,
                    head: byte,
                    preferred_x: Some(p.x + b.style.inset_left - self.source_offset(i).x),
                    ..moved
                });
            }
            let target = c
                .cells
                .iter()
                .filter(|other| {
                    if forward {
                        other.bounds.origin.y >= cell.bounds.origin.y + cell.bounds.size.height
                    } else {
                        other.bounds.origin.y + other.bounds.size.height <= cell.bounds.origin.y
                    }
                })
                .min_by(|a, b| distance(a.bounds, p).total_cmp(&distance(b.bounds, p)));
            if let Some(target) = target {
                let hit = target.flow.hit_test(
                    Point::new(
                        p.x - target.bounds.origin.x,
                        if forward {
                            0.5
                        } else {
                            target.bounds.size.height - 0.5
                        },
                    ),
                    target.bounds.size.width,
                    b.style.align.unwrap_or(align),
                );
                let byte = target.projected.map.to_source(hit.head, hit.affinity);
                return Some(Selection {
                    anchor: byte,
                    head: byte,
                    preferred_x: Some(p.x + b.style.inset_left - self.source_offset(i).x),
                    ..hit
                });
            }
        } else {
            let target = if forward {
                c.cells.get(at + 1)
            } else {
                at.checked_sub(1).and_then(|i| c.cells.get(i))
            };
            if let Some(target) = target {
                return Some(Selection::caret(if forward {
                    target.source.start
                } else {
                    target.source.end
                }));
            }
        }
        let _ = width;
        None
    }
}
