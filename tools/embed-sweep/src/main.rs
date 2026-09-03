//! # `embed-sweep` — does `not-embedded` reach zero, and does the page still
//! look the same?
//!
//! Pass 67.0 phase E's corpus harness. It runs the **whole** operation over
//! every PDF under the given directories and reports what happened, with the
//! denominators spelled out.
//!
//! ## The three questions this exists to answer
//!
//! Fixtures prove each branch works. They cannot prove the branches cover
//! what real producers emit, and they cannot answer the questions that
//! decide whether the feature solves the operator's actual problem — a book
//! rejected by a print-on-demand service because a font is not embedded.
//!
//! 1. **Does `not-embedded` reach zero?** That is the number the service
//!    checks and the number the operator is trying to drive down. A run that
//!    embedded three of seven fonts has done real work and produced a file
//!    that will still be rejected.
//! 2. **Does the file still open and render?** A file that embeds and then
//!    fails to load is the failure an operator finds rather than a test.
//! 3. **★ Does the page still look the SAME?**
//!
//! ## Why question 3 is answerable here and was not for the mirror sweep
//!
//! `unembed-sweep`'s own docs record that it *cannot* compare rasters:
//! unembedding is **defined** to change how a page looks, so a pixel
//! difference is the expected outcome and a comparison would measure
//! nothing.
//!
//! Embedding is the opposite. Positions come from the PDF's own `/Widths`
//! (§9.6.2.1 Table 111, decision 004 §3.6), which this operation either
//! leaves untouched or writes from the Adobe Core-14 metrics a reader was
//! already applying. And in `--bundled` mode the face embedded is **the same
//! face pdfcer's own renderer was already substituting**. So the raster
//! before and the raster after must be **byte-identical**, and any
//! difference is a real defect in one of three places:
//!
//! - the synthesised `/Widths` disagree with the metrics the renderer used,
//! - the written `/Encoding` `/Differences` maps a code to a different
//!   glyph than §9.6.6's chain did,
//! - or the wrong `/FontFile*` key was chosen and the reader fell back.
//!
//! That is a genuine oracle rather than a smoke test, and it is the single
//! most informative column in the output.
//!
//! ## The two modes, and why both are run
//!
//! | Mode | Donors | What it measures |
//! |---|---|---|
//! | `--font-dir <DIR>` | the operator's own faces | **coverage** — how much of the real corpus a real machine's font folder can actually resolve |
//! | `--bundled` | pdfcer's own standard-14 substitutes | **correctness** — the pixel-identity oracle above |
//!
//! Coverage without correctness is a number nobody should trust; correctness
//! over fourteen faces is not coverage. Both flags may be combined, in which
//! case supplied faces win and the bundled set fills the rest — but the
//! pixel oracle only holds for rows whose donors were *all* bundled, and the
//! report says so.
//!
//! ## What is measured
//!
//! | Column | Meaning |
//! |---|---|
//! | `load` | the corpus file opened at all |
//! | `missing_before` | fonts with no program, from the inventory |
//! | `resolved` | fonts a donor was found for |
//! | `targets` | fonts the plan would actually embed |
//! | `refused` | fonts refused, each with a stated reason |
//! | `added` | uncompressed donor bytes the plan would add |
//! | `applied` / `saved` | the operation and the save succeeded |
//! | `reopen` | ★ the written bytes parse as a `Document` |
//! | `missing_after` | ★ fonts with no program in the OUTPUT, re-measured from a fresh parse |
//! | `render` | ★ page 1 rasterises after the operation |
//! | `identical` | ★ the raster is byte-identical to the baseline |
//!
//! `missing_after` is re-measured from the reopened document rather than
//! taken from the plan, because the plan states an intention and this
//! sweep's job is to check the result.
//!
//! ## Usage
//!
//! ```text
//! cargo run --release -- [--font-dir <DIR>]... [--bundled] [--tsv <path>]
//!                       [--limit N] [--no-write] <corpus-dir> [more ...]
//! ```
//!
//! Exit codes: `0` the sweep completed (whatever the numbers were), `2`
//! usage error, `3` a corpus directory could not be walked. A sweep has no
//! notion of "failing" — an unopenable file is data about the corpus.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::font_embed_missing::{EmbedRequest, FontMatch, SuppliedFont};
use pdfcer_core::fontinfo::{self, Program};
use pdfcer_core::page_tree::pages_in;
use pdfcer_core::writer::SaveOptions;
use pdfcer_render::font::EmbedMatch;
use pdfcer_render::font::program::FontProgram;
use pdfcer_render::{FontData, FontEnvironment, RenderOptions, render_page_with};

