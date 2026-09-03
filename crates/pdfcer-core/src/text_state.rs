//! # `text_state` — the ONE model of ISO 32000-1 §9.3's text-state parameters
//!
//! ## Why this module exists
//!
//! Before Pass 19.0, `pdfcer-core` tracked the §9.3 text state **three
//! times, privately, and published it zero times**:
//!
//! | tracker | crate module | what it kept | who could read it |
//! |---|---|---|---|
//! | `TextState` | [`crate::text_extract::page`] | `Tc Tw Tz TL Ts Tr` | nobody (private) |
//! | `Walk` + `BlockTextState` | [`crate::text_edit::edit`] / [`crate::text_edit::reflow_apply`] | `Tc Tw Tz` only | crate-private |
//! | `GState` | [`crate::vector::decompose`] | `Tc Tw Tz TL Ts` | nobody (private) |
//!
//! Three copies that agree today are three copies that can disagree
//! tomorrow — this project's recurring failure shape (decision 011 §Z2),
//! and the same argument that produced
//! [`advance_tx`](crate::text_extract::font) as a single function. The
//! divergence was already real, not hypothetical: the **authoring** walk
//! (`text_edit::edit::Walk`) had no `Ts` arm and no `Tr` arm at all, so
//! pdfcer could not restore an ambient text rise it had never observed —
//! and it had no `q`/`Q` arms either, so any text state (or fill colour)
//! set inside a `q … Q` bracket leaked past the `Q` in the model even
//! though every conforming reader discards it there.
//!
//! This module is the consolidation. It owns:
//!
//! - [`TextStateParam`] — the identity of one parameter (its operator
//!   name, its Table 105 initial value, the bytes that restore that
//!   initial value).
//! - [`TextStateParams`] — the six parameters **resolved to values**, for
//!   consumers that only do arithmetic (the vector decomposer's text
//!   bounding box, the extraction advance formula).
//! - [`AmbientValue`] / [`AmbientOrigin`] / [`AmbientTextState`] — the
//!   same six parameters plus **where each came from**, which is what an
//!   authoring path needs in order to put the stream back the way it
//!   found it.
//! - [`AmbientRestoreError`] — the refusal that fires when a restore
//!   cannot be honestly emitted.
//!
//! ## The three-tier restore ladder (standing rule R88, decision 019 §3.4)
//!
//! pdfcer scopes a text-state operator it emits for one run by **explicit
//! restore-by-value**, never by `q`/`Q`: `q`/`Q` are "Special graphics
//! state" operators and are **not admitted inside a text object** (ISO
//! 32000-1 §8.2 Table 51 / Figure 9), and splitting the `BT … ET` to use
//! them would discard `Tm` (§9.4.1) and destroy the minimal-diff property
//! `ARCHITECTURE.md` §5 is built on.
//!
//! Restoring by value requires knowing what the ambient value *was*, and
//! there are exactly three epistemic states it can be in. [`AmbientOrigin`]
//! is that trichotomy, and it is deliberately modelled in the type system
//! so a caller cannot skip the third case:
//!
//! 1. [`AmbientOrigin::Initial`] — no operator has set the parameter in
//!    this content stream, so Table 105's initial value is provably in
//!    force. Restore by emitting the spec default
//!    ([`TextStateParam::initial_restore_bytes`]).
//! 2. [`AmbientOrigin::Observed`] — an operator set it, and its **raw
//!    operand bytes as written** were captured. Restore by re-emitting
//!    those bytes, so `0.5000 Tc` goes back as `0.5000 Tc` and not as a
//!    renormalized `0.5 Tc`. (Byte fidelity is not cosmetic here: a
//!    re-normalized number is a diff in an object pdfcer claims not to
//!    have logically touched.)
//! 3. [`AmbientOrigin::Unobservable`] — the value is *in force* and
//!    *known*, but the operator that set it is **not in the buffer being
//!    edited**, so no restore can be emitted into that buffer. **Refuse
//!    and disclose** ([`AmbientRestoreError`]) — never guess the default.
//!    Emitting a guessed `0 Tc` would silently change content pdfcer did
//!    not touch, which is precisely the rule-4 (fuzzy-never-sneaky)
//!    failure this project exists not to make.
//!
//! Note carefully that tier 3 still carries the **value**
//! ([`AmbientValue::value`] is always populated). Unobservability is a
//! statement about *restorability*, not about knowledge: a run inside a
//! form XObject inherits a perfectly well-defined text state from its
//! invoking context (§8.10.1), and the §9.4.4 advance arithmetic needs
//! that number. What it cannot do is write a restore for it into the
//! form's own stream.
//!
//! ## Which parameters, and why these six
//!
//! Table 104 lists nine text-state parameters. This module models the six
//! set by a single-numeric-operand operator — `Tc`, `Tw`, `Tz`, `TL`,
//! `Ts`, `Tr` — and deliberately excludes three:
//!
//! - **`Tf` / `Tfs`** (font and size). Their *values* are shared but their
//!   *representations* are not: the extraction walk narrows `Tfs` to `f32`
//!   because [`GlyphProvenance::tf_size`](crate::text_extract::GlyphProvenance::tf_size)
//!   publishes `f32`, while the authoring and vector walks keep `f64`; and
//!   the resolved font is an `Rc<ExtractFont>` in one walk and an
//!   `Arc<ExtractFont>` in another. Forcing them into a shared struct
//!   would change the precision at which the extraction advance formula
//!   evaluates, which would move published glyph positions — an
//!   observable output change this correctness slice must not make. They
//!   stay with their consumers, alongside the font handle they belong to.
//! - **`Tk`** (text knockout, §9.3.8) — set by `/TK` in an ExtGState, not
//!   by a content operator, and read by nothing in pdfcer today.
//!
//! The six modelled here are exactly the set standing rule R88 governs.
//!
//! ## Spec citations
//!
//! - §9.3, Table 104 — the nine text-state parameters.
//! - §9.3, Table 105 — the operators and their initial values:
//!   `Tc` = 0, `Tw` = 0, `Tz` = 100, `TL` = 0, `Tr` = 0, `Ts` = 0.
//! - §9.3 scope rule — text state operators "may appear outside text
//!   objects, and the values they set are retained across text objects in
//!   a single content stream"; they are graphics state and are therefore
//!   saved and restored by `q`/`Q` (§8.4.2), and initialized to their
//!   defaults at the start of each page.
//! - §9.3.3 — `Tw` "shall NOT apply to occurrences of the byte value 32 in
//!   multiple-byte codes", which makes it structurally void on composite
//!   (2-byte) runs. See [`AmbientTextState::word_spacing`] and the
//!   `composite` flag published on `GlyphProvenance`.
//! - §9.3.4 — the `Tz` operand is a **percentage**; `Th` = operand ÷ 100.
//!   This module stores the **operand**, not the ratio, because the
//!   operand is what a restore must emit; [`AmbientTextState::params`]
//!   does the division for arithmetic consumers.
//! - §8.10.1 — a form XObject inherits the graphics state of its invoking
//!   context and cannot leak state back out. The inheritance is what makes
//!   tier 3 necessary.
//! - §8.2 Table 51 / Figure 9 — `q`/`Q` are not permitted inside a text
//!   object, which is why restore-by-value exists at all.

