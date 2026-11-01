//! A model readied once for drawing many frames.

use crate::raster::Paint;
use crate::render::{Camera, Image, RenderError, RenderOptions, render_painted};
use crate::{AssembledModel, Texture};

/// The camera-independent per-frame work for an [`AssembledModel`], done
/// once: which textures are opaque (a scan of every texel). It does not
/// borrow the model, so a viewer can keep it beside a model it owns and
/// still repaint that model between frames; pass the model to
/// [`PreparedModel::render`] each frame. [`crate::render_model`] prepares
/// and draws in one call.
///
/// What it holds stays true while the model's `textures` are unchanged.
/// Changing `colours` or `mesh_textures` between frames (to hide or
/// highlight a part) needs no new preparation. If the model passed has a
/// different number of textures than the one prepared, each texture is
/// scanned again for that frame, so the picture is still right, only slower.
///
/// ```
/// use pdfcer_3d::{AssembledModel, Bounds, Camera, PreparedModel, RenderOptions, TriangleMesh};
/// let mut mesh = TriangleMesh::default();
/// mesh.positions = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
/// mesh.triangles = vec![[0, 1, 2]];
/// let mut model = AssembledModel::default();
/// model.meshes = vec![mesh];
/// let prepared = PreparedModel::new(&model);
/// let bounds = Bounds::of(&model.meshes).unwrap();
/// let camera = Camera::fit(&bounds, [0.0, 0.0, -1.0], [0.0, 1.0, 0.0], false, 1.0)?;
/// let options = RenderOptions { width: 32, height: 32, ..RenderOptions::default() };
/// model.colours = vec![Some([255, 0, 0, 255])]; // repaint between frames
/// assert_eq!(prepared.render(&model, &camera, &options)?.rgba.len(), 32 * 32 * 4);
/// # Ok::<(), pdfcer_3d::RenderError>(())
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PreparedModel {
    /// Whether `model.textures[i]` is fully opaque.
    opaque_textures: Vec<bool>,
}

impl PreparedModel {
    /// Prepares `model`; costs one pass over its texture images.
    #[must_use]
    pub fn new(model: &AssembledModel) -> Self {
        PreparedModel {
            opaque_textures: model.textures.iter().map(Texture::is_opaque).collect(),
        }
    }

    /// Draws `model` as [`crate::render_model`] does, reusing the
    /// preparation. `model` is the prepared model, painted as this frame
    /// needs.
    ///
    /// # Errors
    ///
    /// As [`crate::render`].
    pub fn render(
        &self,
        model: &AssembledModel,
        camera: &Camera,
        options: &RenderOptions,
    ) -> Result<Image, RenderError> {
        let (paints, opaque) = self.paints(model, options);
        render_painted(&model.meshes, &paints, &opaque, camera, options)
    }

    /// Each mesh's paint, and whether it is opaque (opaque colour and, when
    /// textured, an opaque texture).
    pub(crate) fn paints<'m>(
        &self,
        model: &'m AssembledModel,
        options: &RenderOptions,
    ) -> (Vec<Paint<'m>>, Vec<bool>) {
        let rescanned;
        let opaque_textures = if self.opaque_textures.len() == model.textures.len() {
            &self.opaque_textures
        } else {
            rescanned = Self::new(model).opaque_textures;
            &rescanned
        };
        let [r, g, b] = options.colour;
        (0..model.meshes.len())
            .map(|i| {
                let colour = model
                    .colours
                    .get(i)
                    .copied()
                    .flatten()
                    .unwrap_or([r, g, b, 255]);
                let slot = model.mesh_textures.get(i).copied().flatten();
                let texture = slot.and_then(|t| model.textures.get(t));
                let texture_opaque =
                    texture.is_none() || slot.and_then(|t| opaque_textures.get(t)) == Some(&true);
                (
                    Paint { colour, texture },
                    colour[3] == 255 && texture_opaque,
                )
            })
            .unzip()
    }
}

#[cfg(test)]
#[allow(clippy::indexing_slicing, clippy::unwrap_used)] // Tests fail loudly by design.
mod tests {
    use super::*;
    use crate::{Bounds, TextureFunction, TextureOrigin, TextureWrap, TriangleMesh};

    fn texture(alpha: u8) -> Texture {
        Texture {
            width: 1,
            height: 1,
            rgba: vec![0, 200, 0, alpha],
            wrap: [TextureWrap::Clamp; 2],
            function: TextureFunction::Replace,
            uv_matrix: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            uv_set: 0,
            origin: TextureOrigin::TopLeft,
        }
    }

    /// A textured square in front of a plain one.
    fn model() -> AssembledModel {
        let square = |z: f64, uv: bool| {
            let mut mesh = TriangleMesh {
                positions: vec![[0.0, 0.0, z], [1.0, 0.0, z], [1.0, 1.0, z], [0.0, 1.0, z]],
                triangles: vec![[0, 1, 2], [0, 2, 3]],
                ..TriangleMesh::default()
            };
            if uv {
                mesh.uvs = vec![[0.5, 0.5]];
                mesh.triangle_uvs = vec![vec![Some([0, 0, 0]); 2]];
            }
            mesh
        };
        AssembledModel {
            meshes: vec![square(1.0, true), square(0.0, false)],
            colours: vec![Some([255, 255, 255, 255]), Some([200, 0, 0, 255])],
            textures: vec![texture(255)],
            mesh_textures: vec![Some(0), None],
            ..AssembledModel::default()
        }
    }

    fn draw(prepared: &PreparedModel, model: &AssembledModel) -> (Vec<u8>, Vec<u8>) {
        let bounds = Bounds::of(&model.meshes).unwrap();
        let camera = Camera::fit(&bounds, [0.0, 0.0, -1.0], [0.0, 1.0, 0.0], false, 1.0).unwrap();
        let options = RenderOptions {
            width: 16,
            height: 16,
            ..RenderOptions::default()
        };
        (
            prepared.render(model, &camera, &options).unwrap().rgba,
            crate::render_model(model, &camera, &options).unwrap().rgba,
        )
    }

    #[test]
    fn a_kept_preparation_draws_a_repainted_model_as_a_fresh_one_does() {
        let mut model = model();
        let prepared = PreparedModel::new(&model);
        let (kept, fresh) = draw(&prepared, &model);
        assert_eq!(kept, fresh);
        // Hide the front part for one frame, as a viewer does.
        model.colours[0] = Some([0, 0, 0, 0]);
        model.mesh_textures[0] = None;
        let (hidden, fresh) = draw(&prepared, &model);
        assert_eq!(hidden, fresh);
        assert_ne!(hidden, kept, "the repaint reached the picture");
    }

    #[test]
    fn a_model_with_other_textures_is_scanned_again() {
        let mut model = model();
        let prepared = PreparedModel::new(&model);
        // Index 0 is now see-through; the preparation said opaque.
        model.textures = vec![texture(60), texture(255)];
        let (kept, fresh) = draw(&prepared, &model);
        assert_eq!(kept, fresh);
    }
}
