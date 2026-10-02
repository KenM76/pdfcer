//! The seam through which an edit learns what an embedded font program can
//! draw (decision 172).
//!
//! `pdfcer-core` has no font parser (`R21`), so the shell installs an
//! [`EmbeddedGlyphs`] implementation on the session — `pdfcer-render`'s
//! `EmbeddedProgramGlyphs` — and the embedded-subset floor asks it whether a
//! character the page never shows is nevertheless outlined in the program.

/// One glyph an embedded font program draws for a character.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct ProgramGlyph {
    /// The glyph id in the program.
    pub gid: u32,
    /// The glyph's advance width in PDF glyph space (1000 units per em),
    /// from the program's `hmtx`.
    pub advance: f64,
}

impl ProgramGlyph {
    /// A glyph `gid` advancing `advance` thousandths of an em.
    #[must_use]
    pub fn new(gid: u32, advance: f64) -> Self {
        Self { gid, advance }
    }
}

/// Reads embedded font programs on `pdfcer-core`'s behalf.
///
/// Supplied per edit through
/// [`EditOptions::with_embedded_glyphs`](crate::text_edit::EditOptions::with_embedded_glyphs).
/// Implementations must be pure functions of their arguments, so a preview
/// and the commit of the same edit agree.
pub trait EmbeddedGlyphs: Send + Sync + std::fmt::Debug {
    /// The glyph that ISO 32000-2 §9.6.6.4's nonsymbolic TrueType chain
    /// reaches for `ch` — Unicode through the program's `(3,1)` (or `(3,10)`)
    /// `cmap` subtable — in the decoded sfnt `program`.
    ///
    /// `None` when the program is not sfnt, has no such subtable, maps `ch`
    /// to no glyph or to glyph 0, or the glyph has no outline (a subset keeps
    /// empty slots for glyphs it dropped).
    fn unicode_glyph(&self, program: &[u8], ch: char) -> Option<ProgramGlyph>;

    /// Every character for which [`Self::unicode_glyph`] answers `Some` —
    /// what a repertoire query can offer beyond the font's encoding. The
    /// default offers none, which only narrows that query.
    fn unicode_chars(&self, program: &[u8]) -> Vec<char> {
        let _ = program;
        Vec::new()
    }

    /// Glyph `gid` of `program` — the glyph a composite font's CID selects
    /// through `/CIDToGIDMap` (§9.7.4.2) — when it has an outline, or is
    /// blank by design because `ch`, the character it will be typed as, is
    /// whitespace. The default reads nothing, which only narrows an edit.
    fn glyph_by_id(&self, program: &[u8], gid: u32, ch: char) -> Option<ProgramGlyph> {
        let _ = (program, gid, ch);
        None
    }

    /// The glyph the program's `post` table names `name` — §9.6.6.4's last
    /// resort when a glyph name does not reach a glyph through the `cmap` —
    /// under the same outline rule as [`Self::glyph_by_id`]. The default
    /// reads nothing, which only narrows an edit.
    fn glyph_named(&self, program: &[u8], name: &str, ch: char) -> Option<ProgramGlyph> {
        let _ = (program, name, ch);
        None
    }

    /// Every name the program's `post` table gives a glyph with an outline.
    /// `pdfcer-core` reads each through the Adobe Glyph List to offer the
    /// characters [`Self::unicode_chars`] misses. The default offers none.
    fn glyph_names(&self, program: &[u8]) -> Vec<String> {
        let _ = program;
        Vec::new()
    }
}
