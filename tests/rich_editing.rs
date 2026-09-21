//! Rich transactions share the plain editor's history, constraints and IME.
#![cfg(feature = "editing")]
use std::{
    borrow::Cow,
    collections::BTreeMap,
    sync::Arc,
    time::{Duration, Instant},
};
use voidui::{
    editing::*,
    render::{self, FontStyle, FontWeight},
};

fn bold() -> InlineStyle {
    InlineStyle {
        font_weight: Some(FontWeight::BOLD),
        ..Default::default()
    }
}
fn italic() -> InlineStyle {
    InlineStyle {
        font_style: Some(FontStyle::Italic),
        ..Default::default()
    }
}
fn rich(text: &str, spans: impl IntoIterator<Item = StyleSpan>) -> RichText {
    RichText::from_spans(text, spans).unwrap()
}
fn snapshot(state: &EditorState) -> RichText {
    state.document().rich_text()
}
fn apply(
    state: &mut EditorState,
    range: std::ops::Range<usize>,
    patch: StylePatch,
) -> Option<Change> {
    state
        .transact(Transaction::new(state.revision(), []).format(range, patch))
        .unwrap()
}
fn assert_canonical(state: &EditorState) {
    assert_eq!(snapshot(state).spans(), state.document().spans());
    for pair in state.document().spans().windows(2) {
        assert!(pair[0].range.end <= pair[1].range.start);
        assert!(pair[0].range.end != pair[1].range.start || pair[0].style != pair[1].style);
    }
}

#[test]
fn formatting_only_changes_revision_and_undo_without_text_deltas() {
    let mut state = EditorState::new("a中文z");
    let before = snapshot(&state);
    let change = apply(&mut state, 1..7, StylePatch::replace(bold())).unwrap();
    assert!(change.changes.edits().is_empty());
    assert!(!change.changes.is_empty());
    assert_eq!(change.changes.style_changes().len(), 1);
    assert_eq!(change.changes.map(4, Bias::Before), 4);
    assert_eq!(state.revision(), 1);
    assert_eq!(state.document().spans(), &[StyleSpan::new(1..7, bold())]);
    let after = snapshot(&state);
    let undone = state.undo().unwrap();
    assert_eq!(undone[0].kind, EditKind::Undo);
    assert!(undone[0].changes.edits().is_empty());
    assert_eq!(snapshot(&state), before);
    state.redo().unwrap();
    assert_eq!(snapshot(&state), after);
    assert_eq!(state.revision(), 3);
    let generation = state.generation();
    let bytes = state.history_bytes();
    assert!(apply(&mut state, 1..7, StylePatch::replace(bold())).is_none());
    assert_eq!(state.generation(), generation);
    assert_eq!(state.history_bytes(), bytes);
}

#[test]
fn patches_preserve_clear_and_merge_independent_fields_and_metadata() {
    let mut state = EditorState::new("abcdef");
    apply(&mut state, 0..6, StylePatch::replace(bold()));
    let metadata = Arc::new(BTreeMap::from([(
        "href".into(),
        "https://example.test".into(),
    )]));
    apply(
        &mut state,
        2..4,
        StylePatch {
            font_style: Some(Some(FontStyle::Italic)),
            metadata: Some(Some(metadata.clone())),
            ..Default::default()
        },
    );
    assert_eq!(state.document().spans().len(), 3);
    let style = &state.document().spans()[1].style;
    assert_eq!(style.font_weight, Some(FontWeight::BOLD));
    assert_eq!(style.metadata.as_ref(), Some(&metadata));
    apply(
        &mut state,
        2..4,
        StylePatch {
            font_style: Some(None),
            metadata: Some(None),
            ..Default::default()
        },
    );
    assert_eq!(state.document().spans(), &[StyleSpan::new(0..6, bold())]);
    apply(
        &mut state,
        1..5,
        StylePatch::replace(InlineStyle::default()),
    );
    assert_eq!(
        state.document().spans(),
        &[StyleSpan::new(0..1, bold()), StyleSpan::new(5..6, bold())]
    );
    state.undo().unwrap();
    assert_eq!(state.document().spans(), &[StyleSpan::new(0..6, bold())]);
}

