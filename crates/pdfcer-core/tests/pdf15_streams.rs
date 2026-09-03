//! # PDF 1.5 structure: cross-reference streams, object streams,
//! hybrid-reference files — end-to-end
//!
//! Whole-file coverage for ISO 32000-1 §7.5.7 (object streams),
//! §7.5.8 (cross-reference streams) and §7.5.8.4 (hybrid-reference
//! files), driven only through `pdfcer-core`'s **public** API
//! (`xref::load_xref_chain`, `Document::from_bytes`). Field-level unit
//! tests for row/dictionary decoding live next to the code in
//! `src/xref.rs`; this file proves the pieces compose into a loadable
//! document.
//!
//! ## Why the fixtures are built in code
//!
//! Every PDF here is **synthetic and generated at test time**
//! (docs/LEGAL.md §5: fixtures are synthetic or rights-cleared, never a
//! downloaded real-world file). Generating rather than checking in
//! bytes also keeps the offsets self-consistent: an xref stream must
//! contain the byte offset of *itself*, which a hand-written fixture
//! silently breaks the moment anyone edits a line above it.
//!
//! ## What each builder models
//!
//! - [`build_xref_stream_pdf`] — a file that uses xref streams
//!   **entirely** (§7.5.8.1: "the keywords `xref` and `trailer` shall
//!   no longer be used"), optionally in the near-universal real-world
//!   encoding (`FlateDecode` + `/Predictor 12`, PNG "Up" prediction
//!   over fixed-width rows).
//! - [`objstm_body`] — an object stream laid out exactly per §7.5.7:
//!   `N` `objnum offset` pairs, then the object values at `First`, with
//!   no `obj`/`endobj` framing.
//! - [`build_hybrid_pdf`] — the §7.5.8.4 arrangement: a classic main
//!   section that marks an object **free** (so a pre-1.5 reader sees
//!   null), an update section whose trailer carries `/XRefStm`, and a
//!   cross-reference stream that gives that same object a real
//!   type-1 entry.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::io::Write as _;

use pdfcer_core::document::{DocError, Document};
use pdfcer_core::object::{ObjId, Object, Provenance};
use pdfcer_core::xref::{self, XrefEntry, XrefErrorKind};

// ---------------------------------------------------------------------------
// Fixture builders
// ---------------------------------------------------------------------------

/// One packed cross-reference-stream row, in Table 18 field order
/// (`type`, field 2, field 3).
type Row = (u64, u64, u64);

/// Big-endian encode `value` into exactly `width` bytes.
///
/// §7.5.8.3: "Fields requiring more than one byte are stored with the
/// high-order byte first." A `width` of 0 emits nothing — that is how
/// the spec expresses an absent field.
fn be(value: u64, width: usize) -> Vec<u8> {
    (0..width)
        .rev()
        .map(|i| u8::try_from((value >> (8 * i)) & 0xFF).unwrap())
        .collect()
}

/// Pack rows into the fixed-width byte layout an xref stream carries.
fn pack(rows: &[Row], w: [usize; 3]) -> Vec<u8> {
    let mut out = Vec::new();
    for &(f1, f2, f3) in rows {
        out.extend(be(f1, w[0]));
        out.extend(be(f2, w[1]));
        out.extend(be(f3, w[2]));
    }
    out
}

/// Apply PNG "Up" prediction (RFC 2083 §6.3, algorithm tag 2) to
/// fixed-width rows — the encoding almost every real producer uses for
/// cross-reference streams, since consecutive rows differ little.
fn png_up_encode(data: &[u8], row_len: usize) -> Vec<u8> {
    let mut out = Vec::new();
    let mut prev = vec![0u8; row_len];
    for row in data.chunks(row_len) {
        out.push(2); // the per-row algorithm tag
        for (i, &b) in row.iter().enumerate() {
            out.push(b.wrapping_sub(prev[i]));
        }
        prev = row.to_vec();
    }
    out
}

