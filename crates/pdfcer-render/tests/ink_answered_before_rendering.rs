//! `page_composites_in_ink` answers before the render, and cannot disagree
//! with it (`Pass 296.4`).
//!
//! # What this closes
//!
//! The consuming shell needs to know whether a page will composite in ink in
//! order to decide what to do with it. The only route was to render the page
//! once and read `cmyk_buffer_engaged || cmyk_buffer_refused` afterwards —
//! sound, and a full render to ask a question the page's own dictionary
//! answers. `page_blend_space` had the answer and was `pub(crate)`.
//!
//! # ★★ The assertion that matters is the AGREEMENT
//!
//! Not "the accessor returns true on the CMYK fixture" — that would pass on a
//! second, independent implementation that had drifted. Each fixture is asked
//! **and** rendered, and the two answers are required to match. A pre-flight
//! that can disagree with the render is worse than no pre-flight, because a
//! caller acts on it.
//!
//! The two fixtures are identical but for the page group
//! (`fixtures/synthetic/ink-probe/PROVENANCE.md`), so nothing but the thing
//! under test differs between the rows.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};

use pdfcer_core::document::Document;
use pdfcer_core::page_tree;
use pdfcer_render::{BlendSpaceFrom, RenderOptions, page_composites_in_ink, render_page_with_view};

const SCALE: f32 = 2.0;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/ink-probe")
        .join(name)
}

#[test]
fn the_pre_flight_answer_matches_what_the_render_does() {
    for (name, want_ink) in [
        ("flat-cmyk-subtractive.pdf", true),
        ("flat-cmyk-additive.pdf", false),
    ] {
        let doc = Document::load(Path::new(&fixture(name))).expect("fixture loads");
        let pages = page_tree::pages(&doc).expect("page tree");
        let options = RenderOptions::default();

        let asked = page_composites_in_ink(&doc.view(), &pages[0], &options);
        assert_eq!(
            asked.composites_in_ink, want_ink,
            "{name}: the pre-flight answer is wrong before anything else is checked"
        );

        let rendered = render_page_with_view(&doc.view(), &pages[0], SCALE, &options)
            .expect("the fixture renders");

        // ★ The union, which is what "this page wanted ink" means: engaged is
        // the buffer running, refused is the buffer being declined on budget.
        // A page that wanted ink and was refused still WANTED it, and the
        // pre-flight is about the space, not the budget.
        let wanted_ink = rendered.diagnostics.cmyk_buffer_engaged
            || rendered.diagnostics.cmyk_buffer_refused > 0;
        assert_eq!(
            asked.composites_in_ink, wanted_ink,
            "{name}: the pre-flight and the render disagree — a caller acting on \
             the pre-flight would be acting on a different document than the one \
             that renders"
        );
    }
}

#[test]
fn the_answer_discloses_which_clause_supplied_it() {
    // Rule 4: a blending space is the extreme case of an invisible inference —
    // it changes every colour on the page and leaves no mark saying so. The
    // subtractive fixture DECLARES its group, so the provenance must say the
    // page group rather than a device default; the additive one does not, and
    // must not claim it did.
    let doc = Document::load(Path::new(&fixture("flat-cmyk-subtractive.pdf"))).expect("loads");
    let pages = page_tree::pages(&doc).expect("page tree");
    let declared = page_composites_in_ink(&doc.view(), &pages[0], &RenderOptions::default());
    assert_eq!(declared.source, BlendSpaceFrom::PageGroup);

    let doc2 = Document::load(Path::new(&fixture("flat-cmyk-additive.pdf"))).expect("loads");
    let pages2 = page_tree::pages(&doc2).expect("page tree");
    let other = page_composites_in_ink(&doc2.view(), &pages2[0], &RenderOptions::default());
    assert!(
        !other.composites_in_ink,
        "the additive fixture must not report ink"
    );
}
