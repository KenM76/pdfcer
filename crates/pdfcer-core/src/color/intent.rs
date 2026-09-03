//! **The PDF rendering intent** — `/RI`, `ri`, and an image's `/Intent`
//! (ISO 32000-1 §8.6.5.8, Table 70; ISO 32000-2 Table 69) (`Pass 199.0`).
//!
//! # Why this type exists at all
//!
//! pdfcer **parsed the `ri` operator and threw the value away** — a recognised
//! no-op — until this Pass. That was not a quality-of-implementation gap, and
//! the distinction is worth being exact about because the clause reads
//! permissive at first glance:
//!
//! - §8.6.5.8, **`shall`**: *"Table 70 lists the standard rendering intents
//!   that **shall be recognized**."*
//! - §8.6.5.8, **`shall`**: an unrecognised name *"**shall** use the
//!   `RelativeColorimetric` intent by default"*.
//! - §11.7.5.3, **`shall`**: *"the rendering intent used **shall be** the
//!   current rendering intent in effect in the graphics state at the time of
//!   the painting operation."*
//!
//! ★ **The sentence that makes it look optional has been struck.** The printed
//! NOTE says a device *"does not have to support all PDF rendering intents"* —
//! and ISO-approved erratum `pdf-issues` #63 (closed 2021-04-16) removes it,
//! its resolution reading *"NOTEs are informative only … the existing normative
//! requirements to support all 4 rendering intents remains"*. A reader working
//! from the printed page alone would conclude the opposite of the truth.
//!
//! # ★★ FOUR DIFFERENT DEFAULTS, AND MERGING THEM IS THE FAILURE MODE
//!
//! They are separate rules with separate clauses, and a single
//! `Default::default()` used for all four would be wrong three times:
//!
//! | | question | answer | clause |
//! |---|---|---|---|
//! | **D1** | the intent at page start | [`RenderingIntent::RelativeColorimetric`] | ISO 32000-1 Table 52's *Initial value*, made binding by §8.4.1 (`shall`) |
//! | **D2** | a name that is not one of the four | [`RenderingIntent::RelativeColorimetric`] | §8.6.5.8 (`shall`) — see [`RenderingIntent::from_name`] |
//! | **D3** | an image with no `/Intent` | **the graphics state's current intent**, NOT a constant | ISO 32000-1 Table 89's *Default value* |
//! | **D4** | the page group → device conversion | `RelativeColorimetric` in ISO 32000-2 §11.4.7 (`shall`); ISO 32000-1 says *"the default rendering intent for the page"*, a term it uses once and never defines | — |
//!
//! ★★★ **D4 IS THE ONE THAT WOULD HAVE BEEN GOT WRONG.** A content stream
//! saying `ri /Saturation` does **not** govern the final page→device hop. The
//! graphics-state intent governs *painting* (§11.7.5.3); the page group's
//! conversion to the device is its own step with its own answer. Conflating
//! them would apply a source-side intent to a destination-side conversion and
//! be wrong in exactly the cases anyone would test.
//!
//! # ★ And a fifth rule that is NOT a default: `gs` does not reset it
//!
//! An `/ExtGState` **without** `/RI` leaves the intent alone. §8.4.5: *"The
//! results of `gs` **shall be cumulative** … parameter values … persist until
//! explicitly overridden."* ISO 32000-2's Table 57 uniquely printed *"The
//! default value is: Default"* for this entry, and ISO-approved erratum
//! `pdf-issues` #360 **deletes** it precisely because no other entry claims
//! one. It was re-raised as #746 in 2026 and closed as a duplicate, so it is a
//! live implementer trap rather than a historical curiosity.
//!
//! # What the standard does NOT give you, stated so nobody looks for it
//!
//! An output metric. The two **colorimetric** intents carry a testable
//! constraint (§8.6.5.8, `shall`: *"In-gamut colours shall be reproduced
//! exactly; out-of-gamut colours shall be mapped to the nearest value within
//! the reproducible gamut"*). `Saturation` and `Perceptual` do not:
//! reproduction *"may or may not be colourimetrically accurate"* and colours
//! *"shall be generally modified"*. ISO 32000-1 §10.2 puts gamut mapping
//! squarely in the reader: *"the gamut mapping and colour mapping functions are
//! part of the implementation of the conforming reader"*. ISO 32000-2 §10.3.1
//! defers to ICC.1:2010, whose own clause 0.4 then says *"the colour rendering
//! of the perceptual and saturation rendering intents is **vendor specific**"*.
//!
//! ⇒ **The obligation to honour the intent is real and hard; the metric for two
//! of the four does not exist in any standard.** So pdfcer's job is to carry the
//! file's choice faithfully to whatever converts colour — never to decide that
//! one intent "looks right" because a fixture matches it.
//!
//! # Not to be confused with [`crate::settings::CmykIntent`]
//!
//! That is pdfcer's own CMYK→sRGB *back*-conversion policy
//! (`Calibrated`/`NeutralBlack`). It is a display choice about a table pdfcer
//! ships. This is the document's own declared intent for a *forward*
//! conversion. They share a word and nothing else, and mapping one onto the
//! other would be a category error.

