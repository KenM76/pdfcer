//! # What one content edit actually costs on a dense CAD drawing
//!
//! An instrument, not a gate. `#[ignore]`d and self-skipping — it prints why
//! and passes when the subject drawing is not on the machine, because the file
//! is a real-world CAD export that `docs/LEGAL.md` §5 forbids checking in.
//!
//! ```text
//! cargo test -p pdfcer-core --release --test edit_latency -- --ignored --nocapture
//! ```
//!
//! ## Why this exists in `pdfcer` and not only in the shell
//!
//! `pdfcer-gui` measured this first and reported it (2026-08-30) as a boundary
//! finding: one edit costs two decompositions of the same page, one inside the
//! verb and one in the shell's cache rebuild, and neither side can see the
//! other's. Their instrument lives in their repository and depends only on
//! `pdfcer_core`.
//!
//! It is reproduced here for one reason: **a claim about the engine's cost
//! should be falsifiable inside the engine.** A number that only exists in a
//! consuming project's test cannot be re-run by the person changing the code
//! that produces it.
//!
//! ## ★ RELEASE MODE OR THE NUMBERS ARE MEANINGLESS
//!
//! `decompose` is a tight loop over content-stream operators. A debug build
//! exaggerates it by roughly an order of magnitude, which would turn a real
//! 450 ms into a fictional 5 s and send someone optimising the wrong thing.
//! The test prints the profile it was built in and says so.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;
use std::time::Instant;

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::page_tree;
use pdfcer_core::vector::{Matrix, decompose_page};

/// Decompose page 0 of `session`'s CURRENT (edited) view.
///
/// This is the call the shell makes, spelled the way the shell spells it —
/// `decompose_page` over `session.view()`, per decision 018. Measuring some
/// other entry point would measure something the shell does not do.
fn decompose(session: &EditSession) -> pdfcer_core::vector::PageObjects {
    let view = session.view();
    let pages = page_tree::pages_in(&view).expect("page tree");
    decompose_page(&view, &pages[0], Matrix::IDENTITY).expect("decomposes")
}

/// The subject drawing: a dense vector site plan, ~130k objects, 5.6 MB.
///
/// Absolute and outside the repository on purpose — it is a real-world CAD
/// export of unknown redistribution status, which rule 7 and `LEGAL.md` §5
/// keep out of `fixtures/`.
const SUBJECT: &str = r"D:\Dev\pdfTests\ncored-benchmark-cad-drawing.pdf";

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[v.len() / 2]
}

