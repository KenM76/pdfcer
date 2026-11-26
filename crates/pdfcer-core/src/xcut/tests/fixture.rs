//! One fixture per annotation-family kind: a two-page document with an
//! AcroForm and an optional-content group, whose page 1 holds the target
//! annotation, a popup and a note the target replies to.
//!
//! The target carries non-default values for every common key (ISO 32000-1
//! §12.5.2 Table 164, §12.5.6.2 Table 170) plus the sentinel: a third-class
//! key (Annex E `XX` prefix) holding a string, and a second one holding a
//! stream reference, so a carry that rebuilds the dictionary from a list of
//! keys it knows loses them.

use crate::dimension::{DEFAULT_GROUP_ID, DimensionKind};
use crate::edit::EditSession;
use crate::object::{Name, ObjId, Object};
use crate::pageops::tests_support::{build_pdf, stream};
use crate::vector::{AxisConstraint, Point};
use crate::xcut::ObjectKind;

/// The sentinel string; it must survive every `Preserve` carry.
pub(super) const SENTINEL: &str = "xcut-sentinel-7810";
pub(super) const SENTINEL_KEY: &[u8] = b"XXpdfcerSentinel";
/// The sentinel stream's decoded data.
pub(super) const SENTINEL_DATA: &str = "xcut-sentinel-stream-7810";

/// Object numbers the fixture fixes.
pub(super) const TARGET: u32 = 10;
const POPUP: u32 = 11;
const NOTE: u32 = 12;

/// The common keys, written onto every target but the ce dimension (which
/// authors its own `/Rect`, `/AP` and `/Subtype`).
const COMMON: &str = "/F 28 /C [1 0 0] /Border [0 0 2] /CA 0.5 /T (Ken) /Subj (Review) \
    /Contents (hello) /M (D:20260101000000Z) /CreationDate (D:20251231000000Z) \
    /NM (target-1) /StructParent 7 /OC 9 0 R /Lang (en-CA) /IRT 12 0 R \
    /XXpdfcerSentinel (xcut-sentinel-7810) /XXpdfcerSentinelStream 14 0 R";

/// Every kind this fixture builds (the annotation and form-widget rows).
pub(super) fn kinds() -> impl Iterator<Item = ObjectKind> {
    ObjectKind::ALL.iter().copied().filter(|k| {
        matches!(
            k.group(),
            crate::xcut::KindGroup::Annotation | crate::xcut::KindGroup::Form
        ) && *k != ObjectKind::Field
    })
}

/// The open session and the target annotation's id.
pub(super) fn build(kind: ObjectKind) -> (EditSession, ObjId) {
    if kind == ObjectKind::CeDimension {
        return ce_dimension();
    }
    let (subtype, specific) = specific(kind);
    let popup = if kind == ObjectKind::Popup {
        String::new()
    } else {
        format!("/Popup {POPUP} 0 R")
    };
    let target = format!(
        "<< /Type /Annot /Subtype /{subtype} /Rect [100 100 200 150] /P 3 0 R \
         /AP << /N 13 0 R >> {COMMON} {popup} {specific} >>"
    );
    (
        EditSession::new(pdf(&objects(&target, TARGET))),
        ObjId::new(TARGET, 0),
    )
}

/// A Screen annotation whose rendition action names itself through `/AN`
/// (§12.6.4.13, the conforming self-play shape).
pub(super) fn self_playing_screen() -> (EditSession, ObjId) {
    let target = format!(
        "<< /Type /Annot /Subtype /Screen /Rect [100 100 200 150] /P 3 0 R          /A << /S /Rendition /OP 0 /AN {TARGET} 0 R >> >>"
    );
    (
        EditSession::new(pdf(&objects(&target, TARGET))),
        ObjId::new(TARGET, 0),
    )
}

