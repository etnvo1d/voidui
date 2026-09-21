//! Match authored widgets to runtime handles without asking parsers to maintain
//! an ID registry. Keys are extension-local; unkeyed widgets follow source ranges.
use super::{Bias, ChangeSet, EditError, Projection, ReplacementContent, ViewId, ViewSpec};
use std::{
    any::TypeId,
    collections::{BTreeMap, BTreeSet, VecDeque},
    ops::Range,
};

#[derive(PartialEq, Eq, PartialOrd, Ord)]
enum Identity {
    Key(Option<ViewId>, smol_str::SmolStr),
    Range(Option<ViewId>, TypeId, usize, usize, bool),
}
fn identity(range: &Range<usize>, spec: &ViewSpec, block: bool) -> Identity {
    if let Some(key) = &spec.key {
        Identity::Key(spec.scope, key.clone())
    } else {
        Identity::Range(
            spec.scope,
            spec.description_type(),
            range.start,
            range.end,
            block,
        )
    }
}

impl Projection {
    pub(crate) fn widgets(&self) -> impl Iterator<Item = (ViewId, &Range<usize>, &ViewSpec, bool)> {
        self.replacements
            .iter()
            .filter_map(|r| match &r.content {
                ReplacementContent::Widget(spec) => Some((r.id, &r.range, spec, false)),
                _ => None,
            })
            .chain(
                self.blocks
                    .iter()
                    .filter_map(|b| b.widget.as_ref().map(|spec| (b.id, &b.range, spec, true))),
            )
    }
    fn widgets_mut(
        &mut self,
    ) -> impl Iterator<Item = (&mut ViewId, &Range<usize>, &mut ViewSpec, bool)> {
        self.replacements
            .iter_mut()
            .filter_map(|r| match &mut r.content {
                ReplacementContent::Widget(spec) => Some((&mut r.id, &r.range, spec, false)),
                _ => None,
            })
            .chain(self.blocks.iter_mut().filter_map(|b| {
                b.widget
                    .as_mut()
                    .map(|spec| (&mut b.id, &b.range, spec, true))
            }))
    }
    pub(crate) fn scope_widgets(&mut self, scope: ViewId) {
        for (_, _, spec, _) in self.widgets_mut() {
            spec.scope = Some(scope);
        }
    }
    pub(crate) fn view_ids(&self) -> BTreeSet<ViewId> {
        self.replacements
            .iter()
            .filter_map(|r| (!matches!(r.content, ReplacementContent::Text(_))).then_some(r.id))
            .chain(self.blocks.iter().map(|b| b.id))
            .collect()
    }

    /// Unkeyed instances survive known coordinate mappings only. When revisions
    /// were skipped, a key can still establish identity without guessing which
    /// occurrence of an equal formula moved where.
    pub(crate) fn match_widgets(
        &mut self,
        previous: Option<&Projection>,
        changes: Option<&ChangeSet>,
        match_ranges: bool,
        match_keys: bool,
    ) -> Result<(), EditError> {
        let used: BTreeSet<_> = self
            .replacements
            .iter()
            .filter_map(|r| (!matches!(r.content, ReplacementContent::Widget(_))).then_some(r.id))
            .chain(
                self.blocks
                    .iter()
                    .filter(|b| b.widget.is_none())
                    .map(|b| b.id),
            )
            .collect();
        let mut available: BTreeMap<Identity, VecDeque<ViewId>> = BTreeMap::new();
        let mut reserved = used.clone();
        if let Some(previous) = previous {
            reserved.extend(previous.view_ids());
            for (id, range, spec, block) in previous.widgets() {
                if used.contains(&id)
                    || if spec.key.is_some() {
                        !match_keys
                    } else {
                        !match_ranges
                    }
                {
                    continue;
                }
                let mut range = range.clone();
                if let Some(changes) = changes.filter(|_| spec.key.is_none()) {
                    // Fully replaced source denotes a new occurrence, even if it
                    // happens to have the same length and description type.
                    if !range.is_empty()
                        && changes.edits().iter().any(|edit| {
                            edit.range.start <= range.start && edit.range.end >= range.end
                        })
                    {
                        continue;
                    }
                    range = changes.map(range.start, Bias::After)
                        ..changes.map(
                            range.end,
                            if range.is_empty() {
                                Bias::After
                            } else {
                                Bias::Before
                            },
                        );
                }
                available
                    .entry(identity(&range, spec, block))
                    .or_default()
                    .push_back(id);
            }
        }
        let mut keys = BTreeSet::new();
        let mut next = 0u64;
        for (id, range, spec, block) in self.widgets_mut() {
            if let Some(key) = &spec.key {
                if !keys.insert((spec.scope, key.clone())) {
                    return Err(EditError::Rejected(
                        "duplicate widget key in projection scope",
                    ));
                }
            }
            let matched = available
                .get_mut(&identity(range, spec, block))
                .and_then(VecDeque::pop_front);
            *id = if let Some(matched) = matched {
                matched
            } else {
                while reserved.contains(&ViewId(next)) {
                    next = next
                        .checked_add(1)
                        .ok_or(EditError::Rejected("view ID space exhausted"))?;
                }
                ViewId(next)
            };
            reserved.insert(*id);
        }
        Ok(())
    }
}
