//! # Dimension annotation + `/AP` authoring (decision 011 §2.3, ISO 32000-1 §12.5.6.7 / §12.9)
//!
//! Turn a stored [`DimensionKind`] + its group's scale/format into a real,
//! portable `/Line` annotation with `/IT /LineDimension` and a **fully-baked
//! `/AP`** (leader + extension ticks + arrowheads + value label). This is the
//! **additive** authoring half (decision 011 §5.8 overlay-append, R46
//! zero-exception): it emits new indirect objects only; no page content-stream
//! byte is touched. [`crate::edit`] allocates object numbers and wires
//! `/AP`/`/P`/`/OC` in (mirroring `add_markup`).
//!
//! ## Why a baked `/AP`, always (R44)
//!
//! Like [`crate::annot_author`], every dimension carries a complete `/AP` so
//! the drawn dimension + value render in **any** reader (pdfium included, the
//! R59 gate), never relying on a consumer to synthesise appearance from the
//! `/Line`/`/IT`/`/Measure` keys. The value text is laid out in Base-14
//! Helvetica (§9.6.2.1 program-free dict, shared with [`crate::vartext`]).
//!
//! ## The `/Measure` mirror (interop, §12.9)
//!
//! When the group has a scale set, the group's portable `/Measure` dict
//! ([`super::measure_dict::build_measure_dict`]) is attached to the annotation
//! (per-annotation scale, PDF 1.7 — the co-equal alternative to a page
//! `/Viewport`, and the one that side-steps the geometric-partition problem for
//! overlapping different-scale groups). This is the reader-visible scale that
//! survives even if a foreign editor drops the `/PieceInfo` sidecar.
//!
//! ## Deterministic regeneration (the scale-change story)
//!
//! [`author_dimension`] is a **pure function of** `(kind, style)`, so
//! changing a group's scale re-runs it for every member and replaces each
//! member's `/AP` stream object + `/Contents` + `/Measure` (the Pass 7.1
//! regenerate-appearances pattern, decision 011 §2.3).

use crate::fontdata::Std14;
use crate::object::{Dict, Name, Object};
use crate::page_tree::Rect;
use crate::vartext::standard14_font_dict;
use crate::vector::Point;
use crate::writer::content::{ContentBuilder, LineCap, LineJoin, Paint};

use super::group::{DimStandard, DimensionKind};
use super::measure_dict::build_measure_dict;
use super::style::{ArrowForm, StyleDefaults, StyleOverrides};
use super::tolerance::Tolerance;
use super::units::{NumberFormat, ScaleState};
use crate::vector::Rgb;

/// The resource name the dimension label's font is authored under (matches the
/// `/AP` `/Resources` `/Font` key and the `Tf` operator).
const FONT_RESOURCE: &[u8] = b"Helv";

/// Padding either side of the value where it interrupts an ANSI dimension
/// line, in points. **Convention, not mandated** — see
/// [`DimensionStyle::extension_metrics`].
const TEXT_BREAK_PAD: f64 = 3.0;

/// How far an ISO value sits above its (unbroken) dimension line, in points.
/// **Convention, not mandated.**
const TEXT_ABOVE_GAP: f64 = 3.0;

/// The token an operator's text override may contain to have the MEASURED
/// caption substituted in (`Pass 175.0`, decision 097) — see
/// [`author_dimension_with_label`] for the contract.
///
/// Uppercase and angle-bracketed to match the convention CAD drafting tools
/// already use for the same idea, so an operator arriving from one does not
/// have to learn a second spelling. Every character of it is in
/// `WinAnsiEncoding`, which matters because the override is baked through
/// [`crate::vartext::encode_winansi`]: a placeholder that could not itself be
/// drawn would turn into `?????` in the one case where the substitution
/// failed to happen.
pub const DIM_PLACEHOLDER: &str = "<DIM>";

/// Everything the appearance of one ce dimension depends on besides its
/// geometry (Pass 27.2).
///
/// A struct rather than three positional parameters: `author_dimension` had
/// reached four arguments, three of them same-shaped, which is exactly the
/// call site the Rust API guidelines warn about
/// (`author_dimension(&k, s, f, std)` — good luck spotting a swap). The purity
/// contract is strengthened, not weakened: the function is now a pure function
/// of `(kind, style)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DimensionStyle {
    /// The group's scale.
    pub scale: ScaleState,
    /// The group's number format, including its decimal marker.
    pub format: NumberFormat,
    /// The drafting standard the dimension is drawn to.
    pub standard: DimStandard,
    /// Label point size (Pass 69.0; was the `LABEL_SIZE` constant).
    pub text_height: f64,
    /// Leader/extension/dimension-line stroke width in points (Pass 69.0; was
    /// the `LINE_WIDTH` constant).
    pub line_width: f64,
    /// Arrowhead length in points (Pass 69.0; was the `ARROW_LEN` constant).
    pub arrow_length: f64,
    /// Terminator form (Pass 69.0; the baker previously always filled a
    /// triangle).
    pub arrow_form: ArrowForm,
    /// Line, text and terminator colour (Pass 69.0; previously an
    /// unconditional black, written both into the content stream and the
    /// annotation's `/C`).
    pub color: Rgb,
    /// The tolerance drawn beside (or, for a limit tolerance, instead of) the
    /// nominal value (Pass 69.1).
    pub tolerance: Tolerance,
    /// The tolerance's own decimal precision; `None` ⇒ the nominal's
    /// (Pass 69.1).
    pub tolerance_places: Option<u32>,
}

impl DimensionStyle {
    /// A style with the factory appearance defaults and the caller's
    /// measurement fields — the constructor every test and doc example uses,
    /// so adding a sixth appearance property does not touch a dozen struct
    /// literals.
    ///
    /// **Not the path production code should take.** A real ce dimension's
    /// style comes from [`super::style::resolve_style`], which walks the
    /// factory → group → ce-dimension cascade. This constructor exists for
    /// call sites that genuinely have no group (doc examples, unit tests of
    /// the baker itself).
    #[must_use]
    pub const fn new(scale: ScaleState, format: NumberFormat, standard: DimStandard) -> Self {
        let f = StyleDefaults::FACTORY;
        Self {
            scale,
            format,
            standard,
            text_height: f.text_height,
            line_width: f.line_width,
            arrow_length: f.arrow_length,
            arrow_form: f.arrow_form,
            color: f.color,
            tolerance: f.tolerance,
            tolerance_places: f.tolerance_places,
        }
    }
}

impl From<&super::group::Group> for DimensionStyle {
    /// The group's style with no per-ce-dimension overrides — i.e. the two
    /// upper tiers of the cascade only.
    ///
    /// Kept as a `From` because "the group is the authority" is still true for
    /// every property a ce dimension does not override, and several call sites
    /// legitimately have a group and nothing else (a group-wide preview, a
    /// regeneration of a record that has been looked up separately). Call
    /// sites that DO have the record must use
    /// [`super::style::resolve_style`] — a `From<&Group>` there would silently
    /// discard the operator's per-ce-dimension overrides, and the file would
    /// disagree with the panel.
    fn from(g: &super::group::Group) -> Self {
        super::style::resolve_style(g, &StyleOverrides::default())
    }
}

