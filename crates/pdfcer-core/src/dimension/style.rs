//! # The ce-dimension STYLE CASCADE (Pass 69.0)
//!
//! A three-tier, **per-property** inheritance model for everything about a ce
//! dimension's appearance that is not its geometry:
//!
//! ```text
//! 1. FACTORY DEFAULT   [`StyleDefaults::FACTORY`] — the values pdfcer shipped
//!                      with, and the values every pre-Pass-69.0 ce dimension
//!                      was drawn with, so an old document reopens identical
//!        ↓ supplies the value for any property the group leaves unset
//! 2. GROUP             [`GroupStyle`] on [`super::group::Group`] — the
//!                      operator's default "for this group of ce dimensions"
//!        ↓ supplies the value for any property the ce dimension leaves unset
//! 3. ce DIMENSION      [`StyleOverrides`] on [`super::group::DimensionRecord`]
//!                      — the operator's "except THIS one"
//! ```
//!
//! ## Why per-property, and not a single "overrides the style" bit
//!
//! This is the shape the reference tool actually uses, and it was checked
//! rather than recalled. `D:\Dev\Rag-Specialized\SolidWorks_Dimensions\`
//! §F.3 measured the SOLIDWORKS API surface and found that essentially every
//! settable display property is a **(inherit-flag, value) pair** — a separate
//! `UseDoc` boolean on `SetArrowHeadStyle2`, `SetUnits2`, `SetDual2`,
//! `SetWitnessLineGap`, `SetBentLeaderLength`, and a dozen more. There is no
//! single per-dimension "detached from the document" flag anywhere in it.
//! §B.4 is the strongest single piece of that evidence: four of the nine
//! members of `swDetailingDimTrailingZero_e` are *inherit-pointers naming four
//! different sources*.
//!
//! The operator asked for exactly this, in these words (2026-08-12):
//!
//! > *"groups of dimensions should have a default dimensioning and tolerance
//! > style that can be set for the group, but these should have a checkbox to
//! > override and set differently."*
//!
//! An `Option<T>` per property IS that checkbox: `None` = the box is clear and
//! the value is inherited; `Some(v)` = the box is ticked and this ce dimension
//! (or this group) says `v`. Nothing else needs storing, and — the part that
//! matters for the operator's stated fear of *"changing one and being
//! surprised 40 others changed or didn't"* — the inherited/overridden state is
//! **recoverable from the data itself**, not a UI-only affordance. See
//! [`StyleProvenance`], which answers *which tier did each property come
//! from* for any surface that wants to disclose it.
//!
//! ## Deliberate divergences from the reference, recorded not silently taken
//!
//! The parity posture for this project is that SolidWorks is the **floor**,
//! not the ceiling (operator, standing preference). Three divergences:
//!
//! 1. **One representation of "inherit", not three.** The reference expresses
//!    the inherit-flag three different ways in one API — a `UseDoc` boolean
//!    parameter, a negative sentinel inside the value's own numeric range
//!    (`swPrecisionFollowsDocumentSetting` = −2), and a member of the value
//!    enum itself (`swDimArrowsFollowDoc` = 3). The RAG's own advice is
//!    *"pick one representation in pdfcer; do not replicate the
//!    inconsistency."* pdfcer uses `Option::None`, everywhere, for every
//!    property.
//! 2. **The exotic inherit-sources are collapsed.** The reference can inherit
//!    a tolerance's text size from *the parent dimension*, an extension line's
//!    style from *the leader*, a tolerance's precision from *the nominal*, and
//!    leading zeros from *the drafting standard* (§F.3). pdfcer's cascade has
//!    exactly three tiers and every property inherits along the same chain.
//!    This is a documented simplification, not an oversight: a property whose
//!    inherit-source differs from its neighbours' is a property nobody can
//!    predict without reading the manual.
//! 3. **The SCALE is not overridable per ce dimension.** It is on the group
//!    and stays there. A scale is not appearance — it is what turns page
//!    points into a real-world length, and a ce dimension that quietly used a
//!    different one from its group's would report a number that is wrong in a
//!    way nothing on the page discloses. `fuzzy-never-sneaky` (project rule 4)
//!    makes that a refusal rather than a feature.
//!
//! ## Tolerance is one of these properties, not a parallel system
//!
//! `Pass 69.1` added [`super::tolerance::Tolerance`] and its own precision
//! slot as the tenth and eleventh properties of this same cascade — which was
//! the point of building the mechanism first. A group can carry a default
//! tolerance and one ce dimension can override it, using the same `Option`,
//! the same provenance reporting and the same clear-restores-inheritance
//! semantics as a stroke width.

