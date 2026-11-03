//! `/RM`, a 3D view's render mode (ISO 32000-1 §13.6.4.4 Tables 307,
//! 308; PDF 1.7).

use crate::graph::ObjectGraph;
use crate::object::{Dict, Name, Object};

use super::name_of;

/// A render mode's `/Subtype` (ISO 32000-1 Table 308), in table order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ThreeDRenderModeKind {
    /// `/Solid`: lit, textured faces.
    Solid,
    /// `/SolidWireframe`: faces with edges in the auxiliary colour.
    SolidWireframe,
    /// `/Transparent`: faces at the `/O` opacity.
    Transparent,
    /// `/TransparentWireframe`: translucent faces with opaque edges.
    TransparentWireframe,
    /// `/BoundingBox`: each node's bounding-box edges.
    BoundingBox,
    /// `/TransparentBoundingBox`: each node's bounding-box faces,
    /// translucent, in the face colour.
    TransparentBoundingBox,
    /// `/TransparentBoundingBoxOutline`: the faces and their edges.
    TransparentBoundingBoxOutline,
    /// `/Wireframe`: edges only.
    Wireframe,
    /// `/ShadedWireframe`: edges only, in their vertices' colours, lit.
    ShadedWireframe,
    /// `/HiddenWireframe`: edges with back-facing and obscured ones removed.
    HiddenWireframe,
    /// `/Vertices`: vertices only.
    Vertices,
    /// `/ShadedVertices`: vertices only, in their colours, lit.
    ShadedVertices,
    /// `/Illustration`: silhouette edges over faces in the face colour.
    Illustration,
    /// `/SolidOutline`: silhouette edges over lit, textured faces.
    SolidOutline,
    /// `/ShadedIllustration`: [`Self::SolidOutline`] with an emissive term.
    ShadedIllustration,
}

const NAMES: [(ThreeDRenderModeKind, &[u8]); 15] = [
    (ThreeDRenderModeKind::Solid, b"Solid"),
    (ThreeDRenderModeKind::SolidWireframe, b"SolidWireframe"),
    (ThreeDRenderModeKind::Transparent, b"Transparent"),
    (
        ThreeDRenderModeKind::TransparentWireframe,
        b"TransparentWireframe",
    ),
    (ThreeDRenderModeKind::BoundingBox, b"BoundingBox"),
    (
        ThreeDRenderModeKind::TransparentBoundingBox,
        b"TransparentBoundingBox",
    ),
    (
        ThreeDRenderModeKind::TransparentBoundingBoxOutline,
        b"TransparentBoundingBoxOutline",
    ),
    (ThreeDRenderModeKind::Wireframe, b"Wireframe"),
    (ThreeDRenderModeKind::ShadedWireframe, b"ShadedWireframe"),
    (ThreeDRenderModeKind::HiddenWireframe, b"HiddenWireframe"),
    (ThreeDRenderModeKind::Vertices, b"Vertices"),
    (ThreeDRenderModeKind::ShadedVertices, b"ShadedVertices"),
    (ThreeDRenderModeKind::Illustration, b"Illustration"),
    (ThreeDRenderModeKind::SolidOutline, b"SolidOutline"),
    (
        ThreeDRenderModeKind::ShadedIllustration,
        b"ShadedIllustration",
    ),
];

impl ThreeDRenderModeKind {
    /// The `/Subtype` name, as written.
    ///
    /// ```
    /// use pdfcer_core::threed::ThreeDRenderModeKind;
    /// assert_eq!(ThreeDRenderModeKind::Vertices.as_name(), b"Vertices");
    /// // Table 307's NOTE says "Points"; no such mode exists.
    /// assert_eq!(ThreeDRenderModeKind::from_name(b"Points"), None);
    /// ```
    #[must_use]
    pub fn as_name(self) -> &'static [u8] {
        NAMES
            .iter()
            .find(|(k, _)| *k == self)
            .map_or(b"Solid", |(_, n)| n)
    }

    /// The mode a `/Subtype` names, or `None` for a name Table 308 does not
    /// list (the dictionary is then ignored).
    #[must_use]
    pub fn from_name(name: &[u8]) -> Option<Self> {
        NAMES.iter().find(|(_, n)| *n == name).map(|(k, _)| *k)
    }
}

