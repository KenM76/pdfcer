//! `optimize`'s report of what clean-up and `--discard` remove.

use std::fmt::Write as _;

use pdfcer_core::doc_metadata::MetadataItem;
use pdfcer_core::optimize::{CleanupPlan, ObjectDiscard};

use crate::extract::json_escape;

/// The text report's cleanup section: what the plan removes or rewrites.
pub(crate) fn cleanup_text(c: &CleanupPlan) -> String {
    let mut out = String::new();
    if c.is_empty() {
        return out;
    }
    let _ = writeln!(
        out,
        "cleanup: {} link(s), {} bookmark(s), {} named destination(s) removed",
        c.links.len(),
        c.bookmarks.len(),
        c.destinations.len()
    );
    for l in &c.links {
        let _ = writeln!(
            out,
            "  link     {:<8} page {}: {}",
            l.id.to_string(),
            l.page_index + 1,
            l.target
        );
    }
    for b in &c.bookmarks {
        let why = b.target.as_ref().map_or_else(
            || "groups only removed items".to_owned(),
            ToString::to_string,
        );
        let _ = writeln!(
            out,
            "  bookmark {:<8} {:?}: {why}{}",
            b.id.to_string(),
            b.title,
            match b.descendants {
                0 => String::new(),
                n => format!(" (with {n} under it)"),
            }
        );
    }
    for d in &c.destinations {
        let _ = writeln!(out, "  dest     {:?}", String::from_utf8_lossy(d));
    }
    out
}

/// The text report's metadata-discard lines, one per item.
pub(crate) fn discard_text(items: &[MetadataItem]) -> String {
    let mut out = String::new();
    if items.is_empty() {
        return out;
    }
    let _ = writeln!(out, "discard: {} item(s) removed", items.len());
    for i in items {
        let _ = writeln!(
            out,
            "  {:<17} {}  {}, {} B",
            i.kind.as_str(),
            i.id,
            i.location,
            i.bytes
        );
    }
    out
}

/// The text report's per-object discard lines.
pub(crate) fn discard_objects_text(items: &[ObjectDiscard]) -> String {
    let mut out = String::new();
    if items.is_empty() {
        return out;
    }
    let _ = writeln!(out, "discard objects: {} removed", items.len());
    for i in items {
        // string-gap-exempt: the report pads its class column
        let _ = writeln!(
            out,
            "  {:<17} {}, {} B",
            i.object.as_str(),
            i.location,
            i.bytes
        );
    }
    out
}

/// The JSON report's per-object discard array.
pub(crate) fn discard_objects_json(items: &[ObjectDiscard]) -> String {
    let q = |s: &str| format!("\"{}\"", json_escape(s));
    let rows: Vec<String> = items
        .iter()
        .map(|i| {
            format!(
                "{{\"object\":{},\"location\":{},\"bytes\":{}}}",
                q(i.object.as_str()),
                q(&i.location),
                i.bytes
            )
        })
        .collect();
    format!("[{}]", rows.join(","))
}

/// The JSON report's metadata-discard array.
pub(crate) fn discard_json(items: &[MetadataItem]) -> String {
    let q = |s: &str| format!("\"{}\"", json_escape(s));
    let rows: Vec<String> = items
        .iter()
        .map(|i| {
            format!(
                "{{\"id\":{},\"kind\":{},\"location\":{},\"bytes\":{}}}",
                q(i.id.as_str()),
                q(i.kind.as_str()),
                q(&i.location),
                i.bytes
            )
        })
        .collect();
    format!("[{}]", rows.join(","))
}

/// The JSON report's `cleanup` value.
pub(crate) fn cleanup_json(c: &CleanupPlan) -> String {
    let q = |s: &str| format!("\"{}\"", json_escape(s));
    let links: Vec<String> = c
        .links
        .iter()
        .map(|l| {
            format!(
                "{{\"id\":{},\"page\":{},\"reason\":{}}}",
                q(&l.id.to_string()),
                l.page_index + 1,
                q(&l.target.to_string())
            )
        })
        .collect();
    let bookmarks: Vec<String> = c
        .bookmarks
        .iter()
        .map(|b| {
            format!(
                "{{\"id\":{},\"title\":{},\"reason\":{},\"descendants\":{}}}",
                q(&b.id.to_string()),
                q(&b.title),
                b.target
                    .as_ref()
                    .map_or_else(|| "null".to_owned(), |t| q(&t.to_string())),
                b.descendants
            )
        })
        .collect();
    let dests: Vec<String> = c
        .destinations
        .iter()
        .map(|d| q(&String::from_utf8_lossy(d)))
        .collect();
    format!(
        "{{\"links\":[{}],\"bookmarks\":[{}],\"destinations\":[{}]}}",
        links.join(","),
        bookmarks.join(","),
        dests.join(",")
    )
}
