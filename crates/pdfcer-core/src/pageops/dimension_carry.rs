//! Carrying the ce-dimension sidecar (`/PieceInfo` → `pdfcer` → `/Private`,
//! ISO 32000-1 §14.5) into an assembled document, so ce dimensions on the
//! copied pages stay editable ce dimensions.
//!
//! From one source, every group is carried and a record is carried when
//! its annotation was copied, ids unchanged. From several, the sidecars are
//! combined ([`crate::dimension::combine`]). Object references are
//! re-pointed at the copies either way.

use crate::dimension::combine::combine;
use crate::dimension::measure_dict::{mentions, rebuild_ocproperties};
use crate::dimension::{
    DimensionModel, SIDECAR_VERSION, deserialize_model, serialize_model, sidecar_version,
};
use crate::object::{Dict, Name, ObjId, Object};
use crate::pageops::PageOpError;
use crate::pageops::assemble::{AssembleOptions, AssembleReport, Copier, DocumentView};

/// `view`'s sidecar: the `/PieceInfo` `pdfcer` dictionary and its `/Private`.
pub(crate) fn sidecar(view: &DocumentView<'_>) -> Option<(Dict, Object)> {
    let graph = view.graph();
    let entry = |dict: &Dict, key: &[u8]| dict.get(key).map(|v| graph.resolve(v).clone());
    let piece = entry(graph.catalog_dict()?, b"PieceInfo")?;
    let Object::Dict(data) = entry(piece.as_dict()?, b"pdfcer")? else {
        return None;
    };
    let private = entry(&data, b"Private")?;
    Some((data, private))
}

