//! # `tw-census` — the corpus measurement that gates a `Tw` authoring control
//!
//! Decision 019 (`docs/decisions/019-ffh-spacing-scaling-synthetic-styles.md`)
//! §3.3 declined to ship word spacing (`Tw`) as an operator-facing control
//! and made the question conditional on a census that had never been run.
//! ISO 32000-1 §9.3.3 confines `Tw` to the **single-byte** character code
//! 32, so it is structurally void on composite (multi-byte-code) fonts —
//! which means the control's ceiling is set by how much of real text is in
//! simple fonts. §3.3 fixed the decision bands in advance:
//!
//! | reachability | verdict |
//! |---|---|
//! | ≥ 60 % | build the control (R83-gated, simple-font-only, R91) |
//! | ≤ 25 % | close the item permanently; point the operator at reflow/justify |
//! | 25–60 % | escalate — a product judgement, not a technical one |
//!
//! **This tool measures. It does not decide.** It prints the numbers with
//! their denominators spelled out and lets the band speak.
//!
//! ## Usage
//!
//! ```text
//! cargo run --release -- [--tsv <path>] [--budget <secs>] <corpus-dir> [more-dirs ...]
//! ```
//!
//! Exit codes: `0` the sweep completed (whatever the numbers were), `2`
//! usage error, `3` a corpus directory could not be walked. A census has no
//! notion of "failing": an unopenable file is data about the corpus, not a
//! gate breach.
//!
//! ## Why a headline percentage on its own would be worse than no number
//!
//! The answer depends entirely on the denominator, and there are at least
//! three defensible ones that give materially different numbers:
//!
//! - **By document** — "would an operator opening a random file find the
//!   control usable anywhere in it?" Weights a one-line label the same as a
//!   400-page report.
//! - **By show operator** — decision 019 §3.3's own literal definition of
//!   reachability (a). Weights a `TJ` array of one glyph the same as one of
//!   four hundred.
//! - **By shown character code** — "of all the text in the corpus, how much
//!   sits in a run the control could touch?" This is the most
//!   decision-relevant of the three, because it is the one that scales with
//!   how much text an operator is actually looking at.
//!
//! All three are reported, always, side by side, and each is additionally
//! reported in a **strict** form that requires the run to contain an actual
//! code 32 — a simple-font run with no spaces in it is a run `Tw` cannot
//! affect either, and counting it as "works" inflates the headline.
//!
//! Four more categories are reported because they change how the headline
//! should be read, and folding any of them into it would mislead:
//!
//! - **Load failures** — a file pdfcer cannot open is not evidence about
//!   `Tw` in either direction. Excluded from every share.
//! - **Text-free documents** — scans and blank files belong in neither
//!   numerator nor denominator; silently scoring them as "no simple text"
//!   would understate `Tw` badly.
//! - **Mixed documents** — an operator editing one of these finds the
//!   control present on some selections and absent on others, which
//!   decision 019 §3.3 calls out as a worse experience than a clean yes or
//!   no. Counted as its own document class.
//! - **Invisible glyphs** (render mode 3/7, the OCR "sandwich") — real,
//!   extractable, editable text that paints nothing. Reported separately so
//!   the headline can be read with or without it.
//!
//! ## Architecture
//!
//! - [`census`] holds the pure classification logic and all of its unit
//!   tests. It never touches the filesystem.
//! - This file holds the corpus walk, the per-file isolation, the TSV, and
//!   the summary arithmetic.
//!
//! ### Isolation: one bad file must not take the run down
//!
//! Every file is measured on its own thread inside `catch_unwind`, with a
//! wall-clock budget. A panic becomes a reported outcome carrying its
//! message; a hang becomes `TimedOut` and the worker is abandoned. Same
//! pattern as `tools/roundtrip` and `tools/corpus-report`, and for the same
//! reason: a 4,000-file sweep that aborts on file 400 measures nothing —
//! and these corpora are *deliberately* full of files designed to break
//! parsers.
//!
//! ### Determinism
//!
//! Files are walked in sorted relative-path order, one at a time, and every
//! aggregate is an exact integer sum. The one `HashMap` involved
//! ([`census::PageCensus`]'s run pool) is only ever *summed over*, never
//! sampled or indexed — see that module's docs for why that is the safe
//! use of a non-deterministic iteration order, and for the prior-harness
//! bug that made the distinction worth writing down.
//!
//! ### Provenance bias — stated here so it travels with the numbers
//!
//! The corpus under `fixtures/external/` is assembled from the test suites
//! of PDF *tooling* projects (veraPDF's PDF/A conformance corpus, pdfium's
//! and PDFBox's regression suites, qpdf's qtest files, the PDF 2.0 example
//! set). Those are curated to be **pathological**: minimal hand-written
//! files exercising one clause each, deliberately malformed files, and
//! synthetic conformance probes. They are *not* a random sample of
//! documents an operator would open in a PDF editor, and a document-level
//! share taken over them is closer to "share of edge cases" than "share of
//! real work". The per-sub-corpus breakdown is therefore mandatory output,
//! not an option: a number from veraPDF's conformance suite means something
//! different from a number from the PDF 2.0 example set, and blending them
//! silently would launder the difference.

