//! `ocr` — recognise a scanned page and add an invisible text layer.

use super::*;
use pdfcer_core::ocr::layer::{OcrLayerOptions, OcrLayerReport};
use pdfcer_core::ocr::{OcrPage, PagePlacement, layer, words_to_page_space_on};
use pdfcer_core::page_tree::Page;

/// The `ocr` subcommand's arguments, past the model choice.
pub(crate) struct OcrRequest<'a> {
    pub(crate) input: &'a Path,
    pub(crate) page_number: u32,
    pub(crate) output: Option<&'a Path>,
    pub(crate) in_place: bool,
    pub(crate) dpi: f32,
    pub(crate) ocr_lang: &'a str,
    pub(crate) show_words: bool,
    pub(crate) dump_image: Option<&'a Path>,
    pub(crate) existing: ExistingOcrArg,
    /// `--layout`: read region by region (`paddle-vl` only).
    pub(crate) layout: bool,
    /// `--region-layers`: one layer per region group.
    pub(crate) region_layers: bool,
}

/// `ocr` — recognise a scanned page and add an invisible text layer.
///
/// # The pipeline, and where each step can go wrong
///
/// 1. **Rasterise** the page at `--dpi` (`pdfcer_render::render_page_with`).
/// 2. **Convert RGBA to 8-bit grey**, the only layout
///    `OcrEngine::recognize` accepts.
/// 3. **Recognise**, producing words in IMAGE pixels, y-down.
/// 4. **Map to page space**, y-up, undoing the rasteriser's geometry
///    exactly, `/Rotate` included.
/// 5. **Write the layer** and save incrementally.
///
/// Step 4 fails silently: a mis-mapped word is a well-formed word in the
/// wrong place under an invisible layer. That is why `--words` exists and
/// why `PagePlacement` carries the rotation. The mapping uses the **crop**
/// box, which is what `pdfcer_render::page_device_geometry` rasterises; the
/// media box would scale every word by the ratio between the two.
pub(crate) fn cmd_ocr(req: &OcrRequest<'_>, model_choice: &OcrModelChoice<'_>) -> u8 {
    match run_ocr(req, model_choice) {
        Ok(()) => exit::SUCCESS,
        Err(code) => code,
    }
}

fn run_ocr(req: &OcrRequest<'_>, model_choice: &OcrModelChoice<'_>) -> Result<(), u8> {
    let destination = destination(req)?;
    let (doc, page, index) = open_page(req.input, req.page_number)?;
    let (engine, engine_choice, source) =
        load_ocr_engine(model_choice, req.ocr_lang, req.dpi, req.layout)?;
    let (iw, ih, grey) = rasterise_grey(&doc, &page, req.dpi, req.page_number)?;
    if let Some(path) = req.dump_image {
        dump_grey(path, iw, ih, &grey)?;
    }
    let raw = engine.recognize_page(iw, ih, &grey, None).map_err(|err| {
        eprintln!("pdfcer: ocr: recognition failed: {err}");
        exit::RUNTIME_ERROR
    })?;
    // `words_to_page_space_on`, not `words_to_page_space`: the latter is
    // right on `/Rotate 0` only. Word order is kept, so line and block
    // indices stay valid.
    let placement = PagePlacement::new(page.crop_box, i32::from(page.rotate));
    let placed = words_to_page_space_on(&raw.words, iw, ih, placement);
    let confidence_available = raw.confidence_available;
    let recognised = raw.words.len();
    let ocr_page = OcrPage {
        words: placed,
        ..raw
    };
    if req.show_words {
        print_words(&ocr_page);
    }
    #[cfg(feature = "ocr-vl")]
    if req.layout {
        print_regions(&engine, iw, ih, placement);
    }
    // The engine name goes into the layer's marker so a later run, or
    // another tool, can tell which recogniser produced it.
    let opts = OcrLayerOptions::new()
        .with_engine(engine_choice.name())
        .with_existing(req.existing.into());
    let (bytes, report) = write_layer(req, doc, index, &ocr_page, opts)?;
    if let Err(err) = write_output(destination, &bytes) {
        eprintln!("pdfcer: {}: {err}", destination.display());
        return Err(exit::IO_ERROR);
    }
    println!(
        "ocr {} page={} -> {} engine={} dpi={} image={iw}x{ih} rotate={} \
recognised={recognised} written={} replaced={} confidence={}",
        req.input.display(),
        req.page_number,
        destination.display(),
        engine_choice.name(),
        req.dpi,
        page.rotate,
        report.words_written,
        report.layers_replaced,
        if confidence_available {
            "reported"
        } else {
            "none"
        },
    );
    disclose(&report, &engine, &source, confidence_available, destination);
    Ok(())
}

