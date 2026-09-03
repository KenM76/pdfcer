//! # Annotation appearance authoring — geometric markup (ISO 32000-1 §12.5.6)
//!
//! The **generation half** of Pass 6.1: turn a geometric-markup
//! *specification* (a Square's rectangle, an Ink annotation's strokes, a
//! Highlight's quadrilaterals) into the two things a conforming annotation
//! needs — the annotation dictionary (Table 164 + the per-subtype geometry
//! keys of §12.5.6) and a fully-baked `/AP` `/N` appearance form XObject
//! (§8.10 + §12.5.5). It emits **no page content** (R47): everything here
//! becomes new indirect objects that [`crate::edit::EditSession`] wires
//! into the document.
//!
//! Decision 005's axis (R26) still holds: this module **models and
//! generates**; it does not paint. The appearance it generates is written
//! into the saved file as a real `/AP` (R44) and painted by the *existing*
//! Pass 6.0 read path — pdfcer never carries a private rendering of its own
//! authored annotation.
//!
//! ## Why a baked `/AP`, always (R44 + the Acrobat parity contrast)
//!
//! `markup__appearance_stream_generation.md`: *"every annotation Acrobat
//! authors gets a fully-baked `/AP` written at creation time; it does not
//! rely on a consuming reader to synthesise appearance from the cosmetic
//! keys."* pdfcer matches that on the **authoring** side even though it is
//! deliberately more lenient than Acrobat on the **reading** side. So this
//! module writes the geometry keys (`/L`, `/Vertices`, `/InkList`,
//! `/QuadPoints`, `/C`, `/IC`, …) **and** a complete `/AP` — the keys for
//! other tools that re-generate, the `/AP` because it is the only thing
//! pdfcer (and, per the RAG, a strict Acrobat) reliably paints.
//!
//! ## Placement discipline — 1:1, no distortion (§12.5.5 / §8.10 W-B)
//!
//! Every appearance here is authored with `Matrix` = identity and
//! `BBox` = the annotation `/Rect`, with all geometry drawn in **absolute
//! default-user-space (page) coordinates**. Under the §12.5.5 algorithm
//! that makes the transformed appearance box equal `Rect`, matrix **A** a
//! pure identity, and **AA = identity** — so content renders 1:1 with no
//! anisotropic stretch (§12.5.5's aspect-mismatch trap is avoided by
//! construction, W-B's recommended pattern). The one invariant this
//! demands: the computed `Rect` must have **strictly positive width and
//! height** (WF4 — a degenerate `/AP` `BBox` is a §12.5.5 NEGATIVE RESULT),
//! which [`positive_rect`] guarantees.
//!
//! ## QuadPoints ordering policy (the open spec item — DECIDED here)
//!
//! §12.5.6.10 specifies `/QuadPoints` corner order, but the historical
//! spec text and real producers disagree, and pdfcer Pass 6.0 filed the
//! conflict as an open `C:\personal_rag\pdf` finding (*"QuadPoints
//! CCW-vs-Z-order unresolved — the spec says CCW but real producers /
//! Acrobat emit Z / reading order; only bites 6.1 generation"*). Pass 6.1
//! **decides it**: pdfcer authors quads in **Z / reading order** —
//! `(x1,y1)` upper-left, `(x2,y2)` upper-right, `(x3,y3)` lower-left,
//! `(x4,y4)` lower-right — the dominant convention emitted by Acrobat,
//! PDFBox and pdf.js, chosen to maximise interoperability with the readers
//! most files are consumed by. See [`Quad`] and
//! [`text_markup_quad_object`]. Because pdfcer bakes a full `/AP` (R44), its
//! own rendering does not depend on any reader's quad interpretation; the
//! order is chosen purely for third-party consumers that re-derive from
//! `/QuadPoints`.
//!
//! ## Default appearances (Acrobat parity, `markup__*` RAG)
//!
//! - **Highlight** — default yellow, and painted with blend mode
//!   **Multiply** (`/BM /Multiply` on the annotation *and* a `/GS0 gs`
//!   ExtGState in the appearance's own `/Resources`) so overlapping
//!   highlights do not darken the way naive alpha compositing would
//!   (`markup__text_markup_quadpoints.md`, the one well-corroborated
//!   blend fact — locked into Pass 6.1 as an acceptance detail).
//! - **Line** — default line-ending style **Open** at both ends
//!   (`markup__shape_and_line_annotations.md`, a sourced default).
//! - **Square / Circle / Underline / StrikeOut / Squiggly** defaults are a
//!   documented **GAP** in the Acrobat RAG (single-weak-source or absent);
//!   pdfcer's chosen defaults are named at each constructor as pdfcer's own
//!   contract, not a claimed Acrobat match.

use crate::graph::ObjectGraph;
use crate::object::{Dict, Name, Object};
use crate::page_tree::Rect;
use crate::settings::QuadPointOrder;
use crate::writer::content::{ContentBuilder, LineCap, LineJoin, Paint};

/// A device colour for a markup annotation's stroke/fill and its `/C` /
/// `/IC` arrays (§12.5.6). The array length selects the device colour
/// space exactly as §8.6.3 does: 1 → DeviceGray, 3 → DeviceRGB,
/// 4 → DeviceCMYK (0 → "no colour / transparent", handled by the callers
/// that accept `Option<Color>`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Color {
    /// DeviceGray (0.0 = black, 1.0 = white).
    Gray(f64),
    /// DeviceRGB, each channel 0.0–1.0.
    Rgb(f64, f64, f64),
    /// DeviceCMYK, each channel 0.0–1.0.
    Cmyk(f64, f64, f64, f64),
}

impl Color {
    /// The `/C` / `/IC` array form (§12.5.6): 1, 3 or 4 numbers.
    fn to_array(self) -> Object {
        let nums = match self {
            Self::Gray(g) => vec![g],
            Self::Rgb(r, g, b) => vec![r, g, b],
            Self::Cmyk(c, m, y, k) => vec![c, m, y, k],
        };
        Object::Array(nums.into_iter().map(Object::Real).collect())
    }

    /// Apply as the stroking colour in the content stream (§8.6 device
    /// operators). Must be emitted before the path (W-E).
    fn apply_stroke(self, b: &mut ContentBuilder) {
        match self {
            Self::Gray(g) => b.set_stroke_gray(g),
            Self::Rgb(r, g, bl) => b.set_stroke_rgb(r, g, bl),
            Self::Cmyk(c, m, y, k) => b.set_stroke_cmyk(c, m, y, k),
        }
    }

    /// Apply as the non-stroking (fill) colour in the content stream.
    fn apply_fill(self, b: &mut ContentBuilder) {
        match self {
            Self::Gray(g) => b.set_fill_gray(g),
            Self::Rgb(r, g, bl) => b.set_fill_rgb(r, g, bl),
            Self::Cmyk(c, m, y, k) => b.set_fill_cmyk(c, m, y, k),
        }
    }
}

/// One text-markup quadrilateral (`/QuadPoints` group, §12.5.6.10),
/// authored in the Z / reading-order convention documented in the module
/// docs: upper-left, upper-right, lower-left, lower-right.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quad {
    /// Upper-left `(x, y)`.
    pub ul: (f64, f64),
    /// Upper-right `(x, y)`.
    pub ur: (f64, f64),
    /// Lower-left `(x, y)`.
    pub ll: (f64, f64),
    /// Lower-right `(x, y)`.
    pub lr: (f64, f64),
}

impl Quad {
    /// A quad covering an axis-aligned rectangle — the marquee/dragged
    /// case (`markup__text_markup_quadpoints.md`: a Highlight over an
    /// image-only page is a geometry-only quad matching the dragged rect).
    #[must_use]
    pub fn from_rect(rect: Rect) -> Self {
        Self {
            ul: (rect.llx, rect.ury),
            ur: (rect.urx, rect.ury),
            ll: (rect.llx, rect.lly),
            lr: (rect.urx, rect.lly),
        }
    }

    /// The eight `/QuadPoints` numbers in the authored (Z-order) sequence:
    /// `x1 y1 x2 y2 x3 y3 x4 y4` = UL, UR, LL, LR.
    /// The eight numbers `/QuadPoints` carries, in `order` (§12.5.6.10).
    ///
    /// The two orders differ **only in the last two corners** — reading order
    /// ends `LL, LR`, the counterclockwise walk ends `LR, LL`. That is the
    /// whole of ambiguity `QP-A1`, and it is why the wrong one draws a
    /// bow-tie: swapping the bottom pair crosses the edges.
    fn points(self, order: QuadPointOrder) -> [f64; 8] {
        let (a, b) = match order {
            QuadPointOrder::ReadingOrder => (self.ll, self.lr),
            QuadPointOrder::Counterclockwise => (self.lr, self.ll),
        };
        [
            self.ul.0, self.ul.1, self.ur.0, self.ur.1, a.0, a.1, b.0, b.1,
        ]
    }

    /// The axis-aligned bounds of this quad's four corners.
    fn bounds(self) -> Rect {
        let xs = [self.ul.0, self.ur.0, self.ll.0, self.lr.0];
        let ys = [self.ul.1, self.ur.1, self.ll.1, self.lr.1];
        Rect {
            llx: xs.iter().copied().fold(f64::INFINITY, f64::min),
            lly: ys.iter().copied().fold(f64::INFINITY, f64::min),
            urx: xs.iter().copied().fold(f64::NEG_INFINITY, f64::max),
            ury: ys.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        }
    }
}

/// Which text-markup subtype a [`MarkupSpec::TextMarkup`] authors
/// (§12.5.6.10). All four share the `/QuadPoints` geometry model and
/// differ only in what the appearance draws over each quad.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextMarkupKind {
    /// `/Highlight` — a translucent colour wash (Multiply blend).
    Highlight,
    /// `/Underline` — a line near the quad's baseline.
    Underline,
    /// `/StrikeOut` — a line through the quad's vertical middle.
    StrikeOut,
    /// `/Squiggly` — a wavy line at the quad's baseline. pdfcer authors
    /// this natively even though Acrobat's own UI does not (a deliberate
    /// exceed-Acrobat choice; the subtype is fully spec-legal and Acrobat
    /// displays it — `markup__text_markup_quadpoints.md`).
    Squiggly,
}

impl TextMarkupKind {
    /// The `/Subtype` name bytes (§12.5.6.10).
    const fn subtype(self) -> &'static [u8] {
        match self {
            Self::Highlight => b"Highlight",
            Self::Underline => b"Underline",
            Self::StrikeOut => b"StrikeOut",
            Self::Squiggly => b"Squiggly",
        }
    }
}

/// A geometric-markup annotation to author. The single input to
/// [`build_appearance`]; each variant carries exactly the geometry and
/// cosmetic properties its subtype needs, with defaults documented at the
/// convenience constructors.
///
/// Deliberately **no text-bearing variant** — FreeText/Text/Stamp are
/// Pass 6.2 (decision 008), kept out of 6.1 so the authoring infrastructure
/// is exercised without variable-text confounding it.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum MarkupSpec {
    /// `/Square` (§12.5.6.8) — an axis-aligned rectangle.
    Square {
        /// The rectangle in default user space.
        rect: Rect,
        /// Border (stroke) colour `/C`, or `None` for no border.
        border: Option<Color>,
        /// Interior (fill) colour `/IC`, or `None` for a transparent
        /// interior.
        interior: Option<Color>,
        /// Border width `/BS /W` in points.
        border_width: f64,
        /// Cloudy border effect `/BE` (§12.5.4 Table 167), or `None` for
        /// an ordinary straight border.
        ///
        /// `Some(i)` writes `/BE << /S /C /I i >>` and bakes a scalloped
        /// appearance; `None` omits `/BE` entirely. There is no third
        /// state: `/S /S` *is* the "no effect" value and it is the
        /// default, so writing it would add a key that changes nothing.
        ///
        /// **`i` is a continuous intensity in `0.0..=2.0`** — see
        /// [`Self::Cloud`]'s `intensity` for why that is a range and not
        /// the enumeration it is often assumed to be.
        ///
        /// ★ **This is the half of revision-cloud support that carries the
        /// value.** *Drag a box, make it cloudy* is the gesture reviewers
        /// actually perform; a cloud from a free-form vertex list is the
        /// rarer case. One optional key on a subtype pdfcer already authors
        /// reaches the common workflow.
        border_effect: Option<f64>,
    },
    /// `/Circle` (§12.5.6.9) — an ellipse inscribed in `rect`.
    Circle {
        /// The bounding rectangle of the ellipse.
        rect: Rect,
        /// Border (stroke) colour `/C`.
        border: Option<Color>,
        /// Interior (fill) colour `/IC`.
        interior: Option<Color>,
        /// Border width in points.
        border_width: f64,
    },
    /// `/Line` (§12.5.6.7) — a single segment, optionally arrow-headed.
    Line {
        /// Start point `(x, y)` (`/L` first pair).
        start: (f64, f64),
        /// End point `(x, y)` (`/L` second pair).
        end: (f64, f64),
        /// Stroke colour `/C`.
        color: Color,
        /// Line width in points.
        width: f64,
        /// Line-ending style at each end (`/LE`). Default Open/Open.
        endings: (LineEnding, LineEnding),
    },
    /// `/Ink` (§12.5.6.12) — one or more freehand strokes (`/InkList`).
    Ink {
        /// Each inner vector is one continuous stroke's point list.
        strokes: Vec<Vec<(f64, f64)>>,
        /// Stroke colour `/C`.
        color: Color,
        /// Uniform stroke width in points (Acrobat's Ink is single-width;
        /// no pressure variation — `markup__ink_annotation.md`).
        width: f64,
    },
    /// `/Polygon` (§12.5.6.13) — a closed multi-segment shape
    /// (`/Vertices`).
    Polygon {
        /// The vertices in order; the shape closes back to the first.
        vertices: Vec<(f64, f64)>,
        /// Border (stroke) colour `/C`.
        border: Option<Color>,
        /// Interior (fill) colour `/IC`.
        interior: Option<Color>,
        /// Border width in points.
        width: f64,
    },
    /// A **revision cloud** — a `/Polygon` (§12.5.6.9) carrying the cloudy
    /// border effect `/BE << /S /C /I n >>` (§12.5.4, Table 167).
    ///
    /// # Why this is a `/Polygon` and not a subtype of its own
    ///
    /// There is no `/Cloud` annotation subtype in ISO 32000. A revision
    /// cloud **is** a polygon whose border is drawn cloudy, and Table 181
    /// declares `/BE` on Polygon/PolyLine with the qualifier *"meaningful
    /// only for polygon annotations"*. So this variant writes `/Subtype
    /// /Polygon` and differs from [`Self::Polygon`] only by `/BE` and by
    /// the baked appearance.
    ///
    /// # ★ `intensity` is a CONTINUOUS range, and this is worth stating
    ///
    /// Table 167's `I` row is typed **`number`** and constrains it *"in
    /// the range 0 to 2"* — **character-for-character identical in ISO
    /// 32000-1:2008 and ISO 32000-2:2020** (where the table renumbers to
    /// 169). It is routinely mis-read as the enumeration `{0, 1, 2}`.
    ///
    /// The decisive evidence is inside the same four-line table: the
    /// sibling `S` row uses the standard's **enumeration** idiom
    /// (*"Possible values are:"* + a list) while `I` uses the **range**
    /// idiom. One table, both idioms, one per row. **`/I 1.5` is
    /// conformant** and pdfcer accepts it.
    ///
    /// Values outside `0.0..=2.0`, and non-finite values, are refused by
    /// name at author time — see `EditError::BorderEffectIntensityOutOfRange`.
    Cloud {
        /// The vertices in order; the shape closes back to the first.
        /// **At least three** are required (`Pass 82.1`) — a two-vertex
        /// cloud is a line pretending to be an area.
        vertices: Vec<(f64, f64)>,
        /// Border (stroke) colour `/C`.
        border: Option<Color>,
        /// Interior (fill) colour `/IC`.
        interior: Option<Color>,
        /// Border width in points.
        width: f64,
        /// `/BE /I` — the cloud intensity, a continuous value in
        /// `0.0..=2.0`.
        intensity: f64,
    },
    /// `/PolyLine` (§12.5.6.13) — an open multi-segment path (`/Vertices`).
    PolyLine {
        /// The vertices in order; the path does **not** close.
        vertices: Vec<(f64, f64)>,
        /// Stroke colour `/C`.
        color: Color,
        /// Border width in points.
        width: f64,
    },
    /// The text-markup family (§12.5.6.10): Highlight/Underline/StrikeOut/
    /// Squiggly, all `/QuadPoints`-based.
    TextMarkup {
        /// Which of the four subtypes.
        kind: TextMarkupKind,
        /// The marked quadrilaterals (Z-order, module docs).
        quads: Vec<Quad>,
        /// The mark colour `/C`.
        color: Color,
    },
}

/// Why an existing annotation's [`MarkupSpec`] could not be read back out
/// of its dictionary.
///
/// Every variant is a **refusal to guess**. [`spec_from_dict`] exists so
/// an *existing* annotation can be restyled by regenerating its
/// appearance from its own declared geometry; if that geometry is not
/// there, or is not something pdfcer models, the only alternatives to
/// refusing are to invent geometry or to write a `/C` the appearance does
/// not honour — the first is what rule 4 forbids outright, and the second
/// produces a file whose dictionary and appearance disagree, which under
/// R43 means the operator's colour change has no visible effect at all.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum SpecReadError {
    /// The `/Subtype` is not one [`build_appearance`] can author, so pdfcer
    /// has no model of what its appearance should look like.
    ///
    /// Includes the text-bearing types (`FreeText`, `Stamp`, `Text`),
    /// interactive ones (`Widget`, `Link`), and anything non-standard.
    /// A `Widget`'s appearance belongs to the form subsystem
    /// ([`crate::forms_author`]) and must not be regenerated by the markup
    /// path.
    #[error(
        "/Subtype /{subtype} is not a markup type pdfcer can author, so its appearance cannot be \
         regenerated from its properties"
    )]
    UnsupportedSubtype {
        /// The `/Subtype` name as it appears in the file, lossily decoded
        /// for the message.
        subtype: String,
    },
    /// The geometry key this subtype's appearance is drawn from is absent,
    /// the wrong type, or has the wrong number of coordinates.
    ///
    /// Note the asymmetry with the *read* side ([`crate::annot`]), which
    /// models a malformed annotation rather than rejecting it: reading a
    /// broken annotation and reporting it is right, but **rewriting** one
    /// from geometry pdfcer could not parse would mean inventing the
    /// missing half.
    #[error(
        "the annotation's /{key} geometry is missing or malformed, so there is nothing to \
         regenerate an appearance from and pdfcer will not guess it"
    )]
    BadGeometry {
        /// The geometry key that failed — `Rect`, `L`, `InkList`,
        /// `Vertices` or `QuadPoints`.
        key: &'static str,
    },
}