use std::fmt;
use std::sync::Arc;

use thiserror::Error;

// =====================================================================
// Parameter identity
// =====================================================================

/// One of the six §9.3 text-state parameters pdfcer models as a shared,
/// restorable quantity.
///
/// Carrying the parameter as a value (rather than as six near-identical
/// code paths) is what lets [`AmbientValue::restore_bytes`] be written
/// once: the operator spelling, the Table 105 initial value, and the
/// operator-facing name all hang off this enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum TextStateParam {
    /// `Tc` — character spacing (§9.3.2), unscaled text-space units.
    CharSpacing,
    /// `Tw` — word spacing (§9.3.3), unscaled text-space units. Applies
    /// **only** to the single-byte code 32.
    WordSpacing,
    /// `Tz` — horizontal scaling (§9.3.4), a **percentage**.
    HorizScale,
    /// `TL` — leading (§9.3.5), unscaled text-space units.
    Leading,
    /// `Ts` — text rise (§9.3.7), unscaled text-space units.
    Rise,
    /// `Tr` — text rendering mode (§9.3.6, Table 106), an integer.
    RenderMode,
}

impl TextStateParam {
    /// Every modelled parameter, in Table 105 order — for exhaustive
    /// iteration by a caller that must consider all of them (a restore
    /// planner, a diagnostic dump) without hand-listing the set and
    /// silently missing one when a seventh is added.
    pub const ALL: [Self; 6] = [
        Self::CharSpacing,
        Self::WordSpacing,
        Self::HorizScale,
        Self::Leading,
        Self::Rise,
        Self::RenderMode,
    ];

    /// The content-stream operator that sets this parameter, as it is
    /// spelled in the stream (§9.3 Table 105).
    #[must_use]
    pub const fn operator(self) -> &'static [u8] {
        match self {
            Self::CharSpacing => b"Tc",
            Self::WordSpacing => b"Tw",
            Self::HorizScale => b"Tz",
            Self::Leading => b"TL",
            Self::Rise => b"Ts",
            Self::RenderMode => b"Tr",
        }
    }

    /// Table 105's **initial value** for this parameter — the value in
    /// force at the start of every page, before any operator runs.
    ///
    /// This is the *operand* value, not a derived one: `Tz`'s initial is
    /// **100** (a percentage), not `1.0` (the `Th` ratio).
    #[must_use]
    pub const fn initial_operand(self) -> f64 {
        match self {
            Self::HorizScale => 100.0,
            Self::CharSpacing
            | Self::WordSpacing
            | Self::Leading
            | Self::Rise
            | Self::RenderMode => 0.0,
        }
    }

    /// The exact operator bytes that reinstate this parameter's Table 105
    /// initial value — the tier-1 restore of the R88 ladder.
    ///
    /// Written as literal bytes rather than formatted from
    /// [`Self::initial_operand`] so the emitted spelling is fixed and
    /// reviewable (`100 Tz`, never `100.0 Tz` or `1e2 Tz`), and so it
    /// cannot drift with a number-formatting change elsewhere.
    #[must_use]
    pub const fn initial_restore_bytes(self) -> &'static [u8] {
        match self {
            Self::CharSpacing => b"0 Tc",
            Self::WordSpacing => b"0 Tw",
            Self::HorizScale => b"100 Tz",
            Self::Leading => b"0 TL",
            Self::Rise => b"0 Ts",
            Self::RenderMode => b"0 Tr",
        }
    }

    /// A short human-readable name, for a diagnostic or a disclosure
    /// string. Deliberately the typographic name, not the operator: a
    /// refusal that says "character spacing" is readable by an operator
    /// who has never opened the PDF specification.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::CharSpacing => "character spacing",
            Self::WordSpacing => "word spacing",
            Self::HorizScale => "horizontal scaling",
            Self::Leading => "leading",
            Self::Rise => "text rise",
            Self::RenderMode => "text rendering mode",
        }
    }

    /// Which parameter an operator name sets, if any.
    ///
    /// `None` for every operator that is not one of the six — including
    /// `Tf` (two operands, one of them a name; see the module docs for why
    /// it is not modelled here) and `"` (which sets `Tw` **and** `Tc` and
    /// therefore cannot be described by a single parameter; callers handle
    /// it explicitly, as [`AmbientTextState::apply_operator`] does).
    #[must_use]
    pub fn from_operator(name: &[u8]) -> Option<Self> {
        match name {
            b"Tc" => Some(Self::CharSpacing),
            b"Tw" => Some(Self::WordSpacing),
            b"Tz" => Some(Self::HorizScale),
            b"TL" => Some(Self::Leading),
            b"Ts" => Some(Self::Rise),
            b"Tr" => Some(Self::RenderMode),
            _ => None,
        }
    }
}

