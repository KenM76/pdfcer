//! Product-occurrence entity references: the colour, visibility and
//! placement an occurrence imposes on an entity it reaches [WD 7.3.10.1,
//! 7.4.4; `prc__8137__model_tree_asm.md` §12].

use super::*;

/// `PRC_TYPE_TOPO_Face` [WD 6.2; `prc__8137__entity_type_ids.md`].
const TOPO_FACE: u32 = 149;

/// Which entities an occurrence's entity references reach. ISO 14739-1
/// says a reference overrides the referenced entity's properties and not
/// where (`prc__8137__model_tree_asm.md` §12).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum EntityOverrides {
    /// Only where the occurrence's own subtree places the target, so one
    /// instance of a shared part can be coloured alone.
    #[default]
    Subtree,
    /// Every placement of the target anywhere in the model.
    Everywhere,
    /// Read past them: every entity keeps its own and inherited graphics.
    Ignore,
}

/// One `MISC_EntityReference` held by a product occurrence.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Override {
    /// The reference's graphics; style 0 leaves the target's colour alone.
    pub(crate) graphics: Graphics,
    /// The graphics hide the target (Show clear or Removed set).
    pub(crate) hidden: bool,
    /// `index_local_coordinate_system + 1` replacing the target's; 0 none.
    pub(crate) local: u32,
    pub(crate) target: Target,
}

/// What an entity reference names.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Target {
    /// `ReferenceOnPRCBase`: the entity of type `kind` whose PRC unique id
    /// is `uid` in structure `fs` (`None`: the occurrence's own).
    Entity {
        kind: u32,
        fs: Option<UniqueId>,
        uid: u32,
    },
    /// `ReferenceOnTopology` into the B-rep stored as `(context, body)`:
    /// the topological items of type `kind` at `indices` (0-based). With
    /// `fs` `None` it matches that B-rep in any structure the subtree
    /// reaches: producers write "same structure" for an assembly's
    /// reference into a part's own structure.
    Topology {
        kind: u32,
        fs: Option<UniqueId>,
        brep: (u32, u32),
        indices: Vec<u32>,
    },
}

/// The overrides that reach one representation item.
#[derive(Debug, Default)]
pub(crate) struct Reached {
    /// Graphics replacing the item's resolved style, in its owner's
    /// structure.
    pub(crate) item: Option<Graphics>,
    pub(crate) hidden: bool,
    /// The owner's structure and its local coordinate system.
    pub(crate) local: Option<(usize, u32)>,
    /// Per face index, graphics replacing that face's style.
    pub(crate) faces: Vec<(u32, Graphics)>,
}

impl Reached {
    /// Applies `active`, innermost first, so an outer occurrence's
    /// override wins over an inner one's, as an assembly's appearance
    /// beats its sub-assembly's.
    pub(crate) fn collect(
        active: &[(usize, Option<usize>, &Override)],
        fs: usize,
        item: &Item,
    ) -> Self {
        let mut r = Reached::default();
        for &(owner, named, ov) in active.iter().rev() {
            let in_fs = named.is_none_or(|n| n == fs);
            let g = Graphics {
                fs: owner,
                ..ov.graphics
            };
            match &ov.target {
                Target::Entity { kind, uid, .. } => {
                    if !in_fs || !is_item(*kind) || !item.uids.contains(uid) {
                        continue;
                    }
                    if ov.graphics.style != 0 {
                        r.item = Some(g);
                    }
                    r.hidden = ov.hidden;
                    if ov.local != 0 {
                        r.local = Some((owner, ov.local));
                    }
                }
                Target::Topology {
                    kind,
                    brep,
                    indices,
                    ..
                } => {
                    if !in_fs || *kind != TOPO_FACE || item.brep != Some(*brep) {
                        continue;
                    }
                    if ov.graphics.style == 0 {
                        continue;
                    }
                    for &f in indices {
                        r.faces.retain(|(i, _)| *i != f);
                        r.faces.push((f, g));
                    }
                }
            }
        }
        r.faces.sort_unstable_by_key(|(i, _)| *i);
        r
    }
}

/// Representation-item types a `ReferenceOnPRCBase` can colour.
fn is_item(kind: u32) -> bool {
    (RI_BREP_MODEL..=RI_COORDINATE_SYSTEM).contains(&kind)
}

impl Target {
    /// The structure the reference names, if it names one.
    pub(crate) fn fs(&self) -> Option<UniqueId> {
        match self {
            Target::Entity { fs, .. } | Target::Topology { fs, .. } => *fs,
        }
    }
}

/// Per triangle of `mesh`, its face index: the compressed form's stored
/// face, else the `TESS_Face` it was read from.
pub(crate) fn triangle_faces(mesh: &crate::TriangleMesh) -> Vec<u32> {
    if mesh.triangle_faces.len() == mesh.triangles.len() {
        return mesh.triangle_faces.clone();
    }
    let mut out = vec![u32::MAX; mesh.triangles.len()];
    for (i, range) in mesh.faces.iter().enumerate() {
        for f in out.get_mut(range.clone()).into_iter().flatten() {
            *f = i as u32;
        }
    }
    out
}
