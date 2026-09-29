//! `button_action` reads back every action `set_button_action` writes
//! (`GoTo`, `SubmitForm`, `Hide`, as well as `ResetForm` and `Named`), and
//! answers `Unmodelled` for an authored subtype in a shape pdfcer would not
//! write — never a nearest match.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::{
    ButtonAction, ButtonActionState, EditSession, FdfOptions, PageView, SubmitFormat, SubmitScope,
    SubmitSpec,
};
use pdfcer_core::writer::SaveOptions;

/// Two pages (the second with a non-zero crop origin) and a push button
/// `Go` on page 1 whose `/A` is `action` (empty for none).
fn form(action: &str) -> Vec<u8> {
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [5 0 R 6 0 R] >> >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 400] /Annots [5 0 R 6 0 R] >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /CropBox [10.5 20 600 780.25] >>"
            .to_owned(),
        format!(
            "<< /Type /Annot /Subtype /Widget /FT /Btn /Ff 65536 /T (Go) \
             /Rect [20 300 100 325] /P 3 0 R {action} >>"
        ),
        "<< /Type /Annot /Subtype /Widget /FT /Tx /T (Name) /Rect [20 200 200 220] /P 3 0 R >>"
            .to_owned(),
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
    for off in &offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n")
            .as_bytes(),
    );
    buf
}

fn session(action: &str) -> EditSession {
    EditSession::new(Document::from_bytes(form(action)).unwrap())
}

/// Write `action`, save, reopen, and read it back.
fn round_trip(action: ButtonAction) -> ButtonActionState {
    let mut s = session("");
    s.set_button_action("Go", Some(action)).expect("authors");
    let bytes = s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0;
    EditSession::new(Document::from_bytes(bytes).unwrap())
        .button_action("Go")
        .unwrap()
}

#[test]
fn every_goto_view_reads_back_after_a_save() {
    for (page_index, view) in [
        (0, PageView::WholePage),
        (1, PageView::FullWidth),
        (1, PageView::TopLeft),
    ] {
        let action = ButtonAction::GoToPage { page_index, view };
        assert_eq!(round_trip(action.clone()), ButtonActionState::Known(action));
    }
}

#[test]
fn every_submit_format_and_scope_reads_back_after_a_save() {
    let mut fdf = SubmitSpec::new("https://example.com/in");
    let mut opts = FdfOptions::default();
    opts.include_annotations = true;
    opts.embed_form = true;
    fdf.format = SubmitFormat::Fdf(opts);
    fdf.include_no_value_fields = true;

    let mut html = SubmitSpec::new("https://example.com/in");
    html.format = SubmitFormat::Html {
        get: true,
        coordinates: false,
    };
    html.scope = SubmitScope::Only(vec!["Name".to_owned()]);

    let mut xfdf = SubmitSpec::new("https://example.com/in");
    xfdf.format = SubmitFormat::Xfdf;
    xfdf.scope = SubmitScope::Except(vec!["Name".to_owned()]);
    xfdf.canonical_dates = true;

    let mut whole = SubmitSpec::new("mailto:forms@example.com");
    whole.format = SubmitFormat::WholeDocument;

    for spec in [
        SubmitSpec::new("https://example.com/in"),
        fdf,
        html,
        xfdf,
        whole,
    ] {
        let action = ButtonAction::SubmitForm(spec);
        assert_eq!(round_trip(action.clone()), ButtonActionState::Known(action));
    }
}

#[test]
fn hide_and_show_read_back_for_one_target_and_several() {
    for (targets, hidden) in [
        (vec!["Name".to_owned()], true),
        (vec!["Name".to_owned(), "Go".to_owned()], false),
    ] {
        let action = ButtonAction::SetHidden { targets, hidden };
        assert_eq!(round_trip(action.clone()), ButtonActionState::Known(action));
    }
}

#[test]
fn a_hide_without_h_hides() {
    let s = session("/A << /S /Hide /T (Name) >>");
    assert_eq!(
        s.button_action("Go").unwrap(),
        ButtonActionState::Known(ButtonAction::SetHidden {
            targets: vec!["Name".to_owned()],
            hidden: true,
        })
    );
}

#[test]
fn a_topleft_with_zoom_zero_is_the_same_as_null() {
    let s = session("/A << /S /GoTo /D [4 0 R /XYZ 10.5 780.25 0] >>");
    assert_eq!(
        s.button_action("Go").unwrap(),
        ButtonActionState::Known(ButtonAction::GoToPage {
            page_index: 1,
            view: PageView::TopLeft,
        })
    );
}

#[test]
fn shapes_pdfcer_would_not_write_are_unmodelled() {
    for (a, subtype) in [
        // Named destination.
        ("/A << /S /GoTo /D (chapter1) >>", "GoTo"),
        // A zoom pdfcer never writes.
        ("/A << /S /GoTo /D [3 0 R /XYZ 0 400 2] >>", "GoTo"),
        // A FitH top that is not the crop box's upper edge.
        ("/A << /S /GoTo /D [3 0 R /FitH 100] >>", "GoTo"),
        // A page given by number (a remote-style destination).
        ("/A << /S /GoTo /D [0 /Fit] >>", "GoTo"),
        // A bare-string /F is a file path, not a URL.
        (
            "/A << /S /SubmitForm /F (https://e.com/x) /Flags 0 >>",
            "SubmitForm",
        ),
        // Reserved bit 13.
        (
            "/A << /S /SubmitForm /F << /FS /URL /F (https://e.com/x) >> /Flags 4096 >>",
            "SubmitForm",
        ),
        // An HTML bit on an XFDF submit.
        (
            "/A << /S /SubmitForm /F << /FS /URL /F (https://e.com/x) >> /Flags 40 >>",
            "SubmitForm",
        ),
        // Exclude bit with no /Fields.
        (
            "/A << /S /SubmitForm /F << /FS /URL /F (https://e.com/x) >> /Flags 1 >>",
            "SubmitForm",
        ),
        // An annotation named by reference.
        ("/A << /S /Hide /T 6 0 R >>", "Hide"),
        ("/A << /S /Hide /T [(Name) 6 0 R] >>", "Hide"),
    ] {
        assert_eq!(
            session(a).button_action("Go").unwrap(),
            ButtonActionState::Unmodelled(subtype.to_owned()),
            "{a}"
        );
    }
}
