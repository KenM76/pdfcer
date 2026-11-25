//! A preflight report as JSON for a program to read.

use super::profile::quote;
use super::report_text::{Outcome, finding_text, fixups_of, rule_findings};
use super::{Finding, FindingKind, FindingTarget, PreflightReport, Profile, Severity};

/// The report as one JSON object (RFC 8259), carrying what
/// [`super::report_text`] says in fields a program can read.
///
/// The verdict rule and every finding fact are the text report's:
///
/// - `profile`, `description`, `pages_checked`, `page_count`, `cancelled`,
///   `dropped` (findings past the result cap, not kept).
/// - `verdict`: `fails` (an error-severity finding, or dropped findings),
///   `needs-review` (no error, but any finding remains or the run was
///   cancelled) or `passes`; the text report's `FAILS`, `needs review` and
///   `passes`.
/// - `counts`: `error`, `warning`, `info`, `violation`, `needs_person`,
///   `incomplete`.
/// - `rules`: every profile rule in order, each with `id`, `severity`,
///   `findings` (its count), `fixups` (the names the profile gives) and
///   `suggested_fixup` (pdfcer's suggestion when the rule names none, else
///   `null`).
/// - `findings`: every kept finding in report order, each with `rule`,
///   `severity`, `kind` (`violation`, `needs-person`, `incomplete`),
///   `page` (1-based, `null` for the document), `target`, `rect`
///   (`[llx, lly, urx, ury]` or `null`), `measured` (`value`, `limit`,
///   `unit`, or `null`), `message` and `text`, the text report's line for
///   it.
/// - `target` is an object whose `kind` is `document`, `page`, `object` or
///   `leaf` (with `index`), or `annotation`, `font`, `colour-space`,
///   `image`, `graphics-state` or `struct-elem` (with `num` and
///   `generation`).
///
/// A number that is not finite is written `null`. Pure: no I/O, the same
/// report always gives the same text.
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
/// let json = preflight::report_json(&report, &profile);
/// assert!(json.starts_with("{\n  \"profile\": "));
/// # Ok(())
/// # }
/// ```
#[must_use]
pub fn report_json(report: &PreflightReport, profile: &Profile) -> String {
    let rules: Vec<String> = profile
        .rules
        .iter()
        .map(|rule| {
            let (names, suggested) = fixups_of(rule);
            let names: Vec<String> = names.iter().map(|n| quote(n)).collect();
            format!(
                "{{\"id\": {}, \"severity\": {}, \"findings\": {}, \"fixups\": [{}], \
                 \"suggested_fixup\": {}}}",
                quote(&rule.id),
                quote(rule.severity.as_str()),
                rule_findings(report, rule).len(),
                names.join(", "),
                suggested.map_or_else(|| "null".to_owned(), quote)
            )
        })
        .collect();
    let findings: Vec<String> = report.findings.iter().map(finding_json).collect();
    format!(
        "{{\n  \"profile\": {},\n  \"description\": {},\n  \"pages_checked\": {},\n  \
         \"page_count\": {},\n  \"cancelled\": {},\n  \"dropped\": {},\n  \"verdict\": {},\n  \
         \"counts\": {},\n  \"rules\": [{}],\n  \"findings\": [{}]\n}}\n",
        quote(&profile.name),
        quote(&profile.description),
        report.pages_checked,
        report.page_count,
        report.cancelled,
        report.dropped,
        quote(Outcome::of(report).json()),
        counts(report),
        list(&rules),
        list(&findings)
    )
}

fn list(items: &[String]) -> String {
    if items.is_empty() {
        String::new()
    } else {
        format!("\n    {}\n  ", items.join(",\n    "))
    }
}

fn counts(report: &PreflightReport) -> String {
    let sev = |s: Severity| report.findings.iter().filter(|f| f.severity == s).count();
    let kind = |k: FindingKind| report.findings.iter().filter(|f| f.kind == k).count();
    format!(
        "{{\"error\": {}, \"warning\": {}, \"info\": {}, \"violation\": {}, \
         \"needs_person\": {}, \"incomplete\": {}}}",
        sev(Severity::Error),
        sev(Severity::Warning),
        sev(Severity::Info),
        kind(FindingKind::Violation),
        kind(FindingKind::NeedsPerson),
        kind(FindingKind::Incomplete)
    )
}

fn finding_json(f: &Finding) -> String {
    let kind = match f.kind {
        FindingKind::Violation => "violation",
        FindingKind::NeedsPerson => "needs-person",
        FindingKind::Incomplete => "incomplete",
    };
    let rect = f.rect.map_or_else(
        || "null".to_owned(),
        |r| {
            format!(
                "[{}, {}, {}, {}]",
                num(r.llx),
                num(r.lly),
                num(r.urx),
                num(r.ury)
            )
        },
    );
    let measured = f.measured.map_or_else(
        || "null".to_owned(),
        |m| {
            format!(
                "{{\"value\": {}, \"limit\": {}, \"unit\": {}}}",
                num(m.value),
                num(m.limit),
                quote(m.unit.as_str())
            )
        },
    );
    format!(
        "{{\"rule\": {}, \"severity\": {}, \"kind\": {}, \"page\": {}, \"target\": {}, \
         \"rect\": {rect}, \"measured\": {measured}, \"message\": {}, \"text\": {}}}",
        quote(&f.rule_id),
        quote(f.severity.as_str()),
        quote(kind),
        f.page.map_or_else(|| "null".to_owned(), |p| p.to_string()),
        target_json(f.target),
        quote(&f.message),
        quote(&finding_text(f))
    )
}

fn target_json(t: FindingTarget) -> String {
    let (kind, id) = match t {
        FindingTarget::Document => return "{\"kind\": \"document\"}".to_owned(),
        FindingTarget::Page => return "{\"kind\": \"page\"}".to_owned(),
        FindingTarget::PageObject(i) => return format!("{{\"kind\": \"object\", \"index\": {i}}}"),
        FindingTarget::FormLeaf(i) => return format!("{{\"kind\": \"leaf\", \"index\": {i}}}"),
        FindingTarget::Annotation(id) => ("annotation", id),
        FindingTarget::Font(id) => ("font", id),
        FindingTarget::ColourSpace(id) => ("colour-space", id),
        FindingTarget::Image(id) => ("image", id),
        FindingTarget::GraphicsState(id) => ("graphics-state", id),
        FindingTarget::StructElem(id) => ("struct-elem", id),
    };
    format!(
        "{{\"kind\": \"{kind}\", \"num\": {}, \"generation\": {}}}",
        id.num, id.generation
    )
}

fn num(v: f64) -> String {
    if v.is_finite() {
        format!("{v}")
    } else {
        "null".to_owned()
    }
}
