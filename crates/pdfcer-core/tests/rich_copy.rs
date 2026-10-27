//! Formatted copy: one paragraph per recognised block, styled runs, and the
//! HTML / CF_HTML / RTF a word processor pastes with its formatting.

use pdfcer_core::document::Document;
use pdfcer_core::page_tree;
use pdfcer_core::text_edit::decoration::page_decorations;
use pdfcer_core::text_edit::model::{PageFonts, RichText, RichTextSources};
use pdfcer_core::text_edit::{EditableTextModel, reflow_recognition_options};
use pdfcer_core::text_extract::{self, ExtractOptions};

use crate::block_layout::build_pdf;

/// F1 Helvetica, F2 Helvetica-Bold, F3 Helvetica-Oblique, F4 a subset
/// `TimesNewRomanPSMT` with widths.
fn doc(content: &str) -> Document {
    let widths = vec!["500"; 95].join(" ");
    let bodies = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R \
         /Resources << /Font << /F1 5 0 R /F2 6 0 R /F3 7 0 R /F4 8 0 R >> >> >>"
            .to_owned(),
        format!("STREAM:{content}"),
        font("Helvetica"),
        font("Helvetica-Bold"),
        font("Helvetica-Oblique"),
        format!(
            "<< /Type /Font /Subtype /TrueType /BaseFont /ABCDEF+TimesNewRomanPSMT \
             /Encoding /WinAnsiEncoding /FirstChar 32 /LastChar 126 /Widths [{widths}] >>"
        ),
    ];
    Document::from_bytes(build_pdf(&bodies)).expect("loads")
}

fn font(base: &str) -> String {
    format!("<< /Type /Font /Subtype /Type1 /BaseFont /{base} /Encoding /WinAnsiEncoding >>")
}

/// Heading, a blank line, then a two-line paragraph with a bold word, an
/// italic red word and a Times run.
const PAGE: &str = "\
BT /F2 14 Tf 72 730 Td (Seat Belt Anchorages) Tj ET
BT /F1 12 Tf 72 712 Td ( ) Tj ET
BT /F1 12 Tf 72 694 Td (The ) Tj /F2 12 Tf (bold) Tj /F1 12 Tf ( word and an ) Tj ET
BT 1 0 0 rg /F3 12 Tf 72 680 Td (italic) Tj 0 g /F4 12 Tf ( Times end.) Tj ET
";

fn rich(content: &str, whole: bool) -> RichText {
    let doc = doc(content);
    let pages = page_tree::pages(&doc).expect("page tree");
    let opts = ExtractOptions::default().with_provenance(true);
    let text = text_extract::extract_page(&doc, &pages[0], 0, &opts).expect("extracts");
    let model = EditableTextModel::recognize(&text, &reflow_recognition_options());
    let view = doc.view();
    let fonts = PageFonts::read(&view, &pages[0]);
    let deco = page_decorations(&view, &pages[0]).expect("decorations");
    let sources = RichTextSources::default()
        .with_fonts(&fonts)
        .with_decorations(&deco);
    let range = if whole {
        None
    } else {
        let at = model.hit_test(80.0, 697.0).expect("hits the paragraph");
        model.line_range_at(at)
    };
    model.rich_text(range, &sources)
}

#[test]
fn one_paragraph_per_block_with_its_runs_styled() {
    let r = rich(PAGE, true);
    let paras: Vec<Vec<String>> = r
        .paragraphs
        .iter()
        .map(|p| p.spans.iter().map(|s| s.text.clone()).collect())
        .collect();
    assert_eq!(
        paras,
        vec![
            vec!["Seat Belt Anchorages".to_owned()],
            vec![],
            vec![
                "The ".to_owned(),
                "bold".to_owned(),
                " word and an ".to_owned(),
                "italic".to_owned(),
                " Times end.".to_owned(),
            ],
        ],
        "{r:#?}"
    );
    assert_eq!(r.outcome.paragraphs, 3);
    let head = &r.paragraphs[0].spans[0].style;
    assert!(head.bold && !head.italic);
    assert_eq!(head.size, 14.0);
    assert_eq!(head.family.as_deref(), Some("Helvetica"));
    let body = &r.paragraphs[2].spans;
    assert!(!body[0].style.bold && body[1].style.bold);
    assert!(body[3].style.italic);
    assert_eq!(body[3].style.color, Some([255, 0, 0]));
    assert_eq!(body[4].style.family.as_deref(), Some("Times New Roman"));
    assert_eq!(body[4].style.color, Some([0, 0, 0]));
    assert_eq!(r.outcome.families_missing, 0);
    assert_eq!(
        r.plain_text(),
        "Seat Belt Anchorages\n\nThe bold word and an italic Times end."
    );
}

#[test]
fn a_range_copies_only_its_glyphs() {
    let r = rich(PAGE, false);
    assert_eq!(r.paragraphs.len(), 1, "{r:#?}");
    assert_eq!(r.plain_text(), "The bold word and an");
}

