//! # font-parity — embedded-font-program parse/routing regression gate
//!
//! The targeted guard for the class of bug the render-parity gate can only
//! catch *late* (as "text missing on a whole class of files"): a
//! **misroute** in [`FontProgram::parse`]
//! (`crates/pdfcer-render/src/font/program.rs`) — a valid embedded program
//! whose binary framing is detected as the WRONG variant and handed to the
//! wrong parser.
//!
//! ## The bug this exists to catch (verbatim signature)
//!
//! `FontProgram::parse` detects font-program framing by matching binary
//! magics on the RAW decoded bytes:
//!
//! | Magic (raw first bytes) | Framing | Variant |
//! |---|---|---|
//! | `00 01 00 00` / `OTTO` / `true` / `ttcf` | sfnt | [`FontProgram::Sfnt`] |
//! | `01 00` | bare CFF | [`FontProgram::Cff`] |
//! | `80 01` (PFB) or `%!` after ASCII-whitespace trim (PFA) | Type 1 | [`FontProgram::Type1`] |
//!
//! The recently-fixed regression: a leading-whitespace trim that INCLUDED
//! NUL stripped the leading `0x00` of a TrueType `0x00010000` version tag,
//! leaving `0x01 00 …`, which then matched the bare-CFF magic → the sfnt
//! program was handed to the CFF parser → "offset out of bounds" → every
//! embedded TrueType with the standard version rejected → text on a whole
//! class of real files (SolidWorks / AutoCAD / Office `CIDFontType2`)
//! silently skipped. The fix matches magics on raw bytes and only
//! whitespace-trims the Type-1 *text* path.
//!
//! ## What the gate asserts (routing correctness + fail-clean)
//!
//! For **every** embedded font program in **every** loadable corpus file
//! (simple-font `FontFile/FontFile2/FontFile3` and composite/descendant-
//! font descriptors alike), the program must EITHER:
//!
//! - **parse to the CORRECT variant** — its parsed variant agrees with the
//!   framing its magic bytes imply (a `00 01 00 00`/`OTTO`/`true`/`ttcf`
//!   program becomes `Sfnt`, NEVER `Cff`; a `01 00` program → `Cff`; a
//!   Type-1 program → `Type1`), OR
//! - **fail clean** with a named [`ProgramError`] (`UnknownFormat` /
//!   `Parse`) — a legitimate outcome for a truncated / corrupt subset.
//!
//! It must NEVER **misroute** (parse to a variant that disagrees with its
//! magic — the exact bug signature) and NEVER **panic**. Misroutes and
//! panics fail the gate; clean parse failures are reported (they are a
//! real-world coverage picture) but do not fail it.
//!
//! ## Coherence with the `fonts_unsupported` diagnostic taxonomy
//!
//! `crates/pdfcer-render/src/text.rs` maps ANY `FontProgram::parse` failure
//! to the single `UnsupportedFont::UnusableProgram` reason key (which feeds
//! `Diagnostics::fonts_unsupported`). This harness reports the FINER
//! [`ProgramError`] reason (`UnknownFormat` vs `Parse`) — a refinement of
//! that one bucket, never a contradiction of it: everything this harness
//! counts as a clean parse failure is exactly what text.rs would count as
//! `UnusableProgram`. The two taxonomies agree; this one is strictly more
//! granular.
//!
//! ## Standing gate membership
//!
//! This is a local corpus gate (no CI dependency), the font-layer analogue
//! of R46 (content-identity) and R59 (render-parity): **re-run on any change
//! to `program.rs` or the font layer** (`crates/pdfcer-render/src/font/`).
//! Documented as the intended standing rule R62 (librarian assigns the
//! number) — "embedded font programs route to the correct parser or fail
//! clean; a magic/variant disagreement is a gate failure."
//!
//! ## Usage
//!
//! ```text
//! font-parity <dir> [<dir> ...]        # e.g. fixtures/external
//! font-parity --gate <dir> [<dir> ...] # exit non-zero on any misroute/panic
//! ```
//!
//! Without `--gate` the tool always exits 0 after printing the report (it is
//! a measurement run). With `--gate` it exits 1 if any misroute or panic was
//! found — the form the standing gate uses. Exit 2 = usage error; exit 3 = a
//! corpus directory could not be walked.

