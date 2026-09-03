//! # Units, number formatting, and scale back-calculation (decision 011 §2.4)
//!
//! The measurement-value arithmetic and display-string generation of the
//! dimensioning subsystem — the Rust twin of the ISO 32000-1 §12.9 Table 263
//! number-format algorithm pdfcer also encodes into the portable `/Measure`
//! dict (see [`super::measure_dict`]). Everything here is **pure** and has no
//! dependency outside `pdfcer-core`; it is exhaustively unit-tested because it
//! is the correctness heart of "the displayed value is right."
//!
//! ## The value model (decision 011 §2.3 "value data model")
//!
//! Geometry is immutable and stored in **PDF default user space (points,
//! 1/72")**: a linear dimension stores its `measured_pdf_length`; a
//! radius/diameter stores its `fitted_radius` (both in points). The
//! **displayed** value is *derived* — `value_in_top_unit = measured_points ×
//! scale` — where `scale` is the group's real-display-units-per-point factor
//! ([`ScaleState`]). Changing a group's scale re-derives every member's
//! displayed value with no change to stored geometry: that is what makes
//! "change the group scale → all member dimensions update" cheap.
//!
//! ## The scale IS the ISO §12.9 `/X` first `/C` (spec-grounded)
//!
//! Per `iso32000__s__12.9.md` (Question 3, confirmed): `scale = real_length /
//! drawn_pdf_length_points` is **exactly** the first `/X`-array `NumberFormat`
//! dict's `/C` — the value pdfcer stores AND the value the `/Measure` mirror
//! encodes. There is no separate ad-hoc scale field; the whole subsystem
//! turns on this one number, expressed in the group's *top* unit.
//!
//! ## The tri-state scale (ui-spec §4.3, binding ask #7)
//!
//! A group's scale is **never** collapsed to `Option<f64>` where `None` and
//! `Some(1.0)` are indistinguishable. [`ScaleState`] has three genuinely
//! different states — never-set, explicitly-1:1, calibrated — so a legitimate
//! full-size (1:1) drawing is never confused in the UI with "forgot to
//! calibrate." A never-set group discloses "raw page units"; a 1:1 group
//! shows the scaled value with no caveat.

use std::fmt::Write as _;

/// A display unit for a dimension group (decision 011 §2.4: the six beta
/// units). The metric/decimal units render as a decimal number; `Inch` can
/// render decimal or as a nearest-fraction; `FeetInches` is the architectural
/// `F'-I W/D"` form that **exceeds Acrobat** (a durable, still-open Acrobat
/// feature request — `measure__units_and_number_format.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    /// Millimetres.
    Millimeter,
    /// Centimetres.
    Centimeter,
    /// Metres.
    Meter,
    /// Inches (decimal or fractional, per the group's [`NumberFormat`]).
    Inch,
    /// Decimal feet (e.g. `12.50 ft`).
    DecimalFeet,
    /// Architectural feet-and-inches (e.g. `12'-6 1/2"`) — the top array unit
    /// is feet, the second is inches with a fractional denominator.
    FeetInches,
}

