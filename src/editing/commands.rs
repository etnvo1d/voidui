//! Logical editing commands can be reused by alternative keymaps and custom views.
use super::TextRead;
use super::{Change, Edit, EditError, EditKind, EditorState, Selection, SelectionSet, Transaction};
use unicode_segmentation::{GraphemeCursor, GraphemeIncomplete, UnicodeSegmentation};

pub fn grapheme_boundary(text: &str, position: usize, forward: bool) -> usize {
    let mut cursor = GraphemeCursor::new(position, text.len(), true);
    let boundary = if forward {
        cursor.next_boundary(text, 0)
    } else {
        cursor.prev_boundary(text, 0)
    };
    boundary
        .expect("complete UTF-8 text supplies all grapheme context")
        .unwrap_or(if forward { text.len() } else { 0 })
}
pub fn word_boundary(text: &str, position: usize, forward: bool) -> usize {
    if forward {
        let mut boundary = text.len();
        for (at, word) in text[position..].split_word_bound_indices() {
            boundary = position + at + word.len();
            if !word.chars().all(char::is_whitespace) {
                break;
            }
        }
        boundary
    } else {
        let mut boundary = 0;
        for (at, word) in text[..position].split_word_bound_indices().rev() {
            boundary = at;
            if !word.chars().all(char::is_whitespace) {
                break;
            }
        }
        boundary
    }
}
/// Navigate graphemes directly across storage chunks, supplying UAX #29 context
/// on demand. No whole-document String is required for backspace or delete.
pub fn source_grapheme_boundary(
    source: &(impl TextRead + ?Sized),
    position: usize,
    forward: bool,
) -> usize {
    let mut cursor = GraphemeCursor::new(position, source.len(), true);
    let (mut chunk, mut start) = source.chunk(if !forward && position > 0 {
        position - 1
    } else {
        position
    });
    loop {
        let result = if forward {
            cursor.next_boundary(chunk, start)
        } else {
            cursor.prev_boundary(chunk, start)
        };
        match result {
            Ok(boundary) => return boundary.unwrap_or(if forward { source.len() } else { 0 }),
            Err(GraphemeIncomplete::NextChunk) => {
                (chunk, start) = source.chunk(start + chunk.len());
            }
            Err(GraphemeIncomplete::PrevChunk) => {
                (chunk, start) = source.chunk(start.saturating_sub(1));
            }
            Err(GraphemeIncomplete::PreContext(end)) => {
                let (context, at) = source.chunk(end.saturating_sub(1));
                cursor.provide_context(&context[..end - at], at);
            }
            Err(GraphemeIncomplete::InvalidOffset) => panic!("invalid source grapheme position"),
        }
    }
}
fn source_word_boundary(
    source: &(impl TextRead + ?Sized),
    position: usize,
    forward: bool,
) -> usize {
    let (chunk, start) = source.chunk(position);
    let mut a = start;
    let mut b = start + chunk.len();
    loop {
        let text = source.read(a..b).unwrap();
        let edge = a + word_boundary(&text, position - a, forward);
        if forward && edge == b && b < source.len() {
            let (c, at) = source.chunk(b);
            b = at + c.len();
        } else if !forward && edge == a && a > 0 {
            a = source.chunk(a - 1).1;
        } else {
            return edge;
        }
    }
}
impl EditorState {
    pub fn delete(&mut self, forward: bool, word: bool) -> Result<Option<Change>, EditError> {
        let ranges = self.selections().iter().map(|s| {
            if !s.is_caret() {
                return *s;
            }
            let edge = if word {
                source_word_boundary(self.document(), s.head, forward)
            } else {
                source_grapheme_boundary(self.document(), s.head, forward)
            };
            Selection::range(s.head, edge)
        });
        let ranges = SelectionSet::new(ranges, self.selections().primary_index())?;
        let edits: Vec<_> = ranges
            .iter()
            .map(|s| Edit::new(s.text_range(), ""))
            .collect();
        let mut transaction = Transaction::new(self.revision(), edits);
        transaction.kind = EditKind::Delete;
        self.transact(transaction)
    }
}
