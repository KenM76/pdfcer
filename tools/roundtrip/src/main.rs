//! # roundtrip — the corpus-wide proof of `ARCHITECTURE.md` §5
//!
//! Walks one or more corpus directories, and for every `*.pdf` that
//! `pdfcer-core` can load, runs all three save paths and checks the
//! contract each one promises. Emits a per-file TSV and an aggregate
//! summary in which **every shortfall is enumerated by file and by
//! reason** — never rounded away (the R20-style counted-shortfall
//! discipline, applied to the writer).
//!
//! This is Pass 3.0's headline deliverable. Decision 007's argument for
//! doing the writer *now*, with no editing capability, is that the §5
//! invariant becomes a measured gate **before** any code exists that
//! could violate it. This tool is that gate.
//!
//! ## Usage
//!
//! ```text
//! cargo run --release -- [--mutate N | --no-mutate] <corpus-dir> [more-dirs ...]
//! ```
//!
//! Exit codes: `0` all files passed every applicable check; `1` at least
//! one shortfall or panic; `2` usage error; `3` a directory could not be
//! walked.
//!
//! ## The fourth mode: mutation (Pass 3.1)
//!
//! The three modes above prove that an **untouched** file survives a
//! save. That was the strongest claim available while pdfcer had no
//! editing capability, and it is not the claim Acrobat users actually
//! depend on. This one is:
//!
//! > changing one thing in a document does not perturb anything else.
//!
//! So for every loadable file the harness performs a real edit through
//! the real command log (`pdfcer_core::edit::EditSession` — the same type
//! the GUI and the CLI drive), saves incrementally, and checks three
//! things that were unmeasurable before Pass 3.1:
//!
//! | check | failure verdict |
//! |---|---|
//! | the edited object **changed as asked** (verified by reloading and reading the value back) | `MutationNotApplied` |
//! | every **other** object is still byte-verbatim through the reloaded file's own xref | `MutationPerturbedOthers` |
//! | **edit → undo → save is byte-identical to the input** | `UndoNotByteIdentical` |
//!
//! The third is `ARCHITECTURE.md` §11.1's contract — the dirty set is a
//! diff against the base revision, never the union of every command run
//! — evaluated across the whole corpus rather than against a fixture.
//! It is the check most likely to catch a regression, because the bug it
//! guards against produces a file that loads, renders and reloads
//! perfectly.
//!
//! The edit chosen is a 90° page rotation (Table 30) where the document
//! has a page, and a `/Title` metadata change otherwise. Both touch a
//! dictionary value and nothing else, which is the point: the mode
//! measures the dirty-set machinery, not content re-emission.
//!
//! Mutation runs on every loadable file by default; `--mutate N` caps it
//! at the first N (in sorted path order, so a capped run is
//! reproducible) and `--no-mutate` skips it entirely, which is what
//! reproduces a pre-Pass-3.1 measurement for comparison. Whichever was
//! used is printed in the summary, so a capped run can never be read as
//! a complete one.
//!
//! ## The three modes, and why they are three
//!
//! Decision 007 W1/R32 names conflating them *"the single likeliest
//! source of a false green or a false red in this Pass"*. Each mode
//! promises something different, and the tool asserts each mode's own
//! promise — never a weaker shared one.
//!
//! | Mode | Promise | Failure category |
//! |---|---|---|
//! | `incremental` (empty dirty set) | the output **is** the input, byte for byte | `NotByteIdentical` |
//! | `append-identity` (every object re-emitted unchanged) | every byte below the original EOF is unchanged (§7.5.6) | `AppendPerturbedPriorBytes` |
//! | `full` | every `File`-provenance object's **definition bytes** appear verbatim | `FullLostObjectBytes` |
//!
//! Plus, for the two modes that produce a new file: the output must
//! reload (`ReloadFailed`), and page 1 must re-render to an identical
//! raster (`RasterDiffers`).
//!
//! ### Why `append-identity` exists at all
//!
//! Without it, the headline gate would be a `memcpy` test. An empty
//! dirty set makes `save_incremental` return the input unchanged, by
//! design and by contract — which proves the retained buffer is intact
//! and proves nothing whatever about the §7.5.6 append writer. The
//! `append-identity` mode re-emits every object of the base revision
//! **unchanged**, which drives object re-emission, update-section
//! construction, `/Prev` chaining, trailer copying, `/Size` computation
//! and `startxref` placement over thousands of real files. It changes no
//! object's value, so it is a verification mode and not an editing
//! capability — Pass 3.0's non-goals hold.
//!
//! ## The raster oracle is a SELF-comparison
//!
//! Page 1 of the input is rendered, page 1 of the output is rendered,
//! and the pixel buffers are compared. No reference renderer is
//! involved. This is deliberately **not** Pass 1.1's outstanding
//! pdfcer-vs-pdfium pixel-parity harness, which remains owed and must not
//! be reported as closed by this tool. It is, however, most of the same
//! plumbing.
//!
//! A file whose page 1 does not render is not a round-trip failure —
//! plenty of conformance corpora contain deliberately broken files — so
//! the comparison is skipped and counted as skipped rather than failed.
//!
//! ## Refusals are outcomes, not failures
//!
//! `save_full` declines a §7.5.8.4 hybrid-reference file **by name**,
//! because flattening it to a single section would destroy its pre-1.5
//! readability (R33 forbids that normalization outright). A gate that
//! scored a principled refusal as a failure would be reporting a lie and
//! would pressure the implementation to guess. Refusals get their own
//! category and their own column.
//!
//! ## Isolation: one bad file must not take the run down
//!
//! Every file is measured on its own thread inside `catch_unwind`, with
//! a wall-clock budget. A panic becomes a reported finding with its
//! message; a hang becomes a `Timeout` and the worker is abandoned. Same
//! pattern as `tools/corpus-report`, and for the same reason: a
//! 2,900-file sweep that aborts on file 400 measures nothing.

