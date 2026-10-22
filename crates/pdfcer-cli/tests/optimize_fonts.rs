//! `pdfcer optimize --unembed-fonts`: removed fonts, kept fonts with why,
//! PDF/A, and the profile carrying the choice; `--resubset-fonts`: the cut,
//! its JSON, the refusals it reports, and merging copies of one face.

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

#[test]
fn resubset_cuts_a_program_and_reports_the_glyphs_kept() {
    let path = out("resubset");
    let o = run(
        "resubset/simple.pdf",
        &["-o", path.to_str().unwrap(), "--resubset-fonts"],
    );
    let report = text(&o);
    assert_eq!(o.status.code(), Some(0), "{report}");
    assert!(report.contains("resubset: 1 cut, 0 refused"), "{report}");
    assert!(report.contains("to 5 of 8 glyphs"), "{report}");
    let written = std::fs::read(&path).unwrap();
    let input = std::fs::read(fixture("resubset/simple.pdf")).unwrap();
    assert!(written.len() < input.len());
}

#[test]
fn resubset_cuts_a_cff_program_keeping_a_seac_glyphs_parts() {
    let o = run("resubset/type1c.pdf", &["--dry-run", "--resubset-fonts"]);
    let report = text(&o);
    assert_eq!(o.status.code(), Some(0), "{report}");
    assert!(report.contains("resubset: 1 cut, 0 refused"), "{report}");
    assert!(report.contains("to 5 of 7 glyphs"), "{report}");
}

#[test]
fn resubset_cuts_a_font_listing_its_glyphs_in_cidset_or_charset() {
    for rel in ["resubset/type0-cidset.pdf", "resubset/type1c-charset.pdf"] {
        let o = run(rel, &["--dry-run", "--resubset-fonts"]);
        let report = text(&o);
        assert_eq!(o.status.code(), Some(0), "{rel}: {report}");
        assert!(
            report.contains("resubset: 1 cut, 0 refused"),
            "{rel}: {report}"
        );
    }
}

#[test]
fn resubset_json_lists_the_cut_and_a_font_using_every_glyph_is_refused() {
    let o = run(
        "resubset/type0-identity.pdf",
        &["--dry-run", "--json", "--resubset-fonts"],
    );
    let report = text(&o);
    assert!(report.contains("\"resubset\":{\"cut\":[{"), "{report}");
    assert!(
        report.contains("\"glyphs_kept\":2,\"glyphs_total\":8"),
        "{report}"
    );
    let o = run(
        "text/subset-simple-embedded.pdf",
        &["--dry-run", "--resubset-fonts"],
    );
    let report = text(&o);
    assert!(report.contains("resubset: 0 cut, 1 refused"), "{report}");
    assert!(report.contains("nothing to cut"), "{report}");
}

#[test]
fn copies_of_one_face_merge_unless_told_not_to() {
    let o = run("resubset/pair.pdf", &["--dry-run", "--resubset-fonts"]);
    let report = text(&o);
    assert!(report.contains("resubset: 1 cut, 0 refused"), "{report}");
    assert!(report.contains("11 0     merged into 7 0"), "{report}"); // string-gap-exempt: the id column is padded to 8
    let o = run(
        "resubset/pair.pdf",
        &[
            "--dry-run",
            "--json",
            "--resubset-fonts",
            "--no-merge-font-subsets",
        ],
    );
    let report = text(&o);
    assert_eq!(report.matches("\"merged\":[]}").count(), 2, "{report}");
    assert!(report.contains("\"unmerged\":[]"), "{report}");
}

#[test]
fn no_resubset_flag_cuts_nothing() {
    let o = run("resubset/simple.pdf", &["--dry-run", "--json"]);
    assert!(text(&o).contains("\"resubset\":null"), "{}", text(&o));
}
