#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic
)] // tests

use crate::document::Document;
use crate::edit::{CommandKind, EditSession, FixPdfuaOptions, PdfuaFixup};
use crate::filters::flate;
use crate::optimize::ResampleMethod;
use crate::optimize::tests::build;
use crate::pageops::tests_support::build_pdf_bytes;
use crate::preflight::{
    self, Check, Fixup, FixupOutcome, FixupScope, FixupTools, PdfuaPart, Profile, ProfileRule,
    Severity, hairlines_in,
};
use crate::space_audit::tests::stream;
use crate::transparency::FlattenOptions;

/// Two pages, each with one 0.1 pt stroke and one 1 pt stroke.
fn two_pages() -> Vec<u8> {
    let content = "<< /Length 40 >>\nstream\n0.1 w 0 0 20 20 re S 1 w 30 30 5 5 re S\nendstream";
    build_pdf_bytes(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>"),
        (2, "<< /Type /Pages /Kids [3 0 R 5 0 R] /Count 2 >>"),
        (
            3,
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents 4 0 R >>",
        ),
        (4, content),
        (
            5,
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents 6 0 R >>",
        ),
        (6, content),
    ])
}

/// A 300 x 300 grey noise image (object 5) drawn 100 pt wide: 216 ppi.
fn image_doc() -> Vec<u8> {
    let mut x = 0x9E37_79B9u32;
    let px: Vec<u8> = (0..300u32 * 300)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            (x >> 24) as u8
        })
        .collect();
    image_doc_from(&px)
}

/// [`image_doc`] with these 300 x 300 pixels.
fn image_doc_from(px: &[u8]) -> Vec<u8> {
    let image = stream(
        "/Type /XObject /Subtype /Image /Width 300 /Height 300 /ColorSpace /DeviceGray \
         /BitsPerComponent 8 /Filter /FlateDecode",
        &flate::encode_with_level(px, 9),
    );
    build(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 400] /Contents 4 0 R \
          /Resources << /XObject << /Im5 5 0 R >> >> >>"
            .to_vec(),
        stream("", b"q 100 0 0 100 0 0 cm /Im5 Do Q"),
        image,
    ])
}

fn session(bytes: Vec<u8>) -> EditSession {
    EditSession::new(Document::from_bytes(bytes).unwrap())
}

fn profile(rules: Vec<ProfileRule>) -> Profile {
    Profile {
        name: "test".into(),
        description: "A test profile.".into(),
        rules,
    }
}

fn hairline_rule() -> ProfileRule {
    ProfileRule::new(
        "hairline",
        Check::StrokeBelow { min_width_pt: 0.25 },
        Severity::Error,
    )
}

fn hairlines_on(s: &mut EditSession, page: usize) -> usize {
    hairlines_in(&s.page_objects(page).unwrap(), 0.25).len()
}

#[test]
fn all_scope_is_one_undo_step_that_undo_takes_back() {
    let mut s = session(two_pages());
    let depth = s.undo_depth();
    let fixup = Fixup::FixHairlines { min_width_pt: 0.25 };
    let r = s.apply_fixup(&fixup, &FixupScope::All, &FixupTools::new());
    assert_eq!(r.steps.len(), 1);
    assert_eq!(r.steps[0].outcome, FixupOutcome::Applied);
    assert_eq!(r.steps[0].count, 2);
    assert_eq!(r.undo_entries, 1);
    assert_eq!(s.undo_depth(), depth + 1);
    assert_eq!((hairlines_on(&mut s, 0), hairlines_on(&mut s, 1)), (0, 0));
    assert_eq!(s.undo(), Some(CommandKind::ApplyFixup));
    assert_eq!((hairlines_on(&mut s, 0), hairlines_on(&mut s, 1)), (1, 1));
    assert!(
        r.to_text()
            .contains("fixup fix-hairlines: applied (2 changed)")
    );
}

