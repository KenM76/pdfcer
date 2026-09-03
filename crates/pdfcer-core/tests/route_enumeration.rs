//! # Every entry point that takes a `find` and a pin resolves it the same way
//! (`Pass 148.0`)
//!
//! **This file is a route enumeration with teeth, and it exists because two
//! consecutive attempts to fix one defect fixed one route each.**
//!
//! The defect: a `find` string and an optional `pinned_span` describe the same
//! target two ways, and an **empty** `find` with a pin means *"the whole
//! pinned operator"* (`Pass 145.0`). Any code path that takes both and uses
//! the caller's string directly gets **zero characters** — and a validator
//! given zero characters finds zero violations, so it answers **yes to
//! everything**.
//!
//! ## The three fixes, and what each one missed
//!
//! | Pass | route fixed | route left broken |
//! |---|---|---|
//! | `145.0` | `plan_format`, `plan_edit` | both preview queries |
//! | `147.0` | `preview_font_resources` | `preview_style_resolution` |
//! | `148.0` | `preview_style_resolution` | — |
//!
//! `147.0` was reported by a consuming project. `148.0` was found by the
//! librarian's rule-11 sweep asking *"what ELSE decides from the caller's
//! string?"* — **not** by the engineer, four hours earlier, editing the same
//! file. Enumerating routes from the *function being fixed* enumerates the
//! instance, not the class.
//!
//! `preview_style_resolution` was the worst of the three and the last found:
//! it returns a **routing decision**, not a list. On `format_family.pdf` it
//! answered `RealFaceResolves { real_font: "Times-Bold" }` — the one face that
//! cannot show the run — so a shell routing Bold from it would call
//! `set_font("Times-Bold")` and get a refusal. **No bold by either route**,
//! which is `Pass 144.0`'s defect reached through a different door.
//!
//! ## What this file asserts, and why it is a source scan
//!
//! Two things, and only the first is a behaviour test:
//!
//! 1. **Every entry point behaves identically** on the three cases — pinned
//!    empty, unpinned empty, explicit — checked by calling all of them.
//! 2. **`find_anchor` has no call site that fails to resolve.** That is a
//!    property of the *source*, not of any one behaviour, and it is the only
//!    assertion that can fail on a route **added tomorrow**. A behaviour test
//!    can only cover routes somebody remembered to list; this one covers the
//!    ones they did not.
//!
//! Test 2 is deliberately a grep over `format.rs`/`edit.rs` rather than a
//! clever type. A newtype forcing resolution at the boundary would be
//! stronger and is the right answer if this recurs — but it changes four
//! signatures to prevent a class that has now been closed, and the cheap
//! detector comes first.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::span::ByteSpan;
use pdfcer_core::text_edit::{
    EditOptions, EditRequest, FormatOptions, FormatRequest, StyleSynthesis, set_format,
};
use pdfcer_core::text_extract::{ExtractOptions, extract_page};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/textedit")
        .join(name)
}

fn doc() -> Document {
    Document::from_bytes(std::fs::read(fixture("format_family.pdf")).unwrap()).unwrap()
}

fn first_operator_span(d: &Document) -> ByteSpan {
    let pages = pdfcer_core::page_tree::pages(d).unwrap();
    let opts = ExtractOptions::default().with_provenance(true);
    extract_page(d, &pages[0], 0, &opts)
        .unwrap()
        .runs
        .iter()
        .flat_map(|r| r.glyphs.iter())
        .find_map(|g| g.provenance.as_ref().map(|p| p.operator_span))
        .unwrap()
}

// ---------------------------------------------------------------------------
// 1. Behaviour — all four routes agree
// ---------------------------------------------------------------------------

#[test]
fn a_pinned_empty_find_resolves_to_the_operator_on_every_route() {
    let d = doc();
    let span = first_operator_span(&d);
    let session = EditSession::new(doc());

    // Route 1 — the format planner.
    let r = set_format(
        &d,
        &FormatRequest::whole_operator(0, span).size(24.0),
        &FormatOptions::default(),
    )
    .expect("plan_format resolves");
    assert_eq!(r.report.size_change, Some((12.0, 24.0)));

    // Route 2 — the replace planner.
    let mut req = EditRequest::find_replace(0, "", "x");
    req.pinned_span = Some(span);
    assert!(
        pdfcer_core::text_edit::edit_text(&d, &req, &EditOptions::default()).is_ok(),
        "plan_edit resolves"
    );

    // Route 3 — the font pre-flight.
    let pre = session.preview_font_resources(0, "", Some(span)).unwrap();
    assert_eq!(pre.text, "hello world", "preview_font_resources resolves");

    // Route 4 — the style query. THE ONE THAT WAS LEFT, and the one whose
    // wrong answer is a routing decision rather than a list entry.
    let res = session
        .preview_style_resolution(0, "", Some(span), StyleSynthesis::Bold)
        .expect("preview_style_resolution resolves");
    let combined = format!("{:?}", res.combined);
    assert!(
        combined.contains("Calibri-Bold"),
        "it must name the face that can SHOW the run, not /F3 Times-Bold \
         which refuses it: {combined}"
    );
    assert!(
        !combined.contains("Times-Bold"),
        "and specifically not the refusing one: {combined}"
    );
}

#[test]
fn the_two_preview_queries_agree_with_an_explicit_find() {
    // The property, stated directly: a pinned empty `find` and the operator's
    // own text must be the same question. If a route ever answers them
    // differently, a shell's preview and its commit describe different things.
    let d = doc();
    let span = first_operator_span(&d);
    let s = EditSession::new(doc());

    assert_eq!(
        s.preview_font_resources(0, "", Some(span)).unwrap(),
        s.preview_font_resources(0, "hello world", None).unwrap()
    );
    assert_eq!(
        s.preview_style_resolution(0, "", Some(span), StyleSynthesis::Bold)
            .unwrap(),
        s.preview_style_resolution(0, "hello world", None, StyleSynthesis::Bold)
            .unwrap()
    );
    let _ = d;
}

