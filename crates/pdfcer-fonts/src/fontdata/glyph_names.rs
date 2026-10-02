//! Unicode → glyph name, the inverse of [`super::glyph_name_to_unicode`].

use super::{BaseEncoding, encoding_glyph_name, glyph_name_to_unicode};

/// A glyph name that [`glyph_name_to_unicode`] reads back as `ch`, for a
/// `/Differences` entry (ISO 32000-2 §9.6.5.1) that must make `ch` reachable
/// through a nonsymbolic TrueType font's name → Unicode → `cmap` chain
/// (§9.6.6.4).
///
/// The most widely understood name wins, deterministically:
///
/// 1. a name `WinAnsiEncoding`, `MacRomanEncoding` or `StandardEncoding`
///    assigns (Annex D.2), so a §9.10.2 method-2 reader can extract it;
/// 2. else `uniXXXX` (BMP) or `uXXXXX` (AGL Specification §2).
///
/// Other list names are skipped: `Symbol` and `ZapfDingbats` names are
/// font-specific, and a few AGL names (`Delta`, `Omega`) map to two
/// characters, so a reader could resolve them to the wrong one.
///
/// `None` only for a character no name reads back as.
///
/// # Examples
///
/// ```
/// use pdfcer_fonts::fontdata::unicode_to_glyph_name;
///
/// assert_eq!(unicode_to_glyph_name('A').as_deref(), Some("A"));
/// assert_eq!(unicode_to_glyph_name('\u{2013}').as_deref(), Some("endash"));
/// assert_eq!(unicode_to_glyph_name('\u{0394}').as_deref(), Some("uni0394"));
/// assert_eq!(unicode_to_glyph_name('\u{4E2D}').as_deref(), Some("uni4E2D"));
/// assert_eq!(unicode_to_glyph_name('\u{1040C}').as_deref(), Some("u1040C"));
/// ```
#[must_use]
pub fn unicode_to_glyph_name(ch: char) -> Option<String> {
    let reads_back = |name: &str| glyph_name_to_unicode(name) == Some(ch);
    let encoded = [
        BaseEncoding::WinAnsi,
        BaseEncoding::MacRoman,
        BaseEncoding::Standard,
    ]
    .into_iter()
    .flat_map(|enc| (0..=255u8).filter_map(move |c| encoding_glyph_name(enc, c)))
    .find(|name| reads_back(name));
    if let Some(name) = encoded {
        return Some(name.to_owned());
    }
    let formed = if u32::from(ch) <= 0xFFFF {
        format!("uni{:04X}", u32::from(ch))
    } else {
        format!("u{:X}", u32::from(ch))
    };
    reads_back(&formed).then_some(formed)
}
