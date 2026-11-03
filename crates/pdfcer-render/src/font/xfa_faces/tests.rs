#![allow(clippy::expect_used)] // a panic here is the test failing

use pdfcer_core::xfa::FontProgramKind;

use super::*;

fn fixture(name: &str) -> FontData {
    let path = format!(
        "{}/../../fixtures/synthetic/xfa/fonts/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    FontData::new(std::fs::read(path).expect("fixture font"))
}

fn env() -> FontEnvironment {
    let mut env = FontEnvironment::bundled();
    for (name, file) in [
        ("XfaTestSans-Regular", "XfaTestSans-Regular.ttf"),
        ("XfaTestSans-Bold", "XfaTestSans-Bold.ttf"),
        ("XfaTestSans-Cond", "XfaTestSans-Cond.ttf"),
        ("XfaTestSerif-Regular", "XfaTestSerif-Regular.otf"),
        ("XfaTestLocked-Regular", "XfaTestLocked-Regular.ttf"),
    ] {
        env.insert_named(name, fixture(file));
    }
    env
}

#[test]
fn a_typeface_matches_its_family_at_the_asked_weight_and_normal_width() {
    let fonts = xfa_fonts(&env());
    let regular = fonts.face("Xfa Test Sans", false, false).expect("face");
    assert_eq!(regular.base_font, "XfaTestSans-Regular");
    assert_eq!(regular.width(b'A'), Some(800));
    assert_eq!(regular.width(b' '), Some(400));
    let bold = fonts.face("xfa-test-sans", true, false).expect("bold");
    assert_eq!(bold.base_font, "XfaTestSans-Bold");
    assert!(bold.bold);
}

#[test]
fn a_face_carries_its_metrics_and_program_kind() {
    let fonts = xfa_fonts(&env());
    let f = fonts.face("Xfa Test Sans", false, false).expect("face");
    assert_eq!((f.ascent, f.descent, f.cap_height), (800.0, -200.0, 700.0));
    let p = f.program.as_ref().expect("program");
    assert_eq!(p.kind, FontProgramKind::TrueType);
    assert_eq!(p.embedding_refusal(), None);
    let serif = fonts.face("Xfa Test Serif", false, false).expect("serif");
    let p = serif.program.as_ref().expect("program");
    assert_eq!(p.kind, FontProgramKind::OpenTypeCff);
}

#[test]
fn a_restricted_face_still_measures_but_refuses_embedding() {
    let fonts = xfa_fonts(&env());
    let f = fonts.face("Xfa Test Locked", false, false).expect("face");
    assert_eq!(f.width(b'x'), Some(600));
    let p = f.program.as_ref().expect("program");
    assert_eq!(p.embedding_refusal(), Some("Restricted License embedding"));
}

#[test]
fn an_unregistered_family_or_a_prefix_only_match_is_none() {
    let fonts = xfa_fonts(&env());
    assert!(fonts.face("Myriad Pro", false, false).is_none());
    // "Xfa Test" prefixes every file's name but is no face's family.
    assert!(fonts.face("Xfa Test", false, false).is_none());
}
