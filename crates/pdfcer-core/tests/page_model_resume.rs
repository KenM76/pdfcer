//! After an edit confined to later `/Contents` streams, `page_objects`
//! resumes the previous decomposition (request G140). Whatever it reuses, the
//! model must equal a fresh `decompose_page` of the edited page.

use crate::shared_page_content_edit::{assemble, stream};
use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::vector::{Matrix, decompose_page};

/// One page drawing `parts` as separate `/Contents` streams (objects 5..),
/// with form `/Fm0` (object 4) available to them.
fn page_of(parts: &[&str]) -> EditSession {
    let refs: Vec<String> = (0..parts.len()).map(|i| format!("{} 0 R", i + 5)).collect();
    let mut bodies = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 200] \
             /Resources << /XObject << /Fm0 4 0 R >> >> /Contents [{}] >>",
            refs.join(" ")
        ),
        "<< /Type /XObject /Subtype /Form /BBox [0 0 50 50] /Length 13 >>\nstream\n\
         0 0 m 9 9 l S\nendstream"
            .to_owned(),
    ];
    bodies.extend(parts.iter().map(|p| stream(p)));
    EditSession::new(Document::from_bytes(assemble(&bodies)).expect("fixture loads"))
}

/// The session's model of page 0 equals a fresh decomposition of it.
fn assert_fresh(s: &mut EditSession) {
    let got = s.page_objects(0).expect("page_objects");
    let pages = s.pages().expect("pages");
    let want = decompose_page(&s.view(), &pages[0], Matrix::IDENTITY).expect("decompose");
    assert_eq!(*got, want);
}

fn last(s: &mut EditSession) -> usize {
    s.page_objects(0).expect("page_objects").objects.len() - 1
}

#[test]
fn moving_in_the_last_stream_matches_a_full_rebuild() {
    let mut s = page_of(&[
        "q 2 0 0 2 0 0 cm /Fm0 Do 1 0 0 RG",
        "0 0 m 50 0 l S BT 10 10 Td ET",
        "5 5 m 60 5 l S",
    ]);
    assert_fresh(&mut s);
    for dx in [10.0, -3.0] {
        let i = last(&mut s);
        s.move_objects(0, &[i], dx, 1.0).expect("move");
        assert_fresh(&mut s);
    }
    s.undo();
    assert_fresh(&mut s);
    s.redo();
    assert_fresh(&mut s);
    s.move_objects(0, &[1], 4.0, 0.0)
        .expect("move in the middle stream");
    assert_fresh(&mut s);
}

#[test]
fn a_stream_boundary_inside_an_object_or_operation_still_matches() {
    let mut s = page_of(&[
        "BT 10 10 Td",
        "ET 1 0 0 1 5 5",
        "cm 0 0 m 9 9 l S",
        "/Fm0 Do",
        "20 20 m 30 30 l S",
    ]);
    assert_fresh(&mut s);
    let i = last(&mut s);
    s.move_objects(0, &[i], 2.0, 2.0).expect("move");
    assert_fresh(&mut s);
}

#[test]
fn an_edit_outside_the_page_contents_rebuilds() {
    let mut s = page_of(&["/Fm0 Do", "20 20 m 30 30 l S"]);
    assert_fresh(&mut s);
    s.move_objects_in_form(0, &[0], 3.0, 0.0)
        .expect("move inside the form");
    let i = last(&mut s);
    s.move_objects(0, &[i], 1.0, 0.0).expect("move");
    assert_fresh(&mut s);
}

#[test]
fn a_model_still_held_by_the_caller_is_left_intact() {
    let mut s = page_of(&["0 0 m 50 0 l S", "5 5 m 60 5 l S"]);
    let held = s.page_objects(0).expect("page_objects");
    let before = (*held).clone();
    let i = last(&mut s);
    s.move_objects(0, &[i], 10.0, 0.0).expect("move");
    assert_fresh(&mut s);
    assert_eq!(*held, before);
}
