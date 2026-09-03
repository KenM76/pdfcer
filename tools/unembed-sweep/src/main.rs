//! # `unembed-sweep` — does an unembedded file still open, and still render?
//!
//! Pass 67.0 phase B's corpus harness. It runs the **whole** operation over
//! every PDF under the given directories and reports what happened, with the
//! denominators spelled out.
//!
//! ## The question this exists to answer
//!
//! Fixtures prove each branch works. They cannot prove the branches cover
//! what real producers emit, and they cannot answer the question that
//! decides whether a destructive feature is shippable:
//!
//! > **Does the file still open afterwards, and does page 1 still render?**
//!
//! A file that unembeds and then fails to load is the failure mode an
//! operator would find rather than a test. So every file that has anything
//! to unembed goes through the complete round trip:
//!
//! ```text
//!   Document::from_bytes(corpus file)
//!     │
//!     │  EditSession::unembed_fonts(AllRemovable)
//!     ▼
//!   EditSession  ── to_full_bytes ──►  Vec<u8>      ← the ONLY save mode
//!                                        │            that reclaims bytes
//!                                        │  Document::from_bytes
//!                                        ▼
//!                                     Document
//!                                        ├─ fontinfo::inventory   (are the
//!                                        │                         programs
//!                                        │                         gone?)
//!                                        └─ render_page(page 1)   (does it
//!                                                                  still
//!                                                                  draw?)
//! ```
//!
//! ## What is measured, and what each number means
//!
//! | Column | Meaning |
//! |---|---|
//! | `load` | the corpus file opened at all |
//! | `targets` | fonts the plan would unembed |
//! | `blocked` | fonts refused, each with a stated reason |
//! | `reclaim` | bytes a full rewrite drops, from the plan |
//! | `applied` | `unembed_fonts` succeeded |
//! | `saved` | `to_full_bytes` produced bytes |
//! | `reopen` | ★ the written bytes parse as a `Document` |
//! | `render` | ★ page 1 rasterises after the removal |
//! | `still_embedded` | ★ programs still present after the operation — must be 0 for the targeted fonts |
//! | `delta` | output size minus input size; negative is a saving |
//!
//! The two starred reopen/render columns are the point. Everything else is
//! context for them.
//!
//! ## What it deliberately does NOT do
//!
//! It does not compare the rendered page against the original. It **cannot**:
//! unembedding is defined to change how the page looks, so a pixel
//! difference is the expected outcome and a pixel comparison would measure
//! nothing. What it checks is that the rasteriser still produces a page —
//! that the document is structurally intact and every font reference still
//! resolves to *something* a reader can draw with.
//!
//! It also does not write to any corpus file. Inputs are read; outputs go to
//! a temp directory or nowhere at all (`--no-write`).
//!
//! ## Usage
//!
//! ```text
//! cargo run --release -- [--tsv <path>] [--limit N] [--no-write] <corpus-dir> [more-dirs ...]
//! ```
//!
//! Exit codes: `0` the sweep completed (whatever the numbers were), `2`
//! usage error, `3` a corpus directory could not be walked. A sweep has no
//! notion of "failing" — an unopenable file is data about the corpus, not a
//! gate breach.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::font_unembed::UnembedRequest;
use pdfcer_core::fontinfo;
use pdfcer_core::page_tree::pages_in;
use pdfcer_core::writer::SaveOptions;
use pdfcer_render::render_page;

/// One file's result.
struct Row {
    file: String,
    /// `None` when the file would not open at all.
    outcome: Outcome,
}

