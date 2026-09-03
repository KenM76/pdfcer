//! `/I` and `/TI` — a choice field's selected-index array and top index
//! (ISO 32000-1 §12.7.4.4, Table 231).
//!
//! ## Why these two keys need tests of their own
//!
//! Neither changes what pdfcer draws. `/I` restates a selection that `/V`
//! already carries, and `/TI` is a scroll position pdfcer's own list-box
//! appearance ignores entirely — it paints the selected values, not a
//! scrollable option list. So every defect in this area is **invisible in
//! pdfcer and visible in Acrobat**, which renders a list box as a live control
//! from `/Opt` regardless of `/AP`.
//!
//! That makes them exactly the kind of thing a fill path gets subtly wrong
//! and nobody notices:
//!
//! - **`/I` unsorted.** Table 231 says *"sorted in ascending order"*. The
//!   selections arrive in the order the caller named them, and `--value MX
//!   --value CA` against `[CA MX AR]` yields `[1 0]` — a conforming reader's
//!   cue that the array is not to be trusted.
//! - **`/I` on a single-select field.** Table 231 scopes it to `MultiSelect`
//!   and says `/V` wins on conflict, so a redundant `/I` is a second thing
//!   for a later editor to forget to update, whose only defined behaviour on
//!   disagreement is to be ignored.
//! - **`/TI` never written.** Fill the fortieth option of a fifty-option list
//!   and the operator opens the form to a window showing options one through
//!   six, with no visible selection at all.
//! - **`/TI` never CLEARED.** A field scrolled by an earlier fill that then
//!   gets a top-of-list selection would keep the stale window.
//!
//! Each of those is asserted against the written dictionary, because the
//! model reads `/V` and would report the right selection in every one of
//! them.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::{ChoiceOption, EditSession, NewChoiceField};
use pdfcer_core::forms;
use pdfcer_core::graph::ObjectGraph;
use pdfcer_core::object::{Dict, ObjId, Object};
use pdfcer_core::page_tree::Rect;
use std::path::{Path, PathBuf};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic")
        .join(name)
}

fn session(rel: &str) -> EditSession {
    EditSession::new(Document::load(&fixture(rel)).expect("load fixture"))
}

/// A list box tall enough to show SEVERAL rows at the auto-size the generator
/// picks for it (60 pt ⇒ 12 pt text ⇒ four visible rows) — big enough that
/// "below the fold" is a real state rather than an artefact of a window that
/// only ever holds one thing.
fn tall_rect() -> Rect {
    Rect {
        llx: 40.0,
        lly: 500.0,
        urx: 240.0,
        ury: 560.0,
    }
}

/// A short box, ONE line high — every option after the first is below the
/// fold. The degenerate end of the range, and worth having: a window of one
/// is where an off-by-one in the visibility test stops being ambiguous.
///
/// (18 points tall ⇒ `auto_size` picks 12 pt ⇒ one visible row. Both numbers
/// come from the layout engine's own constants, which is why the tests below
/// ask `visible_line_count` rather than hard-coding a count that would go
/// stale the moment `LINE_FACTOR` moved.)
fn short_rect() -> Rect {
    Rect {
        llx: 40.0,
        lly: 100.0,
        urx: 240.0,
        ury: 118.0,
    }
}

/// `[CA MX AR]` — export and display DELIBERATELY different, so a fill that
/// collapsed the pair would still look right and would still be wrong.
fn countries() -> Vec<ChoiceOption> {
    vec![
        ChoiceOption::new("CA", "Canada"),
        ChoiceOption::new("MX", "Mexico"),
        ChoiceOption::new("AR", "Argentina"),
    ]
}

/// A long list, so an index can be genuinely below the fold.
fn many(n: usize) -> Vec<ChoiceOption> {
    (0..n)
        .map(|i| ChoiceOption::new(format!("E{i}"), format!("Option {i}")))
        .collect()
}