/// A 3D view's render mode dictionary (ISO 32000-1 Table 307).
///
/// A malformed entry reads as its default: `/AC` and `/FC` arrays must be
/// `[/DeviceRGB r g b]` with each component a number in 0–1, `/O` and
/// `/CV` finite numbers.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct ThreeDRenderMode {
    /// `/Subtype`.
    pub kind: ThreeDRenderModeKind,
    /// `/AC`, the auxiliary colour for edges and points, DeviceRGB 0–1.
    /// Default black.
    pub aux_colour: [f64; 3],
    /// `/FC`, the face colour, DeviceRGB 0–1; `None` for `/BG` (the
    /// default, and any other name): the background colour.
    pub face_colour: Option<[f64; 3]>,
    /// `/O`, the opacity of the added transparency. Default 0.5.
    pub opacity: f64,
    /// `/CV`, the crease value in degrees for silhouette edges. Default 45.
    pub crease_angle: f64,
}

impl ThreeDRenderMode {
    /// `kind` with every other entry at its Table 307 default.
    #[must_use]
    pub fn new(kind: ThreeDRenderModeKind) -> Self {
        Self {
            kind,
            aux_colour: [0.0; 3],
            face_colour: None,
            opacity: 0.5,
            crease_angle: 45.0,
        }
    }

    /// The Table 307 dictionary. Each entry at its default, and a
    /// non-finite `/O` or `/CV`, is left out; colour components are
    /// clamped to 0–1.
    pub(crate) fn to_dict(self) -> Dict {
        let mut d = Dict::new();
        let name = |n: &[u8]| Object::Name(Name::from(n));
        d.insert(Name::from(b"Type"), name(b"3DRenderMode"));
        d.insert(Name::from(b"Subtype"), name(self.kind.as_name()));
        let rgb = |c: [f64; 3]| {
            let mut a = vec![name(b"DeviceRGB")];
            a.extend(c.map(|v| Object::Real(v.clamp(0.0, 1.0))));
            Object::Array(a)
        };
        if self.aux_colour != [0.0; 3] {
            d.insert(Name::from(b"AC"), rgb(self.aux_colour));
        }
        if let Some(fc) = self.face_colour {
            d.insert(Name::from(b"FC"), rgb(fc));
        }
        for (key, value, default) in [
            (&b"O"[..], self.opacity, 0.5),
            (&b"CV"[..], self.crease_angle, 45.0),
        ] {
            if value.is_finite() && value != default {
                d.insert(Name::from(key), Object::Real(value));
            }
        }
        d
    }
}

/// The renderer's mode for this dictionary, for drawing a view as it says:
/// `pdfcer_3d::RenderOptions::render_mode`. Colours are rounded to 8 bits;
/// the choices Table 308 leaves open keep the renderer's defaults.
///
/// ```
/// use pdfcer_core::threed::{ThreeDRenderMode, ThreeDRenderModeKind};
/// let mode = pdfcer_3d::RenderMode::from(ThreeDRenderMode::new(ThreeDRenderModeKind::Illustration));
/// assert_eq!(mode.kind, pdfcer_3d::RenderModeKind::Illustration);
/// assert_eq!(mode.face_colour, pdfcer_3d::FaceColour::Background);
/// ```
#[cfg(feature = "3d")]
impl From<ThreeDRenderMode> for pdfcer_3d::RenderMode {
    fn from(mode: ThreeDRenderMode) -> Self {
        let bytes = |c: [f64; 3]| c.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
        let name = std::str::from_utf8(mode.kind.as_name()).unwrap_or("Solid");
        pdfcer_3d::RenderMode {
            kind: pdfcer_3d::RenderModeKind::from_name(name).unwrap_or_default(),
            aux_colour: bytes(mode.aux_colour),
            face_colour: mode
                .face_colour
                .map_or(pdfcer_3d::FaceColour::Background, |c| {
                    pdfcer_3d::FaceColour::Rgb(bytes(c))
                }),
            opacity: mode.opacity,
            crease_angle: mode.crease_angle,
            ..pdfcer_3d::RenderMode::default()
        }
    }
}

