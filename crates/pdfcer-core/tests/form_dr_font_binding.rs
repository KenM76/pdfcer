//! A regenerated field appearance draws with the document's own `/DR` font
//! when that font is not standard-14, rather than a Helvetica stand-in.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditSession, FieldAppearance, FieldEdit, NewPushButton};
use pdfcer_core::forms;
use pdfcer_core::graph::ObjectGraph as _;
use pdfcer_core::object::{Dict, ObjId, Object};
use pdfcer_core::page_tree::Rect;
use pdfcer_core::vartext::TextColor;

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

/// A one-field form whose `/DA` names `da_key`. `/DR` `/Font` carries
/// `/F1` (a WinAnsi Calibri, object 5), `/F2` (the same face with
/// `/Differences` and no `/Widths`, object 6), `/Helv` (standard-14,
/// object 7), `/F3` (codes 65 and 72 swapped by `/Differences`, with
/// `/Widths`, object 8), `/F4` (`/MacRomanEncoding`, object 9) and `/F5`
/// (`/F3` flagged Symbolic, object 10).
fn session(da_key: &str) -> EditSession {
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [4 0 R] \
         /DR << /Font << /F1 5 0 R /F2 6 0 R /Helv 7 0 R /F3 8 0 R /F4 9 0 R /F5 10 0 R >> >> /DA (/Helv 0 Tf 0 g) >> >>"
            .to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] /Annots [4 0 R] >>".to_owned(),
        format!(
            "<< /FT /Tx /T (t) /Type /Annot /Subtype /Widget /P 3 0 R \
             /Rect [20 50 200 72] /Q 1 /DA (/{da_key} 12 Tf 0 g) >>"
        ),
        "<< /Type /Font /Subtype /TrueType /BaseFont /Calibri \
         /Encoding /WinAnsiEncoding /FirstChar 72 /LastChar 72 /Widths [1000] \
         /FontDescriptor << /MissingWidth 250 /Ascent 900 >> >>"
            .to_owned(),
        "<< /Type /Font /Subtype /TrueType /BaseFont /Calibri \
         /Encoding << /BaseEncoding /WinAnsiEncoding /Differences [65 /B] >> >>"
            .to_owned(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica \
         /Encoding /WinAnsiEncoding >>"
            .to_owned(),
        "<< /Type /Font /Subtype /TrueType /BaseFont /Calibri \
         /Encoding << /BaseEncoding /WinAnsiEncoding /Differences [65 /H 72 /A] >> \
         /FirstChar 65 /LastChar 72 /Widths [500 0 0 0 0 0 0 1000] \
         /FontDescriptor << /MissingWidth 250 >> >>"
            .to_owned(),
        "<< /Type /Font /Subtype /TrueType /BaseFont /Calibri \
         /Encoding /MacRomanEncoding /FirstChar 142 /LastChar 142 /Widths [600] \
         /FontDescriptor << /MissingWidth 250 >> >>"
            .to_owned(),
        "<< /Type /Font /Subtype /TrueType /BaseFont /Calibri \
         /Encoding << /BaseEncoding /WinAnsiEncoding /Differences [65 /H 72 /A] >> \
         /FirstChar 65 /LastChar 72 /Widths [500 0 0 0 0 0 0 1000] \
         /FontDescriptor << /MissingWidth 250 /Flags 4 >> >>"
            .to_owned(),
    ];
    EditSession::new(Document::from_bytes(assemble(&bodies)).expect("fixture parses"))
}

/// The `/Resources` `/Font` entry `key` of the field's `/AP` `/N`.
fn ap_font(s: &EditSession, key: &[u8]) -> Object {
    let g = s.graph();
    let Object::Dict(field) = g.resolve(&Object::Reference(ObjId::new(4, 0))).clone() else {
        panic!("field is not a dictionary");
    };
    let ap: Dict = field
        .get(b"AP")
        .map(|o| g.resolve(o))
        .and_then(Object::as_dict)
        .cloned()
        .expect("/AP");
    let Some(Object::Stream(n)) = ap.get(b"N").map(|o| g.resolve(o).clone()) else {
        panic!("no /AP /N stream");
    };
    n.dict
        .get(b"Resources")
        .and_then(Object::as_dict)
        .and_then(|r| r.get(b"Font"))
        .and_then(Object::as_dict)
        .and_then(|f| f.get(key))
        .cloned()
        .unwrap_or_else(|| panic!("no /Font entry {}", String::from_utf8_lossy(key)))
}

