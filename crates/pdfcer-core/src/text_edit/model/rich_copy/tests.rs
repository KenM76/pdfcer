#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)] // a panic here is the test failing
//! Rich copy writer hardening: hostile font names and text, non-finite
//! sizes, and the caps (request X040).

use std::fmt::Write as _;

use super::write::{END_FRAGMENT, MAX_FAMILY_CHARS, SPAN_BYTES_MAX};
use super::*;
use crate::document::Document;
use crate::text_edit::reflow_recognition_options;
use crate::text_extract::{self, ExtractOptions, PageText};

fn span(text: &str, family: Option<&str>, size: f32) -> RichSpan {
    RichSpan {
        text: text.to_owned(),
        style: SpanStyle {
            family: family.map(str::to_owned),
            size,
            ..SpanStyle::default()
        },
    }
}

fn copy(paragraphs: Vec<Vec<RichSpan>>) -> RichText {
    RichText {
        paragraphs: paragraphs
            .into_iter()
            .map(|spans| RichParagraph {
                spans,
                size: 12.0,
                space_after: 0.0,
            })
            .collect(),
        outcome: RichTextOutcome::default(),
    }
}

/// Every `style="…"` attribute value in `html`.
fn style_attrs(html: &str) -> Vec<&str> {
    html.split("style=\"")
        .skip(1)
        .filter_map(|rest| rest.split('"').next())
        .collect()
}

/// The `{\fonttbl …}` group of `rtf`.
fn font_table(rtf: &str) -> &str {
    let start = rtf.find("{\\fonttbl").expect("font table");
    let end = rtf[start..]
        .find("}{\\colortbl")
        .expect("colour table follows");
    &rtf[start..start + end]
}

/// Tags balanced and the document closed.
fn assert_html_well_formed(html: &str) {
    assert_eq!(
        html.matches("<span ").count(),
        html.matches("</span>").count()
    );
    assert_eq!(html.matches("<p ").count(), html.matches("</p>").count());
    assert!(html.contains(END_FRAGMENT));
    assert!(html.ends_with("</body></html>\n"));
}

/// Groups balanced, honouring `\{`, `\}` and `\\`.
fn assert_rtf_well_formed(rtf: &str) {
    let mut depth = 0_i64;
    let mut chars = rtf.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                chars.next();
            }
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                assert!(depth >= 0, "a group closed that was never opened");
            }
            _ => {}
        }
    }
    assert_eq!(depth, 0, "every group closed");
    assert!(rtf.ends_with('}'));
}

#[test]
fn hostile_font_names_stay_inside_their_css_string_and_rtf_entry() {
    let rich = copy(vec![vec![
        span("one", Some("a\\b\nc\0'\"&<>"), 12.0),
        span("two", Some("x;y"), 12.0),
    ]]);
    let html = rich.to_html();
    for style in style_attrs(&html) {
        if let Some(family) = style.strip_prefix("font-family:'") {
            let family = family.split("';").next().expect("string closes");
            assert!(
                !family.contains(['\n', '\0', '\'', '"', '<', '>', '&']),
                "{family:?}"
            );
            // Every backslash opens a hex escape ended by a space.
            for esc in family.split('\\').skip(1) {
                let hex: String = esc.chars().take_while(char::is_ascii_hexdigit).collect();
                assert!(
                    !hex.is_empty() && esc[hex.len()..].starts_with(' '),
                    "{family:?}"
                );
            }
        }
    }
    assert!(
        html.contains("font-family:'a\\5c bc\\27 \\22 \\26 \\3c \\3e ';"),
        "{html}"
    );
    let rtf = rich.to_rtf();
    let table = font_table(&rtf);
    assert_eq!(table.matches("\\fnil ").count(), 2);
    assert_eq!(
        table.matches(';').count(),
        2,
        "one entry terminator per font: {table}"
    );
    assert!(table.contains("{\\f0\\fnil a\\\\bc'\"&<>;}"), "{table}");
    assert!(table.contains("{\\f1\\fnil xy;}"), "{table}");
    assert!(!rtf.contains('\0'));
}

