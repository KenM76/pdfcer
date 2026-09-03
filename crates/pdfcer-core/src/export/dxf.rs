//! # DXF export — pdfcer's vector model written as CAD geometry
//!
//! Turns a decomposed page ([`crate::vector::PageObjects`]) into an ASCII
//! DXF file that SOLIDWORKS, AutoCAD LT and plasma-table controllers open
//! natively.
//!
//! ## Why this exists, and why it is not "make pdfcer look like Acrobat"
//!
//! SOLIDWORKS gates its own PDF import on Adobe Acrobat/Illustrator being
//! installed and licensed. The operator asked whether pdfcer could satisfy
//! that gate instead. It could not honourably — that would mean
//! impersonating Adobe's COM registration to make another vendor's licence
//! check pass — **and it would buy nothing**, because SOLIDWORKS imports
//! **DXF natively with no Adobe dependency at all**. The established
//! workflow is already PDF → Illustrator/CorelDraw/Inkscape → DXF →
//! SOLIDWORKS; Adobe is only ever the *converter* in that chain.
//!
//! So this module does not work around the gate. It makes the gate
//! irrelevant, on every seat, with nothing to satisfy.
//!
//! ## What makes this pdfcer's feature rather than a generic converter
//!
//! **Scale.** A PDF drawing is at *paper* scale: a 1:2 view exports at half
//! size, and every generic PDF→DXF converter hands you geometry that is
//! silently wrong by a factor nobody wrote down. pdfcer already knows how to
//! ask — the measure tool's *scale by known dimension* takes the length the
//! drawing says a feature is and derives the rest. [`DxfOptions::scale`] is
//! where that answer arrives.
//!
//! ## Format: ASCII DXF R2000 (`AC1015`), hand-written
//!
//! `HEADER`, `TABLES` (`LTYPE` + `LAYER`) and `ENTITIES`, using
//! `LWPOLYLINE`, `CIRCLE`, `ARC` and `SPLINE`.
//!
//! ### ★ The version was wrong first, and the RAG had already said so
//!
//! This declared `AC1009` (R12) on my own reasoning that R12 is "one step
//! more conservative than R2000 and therefore reaches further" — while
//! emitting `LWPOLYLINE` and `SPLINE`, **which R12 does not have.** R12
//! draws polylines as `POLYLINE`/`VERTEX`/`SEQEND` and has no spline
//! entity at all, so the file claimed one dialect and spoke another.
//! `ezdxf` rejected every output with *"missing 'AcDbPolyline' subclass"*.
//!
//! `C:\personal_rag\dxf\lesson_20260603_ezdxf_authoring_cut_files_lwpolyline.md`
//! had already named **`AC1015` (R2000)** as the compatible baseline for
//! AutoCAD LT 2004 and older plasma controllers. I had read it, and
//! substituted a guess for its recommendation. Recorded here because the
//! guess was not merely wrong, it was *incoherent* — and the only thing
//! that caught it was parsing the output with a real DXF reader instead of
//! grepping it for strings.
//!
//! R2000 requires what R12 does not: an entity **handle** (group code 5),
//! and `100` **subclass markers** (`AcDbEntity`, then `AcDbPolyline` /
//! `AcDbCircle` / `AcDbArc` / `AcDbSpline`). Those are emitted, and
//! `$HANDSEED` is kept above every handle issued.
//!
//! **Hand-written, with no new dependency**, matching the precedent
//! `Pass 48.4` set for TIFF import. That is not merely house style here — it
//! is what makes the compatibility constraints below hold *by construction*
//! rather than by post-processing (see the next section).
//!
//! ## The AutoCAD LT 2004 constraints, and why writing by hand satisfies them for free
//!
//! From `C:\personal_rag\dxf\lesson_20260424_autocad_lt_2004_compat.md`:
//! a plasma cutter's CAM software often runs AutoCAD LT 2004, which
//! **refuses the whole file** — "Unknown entity" / "Drawing recovery" /
//! silent failure — when it meets either of two things modern writers emit
//! even in R2000 mode:
//!
//! 1. **`MATERIAL` objects**, auto-created in the `OBJECTS` section.
//! 2. **Group code 94** on entities.
//!
//! The operator's existing `ezdxf` pipeline has to strip both afterwards.
//! **This writer emits neither, because it emits only what is listed above
//! — there is no `OBJECTS` section to hold a `MATERIAL` and no code path
//! that writes 94.** A constraint that cannot be violated is worth more
//! than one that is fixed downstream, and it is the concrete payoff of not
//! reaching for a library.
//!
//! ## Curves: arcs are recognised, not flattened
//!
//! From the same RAG (`lesson_20260603_ezdxf_authoring_cut_files_lwpolyline.md`):
//! flattening circular features to fine polylines **bloats the file
//! catastrophically** — a measured ~40 washers came out at **767 KB**
//! because each circle became hundreds of segments.
//!
//! PDF has no arc primitive: a circle is four cubic Béziers (§8.5.2.1 has
//! `c`/`v`/`y` and nothing else), so a naive PDF→DXF converter reproduces
//! that bloat exactly. [`arc_fit`] therefore tries to recognise a cubic as
//! a circular arc before falling back, and a subpath of four such arcs
//! closing on itself becomes one `CIRCLE`.
//!
//! ## What this does NOT do, named so nobody promises it
//!
//! A PDF of a CAD drawing is **printed output** — derived geometry. Import
//! yields sketch entities: never features, never dimensions-as-constraints,
//! never a parametric model. It is the right tool for tracing a legacy
//! drawing or a supplier's PDF, and it is not a route back to a model.

use crate::vector::{
    Bounds, PageObjects, PathObject, Point, Segment, Subpath, TextObject, VectorObject,
};

/// The layer every geometric entity is written to.
///
/// Layer `0` is DXF's always-present default, so using it means the file
/// needs no layer table beyond the one it must have anyway.
const GEOMETRY_LAYER: &str = "0";

