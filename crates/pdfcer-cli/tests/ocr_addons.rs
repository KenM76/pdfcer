//! OCR model add-on folders (decision 182): `ocr-models`, `--ocr-folder`,
//! the settings file's `ocr_folder`, `--ocr-model` and manifest SHA-256.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

/// SHA-256 of the three bytes `abc` (FIPS 180-2 appendix B.1).
const ABC_SHA256: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

fn scan() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/ocr/scan.pdf")
}

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pdfcer-ocr-addons-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// An add-on folder `root/name` with a manifest and a `det.onnx` of `abc`.
fn addon(root: &Path, name: &str, engine: &str) -> PathBuf {
    let dir = root.join(name);
    std::fs::create_dir_all(&dir).unwrap();
    let manifest = format!(
        "name = {name}\nengine = {engine}\nlabel = \"Test {name}\"\nlanguages = de, fr\n\
         licence = MIT\nversion = 1.2\nsha256 = det.onnx {ABC_SHA256}\n"
    );
    std::fs::write(dir.join("pdfcer-ocr-model.txt"), manifest).unwrap();
    std::fs::write(dir.join("det.onnx"), b"abc").unwrap();
    dir
}

fn run(args: &[&std::ffi::OsStr]) -> (Option<i32>, String, String) {
    let o: Output = Command::new(BIN).args(args).output().unwrap();
    (
        o.status.code(),
        String::from_utf8_lossy(&o.stdout).into_owned(),
        String::from_utf8_lossy(&o.stderr).into_owned(),
    )
}

fn list(extra: &[&Path]) -> (Option<i32>, String, String) {
    let mut args: Vec<&std::ffi::OsStr> = vec!["--no-settings".as_ref(), "ocr-models".as_ref()];
    for root in extra {
        args.push("--ocr-folder".as_ref());
        args.push(root.as_os_str());
    }
    args.push("--verify".as_ref());
    run(&args)
}

fn ocr_with(root: &Path, extra: &[&str]) -> (Option<i32>, String, String) {
    let out = root.join("out.pdf");
    let input = scan();
    let mut args: Vec<&std::ffi::OsStr> = vec![
        "--no-settings".as_ref(),
        "ocr".as_ref(),
        input.as_os_str(),
        "-o".as_ref(),
        out.as_os_str(),
        "--ocr-folder".as_ref(),
        root.as_os_str(),
    ];
    args.extend(extra.iter().map(|s| std::ffi::OsStr::new(*s)));
    run(&args)
}

/// Dropping a folder in installs it, every manifest field is listed, a
/// second root's same-named add-on is shadowed and said to be, and deleting
/// the folder uninstalls it.
#[test]
fn a_dropped_folder_is_listed_and_deleting_it_uninstalls() {
    let a = scratch("list-a");
    let b = scratch("list-b");
    let kept = addon(&a, "mine", "paddle");
    addon(&b, "mine", "ocrs");
    std::fs::create_dir_all(a.join("tesseract")).unwrap();
    let (code, out, err) = list(&[&a, &b]);
    assert_eq!(code, Some(0), "{err}");
    let line = out
        .lines()
        .find(|l| l.starts_with("ocr-model mine "))
        .unwrap();
    for field in [
        "engine=paddle",
        "label=\"Test mine\"",
        "languages=de,fr",
        "licence=MIT",
        "version=1.2",
        "verified=1",
    ] {
        assert!(line.contains(field), "{field} missing: {line}");
    }
    assert!(
        line.contains(&format!("{:?}", kept.display().to_string())),
        "{line}"
    );
    // A program is never bare: a `tesseract` folder without a manifest is
    // not a model (decision 184).
    assert!(!out.contains("ocr-model tesseract"), "{out}");
    assert_eq!(out.lines().count(), 1, "{out}");
    assert!(err.contains("is shadowed by the one in"), "{err}");
    assert!(err.contains("1 model(s) found"), "{err}");

    std::fs::remove_dir_all(&kept).unwrap();
    let (_, out, _) = list(&[&a, &b]);
    assert!(
        out.contains("engine=ocrs"),
        "the shadowed one surfaces: {out}"
    );
}