/// Font-file extensions the `--font-dir` walk attempts, matching the CLI's
/// own list so the sweep and the shipped command see the same folder.
const FONT_EXTENSIONS: [&str; 7] = ["ttf", "otf", "ttc", "cff", "pfb", "pfa", "otc"];
/// The per-file ceiling, matching `pdfcer`'s.
const MAX_FONT_FILE_BYTES: u64 = 64 * 1024 * 1024;

struct Row {
    file: String,
    outcome: Outcome,
}

/// What happened to one file, as a state machine that stops at the first
/// thing that did not work — so a "render failed" row is never confused
/// with a "never got that far" row.
enum Outcome {
    /// The corpus file would not open. Data about the corpus.
    LoadFailed(String),
    /// Opened, and no font in it is missing a program. The single most
    /// common outcome, and not a failure of anything.
    NothingMissing,
    /// Fonts are missing, but the plan could embed none of them. Carries the
    /// refusal counts, which is the whole point of the row.
    NothingToDo {
        missing: usize,
        blocked: BTreeMap<&'static str, usize>,
    },
    ApplyRefused(String),
    SaveFailed(String),
    /// ★ The written bytes would not parse.
    ReopenFailed(String),
    Done {
        missing_before: usize,
        targets: usize,
        exact: usize,
        substitute: usize,
        blocked: BTreeMap<&'static str, usize>,
        added: u64,
        in_bytes: usize,
        out_bytes: usize,
        /// ★ Re-measured from the REOPENED document, never from the plan.
        missing_after: usize,
        /// `Ok(())` when page 1 rasterised, `None` when there are no pages.
        render: Option<Result<(), String>>,
        /// Whether page 1 rasterised BEFORE the operation. Without this the
        /// `render` column measures the corpus, not the feature — the
        /// veraPDF corpus deliberately contains malformed files that fail
        /// identically either way.
        baseline_render: Option<Result<(), String>>,
        /// ★ The pixel oracle: `Some(true)` when the raster is
        /// byte-identical to the baseline. `None` when it does not apply —
        /// either a render failed, or at least one donor was NOT bundled
        /// (in which case a different face is drawing and a difference is
        /// expected, not a defect).
        identical: Option<bool>,
    },
}

fn main() {
    let mut args = std::env::args().skip(1);
    let mut roots: Vec<PathBuf> = Vec::new();
    let mut font_dirs: Vec<PathBuf> = Vec::new();
    let mut tsv: Option<PathBuf> = None;
    let mut limit: Option<usize> = None;
    let mut write_output = false;
    let mut bundled = false;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--font-dir" => match args.next() {
                Some(p) => font_dirs.push(PathBuf::from(p)),
                None => usage("--font-dir needs a path"),
            },
            "--bundled" => bundled = true,
            "--tsv" => match args.next() {
                Some(p) => tsv = Some(PathBuf::from(p)),
                None => usage("--tsv needs a path"),
            },
            "--limit" => match args.next().and_then(|n| n.parse().ok()) {
                Some(n) => limit = Some(n),
                None => usage("--limit needs a number"),
            },
            "--write" => write_output = true,
            other if other.starts_with("--") => usage(&format!("unknown flag {other}")),
            other => roots.push(PathBuf::from(other)),
        }
    }
    if roots.is_empty() {
        usage("at least one corpus directory is required");
    }
    if font_dirs.is_empty() && !bundled {
        usage("nothing to embed FROM: pass --font-dir <DIR> and/or --bundled");
    }

    let out_dir = std::env::temp_dir().join("pdfcer-embed-sweep");
    if write_output {
        let _ = fs::create_dir_all(&out_dir);
    }

    let (env, registered) = build_font_environment(&font_dirs);
    eprintln!("embed-sweep: {registered} supplied face registration(s), bundled={bundled}");

    let mut files: Vec<(String, PathBuf)> = Vec::new();
    for root in &roots {
        match collect_pdfs(root) {
            Ok(found) => files.extend(found),
            Err(e) => {
                eprintln!("embed-sweep: {e}");
                std::process::exit(3);
            }
        }
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));
    if let Some(n) = limit {
        files.truncate(n);
    }

    eprintln!("embed-sweep: {} file(s)", files.len());
    let mut rows: Vec<Row> = Vec::with_capacity(files.len());
    for (i, (rel, path)) in files.iter().enumerate() {
        if i % 200 == 0 {
            eprintln!("  {i}/{}", files.len());
        }
        // One panicking file must not abandon the sweep; a panic is itself a
        // finding and is recorded rather than taking the run down.
        // `AssertUnwindSafe` because the only shared state across the
        // boundary is a read-only `FontEnvironment`: nothing here mutates it,
        // so a panic cannot leave it half-updated. `FontData` wraps an
        // `Arc<dyn AsRef<[u8]>>`, which the compiler cannot prove immutable.
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            measure(path, &env, bundled, &out_dir, write_output, rel)
        }))
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
    eprintln!("embed-sweep: {msg}");
    eprintln!(
        "usage: embed-sweep [--font-dir <DIR>]... [--bundled] [--tsv <path>] [--limit N] \
[--write] <corpus-dir> [more ...]"
    );
    std::process::exit(2);
}