use crate::vector::Rgb;

use super::author::DimensionStyle;
use super::group::{DimStandard, Group};
use super::tolerance::Tolerance;
use super::units::{DecimalMarker, FractionMode, NumberFormat, Unit};

/// The terminator drawn at each end of a ce dimension line.
///
/// A subset of the reference's thirteen `swArrowStyle_e` members
/// (`SolidWorks_Dimensions` §C.1), chosen as the forms that are (a) in common
/// drafting use across ANSI and ISO practice and (b) drawable from the
/// primitives the `/AP` baker already emits. The omitted members are the
/// vendor- and standard-specific ones (`swRUS_ARROWHEAD` GOST,
/// `swISOWIDE_ARROWHEAD`, the half-filled `swCLOSETOP`/`swCLOSEBOT`, and
/// `swSMART_ARROWHEAD`'s lightning-bolt glyph); adding one later is a new
/// variant here plus a new arm in the baker, and nothing else.
///
/// # Why the default is `Filled`
///
/// It is what pdfcer has always drawn (`author::arrowhead` filled a triangle
/// unconditionally before this Pass), so a document authored before the style
/// cascade existed reopens looking **identical** — the acceptance criterion
/// this whole design is arranged around.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum ArrowForm {
    /// A solid filled triangle — ANSI/ASME mechanical practice, and pdfcer's
    /// factory default. `swCLOSED_ARROWHEAD`.
    #[default]
    Filled,
    /// An open (stroked, unfilled) V — `swOPEN_ARROWHEAD`. Common in
    /// architectural and civil work.
    Open,
    /// A 45-degree tick through the dimension line — `swSLASH_ARROWHEAD`.
    /// Standard architectural practice; ISO 129-1 permits it.
    Slash,
    /// A filled dot — `swDOT_ARROWHEAD`. Used where the arrow would not fit,
    /// and for the origin end of an ordinate run.
    Dot,
    /// No terminator at all — `swNO_ARROWHEAD`. The extension lines and the
    /// dimension line still mark the extent.
    None,
}

impl ArrowForm {
    /// The CLI/sidecar token for this form (lowercase, stable, never
    /// localised — it is written into documents).
    #[must_use]
    pub const fn token(self) -> &'static str {
        match self {
            Self::Filled => "filled",
            Self::Open => "open",
            Self::Slash => "slash",
            Self::Dot => "dot",
            Self::None => "none",
        }
    }

    /// Parse a [`Self::token`], case-insensitively. `None` for anything else.
    ///
    /// Total and lenient by design: this is also the sidecar's reader, and a
    /// document carrying an arrow form a future pdfcer added must degrade to
    /// "inherit" rather than losing the whole record (see
    /// [`super::sidecar`]'s forward-compatibility contract).
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "filled" => Some(Self::Filled),
            "open" => Some(Self::Open),
            "slash" => Some(Self::Slash),
            "dot" => Some(Self::Dot),
            "none" => Some(Self::None),
            _ => None,
        }
    }

    /// Every form, in token order — for a CLI help string or a GUI picker that
    /// must not drift from the enum.
    pub const ALL: [Self; 5] = [Self::Filled, Self::Open, Self::Slash, Self::Dot, Self::None];
}

