//! The look of a signature field (ISO 32000-1 §12.7.4.5): the text its
//! signature dictionary states (§12.8.1 Table 252 `/Name`, `/M`, `/Reason`,
//! `/Location`) set in the widget's rectangle, with an optional image, and
//! the neutral placeholder of an unsigned field.
//!
//! Nothing here claims validity. Table 252 calls `/M` an unverified time and
//! `/Name` a convenience for when the certificate cannot be read, so what is
//! drawn is what the dictionary says, never whether it verifies.

use crate::annot_author::{self, WidgetChrome};
use crate::edit::decode_text_string;
use crate::field_display::DisplayNotes;
use crate::fontdata::{self, BaseEncoding, Std14};
use crate::forms::{Field, FieldValue, MAX_FIELD_TREE_DEPTH};
use crate::graph::ObjectGraph;
use crate::object::{Dict, Name, ObjId, Object};
use crate::view::DocumentView;

/// Smallest size the layout will use; below this the text is not legible in
/// print, so the layout drops optional lines or refuses instead (rule 4).
pub const APPEARANCE_MIN_SIZE: f64 = 4.0;
/// Largest size the layout will use: the frame stays a modest label.
pub const APPEARANCE_MAX_SIZE: f64 = 10.0;
/// Line height as a multiple of the size.
const LEADING: f64 = 1.15;
/// Inset from the frame, pt, each side.
const PAD: f64 = 2.0;
/// The resource name the image is drawn under.
const IMAGE_NAME: &[u8] = b"Im0";

/// How the signer's name and the date are worded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum SignatureLabels {
    /// The values alone: the name, then the date. What a viewer draws for a
    /// signed field with no `/AP`.
    #[default]
    Plain,
    /// `Digitally signed by <name>` and `Date: <date>`, both always present:
    /// the lines the signing flow writes.
    Described,
}

/// An image XObject to draw beside the text, by reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct SignatureImage {
    /// The image XObject (§8.9.5); named `/Im0` in the appearance's
    /// resources.
    pub xobject: ObjId,
    /// Its `/Width` in samples, for the aspect ratio.
    pub width: u32,
    /// Its `/Height` in samples.
    pub height: u32,
}

impl SignatureImage {
    /// An image XObject `xobject` of `width` x `height` samples.
    #[must_use]
    pub const fn new(xobject: ObjId, width: u32, height: u32) -> Self {
        Self {
            xobject,
            width,
            height,
        }
    }
}

/// Builds a signature widget's `/AP /N` form: the name, the date, an
/// optional reason and location, and an optional image, fitted to the
/// rectangle by shrinking the text, then by dropping the location and the
/// reason, never by overflowing it.
///
/// ```
/// use pdfcer_core::widget_draw::SignatureAppearanceBuilder;
///
/// let ap = SignatureAppearanceBuilder::new("A. Tester", "D:20261111120000Z")
///     .reason("Approved")
///     .build(160.0, 40.0)?;
/// assert_eq!(ap.lines, ["A. Tester", "2026.11.11 12:00:00 Z", "Reason: Approved"]);
/// assert!(ap.dropped.is_empty());
/// # Ok::<(), pdfcer_core::widget_draw::SignatureAppearanceError>(())
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct SignatureAppearanceBuilder {
    name: String,
    date: String,
    reason: Option<String>,
    location: Option<String>,
    image: Option<SignatureImage>,
    labels: SignatureLabels,
    chrome: Option<WidgetChrome>,
}

