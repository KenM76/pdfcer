//! Carrying the ce-dimension sidecar (`/PieceInfo` → `pdfcer` → `/Private`,
//! ISO 32000-1 §14.5) into an assembled document, so ce dimensions on the
//! copied pages stay editable ce dimensions.
//!
//! Every group is carried; a record is carried when its annotation was
//! copied, with its object references re-pointed at the copies.

use crate::dimension::{
    DimensionModel, SIDECAR_VERSION, deserialize_model, serialize_model, sidecar_version,
};
use crate::object::{Dict, Name, ObjId, Object};
use crate::pageops::PageOpError;
use crate::pageops::assemble::{AssembleReport, Copier, DocumentView};

/// Write the subset sidecar into `catalog`. Call after the copier has
/// drained, so every annotation on a copied page already has its number.
///
/// A sidecar this build cannot fully represent (a newer schema version) is
/// left behind whole rather than rewritten lossily; its records are counted
/// in [`AssembleReport::ce_dimensions_left`].
pub(super) fn carry(
    copier: &mut Copier,
    view: &DocumentView<'_>,
    source_index: usize,
    source_catalog: &Dict,
    catalog: &mut Dict,
    report: &mut AssembleReport,
) -> Result<(), PageOpError> {
    let graph = view.graph();
    let entry = |dict: &Dict, key: &[u8]| dict.get(key).map(|v| graph.resolve(v).clone());
    let Some(Object::Dict(data)) = entry(source_catalog, b"PieceInfo")
        .as_ref()
        .and_then(Object::as_dict)
        .and_then(|piece| entry(piece, b"pdfcer"))
    else {
        return Ok(());
    };
    let Some(private) = entry(&data, b"Private") else {
        return Ok(());
    };
    if sidecar_version(&private).is_some_and(|v| v > SIDECAR_VERSION) {
        report.ce_dimension_sidecar_newer = true;
        report.ce_dimensions_left =
            deserialize_model(&private).map_or(0, |model| model.dimensions().len());
        return Ok(());
    }
    let Some(model) = deserialize_model(&private) else {
        return Ok(());
    };

    let mapped = |copier: &Copier, id: ObjId| {
        copier
            .mapped(source_index, id)
            .map(|number| ObjId::new(number, 0))
    };
    let mut subset = DimensionModel::empty();
    for group in model.groups() {
        let mut group = group.clone();
        group.ocg = group.ocg.and_then(|id| mapped(copier, id));
        subset.insert_group(group);
    }
    for record in model.dimensions() {
        let Some(annot) = record.annot.and_then(|id| mapped(copier, id)) else {
            report.ce_dimensions_left += 1;
            continue;
        };
        let mut record = record.clone();
        record.annot = Some(annot);
        record.ap = record.ap.and_then(|id| mapped(copier, id));
        subset.insert_dimension(record);
        report.ce_dimensions_carried += 1;
    }

    let mut out = Dict::new();
    if let Some(stamp) = data.get(b"LastModified")
        && let Some(stamp) = copier.copy_value(view, source_index, stamp, 0)?
    {
        out.insert(Name::from(b"LastModified"), stamp);
    }
    out.insert(Name::from(b"Private"), serialize_model(&subset));
    let mut piece = Dict::new();
    piece.insert(Name::from(b"pdfcer"), Object::Dict(out));
    catalog.insert(Name::from(b"PieceInfo"), Object::Dict(piece));
    Ok(())
}
