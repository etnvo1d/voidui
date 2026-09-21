//! Reusable component content without cloning retained widget instances.
#![doc = include_str!("../../docs/composition.md")]
use super::{
    component::{ComponentElement, component},
    element::IntoElement,
};
use std::rc::Rc;

/// Content accepted by a component's `.child(...)` method. Pass a component
/// description directly, or a repeatable closure for a raw widget tree. Closures
/// execute in a child component scope, never during the container's render.
pub trait IntoChild {
    fn into_child(self) -> ComponentElement;
}
impl IntoChild for ComponentElement {
    fn into_child(self) -> ComponentElement {
        self
    }
}
impl<F, E> IntoChild for F
where
    F: FnMut() -> E + 'static,
    E: IntoElement,
{
    fn into_child(self) -> ComponentElement {
        component(self)
    }
}
impl IntoChild for &'static str {
    fn into_child(self) -> ComponentElement {
        component(move || self)
    }
}
impl IntoChild for String {
    fn into_child(self) -> ComponentElement {
        component(move || self.clone())
    }
}

/// An immutable, cheaply cloned content list. Each placement mounts independent
/// component state. Descriptions share captured inputs and callbacks, not mounted
/// hooks or widget instances. Mutable closure captures are shared; use `state()`
/// inside child components for state that belongs to an individual placement.
#[derive(Clone, Default)]
pub struct Children(Option<Rc<Vec<ComponentElement>>>);
impl Children {
    pub fn new() -> Self {
        Self::default()
    }

    /// Append reusable content, retaining its component key and modifiers.
    pub fn child(mut self, child: impl IntoChild) -> Self {
        Rc::make_mut(self.0.get_or_insert_with(|| Rc::new(Vec::new()))).push(child.into_child());
        self
    }

    /// Append a list in order. Use keys when its items can be inserted or moved.
    pub fn children(mut self, children: impl IntoIterator<Item = impl IntoChild>) -> Self {
        let mut children = children.into_iter().peekable();
        if children.peek().is_some() {
            Rc::make_mut(self.0.get_or_insert_with(|| Rc::new(Vec::new())))
                .extend(children.map(IntoChild::into_child));
        }
        self
    }
    fn as_slice(&self) -> &[ComponentElement] {
        self.0.as_deref().map_or(&[], Vec::as_slice)
    }
    pub fn len(&self) -> usize {
        self.as_slice().len()
    }
    pub fn is_empty(&self) -> bool {
        self.as_slice().is_empty()
    }
    pub fn iter(&self) -> impl ExactSizeIterator<Item = ComponentElement> + '_ {
        self.as_slice().iter().cloned()
    }

    /// Forward one root without adding a layout node. A transparent scope needs
    /// exactly one child; put multiple siblings in a layout container explicitly.
    pub fn single(&self) -> ComponentElement {
        assert_eq!(
            self.len(),
            1,
            "transparent content requires exactly one child"
        );
        self.as_slice()[0].clone()
    }
}
impl PartialEq for Children {
    fn eq(&self, other: &Self) -> bool {
        (self.is_empty() && other.is_empty())
            || self
                .0
                .as_ref()
                .zip(other.0.as_ref())
                .is_some_and(|(a, b)| Rc::ptr_eq(a, b))
    }
}
impl Eq for Children {}
/// An owned iterator that shares the content list instead of copying its vector.
pub struct ChildrenIter {
    children: Children,
    position: usize,
}
impl Iterator for ChildrenIter {
    type Item = ComponentElement;
    fn next(&mut self) -> Option<Self::Item> {
        let child = self.children.as_slice().get(self.position)?.clone();
        self.position += 1;
        Some(child)
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.children.len() - self.position;
        (remaining, Some(remaining))
    }
}
impl ExactSizeIterator for ChildrenIter {}
impl std::iter::FusedIterator for ChildrenIter {}
impl IntoIterator for Children {
    type Item = ComponentElement;
    type IntoIter = ChildrenIter;
    fn into_iter(self) -> Self::IntoIter {
        ChildrenIter {
            children: self,
            position: 0,
        }
    }
}
impl IntoIterator for &Children {
    type Item = ComponentElement;
    type IntoIter = ChildrenIter;
    fn into_iter(self) -> Self::IntoIter {
        self.clone().into_iter()
    }
}
