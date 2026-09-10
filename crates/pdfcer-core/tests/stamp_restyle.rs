//! `Pass 292.0` — a placed stamp's label size can be read and written, and a
//! restyle stops eating the stamp's own words.
//!
//! # The gap, as the consuming shell measured it
//!
//! > *"`Pass 287.0` gave the operator a stamp's label size **at authoring
//! > time** … It gave him nothing **afterwards**: a stamp that is already on
//! > the page has a size he cannot see and cannot change. The read half and
//! > the write half are both absent, and the properties panel needs both — a
//! > field with nothing to open on is as unbuildable as a field with no verb
//! > to call."*
//!
//! Both halves land here, and they are one Pass rather than two because
//! either alone is a surface nobody can build: a read with no write gives a
//! number that cannot be acted on, and a write with no read gives a control
//! that opens on a guess and silently overwrites whatever was there.
//!
//! # ★★★ And a live defect found on the way in
//!
//! `set_text_annot_style` re-bakes the annotation's appearance from a spec
//! read back out of the file — and `text_spec_from_dict` reports `label: None`
//! for a `/Stamp`, deliberately (a stamp's `/Contents` is a comment ABOUT the
//! stamp and must not drive its face). `None` rebuilds as **the stamp name's
//! default label**, so changing the colour of a stamp reading
//! `APPROVED FOR CONSTRUCTION` produced one reading `DRAFT`.
//!
//! Measured on a real file before it was fixed, from a control captioned
//! "colour". `R245`'s exact shape: the recovery EXISTS and the resize route
//! already calls it; this verb re-bakes the same annotation family and did
//! not. A capability present on one route of two.

use pdfcer_core::annot::StampSizeSource;
use pdfcer_core::annot_author::{
    Color, StampFit, StampLabelFit, StampName, StampStyle, TextAnnotSpec,
};
use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditError, EditSession, TextAnnotStyle};
use pdfcer_core::object::{Name, Object};
use pdfcer_core::page_tree::Rect;

const LABEL: &str = "APPROVED FOR CONSTRUCTION";

fn one_page() -> Vec<u8> {
    let bodies: Vec<(u32, String)> = vec![
        (1, "<< /Type /Catalog /Pages 2 0 R >>".to_owned()),
        (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned()),
        (
            3,
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << >> >>".to_owned(),
        ),
    ];
    let mut buf = b"%PDF-1.7\n".to_vec();
    let mut offsets: Vec<(u32, usize)> = Vec::new();
    for (num, body) in &bodies {
        offsets.push((*num, buf.len()));
        buf.extend_from_slice(format!("{num} 0 obj\n{body}\nendobj\n").as_bytes());
    }
    let xref_at = buf.len();
    buf.extend_from_slice(b"xref\n0 4\n0000000000 65535 f \n");
    for (_, off) in &offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size 4 /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n").as_bytes(),
    );
    buf
}

fn rect(width: f64) -> Rect {
    Rect {
        llx: 50.0,
        lly: 50.0,
        urx: 50.0 + width,
        ury: 90.0,
    }
}

/// A session holding one page with one stamp, labelled [`LABEL`] at 12 pt in
/// a box wide enough to hold it.
fn session_with_stamp(width: f64) -> (EditSession, pdfcer_core::object::ObjId) {
    let doc = Document::from_bytes(one_page()).expect("fixture loads");
    let mut session = EditSession::new(doc);
    let id = session
        .add_text_annotation(
            0,
            &TextAnnotSpec::Stamp {
                rect: rect(width),
                name: StampName::Draft,
                label: Some(LABEL.to_owned()),
                color: Color::Gray(0.0),
                style: StampStyle::points(12.0),
            },
        )
        .expect("the stamp is authored");
    (session, id)
}

/// The baked `Tf` size in an annotation's appearance stream.
fn baked_size(session: &EditSession, annot_id: pdfcer_core::object::ObjId) -> f64 {
    let params = session
        .stamp_label_parameters(annot_id)
        .expect("the annotation exists")
        .expect("pdfcer drew this stamp");
    params.size
}

// ------------------------------------------- 1. the defect found on the way

/// ★★★ A COLOUR CHANGE MUST NOT REWRITE THE STAMP'S WORDS.
///
/// This is the regression test for a live defect: the re-bake read the
/// annotation back through `text_spec_from_dict`, which reports `label: None`
/// for a stamp, and `None` means *use the stamp name's default* — `DRAFT`.
#[test]
fn a_colour_restyle_keeps_the_stamps_own_label() {
    let (mut session, id) = session_with_stamp(350.0);
    session
        .set_text_annot_style(
            id,
            &TextAnnotStyle {
                icon: None,
                color: Some(Color::Rgb(1.0, 0.0, 0.0)),
                font_size: None,
                stamp_fit: None,
            },
        )
        .expect("the restyle succeeds");

    let params = session
        .stamp_label_parameters(id)
        .expect("the annotation exists")
        .expect("pdfcer drew this stamp");
    assert_eq!(
        params.label, LABEL,
        "the operator asked for a colour, not for different words"
    );
}

