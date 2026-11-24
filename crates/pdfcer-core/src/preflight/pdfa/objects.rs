//! PDF/A rows answered by looking at every dictionary of every live object:
//! stream keys and filters (ISO 19005-1 §6.1.7, §6.1.10, -2 §6.1.7),
//! images and XObjects (-1 §6.2.4 to §6.2.7, -2 §6.2.8, §6.2.9), JPEG 2000
//! at 1b (-1 §5.4), actions (-1 §6.6.1, -2 §6.5.1) and embedded files (-1
//! §6.1.11, -2/-3 §6.8).

use std::collections::BTreeSet;

use crate::document::Document;
use crate::font_unembed::{PdfaClaim, detect_pdfa};
use crate::graph::ObjectGraph;
use crate::object::{Dict, ObjId, Object};
use crate::preflight::FindingTarget;
use crate::preflight::checks::Sink;
use crate::preflight::cos::{decoded, each_dict, name};
use crate::view::DocumentView;

use super::{Ctx, PdfaLevel};

/// Filters ISO 32000-1 §7.4 defines for a stream dictionary.
const FILTERS: [&[u8]; 9] = [
    b"ASCIIHexDecode",
    b"ASCII85Decode",
    b"FlateDecode",
    b"RunLengthDecode",
    b"CCITTFaxDecode",
    b"JBIG2Decode",
    b"DCTDecode",
    b"JPXDecode",
    b"Crypt",
];

/// One dictionary met in the walk.
struct Probe<'a, 'v> {
    view: &'a DocumentView<'v>,
    dict: &'a Dict,
    stream: bool,
    level: PdfaLevel,
}

impl<'a> Probe<'a, '_> {
    fn name(&self, key: &[u8]) -> Option<&[u8]> {
        name(self.view, self.dict, key)
    }

    fn has(&self, key: &[u8]) -> bool {
        self.dict.contains_key(key)
    }

    fn get(&self, key: &[u8]) -> Option<&'a Object> {
        self.dict.get(key).map(|o| self.view.resolve(o))
    }

    fn is_true(&self, key: &[u8]) -> bool {
        matches!(self.get(key), Some(Object::Boolean(true)))
    }

    fn is_image(&self) -> bool {
        self.stream && self.name(b"Subtype") == Some(b"Image")
    }

    fn is_form(&self) -> bool {
        self.stream && self.name(b"Subtype") == Some(b"Form")
    }

    /// The `/Filter` names, in order.
    fn filters(&self) -> Vec<&[u8]> {
        match self.get(b"Filter") {
            Some(Object::Name(n)) => vec![n.as_bytes()],
            Some(Object::Array(a)) => a
                .iter()
                .filter_map(|o| self.view.resolve(o).as_name())
                .map(|n| n.as_bytes())
                .collect(),
            _ => Vec::new(),
        }
    }
}

/// What one dictionary contributes to a row.
enum Hit {
    Found(String),
    Unread(String),
}

fn found(what: &str) -> Option<Hit> {
    Some(Hit::Found(what.to_owned()))
}

type Test = fn(&Probe<'_, '_>) -> Option<Hit>;

/// Run the row if it is one of this module's; `false` when it is not.
pub(super) fn check(view: &DocumentView<'_>, ctx: &Ctx<'_>, sink: &mut Sink) -> bool {
    if ctx.row.id == "PAI-EO-7" {
        unassociated(view, ctx, sink);
        return true;
    }
    let Some(test) = image_test(ctx.row.id).or_else(|| other_test(ctx.row.id)) else {
        return false;
    };
    run(view, ctx, test, sink);
    true
}

