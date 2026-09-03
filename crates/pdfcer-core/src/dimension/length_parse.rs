//! Parse a real-world length the way it is WRITTEN ON THE DRAWING.
//!
//! # Why this exists
//!
//! The scale-by-known-dimension workflow (decision 011 §2.4's `RealLength`
//! path) asks the operator for the true length of a span they just picked.
//! The whole point is that they are reading that number off the drawing in
//! front of them — a dimension that says `55 5/8"`, or `4'-7 1/2"`.
//!
//! Until this module, the only way to supply it was a numeric spinner. So an
//! operator holding `55 5/8"` had to convert it to `55.625` in their head,
//! remember to switch the unit dropdown to inches, and type the decimal. Every
//! one of those steps is a chance to enter a number that is *plausible and
//! wrong* — and a wrong calibration is not a visible error, it silently
//! rescales every dimension in the group.
//!
//! So: accept what the drawing says.
//!
//! # What it accepts
//!
//! | Input | Value | Unit |
//! |---|---|---|
//! | `55 5/8"` | 55.625 | inches |
//! | `5/8"` | 0.625 | inches |
//! | `4' 7 1/2"` | 4.625 | feet-inches |
//! | `4'-7 1/2"` | 4.625 | feet-inches |
//! | `12'` | 12 | decimal feet |
//! | `1200mm` | 1200 | millimetres |
//! | `1.2 m` | 1.2 | metres |
//! | `55.625` | 55.625 | *the caller's default* |
//!
//! The **notation chooses the unit**, because someone typing `55 5/8"` has
//! already said what they mean and should not have to say it twice in a
//! dropdown. A bare number defers to the caller's current selection, which is
//! the only case where the dropdown is load-bearing.
//!
//! Feet-and-inches together yield [`Unit::FeetInches`]; feet alone yield
//! [`Unit::DecimalFeet`]; inches alone yield [`Unit::Inch`]. That mirrors what
//! was written rather than imposing a house style — and the caller is free to
//! change the unit afterwards, because parsing is not a commitment.
//!
//! The architectural hyphen in `4'-7 1/2"` is deliberately supported: it is
//! how the notation is conventionally *printed*, so it is exactly what an
//! operator copying a dimension off a drawing will type.
//!
//! # What it refuses, and why refusing beats guessing
//!
//! Anything it cannot read in full, and any non-positive result. A calibration
//! is a multiplier applied to every dimension in a group; a silently
//! misparsed input would produce a document full of confidently wrong numbers
//! with nothing to indicate it. Every refusal names the specific problem
//! (R27) so the operator can fix the input rather than guess at it.
//!
//! Values are returned in the returned unit's own magnitude — feet for
//! [`Unit::DecimalFeet`]/[`Unit::FeetInches`], inches for [`Unit::Inch`] —
//! matching [`Unit::baseline_per_point`], so a caller can hand the pair
//! straight to [`ScaleEntry::RealLength`](super::ScaleEntry::RealLength)
//! without a conversion step of its own.

use super::units::Unit;

/// Why a typed length could not be read.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[non_exhaustive]
pub enum LengthParseError {
    /// Nothing but whitespace.
    #[error("type the real length of the line you picked, for example 55 5/8\" or 4'-7 1/2\"")]
    Empty,
    /// The text has a shape this parser does not recognise.
    #[error(
        "couldn't read {input:?} as a length. Try a form like 55 5/8\", 4'-7 1/2\", 12', 1200mm, \
         or a plain number like 55.625"
    )]
    Unrecognised { input: String },
    /// A fraction with a zero denominator.
    #[error("{input:?} has a fraction divided by zero")]
    ZeroDenominator { input: String },
    /// The value is zero or negative.
    ///
    /// Separate from [`Self::Unrecognised`] because the input was *read*
    /// correctly — the operator does not need to re-check their typing, they
    /// need a different number.
    #[error("a length must be greater than zero, and {input:?} is not")]
    NotPositive { input: String },
    /// Inches part outside 0..12 in a feet-and-inches value.
    ///
    /// `4'-15"` is a real thing people type when they mean `5'-3"`, and
    /// accepting it silently would calibrate against a length they did not
    /// intend. Named rather than normalised.
    #[error("{input:?} has {inches} inches in the feet-and-inches part; it should be under 12")]
    InchesOutOfRange { input: String, inches: f64 },
}

