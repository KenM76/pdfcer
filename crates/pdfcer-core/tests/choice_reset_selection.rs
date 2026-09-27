//! `reset_form` on a list box restores `/V` from `/DV` AND re-derives `/I`
//! and the appearance from it (ISO 32000-1 §12.7.4.4 Table 231: `/I` holds
//! the sorted indices of the selected options and must agree with `/V`).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::forms;
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

/// A multi-select list box "L" over a, b, c, holding `v_and_i` and
/// defaulting to `dv`.
fn session(v_and_i: &str, dv: &str) -> EditSession {
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [4 0 R] \
         /DA (/Helv 10 Tf 0 g) /DR << /Font << /Helv 5 0 R >> >> >> >>"
            .to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] /Annots [4 0 R] >>".to_owned(),
        format!(
            "<< /FT /Ch /Ff 2097152 /T (L) /Type /Annot /Subtype /Widget /P 3 0 R \
             /Rect [20 50 200 110] /Opt [(a) (b) (c)] {v_and_i} {dv} >>"
        ),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica \
         /Encoding /WinAnsiEncoding >>"
            .to_owned(),
    ];
    EditSession::new(Document::from_bytes(assemble(&bodies)).expect("fixture parses"))
}

fn field(s: &EditSession) -> forms::Field {
    forms::parse_acroform(&s.graph())
        .unwrap()
        .fields
        .into_iter()
        .find(|f| f.fully_qualified_name == "L")
        .unwrap()
}

fn dict(s: &EditSession) -> Dict {
    s.graph().resolved(field(s).id).as_dict().cloned().unwrap()
}

fn indices(d: &Dict) -> Option<Vec<i64>> {
    match d.get(b"I") {
        Some(Object::Array(a)) => Some(a.iter().filter_map(Object::as_int).collect()),
        None => None,
        other => panic!("/I is {other:?}"),
    }
}

fn normal_ap(s: &EditSession) -> Vec<u8> {
    let g = s.graph();
    let n = dict(s)
        .get(b"AP")
        .map(|o| g.resolve(o).clone())
        .and_then(|ap| ap.as_dict().and_then(|d| d.get(b"N")).cloned())
        .map(|o| g.resolve(&o).clone());
    let Some(Object::Stream(st)) = n else {
        panic!("no /AP /N stream: {n:?}");
    };
    s.view().slice(st.data_span).unwrap().to_vec()
}

#[test]
fn a_reset_reindexes_a_multi_select_default() {
    let mut s = session("/V [(b) (c)] /I [1 2]", "/DV [(a) (c)]");
    s.reset_form(None).unwrap();
    assert_eq!(indices(&dict(&s)), Some(vec![0, 2]), "stale /I after reset");
}

#[test]
fn a_reset_to_a_single_default_reindexes_it() {
    let mut s = session("/V [(b) (c)] /I [1 2]", "/DV (a)");
    s.reset_form(None).unwrap();
    assert_eq!(indices(&dict(&s)), Some(vec![0]), "stale /I after reset");
}

#[test]
fn a_reset_draws_the_default_selection() {
    let mut reset = session("/V [(b) (c)] /I [1 2]", "/DV [(a) (c)]");
    reset.reset_form(None).unwrap();
    let mut chosen = session("/V [(b) (c)] /I [1 2]", "");
    chosen.set_choice_value("L", &["a", "c"]).unwrap();
    assert_eq!(
        String::from_utf8_lossy(&normal_ap(&reset)),
        String::from_utf8_lossy(&normal_ap(&chosen)),
        "the reset drew a selection other than its default"
    );
}
