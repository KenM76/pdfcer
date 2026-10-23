//! `optimize --compat`: hold the result to a PDF version and report which
//! version-dependent features it still uses.

use std::fmt::Write as _;

use pdfcer_core::PdfVersion;
use pdfcer_core::document::Document;
use pdfcer_core::optimize::{
    FeatureUse, FloorOptions, IccVersionGate, VersionFloor, features_used, version_floor_with,
};

use crate::json_escape;

/// The compatibility flag.
#[derive(clap::Args, Debug, Clone, Default)]
pub(crate) struct CompatArgs {
    /// The highest PDF version the result may need: 1.3, 1.4, 1.5, 1.6,
    /// 1.7, 2.0, or retain for the input's own version. A change needing
    /// more is skipped with a reason instead of raising the version (JBIG2
    /// needs 1.4, Brotli 2.0), and object streams stay unpacked under 1.5.
    /// The report lists every version-dependent feature the result uses
    /// and which are above the target; those are kept, not removed, and
    /// the declared version is never lowered. It also states the lowest
    /// version the result's content needs, and what sets it. Without it
    /// the version is raised as the changes need.
    #[arg(long, value_name = "VERSION", value_parser = parse_target)]
    pub(crate) compat: Option<Target>,
    /// With --compat: let an embedded ICC profile's version raise the
    /// stated lowest version (profile 2.2 needs PDF 1.4, 4.1 needs 1.6,
    /// 4.3 needs 2.0). By default the profile versions are listed but
    /// raise nothing: PDF 2.0 no longer ties them to a PDF version, and a
    /// reader that cannot use a profile falls back to its alternate.
    #[arg(long, requires = "compat")]
    pub(crate) strict_icc: bool,
}

/// A parsed `--compat` value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Target {
    /// The input's effective version.
    Retain,
    /// A named version.
    Version(PdfVersion),
}

fn parse_target(s: &str) -> Result<Target, String> {
    let v = |major, minor| Ok(Target::Version(PdfVersion { major, minor }));
    match s {
        "retain" => Ok(Target::Retain),
        "1.3" => v(1, 3),
        "1.4" => v(1, 4),
        "1.5" => v(1, 5),
        "1.6" => v(1, 6),
        "1.7" => v(1, 7),
        "2.0" => v(2, 0),
        _ => Err("one of 1.3, 1.4, 1.5, 1.6, 1.7, 2.0 or retain".to_owned()),
    }
}

impl CompatArgs {
    fn floor_options(&self) -> FloorOptions {
        let gate = if self.strict_icc {
            IccVersionGate::Strict
        } else {
            IccVersionGate::Permissive
        };
        FloorOptions::default().with_icc(gate)
    }

    /// The target for a document at `version`, if one was asked for.
    pub(crate) fn resolve(&self, version: PdfVersion) -> Option<PdfVersion> {
        self.compat.map(|t| match t {
            Target::Retain => version,
            Target::Version(v) => v,
        })
    }
}

/// What the result uses against the target.
pub(crate) struct CompatSummary {
    target: PdfVersion,
    /// The result's declared (effective) version.
    declared: PdfVersion,
    used: Vec<FeatureUse>,
    floor: VersionFloor,
}

impl CompatSummary {
    fn above(&self) -> impl Iterator<Item = &FeatureUse> {
        self.used.iter().filter(|u| u.above(self.target))
    }
}

/// Scan `result` against `target`.
pub(crate) fn summarize(target: PdfVersion, result: &Document, args: &CompatArgs) -> CompatSummary {
    CompatSummary {
        target,
        declared: result.version(),
        used: features_used(result),
        floor: version_floor_with(result, args.floor_options()),
    }
}

fn example(u: &FeatureUse) -> String {
    u.example
        .map_or_else(String::new, |id| format!(", e.g. object {id}"))
}

