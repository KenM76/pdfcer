//! Bates numbering: the label format and where a label sits on a page.
//!
//! Pure functions only; [`crate::edit::EditSession::stamp_bates`] writes the
//! stamp. A label is drawn in Helvetica (a standard-14 font, never embedded)
//! with `WinAnsiEncoding` (ISO 32000-1 §9.6.2.2, Annex D.2), so a label is
//! limited to characters that encoding has; anything else is refused rather
//! than drawn as a substitute glyph.

use crate::fontdata::{self, BaseEncoding, Std14};
use crate::page_tree::Rect;

/// The most digits a Bates number may be padded to.
pub const MAX_DIGITS: u8 = 15;

/// How a Bates number is written: `prefix`, the number zero-padded to
/// `digits`, then `suffix` — `ACME000123-C` for prefix `ACME`, 6 digits,
/// suffix `-C`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct BatesNumbering {
    /// Text before the number.
    pub prefix: String,
    /// Minimum width of the number, zero-padded; 1 to [`MAX_DIGITS`].
    pub digits: u8,
    /// Text after the number.
    pub suffix: String,
}

impl Default for BatesNumbering {
    /// No prefix or suffix, six digits.
    fn default() -> Self {
        Self {
            prefix: String::new(),
            digits: 6,
            suffix: String::new(),
        }
    }
}

impl BatesNumbering {
    /// A numbering with the given parts.
    #[must_use]
    pub fn new(prefix: impl Into<String>, digits: u8, suffix: impl Into<String>) -> Self {
        Self {
            prefix: prefix.into(),
            digits,
            suffix: suffix.into(),
        }
    }

    /// The label for `number`.
    ///
    /// # Errors
    ///
    /// [`BatesError::Digits`] when `digits` is 0 or above [`MAX_DIGITS`];
    /// [`BatesError::Overflow`] when `number` needs more than `digits`
    /// digits — a Bates series has a fixed width, and a wider number would
    /// break the sort order the width exists for;
    /// [`BatesError::Unencodable`] when the prefix or suffix has a character
    /// Helvetica's `WinAnsiEncoding` cannot draw.
    ///
    /// # Examples
    ///
    /// ```
    /// use pdfcer_core::bates::BatesNumbering;
    ///
    /// let n = BatesNumbering::new("ACME", 6, "");
    /// assert_eq!(n.label(123)?, "ACME000123");
    /// assert!(n.label(1_000_000).is_err());
    /// # Ok::<(), pdfcer_core::bates::BatesError>(())
    /// ```
    pub fn label(&self, number: u64) -> Result<String, BatesError> {
        self.check()?;
        let width = usize::from(self.digits);
        let digits = number.to_string();
        if digits.len() > width {
            return Err(BatesError::Overflow {
                number,
                digits: self.digits,
            });
        }
        Ok(format!("{}{digits:0>width$}{}", self.prefix, self.suffix))
    }

    /// Validate the digit count and the prefix/suffix characters.
    ///
    /// # Errors
    ///
    /// [`BatesError::Digits`] or [`BatesError::Unencodable`], as
    /// [`Self::label`].
    pub fn check(&self) -> Result<(), BatesError> {
        if self.digits == 0 || self.digits > MAX_DIGITS {
            return Err(BatesError::Digits(self.digits));
        }
        for c in self.prefix.chars().chain(self.suffix.chars()) {
            if winansi_code(c).is_none() {
                return Err(BatesError::Unencodable(c));
            }
        }
        Ok(())
    }

    /// The largest number this numbering can write.
    #[must_use]
    pub fn max_number(&self) -> u64 {
        10u64
            .checked_pow(u32::from(self.digits.min(MAX_DIGITS)))
            .map_or(u64::MAX, |p| p - 1)
    }
}

/// Where on the page a Bates label sits, as the page is displayed (after
/// `/Rotate`), inside its crop box.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum BatesPosition {
    /// Top edge, left end.
    TopLeft,
    /// Top edge, centred.
    TopCenter,
    /// Top edge, right end.
    TopRight,
    /// Bottom edge, left end.
    BottomLeft,
    /// Bottom edge, centred.
    BottomCenter,
    /// Bottom edge, right end — the usual place for a Bates number.
    #[default]
    BottomRight,
}

