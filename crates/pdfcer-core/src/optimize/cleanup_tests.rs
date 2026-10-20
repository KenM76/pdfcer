#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test code: a failed lookup should fail the test loudly

use super::tests::{build, session};
use super::*;
use crate::edit::EditSession;
use crate::object::Object;
use crate::outline::read_outline;
use crate::pageops::references::DestinationResolver;
use crate::writer::SaveOptions;

/// Two pages (3, 4). Page 3's links: 5 good, 6 to a non-page, 7 to an
/// undefined name, 8 good through a GoTo name. Outline: 11 good; 12 broken
/// with good child 14; 13 a group whose only child 15 is broken. Named
/// destinations: legacy `legacyused` (the open action) and `legacyunused`;
/// tree `alias` (naming `unused`), `unused` and `used` under a `/Kids` root.
fn navigation(link8_action: &str) -> Vec<u8> {
    let b = |s: &str| s.as_bytes().to_vec();
    build(&[
        b(
            "<< /Type /Catalog /Pages 2 0 R /Outlines 10 0 R /Dests 20 0 R \
           /Names << /Dests 21 0 R >> /OpenAction << /S /GoTo /D /legacyused >> >>",
        ),
        b("<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>"),
        b("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] \
           /Annots [5 0 R 6 0 R 7 0 R 8 0 R] >>"),
        b("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] >>"),
        b("<< /Type /Annot /Subtype /Link /Rect [0 0 10 10] /Dest [4 0 R /Fit] >>"),
        b("<< /Type /Annot /Subtype /Link /Rect [0 0 10 10] /Dest [9 0 R /Fit] >>"),
        b("<< /Type /Annot /Subtype /Link /Rect [0 0 10 10] /Dest (undefined) >>"),
        format!("<< /Type /Annot /Subtype /Link /Rect [0 0 10 10] /A {link8_action} >>")
            .into_bytes(),
        b("<< /Orphan true >>"),
        b("<< /Type /Outlines /First 11 0 R /Last 13 0 R /Count 3 >>"),
        b("<< /Title (Good) /Parent 10 0 R /Next 12 0 R /Dest [3 0 R /Fit] >>"),
        b(
            "<< /Title (Broken) /Parent 10 0 R /Prev 11 0 R /Next 13 0 R \
           /First 14 0 R /Last 14 0 R /Count 1 /Dest /missing >>",
        ),
        b("<< /Title (Group) /Parent 10 0 R /Prev 12 0 R /First 15 0 R /Last 15 0 R /Count 1 >>"),
        b("<< /Title (Good child) /Parent 12 0 R /Dest (used) >>"),
        b("<< /Title (Dead) /Parent 13 0 R /Dest [9 0 R /XYZ null null null] >>"),
        b("null"),
        b("null"),
        b("null"),
        b("null"),
        b("<< /legacyused [3 0 R /Fit] /legacyunused [3 0 R /Fit] >>"),
        b("<< /Kids [22 0 R] >>"),
        b(
            "<< /Names [(alias) (unused) (unused) [3 0 R /Fit] (used) [4 0 R /Fit]] /Limits [(alias) (used)] >>",
        ),
    ])
}

const GOTO_USED: &str = "<< /S /GoTo /D (used) >>";

fn cleanup_only(options: CleanupOptions) -> OptimizeSettings {
    let mut s = OptimizeSettings::new();
    s.cleanup = options;
    s
}

fn annots(s: &EditSession) -> Vec<u32> {
    let Some(Object::Dict(page)) = s.value(ObjId::new(3, 0)) else {
        panic!("page 3");
    };
    let Some(Object::Array(a)) = page.get(b"Annots") else {
        panic!("annots");
    };
    a.iter().map(|o| o.as_reference().unwrap().num).collect()
}

fn titles(s: &EditSession) -> Vec<String> {
    read_outline(&s.graph())
        .flatten()
        .iter()
        .map(|i| i.title.clone())
        .collect()
}

fn names(s: &EditSession) -> Vec<String> {
    let mut v: Vec<String> = DestinationResolver::new(&s.graph())
        .iter()
        .map(|(k, _)| String::from_utf8_lossy(k).into_owned())
        .collect();
    v.sort();
    v
}

#[test]
fn a_plan_names_each_broken_link_bookmark_and_unused_destination() {
    let plan = session(navigation(GOTO_USED))
        .plan_optimize(&cleanup_only(CleanupOptions::all()))
        .unwrap();
    let c = &plan.cleanup;
    let links: Vec<(u32, BrokenTarget)> = c
        .links
        .iter()
        .map(|l| (l.id.num, l.target.clone()))
        .collect();
    assert_eq!(
        links,
        [
            (6, BrokenTarget::MissingPage(ObjId::new(9, 0))),
            (7, BrokenTarget::UndefinedName(b"undefined".to_vec())),
        ]
    );
    // 12 is broken but keeps a working child; 13 only grouped a dead child.
    assert_eq!(c.bookmarks.len(), 1);
    assert_eq!(c.bookmarks[0].id.num, 13);
    assert_eq!(c.bookmarks[0].target, None);
    assert_eq!(c.bookmarks[0].descendants, 1);
    assert_eq!(
        c.destinations,
        [
            b"alias".to_vec(),
            b"legacyunused".to_vec(),
            b"unused".to_vec()
        ]
    );
    assert!(plan.notes.iter().any(|n| n.contains("another file")));
}

