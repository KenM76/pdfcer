//! OCR engine loading and model add-on resolution for `ocr` and
//! `ocr-models` (decision 182). Discovery is `pdfcer_core::ocr::addons`;
//! this file supplies the search roots and prints every choice made.

use super::*;
use pdfcer_core::ocr::addon_manifest::MANIFEST_FILE;
use pdfcer_core::ocr::addons::{OcrModel, OcrModelDiscovery, discover_ocr_models};

/// A loaded recogniser, whichever `--ocr-engine` chose.
///
/// An enum rather than `dyn OcrEngine` because the trait's associated error
/// type differs per engine; errors are flattened to their message here, the
/// only place the CLI needs them.
pub(crate) enum LoadedOcrEngine {
    #[cfg(feature = "ocrs")]
    Ocrs(pdfcer_core::ocr::engine_ocrs::OcrsEngine),
    #[cfg(feature = "ocrcer")]
    Ocrcer(Box<pdfcer_core::ocr::engine_ocrcer::OcrcerEngine>),
    #[cfg(feature = "paddle")]
    Paddle(Box<pdfcer_core::ocr::engine_paddle::PaddleEngine>),
    /// The engine and the last page's reading, kept for the disclosure.
    #[cfg(feature = "ocr-vl")]
    PaddleVl(
        Box<pdfcer_core::ocr::engine_paddle_vl::PaddleVlEngine>,
        std::cell::RefCell<Option<pdfcer_core::ocr::engine_paddle_vl::RegionReading>>,
    ),
    Tesseract(pdfcer_ocr_host::ProgramEngine),
}

impl LoadedOcrEngine {
    /// Recognise one page image with whichever engine was loaded.
    pub(crate) fn recognize(
        &self,
        width: u32,
        height: u32,
        pixels: &[u8],
    ) -> Result<Vec<pdfcer_core::ocr::RecognizedWord>, String> {
        #[cfg(any(feature = "ocrs", feature = "ocrcer", feature = "paddle"))]
        use pdfcer_core::ocr::OcrEngine;
        match self {
            #[cfg(feature = "ocrs")]
            Self::Ocrs(e) => e
                .recognize(width, height, pixels)
                .map_err(|e| e.to_string()),
            #[cfg(feature = "ocrcer")]
            Self::Ocrcer(e) => e
                .recognize(width, height, pixels)
                .map_err(|e| e.to_string()),
            #[cfg(feature = "paddle")]
            Self::Paddle(e) => e
                .recognize(width, height, pixels)
                .map_err(|e| e.to_string()),
            #[cfg(feature = "ocr-vl")]
            Self::PaddleVl(e, last) => {
                let reading = e
                    .read_region(width, height, pixels)
                    .map_err(|e| e.to_string())?;
                let lines = reading.lines.clone();
                *last.borrow_mut() = Some(reading);
                Ok(lines)
            }
            Self::Tesseract(e) => e
                .recognize(width, height, pixels)
                .map_err(|e| e.to_string()),
        }
    }

    /// Whether this engine reports a per-word confidence.
    pub(crate) fn reports_confidence(&self) -> bool {
        #[cfg(any(feature = "ocrs", feature = "ocrcer", feature = "paddle"))]
        use pdfcer_core::ocr::OcrEngine;
        match self {
            #[cfg(feature = "ocrs")]
            Self::Ocrs(e) => e.reports_confidence(),
            #[cfg(feature = "ocrcer")]
            Self::Ocrcer(e) => e.reports_confidence(),
            #[cfg(feature = "paddle")]
            Self::Paddle(e) => e.reports_confidence(),
            // The region's mean token probability, on every line.
            #[cfg(feature = "ocr-vl")]
            Self::PaddleVl(..) => true,
            // Tesseract's TSV carries a `conf` column on every word row.
            Self::Tesseract(_) => true,
        }
    }

    /// A line naming anything the engine chose on the operator's behalf, for
    /// the rule-4 report.
    pub(crate) fn disclosure(&self) -> Option<String> {
        match self {
            #[cfg(feature = "paddle")]
            Self::Paddle(e) => {
                use pdfcer_core::ocr::engine_paddle::{DICTIONARY, DictionarySource};
                Some(match e.dictionary_source() {
                    DictionarySource::File(p) => format!(
                        "PaddleOCR dictionary: {} ({} entries)",
                        p.display(),
                        e.dictionary_len()
                    ),
                    _ => format!(
                        "PaddleOCR dictionary: no {DICTIONARY}, so the list embedded in \
                         rec.onnx was used ({} entries)",
                        e.dictionary_len()
                    ),
                })
            }
            #[cfg(feature = "ocr-vl")]
            Self::PaddleVl(_, last) => Some(paddle_vl_disclosure(last.borrow().as_ref())),
            _ => None,
        }
    }
}

