//! Tests for `pdfcer_core::dimension::units`, run against its public API.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_cmp
)]

use pdfcer_core::dimension::units::*;

// ---- decimal formatting across the metric/decimal units ----

#[test]
fn decimal_units_format_with_fixed_precision() {
    assert_eq!(
        NumberFormat::decimal(Unit::Millimeter, 2).format(12.5),
        "12.50 mm"
    );
    assert_eq!(
        NumberFormat::decimal(Unit::Centimeter, 2).format(1.25),
        "1.25 cm"
    );
    assert_eq!(NumberFormat::decimal(Unit::Meter, 3).format(3.1), "3.100 m");
    assert_eq!(NumberFormat::decimal(Unit::Inch, 2).format(6.5), "6.50 in");
    assert_eq!(
        NumberFormat::decimal(Unit::DecimalFeet, 2).format(12.5),
        "12.50 ft"
    );
}

// ---- inch fractions (kept, not reduced by default) ----

#[test]
fn inch_fraction_keeps_denominator() {
    assert_eq!(NumberFormat::inch_fraction(8).format(6.5), "6 4/8 in");
    assert_eq!(NumberFormat::inch_fraction(16).format(2.25), "2 4/16 in");
    // Exactly on a whole inch → no fraction.
    assert_eq!(NumberFormat::inch_fraction(8).format(3.0), "3 in");
}

#[test]
fn inch_fraction_reduces_when_asked() {
    let f = NumberFormat {
        unit: Unit::Inch,
        fraction: FractionMode::Fraction {
            denominator: 8,
            reduce: true,
        },
        decimal_marker: DecimalMarker::Point,
    };
    assert_eq!(f.format(6.5), "6 1/2 in");
}

// ---- feet-inches (the exceed-Acrobat case) ----

#[test]
fn feet_inches_basic_cases() {
    let f = NumberFormat::feet_inches(8, false);
    assert_eq!(f.format(12.5), "12'-6\"");
    assert_eq!(f.format(4.0), "4'-0\"");
    // 12.5417 ft = 12 ft 6.5 in → 6 4/8"
    assert_eq!(f.format(12.0 + 6.5 / 12.0), "12'-6 4/8\"");
}

#[test]
fn feet_inches_rolls_over_inches_to_feet() {
    // 11.999 ft rounds the inches to 12 → carry to 12 ft 0 in.
    let f = NumberFormat::feet_inches(8, false);
    // 11 ft + 11.98 in → nearest 1/8 of 11.98 is 12.0 in → 12'-0"
    let v = 11.0 + 11.98 / 12.0;
    assert_eq!(f.format(v), "12'-0\"");
}

#[test]
fn feet_inches_negative() {
    let f = NumberFormat::feet_inches(8, false);
    assert_eq!(f.format(-3.5), "-3'-6\"");
}

// ---- the value model: measured_points × scale ----

#[test]
fn format_measurement_scales_and_discloses_raw() {
    let fmt = NumberFormat::decimal(Unit::DecimalFeet, 2);
    // 144 pt at 0.25 ft/pt = 36 ft.
    let d = format_measurement(144.0, ScaleState::Calibrated { scale: 0.25 }, fmt);
    assert_eq!(d.text, "36.00 ft");
    assert!(!d.raw_page_units);
    // Never-set → raw points, disclosed.
    let raw = format_measurement(144.0, ScaleState::NeverSet, fmt);
    assert_eq!(raw.text, "144.00 pt");
    assert!(raw.raw_page_units);
}

#[test]
fn one_to_one_uses_the_unit_baseline_and_is_distinct_from_never_set() {
    // 72 points at true 1:1 is exactly one inch.
    let fmt = NumberFormat::decimal(Unit::Inch, 3);
    let d = format_measurement(72.0, ScaleState::OneToOne, fmt);
    assert_eq!(d.text, "1.000 in");
    assert!(!d.raw_page_units);
    // OneToOne and NeverSet are genuinely different states (ui-spec §4.3).
    assert!(ScaleState::NeverSet.is_never_set());
    assert!(!ScaleState::OneToOne.is_never_set());
    assert_eq!(
        ScaleState::OneToOne.effective_scale(Unit::Meter),
        Some(0.0254 / 72.0)
    );
    assert_eq!(ScaleState::NeverSet.effective_scale(Unit::Meter), None);
}