/// A parsed real-world length: a magnitude and the unit it is expressed in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParsedLength {
    /// The value, in `unit`'s own magnitude (feet for feet-based units).
    pub value: f64,
    /// The unit the notation named, or the caller's default for a bare number.
    pub unit: Unit,
    /// Whether the unit came from the text rather than the caller's default.
    ///
    /// Lets a caller sync its unit dropdown to what was typed *and* know not
    /// to when the operator typed a bare number — changing the dropdown out
    /// from under someone who did not name a unit would be the tool
    /// second-guessing them.
    pub unit_from_text: bool,
}

/// Parse `input` as a real-world length, falling back to `default_unit` when
/// the text names no unit.
///
/// # Errors
///
/// See [`LengthParseError`]; every variant names a specific, fixable problem.
///
/// # Examples
///
/// ```
/// use pdfcer_core::dimension::{parse_length, Unit};
///
/// // What the drawing says, typed verbatim.
/// let p = parse_length("55 5/8\"", Unit::Meter).unwrap();
/// assert!((p.value - 55.625).abs() < 1e-12);
/// assert_eq!(p.unit, Unit::Inch);
/// assert!(p.unit_from_text);
///
/// // Architectural feet-and-inches, hyphen and all.
/// let a = parse_length("4'-7 1/2\"", Unit::Meter).unwrap();
/// assert!((a.value - 4.625).abs() < 1e-12);
/// assert_eq!(a.unit, Unit::FeetInches);
///
/// // A bare number defers to the caller's unit.
/// let b = parse_length("55.625", Unit::Inch).unwrap();
/// assert!((b.value - 55.625).abs() < 1e-12);
/// assert_eq!(b.unit, Unit::Inch);
/// assert!(!b.unit_from_text);
/// ```
pub fn parse_length(input: &str, default_unit: Unit) -> Result<ParsedLength, LengthParseError> {
    // Normalise the quote characters a real drawing (or a PDF's own text)
    // uses: typographic primes and curly quotes all mean foot and inch here.
    // A `"` pasted out of a PDF is very often U+201D, and refusing that would
    // make the feature fail on exactly the copy-paste path it exists to serve.
    let norm: String = input
        .chars()
        .map(|c| match c {
            '\u{2032}' | '\u{2018}' | '\u{2019}' => '\'', // ′ ‘ ’
            '\u{2033}' | '\u{201C}' | '\u{201D}' => '"',  // ″ “ ”
            c => c,
        })
        .collect();
    let s = norm.trim();
    if s.is_empty() {
        return Err(LengthParseError::Empty);
    }
    let orig = input.to_owned();

    // --- metric: a number followed by mm / cm / m --------------------------
    // Checked before the imperial forms because `m` would otherwise be eaten
    // by nothing and fall through to `Unrecognised`. Longest suffix first, so
    // `mm` is not read as `m` with a stray leading `m`.
    for (suffix, unit) in [
        ("mm", Unit::Millimeter),
        ("cm", Unit::Centimeter),
        ("m", Unit::Meter),
    ] {
        if let Some(head) = strip_suffix_ci(s, suffix) {
            let v = parse_decimal(head.trim(), &orig)?;
            return finish(v, unit, true, &orig);
        }
    }

    // --- imperial ----------------------------------------------------------
    // Split on the FIRST foot marker. Everything before it is feet; everything
    // after is inches. `4'-7 1/2"`, `4' 7 1/2"` and `4'7 1/2"` all land here,
    // and the architectural hyphen is stripped as a separator rather than
    // being read as a minus sign — which is the one ambiguity in the notation
    // and the reason this is split explicitly instead of by a general tokeniser.
    if let Some(idx) = s.find(['\'']) {
        let (feet_txt, rest) = s.split_at(idx);
        let rest = rest
            .get(1..)
            .unwrap_or("")
            .trim_start()
            .trim_start_matches('-')
            .trim();
        let feet = parse_mixed(feet_txt.trim(), &orig)?;
        if rest.is_empty() {
            // `12'` — feet only.
            return finish(feet, Unit::DecimalFeet, true, &orig);
        }
        let inch_txt = strip_inch_suffix(rest);
        let inches = parse_mixed(inch_txt.trim(), &orig)?;
        if !(0.0..12.0).contains(&inches) {
            return Err(LengthParseError::InchesOutOfRange {
                input: orig,
                inches,
            });
        }
        return finish(feet + inches / 12.0, Unit::FeetInches, true, &orig);
    }

    // Inches, with an explicit marker: `55 5/8"`, `55 5/8 in`.
    if let Some(head) = strip_inch_marker(s) {
        let v = parse_mixed(head.trim(), &orig)?;
        return finish(v, Unit::Inch, true, &orig);
    }

    // Feet spelled out without a prime: `12 ft`.
    for suffix in ["feet", "ft"] {
        if let Some(head) = strip_suffix_ci(s, suffix) {
            let v = parse_mixed(head.trim(), &orig)?;
            return finish(v, Unit::DecimalFeet, true, &orig);
        }
    }

    // --- bare number (possibly a mixed fraction) ---------------------------
    // The only case where the caller's unit selection decides.
    let v = parse_mixed(s, &orig)?;
    finish(v, default_unit, false, &orig)
}

