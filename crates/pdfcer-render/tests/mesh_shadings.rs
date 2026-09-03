//! ★ `Pass 125.0` — the four mesh shading types render, and this file is the
//! only end-to-end evidence for three of them.
//!
//! # Why this file exists rather than more unit tests
//!
//! `crates/pdfcer-render/src/mesh.rs` unit-tests the pieces: MSB-first bit
//! order, the `/Decode` map at both endpoints and under an inverted range,
//! the Coons internal-point equations' affine invariant, the corner-colour
//! walk-around order, the stream index table. Those prove the **arithmetic**.
//!
//! They cannot prove the arithmetic is **reachable**. Between a correct
//! evaluator and a painted pixel sit the stream lookup, `/Filter`, the
//! `/Decode` array's arity, the flag state machine, the inheritance table,
//! the transform, and the rasteriser — and a mistake in any of them renders
//! a plausible picture rather than an error. That gap is where
//! `nonisolated_group_sees_its_backdrop.rs`'s Pass nearly shipped as dead
//! code, and the lesson transfers unchanged.
//!
//! The one mesh shading this project has real-world access to is a pair of
//! type 7 patches inside a licensed conformance patch, which is **not
//! redistributable** (`docs/LEGAL.md` §5) and covers one of the four types,
//! one edge-flag value, one bit-width combination, and neither the
//! `/Function` form nor any refusal path. So the fixtures are synthetic:
//! `tools/gen-mesh-fixtures.py`.
//!
//! # The oracle problem, and how these tests avoid inventing one
//!
//! A rendered gradient has no obviously-right answer to assert against, and
//! a committed "known good" PNG only pins whatever the code did the day it
//! was taken (`R215`: an oracle that is a memory of an output is not an
//! oracle). Every assertion here is instead one of three kinds:
//!
//! 1. **An equivalence the STANDARD requires**, between two files that reach
//!    the same picture by different code paths. A disagreement is a defect
//!    whichever side is wrong, and neither file needs blessing:
//!    - a Coons patch **is** a tensor patch whose four internal points are
//!      derived from its boundary (`MSH30`), so `type6_coons.pdf` and
//!      `type7_tensor.pdf` must agree — and the type 7 file's four internal
//!      points were computed by the fixture generator's own independent
//!      transcription of those equations, so agreement is a check of the
//!      renderer's transcription against a second one;
//!    - the type 4 edge-flag sequence `0,0,0,1` builds exactly the two
//!      triangles a 2×2 type 5 lattice builds (`MSH19` vs `MSH22`), so
//!      `type4_triangles.pdf` and `type5_lattice.pdf` must agree — across
//!      two parsers with different record shapes, one of which has no flag
//!      field at all.
//! 2. **A value fixed by arithmetic**: every one of these surfaces
//!    interpolates its corners exactly, so a corner pixel must be the corner
//!    colour. That is Bernstein basis and bilinear interpolation, not taste.
//! 3. **A disagreement the standard requires**, which is what keeps (1) from
//!    passing vacuously. A bilinear patch interior and a two-triangle
//!    Gouraud interior are different surfaces over the same four corners, so
//!    `type6_coons.pdf` and `type4_triangles.pdf` must **differ** — and if
//!    they did not, the equivalences above would be comparing two renders of
//!    nothing.
//!
//! # What is deliberately not asserted
//!
//! Nothing here checks a mesh against Acrobat. That comparison was made
//! during the Pass, on the conformance patch, and its result belongs in the
//! record rather than in a test: pdfcer's mesh differs from Acrobat's by
//! **the same amount** pdfcer's plain raster image differs from Acrobat's
//! plain raster image on the same page (mean |Δ| 26.5 vs 29.6 on one cell,
//! 27.1 vs 26.7 on the other). ⇒ the residual is the `DeviceCMYK` → sRGB
//! path, which belongs to the sibling `iccce` project by decision 064, and
//! is not a mesh defect. Encoding that as a test here would pin a colour
//! table this project does not own.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use pdfcer_core::document::Document;
use pdfcer_core::page_tree;
use pdfcer_render::RenderedPage;

