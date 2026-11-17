//! Grouping objects into a form XObject and ungrouping a form (pdfcer-gui
//! request G280) leave every pixel where it was.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::{CommandKind, EditError, EditSession};
use pdfcer_core::vector::{GroupError, ImageSource, VectorObject, is_group_form};
use pdfcer_core::writer::SaveOptions;
use pdfcer_render::render_page;

/// 0 blue square; 1 a thick mitred stroke under a rotating `cm`; 2 a green
/// square away from the rest; 3 a half-transparent grey square over 0, whose
/// bytes set the `gs` and fill 5 relies on; 4 text; 5 a square in 3's state.
const PAGE: &str = "0 0 1 rg 10 10 40 40 re f\n\
    q 0.866 0.5 -0.5 0.866 100 20 cm 1 0 0 RG 6 w 0 0 m 40 0 l 5 8 l S Q\n\
    0 1 0 rg 150 150 30 30 re f\n\
    /GS1 gs 0.5 g 20 20 40 40 re f\n\
    BT /F1 14 Tf 0 0 0 rg 20 120 Td (Group) Tj ET\n\
    60 150 20 20 re f\n";

/// A form whose `/BBox` cuts its second square in half, placed by a
/// `/Matrix`, with a `/GS1` of its own that is not the page's.
const FORM: &str = "/GS1 gs 1 0 0 rg 0 0 30 30 re f 0 0 1 rg 40 0 30 30 re f\n";
const FORM_PAGE: &str = "/GS1 gs 0 1 0 rg 10 10 30 30 re f\n\
    q 1 0 0 1 20 100 cm /Fm1 Do Q\n\
    0 0 0 rg 150 10 20 20 re f\n";

fn file(page: &str, resources: &str, extra: &[String]) -> Vec<u8> {
    let mut objs = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << /ExtGState \
             << /GS1 << /ca 0.5 >> >> /Font << /F1 5 0 R >> {resources} >> /Contents 4 0 R >>"
        ),
        format!("<< /Length {} >>\nstream\n{page}endstream", page.len()),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_owned(),
    ];
    objs.extend_from_slice(extra);
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

fn form_file(form_extra: &str) -> Vec<u8> {
    let form = format!(
        "<< /Type /XObject /Subtype /Form /BBox [0 0 55 30] /Matrix [1 0 0 1 5 5] \
         /Resources << /ExtGState << /GS1 << /ca 0.25 >> >> >> {form_extra} /Length {} \
         >>\nstream\n{FORM}endstream",
        FORM.len()
    );
    file(FORM_PAGE, "/XObject << /Fm1 6 0 R >>", &[form])
}

fn session(bytes: Vec<u8>) -> EditSession {
    EditSession::new(Document::from_bytes(bytes).unwrap())
}

/// The page as saved and reopened, at 2x.
fn pixels(s: &EditSession) -> Vec<u8> {
    let bytes = s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0;
    let doc = Document::from_bytes(bytes).unwrap();
    let pages = pdfcer_core::page_tree::pages(&doc).unwrap();
    render_page(&doc, &pages[0], 2.0)
        .unwrap()
        .pixmap
        .data()
        .to_vec()
}

fn assert_same(a: &[u8], b: &[u8], what: &str) {
    let differing = a.chunks(4).zip(b.chunks(4)).filter(|(x, y)| x != y).count();
    assert_eq!(differing, 0, "{what}: {differing} pixels differ");
}

#[test]
fn grouping_then_ungrouping_changes_no_pixel() {
    let mut s = session(file(PAGE, "", &[]));
    let before = pixels(&s);
    assert_eq!(s.page_objects(0).unwrap().objects.len(), 6);

    let made = s.group_objects(0, &[4, 0, 3, 1, 0]).unwrap();
    assert_eq!((made.index, made.members), (1, 4));
    assert_eq!(s.undo_kind(), Some(CommandKind::GroupObjects));
    let model = s.page_objects(0).unwrap();
    assert_eq!(model.objects.len(), 3);
    let VectorObject::Image(group) = &model.objects[1] else {
        panic!("the group is not a form: {:?}", model.objects[1]);
    };
    assert_eq!(group.source, ImageSource::Form);
    assert_eq!(group.xobject, Some(made.form));
    assert!(is_group_form(&s.graph(), made.form));
    assert_same(&before, &pixels(&s), "grouped");

    let out = s.ungroup_form(0, made.index).unwrap();
    assert_eq!(out.indices, 1..5);
    assert!(!out.clipped, "a pdfcer group ungroups without a clip");
    assert_eq!((out.form, out.renamed), (made.form, 0));
    assert_eq!(s.page_objects(0).unwrap().objects.len(), 6);
    assert_same(&before, &pixels(&s), "ungrouped");

    // The ungrouped objects are free to group again, and a group nests.
    let again = s.group_objects(0, &[1, 2, 3, 4]).unwrap();
    assert_eq!((again.index, again.members), (1, 4));
    let nested = s.group_objects(0, &[0, 1]).unwrap();
    assert_eq!((nested.index, nested.members), (0, 2));
    assert_same(&before, &pixels(&s), "regrouped and nested");
    let out = s.ungroup_form(0, 0).unwrap();
    assert_eq!(out.indices, 0..2);
    assert_same(&before, &pixels(&s), "outer group ungrouped");

    for _ in 0..5 {
        s.undo().unwrap();
    }
    assert!(!s.can_undo());
    assert_same(&before, &pixels(&s), "undone");
}

