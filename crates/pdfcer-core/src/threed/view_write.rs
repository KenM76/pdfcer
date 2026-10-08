//! Building and writing 3D view dictionaries (ISO 32000-1 §13.6.4 Table
//! 304, Table 305 projection), the write side of [`super::view`].

use crate::object::{Dict, Name, ObjId, Object};

use super::ThreeDEmbedError;
use super::view::{OrthoBinding, ThreeDSavedView};

/// Most views [`crate::edit::EditSession::set_3d_views`] writes.
pub const MAX_3D_VIEWS: usize = 4096;

/// What [`crate::edit::EditSession::set_3d_views`] wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ThreeDViewsOutcome {
    /// The `/3D` annotation, whose `/3DV` now names [`Self::default`].
    pub annot_id: ObjId,
    /// The 3D stream whose `/VA` and `/DV` were rewritten.
    pub stream_id: ObjId,
    /// How many views `/VA` held before.
    pub views_before: usize,
    /// How many it holds now.
    pub views_after: usize,
    /// The view a reader opens on; `None` leaves the choice to the stream
    /// or the artwork.
    pub default: Option<usize>,
    /// The stream was reached through a `/3DRef` that other annotations may
    /// name, so they get these views too.
    pub shared_stream: bool,
    /// Off-canvas statements for the shell to show.
    pub disclosures: Vec<String>,
}

impl ThreeDSavedView {
    /// A view named `name` (Table 304 `/XN`, the label a reader lists)
    /// with camera-to-world matrix `camera_to_world` (`/C2W`, laid out as
    /// [`Self::camera_to_world`]), in perspective at the reader's default
    /// 90° until [`Self::with_perspective`] or [`Self::with_orthographic`].
    ///
    /// ```
    /// use pdfcer_core::threed::ThreeDSavedView;
    /// let v = ThreeDSavedView::new("Back", [1., 0., 0., 0., 0., 1., 0., -1., 0., 0., 5., 0.])
    ///     .with_orbit_distance(5.0);
    /// assert_eq!(v.aim(1.0).map(|a| a.direction), Some([0., -1., 0.]));
    /// ```
    #[must_use]
    pub fn new(name: impl Into<String>, camera_to_world: [f64; 12]) -> Self {
        Self {
            name: name.into(),
            camera_to_world: Some(camera_to_world),
            ..Self::default()
        }
    }

    /// `/CO`: the distance from the camera to the centre of orbit along the
    /// look direction.
    #[must_use]
    pub fn with_orbit_distance(mut self, distance: f64) -> Self {
        self.orbit_distance = Some(distance);
        self
    }

    /// Perspective with field of view `degrees` (Table 305 `/FOV`): the
    /// full angle of the cone whose circle spans the annotation's width
    /// (`/PS` default `/W`), so a horizontal field of view.
    #[must_use]
    pub fn with_perspective(mut self, degrees: f64) -> Self {
        self.orthographic = false;
        self.field_of_view = Some(degrees);
        self
    }

    /// Orthographic, with Table 305 `/OS` `scale` and `/OB` `binding`.
    /// pdfcer reads a binding as fitting the bound side to `1/scale` camera
    /// units ([`Self::aim`]); the standard gives the scale no unit.
    #[must_use]
    pub fn with_orthographic(mut self, scale: f64, binding: OrthoBinding) -> Self {
        self.orthographic = true;
        self.ortho_scale = scale;
        self.ortho_binding = binding;
        self.field_of_view = None;
        self
    }
}

#[cfg(feature = "3d")]
impl ThreeDSavedView {
    /// The view `camera` shows in an annotation of `aspect` (width /
    /// height), named `name`: its matrix, `/CO` the eye-to-target distance,
    /// and its projection. A perspective camera's vertical field of view
    /// becomes the horizontal one `/FOV` wants; an orthographic camera's
    /// visible height `h` becomes `/OB /H` with `/OS` `1/h`, which pdfcer
    /// reads back as the same height.
    ///
    /// The matrix's z column is the unit look direction, its y column the
    /// unit part of `camera.up` across it, its x column y × z (right-handed,
    /// as CAD exports write), its translation the eye.
    ///
    /// `None` when the eye is on the target, `up` is parallel to the look
    /// direction, `aspect` is not positive, or a number is not finite.
    ///
    /// ```
    /// use pdfcer_3d::{Bounds, Camera, NamedView, UpAxis};
    /// use pdfcer_core::threed::ThreeDSavedView;
    /// let bounds = Bounds { min: [-1.0; 3], max: [1.0; 3] };
    /// let (dir, up) = NamedView::Front.direction(UpAxis::Z);
    /// let camera = Camera::fit(&bounds, dir, up, true, 1.0)?;
    /// let view = ThreeDSavedView::from_camera("Front", &camera, 1.0).expect("a view");
    /// let aim = view.aim(1.0).expect("a matrix");
    /// assert!((aim.direction[1] - 1.0).abs() < 1e-12 && (aim.up[2] - 1.0).abs() < 1e-12);
    /// # Ok::<(), pdfcer_3d::RenderError>(())
    /// ```
    #[must_use]
    pub fn from_camera(
        name: impl Into<String>,
        camera: &pdfcer_3d::Camera,
        aspect: f64,
    ) -> Option<Self> {
        if !(aspect.is_finite() && aspect > 0.0) {
            return None;
        }
        let d = sub(camera.target, camera.eye);
        let distance = dot(d, d).sqrt();
        let z = unit(d)?;
        let y = unit(sub(camera.up, scale(z, dot(camera.up, z))))?;
        let x = cross(y, z);
        let [ex, ey, ez] = camera.eye;
        let [x0, x1, x2] = x;
        let [y0, y1, y2] = y;
        let [z0, z1, z2] = z;
        let m = [x0, x1, x2, y0, y1, y2, z0, z1, z2, ex, ey, ez];
        if !m.iter().all(|v| v.is_finite()) {
            return None;
        }
        let view = Self::new(name, m).with_orbit_distance(distance);
        match camera.projection {
            pdfcer_3d::Projection::Perspective { fov_y } => {
                let half = (fov_y.to_radians() / 2.0).tan() * aspect;
                let fov = 2.0 * half.atan().to_degrees();
                (fov.is_finite() && fov > 0.0 && fov <= 180.0).then(|| view.with_perspective(fov))
            }
            pdfcer_3d::Projection::Orthographic { height } => (height.is_finite() && height > 0.0)
                .then(|| view.with_orthographic(1.0 / height, OrthoBinding::Height)),
        }
    }
}

