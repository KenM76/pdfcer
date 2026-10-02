//! `pdfcer-core`'s [`EmbeddedGlyphs`] seam, answered with the one skrifa
//! parser (`R21`). Supply it per edit with `EditOptions::with_embedded_glyphs`.

use pdfcer_core::text_edit::{EmbeddedGlyphs, ProgramGlyph};

use super::program::FontProgram;

/// Answers [`EmbeddedGlyphs`] by parsing the embedded program.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EmbeddedProgramGlyphs;

impl EmbeddedGlyphs for EmbeddedProgramGlyphs {
    fn unicode_glyph(&self, program: &[u8], ch: char) -> Option<ProgramGlyph> {
        glyph_in(&FontProgram::parse(program).ok()?, ch)
    }

    fn unicode_chars(&self, program: &[u8]) -> Vec<char> {
        use skrifa::MetadataProvider;
        let Ok(parsed) = FontProgram::parse(program) else {
            return Vec::new();
        };
        let FontProgram::Sfnt(font) = &parsed else {
            return Vec::new();
        };
        font.charmap()
            .mappings()
            .filter_map(|(cp, _)| char::from_u32(cp))
            .filter(|&ch| glyph_in(&parsed, ch).is_some())
            .collect()
    }
}

fn glyph_in(parsed: &FontProgram<'_>, ch: char) -> Option<ProgramGlyph> {
    let FontProgram::Sfnt(font) = parsed else {
        return None;
    };
    let gid = parsed.glyph_for_char(ch).filter(|&g| g != 0)?;
    let advance = {
        use skrifa::MetadataProvider;
        font.glyph_metrics(
            skrifa::instance::Size::unscaled(),
            skrifa::instance::LocationRef::default(),
        )
        .advance_width(skrifa::GlyphId::new(gid))?
    };
    // A subset keeps an empty slot for every glyph it dropped, so an empty
    // outline means "not here" — except for a blank glyph, which is empty by
    // design and is told apart by its advance.
    let drawn = matches!(parsed.outline(gid), Ok(Some(_)));
    if !(drawn || (ch.is_whitespace() && advance > 0.0)) {
        return None;
    }
    let upem = f64::from(parsed.upem());
    Some(ProgramGlyph::new(gid, f64::from(advance) * 1000.0 / upem))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A program core cannot parse answers `None`, not an error.
    #[test]
    fn garbage_is_no_glyph() {
        assert_eq!(
            EmbeddedProgramGlyphs.unicode_glyph(b"not a font", 'A'),
            None
        );
    }
}