impl DimensionStyle {
    /// The extension-line gap and overshoot, in points, for this standard.
    ///
    /// # The one structural difference between the two traditions
    ///
    /// ANSI expresses these as ABSOLUTE lengths (~1.5 mm gap, ~1 mm overshoot
    /// in mechanical practice); ISO expresses them as MULTIPLES OF THE LINE
    /// WIDTH (ISO 129-1 gives "approximately 8 x the line width"). Modelling
    /// that split is what lets one set of geometry reproduce both, rather than
    /// two drawing paths that drift.
    ///
    /// **Every number here is drafting CONVENTION, not a standard's
    /// requirement.** ISO 129-1 requires a gap without fixing its value; the
    /// ANSI figures are practice, and ASME Y14.2 is paywalled and was not
    /// obtained. Decision 026 records the sourcing and confidence for each.
    ///
    /// Since Pass 69.0 the ISO branch reads the RESOLVED [`Self::line_width`]
    /// rather than a module constant, so a group (or one ce dimension) that
    /// thickens its lines gets the proportionally larger gap ISO's convention
    /// actually asks for. The ANSI branch is unchanged — its figures are
    /// absolute lengths by construction, which is the whole reason the two
    /// traditions are modelled separately.
    #[must_use]
    pub fn extension_metrics(self) -> (f64, f64) {
        match self.standard {
            // ~1.4 mm and ~1 mm at 72 dpi.
            DimStandard::Ansi => (4.0, 3.0),
            // 8x the stroke width, per ISO's line-width-relative convention.
            DimStandard::Iso => (self.line_width * 8.0, self.line_width * 8.0),
        }
    }

    /// Whether the dimension line is BROKEN to make room for the value.
    ///
    /// ANSI interrupts the line and centres the value in the gap; ISO places
    /// the value **above an unbroken line** (ISO 129-1:2018 cl. 4.1.1, *"shall
    /// be indicated above the dimension line and read from the bottom"* —
    /// verified).
    #[must_use]
    pub const fn breaks_line_for_text(self) -> bool {
        matches!(self.standard, DimStandard::Ansi)
    }
}

/// The annotation-dictionary keys [`author_dimension`] OWNS — the ones a
/// regeneration must overwrite, and must REMOVE when the new state does not
/// produce them.
///
/// Declared here, next to the code that writes them, because "which keys does
/// authoring own" has exactly one correct answer and it belongs where the
/// authoring happens. A regenerator keeping its own list would drift the first
/// time this function learns a new key — and the failure would be silent: a
/// stale `/Measure` left behind by an uncalibrated regeneration claims a scale
/// that no longer applies, and every conforming reader believes it.
///
/// `/AP` is deliberately NOT here. The appearance stream's object id is
/// allocated once when the dimension is wired into a document and reused
/// across regenerations, so the reference must survive; only the stream's
/// CONTENT is rewritten.
///
/// `/C` is deliberately NOT here either, though this function does write it on
/// first authoring. It is a default colour, not something derived from the
/// geometry, scale or format — so nothing about a regeneration makes an
/// existing `/C` stale. Owning it would mean the first recolouring feature
/// silently loses its work the next time anything is regenerated, which is a
/// bug that would be very hard to attribute.
/// ★ `/L` and `/Vertices` are BOTH here, and only one of them is ever written
/// for a given ce dimension (`Pass 107.0`). That is the point: the
/// overwrite-or-remove rule then guarantees that authoring a perimeter REMOVES
/// a stale `/L`, and authoring a linear ce dimension removes a stale
/// `/Vertices`. A dictionary carrying both would give a reader that honours
/// `/L` and a reader that honours `/Vertices` two different pictures of the
/// same annotation.
pub const AUTHORED_ANNOT_KEYS: [&[u8]; 7] = [
    b"Type",
    b"Subtype",
    b"IT",
    b"Rect",
    b"L",
    b"Vertices",
    b"Contents",
];

/// The key authoring writes only when the group is calibrated (§12.9 Table
/// 261) — separated from [`AUTHORED_ANNOT_KEYS`] only for documentation; it is
/// handled by the same overwrite-or-remove rule.
pub const AUTHORED_MEASURE_KEY: &[u8] = b"Measure";

/// The result of authoring one dimension — the pieces [`crate::edit`] wires
/// into a document. Mirrors [`crate::annot_author::AuthoredAppearance`] so the
/// edit-session wiring is identical (allocate `/AP` + annot numbers, stage
/// content, patch `/Annots`).
#[derive(Debug, Clone, PartialEq)]
pub struct AuthoredDimension {
    /// The annotation dict. For a linear, circular or angular ce dimension:
    /// `/Type /Annot /Subtype /Line /IT /LineDimension /Rect /L /C /Contents`
    /// and (when scaled) `/Measure`. For a PERIMETER (`Pass 107.0`), and per
    /// ISO 32000-1 §12.5.6.9 Table 178: `/Subtype /Polygon` with
    /// `/IT /PolygonDimension` when the shape closes, `/Subtype /PolyLine`
    /// with `/IT /PolyLineDimension` when it does not, carrying `/Vertices`
    /// in place of `/L`.
    ///
    /// **Missing `/AP`, `/P`, `/OC`** — the session adds them.
    pub annot: Dict,
    /// The `/AP` `/N` form-XObject dict (`/BBox` = `/Rect`, identity matrix,
    /// `/Resources` carrying the Helvetica label font). `/Length` added by the
    /// serializer.
    pub ap_dict: Dict,
    /// The appearance content-stream bytes (raw, unfiltered).
    pub ap_content: Vec<u8>,
    /// The computed `/Rect`, guaranteed positive-area.
    pub rect: Rect,
    /// The display label (for CLI/GUI echo; also stored as `/Contents`).
    pub label: String,
    /// **Whether [`Self::label`] came from an operator override** rather than
    /// from the measurement (`Pass 175.0`, decision 097).
    ///
    /// Carried out of the baker rather than recomputed by callers, because the
    /// baker is the only place that knows whether the override it was handed
    /// actually reached the caption. A caller comparing `label` against a
    /// separately formatted measurement would get the answer wrong for a
    /// `<DIM>`-only override, which prints exactly the measured caption and is
    /// still an override.
    ///
    /// Exists so a shell can DISCLOSE the divergence without re-deriving it —
    /// rule 4's obligation is on disclosure, and a fact that has to be
    /// recomputed to be disclosed is a fact that will eventually be disclosed
    /// wrongly.
    pub label_overridden: bool,
}

/// Author a dimension's `/Line` annotation + baked `/AP` from its geometry and
/// its group's scale + format. Pure and deterministic (regeneration-safe).
///
/// # Examples
///
/// ```
/// use pdfcer_core::dimension::{
///     author_dimension, DimStandard, DimensionKind, DimensionStyle, NumberFormat, ScaleState, Unit,
/// };
/// use pdfcer_core::vector::{AxisConstraint, Point};
///
/// let kind = DimensionKind::Linear {
///     a: Point::new(100.0, 100.0),
///     b: Point::new(200.0, 100.0),
///     constraint: AxisConstraint::Horizontal,
///     offset: 0.0,
///     text_along: 0.0,
/// };
/// let authored = author_dimension(
///     &kind,
///     // `new` = the factory appearance defaults. A real ce dimension's style
///     // comes from `dimension::resolve_style`, which walks the
///     // factory -> group -> ce-dimension cascade.
///     DimensionStyle::new(
///         ScaleState::Calibrated { scale: 0.01 },
///         NumberFormat::decimal(Unit::Meter, 2),
///         DimStandard::Ansi,
///     ),
/// );
/// // 100 pt at 0.01 m/pt = 1.00 m.
/// assert_eq!(authored.label, "1.00 m");
/// assert_eq!(authored.annot.get(b"Subtype").unwrap().as_name().unwrap().as_bytes(), b"Line");
/// assert_eq!(authored.annot.get(b"IT").unwrap().as_name().unwrap().as_bytes(), b"LineDimension");
/// assert!(authored.annot.get(b"Measure").is_some()); // scale mirror
/// ```
#[must_use]
pub fn author_dimension(kind: &DimensionKind, style: DimensionStyle) -> AuthoredDimension {
    author_dimension_with_label(kind, style, None)
}

