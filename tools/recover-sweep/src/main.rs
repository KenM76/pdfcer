//! recover-sweep — decision 013 Pass B cross-reference-recovery measurement.
//!
//! Walks each directory argument for `*.pdf`, LOADS each file through
//! `pdfcer_core::document::Document::from_bytes` (which now routes an
//! unparseable cross-reference through rebuild-by-scan recovery), and
//! classifies the outcome:
//!
//! - **clean** — loaded via the strict path (`recovery() == None`);
//! - **recovered** — loaded via rebuild-by-scan (`recovery() == Some`),
//!   tallied by the originating `RecoveryReason`;
//! - **encrypted** — the deliberate `/Encrypt` capability gap;
//! - **recovery-refused** — recovery ran but fail-cleaned (no catalog / no
//!   objects / cap);
//! - **still-failing** — a load error recovery does not address (object
//!   body corruption after a clean xref, etc.), by normalized error kind;
//! - **panic / timeout** — a robustness bug (there should be none).
//!
//! Because recovery fires ONLY on the strict-load error path, the
//! **recovered** tally IS the converted-by-Pass-B set (every one failed
//! strict load before this Pass), and any **clean** file is one that always
//! loaded — so a recovered file appearing where a clean load was expected
//! would be the zero-regression violation. Reports per-directory and total.
//!
//! Usage: `cargo run --release -- <dir> [<dir> ...]`

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use pdfcer_core::document::{DocError, Document};
use pdfcer_core::recover::{RecoverError, RecoveryReason};
use pdfcer_core::xref::XrefErrorKind;

/// Per-file load budget. Load is O(n) with resource guards, so this only
/// catches a robustness bug (an infinite loop would be a Pass-B finding).
const FILE_BUDGET: Duration = Duration::from_secs(20);

#[derive(Default)]
struct Tally {
    clean: usize,
    recovered: BTreeMap<String, usize>,
    encrypted: usize,
    recovery_refused: BTreeMap<String, usize>,
    still_failing: BTreeMap<String, usize>,
    panics: usize,
    timeouts: usize,
    total: usize,
}

impl Tally {
    fn add(&mut self, other: &Tally) {
        self.clean += other.clean;
        self.encrypted += other.encrypted;
        self.panics += other.panics;
        self.timeouts += other.timeouts;
        self.total += other.total;
        for (k, v) in &other.recovered {
            *self.recovered.entry(k.clone()).or_default() += v;
        }
        for (k, v) in &other.recovery_refused {
            *self.recovery_refused.entry(k.clone()).or_default() += v;
        }
        for (k, v) in &other.still_failing {
            *self.still_failing.entry(k.clone()).or_default() += v;
        }
    }

    fn recovered_total(&self) -> usize {
        self.recovered.values().sum()
    }
    fn refused_total(&self) -> usize {
        self.recovery_refused.values().sum()
    }
    fn failing_total(&self) -> usize {
        self.still_failing.values().sum()
    }

    fn print(&self, label: &str) {
        println!("=== {label} ===");
        println!("  total files           {}", self.total);
        println!("  clean (strict load)   {}", self.clean);
        println!(
            "  RECOVERED (converted) {}   <- previously failed strict load",
            self.recovered_total()
        );
        for (reason, n) in &self.recovered {
            println!("      by reason: {reason:28} {n}");
        }
        println!("  encrypted (refused)   {}", self.encrypted);
        println!("  recovery-refused      {}", self.refused_total());
        for (k, n) in &self.recovery_refused {
            println!("      {k:32} {n}");
        }
        println!("  still-failing         {}", self.failing_total());
        for (k, n) in &self.still_failing {
            println!("      {k:32} {n}");
        }
        if self.panics > 0 || self.timeouts > 0 {
            println!("  !! panics {}  timeouts {}", self.panics, self.timeouts);
        }
        println!();
    }
}

/// Classify one file's load outcome into a tally bucket, optionally
/// recording the path of a recovered file (for the `*-fail-*`
/// reconciliation enumeration) when `path` is `Some`.
fn classify_path(bytes: Vec<u8>, tally: &mut Tally, path: Option<&Path>) {
    let result =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| Document::from_bytes(bytes)));
    if let Ok(Ok(doc)) = &result
        && let Some(report) = doc.recovery()
        && let Some(p) = path
    {
        // Machine-readable line for the reconciliation gate.
        println!("RECOVERED\t{:?}\t{}", report.reason, p.display());
    }
    match result {
        Err(_) => tally.panics += 1,
        Ok(Ok(doc)) => match doc.recovery() {
            None => tally.clean += 1,
            Some(report) => {
                *tally
                    .recovered
                    .entry(reason_label(report.reason))
                    .or_default() += 1;
            }
        },
        Ok(Err(e)) => classify_error(&e, tally),
    }
}

