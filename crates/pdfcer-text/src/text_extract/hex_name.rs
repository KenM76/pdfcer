//! Glyph names that spell a character code in hex: `/G41` for code 0x41.
//!
//! The Windows PostScript printer driver names the glyphs of each TrueType
//! font it converts to Type 1 this way (the `MSTT31…` fonts that Distiller
//! output carries), with no `/ToUnicode`. The AGL has no entry for such a
//! name, so §9.10.2 ends at rung 4 and the page copies as U+FFFD. pdfcer
//! reads the hex as a code in the font's base encoding (WinAnsi when the
//! font names none, the driver's own code page), counted as
//! [`super::LadderRung::HexGlyphName`]: an inference, never sourced text.
//! pdf.js recovers these names the same way.

use pdfcer_fonts::fontdata::{self, BaseEncoding};
use pdfcer_model::graph::ObjectGraph;
use pdfcer_model::object::{Dict, Object};
use pdfcer_model::view::DocumentView;

/// The code a `G` + exactly two hex digits name spells (`G41` → 0x41).
///
/// Lowercase `g` names are refused: other producers write `/g3` for a
/// glyph index, which is not a character code.
pub(super) fn code(name: &str) -> Option<u8> {
    let hex = name.strip_prefix('G')?;
    if hex.len() != 2 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    u8::from_str_radix(hex, 16).ok()
}

/// The text a hex glyph name stands for, read through `base`.
pub(super) fn text(name: &str, base: BaseEncoding) -> Option<String> {
    let glyph = fontdata::encoding_glyph_name(base, code(name)?)?;
    fontdata::glyph_name_to_unicode_string(glyph)
}

/// The encoding a hex name's code is read in: the font's named base
/// encoding, else WinAnsi.
pub(super) fn base(doc: &DocumentView<'_>, font_dict: &Dict) -> BaseEncoding {
    let named = match doc.resolve(font_dict.get(b"Encoding").unwrap_or(&Object::Null)) {
        Object::Name(n) => Some(n.as_bytes().to_vec()),
        Object::Dict(d) => match d.get(b"BaseEncoding").map(|o| doc.resolve(o)) {
            Some(Object::Name(n)) => Some(n.as_bytes().to_vec()),
            _ => None,
        },
        _ => None,
    };
    match named.as_deref() {
        Some(b"MacRomanEncoding") => BaseEncoding::MacRoman,
        Some(b"StandardEncoding") => BaseEncoding::Standard,
        _ => BaseEncoding::WinAnsi,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_capital_g_and_two_hex_digits_spell_a_code() {
        assert_eq!(code("G41"), Some(0x41));
        assert_eq!(code("G2c"), Some(0x2C));
        assert_eq!(code("g41"), None);
        assert_eq!(code("G4"), None);
        assert_eq!(code("G041"), None);
        assert_eq!(code("GXY"), None);
    }

    #[test]
    fn the_code_is_read_through_the_base_encoding() {
        assert_eq!(text("G41", BaseEncoding::WinAnsi).as_deref(), Some("A"));
        assert_eq!(text("G20", BaseEncoding::WinAnsi).as_deref(), Some(" "));
        // 0x92 is a curly apostrophe in WinAnsi, not U+0092.
        assert_eq!(
            text("G92", BaseEncoding::WinAnsi).as_deref(),
            Some("\u{2019}")
        );
        assert_eq!(text("G8A", BaseEncoding::MacRoman).as_deref(), Some("ä"));
    }
}
