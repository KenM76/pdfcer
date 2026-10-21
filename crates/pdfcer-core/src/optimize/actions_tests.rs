#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test code: a failed lookup should fail the test loudly

use super::tests::{build, dict, id, only, session};
use super::*;
use crate::graph::ObjectGraph;
use crate::object::{Dict, Object};
use crate::writer::SaveOptions;

/// A page (3) with a submit button (5) whose chain continues to a script and
/// a named action (11); links to another file (6), the web (7), and a page
/// with a Launch chained after (8); a ResetForm open action (9) chained to a
/// GoTo (12) that is chained to a GoToE into another file (13); an indirect
/// page `/AA` (10) importing data on open; a Movie whose `/A` is an
/// activation (14); and a link (16) to a SubmitForm chained to itself (15).
fn sample() -> Vec<u8> {
    let b = |s: &str| s.as_bytes().to_vec();
    build(&[
        b(
            "<< /Type /Catalog /Pages 2 0 R /OpenAction 9 0 R /AA << /WC << /S /JavaScript /JS (closing) >> >> /AcroForm << /Fields [5 0 R] >> >>",
        ),
        b("<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        b(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R /AA 10 0 R /Annots [5 0 R 6 0 R 7 0 R 8 0 R 14 0 R 16 0 R] >>",
        ),
        b("<< /Length 0 >>\nstream\n\nendstream"),
        b(
            "<< /Type /Annot /Subtype /Widget /FT /Btn /Ff 65536 /T (send) /Rect [0 0 10 10] /A << /S /SubmitForm /F (https://example.invalid/submit) /Next [<< /S /JavaScript /JS (after) >> 11 0 R] >> >>",
        ),
        b(
            "<< /Type /Annot /Subtype /Link /Rect [0 0 10 10] /A << /S /GoToR /F (other.pdf) /D [0 /Fit] >> >>",
        ),
        b(
            "<< /Type /Annot /Subtype /Link /Rect [0 0 10 10] /A << /S /URI /URI (https://example.invalid/) >> >>",
        ),
        b(
            "<< /Type /Annot /Subtype /Link /Rect [0 0 10 10] /A << /S /GoTo /D [3 0 R /Fit] /Next << /S /Launch /F (calc.exe) >> >> >>",
        ),
        b("<< /S /ResetForm /Next 12 0 R >>"),
        b("<< /O << /S /ImportData /F (data.fdf) >> /C << /S /GoTo /D [3 0 R /Fit] >> >>"),
        b("<< /S /Named /N /NextPage >>"),
        b("<< /S /GoTo /D [3 0 R /Fit] /Next 13 0 R >>"),
        b("<< /S /GoToE /F (root.pdf) /T << /R /C /N (a) >> >>"),
        b(
            "<< /Type /Annot /Subtype /Movie /Rect [0 0 10 10] /Movie << /F (clip.mov) >> /A << /S /Launch /Mode /Once >> >>",
        ),
        b("<< /S /SubmitForm /F (https://example.invalid/loop) /Next 15 0 R >>"),
        b("<< /Type /Annot /Subtype /Link /Rect [0 0 10 10] /A 15 0 R >>"),
    ])
}

fn both() -> DiscardOptions {
    DiscardOptions::none()
        .with_object(DiscardObject::FormActions)
        .with_object(DiscardObject::ExternalReferences)
}

fn kind(d: &Dict) -> &[u8] {
    d.get(b"S").and_then(Object::as_name).unwrap().as_bytes()
}

fn owners(plan: &OptimizePlan, object: DiscardObject) -> Vec<u32> {
    let items = plan.discard_objects.iter().filter(|i| i.object == object);
    items.map(|i| i.owner.num).collect()
}