/// Rows on images and XObjects.
fn image_test(row: &str) -> Option<Test> {
    Some(match row {
        "PAI-TG-2" => |p| {
            (p.stream && p.has(b"SMask") && (p.is_image() || p.is_form()))
                .then(|| Hit::Found("is an XObject with an /SMask".into()))
        },
        "PAI-TG-3" => |p| {
            let group = p.get(b"Group").and_then(Object::as_dict);
            let transparency =
                group.is_some_and(|g| name(p.view, g, b"S") == Some(b"Transparency"));
            (transparency && (p.is_form() || p.name(b"Type") == Some(b"Page")))
                .then(|| Hit::Found("has a transparency group".into()))
        },
        "PAI-TG-20" => |p| (p.is_image() && p.has(b"Alternates")).then(|| hit("has /Alternates")),
        "PAI-TG-21" => |p| (p.stream && p.has(b"OPI")).then(|| hit("carries an OPI reference")),
        "PAI-TG-22" => {
            |p| (p.is_image() && p.is_true(b"Interpolate")).then(|| hit("has /Interpolate true"))
        }
        "PAI-TG-23" | "PAI-TG-24" => bits,
        "PAI-TG-25" => |p| {
            let bpc = p.get(b"BitsPerComponent").and_then(Object::as_int);
            (p.is_image() && p.is_true(b"ImageMask") && bpc.is_some_and(|b| b != 1))
                .then(|| hit("is an image mask with BitsPerComponent other than 1"))
        },
        "PAI-TG-29" => |p| {
            p.filters()
                .contains(&b"JPXDecode".as_slice())
                .then(|| hit("is JPEG 2000-compressed, which post-dates the PDF 1.4 base"))
        },
        "PAI-TG-30" => |p| {
            (p.is_form() && (p.name(b"Subtype2") == Some(b"PS") || p.has(b"PS")))
                .then(|| hit("is a form XObject carrying PostScript (/Subtype2 /PS or /PS)"))
        },
        "PAI-TG-31" => {
            |p| (p.is_form() && p.has(b"Ref")).then(|| hit("is a reference XObject (/Ref)"))
        }
        "PAI-TG-32" => |p| {
            (p.stream && p.name(b"Subtype") == Some(b"PS")).then(|| hit("is a PostScript XObject"))
        },
        _ => return None,
    })
}

fn hit(what: &str) -> Hit {
    Hit::Found(what.to_owned())
}

/// Rows on streams, actions and file specifications.
fn other_test(row: &str) -> Option<Test> {
    Some(match row {
        "PAI-FS-14" => |p| {
            let key = [b"F".as_slice(), b"FFilter", b"FDecodeParms"]
                .into_iter()
                .find(|k| p.has(k));
            key.filter(|_| p.stream).map(|k| {
                hit(&format!(
                    "is a stream with /{}, which refers to an external file",
                    String::from_utf8_lossy(k)
                ))
            })
        },
        "PAI-FS-15" => filters,
        "PAI-FS-16" => crypt,
        "PAI-TG-15" => |p| {
            let image = p.is_image().then(|| p.name(b"Intent")).flatten();
            let ri = (!p.stream).then(|| p.name(b"RI")).flatten();
            image.or(ri).filter(|i| !is_intent(i)).map(|i| {
                hit(&format!(
                    "uses the rendering intent /{}",
                    String::from_utf8_lossy(i)
                ))
            })
        },
        "PAI-IA-20" | "PAI-IA-21" => action_types,
        "PAI-IA-22" => named_actions,
        "PAI-EO-1" => |p| {
            p.has(b"EF")
                .then(|| hit("is a file specification with /EF"))
        },
        "PAI-EO-3" => |p| {
            (p.has(b"EF") && !(p.has(b"F") && p.has(b"UF")))
                .then(|| hit("is a file specification with /EF but not both /F and /UF"))
        },
        "PAI-EO-4" => embedded_claim,
        "PAI-EO-5" => mime,
        "PAI-EO-6" => |p| {
            (p.has(b"EF") && !p.has(b"AFRelationship"))
                .then(|| hit("is a file specification with /EF and no /AFRelationship"))
        },
        _ => return None,
    })
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
            Hit::Unread(what) => ctx.unread(None, &format!("object {id} {what}")),
        });
    });
}