#[test]
fn an_imported_form_ungroups_under_its_matrix_clip_and_resources() {
    let mut s = session(form_file(""));
    let before = pixels(&s);
    let model = s.page_objects(0).unwrap();
    let VectorObject::Image(form) = &model.objects[1] else {
        panic!("object 1 is not the form");
    };
    let id = form.xobject.unwrap();
    assert!(!is_group_form(&s.graph(), id));

    let out = s.ungroup_form(0, 1).unwrap();
    assert!(out.clipped, "an imported form keeps its /BBox clip");
    assert_eq!(out.renamed, 1, "the form's /GS1 is not the page's");
    assert_eq!(out.indices.start, 1);
    assert_same(&before, &pixels(&s), "imported form ungrouped");
    assert_eq!(s.undo_kind(), Some(CommandKind::UngroupForm));
}

#[test]
fn forms_that_cannot_be_inlined_are_refused_by_name() {
    let refused = |extra: &str| {
        let mut s = session(form_file(extra));
        let err = s.ungroup_form(0, 1).unwrap_err();
        assert!(!s.can_undo());
        err
    };
    assert!(matches!(
        refused("/Group << /S /Transparency >>"),
        EditError::Group(GroupError::TransparencyGroup)
    ));
    assert!(matches!(
        refused("/OC << /Type /OCG /Name (L) >>"),
        EditError::Group(GroupError::OptionalContent)
    ));
    let mut s = session(form_file(""));
    assert!(matches!(
        s.ungroup_form(0, 0),
        Err(EditError::Group(GroupError::NotAForm { index: 0 }))
    ));
}

#[test]
fn a_member_under_another_clip_cannot_join() {
    let page = "0 0 1 rg 10 10 40 40 re f\n\
        q 0 0 100 100 re W n 1 0 0 rg 20 20 40 40 re f Q\n\
        0 1 0 rg 60 60 20 20 re f\n";
    let mut s = session(file(page, "", &[]));
    let n = s.page_objects(0).unwrap().objects.len();
    let err = s.group_objects(0, &[0, n - 2]).unwrap_err();
    assert!(
        matches!(err, EditError::Group(GroupError::MemberCannotJoin { .. })),
        "{err:?}"
    );
    assert!(matches!(
        s.group_objects(0, &[]),
        Err(EditError::Group(GroupError::NoObjects))
    ));
    assert!(!s.can_undo());
}

/// The leaves directly inside `form`, as page leaf indices.
fn leaves_in(s: &mut EditSession, form: pdfcer_core::object::ObjId) -> Vec<usize> {
    let model = s.page_objects(0).unwrap();
    (0..model.leaves.len())
        .filter(|&i| model.leaves[i].parent() == Some(form))
        .collect()
}

#[test]
fn a_group_inside_a_group_changes_no_pixel_and_dissolves_level_by_level() {
    let mut s = session(file(PAGE, "", &[]));
    let before = pixels(&s);
    let outer = s.group_objects(0, &[0, 1, 3, 4]).unwrap();
    let inside = leaves_in(&mut s, outer.form);
    assert_eq!(inside.len(), 4);

    let made = s
        .group_objects_in_form(0, &[inside[2], inside[0], inside[1]])
        .unwrap();
    assert_eq!((made.container, made.members), (outer.form, 3));
    assert_eq!((made.invocations, made.pages), (1, 1));
    assert!(made.disclosures.is_empty(), "{:?}", made.disclosures);
    assert_eq!(s.undo_kind(), Some(CommandKind::GroupObjects));
    assert!(is_group_form(&s.graph(), made.group));
    assert_same(&before, &pixels(&s), "grouped inside a group");

    let model = s.page_objects(0).unwrap();
    assert_eq!(
        model.objects.len(),
        3,
        "the page's own objects did not move"
    );
    assert_eq!(model.leaves.len(), 4);
    let nested = leaves_in(&mut s, made.group);
    assert_eq!(nested.len(), 3);
    assert_eq!(
        model.leaves[nested[0]].containment,
        vec![outer.form, made.group]
    );
    assert_eq!(leaves_in(&mut s, outer.form).len(), 1);

    // A third level, then back out one level at a time.
    let deeper = s.group_objects_in_form(0, &nested[..2]).unwrap();
    assert_eq!(deeper.container, made.group);
    assert_same(&before, &pixels(&s), "three levels");
    let leaf = leaves_in(&mut s, deeper.group)[0];
    let out = s.ungroup_form_in_form(0, leaf).unwrap();
    assert_eq!((out.form, out.container), (deeper.group, Some(made.group)));
    assert!(!out.clipped, "a pdfcer group ungroups without a clip");
    assert_eq!(s.undo_kind(), Some(CommandKind::UngroupForm));
    assert_same(&before, &pixels(&s), "inner level dissolved");

    let leaf = leaves_in(&mut s, made.group)[0];
    let out = s.ungroup_form_in_form(0, leaf).unwrap();
    assert_eq!((out.form, out.container), (made.group, Some(outer.form)));
    assert_eq!(leaves_in(&mut s, outer.form).len(), 4);
    assert_same(&before, &pixels(&s), "middle level dissolved");

    // The last level is the page's own ungroup.
    let leaf = leaves_in(&mut s, outer.form)[0];
    let out = s.ungroup_form_in_form(0, leaf).unwrap();
    assert_eq!((out.form, out.container), (outer.form, None));
    assert_eq!(s.page_objects(0).unwrap().objects.len(), 6);
    assert_same(&before, &pixels(&s), "outer level dissolved");

    for _ in 0..6 {
        s.undo().unwrap();
    }
    assert!(!s.can_undo());
    assert_same(&before, &pixels(&s), "undone");
}

