//! # Vector object decomposition (ISO 32000-1 §8.5 paths, §8.2 operators)
//!
//! The **read-only decomposition** that turns a page's lossless
//! content-token stream ([`crate::content::ContentStream`]) into a list of
//! selectable [`VectorObject`]s, per
//! `docs/decisions/011-first-beta-scaled-measurement-dimensioning-tool.md`
//! §2.1. It **indexes** the token model; it never rewrites it. Pass 9a is
//! **byte-inert** (R46): running this over the corpus changes zero output
//! bytes — proven by re-running the content-identity gate.
//!
//! ## What an object is (§8.5.2–§8.5.3, Table 59/60)
//!
//! Walking the token stream while tracking graphics state (CTM via
//! `q`/`Q`/`cm`, line width, device colour), a **path object** is the run
//! of path-construction operators (`m l c v y re h`) terminating in a
//! **painting operator** (`S s f F f* B B* b b* n`). Each object captures,
//! per decision 011 §2.1:
//!
//! 1. its subpaths as **node lists** ([`Subpath`] of anchors + Bézier
//!    control points) in **user space**, plus the effective **CTM** that
//!    maps them to **page space** (for hit-test/snap — see
//!    [`PathObject::page_subpaths`]);
//! 2. the **effective graphics state** at paint time (CTM captured at the
//!    path's *first* construction op — the same rule the renderer uses;
//!    line width; fill/stroke colour);
//! 3. the **content-token index range** ([`TokenRange`]) and the
//!    equivalent [`ByteSpan`] of its defining operators — the handle a
//!    later editing Pass (9c-min) maps back to the Pass 8.0 surgery
//!    interpreter. **9a captures it; it does not use it.**
//!
//! **Text objects** (`BT`…`ET`) and **image objects** (`Do` on an image
//! XObject, an inline `BI`/`ID`/`EI`, or a form `Do`) are decomposed as
//! **selectable-for-move/delete** objects — not node-editable in the beta
//! (dimensioning cares about path geometry) — carrying their bbox, their
//! token range, and the small **identifying detail** a human needs to tell
//! one from another: a text object's shown string and font
//! ([`TextPreview`], [`TextFont`]), an image object's pixel dimensions
//! ([`ImageObject::pixel_size`]). A text object's bbox is a documented
//! **approximation** (see [`TextObject`]).
//!
//! ## Identifying detail, and the two rules it obeys
//!
//! ui-spec `pass-17-dock-and-layer-tree.md` §B.4 asks for exactly this: an
//! object list that can only say `Text` three times is not a troubleshooting
//! tool. Two rules bind how it is produced:
//!
//! 1. **One decoder, never two.** Show-operator strings are decoded through
//!    [`crate::text_extract::ExtractFont`] — the same §9.10.2 ladder
//!    `extract-text` climbs — reached through the [`FontResolver`] seam.
//!    A second, simpler decoder here would disagree with `extract-text` on
//!    exactly the fonts that are hard, which is the decision 011 §Z2
//!    divergence shape one layer up from geometry.
//! 2. **Never invent a value that cannot be justified.** When the ladder
//!    recovers nothing for a text object, the preview is
//!    [`TextPreview::Undecodable`], **not** a string of mojibake; when the
//!    caller supplied no font resolver at all, it is
//!    [`TextPreview::Unavailable`], which is a different fact and says so.
//!    Rule 4 (fuzzy, never sneaky) applied where the operator is least able
//!    to catch a fabrication.
//!
//! ### Bounded memory (the 50k-object page)
//!
//! [`PageObjects`] now carries owned `String`s, so the cost is capped **at
//! decomposition**, not at display: a preview is cut at
//! [`MAX_TEXT_PREVIEW_CHARS`] characters and the decode loop *stops there*
//! (a 10 kB show string is not decoded and then thrown away), and font
//! names are cut at [`MAX_FONT_NAME_BYTES`]. Worst case per text object is
//! therefore ~256 B of preview (64 chars × 4 bytes for astral code points)
//! plus two ≤64 B names plus their `String` headers — under ~450 B. A
//! hostile page of 50,000 text objects costs ≈22 MB of preview at the
//! absolute worst and ≈5 MB for realistic Latin text, against the
//! [`MAX_OBJECTS`] ceiling of 1,000,000 objects that already bounds the
//! object list itself. Truncation is **disclosed**
//! ([`TextPreview::Decoded::truncated`]), never silent.
//!
//! ## Agreement with the renderer (the Z2 risk, decision 011)
//!
//! The construction rules here mirror `pdfcer-render`'s interpreter
//! exactly — the `v`/`y` implicit-control-point traps and the `re`
//! expansion go through the SHARED primitives in
//! [`crate::vector::geometry`] (`cubic_from_v`/`cubic_from_y`/
//! `rect_corners`) that the renderer also calls, and the CTM update uses
//! the same `post_concat` composition. A cross-check acceptance test in
//! `pdfcer-render` compares the full page-space geometry the two pipelines
//! produce on the vector fixtures, so a divergence is caught by a test,
//! not by a mis-rendered dimension.
//!
//! ## Panic-free / adversarial input (ARCHITECTURE.md §10)
//!
//! Every operand access is checked; every degenerate shape (missing
//! current point, unbalanced `q`/`Q`, mid-path `cm`, non-finite operands,
//! huge node counts) is tolerated and **counted** in
//! [`DecomposeDiagnostics`] rather than panicking — the same
//! "fuzzy, never sneaky" posture the renderer takes. A fuzz target
//! (`fuzz/fuzz_targets/vector_decompose.rs`) drives exactly these shapes.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use crate::content::{ContentStream, ContentToken, ContentTokenKind};
use crate::graph::ObjectGraph;
use crate::object::{Dict, ObjId, Object};
use crate::page_tree::Page;
use crate::settings::UnmappableCode;
use crate::span::ByteSpan;
use crate::text_extract::{ExtractFont, LadderRung};
use crate::text_state::TextStateParams;
use crate::view::DocumentView;

use super::geometry::{Bounds, Matrix, Point, Rgb, cubic_from_v, cubic_from_y, rect_corners};

/// Guard on the number of objects a single page can decompose to
/// (ARCHITECTURE.md §10 adversarial-input posture). A legitimate complex
/// vector drawing has thousands of path objects; a hostile stream that is
/// nothing but paint operators would otherwise allocate without bound.
/// 1,000,000 is far above any real page and still cheap to reject past.
pub const MAX_OBJECTS: usize = 1_000_000;

/// Guard on the total number of path nodes retained across one page, for
/// the same reason as [`MAX_OBJECTS`]: a stream of a million `l` operators
/// is a memory-amplification vector. Past this bound, further construction
/// operators are counted-and-dropped (the object still terminates and is
/// emitted with the nodes it has).
pub const MAX_NODES: usize = 4_000_000;

/// Per-object ceiling on retained [`TextObject::runs`].
///
/// A `BT`…`ET` may contain arbitrarily many show operators, and each
/// retained box costs 32 bytes on an object that already exists. This bounds
/// that at ~128 KB for the pathological case while sitting far above any
/// real text object — the SolidWorks export that motivated per-run bounds
/// carried its whole label set in one object and did not approach it.
///
/// Past the ceiling `runs` is CLEARED rather than truncated. A truncated
/// list is worse than none: a consumer testing against the first N runs
/// would silently stop hit-testing the rest of the object, which is a
/// wrong answer wearing the shape of a right one. Empty means "fall back to
/// `page_bbox`", which is merely imprecise.
pub const MAX_TEXT_RUNS: usize = 4_096;

/// How many decoded characters of a text object's shown string
/// [`TextPreview::Decoded`] retains.
///
/// A *preview*, not the text: the consumer is a one-line object row and a
/// one-line status readout, both of which elide well before this. The
/// number is set here rather than at display time because it is the
/// **memory bound** (module docs' "Bounded memory") — a page of 50,000 text
/// objects must not be able to make the object model larger than the file
/// it came from. Callers that want a page's actual text call
/// [`crate::text_extract::extract_page`], which is the pipeline for that
/// question and streams rather than retaining.
///
/// 64 is chosen to comfortably contain a caption, a dimension label or a
/// short heading — the strings that make a row identifiable — while a
/// paragraph-sized run is cut and **says** it was cut.
pub const MAX_TEXT_PREVIEW_CHARS: usize = 64;

/// Byte ceiling on a retained font name ([`TextFont::resource`] and
/// [`TextFont::base_font`]).
///
/// A `/BaseFont` is a PDF name, and §7.3.5 caps a name at 127 bytes in
/// practice; a hostile file can nevertheless carry a long one on every one
/// of a page's text objects. Cutting at 64 bytes keeps the per-object cost
/// bounded without touching any real font name (`ABCDEF+Helvetica-BoldOblique`
/// is 28). Truncation is on a UTF-8 character boundary, so the result is
/// always valid text.
pub const MAX_FONT_NAME_BYTES: usize = 64;

/// One segment of a subpath, in the coordinate space of its [`Subpath`]
/// (user space as stored; page space after [`PathObject::page_subpaths`]).
///
/// A subpath is a start anchor followed by a list of these. A straight
/// `l`/`re`-edge is a [`Segment::Line`]; every `c`/`v`/`y` cubic is a
/// [`Segment::Cubic`] with its two control points made explicit (the
/// `v`/`y` implicit points already resolved by the shared primitives, so a
/// consumer never has to re-derive them).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Segment {
    /// A straight line to `to`.
    Line {
        /// The segment's end anchor.
        to: Point,
    },
    /// A cubic Bézier with control points `c1`, `c2` ending at `to`.
    Cubic {
        /// First control point.
        c1: Point,
        /// Second control point.
        c2: Point,
        /// The segment's end anchor.
        to: Point,
    },
}

impl Segment {
    /// The segment's end anchor (the on-curve point a following segment
    /// starts from).
    #[must_use]
    pub fn end(self) -> Point {
        match self {
            Segment::Line { to } | Segment::Cubic { to, .. } => to,
        }
    }

    /// Map every point of this segment through `m` (user → page space).
    #[must_use]
    pub fn transformed(self, m: Matrix) -> Self {
        match self {
            Segment::Line { to } => Segment::Line {
                to: m.map_point(to),
            },
            Segment::Cubic { c1, c2, to } => Segment::Cubic {
                c1: m.map_point(c1),
                c2: m.map_point(c2),
                to: m.map_point(to),
            },
        }
    }
}

/// One subpath: a start anchor, its segments, and whether it was closed
/// (`h`, `re`, or a close-and-paint operator such as `s`/`b`).
#[derive(Debug, Clone, PartialEq)]
pub struct Subpath {
    /// The subpath's first on-curve anchor (`m`, or the implicit reopen
    /// after `h`).
    pub start: Point,
    /// The segments after the start, in order.
    pub segments: Vec<Segment>,
    /// Whether the subpath is closed (a closing edge back to `start`).
    pub closed: bool,
    /// The content-token range of the operators that construct this subpath
    /// (Pass 28.0) — its opening `m`/`re` through its last segment, including
    /// a closing `h`.
    ///
    /// This is what makes per-subpath EDITING expressible. Without it, an
    /// editor had to re-walk the operator bytes and hope its walk agreed with
    /// this one about how many subpaths there are; `plan_delete_subpath`
    /// shipped exactly that, with a count guard that refused the whole object
    /// whenever the two disagreed. Recording the range on the walk that
    /// already knows it makes the agreement structural instead of checked.
    pub tokens: TokenRange,
    /// Whether this subpath was started IMPLICITLY — by a segment operator
    /// after `h`, which reopens at the closed subpath's start point (§8.5.2.1)
    /// with no operator of its own saying where.
    ///
    /// # Why this must be recorded rather than inferred
    ///
    /// Such a subpath's start point is INHERITED and carried by no operand. Two
    /// consequences, both silent if unchecked:
    ///
    /// - it cannot be MOVED, because there is no coordinate pair to rewrite;
    /// - the subpath BEFORE it cannot be deleted, because excising those
    ///   operators changes the current point the implicit one starts from, so
    ///   a byte-minimal edit that passes every round-trip check still moves a
    ///   line the operator never touched.
    pub starts_implicitly: bool,
}

impl Subpath {
    /// Map every node of this subpath through `m` (user → page space).
    #[must_use]
    pub fn transformed(&self, m: Matrix) -> Self {
        Self {
            // The token range and the implicit flag describe the SOURCE
            // operators, which a coordinate transform does not move.
            tokens: self.tokens,
            starts_implicitly: self.starts_implicitly,
            start: m.map_point(self.start),
            segments: self.segments.iter().map(|s| s.transformed(m)).collect(),
            closed: self.closed,
        }
    }

    /// The on-curve anchor points of this subpath (start + each segment
    /// end), in order — the snap/hit vertices dimensioning cares about.
    /// Control points are excluded (a snap target is an anchor, not a
    /// handle — Bézier handle editing is a fast-follow).
    pub fn anchors(&self) -> impl Iterator<Item = Point> + '_ {
        std::iter::once(self.start).chain(self.segments.iter().map(|s| s.end()))
    }
}

/// The painting disposition of a [`PathObject`] (§8.5.3, Table 60).
///
/// `fill` is `Some(rule)` for the fill operators (`f`/`F`/`B`/`b` →
/// [`FillRule::NonZero`]; `f*`/`B*`/`b*` → [`FillRule::EvenOdd`]);
/// `stroke` is true for the stroke operators (`S`/`s`/`B…`/`b…`). Both
/// false is the `n` no-op / clip-only path (§8.5.4): geometry that paints
/// nothing yet is still a selectable object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaintStyle {
    /// The fill winding rule, if the object is filled.
    pub fill: Option<FillRule>,
    /// Whether the object is stroked.
    pub stroke: bool,
}

impl PaintStyle {
    /// Whether the object paints nothing (`n` — a clip or bare end-path).
    #[must_use]
    pub fn is_invisible(self) -> bool {
        self.fill.is_none() && !self.stroke
    }
}

/// A path fill winding rule (§8.5.3.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FillRule {
    /// Nonzero winding (`f`, `F`, `B`, `b`).
    NonZero,
    /// Even-odd (`f*`, `B*`, `b*`).
    EvenOdd,
}

/// The half-open range of content-token indices `[start, end)` that a
/// decomposed object's **defining operators** occupy in
/// [`ContentStream::tokens`] — from the first construction operator's
/// first token through the painting operator (or `BT`→`ET`, or the `Do`
/// operation). The editing handle 9c-min will map to the Pass 8.0 surgery
/// interpreter; **9a captures it and does not use it**.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TokenRange {
    /// Index of the first defining token (inclusive).
    pub start: usize,
    /// Index one past the last defining token (exclusive) — i.e. the
    /// painting/`ET`/`Do` operator index plus one.
    pub end: usize,
}

impl TokenRange {
    /// The range as a [`std::ops::Range`] for slicing
    /// [`ContentStream::tokens`].
    #[must_use]
    pub fn as_range(self) -> std::ops::Range<usize> {
        self.start..self.end
    }
}

/// A path object — the node-editable heart of the model (module docs).
#[derive(Debug, Clone, PartialEq)]
pub struct PathObject {
    /// The subpaths in **user space** (decision 011 §2.1 item 1). Map them
    /// to page space via [`PathObject::page_subpaths`].
    pub subpaths: Vec<Subpath>,
    /// The effective CTM captured at the path's **first** construction op
    /// (the same rule the renderer's `path_ctm` uses), mapping user → page
    /// space.
    pub ctm: Matrix,
    /// Fill/stroke disposition at paint time.
    pub style: PaintStyle,
    /// Line width in **user-space** units at paint time (§8.4.3.2) — what a
    /// stroke-proximity hit-test widens by.
    pub line_width: f64,
    /// Non-stroking (fill) colour at paint time.
    ///
    /// ★ ONLY MEANINGFUL WHEN [`Self::fill_paint`] IS `Device` OR `Default`.
    /// For a `/Separation`, `/DeviceN`, `/ICCBased`, `/Indexed`, `/Lab` or
    /// pattern fill this holds pdfcer's best-effort screen value, which before
    /// `Pass 218.0` was a stale colour from an unrelated earlier operator.
    /// Consult `fill_paint` before asserting a colour or writing one out.
    pub fill_color: Rgb,
    /// Stroking colour at paint time. Same caveat as [`Self::fill_color`].
    pub stroke_color: Rgb,
    /// The fill paint **as the file states it**, including the case pdfcer
    /// cannot decode. See [`PathPaint`].
    pub fill_paint: PathPaint,
    /// The stroke paint as the file states it. See [`PathPaint`].
    pub stroke_paint: PathPaint,
    /// The defining-operator token range (the future editing handle).
    pub tokens: TokenRange,
    /// The equivalent byte span in the decoded content buffer.
    pub bytes: ByteSpan,
    /// Precomputed **page-space** bounds (control-point hull — a
    /// conservative superset of the exact curve bounds), for the hit-test
    /// bbox pre-filter and marquee enclosure.
    pub page_bbox: Bounds,
}

impl PathObject {
    /// The subpaths mapped into **page space** by [`PathObject::ctm`] —
    /// what the hit-test, snapping engine (12.M1) and centerline
    /// derivation consume (decision 011 §2.1 item 1).
    #[must_use]
    pub fn page_subpaths(&self) -> Vec<Subpath> {
        self.subpaths
            .iter()
            .map(|s| s.transformed(self.ctm))
            .collect()
    }

    /// Whether the object is exactly one closed 4-anchor quad (an `re`
    /// rectangle or a hand-drawn 4-line closed quad) — the shape the
    /// filled-line centerline derivation (`super::centerline`) inspects.
    #[must_use]
    pub fn is_quad(&self) -> bool {
        matches!(self.subpaths.as_slice(), [only] if only.closed && subpath_is_quad(only))
    }
}

/// Whether the source of an image object was an inline image, an image
/// XObject, or a form XObject (§8.8/§8.9.7). Recorded for disclosure; all
/// three are bbox-selectable and none is node-editable in the beta.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageSource {
    /// A `BI`/`ID`/`EI` inline image (§8.9.7).
    Inline,
    /// A `Do` on an image XObject (§8.9).
    XObject,
    /// A `Do` on a form XObject (§8.10) — treated as one opaque
    /// selectable object bounded by its `/BBox`; 9a does NOT recurse into
    /// the form's own content (per-form path decomposition is a
    /// fast-follow).
    Form,
}

/// An image/form object — selectable-for-move/delete, bbox only (module
/// docs). Its page bbox is the image unit square (§8.9.4) or the form
/// `/BBox` (§8.10.1) mapped by the effective transform.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ImageObject {
    /// The effective CTM at the `Do`/inline-image operator.
    pub ctm: Matrix,
    /// Page-space bounds (unit square or form `/BBox` under the transform).
    pub page_bbox: Bounds,
    /// Where the object came from.
    pub source: ImageSource,
    /// The XObject stream's identity, for an object drawn by `Do`.
    ///
    /// `None` for an inline image (§8.9.7 — it *is* the content, it has no
    /// object of its own) and for a `Do` whose resource entry was a direct
    /// stream rather than a reference.
    ///
    /// # ★ What a caller does with it
    ///
    /// For a **form**, this names the content stream that a token range from
    /// *inside* that form indexes — a different buffer from the page's. It is
    /// also the key a recursion has to guard on: §8.10.1 does not forbid a
    /// form invoking itself, and the same stream is reachable under different
    /// resource names, so a name-keyed cycle guard misses the cycle.
    ///
    /// For an **image**, it is the identity of the sample data, which is what
    /// tells two placements of one image apart from two separate images.
    pub xobject: Option<ObjId>,
    /// The image's size in **samples** — `(width, height)` from the image
    /// dictionary's `/Width` and `/Height` (ISO 32000-1 §8.9.5, Table 89:
    /// both **required** integers, "width/height … in samples"), or the
    /// inline image's normalized `/W`/`/H` (§8.9.7, Table 93).
    ///
    /// **This is a sample count, not a size on the page.** §8.9.5's own
    /// note is blunt about it: an image occupies the user-space unit square
    /// under the CTM, so its printed size comes from the CTM and has no
    /// fixed relationship to these numbers. `640×480` in a row answers
    /// "which image is this?" (and, against [`page_bbox`](Self::page_bbox),
    /// "at what effective resolution is it placed?"); it never answers "how
    /// big is it on the paper".
    ///
    /// `None` for a form XObject (§8.10 — a form has no samples, it has a
    /// `/BBox`), and for a malformed image whose `/Width`/`/Height` are
    /// absent, non-integer, negative or larger than `u32`. A missing value
    /// is reported as missing rather than guessed at from the CTM, which
    /// would be a fabricated number the operator could not check.
    pub pixel_size: Option<(u32, u32)>,
    /// The defining-operator token range.
    pub tokens: TokenRange,
    /// The equivalent byte span.
    pub bytes: ByteSpan,
}

/// The font in effect at a text object's first show operator.
///
/// Captured at the FIRST `Tj`/`TJ`/`'`/`"` rather than at `ET`, because a
/// text object that switches font mid-run should be identified by the font
/// its visible run *starts* in — the same run [`TextPreview`] previews. A
/// text object that never shows anything has no font (there is no
/// operand to report), and one that shows without a preceding `Tf` has an
/// empty [`resource`](Self::resource) recorded as `None` rather than as `""`.
#[derive(Debug, Clone, PartialEq)]
pub struct TextFont {
    /// The `/Tf` **resource name** as written in the content stream (`F1`,
    /// `TT2`), decoded from the name's bytes with invalid UTF-8 replaced,
    /// and cut at [`MAX_FONT_NAME_BYTES`].
    ///
    /// This is the key into the page's `/Font` resource dictionary — the
    /// handle a future edit needs — not a typeface name. Prefer
    /// [`base_font`](Self::base_font) when showing a human which typeface
    /// they are looking at.
    pub resource: String,
    /// `/BaseFont` from the resolved font dictionary (§9.6.2.1 Table 111 /
    /// §9.7.4 Table 120), subset tag included (`ABCDEF+Helvetica`), cut at
    /// [`MAX_FONT_NAME_BYTES`].
    ///
    /// `None` when no [`FontResolver`] was supplied, or the name is not in
    /// the resource dictionary, or the font dictionary carries no
    /// `/BaseFont`. Never synthesised from the resource name — `F1` is not
    /// evidence of any typeface.
    pub base_font: Option<String>,
    /// The **`Tf` size operand** in effect (§9.3.1, Table 105 `Tfs`).
    ///
    /// A *text-space* quantity, reported exactly as the file states it and
    /// **not** scaled by the text matrix or the CTM. A file that writes
    /// `/F1 1 Tf` and then `12 0 0 12 x y Tm` renders 12 pt type and is
    /// reported here as `1` — which is what the file says. Folding the
    /// matrices in would produce a confident number that disagrees with the
    /// operand an operator would find in the content stream, and pdfcer has
    /// no glyph metrics with which to defend a measured alternative
    /// (see [`TextObject`]'s bbox note for the same limitation).
    pub size: f64,
}

/// What a text object's shown string decoded to — or, honestly, why it did
/// not (module docs' rule 2).
///
/// Four distinguishable answers, because "no text" has four genuinely
/// different causes and collapsing them into `Option<String>` would tell the
/// operator that the file is empty when in fact pdfcer declined to guess.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextPreview {
    /// Strings were shown, but **no decoder was in scope**, so no decoding
    /// was attempted: either the caller supplied no [`FontResolver`] (what
    /// plain [`decompose`] does), or the `Tf` name did not resolve to a font
    /// in the page's `/Font` resources.
    ///
    /// A property of the LOOKUP, never of the document's text. The
    /// distinction from [`TextPreview::Empty`] matters: one says pdfcer did
    /// not look, the other says there was nothing to find.
    Unavailable,
    /// Decoding ran and produced characters.
    Decoded {
        /// The decoded characters, at most [`MAX_TEXT_PREVIEW_CHARS`] of
        /// them.
        ///
        /// Sourced characters only: the codes are mapped through the
        /// §9.10.2 ladder and concatenated **verbatim**, with none of
        /// `text_extract`'s derived inter-word spacing or line breaking
        /// (§14.8.2.5 S2/S3/S5 — a content stream carries no word or line
        /// signal, and inventing one in a row label would be a guess the
        /// operator cannot review). A `TJ` array's kerning offsets are
        /// therefore invisible here, exactly as
        /// [`ExtractedText::sourced_text`](crate::text_extract::ExtractedText::sourced_text)
        /// treats them.
        text: String,
        /// Whether the shown string ran past [`MAX_TEXT_PREVIEW_CHARS`] and
        /// was cut. Disclosed so a display can mark the elision rather than
        /// silently present a prefix as the whole string.
        truncated: bool,
        /// Whether **some** codes in the decoded prefix defeated the ladder
        /// and were emitted as U+FFFD (`LadderRung::Failed`).
        ///
        /// The replacement characters are left in `text` — that is
        /// `text_extract`'s own disclosed policy for an unmappable code —
        /// and this flag is what lets a consumer say so in words instead of
        /// leaving the operator to interpret a row full of `�`.
        lossy: bool,
    },
    /// Codes were shown and **not one of them** could be mapped to a
    /// character: every code reached §9.10.2's failure clause ("there is no
    /// way to determine what the character code represents"), the canonical
    /// case being `Identity-H` with no `/ToUnicode`.
    ///
    /// A distinct variant rather than `Decoded { text: "���" }` because the
    /// honest answer is *"this text cannot be read"*, and a row of
    /// replacement characters looks instead like a pdfcer bug.
    Undecodable,
    /// The text object showed no strings at all — a `BT`/`ET` that only
    /// positioned, or whose show operands were not strings.
    Empty,
}