fn objects(target: &str, parent: u32) -> Vec<(u32, String)> {
    let ap = stream(
        "/Type /XObject /Subtype /Form /BBox [0 0 100 50]",
        "0 0 1 rg 0 0 100 50 re f",
    );
    vec![
        (
            1,
            "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [10 0 R] >> \
             /OCProperties << /OCGs [9 0 R] /D << /ON [9 0 R] >> >> >>"
                .to_owned(),
        ),
        (
            2,
            "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>".to_owned(),
        ),
        (
            3,
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 5 0 R \
                 /Annots [{}11 0 R 12 0 R] >>",
                if target.is_empty() { "" } else { "10 0 R " }
            ),
        ),
        (
            4,
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Annots [] >>".to_owned(),
        ),
        (5, stream("", "0 0 m 10 10 l S")),
        (9, "<< /Type /OCG /Name (Review layer) >>".to_owned()),
        (TARGET, target.to_owned()),
        (
            POPUP,
            format!(
                "<< /Type /Annot /Subtype /Popup /Rect [210 100 310 150] /P 3 0 R \
                 /Parent {parent} 0 R /Open false >>"
            ),
        ),
        (
            NOTE,
            "<< /Type /Annot /Subtype /Text /Rect [300 300 320 320] /P 3 0 R /NM (note-1) >>"
                .to_owned(),
        ),
        (13, ap),
        (14, stream("/XXpdfcerSentinelKind /Probe", SENTINEL_DATA)),
        (
            15,
            "<< /Type /Filespec /F (a.txt) /UF (a.txt) /EF << /F 16 0 R >> >>".to_owned(),
        ),
        (
            16,
            stream("/Type /EmbeddedFile /Subtype /text#2Fplain", "attached"),
        ),
        (
            17,
            stream("/Type /Sound /R 8000 /C 1 /B 8 /E /Raw", "abcdefgh"),
        ),
        (18, stream("/Type /3D /Subtype /U3D", "U3D-probe")),
    ]
    .into_iter()
    .filter(|(n, body)| *n != TARGET || !body.is_empty())
    .collect()
}

fn pdf(objects: &[(u32, String)]) -> crate::document::Document {
    let borrowed: Vec<(u32, &str)> = objects.iter().map(|(n, b)| (*n, b.as_str())).collect();
    build_pdf(&borrowed)
}

