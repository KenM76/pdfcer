//! `validate-pdfx`: check a document against one PDF/X level (ISO 15930).

use super::*;

use pdfcer_core::preflight::{self, PdfxLevel, PdfxReport, RunOptions};

use crate::validation::{Validation, print_validation, validation_json};

/// The `--level` value: an ISO 15930 level name.
pub(crate) fn parse_pdfx_level(s: &str) -> Result<PdfxLevel, String> {
    PdfxLevel::from_name(s).ok_or_else(|| {
        let names: Vec<_> = PdfxLevel::ALL.iter().map(|l| l.as_str()).collect();
        format!(
            "not a PDF/X level; use x-1a, x-3, x-4 or one of {}",
            names.join(", ")
        )
    })
}

/// `validate-pdfx` — read-only.
///
/// ## Contract
///
/// - One header line,
///   `validate-pdfx <input> level=<level> claim="<claimed level>|-" verdict=<fails|unconfirmed|conforms> findings=<n> dropped=<d>`,
///   then one `preflight`-style line per finding with ` clause="<clause>"`
///   inserted before the sentence.
/// - `--json`: one object `{input, level, claim, verdict, dropped, findings:
///   [{rule, severity, kind, page, target, clause, message}]}` instead.
/// - Exit `PREFLIGHT_FAILED` (18) on a `fails` verdict; 0 otherwise, so an
///   `unconfirmed` file (only questions for a person or unchecked rows)
///   exits 0 and its findings say what is left. The usual read codes for an
///   unreadable input.
pub(crate) fn cmd_validate_pdfx(
    input: &Path,
    level: PdfxLevel,
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
    let report = preflight::validate_pdfx_with(&doc.view(), level, &options);
    let head = Validation {
        verb: "validate-pdfx",
        input,
        level: report.level.as_str(),
        claim: report.claim.as_ref().map(|c| c.level.clone()),
        verdict: report.verdict().as_str(),
        report: &report.preflight,
    };
    let clause_of = |f: &_| report.clause_of(f);
    if json {
        println!("{}", validation_json(&head, clause_of));
    } else {
        print_validation(&head, clause_of);
    }
    finish(&report)
}

fn finish(report: &PdfxReport) -> u8 {
    if report.preflight.fails() {
        exit::PREFLIGHT_FAILED
    } else {
        exit::SUCCESS
    }
}