#[test]
fn a_finding_scope_reaches_only_its_own_page() {
    let mut s = session(two_pages());
    let p = profile(vec![hairline_rule()]);
    let report = preflight::run(&s.view(), &p);
    let second = report
        .findings
        .iter()
        .find(|f| f.page == Some(2))
        .unwrap()
        .clone();
    let fixup = Fixup::FixHairlines { min_width_pt: 0.25 };
    let r = s.apply_fixup(&fixup, &FixupScope::Finding(&second), &FixupTools::new());
    assert_eq!(r.steps[0].count, 1);
    assert_eq!((hairlines_on(&mut s, 0), hairlines_on(&mut s, 1)), (1, 0));
}

#[test]
fn a_rule_scope_with_no_violation_runs_nothing_and_says_so() {
    let mut s = session(two_pages());
    let depth = s.undo_depth();
    let scope = FixupScope::Rule {
        rule_id: "absent",
        findings: &[],
    };
    let fixup = Fixup::FixHairlines { min_width_pt: 0.25 };
    let r = s.apply_fixup(&fixup, &scope, &FixupTools::new());
    assert_eq!(r.steps[0].outcome, FixupOutcome::Unavailable);
    assert_eq!(r.steps[0].rule_ids, vec!["absent".to_owned()]);
    assert!(r.steps[0].not_done[0].contains("no violation"));
    assert_eq!(r.undo_entries, 0);
    assert_eq!(s.undo_depth(), depth);
}

#[test]
fn missing_engines_and_profiles_are_reported_unavailable() {
    let mut s = session(two_pages());
    let tools = FixupTools::new();
    for fixup in [
        Fixup::FlattenTransparency(FlattenOptions::default()),
        Fixup::suggested(&Check::ColourSpaceUsed {
            families: vec![crate::preflight::ColourFamily::DeviceRgb],
        })
        .unwrap(),
        Fixup::suggested(&Check::PdfxRequirement {
            level: crate::preflight::PdfxLevel::X4,
            row: "PXQ-AN-1",
        })
        .unwrap(),
    ] {
        let r = s.apply_fixup(&fixup, &FixupScope::All, &tools);
        assert_eq!(r.steps[0].outcome, FixupOutcome::Unavailable, "{fixup:?}");
        assert!(!r.steps[0].not_done.is_empty(), "{fixup:?}");
        assert_eq!(r.undo_entries, 0);
    }
}

#[test]
fn a_spot_conversion_with_no_spot_is_nothing_to_do() {
    let mut s = session(two_pages());
    let fixup = Fixup::SpotToProcess { keep: Vec::new() };
    let r = s.apply_fixup(&fixup, &FixupScope::All, &FixupTools::new());
    assert_eq!(r.steps[0].outcome, FixupOutcome::NothingToDo);
    assert_eq!(r.undo_entries, 0);
}

#[test]
fn image_above_ppi_finds_the_image_and_the_downsample_clears_it() {
    let mut s = session(image_doc());
    let rule = ProfileRule::new(
        "too-fine",
        Check::ImageAbovePpi { max_ppi: 150.0 },
        Severity::Warning,
    );
    let p = profile(vec![rule]);
    let before = preflight::run(&s.view(), &p);
    assert_eq!(before.findings.len(), 1);
    assert!(
        before.findings[0]
            .message
            .contains("216 x 216 ppi, above 150")
    );

    let fixup = Fixup::suggested(&p.rules[0].check).unwrap();
    assert_eq!(
        fixup,
        Fixup::DownsampleImages {
            above_ppi: 150,
            to_ppi: 150,
            method: ResampleMethod::Bicubic
        }
    );
    let r = s.apply_fixup(
        &fixup,
        &FixupScope::Finding(&before.findings[0]),
        &FixupTools::new(),
    );
    assert_eq!(r.steps[0].outcome, FixupOutcome::Applied, "{}", r.to_text());
    assert!(
        r.steps[0].changed[0].contains("from 300 x 300 to"),
        "{}",
        r.to_text()
    );
    assert_eq!(r.undo_entries, 1);
    assert!(preflight::run(&s.view(), &p).findings.is_empty());
    assert_eq!(s.undo(), Some(CommandKind::ApplyFixup));
    assert_eq!(preflight::run(&s.view(), &p).findings.len(), 1);
}

