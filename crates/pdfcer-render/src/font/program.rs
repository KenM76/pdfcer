//! # Embedded font-program parsing → glyph outlines (decision 004 §4.1)
//!
//! The ONE place `skrifa::raw::ps::*` (and skrifa generally) is used
//! (rule R21; confinement is deliberate — 004 §9 trigger 2 notes the
//! `ps` module is a lower-level surface, so a break on a version bump
//! is a one-file fix here). Covers all four PDF font-program cases
//! (§9.9 Table 126):
//!
//! | PDF stream | Format | Route |
//! |---|---|---|
//! | `FontFile2` / `FontFile3 /OpenType` | sfnt | [`skrifa::FontRef`] + outline collection |
//! | `FontFile3 /Type1C`, `/CIDFontType0C` | bare CFF | `raw::ps::cff::CffFontRef` |
//! | `FontFile` | bare Type 1 (PFB/PFA/raw eexec) | `raw::ps::type1::Type1Font` |
//!
//! Outlines are produced **unhinted, in font units** (rule R18): the
//! interpreter applies its own `glyph-space → text space` scale
//! (1000-per-em convention with the actual [`FontProgram::upem`] as
//! divisor — CFF/Type 1 may carry a non-1000 FontMatrix, 004 §3.3)
//! and then `Trm × CTM`. Hinting would grid-fit to a raster that the
//! arbitrary PDF transform makes meaningless.
//!
//! Two format-detection traps (both verified at source, filed in
//! `C:\personal_rag\pdf\`):
//!
//! 1. `Type1Font::new` requires the data to BEGIN with `%!PS-AdobeFont`
//!    or `%!FontType`, so the Type 1 *text* (PFA) path in
//!    [`FontProgram::parse`] tolerates leading ASCII whitespace/comments
//!    before the `%!` sigil.
//! 2. The TrueType sfnt version `0x00010000` BEGINS with a `0x00` (NUL)
//!    byte, and NUL is itself PDF whitespace (ISO 32000-1 Table 1). So
//!    the leading-whitespace tolerance from (1) must **never** be applied
//!    before binary-magic detection: doing so shifts every ordinary
//!    embedded TrueType (`FontFile2`, sfnt version `0x00010000`) by one
//!    byte, leaving `0x01 0x00 …` which then matches the bare-CFF magic
//!    and misroutes into the CFF parser (fails "offset out of bounds").
//!    Binary magics are therefore matched on the RAW bytes; only the
//!    Type 1 text path trims. (Confirmed on real CAD/Office subset
//!    TrueType — SolidWorks/AutoCAD/Office output — where all 7 fonts
//!    carried version `0x00010000`.)

use skrifa::outline::DrawSettings;
use skrifa::outline::pen::OutlinePen;
use skrifa::raw::TableProvider as _;
use skrifa::raw::tables::cmap::{CmapSubtable, PlatformId};
use skrifa::raw::types::{GlyphId16, NameId};
use skrifa::raw::{FontData as RawFontData, ps};
use skrifa::{FontRef, GlyphId, MetadataProvider};
use tiny_skia::PathBuilder;

/// `(platform, encoding)` of the Microsoft Unicode BMP cmap subtable —
/// §9.6.6.4 Branch A's first chain (`code → name → Unicode → glyph`).
const CMAP_MS_UNICODE: (u16, u16) = (3, 1);
/// Microsoft Symbol subtable — §9.6.6.4 Branch B, the `0xF000` page.
const CMAP_MS_SYMBOL: (u16, u16) = (3, 0);
/// Macintosh Roman subtable — Branch A's second chain and Branch B's
/// single-byte fallback.
const CMAP_MAC_ROMAN: (u16, u16) = (1, 0);
/// Microsoft Unicode full-repertoire (UCS-4) subtable — not named by
/// §9.6.6.4 (which predates it) but universally present alongside or
/// instead of `(3, 1)` in modern fonts; consulted only after `(3, 1)`.
const CMAP_MS_UCS4: (u16, u16) = (3, 10);
/// The four high bytes §9.6.6.4 Branch B permits for a `(3, 0)`
/// subtable's code ranges, most-likely-first.
const SYMBOL_PAGES: [u16; 4] = [0xF000, 0x0000, 0xF100, 0xF200];

