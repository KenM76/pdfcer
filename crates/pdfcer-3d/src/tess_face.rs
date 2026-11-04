//! `TESS_Face` triangulation [WD 7.8.5-7.8.6]: a face's entities turned
//! into triangles with their corner normal and texture slots, and the
//! point, normal and texture-coordinate arrays they index.

use crate::PrcError;
use crate::tess::malformed;

/// A fan/strip count's point-count bits; bit 30 is `NORMAL_Single`
/// [WD 7.8.6.2].
const COUNT_MASK: u32 = 0x3FFF_FFFF;
pub(crate) const NORMAL_SINGLE: u32 = 0x4000_0000;

/// Texture-coordinate sets a face may carry. Each set costs a slot per
/// point and a corner triple per triangle of the whole mesh, so the count
/// is capped; a face past it is Malformed.
pub(crate) const MAX_TEXTURE_SETS: usize = 8;

/// The normal array and each triangle's corner normals, or both empty when
/// any slot fails to name a normal or the array ends in a partial triple: a
/// mesh shades from its stored normals wholly or not at all.
pub(crate) fn stored_normals(
    coords: &[f64],
    slots: &[[u32; 3]],
    triangles: usize,
) -> (Vec<[f64; 3]>, Vec<[u32; 3]>) {
    let Ok(normals) = points(coords, "normal") else {
        return (Vec::new(), Vec::new());
    };
    let named = |k: u32| k.is_multiple_of(3) && (k as usize / 3) < normals.len();
    if normals.is_empty() || slots.len() != triangles || !slots.iter().flatten().all(|&k| named(k))
    {
        return (Vec::new(), Vec::new());
    }
    let corners = slots.iter().map(|t| t.map(|k| k / 3)).collect();
    (normals, corners)
}

/// `coords` as (x, y, z) triples [WD 7.8.4: a TESS_3D's or wire's Doubles
/// are triples]; Malformed when the length is not a multiple of 3, as no
/// reading of a partial last point is defined. `what` names the array.
pub(crate) fn points(coords: &[f64], what: &str) -> Result<Vec<[f64; 3]>, PrcError> {
    if !coords.len().is_multiple_of(3) {
        return Err(malformed(format!(
            "{} {what} coordinates are not whole points",
            coords.len()
        )));
    }
    Ok(coords
        .chunks_exact(3)
        .map(|c| match c {
            [x, y, z] => [*x, *y, *z],
            _ => [0.0; 3],
        })
        .collect())
}

/// The point a Double-offset index names [WD 7.8.5.2: multiples of 3].
pub(crate) fn point_at(pts: &[[f64; 3]], idx: u32) -> Result<[f64; 3], PrcError> {
    if !idx.is_multiple_of(3) {
        return Err(malformed(format!(
            "point index {idx} is not a multiple of 3"
        )));
    }
    pts.get(idx as usize / 3)
        .copied()
        .ok_or_else(|| malformed(format!("point index {idx} past the coordinates")))
}

pub(crate) struct Face {
    pub(crate) flags: u32,
    pub(crate) start: usize,
    pub(crate) data: Vec<u32>,
    pub(crate) textures: usize,
    /// `line_attributes`, each a style index + 1: none = the owner's
    /// graphics, one = the whole face's, more = one per triangulation
    /// entity [WD 7.8.6].
    pub(crate) styles: Vec<u32>,
    /// The behaviour bits read when `styles` is not empty.
    pub(crate) behaviour: u16,
    /// Left out of a lenient read: its behaviour did not fit 16 bits.
    pub(crate) damaged: bool,
}

/// Append one [`Graphics`](crate::tree::Graphics) per triangle of `face`,
/// whose entities emitted `entities[i]` triangles each. Entities past the
/// face's styles inherit (style 0).
pub(crate) fn face_graphics(face: &Face, entities: &[usize], out: &mut Vec<crate::tree::Graphics>) {
    let g = |style: u32| crate::tree::Graphics {
        style,
        bits: face.behaviour,
        fs: 0,
    };
    let whole = match face.styles.as_slice() {
        [] => Some(0),
        [one] => Some(*one),
        _ => None,
    };
    for (i, &n) in entities.iter().enumerate() {
        let style = whole.unwrap_or_else(|| face.styles.get(i).copied().unwrap_or(0));
        out.extend(std::iter::repeat_n(g(style), n));
    }
}

