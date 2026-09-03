//! # `render-profile` — the standing answer to "where does the time go?"
//!
//! Loads a PDF, renders one page at a series of scales, and reports the
//! load/parse/render split, the scaling curve, and what the page's
//! content actually looks like to the renderer.
//!
//! ## Why this is a committed tool and not a scratch file
//!
//! On 2026-08-07 three throwaway probes were written into `interpret.rs`
//! and deleted within hours. **Two produced figures wrong by two orders
//! of magnitude, and both were believed and acted on:**
//!
//! - `Mask::new` reported as 10.1 s of an 18 s render; it is 1.02 s. The
//!   figure came from an ablation that skipped `intersect_clip`
//!   entirely, which also makes every `q` cheap and lets tiny-skia skip
//!   mask sampling — construction plus use, attributed to construction.
//! - Mean clip bbox reported as **0.663% of the page**; it is **66.36%**
//!   — a fraction printed as a percent. That 100× error became the
//!   stated premise of a follow-on optimization, and is still written
//!   into `intersect_clip`'s doc comment as "clips in real drawings are
//!   SMALL relative to the paper".
//!
//! Neither survived contact with a second measurement. Both survived for
//! hours because **there was no second measurement to make** — the probe
//! that produced them no longer existed. A harness that must be
//! rewritten each session is one nobody runs, and an unrepeatable number
//! ages into a fact.
//!
//! ## Reading the output
//!
//! **The scaling curve is the diagnostic**, not any single row. A cost
//! that is quadratic in area rises by the same factor at every doubling;
//! one that jumps at a single step is a cache boundary. On the reference
//! CAD sheet the steps ran 3.23× / 3.14× / 14.1× — three smooth steps
//! then a cliff, which identified a working set crossing L3 rather than
//! an algorithmic term. **A single before/after pair could not have told
//! those apart.**
//!
//! `parse` is the content-stream interpretation *plus* rasterization,
//! because they are not separable from outside: the interpreter paints
//! as it walks. `load` is `Document::from_bytes` — the object graph and
//! xref only. When `load` is a rounding error, optimizing the reader is
//! wasted effort, and on the reference sheet it is ~0.005%.
//!
//! ## `--ablate` — the FLOOR, and why a delta is never a value
//!
//! Ablation switches a cost centre off and re-renders. **The difference
//! is an upper bound on that centre's cost, never its value**, because
//! removing one thing can remove others with it — and that is not a
//! caveat, it is the day's worst error: `Mask::new` was reported at
//! 10.1 s because skipping clip construction *also* removed clip
//! sampling from every later paint and the `Arc` clone from every `q`.
//!
//! So this mode never prints a bare delta. Every row carries what its
//! ablation additionally suppressed, and rows with no confound are
//! marked as attributable — currently only `clip-sample`, which is
//! exactly why it exists as a switch separate from `clip-build`.
//!
//! **The floor** is every centre off: content-stream interpretation and
//! path construction, the cost of *walking* the page. Nothing done to
//! the rasterizer can go below it. Two things make it the number to get
//! first:
//!
//! - It bounds the win. An optimization targeting a centre worth less
//!   than `total − floor` cannot deliver more than that, whatever it
//!   does.
//! - **It is scale-flat if it is per-operation.** Run `--ablate-sweep`
//!   across scales: a floor that barely moves between 0.25× and 2× is
//!   fixed per-operation cost, which tiling and low-resolution proxies
//!   *cannot* reduce — they render fewer pixels, not fewer operators.
//!   That is the standing answer to "would tiling help", and it is
//!   measured here rather than argued.
//!
//! ## Usage
//!
//! ```text
//! cargo run --release -- <file.pdf> [--page N] [--scales 0.25,0.5,1,2] [--repeat N]
//! cargo run --release -- <file.pdf> --ablate-sweep [--scales …]
//! cargo run --release -- <file.pdf> --ablate clip-build,paint
//! ```
//!
//! Exits 2 on a usage or load error, 0 otherwise. It reports; it does
//! not judge, and has no pass/fail threshold to drift out of date.