mod census;

use std::collections::BTreeMap;
use std::io::Write as _;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::mpsc;
use std::time::Duration;

use pdfcer_core::document::Document;
use pdfcer_core::page_tree;
use pdfcer_core::text_extract::{self, ExtractOptions};

use census::{DocCensus, FontMix, Outcome, PageCensus, Prevalence, Totals, share};

/// Default per-file wall-clock budget.
///
/// Extraction with provenance capture is cheaper than `roundtrip`'s three
/// saves plus two renders, but these corpora contain files engineered to
/// make a parser loop, so a budget is not optional. Overridable with
/// `--budget` so a slow machine can be given room without editing code.
const DEFAULT_BUDGET_SECS: u64 = 30;

/// Maximum characters kept of any detail string in the TSV.
const DETAIL_MAX: usize = 200;

/// One measured file: where it came from and what it yielded.
struct Row {
    /// Sub-corpus label — the corpus directory's own file name. This is the
    /// grouping key for the mandatory per-sub-corpus breakdown.
    corpus: String,
    /// Path relative to that corpus directory, forward-slashed.
    rel: String,
    outcome: Outcome,
}

/// Aggregated counts for one sub-corpus (or, with everything merged, for
/// the whole sweep).
///
/// Document-level counters and text-level counters live in the same struct
/// because every share this tool reports is a ratio of two of these fields,
/// and keeping them together makes the denominator of each share visible at
/// the point it is computed.
#[derive(Debug, Clone, Copy, Default)]
struct Agg {
    /// Files considered.
    files: u64,
    /// Files that could not be read from disk.
    read_failed: u64,
    /// Files `Document::from_bytes` refused.
    load_failed: u64,
    /// Files that loaded but whose page tree could not be walked.
    pagetree_failed: u64,
    /// Files that panicked pdfcer-core.
    panicked: u64,
    /// Files that exceeded the wall-clock budget.
    timed_out: u64,
    /// Files that loaded and were walked (`no_text` + `text_bearing`).
    measured: u64,
    /// Of those, files with zero show operators — scans and blanks.
    no_text: u64,
    /// Of those, files with at least one show operator. **This is the
    /// denominator of every document-level share below.**
    text_bearing: u64,
    /// Text-bearing files whose runs are all simple-font.
    all_simple: u64,
    /// Text-bearing files whose runs are all composite.
    all_composite: u64,
    /// Text-bearing files carrying both kinds.
    mixed: u64,
    /// Text-bearing files with at least one simple-font run (=
    /// `all_simple + mixed`), kept explicitly so the summary does not have
    /// to re-derive a headline number by addition.
    any_simple: u64,
    /// Text-bearing files with at least one simple-font run that actually
    /// contains a code 32 — the strict document-level measure.
    any_simple_spaced: u64,
    /// Pages walked, and pages skipped after an extraction error.
    pages: u64,
    page_errors: u64,
    /// Glyphs that arrived with no provenance despite capture being on.
    unprovenanced: u64,
    /// Show-operator and glyph counters, summed over every measured file.
    totals: Totals,
    /// Documents in which each text-state parameter was ever observed set /
    /// ever seen holding a non-default value. Decision 019 §3.3's second
    /// number, "(b) prevalence".
    prev_observed: [u64; 6],
    prev_nondefault: [u64; 6],
}