/// `ocr_folder` in a settings file is searched like `--ocr-folder`.
#[test]
fn the_settings_file_ocr_folder_is_searched() {
    let root = scratch("settings");
    addon(&root.join("addons"), "from-settings", "paddle");
    let settings = root.join("pdfcer-settings.txt");
    std::fs::write(&settings, "ocr_folder = addons\n").unwrap();
    let (code, out, err) = run(&[
        "--settings".as_ref(),
        settings.as_os_str(),
        "ocr-models".as_ref(),
    ]);
    assert_eq!(code, Some(0), "{err}");
    assert!(
        out.contains("ocr-model from-settings engine=paddle"),
        "{out}"
    );
    assert!(err.contains("ocr_folders=1"), "{err}");
}

/// A changed file fails `--verify` and is refused by `ocr --ocr-model`.
#[test]
fn a_hash_mismatch_is_refused() {
    let root = scratch("mismatch");
    let dir = addon(&root, "tampered", "paddle");
    std::fs::write(dir.join("det.onnx"), b"abd").unwrap();
    std::fs::write(dir.join("rec.onnx"), b"unlisted, so not hashed").unwrap();
    let (code, out, err) = list(&[&root]);
    assert_eq!(code, Some(1), "{err}");
    assert!(out.contains("verified=FAILED"), "{out}");
    assert!(err.contains(ABC_SHA256), "{err}");

    let (code, _, err) = ocr_with(&root, &["--ocr-model", "tampered"]);
    assert_eq!(code, Some(1), "{err}");
    assert!(err.contains("OCR model `tampered` refused"), "{err}");
    assert!(err.contains("the manifest says"), "{err}");
}

/// An unknown name, an engine pdfcer lacks and a contradicting
/// `--ocr-engine` are each refused by name.
#[test]
fn ocr_model_refusals_name_the_problem() {
    let root = scratch("refusals");
    addon(&root, "future", "quantum");
    addon(&root, "mine", "paddle");

    let (code, _, err) = ocr_with(&root, &["--ocr-model", "absent"]);
    assert_eq!(code, Some(1), "{err}");
    assert!(err.contains("no OCR model named `absent`"), "{err}");

    let (code, _, err) = ocr_with(&root, &["--ocr-model", "future"]);
    assert_eq!(code, Some(64), "{err}");
    assert!(err.contains("the `quantum` engine"), "{err}");

    let (code, _, err) = ocr_with(&root, &["--ocr-model", "mine", "--ocr-engine", "tesseract"]);
    assert_eq!(code, Some(1), "{err}");
    assert!(
        err.contains("is a `paddle` model, but --ocr-engine says `tesseract`"),
        "{err}"
    );

    let (_, out, _) = list(&[&root]);
    assert!(
        out.contains("ocr-model future engine=quantum in-build=unknown-engine"),
        "{out}"
    );
}

/// The shipped PaddleOCR folder carries a manifest whose hashes match, and
/// `--ocr-engine paddle` finds it through `--ocr-folder` and names it.
#[cfg(feature = "paddle")]
#[test]
fn the_shipped_paddle_manifest_verifies_and_is_used() {
    let assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("../pdfcer-core/assets/models");
    let (code, out, err) = list(&[&assets]);
    assert_eq!(code, Some(0), "{err}");
    assert!(
        out.contains("ocr-model ppocrv4-ch-en engine=paddle in-build=yes"),
        "{out}"
    );
    assert!(out.contains("licence=Apache-2.0"), "{out}");
    assert!(out.contains("verified=2"), "{out}");

    let out_dir = scratch("paddle-run");
    let (code, _, err) = run(&[
        "--no-settings".as_ref(),
        "ocr".as_ref(),
        scan().as_os_str(),
        "-o".as_ref(),
        out_dir.join("out.pdf").as_os_str(),
        "--ocr-engine".as_ref(),
        "paddle".as_ref(),
        "--ocr-folder".as_ref(),
        assets.as_os_str(),
    ]);
    assert_eq!(code, Some(0), "{err}");
    assert!(err.contains("using OCR model `ppocrv4-ch-en`"), "{err}");
    assert!(
        err.contains("2 file(s) match the manifest's SHA-256"),
        "{err}"
    );
    assert!(err.contains("(add-on `ppocrv4-ch-en` under"), "{err}");
}

