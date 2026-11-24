//! Page-level PDF/A rows: device colour against the output intent (ISO
//! 19005-1 §6.2.3.3, -2 §6.2.4.3), annotation colour at 1b (-1 §6.5.3),
//! transparency groups (-2 §6.2.10), page boundaries (-2 §6.1.13),
//! annotations and widgets (-1 §6.5, §6.6, §6.9, -2 §6.3 to §6.5) and
//! presentation steps (-2 §6.10).

use crate::graph::ObjectGraph;
use crate::object::{Dict, ObjId, Object};
use crate::page_tree::{BoxResolution, Rect};
use crate::preflight::checks::Sink;
use crate::preflight::cos::name;
use crate::preflight::walk::Event;
use crate::preflight::{ColourFamily, Finding, FindingTarget};

use super::doc_rows::{Intent, pdfa_intent};
use super::{Ctx, PageCtx};

/// Annotation subtypes ISO 19005-1 §6.5.2 permits.
const SUBTYPES_1: [&[u8]; 16] = [
    b"Text",
    b"Link",
    b"FreeText",
    b"Line",
    b"Square",
    b"Circle",
    b"Highlight",
    b"Underline",
    b"Squiggly",
    b"StrikeOut",
    b"Stamp",
    b"Ink",
    b"Popup",
    b"Widget",
    b"PrinterMark",
    b"TrapNet",
];

/// Further subtypes ISO 19005-2 §6.3.1 permits.
const SUBTYPES_2: [&[u8]; 6] = [
    b"Polygon",
    b"PolyLine",
    b"Caret",
    b"FileAttachment",
    b"Watermark",
    b"Redact",
];

/// Annotation flag bits (ISO 32000-1 Table 165).
const INVISIBLE: i64 = 1;
const HIDDEN: i64 = 2;
const PRINT: i64 = 4;
const NO_VIEW: i64 = 32;
const TOGGLE_NO_VIEW: i64 = 256;

/// Inheritance followed up a field's `/Parent` chain.
const MAX_PARENTS: usize = 32;

/// The per-page rows.
pub(super) fn check(ctx: &Ctx<'_>, p: &PageCtx<'_, '_>, sink: &mut Sink) {
    let view = p.view;
    let page_dict = view.graph().value(p.page.id).and_then(Object::as_dict);
    let on_page = |m: &str| ctx.found(Some(p.number), FindingTarget::Page, m);
    match ctx.row.id {
        "PAI-CS-4" | "PAI-CS-5" | "PAI-CS-6" => device(ctx, p, sink),
        "PAI-FS-44" => boxes(ctx, p, sink),
        "PAI-TG-8" => group(ctx, p, page_dict, sink),
        "PAI-IA-27" if page_dict.is_some_and(|d| d.contains_key(b"AA")) => {
            sink.push(on_page(
                "The page has an additional-actions dictionary (/AA).",
            ));
        }
        "PAI-EO-21" if page_dict.is_some_and(|d| d.contains_key(b"PresSteps")) => {
            sink.push(on_page("The page has presentation steps (/PresSteps)."));
        }
        "PAI-IA-27" | "PAI-EO-21" => {}
        _ => {
            for (id, annot) in annotations(p, page_dict) {
                if let Some(what) = annotation(ctx, p, &annot) {
                    let subtype = name(view, &annot, b"Subtype").map_or("untyped".into(), |s| {
                        format!("/{}", String::from_utf8_lossy(s))
                    });
                    let target = id.map_or(FindingTarget::Page, FindingTarget::Annotation);
                    let m = format!("A {subtype} annotation {what}.");
                    sink.push(ctx.found(Some(p.number), target, &m));
                }
            }
        }
    }
}

/// The page's annotation dictionaries, each with its id when indirect.
fn annotations(p: &PageCtx<'_, '_>, page: Option<&Dict>) -> Vec<(Option<ObjId>, Dict)> {
    let annots = page
        .and_then(|d| d.get(b"Annots"))
        .map(|o| p.view.resolve(o))
        .and_then(Object::as_array)
        .unwrap_or_default();
    annots
        .iter()
        .filter_map(|o| {
            let d = p.view.resolve(o).as_dict()?;
            Some((o.as_reference(), d.clone()))
        })
        .collect()
}