use std::collections::{BTreeMap, HashSet};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::mpsc;
use std::time::Duration;

use pdfcer_core::document::Document;
use pdfcer_core::object::{Dict, ObjId, Object};
use pdfcer_render::font::program::{FontProgram, ProgramError};

/// Per-file wall-clock budget. Font parsing is fast and zero-copy, but a
/// pathological `/Filter` chain on a font stream could be slow; a hung
/// worker must never stall the ~2,900-file sweep. On timeout the worker is
/// abandoned (the same one deliberate leak as `tools/corpus-report`).
const FILE_BUDGET: Duration = Duration::from_secs(30);

/// Maximum structural recursion depth when scanning one indirect object for
/// `FontFile*` references. Parsed objects are finite trees (indirect
/// references are NOT followed here), so this is a pure defensive belt
/// against a pathologically deep dictionary nest (ARCHITECTURE.md §10).
const MAX_SCAN_DEPTH: u32 = 128;

/// How many enumerated samples to keep for the unbounded report lists
/// (clean-failure examples). Misroutes and panics are NEVER sampled away —
/// they are the gate failures and every one is named.
const SAMPLE_CAP: usize = 40;

/// The framing a program's RAW magic bytes imply. This is an EXACT copy of
/// the detection ladder in `FontProgram::parse` — it is the independent
/// oracle the misroute check compares the parser's actual verdict against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Framing {
    Sfnt,
    Cff,
    Type1,
    Unknown,
}

impl Framing {
    const fn label(self) -> &'static str {
        match self {
            Self::Sfnt => "sfnt",
            Self::Cff => "cff",
            Self::Type1 => "type1",
            Self::Unknown => "unknown",
        }
    }
}

/// The variant `FontProgram::parse` actually produced, reduced to a plain
/// `Copy` tag so it can cross the `catch_unwind` / thread boundary without
/// carrying the borrowed program (which is neither `Send` nor `UnwindSafe`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Variant {
    Sfnt,
    Cff,
    Type1,
}

impl Variant {
    const fn label(self) -> &'static str {
        match self {
            Self::Sfnt => "Sfnt",
            Self::Cff => "Cff",
            Self::Type1 => "Type1",
        }
    }

    /// The framing this parsed variant corresponds to — the correspondence
    /// the misroute check enforces (`Sfnt` magic ⇒ `Sfnt` variant, etc.).
    const fn expected_framing(self) -> Framing {
        match self {
            Self::Sfnt => Framing::Sfnt,
            Self::Cff => Framing::Cff,
            Self::Type1 => Framing::Type1,
        }
    }
}

