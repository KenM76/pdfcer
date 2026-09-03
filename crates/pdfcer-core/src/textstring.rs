//! # Text strings — §7.9.2 decode and the Annex D.3 PDFDocEncoding table
//!
//! **This module decodes `text string` objects, NOT content-stream show
//! strings.** The distinction is the classic PDF text bug and it is
//! load-bearing here: a show string's bytes are *character codes*
//! interpreted through the font's `/Encoding` or CMap (§9.6.6 / §9.7,
//! and for extraction the §9.10.2 ladder in [`crate::text_extract`]);
//! a text string's bytes are interpreted by exactly one of two rules
//! stated in §7.9.2.2 and by nothing else. Running one through the
//! other's decoder produces mojibake in one direction and silent
//! character loss in the other.
//!
//! Spec sources in the PDF-spec RAG: `iso32000__s__7.9.2.md` (the type
//! system, the BOM rule, the language escape) and
//! `iso32000__annex__d3.md` (the 256-code table, its four source
//! defects, and the two divergences from Latin-1).
//!
//! ## Where text strings appear
//!
//! `/Title` `/Author` `/Subject` `/Keywords` (§14.3.3 `/Info`),
//! outline-item titles, annotation `/Contents`, form-field `/TU` and
//! `/V`, and — the reason this module exists for Pass 4 —
//! **`/ActualText`, `/Alt` and `/E`** (Table 323), which feed the
//! extracted character stream directly. Two entirely separate decode
//! paths (font-encoded show strings and BOM-or-PDFDocEncoding text
//! strings) converge on one output string.
//!
//! ## The discriminator (§7.9.2.2, verbatim rule)
//!
//! > "For text strings encoded in Unicode, the first two bytes shall be
//! > 254 followed by 255."
//!
//! `254 255` decimal is `0xFE 0xFF` — the UTF-16BE byte-order marker
//! U+FEFF. **The prefix is the entire test.** There is no length-parity
//! heuristic, no statistical sniffing, and no `/Encoding` key on a
//! string object. Absent the prefix, the bytes are PDFDocEncoding.
//!
//! §7.9.2.2 NOTE 3 records why the discriminator is safe: it precludes a
//! PDFDocEncoded string beginning with `thorn ydieresis` (`þÿ`), "which
//! is unlikely to be a meaningful beginning of a word or phrase". The
//! collision is real; the standard accepts it.
//!
//! ## PDFDocEncoding is NOT Latin-1 (the two divergences that bite)
//!
//! | Code | PDFDocEncoding | Latin-1 / CP1252 |
//! |---|---|---|
//! | `0xA0` | **U+20AC EURO SIGN** | NO-BREAK SPACE |
//! | `0xAD` | **UNDEFINED** | SOFT HYPHEN (and `hyphen` in `WinAnsiEncoding`) |
//!
//! and two whole ranges are reassigned: `0x18`–`0x1F` are eight
//! typographic modifier letters (breve, caron, …) where ASCII has C0
//! controls, and `0x80`–`0xA0` are punctuation/ligatures/European
//! letters. A Latin-1 decoder silently mistranslates every bullet, dash,
//! quote and the Euro sign.
//!
//! `0xAD` is the one code where the *string* table (Annex D.3) and the
//! *font* table (`WinAnsiEncoding`, Annex D.2) **disagree by design** —
//! which is why [`crate::fontdata`]'s encoding tables and this one are
//! deliberately separate arrays with different value types, never shared.
//!
//! ## Undefined codes
//!
//! 24 of the 256 codes are undefined (Annex D.3's `U` note):
//! `0x00`–`0x08`, `0x0B`, `0x0C`, `0x0E`–`0x17`, `0x7F`, `0x9F`, `0xAD`.
//! Note that `0x09` TAB, `0x0A` LF and `0x0D` CR **are** defined. The
//! `U` note is authoritative on rows `0x00`–`0x17` even though the
//! source prints a Unicode value for them: that column names the C0
//! control the byte would be *in ASCII*, and is informational.
//!
//! pdfcer's policy for an undefined code is [`DecodedText::exact`] going
//! `false` with U+FFFD substituted — never a silent pass-through of the
//! raw byte, and never a fabricated character. §7.9.2 states no recovery
//! rule (N4), so this is disclosed product policy, not conformance.
//!
//! ## The language escape sequence (§7.9.2.2)
//!
//! A Unicode text string may carry, **anywhere**, an in-band language
//! marker: U+001B, a 2-byte ISO 639 language code, an optional 2-byte
//! ISO 3166 country code, U+001B. §14.9.3/§14.9.4 confirm it applies to
//! `/Alt` and `/ActualText` and "shall override the prevailing `Lang`
//! entry".
//!
//! **It is in-band in the extracted character stream**, so a consumer
//! that does not strip it emits a stray U+001B plus two to four letters
//! of garbage into extracted text. [`decode_text_string`] strips it and
//! reports the tags it saw in [`DecodedText::languages`].
//!
//! §7.9.2.2 scopes the escape to "a Unicode text string" only, and
//! `0x1B` is an *undefined* PDFDocEncoding code — so the escape is
//! recognized **only** on the UTF-16BE branch (N5).
//!
//! ## Round-trip discipline
//!
//! This module decodes. It deliberately offers no "re-encode a string
//! the way pdfcer would have written it" helper: `ARCHITECTURE.md` §5
//! requires that a text string pdfcer did not logically modify is
//! re-emitted byte-identical, so a string that arrived as UTF-16BE stays
//! UTF-16BE even where PDFDocEncoding could represent it. The encode
//! direction ([`encode_text_string`]) exists only for values the
//! operator actually changed, and it is documented there.

