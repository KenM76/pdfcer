//! # Pass 9c-min integration test — basic vector editing, end to end
//!
//! Drives the whole move / delete / drag-node surgery
//! (decision 011 §2.5, `docs/decisions/011-first-beta-scaled-measurement-
//! dimensioning-tool.md` Appendix A **Pass 9c-min**) only through the public
//! API — `EditSession::{move_object, delete_object, move_node}` for the
//! mutation and undo, `writer::save_incremental` for the bytes, and
//! `vector::decompose_page` to read the geometry back. Unit tests next to
//! the surgery (`vector::edit`) cover the operand arithmetic; this file
//! proves the pieces compose into files that honour the invariants the Pass
//! exists to honour:
//!
//! 1. **content-identity / R46 named exception** — after a move/delete/drag,
//!    ONLY the one edited content stream differs from the base; every other
//!    object is byte-verbatim, and (incremental save) every byte below the
//!    base EOF is untouched (`assert_only_the_named_objects_changed`).
//! 2. **the edit took** — reloading and re-decomposing shows the object
//!    moved / gone / its node relocated (not by trusting the writer).
//! 3. **undo restores byte-identical pre-edit state** (Pass 3.1 command
//!    log): edit → undo → save == input, byte for byte.
//! 4. **compressed content is decoded, edited, and re-emitted raw with no
//!    stale copy** (§5.7/§5.9): a FlateDecode content stream round-trips
//!    through the surgery to the moved geometry, and the output stream
//!    carries no `/Filter`.
//! 5. **fuzzy-never-sneaky refusals** — a non-path move, a rectangle-corner
//!    node drag, and a second in-session edit are all refused **by name**
//!    with the session left untouched (rule 4).
//!
//! Fixtures: the committed synthetic `fixtures/synthetic/vector/edit.pdf`
//! (three isolated, index-predictable objects — see that directory's
//! `PROVENANCE.md`) and `mixed.pdf` (a text/image object for the refusal
//! path), plus in-test builders for the compressed-content and object-stream
//! cases (same discipline as `edit_undo.rs`: a checked-in real-world PDF is
//! forbidden by `LEGAL.md` §5, and an xref stream stores its own byte
//! offset, so the builder names the clause it models).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::io::Write as _;
use std::path::{Path, PathBuf};

use pdfcer_core::document::Document;
use pdfcer_core::edit::{CommandKind, EditError, EditSession, PaintRefusalReason};
use pdfcer_core::object::{ObjId, Object, Provenance};
use pdfcer_core::page_tree;
use pdfcer_core::vector::{
    Matrix, PageObjects, PathObject, Point, Rgb, Segment, VectorEditError, VectorObject,
    decompose_page,
};
use pdfcer_core::writer::SaveOptions;

// ---------------------------------------------------------------------------
// Fixtures + helpers
// ---------------------------------------------------------------------------

/// The committed three-object edit fixture (line / rectangle / triangle).
fn edit_fixture() -> Vec<u8> {
    std::fs::read(fixture("edit.pdf")).expect("edit.pdf fixture loads")
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/vector")
        .join(name)
}

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

/// Decompose page 0 of a document into its selectable objects.
fn decompose0(doc: &Document) -> PageObjects {
    let pages = page_tree::pages(doc).unwrap();
    decompose_page(&doc.view(), &pages[0], Matrix::IDENTITY).unwrap()
}

/// The object id of page 0's first `/Contents` stream — the only object a
/// vector edit may change.
fn content_id(doc: &Document) -> ObjId {
    let pages = page_tree::pages(doc).unwrap();
    *pages[0].contents.first().unwrap()
}

/// The first path object of a decomposition, and its index.
fn first_path(model: &PageObjects) -> (usize, &PathObject) {
    model
        .objects
        .iter()
        .enumerate()
        .find_map(|(i, o)| match o {
            VectorObject::Path(p) => Some((i, p)),
            _ => None,
        })
        .expect("a path object")
}

/// The page-space start anchor of a path object's first subpath.
fn first_start(p: &PathObject) -> Point {
    p.page_subpaths()[0].start
}