/// Names of the six text-state parameters, in the order used by
/// [`Agg::prev_observed`] / [`Agg::prev_nondefault`].
const PARAM_NAMES: [&str; 6] = ["Tc", "Tw", "Tz", "TL", "Ts", "Tr"];

impl Agg {
    /// Fold one file's outcome in.
    fn add(&mut self, outcome: &Outcome) {
        self.files += 1;
        match outcome {
            Outcome::ReadFailed(_) => self.read_failed += 1,
            Outcome::LoadFailed(_) => self.load_failed += 1,
            Outcome::PageTreeFailed(_) => self.pagetree_failed += 1,
            Outcome::Panicked(_) => self.panicked += 1,
            Outcome::TimedOut => self.timed_out += 1,
            Outcome::Measured(c) => self.add_measured(c),
        }
    }

    fn add_measured(&mut self, c: &DocCensus) {
        self.measured += 1;
        self.pages += c.pages;
        self.page_errors += c.page_errors;
        self.unprovenanced += c.unprovenanced_glyphs;
        self.totals.merge(c.totals);

        if c.totals.runs == 0 {
            self.no_text += 1;
            // A text-free document contributes no prevalence evidence
            // either — there was no shown glyph at which to observe an
            // ambient state.
            return;
        }
        self.text_bearing += 1;
        match c.totals.mix() {
            FontMix::AllSimple => self.all_simple += 1,
            FontMix::AllComposite => self.all_composite += 1,
            FontMix::Mixed => self.mixed += 1,
            FontMix::NoText => unreachable!("runs > 0 was just checked"),
        }
        if c.totals.runs_simple > 0 {
            self.any_simple += 1;
        }
        if c.totals.runs_simple_spaced > 0 {
            self.any_simple_spaced += 1;
        }

        for (i, flags) in prevalence_slots(&c.prevalence).into_iter().enumerate() {
            if flags.observed {
                self.prev_observed[i] += 1;
            }
            if flags.nondefault {
                self.prev_nondefault[i] += 1;
            }
        }
    }

    /// Sum two aggregates, so sub-corpora roll into the grand total by the
    /// same arithmetic the reader could redo from the TSV.
    fn merge(&mut self, o: &Self) {
        self.files += o.files;
        self.read_failed += o.read_failed;
        self.load_failed += o.load_failed;
        self.pagetree_failed += o.pagetree_failed;
        self.panicked += o.panicked;
        self.timed_out += o.timed_out;
        self.measured += o.measured;
        self.no_text += o.no_text;
        self.text_bearing += o.text_bearing;
        self.all_simple += o.all_simple;
        self.all_composite += o.all_composite;
        self.mixed += o.mixed;
        self.any_simple += o.any_simple;
        self.any_simple_spaced += o.any_simple_spaced;
        self.pages += o.pages;
        self.page_errors += o.page_errors;
        self.unprovenanced += o.unprovenanced;
        self.totals.merge(o.totals);
        for i in 0..6 {
            self.prev_observed[i] += o.prev_observed[i];
            self.prev_nondefault[i] += o.prev_nondefault[i];
        }
    }
}

/// The six prevalence flag slots in `PARAM_NAMES` order.
fn prevalence_slots(p: &Prevalence) -> [census::ParamFlags; 6] {
    [
        p.char_spacing,
        p.word_spacing,
        p.h_scale,
        p.leading,
        p.rise,
        p.render_mode,
    ]
}