impl Unit {
    /// The short unit label used in a decimal display and as the `/U` string
    /// of the *top* array element in the portable `/Measure` dict
    /// (§12.9 Table 263 `/U`). `FeetInches` returns `ft` (its top array
    /// element); its inch part carries its own `in` label.
    #[must_use]
    pub const fn abbrev(self) -> &'static str {
        match self {
            Unit::Millimeter => "mm",
            Unit::Centimeter => "cm",
            Unit::Meter => "m",
            Unit::Inch => "in",
            Unit::DecimalFeet | Unit::FeetInches => "ft",
        }
    }

    /// The **true-scale (1:1) baseline** conversion — display units of this
    /// unit per one PDF point (1/72"), when one page point equals one
    /// physical point. These are the exact constants from
    /// `iso32000__s__12.9.md`'s derived unit table (SI + international inch:
    /// 1 in = 25.4 mm, 1 pt = 1/72 in). Used to give an explicit-1:1 group its
    /// effective scale ([`ScaleState::effective_scale`]) and to convert a
    /// ratio-path entry from its paper-unit basis.
    #[must_use]
    pub fn baseline_per_point(self) -> f64 {
        match self {
            Unit::Millimeter => 25.4 / 72.0,
            Unit::Centimeter => 2.54 / 72.0,
            Unit::Meter => 0.0254 / 72.0,
            Unit::Inch => 1.0 / 72.0,
            Unit::DecimalFeet | Unit::FeetInches => 1.0 / 864.0,
        }
    }

    /// A sensible default number format for this unit: 2 decimals for
    /// mm/cm/in/decimal-ft, 3 for m (a metre is coarse per-unit), and nearest
    /// 1/8" for feet-inches (architectural convention).
    #[must_use]
    pub fn default_format(self) -> NumberFormat {
        match self {
            Unit::Meter => NumberFormat::decimal(self, 3),
            Unit::FeetInches => NumberFormat::feet_inches(8, false),
            _ => NumberFormat::decimal(self, 2),
        }
    }

    /// All six units, in a stable order — the GUI unit dropdown and the CLI
    /// unit parser iterate this.
    #[must_use]
    pub const fn all() -> [Unit; 6] {
        [
            Unit::Millimeter,
            Unit::Centimeter,
            Unit::Meter,
            Unit::Inch,
            Unit::DecimalFeet,
            Unit::FeetInches,
        ]
    }

    /// Parse a unit from a lowercase token (CLI/`ScripTree` friendly):
    /// `mm|cm|m|in|ft|ft-in`. `None` on an unknown token.
    #[must_use]
    pub fn parse(s: &str) -> Option<Unit> {
        match s {
            "mm" => Some(Unit::Millimeter),
            "cm" => Some(Unit::Centimeter),
            "m" => Some(Unit::Meter),
            "in" | "inch" => Some(Unit::Inch),
            "ft" | "feet" | "decimal-ft" => Some(Unit::DecimalFeet),
            "ft-in" | "feet-inches" | "ftin" => Some(Unit::FeetInches),
            _ => None,
        }
    }

    /// The stable token [`Unit::parse`] accepts back — for CLI output and the
    /// sidecar serialization.
    #[must_use]
    pub const fn token(self) -> &'static str {
        match self {
            Unit::Millimeter => "mm",
            Unit::Centimeter => "cm",
            Unit::Meter => "m",
            Unit::Inch => "in",
            Unit::DecimalFeet => "ft",
            Unit::FeetInches => "ft-in",
        }
    }
}

/// How the fractional part of a value is displayed (§12.9 Table 263 `/F`,
/// `/D`, `/FD`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FractionMode {
    /// Decimal to `places` decimal places (`/F /D`, `/D = 10^places`).
    Decimal {
        /// Number of decimal places shown (fixed — pdfcer shows the full
        /// precision so `3.10 m` reads consistently, matching the ui-spec's
        /// own `12.4 pt → 3.10 m` example).
        places: u32,
    },
    /// Nearest `1/denominator` fraction (`/F /F`, `/D = denominator`). When
    /// `reduce` is false the denominator is kept (spec `/FD true`), the
    /// architectural convention (`6/8"` not `3/4"`); when true it is reduced.
    Fraction {
        /// The fraction denominator (8, 16, 32, …).
        denominator: u32,
        /// Whether to reduce the fraction to lowest terms (`/FD` inverse).
        reduce: bool,
    },
}

