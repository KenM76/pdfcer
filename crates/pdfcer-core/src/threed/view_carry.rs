//! Keeping what pdfcer does not edit when a saved 3D view is rewritten: a
//! view read from a file remembers its dictionary, and writing it changes only
//! the keys of the fields that differ (ISO 32000-1 §13.6.4 Table 304).

use std::sync::Arc;

use crate::graph::ObjectGraph;
use crate::object::{Dict, Name, ObjId, Object};

use super::measure::{ThreeDMeasurement, measurement_dict, measurement_entries};
use super::view::{ThreeDSavedView, read_view};
use super::view_write::view_dict;

/// The dictionary a view was read from. Compares equal to any other, so a
/// view read from a file still equals one built with the same fields.
#[derive(Debug, Clone, Default)]
pub(crate) struct ViewSource(Option<Arc<Dict>>);

impl ViewSource {
    /// Remember `dict`, the view dictionary as the file wrote it.
    pub(super) fn of(dict: &Dict) -> Self {
        Self(Some(Arc::new(dict.clone())))
    }

    /// The remembered dictionary; `None` for a view built rather than read.
    pub(crate) fn dict(&self) -> Option<&Dict> {
        self.0.as_deref()
    }
}

impl PartialEq for ViewSource {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

/// What [`crate::edit::EditSession::set_3d_view_measurements`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ThreeDViewMeasuresOutcome {
    /// The `/3D` annotation, which is not written.
    pub annot_id: ObjId,
    /// The 3D stream holding the view.
    pub stream_id: ObjId,
    /// The index into [`super::SavedViews::views`] that was asked for.
    pub view_index: usize,
    /// The one object written: the view itself when `/VA` refers to it, else
    /// an indirect `/VA` array, else the 3D stream. `None` when the list
    /// given is what the view already holds, so nothing was written and no
    /// undo entry was made.
    pub written: Option<ObjId>,
    /// How many measurements pdfcer could read in the view before.
    pub measurements_before: usize,
    /// How many it can read now: the length of the list given.
    pub measurements_after: usize,
    /// `/MA` entries pdfcer cannot read, which stay where they are.
    pub unread_entries_kept: usize,
    /// The stream was reached through a `/3DRef` that other annotations may
    /// name, so they get this change too.
    pub shared_stream: bool,
    /// Off-canvas statements for the shell to show.
    pub disclosures: Vec<String>,
}

/// A view's `/MA` as it should become.
#[derive(Debug, Clone, Default)]
pub(crate) struct MeasureMerge {
    /// The new array. An entry for a measurement the view already held is
    /// its original object, unknown keys and `/S` included.
    pub(crate) entries: Vec<Object>,
    /// Positions in the wanted list that are written anew, the rest being
    /// entries the view already held.
    pub(crate) new: Vec<usize>,
    /// Measurements read from the view before.
    pub(crate) before: usize,
    /// A measurement written anew had a comment link it cannot carry.
    pub(crate) lost_comment: bool,
    /// A measurement left out was linked to a comment.
    pub(crate) removed_comment: bool,
    /// Entries pdfcer cannot read, kept in place.
    pub(crate) unread: usize,
}

/// `wanted` merged into `view`'s `/MA`. Unreadable entries keep their position;
/// `wanted`, in its own order, fills the slots of the readable ones and any
/// excess goes on the end.
pub(crate) fn merge_measurements<G: ObjectGraph + ?Sized>(
    graph: &G,
    view: &Dict,
    wanted: &[ThreeDMeasurement],
) -> MeasureMerge {
    let existing = measurement_entries(graph, view);
    let mut pool: Vec<Option<(&Object, &ThreeDMeasurement)>> = existing
        .iter()
        .filter_map(|(o, m)| m.as_ref().map(|m| Some((*o, m))))
        .collect();
    let before = pool.len();
    let mut lost_comment = false;
    let mut new = Vec::new();
    let placed: Vec<Object> = wanted
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let kept = pool
                .iter_mut()
                .find(|slot| slot.is_some_and(|(_, read)| read == m))
                .and_then(Option::take);
            kept.map_or_else(
                || {
                    lost_comment |= m.has_comment;
                    new.push(i);
                    Object::Dict(measurement_dict(m))
                },
                |(o, _)| o.clone(),
            )
        })
        .collect();
    let removed_comment = pool.iter().flatten().any(|(_, read)| read.has_comment);
    let mut placed = placed.into_iter();
    let mut entries = Vec::new();
    for (object, read) in &existing {
        if read.is_none() {
            entries.push((*object).clone());
        } else if let Some(next) = placed.next() {
            entries.push(next);
        }
    }
    entries.extend(placed);
    MeasureMerge {
        unread: existing.len() - before,
        entries,
        new,
        before,
        lost_comment,
        removed_comment,
    }
}

/// Set `view`'s `/MA` to `entries`, removing the key when there are none.
pub(crate) fn put_measurements(view: &mut Dict, entries: Vec<Object>) {
    if entries.is_empty() {
        view.remove(b"MA");
    } else {
        view.insert(Name::from(b"MA"), Object::Array(entries));
    }
}

/// `source` with `v`'s differences written in. `dict` is `None` when `v` is
/// what `source` reads as, so the entry stays as it is.
pub(crate) fn carry_view<G: ObjectGraph + ?Sized>(
    graph: &G,
    source: &Dict,
    v: &ThreeDSavedView,
) -> (Option<Dict>, MeasureMerge) {
    let was = read_view(graph, source);
    let fresh = view_dict(v);
    let mut out = source.clone();
    let mut changed = false;
    let mut group = |same: bool, keys: &[&[u8]]| {
        if same {
            return;
        }
        changed = true;
        for key in keys {
            if let Some(value) = fresh.get(key) {
                out.insert(Name::from(*key), value.clone());
            } else {
                out.remove(key);
            }
        }
    };
    group(was.name == v.name, &[b"XN"]);
    group(
        was.camera_to_world == v.camera_to_world && was.orbit_distance == v.orbit_distance,
        &[b"MS", b"C2W", b"CO", b"U3DPath"],
    );
    group(
        was.orthographic == v.orthographic
            && was.ortho_scale == v.ortho_scale
            && was.ortho_binding == v.ortho_binding
            && was.field_of_view == v.field_of_view,
        &[b"P"],
    );
    group(was.lighting == v.lighting, &[b"LS"]);
    group(was.render_mode == v.render_mode, &[b"RM"]);
    let mut merge = MeasureMerge::default();
    if was.measurements != v.measurements {
        merge = merge_measurements(graph, source, &v.measurements);
        put_measurements(&mut out, merge.entries.clone());
        changed = true;
    }
    (changed.then_some(out), merge)
}