fn main() -> ExitCode {
    let mut dirs: Vec<PathBuf> = Vec::new();
    let mut tsv_path = PathBuf::from("tw-census.tsv");
    let mut budget = Duration::from_secs(DEFAULT_BUDGET_SECS);

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--tsv" => match args.next() {
                Some(p) => tsv_path = PathBuf::from(p),
                None => return usage("--tsv needs a path"),
            },
            "--budget" => match args.next().and_then(|s| s.parse::<u64>().ok()) {
                Some(s) if s > 0 => budget = Duration::from_secs(s),
                _ => return usage("--budget needs a positive number of seconds"),
            },
            "-h" | "--help" => {
                eprintln!(
                    "tw-census [--tsv <path>] [--budget <secs>] <corpus-dir> [more-dirs ...]"
                );
                return ExitCode::SUCCESS;
            }
            other if other.starts_with('-') => return usage(&format!("unknown flag {other}")),
            other => dirs.push(PathBuf::from(other)),
        }
    }
    if dirs.is_empty() {
        return usage("at least one corpus directory is required");
    }

    let mut rows: Vec<Row> = Vec::new();
    for dir in &dirs {
        let corpus = dir
            .file_name()
            .map_or_else(|| "corpus".to_owned(), |n| n.to_string_lossy().into_owned());
        let files = match collect_pdfs(dir) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("error: {e}");
                return ExitCode::from(3);
            }
        };
        let total = files.len();
        eprintln!("[{corpus}] {total} PDF file(s)");
        for (i, (rel, abs)) in files.into_iter().enumerate() {
            if i % 200 == 0 {
                eprintln!("[{corpus}] {i}/{total} ...");
            }
            let outcome = measure_file(&abs, budget);
            rows.push(Row {
                corpus: corpus.clone(),
                rel,
                outcome,
            });
        }
    }

    if let Err(e) = write_tsv(&tsv_path, &rows) {
        eprintln!("error: {e}");
        return ExitCode::from(3);
    }
    print_summary(&rows, budget);
    ExitCode::SUCCESS
}

fn usage(msg: &str) -> ExitCode {
    eprintln!("usage error: {msg}");
    eprintln!("tw-census [--tsv <path>] [--budget <secs>] <corpus-dir> [more-dirs ...]");
    ExitCode::from(2)
}

/// Recursively collect every `*.pdf` under `root`, skipping
/// dot-directories, sorted by relative path.
///
/// The sort is what makes a partial or repeated run comparable: two
/// invocations visit the same files in the same order, so a TSV diff shows
/// real changes rather than directory-order noise.
fn collect_pdfs(root: &Path) -> Result<Vec<(String, PathBuf)>, String> {
    let mut out = Vec::new();
    let mut stack = vec![(root.to_path_buf(), String::new())];
    while let Some((dir, prefix)) = stack.pop() {
        let entries =
            std::fs::read_dir(&dir).map_err(|e| format!("read_dir {}: {e}", dir.display()))?;
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            let path = entry.path();
            let rel = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}/{name}")
            };
            match entry.file_type() {
                Ok(t) if t.is_dir() => stack.push((path, rel)),
                Ok(t) if t.is_file() && name.to_ascii_lowercase().ends_with(".pdf") => {
                    out.push((rel, path));
                }
                _ => {}
            }
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

/// Measure one file with panic and timeout isolation.
fn measure_file(path: &Path, budget: Duration) -> Outcome {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => return Outcome::ReadFailed(e.to_string()),
    };
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let result = match catch_unwind(AssertUnwindSafe(|| measure_bytes(bytes))) {
            Ok(o) => o,
            Err(payload) => {
                let msg = payload
                    .downcast_ref::<&str>()
                    .map(|s| (*s).to_owned())
                    .or_else(|| payload.downcast_ref::<String>().cloned())
                    .unwrap_or_else(|| "<non-string panic payload>".to_owned());
                Outcome::Panicked(msg)
            }
        };
        // A late result after a timeout is deliberately dropped.
        let _ = tx.send(result);
    });
    rx.recv_timeout(budget).unwrap_or(Outcome::TimedOut)
}