/// A `used_entities_flag` block's shape [WD 7.8.5.5].
#[derive(Clone, Copy)]
pub(crate) enum Shape {
    Triangles,
    Fan,
    Strip,
}

/// Per block: `(bit, shape, one normal per entity, textured)`, low bit first.
pub(crate) const BLOCKS: [(u32, Shape, bool, bool); 12] = [
    (0x0002, Shape::Triangles, false, false),
    (0x0004, Shape::Fan, false, false),
    (0x0008, Shape::Strip, false, false),
    (0x0020, Shape::Triangles, true, false),
    (0x0040, Shape::Fan, true, false),
    (0x0080, Shape::Strip, true, false),
    (0x0200, Shape::Triangles, false, true),
    (0x0400, Shape::Fan, false, true),
    (0x0800, Shape::Strip, false, true),
    (0x2000, Shape::Triangles, true, true),
    (0x4000, Shape::Fan, true, true),
    (0x8000, Shape::Strip, true, true),
];

impl Face {
    /// Entity sizes per set block: `(block, [points per entity])`, from
    /// `TriangulatedData` [WD 7.8.6]. A triangle block is one entity of
    /// `3 × count` points; fans and strips list a vertex count each.
    pub(crate) fn entities(&self) -> Result<Vec<(usize, Vec<u32>)>, PrcError> {
        let mut d = self.data.iter().copied();
        let mut next = || {
            d.next()
                .ok_or_else(|| malformed("triangulated data ends early".into()))
        };
        let mut out = Vec::new();
        for (i, (bit, shape, ..)) in BLOCKS.iter().enumerate() {
            if self.flags & bit == 0 {
                continue;
            }
            let sizes = match shape {
                Shape::Triangles => vec![next()?],
                Shape::Fan | Shape::Strip => {
                    let n = next()? as usize;
                    if n > self.data.len() {
                        return Err(malformed("triangulated data ends early".into()));
                    }
                    (0..n).map(|_| next()).collect::<Result<_, _>>()?
                }
            };
            out.push((i, sizes));
        }
        Ok(out)
    }

    /// Texture sets the face's triangles carry: its count when one of its
    /// blocks is textured, at most [`MAX_TEXTURE_SETS`] (a face past it is
    /// refused by [`triangulate`]).
    pub(crate) fn texture_sets(&self) -> usize {
        let textured = BLOCKS
            .iter()
            .any(|&(bit, _, _, textured)| textured && self.flags & bit != 0);
        if textured {
            self.textures.min(MAX_TEXTURE_SETS)
        } else {
            0
        }
    }

    /// Points the face's index slots name: the vertex-colour count
    /// [ISS #820].
    pub(crate) fn point_count(&self) -> Result<usize, PrcError> {
        let mut n = 0usize;
        for (i, sizes) in self.entities()? {
            let tri = matches!(BLOCKS.get(i), Some((_, Shape::Triangles, ..)));
            for s in sizes {
                let s = (s & COUNT_MASK) as usize;
                n = n.saturating_add(if tri { s.saturating_mul(3) } else { s });
            }
        }
        Ok(n)
    }
}

/// One triangulation entity's index slots: per point its position, normal
/// slot (when the array stores normals) and texture slots.
#[derive(Default)]
pub(crate) struct Slots {
    pub(crate) verts: Vec<u32>,
    pub(crate) normals: Vec<u32>,
    /// Per texture set, each point's slot.
    pub(crate) textures: Vec<Vec<u32>>,
}

/// What a face's triangles carry besides positions, built in step with
/// them: corner normal slots and, per texture set, corner texture slots
/// (`None` for a triangle whose face stores fewer sets).
#[derive(Default)]
pub(crate) struct Corners {
    pub(crate) normals: Vec<[u32; 3]>,
    pub(crate) textures: Vec<Vec<Option<[u32; 3]>>>,
}

