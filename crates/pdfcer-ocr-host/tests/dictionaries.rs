//! Word-list options (`Dictionaries`) through program add-ons, run against
//! the crate's stand-in engine, and their refusal by in-process engines.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use std::path::{Path, PathBuf};

use pdfcer_core::ocr::addon_manifest::MANIFEST_FILE;
use pdfcer_core::ocr::addons::{OcrModel, discover_ocr_models};
use pdfcer_ocr_host::{
    Dictionaries, MAX_USER_WORDS_BYTES, OcrRunner, ProgramEngine, ProgramError, RunOptions,
    RunnerError,
};

const TEST_ENGINE: &str = env!("CARGO_BIN_EXE_pdfcer-ocr-test-engine");

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("pdfcer-ocr-dict-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A stock-layout folder: the stand-in engine as `tesseract` plus
/// `tessdata/eng.traineddata`.
fn stock(tag: &str) -> PathBuf {
    let dir = scratch(tag).join("stock");
    std::fs::create_dir_all(dir.join("tessdata")).unwrap();
    std::fs::write(dir.join("tessdata/eng.traineddata"), b"eng").unwrap();
    let exe = format!("tesseract{}", std::env::consts::EXE_SUFFIX);
    std::fs::copy(TEST_ENGINE, dir.join(exe)).unwrap();
    dir
}

fn load(dir: &Path, dictionaries: Dictionaries) -> Result<OcrRunner, ProgramError> {
    let options = RunOptions::new("eng", 300.0).with_dictionaries(dictionaries);
    ProgramEngine::from_operator_folder(dir, &options).map(OcrRunner::from_program)
}

/// The words after the stand-in engine's fixed five.
fn extra_words(runner: &OcrRunner) -> Vec<String> {
    let words = runner.recognize(2, 1, &[0, 255]).unwrap();
    words.into_iter().skip(5).map(|w| w.text).collect()
}

#[test]
fn the_default_passes_no_word_list_settings() {
    let dir = stock("default");
    let runner = load(&dir, Dictionaries::default()).unwrap();
    assert!(extra_words(&runner).is_empty());
    assert_eq!(
        runner.dictionary_note(),
        "Tesseract's built-in word lists for eng"
    );
}

#[test]
fn none_turns_both_built_in_lists_off() {
    let dir = stock("none");
    let runner = load(&dir, Dictionaries::none()).unwrap();
    assert_eq!(extra_words(&runner), ["nodawg"]);
    assert!(
        runner.dictionary_note().contains("load_system_dawg=0"),
        "{}",
        runner.dictionary_note()
    );
}

#[test]
fn user_word_files_are_merged_deduplicated_and_cleaned() {
    let dir = stock("merge");
    let a = dir.join("a.txt");
    let b = dir.join("b.txt");
    std::fs::write(&a, "\u{feff}FLANGE\r\n\r\n  GUSSET  \r\nFLANGE\r\n").unwrap();
    std::fs::write(&b, "GUSSET\nCLEVIS\n").unwrap();
    let dictionaries = Dictionaries::none().with_user_words(&a).with_user_words(&b);
    let runner = load(&dir, dictionaries).unwrap();
    assert_eq!(
        extra_words(&runner),
        ["nodawg", "words:FLANGE,GUSSET,CLEVIS"]
    );
    assert!(
        runner.dictionary_note().ends_with(" + 3 user word(s)"),
        "{}",
        runner.dictionary_note()
    );
}

#[test]
fn the_merged_file_goes_with_the_engine() {
    let dir = stock("cleanup");
    let a = dir.join("a.txt");
    std::fs::write(&a, "MERGED-FILE-CLEANUP-PROBE\n").unwrap();
    let runner = load(&dir, Dictionaries::builtin().with_user_words(&a)).unwrap();
    let merged = merged_files_holding("MERGED-FILE-CLEANUP-PROBE\n");
    assert_eq!(merged.len(), 1);
    drop(runner);
    assert!(!merged[0].exists());
}

/// This process's merged word files in the temp folder holding exactly
/// `text`; other tests may be merging their own files meanwhile.
fn merged_files_holding(text: &str) -> Vec<PathBuf> {
    let prefix = format!("pdfcer-user-words-{}-", std::process::id());
    std::fs::read_dir(std::env::temp_dir())
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with(&prefix))
                && std::fs::read_to_string(p).is_ok_and(|t| t == text)
        })
        .collect()
}