use std::time::Instant;

use pdfcer_core::document::Document;
use pdfcer_core::page_tree;
use pdfcer_core::view::DocumentView;
use pdfcer_render::profile::Ablation;
use pdfcer_render::{RenderOptions, profile, render_page_with_view};

/// The ablations a sweep runs, in the order the table prints them.
///
/// `clip-sample` sits above `clip-build` deliberately: it is the only
/// row whose delta is attributable, so a reader meets an honest number
/// before meeting a confounded one.
const SWEEP: &[Ablation] = &[
    Ablation::NONE,
    Ablation {
        clip_sample: true,
        clip_build: false,
        paint: false,
    },
    Ablation {
        clip_build: true,
        clip_sample: false,
        paint: false,
    },
    Ablation {
        paint: true,
        clip_build: false,
        clip_sample: false,
    },
    Ablation::ALL,
];

fn main() -> std::process::ExitCode {
    let mut args = std::env::args().skip(1);
    let mut path: Option<String> = None;
    let mut page_index: usize = 0;
    let mut scales: Vec<f32> = vec![0.25, 0.5, 1.0, 2.0];
    let mut repeat: usize = 1;
    let mut ablate: Option<Ablation> = None;
    let mut ablate_sweep = false;

    while let Some(a) = args.next() {
        match a.as_str() {
            "--ablate" => {
                let Some(spec) = args.next() else {
                    eprintln!("--ablate needs a set: clip-build,clip-sample,paint,all,none");
                    return std::process::ExitCode::from(2);
                };
                match Ablation::parse(&spec) {
                    Ok(a) => ablate = Some(a),
                    Err(bad) => {
                        // Rejected, never ignored: an ignored typo runs an
                        // un-ablated render and reports a zero delta, which
                        // reads as "this centre is free".
                        eprintln!(
                            "unknown ablation '{bad}' — expected any of \
                             clip-build, clip-sample, paint, all, none"
                        );
                        return std::process::ExitCode::from(2);
                    }
                }
            }
            "--ablate-sweep" => ablate_sweep = true,
            "--page" => {
                page_index = args.next().and_then(|v| v.parse().ok()).unwrap_or(0);
            }
            "--scales" => {
                if let Some(v) = args.next() {
                    scales = v.split(',').filter_map(|s| s.trim().parse().ok()).collect();
                }
            }
            "--repeat" => {
                repeat = args.next().and_then(|v| v.parse().ok()).unwrap_or(1).max(1);
            }
            "-h" | "--help" => {
                eprintln!(
                    "render-profile <file.pdf> [--page N] [--scales 0.25,0.5,1,2] [--repeat N]\n\
                                               [--ablate SET | --ablate-sweep]\n\
                     \n\
                     SET is a comma list of: clip-build, clip-sample, paint, all, none\n\
                     --ablate-sweep runs each in turn and reports the FLOOR."
                );
                return std::process::ExitCode::SUCCESS;
            }
            other => path = Some(other.to_owned()),
        }
    }

    let Some(path) = path else {
        eprintln!("usage: render-profile <file.pdf> [--page N] [--scales …] [--repeat N]");
        return std::process::ExitCode::from(2);
    };

    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("cannot read {path}: {e}");
            return std::process::ExitCode::from(2);
        }
    };
    let input_len = bytes.len();

    let t = Instant::now();
    let doc = match Document::from_bytes(bytes) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("cannot load {path}: {e}");
            return std::process::ExitCode::from(2);
        }
    };
    let load = t.elapsed();

    let pages = match page_tree::pages(&doc) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("cannot read page tree: {e}");
            return std::process::ExitCode::from(2);
        }
    };
    let Some(page) = pages.get(page_index) else {
        eprintln!("page {page_index} out of range ({} pages)", pages.len());
        return std::process::ExitCode::from(2);
    };

    let view = DocumentView::new(&doc, doc.bytes(), doc.version());
    let opts = RenderOptions::default();

    println!("file      : {path}");
    println!("bytes     : {input_len}");
    println!("pages     : {}, profiling page {page_index}", pages.len());
    println!(
        "load      : {:.3} ms  (object graph + xref only)",
        load.as_secs_f64() * 1e3
    );

    // A single --ablate applies to the scale table below. Say so before
    // the numbers, not after: a reader who scrolls to the table must not
    // be able to mistake an ablated row for a real render.
    if let Some(a) = ablate {
        profile::set_ablation(a);
        println!();
        println!(
            "ABLATED   : {}  <-- THE NUMBERS BELOW ARE NOT A REAL RENDER",
            a.label()
        );
        if a.output_is_wrong() {
            println!(
                "            the rendered picture is WRONG by construction; do not screenshot it"
            );
        }
        let confounds = a.confounds();
        if confounds.is_empty() {
            println!("            attributable: no other cost centre changes with this");
        } else {
            println!("            ALSO suppresses:");
            for c in confounds {
                println!("              - {c}");
            }
            println!(
                "            so any difference from a baseline is an UPPER BOUND on this\n            \
                 centre's cost, never its value (R164)"
            );
        }
    }
    println!();
    println!(
        "{:>7}  {:>12}  {:>10}  {:>8}  {:>9}",
        "scale", "pixels", "render", "step", "per Mpx"
    );

    let mut prev: Option<(f64, f64)> = None;
    for &scale in &scales {
        let mut best = f64::MAX;
        let mut px = 0u64;
        for i in 0..repeat {
            // Reset before EVERY repeat, not once per scale.
            //
            // Counters accumulate, so resetting once per scale reported
            // `repeat`× the true counts — 445,551 paints and 72,384 clips
            // at `--repeat 3` instead of 148,517 and 24,128. **Under the
            // tool's own recommended setting**, since `--repeat 1` is
            // warned against for cold-start inflation: the advice for
            // accurate timings silently corrupted the content block.
            //
            // Derived percentages survived it (numerator and denominator
            // both scaled), which is exactly why it was not obvious — the
            // wrong numbers sat beside right ones.
            //
            // Resetting per repeat leaves the counters describing the LAST
            // render, which is one render, which is what the block claims
            // to describe.
            let _ = i;
            profile::reset();
            let t = Instant::now();
            match render_page_with_view(&view, page, scale, &opts) {
                Ok(r) => {
                    px = u64::from(r.pixmap.width()) * u64::from(r.pixmap.height());
                    best = best.min(t.elapsed().as_secs_f64());
                }
                Err(e) => {
                    eprintln!("render at {scale}x failed: {e}");
                    return std::process::ExitCode::from(2);
                }
            }
        }
        let mpx = px as f64 / 1e6;
        // The step ratio is the diagnostic — see the module docs.
        let step = match prev {
            Some((_, pt)) => format!("{:.2}x", best / pt),
            None => "—".to_owned(),
        };
        println!(
            "{scale:>7}  {px:>12}  {:>9.2}s  {step:>8}  {:>8.2}s",
            best,
            if mpx > 0.0 { best / mpx } else { 0.0 }
        );
        prev = Some((mpx, best));
    }

    // Counters come from the LAST scale rendered. They are geometry and
    // counts, not timings, so they do not vary with scale except where
    // device-space bounds clamp to the page.
    let c = profile::snapshot();
    println!();
    println!("content (at {}x):", scales.last().copied().unwrap_or(1.0));
    println!("  paints            : {}", c.paints);
    println!("    unclipped       : {}", c.paints_unclipped);
    println!(
        "    bbox-cullable   : {} ({:.2}% of clipped)",
        c.paints_cullable,
        c.cullable_pct()
    );
    println!("  clip operations   : {}", c.clips);
    {
        // Hits + misses is the application count, so the denominator is
        // stated rather than left to be inferred (hard rule 10).
        let served = c.clip_cache_hits + c.clip_cache_misses;
        if served > 0 {
            #[expect(
                clippy::cast_precision_loss,
                reason = "counts fit f64 exactly far beyond any real page"
            )]
            let pct = c.clip_cache_hits as f64 * 100.0 / served as f64;
            println!(
                "    mask cache      : {} hits + {} built = {} applications ({:.2}% served)",
                c.clip_cache_hits, c.clip_cache_misses, served, pct
            );
        }
    }
    println!(
        "    mean bbox       : {:.2}% of page (individual), {:.2}% (accumulated)",
        c.mean_clip_indiv_pct(),
        c.mean_clip_accum_pct()
    );
    if c.clips > 0 && c.mean_clip_indiv_pct() > 25.0 {
        println!();
        println!(
            "  NOTE: clips cover a large share of the page. Optimizations premised on\n  \
             clips being small relative to the paper do not apply to this file."
        );
    }

    if c.clips > 0 && c.clip_distinct > 0 {
        report_clip_reuse(&c);
    }

    if c.clips > 0 && c.clip_phase_ns() > 0 {
        report_clip_phases(&c);
    }

    if ablate_sweep {
        run_ablation_sweep(&view, page, &scales, repeat, &opts);
    }

    // Leave the renderer un-ablated for anything that runs after us.
    profile::set_ablation(Ablation::NONE);
    std::process::ExitCode::SUCCESS
}