#[test]
fn grouping_inside_an_imported_form_keeps_its_resources_and_says_its_reach() {
    // The form is painted twice, so one edit changes two places.
    let page = format!("{FORM_PAGE}q 1 0 0 1 100 150 cm /Fm1 Do Q\n");
    let form = format!(
        "<< /Type /XObject /Subtype /Form /BBox [0 0 55 30] /Matrix [1 0 0 1 5 5] \
         /Resources << /ExtGState << /GS1 << /ca 0.25 >> >> >> /Length {} \
         >>\nstream\n{FORM}endstream",
        FORM.len()
    );
    let mut s = session(file(&page, "/XObject << /Fm1 6 0 R >>", &[form]));
    let before = pixels(&s);
    let model = s.page_objects(0).unwrap();
    let VectorObject::Image(placed) = &model.objects[1] else {
        panic!("object 1 is not the form");
    };
    let id = placed.xobject.unwrap();
    let first: Vec<usize> = (0..model.leaves.len())
        .filter(|&i| model.leaves[i].paint_order == 1)
        .collect();
    assert_eq!(first.len(), 2);

    let made = s.group_objects_in_form(0, &first).unwrap();
    assert_eq!((made.container, made.members), (id, 2));
    assert_eq!((made.invocations, made.pages), (2, 1));
    assert_same(&before, &pixels(&s), "grouped inside an imported form");

    // Either invocation's leaf dissolves the one group both paint.
    let model = s.page_objects(0).unwrap();
    let other = (0..model.leaves.len())
        .rfind(|&i| model.leaves[i].parent() == Some(made.group))
        .unwrap();
    let out = s.ungroup_form_in_form(0, other).unwrap();
    assert_eq!((out.form, out.container), (made.group, Some(id)));
    assert_eq!((out.invocations, out.renamed), (2, 0));
    assert_same(&before, &pixels(&s), "ungrouped inside an imported form");
}

#[test]
fn nesting_stops_where_the_members_would_stop_being_editable() {
    let mut s = session(file(PAGE, "", &[]));
    let before = pixels(&s);
    let mut form = s.group_objects(0, &[0, 1]).unwrap().form;
    let mut depth = 1;
    let err = loop {
        let inside = leaves_in(&mut s, form);
        assert_eq!(
            inside.len(),
            2,
            "both members are still leaves at depth {depth}"
        );
        match s.group_objects_in_form(0, &inside) {
            Ok(made) => {
                form = made.group;
                depth += 1;
            }
            Err(err) => break err,
        }
        assert!(depth < 100, "nesting never stopped");
    };
    assert!(
        matches!(err, EditError::Group(GroupError::TooDeep { limit }) if limit == depth),
        "{err:?} at depth {depth}"
    );
    assert_same(&before, &pixels(&s), "nested to the limit");
}

#[test]
fn grouping_inside_a_form_refuses_before_anything_changes() {
    let mut s = session(form_file(""));
    assert!(matches!(
        s.group_objects_in_form(0, &[]),
        Err(EditError::Group(GroupError::NoObjects))
    ));
    assert!(matches!(
        s.group_objects_in_form(0, &[0, 99]),
        Err(EditError::FormLeafOutOfRange { index: 99, .. })
    ));
    assert!(matches!(
        s.ungroup_form_in_form(0, 99),
        Err(EditError::FormLeafOutOfRange { index: 99, .. })
    ));
    assert!(!s.can_undo());

    // A form with no /Resources of its own has nowhere to bind the group.
    let bare = "0 0 1 rg 0 0 30 30 re f 1 0 0 rg 40 0 10 10 re f\n";
    let form = format!(
        "<< /Type /XObject /Subtype /Form /BBox [0 0 55 30] /Length {} >>\nstream\n{bare}endstream",
        bare.len()
    );
    let mut s = session(file(FORM_PAGE, "/XObject << /Fm1 6 0 R >>", &[form]));
    assert!(matches!(
        s.group_objects_in_form(0, &[0, 1]),
        Err(EditError::FormInheritsResources { .. })
    ));
    assert!(!s.can_undo());
}