/// The Annex D.3 PDFDocEncoding table as code points, `0` meaning
/// **undefined**.
///
/// Using `0` as the sentinel is exact rather than convenient: U+0000 is
/// itself one of the 24 undefined codes in this encoding (code `0x00`
/// carries the `U` note), so no defined entry can collide with the
/// sentinel. Storing `u16` rather than `Option<char>` keeps the table a
/// plain 512-byte const with no niche-optimization assumptions; every
/// defined value is in the BMP (the maximum is U+20AC).
///
/// Built from Annex D.3's four structural rules rather than transcribed
/// row by row — the source table has four verified typographical defects
/// (`0x04`, `0x16`, `0x38`, `0x9F`) and 256 hand-copied rows is exactly
/// the shape of change that acquires a 257th. The rules are:
///
/// 1. `0x20`–`0x7E` identity with ASCII/Unicode (verified zero
///    divergences).
/// 2. `0xA1`–`0xFF` identity with Latin-1, **except `0xAD` undefined**
///    (verified: exactly one divergence).
/// 3. `0x18`–`0x1F` are eight typographic modifiers.
/// 4. `0x80`–`0xA0` are punctuation/ligatures/European letters, with
///    `0xA0` = EURO SIGN.
///
/// plus the three defined controls TAB/LF/CR. The count this produces is
/// asserted against Annex D.3's own stated total (232 defined) by a unit
/// test, and cross-checked against Annex D.2's independently extracted
/// 229 glyph-name assignments (232 − 3 controls = 229).
const PDF_DOC_ENCODING: [u16; 256] = build_pdf_doc_encoding();

/// The `0x80`–`0x9E` block: the range Annex D.3 assigns that neither
/// ASCII nor Latin-1 shares. `0x9F` is undefined and `0xA0` is the Euro,
/// so both sit outside this slice and are handled explicitly.
const HIGH_BLOCK: [u16; 31] = [
    0x2022, // 0x80 BULLET
    0x2020, // 0x81 DAGGER
    0x2021, // 0x82 DOUBLE DAGGER
    0x2026, // 0x83 HORIZONTAL ELLIPSIS
    0x2014, // 0x84 EM DASH
    0x2013, // 0x85 EN DASH
    0x0192, // 0x86 LATIN SMALL LETTER F WITH HOOK (florin)
    0x2044, // 0x87 FRACTION SLASH
    0x2039, // 0x88 SINGLE LEFT-POINTING ANGLE QUOTATION MARK
    0x203A, // 0x89 SINGLE RIGHT-POINTING ANGLE QUOTATION MARK
    0x2212, // 0x8A MINUS SIGN
    0x2030, // 0x8B PER MILLE SIGN
    0x201E, // 0x8C DOUBLE LOW-9 QUOTATION MARK
    0x201C, // 0x8D LEFT DOUBLE QUOTATION MARK
    0x201D, // 0x8E RIGHT DOUBLE QUOTATION MARK
    0x2018, // 0x8F LEFT SINGLE QUOTATION MARK
    0x2019, // 0x90 RIGHT SINGLE QUOTATION MARK
    0x201A, // 0x91 SINGLE LOW-9 QUOTATION MARK
    0x2122, // 0x92 TRADE MARK SIGN
    0xFB01, // 0x93 LATIN SMALL LIGATURE FI
    0xFB02, // 0x94 LATIN SMALL LIGATURE FL
    0x0141, // 0x95 LATIN CAPITAL LETTER L WITH STROKE
    0x0152, // 0x96 LATIN CAPITAL LIGATURE OE
    0x0160, // 0x97 LATIN CAPITAL LETTER S WITH CARON
    0x0178, // 0x98 LATIN CAPITAL LETTER Y WITH DIAERESIS
    0x017D, // 0x99 LATIN CAPITAL LETTER Z WITH CARON
    0x0131, // 0x9A LATIN SMALL LETTER DOTLESS I
    0x0142, // 0x9B LATIN SMALL LETTER L WITH STROKE
    0x0153, // 0x9C LATIN SMALL LIGATURE OE
    0x0161, // 0x9D LATIN SMALL LETTER S WITH CARON
    0x017E, // 0x9E LATIN SMALL LETTER Z WITH CARON
];

