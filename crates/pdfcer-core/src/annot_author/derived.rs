//! The look of a markup annotation that has no `/AP`, derived from what its
//! own dictionary states (decision 203: derived, not invented).
//!
//! One derivation, two callers: `pdfcer-render` paints it and stores nothing;
//! [`crate::edit::EditSession::bake_missing_appearances`] writes it as a real
//! `/AP` on request. Anything the file does not state is declined with a
//! reason or, where ISO 32000 leaves it open, settled and disclosed in
//! [`DerivedAppearance::notes`].
//!
//! `/FreeText` (ISO 32000-1 Table 174) has two such gaps:
//!
//! - **The font.** `/DA` names a resource; §12.7.3.3 resolves it against the
//!   AcroForm `/DR` (pdf-issues #726, spec RAG FT-A4). A `/DR` entry naming a
//!   standard-14 font is used; anything else draws in Helvetica, noted.
//! - **What `/C` colours.** No key carries a FreeText fill (FT-A1).
//!   [`FreeTextC`] chooses, unless `/IC` is present.

use super::{ColorEntry, MarkupSpec, Rect, TextAnnotSpec};
use crate::fontdata::{self, Std14};
use crate::graph::ObjectGraph;
use crate::object::{Dict, Object};
use crate::vartext;

/// Which colour a `/FreeText`'s `/C` is when its look is derived and it has
/// no `/IC` (ISO 32000 defines no FreeText fill key).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum FreeTextC {
    /// `/C` fills the box, as Acrobat reads it. The default.
    #[default]
    Fill,
    /// `/C` strokes the border, as pdfcer's own FreeText writes it.
    Border,
}

/// The `/Subtype`s whose look [`derive_appearance`] can derive.
pub const DERIVED_SUBTYPES: [&str; 11] = [
    "Ink",
    "Highlight",
    "Underline",
    "StrikeOut",
    "Squiggly",
    "Square",
    "Circle",
    "Polygon",
    "PolyLine",
    "Line",
    "FreeText",
];

/// A derived look as the baker drew it.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct DerivedAppearance {
    /// The `/AP /N` form dictionary (`/BBox`, `/Resources`; no `/Length`).
    pub ap_dict: Dict,
    /// The appearance content-stream bytes.
    pub ap_content: Vec<u8>,
    /// Where the look is drawn: its `/BBox` maps onto this rectangle.
    pub rect: Rect,
    /// What was settled rather than read, in words, for disclosure.
    pub notes: Vec<String>,
}

/// A [`DerivedAppearance`] placed against an annotation's own `/Rect`, as
/// [`DerivedAppearance::place`] returns it: the dictionary to write and the
/// `/Rect` it needs.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct PlacedAppearance {
    /// The `/AP /N` form dictionary, `/BBox` adjusted (no `/Length`).
    pub ap_dict: Dict,
    /// The `/Rect` the annotation must carry for `ap_dict` to draw the look.
    pub rect: Rect,
    /// `rect` differs from the annotation's own `/Rect`.
    pub rect_changed: bool,
}

fn rect_numbers(dict: &Dict, key: &[u8]) -> Option<Rect> {
    let Some(Object::Array(a)) = dict.get(key) else {
        return None;
    };
    let n: Vec<f64> = a.iter().filter_map(Object::as_number).collect();
    match n.as_slice() {
        [a, b, c, d] if a.is_finite() && b.is_finite() && c.is_finite() && d.is_finite() => {
            Some(Rect::from_corners(*a, *b, *c, *d))
        }
        _ => None,
    }
}

impl DerivedAppearance {
    /// Place the look against the annotation's `/Rect` (`own`), keeping
    /// that rectangle unless the look extends past it.
    ///
    /// ISO 32000-1 §12.5.5 maps the form's `/BBox` (through `/Matrix`) onto
    /// `/Rect`. With no `/Matrix` and a `/BBox` the size of [`Self::rect`],
    /// that map is a translation, so the `/BBox` is widened by the same
    /// amount as the `/Rect` (their union with `own`) and the look lands
    /// exactly where the baker put it. Otherwise `/Rect` becomes
    /// [`Self::rect`]. Painting and baking both place through this, so a
    /// baked page renders as the unbaked one did, clip edge included.
    #[must_use]
    pub fn place(&self, own: Option<Rect>) -> PlacedAppearance {
        let look = self.rect;
        let bbox = rect_numbers(&self.ap_dict, b"BBox");
        let same_size = bbox.is_some_and(|b| {
            ((b.urx - b.llx) - (look.urx - look.llx)).abs() < 1e-9
                && ((b.ury - b.lly) - (look.ury - look.lly)).abs() < 1e-9
        });
        let mut ap_dict = self.ap_dict.clone();
        let rect = match (own, bbox) {
            (Some(r), Some(b)) if same_size && self.ap_dict.get(b"Matrix").is_none() => {
                let u = Rect {
                    llx: r.llx.min(look.llx),
                    lly: r.lly.min(look.lly),
                    urx: r.urx.max(look.urx),
                    ury: r.ury.max(look.ury),
                };
                let (dx, dy) = (b.llx - look.llx, b.lly - look.lly);
                let widened = [u.llx + dx, u.lly + dy, u.urx + dx, u.ury + dy];
                ap_dict.insert(
                    crate::object::Name::from(b"BBox"),
                    Object::Array(widened.iter().map(|v| Object::Real(*v)).collect()),
                );
                u
            }
            _ => look,
        };
        PlacedAppearance {
            ap_dict,
            rect,
            rect_changed: own != Some(rect),
        }
    }
}

