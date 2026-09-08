//! # A symbolic TrueType takes Branch B, and Branch A would draw the WRONG glyph
//!
//! ## The defect
//!
//! ISO 32000-1 §9.6.6.4 splits simple-TrueType glyph selection in two, and the
//! split hinges on the descriptor's `Symbolic` flag:
//!
//! - **Branch A** (nonsymbolic, `/Encoding` present) — code → glyph **name**,
//!   then name → Unicode → `(3,1)` cmap; failing that, name → a **Mac OS Roman
//!   code** → `(1,0)` cmap; failing that, the `post` table.
//! - **Branch B** (`Symbolic` set, in which case *"the `Encoding` entry is
//!   ignored"*) — the raw code, straight into the program's own cmap.
//!
//! pdfcer ran Branch A first whenever a glyph name was present, symbolic or
//! not. For a symbolic subset that is not merely non-conformant, it is
//! **actively wrong in the worst available way**.
//!
//! ## Why it produces a wrong glyph rather than a missing one
//!
//! Branch A's second chain is entitled to assume that platform 1 / encoding 0
//! **means** Mac OS Roman, because that is what those numbers denote. A
//! subsetter emitting a symbolic font does not honour that: it writes a
//! *private* `(1,0)` table whose codes are 1, 2, 3 … in the order the glyphs
//! happened to be used.
//!
//! So the Mac-code lookup does not miss. It returns a perfectly valid GID for
//! an unrelated glyph, and the page paints confident nonsense.
//!
//! ★ Found 2026-09-08 on a real 2013 SolidWorks drawing. `59 3/4"` painted as
//! `@ U / M@`-shaped garbage — code 3 `/three` → Mac code 51 → GID 56 `U`,
//! code 4 `/four` → Mac 52 → GID 57 `V`, code 8 `/one` → Mac 49 → GID 48 `M`
//! — while **text extraction was perfectly correct**, because extraction reads
//! the `/Differences` names and those were right all along. 961 glyphs on one
//! page. A file that renders as garbage and copies as clean text is this bug's
//! signature, and the two halves disagreeing points at the glyph ladder rather
//! than at the encoding.
//!
//! ## ★★ Why the fixture is built the way it is
//!
//! The obvious fixture — a symbolic font with private codes and nothing else —
//! **does not catch this**. Under the defect, Branch A's chains would simply
//! all fail (no `(3,1)`, no matching Mac code, no `post` names), the ladder
//! would fall through to Branch B regardless, and the test would pass against
//! the very bug it was written for.
//!
//! So `symbolic-truetype-private-cmap.pdf` sets a trap. Its `(1,0)` cmap is
//! populated **twice**:
//!
//! | code | glyph | reached by |
//! |---|---|---|
//! | 1, 2, 3 | `boxLow1..3` | Branch B — the raw code. **Correct.** |
//! | 65, 66, 67 | `boxHigh1..3` | Branch A chain 2 — the Mac OS Roman codes for `A`, `B`, `C`. **Wrong.** |
//!
//! and the PDF says `/Differences [1 /A /B /C]`. Both outcomes paint real ink;
//! only *where* differs. `boxLow*` paint in the lower half of the em square and
//! `boxHigh*` in the upper half, so the assertion is **ink in one band and none
//! in the other** — no dependence on glyph identity, hinting, or antialiasing.
//!
//! The generator asserts the trap is set (`gids[private] != gids[mac]`, both
//! non-zero) by re-reading its own saved bytes, so a fixture that quietly lost
//! the collision fails loudly instead of silently testing nothing.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use pdfcer_core::document::Document;
use pdfcer_core::page_tree;
use pdfcer_render::{RenderOptions, RenderedPage, render_page_with};

fn fixture(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic")
        .join(rel)
}

fn render(rel: &str) -> RenderedPage {
    let doc = Document::load(&fixture(rel)).expect("fixture parses");
    let p = page_tree::pages(&doc).expect("page tree").remove(0);
    render_page_with(&doc, &p, 1.0, &RenderOptions::default()).expect("render")
}

/// Count inked pixels whose PDF-space `y` falls in `y0..y1`.
///
/// PDF user space is y-UP and the pixmap is y-DOWN, so the band is flipped
/// here rather than in the caller — getting that backwards would swap the two
/// assertions and turn this file into a test that passes on the defect.
fn ink_in_band(r: &RenderedPage, page_height: f32, y0: f32, y1: f32) -> u32 {
    let pm = &r.pixmap;
    let h = pm.height() as f32;
    let top = ((1.0 - y1 / page_height) * h).floor().max(0.0) as u32;
    let bottom = ((1.0 - y0 / page_height) * h).ceil().min(h) as u32;
    let mut n = 0;
    for y in top..bottom.min(pm.height()) {
        for x in 0..pm.width() {
            if pm.pixel(x, y).is_some_and(|p| p.demultiply().red() < 128) {
                n += 1;
            }
        }
    }
    n
}

const PAGE: f32 = 300.0;
// The text sits at 100pt on a baseline at y=100, so the glyph boxes land at
// roughly y 100..135 (low) and y 150..185 (high).
const LOW: (f32, f32) = (100.0, 135.0);
const HIGH: (f32, f32) = (150.0, 185.0);

