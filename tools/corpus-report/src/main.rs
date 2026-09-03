//! # corpus-report — measure pdfcer against a real-world PDF corpus
//!
//! Pass 1.1's first deliverable (docs/ROADMAP.md): pdfcer's Pass 1
//! parser + renderer were proven only against synthetic fixtures. This
//! harness walks a corpus directory of rights-cleared PDFs
//! (docs/LEGAL.md §5 — veraPDF corpus, PDF Association PDF 2.0
//! examples, fetched by `fixtures/fetch-corpora.sh`) and classifies
//! every file by how far the pdfcer pipeline gets with it.
//!
//! ## Pipeline measured, per file
//!
//! 1. `Document::from_bytes` — the strict loader (classic xref tables,
//!    cross-reference streams, object streams, hybrid-reference files;
//!    encrypted documents are a clean refusal).
//! 2. If loaded: `page_tree::pages` — the full §7.7.3 tree walk with
//!    inheritance (strict-refuses a missing required `/Resources`).
//! 3. If pages exist: `render_page(page 1, scale 0.5)` with the
//!    bundled substitute fonts (`RenderOptions::default()`), and the
//!    render's honesty [`Diagnostics`] are accumulated.
//!
//! ## Classification (one category per file)
//!
//! | Category | Meaning |
//! |---|---|
//! | `Ok` | Loaded, tree walked, page 1 rendered (or zero pages — a well-formed empty tree; no render attempted). Diagnostics counted. |
//! | `RefusedEncrypted` | The deliberate `XrefErrorKind::EncryptionUnsupported` refusal (§7.6) — pdfcer has no security handler yet. |
//! | `LoadError` | Any other `DocError` (detail = its `Display`, truncated). |
//! | `MissingResources` | `PageTreeError::MissingRequired("Resources")` — the Pass 1.1 tolerance question's headline number. |
//! | `OtherPageTreeError` | Any other `PageTreeError`. |
//! | `RenderError` | `render_page` returned `RenderError`. |
//! | `Timeout` | The file exceeded the per-file wall-clock budget ([`FILE_BUDGET`]) — recorded and skipped, never blocks the run. |
//! | `Panic` | Any stage panicked (caught via `catch_unwind`). **A panic here is a pdfcer BUG** — the fuzzers found none, but corpus files are structured differently; the file name is reported prominently. |
//!
//! ## Timeout mechanics (and the one deliberate leak)
//!
//! Rust cannot kill a thread, so each file is measured on its own
//! spawned worker thread and the supervisor waits with
//! `recv_timeout(FILE_BUDGET)`. On timeout the worker is *abandoned*
//! (it keeps running detached until process exit; its late result is
//! dropped because the channel receiver is gone). That is an accepted
//! cost for a measurement tool — a hung worker must never stall the
//! other ~2900 files — and the `Timeout` count in the summary is the
//! exact number of leaked workers, so runaway leakage is visible.
//!
//! ## Panic capture
//!
//! The worker wraps the whole pipeline in
//! `catch_unwind(AssertUnwindSafe(..))` and a process-wide silent
//! panic hook keeps the default backtrace spew out of the report
//! (the payload message is preserved in the `Panic` detail instead).
//! `AssertUnwindSafe` is sound here: the closure owns all its state
//! (the file's bytes) and nothing crosses the boundary afterward.
//!
//! ## Output
//!
//! - **stdout**: per-corpus summary table — count + percentage per
//!   category (sorted by count, then name, so output is deterministic),
//!   aggregate render diagnostics (glyphs substituted / notdef, fonts
//!   unsupported, unknown + deferred ops, structural tolerations), the
//!   most common normalized error kinds per error category (digit runs
//!   collapsed to `#` so byte offsets don't fragment the grouping), and
//!   a prominent list of any panicking files.
//! - **TSV**: `"<corpus-dir>-report.tsv"` written NEXT TO the corpus
//!   directory (inside the gitignored `fixtures/external/` when run on
//!   the fetched corpora): one row per file —
//!   `relative/path<TAB>Category<TAB>detail` — for later analysis.
//! - **stderr**: coarse progress (every 100 files), so a long run is
//!   visibly alive without polluting the captured stdout report.
//!
//! ## Determinism
//!
//! Files are collected recursively (skipping dot-directories like
//! `.git`), matched case-insensitively on `.pdf`, and sorted by their
//! forward-slash relative path before processing — the same corpus
//! checkout always yields byte-identical stdout and TSV.
//!
//! ## Exit codes
//!
//! `0` = measurement ran (whatever it found — errors in *corpus files*
//! are data, not tool failures); `1` = at least one PANIC was found
//! (a pdfcer bug worth failing loudly for); `2` = usage error; `3` = a
//! corpus directory could not be walked or the TSV could not be
//! written.