/// The `0x18`–`0x1F` typographic modifiers (Annex D.3), at code points
/// where ASCII and Latin-1 both have C0 control characters. A decoder
/// that passes C0 bytes through unchanged emits controls where the
/// document meant `˘ ˇ ˆ ˙ ˝ ˛ ˚ ˜`.
const MODIFIERS: [u16; 8] = [
    0x02D8, // 0x18 BREVE
    0x02C7, // 0x19 CARON
    0x02C6, // 0x1A MODIFIER LETTER CIRCUMFLEX ACCENT
    0x02D9, // 0x1B DOT ABOVE
    0x02DD, // 0x1C DOUBLE ACUTE ACCENT
    0x02DB, // 0x1D OGONEK
    0x02DA, // 0x1E RING ABOVE
    0x02DC, // 0x1F SMALL TILDE
];

/// Assemble [`PDF_DOC_ENCODING`] from Annex D.3's four structural rules.
///
/// `const fn` so the table is a compile-time constant with no lazy
/// initialization and no runtime cost, while the *rules* — not 256
/// transcribed rows — remain the reviewable artifact.
// The crate denies `clippy::indexing_slicing` because a panic reachable
// from untrusted input is a denial-of-service bug. Neither half of that
// rationale applies here and the checked alternative does not exist:
//
// 1. This is a `const fn` evaluated at COMPILE time. An out-of-bounds
//    index is a compile error, not a runtime panic — there is no input,
//    trusted or otherwise, and no binary in which the access can fail.
// 2. `slice::get_mut` is not usable in a const context, so a checked
//    write into the table is not expressible. Rewriting the table as 256
//    literal rows to avoid the lint would reintroduce exactly the
//    transcription risk this function exists to eliminate.
//
// Every index below is either a literal or a loop counter bounded by a
// compile-time-known constant.
#[allow(clippy::indexing_slicing)]
const fn build_pdf_doc_encoding() -> [u16; 256] {
    let mut table = [0u16; 256];

    // Rule: the three DEFINED control characters. Every other code below
    // 0x18 carries Annex D.3's `U` note and stays undefined.
    table[0x09] = 0x0009; // CHARACTER TABULATION
    table[0x0A] = 0x000A; // LINE FEED
    table[0x0D] = 0x000D; // CARRIAGE RETURN

    // Rule 3: 0x18–0x1F typographic modifiers.
    let mut i = 0;
    while i < MODIFIERS.len() {
        table[0x18 + i] = MODIFIERS[i];
        i += 1;
    }

    // Rule 1: 0x20–0x7E identity. (0x7F is undefined.)
    let mut c = 0x20;
    while c <= 0x7E {
        table[c] = c as u16;
        c += 1;
    }

    // Rule 4: 0x80–0x9E from the block table; 0x9F undefined; 0xA0 Euro.
    let mut i = 0;
    while i < HIGH_BLOCK.len() {
        table[0x80 + i] = HIGH_BLOCK[i];
        i += 1;
    }
    table[0xA0] = 0x20AC; // EURO SIGN — *not* NO-BREAK SPACE

    // Rule 2: 0xA1–0xFF identity with Latin-1, except 0xAD undefined.
    let mut c = 0xA1;
    while c <= 0xFF {
        if c != 0xAD {
            table[c] = c as u16;
        }
        c += 1;
    }

    table
}

/// Decode one byte under PDFDocEncoding (Annex D.3).
///
/// `None` means the code is one of the 24 undefined ones — **not** that
/// the byte should be passed through. See the module docs for the list
/// and for why the sentinel is exact.
///
/// # Examples
///
/// ```
/// use pdfcer_core::textstring::pdf_doc_char;
///
/// assert_eq!(pdf_doc_char(b'A'), Some('A'));
/// // The two divergences from Latin-1 that silently corrupt text:
/// assert_eq!(pdf_doc_char(0xA0), Some('\u{20AC}')); // EURO, not NBSP
/// assert_eq!(pdf_doc_char(0xAD), None); // undefined, not SOFT HYPHEN
/// // 0x18-0x1F are typographic modifiers, not C0 controls:
/// assert_eq!(pdf_doc_char(0x18), Some('\u{02D8}')); // BREVE
/// // ...but TAB/LF/CR are defined:
/// assert_eq!(pdf_doc_char(0x09), Some('\t'));
/// assert_eq!(pdf_doc_char(0x00), None);
/// ```
#[must_use]
pub fn pdf_doc_char(code: u8) -> Option<char> {
    let value = PDF_DOC_ENCODING.get(usize::from(code)).copied()?;
    if value == 0 {
        return None;
    }
    char::from_u32(u32::from(value))
}