#[test]
fn rich_replacement_and_formatting_use_output_coordinates_atomically() {
    let original = rich(
        "ab中文cd",
        [StyleSpan::new(0..2, bold()), StyleSpan::new(2..8, italic())],
    );
    let fragment = rich(
        "XY!",
        [StyleSpan::new(0..1, italic()), StyleSpan::new(2..3, bold())],
    );
    let mut state = EditorState::from_rich(original.clone());
    let tx =
        Transaction::new(0, [Edit::rich(2..8, fragment)]).format(5..7, StylePatch::replace(bold()));
    state.transact(tx).unwrap();
    assert_eq!(state.text(), "abXY!cd");
    assert_eq!(
        state.document().spans(),
        &[
            StyleSpan::new(0..2, bold()),
            StyleSpan::new(2..3, italic()),
            StyleSpan::new(4..7, bold())
        ]
    );
    let result = snapshot(&state);
    state.undo().unwrap();
    assert_eq!(snapshot(&state), original);
    state.redo().unwrap();
    assert_eq!(snapshot(&state), result);
}

#[test]
fn text_identical_rich_replacement_changes_only_styles() {
    let mut state = EditorState::from_rich(rich("hello", [StyleSpan::new(0..5, bold())]));
    state
        .select(SelectionSet::single(Selection::range(1, 4)))
        .unwrap();
    let selections = state.selections().clone();
    let change = state
        .transact(Transaction::new(
            0,
            [Edit::rich(1..4, RichText::new("ell"))],
        ))
        .unwrap()
        .unwrap();
    assert!(change.changes.edits().is_empty());
    assert_eq!(state.selections(), &selections);
    assert_eq!(
        state.document().spans(),
        &[StyleSpan::new(0..1, bold()), StyleSpan::new(4..5, bold())]
    );
    state.undo().unwrap();
    assert_eq!(state.document().spans(), &[StyleSpan::new(0..5, bold())]);
}

#[test]
fn invalid_styles_output_ranges_and_selections_leave_all_state_untouched() {
    let mut state = EditorState::from_rich(rich("a中b", [StyleSpan::new(1..4, bold())]));
    let original = snapshot(&state);
    let bad_styles = [
        InlineStyle {
            font_size: Some(f32::NAN),
            ..Default::default()
        },
        InlineStyle {
            line_height: Some(-1.0),
            ..Default::default()
        },
    ];
    for style in bad_styles {
        assert!(
            state
                .transact(Transaction::new(0, [Edit::styled(0..1, "x", style)]))
                .is_err()
        );
    }
    for range in [2..4, 0..9, 4..1] {
        assert!(
            state
                .transact(
                    Transaction::new(0, [Edit::new(0..1, "x")])
                        .format(range, StylePatch::replace(bold()))
                )
                .is_err()
        );
    }
    let mut tx = Transaction::new(0, []).format(0..1, StylePatch::replace(italic()));
    tx.selection = Some(SelectionSet::single(Selection::caret(2)));
    assert_eq!(state.transact(tx).unwrap_err(), EditError::InvalidSelection);
    assert_eq!(snapshot(&state), original);
    assert_eq!(state.revision(), 0);
    assert_eq!(state.generation(), 0);
    assert!(!state.can_undo());
}

#[test]
fn plain_insertion_inherits_but_rich_gaps_clear_and_queries_respect_bias() {
    let mut state = EditorState::from_rich(rich(
        "abcd",
        [StyleSpan::new(0..2, bold()), StyleSpan::new(2..4, italic())],
    ));
    assert_eq!(state.document().style_at(2, Bias::Before), Some(bold()));
    assert_eq!(state.document().style_at(2, Bias::After), Some(italic()));
    assert_eq!(state.document().style_at(99, Bias::After), None);
    state
        .transact(Transaction::new(0, [Edit::new(2..2, "XY")]))
        .unwrap();
    assert_eq!(
        state.document().spans(),
        &[StyleSpan::new(0..4, bold()), StyleSpan::new(4..6, italic())]
    );
    state
        .transact(Transaction::new(1, [Edit::rich(1..3, RichText::new("zz"))]))
        .unwrap();
    assert_eq!(
        state.document().spans(),
        &[
            StyleSpan::new(0..1, bold()),
            StyleSpan::new(3..4, bold()),
            StyleSpan::new(4..6, italic())
        ]
    );
    assert_eq!(
        state.document().rich_slice(2..5).unwrap().spans(),
        &[StyleSpan::new(1..2, bold()), StyleSpan::new(2..3, italic())]
    );
}

