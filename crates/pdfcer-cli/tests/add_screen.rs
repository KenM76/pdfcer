//! `add-screen` (`Pass 261.3`): a media clip embedded behind a screen
//! annotation, with the inferred MIME type and the temp-file default printed.

use std::path::{Path, PathBuf};
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

fn temp_out(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pdfcer-add-screen-tests-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let p = dir.join(name);
    let _ = std::fs::remove_file(&p);
    p
}

fn run(args: &[&str]) -> (i32, String, String) {
    let out = Command::new(BIN).args(args).output().expect("pdfcer runs");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn s(p: &Path) -> &str {
    p.to_str().expect("utf-8 path")
}

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/minimal.pdf")
}

fn clip(name: &str) -> PathBuf {
    let p = temp_out(name);
    std::fs::write(&p, b"\0\0\0\x18ftypmp42 synthetic").expect("clip written");
    p
}

#[test]
fn an_mp4_is_embedded_with_its_inferred_type_and_the_defaults_are_reported() {
    let file = clip("walk.mp4");
    let out = temp_out("screen.pdf");
    let (code, stdout, stderr) = run(&[
        "add-screen",
        s(&fixture()),
        "--file",
        s(&file),
        "--page",
        "1",
        "--rect",
        "72,400,372,600",
        "--title",
        "Walkthrough",
        "--apply",
        "-o",
        s(&out),
    ]);
    assert_eq!(code, 0, "{stdout}\n{stderr}");
    for needle in [
        "content-type=video/mp4",
        "trigger=click",
        "temp=TEMPACCESS",
        "inferred: content-type video/mp4 from the file extension",
        "default: temp=TEMPACCESS",
    ] {
        assert!(stdout.contains(needle), "missing {needle}: {stdout}");
    }
    let text = String::from_utf8_lossy(&std::fs::read(&out).expect("output written")).into_owned();
    for needle in [
        "/Subtype /Screen",
        "/S /Rendition",
        "/S /MR",
        "/S /MCD",
        "/Type /Filespec",
        "(walk.mp4)",
    ] {
        assert!(text.contains(needle), "missing {needle}");
    }
}

#[test]
fn an_explicit_type_is_not_reported_as_inferred_and_an_unknown_extension_is_refused() {
    let file = clip("tone.bin");
    let out = temp_out("dry.pdf");
    let (code, stdout, stderr) = run(&[
        "add-screen",
        s(&fixture()),
        "--file",
        s(&file),
        "--page",
        "1",
        "--rect",
        "72,400,372,600",
        "--content-type",
        "audio/mpeg",
        "--trigger",
        "page-open",
        "--temp",
        "never",
        "-o",
        s(&out),
    ]);
    assert_eq!(code, 0, "{stdout}\n{stderr}");
    assert!(
        stdout.contains("content-type=audio/mpeg")
            && stdout.contains("trigger=page-open")
            && stdout.contains("temp=TEMPNEVER")
            && stdout.contains("applied=0")
            && !stdout.contains("inferred:")
            && !stdout.contains("default:"),
        "{stdout}"
    );
    assert!(!out.exists());

    let (code, _, stderr) = run(&[
        "add-screen",
        s(&fixture()),
        "--file",
        s(&file),
        "--page",
        "1",
        "--rect",
        "72,400,372,600",
    ]);
    assert_ne!(code, 0);
    assert!(stderr.contains("pass --content-type"), "{stderr}");
}