#[test]
fn a_long_family_is_cut() {
    let long = "a".repeat(MAX_FAMILY_CHARS * 4);
    let rich = copy(vec![vec![span("t", Some(&long), 12.0)]]);
    let html = rich.to_html();
    assert!(html.contains(&format!("font-family:'{}';", "a".repeat(MAX_FAMILY_CHARS))));
    assert!(font_table(&rich.to_rtf()).contains(&format!(" {};", "a".repeat(MAX_FAMILY_CHARS))));
}

#[test]
fn control_characters_are_absent_from_both_outputs() {
    let rich = copy(vec![vec![span(
        "a\0b\u{1}c\u{1f}d\re\u{7f}f\u{85}g\th\ni",
        None,
        12.0,
    )]]);
    let html = rich.to_html();
    let rtf = rich.to_rtf();
    for out in [&html, &rtf] {
        assert!(
            !out.chars()
                .any(|c| matches!(c, '\0' | '\u{1}' | '\u{1f}' | '\r' | '\u{7f}' | '\u{85}')),
            "{out:?}"
        );
    }
    assert!(html.contains(">abcdefg\th<br>i</span>"), "{html}");
    assert!(rtf.contains(" abcdefg\\tab h\\line i}"), "{rtf}");
}

#[test]
fn plain_text_drops_the_controls_the_other_writers_drop() {
    let rich = copy(vec![vec![span(
        "a\0b\u{1}c\td\u{85}",
        Some("Helvetica"),
        12.0,
    )]]);
    assert_eq!(rich.plain_text(), "abc\td");
}

#[test]
fn non_finite_and_out_of_range_sizes_are_written_finite_and_in_range() {
    let sizes = [
        f32::NAN,
        f32::INFINITY,
        f32::NEG_INFINITY,
        -5.0,
        0.0,
        5000.0,
    ];
    let mut rich = copy(
        sizes
            .iter()
            .map(|&s| vec![span("x", None, s)])
            .chain([vec![]])
            .collect(),
    );
    for p in &mut rich.paragraphs {
        p.size = f32::NAN;
        p.space_after = f32::INFINITY;
    }
    let html = rich.to_html();
    assert!(!html.contains("NaN") && !html.contains("inf"), "{html}");
    for v in html.split("font-size:").skip(1) {
        let n: f32 = v.split("pt").next().unwrap().parse().expect("a number");
        assert!(
            (RICH_COPY_MIN_SIZE_PT..=RICH_COPY_MAX_SIZE_PT).contains(&n),
            "{n}"
        );
    }
    assert!(
        html.contains("font-size:12pt\">x"),
        "NaN falls back to the default"
    );
    assert!(
        html.contains("font-size:1pt\">x"),
        "-5 clamps to the minimum"
    );
    assert!(
        html.contains("font-size:1638pt\">x"),
        "5000 clamps to the maximum"
    );
    let rtf = rich.to_rtf();
    for v in rtf.split("\\fs").skip(1) {
        let n: i32 = v
            .chars()
            .take_while(char::is_ascii_digit)
            .collect::<String>()
            .parse()
            .expect("a half-point size");
        assert!((2..=3276).contains(&n), "{n}");
    }
    for v in rtf.split("\\sa").skip(1) {
        let n: String = v
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '-')
            .collect();
        assert!(
            (0..=32760).contains(&n.parse::<i32>().expect("twips")),
            "{n}"
        );
    }
}

