//! `EditSession::to_print_ready_bytes` (pdfcer-gui request G273). Every claim
//! is checked on the re-opened copy, from outside the crate.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeSet;

use pdfcer_core::annot::{
    apply_print_usage, apply_view_usage, optional_content_default_off, page_annotations,
};
use pdfcer_core::annot_author::FreeTextC;
use pdfcer_core::crypto::PermissionBit;
use pdfcer_core::document::Document;
use pdfcer_core::edit::MarkupOptions;
use pdfcer_core::edit::{EditSession, EncryptionSettings, PrintReadyCopy, PrintReadyError};
use pdfcer_core::graph::ObjectGraph as _;
use pdfcer_core::object::{ObjId, Object};
use pdfcer_core::threed::ThreeDSpec;
use pdfcer_core::writer::{SaveOptions, WriteError};

use super::flatten_annotations::assemble;

fn form(content: &str) -> String {
    format!(
        "<< /Type /XObject /Subtype /Form /BBox [0 0 10 10] /Length {} >>\nstream\n{content}\nendstream",
        content.len()
    )
}

fn stream(body: &str) -> String {
    format!("<< /Length {} >>\nstream\n{body}\nendstream", body.len())
}

fn annot(subtype: &str, flags: u32, ap: u32, extra: &str) -> String {
    format!(
        "<< /Type /Annot /Subtype /{subtype} /F {flags} /Rect [0 0 10 10] \
         /AP << /N {ap} 0 R >> {extra} >>"
    )
}

/// Page 1 carries each case; `catalog_extra` adds to the catalog. Printing:
/// 4 3D, 8 Widget, 12 NoView+Print. Not printing: 9 Print clear, 10 Hidden.
/// 13 is a pop-up. Each burned look writes its own mark so it can be found.
fn source(catalog_extra: &str) -> Vec<u8> {
    assemble(&[
        format!(
            "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [8 0 R] >> {catalog_extra} >>"
        ),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents 16 0 R \
         /Annots [4 0 R 8 0 R 9 0 R 10 0 R 12 0 R 13 0 R] >>"
            .to_owned(),
        annot("3D", 4, 5, "/3DD 6 0 R"),
        form("0.11 g 0 0 10 10 re f"),
        "<< /Type /3D /Subtype /U3D /Length 11 >>\nstream\nMODELSECRET\nendstream".to_owned(),
        "null".to_owned(),
        annot("Widget", 4, 11, "/FT /Tx /T (f) /V (x)"),
        annot("Square", 0, 14, "/Contents (UNPRINTEDNOTE)"),
        annot("Square", 6, 15, ""),
        form("0.22 g 0 0 10 10 re f"),
        annot("Square", 36, 7, "").replace("/N 7 0 R", "/N 17 0 R"),
        "<< /Type /Annot /Subtype /Popup /Rect [0 0 1 1] /F 4 >>".to_owned(),
        form("0.33 g 0 0 10 10 re f"),
        form("0.44 g 0 0 10 10 re f"),
        stream("0.5 g 50 50 10 10 re f"),
        form("0.55 g 0 0 10 10 re f"),
    ])
}

fn copy_of(bytes: Vec<u8>) -> PrintReadyCopy {
    EditSession::new(Document::from_bytes(bytes).unwrap())
        .to_print_ready_bytes(FreeTextC::default())
        .unwrap()
}

