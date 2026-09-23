//! Retained editor geometry, clipping, scrolling, and paint-only updates.
use super::*;

impl TextEdit {
    pub(super) fn prepare(
        &self,
        state: &EditorState,
        system: Arc<TextSystem>,
        bounds: Rect<f32>,
        style: &TextStyle,
    ) -> render::Result<()> {
        let options = LayoutOptions {
            font: style.font.clone(),
            font_size: style.font_size,
            line_height: style.line_height.resolve(style.font_size),
            width: (self.multiline && style.wrap).then_some(bounds.size.width.max(0.0)),
        };
        let key = (
            state.revision(),
            state.highlight_revision(),
            state.projection_revision(),
            (state.composition().is_some()
                || state.projection_revision() != 0
                || !self.extensions.is_empty())
            .then(|| state.generation()),
        );
        let mut view = self.view.borrow_mut();
        // Selection-only changes can reuse geometry when every extension
        // confirms that its revealed syntax is unchanged. Composition always
        // uses the normal path, including its source-coordinate mapping.
        if state.composition().is_none()
            && view.pointer_projection.is_none()
            && view.source.is_some_and(|old| {
                old.0 == key.0 && old.1 == key.1 && old.2 == key.2 && old.3 != key.3
            })
            && self
                .extension_host
                .borrow_mut()
                .selection_unchanged(&state.snapshot())
        {
            view.source = Some(key);
        }
        let changed = view.source != Some(key)
            || view.options.as_ref() != Some(&options)
            || view.font_revision != system.font_revision()
            || view
                .system
                .as_ref()
                .is_none_or(|s| !Arc::ptr_eq(s, &system));
        if changed {
            // Reflow changes geometry, not the user's navigation intent. Keep
            // manual scrolling across resize/typography changes; editor state
            // changes and explicit navigation below still reveal the caret.
            if view.font_revision != system.font_revision() {
                view.placeholder_key = None;
            }
            let width_only = view.source == Some(key)
                && view.font_revision == system.font_revision()
                && view
                    .system
                    .as_ref()
                    .is_some_and(|old| Arc::ptr_eq(old, &system))
                && view.options.as_ref().is_some_and(|old| {
                    old.font == options.font
                        && old.font_size == options.font_size
                        && old.line_height == options.line_height
                });
            let viewport = Rect::from_xywh(
                view.scroll.x,
                view.scroll.y,
                bounds.size.width,
                bounds.size.height,
            );
            if width_only {
                // A wheel/scrollbar update may be newer than the last measured
                // frame. Anchor reflow to that requested viewport, not stale Y.
                view.scroll.y = view.text.set_viewport(viewport)?;
                view.scroll.y = view.text.reflow(options.width);
            } else {
                let mut projection = if let Some(frozen) = view
                    .pointer_projection
                    .as_ref()
                    .filter(|p| p.0 == state.revision() && state.composition().is_none())
                {
                    frozen.1.clone()
                } else {
                    let committed = state.snapshot();
                    let projection = self.extension_host.borrow_mut().project(
                        &committed,
                        state.projection(),
                        state.last_change(),
                    )?;
                    projection.active(
                        state.selections(),
                        state.composition().map(|c| c.range.clone()),
                    )
                };
                if let Some(c) = state.composition() {
                    projection.compose(c.range.clone(), c.text.len());
                }
                let mut snapshot = state.display_snapshot();
                snapshot.spans =
                    crate::editing::highlights::overlay(&snapshot.spans, &projection.styles).into();
                view.text
                    .configure_views(self.views.clone(), self.invalidator.clone());
                view.scroll.y = view.text.prepare_snapshot(
                    snapshot,
                    projection,
                    options.clone(),
                    system.clone(),
                    Some(viewport),
                    self.viewport_options,
                )?;
            }
            view.options = Some(options);
            view.source = Some(key);
            view.font_revision = system.font_revision();
            view.system = Some(system);
        }
        // Scroll-only updates prepare newly visible blocks even with unchanged text.
        let viewport = Rect::from_xywh(
            view.scroll.x,
            view.scroll.y,
            bounds.size.width,
            bounds.size.height,
        );
        view.scroll.y = view.text.set_viewport(viewport)?;
        let generation = state.generation();
        if view.reveal_generation != Some(generation) {
            // Candidate length changes may wrap inside a stable cell. Keep the
            // page and table anchored at the composition start; the platform
            // still receives the actual candidate caret from bounds_for_range.
            let selection = state
                .composition()
                .filter(|c| view.text.is_source_backed(c.range.start))
                .map_or_else(
                    || display_selection(state),
                    |c| crate::editing::Selection::caret(c.range.start),
                );
            let width = bounds.size.width.max(view.text.size().width);
            view.text.reveal_source_caret(
                selection.head,
                selection.affinity,
                width,
                style.align.resolve(style.direction),
            );
            if let Some(caret) = view.text.caret(
                selection.head,
                selection.affinity,
                width,
                style.align.resolve(style.direction),
            ) {
                reveal_axis(
                    &mut view.scroll.x,
                    caret.origin.x,
                    // Reveal the insertion position. Caret ink at the viewport
                    // edge is painted inward and does not enlarge the document.
                    0.0,
                    bounds.size.width,
                );
                reveal_axis(
                    &mut view.scroll.y,
                    caret.origin.y,
                    caret.size.height,
                    bounds.size.height,
                );
            }
            view.reveal_generation = Some(generation);
        }
        clamp_scroll(&mut view, bounds);
        Ok(())
    }
    pub(super) fn paint_control(
        &self,
        painter: &mut Painter<'_>,
        cx: DrawContext,
    ) -> render::Result<()> {
        let system = painter.text_system().clone();
        self.editor().with(|state| -> render::Result<()> {
            self.prepare(state, system.clone(), cx.content_bounds, &self.style)?;
            let clip = native_rect(cx.content_bounds);
            painter.with_clip(clip, |painter| -> render::Result<()> {
                let placeholder = state.document().is_empty()
                    && state.composition().is_none()
                    && !self.placeholder.is_empty();
                let mut view = self.view.borrow_mut();
                if placeholder {
                    let style = &self.placeholder_style;
                    let options = LayoutOptions {
                        font: style.font.clone(),
                        font_size: style.font_size,
                        line_height: style.line_height.resolve(style.font_size),
                        width: (self.multiline && style.wrap)
                            .then_some(cx.content_bounds.size.width),
                    };
                    let key = (self.placeholder.clone(), options.clone());
                    if view.placeholder_key.as_ref() != Some(&key) {
                        view.placeholder
                            .prepare(&self.placeholder, options, system)?;
                        view.placeholder_key = Some(key);
                    }
                    view.placeholder.paint(
                        painter,
                        cx.content_bounds.origin,
                        cx.content_bounds,
                        cx.content_bounds.size.width,
                        cx.text_align,
                        style.color.resolve(cx.color).into(),
                        &[],
                        self.selection_colors.color.into(),
                        self.selection_colors.background.into(),
                    )?;
                } else {
                    let ranges: Vec<_> = if state.composition().is_some() {
                        let range = display_selection(state).text_range();
                        if range.is_empty() {
                            Vec::new()
                        } else {
                            vec![range]
                        }
                    } else {
                        state
                            .selections()
                            .iter()
                            .filter(|s| !s.is_caret())
                            .map(|s| s.text_range())
                            .collect()
                    };
                    let origin = Point::new(
                        cx.content_bounds.origin.x - view.scroll.x,
                        cx.content_bounds.origin.y - view.scroll.y,
                    );
                    let width = cx.content_bounds.size.width.max(view.text.size().width);
                    view.text.paint(
                        painter,
                        origin,
                        cx.content_bounds,
                        width,
                        cx.text_align,
                        cx.color.into(),
                        &ranges,
                        self.selection_colors.color.into(),
                        self.selection_colors.background.into(),
                    )?;
                    if let Some(compose) = state.composition() {
                        let range = compose.range.start..compose.range.start + compose.text.len();
                        for rect in view.text.selection_rectangles(range, width, cx.text_align) {
                            paint_rect(
                                painter,
                                Rect::from_xywh(
                                    origin.x + rect.origin.x,
                                    origin.y + rect.origin.y + rect.size.height - 1.0,
                                    rect.size.width,
                                    1.0,
                                ),
                                cx.color.into(),
                            );
                        }
                    }
                }
                if self.active
                    && !self.disabled
                    && !state.selections().is_structured()
                    && view.text.focused_view().is_none()
                {
                    let composed_selection = state.composition().map(|_| display_selection(state));
                    let selections = composed_selection.as_ref().into_iter().chain(
                        state
                            .selections()
                            .iter()
                            .filter(|_| composed_selection.is_none()),
                    );
                    if !state.composition().is_some_and(|c| c.cursor.is_none()) {
                        for selection in selections.filter(|s| s.is_caret()) {
                            let width = cx.content_bounds.size.width.max(view.text.size().width);
                            if let Some(caret) = view.text.caret(
                                selection.head,
                                selection.affinity,
                                width,
                                cx.text_align,
                            ) {
                                let bounds = viewport_caret(caret, cx.content_bounds, view.scroll);
                                painter.paint_caret(
                                    render::fill(
                                        native_rect(bounds),
                                        render::Hsla::from(
                                            self.style.caret_color.resolve(cx.color),
                                        ),
                                    ),
                                    self.caret_visible || !self.style.caret_animation,
                                );
                            }
                        }
                    }
                }
                Ok(())
            })
        })
    }
}