/// Exactly one destination. Neither is a refusal, not a default: writing
/// beside the input invents a path, and writing over it silently is the
/// one thing a tool must never do by accident.
fn destination<'a>(req: &OcrRequest<'a>) -> Result<&'a Path, u8> {
    match (req.output, req.in_place) {
        (Some(o), false) => Ok(o),
        (None, true) => Ok(req.input),
        (None, false) => {
            eprintln!(
                "pdfcer: ocr: give either --output <PATH> or --in-place. \
                 --in-place writes the recognised layer back over {}.",
                req.input.display()
            );
            Err(exit::RUNTIME_ERROR)
        }
        // Unreachable while clap's `conflicts_with = "output"` stands.
        (Some(_), true) => {
            eprintln!("pdfcer: ocr: --output and --in-place are mutually exclusive.");
            Err(exit::RUNTIME_ERROR)
        }
    }
}

fn open_page(input: &Path, page_number: u32) -> Result<(Document, Page, usize), u8> {
    let doc = open_document(input).map_err(|err| {
        eprintln!("pdfcer: {}: {err}", input.display());
        exit_code_for_doc(&err)
    })?;
    let pages = pdfcer_core::page_tree::pages(&doc).map_err(|err| {
        eprintln!("pdfcer: {}: {err}", input.display());
        exit::RUNTIME_ERROR
    })?;
    let count = pages.len();
    let index = usize::try_from(page_number)
        .ok()
        .and_then(|n| n.checked_sub(1))
        .filter(|&i| i < pages.len());
    let Some((index, page)) = index.and_then(|i| pages.into_iter().nth(i).map(|p| (i, p))) else {
        eprintln!("pdfcer: page {page_number} is out of range — the document has {count} page(s)");
        return Err(exit::RUNTIME_ERROR);
    };
    Ok((doc, page, index))
}

/// The page rasterised at `dpi`, as 8-bit grey (Rec.601 luma), with the
/// pixmap's own dimensions: the rasteriser rounds up to whole pixels, so a
/// recomputed `crop * scale` would be a fraction of a pixel out.
fn rasterise_grey(
    doc: &Document,
    page: &Page,
    dpi: f32,
    page_number: u32,
) -> Result<(u32, u32, Vec<u8>), u8> {
    let scale = dpi / 72.0;
    if !(scale.is_finite() && scale > 0.0) {
        eprintln!("pdfcer: ocr: --dpi must be a positive number, got {dpi}");
        return Err(exit::RUNTIME_ERROR);
    }
    let rendered =
        pdfcer_render::render_page_with(doc, page, scale, &pdfcer_render::RenderOptions::default())
            .map_err(|err| {
                eprintln!("pdfcer: ocr: could not rasterise page {page_number}: {err}");
                exit::RUNTIME_ERROR
            })?;
    let (iw, ih) = (rendered.pixmap.width(), rendered.pixmap.height());
    let grey = rendered
        .pixmap
        .data()
        .chunks_exact(4)
        .map(|px| match *px {
            [r, g, b, _] => {
                let (r, g, b) = (u32::from(r), u32::from(g), u32::from(b));
                #[allow(clippy::cast_possible_truncation)] // a weighted mean of bytes
                {
                    ((r * 299 + g * 587 + b * 114) / 1000) as u8
                }
            }
            _ => 255,
        })
        .collect();
    Ok((iw, ih, grey))
}