impl BatesPosition {
    /// Every position, top row first.
    pub const ALL: [Self; 6] = [
        Self::TopLeft,
        Self::TopCenter,
        Self::TopRight,
        Self::BottomLeft,
        Self::BottomCenter,
        Self::BottomRight,
    ];

    /// `true` for the three top positions.
    #[must_use]
    pub const fn is_top(self) -> bool {
        matches!(self, Self::TopLeft | Self::TopCenter | Self::TopRight)
    }
}

/// A Bates stamping request for [`crate::edit::EditSession::stamp_bates`].
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct BatesStamp {
    /// The label format.
    pub numbering: BatesNumbering,
    /// Where on each displayed page the label sits.
    pub position: BatesPosition,
    /// Distance in points from the displayed page edges; at least 0.
    pub margin: f64,
    /// Font size in points; above 0.
    pub font_size: f64,
    /// 0-based pages to stamp; numbered in document order whatever order
    /// they are given in. `None` stamps every page.
    pub pages: Option<Vec<usize>>,
}

impl Default for BatesStamp {
    /// Six digits, bottom right, 36 pt margin, 10 pt, every page.
    fn default() -> Self {
        Self {
            numbering: BatesNumbering::default(),
            position: BatesPosition::default(),
            margin: 36.0,
            font_size: 10.0,
            pages: None,
        }
    }
}

impl BatesStamp {
    /// A stamp with `numbering` and every other field at its default.
    #[must_use]
    pub fn new(numbering: BatesNumbering) -> Self {
        Self {
            numbering,
            ..Self::default()
        }
    }
}

/// What [`crate::edit::EditSession::stamp_bates`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct BatesOutcome {
    /// The number on the first stamped page.
    pub first: u64,
    /// The number the next document in a batch starts at.
    pub next: u64,
    /// 0-based pages stamped, in the order numbered.
    pub pages: Vec<usize>,
    /// The first label written.
    pub first_label: String,
    /// The last label written.
    pub last_label: String,
}

/// Why a Bates label could not be made or placed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum BatesError {
    /// The digit count is 0 or above [`MAX_DIGITS`].
    #[error("a Bates number has 1 to {MAX_DIGITS} digits, not {0}")]
    Digits(u8),
    /// The number needs more digits than the numbering allows.
    #[error("Bates number {number} does not fit in {digits} digits")]
    Overflow {
        /// The number that did not fit.
        number: u64,
        /// The numbering's digit count.
        digits: u8,
    },
    /// A prefix or suffix character Helvetica's `WinAnsiEncoding` has no
    /// code for.
    #[error("the Bates label character {0:?} cannot be drawn in Helvetica (WinAnsiEncoding)")]
    Unencodable(char),
    /// The margin is negative or not finite, or the font size is not above 0.
    #[error(
        "a Bates stamp needs a margin of at least 0 and a font size above 0 (got margin {margin}, size {size})"
    )]
    Geometry {
        /// The margin given, in points.
        margin: String,
        /// The font size given, in points.
        size: String,
    },
    /// The stamp selects no pages.
    #[error("a Bates stamp needs at least one page")]
    NoPages,
}

/// `c`'s `WinAnsiEncoding` code (Annex D.2), if it has one.
pub(crate) fn winansi_code(c: char) -> Option<u8> {
    if c.is_ascii_graphic() || c == ' ' {
        return u8::try_from(c).ok();
    }
    (0x80u8..=0xFF).find(|&code| {
        fontdata::encoding_glyph_name(BaseEncoding::WinAnsi, code)
            .and_then(fontdata::glyph_name_to_unicode)
            == Some(c)
    })
}

/// `label` as `WinAnsiEncoding` bytes. Callers have run
/// [`BatesNumbering::check`], so every character has a code; one without is
/// skipped rather than drawn as something else.
pub(crate) fn winansi_bytes(label: &str) -> Vec<u8> {
    label.chars().filter_map(winansi_code).collect()
}

