//! Retained function components. See the state guide for identity and input rules.
#![doc = include_str!("../../docs/state.md")]
use super::element::{Element, ElementKind, ElementProps, IntoElement};
use std::{
    any::{Any, TypeId},
    cell::RefCell,
    rc::Rc,
};
use winit::keyboard::SmolStr;

/// A cheap, shared callback. Cloning does not clone its captures.
pub struct Callback<T>(Option<Rc<dyn Fn(T)>>);
impl<T> Clone for Callback<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}
impl<T> Callback<T> {
    pub fn new(callback: impl Fn(T) + 'static) -> Self {
        Self(Some(Rc::new(callback)))
    }
    pub fn call(&self, value: T) {
        if let Some(callback) = &self.0 {
            callback(value);
        }
    }
}
/// An omitted notification callback intentionally does nothing.
impl<T> Default for Callback<T> {
    fn default() -> Self {
        Self(None)
    }
}
impl<T, F: Fn(T) + 'static> From<F> for Callback<T> {
    fn from(callback: F) -> Self {
        Self::new(callback)
    }
}
impl<T> From<&Callback<T>> for Callback<T> {
    fn from(callback: &Callback<T>) -> Self {
        callback.clone()
    }
}
impl<T> PartialEq for Callback<T> {
    fn eq(&self, other: &Self) -> bool {
        match (&self.0, &other.0) {
            (None, None) => true,
            (Some(a), Some(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }
}
impl<T> Eq for Callback<T> {}

#[derive(Clone)]
struct MemoInputs {
    value: Rc<dyn Any>,
    equal: fn(&dyn Any, &dyn Any) -> bool,
}

/// A deferred component invocation; modifiers never add a CSS or layout node.
/// Clones share their render closure and captured inputs, but each mounting owns
/// independent hooks. Put per-placement mutable data in `state`, not in a mutable
/// closure capture. A render closure must not synchronously reenter its own clone.
#[derive(Clone)]
pub struct ComponentElement {
    pub(crate) events: super::interaction::EventBindings,
    pub(crate) kind: TypeId,
    pub(crate) render: Rc<RefCell<dyn FnMut() -> Element>>,
    memo: Option<Rc<MemoInputs>>,
    pub(crate) key: Option<SmolStr>,
    pub(crate) id: Option<SmolStr>,
    pub(crate) classes: Vec<SmolStr>,
    // Unstyled components retain only a pointer-sized empty slot. Allocate the
    // root overlay on the first setter, without invoking the component body.
    pub(crate) style: Option<Box<crate::style::style::Style>>,
}
impl ComponentElement {
    crate::core::interaction::event_methods!();
    /// Retain a repeatable closure. Its type identifies the component across updates.
    pub fn new<F, E>(mut render: F) -> Self
    where
        F: FnMut() -> E + 'static,
        E: IntoElement,
    {
        Self {
            events: Default::default(),
            kind: TypeId::of::<F>(),
            render: Rc::new(RefCell::new(move || render().into_element())),
            memo: None,
            key: None,
            id: None,
            classes: Vec::new(),
            style: None,
        }
    }
    /// Construct a named component from normalized inputs. Its identity includes
    /// the declaration and retained input types, never call-site conversion types.
    #[doc(hidden)]
    pub fn with_inputs<C, P, F, E>(inputs: P, mut render: F) -> Self
    where
        C: 'static,
        P: 'static,
        F: FnMut(&P) -> E + 'static,
        E: IntoElement,
    {
        let mut result = Self::new(move || render(&inputs));
        result.kind = TypeId::of::<(C, P)>();
        result
    }
    #[doc(hidden)]
    pub fn with_memo_inputs<C, P, F, E>(inputs: P, render: F) -> Self
    where
        C: 'static,
        P: PartialEq + 'static,
        F: FnMut(&P) -> E + 'static,
        E: IntoElement,
    {
        let mut result = Self::memoized(inputs, render);
        result.kind = TypeId::of::<(C, P)>();
        result
    }
    /// Retain comparable inputs. Equal inputs skip a parent-driven execution;
    /// this component's own state dependencies always remain active.
    pub fn memoized<P, F, E>(inputs: P, mut render: F) -> Self
    where
        P: PartialEq + 'static,
        F: FnMut(&P) -> E + 'static,
        E: IntoElement,
    {
        let inputs = Rc::new(inputs);
        let retained = inputs.clone();
        let mut result = Self::new(move || render(&retained));
        result.memo = Some(Rc::new(MemoInputs {
            value: inputs,
            equal: |a, b| {
                a.downcast_ref::<P>()
                    .zip(b.downcast_ref::<P>())
                    .is_some_and(|(a, b)| a == b)
            },
        }));
        result
    }
    pub(crate) fn same_inputs(&self, next: &Self) -> bool {
        // Wrapper events may have new captures even when the body inputs match.
        // Reexecute when modifiers change so no event/style update is lost.
        self.id == next.id
            && self.classes == next.classes
            && self.style == next.style
            && self.events.is_empty()
            && next.events.is_empty()
            && self
                .memo
                .as_ref()
                .zip(next.memo.as_ref())
                .is_some_and(|(old, new)| (old.equal)(old.value.as_ref(), new.value.as_ref()))
    }
    /// Preserve this sibling instance across insertion and reordering.
    pub fn key(mut self, key: impl Into<SmolStr>) -> Self {
        self.key = Some(key.into());
        self
    }
    /// Override the rendered root's CSS ID without changing component identity.
    pub fn id(mut self, id: impl Into<SmolStr>) -> Self {
        self.id = Some(id.into());
        self
    }
    /// Add whitespace-separated classes to the rendered root.
    pub fn class(mut self, classes: impl Into<SmolStr>) -> Self {
        for class in classes.into().split_ascii_whitespace() {
            if !self.classes.iter().any(|s| s == class) {
                self.classes.push(class.into());
            }
        }
        self
    }
    /// Alias for class, matching the function-component design examples.
    pub fn add_class(self, classes: impl Into<SmolStr>) -> Self {
        self.class(classes)
    }
}
impl crate::style::builder::StyleTarget for ComponentElement {
    fn __style_mut(&mut self) -> &mut crate::style::style::Style {
        self.style.get_or_insert_with(Default::default)
    }
}
impl IntoElement for ComponentElement {
    fn into_element(self) -> Element {
        Element {
            events: Default::default(),
            kind: ElementKind::Component(Box::new(self)),
            props: ElementProps::new(Default::default()),
            children: Vec::new(),
        }
    }
}

/// Build a component without a macro. Captures need not be Clone when the closure
/// can render repeatedly by borrowing them. Use owned captures, such as Rc models.
pub fn component<F, E>(render: F) -> ComponentElement
where
    F: FnMut() -> E + 'static,
    E: IntoElement,
{
    ComponentElement::new(render)
}

/// A repeatable async callback for component inputs. Each call retains its captured
/// values until the returned future completes; the caller chooses its task scope.
pub struct AsyncCallback<A, R = ()> {
    call: Rc<AsyncCall<A, R>>,
}
type AsyncCall<A, R> = dyn Fn(A) -> std::pin::Pin<Box<dyn std::future::Future<Output = R>>>;
impl<A, R> Clone for AsyncCallback<A, R> {
    fn clone(&self) -> Self {
        Self {
            call: self.call.clone(),
        }
    }
}
impl<A: 'static, R: 'static> AsyncCallback<A, R> {
    pub fn new(callback: impl AsyncFn(A) -> R + 'static) -> Self {
        let callback = Rc::new(callback);
        Self {
            call: Rc::new(move |arg| {
                let callback = callback.clone();
                Box::pin(async move { callback(arg).await })
            }),
        }
    }
    pub fn call(&self, arg: A) -> impl std::future::Future<Output = R> + use<A, R> {
        (self.call)(arg)
    }
}
impl<A: 'static, R: 'static, F: AsyncFn(A) -> R + 'static> From<F> for AsyncCallback<A, R> {
    fn from(callback: F) -> Self {
        Self::new(callback)
    }
}
impl<A, R> From<&AsyncCallback<A, R>> for AsyncCallback<A, R> {
    fn from(callback: &AsyncCallback<A, R>) -> Self {
        callback.clone()
    }
}
impl<A, R> PartialEq for AsyncCallback<A, R> {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.call, &other.call)
    }
}
impl<A, R> Eq for AsyncCallback<A, R> {}

