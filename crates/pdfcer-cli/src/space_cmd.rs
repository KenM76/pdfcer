//! `audit-space`: where a PDF's bytes go (Pass 616.0).

use std::fmt::Write as _;

use pdfcer_core::space_audit::{self, ObjectSpace, SpaceAudit, SpaceDiff};

use super::*;

/// Print the space audit of `input`, or its category diff against `other`.
pub(crate) fn cmd_audit_space(input: &Path, other: Option<&Path>, top: usize, json: bool) -> u8 {
    let report = match audit_file(input) {
        Ok(r) => r,
        Err(code) => return code,
    };
    if let Some(other) = other {
        let after = match audit_file(other) {
            Ok(r) => r,
            Err(code) => return code,
        };
        let d = space_audit::diff(&report, &after);
        print!("{}", if json { diff_json(&d) } else { diff_text(&d) });
        return exit::SUCCESS;
    }
    print!(
        "{}",
        if json {
            audit_json(&report, top)
        } else {
            audit_text(&report, top)
        }
    );
    exit::SUCCESS
}

fn audit_file(path: &Path) -> Result<SpaceAudit, u8> {
    match open_document(path) {
        Ok(doc) => Ok(space_audit::audit(&doc)),
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", path.display());
            Err(exit_code_for_doc(&err))
        }
    }
}

fn percent(part: u64, whole: u64) -> f64 {
    if whole == 0 {
        0.0
    } else {
        part as f64 * 100.0 / whole as f64
    }
}

fn audit_text(report: &SpaceAudit, top: usize) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "file bytes: {}", report.file_bytes);
    let _ = writeln!(
        out,
        "{:<22} {:>12} {:>7} {:>8}",
        "category", "bytes", "%", "objects"
    );
    for c in report
        .categories
        .iter()
        .filter(|c| c.bytes > 0 || c.objects > 0)
    {
        let _ = writeln!(
            out,
            "{:<22} {:>12} {:>6.1}% {:>8}",
            c.category.as_str(),
            c.bytes,
            percent(c.bytes, report.file_bytes),
            c.objects
        );
    }
    if top > 0 && !report.objects.is_empty() {
        let _ = writeln!(out, "\nlargest objects:");
        for o in report.objects.iter().take(top) {
            let _ = writeln!(out, "  {}", object_line(o));
        }
    }
    let images: Vec<&ObjectSpace> = report
        .objects
        .iter()
        .filter(|o| o.image.is_some())
        .collect();
    if !images.is_empty() {
        let _ = writeln!(out, "\nimages:");
        for o in images {
            let _ = writeln!(out, "  {}", image_line(o));
        }
    }
    for note in &report.notes {
        let _ = writeln!(out, "note: {note}");
    }
    out
}

fn object_line(o: &ObjectSpace) -> String {
    let mut line = format!(
        "{:<8} {:<20} {:>10} B",
        o.id.to_string(),
        o.category.as_str(),
        o.stored_bytes
    );
    if let Some(container) = o.in_object_stream {
        let _ = write!(line, "  in objstm {container}");
    }
    if let Some(n) = o.stream_bytes {
        let _ = write!(line, "  stream {n} B");
    }
    if !o.filters.is_empty() {
        let _ = write!(line, "  [{}]", o.filters.join(" "));
    }
    for name in [&o.type_name, &o.subtype].into_iter().flatten() {
        let _ = write!(line, " /{name}");
    }
    if !o.pages.is_empty() {
        let _ = write!(line, "  pages {}", page_ranges(&o.pages));
    }
    line
}

/// `[1, 2, 3, 5]` as `1-3,5`.
fn page_ranges(pages: &[usize]) -> String {
    let mut runs: Vec<(usize, usize)> = Vec::new();
    for &p in pages {
        match runs.last_mut() {
            Some((_, end)) if *end + 1 == p => *end = p,
            _ => runs.push((p, p)),
        }
    }
    let parts: Vec<String> = runs
        .iter()
        .map(|&(a, b)| {
            if a == b {
                a.to_string()
            } else {
                format!("{a}-{b}")
            }
        })
        .collect();
    parts.join(",")
}

fn image_line(o: &ObjectSpace) -> String {
    let Some(img) = &o.image else {
        return String::new();
    };
    let mut line = format!(
        "{:<8} {}x{} {} {}  {} B",
        o.id.to_string(),
        img.width,
        img.height,
        img.color_space.as_deref().unwrap_or("-"),
        img.bits_per_component
            .map_or("-".to_owned(), |b| format!("{b}bpc")),
        o.stored_bytes
    );
    if !o.filters.is_empty() {
        let _ = write!(line, " [{}]", o.filters.join(" "));
    }
    if img.placements.is_empty() {
        line.push_str("  not drawn by page content");
    }
    for p in &img.placements {
        let _ = write!(
            line,
            "  p{} {:.0}x{:.0}pt {:.0}ppi",
            p.page,
            p.width_pt,
            p.height_pt,
            p.effective_ppi()
        );
    }
    line
}

