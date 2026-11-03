//! Depth-tested lines and points drawn over a finished frame: a model's
//! wires, and the edges and vertices a render mode adds. Unlit; a caller
//! shades a stroke's colours before handing it over.
//!
//! A line is rasterised as OpenGL draws an aliased one: one pixel per
//! column (or row) along its major axis, at pixel centres in `[start, end)`,
//! widened along the minor axis to the line width; colour interpolated
//! perspective-correct. A point is the square of pixel centres within half
//! its size of it. Both are pulled towards the eye by a bias in model units
//! so an edge lying on a surface is not lost to it, then depth-tested
//! nearer-or-equal and written as a face pixel is ([`Target::blend`]).

use crate::raster::{Target, View, ViewCorner, at, view_point};
use crate::render::{Camera, RenderError};
use crate::vec3::{add, dot, normalize, scale, sub};
use crate::{AssembledModel, ModelLine};

/// A segment in model space, a straight-RGBA colour at each end.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Stroke {
    pub(crate) ends: [[f64; 3]; 2],
    pub(crate) colours: [[u8; 4]; 2],
}

/// The camera a frame's strokes are projected through.
pub(crate) struct Overlay {
    view: View,
    /// Right, up and forward.
    basis: [[f64; 3]; 3],
    eye: [f64; 3],
    /// Model units every stroke is moved towards the eye.
    bias: f64,
}

impl Overlay {
    /// The overlay for `camera` over a `width` × `height` frame.
    pub(crate) fn new(
        camera: &Camera,
        (width, height): (u32, u32),
        bias: f64,
    ) -> Result<Self, RenderError> {
        let aspect = f64::from(width) / f64::from(height);
        Ok(Overlay {
            view: View::new(camera, aspect)?,
            basis: camera.basis()?,
            eye: camera.eye,
            bias,
        })
    }

    /// `p` moved `bias` towards the eye, in view space: along the ray to
    /// the eye in perspective, against the view direction in orthographic.
    fn view_of(&self, p: [f64; 3]) -> ViewCorner {
        let [right, up, forward] = self.basis;
        let towards = match self.view {
            View::Perspective { .. } => normalize(sub(self.eye, p)).unwrap_or([0.0; 3]),
            View::Orthographic { .. } => scale(forward, -1.0),
        };
        let d = sub(add(p, scale(towards, self.bias)), self.eye);
        view_point(dot(d, right), dot(d, up), dot(d, forward))
    }

    /// The part of the segment `a`–`b` (view space) in front of the near
    /// plane, as model-segment parameters.
    fn clip(&self, a: ViewCorner, b: ViewCorner) -> Option<[f64; 2]> {
        let View::Perspective { near, .. } = self.view else {
            return Some([0.0, 1.0]);
        };
        let (za, zb) = (a.get(2).copied()?, b.get(2).copied()?);
        match (za >= near, zb >= near) {
            (true, true) => Some([0.0, 1.0]),
            (false, false) => None,
            (true, false) => Some([0.0, (near - za) / (zb - za)]),
            (false, true) => Some([(near - za) / (zb - za), 1.0]),
        }
    }

    /// Draws `stroke` `width` pixels wide into `target`.
    pub(crate) fn line(&self, target: &mut Target, stroke: &Stroke, width: u32) {
        let [a, b] = stroke.ends.map(|p| self.view_of(p));
        let Some([s0, s1]) = self.clip(a, b) else {
            return;
        };
        let along = |s: f64| -> ViewCorner {
            let mut v = a;
            for (c, d) in v.iter_mut().zip(b) {
                *c += s * (d - *c);
            }
            v
        };
        let (w, h) = (target.width, target.height);
        let p = self.view.project(along(s0), w, h).at;
        let q = self.view.project(along(s1), w, h).at;
        if !p.iter().chain(&q).all(|c| c.is_finite()) {
            return;
        }
        let model_param = |t: f64| {
            let t = match self.view {
                View::Perspective { .. } => {
                    let (kp, kq) = (at(p, 2), at(q, 2));
                    t * kq / ((1.0 - t) * kp + t * kq)
                }
                View::Orthographic { .. } => t,
            };
            s0 + t * (s1 - s0)
        };
        let colour = |t: f64| mix(stroke.colours, model_param(t));
        let x_major = (at(q, 0) - at(p, 0)).abs() >= (at(q, 1) - at(p, 1)).abs();
        let (major, minor) = if x_major { (0, 1) } else { (1, 0) };
        let (m0, m1) = (at(p, major), at(q, major));
        if m0 == m1 {
            return;
        }
        let limit = f64::from(if x_major { w } else { h });
        let first = (m0.min(m1) - 0.5).ceil().max(0.0);
        let end = m0.max(m1).min(limit);
        let mut m = first;
        while m + 0.5 < end {
            let t = (m + 0.5 - m0) / (m1 - m0);
            let across = (at(p, minor) + t * (at(q, minor) - at(p, minor))).floor();
            let key = at(p, 2) + t * (at(q, 2) - at(p, 2));
            let rgba = colour(t);
            let lo = across - f64::from((width.max(1) - 1) / 2);
            for k in 0..width.max(1) {
                let n = lo + f64::from(k);
                let (x, y) = if x_major { (m, n) } else { (n, m) };
                put(target, (x, y), key, rgba);
            }
            m += 1.0;
        }
    }
}

