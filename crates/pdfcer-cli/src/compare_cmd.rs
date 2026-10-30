//! `compare`: the text differences between two PDFs (Pass 645.0), and with
//! `--visual` the appearance differences (Pass 645.2, `compare_visual`).

use std::fmt::Write as _;

use pdfcer_core::compare::{
    Change, ChangeKind, CompareError, CompareOptions, CompareReport, Place, RegionReport,
    ResultOptions, ResultReport, Side, compare_documents, mark_changes, mark_regions,
};
use pdfcer_core::page_tree::Rect;
use pdfcer_core::text_extract::TextColor;
use pdfcer_render::VisualCompareReport;

use super::*;
use crate::edit_common::{finish_edit, save_edited};
use crate::extract::json_escape;

/// The `compare` arguments.
#[derive(Debug, Clone, clap::Args)]
pub(crate) struct CompareArgs {
    /// The earlier version.
    pub(crate) old: PathBuf,
    /// The later version.
    pub(crate) new: PathBuf,
    /// Pages of OLD to compare, 1-based: all, 3, 2-5,8. Default all.
    #[arg(long, value_name = "PAGES")]
    pub(crate) pages_old: Option<String>,
    /// Pages of NEW to compare. Default all.
    #[arg(long, value_name = "PAGES")]
    pub(crate) pages_new: Option<String>,
    /// Also report words whose text is the same but whose size, weight or
    /// colour changed.
    #[arg(long)]
    pub(crate) formatting: bool,
    /// Also compare artifact text (running heads, page numbers, watermarks).
    #[arg(long)]
    pub(crate) artifacts: bool,
    /// Read NEW's password from this file's first line (- for stdin).
    #[arg(long, value_name = "FILE")]
    pub(crate) new_password_file: Option<PathBuf>,
    /// Print the report as JSON.
    #[arg(long)]
    pub(crate) json: bool,
    /// Exit 16 when the text differs.
    #[arg(long)]
    pub(crate) exit_code: bool,
    /// Also write a copy of NEW with every change marked: blue highlight
    /// inserted, orange highlight replaced, red caret deleted, purple
    /// underline formatting, each with a note holding the old text. OLD and
    /// NEW are not modified.
    #[arg(long, value_name = "RESULT.pdf")]
    pub(crate) output: Option<PathBuf>,
    /// With --output: leave out the summary page (counts and colour key)
    /// that is otherwise inserted before page 1.
    #[arg(long, requires = "output")]
    pub(crate) no_summary: bool,
    /// With --output: the author written on every mark.
    #[arg(long, value_name = "NAME", requires = "output")]
    pub(crate) author: Option<String>,
    #[command(flatten)]
    pub(crate) visual: crate::compare_visual::VisualArgs,
}

/// Run `compare`: open both files, diff their text, print the report and
/// map the outcome to an exit code (16 only under `--exit-code`).
pub(crate) fn cmd_compare(args: &CompareArgs) -> u8 {
    if let Some(out) = &args.output
        && (same_file(out, &args.old) || same_file(out, &args.new))
    {
        eprintln!(
            "pdfcer: compare: --output must not be OLD or NEW; the sources are never modified"
        );
        return 2;
    }
    let new_password = match &args.new_password_file {
        Some(path) => match resolve_cli_password(None, Some(path.clone())) {
            Ok(p) => p,
            Err(message) => {
                eprintln!("pdfcer: compare: {message}");
                return exit::IO_ERROR;
            }
        },
        None => cli_password().map(<[u8]>::to_vec),
    };
    let old = match open_document(&args.old) {
        Ok(d) => d,
        Err(err) => return open_failed(&args.old, &err),
    };
    let new = match open_document_with(&args.new, new_password.as_deref()) {
        Ok(d) => d,
        Err(err) => return open_failed(&args.new, &err),
    };
    let (options, pages) = match options(args, &old, &new) {
        Ok(o) => o,
        Err(message) => {
            eprintln!("pdfcer: compare: {message}");
            return 2;
        }
    };
    let report = match compare_documents(&old, &new, &options) {
        Ok(r) => r,
        Err(err) => {
            eprintln!("pdfcer: compare: {err}");
            return match err {
                CompareError::CopyNotPermitted { .. } => exit::EDIT_REFUSED,
                _ => exit::RUNTIME_ERROR,
            };
        }
    };
    let visual = match crate::compare_visual::run(args, &old, &new, pages) {
        Ok(v) => v,
        Err(message) => {
            eprintln!("pdfcer: compare: --visual: {message}");
            return exit::RUNTIME_ERROR;
        }
    };
    print_reports(args.json, &report, visual.as_ref());
    if let Some(output) = &args.output
        && let Err(code) = write_result(args, output, new, &report, visual.as_ref())
    {
        return code;
    }
    let visual_differs = visual.as_ref().is_some_and(crate::compare_visual::differs);
    if args.exit_code && (!report.is_identical() || visual_differs) {
        exit::DOCUMENTS_DIFFER
    } else {
        exit::SUCCESS
    }
}

