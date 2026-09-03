//! Deriving a DXF export scale from the page's **ce dimensions**
//! (`Pass 52.2` substrate).
//!
//! ## What is being protected
//!
//! A PDF drawing is at *paper* scale. A 1:2 detail exported at face value
//! arrives at half real size, and — this is the part that makes it
//! dangerous rather than merely wrong — it **looks entirely plausible**.
//! Nothing about the resulting DXF says it is half size. The operator finds
//! out at the cutting table.
//!
//! Every generic PDF→DXF converter has this problem and none of them can
//! solve it, because the scale is not in the file. pdfcer can, because it
//! already asked: the measure tool's *scale by known dimension* takes the
//! length the drawing itself prints for a feature and derives the factor.
//! [`suggest_scale`] is the bridge from that answer to
//! [`DxfOptions::scale`].
//!
//! ## The three cases, and why the third is the reason this is a test file
//!
//! `Uncalibrated` and `Calibrated` are the obvious two. `Conflicting` is
//! the one that would otherwise be handled by accident: a sheet carrying a
//! 1:1 plan **and** a 1:5 detail is an ordinary drawing, and DXF has one
//! scale. Silently taking the first group's answer exports half the sheet
//! wrong — and, again, plausibly.
//!
//! ## Unit-independence is load-bearing, not incidental
//!
//! `ScaleState::effective_scale` answers *"how many of the group's display
//! units is one PDF point?"*, which is a **different number** for a
//! millimetre group than for an inch group describing the same 1:1
//! drawing (0.3528 vs 0.01389). Comparing those raw would report a
//! conflict between two groups that agree perfectly. The division by the
//! unit's own baseline is what cancels the unit out, and
//! `two_groups_in_different_units_describing_one_scale_do_not_conflict`
//! is the assertion that it actually does.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::dimension::{
    DimensionKind, DimensionModel, GroupId, NumberFormat, ScaleState, Unit,
};
use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::export::dxf::{
    DxfScaleSuggestion, DxfUnits, suggest_scale, suggest_scale_for_groups,
};
use pdfcer_core::vector::{AxisConstraint, Point};

/// The relative slack the assertions allow. The conversions are exact in
/// binary for inches (1/72) but not for millimetres (25.4/72), so an exact
/// comparison would be testing float representation rather than the
/// arithmetic.
const EPS: f64 = 1e-12;

fn assert_close(got: f64, want: f64, what: &str) {
    assert!(
        (got - want).abs() <= EPS * want.abs().max(1.0),
        "{what}: expected {want}, got {got}"
    );
}

// ---------------------------------------------------------------------------
// Uncalibrated
// ---------------------------------------------------------------------------

/// **A fresh document infers nothing — it does not infer 1.0.**
///
/// The distinction is the whole point. `Uncalibrated` makes the caller say
/// *"pdfcer does not know the scale of this drawing"*; a `1.0` fallback
/// would let it say nothing at all and export at paper scale, which is the
/// failure this feature exists to prevent.
#[test]
fn a_document_with_no_calibrated_group_infers_nothing_rather_than_one() {
    let model = DimensionModel::new();
    assert_eq!(
        suggest_scale(&model),
        DxfScaleSuggestion::Uncalibrated,
        "the default group's scale is NeverSet, so there is nothing to infer"
    );
}

// ---------------------------------------------------------------------------
// Calibrated
// ---------------------------------------------------------------------------

/// **A group calibrated to a 1:2 view yields scale 2.0.**
///
/// 1 pt = 25.4/36 mm is twice the true-scale 25.4/72, i.e. the drawing
/// shows a feature at half its real size, so real-units-per-paper-unit is
/// 2.
#[test]
fn a_one_to_two_millimetre_group_yields_scale_two() {
    let mut model = DimensionModel::new();
    let g = model.add_group("Detail", Unit::Millimeter);
    model.set_group_scale(
        g,
        ScaleState::Calibrated { scale: 25.4 / 36.0 },
        NumberFormat::decimal(Unit::Millimeter, 2),
    );
    match suggest_scale(&model) {
        DxfScaleSuggestion::Calibrated {
            scale,
            units,
            group,
            agreeing,
        } => {
            assert_close(scale, 2.0, "a 1:2 view");
            assert_eq!(units, DxfUnits::Millimetres, "a millimetre group");
            assert_eq!(group, "Detail", "named so the disclosure can cite it");
            assert_eq!(agreeing, 1);
        }
        other => panic!("expected Calibrated, got {other:?}"),
    }
}

