//! Viewport layout over source-backed blocks. Projection, native text flow and
//! embedded views share one source-coordinate selection and input interface.
use crate::editing::{
    Bias, EditorViews, ParagraphStyle, ProjectedText, Projection, ProjectionSnapshot, Selection,
    TextRead, TextSnapshot, ViewId,
};
use crate::editing::{flow::TextFlow, height_index::HeightIndex, views::MountedViews};
use crate::{
    StyleSpan,
    core::geometry::{Point, Rect, Size},
    render::{self, Font, Hsla, Painter, TextAlign, TextLayoutCache, TextRun, TextSystem},
};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    ops::Range,
    rc::Rc,
    sync::Arc,
};
#[derive(Debug, Clone, PartialEq)]
pub struct LayoutOptions {
    pub font: Font,
    pub font_size: f32,
    pub line_height: f32,
    pub width: Option<f32>,
}
impl LayoutOptions {
    /// Resolve line-container typography before shaping its inline children.
    /// Keeping this independent of visible text preserves concealed/empty rows.
    fn for_paragraph(&self, style: &ParagraphStyle) -> Self {
        Self {
            font: style.font.clone().unwrap_or_else(|| self.font.clone()),
            font_size: style.font_size.unwrap_or(self.font_size),
            line_height: style.line_height.unwrap_or(self.line_height),
            width: self.width,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Motion {
    Left,
    Right,
    WordLeft,
    WordRight,
    Up,
    Down,
    LineStart,
    LineEnd,
    DocumentStart,
    DocumentEnd,
    PageUp,
    PageDown,
}
#[derive(Debug, Clone, Copy)]
pub struct ViewportOptions {
    pub overscan: f32,
    pub max_cached_blocks: usize,
    /// Estimated average advance, as a multiple of font size, for unseen rows.
    pub estimated_advance: f32,
}
impl Default for ViewportOptions {
    fn default() -> Self {
        Self {
            overscan: 300.0,
            max_cached_blocks: 128,
            estimated_advance: 0.5,
        }
    }
}
#[derive(Debug, Clone, Copy, Default)]
pub struct LayoutStats {
    pub blocks: usize,
    pub cached_blocks: usize,
    pub mounted_views: usize,
    pub estimated_height: f32,
}
#[derive(Clone)]
struct Block {
    range: Range<usize>,
    body: Range<usize>,
    style: ParagraphStyle,
    view: Option<ViewId>,
    empty_height: f32,
}
struct CellFlow {
    source: Range<usize>,
    bounds: Rect<f32>,
    projected: ProjectedText,
    flow: TextFlow,
}
struct Cached {
    window: Option<Rect<f32>>,
    cells: Vec<Rc<CellFlow>>,
    rules: Vec<(Rect<f32>, Hsla)>,
    decoration: Option<crate::editing::ViewSpec>,
    projected: ProjectedText,
    flow: TextFlow,
    width: f32,
    height: f32,
    objects: Vec<(ViewId, Range<usize>, Rect<f32>)>,
}
#[derive(Clone)]
struct Engine {
    composing: bool,
    composition_cells: BTreeMap<ViewId, Vec<crate::editing::TextCell>>,
    source: TextSnapshot,
    revision: u64,
    document_id: u64,
    styles: Arc<[StyleSpan]>,
    projection: Projection,
    descriptions: BTreeMap<ViewId, crate::editing::ViewSpec>,
    present_views: BTreeSet<ViewId>,
    blocks: Vec<Block>,
    heights: HeightIndex,
    cache: BTreeMap<usize, Rc<Cached>>,
    options: LayoutOptions,
    system: Arc<TextSystem>,
    font_revision: u64,
    viewport: Option<Rect<f32>>,
    scroll_anchor: Option<ScrollAnchor>,
    requested_position: Option<usize>,
    virtual_options: ViewportOptions,
    views: Rc<RefCell<MountedViews>>,
}
#[derive(Default)]
pub struct EditorLayout {
    engine: RefCell<Option<Engine>>,
    views: Rc<RefCell<MountedViews>>,
}
impl EditorLayout {
    /// Capture the effective display plan for a pointer selection gesture. Keep
    /// source-coordinate hit testing stable until the gesture releases the plan.
    pub(crate) fn projection_snapshot(&self) -> Option<Projection> {
        self.engine.borrow().as_ref().map(|e| e.projection.clone())
    }
    pub fn configure_views(
        &mut self,
        views: EditorViews,
        invalidate: Option<crate::core::updates::WidgetInvalidator>,
    ) {
        let mut host = self.views.borrow_mut();
        if !host.factories.same(&views) {
            // Registry changes do not affect widgets owned by the projection.
            let registered: Vec<_> = host
                .views
                .keys()
                .filter(|id| !host.descriptions.contains_key(id))
                .copied()
                .collect();
            for id in registered {
                host.remove(id);
            }
            host.factories = views;
            if let Some(e) = self.engine.get_mut() {
                e.cache.clear();
                e.heights.forget_measurements();
            }
        }
        host.invalidate = invalidate;
    }
    pub fn stats(&self) -> LayoutStats {
        self.engine
            .borrow()
            .as_ref()
            .map_or(LayoutStats::default(), |e| LayoutStats {
                blocks: e.blocks.len(),
                cached_blocks: e.cache.len(),
                mounted_views: e.views.borrow().views.len(),
                estimated_height: e.heights.total(),
            })
    }
    pub fn size(&self) -> Size<f32> {
        self.engine
            .borrow()
            .as_ref()
            .map_or(Size::default(), Engine::size)
    }
    pub fn paragraph_count(&self) -> usize {
        self.engine.borrow().as_ref().map_or(0, |e| e.blocks.len())
    }
    pub fn line_height(&self) -> f32 {
        self.engine
            .borrow()
            .as_ref()
            .map_or(0.0, |e| e.options.line_height)
    }
    pub fn prepare(
        &mut self,
        text: &str,
        options: LayoutOptions,
        system: Arc<TextSystem>,
    ) -> render::Result<()> {
        self.prepare_styled(text, &[], options, system)
    }
    pub fn prepare_styled(
        &mut self,
        text: &str,
        spans: &[StyleSpan],
        options: LayoutOptions,
        system: Arc<TextSystem>,
    ) -> render::Result<()> {
        crate::core::rich_text::validate_spans(text, spans)?;
        self.prepare_snapshot(
            ProjectionSnapshot {
                composing: false,
                document_id: 0,
                revision: 0,
                change: None,
                text: TextSnapshot::from_text(text),
                spans: spans.to_vec().into(),
                selections: Default::default(),
            },
            Projection::default(),
            options,
            system,
            None,
            ViewportOptions::default(),
        )
        .map(|_| ())
    }
    /// Build metadata for the document, but shape only the requested viewport.
    /// Without a viewport all blocks are measured for compatibility/intrinsics.
    /// Failure leaves the previously prepared layout usable.
    pub fn prepare_snapshot(
        &mut self,
        snapshot: ProjectionSnapshot,
        mut projection: Projection,
        options: LayoutOptions,
        system: Arc<TextSystem>,
        viewport: Option<Rect<f32>>,
        virtual_options: ViewportOptions,
    ) -> render::Result<f32> {
        anyhow::ensure!(
            options.font_size.is_finite()
                && options.font_size > 0.0
                && options.line_height.is_finite()
                && options.line_height > 0.0
                && options.width.is_none_or(|w| w.is_finite() && w >= 0.0),
            "invalid text layout dimensions"
        );
        anyhow::ensure!(
            virtual_options.overscan.is_finite()
                && virtual_options.overscan >= 0.0
                && virtual_options.max_cached_blocks > 0
                && virtual_options.estimated_advance.is_finite()
                && virtual_options.estimated_advance > 0.0,
            "invalid viewport options"
        );
        projection.validate(&snapshot.text)?;
        for s in snapshot.spans.iter() {
            anyhow::ensure!(
                snapshot.text.is_char_boundary(s.range.start)
                    && snapshot.text.is_char_boundary(s.range.end)
                    && s.range.start <= s.range.end,
                "invalid source styles"
            );
            s.style.validate()?;
        }
        let change = snapshot.change;
        let old = self.engine.get_mut().as_ref();
        let same_document = old.is_some_and(|old| {
            old.document_id == snapshot.document_id && snapshot.document_id != 0
        });
        let consecutive = old.filter(|_| same_document).and_then(|old| {
            change.as_ref().filter(|change| {
                change.before_revision == old.revision && change.revision == snapshot.revision
            })
        });
        let mapped_changes = consecutive.map(|change| &change.changes);
        // Keep the old source position on screen while previews change. The
        // incoming pixel offset still belongs to the previous height index.
        let anchor = viewport.and_then(|viewport| {
            old.filter(|old| {
                same_document && (old.revision == snapshot.revision || consecutive.is_some())
            })
            .map(|old| old.anchor_for(viewport).map(mapped_changes))
        });
        projection.match_widgets(
            old.map(|old| &old.projection),
            consecutive.map(|change| &change.changes),
            same_document
                && (old.is_some_and(|old| old.revision == snapshot.revision)
                    || consecutive.is_some()),
            same_document,
        )?;
        let descriptions = projection
            .widgets()
            .map(|(id, _, spec, _)| (id, spec.clone()))
            .collect();
        let present_views = projection.view_ids();
        let composition_cells = if snapshot.composing {
            old.filter(|old| old.document_id == snapshot.document_id)
                .map(|old| {
                    old.cache
                        .iter()
                        .filter_map(|(i, cached)| {
                            Some((
                                old.blocks[*i].view?,
                                cached
                                    .cells
                                    .iter()
                                    .map(|cell| crate::editing::TextCell {
                                        source: cell.source.clone(),
                                        bounds: cell.bounds,
                                    })
                                    .collect(),
                            ))
                        })
                        .collect()
                })
                .unwrap_or_default()
        } else {
            BTreeMap::new()
        };
        let mut next = Engine {
            composing: snapshot.composing,
            composition_cells,
            document_id: snapshot.document_id,
            revision: snapshot.revision,
            source: snapshot.text,
            styles: snapshot.spans,
            projection,
            descriptions,
            present_views,
            blocks: Vec::new(),
            heights: HeightIndex::default(),
            cache: BTreeMap::new(),
            options,
            system: system.clone(),
            font_revision: system.font_revision(),
            viewport,
            scroll_anchor: None,
            requested_position: None,
            virtual_options,
            views: self.views.clone(),
        };
        if let Some(old) = self.engine.get_mut() {
            if !next.composing
                && !old.composing
                && next.document_id != 0
                && old.document_id == next.document_id
                && old.projection == next.projection
                && (next.revision == old.revision
                    || change.as_ref().is_some_and(|c| {
                        c.before_revision == old.revision && c.changes.edits().is_empty()
                    }))
            {
                next.blocks = old.blocks.clone();
                for b in &mut next.blocks {
                    // A nonempty source line may also project to an empty row.
                    b.empty_height = Engine::empty_height(
                        &next.source,
                        &next.styles,
                        b.range.start,
                        b.style.line_height.unwrap_or(next.options.line_height),
                    );
                }
                next.reset_estimates();
            } else if let Some(change) = change.as_ref().filter(|c| {
                old.document_id == next.document_id
                    && next.document_id != 0
                    && c.before_revision == old.revision
                    && c.revision == next.revision
                    && !c.changes.edits().is_empty()
            }) {
                if !next.index_changed(old, &change.changes)? {
                    next.index_blocks()?;
                }
            } else {
                next.index_blocks()?;
            }
            next.reuse(old, mapped_changes)?;
        } else {
            next.index_blocks()?;
        }
        let (previous_descriptions, previous_mounted) = {
            let mut host = self.views.borrow_mut();
            let mounted = host.views.keys().copied().collect::<BTreeSet<_>>();
            (
                std::mem::replace(&mut host.descriptions, next.descriptions.clone()),
                mounted,
            )
        };
        let prepared = (|| {
            // Focused controls may be outside the viewport but must still receive
            // current props before the next keyboard/IME query.
            {
                let mut host = self.views.borrow_mut();
                if let Some(id) = host.focused.filter(|id| next.present_views.contains(id)) {
                    if host.factories.block_layout(id).is_none() {
                        host.get(id)?;
                    }
                }
            }
            // A focused source-backed decoration may be outside the viewport.
            // Refresh its generated inputs before clipboard or keyboard queries.
            let focused = self.views.borrow().focused;
            if let Some(id) = focused
                && self.views.borrow().factories.block_layout(id).is_some()
                && let Some(i) = next.blocks.iter().position(|b| b.view == Some(id))
            {
                next.ensure(i)?;
            }
            if let Some(viewport) = viewport {
                next.prepare_visible(viewport, anchor)
            } else {
                for i in 0..next.blocks.len() {
                    next.ensure(i)?;
                }
                Ok(0.0)
            }
        })();
        let y = match prepared {
            Ok(y) => y,
            Err(error) => {
                // A failed measurement must not publish new props to input or
                // painting through the previous, still usable layout.
                let mut host = self.views.borrow_mut();
                host.descriptions = previous_descriptions;
                host.retain_present(&previous_mounted);
                for id in previous_mounted {
                    host.get(id)?;
                }
                return Err(error);
            }
        };
        self.views.borrow_mut().retain_present(&next.present_views);
        *self.engine.get_mut() = Some(next);
        Ok(y)
    }
    pub fn set_viewport(&self, viewport: Rect<f32>) -> render::Result<f32> {
        let mut engine = self.engine.borrow_mut();
        let Some(e) = engine.as_mut() else {
            return Ok(viewport.origin.y);
        };
        e.prepare_visible(viewport, None)
    }
    /// Rebreak at a new width and return the corrected vertical scroll offset.
    /// Supply any pending manual scroll through `set_viewport` first, then apply
    /// the returned offset before supplying the next viewport.
    pub fn reflow(&mut self, width: Option<f32>) -> f32 {
        assert!(
            width.is_none_or(|w| w.is_finite() && w >= 0.0),
            "wrap_width must be finite and nonnegative"
        );
        let Some(old) = self.engine.get_mut() else {
            return 0.0;
        };
        if old.options.width == width {
            return old.viewport.map_or(0.0, |v| v.origin.y);
        }
        let anchor = old.viewport.map(|v| old.anchor_for(v));
        // Width changes clone only retained native paragraphs, then rebreak them.
        let mut next = old.clone();
        next.options.width = width;
        next.cache.clear();
        next.reset_estimates();
        for (i, cached) in &old.cache {
            let b = &next.blocks[*i];
            if b.view.is_some() || !cached.cells.is_empty() || !cached.objects.is_empty() {
                // Embedded content receives the new available width through
                // measure; rebreaking an old paragraph would freeze its boxes.
                continue;
            }
            let w = next.content_width(b);
            let mut flow = TextFlow::from_paragraph(
                cached.flow.paragraph().clone(),
                next.flow_options(b),
                system_clone(&next),
            );
            flow.reflow(next.options.width.map(|_| w));
            let mut c = Cached {
                window: None,
                cells: Vec::new(),
                decoration: None,
                rules: Vec::new(),
                projected: cached.projected.clone(),
                height: flow.size().height.max(if cached.projected.text.is_empty() {
                    b.empty_height
                } else {
                    0.0
                }),
                width: flow.size().width,
                objects: Vec::new(),
                flow,
            };
            c.update_objects(w, b.style.align.unwrap_or(TextAlign::Left));
            next.heights
                .set(*i, c.height + b.style.space_before + b.style.space_after);
            next.cache.insert(*i, Rc::new(c));
        }
        let y = if let Some(v) = next.viewport {
            next.prepare_visible(v, anchor)
                .expect("failed to reflow viewport")
        } else {
            for i in 0..next.blocks.len() {
                next.ensure(i).expect("failed to reflow text");
            }
            0.0
        };
        *old = next;
        y
    }
    pub fn next_cell(&self, position: usize, backwards: bool) -> Option<Selection> {
        let mut state = self.engine.borrow_mut();
        let e = state.as_mut()?;
        let i = e.block_at(position)?;
        e.ensure(i).ok()?;
        let cells = &e.cache[&i].cells;
        let at = cells
            .iter()
            .position(|c| c.source.start <= position && position <= c.source.end)?;
        let at = if backwards {
            at.checked_sub(1)?
        } else {
            at + 1
        };
        Some(Selection::caret(cells.get(at)?.source.start))
    }
    pub fn deletion_range(
        &self,
        selection: Selection,
        forward: bool,
        word: bool,
        width: f32,
        align: TextAlign,
        height: f32,
    ) -> Range<usize> {
        let range = if selection.is_caret() {
            let motion = match (forward, word) {
                (true, true) => Motion::WordRight,
                (true, false) => Motion::Right,
                (false, true) => Motion::WordLeft,
                (false, false) => Motion::Left,
            };
            let next = self.move_selection(selection, motion, true, width, align, height);
            next.text_range()
        } else {
            selection.text_range()
        };
        self.engine
            .borrow()
            .as_ref()
            .map_or(range.clone(), |e| e.projection.atomic_range(range))
    }
}
impl Cached {
    fn object_bounds(
        &self,
        width: f32,
        align: TextAlign,
    ) -> std::borrow::Cow<'_, [(ViewId, Range<usize>, Rect<f32>)]> {
        if align == TextAlign::Left || self.objects.is_empty() {
            return std::borrow::Cow::Borrowed(&self.objects);
        }
        let mut out = Vec::new();
        if self.cells.is_empty() && !self.projected.objects.is_empty() {
            for (id, _, r) in self.flow.paragraph().inline_boxes(width, align) {
                if let Some(o) = self.projected.objects.iter().find(|o| o.id.0 == id) {
                    out.push((
                        o.id,
                        o.source.clone(),
                        Rect::from_xywh(r.origin.x, r.origin.y, r.size.width, r.size.height),
                    ));
                }
            }
        } else if !self.cells.is_empty() {
            for cell in &self.cells {
                for (id, _, r) in cell
                    .flow
                    .paragraph()
                    .inline_boxes(cell.bounds.size.width, align)
                {
                    if let Some(o) = cell.projected.objects.iter().find(|o| o.id.0 == id) {
                        out.push((
                            o.id,
                            o.source.clone(),
                            Rect::from_xywh(
                                cell.bounds.origin.x + r.origin.x,
                                cell.bounds.origin.y + r.origin.y,
                                r.size.width,
                                r.size.height,
                            ),
                        ));
                    }
                }
            }
        } else {
            return std::borrow::Cow::Borrowed(&self.objects);
        }
        std::borrow::Cow::Owned(out)
    }

    fn update_objects(&mut self, width: f32, align: TextAlign) {
        self.objects = self
            .flow
            .paragraph()
            .inline_boxes(width, align)
            .into_iter()
            .filter_map(|(id, _, r)| {
                self.projected
                    .objects
                    .iter()
                    .find(|o| o.id.0 == id)
                    .map(|o| {
                        (
                            o.id,
                            o.source.clone(),
                            Rect::from_xywh(r.origin.x, r.origin.y, r.size.width, r.size.height),
                        )
                    })
            })
            .collect();
    }
}
fn system_clone(e: &Engine) -> Arc<TextSystem> {
    e.system.clone()
}
fn contains(r: Rect<f32>, p: Point<f32>) -> bool {
    p.x >= r.origin.x
        && p.x <= r.origin.x + r.size.width
        && p.y >= r.origin.y
        && p.y <= r.origin.y + r.size.height
}
fn paint_rect(p: &mut Painter<'_>, r: Rect<f32>, c: Hsla) {
    p.paint_quad(render::fill(
        render::Bounds::new(
            render::point(render::px(r.origin.x), render::px(r.origin.y)),
            render::size(render::px(r.size.width), render::px(r.size.height)),
        ),
        c,
    ));
}

fn distance(r: Rect<f32>, p: Point<f32>) -> f32 {
    let dx = (r.origin.x - p.x)
        .max(0.0)
        .max(p.x - r.origin.x - r.size.width);
    let dy = (r.origin.y - p.y)
        .max(0.0)
        .max(p.y - r.origin.y - r.size.height);
    dx * dx + dy * dy
}

mod cells;
mod engine;
mod input;
mod navigation;
mod paint;
mod reuse;
mod scroll;
use cells::CellMeasure;
use scroll::ScrollAnchor;