/// A PDF rendering intent (§8.6.5.8, ISO 32000-1 Table 70 / ISO 32000-2
/// Table 69).
///
/// The four names are character-identical in both editions — including the
/// American `Color` spelling, which both keep even where ISO 32000-2 switches
/// the surrounding prose to `colorimetric`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum RenderingIntent {
    /// Colours are represented with respect to the combination of the output
    /// device's light source and the paper's absolute white.
    AbsoluteColorimetric,
    /// The same, but relative to the output device's white point.
    ///
    /// **The default** — D1 and D2 both land here (see the module docs), which
    /// is why it is the `Default` impl.
    #[default]
    RelativeColorimetric,
    /// Saturation is preserved or emphasised; in-gamut colours *"may or may
    /// not"* be colourimetrically accurate.
    Saturation,
    /// Both in-gamut and out-of-gamut colours *"shall be generally modified"*
    /// from their precise colourimetric values.
    Perceptual,
}

impl RenderingIntent {
    /// Resolve a `/RI`, `ri` or `/Intent` name.
    ///
    /// # ★ An unrecognised name is not an error — it is `RelativeColorimetric`
    ///
    /// §8.6.5.8, `shall`: *"If a conforming reader does not recognize the
    /// specified name, it shall use the `RelativeColorimetric` intent by
    /// default."* So this returns the intent rather than an `Option`, and a
    /// caller cannot accidentally treat an unknown name as "no intent
    /// specified" — which is a **different** state (that one leaves the
    /// graphics state alone; this one overwrites it with
    /// `RelativeColorimetric`).
    ///
    /// The caller that needs to tell those two apart is the `gs` handler: an
    /// `/ExtGState` with no `/RI` at all must not touch the intent (§8.4.5,
    /// cumulative), while one carrying `/RI /Nonsense` must set it to
    /// `RelativeColorimetric`. That is why this function takes a name and the
    /// *absence* is handled by the caller not calling it.
    ///
    /// # Examples
    ///
    /// ```
    /// use pdfcer_core::color::RenderingIntent;
    ///
    /// assert_eq!(
    ///     RenderingIntent::from_name(b"Saturation"),
    ///     RenderingIntent::Saturation
    /// );
    /// // §8.6.5.8: an unrecognised name is RelativeColorimetric, not an error.
    /// assert_eq!(
    ///     RenderingIntent::from_name(b"NoSuchIntent"),
    ///     RenderingIntent::RelativeColorimetric
    /// );
    /// ```
    #[must_use]
    pub fn from_name(name: &[u8]) -> Self {
        match name {
            b"AbsoluteColorimetric" => Self::AbsoluteColorimetric,
            b"RelativeColorimetric" => Self::RelativeColorimetric,
            b"Saturation" => Self::Saturation,
            b"Perceptual" => Self::Perceptual,
            // ★ ISO 32000-2 §8.6.5.9 writes `AbsColorimetric` once, a token
            // defined nowhere else in 1,023 pages and carrying no erratum.
            // Read as `AbsoluteColorimetric` -- recorded rather than
            // normalised, because guessing at a typo in a standard is a
            // judgement and should look like one.
            b"AbsColorimetric" => Self::AbsoluteColorimetric,
            _ => Self::RelativeColorimetric,
        }
    }