use std::collections::BTreeMap;
use std::io::Write as _;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::mpsc;
use std::time::Duration;

use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditSession, InfoField};
use pdfcer_core::object::{ObjId, Provenance, equivalent_across_buffers};
use pdfcer_core::writer::{DirtySet, SaveOptions, save_full, save_incremental};
use pdfcer_core::xref::SectionShape;

/// Per-file wall-clock budget.
///
/// Larger than `corpus-report`'s 10 s because this tool does strictly
/// more work per file: three saves, two page renders, and a full object
/// graph comparison per produced file.
const FILE_BUDGET: Duration = Duration::from_secs(30);

/// Device pixels per user-space unit for the raster oracle
/// (≈36 DPI) — cheap, but it drives the whole content interpreter.
/// Matches `corpus-report`'s scale so the two tools stress the same
/// code path.
const RASTER_SCALE: f32 = 0.5;

/// Cap on objects re-emitted in `append-identity` mode.
///
/// A 40,000-object document would otherwise append a 40,000-object
/// revision, which is a memory and time cost with no extra coverage:
/// the append writer's branches are all exercised within the first few
/// dozen objects. The cap is recorded in the TSV so a truncated run is
/// never mistaken for a complete one.
const MAX_APPEND_OBJECTS: usize = 256;

/// Maximum characters kept of any detail string in the TSV.
const DETAIL_MAX: usize = 240;

/// One file's round-trip verdict.
///
/// Ordered worst-first in the `Ord` derive so the summary can present
/// the most serious findings at the top without a second sort key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Verdict {
    /// Every applicable check passed.
    Ok,
    /// The file does not load at all — outside this gate's denominator.
    /// Counted separately so the gate's own percentage is honest.
    NotLoadable,
    /// pdfcer declined a save by name (e.g. a hybrid full rewrite). A
    /// correct outcome, tallied apart from failures.
    Refused,
    /// Page 1 re-renders to different pixels after a save.
    RasterDiffers,
    /// A full rewrite dropped some object's verbatim definition bytes.
    FullLostObjectBytes,
    /// An append modified bytes below the original EOF — the §7.5.6
    /// violation that would break every prior signature.
    AppendPerturbedPriorBytes,
    /// An empty-dirty-set incremental save produced a file that is not
    /// byte-identical to the input. **The headline gate's failure.**
    NotByteIdentical,
    /// A produced file could not be loaded back by pdfcer itself.
    ReloadFailed,
    /// The object graph changed across a save.
    GraphChanged,
    /// An edit was made and saved, but reloading the result does not
    /// show it — the writer produced a plausible, working file that
    /// silently dropped the operator's change.
    MutationNotApplied,
    /// An edit perturbed an object it did not name — the minimal-diff
    /// violation the whole invariant exists to prevent, now measurable
    /// against a real mutation.
    MutationPerturbedOthers,
    /// **edit → undo → save was not byte-identical to the input.**
    /// `ARCHITECTURE.md` §11.1's "union of every command ever run" bug.
    UndoNotByteIdentical,
    /// The file exceeded [`FILE_BUDGET`].
    Timeout,
    /// A worker panicked. `pdfcer-core` has a crate-level panic-free
    /// policy, so any sighting is a bug, not a data problem.
    Panic,
}

impl Verdict {
    /// Stable name used in the TSV and the summary table.
    const fn name(self) -> &'static str {
        match self {
            Self::Ok => "Ok",
            Self::NotLoadable => "NotLoadable",
            Self::Refused => "Refused",
            Self::RasterDiffers => "RasterDiffers",
            Self::FullLostObjectBytes => "FullLostObjectBytes",
            Self::AppendPerturbedPriorBytes => "AppendPerturbedPriorBytes",
            Self::NotByteIdentical => "NotByteIdentical",
            Self::ReloadFailed => "ReloadFailed",
            Self::GraphChanged => "GraphChanged",
            Self::MutationNotApplied => "MutationNotApplied",
            Self::MutationPerturbedOthers => "MutationPerturbedOthers",
            Self::UndoNotByteIdentical => "UndoNotByteIdentical",
            Self::Timeout => "Timeout",
            Self::Panic => "Panic",
        }
    }

    /// Whether this verdict is a **shortfall against the §5 invariant**,
    /// as opposed to a file outside the gate's scope (`NotLoadable`) or
    /// a principled refusal (`Refused`).
    const fn is_shortfall(self) -> bool {
        !matches!(self, Self::Ok | Self::NotLoadable | Self::Refused)
    }
}

/// One file's measurement.
#[derive(Debug)]
struct Outcome {
    verdict: Verdict,
    /// Which mode produced the verdict (`""` for `Ok`/`NotLoadable`).
    mode: &'static str,
    /// Human-readable reason, sanitized and truncated for the TSV.
    detail: String,
    /// Counters, tallied across the corpus for the summary.
    stats: Stats,
}