use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::mpsc;
use std::time::Duration;

use pdfcer_core::document::{DocError, Document};
use pdfcer_core::page_tree::{self, PageTreeError};
use pdfcer_core::xref::XrefErrorKind;
use pdfcer_render::render_page;

/// Per-file wall-clock budget. A file that exceeds it is recorded as
/// `Timeout` and its worker thread abandoned (module docs).
const FILE_BUDGET: Duration = Duration::from_secs(10);

/// Maximum characters kept of any error `Display` in the TSV detail
/// column — enough to identify the failure, short enough to keep the
/// TSV greppable.
const DETAIL_MAX: usize = 200;

/// How many distinct normalized error kinds to print per category in
/// the stdout summary.
const TOP_KINDS: usize = 10;

/// The measurement outcome categories (module-docs table). Ordering of
/// the derive is irrelevant — summary rows sort by count.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Category {
    Ok,
    RefusedEncrypted,
    LoadError,
    MissingResources,
    OtherPageTreeError,
    RenderError,
    Timeout,
    Panic,
}

impl Category {
    /// Stable name used in both the summary table and the TSV.
    const fn name(self) -> &'static str {
        match self {
            Self::Ok => "Ok",
            Self::RefusedEncrypted => "RefusedEncrypted",
            Self::LoadError => "LoadError",
            Self::MissingResources => "MissingResources",
            Self::OtherPageTreeError => "OtherPageTreeError",
            Self::RenderError => "RenderError",
            Self::Timeout => "Timeout",
            Self::Panic => "Panic",
        }
    }
}

/// Render-diagnostics totals accumulated across every `Ok` file (the
/// headline counters from `pdfcer_render::Diagnostics`, plus the
/// structural ones — cheap to carry, useful in the report).
#[derive(Debug, Default, Clone, Copy)]
struct DiagTotals {
    glyphs_substituted: usize,
    glyphs_notdef: usize,
    fonts_unsupported: usize,
    unknown_ops: usize,
    deferred_ops: usize,
    tolerated: usize,
    images_rendered: usize,
    images_unsupported: usize,
    forms_rendered: usize,
    xobject_depth_overflows: usize,
    // --- Pass 2.1 image-codec counters (decision 005 §6.4) ---------
    /// Images refused because the CODEC is unimplemented, as opposed to
    /// the data being broken. This is the number Pass 2.1 exists to
    /// move: DCTDecode was 82% of the measured gap.
    images_codec_unsupported: usize,
    /// Sum of `codec_feature_unsupported`. The per-name breakdown lives
    /// in `Diagnostics`; a corpus roll-up only needs the magnitude.
    codec_features_unsupported: usize,
    codec_geometry_mismatch: usize,
    dct_cmyk_images: usize,
    /// Pass 2.3: JPX images whose colour channels arrived preblended
    /// with a backdrop (`/SMaskInData 2`, Table 89). Drawn from the
    /// preblended channels and named; the `Matte` un-premultiplication
    /// needs the transparency model.
    jpx_smask_in_data_preblended: usize,
    lzw_framing_anomalies: usize,
}