/// Rule 4 for PaddleOCR-VL: its text is per region, and the line boxes are
/// inferred from the ink, not reported by the model.
#[cfg(feature = "ocr-vl")]
fn paddle_vl_disclosure(
    reading: Option<&pdfcer_core::ocr::engine_paddle_vl::RegionReading>,
) -> String {
    use pdfcer_core::ocr::vl_decode::StopReason;
    use pdfcer_core::ocr::vl_pre::LinePlacement;
    let lead = "PaddleOCR-VL reads the page's ink as ONE region: the text layer is \
                region-aligned, one box per LINE and never per word, and those boxes are \
                INFERRED from the ink, not reported by the model";
    let Some(r) = reading else {
        return format!("{lead}.");
    };
    let placement = match r.placement {
        LinePlacement::Bands => "each line placed on its own band of inked rows",
        LinePlacement::Even => {
            "the line count did not match the inked bands, so the lines divide the ink box \
             evenly and may sit off their text"
        }
        _ => "the page had no ink, so the model was not run",
    };
    let stop = match r.stop {
        Some(StopReason::TokenLimit) => format!(
            "; WARNING: decoding hit the {}-token ceiling, so the text may be cut short",
            r.tokens
        ),
        Some(_) => format!(
            "; {} token(s) from {} image token(s)",
            r.tokens, r.image_tokens
        ),
        None => String::new(),
    };
    format!("{lead}. Last page: {placement}{stop}.")
}

/// What `ocr` was asked to use.
pub(crate) struct OcrModelChoice<'a> {
    /// `--ocr-engine`; `None` means `ocrs` unless `--ocr-model` names one.
    pub(crate) engine: Option<OcrEngineArg>,
    /// `--model-dir`: one folder, no discovery.
    pub(crate) model_dir: Option<&'a Path>,
    /// `--ocr-model NAME`.
    pub(crate) model: Option<&'a str>,
    /// `--ocr-folder`, searched after the settings file's folders.
    pub(crate) folders: &'a [PathBuf],
}

/// The folder the models were loaded from and how it was chosen.
pub(crate) struct ResolvedModel {
    pub(crate) dir: PathBuf,
    pub(crate) how: String,
}

/// Resolve the models (printing every choice made) and load the engine.
pub(crate) fn load_ocr_engine(
    choice: &OcrModelChoice<'_>,
    ocr_lang: &str,
    dpi: f32,
) -> Result<(LoadedOcrEngine, OcrEngineArg, ResolvedModel), u8> {
    let (engine, resolved) = resolve(choice)?;
    let loaded = load_from(engine, &resolved.dir, ocr_lang, dpi)?;
    Ok((loaded, engine, resolved))
}

const ALL_ENGINES: [OcrEngineArg; 5] = [
    OcrEngineArg::Ocrs,
    OcrEngineArg::Ocrcer,
    OcrEngineArg::Paddle,
    OcrEngineArg::PaddleVl,
    OcrEngineArg::Tesseract,
];

fn engine_arg(name: &str) -> Option<OcrEngineArg> {
    ALL_ENGINES.into_iter().find(|e| e.name() == name)
}

/// The engine used when none is named: `paddle`, the one model the portable
/// package bundles, or `ocrs` in a build compiled without `paddle`.
fn default_engine() -> OcrEngineArg {
    if engine_compiled(OcrEngineArg::Paddle) || !engine_compiled(OcrEngineArg::Ocrs) {
        OcrEngineArg::Paddle
    } else {
        OcrEngineArg::Ocrs
    }
}

/// Whether this build can run `engine`.
fn engine_compiled(engine: OcrEngineArg) -> bool {
    match engine {
        OcrEngineArg::Ocrs => cfg!(feature = "ocrs"),
        OcrEngineArg::Ocrcer => cfg!(feature = "ocrcer"),
        OcrEngineArg::Paddle => cfg!(feature = "paddle"),
        OcrEngineArg::PaddleVl => cfg!(feature = "ocr-vl"),
        OcrEngineArg::Tesseract => true,
    }
}