/// Assert the incremental minimal-diff property (copied from `edit_undo.rs`):
/// every byte below the base EOF is untouched, and every object the edit did
/// not name still resolves, through the reloaded file's own xref, to its
/// original definition bytes — proving exactly one changed stream (R46).
fn assert_only_the_named_objects_changed(base: &[u8], out: &[u8], edited: &[ObjId]) {
    assert!(
        out.starts_with(base),
        "an incremental save modified bytes below the original EOF (§7.5.6)"
    );
    let before = Document::from_bytes(base.to_vec()).unwrap();
    let after = Document::from_bytes(out.to_vec()).unwrap();
    for io in before.objects() {
        if edited.contains(&io.id) {
            continue;
        }
        let Provenance::File(span) = io.provenance else {
            continue; // a compressed object is checked via its container
        };
        let want = span.slice(before.bytes());
        let got = after
            .get(io.id)
            .and_then(|o| o.file_span())
            .and_then(|s| s.slice(after.bytes()));
        assert_eq!(
            got, want,
            "object {} was perturbed by a vector edit that did not name it",
            io.id
        );
    }
}

// ---------------------------------------------------------------------------
// MOVE
// ---------------------------------------------------------------------------

#[test]
fn move_object_relocates_only_that_object_and_only_the_content_stream() {
    let base = edit_fixture();
    let cid = content_id(&Document::from_bytes(base.clone()).unwrap());

    // Object 0 is the stroked line `50 50 m 150 150 l S`.
    let mut s = session(&base);
    let before = decompose0(s.document());
    let (idx, line) = first_path(&before);
    assert_eq!(first_start(line), Point::new(50.0, 50.0));

    s.move_object(0, idx, 30.0, -20.0).unwrap();
    assert_eq!(s.undo_kind(), Some(CommandKind::MoveObject));

    let out = save(&s);
    // R46 content-identity: exactly the one content stream changed.
    assert_only_the_named_objects_changed(&base, &out, &[cid]);

    // The edit took: reload and re-decompose.
    let back = Document::from_bytes(out).unwrap();
    let after = decompose0(&back);
    let (_, moved) = first_path(&after);
    assert_eq!(first_start(moved), Point::new(80.0, 30.0), "start shifted");
    // The `l` endpoint moved by the same delta.
    match moved.page_subpaths()[0].segments[0] {
        Segment::Line { to } => assert_eq!(to, Point::new(180.0, 130.0)),
        other => panic!("expected a line segment, got {other:?}"),
    }
}

#[test]
fn move_object_then_undo_then_save_is_byte_identical() {
    let base = edit_fixture();
    let mut s = session(&base);
    s.move_object(0, 0, 12.5, 7.25).unwrap();
    assert!(s.is_modified());
    s.undo();
    assert!(!s.is_modified());
    assert_eq!(save(&s), base, "move -> undo -> save must change nothing");
}

#[test]
fn moving_a_rectangle_shifts_its_origin_not_its_size() {
    let base = edit_fixture();
    let mut s = session(&base);
    // Object 1 is the filled rectangle `200 50 80 60 re f`.
    let (idx, _) = {
        let model = decompose0(s.document());
        // The rectangle is the second path (paint order 1).
        let mut paths = model
            .objects
            .iter()
            .enumerate()
            .filter_map(|(i, o)| match o {
                VectorObject::Path(p) => Some((i, p.clone())),
                _ => None,
            });
        let _line = paths.next().unwrap();
        let rect = paths.next().unwrap();
        (rect.0, rect.1)
    };
    s.move_object(0, idx, 100.0, 0.0).unwrap();
    let back = Document::from_bytes(save(&s)).unwrap();
    let model = decompose0(&back);
    // The rectangle's page bbox is the same width/height, shifted +100 in x.
    let rect = model
        .objects
        .iter()
        .filter_map(|o| match o {
            VectorObject::Path(p) if p.is_quad() => Some(p),
            _ => None,
        })
        .next()
        .expect("the rectangle survives as a quad");
    assert_eq!(rect.page_bbox.min, Point::new(300.0, 50.0));
    assert_eq!(rect.page_bbox.max, Point::new(380.0, 110.0));
}

