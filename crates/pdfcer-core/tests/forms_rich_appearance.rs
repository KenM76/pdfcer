//! A rich-text field's appearance is drawn from its `/RV` and `/DS`
//! (ISO 32000-1 §12.7.3.4, RT-M6/M9): per-run fonts, colours, decoration
//! and paragraph alignment, with what was not drawn reported.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditError, EditSession};
use pdfcer_core::forms;
use pdfcer_core::graph::ObjectGraph as _;
use pdfcer_core::object::Object;
use pdfcer_core::richtext::{FlatText, RichTextError};
use pdfcer_core::widget_draw::WidgetAppearances;

fn assemble(bodies: &[String]) -> Vec<u8> {
    let mut buf = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref_at = buf.len();
    let size = bodies.len() + 1;
    buf.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f \n").as_bytes());
    for off in &offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n")
            .as_bytes(),
    );
    buf
}

/// `/Ff` bit 26, `RichText`.
const RICH: i64 = 1 << 25;

/// A 300x40 text field "t" with `/Ff ff` and `extra` entries, under a form
/// whose `/DR` holds only Helvetica as `/Helv`.
fn doc(ff: i64, extra: &str) -> Document {
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [4 0 R] \
         /DA (/Helv 10 Tf 0 g) /DR << /Font << /Helv 5 0 R >> >> >> >>"
            .to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 200] /Annots [4 0 R] >>".to_owned(),
        format!(
            "<< /FT /Tx /T (t) /Ff {ff} /Type /Annot /Subtype /Widget /P 3 0 R \
             /Rect [20 50 320 90] {extra} >>"
        ),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica \
         /Encoding /WinAnsiEncoding >>"
            .to_owned(),
    ];
    Document::from_bytes(assemble(&bodies)).expect("fixture parses")
}

fn field(s: &EditSession) -> forms::Field {
    forms::parse_acroform(&s.graph())
        .unwrap()
        .fields
        .into_iter()
        .find(|f| f.fully_qualified_name == "t")
        .unwrap()
}

/// The field's `/AP /N` content and its `/Resources /Font` dictionary.
fn normal_ap(s: &EditSession) -> (String, String) {
    let g = s.graph();
    let w = g
        .resolved(field(s).widgets[0].id)
        .as_dict()
        .cloned()
        .unwrap();
    let ap = w.get(b"AP").map(|o| g.resolve(o).clone()).unwrap();
    let n = ap
        .as_dict()
        .and_then(|d| d.get(b"N"))
        .map(|o| g.resolve(o).clone());
    let Some(Object::Stream(st)) = n else {
        panic!("no /AP /N stream: {n:?}");
    };
    let content = String::from_utf8_lossy(s.view().slice(st.data_span).unwrap()).into_owned();
    (content, format!("{:?}", st.dict))
}

/// The x operand of the first `Tm`.
fn first_tm_x(content: &str) -> f64 {
    let line = content.lines().find(|l| l.ends_with(" Tm")).unwrap();
    line.split_whitespace().nth(4).unwrap().parse().unwrap()
}

const MIXED: &str = "<body><p>plain <b>bold</b> <i>italic</i> \
                     <span style=\"color:#ff0000\">red</span></p></body>";

#[test]
fn each_run_is_drawn_in_its_own_font_and_colour() {
    let mut s = EditSession::new(doc(RICH, ""));
    s.fill_rich_text_field("t", MIXED, FlatText::default())
        .unwrap();
    let (content, dict) = normal_ap(&s);
    let at = |needle: &str| {
        content
            .find(needle)
            .unwrap_or_else(|| panic!("{needle:?} missing from:\n{content}"))
    };
    let bold = at("(bold) Tj");
    let italic = at("(italic) Tj");
    let red = at("(red) Tj");
    assert!(at("/Helv 10 Tf") < at("(plain) Tj"), "{content}");
    let bold_tf = content[..bold].rfind(" Tf").unwrap();
    assert!(
        content[..bold_tf].ends_with("/RtHelveticaBold 10"),
        "{content}"
    );
    let italic_tf = content[..italic].rfind(" Tf").unwrap();
    assert!(
        content[..italic_tf].ends_with("/RtHelveticaOblique 10"),
        "{content}"
    );
    let red_fill = content[..red].rfind(" rg").unwrap();
    assert!(content[..red_fill].ends_with("1 0 0"), "{content}");
    assert!(
        dict.contains("Helvetica-Bold"),
        "bold face not in /Resources: {dict}"
    );
    assert!(
        dict.contains("Helvetica-Oblique"),
        "italic face not in /Resources: {dict}"
    );
}

#[test]
fn a_centred_paragraph_is_drawn_centred() {
    let mut rich = EditSession::new(doc(RICH, ""));
    rich.fill_rich_text_field(
        "t",
        "<body><p style=\"text-align:center\">Hi</p></body>",
        FlatText::default(),
    )
    .unwrap();
    let mut left = EditSession::new(doc(RICH, ""));
    left.fill_rich_text_field("t", "<body><p>Hi</p></body>", FlatText::default())
        .unwrap();
    let centred = first_tm_x(&normal_ap(&rich).0);
    let left = first_tm_x(&normal_ap(&left).0);
    // "Hi" at 10pt Helvetica is 722+222 = 9.44pt wide in a 300pt box.
    assert!(
        (centred - (300.0 - 9.44) / 2.0).abs() < 0.01,
        "centred x {centred}, left x {left}"
    );
}

