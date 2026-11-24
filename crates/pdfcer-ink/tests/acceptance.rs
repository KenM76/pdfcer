//! X107's acceptance cases: overprint, OPM, spot planes, the coverage rules,
//! group alpha and the refusal ceilings.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // Tests fail loudly.

use pdfcer_ink::{
    Blend, Coverage, CoverageWarning, Group, InkCanvas, InkError, Limits, Overprint, Paint, Span,
};

const LIMITS: Limits = Limits::new(1 << 24, 8);
const FULL: [u8; 1] = [255];

fn paint(c: &mut InkCanvas, tints: &[Option<f32>], alpha: f32, overprint: Overprint) {
    c.paint(&Paint {
        coverage: Coverage::Mask(&FULL),
        tints,
        alpha,
        blend: Blend::Normal,
        overprint,
    })
    .unwrap();
}

fn plane(c: &InkCanvas, i: usize) -> f32 {
    c.landed(i).unwrap()[0]
}

fn assert_near(got: f32, want: f32) {
    assert!((got - want).abs() < 1e-5, "got {got}, want {want}");
}

const C100: [Option<f32>; 1] = [Some(1.0)];
const M100: [Option<f32>; 2] = [None, Some(1.0)];

#[test]
fn cyan_overprinting_magenta_keeps_both_planes() {
    let mut c = InkCanvas::new(1, 1, &[], LIMITS).unwrap();
    paint(&mut c, &M100, 1.0, Overprint::Off);
    paint(&mut c, &C100, 1.0, Overprint::On);
    assert_near(plane(&c, 0), 1.0);
    assert_near(plane(&c, 1), 1.0);
}

#[test]
fn cyan_without_overprint_knocks_out_magenta() {
    let mut c = InkCanvas::new(1, 1, &[], LIMITS).unwrap();
    paint(&mut c, &M100, 1.0, Overprint::Off);
    paint(&mut c, &C100, 1.0, Overprint::Off);
    assert_near(plane(&c, 0), 1.0);
    assert_near(plane(&c, 1), 0.0);
}

#[test]
fn opm_1_keeps_cyan_under_a_zero_cyan_tint_and_opm_0_knocks_it_out() {
    let zero_c = [Some(0.0), Some(1.0), Some(0.0), Some(0.0)];
    let mut nonzero = InkCanvas::new(1, 1, &[], LIMITS).unwrap();
    paint(&mut nonzero, &C100, 1.0, Overprint::Off);
    paint(&mut nonzero, &zero_c, 1.0, Overprint::OnNonzero);
    assert_near(plane(&nonzero, 0), 1.0);
    assert_near(plane(&nonzero, 1), 1.0);

    let mut zero = InkCanvas::new(1, 1, &[], LIMITS).unwrap();
    paint(&mut zero, &C100, 1.0, Overprint::Off);
    paint(&mut zero, &zero_c, 1.0, Overprint::On);
    assert_near(plane(&zero, 0), 0.0);
    assert_near(plane(&zero, 1), 1.0);
}

#[test]
fn a_spot_paint_fills_only_its_plane_and_counts_for_tac_not_rich_black() {
    let mut c = InkCanvas::new(1, 1, &["Gold"], LIMITS).unwrap();
    paint(
        &mut c,
        &[Some(0.0), Some(0.0), Some(0.0), Some(1.0)],
        1.0,
        Overprint::Off,
    );
    paint(
        &mut c,
        &[None, None, None, None, Some(1.0)],
        1.0,
        Overprint::On,
    );
    for i in 0..3 {
        assert_near(plane(&c, i), 0.0);
    }
    assert_near(plane(&c, 3), 1.0);
    assert_near(plane(&c, 4), 1.0);
    let masks = c.coverage_masks(&[
        CoverageWarning::TotalAreaCoverage { limit_percent: 150 },
        CoverageWarning::RichBlack {
            black_start_percent: 95,
        },
    ]);
    assert_eq!(masks[0].flagged, 1, "K100 + Gold100 = 200% > 150%");
    assert_eq!(masks[1].flagged, 0, "a spot does not make black rich");
}

#[test]
fn a_spot_paint_without_overprint_fills_only_its_plane_on_paper() {
    let mut c = InkCanvas::new(1, 1, &["Gold"], LIMITS).unwrap();
    paint(
        &mut c,
        &[None, None, None, None, Some(0.6)],
        1.0,
        Overprint::Off,
    );
    for i in 0..4 {
        assert_near(plane(&c, i), 0.0);
    }
    assert_near(plane(&c, 4), 0.6);
}

fn rules() -> [CoverageWarning; 2] {
    [
        CoverageWarning::TotalAreaCoverage { limit_percent: 250 },
        CoverageWarning::RichBlack {
            black_start_percent: 95,
        },
    ]
}

