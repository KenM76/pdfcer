//! `G085` — an underline or strikethrough is tied to its text: the rule is
//! recomputed from the decorated glyphs after every edit to the page.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use pdfcer_core::content::ContentStream;
use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::text_edit::decoration::{DecorationSet, StrikeSource, page_decorations};
use pdfcer_core::text_edit::{FormatError, FormatOptions, FormatRequest};
use pdfcer_core::text_extract::{ExtractOptions, extract_page};
use pdfcer_core::writer::SaveOptions;

const TWO_RUNS: &str = "BT /F1 12 Tf 1 0 0 1 72 700 Tm (Hello) Tj 1 0 0 1 200 700 Tm (World) Tj ET";

fn pdf(content: &str) -> Vec<u8> {
    pdf_with("<< /Type /Catalog /Pages 2 0 R >>", content)
}

fn pdf_with(catalog: &str, content: &str) -> Vec<u8> {
    pdf_streams(catalog, &[content])
}

/// A one-page document whose `/Contents` is one stream per entry.
fn pdf_streams(catalog: &str, streams: &[&str]) -> Vec<u8> {
    let widths = (0..95).map(|_| "500").collect::<Vec<_>>().join(" ");
    let refs: Vec<String> = (0..streams.len())
        .map(|i| format!("{} 0 R", i + 5))
        .collect();
    let mut objects = vec![
        catalog.to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents [{}] \
             /Resources << /Font << /F1 4 0 R >> >> >>",
            refs.join(" ")
        ),
        format!(
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica \
             /Encoding /WinAnsiEncoding /FirstChar 32 /LastChar 126 /Widths [{widths}] >>"
        ),
    ];
    objects.extend(
        streams
            .iter()
            .map(|c| format!("<< /Length {} >>\nstream\n{c}\nendstream", c.len())),
    );
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    out.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for off in offsets {
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

fn session(content: &str) -> EditSession {
    EditSession::new(Document::from_bytes(pdf(content)).unwrap())
}

fn decorate(s: &mut EditSession, find: &str, set: DecorationSet) -> Result<(), FormatError> {
    let req = FormatRequest::new(0, find).decoration(set);
    s.format_text(&req, &FormatOptions::default()).map(|_| ())
}

fn saved(s: &EditSession) -> Document {
    Document::from_bytes(s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0).unwrap()
}

fn content(doc: &Document) -> String {
    let pages = pdfcer_core::page_tree::pages(doc).unwrap();
    let cs = ContentStream::from_page(&doc.view(), &pages[0]).unwrap();
    String::from_utf8(cs.buf).unwrap()
}

/// Every rule block as `(cm matrix, re operands)`.
fn rules(text: &str) -> Vec<([f64; 6], [f64; 4])> {
    let nums = |s: &str| -> Vec<f64> { s.split_whitespace().map(|n| n.parse().unwrap()).collect() };
    text.match_indices("<</Rule")
        .map(|(at, _)| {
            let block = &text[at..];
            let q = block.find(" q ").unwrap() + 3;
            let cm = block.find(" cm").unwrap();
            let m = nums(&block[q..cm]);
            let re = block.find(" re ").unwrap();
            let before_re = &block[..re];
            let parts: Vec<&str> = before_re.split_whitespace().collect();
            let r = nums(&parts[parts.len() - 4..].join(" "));
            (
                [m[0], m[1], m[2], m[3], m[4], m[5]],
                [r[0], r[1], r[2], r[3]],
            )
        })
        .collect()
}

#[test]
fn an_underline_is_drawn_under_the_word() {
    let mut s = session(TWO_RUNS);
    decorate(&mut s, "World", DecorationSet::UNDERLINE).unwrap();
    let text = content(&saved(&s));
    let r = rules(&text);
    assert_eq!(r.len(), 1, "{text}");
    let (m, re) = r[0];
    assert_eq!([m[4], m[5]], [200.0, 700.0], "{text}");
    // 5 glyphs × 500/1000 em × 12 pt; centre −0.1 em, thickness 0.05 em.
    assert!(
        (re[0]).abs() < 1e-6 && (re[2] - 30.0).abs() < 1e-6,
        "{re:?}"
    );
    assert!(
        (re[1] + 1.5).abs() < 1e-6 && (re[3] - 0.6).abs() < 1e-6,
        "{re:?}"
    );
}

#[test]
fn moving_the_run_moves_its_rule() {
    let mut s = session(TWO_RUNS);
    decorate(&mut s, "World", DecorationSet::UNDERLINE).unwrap();
    s.move_text_run(0, 0, 1, 20.0, 0.0).unwrap();
    let text = content(&saved(&s));
    let r = rules(&text);
    assert_eq!(r.len(), 1, "{text}");
    assert_eq!([r[0].0[4], r[0].0[5]], [220.0, 700.0], "{text}");
}

#[test]
fn deleting_the_run_removes_its_rule() {
    let mut s = session(TWO_RUNS);
    decorate(&mut s, "World", DecorationSet::UNDERLINE).unwrap();
    s.delete_text_run(0, 0, 1).unwrap();
    let text = content(&saved(&s));
    assert!(!text.contains("pdfc_Deco"), "{text}");
    assert!(!text.contains(" re f"), "{text}");
}

#[test]
fn the_read_reports_decorated_glyphs_only() {
    let mut s = session(TWO_RUNS);
    decorate(&mut s, "orl", DecorationSet::STRIKETHROUGH).unwrap();
    let doc = saved(&s);
    let pages = pdfcer_core::page_tree::pages(&doc).unwrap();
    let decorations = page_decorations(&doc.view(), &pages[0]).unwrap();
    let opts = ExtractOptions::default().with_provenance(true);
    let page = extract_page(&doc, &pages[0], 0, &opts).unwrap();
    let mut seen = String::new();
    for run in &page.runs {
        for g in &run.glyphs {
            let ch = run.text[g.text_start as usize..].chars().next().unwrap();
            let set = decorations.of(g.provenance.as_ref().unwrap());
            seen.push(if set.strikethrough {
                ch.to_ascii_uppercase()
            } else {
                ch.to_ascii_lowercase()
            });
        }
    }
    assert_eq!(seen.replace(' ', ""), "hellowORLd");
}

#[test]
fn a_marker_over_two_lines_draws_two_rules() {
    let two_lines = "BT /F1 12 Tf 14 TL 1 0 0 1 72 700 Tm \
                     /pdfc_Deco <</Line /Underline /Id 1>> BDC (ab) Tj T* (cd) Tj EMC ET \
                     BT /F1 12 Tf 1 0 0 1 72 600 Tm (Other) Tj ET";
    let mut s = session(two_lines);
    let req = FormatRequest::new(0, "Other").size(14.0);
    s.format_text(&req, &FormatOptions::default()).unwrap();
    let text = content(&saved(&s));
    let r = rules(&text);
    assert_eq!(r.len(), 2, "{text}");
    assert_eq!(r[0].0[5], 700.0, "{text}");
    assert_eq!(r[1].0[5], 686.0, "{text}");
}

#[test]
fn clearing_part_of_a_decoration_splits_it() {
    let mut s = session(TWO_RUNS);
    decorate(&mut s, "World", DecorationSet::UNDERLINE).unwrap();
    let req = FormatRequest::new(0, "rl").decoration(DecorationSet::NONE);
    s.format_text(&req, &FormatOptions::default()).unwrap();
    let doc = saved(&s);
    let text = content(&doc);
    assert_eq!(rules(&text).len(), 2, "{text}");
    let pages = pdfcer_core::page_tree::pages(&doc).unwrap();
    let decorations = page_decorations(&doc.view(), &pages[0]).unwrap();
    let ids: Vec<u32> = decorations.spans.iter().map(|d| d.id).collect();
    assert_eq!(ids.len(), 2, "{text}");
    assert_ne!(ids[0], ids[1], "{text}");
}

#[test]
fn invisible_text_cannot_be_decorated() {
    let mut s = session("BT /F1 12 Tf 3 Tr 72 700 Td (Hidden) Tj ET");
    let err = decorate(&mut s, "Hidden", DecorationSet::UNDERLINE).unwrap_err();
    assert!(
        matches!(err, FormatError::DecorationOnInvisibleText { mode: 3 }),
        "{err:?}"
    );
}

#[test]
fn refresh_is_idempotent_across_unrelated_edits() {
    let mut s = session(TWO_RUNS);
    decorate(&mut s, "World", DecorationSet::UNDERLINE).unwrap();
    let first = content(&saved(&s));
    let req = FormatRequest::new(0, "Hello").size(12.0);
    s.format_text(&req, &FormatOptions::default()).unwrap();
    let second = content(&saved(&s));
    assert_eq!(rules(&first), rules(&second));
    assert_eq!(second.matches("<</Rule").count(), 1, "{second}");
}

#[test]
fn an_unembedded_standard_font_strikes_at_half_its_afm_x_height() {
    let mut s = session(TWO_RUNS);
    let req = FormatRequest::new(0, "World").decoration(DecorationSet::STRIKETHROUGH);
    let report = s.format_text(&req, &FormatOptions::default()).unwrap();
    assert_eq!(report.strike_source, Some(StrikeSource::XHeight));
    assert!(
        report
            .disclosures
            .iter()
            .any(|d| d.contains("inferred at half the font's x-height")),
        "{:?}",
        report.disclosures
    );
    let text = content(&saved(&s));
    let r = rules(&text);
    assert_eq!(r.len(), 1, "{text}");
    // Helvetica XHeight 523: centre 261.5, thickness 50, at 12 pt.
    let re = r[0].1;
    assert!(
        (re[1] - 2.838).abs() < 1e-6 && (re[3] - 0.6).abs() < 1e-6,
        "{re:?}"
    );
}

const MARKED: &str = "<< /Type /Catalog /Pages 2 0 R /MarkInfo << /Marked true >> >>";

fn underlined(catalog: &str, page: &str) -> String {
    let mut s = EditSession::new(Document::from_bytes(pdf_with(catalog, page)).unwrap());
    decorate(&mut s, "Hello", DecorationSet::UNDERLINE).unwrap();
    let req = FormatRequest::new(0, "World").size(12.0);
    s.format_text(&req, &FormatOptions::default()).unwrap();
    content(&saved(&s))
}

#[test]
fn a_tagged_page_rule_outside_any_tag_is_an_artifact() {
    let text = underlined(
        MARKED,
        "BT /P <</MCID 0>> BDC /F1 12 Tf 1 0 0 1 72 700 Tm (Hello) Tj \
         1 0 0 1 200 700 Tm (World) Tj EMC ET",
    );
    assert_eq!(text.matches("<</Rule").count(), 1, "{text}");
    assert_eq!(
        text.matches("/Artifact <</Type /Layout>> BDC").count(),
        1,
        "{text}"
    );
    assert!(text.contains("re f Q EMC EMC"), "{text}");
}

#[test]
fn a_rule_inside_a_tagged_sequence_stays_its_content() {
    let text = underlined(
        MARKED,
        "/P <</MCID 0>> BDC BT /F1 12 Tf 1 0 0 1 72 700 Tm (Hello) Tj \
         1 0 0 1 200 700 Tm (World) Tj ET EMC",
    );
    assert_eq!(text.matches("<</Rule").count(), 1, "{text}");
    assert!(!text.contains("/Artifact"), "{text}");
}

#[test]
fn an_untagged_document_gets_no_artifact() {
    let text = underlined(
        "<< /Type /Catalog /Pages 2 0 R >>",
        "BT /P <</MCID 0>> BDC /F1 12 Tf 1 0 0 1 72 700 Tm (Hello) Tj \
         1 0 0 1 200 700 Tm (World) Tj EMC ET",
    );
    assert_eq!(text.matches("<</Rule").count(), 1, "{text}");
    assert!(!text.contains("/Artifact"), "{text}");
}

#[test]
fn a_page_split_across_streams_keeps_its_rule_on_the_text() {
    let doc = pdf_streams(
        "<< /Type /Catalog /Pages 2 0 R >>",
        &[
            "BT /F1 12 Tf 1 0 0 1 72 700 Tm (Hello) Tj ET",
            "BT /F1 12 Tf 1 0 0 1 200 700 Tm (World) Tj ET",
        ],
    );
    let mut s = EditSession::new(Document::from_bytes(doc).unwrap());
    decorate(&mut s, "World", DecorationSet::UNDERLINE).unwrap();
    s.move_text_run(0, 1, 0, 20.0, 0.0).unwrap();
    let text = content(&saved(&s));
    let r = rules(&text);
    assert_eq!(r.len(), 1, "{text}");
    assert_eq!([r[0].0[4], r[0].0[5]], [220.0, 700.0], "{text}");
}

#[test]
fn a_transform_before_the_text_is_not_applied_twice() {
    // The page's own `cm` is still in force after `ET`, so the rule carries
    // `Tm` alone; carrying `Tm` x CTM would land it 10,20 off its text.
    let mut s = session(
        "1 0 0 1 10 20 cm BT /F1 12 Tf 1 0 0 1 72 700 Tm (Hello) Tj          1 0 0 1 200 700 Tm (World) Tj ET",
    );
    decorate(&mut s, "World", DecorationSet::UNDERLINE).unwrap();
    let text = content(&saved(&s));
    let r = rules(&text);
    assert_eq!(r.len(), 1, "{text}");
    assert_eq!(r[0].0, [1.0, 0.0, 0.0, 1.0, 200.0, 700.0], "{text}");
}

#[test]
fn a_flipped_page_keeps_its_rule_under_the_text() {
    let mut s = session(
        "1 0 0 -1 0 792 cm BT /F1 12 Tf 1 0 0 -1 72 92 Tm (Hello) Tj          1 0 0 -1 200 92 Tm (World) Tj ET",
    );
    decorate(&mut s, "World", DecorationSet::UNDERLINE).unwrap();
    let text = content(&saved(&s));
    let r = rules(&text);
    assert_eq!(r.len(), 1, "{text}");
    assert_eq!(r[0].0, [1.0, 0.0, 0.0, -1.0, 200.0, 92.0], "{text}");
    assert!((r[0].1[1] + 1.5).abs() < 1e-6, "{text}");
}
