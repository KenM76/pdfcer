//! `edit-text --fallback-font` / `--fallback-font-file`: the face characters
//! the run's font cannot encode are set in (`EditOptions::fallback`).

use std::path::Path;

use pdfcer_core::text_edit::{EditReport, FallbackFace, FallbackSource, RunRepertoire};
use pdfcer_render::font::subset::{SubsetError, plan_subset, subset_tag_for};

use crate::exit;

/// The fallback face the flags name, or `None` without either flag.
///
/// A font file is subset for the characters of `replace` it covers; the
/// characters the run's font keeps are decided later, in core, so a few
/// extra glyphs may be embedded. The face is leaked: `EditOptions` is `Copy`
/// and the process makes one edit.
///
/// # Errors
///
/// The exit code, after printing why the file cannot serve as a fallback.
pub(crate) fn fallback_face(
    name: Option<&str>,
    file: Option<&Path>,
    replace: &str,
) -> Result<Option<&'static FallbackFace>, u8> {
    let face = match (name, file) {
        (Some(name), _) => FallbackFace::Named(name.to_owned()),
        (None, Some(path)) => FallbackFace::Embedded(Box::new(file_plan(path, replace)?)),
        (None, None) => return Ok(None),
    };
    Ok(Some(Box::leak(Box::new(face))))
}

fn file_plan(path: &Path, replace: &str) -> Result<pdfcer_core::font_embed::FontEmbedPlan, u8> {
    let donor = std::fs::read(path).map_err(|e| {
        eprintln!("pdfcer: cannot read the font file {}: {e}", path.display());
        exit::IO_ERROR
    })?;
    let mut wanted: Vec<char> = replace.chars().collect();
    wanted.sort_unstable();
    wanted.dedup();
    let stem = path.file_stem().map_or_else(
        || "FallbackFont".to_owned(),
        |s| s.to_string_lossy().into_owned(),
    );
    let tag = subset_tag_for(&stem);
    let plan = match plan_subset(&donor, 0, &wanted, &stem, &tag) {
        Err(SubsetError::IncompleteCoverage { missing }) => {
            wanted.retain(|c| !missing.contains(c));
            plan_subset(&donor, 0, &wanted, &stem, &tag)
        }
        other => other,
    };
    plan.map_err(|e| {
        eprintln!(
            "pdfcer: edit-text refused: --fallback-font-file {}: {e}",
            path.display()
        );
        exit::EDIT_REFUSED
    })
}

/// The `fallback=` line: which characters went to which face, and from where.
pub(crate) fn print_fallback(report: &EditReport) {
    let Some(used) = &report.fallback else {
        return;
    };
    let chars: Vec<String> = used
        .characters
        .iter()
        .map(|c| format!("U+{:04X}", u32::from(*c)))
        .collect();
    let source = match used.source {
        FallbackSource::PageResource => "page-resource",
        FallbackSource::AddedStandard14 => "added-standard-14",
        FallbackSource::EmbeddedSubset => "embedded-subset",
        _ => "other",
    };
    println!(
        "  fallback={} face={} resource=/{} source={source}",
        chars.join(","),
        used.base_font,
        String::from_utf8_lossy(&used.font_resource)
    );
}

/// The `run-repertoire` suffix for `--fallback-font`: ` fallback=N`, plus
/// ` fallback_chars=U+...` with `--list`; empty without the flag.
pub(crate) fn repertoire_fallback(rep: &RunRepertoire, asked: bool, list: bool) -> String {
    if !asked {
        return String::new();
    }
    let mut out = format!(" fallback={}", rep.via_fallback.len());
    if list {
        let chars: Vec<String> = rep
            .via_fallback
            .iter()
            .map(|c| format!("U+{:04X}", u32::from(*c)))
            .collect();
        out.push_str(" fallback_chars=");
        out.push_str(&chars.join(","));
    }
    out
}