/// The measurement itself: load, walk every page, pool by show operator.
///
/// Pages are extracted one at a time and their `PageText` dropped
/// immediately after pooling, so peak memory is one page's glyphs rather
/// than a whole document's. That matters here because provenance capture
/// allocates per glyph and some corpus files are thousands of pages.
///
/// A page whose extraction errors is **counted and skipped**, not fatal: a
/// document with one unparseable page still has measurable text on the
/// others, and discarding it would bias the census toward clean files.
pub fn measure_bytes(bytes: Vec<u8>) -> Outcome {
    let doc = match Document::from_bytes(bytes) {
        Ok(d) => d,
        Err(e) => return Outcome::LoadFailed(e.to_string()),
    };
    let pages = match page_tree::pages(&doc) {
        Ok(p) => p,
        Err(e) => return Outcome::PageTreeFailed(e.to_string()),
    };

    // Provenance capture is the whole point: `composite` and the ambient
    // text state are published on `GlyphProvenance` (Pass 19.0) and are
    // `None` without this flag.
    // `ExtractOptions` is `#[non_exhaustive]`, so it is built by mutating
    // the default rather than by a struct expression — which is the point
    // of the attribute: a future field arrives with pdfcer's chosen default
    // instead of breaking this harness or, worse, silently changing what it
    // measures.
    let mut options = ExtractOptions::default();
    options.capture_provenance = true;
    // Artifact runs (running heads, folios, watermarks) are real text an
    // operator can select and edit, so they belong in the census. They are
    // present in `PageText::runs` regardless of this flag — it only gates
    // `plain_text` — but setting it makes the intent explicit rather than
    // incidental.
    options.include_artifacts = true;

    let mut pool = PageCensus::default();
    let mut walked = 0u64;
    let mut errors = 0u64;
    for (index, page) in pages.iter().enumerate() {
        match text_extract::extract_page(&doc, page, index, &options) {
            Ok(text) => {
                pool.add_page(&text);
                walked += 1;
            }
            Err(_) => errors += 1,
        }
    }

    let (totals, prevalence, unprovenanced) = pool.finish();
    Outcome::Measured(DocCensus {
        totals,
        prevalence,
        pages: walked,
        page_errors: errors,
        unprovenanced_glyphs: unprovenanced,
    })
}

/// Strip control characters and cap length, so one malformed error string
/// cannot corrupt the TSV's row/column structure.
fn sanitize(s: &str) -> String {
    let mut out: String = s
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    if out.chars().count() > DETAIL_MAX {
        out = out.chars().take(DETAIL_MAX).collect::<String>() + "...";
    }
    out
}

/// The TSV's column names, in order.
///
/// The header and both row shapes are generated from this one list, and
/// [`write_tsv`] asserts every row's field count against its length. That
/// is not ceremony: the first draft of this function spelled the header as
/// one string literal and the failure-row shape as a run of literal tabs,
/// and the two disagreed by exactly one tab — so every non-measured row was
/// silently 29 fields against a 30-field header, which puts the `detail`
/// text in the `Ts_nd` column for any tool that reads by index. The
/// evidence file for a decision has to be re-derivable by someone who was
/// not here, so its shape cannot depend on counting tabs correctly.
const COLUMNS: [&str; 30] = [
    "corpus",
    "file",
    "outcome",
    "mix",
    "pages",
    "page_errors",
    "runs",
    "runs_simple",
    "runs_simple_spaced",
    "glyphs",
    "glyphs_simple",
    "glyphs_simple_spaced",
    "glyphs_invisible",
    "spaces_simple",
    "spaces_composite",
    "font_conflicts",
    "unprovenanced",
    "Tc_set",
    "Tw_set",
    "Tz_set",
    "TL_set",
    "Ts_set",
    "Tr_set",
    "Tc_nd",
    "Tw_nd",
    "Tz_nd",
    "TL_nd",
    "Ts_nd",
    "Tr_nd",
    "detail",
];

/// Build one file's TSV fields, in [`COLUMNS`] order.
///
/// Non-measured files get **empty** count columns rather than zeros: a
/// zero is a measurement, and a file that would not load is not one.
fn row_fields(row: &Row) -> Vec<String> {
    let mut f: Vec<String> = vec![
        row.corpus.clone(),
        row.rel.clone(),
        row.outcome.label().to_owned(),
    ];
    match &row.outcome {
        Outcome::Measured(c) => {
            let t = &c.totals;
            f.push(t.mix().label().to_owned());
            for n in [
                c.pages,
                c.page_errors,
                t.runs,
                t.runs_simple,
                t.runs_simple_spaced,
                t.glyphs,
                t.glyphs_simple,
                t.glyphs_simple_spaced,
                t.glyphs_invisible,
                t.space_codes_simple,
                t.space_codes_composite,
                t.font_conflicts,
                c.unprovenanced_glyphs,
            ] {
                f.push(n.to_string());
            }
            let slots = prevalence_slots(&c.prevalence);
            for flags in slots {
                f.push(u8::from(flags.observed).to_string());
            }
            for flags in slots {
                f.push(u8::from(flags.nondefault).to_string());
            }
            f.push(String::new()); // detail
        }
        _ => {
            // Every column between `outcome` and `detail` is blank.
            f.resize(COLUMNS.len() - 1, String::new());
            f.push(sanitize(row.outcome.detail()));
        }
    }
    f
}