/// Width of `bytes` in Helvetica, in text-space units at size 1 (§9.2.4:
/// glyph widths are in thousandths of a unit of text space).
pub(crate) fn helvetica_width(bytes: &[u8]) -> f64 {
    bytes
        .iter()
        .map(|&code| {
            fontdata::encoding_glyph_name(BaseEncoding::WinAnsi, code)
                .and_then(|g| fontdata::std14_width(Std14::Helvetica, g))
                .map_or(0.0, f64::from)
        })
        .sum::<f64>()
        / 1000.0
}

/// The content stream that draws one label: restore the state the shared
/// leading `q` saved, then the label as a `/Pagination /Bates` artifact
/// (ISO 32000-2 §14.8.2.2.2, Table 363) in font resource `font` at size 1
/// under text matrix `m`, black, with `label` as WinAnsi bytes.
pub(crate) fn label_content(font: &[u8], m: [f64; 6], label: &[u8]) -> Vec<u8> {
    let mut out = LABEL_HEAD.to_vec();
    out.extend_from_slice(b"BT /");
    out.extend_from_slice(font);
    out.extend_from_slice(b" 1 Tf 0 g ");
    for v in m {
        out.extend_from_slice(number(v).as_bytes());
        out.push(b' ');
    }
    out.extend_from_slice(b"Tm (");
    for &b in label {
        match b {
            b'(' | b')' | b'\\' => out.extend_from_slice(&[b'\\', b]),
            0x20..=0x7E => out.push(b),
            _ => out.extend_from_slice(format!("\\{b:03o}").as_bytes()),
        }
    }
    out.extend_from_slice(b") Tj ET\nEMC Q\n");
    out
}

/// How every label stream [`label_content`] writes begins. Removal
/// recognises pdfcer's own labels by it, and nothing else.
pub(crate) const LABEL_HEAD: &[u8] = b"q /Artifact <</Type /Pagination /Subtype /Bates>> BDC\n";

/// The font resource name and label text of a stream [`label_content`]
/// wrote, or `None` for any other stream.
pub(crate) fn parse_label(content: &[u8]) -> Option<(Vec<u8>, String)> {
    let rest = content.strip_prefix(LABEL_HEAD)?.strip_prefix(b"BT /")?;
    let end = rest.iter().position(|&b| b == b' ')?;
    let font = rest.get(..end)?.to_vec();
    let open = rest.iter().position(|&b| b == b'(')?;
    let mut label = String::new();
    let mut bytes = rest.get(open + 1..)?.iter().copied();
    while let Some(b) = bytes.next() {
        let code = match b {
            b')' => return Some((font, label)),
            b'\\' => match bytes.next()? {
                d @ b'0'..=b'7' => {
                    let mut v = u32::from(d - b'0');
                    for _ in 0..2 {
                        v = v * 8 + u32::from(bytes.next()?.checked_sub(b'0')?);
                    }
                    u8::try_from(v).ok()?
                }
                other => other,
            },
            other => other,
        };
        label.push(winansi_char(code));
    }
    None
}

/// `true` when `content` has the name token `/name` (§7.3.5: a name ends
/// at whitespace or a delimiter), so `/Bates` is not found in `/Bates2`.
pub(crate) fn names_resource(content: &[u8], name: &[u8]) -> bool {
    let mut token = vec![b'/'];
    token.extend_from_slice(name);
    content.windows(token.len()).enumerate().any(|(at, w)| {
        w == token.as_slice()
            && content
                .get(at + token.len())
                .is_none_or(|&b| b.is_ascii_whitespace() || b"()<>[]{}/%".contains(&b))
    })
}

/// The character `WinAnsiEncoding` code `code` draws; U+FFFD for none.
fn winansi_char(code: u8) -> char {
    if code.is_ascii_graphic() || code == b' ' {
        return char::from(code);
    }
    fontdata::encoding_glyph_name(BaseEncoding::WinAnsi, code)
        .and_then(fontdata::glyph_name_to_unicode)
        .unwrap_or('\u{FFFD}')
}

/// What [`crate::edit::EditSession::remove_bates`] took off.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct BatesRemoval {
    /// 0-based pages that had a label removed, ascending.
    pub pages: Vec<usize>,
    /// Every removed label's text, page by page, in drawing order.
    pub labels: Vec<String>,
}

