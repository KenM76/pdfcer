//! `preflight`: run a profile over a document and print every finding.

use super::*;

use pdfcer_core::preflight::{
    self, FindingKind, FindingTarget, PreflightReport, Profile, RunOptions,
};

#[path = "preflight_fix.rs"]
mod fix;

/// Profile files above this size are refused unread.
const MAX_PROFILE_BYTES: u64 = 1 << 20;

/// The `preflight` arguments after parsing.
pub(crate) struct PreflightArgs<'a> {
    /// The PDF to check; `None` only with `dump`.
    pub(crate) input: Option<&'a Path>,
    /// `--profile`: a built-in name or a JSON file.
    pub(crate) profile: &'a str,
    /// `--max-findings`.
    pub(crate) max_findings: usize,
    /// `--dump-profile`.
    pub(crate) dump: bool,
    /// `--report FILE`.
    pub(crate) report: Option<&'a Path>,
    /// `--report-format`.
    pub(crate) report_format: ReportFormatArg,
    /// `--fix OUTPUT`.
    pub(crate) fix: Option<&'a Path>,
    /// `--fix-suggested`.
    pub(crate) suggested: bool,
    /// `--icc FILE`.
    pub(crate) icc: Option<&'a Path>,
    /// `--mode`.
    pub(crate) mode: SaveMode,
    /// `--verify-undo`.
    pub(crate) verify_undo: bool,
}

/// `preflight` — read-only unless `--fix`.
///
/// ## Contract
///
/// - With `--dump-profile`: the profile's JSON on stdout, exit 0.
/// - Otherwise one header line,
///   `preflight <input> profile="<name>" pages=<checked>/<count> findings=<n> dropped=<d>`,
///   then one line per finding:
///   `<severity> <violation|needs-person|incomplete> rule=<id> page=<N|-> target=<t>`
///   with ` rect=<llx>,<lly>,<urx>,<ury>` and
///   ` measured=<v> limit=<l> unit=<u>` when known, then ` "<sentence>"`.
/// - `--report FILE` also writes `preflight::report_text` of the checked
///   document to FILE (`preflight::report_json` with `--report-format
///   json`); with `--fix`, of the fixed output.
/// - `--fix OUTPUT`: see [`fix::cmd_preflight_fix`].
/// - Exit `PREFLIGHT_FAILED` (18) when an error-severity rule found anything
///   or findings were dropped; `EDIT_REFUSED` for an unknown or invalid
///   profile; the usual read codes for an unreadable input.
pub(crate) fn cmd_preflight(a: &PreflightArgs<'_>) -> u8 {
    let profile = match load_profile(a.profile) {
        Ok(p) => p,
        Err(why) => {
            eprintln!("pdfcer: --profile {}: {why}", a.profile);
            return exit::EDIT_REFUSED;
        }
    };
    if a.dump {
        print!("{}", profile.to_json());
        return exit::SUCCESS;
    }
    let Some(input) = a.input else {
        eprintln!("pdfcer: preflight needs an input file");
        return exit::RUNTIME_ERROR;
    };
    if let Some(output) = a.fix {
        return fix::cmd_preflight_fix(a, input, output, &profile);
    }
    let doc = match open_for_read(input) {
        Ok(doc) => doc,
        Err(code) => return code,
    };
    let report = check(&doc.view(), &profile, a);
    print_report(input, &profile, &report);
    if let Err(code) = write_report(a, &report, &profile) {
        return code;
    }
    if report.fails() {
        exit::PREFLIGHT_FAILED
    } else {
        exit::SUCCESS
    }
}

/// Run `profile` over `view`, measuring ink coverage with the renderer.
pub(crate) fn check(
    view: &pdfcer_core::view::DocumentView<'_>,
    profile: &Profile,
    a: &PreflightArgs<'_>,
) -> PreflightReport {
    let meter = pdfcer_render::TacMeter::new();
    preflight::run_with_meter(view, profile, &run_options(a), Some(&meter))
}