/// The outcome of parsing one embedded program (plain data — no borrow).
#[derive(Debug, Clone, PartialEq, Eq)]
enum ParseOutcome {
    /// Parsed to a variant.
    Parsed(Variant),
    /// Failed clean with a named `ProgramError` reason key.
    Failed(&'static str),
    /// `FontProgram::parse` PANICKED — a hard gate failure.
    Panicked(String),
}

/// One embedded program's full record: what its magic said, what the parser
/// did, and the source object id (for enumeration in the report).
#[derive(Debug, Clone)]
struct ProgramRecord {
    obj: ObjId,
    magic: Framing,
    outcome: ParseOutcome,
}

impl ProgramRecord {
    /// A misroute is a SUCCESSFUL parse whose variant disagrees with the
    /// framing its magic bytes imply — the exact bug signature. A clean
    /// failure is not a misroute; a program that parsed despite `Unknown`
    /// magic is also flagged (it should have failed `UnknownFormat`).
    fn is_misroute(&self) -> bool {
        match &self.outcome {
            ParseOutcome::Parsed(v) => v.expected_framing() != self.magic,
            ParseOutcome::Failed(_) | ParseOutcome::Panicked(_) => false,
        }
    }
}

/// One file's report, returned across the worker-thread boundary.
#[derive(Debug, Default)]
struct FileReport {
    loadable: bool,
    /// Streams referenced by a `FontFile*` key that could not be decoded
    /// (broken `/Filter` chain, dangling ref, non-stream). Counted apart
    /// from parse outcomes — decode never reaches the router.
    decode_failed: usize,
    programs: Vec<ProgramRecord>,
    /// Set if the whole file walk (loader / object walk) panicked.
    file_panic: Option<String>,
    /// Set if the worker exceeded [`FILE_BUDGET`] and was abandoned.
    timed_out: bool,
}

/// Aggregate counters across the whole sweep.
#[derive(Default)]
struct Totals {
    files_scanned: usize,
    files_loadable: usize,
    programs_total: usize,
    decode_failed: usize,
    by_magic: BTreeMap<&'static str, usize>,
    parsed_sfnt: usize,
    parsed_cff: usize,
    parsed_type1: usize,
    failed_by_reason: BTreeMap<&'static str, usize>,
    failure_samples: Vec<String>,
    /// GATE-FAILING lists — never sampled away.
    misroutes: Vec<String>,
    program_panics: Vec<String>,
    file_panics: Vec<String>,
    timeouts: Vec<String>,
}

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let gate = args.iter().any(|a| a == "--gate");
    args.retain(|a| a != "--gate");
    if args.is_empty() {
        eprintln!("usage: font-parity [--gate] <dir> [<dir> ...]");
        eprintln!("  Walks every embedded font program (FontFile/FontFile2/FontFile3, simple +");
        eprintln!(
            "  composite/descendant) in every loadable *.pdf, runs FontProgram::parse on each,"
        );
        eprintln!(
            "  and asserts routing correctness (magic bytes agree with parsed variant) + fail-"
        );
        eprintln!("  clean. --gate exits non-zero on any misroute or panic.");
        return ExitCode::from(2);
    }

    // Worker panics are CAPTURED findings (named in the report), not console
    // spew — silence the default hook process-wide before any worker exists.
    std::panic::set_hook(Box::new(|_| {}));

    let mut totals = Totals::default();
    for dir in &args {
        let files = match collect_pdfs(Path::new(dir)) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("error: {dir}: {e}");
                return ExitCode::from(3);
            }
        };
        let total = files.len();
        eprintln!("[{dir}] {total} PDF file(s) found");
        for (i, (rel, abs)) in files.into_iter().enumerate() {
            if i % 200 == 0 {
                eprintln!("[{dir}] {i}/{total} ...");
            }
            totals.files_scanned += 1;
            accumulate(&rel, measure_file(&abs), &mut totals);
        }
    }

    print_summary(&totals);

    let gate_ok = totals.misroutes.is_empty()
        && totals.program_panics.is_empty()
        && totals.file_panics.is_empty();
    if gate && !gate_ok {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

/// Fold one file's report into the running totals, enumerating gate
/// failures (misroutes, panics) fully and sampling clean failures.
fn accumulate(rel: &str, report: FileReport, t: &mut Totals) {
    if report.timed_out {
        t.timeouts.push(rel.to_string());
        return;
    }
    if let Some(msg) = report.file_panic {
        t.file_panics.push(format!("{rel}: {msg}"));
        return;
    }
    if !report.loadable {
        return; // not loadable — out of scope for this gate (like roundtrip)
    }
    t.files_loadable += 1;
    t.decode_failed += report.decode_failed;
    for rec in &report.programs {
        t.programs_total += 1;
        *t.by_magic.entry(rec.magic.label()).or_insert(0) += 1;
        if rec.is_misroute()
            && let ParseOutcome::Parsed(v) = &rec.outcome
        {
            t.misroutes.push(format!(
                "{rel}: obj {} {} — magic={} but parsed={} (MISROUTE)",
                rec.obj.num,
                rec.obj.generation,
                rec.magic.label(),
                v.label(),
            ));
        }
        match &rec.outcome {
            ParseOutcome::Parsed(Variant::Sfnt) => t.parsed_sfnt += 1,
            ParseOutcome::Parsed(Variant::Cff) => t.parsed_cff += 1,
            ParseOutcome::Parsed(Variant::Type1) => t.parsed_type1 += 1,
            ParseOutcome::Failed(reason) => {
                *t.failed_by_reason.entry(reason).or_insert(0) += 1;
                if t.failure_samples.len() < SAMPLE_CAP {
                    t.failure_samples.push(format!(
                        "{rel}: obj {} magic={} → {reason}",
                        rec.obj.num,
                        rec.magic.label(),
                    ));
                }
            }
            ParseOutcome::Panicked(msg) => {
                t.program_panics.push(format!(
                    "{rel}: obj {} magic={} — parse PANICKED: {msg}",
                    rec.obj.num,
                    rec.magic.label(),
                ));
            }
        }
    }
}

/// Measure one file under the wall-clock budget: hand its bytes to a worker
/// thread running [`process_file`] under `catch_unwind`, wait at most
/// [`FILE_BUDGET`]. Timeout abandons the worker (the one deliberate leak).
fn measure_file(path: &Path) -> FileReport {
    let Ok(bytes) = std::fs::read(path) else {
        return FileReport::default(); // unreadable ⇒ treated as not loadable
    };
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let report = match catch_unwind(AssertUnwindSafe(|| process_file(bytes))) {
            Ok(r) => r,
            Err(payload) => FileReport {
                file_panic: Some(panic_message(&*payload)),
                ..FileReport::default()
            },
        };
        let _ = tx.send(report); // receiver may be gone on timeout — fine
    });
    match rx.recv_timeout(FILE_BUDGET) {
        Ok(report) => report,
        Err(_) => FileReport {
            timed_out: true,
            ..FileReport::default()
        },
    }
}

