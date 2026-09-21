//! Source-based viewport anchors captured before any height-index replacement.
use super::*;

#[derive(Clone, Copy)]
pub(super) enum ScrollAnchor {
    Block { position: usize, screen_y: f32 },
    Bottom,
}
impl ScrollAnchor {
    pub(super) fn map(self, changes: Option<&crate::editing::ChangeSet>) -> Self {
        match (self, changes) {
            (Self::Block { position, screen_y }, Some(changes)) => Self::Block {
                position: changes.map(position, Bias::Before),
                screen_y,
            },
            _ => self,
        }
    }
    pub(super) fn resolve(self, engine: &Engine, viewport: Rect<f32>) -> f32 {
        match self {
            Self::Block { position, screen_y } => {
                let index = engine
                    .block_at(position.min(engine.source.len()))
                    .unwrap_or(0);
                (engine.heights.top(index) - screen_y).max(0.0)
            }
            Self::Bottom => (engine.heights.total() - viewport.size.height).max(0.0),
        }
    }
}
impl Engine {
    pub(super) fn capture_anchor(&self, viewport: Rect<f32>) -> ScrollAnchor {
        // A genuine bottom viewport follows appended content and viewport size
        // changes. An initial, unscrolled viewport always stays at the top.
        if viewport.origin.y > 0.0
            && (viewport.origin.y + viewport.size.height - self.heights.total()).abs() < 0.1
        {
            ScrollAnchor::Bottom
        } else {
            let index = self.heights.at(viewport.origin.y);
            ScrollAnchor::Block {
                position: self.blocks[index].range.start,
                screen_y: self.heights.top(index) - viewport.origin.y,
            }
        }
    }
    pub(super) fn anchor_for(&self, viewport: Rect<f32>) -> ScrollAnchor {
        // Measurements requested by caret/IME queries can happen between frames.
        // Preserve the last displayed anchor unless the caller actually scrolled.
        self.scroll_anchor
            .filter(|_| {
                self.viewport
                    .is_some_and(|old| old.origin.y == viewport.origin.y)
            })
            .unwrap_or_else(|| self.capture_anchor(viewport))
    }
}
