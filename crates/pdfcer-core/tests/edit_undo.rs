//! # Mutation writer + command log: the Pass 3.1 contracts, end to end
//!
//! Whole-file coverage of `pdfcer-core`'s **first editing capability**,
//! driven only through the public API: `edit::EditSession` for the
//! mutation and undo/redo, `writer::save_incremental` / `save_full` for
//! the bytes. Unit tests next to the code cover the pieces; this file
//! proves they compose into files that are *correct in the specific ways
//! the round-trip invariant cares about*.
//!
//! ## The key test, and why it is the Pass's reason to exist
//!
//! **edit → undo → save must produce a file byte-identical to the
//! input.** `ARCHITECTURE.md` §11.1 warns that computing the dirty set
//! as "the union of every object any command touched" instead of "what
//! currently differs from the base" *"would silently violate the
//! minimal-diff promise the moment undo is involved"*. That bug produces
//! a file which loads perfectly, renders identically, and carries a
//! spurious revision restating an object's original value — invisible to
//! every test except a byte comparison.
//!
//! So the assertion is `output == input`, and it is deliberately made
//! **after a real save through the real writer**, not against
//! `DirtySet::is_empty()`. An "empty-ish" dirty set — one holding a
//! net-zero entry — passes the cheap check and fails this one.
//!
//! ## The minimal-diff proof for a *real* mutation
//!
//! Pass 3.0 could only prove that an untouched file stays untouched.
//! With edits, a stronger and previously unmeasurable claim becomes
//! testable, and it is the one Acrobat users actually depend on:
//!
//! > adding one change to a document does not perturb anything else.
//!
//! Split into two assertions that are checked separately throughout:
//!
//! 1. the edited object **did** change, in the way that was asked for
//!    (proved by reloading and reading the value back — not by trusting
//!    the writer's own report);
//! 2. every **other** object is still byte-verbatim, and for an
//!    incremental save every byte below the original EOF is untouched
//!    (§7.5.6, which is simultaneously the signature-safety property of
//!    §12.8.1 NOTE 1).
//!
//! ## Fixtures are synthesized, with provenance
//!
//! Same discipline as `writer_roundtrip.rs`: `docs/LEGAL.md` §5 forbids
//! checked-in real-world PDFs, and a cross-reference stream contains the
//! byte offset *of itself*, which any hand-edited fixture breaks the
//! moment a line above it moves. Each builder names the clause it
//! models.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::io::Write as _;

use pdfcer_core::document::Document;
use pdfcer_core::edit::{CommandKind, EditSession, InfoField};
use pdfcer_core::object::{ObjId, Object, Provenance};
use pdfcer_core::writer::{DirtySet, ProducerPolicy, SaveOptions, save_full, save_incremental};

// ---------------------------------------------------------------------------
// Fixture builders
// ---------------------------------------------------------------------------

/// Big-endian encoding in `width` bytes (§7.5.8.3: *"each field shall be
/// stored with the high-order byte first"*).
fn be(value: u64, width: usize) -> Vec<u8> {
    (0..width)
        .rev()
        .map(|i| ((value >> (i * 8)) & 0xFF) as u8)
        .collect()
}

fn pack(rows: &[(u64, u64, u64)], w: [usize; 3]) -> Vec<u8> {
    let mut out = Vec::new();
    for &(t, f2, f3) in rows {
        out.extend(be(t, w[0]));
        out.extend(be(f2, w[1]));
        out.extend(be(f3, w[2]));
    }
    out
}

fn zlib(data: &[u8]) -> Vec<u8> {
    let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    e.write_all(data).unwrap();
    e.finish().unwrap()
}

