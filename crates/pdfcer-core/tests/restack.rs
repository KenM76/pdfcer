//! Restacking objects in paint order (pdfcer-gui request G157): each moved
//! object keeps the graphics state it was painted under, and everything else
//! keeps its own.

use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditError, EditSession};
use pdfcer_core::object::Object;
use pdfcer_core::vector::{
    Matrix, RestackLimitReason, StackMove, VectorEditError, VectorObject, decompose_page,
};
use pdfcer_core::writer::SaveOptions;

/// 0 blue square; 1 green/red stroked dashed square (2 w); 2 yellow square
/// under a translating `cm`; 3 grey square under `/GS1` (`/ca 0.5`); 4 text
/// whose `Tf` and red fill sit inside its `BT … ET`; 5 text relying on them;
/// 6 the clip path `re W n` itself (it paints nothing); 7 a square under
/// that clip; 8 a blue square after the clip ends.
const PAGE: &str = "1 0 0 RG 0 0 1 rg\n10 10 30 30 re f\n\
    0 1 0 rg 2 w [3 1] 0 d\n20 20 30 30 re B\n\
    q 1 0 0 1 50 0 cm 1 1 0 rg\n0 0 20 20 re f\nQ\n\
    /GS1 gs 0.5 g\n15 15 10 10 re f\n\
    BT /F1 12 Tf 1 0 0 rg 10 80 Td (A) Tj ET\n\
    BT 10 60 Td (B) Tj ET\n\
    q 0 0 40 40 re W n\n0 0 0 rg 5 5 50 50 re f\nQ\n\
    0 0 1 rg 70 70 10 10 re f\n";

fn fixture() -> Vec<u8> {
    fixture_with(PAGE, "/GS1 << /ca 0.5 >>")
}

/// A one-page file with content `page` and `/ExtGState` entries `gs`.
fn fixture_with(page: &str, gs: &str) -> Vec<u8> {
    let objs = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Resources << /ExtGState \
             << {gs} >> /Font << /F1 5 0 R >> >> /Contents 4 0 R >>"
        ),
        format!("<< /Length {} >>\nstream\n{page}endstream", page.len()),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_owned(),
    ];
    let mut buf = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objs.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref_at = buf.len();
    let size = offsets.len() + 1;
    buf.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f \n").as_bytes());
    for off in &offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n")
            .as_bytes(),
    );
    buf
}

fn session() -> EditSession {
    EditSession::new(Document::from_bytes(fixture()).unwrap())
}

/// Everything about an object that its state decides, and none of where its
/// bytes are.
fn sig(o: &VectorObject) -> String {
    let b = o.page_bbox();
    let bbox = format!(
        "{:.3},{:.3},{:.3},{:.3}",
        b.min.x, b.min.y, b.max.x, b.max.y
    );
    match o {
        VectorObject::Path(p) => format!(
            "path {bbox} fill={:?} stroke={:?} w={} dash={:?} ca={} CA={} {:?}",
            p.fill_color,
            p.stroke_color,
            p.line_width,
            p.dash,
            p.fill_alpha,
            p.stroke_alpha,
            p.style
        ),
        VectorObject::Text(t) => format!("text {bbox} font={:?}", t.font),
        VectorObject::Image(i) => format!("image {bbox} ca={}", i.fill_alpha),
    }
}

/// The page as saved and reopened, decomposed independently of the session.
fn reopened(s: &EditSession) -> Vec<String> {
    let bytes = s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0;
    let doc = Document::from_bytes(bytes).unwrap();
    let pages = pdfcer_core::page_tree::pages(&doc).unwrap();
    decompose_page(&doc.view(), &pages[0], Matrix::IDENTITY)
        .unwrap()
        .objects
        .iter()
        .map(sig)
        .collect()
}

fn page_content(s: &EditSession) -> String {
    let bytes = s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0;
    let doc = Document::from_bytes(bytes).unwrap();
    let pages = pdfcer_core::page_tree::pages(&doc).unwrap();
    let content_ref = Object::Reference(pages[0].contents[0]);
    let Object::Stream(st) = doc.resolve(&content_ref) else {
        panic!("no content stream");
    };
    let raw = st.data_span.slice(doc.bytes()).unwrap();
    String::from_utf8_lossy(&pdfcer_core::filters::decode_stream(&st.dict, raw).unwrap())
        .into_owned()
}

/// `before` with the objects at `from` moved to positions `to`.
fn expected(before: &[String], from: &[usize], to: &[usize]) -> Vec<String> {
    let mut rest: Vec<String> = before
        .iter()
        .enumerate()
        .filter(|(i, _)| !from.contains(i))
        .map(|(_, s)| s.clone())
        .collect();
    let mut placed: Vec<(usize, String)> = from
        .iter()
        .zip(to)
        .map(|(&f, &t)| (t, before[f].clone()))
        .collect();
    placed.sort();
    for (t, s) in placed {
        rest.insert(t, s);
    }
    rest
}