#[test]
fn multiselection_rich_paste_and_deletion_restore_exact_spans_and_selection() {
    let original = rich(
        "aa bb cc",
        [StyleSpan::new(0..2, bold()), StyleSpan::new(3..5, italic())],
    );
    let mut state = EditorState::from_rich(original.clone());
    state
        .select(SelectionSet::new([Selection::range(0, 2), Selection::range(3, 5)], 1).unwrap())
        .unwrap();
    let selected = state.selections().clone();
    let fragment = rich("中!", [StyleSpan::new(0..3, italic())]);
    state
        .replace_selections_rich(&fragment, EditKind::Paste)
        .unwrap();
    assert_eq!(state.text(), "中! 中! cc");
    assert_eq!(state.selections().primary().head, 9);
    assert_canonical(&state);
    let after = snapshot(&state);
    state.undo().unwrap();
    assert_eq!(snapshot(&state), original);
    assert_eq!(state.selections(), &selected);
    state.redo().unwrap();
    assert_eq!(snapshot(&state), after);
    state.delete(false, false).unwrap();
    state.undo().unwrap();
    assert_eq!(snapshot(&state), after);
}

#[test]
fn composition_projects_styles_without_mutation_and_commits_one_rich_history_step() {
    let original = rich(
        "a中文z",
        [
            StyleSpan::new(0..1, bold()),
            StyleSpan::new(1..7, italic()),
            StyleSpan::new(7..8, bold()),
        ],
    );
    let mut state = EditorState::from_rich(original.clone());
    assert!(matches!(state.display_spans(), Cow::Borrowed(_)));
    state
        .select(SelectionSet::single(Selection::range(1, 7)))
        .unwrap();
    state.set_typing_style(Some(bold())).unwrap();
    state.set_composition("ni", Some((2, 2))).unwrap();
    assert_eq!(
        state.display_spans().as_ref(),
        &[StyleSpan::new(0..4, bold())]
    );
    assert_eq!(snapshot(&state), original);
    assert!(!state.can_undo());
    assert_eq!(
        state.set_typing_style(Some(italic())).unwrap_err(),
        EditError::CompositionActive
    );
    assert_eq!(
        state
            .format_selections(StylePatch::replace(italic()))
            .unwrap_err(),
        EditError::CompositionActive
    );
    state.set_composition("你", Some((3, 3))).unwrap();
    assert_eq!(
        state.display_spans().as_ref(),
        &[StyleSpan::new(0..5, bold())]
    );
    state.commit_composition("你").unwrap();
    assert_eq!(state.text(), "a你z");
    assert_eq!(state.document().spans(), &[StyleSpan::new(0..5, bold())]);
    assert!(state.composition().is_none());
    state.undo().unwrap();
    assert_eq!(snapshot(&state), original);
    assert_eq!(state.selections().primary().text_range(), 1..7);
    assert_eq!(state.typing_style(), Some(&bold()));
    assert!(!state.can_undo());
    state.redo().unwrap();
    assert_eq!(state.text(), "a你z");
}

#[test]
fn constraints_see_style_deltas_and_rejected_commit_or_undo_preserves_preedit() {
    let mut state = EditorState::new("ab");
    apply(&mut state, 0..2, StylePatch::replace(bold()));
    let guard = state.constrain(|_, changes| {
        if !changes.edits().is_empty()
            || changes
                .style_changes()
                .iter()
                .any(|s| s.style == InlineStyle::default())
        {
            Err(EditError::Rejected("protected"))
        } else {
            Ok(())
        }
    });
    state.set_composition("ni", Some((2, 2))).unwrap();
    let composition = state.composition().cloned();
    let generation = state.generation();
    assert!(state.commit_composition("你").is_err());
    assert_eq!(state.composition(), composition.as_ref());
    assert_eq!(state.generation(), generation);
    assert!(state.undo().is_err());
    assert_eq!(state.composition(), composition.as_ref());
    assert_eq!(state.generation(), generation);
    assert!(state.can_undo());
    assert_eq!(state.revision(), 1);
    drop(guard);
    state.undo().unwrap();
    assert!(state.composition().is_none());
    assert!(state.document().spans().is_empty());
}

