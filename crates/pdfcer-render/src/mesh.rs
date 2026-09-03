//! # Mesh shadings — `ShadingType` 4, 5, 6 and 7 (ISO 32000-1 §8.7.4.5.5–.8)
//!
//! The four shading types whose geometry lives in a **stream** rather than
//! in a dictionary. Everything in this file is sourced label-by-label from
//! the PDF-spec RAG file
//! `D:\Dev\Rag-Specialized\PDF_Spec\iso32000\iso32000__s__8.7.4.5__mesh.md`
//! (labels `MSH1`–`MSH36`, `MSH-A1`–`MSH-A5`, `MSH-N1`–`MSH-N5`). Each
//! function names the labels it implements so a future reader can check the
//! code against the clause without re-deriving anything, and so a claim
//! that is **pdfcer's own** rather than the standard's is visibly marked.
//!
//! ## Why this is a separate module from [`crate::shading`]
//!
//! Not tidiness — the two families are rendered by **opposite** algorithms,
//! and mixing them in one file invites the wrong one being reached for.
//!
//! * The analytic types (1/2/3) are **inverse-mapped**: walk destination
//!   pixels, map each back into the shading's target space, solve for a
//!   parametric `t`, look the colour up in a pre-sampled ramp. There is a
//!   closed form for `t` at every point, so the pixel loop is the whole
//!   algorithm.
//! * A mesh has **no closed form**. Its colour is defined only *on* a set
//!   of triangles or patches, by interpolation between vertices. So it is
//!   **forward-rasterised**: decode the stream into geometry in target
//!   space, map that geometry into device space, and scan-convert it.
//!
//! [`crate::shading::Shading`] still owns the dictionary half — colour
//! space, `/Function`, `/BBox`, `/Background`, the paint route — because
//! Table 78 governs those identically for all seven types (`MSH-N1`:
//! measured zero mentions of `/Background`, `/BBox`, `/AntiAlias`,
//! `/Extend` or smoothness across §8.7.4.5.5–.8, which is a *positive*
//! result — the general rules apply unmodified).
//!
//! ## The data flow, end to end
//!
//! ```text
//!   shading stream bytes  (already through /Filter — MSH1: types 4-7
//!                          "shall be represented as streams")
//!            |
//!            v
//!   BitReader        MSH3: MSB-first, big-endian within each field,
//!            |             fields in the stated order, no straddling
//!            v
//!   raw integers     BitsPerCoordinate / BitsPerComponent / BitsPerFlag
//!            |
//!            v
//!   /Decode          MSH11: y = Dmin + x·(Dmax − Dmin)/(2ⁿ − 1), with a
//!            |             DIFFERENT n for coordinates and components
//!            v
//!   target-space geometry + per-vertex colour
//!            |            (Rgb when the file gives n components;
//!            |             a parametric t when /Function is present —
//!            |             MSH14, and the t is NOT resolved here)
//!            v
//!   Mesh { data: Triangles | Patches }        <- what this module stores
//!            |
//!            v  paint time, device transform known
//!   subdivision (patches only, MSH-A3) -> triangles
//!            |
//!            v
//!   scan conversion with barycentric interpolation -> pixels
//! ```
//!
//! ## Three traps this module is shaped to avoid, all from the RAG's §9
//!
//! 1. **One evaluator, not two.** A Coons patch (type 6) *is* a
//!    tensor-product patch (type 7) whose four internal control points are
//!    derived from its boundary (`MSH30`). So [`Patch`] always holds
//!    **sixteen** points and a type 6 patch is converted on the way in.
//!    Writing a separate Coons evaluator would be a second rendering path
//!    for the same content, which project rule 4's 2026-08-13 narrowing
//!    identifies as a bug class in its own right.
//! 2. **The corner-colour walk-around order** (`MSH24`, `MSH29`). Colours
//!    are given in the order the *control points* walk the boundary —
//!    `(0,0)`, `(0,1)`, `(1,1)`, `(1,0)` — **not** in raster order. Getting
//!    it wrong transposes the patch's colours across one diagonal, which
//!    renders, looks plausible, and is wrong.
//! 3. **`/Function` changes the record size** (`MSH14`). With it, each
//!    vertex or corner carries exactly **one** `BitsPerComponent`-wide
//!    field no matter how many components the colour space has. Sizing the
//!    record from the colour space while `/Function` is present
//!    desynchronises every record after the first, which reads as a corrupt
//!    file rather than as a parse bug.
//!
//! ## What is deliberately NOT done here, and where it is disclosed
//!
//! ★★ THIS SECTION LISTED THREE THINGS AND `Pass 137.1` DELIVERED TWO OF
//! THEM. The old text is kept legible rather than silently replaced, because
//! *what it said* is part of why the defect survived as long as it did — a
//! reader who checked this list came away believing the gap was known and
//! tracked, when in fact nothing was tracking it and the operator found it by
//! looking at a page.
//!
//! ~~"**No native-ink (colorant) route.** A mesh resolves its colour to sRGB
//! at parse time (or through the ramp at paint time), so by the time a pixel
//! exists there are no authored colorants left to composite… Making the mesh
//! path native is the mesh half of `Pass 97.1k`."~~ — **DONE**, `Pass 137.1`.
//! [`Shade::Ink`] carries the authored colorants alongside the converted
//! value, [`MeshColorants`] says once and for all whether a mesh has any, and
//! [`paint_cmyk`] composites them into the buffer directly. The premise of
//! the old sentence was right; the implied conclusion — that there was
//! nothing to be done short of a rework — was not. The answer was a
//! **carrier**, not a wider gate.
//!
//! ~~"**No overprint.** Same cause as above and the same disclosure."~~ —
//! **DONE, and it came for free**, which is the part worth noticing.
//! [`paint_cmyk`] takes `rules` because every ink source in this crate does,
//! so a `Separation`/`DeviceN` mesh under `/OP true` gets §11.7.4.3's
//! composite by the same route a path does. Nothing in this module reasons
//! about Table 149; it hands values to the buffer, which already did.
//!
//! * **No anti-aliasing of the mesh outline.** `/AntiAlias` is a hint
//!   (Table 78) and defaults to false; pixel centres decide coverage.
//!   Interior edges between adjacent triangles are seamless regardless
//!   (see [`fill_triangle`]); it is only the *outer* silhouette that is
//!   hard-edged. **Still true.**
//!
//! ★ What still bridges, so this list stays honest: a mesh whose colour
//! space is **additive** ([`MeshColorants::None`]), and a **parametric** mesh
//! whose ramp carries no colorants. Neither has authored ink to preserve, so
//! the conversion is the honest route rather than a shortfall — and both are
//! still counted in `cmyk_bridged_pixels`.
//!
//! ## The ambiguities this module had to take a position on
//!
//! * **`MSH-A1` — patch-record byte alignment.** §8.7.4.5.5's padding rule
//!   is scoped to *"each set of **vertex** data"*, and a patch has no
//!   vertices; §8.7.4.5.7/.8 defer to it without saying what the padded
//!   unit becomes. Two readings survive the text and **ISO 32000-2 does not
//!   resolve it** (RAG delta `D3`: the sentence is word-for-word
//!   identical). Under the project's standing "a spec ambiguity becomes a
//!   setting" rule this is
//!   [`pdfcer_core::settings::MeshPatchPadding`], defaulting to
//!   `PerRecord` — the only reading under which the deferral has any
//!   content. It is observable **only** when
//!   `BitsPerFlag + k·BitsPerCoordinate + m·BitsPerComponent` is not a
//!   multiple of 8, which the common real-world combination
//!   (`8`/`16`/`8`, and the `8`/`32`/`8` this project has measured) never
//!   is.
//! * **`MSH-A2` — the interpolation function for types 4/5 is explicitly
//!   left open** ("may be linear or nonlinear"). pdfcer interpolates
//!   linearly in barycentric coordinates, which is what "Gouraud" names and
//!   what every renderer does; the standard permits it and does not require
//!   it.
//! * **`MSH-A3` — nothing states a subdivision density for a patch**, and
//!   `/SM` bounds *colour* error rather than geometric deviation. pdfcer
//!   picks the density from the patch's **device-space size** (see
//!   [`subdivision_for`]), so the approximation is bounded in the units a
//!   viewer actually perceives rather than in patch-parameter units. This
//!   is deliberately **not** a setting: a knob whose right value is a
//!   function of zoom is a knob the operator cannot set correctly.
//! * **`MSH-A4` — an `Indexed` mesh colour space.** The mesh clauses do not
//!   repeat types 1/2/3's blanket exclusion of `Indexed`; a literal read
//!   permits one, in which interpolation would happen **on palette
//!   indices** and produce palette-order rainbows. pdfcer refuses it with a
//!   named disclosure rather than rendering something nobody intended.
//! * **`MSH-N2` — only type 4 states an error condition.** For a truncated
//!   final record, a type-5 stream that is not a whole number of rows, or a
//!   first patch whose flag is nonzero, the standard is silent. pdfcer
//!   paints the complete records it got, discards the partial tail, and
//!   **discloses the discard** (`mesh_truncated`); a nonzero flag on the
//!   *first* patch is unrecoverable and is the one case that refuses
//!   (`mesh_unusable`).

use pdfcer_core::settings::MeshPatchPadding;

use crate::color::{ColorDiagnostics, ColorSpace};
use crate::gstate::Rgb;
use crate::shading::ColorRamp;

/// Ceiling on records (vertices or patches) decoded from one mesh stream.
///
/// `ARCHITECTURE.md` §10: a decoder driven by untrusted input gets an
/// output-size ceiling, not just an input one. A 4 GB stream of two-bit
/// coordinates is a legal PDF; the geometry it expands to is not bounded by
/// the file's own size in any useful way.
///
/// Chosen well above anything observed: the largest mesh in this project's
/// corpora is ~1 800 patches, and the suite's type 7 pair are 578 and 1 792.
const MAX_RECORDS: usize = 1_000_000;

/// Ceiling on triangles emitted for one mesh at one paint.
///
/// Separate from [`MAX_RECORDS`] because subdivision multiplies: a modest
/// patch count at a high zoom is where the product runs away, and the
/// record ceiling cannot see the zoom.
const MAX_TRIANGLES: usize = 4_000_000;

/// How far, in device pixels, a triangle's edge is pushed outward before
/// the coverage test - pdfcer's own choice, and the fix for a defect that is
/// worth naming because it looks like something else.
///
/// # The defect
///
/// Adjacent primitives in a mesh share an edge in the SURFACE but not in
/// the approximation: two patches flatten their common boundary
/// independently, at densities chosen independently from their own device
/// sizes, so the two polylines agree to within the flattening error rather
/// than exactly. That leaves slivers a fraction of a pixel wide between
/// primitives that are mathematically adjacent, and a pixel centre landing
/// in one is painted by NEITHER neighbour.
///
/// Measured on the print-conformance suite's type 7 mesh before this
/// existed: **one** unpainted pixel in a 60x60 cell. What made it worth
/// chasing rather than tolerating is what showed through it - the suite
/// draws its failure marker UNDERNEATH the shading, so a one-pixel hole
/// renders as a black speck on a cyan field. A crack in a gradient does not
/// read as a crack; it reads as a stray mark in the artwork.
///
/// # Why a margin rather than a seam-free tessellation
///
/// A globally consistent tessellation would mean one subdivision density
/// for the whole mesh, chosen from its largest patch - which multiplies the
/// triangle count by the ratio of the largest patch to the smallest, for
/// meshes (like the suite's) where that ratio is large. The margin costs
/// three edge lengths per triangle.
///
/// # What it costs, and the cost the first draft paid by accident
///
/// The margin is applied **only to pixels nothing has painted yet**. That
/// restriction is not caution; it is the difference between fixing a defect
/// and trading it for a subtler one.
///
/// The first draft dilated unconditionally, and on the suite's own type 7
/// mesh it closed the crack and moved the cell's correlation against its
/// printed reference image the WRONG way - 0.9485 to 0.9347. The reason is
/// that primitives are painted in stream order, so an unconditional
/// dilation lets every primitive overwrite up to `CRACK_MARGIN_PX` of its
/// predecessor: every interior colour boundary in the mesh shifts a third
/// of a pixel in the direction of the stream. On a mesh whose adjacent
/// patches carry near-identical colours that is invisible; on one carrying
/// hard-edged artwork it is a systematic edge bias, and it is exactly the
/// kind of plausible-looking wrongness a crack fix should not introduce.
///
/// Restricted to unpainted pixels, the margin can only ever ADD coverage
/// where there is none: an interior edge keeps whichever primitive's exact
/// test claimed it, and a sliver between two primitives gets the colour of
/// whichever reached it first - a colour both of them carry along that
/// shared edge anyway.
///
/// The mesh's OUTER silhouette still grows by this much, because no test
/// can tell an outer edge from a shared one. 0.35 px is under half a pixel,
/// so a silhouette gains at most one pixel of fringe, and only where the
/// true edge already passes within a third of a pixel of a sample centre.
const CRACK_MARGIN_PX: f32 = 0.35;

