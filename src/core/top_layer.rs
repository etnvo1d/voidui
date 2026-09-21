//! Low-level top-layer lifecycle. Callers build ordinary elements and style them
//! with CSS; no modal, popover or tooltip component is supplied.
#![doc = include_str!("../../docs/overlays.md")]
use super::{widget::WidgetId, widget_tree::WidgetTree};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TopLayerKind {
    Modal,
    Popover,
}
#[derive(Debug, Clone, Copy)]
pub(crate) struct TopLayerEntry {
    pub id: WidgetId,
    pub kind: TopLayerKind,
    pub restore_focus: Option<WidgetId>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HitTarget {
    Element(WidgetId),
    Backdrop(WidgetId),
}

impl WidgetTree {
    pub fn is_top_layer(&self, id: WidgetId) -> bool {
        self.nodes.get(id).is_some_and(|n| n.top_layer.is_some())
    }
    pub fn top_layer_kind(&self, id: WidgetId) -> Option<TopLayerKind> {
        self.nodes.get(id).and_then(|n| n.top_layer)
    }
    pub fn top_layer(&self) -> impl DoubleEndedIterator<Item = WidgetId> + '_ {
        self.top_layers.iter().map(|entry| entry.id)
    }
    pub fn active_modal(&self) -> Option<WidgetId> {
        self.top_layers
            .iter()
            .rev()
            .find(|e| e.kind == TopLayerKind::Modal)
            .map(|e| e.id)
    }
    pub fn focused(&self) -> Option<WidgetId> {
        self.statused_widgets.focused
    }