#[test]
fn typing_style_and_caret_formatting_survive_grouped_undo() {
    let mut state = EditorState::new("");
    state
        .format_selections(StylePatch::replace(bold()))
        .unwrap();
    assert!(!state.can_undo());
    assert_eq!(state.revision(), 0);
    state.replace_selections("a", EditKind::Typing).unwrap();
    state.replace_selections("b", EditKind::Typing).unwrap();
    assert_eq!(state.document().spans(), &[StyleSpan::new(0..2, bold())]);
    state.undo().unwrap();
    assert_eq!(state.text(), "");
    assert_eq!(state.typing_style(), Some(&bold()));
    state.redo().unwrap();
    state
        .select(SelectionSet::single(Selection::caret(0)))
        .unwrap();
    assert!(state.typing_style().is_none());
    state
        .set_typing_style(Some(InlineStyle::default()))
        .unwrap();
    state.replace_selections("x", EditKind::Typing).unwrap();
    assert_eq!(state.document().spans(), &[StyleSpan::new(1..3, bold())]);
}

#[test]
fn rich_history_is_bounded_including_metadata_and_new_format_discards_redo() {
    let mut state = EditorState::new("a");
    state.set_history_options(HistoryOptions {
        max_bytes: 2048,
        ..Default::default()
    });
    let style = InlineStyle {
        metadata: Some(Arc::new(BTreeMap::from([(
            "large".into(),
            "x".repeat(4096).into(),
        )]))),
        ..Default::default()
    };
    apply(&mut state, 0..1, StylePatch::replace(style));
    assert!(state.history_bytes() <= 2048);
    assert!(!state.can_undo());
    state.set_history_options(HistoryOptions {
        max_bytes: 65536,
        ..Default::default()
    });
    apply(&mut state, 0..1, StylePatch::replace(bold()));
    state.undo().unwrap();
    assert!(state.can_redo());
    apply(&mut state, 0..1, StylePatch::replace(italic()));
    assert!(!state.can_redo());
    state.set_history_options(HistoryOptions {
        max_bytes: 0,
        ..Default::default()
    });
    apply(&mut state, 0..1, StylePatch::replace(bold()));
    assert_eq!(state.history_bytes(), 0);
    assert!(!state.can_undo());
}

#[test]
fn editor_handle_keeps_rich_initialization_and_format_only_generations() {
    let original = rich("abc", [StyleSpan::new(0..3, bold())]);
    let editor = Editor::from_rich(original.clone());
    assert_eq!(editor.rich_text(), original);
    let other = editor.clone();
    let old = editor.with(EditorState::generation);
    other.update(|s| apply(s, 1..2, StylePatch::replace(italic())));
    assert!(editor.with(EditorState::generation) > old);
    assert_eq!(editor.text(), "abc");
    assert_eq!(editor.rich_text(), other.rich_text());
}

#[test]
fn format_history_is_a_boundary_between_typing_groups() {
    let mut state = EditorState::new("");
    let now = Instant::now();
    let mut tx = Transaction::new(0, [Edit::new(0..0, "a")]);
    tx.kind = EditKind::Typing;
    state.transact_at(tx, now).unwrap();
    apply(&mut state, 0..1, StylePatch::replace(bold()));
    let mut tx = Transaction::new(2, [Edit::new(1..1, "b")]);
    tx.kind = EditKind::Typing;
    state
        .transact_at(tx, now + Duration::from_millis(1))
        .unwrap();
    state.undo().unwrap();
    assert_eq!(state.text(), "a");
    assert_eq!(state.document().spans(), &[StyleSpan::new(0..1, bold())]);
    state.undo().unwrap();
    assert_eq!(state.text(), "a");
    assert!(state.document().spans().is_empty());
    state.undo().unwrap();
    assert_eq!(state.text(), "");
}