/// A parsed font program, borrowing the underlying bytes.
///
/// Parsed per render (cheap — skrifa parsing is lazy/zero-copy); a
/// caching layer can come later without changing this surface.
pub enum FontProgram<'a> {
    /// sfnt-framed (TrueType or OpenType, incl. CFF-in-OpenType).
    Sfnt(FontRef<'a>),
    /// Bare CFF (Type1C / CIDFontType0C).
    Cff(ps::cff::CffFontRef<'a>),
    /// Bare PostScript Type 1 (owns its decrypted charstrings —
    /// `Type1Font` decodes eexec into owned buffers, hence no borrow).
    Type1(ps::type1::Type1Font),
}

impl std::fmt::Debug for FontProgram<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Sfnt(_) => "FontProgram::Sfnt",
            Self::Cff(_) => "FontProgram::Cff",
            Self::Type1(_) => "FontProgram::Type1",
        })
    }
}

/// Program-parse failures (fail-clean; the interpreter maps these to
/// substitution + diagnostics, never a silent wrong glyph).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ProgramError {
    /// The bytes match no known font-program framing.
    #[error("unrecognized font program format")]
    UnknownFormat,
    /// skrifa/read-fonts rejected the program.
    #[error("font program parse failed: {0}")]
    Parse(String),
    /// The glyph exists but drawing it failed.
    #[error("glyph {0} draw failed: {1}")]
    Draw(u32, String),
    /// The glyph id is not in the font.
    #[error("glyph {0} not in font")]
    MissingGlyph(u32),
}

impl<'a> FontProgram<'a> {
    /// Detect the framing and parse. `data` is the DECODED FontFile
    /// stream contents.
    ///
    /// # Errors
    ///
    /// [`ProgramError`] on unknown framing or parse failure.
    pub fn parse(data: &'a [u8]) -> Result<Self, ProgramError> {
        // Binary font-program magics are matched on the RAW bytes — see
        // module trap (2): the TrueType sfnt version `0x00010000` begins
        // with a NUL, which is PDF whitespace, so trimming before this
        // match would misroute every ordinary embedded TrueType.
        match data {
            [0x00, 0x01, 0x00, 0x00, ..]
            | [b'O', b'T', b'T', b'O', ..]
            | [b't', b'r', b'u', b'e', ..]
            | [b't', b't', b'c', b'f', ..] => {
                return FontRef::new(data)
                    .map(Self::Sfnt)
                    .map_err(|e| ProgramError::Parse(e.to_string()));
            }
            // Bare CFF (Type1C / CIDFontType0C): header major=1, minor=0.
            [0x01, 0x00, ..] => {
                return ps::cff::CffFontRef::new_cff(data, 0, None)
                    .map(Self::Cff)
                    .map_err(|e| ProgramError::Parse(e.to_string()));
            }
            // PFB-tagged (binary) Type 1: segment tag `0x8001`; carries
            // no leading whitespace, so it is a raw match too.
            [0x80, 0x01, ..] => {
                return ps::type1::Type1Font::new(data)
                    .map(Self::Type1)
                    .map_err(|e| ProgramError::Parse(e.to_string()));
            }
            _ => {}
        }

        // Type 1 in PFA / text form only: `Type1Font::new` requires the
        // data to BEGIN with `%!PS-AdobeFont` / `%!FontType` (module trap
        // 1), so tolerate leading ASCII whitespace/comments before the
        // `%!` sigil. NUL is deliberately NOT trimmed here — it cannot
        // legitimately precede `%!`, and trimming it is exactly what
        // corrupts the binary formats above.
        let start = data
            .iter()
            .position(|b| !matches!(b, b' ' | b'\t' | b'\r' | b'\n' | b'\x0C'))
            .unwrap_or(0);
        let trimmed = data.get(start..).unwrap_or(data);
        if let [b'%', b'!', ..] = trimmed {
            return ps::type1::Type1Font::new(trimmed)
                .map(Self::Type1)
                .map_err(|e| ProgramError::Parse(e.to_string()));
        }

        Err(ProgramError::UnknownFormat)
    }

