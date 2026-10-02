//! Border edits on widgets whose appearance pdfcer did not author: an
//! unsigned signature field is redrawn as an empty box (a signed one is
//! left to its signer), and a foreign check box's artwork is replaced only
//! under `WidgetEdit::with_replace_foreign_appearance`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::{
    AppearanceOutcome, BorderSpec, BorderStyle, EditSession, WidgetEdit, WidgetEditOutcome,
};
use pdfcer_core::forms::{self, MkColor};
use pdfcer_core::graph::ObjectGraph as _;
use pdfcer_core::object::{Dict, Object};

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

fn stream(content: &str) -> String {
    format!(
        "<< /Type /XObject /Subtype /Form /BBox [0 0 20 20] /Length {} >>\nstream\n{content}\nendstream",
        content.len()
    )
}

/// Objects 1–5: catalog, pages, page, the field/widget `widget` as object 4,
/// Helvetica. `extra` follows as objects 6 onward.
fn session(widget: &str, extra: &[String]) -> EditSession {
    let mut bodies = vec![
        "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [4 0 R] \
         /DA (/Helv 10 Tf 0 g) /DR << /Font << /Helv 5 0 R >> >> >> >>"
            .to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] /Annots [4 0 R] >>".to_owned(),
        widget.to_owned(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"
            .to_owned(),
    ];
    bodies.extend_from_slice(extra);
    EditSession::new(Document::from_bytes(assemble(&bodies)).expect("fixture parses"))
}

fn sig_session(signed: bool) -> EditSession {
    let v = if signed { "/V 7 0 R " } else { "" };
    let widget = format!(
        "<< /FT /Sig /T (sig) {v}/Type /Annot /Subtype /Widget /P 3 0 R /F 4 \
         /Rect [20 50 200 92] /BS << /S /S /W 1 >> /MK << /BC [0 0 0] >> /AP << /N 6 0 R >> >>"
    );
    let extra = [
        stream("0 0 0 RG 1 w 0.5 0.5 179 41 re S"),
        "<< /Type /Sig /Filter /Adobe.PPKLite /SubFilter /adbe.pkcs7.detached \
         /ByteRange [0 0 0 0] /Contents <00> >>"
            .to_owned(),
    ];
    session(&widget, &extra)
}

/// A check box whose artwork another producer drew: a heavy blue frame in
/// both states, a filled square for on, and a `/D` down appearance.
fn foreign_check_box() -> EditSession {
    let widget = "<< /FT /Btn /T (cb) /V /Off /AS /Off /Type /Annot /Subtype /Widget \
                  /P 3 0 R /F 4 /Rect [20 50 40 70] /MK << /BC [0 0 0] /CA (4) >> \
                  /AP << /N << /Off 6 0 R /Yes 7 0 R >> /D << /Off 8 0 R /Yes 8 0 R >> >> >>";
    let extra = [
        stream("0 0 1 RG 3 w 1.5 1.5 17 17 re S"),
        stream("0 0 1 RG 3 w 1.5 1.5 17 17 re S 0 g 5 5 10 10 re f"),
        stream("0.5 g 0 0 20 20 re f"),
    ];
    session(widget, &extra)
}

fn widget_dict(s: &EditSession) -> Dict {
    s.graph()
        .resolved(pdfcer_core::object::ObjId::new(4, 0))
        .as_dict()
        .cloned()
        .unwrap()
}

/// The content bytes of every `/AP /N` state (or the single stream), keyed
/// by state name (`""` for a single stream).
fn normal_states(s: &EditSession) -> Vec<(Vec<u8>, Vec<u8>)> {
    let g = s.graph();
    let ap = widget_dict(s).get(b"AP").map(|o| g.resolve(o).clone());
    let n = ap
        .as_ref()
        .and_then(Object::as_dict)
        .and_then(|d| d.get(b"N"))
        .unwrap()
        .clone();
    let bytes = |o: &Object| match g.resolve(o) {
        Object::Stream(st) => s.view().slice(st.data_span).unwrap().to_vec(),
        other => panic!("not a stream: {other:?}"),
    };
    match g.resolve(&n) {
        Object::Dict(states) => states
            .0
            .iter()
            .map(|(k, v)| (k.0.clone(), bytes(v)))
            .collect(),
        _ => vec![(Vec::new(), bytes(&n))],
    }
}