/// `--dump-image`: the exact buffer handed to the recogniser, encoded
/// through a fresh pixmap rather than a hand-rolled PNG writer.
fn dump_grey(path: &Path, iw: u32, ih: u32, grey: &[u8]) -> Result<(), u8> {
    let Some(mut dump) = pdfcer_render::tiny_skia::Pixmap::new(iw, ih) else {
        eprintln!("pdfcer: ocr: --dump-image: could not allocate {iw}x{ih}");
        return Err(exit::RUNTIME_ERROR);
    };
    for (px, &g) in dump.pixels_mut().iter_mut().zip(grey.iter()) {
        if let Some(v) = pdfcer_render::tiny_skia::PremultipliedColorU8::from_rgba(g, g, g, 255) {
            *px = v;
        }
    }
    let png = dump.encode_png().map_err(|err| {
        eprintln!("pdfcer: ocr: --dump-image: {err}");
        exit::RUNTIME_ERROR
    })?;
    if let Err(err) = write_output(path, &png) {
        eprintln!("pdfcer: {}: {err}", path.display());
        return Err(exit::IO_ERROR);
    }
    eprintln!(
        "pdfcer: ocr: wrote the recogniser's own input to {} ({iw}x{ih} grey)",
        path.display()
    );
    Ok(())
}

fn print_words(page: &OcrPage) {
    for w in &page.words {
        println!(
            "word text={:?} rect={:.2},{:.2},{:.2},{:.2} confidence={}",
            w.text,
            w.rect.llx,
            w.rect.lly,
            w.rect.urx,
            w.rect.ury,
            w.confidence
                .map_or_else(|| "none".to_owned(), |c| format!("{c:.3}"))
        );
    }
}

/// `--layout`: one `region` line per layout region, its box in page space.
#[cfg(feature = "ocr-vl")]
fn print_regions(engine: &pdfcer_ocr_host::OcrRunner, iw: u32, ih: u32, placement: PagePlacement) {
    use pdfcer_core::ocr::RecognizedWord;
    let Some(read) = engine.last_layout() else {
        return;
    };
    for (i, r) in read.regions.iter().enumerate() {
        let [x0, y0, x1, y1] = r.region.bbox.map(f64::from);
        let corner = RecognizedWord {
            text: String::new(),
            rect: pdfcer_core::page_tree::Rect::from_corners(x0, y0, x1, y1),
            confidence: None,
        };
        let Some(at) = words_to_page_space_on(&[corner], iw, ih, placement).pop() else {
            continue;
        };
        println!(
            "region index={} class={} group={} score={:.2} rect={:.2},{:.2},{:.2},{:.2} read={}",
            i + 1,
            r.region.class,
            r.region.class.group(),
            r.region.score,
            at.rect.llx,
            at.rect.lly,
            at.rect.urx,
            at.rect.ury,
            match (&r.reading, r.grid_placed) {
                (None, _) => "no",
                (Some(_), true) => "table-grid",
                (Some(_), false) => "yes",
            }
        );
    }
}

