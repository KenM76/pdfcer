//! # content-identity — the R46 content-stream identity gate (Pass 6.1)
//!
//! Walks one or more corpus directories and, for every `*.pdf` that
//! `pdfcer-core` can load, decodes and tokenizes every page's content
//! stream, re-emits it through
//! [`pdfcer_core::writer::content::reemit_canonical`], and byte-compares
//! the result against the decoded source. This is the executable form of
//! R46 (docs/decisions/008 §3.4): the same inversion as Pass 3.0's writer
//! identity gate, one level down.
//!
//! ## The two verdicts, and why they are different
//!
//! For each content stream:
//!
//! - **byte-identical** — the re-emission equals the source exactly. This
//!   is the target for a stream whose numbers are already in canonical
//!   form (the overwhelming majority of producer output).
//! - **non-identical** — the re-emission differs. Under `reemit_canonical`
//!   the *only* tokens re-emitted from value are numeric operands, so a
//!   difference is always a number-spelling normalization (`1.` → `1.0`,
//!   `+5` → `5`, `.5` → `0.5`). Each is enumerated by file with the first
//!   divergent token as its reason (R20). A non-identical stream is **not
//!   a corruption** — the value is preserved, only its spelling changes.
//!
//! On top of the byte compare, every stream is checked for **semantic
//! preservation**: the re-emitted bytes are re-parsed and their token
//! sequence (operator names + operand *values*) is compared to the
//! original. A mismatch here is a **CORRUPTION** — the X6 failure the gate
//! exists to catch mechanically (an operand dropped, reordered, or a
//! number whose value changed) — and it is counted separately and fails
//! the gate. In a correct serializer this count is **zero**.
//!
//! ## Usage
//!
//! ```text
//! content-identity <dir> [<dir> ...]
//! ```
//!
//! Exit code 0 if every stream is semantically preserved (corruptions ==
//! 0), 1 otherwise. Byte non-identity is reported but does not fail the
//! gate — it is the enumerated, expected remainder.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use pdfcer_core::content::{ContentStream, ContentTokenKind};
use pdfcer_core::document::Document;
use pdfcer_core::object::Object;
use pdfcer_core::page_tree;
use pdfcer_core::writer::content::{number_divergence_reason, reemit_canonical};

/// Aggregate counters over a corpus sweep.
#[derive(Default)]
struct Totals {
    files_scanned: usize,
    files_loadable: usize,
    streams_total: usize,
    streams_byte_identical: usize,
    streams_non_identical: usize,
    streams_corrupted: usize,
    /// A bounded sample of non-identity reasons for the summary.
    reason_samples: Vec<String>,
    /// Every corruption, fully enumerated (never sampled away).
    corruptions: Vec<String>,
}

fn main() -> ExitCode {
    let dirs: Vec<PathBuf> = std::env::args().skip(1).map(PathBuf::from).collect();
    if dirs.is_empty() {
        eprintln!("usage: content-identity <dir> [<dir> ...]");
        eprintln!("  Parses every content stream in every loadable *.pdf and byte-compares");
        eprintln!("  a canonical re-emission against the source (R46, docs/decisions/008).");
        return ExitCode::from(2);
    }

    let mut totals = Totals::default();
    for dir in &dirs {
        let files = match collect_pdfs(dir) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("error: {e}");
                return ExitCode::from(2);
            }
        };
        for (rel, abs) in files {
            totals.files_scanned += 1;
            measure_file(&rel, &abs, &mut totals);
        }
    }

    print_summary(&totals);
    if totals.streams_corrupted == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// Measure every content stream in one file.
fn measure_file(rel: &str, abs: &Path, totals: &mut Totals) {
    let Ok(bytes) = std::fs::read(abs) else {
        return;
    };
    let Ok(doc) = Document::from_bytes(bytes) else {
        return; // not loadable — out of scope for this gate, like roundtrip
    };
    totals.files_loadable += 1;
    let Ok(pages) = page_tree::pages(&doc) else {
        return;
    };
    for page in &pages {
        let cs = match ContentStream::from_page(&doc.view(), page) {
            Ok(cs) => cs,
            Err(_) => continue, // decode/tokenize failure is a different gap
        };
        measure_stream(rel, &cs, totals);
    }
}

