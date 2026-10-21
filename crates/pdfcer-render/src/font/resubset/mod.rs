//! The [`FontResubsetter`] a shell installs for `pdfcer-core`'s optimize
//! run: an embedded TrueType program with every glyph record the document
//! cannot draw emptied, and every glyph ID, table and mapping kept
//! (`docs/font-resubset-plan.md`).

mod keep;
mod zero;

use pdfcer_core::optimize::resubset::{
    FontResubsetter, ResubsetProgram, ResubsetRefusal, ResubsetRequest,
};

use crate::font::program::FontProgram;
use crate::font::subset::{SubsetError, check_embedding_permission};

/// Empties the glyph records of a `/FontFile2` program that no shown code
/// can reach.
///
/// The kept set is deliberately wide: for each code, every glyph any rung
/// of ISO 32000-2 §9.6.6.4's resolution order could pick, so a reader that
/// resolves codes differently from pdfcer still finds its glyph. Glyph 0
/// and the components of kept composites are always kept.
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
        if request.program.starts_with(b"ttcf") {
            return Err(ResubsetRefusal::new(
                "collection",
                "the program is a font collection",
            ));
        }
        let font = skrifa::FontRef::new(request.program).map_err(|_| malformed())?;
        check_embedding_permission(&font).map_err(fs_type)?;
        let parsed = FontProgram::parse(request.program).map_err(|_| malformed())?;
        let roots = keep::keep_set(request, &parsed)?;
        let cut = zero::zero(request.program, &roots)?;
        Ok(ResubsetProgram::new(cut.program, cut.kept, cut.total))
    }
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
