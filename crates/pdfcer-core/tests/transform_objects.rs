//! `Pass 113.0`/`113.1` — `transform_objects` and `transform_preview`:
//! scale, rotate, shear or move a whole selection by one page-space matrix.
//!
//! ## The verb the consuming shell was blocked on, and the mechanism it assumed
//!
//! `pdfcer-gui` asked for `transform_objects` on the reasoning that
//! `move_objects` already existed and just needed a matrix. **It did not.**
//! `move_objects` rewrites numeric *operands*, and operand rewriting can
//! express translation and nothing else — a rotated rectangle has no `re`
//! spelling, `line_width` is a user-space scalar a coordinate scale leaves
//! behind, and neither text nor images carry coordinate operands at all
//! (which is exactly why that verb refuses them with `NotAPath`).
//!
//! So this wraps each object's operator run in `q <cm> … Q`, which never looks
//! at an operand and is therefore **kind-agnostic by construction** — which is
//! the requester's own argument that *"a placed image and a placed text run
//! are the same shape"*, granted by the mechanism rather than by a match arm
//! per kind.
//!
//! ## What these tests pin, hardest first
//!
//! 1. **★ The emitted matrix is not the one passed in.** `page_matrix` is in
//!    page space; `cm` composes into the CTM in force at that point in the
//!    stream. Emitting the requested matrix directly is correct **only** when
//!    the object's CTM is the identity — and silently wrong at every scale or
//!    slant a producer left in force. `local_ctm_is_compensated` is the test
//!    that fails if that compensation is ever dropped, and it is the one worth
//!    reading first.
//! 2. **Both `R206` options exist and the defaults are the stated ones** —
//!    mixed selections transform whole, a singular matrix is refused by name.
//! 3. **The preview and the verb cannot disagree**, because they share one
//!    body. `preview(..).is_ok()` **is** the predicate.
//! 4. **Untouched objects stay byte-verbatim** and undo restores the file
//!    exactly (`R46`, `ARCHITECTURE.md` §11.1).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::content::ContentStream;
use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::vector::{
    Matrix, MixedSelection, NoXObjects, Point, SingularPolicy, TransformOptions, VectorEditError,
    decompose, plan_transform_many,
};
use pdfcer_core::writer::SaveOptions;

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

/// A one-page PDF whose content is `content`, with a Helvetica `/F1` and an
/// image XObject `/Im1`, so a selection can mix all three object kinds.
fn pdf(content: &str) -> Vec<u8> {
    let image = "<< /Type /XObject /Subtype /Image /Width 1 /Height 1 \
                 /ColorSpace /DeviceGray /BitsPerComponent 8 /Length 1 >>\nstream\n\u{0}\nendstream";
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R \
         /Resources << /Font << /F1 5 0 R >> /XObject << /Im1 6 0 R >> >> >>"
            .to_owned(),
        format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len() + 1
        ),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"
            .to_owned(),
        image.to_owned(),
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

/// Plan a transform over the whole decomposition of an inline content stream.
fn plan_all(src: &str, m: Matrix, opts: TransformOptions) -> Result<Vec<u8>, VectorEditError> {
    let cs = ContentStream::parse(src.as_bytes().to_vec()).unwrap();
    let model = decompose(&cs, Matrix::IDENTITY, &NoXObjects);
    let refs: Vec<_> = model.objects.iter().collect();
    plan_transform_many(&cs, &refs, m, opts).map(|p| p.content)
}

