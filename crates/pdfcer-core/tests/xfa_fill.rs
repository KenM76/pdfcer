//! `EditSession::fill_xfa_field`: an XFA field's value written back into
//! the `datasets` packet, read back through `merge_data`, undone, and saved
//! incrementally (Pass 647.3d).

#![allow(clippy::unwrap_used, clippy::indexing_slicing)] // a panic here is the test failing

use std::io::Write as _;

use pdfcer_core::document::Document;
use pdfcer_core::edit::{CommandKind, EditError, EditSession};
use pdfcer_core::writer::SaveOptions;
use pdfcer_core::xfa::{
    DataCreation, MergeOptions, MergedForm, XfaFillError, merge_data, read_template, read_xdp,
};

const XDP: &str = r#"<xdp:xdp xmlns:xdp="http://ns.adobe.com/xdp/"><template xmlns="http://www.xfa.org/schema/xfa-template/3.3/"><subform name="form1"><field name="a"/><field name="b"/><field name="c"/></subform></template><xfa:datasets xmlns:xfa="http://www.xfa.org/schema/xfa-data/1.0/"><xfa:data><form1 c="7"><a>old</a><b/></form1></xfa:data></xfa:datasets></xdp:xdp>"#;

/// A one-page PDF whose `/AcroForm /XFA` is `xfa`, with `streams` as
/// objects 4 onward (each `(dict extras, data)`).
fn pdf(xfa: &str, streams: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut objects: Vec<Vec<u8>> = vec![
        format!("<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [] /XFA {xfa} >> >>")
            .into_bytes(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] >>".to_vec(),
    ];
    for (extra, data) in streams {
        let mut o = format!("<< /Length {} {extra} >>\nstream\n", data.len()).into_bytes();
        o.extend_from_slice(data);
        o.extend_from_slice(b"\nendstream");
        objects.push(o);
    }
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for o in offsets {
        out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
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

fn zlib(data: &[u8]) -> Vec<u8> {
    let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    e.write_all(data).unwrap();
    e.finish().unwrap()
}

fn merged(doc: &Document) -> MergedForm {
    let xdp = read_xdp(&doc.view()).unwrap().unwrap();
    let template = read_template(&xdp).unwrap().unwrap();
    merge_data(&template, &xdp, &MergeOptions::new()).unwrap()
}

/// The template path of the field that bound `value`.
fn path_of(form: &MergedForm, value: &str) -> Vec<usize> {
    form.bound
        .iter()
        .find(|b| b.value == value)
        .unwrap()
        .path
        .clone()
}

fn session(bytes: Vec<u8>) -> EditSession {
    EditSession::new(Document::from_bytes(bytes).unwrap())
}

fn saved(s: &EditSession) -> Vec<u8> {
    s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0
}

fn values(bytes: Vec<u8>) -> Vec<String> {
    let doc = Document::from_bytes(bytes).unwrap();
    merged(&doc).bound.into_iter().map(|b| b.value).collect()
}

#[test]
fn a_filled_value_reads_back_after_save_and_undo_restores_it() {
    let original = pdf("4 0 R", &[("", XDP.as_bytes().to_vec())]);
    let mut s = session(original.clone());
    let form = merged(&Document::from_bytes(original.clone()).unwrap());
    let a = path_of(&form, "old");
    let out = s
        .fill_xfa_field(&a, "new <&> \"q\"", &MergeOptions::new())
        .unwrap();
    assert_eq!(out.stream.num, 4);
    assert!(out.shared_with.is_empty());

    let bytes = saved(&s);
    assert!(bytes.starts_with(&original), "an incremental save appends");
    assert_eq!(values(bytes), ["new <&> \"q\"", "", "7"]);

    assert_eq!(s.undo(), Some(CommandKind::FillXfaField));
    assert_eq!(values(saved(&s)), ["old", "", "7"]);
}

#[test]
fn an_empty_element_and_an_attribute_take_values() {
    let original = pdf("4 0 R", &[("", XDP.as_bytes().to_vec())]);
    let mut s = session(original.clone());
    let form = merged(&Document::from_bytes(original).unwrap());
    let opts = MergeOptions::new();
    let b = form
        .bound
        .iter()
        .find(|v| v.value.is_empty())
        .unwrap()
        .path
        .clone();
    let c = path_of(&form, "7");
    s.fill_xfa_field(&b, "bee", &opts).unwrap();
    s.fill_xfa_field(&c, "8\n9", &opts).unwrap();
    assert_eq!(values(saved(&s)), ["old", "bee", "8\n9"]);
}

#[test]
fn a_flate_split_xdp_with_a_bom_keeps_both() {
    let at = XDP.find("<xfa:datasets").unwrap();
    let mut head = b"\xEF\xBB\xBF".to_vec();
    head.extend_from_slice(&XDP.as_bytes()[..at]);
    let tail = zlib(&XDP.as_bytes()[at..]);
    let original = pdf(
        "[(template) 4 0 R (datasets) 5 0 R]",
        &[("", head), ("/Filter /FlateDecode", tail)],
    );
    let mut s = session(original.clone());
    let form = merged(&Document::from_bytes(original).unwrap());
    let out = s
        .fill_xfa_field(&path_of(&form, "old"), "flated", &MergeOptions::new())
        .unwrap();
    assert_eq!(out.stream.num, 5, "only the datasets stream is rewritten");
    assert_eq!(values(saved(&s)), ["flated", "", "7"]);
}

#[test]
fn a_rewritten_stream_keeps_its_bom() {
    let mut data = b"\xEF\xBB\xBF".to_vec();
    data.extend_from_slice(XDP.as_bytes());
    let original = pdf("4 0 R", &[("", data)]);
    let mut s = session(original.clone());
    let base = original.len();
    let form = merged(&Document::from_bytes(original).unwrap());
    s.fill_xfa_field(&path_of(&form, "old"), "bommed", &MergeOptions::new())
        .unwrap();
    let bytes = saved(&s);
    let appended = &bytes[base..];
    let bom_xdp = b"\xEF\xBB\xBF<xdp:xdp";
    assert!(
        appended.windows(bom_xdp.len()).any(|w| w == bom_xdp),
        "the BOM is kept"
    );
    assert_eq!(values(bytes), ["bommed", "", "7"]);
}

/// X023 Form A: `sec` is `match="none"`, so `colour` binds at the root.
#[test]
fn a_value_under_an_unbound_subform_saves_to_its_own_data_node() {
    const FORM_A: &str = r#"<xdp:xdp xmlns:xdp="http://ns.adobe.com/xdp/"><template><subform name="form"><subform name="sec"><bind match="none"/><field name="colour"/><subform name="inner"><field name="pet"/></subform></subform><subform name="bound"><field name="town"/></subform></subform></template><xfa:datasets xmlns:xfa="http://www.xfa.org/schema/xfa-data/1.0/"><xfa:data><form><colour>Teal</colour><inner><pet>Biscuit</pet></inner><bound><town>Springfield</town></bound></form></xfa:data></xfa:datasets></xdp:xdp>"#;
    let original = pdf("4 0 R", &[("", FORM_A.as_bytes().to_vec())]);
    let mut s = session(original.clone());
    let base = original.len();
    let form = merged(&Document::from_bytes(original).unwrap());
    s.fill_xfa_field(&path_of(&form, "Teal"), "Red", &MergeOptions::new())
        .unwrap();
    let bytes = saved(&s);
    let appended = String::from_utf8_lossy(&bytes[base..]).into_owned();
    assert!(
        appended.contains("<xfa:data><form><colour>Red</colour><inner>"),
        "{appended}"
    );
    assert!(!appended.contains("<sec"), "no sec data group was added");
    assert_eq!(values(bytes), ["Red", "Biscuit", "Springfield"]);
}

#[test]
fn refusals_are_named() {
    let original = pdf("4 0 R", &[("", XDP.as_bytes().to_vec())]);
    let mut s = session(original);
    let err = s
        .fill_xfa_field(&[0, 99], "x", &MergeOptions::new())
        .unwrap_err();
    assert!(matches!(err, EditError::XfaFill(XfaFillError::Unbound)));

    let mut plain = session(pdf("[]", &[]));
    let err = plain
        .fill_xfa_field(&[0], "x", &MergeOptions::new())
        .unwrap_err();
    assert!(matches!(err, EditError::XfaFill(_)), "{err:?}");
}

/// X022 Form G: a global `userName` on page 1 and a protected copy later,
/// with no data at all.
const FORM_G: &str = r#"<xdp:xdp xmlns:xdp="http://ns.adobe.com/xdp/"><template xmlns="http://www.xfa.org/schema/xfa-template/3.3/"><subform name="form1"><subform name="page1"><field name="userName"><bind match="global"/></field></subform><subform name="later"><field name="userName" access="protected"><bind match="global"/></field></subform></subform></template></xdp:xdp>"#;

#[test]
fn a_global_field_with_no_data_creates_one_shared_node() {
    let original = pdf("4 0 R", &[("", FORM_G.as_bytes().to_vec())]);
    let mut s = session(original.clone());
    let opts = MergeOptions::new();
    let out = s.fill_xfa_field(&[0, 0, 0], "A. Tester", &opts).unwrap();
    assert_eq!(out.created, ["form1", "userName"]);
    assert_eq!(out.shared_with, [vec![0, 1, 0]]);
    let bytes = saved(&s);
    assert_eq!(values(bytes.clone()), ["A. Tester", "A. Tester"]);
    let form = merged(&Document::from_bytes(bytes.clone()).unwrap());
    assert_eq!(form.bound[0].data, form.bound[1].data, "one node");

    let mut s = session(bytes);
    let out = s.fill_xfa_field(&[0, 1, 0], "B", &opts).unwrap();
    assert!(out.created.is_empty());
    let bytes = saved(&s);
    let appended = String::from_utf8_lossy(&bytes).into_owned();
    assert_eq!(appended.matches("<userName>B</userName>").count(), 1);
    assert_eq!(values(bytes), ["B", "B"]);

    let mut fresh = session(original);
    fresh.fill_xfa_field(&[0, 0, 0], "x", &opts).unwrap();
    assert_eq!(fresh.undo(), Some(CommandKind::FillXfaField));
    assert_eq!(values(saved(&fresh)), Vec::<String>::new());
}

#[test]
fn an_unmatched_subform_gets_its_data_group_inserted() {
    const XDP: &str = r#"<xdp:xdp xmlns:xdp="http://ns.adobe.com/xdp/"><template><subform name="form1"><field name="a"/><subform name="sec"><field name="x"/></subform></subform></template><xfa:datasets xmlns:xfa="http://www.xfa.org/schema/xfa-data/1.0/"><xfa:data><form1><a>1</a></form1></xfa:data></xfa:datasets></xdp:xdp>"#;
    let original = pdf("4 0 R", &[("", XDP.as_bytes().to_vec())]);
    let base = original.len();
    let mut s = session(original.clone());
    let refuse = MergeOptions::new().with_data_creation(DataCreation::Refuse);
    let err = s.fill_xfa_field(&[0, 1, 0], "v", &refuse).unwrap_err();
    assert!(matches!(err, EditError::XfaFill(XfaFillError::Unbound)));

    let out = s
        .fill_xfa_field(&[0, 1, 0], "v & w", &MergeOptions::new())
        .unwrap();
    assert_eq!(out.created, ["sec", "x"]);
    let bytes = saved(&s);
    let appended = String::from_utf8_lossy(&bytes[base..]).into_owned();
    assert!(
        appended.contains("<form1><a>1</a><sec><x>v &amp; w</x></sec></form1>"),
        "{appended}"
    );
    assert_eq!(values(bytes), ["1", "v & w"]);
}

#[test]
fn a_packet_array_with_no_datasets_stream_is_refused() {
    let at = FORM_G.find("</xdp:xdp>").unwrap();
    let original = pdf(
        "[(template) 4 0 R (</xdp:xdp>) 5 0 R]",
        &[
            ("", FORM_G.as_bytes()[..at].to_vec()),
            ("", b"</xdp:xdp>".to_vec()),
        ],
    );
    let mut s = session(original);
    let err = s
        .fill_xfa_field(&[0, 0, 0], "x", &MergeOptions::new())
        .unwrap_err();
    assert!(
        matches!(err, EditError::XfaFill(XfaFillError::NoDatasetsPacket)),
        "{err:?}"
    );
}