/// The bottom of the cascade: the values pdfcer draws with when neither the ce
/// dimension nor its group says otherwise.
///
/// # Every number here is what the code already hard-coded
///
/// Before this Pass these were module constants in [`super::author`]
/// (`LABEL_SIZE`, `LINE_WIDTH`, `ARROW_LEN`) and an unconditional black in the
/// content stream. They are reproduced here **exactly**, which is what makes
/// criterion 4 hold: a ce dimension authored before the style cascade existed
/// carries no overrides, resolves to these, and re-bakes byte-identically.
/// Changing a value here would silently redraw every existing document on its
/// next regeneration — so treat this struct as a compatibility surface, not a
/// preferences file.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StyleDefaults {
    /// Label point size.
    pub text_height: f64,
    /// Leader/extension/dimension-line stroke width, in points.
    pub line_width: f64,
    /// Arrowhead length, in points.
    pub arrow_length: f64,
    /// Terminator form.
    pub arrow_form: ArrowForm,
    /// Line + text + terminator colour.
    pub color: Rgb,
    /// The tolerance drawn beside the nominal (`Pass 69.1`).
    pub tolerance: Tolerance,
    /// The tolerance's OWN decimal precision, or `None` for "same as the
    /// nominal's" (`Pass 69.1`).
    ///
    /// A separate slot because the reference has one too — four precision
    /// slots, not one (`SolidWorks_Dimensions` §B.1). `None` is pdfcer's
    /// spelling of `swTolerancePrecisionFollowsNominal`, which the reference
    /// expresses as a −3 sentinel hidden inside the digit count; a distinct
    /// absent-value is the same information without the trap.
    pub tolerance_places: Option<u32>,
}

impl StyleDefaults {
    /// The factory defaults. See the type's doc comment before changing any
    /// number here — they are a backward-compatibility contract.
    pub const FACTORY: Self = Self {
        text_height: 10.0,
        line_width: 0.75,
        arrow_length: 7.0,
        arrow_form: ArrowForm::Filled,
        color: Rgb::BLACK,
        // No tolerance is the factory default, and it is what every ce
        // dimension authored before `Pass 69.1` carries.
        tolerance: Tolerance::None,
        tolerance_places: None,
    };
}

/// Tier 2 — a **group's** appearance defaults. `None` on a field means "use
/// the factory default" ([`StyleDefaults::FACTORY`]).
///
/// # Why the group tier carries only the appearance properties
///
/// The group already holds concrete, always-present fields for unit, number
/// format, decimal marker, drafting standard and scale
/// ([`super::group::Group`]) — those have been group properties since Pass
/// 12.M2 and every consumer reads them directly. Duplicating them here as
/// `Option`s would create two places to ask the same question and one of them
/// would eventually be stale. So the group tier for those five properties IS
/// the group's own fields; this struct adds the five that had no home at all.
///
/// The ce-dimension tier ([`StyleOverrides`]) does carry all of them, because
/// there the `None` genuinely means something different from any concrete
/// value: *inherit*.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct GroupStyle {
    /// Label point size; `None` ⇒ factory.
    pub text_height: Option<f64>,
    /// Stroke width in points; `None` ⇒ factory.
    pub line_width: Option<f64>,
    /// Arrowhead length in points; `None` ⇒ factory.
    pub arrow_length: Option<f64>,
    /// Terminator form; `None` ⇒ factory.
    pub arrow_form: Option<ArrowForm>,
    /// Line/text/terminator colour; `None` ⇒ factory (black).
    pub color: Option<Rgb>,
    /// The group's DEFAULT tolerance (`Pass 69.1`); `None` ⇒ factory (none).
    ///
    /// This is the ui-spec's *"document-level default tolerance"* (§C.11.1,
    /// its item 15) landing at the group tier instead — the tier that already
    /// owns every other default, and the one the operator named.
    pub tolerance: Option<Tolerance>,
    /// The group's default tolerance precision; `None` ⇒ factory (follow the
    /// nominal's).
    pub tolerance_places: Option<u32>,
}

