#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test code: a failed lookup should fail the test loudly

use super::tests::{build, only, session};
use super::*;
use crate::edit::EditSession;
use crate::graph::ObjectGraph;
use crate::object::{ObjId, Object};
use crate::writer::SaveOptions;

/// One page (3) drawing an image (5) whose print alternate is 6; one
/// bookmark (7, under 8) naming a structure element; a structure tree (10)
/// with one element (9) and a parent tree (11); a link (12) whose GoTo names
/// the element and one (13) whose GoToR names one in another file; print and
/// display preferences.
fn sample() -> Vec<u8> {
    let b = |s: &str| s.as_bytes().to_vec();
    let image = |marker: &str, extra: &str| {
        let data = marker.repeat(20);
        format!(
            "<< /Type /XObject /Subtype /Image /Width 10 /Height 10 /ColorSpace /DeviceGray /BitsPerComponent 8 {extra} /Length {} >>\nstream\n{data}\nendstream",
            data.len()
        )
        .into_bytes()
    };
    build(&[
        b(
            "<< /Type /Catalog /Pages 2 0 R /Outlines 8 0 R /PageMode /UseOutlines /StructTreeRoot 10 0 R /MarkInfo << /Marked true >> /ViewerPreferences << /PrintScaling /None /Enforce [/PrintScaling] /Duplex /Simplex /NonFullScreenPageMode /UseOutlines /HideToolbar true >> >>",
        ),
        b("<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        b(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << /XObject << /Im1 5 0 R >> >> /Contents 4 0 R /StructParents 0 /Annots [12 0 R 13 0 R] >>",
        ),
        b("<< /Length 28 >>\nstream\nq 10 0 0 10 0 0 cm /Im1 Do Q\nendstream"),
        image(
            "x",
            "/Alternates [<< /Image 6 0 R /DefaultForPrinting true >>]",
        ),
        image("ALTPRINTDATA", ""),
        b("<< /Title (BOOKMARKTITLE) /Parent 8 0 R /Dest [3 0 R /Fit] /SE 9 0 R >>"),
        b("<< /Type /Outlines /First 7 0 R /Last 7 0 R /Count 1 >>"),
        b("<< /Type /StructElem /S /P /P 10 0 R /K 0 /Pg 3 0 R /T (STRUCTMARK) >>"),
        b("<< /Type /StructTreeRoot /K 9 0 R /ParentTree 11 0 R >>"),
        b("<< /Nums [0 [9 0 R]] >>"),
        b(
            "<< /Type /Annot /Subtype /Link /Rect [0 0 10 10] /StructParent 1 /A << /S /GoTo /D [3 0 R /Fit] /SD [9 0 R] >> >>",
        ),
        b(
            "<< /Type /Annot /Subtype /Link /Rect [0 0 10 10] /A << /S /GoToR /F (other.pdf) /D [0 /Fit] /SD [(remote) /Fit] >> >>",
        ),
    ])
}

fn every_object() -> DiscardOptions {
    DiscardObject::ALL
        .into_iter()
        .fold(DiscardOptions::none(), DiscardOptions::with_object)
}

fn classes(plan: &OptimizePlan) -> Vec<DiscardObject> {
    plan.discard_objects.iter().map(|i| i.object).collect()
}

fn dict_keys(s: &EditSession, id: ObjId) -> Vec<String> {
    let view = s.view();
    let dict = match view.value(id) {
        Some(Object::Dict(d)) => d,
        Some(Object::Stream(x)) => &x.dict,
        _ => panic!("object {id:?} is not a dictionary"),
    };
    dict.iter()
        .map(|(k, _)| String::from_utf8_lossy(k.as_bytes()).into_owned())
        .collect()
}

fn id(num: u32) -> ObjId {
    ObjId::new(num, 0)
}

#[test]
fn a_plan_lists_only_the_selected_classes_each_measured() {
    let s = session(sample());
    for object in DiscardObject::ALL {
        let plan = s
            .plan_optimize(&only(DiscardOptions::none().with_object(object)))
            .unwrap();
        assert_eq!(classes(&plan), [object]);
        assert!(plan.discard_objects[0].bytes > 0, "{object}");
    }
    let screen = s.plan_optimize(&Preset::Screen.settings()).unwrap();
    assert_eq!(classes(&screen), [DiscardObject::AlternateImages]);
    assert!(
        s.plan_optimize(&Preset::Standard.settings())
            .unwrap()
            .discard_objects
            .is_empty()
    );
}

#[test]
fn discarding_tags_is_disclosed() {
    let s = session(sample());
    let tags = DiscardOptions::none().with_object(DiscardObject::Tags);
    let plan = s.plan_optimize(&only(tags)).unwrap();
    assert!(
        plan.notes.iter().any(|n| n.contains("no longer tagged")),
        "{:?}",
        plan.notes
    );
}

