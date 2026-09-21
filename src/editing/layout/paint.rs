//! Visible text, decorations and embedded-view painting share display geometry.
use super::*;
impl EditorLayout {
    pub fn paint(
        &self,
        painter: &mut Painter<'_>,
        origin: Point<f32>,
        clip: Rect<f32>,
        width: f32,
        align: TextAlign,
        color: Hsla,
        selections: &[Range<usize>],
        selection_color: Hsla,
        selection_background: Hsla,
    ) -> render::Result<()> {
        let mut state = self.engine.borrow_mut();
        let Some(e) = state.as_mut() else {
            return Ok(());
        };
        let viewport = Rect::from_xywh(
            clip.origin.x - origin.x,
            clip.origin.y - origin.y,
            clip.size.width,
            clip.size.height,
        );
        e.prepare_visible(viewport, None)?;
        let first = e.heights.at(viewport.origin.y);
        let mut visible = BTreeSet::new();
        for i in first..e.blocks.len() {
            let y = e.heights.top(i);
            if y > viewport.origin.y + viewport.size.height {
                break;
            }
            e.ensure(i)?;
            let b = &e.blocks[i];
            let c = &e.cache[&i];
            let local_origin = Point::new(
                origin.x + b.style.inset_left,
                origin.y + y + b.style.space_before,
            );
            let w = e.content_width_at(b, width);
            if let Some(bg) = b.style.background {
                paint_rect(
                    painter,
                    Rect::from_xywh(
                        origin.x,
                        origin.y + y,
                        width,
                        c.height + b.style.space_before + b.style.space_after,
                    ),
                    bg,
                );
            }
            if let Some((thickness, fg)) = b.style.leading_rule {
                paint_rect(
                    painter,
                    Rect::from_xywh(origin.x, local_origin.y, thickness, c.height),
                    fg,
                );
            }
            let ranges: Vec<_> = selections
                .iter()
                .filter_map(|r| {
                    let a = r.start.max(b.body.start);
                    let z = r.end.min(b.body.end);
                    (a < z).then(|| {
                        c.projected.map.to_display(a, Bias::Before)
                            ..c.projected.map.to_display(z, Bias::After)
                    })
                })
                .collect();
            if c.decoration.is_some() {
                let id = b.view.expect("decorated layout has a block ID");
                visible.insert(id);
                e.views.borrow_mut().get(id)?.paint(
                    painter,
                    Rect::new(local_origin, Size::new(c.width, c.height)),
                )?;
            }
            let source_viewport = e.source_viewport(i);
            let offset = e.source_offset(i);
            let source_clip = source_viewport.map_or(clip, |v| {
                let left = (local_origin.x + v.bounds.origin.x).max(clip.origin.x);
                let top = (local_origin.y + v.bounds.origin.y).max(clip.origin.y);
                let right = (local_origin.x + v.bounds.origin.x + v.bounds.size.width)
                    .min(clip.origin.x + clip.size.width);
                let bottom = (local_origin.y + v.bounds.origin.y + v.bounds.size.height)
                    .min(clip.origin.y + clip.size.height);
                Rect::from_xywh(left, top, (right - left).max(0.0), (bottom - top).max(0.0))
            });
            let local_origin = Point::new(local_origin.x - offset.x, local_origin.y - offset.y);
            let clip = source_clip;
            painter.with_clip(
                render::Bounds::new(
                    render::point(render::px(clip.origin.x), render::px(clip.origin.y)),
                    render::size(render::px(clip.size.width), render::px(clip.size.height)),
                ),
                |painter| -> render::Result<()> {
                    for (r, fg) in &c.rules {
                        paint_rect(
                            painter,
                            Rect::from_xywh(
                                local_origin.x + r.origin.x,
                                local_origin.y + r.origin.y,
                                r.size.width,
                                r.size.height,
                            ),
                            *fg,
                        );
                    }
                    for cell in &c.cells {
                        let ranges: Vec<_> = selections
                            .iter()
                            .filter_map(|s| {
                                let a = s.start.max(cell.source.start);
                                let z = s.end.min(cell.source.end);
                                (a < z).then(|| {
                                    cell.projected.map.to_display(a, Bias::Before)
                                        ..cell.projected.map.to_display(z, Bias::After)
                                })
                            })
                            .collect();
                        cell.flow.paint(
                            painter,
                            Point::new(
                                local_origin.x + cell.bounds.origin.x,
                                local_origin.y + cell.bounds.origin.y,
                            ),
                            clip,
                            cell.bounds.size.width,
                            b.style.align.unwrap_or(align),
                            color,
                            &ranges,
                            selection_color,
                            selection_background,
                        )?;
                    }
                    c.flow.paint(
                        painter,
                        local_origin,
                        clip,
                        w,
                        b.style.align.unwrap_or(align),
                        color,
                        &ranges,
                        selection_color,
                        selection_background,
                    )?;
                    for (id, source, r) in c.object_bounds(w, b.style.align.unwrap_or(align)).iter()
                    {
                        visible.insert(*id);
                        let r = Rect::from_xywh(
                            local_origin.x + r.origin.x,
                            local_origin.y + r.origin.y,
                            r.size.width,
                            r.size.height,
                        );
                        e.views.borrow_mut().get(*id)?.paint(painter, r)?;
                        if selections
                            .iter()
                            .any(|s| s.start < source.end && s.end > source.start)
                        {
                            paint_rect(painter, r, selection_background);
                        }
                    }
                    Ok(())
                },
            )?;
        }
        e.views.borrow_mut().retain(&visible);
        e.trim();
        Ok(())
    }
}
