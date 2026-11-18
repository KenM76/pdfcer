//! Folding one document's ce-dimension model into another's, for merge and
//! insert: the copied ce dimensions stay ce dimensions and join a group.
//!
//! A source group is brought only when a copied ce dimension belongs to it.
//! It joins the destination group of the same name when the two have the
//! same scale, number format, drafting standard and style; otherwise it
//! arrives as a new group, suffixed ` (2)`, ` (3)`… when the name is taken.
//! No destination group's settings are changed, so no ce dimension already
//! in the destination changes value or look.

use std::collections::BTreeMap;

use super::group::{DEFAULT_GROUP_ID, DimensionModel, Group, GroupId};
use super::units::Unit;
use crate::object::ObjId;

/// What [`combine`] did.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Combined {
    /// Records added to the destination.
    pub carried: usize,
    /// Source records whose annotation was not copied.
    pub left: usize,
    /// Source groups that arrived as new groups, renamed ones included, and
    /// a source default group adopted by a destination that had none.
    pub groups_carried: usize,
    /// Source groups that joined a destination group of the same settings.
    pub groups_matched: usize,
    /// Of [`Self::groups_carried`], those given a suffixed name.
    pub groups_renamed: usize,
    /// `(annotation, layer)` for each carried annotation whose `/OC` must
    /// change: `Some` to name that layer, `None` to have no `/OC`.
    pub relayer: Vec<(ObjId, Option<ObjId>)>,
    /// Copied layers no carried ce dimension uses any more, because their
    /// group joined a destination group that has its own layer.
    pub retired_layers: Vec<ObjId>,
}

/// Whether two groups print a measurement the same way. The name, layer and
/// visibility are not part of it.
fn same_settings(a: &Group, b: &Group) -> bool {
    a.scale == b.scale && a.format == b.format && a.standard == b.standard && a.style == b.style
}

/// Add to `dest` every record of `source` whose annotation `mapped` knows,
/// with its object references passed through `mapped`.
///
/// `dest` may be [`DimensionModel::empty`]; it then takes the source's
/// default group as its own.
pub(crate) fn combine(
    dest: &mut DimensionModel,
    source: &DimensionModel,
    mapped: &dyn Fn(ObjId) -> Option<ObjId>,
) -> Combined {
    let mut out = Combined::default();
    let live: Vec<_> = source
        .dimensions()
        .iter()
        .filter_map(|record| Some((record, mapped(record.annot?)?)))
        .collect();
    out.left = source.dimensions().len() - live.len();
    if live.is_empty() {
        return out;
    }

    let mut homes: BTreeMap<GroupId, GroupId> = BTreeMap::new();
    if dest.group(DEFAULT_GROUP_ID).is_none() {
        seed_default(dest, source, mapped, &mut homes, &mut out);
    }
    for (record, annot) in live {
        let group = source.group(record.group);
        let home = match (homes.get(&record.group), group) {
            (Some(home), _) => *home,
            (None, None) => DEFAULT_GROUP_ID,
            (None, Some(group)) => {
                let home = home_for(dest, group, mapped, &mut out);
                homes.insert(record.group, home);
                home
            }
        };
        let id = dest.add_dimension(home, record.kind.clone());
        if let Some(added) = dest.dimension_mut(id) {
            added.annot = Some(annot);
            added.ap = record.ap.and_then(mapped);
            added.style = record.style;
            added.label_override.clone_from(&record.label_override);
        }
        let from = group.and_then(|g| g.ocg).and_then(mapped);
        let to = dest.group(home).and_then(|g| g.ocg);
        if from != to {
            out.relayer.push((annot, to));
            if let Some(from) = from
                && !out.retired_layers.contains(&from)
            {
                out.retired_layers.push(from);
            }
        }
        out.carried += 1;
    }
    out
}

/// Give a group-less `dest` its default group: the source's, when it has
/// one, so a document dimensioned for the first time by a merge keeps the
/// source's default settings.
fn seed_default(
    dest: &mut DimensionModel,
    source: &DimensionModel,
    mapped: &dyn Fn(ObjId) -> Option<ObjId>,
    homes: &mut BTreeMap<GroupId, GroupId>,
    out: &mut Combined,
) {
    let default = source.group(DEFAULT_GROUP_ID).map_or_else(
        || Group::new(DEFAULT_GROUP_ID, "Default", Unit::Millimeter),
        |group| {
            homes.insert(DEFAULT_GROUP_ID, DEFAULT_GROUP_ID);
            out.groups_carried += 1;
            let mut group = group.clone();
            group.ocg = group.ocg.and_then(mapped);
            group
        },
    );
    dest.insert_group(default);
}

