//! Two-sided discharge harness for a `pdfcer-render` resource guard, against
//! a corpus directory.
//!
//! ```text
//! cargo run --release -p pdfcer-render --example guard_probe -- <dir> [more dirs]
//! ```
//!
//! # Why this exists
//!
//! `ROADMAP.md` carries a standing rule that **every new resource guard is
//! run against the veraPDF §6.1.12 implementation-limits suite before it
//! ships**, two-sidedly: it must be shown to FIRE on a real file, and to
//! stay SILENT across the whole suite. That rule has caught `MAX_TOKEN_LEN`
//! and `MAX_XOBJECT_DEPTH` before.
//!
//! `MAX_DISPLAY_LIST_BYTES` (`Pass 75.0`) shipped without the discharge, and
//! the completion report said so rather than implying otherwise. This is the
//! instrument that discharged it: 3,245 files, 0 firings.
//!
//! It also widened the rule. The "fire on a real file" half is
//! **undischargeable** for a ceiling no real input reaches, and a rule cannot
//! demand evidence nobody can produce. What that half exists to defeat is one
//! specific reading — *"0 refused" is indistinguishable from "the guard
//! cannot fire"* — and a **measured non-zero maximum** defeats it just as
//! well, by proving the accumulator counts real magnitudes. This harness
//! reports that maximum for exactly that reason; it is not a convenience
//! statistic.
//!
//! # What it reports, and why "silent" is the half that needs a corpus
//!
//! For every PDF it walks, it records page 1 and classifies the outcome:
//!
//! | outcome | meaning |
//! |---|---|
//! | `recorded` | a display list was produced — the guard did not fire |
//! | `TOO-LARGE` | the guard fired — **the half a false positive shows up in** |
//! | `refused` | the recorder refused for a capability reason (a shading, an overprint composite, a soft mask); nothing to do with the guard |
//! | `unloadable` | the file did not parse — a §6.1.12 suite is full of deliberately broken files, so this is expected and is NOT a failure |
//!
//! A guard's *firing* half can be shown with one constructed input, and
//! `display_list`'s unit tests already do exactly that against a small
//! `max_bytes`. Its *silence* half cannot: the only way to know a ceiling
//! does not trip on legitimate files is to run it over files somebody else
//! chose. That asymmetry is why this harness takes a corpus and not a
//! fixture.
//!
//! # Reading the result honestly
//!
//! A run where nothing is `TOO-LARGE` discharges the silent half **for the
//! files it walked, at the scale it used**, and nothing more. It is not
//! evidence that no document anywhere reaches 256 MiB. Say that when
//! reporting, rather than letting a clean run read as a stronger claim than
//! it is.
//!
//! # ★ THIS FILE'S OWN FIRST RUN FALSIFIED THIS FILE
//!
//! The paragraph above used to continue: *"the largest sheet this project
//! has measured holds ~29.5 MiB, which is 8.5× under the ceiling, so a suite
//! of small conformance files was never going to reach it."*
//!
//! **Both halves were wrong, and the harness disproved them the first time it
//! ran.** The largest list is not the CAD sheet's 29.5 MiB — it is
//! **41.9 MiB**, and it is produced by exactly one of those "small
//! conformance files":
//! `veraPDF test suite 6-1-12-t03-fail-c.pdf`. Real headroom is **6.1×**,
//! not 8.5×.
//!
//! It should not be a surprise in hindsight. A **§6.1.12
//! implementation-limits** file is *built* to stress this class of ceiling,
//! so the suite whose job is to find a resource guard found the biggest
//! consumer in the corpus. The sentence dismissed the one input most likely
//! to falsify it, on the strength of a document that merely happened to be
//! the largest one anybody had looked at.
//!
//! **The transferable rule, and the reason this correction is kept rather
//! than quietly edited out: derive headroom from the MEASURED MAXIMUM, never
//! from a reference document.** A reference document is chosen for being
//! representative; a ceiling is threatened by whatever is extreme, and those
//! are different files.
//!
//! (Recorded here for a second reason. The 8.5× claim was corrected in
//! `display_list.rs` and missed *here* — in a file written in the same
//! commit, by the same author, in the same hour. That is the third instance
//! in one day of correcting an instance instead of a class.)

use std::path::{Path, PathBuf};

use pdfcer_core::document::Document;
use pdfcer_core::page_tree;
use pdfcer_render::{RenderError, RenderOptions, record_page};

fn main() {
    let dirs: Vec<String> = std::env::args().skip(1).collect();
    if dirs.is_empty() {
        eprintln!("usage: guard_probe <dir> [dir...]");
        std::process::exit(2);
    }
    let mut files = Vec::new();
    for d in &dirs {
        collect(Path::new(d), &mut files);
    }
    files.sort();
    println!("walking {} file(s)", files.len());

    let options = RenderOptions::default();
    let (mut recorded, mut too_large, mut refused, mut unloadable) = (0, 0, 0, 0);
    let mut largest: (usize, String) = (0, String::new());

    for path in &files {
        let Ok(doc) = Document::load(path) else {
            unloadable += 1;
            continue;
        };
        let Ok(pages) = page_tree::pages(&doc) else {
            unloadable += 1;
            continue;
        };
        let Some(page) = pages.first() else {
            unloadable += 1;
            continue;
        };
        match record_page(&doc.view(), page, 1.0, 0, &options) {
            Ok(list) => {
                recorded += 1;
                let bytes = list.memory_bytes();
                if bytes > largest.0 {
                    largest = (bytes, path.display().to_string());
                }
            }
            Err(RenderError::PageNotRecordable { reason }) => {
                if reason == pdfcer_render::PoisonReason::TooLarge {
                    too_large += 1;
                    println!("  TOO-LARGE  {}", path.display());
                } else {
                    refused += 1;
                }
            }
            Err(_) => unloadable += 1,
        }
    }

    println!();
    println!("recorded    {recorded}");
    println!("TOO-LARGE   {too_large}   <- the guard firing; nonzero needs explaining");
    println!("refused     {refused}   (capability, not the guard)");
    println!("unloadable  {unloadable}   (expected in a deliberately-broken suite)");
    println!();
    println!(
        "largest list {} bytes ({:.2} MiB) - {}",
        largest.0,
        largest.0 as f64 / (1024.0 * 1024.0),
        largest.1
    );
    println!(
        "ceiling      {} bytes ({} MiB)",
        pdfcer_render::MAX_DISPLAY_LIST_BYTES,
        pdfcer_render::MAX_DISPLAY_LIST_BYTES / (1024 * 1024)
    );
    if largest.0 > 0 {
        println!(
            "headroom     {:.1}x   <- derive this from the MAXIMUM, never from a reference document",
            pdfcer_render::MAX_DISPLAY_LIST_BYTES as f64 / largest.0 as f64
        );
    }
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect(&p, out);
        } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("pdf")) {
            out.push(p);
        }
    }
}