/// What a [`TextObject`]'s bounding box was actually built from.
///
/// The box is an approximation under every variant (no variant means
/// "measured glyph outlines" — see [`TextObject`]'s own note), but the
/// variants are **not** interchangeable: they differ by roughly an order of
/// magnitude in how far the box can be from the ink, and a consumer that
/// discloses the approximation must say which one it is looking at.
/// ui-spec `pass-17-dock-and-layer-tree.md` §E.3 makes that binding: *"the
/// sentence shown always matches the box actually drawn"* — one disclosure
/// sentence covering all four would be accurate for one of them and a lie
/// for the rest.
///
/// Ordered best-to-worst, which is also the order the walk prefers them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TextBoundsBasis {
    /// **The good case.** Every glyph's horizontal advance came from the
    /// font's own width table — `/Widths` for a simple font (§9.6.2.1),
    /// `/W` over `/DW` for a composite one (§9.7.4.3), or the compiled-in
    /// standard-14 AFM metrics for a base-14 dictionary that omitted its
    /// `/Widths` (§9.6.2.2) — and the vertical extent from the font's
    /// `/FontDescriptor` `/Ascent`/`/Descent` (§9.8, Table 122).
    ///
    /// The box is where a conforming reader lays the run out. What it is
    /// still not: the ink. Accented capitals exceed `/Ascent` by the
    /// clause's own definition, italic overhang leans past the advance, and
    /// a run of hyphens never reaches the nominal ascent — so the box can
    /// be a little tall or a little narrow at the extremes.
    FontMetrics,
    /// Advances came from the font's width table as in
    /// [`Self::FontMetrics`], but **no vertical extent was available** —
    /// no usable `/Ascent`+`/Descent`, no `/FontBBox`, and not a
    /// standard-14 face — so the height is pdfcer's nominal em fallback.
    /// The canonical case is a Type 3 font (§9.6.5), whose glyph space is
    /// its own `/FontMatrix` and whose descriptor is optional.
    ///
    /// Horizontally as good as `FontMetrics`; vertically a guess, and the
    /// horizontal axis is the one that was broken.
    MetricAdvancesNominalHeight,
    /// The font carried **no width source at all** — no `/Widths`, and not
    /// one of the standard 14 — so `text_extract` estimated its advances
    /// from metrically-similar Helvetica (its own
    /// [`FontNote::WidthsEstimated`](crate::text_extract::FontNote::WidthsEstimated)).
    /// §9.6.2.2 does not permit this shape; real files ship it anyway.
    ///
    /// The box has the right *structure* — it starts where the run starts
    /// and grows with the run's length — but its width is an estimate of an
    /// estimate. Better than an em box; not a measurement.
    EstimatedAdvances,
    /// **The fallback.** No font was resolvable for at least one of the
    /// object's show operators, so that operator's extent is the legacy
    /// approximation: the hull of the text-showing **origins** (each
    /// `Tj`/`TJ`/`'`/`"` pen position mapped to page space) inflated
    /// isotropically by the largest `Tf` size seen.
    ///
    /// This is the box every text object had before advance accumulation
    /// existed, and its failure mode is why the change was made: for a
    /// single-`Tj` object it is a **square centred on where the run
    /// starts**, so it covers blank paper to the left of the first glyph
    /// and stops roughly one em in — clicking directly on visible letters
    /// past that point misses the object. It is kept, rather than dropped
    /// in favour of nothing, because a loose hit target is strictly better
    /// than none: fonts genuinely go missing (a damaged file, a caller with
    /// no document — [`NoFonts`]), and a selection surface that silently
    /// stops working on such a page would be the worse failure.
    ///
    /// Also the basis reported when an object **mixes** metered and
    /// unmetered show operators: part of the box is an em-box guess, so the
    /// weaker claim is the honest one for the whole.
    EmBox,
}

/// A text object (`BT`…`ET`) — selectable-for-move/delete, with an
/// identifying preview.
///
/// **The bbox is a deliberate approximation — of one of four kinds.**
/// `pdfcer-core` has no glyph *outlines* (font programs live behind the
/// `pdfcer-render` loader, R21), but it does have every font's *layout
/// metrics*, from dictionary data alone: advance widths (§9.6.2.1
/// `/Widths`, §9.7.4.3 `/W`/`/DW`, or the compiled-in standard-14 AFM
/// tables) and vertical extent (§9.8 Table 122 `/Ascent`/`/Descent`). The
/// walk lays the run out with exactly the arithmetic §9.4.4 specifies —
/// per-code advances through the shared
/// [`advance_tx`](crate::text_extract::font) formula, `Tc`/`Tw`/`Tz`
/// applied, `TJ` offsets applied to the text matrix, every glyph's box
/// mapped through `Trm = [Tfs·Th 0 0 Tfs 0 Ts] × Tm × CTM` — and unions the
/// result. That is where a conforming reader puts the text, so the box is
/// where the text is.
///
/// [`bounds_basis`](Self::bounds_basis) says which of the four cases
/// produced this particular box, and [`approximate`](Self::approximate)
/// stays `true` in all of them: a metrics-derived box is still not measured
/// ink (accents exceed `/Ascent`, italics overhang the advance), which is a
/// smaller and more specific gap than the em-box fallback's, but a gap.
///
/// **Not part of the file.** This bbox is read-only descriptive data
/// consumed for selection, hit-testing and marquee; it is never written
/// back, so improving its accuracy has no interaction with the
/// round-trip/minimal-diff invariant (project rule 3).
///
/// **The preview and font are identity, not content.** They exist so an
/// object row can say `Text · "Section A-A" · Helvetica 10` instead of
/// `Text` three times over (ui-spec §B.4 #1); they are capped, they carry no
/// positions, and they are not a text-extraction result. Anything that needs
/// the page's actual text — with provenance, derived spacing and reading
/// order — calls [`crate::text_extract::extract_page`].
#[derive(Debug, Clone, PartialEq)]
pub struct TextObject {
    /// Page-space bounds (module docs), built per
    /// [`bounds_basis`](Self::bounds_basis).
    pub page_bbox: Bounds,
    /// The per-show-operator boxes whose union is [`page_bbox`](Self::page_bbox).
    ///
    /// # Why this exists, and what it fixes
    ///
    /// `page_bbox` is one rectangle enclosing an entire `BT`…`ET`. That is
    /// the right shape for "where is this object" and the WRONG shape for
    /// "did the operator click on it": a producer is free to put every label
    /// on a drawing inside one `BT`…`ET`, and then the enclosing rectangle
    /// spans the whole sheet while the ink covers almost none of it.
    ///
    /// Measured on a real SolidWorks export: one text object carrying every
    /// dimension label, `page_bbox` = 23,14 → 1564,1216 — the whole drawing
    /// — painted near the top of paint order. Hit-testing that rectangle
    /// made it the front-most hit for EVERY click on the page; at one point
    /// on a real line it beat 57 genuine objects underneath it. The operator
    /// experienced it as "clicking does nothing" and "sometimes I get a box
    /// that doesn't correspond to anything".
    ///
    /// So selection tests these instead. Each entry is one show operator's
    /// laid-out extent, so the gaps BETWEEN runs are correctly not part of
    /// the object.
    ///
    /// Empty when no run could be laid out (no resolvable font, an unusable
    /// `Tf`), in which case a consumer falls back to `page_bbox` — the
    /// previous behaviour, which is the honest answer when nothing finer is
    /// known. Also empty past [`MAX_TEXT_RUNS`], where retaining more costs
    /// more than the precision is worth.
    /// **★ `Pass 32.0` substrate:** each entry is now a [`TextRun`] rather
    /// than a bare `Bounds`. The geometry is unchanged and reachable as
    /// [`TextRun::bounds`] (or in bulk via [`Self::run_bounds`]); what is
    /// added is the run's own **byte span** and whether its position is
    /// **inherited** from the previous run's advance.
    ///
    /// Without those two, *"delete this run"* is not expressible against
    /// this model at all — exactly as `move_subpath` was not expressible
    /// before `Pass 28.0` gave [`Subpath`] its span. The span is the
    /// enabling change; the verb is downstream of it.
    pub runs: Vec<TextRun>,
    /// Always `true` — no variant of the bbox is measured glyph ink.
    ///
    /// Kept as a `bool` rather than folded into
    /// [`bounds_basis`](Self::bounds_basis) because it answers a different
    /// question: *"should this box be drawn as an approximation?"* (yes,
    /// always, today) versus *"how good an approximation is it?"*. A future
    /// exact-outline path would flip this one to `false`.
    pub approximate: bool,
    /// Which of the four constructions produced [`page_bbox`](Self::page_bbox).
    pub bounds_basis: TextBoundsBasis,
    /// What the object's shown strings decoded to (or why they did not).
    pub preview: TextPreview,
    /// The font in effect at the first show operator, if there was one.
    pub font: Option<TextFont>,
    /// The `BT`→`ET` token range.
    pub tokens: TokenRange,
    /// The equivalent byte span.
    pub bytes: ByteSpan,
    /// The effective CTM captured at the object's `BT` (`Pass 113.0`).
    ///
    /// The other two object kinds have carried this since decomposition
    /// existed ([`PathObject::ctm`], [`ImageObject::ctm`]); text did not,
    /// because every verb that needed a CTM was a path verb and text's
    /// placement is expressed through `Tm` rather than through operands a
    /// move would rewrite.
    ///
    /// **A page-space transform needs it for every kind.** `cm` composes into
    /// the CTM in force at that point in the stream, so expressing a
    /// page-space matrix as a local one requires the object's own CTM
    /// (`crate::vector::plan_transform_many` derives `CTM x M x CTM-inverse`
    /// from it). Without this field a text object would have had to be
    /// transformed as if its CTM were the identity — correct on a page a
    /// producer never scaled, and silently wrong at a slant everywhere else.
    ///
    /// Note what it is NOT: this is the CTM, not `Tm`. The text matrix is
    /// per-show-operator and lives in [`TextRun`]; this is the graphics-state
    /// transform the whole `BT`...`ET` is drawn under.
    pub ctm: Matrix,
}

impl TextObject {
    /// The decoded characters of run `index`, or `None` when this object
    /// has no such run or its text is not readable.
    ///
    /// # Why `None` covers three different situations, and why that is right
    ///
    /// `None` is returned when the index is out of range, when the preview
    /// is [`TextPreview::Unavailable`] / [`TextPreview::Undecodable`] /
    /// [`TextPreview::Empty`], and when the run opened after the preview hit
    /// [`MAX_TEXT_PREVIEW_CHARS`]. Callers that need to TELL those apart —
    /// a GUI readout wanting to say *"no font resolver"* rather than *"no
    /// text"* — read [`Self::preview`] directly, which is why every one of
    /// those distinctions is preserved there. What this accessor promises
    /// is narrower and is the promise its callers actually need: **the text
    /// it returns is this run's text, or it returns nothing.** It never
    /// returns the object's whole string as a stand-in for one run, which
    /// is the failure a DXF export would otherwise ship as 237 dimension
    /// labels stacked at a single insertion point.
    ///
    /// # Empty string vs `None`
    ///
    /// A run whose show operand was `()` — legal, and emitted by real
    /// producers as a positioning no-op — yields `Some("")`. That is a
    /// deliberate distinction from `None`: the run exists and is readable,
    /// it just says nothing, and a caller deciding whether to emit a DXF
    /// `TEXT` entity needs "readable but empty" to mean *skip*, not
    /// *unreadable*.
    #[must_use]
    pub fn run_text(&self, index: usize) -> Option<&str> {
        let run = self.runs.get(index)?;
        let TextPreview::Decoded { text, .. } = &self.preview else {
            return None;
        };
        // `get` rather than an index: the range is clamped at construction,
        // but a char-boundary miss would still panic on a slice, and a CJK
        // preview is exactly where that lands. `get` returns `None` on a
        // non-boundary offset instead — a label that cannot be read, not a
        // crash mid-export.
        text.get(run.text_range())
    }
}

/// How a show operator's origin was established — the text-side analogue of
/// a subpath's start being explicit or reused (`Pass 30.0`).
///
/// # Why a deletion verb cannot proceed without this
///
/// §9.4.2: a show operator leaves the text matrix advanced past the string
/// it drew, and the **next** show operator starts from wherever the pen was
/// left unless a positioning operator (`Td`, `TD`, `Tm`, `T*`, or the line
/// move built into `'` and `"`) moves it first.
///
/// So a run with no positioning operator between it and its predecessor has
/// **no coordinates of its own**. Delete the predecessor and this run does
/// not stay where it was drawn — it slides back to wherever the pen now
/// ends up. That is invisible in the saved bytes (the file is well-formed
/// and round-trips) and visible only when someone looks at the page, which
/// is the worst combination a text edit can have.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunPositioning {
    /// A positioning operator set this run's origin after the previous run
    /// ended, so the run stands on its own coordinates. Deleting anything
    /// before it cannot move it.
    ///
    /// The **first** run of a `BT`…`ET` is always this: `BT` resets both the
    /// text and line matrices to the identity (§9.4.1), which is an origin
    /// of its own even when no `Tm` follows.
    Explicit,
    /// The run inherits its origin from the previous run's **advance**. It
    /// has no coordinates anywhere in the file, and deleting its predecessor
    /// moves it.
    Inherited,
}

/// One show operator inside a `BT`…`ET`: where it drew, which bytes drew it,
/// and whether it owns its own position.
///
/// # A `TJ` array is ONE run, deliberately
///
/// Its numeric elements are kerning *within* a single positioned string, not
/// separate placements, so splitting on them would fragment a word into
/// per-glyph boxes for no gain — and would make "delete this run" mean
/// something no operator asked for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextRun {
    /// The laid-out page-space extent of this one show operator.
    pub bounds: Bounds,
    /// The operation's token range — its operands through its operator.
    pub tokens: TokenRange,
    /// The equivalent byte span in the decoded content buffer. **This is
    /// what a per-run edit rewrites**, and the reason this struct exists.
    pub bytes: ByteSpan,
    /// Whether deleting what precedes this run would move it.
    pub positioned_by: RunPositioning,
    /// Byte range of this run's decoded text within the enclosing
    /// [`TextObject::preview`] — read it through
    /// [`TextObject::run_text`], which handles the truncation case.
    ///
    /// # Why a RANGE and not a `String`
    ///
    /// The preview is accumulated once, character by character, as the
    /// walker decodes each show operator (§9.10.2's mapping ladder). A
    /// per-run `String` would be a second copy of the same bytes on every
    /// run, and on the measured CAD export — one text object holding all
    /// 237 dimension labels — that is 237 allocations for text the object
    /// already stores contiguously.
    ///
    /// # What needed it
    ///
    /// `TextObject::preview` decodes a whole `BT`…`ET` into ONE running
    /// string with no boundary retained, so *"the text of run 6"* was not
    /// expressible. That blocked two things at once: naming the selected
    /// run in the GUI's rung readout (`pdfcer-ui-specialist` filed it as
    /// owed), and emitting one DXF `TEXT` entity per label instead of one
    /// carrying all 237 concatenated at a single point.
    ///
    /// Stored as a start/end PAIR rather than a `Range<usize>` because
    /// `Range` is not `Copy`, and `TextRun` is copied per candidate on
    /// every hit-test pass. Read it through [`Self::text_range`].
    pub text_start: usize,
    /// End of [`Self::text_start`]'s range, exclusive.
    pub text_end: usize,
}

impl TextRun {
    /// This run's byte range within the enclosing
    /// [`TextObject::preview`] — see [`Self::text_start`].
    ///
    /// Prefer [`TextObject::run_text`], which resolves the range against
    /// the right preview and handles the unreadable cases; this is for
    /// callers that already hold the string.
    #[must_use]
    pub fn text_range(&self) -> core::ops::Range<usize> {
        self.text_start..self.text_end
    }
}
/// A path's paint colour **as the file states it**, including the case pdfcer
/// cannot decode here.
///
/// # Why this replaced a bare `Rgb`
///
/// The decomposer's graphics-state tracker handled only the DEVICE colour
/// operators — `g G rg RG k K` — and had no arm for `cs CS sc scn SC SCN`.
/// A path painted in a `/Separation`, `/DeviceN`, `/ICCBased`, `/Indexed`,
/// `/Lab` or `/Pattern` space therefore inherited whatever the last device
/// operator had set: a **stale, silently wrong colour**, with no value meaning
/// "I do not know".
///
/// ★ That is exactly the shape [`crate::text_edit::FillState`] already solved
/// for TEXT — `Default` / `Device` / `Other`, with the raw operator bytes kept
/// so an undecodable colour can be restored verbatim. One half of this crate
/// had the honest model and the other half had a lossy one, and nothing
/// connected them. This is that model, applied to paths.
///
/// # Why `Other` does not resolve the colour
///
/// Resolving a `/Separation` to a screen colour means evaluating its tint
/// transform, and that machinery lives in `pdfcer-render`, which `pdfcer-core`
/// cannot depend on. Duplicating it here would create a SECOND colour-space
/// implementation — the very class of defect this type exists to remove.
///
/// It is also not needed for the job. A shell asking "may I recolour this?"
/// needs to know the paint is a named spot ink, not what that ink looks like;
/// answering *"this stroke is spot ink `PANTONE 185 C`"* is more useful than a
/// swatch, and infinitely more useful than a wrong swatch.
#[derive(Debug, Clone, PartialEq)]
pub enum PathPaint {
    /// §8.6.8's initial value: `DeviceGray` 0, black. No operator set it.
    ///
    /// Distinct from `Device { … }` holding black, because "nobody chose a
    /// colour" and "somebody chose black" are different facts about the file,
    /// and only the first may be silently replaced.
    Default,
    /// A device colour pdfcer fully models, with its resolved sRGB.
    Device {
        /// Which device operator family set it.
        space: DevicePaintSpace,
        /// The components as written (`0.0..=1.0`, §8.6.4).
        comps: Vec<f64>,
        /// The resolved screen colour.
        rgb: Rgb,
    },
    /// A colour space pdfcer does not decode here. The components are kept
    /// because they are what the file said; the space NAME is kept because it
    /// is what a shell must show and what a refusal must cite.
    Other {
        /// The resource name from `cs`/`CS` — e.g. `CS0` — or `None` when the
        /// operator named an inline space.
        space: Option<Vec<u8>>,
        /// The operands of `sc`/`scn`/`SC`/`SCN`, as written. A `/Separation`
        /// has one (its tint); a `/DeviceN` has one per colorant.
        comps: Vec<f64>,
        /// True when the operator was `scn`/`SCN` **with a name operand** —
        /// i.e. a pattern (§8.7.3). A pattern has no colour at all, which is
        /// a different refusal from "a colour pdfcer cannot decode".
        pattern: bool,
    },
}

/// Which device operator family set a [`PathPaint::Device`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DevicePaintSpace {
    /// `g` / `G`.
    Gray,
    /// `rg` / `RG`.
    Rgb,
    /// `k` / `K`.
    Cmyk,
}

impl PathPaint {
    /// The screen colour, when pdfcer has one.
    ///
    /// `None` for [`Self::Other`] — and callers are expected to SHOW that
    /// rather than substitute black. A control that opens on black for a spot
    /// ink is a control that discards the ink the moment it is touched.
    #[must_use]
    pub fn rgb(&self) -> Option<Rgb> {
        match self {
            Self::Default => Some(Rgb::BLACK),
            Self::Device { rgb, .. } => Some(*rgb),
            Self::Other { .. } => None,
        }
    }

    /// Whether this paint is in a space pdfcer cannot decode here.
    #[must_use]
    pub const fn is_other(&self) -> bool {
        matches!(self, Self::Other { .. })
    }
}

/// A selectable object on a page — the unit the GUI target provider hands
/// back as a hit and the snapping engine (12.M1) consumes.
#[derive(Debug, Clone, PartialEq)]
pub enum VectorObject {
    /// A path object (node-editable in a later Pass).
    Path(PathObject),
    /// A text object (selectable, not node-editable).
    Text(TextObject),
    /// An image or form object (selectable, not node-editable).
    Image(ImageObject),
}

impl VectorObject {
    /// The object's page-space bounding box — the marquee-enclosure and
    /// hit-test pre-filter input for every object kind.
    #[must_use]
    pub fn page_bbox(&self) -> Bounds {
        match self {
            VectorObject::Path(p) => p.page_bbox,
            VectorObject::Text(t) => t.page_bbox,
            VectorObject::Image(i) => i.page_bbox,
        }
    }

    /// The object's defining-operator token range (the future editing
    /// handle for a path; the move/delete handle for text/image).
    #[must_use]
    pub fn tokens(&self) -> TokenRange {
        match self {
            VectorObject::Path(p) => p.tokens,
            VectorObject::Text(t) => t.tokens,
            VectorObject::Image(i) => i.tokens,
        }
    }

    /// The object's byte span in the decoded content buffer.
    #[must_use]
    pub fn bytes(&self) -> ByteSpan {
        match self {
            VectorObject::Path(p) => p.bytes,
            VectorObject::Text(t) => t.bytes,
            VectorObject::Image(i) => i.bytes,
        }
    }
}