// ---------------------------------------------------------------------------
// DELETE
// ---------------------------------------------------------------------------

#[test]
fn delete_object_removes_it_and_leaves_the_rest_verbatim() {
    let base = edit_fixture();
    let cid = content_id(&Document::from_bytes(base.clone()).unwrap());
    let before = decompose0(&Document::from_bytes(base.clone()).unwrap());
    let object_count = before.objects.len();

    let mut s = session(&base);
    // Delete object 0 (the stroked line).
    s.delete_object(0, 0).unwrap();
    assert_eq!(s.undo_kind(), Some(CommandKind::DeleteObject));

    let out = save(&s);
    assert_only_the_named_objects_changed(&base, &out, &[cid]);

    let back = Document::from_bytes(out).unwrap();
    let after = decompose0(&back);
    assert_eq!(
        after.objects.len(),
        object_count - 1,
        "exactly one object removed"
    );
    // The removed line's start (50,50) is gone; the rectangle survives.
    assert!(
        after.objects.iter().all(|o| match o {
            VectorObject::Path(p) => first_start(p) != Point::new(50.0, 50.0),
            _ => true,
        }),
        "the deleted line must not survive"
    );
}

#[test]
fn delete_then_undo_is_byte_identical() {
    let base = edit_fixture();
    let mut s = session(&base);
    s.delete_object(0, 1).unwrap(); // delete the rectangle
    s.undo();
    assert_eq!(save(&s), base);
}

#[test]
fn deleting_a_text_object_is_allowed() {
    // A text/image object is not path-editable, but delete is a pure
    // byte-span removal that works on any kind.
    let base = std::fs::read(fixture("mixed.pdf")).unwrap();
    let model = decompose0(&Document::from_bytes(base.clone()).unwrap());
    let text_idx = model
        .objects
        .iter()
        .position(|o| matches!(o, VectorObject::Text(_)))
        .expect("mixed.pdf has a text object");

    let mut s = session(&base);
    s.delete_object(0, text_idx).unwrap();
    let back = Document::from_bytes(save(&s)).unwrap();
    let after = decompose0(&back);
    assert!(
        after
            .objects
            .iter()
            .all(|o| !matches!(o, VectorObject::Text(_))),
        "the text object was deleted"
    );
}

// ---------------------------------------------------------------------------
// DRAG NODE
// ---------------------------------------------------------------------------

#[test]
fn move_node_relocates_one_anchor_only() {
    let base = edit_fixture();
    let cid = content_id(&Document::from_bytes(base.clone()).unwrap());
    let mut s = session(&base);

    // Object 0 = the line; node 1 = its `l` endpoint (150,150).
    s.move_node(0, 0, 1, Point::new(200.0, 100.0)).unwrap();
    assert_eq!(s.undo_kind(), Some(CommandKind::MoveNode));

    let out = save(&s);
    assert_only_the_named_objects_changed(&base, &out, &[cid]);

    let back = Document::from_bytes(out).unwrap();
    let model = decompose0(&back);
    let (_, line) = first_path(&model);
    // The start is unchanged; only the endpoint moved.
    assert_eq!(first_start(line), Point::new(50.0, 50.0));
    match line.page_subpaths()[0].segments[0] {
        Segment::Line { to } => assert_eq!(to, Point::new(200.0, 100.0)),
        other => panic!("expected a line, got {other:?}"),
    }
}

#[test]
fn move_node_then_undo_is_byte_identical() {
    let base = edit_fixture();
    let mut s = session(&base);
    s.move_node(0, 0, 0, Point::new(10.0, 10.0)).unwrap();
    s.undo();
    assert_eq!(save(&s), base);
}