    /// The name this intent is written as, for round-tripping and disclosure.
    ///
    /// `AbsColorimetric` is deliberately NOT produced: it is a typo pdfcer
    /// tolerates on read and must never emit.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::AbsoluteColorimetric => "AbsoluteColorimetric",
            Self::RelativeColorimetric => "RelativeColorimetric",
            Self::Saturation => "Saturation",
            Self::Perceptual => "Perceptual",
        }
    }

    /// Whether the standard constrains this intent's OUTPUT in a testable way.
    ///
    /// `true` for the two colorimetric intents, which carry *"in-gamut colours
    /// shall be reproduced exactly"*. `false` for `Saturation` and
    /// `Perceptual`, whose rendering ICC.1:2010 clause 0.4 calls *"vendor
    /// specific"*.
    ///
    /// Exposed because it is the honest answer to *"can a test assert a colour
    /// here?"* — and because this project has already been tempted to read a
    /// matching ink measurement as proof that one intent is the correct one.
    /// It is not: a fixture authored against a particular colour engine and a
    /// transform that coincidentally lands near that engine's output are
    /// indistinguishable from the numbers alone.
    #[must_use]
    pub const fn output_is_constrained(self) -> bool {
        matches!(
            self,
            Self::AbsoluteColorimetric | Self::RelativeColorimetric
        )
    }
}

/// Resolve **D3** — the intent in force for one image XObject
/// (ISO 32000-1 Table 89's `/Intent` row).
///
/// # The rule, and the two ways it is got wrong
///
/// Table 89 gives `/Intent` the default *"the current rendering intent in the
/// graphics state"*. That is the present-overrides / absent-inherits idiom, so:
///
/// - **present** → it wins, **for this image only**. ISO 32000-2 strengthens
///   the verb to *"shall be used"*.
/// - **absent** → the graphics-state intent, unchanged. Not a constant — this
///   is the trap. Three of the four defaults in this module are
///   `RelativeColorimetric` and this one is **not**, so a single
///   `unwrap_or_default()` here would be wrong on every page that sets an
///   intent at the top and then draws an image.
///
/// ★ **`is_image_mask` suppresses it.** ISO 32000-2 adds *"ignored if
/// `ImageMask` is `true`"* to Table 87's `/Intent` row, and it follows from
/// §8.9.6.2 anyway: a stencil mask carries no colour at all, so there is
/// nothing for an intent to govern. Passed in rather than re-read here because
/// the caller has already resolved it — and because re-reading a key the caller
/// has decided about is how two answers to one question appear.
///
/// # ★★ A soft-mask image's `/Intent` is IGNORED, and that is not this
/// # function's job
///
/// ISO 32000-1 Table 145 / ISO 32000-2 Table 143 give the `/SMask` image
/// dictionary's `Intent` row the single word **`Ignored.`** — verbatim, and
/// unamended in both editions. A caller decoding a soft-mask image must not
/// call this at all. Stated here because the entry point looks identical: it is
/// an image dictionary that may carry `/Intent`, and the difference is what the
/// image is being used FOR.
///
/// # Examples
///
/// ```
/// use pdfcer_core::color::{RenderingIntent, image_intent};
///
/// let gs = RenderingIntent::Saturation;
/// // Absent: the graphics state's intent survives.
/// assert_eq!(image_intent(gs, None, false), RenderingIntent::Saturation);
/// // Present: it wins, for this image only.
/// assert_eq!(
///     image_intent(gs, Some(b"Perceptual"), false),
///     RenderingIntent::Perceptual
/// );
/// // An image mask has no colour, so the entry is ignored.
/// assert_eq!(image_intent(gs, Some(b"Perceptual"), true), RenderingIntent::Saturation);
/// ```
#[must_use]
pub fn image_intent(
    graphics_state: RenderingIntent,
    image_intent_name: Option<&[u8]>,
    is_image_mask: bool,
) -> RenderingIntent {
    match image_intent_name {
        Some(name) if !is_image_mask => RenderingIntent::from_name(name),
        _ => graphics_state,
    }
}

impl std::fmt::Display for RenderingIntent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}
