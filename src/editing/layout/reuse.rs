//! Reuse local layout inputs and measured heights independently of glyph eviction.
use super::*;
impl Engine {
    pub(super) fn reuse(
        &mut self,
        old: &Engine,
        changes: Option<&crate::editing::ChangeSet>,
    ) -> render::Result<()> {
        if !Arc::ptr_eq(&old.system, &self.system)
            || old.font_revision != self.font_revision
            || old.options.font != self.options.font
            || old.options.font_size != self.options.font_size
            || old.options.line_height != self.options.line_height
        {
            return Ok(());
        }
        self.reuse_heights(old, changes);
        for (old_i, cached) in &old.cache {
            // Equal object IDs alone do not establish equal layout inputs.
            // Changed props invalidate only paragraphs containing those widgets.
            if cached
                .objects
                .iter()
                .any(|(id, _, _)| self.descriptions.get(id) != old.descriptions.get(id))
            {
                continue;
            }
            let old_block = &old.blocks[*old_i];
            let source_start = changes.map_or(old_block.range.start, |c| {
                c.map(old_block.range.start, Bias::After)
            });
            let candidates = [
                self.block_at(source_start),
                Some(*old_i),
                self.blocks.len().checked_sub(old.blocks.len() - old_i),
            ];
            for i in candidates
                .into_iter()
                .flatten()
                .filter(|i| *i < self.blocks.len())
            {
                let b = &self.blocks[i];
                if b.style != old_block.style
                    || b.view != old_block.view
                    || b.empty_height != old_block.empty_height
                {
                    continue;
                }
                if b.view.is_some() {
                    if self.options.width == old.options.width
                        && self.same_block_inputs(b, old, old_block)
                        && b.body == old_block.body
                    {
                        self.cache.insert(i, cached.clone());
                        self.heights.set(
                            i,
                            cached.height + b.style.space_before + b.style.space_after,
                        );
                    }
                    break;
                }
                if self.options.width == old.options.width
                    && b.body == old_block.body
                    && self.projection.replacements.is_empty()
                    && old.projection.replacements.is_empty()
                    && self
                        .source
                        .equal_range(b.body.clone(), &old.source, old_block.body.clone())
                    && same_styles(
                        &self.styles,
                        b.body.clone(),
                        &old.styles,
                        old_block.body.clone(),
                        b.style.font.as_ref().unwrap_or(&self.options.font),
                    )
                    && local_styles(&self.projection, &b.body)
                        .eq(local_styles(&old.projection, &old_block.body))
                {
                    self.cache.insert(i, cached.clone());
                    self.heights.set(
                        i,
                        cached.height + b.style.space_before + b.style.space_after,
                    );
                    break;
                }
                let projected =
                    self.projection
                        .project(&self.source, b.body.clone(), &self.styles)?;
                let visually_equal = projected.spans == cached.projected.spans
                    || crate::core::rich_text::resolve_runs(
                        &projected.text,
                        &projected.spans,
                        0..projected.text.len(),
                        b.style.font.as_ref().unwrap_or(&self.options.font),
                    ) == crate::core::rich_text::resolve_runs(
                        &cached.projected.text,
                        &cached.projected.spans,
                        0..cached.projected.text.len(),
                        b.style.font.as_ref().unwrap_or(&self.options.font),
                    );
                if projected.text == cached.projected.text
                    && visually_equal
                    && projected.objects == cached.projected.objects
                {
                    if self.options.width == old.options.width && b.body == old_block.body {
                        self.cache.insert(i, cached.clone());
                        self.heights.set(
                            i,
                            cached.height + b.style.space_before + b.style.space_after,
                        );
                    } else if cached.cells.is_empty() && cached.objects.is_empty() {
                        let flow = TextFlow::from_paragraph(
                            cached.flow.paragraph().clone(),
                            self.flow_options(b),
                            self.system.clone(),
                        );
                        let empty = projected.text.is_empty();
                        let mut c = Cached {
                            window: None,
                            cells: Vec::new(),
                            decoration: None,
                            rules: cached.rules.clone(),
                            projected,
                            width: flow.size().width,
                            height: flow.size().height.max(if empty {
                                b.empty_height
                            } else {
                                0.0
                            }),
                            flow,
                            objects: cached.objects.clone(),
                        };
                        c.update_objects(
                            self.content_width(b),
                            b.style.align.unwrap_or(TextAlign::Left),
                        );
                        self.heights
                            .set(i, c.height + b.style.space_before + b.style.space_after);
                        self.cache.insert(i, Rc::new(c));
                    }
                    break;
                }
            }
        }
        Ok(())
    }
    /// A projection change elsewhere must not discard this block's measurement.
    /// Source positions are mapped separately from geometry, so inserting or
    /// folding earlier lines does not confuse block indices with identity.
    fn reuse_heights(&mut self, old: &Engine, changes: Option<&crate::editing::ChangeSet>) {
        if self.options.width != old.options.width {
            return;
        }
        for (old_i, height) in old.heights.measurements() {
            let previous = &old.blocks[old_i];
            let start = changes.map_or(previous.range.start, |c| {
                c.map(previous.range.start, Bias::After)
            });
            let end = changes.map_or(previous.body.end, |c| {
                c.map(
                    previous.body.end,
                    if previous.body.is_empty() {
                        Bias::After
                    } else {
                        Bias::Before
                    },
                )
            });
            let Some(i) = self.block_at(start) else {
                continue;
            };
            let block = &self.blocks[i];
            if block.range.start == start
                && block.body.end == end
                && self.same_block_inputs(block, old, previous)
            {
                self.heights.set(i, height);
            }
        }
    }

