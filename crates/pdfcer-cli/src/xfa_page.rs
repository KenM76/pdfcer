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

/// `xfa-materialize --font-embedding`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub(crate) enum FontEmbeddingArg {
    /// Embed each supplied face's whole program.
    #[default]
    Embed,
    /// Reference each supplied face by name only.
    Reference,
}

/// The fonts an XFA command sets text in.
#[derive(Debug, Clone, Copy)]
pub(crate) struct XfaFontArgs<'a> {
    pub(crate) dirs: &'a [PathBuf],
    pub(crate) embedding: FontEmbeddingArg,
}

/// The renderer's environment and the XFA faces over it for `--font-dir`
/// `dirs` (and the settings file's folders), its notes printed to stderr.
pub(crate) fn xfa_font_env(
    dirs: &[PathBuf],
) -> (pdfcer_render::FontEnvironment, pdfcer_core::xfa::XfaFonts) {
    let (env, _, notes) = build_font_environment(dirs);
    for note in &notes {
        eprintln!("pdfcer: font-dir: {note}");
    }
    let fonts = pdfcer_render::font::xfa_fonts(&env);
    (env, fonts)
}

/// Rule 4's disclosure on stderr: each typeface drawn in a standard-14
/// face, and each supplied face written by name only and why.
pub(crate) fn disclose_fonts(
    input: &Path,
    fonts: &pdfcer_core::xfa::XfaFonts,
    written: &[pdfcer_core::xfa::PaintedFont],
) {
    for (typeface, base) in fonts.fallbacks() {
        eprintln!(
            "pdfcer: {}: XFA typeface \"{typeface}\" is not among the supplied fonts; its text is drawn in {base} (--font-dir adds a folder).",
            input.display()
        );
    }
    for f in written.iter().filter(|f| f.program.is_none()) {
        let why = f.refusal.unwrap_or("--font-embedding reference");
        eprintln!(
            "pdfcer: {}: font {} is referenced by name, not embedded ({why}); it is drawn right only where that font is installed.",
            input.display(),
            f.base_font
        );
    }
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
    font_env: &pdfcer_render::FontEnvironment,
) -> u8 {
    let fail = |msg: &dyn std::fmt::Display| {
        eprintln!("pdfcer: {}: {msg}", input.display());
        exit::RUNTIME_ERROR
    };
    let fonts = pdfcer_render::font::xfa_fonts(font_env);
    let (merged, layout) = match lay_out(doc, &fonts) {
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
    let mut options = pdfcer_render::RenderOptions::default();
    options.fonts = font_env.clone();
    let out = match pdfcer_render::xfa::render_xfa_page_with_images(
        &view,
        &merged.template,
        page,
        &paint,
        &images,
        scale,
        &options,
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
    disclose_fonts(input, &fonts, &[]);
    eprintln!(
        "pdfcer: {}: dynamic XFA form; page {page_number} is pdfcer's layout of the template with {} value(s) merged from the form's data, not a page stored in the file (--dynamic-xfa shell draws the stored page).",
        input.display(),
        merged.bound.len()
    );
    exit::SUCCESS
}

/// The form's merged template and its computed layout in `fonts`, at
/// default settings.
pub(crate) fn lay_out(
    doc: &Document,
    fonts: &pdfcer_core::xfa::XfaFonts,
) -> Result<(pdfcer_core::xfa::MergedForm, pdfcer_core::xfa::XfaLayout), String> {
    let args = XfaLayoutArgs {
        image_dpi: 72,
        ..XfaLayoutArgs::default()
    };
    let (merged, _) = read_merged(doc, args, &XfaViewFlags::default())?;
    let images = pdfcer_core::xfa::XfaImages::from_view(&doc.view());
    let layout =
        pdfcer_core::xfa::layout_template_with(&merged.template, &args.options(), &images, fonts)
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