    /// The face's advertised name(s), for the shell to register a
    /// supplied face under without parsing fonts itself (decision 012 —
    /// R21: this reuses the ONE skrifa parse, no second parser enters
    /// the read path).
    ///
    /// Returns every distinct name pdfcer can extract, most-specific
    /// first, so the shell can key a `FontEnvironment` under all of them
    /// and match however a PDF's `/BaseFont` happens to spell the family:
    ///
    /// - **sfnt** — the `name` table's family (id 1) and full-name
    ///   (id 4) records, plus the PostScript name (id 6). These are the
    ///   strings a producer writes verbatim into `/BaseFont`
    ///   (`Calibri`, `Calibri Bold`, `Calibri-Bold`).
    /// - **bare CFF** — the CFF `name` INDEX (the font's PostScript
    ///   name).
    /// - **bare Type 1** — the `/FontName` from the cleartext header.
    ///
    /// Empty when the program advertises no name at all — the shell then
    /// falls back to the filename stem alone. Deduped, order-preserving,
    /// so the return is deterministic for a given face (R19 discipline,
    /// even though the walk itself is shell-side).
    #[must_use]
    pub fn face_names(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let mut push = |name: String| {
            let name = name.trim().to_owned();
            if !name.is_empty() && !out.contains(&name) {
                out.push(name);
            }
        };
        match self {
            Self::Sfnt(font) => {
                if let Ok(name_table) = font.name() {
                    let data = name_table.string_data();
                    // OpenType `name` IDs, most-canonical first: 6 =
                    // PostScript name (the exact `/BaseFont` spelling a
                    // producer emits and what subset-tag stripping
                    // targets), 4 = full font name, 1 = font family.
                    for wanted in [
                        NameId::POSTSCRIPT_NAME,
                        NameId::FULL_NAME,
                        NameId::FAMILY_NAME,
                    ] {
                        for record in name_table.name_record() {
                            if record.name_id() == wanted
                                && let Ok(s) = record.string(data)
                            {
                                push(s.to_string());
                            }
                        }
                    }
                }
            }
            Self::Cff(cff) => {
                if let Some(meta) = cff.metadata() {
                    for name in [meta.name(), meta.full_name(), meta.family_name()]
                        .into_iter()
                        .flatten()
                    {
                        push(name.to_owned());
                    }
                }
            }
            Self::Type1(t1) => {
                for name in [t1.name(), t1.full_name(), t1.family_name()]
                    .into_iter()
                    .flatten()
                {
                    push(name.to_owned());
                }
            }
        }
        out
    }

    /// Design units per em (the glyph-space divisor; 1000 for
    /// conventional CFF/Type 1, 1000 or 2048 for most sfnts, but
    /// NEVER assumed — 004 §3.3).
    #[must_use]
    pub fn upem(&self) -> f32 {
        match self {
            Self::Sfnt(font) => font
                .head()
                .map(|h| f32::from(h.units_per_em()))
                .unwrap_or(1000.0),
            Self::Cff(cff) => cff.upem() as f32,
            Self::Type1(t1) => t1.upem() as f32,
        }
    }

    /// Whether the bare-CFF program is CID-keyed (drives the
    /// CIDFontType0 CID→GID mapping, §9.7.4.2).
    #[must_use]
    pub fn is_cid_cff(&self) -> bool {
        matches!(self, Self::Cff(cff) if cff.is_cid())
    }

    /// For a CID-keyed CFF: map a CID to a glyph id via the charset
    /// (§9.7.4.2); identity when not CID-keyed or no charset.
    #[must_use]
    pub fn cff_cid_to_gid(&self, cid: u16) -> u32 {
        if let Self::Cff(cff) = self
            && cff.is_cid()
            && let Some(charset) = cff.charset()
            && let Ok(gid) = charset.glyph_id(skrifa::raw::ps::string::Sid::new(cid))
        {
            return gid.to_u32();
        }
        u32::from(cid)
    }

    /// Number of glyphs in the program — the bound every GID coming
    /// from a PDF object (a CID, a `CIDToGIDMap` entry) must be checked
    /// against before it reaches [`Self::outline`] (ARCHITECTURE.md
    /// §10: never trust a length or an index from the file).
    #[must_use]
    pub fn num_glyphs(&self) -> u32 {
        match self {
            Self::Sfnt(font) => font.maxp().map(|m| u32::from(m.num_glyphs())).unwrap_or(0),
            Self::Cff(cff) => cff.num_glyphs(),
            Self::Type1(t1) => t1.num_glyphs(),
        }
    }