/// Ceiling on the per-patch subdivision, in cells per side.
///
/// 64 cells per side is 8 192 triangles for one patch — past the point
/// where a further doubling can move an 8-bit pixel for any patch small
/// enough to be on screen.
const MAX_SUBDIVISION: u32 = 64;

/// Colour carried at one mesh vertex or patch corner.
///
/// # Why this is an enum rather than always-`Rgb`
///
/// `MSH14`, clause 3, and it is a **correctness** distinction rather than a
/// storage one. With a `/Function` present the file gives one parametric
/// `t` per vertex, and the standard says interpolation happens **in `t`**:
///
/// > "All linear interpolation within the triangle mesh shall be done using
/// > the `t` values. After interpolation, the results shall be passed to the
/// > function(s)…"
///
/// So the correct answer is `f(lerp(t₀, t₁))` and the wrong one is
/// `lerp(f(t₀), f(t₁))`. For a non-linear `/Function` — which is the whole
/// reason the entry exists — those differ visibly. Resolving `t` to a
/// colour at parse time would silently commit to the wrong one, and the
/// mistake would look like a slightly-off gradient rather than like a bug.
///
/// A mesh is uniformly one variant or the other; the parser never mixes
/// them. [`lerp`](Shade::lerp) still handles a mismatch rather than
/// panicking, by returning the first operand — a defensive branch that
/// should be unreachable, not a semantic.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Shade {
    /// `n` colour components, already through the shading's `/ColorSpace`
    /// and into sRGB, with **no colorants to keep** — the mesh had no
    /// `/Function` and its colour space is additive.
    Rgb(Rgb),
    /// The same, but the space **was** subtractive, so the authored ink is
    /// carried alongside the converted value.
    ///
    /// # ★★ Why both, rather than the ink alone
    ///
    /// The two are needed at different times and neither can be recovered
    /// from the other. `rgb` is what an additive page composites with, and
    /// deriving it from `cmyk` at paint time would run the conversion once
    /// per **pixel** instead of once per **vertex**. `cmyk` is what an ink
    /// page composites with, and deriving it from `rgb` is the exact round
    /// trip this variant exists to avoid — `CMYK → sRGB` is many-to-one, so
    /// the ink that comes back is not the ink that left.
    ///
    /// Carrying both also makes the sRGB path **byte-identical** to what it
    /// was before this variant existed, which is the property that let this
    /// ship without re-blessing every mesh test.
    ///
    /// # ★ Interpolation happens in BOTH, independently
    ///
    /// [`lerp`](Shade::lerp) moves `rgb` and `cmyk` separately and linearly,
    /// so a triangle's interior is *not* the conversion of the interpolated
    /// ink, nor the interpolation of converted values — each space is
    /// interpolated in itself. That is deliberate: §8.7.4.5.5 interpolates
    /// *colour*, and a mesh's colour is stated in its own `/ColorSpace`, so
    /// the ink path interpolating ink is the faithful reading. The sRGB
    /// path keeps interpolating sRGB because that is what it already did
    /// and because changing it would alter every additive mesh render for
    /// no defect.
    ///
    /// A consequence worth stating rather than discovering: on a mesh whose
    /// conversion is non-linear, the two paths differ in the interior even
    /// though they agree at every vertex. Both are defensible; the spec
    /// picks neither.
    Ink {
        /// The converted value, for an additive destination.
        rgb: Rgb,
        /// The authored colorants, for a subtractive one.
        cmyk: [f32; 4],
    },
    /// The parametric `t` of `MSH14`, still unevaluated. Resolved through
    /// [`ColorRamp::at`] — or [`ColorRamp::at_cmyk`] — **after**
    /// interpolation.
    Parametric(f32),
}

impl Shade {
    /// Interpolate between two shades, `f` running 0 → `self`, 1 → `other`.
    #[must_use]
    fn lerp(self, other: Self, f: f32) -> Self {
        match (self, other) {
            (Self::Rgb(a), Self::Rgb(b)) => Self::Rgb(Rgb {
                r: f.mul_add(b.r - a.r, a.r),
                g: f.mul_add(b.g - a.g, a.g),
                b: f.mul_add(b.b - a.b, a.b),
            }),
            (Self::Ink { rgb: ra, cmyk: ca }, Self::Ink { rgb: rb, cmyk: cb }) => Self::Ink {
                rgb: Rgb {
                    r: f.mul_add(rb.r - ra.r, ra.r),
                    g: f.mul_add(rb.g - ra.g, ra.g),
                    b: f.mul_add(rb.b - ra.b, ra.b),
                },
                cmyk: [
                    f.mul_add(cb[0] - ca[0], ca[0]),
                    f.mul_add(cb[1] - ca[1], ca[1]),
                    f.mul_add(cb[2] - ca[2], ca[2]),
                    f.mul_add(cb[3] - ca[3], ca[3]),
                ],
            },
            (Self::Parametric(a), Self::Parametric(b)) => Self::Parametric(f.mul_add(b - a, a)),
            // Unreachable by construction — a mesh is parametric or it is
            // not, decided once from `/Function`'s presence, and within the
            // non-parametric branch a mesh's colour space either yields
            // colorants for every vertex or for none (see
            // [`MeshColorants`]). Kept total rather than `unreachable!()`
            // because a panic in a renderer reached from a malformed file is
            // a worse failure than a wrong pixel, and this branch cannot
            // produce a wrong pixel without the parser already having
            // produced a mixed mesh.
            (a, _) => a,
        }
    }

    /// Resolve to sRGB, consulting `ramp` for the parametric form.
    ///
    /// Returns `None` where the ramp has a hole (the `/Function` failed at
    /// that sample) or where a parametric mesh reached paint with no ramp
    /// at all. Both leave the pixel unpainted, which is the same treatment
    /// [`crate::shading`] gives a ramp hole in an analytic shading.
    #[must_use]
    fn resolve(self, ramp: Option<&ColorRamp>) -> Option<Rgb> {
        match self {
            Self::Rgb(c) | Self::Ink { rgb: c, .. } => Some(c),
            Self::Parametric(t) => ramp?.at(t),
        }
    }

    /// Resolve to **authored colorants**, the ink twin of [`Self::resolve`].
    ///
    /// # ★ `None` here must never be answered by converting
    ///
    /// A caller that gets `None` has to paint through [`Self::resolve`] and
    /// let the sRGB bridge do its work — it must **not** convert the sRGB
    /// value back to ink and pretend that is the authored value. The whole
    /// reason this method exists is that the converted value cannot answer
    /// the question, and a fallback that converts would reintroduce exactly
    /// the round trip the ink path removes, while making the counter that
    /// measures the round trip read zero.
    ///
    /// This is the same contract [`ColorRamp::at_cmyk`] states, for the same
    /// reason, and the two are worded alike deliberately.
    #[must_use]
    fn resolve_cmyk(self, ramp: Option<&ColorRamp>) -> Option<[f32; 4]> {
        match self {
            Self::Ink { cmyk, .. } => Some(cmyk),
            Self::Parametric(t) => ramp?.at_cmyk(t),
            Self::Rgb(_) => None,
        }
    }
}

/// Where a mesh's ink comes from, or that it has none.
///
/// # Why this is a field on [`Mesh`] and not recomputed at paint time
///
/// Deciding it once, at parse, is what makes it **all-or-nothing** — the
/// same discipline [`ColorRamp`] applies to its own colorant samples, and
/// for the same reason: a mesh that yielded ink for some vertices and not
/// others would composite part of its area natively and part through the
/// bridge, and the two do not agree, so the boundary would show as a seam
/// no file asked for. One flag for the whole mesh cannot produce a seam.
///
/// It also keeps the paint-time test `O(1)`. Walking every vertex on every
/// paint would be `O(n)` on geometry that can run to a megabyte, once per
/// frame, to answer a question whose answer never changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeshColorants {
    /// No ink: an additive colour space with nothing to preserve. Painting
    /// must go through the sRGB bridge.
    None,
    /// Every vertex carries authored colorants ([`Shade::Ink`]).
    Vertex,
    /// The mesh is parametric, so its ink — if any — lives in the
    /// [`ColorRamp`], which is not known at parse time. **A caller must
    /// still check [`ColorRamp::has_colorants`]**; this variant says only
    /// that the question is the ramp's to answer.
    Parametric,
}

/// One triangle of a type 4 or type 5 mesh, in the shading's **target
/// coordinate space** (`MSH2` — pattern space under a `PatternType 2`
/// pattern, current user space under `sh`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Triangle {
    /// The three vertices, in the order the state machine emitted them.
    ///
    /// Winding is **not** normalised and must not be: nothing in
    /// §8.7.4.5.5 gives a triangle a facing, the fill test below is
    /// orientation-agnostic, and "fixing" the winding would discard the
    /// stream order that `MSH19`'s relabelling depends on.
    pub xy: [[f32; 2]; 3],
    /// Colour at each vertex, positionally matching [`Self::xy`].
    pub shade: [Shade; 3],
}

/// One patch of a type 6 or type 7 mesh — **always sixteen control points**.
///
/// A type 6 (Coons) patch is stored here after its four internal points
/// have been derived by `MSH30`'s equations, so that one evaluator serves
/// both types. See the module docs, trap 1.
///
/// # The index convention, which is the easiest thing here to get wrong
///
/// `p[i][j]` is the control point in **column `i`, row `j`** — `i` is the
/// `u` index and `j` is the `v` index (`MSH28`). ISO 32000-1 prints the
/// array with row `j = 3` on **top**:
///
/// ```text
///     p03 p13 p23 p33          <- v = 1 edge
///     p02 p12 p22 p32
///     p01 p11 p21 p31
///     p00 p10 p20 p30          <- v = 0 edge
///      ^                ^
///      u = 0 edge       u = 1 edge
/// ```
///
/// A reader who applies the usual matrix convention (`p[row][col]`)
/// **transposes the patch**. The surface still renders and is still wrong.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Patch {
    /// `p[i][j]` as `[x, y]`, `i` = column (`u`), `j` = row (`v`).
    pub p: [[[f32; 2]; 4]; 4],
    /// Corner colours in **walk-around** order: `p00`, `p03`, `p33`, `p30`
    /// — i.e. `(u,v) = (0,0)`, `(0,1)`, `(1,1)`, `(1,0)`. `MSH24`/`MSH29`.
    pub corner: [Shade; 4],
}

/// The decoded geometry of one mesh shading.
#[derive(Debug, Clone, PartialEq)]
pub enum MeshData {
    /// Types 4 and 5 — a triangle list, already resolved from edge flags
    /// (type 4, `MSH16`–`MSH19`) or from the lattice (type 5, `MSH22`).
    Triangles(Vec<Triangle>),
    /// Types 6 and 7 — patches in stream order, which is also **paint
    /// order**: `MSH33` says a later patch paints over an earlier one.
    Patches(Vec<Patch>),
}

/// A parsed mesh shading, ready to rasterise.
///
/// Held behind an [`std::sync::Arc`] on [`crate::shading::Shading`] so that cloning a
/// shading — which happens per paint on some routes — does not copy a
/// megabyte of geometry.
#[derive(Debug, Clone, PartialEq)]
pub struct Mesh {
    /// 4, 5, 6 or 7. Kept for disclosure, not for dispatch: dispatch is on
    /// [`Self::data`], which has already erased the 6-versus-7 difference.
    pub shading_type: u8,
    /// The geometry.
    pub data: MeshData,
    /// Vertex or patch records successfully decoded.
    pub records: usize,
    /// `true` when the stream ended part-way through a record and the
    /// remainder was discarded (`MSH-N2`), or when a type-5 stream did not
    /// contain a whole number of rows. Disclosed as `mesh_truncated`.
    pub truncated: bool,
    /// For type 5 only: the row count *m*, which the dictionary does not
    /// carry and which is therefore **inferred** from the stream length
    /// (`MSH21`). `None` for every other type.
    pub rows_inferred: Option<usize>,
    /// Whether this mesh's colour can reach a compositor **as ink**, and
    /// where that ink lives. Decided once, at parse — see
    /// [`MeshColorants`].
    pub colorants: MeshColorants,
}

impl Mesh {
    /// How many triangles this mesh contributes, without subdividing.
    ///
    /// Used only for disclosure and for the ceiling check; a patch mesh's
    /// real triangle count is a paint-time property of the zoom.
    #[must_use]
    pub fn primitive_count(&self) -> usize {
        match &self.data {
            MeshData::Triangles(t) => t.len(),
            MeshData::Patches(p) => p.len(),
        }
    }
}

