//! The Ink Manager: `prepress::inks`, `EditSession::alias_ink` and
//! `EditSession::spot_to_process` over synthetic fixtures (ISO 32000-2
//! §8.6.6.4, §8.6.6.5, §8.6.7). Every edit assertion reads the session's
//! view or its saved bytes.

use pdfcer_core::document::Document;
use pdfcer_core::edit::{CommandKind, EditError, EditSession};
use pdfcer_core::object::{ObjId, Object};
use pdfcer_core::preflight::{self, Check, Profile, ProfileRule, Severity};
use pdfcer_core::prepress::{InkKind, InkSelection, inks};
use pdfcer_core::writer::SaveOptions;

/// An offset-consistent classic PDF from `(number, body)` pairs.
fn build(objects: &[(u32, String)]) -> Vec<u8> {
    let mut buf = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets: Vec<(u32, usize)> = Vec::new();
    for (num, body) in objects {
        offsets.push((*num, buf.len()));
        buf.extend_from_slice(format!("{num} 0 obj\n{body}\nendobj\n").as_bytes());
    }
    let xref_at = buf.len();
    let max_num = objects.iter().map(|(n, _)| *n).max().unwrap_or(0);
    buf.extend_from_slice(format!("xref\n0 {}\n", max_num + 1).as_bytes());
    buf.extend_from_slice(b"0000000000 65535 f \n");
    for num in 1..=max_num {
        match offsets.iter().find(|(n, _)| *n == num) {
            Some((_, off)) => buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes()),
            None => buf.extend_from_slice(b"0000000000 65535 f \n"),
        }
    }
    buf.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R /ID [<0102> <0304>] >>\nstartxref\n{xref_at}\n%%EOF\n",
            max_num + 1
        )
        .as_bytes(),
    );
    buf
}

fn stream(dict: &str, data: &[u8]) -> String {
    format!(
        "<< {dict} /Length {} >>\nstream\n{}\nendstream",
        data.len(),
        String::from_utf8_lossy(data)
    )
}

/// One page: `/S1` is Gold (alternate DeviceCMYK, object 7), `/D1` a
/// DeviceN of Gold and Black, `/R1` Rust (alternate DeviceRGB), `/I1` an
/// Indexed palette over Gold, `/Im` a 2x1 Gold image (hex, so the fixture stays text); `/G0` turns
/// overprint on in mode 0. Object 9 is untouched by every edit.
fn fixture(content: &str) -> Vec<u8> {
    let resources = "<< /ColorSpace << /S1 7 0 R /D1 [/DeviceN [/Gold /Black] /DeviceCMYK 6 0 R] \
                     /R1 [/Separation /Rust /DeviceRGB 8 0 R] /I1 11 0 R >> \
                     /ExtGState << /G0 << /OP true /OPM 0 >> >> /XObject << /Im 10 0 R >> >>";
    let type4 = b"{ exch pop 0 0 0 4 3 roll }";
    build(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>".into()),
        (
            2,
            "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 100 100] >>".into(),
        ),
        (
            3,
            format!(
                "<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources {resources} /Extra 9 0 R >>"
            ),
        ),
        (4, stream("", content.as_bytes())),
        (
            5,
            "<< /FunctionType 2 /Domain [0 1] /C0 [0 0 0 0] /C1 [0 0.2 0.8 0.1] /N 1 >>".into(),
        ),
        (
            6,
            stream(
                "/FunctionType 4 /Domain [0 1 0 1] /Range [0 1 0 1 0 1 0 1]",
                type4,
            ),
        ),
        (7, "[/Separation /Gold /DeviceCMYK 5 0 R]".into()),
        (
            8,
            "<< /FunctionType 2 /Domain [0 1] /C0 [1 1 1] /C1 [0.5 0.2 0.1] /N 1 >>".into(),
        ),
        (9, "<< /Note (untouched) >>".into()),
        (
            10,
            stream(
                "/Type /XObject /Subtype /Image /Width 2 /Height 1 /BitsPerComponent 8 /ColorSpace 7 0 R /Filter /ASCIIHexDecode",
                b"00FF>",
            ),
        ),
        (11, "[/Indexed 7 0 R 1 <00FF>]".into()),
    ])
}

