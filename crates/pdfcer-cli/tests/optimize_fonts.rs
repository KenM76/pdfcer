//! `pdfcer optimize --unembed-fonts`: removed fonts, kept fonts with why,
//! PDF/A, and the profile carrying the choice.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // Tests fail loudly.

use std::path::PathBuf;
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_pdfcer");

fn fixture(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic")
        .join(rel)
}

fn out(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("pdfcer-optfonts-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d.join("out.pdf")
}

fn run(rel: &str, args: &[&str]) -> Output {
    Command::new(BIN)
        .arg("optimize")
        .arg(fixture(rel))
        .args(args)
        .output()
        .unwrap()
}

fn text(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

#[test]
fn all_unembeds_and_reports_each_font_and_the_bytes() {
    let path = out("all");
    let o = run(
        "text/subset-simple-embedded.pdf",
        &["-o", path.to_str().unwrap(), "--unembed-fonts", "all"],
    );
    let report = text(&o);
    assert_eq!(o.status.code(), Some(0), "{report}");
    assert!(report.contains("fonts: 1 unembedded"), "{report}");
    assert!(report.contains(" unembed "), "{report}");
    assert!(report.contains("look different"), "{report}");
    let written = std::fs::read(&path).unwrap();
    assert!(
        !written.windows(9).any(|w| w == b"/FontFile"),
        "a program survived"
    );
    let input = std::fs::read(fixture("text/subset-simple-embedded.pdf")).unwrap();
    assert!(written.len() < input.len());
}

#[test]
fn no_flag_touches_no_font() {
    let o = run("text/subset-simple-embedded.pdf", &["--dry-run"]);
    let report = text(&o);
    assert!(!report.contains("fonts:"), "{report}");
}

#[test]
fn a_pdfa_document_keeps_its_fonts() {
    let o = run(
        "unembed/unembed-pdfa.pdf",
        &["--dry-run", "--unembed-fonts", "all"],
    );
    let report = text(&o);
    assert!(report.contains("fonts kept embedded"), "{report}");
    assert!(report.contains("PDF/A"), "{report}");
    assert!(!report.contains("fonts: "), "{report}");
}

#[test]
fn a_name_matching_nothing_is_reported() {
    let o = run(
        "text/subset-simple-embedded.pdf",
        &["--dry-run", "--unembed-fonts", "NoSuchFont"],
    );
    let report = text(&o);
    assert!(
        report.contains("no font is named \"NoSuchFont\""),
        "{report}"
    );
}

#[test]
fn json_lists_unembedded_and_refused_fonts() {
    let o = run(
        "unembed/unembed-shared-descriptor.pdf",
        &["--dry-run", "--json", "--unembed-fonts", "all"],
    );
    let report = text(&o);
    assert!(report.contains("\"fonts\":{\"unembedded\":["), "{report}");
    assert!(report.contains("\"refused\":[{"), "{report}");
}

#[test]
fn the_profile_carries_the_font_choice() {
    let o = Command::new(BIN)
        .args([
            "optimize",
            "--write-profile",
            "-",
            "--unembed-fonts",
            "Arial,Helvetica",
            "--keep-subset-tag",
        ])
        .output()
        .unwrap();
    let profile = text(&o);
    assert!(
        profile.contains("unembed-fonts = [\"Arial\", \"Helvetica\"]"),
        "{profile}"
    );
    assert!(profile.contains("keep-subset-tag = true"), "{profile}");
}