/// Load one file and parse every embedded font program it references.
///
/// Coverage strategy: iterate EVERY indirect object and structurally scan
/// its DIRECT value (dicts + arrays, **not** following references) for the
/// three `FontFile*` keys, collecting the object ids of the referenced
/// program streams into a document-wide dedup set. Because streams are
/// always indirect (§7.3.8.1) a `FontFile*` value is always a reference, so
/// deduping by that target id yields exactly the set of DISTINCT embedded
/// programs — every simple-font descriptor and every composite descendant-
/// font descriptor alike, regardless of page reachability (a font on an
/// unrendered page counts as much as one on page 1).
fn process_file(bytes: Vec<u8>) -> FileReport {
    let Ok(doc) = Document::from_bytes(bytes) else {
        return FileReport::default();
    };
    let mut report = FileReport {
        loadable: true,
        ..FileReport::default()
    };

    // Pass 1 — collect the deduped set of program-stream object ids.
    let mut ids: HashSet<ObjId> = HashSet::new();
    for io in doc.objects() {
        collect_fontfile_refs(&io.value, &mut ids, 0);
    }

    // Pass 2 — decode + parse each distinct program. Deterministic order so
    // the enumerated report is stable across runs.
    let mut ordered: Vec<ObjId> = ids.into_iter().collect();
    ordered.sort();
    for id in ordered {
        match decode_program(&doc, id) {
            Some(decoded) => {
                let magic = magic_of(&decoded);
                let outcome = parse_outcome(&decoded);
                report.programs.push(ProgramRecord {
                    obj: id,
                    magic,
                    outcome,
                });
            }
            None => report.decode_failed += 1,
        }
    }
    report
}

/// Recursively collect the object ids targeted by any `FontFile*` key in
/// `obj`'s DIRECT structure. Indirect references are never followed (Pass 1
/// visits every indirect object independently), so this terminates on the
/// finite parsed tree; [`MAX_SCAN_DEPTH`] is a defensive belt only.
fn collect_fontfile_refs(obj: &Object, out: &mut HashSet<ObjId>, depth: u32) {
    if depth > MAX_SCAN_DEPTH {
        return;
    }
    match obj {
        Object::Dict(d) => {
            scan_dict_keys(d, out);
            for (_, v) in &d.0 {
                collect_fontfile_refs(v, out, depth + 1);
            }
        }
        Object::Stream(s) => {
            // A stream's dict can itself be (or contain) a descriptor.
            scan_dict_keys(&s.dict, out);
            for (_, v) in &s.dict.0 {
                collect_fontfile_refs(v, out, depth + 1);
            }
        }
        Object::Array(a) => {
            for v in a {
                collect_fontfile_refs(v, out, depth + 1);
            }
        }
        _ => {}
    }
}