pub(super) fn display_selection(state: &EditorState) -> Selection {
    if let Some(c) = state.composition() {
        let (a, b) = c.cursor.unwrap_or((c.text.len(), c.text.len()));
        Selection::range(c.range.start + a, c.range.start + b)
    } else {
        state.selections().primary()
    }
}
pub(super) fn clamp_scroll(view: &mut ViewLayout, bounds: Rect<f32>) {
    view.scroll.x = view
        .scroll
        .x
        .clamp(0.0, (view.text.size().width - bounds.size.width).max(0.0));
    view.scroll.y = view
        .scroll
        .y
        .clamp(0.0, (view.text.size().height - bounds.size.height).max(0.0));
}
/// Scroll a drag only when its pointer leaves the viewport. Selecting a range
/// inside it must not pull an offscreen selection endpoint into view.
pub(super) fn reveal_drag_point(view: &mut ViewLayout, point: Point<f32>, bounds: Rect<f32>) {
    reveal_axis(&mut view.scroll.x, point.x, 0.0, bounds.size.width);
    reveal_axis(&mut view.scroll.y, point.y, 0.0, bounds.size.height);
    clamp_scroll(view, bounds);
}

/// Convert a logical insertion position to the visible caret used by painting,
/// IME and completion anchors. Only visible positions can move inward: offscreen
/// carets stay clipped instead of appearing spuriously at the viewport edge.
pub(super) fn viewport_caret(
    mut caret: Rect<f32>,
    bounds: Rect<f32>,
    scroll: Point<f32>,
) -> Rect<f32> {
    caret.origin.x += bounds.origin.x - scroll.x;
    caret.origin.y += bounds.origin.y - scroll.y;
    let width = bounds.size.width.max(0.0);
    let right = bounds.origin.x + width;
    if caret.origin.x >= bounds.origin.x && caret.origin.x <= right {
        caret.size.width = caret.size.width.min(width);
        caret.origin.x = caret.origin.x.min(right - caret.size.width);
    }
    caret
}