// ---- scale back-calculation, both paths ----

#[test]
fn real_length_back_calc_is_length_over_drawn() {
    let p = preview_group_scale(ScaleEntry::RealLength {
        drawn_pdf_length: 42.3,
        real_length: 25.0,
        unit: Unit::DecimalFeet,
    })
    .unwrap();
    assert!((p.scale - 25.0 / 42.3).abs() < 1e-12);
    assert_eq!(p.unit, Unit::DecimalFeet);
    assert_eq!(p.ratio_label, "25.00 ft = 42.30 pt");
}

#[test]
fn ratio_back_calc_uses_the_paper_basis() {
    // 1:100 on the inch basis: 1 point = 1/72 in paper = 100/72 in real.
    let p = preview_group_scale(ScaleEntry::Ratio {
        paper: 1.0,
        real: 100.0,
        basis: Unit::Inch,
    })
    .unwrap();
    assert!((p.scale - 100.0 / 72.0).abs() < 1e-12);
    assert_eq!(p.unit, Unit::Inch);
    assert_eq!(p.ratio_label, "1:100");
    // A ratio on a mm basis scales by the mm baseline.
    let mm = preview_group_scale(ScaleEntry::Ratio {
        paper: 1.0,
        real: 50.0,
        basis: Unit::Millimeter,
    })
    .unwrap();
    assert!((mm.scale - 50.0 * (25.4 / 72.0)).abs() < 1e-12);
}

#[test]
fn scale_change_repropagates_the_displayed_value() {
    // The "change the group scale → all member dimensions update" story,
    // at the value-model level: the SAME stored geometry (100 pt) yields
    // different displayed values under different scales.
    let fmt = NumberFormat::decimal(Unit::Meter, 2);
    let before = format_measurement(100.0, ScaleState::Calibrated { scale: 0.01 }, fmt);
    assert_eq!(before.text, "1.00 m");
    let after = format_measurement(100.0, ScaleState::Calibrated { scale: 0.05 }, fmt);
    assert_eq!(after.text, "5.00 m");
}

#[test]
fn degenerate_scale_entries_return_none() {
    assert!(
        preview_group_scale(ScaleEntry::RealLength {
            drawn_pdf_length: 0.0,
            real_length: 25.0,
            unit: Unit::Meter,
        })
        .is_none()
    );
    assert!(
        preview_group_scale(ScaleEntry::RealLength {
            drawn_pdf_length: f64::NAN,
            real_length: 25.0,
            unit: Unit::Meter,
        })
        .is_none()
    );
    assert!(
        preview_group_scale(ScaleEntry::Ratio {
            paper: 0.0,
            real: 100.0,
            basis: Unit::Inch,
        })
        .is_none()
    );
}

#[test]
fn unit_parse_round_trips_its_token() {
    // `.iter().copied()` since `G013` turned `all()` into a slice. This
    // line failing to compile is the intended cost of that change, paid
    // once here and once in the consuming project.
    for u in Unit::all().iter().copied() {
        assert_eq!(Unit::parse(u.token()), Some(u), "token {}", u.token());
    }
    assert!(Unit::parse("furlong").is_none());
}