/// The layer every `TEXT` entity is written to — **deliberately not `0`.**
///
/// # Why text gets its own layer, sourced rather than guessed
///
/// `C:\personal_rag\dxf\lesson_20260519_sheet_border_titleblock_furniture.md`
/// records, from the operator's own drawings, that
///
/// > **The title block is often drawn on layer `0`**, not on a
/// > `BORDER`/`TEXT`/`TITLE` layer, so layer filtering does not remove it.
///
/// That is a documented, already-painful failure in this operator's
/// pipeline: text mixed into layer `0` cannot be filtered out, so the
/// downstream cleanup has to identify furniture *geometrically* — the
/// concentric-rectangle and title-block-grid heuristics that same lesson
/// had to invent. Every one of those heuristics exists because an upstream
/// producer put text where it could not be separated.
///
/// pdfcer is that upstream producer here, so it declines to repeat the
/// mistake. Text on its own layer means the import is a checkbox: turn
/// `PDFCER_TEXT` off and what remains is the geometry to trace. No
/// heuristics, no area comparisons, no guessing which rectangle is the part.
///
/// # Why this is not merely "nice"
///
/// It is what makes emitting text SAFE at all. Labels are reference
/// material — you want to read the dimension, you do not want to cut it.
/// Mixed into layer `0`, a helpful label becomes a cut path on a plasma
/// table. Separated, it cannot.
const TEXT_LAYER: &str = "PDFCER_TEXT";

/// The text style `TEXT` entities reference (group code 7).
///
/// `STANDARD` is the name every CAD consumer already has, and the `STYLE`
/// table below defines it anyway — a reference to an undefined style is
/// the same class of dangling reference as the `LTYPE` one that the R12→
/// R2000 correction had to fix (§7.3.10's principle, in DXF's dialect).
const TEXT_STYLE: &str = "STANDARD";

/// Units for the DXF header's `$INSUNITS` (group code 70).
///
/// **Not optional, and not defaultable to "whatever".** PDF user space is
/// points — 1/72 inch (§8.3.2.3) — which is a unit no CAD consumer expects
/// to receive. A file that does not say what its numbers mean gets
/// interpreted as the receiving application's current default, and the
/// operator discovers the mistake at the cutting table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DxfUnits {
    /// `$INSUNITS 1` — inches. The default, because PDF's own unit is a
    /// 72nd of one and the conversion is therefore exact in binary.
    #[default]
    Inches,
    /// `$INSUNITS 4` — millimetres.
    Millimetres,
}

impl DxfUnits {
    /// The `$INSUNITS` code.
    const fn code(self) -> i32 {
        match self {
            Self::Inches => 1,
            Self::Millimetres => 4,
        }
    }

    /// How many of this unit one PDF point is.
    ///
    /// §8.3.2.3: the default user-space unit is 1/72 inch.
    const fn per_point(self) -> f64 {
        match self {
            Self::Inches => 1.0 / 72.0,
            Self::Millimetres => 25.4 / 72.0,
        }
    }
}

/// How the export is configured.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DxfOptions {
    /// Output units (`$INSUNITS`).
    pub units: DxfUnits,
    /// The **drawing scale** — how many real-world units one paper unit
    /// represents. `1.0` exports at paper size; `2.0` on a 1:2 view
    /// restores full size.
    ///
    /// # This is the field the whole feature turns on
    ///
    /// Every generic PDF→DXF converter exports at paper scale and says
    /// nothing, so a 1:2 detail arrives at half size and looks plausible.
    /// pdfcer can do better because it already has the measure tool's
    /// *scale by known dimension*: the operator types the length the
    /// drawing itself prints for a feature, and this is where that answer
    /// lands.
    pub scale: f64,
    /// Emit `CIRCLE`/`ARC` for Béziers that are circular within
    /// [`Self::arc_tolerance`], instead of `SPLINE`.
    ///
    /// On by default: PDF has no arc primitive, so every hole and fillet
    /// arrives as cubics, and not recognising them is what produced a
    /// measured 767 KB for forty washers.
    pub fit_arcs: bool,
    /// How far, in **PDF points before scaling**, a cubic may deviate from
    /// a true circular arc and still be emitted as one.
    ///
    /// Deliberately expressed pre-scale: it is a statement about how well
    /// the producer approximated a circle, which is a property of the
    /// input, not of the output size.
    pub arc_tolerance: f64,
    /// What to do with the page's text.
    pub text: DxfText,
}

/// Whether page text becomes `TEXT` entities, and where it lands.
///
/// # The decision this enum records
///
/// Three mappings were available and only one survives contact with what
/// a DXF is FOR here:
///
/// 1. **Omit text** (what the first slice did). Honest, disclosed, and
///    lossy in a way that hurts: the dimensions printed on a legacy drawing
///    are most of why an operator is tracing it. They are reading the
///    sheet, not just its outlines.
/// 2. **Convert glyphs to geometry.** This is what SOLIDWORKS' own
///    flat-pattern export does — `lesson_20260721_sketch_text_stencil_…`
///    records through-cut stencil text arriving as 29 open `LWPOLYLINE`
///    chunks. Correct when the text is *meant to be cut*. Wrong here:
///    it makes a reference label indistinguishable from a cut path, and
///    unreadable as text.
/// 3. **`TEXT` entities on a separate layer.** Readable, selectable,
///    and — because of [`TEXT_LAYER`] — removable in one click.
///
/// (3) is the default. (1) stays available because it is the right answer
/// when the destination is a cutting table and any stray entity is a
/// hazard.
///
/// `MTEXT` was considered and rejected: it carries formatting pdfcer cannot
/// faithfully derive from a content stream (its `\f` font codes, stacking
/// and column layout have no PDF equivalent), and older LT versions handle
/// it least well — the same conservatism that chose `AC1015`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DxfText {
    /// One `TEXT` entity per text **run**, on [`TEXT_LAYER`].
    ///
    /// Per run, not per text object: a `BT`…`ET` on a real drawing holds
    /// every label on the sheet — the operator's own has 237 in one object
    /// — and one `TEXT` carrying all of them concatenated at a single
    /// insertion point would be worse than omitting them.
    #[default]
    Entities,
    /// Leave text out entirely, counted in [`DxfOutcome::skipped_text`].
    Omit,
}