/// A number-format spec for one unit — the display half of a group's model
/// (decision 011 §2.4). Pairs the [`Unit`] with its [`FractionMode`]; for
/// [`Unit::FeetInches`] the fraction mode governs the *inch* part's rounding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NumberFormat {
    /// The unit this format renders in.
    pub unit: Unit,
    /// How the fractional part is shown.
    pub fraction: FractionMode,
    /// The character separating the whole and fractional parts (Pass 27.2).
    ///
    /// # Why this lives on the format and not on the drafting standard
    ///
    /// ISO 129-1:2018 cl. 4.1.1 **mandates** a comma (*"shall use a comma as
    /// the decimal marker"*), so the numeric string is not standard-independent
    /// after all. But putting the marker on [`super::DimStandard`] would make
    /// every value-formatting path depend on a drawing convention.
    ///
    /// `NumberFormat` is exactly what `measure_dict` projects into a §12.9
    /// NumberFormat dict, and `/RD` is exactly that dict's decimal-marker key —
    /// so the marker's home in pdfcer mirrors its home in the spec, and the two
    /// agree by construction rather than by remembering to keep them in step.
    ///
    /// Setting a group's standard to ISO sets this as a **disclosed** side
    /// effect the operator may then override; it is not welded to the standard.
    pub decimal_marker: DecimalMarker,
}

/// The character between the whole and fractional parts of a measurement
/// (§12.9 Table 263 `/RD`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DecimalMarker {
    /// `1.5` — ANSI/ASME practice and pdfcer's default.
    #[default]
    Point,
    /// `1,5` — mandated by ISO 129-1:2018 cl. 4.1.1, and widely violated in
    /// practice, which is why it is overridable rather than implied.
    Comma,
}

impl DecimalMarker {
    /// The marker as it is written.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Point => ".",
            Self::Comma => ",",
        }
    }
}

impl NumberFormat {
    /// A decimal format: `places` fixed decimal places in `unit`.
    #[must_use]
    pub const fn decimal(unit: Unit, places: u32) -> Self {
        Self {
            unit,
            fraction: FractionMode::Decimal { places },
            decimal_marker: DecimalMarker::Point,
        }
    }

    /// An inch format shown as a nearest-`1/denominator` fraction.
    #[must_use]
    pub const fn inch_fraction(denominator: u32) -> Self {
        Self {
            unit: Unit::Inch,
            fraction: FractionMode::Fraction {
                denominator,
                reduce: false,
            },
            decimal_marker: DecimalMarker::Point,
        }
    }

    /// A feet-inches format: whole feet + inches rounded to nearest
    /// `1/denominator` (default `reduce = false`, the architectural
    /// convention that keeps the denominator).
    #[must_use]
    pub const fn feet_inches(denominator: u32, reduce: bool) -> Self {
        Self {
            unit: Unit::FeetInches,
            fraction: FractionMode::Fraction {
                denominator,
                reduce,
            },
            decimal_marker: DecimalMarker::Point,
        }
    }

    /// Format `value_in_top_unit` (already `measured_points × scale`, in this
    /// format's *top* unit) as a display string with its unit label.
    ///
    /// This is the exact string pdfcer's live readout and each baked `/AP`
    /// label show, and it is the value an ISO §12.9-honouring reader computes
    /// for the same file from the mirrored `/Measure` dict (the two agree by
    /// construction — same algorithm, [`super::measure_dict`]).
    ///
    /// # Examples
    ///
    /// ```
    /// use pdfcer_core::dimension::{NumberFormat, Unit};
    ///
    /// // 3.1 metres at 3 decimals.
    /// assert_eq!(NumberFormat::decimal(Unit::Meter, 3).format(3.1), "3.100 m");
    /// // 12.5 feet as architectural feet-inches (nearest 1/8").
    /// assert_eq!(NumberFormat::feet_inches(8, false).format(12.5), "12'-6\"");
    /// // 6.5 inches as a nearest-1/8 fraction (kept, not reduced).
    /// assert_eq!(NumberFormat::inch_fraction(8).format(6.5), "6 4/8 in");
    /// ```
    #[must_use]
    pub fn format(&self, value_in_top_unit: f64) -> String {
        if !value_in_top_unit.is_finite() {
            return "—".to_owned();
        }
        match (self.unit, self.fraction) {
            (
                Unit::FeetInches,
                FractionMode::Fraction {
                    denominator,
                    reduce,
                },
            ) => format_feet_inches(value_in_top_unit, denominator.max(1), reduce),
            // A feet-inches unit configured (defensively) with a decimal
            // fraction falls back to decimal feet.
            (Unit::FeetInches, FractionMode::Decimal { places }) => {
                format!("{} ft", trim_or_fixed(value_in_top_unit, places))
            }
            (unit, FractionMode::Decimal { places }) => {
                format!(
                    "{} {}",
                    trim_or_fixed(value_in_top_unit, places),
                    unit.abbrev()
                )
            }
            (
                unit,
                FractionMode::Fraction {
                    denominator,
                    reduce,
                },
            ) => {
                format!(
                    "{} {}",
                    format_fraction(value_in_top_unit, denominator.max(1), reduce),
                    unit.abbrev()
                )
            }
        }
    }
}