/// The `Pass 175.0` widening of [`author_dimension`]: author the same
/// annotation + baked `/AP`, with the caption optionally REPLACED by an
/// operator-supplied string (decision 097, branch 1).
///
/// [`author_dimension`] is this function with `label_override = None` and is
/// kept as the name every existing caller uses. **One implementation, not
/// two** — a second baker would be a second place for "which keys does
/// authoring own", for the WinAnsi encoding, and for the ANSI text-break
/// geometry to be answered, and `Pass 68.0` is this module's own record of
/// what two answers to one display question costs (the pane read `77.5°`
/// while the `/AP` baked into the file read `77.47 pt`).
///
/// # The contract of an override
///
/// - `None` ⇒ the caption is the measurement, exactly as before this Pass:
///   `kind.caption_prefix()` + the formatted value + `style.tolerance`'s
///   caption (or, for a limit tolerance, the two limits alone).
/// - `Some(text)` ⇒ **the caption is `text`, WHOLE.** The prefix and the
///   tolerance caption are not appended around it. That is a deliberate
///   choice and not an omission: an override is the operator saying what the
///   dimension reads, and a `R ` prefix or a `±0.5` suffix bolted onto their
///   sentence would produce a caption nobody typed. It also composes with a
///   LIMIT tolerance without a special case, which the alternative does not —
///   a limit tolerance suppresses the nominal entirely, so there is no
///   nominal for an override to sit beside.
/// - The measured geometry, the group's scale and the `/Measure` dict are
///   **untouched** in every case. An override changes what is DRAWN, never
///   what was measured (decision 097; `CLAUDE.md` rule 15's provenance
///   distinction survives an override intact).
///
/// # `<DIM>`, and why pdfcer ships a placeholder Acrobat's dimension tool
/// does not
///
/// Every occurrence of the literal ASCII token `<DIM>` in `text` is replaced
/// by the measured caption (prefix + value + tolerance). So:
///
/// - `"2X <DIM>"` on a 25 mm feature prints `2X 25.00 mm` and **keeps
///   tracking the geometry** — re-scale the group and the printed number
///   follows, because the substitution happens here, at bake time, on every
///   regeneration.
/// - `"55 5/8"` prints `55 5/8` and tracks nothing, which is the operator
///   explicitly saying so.
///
/// This is a parity-PLUS: it is modelled on the `<DIM>` placeholder CAD
/// drafting tools give dimension text, and it exists because it makes
/// decision 097's central guarantee — *the measurement is retained, only
/// shadowed* — visible in the output rather than merely true in the sidecar.
/// There is no escape for a literal `<DIM>`; a caption that needs to print
/// those five characters is not a case this Pass serves, and inventing an
/// escape syntax for it would be a notation the operator has to learn for a
/// case that has never come up.
///
/// # What is NOT validated here
///
/// This function is pure and total: it bakes whatever string it is handed.
/// Characters outside `WinAnsiEncoding` are substituted with `?` by
/// [`crate::vartext::encode_winansi`] the same way they always were.
/// **The refusal lives at the verb** — `EditSession::set_dimension_label`
/// rejects an unprintable override before it can reach the sidecar, because
/// that is where an operator is present to be told. Baking is downstream of
/// that check and must stay able to re-bake whatever is already stored,
/// including a sidecar written by hand.
#[must_use]
pub fn author_dimension_with_label(
    kind: &DimensionKind,
    style: DimensionStyle,
    label_override: Option<&str>,
) -> AuthoredDimension {
    let DimensionStyle { scale, format, .. } = style;
    let label_size = style.text_height;
    // Through `display_with`, never `format_measurement` directly: an ANGULAR
    // ce dimension must not be run through the length formatter, and this
    // baked label is the copy that outlives the session. See
    // `DimensionKind::display_with`.
    let display = kind.display_with(scale, format);
    // The label, nominal + tolerance, built in ONE place (Pass 69.1).
    //
    // `Pass 68.0` shipped a defect whose entire cause was two independent
    // derivations of a display value — the pane read `77.5°` while the `/AP`
    // baked into the document read `77.47 pt`. The tolerance caption is
    // therefore assembled by `Tolerance::caption`, here, and every other
    // surface reads what this produced rather than recomputing it.
    let tol_places = style
        .tolerance_places
        .unwrap_or_else(|| nominal_places(format));
    let measured_caption = if style.tolerance.suppresses_nominal() {
        // A limit tolerance PRINTS ITS TWO LIMITS AND NOT THE NOMINAL
        // (`SolidWorks_Dimensions` §A.1). The measured value is unchanged and
        // still in the sidecar — this is a display decision, not a loss.
        style.tolerance.caption(format, tol_places)
    } else {
        format!(
            "{}{}{}",
            kind.caption_prefix(),
            display.text,
            style.tolerance.caption(format, tol_places)
        )
    };
    // The override, applied LAST and to the finished measured caption, so
    // `<DIM>` substitutes the same string the un-overridden dimension would
    // have printed — including its prefix and its tolerance. Substituting
    // only `display.text` would make `<DIM>` mean "the number" on a linear
    // dimension and "the number without its R" on a circular one, which is
    // one token with two meanings.
    let label_overridden = label_override.is_some();
    let label = match label_override {
        Some(text) => text.replace(DIM_PLACEHOLDER, &measured_caption),
        None => measured_caption,
    };

    // The leader endpoints in page space (the /L pair).
    let (l0, l1) = leader_endpoints(kind);

    // Accumulate the drawn bbox as we build the appearance content.
    let mut bounds = BoundsAcc::new();
    let mut b = ContentBuilder::new();
    // ★ Grey stays grey, and that is a compatibility decision rather than a
    // stylistic one. Before Pass 69.0 this emitted `0 g` / `0 G`
    // unconditionally; every ce dimension in every existing document was baked
    // that way. Emitting `0 0 0 rg` for the same black would repaint identical
    // pixels while changing the stream bytes, so the first unrelated
    // regeneration (a scale edit on some other property) would rewrite every
    // appearance stream in the file for no visible reason — a minimal-diff
    // (R34) violation that is invisible until somebody diffs two saves.
    //
    // So: a pure grey is written with the grey operators it always used, and
    // DeviceRGB appears only once a colour actually needs it.
    let c = style.color;
    #[allow(clippy::float_cmp)] // exact equality is the intent: only an
    // untouched/explicitly-grey colour takes the legacy path.
    let grey = c.r == c.g && c.g == c.b;
    if grey {
        b.set_stroke_gray(f64::from(c.r));
        b.set_fill_gray(f64::from(c.r));
    } else {
        b.set_stroke_rgb(f64::from(c.r), f64::from(c.g), f64::from(c.b));
        b.set_fill_rgb(f64::from(c.r), f64::from(c.g), f64::from(c.b));
    }
    b.set_line_width(style.line_width);
    b.set_line_cap(LineCap::Butt);
    b.set_line_join(LineJoin::Miter);

    // The label's metrics are needed BEFORE the line is stroked, because under
    // ANSI the line is broken to make room for it. Computing them here rather
    // than after drawing is what lets one function serve both standards.
    let text_w = estimate_text_width(&label, label_size);
    let anchor = kind.label_anchor().unwrap_or_else(|| l0.midpoint(l1));

    match *kind {
        DimensionKind::Linear { .. } => {
            // The measured points, for the extension lines. `linear_geometry`
            // is the ONE definition of this frame — `leader_endpoints` above
            // reads the same function for the dimension line's ends, so the
            // two cannot disagree about where the dimension sits.
            let (ext_a, ext_b) = kind
                .linear_geometry()
                .map_or((l0, l1), |(_, _, pa, pb)| (pa, pb));
            // ANSI interrupts the dimension line and centres the value in the
            // gap; ISO runs the line unbroken with the value above it (ISO
            // 129-1:2018 cl. 4.1.1, verified). One geometry, one flag.
            let brk = style
                .breaks_line_for_text()
                .then(|| (anchor, text_w / 2.0 + TEXT_BREAK_PAD));
            draw_linear(
                &mut b,
                &mut bounds,
                LinearDraw {
                    dim: (l0, l1),
                    ext: (ext_a, ext_b),
                    style,
                    text_break: brk,
                },
            );
        }
        DimensionKind::Circular { fit, .. } => {
            draw_circular(&mut b, &mut bounds, fit.center, fit.radius, l1, style);
        }
        DimensionKind::Angular {
            apex,
            dir_a,
            dir_b,
            radius,
            ..
        } => {
            draw_angular(&mut b, &mut bounds, apex, dir_a, dir_b, radius, style);
        }
        DimensionKind::Perimeter {
            ref points, closed, ..
        } => {
            draw_perimeter(&mut b, &mut bounds, points, closed);
        }
    }

    // The value label. Anchored where the operator DROPPED it along the
    // dimension line (`label_anchor`), not unconditionally at the midpoint —
    // SolidWorks stores a dimension's placement as a point, and sliding the
    // number along its own line is half of what that point expresses. Falls
    // back to the midpoint for a circular dimension, which has no such axis.
    // Text placement and orientation, both standard-dependent.
    //
    // ANSI is UNIDIRECTIONAL: every value reads horizontally regardless of the
    // dimension's direction, and sits in the break in the line. ISO is
    // ALIGNED: the value runs parallel to its dimension line and sits above it
    // (cl. 4.1.1, *"shall be indicated above the dimension line and read from
    // the bottom"* — verified). For a horizontal dimension the two coincide,
    // which is why the difference only becomes visible on an aligned one.
    let (ux, uy) = match (style.standard, kind.axis_frame()) {
        // Aligned text, flipped where it would otherwise read upside down —
        // cl. 4.1.1's "read from the bottom", and "from the right" for a
        // vertical one.
        //
        // The tie-break on `u.y` is load-bearing, not defensive: an ALIGNED
        // dimension pointing straight down has `u = (0, -1)`, where `u.x` is
        // exactly zero. Testing `u.x < 0` alone let that case through and the
        // value read top-to-bottom — the one orientation ISO names. Found by
        // exercising all four cardinal directions rather than the two that
        // happened to be convenient.
        (DimStandard::Iso, Some((u, _))) if u.x < 0.0 || (u.x == 0.0 && u.y < 0.0) => (-u.x, -u.y),
        (DimStandard::Iso, Some((u, _))) => (u.x, u.y),
        _ => (1.0, 0.0),
    };
    // Perpendicular to the text direction, for the ISO standoff.
    let (px, py) = (-uy, ux);
    let lift = if matches!(kind, DimensionKind::Perimeter { .. }) {
        // A perimeter's anchor is a FREE POINT the operator dropped (the
        // vertex centroid, displaced by the placement pair), not a line the
        // number has to clear. So it is centred ON that point under BOTH
        // standards. ISO 129-1 cl. 4.1.1's "above the dimension line" has no
        // dimension line to be above here, and applying it anyway would lift
        // the label off the point it was dropped at — the drag would feel
        // like it missed by a few points, every time, for no visible reason.
        -label_size * 0.35
    } else if style.breaks_line_for_text() {
        // Centred ON the line: drop the baseline by roughly half a cap height
        // so the glyphs straddle it rather than sit on it.
        -label_size * 0.35
    } else {
        TEXT_ABOVE_GAP
    };
    let tx = anchor.x - ux * text_w / 2.0 + px * lift;
    let ty = anchor.y - uy * text_w / 2.0 + py * lift;
    // Bounds from the rotated box's four corners, not an axis-aligned guess —
    // an under-sized /Rect clips the value in every conforming reader.
    for (du, dv) in [
        (0.0, 0.0),
        (text_w, 0.0),
        (0.0, label_size),
        (text_w, label_size),
    ] {
        bounds.add(Point::new(tx + ux * du + px * dv, ty + uy * du + py * dv));
        bounds.add(Point::new(
            tx + ux * du - px * label_size * 0.3,
            ty + uy * du - py * label_size * 0.3,
        ));
    }
    // The BASIC box (Pass 69.1): a theoretically-exact value is drawn inside a
    // rectangle, and the box IS the notation - `Tolerance::Basic` prints no
    // text of its own. Drawn BEFORE the text so a future filled box cannot
    // paint over the glyphs, and sized off the same `text_w`/`label_size` the
    // text uses so the two cannot disagree.
    if style.tolerance.is_boxed() {
        let pad = label_size * 0.25;
        let (bw, bh) = (text_w + pad * 2.0, label_size + pad * 2.0);
        // In the text's own frame, so a rotated (ISO-aligned) label gets a
        // rotated box rather than an axis-aligned one that no longer fits.
        let corner = |du: f64, dv: f64| Point::new(tx + ux * du + px * dv, ty + uy * du + py * dv);
        let quad = [
            corner(-pad, -pad - label_size * 0.25),
            corner(bw - pad, -pad - label_size * 0.25),
            corner(bw - pad, bh - pad - label_size * 0.25),
            corner(-pad, bh - pad - label_size * 0.25),
        ];
        b.move_to(quad[0].x, quad[0].y);
        for q in &quad[1..] {
            b.line_to(q.x, q.y);
        }
        b.close_subpath();
        b.paint(Paint::Stroke);
        for q in quad {
            bounds.add(q);
        }
    }

    b.begin_text();
    b.set_font(FONT_RESOURCE, label_size);
    b.set_text_matrix(ux, uy, -uy, ux, tx, ty);
    // ★ WinAnsi, not raw UTF-8. The label font is declared
    // `/WinAnsiEncoding` (`standard14_font_dict`), so a multi-byte UTF-8
    // character is read as one byte per byte: a degree sign (U+00B0, UTF-8
    // `C2 B0`) drew as `Â°`. Harmless for as long as every label was ASCII,
    // which it was until angular ce dimensions arrived.
    //
    // The substitution count is deliberately ignored HERE rather than
    // discarded silently: nothing a ce-dimension label can contain is outside
    // WinAnsi (digits, the unit words, the decimal marker, the foot and inch
    // marks, and the degree sign all have codes), so a miss would mean the
    // formatter had started emitting something new — which is a change that
    // should come with its own disclosure decision rather than inherit one
    // guessed at here.
    let (label_bytes, _unmapped) = crate::vartext::encode_winansi(&label);
    b.show_text(&label_bytes);
    b.end_text();

    let rect = bounds.into_rect();

    // The annotation dict. Its SUBTYPE, its `/IT` intent and its GEOMETRY KEY
    // are all kind-dependent (`Pass 107.0`); everything below them is shared.
    //
    // - linear/circular/angular -> `/Line` + `/IT /LineDimension` + `/L`
    //   (ISO 32000-1 §12.5.6.7 Table 175)
    // - perimeter, closed       -> `/Polygon` + `/IT /PolygonDimension`
    // - perimeter, open         -> `/PolyLine` + `/IT /PolyLineDimension`
    //   (both §12.5.6.9 Table 178, `/IT` values PDF 1.7)
    let mut annot = Dict::new();
    annot.insert(Name::from(b"Type"), Object::Name(Name::from(b"Annot")));
    annot.insert(Name::from(b"Rect"), rect_array(rect));
    if let DimensionKind::Perimeter {
        ref points, closed, ..
    } = *kind
    {
        // ★ `/Polygon` for a closed shape even though Acrobat's own AREA tool
        // uses that subtype for its measurements, and `/IT /PolygonDimension`
        // with it. The subtype is a STRUCTURAL fact — the shape closes — while
        // `/IT` is explicitly *"a hint of intent"* a reader may ignore
        // (§12.5.6.9, Table 170 cross-reference), and no intent name for
        // "the perimeter of a polygon" exists in Table 178's list. The
        // alternative — writing `/PolyLine` with the first vertex repeated at
        // the end — was refused twice over: it misrepresents a closed shape as
        // open to every reader, and it would put n+1 vertices in the file
        // against n in the sidecar, which is exactly the transposition-shaped
        // mistake the flat `/Points` layout was chosen to avoid.
        //
        // Nothing is at risk from a reader that reads `/PolygonDimension` as
        // "area": the number is baked into the `/AP` and mirrored in
        // `/Contents`, and `/IT` cannot cause a value to be recomputed.
        let (subtype, intent): (&[u8], &[u8]) = if closed {
            (b"Polygon", b"PolygonDimension")
        } else {
            (b"PolyLine", b"PolyLineDimension")
        };
        annot.insert(Name::from(b"Subtype"), Object::Name(Name(subtype.to_vec())));
        annot.insert(Name::from(b"IT"), Object::Name(Name(intent.to_vec())));
        // §12.5.6.9 Table 178: a FLAT array of alternating x and y in DEFAULT
        // USER SPACE — the same space as `/Rect` and as `/Line`'s `/L`, so no
        // transform is involved. An array of `[x y]` pairs would be wrong: the
        // nested form belongs to PDF 2.0's `/Path` and to `/InkList`.
        //
        // ★ For a `/Polygon` the closing segment is supplied BY THE READER and
        // the first vertex is NOT repeated (§12.5.6.9: a polyline differs from
        // a polygon "except that the first and last vertex are not implicitly
        // connected"). Repeating it is not forbidden but is undefined, and the
        // spec corpus names the opposite error as the real hazard: closing the
        // ring for a `/PolyLine`, or failing to close it for a `/Polygon`.
        // pdfcer's own measurement does the closing itself — see
        // `polyline_length` — and this array stays exactly the picked vertices.
        let mut flat = Vec::with_capacity(points.len() * 2);
        for p in points {
            flat.push(Object::Real(p.x));
            flat.push(Object::Real(p.y));
        }
        annot.insert(Name::from(b"Vertices"), Object::Array(flat));
        // `/LE`, `/IC` and `/BS` are all deliberately absent. `/LE` defaults to
        // `[/None /None]`, which is what a measurement wants — a terminator
        // would assert that the number spans from one end to the other. `/IC`
        // absent means no interior fill, which is what a perimeter is. `/BS`
        // is moot: Table 178 states that a present `/AP` "shall take precedence
        // over the `Vertices` and `BS` entries", and pdfcer always bakes one.
    } else {
        annot.insert(Name::from(b"Subtype"), Object::Name(Name::from(b"Line")));
        annot.insert(
            Name::from(b"IT"),
            Object::Name(Name::from(b"LineDimension")),
        );
        annot.insert(
            Name::from(b"L"),
            Object::Array(vec![
                Object::Real(l0.x),
                Object::Real(l0.y),
                Object::Real(l1.x),
                Object::Real(l1.y),
            ]),
        );
    }
    // `/C` — the annotation's own colour (§12.5.2 Table 164), kept in step
    // with what the baked `/AP` actually paints. A reader that ignores the
    // appearance stream and draws from the annotation keys alone (and some
    // do, for a `/Line`) would otherwise draw a black dimension over a
    // coloured one.
    annot.insert(
        Name::from(b"C"),
        Object::Array(vec![
            Object::Real(f64::from(style.color.r)),
            Object::Real(f64::from(style.color.g)),
            Object::Real(f64::from(style.color.b)),
        ]),
    );
    annot.insert(
        Name::from(b"Contents"),
        Object::String(label.as_bytes().to_vec()),
    );
    // The portable /Measure scale mirror (only when a scale is set).
    if let Some(s) = scale.effective_scale(format.unit) {
        annot.insert(Name::from(b"Measure"), build_measure_dict(s, format));
    }

    AuthoredDimension {
        annot,
        ap_dict: ap_form_dict(rect),
        ap_content: b.into_bytes(),
        rect,
        label,
        label_overridden,
    }
}

