#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test code: a failed lookup should fail the test loudly

use super::tests::{build, dict, id, only, session};
use super::*;
use crate::edit::EditSession;
use crate::graph::ObjectGraph;
use crate::object::{Dict, Object};

/// Two pages (3, 4). Elements: 6 marks content on page 2 by MCID; 7 has no
/// `/Pg` and reaches page 2 through its child 8's marked-content
/// reference; 9 has no kids, so no page; 11 reaches page 2 through an
/// object reference. Destinations naming them: the open action, link 12's
/// `/Dest`, link 13's GoTo `/D`, outline item 14's GoTo with only an `/SD`,
/// action 15 (a page `/D` and an `/SD`), the catalog's `/Dests` (16) and a
/// `/Dests` name-tree leaf (18).
fn sample() -> Vec<u8> {
    let b = |s: &str| s.as_bytes().to_vec();
    build(&[
        b(
            "<< /Type /Catalog /Pages 2 0 R /Outlines 5 0 R /StructTreeRoot 10 0 R /OpenAction [11 0 R /Fit] /Dests 16 0 R /Names << /Dests 17 0 R >> >>",
        ),
        b("<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>"),
        b("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Annots [12 0 R 13 0 R] >>"),
        b("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /AA << /O 15 0 R >> >>"),
        b("<< /Type /Outlines /First 14 0 R /Last 14 0 R /Count 1 >>"),
        b("<< /Type /StructElem /S /P /P 10 0 R /K 0 /Pg 4 0 R >>"),
        b("<< /Type /StructElem /S /Sect /P 10 0 R /K [8 0 R] >>"),
        b("<< /Type /StructElem /S /P /P 7 0 R /K << /Type /MCR /Pg 4 0 R /MCID 1 >> >>"),
        b("<< /Type /StructElem /S /Figure /P 10 0 R /K [] >>"),
        b("<< /Type /StructTreeRoot /K [6 0 R 7 0 R 9 0 R 11 0 R] >>"),
        b("<< /Type /StructElem /S /H1 /P 10 0 R /K << /Type /OBJR /Obj 12 0 R /Pg 4 0 R >> >>"),
        b("<< /Type /Annot /Subtype /Link /Rect [0 0 10 10] /Dest [6 0 R /XYZ 0 100 0] >>"),
        b("<< /Type /Annot /Subtype /Link /Rect [0 0 10 10] /A << /S /GoTo /D [9 0 R /Fit] >> >>"),
        b("<< /Title (Section) /Parent 5 0 R /A << /S /GoTo /SD [7 0 R /FitH 50] >> >>"),
        b("<< /S /GoTo /D [3 0 R /Fit] /SD [6 0 R /Fit] >>"),
        b("<< /one [6 0 R /Fit] /two << /D [3 0 R /Fit] /SD [6 0 R /Fit] >> >>"),
        b("<< /Kids [18 0 R] >>"),
        b("<< /Limits [(a) (a)] /Names [(a) << /D [7 0 R /Fit] >>] >>"),
    ])
}

fn untag(bytes: Vec<u8>) -> (EditSession, String) {
    let mut s = session(bytes);
    let tags = DiscardOptions::none().with_object(DiscardObject::Tags);
    let plan = s.plan_optimize(&only(tags)).unwrap();
    let location = plan.discard_objects[0].location.clone();
    s.apply_optimize(&plan).unwrap();
    (s, location)
}

/// The page number (object) a destination array names, and its length.
fn target(dest: &Object) -> (u32, usize) {
    let a = dest.as_array().unwrap();
    (a[0].as_reference().unwrap().num, a.len())
}

fn inline<'a>(d: &'a Dict, key: &[u8]) -> &'a Dict {
    d.get(key).and_then(Object::as_dict).unwrap()
}

#[test]
fn every_structure_destination_names_its_page_once_the_tags_go() {
    let (s, location) = untag(sample());
    assert_eq!(
        location,
        "catalog: 6 link(s) to a tag now go to its page, 1 of them to the first page (the tag had no page)"
    );
    let cat = dict(&s, 1);
    assert!(cat.get(b"StructTreeRoot").is_none());
    assert_eq!(target(cat.get(b"OpenAction").unwrap()), (4, 2), "via OBJR");
    assert_eq!(
        target(dict(&s, 12).get(b"Dest").unwrap()),
        (4, 5),
        "via MCID"
    );
    let a13 = dict(&s, 13);
    assert_eq!(
        target(inline(&a13, b"A").get(b"D").unwrap()),
        (3, 2),
        "no page: first"
    );
    let a14 = dict(&s, 14);
    let goto = inline(&a14, b"A");
    assert!(goto.get(b"SD").is_none());
    assert_eq!(target(goto.get(b"D").unwrap()), (4, 3), "/D built from /SD");
}