/// Read a [`MarkupSpec`] back out of an existing annotation dictionary —
/// the inverse of [`build_appearance`]'s dictionary half.
///
/// # Why this exists
///
/// Changing a placed markup's colour has to change its **appearance
/// stream**, not just its `/C`: R43 paints from `/AP` or not at all, so a
/// `/C` edit alone would leave the operator's colour change invisible
/// while the file quietly claims otherwise. Regenerating the appearance
/// needs the geometry, and the geometry is in the annotation's own
/// dictionary (`/Vertices`, `/L`, `/InkList`, `/QuadPoints`, `/Rect`) —
/// exactly where §12.5.6 puts it. So this reads it back, the caller
/// applies its overrides to the returned spec, and
/// [`build_appearance`] regenerates from that.
///
/// **Nothing is inferred.** Every value comes from a key the file
/// actually carries, or from a default the standard states:
///
/// * `/BS` `/W` — Table 168's border width. Absent `/BS` falls back to
///   `/Border`'s third element, and absent both to **1.0**, which is
///   Table 164's stated default (`/Border` defaults to `[0 0 1]`).
/// * `/C` and `/IC` — §12.5.6 colour arrays, length-selected exactly as
///   §8.6.3 does (1 = DeviceGray, 3 = DeviceRGB, 4 = DeviceCMYK). A
///   **zero-length** array is the standard's own spelling of "no colour",
///   so it reads as `None`, not as an error.
/// * `/LE` — Table 176 line endings. A value outside the three pdfcer
///   authors degrades to [`LineEnding::None`] rather than failing the
///   whole read: the ending is cosmetic, the line is not, and refusing to
///   recolour a line because its arrowhead is a `/Butt` would be a refusal
///   with no upside. **This is the one lossy step**, and callers disclose
///   it rather than let it pass silently.
///
/// # Round-trip note the caller must not miss
///
/// For every subtype except `Square` and `Circle`, `/Rect` is **derived**
/// from the geometry plus a margin that depends on the stroke width
/// (see `bounds_of`). So regenerating after a width change legitimately
/// **moves `/Rect`** — that is correct, not drift. `Square` and `Circle`
/// are the exception because for them `/Rect` *is* the geometry.
///
/// # Errors
///
/// [`SpecReadError`] — an unmodelled subtype, or geometry pdfcer will not
/// guess at.
pub fn spec_from_dict<G: ObjectGraph + ?Sized>(
    graph: &G,
    annot: &Dict,
) -> Result<MarkupSpec, SpecReadError> {
    let subtype = match annot.get(b"Subtype").map(|o| graph.resolve(o)) {
        Some(Object::Name(n)) => n.as_bytes().to_vec(),
        _ => Vec::new(),
    };
    let color = |key: &[u8]| read_color(graph, annot, key);
    let width = read_border_width(graph, annot);

    match subtype.as_slice() {
        b"Square" | b"Circle" => {
            let rect = read_rect(graph, annot, b"Rect")
                .ok_or(SpecReadError::BadGeometry { key: "Rect" })?;
            let (border, interior) = (color(b"C"), color(b"IC"));
            Ok(if subtype == b"Circle" {
                MarkupSpec::Circle {
                    rect,
                    border,
                    interior,
                    border_width: width,
                }
            } else {
                MarkupSpec::Square {
                    rect,
                    border,
                    interior,
                    border_width: width,
                    // ★ READ BACK as of `Pass 98.0`. This used to be a hard
                    // `None` with a comment calling a foreign `/BE` "a
                    // separate concern from authoring one" — and the
                    // consequence was that an Acrobat-authored cloudy
                    // square, opened in pdfcer and merely RESTYLED (a colour
                    // or width change), came back a plain rectangle.
                    //
                    // Disclosed, never silent — `DroppedProperty::BorderEffect`
                    // said so — but "why did my cloud go flat?" is a
                    // question the operator should not have to ask, and the
                    // fix is one already-open dictionary key.
                    border_effect: read_border_effect(graph, annot),
                }
            })
        }
        b"Line" => {
            let nums = read_numbers(graph, annot, b"L");
            let [x1, y1, x2, y2] = nums.as_slice() else {
                return Err(SpecReadError::BadGeometry { key: "L" });
            };
            Ok(MarkupSpec::Line {
                start: (*x1, *y1),
                end: (*x2, *y2),
                // §12.5.6.7 makes /C the line's colour and Table 164 makes
                // it Optional; a line with no /C still has to draw as
                // SOMETHING, and black is what every reader falls back to.
                color: color(b"C").unwrap_or(Color::Gray(0.0)),
                width,
                endings: read_line_endings(graph, annot),
            })
        }
        b"Ink" => {
            let Some(Object::Array(list)) = annot.get(b"InkList").map(|o| graph.resolve(o)) else {
                return Err(SpecReadError::BadGeometry { key: "InkList" });
            };
            let strokes: Vec<Vec<(f64, f64)>> = list
                .iter()
                .map(|stroke| pairs_of(graph.resolve(stroke), graph))
                .filter(|s| !s.is_empty())
                .collect();
            if strokes.is_empty() {
                return Err(SpecReadError::BadGeometry { key: "InkList" });
            }
            Ok(MarkupSpec::Ink {
                strokes,
                color: color(b"C").unwrap_or(Color::Gray(0.0)),
                width,
            })
        }
        b"Polygon" | b"PolyLine" => {
            let vertices = annot
                .get(b"Vertices")
                .map(|o| pairs_of(graph.resolve(o), graph))
                .unwrap_or_default();
            if vertices.is_empty() {
                return Err(SpecReadError::BadGeometry { key: "Vertices" });
            }
            Ok(if subtype == b"Polygon" {
                // ★ A CLOUDY POLYGON RECONSTRUCTS AS A CLOUD, not as a
                // polygon that happens to have lost its border effect
                // (`Pass 98.0`).
                //
                // The distinction matters because the two take different
                // authoring paths downstream: `MarkupSpec::Polygon` bakes a
                // straight outline, so restyling a foreign revision cloud
                // used to FLATTEN it. There is no `/Cloud` subtype in ISO
                // 32000 — a cloud *is* a `/Polygon` with `/BE << /S /C >>`
                // (Table 181's "meaningful only for polygon annotations") —
                // so the shape is only recoverable by reading `/BE`.
                if let Some(intensity) = read_border_effect(graph, annot) {
                    MarkupSpec::Cloud {
                        vertices,
                        border: color(b"C"),
                        interior: color(b"IC"),
                        width,
                        intensity,
                    }
                } else {
                    MarkupSpec::Polygon {
                        vertices,
                        border: color(b"C"),
                        interior: color(b"IC"),
                        width,
                    }
                }
            } else {
                MarkupSpec::PolyLine {
                    vertices,
                    color: color(b"C").unwrap_or(Color::Gray(0.0)),
                    width,
                }
            })
        }
        b"Highlight" | b"Underline" | b"StrikeOut" | b"Squiggly" => {
            let nums = read_numbers(graph, annot, b"QuadPoints");
            if nums.is_empty() || !nums.len().is_multiple_of(8) {
                return Err(SpecReadError::BadGeometry { key: "QuadPoints" });
            }
            // §12.5.6.10's groups of eight, in the UL/UR/LL/LR order
            // `Quad::points` emits. `chunks_exact(8)` cannot yield a short
            // chunk, so the indexing below is total.
            let quads = nums
                .chunks_exact(8)
                .filter_map(|q| match q {
                    &[ulx, uly, urx, ury, llx, lly, lrx, lry] => Some(Quad {
                        ul: (ulx, uly),
                        ur: (urx, ury),
                        ll: (llx, lly),
                        lr: (lrx, lry),
                    }),
                    _ => None,
                })
                .collect();
            let kind = match subtype.as_slice() {
                b"Underline" => TextMarkupKind::Underline,
                b"StrikeOut" => TextMarkupKind::StrikeOut,
                b"Squiggly" => TextMarkupKind::Squiggly,
                _ => TextMarkupKind::Highlight,
            };
            Ok(MarkupSpec::TextMarkup {
                kind,
                quads,
                color: color(b"C").unwrap_or(Color::Gray(0.0)),
            })
        }
        other => Err(SpecReadError::UnsupportedSubtype {
            subtype: String::from_utf8_lossy(other).into_owned(),
        }),
    }
}

/// Read a `/C`-shaped colour array (§12.5.6), length-selecting the device
/// space exactly as §8.6.3 does.
///
/// A **zero-length** array reads as `None` and so does an absent key: the
/// standard defines `0` components as "no colour", so the two genuinely
/// mean the same here. Any other length is also `None` — a 2- or
/// 5-component array names no device space, and treating it as an error
/// would refuse to restyle an annotation over a key the caller is about to
/// overwrite anyway.
fn read_color<G: ObjectGraph + ?Sized>(graph: &G, annot: &Dict, key: &[u8]) -> Option<Color> {
    let nums = read_numbers(graph, annot, key);
    match *nums.as_slice() {
        [g] => Some(Color::Gray(g)),
        [r, g, b] => Some(Color::Rgb(r, g, b)),
        [c, m, y, k] => Some(Color::Cmyk(c, m, y, k)),
        _ => None,
    }
}

/// Read the border width: `/BS` `/W` (Table 168), else `/Border`'s third
/// element, else Table 164's stated default of **1.0**.
///
/// The `/Border` fallback matters on real files: Table 164 says `/Border`
/// is *"ignored if a `BS` entry is present"*, which means a file with only
/// `/Border` is the normal older-producer shape, not a malformed one.
pub(crate) fn read_border_width<G: ObjectGraph + ?Sized>(graph: &G, annot: &Dict) -> f64 {
    if let Some(Object::Dict(bs)) = annot.get(b"BS").map(|o| graph.resolve(o))
        && let Some(w) = bs
            .get(b"W")
            .map(|o| graph.resolve(o))
            .and_then(Object::as_number)
    {
        return w;
    }
    read_numbers(graph, annot, b"Border")
        .get(2)
        .copied()
        .unwrap_or(1.0)
}

/// Read `/LE` (Table 176), degrading anything outside the three pdfcer
/// authors to [`LineEnding::None`].
///
/// Degrading rather than failing: see [`spec_from_dict`]'s note — this is
/// the one lossy step, and it is lossy in a direction the caller can
/// disclose.
/// Read a cloudy border effect back out of an annotation dictionary —
/// `/BE << /S /C /I n >>` (§12.5.4, Table 167).
///
/// Returns `Some(intensity)` when the annotation carries a **cloudy** effect,
/// and `None` when it carries none, carries `/S /S` (the "no effect" default,
/// which Table 167 defines and which is why writing it would be noise), or
/// carries a style pdfcer does not author.
///
/// # Why the intensity is clamped rather than refused
///
/// This is a READER. `EditSession`'s authoring path refuses an out-of-range
/// `/I` by name (`EditError::BorderEffectIntensityOutOfRange`), because an
/// operator asking for 5.0 has made a mistake worth telling them about. A
/// FILE saying 5.0 is a different situation: the annotation exists, some
/// other producer wrote it, and refusing to read it would mean pdfcer turns a
/// non-conforming cloud into a plain polygon — losing MORE than the file got
/// wrong.
///
/// Table 167 constrains `/I` to `0..2`, so clamping to that range is the
/// standard's own answer to what the value means. The default when `/I` is
/// absent is `0`, per the same table.
///
/// # `/S` is checked, not assumed
///
/// Table 167 gives `/S` two values — `/S` (solid, i.e. no effect) and `/C`
/// (cloudy). A `/BE` present with `/S /S`, or with a name from a future
/// extension, is **not** a cloud, and reporting one would invent a shape the
/// file does not describe.
pub(crate) fn read_border_effect<G: ObjectGraph + ?Sized>(graph: &G, annot: &Dict) -> Option<f64> {
    let Object::Dict(be) = annot.get(b"BE").map(|o| graph.resolve(o))? else {
        return None;
    };
    // Table 167: /S defaults to /S (solid). Only /C is a cloud.
    match be.get(b"S").map(|o| graph.resolve(o)) {
        Some(Object::Name(n)) if n.as_bytes() == b"C" => {}
        _ => return None,
    }
    let intensity = be
        .get(b"I")
        .map(|o| graph.resolve(o))
        .and_then(|o| o.as_number())
        .filter(|v| v.is_finite())
        .unwrap_or(0.0);
    Some(intensity.clamp(0.0, 2.0))
}

fn read_line_endings<G: ObjectGraph + ?Sized>(graph: &G, annot: &Dict) -> (LineEnding, LineEnding) {
    let Some(Object::Array(items)) = annot.get(b"LE").map(|o| graph.resolve(o)) else {
        return (LineEnding::None, LineEnding::None);
    };
    let read = |i: usize| match items.get(i).map(|o| graph.resolve(o)) {
        Some(Object::Name(n)) => match n.as_bytes() {
            b"OpenArrow" => LineEnding::OpenArrow,
            b"ClosedArrow" => LineEnding::ClosedArrow,
            _ => LineEnding::None,
        },
        _ => LineEnding::None,
    };
    (read(0), read(1))
}

/// Read a key whose value is a flat array of numbers, resolving each
/// element (§7.3.10 substitutability lets any element be indirect).
///
/// Returns empty for an absent key or a non-array value; a non-numeric
/// element is skipped, which the callers then catch as a wrong-length
/// result rather than silently shifting coordinates — hence the exact
/// length matches at every call site.
fn read_numbers<G: ObjectGraph + ?Sized>(graph: &G, annot: &Dict, key: &[u8]) -> Vec<f64> {
    match annot.get(key).map(|o| graph.resolve(o)) {
        Some(Object::Array(items)) => items
            .iter()
            .filter_map(|o| graph.resolve(o).as_number())
            .collect(),
        _ => Vec::new(),
    }
}

/// Turn a flat `[x1 y1 x2 y2 …]` array object into coordinate pairs,
/// dropping a trailing unpaired value.
fn pairs_of<G: ObjectGraph + ?Sized>(obj: &Object, graph: &G) -> Vec<(f64, f64)> {
    let Some(items) = obj.as_array() else {
        return Vec::new();
    };
    let nums: Vec<f64> = items
        .iter()
        .filter_map(|o| graph.resolve(o).as_number())
        .collect();
    nums.chunks_exact(2)
        .filter_map(|c| match c {
            &[x, y] => Some((x, y)),
            _ => None,
        })
        .collect()
}

/// Read a rectangle key, normalized per §7.9.5.
fn read_rect<G: ObjectGraph + ?Sized>(graph: &G, annot: &Dict, key: &[u8]) -> Option<Rect> {
    match read_numbers(graph, annot, key).as_slice() {
        &[x1, y1, x2, y2] => Some(Rect::from_corners(x1, y1, x2, y2)),
        _ => None,
    }
}

/// A line-ending style (`/LE`, §12.5.6.7 Table 176). Pass 6.1 authors the
/// two Acrobat draws by default plus `None`; the full Table 176 set is a
/// documented not-yet-authored remainder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineEnding {
    /// `/None` — no ending shape.
    None,
    /// `/OpenArrow` — an open (unfilled) arrowhead. Acrobat's default at
    /// both ends (`markup__shape_and_line_annotations.md`).
    OpenArrow,
    /// `/ClosedArrow` — a closed (filled) arrowhead.
    ClosedArrow,
}

impl LineEnding {
    /// The `/LE` name bytes.
    const fn name(self) -> &'static [u8] {
        match self {
            Self::None => b"None",
            Self::OpenArrow => b"OpenArrow",
            Self::ClosedArrow => b"ClosedArrow",
        }
    }
}

/// The result of authoring one annotation: the annotation dictionary (no
/// `/AP` and no `/P` yet — [`crate::edit::EditSession`] wires those once it
/// has allocated object numbers), the appearance form-XObject dictionary,
/// the appearance content-stream bytes, and the computed `/Rect`.
#[derive(Debug, Clone, PartialEq)]
pub struct AuthoredAppearance {
    /// The annotation dictionary: `/Type /Annot`, `/Subtype`, `/Rect`,
    /// the per-subtype geometry keys, and cosmetic `/C` / `/IC` / `/CA` /
    /// `/BM` / `/BS`. **Missing `/AP` and `/P`** — the session adds them.
    pub annot: Dict,
    /// The `/AP` `/N` form-XObject dictionary (`/Type /XObject`,
    /// `/Subtype /Form`, `/BBox`, `/Resources`). `/Length` is added by the
    /// serializer, not here.
    pub ap_dict: Dict,
    /// The appearance content-stream bytes (raw, unfiltered — WF2).
    pub ap_content: Vec<u8>,
    /// The computed `/Rect`, guaranteed positive-area (WF4).
    pub rect: Rect,
}

// ---------------------------------------------------------------------------
// The clipboard codec (`Pass 169.0`)
// ---------------------------------------------------------------------------