/// Time one render, returning seconds (best of `repeat`).
fn time_render(
    view: &DocumentView<'_>,
    page: &page_tree::Page,
    scale: f32,
    repeat: usize,
    opts: &RenderOptions,
) -> Option<f64> {
    let mut best = f64::MAX;
    for _ in 0..repeat {
        let t = Instant::now();
        render_page_with_view(view, page, scale, opts).ok()?;
        best = best.min(t.elapsed().as_secs_f64());
    }
    Some(best)
}

/// Run every ablation at every scale and report the floor.
///
/// Prints deltas only as **upper bounds**, each with what its ablation
/// additionally suppressed — see the module docs for the 10.1 s error
/// this shape exists to prevent.
fn run_ablation_sweep(
    view: &DocumentView<'_>,
    page: &page_tree::Page,
    scales: &[f32],
    repeat: usize,
    opts: &RenderOptions,
) {
    println!();
    println!("ablation sweep");
    println!(
        "  Every ablated render draws a WRONG PICTURE by construction. These are\n  \
         measurements, not output — do not screenshot one."
    );
    if repeat < 2 {
        // Cold-start noise lands ENTIRELY in the delta. Measured on the
        // reference sheet: clip-build read 1.17s at --repeat 1 and 0.74s
        // at --repeat 3 — a 58% inflation of the row, and therefore of
        // every difference taken from it. A single run is fine for the
        // scale curve above, where the shape survives; it is not fine
        // here, where the whole output is differences.
        println!();
        println!(
            "  WARNING: --repeat 1. Cold-start cost falls entirely into the deltas below.\n  \
             On the reference sheet clip-build read 1.17s at --repeat 1 and 0.74s at\n  \
             --repeat 3. Use --repeat 3 or more before quoting anything from this table."
        );
    }
    println!();

    print!("{:>7}", "scale");
    for a in SWEEP {
        print!("  {:>13}", a.label());
    }
    println!();

    // rows[ablation_index] = times across scales, for the floor analysis.
    let mut floor_times: Vec<f64> = Vec::new();
    let mut base_times: Vec<f64> = Vec::new();
    let mut last_row: Vec<f64> = Vec::new();

    for &scale in scales {
        print!("{scale:>7}");
        let mut row = Vec::new();
        for a in SWEEP {
            profile::set_ablation(*a);
            match time_render(view, page, scale, repeat, opts) {
                Some(s) => {
                    print!("  {s:>12.2}s");
                    row.push(s);
                }
                None => {
                    print!("  {:>13}", "err");
                    row.push(f64::NAN);
                }
            }
        }
        println!();
        if let (Some(&b), Some(&f)) = (row.first(), row.last()) {
            base_times.push(b);
            floor_times.push(f);
        }
        last_row = row;
    }
    profile::set_ablation(Ablation::NONE);

    // --- The floor, and what its flatness means -------------------------
    if let (Some(&lo), Some(&hi)) = (
        floor_times.iter().min_by(|a, b| a.total_cmp(b)),
        floor_times.iter().max_by(|a, b| a.total_cmp(b)),
    ) && lo > 0.0
    {
        let spread = hi / lo;
        let px_span = match (scales.first(), scales.last()) {
            (Some(&a), Some(&b)) if a > 0.0 => f64::from((b / a) * (b / a)),
            _ => 1.0,
        };
        println!();
        println!(
            "FLOOR: {lo:.2}s .. {hi:.2}s  ({spread:.2}x spread while pixels vary {px_span:.0}x)"
        );
        println!(
            "  Content-stream interpretation and path construction only. No change to\n  \
             the rasterizer can go below this without changing the interpreter."
        );
        // The tiling question, settled by measurement rather than argument.
        if spread < px_span / 4.0 {
            println!();
            println!(
                "  => The floor is SCALE-FLAT: it is PER-OPERATION cost, not per-pixel.\n     \
                 Tiling and low-resolution proxies render fewer PIXELS, not fewer\n     \
                 OPERATORS, so they cannot reduce it. A proxy at the smallest scale\n     \
                 above still costs at least the floor."
            );
        } else {
            println!();
            println!(
                "  => The floor SCALES with area, so it is not a per-operation term.\n     \
                 Rendering less area would reduce it proportionally."
            );
        }
        if let Some(&base) = base_times.last() {
            println!();
            println!(
                "  At the largest scale the floor is {:.1}% of the un-ablated render.",
                hi * 100.0 / base
            );
        }
    }

    // --- Per-ablation detail, deltas as upper bounds only ---------------
    println!();
    println!("what each ablation suppressed, at the largest scale");
    let base = last_row.first().copied().unwrap_or(f64::NAN);
    for (a, &t) in SWEEP.iter().zip(last_row.iter()) {
        if a.is_none() {
            continue;
        }
        println!();
        println!("  {} — {:.2}s", a.label(), t);
        // NEVER a bare delta. The word "at most" is load-bearing.
        //
        // A delta at or below noise is a FINDING, not a broken row: it
        // says the centre is not resolvable at this sample size, which
        // is the honest reading of a negative number. Printing
        // "removes AT MOST -0.01s" instead reads as an error and buries
        // the result — measured on the reference sheet, where clip
        // sampling came out free.
        let delta = base - t;
        let noise = base * 0.02;
        if delta <= noise {
            println!(
                "    NOT RESOLVABLE at this sample size (delta {delta:+.2}s on a {base:.2}s\n    \
                 baseline, at or below noise) — this centre is too cheap to measure here,\n    \
                 which is itself the finding. Raise --repeat to tighten the bound."
            );
        } else {
            println!(
                "    removes AT MOST {delta:.2}s of the {base:.2}s baseline — an upper bound, not a value"
            );
        }
        let confounds = a.confounds();
        if confounds.is_empty() {
            println!("    attributable: no other cost centre changes with it");
        } else {
            println!("    ALSO SUPPRESSES (why the figure is only a bound):");
            for c in confounds {
                println!("      - {c}");
            }
        }
    }
    println!();
    println!(
        "  A delta is what STOPPED HAPPENING, which is not the same as what the named\n  \
         centre costs. Reading one as the other reported Mask::new at 10.1s when it is\n  \
         1.02s (R164). Only rows marked 'attributable' support that reading."
    );
}

