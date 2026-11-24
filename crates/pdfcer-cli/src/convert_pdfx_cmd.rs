//! `convert-pdfx`: convert a document to one PDF/X level and claim it only
//! when the converted file validates (`EditSession::convert_to_pdfx`).

use super::*;

use pdfcer_core::preflight::{PdfxLevel, Severity};
use pdfcer_core::prepress::{PdfxConversion, PdfxConversionReport, PdfxOutputIntent};

use crate::extract::json_escape;
use crate::preflight::finding_line;

/// What `convert-pdfx` converts to and how.
pub(crate) struct PdfxArgs<'a> {
    /// The level.
    pub level: PdfxLevel,
    /// The destination ICC profile, embedded as the output intent.
    pub icc: &'a Path,
    /// `/OutputConditionIdentifier`.
    pub condition: Option<&'a str>,
    /// `/Title` when the document has none; `None` uses the file name.
    pub title: Option<&'a str>,
    /// Print one JSON object.
    pub json: bool,
    /// Save the conversion without the claim when the claim is refused.
    pub write_partial: bool,
    /// Claim only on a `conforms` verdict.
    pub require_conforms: bool,
}

fn conversion(input: &Path, args: &PdfxArgs<'_>) -> Result<PdfxConversion, u8> {
    let profile = std::fs::read(args.icc).map_err(|err| {
        eprintln!("pdfcer: {}: --icc: {err}", args.icc.display());
        exit::EDIT_REFUSED
    })?;
    let mut intent = PdfxOutputIntent::new(profile);
    if let Some(c) = args.condition {
        intent = intent.with_condition(c);
    }
    let title = args.title.map(str::to_owned).unwrap_or_else(|| {
        input.file_stem().map_or_else(
            || "Untitled".to_owned(),
            |s| s.to_string_lossy().into_owned(),
        )
    });
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX));
    Ok(PdfxConversion::new(intent)
        .with_title(title)
        .with_time(now)
        .with_keep_partial(args.write_partial)
        .with_require_conforms(args.require_conforms))
}

/// `convert-pdfx` — one undo step.
///
/// ## Contract
///
/// - Text: one `step: …` line per step, one `note: …` line per disclosure,
///   one `preflight`-style line per error finding still standing, then
///   `convert-pdfx <input> level=<level> before=<verdict> after=<verdict>
///   claimed=<0|1>`, then the save line when a file was written.
/// - `--json`: one object `{input, level, before, after, claimed,
///   kept_partial, written, steps, notes, findings: [{rule, severity,
///   message}]}` instead.
/// - The claim is never written for a file that does not conform. When it
///   is refused nothing is written unless `--write-partial`, and the exit
///   is `PREFLIGHT_FAILED` (18) either way.
/// - An unreadable `--icc` is refused before anything is written
///   (`EDIT_REFUSED`).
pub(crate) fn cmd_convert_pdfx(
    input: &Path,
    args: &PdfxArgs<'_>,
    output: &Path,
    mode: SaveMode,
    verify_undo: bool,
) -> u8 {
    let (source, mut session) = match open_for_edit(input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let conversion = match conversion(input, args) {
        Ok(c) => c,
        Err(code) => return code,
    };
    let flattener = pdfcer_emf::RecordingFlattener::new();
    let converter = pdfcer_render::convert_colours::IcccerConverter::new();
    let report = match session.convert_to_pdfx(args.level, &conversion, &flattener, &converter) {
        Ok(r) => r,
        Err(err) => return report_edit_error(input, &err),
    };
    let write = report.claimed || report.kept_partial;
    if args.json {
        println!("{}", report_json(input, &report, write));
    } else {
        print_text(input, &report);
    }
    if !write {
        return exit::PREFLIGHT_FAILED;
    }
    let outcome = match save_edited(
        &mut session,
        &source,
        output,
        mode,
        ProducerArg::Preserve,
        verify_undo,
    ) {
        Ok(outcome) => outcome,
        Err(code) => return code,
    };
    if !args.json {
        let r = &outcome.report;
        println!(
            "saved mode={} -> {}; changed={} objects={} appended={} out_bytes={} \
undo_verified={} undo_identical={}",
            mode.name(),
            output.display(),
            outcome.changed,
            r.objects_written,
            r.bytes_appended,
            r.bytes_written,
            u32::from(outcome.undo_verified),
            u32::from(outcome.undo_identical),
        );
    }
    let code = finish_edit(input, &outcome);
    if report.claimed {
        code
    } else {
        exit::PREFLIGHT_FAILED
    }
}

fn errors(report: &PdfxConversionReport) -> impl Iterator<Item = &pdfcer_core::preflight::Finding> {
    report
        .validation
        .preflight
        .findings
        .iter()
        .filter(|f| f.severity == Severity::Error)
}

fn print_text(input: &Path, report: &PdfxConversionReport) {
    for s in &report.steps {
        println!("step: {s}");
    }
    for d in &report.disclosures {
        println!("note: {d}");
    }
    for f in errors(report) {
        println!("{}", finding_line(f));
    }
    println!(
        "convert-pdfx {} level={} before={} after={} claimed={}",
        input.display(),
        report.level.as_str(),
        report.before.as_str(),
        report.after.as_str(),
        u32::from(report.claimed)
    );
}

fn report_json(input: &Path, report: &PdfxConversionReport, written: bool) -> String {
    let quote = |s: &str| format!("\"{}\"", json_escape(s));
    let list = |v: &[String]| v.iter().map(|s| quote(s)).collect::<Vec<_>>().join(", ");
    let findings: Vec<String> = errors(report)
        .map(|f| {
            format!(
                "{{\"rule\": {}, \"severity\": \"{}\", \"message\": {}}}",
                quote(&f.rule_id),
                f.severity.as_str(),
                quote(&f.message)
            )
        })
        .collect();
    format!(
        "{{\"input\": {}, \"level\": \"{}\", \"before\": \"{}\", \"after\": \"{}\", \
         \"claimed\": {}, \"kept_partial\": {}, \"written\": {written}, \"steps\": [{}], \
         \"notes\": [{}], \"findings\": [{}]}}",
        quote(&input.display().to_string()),
        report.level.as_str(),
        report.before.as_str(),
        report.after.as_str(),
        report.claimed,
        report.kept_partial,
        list(&report.steps),
        list(&report.disclosures),
        findings.join(", ")
    )
}