/// What happened to one file, as a state machine that stops at the first
/// thing that did not work — so a "render failed" row is never confused with
/// a "never got that far" row.
enum Outcome {
    /// The corpus file would not open. Data about the corpus.
    LoadFailed(String),
    /// Opened, but the plan had nothing to unembed. Carries the refusal
    /// counts so the corpus-wide verdict distribution is measurable.
    NothingToDo { blocked: BTreeMap<String, usize> },
    /// `unembed_fonts` refused after the plan said it had targets.
    ApplyRefused(String),
    /// The save failed.
    SaveFailed(String),
    /// ★ The written bytes would not parse. The failure this sweep exists
    /// to detect.
    ReopenFailed(String),
    /// Reopened; the render is reported separately because a document can
    /// be structurally fine and still hit a rasteriser limitation.
    Done {
        targets: usize,
        blocked: BTreeMap<String, usize>,
        reclaim: u64,
        in_bytes: usize,
        out_bytes: usize,
        /// Embedded programs still present after the operation, over the
        /// WHOLE document. Nonzero is legitimate — a blocked font keeps its
        /// program — so this is reported, not asserted.
        still_embedded: usize,
        /// ★ How many of the fonts this operation TARGETED still carry an
        /// embedded program after the round trip. **Must be zero.**
        ///
        /// `still_embedded` counts the whole document and is legitimately
        /// non-zero (a blocked font keeps its program), so it cannot answer
        /// "did the removal actually happen". This can, and it is the only
        /// number in the sweep with a required value.
        targets_still_embedded: usize,
        /// `Ok(())` when page 1 rasterised, `Err(reason)` otherwise, `None`
        /// when the document has no pages.
        render: Option<Result<(), String>>,
        /// ★ Whether page 1 rasterised **before** the operation.
        ///
        /// Without this the `render` column measures the corpus, not the
        /// feature. The veraPDF corpus deliberately contains malformed
        /// files — three of them carry an invalid hexadecimal string that
        /// stops the lexer at byte 85 — and they fail identically before and
        /// after. A harness that counted those as "unembedding broke the
        /// render" would report a defect that does not exist, and would keep
        /// reporting it until somebody checked by hand.
        baseline_render: Option<Result<(), String>>,
        /// The full rewrite was refused (a hybrid-reference file) and the
        /// round trip continued through an incremental save. `delta` is
        /// positive for these by construction.
        incremental: bool,
    },
}

fn main() {
    let mut args = std::env::args().skip(1);
    let mut roots: Vec<PathBuf> = Vec::new();
    let mut tsv: Option<PathBuf> = None;
    let mut limit: Option<usize> = None;
    let mut write_output = true;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--tsv" => match args.next() {
                Some(p) => tsv = Some(PathBuf::from(p)),
                None => usage("--tsv needs a path"),
            },
            "--limit" => match args.next().and_then(|n| n.parse().ok()) {
                Some(n) => limit = Some(n),
                None => usage("--limit needs a number"),
            },
            // Keeps the round trip entirely in memory. The reopen and render
            // checks still run — they read the produced bytes, not a file —
            // so this changes only whether anything lands on disk.
            "--no-write" => write_output = false,
            other if other.starts_with("--") => usage(&format!("unknown flag {other}")),
            other => roots.push(PathBuf::from(other)),
        }
    }
    if roots.is_empty() {
        usage("at least one corpus directory is required");
    }

    let out_dir = std::env::temp_dir().join("pdfcer-unembed-sweep");
    if write_output {
        let _ = fs::create_dir_all(&out_dir);
    }

    let mut files: Vec<(String, PathBuf)> = Vec::new();
    for root in &roots {
        match collect_pdfs(root) {
            Ok(found) => files.extend(found),
            Err(e) => {
                eprintln!("unembed-sweep: {e}");
                std::process::exit(3);
            }
        }
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));
    if let Some(n) = limit {
        files.truncate(n);
    }

    eprintln!("unembed-sweep: {} file(s)", files.len());
    let mut rows: Vec<Row> = Vec::with_capacity(files.len());
    for (i, (rel, path)) in files.iter().enumerate() {
        if i % 200 == 0 {
            eprintln!("  {i}/{}", files.len());
        }
        // `catch_unwind` so one panicking file does not abandon the sweep.
        // A panic is itself a finding and is recorded as a load failure with
        // its message, rather than taking the run down.
        let outcome = std::panic::catch_unwind(|| measure(path, &out_dir, write_output, rel))
            .unwrap_or_else(|_| Outcome::LoadFailed("panicked".to_owned()));
        rows.push(Row {
            file: rel.clone(),
            outcome,
        });
    }

    report(&rows);
    if let Some(path) = tsv {
        write_tsv(&path, &rows);
    }
}