impl Default for DxfOptions {
    fn default() -> Self {
        Self {
            units: DxfUnits::default(),
            scale: 1.0,
            // Kappa-based Bézier circle approximation is accurate to about
            // 0.02% of the radius, so a tolerance far below a plotter's
            // resolution still admits every honestly-drawn circle while
            // rejecting a curve that merely passes near one.
            fit_arcs: true,
            arc_tolerance: 0.05,
            text: DxfText::Entities,
        }
    }
}

/// What an export produced — the disclosure half (rule 4).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DxfOutcome {
    /// `LINE` + `LWPOLYLINE` entities written.
    pub polylines: usize,
    /// `CIRCLE` entities written.
    pub circles: usize,
    /// `ARC` entities written.
    pub arcs: usize,
    /// `SPLINE` entities written — curves that could not be fitted.
    pub splines: usize,
    /// **Objects skipped because DXF has no representation for them**, by
    /// kind. Text and images, today.
    ///
    /// Counted and reported rather than dropped quietly: an operator whose
    /// drawing was half annotation gets a DXF that looks like the geometry
    /// went missing, and "the labels are not in this file" is a sentence
    /// they need before they open it in SOLIDWORKS, not after.
    pub skipped_text: usize,
    /// Image XObjects skipped — DXF has no raster entity in this subset.
    pub skipped_images: usize,
    /// `TEXT` entities written, on [`TEXT_LAYER`].
    pub text_entities: usize,
    /// Text **runs** that could not be written because their characters
    /// were not readable — no font resolver in scope, or a `/ToUnicode`-less
    /// `Identity-H` encoding whose codes map to nothing (§9.10.2's failure
    /// clause).
    ///
    /// Counted separately from [`Self::skipped_text`] because the two ask
    /// different things of the operator. `skipped_text` means *pdfcer chose
    /// not to write this* — they set [`DxfText::Omit`], and nothing is
    /// wrong. This means *pdfcer could not read it*, which is a fact about
    /// the source PDF and the reason the DXF is missing labels the operator
    /// can plainly see on screen. Rolling them together would let the
    /// second hide inside the first.
    pub unreadable_text: usize,
}

/// Write `model` as an ASCII DXF.
///
/// # Errors
///
/// None today — the writer cannot fail on well-formed input, and
/// malformed input is skipped and counted rather than refused. The
/// signature returns the outcome directly for that reason; if a future
/// slice gains a refusal (a degenerate CTM, say), it becomes a `Result`
/// then rather than pre-emptively now.
#[must_use]
pub fn write_dxf(model: &PageObjects, opts: &DxfOptions) -> (String, DxfOutcome) {
    let mut out = String::with_capacity(4096);
    let mut outcome = DxfOutcome::default();
    let unit_scale = opts.units.per_point() * opts.scale;
    let mut handles = Handles(0x100);

    header(&mut out, opts, model);
    tables(&mut out);

    out.push_str("  0\nSECTION\n  2\nENTITIES\n");
    for obj in &model.objects {
        match obj {
            VectorObject::Path(p) => {
                path_entities(&mut out, p, unit_scale, opts, &mut outcome, &mut handles);
            }
            VectorObject::Text(t) => match opts.text {
                DxfText::Entities => {
                    text_entities(&mut out, t, unit_scale, &mut outcome, &mut handles);
                }
                DxfText::Omit => outcome.skipped_text += 1,
            },
            VectorObject::Image(_) => outcome.skipped_images += 1,
        }
    }
    out.push_str("  0\nENDSEC\n  0\nEOF\n");
    (out, outcome)
}

/// The `HEADER` section.
///
/// Deliberately minimal: `$ACADVER`, `$INSUNITS` and the drawing extents.
/// Every variable omitted is one an old consumer cannot object to, and the
/// LT 2004 lesson is that objecting means refusing the entire file.
fn header(out: &mut String, opts: &DxfOptions, model: &PageObjects) {
    out.push_str("  0\nSECTION\n  2\nHEADER\n");
    // AC1015 = R2000, the RAG's own recommendation for AutoCAD LT 2004 and
    // older plasma controllers. See the module docs for the R12 guess that
    // preceded it and why it could not work.
    out.push_str("  9\n$ACADVER\n  1\nAC1015\n");
    // Above every handle this writer issues. A reader allocating new objects
    // starts here, so a seed below an existing handle is how two objects end
    // up sharing one.
    out.push_str("  9\n$HANDSEED\n  5\nFFFF\n");
    out.push_str("  9\n$INSUNITS\n 70\n");
    out.push_str(&format!("{:6}\n", opts.units.code()));

    let unit_scale = opts.units.per_point() * opts.scale;
    let e = extents(model).unwrap_or(Bounds::EMPTY);
    out.push_str("  9\n$EXTMIN\n");
    point3(out, 10, e.min.x * unit_scale, e.min.y * unit_scale);
    out.push_str("  9\n$EXTMAX\n");
    point3(out, 10, e.max.x * unit_scale, e.max.y * unit_scale);
    out.push_str("  0\nENDSEC\n");
}