/// Structural oddities tolerated during decomposition, counted rather than
/// silently absorbed — the object-model twin of the renderer's
/// [`crate::content`] diagnostics. Every count answers a "how honest is
/// this decomposition?" question.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct DecomposeDiagnostics {
    /// Path objects emitted.
    pub paths: usize,
    /// Text objects emitted.
    pub text: usize,
    /// Image objects (inline + image XObject) emitted.
    pub images: usize,
    /// Form objects emitted (`Do` on a form XObject).
    pub forms: usize,
    /// `cm` seen mid-path-construction (the geometry is approximated with
    /// the first captured CTM, exactly as the renderer does).
    pub midpath_cm: usize,
    /// Unbalanced `Q` (empty graphics-state stack) tolerated.
    pub unbalanced_q: usize,
    /// A segment operator (`l`/`c`/`v`/`y`) with no current point — a
    /// §8.5.2.1 error, skipped.
    pub segment_without_current: usize,
    /// A `Do`/inline-image whose XObject could not be classified (no
    /// resolver, or an unresolvable name) — no object emitted.
    pub unresolved_xobject: usize,
    /// Construction operators dropped because [`MAX_NODES`] was hit.
    pub nodes_dropped: usize,
    /// Objects dropped because [`MAX_OBJECTS`] was hit.
    pub objects_dropped: usize,
    /// Form XObjects not descended into because the nesting chain had already
    /// reached [`crate::content::MAX_FORM_DEPTH`].
    ///
    /// Non-zero means the leaf list is INCOMPLETE, and a caller that presents
    /// it as "everything on the page" would be wrong. Counted rather than
    /// silently truncated for that reason.
    pub form_depth_overflows: usize,
    /// Form XObjects not descended into because the form was already on the
    /// current chain — a cycle (§8.10.1 does not forbid one).
    ///
    /// Keyed on the form's **object number**: the same stream is reachable
    /// under different resource names, so a name-keyed guard would miss it.
    pub form_cycles: usize,
    /// Paths whose fill or stroke was set in a colour space this decomposition
    /// does not decode — `/Separation`, `/DeviceN`, `/ICCBased`, `/Indexed`,
    /// `/Lab` or a pattern (`Pass 218.0`).
    ///
    /// ★ THE DISCLOSURE HALF, AND IT WAS ENTIRELY ABSENT. Before this Pass the
    /// decomposer silently kept whatever colour the last DEVICE operator had
    /// set, so a spot-coloured path was reported with an unrelated colour and
    /// nothing anywhere counted it. This struct had twelve counters and not one
    /// of them could report unmodelled graphics state.
    ///
    /// A non-zero value means [`PathObject::fill_color`] / `stroke_color` are
    /// best-effort for those objects and [`PathObject::fill_paint`] /
    /// `stroke_paint` are the honest answer.
    pub paths_with_undecoded_colour: usize,
    /// Paths that cannot be seen because an `/ExtGState` set their alpha to
    /// zero, yet are reported as ordinary painted objects (`Pass 220.0`).
    ///
    /// ★ A wrong CLAIM rather than a wrong number, which is why it is counted
    /// and not corrected: the object genuinely is in the content stream and a
    /// shell may legitimately want to select it. What was missing was any way
    /// to KNOW -- an operator who clicks apparently empty space and selects
    /// something has no explanation available, and this is it.
    pub paths_invisible_by_alpha: usize,
    /// `sh` shading operators encountered, which this model produces NO
    /// object for (`Pass 221.0`).
    ///
    /// ★ A MISSING OBJECT, not a wrong one — and that is why it is counted
    /// rather than fixed. An operator who cannot select a visible gradient has
    /// no way to tell "pdfcer does not model this" from "I missed it", and the
    /// renderer paints it, so the canvas and the object list disagree with
    /// nothing explaining why.
    ///
    /// Modelling a shading as a selectable object is a real feature with its
    /// own geometry question (a `sh` fills the current clip, which this model
    /// does not track). Counting it is the honest interim.
    pub shadings_unmodelled: usize,
    /// Marked-content sections tagged `/OC` — optional content, i.e. LAYERS
    /// (`Pass 221.0`).
    ///
    /// ★ The model does not resolve layer visibility, so content on a layer
    /// the document turns OFF is listed and selectable while the renderer
    /// correctly does not draw it. The two disagree, and before this counter
    /// nothing said so.
    ///
    /// Counted rather than filtered because filtering needs the catalog's
    /// `/OCProperties` default configuration, which this walk does not have —
    /// and because it was MEASURED as rare: 0.6% of a 500-file corpus sample
    /// carry optional content at all, and **none** had a layer switched off by
    /// default. A shell that sees a non-zero count here can warn; a shell that
    /// sees zero — which is almost every file — needs nothing.
    pub oc_sections: usize,
}

/// One object reached by **descending into a form XObject** — the unit a hit
/// test answers with, and the thing a page-sized form was hiding.
///
/// # ★★★ WHY THIS TYPE EXISTS
///
/// [`decompose_page`] emits a form XObject as **one opaque object** bounded by
/// its `/BBox`, and never enters it. On a page whose visible body is wrapped
/// in a form — which is what SolidWorks emits per orthographic view, and what
/// a great many print files emit per panel — that means:
///
/// * the form is a page-sized object sitting in paint order **above**
///   everything drawn before it, and
/// * a hit test answers every click, anywhere, with the form.
///
/// The operator's report, relayed from the GUI project: *"when I click on one
/// of the objects all I get is the page selected."* He was selecting a real
/// object. It was a form.
///
/// Measured on one print-conformance page: **sixteen** ~20 × 20 pt forms, one
/// per blend-mode cell, each swallowing every click aimed at the swatch inside
/// it. Acrobat, on the same file, selects the individual path — its
/// editable-item model cannot return a form wrapper at all.
///
/// # ★★ WHY THESE ARE A SEPARATE LIST AND NOT MIXED INTO `objects`
///
/// **Because a leaf's token range indexes a different buffer**, and eleven
/// call sites in `edit.rs` resolve a paint-order index and apply surgery to
/// the *page's* content stream. Put leaves in [`PageObjects::objects`] and
/// every one of those verbs would happily apply a form-relative token range to
/// the page and corrupt it — silently, because the range is in bounds.
///
/// Keeping them out of that list makes those eleven sites correct **by
/// construction** rather than by a guard somebody has to remember to add to
/// each. It also means a caller's stored paint-order indices do not move,
/// which the GUI had budgeted to absorb and now does not have to.
#[derive(Debug, Clone, PartialEq)]
pub struct FormLeaf {
    /// The object, with its geometry already mapped into **page space** — the
    /// same space [`PageObjects::objects`] uses, so a caller can hit-test both
    /// lists against one point without transforming anything.
    pub object: VectorObject,
    /// The chain of enclosing form XObjects, **outermost first**, ending with
    /// the form this object is directly inside.
    ///
    /// Never empty: an object with no enclosing form is not a leaf, it is an
    /// ordinary entry in [`PageObjects::objects`].
    ///
    /// This is what lets a shell say *"inside Title block (form)"* and offer
    /// "select the container" as a distinct act. Without it the operator gains
    /// reach and loses all sense of structure, which is a different kind of
    /// lost.
    pub containment: Vec<ObjId>,
    /// The index, in [`PageObjects::objects`], of the **outermost** form this
    /// object is inside — i.e. where on the page's own paint order this leaf
    /// was drawn.
    ///
    /// # ★ Why a hit test cannot be correct without it
    ///
    /// Leaves and page-stream objects are two lists, but they are **one paint
    /// order**: a form's contents are painted exactly where its `Do` sits
    /// among the page's other objects. Something drawn on the page *after* a
    /// form is on top of everything inside that form, and something drawn
    /// before it is underneath.
    ///
    /// Without this field the only orderings available are "all leaves first"
    /// or "all leaves last", and both are wrong for any page that draws
    /// anything outside its forms. With it, a caller — or
    /// [`super::hit_test_point_deep`] — can interleave the two lists into the
    /// order the renderer actually painted them.
    pub paint_order: usize,
    /// **The matrix that placed this leaf's directly-enclosing form on the
    /// page** — the CTM in force at its `Do`, already composed with the form's
    /// `/Matrix` and with every enclosing form's placement (`Pass 188.0`).
    ///
    /// # Why the surgery cannot work without it
    ///
    /// A leaf's geometry above is **page space**; its bytes live in the form's
    /// own stream, which is **form space**. Editing means decomposing that
    /// stream and planning against it, and the planners convert a page-space
    /// target into stream space by inverting the object's `ctm`. That `ctm` is
    /// only page-space if the decomposition of the form's stream *starts* from
    /// this matrix. Pass [`Matrix::IDENTITY`] instead and every drag lands in
    /// the wrong place by exactly the form's placement — silently, and
    /// invisibly on the common case where a form is placed at the origin with
    /// no scale.
    ///
    /// # ★ It is per INVOCATION, not per form
    ///
    /// A form legally appears more than once on a page (§8.10.1 names CAD
    /// output as its own illustration), and each `Do` has its own CTM. Two
    /// leaves can therefore name the same form object and carry different
    /// placements. That is not a wrinkle to be smoothed over — it is why an
    /// edit must be addressed by *which leaf the operator clicked* rather than
    /// by *which form it is in*.
    pub placement: Matrix,
    /// The index of this object in its directly-enclosing form's **own**
    /// decomposition (`Pass 188.0`).
    ///
    /// This is what a geometry verb addresses once it is inside the form: the
    /// operand `object_index` means, for a page, an index into
    /// [`PageObjects::objects`], and for a leaf it means this.
    ///
    /// **Not derivable from the leaf's position in [`PageObjects::leaves`].** A
    /// form that contains a nested form contributes its own children to the
    /// leaf list and then, from the recursion, the nested form's children too;
    /// the two interleave, and the nested ones belong to a different stream
    /// entirely. Recomputing this later would be a second walk that has to
    /// agree with the first in every detail, which is the shape of a bug that
    /// only appears on nested CAD output.
    pub form_object_index: usize,
}

impl FormLeaf {
    /// The form this object is **directly** inside.
    #[must_use]
    pub fn parent(&self) -> Option<ObjId> {
        self.containment.last().copied()
    }

    /// Which content stream this leaf's [`VectorObject::tokens`] range indexes.
    ///
    /// # ★ Read this before doing anything with `tokens()`
    ///
    /// A form XObject's decoded bytes are **a different buffer** from the
    /// page's (§8.10.1 — it is its own content stream). A token range from
    /// inside a form is meaningless against the page's stream, and *in range*,
    /// which is the dangerous combination.
    ///
    /// Deliberately the **same type** `text_extract` uses for the same fact
    /// about a [`crate::text_extract::TextRun`], so a form-interior path and a
    /// form-interior text run describe themselves identically. A shell has to
    /// reconcile both models in one selection; two vocabularies for one fact
    /// would be its problem and our fault.
    #[must_use]
    pub fn stream(&self) -> crate::text_extract::ContentStreamRef {
        self.parent()
            .map_or(crate::text_extract::ContentStreamRef::Page, |id| {
                crate::text_extract::ContentStreamRef::Form { object: id.num }
            })
    }

    /// Whether this leaf can be edited — **true for a path, false otherwise**
    /// (`Pass 188.0`).
    ///
    /// # It was a hard `false`, and the note it carried has been honoured
    ///
    /// The previous body was `false` with a comment saying it was *"a method
    /// rather than a constant so that the answer has somewhere to change when
    /// editing-through-recursion is built"*. It is built: the geometry verbs
    /// have form-scoped twins —
    /// [`move_node_in_form`](crate::edit::EditSession::move_node_in_form),
    /// [`move_nodes_in_form`](crate::edit::EditSession::move_nodes_in_form),
    /// [`move_handle_in_form`](crate::edit::EditSession::move_handle_in_form),
    /// [`move_subpath_in_form`](crate::edit::EditSession::move_subpath_in_form),
    /// [`move_objects_in_form`](crate::edit::EditSession::move_objects_in_form)
    /// and
    /// [`delete_objects_in_form`](crate::edit::EditSession::delete_objects_in_form)
    /// — each addressed by this leaf's index in
    /// [`PageObjects::leaves`].
    ///
    /// `pdfcer-gui` asked for exactly this: *"so that our guards can ask YOUR
    /// predicate instead of our proxy for it. We currently test
    /// `page_object_index().is_none()`, which is a structural stand-in for a
    /// question only you can answer."* Ask this one.
    ///
    /// # ★ What `false` means now, and it is a different fact
    ///
    /// It no longer means *"nothing inside a form can be edited"*. It means
    /// **this particular leaf is not a path**, so there is no node, handle or
    /// subpath to drag. A text run inside a form is edited through
    /// [`EditSession::edit_text`](crate::edit::EditSession::edit_text) with
    /// [`EditTarget::Form`](crate::text_edit::EditTarget), which has worked
    /// since `Pass 119.0`; an image inside a form has no geometry of its own
    /// to grab.
    ///
    /// So a shell that greys out a node handle on `false` is still right, and
    /// a shell that greys out *the whole container* on `false` is now wrong.
    ///
    /// # It says nothing about whether the edit is LOCAL
    ///
    /// Editing a shared form changes every place it is drawn. That is a
    /// disclosure, not a permission — see
    /// [`FormSurgeryOutcome::invocations`](crate::edit::FormSurgeryOutcome::invocations)
    /// and decision 076. This predicate deliberately does not fold the two
    /// questions together: *"can I drag this"* and *"how many sheets will
    /// change"* have different answers and need different words in front of
    /// the operator.
    #[must_use]
    pub const fn is_editable(&self) -> bool {
        matches!(self.object, VectorObject::Path(_))
    }
}

/// The decomposition of one page (or one content stream): the ordered
/// object list plus its diagnostics.
///
/// Objects are in **paint order** — the order the renderer would paint
/// them, so the LAST object at a point is the topmost. [`page_bbox`] and
/// hit-testing rely on this ordering.
#[derive(Debug, Clone, PartialEq)]
pub struct PageObjects {
    /// The objects, in paint order.
    pub objects: Vec<VectorObject>,
    /// The initial CTM the stream was decomposed under (identity for a
    /// page — page-space geometry is then genuine PDF user space).
    pub initial: Matrix,
    /// Tolerated-oddity counts (module docs).
    pub diagnostics: DecomposeDiagnostics,
    /// Objects reached by descending **into** the form XObjects in
    /// [`Self::objects`] — see [`FormLeaf`].
    ///
    /// # ★ Empty unless the walk had a document to descend with
    ///
    /// Populated by [`decompose_page`], which has a [`DocumentView`]. The
    /// resolver-only entry points ([`decompose`], [`decompose_with_fonts`])
    /// leave it empty, because descending needs the form's *content stream*
    /// and the classification seam deliberately exposes only its shape — that
    /// is what lets the fuzz target and the unit tests drive the walk with no
    /// document at all.
    ///
    /// So an empty `leaves` means *"nobody looked"*, not *"nothing there"*,
    /// and the two are distinguished by which entry point was called rather
    /// than by a flag.
    pub leaves: Vec<FormLeaf>,
}

impl PageObjects {
    /// The union of every object's page bbox — the page's drawn extent in
    /// page space (empty if the page has no vector content).
    #[must_use]
    pub fn page_bbox(&self) -> Bounds {
        self.objects
            .iter()
            .fold(Bounds::EMPTY, |acc, o| acc.union(o.page_bbox()))
    }
}

// ---------------------------------------------------------------------------
// XObject classification seam (keeps `decompose` testable without a Document)
// ---------------------------------------------------------------------------

/// The classification of an XObject named by a `Do` operator, enough to
/// bound it without recursing (§8.8 Table 87).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum XObjectShape {
    /// An image XObject (§8.9): bounded by the user-space unit square under
    /// the current CTM.
    Image {
        /// `(/Width, /Height)` in samples (§8.9.5, Table 89), or `None` if
        /// the dictionary does not carry a usable pair — see
        /// [`ImageObject::pixel_size`], which this becomes.
        pixel_size: Option<(u32, u32)>,
        /// The XObject stream's identity, when the `Do` name resolved to an
        /// indirect reference — see [`XObjectShape::Form::object`].
        object: Option<ObjId>,
    },
    /// A form XObject (§8.10): bounded by its `/BBox` (in form space) under
    /// `matrix × ctm`.
    Form {
        /// The form's `/BBox`, normalized, in form space.
        bbox: Bounds,
        /// The form's `/Matrix` (default identity).
        matrix: Matrix,
        /// The form stream's identity, when the `Do` name resolved to an
        /// indirect reference.
        ///
        /// # ★★ Why this is here, and why it is keyed on the OBJECT
        ///
        /// Two things need it and neither can be done from the resource name:
        ///
        /// 1. **A cycle guard.** §8.10.1 does not forbid a form invoking
        ///    itself, directly or through a chain. The same stream is
        ///    reachable under *different names* in different resource
        ///    dictionaries, so a name-keyed guard misses the cycle entirely —
        ///    the reason `text_extract`'s form walk keys on the object number
        ///    and says so at the site.
        /// 2. **Stream identity for anything inside the form.** A token range
        ///    from inside a form indexes the FORM's decoded bytes, which are
        ///    *a different buffer* from the page's. Naming the buffer is what
        ///    lets a caller tell an editable page-stream object from one it
        ///    can only read — the same distinction `ContentStreamRef` draws
        ///    for text.
        ///
        /// `None` when the `Do` name resolved to a direct stream object
        /// rather than a reference, which is legal and carries no identity to
        /// record.
        object: Option<ObjId>,
    },
}

/// The seam [`decompose`] uses to classify a `Do` operator's XObject.
///
/// Split out as a trait so the decomposition is testable with a stub (or
/// [`NoXObjects`]) and drivable by the fuzz target **without constructing a
/// [`DocumentView`]** — the heavy dependency only the `Do`-resolution path
/// needs. Real callers use [`DocumentXObjects`].
pub trait XObjectResolver {
    /// Classify the XObject named `name` in the current resource
    /// dictionary, or `None` if it cannot be resolved (absent, not a
    /// stream, no `/Subtype`).
    fn classify(&self, name: &[u8]) -> Option<XObjectShape>;
}

/// A resolver that classifies nothing — for content with no XObjects, and
/// for unit tests / the fuzz target that exercise the path/text walk
/// without a [`DocumentView`].
#[derive(Debug, Default, Clone, Copy)]
pub struct NoXObjects;

impl XObjectResolver for NoXObjects {
    fn classify(&self, _name: &[u8]) -> Option<XObjectShape> {
        None
    }
}

/// The production resolver: classifies a `Do` name against a page's
/// resolved `/XObject` subdictionary in a [`DocumentView`] (§7.8.3, §8.8).
///
/// Takes a view rather than a `&Document` (decision 018) so that an
/// XObject *created this session* — a dimension's or markup annotation's
/// form XObject, an image inserted by a future Pass — classifies as the
/// object model's caller sees it. A base-only resolver would decline to
/// classify it, and the object would silently vanish from selection and
/// snapping while remaining visible on the canvas: the two-views-disagree
/// failure decision 011 §Z2 warns against.
///
/// The field is named `view` rather than `doc` on purpose: the rename makes
/// every construction site a compile error rather than a silent
/// type-inference success, which is how the base-vs-session intent of each
/// one got audited when this changed.
pub struct DocumentXObjects<'a> {
    /// The document view, for resolving indirect `/XObject` entries against
    /// whichever revision the caller means.
    pub view: &'a DocumentView<'a>,
    /// The resource dictionary the `Do` name is looked up in.
    pub resources: &'a Dict,
}

impl XObjectResolver for DocumentXObjects<'_> {
    fn classify(&self, name: &[u8]) -> Option<XObjectShape> {
        let entry = self
            .resources
            .get(b"XObject")
            .map(|o| self.view.resolve(o))
            .and_then(Object::as_dict)?
            .get(name)?;
        // Captured BEFORE the resolve, because resolving is exactly what
        // discards it: one line later `entry` has become the stream it names.
        let object = entry.as_reference();
        let Object::Stream(stream) = self.view.resolve(entry) else {
            return None;
        };
        let subtype = stream
            .dict
            .get(b"Subtype")
            .map(|o| self.view.resolve(o))
            .and_then(Object::as_name)
            .map(|n| n.as_bytes());
        match subtype {
            Some(b"Image") => Some(XObjectShape::Image {
                pixel_size: dict_pixel_size(self.view, &stream.dict),
                object,
            }),
            Some(b"Form") => Some(XObjectShape::Form {
                bbox: dict_rect(self.view, &stream.dict, b"BBox").unwrap_or(Bounds::EMPTY),
                matrix: dict_matrix(self.view, &stream.dict).unwrap_or(Matrix::IDENTITY),
                object,
            }),
            // Structural inference for a malformed missing /Subtype, matching
            // the renderer's Width+Height ⇒ image, BBox ⇒ form heuristic.
            _ => {
                if stream.dict.contains_key(b"Width") && stream.dict.contains_key(b"Height") {
                    Some(XObjectShape::Image {
                        pixel_size: dict_pixel_size(self.view, &stream.dict),
                        object,
                    })
                } else if stream.dict.contains_key(b"BBox") {
                    Some(XObjectShape::Form {
                        bbox: dict_rect(self.view, &stream.dict, b"BBox").unwrap_or(Bounds::EMPTY),
                        matrix: dict_matrix(self.view, &stream.dict).unwrap_or(Matrix::IDENTITY),
                        object,
                    })
                } else {
                    None
                }
            }
        }
    }
}

/// Read a four-number rectangle entry, normalized per §7.9.5, as a
/// [`Bounds`] in the dictionary's own space.
fn dict_rect(view: &DocumentView<'_>, dict: &Dict, key: &[u8]) -> Option<Bounds> {
    let items = view.resolve(dict.get(key)?).as_array()?;
    let n: Vec<f64> = items
        .iter()
        .filter_map(|o| view.resolve(o).as_number())
        .collect();
    let [x0, y0, x1, y1] = <[f64; 4]>::try_from(n).ok()?;
    Some(Bounds {
        min: Point::new(x0.min(x1), y0.min(y1)),
        max: Point::new(x0.max(x1), y0.max(y1)),
    })
}

/// Read an image dictionary's `(/Width, /Height)` sample counts (§8.9.5,
/// Table 89 — both **required** integers), resolving indirect entries
/// through `view`.
///
/// `None` unless BOTH are present, integral and fit `u32`. Deliberately
/// strict, and deliberately all-or-nothing:
///
/// - A real entry is an integer. A `/Width 640.5` is malformed, and
///   rounding it would report a sample count no decoder would agree with.
/// - `/Width` and `/Height` are, in §8.9.5's own words about the resource
///   ceiling, attacker-controlled integers. A negative or `> u32::MAX`
///   value cannot be a sample count; reporting `Some` for it would put a
///   nonsense number in front of the operator.
/// - Reporting one axis without the other (`640×?`) is less useful than
///   reporting neither and saying so.
fn dict_pixel_size(view: &DocumentView<'_>, dict: &Dict) -> Option<(u32, u32)> {
    let read =
        |key: &[u8]| -> Option<u32> { u32::try_from(view.resolve(dict.get(key)?).as_int()?).ok() };
    Some((read(b"Width")?, read(b"Height")?))
}

/// The same read for an **inline** image's parameter dictionary (§8.9.7).
///
/// A separate function only because inline-image parameters are direct
/// objects (§8.9.7: the dictionary sits between `BI` and `ID` in the
/// content stream, so it has nowhere to hold an indirect reference), which
/// means there is no [`DocumentView`] to resolve through — and the
/// decomposition of an inline image must work without a document at all
/// (the [`NoXObjects`] / fuzz path). [`crate::content`] has already
/// normalized the Table 93 abbreviations `/W` and `/H` to `/Width` and
/// `/Height`, so this reads the same two keys as [`dict_pixel_size`].
fn inline_pixel_size(params: &Dict) -> Option<(u32, u32)> {
    let read = |key: &[u8]| -> Option<u32> { u32::try_from(params.get(key)?.as_int()?).ok() };
    Some((read(b"Width")?, read(b"Height")?))
}

/// Read a six-number `/Matrix` entry (Table 95) as a [`Matrix`].
fn dict_matrix(view: &DocumentView<'_>, dict: &Dict) -> Option<Matrix> {
    let items = view.resolve(dict.get(b"Matrix")?).as_array()?;
    let n: Vec<f64> = items.iter().filter_map(Object::as_number).collect();
    let [a, b, c, d, e, f] = <[f64; 6]>::try_from(n).ok()?;
    Some(Matrix::new(a, b, c, d, e, f))
}

// ---------------------------------------------------------------------------
// Font classification seam (the ONE decoder, reached without a Document)
// ---------------------------------------------------------------------------

/// The seam the decomposition uses to turn a `Tf` resource name into a
/// decoder for that font's show strings.
///
/// The exact twin of [`XObjectResolver`], and split out for the same three
/// reasons: the walk stays drivable with no [`DocumentView`] at all (unit
/// tests, the fuzz target), the *policy* of which revision a font is looked
/// up in belongs to the caller (decision 018 — a session view sees a font
/// added this session, a base view does not), and a caller that does not
/// care about text detail pays nothing.
///
/// The returned value is a [`crate::text_extract::ExtractFont`] — the
/// §9.10.2 ladder `extract-text` climbs — and **not** a bespoke encoding
/// table, so the object row and `extract-text` cannot disagree about what a
/// byte means (module docs' rule 1).
///
/// [`Arc`] rather than a borrow: one font resource is typically named by
/// many text objects on a page, and resolving a `/ToUnicode` CMap per
/// `Tf` would turn a linear walk quadratic. Implementations are expected to
/// cache — [`DocumentFonts`] does.
pub trait FontResolver {
    /// Resolve the font named `name` in the current resource dictionary,
    /// or `None` if it cannot be resolved (absent `/Font` dictionary, name
    /// not present, entry not a dictionary).
    fn resolve(&self, name: &[u8]) -> Option<Arc<ExtractFont>>;

