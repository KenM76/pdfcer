//! Document-level PDF/A rows read from the file's bytes, the trailer and
//! the catalog: header and trailer (ISO 19005-1/-2/-3 §6.1.2–§6.1.4,
//! §6.1.12/13), permissions (-2/-3 §6.1.12), the PDF/A output intent
//! (-1 §6.2.2, -2/-3 §6.2.3), forms and catalog actions (-1 §6.6.2, §6.9;
//! -2/-3 §6.4, §6.5.2), embedded files and optional content (-1 §6.1.11,
//! §6.1.13; -2/-3 §6.8–§6.11).

use std::collections::BTreeSet;

use crate::graph::ObjectGraph;
use crate::object::{Dict, ObjId, Object};
use crate::preflight::checks::Sink;
use crate::preflight::cos::catalog;
use crate::view::DocumentView;

use super::icc::Header;
use super::{Ctx, PdfaLevel};

/// Indirect objects a PDF/A file may hold (-1 §6.1.12, -2/-3 §6.1.13).
const MAX_OBJECTS: usize = 8_388_607;
/// Form-field nesting followed.
const MAX_FIELD_DEPTH: usize = 32;

/// Run the row if it is one of this module's; `false` when it is not.
pub(super) fn check(view: &DocumentView<'_>, ctx: &Ctx<'_>, sink: &mut Sink) -> bool {
    let id = ctx.row.id;
    match id {
        "PAI-FS-1" | "PAI-FS-2" | "PAI-FS-6" => bytes_row(view, ctx, sink),
        "PAI-FS-3" | "PAI-FS-5" | "PAI-FS-9" | "PAI-FS-39" => trailer_row(view, ctx, sink),
        "PAI-OI-3" | "PAI-OI-4" | "PAI-OI-5" | "PAI-OI-6" => intents(view, ctx, sink),
        "PAI-FS-20" => perms(view, ctx, sink),
        "PAI-IA-25" => fields(view, ctx, sink),
        "PAI-EO-11" | "PAI-EO-12" | "PAI-EO-13" | "PAI-EO-14" => optional_content(view, ctx, sink),
        _ => return catalog_row(view, ctx, sink),
    }
    true
}

/// One end-of-line marker at the start of `rest`: its length, or 0.
fn eol(rest: &[u8]) -> usize {
    match rest {
        [b'\r', b'\n', ..] => 2,
        [b'\r' | b'\n', ..] => 1,
        _ => 0,
    }
}

fn bytes_row(view: &DocumentView<'_>, ctx: &Ctx<'_>, sink: &mut Sink) {
    let Some(bytes) = view.bytes() else {
        sink.push(ctx.unread(None, "the file's bytes are not available"));
        return;
    };
    let m = match ctx.row.id {
        "PAI-FS-1" => header(bytes, ctx.level),
        "PAI-FS-2" => binary_comment(bytes),
        _ => after_eof(bytes),
    };
    if let Some(m) = m {
        sink.push(ctx.doc(&m));
    }
}

fn header(bytes: &[u8], level: PdfaLevel) -> Option<String> {
    let ok = match bytes {
        [b'%', b'P', b'D', b'F', b'-', major, b'.', minor, rest @ ..] => {
            major.is_ascii_digit()
                && minor.is_ascii_digit()
                && (level.part() == 1 || (*major == b'1' && *minor <= b'7' && eol(rest) > 0))
        }
        _ => false,
    };
    let want = if level.part() == 1 {
        "%PDF-n.n"
    } else {
        "%PDF-1.0 to %PDF-1.7 and an end of line"
    };
    (!ok).then(|| format!("The file does not begin with {want}."))
}

fn binary_comment(bytes: &[u8]) -> Option<String> {
    let line = bytes.get(8..).map_or(0, eol);
    let ok = bytes
        .get(8 + line..)
        .and_then(|rest| rest.get(..5))
        .and_then(<[u8]>::split_first)
        .is_some_and(|(&h, t)| h == b'%' && t.iter().all(|&b| b > 127));
    (line == 0 || !ok).then(|| {
        "The header is not followed by a comment of four or more bytes above 127.".to_owned()
    })
}

fn after_eof(bytes: &[u8]) -> Option<String> {
    let Some(at) = bytes.windows(5).rposition(|w| w == b"%%EOF") else {
        return Some("The file has no %%EOF marker.".to_owned());
    };
    let rest = bytes.get(at + 5..).unwrap_or_default();
    (rest.len() != eol(rest)).then(|| {
        format!(
            "{} bytes follow the last %%EOF, beyond one end of line.",
            rest.len() - eol(rest)
        )
    })
}

