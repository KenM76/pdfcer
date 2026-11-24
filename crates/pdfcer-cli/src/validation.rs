//! The report shape `validate-pdfx` and `validate-pdfa` share.

use std::path::Path;

use pdfcer_core::preflight::{Finding, PreflightReport};

use crate::extract::json_escape;
use crate::preflight::{finding_line, kind_name, target};

/// The parts of a standard-validation report the CLI prints.
pub(crate) struct Validation<'a> {
    /// The subcommand, leading the header line.
    pub(crate) verb: &'a str,
    pub(crate) input: &'a Path,
    pub(crate) level: &'a str,
    /// What the file claims, when it claims something readable.
    pub(crate) claim: Option<String>,
    pub(crate) verdict: &'a str,
    pub(crate) report: &'a PreflightReport,
}

/// The header line
/// (`<verb> <input> level=… claim="…|-" verdict=… findings=n dropped=d`),
/// then one `preflight` line per finding with ` clause="…"` before its
/// sentence.
pub(crate) fn print_validation(v: &Validation<'_>, clause_of: impl Fn(&Finding) -> Option<String>) {
    println!(
        "{} {} level={} claim=\"{}\" verdict={} findings={} dropped={}",
        v.verb,
        v.input.display(),
        v.level,
        v.claim.as_deref().unwrap_or("-"),
        v.verdict,
        v.report.findings.len(),
        v.report.dropped
    );
    for f in &v.report.findings {
        let line = finding_line(f);
        let clause = clause_of(f).unwrap_or_default();
        match line.find(" \"") {
            Some(at) => println!("{} clause=\"{clause}\"{}", &line[..at], &line[at..]),
            None => println!("{line}"),
        }
    }
}

/// The `--json` object: `{input, level, claim, verdict, dropped, findings:
/// [{rule, severity, kind, page, target, clause, message}]}`.
pub(crate) fn validation_json(
    v: &Validation<'_>,
    clause_of: impl Fn(&Finding) -> Option<String>,
) -> String {
    let quote = |s: &str| format!("\"{}\"", json_escape(s));
    let findings: Vec<String> = v
        .report
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
                quote(&clause_of(f).unwrap_or_default()),
                quote(&f.message)
            )
        })
        .collect();
    let claim = v.claim.as_deref().map_or_else(|| "null".to_owned(), quote);
    format!(
        "{{\"input\": {}, \"level\": \"{}\", \"claim\": {claim}, \"verdict\": \"{}\", \
         \"dropped\": {}, \"findings\": [{}]}}",
        quote(&v.input.display().to_string()),
        v.level,
        v.verdict,
        v.report.dropped,
        findings.join(", ")
    )
}