/// The text report's compatibility lines.
pub(crate) fn compat_text(c: &CompatSummary) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "compatibility: target PDF {}", c.target);
    for u in &c.used {
        let mark = if u.above(c.target) {
            ", above the target"
        } else {
            ""
        };
        let _ = writeln!(
            out,
            "  uses {}: {} object(s){}{mark}",
            u.feature,
            u.objects,
            example(u)
        );
    }
    let above = c.above().count();
    if above == 0 {
        let _ = writeln!(
            out,
            "compatibility: nothing above PDF {}; the declared version {} is not lowered",
            c.target, c.declared
        );
    } else {
        let _ = writeln!(
            out,
            "compatibility: {above} feature(s) above PDF {} kept, not removed; the declared version {} is not lowered",
            c.target, c.declared
        );
    }
    floor_text(&c.floor, &mut out);
    out
}

/// How many constructs at the floor's version the text report names.
const FLOOR_LINES: usize = 8;

fn floor_text(f: &VersionFloor, out: &mut String) {
    let bound = if f.proven() {
        String::new()
    } else {
        format!(
            " at least ({} stream(s) could not be read)",
            f.unprovable.len()
        )
    };
    let _ = writeln!(
        out,
        "compatibility: the content needs PDF {}{bound}",
        f.floor
    );
    let top: Vec<_> = f
        .requirements
        .iter()
        .filter(|r| r.version == f.floor)
        .collect();
    for r in top.iter().take(FLOOR_LINES) {
        let _ = writeln!(out, "  needs {r}");
    }
    if top.len() > FLOOR_LINES {
        let _ = writeln!(
            out,
            "  needs {} more at PDF {}",
            top.len() - FLOOR_LINES,
            f.floor
        );
    }
    icc_text(f, out);
}

/// One line per ICC profile header version, with its count.
fn icc_text(f: &VersionFloor, out: &mut String) {
    let mut seen: std::collections::BTreeMap<(u8, u8), (usize, Option<PdfVersion>)> =
        std::collections::BTreeMap::new();
    for p in &f.icc_profiles {
        seen.entry((p.major, p.minor)).or_insert((0, p.strict)).0 += 1;
    }
    for ((major, minor), (n, strict)) in seen {
        let maps = strict.map_or_else(
            || "named by no PDF version".to_owned(),
            |v| format!("PDF {v} under --strict-icc"),
        );
        let _ = writeln!(
            out,
            "  ICC profile version {major}.{minor}: {n} profile(s), {maps}"
        );
    }
}

fn floor_json(f: &VersionFloor) -> String {
    let needs: Vec<String> = f
        .requirements
        .iter()
        .map(|r| {
            format!(
                "{{\"construct\":\"{}\",\"version\":\"{}\",\"objects\":{},\"example\":{}}}",
                json_escape(&r.construct),
                r.version,
                r.objects,
                r.example
                    .map_or_else(|| "null".to_owned(), |id| format!("\"{id}\""))
            )
        })
        .collect();
    let icc: Vec<String> = f
        .icc_profiles
        .iter()
        .map(|p| {
            format!(
                "{{\"object\":\"{}\",\"version\":\"{}.{}\",\"strict\":{}}}",
                p.object,
                p.major,
                p.minor,
                p.strict
                    .map_or_else(|| "null".to_owned(), |v| format!("\"{v}\""))
            )
        })
        .collect();
    format!(
        "{{\"version\":\"{}\",\"proven\":{},\"unreadable\":{},\"needs\":[{}],\"icc_profiles\":[{}]}}",
        f.floor,
        f.proven(),
        f.unprovable.len(),
        needs.join(","),
        icc.join(",")
    )
}

/// The JSON report's `compatibility` value.
pub(crate) fn compat_json(c: &CompatSummary) -> String {
    let features: Vec<String> = c
        .used
        .iter()
        .map(|u| {
            format!(
                "{{\"feature\":\"{}\",\"introduced\":\"{}\",\"objects\":{},\"example\":{},\"above\":{}}}",
                json_escape(u.feature.label()),
                u.feature.introduced(),
                u.objects,
                u.example
                    .map_or_else(|| "null".to_owned(), |id| format!("\"{id}\"")),
                u.above(c.target)
            )
        })
        .collect();
    format!(
        "{{\"target\":\"{}\",\"declared\":\"{}\",\"features\":[{}],\"above\":{},\"floor\":{}}}",
        c.target,
        c.declared,
        features.join(","),
        c.above().count(),
        floor_json(&c.floor)
    )
}