    /// Glyph **name** → GID.
    ///
    /// This is the primary lookup for name-keyed programs (§9.6.6.2:
    /// "a Type 1 font program's glyph descriptions are keyed by glyph
    /// names, not by character codes") and the LAST RESORT for sfnt
    /// programs (§9.6.6.4: "the glyph name shall be looked up in the
    /// font program's `post` table (if one is present)").
    ///
    /// Linear in the glyph count: callers resolve all 256 codes of a
    /// simple font ONCE at `Tf` time (see `crate::text`), never
    /// per painted glyph.
    #[must_use]
    pub fn glyph_for_name(&self, name: &str) -> Option<u32> {
        match self {
            Self::Sfnt(font) => {
                let post = font.post().ok()?;
                (0..u16::try_from(post.num_names()).unwrap_or(u16::MAX))
                    .find(|&gid| post.glyph_name(GlyphId16::new(gid)) == Some(name))
                    .map(u32::from)
            }
            Self::Cff(cff) => {
                let charset = cff.charset()?;
                charset
                    .iter()
                    .find(|&(_, sid)| cff.string(sid) == Some(name.as_bytes()))
                    .map(|(gid, _)| gid.to_u32())
            }
            Self::Type1(t1) => t1
                .glyph_names()
                .find(|&(_, n)| n == name)
                .map(|(gid, _)| gid.to_u32()),
        }
    }

    /// Unicode scalar → GID via the sfnt `(3, 1)` (or `(3, 10)`) cmap
    /// subtable — §9.6.6.4 Branch A's first resolution chain, reached
    /// after `code → glyph name → Unicode` (the name→Unicode half is
    /// the Adobe Glyph List, which lives in `pdfcer-core`, decision 004
    /// §5.6: ONE AGL in the binary).
    ///
    /// Returns `None` for non-sfnt programs: bare CFF and Type 1 are
    /// name-keyed, so [`Self::glyph_for_name`] is their chain.
    #[must_use]
    pub fn glyph_for_char(&self, ch: char) -> Option<u32> {
        let Self::Sfnt(font) = self else { return None };
        cmap_subtable(font, CMAP_MS_UNICODE)
            .or_else(|| cmap_subtable(font, CMAP_MS_UCS4))?
            .map_codepoint(ch)
            .map(GlyphId::to_u32)
    }

    /// Mac OS Roman code → GID via the `(1, 0)` cmap subtable —
    /// §9.6.6.4 Branch A's second chain, used when the font has no
    /// `(3, 1)` subtable ("code → glyph name → back to a code via the
    /// standard Mac OS Roman encoding → glyph via the (1,0) subtable").
    #[must_use]
    pub fn glyph_for_mac_code(&self, code: u8) -> Option<u32> {
        let Self::Sfnt(font) = self else { return None };
        if cmap_subtable(font, CMAP_MS_UNICODE).is_some() {
            // The (3,1) chain owns this font; do not second-guess it.
            return None;
        }
        cmap_subtable(font, CMAP_MAC_ROMAN)?
            .map_codepoint(u32::from(code))
            .map(GlyphId::to_u32)
    }

    /// Raw character code → GID through the program's **own built-in
    /// encoding**.
    ///
    /// Three different spec rules converge on this one method, because
    /// they are the same question asked of three font formats:
    ///
    /// - **sfnt** — §9.6.6.4 Branch B (no `/Encoding`, or the
    ///   `Symbolic` flag set): try the `(3, 0)` subtable with each
    ///   permitted high byte (`0xF000`, `0x0000`, `0xF100`, `0xF200` —
    ///   the spec says "depending on the range of codes", and the range
    ///   is discovered by probing, since nothing in the PDF declares
    ///   it); otherwise the `(1, 0)` subtable with the single byte.
    /// - **bare CFF** — the font's own `Encoding` (custom or
    ///   predefined-Standard) resolved through its charset.
    /// - **bare Type 1** — the `Encoding` array inside the font program
    ///   (§9.6.6.2: "not to be confused with the `Encoding` entry in
    ///   the PDF font dictionary").
    #[must_use]
    pub fn glyph_for_builtin_code(&self, code: u8) -> Option<u32> {
        match self {
            Self::Sfnt(font) => {
                if let Some(sub) = cmap_subtable(font, CMAP_MS_SYMBOL) {
                    for page in SYMBOL_PAGES {
                        if let Some(gid) = sub.map_codepoint(u32::from(page | u16::from(code))) {
                            return Some(gid.to_u32());
                        }
                    }
                }
                cmap_subtable(font, CMAP_MAC_ROMAN)?
                    .map_codepoint(u32::from(code))
                    .map(GlyphId::to_u32)
            }
            Self::Cff(cff) => {
                if let Some(gid) = cff.encoding().and_then(|e| e.map(code)) {
                    return Some(gid.to_u32());
                }
                // No `Encoding` operator in the Top DICT ⇒ the CFF
                // default, which is the standard encoding: resolve
                // code → SID → GID through the charset.
                let sid = ps::encoding::PredefinedEncoding::Standard.sid(code)?;
                cff.charset()?.glyph_id(sid).ok().map(GlyphId::to_u32)
            }
            Self::Type1(t1) => t1.encoding()?.map(code).map(GlyphId::to_u32),
        }
    }