/// Report the three timed clip phases and the per-clip distribution.
///
/// # Why a distribution and not only a mean
///
/// 24,128 clips at a 350 µs mean is consistent with two completely
/// different worlds: a uniform population, or ~24,000 cheap clips plus a
/// few hundred catastrophic ones. **They need different fixes** — a tail
/// is attacked by finding what makes those clips special, a uniform cost
/// by changing the representation for every clip — and a mean cannot
/// distinguish them. Printing only the mean would hide the question.
///
/// # Why these numbers are timed rather than ablated
///
/// An ablation says what stops happening when a phase is removed, which
/// removes other things with it: an upper bound (R164). These are direct
/// timings — nothing is removed, so nothing is confounded, and the three
/// phases sum to a checkable total.
/// Report clip-path repetition — the census that decides whether a mask
/// cache is worth building.
///
/// Prints the working-set size **beside** the hit rate, because they
/// decide different things and either alone is misleading: a 95% hit
/// rate over 20,000 distinct page-sized masks is 20 GB and infeasible,
/// while a 40% hit rate over 200 masks is cheap and worth having.
fn report_clip_reuse(c: &pdfcer_render::profile::Counters) {
    use pdfcer_render::profile::CLIP_REUSE_EDGES;
    println!();
    println!("clip reuse (build key = path geometry + fill rule + CTM + mask size):");
    // Hard rule 10: the totals and their derived per-item form on one
    // line, so a contradiction between them is visible where it is
    // written rather than 200 lines away.
    println!(
        "  {} applications over {} distinct paths = {:.2} per path",
        c.clips,
        c.clip_distinct,
        c.clip_applications_per_distinct()
    );
    println!(
        "  repeats           : {} ({:.2}% of applications — the CEILING on any cache)",
        c.clip_repeats,
        c.clip_repeat_pct()
    );
    let mb = c.clip_distinct_mask_bytes as f64 / (1024.0 * 1024.0);
    println!(
        "  working set       : {:.1} MiB if every distinct mask were cached",
        mb
    );
    // The number that decides the FORM of a cache: whether a hit can
    // share an Arc (free) or must copy a page-sized mask before the
    // multiply mutates it (saves fill_path, pays a memcpy).
    let full_repeat_pct = if c.clips == 0 {
        0.0
    } else {
        c.clip_full_repeats as f64 * 100.0 / c.clips as f64
    };
    println!(
        "  final-mask reuse  : {} distinct (path, incoming clip) pairs, {:.2}% repeat",
        c.clip_full_distinct, full_repeat_pct
    );

    // How concentrated the reuse is decides how SMALL a bounded cache
    // can be, which the histogram's unbounded last bucket cannot say.
    let top: Vec<u64> = c
        .clip_top_counts
        .iter()
        .copied()
        .filter(|&n| n > 0)
        .collect();
    if !top.is_empty() && c.clips > 0 {
        let mut cum = 0u64;
        print!("  concentration     :");
        for (i, &n) in top.iter().enumerate() {
            cum += n;
            print!(" top-{}={:.1}%", i + 1, cum as f64 * 100.0 / c.clips as f64);
        }
        println!();
    }

    println!("  distinct paths by how many times each is applied:");
    for (i, &n) in c.clip_reuse_hist.iter().enumerate() {
        if n == 0 {
            continue;
        }
        let lo = CLIP_REUSE_EDGES[i];
        let label = match CLIP_REUSE_EDGES.get(i + 1) {
            Some(&hi) if hi == lo + 1 => format!("{lo}"),
            Some(&hi) => format!("{lo}-{}", hi - 1),
            None => format!("{lo}+"),
        };
        println!(
            "    applied {label:>6}x : {n} paths ({:.1}%)",
            n as f64 * 100.0 / c.clip_distinct as f64
        );
    }

    // The verdict, stated by the tool rather than left to the reader —
    // two optimizations have already been scoped on unmeasured clip
    // premises and killed once measured.
    println!();
    if c.clip_repeat_pct() < 5.0 {
        println!(
            "  VERDICT: clip paths are essentially all unique. A mask cache cannot\n  \
             serve {:.2}% of applications and is not worth building for this file.",
            c.clip_repeat_pct()
        );
    } else if mb > 512.0 {
        println!(
            "  VERDICT: {:.2}% of applications repeat, but caching every distinct mask\n  \
             costs {mb:.1} MiB. A bounded cache is the only viable form; measure the\n  \
             hit rate under that bound before building.",
            c.clip_repeat_pct()
        );
    } else {
        println!(
            "  VERDICT: {:.2}% of applications repeat over a {mb:.1} MiB working set.\n  \
             A mask cache is worth costing out.",
            c.clip_repeat_pct()
        );
    }
}