/// The clipboard's exact COS encoding of a [`MarkupSpec`].
///
/// # ★ Why this is not `build_appearance(spec).annot`, which was tried first
///
/// The obvious route — carry the *annotation dictionary* the spec describes,
/// and read it back with [`spec_from_dict`] — would have cost no new code at
/// all, since both halves already ship and are exercised on every real
/// document. **It is wrong, and it is wrong silently.**
///
/// `build_appearance` computes a `/Rect` that BOUNDS WHAT IS DRAWN, which
/// §12.5.2 requires. A cloudy [`MarkupSpec::Square`]'s scallops bulge outside
/// the nominal rectangle, so the authored `/Rect` is larger than the spec's.
/// Reading that dictionary back yields a spec whose rectangle is the expanded
/// one — measured, on a 100×70 square with intensity 1.5: `10,20,110,90`
/// became `2.5,12.5,117.5,97.5`.
///
/// A clipboard built on that would have **grown a cloudy box by 7.5 pt in
/// every direction on every copy/paste cycle**, compounding, with no error
/// and no visibly wrong intermediate state. It is the same class of defect
/// the clip's `put_f64` guards against by writing bit-exact doubles rather
/// than decimals — drift that only shows up after enough repetitions, by
/// which time the operator cannot say when it started.
///
/// [`spec_from_dict`] is not at fault: it is a reader for **foreign**
/// annotations, where the stored `/Rect` *is* the truth and shrinking it
/// would be an invention. It was simply never the inverse of the author, and
/// nothing had required it to be.
///
/// # What this is instead
///
/// A direct encoding of the spec's **own fields**, exact by construction,
/// expressed as a COS object so it goes through the crate's one
/// [`write_object`](crate::writer::serialize::write_object) /
/// [`Parser`](crate::parser::Parser) pair rather than a second byte format —
/// which was the stated objection to serialising annotations at all.
///
/// Keys are pdfcer's own, not §12.5.6's, and deliberately so: this describes a
/// *spec*, not an annotation. `/K` is the variant tag.
#[must_use]
pub fn encode_spec(spec: &MarkupSpec) -> Object {
    let mut d = Dict::new();
    let tag = |d: &mut Dict, k: &[u8]| d.insert(Name::from(b"K"), Object::Name(Name::from(k)));
    match spec {
        MarkupSpec::Square {
            rect,
            border,
            interior,
            border_width,
            border_effect,
        } => {
            tag(&mut d, b"square");
            d.insert(Name::from(b"R"), rect_object(*rect));
            put_colour(&mut d, b"C", border.as_ref());
            put_colour(&mut d, b"IC", interior.as_ref());
            d.insert(Name::from(b"W"), Object::Real(*border_width));
            if let Some(i) = border_effect {
                d.insert(Name::from(b"BE"), Object::Real(*i));
            }
        }
        MarkupSpec::Circle {
            rect,
            border,
            interior,
            border_width,
        } => {
            tag(&mut d, b"circle");
            d.insert(Name::from(b"R"), rect_object(*rect));
            put_colour(&mut d, b"C", border.as_ref());
            put_colour(&mut d, b"IC", interior.as_ref());
            d.insert(Name::from(b"W"), Object::Real(*border_width));
        }
        MarkupSpec::Line {
            start,
            end,
            color,
            width,
            endings,
        } => {
            tag(&mut d, b"line");
            d.insert(Name::from(b"P"), points_object(&[*start, *end]));
            put_colour(&mut d, b"C", Some(color));
            d.insert(Name::from(b"W"), Object::Real(*width));
            d.insert(
                Name::from(b"LE"),
                Object::Array(vec![
                    Object::Name(Name::from(endings.0.name())),
                    Object::Name(Name::from(endings.1.name())),
                ]),
            );
        }
        MarkupSpec::Ink {
            strokes,
            color,
            width,
        } => {
            tag(&mut d, b"ink");
            d.insert(
                Name::from(b"S"),
                Object::Array(strokes.iter().map(|s| points_object(s)).collect()),
            );
            put_colour(&mut d, b"C", Some(color));
            d.insert(Name::from(b"W"), Object::Real(*width));
        }
        MarkupSpec::Polygon {
            vertices,
            border,
            interior,
            width,
        } => {
            tag(&mut d, b"polygon");
            d.insert(Name::from(b"V"), points_object(vertices));
            put_colour(&mut d, b"C", border.as_ref());
            put_colour(&mut d, b"IC", interior.as_ref());
            d.insert(Name::from(b"W"), Object::Real(*width));
        }
        MarkupSpec::Cloud {
            vertices,
            border,
            interior,
            width,
            intensity,
        } => {
            tag(&mut d, b"cloud");
            d.insert(Name::from(b"V"), points_object(vertices));
            put_colour(&mut d, b"C", border.as_ref());
            put_colour(&mut d, b"IC", interior.as_ref());
            d.insert(Name::from(b"W"), Object::Real(*width));
            d.insert(Name::from(b"BE"), Object::Real(*intensity));
        }
        MarkupSpec::PolyLine {
            vertices,
            color,
            width,
        } => {
            tag(&mut d, b"polyline");
            d.insert(Name::from(b"V"), points_object(vertices));
            put_colour(&mut d, b"C", Some(color));
            d.insert(Name::from(b"W"), Object::Real(*width));
        }
        MarkupSpec::TextMarkup { kind, quads, color } => {
            tag(&mut d, b"textmarkup");
            d.insert(
                Name::from(b"TK"),
                Object::Name(Name::from(text_markup_token(*kind))),
            );
            let mut flat = Vec::with_capacity(quads.len() * 8);
            for q in quads {
                for (x, y) in [q.ul, q.ur, q.ll, q.lr] {
                    flat.push(Object::Real(x));
                    flat.push(Object::Real(y));
                }
            }
            d.insert(Name::from(b"Q"), Object::Array(flat));
            put_colour(&mut d, b"C", Some(color));
        }
    }
    Object::Dict(d)
}

/// Read back what [`encode_spec`] wrote.
///
/// # Errors
///
/// [`SpecReadError::UnsupportedSubtype`] for an unknown or missing variant
/// tag — a payload from a newer build, or one that is not a spec at all — and
/// [`SpecReadError::BadGeometry`] when a geometry key is absent or the wrong
/// shape. Both are refusals, never a partial spec: half a shape pasted is a
/// wrong shape drawn, and the clip's own version gate is what should have
/// caught the newer-build case first.
pub fn decode_spec(obj: &Object) -> Result<MarkupSpec, SpecReadError> {
    let d = obj.as_dict().ok_or(SpecReadError::UnsupportedSubtype {
        subtype: "not a dictionary".to_owned(),
    })?;
    let kind = match d.get(b"K") {
        Some(Object::Name(n)) => n.as_bytes().to_vec(),
        _ => Vec::new(),
    };
    // NOT read eagerly: `TextMarkup` has no width, so demanding `/W` up
    // front refused the one variant that legitimately omits it. Each arm
    // that needs it calls this; the ones that do not, do not.
    let width = || {
        d.get(b"W")
            .and_then(Object::as_number)
            .ok_or(SpecReadError::BadGeometry { key: "W" })
    };
    let border = colour_of(d.get(b"C"));
    let interior = colour_of(d.get(b"IC"));
    let rect = || {
        d.get(b"R")
            .and_then(rect_of)
            .ok_or(SpecReadError::BadGeometry { key: "R" })
    };
    let vertices = || {
        d.get(b"V")
            .and_then(points_of)
            .filter(|v: &Vec<(f64, f64)>| !v.is_empty())
            .ok_or(SpecReadError::BadGeometry { key: "V" })
    };
    Ok(match kind.as_slice() {
        b"square" => MarkupSpec::Square {
            rect: rect()?,
            border,
            interior,
            border_width: width()?,
            border_effect: d.get(b"BE").and_then(Object::as_number),
        },
        b"circle" => MarkupSpec::Circle {
            rect: rect()?,
            border,
            interior,
            border_width: width()?,
        },
        b"line" => {
            let p = d
                .get(b"P")
                .and_then(points_of)
                .filter(|p: &Vec<(f64, f64)>| p.len() == 2)
                .ok_or(SpecReadError::BadGeometry { key: "P" })?;
            let ends = match d.get(b"LE") {
                Some(Object::Array(a)) if a.len() == 2 => {
                    (line_ending_of(a.first()), line_ending_of(a.get(1)))
                }
                _ => (LineEnding::None, LineEnding::None),
            };
            MarkupSpec::Line {
                start: *p.first().ok_or(SpecReadError::BadGeometry { key: "P" })?,
                end: *p.get(1).ok_or(SpecReadError::BadGeometry { key: "P" })?,
                color: border.unwrap_or(Color::Gray(0.0)),
                width: width()?,
                endings: ends,
            }
        }
        b"ink" => {
            let Some(Object::Array(items)) = d.get(b"S") else {
                return Err(SpecReadError::BadGeometry { key: "S" });
            };
            let strokes: Vec<Vec<(f64, f64)>> = items.iter().filter_map(points_of).collect();
            if strokes.is_empty() {
                return Err(SpecReadError::BadGeometry { key: "S" });
            }
            MarkupSpec::Ink {
                strokes,
                color: border.unwrap_or(Color::Gray(0.0)),
                width: width()?,
            }
        }
        b"polygon" => MarkupSpec::Polygon {
            vertices: vertices()?,
            border,
            interior,
            width: width()?,
        },
        b"cloud" => MarkupSpec::Cloud {
            vertices: vertices()?,
            border,
            interior,
            width: width()?,
            intensity: d
                .get(b"BE")
                .and_then(Object::as_number)
                .ok_or(SpecReadError::BadGeometry { key: "BE" })?,
        },
        b"polyline" => MarkupSpec::PolyLine {
            vertices: vertices()?,
            color: border.unwrap_or(Color::Gray(0.0)),
            width: width()?,
        },
        b"textmarkup" => {
            let nums = match d.get(b"Q") {
                Some(Object::Array(a)) => {
                    a.iter().filter_map(Object::as_number).collect::<Vec<f64>>()
                }
                _ => Vec::new(),
            };
            if nums.is_empty() || !nums.len().is_multiple_of(8) {
                return Err(SpecReadError::BadGeometry { key: "Q" });
            }
            let quads = nums
                .chunks_exact(8)
                .filter_map(|q| match q {
                    &[ulx, uly, urx, ury, llx, lly, lrx, lry] => Some(Quad {
                        ul: (ulx, uly),
                        ur: (urx, ury),
                        ll: (llx, lly),
                        lr: (lrx, lry),
                    }),
                    _ => None,
                })
                .collect();
            MarkupSpec::TextMarkup {
                kind: match d.get(b"TK") {
                    Some(Object::Name(n)) => match n.as_bytes() {
                        b"underline" => TextMarkupKind::Underline,
                        b"strikeout" => TextMarkupKind::StrikeOut,
                        b"squiggly" => TextMarkupKind::Squiggly,
                        _ => TextMarkupKind::Highlight,
                    },
                    _ => TextMarkupKind::Highlight,
                },
                quads,
                color: border.unwrap_or(Color::Gray(0.0)),
            }
        }
        other => {
            return Err(SpecReadError::UnsupportedSubtype {
                subtype: String::from_utf8_lossy(other).into_owned(),
            });
        }
    })
}

const fn text_markup_token(kind: TextMarkupKind) -> &'static [u8] {
    match kind {
        TextMarkupKind::Highlight => b"highlight",
        TextMarkupKind::Underline => b"underline",
        TextMarkupKind::StrikeOut => b"strikeout",
        TextMarkupKind::Squiggly => b"squiggly",
    }
}

fn line_ending_of(obj: Option<&Object>) -> LineEnding {
    match obj {
        Some(Object::Name(n)) => match n.as_bytes() {
            b"OpenArrow" => LineEnding::OpenArrow,
            b"ClosedArrow" => LineEnding::ClosedArrow,
            _ => LineEnding::None,
        },
        _ => LineEnding::None,
    }
}

fn rect_object(r: Rect) -> Object {
    Object::Array(vec![
        Object::Real(r.llx),
        Object::Real(r.lly),
        Object::Real(r.urx),
        Object::Real(r.ury),
    ])
}

fn rect_of(obj: &Object) -> Option<Rect> {
    let a = obj.as_array()?;
    match a.iter().filter_map(Object::as_number).collect::<Vec<_>>()[..] {
        [llx, lly, urx, ury] => Some(Rect { llx, lly, urx, ury }),
        _ => None,
    }
}

fn points_object(points: &[(f64, f64)]) -> Object {
    let mut flat = Vec::with_capacity(points.len() * 2);
    for (x, y) in points {
        flat.push(Object::Real(*x));
        flat.push(Object::Real(*y));
    }
    Object::Array(flat)
}

fn points_of(obj: &Object) -> Option<Vec<(f64, f64)>> {
    let nums: Vec<f64> = obj
        .as_array()?
        .iter()
        .filter_map(Object::as_number)
        .collect();
    if nums.is_empty() || !nums.len().is_multiple_of(2) {
        return None;
    }
    Some(
        nums.chunks_exact(2)
            .filter_map(|p| match p {
                &[x, y] => Some((x, y)),
                _ => None,
            })
            .collect(),
    )
}

/// A colour as its component array — 1, 3 or 4 reals, exactly as §12.5.6's
/// `/C` uses length to say which space it is in.
fn put_colour(d: &mut Dict, key: &[u8], colour: Option<&Color>) {
    let Some(colour) = colour else {
        return;
    };
    let components = match *colour {
        Color::Gray(g) => vec![g],
        Color::Rgb(r, g, b) => vec![r, g, b],
        Color::Cmyk(c, m, y, k) => vec![c, m, y, k],
    };
    d.insert(
        Name::from(key),
        Object::Array(components.into_iter().map(Object::Real).collect()),
    );
}

fn colour_of(obj: Option<&Object>) -> Option<Color> {
    let a = obj?.as_array()?;
    let n: Vec<f64> = a.iter().filter_map(Object::as_number).collect();
    match n[..] {
        [g] => Some(Color::Gray(g)),
        [r, g, b] => Some(Color::Rgb(r, g, b)),
        [c, m, y, k] => Some(Color::Cmyk(c, m, y, k)),
        _ => None,
    }
}

/// The ellipse Bézier constant: the control-point distance, as a fraction
/// of the radius, that makes four cubic segments approximate a quarter
/// ellipse to within ~0.02% (`4/3 · (√2 − 1)`).
const KAPPA: f64 = 0.552_284_749_830_793_4;

/// The minimum positive extent forced onto a degenerate `/Rect`/`/BBox`
/// axis (WF4). One point is invisibly small yet keeps §12.5.5's placement
/// matrix non-singular — the safe alternative to a NEGATIVE RESULT.
const MIN_EXTENT: f64 = 1.0;

/// Author the annotation dictionary + `/AP` `/N` appearance for `spec`.
///
/// The single entry point. Returns the pieces [`crate::edit::EditSession`]
/// assembles into indirect objects; it never allocates object numbers,
/// never touches a page, and never emits page content (R47).
#[must_use]
pub fn build_appearance(spec: &MarkupSpec) -> AuthoredAppearance {
    build_appearance_with(spec, QuadPointOrder::default())
}

/// As [`build_appearance`], with an explicit `/QuadPoints` corner order
/// (§12.5.6.10, ambiguity **`QP-A1`**).
///
/// Only the text-markup family reads `order`; every other subtype ignores it,
/// because only `/Highlight`, `/Underline`, `/StrikeOut` and `/Squiggly`
/// carry `/QuadPoints`.
///
/// # Why the order is a caller's choice at all
///
/// pdfcer bakes a full `/AP` (R44), so **its own rendering never consults**
/// **`/QuadPoints`** and the order is invisible in pdfcer. It matters to a
/// third-party consumer that re-derives geometry from the array — where the
/// wrong order produces a bow-tie instead of a rectangle — and the two
/// candidate orders serve different consumers: Acrobat, PDFBox and pdf.js
/// emit and expect reading order, while §12.5.6.10 describes a
/// counterclockwise walk. See [`QuadPointOrder`].
///
/// That invisibility is exactly why the ambiguity register rates `QP-A1` its
/// worst case: a deliberate divergence from a `shall`-adjacent statement,
/// with no runtime symptom in the tool that made it.
pub fn build_appearance_with(spec: &MarkupSpec, order: QuadPointOrder) -> AuthoredAppearance {
    match spec {
        MarkupSpec::Square {
            rect,
            border,
            interior,
            border_width,
            border_effect,
        } => rectangle_like(
            b"Square",
            *rect,
            *border,
            *interior,
            *border_width,
            false,
            *border_effect,
        ),
        MarkupSpec::Circle {
            rect,
            border,
            interior,
            border_width,
        } => rectangle_like(
            b"Circle",
            *rect,
            *border,
            *interior,
            *border_width,
            true,
            // `/BE` on `/Circle` is legal (Table 180 lists it without the
            // polygon-only qualifier), but pdfcer does not author it: an
            // ellipse has no straight edges to scallop, so the baked
            // appearance would have to invent a curve-following cloud
            // that the standard describes nowhere. Refused by omission
            // rather than by a runtime error, because the type simply
            // does not offer it.
            None,
        ),
        MarkupSpec::Line {
            start,
            end,
            color,
            width,
            endings,
        } => line(*start, *end, *color, *width, *endings),
        MarkupSpec::Ink {
            strokes,
            color,
            width,
        } => ink(strokes, *color, *width),
        MarkupSpec::Polygon {
            vertices,
            border,
            interior,
            width,
        } => polygon_like(b"Polygon", vertices, *border, *interior, *width, true, None),
        MarkupSpec::Cloud {
            vertices,
            border,
            interior,
            width,
            intensity,
        } => polygon_like(
            b"Polygon",
            vertices,
            *border,
            *interior,
            *width,
            true,
            Some(*intensity),
        ),
        MarkupSpec::PolyLine {
            vertices,
            color,
            width,
        } => polygon_like(
            b"PolyLine",
            vertices,
            Some(*color),
            None,
            *width,
            false,
            // Table 181 declares `/BE` on Polygon/PolyLine with the
            // qualifier "meaningful only for polygon annotations",
            // verbatim in BOTH editions, and §12.5.4's prose independently
            // lists only square, circle and polygon. An open polyline
            // cannot carry one.
            None,
        ),
        MarkupSpec::TextMarkup { kind, quads, color } => text_markup(*kind, quads, *color, order),
    }
}

/// Force a rectangle to strictly positive width and height (WF4). A
/// degenerate axis is expanded by [`MIN_EXTENT`] about its own coordinate;
/// corners are normalised min→max (§7.9.5) first.
fn positive_rect(mut r: Rect) -> Rect {
    if r.urx < r.llx {
        std::mem::swap(&mut r.llx, &mut r.urx);
    }
    if r.ury < r.lly {
        std::mem::swap(&mut r.lly, &mut r.ury);
    }
    if r.urx - r.llx < MIN_EXTENT {
        r.urx = r.llx + MIN_EXTENT;
    }
    if r.ury - r.lly < MIN_EXTENT {
        r.ury = r.lly + MIN_EXTENT;
    }
    r
}

/// The bounding rectangle of a point set, expanded by `margin` on all
/// sides (to contain stroke width / arrowheads), forced positive.
fn bounds_of(points: impl Iterator<Item = (f64, f64)>, margin: f64) -> Rect {
    let mut llx = f64::INFINITY;
    let mut lly = f64::INFINITY;
    let mut urx = f64::NEG_INFINITY;
    let mut ury = f64::NEG_INFINITY;
    let mut any = false;
    for (x, y) in points {
        any = true;
        llx = llx.min(x);
        lly = lly.min(y);
        urx = urx.max(x);
        ury = ury.max(y);
    }
    if !any {
        return positive_rect(Rect {
            llx: 0.0,
            lly: 0.0,
            urx: 0.0,
            ury: 0.0,
        });
    }
    positive_rect(Rect {
        llx: llx - margin,
        lly: lly - margin,
        urx: urx + margin,
        ury: ury + margin,
    })
}

/// The common form-XObject dictionary for an appearance whose `BBox` is
/// `rect` and whose `Resources` is `resources` (empty for pure geometry,
/// an `/ExtGState` map for Highlight's Multiply blend).
fn form_dict(rect: Rect, resources: Dict) -> Dict {
    let mut d = Dict::new();
    // /Type is optional (§8.10 W-A) but conventional on appearance
    // streams; harmless and self-documenting.
    d.insert(Name::from(b"Type"), Object::Name(Name::from(b"XObject")));
    d.insert(Name::from(b"Subtype"), Object::Name(Name::from(b"Form")));
    d.insert(Name::from(b"BBox"), rect_array(rect));
    // /Matrix omitted ⇒ identity (§8.10 Table 95 default), which with
    // BBox == Rect yields AA == identity (§12.5.5 / W-B).
    d.insert(Name::from(b"Resources"), Object::Dict(resources));
    d
}

