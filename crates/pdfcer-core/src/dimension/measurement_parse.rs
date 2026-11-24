//! Typed scaled lengths back to page user space: the inverse of
//! [`format_measurement_on`](crate::dimension::format_measurement_on).

use super::length_parse::{LengthParseError, parse_magnitude};
use super::units::{DecimalMarker, NumberFormat, ScaleState};
use crate::page_tree::Page;

/// The length in `page`'s user space whose
/// [`format_measurement_on`](crate::dimension::format_measurement_on) reading under
/// `scale_state` and `format` is `typed`.
///
/// `typed` is anything [`parse_length`](crate::dimension::parse_length) reads, plus
/// `km`, `yd`, `mi` and `pt`, with `format`'s decimal marker (`2,4` in a
/// comma group) and an optional leading `-`. Zero is accepted, so a
/// position box can use it as well as a size box.
///
/// - A bare number is in the group's top unit; a typed unit other than the
///   top unit (`mm` into a feet group) is converted through the same
///   calibration ([`ScaleState::in_unit`]).
/// - [`ScaleState::OneToOne`] reads the typed unit at true scale.
/// - [`ScaleState::NeverSet`]: a bare number is points, as the raw readout
///   shows them, and a unit-bearing length is a physical length on the
///   paper (`10 mm` is 28.35 pt).
/// - A `pt` suffix is always printed points, whatever the scale.
///
/// The result is divided by the page's `/UserUnit` (ISO 32000-1 §14.11.2,
/// Table 30), so the readout of the returned length is `typed` again, to
/// the format's precision.
///
/// # Errors
///
/// The [`LengthParseError`] for text that is not a length, and
/// [`LengthParseError::UnusableScale`] when the scale or `/UserUnit` is
/// zero, negative or not finite.
///
/// # Examples
///
/// ```
/// use pdfcer_core::dimension::{
///     format_measurement_on, user_units_from_measurement_on, NumberFormat, ScaleState, Unit,
/// };
/// use pdfcer_core::object::ObjId;
/// use pdfcer_core::page_tree::{Page, Rect};
///
/// let sheet = Rect::from_corners(0.0, 0.0, 612.0, 792.0);
/// let page = Page::with_boxes(ObjId::new(3, 0), sheet, sheet, 0);
/// // 1:50 in metres: one point is 50 × 0.0254 / 72 m.
/// let scale = ScaleState::Calibrated { scale: 50.0 * 0.0254 / 72.0 };
/// let fmt = NumberFormat::decimal(Unit::Meter, 2);
///
/// let pt = user_units_from_measurement_on(&page, "2.4", scale, fmt)?;
/// assert_eq!(format_measurement_on(&page, pt, scale, fmt).text, "2.40 m");
/// // 2400 mm is the same length.
/// let mm = user_units_from_measurement_on(&page, "2400 mm", scale, fmt)?;
/// assert!((mm - pt).abs() < 1e-9);
/// # Ok::<(), pdfcer_core::dimension::LengthParseError>(())
/// ```
pub fn user_units_from_measurement_on(
    page: &Page,
    typed: &str,
    scale_state: ScaleState,
    format: NumberFormat,
) -> Result<f64, LengthParseError> {
    let text = with_point_decimals(typed, format.decimal_marker);
    let trimmed = text.trim();
    let (negative, magnitude) = match trimmed.strip_prefix('-') {
        Some(rest) => (true, rest.trim_start()),
        None => (false, trimmed),
    };
    let points = printed_points(magnitude, typed, scale_state, format)?;
    let user = points / page.user_unit;
    if !user.is_finite() || page.user_unit <= 0.0 {
        return Err(unusable(typed));
    }
    Ok(if negative { -user } else { user })
}

/// The physical (printed) length in points that `magnitude` names.
fn printed_points(
    magnitude: &str,
    typed: &str,
    scale_state: ScaleState,
    format: NumberFormat,
) -> Result<f64, LengthParseError> {
    let unrecognised = || LengthParseError::Unrecognised {
        input: typed.to_owned(),
    };
    if let Some(head) = strip_points(magnitude) {
        let parsed = parse_magnitude(head, format.unit)?;
        if parsed.unit_from_text || parsed.value < 0.0 {
            return Err(unrecognised());
        }
        return Ok(parsed.value);
    }
    let parsed = parse_magnitude(magnitude, format.unit)?;
    if parsed.value < 0.0 {
        return Err(unrecognised());
    }
    let per_point = match scale_state.effective_scale(format.unit) {
        None if !parsed.unit_from_text => return Ok(parsed.value),
        None => parsed.unit.baseline_per_point(),
        Some(_) => scale_state
            .in_unit(format.unit, parsed.unit)
            .effective_scale(parsed.unit)
            .ok_or_else(|| unusable(typed))?,
    };
    if !per_point.is_finite() || per_point <= 0.0 {
        return Err(unusable(typed));
    }
    Ok(parsed.value / per_point)
}

fn unusable(typed: &str) -> LengthParseError {
    LengthParseError::UnusableScale {
        input: typed.to_owned(),
    }
}

/// `text` with a trailing `pt` removed, if it has one.
fn strip_points(text: &str) -> Option<&str> {
    let head_len = text.to_ascii_lowercase().strip_suffix("pt")?.len();
    text.get(..head_len).map(str::trim_end)
}

/// `text` with a comma marker between two digits turned into a point, so
/// the parser (which reads commas as thousands separators) sees `2.4`.
fn with_point_decimals(text: &str, marker: DecimalMarker) -> String {
    if marker != DecimalMarker::Comma {
        return text.to_owned();
    }
    let chars: Vec<char> = text.chars().collect();
    let digit_at = |i: Option<usize>| {
        i.and_then(|i| chars.get(i))
            .is_some_and(char::is_ascii_digit)
    };
    chars
        .iter()
        .enumerate()
        .map(|(i, &c)| {
            if c == ',' && digit_at(i.checked_sub(1)) && digit_at(Some(i + 1)) {
                '.'
            } else {
                c
            }
        })
        .collect()
}

#[cfg(test)]
#[path = "measurement_parse_tests.rs"]
mod tests;
