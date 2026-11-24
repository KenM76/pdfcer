//! Document-level PDF/X rows read from the trailer, the Info dictionary,
//! the catalog and the output intents: identification (ISO 15930-1 §5,
//! 15930-4 §6.7, 15930-7 §6.10–§6.11), output intent (15930-3 §6.2.2,
//! 15930-7 §6.4), trapping (15930-4 §6.6, 15930-7 §6.9), encryption
//! (15930-4 §6.11, 15930-7 §6.15), fonts and the person-only rows.

use crate::graph::ObjectGraph;
use crate::object::{Dict, Object};
use crate::preflight::FindingTarget;
use crate::preflight::checks::{self, Sink};
use crate::prepress::{InkKind, inks, pdfx_claim};
use crate::view::DocumentView;

use super::{Ctx, PdfxLevel};

/// Info keys ISO 32000-1 Table 317 defines; any other key is private.
const STANDARD_INFO: [&[u8]; 9] = [
    b"Title",
    b"Author",
    b"Subject",
    b"Keywords",
    b"Creator",
    b"Producer",
    b"CreationDate",
    b"ModDate",
    b"Trapped",
];

/// The rows decided once per document: Info, XMP, output intents, spot alternates, fonts, encryption, trapping.
pub(super) fn check(view: &DocumentView<'_>, ctx: &Ctx<'_>, sink: &mut Sink) {
    match ctx.row.id {
        "PXQ-ID-1" => identification(view, ctx, sink),
        "PXQ-ID-3" => xmp_claim(view, ctx, sink),
        "PXQ-ID-4" => carriers(view, ctx, sink),
        "PXQ-ID-6" => {
            for key in ["Title", "CreationDate", "ModDate"] {
                if info(view)
                    .and_then(|d| text(view, d, key.as_bytes()))
                    .is_none()
                {
                    sink.push(ctx.doc(&format!("The Info dictionary has no {key} text string.")));
                }
            }
        }
        "PXQ-ID-8" => private_info(view, ctx, sink),
        "PXQ-ID-9" => {
            if info(view)
                .and_then(|d| text(view, d, b"GTS_PDFXVersion"))
                .as_deref()
                == Some("CGATS.12/1-1999")
            {
                sink.push(ctx.doc(
                    "The file claims CGATS.12/1-1999, the predecessor of PDF/X-1a:2001, \
                     not PDF/X-1a:2001 itself.",
                ));
            }
        }
        "PXQ-OI-1" | "PXQ-OI-2" | "PXQ-OI-3" | "PXQ-OI-7" | "PXQ-OI-8" => {
            output_intent(view, ctx, sink);
        }
        "PXQ-CS-2" => sink.push(ctx.person(
            None,
            FindingTarget::Document,
            "Whether every colour value was prepared for the output intent's printing \
             condition needs a person.",
        )),
        "PXQ-ST-7" => sink.push(ctx.person(
            None,
            FindingTarget::Document,
            "Whether anything outside the PDF/X rules changes the printed result needs a person.",
        )),
        "PXQ-CS-8" => spot_alternates(view, ctx, sink),
        "PXQ-EN-1" if view.trailer_entry(b"Encrypt").is_some() => {
            sink.push(ctx.doc("The file is encrypted."));
        }
        "PXQ-TP-1" => trapped(view, ctx, sink),
        "PXQ-FT-1" => ctx.relay(ctx.rule, sink, |r, s| checks::fonts(view, r, s)),
        _ => {}
    }
}

/// The document catalog, if the trailer's `/Root` resolves to a dictionary.
pub(super) fn catalog<'v>(view: &'v DocumentView<'_>) -> Option<&'v Dict> {
    view.trailer_entry(b"Root")
        .map(|o| view.resolve(o))
        .and_then(Object::as_dict)
}

fn info<'v>(view: &'v DocumentView<'_>) -> Option<&'v Dict> {
    view.trailer_entry(b"Info")
        .map(|o| view.resolve(o))
        .and_then(Object::as_dict)
}

fn text(view: &DocumentView<'_>, dict: &Dict, key: &[u8]) -> Option<String> {
    match view.resolve(dict.get(key)?) {
        Object::String(s) => Some(crate::textstring::decode_text_string(s).text),
        _ => None,
    }
}

