//! A coarse, stable discriminant a front end can switch on to explain a
//! refused edit — WITHOUT matching pdfcer's internal error variants or parsing
//! its `Display` prose (`pdfcer-gui` request 2026-09-04).
//!
//! # Why this exists
//!
//! [`EditError`] and [`AddTextError`] are rich, precise, and *unstable by
//! design*: they gain and split variants as the editor learns to refuse new
//! things for new reasons, and their `Display` text is deliberately reworded to
//! read well. A front end that wants to say one sentence per *category* of
//! refusal — "this font can't be edited", "the document is locked", "that isn't
//! there", "refused, see diagnostics" — has two bad options without this
//! module:
//!
//! 1. **Match the internal variants.** That copies pdfcer's taxonomy into the
//!    consumer's crate, and the two drift: the first time a variant is split,
//!    the consumer's `match` either stops compiling (best case) or falls into a
//!    wrong arm (likely case, and it tells the operator the *wrong* reason —
//!    worse than saying nothing).
//! 2. **Grep the `Display` string.** That is prose, owned here, reworded at
//!    will; a front end that greps a diagnostic for `"font"` breaks on a comma.
//!
//! [`RefusalKind`] is the third option: a **small, coarse, stable** set of
//! buckets that maps every present and future error variant onto one of four
//! outcomes. The mapping lives HERE, next to the errors, so it moves with them;
//! the consumer writes four sentences and never re-derives pdfcer's reasoning.
//!
//! # Stability contract — and why this enum is NOT `#[non_exhaustive]`
//!
//! The whole value of this type is that a front end may match it **exhaustively**
//! and have the compiler prove the four sentences are complete. `#[non_exhaustive]`
//! would defeat that — it would force a `_` arm and reintroduce exactly the
//! silent-wrong-arm risk this type removes. So the four variants are a
//! **committed contract**: adding a fifth is a deliberate breaking change
//! (a major version bump and a note on the channel), not something that happens
//! because a new `R-INV-*` code appeared. New error variants are absorbed into
//! [`RefusalKind::Other`] by default; only a genuinely new *operator-facing
//! category* would justify growing this enum.

use super::addtext::AddTextError;
use super::edit::EditError;

/// The category of a refused edit, coarse enough to drive one operator-facing
/// sentence per bucket and stable enough to match exhaustively.
///
/// Obtain one with [`RefusalClass::refusal_kind`], implemented for both
/// [`EditError`] and [`AddTextError`].
///
/// The suggested operator-facing wording (the front end owns the exact text):
///
/// - [`UnsupportedFont`](Self::UnsupportedFont) — *"This text is in a font
///   pdfcer can't edit safely, so it was left alone."*
/// - [`StructureFrozen`](Self::StructureFrozen) — *"This document is protected
///   (a signature, encryption, or enforced permissions), so the edit was
///   refused."*
/// - [`NotFound`](Self::NotFound) — *"pdfcer couldn't find what the edit named."*
/// - [`Other`](Self::Other) — *"That edit was refused; see the diagnostics for
///   the reason."*
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RefusalKind {
    /// The font cannot be edited — its encoding, cmap, or an unparsed embedded
    /// program makes the code↔glyph relation unrecoverable, or a substitute/
    /// embedded font could not cover the text. The operator did nothing wrong;
    /// the *content* is uneditable.
    UnsupportedFont,
    /// The document's structure forbids the change — encryption, an enforced
    /// certification signature (`/Perms /DocMDP`), or a `/Size`-suppressed
    /// object set that a new object would expose. The edit is refused rather
    /// than performed-and-breaking.
    StructureFrozen,
    /// The request named something that is not there — a page index out of
    /// range, text that matches no editable run, or a pin pointing at a
    /// different buffer.
    NotFound,
    /// Anything else — an invalid parameter, an unbuilt feature combination, a
    /// content-stream parse failure, a save error. The front end says "refused"
    /// and points the operator at the full diagnostic.
    Other,
}

/// Classify a refusal into a coarse, stable [`RefusalKind`].
///
/// Implemented for the text-editing error types a front end's edit funnel
/// accepts by `Display` bound today — [`EditError`] and [`AddTextError`]. A
/// front end may take `&impl RefusalClass` (or `&dyn RefusalClass`) and handle
/// any of them uniformly, which is why this is a trait rather than two inherent
/// methods.
pub trait RefusalClass {
    /// The coarse category of this refusal.
    fn refusal_kind(&self) -> RefusalKind;
}

