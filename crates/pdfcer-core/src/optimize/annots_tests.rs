#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test code: a failed lookup should fail the test loudly

use super::tests::{build, dict, id, only, session};
use super::*;
use crate::doc_metadata::MetadataKind;
use crate::object::{ObjId, Object};
use crate::writer::SaveOptions;

/// One page (3) holding a comment (5), a text field's widget (6), a screen
/// annotation (7) that plays rendition 13, a 3D annotation (8) with its
/// pop-up (9), a link (10) that changes the 3D view and a web link (11).
/// The catalog has a form, a rendition list (12) and a structure tree whose
/// form element (15) references the widget.
fn sample() -> Vec<u8> {
    let b = |s: &str| s.as_bytes().to_vec();
    build(&[
        b(
            "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [6 0 R] >> /Names << /Renditions 12 0 R >> /StructTreeRoot 14 0 R >>",
        ),
        b("<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        b(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R /Annots [5 0 R 6 0 R 7 0 R 8 0 R 9 0 R 10 0 R 11 0 R] >>",
        ),
        b("<< /Length 0 >>\nstream\n\nendstream"),
        b("<< /Type /Annot /Subtype /Text /Rect [0 0 10 10] /Contents (Check this) >>"),
        b(
            "<< /Type /Annot /Subtype /Widget /FT /Tx /T (Name) /V (FIELD-VALUE) /Rect [20 0 60 10] /P 3 0 R /StructParent 0 >>",
        ),
        b(
            "<< /Type /Annot /Subtype /Screen /Rect [0 20 50 70] /A << /S /Rendition /OP 0 /AN 7 0 R /R 13 0 R >> >>",
        ),
        b("<< /Type /Annot /Subtype /3D /Rect [60 20 120 80] /3DD 16 0 R >>"),
        b("<< /Type /Annot /Subtype /Popup /Rect [60 90 120 120] /Parent 8 0 R >>"),
        b(
            "<< /Type /Annot /Subtype /Link /Rect [0 100 10 110] /A << /S /GoTo3DView /TA 8 0 R /V /F >> >>",
        ),
        b(
            "<< /Type /Annot /Subtype /Link /Rect [0 120 10 130] /A << /S /URI /URI (https://example.invalid/) >> >>",
        ),
        b("<< /Names [(clip) 13 0 R] >>"),
        b("<< /S /MR /C << /S /MCD /D (CLIP-FILE.mp4) >> >>"),
        b("<< /Type /StructTreeRoot /K 15 0 R >>"),
        b("<< /Type /StructElem /S /Form /P 14 0 R /K [<< /Type /OBJR /Obj 6 0 R >> 0] >>"),
        b("<< /Type /3D /Subtype /U3D /Length 8 >>\nstream\nMODEL-3D\nendstream"),
    ])
}

fn discarding(objects: &[DiscardObject]) -> DiscardOptions {
    objects
        .iter()
        .fold(DiscardOptions::none(), |o, &c| o.with_object(c))
}

fn annots(s: &crate::edit::EditSession) -> Vec<ObjId> {
    dict(s, 3)
        .get(b"Annots")
        .and_then(Object::as_array)
        .map(|a| a.iter().filter_map(Object::as_reference).collect())
        .unwrap_or_default()
}

const ALL_THREE: [DiscardObject; 3] = [
    DiscardObject::FormFields,
    DiscardObject::Multimedia,
    DiscardObject::ThreeDModels,
];

#[test]
fn each_class_is_listed_by_page_catalog_structure_and_action() {
    let s = session(sample());
    let plan = s.plan_optimize(&only(discarding(&ALL_THREE))).unwrap();
    let places: Vec<(&str, DiscardObject)> = plan
        .discard_objects
        .iter()
        .map(|i| (i.location.as_str(), i.object))
        .collect();
    assert_eq!(
        places,
        [
            (
                "page 1: form-fields (1), multimedia (1), 3d-models (2)",
                DiscardObject::FormFields
            ),
            ("catalog", DiscardObject::FormFields),
            ("catalog", DiscardObject::Multimedia),
            ("structure element 15 0", DiscardObject::FormFields),
            ("object 7 0: Rendition", DiscardObject::Multimedia),
            ("object 10 0: GoTo3DView", DiscardObject::ThreeDModels),
        ]
    );
    for item in &plan.discard_objects {
        assert!(item.bytes > 0, "{}", item.location);
    }
    for note in ["form fields go", "multimedia goes", "3D models go"] {
        assert!(plan.notes.iter().any(|n| n.contains(note)), "{note}");
    }
}

#[test]
fn one_class_leaves_the_others_alone() {
    let mut s = session(sample());
    let plan = s
        .plan_optimize(&only(discarding(&[DiscardObject::ThreeDModels])))
        .unwrap();
    s.apply_optimize(&plan).unwrap();
    assert_eq!(annots(&s), [id(5), id(6), id(7), id(10), id(11)]);
    assert!(dict(&s, 10).get(b"A").is_none(), "the 3D view action went");
    assert!(dict(&s, 7).get(b"A").is_some(), "the rendition stays");
    assert!(dict(&s, 1).get(b"AcroForm").is_some());
}

#[test]
fn everything_goes_and_undo_restores_it() {
    let mut s = session(sample());
    let plan = s.plan_optimize(&only(discarding(&ALL_THREE))).unwrap();
    s.apply_optimize(&plan).unwrap();
    assert_eq!(annots(&s), [id(5), id(10), id(11)]);
    let cat = dict(&s, 1);
    assert!(cat.get(b"AcroForm").is_none());
    assert!(
        cat.get(b"Names").is_none(),
        "the emptied name dictionary went"
    );
    let form = dict(&s, 15);
    let kids = form.get(b"K").and_then(Object::as_array).unwrap();
    assert_eq!(
        (kids.len(), &kids[0]),
        (1, &Object::Integer(0)),
        "marked content stays"
    );
    assert!(dict(&s, 11).get(b"A").is_some(), "the web link stays");
    s.undo();
    assert_eq!(annots(&s).len(), 7);
    assert!(dict(&s, 1).get(b"AcroForm").is_some());
}

#[test]
fn the_saved_file_holds_none_of_them() {
    let mut s = session(sample());
    let plan = s.plan_optimize(&only(discarding(&ALL_THREE))).unwrap();
    s.apply_optimize(&plan).unwrap();
    let (file, _) = s
        .save_optimized(&plan, &SaveOptions::default(), &RebuildOptions::none())
        .unwrap();
    let bytes = match file {
        OptimizedFile::Smaller(b) | OptimizedFile::NotSmaller { bytes: b, .. } => b,
        other => panic!("expected a written file, got {other:?}"),
    };
    let text = String::from_utf8_lossy(&bytes);
    for gone in [
        "FIELD-VALUE",
        "/Widget",
        "/Screen",
        "/Popup",
        "CLIP-FILE",
        "MODEL-3D",
        "/OBJR",
    ] {
        assert!(!text.contains(gone), "{gone} survived");
    }
    assert!(text.contains("Check this") && text.contains("example.invalid"));
}

#[test]
fn a_comment_discard_in_the_same_run_does_not_stale_the_list() {
    let mut s = session(sample());
    let mut options = discarding(&[DiscardObject::FormFields]);
    options = options.with(MetadataKind::Comment);
    let plan = s.plan_optimize(&only(options)).unwrap();
    s.apply_optimize(&plan).unwrap();
    assert_eq!(annots(&s), [id(7), id(8), id(9), id(10), id(11)]);
}
