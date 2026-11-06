//! Builders for the 3D measurements other than linear (ISO 32000-2
//! §13.6.7.3.3–13.6.7.3.6, Tables 328–331). Each computes the stored `/V`,
//! which a reader shows without re-measuring, and returns only what
//! `set_3d_views` will write.

use super::{ThreeDMeasureKind, ThreeDMeasurement, check_measurement};

use pdfcer_vec3::{add, cross, dot, length, normalize, scale, sub};

/// Below this sine two directions count as parallel.
const PARALLEL: f64 = 1e-12;

/// The unit direction of `a × b`; `None` when either is zero or not finite,
/// or they are parallel.
fn across(a: [f64; 3], b: [f64; 3]) -> Option<[f64; 3]> {
    let c = cross(normalize(a)?, normalize(b)?);
    (length(c) > PARALLEL).then(|| normalize(c)).flatten()
}

fn midpoint([x0, y0, z0]: [f64; 3], [x1, y1, z1]: [f64; 3]) -> [f64; 3] {
    [(x0 + x1) / 2.0, (y0 + y1) / 2.0, (z0 + z1) / 2.0]
}

/// `m` when it can be written.
fn writable(m: ThreeDMeasurement) -> Option<ThreeDMeasurement> {
    check_measurement(&m).ok().map(|()| m)
}

impl ThreeDMeasurement {
    /// A perpendicular distance (`/PD3`, Table 328): how far `a2` is from
    /// `a1` along `normal`, such as a picked face's normal. `/V` is
    /// `|(a2 − a1) · normal|` with `normal` made unit length.
    ///
    /// The measure line runs along `normal` between two parallel leaders
    /// from the anchors, and all of it lies in the annotation plane, so a
    /// plane holding `normal` is what makes the drawn line as long as `/V`.
    /// `plane` is a preference, such as the direction facing the camera:
    /// `/AP` is the plane holding `normal` that is nearest to it, `/D1` is
    /// `/AP × normal`, the text sits at the midpoint and reads from `a1` to
    /// `a2`.
    ///
    /// `None` when the points have no separation along `normal`, `plane` is
    /// parallel to `normal`, either is zero, or a number is not finite.
    ///
    /// ```
    /// use pdfcer_core::threed::ThreeDMeasurement;
    /// let m = ThreeDMeasurement::perpendicular(
    ///     [0.0; 3],
    ///     [3.0, 4.0, 0.0],
    ///     [0.0, 2.0, 0.0],
    ///     [0.0, 0.0, 1.0],
    ///     "mm",
    /// )
    /// .expect("a measurement");
    /// assert_eq!(m.value, Some(4.0));
    /// assert_eq!(m.d1, Some([-1.0, 0.0, 0.0]));
    /// ```
    #[must_use]
    pub fn perpendicular(
        a1: [f64; 3],
        a2: [f64; 3],
        normal: [f64; 3],
        plane: [f64; 3],
        units: impl Into<String>,
    ) -> Option<Self> {
        let along = normalize(normal)?;
        let leader = across(plane, along)?;
        let facing = across(along, leader)?;
        let gap = sub(a2, a1);
        let signed = dot(gap, along);
        if signed.is_nan() || signed.abs() <= PARALLEL * length(gap) {
            return None;
        }
        let text_x = scale(along, signed.signum());
        let mut m = Self::new(ThreeDMeasureKind::Perpendicular);
        m.plane = Some(facing);
        m.a1 = Some(a1);
        m.a2 = Some(a2);
        m.d1 = Some(leader);
        m.text_anchor = Some(midpoint(a1, a2));
        m.text_up = Some(cross(facing, text_x));
        m.value = Some(signed.abs());
        m.units = Some(units.into());
        writable(m)
    }

