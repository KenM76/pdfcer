#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test code: a failed lookup should fail the test loudly

use super::tests::{build, dict, only, session};
use super::*;
use crate::doc_metadata::MetadataKind;
use crate::writer::SaveOptions;

/// One page whose content (4) draws on a visible layer (5, `Notes`) and a
/// hidden one (6, `Draft`); `extra` goes in the page's `/Properties`.
fn sample_with(extra: &str) -> Vec<u8> {
    let b = |s: &str| s.as_bytes().to_vec();
    let content = "/OC /L1 BDC 0 0 m 10 10 l S EMC /OC /L2 BDC 1 1 m 9 9 l S EMC";
    build(&[
        b(
            "<< /Type /Catalog /Pages 2 0 R /OCProperties << /OCGs [5 0 R 6 0 R] /D << /OFF [6 0 R] >> >> >>",
        ),
        b("<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R /Resources << /Properties << /L1 5 0 R /L2 6 0 R {extra}>> >> >>"
        )
        .into_bytes(),
        format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        )
        .into_bytes(),
        b("<< /Type /OCG /Name (Notes) >>"),
        b("<< /Type /OCG /Name (Draft) >>"),
        b("<< /Type /OCMD /OCGs [5 0 R] >>"),
    ])
}

fn sample() -> Vec<u8> {
    sample_with("")
}

fn flatten() -> DiscardOptions {
    DiscardOptions::none().with_object(DiscardObject::FlattenLayers)
}

fn layer_count(s: &crate::edit::EditSession) -> usize {
    crate::layers::list_layers(&s.view()).len()
}

fn saved_text(s: &mut crate::edit::EditSession, plan: &OptimizePlan) -> String {
    let (file, _) = s
        .save_optimized(plan, &SaveOptions::default(), &RebuildOptions::none())
        .unwrap();
    let bytes = match file {
        OptimizedFile::Smaller(b) | OptimizedFile::NotSmaller { bytes: b, .. } => b,
        other => panic!("expected a written file, got {other:?}"),
    };
    String::from_utf8_lossy(&bytes).into_owned()
}

#[test]
fn only_the_visible_layer_is_listed() {
    let s = session(sample());
    let plan = s.plan_optimize(&only(flatten())).unwrap();
    let places: Vec<&str> = plan
        .discard_objects
        .iter()
        .map(|i| i.location.as_str())
        .collect();
    assert_eq!(places, ["layer 5 0: Notes"]);
    assert!(plan.discard_objects[0].bytes > 0);
    assert!(
        plan.notes
            .iter()
            .any(|n| n.contains("visible layers merge"))
    );
}

#[test]
fn the_visible_layer_merges_and_the_hidden_one_stays_a_layer() {
    let mut s = session(sample());
    let plan = s.plan_optimize(&only(flatten())).unwrap();
    s.apply_optimize(&plan).unwrap();
    assert_eq!(layer_count(&s), 1);
    let text = saved_text(&mut s, &plan);
    assert!(
        !text.contains("/L1 BDC"),
        "the visible section was unwrapped"
    );
    assert!(text.contains("0 0 m 10 10 l S"), "what it drew stays");
    assert!(text.contains("/L2 BDC") && text.contains("(Draft)"));
}

#[test]
fn with_hidden_layers_discarded_no_layer_is_left() {
    let mut s = session(sample());
    let options = flatten().with(MetadataKind::HiddenLayer);
    let plan = s.plan_optimize(&only(options)).unwrap();
    s.apply_optimize(&plan).unwrap();
    assert_eq!(layer_count(&s), 0);
    let text = saved_text(&mut s, &plan);
    assert!(text.contains("0 0 m 10 10 l S") && !text.contains("1 1 m 9 9 l S"));
    assert!(!text.contains("BDC"));
    s.undo();
    assert_eq!(layer_count(&s), 2);
}

#[test]
fn a_layer_in_a_membership_dictionary_refuses_the_run_and_writes_nothing() {
    let mut s = session(sample_with("/M 7 0 R "));
    let before = dict(&s, 1);
    let plan = s.plan_optimize(&only(flatten())).unwrap();
    let err = s.apply_optimize(&plan).unwrap_err();
    assert!(
        matches!(err, OptimizeError::ObjectDiscardFailed { .. }),
        "{err}"
    );
    assert_eq!(dict(&s, 1), before);
    assert_eq!(layer_count(&s), 2);
}