#[test]
fn an_unknown_tag_draws_its_text_in_the_default_style_and_is_reported() {
    let mut s = EditSession::new(doc(RICH, ""));
    let out = s
        .fill_rich_text_field(
            "t",
            "<body><p>a <u>under</u></p></body>",
            FlatText::default(),
        )
        .unwrap();
    assert_eq!(out.notes.rich_ignored, ["<u>"]);
    let (content, _) = normal_ap(&s);
    let under = content.find("(under) Tj").expect(&content);
    assert!(!content[..under].contains("RtHelvetica"), "{content}");
    assert!(
        !content.contains(" re\nf"),
        "<u> must not be guessed as underline: {content}"
    );
}

#[test]
fn an_unresolvable_font_uses_the_default_and_is_reported() {
    let mut s = EditSession::new(doc(RICH, ""));
    let out = s
        .fill_rich_text_field(
            "t",
            "<body><p style=\"font-family:Wingdings\">x</p></body>",
            FlatText::default(),
        )
        .unwrap();
    assert_eq!(out.notes.rich_fonts, ["Wingdings -> Helvetica"]);
    assert!(normal_ap(&s).0.contains("/Helv 10 Tf"));
}

#[test]
fn nesting_past_the_bound_is_an_error_not_an_overflow() {
    let nest = |n: usize| {
        format!(
            "<body>{}x{}</body>",
            "<span>".repeat(n),
            "</span>".repeat(n)
        )
    };
    let mut s = EditSession::new(doc(RICH, ""));
    let err = s
        .fill_rich_text_field("t", &nest(40), FlatText::default())
        .unwrap_err();
    assert!(
        matches!(
            err,
            EditError::InvalidRichText {
                source: RichTextError::TooDeep { .. },
                ..
            }
        ),
        "{err:?}"
    );
    let err = s
        .fill_rich_text_field("t", &nest(100_000), FlatText::default())
        .unwrap_err();
    assert!(matches!(err, EditError::InvalidRichText { .. }), "{err:?}");
    assert!(!s.is_modified());
    s.fill_rich_text_field("t", &nest(20), FlatText::default())
        .unwrap();
}

#[test]
fn an_unreadable_stored_rv_is_drawn_plain_and_counted() {
    let deep = format!(
        "<body>{}x{}</body>",
        "<span>".repeat(40),
        "</span>".repeat(40)
    );
    let mut s = EditSession::new(doc(RICH, &format!("/V (plain) /RV ({deep})")));
    let out = s.regenerate_appearances().unwrap();
    assert_eq!(out.display.rich_unreadable, 1);
    assert_eq!(out.display.rich_text, 0);
    assert!(normal_ap(&s).0.contains("(plain) Tj"));
}

#[test]
fn filling_rich_text_keeps_v_as_its_plain_text() {
    let mut s = EditSession::new(doc(RICH, ""));
    s.fill_rich_text_field("t", MIXED, FlatText::default())
        .unwrap();
    assert_eq!(field(&s).value.display_text(), "plain bold italic red");
    let rv = field(&s).rich_value.unwrap();
    assert_eq!(String::from_utf8_lossy(&rv), MIXED);

    let two = "<body><p>a</p><p>b</p></body>";
    s.fill_rich_text_field("t", two, FlatText::ParagraphLines)
        .unwrap();
    assert_eq!(field(&s).value.display_text(), "a\nb");
    s.fill_rich_text_field("t", two, FlatText::Concatenated)
        .unwrap();
    assert_eq!(field(&s).value.display_text(), "ab");
}

#[test]
fn a_plain_field_is_not_given_rich_text() {
    let mut s = EditSession::new(doc(0, ""));
    let err = s
        .fill_rich_text_field("t", MIXED, FlatText::default())
        .unwrap_err();
    assert!(matches!(err, EditError::FieldNotRichText { .. }), "{err:?}");
}

#[test]
fn regeneration_and_the_view_draw_the_stored_rich_text() {
    let rv = "<body><p><b>B</b></p></body>";
    let mut s = EditSession::new(doc(RICH, &format!("/V (B) /RV ({rv})")));
    let view_time = {
        let d = doc(RICH, &format!("/V (B) /RV ({rv})"));
        let view = d.view();
        let form = WidgetAppearances::read(&view).unwrap();
        let id = field(&s).widgets[0].id;
        String::from_utf8_lossy(&form.appearance(&view, id).unwrap().content).into_owned()
    };
    let out = s.regenerate_appearances().unwrap();
    assert_eq!(out.display.rich_text, 1);
    let (content, _) = normal_ap(&s);
    assert!(content.contains("/RtHelveticaBold 10 Tf"), "{content}");
    assert_eq!(content, view_time, "regeneration and view time disagree");
}

#[test]
fn the_downgrade_draws_the_plain_text_not_the_old_rich_value() {
    let rv = "<body><p><b>OLD</b></p></body>";
    let mut s = EditSession::new(doc(RICH, &format!("/V (OLD) /RV ({rv})")));
    s.fill_text_field_downgrading_rich_text("t", "new").unwrap();
    let (content, _) = normal_ap(&s);
    assert!(content.contains("(new) Tj"), "{content}");
    assert!(!content.contains("OLD"), "{content}");
}