fn trailer_row(view: &DocumentView<'_>, ctx: &Ctx<'_>, sink: &mut Sink) {
    let m = match ctx.row.id {
        "PAI-FS-3" => {
            let id = view.trailer_entry(b"ID").map(|o| view.resolve(o));
            (!id.and_then(Object::as_array).is_some_and(|a| !a.is_empty()))
                .then_some("The trailer has no /ID array, or it is empty.".to_owned())
        }
        "PAI-FS-5" => view
            .trailer_entry(b"Encrypt")
            .map(|_| "The file is encrypted (the trailer has /Encrypt).".to_owned()),
        "PAI-FS-9" => xref_stream(view)
            .then(|| "The file uses a cross-reference stream (PDF 1.5).".to_owned()),
        _ => {
            let n = crate::prepress::live_ids(view).len();
            (n > MAX_OBJECTS).then(|| format!("The file holds {n} indirect objects."))
        }
    };
    if let Some(m) = m {
        sink.push(ctx.doc(&m));
    }
}

/// Whether the newest cross-reference section is a stream, or a hybrid
/// trailer points at one.
fn xref_stream(view: &DocumentView<'_>) -> bool {
    let typed = view
        .trailer_entry(b"Type")
        .and_then(Object::as_name)
        .is_some_and(|n| n.as_bytes() == b"XRef");
    let newest = view
        .bytes()
        .and_then(|b| crate::xref::load_xref_chain(b).ok())
        .is_some_and(|x| {
            matches!(
                x.newest_shape,
                crate::xref::SectionShape::Stream { .. }
                    | crate::xref::SectionShape::Classic { xref_stm: Some(_) }
            )
        });
    typed || newest || view.trailer_entry(b"XRefStm").is_some()
}

/// The catalog's `key` entry as a dictionary.
fn sub<'v>(view: &'v DocumentView<'_>, dict: &'v Dict, key: &[u8]) -> Option<&'v Dict> {
    dict.get(key)
        .map(|o| view.resolve(o))
        .and_then(Object::as_dict)
}

fn is_true(view: &DocumentView<'_>, dict: &Dict, key: &[u8]) -> bool {
    dict.get(key)
        .map(|o| view.resolve(o))
        .is_some_and(|o| matches!(o, Object::Boolean(true)))
}

/// Single-entry catalog, AcroForm and name-dictionary rows.
fn catalog_row(view: &DocumentView<'_>, ctx: &Ctx<'_>, sink: &mut Sink) -> bool {
    let Some(cat) = catalog(view) else {
        return false;
    };
    let form = sub(view, cat, b"AcroForm");
    let names = sub(view, cat, b"Names");
    let m = match ctx.row.id {
        "PAI-IA-11" => form
            .filter(|f| is_true(view, f, b"NeedAppearances"))
            .map(|_| "The AcroForm sets /NeedAppearances true."),
        "PAI-IA-13" => form
            .filter(|f| f.get(b"XFA").is_some())
            .map(|_| "The AcroForm carries an XFA form."),
        "PAI-IA-14" => is_true(view, cat, b"NeedsRendering")
            .then_some("The catalog sets /NeedsRendering true."),
        "PAI-IA-26" => cat
            .get(b"AA")
            .map(|_| "The catalog has additional actions (/AA)."),
        "PAI-IA-28" => names
            .filter(|n| n.get(b"JavaScript").is_some())
            .map(|_| "The name dictionary holds document-level JavaScript."),
        "PAI-EO-2" => names
            .filter(|n| n.get(b"EmbeddedFiles").is_some())
            .map(|_| "The name dictionary lists embedded files."),
        "PAI-EO-10" => cat
            .get(b"OCProperties")
            .map(|_| "The catalog has optional content (/OCProperties)."),
        "PAI-EO-20" => names
            .filter(|n| n.get(b"AlternatePresentations").is_some())
            .map(|_| "The name dictionary has /AlternatePresentations."),
        "PAI-EO-22" => cat
            .get(b"Requirements")
            .map(|_| "The catalog has /Requirements."),
        _ => return false,
    };
    if let Some(m) = m {
        sink.push(ctx.doc(m));
    }
    true
}

