//! Integration tests for the ce-dimension **text override** (`Pass 175.0`,
//! `ARCHITECTURE.md` decision 097 branch 1).
//!
//! # What this file is asserting, and why each assertion is not redundant
//!
//! Decision 097's whole content is a set of PROPERTIES, not a feature name.
//! The operator asked for an override *"if it can be selected to be overridden
//! or not so the override can be undone"* — a requirement about the
//! reversibility of the override itself, distinct from command-level undo.
//! Every property that sentence implies is tested here, separately, because
//! any one of them can hold while another does not:
//!
//! 1. **The override reaches the PAGE.** A verb that writes a sidecar key and
//!    a regenerator that ignores it would pass every model-level assertion and
//!    print the measured number anyway (`a_correct_fix_can_be_unreachable`).
//!    Asserted on the baked `/AP` content-stream bytes and on `/Contents`.
//! 2. **The measurement SURVIVES underneath it.** The point of branch 1 over
//!    branch 2. Asserted by clearing and comparing to the pre-override bytes.
//! 3. **Both survive save-and-reopen**, which decision 097 requires by name —
//!    an override that lived only in session state would silently revert.
//! 4. **`<DIM>` keeps tracking the geometry**, so an override does not have to
//!    be a decision to stop measuring.
//! 5. **The refusals fire before any mutation** (`CLAUDE.md` rule 4).
//! 6. **The clipboard carries it**, because a field the caption is derived
//!    from, left off a clip, makes a pasted ce dimension read differently from
//!    the one it was copied from — the exact defect `Pass 173.1` fixed for the
//!    group's scale.
//! 7. **The sidecar version is content-dependent**, so a document with no
//!    override still opens for WRITING in the older build the operator keeps
//!    in the other folder.
//!
//! Public-API only, on the same synthetic one-page PDF the sibling
//! `dimension_roundtrip.rs` builds.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use pdfcer_core::dimension::{
    DEFAULT_GROUP_ID, DimensionId, DimensionKind, NumberFormat, ScaleState, Unit, deserialize_model,
};
use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditError, EditSession};
use pdfcer_core::object::Object;
use pdfcer_core::vector::{AxisConstraint, Point};
use pdfcer_core::writer::SaveOptions;

/// Build a minimal one-page PDF: catalog(1) → pages(2) → page(3).
///
/// Duplicated from `dimension_roundtrip.rs` rather than shared, because an
/// integration test binary cannot import another one and a `tests/common/`
/// module would put the fixture further from both readers than it is long.
fn minimal_pdf() -> Vec<u8> {
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 400] /Resources << >> >>",
    ];
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

/// A 200 pt horizontal linear ce dimension.
fn linear() -> DimensionKind {
    DimensionKind::Linear {
        a: Point::new(100.0, 200.0),
        b: Point::new(300.0, 200.0),
        constraint: AxisConstraint::Horizontal,
        offset: 0.0,
        text_along: 0.0,
    }
}

fn session() -> EditSession {
    EditSession::new(Document::from_bytes(minimal_pdf()).unwrap())
}

fn save(session: &EditSession) -> Vec<u8> {
    session
        .to_incremental_bytes(&SaveOptions::identity())
        .unwrap()
        .0
}

/// A calibrated session with one placed ce dimension, at 0.01 m/pt so the
/// 200 pt line measures `2.00 m` — a caption no accident produces.
fn placed() -> (EditSession, DimensionId) {
    let mut s = session();
    s.set_group_scale(
        DEFAULT_GROUP_ID,
        ScaleState::Calibrated { scale: 0.01 },
        NumberFormat::decimal(Unit::Meter, 2),
    )
    .unwrap();
    let (_, id) = s.add_dimension(0, DEFAULT_GROUP_ID, linear()).unwrap();
    (s, id)
}