/// Write one row per file, including the uninteresting ones.
///
/// Every aggregate printed by [`print_summary`] is a plain sum of these
/// columns, which is the point: the headline can be re-derived — or
/// challenged — without re-running the sweep over 4,000 files.
fn write_tsv(path: &Path, rows: &[Row]) -> Result<(), String> {
    let mut w = std::io::BufWriter::new(
        std::fs::File::create(path).map_err(|e| format!("create {}: {e}", path.display()))?,
    );
    writeln!(w, "{}", COLUMNS.join("\t")).map_err(|e| e.to_string())?;
    for row in rows {
        let fields = row_fields(row);
        assert_eq!(
            fields.len(),
            COLUMNS.len(),
            "TSV row shape drifted from COLUMNS for {}",
            row.rel
        );
        writeln!(w, "{}", fields.join("\t")).map_err(|e| e.to_string())?;
    }
    eprintln!("TSV written: {}", path.display());
    Ok(())
}

/// Format a share as a percentage, or `n/a` when the denominator is zero.
fn pct(numerator: u64, denominator: u64) -> String {
    share(numerator, denominator).map_or_else(|| "  n/a".to_owned(), |p| format!("{p:5.1}%"))
}

/// Print the aggregate report: overall, then one block per sub-corpus.
fn print_summary(rows: &[Row], budget: Duration) {
    let mut by_corpus: BTreeMap<&str, Agg> = BTreeMap::new();
    for row in rows {
        by_corpus
            .entry(row.corpus.as_str())
            .or_default()
            .add(&row.outcome);
    }
    let mut overall = Agg::default();
    for agg in by_corpus.values() {
        overall.merge(agg);
    }

    println!("================================================================");
    println!("tw-census — decision 019 §3.3 reachability + prevalence");
    println!("per-file budget: {}s", budget.as_secs());
    println!("================================================================");

    print_block("ALL SUB-CORPORA COMBINED", &overall);
    for (name, agg) in &by_corpus {
        print_block(name, agg);
    }

    println!();
    println!("---- decision band (decision 019 §3.3) -------------------------");
    println!("  >= 60%  build the Tw control      <= 25%  close the item");
    println!("  25-60%  escalate — product call, not a technical one");
    println!();
    println!("  The band is stated against ONE number in the decision:");
    println!("  reachability (a), 'of all show operators ... the fraction");
    println!("  whose font is simple'. That is the by-run share below.");
    println!("  The by-glyph share is reported alongside it because it is");
    println!("  the more decision-relevant weighting, and the STRICT forms");
    println!("  are reported because a simple-font run with no code 32 is a");
    println!("  run Tw cannot affect either.");
    println!("----------------------------------------------------------------");
}

