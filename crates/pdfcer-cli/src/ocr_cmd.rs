use super::*;

/// `fetch-ocr-models` — download the pinned `ocrs` weights.
///
/// # The pins, and why the detection model's URL is not the obvious one
///
/// The two files come from **different channels**, and that is a measured
/// defect rather than an oversight. Hugging Face hosts a detection model that
/// **does not work with `ocrs` 0.12.2** — on a clean render of a page of 12 pt
/// text it returns fragments at the page margin and one "word" whose box is
/// the whole page. The author's S3 bucket, which the `ocrs` crate's own
/// example fetches from, hosts one that works. The recognition model is fine
/// on either channel and stays on Hugging Face.
///
/// Established by swapping **one file at a time** rather than both; see
/// `crates/pdfcer-core/assets/models/ocrs/PROVENANCE.md` for the four-row
/// table. Do not "tidy" these back onto one channel.
///
/// # Why a hash and not just a URL
///
/// `docs/ocr-engine-survey.md` measured both channels in one session and found
/// the detection files differ by 13,280 bytes under different names. "The ocrs
/// models" is not one thing. A fetch that trusted a URL alone would install
/// weights nobody tested, and would do it silently.
#[cfg(feature = "download")]
pub(crate) fn cmd_fetch_ocr_models(dir: Option<&Path>) -> u8 {
    use pdfcer_fetch::{PinnedArtifact, fetch_verified};

    let target = match dir {
        Some(d) => d.to_path_buf(),
        None => match std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(Path::to_path_buf))
        {
            Some(exe_dir) => exe_dir.join("models").join("ocrs"),
            None => {
                eprintln!(
                    "pdfcer: fetch-ocr-models: could not locate this executable's directory \
                     — pass --dir"
                );
                return exit::RUNTIME_ERROR;
            }
        },
    };
    if let Err(err) = std::fs::create_dir_all(&target) {
        eprintln!("pdfcer: {}: {err}", target.display());
        return exit::IO_ERROR;
    }

    // Pinned by URL AND hash. Measured 2026-08-25; see this function's docs
    // for why the two channels differ.
    let artifacts = [
        PinnedArtifact::new(
            "https://ocrs-models.s3-accelerate.amazonaws.com/text-detection.rten",
            "f15cfb56bd02c4bf478a20343986504a1f01e1665c2b3a0ad66340f054b1b5ca",
            "text-detection.rten",
        ),
        PinnedArtifact::new(
            "https://huggingface.co/robertknight/ocrs/resolve/main/text-rec-checkpoint-s52qdbqt.rten",
            "606d9a0414c6b73c99df75b707c11c70d1c8b12e1d4f900922e185fc37bfca65",
            "text-rec-checkpoint.rten",
        ),
    ];

    eprintln!(
        "pdfcer: fetch-ocr-models: downloading 2 file(s) to {} — these weights are \
         CC-BY-SA-4.0, by Robert Knight (the ocrs project)",
        target.display()
    );
    for art in &artifacts {
        match fetch_verified(art, &target) {
            Ok(path) => println!("fetched {} -> {}", art.url, path.display()),
            Err(err) => {
                eprintln!("pdfcer: fetch-ocr-models: {err}");
                return exit::RUNTIME_ERROR;
            }
        }
    }
    println!(
        "fetch-ocr-models {} files=2 verified=sha256",
        target.display()
    );
    // Rule 4, and a licence obligation rather than a nicety: CC-BY-SA
    // requires attribution, and a file arriving with none attached is one an
    // operator cannot comply with. The bundled copy ships a PROVENANCE.md
    // beside it; a fetched copy has to be told.
    eprintln!(
        "pdfcer: fetch-ocr-models: licence CC-BY-SA-4.0 \
         <https://creativecommons.org/licenses/by-sa/4.0/>, creator Robert Knight, source the \
         ocrs project. Redistributing these files carries that licence's attribution and \
         share-alike terms"
    );
    exit::SUCCESS
}

