#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test code: a failed lookup should fail the test loudly

use super::tests::{build, dict, id, only, session};
use super::*;
use crate::graph::ObjectGraph;
use crate::object::Object;
use crate::writer::SaveOptions;

/// A page (3) with a web link (11); a structure tree (5) whose class map
/// holds a user-properties-only class and a mixed one; a figure (7) whose
/// `/A` is an indirect user-properties object (10); a figure (8) whose `/A`
/// array mixes layout and user properties with revision numbers; and a
/// paragraph (9) with layout only, holding an inline span whose user
/// properties are written inside it.
fn sample() -> Vec<u8> {
    let b = |s: &str| s.as_bytes().to_vec();
    build(&[
        b(
            "<< /Type /Catalog /Pages 2 0 R /StructTreeRoot 5 0 R /MarkInfo << /Marked true /UserProperties true >> >>",
        ),
        b("<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        b(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R /Annots [11 0 R] >>",
        ),
        b("<< /Length 0 >>\nstream\n\nendstream"),
        b(
            "<< /Type /StructTreeRoot /K 6 0 R /ClassMap << /Shape << /O /UserProperties /P [<< /N (Part) /V (A-100) >>] >> /Mixed [<< /O /Layout /Placement /Block >> << /O /UserProperties /P [<< /N (Bin) /V (C-300) >>] >> 3] >> >>",
        ),
        b("<< /Type /StructElem /S /Document /P 5 0 R /K [7 0 R 8 0 R 9 0 R] >>"),
        b("<< /Type /StructElem /S /Figure /P 6 0 R /A 10 0 R /C /Shape >>"),
        b(
            "<< /Type /StructElem /S /Figure /P 6 0 R /A [<< /O /Layout /Placement /Block >> 0 << /O /UserProperties /P [<< /N (Mass) /V 2.5 >>] >> 4] >>",
        ),
        b(
            "<< /Type /StructElem /S /P /P 6 0 R /A << /O /Layout /TextAlign /Center >> /K [0 << /S /Span /A << /O /UserProperties /P [<< /N (Inline) /V (D-400) >>] >> /K 1 >>] >>",
        ),
        b("<< /O /UserProperties /P [<< /N (Part) /V (B-200) >>] >>"),
        b(
            "<< /Type /Annot /Subtype /Link /Rect [0 0 10 10] /A << /S /URI /URI (https://example.invalid/) >> >>",
        ),
    ])
}

fn data() -> DiscardOptions {
    DiscardOptions::none().with_object(DiscardObject::ObjectData)
}

#[test]
fn every_holder_of_user_properties_is_listed_and_measured() {
    let s = session(sample());
    let plan = s.plan_optimize(&only(data())).unwrap();
    let places: Vec<&str> = plan
        .discard_objects
        .iter()
        .map(|i| i.location.as_str())
        .collect();
    assert_eq!(
        places,
        [
            "class map",
            "structure element 7 0",
            "structure element 8 0",
            "structure element 9 0",
            "catalog"
        ]
    );
    for item in &plan.discard_objects {
        assert_eq!(item.object, DiscardObject::ObjectData);
        assert!(item.bytes > 0, "{}", item.location);
    }
    assert!(plan.notes.iter().any(|n| n.contains("object data goes")));

    let with_tags = data().with_object(DiscardObject::Tags);
    let plan = s.plan_optimize(&only(with_tags)).unwrap();
    assert!(
        plan.discard_objects
            .iter()
            .all(|i| i.object == DiscardObject::Tags),
        "the tags discard takes the object data with it"
    );
}

#[test]
fn user_properties_go_and_every_other_attribute_stays() {
    let mut s = session(sample());
    let root_before = dict(&s, 5);
    let plan = s.plan_optimize(&only(data())).unwrap();
    assert_eq!(s.apply_optimize(&plan).unwrap(), 5);

    let root = dict(&s, 5);
    let map = root.get(b"ClassMap").and_then(Object::as_dict).unwrap();
    assert!(map.get(b"Shape").is_none());
    let mixed = map.get(b"Mixed").and_then(Object::as_array).unwrap();
    assert_eq!(mixed.len(), 1, "the revision number went with it");
    assert!(dict(&s, 7).get(b"A").is_none());
    assert_eq!(
        dict(&s, 7)
            .get(b"C")
            .and_then(Object::as_name)
            .unwrap()
            .as_bytes(),
        b"Shape"
    );
    let a = dict(&s, 8);
    let a = a.get(b"A").and_then(Object::as_array).unwrap();
    assert_eq!(a.len(), 2, "layout and its revision stay");
    assert_eq!(a[1], Object::Integer(0));
    let para = dict(&s, 9);
    assert!(para.get(b"A").is_some());
    let kids = para.get(b"K").and_then(Object::as_array).unwrap();
    assert_eq!(kids[0], Object::Integer(0), "marked content stays");
    let span = kids[1].as_dict().unwrap();
    assert!(span.get(b"A").is_none() && span.get(b"K").is_some());
    assert!(
        dict(&s, 11).get(b"A").is_some(),
        "an action is not an attribute"
    );
    assert!(s.view().value(id(10)).is_none(), "object 10 survived");
    let info = dict(&s, 1);
    let info = info.get(b"MarkInfo").and_then(Object::as_dict).unwrap();
    assert!(info.get(b"UserProperties").is_none() && info.get(b"Marked").is_some());

    s.undo();
    assert_eq!(dict(&s, 5), root_before);
    assert!(dict(&s, 7).get(b"A").is_some());
}

#[test]
fn the_saved_file_holds_no_user_properties() {
    let mut s = session(sample());
    let plan = s.plan_optimize(&only(data())).unwrap();
    s.apply_optimize(&plan).unwrap();
    let (file, _) = s
        .save_optimized(&plan, &SaveOptions::default(), &RebuildOptions::none())
        .unwrap();
    let bytes = match file {
        OptimizedFile::Smaller(b) | OptimizedFile::NotSmaller { bytes: b, .. } => b,
        other => panic!("expected a written file, got {other:?}"),
    };
    let text = String::from_utf8_lossy(&bytes);
    for gone in ["UserProperties", "A-100", "B-200", "C-300", "D-400", "Mass"] {
        assert!(!text.contains(gone), "{gone} survived");
    }
    assert!(text.contains("/Layout") && text.contains("/Marked true"));
}

#[test]
fn a_file_without_a_structure_tree_has_no_object_data() {
    let text = String::from_utf8_lossy(&sample()).replace("/StructTreeRoot 5 0 R ", "");
    let s = session(text.into_bytes());
    let plan = s.plan_optimize(&only(data())).unwrap();
    assert!(plan.discard_objects.is_empty());
}