#[test]
fn fluent_patches_override_inherited_typography_and_clear_explicitly() {
    let mut state = EditorState::new("abc");
    let color = render::black();
    apply(
        &mut state,
        0..3,
        StylePatch::new()
            .bold(false)
            .italic(false)
            .color(color)
            .font_size(18.0)
            .line_height(24.0),
    );
    let style = &state.document().spans()[0].style;
    assert_eq!(style.font_weight, Some(FontWeight::NORMAL));
    assert_eq!(style.font_style, Some(FontStyle::Normal));
    assert_eq!(style.color, Some(color));
    assert_eq!(style.font_size, Some(18.0));
    assert_eq!(style.line_height, Some(24.0));
    assert!(
        state
            .transact(
                Transaction::new(state.revision(), [])
                    .format(0..3, StylePatch::new().font_size(f32::INFINITY))
            )
            .is_err()
    );
    apply(&mut state, 0..3, StylePatch::clear());
    assert!(state.document().spans().is_empty());
}

#[test]
fn adjacent_deletions_and_rich_insertions_have_reversible_style_coordinates() {
    let original = rich(
        "abcdef",
        [
            StyleSpan::new(0..2, bold()),
            StyleSpan::new(2..4, italic()),
            StyleSpan::new(4..6, bold()),
        ],
    );
    for edits in [
        vec![Edit::new(0..2, ""), Edit::new(2..4, "")],
        vec![
            Edit::rich(0..2, RichText::new("")),
            Edit::styled(2..4, "中", bold()),
        ],
        vec![
            Edit::styled(0..0, "z", italic()),
            Edit::new(2..4, ""),
            Edit::styled(4..6, "好", italic()),
        ],
    ] {
        let mut state = EditorState::from_rich(original.clone());
        state.transact(Transaction::new(0, edits)).unwrap();
        assert_canonical(&state);
        let after = snapshot(&state);
        state.undo().unwrap();
        assert_eq!(snapshot(&state), original);
        state.redo().unwrap();
        assert_eq!(snapshot(&state), after);
    }
}

#[test]
fn sparse_format_history_retains_only_affected_spans() {
    let count = 10000;
    let original = rich(
        &"a".repeat(count),
        (0..count).map(|i| StyleSpan::new(i..i + 1, if i % 2 == 0 { bold() } else { italic() })),
    );
    let mut state = EditorState::from_rich(original.clone());
    apply(&mut state, 5000..5001, StylePatch::new().font_size(18.0));
    // Formatting one byte must not retain the other 9,999 style records.
    assert!(state.history_bytes() < 8192, "{}", state.history_bytes());
    state.undo().unwrap();
    assert_eq!(snapshot(&state), original);
}

#[test]
fn large_rich_paste_and_undo_apply_disjoint_deltas_in_one_sweep() {
    let count = 12000;
    let fragment = rich(
        &"x".repeat(count),
        (0..count).map(|i| StyleSpan::new(i..i + 1, if i % 2 == 0 { bold() } else { italic() })),
    );
    let mut state = EditorState::new("prefix suffix");
    state.set_history_options(HistoryOptions {
        max_bytes: 64 * 1024 * 1024,
        ..Default::default()
    });
    let before = snapshot(&state);
    state
        .transact(Transaction::new(0, [Edit::rich(7..7, fragment)]))
        .unwrap();
    assert_eq!(state.document().spans().len(), count);
    let after = snapshot(&state);
    assert_canonical(&state);
    state.undo().unwrap();
    assert_eq!(snapshot(&state), before);
    state.redo().unwrap();
    assert_eq!(snapshot(&state), after);
}

