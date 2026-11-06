//! `3d-view-measures` — change one saved view's measurements (ISO 32000-2
//! §13.6.7.3).

use super::views::{stream_annotation, with_measures};
use super::*;
use pdfcer_core::threed::{ThreeDMeasurement, ThreeDViewMeasuresOutcome, saved_3d_views};

/// The arguments of `3d-view-measures`, borrowed from the parsed command.
pub(crate) struct ViewMeasuresArgs<'a> {
    pub(crate) input: &'a Path,
    pub(crate) index: usize,
    pub(crate) view: usize,
    pub(crate) measures: &'a ThreeDMeasureArgs,
    pub(crate) remove: &'a [usize],
    pub(crate) clear: bool,
    pub(crate) apply: bool,
    pub(crate) output: Option<&'a Path>,
    pub(crate) mode: SaveMode,
}

/// `3d-view-measures` — the list `3d-list` shows for view `a.view`, with the
/// removals and additions asked for, written back to that view alone.
pub(crate) fn cmd_view_measures_3d(a: &ViewMeasuresArgs<'_>) -> u8 {
    let doc = match open_document(a.input) {
        Ok(doc) => doc,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", a.input.display());
            return exit_code_for_doc(&err);
        }
    };
    let found = pdfcer_core::threed::list_3d(&doc);
    let (art, annot_id) = match stream_annotation(a.input, a.index, &found) {
        Ok(hit) => hit,
        Err(code) => return code,
    };
    let page_index = art.page_index;
    let refuse = |why: String| {
        eprintln!("pdfcer: {}: {why}", a.input.display());
        exit::EDIT_REFUSED
    };
    let views = saved_3d_views(&doc, art).views;
    let Some(view) = views.get(a.view) else {
        return refuse(format!(
            "3D artwork {} has {} saved view(s), so no view {}",
            a.index,
            views.len(),
            a.view
        ));
    };
    let (name, list) = match wanted_list(a, view) {
        Ok(list) => (view.name.clone(), list),
        Err(why) => return refuse(why),
    };
    let mut session = pdfcer_core::edit::EditSession::new(doc);
    let outcome = match session.set_3d_view_measurements(page_index, annot_id, a.view, &list) {
        Ok(o) => o,
        Err(err) => return refuse(err.to_string()),
    };
    print_summary(a, page_index, &name, &list, &outcome);
    if !a.apply {
        eprintln!("pdfcer: dry run — pass --apply with --output to write the file.");
        return exit::SUCCESS;
    }
    finish_attachment_save(a.input, &mut session, a.output, a.mode)
}

/// The view's measurements after `--remove`/`--clear` and the additions.
fn wanted_list(
    a: &ViewMeasuresArgs<'_>,
    view: &pdfcer_core::threed::ThreeDSavedView,
) -> Result<Vec<ThreeDMeasurement>, String> {
    let held = view.measurements.len();
    if let Some(bad) = a.remove.iter().find(|i| **i >= held) {
        return Err(format!(
            "--remove {bad}: view {} has {held} measurement(s)",
            a.view
        ));
    }
    let mut edited = view.clone();
    let mut index = 0;
    edited.measurements.retain(|_| {
        let keep = !a.clear && !a.remove.contains(&index);
        index += 1;
        keep
    });
    let kept = edited.measurements.len();
    let edited = with_measures(edited, a.measures)?;
    if edited.measurements.len() == kept && !a.clear && a.remove.is_empty() {
        return Err(
            "name a measurement to add, a --remove or --clear; nothing was asked for".to_owned(),
        );
    }
    Ok(edited.measurements)
}

fn print_summary(
    a: &ViewMeasuresArgs<'_>,
    page_index: usize,
    name: &str,
    list: &[ThreeDMeasurement],
    o: &ThreeDViewMeasuresOutcome,
) {
    let written = o
        .written
        .map_or_else(|| "-".to_owned(), |id| id.num.to_string());
    println!(
        "3d-view-measures {} index={} view={} name={name:?} page={} annot={} stream={} \
         measurements={} -> {} unread_kept={} written={written} shared={} mode={} applied={}",
        a.input.display(),
        a.index,
        o.view_index,
        page_index + 1,
        o.annot_id.num,
        o.stream_id.num,
        o.measurements_before,
        o.measurements_after,
        o.unread_entries_kept,
        u8::from(o.shared_stream),
        mode_token(a.mode),
        u8::from(a.apply)
    );
    super::print_measurements(list, name);
    for d in &o.disclosures {
        eprintln!("note: {d}");
    }
}