fn has(bytes: &[u8], needle: &[u8]) -> bool {
    bytes.windows(needle.len()).any(|w| w == needle)
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn no_annotations_or_form(doc: &Document) {
    let session = EditSession::new(doc.clone());
    let slots = session.page_slots().unwrap();
    let graph = session.graph();
    assert!(page_annotations(&graph, slots[0].id).is_empty());
    let root = graph.resolve(doc.trailer().get(b"Root").unwrap());
    let root = root.as_dict().unwrap();
    assert!(!root.contains_key(b"AcroForm"));
    assert!(!root.contains_key(b"Perms"));
}

#[test]
fn what_prints_is_burned_and_the_rest_is_gone() {
    let copy = copy_of(source(""));
    let r = &copy.report;
    assert_eq!(r.burned, 3, "the 3D poster, the widget, the NoView one");
    assert_eq!(
        r.not_printing, 2,
        "Print clear, and Hidden; the pop-up is quiet"
    );
    assert_eq!(r.printed_not_on_screen, 1);
    assert!(r.lost.is_empty(), "{:?}", r.lost);
    assert!(!r.decrypted);

    let doc = Document::from_bytes(copy.bytes.clone()).unwrap();
    no_annotations_or_form(&doc);
    let raw = text(&copy.bytes);
    assert!(
        raw.contains("50 50 10 10 re"),
        "the page's own content: {raw}"
    );
    assert_eq!(raw.matches(" Do").count(), 3, "{raw}");

    // Sanity that a raw scan can see streams here, then what must not be there.
    assert!(
        has(&copy.bytes, b"0.11 g"),
        "the poster's look is in the file"
    );
    for gone in [
        &b"MODELSECRET"[..],
        b"UNPRINTEDNOTE",
        b"0.33 g",
        b"0.44 g",
        b"/Annots",
    ] {
        assert!(!has(&copy.bytes, gone), "{}", String::from_utf8_lossy(gone));
    }
    assert!(
        r.disclosures
            .iter()
            .any(|d| d.starts_with("2 annotation(s) set not to print"))
    );
}

/// An unsaved edit counts, and pdfcer's own 3D annotation is set to print, so
/// its poster comes out and its model does not.
#[test]
fn a_3d_annotation_added_in_the_session_prints_its_poster() {
    let model = b"\0\0PRCMODELBYTES".to_vec();
    let mut model_prc = b"PRC".to_vec();
    model_prc.extend_from_slice(&model);
    let mut session = EditSession::new(
        Document::from_bytes(assemble(&[
            "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] >>".to_owned(),
        ]))
        .unwrap(),
    );
    let rect = pdfcer_core::page_tree::Rect {
        llx: 10.0,
        lly: 10.0,
        urx: 90.0,
        ury: 90.0,
    };
    let spec = ThreeDSpec::new(rect, model_prc).expect("PRC sniffs");
    session
        .add_3d_annotation(0, &spec, &MarkupOptions::default())
        .unwrap();
    let copy = session.to_print_ready_bytes(FreeTextC::default()).unwrap();
    assert_eq!(copy.report.burned, 1, "{:?}", copy.report);
    assert_eq!(copy.report.not_printing, 0);
    let doc = Document::from_bytes(copy.bytes.clone()).unwrap();
    no_annotations_or_form(&doc);
    assert!(!has(&copy.bytes, b"/3DD"));
    assert!(!has(&copy.bytes, b"/Subtype /PRC") && !has(&copy.bytes, b"/Subtype/PRC"));
}

#[test]
fn the_open_session_is_untouched() {
    let bytes = source("");
    let session = EditSession::new(Document::from_bytes(bytes).unwrap());
    let before = session.to_full_bytes(&SaveOptions::default()).unwrap().0;
    let _ = session.to_print_ready_bytes(FreeTextC::default()).unwrap();
    assert!(!session.can_undo());
    assert_eq!(
        session.to_full_bytes(&SaveOptions::default()).unwrap().0,
        before
    );
}

#[test]
fn opacity_and_layer_survive_the_burn() {
    let copy = copy_of(assemble(&[
        "<< /Type /Catalog /Pages 2 0 R /OCProperties << /OCGs [7 0 R] /D << /OFF [7 0 R] >> >> >>"
            .to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Annots [4 0 R 5 0 R] >>".to_owned(),
        annot("Square", 4, 6, "/CA 0.5"),
        annot("Square", 4, 6, "/OC 7 0 R"),
        form("0.11 g 0 0 10 10 re f"),
        "<< /Type /OCG /Name (L) >>".to_owned(),
    ]));
    assert_eq!(copy.report.burned, 2);
    assert!(has(&copy.bytes, b"/ca 0.5"), "the /CA became an ExtGState");
    assert!(has(&copy.bytes, b"/OC"), "the /OC is on the burned form");
}

/// The same annotation with and without NoRotate on a turned page: only the
/// NoRotate one is pre-turned.
#[test]
fn a_rotated_page_keeps_a_no_rotate_annotation_upright() {
    let on_rotated_page = |flags: u32| {
        text(
            &copy_of(assemble(&[
                "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
                "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Rotate 90 \
                 /Annots [4 0 R] >>"
                    .to_owned(),
                annot("Square", flags, 5, ""),
                form("0.11 g 0 0 10 10 re f"),
            ]))
            .bytes,
        )
    };
    let plain = "1 0 0 1 0 0 cm";
    assert!(on_rotated_page(4).contains(plain));
    assert!(!on_rotated_page(4 | 16).contains(plain));
}

#[test]
fn a_missing_look_is_drawn_or_named_and_quiet_kinds_stay_quiet() {
    let copy = copy_of(assemble(&[
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Annots [4 0 R 5 0 R 6 0 R] >>"
            .to_owned(),
        "<< /Type /Annot /Subtype /Ink /F 4 /C [1 0 0] /InkList [[1 1 9 9]] /Rect [0 0 10 10] >>"
            .to_owned(),
        "<< /Type /Annot /Subtype /Link /F 4 /Rect [0 0 10 10] >>".to_owned(),
        "<< /Type /Annot /Subtype /Stamp /F 4 /Rect [0 0 10 10] >>".to_owned(),
    ]));
    let r = &copy.report;
    assert_eq!(r.appearances_drawn, 1, "the Ink");
    assert_eq!(r.burned, 1, "and it burned");
    assert_eq!(r.lost.len(), 1, "the Stamp, and not the Link: {:?}", r.lost);
    assert_eq!(r.lost[0].subtype, b"Stamp");
    assert_eq!(r.lost[0].page_index, 0);
    assert!(
        r.disclosures.iter().any(|d| d.contains("/Stamp")),
        "{:?}",
        r.disclosures
    );
}

fn encrypted(permissions: Vec<PermissionBit>) -> Vec<u8> {
    let session = EditSession::new(Document::from_bytes(source("")).unwrap());
    let mut settings = EncryptionSettings::new(b"u".to_vec(), b"o".to_vec());
    settings.permissions = permissions;
    session
        .set_encryption(&settings, &SaveOptions::default())
        .unwrap()
        .0
}

fn copy_under(bytes: Vec<u8>, password: &[u8]) -> Result<PrintReadyCopy, PrintReadyError> {
    EditSession::new(Document::from_bytes_with_password(bytes, Some(password)).unwrap())
        .to_print_ready_bytes(FreeTextC::default())
}

#[test]
fn an_encrypted_document_gives_a_plain_copy_when_printing_is_allowed() {
    let source = encrypted(vec![PermissionBit::Print, PermissionBit::PrintHighQuality]);
    let copy = copy_under(source, b"u").unwrap();
    assert!(copy.report.decrypted);
    assert_eq!(copy.report.burned, 3);
    assert!(
        copy.report
            .disclosures
            .iter()
            .any(|d| d.contains("not encrypted"))
    );
    let doc = Document::from_bytes(copy.bytes.clone()).unwrap();
    assert!(doc.encryption().is_none());
    no_annotations_or_form(&doc);
    assert!(
        has(&copy.bytes, b"0.11 g"),
        "the poster's look reads as plaintext"
    );
    assert!(!has(&copy.bytes, b"/Encrypt"));
    // AES leaves its padding in the source buffer past the shortened stream;
    // the copy must end each stream at its plaintext.
    let at = copy
        .bytes
        .windows(16)
        .position(|w| w == b"50 50 10 10 re f")
        .expect("the page's content reads as plaintext");
    let after = &copy.bytes[at + 16..];
    let close = after
        .windows(9)
        .position(|w| w == b"endstream")
        .expect("closed");
    assert!(
        close <= 2,
        "ciphertext left in the stream: {:?}",
        &after[..close]
    );
}

#[test]
fn a_password_that_may_not_print_gets_no_copy() {
    let source = encrypted(vec![PermissionBit::Copy]);
    assert!(matches!(
        copy_under(source.clone(), b"u"),
        Err(PrintReadyError::PrintingForbidden)
    ));
    assert!(
        copy_under(source, b"o").is_ok(),
        "the owner password allows everything"
    );
}

#[test]
fn low_quality_only_is_refused() {
    let source = encrypted(vec![PermissionBit::Print]);
    assert!(matches!(
        copy_under(source.clone(), b"u"),
        Err(PrintReadyError::LowQualityPrintingOnly)
    ));
    assert!(copy_under(source, b"o").is_ok());
}

#[test]
fn a_certified_document_still_prints_and_says_the_signature_stays_behind() {
    let certified = assemble(&[
        "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [5 0 R] /SigFlags 3 >> \
         /Perms << /DocMDP 6 0 R >> >>"
            .to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Annots [4 0 R 5 0 R] >>".to_owned(),
        annot("Square", 4, 7, ""),
        annot("Widget", 4, 7, "/FT /Sig /T (s) /V 6 0 R"),
        "<< /Type /Sig /Filter /Adobe.PPKLite /SubFilter /adbe.pkcs7.detached \
         /ByteRange [0 1 2 3] /Contents <00> \
         /Reference [ << /TransformMethod /DocMDP /TransformParams << /P 1 /V /1.2 >> >> ] >>"
            .to_owned(),
        form("0.11 g 0 0 10 10 re f"),
    ]);
    let copy = copy_of(certified);
    assert_eq!(copy.report.burned, 2);
    assert_eq!(copy.report.signatures_dropped, 1);
    assert!(
        copy.report
            .disclosures
            .iter()
            .any(|d| d.contains("signature"))
    );
    no_annotations_or_form(&Document::from_bytes(copy.bytes).unwrap());
}

fn with_redact_mark() -> Vec<u8> {
    assemble(&[
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Annots [4 0 R] >>".to_owned(),
        annot("Redact", 4, 5, "/QuadPoints [0 10 10 10 0 0 10 0]"),
        form("0.11 g 0 0 10 10 re f"),
    ])
}

#[test]
fn a_staged_redaction_is_refused_rather_than_printed_as_applied() {
    let mut session = EditSession::new(Document::from_bytes(with_redact_mark()).unwrap());
    session.apply_redactions_deferred().unwrap();
    assert!(matches!(
        session.to_print_ready_bytes(FreeTextC::default()),
        Err(PrintReadyError::Write(WriteError::RedactionPending))
    ));
}

#[test]
fn an_unapplied_redaction_mark_prints_and_says_nothing_was_removed() {
    let copy = copy_of(with_redact_mark());
    assert_eq!(copy.report.redaction_marks, 1);
    assert!(
        copy.report
            .disclosures
            .iter()
            .any(|d| d.contains("still in this copy"))
    );
}

/// Groups: 4 Print OFF, 5 Print ON but off by default, 6 no setting, 7 has a
/// print setting no `/AS` names. A View entry turns 4 on: it must not count.
fn layered() -> Vec<u8> {
    let group = |name: &str, usage: &str| format!("<< /Type /OCG /Name ({name}) {usage} >>");
    let print = |state: &str| format!("/Usage << /Print << /PrintState /{state} >> >>");
    assemble(&[
        "<< /Type /Catalog /Pages 2 0 R /OCProperties << /OCGs [4 0 R 5 0 R 6 0 R 7 0 R] \
         /D << /OFF [5 0 R] /AS [\
         << /Event /Print /Category [/Print] /OCGs [4 0 R 5 0 R 6 0 R] >> \
         << /Event /View /Category [/View] /OCGs [4 0 R] >>] >> >> >>"
            .to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] >>".to_owned(),
        group(
            "noprint",
            &(print("OFF").replace(">> >>", ">> /View << /ViewState /ON >> >>")),
        ),
        group("printed", &print("ON")),
        group("plain", ""),
        group("unlisted", &print("OFF")),
    ])
}

