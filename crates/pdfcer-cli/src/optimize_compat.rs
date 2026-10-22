//! `optimize --compat`: hold the result to a PDF version and report which
//! version-dependent features it still uses.

use std::fmt::Write as _;

use pdfcer_core::PdfVersion;
use pdfcer_core::document::Document;
use pdfcer_core::optimize::{FeatureUse, features_used};

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
    /// the declared version is never lowered. Without it the version is
    /// raised as the changes need.
    #[arg(long, value_name = "VERSION", value_parser = parse_target)]
    pub(crate) compat: Option<Target>,
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
}

impl CompatSummary {
    fn above(&self) -> impl Iterator<Item = &FeatureUse> {
        self.used.iter().filter(|u| u.above(self.target))
    }
}

/// Scan `result` against `target`.
pub(crate) fn summarize(target: PdfVersion, result: &Document) -> CompatSummary {
    CompatSummary {
        target,
        declared: result.version(),
        used: features_used(result),
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
    out
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
        "{{\"target\":\"{}\",\"declared\":\"{}\",\"features\":[{}],\"above\":{}}}",
        c.target,
        c.declared,
        features.join(","),
        c.above().count()
    )
}