#[test]
fn each_class_lists_the_objects_carrying_it_each_measured() {
    let s = session(sample());
    let plan = s.plan_optimize(&only(both())).unwrap();
    assert_eq!(owners(&plan, DiscardObject::FormActions), [1, 5, 10, 16]);
    assert_eq!(owners(&plan, DiscardObject::ExternalReferences), [6, 8, 12]);
    for item in &plan.discard_objects {
        assert!(item.bytes > 0, "{}", item.location);
    }
    let button = plan.discard_objects.iter().find(|i| i.owner == id(5));
    assert_eq!(button.unwrap().location, "object 5 0: SubmitForm");
    let forms = DiscardOptions::none().with_object(DiscardObject::FormActions);
    let plan = s.plan_optimize(&only(forms)).unwrap();
    assert!(owners(&plan, DiscardObject::ExternalReferences).is_empty());
    assert!(
        plan.notes
            .iter()
            .any(|n| n.contains("do nothing when clicked")),
        "{:?}",
        plan.notes
    );
}

#[test]
fn a_removed_action_is_spliced_out_and_what_follows_it_still_runs() {
    let mut s = session(sample());
    let catalog_before = dict(&s, 1);
    let plan = s.plan_optimize(&only(both())).unwrap();
    assert_eq!(s.apply_optimize(&plan).unwrap(), 7);

    // The button runs its script, then the named action, as before.
    let button = dict(&s, 5);
    let head = button.get(b"A").and_then(Object::as_dict).unwrap();
    assert_eq!(kind(head), b"JavaScript");
    assert_eq!(head.get(b"Next"), Some(&Object::Reference(id(11))));

    assert!(
        dict(&s, 6).get(b"A").is_none(),
        "a GoToR link has no action"
    );
    let web = dict(&s, 7);
    assert_eq!(
        kind(web.get(b"A").and_then(Object::as_dict).unwrap()),
        b"URI"
    );
    let page_link = dict(&s, 8);
    let goto = page_link.get(b"A").and_then(Object::as_dict).unwrap();
    assert!(kind(goto) == b"GoTo" && goto.get(b"Next").is_none());

    let catalog = dict(&s, 1);
    assert_eq!(catalog.get(b"OpenAction"), Some(&Object::Reference(id(12))));
    assert!(catalog.get(b"AA").is_some(), "the script on close stays");
    assert!(dict(&s, 12).get(b"Next").is_none(), "the GoToE is gone");
    let aa = dict(&s, 10);
    assert!(aa.get(b"O").is_none() && aa.get(b"C").is_some());
    let movie = dict(&s, 14);
    assert!(
        movie.get(b"A").is_some(),
        "a movie activation is not an action"
    );
    assert!(
        dict(&s, 16).get(b"A").is_none(),
        "a self-chained submit ends"
    );
    for gone in [9, 13, 15] {
        assert!(s.view().value(id(gone)).is_none(), "object {gone} survived");
    }

    assert_eq!(s.undo_depth(), 1);
    s.undo();
    assert_eq!(dict(&s, 1), catalog_before);
    assert!(dict(&s, 6).get(b"A").is_some());
}

#[test]
fn the_saved_file_holds_none_of_the_discarded_actions() {
    let mut s = session(sample());
    let plan = s.plan_optimize(&only(both())).unwrap();
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
        "SubmitForm",
        "ResetForm",
        "ImportData",
        "GoToR",
        "GoToE",
        "calc.exe",
    ] {
        assert!(!text.contains(gone), "{gone} survived");
    }
    assert!(text.contains("/URI") && text.contains("/NextPage"));
}

#[test]
fn a_go_to_embedded_without_a_file_targets_this_file_and_stays() {
    let text = String::from_utf8_lossy(&sample()).replace("/GoToE /F (root.pdf) ", "/GoToE ");
    let s = session(text.into_bytes());
    let external = DiscardOptions::none().with_object(DiscardObject::ExternalReferences);
    let plan = s.plan_optimize(&only(external)).unwrap();
    assert_eq!(owners(&plan, DiscardObject::ExternalReferences), [6, 8]);
}
