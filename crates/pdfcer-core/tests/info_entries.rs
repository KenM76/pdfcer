//! `/Info` entries of any name, and the XMP packet kept in step with them
//! (`Pass 660.0`, pdfcer-gui request G266). Fixtures are built here.

use pdfcer_core::document::Document;
use pdfcer_core::edit::{
    EditError, EditSession, InfoField, InfoKeyProblem, InfoValue, ReservedInfoKey, SignedPacket,
    XmpMirror, XmpSkip, XmpSync, XmpSyncOptions,
};
use pdfcer_core::writer::SaveOptions;

const PDFAID: &str = " xmlns:pdfaid=\"http://www.aiim.org/pdfa/ns/id/\"";

/// The XMP packet: a `dc:title` of `title`, plus `extra` attributes on
/// the `rdf:Description`.
fn packet(title: &str, extra: &str) -> String {
    format!(
        "<?xpacket begin=\"\u{feff}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>\n\
<x:xmpmeta xmlns:x=\"adobe:ns:meta/\">\n\
 <rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n\
  <rdf:Description rdf:about=\"\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\"{extra}>\n\
   <dc:title><rdf:Alt><rdf:li xml:lang=\"x-default\">{title}</rdf:li></rdf:Alt></dc:title>\n\
  </rdf:Description>\n\
 </rdf:RDF>\n\
</x:xmpmeta>\n\
<?xpacket end=\"w\"?>"
    )
}

/// A one-page classic-xref file with `/Info` 4 (`/Title (Old)`), and when
/// given, `/Metadata` 5 holding `xmp`; `signed` adds a signature field.
fn pdf(xmp: Option<&str>, signed: bool) -> Vec<u8> {
    let metadata = if xmp.is_some() {
        " /Metadata 5 0 R"
    } else {
        ""
    };
    let form = if signed {
        " /AcroForm << /Fields [6 0 R] /SigFlags 3 >>"
    } else {
        ""
    };
    let mut bodies = vec![
        (
            1,
            format!("<< /Type /Catalog /Pages 2 0 R{metadata}{form} >>"),
        ),
        (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned()),
        (
            3,
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] >>".to_owned(),
        ),
        (
            4,
            "<< /Producer (P) /Title (Old) /Trapped /False >>".to_owned(),
        ),
    ];
    if let Some(x) = xmp {
        bodies.push((
            5,
            format!(
                "<< /Type /Metadata /Subtype /XML /Length {} >>\nstream\n{x}\nendstream",
                x.len()
            ),
        ));
    }
    if signed {
        bodies.push((6, "<< /FT /Sig /T (S) /V 7 0 R >>".to_owned()));
        bodies.push((
            7,
            "<< /Type /Sig /Filter /Adobe.PPKLite /SubFilter /adbe.pkcs7.detached \
/ByteRange [0 10 20 10] /Contents <00> >>"
                .to_owned(),
        ));
    }
    let mut buf = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::new();
    for (num, body) in &bodies {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{num} 0 obj\n{body}\nendobj\n").as_bytes());
    }
    let xref_at = buf.len();
    buf.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", bodies.len() + 1).as_bytes(),
    );
    for off in offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    buf.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R /Info 4 0 R >>\nstartxref\n{xref_at}\n%%EOF\n",
            bodies.len() + 1
        )
        .as_bytes(),
    );
    buf
}

fn session(bytes: &[u8]) -> EditSession {
    EditSession::new(Document::from_bytes(bytes.to_vec()).unwrap())
}

fn save(s: &EditSession) -> Vec<u8> {
    s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0
}

fn updated(property: &str) -> XmpSync {
    XmpSync::Updated {
        property: property.to_owned(),
    }
}

fn in_step(property: &str) -> XmpSync {
    XmpSync::InStep {
        property: property.to_owned(),
    }
}

fn skipped(property: &str, reason: XmpSkip) -> XmpSync {
    XmpSync::Skipped {
        property: Some(property.to_owned()),
        reason,
    }
}

fn text_of(s: &EditSession, key: &[u8]) -> Option<String> {
    s.info_entries()
        .into_iter()
        .find(|e| e.key == key)
        .and_then(|e| match e.value {
            InfoValue::Text(t) => Some(t.text),
            _ => None,
        })
}

