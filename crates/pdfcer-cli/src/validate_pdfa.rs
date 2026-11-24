//! `validate-pdfa`: check a document against one PDF/A level (ISO 19005).

use super::*;

use pdfcer_core::font_unembed::PdfaClaim;
use pdfcer_core::preflight::{self, PdfaLevel, PdfaReport, RunOptions};

use crate::validation::{Validation, print_validation, validation_json};

/// The `--level` value: `1b`, `2b` or `3b`.
pub(crate) fn parse_pdfa_level(s: &str) -> Result<PdfaLevel, String> {
    PdfaLevel::from_name(s).ok_or_else(|| {
        let names: Vec<_> = PdfaLevel::ALL.iter().map(|l| l.as_str()).collect();
        format!(
            "not a PDF/A level pdfcer validates; use {}",
            names.join(", ")
        )
    })
}

/// `validate-pdfa` — read-only.
///
/// ## Contract
///
/// - One header line,
///   `validate-pdfa <input> level=<level> claim="<part><conformance>|-" verdict=<fails|unconfirmed|conforms> findings=<n> dropped=<d>`,
///   then one `preflight`-style line per finding with ` clause="<clause>"`
///   inserted before the sentence.
/// - `--json`: one object `{input, level, claim, verdict, dropped, findings:
///   [{rule, severity, kind, page, target, clause, message}]}` instead.
/// - Exit `PREFLIGHT_FAILED` (18) on a `fails` verdict; 0 otherwise. An
///   `unconfirmed` file exits 0 and its `incomplete` findings name what
///   was not checked. The usual read codes for an unreadable input.
pub(crate) fn cmd_validate_pdfa(
    input: &Path,
    level: PdfaLevel,
    json: bool,
    max_findings: usize,
) -> u8 {
    let doc = match open_for_read(input) {
        Ok(doc) => doc,
        Err(code) => return code,
    };
    let options = RunOptions {
        max_findings,
        ..RunOptions::default()
    };
    let report = preflight::validate_pdfa_with(&doc.view(), level, &options);
    let head = Validation {
        verb: "validate-pdfa",
        input,
        level: report.level.as_str(),
        claim: claim(&report),
        verdict: report.verdict().as_str(),
        report: &report.preflight,
    };
    let clause_of = |f: &_| report.clause_of(f);
    if json {
        println!("{}", validation_json(&head, clause_of));
    } else {
        print_validation(&head, clause_of);
    }
    if report.preflight.fails() {
        exit::PREFLIGHT_FAILED
    } else {
        exit::SUCCESS
    }
}

/// What the file claims, as `2b`; `None` when it claims nothing readable.
fn claim(report: &PdfaReport) -> Option<String> {
    match &report.claim {
        PdfaClaim::Identified { part, conformance } => Some(format!(
            "{}{}",
            part.as_deref().unwrap_or("?"),
            conformance.as_deref().unwrap_or("").to_ascii_lowercase()
        )),
        _ => None,
    }
}
