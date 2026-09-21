//! Byte-addressed storage and cheap immutable snapshots. Short controls keep a
//! String; larger documents use a shared rope. Rendering reads only its ranges.
use ropey::{LineType, Rope};
use std::{borrow::Cow, cell::OnceCell, ops::Range};

#[derive(Debug, Clone, Copy)]
pub struct BufferOptions {
    /// Promote to chunked storage above this byte count. Promotion is one-way so
    /// edits around the threshold cannot repeatedly copy the whole document.
    pub inline_bytes: usize,
}
impl Default for BufferOptions {
    fn default() -> Self {
        Self { inline_bytes: 4096 }
    }
}

/// Implemented by buffers, snapshots and borrowed strings. All indices are UTF-8
/// bytes. `read` copies only when a requested range crosses storage chunks.
pub trait TextRead {
    fn len(&self) -> usize;
    fn is_char_boundary(&self, position: usize) -> bool;
    fn read(&self, range: Range<usize>) -> Option<Cow<'_, str>>;
    /// A nonempty chunk containing `position`, or the last chunk at EOF.
    fn chunk(&self, position: usize) -> (&str, usize);
    fn equals(&self, range: Range<usize>, other: &str) -> bool {
        if range.len() != other.len()
            || !self.is_char_boundary(range.start)
            || !self.is_char_boundary(range.end)
        {
            return false;
        }
        let mut at = range.start;
        let mut read = 0;
        while at < range.end {
            let (chunk, start) = self.chunk(at);
            let n = (chunk.len() - (at - start)).min(range.end - at);
            if chunk.as_bytes()[at - start..at - start + n] != other.as_bytes()[read..read + n] {
                return false;
            }
            at += n;
            read += n;
        }
        true
    }
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
impl TextRead for str {
    fn len(&self) -> usize {
        str::len(self)
    }
    fn is_char_boundary(&self, p: usize) -> bool {
        str::is_char_boundary(self, p)
    }
    fn read(&self, r: Range<usize>) -> Option<Cow<'_, str>> {
        self.get(r).map(Cow::Borrowed)
    }
    fn chunk(&self, _: usize) -> (&str, usize) {
        (self, 0)
    }
}
#[derive(Debug, Clone)]
enum Storage {
    Inline(String),
    Rope(Rope),
}
#[derive(Debug)]
pub struct TextBuffer {
    storage: Storage,
    options: BufferOptions,
    flattened: OnceCell<Box<String>>,
}
#[derive(Debug, Clone)]
pub struct TextSnapshot {
    storage: Storage,
}
macro_rules! read_impl {
    ($t:ty) => {
        impl TextRead for $t {
            fn len(&self) -> usize {
                match &self.storage {
                    Storage::Inline(s) => s.len(),
                    Storage::Rope(r) => r.len(),
                }
            }
            fn is_char_boundary(&self, p: usize) -> bool {
                p <= self.len()
                    && match &self.storage {
                        Storage::Inline(s) => s.is_char_boundary(p),
                        Storage::Rope(r) => r.is_char_boundary(p),
                    }
            }
            fn read(&self, range: Range<usize>) -> Option<Cow<'_, str>> {
                if range.start > range.end
                    || !self.is_char_boundary(range.start)
                    || !self.is_char_boundary(range.end)
                {
                    return None;
                }
                match &self.storage {
                    Storage::Inline(s) => Some(Cow::Borrowed(&s[range])),
                    Storage::Rope(r) => {
                        let slice = r.slice(range);
                        Some(match slice.as_str() {
                            Some(s) => Cow::Borrowed(s),
                            None => Cow::Owned(slice.to_string()),
                        })
                    }
                }
            }
            fn chunk(&self, p: usize) -> (&str, usize) {
                match &self.storage {
                    Storage::Inline(s) => (s, 0),
                    Storage::Rope(r) => r.chunk(p.min(r.len())),
                }
            }
        }
    };
}
read_impl!(TextBuffer);
read_impl!(TextSnapshot);
impl Default for TextBuffer {
    fn default() -> Self {
        Self::new(String::new(), BufferOptions::default())
    }
}
impl Clone for TextBuffer {
    fn clone(&self) -> Self {
        Self {
            storage: self.storage.clone(),
            options: self.options,
            flattened: OnceCell::new(),
        }
    }
}
impl TextBuffer {
    pub fn new(text: String, options: BufferOptions) -> Self {
        let storage = if text.len() > options.inline_bytes {
            Storage::Rope(Rope::from_str(&text))
        } else {
            Storage::Inline(text)
        };
        Self {
            storage,
            options,
            flattened: OnceCell::new(),
        }
    }
    pub fn snapshot(&self) -> TextSnapshot {
        TextSnapshot {
            storage: self.storage.clone(),
        }
    }
    /// Explicit compatibility materialization. Large-document code should use
    /// TextRead; this cache is discarded on the next text edit.
    pub fn as_str(&self) -> &str {
        match &self.storage {
            Storage::Inline(s) => s,
            Storage::Rope(r) => self.flattened.get_or_init(|| Box::new(r.to_string())),
        }
    }
    pub fn is_materialized(&self) -> bool {
        self.flattened.get().is_some()
    }
    pub fn is_chunked(&self) -> bool {
        matches!(self.storage, Storage::Rope(_))
    }
    pub fn capacity(&self) -> usize {
        match &self.storage {
            Storage::Inline(s) => s.capacity(),
            Storage::Rope(r) => r.len(),
        }
    }
    pub(crate) fn replace(&mut self, range: Range<usize>, insert: &str) {
        self.flattened.take();
        if let Storage::Inline(s) = &self.storage {
            if s.len() - range.len() + insert.len() > self.options.inline_bytes {
                self.storage = Storage::Rope(Rope::from_str(s));
            }
        }
        match &mut self.storage {
            Storage::Inline(s) => s.replace_range(range, insert),
            Storage::Rope(r) => {
                r.remove(range.clone());
                r.insert(range.start, insert);
            }
        }
    }
}
impl Default for TextSnapshot {
    fn default() -> Self {
        Self::from_text("")
    }
}
impl TextSnapshot {
    pub(crate) fn replace(&mut self, range: Range<usize>, insert: &str) {
        match &mut self.storage {
            Storage::Inline(s) => s.replace_range(range, insert),
            Storage::Rope(r) => {
                r.remove(range.clone());
                r.insert(range.start, insert);
            }
        }
    }