/// Reject a non-positive result once, in one place.
fn finish(
    value: f64,
    unit: Unit,
    unit_from_text: bool,
    orig: &str,
) -> Result<ParsedLength, LengthParseError> {
    if !value.is_finite() || value <= 0.0 {
        return Err(LengthParseError::NotPositive {
            input: orig.to_owned(),
        });
    }
    Ok(ParsedLength {
        value,
        unit,
        unit_from_text,
    })
}

/// Case-insensitive suffix strip that also requires the suffix to be a real
/// token boundary — so `m` does not match the `m` inside `mm`.
fn strip_suffix_ci<'a>(s: &'a str, suffix: &str) -> Option<&'a str> {
    let lower = s.to_ascii_lowercase();
    let head_len = lower.strip_suffix(suffix)?.len();
    s.get(..head_len)
}

/// Strip a trailing inch marker (`"`, `in`, `inch`, `inches`), if present.
fn strip_inch_suffix(s: &str) -> &str {
    strip_inch_marker(s).unwrap_or(s)
}

/// As [`strip_inch_suffix`], but reports whether a marker was actually there.
fn strip_inch_marker(s: &str) -> Option<&str> {
    if let Some(head) = s.strip_suffix('"') {
        return Some(head);
    }
    for suffix in ["inches", "inch", "in"] {
        if let Some(head) = strip_suffix_ci(s, suffix) {
            return Some(head);
        }
    }
    None
}

/// Parse `55 5/8`, `5/8`, or `55.625` — a decimal with an optional fraction.
fn parse_mixed(s: &str, orig: &str) -> Result<f64, LengthParseError> {
    if s.is_empty() {
        return Err(LengthParseError::Unrecognised {
            input: orig.to_owned(),
        });
    }
    // A fraction is the last whitespace-separated token containing '/'.
    if let Some((whole_txt, frac_txt)) = split_fraction(s) {
        let frac = parse_fraction(frac_txt, orig)?;
        let whole = if whole_txt.trim().is_empty() {
            0.0
        } else {
            parse_decimal(whole_txt.trim(), orig)?
        };
        return Ok(whole + frac);
    }
    parse_decimal(s, orig)
}