/// The `TABLES` section — one `LAYER` table holding layer `0`.
///
/// A single layer today. Per-object layers (from OCGs, or by colour) are a
/// named later slice; emitting a `LAYER` table with only `0` is what makes
/// every entity's layer reference resolvable, which some consumers require
/// and none object to.
fn tables(out: &mut String) {
    out.push_str("  0\nSECTION\n  2\nTABLES\n");
    // LTYPE FIRST: layer 0 names CONTINUOUS, and in R2000 a reference to a
    // linetype the file never defines is a dangling one. R12 tolerated it;
    // this is one of the things that changes with the version.
    out.push_str("  0\nTABLE\n  2\nLTYPE\n  5\n5\n100\nAcDbSymbolTable\n 70\n     1\n");
    out.push_str("  0\nLTYPE\n  5\n14\n100\nAcDbSymbolTableRecord\n100\nAcDbLinetypeTableRecord\n");
    out.push_str(
        "  2\nCONTINUOUS\n 70\n     0\n  3\nSolid line\n 72\n    65\n 73\n     0\n 40\n0.0\n",
    );
    out.push_str("  0\nENDTAB\n");

    // TWO layers: geometry on `0`, text on its own. See TEXT_LAYER for the
    // sourced reason. The `70` count is 2 to match, and a count that
    // disagrees with the records that follow it is the kind of internal
    // inconsistency a strict reader rejects the file over.
    out.push_str("  0\nTABLE\n  2\nLAYER\n  5\n2\n100\nAcDbSymbolTable\n 70\n     2\n");
    out.push_str("  0\nLAYER\n  5\n10\n100\nAcDbSymbolTableRecord\n100\nAcDbLayerTableRecord\n");
    out.push_str("  2\n0\n 70\n     0\n 62\n     7\n  6\nCONTINUOUS\n");
    out.push_str("  0\nLAYER\n  5\n11\n100\nAcDbSymbolTableRecord\n100\nAcDbLayerTableRecord\n");
    // Colour 2 (yellow) rather than 7: the text layer reads as distinct
    // from the geometry at a glance, which is the point of separating it.
    out.push_str(&format!(
        "  2\n{TEXT_LAYER}\n 70\n     0\n 62\n     2\n  6\nCONTINUOUS\n"
    ));
    out.push_str("  0\nENDTAB\n");

    // STYLE: `TEXT` entities name a style in group 7, and a name the file
    // never defines is a dangling reference — the same defect class the
    // R12→R2000 correction had to fix for LTYPE. Emitted unconditionally,
    // even when no text is written, because an empty-but-valid table costs
    // eleven lines and a conditional table is a state that only breaks on
    // the documents nobody tested.
    out.push_str("  0\nTABLE\n  2\nSTYLE\n  5\n3\n100\nAcDbSymbolTable\n 70\n     1\n");
    out.push_str(
        "  0\nSTYLE\n  5\n12\n100\nAcDbSymbolTableRecord\n100\nAcDbTextStyleTableRecord\n",
    );
    // 40 = fixed height 0 ("not fixed" — each entity carries its own),
    // 41 = width factor, 50 = oblique, 71 = generation flags, 42 = last
    // height used. txt.shx is the universal fallback font name.
    out.push_str(&format!(
        "  2\n{TEXT_STYLE}\n 70\n     0\n 40\n0.0\n 41\n1.0\n 50\n0.0\n 71\n     0\n 42\n0.2\n  3\ntxt\n  4\n\n"
    ));
    out.push_str("  0\nENDTAB\n  0\nENDSEC\n");
}

/// Every `TEXT` entity one text object contributes — one per **run**.
///
/// # Geometry: where the text lands and how big it is
///
/// The insertion point is the run's ink-box bottom-left, and the height is
/// the ink box's height. Both are read from the laid-out box rather than
/// from `/FontMatrix` and the font size, and that is a deliberate trade:
///
/// - The box is what the operator SEES. Text placed to match it overlays
///   the drawing the way the PDF did, which is the whole job of a
///   reference layer.
/// - Deriving height from [`crate::vector::TextFont::size`] would need the
///   text-rendering matrix (§9.4.4's `Tfs × Th × Tm × CTM` product) resolved
///   per run, and would still be wrong for the case that matters — a
///   `Tz`-condensed or matrix-scaled label, which CAD title blocks use
///   constantly.
///
/// The honest limit, stated rather than left for someone to find: **ink
/// height is not cap height.** An all-caps label (`"SECTION A-A"`, which is
/// most of a drawing) measures very close to correct; an all-lowercase run
/// with no ascender measures its x-height and comes out small. It is a
/// reference layer, and legible-but-slightly-small beats absent.
///
/// # Rotation is not carried
///
/// [`Bounds`] is axis-aligned, so a rotated run's box is its bounding
/// rectangle and the rotation angle is not recoverable from it. `TEXT` is
/// therefore written at rotation 0, inside that box. A vertical dimension
/// label will read horizontally in the DXF. This is disclosed here rather
/// than fixed because fixing it means carrying the text-rendering matrix
/// per run, which is a real slice and not a line.
fn text_entities(
    out: &mut String,
    text: &TextObject,
    unit_scale: f64,
    outcome: &mut DxfOutcome,
    h: &mut Handles,
) {
    // A text object that decomposed to NO runs at all. It happens for real:
    // no font resolver in scope leaves the walker unable to advance the pen,
    // so nothing ever gets a box and nothing is pushed — and the object then
    // contributes to no counter and disappears from the outcome entirely.
    //
    // That silent drop is precisely what the disclosure exists to prevent,
    // and it was found by the test guarding the OLD behaviour rather than by
    // reading this code. Counted at object granularity because run
    // granularity is exactly the thing that is unavailable here.
    if text.runs.is_empty() {
        outcome.unreadable_text += 1;
        return;
    }
    for (i, run) in text.runs.iter().enumerate() {
        let Some(s) = text.run_text(i) else {
            outcome.unreadable_text += 1;
            continue;
        };
        let s = sanitize_text(s);
        // An empty run is not a failure and not an omission — it is a
        // positioning no-op the producer wrote (`() Tj` is legal and
        // common). Nothing to write and nothing to disclose.
        if s.is_empty() {
            continue;
        }
        let b = run.bounds;
        // Non-finite is checked as well as empty: `Bounds::EMPTY` is
        // `min = +∞`, so an unbounded box passes `is_empty` only by the
        // ordering test and a `NaN` coordinate passes BOTH. A `NaN` written
        // into group 10 is a coordinate no reader can parse.
        let finite = b.min.x.is_finite()
            && b.min.y.is_finite()
            && b.max.x.is_finite()
            && b.max.y.is_finite();
        if !finite || b.is_empty() {
            outcome.unreadable_text += 1;
            continue;
        }
        let height = (b.max.y - b.min.y) * unit_scale;
        // A zero-height TEXT is invalid and some readers reject the file
        // over it, so a degenerate box is skipped rather than written.
        // `is_finite` as well as `> 0`: the box is finite by the check
        // above, but `unit_scale` is caller-supplied and a non-finite scale
        // would make the product NaN, which `<= 0.0` alone does not catch.
        if !height.is_finite() || height <= 0.0 {
            outcome.unreadable_text += 1;
            continue;
        }
        entity_head(out, "TEXT", h, "AcDbText", TEXT_LAYER);
        out.push_str(&format!(
            " 10\n{:.6}\n 20\n{:.6}\n 30\n0.0\n 40\n{height:.6}\n  1\n{s}\n  7\n{TEXT_STYLE}\n",
            b.min.x * unit_scale,
            b.min.y * unit_scale,
        ));
        // TEXT carries the AcDbText marker TWICE — once before the data
        // above and once before the second alignment point's group 73.
        // Both are emitted because a reader that looks for the second and
        // does not find it reports the same "missing subclass" error the
        // R12 mistake produced.
        out.push_str("100\nAcDbText\n 73\n     0\n");
        outcome.text_entities += 1;
    }
}

