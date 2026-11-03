//! `3d-render --render-mode`: how faces, edges and vertices are drawn
//! (ISO 32000-1 §13.6.4.4 Tables 307, 308).

/// `--render-mode`: a Table 308 mode, or `auto`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum RenderModeArg {
    /// The mode of the view the model opens on, when it names one;
    /// otherwise solid (default).
    #[default]
    Auto,
    /// Lit, textured faces.
    Solid,
    /// Lit faces with their edges on top.
    SolidWireframe,
    /// Lit faces, see-through.
    Transparent,
    /// See-through faces with their edges on top.
    TransparentWireframe,
    /// The edges of each part's bounding box.
    BoundingBox,
    /// Each part's bounding box, see-through.
    TransparentBoundingBox,
    /// See-through bounding boxes with their edges.
    TransparentBoundingBoxOutline,
    /// Edges only.
    Wireframe,
    /// Edges only, in the model's colours, shaded.
    ShadedWireframe,
    /// Edges only, those hidden behind faces removed.
    HiddenWireframe,
    /// Vertices only.
    Vertices,
    /// Vertices only, in the model's colours, shaded.
    ShadedVertices,
    /// Flat faces in one colour, outlined.
    Illustration,
    /// Lit faces, outlined.
    SolidOutline,
    /// Lit faces, outlined, with dark areas lifted.
    ShadedIllustration,
}

/// `--mode-edges`: which edges the wireframe modes draw.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum ModeEdgesArg {
    /// Every triangle's edges (default).
    #[default]
    Triangles,
    /// Only creases at the mode's crease angle or sharper, and open edges.
    Creases,
}

/// The `3d-render` options that shape a render mode.
#[derive(Clone, Copy, Debug, PartialEq, clap::Args)]
pub(crate) struct RenderModeArgs {
    /// How the model is drawn: `auto` (default) uses the render mode of the
    /// view the model opens on, else `solid`. Edges and vertices are drawn
    /// in the view's auxiliary colour, black when it names none.
    #[arg(long, value_enum, default_value_t)]
    pub(crate) render_mode: RenderModeArg,
    /// Which edges the wireframe modes draw: `triangles` (default) or
    /// `creases`. The standard does not say.
    #[arg(long, value_enum, default_value_t)]
    pub(crate) mode_edges: ModeEdgesArg,
    /// Size in pixels, 1 to 16 (default 3), of the vertex modes' points.
    #[arg(long, default_value_t = 3, value_parser = clap::value_parser!(u32).range(1..=16))]
    pub(crate) point_size: u32,
    /// How much of a surface's unlit colour `shaded-illustration` adds to
    /// its lit colour, 0 to 1 (default 0.5). The standard does not say.
    #[arg(long, default_value_t = 0.5, value_parser = parse_unit)]
    pub(crate) emissive: f64,
}

fn parse_unit(s: &str) -> Result<f64, String> {
    match s.trim().parse::<f64>() {
        Ok(v) if (0.0..=1.0).contains(&v) => Ok(v),
        _ => Err(format!("`{s}`: expected a number from 0 to 1")),
    }
}

/// The mode `3d-render` draws in, and where it came from.
#[cfg(feature = "3d")]
pub(crate) struct ChosenMode {
    pub(crate) mode: pdfcer_3d::RenderMode,
    pub(crate) source: &'static str,
}

/// Resolves `--render-mode` against the opening view's `/RM`. An explicit
/// mode keeps the view's colours, opacity and crease angle when the view
/// names a mode, since those belong to the document's author.
#[cfg(feature = "3d")]
pub(crate) fn render_mode(
    a: &RenderModeArgs,
    saved: Option<&pdfcer_core::threed::ThreeDSavedView>,
) -> ChosenMode {
    let from_view = saved
        .and_then(|v| v.render_mode)
        .map(pdfcer_3d::RenderMode::from);
    let (mut mode, source) = match (kind_of(a.render_mode), from_view) {
        (Some(kind), Some(view)) => (pdfcer_3d::RenderMode { kind, ..view }, "--render-mode"),
        (Some(kind), None) => (pdfcer_3d::RenderMode::new(kind), "--render-mode"),
        (None, Some(view)) => (view, "the opening view's"),
        (None, None) => (
            pdfcer_3d::RenderMode::default(),
            "the default; the opening view names none",
        ),
    };
    mode.edges = match a.mode_edges {
        ModeEdgesArg::Triangles => pdfcer_3d::EdgeSet::Triangles,
        ModeEdgesArg::Creases => pdfcer_3d::EdgeSet::Creases,
    };
    mode.point_size = a.point_size;
    mode.emissive = a.emissive;
    ChosenMode { mode, source }
}

#[cfg(feature = "3d")]
fn kind_of(arg: RenderModeArg) -> Option<pdfcer_3d::RenderModeKind> {
    use pdfcer_3d::RenderModeKind as K;
    let i = (arg as usize).checked_sub(1)?;
    K::ALL.get(i).copied()
}

#[cfg(feature = "3d")]
impl ChosenMode {
    /// The `note: render mode:` line.
    pub(crate) fn print_note(&self) {
        let m = &self.mode;
        let [r, g, b] = m.aux_colour;
        println!(
            "note: render mode: {} ({}); edge and point colour #{r:02x}{g:02x}{b:02x}, \
             opacity {}, crease angle {}",
            m.kind.as_name(),
            self.source,
            m.opacity,
            m.crease_angle
        );
    }
}

#[cfg(all(test, feature = "3d"))]
mod tests {
    use super::*;
    use clap::ValueEnum;

    #[test]
    fn every_mode_arg_maps_to_the_kind_of_the_same_name() {
        for arg in RenderModeArg::value_variants().iter().skip(1) {
            let name = arg
                .to_possible_value()
                .map(|v| v.get_name().replace('-', ""));
            let kind = kind_of(*arg).map(|k| k.as_name().to_ascii_lowercase());
            assert_eq!(name, kind, "{arg:?}");
        }
        assert_eq!(kind_of(RenderModeArg::Auto), None);
    }
}