/// Split `55 5/8` into (`55`, `5/8`), or `5/8` into (``, `5/8`).
fn split_fraction(s: &str) -> Option<(&str, &str)> {
    let slash = s.find('/')?;
    // Walk back to the start of the fraction's numerator.
    let head = s.get(..slash)?;
    let num_start = head.rfind(|c: char| c.is_whitespace()).map_or(0, |i| i + 1);
    Some((s.get(..num_start)?, s.get(num_start..)?))
}

fn parse_fraction(s: &str, orig: &str) -> Result<f64, LengthParseError> {
    let (n, d) = s
        .split_once('/')
        .ok_or_else(|| LengthParseError::Unrecognised {
            input: orig.to_owned(),
        })?;
    let n = parse_decimal(n.trim(), orig)?;
    let d = parse_decimal(d.trim(), orig)?;
    if d == 0.0 {
        return Err(LengthParseError::ZeroDenominator {
            input: orig.to_owned(),
        });
    }
    Ok(n / d)
}

fn parse_decimal(s: &str, orig: &str) -> Result<f64, LengthParseError> {
    // Thousands separators are common in typed lengths and mean nothing here.
    let cleaned: String = s.chars().filter(|c| *c != ',').collect();
    cleaned
        .trim()
        .parse::<f64>()
        .map_err(|_| LengthParseError::Unrecognised {
            input: orig.to_owned(),
        })
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::float_cmp
)]
mod tests {
    use super::*;

    fn ok(s: &str, default_unit: Unit) -> ParsedLength {
        parse_length(s, default_unit).unwrap_or_else(|e| panic!("{s:?} should parse: {e}"))
    }

