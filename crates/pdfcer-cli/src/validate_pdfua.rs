//! `validate-pdfua`: check a document against ISO 14289 (PDF/UA-1 or -2).

use super::*;

use pdfcer_core::preflight::{
    self, FindingKind, PdfuaPart, PdfuaReport, Requirement, validate_pdfua,
};

use crate::extract::json_escape;
use crate::preflight::{finding_line, target};

/// `validate-pdfua` — read-only.
///
/// ## Contract
///
/// - Text: one header line,
///   `validate-pdfua <input> part=<1|2> pages=<checked>/<count> violations=<v>
///   incomplete=<i> needs-person=<p> not-checked=<n> verdict=<verdict>`,
///   where `<verdict>` is `fails`, `unconfirmed` or `conforms`
///   (`PdfuaReport::verdict`: any error fails; otherwise any finding,
///   unchecked requirement or unexamined page leaves it unconfirmed), then
///   one line per finding in the `preflight` line format, then one
///   `not-checked rule=<id> "<clause>"` line per requirement no check covers.
/// - `--json`: one object with `input`, `part`, `pages_checked`,
///   `page_count`, `verdict` (as in the text header), `findings` (each with
///   `rule`, `kind`, `severity`, `page`, `target`, `clause`, `matterhorn`,
///   `message`) and `not_checked` (each with `rule`, `clause`, `matterhorn`,
///   `decided_by`).
/// - Exit `PREFLIGHT_FAILED` (18) when any violation or incomplete is found;
///   needs-a-person findings alone do not fail. The usual read codes for an
///   unreadable input.
pub(crate) fn cmd_validate_pdfua(input: &Path, part: u8, json: bool) -> u8 {
    let Some(part) = PdfuaPart::from_number(part) else {
        eprintln!("pdfcer: --part must be 1 or 2");
        return exit::RUNTIME_ERROR;
    };
    let doc = match open_for_read(input) {
        Ok(doc) => doc,
        Err(code) => return code,
    };
    let report = validate_pdfua(&doc.view(), part);
    if json {
        print!("{}", report_json(input, &report));
    } else {
        print_text(input, &report);
    }
    if report.fails() {
        exit::PREFLIGHT_FAILED
    } else {
        exit::SUCCESS
    }
}

fn count(report: &PdfuaReport, kind: FindingKind) -> usize {
    report.findings.iter().filter(|f| f.kind == kind).count()
}

fn print_text(input: &Path, report: &PdfuaReport) {
    println!(
        "validate-pdfua {} part={} pages={}/{} violations={} incomplete={} needs-person={} \
         not-checked={} verdict={}",
        input.display(),
        report.part.number(),
        report.pages_checked,
        report.page_count,
        count(report, FindingKind::Violation),
        count(report, FindingKind::Incomplete),
        count(report, FindingKind::NeedsPerson),
        report.not_checked.len(),
        report.verdict().as_str()
    );
    for f in &report.findings {
        println!("{}", finding_line(f));
    }
    for r in &report.not_checked {
        println!("not-checked rule={} \"{}\"", r.id, r.clause);
    }
}

fn opt(v: Option<&str>) -> String {
    v.map_or_else(|| "null".to_owned(), |s| format!("\"{}\"", json_escape(s)))
}

/// One finding as a JSON object; shared with `fix-pdfua`.
pub(crate) fn finding_json(f: &preflight::Finding) -> String {
    let kind = match f.kind {
        FindingKind::Violation => "violation",
        FindingKind::NeedsPerson => "needs-person",
        FindingKind::Incomplete => "incomplete",
    };
    let req = preflight::requirement(&f.rule_id);
    format!(
        "{{\"rule\": \"{}\", \"kind\": \"{kind}\", \"severity\": \"{}\", \"page\": {}, \
         \"target\": \"{}\", \"clause\": {}, \"matterhorn\": {}, \"message\": \"{}\"}}",
        json_escape(&f.rule_id),
        f.severity.as_str(),
        f.page.map_or_else(|| "null".to_owned(), |p| p.to_string()),
        target(f.target),
        opt(req.map(|r| r.clause)),
        opt(req.and_then(|r| r.matterhorn)),
        json_escape(&f.message)
    )
}

fn requirement_json(r: &Requirement) -> String {
    let by = match r.decided_by {
        preflight::DecidedBy::Machine => "machine",
        preflight::DecidedBy::Person => "person",
        preflight::DecidedBy::NoTest => "no-test",
    };
    format!(
        "{{\"rule\": \"{}\", \"clause\": \"{}\", \"matterhorn\": {}, \"decided_by\": \"{by}\"}}",
        r.id,
        json_escape(r.clause),
        opt(r.matterhorn)
    )
}

fn report_json(input: &Path, report: &PdfuaReport) -> String {
    let findings: Vec<String> = report.findings.iter().map(finding_json).collect();
    let skipped: Vec<String> = report
        .not_checked
        .iter()
        .map(|r| requirement_json(r))
        .collect();
    format!(
        "{{\n  \"input\": \"{}\",\n  \"part\": {},\n  \"pages_checked\": {},\n  \
         \"page_count\": {},\n  \"verdict\": \"{}\",\n  \"findings\": [\n    {}\n  ],\n  \
         \"not_checked\": [\n    {}\n  ]\n}}\n",
        json_escape(&input.display().to_string()),
        report.part.number(),
        report.pages_checked,
        report.page_count,
        report.verdict().as_str(),
        findings.join(",\n    "),
        skipped.join(",\n    ")
    )
}