fn reveal_axis(scroll: &mut f32, start: f32, size: f32, viewport: f32) {
    if start < *scroll {
        *scroll = start;
    } else if start + size > *scroll + viewport {
        *scroll = start + size - viewport;
    }
}
fn native_rect(bounds: Rect<f32>) -> render::Bounds<render::Pixels> {
    render::Bounds::new(
        render::point(render::px(bounds.origin.x), render::px(bounds.origin.y)),
        render::size(
            render::px(bounds.size.width),
            render::px(bounds.size.height),
        ),
    )
}
fn paint_rect(painter: &mut Painter<'_>, bounds: Rect<f32>, color: render::Hsla) {
    painter.paint_quad(render::fill(native_rect(bounds), color));
}

// Range queries do not flatten the entire document on every IME cursor update.
pub(super) fn display_boundary(state: &EditorState, index: usize) -> bool {
    let Some(c) = state.composition() else {
        return state.document().is_char_boundary(index);
    };
    if index < c.range.start {
        return state.document().is_char_boundary(index);
    }
    if index <= c.range.start + c.text.len() {
        return c.text.is_char_boundary(index - c.range.start);
    }
    index
        .checked_sub(c.range.start + c.text.len())
        .and_then(|i| c.range.end.checked_add(i))
        .is_some_and(|i| state.document().is_char_boundary(i))
}
pub(super) fn display_slice(state: &EditorState, range: Range<usize>) -> Option<String> {
    if range.start > range.end
        || !display_boundary(state, range.start)
        || !display_boundary(state, range.end)
    {
        return None;
    }
    let Some(c) = state.composition() else {
        return state.document().read(range).map(|s| s.into_owned());
    };
    let mut out = String::with_capacity(range.len());
    let start = range.start;
    let end = range.end;
    if start < c.range.start {
        out.push_str(&state.document().read(start..end.min(c.range.start))?);
    }
    let a = start.max(c.range.start);
    let b = end.min(c.range.start + c.text.len());
    if a < b {
        out.push_str(&c.text[a - c.range.start..b - c.range.start]);
    }
    let a = start.max(c.range.start + c.text.len());
    if a < end {
        out.push_str(
            &state
                .document()
                .read(a - c.text.len() + c.range.len()..end - c.text.len() + c.range.len())?,
        );
    }
    Some(out)
}
