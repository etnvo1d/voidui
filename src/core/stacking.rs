//! CSS stacking contexts compile into a retained paint list. Painting and hit
//! testing consume the same list, so visual ordering cannot diverge from input.
use super::layout::{Display, Layout};
use super::{
    geometry::{Point, Rect},
    top_layer::HitTarget,
    widget::WidgetId,
    widget_tree::WidgetTree,
};
use crate::style::{
    computed::ComputedStyle,
    layer::{Isolation, PointerEvents, Position, Visibility, ZIndex},
};

pub(crate) struct Backdrop {
    pub specified: crate::style::style::Style,
    pub style: ComputedStyle,
    pub layout: Layout,
    pub bounds: Rect<f32>,
    pub cache: super::paint::PaintCache,
}
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Clip {
    pub x: Option<(f32, f32)>,
    pub y: Option<(f32, f32)>,
}
impl Clip {
    fn intersect(self, other: Self) -> Self {
        let axis = |a: Option<(f32, f32)>, b: Option<(f32, f32)>| match (a, b) {
            (Some(a), Some(b)) => Some((a.0.max(b.0), a.1.min(b.1))),
            (a, b) => a.or(b),
        };
        Self {
            x: axis(self.x, other.x),
            y: axis(self.y, other.y),
        }
    }
    pub fn contains(self, p: Point<f32>) -> bool {
        self.x.is_none_or(|(a, b)| p.x >= a && p.x < b)
            && self.y.is_none_or(|(a, b)| p.y >= a && p.y < b)
    }
    pub fn bounds(
        self,
        mut bounds: voidui_gpui_wgpu::Bounds<voidui_gpui_wgpu::Pixels>,
    ) -> voidui_gpui_wgpu::Bounds<voidui_gpui_wgpu::Pixels> {
        if let Some((a, b)) = self.x {
            bounds.origin.x = voidui_gpui_wgpu::px(a);
            bounds.size.width = voidui_gpui_wgpu::px((b - a).max(0.0));
        }
        if let Some((a, b)) = self.y {
            bounds.origin.y = voidui_gpui_wgpu::px(a);
            bounds.size.height = voidui_gpui_wgpu::px((b - a).max(0.0));
        }
        bounds
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Phase {
    Box,
    Content,
    Backdrop,
    Scrollbar,
}
#[derive(Debug, Clone)]
pub(crate) struct PaintEntry {
    pub space: Option<std::sync::Arc<crate::render::PaintSpace>>,
    pub id: WidgetId,
    pub phase: Phase,
    pub clip: Clip,
}
#[derive(Default)]
pub(crate) struct PaintOrder {
    pub entries: Vec<PaintEntry>,
    pub rebuilds: u64,
    pub revision: u64,
}
// `current` is the local paint-as-if group; `real` is the actual stacking
// context. Positioned descendants escape an auto group but never a real one:
//
// real context
//   +-- relative / z:auto   (ordinary descendants stay together)
//   |     +-- z:10 --------> collected by the real context
//   +-- relative / z:0     (descendants stay inside this atomic context)
#[derive(Default)]
struct Group {
    root: Option<WidgetId>,
    negative: Vec<(i32, usize, usize)>,
    normal: Vec<Item>,
    content: Vec<WidgetId>,
    zero: Vec<(i32, usize, usize)>,
    positive: Vec<(i32, usize, usize)>,
}
enum Item {
    Box(WidgetId),
    Group(usize),
}

impl PaintOrder {
    fn rebuild(&mut self, tree: &WidgetTree) {
        self.entries.clear();
        self.rebuilds += 1;
        let Some(root) = tree.root() else {
            return;
        };
        self.append_context(tree, root);
        for entry in &tree.top_layers {
            if !tree.is_rendered(entry.id) {
                continue;
            }
            self.entries.push(PaintEntry {
                id: entry.id,
                phase: Phase::Backdrop,
                space: tree.entry_space(entry.id, Phase::Backdrop),
                clip: Clip::default(),
            });
            self.append_context(tree, entry.id);
        }
    }
    fn append_context(&mut self, tree: &WidgetTree, root: WidgetId) {
        if !tree.is_rendered(root) {
            return;
        }
        let mut groups = vec![Group {
            root: Some(root),
            content: if tree.nodes[root]
                .widget
                .as_ref()
                .is_some_and(|w| w.paints_content())
            {
                vec![root]
            } else {
                Vec::new()
            },
            ..Default::default()
        }];
        fn visit(
            tree: &WidgetTree,
            id: WidgetId,
            current: usize,
            real: usize,
            groups: &mut Vec<Group>,
        ) {
            let node = &tree.nodes[id];
            if node.computed.layout.display == Display::None || tree.is_top_layer(id) {
                return;
            }
            let style = node.computed.layer;
            let positioned = style.position != Position::Static;
            let flex_item = node.parent.is_some_and(|p| {
                matches!(
                    tree.nodes[p].computed.layout.display,
                    Display::Flex | Display::Grid
                )
            }) && !matches!(style.position, Position::Absolute | Position::Fixed);
            let z = if positioned || flex_item {
                match style.z_index {
                    ZIndex::Integer(z) => z,
                    ZIndex::Auto => 0,
                }
            } else {
                0
            };
            let context = matches!(style.position, Position::Fixed | Position::Sticky)
                || !node.computed.transform.is_none()
                || style.isolation == Isolation::Isolate
                || ((positioned || flex_item) && matches!(style.z_index, ZIndex::Integer(_)));
            let (current, real) = if context || positioned || flex_item || node.scroll.is_some() {
                let index = groups.len();
                groups.push(Group {
                    root: Some(id),
                    content: if node.widget.as_ref().is_some_and(|w| w.paints_content()) {
                        vec![id]
                    } else {
                        Vec::new()
                    },
                    ..Default::default()
                });
                if context || positioned {
                    let item = (z, node.tree_order, index);
                    if z < 0 {
                        groups[real].negative.push(item);
                    } else if z > 0 {
                        groups[real].positive.push(item);
                    } else {
                        groups[real].zero.push(item);
                    }
                } else {
                    groups[current].normal.push(Item::Group(index));
                }
                (index, if context { index } else { real })
            } else {
                groups[current].normal.push(Item::Box(id));
                if node.widget.as_ref().is_some_and(|w| w.paints_content()) {
                    groups[current].content.push(id);
                }
                (current, real)
            };
            for child in &node.children {
                visit(tree, *child, current, real, groups);
            }
        }
        for child in &tree.nodes[root].children {
            visit(tree, *child, 0, 0, &mut groups);
        }
        for group in &mut groups {
            group.negative.sort_by_key(|(z, order, _)| (*z, *order));
            group.zero.sort_by_key(|(_, order, _)| *order);
            group.positive.sort_by_key(|(z, order, _)| (*z, *order));
        }
        fn emit(tree: &WidgetTree, groups: &[Group], index: usize, out: &mut Vec<PaintEntry>) {
            let group = &groups[index];
            let entry = |id, phase| PaintEntry {
                id,
                phase,
                clip: tree.entry_clip(id, phase),
                space: tree.entry_space(id, phase),
            };
            if let Some(root) = group.root {
                out.push(entry(root, Phase::Box));
            }
            for (_, _, child) in &group.negative {
                emit(tree, groups, *child, out);
            }
            for item in &group.normal {
                match item {
                    Item::Box(id) => out.push(entry(*id, Phase::Box)),
                    Item::Group(group) => emit(tree, groups, *group, out),
                }
            }
            for id in &group.content {
                out.push(entry(*id, Phase::Content));
            }
            for (_, _, child) in group.zero.iter().chain(&group.positive) {
                emit(tree, groups, *child, out);
            }
            if let Some(root) = group.root
                && tree.nodes[root].scroll.is_some()
            {
                out.push(entry(root, Phase::Scrollbar));
            }
        }
        let start = self.entries.len();
        emit(tree, &groups, 0, &mut self.entries);
        self.finish_scrollbar_order(tree, start);
    }
    fn finish_scrollbar_order(&mut self, tree: &WidgetTree, start: usize) {
        // A positioned descendant may escape its parent's paint-as-if group.
        // Keep scroll controls above those descendants too, using layout ancestry
        // so viewport-fixed and top-layer content never participates accidentally.
        let mut bars = std::collections::HashMap::new();
        for entry in &self.entries[start..] {
            if entry.phase == Phase::Scrollbar {
                bars.insert(entry.id, (0usize, 0usize, entry.clone()));
            }
        }
        if bars.is_empty() {
            return;
        }
        let body: Vec<_> = self.entries[start..]
            .iter()
            .cloned()
            .filter(|e| e.phase != Phase::Scrollbar)
            .collect();
        for (index, entry) in body.iter().enumerate() {
            let mut current = Some(entry.id);
            while let Some(id) = current {
                if let Some(bar) = bars.get_mut(&id) {
                    bar.0 = index;
                }
                current = tree.nodes[id].layout_parent;
            }
        }
        for (id, bar) in &mut bars {
            let mut current = tree.nodes[*id].layout_parent;
            while let Some(id) = current {
                bar.1 += 1;
                current = tree.nodes[id].layout_parent;
            }
        }
        let mut bars: Vec<_> = bars.into_values().collect();
        bars.sort_by_key(|(at, depth, _)| (*at, std::cmp::Reverse(*depth)));
        self.entries.truncate(start);
        let mut bars = bars.into_iter().peekable();
        for (index, entry) in body.into_iter().enumerate() {
            self.entries.push(entry);
            while bars.peek().is_some_and(|b| b.0 == index) {
                self.entries.push(bars.next().unwrap().2);
            }
        }
    }
}

impl WidgetTree {
    // Clipping follows containing-block ancestry. An absolute descendant can
    // bypass intervening static overflow ancestors; a viewport-fixed/top-layer
    // root starts with no ancestor clip. DOM inheritance/selector links stay intact.
    pub(crate) fn node_clip(&self, id: WidgetId) -> Clip {
        let mut clip = Clip::default();
        let mut current = self.nodes[id].layout_parent;
        while let Some(id) = current {
            let node = &self.nodes[id];
            let b = self.scrollport(id);
            let next = Clip {
                x: (node.computed.scroll.overflow.x != crate::Overflow::Visible)
                    .then_some((b.origin.x, b.origin.x + b.size.width)),
                y: (node.computed.scroll.overflow.y != crate::Overflow::Visible)
                    .then_some((b.origin.y, b.origin.y + b.size.height)),
            };
            clip = clip.intersect(next);
            current = node.layout_parent;
        }
        clip
    }
    pub(crate) fn entry_clip(&self, id: WidgetId, phase: Phase) -> Clip {
        if self.nodes[id].visual.is_some() {
            return Clip::default();
        }
        let clip = self.node_clip(id);
        if phase != Phase::Content {
            return clip;
        }
        let b = self.scrollport(id);
        let o = self.nodes[id].computed.scroll.overflow;
        clip.intersect(Clip {
            x: (o.x != crate::Overflow::Visible).then_some((b.origin.x, b.origin.x + b.size.width)),
            y: (o.y != crate::Overflow::Visible)
                .then_some((b.origin.y, b.origin.y + b.size.height)),
        })
    }
    pub(crate) fn entry_contains(&self, entry: &PaintEntry, point: Point<f32>) -> bool {
        entry.clip.contains(point)
            && entry.space.as_ref().is_none_or(|space| {
                space.inverse.is_some()
                    && space
                        .clips
                        .as_ref()
                        .is_none_or(|c| c.contains([point.x, point.y]))
            })
    }
    pub(crate) fn ensure_paint_order(&self) {
        if self.paint_order_dirty.replace(false) {
            self.paint_clip_dirty.set(false);
            let mut order = self.paint_order.borrow_mut();
            order.rebuild(self);
            order.revision += 1;
        } else if self.paint_clip_dirty.replace(false) {
            // Geometry changes refresh clips but do not re-sort unchanged contexts.
            let mut order = self.paint_order.borrow_mut();
            for entry in &mut order.entries {
                if entry.phase != Phase::Backdrop {
                    entry.clip = self.entry_clip(entry.id, entry.phase);
                }
                entry.space = self.entry_space(entry.id, entry.phase);
            }
            order.revision += 1;
        }
    }
    /// Diagnostic count: color animations and unchanged redraws reuse the paint order.
    pub fn paint_order_rebuilds(&self) -> u64 {
        self.paint_order.borrow().rebuilds
    }
    /// The frontmost CSS-visible, pointer-enabled target. Shadows do not enlarge
    /// hit regions. Backdrops are distinct targets; modal inertness still applies
    /// when a backdrop has pointer-events:none.
    pub fn hit_test(&self, point: Point<f32>) -> Option<HitTarget> {
        let mut target = None;
        self.visit_hit_regions(|region| {
            if region.contains(point) {
                target = Some(region.target);
                false
            } else {
                true
            }
        });
        target
    }
    /// Native window hit tests consume exactly the same clipped, front-to-back
    /// rectangles as pointer dispatch. No parallel z-order or modal rules exist.
    pub(crate) fn visit_hit_regions(&self, mut visit: impl FnMut(HitRegion) -> bool) {
        if !self.layout_ready {
            return;
        }
        self.ensure_paint_order();
        let viewport = Clip {
            x: Some((
                self.viewport.origin.x,
                self.viewport.origin.x + self.viewport.layout.size.width,
            )),
            y: Some((
                self.viewport.origin.y,
                self.viewport.origin.y + self.viewport.layout.size.height,
            )),
        };
        for entry in self.paint_order.borrow().entries.iter().rev() {
            let node = &self.nodes[entry.id];
            if self.is_inert(entry.id) {
                continue;
            }
            let clip = entry.clip.intersect(viewport);
            let visible = |layer: &crate::style::layer::LayerStyle| {
                layer.visibility == Visibility::Visible
                    && layer.pointer_events != PointerEvents::None
            };
            let mut emit = |bounds, target| {
                visit(HitRegion {
                    bounds,
                    clip,
                    target,
                    phase: entry.phase,
                    space: entry.space.clone(),
                })
            };
            if entry.phase == Phase::Backdrop {
                if let Some(backdrop) = &node.backdrop
                    && backdrop.style.layout.display != Display::None
                    && visible(&backdrop.style.layer)
                    && !emit(backdrop.bounds, HitTarget::Backdrop(entry.id))
                {
                    return;
                }
            } else if visible(&node.computed.layer)
                && !node.visual.as_ref().is_some_and(|v| v.inverse.is_none())
            {
                if entry.phase == Phase::Scrollbar {
                    for axis in [
                        super::scroll::ScrollAxis::Horizontal,
                        super::scroll::ScrollAxis::Vertical,
                    ] {
                        if let Some(g) = self.scrollbar_geometry(entry.id, axis)
                            && !emit(g.track, HitTarget::Element(entry.id))
                        {
                            return;
                        }
                    }
                } else if !emit(node.global_bounds, HitTarget::Element(entry.id)) {
                    return;
                }
            }
        }
    }
    pub(crate) fn layout_backdrops(&mut self) {
        let size = self.viewport.layout.size;
        let origin = self.viewport.origin;
        for node in self.nodes.values_mut() {
            if let Some(backdrop) = &mut node.backdrop {
                backdrop.layout =
                    super::layout::layout_positioned_box(&backdrop.style.layout, size);
                let b = &backdrop.layout;
                backdrop.bounds = Rect::from_xywh(
                    origin.x + b.location.x,
                    origin.y + b.location.y,
                    b.size.width,
                    b.size.height,
                );
            }
        }
    }
}
fn inside(b: Rect<f32>, p: Point<f32>) -> bool {
    p.x >= b.origin.x
        && p.x < b.origin.x + b.size.width
        && p.y >= b.origin.y
        && p.y < b.origin.y + b.size.height
}

#[derive(Debug, Clone)]
pub(crate) struct HitRegion {
    pub space: Option<std::sync::Arc<crate::render::PaintSpace>>,
    pub phase: Phase,
    pub bounds: Rect<f32>,
    pub clip: Clip,
    pub target: HitTarget,
}
impl HitRegion {
    pub fn contains(&self, point: Point<f32>) -> bool {
        if !self.clip.contains(point) {
            return false;
        }
        if let Some(space) = &self.space {
            let p = [point.x, point.y];
            if !space.clips.as_ref().is_none_or(|c| c.contains(p)) {
                return false;
            }
            let Some(inverse) = space.inverse else {
                return false;
            };
            let [x, y] = inverse.map(p);
            inside(self.bounds, Point::new(x, y))
        } else {
            inside(self.bounds, point)
        }
    }
}