/// Walk each font directory once, exactly as `pdfcer` does.
fn build_font_environment(dirs: &[PathBuf]) -> (FontEnvironment, usize) {
    let mut env = FontEnvironment::bundled();
    let mut registered = 0usize;
    for dir in dirs {
        let Ok(rd) = fs::read_dir(dir) else { continue };
        let mut paths: Vec<PathBuf> = rd
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| {
                p.is_file()
                    && p.extension()
                        .and_then(|e| e.to_str())
                        .map(str::to_ascii_lowercase)
                        .is_some_and(|e| FONT_EXTENSIONS.contains(&e.as_str()))
            })
            .collect();
        paths.sort();
        for path in paths {
            if fs::metadata(&path).is_ok_and(|m| m.len() > MAX_FONT_FILE_BYTES) {
                continue;
            }
            let Ok(bytes) = fs::read(&path) else { continue };
            let mut names = match FontProgram::parse(&bytes) {
                Ok(p) => p.face_names(),
                Err(_) => continue,
            };
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str())
                && !names.iter().any(|n| n == stem)
            {
                names.push(stem.to_owned());
            }
            let data = FontData::new(bytes);
            for name in &names {
                env.insert_named(name, data.clone());
                registered += 1;
            }
        }
    }
    (env, registered)
}