// ===========================================================================
// BIT-LEVEL DECODING — MSH3, MSH4, MSH5, MSH6, MSH11, MSH12, MSH-A1
// ===========================================================================

/// An MSB-first bit reader over a decoded mesh stream.
///
/// `MSH3`, verbatim: the data for each vertex is read *"in sequence from
/// higher-order to lower-order bit positions"*. So bit 7 of byte 0 is the
/// first bit of the stream, fields are big-endian within themselves, and a
/// field may straddle a byte boundary without changing that rule.
///
/// Written as a plain loop rather than a word-at-a-time shift because the
/// legal widths are `1, 2, 4, 8, 12, 16, 24, 32` (`MSH4`) — several of
/// which are not byte multiples — and a word-at-a-time reader has to
/// special-case exactly those. At the volumes involved (a 188 kB stream is
/// ~1.5 M bit reads) the loop is not the cost.
struct BitReader<'a> {
    data: &'a [u8],
    /// Position in **bits** from the start of `data`.
    pos: usize,
}

impl<'a> BitReader<'a> {
    const fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    /// Bits still unread.
    const fn remaining(&self) -> usize {
        (self.data.len() * 8).saturating_sub(self.pos)
    }

    /// Read `bits` bits as an unsigned integer, MSB first.
    ///
    /// Returns `None` when fewer than `bits` bits remain — which is how the
    /// callers detect the partial tail `MSH-N2` leaves undefined.
    fn read(&mut self, bits: u32) -> Option<u64> {
        if bits == 0 || bits > 32 || self.remaining() < bits as usize {
            return None;
        }
        let mut value: u64 = 0;
        for _ in 0..bits {
            let byte = self.data[self.pos >> 3];
            let bit = (byte >> (7 - (self.pos & 7))) & 1;
            value = (value << 1) | u64::from(bit);
            self.pos += 1;
        }
        Some(value)
    }

    /// Advance to the next byte boundary, discarding the pad bits.
    ///
    /// `MSH12`, verbatim: *"the last data byte for each vertex is padded at
    /// the end with extra bits, **which shall be ignored**."* Ignored, not
    /// zero — a writer may put anything there and a reader may not check.
    const fn align(&mut self) {
        self.pos = self.pos.div_ceil(8) * 8;
    }
}

/// Map a raw field to its decoded value (`MSH11`).
///
/// > `y = Dmin + x × (Dmax − Dmin) / (2ⁿ − 1)`
///
/// `Dmin > Dmax` is legal — the map simply takes a negative slope, exactly
/// as it does for an image `/Decode` (§8.9.5.2). The `n` used here is the
/// width of *this* field: `BitsPerCoordinate` for `x`/`y`,
/// `BitsPerComponent` for a colour component. **They are different values
/// in the same record**, and using one for both is the classic parse bug
/// this signature is shaped to make hard.
fn decode_field(raw: u64, bits: u32, range: [f32; 2]) -> f32 {
    // `1 << 32` would overflow a u32 and `(1u64 << bits) - 1` is exact for
    // every legal width, including 32.
    let max = (1u64 << bits) - 1;
    #[allow(clippy::cast_precision_loss)]
    let frac = raw as f64 / max as f64;
    let lo = f64::from(range[0]);
    let hi = f64::from(range[1]);
    #[allow(clippy::cast_possible_truncation)]
    {
        frac.mul_add(hi - lo, lo) as f32
    }
}

/// The stream-format parameters common to all four mesh types.
///
/// Built once per shading and validated on the way in, so that no later
/// code has to ask whether a width is legal.
#[derive(Debug, Clone)]
struct Params {
    /// `BitsPerCoordinate` — `MSH4`.
    bpco: u32,
    /// `BitsPerComponent` — `MSH5`. **Note the legal set differs from
    /// `bpco`**: no 24, no 32.
    bpcp: u32,
    /// `BitsPerFlag` — `MSH6`. Absent on type 5 (`MSH7`), where it is
    /// stored as 0 and never read.
    bpf: u32,
    /// `/Decode`, unpacked as `[x, y, c1, … cn]` ranges — `MSH10`.
    decode: Vec<[f32; 2]>,
    /// Colour fields per vertex or corner: the colour space's component
    /// count, or **1** when `/Function` is present (`MSH14` clause 1).
    ncomp: usize,
    /// Whether the colour fields are a parametric `t` rather than
    /// components.
    parametric: bool,
    /// How a type 6/7 patch record is padded — `MSH-A1`, the ambiguity.
    patch_padding: MeshPatchPadding,
}

/// Everything a vertex colour converts THROUGH: the shading's space, the
/// page's bridges for it, and the fixed CMYK→sRGB table (`Pass 243.0`).
///
/// One struct rather than three parameters because the three are one
/// dependency — a bridge built for a different space or intent is a valid
/// object that answers the wrong question (R237's shape) — and because
/// threading them separately pushed the per-type parsers past clippy's
/// argument ceiling, which is the lint doing its job.
#[derive(Clone, Copy)]
struct ShadeContext<'a> {
    space: &'a ColorSpace,
    bridges: &'a crate::icc::ColorBridges,
    intent: pdfcer_core::settings::CmykIntent,
}

impl Params {
    /// Read a coordinate pair and decode it.
    fn read_point(&self, r: &mut BitReader<'_>) -> Option<[f32; 2]> {
        let x = decode_field(r.read(self.bpco)?, self.bpco, self.decode[0]);
        let y = decode_field(r.read(self.bpco)?, self.bpco, self.decode[1]);
        Some([x, y])
    }

    /// Read one vertex/corner colour and convert it.
    ///
    /// The conversion to sRGB happens **here**, once per vertex, rather
    /// than per pixel — the same trade [`ColorRamp`] makes for the analytic
    /// types, and it matters for the same reason: a `Separation` or
    /// `DeviceN` colour runs an arbitrary `/tintTransform`, and a patch mesh
    /// can have a million corners but only a few pixels each.
    ///
    /// A component the colour space refuses yields `Shade::Rgb(BLACK)`
    /// rather than a failure, because the record has already been consumed
    /// and refusing here would desynchronise the stream. The colour
    /// diagnostics carry the refusal.
    fn read_shade(
        &self,
        r: &mut BitReader<'_>,
        cx: ShadeContext<'_>,
        diag: &mut ColorDiagnostics,
        comps: &mut Vec<f32>,
    ) -> Option<Shade> {
        let ShadeContext {
            space,
            bridges,
            intent,
        } = cx;
        comps.clear();
        for i in 0..self.ncomp {
            let raw = r.read(self.bpcp)?;
            comps.push(decode_field(raw, self.bpcp, self.decode[2 + i]));
        }
        if self.parametric {
            return Some(Shade::Parametric(comps[0]));
        }
        // Through the page's bridges (`Pass 243.0`), so an `ICCBased` or
        // `Lab` corner converts as a fill in the same space does.
        let rgb = bridges
            .to_rgb(space, comps, intent, diag)
            .unwrap_or(Rgb::BLACK);
        // ★ Both answers come out of the SAME `comps`, in the same call, for
        // the same reason `ColorRamp::new` builds its two vectors in one
        // loop: a `/Separation` or `/DeviceN` space converts through a
        // `/tintTransform` that is allowed to be arbitrary PostScript, and
        // nothing would force two separate evaluations of it to agree.
        match bridges.to_cmyk(space, comps, diag) {
            Some(cmyk) => Some(Shade::Ink { rgb, cmyk }),
            None => Some(Shade::Rgb(rgb)),
        }
    }

    /// Bits in one type 4 or type 5 vertex record, before padding.
    const fn vertex_bits(&self, with_flag: bool) -> usize {
        let flag = if with_flag { self.bpf as usize } else { 0 };
        flag + 2 * self.bpco as usize + self.ncomp * self.bpcp as usize
    }
}

/// Why a mesh stream could not be turned into geometry at all.
///
/// Every variant is a **disclosure**, never a silent skip: the caller
/// counts it as `mesh_unusable` and names it in the shading notes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeshRefusal {
    /// The shading object was a dictionary, not a stream. `MSH1` makes
    /// that impossible for a conforming type 4–7 shading.
    NotAStream,
    /// `/Filter` failed, or the stream bytes were unreachable.
    Undecodable,
    /// `BitsPerCoordinate` outside `{1,2,4,8,12,16,24,32}` (`MSH4`).
    BadBitsPerCoordinate,
    /// `BitsPerComponent` outside `{1,2,4,8,12,16}` (`MSH5`).
    BadBitsPerComponent,
    /// `BitsPerFlag` outside `{2,4,8}` (`MSH6`), on a type that needs one.
    BadBitsPerFlag,
    /// `/Decode` absent, or not `4 + 2n` numbers (`MSH10`).
    BadDecode,
    /// `VerticesPerRow` absent or below 2 (type 5, Table 83).
    BadVerticesPerRow,
    /// An `Indexed` `/ColorSpace` — `MSH-A4`. Interpolating palette
    /// **indices** is what a literal reading asks for and is almost
    /// certainly not what any file intends.
    IndexedColorSpace,
    /// The first patch of a type 6/7 mesh carried a nonzero edge flag, so
    /// there is no previous patch for it to inherit from (`MSH-N2`).
    FirstPatchNotNew,
    /// Not one complete record in the stream. For type 6/7 this is
    /// §8.7.4.5.7's *"At least one complete patch shall be specified"*
    /// (`MSH34`); for type 4 it is `MSH20`'s sole "an error occurs".
    NoCompleteRecord,
}

impl MeshRefusal {
    /// A one-line reason, for the shading note list.
    #[must_use]
    pub const fn reason(self) -> &'static str {
        match self {
            Self::NotAStream => "mesh shading (type 4-7) is not a stream; §8.7.4.5.5 requires one",
            Self::Undecodable => "mesh shading stream would not decode through its /Filter",
            Self::BadBitsPerCoordinate => {
                "mesh /BitsPerCoordinate is not one of 1, 2, 4, 8, 12, 16, 24, 32"
            }
            Self::BadBitsPerComponent => "mesh /BitsPerComponent is not one of 1, 2, 4, 8, 12, 16",
            Self::BadBitsPerFlag => "mesh /BitsPerFlag is not one of 2, 4, 8",
            Self::BadDecode => "mesh /Decode is absent or is not 4 + 2n numbers (Table 82)",
            Self::BadVerticesPerRow => {
                "type 5 mesh /VerticesPerRow is absent or below 2 (Table 83)"
            }
            Self::IndexedColorSpace => {
                "mesh /ColorSpace is Indexed; interpolating palette indices is refused rather than guessed (MSH-A4)"
            }
            Self::FirstPatchNotNew => {
                "first patch of a type 6/7 mesh has a nonzero edge flag, so it has no previous patch to inherit from"
            }
            Self::NoCompleteRecord => {
                "mesh stream contains no complete record, so nothing can be painted"
            }
        }
    }
}

/// Everything the parser needs that lives outside the stream.
pub struct ParseInput<'a> {
    /// `ShadingType`, 4–7.
    pub shading_type: u8,
    /// The decoded stream bytes — already through `/Filter`.
    pub data: &'a [u8],
    /// `/Decode`, raw. Validated here rather than by the caller so the
    /// arity rule (`MSH10`: **one** `c` pair when `/Function` is present)
    /// is checked in the same place it is used.
    pub decode: Option<&'a [f32]>,
    /// `/BitsPerCoordinate`.
    pub bits_per_coordinate: Option<u32>,
    /// `/BitsPerComponent`.
    pub bits_per_component: Option<u32>,
    /// `/BitsPerFlag`. Ignored for type 5 (`MSH7`).
    pub bits_per_flag: Option<u32>,
    /// `/VerticesPerRow`. Type 5 only.
    pub vertices_per_row: Option<u32>,
    /// The shading's colour space.
    pub space: &'a ColorSpace,
    /// The page's colour bridges for that space (`Pass 243.0`) — what a
    /// per-vertex colour converts through, so a mesh corner and a fill of
    /// one colour take one route. [`crate::icc::ColorBridges::none`] when
    /// the caller has none.
    pub bridges: &'a crate::icc::ColorBridges,
    /// Whether `/Function` is present — the switch `MSH14` turns.
    pub parametric: bool,
    /// Which `MSH-A1` reading is in force.
    pub patch_padding: MeshPatchPadding,
    /// Which fixed `DeviceCMYK` → sRGB table to use for corner colours.
    pub intent: pdfcer_core::settings::CmykIntent,
}