impl GroupStyle {
    /// Whether this group overrides nothing at all — i.e. is pure factory.
    ///
    /// Used by the sidecar to keep a never-styled document's bytes exactly as
    /// they were (R34 minimal diff: an all-`None` style writes no keys), and
    /// by any surface that wants to say "this group uses the defaults".
    #[must_use]
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// Tier 3 — one **ce dimension's** overrides. `None` on a field means
/// "inherit" (from the group, which may itself defer to the factory).
///
/// This is the operator's per-ce-dimension checkbox, expressed as data: a
/// ticked box is `Some(value)`, a clear box is `None`. Nothing records "was
/// ticked once" — clearing an override restores inheritance completely, which
/// is deliberately UNLIKE the reference tool, whose `DeleteStyle` leaves the
/// annotations carrying the attributes the style had pushed into them
/// (`SolidWorks_Dimensions` §F.4). pdfcer's cascade is a live link in both
/// directions: change the group and every non-overriding member follows.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct StyleOverrides {
    /// Display unit; `None` ⇒ the group's.
    pub unit: Option<Unit>,
    /// Decimal places / fraction denominator; `None` ⇒ the group's.
    pub fraction: Option<FractionMode>,
    /// Decimal marker; `None` ⇒ the group's.
    pub decimal_marker: Option<DecimalMarker>,
    /// Drafting standard (terminator/text conventions); `None` ⇒ the group's.
    pub standard: Option<DimStandard>,
    /// Label point size; `None` ⇒ the group's, then factory.
    pub text_height: Option<f64>,
    /// Stroke width in points; `None` ⇒ the group's, then factory.
    pub line_width: Option<f64>,
    /// Arrowhead length in points; `None` ⇒ the group's, then factory.
    pub arrow_length: Option<f64>,
    /// Terminator form; `None` ⇒ the group's, then factory.
    pub arrow_form: Option<ArrowForm>,
    /// Colour; `None` ⇒ the group's, then factory.
    pub color: Option<Rgb>,
    /// This ce dimension's tolerance (`Pass 69.1`); `None` ⇒ the group's
    /// default, then factory (none).
    ///
    /// Overriding a group default WITH "no tolerance" is `Some(Tolerance::None)`
    /// — deliberately distinct from `None`, which means inherit. A group that
    /// tolerances everything and one feature that must not be toleranced is a
    /// real drawing, and it cannot be expressed if the two collapse.
    pub tolerance: Option<Tolerance>,
    /// This ce dimension's tolerance precision; `None` ⇒ the group's, then
    /// factory (follow the nominal's).
    pub tolerance_places: Option<u32>,
}

impl StyleOverrides {
    /// Whether this ce dimension overrides nothing — i.e. fully inherits.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// How many properties this ce dimension overrides.
    ///
    /// Exists so a shell can say *"3 properties set on this ce dimension"*
    /// without hand-counting nine `Option`s and quietly missing the one added
    /// next Pass.
    #[must_use]
    pub fn count(&self) -> usize {
        usize::from(self.unit.is_some())
            + usize::from(self.fraction.is_some())
            + usize::from(self.decimal_marker.is_some())
            + usize::from(self.standard.is_some())
            + usize::from(self.text_height.is_some())
            + usize::from(self.line_width.is_some())
            + usize::from(self.arrow_length.is_some())
            + usize::from(self.arrow_form.is_some())
            + usize::from(self.color.is_some())
            + usize::from(self.tolerance.is_some())
            + usize::from(self.tolerance_places.is_some())
    }
}

/// Which tier a resolved property's value actually came from.
///
/// The data behind the operator's *"I cannot change one and be surprised 40
/// others changed or didn't"* requirement. A surface that shows a resolved
/// value without showing its source is showing a number whose behaviour under
/// a group edit is unpredictable — which is precisely the complaint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StyleSource {
    /// pdfcer's built-in default — neither the group nor the ce dimension set
    /// this property.
    Factory,
    /// The group's default — this ce dimension does not override it, so a
    /// group edit WILL move it.
    Group,
    /// Set on this ce dimension — a group edit will NOT move it.
    Dimension,
}