/// Emit a face's triangles; `normals` = the index array stores normal slots,
/// in which case each triangle's corner normal slots go to `corners`, as do
/// its corner texture slots. Returns the triangles each triangulation entity
/// emitted, in order: one entity per triangle of a triangle block, per fan,
/// per strip [WD 7.8.6].
///
/// Each triangle is charged `charge.per_triangle` elements before its
/// entity is built. Faces may start anywhere in the index array, so F faces
/// over one range of k triangles cost F x k: the input bounds each entity,
/// not their sum. A face declaring more than [`MAX_TEXTURE_SETS`] sets is
/// Malformed before anything is allocated.
pub(crate) fn triangulate(
    face: &Face,
    indices: &[u32],
    normals: bool,
    n_points: usize,
    out: &mut Vec<[u32; 3]>,
    corners: &mut Corners,
    charge: Charge<'_>,
) -> Result<Vec<usize>, PrcError> {
    if face.textures > MAX_TEXTURE_SETS {
        return Err(malformed(format!(
            "{} texture-coordinate sets on a face",
            face.textures
        )));
    }
    let mut entities = Vec::new();
    let mut slots = indices.get(face.start..).unwrap_or(&[]).iter().copied();
    let mut take = || {
        slots
            .next()
            .ok_or_else(|| malformed("face runs past the triangulated index array".into()))
    };
    for (i, sizes) in face.entities()? {
        let Some(&(_, shape, one_normal, textured)) = BLOCKS.get(i) else {
            continue;
        };
        let t = if textured { face.textures } else { 0 };
        for word in sizes {
            let count = (word & COUNT_MASK) as usize;
            // Points in this entity, and whether each point carries its own
            // normal slot.
            let (points, per_point_normal) = match shape {
                Shape::Triangles => (count.saturating_mul(3), !one_normal),
                Shape::Fan | Shape::Strip => (count, !one_normal || word & NORMAL_SINGLE == 0),
            };
            if points.saturating_mul(t + 1) > indices.len() {
                return Err(malformed(
                    "face runs past the triangulated index array".into(),
                ));
            }
            let triangles = match shape {
                Shape::Triangles => count,
                Shape::Fan | Shape::Strip => points.saturating_sub(2),
            };
            let cost = (triangles as u64).saturating_mul(charge.per_triangle);
            crate::elements::charge(charge.spent, cost)?;
            let mut s = Slots {
                textures: vec![Vec::with_capacity(points); t],
                ..Slots::default()
            };
            let mut current = 0;
            for k in 0..points {
                let new_entity = match shape {
                    Shape::Triangles => k % 3 == 0,
                    _ => k == 0,
                };
                if normals && (per_point_normal || new_entity) {
                    current = take()?;
                }
                if normals {
                    s.normals.push(current);
                }
                for set in &mut s.textures {
                    set.push(take()?);
                }
                let p = take()?;
                if p % 3 != 0 || p as usize / 3 >= n_points {
                    return Err(malformed(format!("point index {p} is not a point")));
                }
                s.verts.push(p / 3);
            }
            let made = emit(shape, &s, out, corners);
            match shape {
                Shape::Triangles => entities.extend(std::iter::repeat_n(1, made)),
                Shape::Fan | Shape::Strip => entities.push(made),
            }
        }
    }
    Ok(entities)
}

/// The decoded-element allowance a face's triangles are charged to.
pub(crate) struct Charge<'a> {
    pub(crate) spent: &'a mut u64,
    /// Elements one triangle costs ([`triangle_elements`]).
    pub(crate) per_triangle: u64,
}

/// The 8-byte elements of [`crate::MAX_DECODED_ELEMENTS`] one `TESS_3D`
/// triangle holds: its corners, its normal-slot corners when the index
/// array stores them, a corner triple per texture set any face of the mesh
/// uses (the [`Corners`] padding gives every triangle one) and its
/// graphics when the mesh is styled.
pub(crate) fn triangle_elements(normals: bool, sets: usize, styled: bool) -> u64 {
    let units = |bytes: usize| bytes.div_ceil(8) as u64;
    let corners = units(size_of::<[u32; 3]>());
    let normals = if normals { corners } else { 0 };
    let texture = units(size_of::<Option<[u32; 3]>>()).saturating_mul(sets as u64);
    let style = if styled {
        units(size_of::<crate::tree::Graphics>())
    } else {
        0
    };
    corners
        .saturating_add(normals)
        .saturating_add(texture)
        .saturating_add(style)
}