/// Decode a mesh stream into geometry, or say why not.
///
/// # Errors
///
/// [`MeshRefusal`] — every variant is a disclosure the caller must count
/// and name. There is no silent failure path.
pub fn parse(input: &ParseInput<'_>, diag: &mut ColorDiagnostics) -> Result<Mesh, MeshRefusal> {
    // MSH-A4, taken before anything else because it is a property of the
    // dictionary rather than of the stream, and because parsing first would
    // spend the work only to throw it away.
    if matches!(input.space, ColorSpace::Indexed { .. }) {
        return Err(MeshRefusal::IndexedColorSpace);
    }

    let bpco = input
        .bits_per_coordinate
        .filter(|b| matches!(b, 1 | 2 | 4 | 8 | 12 | 16 | 24 | 32))
        .ok_or(MeshRefusal::BadBitsPerCoordinate)?;
    let bpcp = input
        .bits_per_component
        .filter(|b| matches!(b, 1 | 2 | 4 | 8 | 12 | 16))
        .ok_or(MeshRefusal::BadBitsPerComponent)?;
    // MSH7: type 5 has no flag field at all, and Table 83 does not list the
    // key. A type-5 file that carries one anyway is not thereby malformed —
    // the entry is simply not part of that type's dictionary — so it is
    // ignored rather than validated.
    let bpf = if input.shading_type == 5 {
        0
    } else {
        input
            .bits_per_flag
            .filter(|b| matches!(b, 2 | 4 | 8))
            .ok_or(MeshRefusal::BadBitsPerFlag)?
    };

    // MSH14 clause 1: with /Function each record carries ONE colour field.
    let ncomp = if input.parametric {
        1
    } else {
        input.space.components()
    };
    // MSH10: `[xmin xmax ymin ymax c1,min c1,max … cn,min cn,max]`, and
    // MSH14 clause 2 shrinks it to six numbers when /Function is present.
    let raw_decode = input.decode.ok_or(MeshRefusal::BadDecode)?;
    if ncomp == 0 || raw_decode.len() < 4 + 2 * ncomp {
        return Err(MeshRefusal::BadDecode);
    }
    let decode: Vec<[f32; 2]> = (0..2 + ncomp)
        .map(|i| [raw_decode[2 * i], raw_decode[2 * i + 1]])
        .collect();

    let params = Params {
        bpco,
        bpcp,
        bpf,
        decode,
        ncomp,
        parametric: input.parametric,
        patch_padding: input.patch_padding,
    };

    let mut reader = BitReader::new(input.data);
    let mut truncated = false;
    let mut rows_inferred = None;

    let cx = ShadeContext {
        space: input.space,
        bridges: input.bridges,
        intent: input.intent,
    };
    let data = match input.shading_type {
        4 => MeshData::Triangles(parse_type4(&mut reader, &params, cx, diag, &mut truncated)?),
        5 => {
            let k = input
                .vertices_per_row
                .filter(|v| *v >= 2)
                .ok_or(MeshRefusal::BadVerticesPerRow)? as usize;
            let (tris, rows) = parse_type5(&mut reader, &params, k, cx, diag, &mut truncated)?;
            rows_inferred = Some(rows);
            MeshData::Triangles(tris)
        }
        6 | 7 => MeshData::Patches(parse_patches(
            &mut reader,
            &params,
            input.shading_type == 7,
            cx,
            diag,
            &mut truncated,
        )?),
        _ => return Err(MeshRefusal::NoCompleteRecord),
    };

    // Triangles for types 4/5, patches for types 6/7 -- the unit the
    // operator's question is about ("how much geometry did you get out of
    // this stream?"), not the unit the stream is written in.
    let records = match &data {
        MeshData::Triangles(t) => t.len(),
        MeshData::Patches(p) => p.len(),
    };

    let colorants = classify_colorants(&data_colorant_census(&data));
    Ok(Mesh {
        shading_type: input.shading_type,
        data,
        records,
        truncated,
        rows_inferred,
        colorants,
    })
}

/// Census of what the decoded shades actually are: `(ink, rgb, parametric)`.
///
/// Counted rather than sampled. A mesh whose first vertex converts and whose
/// thousandth does not is exactly the case [`MeshColorants`] exists to make
/// impossible, and a first-vertex probe would miss it while looking rigorous.
fn data_colorant_census(data: &MeshData) -> (usize, usize, usize) {
    let mut census = (0usize, 0usize, 0usize);
    let mut count = |s: &Shade| match s {
        Shade::Ink { .. } => census.0 += 1,
        Shade::Rgb(_) => census.1 += 1,
        Shade::Parametric(_) => census.2 += 1,
    };
    match data {
        MeshData::Triangles(tris) => {
            for t in tris {
                for s in &t.shade {
                    count(s);
                }
            }
        }
        MeshData::Patches(patches) => {
            for p in patches {
                for s in &p.corner {
                    count(s);
                }
            }
        }
    }
    census
}

/// Turn the census into a verdict, **all-or-nothing**.
///
/// Any `Rgb` at all demotes the whole mesh to [`MeshColorants::None`], even
/// beside a thousand `Ink` vertices. That is the seam argument in
/// [`MeshColorants`]: half a mesh painted natively and half bridged do not
/// meet, and a visible boundary inside one object is a worse outcome than
/// the whole object taking the conversion every other renderer takes.
///
/// An empty mesh is [`MeshColorants::None`] — there is nothing to paint, so
/// the answer is the one that promises least.
const fn classify_colorants((ink, rgb, parametric): &(usize, usize, usize)) -> MeshColorants {
    if *parametric > 0 {
        return MeshColorants::Parametric;
    }
    if *ink > 0 && *rgb == 0 {
        return MeshColorants::Vertex;
    }
    MeshColorants::None
}

/// Type 4 — free-form Gouraud-shaded triangle mesh (§8.7.4.5.5).
///
/// Implements the state machine of `MSH16`–`MSH19` exactly as the RAG
/// derives it. The relabelling after a nonzero flag is **not** a guess: the
/// clause fixes two invariants — stream order (`va` before `vb` before
/// `vc`) and *"side `vab` is assumed to be shared with a preceding
/// triangle"* — and only one assignment satisfies both.
///
/// ```text
/// f = 0  read two MORE vertices, whose own flags are read and DISCARDED
///        (MSH16: "ignored", not absent — a parser that skips their flag
///         field desynchronises the whole stream)
/// f = 1  new triangle (vb, vc, vd), sharing side vbc
/// f = 2  new triangle (va, vc, vd), sharing side vac
/// f = 3  not legal for type 4 (MSH8) — treated as the end of usable data
/// ```
fn parse_type4(
    r: &mut BitReader<'_>,
    p: &Params,
    cx: ShadeContext<'_>,
    diag: &mut ColorDiagnostics,
    truncated: &mut bool,
) -> Result<Vec<Triangle>, MeshRefusal> {
    let record_bits = p.vertex_bits(true);
    let mut comps = Vec::new();
    let mut tris: Vec<Triangle> = Vec::new();
    // The rolling (va, vb, vc) of MSH19.
    let mut state: Option<[([f32; 2], Shade); 3]> = None;

    let mut read_vertex =
        |r: &mut BitReader<'_>, diag: &mut ColorDiagnostics| -> Option<(u8, [f32; 2], Shade)> {
            let flag = r.read(p.bpf)? as u8 & 3;
            let xy = p.read_point(r)?;
            let shade = p.read_shade(r, cx, diag, &mut comps)?;
            // MSH12: the padded unit for types 4 and 5 is exactly one vertex,
            // and this is normative rather than inferred.
            r.align();
            Some((flag, xy, shade))
        };

    while r.remaining() >= record_bits && tris.len() < MAX_TRIANGLES {
        let Some((flag, xy, shade)) = read_vertex(r, diag) else {
            *truncated = true;
            break;
        };
        match flag {
            0 => {
                // MSH16: at least two more vertices follow, and their edge
                // flags are ignored. If they are not both there the stream
                // ends on an incomplete triangle — MSH20's sole "an error
                // occurs", which MSH-N2 makes a product decision: keep what
                // completed, discard the tail, disclose the discard.
                let Some((_, xy2, s2)) = read_vertex(r, diag) else {
                    *truncated = true;
                    break;
                };
                let Some((_, xy3, s3)) = read_vertex(r, diag) else {
                    *truncated = true;
                    break;
                };
                let tri = [(xy, shade), (xy2, s2), (xy3, s3)];
                tris.push(triangle_of(tri));
                state = Some(tri);
            }
            1 | 2 => {
                let Some([a, b, c]) = state else {
                    // A nonzero flag with no preceding triangle. Nothing to
                    // share an edge with; the record is consumed and
                    // dropped rather than guessed at.
                    *truncated = true;
                    continue;
                };
                let next = if flag == 1 {
                    [b, c, (xy, shade)]
                } else {
                    [a, c, (xy, shade)]
                };
                tris.push(triangle_of(next));
                state = Some(next);
            }
            // MSH8: type 4's legal flags are 0, 1, 2 only. A 3 means the
            // stream is not what it claims to be; stop rather than invent.
            _ => {
                *truncated = true;
                break;
            }
        }
    }

    if r.remaining() > 0 && r.remaining() < record_bits {
        *truncated = true;
    }
    if tris.is_empty() {
        return Err(MeshRefusal::NoCompleteRecord);
    }
    Ok(tris)
}

/// Assemble a [`Triangle`] from three `(point, shade)` pairs.
fn triangle_of(v: [([f32; 2], Shade); 3]) -> Triangle {
    Triangle {
        xy: [v[0].0, v[1].0, v[2].0],
        shade: [v[0].1, v[1].1, v[2].1],
    }
}

/// Type 5 — lattice-form Gouraud-shaded triangle mesh (§8.7.4.5.6).
///
/// `MSH21`/`MSH22`. Two facts make this the type most often mis-parsed:
///
/// * **There is no edge flag** (`MSH7`), so a record that would have been
///   byte-aligned as a type 4 record often is not as a type 5 one — and
///   the per-vertex padding rule (`MSH12`) still applies.
/// * **The row count *m* is not in the dictionary.** It is inferred from
///   the stream length, and pdfcer discloses the inference and any partial
///   final row rather than silently rounding.
///
/// Returns the triangles and the inferred *m*.
fn parse_type5(
    r: &mut BitReader<'_>,
    p: &Params,
    k: usize,
    cx: ShadeContext<'_>,
    diag: &mut ColorDiagnostics,
    truncated: &mut bool,
) -> Result<(Vec<Triangle>, usize), MeshRefusal> {
    let record_bits = p.vertex_bits(false);
    let mut comps = Vec::new();
    let mut verts: Vec<([f32; 2], Shade)> = Vec::new();

    while r.remaining() >= record_bits && verts.len() < MAX_RECORDS {
        let Some(xy) = p.read_point(r) else {
            *truncated = true;
            break;
        };
        let Some(shade) = p.read_shade(r, cx, diag, &mut comps) else {
            *truncated = true;
            break;
        };
        r.align();
        verts.push((xy, shade));
    }
    if r.remaining() > 0 && r.remaining() < record_bits {
        *truncated = true;
    }

    let m = verts.len() / k;
    if !verts.len().is_multiple_of(k) {
        // A partial final row. MSH-N2 leaves this undefined; pdfcer keeps
        // the whole rows and discloses the remainder.
        *truncated = true;
    }
    if m < 2 {
        return Err(MeshRefusal::NoCompleteRecord);
    }

    // MSH22, verbatim: for 0 ≤ i ≤ m−2 and 0 ≤ j ≤ k−2,
    //   (V_i,j , V_i,j+1 , V_i+1,j)  and  (V_i,j+1 , V_i+1,j , V_i+1,j+1)
    let mut tris = Vec::with_capacity(2 * (m - 1) * (k - 1));
    for i in 0..m - 1 {
        for j in 0..k - 1 {
            let a = verts[i * k + j];
            let b = verts[i * k + j + 1];
            let c = verts[(i + 1) * k + j];
            let d = verts[(i + 1) * k + j + 1];
            tris.push(triangle_of([a, b, c]));
            tris.push(triangle_of([b, c, d]));
        }
    }
    Ok((tris, m))
}

/// The **stream order** of a patch's boundary control points, expressed as
/// `(i, j)` tensor indices — `MSH29`.
///
/// The first twelve entries are the boundary walked counterclockwise from
/// `p00`, and they are **identical for types 6 and 7** (`MSH30`'s practical
/// form: "stream indices 1–12 of a type 7 patch are the same points, in the
/// same order, as `x1 y1 … x12 y12` of a type 6 patch"). The last four are
/// type 7's internal points, in the **cycle** `p11 → p12 → p22 → p21` —
/// *not* row-major, and that is the type-7-specific transposition trap.
const PATCH_ORDER: [(usize, usize); 16] = [
    (0, 0), // p00   x1
    (0, 1), // p01   x2
    (0, 2), // p02   x3
    (0, 3), // p03   x4
    (1, 3), // p13   x5
    (2, 3), // p23   x6
    (3, 3), // p33   x7
    (3, 2), // p32   x8
    (3, 1), // p31   x9
    (3, 0), // p30   x10
    (2, 0), // p20   x11
    (1, 0), // p10   x12
    (1, 1), // p11   internal
    (1, 2), // p12   internal
    (2, 2), // p22   internal
    (2, 1), // p21   internal
];

