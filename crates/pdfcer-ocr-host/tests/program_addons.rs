//! Program add-ons (decision 184), run against the crate's stand-in engine.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use std::path::{Path, PathBuf};

use pdfcer_core::ocr::addon_manifest::MANIFEST_FILE;
use pdfcer_core::ocr::addons::{
    DiscoveryNote, OcrModel, VerifyError, check_digest, discover_ocr_models,
};
use pdfcer_ocr_host::{
    OcrRunner, ProgramEngine, ProgramError, ProgramPolicy, ProgramRefusal, ProgramSource,
    RunOptions, RunnerError, check_runnable, program_status,
};

const TEST_ENGINE: &str = env!("CARGO_BIN_EXE_pdfcer-ocr-test-engine");

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("pdfcer-ocr-host-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn exe(stem: &str) -> String {
    format!("{stem}{}", std::env::consts::EXE_SUFFIX)
}

fn sha256_hex(path: &Path) -> String {
    let mut f = std::fs::File::open(path).unwrap();
    match check_digest(path, &mut f, &[0; 32]) {
        Err(VerifyError::Mismatch { actual, .. }) => actual,
        other => panic!("{other:?}"),
    }
}

/// A folder with two copies of the stand-in engine, `engine-a` and
/// `engine-b`, plus `tessdata/eng.traineddata`.
fn two_engines(tag: &str) -> PathBuf {
    let dir = scratch(tag).join("addon");
    std::fs::create_dir_all(dir.join("tessdata")).unwrap();
    std::fs::write(dir.join("tessdata").join("eng.traineddata"), b"eng").unwrap();
    for stem in ["engine-a", "engine-b"] {
        std::fs::copy(TEST_ENGINE, dir.join(exe(stem))).unwrap();
    }
    dir
}

/// Write a program manifest for `program`, hashing it and each of `also`.
fn manifest(dir: &Path, program: &str, extra: &str, also: &[&str]) {
    let mut text =
        format!("name = t\nengine = tesseract\nkind = program\nprogram = {program}\n{extra}");
    for f in std::iter::once(&program).chain(also) {
        let hex = sha256_hex(&f.split('/').fold(dir.to_path_buf(), |p, c| p.join(c)));
        text.push_str(&format!("sha256 = {f} {hex}\n"));
    }
    std::fs::write(dir.join(MANIFEST_FILE), text).unwrap();
}

fn only_model(dir: &Path) -> OcrModel {
    let found = discover_ocr_models(&[dir.to_path_buf()]);
    assert!(found.notes.is_empty(), "{:?}", found.notes);
    found.models.into_iter().next().unwrap()
}

fn words(runner: &OcrRunner) -> Vec<String> {
    runner
        .recognize(2, 1, &[0, 255])
        .unwrap()
        .into_iter()
        .map(|w| w.text)
        .collect()
}

#[test]
fn only_the_named_program_runs() {
    let dir = two_engines("named");
    let options = RunOptions::new("eng", 300.0);
    for stem in ["engine-a", "engine-b"] {
        manifest(&dir, &exe(stem), "", &["tessdata/eng.traineddata"]);
        let model = only_model(&dir);
        check_runnable(&model, options.policy).unwrap();
        let runner = OcrRunner::load(&model, &options).unwrap();
        assert_eq!(runner.as_program().unwrap().program(), dir.join(exe(stem)));
        assert_eq!(words(&runner), [stem, "tessdata", "eng", "pgm", "300"]);
        assert_eq!(
            runner.as_program().unwrap().source(),
            &ProgramSource::Addon {
                name: "t".into(),
                hashed_files: 2
            }
        );
    }
}

#[test]
fn each_page_can_tell_the_program_its_own_resolution() {
    let dir = two_engines("dpi");
    manifest(&dir, &exe("engine-a"), "", &["tessdata/eng.traineddata"]);
    let runner = OcrRunner::load(&only_model(&dir), &RunOptions::new("eng", 300.0)).unwrap();
    let dpi_told = |dpi: f32| {
        let words = runner.recognize_at(2, 1, &[0, 255], dpi).unwrap();
        words.into_iter().last().unwrap().text
    };
    assert_eq!(dpi_told(150.0), "150");
    assert_eq!(dpi_told(600.4), "600");
    assert_eq!(dpi_told(f32::NAN), "1", "a nonsense resolution is clamped");
    assert_eq!(
        words(&runner)[4],
        "300",
        "recognize keeps the load-time dpi"
    );
}