// ------------------------------------------------------ 2. the write half

/// A new size is written, and it is the size the stamp then reports.
#[test]
fn a_new_label_size_is_written_and_reads_back() {
    let (mut session, id) = session_with_stamp(350.0);
    assert_eq!(baked_size(&session, id), 12.0);

    let change = session
        .set_text_annot_style(
            id,
            &TextAnnotStyle {
                icon: None,
                color: None,
                font_size: Some(20.0),
                stamp_fit: None,
            },
        )
        .expect("the restyle succeeds");

    assert!(change.font_size_written);
    assert_eq!(baked_size(&session, id), 20.0);
    // And the label is still the operator's.
    let params = session
        .stamp_label_parameters(id)
        .expect("exists")
        .expect("described");
    assert_eq!(params.label, LABEL);
    assert_eq!(params.size_source, StampSizeSource::DeclaredInDa);
}

/// ★★ A size that no longer fits the box moves the BOX, and says so.
///
/// Writing `/DA` without touching `/Rect` would re-open the clipped-stamp trap
/// `Pass 287.0` closed, through a route that Pass never covered — so the
/// re-bake runs the same fit policy the authoring path runs, and the outcome
/// comes back with the change rather than being left for the caller to
/// re-derive.
#[test]
fn a_resized_label_refits_its_box_and_reports_it() {
    let (mut session, id) = session_with_stamp(350.0);
    let change = session
        .set_text_annot_style(
            id,
            &TextAnnotStyle {
                icon: None,
                color: None,
                // Far too large for a 350pt box holding 25 characters.
                font_size: Some(48.0),
                stamp_fit: None,
            },
        )
        .expect("the restyle succeeds");

    let fit = change.stamp_label_fit.expect("a stamp reports its fit");
    assert!(fit.is_inference(), "the box moved without being asked");
    assert_eq!(fit.token(), "box_grown", "grow is the default policy");
    assert!(
        change.rect_after.width() > 350.0,
        "the rectangle grew to {}",
        change.rect_after.width()
    );
}

/// The caller can ask for a different policy — and `shrink` keeps the box the
/// operator drew, which is the whole point of offering it.
#[test]
fn the_caller_chooses_the_fit_policy_for_a_resize() {
    let (mut session, id) = session_with_stamp(350.0);
    let change = session
        .set_text_annot_style(
            id,
            &TextAnnotStyle {
                icon: None,
                color: None,
                font_size: Some(48.0),
                stamp_fit: Some(StampFit::ShrinkToBox),
            },
        )
        .expect("the restyle succeeds");

    let fit = change.stamp_label_fit.expect("a stamp reports its fit");
    assert_eq!(fit.token(), "label_shrunk");
    assert!(
        (change.rect_after.width() - 350.0).abs() < 1e-9,
        "the drawn box is kept"
    );
    match fit {
        StampLabelFit::LabelShrunk { size, requested } => {
            assert_eq!(requested, 48.0, "what the operator asked for");
            assert!(size < 48.0, "what the box allowed, got {size}");
        }
        other => panic!("expected LabelShrunk, got {other:?}"),
    }
}

/// A sticky note draws an ICON. A label size is refused BY NAME rather than
/// swallowed — `Pass 258.0`'s posture, and the mirror of `icon`'s refusal on
/// the other two subtypes.
#[test]
fn a_font_size_is_refused_on_a_sticky_note() {
    let doc = Document::from_bytes(one_page()).expect("fixture loads");
    let mut session = EditSession::new(doc);
    let id = session
        .add_text_annotation(
            0,
            &TextAnnotSpec::Sticky {
                rect: rect(20.0),
                icon: pdfcer_core::annot_author::StickyIcon::Note,
                contents: "hello".to_owned(),
                color: Color::Rgb(1.0, 1.0, 0.0),
                open: false,
            },
        )
        .expect("the note is authored");

    let err = session
        .set_text_annot_style(
            id,
            &TextAnnotStyle {
                icon: None,
                color: None,
                font_size: Some(20.0),
                stamp_fit: None,
            },
        )
        .expect_err("a sticky note has no label to size");
    match err {
        EditError::StylePropertyNotApplicable {
            subtype, property, ..
        } => {
            assert_eq!(subtype, "Text");
            assert_eq!(property, "a label font size");
        }
        other => panic!("expected StylePropertyNotApplicable, got {other:?}"),
    }
}