/// Types 6 and 7 — Coons and tensor-product patch meshes (§8.7.4.5.7/.8).
///
/// One parser for both. The only differences are the number of control
/// points in the stream (12 versus 16 on a new patch, 8 versus 12 on a
/// continued one) and whether the four internal points are read or derived.
///
/// # The inheritance rule, read structurally (Tables 85 and 86)
///
/// Tables 85/86 print three separate rows for `f = 1, 2, 3`, which reads as
/// three rules. It is **one** rule: every nonzero flag reuses one of the
/// previous patch's three *available* boundary curves as the new patch's
/// `u = 0` edge, together with the two corner colours at that curve's ends.
///
/// | `f` | previous edge reused | new `p00 p01 p02 p03` | new `c00`, `c03` |
/// |---|---|---|---|
/// | 1 | previous `v = 1` row | `p03 p13 p23 p33` | prev `c03`, prev `c33` |
/// | 2 | previous `u = 1` column, reversed | `p33 p32 p31 p30` | prev `c33`, prev `c30` |
/// | 3 | previous `v = 0` row, reversed | `p30 p20 p10 p00` | prev `c30`, prev `c00` |
///
/// The previous patch's own `u = 0` edge is **not** available — it is
/// already attached to the patch before it. That is the exact analogue of
/// type 4's *"side `vab` … is not available for continuing the mesh"*
/// (`MSH18`): same design, one dimension up.
///
/// **The four internal points are never inherited.** Every continued type 7
/// record supplies `p11 p12 p22 p21` explicitly, which is why the read
/// count is 12 pairs and not 8.
#[allow(clippy::too_many_lines)]
fn parse_patches(
    r: &mut BitReader<'_>,
    p: &Params,
    tensor: bool,
    cx: ShadeContext<'_>,
    diag: &mut ColorDiagnostics,
    truncated: &mut bool,
) -> Result<Vec<Patch>, MeshRefusal> {
    let mut comps = Vec::new();
    let mut patches: Vec<Patch> = Vec::new();
    // Stream-order scratch: the 16 tensor points, filled by PATCH_ORDER.
    let mut prev: Option<Patch> = None;

    loop {
        if patches.len() >= MAX_RECORDS {
            *truncated = true;
            break;
        }
        let Some(flag) = r.read(p.bpf) else {
            break;
        };
        let flag = flag as u8 & 3;
        if flag != 0 && prev.is_none() {
            // MSH-N2: unrecoverable, and the only patch case that refuses.
            return Err(if patches.is_empty() {
                MeshRefusal::FirstPatchNotNew
            } else {
                MeshRefusal::NoCompleteRecord
            });
        }

        // How many boundary points this record carries. A new patch gives
        // all of them; a continued one omits the four that are inherited.
        let read_from = if flag == 0 { 0 } else { 4 };
        let read_to = if tensor { 16 } else { 12 };

        let mut pts = [[0.0f32; 2]; 16];
        let mut ok = true;
        for &(i, j) in &PATCH_ORDER[read_from..read_to] {
            let Some(xy) = p.read_point(r) else {
                ok = false;
                break;
            };
            pts[i * 4 + j] = xy;
        }
        if !ok {
            *truncated = true;
            break;
        }

        // MSH24/MSH29: corner colours in walk-around order — c00, c03, c33,
        // c30. A continued patch supplies only the far two (c33, c30) and
        // inherits the near two.
        let mut corner = [Shade::Rgb(Rgb::BLACK); 4];
        let first_colour = if flag == 0 { 0 } else { 2 };
        for slot in corner.iter_mut().skip(first_colour) {
            let Some(s) = p.read_shade(r, cx, diag, &mut comps) else {
                ok = false;
                break;
            };
            *slot = s;
        }
        if !ok {
            *truncated = true;
            break;
        }

        // MSH-A1 — the genuine ambiguity, and the only place this setting
        // is observable. §8.7.4.5.5's padding rule is scoped to a VERTEX
        // and a patch has no vertices; §8.7.4.5.7/.8 import the rule
        // without redefining its unit.
        if p.patch_padding == MeshPatchPadding::PerRecord {
            r.align();
        }

        // Fill the inherited quarter, in tensor indices.
        if flag != 0 {
            let prev_patch = prev.expect("checked above");
            let src: [(usize, usize); 4] = match flag {
                1 => [(0, 3), (1, 3), (2, 3), (3, 3)],
                2 => [(3, 3), (3, 2), (3, 1), (3, 0)],
                _ => [(3, 0), (2, 0), (1, 0), (0, 0)],
            };
            for (slot, (i, j)) in src.into_iter().enumerate() {
                pts[slot] = prev_patch.p[i][j];
            }
            let (c0, c1) = match flag {
                1 => (prev_patch.corner[1], prev_patch.corner[2]),
                2 => (prev_patch.corner[2], prev_patch.corner[3]),
                _ => (prev_patch.corner[3], prev_patch.corner[0]),
            };
            corner[0] = c0;
            corner[1] = c1;
        }

        // `pts` is indexed `i * 4 + j`, but the inherited quarter above was
        // written to slots 0..4 in PATCH_ORDER terms, which are (0,0),
        // (0,1), (0,2), (0,3) — i.e. `i * 4 + j` = 0, 1, 2, 3. The two
        // agree because the u = 0 column is contiguous in this layout; the
        // assertion below is what keeps that from being a coincidence a
        // future edit can break.
        debug_assert_eq!(PATCH_ORDER[0], (0, 0));
        debug_assert_eq!(PATCH_ORDER[3], (0, 3));

        let mut grid = [[[0.0f32; 2]; 4]; 4];
        for (i, col) in grid.iter_mut().enumerate() {
            for (j, cell) in col.iter_mut().enumerate() {
                *cell = pts[i * 4 + j];
            }
        }
        if !tensor {
            // MSH30 — a Coons patch IS a tensor patch whose four internal
            // points are implied by its boundary.
            coons_internals(&mut grid);
        }

        let patch = Patch { p: grid, corner };
        patches.push(patch);
        prev = Some(patch);
    }

    // Bits left over that are not a whole record. MSH-N2 leaves this
    // undefined for types 6/7 (the "an error occurs" sentence is type 4's
    // alone), so pdfcer keeps what completed and says that it did.
    if r.remaining() > 0 {
        *truncated = true;
    }

    if patches.is_empty() {
        // MSH34: "At least one complete patch shall be specified."
        return Err(MeshRefusal::NoCompleteRecord);
    }
    Ok(patches)
}

/// Derive a Coons patch's four internal control points from its boundary
/// (`MSH30`, §8.7.4.5.8's four `1/9 (…)` equations).
///
/// ```text
/// p11 = 1/9 [ −4·p00 + 6·(p01 + p10) − 2·(p03 + p30) + 3·(p31 + p13) − p33 ]
/// p12 = 1/9 [ −4·p03 + 6·(p02 + p13) − 2·(p00 + p33) + 3·(p32 + p10) − p30 ]
/// p21 = 1/9 [ −4·p30 + 6·(p31 + p20) − 2·(p33 + p00) + 3·(p01 + p23) − p03 ]
/// p22 = 1/9 [ −4·p33 + 6·(p32 + p23) − 2·(p30 + p03) + 3·(p02 + p20) − p00 ]
/// ```
///
/// Each is the same template rotated by one corner, and the coefficients
/// sum to `−4 + 12 − 4 + 6 − 1 = 9`, so the operator is **affine** — a
/// partition of unity. That gives a free unit-test invariant: feed it a
/// planar patch and the internals must land in the same plane. See
/// `coons_internals_are_affine_so_a_planar_patch_stays_planar`.
fn coons_internals(g: &mut [[[f32; 2]; 4]; 4]) {
    // Read the twelve boundary points once, by name, so the four equations
    // below read like the spec's own rather than like array arithmetic.
    let p00 = g[0][0];
    let p01 = g[0][1];
    let p02 = g[0][2];
    let p03 = g[0][3];
    let p13 = g[1][3];
    let p23 = g[2][3];
    let p33 = g[3][3];
    let p32 = g[3][2];
    let p31 = g[3][1];
    let p30 = g[3][0];
    let p20 = g[2][0];
    let p10 = g[1][0];

    // One closure for all four equations, because they ARE one equation
    // rotated by a corner:
    //     -4a + 6(b + c) - 2(d + e) + 3(h + i) - j,  all over 9.
    // Writing them out separately would be four places for the same
    // transcription error to hide in.
    let f = |a: [f32; 2],
             b: [f32; 2],
             c: [f32; 2],
             d: [f32; 2],
             e: [f32; 2],
             h: [f32; 2],
             i: [f32; 2],
             j: [f32; 2]|
     -> [f32; 2] {
        let mut out = [0.0f32; 2];
        for t in 0..2 {
            out[t] = (-4.0 * a[t] + 6.0 * (b[t] + c[t]) - 2.0 * (d[t] + e[t])
                + 3.0 * (h[t] + i[t])
                - j[t])
                / 9.0;
        }
        out
    };

    g[1][1] = f(p00, p01, p10, p03, p30, p31, p13, p33);
    g[1][2] = f(p03, p02, p13, p00, p33, p32, p10, p30);
    g[2][1] = f(p30, p31, p20, p33, p00, p01, p23, p03);
    g[2][2] = f(p33, p32, p23, p30, p03, p02, p20, p00);
}

// ===========================================================================
// PAINTING — forward rasterisation
// ===========================================================================

/// The cubic Bernstein basis (`MSH28`).
///
/// ```text
/// B0(t) = (1 − t)³   B1(t) = 3t(1 − t)²   B2(t) = 3t²(1 − t)   B3(t) = t³
/// ```
fn bernstein(t: f32) -> [f32; 4] {
    let s = 1.0 - t;
    [s * s * s, 3.0 * t * s * s, 3.0 * t * t * s, t * t * t]
}

impl Patch {
    /// Evaluate the tensor-product surface at `(u, v)` — `MSH28`.
    ///
    /// `S(u, v) = Σᵢ Σⱼ pᵢⱼ · Bᵢ(u) · Bⱼ(v)`, with `i` the column (`u`)
    /// index and `j` the row (`v`) index.
    #[must_use]
    fn at(&self, u: f32, v: f32) -> [f32; 2] {
        let bu = bernstein(u);
        let bv = bernstein(v);
        let mut out = [0.0f32; 2];
        for (col, bui) in self.p.iter().zip(bu) {
            for (pt, bvj) in col.iter().zip(bv) {
                let w = bui * bvj;
                out[0] = w.mul_add(pt[0], out[0]);
                out[1] = w.mul_add(pt[1], out[1]);
            }
        }
        out
    }

    /// Bilinear colour over the unit square — §8.7.4.5.7's first bullet,
    /// inherited unchanged by type 7 (`MSH31`: .8 changes only geometry).
    ///
    /// `C(u,v) = (1−u)(1−v)·c00 + (1−u)v·c03 + uv·c33 + u(1−v)·c30`
    ///
    /// with the corner order of `MSH24`. Written as two lerps of lerps so
    /// that the parametric form composes: interpolating a `t` bilinearly
    /// and then evaluating the function is exactly `MSH14`'s
    /// `f(lerp(t))`.
    #[must_use]
    fn shade_at(&self, u: f32, v: f32) -> Shade {
        // v edge at u = 0: c00 -> c03.  v edge at u = 1: c30 -> c33.
        let left = self.corner[0].lerp(self.corner[1], v);
        let right = self.corner[3].lerp(self.corner[2], v);
        left.lerp(right, u)
    }
}