impl SignatureAppearanceBuilder {
    /// A builder for signer `name` and signing time `date`. `date` is a PDF
    /// date (§7.9.4) shown as `YYYY.MM.DD HH:MM:SS` plus its zone; any other
    /// text is shown as given. Under [`SignatureLabels::Plain`] an empty name
    /// or date is left out.
    #[must_use]
    pub fn new(name: impl Into<String>, date: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            date: date.into(),
            reason: None,
            location: None,
            image: None,
            labels: SignatureLabels::Plain,
            chrome: None,
        }
    }

    /// Add a `Reason:` line (Table 252 `/Reason`); dropped second to fit.
    #[must_use]
    pub fn reason(mut self, reason: impl Into<String>) -> Self {
        self.reason = Some(reason.into());
        self
    }

    /// Add a `Location:` line (Table 252 `/Location`); dropped first to fit.
    #[must_use]
    pub fn location(mut self, location: impl Into<String>) -> Self {
        self.location = Some(location.into());
        self
    }

    /// Draw `image` beside the text: at the left of a wide rectangle, at the
    /// top of a tall one, aspect ratio kept and taking at most half; the
    /// whole of it when there is no text.
    #[must_use]
    pub const fn image(mut self, image: SignatureImage) -> Self {
        self.image = Some(image);
        self
    }

    /// Word the lines as `labels` says.
    #[must_use]
    pub const fn labels(mut self, labels: SignatureLabels) -> Self {
        self.labels = labels;
        self
    }

    /// Draw the widget's `/MK` background and border (Table 189) instead of
    /// the default thin black frame.
    #[must_use]
    pub fn chrome(mut self, chrome: WidgetChrome) -> Self {
        self.chrome = Some(chrome);
        self
    }

    /// The form for a `w` x `h` pt rectangle. A size, location or reason
    /// that had to give way to fit is reported on the result.
    ///
    /// # Errors
    ///
    /// [`SignatureAppearanceError::DoesNotFit`] when the name and date alone
    /// do not fit at [`APPEARANCE_MIN_SIZE`].
    pub fn build(&self, w: f64, h: f64) -> Result<SignatureAppearance, SignatureAppearanceError> {
        let (w, h) = (finite_extent(w), finite_extent(h));
        let (required, mut optional) = self.compose();
        let has_text = !required.is_empty() || !optional.is_empty();
        let (image_box, text_box) = split(self.image, has_text, w, h);
        let mut dropped = Vec::new();
        let (lines, size) = loop {
            let lines: Vec<String> = required.iter().chain(&optional).cloned().collect();
            if let Some(size) = fit(&lines, text_box) {
                break (lines, size);
            }
            let Some(line) = optional.pop() else {
                return Err(SignatureAppearanceError::DoesNotFit {
                    lines: required.len(),
                    width: w,
                    height: h,
                    min_size: APPEARANCE_MIN_SIZE,
                });
            };
            dropped.insert(0, line);
        };
        let mut content = match &self.chrome {
            Some(chrome) => annot_author::field_box_art(chrome, w, h, None),
            None => thin_frame(w, h).into_bytes(),
        };
        let image_drawn = match (self.image, image_box) {
            (Some(image), Some(rect)) => place_image(&mut content, image, rect),
            _ => false,
        };
        let font_size = (!lines.is_empty()).then_some(size);
        if let Some(size) = font_size {
            content.extend_from_slice(text_content(&lines, size, text_box).as_bytes());
        }
        Ok(SignatureAppearance {
            ap_dict: form_dict(w, h, self.image.filter(|_| image_drawn)),
            content,
            auto_sized: font_size.is_some_and(|s| s < APPEARANCE_MAX_SIZE),
            font_size,
            lines,
            dropped,
            image_drawn,
        })
    }

    /// The lines that must show, and the optional ones in drop-last order.
    fn compose(&self) -> (Vec<String>, Vec<String>) {
        let name = self.name.trim();
        let date = human_pdf_date(self.date.trim());
        let required = match self.labels {
            SignatureLabels::Described => {
                vec![
                    format!("Digitally signed by {name}"),
                    format!("Date: {date}"),
                ]
            }
            SignatureLabels::Plain => [name.to_owned(), date]
                .into_iter()
                .filter(|l| !l.is_empty())
                .collect(),
        };
        let optional = [("Reason", &self.reason), ("Location", &self.location)]
            .into_iter()
            .filter_map(|(label, v)| {
                let v = v.as_deref()?.trim();
                (!v.is_empty()).then(|| format!("{label}: {v}"))
            })
            .collect();
        (required, optional)
    }
}

/// A built signature appearance and what fitting it cost.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct SignatureAppearance {
    /// The form dict: `/Type /XObject /Subtype /Form`, `/Resources` (an
    /// inline standard-14 `/Helv`, and `/Im0` when an image is drawn) and
    /// `/BBox [0 0 w h]`. No `/Length`; the writer adds it.
    pub ap_dict: Dict,
    /// The content-stream bytes (unfiltered, 7-bit clean).
    pub content: Vec<u8>,
    /// The lines drawn, top to bottom.
    pub lines: Vec<String>,
    /// Optional lines left out so the rest would fit, in the order they
    /// would have been drawn.
    pub dropped: Vec<String>,
    /// The Helvetica size the lines are set in; `None` when there are none.
    pub font_size: Option<f64>,
    /// Whether the text was shrunk below [`APPEARANCE_MAX_SIZE`] to fit.
    pub auto_sized: bool,
    /// Whether the image was drawn (false for one with no samples).
    pub image_drawn: bool,
}