/// Storage used by generated named-property builders.
#[doc(hidden)]
#[derive(Clone, Default)]
pub struct ComponentOptions {
    key: Option<SmolStr>,
    id: Option<SmolStr>,
    classes: Vec<SmolStr>,
    style: Option<Box<crate::style::style::Style>>,
}
#[doc(hidden)]
pub type ComponentString = SmolStr;
impl ComponentOptions {
    pub fn key(&mut self, key: impl Into<SmolStr>) {
        self.key = Some(key.into());
    }
    pub fn id(&mut self, id: impl Into<SmolStr>) {
        self.id = Some(id.into());
    }
    pub fn class(&mut self, classes: impl Into<SmolStr>) {
        for class in classes.into().split_ascii_whitespace() {
            if !self.classes.iter().any(|old| old == class) {
                self.classes.push(class.into());
            }
        }
    }
    pub fn apply(
        self,
        mut component: ComponentElement,
        events: super::interaction::EventBindings,
    ) -> ComponentElement {
        component.key = self.key;
        component.id = self.id;
        component.classes = self.classes;
        component.style = self.style;
        component.events.append(events);
        component
    }
}
impl crate::style::builder::StyleTarget for ComponentOptions {
    fn __style_mut(&mut self) -> &mut crate::style::style::Style {
        self.style.get_or_insert_with(Default::default)
    }
}

