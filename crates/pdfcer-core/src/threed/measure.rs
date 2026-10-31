//! 3D measurements on a saved view (ISO 32000-2 §13.6.7.3, Tables 326-331;
//! carried by a 3D view's `/MA`, Table 315).
//!
//! The value is stored, not measured: a reader shows `/V` as written. A
//! measurement promoted to a comment links a Projection annotation through
//! `/S` (§13.6.7.4); pdfcer reads that link's presence but never writes it.

use crate::graph::ObjectGraph;
use crate::object::{Dict, Name, Object};
use crate::textstring::{decode_text_string, encode_text_string};

use super::view_write::cross;

/// Most `/MA` entries read or written per view (a pdfcer guard; the spec
/// sets none).
pub const MAX_3D_MEASUREMENTS: usize = 4096;

/// `/Subtype` of a 3D measurement (Table 326).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ThreeDMeasureKind {
    /// `/LD3`: distance between two points (Table 327).
    Linear,
    /// `/PD3`: perpendicular distance (Table 328).
    Perpendicular,
    /// `/AD3`: angle (Table 329).
    Angular,
    /// `/RD3`: radius or diameter (Table 330).
    Radial,
    /// `/3DC`: a comment note on a point (Table 331).
    Comment,
}

impl ThreeDMeasureKind {
    fn from_name(n: &[u8]) -> Option<Self> {
        Some(match n {
            b"LD3" => Self::Linear,
            b"PD3" => Self::Perpendicular,
            b"AD3" => Self::Angular,
            b"RD3" => Self::Radial,
            b"3DC" => Self::Comment,
            _ => return None,
        })
    }

    fn name(self) -> &'static [u8] {
        match self {
            Self::Linear => b"LD3",
            Self::Perpendicular => b"PD3",
            Self::Angular => b"AD3",
            Self::Radial => b"RD3",
            Self::Comment => b"3DC",
        }
    }
}

/// One 3D measurement dictionary (Tables 326-331). Every key is kept as
/// written, `None` when absent; which are required depends on
/// [`Self::kind`], and [`crate::edit::EditSession::set_3d_views`] refuses
/// a measurement missing one. Points and directions are in world space.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct ThreeDMeasurement {
    /// `/Subtype`.
    pub kind: ThreeDMeasureKind,
    /// `/TRL`: the name a reader lists, e.g. "Measurement 1".
    pub name: Option<String>,
    /// `/AP`: the annotation plane the markup is drawn in (its normal).
    pub plane: Option<[f64; 3]>,
    /// `/A1`: first anchor; a radial measurement's circle centre.
    pub a1: Option<[f64; 3]>,
    /// `/A2`: second anchor; a radial measurement's point on the arc.
    pub a2: Option<[f64; 3]>,
    /// `/A3`: a radial arc's start.
    pub a3: Option<[f64; 3]>,
    /// `/A4`: a radial arc's end.
    pub a4: Option<[f64; 3]>,
    /// `/N1`: the node `a1` lies on; a reader hides the measurement when
    /// that node is missing or hidden.
    pub node1: Option<String>,
    /// `/N2`: the node `a2` lies on.
    pub node2: Option<String>,
    /// `/D1`: leader direction at `a1` (both leaders for perpendicular).
    pub d1: Option<[f64; 3]>,
    /// `/D2`: leader direction at `a2` (angular).
    pub d2: Option<[f64; 3]>,
    /// `/TP`: where the text's lower-left corner sits.
    pub text_anchor: Option<[f64; 3]>,
    /// `/TX`: the text's x direction (angular, radial).
    pub text_x: Option<[f64; 3]>,
    /// `/TY`: the text's up direction.
    pub text_up: Option<[f64; 3]>,
    /// `/TS`: text height in points, whatever the zoom (default 12).
    pub text_size: Option<f64>,
    /// `/C`: DeviceRGB markup colour (default white, `[1 1 1]`).
    pub color: Option<[f64; 3]>,
    /// `/V`: the measured value, as stored.
    pub value: Option<f64>,
    /// `/U`: the units label.
    pub units: Option<String>,
    /// `/P`: decimal places shown (default 3).
    pub places: Option<u32>,
    /// `/UT`: a suffix after the value; a comment note's text.
    pub user_text: Option<String>,
    /// `/DR`: an angle is in degrees (default `true`) rather than radians.
    pub degrees: Option<bool>,
    /// `/EL`: a radial extension line's length in points (default 60).
    pub extension: Option<f64>,
    /// `/SC`: draw the underlying circle or arc (default `false`).
    pub show_circle: Option<bool>,
    /// `/R`: show the radius (default `true`) rather than the diameter.
    pub radius: Option<bool>,
    /// `/TB`: a comment note's text box, `[columns rows]`.
    pub text_box: Option<[u32; 2]>,
    /// `/S` names a Projection annotation (the measurement is also a
    /// comment). Read only: a written measurement has no `/S`.
    pub has_comment: bool,
}