/// The page-space bounding box of object `index` after re-decomposing `src`.
fn bbox_of(src: &str, index: usize) -> pdfcer_core::vector::Bounds {
    let cs = ContentStream::parse(src.as_bytes().to_vec()).unwrap();
    let model = decompose(&cs, Matrix::IDENTITY, &NoXObjects);
    model.objects[index].page_bbox()
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

// ---------------------------------------------------------------------------
// ★ THE ONE THAT MATTERS: page space vs local space
// ---------------------------------------------------------------------------

/// ★★ **A page-space transform under a non-identity CTM must land in PAGE
/// space.**
///
/// The rectangle below is drawn inside `q 2 0 0 2 0 0 cm`, so its user-space
/// `0 0 10 10 re` covers page-space 0,0 → 20,20. Asked to move it **10 page
/// units right**, it must end at 10,0 → 30,20.
///
/// The naive implementation — emit the requested matrix as the `cm` operand —
/// would move it **20** page units, because a `translate(10, 0)` composed into
/// a doubled CTM translates by 10 *user* units. Nothing errors, nothing looks
/// malformed, and the object lands twice as far as the pointer went.
///
/// The compensation is `X = CTM × M × CTM⁻¹`. This test is what fails if it is
/// ever simplified away.
#[test]
fn local_ctm_is_compensated_so_a_page_space_move_lands_in_page_space() {
    const SRC: &str = "q 2 0 0 2 0 0 cm 0 0 10 10 re S Q";
    let before = bbox_of(SRC, 0);
    assert!(
        close(before.min.x, 0.0) && close(before.max.x, 20.0),
        "the fixture must start at page-space 0..20, got {before:?}"
    );

    let out = plan_all(
        SRC,
        Matrix::translate(10.0, 0.0),
        TransformOptions::default(),
    )
    .unwrap();
    let after = bbox_of(std::str::from_utf8(&out).unwrap(), 0);
    assert!(
        close(after.min.x, 10.0) && close(after.max.x, 30.0),
        "a 10-unit PAGE-space move under a 2x CTM must move 10 page units, not 20 -- got {after:?} from {}",
        String::from_utf8_lossy(&out)
    );
    assert!(
        close(after.min.y, before.min.y) && close(after.max.y, before.max.y),
        "a horizontal move must not touch y: {after:?}"
    );
}

/// The identity-CTM case, where the naive implementation happens to be right —
/// pinned so that a fix to the test above cannot be "make everything behave
/// like the compensated case even when it should not".
#[test]
fn an_identity_ctm_emits_the_requested_matrix_unchanged() {
    const SRC: &str = "0 0 10 10 re S";
    let out = plan_all(
        SRC,
        Matrix::translate(7.0, 3.0),
        TransformOptions::default(),
    )
    .unwrap();
    let text = String::from_utf8(out).unwrap();
    assert!(
        text.starts_with("q 1 0 0 1 7 3 cm "),
        "expected the requested matrix verbatim, got {text}"
    );
    assert!(text.ends_with(" Q"), "the wrap must be closed: {text}");
    assert!(
        text.contains("0 0 10 10 re S"),
        "the object's own bytes must pass through verbatim: {text}"
    );
}

// ---------------------------------------------------------------------------
// The capability move_objects could not have
// ---------------------------------------------------------------------------

/// ★ **Rotation, which is the whole reason operand rewriting could not be
/// extended.**
///
/// A quarter-turn about the rectangle's own centre leaves a square's bbox
/// where it was — the strongest simple assertion available, because a wrap
/// that silently did nothing would also pass a "bbox unchanged" check on its
/// own. So the emitted operand is checked too: a rotation is present in the
/// bytes, and the `re` survives **unexpanded**, which is the property operand
/// rewriting cannot deliver at all.
#[test]
fn a_rotation_wraps_rather_than_expanding_the_rectangle() {
    const SRC: &str = "0 0 10 10 re S";
    let out = plan_all(
        SRC,
        Matrix::rotate(std::f64::consts::FRAC_PI_2).about(Point::new(5.0, 5.0)),
        TransformOptions::default(),
    )
    .unwrap();
    let text = String::from_utf8(out).unwrap();
    assert!(
        text.contains("0 0 10 10 re"),
        "the rectangle must stay an `re` -- expanding it is what wrapping avoids: {text}"
    );
    let after = bbox_of(&text, 0);
    assert!(
        close(after.min.x, 0.0)
            && close(after.max.x, 10.0)
            && close(after.min.y, 0.0)
            && close(after.max.y, 10.0),
        "a square rotated a quarter-turn about its own centre occupies the same box: {after:?}"
    );
}

/// A scale about a pivot leaves the pivot fixed and doubles the extent — the
/// resize gesture, end to end.
#[test]
fn a_scale_about_a_pivot_leaves_the_pivot_where_it_was() {
    const SRC: &str = "10 10 10 10 re S";
    let out = plan_all(
        SRC,
        Matrix::scale(2.0, 2.0).about(Point::new(10.0, 10.0)),
        TransformOptions::default(),
    )
    .unwrap();
    let after = bbox_of(std::str::from_utf8(&out).unwrap(), 0);
    assert!(
        close(after.min.x, 10.0) && close(after.min.y, 10.0),
        "the pivot corner must not move: {after:?}"
    );
    assert!(
        close(after.max.x, 30.0) && close(after.max.y, 30.0),
        "the opposite corner must double away from it: {after:?}"
    );
}

// ---------------------------------------------------------------------------
// Kind-agnosticism — the triggering complaint
// ---------------------------------------------------------------------------

/// ★ **A mixed selection transforms whole, by default.** This is the
/// `NotAPath` complaint closed: a path, a text object and a placed image in
/// one marquee, moved by one gesture, as one command.
#[test]
fn a_mixed_selection_of_all_three_kinds_transforms_whole() {
    let content = "0 0 10 10 re S\nBT /F1 12 Tf 20 20 Td (hi) Tj ET\nq 5 0 0 5 40 40 cm /Im1 Do Q";
    let doc = Document::from_bytes(pdf(content)).unwrap();
    let mut session = EditSession::new(doc);

    let outcome = session
        .transform_objects(
            0,
            &[0, 1, 2],
            Matrix::translate(5.0, 0.0),
            TransformOptions::default(),
        )
        .expect("a mixed selection is transformable");
    assert_eq!(
        outcome.objects_transformed, 3,
        "all three kinds, one command: {outcome:?}"
    );
    assert!(!outcome.clamped);
    assert_eq!(session.undo_depth(), 1, "one gesture is one undo entry");
}

/// The opt-in single-kind semantics refuse **by name**, naming both kinds so
/// a shell can word its own message.
#[test]
fn refuse_heterogeneous_is_available_and_names_both_kinds() {
    let content = "0 0 10 10 re S\nBT /F1 12 Tf 20 20 Td (hi) Tj ET";
    let doc = Document::from_bytes(pdf(content)).unwrap();
    let mut session = EditSession::new(doc);

    let err = session
        .transform_objects(
            0,
            &[0, 1],
            Matrix::translate(5.0, 0.0),
            TransformOptions::default().with_mixed(MixedSelection::RefuseHeterogeneous),
        )
        .expect_err("single-kind semantics were requested");
    let message = err.to_string();
    assert!(
        message.contains("path") && message.contains("text"),
        "the refusal must name both kinds: {message}"
    );
    assert_eq!(session.undo_depth(), 0, "a refusal mutates nothing");
}

// ---------------------------------------------------------------------------
// The singular case — R206's second question
// ---------------------------------------------------------------------------

/// **Default: refuse a singular transform by name.** It is irrecoverable —
/// there is no inverse, so no later gesture restores the object.
#[test]
fn a_singular_transform_is_refused_by_name_by_default() {
    let err = plan_all(
        "0 0 10 10 re S",
        Matrix::scale(1.0, 0.0),
        TransformOptions::default(),
    )
    .expect_err("zero area is refused");
    assert!(
        matches!(err, VectorEditError::SingularTransform),
        "got {err:?}"
    );
}

/// ★ **A NEGATIVE scale is not singular.** A mirror is perfectly invertible,
/// so dragging a resize grip through the *opposite* edge is an ordinary
/// transform — only exactly zero is degenerate.
///
/// Pinned because the obvious guard ("refuse a non-positive scale") would
/// break mirroring while passing every singular test above, and mirroring is
/// a gesture an operator makes on purpose.
#[test]
fn a_negative_scale_is_a_mirror_and_is_not_refused() {
    let out = plan_all(
        "0 0 10 10 re S",
        Matrix::scale(-1.0, 1.0),
        TransformOptions::default(),
    )
    .expect("a mirror is invertible");
    assert!(!out.is_empty());
}

/// The clamp option applies, discloses, and says so on the outcome.
#[test]
fn the_clamp_option_clamps_and_discloses() {
    let doc = Document::from_bytes(pdf("0 0 10 10 re S")).unwrap();
    let mut session = EditSession::new(doc);
    let outcome = session
        .transform_objects(
            0,
            &[0],
            Matrix::scale(1.0, 0.0),
            TransformOptions::default().with_singular(SingularPolicy::Clamp { min: 0.01 }),
        )
        .expect("the clamp policy applies rather than refusing");
    assert!(
        outcome.clamped,
        "the outcome must SAY it clamped: {outcome:?}"
    );
    assert!(
        outcome.disclosures.iter().any(|d| d.contains("CLAMPED")),
        "and disclose it in words: {:?}",
        outcome.disclosures
    );
}

/// A clamp that cannot be expressed is refused **by name**, naming the option
/// rather than the gesture.
///
/// A singular matrix that also shears has no single "the scale that went to
/// zero" — its degeneracy is a direction, not an axis — and inventing one
/// would be pdfcer choosing a shape the operator did not draw.
#[test]
fn a_clamp_on_a_sheared_singular_matrix_is_refused_rather_than_invented() {
    let sheared = Matrix {
        a: 1.0,
        b: 1.0,
        c: 1.0,
        d: 1.0,
        e: 0.0,
        f: 0.0,
    };
    assert!(!sheared.is_invertible(), "the fixture must be singular");
    let err = plan_all(
        "0 0 10 10 re S",
        sheared,
        TransformOptions::default().with_singular(SingularPolicy::Clamp { min: 0.01 }),
    )
    .expect_err("there is nothing well-defined to clamp");
    assert!(
        matches!(err, VectorEditError::ClampNotExpressible),
        "got {err:?}"
    );
}

// ---------------------------------------------------------------------------
// The preview — Pass 113.1
// ---------------------------------------------------------------------------

/// ★ **The preview and the verb share one body, so they cannot disagree.**
///
/// Asserted over four cases — a good transform, a singular one, a stale index
/// and a refused mixed selection — rather than over one, because "they agree
/// on the happy path" is what a *second* implementation would also manage.
#[test]
fn the_preview_answers_exactly_what_the_verb_would() {
    let content = "0 0 10 10 re S\nBT /F1 12 Tf 20 20 Td (hi) Tj ET";
    let cases: Vec<(&[usize], Matrix, TransformOptions)> = vec![
        (
            &[0],
            Matrix::translate(5.0, 0.0),
            TransformOptions::default(),
        ),
        (&[0], Matrix::scale(0.0, 1.0), TransformOptions::default()),
        (
            &[99],
            Matrix::translate(5.0, 0.0),
            TransformOptions::default(),
        ),
        (
            &[0, 1],
            Matrix::translate(5.0, 0.0),
            TransformOptions::default().with_mixed(MixedSelection::RefuseHeterogeneous),
        ),
    ];
    for (indices, matrix, options) in cases {
        let doc = Document::from_bytes(pdf(content)).unwrap();
        let mut session = EditSession::new(doc);
        let previewed = session.transform_preview(0, indices, matrix, options);
        assert_eq!(
            session.undo_depth(),
            0,
            "a preview must commit nothing, for {indices:?}"
        );
        let applied = session.transform_objects(0, indices, matrix, options);
        assert_eq!(
            previewed.is_ok(),
            applied.is_ok(),
            "preview and verb disagree for {indices:?}: {previewed:?} vs {applied:?}"
        );
        if let (Ok(p), Ok(a)) = (&previewed, &applied) {
            assert_eq!(p.objects_transformed, a.objects_transformed);
            assert_eq!(p.clamped, a.clamped);
            assert_eq!(p.disclosures, a.disclosures);
        }
    }
}

// ---------------------------------------------------------------------------
// Selection hygiene, minimal diff, undo
// ---------------------------------------------------------------------------

/// Duplicate indices collapse to one wrap.
///
/// Wrapping the same span twice would apply the transform to those marks
/// **twice** — the one arithmetic error here that renders as *almost* right,
/// which is the hardest kind to notice.
#[test]
fn duplicate_indices_are_wrapped_once() {
    let doc = Document::from_bytes(pdf("0 0 10 10 re S")).unwrap();
    let mut session = EditSession::new(doc);
    let outcome = session
        .transform_objects(
            0,
            &[0, 0, 0],
            Matrix::translate(5.0, 0.0),
            TransformOptions::default(),
        )
        .unwrap();
    assert_eq!(
        outcome.objects_transformed, 1,
        "three mentions of one object are one wrap: {outcome:?}"
    );
}

/// A stale index refuses the WHOLE call (`R168`), rather than transforming the
/// part of the selection that happened to resolve.
#[test]
fn one_stale_index_refuses_the_whole_selection() {
    let doc = Document::from_bytes(pdf("0 0 10 10 re S")).unwrap();
    let mut session = EditSession::new(doc);
    let err = session
        .transform_objects(
            0,
            &[0, 42],
            Matrix::translate(5.0, 0.0),
            TransformOptions::default(),
        )
        .expect_err("42 is not on this page");
    assert!(err.to_string().contains("42"), "{err}");
    assert_eq!(session.undo_depth(), 0);
}

/// An unselected object is re-emitted **byte-verbatim** — the minimal-diff
/// invariant, checked on the content rather than claimed.
#[test]
fn an_unselected_object_is_not_touched() {
    const SRC: &str = "0 0 10 10 re S\n50 50 5 5 re f";
    let cs = ContentStream::parse(SRC.as_bytes().to_vec()).unwrap();
    let model = decompose(&cs, Matrix::IDENTITY, &NoXObjects);
    let only_first = vec![&model.objects[0]];
    let out = plan_transform_many(
        &cs,
        &only_first,
        Matrix::translate(5.0, 0.0),
        TransformOptions::default(),
    )
    .unwrap();
    let text = String::from_utf8(out.content).unwrap();
    assert!(
        text.contains("50 50 5 5 re f"),
        "the unselected object must survive verbatim: {text}"
    );
    assert_eq!(text.matches('q').count(), 1, "exactly one wrap: {text}");
}

/// Transform → undo → save produces a byte-identical file (`ARCHITECTURE.md`
/// §11.1: the dirty set is a diff against the base, never a log of what was
/// touched).
#[test]
fn transform_then_undo_leaves_no_trace_in_the_save() {
    let doc = Document::from_bytes(pdf("0 0 10 10 re S")).unwrap();
    let mut session = EditSession::new(doc);
    session
        .transform_objects(
            0,
            &[0],
            Matrix::translate(5.0, 0.0),
            TransformOptions::default(),
        )
        .unwrap();
    session.undo().expect("there is a command to undo");
    let (_bytes, report) = session
        .to_incremental_bytes(&SaveOptions::identity())
        .unwrap();
    assert_eq!(
        report.objects_written, 0,
        "a transformed-then-undone page must appear in no update section: {report:?}"
    );
}

/// An empty selection is not an error — a caller need not special-case it.
#[test]
fn an_empty_selection_plans_an_unchanged_buffer() {
    const SRC: &str = "0 0 10 10 re S";
    let cs = ContentStream::parse(SRC.as_bytes().to_vec()).unwrap();
    let out = plan_transform_many(
        &cs,
        &[],
        Matrix::translate(5.0, 0.0),
        TransformOptions::default(),
    )
    .unwrap();
    assert_eq!(out.content, SRC.as_bytes());
    assert_eq!(out.operators_touched, 0);
}