/// Why an annotation's look was not derived.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{reason}")]
#[non_exhaustive]
pub struct NotDerived {
    /// The reason, in words, e.g. "it is a pdf dimension".
    pub reason: String,
}

impl NotDerived {
    fn new(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
        }
    }
}

fn name_is<G: ObjectGraph + ?Sized>(graph: &G, dict: &Dict, key: &[u8], value: &[u8]) -> bool {
    matches!(dict.get(key).map(|o| graph.resolve(o)),
        Some(Object::Name(n)) if n.as_bytes() == value)
}

fn subtype_of<G: ObjectGraph + ?Sized>(graph: &G, dict: &Dict) -> String {
    match dict.get(b"Subtype").map(|o| graph.resolve(o)) {
        Some(Object::Name(n)) => String::from_utf8_lossy(n.as_bytes()).into_owned(),
        _ => String::new(),
    }
}

/// Derive the look of annotation `dict` from its own keys, or say why not.
///
/// `dict` should have no `/AP /N`; one that has is derived all the same,
/// which is the caller's choice to make. The result is what pdfcer would
/// author for that dictionary, through the same bakers
/// ([`super::build_appearance_opts`], [`super::build_text_annotation`]).
///
/// # Errors
///
/// [`NotDerived`] for a subtype outside [`DERIVED_SUBTYPES`]; a `/Line` that
/// is a pdf dimension (`/Cap true`, `/LL` or `/IT /LineDimension`); no device
/// colour (a closed shape: neither `/C` nor `/IC`); malformed geometry or a
/// 2.0 `/Path`-only shape; a FreeText callout (`/CL`,
/// `/IT /FreeTextCallout`), rich text with no plain `/Contents`, or a
/// missing or unreadable `/DA`.
pub fn derive_appearance<G: ObjectGraph + ?Sized>(
    graph: &G,
    dict: &Dict,
    free_text_c: FreeTextC,
) -> Result<DerivedAppearance, NotDerived> {
    let subtype = subtype_of(graph, dict);
    if !DERIVED_SUBTYPES.contains(&subtype.as_str()) {
        return Err(NotDerived::new(format!(
            "/{subtype} has no look pdfcer derives"
        )));
    }
    if subtype == "FreeText" {
        return free_text(graph, dict, free_text_c);
    }
    let spec = markup_spec(graph, dict, &subtype)?;
    let opts = super::appearance_options_from_dict(graph, dict);
    let baked = super::build_appearance_opts(&spec, &opts);
    Ok(DerivedAppearance {
        ap_dict: baked.ap_dict,
        ap_content: baked.ap_content,
        rect: baked.rect,
        notes: Vec::new(),
    })
}

/// §12.5.6.7 Table 175: `/Cap true`, `/LL` and `/IT /LineDimension` make a
/// line a dimension.
fn is_dimension_line<G: ObjectGraph + ?Sized>(graph: &G, dict: &Dict) -> bool {
    let cap = matches!(
        dict.get(b"Cap").map(|o| graph.resolve(o)),
        Some(Object::Boolean(true))
    );
    cap || name_is(graph, dict, b"IT", b"LineDimension") || dict.get(b"LL").is_some()
}

fn has_colour<G: ObjectGraph + ?Sized>(graph: &G, dict: &Dict, key: &[u8]) -> bool {
    matches!(super::color_entry(graph, dict, key), ColorEntry::Device(_))
}