/// A rectangle corner is DRAGGABLE (Pass 30.0), and undoing restores the
/// original `re` operator byte-for-byte.
///
/// This test previously asserted the refusal. The undo half is the part worth
/// keeping and strengthening: the edit replaces one operator with five, so
/// undo has to restore a *shorter* stream than the one it is undoing — the
/// case a same-length rewrite would never exercise.
#[test]
fn move_node_on_a_rectangle_corner_expands_it_and_undo_restores_the_re() {
    let base = edit_fixture();
    let mut s = session(&base);
    // Object 1 is the `re` rectangle; every corner is a rectangle node.
    s.move_node(0, 1, 0, Point::new(0.0, 0.0))
        .expect("a rectangle corner is draggable");
    assert!(s.is_modified());
    s.undo();
    assert_eq!(
        save(&s),
        base,
        "undoing an operator EXPANSION must restore the original bytes"
    );
}

// ---------------------------------------------------------------------------
// Refusals (fuzzy-never-sneaky, rule 4)
// ---------------------------------------------------------------------------

#[test]
fn moving_a_text_object_is_refused_as_not_a_path() {
    let base = std::fs::read(fixture("mixed.pdf")).unwrap();
    let model = decompose0(&Document::from_bytes(base.clone()).unwrap());
    let text_idx = model
        .objects
        .iter()
        .position(|o| matches!(o, VectorObject::Text(_)))
        .unwrap();

    let mut s = session(&base);
    let err = s.move_object(0, text_idx, 5.0, 5.0).unwrap_err();
    assert!(
        matches!(err, EditError::VectorEdit(VectorEditError::NotAPath { .. })),
        "got {err:?}"
    );
    assert!(!s.is_modified());
}

/// ★ The refusal NAMES THE OFFENDING OBJECT, on a MIXED selection.
///
/// `pdfcer-gui` reported (2026-08-20) that `move_objects`' `NotAPath` "currently
/// names no object", so on a mixed selection the operator has to bisect their
/// own selection by hand, and asked for the index to be added.
///
/// **The index was already there** — `vector_object_as_path(obj, i)` is called
/// with the loop index and `NotAPath` has carried `index` and `kind` since it
/// was minted. `EditError::VectorEdit` is `#[error(transparent)]`, so the
/// message passes through intact.
///
/// This test exists because "it already works" is a claim, and the shell was
/// looking at a real screen when it said otherwise. Pinning the rendered
/// STRING — not just the variant — is what makes the answer checkable and
/// stops a future `#[error]` rewrite from quietly making the report true.
#[test]
fn a_mixed_selection_refusal_names_which_object_is_not_a_path() {
    let base = std::fs::read(fixture("mixed.pdf")).unwrap();
    let model = decompose0(&Document::from_bytes(base.clone()).unwrap());
    let text_idx = model
        .objects
        .iter()
        .position(|o| matches!(o, VectorObject::Text(_)))
        .unwrap();
    let path_idx = model
        .objects
        .iter()
        .position(|o| matches!(o, VectorObject::Path(_)))
        .unwrap();

    let mut s = session(&base);
    // A genuinely MIXED selection — a path and a text object together, which
    // is the case the report was about.
    let err = s
        .move_objects(0, &[path_idx, text_idx], 5.0, 5.0)
        .unwrap_err();
    let rendered = err.to_string();
    assert!(
        rendered.contains(&format!("object {text_idx}")),
        "the refusal must name WHICH object, got: {rendered}"
    );
    assert!(
        rendered.contains("text"),
        "and what kind it turned out to be, got: {rendered}"
    );
    assert!(!s.is_modified(), "a refused move must change nothing");
}

