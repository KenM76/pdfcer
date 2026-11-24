//! Document-level checks: file header, tagging flags, XMP identification
//! and title, viewer preferences, optional content, embedded files, XFA and
//! encryption (ISO 14289-1 §5, §6.1, §6.2, §7.1, §7.2, §7.10, §7.11, §7.15,
//! §7.16; ISO 14289-2 §5, §6.2, §8.4.4, §8.7, §8.10.1, §8.11, §8.14).

use super::structure::Tree;
use super::{FindingTarget, Out, PdfuaPart, string_of, text};
use crate::font_unembed::{catalog_xmp, find_bytes, xmp_short_value};
use crate::graph::ObjectGraph;
use crate::object::{Dict, Object};
use crate::view::DocumentView;
use crate::xfa::XfaFormKind;

const DOC: FindingTarget = FindingTarget::Document;

/// Every document-level check.
pub(super) fn check(view: &DocumentView<'_>, tree: &Tree, out: &mut Out) {
    if out.part == PdfuaPart::Ua1 {
        header(view, out);
    }
    let Some(catalog) = view.graph().catalog_dict() else {
        let message = "The document catalog could not be read.".to_owned();
        out.incomplete("PUI-V-6.2-1", "PUI2-6.2-1", None, DOC, message);
        return;
    };
    mark_info(view, catalog, out);
    metadata(view, tree, out);
    display_doc_title(view, catalog, out);
    if out.part == PdfuaPart::Ua2 && tree.catalog_lang.is_none() {
        let message = "The catalog has no /Lang, so the document's language is unknown.".into();
        out.violation("", "PUI2-8.4.4-1", None, DOC, message);
    }
    optional_content(view, catalog, out);
    embedded_files(view, catalog, out);
    xfa(view, out);
    if out.part == PdfuaPart::Ua1 {
        encryption(view, out);
    }
}

/// veraPDF PDFUA-1 6.1-1: `%PDF-1.n`, `n` in 0–7, then an end of line.
fn header(view: &DocumentView<'_>, out: &mut Out) {
    let Some(bytes) = view.bytes() else {
        let message = "The file's bytes are not available, so its header was not read.".into();
        out.incomplete("PUI-V-6.1-1", "", None, DOC, message);
        return;
    };
    if !header_ok(bytes) {
        let message =
            "The file header is not \"%PDF-1.n\" (n from 0 to 7) on a line of its own.".into();
        out.violation("PUI-V-6.1-1", "", None, DOC, message);
    }
}

/// Whether `bytes` open with the header veraPDF PDFUA-1 6.1-1 accepts.
pub(crate) fn header_ok(bytes: &[u8]) -> bool {
    bytes.starts_with(b"%PDF-1.")
        && matches!(bytes.get(7), Some(b'0'..=b'7'))
        && matches!(bytes.get(8), Some(b'\r' | b'\n'))
}

/// ISO 14289-1 §6.2 / ISO 14289-2 §6.2: `/MarkInfo /Marked true`, and
/// (UA-1, Matterhorn 01-007) no `/Suspects true`.
fn mark_info(view: &DocumentView<'_>, catalog: &Dict, out: &mut Out) {
    let info = catalog
        .get(b"MarkInfo")
        .map(|o| view.resolve(o))
        .and_then(Object::as_dict);
    let flag = |key: &[u8]| {
        info.and_then(|d| d.get(key))
            .map(|o| view.resolve(o))
            .is_some_and(|o| matches!(o, Object::Boolean(true)))
    };
    if !flag(b"Marked") {
        let message = "The catalog's /MarkInfo does not say /Marked true.".into();
        out.violation("PUI-V-6.2-1", "PUI2-6.2-1", None, DOC, message);
    }
    if flag(b"Suspects") {
        let message = "/MarkInfo says /Suspects true: the tagging is flagged as unreliable.".into();
        out.violation("PUI-01-007", "", None, DOC, message);
    }
}

