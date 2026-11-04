//! The tessellation sections decoded into the meshes a model draws, and,
//! under a lenient [`MeshFit`], damage contained to the smallest unit it
//! corrupts: that unit is left out and counted in
//! [`AssembledModel::damaged_why`] while the rest draws.
//!
//! Resource ceilings are never contained: they stay errors.

use crate::container::{FileStructure, SectionKind};
use crate::model::{BEST_FIT_REFUSED, Budget};
use crate::{
    AssembleError, AssembledModel, MeshFit, PrcError, PrcFile, Tessellation, TriangleMesh,
};

/// Damage reasons, each `defect: what is left out`.
pub(crate) const MISSING_TESSELLATION: &str =
    "a placement naming a tessellation its file structure does not hold: the placement is left out";
pub(crate) const SHORT_FILE: &str =
    "the file is shorter than its header states: the model file is read to the end of the data";
const FOLLOWS_UNREADABLE: &str =
    "a tessellation after unreadable data in its section: the tessellation is left out";
const SECTION_TRAILER: &str =
    "the tessellation section's trailing data does not read: nothing drawn is left out";

/// The PRC container, a model-file end past the data clipped and counted
/// unless `fit` is strict.
pub(crate) fn parse(
    data: &[u8],
    fit: MeshFit,
    model: &mut AssembledModel,
) -> Result<PrcFile, PrcError> {
    if fit == MeshFit::Unique {
        return PrcFile::parse(data);
    }
    let (prc, short) = PrcFile::parse_clipping(data)?;
    if short {
        damage(model, SHORT_FILE.to_owned(), 1);
    }
    Ok(prc)
}

/// Per file structure, each tessellation's triangle mesh (if it has one),
/// counting what is not drawn into `model`.
pub(crate) fn decode_meshes(
    prc: &PrcFile,
    fit: MeshFit,
    model: &mut AssembledModel,
    flagged: &mut Flagged,
) -> Result<Vec<Vec<Option<TriangleMesh>>>, AssembleError> {
    flagged.lenient = fit != MeshFit::Unique;
    let rows = prc.file_structures.iter().enumerate().map(|(f, fs)| {
        let tess = if flagged.lenient {
            read_section(fs, f, model, flagged)?
        } else {
            fs.tessellations()?.into_iter().map(Some).collect()
        };
        let mut row = Vec::with_capacity(tess.len());
        for (i, t) in tess.into_iter().enumerate() {
            row.push(t.and_then(|t| decode_one(t, (f, i), fit, model, flagged)));
        }
        Ok(row)
    });
    collect_charged(rows, Budget::default())
}

/// Each file structure's meshes, decoded one structure at a time and
/// charged to `budget` as they are, so decoding stops at the structure that
/// passes [`crate::MAX_PLACED_TRIANGLES`] or
/// [`crate::MAX_PLACED_ATTRIBUTES`]: every structure's meshes are held at
/// once, and the per-structure element allowance does not bound their sum.
pub(crate) fn collect_charged(
    rows: impl IntoIterator<Item = Result<Vec<Option<TriangleMesh>>, PrcError>>,
    mut budget: Budget,
) -> Result<Vec<Vec<Option<TriangleMesh>>>, AssembleError> {
    let mut out = Vec::new();
    for row in rows {
        let row = row?;
        for m in row.iter().flatten() {
            budget.take(m.triangles.len(), crate::model::attributes(m))?;
        }
        out.push(row);
    }
    Ok(out)
}

/// One tessellation's mesh, counting it into `model` when it draws none.
fn decode_one(
    t: Tessellation,
    (f, i): (usize, usize),
    fit: MeshFit,
    model: &mut AssembledModel,
    flagged: &mut Flagged,
) -> Option<TriangleMesh> {
    match t {
        Tessellation::Mesh(m) => Some(m),
        Tessellation::Wire(w) => {
            model.wires += 1;
            flagged.wires.push(((f, i), w));
            None
        }
        Tessellation::Compressed {
            mesh: Some(m),
            best_fit,
            ..
        } if !best_fit || fit == MeshFit::BestFit => {
            model.rebuilt += 1;
            model.best_fit += usize::from(best_fit);
            if best_fit {
                flagged.best_fit.push((f, i));
            }
            Some(m)
        }
        Tessellation::Compressed {
            mesh: None,
            partial: Some(p),
            ..
        } if fit.draws_partial() => {
            let p = *p;
            model.partial += 1;
            flagged.partial.push((f, i));
            let m = if fit == MeshFit::Continue {
                p.mesh
            } else {
                p.sound_mesh()
            };
            model.partial_triangles.0 += m.triangles.len();
            model.partial_triangles.1 += p.total;
            count_why(&mut model.partial_why, p.why);
            Some(m)
        }
        Tessellation::Compressed {
            mesh, not_rebuilt, ..
        } => {
            model.compressed += 1;
            flagged.left_out.push((f, i));
            let why = match mesh {
                Some(_) => BEST_FIT_REFUSED.to_owned(),
                None => not_rebuilt.unwrap_or_default(),
            };
            count_why(&mut model.skipped_why, why);
            None
        }
        _ => {
            model.markups += 1;
            None
        }
    }
}