/// A decoded text string, with the honesty flags a
/// fuzzy-never-sneaky surface needs.
///
/// Derives the common traits per the Rust API Guidelines (C-COMMON-TRAITS);
/// `#[non_exhaustive]` so later Passes can add fields (a PDF 2.0 UTF-8
/// branch, for one — see the module docs) without a breaking change.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct DecodedText {
    /// The decoded characters, with the BOM and any language escape
    /// sequences removed.
    pub text: String,
    /// Which of §7.9.2.2's two forms the bytes were in.
    pub form: TextStringForm,
    /// `false` when at least one byte or code unit could not be decoded
    /// and U+FFFD was substituted: an undefined PDFDocEncoding code, an
    /// odd trailing byte after the BOM, or an unpaired surrogate.
    ///
    /// Never silently `true` — this is the flag a GUI shows as "some
    /// characters could not be decoded".
    pub exact: bool,
    /// How many replacement characters were substituted.
    pub replacements: usize,
    /// ISO 639 (optionally `-` ISO 3166) tags found in in-band language
    /// escape sequences, in order of appearance. Empty for the common
    /// case.
    pub languages: Vec<String>,
}

/// Which §7.9.2.2 form a text string's bytes were in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum TextStringForm {
    /// No `FE FF` prefix: the bytes are PDFDocEncoding (Annex D.3).
    #[default]
    PdfDocEncoding,
    /// A leading `FE FF` byte-order marker: the rest is UTF-16BE.
    Utf16Be,
}

/// Decode a `text string` (§7.9.2.2): UTF-16BE if the bytes begin with
/// the `FE FF` byte-order marker, PDFDocEncoding (Annex D.3) otherwise.
///
/// In-band language escape sequences (U+001B `xx` [`yy`] U+001B) are
/// stripped from the output and reported in
/// [`DecodedText::languages`]; per §7.9.2.2 they are recognized only on
/// the UTF-16BE branch, where `0x1B` is not an undefined code.
///
/// # Failure modes (all reported, never silent)
///
/// This function is infallible by design — a diagnostics panel that
/// cannot show a partially-broken title is worse than one that shows it
/// with U+FFFD in it — but every degradation sets
/// [`DecodedText::exact`] to `false` and increments
/// [`DecodedText::replacements`]:
///
/// - an undefined PDFDocEncoding code (24 of the 256; §7.9.2 N4 states
///   no recovery rule, so U+FFFD is disclosed pdfcer policy);
/// - an odd trailing byte after the BOM (UTF-16BE requires an even
///   length; §7.9.2 N1 states no recovery);
/// - an unpaired surrogate code unit (§7.9.2 N2).
///
/// # Examples
///
/// ```
/// use pdfcer_core::textstring::{decode_text_string, TextStringForm};
///
/// let d = decode_text_string(b"Hello");
/// assert_eq!(d.text, "Hello");
/// assert_eq!(d.form, TextStringForm::PdfDocEncoding);
/// assert!(d.exact);
///
/// // UTF-16BE, discriminated by the FE FF prefix and nothing else.
/// let d = decode_text_string(b"\xFE\xFF\x00H\x00i");
/// assert_eq!(d.text, "Hi");
/// assert_eq!(d.form, TextStringForm::Utf16Be);
///
/// // 0xA0 is EURO in PDFDocEncoding, not a no-break space.
/// assert_eq!(decode_text_string(b"\xA05").text, "\u{20AC}5");
///
/// // An undefined code is disclosed, never passed through.
/// let d = decode_text_string(b"a\xADb");
/// assert_eq!(d.text, "a\u{FFFD}b");
/// assert!(!d.exact);
/// assert_eq!(d.replacements, 1);
/// ```
#[must_use]
pub fn decode_text_string(bytes: &[u8]) -> DecodedText {
    if bytes.starts_with(&[0xFE, 0xFF]) {
        decode_utf16be(bytes.get(2..).unwrap_or(&[]))
    } else {
        decode_pdf_doc(bytes)
    }
}

