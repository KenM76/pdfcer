//! `text-locate`: map searched text to the editable runs that draw it
//! (`pdfcer_core::vector::locate_text_runs`), so a script can go from "the
//! word I can see" to the `--object`/`--leaf` + `--run` operands the
//! `text-run-*` verbs take.

use super::*;
use pdfcer_core::object::Dict;
use pdfcer_core::text_edit::synth::{StyleSynthesis, detect_at};
use pdfcer_core::text_extract::{
    ExtractOptions, GlyphProvenance, TextRun as ExtractedRun, extract_page,
};
use pdfcer_core::vector::{
    DocumentFonts, FontResolver, Matrix, PageObjects, TextRunRef, decompose_page, locate_text_run,
};

/// One `match` line per occurrence of `find` in the page's extracted runs,
/// then a `text-locate` totals line.
///
/// ## Contract
///
/// - `match ordinal=K start=S len=L targets=T unresolved=U`: `S`/`L` the
///   byte range of the occurrence in the page's text (every extracted run's
///   text concatenated in reading order, derived spaces, line breaks and
///   artifacts included, as `extract-text --include-artifacts` prints it),
///   `T` a comma list of `object=I/run=J` (page content) or `leaf=I/run=J`
///   (inside a form XObject) — exactly the operands of `text-run-delete`,
///   `text-run-move` and friends — in glyph order, or `none`. `U` counts the
///   occurrence's glyphs that resolve to no editable run (a Type 3 glyph
///   procedure, `/ActualText`, a repeated form whose placements cannot be
///   told apart).
/// - After each `match` line, one `target match=K ref=R font=F render_mode=M
///   line_width=W synthetic=S` line per target, in the same order: `F` the
///   run's `/BaseFont` (quoted) or `?`, `M` its `Tr` (0-7), `W` the user-space
///   `w`, `S` one of `none`/`bold`/`italic`/`bold-italic` — whether the run's
///   weight or slant is FAKED by a stroke or a shear rather than drawn by the
///   face — or `unknown` when the `/BaseFont` cannot be read, since a
///   stroked Bold face and a faux-bolded regular one differ only by name.
/// - Occurrences do not overlap. A word drawn by several show operators or
///   text objects is found, and lists every run that draws part of it.
/// - No match is a valid answer: `matches=0`, exit 0. An empty `--find`, a
///   bad page or an unreadable page exits `RUNTIME_ERROR` (1).
pub(crate) fn cmd_text_locate(input: &Path, page_number: u32, find: &str) -> u8 {
    if find.is_empty() {
        eprintln!("pdfcer: {}: --find must not be empty", input.display());
        return exit::RUNTIME_ERROR;
    }
    let doc = match open_for_read(input) {
        Ok(doc) => doc,
        Err(code) => return code,
    };
    let pages = match pdfcer_core::page_tree::pages(&doc) {
        Ok(pages) => pages,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit::RUNTIME_ERROR;
        }
    };
    let Some(page) = page_number
        .checked_sub(1)
        .and_then(|i| pages.get(i as usize))
    else {
        eprintln!(
            "pdfcer: {}: page {page_number} is out of range (document has {} page(s), numbered 1..={})",
            input.display(),
            pages.len(),
            pages.len()
        );
        return exit::RUNTIME_ERROR;
    };
    let page_index = (page_number - 1) as usize;
    let options = ExtractOptions::default().with_provenance(true);
    let text = match extract_page(&doc, page, page_index, &options) {
        Ok(text) => text,
        Err(err) => {
            eprintln!("pdfcer: {}: page {page_number}: {err}", input.display());
            return exit::RUNTIME_ERROR;
        }
    };
    let model = match decompose_page(&doc.view(), page, Matrix::IDENTITY) {
        Ok(model) => model,
        Err(err) => {
            eprintln!("pdfcer: {}: page {page_number}: {err}", input.display());
            return exit::RUNTIME_ERROR;
        }
    };
    let page_text: String = text.runs.iter().map(|r| r.text.as_str()).collect();
    let mut matches = 0usize;
    for (start, _) in page_text.match_indices(find) {
        let (targets, unresolved) = occurrence_targets(&model, &text.runs, start, find.len());
        let refs: Vec<TextRunRef> = targets.iter().map(|(r, _)| *r).collect();
        println!(
            "match ordinal={matches} start={start} len={} targets={} unresolved={unresolved}",
            find.len(),
            targets_token(&refs),
        );
        for (target, prov) in &targets {
            let style = target_style(&doc, &page.resources, &model, *target, prov);
            println!("target match={matches} {style}");
        }
        matches += 1;
    }
    println!(
        "text-locate {} page {page_number} find={find:?} matches={matches}",
        input.display()
    );
    exit::SUCCESS
}

