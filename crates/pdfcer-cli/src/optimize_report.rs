//! The per-stream lines of `pdfcer optimize`'s report: each change, with
//! its downsample detail, and each skip.

use std::fmt::Write as _;

use pdfcer_core::optimize::{ImageChange, OptimizePlan, SettingSource, Sourced};

/// The changed and skipped streams, as text.
pub(crate) fn changes_text(plan: &OptimizePlan) -> String {
    let mut out = String::new();
    for c in &plan.changes {
        let _ = writeln!(
            out,
            "  {:<8} {:<20} [{}] -> {}  {} -> {} B{}",
            c.id.to_string(),
            c.category.as_str(),
            if c.filters_before.is_empty() {
                "none".to_owned()
            } else {
                c.filters_before.join(" ")
            },
            c.action,
            c.before_bytes,
            c.after_bytes,
            if c.lossy { "  (lossy)" } else { "" }
        );
        if let Some(img) = &c.image {
            out.push_str(&image_text(img));
        }
    }
    if !plan.skipped.is_empty() {
        let _ = writeln!(out, "skipped: {}", plan.skipped.len());
        for s in &plan.skipped {
            let _ = writeln!(out, "  {:<8} {}: {}", s.id.to_string(), s.action, s.reason);
        }
    }
    out
}

/// The downsample line under a change: sizes, the placement that limited
/// it, and where the method and target came from.
fn image_text(img: &ImageChange) -> String {
    let (bw, bh) = img.size_before;
    let (aw, ah) = img.size_after;
    let s = &img.settings;
    let mut line = format!(
        "           {bw}x{bh} -> {aw}x{ah} px, {} ({})",
        s.method.value, s.method.source
    );
    if let Some(p) = s.ppi {
        line.push_str(&format!(", target {} ppi ({})", p.value, p.source));
    }
    if let (Some(b), Some(after)) = (&img.binding, img.binding_ppi_after) {
        line.push_str(&format!(
            "; binding use page {}, {:.0}x{:.0} pt, {:.0} -> {after:.0} ppi",
            b.page,
            b.width_pt,
            b.height_pt,
            b.effective_ppi()
        ));
    }
    line.push('\n');
    line
}

/// One downsample for `--json`: sizes, the binding placement, and each
/// setting as `{"value", "source"}`.
pub(crate) fn image_json(img: &ImageChange) -> String {
    let s = &img.settings;
    let src = |v: String, from: SettingSource| format!("{{\"value\":{v},\"source\":\"{from}\"}}");
    let opt = |v: Option<Sourced<u16>>| {
        v.map_or_else(|| "null".to_owned(), |v| src(v.value.to_string(), v.source))
    };
    let binding = img.binding.as_ref().map_or_else(
        || "null".to_owned(),
        |b| {
            format!(
                "{{\"page\":{},\"width_pt\":{:.2},\"height_pt\":{:.2},\"ppi_before\":{:.2},\"ppi_after\":{:.2}}}",
                b.page,
                b.width_pt,
                b.height_pt,
                b.effective_ppi(),
                img.binding_ppi_after.unwrap_or(0.0)
            )
        },
    );
    format!(
        "{{\"size_before\":[{},{}],\"size_after\":[{},{}],\"binding\":{binding},\"method\":{},\"ppi\":{},\"above\":{},\"encode\":{},\"min_saving_pct\":{}}}",
        img.size_before.0,
        img.size_before.1,
        img.size_after.0,
        img.size_after.1,
        src(format!("\"{}\"", s.method.value), s.method.source),
        opt(s.ppi),
        opt(s.above),
        src(format!("\"{}\"", s.encode.value), s.encode.source),
        src(s.min_saving_pct.value.to_string(), s.min_saving_pct.source)
    )
}