/// `fetch-ocr-models`, in a build compiled WITHOUT the `download` feature.
///
/// Refuses **by name**, which is the operator's own modularity rule: a
/// stripped capability says what is missing and how to get it back, rather
/// than the subcommand quietly not existing. A missing subcommand reads as a
/// version difference; a named refusal reads as a build choice.
#[cfg(not(feature = "download"))]
pub(crate) fn cmd_fetch_ocr_models(_dir: Option<&Path>) -> u8 {
    eprintln!(
        "pdfcer: fetch-ocr-models: this build was compiled without the `download` feature, \
         so it contains no network code at all and cannot fetch anything. The OCR weights \
         normally ship in `models/ocrs` beside the executable; copy that folder, or point \
         `ocr --model-dir` at one"
    );
    exit::UNIMPLEMENTED
}

/// `list-standards` — the render presets, and the provenance of every value.
///
/// # Why the provenance is a COLUMN and not a footnote
///
/// `pdfce-gui` asked pdfcer for this vector and declined to guess it, on the
/// grounds that *"a control labelled `ISO 15930-7` carries that standard's
/// authority whether or not we intended it to."* That is right, and it means
/// the interesting information is not the value — it is how much weight the
/// value can bear. Most of these are `best-effort`: the standards mostly do
/// not legislate this far, and saying so is the honest output.
pub(crate) fn cmd_list_standards(only: Option<&str>) -> u8 {
    use pdfcer_core::settings::presets::{RenderPreset, RenderStandard};

    let wanted: Vec<RenderStandard> = match only {
        None => RenderStandard::all().to_vec(),
        Some(tok) => match RenderStandard::parse(tok) {
            Ok(s) => vec![s],
            Err(bad) => {
                eprintln!(
                    "pdfcer: list-standards: unknown standard {bad:?} — known: {}",
                    RenderStandard::all()
                        .iter()
                        .map(|s| s.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                );
                return exit::RUNTIME_ERROR;
            }
        },
    };

    for std in wanted {
        let preset = RenderPreset::for_standard(std);
        println!("standard {} title={:?}", std.as_str(), std.title());
        for e in preset.entries() {
            // Formatted by core, deliberately: `PresetAction` is
            // `#[non_exhaustive]`, so a match here would need a wildcard and a
            // seventh variant would print as the fallback while still
            // compiling. See `PresetAction::value_string`.
            let value = e.action.value_string();
            println!(
                "  setting {:<24} value={:<28} evidence={:<15} why={:?}",
                e.key.as_str(),
                value,
                e.evidence.label(),
                e.why
            );
        }
        for line in preset.disclosures() {
            eprintln!("pdfcer: {line}");
        }
    }
    exit::SUCCESS
}

/// Say what a search-driven redaction **could not read**.
///
/// # Why a redaction owes this louder than a search does
///
/// `find-text` reporting zero hits wastes a minute. `redact-mark --search`
/// reporting zero marks, on a document whose text was never recoverable as
/// Unicode, tells an operator that a name is not present when it is on the
/// page in front of them — and the next thing they do is send the file.
///
/// The two populations both **render perfectly**, which is exactly what makes
/// the failure invisible: a Type 3 font with no `/ToUnicode` (ISO 32000-1
/// §9.6.5, glyphs that are content streams named by arbitrary `/CharProcs`
/// keys) and an `Identity-H` font with no `/ToUnicode` (§9.10.2 excludes it
/// from every ladder rung).
///
/// Printed whether or not anything matched, and that is deliberate. A
/// partial match is the more dangerous case, not the safer one: "3 marks
/// authored" reads as success, and the operator has no reason to suspect a
/// fourth occurrence sat in a font the scan could not read.
pub(crate) fn report_unsearchable_redaction(
    input: &Path,
    d: &pdfcer_core::text_extract::TextDiagnostics,
) {
    if d.ladder_failures == 0 {
        return;
    }
    eprintln!(
        "pdfcer: {}: WARNING — {} of {} character code(s) in this document could not be \
         mapped to Unicode, so a search CANNOT have matched them. Marks were authored only \
         where the text was readable",
        input.display(),
        d.ladder_failures,
        d.codes_total
    );
    if d.type3_fonts_without_to_unicode > 0 {
        eprintln!(
            "pdfcer: {}: {} Type 3 font(s) carry no /ToUnicode CMap (ISO 32000-1 §9.6.5) — \
             text set in them renders correctly and cannot be searched or redacted by search",
            input.display(),
            d.type3_fonts_without_to_unicode
        );
    }
    if d.identity_fonts_without_to_unicode > 0 {
        eprintln!(
            "pdfcer: {}: {} font(s) are Identity-H/Adobe-Identity-0 with no /ToUnicode — \
             §9.10.2 excludes them from every ladder rung, so text set in them cannot be \
             searched or redacted by search",
            input.display(),
            d.identity_fonts_without_to_unicode
        );
    }
    eprintln!(
        "pdfcer: {}: DO NOT treat this document as cleared on the strength of a \
         search-driven redaction. Check the unreadable runs by eye, or mark them with --rect",
        input.display()
    );
}

/// A loaded recogniser, whichever `--ocr-engine` chose.
///
/// An enum rather than `dyn OcrEngine` because the trait's associated error
/// type differs per engine; errors are flattened to their message here, the
/// only place the CLI needs them.
pub(crate) enum LoadedOcrEngine {
    Ocrs(pdfcer_core::ocr::engine_ocrs::OcrsEngine),
    #[cfg(feature = "ocrcer")]
    Ocrcer(Box<pdfcer_core::ocr::engine_ocrcer::OcrcerEngine>),
    Tesseract(tesseract::TesseractEngine),
}

impl LoadedOcrEngine {
    /// Recognise one page image with whichever engine was loaded.
    pub(crate) fn recognize(
        &self,
        width: u32,
        height: u32,
        pixels: &[u8],
    ) -> Result<Vec<pdfcer_core::ocr::RecognizedWord>, String> {
        use pdfcer_core::ocr::OcrEngine;
        match self {
            Self::Ocrs(e) => e
                .recognize(width, height, pixels)
                .map_err(|e| e.to_string()),
            #[cfg(feature = "ocrcer")]
            Self::Ocrcer(e) => e
                .recognize(width, height, pixels)
                .map_err(|e| e.to_string()),
            Self::Tesseract(e) => e.recognize(width, height, pixels),
        }
    }

    /// Whether this engine reports a per-word confidence.
    pub(crate) fn reports_confidence(&self) -> bool {
        use pdfcer_core::ocr::OcrEngine;
        match self {
            Self::Ocrs(e) => e.reports_confidence(),
            #[cfg(feature = "ocrcer")]
            Self::Ocrcer(e) => e.reports_confidence(),
            // Tesseract's TSV carries a `conf` column on every word row.
            Self::Tesseract(_) => true,
        }
    }
}

/// Resolve the selected engine's models and load it, printing any failure.
///
/// Resolution names the engine's files, so a directory that exists but is
/// empty does not resolve and shadow a good one further down the search
/// order; on failure every path tried is reported — the difference between
/// "OCR is broken" and "put the models here".
pub(crate) fn load_ocr_engine(
    choice: OcrEngineArg,
    model_dir: Option<&Path>,
    ocr_lang: &str,
    dpi: f32,
) -> Result<(LoadedOcrEngine, pdfcer_core::ocr::models::ModelSource), u8> {
    use pdfcer_core::ocr::models;

    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(std::path::Path::to_path_buf));

    match choice {
        OcrEngineArg::Ocrs => {
            use pdfcer_core::ocr::engine_ocrs::{
                DETECTION_MODEL, MODEL_DIR, OcrsEngine, RECOGNITION_MODEL,
            };
            let source = match models::resolve_model_dir_with(
                MODEL_DIR,
                model_dir,
                exe_dir.as_deref(),
                None,
                &[DETECTION_MODEL, RECOGNITION_MODEL],
            ) {
                Ok(src) => src,
                Err(err) => {
                    eprintln!("pdfcer: ocr: {err}");
                    eprintln!(
                        "pdfcer: ocr: the model files are not bundled inside the executable — \
                         they are two files (`text-detection.rten`, `text-rec-checkpoint.rten`) \
                         that live in a `models/ocrs` folder, which the portable package ships. \
                         Pass --model-dir to point at them, or, in a build compiled with the \
                         `download` feature, run `pdfcer fetch-ocr-models` to fetch the pinned \
                         copies."
                    );
                    return Err(exit::RUNTIME_ERROR);
                }
            };
            match OcrsEngine::from_model_dir(source.path()) {
                Ok(e) => Ok((LoadedOcrEngine::Ocrs(e), source)),
                Err(err) => {
                    eprintln!("pdfcer: ocr: {err}");
                    Err(exit::RUNTIME_ERROR)
                }
            }
        }
        #[cfg(feature = "ocrcer")]
        OcrEngineArg::Ocrcer => {
            use pdfcer_core::ocr::engine_ocrcer::{
                MODEL_DIR, MODEL_FILE as OCRCER_MODEL_FILE, OcrcerEngine,
            };
            let source = match models::resolve_model_dir_with(
                MODEL_DIR,
                model_dir,
                exe_dir.as_deref(),
                None,
                &[OCRCER_MODEL_FILE],
            ) {
                Ok(src) => src,
                Err(err) => {
                    eprintln!("pdfcer: ocr: {err}");
                    eprintln!(
                        "pdfcer: ocr: the OCRcer model is one file, `{OCRCER_MODEL_FILE}`, which \
                         pdfcer neither ships nor downloads. Build or copy it from the OCRcer \
                         project (`model/out/{OCRCER_MODEL_FILE}`) into a `models/ocrcer` \
                         folder beside this executable, or pass --model-dir <its folder>."
                    );
                    return Err(exit::RUNTIME_ERROR);
                }
            };
            let path = source.path().join(OCRCER_MODEL_FILE);
            let bytes = match std::fs::read(&path) {
                Ok(b) => b,
                Err(err) => {
                    eprintln!("pdfcer: {}: {err}", path.display());
                    return Err(exit::IO_ERROR);
                }
            };
            match OcrcerEngine::from_bytes(&bytes) {
                Ok(e) => Ok((LoadedOcrEngine::Ocrcer(Box::new(e)), source)),
                Err(err) => {
                    eprintln!(
                        "pdfcer: ocr: {}: not a usable OCRcer model: {err}",
                        path.display()
                    );
                    Err(exit::RUNTIME_ERROR)
                }
            }
        }
        #[cfg(not(feature = "ocrcer"))]
        OcrEngineArg::Ocrcer => {
            eprintln!(
                "pdfcer: ocr: --ocr-engine ocrcer: this build was compiled without the `ocrcer` \
                 feature, so the OCRcer engine is not in it. Rebuild with \
                 `cargo build -p pdfcer-cli --features ocrcer`, or use --ocr-engine ocrs \
                 (the model file it would need is `ocrcer.ocrw`)."
            );
            Err(exit::UNIMPLEMENTED)
        }
        OcrEngineArg::Tesseract => {
            let source = match models::resolve_model_dir_with(
                tesseract::MODEL_DIR,
                model_dir,
                exe_dir.as_deref(),
                None,
                &[tesseract::EXE_FILE],
            ) {
                Ok(src) => src,
                Err(err) => {
                    eprintln!("pdfcer: ocr: {err}");
                    eprintln!(
                        "pdfcer: ocr: Tesseract is a folder holding `{}` and a `{}` folder of \
                         language files. The portable package ships it as `models/tesseract`; \
                         otherwise pass --model-dir <folder>, which may be a stock Tesseract \
                         install.",
                        tesseract::EXE_FILE,
                        tesseract::TESSDATA_DIR
                    );
                    return Err(exit::RUNTIME_ERROR);
                }
            };
            match tesseract::TesseractEngine::from_dir(source.path(), ocr_lang, dpi) {
                Ok(e) => {
                    eprintln!("pdfcer: ocr: running {} (-l {ocr_lang})", e.exe().display());
                    Ok((LoadedOcrEngine::Tesseract(e), source))
                }
                Err(err) => {
                    eprintln!("pdfcer: ocr: {err}");
                    Err(exit::RUNTIME_ERROR)
                }
            }
        }
    }
}