/// Per-file counters rolled up into the aggregate summary.
#[derive(Debug, Default, Clone, Copy)]
struct Stats {
    /// The file loaded — i.e. it is inside the gate's denominator.
    loadable: usize,
    /// Empty-dirty-set save was byte-identical (the headline number).
    identity_byte_identical: usize,
    /// An append left every prior byte intact.
    append_prefix_preserved: usize,
    /// A full rewrite kept every object's definition bytes verbatim.
    full_per_object_verbatim: usize,
    /// A full rewrite was refused by name.
    full_refused: usize,
    /// Raster comparisons that ran (both renders succeeded).
    raster_compared: usize,
    /// Raster comparisons that matched.
    raster_identical: usize,
    /// Objects a full rewrite had to re-serialize rather than copy.
    /// Should be **0** corpus-wide under `SaveOptions::identity()`.
    reserialized_objects: usize,
    /// Files whose live Fast Web View property a save would spend.
    delinearized: usize,
    /// Files whose newest section is a classic §7.5.4 table.
    shape_classic: usize,
    /// Files whose newest section is a §7.5.8 cross-reference stream.
    shape_stream: usize,
    /// Files that are §7.5.8.4 hybrid-reference.
    shape_hybrid: usize,
    /// Files a real edit was attempted on.
    mutation_attempted: usize,
    /// Files where the edit reached the saved bytes and reloaded back.
    mutation_applied: usize,
    /// Files where every object the edit did not name stayed verbatim.
    mutation_others_intact: usize,
    /// Files where **edit → undo → save** reproduced the input exactly.
    /// The Pass 3.1 headline number.
    undo_byte_identical: usize,
    /// Objects promoted out of an object stream because an **edit**
    /// touched them (R38). Volume, not shortfall — but worth a census,
    /// because it is the first time this code path is reachable at all.
    promotions: usize,
    /// Objects promoted by the `append-identity` mode, which re-emits
    /// **every** object and therefore promotes every compressed one.
    ///
    /// Counted separately from [`Stats::promotions`] because the two
    /// answer different questions. This one is *"how much of this corpus
    /// is compressed at all?"* — an upper bound on how often R38 can
    /// ever fire. The other is *"how often does a realistic edit hit
    /// it?"*, which depends on **which** objects producers compress. The
    /// gap between them is informative: page objects turn out to be
    /// overwhelmingly uncompressed even in files that use object streams
    /// heavily, so a page-rotation edit rarely promotes anything while
    /// an edit to a font or metadata object routinely would.
    identity_promotions: usize,
    /// Files that contain at least one compressed object.
    files_with_compressed_objects: usize,
    /// Files whose page tree could not be walked, so no rotation edit
    /// was possible. Counted rather than silently skipped: they are
    /// outside the mutation gate's denominator and saying so keeps the
    /// percentage honest.
    mutation_skipped: usize,
}

fn main() -> ExitCode {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let mut mutate_limit = usize::MAX;
    let mut dirs: Vec<String> = Vec::new();
    let mut pending_limit = false;
    for arg in raw {
        if pending_limit {
            match arg.parse::<usize>() {
                Ok(n) => mutate_limit = n,
                Err(_) => {
                    eprintln!("error: --mutate expects a count, got {arg:?}");
                    return ExitCode::from(2);
                }
            }
            pending_limit = false;
        } else if arg == "--no-mutate" {
            mutate_limit = 0;
        } else if let Some(rest) = arg.strip_prefix("--mutate=") {
            match rest.parse::<usize>() {
                Ok(n) => mutate_limit = n,
                Err(_) => {
                    eprintln!("error: --mutate expects a count, got {rest:?}");
                    return ExitCode::from(2);
                }
            }
        } else if arg == "--mutate" {
            pending_limit = true;
        } else if arg.starts_with("--") {
            eprintln!("error: unknown option {arg}");
            return ExitCode::from(2);
        } else {
            dirs.push(arg);
        }
    }

    if dirs.is_empty() || pending_limit {
        eprintln!("usage: roundtrip [--mutate N | --no-mutate] <corpus-dir> [more-dirs ...]");
        eprintln!("  Walks each directory for *.pdf and proves the ARCHITECTURE.md §5");
        eprintln!("  round-trip invariant over every loadable file, in four modes:");
        eprintln!("  incremental (identity), append-identity, full rewrite, and mutation.");
        eprintln!("  --mutate N cap the mutation mode at the first N files (sorted order)");
        eprintln!("  --no-mutate  skip the mutation mode entirely");
        eprintln!("  Writes <dir>-roundtrip.tsv next to each directory.");
        return ExitCode::from(2);
    }

    // Worker panics are CAPTURED findings, not console spew.
    std::panic::set_hook(Box::new(|_| {}));

    let mut any_shortfall = false;
    for dir in &dirs {
        match run_corpus(Path::new(dir), mutate_limit) {
            Ok(shortfalls) => any_shortfall |= shortfalls > 0,
            Err(e) => {
                eprintln!("error: {dir}: {e}");
                return ExitCode::from(3);
            }
        }
    }
    if any_shortfall {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

/// Measure one corpus directory: walk, verify every file, write the TSV,
/// print the summary. Returns the shortfall count.
///
/// `mutate_limit` caps how many files get the mutation mode. The cap is
/// applied in sorted path order (which [`collect_pdfs`] guarantees), so a
/// capped run measures the same files every time and two runs are
/// comparable.
fn run_corpus(dir: &Path, mutate_limit: usize) -> Result<usize, String> {
    let files = collect_pdfs(dir)?;
    let total = files.len();
    eprintln!("[{}] {total} PDF file(s) found", dir.display());

    let mut rows: Vec<(String, Outcome)> = Vec::with_capacity(total);
    for (i, (rel, abs)) in files.into_iter().enumerate() {
        if i % 100 == 0 {
            eprintln!("[{}] {i}/{total} ...", dir.display());
        }
        rows.push((rel, measure_file(&abs, i < mutate_limit)));
    }

    write_tsv(dir, &rows)?;
    Ok(print_summary(dir, &rows, mutate_limit))
}

/// Recursively collect every `*.pdf` under `root`, skipping
/// dot-directories, sorted by relative path for determinism.
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
fn measure_file(path: &Path, mutate: bool) -> Outcome {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => return outcome(Verdict::NotLoadable, "", format!("read failed: {e}")),
    };

    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let result = match catch_unwind(AssertUnwindSafe(|| measure_bytes(bytes, mutate))) {
            Ok(o) => o,
            Err(payload) => {
                let msg = payload
                    .downcast_ref::<&str>()
                    .map(|s| (*s).to_owned())
                    .or_else(|| payload.downcast_ref::<String>().cloned())
                    .unwrap_or_else(|| "<non-string panic payload>".to_owned());
                outcome(Verdict::Panic, "", msg)
            }
        };
        // A late result after a timeout is deliberately dropped.
        let _ = tx.send(result);
    });

    match rx.recv_timeout(FILE_BUDGET) {
        Ok(o) => o,
        Err(_) => outcome(
            Verdict::Timeout,
            "",
            format!(
                "exceeded {}s budget; worker abandoned",
                FILE_BUDGET.as_secs()
            ),
        ),
    }
}