/// ★★★ The regression: the symbolic font's own cmap owns the code.
///
/// Ink must land in the LOW band, which only Branch B can reach. Ink in the
/// HIGH band means Branch A's Mac-OS-Roman chain won, which is the shipped
/// defect exactly.
#[test]
fn a_symbolic_truetype_selects_glyphs_by_raw_code_not_by_glyph_name() {
    let r = render("text/symbolic-truetype-private-cmap.pdf");

    let low = ink_in_band(&r, PAGE, LOW.0, LOW.1);
    let high = ink_in_band(&r, PAGE, HIGH.0, HIGH.1);

    assert!(
        low > 0,
        "no ink where the correct glyphs live: Branch B did not run, or ran \
         and found nothing (low={low}, high={high})"
    );
    assert_eq!(
        high, 0,
        "ink in the WRONG band -- Branch A's Mac OS Roman chain resolved the \
         /Differences names against a PRIVATE (1,0) table and drew a valid \
         glyph that nobody asked for. This is the SolidWorks-drawing defect \
         (low={low}, high={high})"
    );
}

/// And nothing fell through to `.notdef`.
///
/// Separate from the band assertion on purpose. A build that resolved nothing
/// at all would paint no ink anywhere and satisfy `high == 0` — so without
/// this, half the assertion above is satisfiable by total failure.
#[test]
fn every_code_resolves_to_a_real_glyph() {
    let r = render("text/symbolic-truetype-private-cmap.pdf");
    assert_eq!(
        r.diagnostics.glyphs_notdef, 0,
        "codes 1..3 are all in the font's own (1,0) and (3,0) cmaps; a notdef \
         here means the raw-code lookup never happened"
    );
    assert_eq!(
        r.diagnostics.glyphs_substituted, 0,
        "the program is embedded, so no substitute face should be involved"
    );
}

/// Total inked pixels — used only to compare two renders of the same text.
fn ink_total(r: &RenderedPage) -> u32 {
    let pm = &r.pixmap;
    let mut n = 0;
    for y in 0..pm.height() {
        for x in 0..pm.width() {
            if pm.pixel(x, y).is_some_and(|p| p.demultiply().red() < 128) {
                n += 1;
            }
        }
    }
    n
}

/// ★★ The `embedded` half of the gate: with NO embedded program, a symbolic
/// font must still take the name chains.
///
/// The rule is *"symbolic **and embedded** → the program's own cmap first"*,
/// and the second half needs its own test or it is a guard nobody has seen
/// fail. It is load-bearing: with nothing embedded, the "program" is a
/// **substitute face**, whose built-in encoding has no relationship to this
/// document's codes. Codes 1–3 are C0 control positions in any normal face, so
/// a raw-code lookup finds nothing and the text disappears.
///
/// Asserted as an A/B against the *nonsymbolic* twin of the same file rather
/// than against an absolute pixel count, so the test needs no knowledge of
/// which substitute face was chosen or how it rasterises. Under the correct
/// rule both take the name chains and paint identically. Under a rule that
/// dropped the `embedded` half, only the nonsymbolic one does.
#[test]
fn a_non_embedded_symbolic_font_still_uses_its_differences() {
    let sym = render("text/symbolic-truetype-not-embedded.pdf");
    let non = render("text/nonsymbolic-truetype-not-embedded.pdf");

    let sym_ink = ink_total(&sym);
    let non_ink = ink_total(&non);

    assert!(
        non_ink > 0,
        "the nonsymbolic control painted nothing, so this comparison proves          nothing about the symbolic case (sym={sym_ink}, non={non_ink})"
    );
    assert_eq!(
        sym_ink, non_ink,
        "the symbolic and nonsymbolic twins of the same text must paint the          same glyphs when NEITHER embeds a program -- a difference means the          symbolic one went to the substitute face's own cmap with raw codes          1..3, which are control positions (sym={sym_ink}, non={non_ink})"
    );
    assert_eq!(
        sym.diagnostics.glyphs_notdef, 0,
        "codes 1..3 name /A /B /C through /Differences and a substitute face          has those glyphs; a notdef means the name chain never ran"
    );
}

/// ★★★ The MIRROR: the same font program, `/Flags 32`, and the correct answer
/// INVERTS.
///
/// This is what makes the `Symbolic` test *provable* rather than merely
/// plausible. The file above shows Branch B works; it cannot show that the
/// flag is what selected it. A rule that ignored the flag and always took
/// Branch B would satisfy that file completely and fail only here.
///
/// Nonsymbolic + `/Encoding` present is squarely Branch A, so the glyph name
/// wins: `/A` → Mac OS Roman 65 → the `(1,0)` entry at 65 → `boxHigh1`. The
/// **high** band, the exact opposite of its symbolic twin, from identical
/// font bytes and an identical content stream.
#[test]
fn a_nonsymbolic_twin_of_the_same_font_resolves_by_name_instead() {
    let r = render("text/nonsymbolic-truetype-private-cmap.pdf");

    let low = ink_in_band(&r, PAGE, LOW.0, LOW.1);
    let high = ink_in_band(&r, PAGE, HIGH.0, HIGH.1);

    assert!(
        high > 0,
        "a NONSYMBOLIC font's /Differences is authoritative -- the name chain          must reach the Mac-code glyphs (low={low}, high={high})"
    );
    assert_eq!(
        low, 0,
        "ink in the symbolic branch's band on a NONSYMBOLIC font: the raw-code          lookup is being applied where the glyph name should win, which would          break every nonsymbolic embedded font whose built-in cmap disagrees          with its /Differences (low={low}, high={high})"
    );
}
