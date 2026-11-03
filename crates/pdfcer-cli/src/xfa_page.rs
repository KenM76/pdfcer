//! `render-page` on a dynamic XFA form: draw pdfcer's computed page, not
//! the placeholder page the file stores for viewers without an XFA engine.

use super::*;

use crate::xfa_layout::{XfaLayoutArgs, XfaViewFlags, dotted, read_merged};
use crate::xfa_png::note_label;

/// `render-page --dynamic-xfa`: what a dynamic XFA form's page shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub(crate) enum DynamicXfaArg {
    /// The page pdfcer lays out from the template and data, as XFA viewers do.
    #[default]
    Layout,
    /// The placeholder page stored in the file.
    Shell,
}

/// Whether `render-page` takes the computed-layout route for `doc`.
pub(crate) fn wants_layout(doc: &Document, arg: DynamicXfaArg) -> bool {
    arg == DynamicXfaArg::Layout
        && pdfcer_core::xfa::form_kind(doc) == pdfcer_core::xfa::XfaFormKind::Dynamic
}

/// Render computed page `page_number` (1-based) of `doc`'s dynamic XFA form
/// to `output` at the default layout and paint settings. Prints a
/// `rendered-xfa` line and one `paint-note` line per thing not drawn as
/// declared; the stderr disclosure says the page is pdfcer's layout.
pub(crate) fn render_dynamic_page(
    doc: &Document,
    input: &Path,
    page_number: u32,
    scale: f32,
    output: &Path,
) -> u8 {
    let fail = |msg: &dyn std::fmt::Display| {
        eprintln!("pdfcer: {}: {msg}", input.display());
        exit::RUNTIME_ERROR
    };
    let (merged, layout) = match lay_out(doc) {
        Ok(m) => m,
        Err(e) => return fail(&e),
    };
    let view = doc.view();
    let images = pdfcer_core::xfa::XfaImages::from_view(&view);
    let count = layout.pages.len();
    let Some(page) = page_number
        .checked_sub(1)
        .and_then(|i| layout.pages.get(i as usize))
    else {
        return fail(&format_args!(
            "page {page_number} is out of range (the XFA form lays out {count} page(s), numbered 1..={count})"
        ));
    };
    let paint = pdfcer_core::xfa::PaintOptions::new();
    let out = match pdfcer_render::xfa::render_xfa_page_with_images(
        &view,
        &merged.template,
        page,
        &paint,
        &images,
        scale,
        &pdfcer_render::RenderOptions::default(),
    ) {
        Ok(o) => o,
        Err(e) => return fail(&format_args!("page {page_number}: {e}")),
    };
    let png = match out.page.pixmap.encode_png() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("pdfcer: {}: PNG encoding failed: {e}", output.display());
            return exit::RUNTIME_ERROR;
        }
    };
    if let Err(e) = write_output(output, &png) {
        eprintln!("pdfcer: {}: {e}", output.display());
        return exit::IO_ERROR;
    }
    report(input, output, page_number, count, &out);
    eprintln!(
        "pdfcer: {}: dynamic XFA form; page {page_number} is pdfcer's layout of the template with {} value(s) merged from the form's data, not a page stored in the file (--dynamic-xfa shell draws the stored page).",
        input.display(),
        merged.bound.len()
    );
    exit::SUCCESS
}

/// The form's merged template and its computed layout, at default settings.
fn lay_out(
    doc: &Document,
) -> Result<(pdfcer_core::xfa::MergedForm, pdfcer_core::xfa::XfaLayout), String> {
    let args = XfaLayoutArgs {
        image_dpi: 72,
        ..XfaLayoutArgs::default()
    };
    let (merged, _) = read_merged(doc, args, &XfaViewFlags::default())?;
    let images = pdfcer_core::xfa::XfaImages::from_view(&doc.view());
    let layout =
        pdfcer_core::xfa::layout_template_with_images(&merged.template, &args.options(), &images)
            .map_err(|e| e.to_string())?;
    Ok((merged, layout))
}

/// The `rendered-xfa` line and one `paint-note` line per note.
fn report(
    input: &Path,
    output: &Path,
    page_number: u32,
    count: usize,
    out: &pdfcer_render::xfa::RenderedXfaPage,
) {
    println!(
        "rendered-xfa {} page {page_number} -> {} {}x{}; pages={count} notes={}",
        input.display(),
        output.display(),
        out.page.pixmap.width(),
        out.page.pixmap.height(),
        out.notes.len()
    );
    for n in &out.notes {
        println!(
            "paint-note page={} kind={} path={}",
            page_number - 1,
            note_label(n.kind),
            dotted(&n.path)
        );
    }
}