/// CS-4 (RGB), CS-5 (CMYK), CS-6 (Gray): device colour needs a PDF/A
/// output intent of the matching space, or at 2b/3b a page `Default*`.
fn device(ctx: &Ctx<'_>, p: &PageCtx<'_, '_>, sink: &mut Sink) {
    let (family, label, default, space): (_, _, &[u8], Option<&[u8; 4]>) = match ctx.row.id {
        "PAI-CS-4" => (
            ColourFamily::DeviceRgb,
            "DeviceRGB",
            b"DefaultRGB",
            Some(b"RGB "),
        ),
        "PAI-CS-5" => (
            ColourFamily::DeviceCmyk,
            "DeviceCMYK",
            b"DefaultCMYK",
            Some(b"CMYK"),
        ),
        _ => (ColourFamily::DeviceGray, "DeviceGray", b"DefaultGray", None),
    };
    if let Some(why) = p.events.iter().find_map(|(_, e)| match e {
        Event::Unwalked(why) => Some(why),
        _ => None,
    }) {
        sink.push(ctx.unread(Some(p.number), why.trim_end_matches('.')));
    }
    let used = p
        .events
        .iter()
        .any(|(_, e)| matches!(e, Event::Colour(u) if u.family == family));
    if !used {
        return;
    }
    let intent = pdfa_intent(p.view);
    let satisfied = match (intent, space) {
        (Intent::Absent, _) => false,
        (_, None) | (Intent::Unread, _) => true,
        (Intent::Space(s), Some(want)) => &s == want,
    };
    if intent == Intent::Unread && space.is_some() {
        sink.push(ctx.unread(Some(p.number), "the PDF/A output profile could not be read"));
        return;
    }
    let defaulted = page_default(p, default);
    if satisfied || (defaulted && ctx.level.part() > 1) {
        return;
    }
    if defaulted {
        let m = format!(
            "whether the page's /{} satisfies the requirement at PDF/A-1 is NOT SOURCED",
            String::from_utf8_lossy(default)
        );
        sink.push(ctx.coverage(Some(p.number), &m));
    }
    let need = space.map_or("a PDF/A output intent".to_owned(), |s| {
        format!(
            "a PDF/A output intent with a {} profile",
            String::from_utf8_lossy(s).trim_end()
        )
    });
    let m = format!("The page paints in {label} without {need}.");
    sink.push(ctx.found(Some(p.number), FindingTarget::Page, &m));
}

/// Whether the page's resources name `/Default…` in `/ColorSpace`.
fn page_default(p: &PageCtx<'_, '_>, key: &[u8]) -> bool {
    p.page
        .resources
        .get(b"ColorSpace")
        .map(|o| p.view.resolve(o))
        .and_then(Object::as_dict)
        .is_some_and(|cs| cs.contains_key(key))
}

/// FS-44: every page boundary written is 3 to 14400 units each way.
fn boxes(ctx: &Ctx<'_>, p: &PageCtx<'_, '_>, sink: &mut Sink) {
    let page = p.page;
    let all = [
        ("MediaBox", page.media_box, BoxResolution::AsWritten),
        ("CropBox", page.crop_box, page.crop_box_resolution),
        ("BleedBox", page.bleed_box, page.bleed_box_resolution),
        ("TrimBox", page.trim_box, page.trim_box_resolution),
        ("ArtBox", page.art_box, page.art_box_resolution),
    ];
    for (label, rect, resolution) in all {
        if resolution == BoxResolution::Defaulted || within(rect) {
            continue;
        }
        let m = format!(
            "The page's {label} is {:.1} by {:.1} units; each side must be 3 to 14400.",
            rect.urx - rect.llx,
            rect.ury - rect.lly
        );
        sink.push(Finding {
            rect: Some(rect),
            ..ctx.found(Some(p.number), FindingTarget::Page, &m)
        });
    }
}

fn within(r: Rect) -> bool {
    let ok = |side: f64| (3.0..=14400.0).contains(&side);
    ok(r.urx - r.llx) && ok(r.ury - r.lly)
}

/// TG-8: a page that uses transparency, in a file with no PDF/A output
/// intent, has a `/Group` with `/CS`.
fn group(ctx: &Ctx<'_>, p: &PageCtx<'_, '_>, page: Option<&Dict>, sink: &mut Sink) {
    if pdfa_intent(p.view) != Intent::Absent {
        return;
    }
    let used = crate::transparency::scan_page(p.view.graph(), p.page);
    if used.is_empty() {
        return;
    }
    let cs = page
        .and_then(|d| d.get(b"Group"))
        .map(|o| p.view.resolve(o))
        .and_then(Object::as_dict)
        .is_some_and(|g| g.contains_key(b"CS"));
    if !cs {
        let m = "The page uses transparency, the file has no PDF/A output intent, and the \
                 page's /Group has no /CS.";
        sink.push(ctx.found(Some(p.number), FindingTarget::Page, m));
    }
}