/// The scale every test here renders at.
///
/// 2× rather than 1×, because the per-patch subdivision density is chosen
/// from **device** size (`MSH-A3` is a silence pdfcer fills), so the geometry
/// under test genuinely changes with scale. One scale is one sample; the
/// crack sweep in the Pass record covers 1, 2, 4, 8 and 16.
const SCALE: f32 = 2.0;

fn render(name: &str) -> RenderedPage {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/mesh")
        .join(name);
    let doc =
        Document::from_bytes(std::fs::read(&path).expect("fixture file")).expect("fixture parses");
    let pages = page_tree::pages(&doc).expect("page tree");
    pdfcer_render::render_page(&doc, &pages[0], SCALE).expect("renders")
}

/// The largest per-channel difference between two renders of the same page.
fn max_channel_delta(a: &RenderedPage, b: &RenderedPage) -> u8 {
    assert_eq!(a.pixmap.width(), b.pixmap.width(), "geometry must match");
    assert_eq!(a.pixmap.height(), b.pixmap.height(), "geometry must match");
    a.pixmap
        .pixels()
        .iter()
        .zip(b.pixmap.pixels().iter())
        .map(|(p, q)| {
            p.red()
                .abs_diff(q.red())
                .max(p.green().abs_diff(q.green()))
                .max(p.blue().abs_diff(q.blue()))
        })
        .max()
        .unwrap_or(0)
}

fn differing_pixels(a: &RenderedPage, b: &RenderedPage) -> usize {
    a.pixmap
        .pixels()
        .iter()
        .zip(b.pixmap.pixels().iter())
        .filter(|(p, q)| p.red() != q.red() || p.green() != q.green() || p.blue() != q.blue())
        .count()
}

/// The pixel at a point in the fixtures' own coordinate space.
///
/// The fixtures are 120 pt square and every mesh covers `[10, 110]²`, so a
/// caller can name a corner in the units the generator used and let this
/// deal with the scale and with PDF's upward `y`.
fn at(p: &RenderedPage, x_pt: f32, y_pt: f32) -> (u8, u8, u8) {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let x = (x_pt * SCALE) as u32;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let y = ((120.0 - y_pt) * SCALE) as u32;
    let px = p.pixmap.pixel(x, y).expect("point is on the page");
    (px.red(), px.green(), px.blue())
}

/// Assert a pixel is within `tol` of an expected colour, per channel.
#[track_caller]
fn near(got: (u8, u8, u8), want: (u8, u8, u8), tol: u8, what: &str) {
    let ok = got.0.abs_diff(want.0) <= tol
        && got.1.abs_diff(want.1) <= tol
        && got.2.abs_diff(want.2) <= tol;
    assert!(ok, "{what}: got {got:?}, want {want:?} (+/- {tol})");
}

// ===========================================================================
// (1) The equivalences the standard requires
// ===========================================================================

/// `MSH30` — a Coons patch is a tensor patch with derived internal points.
///
/// The type 7 fixture carries the four internal points **explicitly**,
/// computed by `tools/gen-mesh-fixtures.py`'s own transcription of
/// §8.7.4.5.8's four `1/9 (…)` equations. `mesh.rs` derives them from the
/// boundary. Two independent transcriptions of the same four equations
/// meeting at the same picture is a much stronger statement than either one
/// matching a stored image.
///
/// The tolerance is **1**, not 0: the generator computes in `f64` and the
/// renderer in `f32`, and a control point half a thousandth of a point
/// apart can move an 8-bit channel by one. A larger tolerance would start
/// to admit a real transcription error, since a single wrong coefficient in
/// those equations moves an interior control point by whole points.
#[test]
fn a_coons_patch_and_the_tensor_patch_it_implies_render_the_same() {
    let coons = render("type6_coons.pdf");
    let tensor = render("type7_tensor.pdf");
    let d = max_channel_delta(&coons, &tensor);
    assert!(
        d <= 1,
        "a type 6 patch and the type 7 patch its boundary implies must \
         render the same surface; max channel delta {d}"
    );
}

