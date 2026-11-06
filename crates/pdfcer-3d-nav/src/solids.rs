//! The model's surface a walker meets: the engine's pick index, built once
//! per mesh list, and the shown-mesh mask, read again when visibility
//! changes.

use pdfcer_3d::{Hit, PickIndex, Ray, TriangleMesh};

/// A cached [`PickIndex`] over the meshes a walker collides with.
///
/// The index is keyed by the mesh slice's address and length, so a new or
/// resized mesh list rebuilds it; a list mutated in place at the same
/// address and length must be followed by [`Solids::forget`].
#[derive(Debug, Clone, Default)]
pub struct Solids {
    model: Option<(usize, usize)>,
    index: Option<PickIndex>,
    version: Option<u32>,
    shown: Option<Vec<bool>>,
}

impl Solids {
    /// The nearest shown triangle `ray` meets, at any distance.
    ///
    /// `version` counts visibility changes: when it differs from the last
    /// call's, `shown` is asked for the mask, one entry per mesh (`true`
    /// shown; `None` or a missing entry means shown).
    pub fn hit(
        &mut self,
        meshes: &[TriangleMesh],
        version: u32,
        shown: impl FnOnce() -> Option<Vec<bool>>,
        ray: &Ray,
    ) -> Option<Hit> {
        let key = (meshes.as_ptr() as usize, meshes.len());
        if self.model != Some(key) {
            self.index = Some(PickIndex::new(meshes));
            (self.model, self.version) = (Some(key), None);
        }
        if self.version != Some(version) {
            self.shown = shown();
            self.version = Some(version);
        }
        let mask = self.shown.as_deref();
        self.index.as_ref()?.pick_where(ray, f64::INFINITY, |mesh| {
            mask.is_none_or(|m| m.get(mesh).copied().unwrap_or(true))
        })
    }

    /// Drop the index and mask, so the next [`Solids::hit`] rebuilds both.
    pub fn forget(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn floor(z: f64) -> TriangleMesh {
        let mut m = TriangleMesh::default();
        m.positions = vec![[-1.0, -1.0, z], [1.0, -1.0, z], [0.0, 1.0, z]];
        m.triangles = vec![[0, 1, 2]];
        m
    }

    #[test]
    fn a_hidden_mesh_is_walked_through_and_a_new_mask_is_read_on_a_new_version() {
        let meshes = vec![floor(0.0), floor(-1.0)];
        let ray = Ray {
            origin: [0.0, 0.0, 1.0],
            direction: [0.0, 0.0, -1.0],
            min_distance: 0.0,
        };
        let mut solids = Solids::default();
        assert_eq!(solids.hit(&meshes, 0, || None, &ray).unwrap().mesh, 0);
        // Same version: the mask is not asked for again.
        let hit = solids.hit(&meshes, 0, || Some(vec![false, true]), &ray);
        assert_eq!(hit.unwrap().mesh, 0);
        let hit = solids.hit(&meshes, 1, || Some(vec![false, true]), &ray);
        assert_eq!(hit.unwrap().mesh, 1);
        solids.forget();
        assert_eq!(solids.hit(&meshes, 1, || None, &ray).unwrap().mesh, 0);
    }
}