// ------------------------------------------------------- 3. the read half

/// The parameters come back with the label, the size, and — the part a panel
/// cannot do without — WHERE the size came from.
#[test]
fn a_placed_stamp_reports_its_label_and_size() {
    let (session, id) = session_with_stamp(350.0);
    let params = session
        .stamp_label_parameters(id)
        .expect("exists")
        .expect("described");
    // Field by field: `StampLabelParameters` is `#[non_exhaustive]`, so a
    // struct literal is not available outside the crate -- which is the point
    // of the attribute and not an inconvenience to work around.
    assert_eq!(params.label, LABEL);
    assert_eq!(params.size, 12.0);
    assert_eq!(params.size_source, StampSizeSource::DeclaredInDa);
}

/// ★★ THE THREE SIZE SOURCES ARE NOT INTERCHANGEABLE, and the shell said so
/// first: *"What we must be able to distinguish … the author stated no size
/// from the author stated a size we could not parse. Those look identical
/// through an `Option` and they mean opposite things to a panel."*
///
/// Driven through the parse directly, on the SAVED document, with the
/// annotation dictionary doctored the two ways a real file can be doctored.
/// Going through the file rather than through a session is deliberate: these
/// are states other producers leave behind, not states pdfcer can author.
#[test]
fn the_size_source_distinguishes_absent_from_unreadable() {
    let (session, _id) = session_with_stamp(350.0);
    let (bytes, _) = session
        .to_full_bytes(&pdfcer_core::writer::SaveOptions::default())
        .expect("the document saves");
    let doc = Document::from_bytes(bytes).expect("and re-opens");

    // The stamp, as it now sits in a file.
    let stamp = pdfcer_core::page_tree::pages(&doc)
        .expect("pages")
        .first()
        .and_then(|page| {
            pdfcer_core::annot::page_annotations(&doc, page.id)
                .into_iter()
                .find(|a| a.subtype == b"Stamp")
        })
        .expect("the stamp is there");
    let dict = match pdfcer_core::graph::ObjectGraph::value(&doc, stamp.id.expect("indirect")) {
        Some(Object::Dict(d)) => d.clone(),
        other => panic!("expected a dictionary, got {other:?}"),
    };
    let source = pdfcer_core::view::StreamSource::Contiguous(doc.bytes());

    // (0) As authored: `/DA` states it.
    let stated = pdfcer_core::annot::stamp_label_parameters_in(&doc, source, &dict)
        .expect("pdfcer drew this stamp");
    assert_eq!(stated.size_source, StampSizeSource::DeclaredInDa);
    assert_eq!(stated.size, 12.0);

    // (a) No `/DA` at all -- every stamp authored before `Pass 287.0`, and
    //     everything another producer wrote. The size is read off the picture,
    //     and that is not an anomaly.
    let mut without_da = dict.clone();
    without_da.remove(b"DA");
    let recovered = pdfcer_core::annot::stamp_label_parameters_in(&doc, source, &without_da)
        .expect("the picture still knows");
    assert_eq!(
        recovered.size_source,
        StampSizeSource::RecoveredFromAppearance
    );
    assert_eq!(recovered.size, 12.0);

    // (b) A `/DA` that IS present and yields no `Tf` size. Same number, and a
    //     completely different thing to tell the operator.
    let mut bad_da = dict;
    bad_da.insert(Name::from(b"DA"), Object::String(b"0 g".to_vec()));
    let unreadable = pdfcer_core::annot::stamp_label_parameters_in(&doc, source, &bad_da)
        .expect("still recoverable");
    assert_eq!(unreadable.size_source, StampSizeSource::DaUnreadable);
    assert_eq!(unreadable.size, 12.0, "still recoverable, still an anomaly");
    assert_eq!(unreadable.label, LABEL);
}

/// A non-stamp answers `None` rather than an error: "this annotation has no
/// stamp label" is a fact about the annotation, not a failure to look.
#[test]
fn a_non_stamp_has_no_stamp_parameters() {
    let doc = Document::from_bytes(one_page()).expect("fixture loads");
    let mut session = EditSession::new(doc);
    let id = session
        .add_text_annotation(
            0,
            &TextAnnotSpec::Sticky {
                rect: rect(20.0),
                icon: pdfcer_core::annot_author::StickyIcon::Note,
                contents: "hello".to_owned(),
                color: Color::Rgb(1.0, 1.0, 0.0),
                open: false,
            },
        )
        .expect("the note is authored");
    assert_eq!(
        session.stamp_label_parameters(id).expect("exists"),
        None,
        "a sticky note is not a stamp"
    );
}