impl fmt::Display for TextStateParam {
    /// Formats as the operator name (`Tc`, `Tz`, …) — the form a
    /// spec-literate diagnostic wants. Use [`Self::label`] for prose.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // `operator()` is always ASCII, so the lossy conversion cannot lose
        // anything; it is used only because the table is `&[u8]`.
        f.write_str(&String::from_utf8_lossy(self.operator()))
    }
}

// =====================================================================
// Resolved values (arithmetic consumers)
// =====================================================================

/// The six modelled §9.3 parameters **resolved to numbers**, for the
/// consumers that only ever do arithmetic with them.
///
/// This is what a *reading* walk needs: the vector decomposer's
/// approximate text bounding box and the §9.4.4 advance formula care about
/// the value in force and nothing else. An *authoring* walk needs
/// [`AmbientTextState`] instead, because it additionally has to put the
/// stream back.
///
/// `h_scale` is the **ratio `Th`** (already divided by 100), not the `Tz`
/// operand — the form §9.4.4's displacement formula multiplies by. This is
/// the one place the two representations differ, and it is deliberate:
/// arithmetic wants the ratio, a restore wants the operand.
///
/// # Examples
///
/// ```
/// use pdfcer_core::text_state::TextStateParams;
///
/// let initial = TextStateParams::INITIAL;
/// assert_eq!(initial.char_spacing, 0.0);
/// assert_eq!(initial.h_scale, 1.0); // Tz = 100 ⇒ Th = 1.0
/// assert_eq!(initial.render_mode, 0);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct TextStateParams {
    /// `Tc` — character spacing (§9.3.2), unscaled text-space units.
    pub char_spacing: f64,
    /// `Tw` — word spacing (§9.3.3), unscaled text-space units. Enters the
    /// advance **only** for a single-byte code 32.
    pub word_spacing: f64,
    /// `Th` — horizontal scaling as a **ratio** (`Tz` ÷ 100, §9.3.4).
    pub h_scale: f64,
    /// `TL` — leading (§9.3.5).
    pub leading: f64,
    /// `Trise` — text rise (§9.3.7). Enters `Trm` as a translation, so it
    /// moves the glyph but does **not** change its advance.
    pub rise: f64,
    /// `Tmode` — text rendering mode (§9.3.6, Table 106). Modes 3 and 7
    /// paint nothing.
    pub render_mode: i64,
}

impl TextStateParams {
    /// Table 105's initial values — the state at the start of every page.
    pub const INITIAL: Self = Self {
        char_spacing: 0.0,
        word_spacing: 0.0,
        h_scale: 1.0,
        leading: 0.0,
        rise: 0.0,
        render_mode: 0,
    };
}

impl Default for TextStateParams {
    /// Table 105's initial values (**not** an all-zero struct — `Th` is
    /// 1.0, and a zeroed `h_scale` would collapse every advance to zero).
    fn default() -> Self {
        Self::INITIAL
    }
}

// =====================================================================
// Ambient state (authoring consumers)
// =====================================================================

/// Why an ambient value cannot be restored by re-emitting bytes into the
/// content stream being edited.
///
/// One variant today; `#[non_exhaustive]` because the set is a property of
/// pdfcer's architecture and will grow as more of the object model becomes
/// editable (an ExtGState-set parameter and a Type 3 glyph procedure are
/// the two obvious future members).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum UnobservableAmbient {
    /// The value was inherited from the context that invoked a form
    /// XObject (§8.10.1), so the operator that set it lives in a
    /// **different** content stream from the run being edited.
    ///
    /// Re-emitting a restore inside the form would be wrong twice over:
    /// the form has no such operator to reproduce, and any value written
    /// there would be discarded at the form's implicit `Q` anyway.
    FormXObject {
        /// The form stream's object number, when the `Do` named an
        /// indirect reference. `None` for a direct stream, which cannot be
        /// named — the refusal still stands, it just cannot cite an id.
        object: Option<u32>,
    },
}

