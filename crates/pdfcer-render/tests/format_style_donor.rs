//! Rung 3 of the automatic bold/italic ladder: a supplied face of the run's
//! own family, subset and embedded (`Pass 142.3`), plus the form-XObject route
//! of the one-shot donor embed (`Pass 142.0`).
//!
//! Lives in `pdfcer-render`'s tests because only this crate can both subset a
//! real donor and drive core's format surgery. The run is set in a
//! non-standard-14 family so rung 2 cannot fire first.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::font_embed::FontEmbedPlan;
use pdfcer_core::settings::StylePolicy;
use pdfcer_core::text_edit::{FormatOptions, FormatRequest, StyleRung, StyleSynthesis};
use pdfcer_core::writer::SaveOptions;
use pdfcer_render::font::subset::plan_subset;

/// The synthetic donor carrying outlines for exactly `A`, `B`, `C`.
fn donor_bytes() -> Vec<u8> {
    std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/synthetic/text/subset-donor.ttf"
    ))
    .expect("donor fixture; run tools/gen-subset-font-fixtures.py")
}

/// A subset of the donor advertised as `name`.
fn donor(name: &str, chars: &[char]) -> FontEmbedPlan {
    plan_subset(&donor_bytes(), 0, chars, name, "ABCDEF").expect("donor covers A-C")
}