/// Why a signature appearance could not be built.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[non_exhaustive]
pub enum SignatureAppearanceError {
    /// The name and date do not fit the rectangle even at the minimum size.
    #[error(
        "the signature's {lines} required line(s) do not fit a {width} x {height} pt rectangle \
         at the {min_size} pt minimum size"
    )]
    DoesNotFit {
        /// How many required lines there were.
        lines: usize,
        /// The rectangle's width, pt.
        width: f64,
        /// The rectangle's height, pt.
        height: f64,
        /// The smallest size tried, pt.
        min_size: f64,
    },
}

/// An unsigned signature field's placeholder: the `/MK` background and
/// border, and a neutral grey "sign here" mark (a cross on a baseline)
/// drawn as vector strokes, with no text and no font.
pub(crate) fn placeholder(chrome: &WidgetChrome, w: f64, h: f64) -> (Dict, Vec<u8>) {
    let (w, h) = (finite_extent(w), finite_extent(h));
    let mut content = annot_author::field_box_art(chrome, w, h, None);
    let s = (h * 0.4).min(w * 0.2);
    if s > 0.0 {
        let inset = PAD + 1.0;
        let x0 = inset + s * 0.25;
        let base = h * 0.25;
        let lw = (s / 10.0).max(0.5);
        content.extend_from_slice(
            format!(
                "q 0 0 {w} {h} re W n 0.6 G {lw} w 1 J\n{x0} {b0} m {x1} {b1} l {x0} {b1} m {x1} {b0} l\n\
                 {x0} {base} m {xe} {base} l S Q\n",
                w = num(w),
                h = num(h),
                lw = num(lw),
                x0 = num(x0),
                x1 = num(x0 + s),
                b0 = num(base + s * 0.25),
                b1 = num(base + s * 1.25),
                base = num(base),
                xe = num((w - inset).max(x0 + s)),
            )
            .as_bytes(),
        );
    }
    (form_dict(w, h, None), content)
}

/// [`view_look`] as a [`super::SynthesizedAppearance`].
pub(crate) fn synthesized(
    view: &DocumentView<'_>,
    field: &Field,
    chrome: &WidgetChrome,
    size: (f64, f64),
) -> super::SynthesizedAppearance {
    let mut notes = DisplayNotes::default();
    let (ap_dict, content, signed) = view_look(view, field, chrome, size, &mut notes);
    super::SynthesizedAppearance {
        ap_dict,
        content,
        kind: if signed {
            super::SynthesizedKind::Signature
        } else {
            super::SynthesizedKind::SignaturePlaceholder
        },
        notes,
    }
}

/// A signature field's view-time look, and whether it is signed. Unsigned:
/// the [`placeholder`]. Signed: the `/MK` chrome and the signature
/// dictionary's `/Name` and `/M` only, [`SignatureLabels::Plain`]; what was
/// drawn and what was not goes in `notes`. A document timestamp's
/// properties are not drawn: Table 252 says that with `/SubFilter
/// /ETSI.RFC3161` they "should not be used and should be ignored".
pub(crate) fn view_look(
    view: &DocumentView<'_>,
    field: &Field,
    chrome: &WidgetChrome,
    (w, h): (f64, f64),
    notes: &mut DisplayNotes,
) -> (Dict, Vec<u8>, bool) {
    let Some(sig) = signature_dict(view, field) else {
        let (dict, content) = placeholder(chrome, w, h);
        return (dict, content, false);
    };
    let text = |key: &[u8]| match sig.get(key).map(|o| view.resolve(o)) {
        Some(Object::String(s)) => decode_text_string(&s[..]).text.trim().to_owned(),
        _ => String::new(),
    };
    let timestamp = sig
        .get(b"SubFilter")
        .and_then(|o| view.resolve(o).as_name())
        .is_some_and(|n| n.as_bytes() == b"ETSI.RFC3161");
    let (name, date) = if timestamp {
        notes.signature_omitted.push(
            "/Name and /M of a document timestamp (/SubFilter /ETSI.RFC3161), which Table 252 \
             says should be ignored"
                .to_owned(),
        );
        (String::new(), String::new())
    } else {
        (text(b"Name"), text(b"M"))
    };
    for (value, missing) in [(&name, "/Name"), (&date, "/M")] {
        if value.is_empty() && !timestamp {
            notes
                .signature_omitted
                .push(format!("{missing}: the signature dictionary has none"));
        }
    }
    let built = SignatureAppearanceBuilder::new(name, date)
        .chrome(chrome.clone())
        .build(w, h);
    match built {
        Ok(ap) => {
            notes.signature_text.clone_from(&ap.lines);
            (ap.ap_dict, ap.content, true)
        }
        Err(e) => {
            notes.signature_omitted.push(format!("the text: {e}"));
            let (w, h) = (finite_extent(w), finite_extent(h));
            let content = annot_author::field_box_art(chrome, w, h, None);
            (form_dict(w, h, None), content, true)
        }
    }
}

