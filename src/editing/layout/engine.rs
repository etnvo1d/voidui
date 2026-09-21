//! Source block indexing, cache reuse and viewport scheduling.
use super::*;
impl Engine {
    pub(super) fn index_blocks(&mut self) -> render::Result<()> {
        let mut ranges = Vec::new();
        let mut line_ranges = self.source.line_ranges().peekable();
        while let Some(mut range) = line_ranges.next() {
            // Folding across paragraphs groups those source lines into one flow.
            loop {
                let upto = self
                    .projection
                    .replacements
                    .partition_point(|r| r.range.start < range.end);
                let replacement = self
                    .projection
                    .replacements
                    .get(upto.saturating_sub(1))
                    .filter(|r| {
                        upto > 0 && r.range.end >= range.end && range.end < self.source.len()
                    });
                let mut end = replacement.map(|r| r.range.end);
                let upto = self
                    .projection
                    .blocks
                    .partition_point(|b| b.range.start <= range.start);
                let block = self
                    .projection
                    .blocks
                    .get(upto.saturating_sub(1))
                    .filter(|b| upto > 0 && b.range.end > range.end);
                if let Some(block) = block {
                    end = Some(end.unwrap_or(0).max(block.range.end.saturating_sub(1)));
                }
                let Some(end) = end else {
                    break;
                };
                while range.end <= end {
                    if let Some(next) = line_ranges.next() {
                        range.end = next.end;
                    } else {
                        break;
                    }
                }
            }
            ranges.push(range);
        }
        drop(line_ranges);
        self.blocks = ranges.into_iter().map(|r| self.make_block(r)).collect();
        self.reset_estimates();
        Ok(())
    }
    fn make_block(&self, range: Range<usize>) -> Block {
        let mut tail = range.end.saturating_sub(4).max(range.start);
        while !self.source.is_char_boundary(tail) {
            tail += 1;
        }
        let separator =
            crate::editing::buffer::separator_len(&self.source.read(tail..range.end).unwrap());
        let body = range.start..range.end - separator;
        let style = self.projection.paragraph_style(range.start);
        let empty_height = Self::empty_height(
            &self.source,
            &self.styles,
            range.start,
            style.line_height.unwrap_or(self.options.line_height),
        );
        let view = self
            .projection
            .blocks
            .iter()
            .find(|b| b.range.start <= range.start && b.range.end >= body.end)
            .map(|b| b.id);
        Block {
            style,
            range,
            body,
            view,
            empty_height,
        }
    }
    pub(super) fn empty_height(
        source: &TextSnapshot,
        styles: &[StyleSpan],
        start: usize,
        fallback: f32,
    ) -> f32 {
        let byte = start.min(source.len().saturating_sub(1));
        let at = styles.partition_point(|s| s.range.end <= byte);
        styles
            .get(at)
            .filter(|s| s.range.contains(&byte))
            .and_then(|s| s.style.line_height)
            .unwrap_or(fallback)
    }
    /// Re-index only the changed hard-line window. Stable prefix/suffix metadata
    /// is remapped without reading or shaping its source text.
    pub(super) fn index_changed(
        &mut self,
        old: &Engine,
        changes: &crate::editing::ChangeSet,
    ) -> render::Result<bool> {
        if !self.projection.replacements.is_empty()
            || !old.projection.replacements.is_empty()
            || !self.projection.blocks.is_empty()
            || !old.projection.blocks.is_empty()
        {
            return Ok(false);
        }
        let Some(first_edit) = changes.edits().first() else {
            return Ok(false);
        };
        let last_edit = changes.edits().last().unwrap();
        let a = old.block_at(first_edit.range.start).unwrap_or(0);
        let z = old
            .block_at(last_edit.range.end)
            .unwrap_or(old.blocks.len() - 1);
        let start = changes.map(old.blocks[a].range.start, Bias::Before);
        let end = changes.map(old.blocks[z].range.end, Bias::After);
        self.blocks.extend(old.blocks[..a].iter().cloned());
        let first = self.source.line_at(start);
        let mut line = first;
        while let Some(range) = self.source.line_range(line) {
            if range.start >= end && !(end == self.source.len() && range.is_empty()) {
                break;
            }
            self.blocks.push(self.make_block(range));
            line += 1;
        }
        self.blocks.extend(old.blocks[z + 1..].iter().map(|b| {
            let mut b = b.clone();
            b.range =
                changes.map(b.range.start, Bias::After)..changes.map(b.range.end, Bias::After);
            b.body = changes.map(b.body.start, Bias::After)..changes.map(b.body.end, Bias::After);
            b
        }));
        if self.blocks.is_empty() {
            self.blocks.push(self.make_block(0..0));
        }
        // Source edits can move or change line decorations outside the edited
        // text window. Refresh their metadata without materializing the text.
        for b in &mut self.blocks {
            b.style = self.projection.paragraph_style(b.range.start);
            b.empty_height = Self::empty_height(
                &self.source,
                &self.styles,
                b.range.start,
                b.style.line_height.unwrap_or(self.options.line_height),
            );
        }
        self.reset_estimates();
        Ok(true)
    }
    pub(super) fn reset_estimates(&mut self) {
        self.heights = HeightIndex::new(
            self.blocks
                .iter()
                .map(|b| {
                    let w = self.content_width(b);
                    let font_size = b.style.font_size.unwrap_or(self.options.font_size);
                    let line_height = b.style.line_height.unwrap_or(self.options.line_height);
                    let lines = if self.options.width.is_some() {
                        (b.body.len() as f32 * font_size * self.virtual_options.estimated_advance
                            / w.max(1.0))
                        .ceil()
                        .max(1.0)
                    } else {
                        1.0
                    };
                    lines * line_height + b.style.space_before + b.style.space_after
                })
                .collect(),
        );
    }
    pub(super) fn content_width(&self, b: &Block) -> f32 {
        self.content_width_at(b, self.options.width.unwrap_or(f32::MAX))
    }
    pub(super) fn content_width_at(&self, b: &Block, width: f32) -> f32 {
        (width - b.style.inset_left - b.style.inset_right).max(0.0)
    }
    pub(super) fn flow_options(&self, b: &Block) -> LayoutOptions {
        LayoutOptions {
            width: self.options.width.map(|_| self.content_width(b)),
            ..self.options.for_paragraph(&b.style)
        }
    }
    fn mount_decoration(&self, i: usize, cached: &Cached) -> render::Result<()> {
        if let Some(id) = self.blocks[i].view {
            let mut host = self.views.borrow_mut();
            if let Some(description) = &cached.decoration {
                host.descriptions.insert(id, description.clone());
                let changed = host.needs_update(id);
                if changed || host.get(id)?.needs_layout() {
                    host.measure(id, cached.width, &TextLayoutCache::new(self.system.clone()))?;
                }
            } else if !self.descriptions.contains_key(&id)
                && host.factories.block_layout(id).is_some()
            {
                // Removing a decoration must release its focus and pointer even
                // when the source-backed block itself remains in the projection.
                host.remove(id);
                host.descriptions.remove(&id);
            }
        }
        Ok(())
    }
    pub(super) fn ensure(&mut self, i: usize) -> render::Result<()> {
        if let Some(cached) = self.cache.get(&i) {
            self.mount_decoration(i, cached)?;
            let mut refresh = cached.window.is_some()
                && (cached.window != self.viewport
                    || self.requested_position.is_some_and(|p| {
                        !cached
                            .cells
                            .iter()
                            .any(|c| c.source.start <= p && p <= c.source.end)
                    }));
            let cache = TextLayoutCache::new(self.system.clone());
            let width = self.content_width(&self.blocks[i]);
            for (id, _, bounds) in &cached.objects {
                let mut views = self.views.borrow_mut();
                let changed = views.needs_update(*id);
                let needs_layout = views.get(*id)?.needs_layout();
                if changed || needs_layout {
                    let metrics = views.measure(*id, width, &cache)?;
                    // Remounted/updated views can change their baseline even if
                    // their bounding box has not changed.
                    refresh |= changed || needs_layout || metrics.size != bounds.size;
                }
            }
            if !refresh {
                return Ok(());
            }
            self.cache.remove(&i);
        }
        let b = &self.blocks[i];
        let projected = if b.view.is_some() {
            ProjectedText::empty(b.body.clone())
        } else {
            self.projection
                .project(&self.source, b.body.clone(), &self.styles)?
        };
        let w = self.content_width(b);
        let options = self.flow_options(b);
        let cache = TextLayoutCache::new(self.system.clone());
        if let Some(provider) = b
            .view
            .filter(|id| !self.descriptions.contains_key(id))
            .and_then(|id| self.views.borrow().factories.block_layout(id))
        {
            let mut measure = CellMeasure {
                composition_cells: b
                    .view
                    .and_then(|id| self.composition_cells.get(&id).map(Vec::as_slice)),
                source: &self.source,
                styles: &self.styles,
                projection: &self.projection,
                options: &options,
                system: self.system.clone(),
                views: self.views.clone(),
                width: w,
                viewport: self.viewport.map(|v| {
                    Rect::from_xywh(
                        v.origin.x,
                        v.origin.y - self.heights.top(i),
                        v.size.width,
                        v.size.height,
                    )
                }),
                cache: BTreeMap::new(),
                required_position: self.requested_position,
            };
            let arrangement = provider.layout(b.range.clone(), &mut measure)?;
            anyhow::ensure!(
                arrangement.size.width.is_finite()
                    && arrangement.size.width >= 0.0
                    && arrangement.size.height.is_finite()
                    && arrangement.size.height > 0.0,
                "invalid block arrangement size"
            );
            let mut cells = Vec::new();
            let mut child_objects = Vec::new();
            for cell in arrangement.cells {
                anyhow::ensure!(
                    cell.source.start >= b.body.start
                        && cell.source.end <= b.range.end
                        && cell.bounds.origin.x.is_finite()
                        && cell.bounds.origin.y.is_finite()
                        && cell.bounds.size.width.is_finite()
                        && cell.bounds.size.width >= 0.0,
                    "invalid source-backed cell"
                );
                measure.measure(cell.source.clone(), cell.bounds.size.width)?;
                let mut cached = measure
                    .cache
                    .remove(&(
                        cell.source.start,
                        cell.source.end,
                        cell.bounds.size.width.to_bits(),
                    ))
                    .unwrap();
                cached.bounds = cell.bounds;
                for (id, _, r) in cached.flow.paragraph().inline_boxes(
                    cell.bounds.size.width,
                    b.style.align.unwrap_or(TextAlign::Left),
                ) {
                    if let Some(object) = cached.projected.objects.iter().find(|o| o.id.0 == id) {
                        child_objects.push((
                            object.id,
                            object.source.clone(),
                            Rect::from_xywh(
                                cell.bounds.origin.x + r.origin.x,
                                cell.bounds.origin.y + r.origin.y,
                                r.size.width,
                                r.size.height,
                            ),
                        ));
                    }
                }
                cells.push(Rc::new(cached));
            }
            let empty = self.system.shape_paragraph(
                "".into(),
                &[],
                options.font_size,
                options.line_height,
                None,
                None,
            )?;
            let cached = Cached {
                window: if arrangement.viewport_dependent {
                    self.viewport
                } else {
                    None
                },
                cells,
                rules: arrangement.rules,
                decoration: arrangement.decoration,
                projected,
                flow: TextFlow::from_paragraph(empty, self.flow_options(b), self.system.clone()),
                width: arrangement.size.width,
                height: arrangement.size.height,
                objects: child_objects,
            };
            self.heights.set(
                i,
                cached.height + b.style.space_before + b.style.space_after,
            );
            self.mount_decoration(i, &cached)?;
            self.cache.insert(i, Rc::new(cached));
            return Ok(());
        }
        let mut boxes = Vec::new();
        let mut objects = Vec::new();
        let mut view_height = 0.0;
        if let Some(id) = b.view {
            let metrics = self.views.borrow_mut().measure(id, w, &cache)?;
            objects.push((
                id,
                b.range.clone(),
                Rect::from_xywh(0.0, 0.0, metrics.size.width, metrics.size.height),
            ));
            view_height = metrics.size.height;
        } else {
            for object in &projected.objects {
                let metrics = self.views.borrow_mut().measure(object.id, w, &cache)?;
                boxes.push(render::InlineTextBox {
                    id: object.id.0,
                    index: object.range.start,
                    width: metrics.size.width,
                    height: metrics.size.height,
                    baseline: metrics.baseline,
                    align: metrics.align,
                    offset_em: metrics.offset_em,
                });
            }
        }
        let text = if b.view.is_some() {
            ""
        } else {
            &projected.text
        };
        let runs = if b.view.is_some() {
            Vec::new()
        } else {
            crate::core::rich_text::resolve_runs(
                text,
                &projected.spans,
                0..text.len(),
                &options.font,
            )
        };
        let runs = if runs.is_empty() {
            vec![TextRun {
                len: text.len(),
                font: options.font.clone(),
                color: render::black(),
                ..Default::default()
            }]
        } else {
            runs
        };
        let paragraph = self.system.shape_inline_paragraph(
            text.to_owned().into(),
            &runs,
            render::InlineTextStyle {
                font: &options.font,
                font_size: options.font_size,
                line_height: options.line_height,
            },
            self.options.width.map(|_| w),
            None,
            &boxes,
            b.style.first_line_indent,
        )?;
        let flow = TextFlow::from_paragraph(paragraph, options, self.system.clone());
        let mut cached = Cached {
            window: None,
            cells: Vec::new(),
            decoration: None,
            rules: Vec::new(),
            height: flow
                .size()
                .height
                .max(if text.is_empty() { b.empty_height } else { 0.0 })
                .max(view_height),
            width: flow
                .size()
                .width
                .max(objects.first().map_or(0.0, |(_, _, r)| r.size.width)),
            flow,
            projected,
            objects,
        };
        if b.view.is_none() {
            cached.update_objects(w, b.style.align.unwrap_or(TextAlign::Left));
        }
        self.heights.set(
            i,
            cached.height + b.style.space_before + b.style.space_after,
        );
        self.cache.insert(i, Rc::new(cached));
        Ok(())
    }
    pub(super) fn block_at(&self, byte: usize) -> Option<usize> {
        if byte > self.source.len() || self.blocks.is_empty() {
            return None;
        }
        Some(
            self.blocks
                .partition_point(|b| b.range.start <= byte)
                .saturating_sub(1),
        )
    }
    pub(super) fn size(&self) -> Size<f32> {
        Size::new(
            self.cache
                .iter()
                .map(|(i, c)| {
                    c.width + self.blocks[*i].style.inset_left + self.blocks[*i].style.inset_right
                })
                .fold(0.0, f32::max),
            self.heights.total(),
        )
    }
    pub(super) fn source_viewport(&self, i: usize) -> Option<crate::editing::SourceViewport> {
        self.cache.get(&i)?.decoration.as_ref()?;
        let id = self.blocks[i].view?;
        self.views.borrow().views.get(&id)?.view.source_viewport()
    }
    pub(super) fn source_offset(&self, i: usize) -> Point<f32> {
        self.source_viewport(i)
            .map_or(Point::default(), |v| v.offset)
    }
    pub(super) fn caret(
        &self,
        i: usize,
        byte: usize,
        bias: Bias,
        width: f32,
        align: TextAlign,
    ) -> Option<Rect<f32>> {
        let offset = self.source_offset(i);
        let b = &self.blocks[i];
        let c = &self.cache[&i];
        let object_bounds = c.object_bounds(
            self.content_width_at(b, width),
            b.style.align.unwrap_or(align),
        );
        let w = self.content_width_at(b, width);
        let local = c.projected.map.to_display(byte.min(b.body.end), bias);
        // An annotation's empty source range names both sides of the view. Let
        // upstream carets use the preceding text's geometry; downstream carets
        // still use the view's trailing edge, including when it ends a paragraph.
        // Objects replacing source text retain their atomic boundary carets.
        if let Some((_, r, bounds)) = object_bounds.iter().find(|(_, r, _)| {
            (r.start == byte || r.end == byte) && (!r.is_empty() || bias == Bias::After)
        }) {
            return Some(Rect::from_xywh(
                b.style.inset_left - offset.x
                    + bounds.origin.x
                    + if byte == r.end {
                        bounds.size.width
                    } else {
                        0.0
                    },
                self.heights.top(i) + b.style.space_before + bounds.origin.y - offset.y,
                1.0,
                bounds.size.height,
            ));
        }
        if let Some(cell) = c
            .cells
            .iter()
            .find(|cell| cell.source.start <= byte && byte <= cell.source.end)
        {
            let local = cell.projected.map.to_display(byte, bias);
            let mut r = cell.flow.caret(
                local,
                bias,
                cell.bounds.size.width,
                b.style.align.unwrap_or(align),
            )?;
            r.origin.x += b.style.inset_left - offset.x + cell.bounds.origin.x;
            r.origin.y +=
                self.heights.top(i) - offset.y + b.style.space_before + cell.bounds.origin.y;
            return Some(r);
        }
        let mut r = c
            .flow
            .caret(local, bias, w, b.style.align.unwrap_or(align))
            .unwrap_or(Rect::from_xywh(0.0, 0.0, 1.0, c.height));
        if c.projected.text.is_empty() {
            r.size.height = c.height;
        }
        r.origin.x += b.style.inset_left - offset.x;
        r.origin.y += self.heights.top(i) - offset.y + b.style.space_before;
        Some(r)
    }
    pub(super) fn hit(
        &self,
        i: usize,
        point: Point<f32>,
        width: f32,
        align: TextAlign,
    ) -> Selection {
        let b = &self.blocks[i];
        let c = &self.cache[&i];
        let object_bounds = c.object_bounds(
            self.content_width_at(b, width),
            b.style.align.unwrap_or(align),
        );
        let point = Point::new(
            point.x - b.style.inset_left + self.source_offset(i).x,
            point.y - self.heights.top(i) - b.style.space_before + self.source_offset(i).y,
        );
        for (_, source, r) in object_bounds.iter() {
            if contains(*r, point) {
                return Selection::caret(if point.x < r.origin.x + r.size.width * 0.5 {
                    source.start
                } else {
                    source.end
                });
            }
        }
        if let Some(cell) = c
            .cells
            .iter()
            .min_by(|a, b| distance(a.bounds, point).total_cmp(&distance(b.bounds, point)))
        {
            let hit = cell.flow.hit_test(
                Point::new(
                    point.x - cell.bounds.origin.x,
                    point.y - cell.bounds.origin.y,
                ),
                cell.bounds.size.width,
                b.style.align.unwrap_or(align),
            );
            let byte = cell.projected.map.to_source(hit.head, hit.affinity);
            return Selection {
                head: byte,
                anchor: byte,
                ..hit
            };
        }
        let local = c.flow.hit_test(
            point,
            self.content_width_at(b, width),
            b.style.align.unwrap_or(align),
        );
        let byte = c.projected.map.to_source(local.head, local.affinity);
        Selection {
            head: byte,
            anchor: byte,
            ..local
        }
    }
    pub(super) fn prepare_visible(
        &mut self,
        viewport: Rect<f32>,
        anchor: Option<ScrollAnchor>,
    ) -> render::Result<f32> {
        if self.blocks.is_empty() {
            return Ok(viewport.origin.y);
        }
        let anchor = anchor.unwrap_or_else(|| self.anchor_for(viewport));
        // A compound block can be much taller than its fresh estimate. Measure
        // the source anchor before locating visible indices; otherwise a viewport
        // inside a table can skip the table and anchor itself to following text.
        if let ScrollAnchor::Block { position, .. } = anchor
            && let Some(i) = self.block_at(position)
        {
            self.ensure(i)?;
        }
        self.requested_position = None;
        let mut y = anchor.resolve(self, viewport);
        for _ in 0..=self.blocks.len() {
            self.viewport = Some(Rect {
                origin: Point::new(viewport.origin.x, y),
                ..viewport
            });
            let first = self
                .heights
                .at((y - self.virtual_options.overscan).max(0.0));
            let mut i = first;
            while i < self.blocks.len()
                && self.heights.top(i) <= y + viewport.size.height + self.virtual_options.overscan
            {
                self.ensure(i)?;
                i += 1;
            }
            let next = anchor.resolve(self, viewport);
            if (next - y).abs() < 0.1 {
                break;
            }
            y = next;
        }
        self.viewport = Some(Rect {
            origin: Point::new(viewport.origin.x, y),
            ..viewport
        });
        self.scroll_anchor = self.viewport.map(|v| self.capture_anchor(v));
        self.trim();
        let first = self
            .heights
            .at((y - self.virtual_options.overscan).max(0.0));
        let last = self
            .heights
            .at(y + viewport.size.height + self.virtual_options.overscan);
        let visible = self
            .cache
            .range(first..=last)
            .flat_map(|(i, c)| {
                c.objects
                    .iter()
                    .map(|(id, _, _)| *id)
                    .chain(c.decoration.as_ref().and(self.blocks[*i].view).into_iter())
            })
            .collect();
        let mut views = self.views.borrow_mut();
        views.retain_present(&self.present_views);
        views.retain(&visible);
        Ok(y)
    }
    pub(super) fn trim(&mut self) {
        let Some(v) = self.viewport else {
            return;
        };
        if self.cache.len() <= self.virtual_options.max_cached_blocks {
            return;
        }
        let a = self
            .heights
            .at((v.origin.y - self.virtual_options.overscan).max(0.0));
        let z = self
            .heights
            .at(v.origin.y + v.size.height + self.virtual_options.overscan);
        let mut candidates: Vec<_> = self
            .cache
            .keys()
            .copied()
            .filter(|i| *i < a || *i > z)
            // Captured widgets still need current geometry outside the viewport.
            .filter(|i| {
                let host = self.views.borrow();
                !self.cache[i]
                    .objects
                    .iter()
                    .any(|(id, _, _)| Some(*id) == host.pointer || Some(*id) == host.focused)
            })
            .collect();
        candidates.sort_by_key(|i| std::cmp::Reverse(i.abs_diff(a)));
        for i in candidates {
            if self.cache.len() <= self.virtual_options.max_cached_blocks {
                break;
            }
            self.cache.remove(&i);
        }
    }
}
