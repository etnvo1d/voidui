//! Immutable, comparable descriptions are cheap to rebuild. Mounted views own
//! the widget trees, textures and input state that should survive equal descriptions.
use super::{EmbeddedView, WidgetView};
use std::{any::Any, fmt, rc::Rc};

/// Describe an embedded view using its render inputs, usually a `PartialEq` struct.
/// Equality must include every input that affects rendering or event handling.
/// Descriptions belong to the UI thread; parser snapshots remain thread-safe.
pub trait ViewDescription: PartialEq + 'static {
    type View: EmbeddedView;

    /// Called only when layout needs an instance, including after an offscreen
    /// instance was unmounted. Keep durable application state outside the view.
    fn create(&self) -> Self::View;

    /// Apply changed inputs to an existing view of this description type.
    /// Return true to preserve its interaction state, or false to replace it.
    /// Layout is invalidated automatically, including changes to the baseline.
    /// Returning false must leave the view unchanged.
    fn update(&self, _view: &mut Self::View) -> bool {
        false
    }
}

/// Type-erased description stored in a projection. Construct it through
/// `Replacement::widget` or `BlockView::widget`; no factory registration is needed.
#[derive(Clone)]
pub struct ViewSpec {
    inner: Rc<dyn ErasedDescription>,
    pub(crate) key: Option<smol_str::SmolStr>,
    pub(crate) scope: Option<super::ViewId>,
}
impl ViewSpec {
    pub(crate) fn new(description: impl ViewDescription) -> Self {
        Self {
            inner: Rc::new(description),
            key: None,
            scope: None,
        }
    }
    pub(crate) fn description_type(&self) -> std::any::TypeId {
        self.inner.as_ref().type_id()
    }
    pub(crate) fn create(&self) -> Box<dyn EmbeddedView> {
        self.inner.create_view()
    }
    pub(crate) fn update(&self, previous: &Self, view: &mut dyn EmbeddedView) -> bool {
        self.description_type() == previous.description_type() && self.inner.update_view(view)
    }
}
impl PartialEq for ViewSpec {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key && self.scope == other.scope && self.inner.same(other.inner.as_ref())
    }
}
impl fmt::Debug for ViewSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ViewSpec")
            .field("type", &self.description_type())
            .field("key", &self.key)
            .field("scope", &self.scope)
            .finish_non_exhaustive()
    }
}
trait ErasedDescription: Any {
    fn same(&self, other: &dyn ErasedDescription) -> bool;
    fn create_view(&self) -> Box<dyn EmbeddedView>;
    fn update_view(&self, view: &mut dyn EmbeddedView) -> bool;
}
impl<D: ViewDescription> ErasedDescription for D {
    fn same(&self, other: &dyn ErasedDescription) -> bool {
        (other as &dyn Any).downcast_ref::<D>() == Some(self)
    }
    fn create_view(&self) -> Box<dyn EmbeddedView> {
        Box::new(self.create())
    }
    fn update_view(&self, view: &mut dyn EmbeddedView) -> bool {
        (view as &mut dyn Any)
            .downcast_mut::<D::View>()
            .is_some_and(|view| self.update(view))
    }
}

/// Comparable inputs for a hosted widget subtree and its source-event policy.
pub struct WidgetDescription<P> {
    props: P,
    render: fn(&P) -> crate::Element,
    ignore_events: fn(&crate::core::input::InputEvent) -> bool,
    align: crate::render::InlineAlignment,
    offset_em: f32,
}
impl<P> WidgetDescription<P> {
    /// Align the widget using its surrounding font, including on object-only lines.
    pub fn inline_align(mut self, align: crate::render::InlineAlignment) -> Self {
        self.align = align;
        self
    }
    /// Apply a relative vertical offset in the surrounding text's em units.
    /// Painting and hit bounds move together; the line's occupied space stays put.
    pub fn inline_offset_em(mut self, offset: f32) -> Self {
        self.offset_em = offset;
        self
    }
    /// Return false for events that may fall back to the source editor.
    pub fn ignore_events(mut self, policy: fn(&crate::core::input::InputEvent) -> bool) -> Self {
        self.ignore_events = policy;
        self
    }
}
impl<P: PartialEq> PartialEq for WidgetDescription<P> {
    fn eq(&self, other: &Self) -> bool {
        self.props == other.props
            && self.align == other.align
            && self.offset_em == other.offset_em
            && std::ptr::fn_addr_eq(self.render, other.render)
            && std::ptr::fn_addr_eq(self.ignore_events, other.ignore_events)
    }
}
impl<P: Clone + PartialEq + 'static> ViewDescription for WidgetDescription<P> {
    type View = WidgetView;

    fn create(&self) -> WidgetView {
        let (props, render) = (self.props.clone(), self.render);
        WidgetView::new(move || render(&props))
            .ignore_events(self.ignore_events)
            .inline_align(self.align)
            .inline_offset_em(self.offset_em)
    }
    fn update(&self, view: &mut WidgetView) -> bool {
        let (props, render) = (self.props.clone(), self.render);
        view.rebuild(move || render(&props));
        view.set_event_policy(self.ignore_events);
        view.set_inline_align(self.align);
        view.set_inline_offset_em(self.offset_em);
        true
    }
}
impl WidgetView {
    /// Embed an existing component without implementing a description trait.
    /// Pass all changing inputs as comparable props; the render function must
    /// not capture hidden inputs. Changed props reconcile the existing subtree.
    pub fn describe<P: Clone + PartialEq + 'static>(
        props: P,
        render: fn(&P) -> crate::Element,
    ) -> WidgetDescription<P> {
        WidgetDescription {
            props,
            render,
            ignore_events: |_| true,
            align: Default::default(),
            offset_em: 0.0,
        }
    }
}