/// Cells per side to subdivide one patch into — `MSH-A3`, pdfcer's own
/// choice, made from the patch's **device-space** size.
///
/// The standard gives no density and `/SM` bounds colour error rather than
/// geometric deviation, so a renderer must pick. Picking from the device
/// extent rather than from a fixed constant is what makes the error bounded
/// in the units a viewer perceives: a patch three pixels across gets three
/// cells and a patch three hundred across gets sixty-four.
///
/// The divisor is 4 device pixels per cell. A cubic curve's deviation from
/// its `n`-segment polyline falls as `O(1/n²)`, so four pixels per cell
/// puts the worst case comfortably below one pixel for any curvature a
/// patch that size can carry.
fn subdivision_for(extent: f32) -> u32 {
    if !extent.is_finite() || extent <= 0.0 {
        return 1;
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let n = (extent / 4.0).ceil() as u32;
    n.clamp(1, MAX_SUBDIVISION)
}

/// Paint a mesh into `pixmap`.
///
/// # The two transforms, and why the mesh needs the forward one
///
/// [`crate::shading::Shading::paint`] is handed `to_target`, which maps
/// **device space into the shading's target space** — the right direction
/// for an inverse-mapped analytic shading. A mesh is forward-rasterised, so
/// it needs the inverse of that, and a non-invertible transform means the
/// geometry has no area on the page and nothing is painted.
///
/// # Why this paints into a scratch and composites once
///
/// Not for speed — for correctness under a non-opaque `alpha`.
///
/// A mesh's triangles **share edges**, and patches may legitimately overlap
/// (`MSH33`) or fold over (`MSH32`). Blending each triangle straight into
/// the destination would composite the shared pixels twice, which shows up
/// as a lattice of darker seams at exactly the density of the
/// subdivision — an artefact that looks like a rasteriser bug and moves
/// when you zoom. §11.6.7 also makes a shading pattern an *implicit
/// non-isolated knockout group*, which is the same statement in
/// transparency terms: the shading composites with the page **once**, as
/// one object, however many primitives it is made of.
///
/// So: rasterise opaquely into a scratch, then composite the scratch once,
/// through `alpha` and the clip.
///
/// # Fold-over
///
/// `MSH32` resolves a fold by taking the point with the **largest `v`**,
/// then the largest `u`. Emitting cells in increasing `v` (outer) and
/// increasing `u` (inner) and painting each opaquely over the last
/// satisfies that for free — the largest-`v` cell is simply painted last.
///
/// # Returns
///
/// Device pixels written, or `None` if the scratch could not be allocated
/// — which the caller distinguishes from "painted nothing", because the
/// shading was paintable and pdfcer did not paint it.
///
/// # On the argument count
///
/// Eight, one past clippy's threshold, and allowed rather than bundled. A
/// `MeshPaintArgs` struct would be built at exactly one call site and read at
/// exactly one place, so it would add an indirection and a second name for
/// every field without removing a single opportunity to pass the wrong
/// value. The arguments are also not interchangeable: seven of the eight
/// have distinct types, so a transposition is a compile error rather than a
/// silent wrong render.
#[allow(clippy::too_many_arguments)]
pub(crate) fn paint(
    mesh: &Mesh,
    ramp: Option<&ColorRamp>,
    to_target: tiny_skia::Transform,
    bbox: Option<[f32; 4]>,
    region: (i32, i32, i32, i32),
    clip: Option<&tiny_skia::Mask>,
    alpha: f32,
    pixmap: &mut tiny_skia::Pixmap,
) -> Option<usize> {
    #[allow(clippy::cast_possible_wrap)]
    let (scratch, ox, oy) = rasterise(
        mesh,
        ramp,
        to_target,
        region,
        (pixmap.width() as i32, pixmap.height() as i32),
        false,
    )?;
    if scratch.empty {
        return Some(0);
    }
    Some(composite(
        &scratch.rgba,
        ox,
        oy,
        to_target,
        bbox,
        clip,
        alpha,
        pixmap,
    ))
}

/// Paint a mesh into a **colorant buffer**, as authored ink.
///
/// The ink twin of [`paint`], and the reason `Pass 137.1` exists: until it,
/// every mesh reached an ink page through the sRGB bridge, so a mesh and an
/// image of the same `DeviceCMYK` colour rendered differently on the same
/// page. Measured on the sheet that exposed it: mean |diff| of 24.1 and
/// 16.9 between a live type 7 mesh and its own reference image, on a page
/// where every other shading type agreed to within 3.5.
///
/// # Returns `None` — meaning "not paintable in ink", not "failed"
///
/// The caller must fall back to [`paint`] on `None`, and must **not**
/// convert. Four ways to get it, all of them legitimate document states
/// rather than errors:
///
/// - the mesh's colour space is additive ([`MeshColorants::None`]), so
///   there is no authored ink to preserve and the bridge is the honest
///   route;
/// - the mesh is parametric and its `ColorRamp` carries no colorants;
/// - the transform is not invertible, so the geometry has no area;
/// - the scratch could not be allocated.
///
/// # Why the `rules` argument, when nothing here reads Table 149 per pixel
///
/// So an overprinting mesh composites through the *same* function every
/// other ink source does. `[ComponentRule::Source; 4]` is `Blend::Normal`
/// in ink, so a non-overprinting caller passes that and gets ordinary
/// source-over — one code path, not two that have to be kept agreeing.
#[allow(clippy::too_many_arguments)]
pub(crate) fn paint_cmyk(
    mesh: &Mesh,
    ramp: Option<&ColorRamp>,
    to_target: tiny_skia::Transform,
    bbox: Option<[f32; 4]>,
    region: (i32, i32, i32, i32),
    clip: Option<&tiny_skia::Mask>,
    alpha: f32,
    rules: [crate::overprint::ComponentRule; 4],
    buf: &mut crate::cmyk_buffer::CmykBuffer,
) -> Option<usize> {
    match mesh.colorants {
        MeshColorants::None => return None,
        MeshColorants::Parametric => {
            if !ramp.is_some_and(ColorRamp::has_colorants) {
                return None;
            }
        }
        MeshColorants::Vertex => {}
    }

    #[allow(clippy::cast_possible_wrap)]
    let (scratch, ox, oy) = rasterise(
        mesh,
        ramp,
        to_target,
        region,
        (buf.width() as i32, buf.height() as i32),
        true,
    )?;
    if scratch.empty {
        return Some(0);
    }
    let ink = scratch.ink.as_ref()?;

    let sw = scratch.rgba.width() as usize;
    let pixels = scratch.rgba.pixels();
    #[allow(clippy::cast_sign_loss)]
    let (ox_u, oy_u) = (ox.max(0) as u32, oy.max(0) as u32);
    let region_u = (
        ox_u,
        oy_u,
        ox_u + scratch.rgba.width(),
        oy_u + scratch.rgba.height(),
    );
    let dst_w = buf.width() as usize;

    // ★ Everything `composite` does — /BBox, the clip, the alpha — is done
    // HERE, per pixel, inside the closure `composite_overprint_varying`
    // drives. The two composites are therefore the same three tests in the
    // same order against the same scratch; only the arithmetic that lands
    // the value differs, and that arithmetic is the buffer's, not this
    // module's.
    let changed = buf.composite_overprint_varying(region_u, rules, alpha, |x, y| {
        let (sx, sy) = ((x - ox_u) as usize, (y - oy_u) as usize);
        let idx = sy * sw + sx;
        // `rgba`'s alpha is the sole occupancy authority — see [`Scratch`].
        if pixels.get(idx)?.alpha() == 0 {
            return None;
        }
        if let Some([bx0, by0, bx1, by1]) = bbox {
            #[allow(clippy::cast_precision_loss)]
            let mut pt = tiny_skia::Point::from_xy(x as f32 + 0.5, y as f32 + 0.5);
            to_target.map_point(&mut pt);
            if pt.x < bx0.min(bx1)
                || pt.x > bx0.max(bx1)
                || pt.y < by0.min(by1)
                || pt.y > by0.max(by1)
            {
                return None;
            }
        }
        let coverage = match clip {
            Some(mask) => f32::from(*mask.data().get(y as usize * dst_w + x as usize)?) / 255.0,
            None => 1.0,
        };
        if coverage <= 0.0 {
            return None;
        }
        Some((*ink.get(idx)?, coverage))
    });
    Some(changed as usize)
}

/// Rasterise the mesh into a [`Scratch`], returning it with its device
/// origin.
///
/// Shared verbatim by [`paint`] and [`paint_cmyk`] — **the geometry is not
/// duplicated for the two destinations**, which is the property that stops
/// an ink render and an sRGB render of the same mesh disagreeing about
/// where the mesh *is* as well as what colour it is. Only the composite
/// step differs.
///
/// Returns `None` if the transform is not invertible (the geometry has no
/// area on the page) or the scratch could not be allocated. A zero-width
/// scratch means the clipped region was empty, which is "painted nothing"
/// rather than a failure and is reported as such by both callers.
#[allow(clippy::cast_possible_wrap, clippy::cast_sign_loss)]
fn rasterise(
    mesh: &Mesh,
    ramp: Option<&ColorRamp>,
    to_target: tiny_skia::Transform,
    region: (i32, i32, i32, i32),
    dst: (i32, i32),
    want_ink: bool,
) -> Option<(Scratch, i32, i32)> {
    let to_device = to_target.invert()?;

    // Clamp the paint area to the destination. Everything below is in
    // device pixels relative to (ox, oy).
    let (x_lo, y_lo, x_hi, y_hi) = region;
    let ox = x_lo.max(0);
    let oy = y_lo.max(0);
    let x_hi = x_hi.min(dst.0);
    let y_hi = y_hi.min(dst.1);
    if x_hi <= ox || y_hi <= oy {
        // ★ Flagged rather than signalled by a zero-sized pixmap, because
        // `tiny_skia::Pixmap::new(0, 0)` returns `None` and would be
        // indistinguishable from an allocation failure — "the clip left
        // nothing to draw" and "the machine is out of memory" are opposite
        // outcomes and must not share a representation.
        return Some((
            Scratch {
                rgba: tiny_skia::Pixmap::new(1, 1)?,
                ink: None,
                empty: true,
            },
            ox,
            oy,
        ));
    }
    let (sw, sh) = ((x_hi - ox) as u32, (y_hi - oy) as u32);
    let mut scratch = Scratch {
        empty: false,
        rgba: tiny_skia::Pixmap::new(sw, sh)?,
        ink: if want_ink {
            let n = (sw as usize).checked_mul(sh as usize)?;
            let mut v: Vec<[f32; 4]> = Vec::new();
            // `try_reserve` rather than `vec![]`, for the same reason
            // `CmykBuffer::try_planes` uses it: a mesh's clipped region is
            // attacker-influenced, and an allocation failure here must be a
            // refusal the caller can disclose, not an abort.
            v.try_reserve_exact(n).ok()?;
            v.resize(n, [0.0; 4]);
            Some(v)
        } else {
            None
        },
    };

    let mut budget = MAX_TRIANGLES;

    match &mesh.data {
        MeshData::Triangles(tris) => {
            for tri in tris {
                if budget == 0 {
                    break;
                }
                budget -= 1;
                let dev = [
                    map(to_device, tri.xy[0]),
                    map(to_device, tri.xy[1]),
                    map(to_device, tri.xy[2]),
                ];
                fill_triangle(&mut scratch, ox, oy, dev, tri.shade, ramp);
            }
        }
        MeshData::Patches(patches) => {
            let mut grid_p: Vec<[f32; 2]> = Vec::new();
            let mut grid_s: Vec<Shade> = Vec::new();
            for patch in patches {
                if budget == 0 {
                    break;
                }
                let n = subdivision_for(patch_device_extent(patch, to_device));
                let side = (n + 1) as usize;
                let cells = (n as usize) * (n as usize) * 2;
                if cells > budget {
                    // The ceiling is on the whole mesh, not on one patch,
                    // so a patch that does not fit stops the mesh rather
                    // than being drawn at a coarser density it never asked
                    // for.
                    break;
                }
                budget -= cells;

                grid_p.clear();
                grid_s.clear();
                for jv in 0..=n {
                    let v = jv as f32 / n as f32;
                    for iu in 0..=n {
                        let u = iu as f32 / n as f32;
                        grid_p.push(map(to_device, patch.at(u, v)));
                        grid_s.push(patch.shade_at(u, v));
                    }
                }
                // MSH32: increasing v outer, increasing u inner, painted in
                // that order, so the largest-v point wins a fold.
                for jv in 0..n as usize {
                    for iu in 0..n as usize {
                        let a = jv * side + iu;
                        let b = a + 1;
                        let c = a + side;
                        let d = c + 1;
                        fill_triangle(
                            &mut scratch,
                            ox,
                            oy,
                            [grid_p[a], grid_p[b], grid_p[c]],
                            [grid_s[a], grid_s[b], grid_s[c]],
                            ramp,
                        );
                        fill_triangle(
                            &mut scratch,
                            ox,
                            oy,
                            [grid_p[b], grid_p[c], grid_p[d]],
                            [grid_s[b], grid_s[c], grid_s[d]],
                            ramp,
                        );
                    }
                }
            }
        }
    }

    Some((scratch, ox, oy))
}

/// Apply a transform to a bare `[x, y]`.
fn map(t: tiny_skia::Transform, p: [f32; 2]) -> [f32; 2] {
    let mut pt = tiny_skia::Point::from_xy(p[0], p[1]);
    t.map_point(&mut pt);
    [pt.x, pt.y]
}

/// The larger device-space side of a patch's control hull.
///
/// The **hull**, not the surface: a cubic Bézier lies inside the convex
/// hull of its control points, so this bounds the patch without evaluating
/// it, which is what makes it usable to *choose* the evaluation density.
fn patch_device_extent(patch: &Patch, to_device: tiny_skia::Transform) -> f32 {
    let mut lo = [f32::INFINITY; 2];
    let mut hi = [f32::NEG_INFINITY; 2];
    for col in &patch.p {
        for pt in col {
            let d = map(to_device, *pt);
            for t in 0..2 {
                lo[t] = lo[t].min(d[t]);
                hi[t] = hi[t].max(d[t]);
            }
        }
    }
    (hi[0] - lo[0]).max(hi[1] - lo[1])
}

/// Scan-convert one triangle into the scratch, opaquely.
///
/// # The fill rule, and why it is inclusive on every edge
///
/// A pixel centre is inside when all three barycentric weights have the
/// same sign as the triangle's own signed area — i.e. `>= 0` after
/// normalising by that area. That is **inclusive on all three edges**, so a
/// pixel exactly on a shared edge is claimed by *both* neighbouring
/// triangles.
///
/// A half-open ("top-left") rule would claim it exactly once, which is what
/// a general rasteriser wants. Here the opposite is correct, and the reason
/// is specific: the two triangles carry the **same interpolated colour**
/// along their shared edge, so double-claiming is invisible, while
/// *under*-claiming leaves a one-pixel transparent crack — and a lattice of
/// cracks through a gradient reads as a broken renderer. Double-writing is
/// harmless here only because this writes **opaquely into a scratch**; it
/// would not be harmless writing into the destination, which is the other
/// half of why [`paint`] uses a scratch.
///
fn fill_triangle(
    scratch: &mut Scratch,
    ox: i32,
    oy: i32,
    dev: [[f32; 2]; 3],
    shade: [Shade; 3],
    ramp: Option<&ColorRamp>,
) {
    let (x0, y0) = (dev[0][0], dev[0][1]);
    let (x1, y1) = (dev[1][0], dev[1][1]);
    let (x2, y2) = (dev[2][0], dev[2][1]);
    if !(x0.is_finite()
        && y0.is_finite()
        && x1.is_finite()
        && y1.is_finite()
        && x2.is_finite()
        && y2.is_finite())
    {
        return;
    }
    // Twice the signed area. Zero means a degenerate triangle, for which
    // MSH-N5 records that the standard is silent — a zero-area triangle
    // covers no pixel centre under any scan conversion, so skipping is not
    // a choice so much as the arithmetic.
    let area = (x1 - x0).mul_add(y2 - y0, -((x2 - x0) * (y1 - y0)));
    if area.abs() < f32::EPSILON {
        return;
    }
    let inv_area = 1.0 / area;

    // Per-edge coverage thresholds implementing CRACK_MARGIN_PX.
    //
    // A normalised barycentric weight IS a normalised distance: `w_i * h_i`
    // is the distance from the sample point to the edge opposite vertex
    // `i`, where `h_i = |area| / |edge_i|` and `area` here is already twice
    // the signed triangle area. So "accept points up to `margin` pixels
    // outside edge `i`" is exactly `w_i >= -margin * |edge_i| / |area|`.
    //
    // Computed per edge rather than as one constant because the three
    // heights of a sliver triangle differ by orders of magnitude, and a
    // single threshold would dilate its long edge by a pixel while barely
    // moving its short one.
    let len = |ax: f32, ay: f32, bx: f32, by: f32| (bx - ax).hypot(by - ay);
    let scale = CRACK_MARGIN_PX / area.abs();
    let t0 = -scale * len(x1, y1, x2, y2);
    let t1 = -scale * len(x0, y0, x2, y2);
    let t2 = -scale * len(x0, y0, x1, y1);

    #[allow(clippy::cast_possible_truncation)]
    let min_x = x0.min(x1).min(x2).floor() as i32;
    #[allow(clippy::cast_possible_truncation)]
    let max_x = x0.max(x1).max(x2).ceil() as i32;
    #[allow(clippy::cast_possible_truncation)]
    let min_y = y0.min(y1).min(y2).floor() as i32;
    #[allow(clippy::cast_possible_truncation)]
    let max_y = y0.max(y1).max(y2).ceil() as i32;

    #[allow(clippy::cast_possible_wrap)]
    let w = scratch.rgba.width() as i32;
    #[allow(clippy::cast_possible_wrap)]
    let h = scratch.rgba.height() as i32;
    let lo_x = (min_x - ox).max(0);
    let hi_x = (max_x - ox + 1).min(w);
    let lo_y = (min_y - oy).max(0);
    let hi_y = (max_y - oy + 1).min(h);
    if lo_x >= hi_x || lo_y >= hi_y {
        return;
    }

    for py in lo_y..hi_y {
        let cy = (py + oy) as f32 + 0.5;
        for px in lo_x..hi_x {
            let cx = (px + ox) as f32 + 0.5;
            // Barycentric weights, normalised by the signed area so the
            // test is orientation-agnostic (MSH-N5 again: nothing gives a
            // mesh triangle a facing).
            let w1 = (cx - x0).mul_add(y2 - y0, -((cy - y0) * (x2 - x0))) * inv_area;
            let w2 = (x1 - x0).mul_add(cy - y0, -((y1 - y0) * (cx - x0))) * inv_area;
            let w0 = 1.0 - w1 - w2;
            #[allow(clippy::cast_sign_loss)]
            let idx = (py as usize) * (scratch.rgba.width() as usize) + (px as usize);
            // Exact coverage first. The margin is consulted only when the
            // exact test says no AND nothing has painted this pixel -- see
            // CRACK_MARGIN_PX for why the unconditional form was wrong.
            if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                let in_margin = w0 >= t0 && w1 >= t1 && w2 >= t2;
                if !in_margin || scratch.rgba.pixels()[idx].alpha() != 0 {
                    continue;
                }
            }
            // MSH-A2: linear (barycentric) interpolation. The standard
            // permits linear or nonlinear and requires neither; this is
            // pdfcer's choice and is what "Gouraud" names.
            let shaded = interpolate(shade, [w0, w1, w2]);
            let Some(rgb) = shaded.resolve(ramp) else {
                continue;
            };
            let to8 = |v: f32| -> u8 {
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                {
                    (v.clamp(0.0, 1.0) * 255.0).round() as u8
                }
            };
            // Opaque, so premultiplied and straight agree and the
            // `from_rgba` invariant (channel <= alpha) cannot be violated
            // by independent rounding — the failure mode `paint_region`
            // documents for the analytic path.
            if let Some(c) =
                tiny_skia::PremultipliedColorU8::from_rgba(to8(rgb.r), to8(rgb.g), to8(rgb.b), 255)
            {
                scratch.rgba.pixels_mut()[idx] = c;
                // ★ The ink plane is written INSIDE the same `if`, keyed on
                // the same `idx`, so a pixel is never marked painted in one
                // plane and not the other. `alpha != 0` in `rgba` is the
                // single authority on coverage for BOTH composites -- the
                // ink plane deliberately carries no occupancy of its own,
                // because two occupancy tests are two things that can
                // disagree.
                if let Some(ink) = scratch.ink.as_mut()
                    && let Some(c) = shaded.resolve_cmyk(ramp)
                {
                    ink[idx] = c;
                }
            }
        }
    }
}