/// The nominal's own decimal precision, for a tolerance that follows it.
///
/// The reference expresses "same as nominal" as a −3 sentinel inside the digit
/// count (`swTolerancePrecisionFollowsNominal`, `SolidWorks_Dimensions` §B.2);
/// pdfcer expresses it as an absent value and resolves it here. A fractional
/// format has no decimal digits at all, and a tolerance beside a `5/8"` is
/// still written as a decimal in practice, so it falls back to two — stated
/// rather than silently assumed, because it IS an assumption.
fn nominal_places(format: NumberFormat) -> u32 {
    match format.fraction {
        super::units::FractionMode::Decimal { places } => places,
        super::units::FractionMode::Fraction { .. } => 2,
    }
}

/// The `/L` leader endpoints for a dimension: the two picked points for a
/// linear dimension, or centre→rim (`centre + (radius, 0)`) for a circular one.
fn leader_endpoints(kind: &DimensionKind) -> (Point, Point) {
    match *kind {
        // The DIMENSION LINE's ends, not the picked points (Pass 27.0). These
        // differ whenever the constraint is Horizontal/Vertical and the picks
        // are not already aligned, or whenever there is a standoff. Returning
        // the picks here is what drew a constrained dimension at an angle and
        // wrote a `/L` that disagreed with the drawn line.
        DimensionKind::Linear { a, b, .. } => kind
            .linear_geometry()
            .map_or((a, b), |(dim_a, dim_b, _, _)| (dim_a, dim_b)),
        DimensionKind::Circular { fit, .. } => (
            fit.center,
            Point::new(fit.center.x + fit.radius, fit.center.y),
        ),
        // The two points where the arc meets the arms. `/L` is a two-point
        // leader and an arc has no two-point form, so these are the closest
        // honest answer: the ends of the thing actually drawn. A reader that
        // understands only `/L` sees the chord of the angle rather than a
        // line unrelated to the dimension.
        DimensionKind::Angular {
            apex,
            dir_a,
            dir_b,
            radius,
            ..
        } => (
            Point::new(
                dir_a.x.mul_add(radius, apex.x),
                dir_a.y.mul_add(radius, apex.y),
            ),
            Point::new(
                dir_b.x.mul_add(radius, apex.x),
                dir_b.y.mul_add(radius, apex.y),
            ),
        ),
        // The path's two ENDS. A perimeter does not write `/L` at all — its
        // geometry key is `/Vertices` (ISO 32000-1 §12.5.6.9 Table 178) — so
        // this pair is only the fallback label anchor, and
        // `DimensionKind::label_anchor` answers for this kind so even that is
        // not reached. Returning the ends rather than, say, the first vertex
        // twice keeps the fallback midpoint somewhere on the shape.
        DimensionKind::Perimeter { ref points, .. } => {
            let first = points.first().copied().unwrap_or(Point::new(0.0, 0.0));
            let last = points.last().copied().unwrap_or(first);
            (first, last)
        }
    }
}