const CONTENT: &str = "/S1 cs 0.5 scn 0 0 10 10 re f /D1 CS 0.2 0.3 SCN 0 0 5 5 re S /Im Do";

fn session(content: &str) -> EditSession {
    EditSession::new(Document::from_bytes(fixture(content)).expect("fixture loads"))
}

fn decoded(session: &EditSession, num: u32) -> Vec<u8> {
    let view = session.view();
    let Some(Object::Stream(s)) = view.graph().value(ObjId::new(num, 0)) else {
        panic!("object {num} is not a stream");
    };
    let raw = view.slice(s.data_span).expect("stream data");
    pdfcer_core::filters::decode_stream(&s.dict, raw).expect("decodes")
}

fn value(session: &EditSession, num: u32) -> Object {
    session
        .view()
        .graph()
        .value(ObjId::new(num, 0))
        .cloned()
        .expect("object exists")
}

#[test]
fn inks_lists_process_then_spots_with_alternates_and_pages() {
    let doc = Document::from_bytes(fixture(CONTENT)).expect("fixture loads");
    let listed = inks(&doc.view());
    let names: Vec<&str> = listed.iter().map(|u| u.name.as_str()).collect();
    assert_eq!(names, ["Black", "Gold", "Rust"]);
    assert_eq!(listed[0].kind, InkKind::Process);
    let gold = &listed[1];
    assert_eq!(gold.kind, InkKind::Spot);
    assert_eq!(gold.alternates, ["DeviceCMYK"]);
    assert_eq!(gold.pages, [1]);
    assert!(gold.uses >= 2, "{gold:?}");
    let rust = &listed[2];
    assert_eq!(rust.alternates, ["DeviceRGB"]);
    assert!(rust.pages.is_empty(), "Rust is defined but never drawn");
}

#[test]
fn alias_renames_every_space_and_refuses_a_devicen_naming_both() {
    let mut s = session(CONTENT);
    let report = s.alias_ink("Gold", "Black").expect("alias runs");
    assert_eq!(report.spaces, 1, "{report:?}");
    assert_eq!(report.refused.len(), 1, "{report:?}");
    assert!(report.refused[0].reason.contains("already names"));
    assert_eq!(
        value(&s, 7),
        Object::Array(vec![
            Object::Name(b"Separation".into()),
            Object::Name(b"Black".into()),
            Object::Name(b"DeviceCMYK".into()),
            Object::Reference(ObjId::new(5, 0)),
        ])
    );
    assert_eq!(s.undo_kind(), Some(CommandKind::AliasInk));
}

#[test]
fn alias_resolves_a_spot_not_allowed_finding() {
    let profile = Profile {
        name: "t".into(),
        description: String::new(),
        rules: vec![ProfileRule::new(
            "spot",
            Check::SpotNotAllowed {
                allowed: vec!["Silver".into(), "Rust".into()],
            },
            Severity::Error,
        )],
    };
    let mut s = session("/S1 cs 1 scn 0 0 1 1 re f");
    assert_eq!(preflight::run(&s.view(), &profile).findings.len(), 1);
    let report = s.alias_ink("Gold", "Silver").expect("alias runs");
    assert!(report.refused.is_empty(), "{report:?}");
    let after = preflight::run(&s.view(), &profile);
    assert!(after.findings.is_empty(), "{:?}", after.findings);
}

#[test]
fn alias_save_leaves_untouched_objects_byte_identical() {
    let source = fixture(CONTENT);
    let mut s = EditSession::new(Document::from_bytes(source.clone()).expect("loads"));
    let report = s.alias_ink("Gold", "Silver").expect("alias runs");
    assert!(
        !report.changed.contains(&ObjId::new(4, 0)),
        "content is not rewritten"
    );
    assert!(!report.changed.contains(&ObjId::new(9, 0)));
    let (inc, _) = s
        .to_incremental_bytes(&SaveOptions::identity())
        .expect("saves");
    assert!(inc.starts_with(&source), "incremental save appends only");
    let appended = &inc[source.len()..];
    assert!(
        !contains(appended, b"\n9 0 obj"),
        "untouched object 9 was re-emitted"
    );
    let (full, _) = s.to_full_bytes(&SaveOptions::identity()).expect("saves");
    for num in [4, 5, 6, 9, 10] {
        let original = object_bytes(&source, num);
        assert!(
            contains(&full, original),
            "object {num} differs in the full rewrite"
        );
    }
}