/// **An explicit 1:1 is a real answer, distinct from never-set.**
///
/// `ScaleState` is deliberately tri-state (ui-spec §4.3) precisely so a
/// deliberate full-size drawing is not confused with an uncalibrated one.
/// If that distinction were lost here, an operator who had explicitly
/// confirmed 1:1 would still be told pdfcer did not know.
#[test]
fn an_explicit_one_to_one_group_is_calibrated_at_scale_one() {
    let mut model = DimensionModel::new();
    let g = model.add_group("Full size", Unit::Inch);
    model.set_group_scale(
        g,
        ScaleState::OneToOne,
        NumberFormat::decimal(Unit::Inch, 3),
    );
    match suggest_scale(&model) {
        DxfScaleSuggestion::Calibrated { scale, units, .. } => {
            assert_close(scale, 1.0, "an explicit 1:1");
            assert_eq!(units, DxfUnits::Inches);
        }
        other => panic!("expected Calibrated, got {other:?}"),
    }
}

/// **Two groups in DIFFERENT units describing the same scale agree.**
///
/// The assertion the unit-cancellation exists for. A millimetre group and
/// an inch group on the same 1:1 sheet have `effective_scale` values of
/// 25.4/72 ≈ 0.3528 and 1/72 ≈ 0.01389 — a factor of 25.4 apart. Compared
/// raw, that is a conflict, and the operator would be asked to resolve a
/// disagreement that does not exist.
#[test]
fn two_groups_in_different_units_describing_one_scale_do_not_conflict() {
    let mut model = DimensionModel::new();
    let mm = model.add_group("Plan (mm)", Unit::Millimeter);
    let inch = model.add_group("Plan (in)", Unit::Inch);
    model.set_group_scale(
        mm,
        ScaleState::OneToOne,
        NumberFormat::decimal(Unit::Millimeter, 1),
    );
    model.set_group_scale(
        inch,
        ScaleState::OneToOne,
        NumberFormat::decimal(Unit::Inch, 3),
    );
    match suggest_scale(&model) {
        DxfScaleSuggestion::Calibrated {
            scale, agreeing, ..
        } => {
            assert_close(scale, 1.0, "both describe full size");
            assert_eq!(
                agreeing, 2,
                "corroboration is counted; it is not a second answer"
            );
        }
        other => panic!(
            "two groups that agree must not be reported as a conflict; got {other:?} — \
             this is the unit-cancellation failing"
        ),
    }
}

// ---------------------------------------------------------------------------
// Conflicting
// ---------------------------------------------------------------------------

/// **A 1:1 plan and a 1:5 detail on one sheet is a conflict, not a pick.**
///
/// This is an ordinary drawing, not a malformed one, and DXF carries one
/// scale. Choosing either group silently exports the other half of the
/// sheet wrong by a factor of five — and the result looks fine.
#[test]
fn groups_calibrated_to_different_scales_conflict_rather_than_first_winning() {
    let mut model = DimensionModel::new();
    let plan = model.add_group("Plan", Unit::Millimeter);
    let detail = model.add_group("Detail 1:5", Unit::Millimeter);
    model.set_group_scale(
        plan,
        ScaleState::OneToOne,
        NumberFormat::decimal(Unit::Millimeter, 1),
    );
    model.set_group_scale(
        detail,
        ScaleState::Calibrated {
            scale: 5.0 * 25.4 / 72.0,
        },
        NumberFormat::decimal(Unit::Millimeter, 1),
    );
    match suggest_scale(&model) {
        DxfScaleSuggestion::Conflicting { candidates } => {
            assert_eq!(candidates.len(), 2, "both opinions must be reported");
            // Group order, so a caller's list does not reshuffle between
            // calls under the operator's cursor.
            assert_eq!(candidates[0].group, "Plan");
            assert_eq!(candidates[1].group, "Detail 1:5");
            assert_close(candidates[0].scale, 1.0, "the plan");
            assert_close(candidates[1].scale, 5.0, "the detail");
        }
        other => panic!("a disagreement must be surfaced, not resolved silently; got {other:?}"),
    }
}