/// **Vector edits accumulate within one session.**
///
/// This test previously asserted the OPPOSITE — that a second surgery on the
/// same page was refused with `VectorEditNeedsReopen`, on the reasoning that
/// base-relative indices no longer hold once the page has been rewritten. The
/// reasoning was right and the conclusion was backwards: the caller that
/// supplies the index (the GUI's object provider) decomposes the session view,
/// so the surgery's own base-decompose was the thing out of step. It now reads
/// the page's CURRENT content, the same helper `edit_text` uses, and the
/// mismatch the refusal guarded against cannot arise.
///
/// The operator found it immediately: *"After clicking and deleting an object
/// I couldn't delete another one after selecting it."* On a CAD drawing whose
/// stray lines are what needs removing, one edit per page per session is not a
/// limitation, it is the feature not working.
#[test]
fn vector_edits_accumulate_in_one_session() {
    let base = edit_fixture();
    let mut s = session(&base);
    s.move_object(0, 0, 1.0, 1.0)
        .expect("the first edit succeeds");
    s.move_object(0, 1, 1.0, 1.0)
        .expect("a second edit on the same page must compose on top of the first");
    assert_eq!(s.undo_depth(), 2, "each edit is its own undoable command");

    // Both edits are really present: undoing once must leave the document
    // still modified (the first edit standing), and undoing twice must return
    // it to pristine. Asserting the DEPTH alone would pass even if the second
    // command had staged nothing.
    s.undo().expect("undo the second edit");
    assert!(
        s.is_modified(),
        "one undo must leave the first edit in place"
    );
    s.undo().expect("undo the first edit");
    assert!(!s.is_modified(), "undoing both must return to pristine");
}

/// **Two subpath deletes in one session remove the two parts that were
/// asked for** — the invariant the old one-edit-per-page refusal existed to
/// protect, now held by construction instead.
///
/// This is the test that matters for accumulation. Undo depth proves commands
/// were recorded; it says nothing about whether the SECOND index still meant
/// what the caller thought. Here the second delete is planned against content
/// the first already rewrote, and the assertion is geometric: after removing
/// part 0 and then part 0 again, the one line left must be the third.
///
/// If `vector_surgery` ever went back to decomposing the base, the second
/// delete would index into stale bytes and this would fail — which is exactly
/// what it is for.
#[test]
fn two_subpath_deletes_in_one_session_remove_the_right_two_parts() {
    let base = std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/synthetic/vector/multi-subpath-one-object.pdf"),
    )
    .expect("the multi-subpath fixture loads");
    let mut s = session(&base);

    // The fixture's six subpaths start at x = 50, 50, 50, 100, 200, 300.
    // Removing index 0 twice leaves the third horizontal plus the verticals.
    let before = decompose0(&Document::from_bytes(base.clone()).unwrap());
    let subpaths_before = match &before.objects[0] {
        pdfcer_core::vector::VectorObject::Path(p) => p.subpaths.len(),
        _ => panic!("expected a path"),
    };
    assert_eq!(subpaths_before, 6, "fixture shape");

    s.delete_subpath(0, 0, 0).expect("first delete");
    s.delete_subpath(0, 0, 0)
        .expect("a SECOND delete on the same page must plan against the first's result");
    assert_eq!(s.undo_depth(), 2);

    let after = Document::from_bytes(save(&s)).expect("the saved file re-parses");
    let model = decompose0(&after);
    let path = match &model.objects[0] {
        pdfcer_core::vector::VectorObject::Path(p) => p,
        _ => panic!("expected a path"),
    };
    assert_eq!(
        path.subpaths.len(),
        4,
        "two deletes must remove exactly two subpaths — not one (the second \
         silently lost) and not three (the second misindexed)"
    );
    let first_xs: Vec<f64> = path
        .subpaths
        .iter()
        .filter_map(|sp| sp.anchors().next().map(|p| p.x))
        .collect();
    assert_eq!(
        first_xs,
        vec![50.0, 100.0, 200.0, 300.0],
        "the two removed must be the FIRST two horizontals; a stale second \
         index would have taken a different pair"
    );
}

#[test]
fn an_out_of_range_object_index_is_refused() {
    let base = edit_fixture();
    let mut s = session(&base);
    let err = s.move_object(0, 999, 1.0, 1.0).unwrap_err();
    assert!(
        matches!(
            err,
            EditError::VectorEdit(VectorEditError::ObjectOutOfRange { index: 999, .. })
        ),
        "got {err:?}"
    );
}

// ---------------------------------------------------------------------------
// Compressed content stream (§5.7/§5.9 — decode, edit, re-emit raw)
// ---------------------------------------------------------------------------

