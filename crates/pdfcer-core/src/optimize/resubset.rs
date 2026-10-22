//! Font re-subsetting in an optimize run: an embedded TrueType program
//! (`/FontFile2`) cut down to the glyphs the document draws.
//!
//! Core finds what each font shows ([`super::glyph_usage`]), decides which
//! programs are safe to cut and writes the result; the cutting itself is a
//! [`FontResubsetter`] a shell installs (`pdfcer-render`'s), so core holds no
//! font-program code. Every glyph ID is kept and unused glyph records are
//! emptied, so content streams, `/Widths`, `/W`, `/Encoding` and
//! `/CIDToGIDMap` stay as they are. The program gets a fresh subset tag
//! (ISO 32000-2 §9.6.4: different subsets in one file have different tags).
//!
//! Subsets of one face held as separate programs (a producer that writes a
//! face once as a simple font and once as a CIDFont) are merged into one
//! program when [`ResubsetFonts::merge`] is on and the cutter can prove they
//! were cut from the same font ([`FontResubsetter::merge`]).
//!
//! A descriptor's `/CIDSet` or `/CharSet` (ISO 32000-1 §9.8.3 Table 124,
//! §9.8.1 Table 122) is left as it is. A glyph is present in a program by its
//! structure (an ID below `numGlyphs`, a charset entry), not its outline, so
//! an emptied glyph is still present and the list stays true; dropping its
//! entries would make it under-claim, which PDF/A-2 and -3 forbid
//! (ISO 19005-2 §6.2.11.4.2).

use std::collections::BTreeSet;

use crate::object::ObjId;
use crate::view::DocumentView;

/// Cuts an embedded font program down on `pdfcer-core`'s behalf.
///
/// Implementations must be pure functions of the request, so a plan and its
/// apply agree.
pub trait FontResubsetter: Send + Sync + std::fmt::Debug {
    /// `request.program` keeping every glyph its users can draw with the
    /// codes they show, glyph 0, and every component of a kept composite
    /// glyph, with every other glyph record empty and every glyph ID kept.
    ///
    /// # Errors
    ///
    /// [`ResubsetRefusal`] naming why the program was left as it is.
    fn resubset(
        &self,
        request: &ResubsetRequest<'_, '_>,
    ) -> Result<ResubsetProgram, ResubsetRefusal>;

    /// One program holding every glyph record of `programs`, which are
    /// subsets of one face, with every glyph ID kept, so each subset's
    /// users draw what they drew before. The default refuses, so a cutter
    /// need not merge.
    ///
    /// # Errors
    ///
    /// [`ResubsetRefusal`] when the programs cannot be shown to be cut from
    /// one font, or a glyph differs between them.
    fn merge(&self, programs: &[&[u8]]) -> Result<ResubsetProgram, ResubsetRefusal> {
        let _ = programs;
        Err(ResubsetRefusal::new(
            "merge-unsupported",
            "the font cutter does not merge subsets",
        ))
    }
}

/// One program to cut, with every font that draws from it.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct ResubsetRequest<'a, 'v> {
    /// The document, for reading each user's font dictionary.
    pub view: &'a DocumentView<'v>,
    /// The program, decoded.
    pub program: &'a [u8],
    /// The fonts drawing from it.
    pub users: &'a [FontUse<'a>],
}

impl<'a, 'v> ResubsetRequest<'a, 'v> {
    /// A request to cut `program` for `users`.
    #[must_use]
    pub fn new(view: &'a DocumentView<'v>, program: &'a [u8], users: &'a [FontUse<'a>]) -> Self {
        Self {
            view,
            program,
            users,
        }
    }
}

/// One font drawing from a program, and the codes the document shows with
/// it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct FontUse<'a> {
    /// The font dictionary a `Tf` selects: the simple font, or the Type 0
    /// font over the CIDFont holding the program.
    pub font: ObjId,
    /// Its shown codes: one byte each for a simple font, two for a Type 0
    /// font under `Identity-H` or `-V`.
    pub codes: &'a BTreeSet<u32>,
}

impl<'a> FontUse<'a> {
    /// `font`, showing `codes`.
    #[must_use]
    pub fn new(font: ObjId, codes: &'a BTreeSet<u32>) -> Self {
        Self { font, codes }
    }
}

/// A cut program.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ResubsetProgram {
    /// The complete new program.
    pub program: Vec<u8>,
    /// Glyphs with an outline left.
    pub glyphs_kept: u32,
    /// Glyphs in the program (`maxp.numGlyphs`, unchanged).
    pub glyphs_total: u32,
}