fn setup_error(dir: &Path, dictionaries: Dictionaries) -> String {
    match load(dir, dictionaries) {
        Err(ProgramError::Setup(message)) => message,
        other => panic!("{other:?}"),
    }
}

#[test]
fn unusable_word_files_are_refused_at_load() {
    let dir = stock("bad");
    let blank = dir.join("blank.txt");
    std::fs::write(&blank, "\r\n  \n").unwrap();
    let latin1 = dir.join("latin1.txt");
    std::fs::write(&latin1, b"caf\xe9\n").unwrap();
    let missing = dir.join("missing.txt");

    let message = setup_error(&dir, Dictionaries::builtin().with_user_words(&blank));
    assert!(message.contains("hold no words"), "{message}");
    let message = setup_error(&dir, Dictionaries::builtin().with_user_words(&latin1));
    assert!(message.contains("not UTF-8"), "{message}");
    let message = setup_error(&dir, Dictionaries::builtin().with_user_words(&missing));
    assert!(message.contains("missing.txt"), "{message}");
}

#[test]
fn word_files_past_the_size_cap_are_refused() {
    let dir = stock("cap");
    let half = usize::try_from(MAX_USER_WORDS_BYTES / 2).unwrap();
    let a = dir.join("a.txt");
    let b = dir.join("b.txt");
    std::fs::write(&a, "x\n".repeat(half / 2)).unwrap();
    std::fs::write(&b, "y\n".repeat(half / 2 + 1)).unwrap();
    let both = Dictionaries::builtin()
        .with_user_words(&a)
        .with_user_words(&b);
    let message = setup_error(&dir, both);
    assert!(message.contains("exceed"), "{message}");
    // Either file alone is under the cap.
    load(&dir, Dictionaries::builtin().with_user_words(&a)).unwrap();
}

/// An in-process add-on folder for `engine`, with no model files: the
/// word-list check comes before the folder is checked.
fn in_process(tag: &str, engine: &str) -> OcrModel {
    let root = scratch(tag);
    let dir = root.join("m");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join(MANIFEST_FILE),
        format!("name = m\nengine = {engine}\n"),
    )
    .unwrap();
    let found = discover_ocr_models(&[root]);
    assert!(found.notes.is_empty(), "{:?}", found.notes);
    found.models.into_iter().next().unwrap()
}

fn refusal(model: &OcrModel, dictionaries: Dictionaries) -> Option<&'static str> {
    let options = RunOptions::new("eng", 300.0).with_dictionaries(dictionaries);
    match OcrRunner::load(model, &options) {
        Err(RunnerError::DictionariesUnsupported { why, .. }) => Some(why),
        _ => None,
    }
}

#[test]
fn in_process_engines_refuse_user_words_and_vl_refuses_none() {
    for engine in ["ocrs", "ocrcer", "paddle", "paddle-vl"] {
        let model = in_process(&format!("inproc-{engine}"), engine);
        assert_eq!(
            refusal(&model, Dictionaries::builtin().with_user_words("w.txt")),
            Some("this engine takes no word list"),
            "{engine}"
        );
        assert_eq!(refusal(&model, Dictionaries::builtin()), None, "{engine}");
        let none = refusal(&model, Dictionaries::none());
        if engine == "paddle-vl" {
            assert_eq!(none, Some("its language model cannot be turned off"));
        } else {
            assert_eq!(none, None, "{engine}");
        }
    }
}

#[test]
fn the_display_form_names_the_choice() {
    assert_eq!(Dictionaries::default().to_string(), "built-in word lists");
    assert_eq!(
        Dictionaries::none().with_user_words("a.txt").to_string(),
        "no built-in word lists + user words from a.txt"
    );
}
