//! `3d-views` — write named views into a 3D annotation (ISO 32000-1
//! §13.6.3 Table 300 `/VA`, Table 298 `/3DV`).

use super::*;
use pdfcer_core::document::Document;
use pdfcer_core::threed::{ThreeDArtwork, ThreeDSavedView, ThreeDViewsOutcome};

/// The arguments of `3d-views`, borrowed from the parsed command.
#[cfg_attr(not(feature = "3d"), allow(dead_code))]
pub(crate) struct ViewsThreeDArgs<'a> {
    pub(crate) input: &'a Path,
    pub(crate) index: usize,
    pub(crate) views: &'a [ThreeDView],
    pub(crate) up: Axis3,
    pub(crate) ortho: bool,
    pub(crate) default: Option<ThreeDView>,
    pub(crate) clear: bool,
    pub(crate) apply: bool,
    pub(crate) output: Option<&'a Path>,
    pub(crate) mode: SaveMode,
}

/// `3d-views` — replace the views of the `/3D` annotation `3d-list` numbers
/// `index` with named views fitted to its model.
pub(crate) fn cmd_views_3d(a: &ViewsThreeDArgs<'_>) -> u8 {
    let doc = match open_document(a.input) {
        Ok(doc) => doc,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", a.input.display());
            return exit_code_for_doc(&err);
        }
    };
    let found = pdfcer_core::threed::list_3d(&doc);
    let Some((art, annot_id)) = found.get(a.index).and_then(|art| match art.source {
        ThreeDSource::Stream { .. } => art.annot_id.map(|id| (art, id)),
        _ => None,
    }) else {
        eprintln!(
            "pdfcer: {}: index {} is not a 3D annotation (the document has {} 3D artwork(s); \
             a RichMedia model has no 3D views). Run `pdfcer 3d-list` to see them.",
            a.input.display(),
            a.index,
            found.len()
        );
        return exit::EDIT_REFUSED;
    };
    let views = if a.clear {
        Vec::new()
    } else {
        match fitted_views(a, &doc, art, annot_id) {
            Ok(views) => views,
            Err(code) => return code,
        }
    };
    let default = default_index(a);
    let page_index = art.page_index;
    let mut session = pdfcer_core::edit::EditSession::new(doc);
    let outcome = match session.set_3d_views(page_index, annot_id, &views, default) {
        Ok(o) => o,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", a.input.display());
            return exit::EDIT_REFUSED;
        }
    };
    print_summary(a, page_index, &views, &outcome);
    if !a.apply {
        eprintln!("pdfcer: dry run — pass --apply with --output to write the file.");
        return exit::SUCCESS;
    }
    finish_attachment_save(a.input, &mut session, a.output, a.mode)
}

/// The view a reader opens on: `--default`'s first match, else the first.
fn default_index(a: &ViewsThreeDArgs<'_>) -> Option<usize> {
    if a.clear || a.views.is_empty() {
        return None;
    }
    a.default
        .and_then(|d| a.views.iter().position(|v| *v == d))
        .or(Some(0))
}

fn print_summary(
    a: &ViewsThreeDArgs<'_>,
    page_index: usize,
    views: &[ThreeDSavedView],
    o: &ThreeDViewsOutcome,
) {
    let names: Vec<&str> = views.iter().map(|v| v.name.as_str()).collect();
    let default = o.default.and_then(|d| names.get(d).copied()).unwrap_or("-");
    println!(
        "3d-views {} index={} page={} annot={} stream={} views={} -> {} [{}] default={default} \
         shared={} mode={} applied={}",
        a.input.display(),
        a.index,
        page_index + 1,
        o.annot_id.num,
        o.stream_id.num,
        o.views_before,
        o.views_after,
        names.join(","),
        u8::from(o.shared_stream),
        mode_token(a.mode),
        u8::from(a.apply)
    );
    for d in &o.disclosures {
        eprintln!("note: {d}");
    }
}

/// The annotation's width over height, from its `/Rect`.
#[cfg(feature = "3d")]
fn annot_aspect(doc: &Document, annot_id: pdfcer_core::object::ObjId) -> Option<f64> {
    use pdfcer_core::graph::ObjectGraph;
    let annot = doc.resolved(annot_id).as_dict()?;
    let rect = doc.resolve(annot.get(b"Rect")?).as_array()?;
    let n: Vec<f64> = rect.iter().filter_map(|o| o.as_number()).collect();
    let [x0, y0, x1, y1] = n.as_slice() else {
        return None;
    };
    let (w, h) = ((x1 - x0).abs(), (y1 - y0).abs());
    (w > 0.0 && h > 0.0).then(|| w / h)
}

/// One view per `--view`, each fitted to the model as `3d-render` fits it,
/// or the exit code after the reason is printed.
#[cfg(feature = "3d")]
fn fitted_views(
    a: &ViewsThreeDArgs<'_>,
    doc: &Document,
    art: &ThreeDArtwork,
    annot_id: pdfcer_core::object::ObjId,
) -> Result<Vec<ThreeDSavedView>, u8> {
    let refuse = |why: String| {
        eprintln!(
            "pdfcer: {}: 3D artwork {}: {why}",
            a.input.display(),
            a.index
        );
        exit::EDIT_REFUSED
    };
    if a.views.is_empty() {
        return Err(refuse(
            "name at least one --view, or pass --clear".to_owned(),
        ));
    }
    let data = extract_3d(&doc.view(), art)
        .map_err(|err| refuse(err.to_string()))?
        .data;
    let model = assemble_with(&data, pdfcer_3d::AssembleOptions::default()).map_err(refuse)?;
    let aspect = annot_aspect(doc, annot_id).unwrap_or(4.0 / 3.0);
    a.views
        .iter()
        .map(|v| {
            let named = v.named();
            let (direction, up) = named.direction(a.up.axis());
            let camera =
                pdfcer_3d::Camera::fit_meshes(&model.meshes, direction, up, !a.ortho, aspect)
                    .map_err(|err| refuse(err.to_string()))?;
            ThreeDSavedView::from_camera(named.label(), &camera, aspect)
                .ok_or_else(|| refuse(format!("the {} view has no usable camera", named.label())))
        })
        .collect()
}

#[cfg(not(feature = "3d"))]
fn fitted_views(
    a: &ViewsThreeDArgs<'_>,
    _doc: &Document,
    _art: &ThreeDArtwork,
    _annot_id: pdfcer_core::object::ObjId,
) -> Result<Vec<ThreeDSavedView>, u8> {
    Err(no_3d_feature(a.input, a.index, "3d-views"))
}