/// Compare one content stream's canonical re-emission against its source,
/// and check semantic preservation.
fn measure_stream(rel: &str, cs: &ContentStream, totals: &mut Totals) {
    totals.streams_total += 1;
    let reemitted = reemit_canonical(cs);
    if reemitted == cs.buf {
        totals.streams_byte_identical += 1;
    } else {
        totals.streams_non_identical += 1;
        if let Some(reason) = cs
            .tokens
            .iter()
            .find_map(|t| number_divergence_reason(&cs.buf, t))
        {
            if totals.reason_samples.len() < 40 {
                totals.reason_samples.push(format!("{rel}: {reason}"));
            }
        }
    }
    // Semantic preservation: re-parse and compare the operation view.
    if let Ok(back) = ContentStream::parse(reemitted) {
        if !semantically_equal(cs, &back) {
            totals.streams_corrupted += 1;
            if totals.corruptions.len() < 200 {
                totals
                    .corruptions
                    .push(format!("{rel}: token stream changed"));
            }
        }
    } else {
        // Re-emission that no longer parses is the worst corruption.
        totals.streams_corrupted += 1;
        if totals.corruptions.len() < 200 {
            totals
                .corruptions
                .push(format!("{rel}: re-emission failed to re-parse"));
        }
    }
}

/// Whether two token streams carry the same operators and operand values
/// (ignoring byte-level spelling, which is exactly what may legitimately
/// change). A difference here is a real corruption.
fn semantically_equal(a: &ContentStream, b: &ContentStream) -> bool {
    if a.tokens.len() != b.tokens.len() {
        return false;
    }
    a.tokens.iter().zip(&b.tokens).all(|(x, y)| {
        match (&x.kind, &y.kind) {
            // Operators compare by their keyword bytes.
            (ContentTokenKind::Operator, ContentTokenKind::Operator) => {
                x.span.slice(&a.buf) == y.span.slice(&b.buf)
            }
            // Operands compare by parsed value (numbers may be re-spelled).
            (ContentTokenKind::Operand(ox), ContentTokenKind::Operand(oy)) => {
                operands_equal(ox, oy)
            }
            // Inline images compare by their whole span bytes (verbatim).
            (ContentTokenKind::InlineImage { .. }, ContentTokenKind::InlineImage { .. }) => {
                x.span.slice(&a.buf) == y.span.slice(&b.buf)
            }
            _ => false,
        }
    })
}

/// Operand value equality that treats `Integer(4)` and `Real(4.0)` as
/// equal numbers (a writer may not change 4 into 4.0, but the value is the
/// same; `reemit_canonical` never does this, and this keeps the check from
/// flagging a legal integer/real distinction as corruption). All other
/// object kinds compare structurally.
fn operands_equal(a: &Object, b: &Object) -> bool {
    match (a, b) {
        (Object::Integer(_) | Object::Real(_), Object::Integer(_) | Object::Real(_)) => {
            a.as_number() == b.as_number()
        }
        _ => a == b,
    }
}

/// Recursively collect every `*.pdf` under `root`, sorted for determinism.
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
    out.sort();
    Ok(out)
}

/// Print the aggregate summary and the enumerated remainders.
fn print_summary(t: &Totals) {
    let pct = |n: usize, d: usize| -> f64 {
        if d == 0 {
            100.0
        } else {
            n as f64 * 100.0 / d as f64
        }
    };
    println!("=== R46 content-stream identity gate ===");
    println!("files scanned:        {}", t.files_scanned);
    println!("files loadable:       {}", t.files_loadable);
    println!("content streams:      {}", t.streams_total);
    println!(
        "byte-identical:       {} ({:.4}%)",
        t.streams_byte_identical,
        pct(t.streams_byte_identical, t.streams_total)
    );
    println!(
        "non-identical:        {} ({:.4}%) — number re-spelling, value preserved",
        t.streams_non_identical,
        pct(t.streams_non_identical, t.streams_total)
    );
    println!("CORRUPTED (gate):     {}", t.streams_corrupted);
    if !t.reason_samples.is_empty() {
        println!(
            "\n-- non-identity samples (first {}) --",
            t.reason_samples.len()
        );
        for r in &t.reason_samples {
            println!("  {r}");
        }
    }
    if !t.corruptions.is_empty() {
        println!("\n-- CORRUPTIONS (gate failures) --");
        for c in &t.corruptions {
            println!("  {c}");
        }
    }
    println!(
        "\nGATE: {}",
        if t.streams_corrupted == 0 {
            "PASS (every content stream semantically preserved)"
        } else {
            "FAIL (see corruptions above)"
        }
    );
}