/// The same equivalence with a **continued** patch on each side.
///
/// Worth its own test rather than folded into the one above, because a
/// continued type 7 record carries **twelve** coordinate pairs and a
/// continued type 6 record carries **eight** — the four internal points are
/// never inherited (Table 86 lists them in every `f != 0` row). A parser
/// that reused the type 6 read count for type 7 would desynchronise here and
/// nowhere else, and the resulting picture would still be a gradient.
#[test]
fn the_equivalence_survives_a_continued_patch() {
    let six = render("type6_flag1.pdf");
    let seven = render("type7_flag1.pdf");
    let d = max_channel_delta(&six, &seven);
    assert!(
        d <= 1,
        "a continued type 7 patch reads 12 coordinate pairs, not 8; \
         max channel delta {d}"
    );
}

/// `MSH19` vs `MSH22` — the flag machine and the lattice build the same mesh.
///
/// The type 4 file's flags are `0, 0, 0, 1`. `MSH16` says the two vertices
/// after a `0` are consumed with their **own flags read and ignored**, and
/// `MSH17` says a `1` shares side `vbc`, giving `(vb, vc, vd)`. For the
/// vertex order the generator uses that is exactly the pair `MSH22` builds
/// for `m = k = 2`.
///
/// Exact equality, not a tolerance: the two files carry the same coordinates
/// and the same colours at the same bit widths, so nothing but the parse can
/// move a pixel. The two records differ in **shape** — type 5 has no flag
/// field at all (`MSH7`), which changes the record's bit length and
/// therefore its byte padding — so this also covers the padding rule for the
/// type where the RAG says it bites most often.
#[test]
fn the_type_4_flag_machine_and_the_type_5_lattice_agree() {
    let four = render("type4_triangles.pdf");
    let five = render("type5_lattice.pdf");
    assert_eq!(
        differing_pixels(&four, &five),
        0,
        "flags 0,0,0,1 must build the same two triangles as a 2x2 lattice"
    );
}

/// The OTHER paint route reaches the same mesh.
///
/// A shading arrives two ways and they anchor in **opposite** coordinate
/// spaces: `sh` in current user space (so a `cm` before it moves the
/// gradient), a `PatternType 2` fill in pattern space mapped by the
/// pattern's own `/Matrix` to the page's *initial* space (so a `cm` does
/// not). The analytic types use that transform directly, to map each
/// destination pixel back; a mesh **inverts** it, to map its own geometry
/// forward. A sign or an inversion lost on the way renders a mesh in the
/// wrong place, at the wrong size, or nowhere.
///
/// ★ This test exists because the first eleven tests in this file all
/// reach the mesh by `sh`. Verifying one instance of a two-instance class
/// and reporting the class as covered is a mistake this project has already
/// made once, on a different feature, and shipped a defect beside a
/// verified twin.
///
/// Both `/Matrix` and the CTM are the identity in the two fixtures, so the
/// pictures must be **identical** — a difference is a transform bug and
/// cannot be anything else. Compared over the interior only: the pattern
/// file paints through a `re f`, whose antialiased edge is a path question
/// rather than a mesh one.
#[test]
fn a_mesh_reached_as_a_shading_pattern_paints_the_same_picture() {
    let by_sh = render("type6_coons.pdf");
    let by_pattern = render("type6_pattern.pdf");
    let w = by_sh.pixmap.width();
    let inset = 26;
    let mut differing = 0usize;
    for y in inset..by_sh.pixmap.height() - inset {
        for x in inset..w - inset {
            let a = by_sh.pixmap.pixel(x, y).expect("in bounds");
            let b = by_pattern.pixmap.pixel(x, y).expect("in bounds");
            if a.red() != b.red() || a.green() != b.green() || a.blue() != b.blue() {
                differing += 1;
            }
        }
    }
    assert_eq!(
        differing, 0,
        "the two routes anchor differently and must still land the same mesh on the same pixels"
    );
}