/// Run the complete round trip on one file.
fn measure(
    path: &Path,
    env: &FontEnvironment,
    bundled: bool,
    out_dir: &Path,
    write_output: bool,
    rel: &str,
) -> Outcome {
    let Ok(bytes) = fs::read(path) else {
        return Outcome::LoadFailed("read failed".to_owned());
    };
    let in_bytes = bytes.len();
    let doc = match Document::from_bytes(bytes) {
        Ok(d) => d,
        Err(e) => return Outcome::LoadFailed(e.to_string()),
    };

    let inventory = fontinfo::inventory(&doc.view());
    let missing_before = inventory
        .fonts
        .iter()
        .filter(|f| matches!(f.program, Program::NotEmbedded))
        .count();
    if missing_before == 0 {
        return Outcome::NothingMissing;
    }

    // ★ The baseline raster, taken before anything changes and with the SAME
    // font environment the "after" render will use. Rendering the two with
    // different environments would make every comparison meaningless.
    // `RenderOptions` is `#[non_exhaustive]`, so it is built through
    // `Default` plus field assignment — the construction shape its own docs
    // prescribe, and what keeps a future field addition source-compatible.
    let mut options = RenderOptions::default();
    options.fonts = env.clone();
    let baseline = match pages_in(&doc.view()) {
        Ok(pages) => pages
            .first()
            .map(|first| render_page_with(&doc, first, 0.5, &options).map_err(|e| e.to_string())),
        Err(e) => Some(Err(format!("page tree: {e}"))),
    };
    let baseline_render = baseline
        .as_ref()
        .map(|r| r.as_ref().map(|_| ()).map_err(Clone::clone));
    let baseline_pixels = baseline
        .as_ref()
        .and_then(|r| r.as_ref().ok().map(hash_page));

    // Resolve a donor for every missing font.
    let mut request = EmbedRequest::all_missing();
    let mut exact = 0usize;
    let mut substitute = 0usize;
    let mut all_bundled = true;
    for record in &inventory.fonts {
        if !matches!(record.program, Program::NotEmbedded) {
            continue;
        }
        let Some(base_font) = record.base_font.as_deref() else {
            continue;
        };
        let Some(donor) = env.resolve_for_embedding(base_font, bundled) else {
            continue;
        };
        let matched = match donor.quality {
            EmbedMatch::Exact => FontMatch::Exact,
            EmbedMatch::Alias => FontMatch::Alias,
            EmbedMatch::Bundled => FontMatch::Bundled,
        };
        if matched != FontMatch::Bundled {
            all_bundled = false;
        }
        request = request.with_font(
            base_font,
            SuppliedFont::new(
                donor.data.bytes().to_vec(),
                donor.face_name.clone(),
                "sweep",
                matched,
            ),
        );
    }

    let mut session = EditSession::new(doc);
    let plan = session.embed_preview(&request);
    let blocked = count_blockers(&plan);
    if plan.targets.is_empty() {
        return Outcome::NothingToDo {
            missing: missing_before,
            blocked,
        };
    }
    for t in &plan.targets {
        if t.matched.is_substitute() {
            substitute += 1;
        } else {
            exact += 1;
        }
    }
    let targets = plan.targets.len();
    let added = plan.bytes_added_uncompressed();

    if let Err(e) = session.embed_fonts(&request) {
        return Outcome::ApplyRefused(e.to_string());
    }
    // An INCREMENTAL save, deliberately, and the opposite choice from the
    // mirror sweep. Unembedding only reclaims bytes under a full rewrite, so
    // that sweep had to use one. Embedding keeps its programs under either
    // mode, and an incremental save is both the shipped default and the
    // stronger claim: the input revision has to survive byte-identical.
    //
    // ★ WITH ONE FALLBACK, and it is a measurement decision rather than a
    // convenience — the mirror of the one `unembed-sweep` makes in the other
    // direction. A document pdfcer RECOVERED (its base cross-reference was
    // invalid) refuses an incremental save by design: appending an update
    // section onto a table nobody could parse would produce a file whose
    // older revision is unreadable. That refusal is not a failure of THIS
    // feature, and treating it as one would hide the questions the sweep
    // exists to answer. Measured: 355 of 4,023 corpus files land here.
    let written = match session.to_incremental_bytes(&SaveOptions::identity()) {
        Ok((bytes, _)) => bytes,
        Err(_) => match session.to_full_bytes(&SaveOptions::default()) {
            Ok((bytes, _)) => bytes,
            Err(e) => return Outcome::SaveFailed(e.to_string()),
        },
    };
    let out_bytes = written.len();
    if write_output {
        let name = rel.replace(['/', '\\'], "_");
        let _ = fs::write(out_dir.join(name), &written);
    }

    // ★ Parsed by a FRESH `Document`. A session that can still answer
    // questions about a file it broke proves nothing.
    let reopened = match Document::from_bytes(written) {
        Ok(d) => d,
        Err(e) => return Outcome::ReopenFailed(e.to_string()),
    };
    let after_inventory = fontinfo::inventory(&reopened.view());
    let missing_after = after_inventory
        .fonts
        .iter()
        .filter(|f| matches!(f.program, Program::NotEmbedded))
        .count();

    let after = match pages_in(&reopened.view()) {
        Ok(pages) => pages.first().map(|first| {
            render_page_with(&reopened, first, 0.5, &options).map_err(|e| e.to_string())
        }),
        Err(e) => Some(Err(format!("page tree: {e}"))),
    };
    let render = after
        .as_ref()
        .map(|r| r.as_ref().map(|_| ()).map_err(Clone::clone));
    // The oracle applies only when every donor was bundled: with a supplied
    // face, a DIFFERENT face is now drawing than the renderer was
    // substituting, so a pixel difference is the expected outcome.
    let identical = if all_bundled {
        match (
            &baseline_pixels,
            after.as_ref().and_then(|r| r.as_ref().ok()),
        ) {
            (Some(before), Some(page)) => Some(*before == hash_page(page)),
            _ => None,
        }
    } else {
        None
    };

    Outcome::Done {
        missing_before,
        targets,
        exact,
        substitute,
        blocked,
        added,
        in_bytes,
        out_bytes,
        missing_after,
        render,
        baseline_render,
        identical,
    }
}