fn check(how: StackMove, from: &[usize], to: &[usize]) {
    let mut s = session();
    let before = reopened(&s);
    let out = s.restack_objects(0, from, how).unwrap();
    assert_eq!(out.indices, to, "{how:?} {out:?}");
    assert!(out.limited.is_empty(), "{out:?}");
    let after = reopened(&s);
    assert_eq!(after, expected(&before, from, to), "{}", page_content(&s));
}

#[test]
fn the_fixture_decomposes_as_documented() {
    let sigs = reopened(&session());
    assert_eq!(sigs.len(), 9, "{sigs:#?}");
    assert!(sigs[4].starts_with("text") && sigs[5].starts_with("text"));
    assert!(sigs[3].contains("ca=0.5"), "{}", sigs[3]);
    assert!(sigs[5].contains("F1"), "{}", sigs[5]);
}

#[test]
fn to_front_keeps_every_objects_state() {
    check(StackMove::Front, &[0], &[8]);
    check(StackMove::Front, &[3], &[8]);
}

#[test]
fn to_front_past_a_gs_undoes_it_with_a_new_ext_gstate() {
    // 0 was painted before `/GS1 gs` set `/ca 0.5`; on top it must still be
    // opaque, and 8, painted after it, must keep its own state.
    let mut s = session();
    s.restack_objects(0, &[0], StackMove::Front).unwrap();
    let content = page_content(&s);
    assert!(content.contains("/pdfcerRS1 gs"), "{content}");
    let after = reopened(&s);
    assert!(after[8].contains("ca=1"), "{}", after[8]);
}

#[test]
fn to_front_restores_a_value_from_the_shared_gs_history() {
    // 0 strokes at `/LW 3` from GS2; GS3 then sets `/LW 5` and overprint,
    // which the reset must take back to 3 and the page-start `false`.
    let page = "/GS2 gs 0 0 10 10 re S\n/GS3 gs 20 20 10 10 re S\n";
    let gs = "/GS2 << /LW 3 >> /GS3 << /LW 5 /OP true >>";
    let mut s = EditSession::new(Document::from_bytes(fixture_with(page, gs)).unwrap());
    let before = reopened(&s);
    assert!(
        before[0].contains("w=3") && before[1].contains("w=5"),
        "{before:?}"
    );
    let out = s.restack_objects(0, &[0], StackMove::Front).unwrap();
    assert_eq!(out.indices, vec![1]);
    assert!(out.limited.is_empty(), "{out:?}");
    let content = page_content(&s);
    assert_eq!(reopened(&s), expected(&before, &[0], &[1]), "{content}");
    assert!(content.contains("/pdfcerRS1 gs"), "{content}");
}

#[test]
fn to_back_carries_gs_and_colour_and_leaves_the_text_state_behind() {
    // Text 4 sets the font and fill text 5 relies on; moving it must leave
    // them in force for 5.
    check(StackMove::Back, &[4], &[0]);
    check(StackMove::Back, &[2], &[0]);
}

#[test]
fn several_objects_keep_their_relative_order() {
    check(StackMove::Front, &[0, 1], &[7, 8]);
    check(StackMove::Back, &[5, 3], &[1, 0]);
}

#[test]
fn forward_and_backward_step_past_the_nearest_overlapping_object() {
    // 0 (10..40) overlaps 1 (20..50); 3 (15..25) overlaps 1 below it.
    check(StackMove::Forward, &[0], &[1]);
    check(StackMove::Backward, &[3], &[1]);
}

#[test]
fn nothing_to_do_commits_nothing() {
    let mut s = session();
    let before = page_content(&s);
    // 8 is already on top; nothing is below 0.
    let out = s.restack_objects(0, &[8], StackMove::Front).unwrap();
    assert_eq!(out.indices, vec![8]);
    assert!(out.moved.is_empty() && out.limited.is_empty(), "{out:?}");
    let out = s.restack_objects(0, &[0], StackMove::Backward).unwrap();
    assert!(out.moved.is_empty() && out.limited.is_empty(), "{out:?}");
    assert_eq!(page_content(&s), before);
    assert!(!s.can_undo());
}

#[test]
fn a_clipped_object_cannot_leave_its_clip() {
    let mut s = session();
    let before = page_content(&s);
    let out = s.restack_objects(0, &[7], StackMove::Front).unwrap();
    assert!(out.moved.is_empty(), "{out:?}");
    assert_eq!(out.indices, vec![7]);
    assert_eq!(out.limited.len(), 1);
    assert_eq!(out.limited[0].object, 7);
    assert_eq!(out.limited[0].reason, RestackLimitReason::Scope);
    assert_eq!(page_content(&s), before);
}

#[test]
fn one_undo_restores_the_page() {
    let mut s = session();
    let before = page_content(&s);
    s.restack_objects(0, &[0, 4], StackMove::Front).unwrap();
    assert_ne!(page_content(&s), before);
    s.undo().unwrap();
    assert_eq!(page_content(&s), before);
}

#[test]
fn a_bad_index_is_refused() {
    let err = session()
        .restack_objects(0, &[0, 99], StackMove::Back)
        .unwrap_err();
    assert!(
        matches!(
            err,
            EditError::VectorEdit(VectorEditError::ObjectOutOfRange { index: 99, .. })
        ),
        "{err:?}"
    );
}