fn usage(msg: &str) -> ! {
    eprintln!("unembed-sweep: {msg}");
    eprintln!(
        "usage: unembed-sweep [--tsv <path>] [--limit N] [--no-write] <corpus-dir> [more ...]"
    );
    std::process::exit(2);
}

/// Run the complete round trip on one file.
fn measure(path: &Path, out_dir: &Path, write_output: bool, rel: &str) -> Outcome {
    let bytes = match fs::read(path) {
        Ok(b) => b,
        Err(e) => return Outcome::LoadFailed(format!("read: {e}")),
    };
    let in_bytes = bytes.len();
    let doc = match Document::from_bytes(bytes) {
        Ok(d) => d,
        Err(e) => return Outcome::LoadFailed(e.to_string()),
    };
    // ★ The BASELINE render, taken before anything changes. See
    // `Outcome::Done::baseline_render` for why this is not optional.
    let baseline_render = match pages_in(&doc.view()) {
        Ok(pages) => pages.first().map(|first| {
            render_page(&doc, first, 0.5)
                .map(|_| ())
                .map_err(|e| e.to_string())
        }),
        Err(e) => Some(Err(format!("page tree: {e}"))),
    };

    let mut session = EditSession::new(doc);
    let request = UnembedRequest::all_removable();
    let plan = session.unembed_preview(&request);
    let blocked = count_blockers(&plan);
    if plan.targets.is_empty() {
        return Outcome::NothingToDo { blocked };
    }
    let targets = plan.targets.len();
    let reclaim = plan.bytes_reclaimable();
    let target_ids: Vec<_> = plan.targets.iter().map(|t| t.id).collect();

    if let Err(e) = session.unembed_fonts(&request) {
        return Outcome::ApplyRefused(e.to_string());
    }
    // A FULL REWRITE, deliberately: it is the only save mode that actually
    // drops the freed objects, and this sweep's `delta` column would be
    // meaningless under an incremental save (which appends and therefore
    // always grows the file).
    //
    // ★ WITH ONE FALLBACK, and it is a measurement decision rather than a
    // convenience. A hybrid-reference file (§7.5.8.4) refuses a full rewrite
    // by design — pdfcer will not silently normalise a cross-reference
    // structure it did not author. That refusal is not a failure of THIS
    // feature, and treating it as one would hide the question the sweep
    // exists to answer: does the unembedded file still open and render? So
    // the fallback re-saves incrementally, the round trip continues, and the
    // row is marked `incremental` so its `delta` (which will be positive) is
    // not read as a size regression.
    let (written, incremental) = match session.to_full_bytes(&SaveOptions::default()) {
        Ok((bytes, _)) => (bytes, false),
        Err(_) => match session.to_incremental_bytes(&SaveOptions::identity()) {
            Ok((bytes, _)) => (bytes, true),
            Err(e) => return Outcome::SaveFailed(e.to_string()),
        },
    };
    let out_bytes = written.len();
    if write_output {
        let name = rel.replace(['/', '\\'], "_");
        let _ = fs::write(out_dir.join(name), &written);
    }

    // ★ The check the whole harness exists for: the produced bytes are
    // parsed by a FRESH `Document`, not inspected through the session that
    // made them. A session that can still answer questions about a file it
    // broke proves nothing.
    let reopened = match Document::from_bytes(written) {
        Ok(d) => d,
        Err(e) => return Outcome::ReopenFailed(e.to_string()),
    };
    let inv = fontinfo::inventory(&reopened.view());
    let still_embedded = inv.embedded_count();
    // ★ Measured over the REOPENED bytes and keyed on the object ids the
    // plan named, so it answers "did the program actually leave the file"
    // rather than "did the session think it removed something".
    let targets_still_embedded = inv
        .fonts
        .iter()
        .filter(|f| f.program.is_embedded())
        .filter(|f| f.id.is_some_and(|id| target_ids.contains(&id)))
        .count();

    let render = match pages_in(&reopened.view()) {
        Ok(pages) => pages.first().map(|first| {
            // Half scale, matching `corpus-report`, so the two harnesses
            // stress the rasteriser the same way.
            render_page(&reopened, first, 0.5)
                .map(|_| ())
                .map_err(|e| e.to_string())
        }),
        Err(e) => Some(Err(format!("page tree: {e}"))),
    };

    Outcome::Done {
        targets,
        blocked,
        reclaim,
        in_bytes,
        out_bytes,
        still_embedded,
        targets_still_embedded,
        render,
        baseline_render,
        incremental,
    }
}