#[test]
fn randomized_rich_transactions_match_independent_scalar_reference_and_history() {
    // The reference uses one style per Unicode scalar, intentionally sharing no
    // interval mapping or canonicalization implementation with the editor.
    for mut seed in [1u64, 42, 999] {
        let mut random = || {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            seed as usize
        };
        let mut state = EditorState::new("a中e\u{301}🙂z");
        state.set_history_options(HistoryOptions {
            max_bytes: 64 * 1024 * 1024,
            ..Default::default()
        });
        let mut reference: Vec<_> = state
            .text()
            .chars()
            .map(|ch| (ch, InlineStyle::default()))
            .collect();
        let mut versions = vec![snapshot(&state)];
        for step in 0..500 {
            let offsets = |chars: &[(char, InlineStyle)]| {
                let mut bytes = vec![0];
                for (ch, _) in chars {
                    bytes.push(bytes.last().unwrap() + ch.len_utf8());
                }
                bytes
            };
            let old = offsets(&reference);
            let a = random() % old.len();
            let b = random() % old.len();
            let (a, b) = (a.min(b), a.max(b));
            let mut tx = Transaction::new(state.revision(), []);
            if step % 3 != 0 {
                let text = ["", "中", "a", "🙂e\u{301}"][random() % 4];
                if step % 2 == 0 {
                    let style = if random() % 2 == 0 { bold() } else { italic() };
                    tx.edits
                        .push(Edit::styled(old[a]..old[b], text, style.clone()));
                    reference.splice(a..b, text.chars().map(|ch| (ch, style.clone())));
                } else {
                    tx.edits.push(Edit::new(old[a]..old[b], text));
                    let replaced: String = reference[a..b].iter().map(|(ch, _)| ch).collect();
                    if replaced != text {
                        let style = reference
                            .get(if a == b && a > 0 { a - 1 } else { a })
                            .map(|(_, style)| style.clone())
                            .unwrap_or_default();
                        reference.splice(a..b, text.chars().map(|ch| (ch, style.clone())));
                    }
                }
            }
            let bytes = offsets(&reference);
            let c = random() % bytes.len();
            let d = random() % bytes.len();
            let (c, d) = (c.min(d), c.max(d));
            let weight = if random() % 2 == 0 {
                Some(FontWeight::BOLD)
            } else {
                None
            };
            tx = tx.format(
                bytes[c]..bytes[d],
                StylePatch {
                    font_weight: Some(weight),
                    ..Default::default()
                },
            );
            for (_, style) in &mut reference[c..d] {
                style.font_weight = weight;
            }
            let changed = state.transact(tx).unwrap().is_some();
            let text: String = reference.iter().map(|(ch, _)| ch).collect();
            let expected = rich(
                &text,
                reference
                    .iter()
                    .enumerate()
                    .map(|(i, (_, style))| StyleSpan::new(bytes[i]..bytes[i + 1], style.clone())),
            );
            assert_eq!(snapshot(&state), expected, "step {step}");
            assert_canonical(&state);
            if changed {
                versions.push(expected);
            }
        }
        for expected in versions[..versions.len() - 1].iter().rev() {
            state.undo().unwrap();
            assert_eq!(&snapshot(&state), expected);
        }
        assert!(!state.can_undo());
        for expected in &versions[1..] {
            state.redo().unwrap();
            assert_eq!(&snapshot(&state), expected);
        }
        assert!(!state.can_redo());
    }
}

#[test]
fn formatting_preserves_selection_affinity_and_vertical_navigation_state() {
    let mut state = EditorState::new("abc");
    let mut selection = Selection::range(2, 0);
    selection.affinity = Bias::Before;
    selection.preferred_x = Some(12.0);
    state.select(SelectionSet::single(selection)).unwrap();
    state
        .format_selections(StylePatch::new().bold(true))
        .unwrap();
    assert_eq!(state.selections().primary(), selection);
    state.undo().unwrap();
    assert_eq!(state.selections().primary(), selection);
    state.redo().unwrap();
    assert_eq!(state.selections().primary(), selection);
}

#[test]
fn overlapping_patches_run_in_order_and_cannot_partially_apply() {
    let mut state = EditorState::new("abcdef");
    state
        .transact(
            Transaction::new(0, [])
                .format(0..5, StylePatch::new().bold(true))
                .format(1..4, StylePatch::new().italic(true))
                .format(2..3, StylePatch::clear()),
        )
        .unwrap();
    let mut combined = bold();
    combined.font_style = Some(FontStyle::Italic);
    assert_eq!(
        state.document().spans(),
        &[
            StyleSpan::new(0..1, bold()),
            StyleSpan::new(1..2, combined.clone()),
            StyleSpan::new(3..4, combined),
            StyleSpan::new(4..5, bold()),
        ]
    );
    let before = snapshot(&state);
    let generation = state.generation();
    assert!(
        state
            .transact(
                Transaction::new(1, [])
                    .format(0..6, StylePatch::clear())
                    .format(6..7, StylePatch::new().bold(true))
            )
            .is_err()
    );
    assert_eq!(snapshot(&state), before);
    assert_eq!(state.generation(), generation);
    state.undo().unwrap();
    assert!(state.document().spans().is_empty());
}