/// Document-level annotation census (Pass 6.0 acceptance criterion 1,
/// docs/decisions/008): re-measures §1.2's numbers with **pdfcer's own
/// machinery** so the Pass gate has a pinned baseline whose denominator
/// pdfcer actually produced (W16). Computed over **all** pages
/// ([`pdfcer_core::annot::page_annotations`]), not just the rendered page
/// 1, because an annotation on page 40 counts as much as one on page 1.
///
/// The `with_appearance` count is pdfcer's **usable-appearance** count
/// (a resolvable `/AP` `/N` stream, model `Appearance::Normal`) — which is
/// a stronger, more meaningful predicate than pypdf's raw `/AP`-key
/// presence, and a material gap between the two is itself the finding the
/// acceptance criterion asks for (do not average it away).
#[derive(Debug, Default, Clone)]
struct AnnotCensus {
    /// This document has ≥1 modelled annotation (per-file boolean).
    has_annots: bool,
    /// The catalog carries an `/AcroForm` (per-file boolean).
    has_acroform: bool,
    /// The `/AcroForm` sets `/NeedAppearances` true (per-file boolean, R51).
    need_appearances: bool,
    /// Total annotations across all pages.
    annots: usize,
    /// Of those, with a usable normal appearance (`Appearance::Normal`).
    with_appearance: usize,
    /// With an `/AP` state subdictionary that could not be selected.
    state_unresolved: usize,
    /// With no usable `/AP` at all (R43 named-not-painted).
    no_ap: usize,
    /// `/Popup` annotations (never page content).
    popup: usize,
    /// `/Widget` annotations (form fields — the dominant organic subtype).
    widget: usize,
    /// Per-subtype histogram of every annotation.
    by_subtype: BTreeMap<String, usize>,
}

/// One file's full measurement result.
#[derive(Debug)]
struct Outcome {
    category: Category,
    /// Human-readable detail for the TSV (error Display, or the Ok
    /// page/diagnostic summary). Tabs/newlines sanitized, truncated.
    detail: String,
    /// Normalized grouping key (digit runs collapsed) for the
    /// most-common-kinds summary. Empty for categories where grouping
    /// is meaningless (`Ok`, `Timeout`).
    kind: String,
    /// Page count (only meaningful for `Ok`).
    pages: usize,
    /// Render diagnostics (only non-zero for `Ok` files that rendered).
    diag: DiagTotals,
    /// Document-level annotation census (populated for every file whose
    /// page tree walked, regardless of render outcome).
    annots: AnnotCensus,
}

impl Outcome {
    /// Constructor for the error-shaped categories.
    fn err(category: Category, kind: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            category,
            detail: sanitize(&detail.into()),
            kind: kind.into(),
            pages: 0,
            diag: DiagTotals::default(),
            annots: AnnotCensus::default(),
        }
    }
}

