//! `preflight`: run a profile over a document and print every finding.

use super::*;

use pdfcer_core::preflight::{self, FindingKind, FindingTarget, Profile, RunOptions};

/// Profile files above this size are refused unread.
const MAX_PROFILE_BYTES: u64 = 1 << 20;

/// `preflight` — read-only.
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
/// - Exit `PREFLIGHT_FAILED` (18) when an error-severity rule found anything
///   or findings were dropped; `EDIT_REFUSED` for an unknown or invalid
///   profile; the usual read codes for an unreadable input.
pub(crate) fn cmd_preflight(
    input: Option<&Path>,
    profile: &str,
    max_findings: usize,
    dump: bool,
) -> u8 {
    let profile = match load_profile(profile) {
        Ok(p) => p,
        Err(why) => {
            eprintln!("pdfcer: --profile {profile}: {why}");
            return exit::EDIT_REFUSED;
        }
    };
    if dump {
        print!("{}", profile.to_json());
        return exit::SUCCESS;
    }
    let Some(input) = input else {
        eprintln!("pdfcer: preflight needs an input file");
        return exit::RUNTIME_ERROR;
    };
    let doc = match open_for_read(input) {
        Ok(doc) => doc,
        Err(code) => return code,
    };
    let options = RunOptions {
        max_findings,
        ..RunOptions::default()
    };
    let report = preflight::run_with(&doc.view(), &profile, &options);
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
    if report.fails() {
        exit::PREFLIGHT_FAILED
    } else {
        exit::SUCCESS
    }
}

fn load_profile(spec: &str) -> Result<Profile, String> {
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

fn finding_line(f: &preflight::Finding) -> String {
    let kind = match f.kind {
        FindingKind::Violation => "violation",
        FindingKind::NeedsPerson => "needs-person",
        FindingKind::Incomplete => "incomplete",
    };
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

fn target(t: FindingTarget) -> String {
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
        _ => "other".into(),
    }
}
