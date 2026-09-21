//! Default commands and the custom-keymap context.
use super::*;

/// The current visual geometry and session are available to custom keymaps.
/// Deref exposes Editor's borrowing/update API; movement also handles bidi/wrap.
pub struct KeyContext<'a> {
    pub editor: &'a Editor,
    pub layout: &'a EditorLayout,
    pub bounds: Rect<f32>,
    pub align: render::TextAlign,
    pub scroll: Point<f32>,
}
impl std::ops::Deref for KeyContext<'_> {
    type Target = Editor;
    fn deref(&self) -> &Editor {
        self.editor
    }
}
impl KeyContext<'_> {
    pub fn move_selection(
        &self,
        motion: Motion,
        extend: bool,
    ) -> Result<bool, crate::editing::EditError> {
        self.editor.update(|s| {
            let selection = SelectionSet::new(
                s.selections().iter().map(|selection| {
                    self.layout.move_selection(
                        *selection,
                        motion,
                        extend,
                        self.bounds.size.width.max(self.layout.size().width),
                        self.align,
                        self.bounds.size.height,
                    )
                }),
                s.selections().primary_index(),
            )?;
            s.select(selection)
        })
    }
    /// Global logical coordinates for completion menus and other caret anchors.
    pub fn caret_bounds(&self) -> Option<Rect<f32>> {
        let selection = self.editor.with(|s| s.selections().primary());
        let caret = self.layout.caret(
            selection.head,
            selection.affinity,
            self.bounds.size.width.max(self.layout.size().width),
            self.align,
        )?;
        Some(view::viewport_caret(caret, self.bounds, self.scroll))
    }
}

impl TextEdit {
    fn move_carets(&self, motion: Motion, extend: bool, cx: &InputContext<'_>) {
        // Navigation reveals the caret after manual scrolling even when it is
        // already at the requested boundary and selection generation stays equal.
        self.view.borrow_mut().reveal_generation = None;
        self.editor().update(|state| {
            state.cancel_composition();
            let view = self.view.borrow();
            let width = cx.bounds.size.width.max(view.text.size().width);
            let selection = SelectionSet::new(
                state.selections().iter().map(|s| {
                    view.text.move_selection(
                        *s,
                        motion,
                        extend,
                        width,
                        cx.style.align.resolve(cx.style.direction),
                        cx.bounds.size.height,
                    )
                }),
                state.selections().primary_index(),
            )
            .unwrap();
            let _ = state.select(selection);
        });
    }
    pub(super) fn key(&mut self, key: &KeyInput, cx: &InputContext<'_>) -> bool {
        // IME owns navigation/confirmation until preedit commits or is cancelled.
        if self.editor().with(|s| s.composition().is_some()) {
            return false;
        }
        if let Some(handler) = &self.key_handler {
            let view = self.view.borrow();
            if handler(
                key,
                KeyContext {
                    editor: self.editor(),
                    layout: &view.text,
                    bounds: cx.bounds,
                    align: cx.style.align.resolve(cx.style.direction),
                    scroll: view.scroll,
                },
            ) {
                return true;
            }
        }
        let command = if cfg!(target_os = "macos") {
            key.modifiers.super_key()
        } else {
            key.modifiers.control_key()
        };
        let word = if cfg!(target_os = "macos") {
            key.modifiers.alt_key()
        } else {
            key.modifiers.control_key()
        };
        if command && !key.modifiers.alt_key() {
            if let Key::Character(ch) = &key.key {
                if ch.eq_ignore_ascii_case("a") {
                    self.editor().update(|s| s.select_all()).ok();
                    return true;
                }
                if ch.eq_ignore_ascii_case("z") {
                    if !cx.readonly {
                        self.editor().update(|s| {
                            if key.modifiers.shift_key() {
                                let _ = s.redo();
                            } else {
                                let _ = s.undo();
                            }
                        });
                    }
                    return true;
                }
                if ch.eq_ignore_ascii_case("y") && !cfg!(target_os = "macos") {
                    if !cx.readonly {
                        let _ = self.editor().update(|s| s.redo());
                    }
                    return true;
                }
            }
        }
        let Key::Named(named) = &key.key else {
            return false;
        };
        if *named == NamedKey::Tab {
            let position = self.editor().with(|s| s.selections().primary().head);
            if let Some(selection) = self
                .view
                .borrow()
                .text
                .next_cell(position, key.modifiers.shift_key())
            {
                self.editor()
                    .update(|s| s.select(SelectionSet::single(selection)))
                    .ok();
                return true;
            }
        }
        let motion = match named {
            NamedKey::ArrowLeft => Some(if command && cfg!(target_os = "macos") {
                Motion::LineStart
            } else if word {
                Motion::WordLeft
            } else {
                Motion::Left
            }),
            NamedKey::ArrowRight => Some(if command && cfg!(target_os = "macos") {
                Motion::LineEnd
            } else if word {
                Motion::WordRight
            } else {
                Motion::Right
            }),
            NamedKey::ArrowUp => Some(if command {
                Motion::DocumentStart
            } else {
                Motion::Up
            }),
            NamedKey::ArrowDown => Some(if command {
                Motion::DocumentEnd
            } else {
                Motion::Down
            }),
            NamedKey::Home => Some(if command {
                Motion::DocumentStart
            } else {
                Motion::LineStart
            }),
            NamedKey::End => Some(if command {
                Motion::DocumentEnd
            } else {
                Motion::LineEnd
            }),
            NamedKey::PageUp => Some(Motion::PageUp),
            NamedKey::PageDown => Some(Motion::PageDown),
            _ => None,
        };
        if let Some(motion) = motion {
            self.move_carets(motion, key.modifiers.shift_key(), cx);
            return true;
        }
        match named {
            NamedKey::Backspace | NamedKey::Delete => {
                if !cx.readonly {
                    let view = self.view.borrow();
                    self.editor()
                        .update(|s| {
                            let ranges = SelectionSet::new(
                                s.selections().iter().map(|selection| {
                                    let range = view.text.deletion_range(
                                        *selection,
                                        *named == NamedKey::Delete,
                                        word,
                                        cx.bounds.size.width.max(view.text.size().width),
                                        cx.style.align.resolve(cx.style.direction),
                                        cx.bounds.size.height,
                                    );
                                    Selection::range(range.start, range.end)
                                }),
                                s.selections().primary_index(),
                            )?;
                            let mut tx = crate::editing::Transaction::new(
                                s.revision(),
                                ranges
                                    .iter()
                                    .map(|s| crate::editing::Edit::new(s.text_range(), "")),
                            );
                            tx.kind = EditKind::Delete;
                            s.transact(tx)
                        })
                        .ok();
                }
                true
            }
            NamedKey::Enter => {
                if self.multiline && !command {
                    if !cx.readonly {
                        self.editor()
                            .update(|s| s.replace_selections("\n", EditKind::Typing))
                            .ok();
                    }
                } else if let Some(submit) = &self.on_submit {
                    submit(self.editor());
                }
                true
            }
            NamedKey::Tab
                if self.options.accept_tab && self.multiline && !key.modifiers.shift_key() =>
            {
                if !cx.readonly {
                    self.editor()
                        .update(|s| s.replace_selections("\t", EditKind::Typing))
                        .ok();
                }
                true
            }
            _ => false,
        }
    }
}