fn zlib(data: &[u8]) -> Vec<u8> {
    let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    e.write_all(data).unwrap();
    e.finish().unwrap()
}

/// A classic PDF whose single page content stream is **FlateDecode**
/// compressed (§7.4.4). Editing it forces the decode → surgery → raw
/// re-emit path: the output content stream must decode to the moved
/// geometry AND carry no `/Filter` (no stale compressed copy, §5.7).
fn flate_content_pdf(raw_content: &[u8]) -> Vec<u8> {
    let data = zlib(raw_content);
    let bodies: Vec<(u32, Vec<u8>)> = vec![
        (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
        (
            2,
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 300 300] >>".to_vec(),
        ),
        (
            3,
            b"<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources << >> >>".to_vec(),
        ),
        (4, {
            let mut v = format!(
                "<< /Filter /FlateDecode /Length {} >>\nstream\n",
                data.len()
            )
            .into_bytes();
            v.extend_from_slice(&data);
            v.extend_from_slice(b"\nendstream");
            v
        }),
    ];
    let mut buf = b"%PDF-1.5\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::new();
    for (num, body) in &bodies {
        offsets.push((*num, buf.len()));
        buf.extend_from_slice(format!("{num} 0 obj\n").as_bytes());
        buf.extend_from_slice(body);
        buf.extend_from_slice(b"\nendobj\n");
    }
    let xref_at = buf.len();
    buf.extend_from_slice(b"xref\n0 5\n0000000000 65535 f \n");
    for num in 1..=4 {
        let off = offsets.iter().find(|(n, _)| *n == num).unwrap().1;
        buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size 5 /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n").as_bytes(),
    );
    buf
}

#[test]
fn editing_a_compressed_content_stream_decodes_edits_and_reemits_raw() {
    let base = flate_content_pdf(b"1 w 0 0 0 RG\n50 50 m 150 150 l S\n");
    let mut s = session(&base);

    // The content stream decodes to the line; move it +10,+10.
    s.move_object(0, 0, 10.0, 10.0).unwrap();
    let out = save(&s);

    let back = Document::from_bytes(out).unwrap();
    // The edit took — the decoded, re-decomposed geometry moved.
    let model = decompose0(&back);
    let (_, line) = first_path(&model);
    assert_eq!(first_start(line), Point::new(60.0, 60.0));

    // No stale copy: the edited content stream carries no /Filter (it was
    // re-emitted raw), so the OLD compressed bytes are not what a reader sees.
    let cid = content_id(&back);
    let Some(Object::Stream(stream)) = back.get(cid).map(|io| &io.value) else {
        panic!("content object is not a stream after the edit");
    };
    assert!(
        stream.dict.get(b"Filter").is_none(),
        "the edited content stream must be re-emitted raw (no /Filter)"
    );
}

// ---------------------------------------------------------------------------
// Object-stream page (§7.5.7 — surgery leaves compressed siblings verbatim)
// ---------------------------------------------------------------------------

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

