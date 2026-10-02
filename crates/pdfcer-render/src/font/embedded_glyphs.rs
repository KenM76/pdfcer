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

    fn glyph_by_id(&self, program: &[u8], gid: u32, ch: char) -> Option<ProgramGlyph> {
        let parsed = FontProgram::parse(program).ok()?;
        (gid != 0).then(|| outlined(&parsed, gid, ch)).flatten()
    }

    fn glyph_named(&self, program: &[u8], name: &str, ch: char) -> Option<ProgramGlyph> {
        let parsed = FontProgram::parse(program).ok()?;
        let gid = parsed.glyph_for_name(name).filter(|&g| g != 0)?;
        outlined(&parsed, gid, ch)
    }

    fn glyph_names(&self, program: &[u8]) -> Vec<String> {
        use skrifa::raw::TableProvider as _;
        let Ok(parsed) = FontProgram::parse(program) else {
            return Vec::new();
        };
        let FontProgram::Sfnt(font) = &parsed else {
            return Vec::new();
        };
        let Ok(post) = font.post() else {
            return Vec::new();
        };
        let count = u16::try_from(parsed.num_glyphs()).unwrap_or(u16::MAX);
        (1..count)
            .filter(|&gid| matches!(parsed.outline(u32::from(gid)), Ok(Some(_))))
            .filter_map(|gid| post.glyph_name(skrifa::raw::types::GlyphId16::new(gid)))
            .map(str::to_owned)
            .collect()
    }
}

fn glyph_in(parsed: &FontProgram<'_>, ch: char) -> Option<ProgramGlyph> {
    let gid = parsed.glyph_for_char(ch).filter(|&g| g != 0)?;
    outlined(parsed, gid, ch)
}

/// Glyph `gid` when it is drawn, or blank by design for whitespace `ch`.
fn outlined(parsed: &FontProgram<'_>, gid: u32, ch: char) -> Option<ProgramGlyph> {
    let FontProgram::Sfnt(font) = parsed else {
        return None;
    };
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