impl ThreeDMeasurement {
    /// A measurement of `kind` with no keys set.
    #[must_use]
    pub fn new(kind: ThreeDMeasureKind) -> Self {
        Self {
            kind,
            name: None,
            plane: None,
            a1: None,
            a2: None,
            a3: None,
            a4: None,
            node1: None,
            node2: None,
            d1: None,
            d2: None,
            text_anchor: None,
            text_x: None,
            text_up: None,
            text_size: None,
            color: None,
            value: None,
            units: None,
            places: None,
            user_text: None,
            degrees: None,
            extension: None,
            show_circle: None,
            radius: None,
            text_box: None,
            has_comment: false,
        }
    }

    /// A linear measurement from `a1` to `a2` drawn in the plane with
    /// normal `plane`, labelled `units`. The value is the distance between
    /// the points in model units; the text sits at the midpoint, its up
    /// direction `plane × (a2 − a1)` as Table 327 draws it.
    ///
    /// `None` when the points coincide, the line is parallel to `plane`'s
    /// normal, or a number is not finite.
    ///
    /// ```
    /// use pdfcer_core::threed::ThreeDMeasurement;
    /// let m = ThreeDMeasurement::linear([0.0; 3], [3.0, 4.0, 0.0], [0.0, 0.0, 1.0], "mm")
    ///     .expect("a measurement");
    /// assert_eq!(m.value, Some(5.0));
    /// assert_eq!(m.value_text().as_deref(), Some("5.000 mm"));
    /// ```
    #[must_use]
    pub fn linear(
        a1: [f64; 3],
        a2: [f64; 3],
        plane: [f64; 3],
        units: impl Into<String>,
    ) -> Option<Self> {
        let d = [a2[0] - a1[0], a2[1] - a1[1], a2[2] - a1[2]];
        let len = norm(d);
        let up = cross(plane, d);
        let up_len = norm(up);
        if !(len.is_finite() && len > 0.0 && up_len.is_finite() && up_len > 1e-12 * len) {
            return None;
        }
        let mut m = Self::new(ThreeDMeasureKind::Linear);
        m.plane = Some(plane);
        m.a1 = Some(a1);
        m.a2 = Some(a2);
        let ([x0, y0, z0], [x1, y1, z1]) = (a1, a2);
        m.text_anchor = Some([(x0 + x1) / 2.0, (y0 + y1) / 2.0, (z0 + z1) / 2.0]);
        m.text_up = Some(up.map(|c| c / up_len));
        m.value = Some(len);
        m.units = Some(units.into());
        Some(m)
    }

    /// The text a reader shows, as pdfcer formats it: `/V` to `/P` places
    /// (default 3), then `/U` after a space, then `/UT`; a comment note's
    /// is its `/UT`. ISO 32000-2 leaves the formatting to the reader.
    /// `None` without a value (or, for a note, text).
    #[must_use]
    pub fn value_text(&self) -> Option<String> {
        if self.kind == ThreeDMeasureKind::Comment {
            return self.user_text.clone();
        }
        let v = self.value.filter(|v| v.is_finite())?;
        let places = self.places.unwrap_or(3).min(15) as usize;
        let mut s = format!("{v:.places$}");
        if let Some(u) = self.units.as_deref().filter(|u| !u.is_empty()) {
            s.push(' ');
            s.push_str(u);
        }
        if let Some(t) = &self.user_text {
            s.push_str(t);
        }
        Some(s)
    }
}

