//! # pdfcer-flatten — PDF transparency over flat colours, as opaque regions
//!
//! A vector format with no alpha, no blend modes and no overprint (an EMF, a
//! plotter file, a cutter path) can still show a transparent page exactly
//! when every paint involved is a polygon of one colour: the overlay of the
//! paints cuts the page into atomic regions, each covered by a fixed set of
//! paints, and §11.4.8's compositing is then one colour per region.
//! [`flatten`] computes those regions and colours.
//!
//! ## Model
//!
//! - A [`Paint`] is a polygon ([`Shape`]), a colour in the blending
//!   [`Space`], a constant alpha, a [`Blend`] mode and an optional
//!   [`Overprint`]. Shape is 1 inside the polygon and 0 outside: there is no
//!   antialiasing and no soft mask.
//! - A [`Group`] holds paints and groups, is isolated or not, knockout or
//!   not (ISO 32000-2 §11.4.5, §11.4.6), and is composited into its parent
//!   with its own alpha and blend.
//! - The root group is the page group: it is composited over nothing, so its
//!   [`Group::blend`] and isolation are ignored and its [`Group::alpha`]
//!   scales the result. The caller composites each region over its paper.
//!
//! ## Arithmetic
//!
//! §11.4.8 per region, with these choices where the standard leaves room:
//!
//! - A non-isolated group is evaluated twice, over a clear backdrop (its
//!   alpha `α_gn`) and over its backdrop (its colour), then §11.4.4's
//!   backdrop removal is applied. Exact for every case §11.4.8 covers.
//! - Inside a knockout group a nested group's shape is taken to be its alpha
//!   (`f_g ≈ α_g`), which §11.4.6 permits when every element inside it has
//!   `q = 1`.
//! - An [`Overprint`] paint replaces the components its tints name and keeps
//!   the backdrop's others, then mixes the result into the backdrop by the
//!   paint's alpha. In an additive space the backdrop goes through the
//!   caller's [`Inks`] to CMYK and back; a transparent backdrop is paper.
//!   This is `pdfcer-render`'s overprint model, not §11.4.8's, which has no
//!   overprint term for an additive group.
//!
//! ## Failure modes
//!
//! [`FlattenError::TooComplex`] when a [`Limits`] ceiling is passed (the
//! caller falls back to pixels), [`FlattenError::Unsupported`] for overprint
//! inside a knockout group.

#![forbid(unsafe_code)]
// Panic-free: the input is built from recordings of untrusted pages. Tests
// opt out per module.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::unreachable,
    clippy::todo,
    clippy::unimplemented
)]

mod blend;
mod composite;
mod overlay;

/// A point in device space.
pub type Point = [f64; 2];

/// A colour in a blending [`Space`]: the space's components first, the rest
/// zero. Each component is in `0.0..=1.0`.
pub type Colour = [f32; 4];

/// How a [`Shape`]'s contours decide inside (ISO 32000-2 §8.5.3.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum FillRule {
    /// Nonzero winding number.
    #[default]
    NonZero,
    /// Even-odd crossing count.
    EvenOdd,
}

/// A polygon: closed contours of straight edges (the last point joins the
/// first) and the rule that decides their inside.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Shape {
    /// The contours, in device space.
    pub contours: Vec<Vec<Point>>,
    /// The inside rule.
    pub rule: FillRule,
}

impl Shape {
    /// The part of `self` inside `other` — a paint under a clipping path.
    /// The result's contours are disjoint outers and their holes, under
    /// [`FillRule::EvenOdd`]; it has no contours when nothing is common.
    ///
    /// ```
    /// use pdfcer_flatten::Shape;
    /// let square = |x: f64| Shape {
    ///     contours: vec![vec![[x, 0.0], [x + 2.0, 0.0], [x + 2.0, 2.0], [x, 2.0]]],
    ///     ..Shape::default()
    /// };
    /// assert!(!square(0.0).intersection(&square(1.0)).contours.is_empty());
    /// assert!(square(0.0).intersection(&square(5.0)).contours.is_empty());
    /// ```
    #[must_use]
    pub fn intersection(&self, other: &Shape) -> Shape {
        overlay::intersection(self, other)
    }
}

/// The blending colour space of the whole scene (§11.3.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Space {
    /// Additive RGB: three components, 1 is full light.
    Rgb,
    /// Subtractive CMYK: four components, 1 is full ink. Blend functions
    /// work on complements (§11.3.4).
    Cmyk,
}

impl Space {
    /// The number of colour components.
    #[must_use]
    pub const fn components(self) -> usize {
        match self {
            Self::Rgb => 3,
            Self::Cmyk => 4,
        }
    }