/// The verification itself, run on a worker thread inside
/// `catch_unwind`.
///
/// Checks run **cheapest-and-strongest first**: the empty-dirty-set
/// identity check is one `Vec` comparison and is the headline contract,
/// so a failure there short-circuits before any rendering happens.
fn measure_bytes(bytes: Vec<u8>, mutate: bool) -> Outcome {
    let source = bytes.clone();
    let doc = match Document::from_bytes(bytes) {
        Ok(doc) => doc,
        Err(e) => return outcome(Verdict::NotLoadable, "", e.to_string()),
    };

    let mut stats = Stats {
        loadable: 1,
        ..Stats::default()
    };
    match doc.section_shape() {
        SectionShape::Classic { xref_stm: None } => stats.shape_classic += 1,
        SectionShape::Classic { xref_stm: Some(_) } => stats.shape_hybrid += 1,
        SectionShape::Stream { .. } => stats.shape_stream += 1,
        _ => {}
    }
    if doc.linearization().save_invalidates_fast_web_view() {
        stats.delinearized += 1;
    }

    let opts = SaveOptions::identity();

    // --- mode 1: incremental, empty dirty set ------------------------
    // THE headline contract. Zero edits means zero bytes.
    match save_incremental(&doc, &DirtySet::empty(), &opts) {
        Ok((out, report)) => {
            if out != source || !report.byte_identical || report.bytes_appended != 0 {
                return Outcome {
                    verdict: Verdict::NotByteIdentical,
                    mode: "incremental",
                    detail: sanitize(&format!(
                        "empty-dirty-set save differs: in={} out={} appended={}",
                        source.len(),
                        report.bytes_written,
                        report.bytes_appended
                    )),
                    stats,
                };
            }
            stats.identity_byte_identical += 1;
        }
        Err(e) => {
            return Outcome {
                verdict: Verdict::NotByteIdentical,
                mode: "incremental",
                detail: sanitize(&format!("empty-dirty-set save failed: {e}")),
                stats,
            };
        }
    }

    // The "before" raster, computed once and shared by both produced
    // files. `None` means page 1 does not render — not a failure.
    let before = render_first_page(&doc);

    // --- mode 2: append-identity -------------------------------------
    // SORT BEFORE TAKE. `Document::objects()` iterates a `HashMap`'s values,
    // so its order varies run to run; taking the first `MAX_APPEND_OBJECTS`
    // without sorting samples a DIFFERENT subset of the file each time.
    //
    // That made this harness nondeterministic in a way that quietly undermined
    // its purpose: two consecutive runs of the SAME binary produced different
    // R38 promotion censuses, so the census could not be used to attribute a
    // change to a code edit. It was noticed while diffing a baseline against a
    // Pass 17.0 build — the only line that differed between the two reports was
    // this census, and it also differed between two runs of the baseline alone.
    // A corpus gate whose output moves on its own trains its readers to
    // discount real differences.
    //
    // Sorting by `ObjId` makes the sample deterministic and gives it a defined
    // meaning (the lowest-numbered objects) rather than an arbitrary one.
    let mut ids: Vec<ObjId> = doc.objects().map(|io| io.id).collect();
    ids.sort_unstable();
    ids.truncate(MAX_APPEND_OBJECTS);
    if !ids.is_empty() {
        match save_incremental(&doc, &DirtySet::identity_reemission(ids), &opts) {
            Ok((out, report)) => {
                // Every compressed object among the re-emitted ones had
                // to be promoted (R38) — there are no verbatim bytes to
                // copy for one. That makes this the corpus-wide census
                // of how much of the sample is compressed at all.
                stats.identity_promotions += report.promoted.len();
                if !report.promoted.is_empty() {
                    stats.files_with_compressed_objects += 1;
                }
                // §7.5.6: prior contents intact. The one permitted
                // insertion is a separating EOL when the base file's
                // final byte is not one (§7.2.3's comment rule).
                let prefix_ok = out.starts_with(&source)
                    || (out.get(..source.len()) == Some(&source[..])
                        && matches!(out.get(source.len()), Some(b'\n')));
                if !prefix_ok {
                    return Outcome {
                        verdict: Verdict::AppendPerturbedPriorBytes,
                        mode: "append-identity",
                        detail: "an appended revision modified bytes below the original EOF"
                            .to_owned(),
                        stats,
                    };
                }
                stats.append_prefix_preserved += 1;
                if let Some(bad) = check_reload(&doc, &out, "append-identity", stats) {
                    return bad;
                }
                if let Some(bad) =
                    check_raster(before.as_deref(), &out, "append-identity", &mut stats)
                {
                    return bad;
                }
            }
            Err(e) => {
                // A named refusal on the append path is unexpected but
                // is still a refusal, not a wrong file.
                return Outcome {
                    verdict: Verdict::Refused,
                    mode: "append-identity",
                    detail: sanitize(&e.to_string()),
                    stats,
                };
            }
        }
    }

    // --- mode 3: full rewrite ----------------------------------------
    match save_full(&doc, &DirtySet::empty(), &opts) {
        Ok((out, report)) => {
            stats.reserialized_objects += report.objects_reserialized;
            // Reload FIRST: the per-object check compares the span the
            // reloaded document resolved for each id, which proves the
            // bytes are reachable through the new cross-reference table
            // rather than merely present somewhere in the file.
            if let Some(bad) = check_reload(&doc, &out, "full", stats) {
                return bad;
            }
            match Document::from_bytes(out.clone()) {
                Ok(back) => {
                    if let Some(missing) = first_object_not_verbatim(&doc, &back) {
                        return Outcome {
                            verdict: Verdict::FullLostObjectBytes,
                            mode: "full",
                            detail: format!("object {missing} lost its verbatim definition bytes"),
                            stats,
                        };
                    }
                }
                Err(_) => unreachable!("check_reload already proved this loads"),
            }
            stats.full_per_object_verbatim += 1;
            if let Some(bad) = check_raster(before.as_deref(), &out, "full", &mut stats) {
                return bad;
            }
        }
        Err(e) => {
            // The expected case here is a hybrid-reference file, which
            // pdfcer declines to flatten (R33). Correct behaviour.
            stats.full_refused += 1;
            return Outcome {
                verdict: Verdict::Refused,
                mode: "full",
                detail: sanitize(&e.to_string()),
                stats,
            };
        }
    }

    // --- mode 4: mutation (Pass 3.1) ---------------------------------
    if mutate && let Some(bad) = check_mutation(&doc, &source, &mut stats) {
        return bad;
    }

    Outcome {
        verdict: Verdict::Ok,
        mode: "",
        detail: String::new(),
        stats,
    }
}