impl fmt::Display for UnobservableAmbient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FormXObject { object: Some(num) } => write!(
                f,
                "it was inherited from the context that invoked form XObject {num} (ISO 32000-1 \
                 §8.10.1), so the operator that set it is in a different content stream"
            ),
            Self::FormXObject { object: None } => f.write_str(
                "it was inherited from the context that invoked a form XObject (ISO 32000-1 \
                 §8.10.1), so the operator that set it is in a different content stream",
            ),
        }
    }
}

/// Where an ambient text-state value came from — the R88 restore ladder,
/// modelled so the third tier cannot be forgotten.
///
/// See the module docs for the full rationale. In short: tier 1 restores
/// the spec default, tier 2 restores the observed raw bytes, tier 3
/// refuses.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum AmbientOrigin {
    /// No operator has set this parameter in the content stream being
    /// walked, so Table 105's initial value is provably in force.
    Initial,
    /// An operator set it, and its raw bytes were captured.
    Observed {
        /// The setting operator's bytes **exactly as written**, operands
        /// included (e.g. `0.5000 Tc`). Shared rather than owned because
        /// one ambient state is cloned onto every glyph of a run when
        /// provenance capture is on, and a refcount bump is the whole cost.
        raw: Arc<[u8]>,
    },
    /// An operator set it **as a side effect**, so that operator's bytes
    /// are *not* a usable restore — re-emitting them would do the other
    /// thing as well.
    ///
    /// There are exactly two such operators, and both are traps a restore
    /// would fall into if this variant did not exist:
    ///
    /// - **`TD`** (§9.4.2 Table 108) "sets the leading parameter to −ty"
    ///   *and* moves to the next line. Re-emitting `72 -14 TD` to restore
    ///   `TL` would also displace every following glyph.
    /// - **`"`** (§9.4.3 Table 109) sets `Tw` and `Tc` *and shows a
    ///   string*. Re-emitting `2 0.25 (hi) "` to restore `Tw` would paint
    ///   the word "hi" onto the page a second time.
    ///
    /// The **value** is fully known, so this is not the refuse tier: the
    /// restore re-spells it as its own operator (`-14 TL`, `2 Tw`). That is
    /// a *narrowing* of byte fidelity, not a guess, and callers disclose it
    /// via [`AmbientValue::is_byte_faithful`] — the same posture
    /// `fill_narrowed` already takes for an unmodelled colour space.
    ObservedIndirect {
        /// The operator that set it, for the disclosure (`TD` or `"`).
        setter: &'static str,
    },
    /// The value is in force and known, but not restorable into this
    /// buffer. See [`UnobservableAmbient`].
    Unobservable(UnobservableAmbient),
}

/// A refusal to emit a text-state restore, because the ambient value is
/// [`AmbientOrigin::Unobservable`].
///
/// This is an error type rather than an `Option` on purpose: the caller
/// must **disclose** the refusal to the operator (rule 4), and an error
/// with a `Display` that reads as a disclosure is what makes forgetting
/// awkward. A guessed default restore is never emitted.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum AmbientRestoreError {
    /// The ambient value for `param` cannot be restored; `reason` says why.
    #[error(
        "cannot restore the ambient {} ({param}) after this edit: {reason} — pdfcer refuses rather \
         than emitting a guessed default, which would silently change content it did not touch \
         (decision 019 §3.4, rule R88)",
        param.label()
    )]
    Unobservable {
        /// Which parameter could not be restored.
        param: TextStateParam,
        /// Why it is unobservable.
        reason: UnobservableAmbient,
    },
}

/// One text-state parameter's ambient value **and** its provenance.
///
/// [`Self::value`] is always populated, including in the
/// [`AmbientOrigin::Unobservable`] case — see the module docs on why
/// unobservability is a statement about restorability, not knowledge.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct AmbientValue {
    /// The parameter's value in force, as the **operand** would be written
    /// (so `h_scale` here is the `Tz` percentage, e.g. `90.0`, not `0.9`).
    pub value: f64,
    /// Where the value came from, and hence whether it can be restored.
    pub origin: AmbientOrigin,
}

impl AmbientValue {
    /// The Table 105 initial value for `param`, marked as never having
    /// been set.
    #[must_use]
    pub const fn initial(param: TextStateParam) -> Self {
        Self {
            value: param.initial_operand(),
            origin: AmbientOrigin::Initial,
        }
    }

    /// An observed value with the raw operator bytes that set it.
    #[must_use]
    pub fn observed(value: f64, raw: &[u8]) -> Self {
        Self {
            value,
            origin: AmbientOrigin::Observed {
                raw: Arc::from(raw),
            },
        }
    }

    /// A value set as a side effect of `setter` (`TD` or `"`), whose bytes
    /// are therefore not a usable restore. See
    /// [`AmbientOrigin::ObservedIndirect`].
    #[must_use]
    pub const fn observed_indirect(value: f64, setter: &'static str) -> Self {
        Self {
            value,
            origin: AmbientOrigin::ObservedIndirect { setter },
        }
    }

    /// Whether this value can be restored at all — `false` exactly when
    /// [`Self::restore_bytes`] would refuse.
    ///
    /// Offered so a planner can decide *whether to attempt* an edit before
    /// building any bytes, rather than discovering the refusal halfway
    /// through assembling a splice.
    #[must_use]
    pub const fn is_restorable(&self) -> bool {
        !matches!(self.origin, AmbientOrigin::Unobservable(_))
    }