/// Census every page's `/Annots` with pdfcer's own model (Pass 6.0
/// acceptance criterion 1).
fn census_annotations(
    doc: &pdfcer_core::document::Document,
    pages: &[page_tree::Page],
) -> AnnotCensus {
    use pdfcer_core::annot::{Appearance, need_appearances, page_annotations};
    let mut c = AnnotCensus {
        has_acroform: doc
            .catalog()
            .ok()
            .and_then(|cat| cat.get(b"AcroForm"))
            .is_some(),
        need_appearances: need_appearances(doc),
        ..AnnotCensus::default()
    };
    for page in pages {
        for annot in page_annotations(doc, page.id) {
            c.annots += 1;
            *c.by_subtype.entry(annot.subtype_label()).or_insert(0) += 1;
            if annot.is_widget() {
                c.widget += 1;
            }
            if annot.is_popup {
                c.popup += 1;
            }
            match annot.appearance {
                Appearance::Normal { .. } => c.with_appearance += 1,
                Appearance::StateUnresolved => c.state_unresolved += 1,
                Appearance::None => c.no_ap += 1,
            }
        }
    }
    c.has_annots = c.annots > 0;
    c
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("usage: corpus-report <corpus-dir> [more-dirs ...]");
        eprintln!("  Walks each directory for *.pdf, measures every file against the");
        eprintln!("  pdfcer load -> page-tree -> render pipeline, prints a summary table");
        eprintln!("  per directory, and writes <dir>-report.tsv next to it.");
        return ExitCode::from(2);
    }

    // Silence the default panic printer: worker panics are CAPTURED
    // findings (reported in the summary), not console spew. Installed
    // once, process-wide, before any worker exists.
    std::panic::set_hook(Box::new(|_| {}));

    let mut any_panic = false;
    for dir in &args {
        match run_corpus(Path::new(dir)) {
            Ok(panics) => any_panic |= panics > 0,
            Err(e) => {
                eprintln!("error: {dir}: {e}");
                return ExitCode::from(3);
            }
        }
    }
    if any_panic {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

/// Measure one corpus directory end to end: walk, classify every file,
/// write the TSV, print the summary. Returns the number of panicking
/// files (the caller escalates the exit code on any).
fn run_corpus(dir: &Path) -> Result<usize, String> {
    let files = collect_pdfs(dir)?;
    let total = files.len();
    eprintln!("[{}] {total} PDF file(s) found", dir.display());

    // Classify every file, preserving the sorted order for the TSV.
    let mut rows: Vec<(String, Outcome)> = Vec::with_capacity(total);
    for (i, (rel, abs)) in files.into_iter().enumerate() {
        if i % 100 == 0 {
            eprintln!("[{}] {i}/{total} ...", dir.display());
        }
        rows.push((rel, measure_file(&abs)));
    }

    write_tsv(dir, &rows)?;
    Ok(print_summary(dir, &rows))
}

/// Recursively collect every `*.pdf` (case-insensitive) under `root`,
/// skipping dot-directories (`.git` in a cloned corpus). Returns
/// `(relative-forward-slash-path, absolute-path)` pairs sorted by the
/// relative path — the determinism guarantee (module docs).
fn collect_pdfs(root: &Path) -> Result<Vec<(String, PathBuf)>, String> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        let entries =
            std::fs::read_dir(&d).map_err(|e| format!("read_dir {}: {e}", d.display()))?;
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

/// Measure one file under the wall-clock budget: read it, hand the
/// bytes to a worker thread running [`measure_bytes`] under
/// `catch_unwind`, and wait at most [`FILE_BUDGET`] for the verdict.
/// Timeout abandons the worker (module docs — the one deliberate leak).
fn measure_file(path: &Path) -> Outcome {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => return Outcome::err(Category::LoadError, "Io", format!("read failed: {e}")),
    };

    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let outcome = match catch_unwind(AssertUnwindSafe(|| measure_bytes(bytes))) {
            Ok(o) => o,
            Err(payload) => {
                // Recover the panic message from the payload the way
                // the default hook would (str or String, else opaque).
                let msg = payload
                    .downcast_ref::<&str>()
                    .map(|s| (*s).to_string())
                    .or_else(|| payload.downcast_ref::<String>().cloned())
                    .unwrap_or_else(|| "<non-string panic payload>".to_string());
                Outcome::err(Category::Panic, normalize_kind(&msg), msg)
            }
        };
        // The receiver may be gone (timeout) — a late result is
        // deliberately dropped, never an error.
        let _ = tx.send(outcome);
    });

    match rx.recv_timeout(FILE_BUDGET) {
        Ok(outcome) => outcome,
        Err(_) => Outcome::err(
            Category::Timeout,
            String::new(),
            format!(
                "exceeded {}s budget; worker abandoned",
                FILE_BUDGET.as_secs()
            ),
        ),
    }
}

