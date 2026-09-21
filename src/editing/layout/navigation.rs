//! Source-coordinate hit testing, selection and visual navigation.
use super::*;
impl EditorLayout {
    pub fn caret(
        &self,
        index: usize,
        affinity: Bias,
        width: f32,
        align: TextAlign,
    ) -> Option<Rect<f32>> {
        let mut e = self.engine.borrow_mut();
        let e = e.as_mut()?;
        let i = e.block_at(index)?;
        e.requested_position = Some(index);
        e.ensure(i).ok()?;
        let result = e.caret(i, index, affinity, width, align);
        e.trim();
        result
    }
    pub fn hit_test(&self, point: Point<f32>, width: f32, align: TextAlign) -> Selection {
        let mut e = self.engine.borrow_mut();
        let Some(e) = e.as_mut() else {
            return Selection::caret(0);
        };
        let i = e.heights.at(point.y);
        if e.ensure(i).is_err() {
            return Selection::caret(0);
        }
        let result = e.hit(i, point, width, align);
        e.trim();
        result
    }
    pub fn word_at(&self, point: Point<f32>, width: f32, align: TextAlign) -> Range<usize> {
        let mut e = self.engine.borrow_mut();
        let Some(e) = e.as_mut() else { return 0..0 };
        let i = e.heights.at(point.y);
        if e.ensure(i).is_err() {
            return 0..0;
        }
        let b = &e.blocks[i];
        let c = &e.cache[&i];
        let p = Point::new(
            point.x - b.style.inset_left + e.source_offset(i).x,
            point.y - e.heights.top(i) - b.style.space_before + e.source_offset(i).y,
        );
        if let Some(cell) = c
            .cells
            .iter()
            .min_by(|a, b| distance(a.bounds, p).total_cmp(&distance(b.bounds, p)))
        {
            let r = cell.flow.word_at(
                Point::new(p.x - cell.bounds.origin.x, p.y - cell.bounds.origin.y),
                cell.bounds.size.width,
                b.style.align.unwrap_or(align),
            );
            return cell.projected.map.to_source(r.start, Bias::Before)
                ..cell.projected.map.to_source(r.end, Bias::After);
        }
        let r = c.flow.word_at(
            p,
            e.content_width_at(b, width),
            b.style.align.unwrap_or(align),
        );
        c.projected.map.to_source(r.start, Bias::Before)
            ..c.projected.map.to_source(r.end, Bias::After)
    }
    pub fn hard_line_at(&self, point: Point<f32>, _: f32, _: TextAlign) -> Range<usize> {
        let e = self.engine.borrow();
        e.as_ref()
            .map_or(0..0, |e| e.blocks[e.heights.at(point.y)].range.clone())
    }
    pub fn selection_rectangles(
        &self,
        range: Range<usize>,
        width: f32,
        align: TextAlign,
    ) -> Vec<Rect<f32>> {
        let mut state = self.engine.borrow_mut();
        let Some(e) = state.as_mut() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        let first = e.blocks.partition_point(|b| b.range.end < range.start);
        for i in first..e.blocks.len() {
            if e.blocks[i].range.start >= range.end {
                break;
            }
            if e.ensure(i).is_err() {
                continue;
            }
            let b = &e.blocks[i];
            let c = &e.cache[&i];
            let a = c
                .projected
                .map
                .to_display(range.start.max(b.body.start), Bias::Before);
            let z = c
                .projected
                .map
                .to_display(range.end.min(b.body.end), Bias::After);
            out.extend(
                c.flow
                    .selection_rectangles(
                        a..z,
                        e.content_width_at(b, width),
                        b.style.align.unwrap_or(align),
                    )
                    .into_iter()
                    .map(|mut r| {
                        r.origin.x += b.style.inset_left - e.source_offset(i).x;
                        r.origin.y +=
                            e.heights.top(i) - e.source_offset(i).y + b.style.space_before;
                        r
                    }),
            );
            for cell in &c.cells {
                let a = range.start.max(cell.source.start);
                let z = range.end.min(cell.source.end);
                if a < z {
                    out.extend(
                        cell.flow
                            .selection_rectangles(
                                cell.projected.map.to_display(a, Bias::Before)
                                    ..cell.projected.map.to_display(z, Bias::After),
                                cell.bounds.size.width,
                                b.style.align.unwrap_or(align),
                            )
                            .into_iter()
                            .map(|mut r| {
                                r.origin.x += b.style.inset_left - e.source_offset(i).x
                                    + cell.bounds.origin.x;
                                r.origin.y += e.heights.top(i) - e.source_offset(i).y
                                    + b.style.space_before
                                    + cell.bounds.origin.y;
                                r
                            }),
                    );
                }
            }
            for (_, source, bounds) in c
                .object_bounds(e.content_width_at(b, width), b.style.align.unwrap_or(align))
                .iter()
            {
                if source.start < range.end && source.end > range.start {
                    let mut r = *bounds;
                    r.origin.x += b.style.inset_left - e.source_offset(i).x;
                    r.origin.y += e.heights.top(i) - e.source_offset(i).y + b.style.space_before;
                    out.push(r);
                }
            }
        }
        out
    }
    pub fn move_selection(
        &self,
        selection: Selection,
        motion: Motion,
        extend: bool,
        width: f32,
        align: TextAlign,
        viewport_height: f32,
    ) -> Selection {
        let mut state = self.engine.borrow_mut();
        let Some(e) = state.as_mut() else {
            return selection;
        };
        e.requested_position = Some(selection.head);
        let Some(i) = e.block_at(selection.head) else {
            return selection;
        };
        if e.ensure(i).is_err() {
            return selection;
        }
        if !extend && !selection.is_caret() && matches!(motion, Motion::Left | Motion::Right) {
            return Selection::caret(if motion == Motion::Left {
                selection.text_range().start
            } else {
                selection.text_range().end
            });
        }
        if let Some(next) = e.move_in_cells(i, selection, motion, width, align, viewport_height) {
            return if extend {
                Selection {
                    anchor: selection.anchor,
                    ..next
                }
            } else {
                next
            };
        }
        let mut next = match motion {
            Motion::DocumentStart => Selection::caret(0),
            Motion::DocumentEnd => Selection::caret(e.source.len()),
            Motion::Up | Motion::Down | Motion::PageUp | Motion::PageDown => {
                let caret = e
                    .caret(i, selection.head, selection.affinity, width, align)
                    .unwrap();
                let x = selection.preferred_x.unwrap_or(caret.origin.x);
                let down = matches!(motion, Motion::Down | Motion::PageDown);
                let b = &e.blocks[i];
                let c = &e.cache[&i];
                let local = Selection {
                    anchor: c.projected.map.to_display(selection.anchor, Bias::Before),
                    head: c
                        .projected
                        .map
                        .to_display(selection.head, selection.affinity),
                    preferred_x: Some(x - b.style.inset_left),
                    ..selection
                };
                let moved = c.flow.move_selection(
                    local,
                    motion,
                    false,
                    e.content_width_at(b, width),
                    b.style.align.unwrap_or(align),
                    viewport_height,
                );
                let target = c.projected.map.to_source(moved.head, moved.affinity);
                let paragraph = c.flow.paragraph();
                let cursor = render::parley::Cursor::from_byte_index(
                    paragraph.layout(),
                    paragraph.layout_index(local.head),
                    if local.affinity == Bias::Before {
                        render::parley::Affinity::Upstream
                    } else {
                        render::parley::Affinity::Downstream
                    },
                );
                let row = paragraph.cursor_row(cursor);
                let crossed = matches!(motion, Motion::Up | Motion::Down)
                    && (if down {
                        row + 1 >= paragraph.line_count()
                    } else {
                        row == 0
                    });
                let mut next = if target != selection.head && !crossed {
                    Selection {
                        head: target,
                        anchor: target,
                        affinity: moved.affinity,
                        preferred_x: Some(x),
                    }
                } else {
                    let j = if down {
                        i.checked_add(1)
                    } else {
                        i.checked_sub(1)
                    };
                    if let Some(j) = j.filter(|j| *j < e.blocks.len()) {
                        if e.ensure(j).is_ok() {
                            let b = &e.blocks[j];
                            let c = &e.cache[&j];
                            let y = e.heights.top(j)
                                + b.style.space_before
                                + if down { 0.5 } else { c.height - 0.5 };
                            e.hit(j, Point::new(x, y), width, align)
                        } else {
                            selection
                        }
                    } else {
                        Selection::caret(if down { e.source.len() } else { 0 })
                    }
                };
                next.preferred_x = Some(x);
                next
            }
            _ => {
                let b = &e.blocks[i];
                let c = &e.cache[&i];
                let forward = matches!(motion, Motion::Right | Motion::WordRight);
                if let Some(r) = e.projection.replacements.iter().find(|r| {
                    !r.range.is_empty()
                        && if forward {
                            r.range.start <= selection.head && selection.head < r.range.end
                        } else {
                            r.range.start < selection.head && selection.head <= r.range.end
                        }
                }) {
                    Selection::caret(if forward { r.range.end } else { r.range.start })
                } else if b.view.is_some() {
                    Selection::caret(if forward { b.range.end } else { b.range.start })
                } else {
                    let local = Selection {
                        anchor: c.projected.map.to_display(selection.anchor, Bias::Before),
                        head: c
                            .projected
                            .map
                            .to_display(selection.head, selection.affinity),
                        ..selection
                    };
                    let mut moved = c.flow.move_selection(
                        local,
                        motion,
                        false,
                        e.content_width_at(b, width),
                        b.style.align.unwrap_or(align),
                        viewport_height,
                    );
                    let mut target = c.projected.map.to_source(moved.head, moved.affinity);
                    if target == selection.head
                        && matches!(
                            motion,
                            Motion::Left | Motion::Right | Motion::WordLeft | Motion::WordRight
                        )
                    {
                        if forward && selection.head >= b.body.end && i + 1 < e.blocks.len() {
                            target = e.blocks[i + 1].body.start;
                        } else if !forward && selection.head <= b.body.start && i > 0 {
                            target = e.blocks[i - 1].body.end;
                        }
                    }
                    moved.head = target;
                    moved.anchor = target;
                    moved
                }
            }
        };
        if extend {
            next.anchor = selection.anchor;
        }
        next
    }
    pub fn object_at(
        &self,
        point: Point<f32>,
        width: f32,
        align: TextAlign,
    ) -> Option<(ViewId, Range<usize>, Rect<f32>)> {
        let mut state = self.engine.borrow_mut();
        let e = state.as_mut()?;
        let i = e.heights.at(point.y);
        e.ensure(i).ok()?;
        // Relative offsets may put a view over a neighboring text row. Use
        // retained visual bounds in reverse paint order, without shaping or
        // walking the document just to answer a pointer query.
        for (&i, c) in e.cache.iter().rev() {
            if c.objects.is_empty() {
                continue;
            }
            let b = &e.blocks[i];
            if let Some(v) = e.source_viewport(i) {
                let local = Point::new(
                    point.x - b.style.inset_left,
                    point.y - e.heights.top(i) - b.style.space_before,
                );
                if !contains(v.bounds, local) {
                    continue;
                }
            }
            for (id, source, r) in c
                .object_bounds(e.content_width_at(b, width), b.style.align.unwrap_or(align))
                .iter()
                .rev()
            {
                let mut r = *r;
                r.origin.x += b.style.inset_left - e.source_offset(i).x;
                r.origin.y += e.heights.top(i) - e.source_offset(i).y + b.style.space_before;
                if contains(r, point) && e.views.borrow_mut().get(*id).ok()?.pointer_events() {
                    return Some((*id, source.clone(), r));
                }
            }
        }
        None
    }
}
