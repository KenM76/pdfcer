#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use crate::document::Document;
use crate::edit::EditSession;
use crate::font_unembed::UnembedRequest;
use crate::optimize::{
    OptimizeError, OptimizeSettings, OptimizedFile, Rule, Selector, StreamAction,
};
use crate::space_audit::SpaceCategory;
use crate::writer::{RebuildOptions, SaveOptions};

fn open(rel: &str) -> EditSession {
    let path = format!(
        "{}/../../fixtures/synthetic/{rel}",
        env!("CARGO_MANIFEST_DIR")
    );
    EditSession::new(Document::from_bytes(std::fs::read(path).unwrap()).unwrap())
}

fn unembed(request: UnembedRequest) -> OptimizeSettings {
    let mut s = OptimizeSettings::new();
    s.unembed = Some(request);
    s
}

fn written(s: &EditSession, plan: &crate::optimize::OptimizePlan) -> Vec<u8> {
    let (file, _) = s
        .save_optimized(plan, &SaveOptions::default(), &RebuildOptions::none())
        .unwrap();
    match file {
        OptimizedFile::Smaller(b) | OptimizedFile::NotSmaller { bytes: b, .. } => b,
        other => panic!("expected a written file, got {other:?}"),
    }
}

fn has(bytes: &[u8], needle: &[u8]) -> bool {
    bytes.windows(needle.len()).any(|w| w == needle)
}

#[test]
fn every_removable_font_goes_in_one_undo_entry_and_the_file_shrinks() {
    let mut s = open("text/subset-simple-embedded.pdf");
    let original = s.base.bytes().len();
    let plan = s
        .plan_optimize(&unembed(UnembedRequest::all_removable()))
        .unwrap();
    let fonts = plan.unembed.as_ref().unwrap();
    assert!(!fonts.targets.is_empty());
    assert!(plan.font_saving() > 0);
    assert!(plan.removes_content());
    assert!(
        plan.notes.iter().any(|n| n.contains("look different")),
        "{:?}",
        plan.notes
    );
    let undo_before = s.undo.len();
    let n = s.apply_optimize(&plan).unwrap();
    assert!(n >= fonts.targets.len());
    assert_eq!(s.undo.len(), undo_before + 1, "one gesture, one entry");
    let out = written(&s, &plan);
    assert!(out.len() < original, "{} !< {original}", out.len());
    assert!(!has(&out, b"/FontFile"), "a program survived");
    s.undo();
    assert!(
        s.unembed_preview(&UnembedRequest::all_removable())
            .targets
            .len()
            == fonts.targets.len()
    );
}

#[test]
fn a_freed_program_is_not_also_recompressed() {
    let s = open("text/subset-simple-embedded.pdf");
    let mut settings = unembed(UnembedRequest::all_removable());
    settings.rules.push(Rule {
        select: Selector {
            categories: vec![SpaceCategory::Fonts],
            ..Selector::default()
        },
        action: StreamAction::Recompress { level: 9 },
    });
    let plan = s.plan_optimize(&settings).unwrap();
    let freed: Vec<_> = plan
        .unembed
        .as_ref()
        .unwrap()
        .targets
        .iter()
        .filter_map(|t| t.program_id)
        .collect();
    assert!(!freed.is_empty());
    for id in &freed {
        assert!(
            plan.changes.iter().all(|c| c.id != *id),
            "{id} still planned"
        );
        assert!(
            plan.skipped.iter().all(|c| c.id != *id),
            "{id} still skipped"
        );
    }
}

#[test]
fn a_pdfa_document_keeps_its_fonts_and_says_why() {
    let s = open("unembed/unembed-pdfa.pdf");
    let plan = s
        .plan_optimize(&unembed(UnembedRequest::all_removable()))
        .unwrap();
    assert!(plan.unembed.is_none());
    assert_eq!(plan.font_saving(), 0);
    assert!(
        plan.notes.iter().any(|n| n.contains("PDF/A")),
        "{:?}",
        plan.notes
    );
}

#[test]
fn no_request_plans_no_fonts() {
    let s = open("text/subset-simple-embedded.pdf");
    let plan = s.plan_optimize(&OptimizeSettings::new()).unwrap();
    assert!(plan.unembed.is_none());
    assert!(!plan.removes_content());
}

#[test]
fn a_name_matching_nothing_is_noted() {
    let s = open("text/subset-simple-embedded.pdf");
    let plan = s
        .plan_optimize(&unembed(UnembedRequest::named(["NoSuchFont"])))
        .unwrap();
    assert!(plan.unembed.as_ref().unwrap().is_empty());
    assert!(
        plan.notes.iter().any(|n| n.contains("\"NoSuchFont\"")),
        "{:?}",
        plan.notes
    );
}

#[test]
fn fonts_unembedded_after_planning_make_the_plan_stale() {
    let mut s = open("text/subset-simple-embedded.pdf");
    let plan = s
        .plan_optimize(&unembed(UnembedRequest::all_removable()))
        .unwrap();
    s.unembed_fonts(&UnembedRequest::all_removable()).unwrap();
    let undo = s.undo.len();
    assert!(matches!(
        s.apply_optimize(&plan),
        Err(OptimizeError::StalePlan { .. })
    ));
    assert_eq!(s.undo.len(), undo, "nothing committed");
}

#[test]
fn a_shared_program_is_disclosed_as_staying() {
    let s = open("unembed/unembed-shared-program.pdf");
    let mut first = s.unembed_preview(&UnembedRequest::all_removable());
    let Some(t) = first.targets.pop() else {
        panic!("fixture has no removable font");
    };
    let plan = s
        .plan_optimize(&unembed(UnembedRequest::objects([t.id])))
        .unwrap();
    let fonts = plan.unembed.as_ref().unwrap();
    assert!(fonts.targets.iter().any(|t| !t.program_freed), "{fonts:?}");
    assert!(
        plan.notes.iter().any(|n| n.contains("share a program")),
        "{:?}",
        plan.notes
    );
    assert_eq!(plan.font_saving(), 0);
}