/// One aggregate's full report block.
fn print_block(name: &str, a: &Agg) {
    println!();
    println!("### {name}");
    println!("  files scanned .................. {}", a.files);
    println!(
        "    load failures ................ {} ({} read, {} parse, {} page-tree, {} panic, {} timeout)",
        a.read_failed + a.load_failed + a.pagetree_failed + a.panicked + a.timed_out,
        a.read_failed,
        a.load_failed,
        a.pagetree_failed,
        a.panicked,
        a.timed_out
    );
    println!("    loaded + walked .............. {}", a.measured);
    println!(
        "      text-free (scan/blank) ..... {}  <- in NEITHER numerator nor denominator",
        a.no_text
    );
    println!(
        "      text-bearing ............... {}  <- the document-level DENOMINATOR",
        a.text_bearing
    );
    println!(
        "  pages walked {} (+{} pages skipped after an extraction error)",
        a.pages, a.page_errors
    );
    if a.unprovenanced > 0 {
        println!(
            "  !! {} glyph(s) arrived with NO provenance and are excluded from every count",
            a.unprovenanced
        );
    }
    if a.totals.font_conflicts > 0 {
        println!(
            "  !! {} show operator(s) had glyphs disagreeing about the composite flag",
            a.totals.font_conflicts
        );
    }

    let t = &a.totals;
    println!();
    println!("  (a) REACHABILITY — three denominators, loose and strict");
    println!(" denominator loose (simple font) strict (+ has code 32)");
    println!(
        "      by document  (n={:>6})           {}              {}",
        a.text_bearing,
        pct(a.any_simple, a.text_bearing),
        pct(a.any_simple_spaced, a.text_bearing)
    );
    println!(
        "      by show op   (n={:>6})           {}              {}",
        t.runs,
        pct(t.runs_simple, t.runs),
        pct(t.runs_simple_spaced, t.runs)
    );
    println!(
        "      by glyph     (n={:>6})           {}              {}",
        t.glyphs,
        pct(t.glyphs_simple, t.glyphs),
        pct(t.glyphs_simple_spaced, t.glyphs)
    );

    println!();
    println!("  document font mix (of {} text-bearing)", a.text_bearing);
    println!(
        " all-simple {:>6} ({}) all-composite {:>6} ({}) MIXED {:>6} ({})",
        a.all_simple,
        pct(a.all_simple, a.text_bearing),
        a.all_composite,
        pct(a.all_composite, a.text_bearing),
        a.mixed,
        pct(a.mixed, a.text_bearing)
    );

    println!();
    println!("  supplementary");
    println!(
        "      invisible glyphs (Tr 3/7, OCR layer) .. {} ({} of all glyphs)",
        t.glyphs_invisible,
        pct(t.glyphs_invisible, t.glyphs)
    );
    println!(
        "      code-32 occurrences in simple runs .... {}  (positions a Tw operand would move)",
        t.space_codes_simple
    );
    println!(
        "      code-32 occurrences in composite runs . {}  (§9.3.3 exempts these)",
        t.space_codes_composite
    );

    println!();
    println!(
        "  (b) PREVALENCE — documents setting each text-state parameter (of {} text-bearing)",
        a.text_bearing
    );
    print!("      set by an operator :");
    for (i, name) in PARAM_NAMES.iter().enumerate() {
        print!(" {name} {}", pct(a.prev_observed[i], a.text_bearing));
    }
    println!();
    print!("      held NON-default   :");
    for (i, name) in PARAM_NAMES.iter().enumerate() {
        print!(" {name} {}", pct(a.prev_nondefault[i], a.text_bearing));
    }
    println!();
}

/// # Ground-truth calibration against known fixtures
///
/// The corpus number is only worth as much as the classifier that produced
/// it, so the classifier is pinned against two synthetic fixtures whose
/// font model is known by construction (`fixtures/synthetic/text/`, built
/// by `tools/gen-text-fixtures.py`, provenance recorded in that directory's
/// `PROVENANCE.md`):
///
/// - `simple-winansi.pdf` — a Type1 simple font with WinAnsiEncoding: one
///   byte per code, so `Tw` reaches it.
/// - `identity-h-no-tounicode.pdf` — a Type0/Identity-H composite: two
///   bytes per code, so §9.3.3 makes `Tw` void.
///
/// If the tool cannot separate those two, the 4,000-file number is
/// meaningless — which is exactly why these are tests rather than a manual
/// spot-check performed once and forgotten.
#[cfg(test)]
mod fixture_tests {
    use super::*;
    use census::RunClass;