#[test]
fn rich_black_is_flagged_by_both_rules_and_plain_black_by_neither() {
    let mut rich = InkCanvas::new(1, 1, &[], LIMITS).unwrap();
    paint(
        &mut rich,
        &[Some(0.6), Some(0.5), Some(0.5), Some(1.0)],
        1.0,
        Overprint::Off,
    );
    let m = rich.coverage_masks(&rules());
    assert_eq!((m[0].flagged, m[1].flagged), (1, 1));
    assert_eq!((m[0].mask.clone(), m[1].mask.clone()), (vec![1], vec![1]));

    let mut plain = InkCanvas::new(1, 1, &[], LIMITS).unwrap();
    paint(
        &mut plain,
        &[Some(0.0), Some(0.0), Some(0.0), Some(1.0)],
        1.0,
        Overprint::Off,
    );
    let m = plain.coverage_masks(&rules());
    assert_eq!((m[0].flagged, m[1].flagged), (0, 0));
}

#[test]
fn half_alpha_normal_in_an_isolated_group_lands_at_half_tint() {
    let mut c = InkCanvas::new(1, 1, &[], LIMITS).unwrap();
    c.begin_group(Group {
        isolated: true,
        knockout: false,
        alpha: 1.0,
        blend: Blend::Normal,
    })
    .unwrap();
    paint(&mut c, &C100, 0.5, Overprint::Off);
    c.end_group().unwrap();
    assert_near(plane(&c, 0), 0.5);
    assert_near(plane(&c, 1), 0.0);
}

#[test]
fn a_knockout_group_paints_its_last_element_over_the_backdrop_alone() {
    let mut c = InkCanvas::new(1, 1, &[], LIMITS).unwrap();
    c.begin_group(Group {
        isolated: true,
        knockout: true,
        alpha: 1.0,
        blend: Blend::Normal,
    })
    .unwrap();
    paint(&mut c, &M100, 1.0, Overprint::Off);
    paint(&mut c, &C100, 0.5, Overprint::Off);
    c.end_group().unwrap();
    assert_near(plane(&c, 0), 0.5);
    assert_near(plane(&c, 1), 0.0);
}

#[test]
fn spans_mark_only_their_pixels() {
    let mut c = InkCanvas::new(4, 2, &[], LIMITS).unwrap();
    let spans = [Span {
        y: 1,
        x0: 1,
        x1: 3,
        coverage: 1.0,
    }];
    c.paint(&Paint {
        coverage: Coverage::Spans(&spans),
        tints: &C100,
        alpha: 1.0,
        blend: Blend::Normal,
        overprint: Overprint::Off,
    })
    .unwrap();
    assert_eq!(
        c.landed(0).unwrap(),
        vec![0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0]
    );
}

#[test]
fn an_oversized_page_is_refused() {
    let err = InkCanvas::new(100_000, 100_000, &[], Limits::new(1 << 30, 8)).err();
    assert!(matches!(err, Some(InkError::TooLarge { .. })), "{err:?}");
    let err = InkCanvas::new(u32::MAX, u32::MAX, &[], Limits::new(u64::MAX, 8)).err();
    assert!(matches!(err, Some(InkError::TooLarge { .. })), "{err:?}");
}

#[test]
fn too_many_colorants_is_refused() {
    let err = InkCanvas::new(1, 1, &["A", "B"], Limits::new(1 << 20, 5)).err();
    assert_eq!(err, Some(InkError::TooManyColorants { count: 6, limit: 5 }));
    let err = InkCanvas::new(1, 1, &["A", "B", "C", "D", "E"], Limits::new(1 << 20, 64)).err();
    assert_eq!(err, Some(InkError::TooManyColorants { count: 9, limit: 8 }));
    let err = InkCanvas::new(1, 1, &["Cyan"], LIMITS).err();
    assert_eq!(err, Some(InkError::DuplicateColorant("Cyan".into())));
}

#[test]
fn a_group_over_the_ceiling_is_refused() {
    // One layer is 80 bytes a pixel: 9 planes, 9 initial-backdrop planes,
    // group alpha and group shape, as f32.
    let one_layer = 16 * 16 * 80;
    let mut c = InkCanvas::new(16, 16, &[], Limits::new(one_layer, 8)).unwrap();
    let group = Group {
        isolated: true,
        knockout: false,
        alpha: 1.0,
        blend: Blend::Normal,
    };
    assert!(matches!(
        c.begin_group(group),
        Err(InkError::TooLarge { .. })
    ));
    assert_eq!(c.end_group(), Err(InkError::NoOpenGroup));
}

#[test]
fn a_wrong_sized_mask_is_refused() {
    let mut c = InkCanvas::new(2, 2, &[], LIMITS).unwrap();
    let err = c.paint(&Paint {
        coverage: Coverage::Mask(&FULL),
        tints: &C100,
        alpha: 1.0,
        blend: Blend::Normal,
        overprint: Overprint::Off,
    });
    assert_eq!(
        err,
        Err(InkError::MaskSize {
            expected: 4,
            actual: 1
        })
    );
}