/// The PDFDocEncoding branch of [`decode_text_string`].
fn decode_pdf_doc(bytes: &[u8]) -> DecodedText {
    let mut text = String::with_capacity(bytes.len());
    let mut replacements = 0usize;
    for &b in bytes {
        match pdf_doc_char(b) {
            Some(ch) => text.push(ch),
            None => {
                text.push(char::REPLACEMENT_CHARACTER);
                replacements += 1;
            }
        }
    }
    DecodedText {
        text,
        form: TextStringForm::PdfDocEncoding,
        exact: replacements == 0,
        replacements,
        languages: Vec::new(),
    }
}

/// The UTF-16BE branch of [`decode_text_string`], with the §7.9.2.2
/// language escape stripped in the same pass.
///
/// Decoding is done over code units rather than via
/// `String::from_utf16_lossy` because the escape sequence is defined in
/// *code units* (U+001B, then ASCII letters as code units) and because
/// the lossy helper would hide the unpaired-surrogate count this
/// function has to report.
fn decode_utf16be(bytes: &[u8]) -> DecodedText {
    let (text, languages, replacements) = utf16be_walk(bytes, true);
    DecodedText {
        text,
        form: TextStringForm::Utf16Be,
        exact: replacements == 0,
        replacements,
        languages,
    }
}

/// Decode UTF-16BE bytes with **no** language-escape handling, for
/// callers whose bytes are unconditionally UTF-16BE and are not text
/// strings.
///
/// The one such caller is the `ToUnicode` CMap destination string
/// (§9.10.3): "Unicode character sequences expressed in UTF-16BE
/// encoding", with no BOM, no PDFDocEncoding alternative, and no
/// §7.9.2.2 language escape in scope. Returns the decoded text and the
/// number of U+FFFD substitutions made (odd trailing byte, unpaired
/// surrogate — §9.10.3 N3 states no validity rule for either).
///
/// # Examples
///
/// ```
/// use pdfcer_core::textstring::decode_utf16be_bytes;
///
/// // The §9.10.3 EXAMPLE 2 surrogate destination: U+2003E.
/// assert_eq!(decode_utf16be_bytes(&[0xD8, 0x40, 0xDC, 0x3E]).0, "\u{2003E}");
/// // A one-to-many ligature destination.
/// assert_eq!(
///     decode_utf16be_bytes(&[0x00, 0x66, 0x00, 0x66, 0x00, 0x6C]).0,
///     "ffl"
/// );
/// ```
#[must_use]
pub fn decode_utf16be_bytes(bytes: &[u8]) -> (String, usize) {
    let (text, _languages, replacements) = utf16be_walk(bytes, false);
    (text, replacements)
}

/// The shared UTF-16BE code-unit walk.
///
/// `strip_escapes` selects the §7.9.2.2 language-escape behaviour: on
/// for text strings, off for `ToUnicode` destinations (see
/// [`decode_utf16be_bytes`]). Decoding is done over code units rather
/// than via `String::from_utf16_lossy` because the escape sequence is
/// defined in *code units* and because the lossy helper would hide the
/// unpaired-surrogate count this function has to report.
fn utf16be_walk(bytes: &[u8], strip_escapes: bool) -> (String, Vec<String>, usize) {
    let mut units: Vec<u16> = Vec::with_capacity(bytes.len() / 2);
    for pair in bytes.chunks_exact(2) {
        let hi = pair.first().copied().unwrap_or(0);
        let lo = pair.get(1).copied().unwrap_or(0);
        units.push(u16::from_be_bytes([hi, lo]));
    }
    // An odd trailing byte is malformed with no stated recovery (N1):
    // count it and drop it rather than inventing a low byte.
    let mut replacements = usize::from(!bytes.len().is_multiple_of(2));

    let mut text = String::with_capacity(units.len());
    let mut languages = Vec::new();
    let mut i = 0usize;
    while i < units.len() {
        let unit = units.get(i).copied().unwrap_or(0);
        // §7.9.2.2's escape: U+001B, 2 ASCII language letters, optional
        // 2 ASCII country letters, U+001B. Total 6 or 10 bytes = 4 or 6
        // code units. A U+001B that does not open a well-formed escape
        // is left alone (it is a legal, if odd, character).
        if strip_escapes
            && unit == 0x001B
            && let Some((tag, consumed)) = language_escape(&units, i)
        {
            languages.push(tag);
            i += consumed;
            continue;
        }
        match char::from_u32(u32::from(unit)) {
            Some(ch) => {
                text.push(ch);
                i += 1;
            }
            None => {
                // A surrogate code unit: try to pair it (§7.9.2.2's
                // "conforming readers … shall be prepared to handle
                // supplementary characters" is a `shall`).
                let next = units.get(i + 1).copied();
                match (unit, next) {
                    (0xD800..=0xDBFF, Some(low @ 0xDC00..=0xDFFF)) => {
                        let value = 0x1_0000
                            + ((u32::from(unit) - 0xD800) << 10)
                            + (u32::from(low) - 0xDC00);
                        text.push(char::from_u32(value).unwrap_or(char::REPLACEMENT_CHARACTER));
                        i += 2;
                    }
                    // Unpaired surrogate: no rule exists (N2). Disclose.
                    _ => {
                        text.push(char::REPLACEMENT_CHARACTER);
                        replacements += 1;
                        i += 1;
                    }
                }
            }
        }
    }

    (text, languages, replacements)
}