/// The two-plane rasterisation target.
///
/// # Why the ink plane is a sidecar rather than a replacement
///
/// Everything the mesh rasteriser does that is *hard* — the crack margin,
/// the fold-over ordering of `MSH32`, the "has this pixel been claimed yet"
/// test — reads **only the alpha channel**, never the colour. So the ink
/// path does not need its own rasteriser: it needs the same one, writing a
/// second value at the same index.
///
/// Keeping `rgba` authoritative for occupancy means the ink composite and
/// the sRGB composite cover **exactly** the same pixels by construction,
/// rather than by two implementations agreeing. If the ink plane carried
/// its own occupancy flag, a mesh with a ramp hole would paint a pixel in
/// one plane and not the other, and which one you got would depend on the
/// destination — a difference nobody would look for.
///
/// # Cost
///
/// 16 bytes per pixel of the shading's clipped region, allocated **only**
/// when a subtractive destination asked for it. An additive page allocates
/// exactly what it did before this existed.
struct Scratch {
    /// Colour and, in its alpha, the sole record of which pixels the mesh
    /// covered.
    rgba: tiny_skia::Pixmap,
    /// Authored colorants at the same indices. `None` when the destination
    /// composites in sRGB and the plane would never be read.
    ink: Option<Vec<[f32; 4]>>,
    /// The clipped region had no area, so `rgba` is a placeholder and must
    /// not be composited. Distinct from a failed allocation, which is
    /// `None` from [`rasterise`] — see the comment at the flag's only
    /// assignment.
    empty: bool,
}

/// Barycentric interpolation of three shades.
fn interpolate(shade: [Shade; 3], w: [f32; 3]) -> Shade {
    match shade {
        [Shade::Rgb(a), Shade::Rgb(b), Shade::Rgb(c)] => Shade::Rgb(Rgb {
            r: w[0].mul_add(a.r, w[1].mul_add(b.r, w[2] * c.r)),
            g: w[0].mul_add(a.g, w[1].mul_add(b.g, w[2] * c.g)),
            b: w[0].mul_add(a.b, w[1].mul_add(b.b, w[2] * c.b)),
        }),
        [
            Shade::Ink { rgb: ra, cmyk: ca },
            Shade::Ink { rgb: rb, cmyk: cb },
            Shade::Ink { rgb: rc, cmyk: cc },
        ] => Shade::Ink {
            rgb: Rgb {
                r: w[0].mul_add(ra.r, w[1].mul_add(rb.r, w[2] * rc.r)),
                g: w[0].mul_add(ra.g, w[1].mul_add(rb.g, w[2] * rc.g)),
                b: w[0].mul_add(ra.b, w[1].mul_add(rb.b, w[2] * rc.b)),
            },
            cmyk: [
                w[0].mul_add(ca[0], w[1].mul_add(cb[0], w[2] * cc[0])),
                w[0].mul_add(ca[1], w[1].mul_add(cb[1], w[2] * cc[1])),
                w[0].mul_add(ca[2], w[1].mul_add(cb[2], w[2] * cc[2])),
                w[0].mul_add(ca[3], w[1].mul_add(cb[3], w[2] * cc[3])),
            ],
        },
        [
            Shade::Parametric(a),
            Shade::Parametric(b),
            Shade::Parametric(c),
        ] => Shade::Parametric(w[0].mul_add(a, w[1].mul_add(b, w[2] * c))),
        [a, _, _] => a,
    }
}

