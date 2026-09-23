//! Cell metrics outlive glyph eviction. Width-independent shaping is retained
//! only for a bounded working set; absolute document offsets never enter keys.
use super::*;
use crate::{
    cache::BudgetCache,
    render::{InlineTextBox, Paragraph},
};
#[derive(Clone, PartialEq)]
pub(super) struct CellKey {
    pub text: String,
    pub spans: Vec<StyleSpan>,
    pub options: LayoutOptions,
    pub boxes: Vec<InlineTextBox>,
    pub revision: u64,
}
impl Eq for CellKey {}
impl std::hash::Hash for CellKey {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        // Equality checks all typography and inline geometry after the hash;
        // collisions are safe and no float is converted to a lossy string key.
        std::hash::Hash::hash(&self.text, state);
        std::hash::Hash::hash(&self.revision, state);
    }
}
impl CellKey {
    pub fn cost(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.text.len()
            + self.spans.len() * std::mem::size_of::<StyleSpan>()
            + self.boxes.len() * std::mem::size_of::<InlineTextBox>()
    }
}
pub(super) struct CellCache {
    pub metrics: BudgetCache<(CellKey, u32), Size<f32>>,
    pub shapes: BudgetCache<CellKey, Paragraph>,
}
impl Default for CellCache {
    fn default() -> Self {
        Self {
            metrics: BudgetCache::new(ViewportOptions::default().cell_metrics),
            shapes: BudgetCache::new(ViewportOptions::default().cell_shapes),
        }
    }
}
