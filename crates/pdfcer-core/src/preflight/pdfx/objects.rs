//! Document-level PDF/X rows answered by looking at every live object:
//! compression (ISO 15930-4 §6.5, 15930-7 §6.8), transfer functions
//! (15930-4 §6.9, 15930-7 §6.13), actions and forms (15930-4 §6.14,
//! 15930-7 §6.18, §6.26), external content, OPI, reference and PostScript
//! XObjects, embedded files (15930-4 §6.4, §6.10, 15930-7 §6.7, §6.14),
//! alternate images (15930-4 §6.12), CIE colour without a destination
//! profile (15930-3 §6.2.2) and features newer than the level's PDF
//! version (15930-7 Introduction, Table 1).

use std::collections::BTreeSet;

use crate::PdfVersion;
use crate::graph::ObjectGraph;
use crate::object::{Dict, ObjId, Object};
use crate::preflight::FindingTarget;
use crate::preflight::checks::Sink;
use crate::prepress::{each_array, live_ids};
use crate::view::DocumentView;

use super::doc_rows::{catalog, gts_intents, name, profile};
use super::{Ctx, PdfxLevel};

/// Nesting followed inside one object.
const MAX_DEPTH: usize = 24;

/// One dictionary met in the walk.
struct Probe<'a, 'v> {
    view: &'a DocumentView<'v>,
    dict: &'a Dict,
    stream: bool,
    level: PdfxLevel,
}

impl Probe<'_, '_> {
    fn name(&self, key: &[u8]) -> Option<&[u8]> {
        name(self.view, self.dict, key)
    }

    fn is_image(&self) -> bool {
        self.stream && self.name(b"Subtype") == Some(b"Image")
    }

    /// `/Filter` and `/FFilter` names.
    fn filters(&self) -> Vec<&[u8]> {
        let mut out = Vec::new();
        for key in [b"Filter".as_slice(), b"FFilter"] {
            match self.dict.get(key).map(|o| self.view.resolve(o)) {
                Some(Object::Name(n)) => out.push(n.as_bytes()),
                Some(Object::Array(a)) => out.extend(
                    a.iter()
                        .filter_map(|o| self.view.resolve(o).as_name())
                        .map(|n| n.as_bytes()),
                ),
                _ => {}
            }
        }
        out
    }

    fn filtered(&self, filter: &[u8]) -> bool {
        self.filters().contains(&filter)
    }

    fn has(&self, key: &[u8]) -> bool {
        self.dict.get(key).is_some()
    }
}

/// What one dictionary contributes to a row.
enum Hit {
    Found(String),
    Person(String),
}

type Test = fn(&Probe<'_, '_>) -> Option<Hit>;

/// Run the row if it is one of this module's; `false` when it is not.
pub(super) fn check(view: &DocumentView<'_>, ctx: &Ctx<'_>, sink: &mut Sink) -> bool {
    let test: Test = match ctx.row.id {
        "PXQ-FL-1" => |p| p.filtered(b"LZWDecode").then(|| found("is LZW-compressed")),
        "PXQ-FL-2" => |p| {
            p.filtered(b"JBIG2Decode")
                .then(|| found("is JBIG2-compressed"))
        },
        "PXQ-FL-3" => |p| {
            p.filtered(b"JPXDecode").then(|| {
                Hit::Person("is JPEG 2000-compressed; whether that is lossy needs a person".into())
            })
        },
        "PXQ-FL-4" => |p| {
            (p.is_image() && p.filtered(b"DCTDecode")).then(|| {
                Hit::Person("is a JPEG image; whether it is continuous-tone needs a person".into())
            })
        },
        "PXQ-FL-5" => fl5,
        "PXQ-CS-9" => cs9,
        "PXQ-AJ-1" => aj1,
        "PXQ-AJ-3" => {
            |p| (p.name(b"Subtype") == Some(b"Widget")).then(|| found("is a form field widget"))
        }
        "PXQ-ST-1" => st1,
        "PXQ-ST-2" => |p| p.has(b"OPI").then(|| found("carries an OPI reference")),
        "PXQ-ST-3" => st3,
        "PXQ-ST-4" => |p| {
            (p.name(b"Subtype") == Some(b"PS") || p.name(b"Subtype2") == Some(b"PS"))
                .then(|| found("is a PostScript XObject"))
        },
        "PXQ-ST-5" => st5,
        "PXQ-ST-6" => |p| {
            (p.is_image() && p.has(b"Alternates")).then(|| {
                Hit::Person(
                    "has alternate images; whether they match the base image needs a person".into(),
                )
            })
        },
        "PXQ-ST-8" => st8,
        "PXQ-OI-6" => {
            cie_without_profile(view, ctx, sink);
            return true;
        }
        _ => return false,
    };
    run(view, ctx, test, sink);
    after(view, ctx, sink);
    true
}