/// Make a decoded run safe to carry in a DXF group-1 string.
///
/// DXF is a line-oriented format: a newline inside a value ends the value
/// and the next line is read as a group code. A run containing one would
/// not corrupt the string, it would **desynchronise the entire rest of the
/// file** — every subsequent entity misparsed. Control characters are
/// therefore replaced with spaces rather than escaped.
///
/// Truncated at 255 characters, DXF's limit for a single `TEXT` value.
/// Truncation is silent here because the alternative — counting it as a
/// disclosure — would fire on decorative rules and separators that are not
/// text an operator is trying to read, and a disclosure that cries wolf
/// gets ignored (the `check-ui-strings.sh` lesson, in a different dialect).
fn sanitize_text(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(255)
        .collect::<String>()
        .trim_end()
        .to_string()
}

/// Issues the unique hexadecimal handles R2000 requires on every entity.
///
/// Starts at `0x100` so it cannot collide with the fixed table handles
/// above, and `$HANDSEED` is written well above anything this can reach
/// for a page of realistic size.
struct Handles(u32);

impl Handles {
    fn next(&mut self) -> String {
        self.0 += 1;
        format!("{:X}", self.0)
    }
}

/// The group codes every R2000 entity opens with: type, handle, the
/// `AcDbEntity` subclass marker, then the layer.
///
/// One function because the order matters and getting it wrong produces
/// exactly the error that caught the R12 mistake — a reader looking for a
/// subclass marker that is not where it must be.
/// Note the group codes are right-justified in **three** columns: `  0`,
/// `  5`, `  8`, and `100` with **no** leading space. A four-character
/// ` 100` is not the same token, and a reader that finds one where a
/// subclass marker belongs reports the marker as missing — which is
/// exactly the error message the R12 mistake produced, from a different
/// cause. Two ways to earn the same symptom is worth one comment.
///
/// `layer` is a parameter rather than the constant `0` it began as
/// because text goes on [`TEXT_LAYER`] — see that constant for why.
fn entity_head(out: &mut String, kind: &str, h: &mut Handles, subclass: &str, layer: &str) {
    out.push_str(&format!(
        "  0\n{kind}\n  5\n{}\n100\nAcDbEntity\n  8\n{layer}\n100\n{subclass}\n",
        h.next()
    ));
}

/// Every entity one path object contributes.
fn path_entities(
    out: &mut String,
    path: &PathObject,
    unit_scale: f64,
    opts: &DxfOptions,
    outcome: &mut DxfOutcome,
    h: &mut Handles,
) {
    for sp in &path.page_subpaths() {
        subpath_entities(out, sp, unit_scale, opts, outcome, h);
    }
}

/// One subpath, as the fewest entities that describe it honestly.
fn subpath_entities(
    out: &mut String,
    sp: &Subpath,
    s: f64,
    opts: &DxfOptions,
    outcome: &mut DxfOutcome,
    h: &mut Handles,
) {
    if sp.segments.is_empty() {
        return;
    }

    // A CLOSED subpath of four circular cubics is a circle. Recognising it
    // is what keeps forty washers at a few KB instead of 767 (see the
    // module docs) — and it is also simply the truthful entity: the
    // producer meant a circle and had no way to say so.
    if opts.fit_arcs
        && sp.closed
        && sp.segments.len() == 4
        && sp
            .segments
            .iter()
            .all(|g| matches!(g, Segment::Cubic { .. }))
        && let Some(c) = circle_fit(sp, opts.arc_tolerance)
    {
        entity_head(out, "CIRCLE", h, "AcDbCircle", GEOMETRY_LAYER);
        point3(out, 10, c.0.x * s, c.0.y * s);
        out.push_str(&format!(" 40\n{}\n", fmt(c.1 * s)));
        outcome.circles += 1;
        return;
    }

    // Otherwise walk the segments. Straight runs accumulate into ONE
    // polyline — a closed rectangle should be one entity, not four lines —
    // and each curve interrupts to emit its own arc or spline.
    let mut run: Vec<Point> = vec![sp.start];
    let mut cursor = sp.start;
    for seg in &sp.segments {
        match seg {
            Segment::Line { to } => {
                run.push(*to);
                cursor = *to;
            }
            Segment::Cubic { c1, c2, to } => {
                flush_run(out, &mut run, false, s, outcome, h);
                if opts.fit_arcs
                    && let Some((centre, radius, a0, a1)) =
                        arc_fit(cursor, *c1, *c2, *to, opts.arc_tolerance)
                {
                    // An ARC declares AcDbCircle FIRST and then AcDbArc: it
                    // is a circle plus a sweep, and the order is fixed.
                    entity_head(out, "ARC", h, "AcDbCircle", GEOMETRY_LAYER);
                    point3(out, 10, centre.x * s, centre.y * s);
                    out.push_str(&format!(" 40\n{}\n", fmt(radius * s)));
                    out.push_str("100\nAcDbArc\n");
                    out.push_str(&format!(" 50\n{}\n", fmt(a0.to_degrees())));
                    out.push_str(&format!(" 51\n{}\n", fmt(a1.to_degrees())));
                    outcome.arcs += 1;
                } else {
                    spline(out, cursor, *c1, *c2, *to, s, h);
                    outcome.splines += 1;
                }
                cursor = *to;
                run = vec![*to];
            }
        }
    }
    flush_run(out, &mut run, sp.closed, s, outcome, h);
}

