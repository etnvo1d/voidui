//! Deterministic editing tests need no font service, OS clipboard, or window.
#![cfg(feature = "editing")]
use std::time::{Duration, Instant};
use voidui::editing::*;

#[test]
fn atomic_edits_use_one_revision_and_preserve_utf8() {
    let mut editor = EditorState::new("a中文z");
    let original = editor.text().to_owned();
    let bad = Transaction::new(0, [Edit::new(1..2, "x"), Edit::new(7..8, "y")]);
    assert!(matches!(
        editor.transact(bad),
        Err(EditError::InvalidRange(_))
    ));
    assert_eq!(editor.text(), original);
    assert_eq!(editor.revision(), 0);
    editor
        .transact(Transaction::new(
            0,
            [Edit::new(7..8, "!"), Edit::new(1..7, "ok")],
        ))
        .unwrap();
    assert_eq!(editor.text(), "aok!");
    assert!(matches!(
        editor.transact(Transaction::new(0, [])),
        Err(EditError::StaleRevision { .. })
    ));
    editor.undo().unwrap();
    assert_eq!(editor.text(), original);
    editor.redo().unwrap();
    assert_eq!(editor.text(), "aok!");
}
#[test]
fn failed_output_selection_does_not_mutate_or_create_history() {
    let mut editor = EditorState::new("abc");
    let mut tx = Transaction::new(0, [Edit::new(0..3, "中")]);
    tx.selection = Some(SelectionSet::single(Selection::caret(1)));
    assert_eq!(
        editor.transact(tx).unwrap_err(),
        EditError::InvalidSelection
    );
    assert_eq!(editor.text(), "abc");
    assert!(!editor.can_undo());
}
#[test]
fn two_cursors_insert_delete_and_undo_as_one_operation() {
    let mut editor = EditorState::new("alpha beta");
    editor
        .select(SelectionSet::new([Selection::caret(0), Selection::caret(6)], 1).unwrap())
        .unwrap();
    let change = editor
        .replace_selections("中", EditKind::Typing)
        .unwrap()
        .unwrap();
    assert_eq!(change.changes.edits().len(), 2);
    assert_eq!(editor.text(), "中alpha 中beta");
    assert_eq!(editor.selections().primary().head, 12);
    editor.undo().unwrap();
    assert_eq!(editor.text(), "alpha beta");
    assert_eq!(editor.selections().len(), 2);
    editor.redo().unwrap();
    editor.delete(false, false).unwrap();
    assert_eq!(editor.text(), "alpha beta");
}
#[test]
fn overlapping_selections_merge_and_primary_survives() {
    let selections = SelectionSet::new(
        [
            Selection::range(8, 3),
            Selection::range(5, 10),
            Selection::caret(3),
        ],
        0,
    )
    .unwrap();
    assert_eq!(selections.len(), 1);
    assert_eq!(selections.primary().text_range(), 3..10);
}
#[test]
fn composition_is_transient_cancellable_and_one_undo_step() {
    let mut editor = EditorState::new("replace me");
    editor
        .select(SelectionSet::single(Selection::range(0, 7)))
        .unwrap();
    editor.set_composition("ni", Some((2, 2))).unwrap();
    editor.set_composition("你", Some((3, 3))).unwrap();
    assert_eq!(editor.text(), "replace me");
    assert!(!editor.can_undo());
    assert_eq!(
        editor.transact(Transaction::new(0, [])).unwrap_err(),
        EditError::CompositionActive
    );
    editor.cancel_composition();
    assert_eq!(editor.selected_text(), "replace");
    editor.set_composition("ni", Some((2, 2))).unwrap();
    editor.commit_composition("你").unwrap();
    assert_eq!(editor.text(), "你 me");
    editor.undo().unwrap();
    assert_eq!(editor.text(), "replace me");
    assert_eq!(editor.selected_text(), "replace");
}
#[test]
fn invalid_preedit_range_keeps_previous_composition() {
    let mut editor = EditorState::new("");
    editor.set_composition("n", Some((1, 1))).unwrap();
    assert!(editor.set_composition("你", Some((1, 1))).is_err());
    assert_eq!(editor.composition().unwrap().text, "n");
}
#[test]
fn deletion_respects_emoji_combining_and_crlf_graphemes() {
    for cluster in ["👩‍👩‍👧‍👦", "e\u{301}", "🇨🇳", "\r\n"] {
        let mut editor = EditorState::new(format!("a{cluster}z"));
        editor
            .select(SelectionSet::single(Selection::caret(1 + cluster.len())))
            .unwrap();
        editor.delete(false, false).unwrap();
        assert_eq!(editor.text(), "az", "{cluster:?}");
        editor.undo().unwrap();
        assert_eq!(editor.text(), format!("a{cluster}z"));
    }
}
#[test]
fn adjacent_typing_groups_but_selection_and_time_break_groups() {
    let mut editor = EditorState::new("");
    let now = Instant::now();
    for (i, text) in ["a", "b", "c"].into_iter().enumerate() {
        let mut tx = Transaction::new(editor.revision(), [Edit::new(i..i, text)]);
        tx.kind = EditKind::Typing;
        editor
            .transact_at(tx, now + Duration::from_millis(i as u64))
            .unwrap();
    }
    editor.undo().unwrap();
    assert_eq!(editor.text(), "");
    editor.redo().unwrap();
    assert_eq!(editor.text(), "abc");
    editor
        .select(SelectionSet::single(Selection::caret(1)))
        .unwrap();
    editor.replace_selections("x", EditKind::Typing).unwrap();
    editor.undo().unwrap();
    assert_eq!(editor.text(), "abc");
}
#[test]
fn history_is_bounded_and_new_edit_discards_redo() {
    let mut editor = EditorState::new("");
    editor.set_history_options(HistoryOptions {
        max_bytes: 4096,
        ..Default::default()
    });
    for _ in 0..500 {
        editor
            .replace_selections("0123456789", EditKind::Command)
            .unwrap();
    }
    assert!(editor.history_bytes() <= 4096);
    editor.undo().unwrap();
    assert!(editor.can_redo());
    editor.replace_selections("x", EditKind::Command).unwrap();
    assert!(!editor.can_redo());
    editor.set_history_options(HistoryOptions {
        max_bytes: 0,
        ..Default::default()
    });
    assert_eq!(editor.history_bytes(), 0);
    assert!(!editor.can_undo());
}
#[test]
fn change_mapping_handles_boundaries_and_insertion_bias() {
    let mut editor = EditorState::new("abcdef");
    let change = editor
        .transact(Transaction::new(
            0,
            [Edit::new(1..1, "XX"), Edit::new(3..5, "Y")],
        ))
        .unwrap()
        .unwrap();
    assert_eq!(editor.text(), "aXXbcYf");
    assert_eq!(change.changes.map(1, Bias::Before), 1);
    assert_eq!(change.changes.map(1, Bias::After), 3);
    assert_eq!(change.changes.map(4, Bias::Before), 5);
    assert_eq!(change.changes.map(4, Bias::After), 6);
    assert_eq!(change.changes.map(5, Bias::After), 6);
}
#[test]
fn randomized_edit_undo_redo_roundtrips() {
    let mut editor = EditorState::new("α\nhello👨‍👩‍👧‍👦");
    let original = editor.text().to_owned();
    let mut seed = 42u64;
    for _ in 0..200 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let boundaries: Vec<_> = editor
            .text()
            .char_indices()
            .map(|(i, _)| i)
            .chain([editor.text().len()])
            .collect();
        let a = boundaries[seed as usize % boundaries.len()];
        let b = boundaries[(seed >> 32) as usize % boundaries.len()];
        editor
            .transact(Transaction::new(
                editor.revision(),
                [Edit::new(
                    a.min(b)..a.max(b),
                    ["中", "a", "\n", ""][seed as usize % 4],
                )],
            ))
            .unwrap();
    }
    let end = editor.text().to_owned();
    while editor.can_undo() {
        editor.undo().unwrap();
    }
    assert_eq!(editor.text(), original);
    while editor.can_redo() {
        editor.redo().unwrap();
    }
    assert_eq!(editor.text(), end);
}