#[test]
fn ime_cancel_empty_commit_and_multicursor_commit_preserve_styles() {
    let original = rich(
        "ab cd",
        [StyleSpan::new(0..2, bold()), StyleSpan::new(3..5, italic())],
    );
    let mut state = EditorState::from_rich(original.clone());
    state
        .select(SelectionSet::new([Selection::range(0, 2), Selection::range(3, 5)], 1).unwrap())
        .unwrap();
    state.set_composition("ni", Some((2, 2))).unwrap();
    assert_eq!(state.display_spans().as_ref(), original.spans());
    state.cancel_composition();
    assert_eq!(snapshot(&state), original);
    state.set_composition("你", None).unwrap();
    assert_eq!(
        state.display_spans().as_ref(),
        &[StyleSpan::new(0..2, bold()), StyleSpan::new(3..6, italic())]
    );
    state.commit_composition("你").unwrap();
    assert_eq!(state.text(), "你 你");
    assert_eq!(
        state.document().spans(),
        &[StyleSpan::new(0..3, bold()), StyleSpan::new(4..7, italic())]
    );
    state.undo().unwrap();
    assert_eq!(snapshot(&state), original);
    state.set_composition("ni", Some((2, 2))).unwrap();
    state.commit_composition("").unwrap();
    assert_eq!(state.text(), " ");
    assert!(state.document().spans().is_empty());
    state.undo().unwrap();
    assert_eq!(snapshot(&state), original);
}

#[test]
fn rich_payload_is_revalidated_if_public_insert_text_is_modified() {
    let mut state = EditorState::new("abc");
    let mut edit = Edit::rich(0..1, rich("中", [StyleSpan::new(0..3, bold())]));
    edit.insert = "x".into();
    assert!(state.transact(Transaction::new(0, [edit])).is_err());
    assert_eq!(state.text(), "abc");
    assert_eq!(state.revision(), 0);
    assert_eq!(state.history_bytes(), 0);
}

#[test]
fn metadata_only_edits_have_revisions_history_and_stale_revision_checks() {
    let mut state = EditorState::new("link");
    let metadata = Arc::new(BTreeMap::from([(
        "href".into(),
        "https://example.test".into(),
    )]));
    apply(
        &mut state,
        0..4,
        StylePatch {
            metadata: Some(Some(metadata)),
            ..Default::default()
        },
    );
    assert!(matches!(
        state.transact(Transaction::new(0, []).format(0..1, StylePatch::clear())),
        Err(EditError::StaleRevision { .. })
    ));
    let after = snapshot(&state);
    state.undo().unwrap();
    assert!(state.document().spans().is_empty());
    state.redo().unwrap();
    assert_eq!(snapshot(&state), after);
}

#[test]
fn rejected_grouped_undo_and_redo_keep_rich_history_atomic() {
    let mut state = EditorState::new("");
    state.set_typing_style(Some(bold())).unwrap();
    state.replace_selections("a", EditKind::Typing).unwrap();
    state.replace_selections("b", EditKind::Typing).unwrap();
    let guard = state.constrain(|document, changes| {
        if document.text() == "a" && changes.edits().iter().any(|e| e.insert.is_empty()) {
            Err(EditError::Rejected("retain a"))
        } else {
            Ok(())
        }
    });
    let before = snapshot(&state);
    let revision = state.revision();
    assert!(state.undo().is_err());
    assert_eq!(snapshot(&state), before);
    assert_eq!(state.revision(), revision);
    assert!(state.can_undo());
    drop(guard);
    state.undo().unwrap();
    let guard = state.constrain(|document, _| {
        if document.text() == "a" {
            Err(EditError::Rejected("reject second record"))
        } else {
            Ok(())
        }
    });
    assert!(state.redo().is_err());
    assert_eq!(state.text(), "");
    assert!(state.document().spans().is_empty());
    assert!(state.can_redo());
    drop(guard);
    state.redo().unwrap();
    assert_eq!(snapshot(&state), before);
}

