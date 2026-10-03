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
            .filter_map(|gid| crate::font::program::post_glyph_name(&post, gid))
            .map(str::to_owned)
            .collect()
    }

    fn program_without_cmap(&self, program: &[u8]) -> Option<Vec<u8>> {
        let dir = super::sfnt::Directory::parse(program)?;
        if program.starts_with(b"ttcf") || dir.table(*b"glyf").is_none() {
            return None;
        }
        // Decision 187 §4: a restricted-licence program (usage value 2) is
        // never copied a second time.
        let fs_type = dir
            .table(*b"OS/2")
            .and_then(|os2| super::sfnt::read_u16(os2, 8));
        if fs_type.is_some_and(|f| f & 0x000F == 0x0002) {
            return None;
        }
        let tables = dir
            .tables
            .iter()
            .filter(|(tag, _)| tag != b"cmap")
            .map(|(tag, data)| (*tag, data.to_vec()))
            .collect();
        Some(super::sfnt::assemble(dir.flavor, tables))
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
#[allow(clippy::unwrap_used)] // tests: a panic is a failure
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

    fn fixture(name: &str) -> Vec<u8> {
        let dir = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/synthetic/text/"
        );
        std::fs::read(format!("{dir}{name}")).unwrap()
    }

    #[test]
    fn stripping_drops_only_cmap_and_keeps_the_checksums_valid() {
        let program = fixture("subset-donor.ttf");
        let stripped = EmbeddedProgramGlyphs
            .program_without_cmap(&program)
            .unwrap();
        let (before, after) = (
            super::super::sfnt::Directory::parse(&program).unwrap(),
            super::super::sfnt::Directory::parse(&stripped).unwrap(),
        );
        assert!(before.table(*b"cmap").is_some() && after.table(*b"cmap").is_none());
        assert_eq!(after.tables.len() + 1, before.tables.len());
        for (tag, data) in &after.tables {
            if tag != b"head" {
                assert_eq!(Some(*data), before.table(*tag), "{tag:?}");
            }
        }
        // OpenType `head.checkSumAdjustment`: the whole file sums to 0xB1B0AFBA.
        assert_eq!(super::super::sfnt::checksum(&stripped), 0xB1B0_AFBA);
        assert_eq!(
            EmbeddedProgramGlyphs.unicode_glyph(&stripped, 'A'),
            None,
            "nothing maps through a cmap any more"
        );
    }

    #[test]
    fn a_restricted_program_is_not_copied() {
        let program = fixture("subset-fstype-restricted.ttf");
        assert!(super::super::sfnt::Directory::parse(&program).is_some());
        assert_eq!(EmbeddedProgramGlyphs.program_without_cmap(&program), None);
    }
}