/// Every content-stream operator token (a crude lexer: enough for the
/// path and colour operators these streams use).
fn ops(content: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(content)
        .split_whitespace()
        .filter(|t| t.starts_with(|c: char| c.is_ascii_alphabetic()) || *t == "'")
        .map(str::to_owned)
        .collect()
}

fn strokes(content: &[u8]) -> bool {
    ops(content)
        .iter()
        .any(|t| matches!(t.as_str(), "S" | "s" | "B" | "B*" | "b" | "b*"))
}

/// Whether a `re` rectangle is ever painted by a stroking operator — a frame.
fn strokes_a_rect(content: &[u8]) -> bool {
    let ops = ops(content);
    ops.iter().enumerate().any(|(i, t)| {
        t == "re"
            && ops[i + 1..]
                .iter()
                .find(|o| {
                    matches!(
                        o.as_str(),
                        "S" | "s" | "f" | "F" | "f*" | "B" | "B*" | "b" | "b*" | "n"
                    )
                })
                .is_some_and(|o| matches!(o.as_str(), "S" | "s" | "B" | "B*" | "b" | "b*"))
    })
}

fn no_border() -> WidgetEdit {
    WidgetEdit::new()
        .with_border(BorderSpec {
            style: BorderStyle::Solid,
            width: 0.0,
        })
        .without_border_color()
}

fn show(content: &[u8]) -> String {
    String::from_utf8_lossy(content).into_owned()
}

#[test]
fn an_unsigned_signature_field_loses_its_border() {
    let mut s = sig_session(false);
    let out = s.edit_widget("sig", 0, &no_border()).unwrap();
    assert_eq!(out.appearance, AppearanceOutcome::Regenerated);
    let states = normal_states(&s);
    assert_eq!(states.len(), 1);
    assert!(!strokes(&states[0].1), "{}", show(&states[0].1));
}

#[test]
fn an_unsigned_signature_field_takes_a_two_point_red_border() {
    let mut s = sig_session(false);
    let edit = WidgetEdit::new()
        .with_border(BorderSpec {
            style: BorderStyle::Solid,
            width: 2.0,
        })
        .with_border_color(MkColor::Rgb(1.0, 0.0, 0.0));
    let out = s.edit_widget("sig", 0, &edit).unwrap();
    assert_eq!(out.appearance, AppearanceOutcome::Regenerated);
    let body = show(&normal_states(&s)[0].1);
    assert!(body.contains("1 0 0 RG"), "{body}");
    assert!(body.contains("2 w"), "{body}");
    assert!(strokes_a_rect(body.as_bytes()), "{body}");
}

#[test]
fn a_signed_signature_field_is_recorded_not_painted() {
    let mut s = sig_session(true);
    let before = normal_states(&s);
    let ap_before = widget_dict(&s).get(b"AP").cloned();
    let out = s.edit_widget("sig", 0, &no_border()).unwrap();
    assert!(
        matches!(&out.appearance, AppearanceOutcome::RecordedNotPainted(m) if m.contains("signer")),
        "{:?}",
        out.appearance
    );
    assert_eq!(normal_states(&s), before);
    assert_eq!(widget_dict(&s).get(b"AP").cloned(), ap_before);
}

fn replaced(out: &WidgetEditOutcome) {
    assert!(out.foreign_appearance_replaced);
    assert_eq!(out.appearance, AppearanceOutcome::Regenerated);
}

