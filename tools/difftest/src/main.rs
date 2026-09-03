//! # difftest — differential test oracle harness (pdfcer-core vs oxidize-pdf)
//!
//! Runs the same fixture PDFs through **both** parsers and reports
//! disagreements. Mandated by
//! `docs/decisions/001-oxidize-pdf-adopt-vs-build.md` §7, which requires
//! this harness to exist *before* pdfcer-core's from-scratch parser is
//! written, so every parser milestone lands with an external
//! cross-check already in place.
//!
//! ## Contract
//!
//! - **The oracle is advisory, never authoritative.** oxidize-pdf has
//!   documented silent-corruption fallbacks (its filter layer can return
//!   raw undecoded bytes as though decoded). A disagreement here means
//!   "open the PDF_Spec RAG and adjudicate", never "pdfcer is wrong".
//! - **Fixture sourcing** (docs/LEGAL.md §5): pdfcer-authored synthetic
//!   PDFs or rights-cleared corpus files only. Never files from
//!   oxidize-pdf's own repository.
//! - **Exit codes**: `0` = all files agree; `1` = at least one
//!   divergence (advisory — see above); `2` = usage error;
//!   `3` = a file could not be read at all.
//!
//! ## Current comparison surface (grows with pdfcer-core)
//!
//! | pdfcer-core milestone | comparison added |
//! |---|---|
//! | Pass 0 header probe (NOW) | declared `%PDF-` version vs oracle's |
//! | Pass 1 tokenizer/COS/xref | full object-graph diff: object numbers, types, dict keys, stream lengths |
//! | Pass 1 FlateDecode | decoded stream byte equality (adjudicating oracle fallbacks!) |
//! | later | page tree shape, content-stream token counts, … |
//!
//! The Pass 1 graph diff is the real payload; this file is deliberately
//! structured so it slots into [`compare_file`] without reshaping the
//! harness.

use std::path::Path;
use std::process::ExitCode;

use oxidize_pdf::parser::PdfReader;

/// One file's comparison outcome.
enum Outcome {
    /// Both parsers succeeded and every compared fact matched.
    Agree,
    /// Parsers produced conflicting facts (details already printed).
    Diverge,
    /// The file couldn't be read at the I/O level (not a parser
    /// disagreement — reported separately so corpus problems aren't
    /// mistaken for parser bugs).
    Unreadable,
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("usage: difftest <fixture.pdf> [more.pdf ...]");
        eprintln!("  Runs each file through pdfcer-core and the pinned oxidize-pdf");
        eprintln!("  oracle and reports disagreements (advisory — see module docs).");
        return ExitCode::from(2);
    }

    let mut diverged = false;
    let mut unreadable = false;
    for arg in &args {
        match compare_file(Path::new(arg)) {
            Outcome::Agree => println!("AGREE     {arg}"),
            Outcome::Diverge => {
                diverged = true;
                println!("DIVERGE   {arg}");
            }
            Outcome::Unreadable => {
                unreadable = true;
                println!("UNREADABLE {arg}");
            }
        }
    }

    if unreadable {
        ExitCode::from(3)
    } else if diverged {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

/// Compare a single file across both parsers, printing per-fact detail on
/// divergence. This is the function each pdfcer-core milestone extends
/// (see the module-docs table).
fn compare_file(path: &Path) -> Outcome {
    // ---- pdfcer side: full Document load (Pass 1 parser stack) ----
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("  io error: {e}");
            return Outcome::Unreadable;
        }
    };
    let pdfcer_doc = pdfcer_core::document::Document::from_bytes(bytes);
    let pdfcer_version = match &pdfcer_doc {
        Ok(doc) => {
            println!("  pdfcer: {} object(s) parsed", doc.object_count());
            Some(doc.version().to_string())
        }
        Err(e) => {
            // A load refusal is a legitimate parse verdict (including
            // the deliberate xref-stream/hybrid "not yet supported"
            // refusals) — compare it against the oracle's verdict.
            eprintln!("  pdfcer: load failed: {e}");
            None
        }
    };

    // ---- oracle side ----
    let oracle = PdfReader::open_document(path);
    let oracle_version = match &oracle {
        Ok(doc) => match doc.version() {
            Ok(v) => Some(v),
            Err(e) => {
                eprintln!("  oracle: opened but version() failed: {e}");
                None
            }
        },
        Err(e) => {
            eprintln!("  oracle: open failed: {e}");
            None
        }
    };

    // Extra oracle facts worth logging (not yet comparable — pdfcer has no
    // page tree until Pass 1; printed so fixture authors can eyeball them).
    if let Ok(doc) = &oracle {
        match doc.page_count() {
            Ok(n) => println!("  oracle: {n} page(s)"),
            Err(e) => println!("  oracle: page_count failed: {e}"),
        }
    }

    // ---- verdict ----
    match (pdfcer_version, oracle_version) {
        (Some(a), Some(b)) if a == b => Outcome::Agree,
        (None, None) => {
            // Both rejected the file. That is agreement on the only fact
            // currently compared ("is this a PDF and what does it declare").
            Outcome::Agree
        }
        (a, b) => {
            println!("  version: pdfcer={a:?} oracle={b:?}");
            Outcome::Diverge
        }
    }
}