#[test]
fn apply_removes_them_as_one_undo_and_undo_restores_them() {
    let mut s = session(navigation(GOTO_USED));
    let plan = s
        .plan_optimize(&cleanup_only(CleanupOptions::all()))
        .unwrap();
    assert_eq!(s.apply_optimize(&plan).unwrap(), 2 + 2 + 3);
    assert_eq!(annots(&s), [5, 8]);
    assert_eq!(titles(&s), ["Good", "Broken", "Good child"]);
    assert_eq!(names(&s), ["legacyused", "used"]);
    assert_eq!(s.undo_depth(), 1);
    assert_eq!(
        s.undo_kinds().next(),
        Some(crate::edit::CommandKind::Optimize { count: 7 })
    );
    s.undo();
    assert_eq!(annots(&s), [5, 6, 7, 8]);
    assert_eq!(
        titles(&s),
        ["Good", "Broken", "Good child", "Group", "Dead"]
    );
    assert_eq!(
        names(&s),
        ["alias", "legacyunused", "legacyused", "unused", "used"]
    );
}

#[test]
fn the_saved_file_reads_back_with_a_flat_destination_tree() {
    let mut s = session(navigation(GOTO_USED));
    let plan = s
        .plan_optimize(&cleanup_only(CleanupOptions::all()))
        .unwrap();
    s.apply_optimize(&plan).unwrap();
    let (file, _) = s
        .save_optimized(&SaveOptions::default(), &RebuildOptions::all())
        .unwrap();
    let OptimizedFile::Smaller(bytes) = file else {
        panic!("expected a smaller file");
    };
    let reopened = session(bytes);
    assert_eq!(names(&reopened), ["legacyused", "used"]);
    assert_eq!(titles(&reopened), ["Good", "Broken", "Good child"]);
    let again = reopened
        .plan_optimize(&cleanup_only(CleanupOptions::all()))
        .unwrap();
    assert!(again.cleanup.is_empty(), "{:?}", again.cleanup);
}

#[test]
fn javascript_keeps_every_named_destination() {
    let plan = session(navigation(
        "<< /S /JavaScript /JS (this.gotoNamedDest\\(\"unused\"\\)) >>",
    ))
    .plan_optimize(&cleanup_only(CleanupOptions::all()))
    .unwrap();
    assert!(plan.cleanup.destinations.is_empty());
    assert!(plan.notes.iter().any(|n| n.contains("JavaScript")));
    // Without link 8's GoTo, `used` is still named by bookmark 14.
    assert_eq!(plan.cleanup.links.len(), 2);
}

#[test]
fn each_option_is_independent_and_none_plans_nothing() {
    let s = session(navigation(GOTO_USED));
    let plan = |o| s.plan_optimize(&cleanup_only(o)).unwrap().cleanup;
    assert!(plan(CleanupOptions::none()).is_empty());
    let links = plan(CleanupOptions {
        invalid_links: true,
        ..CleanupOptions::none()
    });
    assert_eq!(
        (
            links.links.len(),
            links.bookmarks.len(),
            links.destinations.len()
        ),
        (2, 0, 0)
    );
    let marks = plan(CleanupOptions {
        invalid_bookmarks: true,
        ..CleanupOptions::none()
    });
    assert_eq!(
        (
            marks.links.len(),
            marks.bookmarks.len(),
            marks.destinations.len()
        ),
        (0, 1, 0)
    );
    assert!(
        !Preset::Standard
            .settings()
            .cleanup
            .unreferenced_destinations
    );
    assert!(Preset::Screen.settings().cleanup.unreferenced_destinations);
}

#[test]
fn a_named_destination_naming_another_keeps_both() {
    let plan = session(navigation("<< /S /GoTo /D (alias) >>"))
        .plan_optimize(&cleanup_only(CleanupOptions::all()))
        .unwrap();
    assert_eq!(plan.cleanup.destinations, [b"legacyunused".to_vec()]);
}

#[test]
fn a_failed_deletion_undoes_the_whole_apply() {
    let mut s = session(navigation(GOTO_USED));
    let plan = s
        .plan_optimize(&cleanup_only(CleanupOptions::all()))
        .unwrap();
    s.delete_outline_item(ObjId::new(13, 0)).unwrap();
    let err = s.apply_optimize(&plan).unwrap_err();
    assert!(matches!(err, OptimizeError::CleanupFailed { id, .. } if id.num == 13));
    assert_eq!(annots(&s), [5, 6, 7, 8]);
    assert_eq!(s.undo_depth(), 1);
    assert_eq!(titles(&s), ["Good", "Broken", "Good child"]);
}