/// Writes `rgba` at whole pixel `(x, y)` if it is in the image and `key` is
/// nearer than or as near as what is there.
fn put(target: &mut Target, (x, y): (f64, f64), key: f64, rgba: [u8; 4]) {
    let (w, h) = (f64::from(target.width), f64::from(target.height));
    if !(x >= 0.0 && x < w && y >= 0.0 && y < h) || rgba[3] == 0 {
        return;
    }
    let i = (y as usize) * (target.width as usize) + x as usize;
    if target.depth.get(i).is_some_and(|&d| key >= d) {
        target.blend(i, key, rgba);
    }
}

/// The colour a fraction `s` of the way from `colours[0]` to `colours[1]`.
fn mix([a, b]: [[u8; 4]; 2], s: f64) -> [u8; 4] {
    let mut out = a;
    for (o, (x, y)) in out.iter_mut().zip(a.into_iter().zip(b)) {
        *o = (f64::from(x) + s * (f64::from(y) - f64::from(x))).round() as u8;
    }
    out
}

/// How far towards the eye `model`'s strokes are drawn: 0.2% of its
/// meshes' bounding radius, so an edge on a face wins the depth test
/// against that face and loses it to anything clearly in front.
pub(crate) fn bias(model: &AssembledModel) -> f64 {
    crate::Bounds::of(&model.meshes).map_or(0.0, |b| b.radius() * 2e-3)
}

/// Every segment of `lines`, coloured as [`ModelLine::colour_at`] says.
pub(crate) fn wire_strokes(lines: &[ModelLine], fallback: [u8; 3]) -> Vec<Stroke> {
    let mut out = Vec::new();
    for line in lines {
        for (i, pair) in line.points.windows(2).enumerate() {
            if let [a, b] = *pair {
                out.push(Stroke {
                    ends: [a, b],
                    colours: [line.colour_at(i, fallback), line.colour_at(i + 1, fallback)],
                });
            }
        }
    }
    out
}

/// Draws `model`'s wires over a frame `rgba` whose depth keys are `depth`.
pub(crate) fn draw_wires(
    model: &AssembledModel,
    camera: &Camera,
    (rgba, depth): (&mut [u8], &mut [f64]),
    options: &crate::RenderOptions,
    bias: f64,
) -> Result<(), RenderError> {
    if model.lines.is_empty() {
        return Ok(());
    }
    let size = (options.width, options.height);
    let overlay = Overlay::new(camera, size, bias)?;
    let mut target = Target {
        width: options.width,
        height: options.height,
        top: 0,
        rgba,
        depth,
    };
    for stroke in wire_strokes(&model.lines, options.colour) {
        overlay.line(&mut target, &stroke, options.line_width);
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::indexing_slicing, clippy::unwrap_used)] // Tests fail loudly by design.
mod tests {
    use super::*;
    use crate::{Bounds, RenderOptions, TriangleMesh, render_model};

    const RED: [u8; 4] = [255, 0, 0, 255];

    /// A grey unit square at z = 0 and a red wire along y = 0.5 at `z`,
    /// seen from +z, orthographic, 64 px square.
    fn frame(z: f64, line_width: u32) -> Vec<u8> {
        let model = AssembledModel {
            meshes: vec![TriangleMesh {
                positions: vec![[0., 0., 0.], [1., 0., 0.], [1., 1., 0.], [0., 1., 0.]],
                triangles: vec![[0, 1, 2], [0, 2, 3]],
                ..TriangleMesh::default()
            }],
            lines: vec![ModelLine {
                points: vec![[0.1, 0.5, z], [0.9, 0.5, z]],
                colour: Some(RED),
                ..ModelLine::default()
            }],
            ..AssembledModel::default()
        };
        let bounds = Bounds::of(&model.meshes).unwrap();
        let camera = Camera::fit(&bounds, [0., 0., -1.], [0., 1., 0.], false, 1.0).unwrap();
        let options = RenderOptions {
            width: 64,
            height: 64,
            line_width,
            ambient_occlusion: false,
            shadows: false,
            ..RenderOptions::default()
        };
        render_model(&model, &camera, &options).unwrap().rgba
    }

    fn red_rows(rgba: &[u8], column: usize) -> Vec<usize> {
        (0..64)
            .filter(|y| rgba[(y * 64 + column) * 4..][..4] == RED)
            .collect()
    }

    #[test]
    fn a_wire_on_or_before_a_face_shows_and_one_behind_it_does_not() {
        assert_eq!(red_rows(&frame(0.0, 1), 32).len(), 1, "on the face");
        assert_eq!(red_rows(&frame(0.1, 1), 32).len(), 1, "in front");
        assert!(red_rows(&frame(-0.1, 1), 32).is_empty(), "behind");
    }

    #[test]
    fn a_wider_line_covers_that_many_rows_in_a_run() {
        let rows = red_rows(&frame(0.0, 3), 32);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[2] - rows[0], 2);
        // A line ends at its last pixel centre: nothing red at x = 0.05.
        assert!(red_rows(&frame(0.0, 3), 3).is_empty());
    }

    #[test]
    fn colour_runs_from_end_to_end() {
        let s = Stroke {
            ends: [[0.0; 3], [1.0, 0.0, 0.0]],
            colours: [[0, 0, 0, 255], [200, 100, 50, 255]],
        };
        assert_eq!(mix(s.colours, 0.5), [100, 50, 25, 255]);
    }
}