/// How many blocked fonts carry each reason.
fn count_blockers(plan: &pdfcer_core::font_unembed::UnembedPlan) -> BTreeMap<String, usize> {
    let mut out = BTreeMap::new();
    for b in &plan.blocked {
        *out.entry(b.blocker.token().to_owned()).or_insert(0) += 1;
    }
    out
}

/// Print the aggregate, with denominators spelled out.
#[allow(clippy::cast_precision_loss)]
fn report(rows: &[Row]) {
    let total = rows.len();
    let mut load_failed = 0usize;
    let mut nothing_to_do = 0usize;
    let mut apply_refused = 0usize;
    let mut save_failed = 0usize;
    let mut reopen_failed = 0usize;
    let mut done = 0usize;
    let mut rendered_ok = 0usize;
    let mut render_failed = 0usize;
    let mut no_pages = 0usize;
    let mut fonts_unembedded = 0usize;
    let mut reclaim_total = 0u64;
    let mut delta_total = 0i64;
    let mut shrank = 0usize;
    let mut still_embedded_files = 0usize;
    let mut fell_back = 0usize;
    let mut already_broken = 0usize;
    let mut targets_survived = 0usize;
    let mut blockers: BTreeMap<String, usize> = BTreeMap::new();
    let mut reopen_reasons: BTreeMap<String, usize> = BTreeMap::new();
    let mut render_reasons: BTreeMap<String, usize> = BTreeMap::new();

    for row in rows {
        match &row.outcome {
            Outcome::LoadFailed(_) => load_failed += 1,
            Outcome::NothingToDo { blocked } => {
                nothing_to_do += 1;
                merge(&mut blockers, blocked);
            }
            Outcome::ApplyRefused(_) => apply_refused += 1,
            Outcome::SaveFailed(_) => save_failed += 1,
            Outcome::ReopenFailed(why) => {
                reopen_failed += 1;
                *reopen_reasons.entry(short(why)).or_insert(0) += 1;
            }
            Outcome::Done {
                targets,
                blocked,
                reclaim,
                in_bytes,
                out_bytes,
                still_embedded,
                targets_still_embedded,
                render,
                baseline_render,
                incremental,
            } => {
                done += 1;
                if *incremental {
                    fell_back += 1;
                }
                fonts_unembedded += targets;
                reclaim_total += reclaim;
                merge(&mut blockers, blocked);
                let delta = *out_bytes as i64 - *in_bytes as i64;
                // Only full-rewrite rows contribute to the size figures. An
                // incremental fallback always grows the file, and averaging
                // it in would understate what the operation recovers.
                if !*incremental {
                    delta_total += delta;
                    if delta < 0 {
                        shrank += 1;
                    }
                }
                if *still_embedded > 0 {
                    still_embedded_files += 1;
                }
                targets_survived += targets_still_embedded;
                match (render, baseline_render) {
                    (Some(Ok(())), _) => rendered_ok += 1,
                    // ★ Failed after AND before: a property of the corpus
                    // file, not of the operation. Counted separately so the
                    // regression number means what it says.
                    (Some(Err(_)), Some(Err(_))) => already_broken += 1,
                    (Some(Err(why)), _) => {
                        render_failed += 1;
                        *render_reasons.entry(short(why)).or_insert(0) += 1;
                    }
                    (None, _) => no_pages += 1,
                }
            }
        }
    }

    let pct = |n: usize, d: usize| -> String {
        if d == 0 {
            "-".to_owned()
        } else {
            format!("{:.1}%", n as f64 * 100.0 / d as f64)
        }
    };

    println!("== unembed-sweep ==");
    println!("files                {total}");
    println!(
        "  load failed        {load_failed} ({})",
        pct(load_failed, total)
    );
    println!(
        "  nothing to unembed {nothing_to_do} ({})",
        pct(nothing_to_do, total)
    );
    println!("  apply refused      {apply_refused}");
    println!("  save failed        {save_failed}");
    println!("  ★ reopen FAILED    {reopen_failed}");
    println!(
        "  completed          {done} ({} of files with something to do)",
        pct(done, done + apply_refused + save_failed + reopen_failed)
    );
    println!();
    println!("of the {done} completed:");
    println!(
        "  ★ page 1 rendered  {rendered_ok} ({} of those with a page)",
        pct(rendered_ok, rendered_ok + render_failed)
    );
    println!("  ★ render REGRESSED {render_failed} (rendered before, not after)");
    println!(
        "  already unrenderable before the operation: {already_broken} (corpus defect, not this feature)"
    );
    println!("  no pages           {no_pages}");
    println!("  fonts unembedded   {fonts_unembedded}");
    println!("  reclaim (plan)     {reclaim_total} bytes");
    println!("  size delta (real)  {delta_total} bytes");
    println!(
        "  got smaller        {shrank} ({} of the {} full rewrites)",
        pct(shrank, (done - fell_back).max(1)),
        done - fell_back
    );
    println!(
        "  full rewrite refused, saved incrementally instead: {fell_back} (hybrid-reference files, §7.5.8.4 — not a defect)"
    );
    println!(
        "  still have an embedded font afterwards: {still_embedded_files} (expected — a blocked font keeps its program)"
    );
    println!(
        "  ★ TARGETED fonts still embedded after the round trip: {targets_survived} (must be 0)"
    );
    println!();
    // ★ BOTH denominators, spelled out, because one of them was mislabelled
    // once and the mislabelling reached a commit message and a librarian's
    // filing before anyone re-derived it. "Share of refusals" and "share of
    // embedded fonts" are different questions with different answers — 31.6%
    // and 53.6% for the same 836 fonts — and a table headed only "refusal
    // reasons" invites the reader to supply whichever denominator they had
    // in mind.
    let blocked_total: usize = blockers.values().sum();
    // Fonts that actually carry a readable program: everything examined,
    // less the ones with nothing to remove (not embedded, Type 3 — whose
    // glyphs are content streams) and the ones whose program could not be
    // read at all.
    let embedded_total = blocked_total + fonts_unembedded
        - blockers.get("not-embedded").copied().unwrap_or(0)
        - blockers.get("blocked-type3").copied().unwrap_or(0)
        - blockers
            .get("unknown-program-unreadable")
            .copied()
            .unwrap_or(0);
    println!(
        "verdicts: {} fonts examined, {blocked_total} refused, {embedded_total} carrying a readable embedded program",
        blocked_total + fonts_unembedded
    );
    println!(
        "  {:<28} {:>6}  {:>8}  {:>8}",
        "verdict", "n", "of-ref", "of-emb"
    );
    for (token, n) in &blockers {
        // The three verdicts that are BY DEFINITION not in the embedded set
        // print `-` in that column rather than a number. A percentage of a
        // denominator the row is excluded from is not a small inaccuracy —
        // it is a figure that reads as meaningful and is not.
        let of_embedded = match token.as_str() {
            "not-embedded" | "blocked-type3" | "unknown-program-unreadable" => "-".to_owned(),
            _ => pct(*n, embedded_total),
        };
        println!(
            "  {token:<28} {n:>6}  {:>8}  {of_embedded:>8}",
            pct(*n, blocked_total),
        );
    }
    println!(
        "  {:<28} {fonts_unembedded:>6}  {:>8}  {:>8}",
        "removable",
        "-",
        pct(fonts_unembedded, embedded_total)
    );
    if !reopen_reasons.is_empty() {
        println!();
        println!("★ REOPEN FAILURES (the failure mode that matters):");
        for (why, n) in &reopen_reasons {
            println!("  {n:>5}  {why}");
        }
    }
    if !render_reasons.is_empty() {
        println!();
        println!("render failures after unembedding:");
        for (why, n) in &render_reasons {
            println!("  {n:>5}  {why}");
        }
    }
}

