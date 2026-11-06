//! Render modes: how a model's faces, edges and vertices are drawn
//! (ISO 32000-1 §13.6.4.4 Tables 307, 308; PDF 1.7).
//!
//! Where the standard leaves a choice open, [`RenderMode`] carries it as a
//! field with a stated default: the emissive amount of
//! [`RenderModeKind::ShadedIllustration`], which edges the wireframe modes
//! draw, the point size of the vertex modes.

use crate::RenderOptions;

/// A render mode's `/Subtype` (ISO 32000-1 Table 308), in table order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum RenderModeKind {
    /// Lit, textured faces.
    #[default]
    Solid,
    /// Lit, textured faces with edges in the auxiliary colour on top.
    SolidWireframe,
    /// Lit, textured faces at [`RenderMode::opacity`].
    Transparent,
    /// [`Self::Transparent`] with opaque edges in the auxiliary colour.
    TransparentWireframe,
    /// Each node's bounding box's edges, in the auxiliary colour.
    BoundingBox,
    /// Each node's bounding box's faces, in the face colour at
    /// [`RenderMode::opacity`].
    TransparentBoundingBox,
    /// [`Self::TransparentBoundingBox`] with the boxes' edges in the
    /// auxiliary colour.
    TransparentBoundingBoxOutline,
    /// Edges only, in the auxiliary colour.
    Wireframe,
    /// Edges only, in their vertices' colours, shaded.
    ShadedWireframe,
    /// Edges in the auxiliary colour, those faces hide removed.
    HiddenWireframe,
    /// Vertices only, in the auxiliary colour.
    Vertices,
    /// Vertices only, in their colours, shaded.
    ShadedVertices,
    /// Unlit faces in the face colour, silhouette edges in the auxiliary
    /// colour.
    Illustration,
    /// Lit, textured faces with silhouette edges in the auxiliary colour.
    SolidOutline,
    /// [`Self::SolidOutline`] plus an emissive term
    /// ([`RenderMode::emissive`]) that lifts poorly lit areas.
    ShadedIllustration,
}

impl RenderModeKind {
    /// Every mode, in Table 308's order.
    pub const ALL: [RenderModeKind; 15] = [
        Self::Solid,
        Self::SolidWireframe,
        Self::Transparent,
        Self::TransparentWireframe,
        Self::BoundingBox,
        Self::TransparentBoundingBox,
        Self::TransparentBoundingBoxOutline,
        Self::Wireframe,
        Self::ShadedWireframe,
        Self::HiddenWireframe,
        Self::Vertices,
        Self::ShadedVertices,
        Self::Illustration,
        Self::SolidOutline,
        Self::ShadedIllustration,
    ];

    /// The `/Subtype` name, as written.
    ///
    /// ```
    /// use pdfcer_3d::RenderModeKind;
    /// assert_eq!(RenderModeKind::HiddenWireframe.as_name(), "HiddenWireframe");
    /// assert_eq!(RenderModeKind::from_name("Vertices"), Some(RenderModeKind::Vertices));
    /// // Table 307's NOTE says "Points"; no such mode exists.
    /// assert_eq!(RenderModeKind::from_name("Points"), None);
    /// ```
    #[must_use]
    pub fn as_name(self) -> &'static str {
        match self {
            Self::Solid => "Solid",
            Self::SolidWireframe => "SolidWireframe",
            Self::Transparent => "Transparent",
            Self::TransparentWireframe => "TransparentWireframe",
            Self::BoundingBox => "BoundingBox",
            Self::TransparentBoundingBox => "TransparentBoundingBox",
            Self::TransparentBoundingBoxOutline => "TransparentBoundingBoxOutline",
            Self::Wireframe => "Wireframe",
            Self::ShadedWireframe => "ShadedWireframe",
            Self::HiddenWireframe => "HiddenWireframe",
            Self::Vertices => "Vertices",
            Self::ShadedVertices => "ShadedVertices",
            Self::Illustration => "Illustration",
            Self::SolidOutline => "SolidOutline",
            Self::ShadedIllustration => "ShadedIllustration",
        }
    }

    /// The mode a `/Subtype` name names, or `None` for a name Table 308
    /// does not list (the dictionary is then ignored).
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_name() == name)
    }
}

/// `/FC`, the face colour of [`RenderModeKind::Illustration`] and the
/// transparent bounding-box modes (Table 307).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum FaceColour {
    /// `/BG`, the default: the background colour
    /// ([`RenderOptions::background`]).
    #[default]
    Background,
    /// A DeviceRGB colour.
    Rgb([u8; 3]),
}

/// Which edges the wireframe modes (2, 4, 8, 9, 10) draw. Table 308 says
/// "edges" without saying which.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum EdgeSet {
    /// Every triangle's edges (the default), as a tessellation viewer draws
    /// them.
    #[default]
    Triangles,
    /// Only edges where the faces meet at [`RenderMode::crease_angle`] or
    /// more, and open or non-manifold edges: a mesh's feature lines.
    Creases,
}