/// Perform a real edit through the command log and check the three
/// things only a real mutation can prove (module docs).
///
/// Returns `Some(failure)` on a shortfall, `None` on success or on a
/// principled skip. `doc` is consumed into an `EditSession`, so the
/// caller's document is re-parsed here rather than moved — the earlier
/// modes still need it, and a second parse of an already-proven-loadable
/// buffer cannot fail.
fn check_mutation(doc: &Document, source: &[u8], stats: &mut Stats) -> Option<Outcome> {
    let Ok(reparsed) = Document::from_bytes(source.to_vec()) else {
        return None; // unreachable: the file already loaded once
    };
    let mut session = EditSession::new(reparsed);

    // Choose the smallest edit the document can carry. A page rotation
    // where there is a page, a metadata change otherwise — both are
    // dictionary-value edits that touch no content stream, which is
    // what keeps this mode a measurement of the dirty-set machinery
    // rather than of content re-emission.
    let pages = session.pages().unwrap_or_default();
    let (edit, edited_id, expected_rotation) = if let Some(page) = pages.first() {
        let expected = (page.rotate + 90) % 360;
        (session.rotate_page_by(0, 90), Some(page.id), Some(expected))
    } else {
        (
            session.set_info_field(InfoField::Title, Some("pdfcer round-trip probe")),
            None,
            None,
        )
    };
    if edit.is_err() || !session.is_modified() {
        // A named refusal (a page object that is not a dictionary) or a
        // document with nothing editable. Outside this gate's
        // denominator; counted so the percentage stays honest.
        stats.mutation_skipped += 1;
        return None;
    }
    stats.mutation_attempted += 1;

    let opts = SaveOptions::identity();
    let (edited_bytes, report) = match session.to_incremental_bytes(&opts) {
        Ok(pair) => pair,
        Err(e) => {
            return Some(Outcome {
                verdict: Verdict::Refused,
                mode: "mutation",
                detail: sanitize(&format!("an edited save was refused: {e}")),
                stats: *stats,
            });
        }
    };
    stats.promotions += report.promoted.len();

    // Check 1: the edit reached the file and survives a reload.
    let Ok(back) = Document::from_bytes(edited_bytes.clone()) else {
        return Some(Outcome {
            verdict: Verdict::ReloadFailed,
            mode: "mutation",
            detail: "pdfcer could not reload a file it produced from an edit".to_owned(),
            stats: *stats,
        });
    };
    if let Some(expected) = expected_rotation {
        let got = pdfcer_core::page_tree::pages(&back)
            .ok()
            .and_then(|p| p.first().map(|page| page.rotate));
        if got != Some(expected) {
            return Some(Outcome {
                verdict: Verdict::MutationNotApplied,
                mode: "mutation",
                detail: format!("page 1 /Rotate is {got:?} after the save; expected {expected}"),
                stats: *stats,
            });
        }
    } else if title_of(&back).as_deref() != Some("pdfcer round-trip probe") {
        return Some(Outcome {
            verdict: Verdict::MutationNotApplied,
            mode: "mutation",
            detail: "the metadata edit is not present after the save".to_owned(),
            stats: *stats,
        });
    }
    stats.mutation_applied += 1;

    // Check 2: nothing else moved. Prior bytes intact (§7.5.6), and
    // every object the edit did not name still resolves — through the
    // reloaded file's OWN cross-reference table — to its original
    // definition bytes.
    let prefix_ok = edited_bytes.starts_with(source)
        || (edited_bytes.get(..source.len()) == Some(source)
            && matches!(edited_bytes.get(source.len()), Some(b'\n')));
    if !prefix_ok {
        return Some(Outcome {
            verdict: Verdict::MutationPerturbedOthers,
            mode: "mutation",
            detail: "an edited save modified bytes below the original EOF".to_owned(),
            stats: *stats,
        });
    }
    if let Some(bad) = first_object_perturbed(doc, &back, edited_id) {
        return Some(Outcome {
            verdict: Verdict::MutationPerturbedOthers,
            mode: "mutation",
            detail: format!("object {bad} was perturbed by an edit that did not name it"),
            stats: *stats,
        });
    }
    stats.mutation_others_intact += 1;

    // Check 3: THE contract. Undo everything, save, and the result must
    // be the input file, byte for byte (ARCHITECTURE.md §11.1).
    while session.undo().is_some() {}
    match session.to_incremental_bytes(&opts) {
        Ok((undone, _)) if undone == source => {
            stats.undo_byte_identical += 1;
            None
        }
        Ok((undone, _)) => Some(Outcome {
            verdict: Verdict::UndoNotByteIdentical,
            mode: "mutation",
            detail: format!(
                "edit -> undo -> save produced {} bytes; the input is {}",
                undone.len(),
                source.len()
            ),
            stats: *stats,
        }),
        Err(e) => Some(Outcome {
            verdict: Verdict::UndoNotByteIdentical,
            mode: "mutation",
            detail: sanitize(&format!("the post-undo save failed: {e}")),
            stats: *stats,
        }),
    }
}