/// ISO 14289-1 §5, §7.1 ¶8 and §7.2 ¶3; ISO 14289-2 §5, §8.11.1: the XMP
/// packet, its PDF/UA identification and its title.
fn metadata(view: &DocumentView<'_>, tree: &Tree, out: &mut Out) {
    let Some(packet) = catalog_xmp(view) else {
        let message = "The catalog has no readable XMP metadata stream.".into();
        out.violation("PUI-06-001", "PUI2-8.11.1-2", None, DOC, message);
        return;
    };
    identification(&packet, out);
    if !has_title(&packet) {
        let message = "The XMP metadata has no non-empty dc:title.".into();
        out.violation("PUI-06-003", "PUI2-8.11.1-1", None, DOC, message);
    }
    if out.part == PdfuaPart::Ua1
        && find_bytes(&packet, b"x-default").is_some()
        && tree.catalog_lang.is_none()
    {
        let message =
            "The XMP metadata has x-default language text but the catalog has no /Lang.".into();
        out.violation("PUI-11-006", "", None, DOC, message);
    }
}

/// `pdfuaid:part` is the part's number; UA-2 also needs `pdfuaid:rev`
/// `2024` (veraPDF PDFUA-2 5-1, 5-2, 5-5).
fn identification(packet: &[u8], out: &mut Out) {
    let want = out.part.number().to_string();
    let part = xmp_short_value(packet, b"pdfuaid:part");
    match part.as_deref() {
        None => {
            let message = "The XMP metadata has no pdfuaid:part identification.".into();
            out.violation("PUI-06-002", "PUI2-5-1", None, DOC, message);
        }
        Some(p) if p != want => {
            let message = format!("pdfuaid:part is {p}, not {want}.");
            out.violation("PUI-06-002", "PUI2-5-2", None, DOC, message);
        }
        Some(_) => {}
    }
    if out.part == PdfuaPart::Ua2 {
        let rev = xmp_short_value(packet, b"pdfuaid:rev");
        if rev.as_deref() != Some("2024") {
            let shown = rev.unwrap_or_else(|| "absent".into());
            let message = format!("pdfuaid:rev is {shown}, not 2024.");
            out.violation("", "PUI2-5-5", None, DOC, message);
        }
    }
}

/// A `dc:title` with some non-blank text in it.
pub(crate) fn has_title(packet: &[u8]) -> bool {
    let Some(start) = find_bytes(packet, b"<dc:title") else {
        return false;
    };
    let rest = packet.get(start..).unwrap_or_default();
    let end = find_bytes(rest, b"</dc:title>").unwrap_or(rest.len());
    let body = rest.get(..end).unwrap_or_default();
    // Text between a closing `>` and the next `<` that is not whitespace.
    let mut inside = false;
    body.iter().any(|&b| match b {
        b'>' => {
            inside = true;
            false
        }
        b'<' => {
            inside = false;
            false
        }
        _ => inside && !b.is_ascii_whitespace(),
    })
}

/// ISO 14289-1 §7.1 ¶10 / ISO 14289-2 §8.11.2 (Matterhorn 07-001, 07-002).
fn display_doc_title(view: &DocumentView<'_>, catalog: &Dict, out: &mut Out) {
    let value = catalog
        .get(b"ViewerPreferences")
        .map(|o| view.resolve(o))
        .and_then(Object::as_dict)
        .and_then(|d| d.get(b"DisplayDocTitle"))
        .map(|o| view.resolve(o));
    match value {
        Some(Object::Boolean(true)) => {}
        None => {
            let message = "/ViewerPreferences has no /DisplayDocTitle entry.".into();
            out.violation("PUI-07-001", "PUI2-8.11.2-1", None, DOC, message);
        }
        Some(_) => {
            let message = "/ViewerPreferences /DisplayDocTitle is not true.".into();
            out.violation("PUI-07-002", "PUI2-8.11.2-1", None, DOC, message);
        }
    }
}