#[test]
fn extension_commit_transaction_is_atomic_and_restores_rejected_preedit() {
    let mut state = EditorState::new("row\n");
    state
        .select(SelectionSet::single(Selection::caret(4)))
        .unwrap();
    state.set_composition("zhong", Some((0, 5))).unwrap();
    let composing = state.composition().cloned();
    let generation = state.generation();
    let guard = state.constrain(|_, _| Err(EditError::Rejected("protected")));
    let make_tx = |revision| {
        let mut tx = Transaction::new(revision, [Edit::new(4..4, "\n中文")]);
        tx.selection = Some(SelectionSet::single(Selection::caret(11)));
        tx
    };
    assert!(state.commit_transaction(make_tx(state.revision())).is_err());
    assert_eq!(state.text(), "row\n");
    assert_eq!(state.composition(), composing.as_ref());
    assert_eq!(state.generation(), generation);
    assert!(!state.can_undo());
    drop(guard);
    let change = state
        .commit_transaction(make_tx(state.revision()))
        .unwrap()
        .unwrap();
    assert_eq!(change.kind, EditKind::Composition);
    assert_eq!(state.text(), "row\n\n中文");
    assert!(state.composition().is_none());
    state.undo().unwrap();
    assert_eq!(state.text(), "row\n");
    assert_eq!(state.selections().primary(), Selection::caret(4));
    assert!(!state.can_undo());
    state.redo().unwrap();
    assert_eq!(state.text(), "row\n\n中文");
}
