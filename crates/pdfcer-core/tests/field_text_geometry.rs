//! `WidgetAppearances::text_geometry` lays text out exactly as the field's
//! appearance does (§12.7.3.3 variable text, Table 228 Comb/Multiline,
//! Table 189 `/MK /R`), so an editor's caret sits on the drawn glyphs.

use pdfcer_core::document::Document;
use pdfcer_core::fontdata::Std14;
use pdfcer_core::object::ObjId;
use pdfcer_core::vartext::text_width;
use pdfcer_core::widget_draw::{FieldTextGeometry, WidgetAppearances};

const HELLO_LEFT: u32 = 4;
const HELLO_CENTRE: u32 = 5;
const HELLO_RIGHT: u32 = 6;
const COMB: u32 = 7;
const WRAP: u32 = 8;
const TURNED: u32 = 9;

fn form_pdf() -> Vec<u8> {
    let field = |extra: &str, rect: &str| {
        format!(
            "<< /Type /Annot /Subtype /Widget /FT /Tx {extra} /DA (/Helv 12 Tf 0 g) \
             /Rect [{rect}] /P 3 0 R >>"
        )
    };
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [4 0 R 5 0 R 6 0 R 7 0 R 8 0 R 9 0 R] >> >>"
            .to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Resources << >> \
         /Annots [4 0 R 5 0 R 6 0 R 7 0 R 8 0 R 9 0 R] >>"
            .to_owned(),
        field("/T (a) /V (Hello)", "20 250 220 280"),
        field("/T (b) /Q 1 /V (Hello)", "20 210 220 240"),
        field("/T (c) /Q 2 /V (Hello)", "20 170 220 200"),
        field("/T (d) /Ff 16777216 /MaxLen 5", "20 130 120 160"),
        field("/T (e) /Ff 4096", "20 20 70 110"),
        field("/T (f) /MK << /R 90 >>", "240 20 270 220"),
    ];
    let mut buf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref_at = buf.len();
    let size = bodies.len() + 1;
    buf.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f \n").as_bytes());
    for off in offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n")
            .as_bytes(),
    );
    buf
}

fn id(n: u32) -> ObjId {
    ObjId::new(n, 0)
}

fn geometry(widget: u32, text: &str) -> FieldTextGeometry {
    let doc = Document::from_bytes(form_pdf()).expect("loads");
    let form = WidgetAppearances::read(&doc.view()).expect("has a form");
    form.text_geometry(id(widget), text).expect("a text field")
}

/// The x of the single `Tm` in the appearance pdfcer draws for `widget`.
fn tm_x(widget: u32) -> f64 {
    let doc = Document::from_bytes(form_pdf()).expect("loads");
    let form = WidgetAppearances::read(&doc.view()).expect("has a form");
    let ap = form.appearance(&doc.view(), id(widget)).expect("drawn");
    let content = String::from_utf8(ap.content).expect("ascii");
    let before = content.split(" Tm").next().expect("a Tm");
    let nums: Vec<f64> = before
        .split_whitespace()
        .rev()
        .take(6)
        .map(|n| n.parse().expect("a number"))
        .collect();
    nums[1]
}

#[test]
fn hello_left_has_six_increasing_carets_spanning_its_advance() {
    let g = geometry(HELLO_LEFT, "Hello");
    let carets = g.carets();
    assert_eq!(carets.len(), 6);
    assert!(carets.windows(2).all(|p| p[1].bottom[0] > p[0].bottom[0]));
    assert_eq!(
        carets.iter().map(|c| c.offset).collect::<Vec<_>>(),
        [0, 1, 2, 3, 4, 5]
    );
    let first = carets[0].bottom[0];
    assert!((first - (20.0 + tm_x(HELLO_LEFT))).abs() < 1e-3, "{first}");
    let advance = text_width(Std14::Helvetica, 12.0, "Hello");
    assert!((carets[5].bottom[0] - first - advance).abs() < 1e-9);
    assert!((g.font_size - 12.0).abs() < 1e-9);
}