/// The guard that keeps the two equivalences above from being vacuous.
///
/// A bilinear patch interior and a two-triangle Gouraud interior are
/// genuinely different surfaces over the same four corners — the triangle
/// mesh has a visible diagonal seam in its colour field and the patch does
/// not. If these two ever agreed, the tests above would be comparing two
/// renders of nothing at all, and would keep passing while the mesh code was
/// removed entirely.
#[test]
fn a_bilinear_patch_and_a_gouraud_triangle_pair_are_not_the_same_surface() {
    let patch = render("type6_coons.pdf");
    let tris = render("type4_triangles.pdf");
    let n = differing_pixels(&patch, &tris);
    assert!(
        n > 1000,
        "a Coons interior and a two-triangle interior must differ; only \
         {n} pixels did, which suggests neither is being evaluated"
    );
}

// ===========================================================================
// (2) Values fixed by arithmetic
// ===========================================================================

/// `MSH24` — corner colours are given in **walk-around** order, and every one
/// of these surfaces interpolates its corners exactly.
///
/// The Bernstein basis is 1 at each end and the bilinear colour weights are
/// 1 at each corner, so `S(0,0) = p00` and `C(0,0) = c00` are identities
/// rather than approximations. The four corner colours are saturated
/// primaries chosen so that the specific failure this catches — assigning
/// `c1 c2 c3 c4` in raster order (TL, TR, BL, BR) instead of walking the
/// boundary — swaps two corners and cannot be mistaken for rounding.
///
/// The sample is taken two points inside each corner because the outermost
/// pixel of the silhouette is where the `CRACK_MARGIN_PX` fringe lives, and
/// a fringe pixel is a coverage question rather than a colour one.
#[test]
fn the_four_corner_colours_land_on_the_four_corners() {
    let p = render("type6_coons.pdf");
    near(at(&p, 12.0, 12.0), (255, 0, 0), 12, "c00 red at (LO, LO)");
    near(
        at(&p, 12.0, 108.0),
        (0, 255, 0),
        12,
        "c03 green at (LO, HI)",
    );
    near(
        at(&p, 108.0, 108.0),
        (0, 0, 255),
        12,
        "c33 blue at (HI, HI)",
    );
    near(
        at(&p, 108.0, 12.0),
        (255, 255, 0),
        12,
        "c30 yellow at (HI, LO)",
    );
}

