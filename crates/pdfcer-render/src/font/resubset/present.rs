//! The glyphs a program holds by its structure, for the `/CIDSet` or
//! `/CharSet` a PDF/A-1 subset is given (spec RAG
//! `pdfa/pdfa__ref__font_subset_cidset_charset.md` §2-3): a glyph ID below
//! `numGlyphs`, or a CFF charset entry. Outlines are never inspected.

use std::collections::BTreeSet;

use pdfcer_core::optimize::resubset::{PresentGlyphs, ResubsetRefusal};
use skrifa::raw::ps::cff::CffFontRef;
use skrifa::raw::ps::string::Sid;

use super::{has_glyf, malformed};
use crate::font::program::FontProgram;
use crate::font::sfnt::Directory;

/// What `program`, a TrueType, OpenType or bare CFF program, holds.
pub(super) fn present(program: &[u8]) -> Result<PresentGlyphs, ResubsetRefusal> {
    let parsed = FontProgram::parse(program).map_err(|_| malformed())?;
    if has_glyf(&parsed) {
        return Ok(PresentGlyphs::Count(parsed.num_glyphs()));
    }
    let cff = if program.starts_with(b"OTTO") {
        Directory::parse(program)
            .and_then(|d| d.table(*b"CFF "))
            .ok_or_else(malformed)?
    } else {
        program
    };
    let font = CffFontRef::new_cff(cff, 0, None).map_err(|_| malformed())?;
    // read-fonts reads a missing `charset` as ISOAdobe (TN5176 Table 9), so
    // None is a charset that cannot be read.
    let ids: Vec<(u32, u16)> = font
        .charset()
        .ok_or_else(malformed)?
        .iter()
        .map(|(g, s)| (g.to_u32(), s.to_u16()))
        .collect();
    if font.is_cid() {
        // Glyph 0 is CID 0 in every charset (TN5176 §13).
        let cids = std::iter::once(0)
            .chain(ids.iter().map(|&(_, c)| u32::from(c)))
            .collect();
        return Ok(PresentGlyphs::Cids(cids));
    }
    let mut names = BTreeSet::new();
    for (_, sid) in ids {
        let name = font.string(Sid::new(sid)).ok_or_else(malformed)?;
        if name != b".notdef" {
            names.insert(name.to_vec());
        }
    }
    Ok(PresentGlyphs::Names(names))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    fn fixture(path: &str) -> Vec<u8> {
        std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../fixtures/synthetic")
                .join(path),
        )
        .unwrap()
    }

    fn names(list: &[&str]) -> PresentGlyphs {
        PresentGlyphs::Names(list.iter().map(|n| n.as_bytes().to_vec()).collect())
    }

    #[test]
    fn a_truetype_program_holds_its_glyph_count() {
        let face = fixture("text/augment/face.ttf");
        assert_eq!(present(&face).unwrap(), PresentGlyphs::Count(8));
    }

    #[test]
    fn a_name_keyed_cff_holds_its_charset_names_without_notdef() {
        let want = names(&["A", "B", "C", "E", "acute", "Eacute"]);
        for file in ["resubset/face.cff", "resubset/face-cff.otf"] {
            assert_eq!(present(&fixture(file)).unwrap(), want, "{file}");
        }
    }

    #[test]
    fn a_cid_keyed_cff_holds_its_charset_cids_and_cid_zero() {
        let cids = present(&fixture("resubset/face-cid.cff")).unwrap();
        assert_eq!(cids, PresentGlyphs::Cids([0, 1, 2, 3, 5, 8, 9].into()));
    }

    #[test]
    fn a_cff_with_an_unreadable_charset_is_refused() {
        assert!(present(&fixture("resubset/face-cid-badcharset.cff")).is_err());
    }

    #[test]
    fn an_unreadable_program_is_refused() {
        assert!(present(b"not a font").is_err());
    }
}