/// A cheap content hash of a rendered page.
///
/// FNV-1a over the raw pixel bytes plus the dimensions. Not cryptographic
/// and does not need to be: it answers "are these two rasters the same",
/// over two images this process produced seconds apart, and a collision
/// would have to be engineered.
fn hash_page(page: &pdfcer_render::RenderedPage) -> (u32, u32, u64) {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in page.pixmap.data() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    (page.pixmap.width(), page.pixmap.height(), h)
}

fn count_blockers(
    plan: &pdfcer_core::font_embed_missing::EmbedPlan,
) -> BTreeMap<&'static str, usize> {
    plan.blocker_counts()
}

fn collect_pdfs(root: &Path) -> Result<Vec<(String, PathBuf)>, String> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let rd = fs::read_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        for entry in rd.flatten() {
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
            } else if p
                .extension()
                .and_then(|e| e.to_str())
                .map(str::to_ascii_lowercase)
                .as_deref()
                == Some("pdf")
            {
                let rel = p
                    .strip_prefix(root)
                    .unwrap_or(&p)
                    .to_string_lossy()
                    .into_owned();
                out.push((rel, p));
            }
        }
    }
    Ok(out)
}

#[allow(clippy::too_many_lines)]
fn report(rows: &[Row]) {
    let mut load_failed = 0usize;
    let mut nothing_missing = 0usize;
    let mut nothing_to_do = 0usize;
    let mut apply_refused = 0usize;
    let mut save_failed = 0usize;
    let mut reopen_failed = 0usize;
    let mut done = 0usize;

    let mut fonts_missing_total = 0usize;
    let mut fonts_embedded_total = 0usize;
    let mut fonts_exact = 0usize;
    let mut fonts_substitute = 0usize;
    let mut bytes_added = 0u64;
    let mut files_reaching_zero = 0usize;
    let mut files_still_missing = 0usize;
    let mut fonts_still_missing = 0usize;
    let mut render_ok = 0usize;
    let mut render_failed = 0usize;
    let mut render_failed_already = 0usize;
    let mut identical_yes = 0usize;
    let mut identical_no = 0usize;
    let mut blockers: BTreeMap<&'static str, usize> = BTreeMap::new();

    for row in rows {
        match &row.outcome {
            Outcome::LoadFailed(_) => load_failed += 1,
            Outcome::NothingMissing => nothing_missing += 1,
            Outcome::NothingToDo { missing, blocked } => {
                nothing_to_do += 1;
                fonts_missing_total += missing;
                fonts_still_missing += missing;
                files_still_missing += 1;
                for (k, v) in blocked {
                    *blockers.entry(k).or_insert(0) += v;
                }
            }
            Outcome::ApplyRefused(_) => apply_refused += 1,
            Outcome::SaveFailed(_) => save_failed += 1,
            Outcome::ReopenFailed(_) => reopen_failed += 1,
            Outcome::Done {
                missing_before,
                targets,
                exact,
                substitute,
                blocked,
                added,
                missing_after,
                render,
                baseline_render,
                identical,
                ..
            } => {
                done += 1;
                fonts_missing_total += missing_before;
                fonts_embedded_total += targets;
                fonts_exact += exact;
                fonts_substitute += substitute;
                bytes_added += added;
                if *missing_after == 0 {
                    files_reaching_zero += 1;
                } else {
                    files_still_missing += 1;
                    fonts_still_missing += missing_after;
                }
                for (k, v) in blocked {
                    *blockers.entry(k).or_insert(0) += v;
                }
                match (render, baseline_render) {
                    (Some(Ok(())), _) => render_ok += 1,
                    // A file that could not render BEFORE either is a
                    // property of the corpus, not of this feature.
                    (Some(Err(_)), Some(Err(_))) => render_failed_already += 1,
                    (Some(Err(_)), _) => render_failed += 1,
                    (None, _) => {}
                }
                match identical {
                    Some(true) => identical_yes += 1,
                    Some(false) => identical_no += 1,
                    None => {}
                }
            }
        }
    }

    println!("=== embed-sweep ===");
    println!("files                       {}", rows.len());
    println!("  load failed               {load_failed}");
    println!("  no font missing           {nothing_missing}");
    println!("  nothing embeddable        {nothing_to_do}");
    println!("  apply refused             {apply_refused}");
    println!("  save failed               {save_failed}");
    println!("  ★ reopen failed           {reopen_failed}   (must be 0)");
    println!("  completed                 {done}");
    println!();
    println!("fonts missing (all files)   {fonts_missing_total}");
    println!("  embedded                  {fonts_embedded_total}");
    println!("    exact-name match        {fonts_exact}");
    println!("    substitute              {fonts_substitute}");
    println!("  ★ still missing after     {fonts_still_missing}");
    println!("bytes added (uncompressed)  {bytes_added}");
    println!();
    println!("★ files reaching not-embedded=0   {files_reaching_zero}");
    println!("  files still missing something   {files_still_missing}");
    println!();
    println!(
        // string-gap-exempt: aligned report column in the sweep summary
        "render after embedding      ok={render_ok} broken={render_failed} \
already-broken={render_failed_already}   (broken must be 0)"
    );
    println!(
        // string-gap-exempt: aligned report column in the sweep summary
        "★ pixel-identical raster    yes={identical_yes} no={identical_no}   (bundled-donor rows \
only; `no` must be 0)"
    );
    println!();
    println!("refusal reasons:");
    let total: usize = blockers.values().sum();
    let mut sorted: Vec<_> = blockers.into_iter().collect();
    sorted.sort_by_key(|(_, v)| std::cmp::Reverse(*v));
    for (k, v) in sorted {
        let pct = if total == 0 {
            0.0
        } else {
            (v as f64) * 100.0 / (total as f64)
        };
        println!("  {k:<28} {v:>7}  {pct:>5.1}%");
    }
}

