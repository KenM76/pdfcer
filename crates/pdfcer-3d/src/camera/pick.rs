//! Picking: the model point under an image pixel, the inverse of the
//! projection [`crate::render`] draws with, so a pick lands on what the
//! rendered image shows at that pixel.

use crate::render::corners;
use crate::render::raster::View;
use crate::vec3::{add, cross, dot, normalize, scale, sub, unit_cross};
use crate::{Camera, RenderError, TriangleMesh};

/// A half-line through the scene, in model coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ray {
    /// Where the ray starts: the eye (perspective, fisheye) or the point on the
    /// image plane through the eye (orthographic).
    pub origin: [f64; 3],
    /// The unit direction.
    pub direction: [f64; 3],
    /// The smallest distance along the ray that counts as a hit: the
    /// renderer's near plane under perspective, and negative infinity under
    /// orthographic projection (which draws behind the eye too).
    pub min_distance: f64,
}

/// The nearest triangle a [`Ray`] meets ([`pick`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hit {
    /// The index of the mesh in the slice passed to [`pick`].
    pub mesh: usize,
    /// The index of the triangle in that mesh's
    /// [`TriangleMesh::triangles`].
    pub triangle: usize,
    /// The hit point, model coordinates.
    pub point: [f64; 3],
    /// The triangle's unit face normal from its winding (counter-clockwise
    /// seen from the outside, so it points out of a closed solid). It is
    /// not turned towards the ray: a hit on a back face reports the normal
    /// pointing away from the viewer.
    pub normal: [f64; 3],
    /// The distance from [`Ray::origin`] to [`Self::point`] along the ray.
    pub distance: f64,
}

impl Camera {
    /// The image position `point` is drawn at in a `width` × `height`
    /// rendering from this camera, y down: the inverse of
    /// [`Self::ray_through`], through the projection [`crate::render`] draws
    /// with.
    ///
    /// `None` when nothing is drawn for it: nearer than the near plane or
    /// behind the eye, at the eye, straight behind a radial fisheye, a point
    /// that is not finite, a zero size, a camera the renderer refuses, or a
    /// position that overflows. Orthographic draws behind the eye too, so it
    /// is only `None` for the reasons that are not about depth.
    ///
    /// A point in front of the camera but outside the picture, or past a
    /// fisheye's circle, is not `None`: it gets its position past the edge or
    /// circle, so a caller can clip a line that leaves the picture.
    ///
    /// Under the 360° panorama every direction has a position. Longitude
    /// runs across and latitude up, so a point straight behind the eye lands
    /// on the right edge (x = `width`; on the left edge, x = 0, when its
    /// camera-space x is -0), and a point straight up or down lands on the
    /// top or bottom edge. The picture wraps: when two points' positions are
    /// more than half the width apart, the line between them crosses the
    /// left/right seam, and the caller must split it there rather than draw
    /// across the picture.
    ///
    /// # Examples
    ///
    /// ```
    /// use pdfcer_3d::{Camera, Projection};
    /// let camera = Camera {
    ///     eye: [0.0, 0.0, 10.0],
    ///     target: [0.0; 3],
    ///     up: [0.0, 1.0, 0.0],
    ///     projection: Projection::Perspective { fov_y: 30.0 },
    /// };
    /// assert_eq!(camera.pixel_of([0.0; 3], 100, 100), Some([50.0, 50.0]));
    /// assert_eq!(camera.pixel_of([0.0, 0.0, 20.0], 100, 100), None);
    /// ```
    #[must_use]
    pub fn pixel_of(&self, point: [f64; 3], width: u32, height: u32) -> Option<[f64; 2]> {
        if width == 0 || height == 0 || !point.iter().all(|c| c.is_finite()) {
            return None;
        }
        let [right, up, forward] = self.basis().ok()?;
        let from_eye = sub(point, self.eye);
        let c = [
            dot(from_eye, right),
            dot(from_eye, up),
            dot(from_eye, forward),
        ];
        let lens = crate::camera::lens::Lens::for_camera(self, width, height).ok()?;
        let out = match lens {
            Some(lens) if lens.reach_of(c) >= lens.near => lens.pixel(c)?,
            Some(_) => return None,
            None => planar_pixel(self, c, width, height)?,
        };
        out.iter().all(|v| v.is_finite()).then_some(out)
    }

