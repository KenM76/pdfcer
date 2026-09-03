//! # Does a `TextRun` span more than one show operator, and do the glyphs of
//! one operator slice a contiguous range out of the run's text? (`Pass 145.0`)
//!
//! **This file exists to answer a question a consuming project could not
//! answer and had already shipped against.** `pdfcer-gui` restyles a pinned
//! operator by rebuilding a `find` string from the glyphs whose
//! `GlyphProvenance::operator_span` equals the pin, and asked:
//!
//! > *"the glyphs sharing one `operator_span` always slice a contiguous,
//! > matchable range out of the run's text — we believe that is true today
//! > and we have no way to know whether it is guaranteed."*
//!
//! They were right that they could not know it. `operator_span` is not
//! emitted by `pdfcer extract-text --json`, so the property is unobservable
//! from outside `pdfcer-core`. Answering it is owed whichever way it comes out,
//! and the two outcomes are not symmetric: if it holds it is undocumented
//! load-bearing behaviour that the next refactor of `text_extract::layout`
//! could break silently; if it does not, **what they shipped is resting on
//! luck and they need telling promptly**, because it is in a build their
//! operator is using.
//!
//! ## One probe, two answers
//!
//! The same enumeration settles a second claim, recorded in `Pass 145.0`'s
//! entry as **the reporter's account, unverified by this project**: *"a
//! `TextRun` can span several show operators"*, offered as the cause of one
//! of their three failed attempts. `layout` closes a run on **geometry**; a
//! producer closes a show operator on whatever its writer felt like. Whether
//! those two ever disagree in practice is a count, not an argument.
//!
//! ## What is measured
//!
//! Over every PDF under `fixtures/synthetic/`, with provenance capture on:
//!
//! 1. **runs whose glyphs carry more than one distinct `operator_span`** —
//!    the multi-operator claim;
//! 2. **`operator_span` groups whose glyphs' text slices are NOT contiguous**
//!    — the invariant, in the strong form: the union of
//!    `[text_start, text_start + text_len)` over one group is a single
//!    unbroken range;
//! 3. **groups whose contiguous slice does not `find` inside the run's own
//!    `text`** — the "matchable" half, checked separately because
//!    contiguity does not imply it (a slice is trivially findable, but the
//!    check catches a `text_start`/`text_len` pair that indexes outside the
//!    string or lands off a `char` boundary).
//!
//! ## Why the counts are asserted rather than printed
//!
//! A probe that only prints is a probe nobody re-runs. The assertions below
//! are what turn "we measured it once" into "it is still true", which is the
//! whole difference between an answer and a documented guarantee. Each one
//! names what a future failure would mean.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use pdfcer_core::document::Document;
use pdfcer_core::span::ByteSpan;
use pdfcer_core::text_extract::{ExtractOptions, extract_page};

/// Everything one corpus walk found.
#[derive(Default, Debug)]
struct Findings {
    files: usize,
    files_with_text: usize,
    runs: usize,
    glyphs: usize,
    runs_without_provenance: usize,
    /// Runs whose glyphs carry more than one distinct `operator_span`.
    multi_operator_runs: Vec<String>,
    /// `operator_span` groups whose text slices are not one unbroken range.
    non_contiguous_groups: Vec<String>,
    /// Groups whose slice does not index the run's `text` cleanly.
    unmatchable_groups: Vec<String>,
    /// Groups examined, i.e. the denominator for the two counts above.
    groups: usize,
}

/// The corpus root.
///
/// `fixtures/` as a whole, not `fixtures/synthetic/`, so the answer is
/// measured over real third-party producer output wherever
/// `fixtures/fetch-corpora.sh` has been run — that is where a geometric run
/// boundary is most likely to disagree with a producer's operator boundary.
/// The external half is not committed, so on a bare checkout this walks the
/// synthetic corpus alone and the assertions still hold; the printed file
/// count says which of the two was measured, so nobody mistakes the smaller
/// run for the larger one.
fn corpus() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