#[test]
fn a_title_reads_back_from_info_and_dc_title_after_save_and_reopen() {
    let base = pdf(Some(&packet("Old", "")), false);
    let mut s = session(&base);
    let out = s.set_info_field(InfoField::Title, Some("New")).unwrap();
    assert!(out.info_changed);
    assert_eq!(out.xmp, updated("dc:title"));
    assert_eq!(s.undo_depth(), 1, "one undo entry for both writes");

    let saved = save(&s);
    let tail = String::from_utf8_lossy(&saved[base.len()..]);
    assert!(tail.contains("<rdf:li xml:lang=\"x-default\">New</rdf:li>"));
    assert!(tail.contains("<?xpacket end=\"w\"?>"), "wrapper kept");
    assert_eq!(
        tail.matches(" 0 obj").count(),
        2,
        "only /Info and /Metadata"
    );

    let mut reopened = session(&saved);
    assert_eq!(reopened.info_text(InfoField::Title).unwrap().text, "New");
    // Setting the same value again proves the reopened packet says it too.
    let again = reopened
        .set_info_field(InfoField::Title, Some("New"))
        .unwrap();
    assert!(!again.recorded());
    assert_eq!(again.xmp, in_step("dc:title"));
}

#[test]
fn a_custom_entry_round_trips_through_add_edit_remove() {
    let mut bytes = pdf(Some(&packet("Old", "")), false);
    for value in [Some("Tooling"), Some("Tooling & Dies"), None] {
        let mut s = session(&bytes);
        let out = s.set_info_entry(b"My Dept", value).unwrap();
        assert!(out.info_changed);
        assert_eq!(out.xmp, updated("pdfx:My\u{2182}0020Dept"));
        assert_eq!(s.undo_depth(), 1);
        bytes = save(&s);
        let mut reopened = session(&bytes);
        assert_eq!(text_of(&reopened, b"My Dept").as_deref(), value);
        let check = reopened.set_info_entry(b"My Dept", value).unwrap();
        assert!(!check.recorded(), "{value:?} is in /Info and XMP");
        assert_eq!(check.xmp, in_step("pdfx:My\u{2182}0020Dept"));
    }
}

#[test]
fn undo_restores_both_and_saves_byte_identical() {
    let base = pdf(Some(&packet("Old", "")), false);
    let mut s = session(&base);
    s.set_info_entry(b"Creator", Some("CAD 9")).unwrap();
    s.set_info_entry(b"Custom", Some("x")).unwrap();
    assert_eq!(s.undo_depth(), 2);
    s.undo();
    s.undo();
    assert!(!s.is_modified());
    assert_eq!(save(&s), base);
}

#[test]
fn a_no_op_records_nothing_and_an_xmp_only_change_records_one() {
    let mut s = session(&pdf(Some(&packet("Old", "")), false));
    let out = s.set_info_field(InfoField::Title, Some("Old")).unwrap();
    assert!(!out.recorded());
    assert_eq!(s.undo_depth(), 0);

    let mut s = session(&pdf(Some(&packet("Stale", "")), false));
    let out = s.set_info_field(InfoField::Title, Some("Old")).unwrap();
    assert!(!out.info_changed);
    assert_eq!(out.xmp, updated("dc:title"));
    assert_eq!(s.undo_depth(), 1);
}

#[test]
fn creator_maps_to_creator_tool() {
    let mut s = session(&pdf(Some(&packet("Old", "")), false));
    let out = s.set_info_field(InfoField::Creator, Some("CAD")).unwrap();
    assert_eq!(out.xmp, updated("xmp:CreatorTool"));
    assert_eq!(text_of(&s, b"Creator").as_deref(), Some("CAD"));
}