/// The measured pipeline itself: load → page tree → render page 1 at
/// scale 0.5 (≈36 DPI — cheap but exercises the full interpreter).
/// Runs on the worker thread, inside `catch_unwind`.
fn measure_bytes(bytes: Vec<u8>) -> Outcome {
    // Stage 1 — document load (strict Pass 1 loader).
    let doc = match Document::from_bytes(bytes) {
        Ok(doc) => doc,
        // The one remaining DELIBERATE refusal gets its own category —
        // a named capability gap must never be counted as a parse
        // failure (the xref-stream/hybrid categories that used to live
        // here are gone: those files now load).
        Err(DocError::Xref(x)) if matches!(x.kind, XrefErrorKind::EncryptionUnsupported) => {
            return Outcome::err(
                Category::RefusedEncrypted,
                "EncryptionUnsupported",
                x.to_string(),
            );
        }
        Err(e) => {
            let (kind, detail) = describe_doc_error(&e);
            return Outcome::err(Category::LoadError, kind, detail);
        }
    };

    // Stage 2 — page-tree walk with inheritance.
    let pages = match page_tree::pages(&doc) {
        Ok(pages) => pages,
        // The /Resources strictness question gets its own category
        // (ROADMAP Pass 1.1: how often do real files omit it?).
        Err(PageTreeError::MissingRequired("Resources")) => {
            return Outcome::err(
                Category::MissingResources,
                "MissingRequired(Resources)",
                "required /Resources missing on page and all ancestors",
            );
        }
        Err(e) => {
            let msg = e.to_string();
            return Outcome::err(Category::OtherPageTreeError, normalize_kind(&msg), msg);
        }
    };

    // Document-level annotation census (Pass 6.0), over ALL pages —
    // independent of the page-1 render, so it is counted for empty trees
    // and render failures alike.
    let annots = census_annotations(&doc, &pages);

    // A well-formed EMPTY tree (/Count 0) is a legal document with
    // nothing to render — Ok, zero pages, no render attempted.
    let Some(first) = pages.first() else {
        return Outcome {
            category: Category::Ok,
            detail: "0 pages (well-formed empty tree); no render".to_string(),
            kind: String::new(),
            pages: 0,
            diag: DiagTotals::default(),
            annots,
        };
    };

    // Stage 3 — render page 1.
    match render_page(&doc, first, 0.5) {
        Ok(rendered) => {
            let d = &rendered.diagnostics;
            let diag = DiagTotals {
                glyphs_substituted: d.glyphs_substituted,
                glyphs_notdef: d.glyphs_notdef,
                fonts_unsupported: d.fonts_unsupported,
                unknown_ops: d.unknown_ops,
                deferred_ops: d.deferred_ops,
                tolerated: d.tolerated,
                images_rendered: d.images_rendered,
                images_unsupported: d.images_unsupported,
                forms_rendered: d.forms_rendered,
                xobject_depth_overflows: d.xobject_depth_overflows,
                images_codec_unsupported: d.images_codec_unsupported,
                codec_features_unsupported: d.codec_feature_unsupported.values().sum(),
                codec_geometry_mismatch: d.codec_geometry_mismatch,
                dct_cmyk_images: d.dct_cmyk_images,
                jpx_smask_in_data_preblended: d.jpx_smask_in_data_preblended,
                lzw_framing_anomalies: d.lzw_framing_anomalies,
            };
            Outcome {
                category: Category::Ok,
                detail: format!(
                    "pages={} subst={} notdef={} fonts_unsup={} unknown={} deferred={} tolerated={} images={} images_unsup={} forms={} xobj_overflow={} codec_unsup={} codec_feat={} codec_geom={} dct_cmyk={} lzw_anom={} jpx_preblended={}",
                    pages.len(),
                    d.glyphs_substituted,
                    d.glyphs_notdef,
                    d.fonts_unsupported,
                    d.unknown_ops,
                    d.deferred_ops,
                    d.tolerated,
                    d.images_rendered,
                    d.images_unsupported,
                    d.forms_rendered,
                    d.xobject_depth_overflows,
                    d.images_codec_unsupported,
                    d.codec_feature_unsupported.values().sum::<usize>(),
                    d.codec_geometry_mismatch,
                    d.dct_cmyk_images,
                    d.jpx_smask_in_data_preblended,
                    d.lzw_framing_anomalies,
                ),
                kind: String::new(),
                pages: pages.len(),
                diag,
                annots,
            }
        }
        Err(e) => {
            let msg = e.to_string();
            let mut o = Outcome::err(Category::RenderError, normalize_kind(&msg), msg);
            // The census is document-level, so a page-1 render failure does
            // not lose it — attach it so annotation totals stay complete.
            o.annots = annots;
            o
        }
    }
}

