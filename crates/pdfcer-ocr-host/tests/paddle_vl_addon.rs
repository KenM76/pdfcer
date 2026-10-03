//! PaddleOCR-VL add-on folders (engine token `paddle-vl`) through
//! `check_runnable` and `OcrRunner::load`. Real recognition needs the
//! add-on's model files, which are never committed; these folders hold
//! placeholder bytes.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use std::path::PathBuf;

use pdfcer_core::ocr::addon_manifest::MANIFEST_FILE;
use pdfcer_core::ocr::addons::{OcrModel, discover_ocr_models};
use pdfcer_ocr_host::{ProgramPolicy, RunnerError, check_runnable};

/// `engine_paddle_vl::REQUIRED_FILES`, spelled out so a lean build (no
/// `ocr-vl`, so no engine module) can still build the folder.
const VL_FILES: [&str; 5] = [
    "vision_encoder.onnx",
    "decoder.onnx",
    "embedding.onnx",
    "embedding.onnx.data",
    "tokenizer.json",
];

#[cfg(feature = "ocr-vl")]
#[test]
fn the_spelled_out_list_is_the_engines() {
    let mut ours = VL_FILES.to_vec();
    let mut engine = pdfcer_core::ocr::engine_paddle_vl::REQUIRED_FILES.to_vec();
    ours.sort_unstable();
    engine.sort_unstable();
    assert_eq!(ours, engine);
}

/// A `paddle-vl` add-on folder holding every required file but `skip`.
fn vl_model(tag: &str, skip: Option<&str>) -> OcrModel {
    let root =
        std::env::temp_dir().join(format!("pdfcer-ocr-host-vl-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let dir: PathBuf = root.join("vl");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(MANIFEST_FILE), "name = vl\nengine = paddle-vl\n").unwrap();
    for f in VL_FILES.iter().filter(|f| Some(**f) != skip) {
        std::fs::write(dir.join(f), b"placeholder").unwrap();
    }
    let found = discover_ocr_models(&[root]);
    assert!(found.notes.is_empty(), "{:?}", found.notes);
    found.models.into_iter().next().unwrap()
}

#[cfg(feature = "ocr-vl")]
#[test]
fn a_complete_vl_folder_is_runnable_and_its_engine_judges_the_files() {
    let model = vl_model("complete", None);
    check_runnable(&model, ProgramPolicy::Allow).unwrap();
    // Placeholder bytes reach the engine, which refuses them: the runner
    // knows the engine, so this is not `EngineNotInBuild`.
    let err =
        pdfcer_ocr_host::OcrRunner::load(&model, &pdfcer_ocr_host::RunOptions::new("eng", 300.0))
            .unwrap_err();
    assert!(matches!(err, RunnerError::Engine(_)), "{err:?}");
}

#[cfg(feature = "ocr-vl")]
#[test]
fn a_vl_folder_missing_a_file_names_the_required_set() {
    let model = vl_model("missing", Some("tokenizer.json"));
    match check_runnable(&model, ProgramPolicy::Allow) {
        Err(RunnerError::MissingFile { needs, .. }) => {
            assert!(needs.contains("tokenizer.json"), "{needs}");
            assert!(needs.contains("embedding.onnx.data"), "{needs}");
        }
        other => panic!("{other:?}"),
    }
}

#[cfg(feature = "ocr-vl")]
#[test]
fn the_vl_disclosure_says_the_line_boxes_are_inferred() {
    let line = pdfcer_ocr_host::paddle_vl_disclosure(None);
    assert!(line.contains("INFERRED from the ink"), "{line}");
    assert!(line.contains("never per word"), "{line}");
}

#[cfg(not(feature = "ocr-vl"))]
#[test]
fn without_the_feature_a_vl_folder_names_its_engine() {
    let model = vl_model("lean", None);
    match check_runnable(&model, ProgramPolicy::Allow) {
        Err(RunnerError::EngineNotInBuild { engine, .. }) => assert_eq!(engine, "paddle-vl"),
        other => panic!("{other:?}"),
    }
}

#[cfg(feature = "ocr-vl")]
#[test]
fn the_runner_stays_send_and_sync() {
    fn shareable<T: Send + Sync>() {}
    shareable::<pdfcer_ocr_host::OcrRunner>();
}