    /// Resolve the **`/ExtGState`** named `name`, for the `gs` operator
    /// (§8.4.5, Table 58) — `Pass 220.0`.
    ///
    /// # Why this lives on the FONT resolver
    ///
    /// Because it needs exactly what that resolver already holds — the
    /// document view and the current resource dictionary — and because the
    /// most common thing a real `gs` sets is `/Font`. Measured over 300 files
    /// of a 4,023-file corpus: 11% carry an `/ExtGState`, and within those
    /// `/Font` is the most frequent entry by a wide margin (115 occurrences,
    /// against 59 `/CA`, 27 `/BM`, and **zero** `/LW`).
    ///
    /// A separate trait would have meant a fourth parameter on every
    /// `decompose*` entry point and its ~50 call sites, to carry a lookup
    /// against a dictionary this one already has.
    ///
    /// ★ DEFAULTED TO `None` deliberately. Every existing implementor —
    /// [`NoFonts`], and any a test or the fuzz target defines — keeps
    /// compiling and keeps answering "I resolve nothing", which is the honest
    /// answer for a resolver with no document behind it. Only
    /// [`DocumentFonts`] overrides it, and that is the one every real page
    /// decomposition uses, so the fix reaches production callers without a
    /// single call site changing.
    fn ext_gstate(&self, name: &[u8]) -> Option<ExtGStateParams> {
        let _ = name;
        None
    }
}

/// The entries of an `/ExtGState` that this model can act on (§8.4.5,
/// Table 58).
///
/// # Why only these four
///
/// Because they are the ones that change a value the model already exposes,
/// or a claim it already makes. Table 58 has more than twenty entries; the
/// rest set state nothing here reads, and inventing fields for them would be
/// modelling for its own sake.
///
/// ★ `/D` (dash), `/BM` (blend), `/SMask`, `/LC`, `/LJ`, `/ML`, `/RI`, `/OP`,
/// `/op`, `/OPM` are deliberately absent. An unread gap is not a defect, and
/// this file's clipboard sibling already states the principle: *"a fabricated
/// dash is worse than an absent one, because it looks deliberate."*
#[derive(Debug, Clone, Default, PartialEq)]
#[non_exhaustive]
pub struct ExtGStateParams {
    /// `/LW` — line width. Feeds `PathObject::line_width`, which the
    /// stroke-proximity hit test widens by; a stale one makes an operator
    /// click a visible line and select nothing.
    ///
    /// ★ Measured at ZERO occurrences in a 300-file sample. Handled anyway
    /// because it is free once `gs` is parsed at all, and because "rare" is
    /// not "never" for a format this old — but the effort was spent on it
    /// only after measuring that `/Font` is where the exposure actually is.
    pub line_width: Option<f64>,
    /// `/Font` — `[font size]`. The resource NAME and the size, exactly as
    /// `Tf` would have set them. THE COMMON CASE (115 of the corpus sample's
    /// ExtGState entries), and the one that matters: a stale size gives a
    /// text object the wrong bounding box, which is the same
    /// click-selects-nothing symptom one object kind over.
    pub font: Option<(Vec<u8>, f64)>,
    /// `/ca` — non-stroking alpha. Not a value this model exposes; carried so
    /// a fully transparent path can be COUNTED rather than reported as an
    /// ordinary painted object.
    pub fill_alpha: Option<f64>,
    /// `/CA` — stroking alpha. Same.
    pub stroke_alpha: Option<f64>,
}

/// A resolver that resolves nothing — the default, and what plain
/// [`decompose`] passes.
///
/// Every text object it produces carries [`TextPreview::Unavailable`],
/// which says *"no decoding was attempted"* rather than *"this object has
/// no text"*. The distinction is the whole point of having a named unit
/// struct here instead of an `Option`.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoFonts;

impl FontResolver for NoFonts {
    fn resolve(&self, _name: &[u8]) -> Option<Arc<ExtractFont>> {
        None
    }
}

/// The production font resolver: resolves a `Tf` name against a page's
/// `/Font` resource subdictionary (§7.8.3 Table 33, §9.6.2.1) in a
/// [`DocumentView`], memoizing each resolution.
///
/// ## Why the cache is not optional
///
/// [`ExtractFont::resolve`] parses a `/ToUnicode` CMap stream and builds a
/// 256-entry encoding table. A page that sets `/F1 10 Tf` inside every one
/// of a thousand `BT`/`ET` blocks — which is what a word processor emits —
/// would pay that a thousand times. The cache turns the walk back into one
/// resolution per distinct font resource per page.
///
/// [`RefCell`] because [`FontResolver::resolve`] takes `&self` (the walk
/// holds the resolver immutably, exactly as it holds
/// [`DocumentXObjects`]). The borrow is taken and released inside the
/// method with no reentrancy — `ExtractFont::resolve` cannot call back into
/// this — so it cannot panic. The consequence is that `DocumentFonts` is
/// not `Sync`; the decomposition is single-threaded per page and nothing
/// shares one across threads.
pub struct DocumentFonts<'a> {
    /// The document view fonts are resolved against (decision 018: pass a
    /// session view to see a font added this session, a base view for a
    /// one-shot CLI read).
    pub view: &'a DocumentView<'a>,
    /// The resource dictionary the `Tf` name is looked up in.
    pub resources: &'a Dict,
    /// Memoized resolutions, including negative ones (a name that is not in
    /// the dictionary must not be re-looked-up on every `Tf`).
    cache: RefCell<HashMap<Vec<u8>, Option<Arc<ExtractFont>>>>,
}

impl<'a> DocumentFonts<'a> {
    /// Build a resolver over `view`'s `resources`.
    #[must_use]
    pub fn new(view: &'a DocumentView<'a>, resources: &'a Dict) -> Self {
        Self {
            view,
            resources,
            cache: RefCell::new(HashMap::new()),
        }
    }

    /// The uncached lookup: `/Font` → `name` → a font dictionary →
    /// [`ExtractFont::resolve`].
    fn lookup(&self, name: &[u8]) -> Option<Arc<ExtractFont>> {
        let font_dict = self
            .resources
            .get(b"Font")
            .map(|o| self.view.resolve(o))
            .and_then(Object::as_dict)?
            .get(name)?;
        let dict = self.view.resolve(font_dict).as_dict()?;
        Some(Arc::new(ExtractFont::resolve(self.view, dict)))
    }
}

impl FontResolver for DocumentFonts<'_> {
    fn resolve(&self, name: &[u8]) -> Option<Arc<ExtractFont>> {
        if let Some(hit) = self.cache.borrow().get(name) {
            return hit.clone();
        }
        let resolved = self.lookup(name);
        self.cache
            .borrow_mut()
            .insert(name.to_vec(), resolved.clone());
        resolved
    }

    /// Read the four entries this model can act on out of `/ExtGState /name`.
    ///
    /// Deliberately NOT cached. The font cache exists because decoding a font
    /// program is expensive; this is four dictionary lookups, and a `gs`
    /// operator is rare enough (11% of files carry an `/ExtGState` at all)
    /// that a second cache would cost more in code than it saves in work.
    fn ext_gstate(&self, name: &[u8]) -> Option<ExtGStateParams> {
        let dict = self
            .resources
            .get(b"ExtGState")
            .map(|o| self.view.resolve(o))
            .and_then(Object::as_dict)?
            .get(name)
            .map(|o| self.view.resolve(o))
            .and_then(Object::as_dict)?
            .clone();

        let num = |k: &[u8]| {
            dict.get(k)
                .map(|o| self.view.resolve(o))
                .and_then(Object::as_number)
        };

        // Table 58 `/Font` is `[font size]` — an INDIRECT REFERENCE to a font
        // dictionary and a size, not a resource name. The model keys fonts by
        // resource name, so what is recoverable here is the SIZE; the name is
        // taken only when the array's first element happens to be a name,
        // which is not conformant but occurs.
        //
        // ★ The size alone is the half that matters: a text object's bounds
        // come from font metrics scaled by it, so a stale size is a wrong
        // bounding box whether or not the face changed.
        let font = dict
            .get(b"Font")
            .map(|o| self.view.resolve(o))
            .and_then(Object::as_array)
            .and_then(|a| {
                let size = a.get(1).map(|o| self.view.resolve(o))?.as_number()?;
                let face = a
                    .first()
                    .map(|o| self.view.resolve(o))
                    .and_then(|o| match o {
                        Object::Name(n) => Some(n.as_bytes().to_vec()),
                        _ => None,
                    })
                    .unwrap_or_default();
                Some((face, size))
            });

        Some(ExtGStateParams {
            line_width: num(b"LW"),
            font,
            fill_alpha: num(b"ca"),
            stroke_alpha: num(b"CA"),
        })
    }
}

// ---------------------------------------------------------------------------
// Entry points
// ---------------------------------------------------------------------------

/// Descend into every form XObject reachable from `objects`, collecting the
/// leaves in paint order.
///
/// # The two guards, and why one of them is not optional
///
/// * **Depth**, [`crate::content::MAX_FORM_DEPTH`] — a backstop against the
///   linear memory a legitimate-but-absurd chain pins.
/// * **★ Cycle, keyed on the form's OBJECT NUMBER** — this is the real
///   defence. §8.10.1 does not forbid a form invoking itself, directly or
///   through a chain, and **the same stream is reachable under different
///   resource names in different resource dictionaries**, so a name-keyed
///   guard misses the cycle entirely. The same reasoning, and the same key,
///   as `text_extract`'s form walk.
///
/// A `Do` naming a **direct** stream carries no object number and so cannot be
/// cycle-guarded. Such a form is descended into once at the current depth and
/// relies on the depth bound alone — which is sound, because a direct stream
/// cannot be referenced twice and therefore cannot form a cycle with itself.
///
/// # Geometry
///
/// The form object's own `ctm` is *already* `/Matrix × CTM-at-the-`Do``, so it
/// is exactly the form-space → page-space transform and is handed straight to
/// the nested walk as its initial matrix. Every leaf therefore comes back in
/// **page space**, and a caller can hit-test the flat list and the leaf list
/// against one point without transforming anything.
pub(crate) fn collect_form_leaves(
    view: &DocumentView<'_>,
    objects: &[VectorObject],
    path: &mut Vec<ObjId>,
    out: &mut Vec<FormLeaf>,
    diag: &mut DecomposeDiagnostics,
    root: Option<usize>,
) {
    for (index, obj) in objects.iter().enumerate() {
        // `root` is the OUTERMOST form's index in the page's own object list.
        // At the top level that is this object's own index; deeper down it is
        // whatever the top level already decided, carried unchanged -- a nested
        // form is painted where its outermost ancestor's `Do` sits, not
        // somewhere of its own.
        let root = root.unwrap_or(index);
        let VectorObject::Image(img) = obj else {
            continue;
        };
        if img.source != ImageSource::Form {
            continue;
        }
        let Some(id) = img.xobject else {
            // A direct stream: no identity to guard on, and none needed.
            continue;
        };
        if path.contains(&id) {
            diag.form_cycles += 1;
            continue;
        }
        if path.len() >= crate::content::MAX_FORM_DEPTH {
            diag.form_depth_overflows += 1;
            continue;
        }
        let Object::Stream(stream) = view.resolved(id) else {
            continue;
        };
        // `view.slice(span)`, not `span.slice(view.bytes())`: a form the
        // SESSION authored carries an R45 span starting past the end of the
        // base buffer, and only the view's stream source knows which of its
        // two halves such a span indexes. An unresolvable or undecodable form
        // is skipped, not fatal — it is already on the page as an opaque
        // object either way.
        let Some(content) = view
            .slice(stream.data_span)
            .and_then(|raw| crate::filters::decode_stream(&stream.dict, raw).ok())
            .and_then(|decoded| ContentStream::parse(decoded).ok())
        else {
            continue;
        };
        // §7.8.3: a form's own `/Resources` if it has one, otherwise the
        // invoking context's — which is what the outer walk already resolved,
        // and is why this is inherited rather than defaulted to empty.
        let resources = view
            .resolve(stream.dict.get(b"Resources").unwrap_or(&Object::Null))
            .as_dict()
            .cloned()
            .unwrap_or_default();

        let nested = {
            let xobjects = DocumentXObjects {
                view,
                resources: &resources,
            };
            let fonts = DocumentFonts::new(view, &resources);
            decompose_with_fonts(&content, img.ctm, &xobjects, &fonts)
        };

        path.push(id);
        for (form_object_index, child) in nested.objects.iter().enumerate() {
            // A nested form is a container, not a leaf: recursion below emits
            // what is inside it. Emitting the container here too would put a
            // second page-sized hit target into the very list built to stop
            // the first one winning every click.
            let is_form = matches!(
                child,
                VectorObject::Image(c) if c.source == ImageSource::Form
            );
            if !is_form {
                out.push(FormLeaf {
                    object: child.clone(),
                    containment: path.clone(),
                    // `img.ctm` is the CTM in force at this form's `Do`,
                    // already composed with `/Matrix` and with every enclosing
                    // form's placement — because the outer walk passed ITS
                    // placement in as `initial`. So this is page-space at every
                    // depth, which is the property the surgery needs.
                    placement: img.ctm,
                    // The index into THIS form's own decomposition, which is
                    // NOT derivable from a leaf's position in `out`: a form
                    // containing a nested form contributes children here and
                    // more children from the recursion below, and the two
                    // interleave. Recording it is one `enumerate`; recovering
                    // it later would be a second, subtly different walk.
                    form_object_index,
                    paint_order: root,
                });
            }
        }
        collect_form_leaves(view, &nested.objects, path, out, diag, Some(root));
        path.pop();
    }
}

/// Decompose a page's content into selectable vector objects.
///
/// The page's `Contents` streams are concatenated, decoded, and tokenized
/// via [`ContentStream::from_page`] (no bytes change — R46), then walked
/// with `initial` as the starting CTM. Pass [`Matrix::IDENTITY`] to get
/// page-space geometry in genuine PDF default user space (what the GUI
/// provider and the dimensioning subsystem expect).
///
/// ## Which revision gets decomposed (decision 018)
///
/// `view` decides, and the choice is operator-visible:
///
/// - `&session.view()` decomposes **the edited state**. This is what the
///   GUI's `ObjectModelProvider` passes, so a shape the operator just
///   moved is selectable where it now appears, and the dimensioning tool
///   snaps to the geometry actually on screen.
/// - `&doc.view()` decomposes **the base revision**. This is what a
///   one-shot CLI operation passes, and what
///   [`EditSession`](crate::edit::EditSession)'s own vector-surgery path
///   passes deliberately: its `object_index` values must line up with a
///   caller that indexed the base, which is why editing a page whose
///   content was already rewritten this session is *refused* rather than
///   silently misindexed.
///
/// # Errors
///
/// [`crate::content::ContentError`] if the content streams cannot be
/// decoded or tokenized (the same failure the renderer would hit).
///
/// # Examples
///
/// ```no_run
/// use pdfcer_core::document::Document;
/// use pdfcer_core::page_tree::pages;
/// use pdfcer_core::vector::{decompose_page, Matrix};
///
/// # fn demo(doc: &Document) -> Result<(), Box<dyn std::error::Error>> {
/// let page = &pages(doc)?[0];
/// let model = decompose_page(&doc.view(), page, Matrix::IDENTITY)?;
/// println!("{} selectable objects", model.objects.len());
/// # Ok(())
/// # }
/// ```
pub fn decompose_page(
    view: &DocumentView<'_>,
    page: &Page,
    initial: Matrix,
) -> Result<PageObjects, crate::content::ContentError> {
    let content = ContentStream::from_page(view, page)?;
    let xobjects = DocumentXObjects {
        view,
        resources: &page.resources,
    };
    // A page decomposition always resolves fonts: this is the entry point
    // the GUI provider and the CLI both call, and both surface the text
    // preview. The `NoFonts` path exists for callers that have no document
    // (unit tests, the fuzz target) and for `decompose`'s stable signature,
    // not as a cheaper mode a real caller should choose — `DocumentFonts`
    // memoizes, so the cost is one resolution per distinct font resource.
    let fonts = DocumentFonts::new(view, &page.resources);
    let mut model = decompose_with_fonts(&content, initial, &xobjects, &fonts);
    // ★ The descent happens HERE and not inside the walk, because it needs the
    // form's CONTENT STREAM and the walk only has the classification seam --
    // which is deliberate, and is what lets the fuzz target and the unit tests
    // drive the geometry with no document at all.
    let mut path = Vec::new();
    let mut leaves = Vec::new();
    collect_form_leaves(
        view,
        &model.objects,
        &mut path,
        &mut leaves,
        &mut model.diagnostics,
        None,
    );
    model.leaves = leaves;
    Ok(model)
}

/// Decompose an already-tokenized content stream, with an explicit
/// XObject resolver and initial CTM, and **no font resolution**.
///
/// Geometry-only: every text object comes back with
/// [`TextPreview::Unavailable`] and no [`TextFont`]. That is the right
/// answer for every caller that indexes objects for *editing* — `edit.rs`'s
/// surgery planner, the snap engine, the fuzz targets — none of which needs
/// to know what a string says, and all of which would otherwise pay for a
/// `/ToUnicode` parse per font.
///
/// Callers that display objects to a human want
/// [`decompose_with_fonts`] (or [`decompose_page`], which supplies both
/// resolvers for a page). This function's signature is deliberately
/// unchanged from before text previews existed, so every geometry caller
/// stayed a no-diff.
#[must_use]
pub fn decompose(
    content: &ContentStream,
    initial: Matrix,
    xobjects: &dyn XObjectResolver,
) -> PageObjects {
    decompose_with_fonts(content, initial, xobjects, &NoFonts)
}