/// A render mode dictionary (ISO 32000-1 Table 307), with the choices the
/// standard leaves open. [`Default`] is [`RenderModeKind::Solid`] with
/// every entry at its default.
///
/// ```
/// use pdfcer_3d::{RenderMode, RenderModeKind, RenderOptions};
/// let options = RenderOptions {
///     render_mode: RenderMode::new(RenderModeKind::Illustration),
///     ..RenderOptions::default()
/// };
/// assert_eq!(options.render_mode.crease_angle, 45.0);
/// ```
#[derive(Debug, Clone, Copy)]
pub struct RenderMode {
    /// `/Subtype`.
    pub kind: RenderModeKind,
    /// `/AC`, the auxiliary colour of edges and points: black by default.
    pub aux_colour: [u8; 3],
    /// `/FC`.
    pub face_colour: FaceColour,
    /// `/O`, 0–1 (default 0.5): the opacity the transparent modes draw
    /// faces at, multiplying their own alpha. "Standard additive blend" is
    /// undefined in the standard; faces are composited source-over.
    pub opacity: f64,
    /// `/CV`, degrees (default 45): two faces both facing the viewer whose
    /// normals are this far apart or more share a silhouette edge.
    pub crease_angle: f64,
    /// The share of a surface's unlit colour
    /// [`RenderModeKind::ShadedIllustration`] adds to its lit colour,
    /// 0–1 (default 0.5); the standard names the term but not its amount.
    pub emissive: f64,
    /// Which edges the wireframe modes draw.
    pub edges: EdgeSet,
    /// The vertex modes' point size, pixels (default 3; 0 draws as 1).
    pub point_size: u32,
}

impl RenderMode {
    /// `kind` with every other entry at its default.
    #[must_use]
    pub fn new(kind: RenderModeKind) -> Self {
        RenderMode {
            kind,
            aux_colour: [0, 0, 0],
            face_colour: FaceColour::Background,
            opacity: 0.5,
            crease_angle: 45.0,
            emissive: 0.5,
            edges: EdgeSet::Triangles,
            point_size: 3,
        }
    }

    /// The cosine the normals' dot product of two front faces must be at or
    /// below for their edge to be a silhouette.
    pub(crate) fn crease_cos(&self) -> f64 {
        let degrees = if self.crease_angle.is_finite() {
            self.crease_angle.clamp(0.0, 180.0)
        } else {
            45.0
        };
        degrees.to_radians().cos()
    }

    /// [`Self::opacity`], clamped to 0–1 (0.5 when not finite).
    pub(crate) fn clamped_opacity(&self) -> f64 {
        if self.opacity.is_finite() {
            self.opacity.clamp(0.0, 1.0)
        } else {
            0.5
        }
    }

    /// [`Self::emissive`], clamped to 0–1 (0 when not finite).
    pub(crate) fn clamped_emissive(&self) -> f64 {
        if self.emissive.is_finite() {
            self.emissive.clamp(0.0, 1.0)
        } else {
            0.0
        }
    }
}

impl Default for RenderMode {
    fn default() -> Self {
        Self::new(RenderModeKind::Solid)
    }
}

impl PartialEq for RenderMode {
    /// Equal when every field is; numbers compared by their bits.
    fn eq(&self, other: &Self) -> bool {
        self.kind == other.kind
            && self.aux_colour == other.aux_colour
            && self.face_colour == other.face_colour
            && self.opacity.to_bits() == other.opacity.to_bits()
            && self.crease_angle.to_bits() == other.crease_angle.to_bits()
            && self.emissive.to_bits() == other.emissive.to_bits()
            && self.edges == other.edges
            && self.point_size == other.point_size
    }
}

impl Eq for RenderMode {}

/// What a mode draws faces as.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Faces {
    /// None of the model's faces.
    None,
    /// Their own paint.
    Painted,
    /// Their own paint, alpha times this.
    Translucent(f64),
    /// This colour, unlit, keeping the mesh's alpha.
    Coloured([u8; 3]),
    /// The background colour, opaque and unlit: only hides what is behind.
    Hiding([u8; 3]),
    /// Each node's bounding box, this colour at this alpha, unlit.
    Boxes([u8; 3], f64),
}

/// Which edges a mode draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Edges {
    None,
    /// The mesh edges [`RenderMode::edges`] names.
    Mesh,
    /// Silhouettes only, decided per frame.
    Silhouette,
    /// The bounding boxes' edges.
    Boxes,
}

/// How a mode draws a model's wires.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WireLook {
    /// Lines in their own colours.
    Own,
    /// Lines in the auxiliary colour.
    Aux,
    /// Their points, in the auxiliary colour.
    AuxPoints,
    /// Their points, in their own colours.
    OwnPoints,
    /// Not drawn.
    Hidden,
}

/// What a frame draws under a mode.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Plan {
    pub(crate) faces: Faces,
    pub(crate) edges: Edges,
    /// Whether the welded vertices are drawn as points.
    pub(crate) points: bool,
    /// Whether edges and points take their vertices' colours, shaded,
    /// rather than the auxiliary colour.
    pub(crate) shaded: bool,
    pub(crate) wires: WireLook,
    /// The emissive share added to lit faces.
    pub(crate) emissive: f64,
}