impl RefusalClass for EditError {
    fn refusal_kind(&self) -> RefusalKind {
        match self {
            // Every `Refused(Refusal)` is an encoding/cmap/embedded-program
            // invariant (the `R-INV-*` family, `encoding::Refusal`): the font's
            // code↔glyph relation is unrecoverable.
            EditError::Refused(_) => RefusalKind::UnsupportedFont,
            // The document is encrypted — a structural lock.
            EditError::Encrypted => RefusalKind::StructureFrozen,
            // The edit named something absent.
            EditError::PageIndex(_)
            | EditError::NoMatch(_)
            | EditError::PinnedSpanNotFound { .. } => RefusalKind::NotFound,
            // Capability gaps, parse failures, save failures.
            EditError::Unsupported(_)
            | EditError::Content(_)
            | EditError::PageTree(_)
            | EditError::Write(_) => RefusalKind::Other,
        }
    }
}

impl RefusalClass for AddTextError {
    fn refusal_kind(&self) -> RefusalKind {
        match self {
            // Encoding refusal, or a font that could not be embedded / could not
            // cover the text: the font is the obstacle.
            AddTextError::Refused(_)
            | AddTextError::Embed(_)
            | AddTextError::EmbeddedPlanIncomplete { .. } => RefusalKind::UnsupportedFont,
            // Structural locks: encryption, an enforced certification, or a
            // `/Size`-suppressed object set a new object would expose.
            AddTextError::Encrypted
            | AddTextError::CertificationForbidsChange { .. }
            | AddTextError::HiddenObjects { .. } => RefusalKind::StructureFrozen,
            // The edit named an absent page.
            AddTextError::PageIndex(_) => RefusalKind::NotFound,
            // Invalid parameters, an unbuilt combination, exhaustion, parse /
            // save failures.
            AddTextError::EmbeddedBoxedUnsupported
            | AddTextError::EmptyText
            | AddTextError::InvalidSize(_)
            | AddTextError::InvalidBox(_, _)
            | AddTextError::NoWordsToWrap
            | AddTextError::ObjectNumbersExhausted
            | AddTextError::Unsupported(_)
            | AddTextError::PageTree(_)
            | AddTextError::Write(_) => RefusalKind::Other,
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    /// The font refusal the request was filed about — an `R-INV-*` refusal —
    /// classifies as `UnsupportedFont`, so the front end can say "this font
    /// can't be edited" without reading `R-INV-2 … §9.6.6.4 … R21`.
    #[test]
    fn a_font_refusal_is_unsupported_font() {
        let r = crate::text_edit::encoding::Refusal {
            trigger: crate::text_edit::encoding::RInvTrigger::SymbolicNoEncoding,
            character: None,
            base_font: "AAAAAA+JetBrainsMono-Regular".to_owned(),
            message: "symbolic with a built-in cmap and no usable /Encoding".to_owned(),
        };
        assert_eq!(
            EditError::Refused(r).refusal_kind(),
            RefusalKind::UnsupportedFont
        );
    }

    /// Encryption is a structural lock on both error types.
    #[test]
    fn encryption_is_structure_frozen_on_both() {
        assert_eq!(
            EditError::Encrypted.refusal_kind(),
            RefusalKind::StructureFrozen
        );
        assert_eq!(
            AddTextError::Encrypted.refusal_kind(),
            RefusalKind::StructureFrozen
        );
    }

    /// A missing page / unmatched text is `NotFound`.
    #[test]
    fn absent_things_are_not_found() {
        assert_eq!(
            EditError::PageIndex(99).refusal_kind(),
            RefusalKind::NotFound
        );
        assert_eq!(
            EditError::NoMatch("x".to_owned()).refusal_kind(),
            RefusalKind::NotFound
        );
        assert_eq!(
            AddTextError::PageIndex(99).refusal_kind(),
            RefusalKind::NotFound
        );
    }

    /// A bad parameter falls to `Other` — "refused, see diagnostics".
    #[test]
    fn invalid_input_is_other() {
        assert_eq!(
            AddTextError::InvalidSize(-1.0).refusal_kind(),
            RefusalKind::Other
        );
    }
}