/// The document's `/Title`, for the metadata-edit verification.
fn title_of(doc: &Document) -> Option<String> {
    let id = doc.trailer().get(b"Info")?.as_reference()?;
    let dict = doc.get(id)?.value.as_dict()?;
    match dict.get(b"Title")? {
        pdfcer_core::object::Object::String(bytes) => {
            Some(pdfcer_core::edit::decode_text_string(bytes).text)
        }
        _ => None,
    }
}

/// The first object — other than `edited` and the base file's own
/// cross-reference-stream object — whose definition bytes differ across
/// an edited save.
///
/// Same technique and same two reasons as [`first_object_not_verbatim`]:
/// linear rather than quadratic, and stricter, because resolving the
/// span through the reloaded document proves the bytes are reachable
/// *through the new cross-reference table* rather than merely present.
fn first_object_perturbed(
    before: &Document,
    after: &Document,
    edited: Option<ObjId>,
) -> Option<ObjId> {
    for io in before.objects() {
        if Some(io.id) == edited || is_section_object(before, io.id) {
            continue;
        }
        let Provenance::File(span) = io.provenance else {
            continue;
        };
        let want = span.slice(before.bytes())?;
        let got = after
            .get(io.id)
            .and_then(|o| o.file_span())
            .and_then(|s| s.slice(after.bytes()));
        if got != Some(want) {
            return Some(io.id);
        }
    }
    None
}

/// Load `out` back and compare the object graph against `doc`.
///
/// Returns `Some(failure)` on a reload failure or a graph change, else
/// `None`. The base file's own cross-reference-stream object is excluded
/// — it is superseded by the newly written section, so its dictionary
/// legitimately differs.
fn check_reload(doc: &Document, out: &[u8], mode: &'static str, stats: Stats) -> Option<Outcome> {
    let back = match Document::from_bytes(out.to_vec()) {
        Ok(back) => back,
        Err(e) => {
            return Some(Outcome {
                verdict: Verdict::ReloadFailed,
                mode,
                detail: sanitize(&format!("pdfcer could not reload its own output: {e}")),
                stats,
            });
        }
    };
    for io in doc.objects() {
        if is_section_object(doc, io.id) {
            continue;
        }
        // Cross-buffer comparison, NOT the derived PartialEq: a save
        // legitimately moves streams, and `Stream` stores a span rather
        // than bytes, so span-sensitive equality reports a phantom
        // change on every stream-bearing file.
        let same = back.get(io.id).is_some_and(|b| {
            equivalent_across_buffers(&b.value, back.bytes(), &io.value, doc.bytes())
        });
        if !same {
            return Some(Outcome {
                verdict: Verdict::GraphChanged,
                mode,
                detail: format!("object {} changed across the save", io.id),
                stats,
            });
        }
    }
    None
}

