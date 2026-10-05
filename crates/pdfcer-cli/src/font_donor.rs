//! Subsetting a donor font file for `add-text`, `format-text` and style edits.

use super::*;

/// Subset the donor at `path` for the characters of `find` (`format-text
/// --embed-font`), or the exit code after printing why not.
pub(crate) fn donor_plan(
    path: &std::path::Path,
    find: &str,
) -> Result<pdfcer_core::font_embed::FontEmbedPlan, u8> {
    if find.is_empty() {
        eprintln!(
            "pdfcer: format-text refused: --embed-font needs --find, because the subset is built for exactly the characters of the run"
        );
        return Err(exit::EDIT_REFUSED);
    }
    subset_donor(path, find, "format-text")
}

/// Subset the donor at `path` for the distinct characters of `text`, or the
/// exit code after printing why not (`verb` names the refusing command).
/// The tag derives from the file stem (§9.6.4), so repeated runs over one
/// font are byte-reproducible.
pub(crate) fn subset_donor(
    path: &Path,
    text: &str,
    verb: &str,
) -> Result<pdfcer_core::font_embed::FontEmbedPlan, u8> {
    let donor = std::fs::read(path).map_err(|e| {
        eprintln!("pdfcer: cannot read the font file {}: {e}", path.display());
        exit::IO_ERROR
    })?;
    let stem = path.file_stem().map_or_else(
        || "EmbeddedFont".to_owned(),
        |s| s.to_string_lossy().into_owned(),
    );
    let tag = pdfcer_render::font::subset::subset_tag_for(&stem);
    pdfcer_render::font::subset::plan_subset(&donor, 0, &distinct_chars(text), &stem, &tag).map_err(
        |e| {
            eprintln!("pdfcer: {verb} refused: {e}");
            exit::EDIT_REFUSED
        },
    )
}

/// Each character of `text` once, sorted: a subset plan reports coverage
/// gaps against exactly what it was asked for.
pub(crate) fn distinct_chars(text: &str) -> Vec<char> {
    let mut wanted: Vec<char> = text.chars().collect();
    wanted.sort_unstable();
    wanted.dedup();
    wanted
}

/// One `--X` / `--no-X` flag pair as a [`StyleTarget`] axis; clap's
/// `conflicts_with` keeps both from being set.
///
/// [`StyleTarget`]: pdfcer_core::text_edit::StyleTarget
pub(crate) const fn axis_target(on: bool, off: bool) -> Option<bool> {
    if on {
        Some(true)
    } else if off {
        Some(false)
    } else {
        None
    }
}

/// Rung-3 candidates for `format-text --embed-styled-face` (`Pass 142.3`):
/// every TrueType face in `font_dirs` whose PostScript name claims every
/// axis asked on and none asked off, subset for the characters of `find`.
///
/// Only the axis claim is checked here, to avoid subsetting a whole system
/// font folder; which candidate is of the run's family and carries exactly
/// the right style is decided by the ladder in core, the one selector. A face
/// that cannot be subset (CFF, licence, missing glyph) is skipped with a
/// `font-dir:` note.
pub(crate) fn style_donor_plans(
    font_dirs: &[PathBuf],
    find: &str,
    style: pdfcer_core::text_edit::StyleTarget,
) -> Result<Vec<pdfcer_core::font_embed::FontEmbedPlan>, u8> {
    use pdfcer_core::text_edit::synth::{name_claims_bold, name_claims_italic};
    use pdfcer_render::font::program::FontProgram;

    if style.is_keep() {
        eprintln!(
            "pdfcer: format-text refused: --embed-styled-face needs --bold, --italic, --no-bold \
             or --no-italic"
        );
        return Err(exit::EDIT_REFUSED);
    }
    if find.is_empty() {
        eprintln!(
            "pdfcer: format-text refused: --embed-styled-face needs --find, because the \
             subset is built for exactly the characters of the run"
        );
        return Err(exit::EDIT_REFUSED);
    }
    let mut wanted: Vec<char> = find.chars().collect();
    wanted.sort_unstable();
    wanted.dedup();

    let mut plans = Vec::new();
    for dir in font_dirs {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue; // `build_font_environment` already reported it.
        };
        let mut files: Vec<PathBuf> = entries
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.is_file() && has_font_extension(p))
            .collect();
        files.sort();
        for path in files {
            if std::fs::metadata(&path).is_ok_and(|m| m.len() > MAX_FONT_FILE_BYTES) {
                continue;
            }
            let Ok(bytes) = std::fs::read(&path) else {
                continue;
            };
            let Some(name) = FontProgram::parse(&bytes)
                .ok()
                .and_then(|p| p.face_names().into_iter().next())
            else {
                continue;
            };
            let fits = |want: Option<bool>, claims: bool| want.is_none_or(|w| w == claims);
            if !(fits(style.bold, name_claims_bold(&name))
                && fits(style.italic, name_claims_italic(&name)))
            {
                continue;
            }
            let tag = pdfcer_render::font::subset::subset_tag_for(&name);
            match pdfcer_render::font::subset::plan_subset(&bytes, 0, &wanted, &name, &tag) {
                Ok(plan) => plans.push(plan),
                Err(e) => eprintln!(
                    "pdfcer: font-dir: {} ({name}) cannot be a style donor: {e}",
                    path.display()
                ),
            }
        }
    }
    Ok(plans)
}

/// `--augment-subset`'s settings over the `--font-dir` faces. The augmenter
/// is leaked because core holds it as `&'static`; the process runs one edit.
pub(crate) fn subset_augment(
    env: &pdfcer_render::FontEnvironment,
    check: &str,
    hinting: &str,
) -> pdfcer_core::text_edit::SubsetAugment {
    use pdfcer_core::text_edit::{HintingMismatch, OutlineCheck, SubsetAugment};
    let faces = pdfcer_render::font::InstalledFaceAugmenter::from_environment(env);
    SubsetAugment::new(Box::leak(Box::new(faces)))
        .with_outline_check(if check == "shown-only" {
            OutlineCheck::ShownOnly
        } else {
            OutlineCheck::AllShared
        })
        .with_hinting_mismatch(if hinting == "refuse" {
            HintingMismatch::Refuse
        } else {
            HintingMismatch::Strip
        })
}