/// `all()` LISTS EVERY VARIANT — and this test fails to COMPILE if a
/// future unit is added without being listed.
///
/// The `match` below is exhaustive over `Unit`, so adding a variant is a
/// compile error here until someone names it; naming it then asserts it
/// is in `all()`. A plain `assert_eq!(all().len(), 9)` would not do this
/// — it would go red with a number to bump, which is the kind of failure
/// people fix by bumping the number.
///
/// Why it matters more now than it did before `G013`: while `all()`
/// returned `[Unit; 6]`, forgetting to add a new variant to it was a
/// compile error at the array's own type. Widening it to a slice bought
/// callers their compatibility and **took that guard away**, so it has to
/// be replaced rather than simply lost. `Unit` stays exhaustive (not
/// `#[non_exhaustive]`) at the consuming project's explicit request, for
/// the same reason: they want their own `match`es to break.
#[test]
fn all_contains_every_variant() {
    for u in Unit::all().iter().copied() {
        // Exhaustive on purpose — see this test's doc comment.
        let named = match u {
            Unit::Millimeter
            | Unit::Centimeter
            | Unit::Meter
            | Unit::Kilometer
            | Unit::Inch
            | Unit::DecimalFeet
            | Unit::FeetInches
            | Unit::Yard
            | Unit::Mile => u,
        };
        assert!(
            Unit::all().contains(&named),
            "{} is a Unit variant that all() does not list",
            named.token()
        );
    }
    assert_eq!(
        Unit::all().len(),
        9,
        "all() must list each variant exactly once"
    );
}

/// The `G013` units convert the way the definitions say, checked against
/// the metre rather than against a transcribed decimal.
///
/// A label is not checkable by any reader (see `Unit::abbrev`), so the
/// FACTOR is the only half a test can defend. 1 km = 1000 m, 1 mi =
/// 1609.344 m exactly (international mile), 1 yd = 0.9144 m exactly.
#[test]
fn new_units_convert_by_their_definitions() {
    let m = Unit::Meter.baseline_per_point();
    let rel = |a: f64, b: f64| (a - b).abs() / b;

    assert!(
        rel(Unit::Kilometer.baseline_per_point(), m / 1000.0) < 1e-12,
        "a kilometre must be 1000 metres"
    );
    assert!(
        rel(Unit::Mile.baseline_per_point(), m / 1609.344) < 1e-12,
        "a statute mile must be 1609.344 m exactly"
    );
    assert!(
        rel(Unit::Yard.baseline_per_point(), m / 0.9144) < 1e-12,
        "a yard must be 0.9144 m exactly"
    );

    // And the imperial chain closes: 1760 yd to the mile, 3 ft to the yd.
    assert!(
        rel(
            Unit::Mile.baseline_per_point() * 1760.0,
            Unit::Yard.baseline_per_point()
        ) < 1e-12,
        "1760 yards to the mile"
    );
    assert!(
        rel(
            Unit::Yard.baseline_per_point() * 3.0,
            Unit::DecimalFeet.baseline_per_point()
        ) < 1e-12,
        "3 feet to the yard"
    );
}

/// Every unit's `/U` label is distinct, because `/U` carries no
/// arithmetic and a duplicate would be a silently wrong label on a
/// correct number (ISO 32000-1 §12.9 Table 263).
///
/// `DecimalFeet` and `FeetInches` are the one deliberate pair: both are
/// feet, and the second's inch part carries its own label.
#[test]
fn every_unit_has_a_distinct_abbrev_except_the_two_that_are_both_feet() {
    let mut seen: Vec<&'static str> = Vec::new();
    for u in Unit::all().iter().copied() {
        if u == Unit::FeetInches {
            continue;
        }
        assert!(
            !seen.contains(&u.abbrev()),
            "{} duplicates the abbreviation {}",
            u.token(),
            u.abbrev()
        );
        seen.push(u.abbrev());
    }
    assert_eq!(Unit::DecimalFeet.abbrev(), Unit::FeetInches.abbrev());
}

#[test]
fn non_finite_value_formats_as_dash_not_panic() {
    assert_eq!(NumberFormat::decimal(Unit::Meter, 2).format(f64::NAN), "—");
    assert_eq!(
        NumberFormat::feet_inches(8, false).format(f64::INFINITY),
        "—"
    );
}