/// A classic §7.5.4 / §7.5.5 file: one `xref` table, one `trailer`,
/// three pages, and optionally an `/Info` dictionary and an `/ID`.
///
/// Three pages rather than one because the minimal-diff assertion is
/// about the pages that were **not** edited — with a single page there
/// is nothing for a perturbation to show up in.
fn classic_pdf(info: bool, id: bool) -> Vec<u8> {
    let mut bodies: Vec<(u32, String)> = vec![
        (1, "<< /Type /Catalog /Pages 2 0 R >>".to_owned()),
        (
            2,
            "<< /Type /Pages /Kids [3 0 R 4 0 R 5 0 R] /Count 3 >>".to_owned(),
        ),
    ];
    for (i, num) in [3u32, 4, 5].iter().enumerate() {
        bodies.push((
            *num,
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} 100] /Resources << >> >>",
                200 + i * 10
            ),
        ));
    }
    if info {
        bodies.push((
            6,
            "<< /Producer (Original Producer) /Title (T) >>".to_owned(),
        ));
    }

    let mut buf = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets: Vec<(u32, usize)> = Vec::new();
    for (num, body) in &bodies {
        offsets.push((*num, buf.len()));
        buf.extend_from_slice(format!("{num} 0 obj\n{body}\nendobj\n").as_bytes());
    }
    let max = bodies.iter().map(|(n, _)| *n).max().unwrap();
    let xref_at = buf.len();
    buf.extend_from_slice(format!("xref\n0 {}\n", max + 1).as_bytes());
    buf.extend_from_slice(b"0000000000 65535 f \n");
    for num in 1..=max {
        match offsets.iter().find(|(n, _)| *n == num) {
            Some((_, off)) => buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes()),
            None => buf.extend_from_slice(b"0000000000 65535 f \n"),
        }
    }
    let info_key = if info { " /Info 6 0 R" } else { "" };
    let id_key = if id { " /ID [<0102> <0304>]" } else { "" };
    buf.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R{info_key}{id_key} >>\nstartxref\n{xref_at}\n%%EOF\n",
            max + 1
        )
        .as_bytes(),
    );
    buf
}

/// A pure cross-reference-stream file (§7.5.8.1) whose **page objects
/// live inside an object stream** (§7.5.7).
///
/// This is the fixture that makes R38 promotion reachable: editing a
/// page here means editing an object with `Provenance::ObjectStream` and
/// therefore no verbatim bytes to patch.
fn objstm_pdf() -> Vec<u8> {
    let mut buf = b"%PDF-1.5\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets: Vec<(u32, usize)> = Vec::new();

    let mut push = |buf: &mut Vec<u8>, num: u32, body: &str| {
        offsets.push((num, buf.len()));
        buf.extend_from_slice(format!("{num} 0 obj\n{body}\nendobj\n").as_bytes());
    };
    push(&mut buf, 1, "<< /Type /Catalog /Pages 2 0 R >>");
    push(
        &mut buf,
        2,
        "<< /Type /Pages /Kids [3 0 R 5 0 R] /Count 2 >>",
    );

    // §7.5.7: N pairs `objnum offset` (offsets relative to /First), then
    // the values, with no obj/endobj framing.
    let inner: [(u32, &str); 2] = [
        (
            3,
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Resources << >> >>",
        ),
        (
            5,
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 100] /Resources << >> >>",
        ),
    ];
    let mut header = String::new();
    let mut body = String::new();
    for (num, text) in inner {
        header.push_str(&format!("{num} {} ", body.len()));
        body.push_str(text);
        body.push(' ');
    }
    let first = header.len();
    let data = format!("{header}{body}");
    let objstm = format!(
        "<< /Type /ObjStm /N 2 /First {first} /Length {} >>\nstream\n{data}\nendstream",
        data.len()
    );
    push(&mut buf, 4, &objstm);

    let xref_num = 6u32;
    let xref_at = buf.len();
    offsets.push((xref_num, xref_at));
    let size = xref_num + 1;
    let w = [1usize, 4, 2];
    let rows: Vec<(u64, u64, u64)> = (0..size)
        .map(|num| match num {
            0 => (0, 0, 65_535),
            3 => (2, 4, 0),
            5 => (2, 4, 1),
            _ => offsets
                .iter()
                .find(|(n, _)| *n == num)
                .map_or((0, 0, 0), |(_, off)| (1, u64::try_from(*off).unwrap(), 0)),
        })
        .collect();
    let data = zlib(&pack(&rows, w));
    let dict = format!(
        "<< /Type /XRef /Size {size} /W [1 4 2] /Root 1 0 R /Filter /FlateDecode \
/Length {} /ID [<AA> <BB>] >>",
        data.len()
    );
    buf.extend_from_slice(format!("{xref_num} 0 obj\n{dict}\nstream\n").as_bytes());
    buf.extend_from_slice(&data);
    buf.extend_from_slice(b"\nendstream\nendobj\n");
    buf.extend_from_slice(format!("startxref\n{xref_at}\n%%EOF\n").as_bytes());
    buf
}

// ---------------------------------------------------------------------------
// Shared assertions
// ---------------------------------------------------------------------------

fn session(bytes: &[u8]) -> EditSession {
    EditSession::new(Document::from_bytes(bytes.to_vec()).unwrap())
}

/// Save incrementally through the real writer and return the bytes.
fn save(session: &EditSession) -> Vec<u8> {
    session
        .to_incremental_bytes(&SaveOptions::identity())
        .unwrap()
        .0
}

