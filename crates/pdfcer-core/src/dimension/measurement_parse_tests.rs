#![allow(clippy::unwrap_used, clippy::float_cmp)]

use super::*;
use crate::dimension::{Unit, format_measurement_on};
use crate::object::ObjId;
use crate::page_tree::Rect;

fn page(user_unit: f64) -> Page {
    let sheet = Rect::from_corners(0.0, 0.0, 612.0, 792.0);
    let mut p = Page::with_boxes(ObjId::new(3, 0), sheet, sheet, 0);
    p.user_unit = user_unit;
    p
}

fn comma(mut f: NumberFormat) -> NumberFormat {
    f.decimal_marker = DecimalMarker::Comma;
    f
}

fn read(p: &Page, t: &str, s: ScaleState, f: NumberFormat) -> f64 {
    user_units_from_measurement_on(p, t, s, f).unwrap()
}

const M_1_50: ScaleState = ScaleState::Calibrated {
    scale: 50.0 * 0.0254 / 72.0,
};

#[test]
fn every_readout_reads_back_as_itself() {
    let ft_1_48 = ScaleState::Calibrated {
        scale: 48.0 / 864.0,
    };
    let cases = [
        (M_1_50, NumberFormat::decimal(Unit::Meter, 2)),
        (M_1_50, comma(NumberFormat::decimal(Unit::Meter, 3))),
        (
            ScaleState::OneToOne,
            NumberFormat::decimal(Unit::Millimeter, 1),
        ),
        (ScaleState::OneToOne, NumberFormat::inch_fraction(16)),
        (ft_1_48, NumberFormat::feet_inches(8, false)),
        (ft_1_48, NumberFormat::decimal(Unit::DecimalFeet, 2)),
        (
            ScaleState::Calibrated { scale: 1.0 },
            NumberFormat::decimal(Unit::Kilometer, 3),
        ),
        (
            ScaleState::Calibrated { scale: 0.5 },
            NumberFormat::decimal(Unit::Yard, 2),
        ),
        (
            ScaleState::Calibrated { scale: 0.01 },
            NumberFormat::decimal(Unit::Mile, 4),
        ),
        (ScaleState::NeverSet, NumberFormat::decimal(Unit::Meter, 2)),
        (
            ScaleState::NeverSet,
            comma(NumberFormat::decimal(Unit::Meter, 2)),
        ),
    ];
    for user_unit in [1.0, 2.0, 0.75] {
        let p = page(user_unit);
        for (s, f) in cases {
            for x in [0.0, 1.0, 37.25, 144.0, 611.9, -20.5] {
                let shown = format_measurement_on(&p, x, s, f).text;
                let back = read(&p, &shown, s, f);
                assert_eq!(
                    format_measurement_on(&p, back, s, f).text,
                    shown,
                    "{s:?} {f:?} uu={user_unit} x={x}"
                );
            }
        }
    }
}

#[test]
fn user_unit_divides_the_length() {
    let f = NumberFormat::decimal(Unit::Meter, 2);
    let one = read(&page(1.0), "2.4", M_1_50, f);
    let two = read(&page(2.0), "2.4", M_1_50, f);
    assert!((one - 2.0 * two).abs() < 1e-9);
    assert!((one - 2.4 / (50.0 * 0.0254 / 72.0)).abs() < 1e-9);
}

#[test]
fn a_typed_unit_other_than_the_top_unit_is_converted() {
    let p = page(1.0);
    let ft = ScaleState::Calibrated {
        scale: 48.0 / 864.0,
    };
    let f = NumberFormat::feet_inches(8, false);
    let feet = read(&p, "10'", ft, f);
    let mm = read(&p, "3048 mm", ft, f);
    assert!((feet - mm).abs() < 1e-9, "{feet} {mm}");
    let bare = read(&p, "10", ft, f);
    assert!((feet - bare).abs() < 1e-9);
}

#[test]
fn one_to_one_reads_the_typed_unit_at_true_scale() {
    let p = page(1.0);
    let f = NumberFormat::decimal(Unit::Meter, 2);
    assert!((read(&p, "1 in", ScaleState::OneToOne, f) - 72.0).abs() < 1e-9);
    assert!((read(&p, "25.4 mm", ScaleState::OneToOne, f) - 72.0).abs() < 1e-9);
}

#[test]
fn never_set_reads_points_or_a_paper_length() {
    let p = page(2.0);
    let f = NumberFormat::decimal(Unit::Meter, 2);
    assert_eq!(read(&p, "144", ScaleState::NeverSet, f), 72.0);
    assert_eq!(read(&p, "144.00 pt", ScaleState::NeverSet, f), 72.0);
    assert!((read(&p, "1 in", ScaleState::NeverSet, f) - 36.0).abs() < 1e-9);
}

#[test]
fn the_decimal_marker_is_the_formats() {
    let p = page(1.0);
    let iso = comma(NumberFormat::decimal(Unit::Meter, 2));
    let ansi = NumberFormat::decimal(Unit::Meter, 2);
    assert_eq!(read(&p, "2,4", M_1_50, iso), read(&p, "2.4", M_1_50, ansi));
    assert_eq!(
        read(&p, "2,4 m", M_1_50, iso),
        read(&p, "2.4", M_1_50, ansi)
    );
}

#[test]
fn a_pt_suffix_is_printed_points_at_any_scale() {
    let p = page(1.0);
    let f = NumberFormat::decimal(Unit::Meter, 2);
    assert_eq!(read(&p, "72 pt", M_1_50, f), 72.0);
    assert_eq!(read(&p, "-72pt", M_1_50, f), -72.0);
}

#[test]
fn bad_text_and_bad_scales_are_errors() {
    let p = page(1.0);
    let f = NumberFormat::decimal(Unit::Meter, 2);
    for t in ["", "-", "abc", "--4", "3 mm pt"] {
        assert!(
            user_units_from_measurement_on(&p, t, M_1_50, f).is_err(),
            "{t:?}"
        );
    }
    for scale in [0.0, -1.0, f64::NAN] {
        assert!(matches!(
            user_units_from_measurement_on(&p, "1", ScaleState::Calibrated { scale }, f),
            Err(LengthParseError::UnusableScale { .. })
        ));
    }
    assert!(matches!(
        user_units_from_measurement_on(&page(0.0), "1", M_1_50, f),
        Err(LengthParseError::UnusableScale { .. })
    ));
}