/// zlib-compress (the `FlateDecode` wire format, RFC 1950).
fn zlib(data: &[u8]) -> Vec<u8> {
    let mut enc = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    enc.write_all(data).unwrap();
    enc.finish().unwrap()
}

/// Build a complete PDF 1.5 file whose only cross-reference section is
/// a cross-reference stream.
///
/// - `objects` — file-level objects, written in the given order as
///   `N 0 obj … endobj`. Object 1 is expected to be the catalog (the
///   generated dictionary sets `/Root 1 0 R`).
/// - `compressed` — `(object number, container object number, index)`
///   triples that become type-2 entries.
/// - `w` — the `/W` field widths to encode with.
/// - `predictor` — when true, encode as `FlateDecode` +
///   `/DecodeParms << /Predictor 12 /Columns sum(W) >>`.
///
/// The xref stream itself is given the next free object number and, as
/// §7.5.8.3 requires ("an entry for it shall exist in … usually
/// itself"), a type-1 entry pointing at its own offset.
fn build_xref_stream_pdf(
    objects: &[(u32, &str)],
    compressed: &[(u32, u32, u32)],
    w: [usize; 3],
    predictor: bool,
) -> Vec<u8> {
    let mut buf = b"%PDF-1.5\n".to_vec();
    let mut offsets: Vec<(u32, usize)> = Vec::new();
    for (num, body) in objects {
        offsets.push((*num, buf.len()));
        buf.extend_from_slice(format!("{num} 0 obj\n{body}\nendobj\n").as_bytes());
    }

    let xref_num = objects
        .iter()
        .map(|(n, _)| *n)
        .chain(compressed.iter().map(|(n, _, _)| *n))
        .max()
        .unwrap_or(0)
        + 1;
    let xref_at = buf.len();
    offsets.push((xref_num, xref_at));
    let size = xref_num + 1;

    let rows: Vec<Row> = (0..size)
        .map(|num| {
            if num == 0 {
                // §7.5.4: object 0 is permanently the free-list head.
                (0, 0, 65535)
            } else if let Some((_, off)) = offsets.iter().find(|(n, _)| *n == num) {
                (1, u64::try_from(*off).unwrap(), 0)
            } else if let Some((_, c, i)) = compressed.iter().find(|(n, _, _)| *n == num) {
                (2, u64::from(*c), u64::from(*i))
            } else {
                (0, 0, 0)
            }
        })
        .collect();

    let packed = pack(&rows, w);
    let row_len: usize = w.iter().sum();
    let (data, parms) = if predictor {
        (
            zlib(&png_up_encode(&packed, row_len)),
            format!(" /Filter /FlateDecode /DecodeParms << /Predictor 12 /Columns {row_len} >>"),
        )
    } else {
        (packed, String::new())
    };

    let dict = format!(
        "<< /Type /XRef /Size {size} /W [{} {} {}] /Root 1 0 R /Length {}{parms} >>",
        w[0],
        w[1],
        w[2],
        data.len(),
    );
    buf.extend_from_slice(format!("{xref_num} 0 obj\n{dict}\nstream\n").as_bytes());
    buf.extend_from_slice(&data);
    buf.extend_from_slice(b"\nendstream\nendobj\n");
    buf.extend_from_slice(format!("startxref\n{xref_at}\n%%EOF\n").as_bytes());
    buf
}

/// Build the body text (`<< … >> stream … endstream`) of an object
/// stream holding `objects`, laid out per §7.5.7: the `N` pairs first,
/// then the values at `/First`, with no `obj`/`endobj` framing.
///
/// `extra` is appended to the dictionary (used here for `/Extends`).
/// The data is left uncompressed — filter coverage for object streams
/// lives in `src/objstm.rs`'s unit tests.
fn objstm_body(objects: &[(u32, &str)], extra: &str) -> String {
    let mut header = String::new();
    let mut body = String::new();
    for (num, text) in objects {
        header.push_str(&format!("{num} {} ", body.len()));
        body.push_str(text);
        body.push(' ');
    }
    let first = header.len();
    let data = format!("{header}{body}");
    format!(
        "<< /Type /ObjStm /N {} /First {first} /Length {}{extra} >>\nstream\n{data}\nendstream",
        objects.len(),
        data.len(),
    )
}

