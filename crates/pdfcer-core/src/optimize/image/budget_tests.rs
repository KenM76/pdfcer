#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use super::pipeline_tests::{AT_100, N, doc};
use crate::document::Document;
use crate::edit::EditSession;
use crate::filters::flate;
use crate::optimize::budget::{self, BudgetFit, LAST_STEP, step_settings};
use crate::optimize::{ImagePatch, OptimizeSettings};
use crate::space_audit::tests::stream;
use crate::writer::SaveOptions;

/// A 300 x 300 RGB picture with ramps and edges, stored uncompressed in
/// Flate, so every ladder step shrinks it.
fn photo_pdf() -> Vec<u8> {
    let samples: Vec<u8> = (0..N as usize * N as usize * 3)
        .map(|i| {
            let (p, c) = (i / 3, i % 3);
            let (x, y) = (p % N as usize, p / N as usize);
            let edge = if (x / 37 + y / 41) % 2 == 0 { 0 } else { 90 };
            ((x + 2 * y + 40 * c + edge) % 256) as u8
        })
        .collect();
    let image = stream(
        &format!(
            "/Type /XObject /Subtype /Image /Width {N} /Height {N} /ColorSpace /DeviceRGB \
             /BitsPerComponent 8 /Filter /FlateDecode"
        ),
        &flate::encode_with_level(&samples, 0),
    );
    doc(AT_100, &[image])
}

fn session() -> EditSession {
    EditSession::new(Document::from_bytes(photo_pdf()).unwrap())
}

fn fit(settings: &OptimizeSettings, max: u64) -> (EditSession, BudgetFit) {
    let mut s = session();
    let fit = s
        .optimize_to_budget(settings, max, &SaveOptions::default())
        .unwrap();
    (s, fit)
}

/// The bytes ladder step `step` writes, measured on a fresh session.
fn size_at(step: usize) -> u64 {
    let mut s = session();
    let settings = step_settings(&OptimizeSettings::new(), step);
    let plan = s.plan_optimize(&settings).unwrap();
    s.apply_optimize(&plan).unwrap();
    let (file, _) = s
        .save_optimized(&plan, &SaveOptions::default(), &settings.structure)
        .unwrap();
    budget::written_len(&file)
}

#[test]
fn a_budget_the_operators_settings_meet_tries_nothing_lossier() {
    let (s, fit) = fit(&OptimizeSettings::new(), u64::MAX);
    assert_eq!((fit.step, fit.met, fit.attempts.len()), (0, true, 1));
    assert!(fit.plan.changes.iter().all(|c| !c.lossy));
    assert!(s.undo_depth() <= 1);
}

#[test]
fn a_reachable_budget_takes_the_least_lossy_step_that_fits() {
    let budget = size_at(4);
    assert!(budget < size_at(0), "the ladder must shrink the fixture");
    let (s, fit) = fit(&OptimizeSettings::new(), budget);
    assert!(fit.met && (1..=4).contains(&fit.step), "{fit:?}");
    let chosen = fit.attempts.iter().find(|a| a.step == fit.step).unwrap();
    assert!(chosen.bytes <= budget);
    // Planned against the untouched session, not a reverted trial's leftovers.
    assert!(fit.plan.changes.iter().any(|c| c.lossy), "{fit:?}");
    assert_eq!(chosen.bytes, budget::written_len(&fit.file));
    // The step below the chosen one was tried and missed.
    let below = fit
        .attempts
        .iter()
        .find(|a| a.step == fit.step - 1)
        .unwrap();
    assert!(below.bytes > budget);
    assert!(fit.attempts.len() <= 6);
    // Only the chosen step is left applied, as one command.
    assert_eq!(s.undo_depth(), 1);
    let (bytes, _) = s.to_full_bytes(&SaveOptions::default()).unwrap();
    assert!(bytes.len() as u64 <= budget + 1024);
}

#[test]
fn an_unreachable_budget_returns_the_last_step_unmet() {
    let (s, fit) = fit(&OptimizeSettings::new(), 100);
    assert_eq!((fit.step, fit.met), (LAST_STEP, false));
    assert_eq!(fit.attempts.len(), 2);
    assert_eq!(fit.attempts[1].ppi, Some(72));
    assert_eq!(s.undo_depth(), 1);
}

#[test]
fn a_lower_operator_ceiling_survives_the_ladder() {
    let mut settings = OptimizeSettings::new();
    settings.images.quality = Some(20);
    settings.images.ppi = Some(50);
    let stepped = step_settings(&settings, LAST_STEP);
    let crate::optimize::StreamAction::Image(patch) = stepped.rules[0].action else {
        panic!()
    };
    let mut want = ImagePatch::new();
    want.encode = Some(super::ImageEncode::Jpeg);
    want.quality = Some(20);
    want.ppi = Some(50);
    want.above = Some(50);
    assert_eq!(patch, want);
    assert_eq!(step_settings(&settings, 0), settings);
    // The same ceilings given on the default stream action.
    let mut on_streams = OptimizeSettings::new();
    let mut base = ImagePatch::new();
    base.quality = Some(20);
    base.ppi = Some(50);
    on_streams.streams = crate::optimize::StreamAction::Image(base);
    let stepped = step_settings(&on_streams, LAST_STEP);
    assert_eq!(
        stepped.rules[0].action,
        crate::optimize::StreamAction::Image(want)
    );
}

#[test]
fn the_ladder_never_gets_less_lossy() {
    for pair in budget::LADDER.windows(2) {
        let ((q0, p0), (q1, p1)) = (pair[0], pair[1]);
        assert!(q1 <= q0);
        assert!(p1.unwrap_or(u16::MAX) <= p0.unwrap_or(u16::MAX));
    }
}

#[test]
fn attempts_report_the_ceiling_that_applied_and_equal_steps_agree() {
    let mut settings = OptimizeSettings::new();
    settings.images.quality = Some(60);
    assert_eq!(budget::step_ceilings(&settings, 1), (Some(60), None));
    assert_eq!(budget::step_ceilings(&settings, 4), (Some(50), None));
    assert_eq!(budget::step_ceilings(&settings, 0), (None, None));
    let (_, fit) = fit(&settings, size_at(4));
    for a in &fit.attempts {
        for b in &fit.attempts {
            if a.step != 0 && (a.quality, a.ppi) == (b.quality, b.ppi) {
                assert_eq!(a.bytes, b.bytes, "{fit:?}");
            }
        }
    }
    assert!(
        fit.attempts
            .iter()
            .all(|a| a.quality.is_none_or(|q| q <= 60))
    );
}