/// Format a value to a fixed number of decimal places (no trimming — pdfcer
/// shows the requested precision verbatim so `3.10` stays `3.10`).
fn trim_or_fixed(value: f64, places: u32) -> String {
    format!("{value:.*}", places as usize)
}

/// Format `value` as `whole numer/den` at the nearest `1/den`, keeping or
/// reducing the fraction. Carries a rounded-up numerator into the whole part.
fn format_fraction(value: f64, den: u32, reduce: bool) -> String {
    let neg = value < 0.0;
    let a = value.abs();
    let mut whole = a.trunc() as i64;
    let frac = a - a.trunc();
    let mut numer = (frac * f64::from(den)).round() as i64;
    let mut denom = i64::from(den);
    if numer >= denom {
        whole += 1;
        numer = 0;
    }
    if reduce && numer > 0 {
        let g = gcd(numer, denom);
        numer /= g;
        denom /= g;
    }
    let mut out = String::new();
    if neg {
        out.push('-');
    }
    if numer == 0 {
        let _ = write!(out, "{whole}");
    } else if whole == 0 {
        let _ = write!(out, "{numer}/{denom}");
    } else {
        let _ = write!(out, "{whole} {numer}/{denom}");
    }
    out
}

/// Format `value_ft` (a value in feet) as architectural `F'-I"` /
/// `F'-I W/D"`, with the inch part rounded to the nearest `1/den`.
/// (§12.9 Table 263 two-element ft-in array — `iso32000__s__12.9.md`.)
fn format_feet_inches(value_ft: f64, den: u32, reduce: bool) -> String {
    let neg = value_ft < 0.0;
    let a = value_ft.abs();
    let mut feet = a.trunc() as i64;
    let rem_in = (a - a.trunc()) * 12.0;
    let mut whole_in = rem_in.trunc() as i64;
    let frac_in = rem_in - rem_in.trunc();
    let mut numer = (frac_in * f64::from(den)).round() as i64;
    let mut denom = i64::from(den);
    if numer >= denom {
        whole_in += 1;
        numer = 0;
    }
    if whole_in >= 12 {
        feet += 1;
        whole_in -= 12;
    }
    if reduce && numer > 0 {
        let g = gcd(numer, denom);
        numer /= g;
        denom /= g;
    }
    let mut out = String::new();
    if neg {
        out.push('-');
    }
    let _ = write!(out, "{feet}'-{whole_in}");
    if numer > 0 {
        let _ = write!(out, " {numer}/{denom}");
    }
    out.push('"');
    out
}

/// Greatest common divisor (Euclid), for reducing fractions.
fn gcd(mut a: i64, mut b: i64) -> i64 {
    a = a.abs();
    b = b.abs();
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    a.max(1)
}

/// The tri-state scale of a dimension group (ui-spec §4.3, binding ask #7).
///
/// - [`Self::NeverSet`] — a fresh/default group; measurements disclose "raw
///   page units," never silently presented as a real-world number.
/// - [`Self::OneToOne`] — the operator *deliberately* set a full-size (1:1)
///   scale; measurements are the scaled value with no caveat. Its effective
///   per-point factor is the unit's [`Unit::baseline_per_point`].
/// - [`Self::Calibrated`] — any other scale, from the scale-dimension
///   workflow or direct entry.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ScaleState {
    /// No scale has ever been set — display raw page units, disclosed.
    NeverSet,
    /// Explicitly set to a literal 1:1 (full-size) scale.
    OneToOne,
    /// Calibrated to a real-display-units-per-point factor.
    Calibrated {
        /// Real display units (in the group's top unit) per PDF point.
        scale: f64,
    },
}