/// Emit an accumulated straight run as one `LWPOLYLINE`.
///
/// `closed` sets group code 70 bit 1 — the closing edge is a FLAG, not a
/// repeated first vertex. The RAG is explicit about this (`close=True`,
/// "don't repeat pt0"): a duplicated vertex reads to a CAM table as a
/// zero-length segment, which some controllers treat as a pierce.
fn flush_run(
    out: &mut String,
    run: &mut Vec<Point>,
    closed: bool,
    s: f64,
    outcome: &mut DxfOutcome,
    h: &mut Handles,
) {
    if run.len() < 2 {
        run.clear();
        return;
    }
    entity_head(out, "LWPOLYLINE", h, "AcDbPolyline", GEOMETRY_LAYER);
    out.push_str(&format!(" 90\n{:8}\n", run.len()));
    out.push_str(&format!(" 70\n{:6}\n", i32::from(closed)));
    for p in run.iter() {
        out.push_str(&format!(" 10\n{}\n 20\n{}\n", fmt(p.x * s), fmt(p.y * s)));
    }
    outcome.polylines += 1;
    run.clear();
}

/// A cubic Bézier as a degree-3 `SPLINE` with its four control points.
///
/// Exact rather than flattened: a cubic Bézier IS a degree-3 NURBS with a
/// clamped knot vector, so this loses nothing, and it is the reason curves
/// that are not arcs still do not bloat the file.
fn spline(out: &mut String, p0: Point, c1: Point, c2: Point, p3: Point, s: f64, h: &mut Handles) {
    entity_head(out, "SPLINE", h, "AcDbSpline", GEOMETRY_LAYER);
    // 70: 8 = planar. 71: degree. 72: knots. 73: control points. 74: fit pts.
    out.push_str(" 70\n     8\n 71\n     3\n 72\n     8\n 73\n     4\n 74\n     0\n");
    for k in [0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0] {
        out.push_str(&format!(" 40\n{}\n", fmt(k)));
    }
    for p in [p0, c1, c2, p3] {
        out.push_str(&format!(
            " 10\n{}\n 20\n{}\n 30\n0.0\n",
            fmt(p.x * s),
            fmt(p.y * s)
        ));
    }
}

/// Try to read a cubic as a circular arc, returning
/// `(centre, radius, start_angle, end_angle)` in radians, CCW.
///
/// # The test, and why it is sampled rather than solved
///
/// A cubic is a circular arc only in the limit; PDF producers emit the
/// standard kappa approximation, which is *near* circular but never
/// exactly so. So there is no algebraic identity to check. Instead the
/// candidate centre is taken from the perpendicular bisectors of the
/// chord and the control polygon, and the curve is then **sampled** and
/// every sample required to lie within `tol` of that circle.
///
/// That is the honest test: it asks whether this curve *is* an arc to the
/// precision anybody can draw, rather than whether it was constructed by
/// one particular formula.
fn arc_fit(p0: Point, c1: Point, c2: Point, p3: Point, tol: f64) -> Option<(Point, f64, f64, f64)> {
    // Centre from the intersection of the perpendicular bisectors of the
    // start and end tangent chords. Degenerate (collinear) input gives no
    // intersection, which is the correct answer for a straight-ish curve.
    let centre = normal_intersection(p0, c1, c2, p3)?;
    let r = dist(centre, p0);
    if !r.is_finite() || r <= f64::EPSILON {
        return None;
    }
    // Sample the curve, including the ends. 9 samples is enough to reject a
    // curve that merely touches the circle at its endpoints — the failure
    // mode a 2-point check would admit.
    for i in 0..=8 {
        let t = f64::from(i) / 8.0;
        let p = cubic_at(p0, c1, c2, p3, t);
        if (dist(centre, p) - r).abs() > tol {
            return None;
        }
    }
    let a0 = (p0.y - centre.y).atan2(p0.x - centre.x);
    let a1 = (p3.y - centre.y).atan2(p3.x - centre.x);
    // DXF ARC is always counter-clockwise from 50 to 51. A clockwise PDF
    // arc is the same geometry with the angles swapped — emitting them in
    // the drawn order would silently produce the COMPLEMENTARY arc, which
    // looks like a correct file and cuts the wrong shape.
    let mid = cubic_at(p0, c1, c2, p3, 0.5);
    let ccw = cross(sub(p0, centre), sub(mid, centre)) > 0.0;
    Some(if ccw {
        (centre, r, a0, a1)
    } else {
        (centre, r, a1, a0)
    })
}

/// Try to read a closed 4-cubic subpath as one full circle.
fn circle_fit(sp: &Subpath, tol: f64) -> Option<(Point, f64)> {
    let mut cursor = sp.start;
    let mut centre: Option<Point> = None;
    let mut radius = 0.0;
    for seg in &sp.segments {
        let Segment::Cubic { c1, c2, to } = seg else {
            return None;
        };
        let (c, r, _, _) = arc_fit(cursor, *c1, *c2, *to, tol)?;
        match centre {
            None => {
                centre = Some(c);
                radius = r;
            }
            // All four quadrants must agree on ONE centre and radius.
            // Without this a rounded rectangle — four genuine arcs at four
            // different centres — would be emitted as a circle.
            Some(prev) => {
                if dist(prev, c) > tol || (radius - r).abs() > tol {
                    return None;
                }
            }
        }
        cursor = *to;
    }
    centre.map(|c| (c, radius))
}

// ---------------------------------------------------------------------------
// Small geometry helpers
// ---------------------------------------------------------------------------

fn sub(a: Point, b: Point) -> Point {
    Point::new(a.x - b.x, a.y - b.y)
}

fn cross(a: Point, b: Point) -> f64 {
    a.x * b.y - a.y * b.x
}

fn dist(a: Point, b: Point) -> f64 {
    (a.x - b.x).hypot(a.y - b.y)
}

fn cubic_at(p0: Point, c1: Point, c2: Point, p3: Point, t: f64) -> Point {
    let u = 1.0 - t;
    let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
    Point::new(
        a * p0.x + b * c1.x + c * c2.x + d * p3.x,
        a * p0.y + b * c1.y + c * c2.y + d * p3.y,
    )
}

