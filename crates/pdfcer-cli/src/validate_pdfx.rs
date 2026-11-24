//! `validate-pdfx`: check a document against one PDF/X level (ISO 15930).

use super::*;

use pdfcer_core::preflight::{self, PdfxLevel, PdfxReport, RunOptions};

use crate::extract::json_escape;
use crate::preflight::{finding_line, kind_name, target};

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
    if json {
        println!("{}", report_json(input, &report));
    } else {
        print_text(input, &report);
    }
    if report.preflight.fails() {
        exit::PREFLIGHT_FAILED
    } else {
        exit::SUCCESS
    }
}

fn print_text(input: &Path, report: &PdfxReport) {
    let claim = report.claim.as_ref().map_or("-", |c| c.level.as_str());
    println!(
        "validate-pdfx {} level={} claim=\"{claim}\" verdict={} findings={} dropped={}",
        input.display(),
        report.level.as_str(),
        report.verdict().as_str(),
        report.preflight.findings.len(),
        report.preflight.dropped
    );
    for f in &report.preflight.findings {
        let line = finding_line(f);
        let clause = report.clause_of(f).unwrap_or_default();
        match line.find(" \"") {
            Some(at) => println!("{} clause=\"{clause}\"{}", &line[..at], &line[at..]),
            None => println!("{line}"),
        }
    }
}

fn report_json(input: &Path, report: &PdfxReport) -> String {
    let quote = |s: &str| format!("\"{}\"", json_escape(s));
    let findings: Vec<String> = report
        .preflight
        .findings
        .iter()
        .map(|f| {
            let page = f.page.map_or_else(|| "null".to_owned(), |p| p.to_string());
            format!(
                "{{\"rule\": {}, \"severity\": \"{}\", \"kind\": \"{}\", \"page\": {page}, \
                 \"target\": {}, \"clause\": {}, \"message\": {}}}",
                quote(&f.rule_id),
                f.severity.as_str(),
                kind_name(f.kind),
                quote(&target(f.target)),
                quote(&report.clause_of(f).unwrap_or_default()),
                quote(&f.message)
            )
        })
        .collect();
    let claim = report
        .claim
        .as_ref()
        .map_or_else(|| "null".to_owned(), |c| quote(&c.level));
    format!(
        "{{\"input\": {}, \"level\": \"{}\", \"claim\": {claim}, \"verdict\": \"{}\", \
         \"dropped\": {}, \"findings\": [{}]}}",
        quote(&input.display().to_string()),
        report.level.as_str(),
        report.verdict().as_str(),
        report.preflight.dropped,
        findings.join(", ")
    )
}