    /// Promote an existing element, mark it :modal/[open], and make the rest of
    /// the document inert. Opening an already-open modal is an idempotent no-op.
    pub fn show_modal(&mut self, id: WidgetId) -> anyhow::Result<bool> {
        self.show_top_layer(id, TopLayerKind::Modal)
    }
    /// Promote an existing element without modality. This is a manual popover:
    /// application Rust controls dismissal and optional focus behavior.
    pub fn show_popover(&mut self, id: WidgetId) -> anyhow::Result<bool> {
        self.show_top_layer(id, TopLayerKind::Popover)
    }
    fn show_top_layer(&mut self, id: WidgetId, kind: TopLayerKind) -> anyhow::Result<bool> {
        anyhow::ensure!(self.nodes.contains_key(id), "stale overlay element");
        anyhow::ensure!(
            Some(id) != self.root(),
            "the document root cannot enter the top layer"
        );
        if let Some(existing) = self.nodes[id].top_layer {
            anyhow::ensure!(
                existing == kind,
                "close the existing top-layer mode before changing it"
            );
            return Ok(false);
        }
        let restore_focus = self.focused();
        self.nodes[id].top_layer = Some(kind);
        if kind == TopLayerKind::Modal {
            self.set_attribute(id, "open", Some(""));
        }
        self.top_layers.push(TopLayerEntry {
            id,
            kind,
            restore_focus,
        });
        self.css_dirty = true;
        self.layout_ready = false;
        self.paint_order_dirty.set(true);
        self.selection.get_mut().invalidate(false);
        self.end_selection_drag();
        if kind == TopLayerKind::Modal {
            let focus = self
                .descendants(id)
                .find(|child| {
                    self.nodes[*child]
                        .props
                        .attributes
                        .contains_key("autofocus")
                        && !self.nodes[*child].props.attributes.contains_key("disabled")
                        && !self.is_inert(*child)
                })
                .unwrap_or(id);
            self.set_focused(Some(focus));
        }
        self.validate_pointer_capture();
        Ok(true)
    }
    /// Close this entry and its DOM-descendant top-layer entries, deepest/latest
    /// first. There is no nesting limit or reserved range of numeric z-indices.
    /// Closing an already-closed or stale element is a no-op.
    pub fn close_top_layer(&mut self, id: WidgetId) -> bool {
        let Some(index) = self.top_layers.iter().position(|entry| entry.id == id) else {
            return false;
        };
        let restore = self.top_layers[index].restore_focus;
        let ids: Vec<_> = self
            .top_layers
            .iter()
            .filter(|entry| self.is_descendant_or_self(entry.id, id))
            .map(|e| e.id)
            .collect();
        for id in ids.iter().rev() {
            if self.nodes[*id].top_layer == Some(TopLayerKind::Modal) {
                self.set_attribute(*id, "open", None);
            }
            self.nodes[*id].top_layer = None;
            self.nodes[*id].backdrop = None;
        }
        let removed: std::collections::HashSet<_> = ids.iter().copied().collect();
        self.top_layers.retain(|e| !removed.contains(&e.id));
        self.css_dirty = true;
        self.layout_ready = false;
        self.paint_order_dirty.set(true);
        self.selection.get_mut().invalidate(false);
        self.end_selection_drag();
        if self
            .focused()
            .is_some_and(|focus| ids.iter().any(|id| self.is_descendant_or_self(focus, *id)))
        {
            let focus = restore
                .filter(|id| self.nodes.contains_key(*id) && !self.is_inert(*id))
                .or_else(|| self.active_modal());
            self.set_focused(focus);
        }
        true
    }
    pub(crate) fn is_descendant_or_self(&self, mut id: WidgetId, ancestor: WidgetId) -> bool {
        loop {
            if id == ancestor {
                return true;
            }
            match self.nodes.get(id).and_then(|node| node.parent) {
                Some(parent) => id = parent,
                None => return false,
            }
        }
    }
    pub fn is_inert(&self, id: WidgetId) -> bool {
        if !self.nodes.contains_key(id) {
            return true;
        }
        if self
            .active_modal()
            .is_some_and(|modal| !self.is_descendant_or_self(id, modal))
        {
            return true;
        }
        let modal = self.active_modal();
        let mut current = Some(id);
        while let Some(id) = current {
            let node = &self.nodes[id];
            if node.props.attributes.contains_key("inert") {
                return true;
            }
            if Some(id) == modal {
                break;
            }
            current = node.parent;
        }
        false
    }
    fn descendants(&self, id: WidgetId) -> impl Iterator<Item = WidgetId> + '_ {
        let mut stack = vec![id];
        std::iter::from_fn(move || {
            let id = stack.pop()?;
            stack.extend(self.nodes[id].children.iter().rev().copied());
            Some(id)
        })
    }
    /// Move keyboard focus in CSS-visible, non-inert DOM order. Positive tabindex
    /// precedes zero/default tabindex; the active modal traps traversal in its subtree.
    pub fn focus_next(&mut self, reverse: bool) -> bool {
        let mut candidates: Vec<_> = self
            .nodes
            .iter()
            .filter_map(|(id, node)| {
                if self.is_inert(id)
                    || !self.is_rendered(id)
                    || node.computed.layer.visibility != crate::style::layer::Visibility::Visible
                    || node.props.attributes.contains_key("disabled")
                {
                    return None;
                }
                let tab = node
                    .props
                    .attributes
                    .get("tabindex")
                    .and_then(|s| s.parse::<i32>().ok())
                    .or_else(|| {
                        matches!(
                            node.props.tag.as_str(),
                            "button" | "input" | "select" | "textarea"
                        )
                        .then_some(0)
                    })?;
                (tab >= 0).then_some((tab == 0, tab, node.tree_order, id))
            })
            .collect();
        candidates.sort_by_key(|(normal, tab, order, _)| (*normal, *tab, *order));
        let index = candidates
            .iter()
            .position(|(_, _, _, id)| Some(*id) == self.focused());
        let target = if candidates.is_empty() {
            self.active_modal()
        } else {
            let n = candidates.len();
            Some(
                candidates[match index {
                    Some(i) if reverse => (i + n - 1) % n,
                    Some(i) => (i + 1) % n,
                    None if reverse => n - 1,
                    None => 0,
                }]
                .3,
            )
        };
        let previous = self.focused();
        self.set_focused(target);
        previous != self.focused()
    }
    pub(crate) fn is_rendered(&self, mut id: WidgetId) -> bool {
        loop {
            let Some(node) = self.nodes.get(id) else {
                return false;
            };
            if node.computed.layout.display == super::layout::Display::None {
                return false;
            }
            match node.parent {
                Some(parent) => id = parent,
                None => return true,
            }
        }
    }
}
