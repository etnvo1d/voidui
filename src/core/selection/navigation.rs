//! Read-only keyboard extension over Unicode and document boundaries.
use super::*;
use unicode_segmentation::UnicodeSegmentation;

/// Logical movement used by keyboard extension. A range remains a single
/// anchor/focus pair; Unicode graphemes are never split by user navigation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionMove {
    /// Visual movement delegates bidi and line-boundary affinity to Parley.
    Left,
    Right,
    WordLeft,
    WordRight,
    Up,
    Down,
    Backward,
    Forward,
    WordBackward,
    WordForward,
    LineStart,
    LineEnd,
    DocumentStart,
    DocumentEnd,
}
impl WidgetTree {
    pub fn extend_selection(&mut self, movement: SelectionMove) -> bool {
        if matches!(
            movement,
            SelectionMove::Left
                | SelectionMove::Right
                | SelectionMove::WordLeft
                | SelectionMove::WordRight
                | SelectionMove::Up
                | SelectionMove::Down
        ) {
            return self.extend_visual_selection(movement);
        }
        self.ensure_selection_index();
        let result = {
            let state = self.selection.borrow();
            let Some(old) = state.range else {
                return false;
            };
            let forward = matches!(
                movement,
                SelectionMove::Forward
                    | SelectionMove::WordForward
                    | SelectionMove::LineEnd
                    | SelectionMove::DocumentEnd
            );
            let Some(current) = state.index.offset(self, old.focus) else {
                return false;
            };
            let eligible: Vec<_> = state
                .index
                .texts
                .iter()
                .copied()
                .filter(|id| {
                    self.selection_visible(*id) && self.used_select(*id) != UserSelect::None
                })
                .collect();
            if eligible.is_empty() {
                return false;
            }
            let at = eligible
                .iter()
                .position(|id| *id == old.focus.node())
                .unwrap_or_else(|| {
                    eligible
                        .iter()
                        .enumerate()
                        .min_by_key(|(_, id)| {
                            let base = state.index.spans[**id].text.unwrap();
                            let len = self.nodes[**id]
                                .widget
                                .as_ref()
                                .unwrap()
                                .text_content()
                                .unwrap()
                                .len();
                            if current < base {
                                base - current
                            } else {
                                current.saturating_sub(base + len)
                            }
                        })
                        .unwrap()
                        .0
                });
            let node = eligible[at];
            let text = self.nodes[node]
                .widget
                .as_ref()
                .unwrap()
                .text_content()
                .unwrap();
            let base = state.index.spans[node].text.unwrap();
            let byte = current.saturating_sub(base).min(text.len());
            let mut focus = match movement {
                SelectionMove::DocumentStart => {
                    self.contents(self.active_modal().or(self.root()).unwrap())
                        .anchor
                }
                SelectionMove::DocumentEnd => {
                    self.contents(self.active_modal().or(self.root()).unwrap())
                        .focus
                }
                SelectionMove::LineStart | SelectionMove::LineEnd => {
                    let prepared = self.nodes[node].widget.as_ref().unwrap().prepared_text();
                    let row = prepared.and_then(|p| p.row_range(byte));
                    SelectionPoint::text(
                        node,
                        row.map(|r| if forward { r.end } else { r.start })
                            .unwrap_or(byte),
                    )
                }
                _ => {
                    let mut boundaries = if matches!(
                        movement,
                        SelectionMove::WordBackward | SelectionMove::WordForward
                    ) {
                        text.split_word_bound_indices()
                            .flat_map(|(i, s)| [i, i + s.len()])
                            .collect::<Vec<_>>()
                    } else {
                        text.grapheme_indices(true)
                            .map(|(i, _)| i)
                            .chain([text.len()])
                            .collect()
                    };
                    boundaries.sort_unstable();
                    boundaries.dedup();
                    let target = if forward {
                        boundaries.into_iter().find(|b| *b > byte)
                    } else {
                        boundaries.into_iter().rev().find(|b| *b < byte)
                    };
                    if let Some(byte) = target {
                        SelectionPoint::text(node, byte)
                    } else if forward && at + 1 < eligible.len() {
                        SelectionPoint::text(eligible[at + 1], 0)
                    } else if !forward && at > 0 {
                        let node = eligible[at - 1];
                        SelectionPoint::text(
                            node,
                            self.nodes[node]
                                .widget
                                .as_ref()
                                .unwrap()
                                .text_content()
                                .unwrap()
                                .len(),
                        )
                    } else {
                        SelectionPoint::text(node, byte)
                    }
                }
            };
            let mut ancestor = Some(focus.node());
            while let Some(id) = ancestor {
                if self.used_select(id) == UserSelect::Contain && !self.within(old.anchor, id) {
                    focus = if forward {
                        self.after(id)
                    } else {
                        self.before(id)
                    };
                    break;
                }
                ancestor = self.nodes[id].parent;
            }
            self.normalize_user_selection(
                &state.index,
                old.anchor,
                Selection {
                    anchor: old.anchor,
                    focus,
                },
            )
        };
        let state = self.selection.get_mut();
        state.drag = None;
        state.set_user(result).unwrap_or(false)
    }
}