/// Build a hybrid-reference file (§7.5.8.4).
///
/// Structure, in file order:
///
/// 1. objects 1 (catalog), 2 (page tree) and 4 (the *hidden* object,
///    referenced from the catalog's optional `/Outlines`);
/// 2. a **main** classic section listing 1 and 2 as in-use and marking
///    object 4 **free with generation 65535** — the spec's own hiding
///    mechanism, which makes `4 0 R` read as null to a pre-1.5 reader;
/// 3. a cross-reference **stream** (object 5) that gives object 4 a
///    real type-1 entry, plus whatever `shadow_rows` the caller adds;
/// 4. an **update** classic section whose trailer carries `/XRefStm`
///    (pointing at 3) and `/Prev` (pointing at 2).
///
/// `shadow_rows` lets a test put an entry in the `XRefStm` stream that
/// *competes* with the update section's own classic table, which is how
/// the §7.5.8.4 search order is pinned.
fn build_hybrid_pdf(shadow_rows: &[(u32, Row)]) -> Vec<u8> {
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

    // --- 2. main classic section: object 4 is FREE (hidden) ---
    let base_at = buf.len();
    buf.extend_from_slice(b"xref\n0 5\n");
    buf.extend_from_slice(b"0000000000 65535 f\r\n");
    buf.extend_from_slice(format!("{:010} 00000 n\r\n", off(1)).as_bytes());
    buf.extend_from_slice(format!("{:010} 00000 n\r\n", off(2)).as_bytes());
    buf.extend_from_slice(b"0000000000 65535 f\r\n");
    // §7.5.8.4: the free entry's next-generation is 65535 "so that the
    // object number shall not be reused".
    buf.extend_from_slice(b"0000000000 65535 f\r\n");
    buf.extend_from_slice(b"trailer\n<< /Size 6 /Root 1 0 R >>\n");

    // --- 3. the XRefStm cross-reference stream (object 5) ---
    let stm_at = buf.len();
    let w = [1usize, 4, 2];
    let mut rows: Vec<(u32, Row)> = vec![(4, (1, u64::try_from(off(4)).unwrap(), 0))];
    rows.extend_from_slice(shadow_rows);
    rows.sort_by_key(|(n, _)| *n);
    let index: String = rows
        .iter()
        .map(|(n, _)| format!("{n} 1 "))
        .collect::<String>();
    let data = pack(
        &rows.iter().map(|(_, r)| *r).collect::<Vec<_>>(),
        [w[0], w[1], w[2]],
    );
    let dict = format!(
        "<< /Type /XRef /Size 6 /W [{} {} {}] /Index [{}] /Root 1 0 R /Length {} >>",
        w[0],
        w[1],
        w[2],
        index.trim_end(),
        data.len(),
    );
    buf.extend_from_slice(format!("5 0 obj\n{dict}\nstream\n").as_bytes());
    buf.extend_from_slice(&data);
    buf.extend_from_slice(b"\nendstream\nendobj\n");

    // --- 4. update classic section + hybrid trailer ---
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
// Cross-reference streams (§7.5.8)
// ---------------------------------------------------------------------------

#[test]
fn pure_xref_stream_file_loads_unpredicted() {
    // §7.5.8.1: "with the exception of the startxref address / %%EOF
    // segment and comments, a file may be entirely a sequence of
    // objects" — no `xref`, no `trailer` keyword anywhere.
    let bytes = build_xref_stream_pdf(
        &[
            (1, "<< /Type /Catalog /Pages 2 0 R >>"),
            (2, "<< /Type /Pages /Kids [] /Count 0 >>"),
        ],
        &[],
        [1, 4, 2],
        false,
    );
    assert!(
        !bytes.windows(7).any(|w| w == b"trailer"),
        "fixture must not contain a classic trailer"
    );

    let doc = Document::from_bytes(bytes).unwrap();
    assert_eq!(doc.object_count(), 3); // catalog, pages, the xref stream
    let catalog = doc.catalog().unwrap();
    assert_eq!(
        catalog.get(b"Type").unwrap().as_name().unwrap().as_bytes(),
        b"Catalog"
    );
    // The xref stream's dictionary IS the trailer (§7.5.8.1).
    assert_eq!(doc.trailer().get(b"Size").unwrap().as_int(), Some(4));
}

#[test]
fn pure_xref_stream_file_loads_with_flate_and_predictor_12() {
    // The near-universal real-world encoding. Predictor support is a
    // hard gate for xref streams, not a nicety (filter__predictors.md).
    let doc = Document::from_bytes(build_xref_stream_pdf(
        &[
            (1, "<< /Type /Catalog /Pages 2 0 R >>"),
            (2, "<< /Type /Pages /Kids [] /Count 0 >>"),
        ],
        &[],
        [1, 4, 2],
        true,
    ))
    .unwrap();
    assert_eq!(doc.object_count(), 3);
    assert!(doc.catalog().is_ok());
}

#[test]
fn xref_stream_survives_unusual_but_legal_w_widths() {
    // `W` is per-stream and "different cross-reference streams in a PDF
    // file may use different values" (§7.5.8.2) — a decoder that
    // hardcodes the common `[1 2 1]`/`[1 4 2]` shapes breaks here.
    // `[1 8 1]` uses the maximum representable field-2 width together
    // with a single-byte generation field.
    //
    // (A zero-width *type* field cannot be exercised at whole-file
    // level: this fixture necessarily contains object 0's type-0
    // free-list head, and `W[0] = 0` would force every row to type 1.
    // That default is pinned at row level in `src/xref.rs`.)
    let doc = Document::from_bytes(build_xref_stream_pdf(
        &[
            (1, "<< /Type /Catalog /Pages 2 0 R >>"),
            (2, "<< /Type /Pages /Kids [] /Count 0 >>"),
        ],
        &[],
        [1, 8, 1],
        true,
    ))
    .unwrap();
    assert_eq!(doc.object_count(), 3);
}

#[test]
fn xref_stream_free_entries_resolve_to_null() {
    // Object 3 gets no definition, so the builder emits a type-0 row
    // for it. §7.3.10: the reference is null, never an error.
    let doc = Document::from_bytes(build_xref_stream_pdf(
        &[
            (1, "<< /Type /Catalog /Pages 2 0 R >>"),
            (2, "<< /Type /Pages /Kids [] /Count 0 >>"),
            (4, "(object four)"),
        ],
        &[],
        [1, 4, 2],
        false,
    ))
    .unwrap();
    assert!(doc.get(ObjId::new(3, 0)).is_none());
    assert_eq!(
        *doc.resolve(&Object::Reference(ObjId::new(3, 0))),
        Object::Null
    );
    assert!(doc.get(ObjId::new(4, 0)).is_some());
}

#[test]
fn xref_stream_entries_are_visible_in_the_merged_table() {
    let bytes = build_xref_stream_pdf(
        &[
            (1, "<< /Type /Catalog /Pages 2 0 R >>"),
            (2, "<< /Type /Pages /Kids [] /Count 0 >>"),
        ],
        &[],
        [1, 4, 2],
        false,
    );
    let loaded = xref::load_xref_chain(&bytes).unwrap();
    assert!(matches!(
        loaded.table.get(0),
        Some(XrefEntry::Free {
            generation: 65535,
            ..
        })
    ));
    assert!(matches!(
        loaded.table.get(1),
        Some(XrefEntry::InUse { generation: 0, .. })
    ));
}

// ---------------------------------------------------------------------------
// Object streams (§7.5.7)
// ---------------------------------------------------------------------------

#[test]
fn compressed_objects_resolve_through_their_container() {
    // Object 3 is the container; objects 4 and 5 live inside it, and
    // the catalog reaches object 4 by an ordinary reference — the
    // reader must not be able to tell the difference.
    let container = objstm_body(
        &[
            (4, "<< /Type /Metadata /Marker (from objstm) >>"),
            (5, "[1 2 3]"),
        ],
        "",
    );
    let doc = Document::from_bytes(build_xref_stream_pdf(
        &[
            (1, "<< /Type /Catalog /Pages 2 0 R /PieceInfo 4 0 R >>"),
            (2, "<< /Type /Pages /Kids [] /Count 0 >>"),
            (3, &container),
        ],
        &[(4, 3, 0), (5, 3, 1)],
        [1, 4, 2],
        true,
    ))
    .unwrap();

    let catalog = doc.catalog().unwrap();
    let piece = doc.resolve(catalog.get(b"PieceInfo").unwrap());
    assert_eq!(
        piece.as_dict().unwrap().get(b"Marker").unwrap(),
        &Object::String(b"from objstm".to_vec())
    );
    let five_ref = Object::Reference(ObjId::new(5, 0));
    let five = doc.resolve(&five_ref);
    assert_eq!(five.as_array().unwrap().len(), 3);
}

#[test]
fn compressed_objects_carry_object_stream_provenance_not_a_file_span() {
    // The §5 provenance contract: a file-level object re-emits its own
    // bytes verbatim; a compressed object HAS no file bytes and must
    // say so rather than offering a span that means nothing.
    let container = objstm_body(&[(4, "(compressed)")], "");
    let doc = Document::from_bytes(build_xref_stream_pdf(
        &[
            (1, "<< /Type /Catalog /Pages 2 0 R >>"),
            (2, "<< /Type /Pages /Kids [] /Count 0 >>"),
            (3, &container),
        ],
        &[(4, 3, 0)],
        [1, 4, 2],
        false,
    ))
    .unwrap();

    // File-level object: span-backed, and the span really is its bytes.
    let catalog_obj = doc.get(ObjId::new(1, 0)).unwrap();
    let raw = catalog_obj.file_span().unwrap().slice(doc.bytes()).unwrap();
    assert!(raw.starts_with(b"1 0 obj"));
    assert!(raw.ends_with(b"endobj"));

    // Compressed object: no file span, and it names its container.
    let compressed = doc.get(ObjId::new(4, 0)).unwrap();
    assert_eq!(compressed.file_span(), None);
    assert_eq!(
        compressed.provenance,
        Provenance::ObjectStream {
            container: ObjId::new(3, 0),
            index: 0,
        }
    );
    assert_eq!(
        compressed.provenance.container(),
        Some(ObjId::new(3, 0)),
        "the writer must be able to find the container without the xref"
    );
}

#[test]
fn compressed_objects_always_have_generation_zero() {
    // §7.5.7/§7.3.10: a type-2 entry carries no generation, and the
    // object's generation "shall be zero". A reference with any other
    // generation is stale and resolves to null.
    let container = objstm_body(&[(4, "(compressed)")], "");
    let doc = Document::from_bytes(build_xref_stream_pdf(
        &[
            (1, "<< /Type /Catalog /Pages 2 0 R >>"),
            (2, "<< /Type /Pages /Kids [] /Count 0 >>"),
            (3, &container),
        ],
        &[(4, 3, 0)],
        [1, 4, 2],
        false,
    ))
    .unwrap();
    assert!(doc.get(ObjId::new(4, 0)).is_some());
    assert_eq!(
        *doc.resolve(&Object::Reference(ObjId::new(4, 1))),
        Object::Null
    );
}

#[test]
fn extends_linked_object_streams_both_resolve() {
    // Table 16 `/Extends` links object streams into a collection. It is
    // informational for a READER — each type-2 entry names its own
    // container directly — so both halves must resolve without the
    // loader ever walking the chain.
    let base = objstm_body(&[(5, "(in base)")], "");
    let ext = objstm_body(&[(6, "(in extension)")], " /Extends 3 0 R");
    let doc = Document::from_bytes(build_xref_stream_pdf(
        &[
            (1, "<< /Type /Catalog /Pages 2 0 R >>"),
            (2, "<< /Type /Pages /Kids [] /Count 0 >>"),
            (3, &base),
            (4, &ext),
        ],
        &[(5, 3, 0), (6, 4, 0)],
        [1, 4, 2],
        true,
    ))
    .unwrap();
    assert_eq!(
        *doc.resolve(&Object::Reference(ObjId::new(5, 0))),
        Object::String(b"in base".to_vec())
    );
    assert_eq!(
        *doc.resolve(&Object::Reference(ObjId::new(6, 0))),
        Object::String(b"in extension".to_vec())
    );
    // A cyclic /Extends (4 → 3 → 4) is likewise inert, because the
    // chain is never followed.
    let cyclic_base = objstm_body(&[(5, "(in base)")], " /Extends 4 0 R");
    let doc = Document::from_bytes(build_xref_stream_pdf(
        &[
            (1, "<< /Type /Catalog /Pages 2 0 R >>"),
            (2, "<< /Type /Pages /Kids [] /Count 0 >>"),
            (3, &cyclic_base),
            (4, &ext),
        ],
        &[(5, 3, 0), (6, 4, 0)],
        [1, 4, 2],
        false,
    ))
    .unwrap();
    assert!(doc.get(ObjId::new(6, 0)).is_some());
}

#[test]
fn container_disagreeing_with_the_xref_is_refused() {
    // The xref says object 4 is at index 0 of container 3, but the
    // container's pair table stores object 9 there. Strict: refuse and
    // name both, rather than trusting one silently.
    let container = objstm_body(&[(9, "(mislabelled)")], "");
    let err = Document::from_bytes(build_xref_stream_pdf(
        &[
            (1, "<< /Type /Catalog /Pages 2 0 R >>"),
            (2, "<< /Type /Pages /Kids [] /Count 0 >>"),
            (3, &container),
        ],
        &[(4, 3, 0)],
        [1, 4, 2],
        false,
    ))
    .unwrap_err();
    assert!(
        matches!(
            err,
            DocError::ObjectStreamIdMismatch {
                expected: 4,
                found: 9,
                ..
            }
        ),
        "unexpected error: {err}"
    );
}

#[test]
fn type2_entry_naming_a_missing_container_is_refused() {
    let err = Document::from_bytes(build_xref_stream_pdf(
        &[
            (1, "<< /Type /Catalog /Pages 2 0 R >>"),
            (2, "<< /Type /Pages /Kids [] /Count 0 >>"),
        ],
        // Container 3 is never defined.
        &[(4, 3, 0)],
        [1, 4, 2],
        false,
    ))
    .unwrap_err();
    assert!(
        matches!(err, DocError::ObjectStreamMissing { num: 4, .. }),
        "unexpected error: {err}"
    );
}

#[test]
fn out_of_range_index_into_a_container_is_refused() {
    let container = objstm_body(&[(4, "(only one)")], "");
    let err = Document::from_bytes(build_xref_stream_pdf(
        &[
            (1, "<< /Type /Catalog /Pages 2 0 R >>"),
            (2, "<< /Type /Pages /Kids [] /Count 0 >>"),
            (3, &container),
        ],
        &[(4, 3, 7)],
        [1, 4, 2],
        false,
    ))
    .unwrap_err();
    assert!(
        matches!(err, DocError::ObjectStream { .. }),
        "unexpected error: {err}"
    );
}

// ---------------------------------------------------------------------------
// Hybrid-reference files (§7.5.8.4)
// ---------------------------------------------------------------------------

#[test]
fn xrefstm_is_consulted_before_prev_and_reveals_the_hidden_object() {
    // The operative §7.5.8.4 rule: "the search shall proceed to a
    // cross-reference stream specified by the XRefStm entry BEFORE
    // looking in the previous cross-reference section." Object 4 is
    // free-with-65535 in the main section and type-1 in the XRefStm
    // stream; a conforming 1.5+ reader sees the object.
    let bytes = build_hybrid_pdf(&[]);
    let loaded = xref::load_xref_chain(&bytes).unwrap();
    assert!(
        matches!(loaded.table.get(4), Some(XrefEntry::InUse { .. })),
        "XRefStm entry must beat the older section's free entry"
    );

    let doc = Document::from_bytes(bytes).unwrap();
    let outlines = doc.resolve(doc.catalog().unwrap().get(b"Outlines").unwrap());
    assert_eq!(
        outlines
            .as_dict()
            .unwrap()
            .get(b"Type")
            .unwrap()
            .as_name()
            .unwrap()
            .as_bytes(),
        b"Outlines"
    );
}

#[test]
fn xrefstm_does_not_override_its_own_sections_classic_table() {
    // The search order is "not found in THIS classic section → try
    // XRefStm → then Prev". So an XRefStm row competing with an entry
    // in the same section's own table must LOSE. Here the update
    // section's classic table puts object 5 at the real stream offset;
    // the XRefStm claims object 5 lives at offset 1.
    let bytes = build_hybrid_pdf(&[(5, (1, 1, 0))]);
    let loaded = xref::load_xref_chain(&bytes).unwrap();
    let Some(XrefEntry::InUse { offset, .. }) = loaded.table.get(5) else {
        panic!("object 5 must be in use");
    };
    assert_ne!(offset, 1, "XRefStm must not shadow its own section");
    // …and the file still loads, which it would not if offset 1 won.
    assert!(Document::from_bytes(bytes).is_ok());
}

#[test]
fn a_broken_xrefstm_degrades_to_the_classic_view_instead_of_failing() {
    // §7.5.8.4 guarantees everything visible from the root is in the
    // classic tables, so an unusable XRefStm is exactly a pre-1.5
    // reader's situation — a documented safe fallback, not a guess.
    let mut bytes = build_hybrid_pdf(&[]);
    // Corrupt the XRefStm target: overwrite its `/Type /XRef` so the
    // stream is refused, without touching anything classic.
    let pos = bytes
        .windows(10)
        .position(|w| w == b"/Type /XRe")
        .expect("fixture shape changed");
    bytes[pos..pos + 10].copy_from_slice(b"/Type /Nop");

    let doc = Document::from_bytes(bytes).unwrap();
    // Object 4 is hidden again — free ⇒ null, exactly as §7.3.10 says.
    assert_eq!(
        *doc.resolve(doc.catalog().unwrap().get(b"Outlines").unwrap()),
        Object::Null
    );
}

// ---------------------------------------------------------------------------
// Malformed input stays fail-clean
// ---------------------------------------------------------------------------

#[test]
fn truncated_xref_stream_data_is_a_clean_error() {
    // /Index promises more rows than the data holds.
    let mut bytes = build_xref_stream_pdf(
        &[
            (1, "<< /Type /Catalog /Pages 2 0 R >>"),
            (2, "<< /Type /Pages /Kids [] /Count 0 >>"),
        ],
        &[],
        [1, 4, 2],
        false,
    );
    // Inflate /Size (and therefore the default /Index) past the data.
    let pos = bytes
        .windows(8)
        .position(|w| w == b"/Size 4 ")
        .expect("fixture shape changed");
    bytes[pos..pos + 8].copy_from_slice(b"/Size 9 ");
    let err = xref::load_xref_chain(&bytes).unwrap_err();
    assert!(
        matches!(err.kind, XrefErrorKind::BadXrefStream(_)),
        "unexpected error: {err}"
    );
}

#[test]
fn a_startxref_pointing_at_a_non_stream_object_is_a_clean_error() {
    let mut buf: Vec<u8> = b"%PDF-1.5\n".to_vec();
    let at = buf.len();
    buf.extend_from_slice(b"12 0 obj\n<< /Type /XRef >>\nendobj\n");
    buf.extend_from_slice(format!("startxref\n{at}\n%%EOF\n").as_bytes());
    let err = xref::load_xref_chain(&buf).unwrap_err();
    assert!(
        matches!(err.kind, XrefErrorKind::BadXrefStream(_)),
        "unexpected error: {err}"
    );
}

#[test]
fn authoring_onto_a_compressed_page_object_promotes_it_r38_x7() {
    // X7: the /Annots patch lands on a page object that is COMPRESSED
    // inside an object stream (§7.5.7). The corpus cannot supply this
    // (all 75 corpus files with object streams keep their page objects
    // uncompressed), so it is fixture-covered. Authoring an annotation
    // modifies the page dict, and a modified compressed object cannot be
    // patched in place — it must be PROMOTED out to a file-level object
    // (R38). This proves the whole authoring → save → reload path works
    // end-to-end when the page lives in a container.
    use pdfcer_core::annot_author::{Color, MarkupSpec};
    use pdfcer_core::edit::EditSession;
    use pdfcer_core::page_tree::Rect;

    // Object 3 (the page) lives inside object stream 6; every other
    // object is file-level. A type-2 xref entry (3 -> container 6,
    // index 0) is what makes object 3 compressed.
    let objstm = objstm_body(&[(3, "<< /Type /Page /Parent 2 0 R >>")], "");
    let bytes = build_xref_stream_pdf(
        &[
            (1, "<< /Type /Catalog /Pages 2 0 R >>"),
            (
                2,
                "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 300 300] /Resources << >> >>",
            ),
            (6, objstm.as_str()),
        ],
        &[(3, 6, 0)],
        [1, 2, 2],
        false,
    );

    let doc = Document::from_bytes(bytes).unwrap();
    // Confirm the page really is compressed before we edit it.
    assert!(
        matches!(
            doc.get(ObjId::new(3, 0)).unwrap().provenance,
            Provenance::ObjectStream { .. }
        ),
        "the page must start compressed for this test to be meaningful"
    );

    let mut session = EditSession::new(doc);
    let annot_id = session
        .add_markup(
            0,
            &MarkupSpec::Square {
                rect: Rect {
                    llx: 20.0,
                    lly: 20.0,
                    urx: 120.0,
                    ury: 70.0,
                },
                border: Some(Color::Rgb(1.0, 0.0, 0.0)),
                interior: None,
                border_width: 2.0,
                border_effect: None,
            },
        )
        .unwrap();

    let (out, report) = session
        .to_incremental_bytes(&pdfcer_core::writer::SaveOptions::identity())
        .unwrap();
    // The page was promoted out of its container (R38).
    assert!(
        report.promoted.contains(&ObjId::new(3, 0)),
        "the compressed page object must be promoted, got {:?}",
        report.promoted
    );

    // The saved file reloads, the page is now file-level, and the
    // authored annotation with its appearance is intact.
    let reloaded = Document::from_bytes(out).unwrap();
    assert!(matches!(
        reloaded.get(ObjId::new(3, 0)).unwrap().provenance,
        Provenance::File(_)
    ));
    let pages = pdfcer_core::page_tree::pages(&reloaded).unwrap();
    let annots = pdfcer_core::annot::page_annotations(&reloaded, pages[0].id);
    assert_eq!(annots.len(), 1);
    assert_eq!(annots[0].id, Some(annot_id));
    assert!(matches!(
        annots[0].appearance,
        pdfcer_core::annot::Appearance::Normal { .. }
    ));
}
