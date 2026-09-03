//! # ce-dimension TOLERANCE (Pass 69.1)
//!
//! The second half of the operator's 2026-08-12 request — *"a default
//! dimensioning and **tolerance** style that can be set for the group, but
//! these should have a checkbox to override and set differently"* — built as
//! **one more property in the `Pass 69.0` cascade** rather than as a second
//! inheritance design. A tolerance therefore inherits exactly like a stroke
//! width: factory (none) → group default → per-ce-dimension override.
//!
//! ## What a tolerance IS here, and what it is not
//!
//! It is a **documentation property layered over an immutable measurement**,
//! stored on [`super::group::DimensionRecord`]'s style, never on
//! [`super::group::DimensionKind`] — which is documented in-source as *the
//! immutable geometry*. Setting a tolerance can never change what a ce
//! dimension measures; it changes what the drawing **asks the shop for**. The
//! ui-spec argued the same split independently
//! (`docs/ui_specs/tool-options-dock-and-ce-dimension-properties.md` §C.11.1)
//! and this follows it.
//!
//! ## Honest naming: pdfcer draws SolidWorks-STYLE tolerance notation
//!
//! Never "SolidWorks-conformant", and never "ASME Y14.5 conformant". Y14.5 is
//! paywalled and was not obtained; the notation below is drafting practice,
//! read off the reference tool's own API surface
//! (`D:\Dev\Rag-Specialized\SolidWorks_Dimensions\` §A) rather than off a
//! standard. This is the same epistemic discipline
//! [`super::group::DimStandard`] already applies to ISO 129-1.
//!
//! ## Which of the reference's thirteen types are here, and why not the rest
//!
//! The reference's `swTolType_e` has thirteen members (RAG §A.1). Seven are
//! implemented; the six omitted ones are omitted for **stated reasons**, not
//! by accident:
//!
//! | omitted | why |
//! |---|---|
//! | `swTolFIT`, `swTolFITWITHTOL`, `swTolFITTOLONLY` | ISO 286 limits-and-fits. The hole/shaft class strings are opaque in the reference's API and **the RAG explicitly flags the available class list as `UNVERIFIED`** (§A.3). Implementing a fit table from recall is exactly what project rule 1 forbids, and a wrong `H7/g6` deviation is a manufacturing defect, not a cosmetic one. |
//! | `swTolBLOCK`, `swTolGeneral` | Resolve against a document-level block-tolerance table / an ISO 2768 class table (§A.4). Both need a table pdfcer does not have; a "general tolerance" that resolves to nothing would print a promise the file cannot keep. |
//! | `swTolMETRIC` | Not a distinct type — it shares value `7` with `swTolFIT` (§A.1). Modelling it separately would invent a distinction the reference does not have. |
//!
//! Each of those is a future variant in this enum plus an arm in the caption
//! builder. Nothing here forecloses them.
//!
//! ## The units question, answered explicitly because it is easy to get wrong
//!
//! Tolerance values are in the **displayed unit** (millimetres, inches,
//! degrees — whatever the resolved [`super::units::NumberFormat`] shows), NOT
//! in PDF points. A tolerance is a manufacturing quantity the operator types
//! in the units he is thinking in; storing it in points would mean a group
//! rescale silently changed the tolerance, which is the opposite of what a
//! tolerance means. The nominal value is derived from geometry and scale; the
//! tolerance is a literal the operator supplied. They are different kinds of
//! number and only one of them moves when the scale changes.

use super::units::{DecimalMarker, NumberFormat};