/// A cross-reference-stream file whose **page + catalog live in an object
/// stream** (§7.5.7), with a file-level uncompressed content stream. Editing
/// the content stream must not perturb — or need to promote — the compressed
/// siblings; they stay verbatim inside the untouched container (no stale
/// copy of the kind §5.7 warns about).
fn objstm_page_pdf() -> Vec<u8> {
    let content = b"1 w 0 0 0 RG\n50 50 m 150 150 l S\n";
    let mut buf = b"%PDF-1.5\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets: Vec<(u32, usize)> = Vec::new();
    let mut push = |buf: &mut Vec<u8>, num: u32, body: &[u8]| {
        offsets.push((num, buf.len()));
        buf.extend_from_slice(format!("{num} 0 obj\n").as_bytes());
        buf.extend_from_slice(body);
        buf.extend_from_slice(b"\nendobj\n");
    };
    // Object 4 = the file-level content stream (streams cannot live in an
    // objstm, §7.5.7).
    push(&mut buf, 4, &{
        let mut v = format!("<< /Length {} >>\nstream\n", content.len()).into_bytes();
        v.extend_from_slice(content);
        v.extend_from_slice(b"\nendstream");
        v
    });

    // Catalog (1) and Page (3) inside an object stream (object 5).
    let inner: [(u32, &str); 2] = [
        (1, "<< /Type /Catalog /Pages 2 0 R >>"),
        (
            3,
            "<< /Type /Page /Parent 2 0 R /Contents 4 0 R /MediaBox [0 0 300 300] /Resources << >> >>",
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
    let objstm_body = format!(
        "<< /Type /ObjStm /N 2 /First {first} /Length {} >>\nstream\n{data}\nendstream",
        data.len()
    );
    push(&mut buf, 5, objstm_body.as_bytes());
    // The Pages node (2) at file level.
    push(&mut buf, 2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>");

    let xref_num = 6u32;
    let xref_at = buf.len();
    offsets.push((xref_num, xref_at));
    let size = xref_num + 1;
    let w = [1usize, 4, 2];
    let rows: Vec<(u64, u64, u64)> = (0..size)
        .map(|num| match num {
            0 => (0, 0, 65_535),
            1 => (2, 5, 0), // catalog: in objstm 5, index 0
            3 => (2, 5, 1), // page: in objstm 5, index 1
            _ => offsets
                .iter()
                .find(|(n, _)| *n == num)
                .map_or((0, 0, 0), |(_, off)| (1, u64::try_from(*off).unwrap(), 0)),
        })
        .collect();
    let data = zlib(&pack(&rows, w));
    let dict = format!(
        "<< /Type /XRef /Size {size} /W [1 4 2] /Root 1 0 R /Filter /FlateDecode /Length {} >>",
        data.len()
    );
    buf.extend_from_slice(format!("{xref_num} 0 obj\n{dict}\nstream\n").as_bytes());
    buf.extend_from_slice(&data);
    buf.extend_from_slice(b"\nendstream\nendobj\n");
    buf.extend_from_slice(format!("startxref\n{xref_at}\n%%EOF\n").as_bytes());
    buf
}

#[test]
fn editing_a_file_level_content_stream_leaves_the_objstm_siblings_verbatim() {
    let base = objstm_page_pdf();
    let before = Document::from_bytes(base.clone()).unwrap();
    // Precondition: the page lives in an object stream.
    assert!(
        matches!(
            before.get(ObjId::new(3, 0)).unwrap().provenance,
            Provenance::ObjectStream { .. }
        ),
        "fixture precondition: the page must be compressed"
    );

    let mut s = session(&base);
    s.move_object(0, 0, 5.0, 5.0).unwrap();
    let out = save(&s);

    let after = Document::from_bytes(out).unwrap();
    // The edit took.
    let model = decompose0(&after);
    let (_, line) = first_path(&model);
    assert_eq!(first_start(line), Point::new(55.0, 55.0));
    // The compressed page/catalog were NOT promoted or perturbed — the
    // surgery only touched the file-level content stream, so the object
    // stream and its members stay exactly where they were (no stale copy,
    // no spurious promotion).
    assert!(matches!(
        after.get(ObjId::new(3, 0)).unwrap().provenance,
        Provenance::ObjectStream { .. }
    ));
    assert!(matches!(
        after.get(ObjId::new(1, 0)).unwrap().provenance,
        Provenance::ObjectStream { .. }
    ));

    // And undo is byte-identical here too.
    s.undo();
    assert_eq!(save(&s), base);
}

// ---------------------------------------------------------------------------
// Pass 219.0 -- recolouring page objects, and refusing the inks that must not
// be recoloured.
// ---------------------------------------------------------------------------

/// A page with two filled rectangles: one in `DeviceRGB`, one in a
/// `/Separation`.
///
/// ★ The two are painted in that ORDER deliberately. The Separation path's
/// stale colour — the defect `Pass 218.0` fixed — would be the red from the
/// first rectangle, so a test built on this fixture fails loudly if the model
/// ever regresses to inheriting it.
fn two_paths_one_spot() -> Vec<u8> {
    let content = b"1 0 0 rg 0 0 20 20 re f\n/CS0 cs 1 scn 40 0 20 20 re f\n".to_vec();
    let mut objs: Vec<String> = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Resources << /ColorSpace \
         << /CS0 5 0 R >> >> /Contents 4 0 R >>"
            .to_owned(),
        format!(
            "<< /Length {} >>\nstream\n{}endstream",
            content.len(),
            String::from_utf8_lossy(&content)
        ),
        "[/Separation /SpotGreen /DeviceCMYK 6 0 R]".to_owned(),
        "<< /FunctionType 2 /Domain [0 1] /C0 [0 0 0 0] /C1 [1 0 1 0] /N 1 >>".to_owned(),
    ];
    let mut buf = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objs.drain(..).enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref_at = buf.len();
    let size = offsets.len() + 1;
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

/// ★★★ THE ONE THAT MATTERS. A spot-inked path is REFUSED BY NAME, and the
/// device path beside it is recoloured in the same call.
///
/// Writing `DeviceRGB` over a named spot ink would look right on screen and
/// destroy the printing plate — invisibly, which is the worst combination
/// available. The consuming shell asked for exactly this: *"a selection of
/// twelve strokes where three are in a colour space pdfcer will not rewrite
/// needs to say 'nine changed', not 'done'."*
#[test]
fn a_spot_inked_path_is_refused_by_name_while_its_neighbour_is_recoloured() {
    let mut s = session(&two_paths_one_spot());
    let out = s
        .set_object_paint(
            0,
            &[0, 1],
            Some(Rgb {
                r: 0.0,
                g: 0.0,
                b: 1.0,
            }),
            None,
        )
        .expect("the call succeeds; the refusal is DATA, not an error");

    assert_eq!(
        out.changed,
        vec![0],
        "only the DeviceRGB path was recoloured"
    );
    assert_eq!(out.refused.len(), 1, "and the spot one was refused");
    let r = &out.refused[0];
    assert_eq!(r.object, 1);
    assert_eq!(r.reason, PaintRefusalReason::UndecodedColourSpace);
    assert_eq!(
        r.space.as_deref(),
        Some(b"CS0".as_slice()),
        "★ named, not merely counted -- an operator must learn WHICH lines"
    );

    // The new colour is in the file, and the spot path's own bytes are not.
    let text = String::from_utf8_lossy(&save(&s)).into_owned();
    assert!(text.contains(" rg"), "a fill colour was written: {text}");
    assert!(
        text.contains("1 scn"),
        "★ and the spot ink survives verbatim -- the whole point of refusing"
    );
}

/// Refusing is per CHANNEL, not per object.
///
/// Recolouring the FILL of an object whose STROKE is a spot ink is legitimate
/// and must not be blocked by the channel nobody touched. The fixture's spot
/// path has no stroke at all, so asking to change only the stroke must not
/// refuse it for its fill.
#[test]
fn a_channel_nobody_touched_does_not_cause_a_refusal() {
    let mut s = session(&two_paths_one_spot());
    let out = s
        .set_object_paint(
            0,
            &[1],
            None,
            Some(Rgb {
                r: 0.0,
                g: 1.0,
                b: 0.0,
            }),
        )
        .expect("succeeds");
    assert!(
        out.refused.is_empty(),
        "the spot ink is on the FILL; a stroke-only change must not refuse it: {:?}",
        out.refused
    );
    assert_eq!(out.changed, vec![1]);
}

/// One undoable command, and undo restores the bytes exactly.
#[test]
fn recolouring_is_one_command_and_undo_restores_the_content() {
    let mut s = session(&two_paths_one_spot());
    let before = save(&s);
    s.set_object_paint(
        0,
        &[0],
        Some(Rgb {
            r: 0.0,
            g: 0.0,
            b: 1.0,
        }),
        None,
    )
    .expect("succeeds");
    assert_eq!(s.undo_depth(), 1, "exactly one undo entry for one gesture");
    s.undo().expect("undo");
    assert_eq!(save(&s), before, "undo restores the document byte for byte");
}
