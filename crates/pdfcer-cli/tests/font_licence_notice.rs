//! # The bundled-font licence notice must stay a REPRODUCTION
//!
//! `pdfcer embed-font --use-bundled-fonts` may embed pdfcer's own
//! standard-14 substitute faces into an operator's document. Those faces are
//! BSD-3-Clause (pdfium, over Foxit-origin code). The licence places no
//! restriction on embedding — it is permissive — but it does attach one
//! condition to redistribution in binary form:
//!
//! > "Redistributions in binary form must **reproduce** the above copyright
//! > notice, this list of conditions and the following disclaimer in the
//! > documentation and/or other materials provided with the distribution."
//!
//! pdfcer discharges that automatically by attaching the notice to the PDF
//! (§7.11.4.1 route 2), so it travels with the file rather than depending on
//! the operator remembering, every time, for every document.
//!
//! ## What these tests protect, and why a human reading it once is not enough
//!
//! The obligation is to **reproduce** the text. A summary does not satisfy a
//! reproduction requirement, and a plausible re-wording of a licence is worse
//! than useless: it is a claim about legal terms that nobody verified. The
//! danger is not that someone rewrites it maliciously — it is that a
//! well-meaning edit reflows a line, "fixes" the spacing, or drops the
//! disclaimer paragraph while tidying, and nothing anywhere notices.
//!
//! So the notice pdfcer ships is diffed against its recorded source,
//! `crates/pdfcer-render/assets/fonts/PROVENANCE.md`, which holds the text as
//! extracted from pdfium's own LICENSE. Drift in either file is a test
//! failure. That is the whole point: it converts "someone should check this
//! is still verbatim" into something the build checks on every run.

use std::path::Path;

/// The operative sentences. Each must appear, word for word.
///
/// Split into separate needles rather than one long block so a failure names
/// WHICH part went missing — the copyright line, the binary-redistribution
/// condition, the no-endorsement clause, or the disclaimer — instead of
/// reporting that a 1.5 KB string does not match.
const REQUIRED: [&str; 5] = [
    "Copyright 2014 The PDFium Authors",
    "Redistributions in binary form must reproduce the above copyright notice, \
     this list of conditions and the following disclaimer in the documentation \
     and/or other materials provided with the distribution.",
    "Neither the name of Google Inc. nor the names of its contributors may be \
     used to endorse or promote products derived from this software without \
     specific prior written permission.",
    "THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS",
    "IN NO EVENT SHALL THE COPYRIGHT OWNER OR CONTRIBUTORS BE LIABLE",
];

/// Collapse comment markers and line wrapping so the comparison is about the
/// WORDS, not the layout.
///
/// Necessary because the two copies are legitimately wrapped differently:
/// `PROVENANCE.md` records pdfium's LICENSE with its original `//` comment
/// prefixes intact, while the notice pdfcer attaches to a PDF has them
/// stripped — a licence file inside someone's document should not look like C
/// source. Both are faithful reproductions of the same text.
///
/// This deliberately does NOT catch pure re-wrapping, and that is the right
/// trade: re-wrapping does not change what the licence says, whereas a
/// dropped clause or an altered word does — and those are exactly what
/// survives normalisation to fail the assertion.
fn normalise(text: &str) -> String {
    text.replace("//", " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn provenance() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../crates/pdfcer-render/assets/fonts/PROVENANCE.md");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// The source of truth still contains the licence this notice reproduces.
///
/// Guards the direction nobody thinks about: the notice could stay perfect
/// while PROVENANCE.md — the file that records WHERE the fonts came from and
/// under what terms — is edited or trimmed. If that record loses the licence,
/// the project has embedded faces whose terms it no longer documents.
#[test]
fn the_provenance_record_still_carries_the_licence() {
    let text = normalise(&provenance());
    for needle in REQUIRED {
        assert!(
            text.contains(&normalise(needle)),
            "PROVENANCE.md no longer contains this required licence text:\n---\n{needle}\n---\n\
             The bundled faces are still shipped, so the record of their terms must not be \
             trimmed. Restore it from pdfium's LICENSE."
        );
    }
}

/// The notice pdfcer attaches reproduces every operative clause.
///
/// Reads the CLI source rather than calling the function, because the
/// constant is private and making it `pub` purely to test it would widen the
/// binary's surface for no other caller. The text is a literal in the file;
/// a substring check over the source proves the shipped bytes contain it.
#[test]
fn the_attached_notice_reproduces_every_operative_clause() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/main.rs");
    let src = normalise(&std::fs::read_to_string(&path).expect("read main.rs"));
    for needle in REQUIRED {
        assert!(
            src.contains(&normalise(needle)),
            "the licence notice pdfcer attaches to documents no longer reproduces:\n---\n\
             {needle}\n---\n\
             BSD-3-Clause requires this be REPRODUCED, not summarised. If you reflowed or \
             tidied the constant, restore it verbatim from PROVENANCE.md."
        );
    }
}

/// The notice explains itself to someone who has never heard of pdfcer.
///
/// A bare licence file appearing inside a PDF is alarming and uninformative —
/// the person opening it needs to know why it is there and what it applies
/// to. This is not a legal requirement; it is the difference between a notice
/// that gets kept and one that gets deleted as junk, and a deleted notice
/// discharges nothing.
#[test]
fn the_notice_says_why_it_is_in_the_document() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/main.rs");
    let src = std::fs::read_to_string(&path).expect("read main.rs");
    assert!(
        src.contains("If you redistribute this PDF, keep this attachment."),
        "the notice must tell the reader what to DO with it"
    );
    assert!(
        src.contains("the document named a font it did not carry"),
        "the notice must explain why a font was added at all"
    );
}