/// Compare page 1's raster before and after.
///
/// Skipped (and counted as skipped) when either render fails: a
/// conformance corpus contains deliberately broken files, and scoring
/// those as round-trip failures would be a false red.
fn check_raster(
    before: Option<&[u8]>,
    out: &[u8],
    mode: &'static str,
    stats: &mut Stats,
) -> Option<Outcome> {
    let before = before?;
    let after_doc = Document::from_bytes(out.to_vec()).ok()?;
    let after = render_first_page(&after_doc)?;
    stats.raster_compared += 1;
    if before == after.as_slice() {
        stats.raster_identical += 1;
        return None;
    }
    Some(Outcome {
        verdict: Verdict::RasterDiffers,
        mode,
        detail: format!(
            "page 1 raster differs after the save ({} vs {} bytes)",
            before.len(),
            after.len()
        ),
        stats: *stats,
    })
}

/// The first object whose **definition bytes** differ between the base
/// document and the reloaded output, or `None` if every one survived —
/// the R32 per-object assertion.
///
/// Compares each object's retained `ByteSpan` slice on both sides rather
/// than searching the output for the bytes. That is two improvements at
/// once, and the second one matters more:
///
/// 1. **It is linear.** A substring search per object is
///    `objects x filesize`; the veraPDF corpus contains a deliberate
///    Annex C implementation-limits file with ~80,000 objects in 4 MB,
///    which is ~320 GB of byte comparisons and blows any wall-clock
///    budget. This is one slice comparison per object.
/// 2. **It is stricter.** "These bytes appear somewhere in the output"
///    is a weak claim — it would pass even if the object landed at an
///    offset its own cross-reference entry does not name. Comparing the
///    span the RELOADED document resolved for that object id proves the
///    bytes are reachable *through the xref*, which is the property
///    §5 actually promises.
fn first_object_not_verbatim(before: &Document, after: &Document) -> Option<ObjId> {
    for io in before.objects() {
        if is_section_object(before, io.id) {
            continue;
        }
        let Provenance::File(span) = io.provenance else {
            // A compressed object has no file-level bytes of its own;
            // its container carries them and is checked in its turn.
            continue;
        };
        let want = span.slice(before.bytes())?;
        let got = after
            .get(io.id)
            .and_then(|o| o.file_span())
            .and_then(|s| s.slice(after.bytes()));
        if got != Some(want) {
            return Some(io.id);
        }
    }
    None
}

/// Whether `id` is the object that *is* the base file's newest
/// cross-reference section (§7.5.8.1) rather than document content.
fn is_section_object(doc: &Document, id: ObjId) -> bool {
    matches!(doc.section_shape(), SectionShape::Stream { id: sid, .. } if sid == id)
}

/// Rasterize page 1 at [`RASTER_SCALE`], or `None` if it does not
/// render.
fn render_first_page(doc: &Document) -> Option<Vec<u8>> {
    let pages = pdfcer_core::page_tree::pages(doc).ok()?;
    let page = pages.first()?;
    let rendered = pdfcer_render::render_page(doc, page, RASTER_SCALE).ok()?;
    Some(rendered.pixmap.data().to_vec())
}

fn outcome(verdict: Verdict, mode: &'static str, detail: impl Into<String>) -> Outcome {
    Outcome {
        verdict,
        mode,
        detail: sanitize(&detail.into()),
        stats: Stats::default(),
    }
}

/// Flatten tabs/newlines and truncate, so one row is one TSV line.
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

/// Write `<dir>-roundtrip.tsv` beside the corpus directory.
///
/// One row per file, always — including the passing ones. A gate whose
/// evidence file lists only failures cannot be diffed against a previous
/// run to spot a *newly* passing file, and regression detection needs
/// both directions.
fn write_tsv(dir: &Path, rows: &[(String, Outcome)]) -> Result<(), String> {
    let file_name = format!(
        "{}-roundtrip.tsv",
        dir.file_name()
            .map_or_else(|| "corpus".into(), |n| n.to_string_lossy())
    );
    let path = dir.parent().unwrap_or(dir).join(file_name);
    let mut w = std::io::BufWriter::new(
        std::fs::File::create(&path).map_err(|e| format!("create {}: {e}", path.display()))?,
    );
    writeln!(w, "file\tverdict\tmode\tdetail").map_err(|e| e.to_string())?;
    for (rel, o) in rows {
        writeln!(w, "{rel}\t{}\t{}\t{}", o.verdict.name(), o.mode, o.detail)
            .map_err(|e| e.to_string())?;
    }
    eprintln!("[{}] TSV written: {}", dir.display(), path.display());
    Ok(())
}

