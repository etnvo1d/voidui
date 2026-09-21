//! Explicit extension ordering, lifecycle and invalidation. Plugins emit data;
//! only transactions mutate the document. UI factories stay on the UI thread.
use super::*;
use crate::core::{input::InputEvent, updates::WidgetInvalidator};
use std::{collections::BTreeMap, rc::Rc};

pub struct ExtensionContext<'a> {
    pub snapshot: &'a ProjectionSnapshot,
    /// Last atomic delta when available. Compare its input revision to your own
    /// index revision; rebuild when updates were skipped or history was grouped.
    pub change: Option<&'a Change>,
}
pub trait EditorExtension {
    fn project(&mut self, context: ExtensionContext<'_>) -> Result<Projection, EditError>;
    /// May handle committed text, paste and keys. Native Commit is also offered
    /// during composition using committed snapshot coordinates. A returned commit
    /// transaction atomically closes preedit; rejection preserves it.
    fn command(
        &mut self,
        _event: &InputEvent,
        _context: ExtensionContext<'_>,
    ) -> Result<Option<Transaction>, EditError> {
        Ok(None)
    }
    fn mounted(&mut self, _invalidator: Option<WidgetInvalidator>) {}
    fn unmounted(&mut self) {}
}
#[derive(Clone)]
struct Factory {
    priority: i32,
    create: Rc<dyn Fn() -> Box<dyn EditorExtension>>,
}
#[derive(Clone, Default)]
pub struct EditorExtensions {
    factories: BTreeMap<ViewId, Factory>,
}
impl EditorExtensions {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn is_empty(&self) -> bool {
        self.factories.is_empty()
    }
    pub fn register(
        mut self,
        id: ViewId,
        priority: i32,
        create: impl Fn() -> Box<dyn EditorExtension> + 'static,
    ) -> Self {
        self.factories.insert(
            id,
            Factory {
                priority,
                create: Rc::new(create),
            },
        );
        self
    }
    pub(crate) fn same(&self, other: &Self) -> bool {
        self.factories.len() == other.factories.len()
            && self.factories.iter().all(|(id, a)| {
                other
                    .factories
                    .get(id)
                    .is_some_and(|b| a.priority == b.priority && Rc::ptr_eq(&a.create, &b.create))
            })
    }
}
#[derive(Default)]
pub(crate) struct ExtensionHost {
    configuration: EditorExtensions,
    plugins: Vec<(ViewId, Box<dyn EditorExtension>)>,
}
impl ExtensionHost {
    pub fn configure(&mut self, config: EditorExtensions, invalidator: Option<WidgetInvalidator>) {
        if self.configuration.same(&config) {
            return;
        }
        for (_, p) in &mut self.plugins {
            p.unmounted();
        }
        self.plugins.clear();
        let mut ordered: Vec<_> = config.factories.iter().collect();
        ordered.sort_by_key(|(id, f)| (f.priority, **id));
        for (id, f) in ordered {
            let mut p = (f.create)();
            p.mounted(invalidator.clone());
            self.plugins.push((*id, p));
        }
        self.configuration = config;
    }
    pub fn project(
        &mut self,
        snapshot: &ProjectionSnapshot,
        base: Projection,
        change: Option<&Change>,
    ) -> Result<Projection, EditError> {
        let mut out = base;
        for (id, plugin) in &mut self.plugins {
            let mut layer = plugin.project(ExtensionContext { snapshot, change })?;
            layer.scope_widgets(*id);
            layer.validate(&snapshot.text)?;
            // Style precedence is ascending priority, then stable extension ID.
            // Overlapping structural replacements are errors, never order guesses.
            out.styles = super::highlights::overlay(&out.styles, &layer.styles);
            out.replacements.extend(layer.replacements);
            out.paragraphs.extend(layer.paragraphs);
            out.blocks.extend(layer.blocks);
        }
        out.validate(&snapshot.text)?;
        Ok(out)
    }
    pub fn command(
        &mut self,
        event: &InputEvent,
        snapshot: &ProjectionSnapshot,
        change: Option<&Change>,
    ) -> Result<Option<Transaction>, EditError> {
        for (_, plugin) in self.plugins.iter_mut().rev() {
            if let Some(tx) = plugin.command(event, ExtensionContext { snapshot, change })? {
                return Ok(Some(tx));
            }
        }
        Ok(None)
    }
}
impl Drop for ExtensionHost {
    fn drop(&mut self) {
        for (_, p) in &mut self.plugins {
            p.unmounted();
        }
    }
}

/// Stable source anchor. It is mapped through committed edits and undo/redo;
/// view-only folding and highlighting never move it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AnchorId(pub u64);
#[derive(Debug, Clone, Copy)]
pub struct Anchor {
    pub byte: usize,
    pub bias: Bias,
}
#[derive(Debug, Default)]
pub(crate) struct Anchors {
    pub next: u64,
    pub positions: BTreeMap<AnchorId, Anchor>,
}
impl Anchors {
    pub fn map(&mut self, changes: &ChangeSet) {
        for anchor in self.positions.values_mut() {
            anchor.byte = changes.map(anchor.byte, anchor.bias);
        }
    }
}