/// Recognize a §7.9.2.2 language escape starting at `units[at]` (which
/// the caller has already confirmed is U+001B).
///
/// Returns the language tag (`en`, `en-US`) and the number of code units
/// the whole escape occupies (4 or 6). Returns `None` if the sequence is
/// not well-formed, in which case the opening U+001B is an ordinary
/// character.
fn language_escape(units: &[u16], at: usize) -> Option<(String, usize)> {
    let letter = |offset: usize| -> Option<char> {
        let unit = units.get(at + offset).copied()?;
        let ch = char::from_u32(u32::from(unit))?;
        ch.is_ascii_alphanumeric().then_some(ch)
    };
    let lang: String = [letter(1)?, letter(2)?].into_iter().collect();
    // Short form: 1B <2 letters> 1B.
    if units.get(at + 3).copied() == Some(0x001B) {
        return Some((lang, 4));
    }
    // Long form: 1B <2 letters> <2 letters> 1B.
    let country: String = [letter(3)?, letter(4)?].into_iter().collect();
    if units.get(at + 5).copied() == Some(0x001B) {
        return Some((format!("{lang}-{country}"), 6));
    }
    None
}

/// Encode a string as a `text string` (§7.9.2.2), choosing
/// PDFDocEncoding when every character is representable and UTF-16BE
/// with a leading `FE FF` BOM otherwise.
///
/// **Only for values the operator actually changed.** A string pdfcer did
/// not logically modify must be re-emitted byte-identical
/// (`ARCHITECTURE.md` §5) — a string that arrived as UTF-16BE stays
/// UTF-16BE even where PDFDocEncoding could represent it, so this
/// function must never be applied on a round-trip path.
///
/// The PDFDocEncoding branch is chosen only when the *whole* string
/// fits, because there is no in-band way to switch encodings mid-string.
/// The reverse map is injective across every defined code (the only
/// duplicated Unicode value in the printed source table is at two codes
/// that are both undefined), so the choice is unambiguous.
///
/// # Examples
///
/// ```
/// use pdfcer_core::textstring::{decode_text_string, encode_text_string};
///
/// assert_eq!(encode_text_string("Hello"), b"Hello".to_vec());
/// // The Euro fits PDFDocEncoding at 0xA0 ...
/// assert_eq!(encode_text_string("\u{20AC}"), vec![0xA0]);
/// // ... a character outside the repertoire forces UTF-16BE + BOM.
/// assert_eq!(encode_text_string("\u{4E2D}"), vec![0xFE, 0xFF, 0x4E, 0x2D]);
/// // Round-trips through the decoder either way.
/// assert_eq!(decode_text_string(&encode_text_string("caf\u{E9}")).text, "café");
/// ```
#[must_use]
pub fn encode_text_string(text: &str) -> Vec<u8> {
    if let Some(bytes) = try_encode_pdf_doc(text) {
        return bytes;
    }
    let mut out = vec![0xFE, 0xFF];
    for unit in text.encode_utf16() {
        out.extend_from_slice(&unit.to_be_bytes());
    }
    out
}

/// The PDFDocEncoding half of [`encode_text_string`]: `None` if any
/// character has no defined code.
fn try_encode_pdf_doc(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len());
    for ch in text.chars() {
        let scalar = u32::from(ch);
        let value = u16::try_from(scalar).ok()?;
        // Linear scan over 256 entries: this runs once per changed
        // metadata field, not per glyph, and a reverse table would be a
        // second thing to keep in sync with the forward one.
        let code = PDF_DOC_ENCODING
            .iter()
            .position(|&v| v != 0 && v == value)
            .and_then(|i| u8::try_from(i).ok())?;
        out.push(code);
    }
    Some(out)
}