fn pdfs_under(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            pdfs_under(&p, out);
        } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("pdf")) {
            out.push(p);
        }
    }
}

/// The corpus walk, run **once** per test binary.
///
/// Three tests ask three questions of the same enumeration, and the walk is
/// the expensive part (tens of thousands of pages). Caching it keeps the
/// binary's cost the cost of one walk rather than three, and — more usefully
/// — guarantees all three answers describe the *same* measurement.
fn findings() -> &'static Findings {
    static ONCE: std::sync::OnceLock<Findings> = std::sync::OnceLock::new();
    ONCE.get_or_init(probe)
}

/// Walk the corpus once, answering both questions.
fn probe() -> Findings {
    let mut files = Vec::new();
    pdfs_under(&corpus(), &mut files);
    files.sort();

    let mut f = Findings::default();
    for path in &files {
        f.files += 1;
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        // A fixture that does not parse is not evidence about spans; the
        // corpus deliberately contains damaged files for the recovery tests.
        let Ok(doc) = Document::from_bytes(bytes) else {
            continue;
        };
        let Ok(pages) = pdfcer_core::page_tree::pages(&doc) else {
            continue;
        };
        // Provenance is OFF by default and is the entire subject here, so a
        // probe that forgot it would measure an empty set and pass.
        let opts = ExtractOptions::default().with_provenance(true);
        let mut had_text = false;
        for (pi, pg) in pages.iter().enumerate() {
            let Ok(page) = extract_page(&doc, pg, pi, &opts) else {
                continue;
            };
            for (ri, run) in page.runs.iter().enumerate() {
                if run.glyphs.is_empty() {
                    continue;
                }
                had_text = true;
                f.runs += 1;
                f.glyphs += run.glyphs.len();

                // Group this run's glyphs by their operator span, preserving
                // first-seen order so a report names them the way a reader
                // would find them.
                let mut groups: Vec<(ByteSpan, Vec<usize>)> = Vec::new();
                let mut missing = false;
                for (gi, g) in run.glyphs.iter().enumerate() {
                    let Some(prov) = g.provenance.as_ref() else {
                        missing = true;
                        continue;
                    };
                    let span = prov.operator_span;
                    match groups
                        .iter_mut()
                        .find(|(s, _)| s.start == span.start && s.len == span.len)
                    {
                        Some((_, v)) => v.push(gi),
                        None => groups.push((span, vec![gi])),
                    }
                }
                if missing {
                    f.runs_without_provenance += 1;
                }
                if groups.len() > 1 {
                    f.multi_operator_runs.push(format!(
                        "{}:page{pi}:run{ri} — {} distinct operator_spans over {} glyphs",
                        path.display(),
                        groups.len(),
                        run.glyphs.len()
                    ));
                }

                for (span, gis) in &groups {
                    f.groups += 1;
                    // The union of this group's text slices.
                    let lo = gis
                        .iter()
                        .map(|&i| run.glyphs[i].text_start as usize)
                        .min()
                        .unwrap_or(0);
                    let hi = gis
                        .iter()
                        .map(|&i| {
                            run.glyphs[i].text_start as usize + run.glyphs[i].text_len as usize
                        })
                        .max()
                        .unwrap_or(0);

                    // Contiguity: every byte of [lo, hi) is covered by some
                    // glyph in THIS group. A gap means another operator's
                    // glyphs are interleaved inside the range, and a caller
                    // slicing `lo..hi` would restyle text belonging to a
                    // different operator.
                    let mut covered = vec![false; hi.saturating_sub(lo)];
                    for &i in gis {
                        let s = run.glyphs[i].text_start as usize;
                        let e = s + run.glyphs[i].text_len as usize;
                        for b in s..e {
                            if let Some(slot) = covered.get_mut(b.saturating_sub(lo)) {
                                *slot = true;
                            }
                        }
                    }
                    if covered.iter().any(|c| !c) {
                        f.non_contiguous_groups.push(format!(
                            "{}:page{pi}:run{ri} — operator_span {}..{} slices {lo}..{hi} with a gap",
                            path.display(),
                            span.start,
                            span.end()
                        ));
                    }

                    // Matchability: the slice must index the run's text on
                    // char boundaries and be findable in it.
                    match run.text.get(lo..hi) {
                        Some(slice) if run.text.contains(slice) => {}
                        _ => f.unmatchable_groups.push(format!(
                            "{}:page{ri}:run{ri} — slice {lo}..{hi} does not index text of {} bytes",
                            path.display(),
                            run.text.len()
                        )),
                    }
                }
            }
        }
        if had_text {
            f.files_with_text += 1;
        }
    }
    f
}