/// A `[llx lly urx ury]` rectangle array (§7.9.5), emitted normalised.
fn rect_array(r: Rect) -> Object {
    Object::Array(vec![
        Object::Real(r.llx),
        Object::Real(r.lly),
        Object::Real(r.urx),
        Object::Real(r.ury),
    ])
}

/// The base annotation dictionary common to every subtype: `/Type /Annot`,
/// `/Subtype`, `/Rect`. Cosmetic and geometry keys are layered on by the
/// per-subtype builders.
fn base_annot(subtype: &[u8], rect: Rect) -> Dict {
    let mut d = Dict::new();
    d.insert(Name::from(b"Type"), Object::Name(Name::from(b"Annot")));
    d.insert(Name::from(b"Subtype"), Object::Name(Name(subtype.to_vec())));
    d.insert(Name::from(b"Rect"), rect_array(rect));
    d
}

/// A `/BS` border-style dictionary carrying width `w` (§12.5.4 Table 168).
fn border_style(w: f64) -> Object {
    let mut bs = Dict::new();
    bs.insert(Name::from(b"Type"), Object::Name(Name::from(b"Border")));
    bs.insert(Name::from(b"W"), Object::Real(w));
    bs.insert(Name::from(b"S"), Object::Name(Name::from(b"S"))); // solid
    Object::Dict(bs)
}

/// Square (`inscribe_ellipse = false`) or Circle (`= true`): a filled
/// and/or stroked rectangle/ellipse inset by half the border width so the
/// stroke stays inside `BBox`.
fn rectangle_like(
    subtype: &[u8],
    rect: Rect,
    border: Option<Color>,
    interior: Option<Color>,
    border_width: f64,
    inscribe_ellipse: bool,
    cloud: Option<f64>,
) -> AuthoredAppearance {
    let shape = positive_rect(rect);
    // ★ /Rect must CONTAIN the cloud, which bulges outside the square the
    // caller drew. Table 180's own /RD row names exactly this situation:
    // "Such a difference may occur in situations where a border effect
    // (described by BE) causes the size of the Rect to increase beyond
    // that of the square or circle."
    //
    // That row is causal-explanatory and PERMISSIVE — /RD is `(Optional)`
    // and nothing obliges pdfcer to write it. pdfcer writes it anyway, so
    // the square the operator actually drew stays recoverable from the
    // file rather than being inferable only by un-inflating by a constant
    // this crate happens to use today. Filed as a pdfcer product rule
    // filling a spec silence, not as a conformance requirement.
    let bulge = cloud.map_or(0.0, scallop_radius);
    let rect = if bulge > 0.0 {
        positive_rect(Rect {
            llx: shape.llx - bulge,
            lly: shape.lly - bulge,
            urx: shape.urx + bulge,
            ury: shape.ury + bulge,
        })
    } else {
        shape
    };
    let mut annot = base_annot(subtype, rect);
    if let Some(c) = border {
        annot.insert(Name::from(b"C"), c.to_array());
    }
    if let Some(c) = interior {
        annot.insert(Name::from(b"IC"), c.to_array());
    }
    annot.insert(Name::from(b"BS"), border_style(border_width));
    if let Some(i) = cloud {
        annot.insert(Name::from(b"BE"), border_effect_dict(i));
        // All four insets are equal, so the left/top/right/bottom ordering
        // question cannot produce a wrong file here. Stated rather than
        // relied on silently: a future non-uniform bulge would have to
        // settle the order first.
        annot.insert(
            Name::from(b"RD"),
            Object::Array(vec![
                Object::Real(bulge),
                Object::Real(bulge),
                Object::Real(bulge),
                Object::Real(bulge),
            ]),
        );
    }

    let mut b = ContentBuilder::new();
    let has_stroke = border.is_some() && border_width > 0.0;
    if let Some(c) = interior {
        c.apply_fill(&mut b);
    }
    if let (true, Some(c)) = (has_stroke, border) {
        c.apply_stroke(&mut b);
        b.set_line_width(border_width);
        if cloud.is_some() {
            b.set_line_join(LineJoin::Round);
        }
    }
    if let Some(i) = cloud {
        // Scallop the square the caller drew, not the inflated /Rect —
        // the inflation exists to make room for the bulge, so scalloping
        // it too would move the shape outward by the bulge as well.
        cloud_path(
            &mut b,
            &[
                (shape.llx, shape.lly),
                (shape.urx, shape.lly),
                (shape.urx, shape.ury),
                (shape.llx, shape.ury),
            ],
            i,
        );
        b.paint(fill_stroke_paint(interior.is_some(), has_stroke));
    } else {
        // Inset by half the line width so a stroke of width w is fully
        // inside BBox (a stroke straddles the path centre line).
        let inset = if has_stroke { border_width / 2.0 } else { 0.0 };
        let x = rect.llx + inset;
        let y = rect.lly + inset;
        let w = (rect.urx - rect.llx - 2.0 * inset).max(0.0);
        let h = (rect.ury - rect.lly - 2.0 * inset).max(0.0);
        if inscribe_ellipse {
            emit_ellipse(&mut b, x + w / 2.0, y + h / 2.0, w / 2.0, h / 2.0);
        } else {
            b.rect(x, y, w, h);
        }
        b.paint(fill_stroke_paint(interior.is_some(), has_stroke));
    }

    AuthoredAppearance {
        annot,
        ap_dict: form_dict(rect, Dict::new()),
        ap_content: b.into_bytes(),
        rect,
    }
}

/// The paint operator for a closed shape given whether it fills and/or
/// strokes. A shape with neither still emits `n` (end path, no paint) so
/// the content stream is well-formed.
fn fill_stroke_paint(fills: bool, strokes: bool) -> Paint {
    match (fills, strokes) {
        (true, true) => Paint::FillStroke,
        (true, false) => Paint::Fill,
        (false, true) => Paint::Stroke,
        (false, false) => Paint::NoPaint,
    }
}

/// Emit an axis-aligned ellipse centred at `(cx, cy)` with radii
/// `(rx, ry)` as four cubic Béziers (§8.5.2.2), starting at the right
/// vertex and proceeding counter-clockwise.
fn emit_ellipse(b: &mut ContentBuilder, cx: f64, cy: f64, rx: f64, ry: f64) {
    let ox = rx * KAPPA;
    let oy = ry * KAPPA;
    b.move_to(cx + rx, cy);
    b.curve_to(cx + rx, cy + oy, cx + ox, cy + ry, cx, cy + ry);
    b.curve_to(cx - ox, cy + ry, cx - rx, cy + oy, cx - rx, cy);
    b.curve_to(cx - rx, cy - oy, cx - ox, cy - ry, cx, cy - ry);
    b.curve_to(cx + ox, cy - ry, cx + rx, cy - oy, cx + rx, cy);
    b.close_subpath();
}

/// `/Line` with optional arrowheads.
fn line(
    start: (f64, f64),
    end: (f64, f64),
    color: Color,
    width: f64,
    endings: (LineEnding, LineEnding),
) -> AuthoredAppearance {
    // Arrowheads and stroke width extend past the endpoints; the margin
    // must contain them.
    let arrow_len = arrow_length(width);
    let margin = (width / 2.0).max(arrow_len);
    let rect = bounds_of([start, end].into_iter(), margin);

    let mut annot = base_annot(b"Line", rect);
    annot.insert(
        Name::from(b"L"),
        Object::Array(vec![
            Object::Real(start.0),
            Object::Real(start.1),
            Object::Real(end.0),
            Object::Real(end.1),
        ]),
    );
    annot.insert(Name::from(b"C"), color.to_array());
    annot.insert(Name::from(b"BS"), border_style(width));
    annot.insert(
        Name::from(b"LE"),
        Object::Array(vec![
            Object::Name(Name(endings.0.name().to_vec())),
            Object::Name(Name(endings.1.name().to_vec())),
        ]),
    );

    let mut b = ContentBuilder::new();
    color.apply_stroke(&mut b);
    color.apply_fill(&mut b); // for a ClosedArrow fill
    b.set_line_width(width);
    b.set_line_cap(LineCap::Butt);
    b.set_line_join(LineJoin::Miter);
    // The shaft.
    b.move_to(start.0, start.1);
    b.line_to(end.0, end.1);
    b.paint(Paint::Stroke);
    // Arrowheads: the start ending points back along the line from `start`
    // toward `end`'s direction reversed; the end ending points forward.
    emit_line_ending(&mut b, start, end, endings.0, width);
    emit_line_ending(&mut b, end, start, endings.1, width);

    AuthoredAppearance {
        annot,
        ap_dict: form_dict(rect, Dict::new()),
        ap_content: b.into_bytes(),
        rect,
    }
}

/// The arrowhead length pdfcer uses for a given line width (pdfcer's own
/// choice — the exact Acrobat scaling is undocumented). Scales with width
/// and has a floor so a thin line still gets a visible head.
fn arrow_length(width: f64) -> f64 {
    (width * 4.0).max(8.0)
}

/// Emit one line ending at `tip`, with the shaft arriving from `other`.
/// `None` emits nothing; `OpenArrow` strokes a `<` outline; `ClosedArrow`
/// fills a triangle.
fn emit_line_ending(
    b: &mut ContentBuilder,
    tip: (f64, f64),
    other: (f64, f64),
    ending: LineEnding,
    width: f64,
) {
    if ending == LineEnding::None {
        return;
    }
    let dx = tip.0 - other.0;
    let dy = tip.1 - other.1;
    let len = (dx * dx + dy * dy).sqrt();
    if len < f64::EPSILON {
        return; // zero-length line: no meaningful direction
    }
    // Unit vector pointing from `other` toward `tip` (the direction the
    // arrow points).
    let (ux, uy) = (dx / len, dy / len);
    let a = arrow_length(width);
    // Half-angle of the arrowhead ≈ 22.5° ⇒ half-width = a·tan(22.5°).
    let half = a * 0.414_213_562_373_095;
    // The two barb points, back `a` along the shaft and ±half across it.
    let bx = tip.0 - ux * a;
    let by = tip.1 - uy * a;
    // Perpendicular unit vector.
    let (px, py) = (-uy, ux);
    let barb1 = (bx + px * half, by + py * half);
    let barb2 = (bx - px * half, by - py * half);
    match ending {
        LineEnding::OpenArrow => {
            b.move_to(barb1.0, barb1.1);
            b.line_to(tip.0, tip.1);
            b.line_to(barb2.0, barb2.1);
            b.paint(Paint::Stroke);
        }
        LineEnding::ClosedArrow => {
            b.move_to(tip.0, tip.1);
            b.line_to(barb1.0, barb1.1);
            b.line_to(barb2.0, barb2.1);
            b.close_subpath();
            b.paint(Paint::FillStroke);
        }
        LineEnding::None => {}
    }
}

/// `/Ink` — each stroke is a polyline (`move_to` first point, `line_to`
/// the rest), all stroked with one uniform width.
fn ink(strokes: &[Vec<(f64, f64)>], color: Color, width: f64) -> AuthoredAppearance {
    let rect = bounds_of(
        strokes.iter().flat_map(|s| s.iter().copied()),
        (width / 2.0).max(1.0),
    );
    let mut annot = base_annot(b"Ink", rect);
    // /InkList: an array of stroke arrays, each a flat x1 y1 x2 y2 … list.
    let ink_list = Object::Array(
        strokes
            .iter()
            .map(|s| {
                Object::Array(
                    s.iter()
                        .flat_map(|&(x, y)| [Object::Real(x), Object::Real(y)])
                        .collect(),
                )
            })
            .collect(),
    );
    annot.insert(Name::from(b"InkList"), ink_list);
    annot.insert(Name::from(b"C"), color.to_array());
    annot.insert(Name::from(b"BS"), border_style(width));

    let mut b = ContentBuilder::new();
    color.apply_stroke(&mut b);
    b.set_line_width(width);
    b.set_line_cap(LineCap::Round);
    b.set_line_join(LineJoin::Round);
    for stroke in strokes {
        let mut pts = stroke.iter();
        if let Some(&(x0, y0)) = pts.next() {
            b.move_to(x0, y0);
            for &(x, y) in pts {
                b.line_to(x, y);
            }
            b.paint(Paint::Stroke);
        }
    }
    AuthoredAppearance {
        annot,
        ap_dict: form_dict(rect, Dict::new()),
        ap_content: b.into_bytes(),
        rect,
    }
}

/// `/Polygon` (closed, fillable) or `/PolyLine` (open, stroke-only).
/// The border effect dictionary `/BE << /S /C /I n >>` (§12.5.4,
/// Table 167 in ISO 32000-1; Table 169 in ISO 32000-2 — renumbered, text
/// identical).
///
/// Only the cloudy form is ever written. `/S /S` means "no effect" and is
/// the **default**, so emitting it would add a key that changes nothing;
/// callers express that by passing `None` instead.
fn border_effect_dict(intensity: f64) -> Object {
    let mut d = Dict::new();
    d.insert(Name::from(b"S"), Object::Name(Name::from(b"C")));
    d.insert(Name::from(b"I"), Object::Real(intensity));
    Object::Dict(d)
}

/// `4/3·tan(θ/4)` for a 90° arc — the standard cubic-Bézier circle
/// constant, already used elsewhere in this module for ellipses.
const KAPPA_90: f64 = 0.552_284_749_830_793_4;

/// pdfcer's scallop radius for a given `/BE /I` intensity, in points.
///
/// # ★ THIS FUNCTION IS pdfcer'S INVENTION, NOT THE STANDARD'S (`BE-A1`)
///
/// Everything ISO 32000 says about what a cloud *looks like*: the border
/// *"**should** appear 'cloudy'"* (1.7), *"a series of convex curved line
/// segments"* (2.0 — the only shape description in either edition, and
/// still a `should`), *"the width and dash array specified by `BS` shall
/// be honoured"*, and that `/I` is *"a number describing the intensity of
/// the effect, in the range 0 to 2"*.
///
/// **Not stated in either edition:** arc radius, arc sweep, segment count,
/// outward-vs-inward bulge, linearity, interaction with `BS /W`, or —
/// most consequentially — **what quantity the intensity actually
/// scales.** Registered as `BE-A1` in the spec corpus's ambiguity
/// register.
///
/// Because `R43` obliges pdfcer to paint from a baked appearance stream or
/// not at all, pdfcer cannot defer this to the reader. It must choose. So
/// this is a **disclosed product decision** under rule 4, not a
/// conformance behaviour, and a future session is free to retune it
/// without any spec citation being violated.
///
/// # The choice, and the one judgement inside it
///
/// `radius = 3 + 3·intensity` points: `/I 0` → 3 pt, `/I 1` → 6 pt,
/// `/I 2` → 9 pt. Linear, because nothing suggests otherwise and a curve
/// would be a second invention on top of the first.
///
/// **`/I 0` is read as MINIMUM cloud, not ABSENT cloud**, and that is the
/// judgement. The standard makes `0` the default for `/I` while `/S /C`
/// is an explicit request for cloudiness — so reading `0` as "no bulge"
/// would make `<< /S /C >>` a request that cancels itself, which cannot
/// be what the key is for. "No cloud" already has two spellings that do
/// not need a third: `/S /S`, and omitting `/BE` altogether.
fn scallop_radius(intensity: f64) -> f64 {
    3.0 + 3.0 * intensity
}

/// Append a cloudy (scalloped) closed path for `vertices` to `b`.
///
/// # Algorithm
///
/// 1. Determine orientation from the **signed area** so the scallops bulge
///    *outward* regardless of whether the caller wound the polygon
///    clockwise or counter-clockwise. A caller should not have to know
///    pdfcer's winding convention to get a cloud that looks like a cloud.
/// 2. For each edge, fit a whole number of scallops:
///    `n = max(1, round(edge_length / (2·radius)))`, chord `c = length/n`.
///    Fitting per **edge** rather than around the whole perimeter is what
///    keeps a scallop boundary exactly on each corner — spreading them
///    around the perimeter instead lets a scallop straddle a vertex, which
///    reads as a dented corner.
/// 3. Each scallop is a **semicircle on its own chord** (radius `c/2`),
///    emitted as two 90° cubic Béziers. A semicircle as a *single* cubic
///    is a poor fit; two is the usual compromise and is what the ellipse
///    path in this module already does.
///
/// The `c/2` radius means the *drawn* bulge depth is set by how the chord
/// divides, not directly by [`scallop_radius`] — that function sets the
/// target size, and the rounding to a whole number of scallops adjusts it
/// per edge so the pattern closes cleanly at every corner.
fn cloud_path(b: &mut ContentBuilder, vertices: &[(f64, f64)], intensity: f64) {
    let n = vertices.len();
    if n < 3 {
        return;
    }
    // Each edge as a (from, to) pair, closing back to the first vertex.
    // Built by zipping the list against itself rotated by one rather than
    // by indexing: this crate denies `indexing_slicing`, and a parser that
    // cannot panic on a malformed vertex list is the reason why.
    let edges = || {
        vertices
            .iter()
            .copied()
            .zip(vertices.iter().copied().cycle().skip(1))
            .take(n)
    };

    // Shoelace. Positive ⇒ counter-clockwise, in PDF's y-up default user
    // space. The outward normal of an edge (dx, dy) is (dy, -dx) for a
    // CCW polygon, and the negation of that for a CW one. Deriving this
    // rather than requiring a winding convention means a caller cannot
    // get an inward-scalloped "cloud" by drawing their box the other way
    // round.
    let twice_area: f64 = edges()
        .map(|((x0, y0), (x1, y1))| x0.mul_add(y1, -(x1 * y0)))
        .sum();
    let outward = if twice_area >= 0.0 { 1.0 } else { -1.0 };

    let target = scallop_radius(intensity).max(0.25);
    let Some(&(sx, sy)) = vertices.first() else {
        return;
    };
    b.move_to(sx, sy);

    for ((x0, y0), (x1, y1)) in edges() {
        let (dx, dy) = (x1 - x0, y1 - y0);
        let len = dx.hypot(dy);
        if len <= f64::EPSILON {
            // A repeated vertex. Skipping keeps the path valid; a
            // zero-length scallop would put NaN in the control points.
            continue;
        }
        let count = (len / (2.0 * target)).round().max(1.0);
        let chord = len / count;
        let (ux, uy) = (dx / len, dy / len);
        // Outward unit normal.
        let (nx, ny) = (uy * outward, -ux * outward);
        let r = chord / 2.0;
        let k = KAPPA_90 * r;

        let mut t0 = 0.0_f64;
        while t0 < len - f64::EPSILON {
            let ax = ux.mul_add(t0, x0);
            let ay = uy.mul_add(t0, y0);
            let bx = ux.mul_add(t0 + chord, x0);
            let by = uy.mul_add(t0 + chord, y0);
            // Apex of the semicircle: the chord midpoint pushed out by r.
            let mx = nx.mul_add(r, (ax + bx) / 2.0);
            let my = ny.mul_add(r, (ay + by) / 2.0);

            // Arc 1: A → apex. Leaves A along the outward normal and
            // arrives at the apex travelling along the edge.
            b.curve_to(
                nx.mul_add(k, ax),
                ny.mul_add(k, ay),
                (-ux).mul_add(k, mx),
                (-uy).mul_add(k, my),
                mx,
                my,
            );
            // Arc 2: apex → B, the mirror of arc 1.
            b.curve_to(
                ux.mul_add(k, mx),
                uy.mul_add(k, my),
                nx.mul_add(k, bx),
                ny.mul_add(k, by),
                bx,
                by,
            );
            t0 += chord;
        }
    }
    b.close_subpath();
}

