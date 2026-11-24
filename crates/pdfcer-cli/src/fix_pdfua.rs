//! `fix-pdfua`: apply the PDF/UA fixups, then re-validate.

use super::*;

use pdfcer_core::edit::{FixPdfuaOptions, FixPdfuaReport};
use pdfcer_core::preflight::{FindingKind, PdfuaPart};

use crate::extract::json_escape;
use crate::preflight::finding_line;
use crate::validate_pdfua::finding_json;

/// The `fix-pdfua` arguments after parsing.
pub(crate) struct FixPdfuaArgs<'a> {
    /// The PDF to fix.
    pub(crate) input: &'a Path,
    /// ISO 14289 part, 1 or 2.
    pub(crate) part: u8,
    /// `--lang`.
    pub(crate) lang: Option<&'a str>,
    /// `--title`.
    pub(crate) title: Option<&'a str>,
    /// `--json`.
    pub(crate) json: bool,
    /// `--output`.
    pub(crate) output: &'a Path,
    /// `--mode`.
    pub(crate) mode: SaveMode,
    /// `--verify-undo`.
    pub(crate) verify_undo: bool,
}

/// `fix-pdfua` — `EditSession::fix_pdfua`, one undo step.
///
/// ## Contract
///
/// - Text: one `fix-pdfua …` summary line, then
///   `fixup <name>: <applied|already-done|refused|skipped> — <detail>` per
///   fixup in the order run, `disclosure: …` per inference, one
///   `verdict: …` line, then the remaining findings in the `preflight` line
///   format.
/// - `--json`: one object with `input`, `output`, `part`, `fixups` (each
///   `fixup`, `outcome`, `detail`), `disclosures`, `identified`, `passes`
///   and `findings`.
/// - The output is saved whether or not the file now passes. Exit
///   `PREFLIGHT_FAILED` (18) when the re-validation still finds a violation
///   or incomplete; otherwise [`finish_edit`]'s code.
pub(crate) fn cmd_fix_pdfua(a: &FixPdfuaArgs<'_>) -> u8 {
    let Some(part) = PdfuaPart::from_number(a.part) else {
        eprintln!("pdfcer: --part must be 1 or 2");
        return exit::RUNTIME_ERROR;
    };
    let (source, mut session) = match open_for_edit(a.input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let mut options = FixPdfuaOptions::default();
    if let Some(lang) = a.lang {
        options = options.with_lang(lang);
    }
    if let Some(title) = a.title {
        options = options.with_title(title);
    }
    let report = match session.fix_pdfua(part, &options) {
        Ok(r) => r,
        Err(err) => return report_edit_error(a.input, &err),
    };
    let outcome = match save_edited(
        &mut session,
        &source,
        a.output,
        a.mode,
        ProducerArg::Preserve,
        a.verify_undo,
    ) {
        Ok(outcome) => outcome,
        Err(code) => return code,
    };
    if a.json {
        print!("{}", report_json(a, &report));
    } else {
        print_text(a, &outcome, &report);
    }
    let code = finish_edit(a.input, &outcome);
    if code == exit::SUCCESS && report.validation.fails() {
        exit::PREFLIGHT_FAILED
    } else {
        code
    }
}

fn verdict(report: &FixPdfuaReport) -> String {
    let v = &report.validation;
    let failing = v
        .findings
        .iter()
        .filter(|f| f.kind != FindingKind::NeedsPerson)
        .count();
    let person = v.findings.len() - failing;
    if v.fails() {
        format!(
            "verdict: fails PDF/UA-{} — {failing} violation(s) or incomplete(s) remain; no \
             conformance claim written",
            v.part.number()
        )
    } else {
        format!(
            "verdict: no machine-checkable failure for PDF/UA-{}; claim {}; {person} check(s) \
             need a person",
            v.part.number(),
            if report.identified() {
                "written"
            } else {
                "not written"
            }
        )
    }
}

fn print_text(a: &FixPdfuaArgs<'_>, outcome: &EditOutcome, report: &FixPdfuaReport) {
    println!(
        "fix-pdfua {} part={} mode={} -> {}; changed={} out_bytes={} undo_verified={} \
         undo_identical={}",
        a.input.display(),
        a.part,
        a.mode.name(),
        a.output.display(),
        outcome.changed,
        outcome.report.bytes_written,
        u32::from(outcome.undo_verified),
        u32::from(outcome.undo_identical),
    );
    for r in &report.results {
        println!(
            "fixup {}: {} — {}",
            r.fixup.as_str(),
            r.outcome.as_str(),
            r.detail
        );
    }
    for d in &report.disclosures {
        println!("disclosure: {d}");
    }
    println!("{}", verdict(report));
    for f in &report.validation.findings {
        println!("{}", finding_line(f));
    }
}

fn report_json(a: &FixPdfuaArgs<'_>, report: &FixPdfuaReport) -> String {
    let fixups: Vec<String> = report
        .results
        .iter()
        .map(|r| {
            format!(
                "{{\"fixup\": \"{}\", \"outcome\": \"{}\", \"detail\": \"{}\"}}",
                r.fixup.as_str(),
                r.outcome.as_str(),
                json_escape(&r.detail)
            )
        })
        .collect();
    let disclosures: Vec<String> = report
        .disclosures
        .iter()
        .map(|d| format!("\"{}\"", json_escape(d)))
        .collect();
    let findings: Vec<String> = report
        .validation
        .findings
        .iter()
        .map(finding_json)
        .collect();
    format!(
        "{{\n  \"input\": \"{}\",\n  \"output\": \"{}\",\n  \"part\": {},\n  \"fixups\": [\n    \
         {}\n  ],\n  \"disclosures\": [{}],\n  \"identified\": {},\n  \"passes\": {},\n  \
         \"findings\": [\n    {}\n  ]\n}}\n",
        json_escape(&a.input.display().to_string()),
        json_escape(&a.output.display().to_string()),
        a.part,
        fixups.join(",\n    "),
        disclosures.join(", "),
        report.identified(),
        !report.validation.fails(),
        findings.join(",\n    ")
    )
}
