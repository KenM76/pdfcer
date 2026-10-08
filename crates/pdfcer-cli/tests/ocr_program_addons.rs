//! Program OCR add-ons through the CLI (decision 184): the listing, the
//! `ocr_program_addons` setting and `--refuse-ocr-programs`, and the line
//! naming the program an `ocr` run starts. A copy of `pdfcer` itself stands
//! in for the program; it does not speak Tesseract's protocol, so a run
//! that reaches it fails after the disclosure line.

use crate::scratch_dir::Scratch;
use std::path::{Path, PathBuf};
use std::process::Command;

use pdfcer_core::ocr::addons::{VerifyError, check_digest};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");
const EXE: &str = pdfcer_ocr_host::tesseract::EXE_FILE;

fn scan() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/ocr/scan.pdf")
}

fn sha256_hex(path: &Path) -> String {
    let mut f = std::fs::File::open(path).unwrap();
    match check_digest(path, &mut f, &[0; 32]) {
        Err(VerifyError::Mismatch { actual, .. }) => actual,
        other => panic!("{other:?}"),
    }
}

/// `root/tess`: a program add-on whose program is a copy of `pdfcer`.
/// `hashed` decides whether the manifest carries the program's SHA-256.
fn program_addon(tag: &str, hashed: bool) -> (Scratch, PathBuf) {
    let root =
        std::env::temp_dir().join(format!("pdfcer-ocr-program-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let dir = root.join("tess");
    std::fs::create_dir_all(dir.join("tessdata")).unwrap();
    std::fs::write(dir.join("tessdata/eng.traineddata"), b"eng").unwrap();
    std::fs::copy(BIN, dir.join(EXE)).unwrap();
    let mut manifest =
        format!("name = tess\nengine = tesseract\nkind = program\nprogram = {EXE}\n");
    if hashed {
        manifest.push_str(&format!("sha256 = {EXE} {}\n", sha256_hex(&dir.join(EXE))));
    }
    std::fs::write(dir.join("pdfcer-ocr-model.txt"), manifest).unwrap();
    (Scratch(root), dir)
}

fn run(args: &[&std::ffi::OsStr]) -> (Option<i32>, String, String) {
    let o = Command::new(BIN).args(args).output().unwrap();
    (
        o.status.code(),
        String::from_utf8_lossy(&o.stdout).into_owned(),
        String::from_utf8_lossy(&o.stderr).into_owned(),
    )
}

fn list(root: &Path, extra: &[&str]) -> (Option<i32>, String, String) {
    let mut args: Vec<&std::ffi::OsStr> = vec![
        "--no-settings".as_ref(),
        "ocr-models".as_ref(),
        "--ocr-folder".as_ref(),
        root.as_os_str(),
    ];
    args.extend(extra.iter().map(|s| std::ffi::OsStr::new(*s)));
    run(&args)
}

fn ocr(root: &Path, pre: &[&std::ffi::OsStr], extra: &[&str]) -> (Option<i32>, String) {
    let input = scan();
    let out = root.join("out.pdf");
    let mut args: Vec<&std::ffi::OsStr> = pre.to_vec();
    args.extend([
        "ocr".as_ref(),
        input.as_os_str(),
        "-o".as_ref(),
        out.as_os_str(),
        "--ocr-folder".as_ref(),
        root.as_os_str(),
        "--ocr-model".as_ref(),
        "tess".as_ref(),
    ]);
    args.extend(extra.iter().map(|s| std::ffi::OsStr::new(*s)));
    let (code, _, err) = run(&args);
    (code, err)
}

#[test]
fn a_program_addon_lists_its_kind_and_program() {
    let (root, _) = program_addon("list", true);
    let (code, out, err) = list(&root, &[]);
    assert_eq!(code, Some(0), "{err}");
    let line = out
        .lines()
        .find(|l| l.starts_with("ocr-model tess "))
        .unwrap();
    for field in [
        "engine=tesseract in-build=yes",
        "kind=program",
        &format!("program={EXE:?}"),
        "runnable=yes",
    ] {
        assert!(line.contains(field), "{field} missing: {line}");
    }
}

#[test]
fn an_unhashed_program_is_listed_not_runnable_and_never_run() {
    let (root, _) = program_addon("nohash", false);
    let (_, out, err) = list(&root, &[]);
    assert!(out.contains("runnable=no"), "{out}");
    assert!(err.contains("no `sha256` line"), "{err}");
    let (code, err) = ocr(&root, &["--no-settings".as_ref()], &[]);
    assert_eq!(code, Some(1), "{err}");
    assert!(err.contains("no `sha256` line"), "{err}");
    assert!(!err.contains("running program"), "{err}");
}

#[test]
fn the_flag_and_the_setting_each_refuse_programs() {
    let (root, _) = program_addon("refuse", true);
    let (_, out, err) = list(&root, &["--refuse-ocr-programs"]);
    assert!(out.contains("runnable=no"), "{out}");
    assert!(err.contains("turned off"), "{err}");

    let (code, err) = ocr(
        &root,
        &["--no-settings".as_ref()],
        &["--refuse-ocr-programs"],
    );
    assert_eq!(code, Some(1), "{err}");
    assert!(err.contains("turned off"), "{err}");
    assert!(!err.contains("running program"), "{err}");

    let settings = root.join("pdfcer-settings.txt");
    std::fs::write(&settings, "ocr_program_addons = refuse\n").unwrap();
    let (code, err) = ocr(&root, &["--settings".as_ref(), settings.as_os_str()], &[]);
    assert_eq!(code, Some(1), "{err}");
    assert!(err.contains("ocr_program_addons=refuse"), "{err}");
    assert!(err.contains("turned off"), "{err}");
    assert!(!err.contains("running program"), "{err}");
}

#[test]
fn an_ocr_run_names_the_program_it_starts() {
    let (root, dir) = program_addon("run", true);
    let (code, err) = ocr(&root, &["--no-settings".as_ref()], &[]);
    let line = err
        .lines()
        .find(|l| l.contains("running program"))
        .unwrap_or_else(|| panic!("{err}"));
    assert!(
        line.contains(&dir.join(EXE).display().to_string()),
        "{line}"
    );
    assert!(line.contains("OCR model `tess`"), "{line}");
    assert!(line.contains("1 file(s) are re-checked"), "{line}");
    // The stand-in does not speak Tesseract's protocol.
    assert_eq!(code, Some(1), "{err}");
}

#[test]
fn a_changed_program_is_refused_by_name() {
    let (root, dir) = program_addon("changed", true);
    std::fs::OpenOptions::new()
        .append(true)
        .open(dir.join(EXE))
        .and_then(|mut f| std::io::Write::write_all(&mut f, b"x"))
        .unwrap();
    let (code, err) = ocr(&root, &["--no-settings".as_ref()], &[]);
    assert_eq!(code, Some(1), "{err}");
    assert!(
        err.contains(EXE) && err.contains("the manifest says"),
        "{err}"
    );
    assert!(!err.contains("running program"), "{err}");
}