#[test]
fn image_below_ppi_and_a_high_ceiling_leave_the_image_alone() {
    let mut s = session(image_doc());
    let below = profile(vec![ProfileRule::new(
        "too-coarse",
        Check::ImageBelowPpi { min_ppi: 150.0 },
        Severity::Error,
    )]);
    assert!(preflight::run(&s.view(), &below).findings.is_empty());
    let fixup = Fixup::DownsampleImages {
        above_ppi: 300,
        to_ppi: 300,
        method: ResampleMethod::Bicubic,
    };
    let r = s.apply_fixup(&fixup, &FixupScope::All, &FixupTools::new());
    assert_eq!(
        r.steps[0].outcome,
        FixupOutcome::NothingToDo,
        "{}",
        r.to_text()
    );
    assert_eq!(r.undo_entries, 0);
}

#[test]
fn a_profile_run_is_one_undo_step_and_names_unfixed_rules() {
    let mut s = session(two_pages());
    let no_fix = ProfileRule::new("pages", Check::NoTrimBox, Severity::Error);
    let p = profile(vec![hairline_rule(), no_fix]);
    let report = preflight::run(&s.view(), &p);
    let depth = s.undo_depth();
    let r = s.apply_profile_fixups(&p, &report, &FixupTools::new(), true);
    assert_eq!(r.steps.len(), 2, "{}", r.to_text());
    assert_eq!(r.steps[0].fixup, "fix-hairlines");
    assert_eq!(r.steps[0].rule_ids, vec!["hairline".to_owned()]);
    assert_eq!(r.steps[0].count, 2);
    assert_eq!(r.steps[1].fixup, "none");
    assert_eq!(r.steps[1].outcome, FixupOutcome::Unavailable);
    assert!(r.steps[1].not_done[0].contains("rule pages"));
    assert_eq!(r.not_fixed().count(), 1);
    assert_eq!(s.undo_depth(), depth + 1);
    assert_eq!(s.undo(), Some(CommandKind::ApplyFixup));
    assert_eq!(hairlines_on(&mut s, 1), 1);

    let unsuggested = s.apply_profile_fixups(&p, &report, &FixupTools::new(), false);
    assert!(unsuggested.steps.iter().all(|st| st.fixup == "none"));
    assert_eq!(unsuggested.undo_entries, 0);
}

#[test]
fn a_fixup_two_rules_name_runs_once_for_both() {
    let mut s = session(two_pages());
    let fixup = Fixup::FixHairlines { min_width_pt: 0.25 };
    let mut thin = ProfileRule::new(
        "thin",
        Check::StrokeBelow { min_width_pt: 0.2 },
        Severity::Error,
    );
    thin.fixups.push(fixup.clone());
    let mut hair = hairline_rule();
    hair.fixups.push(fixup);
    let p = profile(vec![thin, hair]);
    let report = preflight::run(&s.view(), &p);
    let r = s.apply_profile_fixups(&p, &report, &FixupTools::new(), false);
    assert_eq!(r.steps.len(), 1);
    assert_eq!(
        r.steps[0].rule_ids,
        vec!["thin".to_owned(), "hairline".to_owned()]
    );
}

#[test]
fn one_pdfua_part_runs_alone_and_is_one_undo_step() {
    let mut s = session(two_pages());
    let fixup = Fixup::FixPdfua {
        part: PdfuaPart::Ua1,
        options: FixPdfuaOptions::default().with_lang("en-CA"),
        step: Some(PdfuaFixup::Language),
    };
    let r = s.apply_fixup(&fixup, &FixupScope::All, &FixupTools::new());
    assert_eq!(r.steps[0].outcome, FixupOutcome::Applied, "{}", r.to_text());
    assert!(r.steps[0].changed[0].starts_with("language: "));
    assert_eq!(r.undo_entries, 1);
    assert_eq!(s.undo(), Some(CommandKind::ApplyFixup));
}