/// Assert the minimal-diff property for an appended revision: every byte
/// below the base file's EOF is untouched, and every object the edit did
/// **not** name still resolves, through the reloaded file's own
/// cross-reference table, to its original definition bytes.
///
/// Resolving through the reload rather than searching the output is the
/// stronger claim (and the linear one): it proves the bytes are still
/// *reachable*, which is what §5 promises, rather than merely present.
fn assert_only_the_named_objects_changed(base: &[u8], out: &[u8], edited: &[ObjId]) {
    assert!(
        out.starts_with(base) || (out.get(..base.len()) == Some(base) && out.len() > base.len()),
        "an append modified bytes below the original EOF (§7.5.6)"
    );

    let before = Document::from_bytes(base.to_vec()).unwrap();
    let after = Document::from_bytes(out.to_vec()).unwrap();
    for io in before.objects() {
        if edited.contains(&io.id) {
            continue;
        }
        let Provenance::File(span) = io.provenance else {
            // A compressed object's bytes live in its container, which
            // is itself checked as an ordinary file-level object.
            continue;
        };
        let want = span.slice(before.bytes());
        let got = after
            .get(io.id)
            .and_then(|o| o.file_span())
            .and_then(|s| s.slice(after.bytes()));
        assert_eq!(
            got, want,
            "object {} was perturbed by an edit that did not name it",
            io.id
        );
    }
}

/// The trailer's `/ID` as a pair of byte strings, if it has a
/// well-formed one.
fn file_id(doc: &Document) -> Option<(Vec<u8>, Vec<u8>)> {
    let Some(Object::Array(items)) = doc.trailer().get(b"ID") else {
        return None;
    };
    match (items.first(), items.get(1)) {
        (Some(Object::String(a)), Some(Object::String(b))) => Some((a.clone(), b.clone())),
        _ => None,
    }
}

fn page_rotation(doc: &Document, index: usize) -> u16 {
    pdfcer_core::page_tree::pages(doc).unwrap()[index].rotate
}