fn report_clip_phases(c: &pdfcer_render::profile::Counters) {
    use pdfcer_render::profile::{CLIP_BUCKET_EDGES_US, CLIP_BUCKETS};

    let ns = |v: u64| v as f64 / 1e9;
    let total = c.clip_phase_ns();
    let per = |v: u64| {
        if c.clips == 0 {
            0.0
        } else {
            v as f64 / c.clips as f64 / 1000.0
        }
    };
    let pct = |v: u64| {
        if total == 0 {
            0.0
        } else {
            v as f64 * 100.0 / total as f64
        }
    };

    println!();
    println!(
        "clip construction, timed per phase (one render, {} clips):",
        c.clips
    );
    println!(
        "    {:<14} {:>9} {:>9} {:>8}",
        "phase", "total", "per clip", "share"
    );
    for (name, v) in [
        ("Mask::new", c.clip_new_ns),
        ("fill_path", c.clip_fill_ns),
        ("multiply", c.clip_mul_ns),
    ] {
        println!(
            "    {name:<14} {:>8.2}s {:>8.1}us {:>7.1}%",
            ns(v),
            per(v),
            pct(v)
        );
    }
    println!(
        "    {:<14} {:>8.2}s {:>8.1}us",
        "= sum",
        ns(total),
        per(total)
    );
    println!("    (timed directly, not ablated: nothing is removed, so nothing is confounded)");

    println!();
    println!("per-clip distribution:");
    let n: u64 = c.clip_hist.iter().sum();
    for i in 0..CLIP_BUCKETS {
        let count = c.clip_hist[i];
        if count == 0 {
            continue;
        }
        let label = if i == 0 {
            format!("<{}us", CLIP_BUCKET_EDGES_US[0])
        } else if i == CLIP_BUCKETS - 1 {
            format!(">={}us", CLIP_BUCKET_EDGES_US[CLIP_BUCKETS - 2])
        } else {
            format!(
                "{}-{}us",
                CLIP_BUCKET_EDGES_US[i - 1],
                CLIP_BUCKET_EDGES_US[i]
            )
        };
        let share = if n == 0 {
            0.0
        } else {
            count as f64 * 100.0 / n as f64
        };
        let bar = "#".repeat(((share / 2.0).round() as usize).min(50));
        println!("    {label:>12}  {count:>7}  {share:>5.1}%  {bar}");
    }
    for p in [50.0, 90.0, 99.0] {
        if let Some(v) = c.clip_percentile_us(p) {
            let s = if v == u64::MAX {
                format!(">={}us", CLIP_BUCKET_EDGES_US[CLIP_BUCKETS - 2])
            } else {
                format!("<{v}us")
            };
            println!("    p{p:<3.0}          {s}");
        }
    }
    println!(
        "    (bucket upper edges; a histogram cannot give an exact percentile and\n     \
         interpolating inside a bucket would invent precision the data lacks)"
    );
}