fn print_reports(json: bool, report: &CompareReport, visual: Option<&VisualCompareReport>) {
    if json {
        let text = report_json(report);
        match visual {
            Some(v) => println!(
                "{},\"visual\":{}}}",
                text.strip_suffix('}').unwrap_or(&text),
                crate::compare_visual::report_json(v)
            ),
            None => println!("{text}"),
        }
    } else {
        print!("{}", report_text(report));
        if let Some(v) = visual {
            print!("{}", crate::compare_visual::report_text(v));
        }
    }
}

/// Mark `report` (and `visual`'s regions) on a copy of `new` and save it to
/// `output` as an incremental update; prints what was marked and what was
/// not.
fn write_result(
    args: &CompareArgs,
    output: &Path,
    new: Document,
    report: &CompareReport,
    visual: Option<&VisualCompareReport>,
) -> Result<(), u8> {
    let label = |p: &Path| {
        p.file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
    };
    let mut options = ResultOptions::default()
        .with_summary_page(!args.no_summary)
        .with_labels(label(&args.old), label(&args.new));
    if let Some(a) = &args.author {
        options = options.with_author(a.clone());
    }
    if let Some(v) = visual {
        options = options.with_summary_note(v.summary_note());
    }
    let region_marks = visual.map(|v| v.region_marks(&new));
    let mut session = pdfcer_core::edit::EditSession::new(new);
    // Regions first: the summary page shifts every page index by one.
    let regions = match region_marks.map(|m| mark_regions(&mut session, &m, &options)) {
        Some(Err(err)) => {
            eprintln!("pdfcer: compare: {err}");
            return Err(exit::RUNTIME_ERROR);
        }
        Some(Ok(r)) => Some(r),
        None => None,
    };
    let marked = match mark_changes(&mut session, report, &options) {
        Ok(m) => m,
        Err(err) => {
            eprintln!("pdfcer: compare: {err}");
            return Err(exit::RUNTIME_ERROR);
        }
    };
    let outcome = save_edited(
        &mut session,
        &[],
        output,
        SaveMode::Incremental,
        ProducerArg::Preserve,
        false,
    )?;
    println!("{}", result_text(&marked, regions.as_ref(), output));
    if let Some(r) = &regions {
        crate::compare_visual::report_unmarked(r);
    }
    for u in &marked.unmarked {
        eprintln!(
            "pdfcer: compare: change {} was not marked: {}",
            u.change + 1,
            u.reason
        );
    }
    for d in marked.summary.iter().flat_map(|s| &s.disclosures) {
        eprintln!("pdfcer: compare: summary page: {d}");
    }
    match finish_edit(&args.new, &outcome) {
        exit::SUCCESS => Ok(()),
        code => Err(code),
    }
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

fn result_text(m: &ResultReport, regions: Option<&RegionReport>, output: &Path) -> String {
    let visual = regions.map_or_else(String::new, |r| {
        format!(
            "; {} visual mark(s), {} unmarked",
            r.annotations,
            r.unmarked.len()
        )
    });
    format!(
        "compare: wrote {}: {} change(s) marked with {} annotation(s), {} unmarked{visual}{}",
        output.display(),
        m.changes_marked,
        m.annotations,
        m.unmarked.len(),
        if m.summary.is_some() {
            "; summary page inserted before page 1"
        } else {
            ""
        },
    )
}

fn open_failed(path: &Path, err: &pdfcer_core::document::DocError) -> u8 {
    eprintln!("pdfcer: {}: {err}", path.display());
    exit_code_for_doc(err)
}

/// The text-compare options, and the parsed page lists `(old, new)` the
/// visual comparison reads too.
fn options(
    args: &CompareArgs,
    old: &Document,
    new: &Document,
) -> Result<(CompareOptions, crate::compare_visual::PageLists), String> {
    let mut o = CompareOptions::default()
        .with_formatting(args.formatting)
        .with_artifacts(args.artifacts);
    let mut lists = (None, None);
    for (side, spec, doc) in [
        (Side::Old, &args.pages_old, old),
        (Side::New, &args.pages_new, new),
    ] {
        if let Some(spec) = spec {
            let count = pdfcer_core::page_tree::pages(doc)
                .map_err(|e| format!("{side:?} document: {e}"))?
                .len();
            let pages = crate::sign::parse_pages(spec, count)
                .map_err(|e| format!("--pages-{}: {e}", side_name(side)))?;
            match side {
                Side::Old => lists.0 = Some(pages.clone()),
                _ => lists.1 = Some(pages.clone()),
            }
            o = o.with_pages(side, pages);
        }
    }
    Ok((o, lists))
}

const fn side_name(side: Side) -> &'static str {
    match side {
        Side::Old => "old",
        Side::New => "new",
    }
}