/// ISO 14289-1 §7.10 / ISO 14289-2 §8.7 (Matterhorn 20-001…003): every
/// optional-content configuration has a non-empty `/Name` and no `/AS`.
fn optional_content(view: &DocumentView<'_>, catalog: &Dict, out: &mut Out) {
    let Some(oc) = catalog
        .get(b"OCProperties")
        .map(|o| view.resolve(o))
        .and_then(Object::as_dict)
    else {
        return;
    };
    let default = oc
        .get(b"D")
        .map(|o| view.resolve(o))
        .and_then(Object::as_dict);
    let configs = oc
        .get(b"Configs")
        .map(|o| view.resolve(o))
        .and_then(Object::as_array)
        .unwrap_or_default();
    let alternates = configs
        .iter()
        .filter_map(|o| view.resolve(o).as_dict())
        .map(|d| ("PUI-20-001", "an entry of /Configs", d));
    for (row, what, config) in default
        .map(|d| ("PUI-20-002", "the default /D", d))
        .into_iter()
        .chain(alternates)
    {
        let named = string_of(view, config.get(b"Name")).is_some_and(|s| !text(s).is_empty());
        if !named {
            let message = format!("The optional-content configuration {what} has no /Name.");
            out.violation(row, "PUI2-8.7-1", None, DOC, message);
        }
        if config.contains_key(b"AS") {
            let message = format!("The optional-content configuration {what} has an /AS entry.");
            out.violation("PUI-20-003", "PUI2-8.7-2", None, DOC, message);
        }
    }
}

/// ISO 14289-1 §7.11 (Matterhorn 21-001: `/F` and `/UF`) / ISO 14289-2
/// §8.14.1 (veraPDF 8.14.1-1: `/Desc`) for every embedded file.
fn embedded_files(view: &DocumentView<'_>, catalog: &Dict, out: &mut Out) {
    let Some(root) = catalog
        .get(b"Names")
        .map(|o| view.resolve(o))
        .and_then(Object::as_dict)
        .and_then(|d| d.get(b"EmbeddedFiles"))
        .map(|o| view.resolve(o))
        .and_then(Object::as_dict)
    else {
        return;
    };
    let Some(entries) = crate::pageops::references::name_tree_entries(view, root) else {
        let message = "The embedded-files name tree could not be read.".into();
        out.incomplete("PUI-21-001", "PUI2-8.14.1-1", None, DOC, message);
        return;
    };
    let mut names: Vec<_> = entries.into_iter().collect();
    names.sort_by(|a, b| a.0.cmp(&b.0));
    for (name, spec) in names {
        let Some(spec) = view.resolve(&spec).as_dict().cloned() else {
            continue;
        };
        let shown = text(&name);
        let missing = match out.part {
            PdfuaPart::Ua1 => !(spec.contains_key(b"F") && spec.contains_key(b"UF")),
            PdfuaPart::Ua2 => !spec.contains_key(b"Desc"),
        };
        if missing {
            let need = out.pick("/F and /UF", "/Desc");
            let message = format!("The embedded file \"{shown}\" has no {need}.");
            out.violation("PUI-21-001", "PUI2-8.14.1-1", None, DOC, message);
        }
    }
}

/// ISO 14289-1 §7.15 (Matterhorn 25-001: dynamic XFA) / ISO 14289-2
/// §8.10.1 (veraPDF 8.10.1-3: no XFA at all).
fn xfa(view: &DocumentView<'_>, out: &mut Out) {
    let kind = crate::xfa::form_kind(view);
    let fails = match out.part {
        PdfuaPart::Ua1 => kind == XfaFormKind::Dynamic,
        PdfuaPart::Ua2 => kind != XfaFormKind::NotXfa,
    };
    if fails {
        let message = format!("The form uses XFA ({kind:?}).");
        out.violation("PUI-25-001", "PUI2-8.10.1-3", None, DOC, message);
    }
}

/// ISO 14289-1 §7.16 (Matterhorn 26-001, 26-002): an encrypted file
/// permits assistive technology to extract content (bit 10 of `/P`).
fn encryption(view: &DocumentView<'_>, out: &mut Out) {
    let Some(encrypt) = view
        .trailer_entry(b"Encrypt")
        .map(|o| view.resolve(o))
        .and_then(Object::as_dict)
    else {
        return;
    };
    match encrypt.get(b"P").map(|o| view.resolve(o)) {
        Some(Object::Integer(p)) if p & (1 << 9) != 0 => {}
        Some(Object::Integer(_)) => {
            let message = "The encryption permissions deny content extraction for accessibility \
                           (bit 10 of /P is clear)."
                .into();
            out.violation("PUI-26-002", "", None, DOC, message);
        }
        _ => {
            let message = "The encryption dictionary has no /P permissions entry.".into();
            out.violation("PUI-26-001", "", None, DOC, message);
        }
    }
}