/// `ocr` — recognise a scanned page and add an invisible text layer.
///
/// # The pipeline, and where each step can go wrong
///
/// 1. **Rasterise** the page at `--dpi` (`pdfcer_render::render_page_with`).
/// 2. **Convert RGBA to 8-bit grey**, which is the only layout
///    [`OcrEngine::recognize`] accepts.
/// 3. **Recognise**, producing words in IMAGE pixels, y-down.
/// 4. **Map to page space**, y-up — the step that must undo the rasteriser's
///    geometry EXACTLY, including `/Rotate`.
/// 5. **Write the layer** and save incrementally.
///
/// Step 4 is the one that fails silently. Steps 1-3 announce their own
/// failures (a render error, a size mismatch, zero words); step 4 cannot,
/// because a mis-mapped word is a perfectly well-formed word in the wrong
/// place, and the page still looks right because the layer is invisible. That
/// is why `--words` exists and why `PagePlacement` carries the rotation.
///
/// # Why the crop box and not the media box
///
/// `pdfcer_render::page_device_geometry` rasterises the **crop** box. Handing
/// the mapping a media box that differs from it scales every word by the ratio
/// between the two — a uniform, plausible-looking error that no word count
/// detects.
#[allow(clippy::too_many_arguments)]
pub(crate) fn cmd_ocr(
    input: &Path,
    page_number: u32,
    output: Option<&Path>,
    in_place: bool,
    dpi: f32,
    engine_choice: OcrEngineArg,
    model_dir: Option<&Path>,
    ocr_lang: &str,
    show_words: bool,
    dump_image: Option<&Path>,
    existing: ExistingOcrArg,
) -> u8 {
    use pdfcer_core::ocr::{OcrPage, PagePlacement, layer, models, words_to_page_space_on};

    // Exactly one destination. `conflicts_with` already refuses BOTH, so the
    // only case left is NEITHER -- and that has to be a refusal rather than a
    // default, because both plausible defaults are wrong: writing beside the
    // input invents a path the operator did not choose, and writing over it
    // silently is the one thing a tool must never do by accident.
    let destination: &Path = match (output, in_place) {
        (Some(o), false) => o,
        (None, true) => input,
        (None, false) => {
            eprintln!(
                "pdfcer: ocr: give either --output <PATH> or --in-place. \
                 --in-place writes the recognised layer back over {}.",
                input.display()
            );
            return exit::RUNTIME_ERROR;
        }
        // Unreachable while `conflicts_with = "output"` stands. Handled rather
        // than unwrapped so that removing that attribute is a behaviour change
        // somebody has to look at, not a panic in front of an operator.
        (Some(_), true) => {
            eprintln!("pdfcer: ocr: --output and --in-place are mutually exclusive.");
            return exit::RUNTIME_ERROR;
        }
    };

    let doc = match open_document(input) {
        Ok(doc) => doc,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit_code_for_doc(&err);
        }
    };

    let pages = match pdfcer_core::page_tree::pages(&doc) {
        Ok(p) => p,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit::RUNTIME_ERROR;
        }
    };
    let index = match usize::try_from(page_number)
        .ok()
        .and_then(|n| n.checked_sub(1))
    {
        Some(i) if i < pages.len() => i,
        _ => {
            eprintln!(
                "pdfcer: page {page_number} is out of range — the document has {} page(s)",
                pages.len()
            );
            return exit::RUNTIME_ERROR;
        }
    };
    let page = &pages[index];

    let (engine, source) = match load_ocr_engine(engine_choice, model_dir, ocr_lang, dpi) {
        Ok(loaded) => loaded,
        Err(code) => return code,
    };

    // Rasterise. `scale` is the engine's own unit; DPI is the operator's.
    let scale = dpi / 72.0;
    if !(scale.is_finite() && scale > 0.0) {
        eprintln!("pdfcer: ocr: --dpi must be a positive number, got {dpi}");
        return exit::RUNTIME_ERROR;
    }
    let rendered = match pdfcer_render::render_page_with(
        &doc,
        page,
        scale,
        &pdfcer_render::RenderOptions::default(),
    ) {
        Ok(r) => r,
        Err(err) => {
            eprintln!("pdfcer: ocr: could not rasterise page {page_number}: {err}");
            return exit::RUNTIME_ERROR;
        }
    };

    // The pixmap's OWN dimensions, not `crop * scale` recomputed. The
    // rasteriser rounds up to whole pixels, so a recomputed size is a
    // fraction of a pixel out and every word inherits the discrepancy. The
    // measured value cannot disagree with what was actually drawn.
    let (iw, ih) = (rendered.pixmap.width(), rendered.pixmap.height());

    // RGBA -> 8-bit grey, Rec.601 luma. One byte per pixel is the layout the
    // trait documents; handing it four would be inferred as a 4-channel image
    // and recognised as nonsense rather than refused.
    let grey: Vec<u8> = rendered
        .pixmap
        .data()
        .chunks_exact(4)
        .map(|px| {
            let (r, g, b) = (u32::from(px[0]), u32::from(px[1]), u32::from(px[2]));
            #[allow(clippy::cast_possible_truncation)]
            {
                ((r * 299 + g * 587 + b * 114) / 1000) as u8
            }
        })
        .collect();

    if let Some(path) = dump_image {
        // Encoded through a fresh opaque pixmap rather than by hand: this
        // must show the BUFFER, and a hand-rolled PNG writer here would be a
        // second thing that could be wrong in the same place.
        let mut dump = match pdfcer_render::tiny_skia::Pixmap::new(iw, ih) {
            Some(p) => p,
            None => {
                eprintln!("pdfcer: ocr: --dump-image: could not allocate {iw}x{ih}");
                return exit::RUNTIME_ERROR;
            }
        };
        for (px, &g) in dump.pixels_mut().iter_mut().zip(grey.iter()) {
            if let Some(v) = pdfcer_render::tiny_skia::PremultipliedColorU8::from_rgba(g, g, g, 255)
            {
                *px = v;
            }
        }
        match dump.encode_png() {
            Ok(png) => {
                if let Err(err) = std::fs::write(path, &png) {
                    eprintln!("pdfcer: {}: {err}", path.display());
                    return exit::IO_ERROR;
                }
                eprintln!(
                    "pdfcer: ocr: wrote the recogniser's own input to {} ({iw}x{ih} grey)",
                    path.display()
                );
            }
            Err(err) => {
                eprintln!("pdfcer: ocr: --dump-image: {err}");
                return exit::RUNTIME_ERROR;
            }
        }
    }

    let raw = match engine.recognize(iw, ih, &grey) {
        Ok(w) => w,
        Err(err) => {
            eprintln!("pdfcer: ocr: recognition failed: {err}");
            return exit::RUNTIME_ERROR;
        }
    };

    // THE STEP THAT USED TO BE SILENTLY WRONG ON A ROTATED PAGE.
    // `page.rotate` is read and passed; `words_to_page_space_on` inverts the
    // renderer's own four transforms. Using `words_to_page_space` here would
    // be correct on `/Rotate 0` and wrong on every scan a driver rotated.
    let placement = PagePlacement::new(page.crop_box, i32::from(page.rotate));
    let placed = words_to_page_space_on(&raw, iw, ih, placement);

    let confidence_available = engine.reports_confidence();
    let ocr_page = OcrPage {
        words: placed,
        confidence_available,
    };

    if show_words {
        for w in &ocr_page.words {
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

    // TWO WRITERS, ONE PLAN. `--in-place` goes through
    // `EditSession::add_ocr_layer` and `--output` through the one-shot, and
    // both call the SAME `plan_ocr_layer` underneath -- so the font name, the
    // §7.7.3.4 resources merge, the placeable-word pass and the emitted
    // content stream are decided once. What differs is only where object
    // numbers and staged bytes come from, and where the result lands.
    //
    // The session route is the one a GUI holding an open document uses. Using
    // it here too is what stops the two shells drifting into producing
    // different files from the same input.
    // The engine name goes into the layer's marker so a later run, or another
    // tool, can tell which recogniser produced it.
    let layer_opts = layer::OcrLayerOptions::new()
        .with_engine(engine_choice.name())
        .with_existing(existing.into());
    let (bytes, report) = if in_place {
        let mut session = pdfcer_core::edit::EditSession::new(doc);
        let layers = [pdfcer_core::edit::OcrPageLayer {
            page_index: index,
            recognised: &ocr_page,
        }];
        let reports = match session.add_ocr_layer(&layers, &layer_opts) {
            Ok(r) => r,
            Err(err) => {
                eprintln!("pdfcer: ocr: {err}");
                return exit::EDIT_REFUSED;
            }
        };
        let Some(report) = reports.into_iter().next() else {
            eprintln!("pdfcer: ocr: nothing was written");
            return exit::EDIT_REFUSED;
        };
        match session.to_incremental_bytes(&pdfcer_core::writer::SaveOptions::identity()) {
            Ok((bytes, _)) => (bytes, report),
            Err(err) => {
                eprintln!("pdfcer: save refused: {err}");
                return exit::SAVE_REFUSED;
            }
        }
    } else {
        match layer::add_ocr_layer(&doc, index, &ocr_page, &layer_opts) {
            Ok(o) => (o.bytes, o.report),
            Err(err) => {
                eprintln!("pdfcer: ocr: {err}");
                // Recognising nothing is not a crash and not a malformed file;
                // it is an answer, and a common one on a blank or unreadable
                // page. Giving it the same exit code as "the PDF is broken"
                // would make a script unable to tell them apart.
                return exit::EDIT_REFUSED;
            }
        }
    };
    if let Err(err) = std::fs::write(destination, &bytes) {
        eprintln!("pdfcer: {}: {err}", destination.display());
        return exit::IO_ERROR;
    }

    println!(
        "ocr {} page={page_number} -> {} engine={} dpi={dpi} image={iw}x{ih} rotate={} \
recognised={} written={} replaced={} confidence={}",
        input.display(),
        destination.display(),
        engine_choice.name(),
        page.rotate,
        raw.len(),
        report.words_written,
        report.layers_replaced,
        if confidence_available {
            "reported"
        } else {
            "none"
        },
    );

    // Rule 4: every inference discloses itself, and in the CLI the invocation
    // IS the commit, so it is printed on the way past rather than offered for
    // review. `disclosures()` leads with the word count and adds a line for
    // every word substituted, skipped or scale-clamped.
    for line in report.disclosures() {
        eprintln!("pdfcer: ocr: {line}");
    }
    eprintln!(
        "pdfcer: ocr: models loaded from {} ({})",
        source.path().display(),
        match source {
            models::ModelSource::OperatorSupplied(_) => "--model-dir",
            models::ModelSource::BesideExecutable(_) => "beside the executable",
            models::ModelSource::UserData(_) => "user data",
        }
    );
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

    exit::SUCCESS
}