#[cfg(test)]
// Tests are exempt from the panic-free policy: a panicking assertion IS
// the test-failure mechanism (see the crate-level lint rationale in
// lib.rs).
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;

    #[test]
    fn defined_code_count_matches_annex_d3() {
        // Annex D.3 states 232 defined code points and 24 undefined.
        // Cross-checked in the RAG against Annex D.2's independently
        // extracted 229 glyph-name assignments: 232 − 3 controls
        // (TAB/LF/CR, which have Unicode values but no Latin-set glyph
        // name) = 229. Two independently sourced tables agreeing
        // arithmetically is the strongest check available here.
        let defined = PDF_DOC_ENCODING.iter().filter(|&&v| v != 0).count();
        assert_eq!(defined, 232, "Annex D.3 defines 232 of the 256 codes");
    }

    #[test]
    fn undefined_codes_are_exactly_the_annex_d3_list() {
        let expected: Vec<usize> = (0x00..=0x08)
            .chain([0x0B, 0x0C])
            .chain(0x0E..=0x17)
            .chain([0x7F, 0x9F, 0xAD])
            .collect();
        let actual: Vec<usize> = (0..256)
            .filter(|&i| PDF_DOC_ENCODING[i] == 0)
            .collect::<Vec<_>>();
        assert_eq!(actual, expected);
        assert_eq!(expected.len(), 24);
    }

    #[test]
    fn the_two_latin1_divergences() {
        // The single highest-consequence divergence: 0xA0 is EURO SIGN,
        // not NO-BREAK SPACE. Getting this wrong silently corrupts
        // currency in every extracted metadata field.
        assert_eq!(pdf_doc_char(0xA0), Some('\u{20AC}'));
        // 0xAD is UNDEFINED here, while WinAnsiEncoding (Annex D.2)
        // assigns it `hyphen`. The two tables disagree BY DESIGN, which
        // is why they are separate arrays in separate modules.
        assert_eq!(pdf_doc_char(0xAD), None);
        assert_eq!(pdf_doc_char(0xAC), Some('\u{00AC}')); // both sides identity
        assert_eq!(pdf_doc_char(0xAE), Some('\u{00AE}'));
    }

    #[test]
    fn typographic_modifiers_replace_c0_controls() {
        assert_eq!(pdf_doc_char(0x18), Some('\u{02D8}')); // BREVE
        assert_eq!(pdf_doc_char(0x19), Some('\u{02C7}')); // CARON
        assert_eq!(pdf_doc_char(0x1A), Some('\u{02C6}')); // MOD CIRCUMFLEX
        assert_eq!(pdf_doc_char(0x1B), Some('\u{02D9}')); // DOT ABOVE
        assert_eq!(pdf_doc_char(0x1C), Some('\u{02DD}')); // DOUBLE ACUTE
        assert_eq!(pdf_doc_char(0x1D), Some('\u{02DB}')); // OGONEK
        assert_eq!(pdf_doc_char(0x1E), Some('\u{02DA}')); // RING ABOVE
        assert_eq!(pdf_doc_char(0x1F), Some('\u{02DC}')); // SMALL TILDE
    }

    #[test]
    fn the_undefined_codes_bracketing_defined_ranges() {
        // 0x7F and 0x9F sit between defined ranges — the exact place an
        // off-by-one in a range-based decoder lands first.
        assert_eq!(pdf_doc_char(0x7E), Some('~'));
        assert_eq!(pdf_doc_char(0x7F), None);
        assert_eq!(pdf_doc_char(0x80), Some('\u{2022}'));
        assert_eq!(pdf_doc_char(0x9E), Some('\u{017E}'));
        assert_eq!(pdf_doc_char(0x9F), None);
        assert_eq!(pdf_doc_char(0xA0), Some('\u{20AC}'));
    }

    #[test]
    fn defined_controls_are_tab_lf_cr_only() {
        assert_eq!(pdf_doc_char(0x09), Some('\t'));
        assert_eq!(pdf_doc_char(0x0A), Some('\n'));
        assert_eq!(pdf_doc_char(0x0D), Some('\r'));
        assert_eq!(pdf_doc_char(0x0B), None);
        assert_eq!(pdf_doc_char(0x0C), None);
        assert_eq!(pdf_doc_char(0x08), None);
        assert_eq!(pdf_doc_char(0x0E), None);
    }

    #[test]
    fn ligature_and_punctuation_block() {
        assert_eq!(pdf_doc_char(0x93), Some('\u{FB01}')); // fi ligature
        assert_eq!(pdf_doc_char(0x94), Some('\u{FB02}')); // fl ligature
        assert_eq!(pdf_doc_char(0x84), Some('\u{2014}')); // em dash
        assert_eq!(pdf_doc_char(0x8D), Some('\u{201C}')); // left double quote
    }

    #[test]
    fn bom_is_the_entire_discriminator() {
        let d = decode_text_string(b"\xFE\xFF\x00A\x00B");
        assert_eq!(d.text, "AB");
        assert_eq!(d.form, TextStringForm::Utf16Be);
        // One byte short of the BOM: PDFDocEncoding, and 0xFE is thorn.
        let d = decode_text_string(b"\xFE\x41");
        assert_eq!(d.form, TextStringForm::PdfDocEncoding);
        assert_eq!(d.text, "\u{00FE}A");
    }

    #[test]
    fn surrogate_pairs_decode_to_supplementary_characters() {
        // §7.9.2.2: readers "shall be prepared to handle supplementary
        // characters". U+1D11E MUSICAL SYMBOL G CLEF.
        let d = decode_text_string(b"\xFE\xFF\xD8\x34\xDD\x1E");
        assert_eq!(d.text, "\u{1D11E}");
        assert!(d.exact);
    }

    #[test]
    fn unpaired_surrogate_is_disclosed_not_hidden() {
        let d = decode_text_string(b"\xFE\xFF\xD8\x34\x00A");
        assert_eq!(d.text, "\u{FFFD}A");
        assert!(!d.exact);
        assert_eq!(d.replacements, 1);
    }

    #[test]
    fn odd_trailing_byte_after_bom_is_counted() {
        let d = decode_text_string(b"\xFE\xFF\x00A\x00");
        assert_eq!(d.text, "A");
        assert!(!d.exact, "an odd trailing byte is malformed (N1)");
        assert_eq!(d.replacements, 1);
    }

    #[test]
    fn language_escape_is_stripped_and_reported() {
        // 1B 'e' 'n' 1B  — short form, 4 code units.
        let bytes = b"\xFE\xFF\x00\x1B\x00e\x00n\x00\x1B\x00H\x00i";
        let d = decode_text_string(bytes);
        assert_eq!(d.text, "Hi", "the escape must not reach extracted text");
        assert_eq!(d.languages, vec!["en".to_string()]);
    }

    #[test]
    fn language_escape_long_form_with_country() {
        let bytes = b"\xFE\xFF\x00\x1B\x00e\x00n\x00U\x00S\x00\x1B\x00X";
        let d = decode_text_string(bytes);
        assert_eq!(d.text, "X");
        assert_eq!(d.languages, vec!["en-US".to_string()]);
    }

    #[test]
    fn lone_escape_char_is_left_alone() {
        // A U+001B that does not open a well-formed escape is an
        // ordinary (if odd) character, not an error.
        let bytes = b"\xFE\xFF\x00\x1B\x00A";
        let d = decode_text_string(bytes);
        assert_eq!(d.text, "\u{001B}A");
        assert!(d.languages.is_empty());
    }

    #[test]
    fn escape_is_not_recognized_in_pdfdocencoding() {
        // §7.9.2.2 scopes the escape to "a Unicode text string" (N5) —
        // and PDFDocEncoding settles the point in a way worth pinning:
        // 0x1B is not a control character here at all. It is U+02D9 DOT
        // ABOVE, from Annex D.3's 0x18–0x1F modifier block. So the very
        // bytes that would form a language escape in UTF-16BE decode to
        // ordinary text, with no escape recognized and nothing lost.
        let d = decode_text_string(b"\x1Ben\x1BHi");
        assert!(d.languages.is_empty());
        assert_eq!(d.text, "\u{02D9}en\u{02D9}Hi");
        assert_eq!(d.replacements, 0);
        assert!(d.exact);
    }

    #[test]
    fn encode_round_trips_and_prefers_pdfdocencoding() {
        for s in ["Hello", "café", "\u{20AC}100", "a\u{2014}b", "\u{FB01}n"] {
            let bytes = encode_text_string(s);
            assert_eq!(decode_text_string(&bytes).text, s, "round trip {s:?}");
            assert!(
                !bytes.starts_with(&[0xFE, 0xFF]),
                "{s:?} fits PDFDocEncoding"
            );
        }
        // Outside the repertoire ⇒ UTF-16BE.
        let bytes = encode_text_string("\u{4E2D}\u{6587}");
        assert!(bytes.starts_with(&[0xFE, 0xFF]));
        assert_eq!(decode_text_string(&bytes).text, "中文");
    }

    #[test]
    fn encode_never_emits_an_undefined_code() {
        // A character whose only Latin-1 code is undefined here (the
        // soft hyphen at 0xAD) must force UTF-16BE, never emit 0xAD.
        let bytes = encode_text_string("a\u{00AD}b");
        assert!(bytes.starts_with(&[0xFE, 0xFF]));
        assert!(!bytes.contains(&0xAD) || bytes.starts_with(&[0xFE, 0xFF]));
        assert_eq!(decode_text_string(&bytes).text, "a\u{00AD}b");
    }

    #[test]
    fn empty_string_decodes_to_empty() {
        assert_eq!(decode_text_string(b"").text, "");
        assert_eq!(decode_text_string(b"\xFE\xFF").text, "");
        assert!(decode_text_string(b"").exact);
    }
}