    /// The ray through image position (`x`, `y`) of a `width` × `height`
    /// rendering from this camera: pixel `(i, j)`'s centre is
    /// `(i + 0.5, j + 0.5)`, y down. The inverse of the projection
    /// [`crate::render`] uses, so [`pick`] along it finds the surface drawn
    /// at that pixel.
    ///
    /// # Errors
    ///
    /// [`RenderError::Camera`] for a camera [`crate::render`] refuses, a
    /// position that is not finite, or a ray whose origin or direction
    /// overflows the float range; [`RenderError::Size`] for a zero `width` or `height`. Under a
    /// fisheye: [`RenderError::FisheyeField`] for a refused field, and
    /// [`RenderError::OutsideLens`] for a position outside the picture
    /// circle, which shows only background. A fisheye ray starts at the
    /// eye and its [`Ray::min_distance`] is the near plane of the lens face
    /// drawing that pixel ([`crate::FrameLens`]).
    ///
    /// # Examples
    ///
    /// ```
    /// use pdfcer_3d::{Camera, Projection};
    /// let camera = Camera {
    ///     eye: [0.0, 0.0, 10.0],
    ///     target: [0.0; 3],
    ///     up: [0.0, 1.0, 0.0],
    ///     projection: Projection::Perspective { fov_y: 30.0 },
    /// };
    /// let ray = camera.ray_through(50.0, 50.0, 100, 100)?;
    /// assert_eq!(ray.direction, [0.0, 0.0, -1.0]);
    /// # Ok::<(), pdfcer_3d::RenderError>(())
    /// ```
    pub fn ray_through(&self, x: f64, y: f64, width: u32, height: u32) -> Result<Ray, RenderError> {
        if width == 0 || height == 0 {
            return Err(RenderError::Size { width, height });
        }
        if !(x.is_finite() && y.is_finite()) {
            return Err(RenderError::Camera(
                "the image position is not a finite number",
            ));
        }
        if let Some(lens) = crate::camera::lens::Lens::for_camera(self, width, height)? {
            return finite_ray(lens.ray(self, x, y)?);
        }
        let aspect = f64::from(width) / f64::from(height);
        let view = View::new(self, aspect)?;
        let [right, up, forward] = self.basis()?;
        let nx = x / f64::from(width) * 2.0 - 1.0;
        let ny = 1.0 - y / f64::from(height) * 2.0;
        let ray = match view {
            View::Perspective { focal, near, .. } => {
                let along = add(
                    add(scale(right, nx * aspect / focal), scale(up, ny / focal)),
                    forward,
                );
                let direction = normalize(along).ok_or(RenderError::Camera(
                    "the image position is not a finite number",
                ))?;
                Ray {
                    origin: self.eye,
                    direction,
                    min_distance: near / dot(direction, forward),
                }
            }
            View::Orthographic { half_height, .. } => {
                let offset = add(
                    scale(right, nx * half_height * aspect),
                    scale(up, ny * half_height),
                );
                Ray {
                    origin: add(self.eye, offset),
                    direction: forward,
                    min_distance: f64::NEG_INFINITY,
                }
            }
        };
        finite_ray(ray)
    }
}

/// `ray`, refused when its origin or direction overflowed the float range.
fn finite_ray(ray: Ray) -> Result<Ray, RenderError> {
    if ray
        .origin
        .iter()
        .chain(&ray.direction)
        .all(|c| c.is_finite())
    {
        Ok(ray)
    } else {
        Err(RenderError::Camera("the image position is out of range"))
    }
}

