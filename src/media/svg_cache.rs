//! Per-view SVG rendering with bounded work and latest-request publication.
use super::{PreparedSvg, Raster, SvgOptions};
use crate::{core::context::DrawContext, tasks::OwnedTaskScope};
use anyhow::Result;
use std::{
    cell::RefCell,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

/// Choose whether drawing must wait for an exact SVG raster.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SvgRenderPolicy {
    /// Resolve the current appearance and size synchronously on an atlas miss.
    #[default]
    Exact,
    /// Scale the last completed raster while preparing the latest request on a
    /// CPU worker. The first frame is empty until its raster is ready. Temporary
    /// scaling may change aspect ratio; the completed raster restores SVG rules.
    ScaleWhileRendering,
}

#[derive(Clone)]
pub(crate) enum SvgSource {
    Inline(Arc<str>, SvgOptions),
    Image(Arc<PreparedSvg>),
}
impl SvgSource {
    fn same(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Inline(a, ao), Self::Inline(b, bo)) => {
                (Arc::ptr_eq(a, b) || a == b)
                    && ao.limits == bo.limits
                    && Arc::ptr_eq(&ao.fontdb, &bo.fontdb)
            }
            (Self::Image(a), Self::Image(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }
    fn render(&self, width: u32, height: u32) -> Result<RenderedSvg> {
        let parsed = match self {
            Self::Inline(source, options) => PreparedSvg::parse(source, options)?,
            Self::Image(parsed) => parsed.clone(),
        };
        let raster = parsed.raster(width, height)?;
        let pixels = match self {
            Self::Inline(..) => parsed.pixels(&raster)?,
            Self::Image(..) => parsed.image_pixels(&raster)?,
        };
        Ok(RenderedSvg {
            raster,
            pixels,
            _parsed: parsed,
        })
    }
}

pub(crate) struct RenderedSvg {
    pub raster: Arc<Raster>,
    // Keep the parsed tree available to its weak cache for DPI-only refreshes.
    _parsed: Arc<PreparedSvg>,
    // Keep only the displayed buffer. Atlas recovery must never run CPU SVG
    // rendering on the draw thread in background mode.
    pub pixels: Vec<u8>,
}
#[derive(Clone)]
struct Request {
    source: SvgSource,
    size: (u32, u32),
    generation: u64,
}
#[derive(Default)]
struct State {
    request: Option<Request>,
    generation: Arc<AtomicU64>,
    running: bool,
    ready: Option<Rc<RenderedSvg>>,
    error: Option<String>,
}
#[derive(Default)]
pub(crate) struct SvgRenderCache {
    state: Rc<RefCell<State>>,
    scope: Option<OwnedTaskScope>,
}
impl Drop for SvgRenderCache {
    fn drop(&mut self) {
        // Queued compute closures check this before parsing or allocating pixels.
        self.state
            .borrow()
            .generation
            .fetch_add(1, Ordering::Relaxed);
    }
}
impl SvgRenderCache {
    pub fn request(
        &mut self,
        source: SvgSource,
        size: (u32, u32),
        ctx: &DrawContext<'_>,
    ) -> Result<Option<Rc<RenderedSvg>>> {
        let Some((id, tree)) = ctx.element else {
            anyhow::bail!("background SVG rendering requires a mounted widget context");
        };
        match &source {
            SvgSource::Inline(_, options) => options.limits.check_size(size.0, size.1)?,
            SvgSource::Image(parsed) => parsed.check_size(size.0, size.1)?,
        }
        let mut state = self.state.borrow_mut();
        if state
            .request
            .as_ref()
            .is_none_or(|r| r.size != size || !r.source.same(&source))
        {
            let generation = state.generation.fetch_add(1, Ordering::Relaxed) + 1;
            state.request = Some(Request {
                source,
                size,
                generation,
            });
            state.error = None;
            if !state.running {
                let scope = self
                    .scope
                    .get_or_insert_with(|| tree.task_runtime().scope());
                let weak = Rc::downgrade(&self.state);
                let invalidate = tree.invalidator(id);
                state.running = true;
                let task = scope.spawn(async move {
                    loop {
                        let Some(shared) = weak.upgrade() else { return };
                        let (request, generation) = {
                            let state = shared.borrow();
                            (state.request.clone().unwrap(), state.generation.clone())
                        };
                        drop(shared);
                        let version = request.generation;
                        let result = crate::tasks::workers::compute(move || {
                            if generation.load(Ordering::Relaxed) != version {
                                return Ok(None);
                            }
                            request
                                .source
                                .render(request.size.0, request.size.1)
                                .map(Some)
                        })
                        .await;
                        let Some(shared) = weak.upgrade() else { return };
                        let mut state = shared.borrow_mut();
                        if state.generation.load(Ordering::Relaxed) != version {
                            // Only one compute call per view can be in flight.
                            // Intermediate sizes are replaced, never queued.
                            continue;
                        }
                        match result {
                            Ok(Ok(Some(rendered))) => state.ready = Some(Rc::new(rendered)),
                            Ok(Ok(None)) => {}
                            Ok(Err(error)) => state.error = Some(error.to_string()),
                            Err(error) => state.error = Some(error.to_string()),
                        }
                        state.running = false;
                        drop(state);
                        invalidate.repaint();
                        return;
                    }
                });
                // Admission rejection completes immediately and never polls the
                // future, so record it here instead of leaving a pending view.
                if task.is_finished() {
                    use futures_util::FutureExt;
                    if let Some(Err(error)) = task.now_or_never() {
                        state.running = false;
                        state.error = Some(error.to_string());
                    }
                }
            }
        }
        if let Some(error) = &state.error {
            anyhow::bail!("background SVG rendering failed: {error}");
        }
        Ok(state.ready.clone())
    }
}