/// Keep named-property builders compatible with component wrapper modifiers.
#[doc(hidden)]
#[macro_export]
macro_rules! __voidui_component_modifiers {
    () => {
        $crate::__voidui_event_methods!();
        /// Preserve this sibling across insertion and reordering.
        pub fn key(mut self, key: impl Into<$crate::core::component::ComponentString>) -> Self {
            self.options.key(key);
            self
        }
        /// Override the rendered root's CSS ID.
        pub fn id(mut self, id: impl Into<$crate::core::component::ComponentString>) -> Self {
            self.options.id(id);
            self
        }
        /// Add classes to the rendered root without inserting a layout node.
        pub fn class(
            mut self,
            classes: impl Into<$crate::core::component::ComponentString>,
        ) -> Self {
            self.options.class(classes);
            self
        }
        /// Add classes to the rendered root.
        pub fn add_class(
            self,
            classes: impl Into<$crate::core::component::ComponentString>,
        ) -> Self {
            self.class(classes)
        }
    };
}

#[cfg(test)]
mod style_tests {
    use crate::{Children, div};

    #[crate::component]
    fn internal_panel(#[prop(default = 4)] padding: i32, children: Children) {
        div().padding(padding).children(children)
    }

    #[test]
    fn internal_builders_share_the_api_and_allocate_styles_only_on_use() {
        let plain = internal_panel().build();
        assert!(plain.style.is_none());
        let styled = internal_panel().padding(9).p_4().child("Content").build();
        assert!(styled.style.is_some());
        let mut tree = crate::core::widget_tree::WidgetTree::new();
        tree.build_root(styled);
        tree.update_styles(std::time::Instant::now());
        let root = tree.root().unwrap();
        assert_eq!(
            tree.layout_style(root).padding.left,
            taffy::LengthPercentage::length(16.0)
        );
        assert_eq!(tree.children(root).len(), 1);
    }
}
