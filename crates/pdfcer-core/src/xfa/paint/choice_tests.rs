#![allow(clippy::unwrap_used, clippy::indexing_slicing)] // a panic here is the test failing

use super::check_tests::painted;
use super::*;

/// A list field with display and save items, `open` and `value` as given.
fn list(open: &str, value: &str, extra: &str) -> String {
    format!(
        r#"<field name="c" x="0pt" y="0pt" w="80pt" h="60pt"><ui><choiceList open="{open}"{extra}/></ui><font typeface="Helvetica"/><items><text>Apple</text><text>Pear</text></items><items save="1"><text>a</text><text>p</text></items><value><text>{value}</text></value></field>"#
    )
}

#[test]
fn a_drop_down_shows_the_display_text_of_its_value() {
    let (ops, notes) = painted(&list("userControl", "p", ""), &PaintOptions::new());
    assert!(ops.contains("(Pear) Tj"), "{ops}");
    assert!(!ops.contains("(Apple)"), "{ops}");
    assert!(notes.is_empty(), "{notes:?}");
}

#[test]
fn a_value_matching_no_item_is_shown_as_saved_and_disclosed_unless_typed() {
    let (ops, notes) = painted(&list("onEntry", "kiwi", ""), &PaintOptions::new());
    assert!(ops.contains("(kiwi) Tj"), "{ops}");
    assert_eq!(notes[0].kind, PaintNoteKind::Choice);
    let typed = list("onEntry", "kiwi", r#" textEntry="1""#);
    let (_, notes) = painted(&typed, &PaintOptions::new());
    assert!(notes.is_empty(), "{notes:?}");
}

#[test]
fn an_open_list_shows_every_item_and_highlights_the_selection() {
    let (ops, notes) = painted(&list("multiSelect", "a p", ""), &PaintOptions::new());
    assert!(
        ops.contains("(Apple) Tj") && ops.contains("(Pear) Tj"),
        "{ops}"
    );
    assert_eq!(ops.matches("0.6 0.75 0.95 rg").count(), 2, "{ops}");
    assert!(notes.is_empty(), "{notes:?}");
    let (ops, _) = painted(&list("always", "p", ""), &PaintOptions::new());
    assert_eq!(ops.matches("0.6 0.75 0.95 rg").count(), 1, "{ops}");
}

#[test]
fn the_drop_down_button_is_drawn_only_on_screen() {
    let field = list("userControl", "", "");
    let paper = PaintOptions::new().with_medium(Medium::Paper);
    let (paper, _) = painted(&field, &paper);
    assert!(!paper.contains(" f"), "{paper}");
    let (ops, notes) = painted(&field, &PaintOptions::new());
    assert!(ops.contains("0.83 0.83 0.83 rg"), "{ops}");
    assert!(ops.contains(" h f"), "the arrow: {ops}");
    assert!(notes.is_empty(), "{notes:?}");
}

#[test]
fn bound_items_are_not_resolved_and_say_so() {
    let field = r#"<field name="c" w="80pt" h="20pt"><ui><choiceList/></ui><bindItems ref="$record.L[*]"/><value><text>x</text></value></field>"#;
    let (ops, notes) = painted(field, &PaintOptions::new());
    assert!(ops.contains("(x) Tj"), "{ops}");
    assert_eq!(notes.last().unwrap().kind, PaintNoteKind::Choice);
    assert!(
        !notes.iter().any(|n| n.kind == PaintNoteKind::Content),
        "{notes:?}"
    );
}