/// **A never-set group alongside a calibrated one is not a conflict.**
///
/// `NeverSet` is the absence of an opinion, not a competing one. Treating
/// it as a candidate would make the default group — which every document
/// has, and which starts never-set — conflict with the first real
/// calibration the operator performs, i.e. it would fire on the single
/// most common case there is.
#[test]
fn a_never_set_group_abstains_instead_of_conflicting() {
    let mut model = DimensionModel::new();
    // `DimensionModel::new()` already carries a never-set "Default" group;
    // this adds a second so the abstention is not a one-off.
    let _quiet = model.add_group("Not calibrated", Unit::Meter);
    let g = model.add_group("Measured", Unit::Inch);
    model.set_group_scale(
        g,
        ScaleState::Calibrated { scale: 2.0 / 72.0 },
        NumberFormat::decimal(Unit::Inch, 3),
    );
    match suggest_scale(&model) {
        DxfScaleSuggestion::Calibrated {
            scale,
            agreeing,
            group,
            ..
        } => {
            assert_close(scale, 2.0, "the one group that has an opinion");
            assert_eq!(agreeing, 1, "the two never-set groups do not count");
            assert_eq!(group, "Measured");
        }
        other => panic!("expected Calibrated, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Page scope
// ---------------------------------------------------------------------------
//
// `suggest_scale` reads the WHOLE model. On a one-page drawing that is the
// same thing as reading the page; on a sheet set it is not, and the gap is
// not academic — it is the difference between a DXF cut at 1:1 and one cut
// at 1:5. `suggest_scale_for_groups` narrows the inference to the groups
// that are actually ON the page(s) being written, and
// `EditSession::dimension_groups_on_page` is what resolves that ownership
// (through each ce dimension's annotation `/P`, §12.5.2 — the sidecar
// deliberately does not record a page).

/// A two-page PDF: catalog(1) → pages(2) → page(3), page(4).
///
/// Two pages is the minimum that can express the defect at all. Everything
/// below it — one page, or a model with no document — cannot distinguish
/// document-wide from page-scoped, which is exactly why the limitation
/// survived the first slice's six tests.
fn two_page_pdf() -> Vec<u8> {
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 400] /Resources << >> >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 400] /Resources << >> >>",
    ];
    let mut buf = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref_at = buf.len();
    let size = bodies.len() + 1;
    buf.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f \n").as_bytes());
    for off in &offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n")
            .as_bytes(),
    );
    buf
}

/// A horizontal ce dimension somewhere inside the 400×400 page box.
fn linear() -> DimensionKind {
    DimensionKind::Linear {
        a: Point::new(100.0, 200.0),
        b: Point::new(300.0, 200.0),
        constraint: AxisConstraint::Horizontal,
        offset: 0.0,
        text_along: 0.0,
    }
}

/// A two-page session where page 0 carries an UNCALIBRATED group and page 1
/// carries a 1:5 one. The shape of a real sheet set: the operator has
/// calibrated the detail sheet and not yet touched the general arrangement.
fn sheet_set() -> (EditSession, GroupId, GroupId) {
    let doc = Document::from_bytes(two_page_pdf()).unwrap();
    let mut s = EditSession::new(doc);
    let plan = s.add_dimension_group("Plan", Unit::Millimeter).unwrap();
    let detail = s
        .add_dimension_group("Detail 1:5", Unit::Millimeter)
        .unwrap();
    s.set_group_scale(
        detail,
        ScaleState::Calibrated {
            scale: 5.0 * 25.4 / 72.0,
        },
        NumberFormat::decimal(Unit::Millimeter, 1),
    )
    .unwrap();
    s.add_dimension(0, plan, linear()).unwrap();
    s.add_dimension(1, detail, linear()).unwrap();
    (s, plan, detail)
}

/// **A page reports only the groups whose dimensions are on it.**
///
/// The accessor's whole contract in one assertion. If this ever returns
/// both groups for either page, every page-scoped guarantee below it is
/// vacuous while still passing.
#[test]
fn dimension_groups_on_page_reports_only_that_pages_groups() {
    let (s, plan, detail) = sheet_set();
    assert_eq!(
        s.dimension_groups_on_page(0),
        vec![plan],
        "page 0 carries the plan dimension and nothing else"
    );
    assert_eq!(
        s.dimension_groups_on_page(1),
        vec![detail],
        "page 1 carries the detail dimension and nothing else"
    );
    assert!(
        s.dimension_groups_on_page(9).is_empty(),
        "an out-of-range page has no groups rather than erroring"
    );
}

/// **★ THE DEFECT. An uncalibrated page must not inherit another page's
/// scale.**
///
/// Document-wide, page 0 has exactly one calibrated group in the model —
/// the 1:5 detail on page 1 — so `suggest_scale` reports `Calibrated{5.0}`
/// and an export of page 0 comes out five times real size with nothing
/// anywhere saying so. That is the plausible-looking wrong answer this
/// whole feature exists to prevent, arriving through the feature itself.
///
/// Both halves are asserted deliberately: the document-wide reading is
/// pinned as the WRONG answer so that a future "simplification" back to
/// `suggest_scale` fails here loudly instead of silently reintroducing it.
#[test]
fn an_uncalibrated_page_does_not_inherit_another_pages_scale() {
    let (s, ..) = sheet_set();
    let model = s.dimension_model();

    match suggest_scale(&model) {
        DxfScaleSuggestion::Calibrated { scale, .. } => {
            assert_close(scale, 5.0, "the document-wide reading — this is the trap");
        }
        other => panic!("expected the document-wide reading to find the detail; got {other:?}"),
    }

    assert_eq!(
        suggest_scale_for_groups(&model, &s.dimension_groups_on_page(0)),
        DxfScaleSuggestion::Uncalibrated,
        "page 0's own dimension is uncalibrated, so pdfcer does not know its scale — \
         inheriting page 1's 1:5 would export it five times real size, plausibly"
    );
}

