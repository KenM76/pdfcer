//! # Writer round-trip: end-to-end save behaviour on every input shape
//!
//! Whole-file coverage of `pdfcer-core`'s two save modes (ISO 32000-1
//! §7.5.6 incremental update, and full rewrite) driven only through the
//! **public** API. Byte-layout unit tests live next to the code in
//! `src/writer/`; this file proves the pieces compose into files that
//! reload, and — critically — that the *shape* of the input survives.
//!
//! ## Why these fixtures are synthesized here, with provenance comments
//!
//! **There is no spec-sourced worked example of an incremental update
//! on a cross-reference-stream file or on a hybrid-reference file.**
//! That is a recorded NEGATIVE RESULT, not a gap in the corpus: Annex
//! H.7's four-stage updating example is classic-tables-only, §7.5.8.4's
//! writer text describes *creating* a hybrid rather than appending to
//! one, and §7.5.6 never mentions `/XRefStm` at all. See
//! `iso32000__s__7.5.8.md` § "WRITE DIRECTION — appending to a
//! hybrid-reference file". So the fixtures below are pdfcer's own,
//! generated at test time, each carrying a comment naming the clause it
//! models.
//!
//! Generating rather than checking in bytes is also a correctness
//! requirement, not a convenience: a cross-reference stream contains
//! the byte offset **of itself**, which any hand-edited fixture
//! silently breaks the moment a line above it changes. And
//! `docs/LEGAL.md` §5 forbids checked-in real-world PDFs of unknown
//! provenance regardless.
//!
//! ## The three assertions, kept apart on purpose
//!
//! Decision 007 W1/R32 names conflating these *"the single likeliest
//! source of a false green or a false red in this Pass"*:
//!
//! 1. `save_incremental` + empty dirty set ⇒ **whole-file** byte
//!    identity.
//! 2. `save_incremental` + a dirty set ⇒ every prior byte unchanged
//!    (`output.starts_with(input)`), which is the signature-safety
//!    property (§12.8.1 NOTE 1).
//! 3. `save_full` ⇒ **per-object-definition** byte identity, never
//!    per file — offsets legitimately move.
//!
//! ## What "never normalize" means as a test
//!
//! R33's rule is only testable by asserting on the *output's* shape.
//! Every builder below therefore has a paired assertion that a classic
//! input stays classic and a stream input stays a stream. A writer that
//! quietly upgrades a PDF 1.4 file to cross-reference streams produces
//! a file that loads perfectly and is wrong — the exact failure mode
//! decision 007 W4 describes.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::io::Write as _;

use pdfcer_core::document::Document;
use pdfcer_core::linearization::Linearization;
use pdfcer_core::object::{ObjId, Object, Provenance, equivalent_across_buffers};
use pdfcer_core::writer::{
    DirtySet, ProducerPolicy, SaveOptions, WriteError, save_full, save_incremental,
};
use pdfcer_core::xref::{SectionShape, XrefEntry};

// ---------------------------------------------------------------------------
// Fixture builders
// ---------------------------------------------------------------------------

/// One packed cross-reference-stream row: `(type, field 2, field 3)`.
type Row = (u64, u64, u64);

/// Big-endian encoding of `value` in `width` bytes (§7.5.8.3: *"each
/// field shall be stored with the high-order byte first"*).
fn be(value: u64, width: usize) -> Vec<u8> {
    (0..width)
        .rev()
        .map(|i| ((value >> (i * 8)) & 0xFF) as u8)
        .collect()
}

fn pack(rows: &[Row], w: [usize; 3]) -> Vec<u8> {
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

/// A classic §7.5.4 / §7.5.5 file: one `xref` table, one `trailer`.
///
/// Object 1 is the catalog. `info` optionally adds a `/Info` object so
/// the `/Producer` policy can be exercised.
fn build_classic_pdf(extra_objects: &[(u32, &str)], info: bool) -> Vec<u8> {
    let mut bodies: Vec<(u32, String)> = vec![
        (1, "<< /Type /Catalog /Pages 2 0 R >>".to_owned()),
        (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned()),
        (
            3,
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Resources << >> >>".to_owned(),
        ),
    ];
    if info {
        bodies.push((
            4,
            "<< /Producer (Original Producer) /Title (T) >>".to_owned(),
        ));
    }
    for (n, b) in extra_objects {
        bodies.push((*n, (*b).to_owned()));
    }
    bodies.sort_by_key(|(n, _)| *n);

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
    let info_key = if info { " /Info 4 0 R" } else { "" };
    buf.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R{info_key} /ID [<0102> <0304>] >>\nstartxref\n{xref_at}\n%%EOF\n",
            max + 1
        )
        .as_bytes(),
    );
    buf
}