/// Draw a linear ce dimension: the dimension line, real extension (witness)
/// lines back to the measured points, and terminators (Pass 27.0).
///
/// # What changed, and why the old shape was wrong
///
/// This used to stroke a line straight between the two PICKED points and add
/// a 4 pt perpendicular tick at each end. That is only correct when the picks
/// already lie on the constraint axis and there is no standoff — i.e. almost
/// never. `ext_a`/`ext_b` are the measured points; `a`/`c` are the dimension
/// line's own ends, which the caller derives from
/// [`DimensionKind::linear_geometry`].
///
/// Extension lines are **omitted, not clamped**, when they would be shorter
/// than the gap they must leave — which is exactly the zero-standoff case,
/// where the dimension line already passes through the point and a witness
/// line would be a stub of nothing. The two extension lines may point in
/// OPPOSITE directions (picks straddling the dimension line), so each takes
/// its own direction from its own endpoints rather than from the sign of the
/// standoff.
struct LinearDraw {
    /// The dimension line's two ends.
    dim: (Point, Point),
    /// The two measured points the extension lines reach back to.
    ext: (Point, Point),
    /// The group's style, which decides the extension metrics.
    style: DimensionStyle,
    /// Where the value interrupts the line, as `(centre, half-width)` — `None`
    /// under a standard that does not break the line.
    text_break: Option<(Point, f64)>,
}