#[test]
fn reserved_and_invalid_keys_are_refused_by_name() {
    let mut s = session(&pdf(None, false));
    for (key, which) in [
        (&b"Producer"[..], ReservedInfoKey::Producer),
        (b"CreationDate", ReservedInfoKey::CreationDate),
        (b"ModDate", ReservedInfoKey::ModDate),
        (b"Trapped", ReservedInfoKey::Trapped),
    ] {
        let err = s.set_info_entry(key, Some("x")).unwrap_err();
        assert!(matches!(err, EditError::InfoKeyReserved { key } if key == which));
    }
    for (key, problem) in [
        (&b""[..], InfoKeyProblem::Empty),
        (b"a\0b", InfoKeyProblem::ContainsNul),
        (&[b'k'; 128][..], InfoKeyProblem::TooLong),
    ] {
        let err = s.set_info_entry(key, Some("x")).unwrap_err();
        assert!(matches!(err, EditError::InvalidInfoKey { problem: p } if p == problem));
    }
    assert!(s.set_info_entry(&[b'k'; 127], Some("x")).is_ok());
    assert_eq!(s.undo_depth(), 1, "refusals record nothing");
}

#[test]
fn pdfa_1_to_3_keeps_custom_keys_out_of_xmp() {
    for part in ["1", "2", "3"] {
        let extra = format!("{PDFAID} pdfaid:part=\"{part}\"");
        let mut s = session(&pdf(Some(&packet("Old", &extra)), false));
        let out = s.set_info_entry(b"Dept", Some("x")).unwrap();
        assert!(out.info_changed, "/Info is still written");
        let reason = XmpSkip::PdfaCustomKey {
            part: part.parse().unwrap(),
        };
        assert_eq!(out.xmp, skipped("pdfx:Dept", reason));
        let title = s.set_info_field(InfoField::Title, Some("New")).unwrap();
        assert_eq!(title.xmp, updated("dc:title"), "standard keys still mirror");
    }
    let extra = format!("{PDFAID} pdfaid:part=\"4\"");
    let mut s = session(&pdf(Some(&packet("Old", &extra)), false));
    let out = s.set_info_entry(b"Dept", Some("x")).unwrap();
    assert_eq!(out.xmp, updated("pdfx:Dept"), "PDF/A-4 allows pdfx:");
}

#[test]
fn a_signed_document_leaves_the_packet_unless_told() {
    let bytes = pdf(Some(&packet("Old", "")), true);
    let mut s = session(&bytes);
    let out = s.set_info_field(InfoField::Title, Some("New")).unwrap();
    assert_eq!(out.xmp, skipped("dc:title", XmpSkip::Signed));
    let mut s = session(&bytes);
    s.set_xmp_sync(XmpSyncOptions {
        signed: SignedPacket::Update,
        ..XmpSyncOptions::default()
    });
    let out = s.set_info_field(InfoField::Title, Some("New")).unwrap();
    assert_eq!(out.xmp, updated("dc:title"));
}

#[test]
fn no_packet_off_and_unreadable_are_reported() {
    let mut s = session(&pdf(None, false));
    let out = s.set_info_field(InfoField::Title, Some("New")).unwrap();
    assert_eq!(out.xmp, XmpSync::NoPacket);

    let mut s = session(&pdf(Some(&packet("Old", "")), false));
    s.set_xmp_sync(XmpSyncOptions {
        mirror: XmpMirror::Never,
        ..XmpSyncOptions::default()
    });
    let out = s.set_info_field(InfoField::Title, Some("New")).unwrap();
    assert_eq!(out.xmp, skipped("dc:title", XmpSkip::Off));

    let mut s = session(&pdf(Some("not <xml"), false));
    let out = s.set_info_field(InfoField::Title, Some("New")).unwrap();
    assert!(out.info_changed);
    assert_eq!(out.xmp, skipped("dc:title", XmpSkip::Unreadable));
}

#[test]
fn info_entries_lists_full_values_including_names() {
    let mut s = session(&pdf(None, false));
    let long = "x".repeat(300);
    s.set_info_entry(b"Notes", Some(&long)).unwrap();
    let entries = s.info_entries();
    let keys: Vec<&[u8]> = entries.iter().map(|e| e.key.as_slice()).collect();
    assert_eq!(keys, [&b"Producer"[..], b"Title", b"Trapped", b"Notes"]);
    assert_eq!(text_of(&s, b"Notes").as_deref(), Some(long.as_str()));
    assert_eq!(entries[2].value, InfoValue::Name(b"False".to_vec()));
}