    /// Whether a restore of this value would reproduce the producer's own
    /// bytes, rather than pdfcer's re-spelling of the same number.
    ///
    /// `false` only for [`AmbientOrigin::ObservedIndirect`] — the value is
    /// right, the spelling is pdfcer's. Callers **disclose** that, exactly
    /// as `fill_narrowed` discloses a narrowed colour restore: the operator
    /// is told that a byte in the file changed shape even though nothing
    /// changed meaning.
    #[must_use]
    pub const fn is_byte_faithful(&self) -> bool {
        !matches!(self.origin, AmbientOrigin::ObservedIndirect { .. })
    }

    /// The name of the side-effect operator that set this value (`TD` or
    /// `"`), when the origin is [`AmbientOrigin::ObservedIndirect`].
    ///
    /// Exists so a caller can *name the culprit* in its narrowing
    /// disclosure — "restored by re-spelling because `\"` also shows a
    /// string" is an explanation an operator can act on, while "restored by
    /// re-spelling" alone is a shrug. `None` for every other origin.
    #[must_use]
    pub const fn indirect_setter(&self) -> Option<&'static str> {
        match self.origin {
            AmbientOrigin::ObservedIndirect { setter } => Some(setter),
            _ => None,
        }
    }

    /// The operator bytes that reinstate this value — the R88 ladder.
    ///
    /// - [`AmbientOrigin::Initial`] → `param`'s spec-default bytes.
    /// - [`AmbientOrigin::Observed`] → the recorded raw bytes, verbatim,
    ///   so a `0.5000 Tc` is not renormalized to `0.5 Tc`.
    /// - [`AmbientOrigin::ObservedIndirect`] → the value **re-spelled** as
    ///   `param`'s own operator, because the operator that set it does
    ///   something else as well. Disclose via [`Self::is_byte_faithful`].
    /// - [`AmbientOrigin::Unobservable`] → [`Err`], never a guess.
    ///
    /// # Errors
    ///
    /// [`AmbientRestoreError::Unobservable`] when the ambient value was
    /// inherited from outside the content stream being edited.
    ///
    /// # Examples
    ///
    /// ```
    /// use pdfcer_core::text_state::{AmbientValue, TextStateParam};
    ///
    /// // Never set: restore the Table 105 default.
    /// let unset = AmbientValue::initial(TextStateParam::HorizScale);
    /// assert_eq!(unset.restore_bytes(TextStateParam::HorizScale).unwrap(), b"100 Tz");
    ///
    /// // Set in the stream: restore the bytes AS WRITTEN, trailing zeros and all.
    /// let seen = AmbientValue::observed(0.5, b"0.5000 Tc");
    /// assert_eq!(seen.restore_bytes(TextStateParam::CharSpacing).unwrap(), b"0.5000 Tc");
    ///
    /// // Set by `TD` as a side effect: re-spelled, because re-emitting the
    /// // `TD` itself would also move the line (§9.4.2 Table 108).
    /// let via_td = AmbientValue::observed_indirect(-14.0, "TD");
    /// assert_eq!(via_td.restore_bytes(TextStateParam::Leading).unwrap(), b"-14 TL");
    /// assert!(!via_td.is_byte_faithful());
    /// ```
    pub fn restore_bytes(&self, param: TextStateParam) -> Result<Vec<u8>, AmbientRestoreError> {
        match &self.origin {
            AmbientOrigin::Initial => Ok(param.initial_restore_bytes().to_vec()),
            AmbientOrigin::Observed { raw } => Ok(raw.to_vec()),
            AmbientOrigin::ObservedIndirect { .. } => {
                // Re-spelled through the writer's canonical number emitter
                // (§7.3.3 A1–A3), so the operand looks like every other
                // number pdfcer writes rather than like a `Display` of an
                // `f64` — `-14 TL`, never `-14.0 TL` or `-1.4e1 TL`.
                let mut out = Vec::new();
                crate::writer::content::emit_number(&mut out, self.value);
                out.push(b' ');
                out.extend_from_slice(param.operator());
                Ok(out)
            }
            AmbientOrigin::Unobservable(reason) => Err(AmbientRestoreError::Unobservable {
                param,
                reason: *reason,
            }),
        }
    }
}

