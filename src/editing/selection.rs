//! Logical editor selections are independent of both the widget tree and layout.
use super::{Bias, ChangeSet, EditError};
use std::ops::Range;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Selection {
    pub anchor: usize,
    pub head: usize,
    pub affinity: Bias,
    /// Desired horizontal position for repeated vertical movement, in logical pixels.
    pub preferred_x: Option<f32>,
}
impl Selection {
    pub fn caret(position: usize) -> Self {
        Self::range(position, position)
    }
    pub fn range(anchor: usize, head: usize) -> Self {
        Self {
            anchor,
            head,
            affinity: Bias::After,
            preferred_x: None,
        }
    }
    pub fn text_range(self) -> Range<usize> {
        self.anchor.min(self.head)..self.anchor.max(self.head)
    }
    pub fn is_caret(self) -> bool {
        self.anchor == self.head
    }
    pub fn map(self, changes: &ChangeSet) -> Self {
        Self {
            anchor: changes.map(self.anchor, Bias::After),
            head: changes.map(self.head, Bias::After),
            preferred_x: None,
            ..self
        }
    }
}

/// The common single-caret case has no heap allocation. Extra ranges are sorted
/// and overlapping ranges merge; one range remains the primary IME/scroll target.
#[derive(Debug, Clone, PartialEq)]
pub struct SelectionSet {
    first: Selection,
    rest: Vec<Selection>,
    primary: usize,
}
impl Default for SelectionSet {
    fn default() -> Self {
        Self::single(Selection::caret(0))
    }
}
impl SelectionSet {
    // Vec allocations cannot reach isize::MAX elements, so a valid primary
    // index never uses this bit. Tagging the existing index keeps ordinary
    // editor sessions allocation-free without adding padding to every snapshot.
    const STRUCTURED: usize = 1 << (usize::BITS - 1);

    pub fn single(selection: Selection) -> Self {
        Self {
            first: selection,
            rest: Vec::new(),
            primary: 0,
        }
    }
    pub fn new(
        ranges: impl IntoIterator<Item = Selection>,
        primary: usize,
    ) -> Result<Self, EditError> {
        let mut input = ranges.into_iter();
        let first = input.next().ok_or(EditError::InvalidSelection)?;
        let Some(second) = input.next() else {
            return if primary == 0 {
                Ok(Self::single(first))
            } else {
                Err(EditError::InvalidSelection)
            };
        };
        let mut ranges: Vec<_> = [first, second]
            .into_iter()
            .chain(input)
            .enumerate()
            .map(|(i, s)| (s, i == primary))
            .collect();
        if ranges.is_empty() || primary >= ranges.len() {
            return Err(EditError::InvalidSelection);
        }
        ranges.sort_by_key(|(s, _)| (s.text_range().start, s.text_range().end));
        let mut merged: Vec<(Selection, bool)> = Vec::with_capacity(ranges.len());
        for (selection, primary) in ranges {
            if let Some((last, was_primary)) = merged.last_mut() {
                let a = last.text_range();
                let b = selection.text_range();
                if b.start < a.end
                    || (b.start == a.start && b.end == a.end)
                    || (b.start == a.end && (last.is_caret() || selection.is_caret()))
                {
                    let backwards = if primary {
                        selection.head < selection.anchor
                    } else {
                        last.head < last.anchor
                    };
                    let end = a.end.max(b.end);
                    *last = if backwards {
                        Selection::range(end, a.start)
                    } else {
                        Selection::range(a.start, end)
                    };
                    *was_primary |= primary;
                    continue;
                }
            }
            merged.push((selection, primary));
        }
        let primary = merged.iter().position(|(_, p)| *p).unwrap_or(0);
        let first = merged.remove(0).0;
        Ok(Self {
            first,
            rest: merged.into_iter().map(|(s, _)| s).collect(),
            primary,
        })
    }
    pub fn iter(&self) -> impl Iterator<Item = &Selection> + DoubleEndedIterator {
        std::iter::once(&self.first).chain(self.rest.iter())
    }
    pub fn len(&self) -> usize {
        1 + self.rest.len()
    }
    pub fn is_empty(&self) -> bool {
        false
    }
    pub fn primary_index(&self) -> usize {
        self.primary & !Self::STRUCTURED
    }
    pub fn primary(&self) -> Selection {
        let primary = self.primary_index();
        if primary == 0 {
            self.first
        } else {
            self.rest[primary - 1]
        }
    }
    /// A structured selection keeps a source position for navigation and undo,
    /// but does not expose source syntax or draw a text caret. Its view owns the
    /// selected objects/cells and supplies clipboard text independently.
    pub fn structured(selection: Selection) -> Self {
        Self {
            primary: Self::STRUCTURED,
            ..Self::single(selection)
        }
    }
    pub fn is_structured(&self) -> bool {
        self.primary & Self::STRUCTURED != 0
    }
    pub fn map(&self, changes: &ChangeSet) -> Self {
        let mut next = if self.rest.is_empty() {
            Self::single(self.first.map(changes))
        } else {
            Self::new(self.iter().map(|s| s.map(changes)), self.primary_index()).unwrap()
        };
        if self.is_structured() {
            next.primary |= Self::STRUCTURED;
        }
        next
    }
    pub(crate) fn retained_bytes(&self) -> usize {
        self.rest.capacity() * std::mem::size_of::<Selection>()
    }
    pub(crate) fn valid(&self, text: &(impl super::TextRead + ?Sized)) -> bool {
        self.iter().all(|s| {
            text.is_char_boundary(s.anchor)
                && text.is_char_boundary(s.head)
                && s.preferred_x.is_none_or(f32::is_finite)
        })
    }
}