fn polygon_like(
    subtype: &[u8],
    vertices: &[(f64, f64)],
    border: Option<Color>,
    interior: Option<Color>,
    width: f64,
    closed: bool,
    cloud: Option<f64>,
) -> AuthoredAppearance {
    // ★ The cloud bulges OUTSIDE the vertex hull, so the bounding box has
    // to grow by the scallop radius or the appearance stream would be
    // clipped by its own /BBox — the classic way a baked cloud renders
    // with its bumps sliced flat.
    let margin = (width / 2.0).max(1.0) + cloud.map_or(0.0, scallop_radius);
    let rect = bounds_of(vertices.iter().copied(), margin);
    let mut annot = base_annot(subtype, rect);
    annot.insert(
        Name::from(b"Vertices"),
        Object::Array(
            vertices
                .iter()
                .flat_map(|&(x, y)| [Object::Real(x), Object::Real(y)])
                .collect(),
        ),
    );
    if let Some(c) = border {
        annot.insert(Name::from(b"C"), c.to_array());
    }
    if let Some(c) = interior {
        annot.insert(Name::from(b"IC"), c.to_array());
    }
    annot.insert(Name::from(b"BS"), border_style(width));
    if let Some(i) = cloud {
        annot.insert(Name::from(b"BE"), border_effect_dict(i));
    }

    let mut b = ContentBuilder::new();
    let has_stroke = border.is_some() && width > 0.0;
    if let Some(c) = interior {
        c.apply_fill(&mut b);
    }
    if let (true, Some(c)) = (has_stroke, border) {
        c.apply_stroke(&mut b);
        b.set_line_width(width);
        // Round joins on a cloud: the scallops meet at tangent angles and
        // a miter there spikes. Straight polygons keep the miter they
        // have always had.
        b.set_line_join(if cloud.is_some() {
            LineJoin::Round
        } else {
            LineJoin::Miter
        });
    }
    if let Some(i) = cloud {
        cloud_path(&mut b, vertices, i);
        b.paint(fill_stroke_paint(interior.is_some(), has_stroke));
    } else {
        let mut pts = vertices.iter();
        if let Some(&(x0, y0)) = pts.next() {
            b.move_to(x0, y0);
            for &(x, y) in pts {
                b.line_to(x, y);
            }
            if closed {
                b.close_subpath();
            }
            b.paint(fill_stroke_paint(interior.is_some() && closed, has_stroke));
        }
    }
    AuthoredAppearance {
        annot,
        ap_dict: form_dict(rect, Dict::new()),
        ap_content: b.into_bytes(),
        rect,
    }
}

/// The text-markup family. Rect bounds all quads; the appearance draws the
/// per-subtype mark over each quad.
fn text_markup(
    kind: TextMarkupKind,
    quads: &[Quad],
    color: Color,
    order: QuadPointOrder,
) -> AuthoredAppearance {
    let rect = bounds_of(
        quads
            .iter()
            .flat_map(|q| [q.ul, q.ur, q.ll, q.lr].into_iter()),
        1.0,
    );
    let mut annot = base_annot(kind.subtype(), rect);
    annot.insert(
        Name::from(b"QuadPoints"),
        text_markup_quad_object(quads, order),
    );
    annot.insert(Name::from(b"C"), color.to_array());

    // Highlight is painted Multiply so overlaps do not darken; the annot
    // also carries /BM for readers that consult it, and the appearance's
    // own Resources carry the ExtGState the `gs` operator selects (X8: an
    // appearance stream is a closed resource world).
    let (resources, mut b) = if kind == TextMarkupKind::Highlight {
        annot.insert(Name::from(b"BM"), Object::Name(Name::from(b"Multiply")));
        let mut gs = Dict::new();
        gs.insert(Name::from(b"Type"), Object::Name(Name::from(b"ExtGState")));
        gs.insert(Name::from(b"BM"), Object::Name(Name::from(b"Multiply")));
        gs.insert(Name::from(b"ca"), Object::Real(1.0));
        let mut ext = Dict::new();
        ext.insert(Name::from(b"GS0"), Object::Dict(gs));
        let mut res = Dict::new();
        res.insert(Name::from(b"ExtGState"), Object::Dict(ext));
        let mut b = ContentBuilder::new();
        b.set_ext_gstate(b"GS0");
        (res, b)
    } else {
        (Dict::new(), ContentBuilder::new())
    };

    match kind {
        TextMarkupKind::Highlight => {
            color.apply_fill(&mut b);
            for q in quads {
                // Fill the quad polygon: UL → UR → LR → LL → close.
                b.move_to(q.ul.0, q.ul.1);
                b.line_to(q.ur.0, q.ur.1);
                b.line_to(q.lr.0, q.lr.1);
                b.line_to(q.ll.0, q.ll.1);
                b.close_subpath();
                b.paint(Paint::Fill);
            }
        }
        TextMarkupKind::Underline | TextMarkupKind::StrikeOut | TextMarkupKind::Squiggly => {
            color.apply_stroke(&mut b);
            // Line thickness scales with quad height, floored so a small
            // mark is still visible (pdfcer's own choice; the exact Acrobat
            // value is a RAG GAP).
            for q in quads {
                let bounds = q.bounds();
                let qh = bounds.ury - bounds.lly;
                let thickness = (qh * 0.06).max(0.75);
                b.set_line_width(thickness);
                let left = bounds.llx;
                let right = bounds.urx;
                match kind {
                    TextMarkupKind::Underline => {
                        let y = bounds.lly + qh * 0.10;
                        b.move_to(left, y);
                        b.line_to(right, y);
                        b.paint(Paint::Stroke);
                    }
                    TextMarkupKind::StrikeOut => {
                        let y = bounds.lly + qh * 0.5;
                        b.move_to(left, y);
                        b.line_to(right, y);
                        b.paint(Paint::Stroke);
                    }
                    TextMarkupKind::Squiggly => {
                        emit_squiggle(&mut b, left, right, bounds.lly + qh * 0.10, qh * 0.10);
                    }
                    TextMarkupKind::Highlight => {}
                }
            }
        }
    }

    AuthoredAppearance {
        annot,
        ap_dict: form_dict(rect, resources),
        ap_content: b.into_bytes(),
        rect,
    }
}

/// Emit a squiggly (zig-zag) line from `left` to `right` at baseline `y0`
/// with peak amplitude `amp`, as a stroked polyline. The wavelength is
/// `4·amp` so the zig-zag reads as a wave rather than sawtooth.
fn emit_squiggle(b: &mut ContentBuilder, left: f64, right: f64, y0: f64, amp: f64) {
    let amp = amp.max(0.5);
    let step = (amp * 2.0).max(1.0);
    b.move_to(left, y0);
    let mut x = left;
    let mut up = true;
    while x < right {
        let nx = (x + step).min(right);
        let y = if up { y0 + amp } else { y0 };
        b.line_to(nx, y);
        up = !up;
        x = nx;
    }
    b.paint(Paint::Stroke);
}

/// A `/Redact` redaction mark to author (ISO 32000-1 §12.5.6.23,
/// Table 192). The MARK phase (non-destructive, reviewable): it records
/// the region intended for removal; [`crate::redact::apply_redactions`]
/// performs the destructive removal later.
///
/// The authored `/AP` preview is a **red outline** — never a solid fill —
/// so a marked-but-unapplied region can never be mistaken for a completed
/// redaction (the ui-specialist's mark-vs-apply rule; the #1 real-world
/// redaction failure is saving a marked doc believing it is done).
#[derive(Debug, Clone, PartialEq)]
pub struct RedactSpec {
    /// The quadrilaterals to remove (`/QuadPoints`), default user space.
    pub quads: Vec<Quad>,
    /// The fill colour `/IC` used **on apply**. Not the preview colour.
    ///
    /// `None` writes no `/IC`, and ISO 32000-1 Table 192 is explicit about
    /// what that means on apply: "if this entry is absent, the interior of
    /// the redaction region is left transparent". It is NOT a synonym for
    /// black — pass `Some(Color::gray(0.0))` for a black box.
    pub fill: Option<Color>,
    /// Optional overlay text `/OverlayText`, **drawn on apply** over the
    /// `/IC` fill (Table 192's overlay ladder — `/IC` is ignored only when
    /// `/RO` is present, not when `/OverlayText` is).
    ///
    /// Laid out by [`crate::vartext`] using the `/DA` this authors and the
    /// [`quadding`](Self::quadding) below, so it wraps, clips to the
    /// region, and auto-sizes exactly like a form field's value.
    pub overlay_text: Option<String>,
    /// `/Q` justification for the overlay text.
    pub quadding: Quadding,
}

/// Everything a [`RedactSpec`] carries EXCEPT its geometry — the
/// apply-time appearance of a redaction mark.
///
/// # Why this is split out
///
/// A [`RedactSpec`] describes ONE mark, geometry included. That is the
/// right shape when the caller drew the region and the wrong shape when
/// the caller found N regions by searching: the appearance is a single
/// operator choice ("black box, "REDACTED" in the middle"), the geometry is
/// per-match, and fusing them forces a search-and-mark verb either to
/// invent an appearance per hit or — as pdfcer did until this was added —
/// to hard-code one and silently discard whatever the operator chose.
///
/// That was not a hypothetical. `EditSession::mark_redactions_by_search`
/// and `..._by_pattern` built their specs internally with `fill: None,
/// overlay_text: None`, so the ONLY way to get a coloured or captioned mark
/// was to place every one of them by hand. It was reported from outside
/// (`pdfcer-gui`, 2026-08-17) as a fill-colour control that could not be
/// wired to the find-and-mark path, which is decision 058's case exactly:
/// a consumer's missing surface was a boundary drawn wrong in core.
///
/// [`Default`] is the historical behaviour — no fill, no overlay text,
/// left-quadded — so the pre-existing verbs keep meaning what they meant.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RedactAppearance {
    /// `/IC`, the fill applied **on apply**. See [`RedactSpec::fill`] —
    /// `None` is TRANSPARENT, not black.
    pub fill: Option<Color>,
    /// `/OverlayText`, drawn over the fill on apply.
    pub overlay_text: Option<String>,
    /// `/Q` justification for the overlay text.
    pub quadding: Quadding,
}

impl RedactAppearance {
    /// Combine this appearance with a mark's geometry into a
    /// [`RedactSpec`].
    ///
    /// The one place the two halves are joined, so a caller that acquires
    /// geometry some new way (a future select-by-region, an OCR word box)
    /// cannot accidentally reintroduce a hard-coded appearance.
    #[must_use]
    pub fn to_spec(&self, quads: Vec<Quad>) -> RedactSpec {
        RedactSpec {
            quads,
            fill: self.fill,
            overlay_text: self.overlay_text.clone(),
            quadding: self.quadding,
        }
    }
}

/// Author a `/Redact` mark's annotation dictionary + red-outline preview
/// `/AP` `/N`. The redaction sibling of [`build_appearance`].
///
/// The annotation carries `/Subtype /Redact`, `/QuadPoints`, `/IC` (when a
/// fill was chosen), and `/OverlayText`/`/DA`/`/Q` (when overlay text was
/// given). Its preview appearance strokes each quad in red so the mark
/// reads as "marked, not done".
#[must_use]
pub fn build_redact_mark(spec: &RedactSpec) -> AuthoredAppearance {
    build_redact_mark_with(spec, QuadPointOrder::default())
}

/// As [`build_redact_mark`], with an explicit `/QuadPoints` corner order.
///
/// `/Redact` carries `/QuadPoints` in §12.5.6.10's format, so ambiguity
/// `QP-A1` applies here identically — and with one extra consequence worth
/// naming: a redaction mark whose quads a third-party tool re-derives in the
/// wrong order describes a **bow-tie region**, and a tool that then applied
/// the redaction from that geometry would remove the wrong area. pdfcer
/// applies its own redactions from the parsed quads rather than from the
/// emitted order, so this cannot mis-redact in pdfcer; it is a hazard for
/// hand-off to another tool.
#[must_use]
pub fn build_redact_mark_with(spec: &RedactSpec, order: QuadPointOrder) -> AuthoredAppearance {
    let rect = bounds_of(
        spec.quads
            .iter()
            .flat_map(|q| [q.ul, q.ur, q.ll, q.lr].into_iter()),
        1.0,
    );
    let mut annot = base_annot(b"Redact", rect);
    annot.insert(
        Name::from(b"QuadPoints"),
        text_markup_quad_object(&spec.quads, order),
    );
    if let Some(fill) = spec.fill {
        annot.insert(Name::from(b"IC"), fill.to_array());
    }
    if let Some(text) = &spec.overlay_text {
        annot.insert(Name::from(b"OverlayText"), contents_string(text));
        annot.insert(
            Name::from(b"Q"),
            Object::Integer(i64::from(spec.quadding as u8)),
        );
        let da = vartext::default_appearance_string(TEXT_FONT_RESOURCE, 0.0, TextColor::Gray(0.0));
        annot.insert(Name::from(b"DA"), Object::String(da));
    }

    // Preview appearance: a RED OUTLINE per quad. Never a solid fill —
    // that would read as a completed redaction (ui-spec mark-vs-apply).
    let mut b = ContentBuilder::new();
    b.set_stroke_rgb(1.0, 0.0, 0.0);
    b.set_line_width(1.0);
    for q in &spec.quads {
        b.move_to(q.ul.0, q.ul.1);
        b.line_to(q.ur.0, q.ur.1);
        b.line_to(q.lr.0, q.lr.1);
        b.line_to(q.ll.0, q.ll.1);
        b.close_subpath();
        b.paint(Paint::Stroke);
    }

    AuthoredAppearance {
        annot,
        ap_dict: form_dict(rect, Dict::new()),
        ap_content: b.into_bytes(),
        rect,
    }
}

/// The `/QuadPoints` array object for a quad list, in the authored Z-order
/// (module docs). Flat `x1 y1 … x4 y4` per quad, concatenated.
fn text_markup_quad_object(quads: &[Quad], order: QuadPointOrder) -> Object {
    Object::Array(
        quads
            .iter()
            .flat_map(|q| q.points(order))
            .map(Object::Real)
            .collect(),
    )
}

// =====================================================================
// Text-bearing annotations (Pass 6.2, §12.5.6.4/.6/.12 + §12.7.3.3)
// =====================================================================
//
// The three text-bearing subtypes — FreeText, Text (sticky note), Stamp —
// live in a **separate** spec/build/wire path from [`MarkupSpec`], because
// they share the §12.7.3.3 variable-text pipeline ([`crate::vartext`]) and
// need wiring the geometric family does not: a `/DA` string, a `/Contents`
// text value, per-subtype `/F` flags (Text is always NoZoom+NoRotate), and
// — for Text — a `/Popup` companion object. Keeping them apart leaves the
// Pass-6.1 geometric path (and its exhaustive `match` arms in
// [`crate::edit`]) byte-for-byte unchanged, which is what keeps the R46
// content-identity gate and the R34 round-trip gates from moving.
//
// The one artwork decision this file makes is the sticky-note *marker*
// (R44 choice (a) from `iso32000__s__12.5.6.md` §G-B): the spec supplies
// no icon artwork, so pdfcer authors its OWN plain marker — a filled
// note/paper glyph — never a reproduction of Acrobat's icon set (LEGAL §4
// trade dress). Its exact look is flagged as a `pdfcer-ui-specialist`
// refinement; the icon *names* are modelled and round-tripped regardless.

use crate::annot::AnnotFlags;
use crate::fontdata::Std14;
use crate::vartext::{self, FontResource, Quadding, TextColor, VarTextError};

/// The resource name pdfcer authors its standard-14 font under, inside a
/// text appearance's own `/Resources` `/Font` (FreeText has no `/DR`, so
/// the font lives in the appearance itself — `iso32000__s__12.5.6.md`
/// §G-A). The `/DA` `Tf` operator references this exact name.
const TEXT_FONT_RESOURCE: &[u8] = b"Helv";

/// A Text-annotation icon name (§12.5.6.4, Table 172). Default `Note`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StickyIcon {
    /// `/Comment`.
    Comment,
    /// `/Key`.
    Key,
    /// `/Note` — the default.
    #[default]
    Note,
    /// `/Help`.
    Help,
    /// `/NewParagraph`.
    NewParagraph,
    /// `/Paragraph`.
    Paragraph,
    /// `/Insert`.
    Insert,
}

impl StickyIcon {
    /// The `/Name` bytes (§12.5.6.4).
    const fn name(self) -> &'static [u8] {
        match self {
            Self::Comment => b"Comment",
            Self::Key => b"Key",
            Self::Note => b"Note",
            Self::Help => b"Help",
            Self::NewParagraph => b"NewParagraph",
            Self::Paragraph => b"Paragraph",
            Self::Insert => b"Insert",
        }
    }
}

/// A standard rubber-stamp name (§12.5.6.12, Table 181). These 14 are ISO
/// 32000-1's set; the default is `Draft`. "Additional names may be
/// supported"; Acrobat's fuller business catalogue is a capability-only
/// Acrobat_Features concern (never its artwork).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StampName {
    /// `/Approved`.
    Approved,
    /// `/Experimental`.
    Experimental,
    /// `/NotApproved`.
    NotApproved,
    /// `/AsIs`.
    AsIs,
    /// `/Expired`.
    Expired,
    /// `/NotForPublicRelease`.
    NotForPublicRelease,
    /// `/Confidential`.
    Confidential,
    /// `/Final`.
    Final,
    /// `/Sold`.
    Sold,
    /// `/Departmental`.
    Departmental,
    /// `/ForComment`.
    ForComment,
    /// `/TopSecret`.
    TopSecret,
    /// `/Draft` — the default.
    #[default]
    Draft,
    /// `/ForPublicRelease`.
    ForPublicRelease,
}