/// The signature dictionary a signed field's `/V` names, own or inherited
/// through `/Parent` (§12.7.3.1), depth-bounded.
fn signature_dict<'v>(view: &'v DocumentView<'_>, field: &Field) -> Option<&'v Dict> {
    if field.value != FieldValue::Signature {
        return None;
    }
    let mut node = view.resolved(field.id).as_dict()?;
    for _ in 0..MAX_FIELD_TREE_DEPTH {
        if let Some(v) = node.get(b"V") {
            return view.resolve(v).as_dict();
        }
        node = view.resolve(node.get(b"Parent")?).as_dict()?;
    }
    None
}

/// A rectangle `(x, y, w, h)` in form space.
type Region = (f64, f64, f64, f64);

/// A non-negative, finite extent.
fn finite_extent(v: f64) -> f64 {
    if v.is_finite() { v.max(0.0) } else { 0.0 }
}

/// The image region and the text region: side by side when wide, stacked
/// when tall, each alone when the other is absent. The image's region is
/// what its aspect ratio needs at the rect's full height (wide) or width
/// (tall), at most half, so the text keeps the rest.
fn split(image: Option<SignatureImage>, text: bool, w: f64, h: f64) -> (Option<Region>, Region) {
    let Some(image) = image else {
        return (None, (0.0, 0.0, w, h));
    };
    if !text {
        return (Some((0.0, 0.0, w, h)), (0.0, 0.0, 0.0, 0.0));
    }
    let aspect = f64::from(image.width.max(1)) / f64::from(image.height.max(1));
    if w >= h {
        let iw = ((h - 2.0 * PAD).max(0.0) * aspect + 2.0 * PAD).min(w / 2.0);
        (Some((0.0, 0.0, iw, h)), (iw, 0.0, w - iw, h))
    } else {
        let ih = ((w - 2.0 * PAD).max(0.0) / aspect + 2.0 * PAD).min(h / 2.0);
        (Some((0.0, h - ih, w, ih)), (0.0, 0.0, w, h - ih))
    }
}

/// The one Helvetica size at which every line fits `region` (inside the pad,
/// at the leading), at most the maximum; `None` below the minimum.
fn fit(lines: &[String], (_, _, w, h): Region) -> Option<f64> {
    if lines.is_empty() {
        return Some(APPEARANCE_MAX_SIZE);
    }
    let inner_w = (w - 2.0 * PAD).max(0.0);
    let inner_h = (h - 2.0 * PAD).max(0.0);
    let widest = lines
        .iter()
        .map(|l| helvetica_width_units(l))
        .max()
        .unwrap_or(0);
    let by_width = if widest == 0 {
        APPEARANCE_MAX_SIZE
    } else {
        inner_w * 1000.0 / f64::from(widest)
    };
    #[allow(clippy::cast_precision_loss)] // a handful of lines
    let by_height = inner_h / (lines.len() as f64 * LEADING);
    let size = by_width.min(by_height).min(APPEARANCE_MAX_SIZE);
    // Two decimals is plenty and keeps the stream short and stable.
    (size >= APPEARANCE_MIN_SIZE).then(|| (size * 100.0).floor() / 100.0)
}

/// The default frame: 1 pt black, inset half a point.
fn thin_frame(w: f64, h: f64) -> String {
    format!(
        "0 0 0 RG 1 w 0.5 0.5 {} {} re S\n",
        (w - 1.0).max(0.0),
        (h - 1.0).max(0.0)
    )
}