    fn same_block_inputs(&self, block: &Block, old: &Engine, previous: &Block) -> bool {
        block.style == previous.style
            && block.view == previous.view
            && block.empty_height == previous.empty_height
            && local_styles(&self.projection, &block.range)
                .eq(local_styles(&old.projection, &previous.range))
            && (block.view.is_none()
                || local_paragraphs(&self.projection, &block.range)
                    .eq(local_paragraphs(&old.projection, &previous.range)))
            && self
                .source
                .equal_range(block.range.clone(), &old.source, previous.range.clone())
            && same_styles(
                &self.styles,
                block.range.clone(),
                &old.styles,
                previous.range.clone(),
                block.style.font.as_ref().unwrap_or(&self.options.font),
            )
            && block
                .view
                .is_none_or(|id| self.descriptions.get(&id) == old.descriptions.get(&id))
            && local_replacements(&self.projection, &block.range)
                .eq(local_replacements(&old.projection, &previous.range))
    }
}

/// Projected overrides are separate from document styles. Compare their sparse
/// properties before taking a shortcut that skips constructing projected runs.
fn local_styles<'a>(
    projection: &'a Projection,
    range: &'a Range<usize>,
) -> impl Iterator<Item = (usize, usize, &'a crate::InlineStyle)> + 'a {
    let first = projection
        .styles
        .partition_point(|s| s.range.end <= range.start);
    projection.styles[first..]
        .iter()
        .take_while(move |s| s.range.start < range.end)
        .map(move |s| {
            (
                s.range.start.max(range.start) - range.start,
                s.range.end.min(range.end) - range.start,
                &s.style,
            )
        })
}

/// Source-backed cells can inherit line decorations inside their outer block.
/// Compare relative intersections so edits before a block do not evict it.
fn local_paragraphs<'a>(
    projection: &'a Projection,
    range: &'a Range<usize>,
) -> impl Iterator<Item = (usize, usize, &'a ParagraphStyle)> + 'a {
    projection
        .paragraphs
        .iter()
        .filter(move |p| p.range.start <= range.end && p.range.end >= range.start)
        .map(move |p| {
            (
                p.range.start.saturating_sub(range.start),
                p.range.end.min(range.end).saturating_sub(range.start),
                &p.style,
            )
        })
}

/// Compare canonical interval streams without materializing paragraphs or runs.
fn same_styles(
    a: &[StyleSpan],
    ar: Range<usize>,
    b: &[StyleSpan],
    br: Range<usize>,
    font: &Font,
) -> bool {
    if ar.len() != br.len() {
        return false;
    }
    let (mut i, mut j) = (
        a.partition_point(|s| s.range.end <= ar.start),
        b.partition_point(|s| s.range.end <= br.start),
    );
    let mut offset = 0;
    let default = crate::InlineStyle::default();
    while offset < ar.len() {
        let x = ar.start + offset;
        let y = br.start + offset;
        while i < a.len() && a[i].range.end <= x {
            i += 1;
        }
        while j < b.len() && b[j].range.end <= y {
            j += 1;
        }
        let sa = a.get(i).filter(|s| s.range.start <= x);
        let sb = b.get(j).filter(|s| s.range.start <= y);
        let va = sa.map_or(&default, |s| &s.style);
        let vb = sb.map_or(&default, |s| &s.style);
        if va != vb && va.run(0, font) != vb.run(0, font) {
            return false;
        }
        let na = sa
            .map_or_else(
                || a.get(i).map_or(ar.end, |s| s.range.start),
                |s| s.range.end,
            )
            .min(ar.end)
            - x;
        let nb = sb
            .map_or_else(
                || b.get(j).map_or(br.end, |s| s.range.start),
                |s| s.range.end,
            )
            .min(br.end)
            - y;
        offset += na.min(nb);
    }
    true
}

/// Compare source-relative inputs without copying projected text or reading
/// unrelated decorations. Include boundary annotations and children of tables.
fn local_replacements<'a>(
    projection: &'a Projection,
    range: &'a Range<usize>,
) -> impl Iterator<Item = (usize, usize, ViewId, &'a crate::editing::ReplacementContent)> + 'a {
    let first = projection
        .replacements
        .partition_point(|r| r.range.end < range.start);
    projection.replacements[first..]
        .iter()
        .take_while(move |r| r.range.start <= range.end)
        .filter(move |r| r.range.start >= range.start && r.range.end <= range.end)
        .map(move |r| {
            (
                r.range.start - range.start,
                r.range.end - range.start,
                r.id,
                &r.content,
            )
        })
}