/// Split a non-refusal [`DocError`] into a grouping kind (variant-based,
/// digit-normalized so byte offsets don't fragment the counts) and the
/// full `Display` detail.
fn describe_doc_error(e: &DocError) -> (String, String) {
    let detail = e.to_string();
    let kind = match e {
        DocError::Io(_) => "Io".to_string(),
        DocError::Header(inner) => format!("Header: {}", normalize_kind(&inner.to_string())),
        DocError::Xref(x) => format!("Xref: {}", normalize_kind(&x.kind.to_string())),
        DocError::BadObject { source, .. } => {
            format!("BadObject: {}", normalize_kind(&source.to_string()))
        }
        DocError::ObjectIdMismatch { .. } => "ObjectIdMismatch".to_string(),
        DocError::ObjectStreamMissing { .. } => "ObjectStreamMissing".to_string(),
        DocError::ObjectStream { source, .. } => {
            format!("ObjectStream: {}", normalize_kind(&source.to_string()))
        }
        DocError::ObjectStreamIdMismatch { .. } => "ObjectStreamIdMismatch".to_string(),
        DocError::NoCatalog => "NoCatalog".to_string(),
        // DocError is #[non_exhaustive]; future variants group by their
        // normalized Display until this match learns their names.
        _ => normalize_kind(&detail),
    };
    (kind, detail)
}

/// Collapse every run of ASCII digits to `#` so error strings that
/// differ only in offsets/object numbers group together in the
/// most-common-kinds summary.
fn normalize_kind(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_digits = false;
    for c in s.chars() {
        if c.is_ascii_digit() {
            if !in_digits {
                out.push('#');
                in_digits = true;
            }
        } else {
            in_digits = false;
            out.push(c);
        }
    }
    out
}

/// Make a detail string TSV-safe (no tabs/newlines) and bounded
/// ([`DETAIL_MAX`] chars, `…`-terminated when truncated).
fn sanitize(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .map(|c| {
            if c == '\t' || c == '\n' || c == '\r' {
                ' '
            } else {
                c
            }
        })
        .collect();
    if cleaned.chars().count() > DETAIL_MAX {
        let mut out: String = cleaned.chars().take(DETAIL_MAX).collect();
        out.push('…');
        out
    } else {
        cleaned
    }
}

/// Write the per-file detail TSV next to the corpus directory:
/// `<dir>-report.tsv`, columns `file  category  detail`, rows in the
/// deterministic sorted-path order.
fn write_tsv(dir: &Path, rows: &[(String, Outcome)]) -> Result<(), String> {
    let file_name = format!(
        "{}-report.tsv",
        dir.file_name()
            .map_or_else(|| "corpus".into(), |n| n.to_string_lossy())
    );
    let tsv_path = dir.parent().unwrap_or(dir).join(file_name);
    let mut w = std::io::BufWriter::new(
        std::fs::File::create(&tsv_path)
            .map_err(|e| format!("create {}: {e}", tsv_path.display()))?,
    );
    let write = |w: &mut dyn std::io::Write, line: &str| {
        writeln!(w, "{line}").map_err(|e| format!("write {}: {e}", tsv_path.display()))
    };
    write(&mut w, "file\tcategory\tdetail")?;
    for (rel, outcome) in rows {
        write(
            &mut w,
            &format!("{rel}\t{}\t{}", outcome.category.name(), outcome.detail),
        )?;
    }
    eprintln!("[{}] TSV written: {}", dir.display(), tsv_path.display());
    Ok(())
}