/// A `paddle-vl` manifest routes `--ocr-model` to the PaddleOCR-VL engine:
/// listed as in the build, refused by name when a model file is missing, and
/// handed to that engine's loader when all five files are present.
#[cfg(feature = "ocr-vl")]
#[test]
fn a_paddle_vl_manifest_routes_to_the_vl_engine() {
    let root = scratch("paddle-vl");
    let dir = addon(&root, "vl", "paddle-vl");
    let (code, out, err) = list(&[&root]);
    assert_eq!(code, Some(0), "{err}");
    assert!(
        out.contains("ocr-model vl engine=paddle-vl in-build=yes"),
        "{out}"
    );

    let (code, _, err) = ocr_with(&root, &["--ocr-model", "vl"]);
    assert_eq!(code, Some(1), "{err}");
    assert!(
        err.contains("lacks one of vision_encoder.onnx, decoder.onnx"),
        "{err}"
    );

    for f in [
        "vision_encoder.onnx",
        "decoder.onnx",
        "embedding.onnx",
        "embedding.onnx.data",
    ] {
        std::fs::write(dir.join(f), b"not a model").unwrap();
    }
    std::fs::write(dir.join("tokenizer.json"), b"{\"model\": 1}").unwrap();
    let (code, _, err) = ocr_with(&root, &["--ocr-model", "vl"]);
    assert_eq!(code, Some(1), "{err}");
    assert!(err.contains("engine paddle-vl"), "{err}");
    assert!(err.contains("tokenizer.json"), "the VL loader ran: {err}");
}

/// End to end with a real add-on folder built by
/// `tools/build-paddle-vl-addon.py`, named by `PDFCER_PADDLE_VL_DIR`.
#[cfg(feature = "ocr-vl")]
#[test]
#[ignore = "needs a PaddleOCR-VL add-on folder in PDFCER_PADDLE_VL_DIR"]
fn paddle_vl_reads_the_clean_scan() {
    let model = PathBuf::from(std::env::var_os("PDFCER_PADDLE_VL_DIR").unwrap());
    let input =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/ocr/scan_clean.pdf");
    let out_dir = scratch("paddle-vl-e2e");
    let out = out_dir.join("out.pdf");
    let (code, _, err) = run(&[
        "--no-settings".as_ref(),
        "ocr".as_ref(),
        input.as_os_str(),
        "-o".as_ref(),
        out.as_os_str(),
        "--ocr-engine".as_ref(),
        "paddle-vl".as_ref(),
        "--model-dir".as_ref(),
        model.as_os_str(),
    ]);
    eprintln!("{err}");
    assert_eq!(code, Some(0), "{err}");
    assert!(err.contains("region-aligned"), "{err}");
    assert!(err.contains("INFERRED"), "{err}");
    assert!(
        err.contains("Last page: "),
        "the reading reached the report: {err}"
    );
    assert!(err.contains(" image token(s)"), "{err}");
    let (code, found, err) = run(&[
        "find-text".as_ref(),
        out.as_os_str(),
        "--needle".as_ref(),
        "sleeping".as_ref(),
    ]);
    assert_eq!(code, Some(0), "{err}");
    assert!(found.contains("sleeping"), "{found}");
}

