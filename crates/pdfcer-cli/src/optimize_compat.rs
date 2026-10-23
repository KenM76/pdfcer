//! `optimize --compat`: hold the result to a PDF version and report which
//! version-dependent features it still uses.

use std::fmt::Write as _;

use pdfcer_core::PdfVersion;
use pdfcer_core::document::Document;
use pdfcer_core::optimize::{
    FeatureUse, FloorOptions, IccVersionGate, LowerOptions, LoweredVersion, VersionFloor,
    features_used, lower_version, version_floor_with,
};
use pdfcer_core::writer::SaveOptions;

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
    /// With --compat VERSION: also lower the declared version to VERSION,
    /// when the content provably needs nothing newer. The header digits
    /// are rewritten in place and a catalog /Version is removed; below 1.5
    /// a file with object or cross-reference streams is rebuilt with a
    /// classic cross-reference table, renumbering its objects. Refused
    /// (exit 9, nothing written) for an encrypted file, a signed one
    /// without --invalidate-signatures, content above VERSION, or content
    /// that could not all be read. The file is written even when it is not
    /// smaller.
    #[arg(long, requires = "compat")]
    pub(crate) lower_version: bool,
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
    /// What --lower-version did, when it ran.
    lowered: Option<Lowered>,
}

/// The disclosure of a lowering, without the bytes.
pub(crate) struct Lowered {
    from: PdfVersion,
    to: PdfVersion,
    catalog_version_removed: bool,
    route: String,
}

impl Lowered {
    /// Keep what the report states; hand back the bytes.
    pub(crate) fn split(l: LoweredVersion) -> (Self, Vec<u8>) {
        let note = Self {
            from: l.from,
            to: l.to,
            catalog_version_removed: l.catalog_version_removed,
            route: l.route.to_string(),
        };
        (note, l.bytes)
    }
}

/// --lower-version on `bytes`, or the exit code after printing why not.
pub(crate) fn lower(
    args: &CompatArgs,
    target: PdfVersion,
    bytes: Vec<u8>,
    allow_signed: bool,
) -> Result<LoweredVersion, u8> {
    let options = LowerOptions::default()
        .with_floor(args.floor_options())
        .with_allow_signed(allow_signed);
    lower_version(bytes, target, &options, &SaveOptions::default()).map_err(|e| {
        eprintln!("pdfcer: optimize: --lower-version: {e}");
        crate::exit::EDIT_REFUSED
    })
}

impl CompatSummary {
    fn above(&self) -> impl Iterator<Item = &FeatureUse> {
        self.used.iter().filter(|u| u.above(self.target))
    }
}

/// Scan `result` against `target`.
pub(crate) fn summarize(
    target: PdfVersion,
    result: &Document,
    args: &CompatArgs,
    lowered: Option<Lowered>,
) -> CompatSummary {
    CompatSummary {
        target,
        declared: result.version(),
        used: features_used(result),
        floor: version_floor_with(result, args.floor_options()),
        lowered,
    }
}

impl CompatSummary {
    /// Whether --lower-version changed the declared version.
    pub(crate) fn lowered(&self) -> bool {
        self.lowered.is_some()
    }
}

/// The clause after the compatibility verdict: lowered or not.
fn declared_clause(c: &CompatSummary) -> String {
    match &c.lowered {
        Some(l) => format!(
            "the declared version was lowered from {} to {}",
            l.from, l.to
        ),
        None => format!("the declared version {} is not lowered", c.declared),
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
            "compatibility: nothing above PDF {}; {}",
            c.target,
            declared_clause(c)
        );
    } else {
        let _ = writeln!(
            out,
            "compatibility: {above} feature(s) above PDF {} kept, not removed; {}",
            c.target,
            declared_clause(c)
        );
    }
    if let Some(l) = &c.lowered {
        let catalog = if l.catalog_version_removed {
            ", catalog /Version removed"
        } else {
            ""
        };
        let _ = writeln!(
            out,
            "version: lowered to PDF {}: {}{catalog}",
            l.to, l.route
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
    let lowered = c.lowered.as_ref().map_or_else(
        || "null".to_owned(),
        |l| {
            format!(
                "{{\"from\":\"{}\",\"to\":\"{}\",\"catalog_version_removed\":{},\"route\":\"{}\"}}",
                l.from,
                l.to,
                l.catalog_version_removed,
                json_escape(&l.route)
            )
        },
    );
    format!(
        "{{\"target\":\"{}\",\"declared\":\"{}\",\"features\":[{}],\"above\":{},\"floor\":{},\"lowered\":{lowered}}}",
        c.target,
        c.declared,
        features.join(","),
        c.above().count(),
        floor_json(&c.floor)
    )
}