#[test]
fn report_text_groups_findings_by_rule_with_the_fixup() {
    let s = session(two_pages());
    let p = profile(vec![
        hairline_rule(),
        ProfileRule::new("pages", Check::Encrypted, Severity::Error),
    ]);
    let report = preflight::run(&s.view(), &p);
    let text = preflight::report_text(&report, &p);
    assert!(
        text.starts_with("Preflight report: test\nA test profile.\n"),
        "{text}"
    );
    assert!(text.contains("Pages checked: 2 of 2\n"), "{text}");
    assert!(text.contains("Result: FAILS\n"), "{text}");
    assert!(
        text.contains("Rule hairline (error): 2 finding(s)\n"),
        "{text}"
    );
    assert!(
        text.contains("  suggested fixup: fix-hairlines\n"),
        "{text}"
    );
    assert!(text.contains("  page 2, object 0: "), "{text}");
    assert!(text.ends_with("\nNo findings: pages\n"), "{text}");
    assert_eq!(text, preflight::report_text(&report, &p));
}

#[test]
fn report_json_carries_the_text_reports_verdict_and_findings() {
    use crate::ocr::json_lite::{Json, parse};
    let s = session(two_pages());
    let p = profile(vec![
        hairline_rule(),
        ProfileRule::new("pages", Check::Encrypted, Severity::Error),
    ]);
    let report = preflight::run(&s.view(), &p);
    let text = preflight::report_text(&report, &p);
    let json = parse(preflight::report_json(&report, &p).as_bytes()).expect("valid JSON");
    let str_of = |j: &Json, k: &str| j.get(k).and_then(Json::as_str).map(str::to_owned);
    assert_eq!(str_of(&json, "profile").as_deref(), Some("test"));
    assert_eq!(str_of(&json, "verdict").as_deref(), Some("fails"));
    assert!(text.contains("Result: FAILS\n"));
    assert_eq!(json.get("pages_checked").and_then(Json::as_f64), Some(2.0));
    assert_eq!(json.get("cancelled").and_then(Json::as_bool), Some(false));
    let rules = json.get("rules").and_then(Json::as_array).expect("rules");
    assert_eq!(rules.len(), 2);
    assert_eq!(str_of(&rules[0], "id").as_deref(), Some("hairline"));
    assert_eq!(rules[0].get("findings").and_then(Json::as_f64), Some(2.0));
    assert_eq!(
        str_of(&rules[0], "suggested_fixup").as_deref(),
        Some("fix-hairlines")
    );
    assert!(rules[1].get("suggested_fixup").is_some_and(Json::is_null));
    let findings = json
        .get("findings")
        .and_then(Json::as_array)
        .expect("findings");
    assert_eq!(findings.len(), report.findings.len());
    for f in findings {
        let line = str_of(f, "text").expect("text");
        assert!(text.contains(&format!("  {line}\n")), "{line}");
        assert_eq!(str_of(f, "kind").as_deref(), Some("violation"));
    }
    let target = findings[findings.len() - 1].get("target").expect("target");
    assert_eq!(str_of(target, "kind").as_deref(), Some("object"));

    let clean = profile(vec![ProfileRule::new(
        "pages",
        Check::Encrypted,
        Severity::Error,
    )]);
    let report = preflight::run(&s.view(), &clean);
    let json = parse(preflight::report_json(&report, &clean).as_bytes()).expect("valid JSON");
    assert_eq!(str_of(&json, "verdict").as_deref(), Some("passes"));
    assert!(preflight::report_text(&report, &clean).contains("Result: passes\n"));
}

#[test]
fn a_resample_that_would_grow_the_stream_is_kept_and_listed() {
    // A short repeating ramp compresses to almost nothing; resampling breaks
    // the repeat.
    let px: Vec<u8> = (0..300u32 * 300).map(|i| (i * 7 % 251) as u8).collect();
    let mut s = session(image_doc_from(&px));
    let fixup = Fixup::DownsampleImages {
        above_ppi: 150,
        to_ppi: 150,
        method: ResampleMethod::Bicubic,
    };
    let r = s.apply_fixup(&fixup, &FixupScope::All, &FixupTools::new());
    let text = r.to_text();
    assert_eq!(r.steps[0].count, 0, "{text}");
    assert!(r.steps[0].not_done[0].contains("not smaller"), "{text}");
    assert_eq!(r.not_fixed().count(), 1, "{text}");
    assert_eq!(r.undo_entries, 0);
}
