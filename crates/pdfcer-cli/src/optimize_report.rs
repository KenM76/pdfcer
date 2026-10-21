//! The per-stream lines of `pdfcer optimize`'s report: each change, with
//! its downsample detail, and each skip.

use std::fmt::Write as _;

use pdfcer_core::optimize::{
    Conversion, ImageChange, ImageConvert, ImageEncode, OptimizePlan, SettingSource, Sourced,
};

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
            out.push_str(&format_text(img, &c.filters_before));
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

/// The image line under a change: sizes, the placement that limited a
/// downsample, where the method and target came from, and the encoding,
/// JPEG quality and bit depth written.
fn image_text(img: &ImageChange) -> String {
    let (bw, bh) = img.size_before;
    let (aw, ah) = img.size_after;
    let s = &img.settings;
    let mut line = format!("           {bw}x{bh} -> {aw}x{ah} px");
    if img.size_before != img.size_after {
        line.push_str(&format!(", {} ({})", s.method.value, s.method.source));
    }
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
    line.push_str(&format!("; written as {}", img.encoded_as));
    if let Some(q) = img.jpeg {
        line.push_str(&format!(" quality {} (asked {}", q.used, q.asked));
        if let Some(e) = q.source_estimate {
            line.push_str(&format!(", source about {e}"));
        }
        line.push(')');
    }
    if img.bpc.0 != img.bpc.1 {
        line.push_str(&format!(", {} -> {} bit", img.bpc.0, img.bpc.1));
    }
    line.push('\n');
    line
}

/// The format line: filter, colour space and bit depth before and after,
/// and what a colour conversion did.
fn format_text(img: &ImageChange, filters: &[String]) -> String {
    let short = |f: &str| f.trim_end_matches("Decode").to_owned();
    let before = filters
        .last()
        .map_or_else(|| "raw".to_owned(), |f| short(f));
    let after = if img.encoded_as == ImageEncode::Jpeg {
        "DCT"
    } else {
        "Flate"
    };
    let (cb, ca) = &img.color_space;
    let mut line = format!(
        "           format {before}/{cb} {} bit -> {after}/{ca} {} bit",
        img.bpc.0, img.bpc.1
    );
    if let Some(c) = &img.conversion {
        line.push_str(&conversion_text(c));
    }
    line.push('\n');
    line
}

fn conversion_text(c: &Conversion) -> String {
    let mut out = match c.formula {
        Some(f) => format!("; converted from {} by {f}", c.from),
        None => format!("; converted from {}", c.from),
    };
    if c.not_colour_managed {
        out.push_str(" (not colour-managed)");
    }
    if c.to == ImageConvert::Bilevel {
        match c.threshold {
            Some(t) => out.push_str(&format!(", threshold {t}")),
            None => out.push_str(", Floyd-Steinberg dither"),
        }
    }
    if let Some(n) = c.colors {
        let how = if c.exact {
            "every colour kept"
        } else {
            "median cut, lossy"
        };
        out.push_str(&format!(", {n} colours ({how})"));
    }
    out
}

fn conversion_json(img: &ImageChange) -> String {
    let Some(c) = &img.conversion else {
        return "null".to_owned();
    };
    let opt = |v: Option<String>| v.unwrap_or_else(|| "null".to_owned());
    format!(
        "{{\"to\":\"{}\",\"from\":\"{}\",\"formula\":{},\"not_colour_managed\":{},\"threshold\":{},\"colors\":{},\"exact\":{}}}",
        c.to,
        c.from,
        opt(c.formula.map(|f| format!("\"{f}\""))),
        c.not_colour_managed,
        opt(c.threshold.map(|t| t.to_string())),
        opt(c.colors.map(|n| n.to_string())),
        c.exact
    )
}

/// One image change for `--json`: sizes, the binding placement, the
/// encoding, bit depth and JPEG quality written, and each setting as
/// `{"value", "source"}`.
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
    let jpeg = img.jpeg.map_or_else(
        || "null".to_owned(),
        |q| {
            let est = q
                .source_estimate
                .map_or("null".to_owned(), |e| e.to_string());
            format!(
                "{{\"asked\":{},\"used\":{},\"source_estimate\":{est}}}",
                q.asked, q.used
            )
        },
    );
    format!(
        "{{\"size_before\":[{},{}],\"size_after\":[{},{}],\"binding\":{binding},\"encoded_as\":\"{}\",\"bpc\":[{},{}],\"jpeg\":{jpeg},\"method\":{},\"ppi\":{},\"above\":{},\"encode\":{},\"quality\":{},\"chroma\":{},\"progressive\":{},\"min_saving_pct\":{},\"color_space\":[\"{}\",\"{}\"],\"conversion\":{},\"convert\":{},\"threshold\":{},\"colors\":{}}}",
        img.size_before.0,
        img.size_before.1,
        img.size_after.0,
        img.size_after.1,
        img.encoded_as,
        img.bpc.0,
        img.bpc.1,
        src(format!("\"{}\"", s.method.value), s.method.source),
        opt(s.ppi),
        opt(s.above),
        src(format!("\"{}\"", s.encode.value), s.encode.source),
        src(s.quality.value.to_string(), s.quality.source),
        src(format!("\"{}\"", s.chroma.value), s.chroma.source),
        src(s.progressive.value.to_string(), s.progressive.source),
        src(s.min_saving_pct.value.to_string(), s.min_saving_pct.source),
        img.color_space.0,
        img.color_space.1,
        conversion_json(img),
        src(format!("\"{}\"", s.convert.value), s.convert.source),
        src(format!("\"{}\"", s.threshold.value), s.threshold.source),
        src(s.colors.value.to_string(), s.colors.source)
    )
}
