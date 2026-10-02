//! `EditOptions::fallback` (Pass 431.0): characters the run's font cannot
//! encode are set in a fallback face, the rest stay in the run's font.
//! `fallback-font.pdf`: `/F0` is a WinAnsi TrueType subset showing "Qu5" that
//! carries nothing else; `/F1` is Helvetica showing "Hi".

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::page_tree;
use pdfcer_core::text_edit::{
    self, EditOptions, EditReport, EditRequest, FallbackFace, FallbackSource, FollowerDisposition,
};
use pdfcer_render::font::subset::plan_subset;
use pdfcer_render::{RenderOptions, render_page_with};

const TYPED: &str = "Qu\u{20AC} \u{2265} 5";

fn fixture(name: &str) -> Vec<u8> {
    let path = format!(
        "{}/../../fixtures/synthetic/text/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read(path).expect("run tools/gen-fallback-font-fixture.py")
}

fn base() -> Document {
    Document::from_bytes(fixture("fallback-font.pdf")).unwrap()
}

fn leak(face: FallbackFace) -> EditOptions {
    EditOptions::default().with_fallback(Box::leak(Box::new(face)))
}

fn donor_opts(text: &str) -> EditOptions {
    let mut chars: Vec<char> = text.chars().collect();
    chars.sort_unstable();
    chars.dedup();
    let plan = plan_subset(
        &fixture("fallback-donor.ttf"),
        0,
        &chars,
        "pdfcerFbDonor",
        "FBDAAA",
    )
    .expect("the donor covers the text");
    leak(FallbackFace::Embedded(Box::new(plan)))
}

fn named(name: &str) -> EditOptions {
    leak(FallbackFace::Named(name.to_owned()))
}

fn edit(replace: &str, opts: &EditOptions) -> Result<text_edit::EditOutcome, String> {
    text_edit::edit_text(&base(), &EditRequest::find_replace(0, "Qu5", replace), opts)
        .map_err(|e| e.to_string())
}

fn page_text(bytes: &[u8]) -> String {
    let doc = Document::from_bytes(bytes.to_vec()).unwrap();
    let pages = page_tree::pages(&doc).unwrap();
    pdfcer_core::text_extract::extract_page(&doc, &pages[0], 0, &Default::default())
        .unwrap()
        .sourced_text()
}

/// The page's content stream (object 4), newest revision, whitespace-normalised.
fn content(bytes: &[u8]) -> String {
    let s = String::from_utf8_lossy(bytes);
    let at = s.rfind("\n4 0 obj").expect("content object");
    let end = at + s[at..].find("endobj").unwrap();
    s[at..end].split_whitespace().collect::<Vec<_>>().join(" ")
}

#[test]
fn without_the_option_the_run_refuses() {
    let err = edit(TYPED, &EditOptions::default()).unwrap_err();
    assert!(err.contains("FBKAAA+pdfcerFbRun"), "{err}");
}

#[test]
fn the_typed_text_commits_with_an_embedded_fallback() {
    let out = edit(TYPED, &donor_opts(TYPED)).unwrap();
    let text = page_text(&out.bytes);
    assert!(
        text.contains(TYPED),
        "extraction returns what was typed: {text}"
    );
    let c = content(&out.bytes);
    assert!(
        c.starts_with("4 0 obj") && c.contains("/F0 24 Tf 72 600 Td (Qu) Tj /pdfceF1 24 Tf"),
        "Q and u stay under /F0 in their own codes: {c}"
    );
    assert!(
        c.contains("/F0 24 Tf (5) Tj ET"),
        "the run's font is restored for 5: {c}"
    );
    let appended = String::from_utf8_lossy(&out.bytes[base().bytes().len()..]);
    assert!(appended.contains("/Type0") && appended.contains("/Identity-H"));
}

#[test]
fn the_report_names_each_fallback_character_and_the_face() {
    let out = edit(TYPED, &donor_opts(TYPED)).unwrap();
    let used = out.report.fallback.expect("the fallback was used");
    assert_eq!(used.characters, vec!['\u{20AC}', ' ', '\u{2265}']);
    assert_eq!(used.source, FallbackSource::EmbeddedSubset);
    assert_eq!(used.font_resource, b"pdfceF1");
    assert!(
        used.base_font.ends_with("+pdfcerFbDonor"),
        "{}",
        used.base_font
    );
    let note = out
        .report
        .disclosures
        .iter()
        .find(|d| d.starts_with("fallback:"))
        .expect("a fallback disclosure");
    for needle in ["U+20AC", "U+0020", "U+2265", "pdfcerFbDonor", "/pdfceF1"] {
        assert!(note.contains(needle), "{needle} missing from {note}");
    }
}

#[test]
fn a_replacement_the_run_can_take_ignores_the_fallback() {
    let out = edit("5uQ", &donor_opts(TYPED)).unwrap();
    assert!(out.report.fallback.is_none());
    assert!(!content(&out.bytes).contains("pdfceF1"));
}

#[test]
fn a_named_page_font_is_reused_without_a_new_resource() {
    let out = edit("Qu\u{20AC} 5", &named("Helvetica")).unwrap();
    let used = out.report.fallback.unwrap();
    assert_eq!(used.font_resource, b"F1");
    assert_eq!(used.source, FallbackSource::PageResource);
    assert!(content(&out.bytes).contains("(Qu) Tj /F1 24 Tf (\\200 ) Tj /F0 24 Tf (5) Tj"));
    let appended = String::from_utf8_lossy(&out.bytes[base().bytes().len()..]);
    assert!(
        !appended.contains("/Type /Font"),
        "no font object is written"
    );
    assert!(page_text(&out.bytes).contains("Qu\u{20AC} 5"));
}

#[test]
fn a_fallback_that_cannot_set_a_character_refuses_naming_it() {
    let err = edit(TYPED, &named("Helvetica")).unwrap_err();
    assert!(err.contains("U+2265"), "{err}");
}

#[test]
fn a_standard_14_name_adds_a_resource() {
    let out = edit("Qu\u{20AC}5", &named("Times-Roman")).unwrap();
    let used = out.report.fallback.unwrap();
    assert_eq!(used.source, FallbackSource::AddedStandard14);
    assert_eq!(used.font_resource, b"pdfceF1");
    let appended = String::from_utf8_lossy(&out.bytes[base().bytes().len()..]);
    assert!(appended.contains("/Times-Roman") && appended.contains("/pdfceF1"));
    assert!(page_text(&out.bytes).contains("Qu\u{20AC}5"));
}

#[test]
fn a_name_that_is_no_font_refuses() {
    let err = edit(TYPED, &named("NoSuchFace")).unwrap_err();
    assert!(err.contains("NoSuchFace"), "{err}");
}

#[test]
fn pin_takes_back_the_fallback_advance() {
    let opts = named("Helvetica").with_disposition(FollowerDisposition::Pin);
    let out = edit("Qu\u{20AC}5", &opts).unwrap();
    // The run grew by Helvetica's Euro (556), so the trailing TJ takes 556 back.
    assert!(
        content(&out.bytes).contains("/F0 24 Tf [(5) 556] TJ ET"),
        "{}",
        content(&out.bytes)
    );
}

#[test]
fn the_session_commits_one_undo_entry() {
    let mut session = EditSession::new(base());
    let req = EditRequest::find_replace(0, "Qu5", TYPED);
    let report = session.edit_text(&req, &donor_opts(TYPED)).unwrap();
    assert!(report.fallback.is_some());
    assert_eq!(session.undo_depth(), 1);
    let (bytes, _) = session
        .to_incremental_bytes(&pdfcer_core::writer::SaveOptions::identity())
        .unwrap();
    assert!(page_text(&bytes).contains(TYPED));
    session.undo();
    assert!(!session.can_undo());
}

#[test]
fn the_preview_agrees_with_the_commit_glyph_for_glyph() {
    use pdfcer_render::edit_preview::preview_outlines;
    let opts = donor_opts(TYPED);
    let req = EditRequest::find_replace(0, "Qu5", TYPED);
    let mut session = EditSession::new(base());
    let preview = session.edit_text_preview(&req, &opts).unwrap();
    let flags: String = preview
        .glyphs
        .iter()
        .map(|g| if g.fallback { 'f' } else { 'o' })
        .collect();
    assert_eq!(flags, "ooffffo");
    let face = preview
        .fallback
        .as_ref()
        .expect("the preview names the face");
    assert_eq!(face.font_resource, b"pdfceF1");
    let outlines = preview_outlines(
        &session.view(),
        &preview,
        &pdfcer_render::FontEnvironment::bundled(),
    );
    let drawn: String = outlines
        .glyphs
        .iter()
        .map(|g| if g.is_some() { '#' } else { '.' })
        .collect();
    assert_eq!(
        drawn, "###.#.#",
        "every glyph but the spaces has an outline"
    );

    session.edit_text(&req, &opts).unwrap();
    let pages = session.pages().unwrap();
    let x_opts = pdfcer_core::text_extract::ExtractOptions::default();
    let page = pdfcer_core::text_extract::extract_page_view(&session.view(), &pages[0], 0, &x_opts)
        .unwrap();
    let committed: Vec<(u32, f32)> = page
        .runs
        .iter()
        .flat_map(|r| r.glyphs.iter())
        .filter(|g| (g.y - 600.0).abs() < 1.0)
        .map(|g| (g.code, g.x))
        .collect();
    let previewed: Vec<(u32, f32)> = preview
        .glyphs
        .iter()
        .map(|g| (g.code, g.matrix[4] as f32))
        .collect();
    assert_eq!(
        committed.len(),
        previewed.len(),
        "{committed:?} vs {previewed:?}"
    );
    for (c, p) in committed.iter().zip(&previewed) {
        assert_eq!(c.0, p.0, "{committed:?} vs {previewed:?}");
        assert!((c.1 - p.1).abs() < 1e-2, "{committed:?} vs {previewed:?}");
    }
}

#[test]
fn the_repertoire_reports_what_the_fallback_accepts() {
    let session = EditSession::new(base());
    let strict = session.run_repertoire(0, "Qu5", None).unwrap();
    assert!(strict.via_fallback.is_empty());
    assert!(!strict.accepts('\u{20AC}'));
    let wide = session
        .run_repertoire_with(0, "Qu5", None, &donor_opts(TYPED))
        .unwrap();
    let via: String = wide.via_fallback.iter().collect();
    assert_eq!(via, " \u{20AC}\u{2265}");
    assert!(wide.accepts('\u{2265}') && wide.accepts('Q'));
}

fn variant(name: &str) -> Document {
    Document::from_bytes(fixture(name)).unwrap()
}

/// Edits `doc` through both shells' routes: the one-shot free function and
/// the session's incremental save. Returns the bytes and report of each.
fn both_routes(doc: &Document, replace: &str, opts: &EditOptions) -> Vec<(Vec<u8>, EditReport)> {
    let req = EditRequest::find_replace(0, "Qu5", replace);
    let one_shot = text_edit::edit_text(doc, &req, opts).unwrap();
    let mut session = EditSession::new(Document::from_bytes(doc.bytes().to_vec()).unwrap());
    let report = session.edit_text(&req, opts).unwrap();
    let (bytes, _) = session
        .to_incremental_bytes(&pdfcer_core::writer::SaveOptions::identity())
        .unwrap();
    vec![(one_shot.bytes, one_shot.report), (bytes, report)]
}

#[test]
fn a_run_in_a_form_xobject_takes_the_fallback_into_the_forms_resources() {
    let doc = variant("fallback-font-form.pdf");
    for (replace, opts) in [
        (TYPED, donor_opts(TYPED)),
        ("Qu\u{20AC}5", named("Times-Roman")),
    ] {
        for (bytes, report) in both_routes(&doc, replace, &opts) {
            assert!(page_text(&bytes).contains(replace));
            assert_eq!(report.fallback.unwrap().font_resource, b"pdfceF1");
            let appended = format!("\n{}", String::from_utf8_lossy(&bytes[doc.bytes().len()..]));
            let form = appended.find("\n9 0 obj").expect("the form is rewritten");
            let form = &appended[form..form + appended[form..].find("endobj").unwrap()];
            assert!(form.contains("/pdfceF1"), "{form}");
            assert!(!appended.contains("\n3 0 obj"), "the page is untouched");
        }
    }
}

#[test]
fn inherited_resources_take_the_fallback_and_say_so() {
    let doc = variant("fallback-font-inherited.pdf");
    for (bytes, report) in both_routes(&doc, TYPED, &donor_opts(TYPED)) {
        assert!(page_text(&bytes).contains(TYPED));
        assert!(
            report
                .disclosures
                .iter()
                .any(|d| d.contains("shared with other pages")),
            "{:?}",
            report.disclosures
        );
    }
}

#[test]
fn the_edited_page_renders() {
    let out = edit(TYPED, &donor_opts(TYPED)).unwrap();
    let doc = Document::from_bytes(out.bytes).unwrap();
    let pages = page_tree::pages(&doc).unwrap();
    assert!(render_page_with(&doc, &pages[0], 1.0, &RenderOptions::default()).is_ok());
}
