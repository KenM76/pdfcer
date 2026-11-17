//! Carrying a source document's ce dimensions into this session on merge
//! and insert, so they stay ce dimensions and join a group
//! ([`crate::dimension::combine`]).

use std::collections::BTreeMap;

use super::{EditSession, ObjectWrite, SIDECAR_DATE, SIDECAR_KEY, ce_dimensions_copied};
use crate::dimension::combine::{Combined, combine};
use crate::dimension::measure_dict::{mentions, rebuild_ocproperties};
use crate::dimension::{
    DimensionModel, SIDECAR_VERSION, deserialize_model, serialize_model, sidecar_version,
};
use crate::graph::ObjectGraph;
use crate::object::{Dict, Name, ObjId, Object};
use crate::pageops::dimension_carry::sidecar;
use crate::view::DocumentView;

/// What [`EditSession::carry_ce_dimensions`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) struct DimensionCarry {
    /// ce dimensions that arrived with their record.
    pub carried: usize,
    /// ce dimensions that arrived as plain line annotations.
    pub as_lines: usize,
    /// Source groups that arrived as groups of their own.
    pub groups_carried: usize,
    /// Source groups that joined a group already here.
    pub groups_matched: usize,
    /// Of `groups_carried`, those given a suffixed name.
    pub groups_renamed: usize,
    /// Whether a sidecar from a newer pdfcer, on either side, is why
    /// `as_lines` is not zero.
    pub sidecar_newer: bool,
}

impl EditSession {
    /// Fold the sidecar records of the ce dimensions `mapping` copied from
    /// `source` into this document's sidecar, staging the catalog and the
    /// re-layered annotations in `scratch`. Call after every other step
    /// that stages the catalog's `/OCProperties`.
    ///
    /// Nothing is staged when no ce dimension was copied, or when either
    /// sidecar was written by a newer pdfcer; the copies are then counted
    /// in [`DimensionCarry::as_lines`].
    pub(super) fn carry_ce_dimensions(
        &self,
        source: &DocumentView<'_>,
        mapping: &BTreeMap<ObjId, ObjId>,
        scratch: &mut BTreeMap<ObjId, Object>,
    ) -> DimensionCarry {
        let mut out = DimensionCarry::default();
        let copied = ce_dimensions_copied(source, mapping);
        if copied == 0 {
            return out;
        }
        out.as_lines = copied;
        let Some((_, private)) = sidecar(source) else {
            return out;
        };
        let source_newer = sidecar_version(&private).is_some_and(|v| v > SIDECAR_VERSION);
        if source_newer || self.check_dimension_sidecar().is_err() {
            out.sidecar_newer = true;
            return out;
        }
        let (Some(incoming), Some(catalog_id)) =
            (deserialize_model(&private), self.graph().catalog_id())
        else {
            return out;
        };
        let Some(catalog) = self.staged_catalog(catalog_id, scratch) else {
            return out;
        };

        let mut model = self
            .try_read_dimension_model()
            .unwrap_or_else(DimensionModel::empty);
        let combined = combine(&mut model, &incoming, &|id| mapping.get(&id).copied());
        out.carried = combined.carried;
        out.as_lines = copied.saturating_sub(combined.carried);
        out.groups_carried = combined.groups_carried;
        out.groups_matched = combined.groups_matched;
        out.groups_renamed = combined.groups_renamed;
        if combined.carried == 0 {
            return out;
        }

        self.stage_combined(&model, &combined, catalog, catalog_id, scratch);
        out
    }

    /// Stage what [`combine`] decided: each annotation on its group's layer,
    /// the layers registered once, and the combined sidecar in the catalog.
    fn stage_combined(
        &self,
        model: &DimensionModel,
        combined: &Combined,
        mut catalog: Dict,
        catalog_id: ObjId,
        scratch: &mut BTreeMap<ObjId, Object>,
    ) {
        for (annot, layer) in &combined.relayer {
            if let Some(Object::Dict(annot)) = scratch.get_mut(annot) {
                match layer {
                    Some(layer) => annot.insert(Name::from(b"OC"), Object::Reference(*layer)),
                    None => annot.0.retain(|(key, _)| key.as_bytes() != b"OC"),
                }
            }
        }
        let groups: Vec<(ObjId, bool)> = model
            .groups()
            .iter()
            .filter_map(|g| g.ocg.map(|layer| (layer, g.visible)))
            .collect();
        let unused = unused_layers(
            &combined.retired_layers,
            &groups,
            &catalog,
            catalog_id,
            scratch,
        );
        for id in &unused {
            scratch.remove(id);
        }
        let rebuilt = rebuild_ocproperties(catalog.get(b"OCProperties"), &groups, &unused, &|id| {
            scratch.get(&id).or_else(|| self.value(id)).cloned()
        });
        if let Some(rebuilt) = rebuilt {
            catalog.insert(Name::from(b"OCProperties"), rebuilt);
        }

        let mut data = Dict::new();
        data.insert(
            Name::from(b"LastModified"),
            Object::String(SIDECAR_DATE.as_bytes().to_vec()),
        );
        data.insert(Name::from(b"Private"), serialize_model(model));
        let mut piece = self
            .deref_dict(catalog.get(b"PieceInfo"))
            .unwrap_or_default();
        piece.insert(Name::from(SIDECAR_KEY), Object::Dict(data));
        catalog.insert(Name::from(b"PieceInfo"), Object::Dict(piece));
        scratch.insert(catalog_id, Object::Dict(catalog));
    }

    /// `scratch` as the object writes of one command.
    pub(super) fn staged_writes(&self, scratch: BTreeMap<ObjId, Object>) -> Vec<ObjectWrite> {
        scratch
            .into_iter()
            .map(|(id, value)| ObjectWrite {
                id,
                before: self.state.get(&id).cloned(),
                after: Some(value),
            })
            .collect()
    }
}

/// The retired layers nothing staged refers to, other than the catalog and
/// the `/OCProperties` it points at, which list every layer.
fn unused_layers(
    retired: &[ObjId],
    groups: &[(ObjId, bool)],
    catalog: &Dict,
    catalog_id: ObjId,
    scratch: &BTreeMap<ObjId, Object>,
) -> Vec<ObjId> {
    let held = catalog.get(b"OCProperties").and_then(Object::as_reference);
    retired
        .iter()
        .copied()
        .filter(|id| {
            !groups.iter().any(|(layer, _)| layer == id)
                && !scratch.iter().any(|(number, value)| {
                    number != id
                        && *number != catalog_id
                        && Some(*number) != held
                        && mentions(value, *id, 0)
                })
        })
        .collect()
}