impl Plan {
    /// The plan for `options.render_mode`.
    pub(crate) fn of(options: &RenderOptions) -> Plan {
        use RenderModeKind as K;
        let mode = &options.render_mode;
        let [r, g, b, _] = options.background;
        let fc = match mode.face_colour {
            FaceColour::Background => [r, g, b],
            FaceColour::Rgb(c) => c,
        };
        let o = mode.clamped_opacity();
        let plan = |faces, edges| Plan {
            faces,
            edges,
            points: false,
            shaded: false,
            wires: WireLook::Own,
            emissive: 0.0,
        };
        match mode.kind {
            K::Solid => plan(Faces::Painted, Edges::None),
            K::SolidWireframe => plan(Faces::Painted, Edges::Mesh),
            K::Transparent => plan(Faces::Translucent(o), Edges::None),
            K::TransparentWireframe => plan(Faces::Translucent(o), Edges::Mesh),
            K::BoundingBox => Plan {
                wires: WireLook::Hidden,
                ..plan(Faces::None, Edges::Boxes)
            },
            K::TransparentBoundingBox => Plan {
                wires: WireLook::Hidden,
                ..plan(Faces::Boxes(fc, o), Edges::None)
            },
            K::TransparentBoundingBoxOutline => Plan {
                wires: WireLook::Hidden,
                ..plan(Faces::Boxes(fc, o), Edges::Boxes)
            },
            K::Wireframe => Plan {
                wires: WireLook::Aux,
                ..plan(Faces::None, Edges::Mesh)
            },
            K::ShadedWireframe => Plan {
                shaded: true,
                ..plan(Faces::None, Edges::Mesh)
            },
            K::HiddenWireframe => Plan {
                wires: WireLook::Aux,
                ..plan(Faces::Hiding([r, g, b]), Edges::Mesh)
            },
            K::Vertices => Plan {
                points: true,
                wires: WireLook::AuxPoints,
                ..plan(Faces::None, Edges::None)
            },
            K::ShadedVertices => Plan {
                points: true,
                shaded: true,
                wires: WireLook::OwnPoints,
                ..plan(Faces::None, Edges::None)
            },
            K::Illustration => plan(Faces::Coloured(fc), Edges::Silhouette),
            K::SolidOutline => plan(Faces::Painted, Edges::Silhouette),
            K::ShadedIllustration => Plan {
                emissive: mode.clamped_emissive(),
                ..plan(Faces::Painted, Edges::Silhouette)
            },
        }
    }

    /// Whether the faces this plan draws are unlit.
    pub(crate) fn unlit(&self) -> bool {
        matches!(
            self.faces,
            Faces::Coloured(_) | Faces::Hiding(_) | Faces::Boxes(..) | Faces::None
        )
    }
}

/// `options` as a frame under its render mode lights it: unlit
/// ([`crate::LightingScheme::None`]) when the mode's faces are flat colour.
/// A host building [`crate::FrameLights`] gets this applied for it.
pub(crate) fn frame_options(options: &RenderOptions) -> RenderOptions {
    if Plan::of(options).unlit() {
        RenderOptions {
            lighting: crate::LightingScheme::None,
            ..*options
        }
    } else {
        *options
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_name_reads_back_and_the_crease_cosine_is_clamped() {
        for kind in RenderModeKind::ALL {
            assert_eq!(RenderModeKind::from_name(kind.as_name()), Some(kind));
        }
        let mut mode = RenderMode::new(RenderModeKind::Illustration);
        assert!((mode.crease_cos() - 45f64.to_radians().cos()).abs() < 1e-12);
        mode.crease_angle = f64::NAN;
        assert!((mode.crease_cos() - 45f64.to_radians().cos()).abs() < 1e-12);
        mode.crease_angle = 500.0;
        assert!((mode.crease_cos() + 1.0).abs() < 1e-12);
    }

    #[test]
    fn flat_coloured_modes_are_unlit_and_the_rest_keep_the_scheme() {
        let with = |kind| RenderOptions {
            render_mode: RenderMode::new(kind),
            lighting: crate::LightingScheme::Cad,
            ..RenderOptions::default()
        };
        for kind in RenderModeKind::ALL {
            let unlit = frame_options(&with(kind)).lighting == crate::LightingScheme::None;
            let expected = matches!(
                kind,
                RenderModeKind::Illustration
                    | RenderModeKind::HiddenWireframe
                    | RenderModeKind::BoundingBox
                    | RenderModeKind::TransparentBoundingBox
                    | RenderModeKind::TransparentBoundingBoxOutline
                    | RenderModeKind::Wireframe
                    | RenderModeKind::ShadedWireframe
                    | RenderModeKind::Vertices
                    | RenderModeKind::ShadedVertices
            );
            assert_eq!(unlit, expected, "{kind:?}");
        }
    }
}