/// A lenient read of one file structure's tessellation section: an entry
/// whose content is damaged is `None`; an entry the reader loses its place
/// in ends the row, and the entries after it are counted as lost.
fn read_section(
    fs: &FileStructure,
    f: usize,
    model: &mut AssembledModel,
    flagged: &mut Flagged,
) -> Result<Vec<Option<Tessellation>>, PrcError> {
    let data = fs.section(SectionKind::Tessellation);
    if data.is_empty() {
        return Ok(Vec::new());
    }
    let mut elements = 0;
    let head = fs.schema_within(&mut elements).and_then(|(schema, _)| {
        let mut ctx = crate::tess::Ctx::new(
            crate::bits::BitReader::new(data),
            &schema,
            fs.authoring_version,
        );
        ctx.elements = elements;
        ctx.lenient = true;
        let n = ctx.tessellation_count()?;
        Ok(read_entries(&mut ctx, (f, n), model, flagged))
    });
    match head {
        Ok(row) => row,
        Err(e) => {
            ceiling(e.clone())?;
            let why = format!(
                "a tessellation section that does not read ({e}): every tessellation in it is left out"
            );
            damage(model, why, 1);
            flagged.lost.push((f, 0, usize::MAX));
            Ok(Vec::new())
        }
    }
}

/// The `n` entries of file structure `f`'s section, from `ctx`.
pub(crate) fn read_entries(
    ctx: &mut crate::tess::Ctx<'_, '_>,
    (f, n): (usize, usize),
    model: &mut AssembledModel,
    flagged: &mut Flagged,
) -> Result<Vec<Option<Tessellation>>, PrcError> {
    let mut row = Vec::with_capacity(n.min(1024));
    for i in 0..n {
        let entry = ctx.tessellation();
        let notes = std::mem::take(&mut ctx.notes);
        match entry {
            Ok(Ok(t)) => {
                if !notes.is_empty() {
                    flagged.damaged.push((f, i));
                }
                for why in notes {
                    damage(model, why.to_owned(), 1);
                }
                row.push(Some(t));
            }
            Ok(Err(e)) => {
                ceiling(e.clone())?;
                damage(model, left_out(&e), 1);
                flagged.damaged.push((f, i));
                row.push(None);
            }
            Err(e) => {
                ceiling(e.clone())?;
                damage(model, left_out(&e), 1);
                flagged.damaged.push((f, i));
                let rest = n - i - 1;
                if rest > 0 {
                    damage(model, FOLLOWS_UNREADABLE.to_owned(), rest);
                    flagged.lost.push((f, i + 1, n));
                }
                return Ok(row);
            }
        }
    }
    if let Err(e) = ctx.tessellation_end() {
        ceiling(e)?;
        damage(model, SECTION_TRAILER.to_owned(), 1);
    }
    Ok(row)
}

/// `e` as the reason its tessellation is left out.
fn left_out(e: &PrcError) -> String {
    format!("{e}: the tessellation is left out")
}

/// `Err(e)` when `e` is a resource ceiling, which is never contained: a
/// model past one is hostile or beyond what can be drawn, and a picture
/// cut at the ceiling would mislead.
fn ceiling(e: PrcError) -> Result<(), PrcError> {
    let hit = match &e {
        PrcError::TooLarge { .. } | PrcError::TooManyElements { .. } => true,
        PrcError::Malformed(m) => {
            m.ends_with("step ceiling reached") || m.ends_with("nesting ceiling reached")
        }
        _ => false,
    };
    if hit { Err(e) } else { Ok(()) }
}

/// Counts `n` units left out for `why`.
pub(crate) fn damage(model: &mut AssembledModel, why: String, n: usize) {
    model.damaged = model.damaged.saturating_add(n);
    match model.damaged_why.iter_mut().find(|(w, _)| *w == why) {
        Some((_, k)) => *k = k.saturating_add(n),
        None => model.damaged_why.push((why, n)),
    }
}