/// `dict[key]` as a name, following one indirect reference.
pub(super) fn name<'v>(view: &'v DocumentView<'_>, dict: &'v Dict, key: &[u8]) -> Option<&'v [u8]> {
    dict.get(key)
        .map(|o| view.resolve(o))
        .and_then(Object::as_name)
        .map(|n| n.as_bytes())
}

/// Every `/OutputIntents` entry.
fn intents<'v>(view: &'v DocumentView<'_>) -> Vec<&'v Dict> {
    catalog(view)
        .and_then(|c| c.get(b"OutputIntents"))
        .map(|o| view.resolve(o))
        .and_then(Object::as_array)
        .unwrap_or_default()
        .iter()
        .filter_map(|i| view.resolve(i).as_dict())
        .collect()
}

/// The `/S /GTS_PDFX` output intents.
pub(super) fn gts_intents<'v>(view: &'v DocumentView<'_>) -> Vec<&'v Dict> {
    intents(view)
        .into_iter()
        .filter(|d| name(view, d, b"S") == Some(b"GTS_PDFX"))
        .collect()
}

fn identification(view: &DocumentView<'_>, ctx: &Ctx<'_>, sink: &mut Sink) {
    let (want, conformance) = match ctx.level {
        PdfxLevel::X1a2001 => ("PDF/X-1:2001", Some("PDF/X-1a:2001")),
        level => (level.claim(), None),
    };
    let claim = ctx.level.claim();
    let entries = [
        ("GTS_PDFXVersion", Some(want)),
        ("GTS_PDFXConformance", conformance),
    ];
    for (key, want) in entries {
        let Some(want) = want else {
            continue;
        };
        match info(view).and_then(|d| text(view, d, key.as_bytes())) {
            None => sink.push(ctx.doc(&format!(
                "The Info dictionary has no {key} text string; {claim} requires \"{want}\"."
            ))),
            Some(v) if v.trim() != want => sink.push(ctx.doc(&format!(
                "The Info {key} is \"{v}\"; {claim} requires \"{want}\"."
            ))),
            Some(_) => {}
        }
    }
}

fn xmp_claim(view: &DocumentView<'_>, ctx: &Ctx<'_>, sink: &mut Sink) {
    let has_metadata = catalog(view)
        .and_then(|c| c.get(b"Metadata"))
        .is_some_and(|o| matches!(view.resolve(o), Object::Stream(_)));
    if !has_metadata {
        sink.push(ctx.doc("The catalog has no XMP metadata stream."));
        return;
    }
    match pdfx_claim(view).and_then(|c| c.xmp_version) {
        Some(v) if v == "PDF/X-4" => {}
        Some(v) => sink.push(ctx.doc(&format!(
            "The XMP pdfxid:GTS_PDFXVersion is \"{v}\", not \"PDF/X-4\"."
        ))),
        None => sink
            .push(ctx.doc("The XMP metadata does not state pdfxid:GTS_PDFXVersion \"PDF/X-4\".")),
    }
}

fn carriers(view: &DocumentView<'_>, ctx: &Ctx<'_>, sink: &mut Sink) {
    if let Some(c) = pdfx_claim(view).filter(|c| !c.carriers_agree()) {
        sink.push(ctx.doc(&format!(
            "The Info GTS_PDFXVersion \"{}\" and the XMP value \"{}\" disagree.",
            c.info_version.unwrap_or_default(),
            c.xmp_version.unwrap_or_default()
        )));
    }
    sink.push(ctx.coverage(
        None,
        "only GTS_PDFXVersion is compared; other Info entries are not compared with \
         their XMP properties",
    ));
}

fn private_info(view: &DocumentView<'_>, ctx: &Ctx<'_>, sink: &mut Sink) {
    let Some(info) = info(view) else {
        return;
    };
    for (key, value) in info.iter() {
        let key = key.as_bytes();
        if STANDARD_INFO.contains(&key) || matches!(view.resolve(value), Object::String(_)) {
            continue;
        }
        sink.push(ctx.doc(&format!(
            "The private Info entry /{} is not a text string.",
            String::from_utf8_lossy(key)
        )));
    }
}