fn write_tsv(path: &Path, rows: &[Row]) {
    let Ok(mut f) = fs::File::create(path) else {
        eprintln!("embed-sweep: cannot write {}", path.display());
        return;
    };
    let _ = writeln!(
        f,
        "file\tstate\tmissing_before\ttargets\texact\tsubstitute\tmissing_after\tadded\tin\tout\
\trender\tidentical\tdetail"
    );
    for row in rows {
        let line = match &row.outcome {
            Outcome::LoadFailed(e) => format!("load-failed\t\t\t\t\t\t\t\t\t\t\t{e}"),
            Outcome::NothingMissing => "nothing-missing\t0\t0\t0\t0\t0\t0\t\t\t\t\t".to_owned(),
            Outcome::NothingToDo { missing, .. } => {
                format!("nothing-embeddable\t{missing}\t0\t0\t0\t{missing}\t0\t\t\t\t\t")
            }
            Outcome::ApplyRefused(e) => format!("apply-refused\t\t\t\t\t\t\t\t\t\t\t{e}"),
            Outcome::SaveFailed(e) => format!("save-failed\t\t\t\t\t\t\t\t\t\t\t{e}"),
            Outcome::ReopenFailed(e) => format!("reopen-failed\t\t\t\t\t\t\t\t\t\t\t{e}"),
            Outcome::Done {
                missing_before,
                targets,
                exact,
                substitute,
                added,
                in_bytes,
                out_bytes,
                missing_after,
                render,
                identical,
                ..
            } => format!(
                "done\t{missing_before}\t{targets}\t{exact}\t{substitute}\t{missing_after}\t\
{added}\t{in_bytes}\t{out_bytes}\t{}\t{}\t",
                match render {
                    Some(Ok(())) => "ok",
                    Some(Err(_)) => "failed",
                    None => "no-pages",
                },
                match identical {
                    Some(true) => "yes",
                    Some(false) => "NO",
                    None => "n/a",
                },
            ),
        };
        let _ = writeln!(f, "{}\t{line}", row.file);
    }
    eprintln!("embed-sweep: wrote {}", path.display());
}