fn diff_text(d: &SpaceDiff) -> String {
    let mut out = String::new();
    let change = |before: u64, after: u64| after as i128 - before as i128;
    let _ = writeln!(
        out,
        "file bytes: {} -> {} ({:+} B, {:+.1}%)",
        d.before_bytes,
        d.after_bytes,
        change(d.before_bytes, d.after_bytes),
        percent(d.after_bytes, d.before_bytes) - 100.0
    );
    let _ = writeln!(
        out,
        "{:<22} {:>12} {:>12} {:>12}",
        "category", "before", "after", "change"
    );
    for c in d.categories.iter().filter(|c| c.before > 0 || c.after > 0) {
        let _ = writeln!(
            out,
            "{:<22} {:>12} {:>12} {:>+12}",
            c.category.as_str(),
            c.before,
            c.after,
            change(c.before, c.after)
        );
    }
    out
}

fn audit_json(report: &SpaceAudit, top: usize) -> String {
    let categories: Vec<String> = report
        .categories
        .iter()
        .map(|c| {
            format!(
                "{{\"category\":\"{}\",\"bytes\":{},\"objects\":{}}}",
                c.category.as_str(),
                c.bytes,
                c.objects
            )
        })
        .collect();
    let objects: Vec<String> = report.objects.iter().take(top).map(object_json).collect();
    let images: Vec<String> = report
        .objects
        .iter()
        .filter(|o| o.image.is_some())
        .map(object_json)
        .collect();
    let notes: Vec<String> = report
        .notes
        .iter()
        .map(|n| format!("\"{}\"", json_escape(n)))
        .collect();
    format!(
        "{{\"file_bytes\":{},\"categories\":[{}],\"largest\":[{}],\"images\":[{}],\"notes\":[{}]}}\n",
        report.file_bytes,
        categories.join(","),
        objects.join(","),
        images.join(","),
        notes.join(",")
    )
}

fn opt_str(v: Option<&str>) -> String {
    v.map_or("null".to_owned(), |s| format!("\"{}\"", json_escape(s)))
}

fn object_json(o: &ObjectSpace) -> String {
    let filters: Vec<String> = o
        .filters
        .iter()
        .map(|f| format!("\"{}\"", json_escape(f)))
        .collect();
    let image = o.image.as_ref().map_or("null".to_owned(), |img| {
        let placements: Vec<String> = img
            .placements
            .iter()
            .map(|p| {
                format!(
                    "{{\"page\":{},\"width_pt\":{:.3},\"height_pt\":{:.3},\"ppi_x\":{:.2},\"ppi_y\":{:.2}}}",
                    p.page, p.width_pt, p.height_pt, p.ppi_x, p.ppi_y
                )
            })
            .collect();
        format!(
            "{{\"width\":{},\"height\":{},\"bits_per_component\":{},\"color_space\":{},\"placements\":[{}]}}",
            img.width,
            img.height,
            img.bits_per_component.map_or("null".to_owned(), |b| b.to_string()),
            opt_str(img.color_space.as_deref()),
            placements.join(",")
        )
    });
    format!(
        "{{\"object\":{},\"generation\":{},\"category\":\"{}\",\"stored_bytes\":{},\"object_stream\":{},\"stream_bytes\":{},\"filters\":[{}],\"type\":{},\"subtype\":{},\"pages\":[{}],\"image\":{}}}",
        o.id.num,
        o.id.generation,
        o.category.as_str(),
        o.stored_bytes,
        o.in_object_stream
            .map_or("null".to_owned(), |n| n.to_string()),
        o.stream_bytes.map_or("null".to_owned(), |n| n.to_string()),
        filters.join(","),
        opt_str(o.type_name.as_deref()),
        opt_str(o.subtype.as_deref()),
        o.pages
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(","),
        image
    )
}

fn diff_json(d: &SpaceDiff) -> String {
    let rows: Vec<String> = d
        .categories
        .iter()
        .map(|c| {
            format!(
                "{{\"category\":\"{}\",\"before\":{},\"after\":{}}}",
                c.category.as_str(),
                c.before,
                c.after
            )
        })
        .collect();
    format!(
        "{{\"before_bytes\":{},\"after_bytes\":{},\"categories\":[{}]}}\n",
        d.before_bytes,
        d.after_bytes,
        rows.join(",")
    )
}