#[test]
fn worst_span_fits_its_bound() {
    let family = "\u{1F600}".repeat(MAX_FAMILY_CHARS);
    let mut worst = span("", Some(&family), 1637.5);
    worst.style.bold = true;
    worst.style.italic = true;
    worst.style.underline = true;
    worst.style.strikethrough = true;
    worst.style.color = Some([255, 255, 255]);
    worst.style.family_as_named = Some("\\".repeat(MAX_FAMILY_CHARS));
    let empty = copy(vec![vec![]]);
    let one = copy(vec![vec![worst]]);
    for (a, b) in [
        (empty.to_html().len(), one.to_html().len()),
        (empty.to_rtf().len(), one.to_rtf().len()),
    ] {
        assert!(b - a <= SPAN_BYTES_MAX, "{} > {SPAN_BYTES_MAX}", b - a);
    }
}

#[test]
fn a_copy_over_the_span_cap_is_written_within_the_byte_cap_and_well_formed() {
    let spans: Vec<RichSpan> = (0..RICH_COPY_MAX_SPANS + 500)
        .map(|i| span("w", Some(&format!("F{i}")), 12.0))
        .collect();
    let rich = copy(vec![spans]);
    let html = rich.to_html();
    assert_eq!(html.matches("<span ").count(), RICH_COPY_MAX_SPANS);
    assert!(html.len() <= RICH_COPY_MAX_OUTPUT_BYTES);
    assert_html_well_formed(&html);
    let rtf = rich.to_rtf();
    assert!(rtf.len() <= RICH_COPY_MAX_OUTPUT_BYTES);
    assert_eq!(
        font_table(&rtf).matches("\\fnil ").count(),
        RICH_COPY_MAX_SPANS
    );
    assert_rtf_well_formed(&rtf);
}

#[test]
fn an_oversized_copy_is_cut_within_the_byte_cap_and_well_formed() {
    // `<`, an open brace, a backslash and a space: each escapes to more bytes.
    let unit: String = ['<', '\u{7b}', '\\', ' '].iter().collect();
    let huge = unit.repeat(RICH_COPY_MAX_OUTPUT_BYTES / 4);
    let rich = copy(vec![
        vec![span(&huge, None, 12.0)],
        vec![span("tail", None, 12.0)],
    ]);
    let html = rich.to_html();
    assert!(html.len() <= RICH_COPY_MAX_OUTPUT_BYTES, "{}", html.len());
    assert_html_well_formed(&html);
    assert!(rich.to_cf_html().len() <= RICH_COPY_MAX_OUTPUT_BYTES);
    let rtf = rich.to_rtf();
    assert!(rtf.len() <= RICH_COPY_MAX_OUTPUT_BYTES, "{}", rtf.len());
    assert_rtf_well_formed(&rtf);
}

/// A one-page PDF whose content is `content`, with `/F1` Helvetica and
/// `/F2` Helvetica-Bold.
fn pdf(content: &str) -> Vec<u8> {
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R \
         /Resources << /Font << /F1 5 0 R /F2 6 0 R >> >> >>"
            .to_owned(),
        format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len() + 1
        ),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_owned(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold >>".to_owned(),
    ];
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    out.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", bodies.len() + 1).as_bytes(),
    );
    for o in offsets {
        out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            bodies.len() + 1
        )
        .as_bytes(),
    );
    out
}

#[test]
fn the_model_stops_at_the_span_cap_and_says_so() {
    // Alternating regular and bold glyphs: every glyph is its own run.
    let mut content = String::new();
    let per_line = 120;
    let lines = RICH_COPY_MAX_SPANS / per_line + 2;
    for l in 0..lines {
        let _ = write!(content, "BT 20 {} Td ", 780 - l * 6);
        for g in 0..per_line {
            let font = if g % 2 == 0 { "F1" } else { "F2" };
            let _ = write!(content, "/{font} 4 Tf (a) Tj ");
        }
        content.push_str("ET\n");
    }
    let doc = Document::from_bytes(pdf(&content)).expect("loads");
    let pages = crate::page_tree::pages(&doc).expect("page tree");
    let text: PageText = text_extract::extract_page(&doc, &pages[0], 0, &ExtractOptions::default())
        .expect("extracts");
    let model = EditableTextModel::recognize(&text, &reflow_recognition_options());
    let rich = model.rich_text(None, &RichTextSources::default());
    let spans: usize = rich.paragraphs.iter().map(|p| p.spans.len()).sum();
    assert!(rich.outcome.truncated, "the copy says it was cut");
    assert!(
        spans <= RICH_COPY_MAX_SPANS && spans > RICH_COPY_MAX_SPANS - per_line,
        "{spans}"
    );
    let html = rich.to_html();
    assert!(html.len() <= RICH_COPY_MAX_OUTPUT_BYTES);
    assert_html_well_formed(&html);
    assert_rtf_well_formed(&rich.to_rtf());
}