fn is_intent(name: &[u8]) -> bool {
    matches!(
        name,
        b"RelativeColorimetric" | b"AbsoluteColorimetric" | b"Perceptual" | b"Saturation"
    )
}

/// FS-15. LZW is out at every level, and so is JXLDecode (it is defined by
/// no ISO 32000 part). At 2b/3b only the ISO 32000-1 filters are allowed.
fn filters(p: &Probe<'_, '_>) -> Option<Hit> {
    let strict = p.level.part() > 1;
    p.filters().into_iter().find_map(|f| match f {
        b"LZWDecode" => found("is LZW-compressed"),
        b"JXLDecode" => found(
            "uses /JXLDecode, a filter not defined by ISO 32000-1 or -2 and so not \
             permitted at any PDF/A level",
        ),
        f if strict && !FILTERS.contains(&f) => found(&format!(
            "uses /{}, which is not an ISO 32000-1 standard filter",
            String::from_utf8_lossy(f)
        )),
        _ => None,
    })
}

/// FS-16: every `/Crypt` filter's parameters name the Identity crypt filter
/// (absent `/Name` defaults to Identity, ISO 32000-1 Table 14).
fn crypt(p: &Probe<'_, '_>) -> Option<Hit> {
    let names = p.filters();
    let parms = p.get(b"DecodeParms");
    names.iter().enumerate().find_map(|(i, f)| {
        if *f != b"Crypt" {
            return None;
        }
        let parm = match parms {
            Some(Object::Array(a)) => a.get(i).map(|o| p.view.resolve(o)),
            other => other.filter(|_| names.len() == 1),
        };
        let crypt_name = parm
            .and_then(Object::as_dict)
            .and_then(|d| name(p.view, d, b"Name"));
        crypt_name.filter(|n| *n != b"Identity").and_then(|n| {
            found(&format!(
                "uses the crypt filter /{}",
                String::from_utf8_lossy(n)
            ))
        })
    })
}

/// TG-23 (1b: 1, 2, 4 or 8) and TG-24 (2b/3b: and 16) image depths. A JPEG
/// 2000 image may omit `/BitsPerComponent`; an absent one is not judged.
fn bits(p: &Probe<'_, '_>) -> Option<Hit> {
    if !p.is_image() || p.is_true(b"ImageMask") {
        return None;
    }
    let bpc = p.get(b"BitsPerComponent").and_then(Object::as_int)?;
    let ok = matches!(bpc, 1 | 2 | 4 | 8) || (p.level.part() > 1 && bpc == 16);
    (!ok).then(|| hit(&format!("is an image with BitsPerComponent {bpc}")))
}

/// The actions this dictionary holds under `/A`, `/OpenAction`, `/Next` and
/// `/AA`; a value with no `/S` (an outline's `/Next`) is not an action.
fn held<'a>(p: &Probe<'a, '_>) -> Vec<&'a Dict> {
    let mut held: Vec<&Dict> = Vec::new();
    for key in [b"A".as_slice(), b"OpenAction", b"Next"] {
        match p.get(key) {
            Some(Object::Dict(d)) => held.push(d),
            Some(Object::Array(a)) if key == b"Next" => {
                held.extend(a.iter().filter_map(|o| p.view.resolve(o).as_dict()));
            }
            _ => {}
        }
    }
    if let Some(Object::Dict(aa)) = p.get(b"AA") {
        held.extend(aa.iter().filter_map(|(_, o)| p.view.resolve(o).as_dict()));
    }
    held.retain(|a| a.contains_key(b"S"));
    held
}

/// IA-20 (1b) and IA-21 (2b/3b): the permitted action types.
fn action_types(p: &Probe<'_, '_>) -> Option<Hit> {
    held(p).into_iter().find_map(|a| {
        let s = name(p.view, a, b"S")?;
        let gotoe = p.level.part() > 1 && s == b"GoToE";
        let allowed = matches!(
            s,
            b"GoTo" | b"GoToR" | b"Thread" | b"URI" | b"Named" | b"SubmitForm"
        ) || gotoe;
        (!allowed).then(|| {
            hit(&format!(
                "holds a /{} action, which {} does not permit",
                String::from_utf8_lossy(s),
                p.level.claim()
            ))
        })
    })
}