/// The full ambient §9.3 text state at a point in a content stream, with
/// each parameter's provenance — the substrate an authoring path needs to
/// emit a state change for one run and put the stream back afterwards.
///
/// Published on
/// [`GlyphProvenance`](crate::text_extract::GlyphProvenance) when
/// provenance capture is enabled, so the ambient state a later edit must
/// restore is *sourced from the file* rather than re-derived (or, as
/// before Pass 19.0, silently assumed to be the default).
///
/// # Examples
///
/// ```
/// use pdfcer_core::text_state::{AmbientTextState, TextStateParam};
///
/// let mut ts = AmbientTextState::initial();
/// assert_eq!(ts.params().h_scale, 1.0);
///
/// // `90 Tz` seen in the stream.
/// ts.apply_operator(b"Tz", &[90.0], b"90 Tz");
/// assert_eq!(ts.params().h_scale, 0.9);
/// assert_eq!(
///     ts.restore_bytes(TextStateParam::HorizScale).unwrap(),
///     b"90 Tz"
/// );
///
/// // Descending into a form XObject makes the inherited value unrestorable.
/// ts.enter_form(Some(7));
/// assert!(ts.restore_bytes(TextStateParam::HorizScale).is_err());
/// // …but the VALUE is still known, because the form inherits it (§8.10.1).
/// assert_eq!(ts.params().h_scale, 0.9);
/// ```
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct AmbientTextState {
    /// `Tc` — character spacing (§9.3.2).
    pub char_spacing: AmbientValue,
    /// `Tw` — word spacing (§9.3.3). Spec-void for multi-byte codes, so a
    /// caller emitting it must first check the run's composite flag (R91).
    pub word_spacing: AmbientValue,
    /// `Tz` — horizontal scaling, stored as the **percentage operand**
    /// (§9.3.4). Divide by 100 for `Th`, or call [`Self::params`].
    pub h_scale: AmbientValue,
    /// `TL` — leading (§9.3.5).
    pub leading: AmbientValue,
    /// `Ts` — text rise (§9.3.7).
    pub rise: AmbientValue,
    /// `Tr` — text rendering mode (§9.3.6, Table 106).
    pub render_mode: AmbientValue,
}

impl AmbientTextState {
    /// The Table 105 initial state — every parameter at its default, none
    /// of them observed. This is the state at the start of every page.
    #[must_use]
    pub const fn initial() -> Self {
        Self {
            char_spacing: AmbientValue::initial(TextStateParam::CharSpacing),
            word_spacing: AmbientValue::initial(TextStateParam::WordSpacing),
            h_scale: AmbientValue::initial(TextStateParam::HorizScale),
            leading: AmbientValue::initial(TextStateParam::Leading),
            rise: AmbientValue::initial(TextStateParam::Rise),
            render_mode: AmbientValue::initial(TextStateParam::RenderMode),
        }
    }

    /// Borrow one parameter's ambient value by name.
    #[must_use]
    pub const fn get(&self, param: TextStateParam) -> &AmbientValue {
        match param {
            TextStateParam::CharSpacing => &self.char_spacing,
            TextStateParam::WordSpacing => &self.word_spacing,
            TextStateParam::HorizScale => &self.h_scale,
            TextStateParam::Leading => &self.leading,
            TextStateParam::Rise => &self.rise,
            TextStateParam::RenderMode => &self.render_mode,
        }
    }

    /// Mutably borrow one parameter's ambient value by name.
    #[must_use]
    pub const fn get_mut(&mut self, param: TextStateParam) -> &mut AmbientValue {
        match param {
            TextStateParam::CharSpacing => &mut self.char_spacing,
            TextStateParam::WordSpacing => &mut self.word_spacing,
            TextStateParam::HorizScale => &mut self.h_scale,
            TextStateParam::Leading => &mut self.leading,
            TextStateParam::Rise => &mut self.rise,
            TextStateParam::RenderMode => &mut self.render_mode,
        }
    }

    /// Record that `param` was set to `value` by an operator whose bytes
    /// are `raw`.
    ///
    /// `raw` should span the **whole operator including its operands**
    /// (`0.5000 Tc`, not just `Tc`), because that is what a tier-2 restore
    /// re-emits. Passing a shorter slice is not unsafe, it just produces a
    /// restore that does not compile back to the same operator — so the
    /// walks that call this pass their tokenizer's operator span.
    pub fn set(&mut self, param: TextStateParam, value: f64, raw: &[u8]) {
        *self.get_mut(param) = AmbientValue::observed(value, raw);
    }

    /// Record that `param` was set to `value` as a **side effect** of
    /// `setter` — an operator whose own bytes are not a usable restore.
    ///
    /// Only two operators qualify (`TD` and `"`); see
    /// [`AmbientOrigin::ObservedIndirect`] for why re-emitting either of
    /// them as a restore would be a bug rather than a fidelity nicety.
    pub fn set_indirect(&mut self, param: TextStateParam, value: f64, setter: &'static str) {
        *self.get_mut(param) = AmbientValue::observed_indirect(value, setter);
    }

    /// Apply one content-stream operator, returning whether it was one of
    /// the six this type models.
    ///
    /// **This is the single update rule** the three walks share — the
    /// whole point of the consolidation. `operands` is the operator's
    /// numeric operands in stream order; `raw` is the operator's bytes as
    /// written.
    ///
    /// `"` (`aw ac string "`) is handled explicitly: Table 109 makes it set
    /// **both** `Tw` and `Tc` before showing, which no single
    /// [`TextStateParam`] can describe. Its raw bytes are **not** kept as
    /// the restore, because they include the show string — re-emitting
    /// `2 0.25 (hi) "` to put `Tw` back would paint "hi" onto the page a
    /// second time. Both parameters are therefore recorded as
    /// [`AmbientOrigin::ObservedIndirect`], which restores by re-spelling
    /// the value as `2 Tw` / `0.25 Tc`. `raw` is accepted and ignored for
    /// this operator, so callers need no special case.
    ///
    /// Returns `false` for every other operator, so a caller can chain it
    /// ahead of its own dispatch without listing the six names again.
    pub fn apply_operator(&mut self, name: &[u8], operands: &[f64], raw: &[u8]) -> bool {
        if let Some(param) = TextStateParam::from_operator(name) {
            // Table 105's operators all take exactly one operand; a
            // malformed operator with none leaves the state alone rather
            // than inventing a value (§7.8.2 recovery posture).
            if let Some(&v) = operands.first() {
                self.set(param, v, raw);
            }
            return true;
        }
        if name == b"\"" {
            if let [aw, ac, ..] = operands {
                self.set_indirect(TextStateParam::WordSpacing, *aw, "\"");
                self.set_indirect(TextStateParam::CharSpacing, *ac, "\"");
            }
            return true;
        }
        false
    }