#[test]
fn apply_removes_every_carrier_as_one_undo_and_undo_restores_them() {
    let mut s = session(sample());
    let catalog_before = dict_keys(&s, id(1));
    let plan = s.plan_optimize(&only(every_object())).unwrap();
    assert_eq!(s.apply_optimize(&plan).unwrap(), 4);
    let catalog = dict_keys(&s, id(1));
    for gone in ["Outlines", "PageMode", "StructTreeRoot", "MarkInfo"] {
        assert!(!catalog.contains(&gone.to_owned()), "{gone} in {catalog:?}");
    }
    assert!(!dict_keys(&s, id(3)).contains(&"StructParents".to_owned()));
    assert!(!dict_keys(&s, id(5)).contains(&"Alternates".to_owned()));
    let view = s.view();
    let prefs = view
        .value(id(1))
        .and_then(Object::as_dict)
        .and_then(|c| c.get(b"ViewerPreferences"))
        .and_then(Object::as_dict)
        .unwrap();
    for gone in [
        b"PrintScaling".as_slice(),
        b"Enforce",
        b"Duplex",
        b"NonFullScreenPageMode",
    ] {
        assert!(
            prefs.get(gone).is_none(),
            "{}",
            String::from_utf8_lossy(gone)
        );
    }
    assert!(
        prefs.get(b"HideToolbar").is_some(),
        "display preferences stay"
    );
    assert_eq!(s.undo_depth(), 1);
    s.undo();
    assert_eq!(dict_keys(&s, id(1)), catalog_before);
    assert!(dict_keys(&s, id(5)).contains(&"Alternates".to_owned()));
}

#[test]
fn the_saved_file_no_longer_holds_the_discarded_objects() {
    let mut s = session(sample());
    let plan = s.plan_optimize(&only(every_object())).unwrap();
    s.apply_optimize(&plan).unwrap();
    let (file, _) = s
        .save_optimized(&plan, &SaveOptions::default(), &RebuildOptions::all())
        .unwrap();
    let OptimizedFile::Smaller(bytes) = file else {
        panic!("expected a smaller file");
    };
    // Rebuilt into object streams, so read the file back rather than grep it.
    let saved = session(bytes);
    let view = saved.view();
    let catalog_id = view
        .trailer_entry(b"Root")
        .and_then(Object::as_reference)
        .unwrap();
    let catalog = view.value(catalog_id).and_then(Object::as_dict).unwrap();
    for gone in [b"Outlines".as_slice(), b"StructTreeRoot", b"MarkInfo"] {
        assert!(
            catalog.get(gone).is_none(),
            "{} survived",
            String::from_utf8_lossy(gone)
        );
    }
    let prefs = catalog.get(b"ViewerPreferences").map(|p| view.resolve(p));
    let prefs = prefs.and_then(Object::as_dict).unwrap();
    assert!(prefs.get(b"PrintScaling").is_none() && prefs.get(b"HideToolbar").is_some());
    // Catalog, pages, page, content, image and the two links survive; the
    // rest are the rebuild's object stream and cross-reference stream.
    let kept = (1..=40).filter(|n| view.value(id(*n)).is_some()).count();
    assert_eq!(kept, 9);
}

#[test]
fn a_discarded_bookmark_tree_is_not_also_listed_as_broken() {
    let text =
        String::from_utf8_lossy(&sample()).replace("/Dest [3 0 R /Fit]", "/Dest [99 0 R /Fit]");
    let s = session(text.into_bytes());
    let mut settings = only(DiscardOptions::none().with_object(DiscardObject::Bookmarks));
    settings.cleanup.invalid_bookmarks = true;
    let plan = s.plan_optimize(&settings).unwrap();
    assert_eq!(classes(&plan), [DiscardObject::Bookmarks]);
    assert!(plan.cleanup.bookmarks.is_empty());
}

#[test]
fn names_cover_kinds_and_objects() {
    let known: Vec<&str> = DiscardOptions::known_names().collect();
    assert!(known.contains(&"thumbnail") && known.contains(&"alternate-images"));
    assert_eq!(known.last(), Some(&"all"));
    let d = DiscardOptions::none()
        .with_named("bookmarks")
        .and_then(|d| d.with_named("info"))
        .unwrap();
    assert_eq!(d.names().collect::<Vec<_>>(), ["info", "bookmarks"]);
    assert_eq!(DiscardOptions::all().names().count(), known.len() - 1);
}

#[test]
fn untagging_keeps_bookmarks_and_remote_links_but_drops_their_structure_references() {
    let mut s = session(sample());
    let tags = DiscardOptions::none().with_object(DiscardObject::Tags);
    let plan = s.plan_optimize(&only(tags)).unwrap();
    s.apply_optimize(&plan).unwrap();
    let outline = dict_keys(&s, id(7));
    assert!(outline.contains(&"Dest".to_owned()) && !outline.contains(&"SE".to_owned()));
    assert!(!dict_keys(&s, id(12)).contains(&"StructParent".to_owned()));
    let view = s.view();
    let action = |n| {
        view.value(id(n))
            .and_then(Object::as_dict)
            .and_then(|d| d.get(b"A"))
            .and_then(Object::as_dict)
            .unwrap()
    };
    assert!(action(12).get(b"SD").is_none() && action(12).get(b"D").is_some());
    assert!(
        action(13).get(b"SD").is_some(),
        "a GoToR /SD names another file"
    );
}

#[test]
fn an_image_under_optional_content_keeps_its_alternates() {
    let text = String::from_utf8_lossy(&sample())
        .replace("/Alternates [", "/OC << /Type /OCMD >> /Alternates [");
    let s = session(text.into_bytes());
    let alternates = DiscardOptions::none().with_object(DiscardObject::AlternateImages);
    assert!(
        s.plan_optimize(&only(alternates))
            .unwrap()
            .discard_objects
            .is_empty()
    );
}
