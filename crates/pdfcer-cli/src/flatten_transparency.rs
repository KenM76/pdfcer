//! `flatten-transparency`: composite a page's transparency into opaque
//! content (ISO 32000-2 §11).

use super::*;

/// What `flatten-transparency` draws and at what resolution.
pub(crate) struct FlattenArgs<'a> {
    /// `all` or a 1-based page list.
    pub pages: &'a str,
    /// Redraw every text run as outlines.
    pub outline_text: bool,
    /// Draw redrawn strokes as filled outlines.
    pub outline_strokes: bool,
    /// Raster resolution for what cannot be vector regions.
    pub ppi: f64,
}

/// `flatten-transparency` — `EditSession::flatten_transparency` with the
/// shipped `pdfcer_emf::RecordingFlattener`, as one undo step.
///
/// ## Contract
///
/// - One line per page asked for:
///   `page=<N> mode=<overlay|rebuilt|untouched> vector_regions=<n>
///   rasterised=<n> text_outlined=<n> strokes_outlined=<n>
///   annotations_left=<n>`, then one `rasterised page=<N> reason=<why>
///   rect=[llx lly urx ury]` line per rasterised area, then one `note:`
///   line per disclosure, then the save line, then [`finish_edit`]'s exit.
/// - A page list naming no page, a resolution outside 36..=2400 or a page
///   the flattener cannot record is refused before anything is written
///   (exit `EDIT_REFUSED`).
/// - A run where no page's content uses transparency writes nothing but
///   still saves the input through `--mode`, `changed=0`.
pub(crate) fn cmd_flatten_transparency(
    input: &Path,
    args: &FlattenArgs<'_>,
    output: &Path,
    mode: SaveMode,
    verify_undo: bool,
) -> u8 {
    use pdfcer_core::transparency::FlattenOptions;
    let (source, mut session) = match open_for_edit(input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let count = match session.pages() {
        Ok(p) => p.len(),
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit::EDIT_REFUSED;
        }
    };
    let pages = match parse_pages(args.pages, count) {
        Ok(p) => p,
        Err(err) => {
            eprintln!("pdfcer: {}: --pages: {err}", input.display());
            return exit::EDIT_REFUSED;
        }
    };
    let options = FlattenOptions::new()
        .with_keep_text(!args.outline_text)
        .with_keep_strokes(!args.outline_strokes)
        .with_raster_ppi(args.ppi);
    let flattener = pdfcer_emf::RecordingFlattener::new();
    let report = match session.flatten_transparency(&pages, &options, &flattener) {
        Ok(r) => r,
        Err(err) => return report_edit_error(input, &err),
    };
    print_report(&report);
    let outcome = match save_edited(
        &mut session,
        &source,
        output,
        mode,
        ProducerArg::Preserve,
        verify_undo,
    ) {
        Ok(outcome) => outcome,
        Err(code) => return code,
    };
    let r = &outcome.report;
    println!(
        "flatten-transparency {} pages={} mode={} -> {}; changed={} objects={} appended={} \
out_bytes={} undo_verified={} undo_identical={}",
        input.display(),
        report.pages.len(),
        mode.name(),
        output.display(),
        outcome.changed,
        r.objects_written,
        r.bytes_appended,
        r.bytes_written,
        u32::from(outcome.undo_verified),
        u32::from(outcome.undo_identical),
    );
    finish_edit(input, &outcome)
}

fn print_report(report: &pdfcer_core::transparency::FlattenTransparencyReport) {
    use pdfcer_core::transparency::FlattenMode;
    for p in &report.pages {
        let how = match p.mode {
            Some(FlattenMode::Rebuilt) => "rebuilt",
            Some(_) => "overlay",
            None => "untouched",
        };
        println!(
            "page={} mode={how} vector_regions={} rasterised={} text_outlined={} \
strokes_outlined={} annotations_left={}",
            p.page + 1,
            p.vector_regions,
            p.rasterised.len(),
            p.text_runs_outlined,
            p.strokes_outlined,
            p.annotations_left.len()
        );
        for r in &p.rasterised {
            println!(
                "rasterised page={} reason={} rect=[{:.2} {:.2} {:.2} {:.2}]",
                p.page + 1,
                r.reason.as_str(),
                r.rect.llx,
                r.rect.lly,
                r.rect.urx,
                r.rect.ury
            );
        }
    }
    for d in &report.disclosures {
        println!("note: {d}");
    }
}