fn classify_error(e: &DocError, tally: &mut Tally) {
    match e {
        DocError::Xref(x) if matches!(x.kind, XrefErrorKind::EncryptionUnsupported) => {
            tally.encrypted += 1;
        }
        DocError::Recovery(r) => {
            let k = match r {
                RecoverError::NoObjects => "NoObjects",
                RecoverError::NoCatalog => "NoCatalog",
                RecoverError::Encrypted => "Encrypted",
                RecoverError::TooManyEntries => "TooManyEntries",
                _ => "OtherRecoverError",
            };
            *tally.recovery_refused.entry(k.to_string()).or_default() += 1;
        }
        other => {
            *tally
                .still_failing
                .entry(doc_error_kind(other))
                .or_default() += 1;
        }
    }
}

fn reason_label(r: RecoveryReason) -> String {
    format!("{r:?}")
}

/// A normalized, low-cardinality label for a still-failing DocError.
fn doc_error_kind(e: &DocError) -> String {
    match e {
        DocError::Io(_) => "Io".into(),
        DocError::Header(_) => "Header(not-a-pdf)".into(),
        DocError::Xref(x) => format!("Xref::{}", xref_kind_label(&x.kind)),
        DocError::BadObject { .. } => "BadObject".into(),
        DocError::ObjectIdMismatch { .. } => "ObjectIdMismatch".into(),
        DocError::ObjectStreamMissing { .. } => "ObjectStreamMissing".into(),
        DocError::ObjectStream { .. } => "ObjectStream".into(),
        DocError::ObjectStreamIdMismatch { .. } => "ObjectStreamIdMismatch".into(),
        DocError::NoCatalog => "NoCatalog(trailer)".into(),
        DocError::Recovery(_) => "Recovery".into(),
        _ => "Other".into(),
    }
}

fn xref_kind_label(k: &XrefErrorKind) -> String {
    match k {
        XrefErrorKind::StartxrefNotFound => "StartxrefNotFound",
        XrefErrorKind::BadStartxrefOffset => "BadStartxrefOffset",
        XrefErrorKind::NotAnXrefSection => "NotAnXrefSection",
        XrefErrorKind::BadXrefStream(_) => "BadXrefStream",
        XrefErrorKind::XrefStreamDecode(_) => "XrefStreamDecode",
        XrefErrorKind::EncryptionUnsupported => "EncryptionUnsupported",
        XrefErrorKind::BadSubsectionHeader => "BadSubsectionHeader",
        XrefErrorKind::BadEntry => "BadEntry",
        XrefErrorKind::TooManyEntries => "TooManyEntries",
        XrefErrorKind::BadTrailer(_) => "BadTrailer",
        XrefErrorKind::PrevChainCycle => "PrevChainCycle",
        XrefErrorKind::Parse(_) => "Parse",
        _ => "Other",
    }
    .to_string()
}

/// Recursively collect `*.pdf` paths under `dir` (sorted for determinism).
fn collect_pdfs(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    paths.sort();
    for p in paths {
        if p.is_dir() {
            collect_pdfs(&p, out);
        } else if p.extension().is_some_and(|e| e.eq_ignore_ascii_case("pdf")) {
            out.push(p);
        }
    }
}

fn sweep_dir(dir: &Path) -> Tally {
    let mut files = Vec::new();
    collect_pdfs(dir, &mut files);
    let mut tally = Tally::default();
    for path in files {
        tally.total += 1;
        let Ok(bytes) = std::fs::read(&path) else {
            *tally.still_failing.entry("Io(read)".into()).or_default() += 1;
            continue;
        };
        // Per-file budget on a worker thread so a robustness bug is a
        // counted timeout rather than a hung sweep.
        let list = std::env::var_os("PDFCER_LIST_RECOVERED").is_some();
        let path_for_thread = list.then(|| path.clone());
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut t = Tally::default();
            classify_path(bytes, &mut t, path_for_thread.as_deref());
            let _ = tx.send(t);
        });
        match rx.recv_timeout(FILE_BUDGET) {
            Ok(t) => tally.add(&t),
            Err(_) => tally.timeouts += 1,
        }
    }
    tally
}

fn main() {
    let dirs: Vec<PathBuf> = std::env::args().skip(1).map(PathBuf::from).collect();
    if dirs.is_empty() {
        eprintln!("usage: recover-sweep <dir> [<dir> ...]");
        std::process::exit(2);
    }
    let mut grand = Tally::default();
    for dir in &dirs {
        let t = sweep_dir(dir);
        t.print(&dir.display().to_string());
        grand.add(&t);
    }
    if dirs.len() > 1 {
        grand.print("GRAND TOTAL");
    }
}