/// The tolerance carried by one ce dimension (or a group's default).
///
/// `Copy` and free of heap data on purpose: it rides inside
/// [`super::style::StyleOverrides`], which is `Copy` so the cascade can be
/// resolved without allocation on every `/AP` regeneration. The fit-class
/// variants that would need `String`s are the ones deliberately absent (see
/// the module doc).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Tolerance {
    /// No tolerance shown — the factory default, and what every ce dimension
    /// authored before `Pass 69.1` carries.
    #[default]
    None,
    /// **Basic**: the value is theoretically exact and is drawn in a box
    /// (`swTolBASIC`). Carries no numbers of its own — the box IS the
    /// statement.
    Basic,
    /// **Symmetric**: `50.00 ±0.10` (`swTolSYMMETRIC`). One magnitude.
    ///
    /// The magnitude is stored as given; a negative one is meaningless and is
    /// refused at the API boundary rather than silently absolutised, because
    /// `±-0.1` is far more likely a typo than an intention.
    Symmetric {
        /// The half-width, in the displayed unit.
        magnitude: f64,
    },
    /// **Deviation** (the reference's *bilateral*, `swTolBILAT`):
    /// `50.00 +0.20/-0.10`. Two independent signed deviations.
    ///
    /// Both are stored **signed and as supplied**. A `plus` of `-0.05` is
    /// legal and meaningful (both limits below nominal — a common shaft
    /// callout), which is exactly why this cannot be normalised to magnitudes.
    Deviation {
        /// Upper deviation from nominal, in the displayed unit, signed.
        plus: f64,
        /// Lower deviation from nominal, in the displayed unit, signed.
        minus: f64,
    },
    /// **Limit** (`swTolLIMIT`): `50.20/49.90` — the two limits themselves,
    /// with the nominal suppressed.
    ///
    /// Stored as absolute values in the displayed unit, not as deviations:
    /// that is what the reference's `SetValues2` supplies for this type and
    /// what the drawing prints. Deriving them from the nominal at draw time
    /// would make the printed limits move when the scale changed.
    Limit {
        /// The upper limit, in the displayed unit.
        upper: f64,
        /// The lower limit, in the displayed unit.
        lower: f64,
    },
    /// **MIN** (`swTolMIN`): `50.00 MIN` — no upper limit stated.
    Min,
    /// **MAX** (`swTolMAX`): `50.00 MAX` — no lower limit stated.
    Max,
}

/// Why a tolerance was refused. Returned by [`Tolerance::validate`].
///
/// A named refusal rather than a silent correction, because every one of these
/// is a value that would print something the operator did not mean. The
/// reference models refusal as a first-class outcome too
/// (`swCreateAngRunDimError_e`, RAG §S6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToleranceError {
    /// A supplied number was NaN or infinite.
    NotFinite,
    /// A symmetric tolerance was given a negative magnitude. `±-0.1` is a
    /// typo, not a tolerance.
    NegativeMagnitude,
    /// A limit pair was given with the lower limit above the upper. Silently
    /// swapping them would print a drawing the operator never checked.
    LimitsInverted,
}

impl core::fmt::Display for ToleranceError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let s = match self {
            Self::NotFinite => "a tolerance value must be a finite number",
            Self::NegativeMagnitude => {
                "a symmetric tolerance's magnitude must not be negative (write ±0.1, not ±-0.1)"
            }
            Self::LimitsInverted => "the upper limit must not be below the lower limit",
        };
        f.write_str(s)
    }
}

impl core::error::Error for ToleranceError {}

impl Tolerance {
    /// Whether this tolerance prints anything at all beside the nominal.
    #[must_use]
    pub const fn is_none(self) -> bool {
        matches!(self, Self::None)
    }

    /// Whether the nominal is drawn inside a box (the *Basic* convention).
    ///
    /// A named question rather than a `matches!` at each call site, so the
    /// baker and any future exporter cannot disagree about which types are
    /// boxed.
    #[must_use]
    pub const fn is_boxed(self) -> bool {
        matches!(self, Self::Basic)
    }

    /// Whether the nominal value itself is SUPPRESSED, the way a limit
    /// dimension prints only its two limits (RAG §A.1: *"nominal is
    /// suppressed in display"*).
    #[must_use]
    pub const fn suppresses_nominal(self) -> bool {
        matches!(self, Self::Limit { .. })
    }