#[test]
fn a_structure_destination_keeps_its_operands() {
    let (s, _) = untag(sample());
    let d12 = dict(&s, 12);
    let dest = d12.get(b"Dest").and_then(Object::as_array).unwrap();
    assert_eq!(dest[1].as_name().unwrap().as_bytes(), b"XYZ");
    assert_eq!(dest[3].as_int(), Some(100));
}

#[test]
fn named_destinations_lose_structure_references_and_keep_their_page() {
    let (s, _) = untag(sample());
    let dests = dict(&s, 16);
    assert_eq!(target(dests.get(b"one").unwrap()), (4, 2));
    let two = inline(&dests, b"two");
    assert!(two.get(b"SD").is_none());
    assert_eq!(target(two.get(b"D").unwrap()), (3, 2), "a page /D is kept");
    let leaf = dict(&s, 18);
    let names = leaf.get(b"Names").and_then(Object::as_array).unwrap();
    assert_eq!(
        target(names[1].as_dict().unwrap().get(b"D").unwrap()),
        (4, 2)
    );
    let action = dict(&s, 15);
    assert!(action.get(b"SD").is_none());
    assert_eq!(target(action.get(b"D").unwrap()), (3, 2));
}

#[test]
fn an_inline_names_dictionary_and_tree_root_are_rewritten_in_place() {
    let text = String::from_utf8_lossy(&sample()).replace(
        "/Names << /Dests 17 0 R >>",
        "/Names << /Dests << /Names [(b) [9 0 R /Fit]] >> >>",
    );
    let (s, location) = untag(text.into_bytes());
    assert!(location.starts_with("catalog: 6 link(s)"), "{location}");
    let cat = dict(&s, 1);
    let root = inline(inline(&cat, b"Names"), b"Dests");
    let names = root.get(b"Names").and_then(Object::as_array).unwrap();
    assert_eq!(target(&names[1]), (3, 2));
}

#[test]
fn an_untagged_file_with_no_structure_destination_says_only_catalog() {
    let text = String::from_utf8_lossy(&sample())
        .replace(
            "/OpenAction [11 0 R /Fit] /Dests 16 0 R /Names << /Dests 17 0 R >>",
            "",
        )
        .replace("/Dest [6 0 R /XYZ 0 100 0]", "/Dest [3 0 R /Fit]")
        .replace("/D [9 0 R /Fit]", "/D [4 0 R /Fit]")
        .replace("/SD [7 0 R /FitH 50]", "/D [4 0 R /Fit]");
    let (s, location) = untag(text.into_bytes());
    assert_eq!(location, "catalog");
    assert_eq!(target(dict(&s, 12).get(b"Dest").unwrap()), (3, 2));
}

#[test]
fn a_cycle_in_the_structure_tree_ends_the_search() {
    let text = String::from_utf8_lossy(&sample()).replace(
        "/S /P /P 7 0 R /K << /Type /MCR /Pg 4 0 R /MCID 1 >>",
        "/S /P /P 7 0 R /K [7 0 R]",
    );
    let (s, _) = untag(text.into_bytes());
    let view = s.view();
    let leaf = view.value(id(18)).and_then(Object::as_dict).unwrap();
    let names = leaf.get(b"Names").and_then(Object::as_array).unwrap();
    assert_eq!(
        target(names[1].as_dict().unwrap().get(b"D").unwrap()),
        (3, 2)
    );
}

#[test]
fn an_indirect_goto_with_only_a_structure_destination_gains_a_page_one() {
    let text = String::from_utf8_lossy(&sample()).replace(
        "<< /S /GoTo /D [3 0 R /Fit] /SD [6 0 R /Fit] >>",
        "<< /S /GoTo /SD [6 0 R /Fit] >>",
    );
    let (s, _) = untag(text.into_bytes());
    let action = dict(&s, 15);
    assert!(action.get(b"SD").is_none());
    assert_eq!(target(action.get(b"D").unwrap()), (4, 2));
}