    fn near(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    /// The exact input from the request that motivated this module.
    #[test]
    fn the_dimension_off_the_drawing_parses_verbatim() {
        let p = ok("55 5/8\"", Unit::Meter);
        assert!(near(p.value, 55.625), "{}", p.value);
        assert_eq!(p.unit, Unit::Inch, "the notation names inches");
        assert!(
            p.unit_from_text,
            "the caller's dropdown must not override a unit the operator wrote"
        );
    }

    /// Architectural feet-and-inches, in the three ways it gets written.
    ///
    /// The hyphen form is how the notation is conventionally PRINTED, so it
    /// is what someone copying a dimension off a drawing will type. Reading
    /// that hyphen as a minus sign would silently produce a negative inch
    /// part — a wrong calibration that looks like a successful one.
    #[test]
    fn feet_and_inches_accepts_hyphen_space_and_no_separator() {
        for s in ["4'-7 1/2\"", "4' 7 1/2\"", "4'7 1/2\""] {
            let p = ok(s, Unit::Meter);
            assert!(near(p.value, 4.625), "{s:?} gave {}", p.value);
            assert_eq!(p.unit, Unit::FeetInches, "{s:?}");
        }
    }

    #[test]
    fn feet_alone_and_inches_alone() {
        let f = ok("12'", Unit::Meter);
        assert!(near(f.value, 12.0));
        assert_eq!(f.unit, Unit::DecimalFeet);

        let f2 = ok("12 ft", Unit::Meter);
        assert!(near(f2.value, 12.0));
        assert_eq!(f2.unit, Unit::DecimalFeet);

        let i = ok("7 1/2 in", Unit::Meter);
        assert!(near(i.value, 7.5));
        assert_eq!(i.unit, Unit::Inch);
    }

    /// A fraction with no whole part, which is how small dimensions print.
    #[test]
    fn a_bare_fraction_parses() {
        let p = ok("5/8\"", Unit::Meter);
        assert!(near(p.value, 0.625));
        assert_eq!(p.unit, Unit::Inch);
    }

    #[test]
    fn metric_forms_parse_and_mm_is_not_read_as_m() {
        assert_eq!(ok("1200mm", Unit::Inch).unit, Unit::Millimeter);
        assert!(near(ok("1200mm", Unit::Inch).value, 1200.0));
        assert_eq!(ok("1.2 m", Unit::Inch).unit, Unit::Meter);
        assert_eq!(ok("30 cm", Unit::Inch).unit, Unit::Centimeter);
    }

    /// The one case where the caller's dropdown decides — and it must be
    /// reported as such, so a caller does not "helpfully" change the unit
    /// selection when the operator never named one.
    #[test]
    fn a_bare_number_defers_to_the_callers_unit() {
        let p = ok("55.625", Unit::Inch);
        assert!(near(p.value, 55.625));
        assert_eq!(p.unit, Unit::Inch);
        assert!(!p.unit_from_text);

        let q = ok("55.625", Unit::Meter);
        assert_eq!(q.unit, Unit::Meter, "same text, different caller default");
    }

    /// Typographic primes and curly quotes are what actually comes out of a
    /// PDF's own text, so the copy-paste path this feature exists for must
    /// not fail on them.
    #[test]
    fn typographic_quotes_mean_the_same_as_ascii() {
        assert!(near(ok("55 5/8\u{2033}", Unit::Meter).value, 55.625)); // ″
        assert!(near(ok("55 5/8\u{201D}", Unit::Meter).value, 55.625)); // ”
        let a = ok("4\u{2032}-7 1/2\u{2033}", Unit::Meter); // ′ and ″
        assert!(near(a.value, 4.625));
        assert_eq!(a.unit, Unit::FeetInches);
    }

    /// `4'-15"` is something people type when they mean `5'-3"`. Normalising
    /// it silently would calibrate against a length they did not intend, and
    /// a wrong calibration is invisible — it just rescales everything.
    #[test]
    fn an_out_of_range_inch_part_is_named_not_normalised() {
        let err = parse_length("4'-15\"", Unit::Meter).unwrap_err();
        match err {
            LengthParseError::InchesOutOfRange { inches, .. } => assert!(near(inches, 15.0)),
            other => panic!("expected InchesOutOfRange, got {other:?}"),
        }
    }

    #[test]
    fn non_positive_and_degenerate_inputs_are_refused_distinctly() {
        assert!(matches!(
            parse_length("0", Unit::Inch).unwrap_err(),
            LengthParseError::NotPositive { .. }
        ));
        assert!(matches!(
            parse_length("-5", Unit::Inch).unwrap_err(),
            LengthParseError::NotPositive { .. }
        ));
        assert!(matches!(
            parse_length("5/0\"", Unit::Inch).unwrap_err(),
            LengthParseError::ZeroDenominator { .. }
        ));
        assert!(matches!(
            parse_length("   ", Unit::Inch).unwrap_err(),
            LengthParseError::Empty
        ));
    }

    /// Garbage must be refused, not coerced. A calibration multiplies every
    /// dimension in the group, so a lenient parse is a document full of
    /// confidently wrong numbers.
    #[test]
    fn unreadable_input_is_refused_rather_than_guessed_at() {
        for s in ["abc", "5 5", "\"", "1/2/3", "55 5/8 furlongs"] {
            assert!(
                parse_length(s, Unit::Inch).is_err(),
                "{s:?} should not have parsed"
            );
        }
    }

    /// The refusal has to tell the operator what a good input looks like —
    /// "invalid" alone leaves them guessing at a notation (R27).
    #[test]
    fn the_unrecognised_message_shows_accepted_forms() {
        let msg = parse_length("abc", Unit::Inch).unwrap_err().to_string();
        assert!(msg.contains("55 5/8"), "{msg}");
        assert!(msg.contains("1200mm"), "{msg}");
    }

    /// Thousands separators appear in typed metric lengths and carry no
    /// meaning worth refusing over.
    #[test]
    fn thousands_separators_are_ignored() {
        assert!(near(ok("1,200mm", Unit::Inch).value, 1200.0));
    }
}