/// Draw `image` fitted inside `region`'s pad, aspect kept, centred; false
/// when either has no extent.
fn place_image(content: &mut Vec<u8>, image: SignatureImage, (x, y, w, h): Region) -> bool {
    let (iw, ih) = (f64::from(image.width), f64::from(image.height));
    let (aw, ah) = ((w - 2.0 * PAD).max(0.0), (h - 2.0 * PAD).max(0.0));
    if iw <= 0.0 || ih <= 0.0 || aw <= 0.0 || ah <= 0.0 {
        return false;
    }
    let scale = (aw / iw).min(ah / ih);
    let (dw, dh) = (iw * scale, ih * scale);
    let (dx, dy) = (x + PAD + (aw - dw) / 2.0, y + PAD + (ah - dh) / 2.0);
    content.extend_from_slice(
        format!(
            "q {} 0 0 {} {} {} cm /Im0 Do Q\n",
            num(dw),
            num(dh),
            num(dx),
            num(dy)
        )
        .as_bytes(),
    );
    true
}

/// The lines set in `/Helv` at `size`, top-down from `region`'s pad.
fn text_content(lines: &[String], size: f64, (x, y, _, h): Region) -> String {
    let leading = size * LEADING;
    // First baseline: one size below the top pad (ascent ≈ size for a label).
    let first_y = y + h - PAD - size;
    let mut out = format!(
        "BT 0 g /Helv {size} Tf {} {first_y} Td {leading} TL\n",
        x + PAD
    );
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            out.push_str("T* ");
        }
        out.push_str(&pdf_literal_winansi(line));
        out.push_str(" Tj\n");
    }
    out.push_str("ET\n");
    out
}

/// The form dictionary, with `/Im0` bound when `image` is drawn.
fn form_dict(w: f64, h: f64, image: Option<SignatureImage>) -> Dict {
    let name = |n: &[u8]| Object::Name(Name::from(n));
    let mut helv = Dict::new();
    helv.insert(Name::from(b"Type"), name(b"Font"));
    helv.insert(Name::from(b"Subtype"), name(b"Type1"));
    helv.insert(Name::from(b"BaseFont"), name(b"Helvetica"));
    helv.insert(Name::from(b"Encoding"), name(b"WinAnsiEncoding"));
    let mut fonts = Dict::new();
    fonts.insert(Name::from(b"Helv"), Object::Dict(helv));
    let mut resources = Dict::new();
    resources.insert(Name::from(b"Font"), Object::Dict(fonts));
    if let Some(image) = image {
        let mut xobjects = Dict::new();
        xobjects.insert(Name::from(IMAGE_NAME), Object::Reference(image.xobject));
        resources.insert(Name::from(b"XObject"), Object::Dict(xobjects));
    }
    let mut ap = Dict::new();
    ap.insert(Name::from(b"Type"), name(b"XObject"));
    ap.insert(Name::from(b"Subtype"), name(b"Form"));
    ap.insert(Name::from(b"Resources"), Object::Dict(resources));
    ap.insert(
        Name::from(b"BBox"),
        Object::Array(vec![
            Object::Real(0.0),
            Object::Real(0.0),
            Object::Real(w),
            Object::Real(h),
        ]),
    );
    ap
}