/// The destination group a source group's records join, creating it when no
/// group of the same settings carries the name.
fn home_for(
    dest: &mut DimensionModel,
    group: &Group,
    mapped: &dyn Fn(ObjId) -> Option<ObjId>,
    out: &mut Combined,
) -> GroupId {
    let layer = group.ocg.and_then(mapped);
    // One more candidate name than there are groups always finds a free one.
    for n in 1..=dest.groups().len() + 1 {
        let name = if n == 1 {
            group.name.clone()
        } else {
            format!("{} ({n})", group.name)
        };
        let Some(existing) = dest.groups().iter().find(|g| g.name == name) else {
            let id = dest.add_group(name, group.unit());
            if let Some(added) = dest.group_mut(id) {
                added.scale = group.scale;
                added.format = group.format;
                added.standard = group.standard;
                added.style = group.style;
                added.visible = group.visible;
                added.ocg = layer;
            }
            out.groups_carried += 1;
            out.groups_renamed += usize::from(n > 1);
            return id;
        };
        if same_settings(existing, group) {
            let id = existing.id;
            if existing.ocg.is_none()
                && let Some(joined) = dest.group_mut(id)
            {
                joined.ocg = layer;
            }
            out.groups_matched += 1;
            return id;
        }
    }
    DEFAULT_GROUP_ID
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use super::*;
    use crate::dimension::{DimensionKind, ScaleState};
    use crate::vector::{AxisConstraint, Point};

    fn linear() -> DimensionKind {
        DimensionKind::Linear {
            a: Point::new(0.0, 0.0),
            b: Point::new(10.0, 0.0),
            constraint: AxisConstraint::Horizontal,
            offset: 0.0,
            text_along: 0.0,
            extension_gap: [None; 2],
        }
    }

    /// A model with one record in the default group and one in "Arch", the
    /// annotations numbered 10 and 11, the Arch layer 20.
    fn source() -> DimensionModel {
        let mut m = DimensionModel::new();
        let arch = m.add_group("Arch", Unit::Inch);
        m.group_mut(arch).unwrap().ocg = Some(ObjId::new(20, 0));
        let a = m.add_dimension(DEFAULT_GROUP_ID, linear());
        let b = m.add_dimension(arch, linear());
        m.dimension_mut(a).unwrap().annot = Some(ObjId::new(10, 0));
        m.dimension_mut(b).unwrap().annot = Some(ObjId::new(11, 0));
        m
    }

    fn shifted(id: ObjId) -> Option<ObjId> {
        Some(ObjId::new(id.num + 100, 0))
    }

    #[test]
    fn an_empty_destination_takes_the_groups_and_records() {
        let mut dest = DimensionModel::empty();
        let out = combine(&mut dest, &source(), &shifted);
        assert_eq!((out.carried, out.left), (2, 0));
        // The adopted default counts as carried, beside Arch.
        assert_eq!((out.groups_carried, out.groups_matched), (2, 0));
        assert!(out.relayer.is_empty());
        assert_eq!(dest.groups().len(), 2);
        assert_eq!(dest.groups()[1].ocg, Some(ObjId::new(120, 0)));
        assert_eq!(dest.dimensions()[1].annot, Some(ObjId::new(111, 0)));
        assert_eq!(dest.dimensions()[1].group, dest.groups()[1].id);
    }

    #[test]
    fn a_group_of_the_same_settings_is_joined_and_its_layer_kept() {
        let mut dest = source();
        let out = combine(&mut dest, &source(), &shifted);
        assert_eq!((out.groups_carried, out.groups_matched), (0, 2));
        assert_eq!(dest.groups().len(), 2);
        assert_eq!(dest.dimensions().len(), 4);
        // The Arch record moves to the destination's own layer.
        assert_eq!(out.relayer, [(ObjId::new(111, 0), Some(ObjId::new(20, 0)))]);
        assert_eq!(out.retired_layers, [ObjId::new(120, 0)]);
    }

    #[test]
    fn a_group_of_other_settings_arrives_renamed() {
        let mut dest = source();
        let arch = dest.groups()[1].id;
        dest.group_mut(arch).unwrap().scale = ScaleState::OneToOne;
        let out = combine(&mut dest, &source(), &shifted);
        assert_eq!(
            (out.groups_carried, out.groups_matched, out.groups_renamed),
            (1, 1, 1)
        );
        assert_eq!(dest.groups()[2].name, "Arch (2)");
        assert_eq!(dest.group(arch).unwrap().scale, ScaleState::OneToOne);
        // A second fold joins the renamed group rather than adding a third.
        let out = combine(&mut dest, &source(), &shifted);
        assert_eq!((out.groups_carried, out.groups_matched), (0, 2));
        assert_eq!(dest.groups().len(), 3);
    }

    #[test]
    fn a_record_whose_annotation_was_not_copied_is_left() {
        let mut dest = DimensionModel::new();
        let only_arch = |id: ObjId| (id.num != 10).then_some(id);
        let out = combine(&mut dest, &source(), &only_arch);
        assert_eq!((out.carried, out.left), (1, 1));
        let none = |_: ObjId| None;
        let before = dest.clone();
        let out = combine(&mut dest, &source(), &none);
        assert_eq!((out.carried, out.left), (0, 2));
        assert_eq!(dest, before);
    }
}