impl StyleSource {
    /// A short stable token (`factory` / `group` / `dimension`) for CLI output
    /// and tests.
    #[must_use]
    pub const fn token(self) -> &'static str {
        match self {
            Self::Factory => "factory",
            Self::Group => "group",
            Self::Dimension => "dimension",
        }
    }

    /// Whether a change to the GROUP's style would move this property.
    ///
    /// The question an operator is really asking when he looks at the
    /// inheritance state, phrased as a predicate so no caller has to
    /// re-derive it from the variant (and get `Factory` wrong — a factory-
    /// sourced property DOES follow a group edit, because the group has simply
    /// not spoken yet).
    #[must_use]
    pub const fn follows_group(self) -> bool {
        matches!(self, Self::Factory | Self::Group)
    }
}

/// Where every resolved property came from — the disclosure companion to
/// [`resolve_style`].
///
/// One field per property, same names as [`StyleOverrides`], so a surface can
/// pair them mechanically instead of maintaining its own mapping that drifts
/// the first time a property is added.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StyleProvenance {
    /// Source of the display unit.
    pub unit: StyleSource,
    /// Source of the decimal places / fraction setting.
    pub fraction: StyleSource,
    /// Source of the decimal marker.
    pub decimal_marker: StyleSource,
    /// Source of the drafting standard.
    pub standard: StyleSource,
    /// Source of the label point size.
    pub text_height: StyleSource,
    /// Source of the stroke width.
    pub line_width: StyleSource,
    /// Source of the arrowhead length.
    pub arrow_length: StyleSource,
    /// Source of the terminator form.
    pub arrow_form: StyleSource,
    /// Source of the colour.
    pub color: StyleSource,
    /// Source of the tolerance.
    pub tolerance: StyleSource,
    /// Source of the tolerance precision.
    pub tolerance_places: StyleSource,
}