/// A one-page PDF from numbered objects, with a correct xref.
fn pdf(objects: &[(u32, String)]) -> Vec<u8> {
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offsets = vec![0usize; objects.len() + 1];
    for (n, body) in objects {
        offsets[*n as usize] = out.len();
        out.extend_from_slice(format!("{n} 0 obj\n{body}\nendobj\n").as_bytes());
    }
    let xref = out.len();
    out.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for off in &offsets[1..] {
        out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    out
}

fn simple_font(base_font: &str) -> String {
    let widths = (0..95).map(|_| "500").collect::<Vec<_>>().join(" ");
    format!(
        "<< /Type /Font /Subtype /Type1 /BaseFont /{base_font} /Encoding /WinAnsiEncoding \
         /FirstChar 32 /LastChar 126 /Widths [{widths}] >>"
    )
}

fn stream(dict: &str, content: &str) -> String {
    format!(
        "<< {dict} /Length {} >>\nstream\n{content}\nendstream",
        content.len()
    )
}

/// `CAB` set in `/F1` = `/{family}` directly on the page.
fn page_run(family: &str) -> EditSession {
    let objects = vec![
        (1, "<< /Type /Catalog /Pages 2 0 R >>".to_owned()),
        (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned()),
        (
            3,
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R \
             /Resources << /Font << /F1 5 0 R >> >> >>"
                .to_owned(),
        ),
        (4, stream("", "BT /F1 12 Tf 72 700 Td (CAB) Tj ET")),
        (5, simple_font(family)),
    ];
    EditSession::new(Document::from_bytes(pdf(&objects)).unwrap())
}

fn saved(s: &EditSession) -> Vec<u8> {
    s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0
}

fn page0_text(bytes: &[u8]) -> String {
    let doc = Document::from_bytes(bytes.to_vec()).unwrap();
    let pages = pdfcer_core::page_tree::pages(&doc).unwrap();
    pdfcer_core::text_extract::extract_page(&doc, &pages[0], 0, &Default::default())
        .unwrap()
        .sourced_text()
}

const ABC: [char; 3] = ['A', 'B', 'C'];

fn bold_with(donors: Vec<FontEmbedPlan>) -> FormatRequest {
    donors.into_iter().fold(
        FormatRequest::new(0, "CAB").style(StyleSynthesis::Bold),
        FormatRequest::style_donor,
    )
}

#[test]
fn rung_3_embeds_a_supplied_face_of_the_runs_family_as_one_undo_entry() {
    let mut s = page_run("DemoSans");
    let before = saved(&s);
    let req = bold_with(vec![
        donor("OtherFamily-Bold", &ABC),
        donor("DemoSans-Italic", &ABC),
        donor("DemoSans-Bold", &ABC),
    ]);
    let r = s.format_text(&req, &FormatOptions::default()).unwrap();
    let l = r.style_ladder.as_ref().expect("the ladder ran");
    assert_eq!(l.rung, StyleRung::SuppliedFaceEmbedded);
    assert!(
        l.bound
            .as_deref()
            .is_some_and(|b| b.ends_with("DemoSans-Bold")),
        "{:?}",
        l.bound
    );
    assert_eq!(l.same_family, Some(true));
    assert!(l.synthesised.is_none() && r.synthesis.is_none());
    assert!(
        r.disclosures
            .iter()
            .any(|d| d.starts_with("style: bold via rung 3: a supplied face, embedded")),
        "{:?}",
        r.disclosures
    );

    let out = saved(&s);
    let text = String::from_utf8_lossy(&out);
    assert!(text.contains("/FontFile2") && text.contains("+DemoSans-Bold"));
    assert!(!text.contains("2 Tr"), "no synthetic stroke");
    assert!(page0_text(&out).contains("CAB"), "{}", page0_text(&out));

    s.undo().expect("one undoable command");
    assert_eq!(saved(&s), before, "undo removes every embedded object");
}

#[test]
fn a_mt_suffixed_run_matches_its_mt_suffixed_bold_donor() {
    // `ArialMT` / `Arial-BoldMT` spell one family; a vendor suffix must not
    // hide the match. (Arial maps to no standard-14 face here: the run is set
    // in `DemoMT`, whose stem is not a standard-14 alias.)
    let mut s = page_run("DemoMT");
    let r = s
        .format_text(
            &bold_with(vec![donor("Demo-BoldMT", &ABC)]),
            &FormatOptions::default(),
        )
        .unwrap();
    assert_eq!(
        r.style_ladder.unwrap().rung,
        StyleRung::SuppliedFaceEmbedded
    );
}

#[test]
fn no_matching_donor_falls_through_to_synthesis() {
    for donors in [
        vec![],
        vec![donor("OtherFamily-Bold", &ABC)],
        // Claims an axis that was not asked for.
        vec![donor("DemoSans-BoldItalic", &ABC)],
    ] {
        let mut s = page_run("DemoSans");
        let r = s
            .format_text(&bold_with(donors.clone()), &FormatOptions::default())
            .unwrap();
        let l = r.style_ladder.unwrap();
        assert_eq!(
            l.rung,
            StyleRung::Synthetic,
            "{:?}",
            donors.iter().map(|d| &d.base_name).collect::<Vec<_>>()
        );
        assert!(!String::from_utf8_lossy(&saved(&s)).contains("/FontFile2"));
    }
}

#[test]
fn a_donor_that_cannot_show_the_run_is_passed_over_by_name() {
    let mut s = page_run("DemoSans");
    let r = s
        .format_text(
            &bold_with(vec![donor("DemoSans-Bold", &['A', 'B'])]),
            &FormatOptions::default(),
        )
        .unwrap();
    let l = r.style_ladder.unwrap();
    assert_eq!(l.rung, StyleRung::Synthetic);
    assert!(
        l.passed_over.iter().any(|p| p.base_font == "DemoSans-Bold"),
        "{:?}",
        l.passed_over
    );
}

#[test]
fn rung_3_binds_under_every_posture_because_it_fakes_nothing() {
    for policy in [StylePolicy::Auto, StylePolicy::Warn, StylePolicy::Refuse] {
        let mut s = page_run("DemoSans");
        let r = s
            .format_text(
                &bold_with(vec![donor("DemoSans-Bold", &ABC)]),
                &FormatOptions::default().with_style_policy(policy),
            )
            .unwrap();
        assert_eq!(
            r.style_ladder.unwrap().rung,
            StyleRung::SuppliedFaceEmbedded,
            "{policy:?}"
        );
        assert!(
            r.real_face_passed_over.is_none(),
            "{policy:?}: nothing to warn about"
        );
    }
}

#[test]
fn the_donor_preview_answers_what_the_commit_then_does() {
    let mut s = page_run("DemoSans");
    let opts = FormatOptions::default();
    let donors = vec![donor("DemoSans-Bold", &ABC)];
    let blind = s
        .preview_style_ladder(0, "CAB", None, StyleSynthesis::Bold, &opts)
        .unwrap();
    assert_eq!(blind.rung, StyleRung::Synthetic, "no donors, no rung 3");
    let preview = s
        .preview_style_ladder_with_donors(0, "CAB", None, StyleSynthesis::Bold, &opts, &donors)
        .unwrap();
    assert_eq!(preview.rung, StyleRung::SuppliedFaceEmbedded);
    let committed = s.format_text(&bold_with(donors), &opts).unwrap();
    let l = committed.style_ladder.unwrap();
    assert_eq!(preview.rung, l.rung);
    assert_eq!(preview.bound, l.bound);
}

#[test]
fn a_donor_embeds_into_a_form_xobjects_own_resources() {
    // `Pass 142.0` owed this: the run lives inside a form, so the new font
    // must be registered in the FORM's /Resources (§7.8.3), and the page's
    // /Font dictionary stays as it was.
    let objects = vec![
        (1, "<< /Type /Catalog /Pages 2 0 R >>".to_owned()),
        (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned()),
        (
            3,
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R \
             /Resources << /XObject << /Fm1 6 0 R >> >> >>"
                .to_owned(),
        ),
        (4, stream("", "q /Fm1 Do Q")),
        (5, simple_font("DemoSans")),
        (
            6,
            stream(
                "/Type /XObject /Subtype /Form /BBox [0 0 612 792] \
                 /Resources << /Font << /F1 5 0 R >> >>",
                "BT /F1 12 Tf 72 700 Td (CAB) Tj ET",
            ),
        ),
    ];
    let mut s = EditSession::new(Document::from_bytes(pdf(&objects)).unwrap());
    let before = saved(&s);
    let req = FormatRequest::new(0, "CAB").embedded_font(donor("DemoSans-Bold", &ABC));
    s.format_text(&req, &FormatOptions::default()).unwrap();

    let out = saved(&s);
    let doc = Document::from_bytes(out.clone()).unwrap();
    let pages = pdfcer_core::page_tree::pages(&doc).unwrap();
    assert!(
        pages[0].resources.get(b"Font").is_none(),
        "the page gained no /Font: the run is the form's"
    );
    let text = String::from_utf8_lossy(&out);
    assert!(text.contains("/FontFile2") && text.contains("+DemoSans-Bold"));
    assert!(page0_text(&out).contains("CAB"), "{}", page0_text(&out));

    s.undo().expect("one undoable command");
    assert_eq!(saved(&s), before);
}