#[test]
fn the_data_key_names_the_folder_passed_to_the_program() {
    let dir = two_engines("data");
    std::fs::create_dir_all(dir.join("langs").join("fast")).unwrap();
    std::fs::write(dir.join("langs/fast/deu.traineddata"), b"deu").unwrap();
    manifest(&dir, &exe("engine-a"), "data = langs/fast\n", &[]);
    let runner = OcrRunner::load(&only_model(&dir), &RunOptions::new("deu", 300.0)).unwrap();
    assert_eq!(words(&runner), ["engine-a", "fast", "deu", "pgm", "300"]);
}

#[test]
fn a_program_without_a_hash_is_listed_but_not_run() {
    let dir = two_engines("nohash");
    std::fs::write(
        dir.join(MANIFEST_FILE),
        format!(
            "name = t\nengine = tesseract\nkind = program\nprogram = {}\n",
            exe("engine-a")
        ),
    )
    .unwrap();
    let model = only_model(&dir);
    let refusal = program_status(&model, ProgramPolicy::Allow).unwrap_err();
    assert!(matches!(refusal, ProgramRefusal::NoProgramHash { .. }));
    assert!(
        refusal.to_string().contains("no `sha256` line"),
        "{refusal}"
    );
    let err = OcrRunner::load(&model, &RunOptions::new("eng", 300.0)).unwrap_err();
    assert!(err.to_string().contains("no `sha256` line"), "{err}");
}

#[test]
fn a_program_name_with_a_path_is_not_a_model() {
    let dir = two_engines("badname");
    std::fs::write(
        dir.join(MANIFEST_FILE),
        "name = t\nengine = tesseract\nkind = program\nprogram = ../engine-a\n",
    )
    .unwrap();
    let found = discover_ocr_models(std::slice::from_ref(&dir));
    assert!(found.models.is_empty());
    assert!(
        matches!(&found.notes[..], [DiscoveryNote::BadManifest { .. }]),
        "{:?}",
        found.notes
    );
}

#[test]
fn a_changed_file_is_refused_at_load_and_before_each_run() {
    let dir = two_engines("mismatch");
    manifest(&dir, &exe("engine-a"), "", &["tessdata/eng.traineddata"]);
    let model = only_model(&dir);
    let options = RunOptions::new("eng", 300.0);
    let runner = OcrRunner::load(&model, &options).unwrap();
    assert_eq!(words(&runner)[0], "engine-a");

    let data = dir.join("tessdata").join("eng.traineddata");
    std::fs::write(&data, b"changed").unwrap();
    let err = runner.recognize(2, 1, &[0, 255]).unwrap_err().to_string();
    assert!(
        err.contains("eng.traineddata") && err.contains("the manifest says"),
        "{err}"
    );
    let err = OcrRunner::load(&model, &options).unwrap_err().to_string();
    assert!(err.contains("eng.traineddata"), "{err}");

    std::fs::write(&data, b"eng").unwrap();
    std::fs::copy(dir.join(exe("engine-b")), dir.join(exe("engine-a"))).unwrap();
    std::fs::OpenOptions::new()
        .append(true)
        .open(dir.join(exe("engine-a")))
        .and_then(|mut f| std::io::Write::write_all(&mut f, b"x"))
        .unwrap();
    let err = runner.recognize(2, 1, &[0, 255]).unwrap_err().to_string();
    assert!(err.contains(&exe("engine-a")), "{err}");
}