impl StyleProvenance {
    /// Every property paired with its source, in a stable order — for a CLI
    /// listing or a panel that renders the whole set uniformly.
    ///
    /// Returned as a fixed-size array rather than an iterator so the caller
    /// gets a compile error, not a silently shorter listing, when a property
    /// is added and this method is not extended.
    #[must_use]
    pub const fn each(&self) -> [(&'static str, StyleSource); 11] {
        [
            ("unit", self.unit),
            ("fraction", self.fraction),
            ("decimal-marker", self.decimal_marker),
            ("standard", self.standard),
            ("text-height", self.text_height),
            ("line-width", self.line_width),
            ("arrow-length", self.arrow_length),
            ("arrow-form", self.arrow_form),
            ("color", self.color),
            ("tolerance", self.tolerance),
            ("tolerance-places", self.tolerance_places),
        ]
    }
}

/// Resolve the three tiers into the single fully-specified style the `/AP`
/// baker draws with.
///
/// The one place the cascade is implemented. [`super::author::author_dimension`]
/// takes the RESULT of this, never the tiers, so the baker cannot accidentally
/// consult the group for a property the ce dimension overrode — a class of bug
/// that would show up as "the override works everywhere except in the saved
/// file".
///
/// # Examples
///
/// ```
/// use pdfcer_core::dimension::{
///     ArrowForm, DimStandard, Group, GroupId, GroupStyle, StyleOverrides, Unit,
///     resolve_style,
/// };
///
/// let mut group = Group::new(GroupId(0), "Default", Unit::Millimeter);
/// group.style.arrow_form = Some(ArrowForm::Slash);
///
/// // A ce dimension that overrides nothing follows the group.
/// let inherited = resolve_style(&group, &StyleOverrides::default());
/// assert_eq!(inherited.arrow_form, ArrowForm::Slash);
/// // ...and falls through to the factory for what the group left unset.
/// assert_eq!(inherited.line_width, 0.75);
///
/// // A ce dimension that overrides it does not.
/// let overridden = resolve_style(
///     &group,
///     &StyleOverrides {
///         arrow_form: Some(ArrowForm::Dot),
///         standard: Some(DimStandard::Iso),
///         ..StyleOverrides::default()
///     },
/// );
/// assert_eq!(overridden.arrow_form, ArrowForm::Dot);
/// assert_eq!(overridden.standard, DimStandard::Iso);
/// ```
#[must_use]
pub fn resolve_style(group: &Group, over: &StyleOverrides) -> DimensionStyle {
    let f = StyleDefaults::FACTORY;
    let format = NumberFormat {
        unit: over.unit.unwrap_or(group.format.unit),
        fraction: over.fraction.unwrap_or(group.format.fraction),
        decimal_marker: over.decimal_marker.unwrap_or(group.format.decimal_marker),
    };
    DimensionStyle {
        // Group-only, deliberately: see the module doc's divergence 3.
        scale: group.scale,
        format,
        standard: over.standard.unwrap_or(group.standard),
        text_height: over
            .text_height
            .or(group.style.text_height)
            .unwrap_or(f.text_height),
        line_width: over
            .line_width
            .or(group.style.line_width)
            .unwrap_or(f.line_width),
        arrow_length: over
            .arrow_length
            .or(group.style.arrow_length)
            .unwrap_or(f.arrow_length),
        arrow_form: over
            .arrow_form
            .or(group.style.arrow_form)
            .unwrap_or(f.arrow_form),
        color: over.color.or(group.style.color).unwrap_or(f.color),
        tolerance: over
            .tolerance
            .or(group.style.tolerance)
            .unwrap_or(f.tolerance),
        tolerance_places: over
            .tolerance_places
            .or(group.style.tolerance_places)
            .or(f.tolerance_places),
    }
}

/// Which tier each property of [`resolve_style`]'s result came from.
///
/// Deliberately a second function rather than a second return value: the
/// baker needs the values and never the sources, and the disclosure surfaces
/// need the sources and usually already have the values. Bundling them would
/// make every `/AP` regeneration compute strings it throws away.
#[must_use]
pub fn style_provenance(group: &Group, over: &StyleOverrides) -> StyleProvenance {
    // The four properties whose group tier is a CONCRETE field on `Group`
    // rather than an `Option`: the group always has an answer, so the only
    // question is whether the ce dimension overrode it. There is no `Factory`
    // outcome for these, and saying otherwise would be a lie an operator could
    // act on ("that will follow the factory default" — it will not; it follows
    // the group).
    let two = |set: bool| {
        if set {
            StyleSource::Dimension
        } else {
            StyleSource::Group
        }
    };
    // The five appearance properties, which have all three tiers.
    let three = |dim: bool, grp: bool| {
        if dim {
            StyleSource::Dimension
        } else if grp {
            StyleSource::Group
        } else {
            StyleSource::Factory
        }
    };
    StyleProvenance {
        unit: two(over.unit.is_some()),
        fraction: two(over.fraction.is_some()),
        decimal_marker: two(over.decimal_marker.is_some()),
        standard: two(over.standard.is_some()),
        text_height: three(
            over.text_height.is_some(),
            group.style.text_height.is_some(),
        ),
        line_width: three(over.line_width.is_some(), group.style.line_width.is_some()),
        arrow_length: three(
            over.arrow_length.is_some(),
            group.style.arrow_length.is_some(),
        ),
        arrow_form: three(over.arrow_form.is_some(), group.style.arrow_form.is_some()),
        color: three(over.color.is_some(), group.style.color.is_some()),
        tolerance: three(over.tolerance.is_some(), group.style.tolerance.is_some()),
        tolerance_places: three(
            over.tolerance_places.is_some(),
            group.style.tolerance_places.is_some(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::super::group::GroupId;
    use super::*;

    fn group() -> Group {
        Group::new(GroupId(0), "Default", Unit::Millimeter)
    }

    /// The load-bearing compatibility claim: a group and a ce dimension that
    /// override nothing resolve to EXACTLY the constants the pre-Pass-69.0
    /// baker hard-coded. If this fails, every existing document redraws.
    #[test]
    fn empty_cascade_is_the_pre_pass_69_hard_coded_appearance() {
        let s = resolve_style(&group(), &StyleOverrides::default());
        assert!((s.text_height - 10.0).abs() < f64::EPSILON, "LABEL_SIZE");
        assert!((s.line_width - 0.75).abs() < f64::EPSILON, "LINE_WIDTH");
        assert!((s.arrow_length - 7.0).abs() < f64::EPSILON, "ARROW_LEN");
        assert_eq!(s.arrow_form, ArrowForm::Filled);
        assert_eq!(s.color, Rgb::BLACK);
    }

    #[test]
    fn dimension_tier_beats_group_tier_beats_factory() {
        let mut g = group();
        g.style.line_width = Some(2.0);
        g.style.arrow_form = Some(ArrowForm::Open);

        let inherited = resolve_style(&g, &StyleOverrides::default());
        assert!((inherited.line_width - 2.0).abs() < f64::EPSILON);
        assert_eq!(inherited.arrow_form, ArrowForm::Open);
        // Untouched by either tier ⇒ factory.
        assert!((inherited.arrow_length - 7.0).abs() < f64::EPSILON);

        let over = StyleOverrides {
            line_width: Some(3.0),
            ..StyleOverrides::default()
        };
        let overridden = resolve_style(&g, &over);
        assert!((overridden.line_width - 3.0).abs() < f64::EPSILON);
        // The group's OTHER default still applies — overriding one property
        // must not detach the ce dimension from the group wholesale.
        assert_eq!(overridden.arrow_form, ArrowForm::Open);
    }

    #[test]
    fn provenance_names_the_tier_that_supplied_each_property() {
        let mut g = group();
        g.style.line_width = Some(2.0);
        let over = StyleOverrides {
            arrow_form: Some(ArrowForm::Dot),
            unit: Some(Unit::Inch),
            ..StyleOverrides::default()
        };
        let p = style_provenance(&g, &over);
        assert_eq!(p.line_width, StyleSource::Group);
        assert_eq!(p.arrow_form, StyleSource::Dimension);
        assert_eq!(p.arrow_length, StyleSource::Factory);
        assert_eq!(p.unit, StyleSource::Dimension);
        // A property whose group tier is a concrete field never reports
        // Factory — the group always has an answer for it.
        assert_eq!(p.standard, StyleSource::Group);
        assert_eq!(p.each().len(), 11);
    }

    /// `follows_group` is the predicate a UI actually needs, and getting it
    /// wrong for `Factory` is the easy mistake: a factory-sourced property
    /// DOES move when the group sets one.
    #[test]
    fn factory_sourced_properties_still_follow_a_group_edit() {
        assert!(StyleSource::Factory.follows_group());
        assert!(StyleSource::Group.follows_group());
        assert!(!StyleSource::Dimension.follows_group());
    }

    #[test]
    fn scale_is_never_overridable_per_ce_dimension() {
        // Asserted structurally: `StyleOverrides` has no scale field, so this
        // test is really about the resolved value always being the group's.
        let mut g = group();
        g.scale = super::super::units::ScaleState::Calibrated { scale: 0.01 };
        let s = resolve_style(&g, &StyleOverrides::default());
        assert_eq!(
            s.scale,
            super::super::units::ScaleState::Calibrated { scale: 0.01 }
        );
    }

    #[test]
    fn arrow_form_tokens_round_trip() {
        for f in ArrowForm::ALL {
            assert_eq!(ArrowForm::parse(f.token()), Some(f), "{}", f.token());
        }
        assert_eq!(ArrowForm::parse("FILLED"), Some(ArrowForm::Filled));
        assert_eq!(ArrowForm::parse("wide"), None);
    }

    #[test]
    fn override_counting_is_not_hand_maintained_at_call_sites() {
        assert_eq!(StyleOverrides::default().count(), 0);
        assert!(StyleOverrides::default().is_empty());
        let over = StyleOverrides {
            unit: Some(Unit::Inch),
            color: Some(Rgb::BLACK),
            ..StyleOverrides::default()
        };
        assert_eq!(over.count(), 2);
        assert!(!over.is_empty());
    }
}