/// Print the per-corpus summary to stdout: category table (count, %),
/// aggregate render diagnostics, top normalized error kinds per
/// category, and a prominent panic-file list. Returns the panic count.
fn print_summary(dir: &Path, rows: &[(String, Outcome)]) -> usize {
    let total = rows.len();
    println!("=== corpus-report: {} ({total} files) ===", dir.display());

    // Category counts (BTreeMap for a deterministic tie order; display
    // sorted by count descending, then name).
    let mut counts: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut diag = DiagTotals::default();
    let mut pages_total = 0usize;
    // Pass 6.0 annotation-census accumulators (per-file booleans + totals).
    let mut ac_files_with_annots = 0usize;
    let mut ac_files_with_acroform = 0usize;
    let mut ac_files_need_appearances = 0usize;
    let mut ac_annots = 0usize;
    let mut ac_with_appearance = 0usize;
    let mut ac_state_unresolved = 0usize;
    let mut ac_no_ap = 0usize;
    let mut ac_popup = 0usize;
    let mut ac_widget = 0usize;
    let mut ac_by_subtype: BTreeMap<String, usize> = BTreeMap::new();
    for (_, o) in rows {
        *counts.entry(o.category.name()).or_insert(0) += 1;
        pages_total += o.pages;
        diag.glyphs_substituted += o.diag.glyphs_substituted;
        diag.glyphs_notdef += o.diag.glyphs_notdef;
        diag.fonts_unsupported += o.diag.fonts_unsupported;
        diag.unknown_ops += o.diag.unknown_ops;
        diag.deferred_ops += o.diag.deferred_ops;
        diag.tolerated += o.diag.tolerated;
        diag.images_rendered += o.diag.images_rendered;
        diag.images_unsupported += o.diag.images_unsupported;
        diag.forms_rendered += o.diag.forms_rendered;
        diag.xobject_depth_overflows += o.diag.xobject_depth_overflows;
        diag.images_codec_unsupported += o.diag.images_codec_unsupported;
        diag.codec_features_unsupported += o.diag.codec_features_unsupported;
        diag.codec_geometry_mismatch += o.diag.codec_geometry_mismatch;
        diag.dct_cmyk_images += o.diag.dct_cmyk_images;
        diag.jpx_smask_in_data_preblended += o.diag.jpx_smask_in_data_preblended;
        diag.lzw_framing_anomalies += o.diag.lzw_framing_anomalies;
        // --- Pass 6.0 annotation census accumulation -----------------
        if o.annots.has_annots {
            ac_files_with_annots += 1;
        }
        if o.annots.has_acroform {
            ac_files_with_acroform += 1;
        }
        if o.annots.need_appearances {
            ac_files_need_appearances += 1;
        }
        ac_annots += o.annots.annots;
        ac_with_appearance += o.annots.with_appearance;
        ac_state_unresolved += o.annots.state_unresolved;
        ac_no_ap += o.annots.no_ap;
        ac_popup += o.annots.popup;
        ac_widget += o.annots.widget;
        for (subtype, n) in &o.annots.by_subtype {
            *ac_by_subtype.entry(subtype.clone()).or_insert(0) += n;
        }
    }
    let mut ordered: Vec<(&str, usize)> = counts.into_iter().collect();
    ordered.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));

    println!("\n  category count %");
    println!("  ------------------  -----  -----");
    for (name, n) in &ordered {
        #[allow(clippy::cast_precision_loss)] // corpus sizes are tiny vs f64 mantissa
        let pct = 100.0 * *n as f64 / total.max(1) as f64;
        println!("  {name:<18}  {n:>5}  {pct:>5.1}");
    }

    println!("\n  aggregate over Ok files (first-page renders):");
    println!("    total pages (page-tree counts): {pages_total}");
    println!("    glyphs substituted:  {}", diag.glyphs_substituted);
    println!("    glyphs .notdef:      {}", diag.glyphs_notdef);
    println!("    fonts unsupported:   {}", diag.fonts_unsupported);
    println!("    unknown ops:         {}", diag.unknown_ops);
    println!("    deferred ops:        {}", diag.deferred_ops);
    println!("    tolerated oddities:  {}", diag.tolerated);
    println!("    images rendered:     {}", diag.images_rendered);
    println!("    images unsupported:  {}", diag.images_unsupported);
    println!("    forms rendered:      {}", diag.forms_rendered);
    println!("    xobject overflows:   {}", diag.xobject_depth_overflows);
    println!("    images codec unsup:  {}", diag.images_codec_unsupported);
    println!(
        "    codec feat unsup:    {}",
        diag.codec_features_unsupported
    );
    println!("    codec geom mismatch: {}", diag.codec_geometry_mismatch);
    println!("    DCT CMYK images:     {}", diag.dct_cmyk_images);
    println!(
        "    JPX preblended:      {}",
        diag.jpx_smask_in_data_preblended
    );
    println!("    LZW framing anomaly: {}", diag.lzw_framing_anomalies);

    // --- Pass 6.0 annotation census (docs/decisions/008 acceptance 1) ---
    // Re-measured with pdfcer's OWN machinery over ALL pages of every file
    // whose page tree walked. This is the pinned baseline the Pass gate
    // uses; compare against decision 008's pypdf figures and run down any
    // material divergence (do not average).
    #[allow(clippy::cast_precision_loss)]
    let pct = |n: usize| 100.0 * n as f64 / total.max(1) as f64;
    println!("\n  annotation census (pdfcer-native, all pages):");
    println!(
        "    files with >=1 annotation: {ac_files_with_annots}  ({:.1}%)",
        pct(ac_files_with_annots)
    );
    println!(
        "    files with /AcroForm:      {ac_files_with_acroform}  ({:.1}%)",
        pct(ac_files_with_acroform)
    );
    println!("    files with /NeedAppearances: {ac_files_need_appearances}");
    println!("    annotations total:         {ac_annots}");
    println!("    with usable /AP /N:        {ac_with_appearance}");
    println!("    /AS state unresolved:      {ac_state_unresolved}");
    println!("    no usable /AP (R43):       {ac_no_ap}");
    println!("    /Popup (never painted):    {ac_popup}");
    println!("    /Widget:                   {ac_widget}");
    if !ac_by_subtype.is_empty() {
        let mut sub: Vec<(&String, &usize)> = ac_by_subtype.iter().collect();
        sub.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
        let top: Vec<String> = sub
            .iter()
            .take(12)
            .map(|(name, n)| format!("{name} {n}"))
            .collect();
        println!("    top subtypes:              {}", top.join(", "));
    }

    // Top normalized kinds for each error-shaped category.
    for cat in [
        Category::LoadError,
        Category::OtherPageTreeError,
        Category::RenderError,
        Category::Panic,
    ] {
        let mut kinds: BTreeMap<&str, usize> = BTreeMap::new();
        for (_, o) in rows.iter().filter(|(_, o)| o.category == cat) {
            *kinds.entry(o.kind.as_str()).or_insert(0) += 1;
        }
        if kinds.is_empty() {
            continue;
        }
        let mut ordered: Vec<(&str, usize)> = kinds.into_iter().collect();
        ordered.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
        println!("\n  top {} kinds:", cat.name());
        for (kind, n) in ordered.iter().take(TOP_KINDS) {
            println!("    {n:>5}  {kind}");
        }
    }

    // Panics are BUGS — name every file, loudly.
    let panics: Vec<&(String, Outcome)> = rows
        .iter()
        .filter(|(_, o)| o.category == Category::Panic)
        .collect();
    if !panics.is_empty() {
        println!(
            "\n  *** PANICS (pdfcer BUGS — {} file(s)) ***",
            panics.len()
        );
        for (rel, o) in &panics {
            println!("    {rel}");
            println!("      {}", o.detail);
        }
    }

    // Timeouts likewise get named — there should be very few.
    let timeouts: Vec<&String> = rows
        .iter()
        .filter(|(_, o)| o.category == Category::Timeout)
        .map(|(rel, _)| rel)
        .collect();
    if !timeouts.is_empty() {
        println!("\n  timeouts ({}):", timeouts.len());
        for rel in &timeouts {
            println!("    {rel}");
        }
    }

    println!();
    panics.len()
}