/// A short report, so a failure names the corpus it was measured over rather
/// than only the assertion that broke.
fn report(f: &Findings) -> String {
    format!(
        "corpus: {} file(s), {} with text; {} run(s), {} glyph(s), {} operator_span group(s); \
         {} run(s) with a glyph lacking provenance",
        f.files, f.files_with_text, f.runs, f.glyphs, f.groups, f.runs_without_provenance
    )
}

// ---------------------------------------------------------------------------

/// **The invariant `pdfcer-gui` shipped against.**
///
/// The glyphs sharing one `operator_span` slice a **contiguous** range out of
/// the run's text. A failure here means their workaround — and anything else
/// that reconstructs a `find` from an operator's glyphs — can silently
/// restyle text belonging to a neighbouring operator, and they must be told
/// on the channel the same day.
#[test]
fn glyphs_of_one_operator_slice_a_contiguous_range_of_the_run_text() {
    let f = findings();
    assert!(
        f.groups > 0,
        "the probe found no operator_span groups at all, so it measured nothing: {}",
        report(f)
    );
    assert!(
        f.non_contiguous_groups.is_empty(),
        "{}\nnon-contiguous groups:\n  {}",
        report(f),
        f.non_contiguous_groups.join("\n  ")
    );
}

/// The second half of the same invariant: the contiguous slice actually
/// indexes the run's `text` on `char` boundaries.
///
/// Checked separately from contiguity because the two can fail independently:
/// a `text_start`/`text_len` pair can be contiguous with its neighbours and
/// still land mid-`char` or past the end.
#[test]
fn that_contiguous_range_indexes_the_run_text_cleanly() {
    let f = findings();
    assert!(
        f.unmatchable_groups.is_empty(),
        "{}\nunmatchable groups:\n  {}",
        report(f),
        f.unmatchable_groups.join("\n  ")
    );
}

/// **The multi-operator claim, measured.**
///
/// `Pass 145.0` records *"a `TextRun` can span several show operators"* as the
/// reporter's account, explicitly unverified. This test is the verification,
/// and it is written to **report the count either way** rather than to assert
/// a preferred answer — because both answers are informative and neither is a
/// defect:
///
/// - **zero** ⇒ on this corpus `layout`'s geometric run boundaries never cut
///   across a producer's operator boundaries, so the mechanism offered for
///   their third failed attempt does not occur here. That does not make their
///   workaround wrong; it changes what they should be told about *why* it was
///   needed.
/// - **non-zero** ⇒ the claim is confirmed and the witnesses are named, which
///   is what makes `Pass 145.0`'s whole-operator affordance load-bearing
///   rather than a convenience: a `find` built from a whole run would then be
///   asking `match_run` to span operators, which it cannot express.
///
/// The assertion is only that the probe **ran** — the count is printed for
/// the record. Asserting a specific count here would freeze a property of the
/// fixture corpus rather than of pdfcer.
#[test]
fn how_many_runs_span_more_than_one_show_operator() {
    let f = findings();
    println!("{}", report(f));
    println!(
        "runs spanning more than one show operator: {}",
        f.multi_operator_runs.len()
    );
    for w in f.multi_operator_runs.iter().take(20) {
        println!("  {w}");
    }
    assert!(f.runs > 0, "the probe measured nothing: {}", report(f));
}
