//! Program OCR add-ons for `ocr` and `ocr-models` (decision 184): the
//! policy (settings key plus `--refuse-ocr-programs`), loading a Tesseract
//! folder, and the stderr lines that say which program runs.

use super::*;
use pdfcer_core::ocr::addon_manifest::MANIFEST_FILE;
use pdfcer_core::ocr::addons::{OcrModel, discover_ocr_models};
use pdfcer_ocr_host::{ProgramEngine, ProgramPolicy, ProgramSource, RunOptions};
use std::sync::OnceLock;

static REFUSE_FLAG: OnceLock<bool> = OnceLock::new();

/// Record `--refuse-ocr-programs` for this process.
pub(crate) fn set_refuse_flag(refuse: bool) {
    let _ = REFUSE_FLAG.set(refuse);
}

/// The policy in force: the settings file's `ocr_program_addons`, made
/// stricter by `--refuse-ocr-programs`. Neither can loosen the other.
pub(crate) fn policy() -> ProgramPolicy {
    let flag = if REFUSE_FLAG.get().copied().unwrap_or(false) {
        ProgramPolicy::Refuse
    } else {
        ProgramPolicy::Allow
    };
    settings::active().ocr_program_addons.and(flag)
}

/// Whether a program add-on can run, as `ocr-models` reports it.
pub(crate) fn status(model: &OcrModel) -> Result<(), String> {
    pdfcer_ocr_host::check_runnable(model, policy()).map_err(|e| e.to_string())
}

/// Load the Tesseract program in `dir`: the add-on whose manifest is there,
/// or, with no manifest, the stock install `--model-dir` named (run as
/// named, nothing hashed). Prints which program will run.
pub(crate) fn load(
    dir: &Path,
    lang: &str,
    dpi: f32,
    dictionaries: &pdfcer_ocr_host::Dictionaries,
) -> Result<ProgramEngine, u8> {
    let failed = |err: &dyn std::fmt::Display| {
        eprintln!("pdfcer: ocr: {err}");
        exit::RUNTIME_ERROR
    };
    let mut options = RunOptions::new(lang, dpi).with_dictionaries(dictionaries.clone());
    options.policy = policy();
    let engine = if dir.join(MANIFEST_FILE).is_file() {
        let found = discover_ocr_models(&[dir.to_path_buf()]);
        let model = found.models.first().ok_or(exit::RUNTIME_ERROR)?;
        pdfcer_ocr_host::check_runnable(model, options.policy).map_err(|e| failed(&e))?;
        ProgramEngine::from_model(model, &options).map_err(|e| failed(&e))?
    } else {
        ProgramEngine::from_operator_folder(dir, &options).map_err(|e| failed(&e))?
    };
    let from = match engine.source() {
        ProgramSource::Addon { name, hashed_files } => format!(
            "from OCR model `{name}`; {hashed_files} file(s) are re-checked against the \
             manifest's SHA-256 before each page"
        ),
        _ => {
            "from the folder --model-dir named; it has no manifest, so nothing is hashed".to_owned()
        }
    };
    eprintln!(
        "pdfcer: ocr: running program {} (-l {}) {from}",
        engine.program().display(),
        engine.languages()
    );
    Ok(engine)
}