/// Record the reference target of each `FontFile*` key present in `d`.
fn scan_dict_keys(d: &Dict, out: &mut HashSet<ObjId>) {
    for key in [b"FontFile2".as_slice(), b"FontFile3", b"FontFile"] {
        if let Some(Object::Reference(id)) = d.get(key) {
            out.insert(*id);
        }
    }
}

/// Resolve one program-stream object id to its DECODED bytes (mirrors
/// `text.rs::embedded_program`: slice the raw span, run the `/Filter` chain,
/// reject empty). `None` on any decode gap — the router is never reached.
fn decode_program(doc: &Document, id: ObjId) -> Option<Vec<u8>> {
    let io = doc.get(id)?;
    let Object::Stream(stream) = &io.value else {
        return None;
    };
    let raw = stream.data_span.slice(doc.bytes())?;
    let bytes = pdfcer_core::filters::decode_stream(&stream.dict, raw).ok()?;
    if bytes.is_empty() { None } else { Some(bytes) }
}

/// Classify a program's framing from its RAW magic bytes.
///
/// This is an EXACT transcription of the detection ladder in
/// `FontProgram::parse` (program.rs) — the independent oracle the misroute
/// check compares the parser's verdict against. Keep the two in lockstep:
/// if program.rs's ladder changes, this must change identically, and the
/// `magic_ladder_matches_program_rs` test asserts they still agree on the
/// canonical inputs.
fn magic_of(data: &[u8]) -> Framing {
    match data {
        [0x00, 0x01, 0x00, 0x00, ..]
        | [b'O', b'T', b'T', b'O', ..]
        | [b't', b'r', b'u', b'e', ..]
        | [b't', b't', b'c', b'f', ..] => Framing::Sfnt,
        [0x01, 0x00, ..] => Framing::Cff,
        [0x80, 0x01, ..] => Framing::Type1,
        _ => {
            // Type-1 TEXT (PFA) only: tolerate leading ASCII whitespace
            // (NUL deliberately NOT trimmed — that is the bug) before `%!`.
            let start = data
                .iter()
                .position(|b| !matches!(b, b' ' | b'\t' | b'\r' | b'\n' | b'\x0C'))
                .unwrap_or(0);
            match data.get(start..).unwrap_or(data) {
                [b'%', b'!', ..] => Framing::Type1,
                _ => Framing::Unknown,
            }
        }
    }
}

/// Run `FontProgram::parse` on one decoded program under `catch_unwind`,
/// reducing the result to a plain [`ParseOutcome`] (no borrow crosses the
/// unwind boundary — the parsed program lives and dies inside the closure).
fn parse_outcome(decoded: &[u8]) -> ParseOutcome {
    match catch_unwind(AssertUnwindSafe(|| classify_parse(decoded))) {
        Ok(o) => o,
        Err(payload) => ParseOutcome::Panicked(panic_message(&*payload)),
    }
}

/// The borrow-confined parse: everything touching the borrowed [`FontProgram`]
/// happens here; only the `Copy` outcome escapes.
fn classify_parse(decoded: &[u8]) -> ParseOutcome {
    match FontProgram::parse(decoded) {
        Ok(FontProgram::Sfnt(_)) => ParseOutcome::Parsed(Variant::Sfnt),
        Ok(FontProgram::Cff(_)) => ParseOutcome::Parsed(Variant::Cff),
        Ok(FontProgram::Type1(_)) => ParseOutcome::Parsed(Variant::Type1),
        Err(ProgramError::UnknownFormat) => ParseOutcome::Failed("UnknownFormat"),
        Err(ProgramError::Parse(_)) => ParseOutcome::Failed("Parse"),
        // Draw / MissingGlyph are outline-time, unreachable from parse; a
        // future ProgramError variant lands here until this match learns it.
        Err(_) => ParseOutcome::Failed("Other"),
    }
}

/// Recover a panic payload's message the way the default hook would.
fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|s| (*s).to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "<non-string panic payload>".to_string())
}