fn dict_of(s: &EditSession, id: ObjId) -> Dict {
    s.graph()
        .resolved(id)
        .as_dict()
        .expect("a dictionary")
        .clone()
}

fn ints(s: &EditSession, id: ObjId, key: &[u8]) -> Option<Vec<i64>> {
    let g = s.graph();
    let d = dict_of(s, id);
    let v = g.resolve(d.get(key)?).clone();
    Some(
        v.as_array()?
            .iter()
            .filter_map(|o| g.resolve(o).as_int())
            .collect(),
    )
}

fn int(s: &EditSession, id: ObjId, key: &[u8]) -> Option<i64> {
    let g = s.graph();
    g.resolve(dict_of(s, id).get(key)?).as_int()
}

/// THE HEADLINE for `/I`: named out of order, written in ascending order.
///
/// Table 231: *"an array of integers, **sorted in ascending order**"*.
#[test]
fn the_selected_index_array_is_sorted_ascending_whatever_order_it_was_named_in() {
    let mut s = session("dimension/plain-base.pdf");
    let id = s
        .add_choice_field(
            &NewChoiceField::new(0, "Country", tall_rect(), countries())
                .multi_select(true)
                .declining_tooltip(),
        )
        .expect("add")
        .field_id;

    // Named LAST-to-FIRST. The `/V` array keeps the caller's order (it is the
    // caller's list of values); `/I` does not (it is a spec-shaped index set).
    s.set_choice_value("Country", &["Argentina", "Canada"])
        .expect("fill");

    assert_eq!(
        ints(&s, id, b"I"),
        Some(vec![0, 2]),
        "/I is ascending, not the order the selections were named in"
    );
    // And `/V` is untouched by the sort — the two arrays answer different
    // questions and only one of them is index-shaped.
    let g = s.graph();
    let v = g.resolve(dict_of(&s, id).get(b"V").expect("/V")).clone();
    let exports: Vec<Vec<u8>> = v
        .as_array()
        .expect("multi-select /V is an array")
        .iter()
        .filter_map(|o| match g.resolve(o) {
            Object::String(b) => Some(b.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        exports,
        vec![b"AR".to_vec(), b"CA".to_vec()],
        "/V holds the EXPORT values in the caller's order — sorting it would \
         have changed the submitted data to make an index array tidy"
    );
}

/// A repeated selection produces one index, not two. Naming an option twice
/// is not worth refusing, but `[0 0]` is a malformed index array either way.
#[test]
fn a_repeated_selection_does_not_produce_a_duplicate_index() {
    let mut s = session("dimension/plain-base.pdf");
    let id = s
        .add_choice_field(
            &NewChoiceField::new(0, "Country", tall_rect(), countries())
                .multi_select(true)
                .declining_tooltip(),
        )
        .expect("add")
        .field_id;
    s.set_choice_value("Country", &["Canada", "Canada", "Mexico"])
        .expect("fill");
    assert_eq!(ints(&s, id, b"I"), Some(vec![0, 1]));
}

/// A SINGLE-SELECT field gets no `/I` at all.
///
/// Table 231 scopes `/I` to `MultiSelect` and says `/V` wins when the two
/// disagree — so on a single-select field it is a redundant restatement whose
/// only defined behaviour on conflict is to be ignored.
#[test]
fn a_single_select_field_is_written_without_an_index_array() {
    let mut s = session("dimension/plain-base.pdf");
    let id = s
        .add_choice_field(
            &NewChoiceField::new(0, "Country", tall_rect(), countries()).declining_tooltip(),
        )
        .expect("add")
        .field_id;
    s.set_choice_value("Country", &["Mexico"]).expect("fill");

    assert!(
        dict_of(&s, id).get(b"I").is_none(),
        "/I is absent on a single-select field"
    );
    // The selection still landed — the absent key costs nothing.
    let g = s.graph();
    assert_eq!(
        g.resolve(dict_of(&s, id).get(b"V").expect("/V")).clone(),
        Object::String(b"MX".to_vec())
    );
}

/// A stale `/I` left by another producer is CLEARED on fill, not left
/// pointing at the previous selection.
///
/// The removal is unconditional rather than "remove what we wrote", which is
/// what makes this case work: the field carried an `/I` this session never
/// authored.
#[test]
fn a_stale_index_array_is_cleared_when_the_field_becomes_single_select_shaped() {
    let mut s = session("dimension/plain-base.pdf");
    let id = s
        .add_choice_field(
            &NewChoiceField::new(0, "Country", tall_rect(), countries())
                .multi_select(true)
                .declining_tooltip(),
        )
        .expect("add")
        .field_id;
    s.set_choice_value("Country", &["Argentina"]).expect("fill");
    assert_eq!(ints(&s, id, b"I"), Some(vec![2]), "precondition: /I exists");

    // Clearing to nothing removes it rather than writing an empty array.
    s.set_choice_value("Country", &[]).expect("clear");
    assert!(
        dict_of(&s, id).get(b"I").is_none(),
        "an empty selection leaves NO index array, not an empty one"
    );
}

/// THE HEADLINE for `/TI`: a selection below the fold scrolls the list so it
/// is on screen.
#[test]
fn a_selection_below_the_fold_scrolls_the_list_box_to_it() {
    let mut s = session("dimension/plain-base.pdf");
    let id = s
        .add_choice_field(
            &NewChoiceField::new(0, "Pick", tall_rect(), many(50)).declining_tooltip(),
        )
        .expect("add")
        .field_id;

    let out = s.set_choice_value("Pick", &["Option 40"]).expect("fill");
    let ti = out
        .top_index
        .expect("a selection this far down must scroll");
    assert_eq!(
        int(&s, id, b"TI"),
        Some(ti),
        "the reported top index is the one that was written — a summary that \
         disagreed with the artefact would be the wrong-number-beside-a-\
         right-one shape"
    );
    assert!(
        ti <= 40,
        "the window starts at or before the selection, so the selection is \
         inside it: TI={ti}"
    );
    assert!(ti > 0, "and it actually scrolled: TI={ti}");
    assert_eq!(
        forms::parse_acroform(&s.graph())
            .expect("form")
            .fields
            .iter()
            .find(|f| f.fully_qualified_name == "Pick")
            .expect("field")
            .top_index,
        ti,
        "and the ordinary reader models it back"
    );
}

/// A selection in the FIRST window writes no `/TI` — Table 231's default is
/// 0, so the honest encoding of "top of the list" is an absent key.
#[test]
fn a_selection_in_the_first_window_leaves_the_key_absent() {
    let mut s = session("dimension/plain-base.pdf");
    let id = s
        .add_choice_field(
            &NewChoiceField::new(0, "Pick", tall_rect(), many(50)).declining_tooltip(),
        )
        .expect("add")
        .field_id;

    // Option 2, not option 0: a rule that only ever leaves the key absent for
    // the very first option would pass a test written against index 0 while
    // scrolling every other in-window selection unnecessarily.
    let out = s.set_choice_value("Pick", &["Option 2"]).expect("fill");
    assert_eq!(out.top_index, None);
    assert!(
        dict_of(&s, id).get(b"TI").is_none(),
        "absent, not written as 0"
    );
}

/// A previously-scrolled field that then gets a top-of-list selection has its
/// `/TI` REMOVED, not left stale.
///
/// This is the case an "only write when scrolling" implementation gets wrong:
/// it writes the key on the first fill and never takes it back, so the form
/// opens scrolled past a selection sitting at the top.
#[test]
fn scrolling_back_to_the_top_removes_the_stale_window() {
    let mut s = session("dimension/plain-base.pdf");
    let id = s
        .add_choice_field(
            &NewChoiceField::new(0, "Pick", tall_rect(), many(50)).declining_tooltip(),
        )
        .expect("add")
        .field_id;

    s.set_choice_value("Pick", &["Option 40"]).expect("fill");
    assert!(
        int(&s, id, b"TI").is_some_and(|ti| ti > 0),
        "precondition: the field is scrolled"
    );

    s.set_choice_value("Pick", &["Option 1"]).expect("refill");
    assert!(
        dict_of(&s, id).get(b"TI").is_none(),
        "the stale window is gone — the new selection is in the first window"
    );
}

/// The clamp: a selection near the END of the list does not scroll past the
/// last option.
///
/// Scrolling to `first` there would place the window beyond the list; the
/// clamp lands it on the last full page, which still contains the selection.
/// Asserted by checking the selection is inside `[TI, TI + visible)` for the
/// visible count the layout engine itself reports.
#[test]
fn a_selection_at_the_very_end_clamps_to_the_last_full_window() {
    let mut s = session("dimension/plain-base.pdf");
    let id = s
        .add_choice_field(
            &NewChoiceField::new(0, "Pick", short_rect(), many(50)).declining_tooltip(),
        )
        .expect("add")
        .field_id;

    let out = s.set_choice_value("Pick", &["Option 49"]).expect("fill");
    let ti = out.top_index.expect("the last option must scroll");
    let h = short_rect().ury - short_rect().lly;
    let size = pdfcer_core::vartext::auto_size(h);
    let visible = i64::try_from(pdfcer_core::vartext::visible_line_count(h, size)).unwrap();
    assert!(
        ti <= 49 && 49 < ti + visible,
        "the last option is inside the window [{ti}, {}) — the clamp is what \
         keeps it there rather than scrolling off the bottom",
        ti + visible
    );
    assert_eq!(
        ti,
        50 - visible,
        "and the window is the LAST FULL one, not one starting at the selection"
    );
    assert_eq!(int(&s, id, b"TI"), Some(ti));
}

/// A COMBO box is never scrolled. Table 231 scopes `/TI` to list boxes, and a
/// combo shows one collapsed line — there is no window to position.
#[test]
fn a_combo_box_is_never_given_a_top_index() {
    let mut s = session("dimension/plain-base.pdf");
    let id = s
        .add_choice_field(
            &NewChoiceField::new(0, "Pick", short_rect(), many(50))
                .as_combo(false)
                .declining_tooltip(),
        )
        .expect("add")
        .field_id;

    let out = s.set_choice_value("Pick", &["Option 40"]).expect("fill");
    assert_eq!(out.top_index, None, "a combo box has no scroll position");
    assert!(dict_of(&s, id).get(b"TI").is_none());
}

/// A tall list box shows more rows, so the same selection needs less
/// scrolling — or none. Proves the derivation is a function of the WIDGET,
/// not a constant.
#[test]
fn a_taller_list_box_scrolls_less_for_the_same_selection() {
    let mut short = session("dimension/plain-base.pdf");
    short
        .add_choice_field(
            &NewChoiceField::new(0, "Pick", short_rect(), many(50)).declining_tooltip(),
        )
        .expect("add");
    let short_ti = short
        .set_choice_value("Pick", &["Option 2"])
        .expect("fill")
        .top_index;

    let mut tall = session("dimension/plain-base.pdf");
    tall.add_choice_field(
        &NewChoiceField::new(0, "Pick", tall_rect(), many(50)).declining_tooltip(),
    )
    .expect("add");
    let tall_ti = tall
        .set_choice_value("Pick", &["Option 2"])
        .expect("fill")
        .top_index;

    assert_eq!(
        short_ti,
        Some(2),
        "option 2 is below the fold of a one-row box, so it scrolls to the top of the window"
    );
    assert_eq!(
        tall_ti, None,
        "and inside the first window of a sixty-point one — same selection, \
         same option list, different widget"
    );
}