/// The nearest triangle of `meshes` that `ray` meets at or beyond
/// [`Ray::min_distance`], or `None` when it meets none. Both faces of a
/// triangle count (the renderer draws both); a triangle with a missing or
/// non-finite corner, or no area, is skipped as the renderer skips it.
///
/// # Examples
///
/// ```
/// use pdfcer_3d::{Ray, pick};
/// let ray = Ray { origin: [0.0; 3], direction: [0.0, 0.0, 1.0], min_distance: 0.0 };
/// assert_eq!(pick(&[], &ray), None);
/// ```
pub fn pick(meshes: &[TriangleMesh], ray: &Ray) -> Option<Hit> {
    let mut best: Option<Hit> = None;
    for (m, mesh) in meshes.iter().enumerate() {
        for (t, tri) in mesh.triangles.iter().enumerate() {
            let Some(hit) = corners(mesh, tri).and_then(|c| intersect(&c, ray)) else {
                continue;
            };
            if best.is_none_or(|b| hit.distance < b.distance) {
                best = Some(Hit {
                    mesh: m,
                    triangle: t,
                    ..hit
                });
            }
        }
    }
    best
}

/// Möller–Trumbore ray–triangle intersection, two-sided.
pub(crate) fn intersect(c: &[[f64; 3]; 3], ray: &Ray) -> Option<Hit> {
    intersect_weighted(c, ray).map(|(hit, _)| hit)
}

/// [`intersect`], also giving the hit's barycentric weights on `c`'s corners
/// in order (they sum to 1).
pub(crate) fn intersect_weighted(c: &[[f64; 3]; 3], ray: &Ray) -> Option<(Hit, [f64; 3])> {
    let e1 = sub(c[1], c[0]);
    let e2 = sub(c[2], c[0]);
    let normal = unit_cross(e1, e2)?;
    let p = cross(ray.direction, e2);
    let det = dot(e1, p);
    if det.abs() < f64::EPSILON * dot(e1, e1).max(dot(e2, e2)) {
        return None;
    }
    let s = sub(ray.origin, c[0]);
    let u = dot(s, p) / det;
    let q = cross(s, e1);
    let v = dot(ray.direction, q) / det;
    let distance = dot(e2, q) / det;
    let inside = (0.0..=1.0).contains(&u) && v >= 0.0 && u + v <= 1.0;
    let point = add(ray.origin, scale(ray.direction, distance));
    (inside
        && distance.is_finite()
        && distance >= ray.min_distance
        && point.iter().all(|c| c.is_finite()))
    .then_some((
        Hit {
            mesh: 0,
            triangle: 0,
            point,
            normal,
            distance,
        },
        [1.0 - u - v, u, v],
    ))
}

/// The picture position of camera-space point `[x, y, z]` under a planar
/// projection, as the rasterizer places a corner: divided before multiplied,
/// so an on-screen point's intermediate values stay finite.
fn planar_pixel(camera: &Camera, [x, y, z]: [f64; 3], width: u32, height: u32) -> Option<[f64; 2]> {
    let aspect = f64::from(width) / f64::from(height);
    let (nx, ny) = match View::new(camera, aspect).ok()? {
        View::Perspective { focal, near, .. } => {
            if z < near {
                return None;
            }
            (x / z * (focal / aspect), y / z * focal)
        }
        View::Orthographic { half_height, .. } => (x / half_height / aspect, y / half_height),
    };
    Some([
        (nx + 1.0) / 2.0 * f64::from(width),
        (1.0 - ny) / 2.0 * f64::from(height),
    ])
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing
)] // Tests fail loudly by design.
#[allow(clippy::single_range_in_vec_init)] // One face spanning every triangle is intended.
mod tests {
    use super::*;
    use crate::Projection;

