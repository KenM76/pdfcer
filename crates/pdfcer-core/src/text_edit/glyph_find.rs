//! The glyph a typed character draws in an embedded program: through the
//! program's `cmap`, else through a `post` glyph name (ISO 32000-2 §9.6.6.4's
//! last resort). A `post`-name match is an inference — glyph names are not
//! normative — so it is carried out to the disclosure (decision 172 §6).

use pdfcer_fonts::fontdata::{encoding_glyph_name, glyph_name_to_unicode};

use crate::object::Object;
use crate::text_edit::font_extend::Encoding;
use crate::text_edit::program_glyphs::{EmbeddedGlyphs, ProgramGlyph};

/// A glyph found for a character, and the `post` name it was found by when
/// the `cmap` did not reach it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Found {
    pub(crate) glyph: ProgramGlyph,
    pub(crate) post_name: Option<String>,
}

/// The glyph `ch` draws in `program`. `encoded` is the name the font's
/// encoding gives the code `ch` is shown with: a viewer looks that exact
/// name up in `post` when the `cmap` misses, so no other name will do.
/// Without one, the first `post` name the Adobe Glyph List reads as `ch`.
pub(crate) fn find(
    glyphs: &dyn EmbeddedGlyphs,
    program: &[u8],
    ch: char,
    encoded: Option<&str>,
) -> Option<Found> {
    if let Some(glyph) = glyphs.unicode_glyph(program, ch) {
        return Some(Found {
            glyph,
            post_name: None,
        });
    }
    let name = match encoded {
        Some(n) => n.to_owned(),
        None => post_name_for(glyphs, program, ch)?,
    };
    let glyph = glyphs.glyph_named(program, &name, ch)?;
    Some(Found {
        glyph,
        post_name: Some(name),
    })
}

/// The first `post` name in `program` that reads as `ch`.
pub(crate) fn post_name_for(
    glyphs: &dyn EmbeddedGlyphs,
    program: &[u8],
    ch: char,
) -> Option<String> {
    glyphs
        .glyph_names(program)
        .into_iter()
        .find(|n| glyph_name_to_unicode(n) == Some(ch))
}

/// Every character a `post` name in `program` reads as.
pub(crate) fn named_chars(glyphs: &dyn EmbeddedGlyphs, program: &[u8]) -> Vec<char> {
    let mut out: Vec<char> = glyphs
        .glyph_names(program)
        .iter()
        .filter_map(|n| glyph_name_to_unicode(n))
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// The glyph name `enc` gives `code` (§9.6.5.1: `/Differences` over the base).
pub(crate) fn encoded_name(enc: &Encoding, code: u32) -> Option<String> {
    let mut at: Option<i64> = None;
    let mut found = None;
    for o in &enc.differences {
        match o {
            Object::Integer(n) => at = Some(*n),
            Object::Name(n) => {
                if at == Some(i64::from(code)) {
                    found = Some(String::from_utf8_lossy(n.as_bytes()).into_owned());
                }
                at = at.map(|c| c + 1);
            }
            _ => {}
        }
    }
    found.or_else(|| encoding_glyph_name(enc.base, u8::try_from(code).ok()?).map(str::to_owned))
}

/// The disclosure clause for a glyph found by its `post` name.
pub(crate) fn inferred_clause(ch: char, post_name: &str) -> String {
    format!(
        "found by the program's glyph name /{post_name}, an inference: the program's cmap does \
         not say that glyph draws '{ch}'"
    )
}