/// The run options `a` asks for.
pub(crate) fn run_options(a: &PreflightArgs<'_>) -> RunOptions<'static> {
    RunOptions {
        max_findings: a.max_findings,
        ..RunOptions::default()
    }
}

/// The header line and one line per finding.
pub(crate) fn print_report(input: &Path, profile: &Profile, report: &PreflightReport) {
    println!(
        "preflight {} profile=\"{}\" pages={}/{} findings={} dropped={}",
        input.display(),
        profile.name,
        report.pages_checked,
        report.page_count,
        report.findings.len(),
        report.dropped
    );
    for f in &report.findings {
        println!("{}", finding_line(f));
    }
}

/// Write the report to `--report`, in `--report-format`, when one was
/// asked for.
pub(crate) fn write_report(
    a: &PreflightArgs<'_>,
    report: &PreflightReport,
    profile: &Profile,
) -> Result<(), u8> {
    let Some(path) = a.report else {
        return Ok(());
    };
    let body = match a.report_format {
        ReportFormatArg::Text => preflight::report_text(report, profile),
        ReportFormatArg::Json => preflight::report_json(report, profile),
    };
    std::fs::write(path, body).map_err(|err| {
        eprintln!("pdfcer: --report {}: {err}", path.display());
        exit::IO_ERROR
    })
}

/// A built-in profile by name, or a profile JSON file.
pub(crate) fn load_profile(spec: &str) -> Result<Profile, String> {
    if let Some(p) = Profile::builtin(spec) {
        return Ok(p);
    }
    let path = Path::new(spec);
    let size = std::fs::metadata(path)
        .map_err(|e| {
            format!(
                "not a built-in profile ({}) and not a readable file: {e}",
                Profile::BUILTIN_NAMES.join(", ")
            )
        })?
        .len();
    if size > MAX_PROFILE_BYTES {
        return Err(format!(
            "the profile file is {size} bytes; the limit is {MAX_PROFILE_BYTES}"
        ));
    }
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    Profile::from_json(&bytes).map_err(|e| e.to_string())
}

/// `violation`, `needs-person` or `incomplete`.
pub(crate) fn kind_name(kind: FindingKind) -> &'static str {
    match kind {
        FindingKind::Violation => "violation",
        FindingKind::NeedsPerson => "needs-person",
        _ => "incomplete",
    }
}

/// One finding as a report line, its quoted sentence last.
pub(crate) fn finding_line(f: &preflight::Finding) -> String {
    let kind = kind_name(f.kind);
    let page = f.page.map_or_else(|| "-".to_owned(), |p| p.to_string());
    let mut line = format!(
        "{} {kind} rule={} page={page} target={}",
        f.severity.as_str(),
        f.rule_id,
        target(f.target)
    );
    if let Some(r) = f.rect {
        line.push_str(&format!(
            " rect={:.2},{:.2},{:.2},{:.2}",
            r.llx, r.lly, r.urx, r.ury
        ));
    }
    if let Some(m) = f.measured {
        line.push_str(&format!(
            " measured={:.2} limit={:.2} unit={}",
            m.value,
            m.limit,
            m.unit.as_str()
        ));
    }
    line.push_str(&format!(" \"{}\"", f.message));
    line
}

/// A finding's target as `kind:num gen`, `kind:index`, `page` or `document`.
pub(crate) fn target(t: FindingTarget) -> String {
    match t {
        FindingTarget::Document => "document".into(),
        FindingTarget::Page => "page".into(),
        FindingTarget::PageObject(i) => format!("object:{i}"),
        FindingTarget::FormLeaf(i) => format!("leaf:{i}"),
        FindingTarget::Annotation(id) => format!("annotation:{} {}", id.num, id.generation),
        FindingTarget::Font(id) => format!("font:{} {}", id.num, id.generation),
        FindingTarget::ColourSpace(id) => format!("colour-space:{} {}", id.num, id.generation),
        FindingTarget::Image(id) => format!("image:{} {}", id.num, id.generation),
        FindingTarget::GraphicsState(id) => format!("graphics-state:{} {}", id.num, id.generation),
        FindingTarget::StructElem(id) => format!("struct-elem:{} {}", id.num, id.generation),
        _ => "other".into(),
    }
}