    /// A cube of half-size 1 at the origin, each face's triangles wound
    /// counter-clockwise from outside.
    fn cube() -> TriangleMesh {
        let mut positions = Vec::new();
        let mut triangles = Vec::new();
        for axis in 0..3 {
            for sign in [1.0, -1.0] {
                let mut c = [0.0; 3];
                c[axis] = sign;
                let (mut u, mut v) = ([0.0; 3], [0.0; 3]);
                u[(axis + 1) % 3] = 1.0;
                v[(axis + 2) % 3] = 1.0;
                let mut quad = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)];
                if sign < 0.0 {
                    quad.reverse();
                }
                let base = u32::try_from(positions.len()).unwrap();
                for (a, b) in quad {
                    positions.push(add(c, add(scale(u, a), scale(v, b))));
                }
                triangles.push([base, base + 1, base + 2]);
                triangles.push([base, base + 2, base + 3]);
            }
        }
        TriangleMesh {
            positions,
            triangles,
            faces: vec![0..12],
            normals_recalculated: true,
            normals: Vec::new(),
            triangle_normals: Vec::new(),
            triangle_graphics: Vec::new(),
            triangle_faces: Vec::new(),
            uvs: Vec::new(),
            triangle_uvs: Vec::new(),
        }
    }

    fn camera(eye: [f64; 3], projection: Projection) -> Camera {
        Camera {
            eye,
            target: [0.0; 3],
            up: [0.0, 1.0, 0.0],
            projection,
        }
    }

    fn near(a: [f64; 3], b: [f64; 3]) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-9)
    }

    #[test]
    fn the_centre_pixel_hits_the_facing_face_with_its_outward_normal() {
        let cam = camera([0.0, 0.0, 10.0], Projection::Perspective { fov_y: 30.0 });
        let ray = cam.ray_through(50.0, 50.0, 100, 100).unwrap();
        let hit = pick(&[cube()], &ray).expect("hits");
        assert!(near(hit.point, [0.0, 0.0, 1.0]), "{hit:?}");
        assert!(near(hit.normal, [0.0, 0.0, 1.0]), "{hit:?}");
        assert!((hit.distance - 9.0).abs() < 1e-9);
        assert!(
            (8..10).contains(&hit.triangle),
            "the +z face is triangles 8 and 9"
        );
    }

    /// The pick inverts the renderer's projection: a hit point projects
    /// back onto the pixel it was picked from, under both projections and
    /// a non-square image.
    #[test]
    fn a_hit_projects_back_onto_its_pixel() {
        let eye = [6.0, 5.0, 8.0];
        for projection in [
            Projection::Perspective { fov_y: 40.0 },
            Projection::Orthographic { height: 4.0 },
        ] {
            let cam = camera(eye, projection);
            let (w, h) = (160, 90);
            let view = View::new(&cam, f64::from(w) / f64::from(h)).unwrap();
            let [right, up, forward] = cam.basis().unwrap();
            for (x, y) in [(80.5, 45.5), (75.5, 40.5), (88.5, 50.5)] {
                let ray = cam.ray_through(x, y, w, h).unwrap();
                let hit = pick(&[cube()], &ray).expect("hits");
                let d = sub(hit.point, cam.eye);
                let at = view
                    .project(
                        crate::render::raster::view_point(
                            dot(d, right),
                            dot(d, up),
                            dot(d, forward),
                        ),
                        w,
                        h,
                    )
                    .at;
                assert!(
                    (at[0] - x).abs() < 1e-6 && (at[1] - y).abs() < 1e-6,
                    "{at:?}"
                );
            }
        }
    }

    #[test]
    fn the_nearer_face_wins_and_a_ray_past_the_model_misses() {
        let cam = camera([0.0, 0.0, -10.0], Projection::Orthographic { height: 4.0 });
        let hit = pick(&[cube()], &cam.ray_through(50.0, 50.0, 100, 100).unwrap()).unwrap();
        assert!(near(hit.normal, [0.0, 0.0, -1.0]), "{hit:?}");
        assert!(near(hit.point, [0.0, 0.0, -1.0]));
        assert_eq!(
            pick(&[cube()], &cam.ray_through(1.0, 1.0, 100, 100).unwrap()),
            None
        );
    }

    #[test]
    fn behind_the_perspective_eye_is_not_hit() {
        let cam = camera([0.0, 0.0, 0.0], Projection::Perspective { fov_y: 30.0 });
        let cam = Camera {
            target: [0.0, 0.0, -5.0],
            ..cam
        };
        let hit = pick(&[cube()], &cam.ray_through(50.0, 50.0, 100, 100).unwrap()).unwrap();
        assert!(
            near(hit.point, [0.0, 0.0, -1.0]),
            "the face in front, not behind"
        );
        assert!(
            near(hit.normal, [0.0, 0.0, -1.0]),
            "a back face keeps its own normal"
        );
    }

    #[test]
    fn a_degenerate_image_is_refused() {
        let cam = camera([0.0, 0.0, 10.0], Projection::Perspective { fov_y: 30.0 });
        assert!(matches!(
            cam.ray_through(0.0, 0.0, 0, 10),
            Err(RenderError::Size { .. })
        ));
    }

    #[test]
    fn a_degenerate_position_is_refused() {
        let ortho = camera([0.0, 0.0, 10.0], Projection::Orthographic { height: 4.0 });
        for (x, y) in [(f64::NAN, 1.0), (1.0, f64::INFINITY)] {
            assert!(matches!(
                ortho.ray_through(x, y, 10, 10),
                Err(RenderError::Camera(_))
            ));
        }
        let far = camera(
            [f64::MAX, 0.0, f64::MAX],
            Projection::Orthographic { height: f64::MAX },
        );
        assert!(matches!(
            far.ray_through(9.5, 5.0, 10, 10),
            Err(RenderError::Camera(_))
        ));
    }

    /// `pixel_of` is the renderer's own placement: a picked point goes back
    /// to its pixel under both planar projections and a non-square image.
    #[test]
    fn pixel_of_inverts_the_pick_under_a_planar_projection() {
        let eye = [6.0, 5.0, 8.0];
        for projection in [
            Projection::Perspective { fov_y: 40.0 },
            Projection::Orthographic { height: 4.0 },
        ] {
            let cam = camera(eye, projection);
            let (w, h) = (160, 90);
            for (x, y) in [(80.5, 45.5), (75.5, 40.5), (88.5, 50.5)] {
                let ray = cam.ray_through(x, y, w, h).unwrap();
                let hit = pick(&[cube()], &ray).expect("hits");
                let at = cam.pixel_of(hit.point, w, h).expect("drawn");
                assert!(
                    (at[0] - x).abs() < 1e-6 && (at[1] - y).abs() < 1e-6,
                    "{at:?}"
                );
            }
        }
    }

    #[test]
    fn pixel_of_is_none_only_where_nothing_is_drawn() {
        let flat = camera([0.0, 0.0, 10.0], Projection::Perspective { fov_y: 30.0 });
        assert_eq!(flat.pixel_of([0.0; 3], 100, 100), Some([50.0, 50.0]));
        // Outside the picture is still a position.
        let [x, _] = flat.pixel_of([100.0, 0.0, 0.0], 100, 100).unwrap();
        assert!(x > 100.0, "{x}");
        for (point, w, h) in [
            ([0.0, 0.0, 20.0], 100, 100),
            ([0.0, 0.0, 10.0], 100, 100),
            ([f64::NAN, 0.0, 0.0], 100, 100),
            ([f64::INFINITY, 0.0, 0.0], 100, 100),
            ([0.0; 3], 0, 100),
            ([0.0; 3], 100, 0),
        ] {
            assert_eq!(flat.pixel_of(point, w, h), None, "{point:?} {w}x{h}");
        }
        let View::Perspective { near, .. } = View::new(&flat, 1.0).unwrap() else {
            panic!("a perspective view");
        };
        assert_eq!(flat.pixel_of([0.0, 0.0, 10.0 - near / 2.0], 100, 100), None);
        assert!(
            flat.pixel_of([0.0, 0.0, 10.0 - near * 2.0], 100, 100)
                .is_some()
        );
        let mut refused = flat;
        refused.target = refused.eye;
        assert_eq!(refused.pixel_of([0.0; 3], 100, 100), None);

        // Orthographic draws behind the eye too.
        let ortho = camera([0.0, 0.0, 10.0], Projection::Orthographic { height: 4.0 });
        assert_eq!(
            ortho.pixel_of([0.0, 0.0, 20.0], 100, 100),
            Some([50.0, 50.0])
        );
    }
}