/// Composite the finished scratch onto the destination, once.
///
/// This is where `alpha`, the clip mask and `/BBox` are applied — all three
/// **after** the mesh has been flattened, which is what stops a shared edge
/// being blended twice. `/BBox` is tested by mapping each destination pixel
/// back into target space, matching what [`crate::shading`]'s analytic path
/// does and honouring `MSH-A5`: `/BBox` **clips**, it does not cause
/// geometry to be discarded at parse time.
#[allow(clippy::too_many_arguments)]
fn composite(
    scratch: &tiny_skia::Pixmap,
    ox: i32,
    oy: i32,
    to_target: tiny_skia::Transform,
    bbox: Option<[f32; 4]>,
    clip: Option<&tiny_skia::Mask>,
    alpha: f32,
    pixmap: &mut tiny_skia::Pixmap,
) -> usize {
    let alpha = alpha.clamp(0.0, 1.0);
    if alpha <= 0.0 {
        return 0;
    }
    let dst_w = pixmap.width() as usize;
    let sw = scratch.width() as usize;
    let sh = scratch.height() as usize;
    let mut painted = 0usize;

    for sy in 0..sh {
        for sx in 0..sw {
            let src = scratch.pixels()[sy * sw + sx];
            if src.alpha() == 0 {
                continue;
            }
            #[allow(clippy::cast_possible_wrap)]
            let (dx, dy) = (ox + sx as i32, oy + sy as i32);
            #[allow(clippy::cast_sign_loss)]
            let didx = (dy as usize) * dst_w + (dx as usize);

            if let Some([bx0, by0, bx1, by1]) = bbox {
                let mut pt = tiny_skia::Point::from_xy(dx as f32 + 0.5, dy as f32 + 0.5);
                to_target.map_point(&mut pt);
                if pt.x < bx0.min(bx1)
                    || pt.x > bx0.max(bx1)
                    || pt.y < by0.min(by1)
                    || pt.y > by0.max(by1)
                {
                    continue;
                }
            }

            let coverage = match clip {
                Some(mask) => f32::from(mask.data()[didx]) / 255.0,
                None => 1.0,
            };
            let a = alpha * coverage;
            if a <= 0.0 {
                continue;
            }

            // Source-over in premultiplied space — the same arithmetic, and
            // the same channel-clamped-to-alpha guard, that
            // `shading::paint_region` documents at length. The source is
            // opaque here, so `src.red()` is already the straight value.
            let dst = pixmap.pixels()[didx];
            let inv = 1.0 - a;
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let out_a = a
                .mul_add(255.0, f32::from(dst.alpha()) * inv)
                .round()
                .clamp(0.0, 255.0) as u8;
            let mix = |s: u8, d: u8| -> u8 {
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let v = (f32::from(s) * a)
                    .mul_add(1.0, f32::from(d) * inv)
                    .round()
                    .clamp(0.0, 255.0) as u8;
                v.min(out_a)
            };
            if let Some(c) = tiny_skia::PremultipliedColorU8::from_rgba(
                mix(src.red(), dst.red()),
                mix(src.green(), dst.green()),
                mix(src.blue(), dst.blue()),
                out_a,
            ) {
                pixmap.pixels_mut()[didx] = c;
                painted += 1;
            }
        }
    }
    painted
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------
    // MSH3 — bit order
    // -----------------------------------------------------------------

    #[test]
    fn bits_are_read_most_significant_first() {
        // 0b1011_0010 read as 4 + 4 must give 0b1011 then 0b0010, not the
        // other way round. MSH3: "reading in sequence from higher-order to
        // lower-order bit positions".
        let mut r = BitReader::new(&[0b1011_0010]);
        assert_eq!(r.read(4), Some(0b1011));
        assert_eq!(r.read(4), Some(0b0010));
        assert_eq!(r.read(1), None);
    }

    #[test]
    fn a_field_may_straddle_a_byte_boundary() {
        // 12-bit coordinates are legal (MSH4) and are exactly the case a
        // word-at-a-time reader gets wrong.
        let mut r = BitReader::new(&[0xAB, 0xCD, 0xEF]);
        assert_eq!(r.read(12), Some(0xABC));
        assert_eq!(r.read(12), Some(0xDEF));
    }

    #[test]
    fn align_discards_to_the_next_byte_and_is_a_no_op_when_already_aligned() {
        let mut r = BitReader::new(&[0xFF, 0x00, 0xFF]);
        assert_eq!(r.read(3), Some(0b111));
        r.align();
        assert_eq!(r.pos, 8);
        r.align();
        assert_eq!(r.pos, 8, "aligning twice must not skip a whole byte");
    }

    // -----------------------------------------------------------------
    // MSH11 — /Decode
    // -----------------------------------------------------------------

    #[test]
    fn decode_maps_the_endpoints_exactly() {
        assert!((decode_field(0, 8, [10.0, 20.0]) - 10.0).abs() < 1e-5);
        assert!((decode_field(255, 8, [10.0, 20.0]) - 20.0).abs() < 1e-5);
        // 32 bits is the widest legal coordinate and the one where a
        // `1 << bits` in u32 would overflow.
        assert!((decode_field(u64::from(u32::MAX), 32, [0.0, 1.0]) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn decode_accepts_an_inverted_range() {
        // Dmin > Dmax is legal for the same reason it is for an image
        // /Decode: the map simply takes a negative slope.
        assert!((decode_field(0, 8, [1.0, 0.0]) - 1.0).abs() < 1e-5);
        assert!((decode_field(255, 8, [1.0, 0.0]) - 0.0).abs() < 1e-5);
    }

    // -----------------------------------------------------------------
    // MSH30 — the Coons -> tensor internal points
    // -----------------------------------------------------------------

    fn planar_grid() -> [[[f32; 2]; 4]; 4] {
        // A flat 3x3 unit grid: p[i][j] = (i, j). Every boundary point is
        // on the plane, so an affine operator must put the internals on it
        // too — at exactly (1,1), (1,2), (2,1), (2,2).
        let mut g = [[[0.0f32; 2]; 4]; 4];
        for (i, col) in g.iter_mut().enumerate() {
            for (j, cell) in col.iter_mut().enumerate() {
                *cell = [i as f32, j as f32];
            }
        }
        g
    }

    #[test]
    fn coons_internals_are_affine_so_a_planar_patch_stays_planar() {
        // The coefficients sum to 9 and the operator is therefore a
        // partition of unity — the free invariant the RAG names.
        let mut g = planar_grid();
        let want = [g[1][1], g[1][2], g[2][1], g[2][2]];
        // Blank the internals so the test cannot pass by leaving them
        // untouched — the vacuous-assertion failure mode of
        // `NEXT_SESSION.md` §2 item 2.
        g[1][1] = [99.0, 99.0];
        g[1][2] = [99.0, 99.0];
        g[2][1] = [99.0, 99.0];
        g[2][2] = [99.0, 99.0];
        coons_internals(&mut g);
        let got = [g[1][1], g[1][2], g[2][1], g[2][2]];
        for (a, b) in got.iter().zip(want.iter()) {
            assert!(
                (a[0] - b[0]).abs() < 1e-3 && (a[1] - b[1]).abs() < 1e-3,
                "planar patch moved: got {a:?}, want {b:?}"
            );
        }
    }

    // -----------------------------------------------------------------
    // MSH28 / MSH29 — the surface and the stream order
    // -----------------------------------------------------------------

    #[test]
    fn the_surface_interpolates_its_four_corners_exactly() {
        let patch = Patch {
            p: planar_grid(),
            corner: [
                Shade::Rgb(Rgb {
                    r: 1.0,
                    g: 0.0,
                    b: 0.0,
                }),
                Shade::Rgb(Rgb {
                    r: 0.0,
                    g: 1.0,
                    b: 0.0,
                }),
                Shade::Rgb(Rgb {
                    r: 0.0,
                    g: 0.0,
                    b: 1.0,
                }),
                Shade::Rgb(Rgb {
                    r: 1.0,
                    g: 1.0,
                    b: 0.0,
                }),
            ],
        };
        // Bernstein basis sums to 1 and is 1 at the ends, so S(0,0) = p00
        // and S(1,1) = p33.
        let a = patch.at(0.0, 0.0);
        let d = patch.at(1.0, 1.0);
        assert!((a[0] - 0.0).abs() < 1e-4 && (a[1] - 0.0).abs() < 1e-4);
        assert!((d[0] - 3.0).abs() < 1e-4 && (d[1] - 3.0).abs() < 1e-4);
        // And the corner COLOURS land on the corners in walk-around order,
        // which is the transposition trap of MSH24: (0,0)=c00, (0,1)=c03,
        // (1,1)=c33, (1,0)=c30.
        assert_eq!(patch.shade_at(0.0, 0.0), patch.corner[0]);
        assert_eq!(patch.shade_at(0.0, 1.0), patch.corner[1]);
        assert_eq!(patch.shade_at(1.0, 1.0), patch.corner[2]);
        assert_eq!(patch.shade_at(1.0, 0.0), patch.corner[3]);
    }

    #[test]
    fn the_patch_stream_order_walks_the_boundary_then_cycles_the_internals() {
        // MSH29's table, asserted as data rather than trusted as a comment.
        // The first twelve are the boundary counterclockwise from p00; the
        // last four are the CYCLE p11 -> p12 -> p22 -> p21, not row-major.
        assert_eq!(
            PATCH_ORDER,
            [
                (0, 0),
                (0, 1),
                (0, 2),
                (0, 3),
                (1, 3),
                (2, 3),
                (3, 3),
                (3, 2),
                (3, 1),
                (3, 0),
                (2, 0),
                (1, 0),
                (1, 1),
                (1, 2),
                (2, 2),
                (2, 1),
            ]
        );
        // The four boundary curves MSH25 names must be the four edges of
        // the tensor grid — the check that catches an unreversed edge.
        let d1 = [
            PATCH_ORDER[0],
            PATCH_ORDER[1],
            PATCH_ORDER[2],
            PATCH_ORDER[3],
        ];
        assert_eq!(d1, [(0, 0), (0, 1), (0, 2), (0, 3)], "D1 is the u=0 edge");
        let c2 = [
            PATCH_ORDER[3],
            PATCH_ORDER[4],
            PATCH_ORDER[5],
            PATCH_ORDER[6],
        ];
        assert_eq!(c2, [(0, 3), (1, 3), (2, 3), (3, 3)], "C2 is the v=1 edge");
        let d2 = [
            PATCH_ORDER[9],
            PATCH_ORDER[8],
            PATCH_ORDER[7],
            PATCH_ORDER[6],
        ];
        assert_eq!(
            d2,
            [(3, 0), (3, 1), (3, 2), (3, 3)],
            "D2 is the u=1 edge, and the stream gives it REVERSED"
        );
        let c1 = [
            PATCH_ORDER[0],
            PATCH_ORDER[11],
            PATCH_ORDER[10],
            PATCH_ORDER[9],
        ];
        assert_eq!(
            c1,
            [(0, 0), (1, 0), (2, 0), (3, 0)],
            "C1 is the v=0 edge, and the stream gives it REVERSED"
        );
    }

    // -----------------------------------------------------------------
    // MSH-A3 — subdivision
    // -----------------------------------------------------------------

    #[test]
    fn subdivision_scales_with_device_size_and_is_bounded_at_both_ends() {
        assert_eq!(subdivision_for(0.0), 1);
        assert_eq!(subdivision_for(3.0), 1);
        assert_eq!(subdivision_for(40.0), 10);
        assert_eq!(subdivision_for(100_000.0), MAX_SUBDIVISION);
        assert_eq!(subdivision_for(f32::NAN), 1);
    }

    // -----------------------------------------------------------------
    // MSH14 — the parametric form interpolates t, not colour
    // -----------------------------------------------------------------

    #[test]
    fn a_parametric_shade_interpolates_the_parameter_not_a_colour() {
        let a = Shade::Parametric(0.0);
        let b = Shade::Parametric(1.0);
        assert_eq!(a.lerp(b, 0.25), Shade::Parametric(0.25));
        // And the resolution step is separate, so f(lerp(t)) is what
        // happens rather than lerp(f(t)) — the distinction MSH14 clause 3
        // exists for.
        assert_eq!(Shade::Parametric(0.5).resolve(None), None);
    }

    /// The ink survives interpolation, and it survives it **in its own
    /// space** rather than as a by-product of the sRGB result.
    #[test]
    fn ink_and_srgb_are_interpolated_independently() {
        let a = Shade::Ink {
            rgb: Rgb {
                r: 0.0,
                g: 0.0,
                b: 0.0,
            },
            cmyk: [0.0, 0.0, 0.0, 1.0],
        };
        let b = Shade::Ink {
            rgb: Rgb {
                r: 1.0,
                g: 1.0,
                b: 1.0,
            },
            cmyk: [1.0, 0.0, 0.0, 0.0],
        };
        let Shade::Ink { rgb, cmyk } = a.lerp(b, 0.25) else {
            panic!(
                "lerp of two Ink shades must stay Ink — if it falls back to \
                    Rgb the colorants are lost silently, which is the whole \
                    defect this variant exists to prevent"
            );
        };
        assert!((rgb.r - 0.25).abs() < 1e-6);
        assert!((cmyk[0] - 0.25).abs() < 1e-6);
        assert!((cmyk[3] - 0.75).abs() < 1e-6);
    }

    /// `resolve_cmyk` must never invent ink from an additive shade.
    ///
    /// A `Some(converted)` here would make every counter read as though the
    /// page composited natively while it had in fact done the exact round
    /// trip the ink path exists to avoid — a wrong answer that reports
    /// itself as the right one.
    #[test]
    fn an_additive_shade_has_no_ink_and_does_not_manufacture_any() {
        let rgb = Shade::Rgb(Rgb {
            r: 0.5,
            g: 0.5,
            b: 0.5,
        });
        assert_eq!(rgb.resolve_cmyk(None), None);
        // ...and a parametric shade with no ramp is likewise None rather
        // than a default, for the same reason.
        assert_eq!(Shade::Parametric(0.5).resolve_cmyk(None), None);
    }

    /// The classification is **all-or-nothing**, and one additive vertex
    /// demotes the whole mesh.
    ///
    /// Asserted rather than left to the comment because the failure it
    /// prevents is a *seam* — half a mesh composited natively and half
    /// bridged, meeting along a line no file asked for — and a seam is
    /// exactly the kind of defect that survives review by looking like
    /// anti-aliasing.
    #[test]
    fn one_additive_vertex_demotes_the_whole_mesh() {
        assert_eq!(classify_colorants(&(9, 0, 0)), MeshColorants::Vertex);
        assert_eq!(classify_colorants(&(9, 1, 0)), MeshColorants::None);
        assert_eq!(classify_colorants(&(0, 9, 0)), MeshColorants::None);
        // Parametric wins outright: the ramp, not the vertices, owns the
        // answer, and the caller is told to go ask it.
        assert_eq!(classify_colorants(&(0, 0, 9)), MeshColorants::Parametric);
        // An empty mesh promises least.
        assert_eq!(classify_colorants(&(0, 0, 0)), MeshColorants::None);
    }
}