fn found(what: &str) -> Hit {
    Hit::Found(what.to_owned())
}

fn run(view: &DocumentView<'_>, ctx: &Ctx<'_>, test: Test, sink: &mut Sink) {
    let mut seen = BTreeSet::new();
    each_dict(view, &mut |id, dict, stream| {
        let probe = Probe {
            view,
            dict,
            stream,
            level: ctx.level,
        };
        let Some(hit) = test(&probe) else {
            return;
        };
        if !seen.insert(id) {
            return;
        }
        let target = if probe.is_image() {
            FindingTarget::Image(id)
        } else {
            FindingTarget::Document
        };
        sink.push(match hit {
            Hit::Found(what) => ctx.found(None, target, &format!("Object {id} {what}.")),
            Hit::Person(what) => ctx.person(None, target, &format!("Object {id} {what}.")),
        });
    });
}

/// Row-specific findings and coverage notes after the walk.
fn after(view: &DocumentView<'_>, ctx: &Ctx<'_>, sink: &mut Sink) {
    match ctx.row.id {
        "PXQ-AJ-3" => {
            let fields = catalog(view)
                .and_then(|c| c.get(b"AcroForm"))
                .and_then(|o| view.resolve(o).as_dict())
                .and_then(|a| a.get(b"Fields"))
                .and_then(|o| view.resolve(o).as_array())
                .is_some_and(|f| !f.is_empty());
            if fields {
                sink.push(ctx.doc("The catalog's AcroForm lists form fields."));
            }
        }
        "PXQ-ST-4" => sink.push(ctx.coverage(
            None,
            "the PS operator inside content streams is not scanned",
        )),
        "PXQ-ST-8" if ctx.level.based_on() < (PdfVersion { major: 1, minor: 5 }) => {
            sink.push(ctx.coverage(
                None,
                "object streams and cross-reference streams (PDF 1.5) are not visible to \
                 this check",
            ));
        }
        _ => {}
    }
}

/// Every dictionary inside every live object, with the object's id and
/// whether the dictionary is a stream's.
fn each_dict(view: &DocumentView<'_>, f: &mut dyn FnMut(ObjId, &Dict, bool)) {
    for id in live_ids(view) {
        if let Some(value) = view.graph().value(id) {
            walk(id, value, 0, f);
        }
    }
}

fn walk(id: ObjId, obj: &Object, depth: usize, f: &mut dyn FnMut(ObjId, &Dict, bool)) {
    if depth > MAX_DEPTH {
        return;
    }
    let (dict, stream) = match obj {
        Object::Dict(d) => (d, false),
        Object::Stream(s) => (&s.dict, true),
        Object::Array(a) => {
            a.iter().for_each(|o| walk(id, o, depth + 1, f));
            return;
        }
        _ => return,
    };
    f(id, dict, stream);
    dict.iter().for_each(|(_, v)| walk(id, v, depth + 1, f));
}

fn fl5(p: &Probe<'_, '_>) -> Option<Hit> {
    if p.filtered(b"JPXDecode") {
        return Some(found("is JPEG 2000-compressed"));
    }
    let deep = p.dict.get(b"BitsPerComponent").and_then(Object::as_int) == Some(16);
    (p.is_image() && deep).then(|| found("is a 16-bit image"))
}