/// `--user-words` with an engine that takes no word list is refused by
/// name, and nothing is written.
#[cfg(feature = "paddle")]
#[test]
fn user_words_are_refused_by_an_engine_without_word_lists() {
    let root = scratch("user-words");
    let dir = addon(&root, "mine", "paddle");
    std::fs::write(dir.join("rec.onnx"), b"placeholder").unwrap();
    let words = root.join("words.txt");
    std::fs::write(&words, "FLANGE\n").unwrap();
    let words = words.to_str().unwrap();
    let (code, _, err) = ocr_with(&root, &["--ocr-model", "mine", "--user-words", words]);
    assert_eq!(code, Some(1), "{err}");
    assert!(err.contains("this engine takes no word list"), "{err}");
    assert!(!root.join("out.pdf").exists());
}

/// `--layout` is refused for an engine without a layout model, and
/// `--region-layers` without `--layout` is a usage error; nothing is written.
#[cfg(feature = "paddle")]
#[test]
fn layout_flags_are_refused_where_they_cannot_apply() {
    let root = scratch("layout-refusals");
    let dir = addon(&root, "mine", "paddle");
    std::fs::write(dir.join("rec.onnx"), b"placeholder").unwrap();
    let (code, _, err) = ocr_with(&root, &["--ocr-model", "mine", "--layout"]);
    assert_eq!(code, Some(1), "{err}");
    assert!(
        err.contains("--layout needs --ocr-engine paddle-vl"),
        "{err}"
    );
    let (code, _, err) = ocr_with(&root, &["--region-layers"]);
    assert_eq!(code, Some(2), "{err}");
    assert!(err.contains("--layout"), "{err}");
    assert!(!root.join("out.pdf").exists());
}

/// A `paddle-vl` add-on without `layout.onnx` is refused by `--layout`,
/// naming the missing file.
#[cfg(feature = "ocr-vl")]
#[test]
fn layout_needs_the_layout_model_in_the_add_on() {
    let root = scratch("layout-missing");
    let dir = addon(&root, "vl", "paddle-vl");
    for f in [
        "vision_encoder.onnx",
        "decoder.onnx",
        "embedding.onnx",
        "embedding.onnx.data",
        "tokenizer.json",
    ] {
        std::fs::write(dir.join(f), b"not a model").unwrap();
    }
    let (code, _, err) = ocr_with(&root, &["--ocr-model", "vl", "--layout"]);
    assert_eq!(code, Some(1), "{err}");
    assert!(err.contains("layout.onnx"), "{err}");
}

/// End to end with layout regions and per-region layers, on a real add-on
/// folder carrying `layout.onnx` (named by `PDFCER_PADDLE_VL_DIR`).
#[cfg(feature = "ocr-vl")]
#[test]
#[ignore = "needs a PaddleOCR-VL add-on folder with layout.onnx in PDFCER_PADDLE_VL_DIR"]
fn paddle_vl_layout_puts_regions_on_their_own_layers() {
    let model = PathBuf::from(std::env::var_os("PDFCER_PADDLE_VL_DIR").unwrap());
    let input =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/ocr/scan_clean.pdf");
    let out_dir = scratch("paddle-vl-layout");
    let out = out_dir.join("out.pdf");
    let (code, _, err) = run(&[
        "--no-settings".as_ref(),
        "ocr".as_ref(),
        input.as_os_str(),
        "-o".as_ref(),
        out.as_os_str(),
        "--ocr-engine".as_ref(),
        "paddle-vl".as_ref(),
        "--model-dir".as_ref(),
        model.as_os_str(),
        "--layout".as_ref(),
        "--region-layers".as_ref(),
    ]);
    eprintln!("{err}");
    assert_eq!(code, Some(0), "{err}");
    assert!(err.contains("region(s) found"), "{err}");
    assert!(err.contains("OCR text: text"), "{err}");
    let (code, layers, err) = run(&["list-layers".as_ref(), out.as_os_str()]);
    assert_eq!(code, Some(0), "{err}");
    assert!(layers.contains("OCR text: text"), "{layers}");
}