/// IA-22: named actions are the four page-navigation ones.
fn named_actions(p: &Probe<'_, '_>) -> Option<Hit> {
    held(p).into_iter().find_map(|a| named(p, a))
}

fn named(p: &Probe<'_, '_>, a: &Dict) -> Option<Hit> {
    let n = (name(p.view, a, b"S") == Some(b"Named"))
        .then(|| name(p.view, a, b"N"))
        .flatten()?;
    (!matches!(n, b"NextPage" | b"PrevPage" | b"FirstPage" | b"LastPage")).then(|| {
        hit(&format!(
            "holds the named action /{}",
            String::from_utf8_lossy(n)
        ))
    })
}

/// The embedded file streams of a file specification's `/EF`.
fn embedded<'a>(p: &Probe<'a, '_>) -> Vec<&'a crate::object::Stream> {
    let Some(Object::Dict(ef)) = p.dict.get(b"EF").map(|o| p.view.resolve(o)) else {
        return Vec::new();
    };
    ef.iter()
        .filter_map(|(_, o)| match p.view.resolve(o) {
            Object::Stream(s) => Some(s),
            _ => None,
        })
        .collect()
}

/// EO-4 (2b): an embedded file is itself PDF/A-1 or PDF/A-2, judged by its
/// own claim.
fn embedded_claim(p: &Probe<'_, '_>) -> Option<Hit> {
    embedded(p).into_iter().find_map(|s| {
        let Some(bytes) = decoded(p.view, s) else {
            return Some(Hit::Unread(
                "has an embedded file that could not be decoded".into(),
            ));
        };
        let Ok(doc) = Document::from_bytes(bytes) else {
            return found("embeds a file that is not a readable PDF");
        };
        let claim = detect_pdfa(&doc.view());
        let ok =
            matches!(&claim, PdfaClaim::Identified { part: Some(n), .. } if n == "1" || n == "2");
        (!ok).then(|| hit("embeds a PDF that does not claim PDF/A-1 or PDF/A-2"))
    })
}

/// EO-5 (3b): an embedded file stream's `/Subtype` is a MIME type.
fn mime(p: &Probe<'_, '_>) -> Option<Hit> {
    embedded(p).into_iter().find_map(|s| {
        let subtype = name(p.view, &s.dict, b"Subtype");
        let ok = subtype.is_some_and(|t| {
            let mut parts = t.split(|b| *b == b'/');
            let (a, b) = (parts.next(), parts.next());
            parts.next().is_none()
                && a.is_some_and(|a| !a.is_empty())
                && b.is_some_and(|b| !b.is_empty())
        });
        (!ok).then(|| hit("has an embedded file whose /Subtype is not a MIME type"))
    })
}

/// EO-7 (3b): every file specification object with `/EF` is named by some
/// `/AF` array.
fn unassociated(view: &DocumentView<'_>, ctx: &Ctx<'_>, sink: &mut Sink) {
    let mut named = BTreeSet::<ObjId>::new();
    each_dict(view, &mut |_, dict, _| {
        if let Some(Object::Array(af)) = dict.get(b"AF").map(|o| view.resolve(o)) {
            named.extend(af.iter().filter_map(Object::as_reference));
        }
    });
    each_dict(view, &mut |id, dict, _| {
        let top = view
            .graph()
            .value(id)
            .and_then(Object::as_dict)
            .is_some_and(|d| std::ptr::eq(d, dict));
        if top && dict.contains_key(b"EF") && !named.contains(&id) {
            sink.push(ctx.doc(&format!(
                "Object {id} is a file specification with /EF that no /AF array names."
            )));
        }
    });
}