const fn kind_name(kind: ChangeKind) -> &'static str {
    match kind {
        ChangeKind::Inserted => "inserted",
        ChangeKind::Deleted => "deleted",
        ChangeKind::Replaced => "replaced",
        ChangeKind::Formatting => "formatting",
        _ => "other",
    }
}

/// One line per change, then the disclosures.
fn report_text(r: &CompareReport) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "compare: {} change(s): {} inserted, {} deleted, {} replaced, {} formatting; \
         {} old word(s), {} new word(s)",
        r.changes.len(),
        r.count(ChangeKind::Inserted),
        r.count(ChangeKind::Deleted),
        r.count(ChangeKind::Replaced),
        r.count(ChangeKind::Formatting),
        r.old_word_count,
        r.new_word_count,
    );
    for c in &r.changes {
        let _ = writeln!(out, "  {:<10} {}", kind_name(c.kind), change_text(c));
    }
    let (uo, un) = r.unreadable_pages;
    if uo + un > 0 {
        let _ = writeln!(
            out,
            "note: {uo} old and {un} new page(s) could not be read; their text is missing"
        );
    }
    if r.truncated {
        let _ = writeln!(
            out,
            "note: a stretch exceeded the work limit and is reported as one replacement"
        );
    }
    out
}

fn change_text(c: &Change) -> String {
    let pages = |places: &[Place]| -> String {
        let mut p: Vec<usize> = places.iter().map(|p| p.page + 1).collect();
        p.dedup();
        p.iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(",")
    };
    let anchor = c
        .anchor
        .map(|(s, p)| format!(" (at {} p{})", side_name(s), p.page + 1))
        .unwrap_or_default();
    let (old, new) = (clip(&c.old_text), clip(&c.new_text));
    match c.kind {
        ChangeKind::Inserted => format!("new p{}{anchor}: {new}", pages(&c.new_places)),
        ChangeKind::Deleted => format!("old p{}{anchor}: {old}", pages(&c.old_places)),
        ChangeKind::Formatting => {
            format!("new p{}: {new}{}", pages(&c.new_places), formatting_text(c))
        }
        _ => format!(
            "old p{} -> new p{}: {old} -> {new}",
            pages(&c.old_places),
            pages(&c.new_places),
        ),
    }
}

/// Characters of a change's text shown in the human report; `--json`
/// carries the whole text.
const CLIP_CHARS: usize = 160;

/// `text` quoted, cut to [`CLIP_CHARS`] with the word count appended when
/// longer.
fn clip(text: &str) -> String {
    if text.chars().count() <= CLIP_CHARS {
        return format!("{text:?}");
    }
    let head: String = text.chars().take(CLIP_CHARS).collect();
    let words = text.split(' ').count();
    format!("{:?} ({words} words; --json has all)", format!("{head}..."))
}