fn markup_spec<G: ObjectGraph + ?Sized>(
    graph: &G,
    dict: &Dict,
    subtype: &str,
) -> Result<MarkupSpec, NotDerived> {
    if subtype == "Line" && is_dimension_line(graph, dict) {
        return Err(NotDerived::new(
            "it is a pdf dimension (/Cap, /LL or /IT /LineDimension)",
        ));
    }
    let closed = matches!(subtype, "Square" | "Circle" | "Polygon");
    let coloured = has_colour(graph, dict, b"C") || (closed && has_colour(graph, dict, b"IC"));
    if !coloured {
        return Err(NotDerived::new("it states no colour to draw with (/C)"));
    }
    let mut spec =
        super::spec_from_dict(graph, dict).map_err(|e| NotDerived::new(e.to_string()))?;
    // §12.5.6.8: the shape sits inside /Rect less /RD. `spec_from_dict`
    // insets only under a border effect, which is the authoring round trip.
    if let MarkupSpec::Square {
        rect,
        border_effect: None,
        ..
    }
    | MarkupSpec::Circle { rect, .. } = &mut spec
        && let Some(inner) = super::rect_inside_differences(graph, dict)
    {
        *rect = inner;
    }
    Ok(spec)
}

fn string_bytes<G: ObjectGraph + ?Sized>(graph: &G, dict: &Dict, key: &[u8]) -> Option<Vec<u8>> {
    match dict.get(key).map(|o| graph.resolve(o)) {
        Some(Object::String(s)) => Some(s.clone()),
        _ => None,
    }
}

/// `/DR /Font /<name> /BaseFont`, the §12.7.3.3 resolution of a `/DA` name.
fn dr_base_font<G: ObjectGraph + ?Sized>(graph: &G, name: &[u8]) -> Option<String> {
    let catalog = graph.catalog_dict()?;
    let acroform = graph.resolve(catalog.get(b"AcroForm")?).as_dict()?;
    let dr = graph.resolve(acroform.get(b"DR")?).as_dict()?;
    let fonts = graph.resolve(dr.get(b"Font")?).as_dict()?;
    let font = graph.resolve(fonts.get(name)?).as_dict()?;
    match graph.resolve(font.get(b"BaseFont")?) {
        Object::Name(n) => Some(String::from_utf8_lossy(n.as_bytes()).into_owned()),
        _ => None,
    }
}

/// The font to draw in, and the disclosure when it is a substitute.
fn resolve_font<G: ObjectGraph + ?Sized>(graph: &G, da: &[u8]) -> (Std14, Option<String>) {
    let name = vartext::parse_default_appearance(da)
        .map(|p| p.font_name)
        .unwrap_or_default();
    let shown = String::from_utf8_lossy(&name).into_owned();
    match dr_base_font(graph, &name) {
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
fn free_text_decline<G: ObjectGraph + ?Sized>(graph: &G, dict: &Dict) -> Option<NotDerived> {
    if dict.get(b"CL").is_some() || name_is(graph, dict, b"IT", b"FreeTextCallout") {
        return Some(NotDerived::new(
            "it is a callout (/CL), which pdfcer does not draw without /AP",
        ));
    }
    let plain_empty = string_bytes(graph, dict, b"Contents").is_none_or(|s| s.is_empty());
    if plain_empty && dict.get(b"RC").is_some() {
        return Some(NotDerived::new(
            "its text is rich text only (/RC) with no plain /Contents",
        ));
    }
    None
}

/// A FreeText's look from `/DA`, `/Contents`, `/Q`, `/Rect` less `/RD`.
fn free_text<G: ObjectGraph + ?Sized>(
    graph: &G,
    dict: &Dict,
    mode: FreeTextC,
) -> Result<DerivedAppearance, NotDerived> {
    if let Some(why) = free_text_decline(graph, dict) {
        return Err(why);
    }
    let mut spec =
        super::text_spec_from_dict(graph, dict).map_err(|e| NotDerived::new(e.to_string()))?;
    let mut notes = Vec::new();
    let has_ic = !matches!(super::color_entry(graph, dict, b"IC"), ColorEntry::Absent);
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
        if let Some(inner) = super::rect_inside_differences(graph, dict) {
            *rect = inner;
        }
        // The inner rectangle is "where the text should be displayed": wrap.
        *multiline = true;
        let da = string_bytes(graph, dict, b"DA").unwrap_or_default();
        let (std, substituted) = resolve_font(graph, &da);
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
    let annot = super::build_text_annotation(&spec).map_err(|e| NotDerived::new(e.to_string()))?;
    if let Some(size) = annot.applied_autosize {
        notes.push(format!("auto-size (0 Tf) chose {size:.1} pt"));
    }
    if annot.unencodable_chars > 0 {
        notes.push(format!(
            "{} character(s) outside WinAnsi drawn as ?",
            annot.unencodable_chars
        ));
    }
    Ok(DerivedAppearance {
        ap_dict: annot.ap_dict,
        ap_content: annot.ap_content,
        rect: annot.rect,
        notes,
    })
}