/// The subtype and the subtype-specific keys for `kind`.
fn specific(kind: ObjectKind) -> (&'static str, &'static str) {
    use ObjectKind as K;
    const QUADS: &str = "/QuadPoints [100 150 200 150 100 100 200 100]";
    match kind {
        K::TextNote => ("Text", "/Name /Comment /Open true"),
        K::FreeText => (
            "FreeText",
            "/DA (/Helv 12 Tf 0 g) /Q 1 /DS (font: 12pt Helvetica)",
        ),
        K::Callout => (
            "FreeText",
            "/IT /FreeTextCallout /DA (/Helv 12 Tf 0 g) /CL [100 100 120 120 150 120] \
             /LE /OpenArrow /RD [5 5 5 5]",
        ),
        K::Line => (
            "Line",
            "/IT /LineArrow /L [100 100 200 150] /LE [/None /OpenArrow]",
        ),
        K::Square => ("Square", "/IC [0 0 1] /RD [1 1 1 1]"),
        K::Circle => ("Circle", "/IC [0 1 0] /RD [1 1 1 1]"),
        K::Polygon => (
            "Polygon",
            "/Vertices [100 100 200 100 150 150] /BE << /S /C /I 1 >>",
        ),
        K::PolyLine => ("PolyLine", "/Vertices [100 100 150 150 200 100]"),
        K::Highlight => ("Highlight", QUADS),
        K::Underline => ("Underline", QUADS),
        K::StrikeOut => ("StrikeOut", QUADS),
        K::Squiggly => ("Squiggly", QUADS),
        K::Caret => ("Caret", "/Sy /P /RD [0 0 0 0]"),
        K::ReplaceText => (
            "StrikeOut",
            "/IT /StrikeOutTextEdit /QuadPoints [100 150 200 150 100 100 200 100]",
        ),
        K::Stamp => ("Stamp", "/Name /Approved"),
        K::Ink => ("Ink", "/InkList [[100 100 150 150 200 120]]"),
        K::Popup => ("Popup", "/Parent 12 0 R /Open false"),
        K::FileAttachment => ("FileAttachment", "/FS 15 0 R /Name /PushPin"),
        K::Sound => ("Sound", "/Sound 17 0 R /Name /Speaker"),
        K::Screen => ("Screen", "/MK << /R 0 >>"),
        K::ThreeD => ("3D", "/3DD 18 0 R /3DA << /A /PO >>"),
        K::Link => ("Link", "/A << /S /URI /URI (https://example.org/) >> /H /I"),
        K::Redact => (
            "Redact",
            "/OverlayText (X) /IC [0 0 0] /QuadPoints [100 150 200 150 100 100 200 100]",
        ),
        K::Watermark => (
            "Watermark",
            "/FixedPrint << /Type /FixedPrint /Matrix [1 0 0 1 0 0] /H 0 /V 0 >>",
        ),
        K::UnknownAnnot => ("XXpdfcerUnknown", "/XXpdfcerPayload [1 2 3]"),
        K::Widget => (
            "Widget",
            "/FT /Tx /V (value) /DA (/Helv 0 Tf 0 g) /MK << /BG [1 1 1] >>",
        ),
        K::CeDimension
        | K::Field
        | K::Path
        | K::TextObject
        | K::ImageXObject
        | K::InlineImage
        | K::FormXObject
        | K::Page
        | K::OutlineItem
        | K::Attachment
        | K::Layer
        | K::NamedDest => ("XXpdfcerNoFixture", ""),
    }
}

/// A real ce dimension authored by `add_dimension`, then given the common
/// keys and the sentinel the way a second session would have written them.
#[allow(clippy::unwrap_used)]
fn ce_dimension() -> (EditSession, ObjId) {
    let mut s = EditSession::new(pdf(&objects("", NOTE)));
    let kind = DimensionKind::Linear {
        a: Point::new(100.0, 100.0),
        b: Point::new(200.0, 100.0),
        constraint: AxisConstraint::Horizontal,
        offset: 20.0,
        text_along: 0.0,
        extension_gap: [None; 2],
    };
    let (id, _) = s.add_dimension(0, DEFAULT_GROUP_ID, kind).unwrap();
    let Some(Object::Dict(mut dict)) = s.value(id).cloned() else {
        panic!("dimension annotation");
    };
    for (key, value) in [
        (&b"NM"[..], Object::String(b"dim-1".to_vec())),
        (b"T", Object::String(b"Ken".to_vec())),
        (b"StructParent", Object::Integer(7)),
        (b"OC", Object::Reference(ObjId::new(9, 0))),
        (b"Popup", Object::Reference(ObjId::new(POPUP, 0))),
        (b"IRT", Object::Reference(ObjId::new(NOTE, 0))),
        (SENTINEL_KEY, Object::String(SENTINEL.as_bytes().to_vec())),
        (
            b"XXpdfcerSentinelStream",
            Object::Reference(ObjId::new(14, 0)),
        ),
    ] {
        dict.insert(Name(key.to_vec()), value);
    }
    s.xcut_put_object(id, Object::Dict(dict));
    let Some(Object::Dict(mut popup)) = s.value(ObjId::new(POPUP, 0)).cloned() else {
        panic!("popup");
    };
    popup.insert(Name::from(b"Parent"), Object::Reference(id));
    s.xcut_put_object(ObjId::new(POPUP, 0), Object::Dict(popup));
    (s, id)
}

/// A one-page destination document for a cross-document paste.
pub(super) fn destination() -> EditSession {
    EditSession::new(pdf(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>".to_owned()),
        (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned()),
        (
            3,
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] >>".to_owned(),
        ),
    ]))
}