impl ScaleState {
    /// The effective real-units-per-point factor for a group whose top unit is
    /// `unit`, or `None` when the scale has never been set (⇒ show raw
    /// points). [`Self::OneToOne`] resolves to the unit's true-scale baseline
    /// so a 1:1 metre drawing correctly shows `72 pt → 0.0254 m`.
    #[must_use]
    pub fn effective_scale(self, unit: Unit) -> Option<f64> {
        match self {
            ScaleState::NeverSet => None,
            ScaleState::OneToOne => Some(unit.baseline_per_point()),
            ScaleState::Calibrated { scale } => Some(scale),
        }
    }

    /// Whether this is the never-set state (drives the "raw page units"
    /// disclosure — ui-spec §4.3 / §6).
    #[must_use]
    pub fn is_never_set(self) -> bool {
        matches!(self, ScaleState::NeverSet)
    }
}

/// A formatted measurement result: the display string plus whether it is a
/// **raw** (never-scaled) reading, so the caller renders the "no scale set —
/// showing raw page units" disclosure verbatim (ui-spec §6, disclosures
/// rendered by core, never invented at the GUI layer).
#[derive(Debug, Clone, PartialEq)]
pub struct MeasurementDisplay {
    /// The display string, e.g. `3.10 m`, `12'-6"`, or `12.4 pt` (raw).
    pub text: String,
    /// True when no scale was set and the value is raw page units.
    pub raw_page_units: bool,
}

/// The verbatim disclosure a never-set group shows (ui-spec §6). Kept in core
/// so the GUI renders it, never paraphrases it.
pub const NO_SCALE_DISCLOSURE: &str = "no scale set — showing raw page units";

/// Format a stored geometry length (`measured_points`, in PDF points) for
/// display under a group's `scale_state` + `format`.
///
/// The one place the value model (decision 011 §2.3) is realised: raw points
/// when never-set (disclosed), otherwise `measured_points × effective_scale`
/// rendered by `format`. This exact function is called both for the live
/// readout and for regenerating a member's baked `/AP` label after a scale
/// change (the Pass 7.1 regenerate pattern).
///
/// # Examples
///
/// ```
/// use pdfcer_core::dimension::{format_measurement, NumberFormat, ScaleState, Unit};
///
/// // A 144-point line at scale 1 ft = 4 pt (0.25 ft/pt) → 36 ft.
/// let fmt = NumberFormat::decimal(Unit::DecimalFeet, 2);
/// let d = format_measurement(144.0, ScaleState::Calibrated { scale: 0.25 }, fmt);
/// assert_eq!(d.text, "36.00 ft");
/// assert!(!d.raw_page_units);
///
/// // Never-set → raw page units, disclosed.
/// let raw = format_measurement(144.0, ScaleState::NeverSet, fmt);
/// assert_eq!(raw.text, "144.00 pt");
/// assert!(raw.raw_page_units);
/// ```
#[must_use]
pub fn format_measurement(
    measured_points: f64,
    scale_state: ScaleState,
    format: NumberFormat,
) -> MeasurementDisplay {
    match scale_state.effective_scale(format.unit) {
        None => MeasurementDisplay {
            // The raw-points branch takes the marker too. It is a disclosure
            // state rather than a dimensioned value, but it is still a number
            // shown under a drafting standard — and a document that writes
            // "2,00 m" everywhere except where it falls back to points would
            // look like a bug, not like a distinction.
            text: apply_decimal_marker(format!("{measured_points:.2} pt"), format.decimal_marker),
            raw_page_units: true,
        },
        Some(scale) => MeasurementDisplay {
            // The decimal marker is applied HERE, once, on the finished
            // string, rather than threaded through every numeric branch of
            // `NumberFormat::format` (decimal, fraction, feet-inches). Those
            // branches all emit a point today; substituting at the end is one
            // place to be right instead of three, and a fraction like `5/8`
            // has no decimal point to disturb.
            text: apply_decimal_marker(
                format.format(measured_points * scale),
                format.decimal_marker,
            ),
            raw_page_units: false,
        },
    }
}

