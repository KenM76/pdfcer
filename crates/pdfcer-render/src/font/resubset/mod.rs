//! The [`FontResubsetter`](pdfcer_core::optimize::resubset::FontResubsetter) a shell installs for `pdfcer-core`'s optimize
//! run: an embedded TrueType or CFF program with every glyph the document
//! cannot draw emptied, and every glyph ID, table and mapping kept, and
//! subsets of one face merged into one program (`docs/font-resubset-plan.md`).

mod cff;
mod cff_glyphs;
mod keep;
mod merge;
mod zero;

use pdfcer_core::optimize::resubset::{
    FontResubsetter, ResubsetProgram, ResubsetRefusal, ResubsetRequest,
};

use crate::font::program::FontProgram;
use crate::font::subset::{SubsetError, check_embedding_permission};

/// Empties the glyphs of a `/FontFile2` or `/FontFile3` program that no
/// shown code can reach: a TrueType record becomes empty, a CFF charstring
/// becomes a bare `endchar`.
///
/// The kept set is deliberately wide: for each code, every glyph any rung
/// of ISO 32000-1 §9.6.6.4's resolution order could pick, so a reader that
/// resolves codes differently from pdfcer still finds its glyph. Glyph 0
/// and the components of kept composites (and `seac` accents) are always
/// kept.
///
/// # Example
///
/// ```
/// use pdfcer_core::optimize::resubset::ResubsetFonts;
/// use pdfcer_render::font::resubset::GLYPH_ZERO_RESUBSETTER;
///
/// let request = ResubsetFonts::new(&GLYPH_ZERO_RESUBSETTER);
/// # let _ = request;
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GlyphZeroResubsetter;

/// The instance for [`pdfcer_core::optimize::resubset::ResubsetFonts::new`].
pub static GLYPH_ZERO_RESUBSETTER: GlyphZeroResubsetter = GlyphZeroResubsetter;

impl FontResubsetter for GlyphZeroResubsetter {
    fn resubset(
        &self,
        request: &ResubsetRequest<'_, '_>,
    ) -> Result<ResubsetProgram, ResubsetRefusal> {
        permitted(request.program)?;
        let parsed = FontProgram::parse(request.program).map_err(|_| malformed())?;
        let roots = keep::keep_set(request, &parsed)?;
        let cut = if has_glyf(&parsed) {
            zero::zero(request.program, &roots)?
        } else {
            cff::zero(request.program, &roots)?
        };
        Ok(ResubsetProgram::new(cut.program, cut.kept, cut.total))
    }

    /// Merges only when every table but the glyph data agrees (`head` up to
    /// its checksum and dates, `name` not compared) and every glyph two
    /// programs both carry is the same record.
    fn merge(&self, programs: &[&[u8]]) -> Result<ResubsetProgram, ResubsetRefusal> {
        for program in programs {
            permitted(program)?;
        }
        merge::merge(programs)
    }
}

/// Whether `font` keeps TrueType outlines; otherwise it is CFF, bare or
/// inside an OpenType wrapper.
fn has_glyf(font: &FontProgram<'_>) -> bool {
    use skrifa::raw::TableProvider as _;
    matches!(font, FontProgram::Sfnt(f) if f.glyf().is_ok())
}

/// `Ok` for a single font whose licence permits subsetting. A bare CFF
/// carries no `OS/2` table, so no licence bits to refuse on.
fn permitted(program: &[u8]) -> Result<(), ResubsetRefusal> {
    if program.starts_with(&[1, 0]) {
        return Ok(());
    }
    if program.starts_with(b"ttcf") {
        return Err(ResubsetRefusal::new(
            "collection",
            "the program is a font collection",
        ));
    }
    let font = skrifa::FontRef::new(program).map_err(|_| malformed())?;
    check_embedding_permission(&font).map_err(fs_type)
}

fn malformed() -> ResubsetRefusal {
    ResubsetRefusal::new(
        "malformed-glyf",
        "the program's glyph data could not be read",
    )
}

fn fs_type(e: SubsetError) -> ResubsetRefusal {
    match e {
        SubsetError::SubsettingNotPermitted => ResubsetRefusal::new(
            "fstype-no-subsetting",
            "the font's licence (OS/2 fsType) forbids subsetting it",
        ),
        _ => ResubsetRefusal::new(
            "fstype-restricted",
            "the font's licence (OS/2 fsType) restricts embedding it",
        ),
    }
}

#[cfg(test)]
mod tests;