fn draw_linear(b: &mut ContentBuilder, bounds: &mut BoundsAcc, d: LinearDraw) {
    let LinearDraw {
        dim: (a, c),
        ext: (ext_a, ext_b),
        style,
        text_break,
    } = d;
    let (ext_gap, ext_overshoot) = style.extension_metrics();
    let (ux, uy) = unit_vector(a, c);

    // The dimension line — in two pieces when the value interrupts it.
    //
    // The break is expressed as a centre and a half-width along the line, so a
    // value sitting off-centre (a dragged `text_along`) breaks the line where
    // it actually is rather than in the middle. If the break would consume the
    // whole line, the line is omitted and the terminators still mark the
    // extent, which is what a drafter does in cramped space.
    match text_break {
        Some((centre, half)) => {
            let (ux, uy) = unit_vector(a, c);
            let t_centre = (centre.x - a.x) * ux + (centre.y - a.y) * uy;
            let total = ((c.x - a.x).powi(2) + (c.y - a.y).powi(2)).sqrt();
            let seg1 = (t_centre - half).min(total).max(0.0);
            let seg2 = (t_centre + half).min(total).max(0.0);
            if seg1 > 0.0 {
                b.move_to(a.x, a.y);
                b.line_to(a.x + ux * seg1, a.y + uy * seg1);
                b.paint(Paint::Stroke);
            }
            if seg2 < total {
                b.move_to(a.x + ux * seg2, a.y + uy * seg2);
                b.line_to(c.x, c.y);
                b.paint(Paint::Stroke);
            }
        }
        None => {
            b.move_to(a.x, a.y);
            b.line_to(c.x, c.y);
            b.paint(Paint::Stroke);
        }
    }
    bounds.add(a);
    bounds.add(c);

    // Extension lines: from just clear of the measured point, to just past the
    // dimension line. Both offsets are DRAFTING CONVENTION, not mandated by
    // any standard — ANSI practice is ~1.5 mm gap and ~1 mm overshoot in
    // mechanical work; ISO expresses both as multiples of the line width.
    // Decision 026 records the sourcing and the confidence for each; they are
    // constants here so a per-standard style can replace them without moving
    // the geometry.
    for (point, dim_end) in [(ext_a, a), (ext_b, c)] {
        let (dx, dy) = (dim_end.x - point.x, dim_end.y - point.y);
        let len = dx.hypot(dy);
        if !len.is_finite() || len <= ext_gap + ext_overshoot {
            // Too short to draw as a witness line: the dimension line is
            // already at (or through) the point.
            continue;
        }
        let (nx, ny) = (dx / len, dy / len);
        let start = Point::new(point.x + nx * ext_gap, point.y + ny * ext_gap);
        let end = Point::new(
            point.x + nx * (len + ext_overshoot),
            point.y + ny * (len + ext_overshoot),
        );
        b.move_to(start.x, start.y);
        b.line_to(end.x, end.y);
        b.paint(Paint::Stroke);
        bounds.add(start);
        bounds.add(end);
    }

    // Terminators pointing outward at each end (toward the extension ticks).
    arrowhead(b, bounds, a, (-ux, -uy), style);
    arrowhead(b, bounds, c, (ux, uy), style);
}

/// Draw a circular dimension: the fitted circle outline + a radius leader from
/// centre to rim with an arrowhead at the rim.
fn draw_circular(
    b: &mut ContentBuilder,
    bounds: &mut BoundsAcc,
    center: Point,
    radius: f64,
    rim: Point,
    style: DimensionStyle,
) {
    // The fitted circle outline (four kappa cubics), for context.
    if radius.is_finite() && radius > 0.0 {
        emit_circle(b, center, radius);
        bounds.add(Point::new(center.x - radius, center.y - radius));
        bounds.add(Point::new(center.x + radius, center.y + radius));
    }
    // The radius leader centre → rim.
    b.move_to(center.x, center.y);
    b.line_to(rim.x, rim.y);
    b.paint(Paint::Stroke);
    bounds.add(center);
    bounds.add(rim);
    // Terminator at the rim pointing outward.
    let (ux, uy) = unit_vector(center, rim);
    arrowhead(b, bounds, rim, (ux, uy), style);
}

/// Draw an ANGULAR ce dimension: two extension lines out along the arms, an
/// arc between them at `radius`, and an arrowhead at each end of the arc
/// pointing along the arc (`Pass 68.0`).
///
/// # Why the arc is emitted as line segments rather than Bézier cubics
///
/// `draw_circular` uses the four-kappa-cubic construction because it always
/// draws a FULL circle, where the four quadrant arcs are exact and the error
/// is a known constant. An angular dimension draws an arbitrary sweep, and a
/// single cubic's error grows with the swept angle — a 170-degree sweep drawn
/// as one cubic visibly bulges. Segmenting at a fixed angular step keeps the
/// error bounded by the step regardless of sweep, and a dimension arc is a
/// thin stroked line where a fraction of a point of chord error is invisible.
///
/// The step is 3 degrees: at a 200-point radius that is a chord sagitta of
/// about 0.07 pt, well under a stroke width, and it costs at most 60 segments
/// for a half turn.
fn draw_angular(
    b: &mut ContentBuilder,
    bounds: &mut BoundsAcc,
    apex: Point,
    dir_a: Point,
    dir_b: Point,
    radius: f64,
    style: DimensionStyle,
) {
    if !(radius.is_finite() && radius > 0.0) {
        return;
    }
    let a0 = dir_a.y.atan2(dir_a.x);
    let a1 = dir_b.y.atan2(dir_b.x);
    // Sweep the SHORT way between the two arms. The arms already point into
    // the wedge the operator chose, so the angle between them is the one to
    // draw; normalising into (-pi, pi] picks that sweep without needing to
    // know which arm was clicked first.
    let mut sweep = a1 - a0;
    while sweep > std::f64::consts::PI {
        sweep -= std::f64::consts::TAU;
    }
    while sweep <= -std::f64::consts::PI {
        sweep += std::f64::consts::TAU;
    }

    // Extension lines: from the apex out past the arc, so the arc visibly
    // spans between two drawn arms rather than floating.
    let ext = radius + style.arrow_length;
    for d in [dir_a, dir_b] {
        b.move_to(apex.x, apex.y);
        let (ex, ey) = (d.x.mul_add(ext, apex.x), d.y.mul_add(ext, apex.y));
        b.line_to(ex, ey);
        bounds.add(apex);
        bounds.add(Point::new(ex, ey));
    }
    b.paint(Paint::Stroke);

    // The arc.
    let step = 3f64.to_radians().copysign(sweep);
    let steps = (sweep / step).abs().ceil().max(1.0) as usize;
    let at = |t: f64| -> Point {
        let ang = t.mul_add(sweep, a0);
        Point::new(
            ang.cos().mul_add(radius, apex.x),
            ang.sin().mul_add(radius, apex.y),
        )
    };
    let start = at(0.0);
    b.move_to(start.x, start.y);
    bounds.add(start);
    for i in 1..=steps {
        #[allow(clippy::cast_precision_loss)]
        let p = at(i as f64 / steps as f64);
        b.line_to(p.x, p.y);
        bounds.add(p);
    }
    b.paint(Paint::Stroke);

    // Arrowheads at each end, pointing ALONG the arc (tangentially) and
    // outward — which is what makes the mark read as "this much sweep"
    // rather than as two unrelated ticks. The tangent at the start points
    // backwards along the sweep, and at the end forwards.
    let tangent = |ang: f64, sign: f64| -> (f64, f64) {
        let (tx, ty) = (-ang.sin(), ang.cos());
        (tx * sign, ty * sign)
    };
    let sign = if sweep >= 0.0 { 1.0 } else { -1.0 };
    arrowhead(b, bounds, start, tangent(a0, -sign), style);
    let end = at(1.0);
    arrowhead(b, bounds, end, tangent(a0 + sweep, sign), style);
}