/// `v` to two decimals, as a content-stream number.
fn num(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

/// `D:YYYYMMDDHHmmSS<zone>` → `YYYY.MM.DD HH:mm:SS <zone>`; anything that
/// is not that shape is returned as given.
pub(crate) fn human_pdf_date(s: &str) -> String {
    let d = s.strip_prefix("D:").unwrap_or(s);
    let digits = d.bytes().take_while(u8::is_ascii_digit).count();
    if digits < 14 {
        return s.to_owned();
    }
    let (n, zone) = d.split_at(14);
    let zone = zone.trim();
    let mut out = format!(
        "{}.{}.{} {}:{}:{}",
        &n[0..4],
        &n[4..6],
        &n[6..8],
        &n[8..10],
        &n[10..12],
        &n[12..14]
    );
    if !zone.is_empty() {
        out.push(' ');
        out.push_str(zone);
    }
    out
}

/// Width of `text` in Helvetica at 1000 units/em, WinAnsi-encoded; a
/// character WinAnsi cannot encode counts as `?`, which is also what the
/// content stream shows for it (see [`pdf_literal_winansi`]).
fn helvetica_width_units(text: &str) -> u32 {
    let (bytes, _) = winansi_bytes(text);
    bytes
        .iter()
        .map(|&code| {
            fontdata::encoding_glyph_name(BaseEncoding::WinAnsi, code)
                .and_then(|g| fontdata::std14_width(Std14::Helvetica, g))
                .map_or(556, u32::from)
        })
        .sum()
}

/// Encode `text` as WinAnsi bytes; returns the bytes and how many
/// characters were replaced by `?` because WinAnsi has no code for them.
fn winansi_bytes(text: &str) -> (Vec<u8>, usize) {
    let mut map = std::collections::HashMap::new();
    for code in 0u8..=255 {
        if let Some(c) = fontdata::encoding_glyph_name(BaseEncoding::WinAnsi, code)
            .and_then(fontdata::glyph_name_to_unicode)
        {
            map.entry(c).or_insert(code);
        }
    }
    let mut replaced = 0usize;
    let bytes = text
        .chars()
        .map(|c| {
            if c.is_ascii() {
                c as u8
            } else if let Some(&b) = map.get(&c) {
                b
            } else {
                replaced += 1;
                b'?'
            }
        })
        .collect();
    (bytes, replaced)
}

/// `text` as a PDF literal string `(…)` in WinAnsi, with `(`, `)` and the
/// backslash escaped and every non-ASCII byte written as an octal escape,
/// so the content stream stays 7-bit clean (ISO 32000-1 §7.3.4.2).
fn pdf_literal_winansi(text: &str) -> String {
    let (bytes, _) = winansi_bytes(text);
    let mut out = String::from("(");
    for b in bytes {
        match b {
            0x28 | 0x29 | 0x5C => {
                out.push(char::from(0x5C));
                out.push(char::from(b));
            }
            0x20..=0x7E => out.push(char::from(b)),
            _ => out.push_str(&format!("{}{:03o}", char::from(0x5C), b)),
        }
    }
    out.push(')');
    out
}

#[cfg(test)]
// Tests: a panic is the failure report.
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn lines(l: &[&str]) -> Vec<String> {
        l.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn fit_shrinks_and_refuses_below_the_floor() {
        let l = lines(&[
            "Digitally signed by Somebody",
            "Date: 2026.09.06 03:18:44 Z",
        ]);
        let s = fit(&l, (0.0, 0.0, 300.0, 60.0)).unwrap();
        assert!((s - APPEARANCE_MAX_SIZE).abs() < 1e-9);
        let s = fit(&l, (0.0, 0.0, 90.0, 60.0)).unwrap();
        assert!(
            (APPEARANCE_MIN_SIZE..APPEARANCE_MAX_SIZE).contains(&s),
            "{s}"
        );
        assert!(fit(&l, (0.0, 0.0, 20.0, 8.0)).is_none());
    }

    #[test]
    fn text_escapes_and_encodes() {
        let c = text_content(
            &lines(&["A (b) c", "caf\u{e9} \u{2603}"]),
            8.0,
            (0.0, 0.0, 200.0, 40.0),
        );
        assert!(c.contains("/Helv 8 Tf 2 30 Td"));
        let bs = char::from(0x5C);
        assert!(c.contains(&format!("(A {bs}(b{bs}) c) Tj")));
        // é is WinAnsi 0xE9 → octal 351; the snowman has no code → '?'.
        assert!(c.contains(&format!("(caf{bs}351 ?) Tj")), "{c}");
        assert!(c.is_ascii());
        assert_eq!(
            thin_frame(200.0, 40.0),
            "0 0 0 RG 1 w 0.5 0.5 199 39 re S\n"
        );
    }

    #[test]
    fn dates_are_humanised_or_kept() {
        assert_eq!(
            human_pdf_date("D:20260906031844+05'30'"),
            "2026.09.06 03:18:44 +05'30'"
        );
        assert_eq!(human_pdf_date("yesterday"), "yesterday");
    }

    #[test]
    fn described_labels_always_carry_both_lines() {
        let ap = SignatureAppearanceBuilder::new("", "")
            .labels(SignatureLabels::Described)
            .build(200.0, 40.0)
            .unwrap();
        assert_eq!(ap.lines, ["Digitally signed by ", "Date: "]);
        let plain = SignatureAppearanceBuilder::new(" ", "")
            .build(200.0, 40.0)
            .unwrap();
        assert!(plain.lines.is_empty() && plain.font_size.is_none());
    }

    #[test]
    fn placeholder_is_strokes_only() {
        let (_, c) = placeholder(&WidgetChrome::default(), 120.0, 30.0);
        let c = String::from_utf8(c).unwrap();
        assert!(c.contains(" m ") && c.ends_with("S Q\n"), "{c}");
        assert!(!c.contains("BT") && !c.contains("Tf"), "{c}");
        assert!(placeholder(&WidgetChrome::default(), 0.0, 0.0).1.is_empty());
    }
}
