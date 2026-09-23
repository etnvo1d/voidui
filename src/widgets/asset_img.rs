//! Demand-loaded images reserve intrinsic geometry before worker decoding starts.
use super::img::Img;
use crate::{
    core::{
        context::{DrawContext, LayoutContext},
        layout::{LayoutInput, LayoutOutput},
        widget::{Widget, WidgetBuilder, WidgetUpdate},
    },
    media::ImageAsset,
    render::{Painter, Result},
    tasks::OwnedTaskScope,
};
use std::{cell::RefCell, rc::Rc, sync::Arc};
#[derive(Default)]
struct State {
    requested: Option<(u32, u32)>,
    ready_size: Option<(u32, u32)>,
    ready: Option<Arc<crate::media::Image>>,
    widget: Option<Img>,
    running: bool,
    failed: bool,
}
pub struct AssetImg {
    asset: ImageAsset,
    state: Rc<RefCell<State>>,
    scope: RefCell<Option<OwnedTaskScope>>,
}
pub fn asset_img(asset: ImageAsset) -> WidgetBuilder<AssetImg> {
    WidgetBuilder::from_widget(AssetImg {
        asset,
        state: Default::default(),
        scope: Default::default(),
    })
}
impl Widget for AssetImg {
    fn tag_name(&self) -> &'static str {
        "img"
    }
    fn reconcile(&mut self, next: &dyn Widget) -> WidgetUpdate {
        let Some(next) = (next as &dyn std::any::Any).downcast_ref::<Self>() else {
            return WidgetUpdate::Replace;
        };
        if self.asset == next.asset {
            return WidgetUpdate::Unchanged;
        }
        self.asset = next.asset.clone();
        self.state = Default::default();
        self.scope = Default::default();
        WidgetUpdate::Changed
    }
    fn layout(&mut self, input: LayoutInput, cx: LayoutContext<'_, '_>) -> LayoutOutput {
        crate::media::view::replaced_layout(cx.layout_style(), input, self.asset.intrinsic_size())
    }
    fn draw(&self, painter: &mut Painter<'_>, cx: DrawContext) -> Result<()> {
        let bounds = crate::media::view::bounds_from_rect(cx.content_bounds);
        if !crate::media::view::visible(painter, bounds) || cx.content_bounds.is_empty() {
            return Ok(());
        }
        let media = cx.media_style();
        let [_, _, width, height] = crate::style::media::object_rect(
            [cx.content_bounds.size.width, cx.content_bounds.size.height],
            self.asset.intrinsic_size(),
            media.object_fit(),
            media.object_position(),
        );
        // Cover can paint a larger image behind the clip. Decode for that image
        // rectangle, not just the clipped box, to preserve the displayed detail.
        let size = crate::media::view::physical_size(width, height, painter.scale_factor())?;
        let mut state = self.state.borrow_mut();
        if state.requested != Some(size) {
            state.requested = Some(size);
            state.failed = false;
        }
        if !state.running && !state.failed && state.ready_size != Some(size) {
            if let Some((id, tree)) = cx.element {
                let scope = self
                    .scope
                    .borrow_mut()
                    .get_or_insert_with(|| tree.task_runtime().scope())
                    .handle();
                let weak = Rc::downgrade(&self.state);
                let asset = self.asset.clone();
                let invalidate = tree.invalidator(id);
                state.running = true;
                let task = scope.spawn(async move {
                    let loaded =
                        crate::tasks::workers::compute(move || asset.load(size.0, size.1)).await;
                    if let Some(state) = weak.upgrade() {
                        let mut state = state.borrow_mut();
                        state.running = false;
                        match loaded {
                            Ok(Ok(image)) => {
                                state.ready_size = Some(size);
                                state.widget = Some(
                                    super::img::img((*image).clone())
                                        .render_policy(
                                            crate::media::SvgRenderPolicy::ScaleWhileRendering,
                                        )
                                        .widget,
                                );
                                state.ready = Some(image);
                            }
                            _ => state.failed = state.requested == Some(size),
                        }
                        drop(state);
                        invalidate.repaint();
                    }
                });
                if task.is_finished() {
                    use futures_util::FutureExt;
                    if let Some(Err(_)) = task.now_or_never() {
                        state.running = false;
                        state.failed = true;
                    }
                }
            }
        }
        if let Some(widget) = &state.widget {
            widget.draw(painter, cx)?;
        }
        Ok(())
    }
}