    /// The built-in encoding's glyph NAME for a code, when the program
    /// carries names (bare CFF / Type 1).
    ///
    /// Needed by the width ladder rather than the glyph ladder: a
    /// non-embedded standard-14 font with no `/Widths` array takes its
    /// advances from the AFM tables, which are keyed by glyph name, so
    /// the substitute face's own encoding has to supply the name when
    /// the PDF's `/Encoding` did not.
    #[must_use]
    pub fn builtin_glyph_name(&self, code: u8) -> Option<&str> {
        match self {
            Self::Sfnt(_) => None,
            Self::Cff(cff) => {
                let gid = GlyphId::new(self.glyph_for_builtin_code(code)?);
                let sid = cff.charset()?.string_id(gid).ok()?;
                std::str::from_utf8(cff.string(sid)?).ok()
            }
            Self::Type1(t1) => t1.encoding()?.glyph_name(code),
        }
    }

    /// Extract `gid`'s outline as a tiny-skia path in FONT UNITS
    /// (unhinted, R18; y-up as fonts define it — the interpreter's
    /// transform handles orientation). Returns `Ok(None)` for an
    /// empty outline (space-like glyphs — legitimate, not an error).
    ///
    /// # Errors
    ///
    /// [`ProgramError::MissingGlyph`] / [`ProgramError::Draw`].
    pub fn outline(&self, gid: u32) -> Result<Option<tiny_skia::Path>, ProgramError> {
        let gid = GlyphId::new(gid);
        let mut pen = SkiaPen::default();
        match self {
            Self::Sfnt(font) => {
                let outlines = font.outline_glyphs();
                let glyph = outlines
                    .get(gid)
                    .ok_or(ProgramError::MissingGlyph(gid.to_u32()))?;
                glyph
                    .draw(
                        DrawSettings::unhinted(
                            skrifa::instance::Size::unscaled(),
                            skrifa::instance::LocationRef::default(),
                        ),
                        &mut pen,
                    )
                    .map_err(|e| ProgramError::Draw(gid.to_u32(), e.to_string()))?;
            }
            Self::Cff(cff) => {
                let sub_index = cff.subfont_index(gid).unwrap_or(0);
                let subfont = cff
                    .subfont(sub_index, &[])
                    .map_err(|e| ProgramError::Draw(gid.to_u32(), e.to_string()))?;
                cff.draw(&subfont, gid, &[], None, &mut pen)
                    .map_err(|e| ProgramError::Draw(gid.to_u32(), e.to_string()))?;
            }
            Self::Type1(t1) => {
                t1.draw(gid, None, &mut pen)
                    .map_err(|e| ProgramError::Draw(gid.to_u32(), e.to_string()))?;
            }
        }
        Ok(pen.builder.finish())
    }
}

/// Select a cmap subtable by EXACT `(platform, encoding)` id.
///
/// §9.6.6.4 names specific subtables and its Branch A/B chains differ
/// by which one is present, so an auto-chosen "best" charmap (which is
/// what `skrifa`'s own [`skrifa::charmap::Charmap`] gives) is not
/// sufficient — decision 004 §3.3 records this as the reason the
/// low-level `raw::tables::cmap` surface is used here.
fn cmap_subtable<'a>(font: &FontRef<'a>, (plat, enc): (u16, u16)) -> Option<CmapSubtable<'a>> {
    let cmap = font.cmap().ok()?;
    let data: RawFontData<'a> = cmap.offset_data();
    cmap.encoding_records()
        .iter()
        .find(|r| r.platform_id() == PlatformId::new(plat) && r.encoding_id() == enc)
        .and_then(|r| r.subtable(data).ok())
}

/// [`OutlinePen`] → [`PathBuilder`] adapter (the 1:1 mapping of
/// decision 004 §3.3's table).
#[derive(Default)]
struct SkiaPen {
    builder: PathBuilder,
}