/// Replace the decimal points in a formatted measurement with `marker`.
///
/// Only characters BETWEEN two ASCII digits are touched. That is what keeps a
/// unit abbreviation containing a point (none today, but the unit table is not
/// frozen) and a trailing sentence period out of it — the substitution has to
/// be about the number, not about the string.
/// Format an ANGLE, in degrees, for a ce-dimension label.
///
/// # Why this is separate from [`format_measurement`]
///
/// [`format_measurement`] multiplies by the group's scale, because that is
/// what turns a page length into a real-world length. An angle is invariant
/// under uniform scaling — 30 degrees on a drawing at 1:50 is still 30
/// degrees — so routing an angle through that function would produce a
/// plausible, WRONG number carrying no indication that anything was odd.
///
/// The group's [`NumberFormat`] is still consulted for ONE thing: the decimal
/// marker. ISO 129-1:2018 cl. 4.1.1 mandates a comma decimal marker and says
/// nothing exempting angles, so an ISO group's angle reads `30,5` where an
/// ANSI group's reads `30.5`. The unit and fraction mode are deliberately NOT
/// consulted — an angle is not in millimetres and is not written as a
/// carpenter's fraction.
///
/// Precision is fixed at one decimal place, and that is a KNOWN GAP rather
/// than a decision: SolidWorks offers angular precision and alternative
/// angular units (degrees, deg-min, deg-min-sec), which belong with the wider
/// tolerance-and-precision work rather than being guessed at here.
#[must_use]
pub fn format_angle_degrees(degrees: f64, format: NumberFormat) -> String {
    let text = format!("{degrees:.1}");
    format!(
        "{}\u{b0}",
        apply_decimal_marker(text, format.decimal_marker)
    )
}

fn apply_decimal_marker(text: String, marker: DecimalMarker) -> String {
    if matches!(marker, DecimalMarker::Point) {
        return text;
    }
    let bytes: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    for (i, &c) in bytes.iter().enumerate() {
        let between_digits = c == '.'
            && i > 0
            && i.checked_sub(1)
                .and_then(|p| bytes.get(p))
                .is_some_and(char::is_ascii_digit)
            && bytes.get(i + 1).is_some_and(char::is_ascii_digit);
        if between_digits {
            out.push_str(marker.as_str());
        } else {
            out.push(c);
        }
    }
    out
}

/// One of the two co-equal scale-entry paths (ui-spec §4.2). Exactly one shape
/// is populated per entry (an enum, not two optional pairs — the recommended
/// unambiguous representation).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ScaleEntry {
    /// **Real-length path (recommended):** the operator drew a reference line
    /// of `drawn_pdf_length` points and typed its real-world length + unit.
    /// Back-calc: `scale = real_length / drawn_pdf_length` (decision 011 §2.4,
    /// spec-confirmed as `/X` first `/C`).
    RealLength {
        /// The drawn reference line's length in PDF points.
        drawn_pdf_length: f64,
        /// The operator-typed real-world length, in `unit`.
        real_length: f64,
        /// The unit the real length was typed in (becomes the group top unit).
        unit: Unit,
    },
    /// **Direct-ratio path:** the operator typed `paper : real` (e.g.
    /// `1 : 100`). Needs a paper-unit basis; PDF paper units are 1/72", and the
    /// default basis is inch, disclosed (ui-spec §4.2). Back-calc:
    /// `scale = (real / paper) × basis.baseline_per_point()`.
    Ratio {
        /// The paper side of the ratio (`1` in `1:100`).
        paper: f64,
        /// The real side of the ratio (`100` in `1:100`).
        real: f64,
        /// The paper-unit basis (default [`Unit::Inch`]).
        basis: Unit,
    },
}