#[test]
fn opting_in_replaces_a_foreign_check_box_without_its_frame() {
    let mut s = foreign_check_box();
    let edit = no_border().with_replace_foreign_appearance(true);
    let out = s.edit_widget("cb", 0, &edit).unwrap();
    replaced(&out);
    let states = normal_states(&s);
    let names: Vec<&[u8]> = states.iter().map(|(k, _)| k.as_slice()).collect();
    assert_eq!(names, [b"Off".as_slice(), b"Yes".as_slice()]);
    let (off, on) = (&states[0].1, &states[1].1);
    assert!(!strokes(off), "the off state still strokes: {}", show(off));
    assert!(
        !strokes_a_rect(on),
        "the on state still frames: {}",
        show(on)
    );
    assert!(!on.is_empty() && on != off, "the on state lost its mark");
    assert!(!show(on).contains("0 0 1 RG"), "foreign artwork survived");
    let w = widget_dict(&s);
    let ap = s.graph().resolve(w.get(b"AP").unwrap()).clone();
    assert!(
        ap.as_dict().unwrap().get(b"D").is_none(),
        "the old /D survived"
    );
    assert_eq!(w.get(b"AS").and_then(Object::as_name).unwrap().0, b"Off");
}

#[test]
fn opting_in_keeps_the_edit_the_replacement_rode_on() {
    let mut s = foreign_check_box();
    let edit = no_border().with_replace_foreign_appearance(true);
    s.edit_widget("cb", 0, &edit).unwrap();
    let w = widget_dict(&s);
    let bs = w.get(b"BS").and_then(Object::as_dict).cloned().unwrap();
    assert_eq!(bs.get(b"W").and_then(Object::as_number), Some(0.0));
    let field = forms::parse_acroform(&s.graph()).unwrap().fields;
    assert_eq!(field[0].widgets[0].on_states, [b"Yes".to_vec()]);
}

#[test]
fn without_the_opt_in_a_foreign_check_box_is_untouched() {
    let mut s = foreign_check_box();
    let before = normal_states(&s);
    let out = s.edit_widget("cb", 0, &no_border()).unwrap();
    assert!(!out.foreign_appearance_replaced);
    assert!(
        matches!(&out.appearance, AppearanceOutcome::RecordedNotPainted(m)
            if m.contains("did not draw (its /AP does not match")),
        "{:?}",
        out.appearance
    );
    assert_eq!(normal_states(&s), before);
}

#[test]
fn opting_in_on_pdfcer_artwork_is_an_ordinary_rebuild() {
    let mut s = foreign_check_box();
    let edit = no_border().with_replace_foreign_appearance(true);
    s.edit_widget("cb", 0, &edit).unwrap();
    let out = s
        .edit_widget(
            "cb",
            0,
            &WidgetEdit::new()
                .with_border_color(MkColor::Rgb(1.0, 0.0, 0.0))
                .with_border(BorderSpec::default())
                .with_replace_foreign_appearance(true),
        )
        .unwrap();
    assert!(
        !out.foreign_appearance_replaced,
        "pdfcer's own artwork is rebuilt in place, not replaced"
    );
    assert_eq!(out.appearance, AppearanceOutcome::Regenerated);
}

#[test]
fn opting_in_replaces_a_foreign_radio_button_under_its_own_state_name() {
    let widget = "<< /FT /Btn /Ff 49152 /T (r) /V /A /AS /A /Type /Annot /Subtype /Widget \
                  /P 3 0 R /F 4 /Rect [20 50 40 70] /MK << /BC [0 0 0] >> \
                  /AP << /N << /Off 6 0 R /A 7 0 R >> >> >>";
    let extra = [
        stream("0 0 1 RG 3 w 1.5 1.5 17 17 re S"),
        stream("0 0 1 RG 3 w 1.5 1.5 17 17 re S 0 g 5 5 10 10 re f"),
    ];
    let mut s = session(widget, &extra);
    let out = s
        .edit_widget("r", 0, &no_border().with_replace_foreign_appearance(true))
        .unwrap();
    replaced(&out);
    let states = normal_states(&s);
    assert_eq!(states[1].0, b"A");
    assert!(!strokes_a_rect(&states[0].1) && !strokes_a_rect(&states[1].1));
    assert!(!show(&states[1].1).contains("0 0 1 RG"));
    assert_eq!(
        widget_dict(&s)
            .get(b"AS")
            .and_then(Object::as_name)
            .unwrap()
            .0,
        b"A"
    );
}