impl OutlinePen for SkiaPen {
    fn move_to(&mut self, x: f32, y: f32) {
        self.builder.move_to(x, y);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.builder.line_to(x, y);
    }
    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        self.builder.quad_to(cx0, cy0, x, y);
    }
    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        self.builder.cubic_to(cx0, cy0, cx1, cy1, x, y);
    }
    fn close(&mut self) {
        self.builder.close();
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;
    use crate::font::{FallbackKey, bundled};

    #[test]
    fn every_bundled_face_parses_and_yields_outlines() {
        // R22's verify-don't-assert applied to the shipped assets:
        // each face must parse as bare CFF and produce at least one
        // non-empty outline among the first 40 glyph ids (gid 0 is
        // .notdef, often empty; letters follow).
        let faces = bundled::faces();
        assert_eq!(faces.len(), 14);
        for (key, data) in &faces {
            let program = FontProgram::parse(data.bytes())
                .unwrap_or_else(|e| panic!("{key:?} failed to parse: {e}"));
            assert!(
                matches!(program, FontProgram::Cff(_)),
                "{key:?} is not bare CFF"
            );
            assert!(program.upem() > 0.0, "{key:?} upem");
            let drawn = (0u32..40)
                .filter_map(|gid| program.outline(gid).ok().flatten())
                .count();
            assert!(drawn > 0, "{key:?}: no drawable glyph in first 40 gids");
        }
    }

    #[test]
    fn symbol_and_dingbats_present() {
        let faces = bundled::faces();
        assert!(faces.contains_key(&FallbackKey::Symbol));
        assert!(faces.contains_key(&FallbackKey::Dingbats));
    }

    #[test]
    fn face_names_reads_advertised_names_from_bundled_cff() {
        // decision 012 / R21: the shell registers a supplied face under
        // its advertised name(s) via this ONE parser. The bundled Foxit
        // faces are bare CFF and must expose their PostScript name so the
        // same mechanism works for a real supplied .otf/.ttf.
        let faces = bundled::faces();
        let helv = faces.get(&FallbackKey::Sans).expect("bundled sans present");
        let program = FontProgram::parse(helv.bytes()).expect("bundled sans parses");
        let names = program.face_names();
        assert!(
            !names.is_empty(),
            "a bundled CFF must advertise at least one name"
        );
        // Every returned name is non-empty and trimmed (the shell keys a
        // FontEnvironment on these, so blanks would be a silent bad key).
        assert!(names.iter().all(|n| !n.is_empty() && n.trim() == n));
    }

    #[test]
    fn face_names_empty_for_unparseable_is_never_reached() {
        // face_names is only callable on a successfully parsed program;
        // a garbage buffer fails at parse, so the shell never asks an
        // unparsed face for its names. This documents that contract.
        assert!(FontProgram::parse(b"not a font").is_err());
    }

    #[test]
    fn unknown_format_is_clean_error() {
        assert_eq!(
            FontProgram::parse(b"not a font at all").unwrap_err(),
            ProgramError::UnknownFormat
        );
    }

    #[test]
    fn truetype_sfnt_magic_not_eaten_by_whitespace_trim() {
        // Regression for the NUL-trim misroute: sfnt version 0x00010000
        // begins with a NUL (PDF whitespace). Detection must route it to
        // the sfnt arm, NOT strip the NUL and misread `01 00 …` as bare
        // CFF. We assert the ROUTING (a 12-byte header is enough to reach
        // FontRef::new); a full valid sfnt is exercised by the
        // no-cmap-CIDFontType2 fixture. read-fonts' FontRef::new only
        // needs >= 12 bytes + a valid sfnt version to construct, so this
        // header alone must parse to Sfnt rather than error as CFF.
        let mut header = vec![0x00u8, 0x01, 0x00, 0x00]; // sfnt version
        header.extend_from_slice(&[0x00, 0x00]); // numTables = 0
        header.extend_from_slice(&[0x00, 0x00]); // searchRange
        header.extend_from_slice(&[0x00, 0x00]); // entrySelector
        header.extend_from_slice(&[0x00, 0x00]); // rangeShift
        let program = FontProgram::parse(&header).expect("routes to sfnt, not CFF");
        assert!(matches!(program, FontProgram::Sfnt(_)), "got {program:?}");
    }

    #[test]
    fn leading_whitespace_tolerated_for_type1_detection() {
        // The header trap: %! must be found after whitespace trim.
        // (A real Type1 body would parse further; here we only assert
        // the DETECTION path routes to Type1 and fails in parse, not
        // in UnknownFormat.)
        let e = FontProgram::parse(b"\n%!PS-AdobeFont-1.0 garbage").unwrap_err();
        assert!(matches!(e, ProgramError::Parse(_)), "got {e:?}");
    }
}