impl ResubsetProgram {
    /// `program`, keeping `glyphs_kept` of `glyphs_total` outlines.
    #[must_use]
    pub fn new(program: Vec<u8>, glyphs_kept: u32, glyphs_total: u32) -> Self {
        Self {
            program,
            glyphs_kept,
            glyphs_total,
        }
    }
}

/// Why a program was left as it is.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{reason}")]
#[non_exhaustive]
pub struct ResubsetRefusal {
    /// A stable kebab-case token for scripts, such as `fstype-restricted`.
    pub token: &'static str,
    /// A sentence for the disclosure.
    pub reason: String,
}

impl ResubsetRefusal {
    /// A refusal under `token`, explained by `reason`.
    #[must_use]
    pub fn new(token: &'static str, reason: impl Into<String>) -> Self {
        Self {
            token,
            reason: reason.into(),
        }
    }
}

/// [`super::OptimizeSettings::resubset`]: re-subset every eligible font
/// with `source`.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct ResubsetFonts {
    /// The cutter.
    pub source: &'static dyn FontResubsetter,
    /// Merge subsets of one face into one program. On by default.
    pub merge: bool,
}

impl ResubsetFonts {
    /// Re-subset with `source`, merging subsets of one face.
    #[must_use]
    pub fn new(source: &'static dyn FontResubsetter) -> Self {
        Self {
            source,
            merge: true,
        }
    }

    /// These settings with [`Self::merge`] off: each program is cut on its
    /// own.
    #[must_use]
    pub fn without_merge(self) -> Self {
        Self {
            merge: false,
            ..self
        }
    }
}

/// Two settings are equal when they name the same cutter and agree on
/// merging.
impl PartialEq for ResubsetFonts {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::addr_eq(self.source, other.source) && self.merge == other.merge
    }
}

/// What a re-subset run does: [`super::OptimizePlan::resubset`].
#[derive(Debug, Clone, Default, PartialEq)]
#[non_exhaustive]
pub struct ResubsetPlan {
    /// The programs it cuts, in id order.
    pub targets: Vec<ResubsetTarget>,
    /// The fonts it leaves, each with why.
    pub refused: Vec<ResubsetRefused>,
    /// Subsets of one face it found but did not merge, each with why. Each
    /// program is still cut on its own.
    pub unmerged: Vec<ResubsetUnmerged>,
}

impl ResubsetPlan {
    /// Stored bytes the cut programs save.
    #[must_use]
    pub fn saving(&self) -> u64 {
        self.targets
            .iter()
            .map(|t| t.before_bytes.saturating_sub(t.after_bytes))
            .sum()
    }
}

/// One program the run cuts.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct ResubsetTarget {
    /// The program stream.
    pub program: ObjId,
    /// The other programs of the same face this one replaces, deleted by
    /// the run: their fonts draw from [`Self::program`] after it. Empty
    /// unless subsets were merged.
    pub merged: Vec<ObjId>,
    /// The fonts drawing from it, in id order.
    pub fonts: Vec<ObjId>,
    /// The font name before and after re-tagging, such as `Arial` to
    /// `KQZBXA+Arial`.
    pub name_before: String,
    /// See [`Self::name_before`].
    pub name_after: String,
    /// Glyphs with an outline left.
    pub glyphs_kept: u32,
    /// Glyphs in the program.
    pub glyphs_total: u32,
    /// The stored size now of the program and of every program in
    /// [`Self::merged`].
    pub before_bytes: u64,
    /// Its stored size after the cut, Flate-compressed.
    pub after_bytes: u64,
    /// The new program, Flate-compressed.
    pub(crate) data: Vec<u8>,
    /// Its decoded length, for `/Length1`.
    pub(crate) length1: usize,
    /// The program, fonts, descendant CIDFonts and descriptors as planned:
    /// what the apply rewrites, and checks has not changed.
    pub(crate) before: Vec<(ObjId, crate::object::Object)>,
}

/// A font the run leaves as it is.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ResubsetRefused {
    /// The font dictionary, when it is an indirect object.
    pub font: Option<ObjId>,
    /// Its `/BaseFont`.
    pub name: String,
    /// The refusal.
    pub why: ResubsetRefusal,
}

/// Subsets of one face the run found and left as separate programs.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ResubsetUnmerged {
    /// The face name, without a subset tag, such as `Calibri`.
    pub face: String,
    /// The programs, in id order.
    pub programs: Vec<ObjId>,
    /// Why they were not merged.
    pub why: ResubsetRefusal,
}