/// Write the layer: through `EditSession::add_ocr_layer` for `--in-place`
/// or `--region-layers` (the route a GUI uses, and the one that can create
/// layers), else the one-shot. Both call the same `plan_ocr_layer`.
fn write_layer(
    req: &OcrRequest<'_>,
    doc: Document,
    index: usize,
    ocr_page: &OcrPage,
    opts: OcrLayerOptions,
) -> Result<(Vec<u8>, OcrLayerReport), u8> {
    if !(req.in_place || req.region_layers) {
        // Recognising nothing is an answer, not a malformed file, so it
        // gets the refusal code rather than the parse-failure one.
        return layer::add_ocr_layer(&doc, index, ocr_page, &opts)
            .map(|o| (o.bytes, o.report))
            .map_err(|err| {
                eprintln!("pdfcer: ocr: {err}");
                exit::EDIT_REFUSED
            });
    }
    let mut session = pdfcer_core::edit::EditSession::new(doc);
    let opts = if req.region_layers {
        region_layers(&mut session, ocr_page, opts)?
    } else {
        opts
    };
    let layers = [pdfcer_core::edit::OcrPageLayer {
        page_index: index,
        recognised: ocr_page,
    }];
    let reports = session.add_ocr_layer(&layers, &opts).map_err(|err| {
        eprintln!("pdfcer: ocr: {err}");
        exit::EDIT_REFUSED
    })?;
    let Some(report) = reports.into_iter().next() else {
        eprintln!("pdfcer: ocr: nothing was written");
        return Err(exit::EDIT_REFUSED);
    };
    match session.to_incremental_bytes(&pdfcer_core::writer::SaveOptions::identity()) {
        Ok((bytes, saved)) => {
            crate::edit_common::disclose_rc4(req.input, saved.rc4_keystream_reused);
            Ok((bytes, report))
        }
        Err(err) => {
            eprintln!("pdfcer: save refused: {err}");
            Err(exit::SAVE_REFUSED)
        }
    }
}

/// The layer name a region group's OCR text goes on.
fn region_layer_name(group: pdfcer_core::ocr::layout::RegionGroup) -> String {
    format!("OCR text: {group}")
}

/// One layer per region group on the page: an existing layer of the same
/// name is reused, so repeated runs and pages share one per group.
fn region_layers(
    session: &mut pdfcer_core::edit::EditSession,
    page: &OcrPage,
    mut opts: OcrLayerOptions,
) -> Result<OcrLayerOptions, u8> {
    let mut groups: Vec<_> = page
        .blocks
        .iter()
        .filter_map(|b| b.region.map(|r| r.group()))
        .collect();
    groups.sort();
    groups.dedup();
    if groups.is_empty() {
        eprintln!(
            "pdfcer: ocr: --region-layers: no layout regions were read, so no region \
             layers were made"
        );
    }
    for group in groups {
        let name = region_layer_name(group);
        let existing = pdfcer_core::layers::list_layers(&session.graph())
            .into_iter()
            .find(|l| l.name == name)
            .map(|l| l.id);
        let id = match existing {
            Some(id) => id,
            None => session
                .add_layer(&name, &pdfcer_core::edit::LayerEdit::new())
                .map_err(|err| {
                    eprintln!("pdfcer: ocr: --region-layers: {name}: {err}");
                    exit::EDIT_REFUSED
                })?,
        };
        eprintln!(
            "pdfcer: ocr: text read from `{group}` regions goes on layer \"{name}\"{}",
            if existing.is_some() {
                " (existing)"
            } else {
                ""
            }
        );
        opts = opts.on_region_layer(group, id);
    }
    Ok(opts)
}

/// Rule 4: in the CLI the invocation is the commit, so every inference is
/// printed on the way past.
fn disclose(
    report: &OcrLayerReport,
    engine: &pdfcer_ocr_host::OcrRunner,
    source: &ResolvedModel,
    confidence_available: bool,
    destination: &Path,
) {
    for line in report.disclosures() {
        eprintln!("pdfcer: ocr: {line}");
    }
    eprintln!(
        "pdfcer: ocr: models loaded from {} ({})",
        source.dir.display(),
        source.how
    );
    if let Some(line) = engine.disclosure() {
        eprintln!("pdfcer: ocr: {line}");
    }
    if !confidence_available {
        eprintln!(
            "pdfcer: ocr: this engine reports NO per-word confidence, so nothing above has \
             been scored — that is not the same as everything being right"
        );
    }
    eprintln!(
        "pdfcer: ocr: the layer is INVISIBLE (mode 3) and the page is unchanged — check it \
         with `find-text {} --needle <a word on the page>`, not by looking at it",
        destination.display()
    );
}