/// `view`'s `/RM`, or `None` when absent, not a dictionary, or naming a
/// `/Subtype` Table 308 does not list (the whole dictionary is ignored).
pub(super) fn read_render_mode<G: ObjectGraph + ?Sized>(
    graph: &G,
    view: &Dict,
) -> Option<ThreeDRenderMode> {
    let rm = graph.resolve(view.get(b"RM")?).as_dict()?;
    let kind = ThreeDRenderModeKind::from_name(name_of(graph, rm, b"Subtype")?)?;
    let mut out = ThreeDRenderMode::new(kind);
    if let Some(c) = rm.get(b"AC").and_then(|o| rgb(graph, o)) {
        out.aux_colour = c;
    }
    out.face_colour = rm.get(b"FC").and_then(|o| rgb(graph, o));
    let number = |key: &[u8]| {
        rm.get(key)
            .and_then(|o| graph.resolve(o).as_number())
            .filter(|n| n.is_finite())
    };
    if let Some(o) = number(b"O") {
        out.opacity = o;
    }
    if let Some(cv) = number(b"CV") {
        out.crease_angle = cv;
    }
    Some(out)
}

/// `[/DeviceRGB r g b]`, each component in 0–1.
fn rgb<G: ObjectGraph + ?Sized>(graph: &G, o: &Object) -> Option<[f64; 3]> {
    let items = graph.resolve(o).as_array()?;
    let [space, r, g, b] = items else {
        return None;
    };
    let is_rgb = graph
        .resolve(space)
        .as_name()
        .is_some_and(|n| n.as_bytes() == b"DeviceRGB");
    let c = |o: &Object| {
        graph
            .resolve(o)
            .as_number()
            .filter(|v| (0.0..=1.0).contains(v))
    };
    is_rgb.then_some([c(r)?, c(g)?, c(b)?])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::ObjId;

    struct NoGraph;
    impl ObjectGraph for NoGraph {
        fn value(&self, _id: ObjId) -> Option<&Object> {
            None
        }
        fn trailer_entry(&self, _key: &[u8]) -> Option<&Object> {
            None
        }
    }

    fn view_with(rm: Dict) -> Dict {
        let mut v = Dict::new();
        v.insert(Name::from(b"RM"), Object::Dict(rm));
        v
    }

    #[test]
    fn every_entry_round_trips_and_defaults_are_not_written() {
        let mut mode = ThreeDRenderMode::new(ThreeDRenderModeKind::Illustration);
        assert_eq!(mode.to_dict().len(), 2, "Type and Subtype only");
        mode.aux_colour = [0.0, 0.0, 1.0];
        mode.face_colour = Some([1.0, 1.0, 0.5]);
        mode.opacity = 0.25;
        mode.crease_angle = 30.0;
        let read = read_render_mode(&NoGraph, &view_with(mode.to_dict()));
        assert_eq!(read, Some(mode));
    }

    #[test]
    fn malformed_entries_read_as_defaults_and_an_unknown_mode_as_none() {
        let mut rm = ThreeDRenderMode::new(ThreeDRenderModeKind::Wireframe).to_dict();
        let rgb = |space: &[u8], c: f64| {
            Object::Array(vec![
                Object::Name(Name::from(space)),
                Object::Real(c),
                Object::Real(0.0),
                Object::Real(0.0),
            ])
        };
        rm.insert(Name::from(b"AC"), rgb(b"DeviceCMYK", 1.0));
        rm.insert(Name::from(b"FC"), rgb(b"DeviceRGB", 2.0));
        rm.insert(Name::from(b"O"), Object::Name(Name::from(b"Half")));
        let read = read_render_mode(&NoGraph, &view_with(rm.clone()));
        assert_eq!(
            read,
            Some(ThreeDRenderMode::new(ThreeDRenderModeKind::Wireframe))
        );
        rm.insert(Name::from(b"Subtype"), Object::Name(Name::from(b"Points")));
        assert_eq!(read_render_mode(&NoGraph, &view_with(rm)), None);
        assert_eq!(read_render_mode(&NoGraph, &Dict::new()), None);
    }
}