/// Draw a perimeter / path-length ce dimension: the picked polyline itself,
/// stroked in the resolved style, closed when the kind says so (`Pass 107.0`).
///
/// # What is deliberately NOT drawn, and why each absence is a decision
///
/// **No terminators.** A linear ce dimension puts an arrowhead at each end
/// because the pair of arrows is what says *"the number is the distance
/// between these two points"*. A perimeter's number is the distance
/// **along** a path, and two arrows at the ends of an open one would assert
/// exactly the wrong reading — that the number spans from one end to the
/// other. On a closed shape there are no ends to terminate at all. So the
/// shape is stroked plain, and the shape itself is the notation.
///
/// **No extension (witness) lines.** They exist to reach from a dimension
/// line standing off the drawing back to the points it measures. A perimeter's
/// drawn geometry IS the measured geometry — it stands off nothing — so there
/// is nothing for a witness line to witness.
///
/// **No break in the line for the label under ANSI.** The label sits at the
/// vertex centroid, which is generally not on any segment; breaking a segment
/// that the label is nowhere near would put a gap in the drawing for no
/// reason. See [`author_dimension`]'s `lift`, which centres the label on its
/// anchor under both standards for the same reason.
///
/// # The bounds obligation
///
/// Every vertex is fed to `bounds`. An under-sized `/Rect` clips the
/// annotation in every conforming reader, and on this kind the geometry is
/// the entire drawing — so a missed vertex is a visibly truncated shape
/// rather than a slightly tight box.
///
/// An empty vertex list emits nothing at all rather than a degenerate path.
/// The verbs that can shorten a vertex list refuse before reaching that state
/// ([`crate::edit::EditSession::remove_dimension_vertex`]); this is the
/// belt-and-braces half, and it is silent because there is no operator here to
/// disclose to — the refusal already happened upstream.
fn draw_perimeter(b: &mut ContentBuilder, bounds: &mut BoundsAcc, points: &[Point], closed: bool) {
    let Some(first) = points.first().copied() else {
        return;
    };
    b.move_to(first.x, first.y);
    bounds.add(first);
    for p in points.iter().skip(1) {
        b.line_to(p.x, p.y);
        bounds.add(*p);
    }
    if closed {
        b.close_subpath();
    }
    b.paint(Paint::Stroke);
}

/// Emit the terminator at `tip`, pointing along the unit direction `dir`, in
/// whatever form the resolved style asks for (Pass 69.0).
///
/// # Why one function rather than one per form
///
/// Every form shares the same three inputs (tip, direction, size) and the same
/// obligation to feed `bounds` — and a terminator missing from the bounds
/// accumulator produces an under-sized `/Rect`, which clips it in every
/// conforming reader. Keeping them in one function makes that obligation
/// impossible to forget for a form added later: the `match` returns nothing, so
/// a new arm that never touches `bounds` is visibly different from its
/// neighbours.
///
/// The proportions (a 0.35 half-width, a 45-degree slash, a radius-0.2 dot) are
/// **drafting convention, not standard requirements** — the same honesty
/// [`DimensionStyle::extension_metrics`] applies to the extension-line gap.
/// They scale with [`DimensionStyle::arrow_length`], so enlarging the
/// terminator enlarges it proportionally rather than distorting it.
fn arrowhead(
    b: &mut ContentBuilder,
    bounds: &mut BoundsAcc,
    tip: Point,
    dir: (f64, f64),
    style: DimensionStyle,
) {
    let (ux, uy) = dir;
    if !(ux.is_finite() && uy.is_finite()) {
        return;
    }
    let len = style.arrow_length;
    if !(len.is_finite() && len > 0.0) {
        return;
    }
    let (px, py) = (-uy, ux);
    let half = len * 0.35;
    let bx = tip.x - ux * len;
    let by = tip.y - uy * len;
    let b1 = Point::new(bx + px * half, by + py * half);
    let b2 = Point::new(bx - px * half, by - py * half);

    match style.arrow_form {
        // The pre-Pass-69.0 shape, byte-for-byte: a closed filled triangle.
        ArrowForm::Filled => {
            b.move_to(tip.x, tip.y);
            b.line_to(b1.x, b1.y);
            b.line_to(b2.x, b2.y);
            b.close_subpath();
            b.paint(Paint::Fill);
            bounds.add(tip);
            bounds.add(b1);
            bounds.add(b2);
        }
        // An open V — two strokes meeting at the tip, deliberately NOT closed
        // back across the base. A closed-and-stroked triangle is a different
        // mark (a hollow triangle), and architectural practice wants the V.
        ArrowForm::Open => {
            b.move_to(b1.x, b1.y);
            b.line_to(tip.x, tip.y);
            b.line_to(b2.x, b2.y);
            b.paint(Paint::Stroke);
            bounds.add(tip);
            bounds.add(b1);
            bounds.add(b2);
        }
        // A 45-degree tick THROUGH the dimension line, centred on the tip —
        // it extends on both sides, which is what distinguishes it from a
        // half-length tick that reads as a stray line end.
        ArrowForm::Slash => {
            // Rotate the along-line direction by 45 degrees.
            const R: f64 = std::f64::consts::FRAC_1_SQRT_2;
            let (sx, sy) = (ux.mul_add(R, -(uy * R)), uy.mul_add(R, ux * R));
            let arm = len * 0.5;
            let p1 = Point::new(tip.x - sx * arm, tip.y - sy * arm);
            let p2 = Point::new(tip.x + sx * arm, tip.y + sy * arm);
            b.move_to(p1.x, p1.y);
            b.line_to(p2.x, p2.y);
            b.paint(Paint::Stroke);
            bounds.add(p1);
            bounds.add(p2);
        }
        // A filled dot centred ON the tip (not behind it): the dot marks the
        // point, it does not point at it.
        ArrowForm::Dot => {
            let r = len * 0.2;
            emit_circle_path(b, tip, r);
            b.paint(Paint::Fill);
            bounds.add(Point::new(tip.x - r, tip.y - r));
            bounds.add(Point::new(tip.x + r, tip.y + r));
        }
        // Nothing drawn — but the tip still counts toward the bounds, because
        // the dimension line reaches it and an under-sized `/Rect` would clip
        // the line itself.
        ArrowForm::None => bounds.add(tip),
    }
}

/// Emit a circle outline centred at `c` radius `r` as four kappa cubics.
fn emit_circle(b: &mut ContentBuilder, c: Point, r: f64) {
    emit_circle_path(b, c, r);
    b.paint(Paint::Stroke);
}

/// The circle PATH alone, unpainted — so a caller can choose stroke or fill.
///
/// Split out of [`emit_circle`] for the `Dot` terminator (Pass 69.0), which
/// needs the same four-kappa construction with `f` rather than `S`. The split
/// is deliberate rather than a second copy of the curve maths: the kappa
/// constant and the quadrant order are the kind of thing that gets transcribed
/// slightly wrong, and two copies would drift the first time one is corrected.
fn emit_circle_path(b: &mut ContentBuilder, c: Point, r: f64) {
    const KAPPA: f64 = 0.552_284_749_830_793_4;
    let o = r * KAPPA;
    b.move_to(c.x + r, c.y);
    b.curve_to(c.x + r, c.y + o, c.x + o, c.y + r, c.x, c.y + r);
    b.curve_to(c.x - o, c.y + r, c.x - r, c.y + o, c.x - r, c.y);
    b.curve_to(c.x - r, c.y - o, c.x - o, c.y - r, c.x, c.y - r);
    b.curve_to(c.x + o, c.y - r, c.x + r, c.y - o, c.x + r, c.y);
    b.close_subpath();
}