fn not_compiled(engine: OcrEngineArg) -> u8 {
    let (label, alternative) = match engine {
        OcrEngineArg::Paddle => ("PaddleOCR", "use --ocr-engine ocrs"),
        OcrEngineArg::PaddleVl => ("PaddleOCR-VL", "use --ocr-engine ocrs"),
        OcrEngineArg::Ocrcer => (
            "OCRcer",
            "use --ocr-engine ocrs (the model file it would need is `ocrcer.ocrw`)",
        ),
        _ => (engine.name(), "choose another --ocr-engine"),
    };
    eprintln!(
        "pdfcer: ocr: --ocr-engine {name}: this build was compiled without the `{name}` \
         feature, so the {label} engine is not in it. Rebuild with \
         `cargo build -p pdfcer-cli --features {feature}`, or {alternative}.",
        name = engine.name(),
        feature = engine_feature(engine)
    );
    exit::UNIMPLEMENTED
}

/// The Cargo feature that compiles `engine` in.
fn engine_feature(engine: OcrEngineArg) -> &'static str {
    match engine {
        OcrEngineArg::PaddleVl => "ocr-vl",
        other => other.name(),
    }
}

/// The files a folder must hold to count as `engine`'s models.
fn required_files(engine: OcrEngineArg) -> Vec<&'static str> {
    match engine {
        #[cfg(feature = "ocrs")]
        OcrEngineArg::Ocrs => {
            use pdfcer_core::ocr::engine_ocrs::{DETECTION_MODEL, RECOGNITION_MODEL};
            vec![DETECTION_MODEL, RECOGNITION_MODEL]
        }
        #[cfg(feature = "ocrcer")]
        OcrEngineArg::Ocrcer => vec![pdfcer_core::ocr::engine_ocrcer::MODEL_FILE],
        #[cfg(feature = "paddle")]
        OcrEngineArg::Paddle => {
            use pdfcer_core::ocr::engine_paddle::{DETECTION_MODEL, RECOGNITION_MODEL};
            vec![DETECTION_MODEL, RECOGNITION_MODEL]
        }
        #[cfg(feature = "ocr-vl")]
        OcrEngineArg::PaddleVl => pdfcer_core::ocr::engine_paddle_vl::REQUIRED_FILES.to_vec(),
        // A program add-on's files are its manifest's to name (decision 184).
        OcrEngineArg::Tesseract => Vec::new(),
        // Only an engine compiled out of this build lands here; `resolve`
        // refuses those before asking.
        #[allow(unreachable_patterns)]
        _ => Vec::new(),
    }
}

/// How to supply `engine`'s models, printed when none were found.
fn missing_hint(engine: OcrEngineArg) -> String {
    let what = match engine {
        OcrEngineArg::Ocrs => "the ocrs models are two files (`text-detection.rten`, \
             `text-rec-checkpoint.rten`) in a `models/ocrs` folder, which the ocrs add-on zip \
             installs; in a build compiled with the `download` feature, `pdfcer fetch-ocr-models` \
             fetches the pinned copies"
            .to_owned(),
        OcrEngineArg::Ocrcer => "the OCRcer model is one file, `ocrcer.ocrw`, which the \
             OCRcer add-on zip installs as `models/ocrcer` and pdfcer never downloads; the OCRcer \
             project has it as `model/out/ocrcer.ocrw`"
            .to_owned(),
        OcrEngineArg::Paddle => "the PaddleOCR models are two files, `det.onnx` and \
             `rec.onnx`, in a `models/paddle` folder, which the portable package ships (plus \
             `dict.txt` if the recognition model does not embed one)"
            .to_owned(),
        OcrEngineArg::PaddleVl => "the PaddleOCR-VL models are an add-on folder of five files \
             (`vision_encoder.onnx`, `decoder.onnx`, `embedding.onnx`, `embedding.onnx.data`, \
             `tokenizer.json`) plus a `pdfcer-ocr-model.txt` manifest, which \
             `tools/build-paddle-vl-addon.py` builds from a local copy of the Apache-2.0 \
             onnx-community/PaddleOCR-VL-1.5-ONNX export; pdfcer never downloads it"
            .to_owned(),
        OcrEngineArg::Tesseract => format!(
            "Tesseract is a program add-on: a folder holding `{}`, a `{}` folder of language \
             files and a `{MANIFEST_FILE}` with `kind = program` and the program's SHA-256; \
             its add-on zip installs as `models/tesseract`. A stock Tesseract install \
             has no manifest, so it is found only when --model-dir names it",
            pdfcer_ocr_host::tesseract::EXE_FILE,
            pdfcer_ocr_host::tesseract::TESSDATA_DIR
        ),
    };
    format!(
        "{what}. Put the folder under `models/` beside this executable, or under a folder \
         named by --ocr-folder or an `ocr_folder` line in the settings file, or pass \
         --model-dir <folder>."
    )
}