    /// Mark every **observed** value as inherited-from-outside, because
    /// the walk is descending into a form XObject (§8.10.1).
    ///
    /// [`AmbientOrigin::Initial`] values are deliberately left alone: a
    /// parameter no operator ever set is at its Table 105 default
    /// everywhere, including inside the form, so restoring the spec
    /// default there is provably correct rather than a guess. Only a value
    /// whose *setting operator lives in another buffer* becomes
    /// unrestorable.
    ///
    /// Values set **inside** the form after this call overwrite the mark
    /// (they are observable in the form's own buffer), which is exactly
    /// right and falls out of [`Self::set`] with no special case.
    ///
    /// [`AmbientOrigin::ObservedIndirect`] is marked too: the value was
    /// genuinely set, just outside this buffer, so the re-spelling that
    /// would normally restore it would be writing a value the form's own
    /// stream never stated.
    pub fn enter_form(&mut self, object: Option<u32>) {
        for param in TextStateParam::ALL {
            let slot = self.get_mut(param);
            if matches!(
                slot.origin,
                AmbientOrigin::Observed { .. } | AmbientOrigin::ObservedIndirect { .. }
            ) {
                slot.origin =
                    AmbientOrigin::Unobservable(UnobservableAmbient::FormXObject { object });
            }
        }
    }

    /// The bytes that restore one parameter's ambient value.
    ///
    /// # Errors
    ///
    /// [`AmbientRestoreError::Unobservable`] — see
    /// [`AmbientValue::restore_bytes`].
    pub fn restore_bytes(&self, param: TextStateParam) -> Result<Vec<u8>, AmbientRestoreError> {
        self.get(param).restore_bytes(param)
    }

    /// The six parameters resolved to numbers, `Tz` converted to the `Th`
    /// ratio (§9.3.4).
    ///
    /// `render_mode` is narrowed to `i64` by truncation: Table 105 says
    /// the operand "shall be an integer", and a non-integral one is
    /// malformed. Truncating rather than rounding matches what the
    /// extraction walk did before the consolidation (it read the operand
    /// through `Object::as_int`).
    #[must_use]
    pub fn params(&self) -> TextStateParams {
        TextStateParams {
            char_spacing: self.char_spacing.value,
            word_spacing: self.word_spacing.value,
            h_scale: self.h_scale.value / 100.0,
            leading: self.leading.value,
            rise: self.rise.value,
            render_mode: self.render_mode.value as i64,
        }
    }
}