fn title(doc: &Document) -> Option<String> {
    let id = doc.trailer().get(b"Info")?.as_reference()?;
    let dict = doc.get(id)?.value.as_dict()?;
    match dict.get(b"Title")? {
        Object::String(bytes) => Some(pdfcer_core::edit::decode_text_string(bytes).text),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// THE key test: edit -> undo -> save is byte-identical
// ---------------------------------------------------------------------------

#[test]
fn rotate_then_undo_then_save_is_byte_identical() {
    // §11.1's "union of every command ever run" bug, made executable.
    // A dirty set that tracked *touched* objects rather than *differing*
    // ones would append a revision restating page 1's original value —
    // a file that loads, renders and reloads perfectly, and is wrong.
    let base = classic_pdf(true, true);
    let mut s = session(&base);
    s.set_page_rotation(0, 90).unwrap();
    assert!(s.is_modified());

    s.undo();
    assert!(!s.is_modified());
    assert_eq!(save(&s), base, "edit -> undo -> save must change nothing");
}

#[test]
fn metadata_edit_then_undo_then_save_is_byte_identical() {
    let base = classic_pdf(true, true);
    let mut s = session(&base);
    s.set_info_field(InfoField::Title, Some("A new title"))
        .unwrap();
    s.undo();
    assert_eq!(save(&s), base);
}

#[test]
fn creating_info_then_undoing_it_leaves_no_trace() {
    // The hardest undo shape in this Pass: the command created an
    // object AND changed the trailer, so reverting it has to remove
    // both. A half-revert would append a revision carrying a dangling
    // /Info reference or an orphan object.
    let base = classic_pdf(false, true);
    let mut s = session(&base);
    s.set_info_field(InfoField::Title, Some("Invented"))
        .unwrap();
    assert!(s.is_modified());
    s.undo();
    assert!(!s.is_modified());
    assert_eq!(save(&s), base);
}

#[test]
fn many_edits_all_undone_still_save_byte_identically() {
    // The property has to survive an arbitrary history, not just one
    // command — a dirty set that leaked one entry out of twelve is the
    // realistic version of this bug.
    let base = classic_pdf(true, true);
    let mut s = session(&base);
    for page in 0..3 {
        s.rotate_page_by(page, 90).unwrap();
        s.rotate_page_by(page, 180).unwrap();
    }
    for field in InfoField::all() {
        s.set_info_field(field, Some("x")).unwrap();
    }
    s.set_info_field(InfoField::Title, None).unwrap();
    assert!(s.is_modified());

    while s.can_undo() {
        s.undo();
    }
    assert!(!s.is_modified());
    assert_eq!(save(&s), base);
}

#[test]
fn undo_then_redo_reproduces_exactly_the_same_bytes() {
    // Redo must be a faithful re-application, not an approximation:
    // the two saves are compared byte-for-byte, which also pins the
    // determinism of the /ID[1] derivation.
    let base = classic_pdf(true, true);
    let mut s = session(&base);
    s.set_page_rotation(1, 270).unwrap();
    s.set_info_field(InfoField::Author, Some("Ada")).unwrap();
    let direct = save(&s);

    s.undo();
    s.undo();
    assert_eq!(save(&s), base);
    s.redo();
    s.redo();
    assert_eq!(save(&s), direct, "redo must reproduce the same file");
}

#[test]
fn undo_on_an_object_stream_file_is_byte_identical_too() {
    // The promotion path (R38) must be as reversible as any other: an
    // undone promotion leaves the compressed object exactly where it
    // was, with no type-1 entry superseding its type-2 one.
    let base = objstm_pdf();
    let mut s = session(&base);
    s.set_page_rotation(0, 90).unwrap();
    assert!(s.is_modified());
    s.undo();
    assert_eq!(save(&s), base);
}

// ---------------------------------------------------------------------------
// edit -> save -> reload -> verify
// ---------------------------------------------------------------------------

#[test]
fn a_rotation_survives_a_save_and_a_reload() {
    let base = classic_pdf(true, true);
    let mut s = session(&base);
    s.set_page_rotation(1, 90).unwrap();
    let out = save(&s);

    let back = Document::from_bytes(out.clone()).unwrap();
    assert_eq!(page_rotation(&back, 1), 90, "the edit must be in the file");
    assert_eq!(page_rotation(&back, 0), 0, "and only in the edited page");
    assert_eq!(page_rotation(&back, 2), 0);
    assert_only_the_named_objects_changed(&base, &out, &[ObjId::new(4, 0)]);
}

#[test]
fn a_metadata_edit_survives_a_save_and_a_reload() {
    let base = classic_pdf(true, true);
    let mut s = session(&base);
    s.set_info_field(InfoField::Title, Some("Quarterly Report"))
        .unwrap();
    let out = save(&s);

    let back = Document::from_bytes(out.clone()).unwrap();
    assert_eq!(title(&back).as_deref(), Some("Quarterly Report"));
    assert_only_the_named_objects_changed(&base, &out, &[ObjId::new(6, 0)]);
}

#[test]
fn non_ascii_metadata_survives_a_save_and_a_reload() {
    // §7.9.2's UTF-16BE + BOM escape hatch, end to end through the
    // string serializer's hex form.
    let base = classic_pdf(true, true);
    let mut s = session(&base);
    s.set_info_field(InfoField::Subject, Some("Café — 日本語"))
        .unwrap();
    let out = save(&s);

    let back = Document::from_bytes(out).unwrap();
    let id = back.trailer().get(b"Info").unwrap().as_reference().unwrap();
    let dict = back.get(id).unwrap().value.as_dict().unwrap();
    let Some(Object::String(bytes)) = dict.get(b"Subject") else {
        panic!("subject missing");
    };
    let decoded = pdfcer_core::edit::decode_text_string(bytes);
    assert_eq!(decoded.text, "Café — 日本語");
    assert!(decoded.exact);
}

#[test]
fn creating_info_produces_a_reloadable_document_with_the_reference() {
    // Table 15: /Info "shall be an indirect reference". A created
    // dictionary that the trailer does not point at is invisible.
    let base = classic_pdf(false, true);
    let mut s = session(&base);
    s.set_info_field(InfoField::Author, Some("Grace")).unwrap();
    let out = save(&s);

    let back = Document::from_bytes(out.clone()).unwrap();
    let info_ref = back.trailer().get(b"Info").unwrap();
    assert!(info_ref.as_reference().is_some(), "must be indirect");
    let dict = back.resolve(info_ref).as_dict().unwrap();
    let Some(Object::String(author)) = dict.get(b"Author") else {
        panic!("author missing after reload");
    };
    assert_eq!(author, b"Grace");
    // /Size must cover the created object or readers ignore it (§7.5.5).
    let size = back.trailer().get(b"Size").unwrap().as_int().unwrap();
    let created = info_ref.as_reference().unwrap().num;
    assert!(
        size > i64::from(created),
        "/Size {size} hides object {created}"
    );
    assert_only_the_named_objects_changed(&base, &out, &[]);
}

#[test]
fn clearing_a_field_removes_it_from_the_saved_file() {
    let base = classic_pdf(true, true);
    let mut s = session(&base);
    s.set_info_field(InfoField::Title, None).unwrap();
    let out = save(&s);

    let back = Document::from_bytes(out).unwrap();
    assert_eq!(title(&back), None);
    // The other entries of the same dictionary survive.
    let id = back.trailer().get(b"Info").unwrap().as_reference().unwrap();
    let dict = back.get(id).unwrap().value.as_dict().unwrap();
    assert!(dict.contains_key(b"Producer"));
}

// ---------------------------------------------------------------------------
// Minimal diff, coalescing, and what the report says
// ---------------------------------------------------------------------------

#[test]
fn several_edits_to_one_object_produce_one_object_in_the_update() {
    // Coalescing is structural (a map keyed by object id), and the
    // observable consequence is here: four field edits, one object
    // definition appended, not four.
    let base = classic_pdf(true, true);
    let mut s = session(&base);
    for field in InfoField::all() {
        s.set_info_field(field, Some("value")).unwrap();
    }
    let (out, report) = s.to_incremental_bytes(&SaveOptions::identity()).unwrap();
    assert_eq!(report.objects_written, 1);
    assert_eq!(s.undo_depth(), 4);

    // ...and the appended revision really does define it once.
    let tail = out.get(base.len()..).unwrap();
    let text = String::from_utf8_lossy(tail);
    assert_eq!(text.matches("6 0 obj").count(), 1, "{text}");
}

#[test]
fn edits_to_several_objects_append_exactly_those_objects() {
    let base = classic_pdf(true, true);
    let mut s = session(&base);
    s.set_page_rotation(0, 90).unwrap();
    s.set_page_rotation(2, 180).unwrap();
    s.set_info_field(InfoField::Title, Some("Three")).unwrap();

    let (out, report) = s.to_incremental_bytes(&SaveOptions::identity()).unwrap();
    assert_eq!(report.objects_written, 3);
    assert_eq!(report.objects_verbatim, 0, "every written object changed");
    assert_eq!(report.objects_reserialized, 3);
    assert_only_the_named_objects_changed(
        &base,
        &out,
        &[ObjId::new(3, 0), ObjId::new(5, 0), ObjId::new(6, 0)],
    );

    // The untouched middle page must not appear in the update section.
    let tail = String::from_utf8_lossy(out.get(base.len()..).unwrap()).into_owned();
    assert!(
        !tail.contains("4 0 obj"),
        "an unedited page was rewritten: {tail}"
    );
}

#[test]
fn a_full_rewrite_applies_edits_and_keeps_everything_else_verbatim() {
    // The same minimal-diff claim in the other save mode, where it is
    // per-object rather than per-file (R32).
    let base = classic_pdf(true, true);
    let mut s = session(&base);
    s.set_page_rotation(1, 90).unwrap();

    let (out, report) = s.to_full_bytes(&SaveOptions::identity()).unwrap();
    assert_eq!(report.objects_reserialized, 1, "only the edited object");
    let before = Document::from_bytes(base.clone()).unwrap();
    let after = Document::from_bytes(out).unwrap();
    assert_eq!(page_rotation(&after, 1), 90);
    for io in before.objects() {
        if io.id == ObjId::new(4, 0) {
            continue;
        }
        let want = io.file_span().unwrap().slice(before.bytes());
        let got = after
            .get(io.id)
            .and_then(|o| o.file_span())
            .and_then(|s| s.slice(after.bytes()));
        assert_eq!(got, want, "object {} lost its verbatim bytes", io.id);
    }
}

// ---------------------------------------------------------------------------
// R38 — promotion out of an object stream (decision 007 W3)
// ---------------------------------------------------------------------------

#[test]
fn editing_a_compressed_object_promotes_it_and_says_so() {
    let base = objstm_pdf();
    let before = Document::from_bytes(base.clone()).unwrap();
    assert!(
        matches!(
            before.get(ObjId::new(3, 0)).unwrap().provenance,
            Provenance::ObjectStream { .. }
        ),
        "fixture precondition: page 1 must be compressed"
    );

    let mut s = session(&base);
    s.set_page_rotation(0, 90).unwrap();
    let (out, report) = s.to_incremental_bytes(&SaveOptions::identity()).unwrap();

    // Counted AND named — R38's diagnostic obligation.
    assert_eq!(report.promoted, vec![ObjId::new(3, 0)]);
    assert_eq!(report.objects_reserialized, 1);

    let after = Document::from_bytes(out.clone()).unwrap();
    // The promoted object is now file-level, and the edit took.
    assert!(matches!(
        after.get(ObjId::new(3, 0)).unwrap().provenance,
        Provenance::File(_)
    ));
    assert_eq!(page_rotation(&after, 0), 90);
    // Its neighbour inside the same container is untouched and still
    // compressed — the whole point of promoting rather than rewriting
    // the container.
    assert!(matches!(
        after.get(ObjId::new(5, 0)).unwrap().provenance,
        Provenance::ObjectStream { .. }
    ));
    assert_eq!(page_rotation(&after, 1), 0);
    // And the container itself was not re-emitted at all.
    let tail = String::from_utf8_lossy(out.get(base.len()..).unwrap()).into_owned();
    assert!(
        !tail.contains("4 0 obj"),
        "the container was rewritten: {tail}"
    );
}

#[test]
fn a_full_rewrite_also_promotes_an_edited_compressed_object() {
    // If it did not, the container would be copied through with the OLD
    // value and the edit would silently vanish — a plausible, working,
    // wrong file.
    let base = objstm_pdf();
    let mut s = session(&base);
    s.set_page_rotation(1, 180).unwrap();

    let (out, report) = s.to_full_bytes(&SaveOptions::identity()).unwrap();
    assert_eq!(report.promoted, vec![ObjId::new(5, 0)]);
    let after = Document::from_bytes(out).unwrap();
    assert_eq!(page_rotation(&after, 1), 180);
    assert_eq!(page_rotation(&after, 0), 0);
    assert!(matches!(
        after.get(ObjId::new(5, 0)).unwrap().provenance,
        Provenance::File(_)
    ));
}

#[test]
fn an_identity_reemission_of_a_compressed_object_is_reported_as_a_promotion() {
    // Pass 3.0 counted this only as `objects_reserialized`. It is a
    // representation change to an object nobody edited, so it is named.
    let base = objstm_pdf();
    let doc = Document::from_bytes(base).unwrap();
    let (_, report) = save_incremental(
        &doc,
        &DirtySet::identity_reemission([ObjId::new(3, 0)]),
        &SaveOptions::identity(),
    )
    .unwrap();
    assert_eq!(report.promoted, vec![ObjId::new(3, 0)]);
}

// ---------------------------------------------------------------------------
// §14.4 — /ID discipline, now reachable for the first time
// ---------------------------------------------------------------------------

#[test]
fn a_changed_object_regenerates_id_one_and_never_id_zero() {
    let base = classic_pdf(true, true);
    let original = file_id(&Document::from_bytes(base.clone()).unwrap()).unwrap();

    let mut s = session(&base);
    s.set_page_rotation(0, 90).unwrap();
    let after = Document::from_bytes(save(&s)).unwrap();
    let updated = file_id(&after).unwrap();

    assert_eq!(updated.0, original.0, "ID[0] shall not change (§14.4)");
    assert_ne!(updated.1, original.1, "ID[1] must reflect the update");
    assert_eq!(updated.1.len(), 16);
}

#[test]
fn an_identity_reemission_does_not_regenerate_id_one() {
    // The distinction that keeps Pass 3.0's `append-identity` corpus
    // mode byte-stable: writing an object is not the same as changing
    // one, and only the latter is an "update" in §14.4's sense.
    let base = classic_pdf(true, true);
    let doc = Document::from_bytes(base).unwrap();
    let original = file_id(&doc).unwrap();
    let (out, _) = save_incremental(
        &doc,
        &DirtySet::identity_reemission([ObjId::new(3, 0)]),
        &SaveOptions::identity(),
    )
    .unwrap();
    let after = file_id(&Document::from_bytes(out).unwrap()).unwrap();
    assert_eq!(after, original);
}

#[test]
fn an_empty_dirty_set_does_not_regenerate_id_one() {
    let base = classic_pdf(true, true);
    let s = session(&base);
    assert_eq!(save(&s), base);
}

#[test]
fn a_file_with_no_id_does_not_gain_one() {
    // §14.4 is `should`-strength, and inventing document identity on an
    // append is a judgement the operator did not delegate. Checked in
    // both save modes because the temptation differs between them.
    let base = classic_pdf(true, false);
    let mut s = session(&base);
    s.set_page_rotation(0, 90).unwrap();

    let incremental = Document::from_bytes(save(&s)).unwrap();
    assert!(incremental.trailer().get(b"ID").is_none());

    let (full, _) = s.to_full_bytes(&SaveOptions::identity()).unwrap();
    assert!(
        Document::from_bytes(full)
            .unwrap()
            .trailer()
            .get(b"ID")
            .is_none()
    );
}

#[test]
fn id_regeneration_is_content_derived_and_deterministic() {
    // Two runs of the same edit produce the same file; a different edit
    // produces a different identifier. Both halves matter: the first is
    // what makes byte-comparison testing possible at all, the second is
    // what makes the value mean anything.
    let base = classic_pdf(true, true);

    let mut a = session(&base);
    a.set_page_rotation(0, 90).unwrap();
    let mut b = session(&base);
    b.set_page_rotation(0, 90).unwrap();
    assert_eq!(save(&a), save(&b));

    let mut c = session(&base);
    c.set_page_rotation(0, 180).unwrap();
    assert_ne!(
        file_id(&Document::from_bytes(save(&a)).unwrap()),
        file_id(&Document::from_bytes(save(&c)).unwrap())
    );
}

#[test]
fn a_full_rewrite_of_an_edited_document_also_refreshes_id_one() {
    let base = classic_pdf(true, true);
    let original = file_id(&Document::from_bytes(base.clone()).unwrap()).unwrap();
    let mut s = session(&base);
    s.set_info_field(InfoField::Title, Some("Changed")).unwrap();
    let (out, _) = s.to_full_bytes(&SaveOptions::identity()).unwrap();
    let after = file_id(&Document::from_bytes(out).unwrap()).unwrap();
    assert_eq!(after.0, original.0);
    assert_ne!(after.1, original.1);
}

// ---------------------------------------------------------------------------
// R41 — an edit is not a fingerprint
// ---------------------------------------------------------------------------

#[test]
fn a_metadata_edit_does_not_stamp_a_producer() {
    // The operator changed the title. That is not consent to have
    // pdfcer's name written into their file (decision 001 §6.1
    // obligation 6). Incremental save has no producer knob at all, and
    // must not grow one by accident here.
    let base = classic_pdf(true, true);
    let mut s = session(&base);
    s.set_info_field(InfoField::Title, Some("Changed")).unwrap();
    let out = save(&s);
    assert_eq!(
        String::from_utf8_lossy(&out).matches("pdfcer ").count(),
        0,
        "an edit must not introduce a producer string"
    );
    let back = Document::from_bytes(out).unwrap();
    let id = back.trailer().get(b"Info").unwrap().as_reference().unwrap();
    let dict = back.get(id).unwrap().value.as_dict().unwrap();
    let Some(Object::String(producer)) = dict.get(b"Producer") else {
        panic!("the original producer was dropped");
    };
    assert_eq!(producer, b"Original Producer");
}

#[test]
fn a_full_rewrite_can_still_stamp_a_producer_alongside_an_edit() {
    // The two mechanisms are independent: the policy rewrites
    // /Producer, the edit rewrites /Title, and both land in the one
    // /Info object without either clobbering the other.
    let base = classic_pdf(true, true);
    let mut s = session(&base);
    s.set_info_field(InfoField::Title, Some("Edited")).unwrap();
    let (out, _) = s
        .to_full_bytes(&SaveOptions::default().with_producer(ProducerPolicy::Set))
        .unwrap();

    let back = Document::from_bytes(out).unwrap();
    assert_eq!(title(&back).as_deref(), Some("Edited"));
    let id = back.trailer().get(b"Info").unwrap().as_reference().unwrap();
    let dict = back.get(id).unwrap().value.as_dict().unwrap();
    let Some(Object::String(producer)) = dict.get(b"Producer") else {
        panic!("producer missing");
    };
    assert!(String::from_utf8_lossy(producer).starts_with("pdfcer "));
}

// ---------------------------------------------------------------------------
// Structure preservation under mutation (R33)
// ---------------------------------------------------------------------------

#[test]
fn an_edit_never_changes_the_cross_reference_form() {
    // R33/W4: silently upgrading a classic table to a cross-reference
    // stream (or the reverse) produces a plausible, working, wrong file
    // — and an edit is exactly the moment it would happen.
    let classic = classic_pdf(true, true);
    let mut s = session(&classic);
    s.set_page_rotation(0, 90).unwrap();
    let out = save(&s);
    let tail = String::from_utf8_lossy(out.get(classic.len()..).unwrap()).into_owned();
    assert!(
        tail.contains("xref\n"),
        "classic input must stay classic: {tail}"
    );
    assert!(tail.contains("trailer\n"), "{tail}");

    let stream = objstm_pdf();
    let mut s = session(&stream);
    s.set_page_rotation(0, 90).unwrap();
    let out = save(&s);
    let tail = out.get(stream.len()..).unwrap();
    let text = String::from_utf8_lossy(tail).into_owned();
    assert!(
        !text.contains("trailer\n"),
        "a stream input must not grow a classic trailer: {text}"
    );
    assert!(text.contains("/Type /XRef"), "{text}");
}

#[test]
fn an_edited_document_does_not_raise_the_pdf_version() {
    let base = classic_pdf(true, true);
    let mut s = session(&base);
    s.set_info_field(InfoField::Title, Some("Changed")).unwrap();
    let back = Document::from_bytes(save(&s)).unwrap();
    assert_eq!(back.version().to_string(), "1.4");

    let (full, _) = s.to_full_bytes(&SaveOptions::identity()).unwrap();
    assert_eq!(
        Document::from_bytes(full).unwrap().version().to_string(),
        "1.4"
    );
}

#[test]
fn an_appended_revision_chains_prev_and_ends_with_its_own_eof() {
    // §7.5.6 requirements 3 and 4, under a real edit rather than an
    // identity re-emission.
    let base = classic_pdf(true, true);
    let doc = Document::from_bytes(base.clone()).unwrap();
    let mut s = session(&base);
    s.set_page_rotation(0, 90).unwrap();
    let out = save(&s);

    assert_eq!(String::from_utf8_lossy(&out).matches("%%EOF").count(), 2);
    let back = Document::from_bytes(out).unwrap();
    assert_eq!(
        back.trailer().get(b"Prev").and_then(Object::as_int),
        Some(i64::try_from(doc.base_startxref()).unwrap())
    );
}

#[test]
fn two_successive_edit_saves_chain_into_three_revisions() {
    // Editing, saving, reopening and editing again is the ordinary
    // operator loop, and it is where a mis-chained /Prev silently
    // resurrects superseded objects.
    let base = classic_pdf(true, true);
    let mut s = session(&base);
    s.set_page_rotation(0, 90).unwrap();
    let once = save(&s);

    let mut s2 = session(&once);
    s2.set_page_rotation(1, 180).unwrap();
    let twice = save(&s2);

    assert!(twice.starts_with(&once));
    assert_eq!(String::from_utf8_lossy(&twice).matches("%%EOF").count(), 3);
    let back = Document::from_bytes(twice).unwrap();
    assert_eq!(page_rotation(&back, 0), 90, "the first edit survived");
    assert_eq!(page_rotation(&back, 1), 180);
    assert_eq!(page_rotation(&back, 2), 0);
}

// ---------------------------------------------------------------------------
// Command-log bookkeeping observed through the writer
// ---------------------------------------------------------------------------

#[test]
fn undo_kinds_identify_what_would_be_reversed() {
    let base = classic_pdf(true, true);
    let mut s = session(&base);
    s.set_info_field(InfoField::Keywords, Some("pdf, test"))
        .unwrap();
    s.set_page_rotation(2, 90).unwrap();
    assert_eq!(
        s.undo_kind(),
        Some(CommandKind::SetPageRotation {
            page_index: 2,
            degrees: 90
        })
    );
    s.undo();
    assert_eq!(
        s.undo_kind(),
        Some(CommandKind::SetInfoField(InfoField::Keywords))
    );
    s.undo();
    assert_eq!(s.undo_kind(), None);
}

#[test]
fn a_save_does_not_consume_the_undo_history() {
    // Saving is not a commit point for undo (§11.2 makes redaction the
    // only exception, and only after the fact). An operator must be
    // able to save, look at the result, and step back.
    let base = classic_pdf(true, true);
    let mut s = session(&base);
    s.set_page_rotation(0, 90).unwrap();
    let _ = save(&s);
    assert!(s.can_undo());
    s.undo();
    assert_eq!(save(&s), base);
}

#[test]
fn a_full_rewrite_of_an_unedited_session_is_unchanged_from_pass_3_0() {
    // The identity path must be a strict subset of the edit path, not a
    // parallel one — this is the regression guard for the corpus gate.
    let base = classic_pdf(true, true);
    let doc = Document::from_bytes(base.clone()).unwrap();
    let (via_writer, a) = save_full(&doc, &DirtySet::empty(), &SaveOptions::identity()).unwrap();
    let (via_session, b) = session(&base)
        .to_full_bytes(&SaveOptions::identity())
        .unwrap();
    assert_eq!(via_writer, via_session);
    assert_eq!(a.objects_reserialized, 0);
    assert_eq!(b.objects_reserialized, 0);
    assert!(a.promoted.is_empty());
}