/// A content-stream number: at most four decimals, no trailing zeros.
fn number(v: f64) -> String {
    let s = format!("{v:.4}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" {
        "0".to_owned()
    } else {
        s.to_owned()
    }
}

/// The text matrix (§9.4.2 `Tm`) that draws a label of `width` points at
/// `size` points upright at `position` on a page with crop box `crop` and
/// `/Rotate` `rotate`, `margin` points in from the displayed edges.
///
/// The label's glyphs stay inside the margin: a bottom label's descenders
/// and a top label's ascenders (Helvetica's AFM values) end at the margin.
/// `rotate` is taken modulo 360; a value that is not a multiple of 90 is
/// treated as 0, as the page tree reader already normalises it.
#[must_use]
pub fn label_matrix(
    crop: Rect,
    rotate: u16,
    position: BatesPosition,
    margin: f64,
    size: f64,
    width: f64,
) -> [f64; 6] {
    let quarter = (rotate % 360) / 90;
    let (w, h) = (crop.urx - crop.llx, crop.ury - crop.lly);
    // The displayed page's width and height.
    let (vw, vh) = if quarter % 2 == 1 { (h, w) } else { (w, h) };
    let d = fontdata::std14_descriptor(Std14::Helvetica);
    let u = match position {
        BatesPosition::TopLeft | BatesPosition::BottomLeft => margin,
        BatesPosition::TopCenter | BatesPosition::BottomCenter => (vw - width) / 2.0,
        BatesPosition::TopRight | BatesPosition::BottomRight => vw - margin - width,
    };
    let v = if position.is_top() {
        vh - margin - f64::from(d.ascender) * size / 1000.0
    } else {
        margin - f64::from(d.descender) * size / 1000.0
    };
    // Map displayed (u, v) to user space. `/Rotate` turns the page
    // clockwise for display (§7.7.3.3 Table 30), so for 90 the displayed
    // rightward axis is user +y and the displayed upward axis is user -x.
    match quarter {
        1 => [0.0, size, -size, 0.0, crop.urx - v, crop.lly + u],
        2 => [-size, 0.0, 0.0, -size, crop.urx - u, crop.ury - v],
        3 => [0.0, -size, size, 0.0, crop.llx + v, crop.ury - u],
        _ => [size, 0.0, 0.0, size, crop.llx + u, crop.lly + v],
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::float_cmp)] // test assertions
mod tests {
    use super::*;

    fn letter() -> Rect {
        Rect {
            llx: 0.0,
            lly: 0.0,
            urx: 612.0,
            ury: 792.0,
        }
    }

    /// Where `(tx, ty)` in text space lands in user space under `m`.
    fn apply(m: [f64; 6], tx: f64, ty: f64) -> (f64, f64) {
        (m[0] * tx + m[2] * ty + m[4], m[1] * tx + m[3] * ty + m[5])
    }

    #[test]
    fn labels_are_padded_and_refuse_overflow() {
        let n = BatesNumbering::new("AB", 3, "-X");
        assert_eq!(n.label(7).unwrap(), "AB007-X");
        assert_eq!(n.label(999).unwrap(), "AB999-X");
        assert_eq!(
            n.label(1000),
            Err(BatesError::Overflow {
                number: 1000,
                digits: 3
            })
        );
        assert_eq!(n.max_number(), 999);
        assert_eq!(
            BatesNumbering::new("", 15, "").max_number(),
            999_999_999_999_999
        );
    }

    #[test]
    fn digit_count_and_characters_are_checked() {
        assert_eq!(
            BatesNumbering::new("", 0, "").check(),
            Err(BatesError::Digits(0))
        );
        assert_eq!(
            BatesNumbering::new("", 16, "").check(),
            Err(BatesError::Digits(16))
        );
        assert!(BatesNumbering::new("Société ", 6, "").check().is_ok());
        assert_eq!(
            BatesNumbering::new("\u{4e2d}", 6, "").check(),
            Err(BatesError::Unencodable('\u{4e2d}'))
        );
        assert_eq!(winansi_bytes("é"), vec![0xE9]);
    }

    #[test]
    fn a_bottom_right_label_ends_at_the_margin_on_an_upright_page() {
        let m = label_matrix(letter(), 0, BatesPosition::BottomRight, 36.0, 10.0, 50.0);
        let (x0, y) = apply(m, 0.0, 0.0);
        let (x1, _) = apply(m, 5.0, 0.0);
        assert!((x1 - (612.0 - 36.0)).abs() < 1e-9 && (x0 - 526.0).abs() < 1e-9);
        // Helvetica's descender is -207: the baseline sits 2.07 pt above it.
        assert!((y - 38.07).abs() < 1e-9, "{y}");
    }

    /// On every rotation the label's end lands at the DISPLAYED bottom-right
    /// corner and reads left to right as displayed.
    #[test]
    fn rotation_keeps_the_label_upright_in_the_displayed_corner() {
        let crop = Rect {
            llx: 10.0,
            lly: 20.0,
            urx: 210.0,
            ury: 120.0,
        };
        // (rotate, user-space point of the displayed bottom-right corner,
        //  user-space direction of the displayed rightward axis)
        let cases = [
            (0, (210.0, 20.0), (1.0, 0.0)),
            (90, (210.0, 120.0), (0.0, 1.0)),
            (180, (10.0, 120.0), (-1.0, 0.0)),
            (270, (10.0, 20.0), (0.0, -1.0)),
        ];
        for (rotate, corner, dir) in cases {
            let m = label_matrix(crop, rotate, BatesPosition::BottomRight, 0.0, 1.0, 30.0);
            let (ex, ey) = apply(
                m,
                30.0,
                f64::from(fontdata::std14_descriptor(Std14::Helvetica).descender) / 1000.0,
            );
            assert!(
                (ex - corner.0).abs() < 1e-9 && (ey - corner.1).abs() < 1e-9,
                "{rotate}: ({ex}, {ey})"
            );
            assert_eq!((m[0], m[1]), dir, "{rotate}");
        }
    }

    #[test]
    fn a_centred_label_is_centred_on_the_displayed_width() {
        let m = label_matrix(letter(), 90, BatesPosition::TopCenter, 36.0, 10.0, 100.0);
        // Displayed width is 792 on a 90-degree page; the centred start is
        // (792 - 100) / 2 = 346 along user +y.
        assert_eq!(m[5], 346.0);
    }

    #[test]
    fn helvetica_widths_come_from_the_afm() {
        // "0" is 556 units in Helvetica.
        assert!((helvetica_width(b"00") - 1.112).abs() < 1e-12);
    }

    #[test]
    fn a_written_label_parses_back_to_its_font_and_text() {
        let label = winansi_bytes("A(1)\\é");
        let content = label_content(b"Bates2", [10.0, 0.0, 0.0, 10.0, 5.0, 6.0], &label);
        let (font, text) = parse_label(&content).expect("recognised");
        assert_eq!(font, b"Bates2");
        assert_eq!(text, "A(1)\\é");
        assert!(parse_label(b"q BT /F1 1 Tf (x) Tj ET Q").is_none());
    }

    #[test]
    fn a_resource_name_is_matched_as_a_whole_token() {
        assert!(names_resource(b"/Bates 1 Tf", b"Bates"));
        assert!(names_resource(b"BT /Bates", b"Bates"));
        assert!(names_resource(b"/Bates/F1", b"Bates"));
        assert!(!names_resource(b"/Bates2 1 Tf", b"Bates"));
        assert!(!names_resource(b"(Bates) Tj", b"Bates"));
    }

    #[test]
    fn a_label_is_escaped_into_a_seven_bit_literal() {
        let label = winansi_bytes("(\u{e9})\\1");
        let content = label_content(b"Bates1", [10.0, 0.0, 0.0, 10.0, 1.5, -0.0], &label);
        let text = String::from_utf8(content).expect("7-bit");
        assert!(text.contains("/Bates1 1 Tf 0 g 10 0 0 10 1.5 0 Tm (\\(\\351\\)\\\\1) Tj"));
        assert!(text.starts_with("q /Artifact <</Type /Pagination /Subtype /Bates>> BDC"));
        assert!(text.ends_with("EMC Q\n"));
    }
}