/// `MSH14` clause 3 — the parametric form interpolates `t`, then applies the
/// function. **Not** the other way round.
///
/// The fixture's `/Function` is `FunctionType 2` with `/N 2`, mapping
/// `t -> t²` from black to white, and its four vertices carry
/// `t = 0, 1/3, 2/3, 1`. The two orders of operations therefore disagree
/// everywhere except at the vertices. At the centroid of the lower-left
/// triangle the interpolated `t` is `(0 + 1/3 + 2/3) / 3 = 1/3`, and:
///
/// * correct, `f(lerp(t))`  = `(1/3)²` = `0.1111` -> **28** of 255;
/// * wrong,  `lerp(f(t))` = `(0 + 1/9 + 4/9) / 3` = `0.1852` -> **47**.
///
/// A gap of 19 levels, in the direction the exponent fixes: the correct
/// answer is **darker**, because `t²` is convex and Jensen's inequality
/// makes the function of the average no greater than the average of the
/// function. Asserting the direction as well as the value is what makes this
/// a test of the ORDER rather than of one number.
///
/// ★ THE FIRST DRAFT OF THIS TEST EXPECTED 85 AND 117, AND WOULD HAVE
/// FAILED A CORRECT RENDERER. It applied an sRGB transfer curve to the
/// function's output, which is wrong twice over: a `DeviceRGB` component in
/// a PDF **is** the device value (§8.6.4.3), not a linear-light quantity
/// awaiting encoding, and pdfcer's pixmap stores exactly that. Recorded
/// rather than quietly corrected, because the failure mode was a test
/// asserting a plausible number derived from the wrong model — the shape
/// `NEXT_SESSION.md` §2 warns about, arriving from the direction of the
/// assertion instead of the code.
#[test]
fn a_parametric_mesh_applies_its_function_after_interpolating() {
    let p = render("type4_parametric.pdf");
    // The centroid of the triangle (LO,LO), (LO,HI), (HI,LO).
    let (r, g, b) = at(&p, 43.0, 43.0);
    assert!(
        r == g && g == b,
        "the ramp runs black to white, so every sample is neutral: {r} {g} {b}"
    );
    assert!(
        r < 38,
        "f(lerp(t)) must be used, not lerp(f(t)): a grey of {r} at the \
         centroid is at or above the 47 that averaging AFTER evaluation \
         gives, which is the wrong order and is always lighter for t^2"
    );
    assert!(
        r > 20,
        "a grey of {r} is darker than f(1/3) = 1/9 = 28 can explain; \
         something other than the order of operations is wrong"
    );
}

// ===========================================================================
// (3) The inheritance table — Tables 85 and 86
// ===========================================================================

/// Every nonzero edge flag lands its continued patch where the table says.
///
/// Each fixture tiles the **same** `[10, 110]²` square with two patches: one
/// new, one continued on edge 1, 2 or 3. So "did the inheritance work?" is
/// answerable by coverage alone, with no colour reasoning at all — a
/// mis-oriented inherited edge twists the second patch into a bowtie and
/// leaves most of its half of the square unpainted.
///
/// ★ That is not a hypothetical failure mode. It is what the first draft of
/// the fixture generator produced, because flags 2 and 3 hand the continued
/// patch its `u = 0` edge **reversed** and the generator authored the second
/// patch as if it arrived forward. The picture was a pair of lens shapes
/// with a hole between them, and it looked enough like "a gradient" at a
/// glance to need a second look.
#[test]
fn a_continued_patch_lands_where_the_inheritance_table_says() {
    for flag in [1u8, 2, 3] {
        let name = format!("type6_flag{flag}.pdf");
        let p = render(&name);
        // Walk the interior on a coarse grid and require every sample to be
        // painted. White is the page; nothing in these fixtures paints
        // white, so an unpainted sample is unmistakable.
        let mut unpainted = Vec::new();
        let mut x = 14.0f32;
        while x < 106.0 {
            let mut y = 14.0f32;
            while y < 106.0 {
                let c = at(&p, x, y);
                if c == (255, 255, 255) {
                    unpainted.push((x, y));
                }
                y += 4.0;
            }
            x += 4.0;
        }
        assert!(
            unpainted.is_empty(),
            "{name}: the two patches must tile the whole square; \
             {} sample(s) unpainted, first at {:?}",
            unpainted.len(),
            unpainted.first()
        );
    }
}

/// The seam between two patches carries the colour both of them carry.
///
/// A continued patch inherits two corner **colours** as well as four
/// control points (`c1 = c2ᵖʳᵉᵛ`, `c2 = c3ᵖʳᵉᵛ` for flag 1). If a renderer
/// inherited the geometry and not the colours — or inherited the wrong pair
/// — the tiling test above would still pass and a visible colour
/// discontinuity would run down the seam.
///
/// Sampled two points either side of the shared edge, where a discontinuity
/// is largest and interpolation error is smallest.
#[test]
fn the_shared_edge_of_two_patches_has_no_colour_seam() {
    // flag 1: the seam is the horizontal line y = 60.
    let p = render("type6_flag1.pdf");
    for x in [20.0f32, 60.0, 100.0] {
        let below = at(&p, x, 57.0);
        let above = at(&p, x, 63.0);
        let d = below
            .0
            .abs_diff(above.0)
            .max(below.1.abs_diff(above.1))
            .max(below.2.abs_diff(above.2));
        assert!(
            d < 40,
            "at x={x} the seam jumps by {d}: {below:?} -> {above:?}. The \
             continued patch inherits two CORNER COLOURS as well as four \
             control points; a jump here means it did not."
        );
    }
}