fn formatting_text(c: &Change) -> String {
    let Some(f) = c.formatting else {
        return String::new();
    };
    let mut parts = Vec::new();
    if let Some((a, b)) = f.size {
        parts.push(format!("size {a} -> {b}"));
    }
    if let Some((a, b)) = f.weight {
        parts.push(format!("weight {a} -> {b}"));
    }
    if let Some((a, b)) = f.color {
        parts.push(format!("colour {} -> {}", color_text(a), color_text(b)));
    }
    format!(" ({})", parts.join(", "))
}

fn color_text(c: Option<TextColor>) -> String {
    match c {
        None => "unset".to_owned(),
        Some(TextColor::Gray(g)) => format!("gray {g}"),
        Some(TextColor::Rgb(r, g, b)) => format!("rgb {r} {g} {b}"),
        Some(TextColor::Cmyk(c, m, y, k)) => format!("cmyk {c} {m} {y} {k}"),
        Some(_) => "other".to_owned(),
    }
}

fn report_json(r: &CompareReport) -> String {
    let pages = |p: &[usize]| {
        let v: Vec<String> = p.iter().map(|i| (i + 1).to_string()).collect();
        format!("[{}]", v.join(","))
    };
    let changes: Vec<String> = r.changes.iter().map(change_json).collect();
    format!(
        "{{\"identical\":{},\"old_words\":{},\"new_words\":{},\"old_pages\":{},\
         \"new_pages\":{},\"unreadable_pages\":{{\"old\":{},\"new\":{}}},\
         \"truncated\":{},\"changes\":[{}]}}",
        r.is_identical(),
        r.old_word_count,
        r.new_word_count,
        pages(&r.old_pages),
        pages(&r.new_pages),
        r.unreadable_pages.0,
        r.unreadable_pages.1,
        r.truncated,
        changes.join(",")
    )
}

fn change_json(c: &Change) -> String {
    let anchor = c.anchor.map_or_else(
        || "null".to_owned(),
        |(s, p)| format!("{{\"side\":\"{}\",{}}}", side_name(s), place_fields(&p)),
    );
    let formatting = c.formatting.map_or_else(
        || "null".to_owned(),
        |f| {
            let pair = |x: Option<String>| x.unwrap_or_else(|| "null".to_owned());
            format!(
                "{{\"size\":{},\"weight\":{},\"color\":{}}}",
                pair(f.size.map(|(a, b)| format!("[{a},{b}]"))),
                pair(f.weight.map(|(a, b)| format!("[{a},{b}]"))),
                pair(f.color.map(|(a, b)| format!(
                    "[\"{}\",\"{}\"]",
                    color_text(a),
                    color_text(b)
                ))),
            )
        },
    );
    format!(
        "{{\"kind\":\"{}\",\"old_text\":\"{}\",\"new_text\":\"{}\",\"old_places\":{},\
         \"new_places\":{},\"anchor\":{anchor},\"formatting\":{formatting}}}",
        kind_name(c.kind),
        json_escape(&c.old_text),
        json_escape(&c.new_text),
        places_json(&c.old_places),
        places_json(&c.new_places),
    )
}

fn places_json(places: &[Place]) -> String {
    let v: Vec<String> = places
        .iter()
        .map(|p| format!("{{{}}}", place_fields(p)))
        .collect();
    format!("[{}]", v.join(","))
}

/// `"page":N,"rect":[llx,lly,urx,ury]`, page 1-based, rect rounded to
/// 0.01 pt.
fn place_fields(p: &Place) -> String {
    let Rect { llx, lly, urx, ury } = p.rect;
    let r = |v: f64| (v * 100.0).round() / 100.0;
    format!(
        "\"page\":{},\"rect\":[{},{},{},{}]",
        p.page + 1,
        r(llx),
        r(lly),
        r(urx),
        r(ury)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_places_are_one_based_and_rounded() {
        let p = Place {
            page: 0,
            rect: Rect {
                llx: 1.0,
                lly: 2.004,
                urx: 3.5,
                ury: 4.0,
            },
        };
        assert_eq!(place_fields(&p), "\"page\":1,\"rect\":[1,2,3.5,4]");
    }

    #[test]
    fn long_change_text_is_clipped_with_its_word_count() {
        assert_eq!(clip("a b"), "\"a b\"");
        let long = "word ".repeat(100);
        let c = clip(long.trim_end());
        assert!(c.ends_with("(100 words; --json has all)"), "{c}");
        assert!(c.len() < 220);
    }
}
