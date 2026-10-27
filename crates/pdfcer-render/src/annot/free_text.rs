//! A `/FreeText` with no `/AP`, drawn from its `/DA`, `/Contents`, `/Q`,
//! `/Rect` and `/RD` (decision 203, `Pass 646.22`).
//!
//! ISO 32000-1 Table 174: `/DA` is "the default appearance string that shall
//! be used in formatting the text". Two things the file may not state are
//! settled here, each disclosed in the annotation's note:
//!
//! - **The font.** `/DA` names a resource; §12.7.3.3 resolves it against the
//!   AcroForm `/DR`, and how a FreeText resolves it is open (pdf-issues #726,
//!   spec RAG FT-A4). A `/DR` entry naming a standard-14 font is used; any
//!   other case draws in Helvetica and says so.
//! - **What `/C` colours.** No key carries a FreeText fill (FT-A1). Acrobat
//!   reads `/C` as the fill; pdfcer's own FreeText writes `/C` as the border
//!   and `/IC` as the fill. [`FreeTextC`] chooses, unless `/IC` is present.
//!
//! Declined: a callout (`/CL` or `/IT /FreeTextCallout`), rich text with no
//! plain `/Contents`, and a missing or unreadable `/DA`.

use pdfcer_core::annot_author::{self, AuthoredTextAnnot, ColorEntry, TextAnnotSpec};
use pdfcer_core::fontdata::{self, Std14};
use pdfcer_core::graph::ObjectGraph;
use pdfcer_core::object::{Dict, Object};
use pdfcer_core::vartext;
use pdfcer_core::view::DocumentView;

/// Which colour a `/FreeText`'s `/C` is when the renderer draws one that has
/// no `/AP` and no `/IC` (ISO 32000 defines no FreeText fill key).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum FreeTextC {
    /// `/C` fills the box, as Acrobat reads it. The default.
    #[default]
    Fill,
    /// `/C` strokes the border, as pdfcer's own FreeText writes it.
    Border,
}

/// A baked FreeText look plus the disclosures it owes.
pub(super) struct Baked {
    pub(super) annot: AuthoredTextAnnot,
    pub(super) notes: Vec<String>,
}

fn name_is(doc: &DocumentView<'_>, dict: &Dict, key: &[u8], value: &[u8]) -> bool {
    matches!(dict.get(key).map(|o| doc.resolve(o)),
        Some(Object::Name(n)) if n.as_bytes() == value)
}

fn string_bytes(doc: &DocumentView<'_>, dict: &Dict, key: &[u8]) -> Option<Vec<u8>> {
    match dict.get(key).map(|o| doc.resolve(o)) {
        Some(Object::String(s)) => Some(s.clone()),
        _ => None,
    }
}

/// `/DR /Font /<name> /BaseFont`, the §12.7.3.3 resolution of a `/DA` name.
fn dr_base_font(doc: &DocumentView<'_>, name: &[u8]) -> Option<String> {
    let catalog = doc.catalog_dict()?;
    let acroform = doc.resolve(catalog.get(b"AcroForm")?).as_dict()?;
    let dr = doc.resolve(acroform.get(b"DR")?).as_dict()?;
    let fonts = doc.resolve(dr.get(b"Font")?).as_dict()?;
    let font = doc.resolve(fonts.get(name)?).as_dict()?;
    match doc.resolve(font.get(b"BaseFont")?) {
        Object::Name(n) => Some(String::from_utf8_lossy(n.as_bytes()).into_owned()),
        _ => None,
    }
}

/// The font to draw in, and the disclosure when it is a substitute.
fn resolve_font(doc: &DocumentView<'_>, da: &[u8]) -> (Std14, Option<String>) {
    let name = vartext::parse_default_appearance(da)
        .map(|p| p.font_name)
        .unwrap_or_default();
    let shown = String::from_utf8_lossy(&name).into_owned();
    match dr_base_font(doc, &name) {
        Some(base) => match fontdata::std14_by_base_font(&base) {
            Some(std) => (std, None),
            None => (
                Std14::Helvetica,
                Some(format!("font /{shown} ({base}) drawn in Helvetica")),
            ),
        },
        None => (
            Std14::Helvetica,
            Some(format!(
                "font /{shown} is not in the form's /DR, drawn in Helvetica"
            )),
        ),
    }
}

/// Why this FreeText cannot be drawn without guessing, if it cannot.
fn decline_reason(doc: &DocumentView<'_>, dict: &Dict) -> Option<String> {
    if dict.get(b"CL").is_some() || name_is(doc, dict, b"IT", b"FreeTextCallout") {
        return Some("it is a callout (/CL), which pdfcer does not draw without /AP".to_owned());
    }
    let plain_empty = string_bytes(doc, dict, b"Contents").is_none_or(|s| s.is_empty());
    if plain_empty && dict.get(b"RC").is_some() {
        return Some("its text is rich text only (/RC) with no plain /Contents".to_owned());
    }
    None
}

/// Bake `dict` as pdfcer would author it, or say why not.
pub(super) fn bake(doc: &DocumentView<'_>, dict: &Dict, mode: FreeTextC) -> Result<Baked, String> {
    if let Some(why) = decline_reason(doc, dict) {
        return Err(why);
    }
    let mut spec = annot_author::text_spec_from_dict(doc, dict).map_err(|e| e.to_string())?;
    let mut notes = Vec::new();
    let has_ic = !matches!(
        annot_author::color_entry(doc, dict, b"IC"),
        ColorEntry::Absent
    );
    if let TextAnnotSpec::FreeText {
        rect,
        font,
        multiline,
        border,
        frame,
        ..
    } = &mut spec
    {
        // Table 174 /RD: the text and border sit in the inner rectangle.
        if let Some(inner) = annot_author::rect_inside_differences(doc, dict) {
            *rect = inner;
        }
        // The inner rectangle is "where the text should be displayed": wrap.
        *multiline = true;
        let da = string_bytes(doc, dict, b"DA").unwrap_or_default();
        let (std, substituted) = resolve_font(doc, &da);
        *font = std;
        notes.extend(substituted);
        if !has_ic && border.is_some() && mode == FreeTextC::Fill {
            frame.fill = border.take();
            notes.push("/C read as the fill colour (Acrobat's reading)".to_owned());
            if dict.get(b"BS").is_some() {
                notes.push("border not drawn: no key states its colour".to_owned());
            }
        }
    }
    if dict.get(b"RC").is_some() {
        notes.push("rich text (/RC) styling not reproduced; drawn from /Contents".to_owned());
    }
    let annot = annot_author::build_text_annotation(&spec).map_err(|e| e.to_string())?;
    if let Some(size) = annot.applied_autosize {
        notes.push(format!("auto-size (0 Tf) chose {size:.1} pt"));
    }
    if annot.unencodable_chars > 0 {
        notes.push(format!(
            "{} character(s) outside WinAnsi drawn as ?",
            annot.unencodable_chars
        ));
    }
    Ok(Baked { annot, notes })
}