/// Where the NORMALS at the two endpoints meet — the centre a circular
/// arc with those tangents would have.
///
/// # The mistake this function was written with, kept as a warning
///
/// It first intersected the perpendicular **bisectors of the chords**
/// `p0→c1` and `c2→p3`. That is the classic construction for a circle
/// through three *points*, and it is the wrong tool here: `c1` is not a
/// point on the curve, it is a control point defining the **tangent**.
///
/// For a circle the centre lies on the line through `p0` **perpendicular
/// to the tangent at `p0`** — the normal — not on the bisector of the
/// chord to a control point. On a kappa quarter-circle the two differ by
/// half the control offset, so every arc failed to fit and every hole
/// would have been emitted as splines. Caught by the first test that
/// exported a circle.
///
/// A tangent line at `p0` with direction `d` gives the normal as the
/// locus of `X` where `d · (X − p0) = 0`.
fn normal_intersection(p0: Point, c1: Point, c2: Point, p3: Point) -> Option<Point> {
    let d1 = sub(c1, p0); // tangent leaving the start
    let d2 = sub(p3, c2); // tangent arriving at the end
    let det = d1.x * d2.y - d1.y * d2.x;
    if det.abs() < 1e-12 {
        return None; // parallel tangents — a straight run, not an arc
    }
    let k1 = d1.x * p0.x + d1.y * p0.y;
    let k2 = d2.x * p3.x + d2.y * p3.y;
    Some(Point::new(
        (k1 * d2.y - d1.y * k2) / det,
        (d1.x * k2 - k1 * d2.x) / det,
    ))
}

fn extents(model: &PageObjects) -> Option<Bounds> {
    let mut it = model.objects.iter().map(VectorObject::page_bbox);
    let first = it.next()?;
    Some(it.fold(first, |acc, b| acc.union(b)))
}

/// A 3D point as group codes `n`, `n+10`, `n+20`.
fn point3(out: &mut String, code: i32, x: f64, y: f64) {
    out.push_str(&format!(
        "{:3}\n{}\n{:3}\n{}\n{:3}\n0.0\n",
        code,
        fmt(x),
        code + 10,
        fmt(y),
        code + 20
    ));
}

/// Format a coordinate.
///
/// Six decimals, with a decimal point always present. DXF readers accept
/// integers, but a bare `10` beside `10.5` reads as two different types to
/// some parsers, and this project has already been bitten once by exactly
/// that (a byte-assertion that had to match `"45.0"`, not `"45"`, because
/// the serializer always writes a point).
fn fmt(v: f64) -> String {
    let s = format!("{v:.6}");
    if s.contains('.') { s } else { format!("{s}.0") }
}

// ---------------------------------------------------------------------------
// Deriving the export scale from the measure tool (`Pass 52.2` substrate)
// ---------------------------------------------------------------------------

/// One dimension group's opinion about what scale the drawing is at.
#[derive(Debug, Clone, PartialEq)]
pub struct DxfScaleCandidate {
    /// The group's operator-facing name, so a conflict can be reported in
    /// the operator's own words rather than as a bare number.
    pub group: String,
    /// Real-world units per paper unit — [`DxfOptions::scale`] directly.
    pub scale: f64,
    /// The units that group measures in, mapped to what DXF can say.
    pub units: DxfUnits,
}

/// What pdfcer can INFER about a page's drawing scale from the ce dimensions
/// already on it.
///
/// # Why this is a three-case type and not an `Option<f64>`
///
/// Rule 4 (*fuzzy, never sneaky*): a value pdfcer inferred must be visible
/// before it becomes document state, and rejectable. An `Option<f64>` can
/// express *"here is a number"* and *"here is no number"* — but the case
/// that actually hurts is the third one, and it collapses into `None`
/// where nobody can see it.
///
/// The three cases ask genuinely different things of the operator:
///
/// - [`Self::Uncalibrated`] — pdfcer has **no idea**. Exporting at `1.0`
///   anyway is the exact trap this whole feature exists to avoid: a 1:2
///   detail arrives at half size and looks entirely plausible. The caller
///   must say so, not quietly pick 1.
/// - [`Self::Calibrated`] — pdfcer has an answer and can pre-fill it. This
///   is an inference, so it is shown before it is used.
/// - [`Self::Conflicting`] — two groups disagree about what scale the same
///   page is at. There is no correct automatic answer: a sheet with a 1:1
///   plan and a 1:5 detail is a normal drawing, and DXF has one scale.
///   Picking the first would export half the sheet wrong, silently.
///
/// Making the third case a variant means a caller cannot handle it by
/// accident — it has to be matched.
#[derive(Debug, Clone, PartialEq)]
pub enum DxfScaleSuggestion {
    /// No group has a scale set, so nothing can be inferred.
    Uncalibrated,
    /// Every calibrated group agrees.
    Calibrated {
        /// Real-world units per paper unit — [`DxfOptions::scale`].
        scale: f64,
        /// The DXF units implied by the group's own unit.
        units: DxfUnits,
        /// The group the figure came from, named so the disclosure can say
        /// *where* the number is from rather than only what it is.
        group: String,
        /// How many calibrated groups agreed on it. More than one is
        /// corroboration worth showing; it is not a different answer.
        agreeing: usize,
    },
    /// Calibrated groups disagree — every candidate, in group order.
    Conflicting {
        /// Each distinct opinion, first-seen order preserved so the list is
        /// stable across calls and does not reshuffle under the operator.
        candidates: Vec<DxfScaleCandidate>,
    },
}

impl DxfUnits {
    /// The DXF unit that best carries `unit`.
    ///
    /// Feet and metres have `$INSUNITS` codes of their own, but this
    /// writer emits only inches and millimetres, so each is mapped onto
    /// whichever of those shares its measurement system. The NUMBERS stay
    /// exact either way — [`DxfScaleSuggestion`]'s scale is dimensionless
    /// (real units per paper unit) and `per_point` supplies the rest — so
    /// this choice affects only what the header declares, never the
    /// geometry.
    #[must_use]
    pub const fn for_unit(unit: crate::dimension::Unit) -> Self {
        use crate::dimension::Unit;
        match unit {
            Unit::Millimeter | Unit::Centimeter | Unit::Meter => Self::Millimetres,
            Unit::Inch | Unit::DecimalFeet | Unit::FeetInches => Self::Inches,
        }
    }
}

