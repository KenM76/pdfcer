//! `plan_move_text_runs` / `EditSession::move_text_runs` — moving a SET of
//! show operators inside one text object as one edit (`G030`).
//!
//! The central test is exhaustive: for every fixture and every subset of its
//! runs, either the guard refuses (and the planner refuses with the same
//! sentence), or the saved-and-reopened file has every run in the set moved by
//! exactly the delta and every run outside it exactly where it was.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use pdfcer_core::content::ContentStream;
use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditError, EditSession};
use pdfcer_core::page_tree;
use pdfcer_core::vector::edit::{plan_move_text_runs, text_run_move_refusal_of_set};
use pdfcer_core::vector::{
    Bounds, Matrix, TextObject, VectorEditError, VectorObject, decompose_page,
};
use pdfcer_core::writer::SaveOptions;

/// Operands are re-emitted as decimal text, so the round trip is not
/// bit-exact; a thousandth of a point is far below anything visible.
const EPS: f64 = 1e-3;

const FIXTURES: &[&str] = &[
    "runs-inherited.pdf",
    "runs-td-relative.pdf",
    "runs-tstar-leading.pdf",
    "runs-two-explicit.pdf",
    "runs-quote-show.pdf",
    "runs-rotated-td.pdf",
];

/// A `Tm` followed by a `Td` in one gap: the `Td` is relative to that `Tm`,
/// not to wherever the previous run left the line matrix.
const TM_THEN_TD: &str = "BT /F1 10 Tf 1 0 0 1 72 700 Tm (ALPHA) Tj \
    1 0 0 1 72 650 Tm 30 0 Td (BETA) Tj 30 0 Td (GAMMA) Tj ET";

/// Inherited runs on both lines, a `T*` between them, and a `'`.
const MIXED: &str = "BT /F1 10 Tf 14 TL 72 700 Td (ALPHA) Tj (BETA) Tj \
    T* (GAMMA) Tj (DELTA) ' 10 0 Td (EPS) Tj ET";

fn fixture_bytes(name: &str) -> Vec<u8> {
    let p: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/text")
        .join(name);
    std::fs::read(&p).unwrap_or_else(|e| panic!("missing fixture {}: {e}", p.display()))
}

/// A one-page PDF drawing `content` with standard-14 Helvetica as `/F1`, so
/// runs have real widths and non-empty boxes.
fn pdf_with_content(content: &str) -> Vec<u8> {
    let stream = format!(
        "<< /Length {} >>\nstream\n{content}\nendstream",
        content.len() + 1
    );
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] \
         /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>"
            .to_owned(),
        stream,
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"
            .to_owned(),
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

/// Every source the exhaustive test walks: the fixtures plus the synthetic
/// streams above, as `(label, bytes)`.
fn sources() -> Vec<(String, Vec<u8>)> {
    let mut out: Vec<(String, Vec<u8>)> = FIXTURES
        .iter()
        .map(|n| ((*n).to_owned(), fixture_bytes(n)))
        .collect();
    out.push(("TM_THEN_TD".to_owned(), pdf_with_content(TM_THEN_TD)));
    out.push(("MIXED".to_owned(), pdf_with_content(MIXED)));
    out
}

/// The first text object on page 0, its paint-order index, and the stream.
fn first_text(bytes: &[u8]) -> (ContentStream, usize, TextObject) {
    let doc = Document::from_bytes(bytes.to_vec()).expect("parses");
    let pages = page_tree::pages(&doc).expect("pages");
    let page = pages.first().expect("one page").clone();
    let cs = ContentStream::from_page(&doc.view(), &page).expect("content decodes");
    let model = decompose_page(&doc.view(), &page, Matrix::IDENTITY).expect("decomposes");
    let (i, t) = model
        .objects
        .iter()
        .enumerate()
        .find_map(|(i, o)| match o {
            VectorObject::Text(t) => Some((i, t.clone())),
            _ => None,
        })
        .expect("a text object");
    (cs, i, t)
}

fn boxes(bytes: &[u8]) -> Vec<Bounds> {
    first_text(bytes).2.runs.iter().map(|r| r.bounds).collect()
}

fn saved(s: &EditSession) -> Vec<u8> {
    s.to_incremental_bytes(&SaveOptions::identity())
        .expect("saves")
        .0
}

fn close(a: Bounds, b: Bounds, dx: f64, dy: f64) -> bool {
    (a.min.x + dx - b.min.x).abs() <= EPS
        && (a.min.y + dy - b.min.y).abs() <= EPS
        && (a.max.x + dx - b.max.x).abs() <= EPS
        && (a.max.y + dy - b.max.y).abs() <= EPS
}