fn norm([x, y, z]: [f64; 3]) -> f64 {
    (x * x + y * y + z * z).sqrt()
}

/// The `/MA` measurements of view dictionary `view`. An entry that is not a
/// dictionary, or names no subtype Table 326 lists, is skipped.
pub(super) fn read_measurements<G: ObjectGraph + ?Sized>(
    graph: &G,
    view: &Dict,
) -> Vec<ThreeDMeasurement> {
    let Some(list) = view.get(b"MA").and_then(|o| graph.resolve(o).as_array()) else {
        return Vec::new();
    };
    list.iter()
        .take(MAX_3D_MEASUREMENTS)
        .filter_map(|o| graph.resolve(o).as_dict())
        .filter_map(|d| read_one(graph, d))
        .collect()
}

fn read_one<G: ObjectGraph + ?Sized>(graph: &G, d: &Dict) -> Option<ThreeDMeasurement> {
    let get = |k: &[u8]| d.get(k).map(|o| graph.resolve(o));
    let kind = get(b"Subtype")?
        .as_name()
        .and_then(|n| ThreeDMeasureKind::from_name(n.as_bytes()))?;
    let vec3 = |k: &[u8]| get(k).and_then(|o| vec3(graph, o));
    let text = |k: &[u8]| match get(k) {
        Some(Object::String(s)) => Some(decode_text_string(s).text),
        _ => None,
    };
    let number = |k: &[u8]| get(k).and_then(Object::as_number).filter(|v| v.is_finite());
    let flag = |k: &[u8]| match get(k) {
        Some(Object::Boolean(b)) => Some(*b),
        _ => None,
    };
    let mut m = ThreeDMeasurement::new(kind);
    m.name = text(b"TRL");
    m.plane = vec3(b"AP");
    [m.a1, m.a2, m.a3, m.a4] = [b"A1", b"A2", b"A3", b"A4"].map(|k| vec3(k.as_slice()));
    m.node1 = text(b"N1");
    m.node2 = text(b"N2");
    m.d1 = vec3(b"D1");
    m.d2 = vec3(b"D2");
    m.text_anchor = vec3(b"TP");
    m.text_x = vec3(b"TX");
    m.text_up = vec3(b"TY");
    m.text_size = number(b"TS");
    m.color = vec3(b"C");
    m.value = number(b"V");
    m.units = text(b"U");
    m.places = number(b"P")
        .filter(|p| *p >= 0.0 && *p <= f64::from(u32::MAX))
        .map(|p| p as u32);
    m.user_text = text(b"UT");
    m.degrees = flag(b"DR");
    m.extension = number(b"EL");
    m.show_circle = flag(b"SC");
    m.radius = flag(b"R");
    m.text_box = get(b"TB").and_then(|o| text_box(graph, o));
    m.has_comment = d.contains_key(b"S");
    Some(m)
}

fn vec3<G: ObjectGraph + ?Sized>(graph: &G, o: &Object) -> Option<[f64; 3]> {
    let a = o.as_array()?;
    let n: Vec<f64> = a
        .iter()
        .filter_map(|o| graph.resolve(o).as_number())
        .collect();
    let v = <[f64; 3]>::try_from(n).ok()?;
    (a.len() == 3 && v.iter().all(|c| c.is_finite())).then_some(v)
}

fn text_box<G: ObjectGraph + ?Sized>(graph: &G, o: &Object) -> Option<[u32; 2]> {
    let a = o.as_array()?;
    let [c, r] = a else {
        return None;
    };
    let int = |o: &Object| match graph.resolve(o) {
        Object::Integer(i) => u32::try_from(*i).ok(),
        _ => None,
    };
    Some([int(c)?, int(r)?])
}