#[test]
fn centred_and_right_quadding_match_the_built_appearance() {
    for widget in [HELLO_CENTRE, HELLO_RIGHT] {
        let g = geometry(widget, "Hello");
        let start = g.carets()[0].bottom[0];
        assert!(
            (start - (20.0 + tm_x(widget))).abs() < 1e-3,
            "{widget}: {start}"
        );
    }
    let centre = geometry(HELLO_CENTRE, "Hello").carets()[0].bottom[0];
    let right = geometry(HELLO_RIGHT, "Hello").carets()[0].bottom[0];
    assert!(right > centre + 30.0, "centre {centre} right {right}");
}

#[test]
fn comb_spans_stay_inside_their_cells() {
    let g = geometry(COMB, "abcde");
    assert_eq!(g.chars.len(), 5);
    for (i, c) in g.chars.iter().enumerate() {
        let cell = 20.0 + 20.0 * i as f64;
        assert!(
            c.start[0] >= cell - 1e-9 && c.stop[0] <= cell + 20.0 + 1e-9,
            "{i}: {c:?}"
        );
        assert!(c.stop[0] > c.start[0]);
    }
}

#[test]
fn a_wrapping_multiline_field_has_two_lines_and_maps_offsets_across_the_wrap() {
    let g = geometry(WRAP, "Hello world");
    assert_eq!(g.lines.len(), 2, "{:?}", g.lines);
    let w = g.chars.iter().find(|c| c.offset == 6).expect("the w");
    assert_eq!(w.line, 1);
    let at_w = g.caret_segment(6).expect("a caret");
    let at_h = g.caret_segment(0).expect("a caret");
    assert!(at_w[0][1] < at_h[0][1] - 10.0, "line 2 is below line 1");
    assert!(
        (at_w[0][0] - at_h[0][0]).abs() < 1e-9,
        "both lines start left"
    );
    let mid = [
        (at_w[0][0] + at_w[1][0]) / 2.0,
        (at_w[0][1] + at_w[1][1]) / 2.0,
    ];
    assert_eq!(g.caret_at_point(mid[0], mid[1]), 6);
    assert_eq!(g.caret_at_point(at_h[0][0], at_h[0][1] + 2.0), 0);
}

#[test]
fn a_quarter_turned_field_rotates_the_caret() {
    let g = geometry(TURNED, "Hi");
    let [bottom, top] = g.caret_segment(0).expect("a caret");
    assert!((bottom[1] - top[1]).abs() < 1e-9, "{bottom:?} {top:?}");
    assert!(top[0] < bottom[0] - 5.0, "{bottom:?} {top:?}");
    let carets = g.carets();
    assert!(
        carets[1].bottom[1] > carets[0].bottom[1],
        "text runs up the page"
    );
}

#[test]
fn an_empty_string_has_one_caret_at_the_text_start() {
    let g = geometry(HELLO_LEFT, "");
    let carets = g.carets();
    assert_eq!(carets.len(), 1);
    assert_eq!(carets[0].offset, 0);
    assert!((carets[0].bottom[0] - 22.0).abs() < 1e-9);
    assert_eq!(g.caret_segment(0), Some([carets[0].bottom, carets[0].top]));
}

#[test]
fn an_unencodable_character_is_measured_as_a_question_mark() {
    let g = geometry(HELLO_LEFT, "H\u{4E2D}i");
    let cjk = g
        .chars
        .iter()
        .find(|c| c.offset == 1)
        .expect("the CJK char");
    assert_eq!(cjk.end, 4);
    let question = text_width(Std14::Helvetica, 12.0, "?");
    assert!((cjk.stop[0] - cjk.start[0] - question).abs() < 1e-9);
    assert_eq!(g.carets().len(), 4);
}