#[test]
fn a_winansi_dr_font_is_the_font_the_appearance_draws_with() {
    let mut s = session("F1");
    s.fill_text_field("t", "Hi").unwrap();
    assert_eq!(
        ap_font(&s, b"F1"),
        Object::Reference(ObjId::new(5, 0)),
        "the appearance drew /F1 with a stand-in, not the document's Calibri"
    );
}

#[test]
fn a_font_with_differences_keeps_the_standard_14_stand_in() {
    // The generator writes WinAnsi codes; under /Differences those would
    // select other glyphs, so the document's font is not used.
    let mut s = session("F2");
    s.fill_text_field("t", "Hi").unwrap();
    assert!(
        matches!(ap_font(&s, b"F2"), Object::Dict(_)),
        "a re-encoded font was bound to WinAnsi bytes"
    );
}

#[test]
fn a_standard_14_dr_font_is_unchanged() {
    let mut s = session("Helv");
    s.fill_text_field("t", "Hi").unwrap();
    assert!(matches!(ap_font(&s, b"Helv"), Object::Dict(_)));
}

/// The push-button route redraws through its own builder; it binds too.
#[test]
fn a_push_button_caption_draws_with_the_dr_font() {
    let mut s = session("F1");
    s.add_push_button(
        &NewPushButton::new(
            0,
            "Go",
            Rect {
                llx: 20.0,
                lly: 100.0,
                urx: 120.0,
                ury: 124.0,
            },
            "Send",
        )
        .declining_tooltip(),
    )
    .unwrap();
    s.edit_field(
        "Go",
        &FieldEdit::new().with_appearance(FieldAppearance::resource(
            "F1",
            12.0,
            TextColor::Gray(0.0),
        )),
    )
    .unwrap();
    let g = s.graph();
    let button = forms::parse_acroform(&g)
        .unwrap()
        .fields
        .into_iter()
        .find(|f| f.fully_qualified_name == "Go")
        .unwrap();
    let ap = g
        .resolved(button.widgets[0].id)
        .as_dict()
        .and_then(|d| d.get(b"AP"))
        .map(|o| g.resolve(o).clone())
        .expect("/AP");
    let Some(Object::Stream(n)) = ap
        .as_dict()
        .and_then(|d| d.get(b"N"))
        .map(|o| g.resolve(o).clone())
    else {
        panic!("no /AP /N stream");
    };
    let font = n
        .dict
        .get(b"Resources")
        .and_then(Object::as_dict)
        .and_then(|r| r.get(b"Font"))
        .and_then(Object::as_dict)
        .and_then(|f| f.get(b"F1"))
        .cloned();
    assert_eq!(font, Some(Object::Reference(ObjId::new(5, 0))));
}

/// The first `Tm` x origin in the field's `/AP` `/N`.
fn first_x(s: &EditSession) -> f64 {
    first_tm(s, 4)
}

/// Operand `i` of the first `Tm` in the field's `/AP` `/N`.
fn first_tm(s: &EditSession, i: usize) -> f64 {
    let g = s.graph();
    let Object::Dict(field) = g.resolve(&Object::Reference(ObjId::new(4, 0))).clone() else {
        panic!("field is not a dictionary");
    };
    let n = field
        .get(b"AP")
        .map(|o| g.resolve(o))
        .and_then(Object::as_dict)
        .and_then(|ap| ap.get(b"N"))
        .map(|o| g.resolve(o).clone());
    let Some(Object::Stream(n)) = n else {
        panic!("no /AP /N stream");
    };
    let body =
        String::from_utf8_lossy(s.view().slice(n.data_span).unwrap_or_default()).into_owned();
    let tm = body
        .lines()
        .find(|l| l.ends_with(" Tm"))
        .unwrap_or_else(|| panic!("no Tm in {body}"));
    tm.split(' ').nth(i).unwrap().parse().unwrap()
}