    /// Whether 1 means full ink rather than full light.
    #[must_use]
    pub const fn is_subtractive(self) -> bool {
        matches!(self, Self::Cmyk)
    }

    /// Unmarked paper: white light, or no ink.
    #[must_use]
    pub const fn paper(self) -> Colour {
        match self {
            Self::Rgb => [1.0, 1.0, 1.0, 0.0],
            Self::Cmyk => [0.0; 4],
        }
    }
}

/// A blend mode (ISO 32000-2 Tables 136 and 137).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Blend {
    /// `B = cs`.
    #[default]
    Normal,
    /// `cb · cs`.
    Multiply,
    /// `cb + cs − cb · cs`.
    Screen,
    /// `HardLight(cs, cb)`.
    Overlay,
    /// `min(cb, cs)`.
    Darken,
    /// `max(cb, cs)`.
    Lighten,
    /// Brightens the backdrop toward the source.
    ColorDodge,
    /// Darkens the backdrop toward the source.
    ColorBurn,
    /// Multiply or Screen, by the source.
    HardLight,
    /// Darkens or lightens, by the source.
    SoftLight,
    /// `|cb − cs|`.
    Difference,
    /// `cb + cs − 2 · cb · cs`.
    Exclusion,
    /// The source's hue, the backdrop's saturation and luminosity.
    Hue,
    /// The source's saturation, the backdrop's hue and luminosity.
    Saturation,
    /// The source's hue and saturation, the backdrop's luminosity.
    Color,
    /// The source's luminosity, the backdrop's hue and saturation.
    Luminosity,
}

impl Blend {
    /// Whether the mode works per component (Table 136) rather than on the
    /// whole colour (Table 137).
    #[must_use]
    pub const fn is_separable(self) -> bool {
        !matches!(
            self,
            Self::Hue | Self::Saturation | Self::Color | Self::Luminosity
        )
    }
}

/// A paint routed to overprint (§11.7.4.3): per process component C, M, Y,
/// K, the tint deposited, or `None` where the backdrop's survives (Table 149
/// already applied by the caller, `OPM` included).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Overprint {
    /// Per CMYK component, the tint, or `None` to keep the backdrop.
    pub tints: [Option<f32>; 4],
    /// Which of the caller's [`Inks`] pairs converts an additive backdrop
    /// for this paint. Ignored in [`Space::Cmyk`].
    pub model: usize,
}

/// One flat-coloured paint.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Paint {
    /// Where it paints.
    pub shape: Shape,
    /// Its colour, in the scene's [`Space`].
    pub colour: Colour,
    /// Its constant alpha, `0.0..=1.0`.
    pub alpha: f32,
    /// Its blend mode. Ignored when [`Self::overprint`] is set.
    pub blend: Blend,
    /// Overprint, or `None` to composite normally.
    pub overprint: Option<Overprint>,
}

/// A member of a [`Group`].
#[derive(Debug, Clone, PartialEq)]
pub enum Element {
    /// A paint.
    Paint(Paint),
    /// A nested transparency group.
    Group(Group),
}

/// A transparency group (§11.4.7).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Group {
    /// Its members, bottom first.
    pub elements: Vec<Element>,
    /// Whether it composites over a clear backdrop rather than its parent's
    /// (§11.4.5).
    pub isolated: bool,
    /// Whether each member composites over the group's initial backdrop
    /// rather than the members beneath it (§11.4.6).
    pub knockout: bool,
    /// The constant alpha it is composited into its parent with.
    pub alpha: f32,
    /// The blend mode it is composited into its parent with.
    pub blend: Blend,
}

/// The conversions an additive-space [`Overprint`] makes through CMYK,
/// chosen per paint by [`Overprint::model`].
pub trait Inks {
    /// An additive RGB colour as CMYK tints.
    fn to_cmyk(&self, model: usize, rgb: [f32; 3]) -> [f32; 4];
    /// CMYK tints as an additive RGB colour.
    fn to_rgb(&self, model: usize, cmyk: [f32; 4]) -> [f32; 3];
}

/// `k = 1 − max(r, g, b)` and its exact inverse, for every model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct NaiveInks;

impl Inks for NaiveInks {
    fn to_cmyk(&self, _model: usize, [r, g, b]: [f32; 3]) -> [f32; 4] {
        let k = 1.0 - r.max(g).max(b);
        if k >= 1.0 {
            return [0.0, 0.0, 0.0, 1.0];
        }
        let inv = 1.0 - k;
        [
            (1.0 - r - k) / inv,
            (1.0 - g - k) / inv,
            (1.0 - b - k) / inv,
            k,
        ]
    }