#[cfg(feature = "3d")]
fn sub([a0, a1, a2]: [f64; 3], [b0, b1, b2]: [f64; 3]) -> [f64; 3] {
    [a0 - b0, a1 - b1, a2 - b2]
}

#[cfg(feature = "3d")]
fn scale(a: [f64; 3], s: f64) -> [f64; 3] {
    a.map(|c| c * s)
}

#[cfg(feature = "3d")]
fn dot([a0, a1, a2]: [f64; 3], [b0, b1, b2]: [f64; 3]) -> f64 {
    a0 * b0 + a1 * b1 + a2 * b2
}

#[cfg(feature = "3d")]
fn cross([a0, a1, a2]: [f64; 3], [b0, b1, b2]: [f64; 3]) -> [f64; 3] {
    [a1 * b2 - a2 * b1, a2 * b0 - a0 * b2, a0 * b1 - a1 * b0]
}

#[cfg(feature = "3d")]
fn unit(a: [f64; 3]) -> Option<[f64; 3]> {
    let len = dot(a, a).sqrt();
    (len.is_finite() && len > 1e-12).then(|| scale(a, 1.0 / len))
}

/// Why view `index` cannot be written, if it cannot.
pub(crate) fn check_view(index: usize, v: &ThreeDSavedView) -> Result<(), ThreeDEmbedError> {
    let invalid = |why: &str| ThreeDEmbedError::ViewInvalid {
        index,
        why: why.to_owned(),
    };
    if v.name.trim().is_empty() {
        return Err(ThreeDEmbedError::ViewNameEmpty { index });
    }
    if let Some(m) = v.camera_to_world {
        if !m.iter().all(|c| c.is_finite()) {
            return Err(invalid("its camera matrix has a number that is not finite"));
        }
        let column_zero = |k: usize| m.iter().skip(k * 3).take(3).all(|c| *c == 0.0);
        if column_zero(1) || column_zero(2) {
            return Err(invalid("its camera matrix has no up or look direction"));
        }
    }
    if v.orbit_distance
        .is_some_and(|d| !(d.is_finite() && d >= 0.0))
    {
        return Err(invalid("its orbit distance is negative or not finite"));
    }
    if v.orthographic && !(v.ortho_scale.is_finite() && v.ortho_scale > 0.0) {
        return Err(invalid("its orthographic scale is not a positive number"));
    }
    if !v.orthographic
        && v.field_of_view
            .is_some_and(|f| !(f.is_finite() && f > 0.0 && f <= 180.0))
    {
        return Err(invalid("its field of view is outside 0-180 degrees"));
    }
    Ok(())
}

/// The Table 304 dictionary for `v`. `/IN` is not written, so it defaults
/// to `/XN`; [`ThreeDSavedView::view_box`] belongs to the annotation and is
/// not written. A perspective view with no field of view writes no `/P`,
/// which a reader takes as perspective at 90°.
pub(crate) fn view_dict(v: &ThreeDSavedView) -> Dict {
    let mut d = Dict::new();
    d.insert(Name::from(b"Type"), Object::Name(Name::from(b"3DView")));
    d.insert(
        Name::from(b"XN"),
        Object::String(crate::textstring::encode_text_string(&v.name)),
    );
    if let Some(m) = v.camera_to_world {
        d.insert(Name::from(b"MS"), Object::Name(Name::from(b"M")));
        d.insert(
            Name::from(b"C2W"),
            Object::Array(m.iter().map(|c| Object::Real(*c)).collect()),
        );
        if let Some(co) = v.orbit_distance {
            d.insert(Name::from(b"CO"), Object::Real(co));
        }
    }
    if let Some(p) = projection_dict(v) {
        d.insert(Name::from(b"P"), Object::Dict(p));
    }
    d
}

fn projection_dict(v: &ThreeDSavedView) -> Option<Dict> {
    let mut p = Dict::new();
    if v.orthographic {
        p.insert(Name::from(b"Subtype"), Object::Name(Name::from(b"O")));
        if v.ortho_scale != 1.0 {
            p.insert(Name::from(b"OS"), Object::Real(v.ortho_scale));
        }
        let ob: &[u8] = match v.ortho_binding {
            OrthoBinding::Width => b"W",
            OrthoBinding::Height => b"H",
            OrthoBinding::Min => b"Min",
            OrthoBinding::Max => b"Max",
            OrthoBinding::Absolute => return Some(p),
        };
        p.insert(Name::from(b"OB"), Object::Name(Name::from(ob)));
        return Some(p);
    }
    let fov = v.field_of_view?;
    p.insert(Name::from(b"Subtype"), Object::Name(Name::from(b"P")));
    p.insert(Name::from(b"FOV"), Object::Real(fov));
    Some(p)
}
