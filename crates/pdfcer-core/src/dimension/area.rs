//! Area formatting for an area ce dimension (a closed
//! [`crate::dimension::DimensionKind::Perimeter`] with `area: true`).
//!
//! A length scales by the group scale `s` (display units per point); an area
//! scales by `s²`. That is the same relationship ISO 32000-1 §12.9 Table 262
//! encodes for `/A`: its conversion factor applies to "the units of the first
//! element of X, squared".

use super::units::{
    FractionMode, MeasurementDisplay, NumberFormat, ScaleState, apply_decimal_marker,
};

/// Decimal places used when a format carries no decimal precision of its own.
const FALLBACK_AREA_PLACES: u32 = 2;

/// The unit label for an area in `unit`, e.g. `m²`. Feet-inches has no
/// compound area notation, so it reads `ft²`.
#[must_use]
pub fn area_unit_label(format: NumberFormat) -> String {
    format!("{}\u{b2}", format.unit.abbrev())
}

/// Decimal places for an area value: the format's own when it is decimal,
/// otherwise [`FALLBACK_AREA_PLACES`] — a fraction of a square unit
/// (`5/8 in²`) is not a notation drawings use.
#[must_use]
pub const fn area_places(format: NumberFormat) -> u32 {
    match format.fraction {
        FractionMode::Decimal { places } => places,
        FractionMode::Fraction { .. } => FALLBACK_AREA_PLACES,
    }
}

/// Format an area of `square_points` (square PDF points) for a ce-dimension
/// label, in the group's unit squared.
///
/// With no effective scale the value is shown in raw square points and
/// `raw_page_units` is `true`, exactly like [`super::format_measurement`].
/// The format's decimal marker applies in both branches.
///
/// ```
/// use pdfcer_core::dimension::{NumberFormat, ScaleState, Unit, format_area_measurement};
/// let fmt = NumberFormat::decimal(Unit::Meter, 2);
/// let shown = format_area_measurement(5184.0, ScaleState::Calibrated { scale: 0.05 }, fmt);
/// assert_eq!(shown.text, "12.96 m\u{b2}");
/// assert!(!shown.raw_page_units);
/// ```
#[must_use]
pub fn format_area_measurement(
    square_points: f64,
    scale_state: ScaleState,
    format: NumberFormat,
) -> MeasurementDisplay {
    let Some(scale) = scale_state.effective_scale(format.unit) else {
        return MeasurementDisplay {
            text: apply_decimal_marker(
                format!("{square_points:.2} pt\u{b2}"),
                format.decimal_marker,
            ),
            raw_page_units: true,
        };
    };
    let value = square_points * scale * scale;
    let text = if value.is_finite() {
        format!(
            "{value:.*} {}",
            area_places(format) as usize,
            area_unit_label(format)
        )
    } else {
        "\u{2014}".to_owned()
    };
    MeasurementDisplay {
        text: apply_decimal_marker(text, format.decimal_marker),
        raw_page_units: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dimension::{DecimalMarker, Unit};

    #[test]
    fn raw_area_is_square_points() {
        let fmt = NumberFormat::decimal(Unit::Meter, 2);
        let shown = format_area_measurement(5184.0, ScaleState::NeverSet, fmt);
        assert_eq!(shown.text, "5184.00 pt\u{b2}");
        assert!(shown.raw_page_units);
    }

    #[test]
    fn fraction_format_falls_back_to_two_places_and_marker_applies() {
        let mut fmt = NumberFormat::inch_fraction(8);
        fmt.decimal_marker = DecimalMarker::Comma;
        let shown = format_area_measurement(5184.0, ScaleState::Calibrated { scale: 0.5 }, fmt);
        assert_eq!(shown.text, "1296,00 in\u{b2}");
    }
}