#[test]
fn the_refuse_policy_lists_and_never_runs() {
    let dir = two_engines("refuse");
    manifest(&dir, &exe("engine-a"), "", &[]);
    let model = only_model(&dir);
    let mut options = RunOptions::new("eng", 300.0);
    options.policy = ProgramPolicy::Refuse;
    let refusal = program_status(&model, options.policy).unwrap_err();
    assert!(matches!(refusal, ProgramRefusal::RefusedByPolicy { .. }));
    assert!(refusal.to_string().contains("turned off"), "{refusal}");
    assert!(check_runnable(&model, options.policy).is_err());
    let err = OcrRunner::load(&model, &options).unwrap_err();
    assert!(
        matches!(err, RunnerError::Program(ProgramError::Refused(_))),
        "{err:?}"
    );
    let stock = stock_folder("refuse-stock");
    let err = ProgramEngine::from_operator_folder(&stock, &options).unwrap_err();
    assert!(matches!(err, ProgramError::Refused(_)), "{err:?}");
}

fn stock_folder(tag: &str) -> PathBuf {
    let dir = scratch(tag);
    std::fs::create_dir_all(dir.join("tessdata")).unwrap();
    std::fs::write(dir.join("tessdata").join("eng.traineddata"), b"eng").unwrap();
    std::fs::copy(TEST_ENGINE, dir.join(pdfcer_ocr_host::tesseract::EXE_FILE)).unwrap();
    dir
}

#[test]
fn an_operator_named_stock_folder_runs_without_a_manifest() {
    let dir = stock_folder("stock");
    let engine = ProgramEngine::from_operator_folder(&dir, &RunOptions::new("eng", 300.0)).unwrap();
    assert_eq!(engine.source(), &ProgramSource::OperatorFolder(dir.clone()));
    let runner = OcrRunner::from_program(engine);
    assert_eq!(
        words(&runner),
        ["tesseract", "tessdata", "eng", "pgm", "300"]
    );
    let err = ProgramEngine::from_operator_folder(&dir, &RunOptions::new("deu", 300.0))
        .unwrap_err()
        .to_string();
    assert!(err.contains("deu.traineddata"), "{err}");
}

#[test]
fn a_data_kind_tesseract_or_an_unknown_program_engine_is_refused() {
    let dir = two_engines("kinds");
    std::fs::write(dir.join(MANIFEST_FILE), "name = t\nengine = tesseract\n").unwrap();
    let err = check_runnable(&only_model(&dir), ProgramPolicy::Allow).unwrap_err();
    assert!(
        matches!(err, RunnerError::NeedsProgramKind { .. }),
        "{err:?}"
    );

    manifest(&dir, &exe("engine-a"), "", &[]);
    let text = std::fs::read_to_string(dir.join(MANIFEST_FILE))
        .unwrap()
        .replace("engine = tesseract", "engine = future");
    std::fs::write(dir.join(MANIFEST_FILE), text).unwrap();
    let refusal = program_status(&only_model(&dir), ProgramPolicy::Allow).unwrap_err();
    assert!(
        matches!(refusal, ProgramRefusal::NoProtocol { .. }),
        "{refusal:?}"
    );
}

/// Against a real Tesseract: `PDFCER_TEST_TESSERACT_DIR` or
/// `target/tesseract-bundle`, given a generated manifest in a scratch copy.
#[test]
#[ignore = "needs a Tesseract build; run with --ignored"]
fn a_real_tesseract_runs_as_a_program_addon() {
    let src = std::env::var_os("PDFCER_TEST_TESSERACT_DIR").map_or_else(
        || Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tesseract-bundle"),
        PathBuf::from,
    );
    let exe_file = pdfcer_ocr_host::tesseract::EXE_FILE;
    let dir = scratch("real");
    std::fs::create_dir_all(dir.join("tessdata")).unwrap();
    std::fs::copy(src.join(exe_file), dir.join(exe_file)).unwrap();
    std::fs::copy(
        src.join("tessdata/eng.traineddata"),
        dir.join("tessdata/eng.traineddata"),
    )
    .unwrap();
    manifest(&dir, exe_file, "", &["tessdata/eng.traineddata"]);
    let runner = OcrRunner::load(&only_model(&dir), &RunOptions::new("eng", 300.0)).unwrap();
    let blank = vec![255u8; 64 * 64];
    assert!(runner.recognize(64, 64, &blank).unwrap().is_empty());
}