/// Every `/OutputIntents` entry.
fn all_intents<'v>(view: &'v DocumentView<'_>) -> Vec<&'v Dict> {
    catalog(view)
        .and_then(|c| c.get(b"OutputIntents"))
        .map(|o| view.resolve(o))
        .and_then(Object::as_array)
        .map(|a| a.iter().filter_map(|o| view.resolve(o).as_dict()).collect())
        .unwrap_or_default()
}

/// The file's PDF/A output intent, as the page colour rows need it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Intent {
    /// No `GTS_PDFA1` output intent.
    Absent,
    /// One is present, but its destination profile could not be read.
    Unread,
    /// The destination profile's data colour space (`RGB `, `CMYK`, ...).
    Space([u8; 4]),
}

/// The first `GTS_PDFA1` output intent's profile colour space.
pub(super) fn pdfa_intent(view: &DocumentView<'_>) -> Intent {
    let gts = all_intents(view).into_iter().find(|d| {
        d.get(b"S")
            .map(|o| view.resolve(o))
            .and_then(Object::as_name)
            .is_some_and(|n| n.as_bytes() == b"GTS_PDFA1")
    });
    let Some(intent) = gts else {
        return Intent::Absent;
    };
    match intent.get(b"DestOutputProfile").map(|o| view.resolve(o)) {
        Some(Object::Stream(s)) => {
            Header::of(view, s).map_or(Intent::Unread, |h| Intent::Space(h.space))
        }
        _ => Intent::Unread,
    }
}

fn intents(view: &DocumentView<'_>, ctx: &Ctx<'_>, sink: &mut Sink) {
    let all = all_intents(view);
    match ctx.row.id {
        "PAI-OI-5" => {
            let profiles: BTreeSet<Option<ObjId>> = all
                .iter()
                .filter_map(|d| d.get(b"DestOutputProfile"))
                .map(Object::as_reference)
                .collect();
            let direct = profiles.contains(&None) && all.len() > 1;
            if profiles.len() > 1 || direct {
                sink.push(
                    ctx.doc(
                        "The output intents' destination profiles are not one indirect object.",
                    ),
                );
            }
        }
        "PAI-OI-6" => {
            if all.iter().any(|d| d.get(b"DestOutputProfileRef").is_some()) {
                sink.push(ctx.doc("An output intent has /DestOutputProfileRef."));
            }
        }
        _ => pdfa_profile(view, ctx, &all, sink),
    }
}

/// OI-3 and OI-4: the class, space and version of every GTS_PDFA1
/// destination profile.
fn pdfa_profile(view: &DocumentView<'_>, ctx: &Ctx<'_>, all: &[&Dict], sink: &mut Sink) {
    let max = if ctx.level.part() == 1 { 2 } else { 4 };
    for intent in all {
        let s = intent.get(b"S").map(|o| view.resolve(o));
        if s.and_then(Object::as_name).map(|n| n.as_bytes()) != Some(b"GTS_PDFA1") {
            continue;
        }
        let Some(Object::Stream(stream)) =
            intent.get(b"DestOutputProfile").map(|o| view.resolve(o))
        else {
            continue;
        };
        let Some(h) = Header::of(view, stream) else {
            sink.push(ctx.unread(None, "the PDF/A output profile could not be read"));
            continue;
        };
        let class = matches!(&h.class, b"prtr" | b"mntr");
        let space = matches!(&h.space, b"GRAY" | b"RGB " | b"CMYK");
        if !class || !space || h.major > max {
            sink.push(ctx.doc(&format!(
                "The PDF/A output profile is {}; it must be a prtr or mntr Gray, RGB or CMYK \
                 profile of version {max} or lower.",
                h.describe()
            )));
        }
    }
}

/// FS-20: `/Perms` keys, and digest keys under a DocMDP signature.
fn perms(view: &DocumentView<'_>, ctx: &Ctx<'_>, sink: &mut Sink) {
    let Some(perms) = catalog(view).and_then(|c| sub(view, c, b"Perms")) else {
        return;
    };
    for (key, _) in perms.iter() {
        if !matches!(key.as_bytes(), b"UR3" | b"DocMDP") {
            sink.push(ctx.doc(&format!(
                "The catalog's /Perms holds /{}.",
                String::from_utf8_lossy(key.as_bytes())
            )));
        }
    }
    let refs = sub(view, perms, b"DocMDP")
        .and_then(|sig| sig.get(b"Reference"))
        .map(|o| view.resolve(o))
        .and_then(Object::as_array);
    for r in refs.into_iter().flatten() {
        let Some(r) = view.resolve(r).as_dict() else {
            continue;
        };
        let digest = [
            b"DigestLocation".as_slice(),
            b"DigestMethod",
            b"DigestValue",
        ];
        if digest.iter().any(|k| r.get(k).is_some()) {
            sink.push(ctx.doc("The DocMDP signature's reference dictionary has digest keys."));
        }
    }
}