impl StampName {
    /// The `/Name` bytes (§12.5.6.12).
    const fn name(self) -> &'static [u8] {
        match self {
            Self::Approved => b"Approved",
            Self::Experimental => b"Experimental",
            Self::NotApproved => b"NotApproved",
            Self::AsIs => b"AsIs",
            Self::Expired => b"Expired",
            Self::NotForPublicRelease => b"NotForPublicRelease",
            Self::Confidential => b"Confidential",
            Self::Final => b"Final",
            Self::Sold => b"Sold",
            Self::Departmental => b"Departmental",
            Self::ForComment => b"ForComment",
            Self::TopSecret => b"TopSecret",
            Self::Draft => b"Draft",
            Self::ForPublicRelease => b"ForPublicRelease",
        }
    }

    /// The default label pdfcer paints for this stamp — the name with its
    /// internal word breaks spaced and upper-cased ("NotForPublicRelease"
    /// → "NOT FOR PUBLIC RELEASE"), the conventional rubber-stamp look.
    #[must_use]
    fn default_label(self) -> String {
        let raw = String::from_utf8_lossy(self.name());
        let mut spaced = String::new();
        for (i, ch) in raw.char_indices() {
            if i > 0 && ch.is_ascii_uppercase() {
                spaced.push(' ');
            }
            spaced.push(ch);
        }
        spaced.to_ascii_uppercase()
    }
}

impl From<Color> for TextColor {
    fn from(c: Color) -> Self {
        match c {
            Color::Gray(g) => TextColor::Gray(g),
            Color::Rgb(r, g, b) => TextColor::Rgb(r, g, b),
            Color::Cmyk(cy, m, y, k) => TextColor::Cmyk(cy, m, y, k),
        }
    }
}

/// A text-bearing annotation to author (Pass 6.2). See the module section
/// comment above for why this is separate from [`MarkupSpec`].
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum TextAnnotSpec {
    /// `/FreeText` (§12.5.6.6) — text drawn directly on the page via a
    /// baked `/AP` generated from `/DA` + `/Contents` + `/Q`.
    FreeText {
        /// The annotation rectangle in default user space.
        rect: Rect,
        /// The text to show (plain — `/RC` rich text is a non-goal, VT3).
        text: String,
        /// The standard-14 face to author (Latin only — symbolic faces
        /// are refused by [`crate::vartext`]).
        font: Std14,
        /// Point size; **`0.0` = auto-size** (VT1, disclosed).
        font_size: f64,
        /// Text fill colour.
        color: TextColor,
        /// Justification `/Q` (0/1/2).
        quadding: Quadding,
        /// Whether the box wraps to multiple lines.
        multiline: bool,
        /// Optional border stroke drawn around the box (`/BS`), or `None`
        /// for a borderless FreeText (Acrobat's default).
        border: Option<Color>,
        /// Border width in points (used only when `border` is `Some`).
        border_width: f64,
    },
    /// `/Text` (§12.5.6.4) — a "sticky note": a small marker on the page
    /// whose `/Contents` opens in a `/Popup`.
    Sticky {
        /// The annotation rectangle (the marker is fixed-size —
        /// NoZoom/NoRotate — so only its lower-left corner matters in
        /// practice; the width/height give the marker its size).
        rect: Rect,
        /// The predefined icon name (default `Note`).
        icon: StickyIcon,
        /// The note text (`/Contents`, shown in the popup, never painted
        /// on the page).
        contents: String,
        /// The marker colour `/C`.
        color: Color,
        /// Whether the popup starts open (`/Open`, default false).
        open: bool,
    },
    /// `/Stamp` (§12.5.6.12) — a rubber stamp: pdfcer's own framed-text
    /// appearance (a bordered box with the label rendered in Base-14).
    Stamp {
        /// The annotation rectangle.
        rect: Rect,
        /// The standard stamp name (default `Draft`).
        name: StampName,
        /// A custom label, or `None` to use the name's default label.
        label: Option<String>,
        /// The frame + text colour.
        color: Color,
    },
}

/// The result of authoring one text-bearing annotation: everything
/// [`crate::edit::EditSession`] needs to wire it, plus the disclosures
/// (auto-size, unencodable chars) the front end surfaces.
#[derive(Debug, Clone, PartialEq)]
pub struct AuthoredTextAnnot {
    /// The annotation dictionary (no `/AP`, `/P`, `/F` or `/Popup` link —
    /// the session adds those once it has object numbers).
    pub annot: Dict,
    /// The `/AP` `/N` form-XObject dictionary (`/BBox` = `[0 0 W H]`,
    /// `/Resources` carrying the standard-14 font).
    pub ap_dict: Dict,
    /// The appearance content-stream bytes (raw, unfiltered).
    pub ap_content: Vec<u8>,
    /// The computed annotation `/Rect`, positive-area (WF4).
    pub rect: Rect,
    /// The `/F` flags to set on the annotation (Print, plus
    /// NoZoom+NoRotate for a sticky note).
    pub flags: u32,
    /// A `/Popup` companion dictionary to create (Text only), without its
    /// `/Parent` back-reference (the session wires that). `None` for
    /// FreeText/Stamp.
    pub popup: Option<Dict>,
    /// `Some(size)` when auto-size (VT1) chose the size — disclosed.
    pub applied_autosize: Option<f64>,
    /// How many characters had no `WinAnsi` code and were substituted with
    /// `?` (a named Base-14-Latin limit).
    pub unencodable_chars: usize,
}

/// Author a text-bearing annotation's dictionary + `/AP` `/N` appearance
/// (Pass 6.2). The text-family sibling of [`build_appearance`].
///
/// # Errors
///
/// [`VarTextError`] when the variable-text generation fails — a `/DA` this
/// function itself builds cannot be malformed or fontless, so in practice
/// the only reachable variant is [`VarTextError::SymbolicFont`] if a
/// caller selects `Symbol`/`ZapfDingbats` for a FreeText/Stamp label.
pub fn build_text_annotation(spec: &TextAnnotSpec) -> Result<AuthoredTextAnnot, VarTextError> {
    match spec {
        TextAnnotSpec::FreeText {
            rect,
            text,
            font,
            font_size,
            color,
            quadding,
            multiline,
            border,
            border_width,
        } => free_text(
            *rect,
            text,
            *font,
            *font_size,
            *color,
            *quadding,
            *multiline,
            *border,
            *border_width,
        ),
        TextAnnotSpec::Sticky {
            rect,
            icon,
            contents,
            color,
            open,
        } => Ok(sticky_note(*rect, *icon, contents, *color, *open)),
        TextAnnotSpec::Stamp {
            rect,
            name,
            label,
            color,
        } => stamp(*rect, *name, label.as_deref(), *color),
    }
}

/// The `/AP` form dict for a text appearance: `/BBox = [0 0 W H]` (origin
/// 0, sized to the rect — §12.7.3.3), identity `/Matrix`, and the
/// generated `/Resources`. Distinct from [`form_dict`], whose `/BBox` is
/// the page-space rect (the geometric path draws in absolute coords).
fn text_form_dict(rect: Rect, resources: Dict) -> Dict {
    let local = Rect {
        llx: 0.0,
        lly: 0.0,
        urx: rect.width(),
        ury: rect.height(),
    };
    form_dict(local, resources)
}

/// Emit `Contents` as a text string (§7.9.2) for an annotation dict.
fn contents_string(text: &str) -> Object {
    Object::String(crate::edit::encode_text_string(text))
}

/// A generated widget-field appearance (Pass 7 form fill): the `/AP` `/N`
/// form-XObject dictionary and its content bytes, plus the fuzzy-never-sneaky
/// disclosures the §12.7.3.3 generator surfaces.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldAppearance {
    /// The `/AP` `/N` form-XObject dict (`/BBox = [0 0 w h]`, identity
    /// `/Matrix`, the generated `/Resources`). `/Length` is added by the
    /// serializer.
    pub ap_dict: Dict,
    /// The `/Tx BMC … EMC` content-stream bytes (raw, unfiltered).
    pub content: Vec<u8>,
    /// `Some(size)` when the `/DA` requested auto-size (`0 Tf`) and a size was
    /// chosen — surfaced for disclosure (VT1).
    pub applied_autosize: Option<f64>,
    /// Which constraint decided [`Self::applied_autosize`]. See
    /// [`crate::vartext::AutoFitBound`].
    pub applied_autosize_bound: Option<crate::vartext::AutoFitBound>,
    /// The field's `/DA` named a colour space pdfcer does not model, so this
    /// appearance was generated in black. See
    /// [`crate::vartext::VarTextAppearance::da_colour_unmodelled`].
    pub da_colour_unmodelled: bool,
    /// How many characters had no `WinAnsi` code and were substituted with
    /// `?` (a named Base-14-Latin limit, disclosed).
    pub unencodable_chars: usize,
}

/// One state of a check-box's `/AP` `/N` sub-dictionary: the form-XObject
/// dict and its content bytes.
///
/// A check box's `/AP` `/N` is **not** a stream, as it is for every other
/// field type — it is a DICTIONARY KEYED BY STATE NAME (§12.7.4.2.3), and
/// each entry is a complete appearance stream:
///
/// ```text
/// /AP << /N << /Yes 12 0 R  /Off 13 0 R >> >>
/// ```
///
/// `/AS` then names which of those entries is painted (§12.5.5). This type
/// exists so the caller can stage both streams and assemble that
/// sub-dictionary, rather than the builder needing an object allocator.
#[derive(Debug, Clone, PartialEq)]
pub struct CheckBoxStateAppearance {
    /// The form-XObject dict for this state (`/BBox = [0 0 w h]`).
    pub ap_dict: Dict,
    /// The state's content-stream bytes (raw, unfiltered).
    pub content: Vec<u8>,
}

/// Build a check box's **two** appearance states — on and off — as
/// vector-drawn artwork (§12.7.4.2.3).
///
/// Returns `(off, on)`.
///
/// # Why this is vector-drawn rather than a ZapfDingbats glyph
///
/// Acrobat draws a check box's mark as character `4` (a check) from the
/// **ZapfDingbats** font, named through the widget's `/DA`. pdfcer cannot
/// currently take that route, and the reason is worth recording because it
/// looks like an arbitrary divergence and is not:
///
/// [`Std14::ZapfDingbats`](crate::fontdata::Std14) exists and its metrics are
/// present, but the only appearance generator this crate has —
/// [`vartext::build_variable_text`], the one R92 requires everything to share
/// — is **Latin-only**: it encodes through `WinAnsi` and raises
/// [`VarTextError`] on a symbolic font. So a glyph-drawn check would need a
/// SECOND generator, which is exactly what R92 forbids, or a symbolic-font
/// path through the shared one, which is a font-machinery Pass and not a
/// field-authoring one.
///
/// Vector artwork has no such dependency: two line segments and a rectangle,
/// drawn directly into the appearance stream, produce a check box that every
/// conforming viewer paints identically and that pdfcer's own renderer paints
/// too — which the `/MK`-only border of slice 1 notably does NOT (R43,
/// named-not-painted). The visual difference from Acrobat's glyph is a
/// slightly different check shape. The behavioural difference is none.
///
/// # The off state is drawn, not omitted
///
/// §12.7.4.2.3 makes the off appearance OPTIONAL — a viewer may paint nothing
/// when `/AS` selects a missing state. It is written anyway so the box's
/// border is visible when unchecked. An unchecked box that renders as blank
/// paper is indistinguishable from no field at all, which would make an
/// unfilled form look empty rather than look like a form.
///
/// # Errors
///
/// Never — the geometry is fixed and no text is laid out. Returns a plain
/// pair rather than a `Result` for that reason.
#[must_use]
pub fn build_check_box_appearances(
    width: f64,
    height: f64,
) -> (CheckBoxStateAppearance, CheckBoxStateAppearance) {
    let (w, h) = (width.max(1.0), height.max(1.0));
    let rect = Rect {
        llx: 0.0,
        lly: 0.0,
        urx: w,
        ury: h,
    };
    // A half-unit inset keeps the 1.0-wide border stroke INSIDE the BBox.
    // A stroke is centred on its path, so a border drawn at the BBox edge
    // would have half its width clipped away by the form XObject.
    let inset = 0.5;

    let border = |b: &mut ContentBuilder| {
        b.set_stroke_gray(0.0);
        b.set_line_width(1.0);
        b.rect(inset, inset, w - 2.0 * inset, h - 2.0 * inset);
        b.paint(Paint::Stroke);
    };

    let mut off = ContentBuilder::new();
    border(&mut off);

    let mut on = ContentBuilder::new();
    border(&mut on);
    // The check itself: a short down-stroke into a long up-stroke, scaled to
    // the box and centred, with a round join so the vertex reads as a tick
    // rather than as two crossing lines. Proportions are the conventional
    // check: the descender lands ~40% across and the ascender rises above
    // the start of the down-stroke.
    let m = (w.min(h)) * 0.25;
    let (cx, cy) = (w / 2.0, h / 2.0);
    let s = (w.min(h) - 2.0 * m) / 2.0;
    on.set_stroke_gray(0.0);
    on.set_line_width((s * 0.32).max(0.6));
    on.set_line_cap(LineCap::Round);
    on.set_line_join(LineJoin::Round);
    on.move_to(cx - s, cy + s * 0.1);
    on.line_to(cx - s * 0.25, cy - s * 0.7);
    on.line_to(cx + s, cy + s * 0.8);
    on.paint(Paint::Stroke);

    // No /Resources entries: nothing here names a font, an XObject or a
    // colour space, so an empty dict is correct rather than merely minimal.
    (
        CheckBoxStateAppearance {
            ap_dict: form_dict(rect, Dict::new()),
            content: off.into_bytes(),
        },
        CheckBoxStateAppearance {
            ap_dict: form_dict(rect, Dict::new()),
            content: on.into_bytes(),
        },
    )
}

/// Build a radio button's **two** appearance states — on and off — as
/// vector-drawn artwork (§12.7.4.2.3).
///
/// Returns `(off, on)`, matching [`build_check_box_appearances`]'s order so
/// the two are interchangeable at the call site.
///
/// # Why a circle, and why that is not merely cosmetic
///
/// §12.7.4.2 gives radio buttons and check boxes the same `/FT /Btn` type and
/// distinguishes them by `/Ff` bit 16 alone — nothing in the FILE says one
/// should look round. The round/square convention is a viewer-and-toolkit
/// one, and it is load-bearing for the operator rather than decorative: a
/// check box means *"toggle me independently"* and a radio means *"choose one
/// of these"*, and that difference is invisible until you click. A radio group
/// drawn as square boxes is a form that lies about its own behaviour.
///
/// So pdfcer draws the circle deliberately, and records that this is **pdfcer's
/// own design choice** rather than a spec requirement or an "Acrobat does it"
/// parity claim (§3.6.3's discipline, applied to appearance rather than to
/// deletion).
///
/// # The dot is FILLED, the ring is STROKED
///
/// Both states draw the ring so an unselected radio still reads as a control
/// (the same reasoning as the check box's drawn off state — a blank one is
/// indistinguishable from no field). Only the on state adds the centre dot.
/// The dot is filled rather than stroked because a stroked small circle at
/// typical widget sizes collapses into a smudge: at a 12pt widget the dot is
/// ~3pt across, and a 1pt stroke centred on that path leaves almost no hole.
///
/// # Circle construction
///
/// Four cubic Béziers, the standard circle approximation with control points
/// at `k = 0.5523 · r` — [`ContentBuilder`](crate::writer::content::ContentBuilder)
/// offers `curve_to` and no arc operator, and PDF has no arc operator either
/// (§8.5.2 gives only `m`, `l`, `c`, `v`, `y`, `h`, `re`), so every "circle"
/// in a PDF is this approximation. Max radial error ≈ 0.027 %, far below a
/// device pixel at any plausible widget size.
///
/// # Errors
///
/// Never — fixed geometry, no text laid out. Returns a plain pair, matching
/// [`build_check_box_appearances`].
#[must_use]
pub fn build_radio_button_appearances(
    width: f64,
    height: f64,
) -> (CheckBoxStateAppearance, CheckBoxStateAppearance) {
    let (w, h) = (width.max(1.0), height.max(1.0));
    let rect = Rect {
        llx: 0.0,
        lly: 0.0,
        urx: w,
        ury: h,
    };
    let (cx, cy) = (w / 2.0, h / 2.0);
    // The half-unit inset keeps the 1.0-wide ring stroke inside the BBox, for
    // the same reason the check box insets its border: a stroke straddles its
    // path, so one drawn at the edge loses half its width to the clip.
    let inset = 0.5;
    let r = (w.min(h) / 2.0 - inset).max(0.5);

    /// Append a circle of radius `r` centred at `(cx, cy)` as four Béziers.
    fn circle(b: &mut ContentBuilder, cx: f64, cy: f64, r: f64) {
        // The magic constant: the control-point offset that makes a cubic
        // Bézier hug a quarter arc. 4/3·(√2 − 1).
        let k = r * 0.552_284_749_830_793_4;
        b.move_to(cx + r, cy);
        b.curve_to(cx + r, cy + k, cx + k, cy + r, cx, cy + r);
        b.curve_to(cx - k, cy + r, cx - r, cy + k, cx - r, cy);
        b.curve_to(cx - r, cy - k, cx - k, cy - r, cx, cy - r);
        b.curve_to(cx + k, cy - r, cx + r, cy - k, cx + r, cy);
    }

    let ring = |b: &mut ContentBuilder| {
        b.set_stroke_gray(0.0);
        b.set_line_width(1.0);
        circle(b, cx, cy, r);
        b.paint(Paint::Stroke);
    };

    let mut off = ContentBuilder::new();
    ring(&mut off);

    let mut on = ContentBuilder::new();
    ring(&mut on);
    // The dot at half the ring's radius — the conventional proportion, and
    // large enough to stay visible when a form is printed at reduced scale.
    on.set_fill_gray(0.0);
    circle(&mut on, cx, cy, (r * 0.5).max(0.4));
    on.paint(Paint::Fill);

    // No /Resources entries: no font, XObject or colour space is named.
    (
        CheckBoxStateAppearance {
            ap_dict: form_dict(rect, Dict::new()),
            content: off.into_bytes(),
        },
        CheckBoxStateAppearance {
            ap_dict: form_dict(rect, Dict::new()),
            content: on.into_bytes(),
        },
    )
}