#[test]
fn alias_refuses_names_it_cannot_alias() {
    let mut s = session(CONTENT);
    for (from, to) in [
        ("Black", "Gold"),
        ("All", "Gold"),
        ("Gold", "None"),
        ("Gold", "Gold"),
        ("", "X"),
    ] {
        assert!(
            matches!(s.alias_ink(from, to), Err(EditError::InkNameRefused { .. })),
            "{from} -> {to}"
        );
    }
    assert!(!s.can_undo());
}

#[test]
fn spot_to_process_rewrites_content_images_and_palettes() {
    let mut s = session(CONTENT);
    let report = s.spot_to_process(&InkSelection::All).expect("converts");
    assert_eq!(report.images, 1, "{report:?}");
    assert_eq!(report.lookups, 1, "{report:?}");
    assert!(report.paints >= 4, "{report:?}");
    let content = String::from_utf8(decoded(&s, 4)).expect("ascii");
    assert!(
        !content.contains("scn") && !content.contains("SCN"),
        "{content}"
    );
    assert!(
        content.contains(" k ") && content.contains(" K "),
        "{content}"
    );
    assert!(
        content.contains("0.4"),
        "half Gold is 0 0.1 0.4 0.05: {content}"
    );
    assert_eq!(decoded(&s, 10), [0, 0, 0, 0, 0, 51, 204, 26]);
    let Object::Array(indexed) = value(&s, 11) else {
        panic!("indexed space")
    };
    assert_eq!(indexed[1], Object::Name(b"DeviceCMYK".into()));
    assert_eq!(indexed[3], Object::String(vec![0, 0, 0, 0, 0, 51, 204, 26]));
    assert_eq!(
        value(&s, 7),
        value_of_source(7),
        "the Separation array stays"
    );
    assert!(report.refused.is_empty(), "Rust is never drawn: {report:?}");
    assert_eq!(s.undo(), Some(CommandKind::SpotToProcess));
    assert_eq!(value(&s, 11), value_of_source(11), "undo restores");
}

#[test]
fn spot_to_process_refuses_a_non_cmyk_alternate() {
    let mut s = session("/R1 cs 1 scn 0 0 1 1 re f");
    let report = s
        .spot_to_process(&InkSelection::Only(vec!["Rust".into(), "Nope".into()]))
        .expect("runs");
    assert!(report.changed.is_empty(), "{report:?}");
    assert!(!s.can_undo(), "nothing committed");
    assert_eq!(report.refused.len(), 1, "{report:?}");
    assert!(report.refused[0].reason.contains("icccer"));
    assert!(report.disclosures.iter().any(|d| d.contains("\"Nope\"")));
}

#[test]
fn spot_to_process_reports_overprint_lost_under_mode_zero() {
    let mut s = session("/G0 gs /S1 cs 1 scn 0 0 1 1 re f");
    let report = s.spot_to_process(&InkSelection::All).expect("converts");
    assert_eq!(report.overprint_lost, [ObjId::new(4, 0)], "{report:?}");
    let mut s = session("/S1 cs 1 scn 0 0 1 1 re f");
    let report = s.spot_to_process(&InkSelection::All).expect("converts");
    assert!(report.overprint_lost.is_empty(), "{report:?}");
}

fn value_of_source(num: u32) -> Object {
    let doc = Document::from_bytes(fixture(CONTENT)).expect("loads");
    doc.view()
        .graph()
        .value(ObjId::new(num, 0))
        .cloned()
        .expect("exists")
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

/// `N 0 obj ... endobj` as written in `pdf`.
fn object_bytes(pdf: &[u8], num: u32) -> &[u8] {
    let head = format!("{num} 0 obj\n");
    let start = pdf
        .windows(head.len())
        .position(|w| w == head.as_bytes())
        .expect("object present");
    let end = start
        + pdf[start..]
            .windows(6)
            .position(|w| w == b"endobj")
            .expect("endobj")
        + 6;
    &pdf[start..end]
}