/// Print the aggregate summary. Returns the shortfall count.
///
/// Every shortfall is listed **by file and by reason**. The list is not
/// truncated: a gate that says "and 47 others" has thrown away exactly
/// the information the next engineer needs.
fn print_summary(dir: &Path, rows: &[(String, Outcome)], mutate_limit: usize) -> usize {
    let total = rows.len();
    println!("=== roundtrip: {} ({total} files) ===", dir.display());

    let mut counts: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut s = Stats::default();
    for (_, o) in rows {
        *counts.entry(o.verdict.name()).or_insert(0) += 1;
        s.loadable += o.stats.loadable;
        s.identity_byte_identical += o.stats.identity_byte_identical;
        s.append_prefix_preserved += o.stats.append_prefix_preserved;
        s.full_per_object_verbatim += o.stats.full_per_object_verbatim;
        s.full_refused += o.stats.full_refused;
        s.raster_compared += o.stats.raster_compared;
        s.raster_identical += o.stats.raster_identical;
        s.reserialized_objects += o.stats.reserialized_objects;
        s.delinearized += o.stats.delinearized;
        s.shape_classic += o.stats.shape_classic;
        s.shape_stream += o.stats.shape_stream;
        s.shape_hybrid += o.stats.shape_hybrid;
        s.mutation_attempted += o.stats.mutation_attempted;
        s.mutation_applied += o.stats.mutation_applied;
        s.mutation_others_intact += o.stats.mutation_others_intact;
        s.undo_byte_identical += o.stats.undo_byte_identical;
        s.promotions += o.stats.promotions;
        s.identity_promotions += o.stats.identity_promotions;
        s.files_with_compressed_objects += o.stats.files_with_compressed_objects;
        s.mutation_skipped += o.stats.mutation_skipped;
    }

    let mut table: Vec<(&str, usize)> = counts.into_iter().collect();
    table.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    println!("\n-- verdicts --");
    for (name, n) in &table {
        println!("{name:<28} {n:>6}  {:>6.2}%", pct(*n, total));
    }

    // The denominator that matters: files pdfcer can load. A file it
    // cannot load is outside the §5 gate's scope, and folding it in
    // would flatter the percentage.
    let d = s.loadable;
    println!("\n-- the §5 gate (denominator = {d} loadable files) --");
    println!(
        "incremental, empty dirty set -> BYTE-IDENTICAL   {:>6} / {d}  {:>6.2}%",
        s.identity_byte_identical,
        pct(s.identity_byte_identical, d)
    );
    println!(
        "append-identity -> prior bytes intact (§7.5.6)   {:>6} / {d}  {:>6.2}%",
        s.append_prefix_preserved,
        pct(s.append_prefix_preserved, d)
    );
    println!(
        "full rewrite -> per-object verbatim (R32)        {:>6} / {d}  {:>6.2}%",
        s.full_per_object_verbatim,
        pct(s.full_per_object_verbatim, d)
    );
    println!(
        "full rewrite refused by name (hybrid, R33)       {:>6} / {d}  {:>6.2}%",
        s.full_refused,
        pct(s.full_refused, d)
    );
    println!(
        "raster oracle: identical / compared              {:>6} / {}",
        s.raster_identical, s.raster_compared
    );

    // The mutation gate has its OWN denominator — files a real edit was
    // performed on — which is smaller than `loadable` whenever the run
    // was capped or a document had nothing editable. Reusing `loadable`
    // here would understate every percentage and make a capped run look
    // like a failing one.
    let m = s.mutation_attempted;
    let scope = if mutate_limit == 0 {
        "SKIPPED (--no-mutate)".to_owned()
    } else if mutate_limit == usize::MAX {
        format!(
            "every loadable file; {} had nothing editable",
            s.mutation_skipped
        )
    } else {
        format!(
            "capped at the first {mutate_limit} files (sorted order); {} had nothing editable",
            s.mutation_skipped
        )
    };
    println!("\n-- the mutation gate (denominator = {m} edited files) --");
    println!("scope: {scope}");
    if m > 0 {
        println!(
            "the edit reached the file and reloaded          {:>6} / {m}  {:>6.2}%",
            s.mutation_applied,
            pct(s.mutation_applied, m)
        );
        println!(
            "every OTHER object still byte-verbatim         {:>6} / {m}  {:>6.2}%",
            s.mutation_others_intact,
            pct(s.mutation_others_intact, m)
        );
        println!(
            "edit -> undo -> save is BYTE-IDENTICAL (§11.1) {:>6} / {m}  {:>6.2}%",
            s.undo_byte_identical,
            pct(s.undo_byte_identical, m)
        );
        println!(
            "objects promoted by the EDIT (R38)             {:>6}   (census, not a shortfall)",
            s.promotions
        );
    }

    println!("\n-- structural census --");
    println!(
        "files with compressed objects (§7.5.7) {:>6}",
        s.files_with_compressed_objects
    );
    println!(
        "compressed objects, corpus-wide        {:>6}   (upper bound on how often R38 can fire)",
        s.identity_promotions
    );
    println!("classic §7.5.4 table   {:>6}", s.shape_classic);
    println!("xref stream §7.5.8     {:>6}", s.shape_stream);
    println!("hybrid §7.5.8.4        {:>6}", s.shape_hybrid);
    println!("live linearized (F.1)  {:>6}", s.delinearized);
    println!(
        "objects re-serialized  {:>6}   (MUST be 0 under SaveOptions::identity())",
        s.reserialized_objects
    );

    let shortfalls: Vec<&(String, Outcome)> = rows
        .iter()
        .filter(|(_, o)| o.verdict.is_shortfall())
        .collect();
    if shortfalls.is_empty() {
        println!("\nNo shortfalls. The §5 round-trip invariant holds across this corpus.");
    } else {
        println!(
            "\n-- SHORTFALLS, enumerated by file and reason ({}) --",
            shortfalls.len()
        );
        for (rel, o) in &shortfalls {
            println!(
                "{:<28} {:<16} {rel}\n    {}",
                o.verdict.name(),
                o.mode,
                o.detail
            );
        }
    }
    shortfalls.len()
}

fn pct(n: usize, d: usize) -> f64 {
    if d == 0 {
        0.0
    } else {
        n as f64 * 100.0 / d as f64
    }
}
