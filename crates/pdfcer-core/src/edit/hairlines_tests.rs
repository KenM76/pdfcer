#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic
)] // tests

use crate::document::Document;
use crate::edit::{CommandKind, EditSession};
use crate::preflight::hairlines::tests::fixture;
use crate::preflight::{HAIRLINES_NOT_COVERED, hairlines_in};
use crate::vector::{PathObject, VectorObject};
use crate::writer::SaveOptions;

fn session() -> EditSession {
    EditSession::new(Document::from_bytes(fixture()).unwrap())
}

fn path(o: &VectorObject) -> PathObject {
    match o {
        VectorObject::Path(p) => p.clone(),
        other => panic!("not a path: {other:?}"),
    }
}

#[test]
fn widens_each_hairline_to_exactly_the_minimum_in_one_undo() {
    let mut s = session();
    let depth = s.undo_depth();
    let report = s.fix_hairlines(&[0, 0], 0.25).unwrap();
    assert_eq!(report.changed, 4);
    assert_eq!(report.zero_width, 1);
    assert_eq!(report.per_page, vec![(0, 4)]);
    assert_eq!(report.forms.len(), 1);
    assert!(report.skipped.is_empty());
    assert!(
        report
            .disclosures
            .iter()
            .any(|d| d == HAIRLINES_NOT_COVERED)
    );
    assert!(
        report
            .disclosures
            .iter()
            .any(|d| d.contains("pdf dimensions"))
    );

    let objs = s.page_objects(0).unwrap();
    assert!(hairlines_in(&objs, 0.25).is_empty());
    assert!((path(&objs.objects[0]).line_width - 0.25).abs() < 1e-9);
    assert!((path(&objs.objects[1]).line_width - 0.5).abs() < 1e-9);
    assert!((path(&objs.objects[2]).line_width - 1.0).abs() < 1e-12);
    assert!((path(&objs.objects[3]).line_width - 0.25).abs() < 1e-9);
    assert!((path(&objs.leaves[0].object).line_width - 0.125).abs() < 1e-9);

    assert_eq!(s.undo_depth(), depth + 1);
    assert_eq!(s.undo(), Some(CommandKind::FixHairlines));
    assert_eq!(hairlines_in(&s.page_objects(0).unwrap(), 0.25).len(), 4);
}

#[test]
fn a_second_run_and_a_clean_page_change_nothing() {
    let mut s = session();
    s.fix_hairlines(&[0], 0.25).unwrap();
    let depth = s.undo_depth();
    let again = s.fix_hairlines(&[0], 0.25).unwrap();
    assert_eq!(again.changed, 0);
    assert_eq!(again.disclosures, vec![HAIRLINES_NOT_COVERED.to_owned()]);
    assert_eq!(s.undo_depth(), depth);
}

#[test]
fn untouched_objects_are_not_rewritten() {
    let original = fixture();
    let mut s = session();
    s.fix_hairlines(&[0], 0.25).unwrap();
    let saved = s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0;
    assert!(saved.starts_with(&original));
    let tail = String::from_utf8_lossy(&saved[original.len()..]).into_owned();
    for untouched in ["1 0 obj", "2 0 obj", "6 0 obj"] {
        assert!(!tail.contains(untouched), "{untouched} was rewritten");
    }
    let reopened = Document::from_bytes(saved).unwrap();
    let pages = crate::page_tree::pages(&reopened).unwrap();
    assert!(
        crate::preflight::hairlines(&reopened.view(), &pages[0], 0.25)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn refuses_a_bad_minimum_and_a_missing_page() {
    let mut s = session();
    for bad in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(s.fix_hairlines(&[0], bad).is_err());
    }
    assert!(matches!(
        s.fix_hairlines(&[3], 0.25),
        Err(crate::edit::EditError::PageOutOfRange { index: 3, count: 1 })
    ));
    assert_eq!(s.undo_depth(), 0);
}