#[test]
fn a_copy_under_the_caps_is_not_marked_truncated() {
    let doc = Document::from_bytes(pdf("BT /F1 12 Tf 72 700 Td (Hello) Tj ET")).expect("loads");
    let pages = crate::page_tree::pages(&doc).expect("page tree");
    let text = text_extract::extract_page(&doc, &pages[0], 0, &ExtractOptions::default())
        .expect("extracts");
    let model = EditableTextModel::recognize(&text, &reflow_recognition_options());
    let rich = model.rich_text(None, &RichTextSources::default());
    assert_eq!(rich.plain_text(), "Hello");
    assert!(!rich.outcome.truncated);
}

/// The HTML between the fragment markers, `&nbsp;` read as a space and
/// tags removed: what a paste target shows.
fn pasted(html: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    let body = html.replace("<br>", "\n").replace("&nbsp;", "\u{a0}");
    for c in body.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
}

#[test]
fn spaces_split_across_runs_and_after_a_break_keep_their_width() {
    let mut bold = span(" b", None, 12.0);
    bold.style.bold = true;
    let html = copy(vec![vec![
        span("a ", None, 12.0),
        bold,
        span("\n c", None, 12.0),
    ]])
    .to_html();
    let text = pasted(&html);
    // In HTML a plain space after a space or a line start collapses; every
    // such space must be a non-breaking one.
    assert!(text.contains("a \u{a0}b"), "{html}");
    assert!(text.contains("\n\u{a0}c"), "{html}");
    let lead = copy(vec![vec![span(" x", None, 12.0)]]).to_html();
    assert!(pasted(&lead).contains("\u{a0}x"), "{lead}");
}

#[test]
fn a_hyphen_splits_the_family_only_before_a_style_word() {
    for (base, family, spelt, style) in [
        ("MS-Gothic", "MS Gothic", "MS-Gothic", ""),
        ("Arial-BoldMT", "Arial", "Arial", "BoldMT"),
        (
            "ABCDEF+Helvetica-Oblique",
            "Helvetica",
            "Helvetica",
            "Oblique",
        ),
        ("Times-Roman", "Times", "Times", "Roman"),
        ("TimesNewRomanPSMT", "Times New Roman", "TimesNewRoman", ""),
        ("Arial,BoldItalic", "Arial", "Arial", "BoldItalic"),
        ("Foo-Blackadder", "Foo Blackadder", "Foo-Blackadder", ""),
        ("SimSun", "Sim Sun", "SimSun", ""),
    ] {
        assert_eq!(
            fonts::split_base_font(base),
            (family.to_owned(), spelt.to_owned(), style.to_owned()),
            "{base}"
        );
    }
}

#[test]
fn html_names_the_spaced_family_then_the_spelt_one() {
    let mut s = span("x", Some("Sim Sun"), 12.0);
    s.style.family_as_named = Some("SimSun".to_owned());
    let html = copy(vec![vec![s]]).to_html();
    assert!(html.contains("font-family:'Sim Sun','SimSun';"), "{html}");
    assert!(
        copy(vec![vec![span("x", Some("Sim Sun"), 12.0)]])
            .to_rtf()
            .contains("Sim Sun;")
    );
    let same = copy(vec![vec![span("x", Some("Arial"), 12.0)]]).to_html();
    assert!(same.contains("font-family:'Arial';"), "{same}");
}