/// A bound font is also measured with its own `/Widths`: centring "HA" in
/// the 180 pt box uses H = 1000 (`/Widths`) and A = 250 (`/MissingWidth`,
/// A lying outside `FirstChar..=LastChar`), not Helvetica's 722 + 667.
#[test]
fn a_bound_font_is_laid_out_with_its_own_widths() {
    let mut s = session("F1");
    s.fill_text_field("t", "HA").unwrap();
    let x = first_x(&s);
    let want = (180.0 - 12.0 * 1.25) / 2.0;
    assert!(
        (x - want).abs() < 1e-6,
        "centred at {x}, want {want}: measured with Helvetica, not /Widths"
    );
}

/// A bound font's first baseline sits its own `/Ascent` (900) below the
/// padded box top — 22 - 2 - 10.8 — not Helvetica's 718.
#[test]
fn a_bound_font_places_its_baseline_with_its_own_ascent() {
    let mut s = session("F1");
    s.fill_text_field("t", "HA").unwrap();
    let y = first_tm(&s, 5);
    let want = 22.0 - 2.0 - 900.0 * 12.0 / 1000.0;
    assert!(
        (y - want).abs() < 1e-6,
        "baseline at {y}, want {want}: Helvetica's ascent, not /Ascent"
    );
}

/// The shown string of the first `Tj` in the field's `/AP` `/N`.
fn first_tj(s: &EditSession) -> Vec<u8> {
    let g = s.graph();
    let Object::Dict(field) = g.resolve(&Object::Reference(ObjId::new(4, 0))).clone() else {
        panic!("field is not a dictionary");
    };
    let n = field
        .get(b"AP")
        .map(|o| g.resolve(o))
        .and_then(Object::as_dict)
        .and_then(|ap| ap.get(b"N"))
        .map(|o| g.resolve(o).clone());
    let Some(Object::Stream(n)) = n else {
        panic!("no /AP /N stream");
    };
    let body = s.view().slice(n.data_span).unwrap_or_default().to_vec();
    let tj = body.windows(2).position(|w| w == b"Tj").expect("a Tj");
    let close = body[..tj]
        .iter()
        .rposition(|&b| b == b')')
        .expect("a string");
    let open = body[..close]
        .iter()
        .rposition(|&b| b == b'(')
        .expect("a string");
    body[open + 1..close].to_vec()
}

/// A font whose `/Differences` moves glyphs draws with its own codes: "HA"
/// is written as the codes that show H and A in THIS font (65, 72) and
/// measured by them (H = 500, A = 1000).
#[test]
fn a_font_with_differences_and_widths_draws_with_its_own_codes() {
    let mut s = session("F3");
    s.fill_text_field("t", "HA").unwrap();
    assert_eq!(ap_font(&s, b"F3"), Object::Reference(ObjId::new(8, 0)));
    assert_eq!(
        first_tj(&s),
        b"AH",
        "WinAnsi codes written into a re-encoded font"
    );
    let want = (180.0 - 12.0 * 1.5) / 2.0;
    assert!((first_x(&s) - want).abs() < 1e-6, "{}", first_x(&s));
}

/// A MacRoman font: an e-acute is written as MacRoman's 0x8E, not WinAnsi's
/// 0xE9.
#[test]
fn a_mac_roman_font_draws_with_mac_roman_codes() {
    let mut s = session("F4");
    s.fill_text_field("t", "\u{e9}").unwrap();
    assert_eq!(ap_font(&s, b"F4"), Object::Reference(ObjId::new(9, 0)));
    let shown = first_tj(&s);
    assert!(
        shown == [0x8E] || shown == b"\\216",
        "{shown:?}: not MacRoman's code for e-acute"
    );
}

/// A symbolic font ignores `/Encoding` (§9.6.6.4), so its codes cannot be
/// read from objects; it keeps the stand-in.
#[test]
fn a_symbolic_font_keeps_the_standard_14_stand_in() {
    let mut s = session("F5");
    s.fill_text_field("t", "HA").unwrap();
    assert!(matches!(ap_font(&s, b"F5"), Object::Dict(_)));
}