/// Adds one occurrence of `why` to a `(reason, count)` tally.
pub(crate) fn count_why(tally: &mut Vec<(String, usize)>, why: String) {
    match tally.iter_mut().find(|(w, _)| *w == why) {
        Some((_, n)) => *n += 1,
        None => tally.push((why, 1)),
    }
}

/// What [`decode_meshes`] found, as (file structure, tessellation)
/// indices: compressed tessellations left out, rebuilt by best fit or drawn
/// in part; tessellations damaged; and the wires.
#[derive(Default)]
pub(crate) struct Flagged {
    pub(crate) left_out: Vec<(usize, usize)>,
    pub(crate) best_fit: Vec<(usize, usize)>,
    pub(crate) partial: Vec<(usize, usize)>,
    /// Tessellations left out or drawn without a damaged unit, and the
    /// targets of placements left out for naming no tessellation.
    pub(crate) damaged: Vec<(usize, usize)>,
    /// `(file structure, first, end)`: tessellations lost after unreadable
    /// data, already counted, so a placement naming one is not counted again.
    pub(crate) lost: Vec<(usize, usize, usize)>,
    /// Damage is contained, not refused.
    pub(crate) lenient: bool,
    pub(crate) wires: crate::wire::Wires,
}

impl Flagged {
    /// `(f, t)` is damaged or lost.
    pub(crate) fn is_damaged(&self, (f, t): (usize, usize)) -> bool {
        self.damaged.contains(&(f, t)) || self.is_lost((f, t))
    }

    /// `(f, t)` lies in a lost range.
    pub(crate) fn is_lost(&self, (f, t): (usize, usize)) -> bool {
        self.lost
            .iter()
            .any(|&(lf, from, end)| lf == f && (from..end).contains(&t))
    }
}

/// The labels of the parts whose placements draw one of `tess`, as
/// [`part_labels_where`].
pub(crate) fn part_labels(prc: &PrcFile, tess: &[(usize, usize)]) -> Vec<String> {
    if tess.is_empty() {
        return Vec::new();
    }
    part_labels_where(prc, |k| tess.contains(&k))
}

/// The labels of the parts whose placements draw a tessellation `pick`
/// holds: for each such placement, the deepest tree node whose range holds
/// it. A label several such nodes share is listed once, as `label xN`.
pub(crate) fn part_labels_where(
    prc: &PrcFile,
    pick: impl Fn((usize, usize)) -> bool,
) -> Vec<String> {
    let (Ok(nodes), Ok(placements)) = (prc.model_tree(), prc.placements()) else {
        return Vec::new();
    };
    let mut owners: Vec<usize> = Vec::new();
    for (p, pl) in placements.iter().enumerate() {
        if !pick((pl.file_structure, pl.tessellation)) {
            continue;
        }
        let owner = nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| n.placements.contains(&p))
            .max_by_key(|(_, n)| n.depth)
            .map(|(k, _)| k);
        if let Some(k) = owner.filter(|k| !owners.contains(k)) {
            owners.push(k);
        }
    }
    owners.sort_unstable();
    let mut counted: Vec<(String, usize)> = Vec::new();
    for label in owners
        .into_iter()
        .filter_map(|k| nodes.get(k))
        .map(crate::ModelNode::label)
    {
        match counted.iter_mut().find(|(l, _)| *l == label) {
            Some((_, n)) => *n += 1,
            None => counted.push((label, 1)),
        }
    }
    counted
        .into_iter()
        .map(|(l, n)| if n == 1 { l } else { format!("{l} x{n}") })
        .collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // Tests fail loudly by design.
mod tests {
    use super::*;

    #[test]
    fn decoding_stops_at_the_structure_that_passes_the_ceiling() {
        let pulled = std::cell::Cell::new(0);
        let triangle = TriangleMesh {
            positions: vec![[0.0; 3]; 3],
            triangles: vec![[0, 1, 2]],
            ..TriangleMesh::default()
        };
        let rows = std::iter::repeat_with(|| {
            pulled.set(pulled.get() + 1);
            Ok(vec![Some(triangle.clone())])
        })
        .take(3);
        let mut nearly_full = Budget::default();
        nearly_full
            .take(crate::MAX_PLACED_TRIANGLES - 1, 0)
            .unwrap();
        assert_eq!(
            collect_charged(rows, nearly_full).err(),
            Some(AssembleError::PlacedTooLarge {
                what: "triangles",
                limit: crate::MAX_PLACED_TRIANGLES,
            })
        );
        assert_eq!(pulled.get(), 2, "the third structure is never decoded");
    }
}
