//! Post-layout coordinates. Ordinary nodes keep a single null pointer; only
//! transformed subtrees retain matrices. Scroll updates reuse Taffy/text caches.
use super::{
    geometry::{Point, Rect},
    stacking::Phase,
    widget::WidgetId,
    widget_tree::WidgetTree,
};
use crate::render::{Affine, ClipChain, PaintSpace, SpatialClip};
use std::sync::Arc;
#[derive(Debug)]
pub(crate) struct VisualGeometry {
    pub matrix: Affine,
    pub inverse: Option<Affine>,
    pub box_space: Arc<PaintSpace>,
    pub content_space: Arc<PaintSpace>,
    descendants: Option<Arc<VisualGeometry>>,
}
impl WidgetTree {
    pub(crate) fn update_visual_geometry(&mut self, id: WidgetId) {
        let inherited = self.nodes[id]
            .layout_parent
            .and_then(|p| self.nodes[p].visual.as_ref())
            .map(|v| v.matrix);
        let n = &self.nodes[id];
        if inherited.is_none() && n.computed.transform.is_none() {
            self.nodes[id].visual = None;
            return;
        }
        if n.computed.transform.is_none()
            && n.computed.scroll.overflow.x == crate::Overflow::Visible
            && n.computed.scroll.overflow.y == crate::Overflow::Visible
        {
            let parent = self.nodes[id]
                .layout_parent
                .and_then(|p| self.nodes[p].visual.clone())
                .unwrap();
            self.nodes[id].visual = Some(parent.descendants.clone().unwrap_or(parent));
            return;
        }
        let b = n.global_bounds;
        let o = &n.computed.transform_origin;
        let x = b.origin.x + o.x.resolve(b.size.width);
        let y = b.origin.y + o.y.resolve(b.size.height);
        let matrix = if n.computed.transform.is_none() {
            inherited.unwrap_or_default()
        } else {
            inherited
                .unwrap_or_default()
                .compose(Affine::translate(x, y))
                .compose(n.computed.transform.matrix(b.size.width, b.size.height))
                .compose(Affine::translate(-x, -y))
        };
        let inverse = matrix.inverse();
        let parent = self.nodes[id].layout_parent;
        let clips = parent
            .and_then(|p| self.nodes[p].visual.as_ref())
            .and_then(|v| v.content_space.clips.clone())
            .or_else(|| {
                if parent.is_some_and(|p| self.nodes[p].visual.is_none()) {
                    self.ancestor_clips(parent)
                } else {
                    None
                }
            });
        let same_chain = |a: &Option<Arc<ClipChain>>, b: &Option<Arc<ClipChain>>| match (a, b) {
            (None, None) => true,
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            _ => false,
        };
        let previous = self.nodes[id].visual.as_ref();
        let box_space = if let Some(previous) =
            previous.filter(|v| v.matrix == matrix && same_chain(&v.box_space.clips, &clips))
        {
            previous.box_space.clone()
        } else {
            Arc::new(PaintSpace::new(matrix, clips.clone()))
        };
        let content_space = if let Some(clip) = self.own_clip(id, inverse.unwrap_or_default()) {
            if let Some(previous) = previous.filter(|v| {
                v.matrix == matrix
                    && v.content_space
                        .clips
                        .as_ref()
                        .is_some_and(|c| c.clip == clip && same_chain(&c.parent, &clips))
            }) {
                previous.content_space.clone()
            } else {
                Arc::new(PaintSpace::new(
                    matrix,
                    Some(Arc::new(ClipChain {
                        clip,
                        parent: clips,
                    })),
                ))
            }
        } else {
            box_space.clone()
        };
        let descendants = if Arc::ptr_eq(&box_space, &content_space) {
            None
        } else {
            previous
                .and_then(|v| v.descendants.as_ref())
                .filter(|v| Arc::ptr_eq(&v.content_space, &content_space))
                .cloned()
                .or_else(|| {
                    Some(Arc::new(VisualGeometry {
                        matrix,
                        inverse,
                        box_space: content_space.clone(),
                        content_space: content_space.clone(),
                        descendants: None,
                    }))
                })
        };
        let next = VisualGeometry {
            matrix,
            inverse,
            box_space,
            content_space,
            descendants,
        };
        if self.nodes[id].visual.as_ref().is_some_and(|v| {
            Arc::ptr_eq(&v.box_space, &next.box_space)
                && Arc::ptr_eq(&v.content_space, &next.content_space)
        }) {
            return;
        }
        self.nodes[id].visual = Some(Arc::new(next));
    }
    pub(crate) fn refresh_visual_geometry(&mut self) {
        let origin = self.viewport.origin;
        if let Some(root) = self.root() {
            self.calc_layout_positions(root, origin);
        }
        for index in 0..self.viewport.children.len() {
            self.calc_layout_positions(self.viewport.children[index], origin);
        }
        self.paint_clip_dirty.set(true);
    }
    fn own_clip(&self, id: WidgetId, inverse: Affine) -> Option<SpatialClip> {
        let n = &self.nodes[id];
        let o = n.computed.scroll.overflow;
        let axes = [
            o.x != crate::Overflow::Visible,
            o.y != crate::Overflow::Visible,
        ];
        if !axes[0] && !axes[1] {
            return None;
        }
        let b = self.scrollport(id);
        Some(SpatialClip {
            inverse,
            bounds: [b.origin.x, b.origin.y, b.size.width, b.size.height],
            axes,
        })
    }
    fn ancestor_clips(&self, id: Option<WidgetId>) -> Option<Arc<ClipChain>> {
        let id = id?;
        let n = &self.nodes[id];
        let parent = self.ancestor_clips(n.layout_parent);
        match self.own_clip(id, Affine::IDENTITY) {
            Some(clip) => Some(Arc::new(ClipChain { clip, parent })),
            None => parent,
        }
    }
    pub(crate) fn entry_space(&self, id: WidgetId, phase: Phase) -> Option<Arc<PaintSpace>> {
        if phase == Phase::Backdrop {
            let b = self.nodes[id].backdrop.as_ref()?;
            if b.style.transform.is_none() {
                return None;
            }
            let x = b.bounds.origin.x + b.style.transform_origin.x.resolve(b.bounds.size.width);
            let y = b.bounds.origin.y + b.style.transform_origin.y.resolve(b.bounds.size.height);
            let matrix = Affine::translate(x, y)
                .compose(
                    b.style
                        .transform
                        .matrix(b.bounds.size.width, b.bounds.size.height),
                )
                .compose(Affine::translate(-x, -y));
            return Some(Arc::new(PaintSpace::new(matrix, None)));
        }
        let v = self.nodes[id].visual.as_ref()?;
        Some(if phase == Phase::Content {
            v.content_space.clone()
        } else {
            v.box_space.clone()
        })
    }
    pub(crate) fn layout_to_window(&self, id: WidgetId, p: Point<f32>) -> Point<f32> {
        let [x, y] = self.nodes[id]
            .visual
            .as_ref()
            .map_or([p.x, p.y], |v| v.matrix.map([p.x, p.y]));
        Point::new(x, y)
    }
    pub(crate) fn layout_rect_to_window(&self, id: WidgetId, rect: Rect<f32>) -> Rect<f32> {
        let Some(v) = &self.nodes[id].visual else {
            return rect;
        };
        let b = v
            .matrix
            .map_bounds(super::paint::render_bounds(rect).scale(1.));
        Rect::from_xywh(b.origin.x.0, b.origin.y.0, b.size.width.0, b.size.height.0)
    }
    /// Convert a window point into pre-transform document coordinates. Use this
    /// for custom text/editing adapters whose bounds come from `content_bounds`.
    pub fn window_to_layout(&self, id: WidgetId, point: Point<f32>) -> Option<Point<f32>> {
        let n = self.nodes.get(id)?;
        let Some(v) = &n.visual else {
            return Some(point);
        };
        let [x, y] = v.inverse?.map([point.x, point.y]);
        Some(Point::new(x, y))
    }
    /// Local pointer coordinates include the inverse of every ancestor transform.
    pub fn window_to_local(&self, id: WidgetId, point: Point<f32>) -> Option<Point<f32>> {
        let p = self.window_to_layout(id, point)?;
        let b = self.nodes[id].global_bounds;
        Some(Point::new(p.x - b.origin.x, p.y - b.origin.y))
    }
    /// Axis-aligned visual bounds, analogous to getBoundingClientRect(). Layout
    /// APIs retain the normal-flow dimensions even after scaling or rotation.
    pub fn visual_bounds(&self, id: WidgetId) -> Rect<f32> {
        let n = &self.nodes[id];
        let Some(v) = &n.visual else {
            return n.global_bounds;
        };
        let b = v
            .matrix
            .map_bounds(super::paint::render_bounds(n.global_bounds).scale(1.));
        Rect::from_xywh(b.origin.x.0, b.origin.y.0, b.size.width.0, b.size.height.0)
    }
}