fn exe_models_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    Some(exe.parent()?.join("models"))
}

/// The add-on search roots in priority order: `models/` beside the
/// executable (when it exists), the settings file's `ocr_folder` lines, then
/// `extra` (`--ocr-folder`).
fn search_roots(extra: &[PathBuf]) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = exe_models_dir()
        .filter(|d| d.is_dir())
        .into_iter()
        .collect();
    roots.extend(settings::active().ocr_folders.iter().cloned());
    roots.extend(extra.iter().cloned());
    roots
}

fn discover(verb: &str, extra: &[PathBuf]) -> (Vec<PathBuf>, OcrModelDiscovery) {
    let roots = search_roots(extra);
    let found = discover_ocr_models(&roots);
    for note in &found.notes {
        eprintln!("pdfcer: {verb}: {note}");
    }
    (roots, found)
}

fn joined(paths: &[PathBuf]) -> String {
    if paths.is_empty() {
        return "(no folders)".to_owned();
    }
    paths
        .iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

fn resolve(choice: &OcrModelChoice<'_>) -> Result<(OcrEngineArg, ResolvedModel), u8> {
    if let Some(name) = choice.model {
        return resolve_named(name, choice);
    }
    let engine = choice.engine.unwrap_or_else(default_engine);
    if !engine_compiled(engine) {
        return Err(not_compiled(engine));
    }
    if let Some(dir) = choice.model_dir {
        return resolve_explicit(engine, dir);
    }
    let (roots, found) = discover("ocr", choice.folders);
    let required = required_files(engine);
    let matched = found.for_engine(engine.name(), &required);
    for skipped in &matched.incomplete {
        eprintln!(
            "pdfcer: ocr: passed over OCR model `{}` in {}: it lacks one of {}",
            skipped.name,
            skipped.folder.display(),
            required.join(", ")
        );
    }
    let Some(model) = matched.chosen else {
        eprintln!(
            "pdfcer: ocr: no OCR models for `{}` — looked in: {}",
            engine.name(),
            joined(&roots)
        );
        eprintln!("pdfcer: ocr: {}", missing_hint(engine));
        return Err(exit::RUNTIME_ERROR);
    };
    verify_and_disclose(model)?;
    Ok((engine, resolved_from(model)))
}

fn resolve_named(
    name: &str,
    choice: &OcrModelChoice<'_>,
) -> Result<(OcrEngineArg, ResolvedModel), u8> {
    let (roots, found) = discover("ocr", choice.folders);
    let Some(model) = found.by_name(name) else {
        eprintln!(
            "pdfcer: ocr: no OCR model named `{name}` — looked in: {} (`pdfcer ocr-models` \
             lists what is installed)",
            joined(&roots)
        );
        return Err(exit::RUNTIME_ERROR);
    };
    let Some(engine) = engine_arg(&model.engine) else {
        eprintln!(
            "pdfcer: ocr: OCR model `{name}` is for the `{}` engine, which this pdfcer does \
             not have (it has ocrs, ocrcer, paddle, paddle-vl and tesseract)",
            model.engine
        );
        return Err(exit::UNIMPLEMENTED);
    };
    if let Some(asked) = choice.engine.filter(|asked| *asked != engine) {
        eprintln!(
            "pdfcer: ocr: --ocr-model `{name}` is a `{}` model, but --ocr-engine says `{}`",
            engine.name(),
            asked.name()
        );
        return Err(exit::RUNTIME_ERROR);
    }
    if !engine_compiled(engine) {
        return Err(not_compiled(engine));
    }
    let required = required_files(engine);
    if !model.has_files(&required) {
        eprintln!(
            "pdfcer: ocr: OCR model `{name}` in {} lacks one of {}",
            model.folder.display(),
            required.join(", ")
        );
        return Err(exit::RUNTIME_ERROR);
    }
    verify_and_disclose(model)?;
    Ok((engine, resolved_from(model)))
}

/// `--model-dir`: exactly that folder. A manifest there, if any, must name
/// the same engine and its hashes must match.
fn resolve_explicit(engine: OcrEngineArg, dir: &Path) -> Result<(OcrEngineArg, ResolvedModel), u8> {
    let required = required_files(engine);
    let found = pdfcer_core::ocr::models::resolve_model_dir_with(
        engine.name(),
        Some(dir),
        None,
        None,
        &required,
    );
    if let Err(err) = found {
        eprintln!("pdfcer: ocr: {err}");
        eprintln!("pdfcer: ocr: {}", missing_hint(engine));
        return Err(exit::RUNTIME_ERROR);
    }
    if dir.join(MANIFEST_FILE).is_file() {
        let found = discover_ocr_models(&[dir.to_path_buf()]);
        for note in &found.notes {
            eprintln!("pdfcer: ocr: {note}");
        }
        let Some(model) = found.models.first() else {
            return Err(exit::RUNTIME_ERROR);
        };
        if model.engine != engine.name() {
            eprintln!(
                "pdfcer: ocr: --model-dir {} holds OCR model `{}` for the `{}` engine, not `{}`",
                dir.display(),
                model.name,
                model.engine,
                engine.name()
            );
            return Err(exit::RUNTIME_ERROR);
        }
        verify_and_disclose(model)?;
    }
    let resolved = ResolvedModel {
        dir: dir.to_path_buf(),
        how: "--model-dir".to_owned(),
    };
    Ok((engine, resolved))
}

fn resolved_from(model: &OcrModel) -> ResolvedModel {
    let beside_exe = exe_models_dir().is_some_and(|d| d == model.root);
    let how = match (&model.manifest, beside_exe) {
        (None, true) => "beside the executable".to_owned(),
        (None, false) => format!("folder `{}` under {}", model.name, model.root.display()),
        (Some(_), _) => format!("add-on `{}` under {}", model.name, model.root.display()),
    };
    ResolvedModel {
        dir: model.folder.clone(),
        how,
    }
}

/// Check the manifest's SHA-256 lines, refusing a mismatch, and name the
/// add-on in use.
fn verify_and_disclose(model: &OcrModel) -> Result<(), u8> {
    match model.verify() {
        Ok(0) => {}
        Ok(n) => eprintln!(
            "pdfcer: ocr: OCR model `{}`: {n} file(s) match the manifest's SHA-256",
            model.name
        ),
        Err(err) => {
            eprintln!(
                "pdfcer: ocr: OCR model `{}` refused: {err}. Reinstall the add-on, or delete \
                 its folder to uninstall it",
                model.name
            );
            return Err(exit::RUNTIME_ERROR);
        }
    }
    if model.manifest.is_some() {
        eprintln!(
            "pdfcer: ocr: using OCR model `{}` ({}), engine {}, licence {}, languages {}",
            model.name,
            model.label().unwrap_or("no label"),
            model.engine,
            model.licence().unwrap_or("not stated"),
            or_text(&model.languages().join(","), "not stated")
        );
    }
    Ok(())
}

fn or_text<'a>(s: &'a str, empty: &'a str) -> &'a str {
    if s.is_empty() { empty } else { s }
}

fn load_from(
    engine: OcrEngineArg,
    dir: &Path,
    lang: &str,
    dpi: f32,
) -> Result<LoadedOcrEngine, u8> {
    let failed = |err: &dyn std::fmt::Display| {
        eprintln!("pdfcer: ocr: {err}");
        exit::RUNTIME_ERROR
    };
    match engine {
        #[cfg(feature = "ocrs")]
        OcrEngineArg::Ocrs => pdfcer_core::ocr::engine_ocrs::OcrsEngine::from_model_dir(dir)
            .map(LoadedOcrEngine::Ocrs)
            .map_err(|e| failed(&e)),
        #[cfg(feature = "ocrcer")]
        OcrEngineArg::Ocrcer => load_ocrcer(dir),
        #[cfg(feature = "paddle")]
        OcrEngineArg::Paddle => pdfcer_core::ocr::engine_paddle::PaddleEngine::from_model_dir(dir)
            .map(|e| LoadedOcrEngine::Paddle(Box::new(e)))
            .map_err(|e| failed(&e)),
        #[cfg(feature = "ocr-vl")]
        OcrEngineArg::PaddleVl => {
            pdfcer_core::ocr::engine_paddle_vl::PaddleVlEngine::from_model_dir(dir)
                .map(|e| LoadedOcrEngine::PaddleVl(Box::new(e), Default::default()))
                .map_err(|e| failed(&e))
        }
        OcrEngineArg::Tesseract => {
            ocr_program::load(dir, lang, dpi).map(LoadedOcrEngine::Tesseract)
        }
        // Compiled-out engines are refused in `resolve`.
        #[allow(unreachable_patterns)]
        other => Err(not_compiled(other)),
    }
}

#[cfg(feature = "ocrcer")]
fn load_ocrcer(dir: &Path) -> Result<LoadedOcrEngine, u8> {
    use pdfcer_core::ocr::engine_ocrcer::{MODEL_FILE, OcrcerEngine};
    let path = dir.join(MODEL_FILE);
    let bytes = std::fs::read(&path).map_err(|err| {
        eprintln!("pdfcer: {}: {err}", path.display());
        exit::IO_ERROR
    })?;
    match OcrcerEngine::from_bytes(&bytes) {
        Ok(e) => Ok(LoadedOcrEngine::Ocrcer(Box::new(e))),
        Err(err) => {
            eprintln!(
                "pdfcer: ocr: {}: not a usable OCRcer model: {err}",
                path.display()
            );
            Err(exit::RUNTIME_ERROR)
        }
    }
}

/// `ocr-models` — list the OCR models this pdfcer can find, one line each
/// on stdout. Reads folder listings and manifests only; `verify` also hashes
/// every file a manifest lists, and exits 1 if any differs.
pub(crate) fn cmd_ocr_models(extra: &[PathBuf], verify: bool) -> u8 {
    let (roots, found) = discover("ocr-models", extra);
    for root in &roots {
        eprintln!("pdfcer: ocr-models: searched {}", root.display());
    }
    if let Some(dir) = exe_models_dir().filter(|d| !d.is_dir()) {
        eprintln!(
            "pdfcer: ocr-models: {} does not exist; it is searched first when it does",
            dir.display()
        );
    }
    let mut failed = false;
    for model in &found.models {
        let mut line = model_line(model);
        if let Err(why) = runnable(model) {
            eprintln!("pdfcer: ocr-models: `{}` will not run: {why}", model.name);
        }
        if verify {
            match model.verify() {
                Ok(n) => line.push_str(&format!(" verified={n}")),
                Err(err) => {
                    eprintln!("pdfcer: ocr-models: `{}`: {err}", model.name);
                    line.push_str(" verified=FAILED");
                    failed = true;
                }
            }
        }
        println!("{line}");
    }
    eprintln!("pdfcer: ocr-models: {} model(s) found", found.models.len());
    if failed {
        exit::RUNTIME_ERROR
    } else {
        exit::SUCCESS
    }
}

/// Whether `model` can run in this build under the program policy.
fn runnable(model: &OcrModel) -> Result<(), String> {
    use pdfcer_core::ocr::addon_manifest::AddonKind;
    if model.kind() == AddonKind::Program {
        return ocr_program::status(model);
    }
    let engine = engine_arg(&model.engine)
        .filter(|e| engine_compiled(*e))
        .ok_or_else(|| format!("this build has no `{}` engine", model.engine))?;
    if engine == OcrEngineArg::Tesseract {
        return Err("a `tesseract` add-on needs `kind = program`".to_owned());
    }
    let required = required_files(engine);
    if model.has_files(&required) {
        Ok(())
    } else {
        Err(format!("it lacks one of {}", required.join(", ")))
    }
}

fn model_line(model: &OcrModel) -> String {
    let in_build = match engine_arg(&model.engine) {
        Some(e) if engine_compiled(e) => "yes",
        Some(_) => "no",
        None => "unknown-engine",
    };
    let version = model.manifest.as_ref().and_then(|m| m.version.as_deref());
    let program = model
        .program()
        .map_or_else(String::new, |p| format!(" program={p:?}"));
    format!(
        "ocr-model {} engine={} in-build={in_build} kind={}{program} runnable={} label={:?} \
         languages={} licence={} version={} folder={:?}",
        model.name,
        model.engine,
        model.kind().as_str(),
        if runnable(model).is_ok() { "yes" } else { "no" },
        model.label().unwrap_or(""),
        or_text(&model.languages().join(","), "-"),
        model.licence().unwrap_or("-"),
        version.unwrap_or("-"),
        model.folder.display().to_string()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(feature = "paddle")]
    fn the_default_engine_is_the_bundled_one() {
        assert_eq!(default_engine(), OcrEngineArg::Paddle);
    }
}
