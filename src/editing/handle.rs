//! Cheap shared ownership of a session. Reading borrows the document; text copies
//! and subscriptions are explicit. Only mounted views receive invalidations.
use super::{EditorState, SelectionSet};
use crate::core::updates::WidgetInvalidator;
use std::{
    cell::RefCell,
    rc::{Rc, Weak},
};

/// A selection-only policy applied after each shared-session mutation. Return
/// None to keep the selection. Policies must be idempotent and must not borrow
/// or update the same Editor; the supplied state is already borrowed.
pub type SelectionFilter = dyn Fn(&EditorState) -> Option<SelectionSet>;

struct Shared {
    state: RefCell<EditorState>,
    observers: RefCell<Vec<Weak<Observer>>>,
    selection_filters: RefCell<Vec<Weak<SelectionFilter>>>,
}
pub(crate) struct Observer {
    invalidate: WidgetInvalidator,
}
#[derive(Clone)]
pub struct Editor(Rc<Shared>);
impl Editor {
    pub fn new(text: impl Into<String>) -> Self {
        Self::from_state(EditorState::new(text))
    }
    pub fn from_rich(content: impl Into<super::RichText>) -> Self {
        Self::from_state(EditorState::from_rich(content))
    }
    pub fn from_state(state: EditorState) -> Self {
        Self(Rc::new(Shared {
            state: RefCell::new(state),
            observers: RefCell::new(Vec::new()),
            selection_filters: RefCell::new(Vec::new()),
        }))
    }
    /// Explicit rich snapshot. Borrow document spans with `with` for rendering.
    pub fn rich_text(&self) -> super::RichText {
        self.with(|s| s.document().rich_text())
    }
    pub fn with<R>(&self, read: impl FnOnce(&EditorState) -> R) -> R {
        read(&self.0.state.borrow())
    }
    /// Mutate the session and wake affected views once. Do not call read/update
    /// on this same handle from inside the closure; it already holds its borrow.
    pub fn update<R>(&self, edit: impl FnOnce(&mut EditorState) -> R) -> R {
        let (result, changed, empty_changed) = {
            let mut state = self.0.state.borrow_mut();
            let old = state.generation();
            let empty = state.document().is_empty() && state.composition().is_none();
            let result = edit(&mut state);
            self.filter_selection(&mut state);
            (
                result,
                state.generation() != old,
                empty != (state.document().is_empty() && state.composition().is_none()),
            )
        };
        if changed {
            self.0.observers.borrow_mut().retain(|weak| {
                let Some(observer) = weak.upgrade() else {
                    return false;
                };
                if empty_changed {
                    observer.invalidate.restyle();
                } else {
                    observer.invalidate.repaint();
                }
                true
            });
        }
        result
    }
    /// The owning widget already has a queued invalidation when applying its
    /// external String binding; avoid recursively queuing the same view again.
    pub(crate) fn synchronize<R>(&self, update: impl FnOnce(&mut EditorState) -> R) -> R {
        let mut state = self.0.state.borrow_mut();
        let result = update(&mut state);
        self.filter_selection(&mut state);
        result
    }
    /// Keep the returned guard alive to enforce a selection policy after edits,
    /// pointer navigation and history restoration, before views are notified.
    /// Policies run in registration order; corrections add no document edits or undo entries.
    /// Preedit owns its temporary selection, so filters wait until it ends.
    pub fn filter_selections(
        &self,
        filter: impl Fn(&EditorState) -> Option<SelectionSet> + 'static,
    ) -> Rc<SelectionFilter> {
        let filter: Rc<SelectionFilter> = Rc::new(filter);
        self.0
            .selection_filters
            .borrow_mut()
            .push(Rc::downgrade(&filter));
        self.update(|_| {});
        filter
    }

    fn filter_selection(&self, state: &mut EditorState) {
        if state.composition().is_some() {
            return;
        }
        self.0.selection_filters.borrow_mut().retain(|weak| {
            let Some(filter) = weak.upgrade() else {
                return false;
            };
            if let Some(selection) = filter(state)
                && let Err(error) = state.select(selection)
            {
                log::error!("editor selection filter rejected: {error}");
            }
            true
        });
    }

    pub fn same_session(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
    pub fn text(&self) -> String {
        self.with(|s| s.text().into())
    }
    pub(crate) fn subscribe(&self, invalidate: WidgetInvalidator) -> Rc<Observer> {
        let observer = Rc::new(Observer { invalidate });
        let mut observers = self.0.observers.borrow_mut();
        observers.retain(|o| o.strong_count() > 0);
        observers.push(Rc::downgrade(&observer));
        observer
    }
}
impl Default for Editor {
    fn default() -> Self {
        Self::new("")
    }
}