fn merge(into: &mut BTreeMap<String, usize>, from: &BTreeMap<String, usize>) {
    for (k, v) in from {
        *into.entry(k.clone()).or_insert(0) += v;
    }
}

/// Collapse an error message to its leading clause, so a histogram groups
/// by kind rather than by the particular offsets in one file.
fn short(msg: &str) -> String {
    let head = msg.split(&[':', '('][..]).next().unwrap_or(msg).trim();
    head.chars().take(72).collect()
}

fn write_tsv(path: &Path, rows: &[Row]) {
    let Ok(mut f) = fs::File::create(path) else {
        eprintln!("unembed-sweep: could not write {}", path.display());
        return;
    };
    let _ = writeln!(
        f,
        "file\tstate\ttargets\tblocked\treclaim\tin_bytes\tout_bytes\tstill_embedded\trender\tdetail"
    );
    for row in rows {
        let (state, targets, blocked, reclaim, inb, outb, still, render, detail) =
            match &row.outcome {
                Outcome::LoadFailed(e) => ("load-failed", 0, 0, 0, 0, 0, 0, "-", e.clone()),
                Outcome::NothingToDo { blocked } => (
                    "nothing-to-do",
                    0,
                    blocked.values().sum::<usize>(),
                    0,
                    0,
                    0,
                    0,
                    "-",
                    String::new(),
                ),
                Outcome::ApplyRefused(e) => ("apply-refused", 0, 0, 0, 0, 0, 0, "-", e.clone()),
                Outcome::SaveFailed(e) => ("save-failed", 0, 0, 0, 0, 0, 0, "-", e.clone()),
                Outcome::ReopenFailed(e) => ("reopen-failed", 0, 0, 0, 0, 0, 0, "-", e.clone()),
                Outcome::Done {
                    targets,
                    blocked,
                    reclaim,
                    in_bytes,
                    out_bytes,
                    still_embedded,
                    targets_still_embedded: _,
                    render,
                    baseline_render,
                    incremental,
                } => {
                    let (r, d) = match (render, baseline_render) {
                        (Some(Ok(())), _) => ("ok", String::new()),
                        (Some(Err(e)), Some(Err(_))) => ("already-broken", e.clone()),
                        (Some(Err(e)), _) => ("regressed", e.clone()),
                        (None, _) => ("no-pages", String::new()),
                    };
                    (
                        if *incremental {
                            "done-incremental"
                        } else {
                            "done"
                        },
                        *targets,
                        blocked.values().sum::<usize>(),
                        *reclaim,
                        *in_bytes,
                        *out_bytes,
                        *still_embedded,
                        r,
                        d,
                    )
                }
            };
        let _ = writeln!(
            f,
            "{}\t{state}\t{targets}\t{blocked}\t{reclaim}\t{inb}\t{outb}\t{still}\t{render}\t{}",
            row.file,
            detail.replace(['\t', '\n'], " ")
        );
    }
    eprintln!("unembed-sweep: wrote {}", path.display());
}

/// Recursively collect every `*.pdf` under `root`, skipping dot-directories.
/// Same walk as `tools/corpus-report`, so the two sweeps see one corpus.
fn collect_pdfs(root: &Path) -> Result<Vec<(String, PathBuf)>, String> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        let entries = fs::read_dir(&d).map_err(|e| format!("read_dir {}: {e}", d.display()))?;
        for entry in entries {
            let entry = entry.map_err(|e| format!("read_dir entry in {}: {e}", d.display()))?;
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if path.is_dir() {
                if !name.starts_with('.') {
                    stack.push(path);
                }
            } else if name.to_ascii_lowercase().ends_with(".pdf") {
                let rel = path
                    .strip_prefix(root)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .replace('\\', "/");
                out.push((rel, path));
            }
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}