/// **What one `move_objects` costs, and how much of it is decomposition.**
///
/// The four numbers together answer the question the shell asked; any one of
/// them alone does not:
///
/// - **load** — is the cost the file? (No.)
/// - **decompose** — is it the content stream? (Yes.)
/// - **move_objects** — does the verb pay a whole decomposition? (Yes.)
/// - **second decompose** — is the shell's rebuild the same price again?
#[test]
#[ignore = "needs a real CAD drawing outside the repo; run explicitly"]
fn one_edit_costs_a_whole_decomposition() {
    if cfg!(debug_assertions) {
        println!(
            "\nedit_latency: SKIPPED — this is a DEBUG build and the numbers \
             would be off by roughly 10x.\n  re-run with --release."
        );
        return;
    }
    if !Path::new(SUBJECT).exists() {
        println!("\nedit_latency: SKIPPED — {SUBJECT} is not on this machine.");
        return;
    }

    let bytes = std::fs::read(SUBJECT).expect("the subject is readable");
    println!(
        "\nedit_latency: {} ({} bytes), release build",
        SUBJECT,
        bytes.len()
    );

    // --- load -------------------------------------------------------------
    let mut load = Vec::new();
    for _ in 0..5 {
        let t = Instant::now();
        let d = Document::from_bytes(bytes.clone()).expect("parses");
        load.push(t.elapsed().as_secs_f64() * 1000.0);
        drop(d);
    }
    println!("  Document::from_bytes           {:8.1} ms", median(load));

    // --- decompose --------------------------------------------------------
    let doc = Document::from_bytes(bytes.clone()).expect("parses");
    let session = EditSession::new(doc);
    let mut dec = Vec::new();
    let mut object_count = 0usize;
    for _ in 0..3 {
        let t = Instant::now();
        let objs = decompose(&session);
        dec.push(t.elapsed().as_secs_f64() * 1000.0);
        object_count = objs.objects.len();
    }
    let dec_ms = median(dec);
    println!("  page_objects (decompose)       {dec_ms:8.1} ms   ({object_count} objects)");

    // --- the verb ---------------------------------------------------------
    //
    // A FRESH session per sample. Reusing one would measure the second edit
    // against a page the previous edit had already rewritten, which is a
    // different (and larger) content stream — the numbers would drift upward
    // for a reason that has nothing to do with what is being asked.
    let mut mv = Vec::new();
    for _ in 0..3 {
        let d = Document::from_bytes(bytes.clone()).expect("parses");
        let mut s = EditSession::new(d);
        let t = Instant::now();
        let _ = s.move_objects(0, &[0], 1.0, 0.0);
        mv.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    let mv_ms = median(mv);
    println!("  move_objects (one object)      {mv_ms:8.1} ms");

    // --- the shell's rebuild ---------------------------------------------
    let doc2 = Document::from_bytes(bytes).expect("parses");
    let mut s2 = EditSession::new(doc2);
    let _ = s2.move_objects(0, &[0], 1.0, 0.0);
    let t = Instant::now();
    let after = decompose(&s2);
    let post_ms = t.elapsed().as_secs_f64() * 1000.0;
    println!(
        "  page_objects AFTER the edit    {post_ms:8.1} ms   ({} objects)",
        after.objects.len()
    );

    println!(
        "\n  UNCACHED  one drag = {:.0} ms verb + {:.0} ms shell rebuild = {:.0} ms of parsing",
        mv_ms,
        post_ms,
        mv_ms + post_ms
    );

    // --- the same drag, through the cache --------------------------------
    //
    // This is the shell's real sequence: decompose to obtain the indices,
    // then edit the object at one of them. The only change is that the first
    // step goes through `EditSession::page_objects` instead of the free
    // function, so the verb finds the model already built.
    let doc3 = Document::from_bytes(std::fs::read(SUBJECT).expect("readable")).expect("parses");
    let mut s3 = EditSession::new(doc3);

    let t = Instant::now();
    let pre = s3.page_objects(0).expect("decomposes");
    let cached_pre_ms = t.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(
        pre.objects.len(),
        object_count,
        "same model as the free function"
    );

    let t = Instant::now();
    let _ = s3.move_objects(0, &[0], 1.0, 0.0);
    let cached_mv_ms = t.elapsed().as_secs_f64() * 1000.0;

    // And a second lookup on UNCHANGED content, to show a hit is a hash.
    let t = Instant::now();
    let _ = s3.page_objects(0).expect("decomposes");
    let hit_ms = t.elapsed().as_secs_f64() * 1000.0;

    println!(
        // string-gap-exempt: the run after CACHED is column alignment — it
        // lines this measurement up under the UNCACHED line printed above, so
        // the two numbers sit in the same column and can be read against each
        // other. Rejoining it would break the table this test exists to print.
        "  CACHED    decompose {cached_pre_ms:.0} ms + verb {cached_mv_ms:.0} ms; \
a repeat lookup on unchanged content is {hit_ms:.1} ms"
    );
    println!(
        "\n  the verb went {:.0} ms -> {:.0} ms\n",
        mv_ms, cached_mv_ms
    );

    // ★ The assertion, not just the print. A benchmark that only prints can
    // regress silently; this fails if the cache stops paying. Deliberately
    // loose (half) rather than tight -- the point is "the verb no longer
    // parses", and a threshold tuned to this machine would fail on another.
    assert!(
        cached_mv_ms < mv_ms / 2.0,
        "the verb should not re-parse when the caller already decomposed: \
         uncached {mv_ms:.0} ms, cached {cached_mv_ms:.0} ms"
    );
}