/// How close two groups' scales must be to count as the same answer.
///
/// Relative, not absolute: a 1:100 site plan and a 1:1 detail differ by two
/// orders of magnitude, and one absolute epsilon cannot serve both.
const SCALE_AGREEMENT_TOLERANCE: f64 = 1e-9;

/// Infer the drawing scale for a DXF export from a page's ce dimensions.
///
/// # The conversion, and why it is unit-independent
///
/// [`ScaleState::effective_scale`](crate::dimension::ScaleState::effective_scale)
/// answers *"how many of the group's display units is one PDF point?"* —
/// which is a different question for a millimetre group than for an inch
/// group even when both describe the same 1:2 drawing.
///
/// Dividing by the unit's own true-scale baseline cancels the unit out and
/// leaves a **dimensionless drawing scale**: real units per paper unit,
/// `1.0` at full size and `2.0` on a 1:2 view. That is exactly
/// [`DxfOptions::scale`], and it is what makes two groups measuring the
/// same sheet in different units comparable at all — without it, a
/// millimetre group and an inch group describing one 1:1 drawing would
/// look like a conflict.
///
/// # What it deliberately does not do
///
/// It does not pick a winner when groups disagree, and it does not fall
/// back to `1.0` when nothing is calibrated. Both would be pdfcer quietly
/// deciding something it does not know — see [`DxfScaleSuggestion`].
///
/// # Scope: this reads the WHOLE document
///
/// Every group in the model is consulted, including groups whose
/// dimensions live on pages this export will not touch. That is the right
/// answer only when the caller genuinely means "this document"; a caller
/// exporting **one page** wants
/// [`suggest_scale_for_groups`] with that page's own groups, or a sheet
/// set whose page 3 is a 1:5 detail will either refuse a perfectly
/// unambiguous page-1 export or — worse, when page 1 has no calibration of
/// its own — silently export it at page 3's scale. See
/// [`crate::edit::EditSession::dimension_groups_on_page`] for the
/// page-ownership resolution that produces the id list.
#[must_use]
pub fn suggest_scale(model: &crate::dimension::DimensionModel) -> DxfScaleSuggestion {
    suggest_scale_from(model.groups().iter())
}

/// Infer the drawing scale from **only** the named ce dimension groups.
///
/// The page-scoped sibling of [`suggest_scale`], and the one a shell
/// exporting a specific page (or a specific selection of pages) should
/// call. `groups` is normally
/// [`EditSession::dimension_groups_on_page`](crate::edit::EditSession::dimension_groups_on_page)
/// for the page being exported — the union of that call over several pages
/// when several are being exported at once, in which case a
/// [`DxfScaleSuggestion::Conflicting`] result is exactly the statement
/// *"these pages are not all at one scale, so one DXF scale cannot serve
/// them"*.
///
/// Ids not present in `model` are ignored rather than an error: a stale id
/// describes a group that no longer exists, which is the same amount of
/// evidence as no group at all.
///
/// An **empty** `groups` yields [`DxfScaleSuggestion::Uncalibrated`],
/// which is the truthful answer — a page carrying no ce dimensions
/// supplies no evidence about its scale. It is deliberately not
/// distinguished from "carries dimensions, none calibrated": both mean
/// pdfcer does not know, and both must be disclosed the same way.
#[must_use]
pub fn suggest_scale_for_groups(
    model: &crate::dimension::DimensionModel,
    groups: &[crate::dimension::GroupId],
) -> DxfScaleSuggestion {
    suggest_scale_from(model.groups().iter().filter(|g| groups.contains(&g.id)))
}

/// The shared body of [`suggest_scale`] and [`suggest_scale_for_groups`].
///
/// Written over an iterator rather than over a slice so the page-scoped
/// entry point filters instead of allocating a second `Vec<Group>` — and,
/// more importantly, so there is exactly ONE implementation of the
/// unit-cancellation and the agreement test. Two copies of this arithmetic
/// would be two chances for a document-wide and a page-scoped export of
/// the same single-page document to disagree about its scale.
fn suggest_scale_from<'a>(
    groups: impl Iterator<Item = &'a crate::dimension::Group>,
) -> DxfScaleSuggestion {
    let mut candidates: Vec<DxfScaleCandidate> = Vec::new();
    let mut agreeing = 0usize;

    for group in groups {
        let unit = group.format.unit;
        let Some(per_point) = group.scale.effective_scale(unit) else {
            continue; // NeverSet — this group has no opinion
        };
        let baseline = unit.baseline_per_point();
        // A zero or non-finite baseline cannot happen for any `Unit`, but
        // the division is guarded rather than trusted: a NaN reaching
        // `DxfOptions::scale` would multiply every coordinate in the file.
        if !baseline.is_finite() || baseline <= 0.0 {
            continue;
        }
        let scale = per_point / baseline;
        if !scale.is_finite() || scale <= 0.0 {
            continue;
        }
        agreeing += 1;
        // Compared against what is already recorded rather than sorted
        // afterwards, so first-seen order survives and the list a caller
        // shows does not reorder itself between calls.
        let known = candidates
            .iter()
            .any(|c| (c.scale - scale).abs() <= SCALE_AGREEMENT_TOLERANCE * scale.abs().max(1.0));
        if !known {
            candidates.push(DxfScaleCandidate {
                group: group.name.clone(),
                scale,
                units: DxfUnits::for_unit(unit),
            });
        }
    }

    // Matched as a SLICE PATTERN rather than on `len()` plus an index: the
    // one-candidate arm then binds the element itself, so there is no
    // indexing operation for the reader (or `clippy::indexing_slicing`) to
    // have to prove cannot panic.
    match candidates.as_slice() {
        [] => DxfScaleSuggestion::Uncalibrated,
        [only] => DxfScaleSuggestion::Calibrated {
            scale: only.scale,
            units: only.units,
            group: only.group.clone(),
            agreeing,
        },
        _ => DxfScaleSuggestion::Conflicting { candidates },
    }
}
