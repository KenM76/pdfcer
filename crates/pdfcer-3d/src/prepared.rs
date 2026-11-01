//! A model readied once for drawing many frames.

use crate::raster::Paint;
use crate::render::{Camera, Image, RenderError, RenderOptions, render_painted};
use crate::{AssembledModel, Texture};

/// An [`AssembledModel`] with the per-frame work that does not depend on
/// the camera done once: which textures are opaque (a scan of every
/// texel). Draw it with [`PreparedModel::render`] as often as the view
/// changes; [`crate::render_model`] prepares and draws in one call.
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
/// assert_eq!(prepared.render(&camera, &options)?.rgba.len(), 32 * 32 * 4);
/// # Ok::<(), pdfcer_3d::RenderError>(())
/// ```
#[derive(Debug, Clone)]
pub struct PreparedModel<'m> {
    model: &'m AssembledModel,
    /// Whether `model.textures[i]` is fully opaque.
    opaque_textures: Vec<bool>,
}

impl<'m> PreparedModel<'m> {
    /// Prepares `model`; costs one pass over its texture images.
    #[must_use]
    pub fn new(model: &'m AssembledModel) -> Self {
        PreparedModel {
            model,
            opaque_textures: model.textures.iter().map(Texture::is_opaque).collect(),
        }
    }

    /// The model this draws.
    #[must_use]
    pub fn model(&self) -> &'m AssembledModel {
        self.model
    }

    /// Draws the model as [`crate::render_model`] does.
    ///
    /// # Errors
    ///
    /// As [`crate::render`].
    pub fn render(&self, camera: &Camera, options: &RenderOptions) -> Result<Image, RenderError> {
        let model = self.model;
        let [r, g, b] = options.colour;
        let (paints, opaque): (Vec<Paint>, Vec<bool>) = (0..model.meshes.len())
            .map(|i| {
                let colour = model
                    .colours
                    .get(i)
                    .copied()
                    .flatten()
                    .unwrap_or([r, g, b, 255]);
                let slot = model.mesh_textures.get(i).copied().flatten();
                let texture = slot.and_then(|t| model.textures.get(t));
                let texture_opaque = texture.is_none()
                    || slot.and_then(|t| self.opaque_textures.get(t)) == Some(&true);
                (
                    Paint { colour, texture },
                    colour[3] == 255 && texture_opaque,
                )
            })
            .unzip();
        render_painted(&model.meshes, &paints, &opaque, camera, options)
    }
}