#[test]
fn plain_session_has_no_inline_style_sized_storage() {
    // A whole style and preedit are heap allocated only when used. Keep a small
    // structural budget so adding inline options cannot silently bloat controls.
    assert!(
        std::mem::size_of::<EditorState>() <= 320,
        "{}",
        std::mem::size_of::<EditorState>()
    );
    let state = EditorState::new("");
    assert!(state.document().spans().is_empty());
    assert!(state.typing_style().is_none());
    assert!(state.composition().is_none());
    assert_eq!(
        Edit::styled(0..0, "x", InlineStyle::default()).spans(),
        Some(&[][..])
    );
}

#[test]
fn undo_mapping_includes_all_insertions_at_an_adjacent_deletion_boundary() {
    let mut state = EditorState::from_rich(rich(
        "abcdef",
        [StyleSpan::new(0..2, bold()), StyleSpan::new(2..4, italic())],
    ));
    state
        .transact(Transaction::new(
            0,
            [Edit::new(0..2, ""), Edit::new(2..4, "")],
        ))
        .unwrap();
    let undo = state.undo().unwrap();
    assert_eq!(undo[0].changes.map(0, Bias::Before), 0);
    assert_eq!(undo[0].changes.map(0, Bias::After), 4);
    assert_eq!(undo[0].changes.map(1, Bias::After), 5);
}

#[test]
fn many_disjoint_formatting_operations_use_canonical_output_intervals() {
    let count = 5000;
    let mut state = EditorState::new("x".repeat(count));
    state.set_history_options(HistoryOptions {
        max_bytes: 64 * 1024 * 1024,
        ..Default::default()
    });
    let mut tx = Transaction::new(0, []);
    tx.formats = (0..count)
        .map(|i| Format {
            range: i..i + 1,
            patch: StylePatch::replace(if i % 2 == 0 { bold() } else { italic() }),
        })
        .collect();
    state.transact(tx).unwrap();
    assert_eq!(state.document().spans().len(), count);
    let after = snapshot(&state);
    state.undo().unwrap();
    assert!(state.document().spans().is_empty());
    state.redo().unwrap();
    assert_eq!(snapshot(&state), after);
}

#[test]
#[ignore = "manual performance report; no machine-dependent timing assertions"]
fn rich_editing_performance_report() {
    println!(
        "EditorState={} InlineStyle={} Edit={}",
        std::mem::size_of::<EditorState>(),
        std::mem::size_of::<InlineStyle>(),
        std::mem::size_of::<Edit>()
    );
    let mut plain = EditorState::new("");
    plain.set_history_options(HistoryOptions {
        max_bytes: 0,
        ..Default::default()
    });
    let start = Instant::now();
    for _ in 0..100000 {
        plain.replace_selections("a", EditKind::Typing).unwrap();
    }
    println!("plain_typing edits=100000 time={:?}", start.elapsed());
    for count in [1000, 10000, 100000] {
        let fragment = rich(
            &"x".repeat(count),
            (0..count)
                .map(|i| StyleSpan::new(i..i + 1, if i % 2 == 0 { bold() } else { italic() })),
        );
        let mut state = EditorState::new("");
        state.set_history_options(HistoryOptions {
            max_bytes: 256 * 1024 * 1024,
            ..Default::default()
        });
        let start = Instant::now();
        state
            .replace_selections_rich(&fragment, EditKind::Paste)
            .unwrap();
        let paste = start.elapsed();
        let start = Instant::now();
        state.undo().unwrap();
        let undo = start.elapsed();
        let start = Instant::now();
        state.redo().unwrap();
        let redo = start.elapsed();
        assert_eq!(state.document().spans(), fragment.spans());
        println!(
            "spans={count} paste={paste:?} undo={undo:?} redo={redo:?} history_bytes={}",
            state.history_bytes()
        );
    }
}
