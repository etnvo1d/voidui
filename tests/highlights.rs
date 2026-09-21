//! Automatic styling is derived presentation, never document content or history.
#![cfg(feature = "editing")]
use std::{
    borrow::Cow,
    time::{Duration, Instant},
};
use voidui::{InlineStyle, RichText, StyleSpan, editing::*, render};

fn marked(range: std::ops::Range<usize>) -> StyleSpan {
    StyleSpan::new(range, InlineStyle::new().color(render::white()))
}
fn typing(state: &mut EditorState, value: &str, now: Instant) {
    let end = state.text().len();
    let mut tx = Transaction::new(state.revision(), [Edit::new(end..end, value)]);
    tx.kind = EditKind::Typing;
    tx.selection = Some(SelectionSet::single(Selection::caret(end + value.len())));
    state.transact_at(tx, now).unwrap();
}
#[test]
fn highlight_updates_do_not_touch_document_history_or_typing_style() {
    let mut state = EditorState::new("let x");
    state
        .set_typing_style(Some(InlineStyle::new().italic()))
        .unwrap();
    let generation = state.generation();
    assert!(state.set_highlights(0, [marked(0..3)]).unwrap());
    assert_eq!(state.revision(), 0);
    assert!(state.generation() > generation);
    assert!(!state.can_undo());
    assert!(!state.can_redo());
    assert_eq!(state.history_bytes(), 0);
    assert!(state.document().spans().is_empty());
    assert!(state.document().rich_text().spans().is_empty());
    assert_eq!(state.display_spans().as_ref(), &[marked(0..3)]);
    assert_eq!(state.typing_style(), Some(&InlineStyle::new().italic()));
    let generation = state.generation();
    assert!(
        !state
            .set_highlights(0, [marked(0..1), marked(1..3)])
            .unwrap()
    );
    assert_eq!(state.generation(), generation);
    assert!(state.clear_highlights());
    assert!(!state.clear_highlights());
    assert!(matches!(state.display_spans(), Cow::Borrowed(_)));
}
#[test]
fn highlighting_between_keystrokes_does_not_split_typing_undo() {
    let mut state = EditorState::new("");
    let now = Instant::now();
    typing(&mut state, "a", now);
    let bytes = state.history_bytes();
    state
        .set_highlights(state.revision(), [marked(0..1)])
        .unwrap();
    assert_eq!(state.history_bytes(), bytes);
    typing(&mut state, "b", now + Duration::from_millis(1));
    assert!(state.highlights().is_empty());
    state
        .set_highlights(state.revision(), [marked(0..2)])
        .unwrap();
    assert_eq!(state.undo().unwrap().len(), 2);
    assert_eq!(state.text(), "");
    assert!(!state.can_undo());
    assert!(state.can_redo());
    assert!(state.highlights().is_empty());
    state.redo().unwrap();
    assert_eq!(state.text(), "ab");
    assert!(state.document().spans().is_empty());
}
#[test]
fn rehighlight_after_undo_preserves_redo_and_rejects_late_results() {
    let mut state = EditorState::new("x");
    typing(&mut state, "y", Instant::now());
    let stale = state.revision();
    state.undo().unwrap();
    let revision = state.revision();
    let bytes = state.history_bytes();
    state.set_highlights(revision, [marked(0..1)]).unwrap();
    let generation = state.generation();
    assert!(matches!(
        state.set_highlights(stale, [marked(0..2)]),
        Err(EditError::StaleRevision { .. })
    ));
    assert_eq!(state.generation(), generation);
    assert_eq!(state.history_bytes(), bytes);
    assert!(state.can_redo());
    state.redo().unwrap();
    assert_eq!(state.text(), "xy");
    assert!(state.highlights().is_empty());
}
#[test]
fn user_formatting_remains_undoable_under_the_highlight_layer() {
    let mut state = EditorState::from_rich(
        RichText::from_spans("abcd", [StyleSpan::new(0..4, InlineStyle::new().bold())]).unwrap(),
    );
    state.set_highlights(0, [marked(1..3)]).unwrap();
    state
        .transact(Transaction::new(0, []).format(0..4, StylePatch::new().italic(true)))
        .unwrap();
    assert_eq!(state.display_spans().len(), 3);
    assert_eq!(
        state.display_spans()[1].style,
        InlineStyle::new().bold().italic().color(render::white())
    );
    state.undo().unwrap();
    assert_eq!(
        state.display_spans()[1].style,
        InlineStyle::new().bold().color(render::white())
    );
    assert_eq!(state.highlights(), &[marked(1..3)]);
    state.redo().unwrap();
    state.clear_highlights();
    assert_eq!(
        state.document().spans(),
        &[StyleSpan::new(0..4, InlineStyle::new().bold().italic())]
    );
}
#[test]
fn invalid_highlights_are_atomic_and_do_not_run_edit_constraints() {
    let mut state = EditorState::new("中ab");
    let _guard = state.constrain(|_, _| Err(EditError::Rejected("read only")));
    state.set_highlights(0, [marked(0..3)]).unwrap();
    let generation = state.generation();
    for spans in [
        vec![marked(0..1)],
        vec![marked(0..4), marked(3..5)],
        vec![StyleSpan::new(
            0..3,
            InlineStyle {
                font_size: Some(f32::NAN),
                ..Default::default()
            },
        )],
    ] {
        assert!(state.set_highlights(0, spans).is_err());
        assert_eq!(state.generation(), generation);
        assert_eq!(state.highlights(), &[marked(0..3)]);
    }
    assert!(!state.can_undo());
}
#[test]
fn highlighting_during_ime_preserves_preedit_and_never_becomes_committed_style() {
    let mut state = EditorState::new("abc");
    state
        .select(SelectionSet::single(Selection::range(1, 2)))
        .unwrap();
    state.set_composition("中文", Some((6, 6))).unwrap();
    let composition = state.composition().cloned();
    state.set_highlights(0, [marked(0..3)]).unwrap();
    assert_eq!(state.composition(), composition.as_ref());
    assert_eq!(
        state.display_spans().as_ref(),
        &[marked(0..1), marked(7..8)]
    );
    assert_eq!(state.document().text(), "abc");
    state.commit_composition("中文").unwrap();
    assert_eq!(state.text(), "a中文c");
    assert!(state.highlights().is_empty());
    assert!(state.document().spans().is_empty());
    assert_eq!(state.undo().unwrap().len(), 1);
    assert_eq!(state.text(), "abc");
}
#[test]
fn rejected_text_edit_keeps_highlights_and_display_key() {
    let mut state = EditorState::new("abc");
    state.set_highlights(0, [marked(0..3)]).unwrap();
    let key = state.highlight_revision();
    let _guard = state.constrain(|_, _| Err(EditError::Rejected("read only")));
    assert!(state.replace_selections("x", EditKind::Typing).is_err());
    assert_eq!(state.highlight_revision(), key);
    assert_eq!(state.highlights(), &[marked(0..3)]);
}
#[test]
fn overlays_match_independent_per_character_style_resolution() {
    let text = "abcdefgh";
    for a in 0..text.len() {
        for b in a + 1..=text.len() {
            let base = StyleSpan::new(a..b, InlineStyle::new().bold());
            for c in 0..text.len() {
                for d in c + 1..=text.len() {
                    let mut state =
                        EditorState::from_rich(RichText::from_spans(text, [base.clone()]).unwrap());
                    state.set_highlights(0, [marked(c..d)]).unwrap();
                    let display =
                        RichText::from_spans(text, state.display_spans().into_owned()).unwrap();
                    for i in 0..text.len() {
                        let mut expected = InlineStyle::default();
                        if (a..b).contains(&i) {
                            expected = expected.bold();
                        }
                        if (c..d).contains(&i) {
                            expected = expected.color(render::white());
                        }
                        assert_eq!(
                            display
                                .span_at(i)
                                .map(|s| s.style.clone())
                                .unwrap_or_default(),
                            expected
                        );
                    }
                }
            }
        }
    }
}
