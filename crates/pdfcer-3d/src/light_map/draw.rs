//! [`render_light_map`]: the CPU light map.

use super::LuxBands;
use crate::render::raster::{Corner, Target, View, ViewCorner, covers, edge, owned_edges};
use crate::render::{Image, RenderError, checked_size};
use crate::vec3::{dot, sub};
use crate::{Camera, IlluminanceMap, RenderOptions};

/// `map` from `camera`, every patch coloured through `bands`: unlit,
/// opaque, each pixel's figure blended perspective-correct between its
/// small triangle's points, as the graphics-card shaders do. Uncovered
/// pixels are [`RenderOptions::background`]; only the options' size and
/// background play a part.
///
/// # Errors
///
/// [`RenderError`] for an image size of zero or over the ceiling, or an
/// unusable camera.
pub fn render_light_map(
    map: &IlluminanceMap,
    bands: &LuxBands,
    camera: &Camera,
    options: &RenderOptions,
) -> Result<Image, RenderError> {
    let (width, height) = checked_size(options)?;
    let pixels = u64::from(width) * u64::from(height);
    let [right, up, forward] = camera.basis()?;
    let view = View::new(camera, f64::from(width) / f64::from(height))?;
    let mut rgba = options.background.repeat(pixels as usize);
    let mut depth = vec![f64::NEG_INFINITY; pixels as usize];
    let mut target = Target {
        width,
        height,
        top: 0,
        rgba: &mut rgba,
        depth: &mut depth,
    };
    let corner = |p: [f64; 3], f: f64| -> ViewCorner {
        let d = sub(p, camera.eye);
        let mut v = [0.0; 20];
        let values = [dot(d, right), dot(d, up), dot(d, forward), 0.0, 0.0, 0.0, f];
        for (slot, value) in v.iter_mut().zip(values) {
            *slot = value;
        }
        v
    };
    for ([p, q, r], [fp, fq, fr]) in map.triangles() {
        let clipped = view.clip([corner(p, fp), corner(q, fq), corner(r, fr)]);
        let screen: Vec<Corner> = clipped
            .into_iter()
            .map(|v| view.project(v, width, height))
            .collect();
        if let Some((&a, rest)) = screen.split_first() {
            for pair in rest.windows(2) {
                if let [b, c] = pair {
                    target.fill_figure(&view, [a, *b, *c], bands);
                }
            }
        }
    }
    Ok(Image {
        width,
        height,
        rgba,
    })
}

impl Target<'_> {
    /// Fill a triangle whose corners carry a figure in `uv[0]`, coloured
    /// by `bands`, depth-tested as [`Target::fill`].
    fn fill_figure(&mut self, view: &View, corners: [Corner; 3], bands: &LuxBands) {
        let [a, b, c] = corners;
        let area = edge(a.at, b.at, c.at);
        if !area.is_finite() || area.abs() < 1e-12 {
            return;
        }
        let Some((x0, x1, y0, y1)) = self.bounds(&corners) else {
            return;
        };
        let q = corners.map(|k| view.uv_weight(&k));
        let owned = owned_edges(&corners, area);
        let z = |k: &Corner| k.at.get(2).copied().unwrap_or(0.0);
        for y in y0..=y1 {
            for x in x0..=x1 {
                let p = [f64::from(x) + 0.5, f64::from(y) + 0.5, 0.0];
                let w = [
                    edge(b.at, c.at, p),
                    edge(c.at, a.at, p),
                    edge(a.at, b.at, p),
                ]
                .map(|e| e / area);
                if !covers(w, owned) {
                    continue;
                }
                let key: f64 = w.iter().zip(&corners).map(|(k, c)| k * z(c)).sum();
                let i = (y - self.top) as usize * self.width as usize + x as usize;
                if self.depth.get(i).is_none_or(|&d| key <= d) {
                    continue;
                }
                let qw: f64 = w.iter().zip(q).map(|(k, q)| k * q).sum();
                let f: f64 = w
                    .iter()
                    .zip(&corners)
                    .zip(q)
                    .map(|((k, c), q)| k * q * c.uv.first().copied().unwrap_or(0.0))
                    .sum::<f64>()
                    / qw;
                let [r, g, bl] = bands.colour(f);
                self.blend(i, key, [r, g, bl, 255]);
            }
        }
    }
}
