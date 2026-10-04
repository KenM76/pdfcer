//! `edit-text --fallback-font` / `--fallback-font-file`: the face characters
//! the run's font cannot encode are set in (`EditOptions::fallback`), or
//! `--fallback-font auto`: the installed faces the ladder picks from
//! (`EditOptions::replacement_faces`, decision 178).

use std::path::{Path, PathBuf};

use pdfcer_core::text_edit::{
    CidProgramUse, EditReport, FallbackFace, FallbackSource, RunRepertoire,
};
use pdfcer_render::font::FaceCatalog;
use pdfcer_render::font::subset::{SubsetError, plan_subset, subset_tag_for};

use crate::exit;

/// The `--fallback-font` value that asks for the replacement-face ladder
/// (decision 178) instead of naming a face.
pub(crate) const AUTO: &str = "auto";

/// Every face in the settings file's font folders and each `--font-dir`,
/// labelled with its file path, for `--fallback-font auto`. A file the font
/// environment already skipped is skipped here too, silently: its note was
/// printed then. Leaked, as [`fallback_face`] is.
pub(crate) fn installed_faces(font_dirs: &[PathBuf]) -> &'static FaceCatalog {
    let mut faces = FaceCatalog::new(|path: &str| std::fs::read(path).map_err(|e| e.to_string()));
    for dir in crate::settings::font_dirs().iter().chain(font_dirs) {
        for path in crate::inspect::font_files_in(dir).unwrap_or_default() {
            if let Ok((_, data)) = crate::inspect::font_file_names(&path) {
                faces.add(&path.display().to_string(), data.bytes());
            }
        }
    }
    Box::leak(Box::new(faces))
}

/// `--fallback-font`, else the settings file's `fallback_font` when neither
/// flag is given.
pub(crate) fn effective_name<'a>(flag: Option<&'a str>, file: Option<&Path>) -> Option<&'a str> {
    match (flag, file) {
        (None, None) => crate::settings::active().fallback_font.as_deref(),
        _ => flag,
    }
}

/// The fallback face the flags name, or `None` without either flag or with
/// `--fallback-font auto` (see [`installed_faces`]).
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
        (Some(AUTO), _) => return Ok(None),
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
        FallbackSource::SameProgram => "same-program",
        _ => "other",
    };
    println!(
        "  fallback={} face={} resource=/{} source={source}",
        chars.join(","),
        used.base_font,
        String::from_utf8_lossy(&used.font_resource)
    );
    if let Some(program) = used.cid_program {
        let program = match program {
            CidProgramUse::Shared => "shared",
            CidProgramUse::SharedWithCmap => "shared-with-cmap",
            CidProgramUse::StrippedCopy => "stripped-copy",
            _ => "other",
        };
        println!("  cid_font_program={program}");
    }
    if let Some(m) = &used.chosen_by {
        println!(
            // `source` last: a path may hold spaces.
            "  face_match={} rung={} skipped={} failed={} source={}",
            m.face,
            m.rung.label(),
            m.skipped.len(),
            m.failed.len(),
            m.source.as_deref().unwrap_or("standard-14"),
        );
        if m.name_derived {
            println!("  face_name=derived");
        }
    }
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