/// Decompose an already-tokenized content stream with **both** resolvers —
/// the walk's true entry point.
///
/// `xobjects` classifies `Do` names (§8.8) so an image/form object gets a
/// bbox and its sample count; `fonts` resolves `Tf` names (§9.6.2.1) so a
/// text object gets its decoded preview and typeface. Either may be the
/// inert [`NoXObjects`]/[`NoFonts`]; the walk's geometry is identical
/// either way, which is what makes the byte-inertness claim (R46) and the
/// renderer cross-check independent of whether a caller asked for text
/// detail.
///
/// # Examples
///
/// ```
/// use pdfcer_core::content::ContentStream;
/// use pdfcer_core::vector::{
///     Matrix, NoFonts, NoXObjects, TextPreview, VectorObject, decompose_with_fonts,
/// };
///
/// // With no font resolver the text object is honest about WHY it has no
/// // preview: nothing was attempted, as opposed to nothing being there.
/// let cs = ContentStream::parse(b"BT /F1 12 Tf 10 10 Td (Hi) Tj ET".to_vec())?;
/// let model = decompose_with_fonts(&cs, Matrix::IDENTITY, &NoXObjects, &NoFonts);
/// let VectorObject::Text(text) = &model.objects[0] else { panic!("a text object") };
/// assert_eq!(text.preview, TextPreview::Unavailable);
/// // The `Tf` operands are read straight from the stream, so the resource
/// // name and size are known even with no document behind them.
/// let font = text.font.as_ref().expect("a /Tf was in effect");
/// assert_eq!(font.resource, "F1");
/// assert_eq!(font.size, 12.0);
/// assert_eq!(font.base_font, None); // no resolver, so no typeface claim
/// # Ok::<(), pdfcer_core::content::ContentError>(())
/// ```
#[must_use]
pub fn decompose_with_fonts(
    content: &ContentStream,
    initial: Matrix,
    xobjects: &dyn XObjectResolver,
    fonts: &dyn FontResolver,
) -> PageObjects {
    let mut d = Decomposer::new(content, initial, xobjects, fonts);
    d.run();
    PageObjects {
        objects: d.objects,
        initial,
        diagnostics: d.diag,
        // Empty, and that means "nobody looked". Descending into a form needs
        // its CONTENT STREAM, and this entry point has only the classification
        // seam -- which is exactly what lets the fuzz target and the unit tests
        // drive the walk with no document at all. `decompose_page` fills it.
        leaves: Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// The walk
// ---------------------------------------------------------------------------

/// The Pass 9a subset of the graphics state the object model tracks — the
/// CTM (the only geometry-load-bearing part), the stroke geometry width,
/// the device colours, and the text-state parameters the approximate text
/// bbox and the text preview need. Saved/restored by `q`/`Q` (§8.4.2), like
/// the renderer's [`crate::content`]-driven state.
///
/// `Clone` rather than `Copy` since the resolved font joined it: the font
/// IS part of the text state and therefore part of the graphics state
/// (§9.3), so `q`/`Q` must save and restore it, and an [`Arc`] clone on a
/// `q` is one refcount bump.
#[derive(Debug, Clone)]
struct GState {
    ctm: Matrix,
    line_width: f64,
    fill_color: Rgb,
    stroke_color: Rgb,
    /// The honest paint, tracking colour SPACE as well as value.
    ///
    /// Carried BESIDE the `Rgb` rather than replacing it: the `Rgb` is still
    /// the right answer for the device cases and is what hit-testing and the
    /// selection overlay want. `PathPaint` is what a consumer must consult
    /// before asserting a colour or writing one into a file.
    fill_paint: PathPaint,
    stroke_paint: PathPaint,
    /// The colour-space resource name most recently selected by `cs` / `CS`,
    /// which `sc`/`scn` then take their operands in.
    fill_space: Option<Vec<u8>>,
    stroke_space: Option<Vec<u8>>,
    /// `/ca` and `/CA` from an `/ExtGState` (§8.4.5). Not a value this model
    /// exposes — carried on the graphics state so `q`/`Q` save and restore it
    /// like any other, and read only to COUNT a path that cannot be seen.
    alpha_fill: f64,
    alpha_stroke: f64,
    /// The `Tf` size operand (§9.3.1 `Tfs`), text space, unscaled.
    font_size: f64,
    /// The `Tf` resource name, verbatim from the content stream.
    font_resource: Option<Vec<u8>>,
    /// The decoder for [`Self::font_resource`], if the [`FontResolver`]
    /// could produce one.
    font: Option<Arc<ExtractFont>>,
    /// The §9.3 Table 105 text-state parameters — `Tc`, `Tw`, `Th`, `TL`,
    /// `Trise`, `Tmode` — held as the crate-shared
    /// [`TextStateParams`](crate::text_state::TextStateParams) rather than
    /// as five private fields (Pass 19.0).
    ///
    /// This walk is a *reading* walk: it consumes these values for one
    /// purpose, the approximate text bounding box, and needs no restore
    /// provenance. So it composes the values-only half of the shared
    /// model, not [`AmbientTextState`](crate::text_state::AmbientTextState)
    /// — a deliberate partial adoption, documented rather than silently
    /// divergent. `h_scale` here is `Th` (the ratio), matching what the
    /// §9.4.4 displacement formula multiplies by.
    text: TextStateParams,
}

impl GState {
    fn initial(ctm: Matrix) -> Self {
        Self {
            ctm,
            line_width: 1.0, // Table 52 initial
            fill_color: Rgb::BLACK,
            stroke_color: Rgb::BLACK,
            fill_paint: PathPaint::Default,
            stroke_paint: PathPaint::Default,
            fill_space: None,
            stroke_space: None,
            alpha_fill: 1.0,
            alpha_stroke: 1.0,
            font_size: 0.0,
            font_resource: None,
            font: None,
            // Table 105 initial values: Tc = 0, Tw = 0, Tz = 100 (⇒ Th =
            // 1.0), TL = 0, Ts = 0, Tr = 0 — stated once, in
            // `TextStateParams::INITIAL`, instead of once per tracker.
            text: TextStateParams::INITIAL,
        }
    }
}

/// The in-progress path object (mirrors the renderer's `Interpreter` path
/// fields: `path`/`path_ctm`/`current`/`subpath_start`/`needs_move`).
struct PathAccum {
    subpaths: Vec<Subpath>,
    open: Option<Subpath>,
    ctm: Matrix,
    current: Option<Point>,
    subpath_start: Option<Point>,
    needs_move: bool,
    token_start: usize,
}

/// The in-progress text object (`BT`…`ET`), including the preview
/// accumulator.
struct TextAccum {
    token_start: usize,
    /// The CTM in force at `BT` — see [`TextObject::ctm`].
    ctm: Matrix,
    /// The hull of every show operator's pen-START position, page space.
    ///
    /// Still tracked even when the metrics path is available, for two
    /// reasons: it is the emptiness test that decides whether a `BT`…`ET`
    /// produced an object at all, and it is the input to the
    /// [`TextBoundsBasis::EmBox`] fallback.
    origins: Bounds,
    /// Per-show-operator runs — the un-unioned form of [`Self::ink`], plus
    /// each run's byte span and positioning. See [`TextObject::runs`].
    runs: Vec<TextRun>,
    /// The token range of the show operation currently being laid out, set
    /// by [`Walker::operation`] before it dispatches a show operator.
    ///
    /// `None` would mean a run closed without the walker having recorded
    /// which operation produced it — impossible through the dispatch, and
    /// guarded rather than unwrapped: a wrong span here would delete a
    /// DIFFERENT run from the one picked, and the result would be
    /// well-formed and round-trip cleanly, so nothing downstream could
    /// catch it.
    current_run_tokens: Option<TokenRange>,
    /// Byte offset into [`Self::preview`] at which the run currently being
    /// laid out began — becomes the start of [`TextRun::text`].
    ///
    /// Captured when a show operator opens a run rather than when it
    /// closes, because `decode_show_string` has already appended this
    /// run's characters to `preview` by the time `close_text_run` runs.
    current_run_text_start: usize,
    /// Whether a positioning operator has run since the last show operator
    /// closed — see [`RunPositioning`].
    ///
    /// Starts `true`, because `BT` resets the text and line matrices to the
    /// identity (§9.4.1) and that is an origin of its own: the first run of
    /// a text object is never inherited.
    positioned_since_run: bool,
    /// The box of the show operator currently being laid out, folded into
    /// `runs` when it ends. Separate from `ink` because `ink` must stay the
    /// running union for the existing basis logic.
    current_run: Bounds,
    /// Set once `runs` passes [`MAX_TEXT_RUNS`], so the overflow is decided
    /// once rather than re-tested per run.
    runs_overflowed: bool,
    /// The union of every laid-out glyph's box, page space — the
    /// metrics-derived extent. Empty when no show operator had a usable
    /// font.
    ink: Bounds,
    /// The hull of the pen-start positions of the show operators that could
    /// NOT be laid out (no resolvable font, or an unusable `Tf` size). Each
    /// contributes an em box around its origin, exactly as every text
    /// object did before advances were accumulated.
    unmetered: Bounds,
    /// Whether any laid-out glyph used estimated widths
    /// (`ExtractFont::width_estimated`) — degrades the basis to
    /// [`TextBoundsBasis::EstimatedAdvances`].
    widths_estimated: bool,
    /// Whether any laid-out glyph used a nominal vertical extent
    /// (`ExtractFont::vertical_is_nominal`) — degrades the basis to
    /// [`TextBoundsBasis::MetricAdvancesNominalHeight`].
    vertical_nominal: bool,
    max_font_size: f64,
    text_matrix: Matrix,
    line_matrix: Matrix,
    /// Decoded characters so far, never longer than
    /// [`MAX_TEXT_PREVIEW_CHARS`] characters.
    preview: String,
    /// Character count of `preview` (tracked rather than recounted, since
    /// `String::chars().count()` is O(n) and this is checked per code).
    preview_chars: usize,
    /// Whether decoding stopped at the cap with codes still to come.
    truncated: bool,
    /// Codes in the decoded prefix that the §9.10.2 ladder mapped.
    decoded_codes: usize,
    /// Codes in the decoded prefix that reached the ladder's failure clause.
    failed_codes: usize,
    /// Whether any show operator carried a string operand at all — the
    /// difference between [`TextPreview::Empty`] and a decode result.
    showed_any: bool,
    /// Whether at least one of those strings was decoded through a resolved
    /// font. False means no decoder was in scope (no resolver, or a `Tf`
    /// naming a font the resource dictionary does not hold), which is
    /// [`TextPreview::Unavailable`] — a fact about the LOOKUP, not about the
    /// document's text.
    decode_attempted: bool,
    /// The font at the FIRST show operator ([`TextFont`]'s own rationale).
    font: Option<TextFont>,
}

impl TextAccum {
    fn new(token_start: usize, ctm: Matrix) -> Self {
        Self {
            token_start,
            ctm,
            origins: Bounds::EMPTY,
            runs: Vec::new(),
            current_run_tokens: None,
            current_run_text_start: 0,
            positioned_since_run: true,
            current_run: Bounds::EMPTY,
            runs_overflowed: false,
            ink: Bounds::EMPTY,
            unmetered: Bounds::EMPTY,
            widths_estimated: false,
            vertical_nominal: false,
            max_font_size: 0.0,
            text_matrix: Matrix::IDENTITY,
            line_matrix: Matrix::IDENTITY,
            preview: String::new(),
            preview_chars: 0,
            truncated: false,
            decoded_codes: 0,
            failed_codes: 0,
            showed_any: false,
            decode_attempted: false,
            font: None,
        }
    }

    /// Fold the accumulator into the disclosed [`TextPreview`] (the four
    /// cases the enum documents).
    fn finish(self) -> (TextPreview, Option<TextFont>) {
        let preview = if !self.showed_any {
            TextPreview::Empty
        } else if !self.decode_attempted {
            // A show operator ran, but no decoder was in scope. Saying
            // "empty" here would blame the document for a failed lookup.
            TextPreview::Unavailable
        } else if self.decoded_codes == 0 && self.failed_codes > 0 {
            TextPreview::Undecodable
        } else {
            TextPreview::Decoded {
                text: self.preview,
                truncated: self.truncated,
                lossy: self.failed_codes > 0,
            }
        };
        (preview, self.font)
    }

    /// Which of the four constructions the accumulated state entitles this
    /// object's box to claim (see [`TextBoundsBasis`]).
    ///
    /// Worst-wins, deliberately: an object that laid out three runs from
    /// real `/Widths` and one from nothing is reported as
    /// [`TextBoundsBasis::EmBox`], because part of the box the operator is
    /// looking at IS an em box and a disclosure describing only the good
    /// part would be the more misleading of the two available sentences.
    fn basis(&self) -> TextBoundsBasis {
        if self.ink.is_empty() || !self.unmetered.is_empty() {
            TextBoundsBasis::EmBox
        } else if self.widths_estimated {
            TextBoundsBasis::EstimatedAdvances
        } else if self.vertical_nominal {
            TextBoundsBasis::MetricAdvancesNominalHeight
        } else {
            TextBoundsBasis::FontMetrics
        }
    }
}

struct Decomposer<'a> {
    content: &'a ContentStream,
    xobjects: &'a dyn XObjectResolver,
    fonts: &'a dyn FontResolver,
    stack: Vec<GState>,
    gs: GState,
    path: Option<PathAccum>,
    text: Option<TextAccum>,
    objects: Vec<VectorObject>,
    diag: DecomposeDiagnostics,
    total_nodes: usize,
}

impl<'a> Decomposer<'a> {
    fn new(
        content: &'a ContentStream,
        initial: Matrix,
        xobjects: &'a dyn XObjectResolver,
        fonts: &'a dyn FontResolver,
    ) -> Self {
        Self {
            content,
            xobjects,
            fonts,
            stack: Vec::new(),
            gs: GState::initial(initial),
            path: None,
            text: None,
            objects: Vec::new(),
            diag: DecomposeDiagnostics::default(),
            total_nodes: 0,
        }
    }

    /// Walk the token stream, mirroring [`ContentStream::operations`]'s
    /// operand-run/operator segmentation but tracking each operation's
    /// first-token index (the object token-range start).
    fn run(&mut self) {
        let mut run_start = 0usize;
        for (i, tok) in self.content.tokens.iter().enumerate() {
            match tok.kind {
                ContentTokenKind::Operand(_) => {}
                _ => {
                    let operands = self.content.tokens.get(run_start..i).unwrap_or(&[]);
                    self.operation(operands, tok, run_start, i);
                    run_start = i + 1;
                }
            }
        }
        // A trailing, unpainted path (malformed per §8.5.3 "a painting
        // operator shall follow") is dropped, matching the renderer's
        // discard of an unpainted `PathBuilder`; its tokens stay in the
        // stream (byte-inert).
    }

    /// Handle one operation: `operator` is the operator token (or an inline
    /// image), `operands` the preceding operand run, `first`/`op_index` the
    /// operation's token bounds.
    fn operation(
        &mut self,
        operands: &[ContentToken],
        operator: &ContentToken,
        first: usize,
        op_index: usize,
    ) {
        // The one non-operator "operation": a complete inline image. Its
        // parameter dictionary travels WITH the token (§8.9.7), so its
        // sample count is read here and needs no resolver.
        if let ContentTokenKind::InlineImage { params, .. } = &operator.kind {
            let pixel_size = inline_pixel_size(params);
            self.emit_image(
                ImageSource::Inline,
                self.gs.ctm,
                unit_square(),
                pixel_size,
                // §8.9.7: an inline image IS the content stream's own bytes.
                // It has no object of its own to name.
                None,
                first,
                op_index,
            );
            return;
        }
        let Some(name) = operator.span.slice(&self.content.buf) else {
            return;
        };
        let nums = operand_nums(operands);

        // `Pass 32.0` substrate, recorded here because this is the only
        // place that knows an operation's token bounds.
        //
        // The SHOW operators get their span stashed for `close_text_run`;
        // the POSITIONING operators latch that the next run owns its origin
        // (§9.4.2 — without one, a run starts wherever the previous string
        // left the pen). `'` and `"` are in BOTH lists on purpose: Table 109
        // defines each as a line move followed by a show, so they position
        // themselves and then draw.
        if let Some(t) = self.text.as_mut() {
            match name {
                b"Td" | b"TD" | b"Tm" | b"T*" | b"'" | b"\"" => t.positioned_since_run = true,
                _ => {}
            }
            if matches!(name, b"Tj" | b"TJ" | b"'" | b"\"") {
                t.current_run_tokens = Some(TokenRange {
                    start: first,
                    end: op_index + 1,
                });
                // Latched HERE, one dispatch before `show_string` decodes,
                // because by the time `close_text_run` sees the run its
                // characters are already in `preview` and the start offset
                // is unrecoverable.
                t.current_run_text_start = t.preview.len();
            }
        }

        match name {
            // ---- graphics state (Table 57) ----
            b"q" => self.stack.push(self.gs.clone()),
            b"Q" => match self.stack.pop() {
                Some(prev) => self.gs = prev,
                None => self.diag.unbalanced_q += 1,
            },
            b"cm" => {
                if let &[a, b, c, d, e, f] = nums.as_slice() {
                    self.gs.ctm = Matrix::new(a, b, c, d, e, f).post_concat(self.gs.ctm);
                }
            }
            b"w" => {
                if let &[lw] = nums.as_slice() {
                    self.gs.line_width = lw.max(0.0);
                }
            }
            // ---- device colours (§8.6.4, Table 74 subset) ----
            b"g" => {
                set_color(&mut self.gs.fill_color, Rgb::from_gray, &nums);
                self.gs.fill_paint =
                    device_paint(DevicePaintSpace::Gray, &nums, self.gs.fill_color);
                self.gs.fill_space = None;
            }
            b"G" => {
                set_color(&mut self.gs.stroke_color, Rgb::from_gray, &nums);
                self.gs.stroke_paint =
                    device_paint(DevicePaintSpace::Gray, &nums, self.gs.stroke_color);
                self.gs.stroke_space = None;
            }
            b"rg" => {
                set_rgb(&mut self.gs.fill_color, &nums);
                self.gs.fill_paint = device_paint(DevicePaintSpace::Rgb, &nums, self.gs.fill_color);
                self.gs.fill_space = None;
            }
            b"RG" => {
                set_rgb(&mut self.gs.stroke_color, &nums);
                self.gs.stroke_paint =
                    device_paint(DevicePaintSpace::Rgb, &nums, self.gs.stroke_color);
                self.gs.stroke_space = None;
            }
            b"k" => {
                set_cmyk(&mut self.gs.fill_color, &nums);
                self.gs.fill_paint =
                    device_paint(DevicePaintSpace::Cmyk, &nums, self.gs.fill_color);
                self.gs.fill_space = None;
            }
            b"K" => {
                set_cmyk(&mut self.gs.stroke_color, &nums);
                self.gs.stroke_paint =
                    device_paint(DevicePaintSpace::Cmyk, &nums, self.gs.stroke_color);
                self.gs.stroke_space = None;
            }

            // ---- the graphics-state operator (§8.4.5) -- previously ABSENT.
            //
            // ★★ `gs` had NO ARM AT ALL, so every entry it sets was ignored.
            // The consequence is the same shape as the missing colour-space
            // arms below: a value the model already exposes kept whatever an
            // unrelated earlier operator had set, with nothing recording that
            // pdfcer had not looked.
            //
            // MEASURED before choosing what to handle, over 300 files of a
            // 4,023-file corpus: 11% carry an `/ExtGState`, and within those
            // `/Font` appears 115 times, `/CA` 59, `/BM` 27, `/ca` 18 -- and
            // `/LW` **zero**. So the exposure is the FONT SIZE, which scales a
            // text object's bounds, and not the line width the defect was
            // first reported against. Both are handled; only one of them was
            // ever going to fire on a real file.
            // `sh` (§8.7.4.2) paints a shading directly. This model produces
            // no object for it, so it is COUNTED -- see
            // `DecomposeDiagnostics::shadings_unmodelled`.
            b"sh" => self.diag.shadings_unmodelled += 1,
            // `BDC` with an `/OC` tag opens an optional-content (layer)
            // section. Visibility is not resolved here; counted so a shell can
            // tell that a page HAS layers.
            b"BDC" => {
                if operand_names(operands).first().is_some_and(|n| n == b"OC") {
                    self.diag.oc_sections += 1;
                }
            }
            b"gs" => {
                if let Some(g) = operand_names(operands)
                    .first()
                    .and_then(|n| self.fonts.ext_gstate(n))
                {
                    if let Some(w) = g.line_width {
                        self.gs.line_width = w;
                    }
                    if let Some((face, size)) = g.font {
                        self.gs.font_size = size;
                        if !face.is_empty() {
                            self.gs.font = self.fonts.resolve(&face);
                            self.gs.font_resource = Some(face);
                        }
                    }
                    // Alpha is not a value this model exposes, so it is
                    // COUNTED rather than stored: a fully transparent path is
                    // reported as an ordinary painted object today, and an
                    // operator selecting something invisible is a wrong CLAIM
                    // rather than a wrong number.
                    if let Some(a) = g.fill_alpha {
                        self.gs.alpha_fill = a;
                    }
                    if let Some(a) = g.stroke_alpha {
                        self.gs.alpha_stroke = a;
                    }
                }
            }

            // ---- colour SPACES (§8.6.8, Table 74) -- previously ABSENT.
            //
            // ★★ THE WHOLE POINT OF THIS BLOCK. Without these six arms a path
            // painted in a `/Separation`, `/DeviceN`, `/ICCBased`, `/Indexed`,
            // `/Lab` or `/Pattern` space kept whatever the last DEVICE
            // operator had set -- a stale colour from an unrelated earlier
            // object, with nothing recording that pdfcer did not know.
            //
            // `cs`/`CS` select the space; `sc`/`scn`/`SC`/`SCN` then set a
            // value in it. §8.6.8 also says selecting a space RESETS the
            // colour to that space's initial value, which is why the space
            // operators set the paint rather than only remembering a name.
            b"cs" | b"CS" => {
                let stroking = name == b"CS";
                let space_name = operand_names(operands).into_iter().next();
                let paint = PathPaint::Other {
                    space: space_name.clone(),
                    comps: Vec::new(),
                    pattern: false,
                };
                if stroking {
                    self.gs.stroke_space = space_name;
                    self.gs.stroke_paint = paint;
                } else {
                    self.gs.fill_space = space_name;
                    self.gs.fill_paint = paint;
                }
            }
            b"sc" | b"scn" | b"SC" | b"SCN" => {
                let stroking = name == b"SC" || name == b"SCN";
                // A NAME operand means a pattern (§8.7.3), which has no colour
                // at all -- a different fact from "a colour pdfcer cannot
                // decode", and a different refusal.
                let pattern = !operand_names(operands).is_empty();
                let space = if stroking {
                    self.gs.stroke_space.clone()
                } else {
                    self.gs.fill_space.clone()
                };
                let paint = PathPaint::Other {
                    space,
                    comps: nums.clone(),
                    pattern,
                };
                if stroking {
                    self.gs.stroke_paint = paint;
                } else {
                    self.gs.fill_paint = paint;
                }
            }

            // ---- path construction (Table 59) ----
            b"m" => {
                if let &[x, y] = nums.as_slice() {
                    self.move_to(Point::new(x, y), first, op_index);
                }
            }
            b"l" => {
                if let &[x, y] = nums.as_slice() {
                    self.line_to(Point::new(x, y), first, op_index);
                }
            }
            b"c" => {
                if let &[x1, y1, x2, y2, x3, y3] = nums.as_slice() {
                    self.curve_to(
                        Point::new(x1, y1),
                        Point::new(x2, y2),
                        Point::new(x3, y3),
                        first,
                        op_index,
                    );
                }
            }
            b"v" => {
                // First control = current point (shared primitive).
                if let &[x2, y2, x3, y3] = nums.as_slice()
                    && let Some(cur) = self.current_for_segment(first, op_index)
                {
                    let (c1, c2, end) = cubic_from_v(cur, x2, y2, x3, y3);
                    self.append_cubic(c1, c2, end, op_index);
                }
            }
            b"y" => {
                // Second control = endpoint (shared primitive).
                if let &[x1, y1, x3, y3] = nums.as_slice()
                    && self.current_for_segment(first, op_index).is_some()
                {
                    let (c1, c2, end) = cubic_from_y(x1, y1, x3, y3);
                    self.append_cubic(c1, c2, end, op_index);
                }
            }
            b"h" => self.close_subpath(op_index),
            b"re" => {
                if let &[x, y, w, h] = nums.as_slice() {
                    self.rect(x, y, w, h, first, op_index);
                }
            }

            // ---- path painting (Table 60) + clipping (Table 61) ----
            b"S" => self.paint(
                op_index,
                PaintStyle {
                    fill: None,
                    stroke: true,
                },
                false,
            ),
            b"s" => self.paint(
                op_index,
                PaintStyle {
                    fill: None,
                    stroke: true,
                },
                true,
            ),
            b"f" | b"F" => self.paint(
                op_index,
                PaintStyle {
                    fill: Some(FillRule::NonZero),
                    stroke: false,
                },
                false,
            ),
            b"f*" => self.paint(
                op_index,
                PaintStyle {
                    fill: Some(FillRule::EvenOdd),
                    stroke: false,
                },
                false,
            ),
            b"B" => self.paint(
                op_index,
                PaintStyle {
                    fill: Some(FillRule::NonZero),
                    stroke: true,
                },
                false,
            ),
            b"B*" => self.paint(
                op_index,
                PaintStyle {
                    fill: Some(FillRule::EvenOdd),
                    stroke: true,
                },
                false,
            ),
            b"b" => self.paint(
                op_index,
                PaintStyle {
                    fill: Some(FillRule::NonZero),
                    stroke: true,
                },
                true,
            ),
            b"b*" => self.paint(
                op_index,
                PaintStyle {
                    fill: Some(FillRule::EvenOdd),
                    stroke: true,
                },
                true,
            ),
            b"n" => self.paint(
                op_index,
                PaintStyle {
                    fill: None,
                    stroke: false,
                },
                false,
            ),

            // ---- text objects (Table 107) ----
            b"BT" => {
                self.discard_path(); // defensive: a path open across BT is malformed
                self.text = Some(TextAccum::new(op_index, self.gs.ctm));
            }
            b"ET" => self.end_text(op_index),

            // ---- text state / positioning the bbox + preview need ----
            b"Tf" => {
                // `Tf name size` (§9.3.1): the name operand selects the font
                // resource, the number operand is the size. Both are part of
                // the graphics state, so both survive to the next `BT`.
                if let Some(size) = nums.last().copied() {
                    self.gs.font_size = size;
                }
                if let Some(resource) = last_name(operands) {
                    // Resolve eagerly rather than at the first show operator:
                    // `DocumentFonts` memoizes, so a repeated `Tf` is a hash
                    // lookup, and doing it here keeps the show path (which
                    // runs per string) free of resolution logic.
                    self.gs.font = self.fonts.resolve(&resource);
                    self.gs.font_resource = Some(resource);
                }
            }
            b"TL" => {
                if let &[v] = nums.as_slice() {
                    self.gs.text.leading = v;
                }
            }
            // §9.3 Table 105 text-state parameters that enter the run's
            // layout arithmetic. They are part of the GRAPHICS state, not
            // the text object, so `q`/`Q` saves and restores them and they
            // survive across `BT`/`ET` — which is why they live on `GState`
            // beside the CTM rather than on the text accumulator.
            b"Tc" => {
                if let &[v] = nums.as_slice() {
                    self.gs.text.char_spacing = v;
                }
            }
            b"Tw" => {
                if let &[v] = nums.as_slice() {
                    self.gs.text.word_spacing = v;
                }
            }
            b"Tz" => {
                if let &[v] = nums.as_slice() {
                    // §9.3.4: the operand is a PERCENTAGE.
                    self.gs.text.h_scale = v / 100.0;
                }
            }
            b"Ts" => {
                if let &[v] = nums.as_slice() {
                    self.gs.text.rise = v;
                }
            }
            b"Td" => {
                if let &[tx, ty] = nums.as_slice() {
                    self.text_line_offset(tx, ty);
                }
            }
            b"TD" => {
                if let &[tx, ty] = nums.as_slice() {
                    self.gs.text.leading = -ty;
                    self.text_line_offset(tx, ty);
                }
            }
            b"Tm" => {
                if let &[a, b, c, d, e, f] = nums.as_slice()
                    && let Some(t) = self.text.as_mut()
                {
                    let m = Matrix::new(a, b, c, d, e, f);
                    t.text_matrix = m;
                    t.line_matrix = m;
                }
            }
            b"T*" => {
                let leading = self.gs.text.leading;
                self.text_line_offset(0.0, -leading);
            }
            b"Tj" | b"TJ" => self.show_text(operands),
            // §9.4.3 Table 109: `'` is exactly `T*` followed by `Tj`, and
            // `"` is `aw Tw`, `ac Tc`, `T*`, `Tj`. Before advance
            // accumulation the line move was skipped (the origin hull was
            // inflated by a whole em anyway, which hid a one-line error);
            // a box that now claims to be where the text is has to make it.
            b"'" => {
                let leading = self.gs.text.leading;
                self.text_line_offset(0.0, -leading);
                self.show_text(operands);
            }
            b"\"" => {
                // `aw ac string "` — the two NUMERIC operands are word and
                // character spacing, in that order, and they persist in the
                // graphics state afterwards (Table 109's own wording: the
                // operator "shall set" them).
                if let &[aw, ac] = nums.as_slice() {
                    self.gs.text.word_spacing = aw;
                    self.gs.text.char_spacing = ac;
                }
                let leading = self.gs.text.leading;
                self.text_line_offset(0.0, -leading);
                self.show_text(operands);
            }

            // ---- external objects (§8.8) ----
            b"Do" => self.do_xobject(operands, first, op_index),

            // Everything else (state we don't model for geometry, shading,
            // marked content, unknown operators) is ignored for the object
            // model — it affects neither node geometry nor selectability.
            _ => {}
        }
    }

    // -- path construction helpers (mirror the renderer's Interpreter) --

    /// Capture the CTM at the path's first construction op; a mid-path
    /// `cm` is tolerated (keep the first CTM) and counted, exactly as the
    /// renderer's `capture_path_ctm` does.
    fn ensure_path(&mut self, first: usize) {
        if self.path.is_none() {
            // First construction op of a new object: capture today's CTM.
            self.path = Some(PathAccum {
                subpaths: Vec::new(),
                open: None,
                ctm: self.gs.ctm,
                current: None,
                subpath_start: None,
                needs_move: false,
                token_start: first,
            });
            return;
        }
        // An existing object seeing a different CTM = a mid-path `cm`
        // (legal, vanishingly rare): keep the captured CTM, count it.
        let ctm = self.gs.ctm;
        if self.path.as_ref().is_some_and(|p| p.ctm != ctm) {
            self.diag.midpath_cm += 1;
        }
    }

    fn move_to(&mut self, p: Point, first: usize, op_index: usize) {
        self.ensure_path(first);
        finalize_open(self.path.as_mut());
        if let Some(pa) = self.path.as_mut() {
            pa.open = Some(Subpath {
                start: p,
                segments: Vec::new(),
                closed: false,
                tokens: TokenRange {
                    start: first,
                    end: op_index,
                },
                starts_implicitly: false,
            });
            pa.current = Some(p);
            pa.subpath_start = Some(p);
            pa.needs_move = false;
        }
    }

    /// The renderer's `begin_segment`: a segment needs a current point;
    /// after `h`/`re` (`needs_move`) it opens a new subpath at the current
    /// point. Returns the current point, or `None` (skip + count) if there
    /// is no current point.
    fn current_for_segment(&mut self, first: usize, op_index: usize) -> Option<Point> {
        // A segment with no path at all AND no current point is a
        // §8.5.2.1 error.
        let cur = self.path.as_ref().and_then(|p| p.current);
        let Some(cur) = cur else {
            self.diag.segment_without_current += 1;
            return None;
        };
        self.ensure_path(first);
        if let Some(pa) = self.path.as_mut()
            && pa.needs_move
        {
            pa.open = Some(Subpath {
                start: cur,
                segments: Vec::new(),
                closed: false,
                tokens: TokenRange {
                    start: first,
                    end: op_index,
                },
                // No `m` of its own: the start point is inherited from the
                // subpath that `h` just closed.
                starts_implicitly: true,
            });
            pa.subpath_start = Some(cur);
            pa.needs_move = false;
        }
        Some(cur)
    }

    fn line_to(&mut self, p: Point, first: usize, op_index: usize) {
        if self.current_for_segment(first, op_index).is_some() {
            self.push_segment(Segment::Line { to: p }, p, op_index);
        }
    }

    fn curve_to(&mut self, c1: Point, c2: Point, end: Point, first: usize, op_index: usize) {
        if self.current_for_segment(first, op_index).is_some() {
            self.append_cubic(c1, c2, end, op_index);
        }
    }

    fn append_cubic(&mut self, c1: Point, c2: Point, end: Point, op_index: usize) {
        self.push_segment(Segment::Cubic { c1, c2, to: end }, end, op_index);
    }

    fn push_segment(&mut self, seg: Segment, new_current: Point, op_index: usize) {
        if self.total_nodes >= MAX_NODES {
            self.diag.nodes_dropped += 1;
            return;
        }
        if let Some(pa) = self.path.as_mut()
            && let Some(open) = pa.open.as_mut()
        {
            open.segments.push(seg);
            // Grow the subpath's token range to cover this operator. Recorded
            // here, on the walk that already knows it, rather than re-derived
            // later by a second walk that might disagree.
            open.tokens.end = open.tokens.end.max(op_index);
            pa.current = Some(new_current);
            self.total_nodes += 1;
        }
    }

    /// `h` (§8.5.2.1): close the current subpath; the current point
    /// becomes the subpath start, and the next segment op opens a new
    /// subpath there.
    fn close_subpath(&mut self, op_index: usize) {
        if let Some(pa) = self.path.as_mut() {
            if let Some(open) = pa.open.as_mut() {
                open.closed = true;
                // `h` belongs to the subpath it closes, so the span must
                // include it — otherwise deleting the subpath would leave an
                // orphan `h` that closes whatever precedes it.
                open.tokens.end = open.tokens.end.max(op_index);
            }
            finalize_open_pa(pa);
            pa.current = pa.subpath_start;
            pa.needs_move = true;
        }
    }

    /// `re x y w h` (Table 59): a complete closed subpath, expanded via the
    /// shared [`rect_corners`] primitive.
    fn rect(&mut self, x: f64, y: f64, w: f64, h: f64, first: usize, op_index: usize) {
        self.ensure_path(first);
        finalize_open(self.path.as_mut());
        if self.total_nodes.saturating_add(4) > MAX_NODES {
            self.diag.nodes_dropped += 1;
            return;
        }
        let c = rect_corners(x, y, w, h);
        if let Some(pa) = self.path.as_mut() {
            pa.subpaths.push(Subpath {
                start: c[0],
                segments: vec![
                    Segment::Line { to: c[1] },
                    Segment::Line { to: c[2] },
                    Segment::Line { to: c[3] },
                ],
                closed: true,
                // A whole closed subpath in ONE operator.
                tokens: TokenRange {
                    start: first,
                    end: op_index,
                },
                starts_implicitly: false,
            });
            pa.current = Some(c[0]);
            pa.subpath_start = Some(c[0]);
            pa.needs_move = true;
            self.total_nodes += 4;
        }
    }

    /// Terminate the current path object with `style`; `close` first closes
    /// the open subpath (the `s`/`b`/`b*` operators).
    fn paint(&mut self, op_index: usize, style: PaintStyle, close: bool) {
        let Some(mut pa) = self.path.take() else {
            // A painting operator with no path: the renderer's empty-path
            // case (nothing drawn, a `n`/`W` clips everything). No object.
            return;
        };
        if close {
            if let Some(open) = pa.open.as_mut() {
                open.closed = true;
            } else if let Some(last) = pa.subpaths.last_mut() {
                last.closed = true;
            }
        }
        finalize_open_pa(&mut pa);

        if pa.subpaths.is_empty() {
            return; // no geometry (a lone `m` then paint)
        }
        if self.objects.len() >= MAX_OBJECTS {
            self.diag.objects_dropped += 1;
            return;
        }

        let ctm = pa.ctm;
        let page_bbox = subpaths_page_bounds(&pa.subpaths, ctm);
        let bytes = self.span_of(pa.token_start, op_index);
        let obj = PathObject {
            subpaths: pa.subpaths,
            ctm,
            style,
            line_width: self.gs.line_width,
            fill_color: self.gs.fill_color,
            stroke_color: self.gs.stroke_color,
            fill_paint: self.gs.fill_paint.clone(),
            stroke_paint: self.gs.stroke_paint.clone(),
            tokens: TokenRange {
                start: pa.token_start,
                end: op_index + 1,
            },
            bytes,
            page_bbox,
        };
        self.diag.paths += 1;
        if self.gs.fill_paint.is_other() || self.gs.stroke_paint.is_other() {
            self.diag.paths_with_undecoded_colour += 1;
        }
        // Zero exactly, not an epsilon: `/ca 0` is a deliberate "do not show
        // this", where 0.004 is a faint mark somebody meant to be faint.
        let invisible = (style.fill.is_some() && self.gs.alpha_fill == 0.0)
            || (style.stroke && self.gs.alpha_stroke == 0.0);
        if invisible {
            self.diag.paths_invisible_by_alpha += 1;
        }
        self.objects.push(VectorObject::Path(obj));
    }

    /// Drop an in-progress path without emitting an object (a `BT` opening
    /// while a path is open — malformed, tolerated).
    fn discard_path(&mut self) {
        self.path = None;
    }

    // -- text helpers (approximate bbox, module docs) --

    fn text_line_offset(&mut self, tx: f64, ty: f64) {
        if let Some(t) = self.text.as_mut() {
            t.line_matrix = Matrix::translate(tx, ty).post_concat(t.line_matrix);
            t.text_matrix = t.line_matrix;
        }
    }

    /// Handle one text-showing operator (`Tj`/`TJ`/`'`/`"`): record the pen
    /// origin, capture the font on the first one, decode the operand
    /// strings into the preview, **and lay the run out** so the object's
    /// bbox is the extent of the glyphs rather than a square around where
    /// they start.
    ///
    /// ## The layout, and what is and is not modelled
    ///
    /// Per code, in file order (§9.4.4):
    ///
    /// 1. `w0` — the glyph's advance in text space, from
    ///    [`ExtractFont::width`]: `/Widths` (§9.6.2.1), `/W` over `/DW`
    ///    (§9.7.4.3), the standard-14 AFM tables, or `/MissingWidth`.
    ///    **Dictionary data only** — no font program is opened, which is
    ///    what keeps this walk inside `pdfcer-core` (R21 puts glyph
    ///    rasterization in `pdfcer-render`).
    /// 2. `tx = (w0·Tfs + Tc + Tw)·Th`, through the shared
    ///    [`advance_tx`](crate::text_extract::font) — the ONE copy of that
    ///    formula in the crate, also used by extraction and redaction.
    ///    `Tw` participates only for the single-byte code 32 (§9.3.3), a
    ///    decision [`ExtractFont::codes`] already makes per code.
    /// 3. The glyph's box in text space is `x ∈ [0, w0]`,
    ///    `y ∈ [descent, ascent]` from §9.8 Table 122, mapped to page space
    ///    by `Trm = [Tfs·Th 0 0 Tfs 0 Ts] × Tm × CTM` and unioned into
    ///    [`TextAccum::ink`]. All four corners are mapped, not two, because
    ///    a rotated or skewed `Tm`/CTM makes the axis-aligned page-space
    ///    hull of a text-space rectangle depend on all four.
    /// 4. `Tm ← translate(tx, 0) × Tm`.
    ///
    /// A `TJ` array's numeric elements move the text matrix by
    /// `−(v/1000)·Tfs·Th` **when they are met**, not folded into the
    /// neighbouring glyph's advance — Table 109 is explicit that the amount
    /// "shall be subtracted from the current horizontal coordinate", i.e.
    /// the pen moves and the *next* glyph is shown there.
    ///
    /// **Not modelled, and therefore not claimed:** vertical writing mode
    /// (§9.4.4's `ty` branch — every advance here is horizontal, so a
    /// `Identity-V` run's box will be the width of its glyphs laid side by
    /// side rather than stacked); `Tr` render mode 3/7 (invisible text is
    /// still bounded, deliberately — an OCR layer under a scan is a real,
    /// selectable object and hiding it from selection would be the surprise);
    /// and any clip the text may be under. A run that changes `Tf` SIZE
    /// mid-string is modelled correctly per code, since `Tfs` is read from
    /// the graphics state at each show operator — but the em-box fallback
    /// still uses only the largest size seen.
    fn show_text(&mut self, operands: &[ContentToken]) {
        // Snapshot the graphics-state reads before borrowing `self.text`
        // mutably; `Arc::clone` is a refcount bump, not a font copy.
        let ctm = self.gs.ctm;
        let font_size = self.gs.font_size;
        let font = self.gs.font.clone();
        let resource = self.gs.font_resource.clone();

        let Some(t) = self.text.as_mut() else {
            return; // a show operator outside BT/ET — malformed, ignored
        };

        let origin = ctm.map_point(t.text_matrix.map_point(Point::new(0.0, 0.0)));
        t.origins = t.origins.union_point(origin);
        if font_size > t.max_font_size {
            t.max_font_size = font_size;
        }

        // The font of the FIRST show operator identifies the object
        // (`TextFont`'s own docs). A `Tf`-less show has no font to name.
        if t.font.is_none()
            && let Some(resource) = resource
        {
            t.font = Some(TextFont {
                resource: truncate_name(&String::from_utf8_lossy(&resource)),
                base_font: font
                    .as_ref()
                    .map(|f| truncate_name(&f.base_font))
                    .filter(|b| !b.is_empty()),
                size: font_size,
            });
        }

        // §9.4.3 Table 109: `Tj`/`'` take one string; `"` takes `aw ac
        // string`; `TJ` takes an array of strings interleaved with numeric
        // offsets. Every STRING operand in the run is shown text, so walking
        // the operands and taking the strings covers all four operators
        // without a per-operator branch — and tolerates a malformed run (a
        // `Tj` with two strings) by showing both, which is what a lenient
        // reader would render.
        //
        // A number at the TOP level is NOT a positioning offset (it is
        // `"`'s `aw`/`ac`, already consumed by the operator dispatch);
        // only numbers INSIDE the `TJ` array are.
        for token in operands {
            let ContentTokenKind::Operand(object) = &token.kind else {
                continue;
            };
            match object {
                Object::String(bytes) => self.show_string(bytes),
                Object::Array(items) => {
                    for item in items {
                        match item {
                            Object::String(bytes) => self.show_string(bytes),
                            other => {
                                if let Some(v) = other.as_number() {
                                    self.tj_offset(v);
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }

    /// One show string: decode it into the preview AND lay it out into the
    /// ink bounds. Two jobs, one traversal order, deliberately adjacent so
    /// a future edit cannot advance the pen without decoding or vice versa.
    fn show_string(&mut self, bytes: &[u8]) {
        self.decode_show_string(bytes);
        self.advance_show_string(bytes);
        self.close_text_run();
    }

    /// Fold the just-laid-out show operator's box into `runs`.
    ///
    /// Called per show operator rather than per `BT`…`ET`, because the gaps
    /// BETWEEN operators are exactly what the enclosing rectangle wrongly
    /// claims. A `TJ` array is one run by design: its numeric elements are
    /// kerning within a single positioned string, not separate placements,
    /// so splitting on them would fragment a word into per-glyph boxes for
    /// no gain.
    fn close_text_run(&mut self) {
        // Read out of `self` before the mutable borrow: the span helper
        // needs `&self.content`, which cannot coexist with `self.text`
        // being mutably borrowed below.
        let tokens = self.text.as_ref().and_then(|t| t.current_run_tokens);
        let bytes = tokens.map(|r| self.span_of(r.start, r.end.saturating_sub(1)));

        let Some(t) = self.text.as_mut() else { return };
        // Whatever happens to the box, the positioning latch resets: the pen
        // has moved, so anything shown next inherits unless something moves
        // it. Done FIRST so every early return below still resets it —
        // leaving it set would make the NEXT run read as explicitly placed
        // when it is not, which is the direction that loses data.
        let positioned_by = if t.positioned_since_run {
            RunPositioning::Explicit
        } else {
            RunPositioning::Inherited
        };
        t.positioned_since_run = false;
        let run_tokens = t.current_run_tokens.take();
        // Clamped to the preview's current length so a run that opened
        // AFTER the MAX_TEXT_PREVIEW_CHARS cap stopped appending yields an
        // empty range rather than a backwards one. `start > end` would
        // panic the slice in `run_text`, and on exactly the documents this
        // exists for — a CAD sheet whose labels run past the cap.
        let text_start = t.current_run_text_start.min(t.preview.len());
        let text_end = t.preview.len();

        if t.current_run.is_empty() {
            return;
        }
        let bounds = std::mem::replace(&mut t.current_run, Bounds::EMPTY);
        if t.runs_overflowed {
            return;
        }
        if t.runs.len() >= MAX_TEXT_RUNS {
            // Clear, do not truncate — see MAX_TEXT_RUNS. A partial list
            // would make a consumer stop hit-testing the rest of the object
            // while looking like it had tested all of it.
            t.runs.clear();
            t.runs_overflowed = true;
            return;
        }
        // A run whose operation the walker did not record is DROPPED rather
        // than pushed with a guessed span. It cannot happen through
        // `operation`, and if it ever does, a run missing from the list
        // costs a hit-test miss — while a run carrying the WRONG span would
        // let a later edit delete a different label and leave a file that
        // round-trips perfectly.
        let (Some(tokens), Some(bytes)) = (run_tokens.and(tokens), bytes) else {
            return;
        };
        t.runs.push(TextRun {
            bounds,
            tokens,
            bytes,
            positioned_by,
            text_start,
            text_end,
        });
    }

    /// Apply a `TJ` array's numeric element to the text matrix (Table 109).
    ///
    /// "The number shall be expressed in thousandths of a unit of text
    /// space… This amount shall be subtracted from the current horizontal
    /// coordinate", scaled by the font size and by `Th` (§9.4.4's `−Tj/1000`
    /// term sits inside the same `× Tfs … × Th` product as `w0`).
    ///
    /// A non-finite operand is ignored rather than allowed to poison the
    /// text matrix — one NaN would turn every subsequent bbox on the page
    /// into `NaN` and silently un-hit-test the rest of the object
    /// (ARCHITECTURE.md §10).
    fn tj_offset(&mut self, v: f64) {
        let tfs = self.gs.font_size;
        let th = self.gs.text.h_scale;
        let tx = -(v / 1000.0) * tfs * th;
        if !tx.is_finite() {
            return;
        }
        if let Some(t) = self.text.as_mut() {
            t.text_matrix = Matrix::translate(tx, 0.0).post_concat(t.text_matrix);
        }
    }

    /// Lay out one show string's codes, unioning each glyph's box into
    /// [`TextAccum::ink`] and advancing the text matrix (this function's
    /// contract is documented in full on [`Self::show_text`]).
    ///
    /// When no usable font is in scope the string contributes **nothing to
    /// `ink`** and its pen origin is added to [`TextAccum::unmetered`]
    /// instead, which is what makes the object fall back to the em box and
    /// SAY it did ([`TextBoundsBasis::EmBox`]). "Usable" is deliberately
    /// strict: a resolved font with a zero or non-finite `Tf` size would lay
    /// every glyph out at a single point, producing a degenerate box that
    /// looks like a measurement and is not one.
    fn advance_show_string(&mut self, bytes: &[u8]) {
        let ctm = self.gs.ctm;
        let tfs = self.gs.font_size;
        let th = self.gs.text.h_scale;
        let tc = self.gs.text.char_spacing;
        let word_spacing = self.gs.text.word_spacing;
        let rise = self.gs.text.rise;
        let font = self.gs.font.clone();

        let usable = font.filter(|_| {
            tfs.is_finite() && tfs != 0.0 && th.is_finite() && tc.is_finite() && rise.is_finite()
        });

        let Some(t) = self.text.as_mut() else {
            return;
        };
        let Some(font) = usable else {
            // No width source for this run: its extent is unknowable here,
            // so only its start point is, and the em-box fallback covers it.
            let origin = ctm.map_point(t.text_matrix.map_point(Point::new(0.0, 0.0)));
            t.unmetered = t.unmetered.union_point(origin);
            return;
        };

        if font.width_estimated() {
            t.widths_estimated = true;
        }
        if font.vertical_is_nominal() {
            t.vertical_nominal = true;
        }
        let ascent = f64::from(font.ascent());
        let descent = f64::from(font.descent());

        for code in font.codes(bytes) {
            let w0 = f64::from(font.width(code.value));
            let tw = if code.word_spacing_applies {
                word_spacing
            } else {
                0.0
            };
            let tx = crate::text_extract::font::advance_tx(w0, tfs, tc, tw, th);

            // §9.4.4: Trm = [Tfs·Th 0 0 Tfs 0 Ts] × Tm × CTM.
            let params = Matrix::new(tfs * th, 0.0, 0.0, tfs, 0.0, rise);
            let trm = params.post_concat(t.text_matrix).post_concat(ctm);
            for (gx, gy) in [(0.0, descent), (w0, descent), (0.0, ascent), (w0, ascent)] {
                let p = trm.map_point(Point::new(gx, gy));
                // A corner that is not finite (a degenerate CTM, a
                // hostile `Tm`) is dropped rather than allowed to swallow
                // the whole bbox — `union_point` would propagate the NaN.
                if p.is_finite() {
                    t.ink = t.ink.union_point(p);
                    t.current_run = t.current_run.union_point(p);
                }
            }

            if tx.is_finite() {
                t.text_matrix = Matrix::translate(tx, 0.0).post_concat(t.text_matrix);
            }
        }
    }

    /// Decode one show string's bytes into the in-progress preview through
    /// the §9.10.2 ladder, stopping at [`MAX_TEXT_PREVIEW_CHARS`].
    ///
    /// **Stops decoding, not just appending.** The cap is a work bound as
    /// well as a memory bound: a hostile page can carry a megabyte of show
    /// strings per text object, and mapping every code through a
    /// `/ToUnicode` CMap only to discard the result would be an easy
    /// amplification (ARCHITECTURE.md §10). The consequence — that
    /// [`TextPreview::Decoded::lossy`] describes the decoded PREFIX rather
    /// than the whole string — is documented on the field.
    fn decode_show_string(&mut self, bytes: &[u8]) {
        let font = self.gs.font.clone();
        let Some(t) = self.text.as_mut() else {
            return;
        };
        if !bytes.is_empty() {
            t.showed_any = true;
        }
        let Some(font) = font else {
            return; // no decoder in scope → TextPreview::Unavailable
        };
        if !bytes.is_empty() {
            t.decode_attempted = true;
        }
        for code in font.codes(bytes) {
            if t.preview_chars >= MAX_TEXT_PREVIEW_CHARS {
                t.truncated = true;
                return;
            }
            // `TX-A1` PINNED. This is a bounded PREVIEW string shown
            // beside a decomposed text object, and its whole job is to let
            // an operator recognise the object; a sentinel that renders as
            // nothing would make an undecodable run look like an empty
            // one. `failed_codes` counts either way.
            let (text, rung) = font.to_unicode(code.value, UnmappableCode::ReplacementChar);
            if rung == LadderRung::Failed {
                t.failed_codes += 1;
            } else {
                t.decoded_codes += 1;
            }
            for ch in text.chars() {
                if t.preview_chars >= MAX_TEXT_PREVIEW_CHARS {
                    // One code can map to several characters (§9.10.3), so
                    // the cap can be reached mid-code; the rest of THIS
                    // code's characters are elided too, and disclosed.
                    t.truncated = true;
                    return;
                }
                t.preview.push(ch);
                t.preview_chars += 1;
            }
        }
    }

    fn end_text(&mut self, op_index: usize) {
        // Close any run still open at `ET` through the SAME path every other
        // run takes. It used to be pushed inline further down, which was one
        // more place that had to remember the span and the positioning latch
        // — and the `Pass 32.0` substrate made that duplication a place the
        // two could disagree about which bytes a run owns.
        //
        // Harmless when nothing is open: `close_text_run` returns early on
        // an empty box.
        self.close_text_run();
        let Some(mut t) = self.text.take() else {
            return; // unbalanced ET
        };
        if t.origins.is_empty() {
            return; // a text object that showed nothing
        }
        if self.objects.len() >= MAX_OBJECTS {
            self.diag.objects_dropped += 1;
            return;
        }
        // Assemble the box from whichever construction the run's fonts
        // entitled it to (`TextBoundsBasis`, and `TextAccum::basis`'s
        // worst-wins rule).
        //
        // The em-box margin is the largest `Tf` size seen, floored at 1.0
        // so a `BT`…`ET` that never set a font still produces a target big
        // enough to click. `inflate` is isotropic — it subtracts the margin
        // from `min` and adds it to `max` on BOTH axes — which for a single
        // pen-start point is a square of side `2 × margin` centred on that
        // point. That shape is the whole reason the metrics path exists;
        // it survives only where nothing better is available.
        let basis = t.basis();
        let margin = (t.max_font_size).max(1.0);
        let page_bbox = if t.ink.is_empty() {
            t.origins.inflate(margin)
        } else if t.unmetered.is_empty() {
            t.ink
        } else {
            // Mixed: the metered runs contribute their real extent, the
            // unmetered ones their em box, and `basis` already reports the
            // weaker of the two claims for the union.
            t.ink.union(t.unmetered.inflate(margin))
        };
        let bytes = self.span_of(t.token_start, op_index);
        let token_start = t.token_start;
        let ctm = t.ctm;
        // The still-open run was already folded in by the `close_text_run`
        // at the top of this function.
        let runs = std::mem::take(&mut t.runs);
        let (preview, font) = t.finish();
        self.diag.text += 1;
        self.objects.push(VectorObject::Text(TextObject {
            page_bbox,
            runs,
            approximate: true,
            bounds_basis: basis,
            preview,
            font,
            tokens: TokenRange {
                start: token_start,
                end: op_index + 1,
            },
            bytes,
            ctm,
        }));
    }

    // -- Do / image --

    fn do_xobject(&mut self, operands: &[ContentToken], first: usize, op_index: usize) {
        let Some(name) = last_name(operands) else {
            self.diag.unresolved_xobject += 1;
            return;
        };
        match self.xobjects.classify(&name) {
            Some(XObjectShape::Image { pixel_size, object }) => {
                self.emit_image(
                    ImageSource::XObject,
                    self.gs.ctm,
                    unit_square(),
                    pixel_size,
                    object,
                    first,
                    op_index,
                );
            }
            Some(XObjectShape::Form {
                bbox,
                matrix,
                object,
            }) => {
                let ctm = matrix.post_concat(self.gs.ctm);
                let corners = bounds_corners(bbox);
                // A form has no samples (§8.10) — `None`, not `Some((0, 0))`.
                self.emit_image(
                    ImageSource::Form,
                    ctm,
                    corners,
                    None,
                    object,
                    first,
                    op_index,
                );
            }
            None => self.diag.unresolved_xobject += 1,
        }
    }

    /// Emit an image/form object: `local_corners` are the four corners in
    /// the object's own space (unit square, or a form `/BBox`), mapped to
    /// page space by `ctm`; `pixel_size` is the sample count for an image
    /// and `None` for a form.
    #[allow(clippy::too_many_arguments)]
    fn emit_image(
        &mut self,
        source: ImageSource,
        ctm: Matrix,
        local_corners: [Point; 4],
        pixel_size: Option<(u32, u32)>,
        xobject: Option<ObjId>,
        first: usize,
        op_index: usize,
    ) {
        if self.objects.len() >= MAX_OBJECTS {
            self.diag.objects_dropped += 1;
            return;
        }
        let page_bbox = local_corners
            .iter()
            .fold(Bounds::EMPTY, |acc, &c| acc.union_point(ctm.map_point(c)));
        let bytes = self.span_of(first, op_index);
        match source {
            ImageSource::Form => self.diag.forms += 1,
            ImageSource::Inline | ImageSource::XObject => self.diag.images += 1,
        }
        self.objects.push(VectorObject::Image(ImageObject {
            xobject,
            ctm,
            page_bbox,
            source,
            pixel_size,
            tokens: TokenRange {
                start: first,
                end: op_index + 1,
            },
            bytes,
        }));
    }

    /// The byte span in the decoded content buffer from token `start`'s
    /// first byte through token `end`'s last byte.
    fn span_of(&self, start: usize, end: usize) -> ByteSpan {
        let s = self.content.tokens.get(start).map_or(0, |t| t.span.start);
        let e = self
            .content
            .tokens
            .get(end)
            .map_or_else(|| self.content.buf.len(), |t| t.span.end());
        ByteSpan::from_range(s..e)
    }
}

// ---------------------------------------------------------------------------
// Free helpers
// ---------------------------------------------------------------------------

/// Finalize the open subpath of `pa` (if any), pushing it if it has at
/// least one segment (a lone `m` produces no contour, matching the
/// renderer's `PathBuilder` collapse of an empty move).
fn finalize_open_pa(pa: &mut PathAccum) {
    if let Some(open) = pa.open.take()
        && !open.segments.is_empty()
    {
        pa.subpaths.push(open);
    }
}

/// [`finalize_open_pa`] through an `Option<&mut PathAccum>`.
fn finalize_open(pa: Option<&mut PathAccum>) {
    if let Some(pa) = pa {
        finalize_open_pa(pa);
    }
}

/// Collect the numeric operands of an operation, in order (§8.5's operators
/// take numeric operands; a wrong-typed operand is skipped, matching the
/// renderer's tolerance).
fn operand_nums(operands: &[ContentToken]) -> Vec<f64> {
    operands
        .iter()
        .filter_map(|t| match &t.kind {
            ContentTokenKind::Operand(o) => o.as_number(),
            _ => None,
        })
        .collect()
}

/// The NAME operands of an operation, in order.
///
/// Needed because `sc`/`scn` distinguish a colour from a PATTERN by whether a
/// name is present (§8.7.3), and `cs`/`CS` take a name as their only operand.
/// [`operand_nums`] silently drops names, which is right for every other
/// operator and wrong for these.
fn operand_names(operands: &[ContentToken]) -> Vec<Vec<u8>> {
    operands
        .iter()
        .filter_map(|t| match &t.kind {
            ContentTokenKind::Operand(Object::Name(n)) => Some(n.as_bytes().to_vec()),
            _ => None,
        })
        .collect()
}

/// Cut a name at [`MAX_FONT_NAME_BYTES`], on a UTF-8 character boundary.
///
/// `floor_char_boundary` is not stable, so the boundary is found by
/// scanning back from the limit — at most three bytes, since a UTF-8
/// sequence is at most four. Returning a byte-sliced `String` without this
/// would panic on a multi-byte name, which is precisely the adversarial
/// input a hostile `/BaseFont` would carry.
fn truncate_name(name: &str) -> String {
    if name.len() <= MAX_FONT_NAME_BYTES {
        return name.to_owned();
    }
    let mut end = MAX_FONT_NAME_BYTES;
    while end > 0 && !name.is_char_boundary(end) {
        end -= 1;
    }
    name.get(..end).unwrap_or_default().to_owned()
}

/// The last name operand of an operation (`Do`'s XObject name), taken from
/// the end of the run for the same reason the renderer's `last_name` does.
fn last_name(operands: &[ContentToken]) -> Option<Vec<u8>> {
    operands.iter().rev().find_map(|t| match &t.kind {
        ContentTokenKind::Operand(Object::Name(n)) => Some(n.as_bytes().to_vec()),
        _ => None,
    })
}

/// Build a [`PathPaint::Device`] from a device operator's operands.
///
/// The resolved `Rgb` is passed in rather than recomputed, so the honest paint
/// and the legacy `Rgb` field cannot disagree about the same operator — two
/// derivations of one value are two things that can drift.
fn device_paint(space: DevicePaintSpace, nums: &[f64], rgb: Rgb) -> PathPaint {
    PathPaint::Device {
        space,
        comps: nums.to_vec(),
        rgb,
    }
}

/// Set a colour from a single-component (`g`/`G`) operator.
fn set_color(slot: &mut Rgb, f: fn(f32) -> Rgb, nums: &[f64]) {
    if let &[v] = nums {
        *slot = f(v as f32);
    }
}

/// Set a colour from an `rg`/`RG` operator.
fn set_rgb(slot: &mut Rgb, nums: &[f64]) {
    if let &[r, g, b] = nums {
        *slot = Rgb::from_rgb(r as f32, g as f32, b as f32);
    }
}

/// Set a colour from a `k`/`K` operator.
fn set_cmyk(slot: &mut Rgb, nums: &[f64]) {
    if let &[c, m, y, k] = nums {
        *slot = Rgb::from_cmyk(c as f32, m as f32, y as f32, k as f32);
    }
}

/// The user-space unit square (§8.9.4's image-space boundary), as four
/// corners.
fn unit_square() -> [Point; 4] {
    [
        Point::new(0.0, 0.0),
        Point::new(1.0, 0.0),
        Point::new(1.0, 1.0),
        Point::new(0.0, 1.0),
    ]
}

/// The four corners of a [`Bounds`] (empty box → degenerate origin
/// corners, harmless downstream).
fn bounds_corners(b: Bounds) -> [Point; 4] {
    if b.is_empty() {
        return [Point::new(0.0, 0.0); 4];
    }
    [
        Point::new(b.min.x, b.min.y),
        Point::new(b.max.x, b.min.y),
        Point::new(b.max.x, b.max.y),
        Point::new(b.min.x, b.max.y),
    ]
}

/// The page-space bounding box of a set of user-space subpaths under
/// `ctm` — the control-point hull (a conservative superset of the exact
/// curve bounds; a curve never leaves its control hull).
fn subpaths_page_bounds(subpaths: &[Subpath], ctm: Matrix) -> Bounds {
    let mut b = Bounds::EMPTY;
    for sp in subpaths {
        b = b.union_point(ctm.map_point(sp.start));
        for seg in &sp.segments {
            match *seg {
                Segment::Line { to } => b = b.union_point(ctm.map_point(to)),
                Segment::Cubic { c1, c2, to } => {
                    b = b
                        .union_point(ctm.map_point(c1))
                        .union_point(ctm.map_point(c2))
                        .union_point(ctm.map_point(to));
                }
            }
        }
    }
    b
}

/// Whether a subpath is a 4-anchor quad: exactly 3 line segments after the
/// start (start + 3 lines closing back = 4 corners), all straight. An
/// `re` rectangle and a hand-drawn closed 4-line quad both match.
fn subpath_is_quad(sp: &Subpath) -> bool {
    sp.segments.len() == 3
        && sp
            .segments
            .iter()
            .all(|s| matches!(s, Segment::Line { .. }))
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;
    use crate::PdfVersion;
    use crate::object::{Name, ObjId};
    use std::collections::BTreeMap;

    fn model(src: &[u8]) -> PageObjects {
        let cs = ContentStream::parse(src.to_vec()).unwrap();
        decompose(&cs, Matrix::IDENTITY, &NoXObjects)
    }

    // -- colour spaces (Pass 218.0) -----------------------------------------

    /// ★★★ A path painted in a `/Separation` must NOT report the last device
    /// colour as its own.
    ///
    /// # The defect
    ///
    /// The decomposer handled only `g G rg RG k K` and had no arm for
    /// `cs CS sc scn SC SCN`. A spot-coloured path therefore inherited
    /// whatever the previous device operator had set — here a red fill from an
    /// unrelated earlier object — and reported it as its own colour, with
    /// nothing recording that pdfcer did not know.
    ///
    /// The renderer had this exact bug and fixed it on 2026-08-10, recording
    /// that the consequence "was not a missing feature but WRONG PIXELS". The
    /// decomposer never received that fix; this test is it, three weeks later.
    ///
    /// # Why the red fill is in the fixture
    ///
    /// Without it the stale value would be the initial black, which is also
    /// what a plausible-but-wrong implementation would report — so the test
    /// would pass on the bug. The red makes the stale answer *distinctive*.
    #[test]
    fn a_separation_painted_path_does_not_report_the_previous_device_colour() {
        let m = model(b"1 0 0 rg 0 0 10 10 re f /CS0 cs 0.5 scn 20 20 10 10 re f\n");
        let paths: Vec<&PathObject> = m
            .objects
            .iter()
            .filter_map(|o| match o {
                VectorObject::Path(p) => Some(p),
                _ => None,
            })
            .collect();
        assert_eq!(paths.len(), 2, "two filled rectangles");

        // The DEVICE one is unchanged and still fully modelled.
        assert!(
            matches!(paths[0].fill_paint, PathPaint::Device { .. }),
            "a plain `rg` fill must still be Device, got {:?}",
            paths[0].fill_paint
        );

        // The SEPARATION one must say so rather than inherit the red.
        match &paths[1].fill_paint {
            PathPaint::Other {
                space,
                comps,
                pattern,
            } => {
                assert_eq!(space.as_deref(), Some(b"CS0".as_slice()));
                assert_eq!(comps, &[0.5]);
                assert!(!pattern, "a numeric operand is a colour, not a pattern");
            }
            other => panic!("expected Other for a /Separation fill, got {other:?}"),
        }
        assert_eq!(
            paths[1].fill_paint.rgb(),
            None,
            "★ and it must refuse to name a screen colour it does not have — \
             returning the stale red here is the whole defect"
        );
        assert_eq!(
            m.diagnostics.paths_with_undecoded_colour, 1,
            "and the situation is DISCLOSED, not silent"
        );
    }

    /// A pattern fill (`scn` with a NAME operand) is a third thing.
    ///
    /// §8.7.3: a pattern has no colour at all, which is a different refusal
    /// from "a colour pdfcer cannot decode" — a shell may reasonably offer to
    /// replace an undecodable colour and must not offer to replace a pattern
    /// with one.
    #[test]
    fn a_pattern_fill_is_distinguished_from_an_undecodable_colour() {
        let m = model(b"/Pattern cs /P0 scn 0 0 10 10 re f\n");
        let VectorObject::Path(p) = &m.objects[0] else {
            panic!("expected a path")
        };
        match &p.fill_paint {
            PathPaint::Other { pattern, .. } => assert!(*pattern, "a name operand means a pattern"),
            other => panic!("expected Other, got {other:?}"),
        }
    }

    /// The device operators still set BOTH representations, and they agree.
    ///
    /// Two derivations of one value are two things that can drift, so this
    /// pins that they do not.
    #[test]
    fn a_device_fill_agrees_with_the_legacy_rgb_field() {
        let m = model(b"0 0 1 rg 0 0 10 10 re f\n");
        let VectorObject::Path(p) = &m.objects[0] else {
            panic!("expected a path")
        };
        assert_eq!(p.fill_paint.rgb(), Some(p.fill_color));
        assert_eq!(m.diagnostics.paths_with_undecoded_colour, 0);
    }

    // -- the font-resolution test rig ---------------------------------------
    //
    // A hand-built `ObjectGraph` (the same shape `view.rs`'s own tests use)
    // so the DECODING path is exercised without dragging a parsed file into
    // a unit test. Two font resources, chosen to be the two ends of the
    // §9.10.2 ladder:
    //
    //   /F1           Helvetica, a standard-14 simple font — rung 2 via the
    //                 AGL, so ASCII decodes exactly.
    //   /Undecodable  Type0 / Identity-H with an Adobe-Identity-0 descendant
    //                 and NO /ToUnicode — §9.10.2 excludes Identity-H from
    //                 rung 3's first disjunct by name and the descendant
    //                 satisfies neither half of the second, so every code
    //                 reaches the failure clause. Structurally the same case
    //                 `fixtures/synthetic/text/identity-h-no-tounicode.pdf`
    //                 pins for extraction.

    struct TestGraph {
        objects: BTreeMap<ObjId, Object>,
        trailer: Dict,
    }

    impl ObjectGraph for TestGraph {
        fn value(&self, id: ObjId) -> Option<&Object> {
            self.objects.get(&id)
        }
        fn trailer_entry(&self, key: &[u8]) -> Option<&Object> {
            self.trailer.get(key)
        }
    }

    fn dict(entries: &[(&[u8], Object)]) -> Dict {
        let mut d = Dict::new();
        for (k, v) in entries {
            d.insert(Name::from(*k), v.clone());
        }
        d
    }

    fn name(v: &[u8]) -> Object {
        Object::Name(Name::from(v))
    }

    /// A `/Font` resource dictionary holding the two fonts above.
    fn font_resources() -> Dict {
        let helvetica = dict(&[
            (b"Type", name(b"Font")),
            (b"Subtype", name(b"Type1")),
            (b"BaseFont", name(b"Helvetica")),
        ]);
        let descendant = dict(&[
            (b"Type", name(b"Font")),
            (b"Subtype", name(b"CIDFontType2")),
            (b"BaseFont", name(b"NoUnicode")),
            (
                b"CIDSystemInfo",
                Object::Dict(dict(&[
                    (b"Registry", Object::String(b"Adobe".to_vec())),
                    (b"Ordering", Object::String(b"Identity".to_vec())),
                    (b"Supplement", Object::Integer(0)),
                ])),
            ),
        ]);
        let undecodable = dict(&[
            (b"Type", name(b"Font")),
            (b"Subtype", name(b"Type0")),
            (b"BaseFont", name(b"NoUnicode")),
            (b"Encoding", name(b"Identity-H")),
            (
                b"DescendantFonts",
                Object::Array(vec![Object::Dict(descendant)]),
            ),
        ]);
        // /Widthy — a NON-standard-14 simple font that carries its own
        // §9.6.2.1 `/Widths` array and a §9.8 Table 122 descriptor. The
        // dominant real-world simple-font shape, and the one that must
        // reach `TextBoundsBasis::FontMetrics` without any compiled-in AFM
        // help: every code from 'A' (65) up is 500/1000 em wide, the
        // ascent is 0.8 em and the descent −0.2 em.
        let widthy = dict(&[
            (b"Type", name(b"Font")),
            (b"Subtype", name(b"Type1")),
            (b"BaseFont", name(b"NotAStandardFace")),
            (b"FirstChar", Object::Integer(65)),
            (b"Widths", Object::Array(vec![Object::Integer(500); 60])),
            (
                b"FontDescriptor",
                Object::Dict(dict(&[
                    (b"Type", name(b"FontDescriptor")),
                    (b"FontName", name(b"NotAStandardFace")),
                    (b"Flags", Object::Integer(32)),
                    (b"Ascent", Object::Integer(800)),
                    (b"Descent", Object::Integer(-200)),
                ])),
            ),
        ]);
        // /Widthless — the §9.6.2.2-violating shape real producers ship: a
        // font that is neither standard-14 nor carries `/Widths`, so
        // `text_extract` estimates its advances from Helvetica and says so
        // (`FontNote::WidthsEstimated`). Must degrade to
        // `TextBoundsBasis::EstimatedAdvances`, never claim `FontMetrics`.
        let widthless = dict(&[
            (b"Type", name(b"Font")),
            (b"Subtype", name(b"Type1")),
            (b"BaseFont", name(b"AlsoNotStandard")),
        ]);
        // /Cid — a composite font whose DESCENDANT carries `/W`, `/DW` and
        // a descriptor (§9.7.4.3 + §9.8.1's "the descriptor belongs to the
        // descendant"). Proves the metrics path reaches the Identity-H
        // subsetted case, which is what a modern producer emits.
        let cid_descendant = dict(&[
            (b"Type", name(b"Font")),
            (b"Subtype", name(b"CIDFontType2")),
            (b"BaseFont", name(b"ABCDEF+Wide")),
            (
                b"CIDSystemInfo",
                Object::Dict(dict(&[
                    (b"Registry", Object::String(b"Adobe".to_vec())),
                    (b"Ordering", Object::String(b"Identity".to_vec())),
                    (b"Supplement", Object::Integer(0)),
                ])),
            ),
            (b"DW", Object::Integer(1000)),
            // CIDs 1..=3 are 750/1000 em each.
            (
                b"W",
                Object::Array(vec![
                    Object::Integer(1),
                    Object::Integer(3),
                    Object::Integer(750),
                ]),
            ),
            (
                b"FontDescriptor",
                Object::Dict(dict(&[
                    (b"Type", name(b"FontDescriptor")),
                    (b"FontName", name(b"ABCDEF+Wide")),
                    (b"Flags", Object::Integer(4)),
                    (b"Ascent", Object::Integer(900)),
                    (b"Descent", Object::Integer(-300)),
                ])),
            ),
        ]);
        let cid = dict(&[
            (b"Type", name(b"Font")),
            (b"Subtype", name(b"Type0")),
            (b"BaseFont", name(b"ABCDEF+Wide")),
            (b"Encoding", name(b"Identity-H")),
            (
                b"DescendantFonts",
                Object::Array(vec![Object::Dict(cid_descendant)]),
            ),
        ]);
        let fonts = dict(&[
            (b"F1", Object::Dict(helvetica.clone())),
            (b"F2", Object::Dict(helvetica)),
            (b"Undecodable", Object::Dict(undecodable)),
            (b"Widthy", Object::Dict(widthy)),
            (b"Widthless", Object::Dict(widthless)),
            (b"Cid", Object::Dict(cid)),
        ]);
        dict(&[(b"Font", Object::Dict(fonts))])
    }

    fn test_graph() -> TestGraph {
        TestGraph {
            objects: BTreeMap::new(),
            trailer: Dict::new(),
        }
    }

    /// Decompose `src` with a real [`DocumentFonts`] over [`font_resources`].
    fn model_with_fonts(src: &[u8]) -> PageObjects {
        let graph = test_graph();
        let view = DocumentView::new(&graph, b"", PdfVersion { major: 1, minor: 7 });
        let resources = font_resources();
        let fonts = DocumentFonts::new(&view, &resources);
        let cs = ContentStream::parse(src.to_vec()).unwrap();
        decompose_with_fonts(&cs, Matrix::IDENTITY, &NoXObjects, &fonts)
    }

    fn texts(m: &PageObjects) -> Vec<&TextObject> {
        m.objects
            .iter()
            .filter_map(|o| match o {
                VectorObject::Text(t) => Some(t),
                _ => None,
            })
            .collect()
    }

    fn paths(m: &PageObjects) -> Vec<&PathObject> {
        m.objects
            .iter()
            .filter_map(|o| match o {
                VectorObject::Path(p) => Some(p),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_stroked_line_is_one_path_object_with_two_anchors() {
        let m = model(b"10 20 m 100 200 l S");
        let ps = paths(&m);
        assert_eq!(ps.len(), 1);
        let sp = &ps[0].subpaths;
        assert_eq!(sp.len(), 1);
        assert_eq!(sp[0].start, Point::new(10.0, 20.0));
        assert_eq!(
            sp[0].segments,
            vec![Segment::Line {
                to: Point::new(100.0, 200.0)
            }]
        );
        assert!(!sp[0].closed);
        assert!(ps[0].style.stroke && ps[0].style.fill.is_none());
    }

    #[test]
    fn re_is_a_closed_quad_and_fills_nonzero() {
        let m = model(b"10 10 80 40 re f");
        let ps = paths(&m);
        assert_eq!(ps.len(), 1);
        assert!(ps[0].is_quad());
        assert_eq!(ps[0].style.fill, Some(FillRule::NonZero));
        // page bbox of the rectangle
        assert_eq!(ps[0].page_bbox.min, Point::new(10.0, 10.0));
        assert_eq!(ps[0].page_bbox.max, Point::new(90.0, 50.0));
    }

    #[test]
    fn v_and_y_control_points_come_from_the_shared_primitives() {
        // `v`: first control is the current point (10,10).
        let m = model(b"10 10 m 20 30 40 50 v S");
        let ps = paths(&m);
        assert_eq!(
            ps[0].subpaths[0].segments[0],
            Segment::Cubic {
                c1: Point::new(10.0, 10.0),
                c2: Point::new(20.0, 30.0),
                to: Point::new(40.0, 50.0),
            }
        );
        // `y`: second control is the endpoint.
        let m2 = model(b"10 10 m 20 30 40 50 y S");
        assert_eq!(
            paths(&m2)[0].subpaths[0].segments[0],
            Segment::Cubic {
                c1: Point::new(20.0, 30.0),
                c2: Point::new(40.0, 50.0),
                to: Point::new(40.0, 50.0),
            }
        );
    }

    #[test]
    fn cm_transforms_the_captured_ctm_and_page_space() {
        // Scale by 2 then draw a unit line: page-space nodes are doubled.
        let m = model(b"2 0 0 2 5 5 cm 0 0 m 10 0 l S");
        let ps = paths(&m);
        let page = ps[0].page_subpaths();
        assert_eq!(page[0].start, Point::new(5.0, 5.0)); // (0,0)*2 + (5,5)
        assert_eq!(page[0].segments[0].end(), Point::new(25.0, 5.0)); // (10,0)*2+(5,5)
        // but the stored user-space nodes are untransformed
        assert_eq!(ps[0].subpaths[0].start, Point::new(0.0, 0.0));
    }

    #[test]
    fn q_q_restores_the_ctm_so_a_later_object_is_untransformed() {
        let m = model(b"q 3 0 0 3 0 0 cm 0 0 m 1 0 l S Q 0 0 m 1 0 l S");
        let ps = paths(&m);
        assert_eq!(ps.len(), 2);
        // first object scaled x3, second at identity
        assert_eq!(
            ps[0].page_subpaths()[0].segments[0].end(),
            Point::new(3.0, 0.0)
        );
        assert_eq!(
            ps[1].page_subpaths()[0].segments[0].end(),
            Point::new(1.0, 0.0)
        );
    }

    #[test]
    fn multiple_subpaths_and_close_operators() {
        // Two subpaths, the second closed by `s`.
        let m = model(b"0 0 m 10 0 l 0 0 m 5 5 l 10 0 l s");
        let ps = paths(&m);
        assert_eq!(ps.len(), 1);
        assert_eq!(ps[0].subpaths.len(), 2);
        assert!(ps[0].subpaths[1].closed, "s closes the last subpath");
    }

    #[test]
    fn h_closes_and_reopens_a_subpath() {
        let m = model(b"0 0 m 10 0 l 10 10 l h 20 20 l S");
        let ps = paths(&m);
        assert_eq!(ps[0].subpaths.len(), 2);
        assert!(ps[0].subpaths[0].closed);
        // the reopened subpath starts at the closed subpath's start (0,0)
        assert_eq!(ps[0].subpaths[1].start, Point::new(0.0, 0.0));
    }

    #[test]
    fn n_paints_nothing_but_is_still_a_selectable_object() {
        let m = model(b"0 0 m 10 10 l n");
        let ps = paths(&m);
        assert_eq!(ps.len(), 1);
        assert!(ps[0].style.is_invisible());
    }

    #[test]
    fn token_range_covers_construction_through_paint() {
        // tokens: 0:"10" 1:"20" 2:m 3:"100" 4:"200" 5:l 6:S
        let cs = ContentStream::parse(b"10 20 m 100 200 l S".to_vec()).unwrap();
        let m = decompose(&cs, Matrix::IDENTITY, &NoXObjects);
        let p = match &m.objects[0] {
            VectorObject::Path(p) => p,
            _ => panic!(),
        };
        assert_eq!(p.tokens.start, 0);
        assert_eq!(p.tokens.end, 7); // exclusive, past the S at index 6
        // and the byte span slices to the object's source text
        assert_eq!(p.bytes.slice(&cs.buf).unwrap(), b"10 20 m 100 200 l S");
    }

    #[test]
    fn text_object_is_bbox_and_range_only_and_flagged_approximate() {
        let m = model(b"BT /F1 12 Tf 72 700 Td (Hi) Tj ET");
        let texts = texts(&m);
        assert_eq!(texts.len(), 1);
        assert!(texts[0].approximate);
        // origin (72,700) inflated by the 12 pt font size
        assert!(texts[0].page_bbox.contains(Point::new(72.0, 700.0)));
    }

    // -- text bbox geometry (ui-spec §E.1) ----------------------------------

    /// **The headline fix.** A simple font with its own `/Widths` and a
    /// descriptor produces a box that starts at the pen, ends past the last
    /// glyph, and is as tall as the font says — not a square around the pen.
    ///
    /// `/Widthy` is 500/1000 em per code with ascent 0.8 em and descent
    /// −0.2 em, so `(ABCD) Tj` at 10 pt from (100, 500) must give exactly
    /// `100,498 → 120,508`. Every number is checked, because the failure
    /// this replaces was not "slightly off" — it was a box in the wrong
    /// PLACE, and only pinning all four edges catches a regression to it.
    #[test]
    fn a_run_with_real_widths_is_bounded_by_its_accumulated_advances() {
        let m = model_with_fonts(b"BT /Widthy 10 Tf 100 500 Td (ABCD) Tj ET");
        let t = texts(&m).remove(0);
        assert_eq!(t.bounds_basis, TextBoundsBasis::FontMetrics);
        // Still `approximate`: metrics-derived is not measured ink (§E.2).
        assert!(t.approximate);
        let b = t.page_bbox;
        assert!((b.min.x - 100.0).abs() < 1e-6, "{b:?}");
        assert!((b.max.x - 120.0).abs() < 1e-6, "4 × 0.5 em at 10 pt: {b:?}");
        assert!((b.min.y - 498.0).abs() < 1e-6, "descent −0.2 em: {b:?}");
        assert!((b.max.y - 508.0).abs() < 1e-6, "ascent 0.8 em: {b:?}");
    }

    /// The regression that motivated the change, stated as a property
    /// rather than as numbers: the pen start is the box's LEFT edge, and a
    /// point three-quarters of the way along the run is INSIDE the box.
    ///
    /// Under the old origin-inflate construction both were false — the box
    /// began one em to the left of the pen and ended one em to its right,
    /// so a four-glyph run's last two glyphs sat outside the box that
    /// claimed to bound them.
    #[test]
    fn the_box_covers_the_glyphs_and_not_the_paper_before_them() {
        let m = model_with_fonts(b"BT /Widthy 10 Tf 100 500 Td (ABCDEFGH) Tj ET");
        let b = texts(&m).remove(0).page_bbox;
        // 8 glyphs × 5 pt = 40 pt of run.
        assert!(
            b.contains(Point::new(130.0, 502.0)),
            "a point three-quarters along the run must be inside: {b:?}"
        );
        assert!(
            !b.contains(Point::new(95.0, 502.0)),
            "blank paper 5 pt before the pen must be outside: {b:?}"
        );
        assert!(
            !b.contains(Point::new(145.0, 502.0)),
            "blank paper 5 pt past the run must be outside: {b:?}"
        );
    }

    /// A **composite** font's advances come from the descendant's `/W`
    /// array (§9.7.4.3) and its vertical extent from the descendant's
    /// `/FontDescriptor` (§9.8.1) — a dictionary lookup, not a font-program
    /// read, so the Identity-H subsetted case every modern producer emits
    /// is measured, not fallen back on.
    ///
    /// `/Cid` is 750/1000 em for CIDs 1-3, ascent 0.9, descent −0.3. Codes
    /// are TWO bytes (§9.7.6.2), so `<000100020003>` is three glyphs:
    /// 3 × 0.75 × 20 = 45 pt wide, 18 pt above and 6 pt below the baseline.
    #[test]
    fn a_composite_run_is_measured_from_the_descendants_w_array() {
        let m = model_with_fonts(b"BT /Cid 20 Tf 10 100 Td <000100020003> Tj ET");
        let t = texts(&m).remove(0);
        assert_eq!(t.bounds_basis, TextBoundsBasis::FontMetrics);
        let b = t.page_bbox;
        assert!((b.min.x - 10.0).abs() < 1e-6, "{b:?}");
        assert!((b.max.x - 55.0).abs() < 1e-4, "3 × 0.75 em at 20 pt: {b:?}");
        assert!((b.min.y - 94.0).abs() < 1e-4, "{b:?}");
        assert!((b.max.y - 118.0).abs() < 1e-4, "{b:?}");
    }

    /// A **standard-14** font that omits `/Widths` entirely (§9.6.2.2's
    /// permitted shape) is still measured: the widths come from the
    /// compiled-in AFM tables and the vertical extent from the compiled-in
    /// descriptor, so the basis is the good one with no dictionary metrics
    /// present at all.
    ///
    /// Helvetica's `H`=722, `i`=222, at 20 pt ⇒ 18.88 pt of run; ascent
    /// 718, descent −207.
    #[test]
    fn a_standard_14_font_with_no_widths_is_measured_from_the_afm_tables() {
        let m = model_with_fonts(b"BT /F1 20 Tf 50 50 Td (Hi) Tj ET");
        let t = texts(&m).remove(0);
        assert_eq!(t.bounds_basis, TextBoundsBasis::FontMetrics);
        let b = t.page_bbox;
        assert!((b.max.x - b.min.x - 18.88).abs() < 1e-3, "{b:?}");
        assert!((b.max.y - 50.0 - 14.36).abs() < 1e-3, "ascent: {b:?}");
        assert!((50.0 - b.min.y - 4.14).abs() < 1e-3, "descent: {b:?}");
    }

    /// A font that is neither standard-14 nor carries `/Widths` has its
    /// advances ESTIMATED by `text_extract` (`FontNote::WidthsEstimated`).
    /// The box is still the right shape — it starts at the pen and grows
    /// with the run — but the basis must say the widths were guessed rather
    /// than claim `FontMetrics`.
    #[test]
    fn estimated_widths_degrade_the_basis_rather_than_being_passed_off() {
        let m = model_with_fonts(b"BT /Widthless 10 Tf 0 0 Td (AAAA) Tj ET");
        let t = texts(&m).remove(0);
        assert_eq!(t.bounds_basis, TextBoundsBasis::EstimatedAdvances);
        let b = t.page_bbox;
        assert!((b.min.x - 0.0).abs() < 1e-6, "{b:?}");
        assert!(b.max.x > 10.0, "four glyphs are wider than one em: {b:?}");
    }

    /// **The fallback must still work.** `decompose` with [`NoFonts`] — a
    /// damaged file, a unit test, a caller with no document — produces the
    /// old origin-hull box, flagged [`TextBoundsBasis::EmBox`] so the
    /// disclosure shown for it is the blunt one.
    ///
    /// A hit target that is loose beats one that panics or vanishes: this
    /// is the case where pdfcer genuinely does not know where the text ends,
    /// and the honest answer is a coarse box that says it is coarse.
    #[test]
    fn with_no_font_the_box_falls_back_to_the_origin_em_square_and_says_so() {
        let m = model(b"BT /F1 12 Tf 72 700 Td (Hi) Tj ET");
        let t = texts(&m).remove(0);
        assert_eq!(t.bounds_basis, TextBoundsBasis::EmBox);
        let b = t.page_bbox;
        // Exactly the pre-metrics geometry: a 24 pt square centred on the
        // pen start.
        assert!((b.min.x - 60.0).abs() < 1e-6, "{b:?}");
        assert!((b.max.x - 84.0).abs() < 1e-6, "{b:?}");
        assert!((b.min.y - 688.0).abs() < 1e-6, "{b:?}");
        assert!((b.max.y - 712.0).abs() < 1e-6, "{b:?}");
    }

    /// A text object whose runs MIX measurable and unmeasurable fonts gets
    /// the union of both constructions, and reports the WEAKER basis: part
    /// of the box the operator is looking at really is an em-box guess, and
    /// a disclosure describing only the good half would be the more
    /// misleading of the two sentences available.
    ///
    /// `/Nope` is not in the resource dictionary, so its run has no
    /// decoder and no widths.
    #[test]
    fn a_mixed_run_reports_the_weaker_basis_and_unions_both_boxes() {
        let m = model_with_fonts(
            b"BT /Widthy 10 Tf 100 500 Td (AB) Tj /Nope 10 Tf 200 0 Td (CD) Tj ET",
        );
        let t = texts(&m).remove(0);
        assert_eq!(t.bounds_basis, TextBoundsBasis::EmBox);
        let b = t.page_bbox;
        // The measured run's left edge is the pen at x=100; the unmeasured
        // run at x=300 contributes an em box, so the right edge is 310.
        assert!((b.min.x - 100.0).abs() < 1e-6, "{b:?}");
        assert!((b.max.x - 310.0).abs() < 1e-6, "{b:?}");
    }

    /// A **multi-run** text object: several show operators inside one
    /// `BT`…`ET`, positioned by `Td`/`T*`/`TJ`, are all laid out into one
    /// box. Two lines 12 pt apart, so the box spans both baselines' extents
    /// and is wider than either line alone.
    #[test]
    fn a_multi_run_text_object_bounds_every_run() {
        let m = model_with_fonts(b"BT /Widthy 10 Tf 12 TL 100 500 Td (AB) Tj T* (ABCDEF) Tj ET");
        let t = texts(&m).remove(0);
        assert_eq!(t.bounds_basis, TextBoundsBasis::FontMetrics);
        let b = t.page_bbox;
        // Widest line: 6 glyphs × 5 pt from x=100.
        assert!((b.min.x - 100.0).abs() < 1e-6, "{b:?}");
        assert!((b.max.x - 130.0).abs() < 1e-6, "{b:?}");
        // Top from the first line's ascent (500 + 8), bottom from the
        // second line's descent (488 − 2).
        assert!((b.max.y - 508.0).abs() < 1e-6, "{b:?}");
        assert!((b.min.y - 486.0).abs() < 1e-6, "{b:?}");
    }

    /// The layout parameters §9.4.4 folds into the advance are all applied:
    /// `Tc` widens every glyph's step, `Tz` scales the whole displacement,
    /// and a `TJ` offset moves the pen between glyphs.
    ///
    /// Baseline `(AB)` at 10 pt with /Widthy is 10 pt wide. With `Tc 2`
    /// each of the two steps grows by 2, so the run's advance is 14 — but
    /// the BOX ends at the last glyph's right edge, which is the first
    /// glyph's advance (7) plus the second glyph's width (5) = 12.
    #[test]
    fn character_spacing_and_horizontal_scaling_enter_the_box() {
        /// The right edge of the one text object `src` decomposes to.
        fn right_edge(src: &[u8]) -> f64 {
            let m = model_with_fonts(src);
            texts(&m).remove(0).page_bbox.max.x
        }

        assert!((right_edge(b"BT /Widthy 10 Tf 0 0 Td (AB) Tj ET") - 10.0).abs() < 1e-6);
        assert!((right_edge(b"BT /Widthy 10 Tf 2 Tc 0 0 Td (AB) Tj ET") - 12.0).abs() < 1e-6);
        // Tz 50 halves every horizontal displacement AND the glyph's own
        // width (both are inside Trm's `a` element / the `× Th` product).
        assert!((right_edge(b"BT /Widthy 10 Tf 50 Tz 0 0 Td (AB) Tj ET") - 5.0).abs() < 1e-6);
        // A TJ offset of −1000 (one em, subtracted from the horizontal
        // coordinate ⇒ a move to the RIGHT) opens a 10 pt gap before the
        // second glyph, so the run ends at 5 + 10 + 5 = 20.
        assert!((right_edge(b"BT /Widthy 10 Tf 0 0 Td [(A) -1000 (B)] TJ ET") - 20.0).abs() < 1e-6);
    }

    /// A scaling/translating `Tm` and a `cm` both reach the box, because
    /// every glyph corner is mapped through `Trm = params × Tm × CTM`
    /// (§9.4.4) rather than through the CTM alone. The pre-metrics box
    /// folded neither into its inflation, which is why a text object inside
    /// a scaled form could not be clicked at all.
    #[test]
    fn the_text_matrix_and_ctm_both_scale_the_box() {
        // Tm doubles: a 10 pt (AB) run becomes 20 pt wide starting at 100.
        let m = model_with_fonts(b"BT /Widthy 10 Tf 2 0 0 2 100 200 Tm (AB) Tj ET");
        let b = texts(&m).remove(0).page_bbox;
        assert!((b.min.x - 100.0).abs() < 1e-6, "{b:?}");
        assert!((b.max.x - 120.0).abs() < 1e-6, "{b:?}");
        assert!((b.max.y - 200.0 - 16.0).abs() < 1e-6, "ascent × 2: {b:?}");

        // A `cm` scale multiplies on top of the same run.
        let m = model_with_fonts(b"3 0 0 3 0 0 cm BT /Widthy 10 Tf 0 0 Td (AB) Tj ET");
        let b = texts(&m).remove(0).page_bbox;
        assert!((b.max.x - 30.0).abs() < 1e-6, "{b:?}");
    }

    /// `Tc`/`Tw`/`Tz`/`Ts` are TEXT STATE, which §9.3 makes part of the
    /// graphics state — so `q`/`Q` saves and restores them. A `Tz` set
    /// inside a `q`…`Q` must not leak out and squeeze a later text object.
    #[test]
    fn text_state_parameters_are_saved_and_restored_by_q_and_capital_q() {
        let m = model_with_fonts(b"q 50 Tz Q BT /Widthy 10 Tf 0 0 Td (AB) Tj ET");
        let b = texts(&m).remove(0).page_bbox;
        assert!(
            (b.max.x - 10.0).abs() < 1e-6,
            "the Tz inside q/Q must not survive it: {b:?}"
        );
    }

    /// `'` is `T*` then `Tj` and `"` is `aw Tw`, `ac Tc`, `T*`, `Tj`
    /// (§9.4.3 Table 109). Both move to the next line FIRST, so a box that
    /// claims to be where the text is has to account for the leading.
    #[test]
    fn the_quote_operators_advance_a_line_before_showing() {
        let m = model_with_fonts(b"BT /Widthy 10 Tf 20 TL 0 100 Td (AB) ' ET");
        let b = texts(&m).remove(0).page_bbox;
        // The run is shown at y = 100 − 20 = 80, so the top of the box is
        // 80 + 8 (ascent), not 108.
        assert!((b.max.y - 88.0).abs() < 1e-6, "{b:?}");
        assert!((b.min.y - 78.0).abs() < 1e-6, "{b:?}");
    }

    /// The `Tf` operands are read from the STREAM, so the resource name and
    /// size are known even with no document behind the walk — but no
    /// typeface is claimed and no decoding is attempted, and the preview
    /// says which of those it is.
    #[test]
    fn without_a_font_resolver_the_preview_is_unavailable_not_empty() {
        let m = model(b"BT /F1 12 Tf 72 700 Td (Hi) Tj ET");
        let t = texts(&m).remove(0);
        assert_eq!(t.preview, TextPreview::Unavailable);
        let font = t.font.as_ref().expect("the /Tf is in the stream");
        assert_eq!(font.resource, "F1");
        assert_eq!(font.size, 12.0);
        // No resolver ⇒ no /BaseFont claim. `F1` is not evidence of a
        // typeface and must never be presented as one.
        assert_eq!(font.base_font, None);
    }

    /// A `BT`/`ET` that positions but never shows a string is `Empty` — a
    /// different fact from "pdfcer did not look", and the two must not
    /// collapse.
    #[test]
    fn a_text_object_that_shows_nothing_is_empty_not_unavailable() {
        // A `Tj` with an empty string still records an origin (so the
        // object exists) but shows no codes.
        let m = model(b"BT /F1 12 Tf 72 700 Td () Tj ET");
        let t = texts(&m).remove(0);
        assert_eq!(t.preview, TextPreview::Empty);
    }

    /// With a resolver in scope the shown string decodes through the SAME
    /// §9.10.2 ladder `extract-text` climbs, for `Tj`, `TJ`, `'` and `"`
    /// alike — every string operand in the run is shown text.
    #[test]
    fn show_operators_decode_through_the_extract_font_ladder() {
        // `TJ`'s kerning numbers are positioning, not text: they contribute
        // nothing to the preview (no derived spaces — see TextPreview).
        let m = model_with_fonts(b"BT /F1 12 Tf 10 10 Td [(He) -120 (llo)] TJ ( there) Tj ET");
        let t = texts(&m).remove(0);
        match &t.preview {
            TextPreview::Decoded {
                text,
                truncated,
                lossy,
            } => {
                assert_eq!(text, "Hello there");
                assert!(!truncated);
                assert!(!lossy);
            }
            other => panic!("expected a decoded preview, got {other:?}"),
        }
        assert_eq!(
            t.font.as_ref().and_then(|f| f.base_font.clone()),
            Some("Helvetica".to_owned())
        );
    }

    /// A font whose encoding defeats the ladder must report
    /// `Undecodable` — never a row of replacement characters, which reads
    /// as a pdfcer bug rather than as an honest "this cannot be read".
    #[test]
    fn a_font_whose_encoding_defeats_decoding_reports_undecodable() {
        // `Identity-H` with no `/ToUnicode` and an `Adobe-Identity-0`
        // descendant satisfies neither disjunct of §9.10.2's rung 3, so
        // every code reaches the failure clause (the same property
        // `fixtures/synthetic/text/identity-h-no-tounicode.pdf` pins for
        // extraction).
        let m = model_with_fonts(b"BT /Undecodable 12 Tf 10 10 Td <00480049> Tj ET");
        let t = texts(&m).remove(0);
        assert_eq!(t.preview, TextPreview::Undecodable);
        // The font is still named — knowing WHICH font cannot be read is
        // most of the value of the disclosure.
        assert_eq!(
            t.font.as_ref().and_then(|f| f.base_font.clone()),
            Some("NoUnicode".to_owned())
        );
    }

    /// The memory bound, asserted rather than trusted: a long string is cut
    /// at `MAX_TEXT_PREVIEW_CHARS` and SAYS it was cut.
    #[test]
    fn a_long_string_is_truncated_at_the_documented_cap_and_discloses_it() {
        let long = "A".repeat(MAX_TEXT_PREVIEW_CHARS * 4);
        let src = format!("BT /F1 12 Tf 10 10 Td ({long}) Tj ET");
        let m = model_with_fonts(src.as_bytes());
        let t = texts(&m).remove(0);
        match &t.preview {
            TextPreview::Decoded {
                text, truncated, ..
            } => {
                assert_eq!(text.chars().count(), MAX_TEXT_PREVIEW_CHARS);
                assert!(truncated, "a cut preview must disclose the cut");
            }
            other => panic!("expected a decoded preview, got {other:?}"),
        }
    }

    /// The font is the one in effect at the FIRST show operator, not the
    /// last — the object is identified by the run it starts with, which is
    /// the run the preview previews.
    #[test]
    fn the_captured_font_is_the_one_at_the_first_show_operator() {
        let m = model_with_fonts(b"BT /F1 12 Tf 10 10 Td (a) Tj /F2 30 Tf (b) Tj ET");
        let t = texts(&m).remove(0);
        let font = t.font.as_ref().expect("a font");
        assert_eq!(font.resource, "F1");
        assert_eq!(font.size, 12.0);
    }

    /// `q`/`Q` save and restore the font, because the font is part of the
    /// text state and therefore part of the graphics state (§9.3).
    #[test]
    fn q_q_restores_the_font_resource() {
        let m = model_with_fonts(
            b"/F2 30 Tf q /F1 12 Tf BT 10 10 Td (a) Tj ET Q BT 20 20 Td (b) Tj ET",
        );
        let ts = texts(&m);
        assert_eq!(ts.len(), 2);
        assert_eq!(
            ts[0].font.as_ref().map(|f| f.resource.clone()),
            Some("F1".to_owned())
        );
        // After `Q` the outer `/F2 30 Tf` is in effect again.
        assert_eq!(
            ts[1].font.as_ref().map(|f| f.resource.clone()),
            Some("F2".to_owned())
        );
        assert_eq!(ts[1].font.as_ref().map(|f| f.size), Some(30.0));
    }

    #[test]
    fn inline_image_is_one_image_object_bounded_by_the_unit_square_ctm() {
        // Scale 100x50, translate (10,20): the inline image fills that box.
        let m = model(b"100 0 0 50 10 20 cm BI /W 1 /H 1 /CS /G /BPC 8 ID \x00 EI");
        let imgs: Vec<_> = m
            .objects
            .iter()
            .filter_map(|o| match o {
                VectorObject::Image(i) => Some(i),
                _ => None,
            })
            .collect();
        assert_eq!(imgs.len(), 1);
        assert_eq!(imgs[0].source, ImageSource::Inline);
        assert_eq!(imgs[0].page_bbox.min, Point::new(10.0, 20.0));
        assert_eq!(imgs[0].page_bbox.max, Point::new(110.0, 70.0));
        // §8.9.7 Table 93: `/W`/`/H` are normalized to `/Width`/`/Height` by
        // the tokenizer, so the sample count is read with no resolver at all.
        assert_eq!(imgs[0].pixel_size, Some((1, 1)));
    }

    /// The sample count is `None`, not a guess, when the dictionary does not
    /// carry a usable `/Width`+`/Height` pair (§8.9.5 Table 89 requires
    /// both, as integers).
    #[test]
    fn a_malformed_inline_image_reports_no_pixel_size() {
        // `/H` absent: an unfiltered inline image with no computable length
        // still tokenizes (the scan finds `EI`), and the object is emitted
        // with an honest `None` rather than half a size.
        let m = model(b"100 0 0 50 10 20 cm BI /W 4 /CS /G /BPC 8 ID \x00 EI");
        let imgs: Vec<_> = m
            .objects
            .iter()
            .filter_map(|o| match o {
                VectorObject::Image(i) => Some(i),
                _ => None,
            })
            .collect();
        assert_eq!(imgs.len(), 1);
        assert_eq!(imgs[0].pixel_size, None);
    }

    #[test]
    fn do_image_and_form_classified_via_the_resolver() {
        struct Stub;
        impl XObjectResolver for Stub {
            fn classify(&self, name: &[u8]) -> Option<XObjectShape> {
                match name {
                    b"Im0" => Some(XObjectShape::Image {
                        pixel_size: Some((640, 480)),
                        object: Some(ObjId::new(7, 0)),
                    }),
                    b"Fm0" => Some(XObjectShape::Form {
                        bbox: Bounds {
                            min: Point::new(0.0, 0.0),
                            max: Point::new(4.0, 2.0),
                        },
                        matrix: Matrix::IDENTITY,
                        object: Some(ObjId::new(8, 0)),
                    }),
                    _ => None,
                }
            }
        }
        let cs = ContentStream::parse(b"1 0 0 1 5 5 cm /Im0 Do /Fm0 Do /Zz Do".to_vec()).unwrap();
        let m = decompose(&cs, Matrix::IDENTITY, &Stub);
        let imgs: Vec<_> = m
            .objects
            .iter()
            .filter_map(|o| match o {
                VectorObject::Image(i) => Some(i),
                _ => None,
            })
            .collect();
        assert_eq!(imgs.len(), 2);
        assert_eq!(imgs[0].source, ImageSource::XObject);
        // §8.9.5 Table 89's sample count travels with the classification.
        assert_eq!(imgs[0].pixel_size, Some((640, 480)));
        assert_eq!(imgs[1].source, ImageSource::Form);
        assert_eq!(imgs[1].page_bbox.max, Point::new(9.0, 7.0)); // (4,2)+(5,5)
        // A form has no samples (§8.10) — never `Some((0, 0))`.
        assert_eq!(imgs[1].pixel_size, None);
        assert_eq!(m.diagnostics.unresolved_xobject, 1); // /Zz
    }

    #[test]
    fn unbalanced_q_and_missing_current_point_are_counted_not_panicked() {
        let m = model(b"Q Q 10 20 l S");
        assert_eq!(m.diagnostics.unbalanced_q, 2);
        assert_eq!(m.diagnostics.segment_without_current, 1);
        assert!(paths(&m).is_empty());
    }

    #[test]
    fn a_lone_move_then_paint_emits_no_object() {
        let m = model(b"10 10 m S");
        assert!(paths(&m).is_empty());
    }
}