/// The distinct editable runs behind the glyphs overlapping page-text bytes
/// `start..start + len`, plus how many of those glyphs (or glyph-less sourced
/// runs, such as `/ActualText`) resolve to none. Derived whitespace has no
/// glyphs and no source, so it is neither. Each target keeps the provenance
/// of its first matched glyph: one show operator has one text state.
fn occurrence_targets<'a>(
    model: &PageObjects,
    runs: &'a [ExtractedRun],
    start: usize,
    len: usize,
) -> (Vec<(TextRunRef, &'a GlyphProvenance)>, usize) {
    let end = start + len;
    let mut targets: Vec<(TextRunRef, &GlyphProvenance)> = Vec::new();
    let mut unresolved = 0usize;
    let mut base = 0usize;
    for run in runs {
        let (run_start, run_end) = (base, base + run.text.len());
        base = run_end;
        if run_end <= start || run_start >= end {
            continue;
        }
        if run.glyphs.is_empty() {
            unresolved += usize::from(run.is_sourced());
            continue;
        }
        for g in &run.glyphs {
            let gs = run_start + g.text_start as usize;
            let ge = gs + g.text_len as usize;
            if ge <= start || gs >= end || ge == gs {
                continue;
            }
            match g
                .provenance
                .as_ref()
                .and_then(|p| locate_text_run(model, p).map(|r| (r, p)))
            {
                Some((r, p)) if !targets.iter().any(|(t, _)| *t == r) => targets.push((r, p)),
                Some(_) => {}
                None => unresolved += 1,
            }
        }
    }
    (targets, unresolved)
}

fn targets_token(targets: &[TextRunRef]) -> String {
    if targets.is_empty() {
        return "none".to_owned();
    }
    targets
        .iter()
        .map(|t| match t {
            TextRunRef::Page {
                object_index,
                run_index,
            } => format!("object={object_index}/run={run_index}"),
            TextRunRef::Form {
                leaf_index,
                run_index,
            } => format!("leaf={leaf_index}/run={run_index}"),
        })
        .collect::<Vec<_>>()
        .join(",")
}

/// The `ref=… font=… render_mode=… line_width=… synthetic=…` tail of a
/// `target` line.
fn target_style(
    doc: &Document,
    page_resources: &Dict,
    model: &PageObjects,
    target: TextRunRef,
    prov: &GlyphProvenance,
) -> String {
    let base_font = shown_base_font(doc, page_resources, model, target, prov);
    let synthetic = match base_font.as_deref().map(|name| detect_at(name, prov)) {
        None => "unknown",
        Some(s) => synthesis_token(s),
    };
    let font = base_font.map_or_else(|| "?".to_owned(), |name| format!("{name:?}"));
    format!(
        "ref={} font={font} render_mode={} line_width={} synthetic={synthetic}",
        targets_token(&[target]),
        prov.render_mode(),
        prov.line_width,
    )
}

/// `/BaseFont` of the font the glyph was SHOWN in — its `Tf` resource looked
/// up in the resources of the stream that drew it: the directly enclosing
/// form's own `/Resources`, else the page's (§7.8.3).
fn shown_base_font(
    doc: &Document,
    page_resources: &Dict,
    model: &PageObjects,
    target: TextRunRef,
    prov: &GlyphProvenance,
) -> Option<String> {
    let name = prov.font_resource.as_deref()?;
    let resources = match target {
        TextRunRef::Page { .. } => page_resources,
        TextRunRef::Form { leaf_index, .. } => model
            .leaves
            .get(leaf_index)
            .and_then(|leaf| leaf.parent())
            .and_then(|form| doc.get(form))
            .and_then(|form| form.value.as_dict())
            .and_then(|dict| dict.get(b"Resources"))
            .and_then(|res| doc.resolve(res).as_dict())
            .unwrap_or(page_resources),
    };
    let view = doc.view();
    let font = DocumentFonts::new(&view, resources).resolve(name)?;
    (!font.base_font.is_empty()).then(|| font.base_font.clone())
}
fn synthesis_token(s: StyleSynthesis) -> &'static str {
    match (s.bold(), s.italic()) {
        (false, false) => "none",
        (true, false) => "bold",
        (false, true) => "italic",
        (true, true) => "bold-italic",
    }
}