/// A file that uses cross-reference streams **entirely** (§7.5.8.1:
/// *"the keywords `xref` and `trailer` shall no longer be used"*),
/// optionally with objects compressed into an object stream (§7.5.7).
///
/// The xref stream takes the next free object number and, per §7.5.8.3
/// (*"an entry for it shall exist in … usually itself"*), a type-1
/// entry pointing at its own offset.
fn build_xref_stream_pdf(with_objstm: bool) -> Vec<u8> {
    let mut buf = b"%PDF-1.5\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets: Vec<(u32, usize)> = Vec::new();
    let mut compressed: Vec<(u32, u32, u32)> = Vec::new();

    let push = |buf: &mut Vec<u8>, offsets: &mut Vec<(u32, usize)>, num: u32, body: &str| {
        offsets.push((num, buf.len()));
        buf.extend_from_slice(format!("{num} 0 obj\n{body}\nendobj\n").as_bytes());
    };

    push(
        &mut buf,
        &mut offsets,
        1,
        "<< /Type /Catalog /Pages 2 0 R >>",
    );
    push(
        &mut buf,
        &mut offsets,
        2,
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
    );

    if with_objstm {
        // §7.5.7: pairs `objnum offset` (offsets relative to /First),
        // then the values, with no obj/endobj framing.
        let inner: [(u32, &str); 2] = [
            (
                3,
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Resources << >> >>",
            ),
            (5, "<< /Kind /Compressed >>"),
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
        push(&mut buf, &mut offsets, 4, &objstm);
        compressed.push((3, 4, 0));
        compressed.push((5, 4, 1));
    } else {
        push(
            &mut buf,
            &mut offsets,
            3,
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Resources << >> >>",
        );
    }

    let xref_num = offsets
        .iter()
        .map(|(n, _)| *n)
        .chain(compressed.iter().map(|(n, _, _)| *n))
        .max()
        .unwrap()
        + 1;
    let xref_at = buf.len();
    offsets.push((xref_num, xref_at));
    let size = xref_num + 1;

    let w = [1usize, 4, 2];
    let rows: Vec<Row> = (0..size)
        .map(|num| {
            if num == 0 {
                (0, 0, 65_535)
            } else if let Some((_, off)) = offsets.iter().find(|(n, _)| *n == num) {
                (1, u64::try_from(*off).unwrap(), 0)
            } else if let Some((_, c, i)) = compressed.iter().find(|(n, _, _)| *n == num) {
                (2, u64::from(*c), u64::from(*i))
            } else {
                (0, 0, 0)
            }
        })
        .collect();
    let data = zlib(&pack(&rows, w));
    let dict = format!(
        "<< /Type /XRef /Size {size} /W [1 4 2] /Root 1 0 R /Filter /FlateDecode /Length {} /ID [<AA> <BB>] >>",
        data.len()
    );
    buf.extend_from_slice(format!("{xref_num} 0 obj\n{dict}\nstream\n").as_bytes());
    buf.extend_from_slice(&data);
    buf.extend_from_slice(b"\nendstream\nendobj\n");
    buf.extend_from_slice(format!("startxref\n{xref_at}\n%%EOF\n").as_bytes());
    buf
}

/// A §7.5.8.4 hybrid-reference file: a classic main section that marks
/// object 4 **free with generation 65535** (the spec's own hiding
/// mechanism, so a pre-1.5 reader resolves `4 0 R` to null), a
/// cross-reference stream giving object 4 a real type-1 entry, and an
/// update section whose trailer carries `/XRefStm`.
fn build_hybrid_pdf() -> Vec<u8> {
    let mut buf = b"%PDF-1.5\n".to_vec();
    let bodies: [(u32, &str); 3] = [
        (1, "<< /Type /Catalog /Pages 2 0 R /Outlines 4 0 R >>"),
        (2, "<< /Type /Pages /Kids [] /Count 0 >>"),
        (4, "<< /Type /Outlines /Count 0 >>"),
    ];
    let mut offsets: Vec<(u32, usize)> = Vec::new();
    for (num, body) in bodies {
        offsets.push((num, buf.len()));
        buf.extend_from_slice(format!("{num} 0 obj\n{body}\nendobj\n").as_bytes());
    }
    let off = |n: u32| offsets.iter().find(|(k, _)| *k == n).unwrap().1;

    let base_at = buf.len();
    buf.extend_from_slice(b"xref\n0 5\n");
    buf.extend_from_slice(b"0000000000 65535 f\r\n");
    buf.extend_from_slice(format!("{:010} 00000 n\r\n", off(1)).as_bytes());
    buf.extend_from_slice(format!("{:010} 00000 n\r\n", off(2)).as_bytes());
    buf.extend_from_slice(b"0000000000 65535 f\r\n");
    buf.extend_from_slice(b"0000000000 65535 f\r\n");
    buf.extend_from_slice(b"trailer\n<< /Size 6 /Root 1 0 R >>\n");

    let stm_at = buf.len();
    let w = [1usize, 4, 2];
    let data = pack(&[(1, u64::try_from(off(4)).unwrap(), 0)], w);
    let dict = format!(
        "<< /Type /XRef /Size 6 /W [1 4 2] /Index [4 1] /Root 1 0 R /Length {} >>",
        data.len()
    );
    buf.extend_from_slice(format!("5 0 obj\n{dict}\nstream\n").as_bytes());
    buf.extend_from_slice(&data);
    buf.extend_from_slice(b"\nendstream\nendobj\n");

    let upd_at = buf.len();
    buf.extend_from_slice(b"xref\n0 1\n0000000000 65535 f\r\n5 1\n");
    buf.extend_from_slice(format!("{stm_at:010} 00000 n\r\n").as_bytes());
    buf.extend_from_slice(
        format!("trailer\n<< /Size 6 /Root 1 0 R /XRefStm {stm_at} /Prev {base_at} >>\n")
            .as_bytes(),
    );
    buf.extend_from_slice(format!("startxref\n{upd_at}\n%%EOF\n").as_bytes());
    buf
}

// ---------------------------------------------------------------------------
// Shared assertions
// ---------------------------------------------------------------------------

/// Assertion 1 (R32): an empty dirty set produces the input, byte for
/// byte. Zero edits means zero bytes — not "the input plus an empty
/// revision".
fn assert_identity_save_is_byte_identical(bytes: &[u8]) {
    let doc = Document::from_bytes(bytes.to_vec()).unwrap();
    let (out, report) =
        save_incremental(&doc, &DirtySet::empty(), &SaveOptions::identity()).unwrap();
    assert_eq!(out, bytes, "empty-dirty-set save was not byte-identical");
    assert!(report.byte_identical);
    assert_eq!(report.bytes_appended, 0);
    assert_eq!(report.objects_written, 0);
}

/// Assertion 3 (R32): every `File`-provenance object's **definition
/// bytes** appear verbatim in a full rewrite, and the result reloads to
/// the same object graph.
fn assert_full_rewrite_is_per_object_verbatim(bytes: &[u8]) -> Vec<u8> {
    let doc = Document::from_bytes(bytes.to_vec()).unwrap();
    let (out, report) = save_full(&doc, &DirtySet::empty(), &SaveOptions::identity()).unwrap();
    assert!(
        !report.byte_identical,
        "a full rewrite is never file-identical"
    );

    for io in doc.objects() {
        let Provenance::File(span) = io.provenance else {
            continue;
        };
        // The cross-reference stream object is replaced by the new
        // section, so its OLD bytes are legitimately absent.
        if is_section_object(&doc, io.id) {
            continue;
        }
        let want = span.slice(doc.bytes()).unwrap();
        assert!(
            out.windows(want.len()).any(|w| w == want),
            "object {} lost its verbatim definition bytes in the full rewrite",
            io.id
        );
    }

    let back = Document::from_bytes(out.clone()).unwrap();
    assert_eq!(back.object_count(), doc.object_count());
    for io in doc.objects() {
        if is_section_object(&doc, io.id) {
            continue;
        }
        assert!(
            same_object(&back, &doc, io.id),
            "object {} differs after a full rewrite",
            io.id
        );
    }
    out
}

/// Assertion 2: an append leaves every prior byte untouched — the
/// property §12.8.1 NOTE 1 turns into signature safety — and the
/// result reloads to the same object graph.
fn assert_append_preserves_prior_bytes(bytes: &[u8], dirty: &[ObjId]) -> Vec<u8> {
    let doc = Document::from_bytes(bytes.to_vec()).unwrap();
    let (out, report) = save_incremental(
        &doc,
        &DirtySet::identity_reemission(dirty.iter().copied()),
        &SaveOptions::identity(),
    )
    .unwrap();
    assert!(
        out.starts_with(bytes),
        "an incremental append modified bytes below the original EOF"
    );
    assert!(report.bytes_appended > 0);
    let back = Document::from_bytes(out.clone()).unwrap();
    for io in doc.objects() {
        // The base file's cross-reference-stream object is superseded
        // by the new section, which legitimately carries a different
        // dictionary (a fresh /Prev, a delta /Index, a new /Length).
        // §7.5.6's most-recent-copy rule makes that the *point*, not a
        // regression — so it is excluded from the graph comparison and
        // covered instead by the section-shape assertions.
        if is_section_object(&doc, io.id) {
            continue;
        }
        assert!(
            same_object(&back, &doc, io.id),
            "object {} differs after an identity append",
            io.id
        );
    }
    out
}

/// Whether `id` is the object that *is* the base file's newest
/// cross-reference section (§7.5.8.1), rather than document content.
fn is_section_object(doc: &Document, id: ObjId) -> bool {
    matches!(doc.section_shape(), SectionShape::Stream { id: sid, .. } if sid == id)
}

/// Whether object `id` survived a save unchanged, compared ACROSS the
/// two buffers.
///
/// `Object`'s derived `PartialEq` compares a stream's `ByteSpan`, which
/// is position-dependent — so a save that legitimately relocates a
/// stream would report a phantom change on every stream-bearing file.
/// See `pdfcer_core::object::equivalent_across_buffers`.
fn same_object(after: &Document, before: &Document, id: ObjId) -> bool {
    match (after.get(id), before.get(id)) {
        (Some(a), Some(b)) => {
            equivalent_across_buffers(&a.value, after.bytes(), &b.value, before.bytes())
        }
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Classic files (§7.5.4 / §7.5.5 / §7.5.6)
// ---------------------------------------------------------------------------

#[test]
fn classic_file_identity_save_is_byte_identical() {
    assert_identity_save_is_byte_identical(&build_classic_pdf(&[], true));
}

#[test]
fn classic_file_append_preserves_prior_bytes() {
    let bytes = build_classic_pdf(&[], true);
    let out = assert_append_preserves_prior_bytes(&bytes, &[ObjId::new(3, 0)]);
    // §7.5.6 requirement 4: each trailer gets its own %%EOF.
    assert_eq!(count(&out, b"%%EOF"), 2);
}

#[test]
fn classic_file_full_rewrite_stays_classic() {
    // R33 in one assertion. A writer that "upgrades" this PDF 1.4 file
    // to a cross-reference stream produces a working, wrong file that
    // pre-1.5 readers cannot open.
    let bytes = build_classic_pdf(&[], true);
    let out = assert_full_rewrite_is_per_object_verbatim(&bytes);
    let back = Document::from_bytes(out.clone()).unwrap();
    assert!(matches!(
        back.section_shape(),
        SectionShape::Classic { xref_stm: None }
    ));
    assert!(out.windows(4).any(|w| w == b"xref"));
    assert!(out.windows(7).any(|w| w == b"trailer"));
    // The header line — including the §7.5.2 binary-comment line — is
    // preserved verbatim, so the version is not bumped either.
    assert!(out.starts_with(b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n"));
}

#[test]
fn classic_append_stays_classic_even_though_the_spec_would_allow_a_stream() {
    // §7.5.6 contains NO requirement that an update section match the
    // base file's form — a recorded NEGATIVE RESULT. R33 is therefore
    // pdfcer policy, and policy has to be tested.
    let bytes = build_classic_pdf(&[], true);
    let out = assert_append_preserves_prior_bytes(&bytes, &[ObjId::new(1, 0)]);
    let appended = &out[bytes.len()..];
    assert!(
        appended.windows(4).any(|w| w == b"xref"),
        "the appended section is not a classic table"
    );
    assert!(
        !appended.windows(5).any(|w| w == b"/XRef"),
        "an xref stream was appended to a classic file"
    );
}

#[test]
fn appended_trailer_copies_every_previous_key_except_prev() {
    // §7.5.6 requirement 3, and its trap: copying the old `Prev` AND
    // adding a new one is a duplicate key (§7.3.7 prohibits those).
    let bytes = build_classic_pdf(&[], true);
    let doc = Document::from_bytes(bytes.clone()).unwrap();
    let out = assert_append_preserves_prior_bytes(&bytes, &[ObjId::new(3, 0)]);
    let back = Document::from_bytes(out).unwrap();
    // Every previous key survives...
    assert!(back.trailer().contains_key(b"Root"));
    assert!(back.trailer().contains_key(b"Info"));
    assert!(back.trailer().contains_key(b"ID"));
    // ...and `Prev` names the base file's own startxref, not its /Prev.
    assert_eq!(
        back.trailer().get(b"Prev").and_then(Object::as_int),
        Some(i64::try_from(doc.base_startxref()).unwrap())
    );
}

#[test]
fn id_is_untouched_by_an_identity_save_of_either_kind() {
    // §14.4 vs the round-trip invariant (R39). If nothing changed,
    // nothing was "updated", so §14.4's trigger never fired — and no
    // `shall` anywhere requires regenerating /ID. A gratuitously
    // regenerated /ID is an observable "pdfcer touched this" signal.
    let bytes = build_classic_pdf(&[], true);
    let doc = Document::from_bytes(bytes.clone()).unwrap();

    let (inc, _) = save_incremental(&doc, &DirtySet::empty(), &SaveOptions::identity()).unwrap();
    assert_eq!(inc, bytes);

    let (app, _) = save_incremental(
        &doc,
        &DirtySet::identity_reemission([ObjId::new(3, 0)]),
        &SaveOptions::identity(),
    )
    .unwrap();
    let back = Document::from_bytes(app).unwrap();
    assert_eq!(back.trailer().get(b"ID"), doc.trailer().get(b"ID"));

    let (full, _) = save_full(&doc, &DirtySet::empty(), &SaveOptions::identity()).unwrap();
    let back = Document::from_bytes(full).unwrap();
    assert_eq!(back.trailer().get(b"ID"), doc.trailer().get(b"ID"));
}

#[test]
fn producer_policy_is_effective_and_suppressible() {
    // R41 / decision 001 §6.1 obligation 6, at its enforcement point.
    let bytes = build_classic_pdf(&[], true);
    let doc = Document::from_bytes(bytes).unwrap();

    let (preserved, rep) = save_full(&doc, &DirtySet::empty(), &SaveOptions::identity()).unwrap();
    assert_eq!(count(&preserved, b"pdfcer "), 0);
    assert_eq!(count(&preserved, b"Original Producer"), 1);
    assert_eq!(rep.objects_reserialized, 0);

    let opts = SaveOptions::default().with_producer(ProducerPolicy::Set);
    let (stamped, rep) = save_full(&doc, &DirtySet::empty(), &opts).unwrap();
    assert_eq!(count(&stamped, b"Original Producer"), 0);
    assert_eq!(rep.objects_reserialized, 1);
    assert!(Document::from_bytes(stamped).is_ok());
}

#[test]
fn incremental_save_never_rewrites_info_regardless_of_policy() {
    // The incremental mode has no producer knob BY CONSTRUCTION: it
    // never touches /Info, because doing so would append a revision to
    // a document the operator did not change.
    let bytes = build_classic_pdf(&[], true);
    let doc = Document::from_bytes(bytes.clone()).unwrap();
    for opts in [SaveOptions::default(), SaveOptions::identity()] {
        let (out, _) = save_incremental(&doc, &DirtySet::empty(), &opts).unwrap();
        assert_eq!(out, bytes);
        let (out, _) = save_incremental(
            &doc,
            &DirtySet::identity_reemission([ObjId::new(3, 0)]),
            &opts,
        )
        .unwrap();
        assert_eq!(count(&out, b"Original Producer"), 1);
        assert_eq!(count(&out, b"pdfcer "), 0);
    }
}

// ---------------------------------------------------------------------------
// Cross-reference-stream files (§7.5.8) — pdfcer's own fixtures
// ---------------------------------------------------------------------------

#[test]
fn xref_stream_file_identity_save_is_byte_identical() {
    assert_identity_save_is_byte_identical(&build_xref_stream_pdf(false));
    assert_identity_save_is_byte_identical(&build_xref_stream_pdf(true));
}

#[test]
fn xref_stream_file_append_emits_an_xref_stream_not_a_table() {
    // The fixture the standard does not supply (module docs). R33: the
    // appended section must be a cross-reference stream, because that
    // is what the base file's newest section is.
    let bytes = build_xref_stream_pdf(false);
    let out = assert_append_preserves_prior_bytes(&bytes, &[ObjId::new(3, 0)]);
    let appended = &out[bytes.len()..];
    assert!(
        appended.windows(5).any(|w| w == b"/XRef"),
        "the appended section is not a cross-reference stream"
    );
    assert!(
        !appended.windows(7).any(|w| w == b"trailer"),
        "§7.5.8.1: the `trailer` keyword shall no longer be used"
    );
    let back = Document::from_bytes(out).unwrap();
    assert!(matches!(back.section_shape(), SectionShape::Stream { .. }));
}

#[test]
fn appended_xref_stream_reuses_the_base_streams_object_number() {
    // Allocating a fresh number would raise /Size on every save, for
    // nothing: the base file already spends that number on exactly this
    // role, and §7.5.6's most-recent-copy rule makes re-use correct.
    let bytes = build_xref_stream_pdf(false);
    let doc = Document::from_bytes(bytes.clone()).unwrap();
    let SectionShape::Stream { id: base_id, .. } = doc.section_shape() else {
        panic!("fixture is not stream-shaped");
    };
    let out = assert_append_preserves_prior_bytes(&bytes, &[ObjId::new(3, 0)]);
    let back = Document::from_bytes(out).unwrap();
    let SectionShape::Stream { id: new_id, .. } = back.section_shape() else {
        panic!("append changed the section form");
    };
    assert_eq!(new_id, base_id);
    assert_eq!(
        back.trailer().get(b"Size").and_then(Object::as_int),
        doc.trailer().get(b"Size").and_then(Object::as_int)
    );
}

#[test]
fn object_streams_survive_a_full_rewrite_with_zero_promotions() {
    // Decision 007 W3's hazard, structurally avoided. A type-2 entry's
    // fields are the CONTAINER's object number and an index within it —
    // neither is a byte offset — so re-emitting the container verbatim
    // leaves every type-2 entry still correct. Nothing is promoted, and
    // no other object inside the container is perturbed.
    let bytes = build_xref_stream_pdf(true);
    let doc = Document::from_bytes(bytes.clone()).unwrap();
    assert!(matches!(
        doc.get(ObjId::new(3, 0)).unwrap().provenance,
        Provenance::ObjectStream { .. }
    ));

    let (out, report) = save_full(&doc, &DirtySet::empty(), &SaveOptions::identity()).unwrap();
    assert_eq!(
        report.objects_reserialized, 0,
        "a full rewrite promoted a compressed object; it must not"
    );

    let back = Document::from_bytes(out).unwrap();
    assert_eq!(back.object_count(), doc.object_count());
    // Still compressed, still in the same container at the same index.
    assert_eq!(
        back.get(ObjId::new(3, 0)).unwrap().provenance,
        doc.get(ObjId::new(3, 0)).unwrap().provenance
    );
    assert!(matches!(
        back.xref().get(5),
        Some(XrefEntry::InStream { .. })
    ));
    for io in doc.objects() {
        if is_section_object(&doc, io.id) {
            continue;
        }
        assert!(same_object(&back, &doc, io.id));
    }
}

#[test]
fn xref_stream_full_rewrite_stays_a_stream_and_reloads() {
    let bytes = build_xref_stream_pdf(true);
    let out = assert_full_rewrite_is_per_object_verbatim(&bytes);
    let back = Document::from_bytes(out.clone()).unwrap();
    assert!(matches!(back.section_shape(), SectionShape::Stream { .. }));
    // §7.5.8.1: no `xref`/`trailer` keywords anywhere in a pure
    // xref-stream file.
    assert!(!out.windows(7).any(|w| w == b"trailer"));
    assert!(out.starts_with(b"%PDF-1.5\n"));
}

#[test]
fn repeated_appends_to_a_stream_file_chain_correctly() {
    let mut bytes = build_xref_stream_pdf(true);
    for round in 0..3 {
        let doc = Document::from_bytes(bytes.clone()).unwrap();
        let (out, _) = save_incremental(
            &doc,
            &DirtySet::identity_reemission([ObjId::new(1, 0)]),
            &SaveOptions::identity(),
        )
        .unwrap();
        assert!(out.starts_with(&bytes), "round {round} rewrote prior bytes");
        let back = Document::from_bytes(out.clone()).unwrap();
        assert_eq!(back.object_count(), doc.object_count());
        bytes = out;
    }
    assert_eq!(count(&bytes, b"%%EOF"), 4);
}

// ---------------------------------------------------------------------------
// Hybrid-reference files (§7.5.8.4) — form A, and the full-rewrite refusal
// ---------------------------------------------------------------------------

#[test]
fn hybrid_file_identity_save_is_byte_identical() {
    assert_identity_save_is_byte_identical(&build_hybrid_pdf());
}

#[test]
fn hybrid_append_is_form_a_and_carries_xrefstm_forward() {
    // §7.5.8.4 write-direction analysis, form A. §7.5.6 requirement 3
    // ("all the entries except the Prev entry from the previous
    // trailer") REQUIRES carrying /XRefStm forward — it is such an
    // entry. Form B (dropping it) works but violates the literal text;
    // form C (promoting to a pure xref stream) destroys the pre-1.5
    // view, which is the file's entire reason for existing.
    let bytes = build_hybrid_pdf();
    let doc = Document::from_bytes(bytes.clone()).unwrap();
    assert!(doc.is_hybrid());
    let SectionShape::Classic {
        xref_stm: Some(base_stm),
    } = doc.section_shape()
    else {
        panic!("fixture is not hybrid");
    };

    let out = assert_append_preserves_prior_bytes(&bytes, &[ObjId::new(1, 0)]);
    let appended = &out[bytes.len()..];
    // Form A: a CLASSIC section.
    assert!(
        appended.windows(4).any(|w| w == b"xref"),
        "the appended section is not classic"
    );
    let back = Document::from_bytes(out).unwrap();
    assert!(back.is_hybrid(), "the hybrid property was lost on append");
    assert_eq!(
        back.section_shape(),
        SectionShape::Classic {
            xref_stm: Some(base_stm)
        }
    );
    // The hidden object is still reachable through the carried-forward
    // /XRefStm.
    assert!(back.get(ObjId::new(4, 0)).is_some());
}

#[test]
fn hybrid_full_rewrite_is_refused_by_name_not_normalized() {
    // R33 + R27's fail-clean posture applied to the writer (decision
    // 007 W11). Flattening a hybrid to a single section would silently
    // destroy its pre-1.5 readability — a plausible, working, WRONG
    // file. Refuse, name it, count it.
    let doc = Document::from_bytes(build_hybrid_pdf()).unwrap();
    let err = save_full(&doc, &DirtySet::empty(), &SaveOptions::identity()).unwrap_err();
    assert!(matches!(err, WriteError::HybridFullRewrite));
    assert!(err.to_string().contains("hybrid"));
}

// ---------------------------------------------------------------------------
// Linearization (Annex F) and signatures (§12.8)
// ---------------------------------------------------------------------------

#[test]
fn a_live_linearized_file_is_detected_and_the_save_reports_the_loss() {
    // R36 / "fuzzy, never sneaky": F.1 makes de-linearization on
    // append normative and unavoidable, but it is still an observable
    // property change the operator did not ask for.
    //
    // Fixture note: /L must equal the real file length, so the file is
    // built once to measure it and rebuilt with the true value.
    let build = |l: u64| {
        let dict = format!("<< /Linearized 1.0 /L {l} /H [100 200] /O 3 /E 400 /N 1 /T 500 >>");
        build_classic_pdf(&[(9u32, dict.as_str())], false)
    };
    // The digits of /L change the file's length, so converge on the
    // fixed point where the declared length equals the real one — which
    // is precisely Table F.1's requirement.
    let mut l = build(0).len() as u64;
    for _ in 0..4 {
        let candidate = build(l);
        if candidate.len() as u64 == l {
            break;
        }
        l = candidate.len() as u64;
    }
    let bytes = build(l);

    let doc = Document::from_bytes(bytes).unwrap();
    // The linearization dictionary must be the FIRST body object for
    // F.3.3 detection to see it; build_classic_pdf sorts by number, so
    // object 9 is last and detection correctly reports None. That is
    // itself the assertion worth having: detection is positional.
    assert_eq!(doc.linearization(), Linearization::None);

    // Now the positional case, hand-built.
    let mut lin = b"%PDF-1.4\n".to_vec();
    let head_len = lin.len();
    let make = |total: u64| {
        format!("1 0 obj\n<< /Linearized 1.0 /L {total} /H [1 2] /O 3 /E 4 /N 1 /T 5 >>\nendobj\n")
    };
    let mut total = (head_len + make(0).len()) as u64;
    for _ in 0..4 {
        let t = (head_len + make(total).len()) as u64;
        if t == total {
            break;
        }
        total = t;
    }
    lin.extend_from_slice(make(total).as_bytes());
    assert_eq!(lin.len() as u64, total);
    assert_eq!(
        pdfcer_core::linearization::detect(&lin),
        Linearization::Live {
            declared_length: total
        }
    );
    assert!(
        pdfcer_core::linearization::detect(&lin).save_invalidates_fast_web_view(),
        "a live linearized file must warn on save"
    );
    // pdfcer never strips the dictionary and never patches /L.
    assert!(lin.windows(11).any(|w| w == b"/Linearized"));
}

#[test]
fn a_signature_shaped_object_is_copied_verbatim_never_reserialized() {
    // §12.8: a signature's /Contents is a fixed-width placeholder
    // covered by a /ByteRange of byte offsets. Re-serializing it — even
    // "identically" — is a byte-offset hazard, so pdfcer's structural
    // answer is that such objects ride the verbatim path like any other
    // File-provenance object and are never decomposed.
    //
    // The hex string below is deliberately padded exactly as a real
    // placeholder is; a re-serializer that trimmed it, or that changed
    // its case, would change the signed byte range.
    let sig = "<< /Type /Sig /Filter /Adobe.PPKLite /ByteRange [0 840 960 240] \
               /Contents <00ff00FF0000000000000000> >>";
    let bytes = build_classic_pdf(&[(5, sig)], true);
    let doc = Document::from_bytes(bytes.clone()).unwrap();

    let needle = b"/Contents <00ff00FF0000000000000000>";
    // Full rewrite: bytes preserved exactly, including mixed-case hex.
    let (full, rep) = save_full(&doc, &DirtySet::empty(), &SaveOptions::identity()).unwrap();
    assert_eq!(count(&full, needle), 1);
    assert_eq!(rep.objects_reserialized, 0);
    // Incremental identity re-emission of the signature object itself:
    // the appended copy is byte-for-byte the original.
    let (app, rep) = save_incremental(
        &doc,
        &DirtySet::identity_reemission([ObjId::new(5, 0)]),
        &SaveOptions::identity(),
    )
    .unwrap();
    assert_eq!(count(&app, needle), 2);
    assert_eq!(rep.objects_verbatim, 1);
    assert_eq!(rep.objects_reserialized, 0);
}

// ---------------------------------------------------------------------------
// Structural edge cases
// ---------------------------------------------------------------------------

#[test]
fn a_base_file_with_no_trailing_eol_gets_a_separator_before_the_append() {
    // §7.2.3: a comment runs "up to but not including the end of the
    // line", so an appended `1 0 obj` fused onto an unterminated %%EOF
    // is swallowed whole and the update vanishes silently.
    let mut bytes = build_classic_pdf(&[], false);
    while matches!(bytes.last(), Some(b'\n' | b'\r')) {
        bytes.pop();
    }
    let out = assert_append_preserves_prior_bytes(&bytes, &[ObjId::new(3, 0)]);
    assert_eq!(out[bytes.len()], b'\n');
}

#[test]
fn a_full_rewrite_drops_bytes_before_the_header() {
    // REVERSED 2026-08-07. This test used to assert the opposite — that a
    // full rewrite carries a preamble through — on the reasoning that
    // pdfcer's probe tolerates bytes before `%PDF-` and §5 says do not
    // normalize what the operator did not ask about. The emitted offsets
    // were absolute from byte 0, exactly as §7.5.4/§7.5.5 require.
    //
    // That was self-consistent and spec-literal and still produced files
    // an independent reader could not open. Measured with veraPDF: a
    // minimal 3-object file with correct absolute offsets and 19 bytes of
    // junk ahead of the header fails with "can not locate xref table";
    // the identical file with the junk removed parses clean. veraPDF
    // treats offsets as HEADER-RELATIVE whenever a preamble exists, so
    // preserving one is a defect generator, not a courtesy.
    //
    // `iso32000__s__7.5.md` records this as a real ambiguity that ISO
    // 32000-1 does not resolve. Dropping the preamble makes the two
    // readings COINCIDE — with the header at byte 0 the absolute and
    // header-relative offsets are the same number — so the output is
    // unambiguous to every reader rather than correct only under the
    // reading pdfcer happens to prefer. It also stops re-emitting the
    // §7.5.2 violation ("The first line of a PDF file shall be a header")
    // that the operator never asked pdfcer to preserve.
    //
    // Only a FULL rewrite may do this: it promises per-object identity,
    // not whole-file identity. `assert_identity_save_is_byte_identical`
    // below still pins the incremental path, which must keep the preamble.
    //
    // Built byte-wise rather than by rewriting text: the §7.5.2 binary
    // comment line is deliberately not valid UTF-8.
    const JUNK: &[u8] = b"JUNK-BEFORE-HEADER\n";
    const HEADER: &[u8] = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n";
    let mut buf = JUNK.to_vec();
    let mut offsets: Vec<(u32, usize)> = Vec::new();
    buf.extend_from_slice(HEADER);
    for (num, body) in [
        (1u32, "<< /Type /Catalog /Pages 2 0 R >>"),
        (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        (
            3,
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 9 9] /Resources << >> >>",
        ),
    ] {
        offsets.push((num, buf.len()));
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

    let doc = Document::from_bytes(buf.clone()).unwrap();
    assert_eq!(doc.object_count(), 3);
    assert_identity_save_is_byte_identical(&buf);

    let (out, _) = save_full(&doc, &DirtySet::empty(), &SaveOptions::identity()).unwrap();

    // The header is at byte 0 and the junk is gone entirely — asserted on
    // BYTES, not through pdfcer's reader (R159). pdfcer reads its own
    // preamble-bearing output perfectly; that is precisely why this defect
    // needed an outside judge to surface at all.
    assert!(
        out.starts_with(HEADER),
        "a full rewrite must emit `%PDF-` at byte 0, got {:?}",
        &out[..out.len().min(24)]
    );
    assert!(
        !out.windows(JUNK.len()).any(|w| w == JUNK),
        "the preamble must not survive anywhere in the rewritten file"
    );

    let back = Document::from_bytes(out.clone()).unwrap();
    assert_eq!(back.object_count(), 3);
    // The catalog must actually resolve — the assertion that catches an
    // offset base-point error, which object_count alone would not.
    assert!(back.catalog().is_ok());

    // Every in-use offset must land exactly on its own `N 0 obj`. This is
    // the assertion with teeth: it fails both if the offsets keep a
    // now-absent prefix baked in, and if the header were dropped without
    // recomputing them. `object_count` and `catalog()` both pass in that
    // state, because pdfcer's own loader recovers from it.
    for num in 1u32..=3 {
        let Some(XrefEntry::InUse { offset, .. }) = back.xref().get(num) else {
            panic!("object {num} has no in-use entry");
        };
        let at = offset as usize;
        let want = format!("{num} 0 obj");
        assert!(
            out[at..].starts_with(want.as_bytes()),
            "xref says object {num} is at byte {at}, but that is {:?}",
            String::from_utf8_lossy(&out[at..(at + 12).min(out.len())])
        );
    }
}

#[test]
fn a_full_rewrite_of_a_sparse_file_covers_every_object_number() {
    // §7.5.4: the union of all sections "shall contain one entry for
    // each object number from 0 to the maximum object number defined in
    // the file". A full rewrite is ONE section, so the obligation lands
    // entirely on it — holes become free entries.
    let bytes = build_classic_pdf(&[(9, "<< /Sparse true >>")], false);
    let doc = Document::from_bytes(bytes).unwrap();
    let (out, _) = save_full(&doc, &DirtySet::empty(), &SaveOptions::identity()).unwrap();
    let text = String::from_utf8_lossy(&out).into_owned();
    assert!(text.contains("xref\n0 10\n"), "{text}");
    let back = Document::from_bytes(out).unwrap();
    assert!(back.get(ObjId::new(9, 0)).is_some());
    // Every xref entry in a classic table is exactly 20 bytes, so the
    // table body is exactly 10 * 20.
    let start = text.find("xref\n0 10\n").unwrap() + "xref\n0 10\n".len();
    let end = text.find("\ntrailer").unwrap();
    assert_eq!(end - start, 10 * 20 - 1);
}

fn count(haystack: &[u8], needle: &[u8]) -> usize {
    if needle.is_empty() || haystack.len() < needle.len() {
        return 0;
    }
    haystack
        .windows(needle.len())
        .filter(|w| *w == needle)
        .count()
}