/// Build a **push button's** single appearance stream (§12.7.4.2.2): a
/// filled, bordered plate with the `/MK` `/CA` caption centred on it.
///
/// # Why this returns ONE stream, not a keyed sub-dictionary
///
/// Check boxes and radio buttons return a *pair* because §12.7.4.2.3 makes
/// their `/AP` `/N` a dictionary keyed by state name and `/AS` selects the
/// entry to paint. A push button **has no states** — §12.7.4.2.2 is explicit
/// that it *"retains no permanent value"* and so *"shall not use the `V` and
/// `DV` entries"*. With no `/V` there is nothing for `/AS` to track, so its
/// `/AP` `/N` is a plain stream, the same shape a text field's is. This is
/// the one place where the three `/FT /Btn` kinds diverge structurally rather
/// than only by a flag bit, and getting it wrong produces a button that some
/// viewers paint and others leave blank.
///
/// # The caption goes through the SHARED variable-text generator (R92)
///
/// A caption is not variable text in the §12.7.3.3 sense — it comes from
/// `/MK` `/CA`, not from `/V`, and it never changes when the form is filled.
/// It is nonetheless laid out by [`vartext::build_variable_text`] rather than
/// by a private text emitter, because R92's rule is about *how many text
/// layout engines this crate contains*, not about which dictionary key the
/// string came from. A second emitter would mean a second set of encoding,
/// metrics and clipping bugs to find.
///
/// [`vartext::build_variable_text`]: crate::vartext::build_variable_text
///
/// # Vertical centring, and the trap inside it
///
/// [`vartext::build_variable_text`] places its first baseline at the TOP of
/// the box it is given, which is right for a text field and wrong for a
/// button caption. The fix is the same one the stamp-label builder uses: lay
/// the caption
/// into a band exactly one line high ([`vartext::text_band_height`]) and
/// translate that band to the vertical centre.
///
/// The trap is auto-size. If the caller's `/DA` says `0 Tf` (auto), the size
/// must be resolved **here**, against the WIDGET height, before the band is
/// measured — and then handed to the generator as an explicit size. Passing
/// the auto `/DA` straight through would make the generator re-derive the
/// size from the *band's* height, which is itself a function of the size it
/// would have picked; the two disagree and the caption comes out mis-sized
/// inside a band built for a different one.
///
/// # A caption wider than the plate is CLIPPED, and reads left-to-right
///
/// Verified by rendering, not assumed. [`Quadding::Center`] computes the
/// line's x as `((w - line_width) / 2).max(TEXT_PAD)`, so once the caption is
/// wider than the plate the centring clamps to the left pad and the overflow
/// is clipped off the right by the form XObject's `/BBox`. The operator sees
/// the START of their caption and can tell it was truncated.
///
/// That is the better of the two available behaviours and it is not an
/// accident of the clamp: a genuinely centred overlong caption would lose its
/// first and last words to the clip and read as a meaningless middle
/// fragment. Nothing shrinks the text to fit, because `/MK` `/CA` is not
/// variable text and the auto-size heuristic is a function of the box HEIGHT
/// (§12.7.3.3 mandates only that much) — a width-fitting pass would be a
/// second heuristic, applied to one field type, with no spec behind it.
///
/// # The plate is drawn, not left to `/MK`
///
/// `/MK` `/BG` and `/BC` are written by the caller as well, for viewers that
/// synthesise a dynamic appearance. They are not what makes the button
/// visible in pdfcer: R43 is the standing named-not-painted rule — pdfcer
/// declines to synthesise appearance from `/MK` at display time — so the
/// plate is vector artwork inside this stream, exactly as a check box's
/// border is. A push button whose only border was `/MK` `/BC` would be an
/// invisible click target in pdfcer's own renderer, which for a control whose
/// entire purpose is to be clicked is worse than for any other field type.
///
/// # Errors
///
/// Any [`VarTextError`] from the generator — a malformed `/DA`, a `/DA` font
/// name absent from `resources`, or a symbolic font this Latin-only
/// generator cannot lay out.
pub fn build_push_button_appearance(
    width: f64,
    height: f64,
    caption: &str,
    da: &[u8],
    resources: &[FontResource],
) -> Result<FieldAppearance, VarTextError> {
    // A widget /Rect can be degenerate; floor each axis at one point so the
    // /AP BBox stays a §12.5.5 positive result (WF4), matching
    // `build_field_text_appearance`'s posture.
    let (w, h) = (width.max(1.0), height.max(1.0));
    let bbox = Rect {
        llx: 0.0,
        lly: 0.0,
        urx: w,
        ury: h,
    };

    // THE PLATE. A light-grey fill with a black keyline is the conventional
    // button look every desktop toolkit converges on, and it is pdfcer's own
    // design choice rather than a spec requirement or a parity claim — the
    // same discipline `build_radio_button_appearances` records for drawing a
    // circle. The half-unit inset keeps the 1.0-wide stroke inside the BBox:
    // a stroke straddles its path, so one drawn at the edge loses half its
    // width to the form XObject's clip.
    let inset = 0.5;
    let mut plate = ContentBuilder::new();
    plate.set_fill_gray(PUSH_BUTTON_PLATE_GRAY);
    plate.rect(0.0, 0.0, w, h);
    plate.paint(Paint::Fill);
    plate.set_stroke_gray(0.0);
    plate.set_line_width(1.0);
    plate.rect(inset, inset, w - 2.0 * inset, h - 2.0 * inset);
    plate.paint(Paint::Stroke);
    let mut content = plate.into_bytes();

    // THE CAPTION. Resolved size first (see the auto-size trap above), then a
    // one-line band, then the translate.
    let parsed = vartext::parse_default_appearance(da)?;
    let size = if parsed.font_size > 0.0 {
        parsed.font_size
    } else {
        vartext::auto_size(h)
    };
    let font = resources
        .iter()
        .find(|r| r.name == parsed.font_name)
        .map_or(Std14::Helvetica, |r| r.font);
    let resolved_da = vartext::default_appearance_string(
        &parsed.font_name,
        size,
        parsed.color.unwrap_or(TextColor::Gray(0.0)),
    );

    let band_h = vartext::text_band_height(font, size);
    let band = Rect {
        llx: 0.0,
        lly: 0.0,
        urx: w,
        ury: band_h,
    };
    let va = vartext::build_variable_text(
        band,
        caption,
        &resolved_da,
        Quadding::Center,
        false,
        resources,
    )?;

    // `dy` may be negative when the caption's band is taller than the widget
    // — a two-point-high button with an eight-point caption. The band is
    // translated anyway rather than clamped to zero: centring an oversized
    // line means it overflows equally top and bottom, and the form XObject's
    // BBox clips both. Clamping would push the whole overflow out of the
    // bottom, which reads as a caption that is missing rather than one that
    // does not fit.
    let dy = (h - band_h) / 2.0;
    let mut open = ContentBuilder::new();
    open.save_state();
    open.concat_matrix(1.0, 0.0, 0.0, 1.0, 0.0, dy);
    content.extend_from_slice(&open.into_bytes());
    content.extend_from_slice(&va.content);
    let mut close = ContentBuilder::new();
    close.restore_state();
    content.extend_from_slice(&close.into_bytes());

    Ok(FieldAppearance {
        ap_dict: form_dict(bbox, va.resources),
        content,
        applied_autosize: va.applied_autosize.or({
            // The generator saw an explicit size (this function resolved it),
            // so it reports no auto-size of its own. The DISCLOSURE is still
            // owed when the caller's /DA asked for one — VT1 is about the
            // operator learning that a size was chosen for them, and who did
            // the choosing is not the part they need told.
            if parsed.font_size > 0.0 {
                None
            } else {
                Some(size)
            }
        }),
        applied_autosize_bound: va.applied_autosize_bound,
        da_colour_unmodelled: va.da_colour_unmodelled,
        unencodable_chars: va.unencodable_chars,
    })
}

/// The plate grey a push button is drawn on. Light enough that black
/// caption text stays legible, dark enough to read as a raised control
/// against white paper.
///
/// Public because the same value has to appear TWICE in a created button —
/// once as the fill inside the `/AP` stream this module builds, and once as
/// the widget's `/MK` `/BG` for viewers that synthesise their own appearance
/// instead. Two literals would let the baked look and the declared look
/// drift apart, and the drift would only show up in a different viewer than
/// the one it was authored in.
pub const PUSH_BUTTON_PLATE_GRAY: f64 = 0.85;

/// Generate a text/choice **widget field** appearance (§12.7.3.3) for Pass 7
/// form fill.
///
/// This is the widget half of R49's *one appearance pipeline*: it reuses the
/// exact [`vartext::build_variable_text`] generator Pass 6.2's FreeText uses,
/// wrapped in the same `[0 0 w h]` form-XObject packaging as
/// [`text_form_dict`]. The field's resolved `/DA` (`da`), `/Q` (`quad`), and
/// `Multiline` flag drive layout; `resources` maps the `/DA` font name(s) to
/// their standard-14 faces (the caller resolves them from the AcroForm
/// `/DR`).
///
/// # Errors
///
/// Any [`VarTextError`] from the generator — a malformed `/DA`, a `/DA` font
/// name absent from `resources`, or a symbolic font this Latin generator
/// cannot lay out.
pub fn build_field_text_appearance(
    width: f64,
    height: f64,
    text: &str,
    da: &[u8],
    quad: Quadding,
    multiline: bool,
    resources: &[FontResource],
) -> Result<FieldAppearance, VarTextError> {
    // A widget /Rect can be degenerate; the generator's clip and metrics need
    // a positive box, so floor each axis at one point (matching WF4's posture
    // for authored appearances). The visible result is simply a tiny box.
    let bbox = Rect {
        llx: 0.0,
        lly: 0.0,
        urx: width.max(1.0),
        ury: height.max(1.0),
    };
    let va = vartext::build_variable_text(bbox, text, da, quad, multiline, resources)?;
    Ok(FieldAppearance {
        ap_dict: text_form_dict(bbox, va.resources),
        content: va.content,
        applied_autosize: va.applied_autosize,
        applied_autosize_bound: va.applied_autosize_bound,
        da_colour_unmodelled: va.da_colour_unmodelled,
        unencodable_chars: va.unencodable_chars,
    })
}

/// FreeText (§12.5.6.6): `/DA`-driven text in a `[0 0 W H]` appearance,
/// optionally framed by a `/BS` border.
#[allow(clippy::too_many_arguments)]
fn free_text(
    rect: Rect,
    text: &str,
    font: Std14,
    font_size: f64,
    color: TextColor,
    quadding: Quadding,
    multiline: bool,
    border: Option<Color>,
    border_width: f64,
) -> Result<AuthoredTextAnnot, VarTextError> {
    let rect = positive_rect(rect);
    let w = rect.width();
    let h = rect.height();
    let bbox = Rect {
        llx: 0.0,
        lly: 0.0,
        urx: w,
        ury: h,
    };

    let da = vartext::default_appearance_string(TEXT_FONT_RESOURCE, font_size, color);
    let resources = [FontResource {
        name: TEXT_FONT_RESOURCE.to_vec(),
        font,
    }];
    let va = vartext::build_variable_text(bbox, text, &da, quadding, multiline, &resources)?;

    // Optional border, drawn OUTSIDE the /Tx text block (so it is not
    // clipped by the text clip) — the frame first, then the text body.
    let mut content = Vec::new();
    if let (Some(bc), true) = (border, border_width > 0.0) {
        let mut b = ContentBuilder::new();
        bc.apply_stroke(&mut b);
        b.set_line_width(border_width);
        let inset = border_width / 2.0;
        b.rect(
            inset,
            inset,
            (w - border_width).max(0.0),
            (h - border_width).max(0.0),
        );
        b.paint(Paint::Stroke);
        content = b.into_bytes();
    }
    content.extend_from_slice(&va.content);

    let mut annot = base_annot(b"FreeText", rect);
    annot.insert(Name::from(b"DA"), Object::String(da));
    annot.insert(Name::from(b"Contents"), contents_string(text));
    // /Q only when non-default (0 = left), keeping the dict minimal.
    if quadding != Quadding::Left {
        annot.insert(Name::from(b"Q"), Object::Integer(quadding.code()));
    }
    if let Some(bc) = border {
        annot.insert(Name::from(b"C"), bc.to_array());
        annot.insert(Name::from(b"BS"), border_style(border_width));
    }

    Ok(AuthoredTextAnnot {
        annot,
        ap_dict: text_form_dict(rect, va.resources),
        ap_content: content,
        rect,
        flags: AnnotFlags::PRINT,
        popup: None,
        applied_autosize: va.applied_autosize,
        unencodable_chars: va.unencodable_chars,
    })
}

/// The `/F` value a sticky note always carries: Print + NoZoom + NoRotate
/// (§12.5.6.4 — the icon does not scale or rotate with the page).
const STICKY_FLAGS: u32 = AnnotFlags::PRINT | AnnotFlags::NO_ZOOM | AnnotFlags::NO_ROTATE;

/// Text / sticky note (§12.5.6.4): pdfcer's own plain marker appearance
/// (R44 choice (a)) — a filled "note" glyph. The note text lives in
/// `/Contents` and is shown by the reader's popup, never painted on the
/// page.
fn sticky_note(
    rect: Rect,
    icon: StickyIcon,
    contents: &str,
    color: Color,
    open: bool,
) -> AuthoredTextAnnot {
    let rect = positive_rect(rect);
    let w = rect.width();
    let h = rect.height();

    // pdfcer's own marker: a filled rounded page/note glyph with a folded
    // top-right corner and a few "text" rules — clearly pdfcer-authored,
    // not Acrobat trade dress. Drawn in the [0 0 W H] appearance box.
    let mut b = ContentBuilder::new();
    let fold = (w.min(h) * 0.28).min(6.0);
    // Body fill (the note colour) + a darker stroke for definition.
    color.apply_fill(&mut b);
    let border = Color::Gray(0.25);
    border.apply_stroke(&mut b);
    b.set_line_width((w.min(h) * 0.05).max(0.5));
    b.set_line_join(LineJoin::Round);
    // Page outline with a dog-eared top-right corner.
    let m = (w.min(h) * 0.10).max(0.5); // inset margin
    b.move_to(m, m);
    b.line_to(w - m, m);
    b.line_to(w - m, h - m - fold);
    b.line_to(w - m - fold, h - m);
    b.line_to(m, h - m);
    b.close_subpath();
    b.paint(Paint::FillStroke);
    // The fold triangle.
    b.move_to(w - m - fold, h - m);
    b.line_to(w - m - fold, h - m - fold);
    b.line_to(w - m, h - m - fold);
    b.paint(Paint::Stroke);
    // Two short "text" rules suggesting a note.
    for frac in [0.42, 0.60] {
        let y = m + (h - 2.0 * m) * frac;
        b.move_to(m + fold, y);
        b.line_to(w - m - fold, y);
        b.paint(Paint::Stroke);
    }

    let mut annot = base_annot(b"Text", rect);
    annot.insert(
        Name::from(b"Name"),
        Object::Name(Name(icon.name().to_vec())),
    );
    annot.insert(Name::from(b"Contents"), contents_string(contents));
    annot.insert(Name::from(b"C"), color.to_array());
    annot.insert(Name::from(b"Open"), Object::Boolean(open));

    // The /Popup companion: placed to the right of the note. It carries no
    // appearance (a popup is never painted as page content — Pass 6.0 X4);
    // /Parent is wired by the session.
    let popup_rect = Rect {
        llx: rect.urx,
        lly: (rect.lly - 108.0).max(0.0),
        urx: rect.urx + 150.0,
        ury: rect.ury,
    };
    let mut popup = base_annot(b"Popup", popup_rect);
    popup.insert(Name::from(b"Open"), Object::Boolean(open));

    AuthoredTextAnnot {
        annot,
        ap_dict: text_form_dict(rect, Dict::new()),
        ap_content: b.into_bytes(),
        rect,
        flags: STICKY_FLAGS,
        popup: Some(popup),
        applied_autosize: None,
        unencodable_chars: 0,
    }
}

/// Stamp (§12.5.6.12): pdfcer's own framed-text look — a bordered box with
/// the label centred via the §12.7.3.3 pipeline. NOT Acrobat stamp
/// artwork (LEGAL §4).
fn stamp(
    rect: Rect,
    name: StampName,
    label: Option<&str>,
    color: Color,
) -> Result<AuthoredTextAnnot, VarTextError> {
    let rect = positive_rect(rect);
    let w = rect.width();
    let h = rect.height();
    let label = label
        .map(str::to_owned)
        .unwrap_or_else(|| name.default_label());

    // Frame: a stroked rounded rectangle in the stamp colour.
    let frame_w = (h * 0.06).max(1.5);
    let mut b = ContentBuilder::new();
    color.apply_stroke(&mut b);
    b.set_line_width(frame_w);
    b.set_line_join(LineJoin::Round);
    b.rect(
        frame_w,
        frame_w,
        (w - 2.0 * frame_w).max(0.0),
        (h - 2.0 * frame_w).max(0.0),
    );
    b.paint(Paint::Stroke);
    let mut content = b.into_bytes();

    // The label: a single centred line, auto-fit to the box, translated to
    // the vertical centre of the frame. Reuses the FreeText/widget text
    // pipeline (the whole point of sharing it).
    let size = (h * 0.42).clamp(8.0, 28.0);
    let da = vartext::default_appearance_string(TEXT_FONT_RESOURCE, size, TextColor::from(color));
    let resources = [FontResource {
        name: TEXT_FONT_RESOURCE.to_vec(),
        font: Std14::HelveticaBold,
    }];
    let band_h = vartext::text_band_height(Std14::HelveticaBold, size);
    let band = Rect {
        llx: 0.0,
        lly: 0.0,
        urx: w,
        ury: band_h,
    };
    let va = vartext::build_variable_text(band, &label, &da, Quadding::Center, false, &resources)?;
    // Translate the label band to the vertical centre of the box.
    let dy = (h - band_h) / 2.0;
    let mut wrapped = ContentBuilder::new();
    wrapped.save_state();
    wrapped.concat_matrix(1.0, 0.0, 0.0, 1.0, 0.0, dy);
    let mut label_bytes = wrapped.into_bytes();
    label_bytes.extend_from_slice(&va.content);
    let mut close = ContentBuilder::new();
    close.restore_state();
    label_bytes.extend_from_slice(&close.into_bytes());
    content.extend_from_slice(&label_bytes);

    let mut annot = base_annot(b"Stamp", rect);
    annot.insert(
        Name::from(b"Name"),
        Object::Name(Name(name.name().to_vec())),
    );

    Ok(AuthoredTextAnnot {
        annot,
        ap_dict: text_form_dict(rect, va.resources),
        ap_content: content,
        rect,
        flags: AnnotFlags::PRINT,
        popup: None,
        applied_autosize: va.applied_autosize,
        unencodable_chars: va.unencodable_chars,
    })
}