#[test]
fn an_unpinned_empty_find_is_refused_on_every_route() {
    // Every string contains the empty string, so an unpinned empty `find`
    // silently names the page's FIRST show operator. All four routes refuse
    // it, with the same sentence, rather than answering about an operator the
    // caller never named.
    let d = doc();
    let s = EditSession::new(doc());

    let e = set_format(
        &d,
        &FormatRequest::new(0, "").size(24.0),
        &FormatOptions::default(),
    )
    .unwrap_err();
    assert!(
        e.to_string().contains("empty find text"),
        "plan_format: {e}"
    );

    let e = pdfcer_core::text_edit::edit_text(
        &d,
        &EditRequest::find_replace(0, "", "x"),
        &EditOptions::default(),
    )
    .unwrap_err();
    assert!(e.to_string().contains("empty find text"), "plan_edit: {e}");

    let e = s.preview_font_resources(0, "", None).unwrap_err();
    assert!(e.to_string().contains("empty find text"), "pre-flight: {e}");

    let e = s
        .preview_style_resolution(0, "", None, StyleSynthesis::Bold)
        .unwrap_err();
    assert!(e.to_string().contains("empty find text"), "style: {e}");
}

// ---------------------------------------------------------------------------
// 2. THE SOURCE SCAN — the only assertion that can fail on a route added later
// ---------------------------------------------------------------------------

/// Every function that calls `find_anchor` also calls `effective_find`.
///
/// **This is the assertion that covers routes nobody listed.** The three
/// behaviour tests above enumerate four entry points; a fifth added next month
/// would pass all of them by simply not being mentioned. This one fails.
///
/// # Why per-FUNCTION and not per-line-window
///
/// The first cut scanned a 30-line window after each `find_anchor` call and
/// **reported all four known-good sites as violations** — the resolution
/// legitimately sits 70+ lines later, after the font lookup and the error
/// paths. A window wide enough to admit them would have been wide enough to
/// admit the next function too, which is a gate that cannot fail.
///
/// So the unit is the **function body**: locate each top-level `fn` in the two
/// modules, and require that any body mentioning `find_anchor(` also mentions
/// `effective_find(`. That is exact, has no tuning parameter, and says what it
/// means — *this code path locates an anchor and never resolves the text*.
///
/// ★ Worth recording that the first cut was **loudly** wrong rather than
/// quietly green. A source scan written to be strict fails visibly when its
/// heuristic is bad; one written to be lenient passes forever and is
/// indistinguishable from a working gate.
///
/// If a future call site genuinely needs no resolution, widen the exemption
/// list below **by name, with a reason** — do not delete the test.
#[test]
fn every_function_that_locates_an_anchor_resolves_the_find() {
    /// Functions that legitimately call `find_anchor` without resolving.
    /// Empty today. An entry here is a claim and owes a reason beside it.
    const EXEMPT: &[&str] = &[];

    let src_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/text_edit");
    let mut callers: Vec<String> = Vec::new();
    let mut unresolved: Vec<String> = Vec::new();

    for name in ["format.rs", "edit.rs"] {
        let text = std::fs::read_to_string(src_dir.join(name)).expect("module readable");

        // Split on TOP-LEVEL `fn` declarations — column zero, so nested and
        // impl-block methods stay with their parent, which is what we want:
        // the question is "does this code path resolve", not "does this
        // lexical scope".
        let mut current = String::new();
        let mut current_name = String::from("<file top>");
        let mut blocks: Vec<(String, String)> = Vec::new();
        for line in text.lines() {
            let is_fn_decl = (line.starts_with("fn ")
                || line.starts_with("pub fn ")
                || line.starts_with("pub(crate) fn ")
                || line.starts_with("pub(super) fn "))
                && line.contains('(');
            if is_fn_decl {
                blocks.push((current_name.clone(), std::mem::take(&mut current)));
                current_name = line
                    .split("fn ")
                    .nth(1)
                    .and_then(|r| r.split(['(', '<']).next())
                    .unwrap_or("?")
                    .to_owned();
            }
            current.push_str(line);
            current.push('\n');
        }
        blocks.push((current_name, current));

        for (fname, body) in blocks {
            // The definition of `find_anchor` itself is not a caller of it.
            if fname == "find_anchor" || !body.contains("find_anchor(") {
                continue;
            }
            // A doc-comment mention is not a call.
            let calls = body.lines().any(|l| {
                let t = l.trim_start();
                l.contains("find_anchor(")
                    && !t.starts_with("//")
                    && !t.starts_with("///")
                    && !t.starts_with("*")
            });
            if !calls {
                continue;
            }
            callers.push(format!("{name}::{fname}"));
            if !body.contains("effective_find(") && !EXEMPT.contains(&fname.as_str()) {
                unresolved.push(format!("{name}::{fname}"));
            }
        }
    }

    assert!(
        callers.len() >= 4,
        "expected at least the four known callers, found {}: {callers:?} — has \
         `find_anchor` been renamed, or the `fn` layout changed? A source scan \
         goes VACUOUS rather than red when its anchor moves, so this check is \
         the one that keeps the test honest.",
        callers.len()
    );
    assert!(
        unresolved.is_empty(),
        "these functions locate an anchor and never resolve the find, so an \
         empty one reaches whatever they do with it — which is how three \
         separate Passes each fixed one route and left another:\n  {}",
        unresolved.join("\n  ")
    );
}
