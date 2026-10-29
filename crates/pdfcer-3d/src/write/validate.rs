//! Checking a [`MeshScene`] and laying it out as PRC entities: the
//! occurrence order, which meshes become parts, which primitives become
//! tessellations.

use super::{MAX_DEPTH, MeshScene, PrcWriteError};

/// One `ProductOccurrence`: the node it is for (`None` for the synthetic
/// root over several roots) and its sons as occurrence indices.
pub(super) struct Occ {
    pub(super) node: Option<usize>,
    pub(super) sons: Vec<u32>,
}

/// The scene as PRC entities.
pub(super) struct Plan {
    /// Occurrences, sons before fathers, the root last.
    pub(super) occurrences: Vec<Occ>,
    /// Mesh index of each part.
    pub(super) parts: Vec<usize>,
    /// Part index of each mesh, when it is placed.
    pub(super) part_of_mesh: Vec<Option<u32>>,
    /// `(mesh, primitive)` of each tessellation, in section order.
    pub(super) tessellations: Vec<(usize, usize)>,
    pub(super) empty_primitives: usize,
}

fn bad(what: &'static str, index: usize) -> PrcWriteError {
    PrcWriteError::BadIndex { what, index }
}

fn count(n: usize) -> Result<u32, PrcWriteError> {
    u32::try_from(n).map_err(|_| PrcWriteError::TooLarge)
}

/// Check `scene` and lay it out.
pub(super) fn plan(scene: &MeshScene) -> Result<Plan, PrcWriteError> {
    check_numbers(scene)?;
    check_forest(scene)?;
    // Style ids and double-scaled colour indices are u32s.
    count(scene.materials.len().saturating_mul(3).saturating_add(2))?;
    let occurrences = occurrences(scene)?;
    let mut plan = Plan {
        occurrences,
        parts: Vec::new(),
        part_of_mesh: vec![None; scene.meshes.len()],
        tessellations: Vec::new(),
        empty_primitives: 0,
    };
    for o in &plan.occurrences {
        let Some(m) = o.node.and_then(|n| scene.nodes.get(n)?.mesh) else {
            continue;
        };
        let Some(slot) = plan.part_of_mesh.get_mut(m) else {
            return Err(bad("mesh", m));
        };
        if slot.is_none() {
            *slot = Some(count(plan.parts.len())?);
            plan.parts.push(m);
        }
    }
    for &m in &plan.parts {
        for (p, prim) in scene
            .meshes
            .get(m)
            .into_iter()
            .flat_map(|m| m.primitives.iter().enumerate())
        {
            check_primitive(scene, prim)?;
            if prim.triangles.is_empty() {
                plan.empty_primitives += 1;
            } else {
                plan.tessellations.push((m, p));
            }
        }
    }
    if plan.tessellations.is_empty() {
        return Err(PrcWriteError::Empty);
    }
    count(plan.tessellations.len())?;
    Ok(plan)
}

fn check_primitive(scene: &MeshScene, p: &super::ScenePrimitive) -> Result<(), PrcWriteError> {
    if let Some(m) = p.material.filter(|&m| m >= scene.materials.len()) {
        return Err(bad("material", m));
    }
    if !p.normals.is_empty() && p.normals.len() != p.positions.len() {
        return Err(bad("normal count", p.normals.len()));
    }
    // Every count the tessellation writes is a u32, coordinates three to a
    // vertex and corner indices six to a triangle.
    count(p.positions.len().saturating_mul(3))?;
    count(p.triangles.len().saturating_mul(6))?;
    let n = p.positions.len();
    match p.triangles.iter().flatten().find(|&&v| v as usize >= n) {
        Some(&v) => Err(bad("vertex", v as usize)),
        None => Ok(()),
    }
}

fn finite(vs: impl IntoIterator<Item = f64>, what: &'static str) -> Result<(), PrcWriteError> {
    match vs.into_iter().all(f64::is_finite) {
        true => Ok(()),
        false => Err(PrcWriteError::BadNumber(what)),
    }
}