/// The names of the layers `bytes` hides by default, and whether `/AS` is there.
fn state_of(bytes: Vec<u8>) -> (Vec<String>, bool) {
    let session = EditSession::new(Document::from_bytes(bytes.clone()).unwrap());
    let graph = session.graph();
    let mut off: Vec<String> = optional_content_default_off(&graph)
        .into_iter()
        .map(|id| {
            let group = graph.resolved(id);
            let Some(Object::String(name)) = group.as_dict().unwrap().get(b"Name") else {
                panic!("a named group");
            };
            String::from_utf8_lossy(name).into_owned()
        })
        .collect();
    off.sort();
    (off, has(&bytes, b"/AS"))
}

#[test]
fn a_batch_copy_keeps_the_default_layers_and_says_so() {
    let copy = copy_of(layered());
    assert_eq!(copy.report.layers_with_print_setting, 3);
    assert_eq!(copy.report.layers_changed_for_print, 0);
    assert!(
        copy.report
            .disclosures
            .iter()
            .any(|d| d.contains("not applied")),
        "{:?}",
        copy.report.disclosures
    );
    let (off, usage_kept) = state_of(copy.bytes);
    assert_eq!(off, ["printed"]);
    assert!(usage_kept);
}

#[test]
fn a_viewers_copy_applies_print_usage_over_what_is_hidden_now() {
    let session = EditSession::new(Document::from_bytes(layered()).unwrap());
    // The operator hid "plain" by hand and "printed" is off by default.
    let current = BTreeSet::from([ObjId::new(5, 0), ObjId::new(6, 0)]);
    let copy = session
        .to_print_ready_bytes_with_layers(FreeTextC::default(), &current)
        .unwrap();
    // noprint goes off, printed comes on, plain stays as the operator left it,
    // and unlisted, which no /AS names, keeps its state.
    let (off, usage_kept) = state_of(copy.bytes);
    assert_eq!(off, ["noprint", "plain"]);
    assert!(!usage_kept, "a print service has nothing left to decide");
    assert_eq!(copy.report.layers_with_print_setting, 0);
    assert_eq!(copy.report.layers_changed_for_print, 3);
    assert!(
        copy.report
            .disclosures
            .iter()
            .any(|d| d.contains("differently"))
    );
    let graph = session.graph();
    assert_eq!(
        optional_content_default_off(&graph),
        BTreeSet::from([ObjId::new(5, 0)]),
        "the open document is untouched"
    );
}

