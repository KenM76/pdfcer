//! A preflight report as plain text for a person to read or file.

use super::fixup::{Fixup, rule_matches};
use super::{Finding, FindingKind, FindingTarget, PreflightReport, Profile, Severity};

/// The report as plain text, grouped by rule in profile order.
///
/// The layout: a header (profile, its description, pages checked, the
/// verdict and the counts), then per rule with findings a
/// `Rule <id> (<severity>): <n> finding(s)` line, the fixup it names or
/// pdfcer suggests, and one indented line per finding
/// (`page 3, object 2: <sentence> [412 ppi, limit 300]`), then the rules
/// that found nothing. The verdict is `FAILS`, `needs review` (no error,
/// but warnings, needs-a-person or incomplete findings), or `passes`; a
/// cancelled run or dropped findings are stated, never hidden. Pure: no
/// I/O, the same report always gives the same text.
///
/// # Examples
///
/// ```
/// use pdfcer_core::document::Document;
/// use pdfcer_core::preflight::{self, Profile};
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let doc = Document::from_bytes(
///     include_bytes!("../../../../fixtures/synthetic/hello.pdf").to_vec(),
/// )?;
/// let profile = Profile::digital_printing();
/// let report = preflight::run(&doc.view(), &profile);
/// let text = preflight::report_text(&report, &profile);
/// assert!(text.starts_with("Preflight report: "));
/// # Ok(())
/// # }
/// ```
#[must_use]
pub fn report_text(report: &PreflightReport, profile: &Profile) -> String {
    let mut out = header(report, profile);
    let mut quiet = Vec::new();
    for rule in &profile.rules {
        let found: Vec<&Finding> = report
            .findings
            .iter()
            .filter(|f| rule_matches(&f.rule_id, &rule.id))
            .collect();
        if found.is_empty() {
            quiet.push(rule.id.as_str());
            continue;
        }
        out.push_str(&format!(
            "\nRule {} ({}): {} finding(s)\n",
            rule.id,
            rule.severity.as_str(),
            found.len()
        ));
        let names: Vec<&str> = rule.fixups.iter().map(Fixup::name).collect();
        if !names.is_empty() {
            out.push_str(&format!("  fixup: {}\n", names.join(", ")));
        } else if let Some(s) = Fixup::suggested(&rule.check) {
            out.push_str(&format!("  suggested fixup: {}\n", s.name()));
        }
        for f in found {
            out.push_str(&format!("  {}\n", finding_text(f)));
        }
    }
    if !quiet.is_empty() {
        out.push_str(&format!("\nNo findings: {}\n", quiet.join(", ")));
    }
    out
}

fn header(report: &PreflightReport, profile: &Profile) -> String {
    let count = |s: Severity| report.findings.iter().filter(|f| f.severity == s).count();
    let kind = |k: FindingKind| report.findings.iter().filter(|f| f.kind == k).count();
    let verdict = if report.fails() {
        "FAILS"
    } else if report.findings.is_empty() && !report.cancelled {
        "passes"
    } else {
        "needs review"
    };
    let mut out = format!("Preflight report: {}\n", profile.name);
    if !profile.description.is_empty() {
        out.push_str(&format!("{}\n", profile.description));
    }
    out.push_str(&format!(
        "Pages checked: {} of {}{}\n",
        report.pages_checked,
        report.page_count,
        if report.cancelled { " (cancelled)" } else { "" }
    ));
    out.push_str(&format!("Result: {verdict}\n"));
    out.push_str(&format!(
        "Findings: {} error, {} warning, {} info; {} violation, {} needs a person, {} incomplete\n",
        count(Severity::Error),
        count(Severity::Warning),
        count(Severity::Info),
        kind(FindingKind::Violation),
        kind(FindingKind::NeedsPerson),
        kind(FindingKind::Incomplete)
    ));
    if report.dropped > 0 {
        out.push_str(&format!(
            "Dropped: {} finding(s) past the result cap were not kept\n",
            report.dropped
        ));
    }
    out
}

fn finding_text(f: &Finding) -> String {
    let place = f
        .page
        .map_or_else(|| "document".to_owned(), |p| format!("page {p}"));
    let target = match f.target {
        FindingTarget::Document | FindingTarget::Page => String::new(),
        FindingTarget::PageObject(i) => format!(", object {i}"),
        FindingTarget::FormLeaf(i) => format!(", form leaf {i}"),
        FindingTarget::Annotation(id) => format!(", annotation {id}"),
        FindingTarget::Font(id) => format!(", font {id}"),
        FindingTarget::ColourSpace(id) => format!(", colour space {id}"),
        FindingTarget::Image(id) => format!(", image {id}"),
        FindingTarget::GraphicsState(id) => format!(", graphics state {id}"),
        FindingTarget::StructElem(id) => format!(", structure element {id}"),
    };
    let kind = match f.kind {
        FindingKind::Violation => "",
        FindingKind::NeedsPerson => " (a person must check)",
        FindingKind::Incomplete => " (could not be checked)",
    };
    let measured = f.measured.map_or_else(String::new, |m| {
        format!(
            " [{:.0} {u}, limit {:.0} {u}]",
            m.value,
            m.limit,
            u = m.unit.as_str()
        )
    });
    format!("{place}{target}: {}{kind}{measured}", f.message)
}