/// **A page that IS calibrated is not held hostage by another page.**
///
/// The mirror failure, and the one an operator would report as a bug
/// rather than discover at the cutting table: page 1 is unambiguously 1:5,
/// but a second sheet at 1:2 makes the document-wide reading `Conflicting`,
/// so the CLI refuses and the GUI disables Export — for a page that has
/// exactly one answer.
#[test]
fn a_calibrated_page_is_not_refused_because_another_page_disagrees() {
    let (mut s, plan, _detail) = sheet_set();
    // Give the plan group a scale too, so the DOCUMENT now holds two
    // different calibrated answers while each PAGE still holds one.
    s.set_group_scale(
        plan,
        ScaleState::Calibrated {
            scale: 2.0 * 25.4 / 72.0,
        },
        NumberFormat::decimal(Unit::Millimeter, 1),
    )
    .unwrap();
    let model = s.dimension_model();

    assert!(
        matches!(
            suggest_scale(&model),
            DxfScaleSuggestion::Conflicting { .. }
        ),
        "document-wide, the sheet set genuinely disagrees with itself"
    );
    for (page, want) in [(0usize, 2.0), (1, 5.0)] {
        match suggest_scale_for_groups(&model, &s.dimension_groups_on_page(page)) {
            DxfScaleSuggestion::Calibrated { scale, .. } => {
                assert_close(scale, want, "each page on its own has one answer");
            }
            other => panic!("page {page} should be unambiguous; got {other:?}"),
        }
    }
}

/// **Two selected pages that disagree ARE a conflict.**
///
/// The multi-page export case. Asking for both sheets at once is asking
/// for one DXF scale to serve both, and there is no such number — so the
/// same variant that stops a single ambiguous page stops this, through the
/// same code path rather than through a special case bolted onto the
/// caller.
#[test]
fn a_multi_page_selection_that_disagrees_conflicts() {
    let (mut s, plan, _detail) = sheet_set();
    s.set_group_scale(
        plan,
        ScaleState::Calibrated {
            scale: 2.0 * 25.4 / 72.0,
        },
        NumberFormat::decimal(Unit::Millimeter, 1),
    )
    .unwrap();
    let model = s.dimension_model();
    let mut both = s.dimension_groups_on_page(0);
    both.extend(s.dimension_groups_on_page(1));

    match suggest_scale_for_groups(&model, &both) {
        DxfScaleSuggestion::Conflicting { candidates } => {
            assert_eq!(candidates.len(), 2, "both pages' opinions are reported");
        }
        other => panic!("two pages at different scales cannot share one DXF scale; got {other:?}"),
    }
}

/// **An empty group list infers nothing rather than 1.0.**
///
/// The boundary the GUI hits constantly: a page with no ce dimensions on
/// it at all. It must land in the same `Uncalibrated` disclosure as a page
/// whose dimensions are merely uncalibrated — pdfcer does not know, and the
/// two reasons for not knowing do not change what the operator must be
/// told.
#[test]
fn a_page_with_no_dimensions_infers_nothing() {
    let mut model = DimensionModel::new();
    let g = model.add_group("Elsewhere", Unit::Inch);
    model.set_group_scale(
        g,
        ScaleState::Calibrated { scale: 3.0 / 72.0 },
        NumberFormat::decimal(Unit::Inch, 3),
    );
    assert_eq!(
        suggest_scale_for_groups(&model, &[]),
        DxfScaleSuggestion::Uncalibrated,
        "no groups on the page means no evidence about the page"
    );
}

/// **A stale group id is ignored, not an error.**
///
/// A `GroupId` naming a group that no longer exists carries the same
/// amount of evidence as no group at all, and a shell holding a list one
/// frame out of date is ordinary in immediate mode rather than a fault.
#[test]
fn an_unknown_group_id_is_ignored() {
    let mut model = DimensionModel::new();
    let g = model.add_group("Real", Unit::Millimeter);
    model.set_group_scale(
        g,
        ScaleState::OneToOne,
        NumberFormat::decimal(Unit::Millimeter, 1),
    );
    match suggest_scale_for_groups(&model, &[g, GroupId(9999)]) {
        DxfScaleSuggestion::Calibrated {
            scale, agreeing, ..
        } => {
            assert_close(scale, 1.0, "the group that exists");
            assert_eq!(agreeing, 1, "the phantom id contributes nothing");
        }
        other => panic!("expected Calibrated, got {other:?}"),
    }
}
