//! Fuzz target: the Type 3 font model (`pdfcer_render::type3::Type3Font`;
//! `Pass 126.0`, ISO 32000-1 §9.6.5, Tables 112 and 113).
//!
//! # Why this loader needs a target of its own
//!
//! `ARCHITECTURE.md` §10.2 asks for one on any new untrusted-input
//! parser. This one reads a **font dictionary**, which is the shape of
//! input a fuzzer is unusually good at and a hand-written test is
//! unusually bad at: every field is optional-in-practice, several are
//! arrays whose length the file chooses, and the interesting states are
//! combinations rather than values.
//!
//! Specifically:
//!
//! 1. **`/Widths` is a file-controlled array indexed by a file-controlled
//!    offset.** `/FirstChar` positions it in a 256-slot table, and both
//!    numbers come out of the document. A negative, fractional or
//!    enormous `/FirstChar` beside a long `/Widths` is the classic
//!    out-of-bounds shape.
//! 2. **`/Differences` is a little stack machine.** An integer sets the
//!    current code and each following name assigns and increments — so a
//!    code near 255 followed by many names walks off the end, and an
//!    array of names with no leading integer has no current code at all.
//! 3. **`/FontMatrix` is six numbers that become a transform.** Zeroes,
//!    infinities and NaNs all parse as PDF reals.
//! 4. **The glyph lookup crosses two dictionaries** — `/Encoding` gives a
//!    name, `/CharProcs` is asked for it — and both are file-controlled,
//!    so a name that is present in one and absent from the other is the
//!    ordinary case rather than the exotic one.
//!
//! # The invariants asserted
//!
//! * **No panic, on any dictionary.** The crate's panic-free policy
//!   (X5/X6). Every index in `load_widths` and `load_encoding` is inside
//!   this target's blast radius.
//! * **Every code in the one-byte space is answerable.** `proc_for` and
//!   `advance_text_space` are called for all 256 codes plus values
//!   outside the range, because a `u32` code arrives from the show path
//!   and nothing upstream promises it is a byte.
//! * **A finite advance.** A width that arrives as an infinity or a NaN
//!   and reaches §9.4.4's pen would move the text cursor to a
//!   non-finite position, and every later glyph on the line with it —
//!   which rasterises as *nothing at all*, silently. The parser is
//!   allowed to produce a garbage number here; it is not allowed to
//!   produce one that destroys the rest of the line without saying so.
//!
//! # What it deliberately does not do
//!
//! It does not **run** a glyph procedure. That is the ordinary
//! content-stream interpreter, already covered by
//! `content_and_filters`, reached through a transform this target has no
//! canvas to apply. The font *model* is where the file-controlled
//! lengths live, and lengths are what break.

#![no_main]

use libfuzzer_sys::fuzz_target;
use pdfcer_core::document::Document;
use pdfcer_core::graph::ObjectGraph;
use pdfcer_render::type3::Type3Font;

/// Wrap the fuzz bytes as the body of a Type 3 font dictionary.
///
/// Building a whole document around the input, rather than parsing the
/// input as a document, is what keeps the fuzzer's budget on the FONT.
/// A raw-bytes-as-PDF target would spend almost all of its time failing
/// to find a cross-reference table, which `load_document` already
/// covers.
fn wrap(body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(body.len() + 512);
    out.extend_from_slice(b"%PDF-1.4\n1 0 obj\n<< /Type /Catalog >>\nendobj\n2 0 obj\n<< ");
    out.extend_from_slice(body);
    out.extend_from_slice(b" >>\nendobj\ntrailer\n<< /Size 3 /Root 1 0 R >>\n%%EOF\n");
    out
}

fuzz_target!(|data: &[u8]| {
    // A dictionary body is text; bytes that cannot be a name or a number
    // waste the run without reaching anything interesting. The recovery
    // path (`Document::from_bytes` on a file with no xref) is exercised
    // either way, and is another target's job.
    if data.is_empty() || data.len() > 8192 {
        return;
    }
    let bytes = wrap(data);
    let Ok(doc) = Document::from_bytes(bytes) else {
        return;
    };
    let view = doc.view();
    let Some(obj) = view.value(pdfcer_core::object::ObjId::new(2, 0)) else {
        return;
    };
    let Some(dict) = view.resolve(obj).as_dict() else {
        return;
    };

    let Some(font) = Type3Font::load(&view, dict) else {
        // Table 112's irreducible entries are missing. A refusal is a
        // legitimate outcome for arbitrary bytes and is the common one.
        return;
    };

    // Every code the show path can hand over, including values outside
    // the one-byte space: `LoadedFont::codes` yields a `u32`, and a
    // future composite-Type-3 shape would widen it further.
    for code in [0u32, 1, 32, 64, 127, 128, 254, 255, 256, 65_535, u32::MAX] {
        let _ = font.proc_for(code);
        let adv = font.advance_text_space(code);
        assert!(
            adv.is_finite(),
            "a non-finite advance ({adv}) would move the text cursor off the \
             page and take every later glyph on the line with it, rendering \
             as nothing at all with no diagnostic"
        );
    }
    for code in 0u32..256 {
        let _ = font.proc_for(code);
        assert!(font.advance_text_space(code).is_finite());
    }

    // The bounding-box sentinel is a predicate other code branches on;
    // it must answer for any six numbers the file supplied.
    let _ = font.bbox_is_sentinel();
    // The encoding table is a fixed 256 entries whatever `/Differences`
    // said, because every consumer indexes it by a byte.
    assert_eq!(font.glyph_names.len(), 256);
    assert_eq!(font.widths.len(), 256);
});