    /// Check the numbers make sense, before any of them reach a document.
    ///
    /// # Errors
    ///
    /// [`ToleranceError`] as documented on each variant. Every refusal is by
    /// name; nothing is clamped, swapped or absolutised (project rule 4 — a
    /// corrected value the operator never saw is exactly the sneaky case).
    pub const fn validate(self) -> Result<Self, ToleranceError> {
        match self {
            Self::None | Self::Basic | Self::Min | Self::Max => Ok(self),
            Self::Symmetric { magnitude } => {
                if !magnitude.is_finite() {
                    Err(ToleranceError::NotFinite)
                } else if magnitude < 0.0 {
                    Err(ToleranceError::NegativeMagnitude)
                } else {
                    Ok(self)
                }
            }
            Self::Deviation { plus, minus } => {
                if plus.is_finite() && minus.is_finite() {
                    Ok(self)
                } else {
                    Err(ToleranceError::NotFinite)
                }
            }
            Self::Limit { upper, lower } => {
                if !(upper.is_finite() && lower.is_finite()) {
                    Err(ToleranceError::NotFinite)
                } else if upper < lower {
                    Err(ToleranceError::LimitsInverted)
                } else {
                    Ok(self)
                }
            }
        }
    }

    /// The stable token this tolerance is written under in the `/PieceInfo`
    /// sidecar and accepted under on the CLI.
    #[must_use]
    pub const fn token(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Basic => "basic",
            Self::Symmetric { .. } => "symmetric",
            Self::Deviation { .. } => "deviation",
            Self::Limit { .. } => "limit",
            Self::Min => "min",
            Self::Max => "max",
        }
    }

    /// The text drawn AFTER the nominal (or, for a limit dimension, INSTEAD of
    /// it — see [`Self::suppresses_nominal`]).
    ///
    /// `places` is the tolerance's own decimal precision, which is a separate
    /// slot from the nominal's in the reference too (RAG §B.1: four precision
    /// slots, not one) — and whose "same as nominal" state is expressed here
    /// as the caller passing the nominal's own precision, rather than as a
    /// magic sentinel inside the digit count the way `swTolerancePrecisionFollowsNominal`
    /// (−3) does.
    ///
    /// # Why the caption is built here and not in the baker
    ///
    /// `Pass 68.0` shipped a defect whose whole cause was two places deriving
    /// a display value independently: the properties pane read `77.5°` while
    /// the `/AP` baked into the document read `77.47 pt`. One producer, always.
    ///
    /// # Examples
    ///
    /// ```
    /// use pdfcer_core::dimension::{NumberFormat, Tolerance, Unit};
    ///
    /// let fmt = NumberFormat::decimal(Unit::Millimeter, 2);
    /// assert_eq!(Tolerance::Symmetric { magnitude: 0.1 }.caption(fmt, 2), " ±0.10");
    /// assert_eq!(
    ///     Tolerance::Deviation { plus: 0.2, minus: -0.1 }.caption(fmt, 2),
    ///     " +0.20/-0.10"
    /// );
    /// assert_eq!(Tolerance::Max.caption(fmt, 2), " MAX");
    /// assert_eq!(Tolerance::Basic.caption(fmt, 2), "");
    /// ```
    #[must_use]
    pub fn caption(self, format: NumberFormat, places: u32) -> String {
        // The unit suffix is deliberately absent from every branch: a
        // tolerance is read in the same unit as the nominal it sits beside,
        // and "50.00 mm ±0.10 mm" is not how a drawing is written.
        let n = |v: f64| -> String {
            let text = format!("{v:.*}", places as usize);
            match format.decimal_marker {
                DecimalMarker::Point => text,
                DecimalMarker::Comma => text.replace('.', ","),
            }
        };
        // A signed deviation always shows its sign, including the `+`: the
        // sign is the information, and `0.20/-0.10` would read as a limit
        // pair.
        let signed = |v: f64| -> String {
            if v.is_sign_negative() {
                // `n` already carries the minus sign for a negative number;
                // adding one here would print `--0.10`.
                n(v)
            } else {
                format!("+{}", n(v))
            }
        };
        match self {
            // Basic prints no text — the BOX is the notation. Returning an
            // empty caption rather than a space keeps the label identical to
            // an untoleranced one, which is what makes the box the only
            // difference.
            Self::None | Self::Basic => String::new(),
            Self::Symmetric { magnitude } => format!(" \u{b1}{}", n(magnitude)),
            Self::Deviation { plus, minus } => {
                format!(" {}/{}", signed(plus), signed(minus))
            }
            // The limit form replaces the nominal, so it carries no leading
            // space: the caller substitutes it wholesale.
            Self::Limit { upper, lower } => format!("{}/{}", n(upper), n(lower)),
            Self::Min => " MIN".to_owned(),
            Self::Max => " MAX".to_owned(),
        }
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;
    use crate::dimension::Unit;

    fn mm() -> NumberFormat {
        NumberFormat::decimal(Unit::Millimeter, 2)
    }

    #[test]
    fn captions_read_the_way_a_drawing_is_written() {
        assert_eq!(Tolerance::None.caption(mm(), 2), "");
        assert_eq!(
            Tolerance::Symmetric { magnitude: 0.05 }.caption(mm(), 3),
            " \u{b1}0.050"
        );
        assert_eq!(
            Tolerance::Deviation {
                plus: 0.2,
                minus: -0.1
            }
            .caption(mm(), 2),
            " +0.20/-0.10"
        );
        assert_eq!(
            Tolerance::Limit {
                upper: 50.2,
                lower: 49.9
            }
            .caption(mm(), 1),
            "50.2/49.9"
        );
        assert_eq!(Tolerance::Min.caption(mm(), 2), " MIN");
    }

    /// Both deviations below nominal is a real callout, not an error — which
    /// is why `Deviation` stores signed values and never magnitudes.
    #[test]
    fn a_deviation_pair_may_be_wholly_negative() {
        let t = Tolerance::Deviation {
            plus: -0.01,
            minus: -0.05,
        };
        assert!(t.validate().is_ok());
        assert_eq!(t.caption(mm(), 2), " -0.01/-0.05");
    }

    #[test]
    fn the_decimal_marker_reaches_the_tolerance_too() {
        let mut fmt = mm();
        fmt.decimal_marker = DecimalMarker::Comma;
        assert_eq!(
            Tolerance::Symmetric { magnitude: 0.1 }.caption(fmt, 2),
            " \u{b1}0,10"
        );
    }

    /// Refusals are by name and nothing is silently corrected.
    #[test]
    fn nonsense_is_refused_rather_than_repaired() {
        assert_eq!(
            Tolerance::Symmetric { magnitude: -0.1 }.validate(),
            Err(ToleranceError::NegativeMagnitude)
        );
        assert_eq!(
            Tolerance::Limit {
                upper: 1.0,
                lower: 2.0
            }
            .validate(),
            Err(ToleranceError::LimitsInverted)
        );
        assert_eq!(
            Tolerance::Symmetric {
                magnitude: f64::NAN
            }
            .validate(),
            Err(ToleranceError::NotFinite)
        );
        assert_eq!(
            Tolerance::Deviation {
                plus: f64::INFINITY,
                minus: 0.0
            }
            .validate(),
            Err(ToleranceError::NotFinite)
        );
    }

    #[test]
    fn the_two_display_predicates_are_named_not_re_derived() {
        assert!(Tolerance::Basic.is_boxed());
        assert!(!Tolerance::Basic.suppresses_nominal());
        assert!(
            Tolerance::Limit {
                upper: 1.0,
                lower: 0.0
            }
            .suppresses_nominal()
        );
        assert!(Tolerance::None.is_none());
    }
}
