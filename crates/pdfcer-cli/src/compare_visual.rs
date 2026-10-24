//! `compare --visual` (Pass 645.2): pages matched by appearance, changed
//! regions found by rendering, reported and (with `--output`) marked.

use std::fmt::Write as _;

use pdfcer_core::compare::{RegionMark, RegionReport};
use pdfcer_core::document::Document;
use pdfcer_core::page_tree::Rect;
use pdfcer_render::{PageMatch, PagePair, VisualCompareOptions, VisualCompareReport};

use super::compare_cmd::CompareArgs;

/// 0-based page selections `(old, new)`; `None` is every page.
pub(crate) type PageLists = (Option<Vec<usize>>, Option<Vec<usize>>);

/// The `--visual` arguments, flattened into `compare`.
#[derive(Debug, Clone, clap::Args)]
pub(crate) struct VisualArgs {
    /// Also compare how the pages look: pages are matched by appearance
    /// (inserted, deleted and moved pages found), each matched pair is
    /// rendered, and changed areas are reported as tight boxes. For
    /// drawings and scans, where the text alone misses changes.
    #[arg(long)]
    pub(crate) visual: bool,
    /// With --visual: render resolution. Default 108.
    #[arg(long, value_name = "DPI", requires = "visual")]
    pub(crate) dpi: Option<f32>,
    /// With --visual: how far (0-255) a pixel's colour must move to count
    /// as changed. Default 48; raise it for noisy scans.
    #[arg(long, value_name = "LEVELS", requires = "visual")]
    pub(crate) tolerance: Option<u8>,
    /// With --visual: ignore changed areas smaller than this many pixels.
    /// Default 6.
    #[arg(long, value_name = "PIXELS", requires = "visual")]
    pub(crate) min_region: Option<u64>,
    /// With --visual: how alike (0-1) two pages must look to be paired.
    /// Default 0.5.
    #[arg(long, value_name = "SIMILARITY", requires = "visual")]
    pub(crate) match_threshold: Option<f64>,
    /// With --visual: pair pages yourself instead of matching them, as
    /// OLD=NEW pairs, 1-based: 1=1,2=3. Unlisted pages count as inserted or
    /// deleted.
    #[arg(long, value_name = "PAIRS", requires = "visual")]
    pub(crate) map: Option<String>,
}

/// Build the options from `args`; page lists are 0-based, already parsed.
pub(crate) fn options(args: &VisualArgs, pages: PageLists) -> Result<VisualCompareOptions, String> {
    let mut o = VisualCompareOptions::default().with_pages(pages.0, pages.1);
    if let Some(dpi) = args.dpi {
        if !(dpi.is_finite() && (10.0..=600.0).contains(&dpi)) {
            return Err(format!("--dpi {dpi}: must be 10-600"));
        }
        o = o.with_scale(dpi / 72.0);
    }
    if let Some(t) = args.tolerance {
        o = o.with_threshold(t);
    }
    if let Some(m) = args.min_region {
        o = o.with_min_pixels(m.max(1));
    }
    if let Some(m) = args.match_threshold {
        if !(0.0..=1.0).contains(&m) {
            return Err(format!("--match-threshold {m}: must be 0-1"));
        }
        o = o.with_match_threshold(m);
    }
    if let Some(spec) = &args.map {
        o = o.with_mapping(parse_map(spec)?);
    }
    Ok(o)
}

/// `1=1,2=3` as 0-based pairs.
fn parse_map(spec: &str) -> Result<Vec<(usize, usize)>, String> {
    spec.split(',')
        .map(|pair| {
            let (a, b) = pair
                .split_once('=')
                .ok_or_else(|| format!("--map {pair:?}: expected OLD=NEW"))?;
            let n = |s: &str| match s.trim().parse::<usize>() {
                Ok(v) if v >= 1 => Ok(v - 1),
                _ => Err(format!("--map {pair:?}: pages are numbers from 1")),
            };
            Ok((n(a)?, n(b)?))
        })
        .collect()
}

/// Whether the visual comparison found any difference.
pub(crate) fn differs(r: &VisualCompareReport) -> bool {
    r.pairs
        .iter()
        .any(|p| !p.regions.is_empty() || p.old.is_none() || p.new.is_none() || p.error.is_some())
}

fn page(p: Option<usize>) -> String {
    p.map_or_else(|| "-".to_owned(), |i| (i + 1).to_string())
}

fn how(p: &PagePair) -> String {
    match p.how {
        PageMatch::Inserted => format!("new p{} inserted (no matching old page)", page(p.new)),
        PageMatch::Deleted => format!("old p{} deleted (no matching new page)", page(p.old)),
        PageMatch::Moved => format!("old p{} moved to new p{}", page(p.old), page(p.new)),
        PageMatch::Mapped => format!("old p{} = new p{} (mapped)", page(p.old), page(p.new)),
        _ => format!("old p{} = new p{}", page(p.old), page(p.new)),
    }
}

fn settings(r: &VisualCompareReport) -> String {
    format!(
        "{:.0} dpi, tolerance {}, regions merged within {} pt and under {} px dropped; {}",
        r.scale * 72.0,
        r.threshold,
        r.merge_gap,
        r.min_pixels,
        if r.matched_automatically {
            format!(
                "pages matched by appearance (paired at similarity {} or more)",
                r.match_threshold
            )
        } else {
            "pages paired by --map".to_owned()
        }
    )
}