#[test]
fn print_usage_answers_only_to_print_entries() {
    let session = EditSession::new(Document::from_bytes(layered()).unwrap());
    let graph = session.graph();
    let mut off = optional_content_default_off(&graph);
    let notes = apply_print_usage(&graph, &mut off);
    assert_eq!(off, BTreeSet::from([ObjId::new(4, 0)]));
    assert_eq!((notes.applications, notes.non_view_events), (1, 1));
    // Group 6 has no /Usage, so no verdict; 4 and 5 are managed.
    assert_eq!(notes.groups_managed, 2);

    let mut viewed = optional_content_default_off(&graph);
    let _ = apply_view_usage(&graph, &mut viewed, 1.0);
    assert_eq!(
        viewed,
        BTreeSet::from([ObjId::new(5, 0)]),
        "the Print entry has no say"
    );
}

#[test]
fn a_base_state_of_off_does_not_survive_into_the_fixed_state() {
    let bytes = assemble(&[
        "<< /Type /Catalog /Pages 2 0 R /OCProperties << /OCGs [4 0 R 5 0 R] \
         /D << /BaseState /OFF /ON [4 0 R] >> >> >>"
            .to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] >>".to_owned(),
        "<< /Type /OCG /Name (shown) >>".to_owned(),
        "<< /Type /OCG /Name (hidden) >>".to_owned(),
    ]);
    let session = EditSession::new(Document::from_bytes(bytes).unwrap());
    let current = optional_content_default_off(&session.graph());
    assert_eq!(current, BTreeSet::from([ObjId::new(5, 0)]));
    let copy = session
        .to_print_ready_bytes_with_layers(FreeTextC::default(), &current)
        .unwrap();
    let reopened = EditSession::new(Document::from_bytes(copy.bytes.clone()).unwrap());
    let graph = reopened.graph();
    let catalog = graph.catalog_dict().unwrap();
    let properties = graph.resolve(catalog.get(b"OCProperties").unwrap());
    let default = graph.resolve(properties.as_dict().unwrap().get(b"D").unwrap());
    assert!(
        default.as_dict().unwrap().get(b"ON").is_none(),
        "a stale /ON could contradict /OFF"
    );
    assert_eq!(state_of(copy.bytes).0, ["hidden"]);
}