    /// An angle (`/AD3`, Table 329): the angle at `vertex` between the lines
    /// to `a1` and `a2`, in degrees (`/DR` true). Between 0° and 180°, both
    /// excluded: the spec gives parallel leaders no markup.
    ///
    /// `/A1` and `/A2` are the anchors. `/D1` and `/D2` are the unit
    /// directions from `vertex` to each, which is pdfcer's reading: the
    /// table says only "direction vector for the leader line". The arc lies
    /// in the plane of the three points, so that is `/AP`, and `plane` only
    /// chooses which way it faces, `/AP · plane` not negative. The text sits
    /// inside the angle on its bisector, at the nearer anchor's distance
    /// from `vertex`, which sets the arc's radius; `/TX` runs across the
    /// bisector.
    ///
    /// `None` when a point coincides with `vertex`, the lines are parallel,
    /// `plane` is zero, or a number is not finite.
    ///
    /// ```
    /// use pdfcer_core::threed::ThreeDMeasurement;
    /// let m = ThreeDMeasurement::angular(
    ///     [0.0; 3],
    ///     [2.0, 0.0, 0.0],
    ///     [0.0, 3.0, 0.0],
    ///     [0.0, 0.0, -1.0],
    /// )
    /// .expect("a measurement");
    /// assert_eq!(m.value_text().as_deref(), Some("90.000°"));
    /// assert_eq!(m.plane, Some([0.0, 0.0, -1.0]));
    /// ```
    #[must_use]
    pub fn angular(vertex: [f64; 3], a1: [f64; 3], a2: [f64; 3], plane: [f64; 3]) -> Option<Self> {
        let (to1, to2) = (sub(a1, vertex), sub(a2, vertex));
        let (d1, d2) = (normalize(to1)?, normalize(to2)?);
        let turned = across(d1, d2)?;
        let facing = scale(turned, dot(turned, normalize(plane)?).signum());
        let bisector = normalize(add(d1, d2))?;
        let text_x = normalize(sub(d2, d1))?;
        let reach = length(to1).min(length(to2));
        let mut m = Self::new(ThreeDMeasureKind::Angular);
        m.plane = Some(facing);
        m.a1 = Some(a1);
        m.a2 = Some(a2);
        m.d1 = Some(d1);
        m.d2 = Some(d2);
        m.text_anchor = Some(add(vertex, scale(bisector, reach)));
        m.text_x = Some(text_x);
        m.text_up = Some(cross(facing, text_x));
        m.value = Some(length(cross(d1, d2)).atan2(dot(d1, d2)).to_degrees());
        m.degrees = Some(true);
        writable(m)
    }

    /// A radius (`/RD3`, Table 330): the circle through three points, taken
    /// as an arc from `p1` through `p2` to `p3`. `/A1` is its centre, `/A2`
    /// is `p2`, `/A3` and `/A4` are `p1` and `p3`, `/AP` is its unit normal
    /// by the right-hand rule over that order, and `/V` is its radius.
    ///
    /// The arrow points at `p2` from outside the circle: the text anchor is
    /// half a radius beyond it, and `/TX` runs outward from the centre, so
    /// the extension line continues away from the circle. Which way a
    /// circle's normal faces decides whether the text reads from the near
    /// side; a caller that knows the viewer can reverse the three points.
    ///
    /// `None` when two points coincide, the three are collinear, or a number
    /// is not finite.
    ///
    /// ```
    /// use pdfcer_core::threed::ThreeDMeasurement;
    /// let m = ThreeDMeasurement::radial(
    ///     [2.0, 0.0, 0.0],
    ///     [0.0, 2.0, 0.0],
    ///     [-2.0, 0.0, 0.0],
    ///     "mm",
    /// )
    /// .expect("a measurement");
    /// assert_eq!(m.a1, Some([0.0; 3]));
    /// assert_eq!(m.value_text().as_deref(), Some("R2.000 mm"));
    /// ```
    #[must_use]
    pub fn radial(
        p1: [f64; 3],
        p2: [f64; 3],
        p3: [f64; 3],
        units: impl Into<String>,
    ) -> Option<Self> {
        let (b, c) = (sub(p2, p1), sub(p3, p1));
        let facing = across(b, c)?;
        let w = cross(b, c);
        let from_p1 = add(scale(cross(w, b), dot(c, c)), scale(cross(c, w), dot(b, b)));
        let centre = add(p1, scale(from_p1, 0.5 / dot(w, w)));
        let out = sub(p2, centre);
        let radius = length(out);
        let text_x = normalize(out)?;
        let mut m = Self::new(ThreeDMeasureKind::Radial);
        m.plane = Some(facing);
        m.a1 = Some(centre);
        m.a2 = Some(p2);
        m.a3 = Some(p1);
        m.a4 = Some(p3);
        m.text_anchor = Some(add(centre, scale(text_x, 1.5 * radius)));
        m.text_x = Some(text_x);
        m.text_up = Some(cross(facing, text_x));
        m.value = Some(radius);
        m.units = Some(units.into());
        writable(m)
    }

    /// A note on a point (`/3DC`, Table 331): `text` at `at`, which is both
    /// `/A1` and the corner of the text box nearest it (`/TP`). A caller
    /// wanting a leader moves [`Self::text_anchor`].
    ///
    /// `None` when `text` is empty or a coordinate is not finite.
    ///
    /// ```
    /// use pdfcer_core::threed::ThreeDMeasurement;
    /// let m = ThreeDMeasurement::comment([1.0, 2.0, 3.0], "check this").expect("a note");
    /// assert_eq!(m.value_text().as_deref(), Some("check this"));
    /// ```
    #[must_use]
    pub fn comment(at: [f64; 3], text: impl Into<String>) -> Option<Self> {
        let text = text.into();
        if text.is_empty() {
            return None;
        }
        let mut m = Self::new(ThreeDMeasureKind::Comment);
        m.a1 = Some(at);
        m.text_anchor = Some(at);
        m.user_text = Some(text);
        writable(m)
    }
}