/// The human report: settings, then one line per page row.
pub(crate) fn report_text(r: &VisualCompareReport) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "visual: {} changed region(s) over {} page row(s); {}",
        r.region_count(),
        r.pairs.len(),
        settings(r)
    );
    for p in &r.pairs {
        let sim = p
            .similarity
            .map(|s| format!(", similarity {s:.2}"))
            .unwrap_or_default();
        let size = if p.size_differs {
            ", page sizes differ (compared anchored top-left)"
        } else {
            ""
        };
        let tail = match &p.error {
            Some(e) => format!(": not compared: {e}"),
            None if p.old.is_some() && p.new.is_some() => {
                format!(": {} region(s)", p.regions.len())
            }
            None => String::new(),
        };
        let _ = writeln!(out, "  {}{sim}{size}{tail}", how(p));
    }
    if r.nothing_matched() {
        let _ = writeln!(
            out,
            "note: no page of one document looked like any page of the other; the rows above \
             mean \"no match found\", not \"every page changed\". Try --map or a lower \
             --match-threshold."
        );
    }
    out
}

/// The summary-page paragraph for the marked result.
pub(crate) fn summary_note(r: &VisualCompareReport) -> String {
    let mut s = format!(
        "Visual comparison: {} changed area(s), each an orange cloud with a note; a blue frame \
         marks a page with no match in the old document. {}.\n",
        r.region_count(),
        settings(r)
    );
    for p in r.pairs.iter().filter(|p| p.how != PageMatch::InOrder) {
        let _ = writeln!(s, "{}", how(p));
    }
    if r.nothing_matched() {
        s.push_str("No page of one document looked like any page of the other.\n");
    }
    s
}

/// The marks for the result, in page order.
pub(crate) fn marks(r: &VisualCompareReport, new: &Document) -> Vec<RegionMark> {
    let boxes: Vec<Rect> = pdfcer_core::page_tree::pages(new)
        .map(|p| p.iter().map(|p| p.crop_box).collect())
        .unwrap_or_default();
    let mut out = Vec::new();
    for p in &r.pairs {
        let Some(n) = p.new else { continue };
        if p.old.is_none() {
            if let Some(b) = boxes.get(n) {
                out.push(RegionMark::inserted_page(
                    n,
                    *b,
                    "Inserted page: no page of the old document looks like it.",
                ));
            }
            continue;
        }
        let from = format!("old page {}", page(p.old));
        for g in &p.regions {
            out.push(RegionMark::changed(
                n,
                g.rect,
                format!(
                    "Changed appearance (compared with {from}; {} px)",
                    g.changed_pixels
                ),
            ));
        }
    }
    out
}

/// The JSON object for `--json`.
pub(crate) fn report_json(r: &VisualCompareReport) -> String {
    let rows: Vec<String> = r.pairs.iter().map(pair_json).collect();
    format!(
        "{{\"dpi\":{},\"tolerance\":{},\"merge_gap\":{},\"min_region\":{},\
         \"match_threshold\":{},\"matched_automatically\":{},\"nothing_matched\":{},\
         \"pages\":[{}]}}",
        (r.scale * 72.0).round(),
        r.threshold,
        r.merge_gap,
        r.min_pixels,
        r.match_threshold,
        r.matched_automatically,
        r.nothing_matched(),
        rows.join(",")
    )
}

fn pair_json(p: &PagePair) -> String {
    let n = |v: Option<usize>| v.map_or_else(|| "null".to_owned(), |i| (i + 1).to_string());
    let how = match p.how {
        PageMatch::Inserted => "inserted",
        PageMatch::Deleted => "deleted",
        PageMatch::Moved => "moved",
        PageMatch::Mapped => "mapped",
        _ => "in_order",
    };
    let r = |v: f64| (v * 100.0).round() / 100.0;
    let regions: Vec<String> = p
        .regions
        .iter()
        .map(|g| {
            format!(
                "{{\"rect\":[{},{},{},{}],\"changed_pixels\":{}}}",
                r(g.rect.llx),
                r(g.rect.lly),
                r(g.rect.urx),
                r(g.rect.ury),
                g.changed_pixels
            )
        })
        .collect();
    format!(
        "{{\"old\":{},\"new\":{},\"how\":\"{how}\",\"similarity\":{},\"size_differs\":{},\
         \"error\":{},\"regions\":[{}]}}",
        n(p.old),
        n(p.new),
        p.similarity
            .map_or_else(|| "null".to_owned(), |s| format!("{:.3}", s)),
        p.size_differs,
        p.error.as_ref().map_or_else(
            || "null".to_owned(),
            |e| format!("\"{}\"", crate::extract::json_escape(e))
        ),
        regions.join(",")
    )
}

/// Print a region-mark outcome's leftovers.
pub(crate) fn report_unmarked(r: &RegionReport) {
    for u in &r.unmarked {
        eprintln!(
            "pdfcer: compare: visual region {} was not marked: {}",
            u.change + 1,
            u.reason
        );
    }
}

/// Run the visual comparison when `--visual` was given.
pub(crate) fn run(
    args: &CompareArgs,
    old: &Document,
    new: &Document,
    pages: PageLists,
) -> Result<Option<VisualCompareReport>, String> {
    if !args.visual.visual {
        return Ok(None);
    }
    let o = options(&args.visual, pages)?;
    pdfcer_render::visual_compare(old, new, &o)
        .map(Some)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_map_is_one_based_pairs() {
        assert_eq!(parse_map("1=1, 2=3").unwrap(), [(0, 0), (1, 2)]);
        assert!(parse_map("0=1").is_err());
        assert!(parse_map("1-2").is_err());
    }
}
