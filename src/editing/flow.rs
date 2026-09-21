//! One native text flow. Document partitioning, source mapping, object hosting and
//! virtualization live above this layer; glyphs and row geometry live in Parley.
use super::{Bias, LayoutOptions, Motion, Selection};
use crate::{
    core::geometry::{Point, Rect, Size},
    render::{self, Hsla, Painter, Paragraph, TextAlign, TextSystem, parley},
};
use std::{ops::Range, sync::Arc};
pub(crate) struct TextFlow {
    paragraph: Paragraph,
    options: LayoutOptions,
}
impl TextFlow {
    pub fn from_paragraph(
        mut paragraph: Paragraph,
        options: LayoutOptions,
        _system: Arc<TextSystem>,
    ) -> Self {
        paragraph.reflow(options.width);
        Self { paragraph, options }
    }
    pub fn paragraph(&self) -> &Paragraph {
        &self.paragraph
    }
    pub fn size(&self) -> Size<f32> {
        Size::new(
            self.paragraph.width(),
            if self.paragraph.source().is_empty() {
                self.options.line_height
            } else {
                self.paragraph.height()
            },
        )
    }
    pub fn reflow(&mut self, width: Option<f32>) {
        self.paragraph.reflow(width);
        self.options.width = width;
    }
    pub fn caret(
        &self,
        byte: usize,
        bias: Bias,
        width: f32,
        align: TextAlign,
    ) -> Option<Rect<f32>> {
        if byte > self.paragraph.source().len() {
            return None;
        }
        self.paragraph
            .caret_bounds(byte, affinity(bias), width, align)
            .map(|r| Rect::from_xywh(r.origin.x, r.origin.y, r.size.width, r.size.height))
            .or_else(|| {
                self.paragraph.source().is_empty().then(|| {
                    Rect::from_xywh(
                        match align {
                            TextAlign::Left => 0.0,
                            TextAlign::Center => width * 0.5,
                            TextAlign::Right => width,
                        },
                        0.0,
                        1.0,
                        self.options.line_height,
                    )
                })
            })
    }
    pub fn hit_test(&self, p: Point<f32>, width: f32, align: TextAlign) -> Selection {
        let hit = self
            .paragraph
            .source_cursor_at(render::point(p.x, p.y), width, align);
        let mut selection = Selection::caret(hit.map_or(0, |(byte, _)| byte));
        selection.affinity = hit.map_or(Bias::After, |(_, affinity)| bias(affinity));
        selection
    }
    pub fn word_at(&self, p: Point<f32>, width: f32, align: TextAlign) -> Range<usize> {
        self.paragraph
            .selection_unit(render::point(p.x, p.y), width, align, true)
            .unwrap_or(0..0)
    }
    pub fn selection_rectangles(
        &self,
        range: Range<usize>,
        width: f32,
        align: TextAlign,
    ) -> Vec<Rect<f32>> {
        self.paragraph
            .selection_rectangles(range, width, align)
            .into_iter()
            .map(|(_, r)| Rect::from_xywh(r.origin.x, r.origin.y, r.size.width, r.size.height))
            .collect()
    }
    pub fn move_selection(
        &self,
        selection: Selection,
        motion: Motion,
        extend: bool,
        width: f32,
        align: TextAlign,
        viewport_height: f32,
    ) -> Selection {
        let p = &self.paragraph;
        if !extend && !selection.is_caret() && matches!(motion, Motion::Left | Motion::Right) {
            return Selection::caret(if motion == Motion::Left {
                selection.text_range().start
            } else {
                selection.text_range().end
            });
        }
        let cursor = parley::Cursor::from_byte_index(
            p.layout(),
            p.layout_index(selection.head.min(p.source().len())),
            affinity(selection.affinity),
        );
        let mut next = match motion {
            Motion::DocumentStart => Selection::caret(0),
            Motion::DocumentEnd => Selection::caret(p.source().len()),
            Motion::Up | Motion::Down | Motion::PageUp | Motion::PageDown => {
                let caret = self
                    .caret(selection.head, selection.affinity, width, align)
                    .unwrap_or(Rect::from_xywh(0.0, 0.0, 1.0, self.options.line_height));
                let x = selection.preferred_x.unwrap_or(caret.origin.x);
                let down = matches!(motion, Motion::Down | Motion::PageDown);
                let row = p.cursor_row(cursor);
                let target = if down {
                    row.checked_add(1)
                } else {
                    row.checked_sub(1)
                };
                let mut s = if matches!(motion, Motion::PageUp | Motion::PageDown) {
                    self.hit_test(
                        Point::new(
                            x,
                            caret.origin.y
                                + caret.size.height * 0.5
                                + if down {
                                    viewport_height.max(caret.size.height)
                                } else {
                                    -viewport_height.max(caret.size.height)
                                },
                        ),
                        width,
                        align,
                    )
                } else if let Some(bounds) = target.and_then(|r| p.row_bounds(r, width, align)) {
                    self.hit_test(
                        Point::new(x, bounds.origin.y + bounds.size.height * 0.5),
                        width,
                        align,
                    )
                } else {
                    selection
                };
                s.preferred_x = Some(x);
                s
            }
            Motion::LineStart | Motion::LineEnd => {
                let (byte, affinity) = p
                    .source_row_edge(p.cursor_row(cursor), motion == Motion::LineEnd)
                    .unwrap_or((0, parley::Affinity::Downstream));
                let mut next = Selection::caret(byte);
                next.affinity = bias(affinity);
                next
            }
            _ => {
                let local = parley::Selection::from(cursor);
                let next = match motion {
                    Motion::Left => local.previous_visual(p.layout(), false),
                    Motion::Right => local.next_visual(p.layout(), false),
                    Motion::WordLeft => local.previous_visual_word(p.layout(), false),
                    Motion::WordRight => local.next_visual_word(p.layout(), false),
                    _ => local,
                };
                let cursor = p.snap_cursor(next.focus());
                let mut s = Selection::caret(p.source_index(cursor.index()));
                s.affinity = bias(cursor.affinity());
                s
            }
        };
        if extend {
            next.anchor = selection.anchor;
        }
        next
    }
    pub fn paint(
        &self,
        painter: &mut Painter<'_>,
        origin: Point<f32>,
        clip: Rect<f32>,
        width: f32,
        align: TextAlign,
        color: Hsla,
        selections: &[Range<usize>],
        selection_color: Hsla,
        selection_background: Hsla,
    ) -> render::Result<()> {
        let bounds = render::Bounds::new(
            render::point(origin.x, origin.y),
            render::size(width, self.size().height),
        );
        let clip = render::Bounds::new(
            render::point(render::px(clip.origin.x), render::px(clip.origin.y)),
            render::size(render::px(clip.size.width), render::px(clip.size.height)),
        );
        painter.with_clip(clip, |painter| {
            self.paragraph.paint_selections(
                painter,
                bounds,
                align,
                Some(color),
                Some((selections, selection_color, selection_background)),
            )
        })
    }
}
fn affinity(b: Bias) -> parley::Affinity {
    if b == Bias::Before {
        parley::Affinity::Upstream
    } else {
        parley::Affinity::Downstream
    }
}
fn bias(a: parley::Affinity) -> Bias {
    if a == parley::Affinity::Upstream {
        Bias::Before
    } else {
        Bias::After
    }
}