    fn to_rgb(&self, _model: usize, [c, m, y, k]: [f32; 4]) -> [f32; 3] {
        let w = 1.0 - k;
        [(1.0 - c) * w, (1.0 - m) * w, (1.0 - y) * w]
    }
}

/// Ceilings on the work [`flatten`] does. Past one it refuses rather than
/// run long.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Limits {
    /// Paints in the whole tree.
    pub max_paints: usize,
    /// Atomic regions in the result.
    pub max_regions: usize,
    /// Vertices across the regions at any step.
    pub max_vertices: usize,
    /// Group nesting depth.
    pub max_depth: usize,
}

impl Default for Limits {
    /// 64 paints, 1024 regions, 200 000 vertices, 32 levels of nesting.
    fn default() -> Self {
        Self {
            max_paints: 64,
            max_regions: 1024,
            max_vertices: 200_000,
            max_depth: 32,
        }
    }
}

/// One atomic region of the result: covered by exactly [`Self::paints`].
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Region {
    /// Its contours, even-odd: outer boundaries and their holes.
    pub contours: Vec<Vec<Point>>,
    /// The composited colour, in the scene's [`Space`].
    pub colour: Colour,
    /// The composited alpha.
    pub alpha: f32,
    /// The paints covering it, as indices in depth-first order (the order
    /// the paints are met reading the tree bottom first, groups expanded
    /// where they stand), ascending.
    pub paints: Vec<usize>,
}

/// Which [`Limits`] ceiling was passed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Ceiling {
    /// [`Limits::max_paints`].
    Paints,
    /// [`Limits::max_regions`].
    Regions,
    /// [`Limits::max_vertices`].
    Vertices,
    /// [`Limits::max_depth`].
    Depth,
}

/// Why [`flatten`] returned no regions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, thiserror::Error)]
#[non_exhaustive]
pub enum FlattenError {
    /// The scene passed a ceiling; flatten it some other way.
    #[error("the scene is over the flattening ceiling ({0:?})")]
    TooComplex(Ceiling),
    /// The scene uses a combination this crate does not composite.
    #[error("the scene is not flattenable: {0}")]
    Unsupported(&'static str),
}

/// Flatten `root` in `space`: the atomic regions of its paints, each with
/// the colour and alpha the page group composites there. Where no paint
/// covers there is no region.
///
/// # Errors
///
/// [`FlattenError::TooComplex`] when a [`Limits`] ceiling is passed;
/// [`FlattenError::Unsupported`] for an overprint inside a knockout group.
///
/// # Examples
///
/// ```
/// use pdfcer_flatten::{Element, Group, Limits, NaiveInks, Paint, Shape, Space, flatten};
///
/// let square = |x: f64| Shape {
///     contours: vec![vec![[x, 0.0], [x + 2.0, 0.0], [x + 2.0, 2.0], [x, 2.0]]],
///     ..Shape::default()
/// };
/// let half_red = Paint { shape: square(1.0), colour: [1.0, 0.0, 0.0, 0.0], alpha: 0.5, ..Paint::default() };
/// let blue = Paint { shape: square(0.0), colour: [0.0, 0.0, 1.0, 0.0], alpha: 1.0, ..Paint::default() };
/// let page = Group {
///     elements: vec![Element::Paint(blue), Element::Paint(half_red)],
///     isolated: true,
///     alpha: 1.0,
///     ..Group::default()
/// };
/// let regions = flatten(&page, Space::Rgb, &NaiveInks, &Limits::default())?;
/// // Blue alone, red over blue, red alone.
/// assert_eq!(regions.len(), 3);
/// let both = regions.iter().find(|r| r.paints == [0, 1]).unwrap();
/// assert_eq!(both.colour, [0.5, 0.0, 0.5, 0.0]);
/// # Ok::<(), pdfcer_flatten::FlattenError>(())
/// ```
pub fn flatten(
    root: &Group,
    space: Space,
    inks: &dyn Inks,
    limits: &Limits,
) -> Result<Vec<Region>, FlattenError> {
    let mut shapes = Vec::new();
    composite::collect(root, &mut shapes, 0, limits)?;
    if shapes.len() > limits.max_paints {
        return Err(FlattenError::TooComplex(Ceiling::Paints));
    }
    let atoms = overlay::atoms(&shapes, limits)?;
    let mut out = Vec::with_capacity(atoms.len());
    for atom in atoms {
        let mut covered = vec![false; shapes.len()];
        for &i in &atom.paints {
            if let Some(c) = covered.get_mut(i) {
                *c = true;
            }
        }
        let px = composite::evaluate(root, space, inks, &covered)?;
        out.push(Region {
            contours: atom.contours,
            colour: px.c,
            alpha: px.a,
            paints: atom.paints,
        });
    }
    Ok(out)
}