/// The keys Tables 327-331 require for `kind`, beyond `/Subtype`.
fn required(kind: ThreeDMeasureKind) -> &'static [&'static str] {
    match kind {
        ThreeDMeasureKind::Linear => &["AP", "A1", "A2", "TP", "TY", "V", "U"],
        ThreeDMeasureKind::Perpendicular => &["AP", "A1", "A2", "D1", "TP", "TY", "V", "U"],
        ThreeDMeasureKind::Angular => &["AP", "A1", "A2", "D1", "D2", "TP", "TX", "TY", "V"],
        ThreeDMeasureKind::Radial => &["AP", "A1", "A2", "A3", "A4", "TP", "TX", "TY", "V", "U"],
        ThreeDMeasureKind::Comment => &["A1", "TP"],
    }
}

/// Why `m` cannot be written, if it cannot: a key its kind requires is
/// missing, or a number is not finite.
pub(super) fn check_measurement(m: &ThreeDMeasurement) -> Result<(), String> {
    let dict = measurement_dict(m);
    if let Some(k) = required(m.kind)
        .iter()
        .find(|k| !dict.contains_key(k.as_bytes()))
    {
        return Err(format!("it has no /{k}, which its kind requires"));
    }
    let finite = |o: &Object| match o {
        Object::Real(v) => v.is_finite(),
        Object::Array(a) => a
            .iter()
            .all(|o| !matches!(o, Object::Real(v) if !v.is_finite())),
        _ => true,
    };
    if !dict.iter().all(|(_, o)| finite(o)) {
        return Err("it has a number that is not finite".to_owned());
    }
    Ok(())
}

/// The Table 326-331 dictionary for `m`, without `/S`.
pub(super) fn measurement_dict(m: &ThreeDMeasurement) -> Dict {
    let mut d = Dict::new();
    let mut put = |k: &[u8], o: Object| {
        d.insert(Name::from(k), o);
    };
    put(b"Type", Object::Name(Name::from(b"3DMeasure")));
    put(b"Subtype", Object::Name(Name::from(m.kind.name())));
    let v3 = |v: [f64; 3]| Object::Array(v.iter().map(|c| Object::Real(*c)).collect());
    let s = |t: &str| Object::String(encode_text_string(t));
    let vectors = [
        (b"AP".as_slice(), m.plane),
        (b"A1", m.a1),
        (b"A2", m.a2),
        (b"A3", m.a3),
        (b"A4", m.a4),
        (b"D1", m.d1),
        (b"D2", m.d2),
        (b"TP", m.text_anchor),
        (b"TX", m.text_x),
        (b"TY", m.text_up),
        (b"C", m.color),
    ];
    for (k, v) in vectors {
        if let Some(v) = v {
            put(k, v3(v));
        }
    }
    let texts = [
        (b"TRL".as_slice(), &m.name),
        (b"N1", &m.node1),
        (b"N2", &m.node2),
        (b"U", &m.units),
        (b"UT", &m.user_text),
    ];
    for (k, t) in texts {
        if let Some(t) = t {
            put(k, s(t));
        }
    }
    for (k, v) in [
        (b"TS".as_slice(), m.text_size),
        (b"V", m.value),
        (b"EL", m.extension),
    ] {
        if let Some(v) = v {
            put(k, Object::Real(v));
        }
    }
    for (k, v) in [
        (b"DR".as_slice(), m.degrees),
        (b"SC", m.show_circle),
        (b"R", m.radius),
    ] {
        if let Some(v) = v {
            put(k, Object::Boolean(v));
        }
    }
    if let Some(p) = m.places {
        put(b"P", Object::Integer(i64::from(p)));
    }
    if let Some([c, r]) = m.text_box {
        put(
            b"TB",
            Object::Array(vec![Object::Integer(c.into()), Object::Integer(r.into())]),
        );
    }
    d
}

#[cfg(test)]
// A panic on a missing value is the failure these tests report.
#[allow(clippy::expect_used, clippy::indexing_slicing)]
mod tests;