/// What one annotation breaks for the row, as a predicate phrase.
fn annotation(ctx: &Ctx<'_>, p: &PageCtx<'_, '_>, a: &Dict) -> Option<String> {
    let view = p.view;
    let subtype = name(view, a, b"Subtype").unwrap_or_default();
    let popup = subtype == b"Popup";
    let widget = subtype == b"Widget";
    let flags = a
        .get(b"F")
        .map(|o| view.resolve(o))
        .and_then(Object::as_int);
    let f = flags.unwrap_or(0);
    let bad_flags = f & PRINT == 0 || f & (HIDDEN | INVISIBLE | NO_VIEW) != 0;
    match ctx.row.id {
        "PAI-IA-1" | "PAI-IA-2" => {
            let ok = SUBTYPES_1.contains(&subtype)
                || (ctx.row.id == "PAI-IA-2" && SUBTYPES_2.contains(&subtype));
            (!ok).then(|| format!("is not permitted in {}", ctx.level.claim()))
        }
        "PAI-IA-3" => {
            let ca = a
                .get(b"CA")
                .map(|o| view.resolve(o))
                .and_then(Object::as_number)?;
            #[allow(clippy::float_cmp)] // the requirement is exactly 1.0
            let opaque = ca == 1.0;
            (!opaque).then(|| format!("has /CA {ca}"))
        }
        "PAI-IA-4" => match flags {
            None => Some("has no /F".into()),
            Some(_) if bad_flags => Some(format!(
                "has /F {f}: Print clear or Hidden, Invisible or NoView set"
            )),
            Some(_) => None,
        },
        "PAI-IA-5" => (flags.is_none() && !popup).then(|| "has no /F".into()),
        "PAI-IA-6" => (flags.is_some() && (bad_flags || f & TOGGLE_NO_VIEW != 0)).then(|| {
            format!("has /F {f}: Print clear or Hidden, Invisible, NoView or ToggleNoView set")
        }),
        "PAI-CS-10" => colour(view, a),
        _ => appearance(ctx, p, a, subtype, widget),
    }
}

/// CS-10 (1b): `/C` or `/IC` colour needs an RGB PDF/A output intent.
fn colour(view: &crate::view::DocumentView<'_>, a: &Dict) -> Option<String> {
    let key = [b"C".as_slice(), b"IC"].into_iter().find(|k| {
        a.get(k)
            .map(|o| view.resolve(o))
            .and_then(Object::as_array)
            .is_some_and(|c| !c.is_empty())
    })?;
    (pdfa_intent(view) != Intent::Space(*b"RGB ")).then(|| {
        format!(
            "has a /{} colour but the file has no PDF/A output intent with an RGB profile",
            String::from_utf8_lossy(key)
        )
    })
}

/// IA-7 to IA-10, IA-12, IA-23, IA-24: appearances and widget actions.
fn appearance(
    ctx: &Ctx<'_>,
    p: &PageCtx<'_, '_>,
    a: &Dict,
    subtype: &[u8],
    widget: bool,
) -> Option<String> {
    let view = p.view;
    let ap = a
        .get(b"AP")
        .map(|o| view.resolve(o))
        .and_then(Object::as_dict);
    let normal = ap.and_then(|d| d.get(b"N")).map(|o| view.resolve(o));
    match ctx.row.id {
        "PAI-IA-7" => {
            let exempt = matches!(subtype, b"Popup" | b"Link") || zero_size(view, a);
            (ap.is_none() && !exempt).then(|| "has no appearance dictionary (/AP)".into())
        }
        "PAI-IA-8" => ap
            .and_then(|d| d.iter().map(|(k, _)| k.as_bytes()).find(|k| *k != b"N"))
            .map(|k| format!("has an /AP with /{}", String::from_utf8_lossy(k))),
        "PAI-IA-9" | "PAI-IA-10" => {
            let button = widget && field_type(view, a) == Some(b"Btn".to_vec());
            let normal = normal?;
            match (ctx.row.id, button, normal) {
                ("PAI-IA-9", true, Object::Stream(_)) => {
                    Some("is a button whose /AP /N is a stream, not a state dictionary".into())
                }
                ("PAI-IA-10", false, Object::Dict(_)) => {
                    Some("has an /AP /N that is not a stream".into())
                }
                _ => None,
            }
        }
        "PAI-IA-12" => {
            (widget && ap.is_none()).then(|| "has no appearance dictionary (/AP)".into())
        }
        "PAI-IA-23" => (widget && a.contains_key(b"A")).then(|| "has an action (/A)".into()),
        "PAI-IA-24" => {
            let action = ctx.level.part() > 1 && a.contains_key(b"A");
            (widget && (a.contains_key(b"AA") || action))
                .then(|| "has an action (/A or /AA)".into())
        }
        _ => None,
    }
}

/// Whether `/Rect` has zero width and height.
fn zero_size(view: &crate::view::DocumentView<'_>, a: &Dict) -> bool {
    let Some(r) = a
        .get(b"Rect")
        .map(|o| view.resolve(o))
        .and_then(Object::as_array)
    else {
        return false;
    };
    let n: Vec<f64> = r
        .iter()
        .filter_map(|o| view.resolve(o).as_number())
        .collect();
    #[allow(clippy::float_cmp)] // a zero-size rectangle is exactly degenerate
    let zero = matches!(n.as_slice(), [x0, y0, x1, y1] if x0 == x1 && y0 == y1);
    zero
}

/// A widget's `/FT`, inherited up its `/Parent` chain.
fn field_type(view: &crate::view::DocumentView<'_>, a: &Dict) -> Option<Vec<u8>> {
    let mut node = a.clone();
    for _ in 0..MAX_PARENTS {
        if let Some(ft) = name(view, &node, b"FT") {
            return Some(ft.to_vec());
        }
        node = node
            .get(b"Parent")
            .map(|o| view.resolve(o))
            .and_then(Object::as_dict)?
            .clone();
    }
    None
}