/// IA-25: no field dictionary in the AcroForm tree has `/AA`.
fn fields(view: &DocumentView<'_>, ctx: &Ctx<'_>, sink: &mut Sink) {
    let roots = catalog(view)
        .and_then(|c| sub(view, c, b"AcroForm"))
        .and_then(|f| f.get(b"Fields"))
        .map(|o| view.resolve(o))
        .and_then(Object::as_array);
    let mut seen = BTreeSet::new();
    let mut stack: Vec<(&Object, usize)> = roots.into_iter().flatten().map(|o| (o, 0)).collect();
    while let Some((obj, depth)) = stack.pop() {
        if obj.as_reference().is_some_and(|id| !seen.insert(id)) || depth > MAX_FIELD_DEPTH {
            continue;
        }
        let Some(field) = view.resolve(obj).as_dict() else {
            continue;
        };
        if field.get(b"AA").is_some() {
            let what = obj
                .as_reference()
                .map_or("A form field".to_owned(), |id| format!("Form field {id}"));
            sink.push(ctx.doc(&format!("{what} has additional actions (/AA).")));
        }
        if let Some(kids) = field
            .get(b"Kids")
            .map(|o| view.resolve(o))
            .and_then(Object::as_array)
        {
            stack.extend(kids.iter().map(|k| (k, depth + 1)));
        }
    }
}

/// EO-11 to EO-14: the optional-content configurations.
fn optional_content(view: &DocumentView<'_>, ctx: &Ctx<'_>, sink: &mut Sink) {
    let Some(oc) = catalog(view).and_then(|c| sub(view, c, b"OCProperties")) else {
        return;
    };
    let mut configs: Vec<&Dict> = sub(view, oc, b"D").into_iter().collect();
    if let Some(more) = oc
        .get(b"Configs")
        .map(|o| view.resolve(o))
        .and_then(Object::as_array)
    {
        configs.extend(more.iter().filter_map(|o| view.resolve(o).as_dict()));
    }
    let name = |d: &Dict| match d.get(b"Name").map(|o| view.resolve(o)) {
        Some(Object::String(s)) => crate::textstring::decode_text_string(s).text,
        _ => String::new(),
    };
    match ctx.row.id {
        "PAI-EO-11" => {
            let unnamed = configs.iter().filter(|d| name(d).is_empty()).count();
            if unnamed > 0 {
                sink.push(ctx.doc(&format!(
                    "{unnamed} optional-content configuration(s) have no /Name."
                )));
            }
        }
        "PAI-EO-12" => {
            let mut names = BTreeSet::new();
            for n in configs.iter().map(|d| name(d)).filter(|n| !n.is_empty()) {
                if !names.insert(n.clone()) {
                    sink.push(ctx.doc(&format!(
                        "Two optional-content configurations are named \"{n}\"."
                    )));
                }
            }
        }
        "PAI-EO-13" => order(view, ctx, oc, &configs, sink),
        _ => {
            if configs.iter().any(|d| d.get(b"AS").is_some()) {
                sink.push(ctx.doc("An optional-content configuration has /AS."));
            }
        }
    }
}

/// EO-13: each configuration's `/Order` names every group in `/OCGs`.
fn order(view: &DocumentView<'_>, ctx: &Ctx<'_>, oc: &Dict, configs: &[&Dict], sink: &mut Sink) {
    let groups: BTreeSet<ObjId> = oc
        .get(b"OCGs")
        .map(|o| view.resolve(o))
        .and_then(Object::as_array)
        .map(|a| a.iter().filter_map(Object::as_reference).collect())
        .unwrap_or_default();
    for config in configs {
        let Some(order) = config.get(b"Order").map(|o| view.resolve(o)) else {
            continue;
        };
        let mut listed = BTreeSet::new();
        crate::prepress::each_array(order, 0, &mut |items| {
            listed.extend(items.iter().filter_map(Object::as_reference));
        });
        let missing = groups.difference(&listed).count();
        if missing > 0 {
            sink.push(ctx.doc(&format!(
                "An optional-content configuration's /Order omits {missing} group(s)."
            )));
        }
    }
}