    pub fn line_ranges(&self) -> Box<dyn Iterator<Item = Range<usize>> + '_> {
        let lengths: Box<dyn Iterator<Item = usize> + '_> = match &self.storage {
            Storage::Inline(s) => Box::new(hard_lines(s).map(str::len)),
            Storage::Rope(r) => Box::new(r.lines(LineType::Unicode).map(|s| s.len())),
        };
        Box::new(lengths.scan(0, |start, len| {
            let range = *start..*start + len;
            *start += len;
            Some(range)
        }))
    }

    pub fn equal_range(
        &self,
        range: Range<usize>,
        other: &Self,
        other_range: Range<usize>,
    ) -> bool {
        if range.len() != other_range.len() {
            return false;
        }
        let (mut a, mut b) = (range.start, other_range.start);
        while a < range.end {
            let (x, ax) = self.chunk(a);
            let (y, by) = other.chunk(b);
            let n = (x.len() - (a - ax))
                .min(y.len() - (b - by))
                .min(range.end - a);
            if x.as_bytes()[a - ax..a - ax + n] != y.as_bytes()[b - by..b - by + n] {
                return false;
            }
            a += n;
            b += n;
        }
        true
    }
    pub fn from_text(text: impl Into<String>) -> Self {
        TextBuffer::new(text.into(), BufferOptions::default()).snapshot()
    }
    pub fn line_count(&self) -> usize {
        match &self.storage {
            Storage::Rope(r) => r.len_lines(LineType::Unicode),
            Storage::Inline(s) => hard_lines(s).count(),
        }
    }
    pub fn line_range(&self, index: usize) -> Option<Range<usize>> {
        match &self.storage {
            Storage::Rope(r) => {
                if index >= r.len_lines(LineType::Unicode) {
                    return None;
                }
                let a = r.line_to_byte_idx(index, LineType::Unicode);
                Some(a..a + r.line(index, LineType::Unicode).len())
            }
            Storage::Inline(s) => {
                let mut start = 0;
                for (i, line) in hard_lines(s).enumerate() {
                    if i == index {
                        return Some(start..start + line.len());
                    }
                    start += line.len();
                }
                None
            }
        }
    }
    pub fn line_at(&self, byte: usize) -> usize {
        match &self.storage {
            Storage::Rope(r) => r.byte_to_line_idx(byte.min(r.len()), LineType::Unicode),
            Storage::Inline(_) => self
                .line_ranges()
                .take_while(|r| r.start <= byte)
                .count()
                .saturating_sub(1),
        }
    }
}
/// Include terminators and the trailing empty paragraph. Shared by storage,
/// projection and layout so CRLF and Unicode separators use one byte policy.
pub(crate) fn hard_lines(mut text: &str) -> impl Iterator<Item = &str> {
    let mut done = false;
    std::iter::from_fn(move || {
        if done {
            return None;
        }
        if let Some((i, ch)) = text.char_indices().find(|(_, c)| is_separator(*c)) {
            let n = if text[i..].starts_with("\r\n") {
                2
            } else {
                ch.len_utf8()
            };
            let line = &text[..i + n];
            text = &text[i + n..];
            Some(line)
        } else {
            done = true;
            Some(text)
        }
    })
}
pub(crate) fn is_separator(c: char) -> bool {
    matches!(
        c,
        '\r' | '\n' | '\u{b}' | '\u{c}' | '\u{85}' | '\u{2028}' | '\u{2029}'
    )
}
pub(crate) fn separator_len(text: &str) -> usize {
    if text.ends_with("\r\n") {
        2
    } else {
        text.chars()
            .next_back()
            .filter(|c| is_separator(*c))
            .map_or(0, char::len_utf8)
    }
}