/// The baked `/AP` `/N` content-stream bytes of the one ce dimension in
/// `session`.
///
/// Read from the SAVED FILE rather than from session state, because the file
/// is the copy that outlives the session and is the only one a reader ever
/// sees. A test that asserted on the model would pass on a build whose writer
/// dropped the field.
fn baked_appearance(session: &EditSession) -> String {
    let reloaded = Document::from_bytes(save(session)).unwrap();
    let s = EditSession::new(Document::from_bytes(save(session)).unwrap());
    let model = s.dimension_model();
    let annot_id = model.dimensions()[0]
        .annot
        .expect("a placed ce dimension has an annotation");
    let Object::Dict(annot) = &reloaded.get(annot_id).unwrap().value else {
        panic!("the annotation is a dict");
    };
    let ap_id = annot
        .get(b"AP")
        .unwrap()
        .as_dict()
        .unwrap()
        .get(b"N")
        .and_then(Object::as_reference)
        .expect("/AP /N reference");
    let Some(Object::Stream(ap)) = reloaded.get(ap_id).map(|io| &io.value) else {
        panic!("the /AP /N is a stream");
    };
    String::from_utf8_lossy(ap.data_span.slice(reloaded.bytes()).unwrap()).into_owned()
}

/// The `/Contents` of the one ce dimension's annotation, as saved.
fn saved_contents(session: &EditSession) -> String {
    let reloaded = Document::from_bytes(save(session)).unwrap();
    let s = EditSession::new(Document::from_bytes(save(session)).unwrap());
    let annot_id = s.dimension_model().dimensions()[0]
        .annot
        .expect("a placed ce dimension has an annotation");
    let Object::Dict(annot) = &reloaded.get(annot_id).unwrap().value else {
        panic!("the annotation is a dict");
    };
    match annot.get(b"Contents") {
        Some(Object::String(b)) => String::from_utf8_lossy(b).into_owned(),
        other => panic!("/Contents is a string, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// 1. The override reaches the PAGE.
// ---------------------------------------------------------------------------

/// **The override is what the file draws, not merely what the model holds.**
///
/// This is the assertion the rest of the feature is worthless without, and it
/// is the one a model-level test cannot make: `set_dimension_label` writes the
/// sidecar and `regenerate_dimension_writes` bakes the `/AP`, and until this
/// Pass wired the second, the first could store the operator's text, report it
/// back, satisfy every unit test, and print the measured number on the page.
#[test]
fn the_override_is_baked_into_the_appearance_and_the_contents() {
    let (mut s, id) = placed();
    assert!(
        baked_appearance(&s).contains("(2.00 m)"),
        "sanity: the un-overridden caption is its measurement"
    );

    let change = s.set_dimension_label(id, Some("55 5/8")).unwrap();
    assert!(change.changed);
    assert_eq!(change.measured, "2.00 m");
    assert_eq!(change.printed, "55 5/8");
    assert_eq!(change.previous, None);

    let ap = baked_appearance(&s);
    assert!(
        ap.contains("(55 5/8)"),
        "the override must reach the baked /AP; got {ap}"
    );
    assert!(
        !ap.contains("(2.00 m)"),
        "the measured caption must NOT also be drawn -- two captions is worse than the wrong one"
    );
    assert_eq!(
        saved_contents(&s),
        "55 5/8",
        "/Contents is the annotation's text representation and must agree with what is drawn"
    );
}

// ---------------------------------------------------------------------------
// 2 + 3. The measurement survives underneath, across a save.
// ---------------------------------------------------------------------------

/// **Clearing the override restores the measured caption EXACTLY, after a
/// save-and-reopen, with no re-measurement.**
///
/// The property that makes this branch 1 rather than branch 2 of decision 097,
/// and the property the operator asked for by name. The comparison is against
/// the appearance bytes captured BEFORE the override existed — an equality on
/// a freshly formatted string would pass even on an implementation that
/// re-derived the value from a rounded copy.
#[test]
fn clearing_the_override_restores_the_measured_caption_across_a_save() {
    let (mut s, id) = placed();
    let before = baked_appearance(&s);

    s.set_dimension_label(id, Some("APPROX 6 FT")).unwrap();
    assert!(baked_appearance(&s).contains("(APPROX 6 FT)"));

    // Round-trip: save, reopen, and clear in the REOPENED session, so the
    // measured value has to have survived in the file rather than in memory.
    let bytes = save(&s);
    let mut reopened = EditSession::new(Document::from_bytes(bytes).unwrap());
    let model = reopened.dimension_model();
    assert_eq!(
        model.label_override(id),
        Some("APPROX 6 FT"),
        "the override must survive save-and-reopen -- decision 097 requires this by name"
    );
    assert!(
        baked_appearance(&reopened).contains("(APPROX 6 FT)"),
        "and it must still be what the reopened file draws"
    );

    let cleared = reopened.set_dimension_label(id, None).unwrap();
    assert!(cleared.changed);
    assert_eq!(cleared.previous.as_deref(), Some("APPROX 6 FT"));
    assert_eq!(cleared.applied, None);
    assert_eq!(
        cleared.printed, cleared.measured,
        "a cleared dimension prints its measurement"
    );
    assert_eq!(
        cleared.measured, "2.00 m",
        "and the measurement is the ORIGINAL one -- not re-derived from the override"
    );
    assert_eq!(
        baked_appearance(&reopened),
        before,
        "the restored appearance must be byte-identical to the one before the override"
    );
}

// ---------------------------------------------------------------------------
// 4. `<DIM>` keeps tracking the geometry.
// ---------------------------------------------------------------------------

/// **A `<DIM>` override still follows a scale change.**
///
/// The parity-plus. Without the substitution happening at BAKE time — on every
/// regeneration — a `2X <DIM>` caption would freeze the number it was created
/// with, and a group re-scale would silently leave a drawing asserting a
/// measurement it no longer has. Asserted through `set_group_scale`, which is
/// the route that regenerates every member, so this also proves the override
/// survives a regeneration it did not initiate.
#[test]
fn a_dim_placeholder_override_follows_a_later_scale_change() {
    let (mut s, id) = placed();
    let change = s.set_dimension_label(id, Some("2X <DIM> TYP")).unwrap();
    assert_eq!(
        change.printed, "2X 2.00 m TYP",
        "the placeholder is substituted with the measured caption"
    );
    assert!(baked_appearance(&s).contains("(2X 2.00 m TYP)"));

    // Re-scale to 0.02 m/pt: the same 200 pt line now measures 4.00 m.
    s.set_group_scale(
        DEFAULT_GROUP_ID,
        ScaleState::Calibrated { scale: 0.02 },
        NumberFormat::decimal(Unit::Meter, 2),
    )
    .unwrap();
    let ap = baked_appearance(&s);
    assert!(
        ap.contains("(2X 4.00 m TYP)"),
        "a <DIM> override must FOLLOW the geometry through a regeneration; got {ap}"
    );
    assert_eq!(
        s.dimension_model().label_override(id),
        Some("2X <DIM> TYP"),
        "and the stored override is the template, not the substituted result -- \
         storing the result is how it would stop tracking"
    );
}

/// **A bare override does NOT follow a scale change**, which is the operator
/// saying so explicitly.
///
/// The contrast case for the test above. Both behaviours are correct and the
/// difference between them is the entire user-facing meaning of the
/// placeholder, so asserting one without the other would leave the
/// distinction unprotected.
#[test]
fn a_bare_override_does_not_follow_a_scale_change() {
    let (mut s, id) = placed();
    s.set_dimension_label(id, Some("55 5/8")).unwrap();
    s.set_group_scale(
        DEFAULT_GROUP_ID,
        ScaleState::Calibrated { scale: 0.02 },
        NumberFormat::decimal(Unit::Meter, 2),
    )
    .unwrap();
    assert!(
        baked_appearance(&s).contains("(55 5/8)"),
        "a bare override is a fixed caption and a re-scale must not disturb it"
    );
}

// ---------------------------------------------------------------------------
// 5. The refusals.
// ---------------------------------------------------------------------------

/// **Every refusal happens before any mutation** (`CLAUDE.md` rule 4), and
/// each one is refused BY NAME rather than by a generic invalid-argument.
///
/// The unprintable case is the one worth the most: until this Pass every
/// ce-dimension caption came from a closed machine-generated repertoire, so
/// the baker could ignore `encode_winansi`'s substitution count and says so in
/// a comment. An operator-typed override is the first caption that can contain
/// anything, and accepting one would print a question mark where they typed a
/// character — a silent value corruption in the one place on a drawing where a
/// wrong value costs the most.
#[test]
fn an_unusable_override_is_refused_by_name_and_changes_nothing() {
    let (mut s, id) = placed();
    let before = baked_appearance(&s);

    assert!(
        matches!(
            s.set_dimension_label(id, Some("   ")),
            Err(EditError::DimensionLabelEmpty)
        ),
        "a whitespace-only override is refused -- pass None to restore the measurement"
    );

    let long = "X".repeat(EditSession::MAX_DIMENSION_LABEL + 1);
    assert!(
        matches!(
            s.set_dimension_label(id, Some(&long)),
            Err(EditError::DimensionLabelTooLong { found, max })
                if found == EditSession::MAX_DIMENSION_LABEL + 1
                    && max == EditSession::MAX_DIMENSION_LABEL
        ),
        "an over-long override is refused with both numbers"
    );
    // The boundary itself is ACCEPTED -- an off-by-one here would make the
    // documented limit a lie by one character, which no other assertion catches.
    let at_limit = "Y".repeat(EditSession::MAX_DIMENSION_LABEL);
    assert!(s.set_dimension_label(id, Some(&at_limit)).is_ok());
    s.set_dimension_label(id, None).unwrap();

    // U+2300 DIAMETER SIGN has no WinAnsi code; U+00B0 DEGREE SIGN does, and
    // is included to prove the check is not simply rejecting non-ASCII.
    match s.set_dimension_label(id, Some("\u{2300}12\u{00B0}\u{2300}")) {
        Err(EditError::DimensionLabelUnprintable { chars }) => {
            assert_eq!(
                chars, "'\u{2300}'",
                "the offender is NAMED and deduplicated; the degree sign is fine and must not appear"
            );
        }
        other => panic!("an unprintable override must be refused by name, got {other:?}"),
    }
    // A caption of only WinAnsi-representable non-ASCII is accepted.
    assert!(
        s.set_dimension_label(id, Some("45\u{00B0} \u{00B1}1"))
            .is_ok()
    );
    s.set_dimension_label(id, None).unwrap();

    assert!(
        matches!(
            s.set_dimension_label(DimensionId(9999), Some("x")),
            Err(EditError::DimensionNotFound { id: 9999 })
        ),
        "an unknown id is refused before the model is touched"
    );
    assert_eq!(
        baked_appearance(&s),
        before,
        "after every refusal and a matched set/clear pair, the page is exactly as it was"
    );
}

/// **Setting the state that already holds commits nothing.**
///
/// An empty command on the undo stack is a press of Ctrl+Z that appears to do
/// nothing, which an operator reads as a broken undo rather than as a no-op.
#[test]
fn setting_the_same_override_twice_pushes_no_undo_entry() {
    let (mut s, id) = placed();
    assert!(s.set_dimension_label(id, Some("REF")).unwrap().changed);
    let depth = s.undo_depth();

    let again = s.set_dimension_label(id, Some("REF")).unwrap();
    assert!(!again.changed, "the second set changed nothing");
    assert_eq!(again.printed, "REF");
    assert_eq!(
        again.measured, "2.00 m",
        "and it still reports the measurement, because a shell asking again still owes the disclosure"
    );
    assert_eq!(s.undo_depth(), depth, "no undo entry was pushed");

    // Clearing an override that is not there is the same no-op.
    s.set_dimension_label(id, None).unwrap();
    let depth = s.undo_depth();
    assert!(!s.set_dimension_label(id, None).unwrap().changed);
    assert_eq!(s.undo_depth(), depth);
}

/// **Undo reverts the override and redo reapplies it**, as ONE command.
#[test]
fn the_override_is_one_undoable_command() {
    let (mut s, id) = placed();
    let before = baked_appearance(&s);
    s.set_dimension_label(id, Some("NOTE")).unwrap();
    assert!(baked_appearance(&s).contains("(NOTE)"));

    s.undo().unwrap();
    assert_eq!(
        baked_appearance(&s),
        before,
        "one undo takes back the whole override -- the sidecar write and the /AP rebake together"
    );
    assert_eq!(s.dimension_model().label_override(id), None);

    s.redo().unwrap();
    assert!(baked_appearance(&s).contains("(NOTE)"));
    assert_eq!(s.dimension_model().label_override(id), Some("NOTE"));
}

// ---------------------------------------------------------------------------
// 6. The clipboard route.
// ---------------------------------------------------------------------------

/// **A copied ce dimension keeps its override when pasted.**
///
/// `Pass 173.1` is the precedent and the warning: the group's SCALE was left
/// off the clip, so a pasted ce dimension read differently from the one it was
/// copied from, with nothing erroring and nothing marked. An override is the
/// same shape of fact — the caption is derived from it — so it is wired in the
/// same Pass that created it rather than after a report.
#[test]
fn the_clipboard_carries_the_override_through_a_byte_round_trip() {
    let (mut s, id) = placed();
    s.set_dimension_label(id, Some("2X <DIM>")).unwrap();

    let clip = s.copy_selection(0, &[], &[0]).unwrap();
    // Through the SERIALISED form, not the in-memory clip: the operator copies
    // in one build and pastes in the other, so the bytes are the real channel
    // and an in-memory-only assertion would not exercise the format at all.
    let bytes = clip.to_bytes();
    let round_tripped = pdfcer_core::vector::ObjectClip::from_bytes(&bytes).unwrap();

    let mut dest = session();
    let outcome = dest
        .paste_objects(0, &round_tripped, pdfcer_core::vector::Matrix::IDENTITY)
        .unwrap();
    let disclosures = &outcome.disclosures;
    let model = dest.dimension_model();
    let pasted = model.dimensions()[0].id;
    assert_eq!(
        model.label_override(pasted),
        Some("2X <DIM>"),
        "the override must arrive with the paste"
    );
    assert!(
        disclosures.iter().any(|d| d.contains("TEXT OVERRIDE")),
        "and the paste must SAY the caption is not the measurement (rule 4); got {disclosures:?}"
    );
}

// ---------------------------------------------------------------------------
// 7. The content-dependent sidecar version.
// ---------------------------------------------------------------------------

/// **A document with no override is still written at sidecar version 3.**
///
/// The interop half, and the reason `serialize_model` computes a version
/// rather than emitting the constant. A blanket bump would lock the older
/// build — which the operator deliberately keeps running out of the other
/// folder — out of WRITING every document the new build had ever touched, in
/// exchange for protecting a field those documents do not contain.
#[test]
fn the_sidecar_version_rises_only_for_a_document_that_uses_an_override() {
    fn stored_version(session: &EditSession) -> i64 {
        let doc = Document::from_bytes(save(session)).unwrap();
        let deref = |o: Option<&Object>| -> Object {
            match o {
                Some(Object::Reference(id)) => doc.get(*id).unwrap().value.clone(),
                Some(other) => other.clone(),
                None => panic!("missing key on the way to the sidecar"),
            }
        };
        let catalog = doc.catalog().unwrap();
        let piece = deref(catalog.get(b"PieceInfo"));
        let pdfcer = deref(piece.as_dict().unwrap().get(b"pdfcer"));
        let private = deref(pdfcer.as_dict().unwrap().get(b"Private"));
        pdfcer_core::dimension::sidecar_version(&private).unwrap()
    }

    let (mut s, id) = placed();
    assert_eq!(
        stored_version(&s),
        3,
        "a dimensioned document that uses no Pass 175.0 feature keeps writing version 3"
    );

    s.set_dimension_label(id, Some("REF")).unwrap();
    assert_eq!(
        stored_version(&s),
        pdfcer_core::dimension::SIDECAR_VERSION,
        "an override raises the version, so an older build refuses to WRITE over it"
    );

    // And clearing it lowers the version again: no post-3 feature is in use,
    // so the older build can safely have the file back.
    s.set_dimension_label(id, None).unwrap();
    assert_eq!(
        stored_version(&s),
        3,
        "the version tracks what the document USES, in both directions"
    );
}

/// **A sidecar carrying an unknown-to-us `/LabelOverride` still parses**, and
/// a stored override round-trips through the serializer unchanged.
///
/// The deserialiser half, asserted directly on the codec rather than through a
/// session, so a failure points at the format rather than at the verb.
#[test]
fn the_override_round_trips_through_the_sidecar_codec() {
    let (mut s, id) = placed();
    // A caption using the full accepted repertoire: WinAnsi-representable
    // non-ASCII, which forces the PDFDocEncoding branch of the text-string
    // encoder and would come back as mojibake from a raw-UTF-8 write.
    s.set_dimension_label(id, Some("45\u{00B0} \u{00B1}1 caf\u{00E9}"))
        .unwrap();

    let bytes = save(&s);
    let reopened = EditSession::new(Document::from_bytes(bytes).unwrap());
    let model = reopened.dimension_model();
    assert_eq!(
        model.label_override(id),
        Some("45\u{00B0} \u{00B1}1 caf\u{00E9}"),
        "a non-ASCII override must survive the PDF text-string round trip verbatim"
    );

    // Re-serialising the reopened model must reproduce the same object, which
    // is what makes a no-change save a no-op (R34).
    let again = pdfcer_core::dimension::serialize_model(&model);
    let parsed = deserialize_model(&again).unwrap();
    assert_eq!(
        parsed.label_override(id),
        model.label_override(id),
        "the codec is its own inverse for this key"
    );
}
