//! A rich-text field's appearance is drawn from its `/RV` and `/DS`
//! (ISO 32000-1 §12.7.3.4, RT-M6/M9): per-run fonts, colours, decoration
//! and paragraph alignment, with what was not drawn reported.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditError, EditSession};
use pdfcer_core::forms;
use pdfcer_core::graph::ObjectGraph as _;
use pdfcer_core::object::Object;
use pdfcer_core::richtext::{FlatText, MAX_RICH_BYTES, RichStylePrecedence, RichTextError};
use pdfcer_core::widget_draw::WidgetAppearances;

fn assemble<B: AsRef<[u8]>>(bodies: &[B]) -> Vec<u8> {
    let mut buf = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        buf.extend_from_slice(body.as_ref());
        buf.extend_from_slice(b"\nendobj\n");
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
    doc_with(ff, extra, &[], &[])
}

/// As [`doc`], with `/DR` fonts `(name, font dictionary)` as objects 6..,
/// then `more` objects after them.
fn doc_with(ff: i64, extra: &str, dr: &[(&str, &str)], more: &[Vec<u8>]) -> Document {
    let names: String = dr
        .iter()
        .enumerate()
        .map(|(i, (name, _))| format!(" /{name} {} 0 R", i + 6))
        .collect();
    let mut bodies = vec![
        format!(
            "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [4 0 R] \
             /DA (/Helv 10 Tf 0 g) /DR << /Font << /Helv 5 0 R{names} >> >> >> >>"
        )
        .into_bytes(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 200] /Annots [4 0 R] >>".to_vec(),
        format!(
            "<< /FT /Tx /T (t) /Ff {ff} /Type /Annot /Subtype /Widget /P 3 0 R \
             /Rect [20 50 320 90] {extra} >>"
        )
        .into_bytes(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica \
         /Encoding /WinAnsiEncoding >>"
            .to_vec(),
    ];
    bodies.extend(dr.iter().map(|(_, font)| font.as_bytes().to_vec()));
    bodies.extend(more.iter().cloned());
    Document::from_bytes(assemble(&bodies)).expect("fixture parses")
}

/// The font resource name in force at `needle`'s `Tj`.
fn font_at(content: &str, needle: &str) -> String {
    let at = content
        .find(needle)
        .unwrap_or_else(|| panic!("{needle:?} missing from:\n{content}"));
    let tf = content[..at].rfind(" Tf").unwrap();
    let slash = content[..tf].rfind('/').unwrap();
    content[slash + 1..tf].split(' ').next().unwrap().to_owned()
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

/// A stream `/RV` whose filter fails is reported unreadable, like one that
/// will not parse, and `/V` is drawn plain.
#[test]
fn an_undecodable_rv_stream_is_drawn_plain_and_counted() {
    let garbage = b"not deflate data at all";
    let mut rv = format!(
        "<< /Length {} /Filter /FlateDecode >>\nstream\n",
        garbage.len()
    )
    .into_bytes();
    rv.extend_from_slice(garbage);
    rv.extend_from_slice(b"\nendstream");
    let mut s = EditSession::new(doc_with(RICH, "/V (plain) /RV 6 0 R", &[], &[rv]));
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

/// A simple non-standard TrueType face, WinAnsi-encoded, so `/DR` binds it.
fn truetype(base: &str) -> String {
    format!(
        "<< /Type /Font /Subtype /TrueType /BaseFont /{base} /FirstChar 32 \
         /LastChar 126 /Widths [{}] /Encoding /WinAnsiEncoding >>",
        vec!["500"; 95].join(" ")
    )
}

#[test]
fn a_family_resolves_to_its_dr_font_by_base_font() {
    let courier = "<< /Type /Font /Subtype /Type1 /BaseFont /ABCDEF+Courier-Bold \
                   /Encoding /WinAnsiEncoding >>";
    let mut s = EditSession::new(doc_with(RICH, "", &[("CoBo", courier)], &[]));
    let rv = "<body><p><span style=\"font-family:Courier; font-weight:bold\">x</span>\
              </p></body>";
    s.fill_rich_text_field("t", rv, FlatText::default())
        .unwrap();
    assert_eq!(font_at(&normal_ap(&s).0, "(x) Tj"), "CoBo");
}

#[test]
fn a_run_takes_the_dr_face_of_its_family_in_its_own_weight() {
    let regular = truetype("ABCDEF+Gadget-Regular");
    let bold = truetype("GHIJKL+Gadget-Bold");
    let dr = [("GaR", regular.as_str()), ("GaB", bold.as_str())];
    let mut s = EditSession::new(doc_with(RICH, "", &dr, &[]));
    let rv = "<body><p style=\"font-family:'Gadget'\">plain <b>heavy</b></p></body>";
    let out = s
        .fill_rich_text_field("t", rv, FlatText::default())
        .unwrap();
    let (content, _) = normal_ap(&s);
    assert_eq!(font_at(&content, "(plain) Tj"), "GaR");
    assert_eq!(font_at(&content, "(heavy) Tj"), "GaB");
    assert!(
        out.notes.rich_fonts.is_empty(),
        "{:?}",
        out.notes.rich_fonts
    );
}

#[test]
fn a_stream_rv_is_decoded_and_drawn() {
    use std::io::Write as _;
    let rv = b"<body><p><b>S</b></p></body>";
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    z.write_all(rv).unwrap();
    let packed = z.finish().unwrap();
    let mut stream = format!(
        "<< /Length {} /Filter /FlateDecode >>\nstream\n",
        packed.len()
    )
    .into_bytes();
    stream.extend_from_slice(&packed);
    stream.extend_from_slice(b"\nendstream");
    let fixture = || doc_with(RICH, "/V (S) /RV 6 0 R", &[], std::slice::from_ref(&stream));
    let mut s = EditSession::new(fixture());
    assert_eq!(field(&s).rich_value.as_deref(), Some(&rv[..]));
    let out = s.regenerate_appearances().unwrap();
    assert_eq!(out.display.rich_text, 1);
    let (content, _) = normal_ap(&s);
    assert_eq!(font_at(&content, "(S) Tj"), "RtHelveticaBold");

    let d = fixture();
    let view = d.view();
    let form = forms::parse_acroform(&view).unwrap();
    assert_eq!(form.fields[0].rich_value.as_deref(), Some(&rv[..]));
}

/// A field whose `/DS` and `/DA` disagree on family, size and colour.
fn disagreeing() -> Document {
    doc(
        RICH,
        "/V (x) /RV (<body><p>x <span style=\"font-size:8pt\">own</span></p></body>) \
         /DS (font-family:Courier; font-size:14pt; color:#0000ff)",
    )
}

#[test]
fn the_style_string_wins_over_the_default_appearance_by_default() {
    let mut s = EditSession::new(disagreeing());
    assert_eq!(
        s.rich_style_precedence(),
        RichStylePrecedence::RichStyleFirst
    );
    s.regenerate_appearances().unwrap();
    let (content, _) = normal_ap(&s);
    let x = content.find("(x) Tj").expect(&content);
    assert!(content[..x].contains("/RtCourier 14 Tf"), "{content}");
    assert!(content[..x].contains("0 0 1 rg"), "{content}");
    assert!(
        content.contains("/RtCourier 8 Tf"),
        "own size wins: {content}"
    );
}

#[test]
fn the_default_appearance_can_be_put_first() {
    let mut s = EditSession::new(disagreeing());
    s.set_rich_style_precedence(RichStylePrecedence::DefaultAppearanceFirst);
    s.regenerate_appearances().unwrap();
    let (content, _) = normal_ap(&s);
    let x = content.find("(x) Tj").expect(&content);
    assert!(content[..x].contains("/Helv 10 Tf"), "{content}");
    assert!(!content[..x].contains("0 0 1 rg"), "{content}");
    assert!(content.contains("/Helv 8 Tf"), "own size wins: {content}");

    let d = disagreeing();
    let view = d.view();
    let id = field(&s).widgets[0].id;
    let form = WidgetAppearances::read(&view)
        .unwrap()
        .with_rich_style_precedence(RichStylePrecedence::DefaultAppearanceFirst);
    let drawn = String::from_utf8_lossy(&form.appearance(&view, id).unwrap().content).into_owned();
    assert_eq!(content, drawn, "regeneration and view time disagree");
}

#[test]
fn a_rich_value_past_the_size_bound_is_refused() {
    let rv = format!("<body><p>{}</p></body>", "a".repeat(MAX_RICH_BYTES));
    let mut s = EditSession::new(doc(RICH, ""));
    let err = s
        .fill_rich_text_field("t", &rv, FlatText::default())
        .unwrap_err();
    assert!(
        matches!(
            err,
            EditError::InvalidRichText {
                source: RichTextError::TooLarge {
                    limit: MAX_RICH_BYTES,
                    ..
                },
                ..
            }
        ),
        "{err:?}"
    );
    assert!(!s.is_modified());
}