impl WidgetTree {
    fn extend_visual_selection(&mut self, movement: SelectionMove) -> bool {
        use voidui_gpui_wgpu::parley::{Affinity, Selection as Local};
        self.ensure_selection_index();
        let (range, local_cursor) = {
            let state = self.selection.borrow();
            let Some(old) = state.range else {
                return false;
            };
            let SelectionPoint::Text { node, byte } = old.focus else {
                drop(state);
                return self.extend_selection(
                    if matches!(
                        movement,
                        SelectionMove::Left | SelectionMove::WordLeft | SelectionMove::Up
                    ) {
                        SelectionMove::Backward
                    } else {
                        SelectionMove::Forward
                    },
                );
            };
            let Some(text) = self.nodes[node]
                .widget
                .as_ref()
                .and_then(|w| w.prepared_text())
            else {
                return false;
            };
            let paragraph = text.paragraph();
            let layout = paragraph.layout();
            let local = state
                .local_cursor
                .filter(|(id, c)| *id == node && paragraph.source_index(c.focus().index()) == byte)
                .map(|(_, c)| c.refresh(layout))
                .unwrap_or_else(|| {
                    Local::from_byte_index(
                        layout,
                        paragraph.layout_index(byte),
                        Affinity::Downstream,
                    )
                });
            let step = |s: Local| match movement {
                SelectionMove::Left => s.previous_visual(layout, false),
                SelectionMove::Right => s.next_visual(layout, false),
                SelectionMove::WordLeft => s.previous_visual_word(layout, false),
                SelectionMove::WordRight => s.next_visual_word(layout, false),
                SelectionMove::Up => s.previous_line(layout, false),
                SelectionMove::Down => s.next_line(layout, false),
                _ => unreachable!(),
            };
            let mut next = step(local);
            // Missing-font clusters can be smaller than a Unicode grapheme.
            // Continue the visual walk rather than losing affinity by snapping
            // backwards to the same invalid intermediate cluster indefinitely.
            while paragraph.snap_cursor(next.focus()).index() != next.focus().index() {
                let candidate = step(next);
                if candidate.focus() == next.focus() {
                    break;
                }
                next = candidate;
            }
            let mut focus =
                SelectionPoint::text(node, paragraph.source_index(next.focus().index()));
            let mut native = Some((node, next));
            if next.focus() == local.focus() {
                let forward = matches!(
                    movement,
                    SelectionMove::Right | SelectionMove::WordRight | SelectionMove::Down
                );
                let current = state.index.offset(self, old.focus).unwrap();
                let mut iter = state.index.texts.iter().copied().filter(|id| {
                    *id != node
                        && self.selection_visible(*id)
                        && self.used_select(*id) != UserSelect::None
                });
                let target = if forward {
                    iter.find(|id| state.index.spans[*id].text.unwrap() > current)
                } else {
                    iter.rfind(|id| state.index.spans[*id].text.unwrap() < current)
                };
                if let Some(id) = target {
                    let len = self.nodes[id]
                        .widget
                        .as_ref()
                        .unwrap()
                        .text_content()
                        .unwrap()
                        .len();
                    focus = SelectionPoint::text(id, if forward { 0 } else { len });
                    native = None;
                }
            }
            let range = self.normalize_user_selection(
                &state.index,
                old.anchor,
                Selection {
                    anchor: old.anchor,
                    focus,
                },
            );
            if range.focus != focus {
                native = None;
            }
            // Keep vertical movement's preferred column and the bidi caret side.
            (range, native)
        };
        let state = self.selection.get_mut();
        state.drag = None;
        if let Some(changed) = state.set_user(range) {
            state.local_cursor = local_cursor;
            changed
        } else {
            false
        }
    }
}