/// Recursively collect every `*.pdf` (case-insensitive) under `root`,
/// skipping dot-directories, sorted by relative path for determinism.
fn collect_pdfs(root: &Path) -> Result<Vec<(String, PathBuf)>, String> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        let entries =
            std::fs::read_dir(&d).map_err(|e| format!("read_dir {}: {e}", d.display()))?;
        for entry in entries.flatten() {
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

/// Print the corpus breakdown and the gate verdict.
fn print_summary(t: &Totals) {
    let pct = |n: usize, d: usize| -> f64 {
        if d == 0 {
            0.0
        } else {
            n as f64 * 100.0 / d as f64
        }
    };
    println!("=== font-parity: embedded-font-program parse/routing gate ===");
    println!("files scanned:        {}", t.files_scanned);
    println!("files loadable:       {}", t.files_loadable);
    println!("embedded programs:    {}", t.programs_total);
    println!("stream decode-failed: {}", t.decode_failed);

    println!("\n-- by magic framing (raw bytes) --");
    for (label, n) in &t.by_magic {
        println!("  {label:<8} {n:>7}  ({:.2}%)", pct(*n, t.programs_total));
    }

    let parsed = t.parsed_sfnt + t.parsed_cff + t.parsed_type1;
    let failed: usize = t.failed_by_reason.values().sum();
    println!("\n-- by parse result --");
    println!(
        "  parsed-Sfnt   {:>7}  ({:.2}%)",
        t.parsed_sfnt,
        pct(t.parsed_sfnt, t.programs_total)
    );
    println!(
        "  parsed-Cff    {:>7}  ({:.2}%)",
        t.parsed_cff,
        pct(t.parsed_cff, t.programs_total)
    );
    println!(
        "  parsed-Type1  {:>7}  ({:.2}%)",
        t.parsed_type1,
        pct(t.parsed_type1, t.programs_total)
    );
    println!(
        "  parsed total  {parsed:>7}  ({:.2}%)",
        pct(parsed, t.programs_total)
    );
    println!(
        "  failed clean  {failed:>7}  ({:.2}%)",
        pct(failed, t.programs_total)
    );
    for (reason, n) in &t.failed_by_reason {
        println!("      {reason:<14} {n:>7}");
    }

    if !t.failure_samples.is_empty() {
        println!(
            "\n-- clean-failure samples (first {}) --",
            t.failure_samples.len()
        );
        for s in &t.failure_samples {
            println!("  {s}");
        }
    }

    // GATE-FAILING findings — every one named, never sampled.
    if !t.misroutes.is_empty() {
        println!("\n*** MISROUTES ({}) — GATE FAILURE ***", t.misroutes.len());
        for m in &t.misroutes {
            println!("  {m}");
        }
    }
    if !t.program_panics.is_empty() {
        println!(
            "\n*** PARSE PANICS ({}) — GATE FAILURE ***",
            t.program_panics.len()
        );
        for p in &t.program_panics {
            println!("  {p}");
        }
    }
    if !t.file_panics.is_empty() {
        println!(
            "\n*** FILE-WALK PANICS ({}) — GATE FAILURE ***",
            t.file_panics.len()
        );
        for p in &t.file_panics {
            println!("  {p}");
        }
    }

    let gate_ok = t.misroutes.is_empty() && t.program_panics.is_empty() && t.file_panics.is_empty();
    println!(
        "\nGATE: {}",
        if gate_ok {
            "PASS (every embedded program routed correctly or failed clean; zero misroutes, zero panics)"
        } else {
            "FAIL (see misroutes / panics above)"
        }
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- The exact regression: a 0x00010000-versioned sfnt must ROUTE to
    //     Sfnt (not have its NUL trimmed and misread as bare CFF). --------

    /// A minimal but structurally valid sfnt header (version `0x00010000`,
    /// zero tables) — enough for `read-fonts`' `FontRef::new` to construct.
    fn minimal_sfnt_header() -> Vec<u8> {
        let mut h = vec![0x00u8, 0x01, 0x00, 0x00]; // sfnt version 1.0
        h.extend_from_slice(&[0x00, 0x00]); // numTables = 0
        h.extend_from_slice(&[0x00, 0x00]); // searchRange
        h.extend_from_slice(&[0x00, 0x00]); // entrySelector
        h.extend_from_slice(&[0x00, 0x00]); // rangeShift
        h
    }

    #[test]
    fn sfnt_version_routes_to_sfnt_not_cff() {
        let h = minimal_sfnt_header();
        // The magic oracle must say sfnt...
        assert_eq!(magic_of(&h), Framing::Sfnt);
        // ...and the REAL parser must agree — routing to Sfnt, never Cff.
        assert_eq!(parse_outcome(&h), ParseOutcome::Parsed(Variant::Sfnt));
        // ...which is, by definition, NOT a misroute.
        let rec = ProgramRecord {
            obj: ObjId::new(1, 0),
            magic: magic_of(&h),
            outcome: parse_outcome(&h),
        };
        assert!(!rec.is_misroute(), "sfnt header must not be a misroute");
    }

    #[test]
    fn type1_pfa_leading_whitespace_routes_to_type1() {
        // Leading NEWLINE before `%!` (module trap 1): the Type-1 text path
        // trims ASCII whitespace, so this classifies as Type1 framing and
        // the parser routes into the Type1 arm (failing in Parse on the
        // truncated body — NOT UnknownFormat, which would mean misrouted).
        let data = b"\n%!PS-AdobeFont-1.0: Test 001.001\ngarbage".as_slice();
        assert_eq!(magic_of(data), Framing::Type1);
        assert_eq!(parse_outcome(data), ParseOutcome::Failed("Parse"));
    }

    #[test]
    fn magic_ladder_matches_program_rs_on_canonical_inputs() {
        // Lockstep check: the four sfnt magics, bare CFF, PFB, PFA-after-
        // whitespace, and a non-font all classify as this harness expects.
        assert_eq!(magic_of(&[0x00, 0x01, 0x00, 0x00, 0xAA]), Framing::Sfnt);
        assert_eq!(magic_of(b"OTTO...."), Framing::Sfnt);
        assert_eq!(magic_of(b"true...."), Framing::Sfnt);
        assert_eq!(magic_of(b"ttcf...."), Framing::Sfnt);
        assert_eq!(magic_of(&[0x01, 0x00, 0x04, 0x02]), Framing::Cff);
        assert_eq!(magic_of(&[0x80, 0x01, 0x00, 0x00]), Framing::Type1);
        assert_eq!(magic_of(b"  \t%!PS-Adobe"), Framing::Type1);
        assert_eq!(magic_of(b"not a font"), Framing::Unknown);
        // The bug's fingerprint: a NUL-led sfnt version must NOT be seen as
        // CFF. If the ladder ever trimmed NUL, this would become Cff.
        assert_eq!(magic_of(&[0x00, 0x01, 0x00, 0x00]), Framing::Sfnt);
    }

    #[test]
    fn misroute_detector_flags_magic_variant_disagreement() {
        // magic=sfnt but parsed=Cff is the precise bug signature.
        let bug = ProgramRecord {
            obj: ObjId::new(7, 0),
            magic: Framing::Sfnt,
            outcome: ParseOutcome::Parsed(Variant::Cff),
        };
        assert!(bug.is_misroute());
        // Agreement is clean.
        let ok = ProgramRecord {
            obj: ObjId::new(7, 0),
            magic: Framing::Sfnt,
            outcome: ParseOutcome::Parsed(Variant::Sfnt),
        };
        assert!(!ok.is_misroute());
        // A clean failure is never a misroute.
        let failed = ProgramRecord {
            obj: ObjId::new(7, 0),
            magic: Framing::Sfnt,
            outcome: ParseOutcome::Failed("Parse"),
        };
        assert!(!failed.is_misroute());
    }

    #[test]
    fn unknown_bytes_fail_clean_not_panic() {
        let data = b"this is definitely not a font program";
        assert_eq!(magic_of(data), Framing::Unknown);
        assert_eq!(parse_outcome(data), ParseOutcome::Failed("UnknownFormat"));
    }
}