/// The unit vector from `a` to `b`, or `(1, 0)` for a degenerate (zero-length)
/// segment.
fn unit_vector(a: Point, b: Point) -> (f64, f64) {
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    let len = dx.hypot(dy);
    if len <= f64::EPSILON || !len.is_finite() {
        (1.0, 0.0)
    } else {
        (dx / len, dy / len)
    }
}

/// A coarse text-width estimate (Helvetica averages ~0.5em per character) —
/// good enough to centre the baked label; exact centring is not load-bearing.
fn estimate_text_width(label: &str, size: f64) -> f64 {
    label.chars().count() as f64 * size * 0.5
}

/// The `/AP` form-XObject dict for a dimension: `/BBox` = the page-space
/// `/Rect` (geometry drawn in absolute coords), identity matrix, `/Resources`
/// carrying the Base-14 Helvetica label font.
fn ap_form_dict(rect: Rect) -> Dict {
    let mut fonts = Dict::new();
    fonts.insert(
        Name(FONT_RESOURCE.to_vec()),
        Object::Dict(standard14_font_dict(Std14::Helvetica)),
    );
    let mut resources = Dict::new();
    resources.insert(Name::from(b"Font"), Object::Dict(fonts));

    let mut d = Dict::new();
    d.insert(Name::from(b"Type"), Object::Name(Name::from(b"XObject")));
    d.insert(Name::from(b"Subtype"), Object::Name(Name::from(b"Form")));
    d.insert(Name::from(b"BBox"), rect_array(rect));
    d.insert(Name::from(b"Resources"), Object::Dict(resources));
    d
}

/// A `[llx lly urx ury]` array.
fn rect_array(r: Rect) -> Object {
    Object::Array(vec![
        Object::Real(r.llx),
        Object::Real(r.lly),
        Object::Real(r.urx),
        Object::Real(r.ury),
    ])
}

/// A running bounds accumulator that yields a strictly-positive `/Rect`
/// (§12.5.5 WF4: a degenerate `/BBox` is a NEGATIVE RESULT).
struct BoundsAcc {
    llx: f64,
    lly: f64,
    urx: f64,
    ury: f64,
}

impl BoundsAcc {
    fn new() -> Self {
        Self {
            llx: f64::INFINITY,
            lly: f64::INFINITY,
            urx: f64::NEG_INFINITY,
            ury: f64::NEG_INFINITY,
        }
    }

    fn add(&mut self, p: Point) {
        if p.is_finite() {
            self.llx = self.llx.min(p.x);
            self.lly = self.lly.min(p.y);
            self.urx = self.urx.max(p.x);
            self.ury = self.ury.max(p.y);
        }
    }

    fn into_rect(self) -> Rect {
        // A small margin so strokes/arrowheads are not clipped by the BBox.
        let margin = 2.0;
        let (mut llx, mut lly, mut urx, mut ury) = if self.llx.is_finite() {
            (self.llx, self.lly, self.urx, self.ury)
        } else {
            (0.0, 0.0, 1.0, 1.0)
        };
        llx -= margin;
        lly -= margin;
        urx += margin;
        ury += margin;
        if urx - llx < 1.0 {
            urx = llx + 1.0;
        }
        if ury - lly < 1.0 {
            ury = lly + 1.0;
        }
        Rect { llx, lly, urx, ury }
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
    use crate::content::ContentStream;
    use crate::dimension::fit::FitCircle;
    use crate::dimension::units::Unit;
    use crate::vector::AxisConstraint;

    fn linear() -> DimensionKind {
        DimensionKind::Linear {
            a: Point::new(100.0, 100.0),
            b: Point::new(200.0, 100.0),
            constraint: AxisConstraint::Horizontal,
            offset: 0.0,
            text_along: 0.0,
        }
    }

    #[test]
    fn linear_dimension_bakes_a_line_it_and_measure() {
        let d = author_dimension(
            &linear(),
            DimensionStyle::new(
                ScaleState::Calibrated { scale: 0.01 },
                NumberFormat::decimal(Unit::Meter, 2),
                DimStandard::Ansi,
            ),
        );
        assert_eq!(d.label, "1.00 m");
        assert_eq!(
            d.annot
                .get(b"Subtype")
                .unwrap()
                .as_name()
                .unwrap()
                .as_bytes(),
            b"Line"
        );
        assert_eq!(
            d.annot.get(b"IT").unwrap().as_name().unwrap().as_bytes(),
            b"LineDimension"
        );
        assert!(d.annot.get(b"L").is_some());
        assert!(d.annot.get(b"Measure").is_some());
        assert_eq!(
            d.annot.get(b"Contents").unwrap(),
            &Object::String(b"1.00 m".to_vec())
        );
        // The /Rect has positive area.
        assert!(d.rect.width() > 0.0 && d.rect.height() > 0.0);
    }

    #[test]
    fn appearance_content_reparses_as_a_content_stream() {
        // The baked /AP must never emit a stream the tokenizer rejects (R59
        // renders it; a malformed stream would fail render).
        let d = author_dimension(
            &linear(),
            DimensionStyle::new(
                ScaleState::Calibrated { scale: 0.01 },
                NumberFormat::decimal(Unit::Meter, 2),
                DimStandard::Ansi,
            ),
        );
        ContentStream::parse(d.ap_content.clone()).expect("baked /AP must reparse");
        // The label text is shown, and a font is set.
        let s = String::from_utf8(d.ap_content.clone()).unwrap();
        assert!(s.contains("/Helv 10 Tf"), "{s}");
        assert!(s.contains("(1.00 m) Tj"), "{s}");
    }

    #[test]
    fn never_set_scale_bakes_raw_units_and_no_measure() {
        let d = author_dimension(
            &linear(),
            DimensionStyle::new(
                ScaleState::NeverSet,
                NumberFormat::decimal(Unit::Meter, 2),
                DimStandard::Ansi,
            ),
        );
        assert_eq!(d.label, "100.00 pt");
        assert!(d.annot.get(b"Measure").is_none());
    }

    #[test]
    fn circular_dimension_prefixes_r_or_dia() {
        let fit = FitCircle {
            center: Point::new(50.0, 50.0),
            radius: 20.0,
            residual: 0.1,
        };
        let r = author_dimension(
            &DimensionKind::Circular {
                fit,
                show_diameter: false,
            },
            DimensionStyle::new(
                ScaleState::Calibrated { scale: 0.05 },
                NumberFormat::decimal(Unit::Centimeter, 2),
                DimStandard::Ansi,
            ),
        );
        assert!(r.label.starts_with("R "), "{}", r.label);
        let dia = author_dimension(
            &DimensionKind::Circular {
                fit,
                show_diameter: true,
            },
            DimensionStyle::new(
                ScaleState::Calibrated { scale: 0.05 },
                NumberFormat::decimal(Unit::Centimeter, 2),
                DimStandard::Ansi,
            ),
        );
        assert!(dia.label.starts_with("DIA "), "{}", dia.label);
        // Diameter reads twice the radius.
        assert_eq!(r.label, "R 1.00 cm"); // 20 pt * 0.05 = 1.0
        assert_eq!(dia.label, "DIA 2.00 cm"); // 40 pt * 0.05 = 2.0
    }

    #[test]
    fn regeneration_is_deterministic() {
        // Same inputs → byte-identical appearance (regeneration-safe).
        let a = author_dimension(
            &linear(),
            DimensionStyle::new(
                ScaleState::OneToOne,
                NumberFormat::decimal(Unit::Inch, 2),
                DimStandard::Ansi,
            ),
        );
        let b = author_dimension(
            &linear(),
            DimensionStyle::new(
                ScaleState::OneToOne,
                NumberFormat::decimal(Unit::Inch, 2),
                DimStandard::Ansi,
            ),
        );
        assert_eq!(a, b);
    }
}