fn check_numbers(scene: &MeshScene) -> Result<(), PrcWriteError> {
    if !(scene.units_in_mm.is_finite() && scene.units_in_mm > 0.0) {
        return Err(PrcWriteError::BadNumber("units_in_mm"));
    }
    for n in &scene.nodes {
        if let Some(m) = n.matrix {
            finite(m, "matrix")?;
            if [m[3], m[7], m[11], m[15]] != [0.0, 0.0, 0.0, 1.0] {
                return Err(PrcWriteError::BadNumber("matrix bottom row"));
            }
        }
    }
    finite(scene.materials.iter().flat_map(|m| m.rgba), "colour")?;
    for p in scene.meshes.iter().flat_map(|m| &m.primitives) {
        finite(p.positions.iter().flatten().copied(), "position")?;
        finite(p.normals.iter().flatten().copied(), "normal")?;
    }
    Ok(())
}

/// Every child and root names a node, no node has two parents, no root
/// has one, and no chain is deeper than [`MAX_DEPTH`]. With at most one
/// parent each and parentless roots, what the roots reach has no cycle.
fn check_forest(scene: &MeshScene) -> Result<(), PrcWriteError> {
    let n = scene.nodes.len();
    let mut parent = vec![None; n];
    for (i, node) in scene.nodes.iter().enumerate() {
        for &c in &node.children {
            let slot = parent.get_mut(c).ok_or_else(|| bad("child node", c))?;
            if slot.replace(i).is_some() {
                return Err(PrcWriteError::NotATree("a node has two parents"));
            }
        }
    }
    for &r in &scene.roots {
        match parent.get(r) {
            None => return Err(bad("root node", r)),
            Some(Some(_)) => return Err(PrcWriteError::NotATree("a root has a parent")),
            Some(None) => {}
        }
    }
    let mut depth = vec![0usize; n];
    let mut stack: Vec<usize> = scene.roots.clone();
    while let Some(i) = stack.pop() {
        let d = depth.get(i).copied().unwrap_or(0) + 1;
        if d > MAX_DEPTH {
            return Err(PrcWriteError::NotATree("nested too deeply"));
        }
        for &c in scene
            .nodes
            .get(i)
            .map(|n| n.children.as_slice())
            .unwrap_or(&[])
        {
            if let Some(slot) = depth.get_mut(c) {
                *slot = d;
            }
            stack.push(c);
        }
    }
    Ok(())
}

/// Occurrences with every son before its father and the root last
/// [WD 7.3.6: a son precedes the occurrence that uses it; Acrobat reads in
/// one forward pass and drops a son written after its father], under a
/// synthetic root when there is not exactly one.
fn occurrences(scene: &MeshScene) -> Result<Vec<Occ>, PrcWriteError> {
    let mut out = Vec::new();
    let synthetic = scene.roots.len() != 1;
    if synthetic {
        out.push(Occ {
            node: None,
            sons: Vec::new(),
        });
    }
    // (node, parent occurrence)
    let mut stack: Vec<(usize, Option<usize>)> = scene
        .roots
        .iter()
        .rev()
        .map(|&r| (r, synthetic.then_some(0)))
        .collect();
    while let Some((node, parent)) = stack.pop() {
        let me = out.len();
        if let Some(p) = parent.and_then(|p| out.get_mut(p)) {
            p.sons.push(count(me)?);
        }
        out.push(Occ {
            node: Some(node),
            sons: Vec::new(),
        });
        let children = scene
            .nodes
            .get(node)
            .map(|n| n.children.as_slice())
            .unwrap_or(&[]);
        stack.extend(children.iter().rev().map(|&c| (c, Some(me))));
    }
    let last = count(out.len())?.saturating_sub(1);
    // Reversed pre-order: each subtree precedes its root; sons keep their order.
    out.reverse();
    for o in &mut out {
        for s in &mut o.sons {
            *s = last - *s;
        }
    }
    Ok(out)
}