impl Default for AmbientTextState {
    /// Table 105's initial state (see [`Self::initial`]).
    fn default() -> Self {
        Self::initial()
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

    #[test]
    fn table_105_initial_values() {
        let ts = AmbientTextState::initial();
        let p = ts.params();
        assert_eq!(p.char_spacing, 0.0);
        assert_eq!(p.word_spacing, 0.0);
        assert_eq!(p.h_scale, 1.0, "Tz initial 100 ⇒ Th 1.0");
        assert_eq!(p.leading, 0.0);
        assert_eq!(p.rise, 0.0);
        assert_eq!(p.render_mode, 0);
    }

    #[test]
    fn unset_parameters_restore_to_the_spec_default() {
        let ts = AmbientTextState::initial();
        assert_eq!(
            ts.restore_bytes(TextStateParam::CharSpacing).unwrap(),
            b"0 Tc"
        );
        assert_eq!(
            ts.restore_bytes(TextStateParam::WordSpacing).unwrap(),
            b"0 Tw"
        );
        assert_eq!(
            ts.restore_bytes(TextStateParam::HorizScale).unwrap(),
            b"100 Tz"
        );
        assert_eq!(ts.restore_bytes(TextStateParam::Leading).unwrap(), b"0 TL");
        assert_eq!(ts.restore_bytes(TextStateParam::Rise).unwrap(), b"0 Ts");
        assert_eq!(
            ts.restore_bytes(TextStateParam::RenderMode).unwrap(),
            b"0 Tr"
        );
    }

    /// The tier-2 promise: an observed value restores as the bytes that
    /// were written, NOT a renormalized rendering of the parsed number.
    #[test]
    fn observed_parameters_restore_the_raw_operand_bytes() {
        let mut ts = AmbientTextState::initial();
        assert!(ts.apply_operator(b"Tc", &[0.5], b"0.5000 Tc"));
        assert!(ts.apply_operator(b"Ts", &[3.0], b"+3.0 Ts"));
        assert_eq!(
            ts.restore_bytes(TextStateParam::CharSpacing).unwrap(),
            b"0.5000 Tc",
            "a trailing-zero spelling must survive a restore verbatim"
        );
        assert_eq!(ts.restore_bytes(TextStateParam::Rise).unwrap(), b"+3.0 Ts");
        assert_eq!(ts.params().char_spacing, 0.5);
        assert_eq!(ts.params().rise, 3.0);
    }

    /// Table 109's `"` sets `Tw` and `Tc` **and shows a string**, so its
    /// bytes are not a restore. This is the trap the `ObservedIndirect`
    /// tier exists to stop: a naive tier-2 restore would re-paint the text.
    #[test]
    fn double_quote_sets_both_spacings_but_is_not_byte_restorable() {
        let mut ts = AmbientTextState::initial();
        assert!(ts.apply_operator(b"\"", &[2.0, 0.25], b"2 0.25 (hi) \""));
        assert_eq!(ts.params().word_spacing, 2.0);
        assert_eq!(ts.params().char_spacing, 0.25);

        assert!(!ts.word_spacing.is_byte_faithful());
        assert!(ts.word_spacing.is_restorable());
        assert_eq!(
            ts.restore_bytes(TextStateParam::WordSpacing).unwrap(),
            b"2 Tw",
            "the restore must be a re-spelling, NEVER the `\"` bytes — those \
             would show the string a second time"
        );
        assert_eq!(
            ts.restore_bytes(TextStateParam::CharSpacing).unwrap(),
            b"0.25 Tc"
        );
    }

    /// `TD` sets `TL` **and moves to the next line** (Table 108) — the
    /// other member of the `ObservedIndirect` class. Re-emitting the `TD`
    /// to restore leading would displace every following glyph.
    #[test]
    fn td_derived_leading_restores_as_tl_not_as_td() {
        let mut ts = AmbientTextState::initial();
        ts.set_indirect(TextStateParam::Leading, 14.0, "TD");
        assert_eq!(ts.params().leading, 14.0);
        assert!(!ts.leading.is_byte_faithful());
        assert_eq!(ts.restore_bytes(TextStateParam::Leading).unwrap(), b"14 TL");
    }

    /// An indirectly-set value is still *set*, so descending into a form
    /// makes it unrestorable for the same reason a directly-set one is.
    #[test]
    fn an_indirect_value_also_becomes_unobservable_inside_a_form() {
        let mut ts = AmbientTextState::initial();
        ts.set_indirect(TextStateParam::Leading, 14.0, "TD");
        ts.enter_form(None);
        assert!(!ts.leading.is_restorable());
        let err = ts.restore_bytes(TextStateParam::Leading).unwrap_err();
        assert!(err.to_string().contains("leading"), "{err}");
    }

    #[test]
    fn non_text_state_operators_are_not_claimed() {
        let mut ts = AmbientTextState::initial();
        assert!(!ts.apply_operator(b"Tf", &[12.0], b"/F1 12 Tf"));
        assert!(!ts.apply_operator(b"Tj", &[], b"(hi) Tj"));
        assert!(!ts.apply_operator(b"q", &[], b"q"));
        assert_eq!(ts, AmbientTextState::initial());
    }

    /// Tier 3. The value stays known (the form inherits it, §8.10.1); only
    /// the ability to RESTORE it is lost.
    #[test]
    fn form_xobject_inheritance_refuses_rather_than_guessing() {
        let mut ts = AmbientTextState::initial();
        ts.apply_operator(b"Tc", &[0.5], b"0.5 Tc");
        ts.enter_form(Some(12));

        let err = ts.restore_bytes(TextStateParam::CharSpacing).unwrap_err();
        assert!(matches!(
            err,
            AmbientRestoreError::Unobservable {
                param: TextStateParam::CharSpacing,
                reason: UnobservableAmbient::FormXObject { object: Some(12) },
            }
        ));
        let msg = err.to_string();
        assert!(msg.contains("character spacing"), "{msg}");
        assert!(msg.contains("form XObject 12"), "{msg}");
        assert_eq!(ts.params().char_spacing, 0.5, "the value is still known");
    }

    /// A parameter no operator ever set is at its Table 105 default
    /// everywhere, so descending into a form does NOT make it unobservable.
    #[test]
    fn form_xobject_leaves_never_set_parameters_restorable() {
        let mut ts = AmbientTextState::initial();
        ts.enter_form(Some(3));
        assert_eq!(ts.restore_bytes(TextStateParam::Rise).unwrap(), b"0 Ts");
        assert!(ts.rise.is_restorable());
    }

    /// A value set INSIDE the form is observable in the form's own buffer,
    /// so it overwrites the inherited mark.
    #[test]
    fn a_value_set_inside_the_form_becomes_restorable_again() {
        let mut ts = AmbientTextState::initial();
        ts.apply_operator(b"Ts", &[2.0], b"2 Ts");
        ts.enter_form(Some(3));
        assert!(!ts.rise.is_restorable());
        ts.apply_operator(b"Ts", &[4.0], b"4 Ts");
        assert!(ts.rise.is_restorable());
        assert_eq!(ts.restore_bytes(TextStateParam::Rise).unwrap(), b"4 Ts");
    }

    #[test]
    fn operator_names_round_trip_through_the_parameter_enum() {
        for param in TextStateParam::ALL {
            assert_eq!(TextStateParam::from_operator(param.operator()), Some(param));
            // The spec-default byte string must actually be that operator.
            let bytes = param.initial_restore_bytes();
            assert!(
                bytes.ends_with(param.operator()),
                "{param} default bytes {:?}",
                String::from_utf8_lossy(bytes)
            );
        }
    }
}