/// Transform a [`MarkupSpec`]'s page-space geometry by `m` (`Pass 120.4`).
///
/// Returns the moved spec and whether a **rectangle had to be enclosed rather
/// than rotated**.
///
/// # ★ The one thing a markup annotation cannot express
///
/// `Square` and `Circle` are `/Rect`-based (§12.5.2: `/Rect` is an
/// axis-aligned rectangle in default user space), so a rotated one **has no
/// spelling**. This is the `re` problem from `Pass 113.0` in a different
/// carrier, and it has the same two possible answers.
///
/// **The default is to ENCLOSE and disclose**, not to refuse. An operator
/// pasting a comment onto a rotated placement expects the comment to arrive;
/// refusing the whole paste over an annotation's coordinate convention would
/// be pdfcer declining to do the thing it can do. The enclosing rectangle is
/// the only shape the format admits, so this is not pdfcer choosing between two
/// renderings — it is pdfcer doing the one available and saying so.
///
/// Point-based variants (`Line`, `Ink`, `Polygon`, `Cloud`, `PolyLine`) and
/// `TextMarkup`'s quads carry no such limit: every point maps exactly, and a
/// rotation is faithful.
#[must_use]
pub fn transform_spec(spec: &MarkupSpec, m: crate::vector::Matrix) -> (MarkupSpec, bool) {
    let pt = |p: &(f64, f64)| {
        let mapped = m.map_point(crate::vector::Point::new(p.0, p.1));
        (mapped.x, mapped.y)
    };
    let pts = |v: &[(f64, f64)]| v.iter().map(pt).collect::<Vec<_>>();
    // A rectangle's four corners mapped and re-enclosed — right under a
    // rotation in the only sense a rectangle can be, and exact otherwise.
    let rect = |r: &Rect| {
        let corners = [
            (r.llx, r.lly),
            (r.urx, r.lly),
            (r.urx, r.ury),
            (r.llx, r.ury),
        ]
        .map(|c| pt(&c));
        let (mut llx, mut lly) = (f64::INFINITY, f64::INFINITY);
        let (mut urx, mut ury) = (f64::NEG_INFINITY, f64::NEG_INFINITY);
        for (x, y) in corners {
            llx = llx.min(x);
            lly = lly.min(y);
            urx = urx.max(x);
            ury = ury.max(y);
        }
        Rect { llx, lly, urx, ury }
    };
    // A width is a length, so it scales by the matrix's linear magnitude. The
    // square root of |det| is the isotropic equivalent — exact for a uniform
    // scale and a rotation, and the honest average for a non-uniform one,
    // where a single stroke width cannot be right in both directions anyway.
    let scale = m.determinant().abs().sqrt();
    let w = |v: f64| {
        if scale.is_finite() && scale > 0.0 {
            v * scale
        } else {
            v
        }
    };

    // No wildcard arm: a new markup variant must consciously decide how it
    // transforms, at compile time, rather than shipping as an un-moved copy.
    match spec {
        MarkupSpec::Square {
            rect: r,
            border,
            interior,
            border_width,
            border_effect,
        } => (
            MarkupSpec::Square {
                rect: rect(r),
                border: *border,
                interior: *interior,
                border_width: w(*border_width),
                border_effect: *border_effect,
            },
            true,
        ),
        MarkupSpec::Circle {
            rect: r,
            border,
            interior,
            border_width,
        } => (
            MarkupSpec::Circle {
                rect: rect(r),
                border: *border,
                interior: *interior,
                border_width: w(*border_width),
            },
            true,
        ),
        MarkupSpec::Line {
            start,
            end,
            color,
            width,
            endings,
        } => (
            MarkupSpec::Line {
                start: pt(start),
                end: pt(end),
                color: *color,
                width: w(*width),
                endings: *endings,
            },
            false,
        ),
        MarkupSpec::Ink {
            strokes,
            color,
            width,
        } => (
            MarkupSpec::Ink {
                strokes: strokes.iter().map(|s| pts(s)).collect(),
                color: *color,
                width: w(*width),
            },
            false,
        ),
        MarkupSpec::Polygon {
            vertices,
            border,
            interior,
            width,
        } => (
            MarkupSpec::Polygon {
                vertices: pts(vertices),
                border: *border,
                interior: *interior,
                width: w(*width),
            },
            false,
        ),
        MarkupSpec::Cloud {
            vertices,
            border,
            interior,
            width,
            intensity,
        } => (
            MarkupSpec::Cloud {
                vertices: pts(vertices),
                border: *border,
                interior: *interior,
                width: w(*width),
                intensity: *intensity,
            },
            false,
        ),
        MarkupSpec::PolyLine {
            vertices,
            color,
            width,
        } => (
            MarkupSpec::PolyLine {
                vertices: pts(vertices),
                color: *color,
                width: w(*width),
            },
            false,
        ),
        MarkupSpec::TextMarkup { kind, quads, color } => (
            MarkupSpec::TextMarkup {
                kind: *kind,
                quads: quads
                    .iter()
                    .map(|q| Quad {
                        ul: pt(&q.ul),
                        ur: pt(&q.ur),
                        ll: pt(&q.ll),
                        lr: pt(&q.lr),
                    })
                    .collect(),
                color: *color,
            },
            false,
        ),
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_cmp
)]
mod tests {
    use super::*;

    fn rect(llx: f64, lly: f64, urx: f64, ury: f64) -> Rect {
        Rect { llx, lly, urx, ury }
    }

    fn content_str(a: &AuthoredAppearance) -> String {
        String::from_utf8(a.ap_content.clone()).unwrap()
    }

    fn bbox(a: &AuthoredAppearance) -> Vec<f64> {
        let Object::Array(arr) = a.ap_dict.get(b"BBox").unwrap() else {
            panic!("no BBox");
        };
        arr.iter().map(|o| o.as_number().unwrap()).collect()
    }

    #[test]
    fn square_is_filled_and_stroked_with_positive_bbox() {
        let a = build_appearance(&MarkupSpec::Square {
            rect: rect(10.0, 10.0, 110.0, 60.0),
            border: Some(Color::Rgb(0.0, 0.0, 1.0)),
            interior: Some(Color::Rgb(1.0, 1.0, 0.0)),
            border_width: 2.0,
            border_effect: None,
        });
        // Cosmetic keys present for third-party readers (R44 authoring).
        assert!(a.annot.contains_key(b"C") && a.annot.contains_key(b"IC"));
        assert_eq!(
            a.annot
                .get(b"Subtype")
                .unwrap()
                .as_name()
                .unwrap()
                .as_bytes(),
            b"Square"
        );
        // BBox == Rect (W-B 1:1 placement).
        assert_eq!(bbox(&a), vec![10.0, 10.0, 110.0, 60.0]);
        // Both colours set before the path, then B (fill + stroke).
        let c = content_str(&a);
        assert!(c.contains("1 1 0 rg"));
        assert!(c.contains("0 0 1 RG"));
        assert!(c.trim_end().ends_with('B'));
        // Inset by half the 2-unit border: re at 11 11 with 98×48.
        assert!(c.contains("11 11 98 48 re"), "{c}");
    }

    #[test]
    fn circle_emits_four_beziers() {
        let a = build_appearance(&MarkupSpec::Circle {
            rect: rect(0.0, 0.0, 100.0, 100.0),
            border: Some(Color::Gray(0.0)),
            interior: None,
            border_width: 1.0,
        });
        let c = content_str(&a);
        assert_eq!(c.matches(" c\n").count(), 4, "ellipse is four curves: {c}");
        assert!(c.trim_end().ends_with('S'), "stroke only: {c}");
    }

    #[test]
    fn line_has_open_arrowheads_and_l_key() {
        let a = build_appearance(&MarkupSpec::Line {
            start: (10.0, 10.0),
            end: (90.0, 10.0),
            color: Color::Rgb(1.0, 0.0, 0.0),
            width: 2.0,
            endings: (LineEnding::OpenArrow, LineEnding::OpenArrow),
        });
        assert_eq!(
            a.annot.get(b"L").unwrap().as_array().unwrap().len(),
            4,
            "/L is the two endpoints"
        );
        // Two open arrowheads ⇒ the shaft stroke + two more strokes.
        let c = content_str(&a);
        assert_eq!(c.matches("S\n").count(), 3, "{c}");
        // Rect must contain arrowheads (margin beyond the 10..90 span).
        assert!(a.rect.llx < 10.0 && a.rect.urx > 90.0);
    }

    #[test]
    fn ink_writes_inklist_and_one_stroke_per_gesture() {
        let a = build_appearance(&MarkupSpec::Ink {
            strokes: vec![
                vec![(0.0, 0.0), (10.0, 10.0), (20.0, 0.0)],
                vec![(30.0, 30.0), (40.0, 40.0)],
            ],
            color: Color::Rgb(0.0, 0.0, 0.0),
            width: 1.5,
        });
        let list = a.annot.get(b"InkList").unwrap().as_array().unwrap();
        assert_eq!(list.len(), 2, "two strokes");
        assert_eq!(list[0].as_array().unwrap().len(), 6, "3 points = 6 numbers");
        let c = content_str(&a);
        assert_eq!(c.matches("S\n").count(), 2, "one stroke per gesture: {c}");
    }

    #[test]
    fn polygon_closes_and_polyline_does_not() {
        let poly = build_appearance(&MarkupSpec::Polygon {
            vertices: vec![(0.0, 0.0), (50.0, 0.0), (25.0, 40.0)],
            border: Some(Color::Gray(0.0)),
            interior: Some(Color::Rgb(0.5, 0.5, 0.5)),
            width: 1.0,
        });
        assert!(content_str(&poly).contains("h\n"), "polygon closes");
        assert!(content_str(&poly).trim_end().ends_with('B'), "fill+stroke");

        let line = build_appearance(&MarkupSpec::PolyLine {
            vertices: vec![(0.0, 0.0), (50.0, 0.0), (25.0, 40.0)],
            color: Color::Gray(0.0),
            width: 1.0,
        });
        assert!(!content_str(&line).contains("h\n"), "polyline stays open");
        assert!(content_str(&line).trim_end().ends_with('S'), "stroke only");
    }

    #[test]
    fn highlight_uses_multiply_blend_and_fills_quads() {
        let q = Quad::from_rect(rect(10.0, 10.0, 110.0, 30.0));
        let a = build_appearance(&MarkupSpec::TextMarkup {
            kind: TextMarkupKind::Highlight,
            quads: vec![q],
            color: Color::Rgb(1.0, 1.0, 0.0),
        });
        // Parity: /BM /Multiply on the annot AND a /GS0 gs in the AP.
        assert_eq!(
            a.annot.get(b"BM").unwrap().as_name().unwrap().as_bytes(),
            b"Multiply"
        );
        let c = content_str(&a);
        assert!(c.contains("/GS0 gs"), "{c}");
        assert!(c.contains("1 1 0 rg"), "yellow fill: {c}");
        assert!(c.trim_end().ends_with('f'), "fill: {c}");
        // The AP Resources carry the ExtGState the gs selects (X8).
        let res = a.ap_dict.get(b"Resources").unwrap().as_dict().unwrap();
        assert!(res.get(b"ExtGState").is_some());
        // QuadPoints authored in Z-order: UL, UR, LL, LR.
        let qp: Vec<f64> = a
            .annot
            .get(b"QuadPoints")
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .map(|o| o.as_number().unwrap())
            .collect();
        assert_eq!(qp, vec![10.0, 30.0, 110.0, 30.0, 10.0, 10.0, 110.0, 10.0]);
    }

    #[test]
    fn underline_strikeout_squiggly_stroke_and_have_no_extgstate() {
        for kind in [
            TextMarkupKind::Underline,
            TextMarkupKind::StrikeOut,
            TextMarkupKind::Squiggly,
        ] {
            let a = build_appearance(&MarkupSpec::TextMarkup {
                kind,
                quads: vec![Quad::from_rect(rect(0.0, 0.0, 100.0, 12.0))],
                color: Color::Gray(0.0),
            });
            let c = content_str(&a);
            assert!(c.trim_end().ends_with('S'), "{kind:?}: {c}");
            // No blend mode / ExtGState for line marks.
            assert!(a.annot.get(b"BM").is_none(), "{kind:?}");
            let res = a.ap_dict.get(b"Resources").unwrap().as_dict().unwrap();
            assert!(res.is_empty(), "{kind:?} needs no resources");
        }
    }

    #[test]
    fn degenerate_rect_is_forced_positive_wf4() {
        // A zero-height Square rect would make §12.5.5 placement singular.
        let a = build_appearance(&MarkupSpec::Square {
            rect: rect(10.0, 50.0, 110.0, 50.0),
            border: Some(Color::Gray(0.0)),
            interior: None,
            border_width: 1.0,
            border_effect: None,
        });
        let bb = bbox(&a);
        assert!(bb[3] - bb[1] >= MIN_EXTENT, "positive height: {bb:?}");
    }

    #[test]
    fn inverted_rect_is_normalized() {
        let a = build_appearance(&MarkupSpec::Square {
            rect: rect(110.0, 60.0, 10.0, 10.0), // corners reversed
            border: Some(Color::Gray(0.0)),
            interior: None,
            border_width: 1.0,
            border_effect: None,
        });
        assert_eq!(bbox(&a), vec![10.0, 10.0, 110.0, 60.0]);
    }

    #[test]
    fn every_appearance_content_reparses_as_a_content_stream() {
        // The generator must never emit a stream the tokenizer rejects.
        let specs = vec![
            MarkupSpec::Square {
                rect: rect(0.0, 0.0, 50.0, 50.0),
                border: Some(Color::Rgb(1.0, 0.0, 0.0)),
                interior: Some(Color::Cmyk(0.0, 0.0, 0.0, 0.2)),
                border_width: 3.0,
                border_effect: None,
            },
            MarkupSpec::Circle {
                rect: rect(0.0, 0.0, 40.0, 20.0),
                border: Some(Color::Gray(0.2)),
                interior: None,
                border_width: 1.0,
            },
            MarkupSpec::Line {
                start: (0.0, 0.0),
                end: (30.0, 40.0),
                color: Color::Rgb(0.0, 0.0, 1.0),
                width: 1.0,
                endings: (LineEnding::ClosedArrow, LineEnding::None),
            },
            MarkupSpec::Ink {
                strokes: vec![vec![(0.0, 0.0), (5.5, 9.25), (12.0, 3.0)]],
                color: Color::Gray(0.0),
                width: 2.0,
            },
            MarkupSpec::TextMarkup {
                kind: TextMarkupKind::Squiggly,
                quads: vec![Quad::from_rect(rect(0.0, 0.0, 80.0, 10.0))],
                color: Color::Rgb(0.9, 0.1, 0.1),
            },
        ];
        for spec in &specs {
            let a = build_appearance(spec);
            crate::content::ContentStream::parse(a.ap_content.clone())
                .unwrap_or_else(|e| panic!("appearance did not reparse: {e} for {spec:?}"));
        }
    }

    // -- text-bearing subtypes (Pass 6.2) ------------------------------

    #[test]
    fn freetext_ap_bbox_is_origin_zero_sized_to_rect() {
        let a = build_text_annotation(&TextAnnotSpec::FreeText {
            rect: rect(50.0, 100.0, 250.0, 140.0),
            text: "hi".to_owned(),
            font: Std14::Helvetica,
            font_size: 12.0,
            color: TextColor::Gray(0.0),
            quadding: Quadding::Left,
            multiline: false,
            border: None,
            border_width: 0.0,
        })
        .unwrap();
        // §12.7.3.3: BBox = [0 0 Rect_w Rect_h], NOT the page-space rect.
        let Object::Array(arr) = a.ap_dict.get(b"BBox").unwrap() else {
            panic!("no BBox");
        };
        let nums: Vec<f64> = arr.iter().map(|o| o.as_number().unwrap()).collect();
        assert_eq!(nums, vec![0.0, 0.0, 200.0, 40.0]);
        // The annot /Rect is the page-space rectangle.
        assert_eq!(a.rect, rect(50.0, 100.0, 250.0, 140.0));
        assert_eq!(a.flags, AnnotFlags::PRINT);
        // A /DA and /Contents were baked.
        assert!(a.annot.get(b"DA").is_some());
        assert!(a.annot.get(b"Contents").is_some());
    }

    #[test]
    fn stamp_default_label_spaces_the_camel_case_name() {
        assert_eq!(
            StampName::NotForPublicRelease.default_label(),
            "NOT FOR PUBLIC RELEASE"
        );
        assert_eq!(StampName::Draft.default_label(), "DRAFT");
        let a = build_text_annotation(&TextAnnotSpec::Stamp {
            rect: rect(0.0, 0.0, 160.0, 50.0),
            name: StampName::Draft,
            label: None,
            color: Color::Rgb(0.8, 0.1, 0.1),
        })
        .unwrap();
        assert_eq!(
            a.annot.get(b"Name").unwrap().as_name().unwrap().as_bytes(),
            b"Draft"
        );
        // The frame stroke + the DRAFT label both appear in the content.
        let c = String::from_utf8(a.ap_content.clone()).unwrap();
        assert!(
            c.contains(" re\n") && c.contains("S\n"),
            "frame stroked: {c}"
        );
        assert!(c.contains("(DRAFT) Tj"), "label shown: {c}");
    }

    #[test]
    fn sticky_note_authors_a_popup_and_own_marker() {
        let a = build_text_annotation(&TextAnnotSpec::Sticky {
            rect: rect(80.0, 80.0, 100.0, 100.0),
            icon: StickyIcon::Comment,
            contents: "body".to_owned(),
            color: Color::Rgb(1.0, 0.9, 0.2),
            open: true,
        })
        .unwrap();
        assert_eq!(a.flags, STICKY_FLAGS);
        assert_eq!(
            a.annot.get(b"Name").unwrap().as_name().unwrap().as_bytes(),
            b"Comment"
        );
        let popup = a.popup.expect("a popup companion");
        assert_eq!(
            popup.get(b"Subtype").unwrap().as_name().unwrap().as_bytes(),
            b"Popup"
        );
        // The marker draws something (a filled note glyph), no font resources.
        assert!(!a.ap_content.is_empty());
        let res = a.ap_dict.get(b"Resources").unwrap().as_dict().unwrap();
        assert!(res.get(b"Font").is_none(), "the marker needs no font");
    }

    #[test]
    fn text_appearances_all_reparse_as_content_streams() {
        let specs = [
            TextAnnotSpec::FreeText {
                rect: rect(0.0, 0.0, 120.0, 60.0),
                text: "wrap me across lines please".to_owned(),
                font: Std14::TimesRoman,
                font_size: 0.0,
                color: TextColor::Rgb(0.1, 0.2, 0.9),
                quadding: Quadding::Right,
                multiline: true,
                border: Some(Color::Gray(0.0)),
                border_width: 1.5,
            },
            TextAnnotSpec::Stamp {
                rect: rect(0.0, 0.0, 200.0, 60.0),
                name: StampName::Confidential,
                label: None,
                color: Color::Rgb(0.8, 0.0, 0.0),
            },
            TextAnnotSpec::Sticky {
                rect: rect(0.0, 0.0, 20.0, 20.0),
                icon: StickyIcon::Note,
                contents: "x".to_owned(),
                color: Color::Rgb(1.0, 0.9, 0.2),
                open: false,
            },
        ];
        for spec in &specs {
            let a = build_text_annotation(spec).unwrap();
            crate::content::ContentStream::parse(a.ap_content.clone())
                .unwrap_or_else(|e| panic!("text appearance did not reparse: {e} for {spec:?}"));
        }
    }
}
