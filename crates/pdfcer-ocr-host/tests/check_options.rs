//! `check_options`: the options a model can honour, answered without
//! loading or hashing it, and the same answer `OcrRunner::load` gives.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use std::path::PathBuf;

use pdfcer_core::ocr::addon_manifest::MANIFEST_FILE;
use pdfcer_core::ocr::addons::{OcrModel, discover_ocr_models};
use pdfcer_ocr_host::{
    Dictionaries, OcrRunner, ProgramError, ProgramPolicy, RunOptions, RunnerError, check_options,
    check_runnable,
};

const TEST_ENGINE: &str = env!("CARGO_BIN_EXE_pdfcer-ocr-test-engine");

/// An add-on folder holding `manifest` and each of `files` (placeholder
/// bytes), discovered as a model.
fn model(tag: &str, manifest: &str, files: &[&str]) -> OcrModel {
    let root = std::env::temp_dir().join(format!("pdfcer-ocr-opts-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let dir: PathBuf = root.join("m");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(MANIFEST_FILE), manifest).unwrap();
    for f in files {
        let path = dir.join(f);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"placeholder").unwrap();
    }
    let found = discover_ocr_models(&[root]);
    assert!(found.notes.is_empty(), "{:?}", found.notes);
    found.models.into_iter().next().unwrap()
}

fn data_model(tag: &str, engine: &str, files: &[&str]) -> OcrModel {
    model(tag, &format!("name = m\nengine = {engine}\n"), files)
}

/// A program add-on (the stand-in engine, no hash line) with English data.
fn program_model(tag: &str) -> OcrModel {
    let exe = format!("engine{}", std::env::consts::EXE_SUFFIX);
    let manifest = format!("name = p\nengine = tesseract\nkind = program\nprogram = {exe}\n");
    let m = model(tag, &manifest, &["tessdata/eng.traineddata"]);
    std::fs::copy(TEST_ENGINE, m.folder.join(&exe)).unwrap();
    m
}

fn opts() -> RunOptions {
    RunOptions::new("eng", 300.0)
}

#[test]
fn the_word_list_rules_are_answered_with_no_model_files_present() {
    let model = data_model("words", "ocrs", &[]);
    let options = opts().with_dictionaries(Dictionaries::builtin().with_user_words("w.txt"));
    let err = check_options(&model, &options).unwrap_err();
    assert!(
        matches!(err, RunnerError::DictionariesUnsupported { .. }),
        "{err:?}"
    );
    let loaded = OcrRunner::load(&model, &options).unwrap_err();
    assert_eq!(err.to_string(), loaded.to_string());
    check_options(&model, &opts()).unwrap();
}

#[test]
fn only_paddle_vl_reads_by_layout() {
    let model = data_model("layout-ocrs", "ocrs", &[]);
    let err = check_options(&model, &opts().with_layout(true)).unwrap_err();
    assert!(
        matches!(err, RunnerError::LayoutUnsupported { .. }),
        "{err:?}"
    );
}

#[cfg(feature = "ocr-vl")]
#[test]
fn layout_reading_needs_the_layout_model_in_the_folder() {
    use pdfcer_core::ocr::engine_layout::LAYOUT_MODEL;
    let without = data_model("vl-nolayout", "paddle-vl", &[]);
    let options = opts().with_layout(true);
    let err = check_options(&without, &options).unwrap_err();
    match &err {
        RunnerError::MissingFile { needs, .. } => assert_eq!(needs, LAYOUT_MODEL),
        other => panic!("{other:?}"),
    }
    let with = data_model("vl-layout", "paddle-vl", &[LAYOUT_MODEL]);
    check_options(&with, &options).unwrap();
    // Without layout reading the file is not asked for.
    check_options(&without, &opts()).unwrap();
}

#[test]
fn a_program_needs_the_requested_language_data() {
    let model = program_model("prog");
    check_options(&model, &opts()).unwrap();
    let err = check_options(&model, &RunOptions::new("deu", 300.0)).unwrap_err();
    match &err {
        RunnerError::Program(ProgramError::Setup(why)) => {
            assert!(why.contains("deu.traineddata"), "{why}");
        }
        other => panic!("{other:?}"),
    }
    // Options and model are separate questions: these options pass while
    // the model itself (no hash line) is refused.
    assert!(check_runnable(&model, ProgramPolicy::Allow).is_err());
}

#[test]
fn a_program_refuses_an_unreadable_word_file() {
    let model = program_model("prog-words");
    let missing = model.folder.join("no-such-words.txt");
    let options = opts().with_dictionaries(Dictionaries::builtin().with_user_words(missing));
    let err = check_options(&model, &options).unwrap_err();
    assert!(
        matches!(err, RunnerError::Program(ProgramError::Setup(_))),
        "{err:?}"
    );
}
