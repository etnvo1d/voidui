//! Caret ink is independent of insertion/IME geometry. Wider shapes use the
//! selected grapheme's layout rectangle, including bidi and wrapped text.
use super::*;
use crate::{editing::source_grapheme_boundary, style::text::CaretShape};

pub(super) fn visible_for(selection: &Selection, shape: CaretShape) -> bool {
    selection.is_caret() || shape != CaretShape::Bar
}

pub(super) fn geometry(
    state: &EditorState,
    layout: &EditorLayout,
    selection: Selection,
    shape: CaretShape,
    width: f32,
    align: render::TextAlign,
    fallback_width: f32,
) -> Option<Rect<f32>> {
    let mut head = selection.head;
    // Forward selections are half-open. The visible endpoint of a block
    // selection is its last included grapheme, not the character after it.
    if shape != CaretShape::Bar && selection.head > selection.anchor {
        head = source_grapheme_boundary(state.document(), head, false);
    }
    let mut bounds = layout.caret(head, selection.affinity, width, align)?;
    let thickness = bounds.size.width.min(bounds.size.height);
    if shape == CaretShape::Bar {
        return Some(bounds);
    }
    bounds.size.width = fallback_width.max(bounds.size.width);
    let end = source_grapheme_boundary(state.document(), head, true);
    let source = state.document().read(head..end)?;
    if !source.is_empty() && !source.contains(['\n', '\r']) {
        // Query only this grapheme, never the entire document or visual line.
        if let Some(cell) = layout
            .selection_rectangles(head..end, width, align)
            .into_iter()
            .find(|r| r.size.width > 0.0 && (r.origin.y - bounds.origin.y).abs() < 0.5)
        {
            bounds.origin.x = cell.origin.x;
            bounds.size.width = cell.size.width;
        }
    }
    if shape == CaretShape::Underline {
        // Match the insertion bar's thickness and keep short lines visible.
        bounds.origin.y += bounds.size.height - thickness;
        bounds.size.height = thickness;
    }
    Some(bounds)
}