/// Append the triangles of one entity's `slots` to `out` and `corners`,
/// keeping every texture set as long as `out`. Returns how many it made.
pub(crate) fn emit(
    shape: Shape,
    slots: &Slots,
    out: &mut Vec<[u32; 3]>,
    corners: &mut Corners,
) -> usize {
    let before = out.len();
    assemble(shape, &slots.verts, out);
    assemble(shape, &slots.normals, &mut corners.normals);
    let made = out.len() - before;
    while corners.textures.len() < slots.textures.len() {
        corners.textures.push(vec![None; before]);
    }
    for (k, set) in corners.textures.iter_mut().enumerate() {
        match slots.textures.get(k) {
            Some(own) => {
                let mut tris = Vec::with_capacity(made);
                assemble(shape, own, &mut tris);
                set.extend(tris.into_iter().map(Some));
            }
            None => set.extend(std::iter::repeat_n(None, made)),
        }
    }
    made
}

/// Per texture-coordinate set, each triangle's corner pair indices.
pub(crate) type UvSets = Vec<Vec<Option<[u32; 3]>>>;

/// The texture coordinates as (u, v) pairs, and per set the corner slots
/// turned into pair indices [`prc__8137__tess_3d.md` §9: a slot is a Double
/// offset, so even]; a triangle with a slot naming no pair is `None`.
pub(crate) fn stored_uvs(coords: &[f64], sets: UvSets) -> (Vec<[f64; 2]>, UvSets) {
    let uvs: Vec<[f64; 2]> = coords
        .chunks_exact(2)
        .map(|c| match c {
            [u, v] => [*u, *v],
            _ => [0.0; 2],
        })
        .collect();
    if uvs.is_empty() {
        return (Vec::new(), Vec::new());
    }
    let pair = |k: u32| (k.is_multiple_of(2) && (k as usize / 2) < uvs.len()).then_some(k / 2);
    let sets = sets
        .into_iter()
        .map(|set| {
            set.into_iter()
                .map(|tri| {
                    let [a, b, c] = tri?;
                    Some([pair(a)?, pair(b)?, pair(c)?])
                })
                .collect()
        })
        .collect();
    (uvs, sets)
}

/// Append the triangles `shape` makes of `verts`.
pub(crate) fn assemble(shape: Shape, verts: &[u32], out: &mut Vec<[u32; 3]>) {
    match shape {
        Shape::Triangles => out.extend(verts.chunks_exact(3).filter_map(|c| match c {
            [a, b, c] => Some([*a, *b, *c]),
            _ => None,
        })),
        Shape::Fan => {
            if let Some((&c, rim)) = verts.split_first() {
                out.extend(rim.windows(2).filter_map(|w| match w {
                    [a, b] => Some([c, *a, *b]),
                    _ => None,
                }));
            }
        }
        Shape::Strip => out.extend(verts.windows(3).enumerate().filter_map(|(k, w)| match w {
            [a, b, c] if k % 2 == 0 => Some([*a, *b, *c]),
            [a, b, c] => Some([*b, *a, *c]),
            _ => None,
        })),
    }
}

/// `mesh` with only the triangles `keep` marks, each per-triangle array and
/// face range following.
pub(crate) fn keep_triangles(mesh: &crate::TriangleMesh, keep: &[bool]) -> crate::TriangleMesh {
    fn pick<T: Clone>(from: &[T], keep: &[bool]) -> Vec<T> {
        from.iter()
            .zip(keep)
            .filter(|(_, k)| **k)
            .map(|(t, _)| t.clone())
            .collect()
    }
    let mut out = mesh.clone();
    out.triangles = pick(&mesh.triangles, keep);
    out.triangle_normals = pick(&mesh.triangle_normals, keep);
    out.triangle_graphics = pick(&mesh.triangle_graphics, keep);
    out.triangle_faces = pick(&mesh.triangle_faces, keep);
    out.triangle_uvs = mesh.triangle_uvs.iter().map(|s| pick(s, keep)).collect();
    let mut before = Vec::with_capacity(keep.len() + 1);
    before.push(0);
    for &k in keep {
        before.push(before.last().copied().unwrap_or(0) + usize::from(k));
    }
    let at = |i: usize| before.get(i.min(keep.len())).copied().unwrap_or(0);
    out.faces = mesh.faces.iter().map(|r| at(r.start)..at(r.end)).collect();
    out
}