#[test]
fn html_carries_styles_and_escapes_text() {
    let r = rich("BT /F2 12 Tf 72 730 Td (a < b & \"c\"  d) Tj ET\n", true);
    let html = r.to_html();
    assert!(
        html.contains(
            "<span style=\"font-family:'Helvetica';font-size:12pt;font-weight:bold\">\
             a &lt; b &amp; &quot;c&quot; &nbsp;d</span>"
        ),
        "{html}"
    );
    let empty = rich(PAGE, true).to_html();
    assert!(empty.contains(";font-size:12pt\">&nbsp;</p>"), "{empty}");
}

#[test]
fn cf_html_offsets_point_at_the_document_and_fragment() {
    let cf = rich(PAGE, true).to_cf_html();
    let field = |name: &str| -> usize {
        let at = cf.find(name).expect("field") + name.len();
        cf[at..at + 10].parse().expect("ten digits")
    };
    let (sh, eh) = (field("StartHTML:"), field("EndHTML:"));
    let (sf, ef) = (field("StartFragment:"), field("EndFragment:"));
    assert_eq!(eh, cf.len());
    assert!(cf[sh..].starts_with("<html>"));
    assert!(cf[..sf].ends_with("<!--StartFragment-->"));
    assert!(cf[ef..].starts_with("<!--EndFragment-->"));
    assert!(cf[sf..ef].starts_with("<p "));
}

#[test]
fn rtf_has_font_and_colour_tables_and_escapes() {
    let rtf = rich(PAGE, true).to_rtf();
    assert!(rtf.starts_with("{\\rtf1\\ansi"), "{rtf}");
    assert!(
        rtf.contains("{\\f0\\fnil Helvetica;}{\\f1\\fnil Times New Roman;}"),
        "{rtf}"
    );
    assert!(rtf.contains("\\red255\\green0\\blue0;"), "{rtf}");
    assert!(
        rtf.contains("{\\f0\\fs28\\b Seat Belt Anchorages}\\par"),
        "{rtf}"
    );
    assert!(rtf.contains("{\\f0\\fs24\\i\\cf1 italic}"), "{rtf}");
    let esc = rich("BT /F1 12 Tf 72 730 Td ({x} \\\\ caf\\351) Tj ET\n", true).to_rtf();
    assert!(esc.contains(" \\{x\\} \\\\ caf\\u233?}"), "{esc}");
}

#[test]
fn an_unconvertible_colour_is_counted_not_guessed() {
    let r = rich("BT 0 0 0 1 k /F1 12 Tf 72 730 Td (cmyk) Tj ET\n", true);
    assert_eq!(r.outcome.colours_dropped, 4);
    assert_eq!(r.paragraphs[0].spans[0].style.color, None);
}

/// One page painting form `Fm1` (readable) and, when `broken`, form `Fm2`
/// whose Flate stream does not decode.
fn forms_doc(broken: bool) -> Document {
    let text = "BT /F1 12 Tf 72 700 Td (in a form) Tj ET";
    let content = if broken {
        "q /Fm1 Do Q q /Fm2 Do Q"
    } else {
        "q /Fm1 Do Q"
    };
    let bodies = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R \
         /Resources << /XObject << /Fm1 6 0 R /Fm2 7 0 R >> >> >>"
            .to_owned(),
        format!("STREAM:{content}"),
        font("Helvetica"),
        format!(
            "<< /Type /XObject /Subtype /Form /BBox [0 0 612 792] \
             /Resources << /Font << /F1 5 0 R >> >> /Length {} >>\nstream\n{text}\nendstream",
            text.len()
        ),
        "<< /Type /XObject /Subtype /Form /BBox [0 0 612 792] /Filter /FlateDecode \
         /Length 9 >>\nstream\nnot flate\nendstream"
            .to_owned(),
    ];
    Document::from_bytes(build_pdf(&bodies)).expect("loads")
}

#[test]
fn a_form_whose_decorations_were_not_read_is_counted() {
    for (broken, want) in [(false, 0), (true, 1)] {
        let doc = forms_doc(broken);
        let pages = page_tree::pages(&doc).expect("page tree");
        let view = doc.view();
        let deco = page_decorations(&view, &pages[0]).expect("decorations");
        assert_eq!(deco.forms_unread, want, "broken={broken}");
        let opts = ExtractOptions::default().with_provenance(true);
        let text = text_extract::extract_page(&doc, &pages[0], 0, &opts).expect("extracts");
        let model = EditableTextModel::recognize(&text, &reflow_recognition_options());
        let sources = RichTextSources::default().with_decorations(&deco);
        let rich = model.rich_text(None, &sources);
        assert_eq!(
            rich.outcome.decoration_forms_unread, want,
            "broken={broken}"
        );
        let bare = model.rich_text(None, &RichTextSources::default());
        assert_eq!(bare.outcome.decoration_forms_unread, 0);
    }
}