fn cs9(p: &Probe<'_, '_>) -> Option<Hit> {
    [b"TR".as_slice(), b"TR2"].into_iter().find_map(|key| {
        let value = p.dict.get(key).map(|o| p.view.resolve(o))?;
        let allowed = matches!(
            value.as_name().map(|n| n.as_bytes()),
            Some(b"Default" | b"Identity")
        );
        (!allowed).then(|| {
            found(&format!(
                "sets a transfer function (/{})",
                String::from_utf8_lossy(key)
            ))
        })
    })
}

fn aj1(p: &Probe<'_, '_>) -> Option<Hit> {
    let action = p.name(b"S") == Some(b"JavaScript") || p.has(b"JS");
    let tree = p.has(b"JavaScript") && p.name(b"Type") != Some(b"Catalog");
    (action || tree).then(|| found("holds JavaScript"))
}

fn st1(p: &Probe<'_, '_>) -> Option<Hit> {
    if p.stream && p.has(b"F") {
        return Some(found("is a stream whose data is in an external file (/F)"));
    }
    let x4 = p.level == PdfxLevel::X4;
    if x4 && p.has(b"OPI") {
        return Some(found("carries an OPI reference"));
    }
    if !p.level.is_2003() {
        return st3(p);
    }
    None
}

fn st3(p: &Probe<'_, '_>) -> Option<Hit> {
    (p.has(b"Ref") && p.name(b"Subtype") == Some(b"Form")).then(|| found("is a reference XObject"))
}

fn st5(p: &Probe<'_, '_>) -> Option<Hit> {
    if p.name(b"Type") == Some(b"EmbeddedFile") {
        Some(found("is an embedded file"))
    } else if p.name(b"Subtype") == Some(b"FileAttachment") {
        Some(found("is a file attachment annotation"))
    } else {
        p.has(b"EmbeddedFiles")
            .then(|| found("names embedded files"))
    }
}

/// The first feature in the dictionary newer than the level's PDF version
/// (ISO 32000-2 version catalogue).
fn st8(p: &Probe<'_, '_>) -> Option<Hit> {
    let v = |minor| PdfVersion { major: 1, minor };
    let deep = p.is_image() && p.dict.get(b"BitsPerComponent").and_then(Object::as_int) == Some(16);
    let features = [
        (p.has(b"Metadata"), "XMP metadata", v(4)),
        (p.filtered(b"JBIG2Decode"), "JBIG2 compression", v(4)),
        (p.filtered(b"JPXDecode"), "JPEG 2000 compression", v(5)),
        (
            p.has(b"OCProperties") || p.has(b"OC"),
            "optional content",
            v(5),
        ),
        (p.has(b"SMaskInData"), "SMaskInData", v(5)),
        (deep, "16-bit samples", v(5)),
        (p.has(b"XFA"), "XFA forms", v(5)),
        (p.has(b"Collection"), "a portable collection", v(7)),
    ];
    let base = p.level.based_on();
    features
        .into_iter()
        .find(|(present, _, since)| *present && *since > base)
        .map(|(_, what, since)| {
            found(&format!(
                "uses {what}, introduced in PDF {since}; {} is based on PDF {base}",
                p.level.claim()
            ))
        })
}

/// CIE-based colour (CalGray, CalRGB, Lab, ICCBased) while the GTS_PDFX
/// output intent has no `/DestOutputProfile`.
fn cie_without_profile(view: &DocumentView<'_>, ctx: &Ctx<'_>, sink: &mut Sink) {
    if gts_intents(view)
        .first()
        .is_some_and(|d| profile(view, d).is_some())
    {
        return;
    }
    for id in live_ids(view) {
        let Some(value) = view.graph().value(id) else {
            continue;
        };
        let mut space = None;
        each_array(value, 0, &mut |items| {
            let first = items
                .first()
                .and_then(Object::as_name)
                .map(|n| n.as_bytes());
            if let Some(n @ (b"CalGray" | b"CalRGB" | b"Lab" | b"ICCBased")) = first {
                space.get_or_insert_with(|| String::from_utf8_lossy(n).into_owned());
            }
        });
        if let Some(space) = space {
            sink.push(ctx.doc(&format!(
                "Object {id} uses {space}, a CIE-based space, but no GTS_PDFX output intent \
                 has a DestOutputProfile."
            )));
        }
    }
}