// ===========================================================================
// (4) Refusals and partial data — MSH-N2, MSH-A4
// ===========================================================================

/// `MSH-N2` — a truncated stream paints what completed.
///
/// The fixture's stream stops three bytes into its fourth vertex. Three
/// complete vertices remain, which is one whole triangle, and §8.7.4.5.5's
/// sole "an error occurs" is about the file rather than about what a reader
/// must do — so pdfcer paints the triangle and discloses the discard rather
/// than refusing the whole shading.
///
/// Asserted by geometry: the surviving triangle is `(LO,LO)`, `(LO,HI)`,
/// `(HI,LO)`, so a point well inside it is painted and a point in the
/// opposite corner — which only the discarded fourth vertex could have
/// reached — is not.
#[test]
fn a_truncated_mesh_paints_its_complete_records_and_drops_the_rest() {
    let p = render("type4_truncated.pdf");
    assert_ne!(
        at(&p, 30.0, 30.0),
        (255, 255, 255),
        "the one complete triangle must still paint"
    );
    assert_eq!(
        at(&p, 100.0, 100.0),
        (255, 255, 255),
        "the far corner belongs to the triangle the truncated vertex would \
         have completed; painting it means the partial record was used"
    );
}

/// `MSH-A4` — an `Indexed` mesh colour space is refused, not rendered.
///
/// The mesh clauses do not repeat types 1/2/3's blanket exclusion of
/// `Indexed`, so a literal reading permits one — in which the per-vertex
/// "colour components" are palette **indices**, `/Decode` maps them into
/// `[0, hival]`, and interpolation runs across palette order. That produces
/// rainbows nobody authored. pdfcer refuses with a named reason instead, and
/// the page stays blank.
#[test]
fn an_indexed_mesh_colour_space_is_refused_rather_than_interpolated() {
    let p = render("type4_indexed.pdf");
    assert_eq!(
        at(&p, 30.0, 30.0),
        (255, 255, 255),
        "an Indexed mesh must paint nothing; interpolating palette indices \
         is a literal reading of the clause and is not what any file means"
    );
}

/// `MSH-A1` — the patch-padding ambiguity is a setting, and the default
/// reading is the one that decodes a byte-padded file.
///
/// `type6_unaligned.pdf` uses `BitsPerFlag 4`, `BitsPerCoordinate 12`,
/// `BitsPerComponent 4`, which makes a new-patch record 340 bits — not a
/// multiple of 8 — so the two readings of the ambiguity disagree about where
/// the second record starts. It is authored under reading (a), the shipped
/// default, and it carries the same two patches as `type6_flag2.pdf`.
///
/// So the assertion is that it renders the same picture as its byte-aligned
/// twin, to within the quantisation the narrower fields cost. A renderer
/// taking the other reading would find the second record's flag in the
/// middle of the first record's padding and produce something unrecognisable
/// — not a slightly different picture.
#[test]
fn the_default_patch_padding_reading_decodes_an_unaligned_file() {
    let aligned = render("type6_flag2.pdf");
    let unaligned = render("type6_unaligned.pdf");
    let d = max_channel_delta(&aligned, &unaligned);
    assert!(
        d <= 8,
        "12-bit coordinates and 4-bit components cost quantisation, not \
         structure; a max channel delta of {d} means the second patch \
         record was not found where the default reading says it is"
    );
}