/// The annotation of every ce-dimension record in `view`'s sidecar. One
/// copied without its record arrives as a plain line annotation.
pub(crate) fn record_annots(view: &DocumentView<'_>) -> Vec<ObjId> {
    sidecar(view)
        .and_then(|(_, private)| deserialize_model(&private))
        .map(|model| model.dimensions().iter().filter_map(|r| r.annot).collect())
        .unwrap_or_default()
}

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
    catalog: &mut Dict,
    report: &mut AssembleReport,
) -> Result<(), PageOpError> {
    let Some((data, private)) = sidecar(view) else {
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

/// How many ce-dimension annotations of `view` were copied.
fn copied_records(copier: &Copier, view: &DocumentView<'_>, source_index: usize) -> usize {
    record_annots(view)
        .into_iter()
        .filter(|id| copier.mapped(source_index, *id).is_some())
        .count()
}

/// Carry the sidecars as [`AssembleOptions::carry_dimension_sidecar`]
/// describes, and fill the ce-dimension fields of the report. Call after the
/// copier has drained and before the catalog is stored.
pub(super) fn carry_for(
    copier: &mut Copier,
    sources: &[DocumentView<'_>],
    options: &AssembleOptions,
    catalog: &mut Dict,
    report: &mut AssembleReport,
) -> Result<(), PageOpError> {
    report.ce_dimensions_as_lines = vec![0; sources.len()];
    if options.carry_dimension_sidecar && sources.len() > 1 {
        return carry_all(copier, sources, catalog, report);
    }
    let mut carried_from = None;
    if options.carry_dimension_sidecar
        && let Some(index) = options.catalog_from
        && let Some(view) = sources.get(index)
    {
        carry(copier, view, index, catalog, report)?;
        carried_from = (!report.ce_dimension_sidecar_newer).then_some(index);
    }
    for (index, view) in sources.iter().enumerate() {
        if carried_from != Some(index)
            && let Some(slot) = report.ce_dimensions_as_lines.get_mut(index)
        {
            *slot = copied_records(copier, view, index);
        }
    }
    Ok(())
}

/// Combine every sidecar into one and write it into `catalog`, with the
/// group layers registered in `/OCProperties`. Nothing is written when no
/// ce dimension was carried.
fn carry_all(
    copier: &mut Copier,
    sources: &[DocumentView<'_>],
    catalog: &mut Dict,
    report: &mut AssembleReport,
) -> Result<(), PageOpError> {
    let mut model = DimensionModel::empty();
    let mut stamp = None;
    let mut retired = Vec::new();
    for (index, view) in sources.iter().enumerate() {
        let Some((data, private)) = sidecar(view) else {
            continue;
        };
        let newer = sidecar_version(&private).is_some_and(|v| v > SIDECAR_VERSION);
        let Some(source) = deserialize_model(&private).filter(|_| !newer) else {
            report.ce_dimension_sidecar_newer |= newer;
            let copied = copied_records(copier, view, index);
            report.ce_dimensions_left += copied;
            if let Some(slot) = report.ce_dimensions_as_lines.get_mut(index) {
                *slot = copied;
            }
            continue;
        };
        let out = combine(&mut model, &source, &|id| {
            copier.mapped(index, id).map(|number| ObjId::new(number, 0))
        });
        report.ce_dimensions_carried += out.carried;
        report.ce_dimensions_left += out.left;
        report.ce_dimension_groups_carried += out.groups_carried;
        report.ce_dimension_groups_matched += out.groups_matched;
        report.ce_dimension_groups_renamed += out.groups_renamed;
        for (annot, layer) in out.relayer {
            relayer(copier, annot, layer);
        }
        retired.extend(out.retired_layers);
        if stamp.is_none()
            && out.carried > 0
            && let Some(value) = data.get(b"LastModified")
        {
            stamp = copier.copy_value(view, index, value, 0)?;
        }
    }
    if report.ce_dimensions_carried == 0 {
        return Ok(());
    }

    let mut out = Dict::new();
    if let Some(stamp) = stamp {
        out.insert(Name::from(b"LastModified"), stamp);
    }
    out.insert(Name::from(b"Private"), serialize_model(&model));
    let mut piece = Dict::new();
    piece.insert(Name::from(b"pdfcer"), Object::Dict(out));
    catalog.insert(Name::from(b"PieceInfo"), Object::Dict(piece));
    register_layers(copier, &model, &retired, catalog);
    Ok(())
}

/// Point a copied annotation at `layer`, or strip its `/OC` for `None`.
fn relayer(copier: &mut Copier, annot: ObjId, layer: Option<ObjId>) {
    let Some(Object::Dict(annot)) = copier.object_mut(annot.num) else {
        return;
    };
    match layer {
        Some(layer) => annot.insert(Name::from(b"OC"), Object::Reference(layer)),
        None => annot.0.retain(|(key, _)| key.as_bytes() != b"OC"),
    }
}

/// Register the group layers of `model` in the `/OCProperties` of `catalog`
/// (ISO 32000-1 section 8.11.4.2), dropping each retired layer nothing
/// else refers to. Without an `/OCProperties` to extend, every copied
/// layer is registered, since Table 100 requires `/OCGs` to list them all.
fn register_layers(
    copier: &mut Copier,
    model: &DimensionModel,
    retired: &[ObjId],
    catalog: &mut Dict,
) {
    let groups: Vec<(ObjId, bool)> = model
        .groups()
        .iter()
        .filter_map(|g| g.ocg.map(|layer| (layer, g.visible)))
        .collect();
    let held = match catalog.get(b"OCProperties") {
        Some(Object::Reference(held)) => Some(held.num),
        _ => None,
    };
    let unused: Vec<ObjId> = retired
        .iter()
        .copied()
        .filter(|id| {
            !groups.iter().any(|(layer, _)| layer == id)
                && !copier.stored().any(|(number, value)| {
                    number != id.num && Some(number) != held && mentions(value, *id, 0)
                })
        })
        .collect();
    for id in &unused {
        copier.discard(id.num);
    }
    let existing = catalog.get(b"OCProperties").cloned().or_else(|| {
        let layers: Vec<Object> = copier
            .stored()
            .filter(|(_, value)| is_layer(value))
            .map(|(number, _)| Object::Reference(ObjId::new(number, 0)))
            .collect();
        let mut listed = Dict::new();
        listed.insert(Name::from(b"OCGs"), Object::Array(layers));
        Some(Object::Dict(listed))
    });
    let rebuilt = rebuild_ocproperties(existing.as_ref(), &groups, &unused, &|id| {
        copier.object(id.num).cloned()
    });
    let Some(rebuilt) = rebuilt else {
        return;
    };
    match held {
        Some(number) => copier.store(number, rebuilt),
        None => catalog.insert(Name::from(b"OCProperties"), rebuilt),
    }
}

/// Whether `value` is an optional content group dictionary (Table 98).
fn is_layer(value: &Object) -> bool {
    matches!(value, Object::Dict(d)
        if matches!(d.get(b"Type"), Some(Object::Name(n)) if n.as_bytes() == b"OCG"))
}