/// **The whole claim**, over every subset of every source.
#[test]
fn every_accepted_set_moves_exactly_its_members() {
    let (dx, dy) = (7.0, -30.0);
    let mut accepted = 0;
    let mut refused = 0;
    for (label, bytes) in sources() {
        let (cs, object, text) = first_text(&bytes);
        let n = text.runs.len();
        assert!((2..=8).contains(&n), "{label}: expected 2..8 runs, got {n}");
        assert!(
            text.runs.iter().all(|r| r.bounds.max.x > r.bounds.min.x),
            "{label}: runs must have real boxes or this test proves nothing",
        );
        let before = boxes(&bytes);
        for mask in 1u32..(1 << n) {
            let set: Vec<usize> = (0..n).filter(|i| mask & (1 << i) != 0).collect();
            let pre = text_run_move_refusal_of_set(&text, &set);
            let planned = plan_move_text_runs(&cs, &text, &set, dx, dy);
            if let Some(refusal) = pre {
                let err = planned.expect_err("the planner must refuse what the guard refuses");
                assert_eq!(refusal.to_string(), err.to_string(), "{label} {set:?}");
                refused += 1;
                continue;
            }
            planned.unwrap_or_else(|e| panic!("{label} {set:?}: guard passed, planner said {e}"));
            let mut s = EditSession::new(Document::from_bytes(bytes.clone()).unwrap());
            s.move_text_runs(0, object, &set, dx, dy)
                .unwrap_or_else(|e| panic!("{label} {set:?}: {e}"));
            let after = boxes(&saved(&s));
            assert_eq!(after.len(), n, "{label} {set:?}: runs must not renumber");
            for i in 0..n {
                let (wx, wy) = if set.contains(&i) {
                    (dx, dy)
                } else {
                    (0.0, 0.0)
                };
                assert!(
                    close(before[i], after[i], wx, wy),
                    "{label} {set:?}: run {i} should move by ({wx},{wy}); {:?} -> {:?}",
                    before[i],
                    after[i],
                );
            }
            accepted += 1;
        }
    }
    assert!(
        accepted > 50 && refused > 10,
        "accepted={accepted} refused={refused}"
    );
}

/// `G030` §3: a line whose second fragment inherits its position moves as a
/// set, where the single-run verb refused it.
#[test]
fn a_line_with_an_inherited_fragment_moves_as_a_set() {
    let bytes = fixture_bytes("runs-inherited.pdf");
    let (_, object, _) = first_text(&bytes);
    let mut s = EditSession::new(Document::from_bytes(bytes.clone()).unwrap());
    assert!(matches!(
        s.move_text_run(0, object, 0, 0.0, -30.0),
        Err(EditError::VectorEdit(
            VectorEditError::MoveWouldMoveNextRun { index: 0 }
        ))
    ));
    let d = s.move_text_runs(0, object, &[1, 0, 1], 0.0, -30.0).unwrap();
    assert!(d.is_empty(), "a Tm rewrite owes no disclosure: {d:?}");
}

/// The refusals name the right run.
#[test]
fn refusals_name_the_run_that_would_tear() {
    let (_, _, text) = first_text(&fixture_bytes("runs-inherited.pdf"));
    assert_eq!(text.runs.len(), 4);
    assert_eq!(
        text_run_move_refusal_of_set(&text, &[0]),
        Some(VectorEditError::MoveWouldMoveNextRun { index: 0 })
    );
    assert_eq!(
        text_run_move_refusal_of_set(&text, &[1]),
        Some(VectorEditError::TextRunHasNoPositionOfItsOwn { index: 1 })
    );
    assert_eq!(
        text_run_move_refusal_of_set(&text, &[0, 1, 3]),
        Some(VectorEditError::TextRunHasNoPositionOfItsOwn { index: 3 })
    );
    assert_eq!(
        text_run_move_refusal_of_set(&text, &[0, 1, 2]),
        Some(VectorEditError::MoveWouldMoveNextRun { index: 2 })
    );
    assert_eq!(
        text_run_move_refusal_of_set(&text, &[]),
        Some(VectorEditError::EmptyTextRunMove)
    );
    assert_eq!(
        text_run_move_refusal_of_set(&text, &[0, 9]),
        Some(VectorEditError::TextRunOutOfRange { index: 9, count: 4 })
    );
}

/// Moving a whole object's runs rewrites only its `Tm`: the relative `Td`
/// that places the second line is left alone.
#[test]
fn moving_every_run_rewrites_only_the_absolute_placement() {
    let (cs, _, text) = first_text(&fixture_bytes("runs-inherited.pdf"));
    let plan = plan_move_text_runs(&cs, &text, &[0, 1, 2, 3], 10.0, 0.0).unwrap();
    let out = String::from_utf8_lossy(&plan.content).into_owned();
    assert!(out.contains("1 0 0 1 82 700 Tm"), "{out}");
    assert!(out.contains("0 -20 Td"), "{out}");
    assert_eq!(plan.operators_touched, 1);
}

/// Several inserted `Td`s, one disclosure.
#[test]
fn inserted_operators_are_disclosed_once_per_call() {
    let (cs, _, text) = first_text(&fixture_bytes("runs-tstar-leading.pdf"));
    let plan = plan_move_text_runs(&cs, &text, &[0, 1], 5.0, 0.0).unwrap();
    assert_eq!(plan.operators_touched, 2, "run 0 moves, run 2 is put back");
    assert_eq!(plan.disclosures.len(), 1, "{:?}", plan.disclosures);
}

/// One gesture, one undo entry; undo restores the bytes exactly.
#[test]
fn a_set_move_is_one_undo_entry() {
    let bytes = fixture_bytes("runs-td-relative.pdf");
    let (_, object, _) = first_text(&bytes);
    let mut s = EditSession::new(Document::from_bytes(bytes.clone()).unwrap());
    let base = saved(&s);
    s.move_text_runs(0, object, &[1, 2], 3.0, 4.0).unwrap();
    assert!(s.is_modified());
    s.undo();
    assert!(!s.is_modified(), "one call, one undo entry");
    assert_eq!(saved(&s), base);
}