    /// Path to a repo fixture, relative to this package's manifest dir.
    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/synthetic/text")
            .join(name)
    }

    fn measure(name: &str) -> DocCensus {
        let bytes =
            std::fs::read(fixture(name)).unwrap_or_else(|e| panic!("read fixture {name}: {e}"));
        match measure_bytes(bytes) {
            Outcome::Measured(c) => c,
            other => panic!("fixture {name} did not measure: {}", other.label()),
        }
    }

    #[test]
    fn simple_winansi_fixture_is_classified_simple_and_reachable() {
        let c = measure("simple-winansi.pdf");
        assert!(c.totals.runs > 0, "fixture must have show operators");
        assert_eq!(
            c.totals.runs_simple, c.totals.runs,
            "every run of a WinAnsi Type1 fixture must be simple"
        );
        assert_eq!(c.totals.mix(), FontMix::AllSimple);
        // The fixture's text contains spaces, so it is reachable in the
        // STRICT sense too — this is what makes it a calibration of the
        // code-32 half of the classifier and not just the font half.
        assert!(
            c.totals.space_codes_simple > 0,
            "expected literal code-32 bytes in the simple show strings"
        );
        assert!(c.totals.runs_simple_spaced > 0);
        assert_eq!(c.totals.space_codes_composite, 0);
    }

    #[test]
    fn identity_h_fixture_is_classified_composite_and_unreachable() {
        let c = measure("identity-h-no-tounicode.pdf");
        assert!(c.totals.runs > 0, "fixture must have show operators");
        assert_eq!(
            c.totals.runs_simple, 0,
            "an Identity-H Type0 fixture must yield no simple runs"
        );
        assert_eq!(c.totals.mix(), FontMix::AllComposite);
        assert_eq!(c.totals.glyphs_simple, 0);
        assert_eq!(c.totals.runs_simple_spaced, 0);
    }

    #[test]
    fn every_measured_glyph_carries_provenance() {
        // The census is blind to any glyph without provenance. If capture
        // ever regressed to partial, every share would silently be taken
        // over a subset — so assert the invariant rather than trusting it.
        for name in ["simple-winansi.pdf", "identity-h-no-tounicode.pdf"] {
            assert_eq!(measure(name).unprovenanced_glyphs, 0, "{name}");
        }
    }

    #[test]
    fn one_show_operator_is_one_run_not_one_extraction_run() {
        // Guards the RunKey grouping: pdfcer's `TextRun` splits on geometry
        // and marked content, so a per-`TextRun` count would over-report
        // the number of show operators. The fixtures are small enough that
        // runs <= glyphs must hold with a wide margin.
        let c = measure("simple-winansi.pdf");
        assert!(
            c.totals.runs < c.totals.glyphs,
            "grouping by show operator should collapse many glyphs into few runs \
             (runs={}, glyphs={})",
            c.totals.runs,
            c.totals.glyphs
        );
    }

    #[test]
    fn every_outcome_shape_fills_exactly_the_column_list() {
        // The regression this pins: a failure row that is one field short
        // of the header shifts `detail` into a numeric column, so anything
        // reading the evidence file by index reads garbage — silently, and
        // only for the rows describing the files that failed.
        let measured = Outcome::Measured(measure("simple-winansi.pdf"));
        for outcome in [
            Outcome::ReadFailed("io".into()),
            Outcome::LoadFailed("bad xref".into()),
            Outcome::PageTreeFailed("no /Pages".into()),
            Outcome::Panicked("boom".into()),
            Outcome::TimedOut,
            measured,
        ] {
            let row = Row {
                corpus: "c".into(),
                rel: "f.pdf".into(),
                outcome,
            };
            assert_eq!(row_fields(&row).len(), COLUMNS.len(), "{}", row.rel);
        }
    }

    #[test]
    fn a_failure_rows_detail_lands_in_the_detail_column() {
        let row = Row {
            corpus: "c".into(),
            rel: "f.pdf".into(),
            outcome: Outcome::LoadFailed("bad xref".into()),
        };
        let fields = row_fields(&row);
        assert_eq!(fields[COLUMNS.len() - 1], "bad xref");
        assert_eq!(fields[2], "load-failed");
        // Everything between must be blank, not "0" — a zero would be read
        // as a measurement of a file that was never measured.
        assert!(fields[3..COLUMNS.len() - 1].iter().all(String::is_empty));
    }

    #[test]
    fn class_of_the_two_fixtures_differs() {
        // The headline claim in one assertion.
        let simple = measure("simple-winansi.pdf");
        let composite = measure("identity-h-no-tounicode.pdf");
        let simple_class = census::RunRecord {
            composite: false,
            glyphs: simple.totals.glyphs,
            invisible: 0,
            space_codes: simple.totals.space_codes_simple,
            font_conflict: false,
        }
        .class();
        assert_eq!(simple_class, RunClass::SimpleSpaced);
        assert_eq!(composite.totals.runs_simple, 0);
    }
}