/// The live preview of a scale entry (ui-spec §4.2: "→ scale = 25.0 ft /
/// 42.3 pt", shown BEFORE Accept). Pure arithmetic; no mutation.
#[derive(Debug, Clone, PartialEq)]
pub struct ScalePreview {
    /// The back-calculated scale — real display units (in `unit`) per PDF
    /// point. This is the value stored as [`ScaleState::Calibrated`] and
    /// mirrored as the `/X` first `/C` in the portable `/Measure` dict.
    pub scale: f64,
    /// The unit the scale (and the group) is expressed in.
    pub unit: Unit,
    /// A human-readable `/R`-style ratio label (§12.9 Table 262 `/R`,
    /// DISPLAY-ONLY), e.g. `1:100` or `25 ft = 42.3 pt`.
    pub ratio_label: String,
}

/// Preview the scale a [`ScaleEntry`] would set, without mutating anything
/// (ui-spec §4.5 binding ask #6: the pure preview sibling of the commit).
///
/// `None` for a degenerate entry (a non-positive or non-finite drawn length /
/// paper side, or a non-finite real value) — the GUI shows nothing to Accept.
///
/// # Examples
///
/// ```
/// use pdfcer_core::dimension::{preview_group_scale, ScaleEntry, Unit};
///
/// // Real-length: a 42.3-pt line is 25 ft → 0.591 ft/pt.
/// let p = preview_group_scale(ScaleEntry::RealLength {
///     drawn_pdf_length: 42.3, real_length: 25.0, unit: Unit::DecimalFeet,
/// }).unwrap();
/// assert!((p.scale - 25.0 / 42.3).abs() < 1e-12);
/// assert_eq!(p.unit, Unit::DecimalFeet);
///
/// // Ratio 1:100 on the default inch basis → 100/72 in per point.
/// let r = preview_group_scale(ScaleEntry::Ratio {
///     paper: 1.0, real: 100.0, basis: Unit::Inch,
/// }).unwrap();
/// assert!((r.scale - 100.0 / 72.0).abs() < 1e-12);
/// assert_eq!(r.ratio_label, "1:100");
/// ```
#[must_use]
pub fn preview_group_scale(entry: ScaleEntry) -> Option<ScalePreview> {
    match entry {
        ScaleEntry::RealLength {
            drawn_pdf_length,
            real_length,
            unit,
        } => {
            if !(drawn_pdf_length.is_finite() && drawn_pdf_length > 0.0 && real_length.is_finite())
            {
                return None;
            }
            let scale = real_length / drawn_pdf_length;
            if !scale.is_finite() {
                return None;
            }
            Some(ScalePreview {
                scale,
                unit,
                ratio_label: format!(
                    "{} {} = {} pt",
                    trim_or_fixed(real_length, 2),
                    unit.abbrev(),
                    trim_or_fixed(drawn_pdf_length, 2)
                ),
            })
        }
        ScaleEntry::Ratio { paper, real, basis } => {
            if !(paper.is_finite() && paper > 0.0 && real.is_finite() && real >= 0.0) {
                return None;
            }
            let scale = (real / paper) * basis.baseline_per_point();
            if !scale.is_finite() {
                return None;
            }
            Some(ScalePreview {
                scale,
                unit: basis,
                ratio_label: format!("{}:{}", trim_ratio(paper), trim_ratio(real)),
            })
        }
    }
}

/// Format a ratio side without trailing decimals when it is a whole number
/// (`1:100`, not `1.00:100.00`).
fn trim_ratio(v: f64) -> String {
    if (v.fract()).abs() < 1e-9 {
        format!("{}", v.round() as i64)
    } else {
        format!("{v:.2}")
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_cmp
)]
mod tests {
    use super::*;

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
        for u in Unit::all() {
            assert_eq!(Unit::parse(u.token()), Some(u), "token {}", u.token());
        }
        assert!(Unit::parse("furlong").is_none());
    }

    #[test]
    fn non_finite_value_formats_as_dash_not_panic() {
        assert_eq!(NumberFormat::decimal(Unit::Meter, 2).format(f64::NAN), "—");
        assert_eq!(
            NumberFormat::feet_inches(8, false).format(f64::INFINITY),
            "—"
        );
    }
}