fn output_intent(view: &DocumentView<'_>, ctx: &Ctx<'_>, sink: &mut Sink) {
    let gts = gts_intents(view);
    match ctx.row.id {
        "PXQ-OI-1" if gts.is_empty() => {
            let others: Vec<String> = intents(view)
                .iter()
                .map(|d| {
                    name(view, d, b"S").map_or("an unnamed kind".into(), |n| {
                        format!("/{}", String::from_utf8_lossy(n))
                    })
                })
                .collect();
            let message = if others.is_empty() {
                "The document has no GTS_PDFX output intent.".to_owned()
            } else {
                format!(
                    "The output intents are of kind {}; none is GTS_PDFX.",
                    others.join(", ")
                )
            };
            sink.push(ctx.doc(&message));
        }
        "PXQ-OI-2" if gts.len() > 1 => sink.push(ctx.doc(&format!(
            "The document has {} GTS_PDFX output intents; exactly one is allowed.",
            gts.len()
        ))),
        "PXQ-OI-3" => {
            for _ in gts
                .iter()
                .filter(|d| text(view, d, b"OutputConditionIdentifier").is_none())
            {
                sink.push(
                    ctx.doc(
                        "A GTS_PDFX output intent has no OutputConditionIdentifier text string.",
                    ),
                );
            }
        }
        "PXQ-OI-7" => {
            for d in gts.iter().filter(|d| profile(view, d).is_none()) {
                let what = text(view, d, b"OutputConditionIdentifier").unwrap_or_default();
                sink.push(ctx.doc(&format!(
                    "The GTS_PDFX output intent \"{what}\" embeds no DestOutputProfile."
                )));
            }
        }
        "PXQ-OI-8" => {
            for d in &gts {
                profile_class(view, ctx, d, sink);
            }
        }
        _ => {}
    }
}

/// The intent's `/DestOutputProfile` stream.
pub(super) fn profile<'v>(
    view: &'v DocumentView<'_>,
    intent: &'v Dict,
) -> Option<&'v crate::object::Stream> {
    match view.resolve(intent.get(b"DestOutputProfile")?) {
        Object::Stream(s) => Some(s),
        _ => None,
    }
}

/// The ICC header's profile/device class (bytes 12..16) is `prtr`.
fn profile_class(view: &DocumentView<'_>, ctx: &Ctx<'_>, intent: &Dict, sink: &mut Sink) {
    let Some(stream) = profile(view, intent) else {
        return;
    };
    let decoded = view
        .slice(stream.data_span)
        .and_then(|raw| crate::filters::decode_stream(&stream.dict, raw).ok());
    match decoded.as_deref().and_then(|b| b.get(12..16)) {
        None => sink.push(ctx.unread(None, "the DestOutputProfile could not be read")),
        Some(b"prtr") => {}
        Some(class) => sink.push(ctx.doc(&format!(
            "The DestOutputProfile is a '{}' profile, not an output-device (prtr) profile.",
            String::from_utf8_lossy(class)
        ))),
    }
}

fn spot_alternates(view: &DocumentView<'_>, ctx: &Ctx<'_>, sink: &mut Sink) {
    for ink in inks(view) {
        if ink.kind == InkKind::Spot && ink.alternates.len() > 1 {
            sink.push(ctx.doc(&format!(
                "The spot ink \"{}\" is defined with different alternate spaces ({}).",
                ink.name,
                ink.alternates.join(", ")
            )));
        }
    }
    sink.push(ctx.coverage(
        None,
        "alternate spaces are compared by family; tint transforms are not compared",
    ));
}

fn trapped(view: &DocumentView<'_>, ctx: &Ctx<'_>, sink: &mut Sink) {
    let value = info(view)
        .and_then(|d| d.get(b"Trapped"))
        .map(|o| view.resolve(o));
    match value {
        Some(Object::Name(n)) if matches!(n.as_bytes(), b"True" | b"False") => {}
        Some(Object::Name(n)) => sink.push(ctx.doc(&format!(
            "The Info Trapped is /{}; it must be /True or /False.",
            String::from_utf8_lossy(n.as_bytes())
        ))),
        Some(_) => {
            sink.push(ctx.doc("The Info Trapped is not a name; it must be /True or /False."))
        }
        None if ctx.level == PdfxLevel::X4 => sink.push(ctx.coverage(
            None,
            "the Info dictionary has no Trapped entry; whether PDF/X-4 accepts the XMP \
             pdf:Trapped property in its place is NOT SOURCED",
        )),
        None => sink.push(ctx.doc("The Info dictionary has no Trapped entry.")),
    }
}
