//! # Redaction — true content removal (ISO 32000-1 §12.5.6.23)
//!
//! The **one deliberately destructive** subsystem in pdfcer, and the one
//! where *correctness IS security*. Every other operation honours the
//! §5 minimal-diff / round-trip invariant; redaction is the single,
//! explicitly-named exception (R35, `ARCHITECTURE.md` §5 corollary):
//! applying a redaction must **truly remove** the covered content, not
//! visually mask it.
//!
//! ## The cardinal rule, above everything
//!
//! **pdfcer must NEVER claim content is redacted when it is not.**
//! Under-redaction that is *disclosed* or *refused* is acceptable;
//! silent under-redaction is a catastrophic failure. Every carrier this
//! module cannot fully scrub in a given build is reported to the
//! operator as an un-redacted residual ([`RedactionReport`]), never
//! silently left. This is "fuzzy, never sneaky" (rule 4) at maximum
//! force.
//!
//! ## The spec frame — outcome-bound, method-deferred
//!
//! §12.5.6.23 is explicit that content removal "is application-specific"
//! and specifies **no removal algorithm**. What it *does* impose are four
//! `shall`-strength OUTCOME constraints, which are pdfcer's acceptance
//! test rather than an algorithm to copy:
//!
//! 1. remove **all traces** of the specified content, plus the /Redact
//!    annotation itself;
//! 2. image data **shall be destroyed** in-region — "clipping or image
//!    masks shall not be used to hide that data";
//! 3. remove the /Redact annotations after applying;
//! 4. be **diligent** about all content that can exist — XFA and XMP
//!    named explicitly.
//!
//! The removal MECHANICS are assembled in the spec RAG's derived
//! consolidator `iso32000__ref__redaction_removal.md`; this module is
//! their enactment.
//!
//! ## What this cut does, and what it discloses instead of doing
//!
//! | Concern | This build |
//! |---|---|
//! | Text glyphs in-region | **removed** — advance-preserving content-stream surgery (§3 below) |
//! | Surviving text on the same line | **kept in place** — the removed run is replaced by an equivalent `TJ` advance |
//! | Object streams holding a removed/edited object | **decomposed** (§7.5.7 Strategy B — promote survivors, drop container) so no removed byte survives compressed |
//! | Overlapping annotations (& their /AP/Contents/RC) | **removed** (the stricter Acrobat-parity reading) |
//! | `/Info` and XMP strings duplicating redacted text | **scrubbed** (the redacted characters are known — the interpreter decodes them while removing them) |
//! | Prior incremental revisions | **dropped** — apply forces a FULL REWRITE (R35), never incremental |
//! | Images intersecting a region | **samples DESTROYED** — decoded, the covered cells overwritten, re-encoded losslessly; a wholly covered placement is removed outright; a shared image is copied-on-write; a placement pdfcer cannot decode RETAINS its mark and is disclosed by name (`redact_image`) |
//! | Form-XObject content in-region | **disclosed** — not surgically redacted this cut (verify manually) |
//! | Vector paths in-region | **CUT** — strokes are cut against the region expanded by the stroke width, fills are clipped to the region's complement, and a path wholly inside is deleted (`redact_vector`); a malformed path object pdfcer cannot rewrite as a unit is counted and disclosed as a residual |
//! | `sh` shading paints whose clip meets a region | **disclosed, by count** — a shading fills its clip (§8.7.4.5.1), tracked here as a bounding box; not cut this build |
//! | Overlay marking (Table 192 ladder) | `/OverlayText` **burnt in** (via §12.7.3.3 variable text, `/DA`-formatted, `/Q`-justified); `/IC` filled under it; **absent `/IC` ⇒ TRANSPARENT**, per Table 192; `/RO` **not drawn** — disclosed, falls back to a plain box; `/Repeat` **ignored** — disclosed |
//! | XFA / file attachments / structure-tree ActualText / thumbnails | **detected + disclosed** (not asserted-absent) |
//!
//! ## §3 — the advance-preservation hazard, stated once
//!
//! Deleting a show operator mid-line shifts every subsequent
//! advance-relative glyph LEFT by the removed run's advance (§9.4.4:
//! painting a glyph advances `Tm` by `tx = ((w0)·Tfs + Tc + Tw)·Th`).
//! A naive deletion therefore *moves the survivors* — a correctness
//! failure that "looks almost right". The fix (approach 1 of the RAG's
//! three): replace the removed run with a `TJ` numeric adjustment that
//! consumes the **exact same** `tx`, so `Tm` advances identically and
//! the survivors stay put. `TJ` numbers are thousandths of text space,
//! subtracted (§9.4.3), so the adjustment for a removed run of total
//! advance `Σtx` (text-line units) is `N = −Σtx · 1000 / (Tfs·Th)`.
//!
//! **The security guarantee is independent of width accuracy.** Whether
//! a survivor ends up one point off has no bearing on whether the
//! redacted glyph's bytes are gone — they are removed from the show
//! string regardless. Width precision affects only the *cosmetic*
//! quality of advance preservation, so an estimated width (disclosed) is
//! never a security regression.
//!
//! ## Spec sources (PDF-spec RAG, ISO 32000-1:2008)
//!
//! - `iso32000__s__12.5.6.23.md` — the /Redact mark, Table 192, the four
//!   `shall` outcome constraints, the overlay precedence ladder.
//! - `iso32000__ref__redaction_removal.md` — the derived removal
//!   mechanics: content-stream text surgery (§9.4/§8.2), object-stream
//!   container decomposition (§7.5.7/§7.5.8), image re-encode/refuse, the
//!   carrier sweep, the forced-full-rewrite rule.
//! - `iso32000__s__9.4.md` — the §9.4.4 advance formula this module's
//!   surgery is built on.

use std::collections::BTreeSet;

use crate::content::{ContentStream, ContentTokenKind};
use crate::document::Document;
use crate::graph::ObjectGraph;
use crate::object::{Dict, Name, ObjId, Object, Stream};
use crate::page_tree::{self, PageTreeError, Rect};
use crate::redact_image::{self, ImageHit, ImageSource};
use crate::redact_vector::{self, Paint as PathPaint, PathRecord};
use crate::settings::UnmappableCode;
use crate::span::ByteSpan;
use crate::text_extract::font::ExtractFont;
use crate::vartext::Quadding;
use crate::writer::content::{ContentBuilder, Paint, emit_literal_string, emit_number};
use crate::writer::{SaveOptions, WriteError, save_full};

/// The vertical over-coverage of a glyph box, as a fraction of the font
/// size, below the baseline and above the em top.
///
/// A glyph's advance box is `x ∈ [0, w0]`, but its ink extends below the
/// baseline (descenders) and to the cap/ascent above. Redaction
/// **over-covers** deliberately (fuzzy-never-sneaky: a partial glyph at a
/// region edge is removed, not kept — a leak is worse than an
/// over-redaction), so the region-intersection test uses a slightly
/// enlarged box.
const GLYPH_BOX_DESCENT: f64 = 0.25;
const GLYPH_BOX_ASCENT: f64 = 1.0;

/// A 2-D affine transform in PDF's row-vector convention
/// `[a b 0 / c d 0 / e f 1]` (§8.3.3), in `f64` for interpreter
/// precision.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Mat {
    pub(crate) a: f64,
    pub(crate) b: f64,
    pub(crate) c: f64,
    pub(crate) d: f64,
    pub(crate) e: f64,
    pub(crate) f: f64,
}

impl Default for Mat {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Mat {
    pub(crate) const IDENTITY: Self = Self {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        e: 0.0,
        f: 0.0,
    };

    /// `self × other`, PDF order — `self` applies first.
    fn mul(self, o: Self) -> Self {
        Self {
            a: self.a * o.a + self.b * o.c,
            b: self.a * o.b + self.b * o.d,
            c: self.c * o.a + self.d * o.c,
            d: self.c * o.b + self.d * o.d,
            e: self.e * o.a + self.f * o.c + o.e,
            f: self.e * o.b + self.f * o.d + o.f,
        }
    }

    const fn translate(tx: f64, ty: f64) -> Self {
        Self {
            a: 1.0,
            b: 0.0,
            c: 0.0,
            d: 1.0,
            e: tx,
            f: ty,
        }
    }

    /// Transform a point (row-vector convention).
    pub(crate) fn apply(self, x: f64, y: f64) -> (f64, f64) {
        (
            self.a * x + self.c * y + self.e,
            self.b * x + self.d * y + self.f,
        )
    }
}

/// A rectangular redaction region in default user space (the AABB of one
/// /Redact quad or /Rect — orientation is irrelevant for a removal mask,
/// so quads are reduced to bounds per the RAG's guidance).
#[derive(Debug, Clone, Copy)]
pub(crate) struct RegionBox {
    pub(crate) min_x: f64,
    pub(crate) min_y: f64,
    pub(crate) max_x: f64,
    pub(crate) max_y: f64,
}

impl RegionBox {
    fn from_rect(r: Rect) -> Self {
        Self {
            min_x: r.llx,
            min_y: r.lly,
            max_x: r.urx,
            max_y: r.ury,
        }
    }

    /// AABB overlap (touch counts — the over-redaction bias).
    fn intersects(self, min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> bool {
        self.min_x <= max_x && min_x <= self.max_x && self.min_y <= max_y && min_y <= self.max_y
    }
}

/// Why a redaction apply could not be performed. Every variant names a
/// condition an operator can act on — there is no catch-all.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum RedactError {
    /// The page tree could not be walked.
    #[error("page tree: {0}")]
    PageTree(#[from] PageTreeError),
    /// The document has no /Redact annotations to apply.
    #[error("the document has no redaction marks to apply")]
    NothingToApply,
    /// **Every** redaction mark in the document touches a raster image
    /// pdfcer could not destroy, so there is nothing to apply.
    ///
    /// A single such mark is not an error: it is RETAINED (left in the
    /// document, unapplied) and disclosed in the report while the other
    /// marks are applied — see [`RedactionReport::marks_retained`]. This
    /// variant is the degenerate case where retaining would mean writing
    /// a file with nothing redacted, which is refused **by name** rather
    /// than reported as a success with zero marks applied. `reason` is
    /// the first placement's reason, in the same words the report would
    /// have used.
    #[error(
        "no redaction mark could be applied: every mark touches a raster image pdfcer could not \
         destroy (page {page}: {reason}); a mask or clip would leave the samples recoverable \
         (ISO 32000-1 §12.5.6.23), so the marks were left in place"
    )]
    ImageUndestroyable {
        /// 1-based page number of the first undestroyable placement.
        page: usize,
        /// Why that placement's samples could not be destroyed.
        reason: String,
    },
    /// A content stream could not be tokenized, so its glyphs could not
    /// be located for removal. Refused rather than risk leaving covered
    /// text behind on an unreadable page.
    #[error(
        "page {page} content could not be parsed, so redaction cannot verify removal: {source}"
    )]
    Content {
        /// 1-based page number.
        page: usize,
        /// The underlying tokenization error.
        source: crate::content::ContentError,
    },
    /// The document is encrypted; per-object string decryption is Pass 5,
    /// so redaction is refused rather than operating on ciphertext.
    #[error(
        "this document is encrypted (/Encrypt); redaction of encrypted documents is not yet supported"
    )]
    Encrypted,
    /// The full-rewrite save failed.
    #[error("writing the redacted document failed: {0}")]
    Write(#[from] WriteError),
    /// Re-parsing pdfcer's OWN serialized output failed while applying a
    /// redaction into an [`crate::EditSession`] (`Pass 250.1`). This is an
    /// internal invariant violation, not a document defect: the bytes were
    /// produced by pdfcer's writer moments earlier and must re-load.
    #[error("re-reading the document during redaction failed: {0}")]
    Reload(#[source] crate::document::DocError),
}

/// What a carrier sweep found and did for one class of duplicated
/// content. This is the executable form of §12.5.6.23's "diligent about
/// all content that can exist" obligation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CarrierStatus {
    /// A short stable identifier for the carrier (`info`, `xmp`, `xfa`,
    /// `struct_tree`, `attachments`, `ocg`, `thumbnails`,
    /// `object_streams`, `prior_revisions`, `overlapping_annotations`).
    pub carrier: &'static str,
    /// Whether this carrier was present in the document at all.
    pub present: bool,
    /// What pdfcer did about it.
    pub action: CarrierAction,
}

/// What redaction did about one carrier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CarrierAction {
    /// Not present — nothing to do.
    Absent,
    /// pdfcer removed the redacted content from this carrier; it is part
    /// of the absence guarantee.
    Scrubbed,
    /// The carrier's superseded content is dropped as a side effect of
    /// the forced full rewrite (prior revisions, object-stream survivors).
    DroppedByRewrite,
    /// **Present and NOT scrubbed** — disclosed to the operator as an
    /// un-redacted residual to verify manually. The cardinal-rule-honest
    /// outcome for a carrier this build cannot fully redact.
    DisclosedNotScrubbed,
}

impl CarrierAction {
    /// A short stable identifier for machine-readable output.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Absent => "absent",
            Self::Scrubbed => "scrubbed",
            Self::DroppedByRewrite => "dropped_by_rewrite",
            Self::DisclosedNotScrubbed => "DISCLOSED_NOT_SCRUBBED",
        }
    }
}

/// The redaction report: exactly what was removed and which carriers
/// were checked, scrubbed, or left. This report existing — and being
/// printed — is the mechanism that makes silent under-redaction
/// impossible: every residual pdfcer cannot remove is named here.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct RedactionReport {
    /// Pages that carried at least one /Redact mark.
    pub pages_redacted: usize,
    /// /Redact annotations applied and then removed.
    pub marks_applied: u64,
    /// Text character codes removed from content streams.
    pub glyphs_removed: u64,
    /// Show operators (`Tj`/`TJ`/`'`/`"`) rewritten by the surgery.
    pub show_operators_edited: u64,
    /// Content stream objects replaced with a redacted rewrite.
    pub content_streams_rewritten: u64,
    /// Annotations (redaction marks + overlapping annotations) removed.
    pub annotations_removed: u64,
    /// Object-stream containers decomposed so no removed object survived
    /// compressed (§7.5.7 Strategy B).
    pub containers_decomposed: u64,
    /// Objects promoted out of an object stream by the decomposition.
    pub objects_promoted: u64,
    /// `/Info` string entries scrubbed of redacted text.
    pub info_strings_scrubbed: u64,
    /// Distinct fonts whose advance widths were estimated (no `/Widths`,
    /// not standard-14) — affects only advance-preservation cosmetics,
    /// never the removal itself. Disclosed.
    pub estimated_width_fonts: u64,
    /// Regions whose `/OverlayText` was burnt into the page (Table 192's
    /// overlay-text regime).
    pub overlay_text_burned: u64,
    /// Regions carrying an `/RO` overlay appearance that pdfcer could not
    /// draw, and fell back to a plain box for. Disclosed, never silent.
    pub overlay_ro_not_drawn: u64,
    /// Regions left TRANSPARENT because the mark carried no `/RO`,
    /// `/OverlayText` or `/IC` — Table 192's stated default.
    pub overlay_transparent: u64,
    /// Image placements whose in-region samples were destroyed and the
    /// image re-encoded (losslessly, `FlateDecode`) — §12.5.6.23's
    /// "that portion of the image data shall be destroyed".
    pub images_cleared: u64,
    /// Image placements removed from the page outright because a region
    /// contained the whole placement — the `Do` (or inline `BI…EI`) is
    /// gone, and the object is a 1×1 blank if this was its last use.
    pub images_removed: u64,
    /// Placements whose image is painted elsewhere too (another page, a
    /// form, an appearance stream, or an unmarked placement on the same
    /// page) and therefore received a copy-on-write clone; the original's
    /// samples survive for those other placements, and a note says so.
    pub images_cloned_shared: u64,
    /// Placements whose rotated or skewed matrix made the cleared cells a
    /// bounding-box over-cover — more destroyed than marked, never less.
    pub images_overcovered: u64,
    /// Painted vector paths that crossed a region and could NOT be cut:
    /// a malformed path object carrying an operator §8.2 forbids inside
    /// one (a `cm`, a `q`, a colour operator between construction and
    /// paint), which cannot be replaced as a unit. Each is an un-redacted
    /// residual — the path's bytes remain — reported through the
    /// `vector_paths` carrier as `DisclosedNotScrubbed`. Zero on every
    /// well-formed page since `Pass 246.0`.
    pub vector_paths_intersecting: u64,
    /// Path objects rewritten so no painted segment or filled area lies in
    /// a region (`Pass 246.0`, `redact_vector`): strokes cut against the
    /// region expanded by the stroke width, fills clipped to the region's
    /// complement.
    pub vector_paths_cut: u64,
    /// Of `vector_paths_cut`, the objects that lay wholly inside a region
    /// and were deleted outright.
    pub vector_paths_dropped: u64,
    /// Cut path objects that also set a clip (`W`/`W*`): the paint was cut
    /// but the ORIGINAL geometry was kept as the clip, because §8.5.4
    /// applies the clip after painting and rewriting it would shrink the
    /// window every later object on the page draws through. The kept
    /// geometry is not painted content; it is disclosed because a clip
    /// shaped like the redacted content is still a shape in the file.
    pub vector_clips_kept: u64,
    /// `sh` shading paints (§8.7.4.5.1) whose current clip's bounding box
    /// met a region. A shading fills its clip, so each one may have painted
    /// the region; pdfcer does not cut shadings this build, and the exact
    /// clip shape is not tracked — so every one is an un-redacted residual,
    /// reported through the `shadings` carrier as `DisclosedNotScrubbed`.
    pub shadings_intersecting: u64,
    /// `/Redact` marks left IN the document, unapplied, because a region
    /// touched an image whose samples pdfcer could not destroy. Each is
    /// named in `notes` with its reason, and the `images` carrier reads
    /// `DisclosedNotScrubbed` so a shell surfaces it loudly. Nothing was
    /// removed under a retained mark and no overlay was drawn over it: the
    /// page shows the mark, not a false redaction.
    pub marks_retained: u64,
    /// Per-carrier diligence status (§12.5.6.23's "all content" sweep).
    pub carriers: Vec<CarrierStatus>,
    /// The distinct redacted text strings, for the operator's review and
    /// for the absence-proof gate to grep. Kept because the interpreter
    /// decodes the removed codes while removing them.
    pub redacted_text: Vec<String>,
    /// Named diagnostics and disclosures (human-readable).
    pub notes: Vec<String>,
}

impl RedactionReport {
    fn note(&mut self, text: String) {
        if !self.notes.contains(&text) {
            self.notes.push(text);
        }
    }

    fn add_carrier(&mut self, carrier: &'static str, present: bool, action: CarrierAction) {
        self.carriers.push(CarrierStatus {
            carrier,
            present,
            action,
        });
    }

    /// Whether any carrier was present but disclosed-not-scrubbed — i.e.
    /// the operator must verify a residual manually. A caller (CLI/GUI)
    /// surfaces this loudly.
    #[must_use]
    pub fn has_disclosed_residuals(&self) -> bool {
        self.carriers
            .iter()
            .any(|c| c.action == CarrierAction::DisclosedNotScrubbed)
    }
}

// ===================================================================
// §3 — content-stream text surgery interpreter
// ===================================================================

/// The graphics + text state the surgery interpreter maintains. A focused
/// mirror of the Pass-4 extraction walker's state, kept **self-contained**
/// in this module so the security-critical byte surgery is auditable in
/// one place (the RAG's §3 recommendation: "edit surgically and re-emit
/// the rest byte-faithfully, so the diff is auditable").
struct Surgeon<'a> {
    doc: &'a Document,
    resources: &'a Dict,
    regions: &'a [RegionBox],
    // graphics state
    ctm: Mat,
    ctm_stack: Vec<Mat>,
    // text state (§9.3)
    tm: Mat,
    tlm: Mat,
    tf_size: f64,
    tc: f64,
    tw: f64,
    th: f64,
    trise: f64,
    tl: f64,
    ts_stack: Vec<TextSnapshot>,
    font: Option<ExtractFont>,
    // outputs
    edits: Vec<Edit>,
    removed_text: Vec<String>,
    glyphs_removed: u64,
    ops_edited: u64,
    form_intersect: bool,
    /// Every image placement that intersects a region, with what is
    /// needed to destroy its samples.
    image_hits: Vec<ImageHit>,
    /// The path object under construction (`m`/`l`/`c`/`v`/`y`/`re`/`h`
    /// since the last painting operator), in the authored coordinates
    /// with the CTM captured at its first operator.
    path: PathRecord,
    /// The current line width (`w`), user units — the stroke-expansion
    /// input for `redact_vector`. Saved/restored with `q`/`Q`.
    line_width: f64,
    lw_stack: Vec<f64>,
    /// The current clipping path's bounding box in page space (`None` =
    /// unclipped), intersected on every `W`/`W*` and saved/restored with
    /// `q`/`Q`. A box, not the path: it exists only to bound `sh`.
    clip_bbox: Option<(f64, f64, f64, f64)>,
    clip_stack: Vec<Option<(f64, f64, f64, f64)>>,
    /// `sh` operators (§8.7.4.5.1: paint the shading over the whole
    /// current clip) whose clip box meets a region. Not cut this build —
    /// a residual, disclosed.
    shadings_intersecting: u64,
    /// Painted paths (`S`, `f`, `B`, … — not `n`) that crossed a region
    /// and could NOT be cut — a malformed object with a foreign operator
    /// inside it, which cannot be replaced as a unit. Each is an
    /// un-redacted residual and is disclosed.
    vector_paths_intersecting: u64,
    /// Path objects rewritten by `redact_vector`.
    vector_paths_cut: u64,
    /// Path objects deleted because they lay wholly inside a region.
    vector_paths_dropped: u64,
    /// Clip-marked path objects whose ORIGINAL geometry was kept as the
    /// clip (`W n`) after the cut paint — see `redact_vector`'s module doc.
    vector_clips_kept: u64,
    estimated_fonts: BTreeSet<String>,
}

/// The text-state fields saved/restored by `q`/`Q`. (`Tf`/font is part of
/// text state too, but cloning an `ExtractFont` per `q` is wasteful; the
/// interpreter re-resolves the font on the next `Tf`, and `q`/`Q` in real
/// content rarely straddles a `Tf` boundary that matters to geometry.)
#[derive(Clone, Copy)]
struct TextSnapshot {
    tf_size: f64,
    tc: f64,
    tw: f64,
    th: f64,
    trise: f64,
    tl: f64,
}

/// One byte-range replacement in the decoded content buffer.
struct Edit {
    start: usize,
    end: usize,
    bytes: Vec<u8>,
}

/// The result of redacting one page's content.
struct SurgeryResult {
    /// The rewritten (redacted + overlay-baked) content bytes.
    content: Vec<u8>,
    removed_text: Vec<String>,
    glyphs_removed: u64,
    ops_edited: u64,
    /// A form XObject intersects a region — disclosed, not refused.
    form_intersect: bool,
    /// Painted vector paths that crossed a region and could not be cut.
    vector_paths_intersecting: u64,
    vector_paths_cut: u64,
    vector_paths_dropped: u64,
    vector_clips_kept: u64,
    /// `sh` paints whose clip box met a region.
    shadings_intersecting: u64,
    estimated_fonts: BTreeSet<String>,
}

impl<'a> Surgeon<'a> {
    fn new(doc: &'a Document, resources: &'a Dict, regions: &'a [RegionBox]) -> Self {
        Self {
            doc,
            resources,
            regions,
            ctm: Mat::IDENTITY,
            ctm_stack: Vec::new(),
            tm: Mat::IDENTITY,
            tlm: Mat::IDENTITY,
            tf_size: 0.0,
            tc: 0.0,
            tw: 0.0,
            th: 1.0,
            trise: 0.0,
            tl: 0.0,
            ts_stack: Vec::new(),
            font: None,
            edits: Vec::new(),
            removed_text: Vec::new(),
            glyphs_removed: 0,
            ops_edited: 0,
            form_intersect: false,
            image_hits: Vec::new(),
            path: PathRecord::default(),
            line_width: 1.0,
            lw_stack: Vec::new(),
            clip_bbox: None,
            clip_stack: Vec::new(),
            shadings_intersecting: 0,
            vector_paths_intersecting: 0,
            vector_paths_cut: 0,
            vector_paths_dropped: 0,
            vector_clips_kept: 0,
            estimated_fonts: BTreeSet::new(),
        }
    }

    /// Resolve the numeric operands of an operation, in order.
    fn nums(operands: &[crate::content::ContentToken]) -> Vec<f64> {
        operands
            .iter()
            .filter_map(|t| match &t.kind {
                ContentTokenKind::Operand(o) => o.as_number(),
                _ => None,
            })
            .collect()
    }

    /// Resolve a `/Font /<name>` resource to an [`ExtractFont`].
    fn resolve_font(&self, name: &[u8]) -> Option<ExtractFont> {
        let fonts = self
            .resources
            .get(b"Font")
            .map(|o| self.doc.resolve(o))
            .and_then(Object::as_dict)?;
        let font_dict = fonts
            .get(name)
            .map(|o| self.doc.resolve(o))
            .and_then(Object::as_dict)?;
        // `&self.doc.view()` (Pass 17.1): `ExtractFont::resolve` now takes a
        // read VIEW because it may need a `/ToUnicode` stream's bytes. This
        // census deliberately reads the loaded document, so the contiguous
        // base view is the right one and behaviour is unchanged.
        Some(ExtractFont::resolve(&self.doc.view(), font_dict))
    }

    /// Is a named XObject an image (or a form) whose unit-square placement
    /// intersects a region? Sets `image_intersect` / `form_intersect`, and
    /// records an image placement (with the `Do`'s byte span, so the
    /// operation can be rewritten or removed) for the image surgery.
    fn check_xobject(&mut self, name: &[u8], span: (usize, usize)) {
        let Some(xobjects) = self
            .resources
            .get(b"XObject")
            .map(|o| self.doc.resolve(o))
            .and_then(Object::as_dict)
        else {
            return;
        };
        let Some(entry) = xobjects.get(name) else {
            return;
        };
        let id = entry.as_reference();
        let Object::Stream(stream) = self.doc.resolve(entry) else {
            return;
        };
        let subtype = stream
            .dict
            .get(b"Subtype")
            .and_then(Object::as_name)
            .map(|n| n.as_bytes().to_vec())
            .unwrap_or_default();
        // A form/image placement is the unit square (0,0)-(1,1) × CTM.
        if !self.unit_square_intersects() {
            return;
        }
        match subtype.as_slice() {
            b"Image" => {
                self.image_hits.push(ImageHit {
                    span,
                    ctm: self.ctm,
                    source: ImageSource::XObject {
                        name: name.to_vec(),
                        id,
                    },
                });
            }
            b"Form" => self.form_intersect = true,
            _ => {}
        }
    }

    /// Whether the current CTM's unit square intersects any region.
    fn unit_square_intersects(&self) -> bool {
        let corners = [
            self.ctm.apply(0.0, 0.0),
            self.ctm.apply(1.0, 0.0),
            self.ctm.apply(0.0, 1.0),
            self.ctm.apply(1.0, 1.0),
        ];
        let (min_x, min_y, max_x, max_y) = aabb(&corners);
        self.regions
            .iter()
            .any(|r| r.intersects(min_x, min_y, max_x, max_y))
    }

    /// Set the text matrix and line matrix (`Td`/`TD`/`T*`/`Tm`).
    fn set_line(&mut self, m: Mat) {
        self.tm = m;
        self.tlm = m;
    }

    /// Process one content operation, updating state and (for show
    /// operators) recording any surgery edit.
    fn operation(&mut self, op: &crate::content::Operation<'_>, buf: &[u8]) {
        let Some(name) = op.operator_name(buf) else {
            // An inline image: its unit-square placement may intersect.
            if let ContentTokenKind::InlineImage { params, data } = &op.operator.kind
                && self.unit_square_intersects()
            {
                self.image_hits.push(ImageHit {
                    span: (op.operator.span.start, op.operator.span.end()),
                    ctm: self.ctm,
                    source: ImageSource::Inline {
                        params: params.clone(),
                        data: *data,
                    },
                });
            }
            return;
        };
        let n = Self::nums(op.operands);
        // §8.2: inside a path object only construction, clipping and
        // painting operators may appear. Anything else means the bytes
        // from the first construction operand to the paint cannot be
        // replaced as one unit, so the object is left alone and disclosed.
        if self.path.start.is_some()
            && !matches!(
                name,
                b"m" | b"l"
                    | b"c"
                    | b"v"
                    | b"y"
                    | b"h"
                    | b"re"
                    | b"W"
                    | b"W*"
                    | b"n"
                    | b"S"
                    | b"s"
                    | b"f"
                    | b"F"
                    | b"f*"
                    | b"B"
                    | b"B*"
                    | b"b"
                    | b"b*"
            )
        {
            self.path.dirty = true;
        }
        match name {
            b"q" => {
                self.ctm_stack.push(self.ctm);
                self.ts_stack.push(self.snapshot());
                self.lw_stack.push(self.line_width);
                self.clip_stack.push(self.clip_bbox);
            }
            b"Q" => {
                if let Some(m) = self.ctm_stack.pop() {
                    self.ctm = m;
                }
                if let Some(s) = self.ts_stack.pop() {
                    self.restore(s);
                }
                if let Some(w) = self.lw_stack.pop() {
                    self.line_width = w;
                }
                if let Some(c) = self.clip_stack.pop() {
                    self.clip_bbox = c;
                }
            }
            b"w" => {
                if let Some(v) = n.first() {
                    self.line_width = *v;
                }
            }
            b"cm" => {
                if let [a, b, c, d, e, f] = n.as_slice() {
                    self.ctm = Mat {
                        a: *a,
                        b: *b,
                        c: *c,
                        d: *d,
                        e: *e,
                        f: *f,
                    }
                    .mul(self.ctm);
                }
            }
            b"BT" => {
                self.tm = Mat::IDENTITY;
                self.tlm = Mat::IDENTITY;
            }
            b"Tf" => {
                if let Some(fname) = op.operands.iter().find_map(|t| match &t.kind {
                    ContentTokenKind::Operand(Object::Name(nm)) => Some(nm.as_bytes().to_vec()),
                    _ => None,
                }) {
                    self.font = self.resolve_font(&fname);
                }
                if let Some(size) = n.last() {
                    self.tf_size = *size;
                }
            }
            b"Td" => {
                if let [tx, ty] = n.as_slice() {
                    self.set_line(Mat::translate(*tx, *ty).mul(self.tlm));
                }
            }
            b"TD" => {
                if let [tx, ty] = n.as_slice() {
                    self.tl = -*ty;
                    self.set_line(Mat::translate(*tx, *ty).mul(self.tlm));
                }
            }
            b"Tm" => {
                if let [a, b, c, d, e, f] = n.as_slice() {
                    self.set_line(Mat {
                        a: *a,
                        b: *b,
                        c: *c,
                        d: *d,
                        e: *e,
                        f: *f,
                    });
                }
            }
            b"T*" => {
                self.set_line(Mat::translate(0.0, -self.tl).mul(self.tlm));
            }
            b"Tc" => {
                if let Some(v) = n.first() {
                    self.tc = *v;
                }
            }
            b"Tw" => {
                if let Some(v) = n.first() {
                    self.tw = *v;
                }
            }
            b"Tz" => {
                if let Some(v) = n.first() {
                    self.th = *v / 100.0;
                }
            }
            b"TL" => {
                if let Some(v) = n.first() {
                    self.tl = *v;
                }
            }
            b"Ts" => {
                if let Some(v) = n.first() {
                    self.trise = *v;
                }
            }
            b"Do" => {
                if let Some(xname) = op.operands.iter().find_map(|t| match &t.kind {
                    ContentTokenKind::Operand(Object::Name(nm)) => Some(nm.as_bytes().to_vec()),
                    _ => None,
                }) {
                    self.check_xobject(&xname, op_span(op));
                }
            }
            b"Tj" => self.show_simple(op, ShowKind::Tj),
            b"'" => {
                self.set_line(Mat::translate(0.0, -self.tl).mul(self.tlm));
                self.show_simple(op, ShowKind::Quote);
            }
            b"\"" => {
                if let [aw, ac] = n.as_slice() {
                    self.tw = *aw;
                    self.tc = *ac;
                }
                self.set_line(Mat::translate(0.0, -self.tl).mul(self.tlm));
                self.show_simple(op, ShowKind::DoubleQuote);
            }
            b"TJ" => self.show_array(op),
            // Path construction (§8.5.2), recorded for the cut.
            b"m" => {
                if let [x, y] = n.as_slice() {
                    self.path.begin(op_span(op).0, self.ctm);
                    self.path.move_to(*x, *y);
                }
            }
            b"l" => {
                if let [x, y] = n.as_slice() {
                    self.path.begin(op_span(op).0, self.ctm);
                    self.path.line_to(*x, *y);
                }
            }
            b"c" => {
                if let [x1, y1, x2, y2, x3, y3] = n.as_slice() {
                    self.path.begin(op_span(op).0, self.ctm);
                    self.path.curve_to(*x1, *y1, *x2, *y2, *x3, *y3);
                }
            }
            b"v" => {
                if let [x2, y2, x3, y3] = n.as_slice() {
                    self.path.begin(op_span(op).0, self.ctm);
                    self.path.curve_v(*x2, *y2, *x3, *y3);
                }
            }
            b"y" => {
                if let [x1, y1, x3, y3] = n.as_slice() {
                    self.path.begin(op_span(op).0, self.ctm);
                    self.path.curve_y(*x1, *y1, *x3, *y3);
                }
            }
            b"h" => {
                self.path.begin(op_span(op).0, self.ctm);
                self.path.close();
            }
            b"re" => {
                if let [x, y, w, h] = n.as_slice() {
                    self.path.begin(op_span(op).0, self.ctm);
                    self.path.rect(*x, *y, *w, *h);
                }
            }
            b"W" | b"W*" => {
                if self.path.clip.is_none() {
                    self.path.clip_start = Some(op_span(op).0);
                }
                self.path.clip = Some(if name == b"W" { b"W" } else { b"W*" });
            }
            // Path painting (§8.5.3): the cut happens here.
            b"S" | b"s" | b"f" | b"F" | b"f*" | b"B" | b"B*" | b"b" | b"b*" => {
                if let Some(paint) = PathPaint::from_op(name) {
                    self.path_painted(op, buf, paint);
                }
                self.apply_clip();
                self.path = PathRecord::default();
            }
            // `n` ends a path without painting it (a clip definition, or
            // nothing): not content, never touched — but the clip it sets
            // bounds every later `sh`.
            b"n" => {
                self.apply_clip();
                self.path = PathRecord::default();
            }
            // A shading paints the whole current clip. Without the clip's
            // exact shape the interpreter can only say whether its box meets
            // a region; when it does, that is an un-redacted residual.
            b"sh" => {
                let clip = self.clip_bbox.unwrap_or((
                    f64::NEG_INFINITY,
                    f64::NEG_INFINITY,
                    f64::INFINITY,
                    f64::INFINITY,
                ));
                if self.regions.iter().any(|r| {
                    r.min_x < clip.2 && clip.0 < r.max_x && r.min_y < clip.3 && clip.1 < r.max_y
                }) {
                    self.shadings_intersecting += 1;
                }
            }
            _ => {}
        }
    }

    fn snapshot(&self) -> TextSnapshot {
        TextSnapshot {
            tf_size: self.tf_size,
            tc: self.tc,
            tw: self.tw,
            th: self.th,
            trise: self.trise,
            tl: self.tl,
        }
    }

    fn restore(&mut self, s: TextSnapshot) {
        self.tf_size = s.tf_size;
        self.tc = s.tc;
        self.tw = s.tw;
        self.th = s.th;
        self.trise = s.trise;
        self.tl = s.tl;
    }

    /// If the path object under construction carries `W`/`W*`, intersect
    /// the tracked clip box with its page-space box (§8.5.4: the clip takes
    /// effect after the painting operator).
    fn apply_clip(&mut self) {
        if self.path.clip.is_none() {
            return;
        }
        let Some((x0, y0, x1, y1)) = self.path.page_bbox() else {
            // A clip with no geometry clips everything.
            self.clip_bbox = Some((0.0, 0.0, 0.0, 0.0));
            return;
        };
        self.clip_bbox = Some(match self.clip_bbox {
            None => (x0, y0, x1, y1),
            Some((a0, b0, a1, b1)) => (a0.max(x0), b0.max(y0), a1.min(x1), b1.min(y1)),
        });
    }

    /// A painting operator ended the current path object: cut it against
    /// the regions (`redact_vector`) and record the replacement edit, or —
    /// for a malformed object that cannot be replaced as a unit — count it
    /// as a residual.
    fn path_painted(&mut self, op: &crate::content::Operation<'_>, buf: &[u8], paint: PathPaint) {
        let Some(start) = self.path.start else {
            return; // a paint with no construction: nothing to cut
        };
        let end = op.operator.span.end();
        if self.path.dirty || end <= start {
            // Count it only if it actually crosses a region.
            if let Some((x0, y0, x1, y1)) = self.path.page_bbox()
                && self
                    .regions
                    .iter()
                    .any(|r| r.min_x < x1 && x0 < r.max_x && r.min_y < y1 && y0 < r.max_y)
            {
                self.vector_paths_intersecting += 1;
            }
            return;
        }
        // The construction bytes end where the clip operator (if any) or
        // the paint operator begins.
        let construction_end = self.path.clip_start.unwrap_or(op.operator.span.start);
        let original = buf.get(start..construction_end).unwrap_or(&[]);
        let Some(cut) =
            redact_vector::cut_path(&self.path, paint, self.line_width, self.regions, original)
        else {
            return;
        };
        self.vector_paths_cut += 1;
        if cut.dropped_whole {
            self.vector_paths_dropped += 1;
        }
        if cut.clip_kept {
            self.vector_clips_kept += 1;
        }
        self.edits.push(Edit {
            start,
            end,
            bytes: cut.bytes,
        });
    }

    /// The horizontal advance `tx` for one code (text-line units, §9.4.4),
    /// and whether the glyph's box intersects a region.
    fn glyph(&self, code: u32, word_spacing: bool) -> (f64, bool) {
        let Some(font) = &self.font else {
            return (0.0, false);
        };
        let w0 = f64::from(font.width(code));
        let tw = if word_spacing { self.tw } else { 0.0 };
        // The ONE copy of §9.4.4's displacement (`text_extract::font::
        // advance_tx`), shared with extraction and with the vector object
        // model's text bounding box.
        let tx = crate::text_extract::font::advance_tx(w0, self.tf_size, self.tc, tw, self.th);

        // Trm = [Tfs·Th 0 0 Tfs 0 Ts] × Tm × CTM (§9.4.4). The glyph box
        // in text space is x ∈ [0, w0], y ∈ [-descent, ascent], deliberately
        // over-covered (module docs).
        let params = Mat {
            a: self.tf_size * self.th,
            b: 0.0,
            c: 0.0,
            d: self.tf_size,
            e: 0.0,
            f: self.trise,
        };
        let trm = params.mul(self.tm.mul(self.ctm));
        let corners = [
            trm.apply(0.0, -GLYPH_BOX_DESCENT),
            trm.apply(w0, -GLYPH_BOX_DESCENT),
            trm.apply(0.0, GLYPH_BOX_ASCENT),
            trm.apply(w0, GLYPH_BOX_ASCENT),
        ];
        let (min_x, min_y, max_x, max_y) = aabb(&corners);
        let hit = self
            .regions
            .iter()
            .any(|r| r.intersects(min_x, min_y, max_x, max_y));
        (tx, hit)
    }

    /// `Tj`/`'`/`"`: a single show string. Build an advance-preserving
    /// replacement if any code is in-region.
    fn show_simple(&mut self, op: &crate::content::Operation<'_>, kind: ShowKind) {
        let Some(string) = op.operands.iter().rev().find_map(|t| match &t.kind {
            ContentTokenKind::Operand(Object::String(s)) => Some(s.clone()),
            _ => None,
        }) else {
            return;
        };
        let Some(elem) = self.redact_string(&string) else {
            // Nothing in-region: advance Tm for the whole string and keep
            // the operator verbatim.
            self.advance_string(&string);
            return;
        };
        // Something was removed. Emit the replacement TJ (the survivors +
        // compensating advances) with any leading positioning kind needs.
        let mut out = Vec::new();
        match kind {
            ShowKind::Tj => {}
            ShowKind::Quote => out.extend_from_slice(b"T* "),
            ShowKind::DoubleQuote => {
                // Re-establish Tw/Tc (the " operator sets them and they
                // persist for following operators) then the line move.
                let nums = Self::nums(op.operands);
                if let [aw, ac] = nums.as_slice() {
                    emit_num(&mut out, *aw);
                    out.extend_from_slice(b" Tw ");
                    emit_num(&mut out, *ac);
                    out.extend_from_slice(b" Tc ");
                }
                out.extend_from_slice(b"T* ");
            }
        }
        emit_tj_array(&mut out, &elem);
        let (start, end) = op_span(op);
        self.record_edit(start, end, out);
    }

    /// `TJ`: an array of strings and kerning numbers. Rebuild it,
    /// replacing in-region code runs with compensating advances.
    fn show_array(&mut self, op: &crate::content::Operation<'_>) {
        let Some(items) = op.operands.iter().rev().find_map(|t| match &t.kind {
            ContentTokenKind::Operand(Object::Array(a)) => Some(a.clone()),
            _ => None,
        }) else {
            return;
        };
        let mut new_elems: Vec<TjElem> = Vec::new();
        let mut any_removed = false;
        for item in &items {
            match item {
                Object::String(s) => match self.redact_string(s) {
                    Some(elems) => {
                        any_removed = true;
                        new_elems.extend(elems);
                    }
                    None => {
                        self.advance_string(s);
                        new_elems.push(TjElem::Str(s.clone()));
                    }
                },
                other => {
                    // A kerning number: it adjusts Tm too, but does not
                    // show a glyph, so it is never "in region" — keep it.
                    if let Some(v) = other.as_number() {
                        self.tm =
                            Mat::translate(-v / 1000.0 * self.tf_size * self.th, 0.0).mul(self.tm);
                        new_elems.push(TjElem::Num(v));
                    }
                }
            }
        }
        if !any_removed {
            return;
        }
        let mut out = Vec::new();
        emit_tj_array(&mut out, &new_elems);
        let (start, end) = op_span(op);
        self.record_edit(start, end, out);
    }

    /// Advance `Tm` across a whole non-redacted string (so following
    /// operators stay correctly positioned) without recording an edit.
    fn advance_string(&mut self, string: &[u8]) {
        let Some(font) = self.font.clone() else {
            return;
        };
        for code in font.codes(string) {
            let (tx, _) = self.glyph(code.value, code.word_spacing_applies);
            self.tm = Mat::translate(tx, 0.0).mul(self.tm);
        }
    }

    /// Walk one show string's codes, computing which are in-region and
    /// advancing `Tm` per code. Returns `None` if none are in-region
    /// (caller keeps the operator verbatim), or the rebuilt element list
    /// (surviving byte segments + compensating advances) otherwise.
    ///
    /// Codes are segmented on **code** boundaries (1 byte for a simple
    /// font, 2 for a composite CID) so a multi-byte CID is never split
    /// (`iso32000__ref__redaction_removal.md` §3).
    fn redact_string(&mut self, string: &[u8]) -> Option<Vec<TjElem>> {
        let font = self.font.clone()?;
        let bpc = font.bytes_per_code();
        let codes = font.codes(string);
        // First pass: per-code hit + advance, and whether anything hits.
        let mut hits = Vec::with_capacity(codes.len());
        let mut any = false;
        for code in &codes {
            let (tx, hit) = self.glyph(code.value, code.word_spacing_applies);
            self.tm = Mat::translate(tx, 0.0).mul(self.tm);
            if hit {
                any = true;
            }
            hits.push((tx, hit, code.value));
        }
        if !any {
            return None;
        }
        if font.width_estimated() {
            self.estimated_fonts.insert(font.base_font_name());
        }
        // Second pass: build the replacement elements, coalescing runs.
        let mut elems: Vec<TjElem> = Vec::new();
        let mut seg_bytes: Vec<u8> = Vec::new();
        let mut removed_tx = 0.0f64;
        let mut removed_text = String::new();
        for (i, (tx, hit, code_val)) in hits.iter().enumerate() {
            let byte_start = i * bpc;
            let seg = string.get(byte_start..byte_start + bpc).unwrap_or(&[]);
            if *hit {
                // flush any pending surviving segment
                if !seg_bytes.is_empty() {
                    elems.push(TjElem::Str(std::mem::take(&mut seg_bytes)));
                }
                removed_tx += *tx;
                // `TX-A1` is deliberately PINNED here rather than
                // taking the operator's setting. `removed_text` is the
                // audit record of what this redaction destroyed, and an
                // audit record must show that something was there:
                // `UnmappableCode::Omit` would report a removed
                // unmappable glyph as nothing removed at all. The
                // sentinel is fixed to the length-preserving, visible one
                // so the record cannot understate the removal.
                let (chars, _) = font.to_unicode(*code_val, UnmappableCode::ReplacementChar);
                removed_text.push_str(&chars);
                self.glyphs_removed += 1;
            } else {
                // flush any pending removed run as a compensating advance
                if removed_tx != 0.0 {
                    elems.push(TjElem::Num(advance_to_tj(
                        removed_tx,
                        self.tf_size,
                        self.th,
                    )));
                    removed_tx = 0.0;
                }
                seg_bytes.extend_from_slice(seg);
            }
        }
        if !seg_bytes.is_empty() {
            elems.push(TjElem::Str(seg_bytes));
        }
        if removed_tx != 0.0 {
            elems.push(TjElem::Num(advance_to_tj(
                removed_tx,
                self.tf_size,
                self.th,
            )));
        }
        if !removed_text.is_empty() {
            self.removed_text.push(removed_text);
        }
        self.ops_edited += 1;
        Some(elems)
    }

    fn record_edit(&mut self, start: usize, end: usize, bytes: Vec<u8>) {
        self.edits.push(Edit { start, end, bytes });
    }
}

/// Which single-string show operator is being rewritten.
enum ShowKind {
    Tj,
    Quote,
    DoubleQuote,
}

/// One element of a rebuilt `TJ` array.
enum TjElem {
    Str(Vec<u8>),
    Num(f64),
}

/// The `TJ` number that consumes a removed run's total advance `Σtx`
/// (text-line units): `N = −Σtx · 1000 / (Tfs·Th)` (§9.4.3). Guards a
/// zero scale (invisible text advances nothing).
fn advance_to_tj(sum_tx: f64, tfs: f64, th: f64) -> f64 {
    let scale = tfs * th;
    if scale.abs() < f64::EPSILON {
        0.0
    } else {
        -sum_tx * 1000.0 / scale
    }
}

/// The AABB of a set of transformed corner points.
pub(crate) fn aabb(pts: &[(f64, f64)]) -> (f64, f64, f64, f64) {
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for &(x, y) in pts {
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x);
        max_y = max_y.max(y);
    }
    (min_x, min_y, max_x, max_y)
}

/// The byte span of a whole operation (operands + operator) in the
/// decoded buffer, for replacement.
fn op_span(op: &crate::content::Operation<'_>) -> (usize, usize) {
    let start = op
        .operands
        .first()
        .map_or(op.operator.span.start, |t| t.span.start);
    (start, op.operator.span.end())
}

/// Emit a number into a content stream (integer form when integral).
fn emit_num(out: &mut Vec<u8>, v: f64) {
    emit_number(out, v);
}

/// Emit a rebuilt `TJ` array: `[ (str) num (str) … ] TJ`.
fn emit_tj_array(out: &mut Vec<u8>, elems: &[TjElem]) {
    out.push(b'[');
    for (i, e) in elems.iter().enumerate() {
        if i > 0 {
            out.push(b' ');
        }
        match e {
            TjElem::Str(s) => emit_literal_string(out, s),
            TjElem::Num(v) => emit_number(out, *v),
        }
    }
    out.extend_from_slice(b"] TJ");
}

/// Redact one page's concatenated content: run the interpreter, splice
/// the edits, and append the overlay marking.
fn redact_page_content(
    doc: &Document,
    resources: &Dict,
    regions: &[RegionBox],
    stream: &ContentStream,
    overlay: &[u8],
    image_edits: Vec<(usize, usize, Vec<u8>)>,
) -> SurgeryResult {
    let mut surgeon = Surgeon::new(doc, resources, regions);
    for op in stream.operations() {
        surgeon.operation(&op, &stream.buf);
    }
    // The image surgery's edits (a `Do` renamed to its clone, a `Do` or
    // `BI…EI` removed, an inline image re-encoded) splice in beside the
    // glyph edits. They never overlap: a painting operator is not a show
    // operator.
    for (start, end, bytes) in image_edits {
        surgeon.edits.push(Edit { start, end, bytes });
    }
    // Splice edits (sorted, non-overlapping) into the buffer.
    surgeon.edits.sort_by_key(|e| e.start);
    let mut content = Vec::with_capacity(stream.buf.len() + overlay.len());
    let mut cursor = 0usize;
    for edit in &surgeon.edits {
        if edit.start < cursor {
            continue; // defensive: overlapping edit, skip
        }
        if let Some(gap) = stream.buf.get(cursor..edit.start) {
            content.extend_from_slice(gap);
        }
        content.extend_from_slice(&edit.bytes);
        cursor = edit.end;
    }
    if let Some(tail) = stream.buf.get(cursor..) {
        content.extend_from_slice(tail);
    }
    // The overlay marking is drawn AFTER the (now-redacted) content so it
    // sits on top. A leading EOL guards against fusing onto a final token.
    if !overlay.is_empty() {
        content.push(b'\n');
        content.extend_from_slice(overlay);
    }
    SurgeryResult {
        content,
        removed_text: surgeon.removed_text,
        glyphs_removed: surgeon.glyphs_removed,
        ops_edited: surgeon.ops_edited,
        form_intersect: surgeon.form_intersect,
        vector_paths_intersecting: surgeon.vector_paths_intersecting,
        vector_paths_cut: surgeon.vector_paths_cut,
        vector_paths_dropped: surgeon.vector_paths_dropped,
        vector_clips_kept: surgeon.vector_clips_kept,
        shadings_intersecting: surgeon.shadings_intersecting,
        estimated_fonts: surgeon.estimated_fonts,
    }
}

/// The image placements on one page that intersect any of `regions` — the
/// census the blocker pass runs BEFORE any surgery, so a mark that touches
/// an undestroyable image can be retained rather than half-applied.
fn census_images(
    doc: &Document,
    resources: &Dict,
    regions: &[RegionBox],
    stream: &ContentStream,
) -> Vec<ImageHit> {
    let mut surgeon = Surgeon::new(doc, resources, regions);
    for op in stream.operations() {
        surgeon.operation(&op, &stream.buf);
    }
    surgeon.image_hits
}

/// Does `hit`'s placement overlap `region` with positive area? (A mere
/// touch of bounding boxes destroys no cell and covers nothing.)
fn hit_covers(hit: &ImageHit, region: RegionBox) -> bool {
    let (x0, y0, x1, y1) = hit.bbox();
    region.min_x < x1 && x0 < region.max_x && region.min_y < y1 && y0 < region.max_y
}

/// What [`build_overlay`] did, so the caller can DISCLOSE it.
///
/// Every field here exists because project rule 4 forbids a silent
/// inference, and the overlay path makes several: a substituted `/DA`, an
/// auto-chosen font size, a character with no WinAnsi code, an `/RO` that
/// could not be drawn. None of those is visible to an operator looking at
/// the result — a burnt-in overlay looks equally deliberate whether the
/// size was the author's or pdfcer's — and the mark carrying the evidence
/// is deleted by the same operation, so if this struct does not carry it,
/// nothing does.
#[derive(Debug, Default)]
struct OverlayOutcome {
    /// The content-stream bytes to append after the redacted content.
    content: Vec<u8>,
    /// `/Font` resource entries the content needs, keyed by the resource
    /// name its `/DA` referenced. Merged into the page's `/Resources`.
    fonts: Dict,
    /// Regions whose `/OverlayText` was burnt in.
    text_regions: u64,
    /// Regions where `/RO` was present and could not be drawn.
    ro_regions: u64,
    /// Regions left transparent because no `/RO`, `/OverlayText` or `/IC`
    /// was present (Table 192's default).
    transparent_regions: u64,
    /// Regions whose `/Repeat true` was ignored.
    repeat_ignored: u64,
    /// Regions whose `/OverlayText` was present with no `/DA`, which
    /// Table 192 makes conditionally required.
    da_substituted: u64,
    /// Auto-sizes pdfcer chose (`/DA` size 0), in points.
    autosizes: Vec<f64>,
    /// Characters with no WinAnsi code, replaced by `?`.
    unencodable_chars: usize,
    /// Overlay text that could not be laid out at all, with the reason.
    text_failures: Vec<String>,
}

impl OverlayOutcome {
    /// Accumulate one page's outcome into a document-wide total.
    ///
    /// Deliberately does NOT accumulate `content` or `fonts`: those are
    /// per-page by construction (they are baked into that page's content
    /// stream and merged into that page's `/Resources`), and summing them
    /// across pages would produce a document-wide font set that no single
    /// page's `/DA` names. Only the DISCLOSURE counters are document-wide,
    /// because the report is.
    fn absorb(&mut self, other: &Self) {
        self.text_regions += other.text_regions;
        self.ro_regions += other.ro_regions;
        self.transparent_regions += other.transparent_regions;
        self.repeat_ignored += other.repeat_ignored;
        self.da_substituted += other.da_substituted;
        self.unencodable_chars += other.unencodable_chars;
        self.autosizes.extend_from_slice(&other.autosizes);
        for f in &other.text_failures {
            if !self.text_failures.contains(f) {
                self.text_failures.push(f.clone());
            }
        }
    }
}

/// Build the overlay content bytes for a page's regions, following the
/// Table 192 precedence ladder reified in [`OverlayRegime`].
///
/// Everything is wrapped in one `q … Q` so the overlay cannot perturb the
/// state of any content that follows, and each text block gets its own
/// nested `q … Q` plus a translation, because
/// [`crate::vartext::build_variable_text`] emits its content in a box-local space
/// with the origin at the bottom-left — exactly like an appearance stream
/// `/BBox`. Translating instead of re-laying-out is what keeps ONE text
/// layout implementation in the binary: the same code lays out a form
/// field's value, a FreeText annotation, and this. A second layout path
/// reached only by redaction would be a path only redaction could get
/// wrong, on the one operation with no undo.
fn build_overlay(
    doc: &Document,
    page_resources: &Dict,
    regions: &[(RegionBox, OverlayRegime)],
) -> OverlayOutcome {
    let mut out = OverlayOutcome::default();
    let mut b = ContentBuilder::new();
    b.save_state();
    for (region, regime) in regions {
        let (x, y) = (region.min_x, region.min_y);
        let (w, h) = (region.max_x - region.min_x, region.max_y - region.min_y);
        // The box fill, when the ladder calls for one.
        let box_fill = match regime {
            OverlayRegime::Ro { fallback } => {
                out.ro_regions += 1;
                Some(*fallback)
            }
            OverlayRegime::Text { fill, .. } => *fill,
            OverlayRegime::Fill(rgb) => Some(*rgb),
            OverlayRegime::Transparent => {
                out.transparent_regions += 1;
                None
            }
        };
        if let Some(rgb) = box_fill {
            b.set_fill_rgb(rgb[0], rgb[1], rgb[2]);
            b.rect(x, y, w, h);
            b.paint(Paint::Fill);
        }
        let OverlayRegime::Text {
            text,
            da,
            quad,
            repeat,
            ..
        } = regime
        else {
            continue;
        };
        if *repeat {
            out.repeat_ignored += 1;
        }
        // Table 192 makes /DA required whenever /OverlayText is present.
        // A mark without one is malformed — but the content is ALREADY
        // gone by the time this runs, so refusing would leave the document
        // redacted and unmarked, which is strictly worse than a disclosed
        // default. Substitute and say so.
        let da = if da.is_empty() {
            out.da_substituted += 1;
            crate::vartext::default_appearance_string(
                b"Helv",
                0.0,
                crate::vartext::TextColor::Gray(0.0),
            )
        } else {
            da.clone()
        };
        let fonts = overlay_font_resources(doc, page_resources, &da);
        let bbox = Rect {
            llx: 0.0,
            lly: 0.0,
            urx: w,
            ury: h,
        };
        match crate::vartext::build_variable_text(bbox, text, &da, *quad, true, &fonts) {
            Ok(app) => {
                if let Some(size) = app.applied_autosize {
                    out.autosizes.push(size);
                }
                out.unencodable_chars += app.unencodable_chars;
                // Place the box-local content at the region's lower-left.
                b.save_state();
                b.concat_matrix(1.0, 0.0, 0.0, 1.0, x, y);
                b.append_raw(&app.content);
                b.restore_state();
                // Merge the font dict this block needs.
                if let Some(f) = app.resources.get(b"Font").and_then(Object::as_dict) {
                    for (name, val) in f.iter() {
                        if out.fonts.get(name.as_bytes()).is_none() {
                            out.fonts.insert(name.clone(), val.clone());
                        }
                    }
                }
                out.text_regions += 1;
            }
            Err(e) => out.text_failures.push(e.to_string()),
        }
    }
    b.restore_state();
    out.content = b.into_bytes();
    out
}

/// The font resources a redaction overlay's `/DA` may name.
///
/// Mirrors `EditSession::resolve_dr_fonts`'s contract for form fields, one
/// level down: a synthetic `Helv → Helvetica` is ALWAYS present so the
/// overwhelmingly common `/DA /Helv 0 Tf 0 g` resolves even on a page with
/// no `/Resources /Font` at all, and every font the page does declare is
/// added under its own resource name with its `/BaseFont` mapped to a
/// standard-14 face.
///
/// Resolving against the PAGE's resources rather than the AcroForm `/DR`
/// is deliberate: a `/Redact` annotation is not a form field, its `/DA` is
/// not scoped by `/DR`, and the overlay is being baked into THIS page's
/// content stream, so the page is the only dictionary whose names are
/// guaranteed to mean the same thing after the bake.
fn overlay_font_resources(
    doc: &Document,
    page_resources: &Dict,
    da: &[u8],
) -> Vec<crate::vartext::FontResource> {
    let mut out = vec![crate::vartext::FontResource {
        name: b"Helv".to_vec(),
        font: crate::fontdata::Std14::Helvetica,
    }];
    if let Some(fonts) = page_resources
        .get(b"Font")
        .map(|o| doc.resolve(o))
        .and_then(Object::as_dict)
    {
        for (name, val) in fonts.iter() {
            let face = doc
                .resolve(val)
                .as_dict()
                .and_then(|fd| fd.get(b"BaseFont"))
                .and_then(Object::as_name)
                .and_then(|n| crate::fontdata::basefont_to_std14(n.as_bytes()))
                .unwrap_or(crate::fontdata::Std14::Helvetica);
            let nm = name.as_bytes().to_vec();
            if !out.iter().any(|r| r.name == nm) {
                out.push(crate::vartext::FontResource {
                    name: nm,
                    font: face,
                });
            }
        }
    }
    // A /DA naming a font neither the page nor the synthetic default
    // declares would otherwise fail to resolve and lose the text
    // entirely. Bind it to Helvetica so the text is DRAWN (disclosed via
    // the autosize/substitution counters) rather than dropped.
    if let Ok(parsed) = crate::vartext::parse_default_appearance(da)
        && !out.iter().any(|r| r.name == parsed.font_name)
    {
        out.push(crate::vartext::FontResource {
            name: parsed.font_name,
            font: crate::fontdata::Std14::Helvetica,
        });
    }
    out
}

// ===================================================================
// Apply orchestration — the destructive path (R35 forced full rewrite)
// ===================================================================

/// The `/Redact` annotations found on one page, resolved into geometry.
/// One page after pass 1 of [`apply_redactions`]: its surviving plan, its
/// parsed content, and the image placements the census found — everything
/// pass 2 needs, so nothing is parsed or decoded twice.
struct PreparedPage {
    /// Index into the page list.
    index: usize,
    /// The plan with retained marks already removed.
    red: PageRedaction,
    /// The page's `/Contents` stream ids.
    contents: Vec<ObjId>,
    /// The parsed, concatenated content.
    stream: ContentStream,
    /// Image placements intersecting any region (before retention).
    hits: Vec<ImageHit>,
}

struct PageRedaction {
    page_id: ObjId,
    /// Surgery regions (all quads across all marks on this page).
    boxes: Vec<RegionBox>,
    /// The `/Redact` annotation each entry of `boxes` came from, index for
    /// index — so a blocked region can retain exactly its own mark.
    box_marks: Vec<ObjId>,
    /// Overlay regions with the Table 192 marking regime each mark selected.
    overlay: Vec<(RegionBox, OverlayRegime)>,
    /// The `/Redact` annotation object ids to remove.
    redact_ids: Vec<ObjId>,
    /// Non-redact annotations intersecting a region — removed (the
    /// stricter Acrobat-parity reading; security over convenience).
    overlap_ids: Vec<ObjId>,
}

/// Apply every `/Redact` mark in `doc`: remove the covered content, scrub
/// the diligence carriers, drop the marks, and return the **full-rewrite**
/// bytes plus the [`RedactionReport`].
///
/// This is the one deliberately destructive operation in pdfcer (R35). It
/// **forces a full rewrite** — never an incremental save — so no prior
/// revision survives with the un-redacted content, and every carrier
/// scrub rides that same rewrite (an incremental scrub would leave the
/// "removed" carrier recoverable in the prior revision).
///
/// ## Images under a region are destroyed, and a mark can be retained
///
/// A raster image a region touches has the covered samples destroyed and
/// is re-encoded (or removed outright when wholly covered) — see
/// `redact_image`. When an image's samples cannot be decoded, the
/// marks touching it are **retained**: left in the output as unapplied
/// `/Redact` annotations, with nothing removed under them and no box drawn
/// over them, and counted in [`RedactionReport::marks_retained`] with a
/// note naming the placement and the reason. Every other mark applies. A
/// caller must therefore read `marks_retained` (or the `images` carrier)
/// before presenting the output as redacted.
///
/// # Errors
///
/// [`RedactError::NothingToApply`] if there are no marks;
/// [`RedactError::ImageUndestroyable`] if EVERY mark would be retained
/// (nothing could be applied); [`RedactError::Content`] if a redacted page
/// cannot be parsed; [`RedactError::Encrypted`]; [`RedactError::Write`].
pub fn apply_redactions(
    doc: &Document,
    options: &SaveOptions,
) -> Result<(Vec<u8>, RedactionReport), RedactError> {
    if doc.trailer().contains_key(b"Encrypt") {
        return Err(RedactError::Encrypted);
    }
    let pages = page_tree::pages(doc)?;

    // --- gather the marks per page ---
    let mut plan: Vec<(usize, PageRedaction, Vec<ObjId>)> = Vec::new(); // (page_index, plan, contents)
    for (index, page) in pages.iter().enumerate() {
        let Some(redaction) = gather_page(doc, page.id) else {
            continue;
        };
        plan.push((index, redaction, page.contents.clone()));
    }
    if plan.is_empty() {
        return Err(RedactError::NothingToApply);
    }

    let mut report = RedactionReport::default();
    let mut dirty = crate::writer::DirtySet::empty();
    let mut staging: Vec<u8> = Vec::new();
    let base_len = doc.bytes().len();
    let mut next_num = doc.next_object_number().unwrap_or(1);
    let mut form_intersect_any = false;
    let mut images_seen = false;
    let mut estimated_fonts: BTreeSet<String> = BTreeSet::new();
    let mut overlay_totals = OverlayOutcome::default();

    // --- pass 1: parse, census the images, decide which marks survive ---
    //
    // Every page's content is parsed and every image placement that
    // intersects a region is decoded BEFORE any page is rewritten. A
    // placement pdfcer cannot destroy retains the marks that touch it (the
    // mark stays in the document, unapplied, and is disclosed by name);
    // everything else proceeds. Doing this for the whole document first is
    // what lets the copy-on-write decision see every marked placement of a
    // shared image, on every page, before the first clone is made.
    let view = doc.view();
    let mut cache = redact_image::DecodeCache::default();
    let uses = redact_image::image_use_census(doc, &pages);
    let mut covered: std::collections::BTreeMap<ObjId, usize> = std::collections::BTreeMap::new();
    let mut prepared: Vec<PreparedPage> = Vec::new();
    let mut first_blocker: Option<(usize, String)> = None;
    for (index, red, contents) in plan {
        let page = pages.get(index).ok_or(RedactError::NothingToApply)?;
        // Parse the page's concatenated content. BASE READ (decision 018
        // caller audit): `apply_redactions` is a one-shot whole-document
        // operation over a loaded `&Document` — there is no session here,
        // and the spans it computes are consumed by the writer, which is
        // contractually a base-bytes consumer.
        let stream = ContentStream::from_page(&view, page).map_err(|e| RedactError::Content {
            page: index + 1,
            source: e,
        })?;
        let hits = census_images(doc, &page.resources, &red.boxes, &stream);
        if !hits.is_empty() {
            images_seen = true;
        }
        let mut retain: BTreeSet<ObjId> = BTreeSet::new();
        for hit in &hits {
            let Some(why) =
                redact_image::blocker(doc, &view, &page.resources, &stream.buf, hit, &mut cache)
            else {
                continue;
            };
            let (bx0, by0, bx1, by1) = hit.bbox();
            let touching: Vec<ObjId> = red
                .boxes
                .iter()
                .zip(red.box_marks.iter())
                .filter(|(rb, _)| rb.intersects(bx0, by0, bx1, by1))
                .map(|(_, m)| *m)
                .collect();
            for mark in &touching {
                if retain.insert(*mark) {
                    report.marks_retained += 1;
                    report.note(format!(
                        "redaction: page {} — mark {} 0 R was RETAINED (left in the document, \
                         unapplied) because it touches an image at ({bx0:.1}, {by0:.1}) \
                         {:.1}×{:.1} pt whose samples pdfcer could not destroy: {why}. Nothing \
                         under it was removed and no box was drawn over it; the page shows the \
                         mark, not a false redaction",
                        index + 1,
                        mark.num,
                        bx1 - bx0,
                        by1 - by0
                    ));
                }
            }
            if first_blocker.is_none() && !touching.is_empty() {
                first_blocker = Some((index + 1, why));
            }
        }
        let red = if retain.is_empty() {
            red
        } else {
            red.without_marks(doc, &retain)
        };
        if red.redact_ids.is_empty() {
            continue; // every mark on this page was retained
        }
        // Count this page's marked placements per image, for copy-on-write.
        for hit in &hits {
            if let ImageSource::XObject { id: Some(id), .. } = &hit.source
                && (redact_image::wholly_covered(hit.ctm, &red.boxes)
                    || red.boxes.iter().any(|r| hit_covers(hit, *r)))
            {
                *covered.entry(*id).or_insert(0) += 1;
            }
        }
        prepared.push(PreparedPage {
            index,
            red,
            contents,
            stream,
            hits,
        });
    }
    if prepared.is_empty() {
        let (page, reason) = first_blocker.unwrap_or((1, "unknown".to_string()));
        return Err(RedactError::ImageUndestroyable { page, reason });
    }

    // --- pass 2: the surgery, page by page ---
    let mut tombstoned: std::collections::BTreeMap<ObjId, ()> = std::collections::BTreeMap::new();
    for PreparedPage {
        index,
        red,
        contents,
        stream,
        hits,
    } in &prepared
    {
        let page = pages.get(*index).ok_or(RedactError::NothingToApply)?;
        let ov = build_overlay(doc, &page.resources, &red.overlay);
        overlay_totals.absorb(&ov);

        // Image surgery for this page (§12.5.6.23's destroy clause).
        let mut images = redact_image::ImageOutcome::default();
        {
            let mut alloc = redact_image::Allocator {
                staging: &mut staging,
                base_len,
                next_num: &mut next_num,
            };
            redact_image::plan_page(
                doc,
                &view,
                index + 1,
                &page.resources,
                &stream.buf,
                hits,
                &red.boxes,
                &uses,
                &covered,
                &mut tombstoned,
                &mut cache,
                &mut alloc,
                &mut images,
            );
        }
        for (id, obj) in images.objects.drain(..) {
            dirty.replace(id, obj);
        }
        let mut image_clones = Dict::new();
        for (name, id) in &images.bindings {
            image_clones.insert(Name::from(name.as_slice()), Object::Reference(*id));
        }
        report.images_cleared += images.cleared;
        report.images_removed += images.removed;
        report.images_cloned_shared += images.cloned_shared;
        report.images_overcovered += images.rotated_overcovered;
        for note in images.notes.drain(..) {
            report.note(note);
        }

        let result = redact_page_content(
            doc,
            &page.resources,
            &red.boxes,
            stream,
            &ov.content,
            std::mem::take(&mut images.edits),
        );

        if result.form_intersect {
            form_intersect_any = true;
        }
        report.vector_paths_intersecting += result.vector_paths_intersecting;
        report.vector_paths_cut += result.vector_paths_cut;
        report.vector_paths_dropped += result.vector_paths_dropped;
        report.vector_clips_kept += result.vector_clips_kept;
        report.shadings_intersecting += result.shadings_intersecting;
        estimated_fonts.extend(result.estimated_fonts);
        report.glyphs_removed += result.glyphs_removed;
        report.show_operators_edited += result.ops_edited;
        for t in result.removed_text {
            if !report.redacted_text.contains(&t) {
                report.redacted_text.push(t);
            }
        }

        // Rewrite the FIRST content object with the redacted+overlay bytes;
        // empty the rest. (Content streams are File-provenance, never in an
        // object stream, so save_full re-serializes them and the old glyph
        // bytes never reach the output. Emptying-in-place avoids the delete/
        // sharing traps.)
        let content_id = match contents.first() {
            Some(id) => *id,
            None => {
                // No content: create a stream just for the overlay.
                let id = ObjId::new(alloc(&mut next_num), 0);
                let span = stage(&mut staging, base_len, &result.content);
                dirty.replace(id, make_raw_stream(span, result.content.len()));
                // Wire it into the page /Contents below via the page write.
                report.content_streams_rewritten += 1;
                id
            }
        };
        if !contents.is_empty() {
            let span = stage(&mut staging, base_len, &result.content);
            dirty.replace(content_id, make_raw_stream(span, result.content.len()));
            report.content_streams_rewritten += 1;
            for extra in contents.iter().skip(1) {
                let empty = stage(&mut staging, base_len, &[]);
                dirty.replace(*extra, make_raw_stream(empty, 0));
            }
        }

        // Delete the redaction marks + overlapping annotations (and their
        // appearance/popup streams — an /AP over the region renders the
        // redacted content).
        let mut remove_annots: Vec<ObjId> = Vec::new();
        remove_annots.extend(&red.redact_ids);
        remove_annots.extend(&red.overlap_ids);
        for aid in &remove_annots {
            for sub in appearance_children(doc, *aid) {
                dirty.delete(sub);
            }
            dirty.delete(*aid);
            report.annotations_removed += 1;
        }

        // Rewrite the page dict: /Contents -> [content_id], /Annots with the
        // removed marks/overlaps gone, /Thumb dropped.
        let page_write = rewrite_page_dict(
            doc,
            red.page_id,
            content_id,
            &remove_annots,
            &ov.fonts,
            &image_clones,
        );
        if let Some((new_dict, thumb)) = page_write {
            dirty.replace(red.page_id, Object::Dict(new_dict));
            if let Some(thumb_id) = thumb {
                dirty.delete(thumb_id);
            }
        }
        report.pages_redacted += 1;
        report.marks_applied += red.redact_ids.len() as u64;
    }

    for f in &estimated_fonts {
        report.note(format!(
            "redaction: advance widths for font {f} were estimated (no /Widths) — survivor \
             positioning is approximate; the removal itself is unaffected"
        ));
    }
    report.estimated_width_fonts = estimated_fonts.len() as u64;

    // --- overlay-marking disclosures (Table 192 ladder, project rule 4) ---
    //
    // Every one of these describes something an operator CANNOT see by
    // looking at the result: a burnt-in overlay looks equally deliberate
    // whether pdfcer chose the size or the author did, a fallback box looks
    // exactly like an intended box, and a transparent region looks like a
    // region nothing happened to. The annotation that carried the evidence
    // is deleted by this same operation, so this report is the only place
    // the information can still exist.
    report.overlay_text_burned = overlay_totals.text_regions;
    report.overlay_ro_not_drawn = overlay_totals.ro_regions;
    report.overlay_transparent = overlay_totals.transparent_regions;
    if report.images_overcovered > 0 {
        report.note(format!(
            "redaction: {} image placement(s) are rotated or skewed, so the destroyed cells \
             are the region's bounding rectangle in image space — more than the region, \
             never less",
            report.images_overcovered
        ));
    }
    if overlay_totals.text_regions > 0 {
        report.note(format!(
            "redaction: overlay text burnt into {} region(s) (ISO 32000-1 Table 192 \
             /OverlayText, formatted by /DA and justified by /Q)",
            overlay_totals.text_regions
        ));
    }
    if overlay_totals.ro_regions > 0 {
        report.note(format!(
            "redaction: {} region(s) carried an /RO overlay appearance that pdfcer does NOT \
             draw this build — a plain /IC-coloured box (black when no /IC) was painted \
             instead, so the region IS marked but NOT with the appearance its author \
             supplied; the content removal itself is unaffected",
            overlay_totals.ro_regions
        ));
    }
    if overlay_totals.transparent_regions > 0 {
        report.note(format!(
            "redaction: {} region(s) were left TRANSPARENT — the mark carried no /RO, \
             /OverlayText or /IC, and Table 192 says an absent /IC leaves the interior \
             transparent; the content is removed but the region carries no visible mark",
            overlay_totals.transparent_regions
        ));
    }
    if overlay_totals.repeat_ignored > 0 {
        report.note(format!(
            "redaction: /Repeat true was IGNORED on {} region(s) — the overlay text was \
             drawn once, not tiled to fill the region",
            overlay_totals.repeat_ignored
        ));
    }
    if overlay_totals.da_substituted > 0 {
        report.note(format!(
            "redaction: {} region(s) had /OverlayText with no /DA, which Table 192 makes \
             required — pdfcer substituted auto-sized Helvetica in black",
            overlay_totals.da_substituted
        ));
    }
    if !overlay_totals.autosizes.is_empty() {
        let sizes: Vec<String> = overlay_totals
            .autosizes
            .iter()
            .map(|s| format!("{s:.1}"))
            .collect();
        report.note(format!(
            "redaction: overlay text auto-sized (/DA size 0) to {} pt — pdfcer's heuristic, \
             not a spec formula",
            sizes.join(", ")
        ));
    }
    if overlay_totals.unencodable_chars > 0 {
        report.note(format!(
            "redaction: {} overlay-text character(s) have no WinAnsi code and were drawn as \
             '?' — the overlay text is Base-14 Latin only this build",
            overlay_totals.unencodable_chars
        ));
    }
    for failure in &overlay_totals.text_failures {
        report.note(format!(
            "redaction: overlay text could NOT be laid out and was not drawn ({failure}); \
             the region carries only its /IC fill, if any"
        ));
    }

    // --- carrier sweep (the §12.5.6.23 diligence obligation) ---
    let redacted_text = report.redacted_text.clone();
    carrier_info(doc, &redacted_text, &mut dirty, &mut report);
    carrier_xmp(
        doc,
        &redacted_text,
        &mut staging,
        base_len,
        &mut dirty,
        &mut report,
    );
    carrier_detect_disclose(doc, form_intersect_any, images_seen, &mut report);

    // --- container decomposition (§7.5.7 Strategy B) ---
    decompose_containers(doc, &mut dirty, &mut report);

    // Prior revisions are dropped by the full rewrite itself.
    report.add_carrier("prior_revisions", true, CarrierAction::DroppedByRewrite);

    // --- forced FULL REWRITE (R35) ---
    if !staging.is_empty() {
        dirty.set_staging(staging);
    }
    let (bytes, _save) = save_full(doc, &dirty, options)?;
    Ok((bytes, report))
}

/// Allocate the next object number, advancing the counter.
fn alloc(next: &mut u32) -> u32 {
    let n = *next;
    *next = next.saturating_add(1);
    n
}

/// Append `bytes` to the staging buffer and return their combined-space
/// span (base ++ staging), so a created stream keeps the span model.
fn stage(staging: &mut Vec<u8>, base_len: usize, bytes: &[u8]) -> ByteSpan {
    let start = base_len + staging.len();
    staging.extend_from_slice(bytes);
    ByteSpan::new(start, bytes.len())
}

/// A raw (unfiltered) content stream object with the given data span and
/// length. No `/Filter`: the redacted content is emitted verbatim.
fn make_raw_stream(span: ByteSpan, len: usize) -> Object {
    let mut dict = Dict::new();
    dict.insert(
        Name::from(b"Length"),
        Object::Integer(i64::try_from(len).unwrap_or(i64::MAX)),
    );
    Object::Stream(Stream {
        dict,
        data_span: span,
    })
}

/// Resolve one page's `/Redact` annotations into geometry, or `None` if
/// the page carries none.
fn gather_page(doc: &Document, page_id: ObjId) -> Option<PageRedaction> {
    let page = doc
        .get(page_id)
        .map(|io| &io.value)
        .and_then(Object::as_dict)?;
    let annots = page
        .get(b"Annots")
        .map(|o| doc.resolve(o))
        .and_then(Object::as_array)?;

    let mut boxes = Vec::new();
    let mut box_marks = Vec::new();
    let mut overlay = Vec::new();
    let mut redact_ids = Vec::new();
    let mut other: Vec<(ObjId, RegionBox)> = Vec::new();

    for entry in annots {
        let Some(aid) = entry.as_reference() else {
            continue;
        };
        let Some(dict) = doc.get(aid).map(|io| &io.value).and_then(Object::as_dict) else {
            continue;
        };
        let subtype = dict
            .get(b"Subtype")
            .map(|o| doc.resolve(o))
            .and_then(Object::as_name)
            .map(|n| n.as_bytes().to_vec())
            .unwrap_or_default();
        if subtype == b"Redact" {
            let regime = annot_overlay(doc, dict);
            for rb in annot_regions(doc, dict) {
                boxes.push(rb);
                box_marks.push(aid);
                overlay.push((rb, regime.clone()));
            }
            redact_ids.push(aid);
        } else if let Some(rb) = annot_rect_box(doc, dict) {
            other.push((aid, rb));
        }
    }
    if redact_ids.is_empty() {
        return None;
    }
    // Overlapping non-redact annotations → removed (stricter reading).
    let mut overlap_ids = Vec::new();
    for (aid, rb) in other {
        if boxes
            .iter()
            .any(|r| r.intersects(rb.min_x, rb.min_y, rb.max_x, rb.max_y))
        {
            overlap_ids.push(aid);
        }
    }
    Some(PageRedaction {
        page_id,
        boxes,
        box_marks,
        overlay,
        redact_ids,
        overlap_ids,
    })
}

impl PageRedaction {
    /// This page's plan with the marks in `retain` removed: their regions
    /// are not surgery targets, their overlays are not drawn, their
    /// annotations are not deleted, and the overlapping-annotation set is
    /// recomputed against the regions that remain.
    fn without_marks(&self, doc: &Document, retain: &BTreeSet<ObjId>) -> Self {
        let mut boxes = Vec::new();
        let mut box_marks = Vec::new();
        let mut overlay = Vec::new();
        for (i, rb) in self.boxes.iter().enumerate() {
            let Some(mark) = self.box_marks.get(i) else {
                continue;
            };
            if retain.contains(mark) {
                continue;
            }
            boxes.push(*rb);
            box_marks.push(*mark);
            if let Some(ov) = self.overlay.get(i) {
                overlay.push(ov.clone());
            }
        }
        let redact_ids: Vec<ObjId> = self
            .redact_ids
            .iter()
            .copied()
            .filter(|id| !retain.contains(id))
            .collect();
        let overlap_ids: Vec<ObjId> = self
            .overlap_ids
            .iter()
            .copied()
            .filter(|aid| {
                let Some(dict) = doc.get(*aid).map(|io| &io.value).and_then(Object::as_dict) else {
                    return false;
                };
                annot_rect_box(doc, dict).is_some_and(|rb| {
                    boxes
                        .iter()
                        .any(|r| r.intersects(rb.min_x, rb.min_y, rb.max_x, rb.max_y))
                })
            })
            .collect();
        Self {
            page_id: self.page_id,
            boxes,
            box_marks,
            overlay,
            redact_ids,
            overlap_ids,
        }
    }
}

/// The regions a `/Redact` annotation covers: its `/QuadPoints`
/// (8×n numbers → n quads) if present, else its `/Rect` (Table 192).
fn annot_regions(doc: &Document, dict: &Dict) -> Vec<RegionBox> {
    if let Some(qp) = dict
        .get(b"QuadPoints")
        .map(|o| doc.resolve(o))
        .and_then(Object::as_array)
    {
        let nums: Vec<f64> = qp
            .iter()
            .filter_map(|o| doc.resolve(o).as_number())
            .collect();
        let mut boxes = Vec::new();
        for quad in nums.chunks_exact(8) {
            let [x0, y0, x1, y1, x2, y2, x3, y3] = quad else {
                continue;
            };
            let xs = [*x0, *x1, *x2, *x3];
            let ys = [*y0, *y1, *y2, *y3];
            boxes.push(RegionBox {
                min_x: xs.iter().copied().fold(f64::INFINITY, f64::min),
                min_y: ys.iter().copied().fold(f64::INFINITY, f64::min),
                max_x: xs.iter().copied().fold(f64::NEG_INFINITY, f64::max),
                max_y: ys.iter().copied().fold(f64::NEG_INFINITY, f64::max),
            });
        }
        if !boxes.is_empty() {
            return boxes;
        }
    }
    annot_rect_box(doc, dict).into_iter().collect()
}

/// An annotation's `/Rect` as a [`RegionBox`].
fn annot_rect_box(doc: &Document, dict: &Dict) -> Option<RegionBox> {
    let arr = dict
        .get(b"Rect")
        .map(|o| doc.resolve(o))
        .and_then(Object::as_array)?;
    let n: Vec<f64> = arr
        .iter()
        .filter_map(|o| doc.resolve(o).as_number())
        .collect();
    if let [x0, y0, x1, y1] = n.as_slice() {
        Some(RegionBox::from_rect(Rect::from_corners(*x0, *y0, *x1, *y1)))
    } else {
        None
    }
}

/// A `/Redact` mark's fill colour: `/IC` (DeviceRGB, three numbers) or the
/// default black. `/IC` is ignored if `/RO` is present (Table 192); RO
/// burn-in is a named follow-up, so this build honours `/IC`/default.
fn annot_fill(doc: &Document, dict: &Dict) -> Option<[f64; 3]> {
    let ic = dict
        .get(b"IC")
        .map(|o| doc.resolve(o))
        .and_then(Object::as_array)?;
    let n: Vec<f64> = ic
        .iter()
        .filter_map(|o| doc.resolve(o).as_number())
        .collect();
    match n.as_slice() {
        [r, g, b] => Some([*r, *g, *b]),
        _ => None,
    }
}

/// Which overlay-marking regime a `/Redact` mark selects, per the "ignored
/// if" chain in ISO 32000-1 Table 192 (§12.5.6.23).
///
/// The standard states the precedence as four separate "ignored if"
/// clauses spread across five table rows rather than as a ladder, which is
/// why it is reified here as one type: reading those clauses as
/// independent booleans is how a decoder ends up drawing `/IC` underneath
/// an `/RO` that was supposed to suppress it. The derived ladder is:
///
/// ```text
/// RO present        -> draw the RO form XObject at the annotation
///                      rectangle's lower-left; IC/OverlayText/DA/Q ALL ignored
/// else OverlayText  -> IC fills the region first (IC is "ignored if RO",
///                      NOT "ignored if OverlayText"), then the text is
///                      drawn over it, formatted by DA and justified by Q
/// else IC present   -> fill the region with that DeviceRGB colour
/// else              -> leave the region TRANSPARENT
/// ```
///
/// The last rung is the one most easily got wrong, and pdfcer got it wrong
/// until this Pass: Table 192's `/IC` row says in as many words that "if
/// this entry is absent, the interior of the redaction region is left
/// transparent". Defaulting an absent `/IC` to black paints a box the
/// standard says not to paint — the same shape of defect as painting a
/// `/Separation /None` image (§8.6.6.4), where "it looks like what people
/// expect" is not the same as "the standard permits it".
#[derive(Debug, Clone)]
enum OverlayRegime {
    /// `/RO` is present. This build cannot bake a form XObject into page
    /// content, so it discloses that and falls back to a visible box: the
    /// `/IC` colour when one is given, black otherwise.
    ///
    /// Falling back to *something visible* rather than to the spec's
    /// transparent default is deliberate and is a redaction-safety
    /// judgement, not a spec reading: a mark whose author went to the
    /// trouble of supplying a custom overlay appearance plainly intended
    /// the region to be marked, and silently leaving it bare would be the
    /// one failure mode this feature cannot have.
    Ro { fallback: [f64; 3] },
    /// `/OverlayText` is present (and `/RO` is not).
    Text {
        /// `/IC`, drawn UNDER the text when present. `None` leaves the
        /// region transparent behind the glyphs.
        fill: Option<[f64; 3]>,
        /// The decoded `/OverlayText` string.
        text: String,
        /// `/DA` — conditionally REQUIRED by Table 192 whenever
        /// `/OverlayText` is present. A mark that omits it is malformed;
        /// pdfcer substitutes a default and discloses rather than refusing,
        /// because the removal has already happened by this point and
        /// aborting would leave the document redacted but unmarked.
        da: Vec<u8>,
        /// `/Q` justification (default 0, left).
        quad: Quadding,
        /// `/Repeat` — tile the text to fill the region. Not implemented;
        /// disclosed when true.
        repeat: bool,
    },
    /// `/IC` alone.
    Fill([f64; 3]),
    /// No `/RO`, no `/OverlayText`, no `/IC` — Table 192's transparent
    /// default.
    Transparent,
}

/// Resolve one `/Redact` annotation dictionary to its [`OverlayRegime`].
///
/// Reads, in ladder order, `/RO`, `/OverlayText`, `/IC`, `/DA`, `/Q` and
/// `/Repeat`. `/OverlayText` is a PDF text string (§7.9.2.2), so it is
/// decoded through [`crate::textstring::decode_text_string`] rather than
/// treated as bytes — a producer is free to emit it UTF-16BE, and reading
/// those bytes as PDFDocEncoding would burn mojibake into the page
/// permanently, on the one operation that cannot be undone.
fn annot_overlay(doc: &Document, dict: &Dict) -> OverlayRegime {
    let fill = annot_fill(doc, dict);
    if dict.get(b"RO").is_some() {
        return OverlayRegime::Ro {
            fallback: fill.unwrap_or([0.0, 0.0, 0.0]),
        };
    }
    let overlay_text = dict
        .get(b"OverlayText")
        .map(|o| doc.resolve(o))
        .and_then(|o| match o {
            Object::String(s) => Some(s.as_slice()),
            _ => None,
        })
        .map(|s| crate::textstring::decode_text_string(s).text);
    if let Some(text) = overlay_text {
        let da = dict
            .get(b"DA")
            .map(|o| doc.resolve(o))
            .and_then(|o| match o {
                Object::String(s) => Some(s.clone()),
                _ => None,
            })
            .unwrap_or_default();
        let quad = dict
            .get(b"Q")
            .map(|o| doc.resolve(o))
            .and_then(|o| match o {
                Object::Integer(i) => Some(*i),
                _ => None,
            })
            .map_or(Quadding::Left, Quadding::from_code);
        let repeat = dict
            .get(b"Repeat")
            .map(|o| doc.resolve(o))
            .and_then(|o| match o {
                Object::Boolean(v) => Some(*v),
                _ => None,
            })
            .unwrap_or(false);
        return OverlayRegime::Text {
            fill,
            text,
            da,
            quad,
            repeat,
        };
    }
    match fill {
        Some(rgb) => OverlayRegime::Fill(rgb),
        None => OverlayRegime::Transparent,
    }
}

/// The appearance/popup child object ids of an annotation (its `/AP`
/// `/N`/`/D`/`/R` streams and `/Popup`), which must be deleted with it so
/// no rendered copy of the redacted content survives.
fn appearance_children(doc: &Document, annot_id: ObjId) -> Vec<ObjId> {
    let mut out = Vec::new();
    let Some(dict) = doc
        .get(annot_id)
        .map(|io| &io.value)
        .and_then(Object::as_dict)
    else {
        return out;
    };
    if let Some(ap) = dict
        .get(b"AP")
        .map(|o| doc.resolve(o))
        .and_then(Object::as_dict)
    {
        for (_k, v) in ap.iter() {
            collect_refs(doc, v, &mut out);
        }
    }
    if let Some(Object::Reference(p)) = dict.get(b"Popup") {
        out.push(*p);
    }
    out
}

/// Collect the indirect-reference ids reachable one level down from `obj`
/// (a stream reference, or a sub-dictionary of appearance-state
/// references).
fn collect_refs(_doc: &Document, obj: &Object, out: &mut Vec<ObjId>) {
    match obj {
        Object::Reference(id) => out.push(*id),
        Object::Dict(d) => {
            for (_k, v) in d.iter() {
                if let Object::Reference(id) = v {
                    out.push(*id);
                }
            }
        }
        _ => {}
    }
}

/// Build the rewritten page dictionary: `/Contents -> [content_id]`,
/// `/Annots` with `remove` filtered out, `/Thumb` dropped. Returns the new
/// dict and the dropped `/Thumb` object id, or `None` if the page dict is
/// unreadable.
fn rewrite_page_dict(
    doc: &Document,
    page_id: ObjId,
    content_id: ObjId,
    remove: &[ObjId],
    overlay_fonts: &Dict,
    image_clones: &Dict,
) -> Option<(Dict, Option<ObjId>)> {
    let page = doc
        .get(page_id)
        .map(|io| &io.value)
        .and_then(Object::as_dict)?;
    let mut updated = page.clone();
    updated.insert(
        Name::from(b"Contents"),
        Object::Array(vec![Object::Reference(content_id)]),
    );
    // A burnt-in `/OverlayText` block names a font resource, so that name
    // must resolve from THIS page after the bake.
    //
    // The merge writes an explicit `/Resources` onto the page even when the
    // page previously INHERITED one from an ancestor `/Pages` node (§7.7.3.4).
    // That is a deliberate, narrow denormalisation: the effective resource
    // set is preserved exactly (the inherited dictionary is resolved and
    // copied first), and it is confined to pages that actually gained an
    // overlay font. Mutating the shared ancestor instead would silently
    // change every OTHER page that inherits from it — a much larger edit
    // than the operator asked for, on the one operation that forces a full
    // rewrite and has no undo.
    //
    // An existing binding for the same name is NEVER overwritten: the page's
    // own font wins, and `overlay_font_resources` has already laid the text
    // out against that same face, so the two agree.
    //
    // The same denormalisation carries the image surgery's copy-on-write
    // clones: a fresh `/XObject` name per cleared placement, bound on THIS
    // page only, so a shared original keeps serving every other page.
    if !overlay_fonts.is_empty() || !image_clones.is_empty() {
        let mut resources = page
            .get(b"Resources")
            .map(|o| doc.resolve(o))
            .and_then(Object::as_dict)
            .cloned()
            .unwrap_or_default();
        if !overlay_fonts.is_empty() {
            let mut fonts = resources
                .get(b"Font")
                .map(|o| doc.resolve(o))
                .and_then(Object::as_dict)
                .cloned()
                .unwrap_or_default();
            for (name, val) in overlay_fonts.iter() {
                if fonts.get(name.as_bytes()).is_none() {
                    fonts.insert(name.clone(), val.clone());
                }
            }
            resources.insert(Name::from(b"Font"), Object::Dict(fonts));
        }
        if !image_clones.is_empty() {
            let mut xobjects = resources
                .get(b"XObject")
                .map(|o| doc.resolve(o))
                .and_then(Object::as_dict)
                .cloned()
                .unwrap_or_default();
            for (name, val) in image_clones.iter() {
                xobjects.insert(name.clone(), val.clone());
            }
            resources.insert(Name::from(b"XObject"), Object::Dict(xobjects));
        }
        updated.insert(Name::from(b"Resources"), Object::Dict(resources));
    }
    // /Annots: drop the removed refs. If it is an indirect array, inline a
    // fresh direct array (simplest correct rewrite for the destructive path).
    if let Some(annots) = page
        .get(b"Annots")
        .map(|o| doc.resolve(o))
        .and_then(Object::as_array)
    {
        let kept: Vec<Object> = annots
            .iter()
            .filter(|o| o.as_reference().is_none_or(|id| !remove.contains(&id)))
            .cloned()
            .collect();
        updated.insert(Name::from(b"Annots"), Object::Array(kept));
    }
    let thumb = match page.get(b"Thumb") {
        Some(Object::Reference(id)) => Some(*id),
        _ => None,
    };
    if thumb.is_some() {
        updated.remove(b"Thumb");
    }
    Some((updated, thumb))
}

/// Carrier: `/Info` — remove any string entry whose bytes contain a
/// redacted string (over-scrub: drop the whole entry). The scrub rides the
/// forced full rewrite, so the old `/Info` object's bytes do not survive.
fn carrier_info(
    doc: &Document,
    redacted: &[String],
    dirty: &mut crate::writer::DirtySet,
    report: &mut RedactionReport,
) {
    let Some(info_id) = doc.trailer().get(b"Info").and_then(Object::as_reference) else {
        report.add_carrier("info", false, CarrierAction::Absent);
        return;
    };
    let Some(info) = doc
        .get(info_id)
        .map(|io| &io.value)
        .and_then(Object::as_dict)
    else {
        report.add_carrier("info", false, CarrierAction::Absent);
        return;
    };
    let mut updated = info.clone();
    let mut changed = 0u64;
    let keys: Vec<Name> = info.iter().map(|(k, _)| k.clone()).collect();
    for key in keys {
        if let Some(Object::String(bytes)) = info.get(key.as_bytes())
            && redacted.iter().any(|t| bytes_contain_text(bytes, t))
        {
            updated.remove(key.as_bytes());
            changed += 1;
        }
    }
    if changed > 0 {
        report.info_strings_scrubbed = changed;
        dirty.replace(info_id, Object::Dict(updated));
        report.add_carrier("info", true, CarrierAction::Scrubbed);
    } else {
        report.add_carrier("info", true, CarrierAction::Absent);
    }
}

/// Carrier: XMP `/Metadata` — decode the packet, blank every occurrence of
/// a redacted string, and re-emit it **raw** (dropping any filter) so the
/// scrubbed packet cannot survive compressed either.
fn carrier_xmp(
    doc: &Document,
    redacted: &[String],
    staging: &mut Vec<u8>,
    base_len: usize,
    dirty: &mut crate::writer::DirtySet,
    report: &mut RedactionReport,
) {
    let meta = doc
        .catalog()
        .ok()
        .and_then(|c| c.get(b"Metadata").map(|o| doc.resolve(o)).cloned());
    let Some(Object::Stream(stream)) = meta else {
        report.add_carrier("xmp", false, CarrierAction::Absent);
        return;
    };
    let meta_id = doc
        .catalog()
        .ok()
        .and_then(|c| c.get(b"Metadata").and_then(Object::as_reference));
    let Some(meta_id) = meta_id else {
        report.add_carrier("xmp", true, CarrierAction::Absent);
        return;
    };
    let Some(raw) = stream.data_span.slice(doc.bytes()) else {
        report.add_carrier("xmp", true, CarrierAction::Absent);
        return;
    };
    let decoded = crate::filters::decode_stream(&stream.dict, raw).unwrap_or_else(|_| raw.to_vec());
    let mut scrubbed = decoded.clone();
    let mut changed = false;
    for t in redacted {
        if replace_all_bytes(&mut scrubbed, t.as_bytes(), b'X') {
            changed = true;
        }
        // Also the UTF-16BE encoding, in case the packet is UTF-16.
        let u16be = utf16be(t);
        if replace_all_bytes(&mut scrubbed, &u16be, b'X') {
            changed = true;
        }
    }
    if !changed {
        report.add_carrier("xmp", true, CarrierAction::Absent);
        return;
    }
    let mut dict = stream.dict.clone();
    dict.remove(b"Filter");
    dict.remove(b"DecodeParms");
    dict.insert(
        Name::from(b"Length"),
        Object::Integer(i64::try_from(scrubbed.len()).unwrap_or(i64::MAX)),
    );
    let span = stage(staging, base_len, &scrubbed);
    dirty.replace(
        meta_id,
        Object::Stream(Stream {
            dict,
            data_span: span,
        }),
    );
    report.add_carrier("xmp", true, CarrierAction::Scrubbed);
}

/// Carriers pdfcer **detects but does not scrub** this build — disclosed as
/// residuals for manual verification (never silently left).
fn carrier_detect_disclose(
    doc: &Document,
    form_intersect: bool,
    images_seen: bool,
    report: &mut RedactionReport,
) {
    let catalog = doc.catalog().ok();

    // Raster images (§12.5.6.23: "that portion of the image data shall be
    // destroyed"). Present when any placement intersected a region;
    // scrubbed when every such placement was destroyed or removed; disclosed
    // when a mark had to be retained over an image pdfcer could not decode.
    let images_action = if !images_seen {
        CarrierAction::Absent
    } else if report.marks_retained > 0 {
        CarrierAction::DisclosedNotScrubbed
    } else {
        CarrierAction::Scrubbed
    };
    report.add_carrier("images", images_seen, images_action);

    // XFA — a parallel XML copy of form/text content (§12.5.6.23 names it).
    let xfa = catalog
        .and_then(|c| {
            c.get(b"AcroForm")
                .map(|o| doc.resolve(o))
                .and_then(Object::as_dict)
        })
        .is_some_and(|acro| acro.contains_key(b"XFA"));
    if xfa {
        report.add_carrier("xfa", true, CarrierAction::DisclosedNotScrubbed);
        report.note(
            "redaction: /AcroForm /XFA present — the XFA XML may duplicate redacted content and \
             was NOT scrubbed (pdfcer is XFA detect-only); verify or remove XFA manually"
                .to_string(),
        );
    } else {
        report.add_carrier("xfa", false, CarrierAction::Absent);
    }

    // Structure tree /ActualText/Alt/E — tagged replacement text that an
    // extractor reads even after glyph removal.
    let struct_tree = catalog.is_some_and(|c| c.contains_key(b"StructTreeRoot"));
    if struct_tree {
        report.add_carrier("struct_tree", true, CarrierAction::DisclosedNotScrubbed);
        report.note(
            "redaction: /StructTreeRoot present — tagged /ActualText//Alt//E may duplicate \
             redacted glyphs and was NOT scrubbed; verify the structure tree manually"
                .to_string(),
        );
    } else {
        report.add_carrier("struct_tree", false, CarrierAction::Absent);
    }

    // Embedded files / attachments — whole documents outside region scope.
    let attachments = catalog.is_some_and(|c| {
        c.get(b"Names")
            .map(|o| doc.resolve(o))
            .and_then(Object::as_dict)
            .is_some_and(|n| n.contains_key(b"EmbeddedFiles"))
    });
    if attachments {
        report.add_carrier("attachments", true, CarrierAction::DisclosedNotScrubbed);
        report.note(
            "redaction: embedded files (/Names /EmbeddedFiles) present — out of region scope and \
             NOT scrubbed; review attachments manually"
                .to_string(),
        );
    } else {
        report.add_carrier("attachments", false, CarrierAction::Absent);
    }

    // OCG layers — redacted by GEOMETRY (the interpreter walks all content
    // regardless of optional-content visibility), so covered content in an
    // OFF layer is still removed. Reported as scrubbed-by-geometry.
    let ocg = catalog.is_some_and(|c| c.contains_key(b"OCProperties"));
    if ocg {
        report.add_carrier("ocg", true, CarrierAction::Scrubbed);
        report.note(
            "redaction: optional-content (/OCProperties) present — redacted by GEOMETRY (layer \
             visibility ignored), so content in hidden layers within a region was still removed"
                .to_string(),
        );
    } else {
        report.add_carrier("ocg", false, CarrierAction::Absent);
    }

    if form_intersect {
        report.note(
            "redaction: a form XObject overlaps a redaction region — its content was NOT \
             surgically redacted this build; verify manually or flatten the form first"
                .to_string(),
        );
    }

    // Vector paths (§8.5) crossing a region. Cut by the surgery
    // interpreter (`redact_vector`); the residual is the malformed object
    // that could not be rewritten as a unit. On a CAD sheet this is the
    // carrier that matters most, and it must never read as "redacted"
    // when anything was left.
    let uncut = report.vector_paths_intersecting;
    let cut = report.vector_paths_cut;
    if uncut > 0 {
        report.add_carrier("vector_paths", true, CarrierAction::DisclosedNotScrubbed);
        report.note(format!(
            "redaction: {uncut} painted vector path(s) cross a redaction region and were NOT \
             cut — each is a malformed path object carrying an operator ISO 32000-1 §8.2 \
             forbids between construction and painting, so its bytes could not be replaced \
             as a unit; those lines, fills or curves remain in the content stream; verify the \
             region by eye"
        ));
    } else if cut > 0 {
        report.add_carrier("vector_paths", true, CarrierAction::Scrubbed);
    } else {
        report.add_carrier("vector_paths", false, CarrierAction::Absent);
    }
    if cut > 0 {
        report.note(format!(
            "redaction: {cut} vector path object(s) crossing a region were CUT ({} deleted \
             outright as wholly covered) — strokes cut against the region expanded by their \
             stroke width, fills clipped to the region's complement, every piece re-emitted in \
             its own coordinates; the pieces are new path objects, so a dashed stroke restarts \
             its dash phase at each cut",
            report.vector_paths_dropped
        ));
    }
    // Shadings (§8.7.4.5.1): `sh` paints the whole current clip, and the
    // interpreter tracks that clip only as a box. Not cut; disclosed.
    let shadings = report.shadings_intersecting;
    if shadings > 0 {
        report.add_carrier("shadings", true, CarrierAction::DisclosedNotScrubbed);
        report.note(format!(
            "redaction: {shadings} shading paint(s) (`sh`) have a clipping region that meets a \
             redaction region and were NOT cut — a shading fills its whole clip, and pdfcer does \
             not cut shadings this build; whatever the shading painted inside the region is \
             still painted; verify the region by eye"
        ));
    } else {
        report.add_carrier("shadings", false, CarrierAction::Absent);
    }
    if report.vector_clips_kept > 0 {
        report.note(format!(
            "redaction: {} cut path object(s) also set a clipping path (W/W*); the paint was \
             cut but the ORIGINAL geometry was kept as the clip, because the clip applies after \
             painting and shrinking it would hide unmarked content drawn later on the page — \
             the kept geometry is not painted, but it is a shape in the file; review it if the \
             clip itself could be sensitive",
            report.vector_clips_kept
        ));
    }
}

/// Container decomposition (§7.5.7 Strategy B): any object the redaction
/// removed or replaced that lives in an object stream would otherwise
/// survive verbatim inside its untouched container (pdfcer's `save_full`
/// re-emits `/ObjStm` intact by design). Promote every survivor of such a
/// container out to file level and drop the container, so no removed byte
/// survives compressed.
fn decompose_containers(
    doc: &Document,
    dirty: &mut crate::writer::DirtySet,
    report: &mut RedactionReport,
) {
    // Snapshot the objects the redaction already touches.
    let touched: BTreeSet<ObjId> = dirty.iter().collect();
    // Which object-stream containers hold a touched object?
    let mut containers: BTreeSet<ObjId> = BTreeSet::new();
    for id in &touched {
        if let Some(io) = doc.get(*id)
            && let Some(c) = io.provenance.container()
        {
            containers.insert(c);
        }
    }
    if containers.is_empty() {
        report.add_carrier("object_streams", false, CarrierAction::Absent);
        return;
    }
    let mut promoted = 0u64;
    for container in &containers {
        for io in doc.objects() {
            if io.provenance.container() == Some(*container) && !touched.contains(&io.id) {
                // Promote the survivor: replacing it with its current value
                // makes save_full write it at file level (type-1),
                // superseding the type-2 entry.
                dirty.replace(io.id, io.value.clone());
                promoted += 1;
            }
        }
        // Drop the now-empty container so its verbatim bytes (holding the
        // removed object) are never emitted.
        dirty.delete(*container);
    }
    report.containers_decomposed = containers.len() as u64;
    report.objects_promoted = promoted;
    report.add_carrier("object_streams", true, CarrierAction::DroppedByRewrite);
    report.note(format!(
        "redaction: decomposed {} object stream(s), promoting {} survivor(s) out so no removed \
         object survives compressed (ISO 32000-1 §7.5.7)",
        containers.len(),
        promoted
    ));
}

/// Whether `value` (raw PDF string bytes) contains `needle` in either its
/// ASCII/PDFDocEncoding form or its UTF-16BE form (§7.9.2's two text-string
/// encodings). Case-insensitive on the ASCII form.
fn bytes_contain_text(value: &[u8], needle: &str) -> bool {
    if needle.is_empty() {
        return false;
    }
    let hay_lower: Vec<u8> = value.iter().map(u8::to_ascii_lowercase).collect();
    let need_lower: Vec<u8> = needle.bytes().map(|b| b.to_ascii_lowercase()).collect();
    if contains_subslice(&hay_lower, &need_lower) {
        return true;
    }
    let u16be = utf16be(needle);
    contains_subslice(value, &u16be)
}

/// UTF-16BE encoding of a string (no BOM).
fn utf16be(s: &str) -> Vec<u8> {
    let mut out = Vec::new();
    for u in s.encode_utf16() {
        out.push((u >> 8) as u8);
        out.push((u & 0xff) as u8);
    }
    out
}

/// Replace every occurrence of `needle` in `hay` with `fill`-repeated
/// bytes of the same length. Returns whether anything changed.
fn replace_all_bytes(hay: &mut [u8], needle: &[u8], fill: u8) -> bool {
    if needle.is_empty() || needle.len() > hay.len() {
        return false;
    }
    let mut changed = false;
    let mut i = 0;
    while i + needle.len() <= hay.len() {
        if hay.get(i..i + needle.len()) == Some(needle) {
            for j in i..i + needle.len() {
                if let Some(slot) = hay.get_mut(j) {
                    *slot = fill;
                }
            }
            changed = true;
            i += needle.len();
        } else {
            i += 1;
        }
    }
    changed
}

/// First index of `needle` in `hay`, else `None`.
fn contains_subslice(hay: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() || needle.len() > hay.len() {
        return false;
    }
    hay.windows(needle.len()).any(|w| w == needle)
}

/// One `/Redact` mark located in a document, as the review surfaces need
/// it: which page carries it, which object it is, and where it sits.
///
/// A **display projection of an actual annotation**, produced fresh by
/// [`redaction_marks`] on every call — never a cached list a UI keeps and
/// patches incrementally. That is the same discipline
/// [`count_redaction_marks`] already enforces for the count, and it exists
/// for the same reason: a review list that can drift from the document is a
/// review list that can tell an operator a mark was deleted when it was not,
/// on the one feature where a wrong answer is a leak.
///
/// `rect` is the annotation's `/Rect` **as stored**, normalised to
/// `[llx, lly, urx, ury]` (Table 166 permits either diagonal), or `None`
/// when the annotation carries no usable `/Rect`. It is display information
/// only — the geometry apply actually removes comes from `/QuadPoints` when
/// present (see [`apply_redactions`]), so this must never be used to decide
/// *what* gets removed, only to describe a mark to a human.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RedactionMark {
    /// 0-based index into the document's flattened page list.
    pub page_index: usize,
    /// The annotation object itself — the stable identity a review surface
    /// addresses a single mark by (a row's remove button, a jump-to click).
    pub annot_id: ObjId,
    /// `[llx, lly, urx, ury]` in default user space, or `None` when the
    /// annotation has no usable `/Rect`.
    pub rect: Option<[f64; 4]>,
}

/// Enumerate every `/Redact` mark in the document, in page order then
/// `/Annots` order.
///
/// The list form of [`count_redaction_marks`], which now delegates to it so
/// the two can never disagree about what a mark is — a count that says "3"
/// beside a list that shows 2 rows is a defect an operator has no way to
/// resolve, and the cheapest fix is to make it structurally impossible.
///
/// Generic over [`ObjectGraph`] for exactly the reason
/// [`count_redaction_marks`] is (see its docs): the GUI must pass
/// `&session.graph()` so a mark authored **this session** — the one most
/// likely to be forgotten — is enumerated, while `&Document` callers keep
/// compiling unchanged.
///
/// Returns an empty vector rather than an error when the page tree cannot
/// be walked: a review surface that cannot list marks must show "no marks
/// found", and the loud disclosure of a broken page tree is owed by the
/// document-open path, not by a census.
#[must_use]
pub fn redaction_marks<G: ObjectGraph + ?Sized>(graph: &G) -> Vec<RedactionMark> {
    let mut found = Vec::new();
    let Ok(pages) = page_tree::pages_in(graph) else {
        return found;
    };
    for (page_index, page) in pages.iter().enumerate() {
        let Some(dict) = graph.value(page.id).and_then(Object::as_dict) else {
            continue;
        };
        let Some(annots) = dict
            .get(b"Annots")
            .map(|o| graph.resolve(o))
            .and_then(Object::as_array)
        else {
            continue;
        };
        for entry in annots {
            let Some(annot_id) = entry.as_reference() else {
                continue;
            };
            let Some(ad) = graph.value(annot_id).and_then(Object::as_dict) else {
                continue;
            };
            if ad
                .get(b"Subtype")
                .and_then(Object::as_name)
                .is_none_or(|n| n.as_bytes() != b"Redact")
            {
                continue;
            }
            found.push(RedactionMark {
                page_index,
                annot_id,
                rect: annot_rect(graph, ad),
            });
        }
    }
    found
}

/// Read an annotation's `/Rect` into a normalised `[llx, lly, urx, ury]`.
///
/// Table 166 allows the two corners in either order, so the min/max
/// normalisation is required, not defensive: an unnormalised rect would
/// render as a negative-size region in a review row.
fn annot_rect<G: ObjectGraph + ?Sized>(graph: &G, annot: &Dict) -> Option<[f64; 4]> {
    let arr = annot
        .get(b"Rect")
        .map(|o| graph.resolve(o))
        .and_then(Object::as_array)?;
    if arr.len() < 4 {
        return None;
    }
    let mut v = [0.0_f64; 4];
    for (slot, obj) in v.iter_mut().zip(arr.iter()) {
        *slot = graph.resolve(obj).as_number()?;
    }
    Some([
        v[0].min(v[2]),
        v[1].min(v[3]),
        v[0].max(v[2]),
        v[1].max(v[3]),
    ])
}

/// Count the `/Redact` marks currently present in a document — the census
/// the GUI status bar uses to disclose UNAPPLIED redactions (computed from
/// the graph itself, never a session counter, so it survives save/reload
/// and cannot lie about a marked-but-not-applied file).
///
/// # Why this is generic over [`ObjectGraph`] (Pass 17.1)
///
/// It used to take `&Document`, and the GUI's only caller passed
/// `session.document()` — the BASE revision. The consequence was the
/// precise failure this disclosure exists to prevent, wearing the
/// disclosure's own face: **a `/Redact` mark the operator placed during
/// this session was not counted**, so the very banner whose job is to say
/// "this document has marks you have not applied yet" stayed silent about
/// the marks most likely to be forgotten — the ones just made. Decision
/// 018 §8 names this the confirmed bug of the Pass 17.1 audit.
///
/// Generic rather than `&DocumentView` because the census reads **only**
/// dictionaries (`/Annots` → `/Subtype`); it never touches stream bytes,
/// so it needs an object graph and nothing else. That keeps every existing
/// caller (`pdfcer`, the redaction tests) compiling unchanged — a
/// `&Document` *is* an `ObjectGraph` — while the GUI can now pass
/// `&session.graph()` and get the truth.
///
/// A mark applied and then undone is correctly *not* counted: the session
/// overlay holds the base value again, and this walks values, never a
/// history.
///
/// Delegates to [`redaction_marks`] (Pass 8.1). The count and the list a
/// review surface shows are therefore the SAME walk, not two walks that
/// agree by inspection — a status bar reading "3 unapplied marks" beside a
/// panel listing 2 rows would leave an operator with no way to tell which
/// number to believe on the one feature where believing the wrong one leaks
/// content.
#[must_use]
pub fn count_redaction_marks<G: ObjectGraph + ?Sized>(graph: &G) -> usize {
    redaction_marks(graph).len()
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
    use crate::edit::EditSession;
    use crate::text_extract::{self, ExtractOptions};
    use crate::writer::SaveOptions;

    /// Assemble a classic single-page PDF from body strings (objects
    /// `1..=n`), computing a correct xref table. Object 1 must be the
    /// catalog.
    fn assemble(bodies: &[&str], extra_trailer: &str) -> Vec<u8> {
        let mut buf = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
        let mut offsets = Vec::new();
        for (i, body) in bodies.iter().enumerate() {
            offsets.push(buf.len());
            buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
        }
        let xref_at = buf.len();
        let n = bodies.len() + 1;
        buf.extend_from_slice(format!("xref\n0 {n}\n0000000000 65535 f \n").as_bytes());
        for off in &offsets {
            buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
        }
        buf.extend_from_slice(
            format!(
                "trailer\n<< /Size {n} /Root 1 0 R {extra_trailer} >>\nstartxref\n{xref_at}\n%%EOF\n"
            )
            .as_bytes(),
        );
        buf
    }

    /// A page whose content shows "SECRET PUBLIC" in one `Tj`, in
    /// standard-14 Helvetica (accurate AFM widths, so advance preservation
    /// is exact). `SECRET ` is what we redact; ` PUBLIC` must survive in
    /// place.
    fn redactable_pdf() -> Vec<u8> {
        let content = b"BT /F1 24 Tf 20 100 Td (SECRET PUBLIC) Tj ET";
        let stream = format!(
            "<< /Length {} >>\nstream\n{}\nendstream",
            content.len(),
            std::str::from_utf8(content).unwrap()
        );
        assemble(
            &[
                "<< /Type /Catalog /Pages 2 0 R >>",
                "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 200] \
                 /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>",
                &stream,
                "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
            ],
            "",
        )
    }

    /// Decode every content stream of a document into one blob (for the
    /// absence assertion over decoded bytes).
    fn all_decoded_content(doc: &Document) -> Vec<u8> {
        let mut out = Vec::new();
        let pages = page_tree::pages(doc).unwrap();
        for page in &pages {
            if let Ok(cs) = ContentStream::from_page(&doc.view(), page) {
                out.extend_from_slice(&cs.buf);
            }
        }
        out
    }

    fn contains(hay: &[u8], needle: &[u8]) -> bool {
        hay.windows(needle.len()).any(|w| w == needle)
    }

    /// Mark "SECRET" by search, save, reload — the state apply operates on.
    fn mark_and_save(input: &[u8]) -> Vec<u8> {
        let doc = Document::from_bytes(input.to_vec()).unwrap();
        let mut session = EditSession::new(doc);
        let ids = session.mark_redactions_by_search("SECRET", false).unwrap();
        assert!(!ids.is_empty(), "search should have found SECRET");
        let (bytes, _) = session
            .to_incremental_bytes(&SaveOptions::identity())
            .unwrap();
        bytes
    }

    /// Pass 17.1 regression gate: a mark added THIS SESSION is counted.
    ///
    /// This is the confirmed bug of decision 018 §8's `session.document()`
    /// audit, and it is the nastiest shape a bug can take — the disclosure
    /// whose entire job is to say *"this document has redaction marks you
    /// have not applied yet"* was blind to exactly the marks most likely to
    /// be forgotten: the ones just made. `count_redaction_marks` took a
    /// `&Document`, and the GUI's only caller handed it
    /// `session.document()`, the base revision, which by construction cannot
    /// carry an unsaved mark.
    ///
    /// The test pins all three states, because only the contrast makes the
    /// fix meaningful:
    ///
    /// 1. the BASE still counts 0 (the file on disk really has no mark);
    /// 2. the SESSION graph counts 1 (what the operator must be told);
    /// 3. after undo the session counts 0 again — proving the census walks
    ///    object VALUES rather than a counter that edits increment, which is
    ///    the property that lets it survive save/reload and refuse to lie.
    #[test]
    fn a_mark_added_this_session_is_counted_over_the_session_graph() {
        use crate::annot_author::{Quad, RedactSpec};
        use crate::vartext::Quadding;

        let doc = Document::from_bytes(redactable_pdf()).unwrap();
        let mut session = EditSession::new(doc);
        assert_eq!(
            count_redaction_marks(session.document()),
            0,
            "the fixture starts with no marks"
        );

        session
            .add_redaction(
                0,
                &RedactSpec {
                    quads: vec![Quad::from_rect(Rect::from_corners(
                        20.0, 90.0, 120.0, 130.0,
                    ))],
                    fill: None,
                    overlay_text: None,
                    quadding: Quadding::Left,
                },
            )
            .unwrap();

        assert_eq!(
            count_redaction_marks(&session.graph()),
            1,
            "a /Redact mark added this session MUST be disclosed — this is the Pass 17.1 bug"
        );
        assert_eq!(
            count_redaction_marks(session.document()),
            0,
            "the base revision is unchanged until the document is saved"
        );

        session.undo().expect("the mark is one undoable command");
        assert_eq!(
            count_redaction_marks(&session.graph()),
            0,
            "an undone mark is not a pending mark — the census walks values, not a counter"
        );
    }

    // -- the mark census and per-mark removal (Pass 8.1) -----------------

    /// [`redaction_marks`] must locate a mark on the right page and report a
    /// usable rect, because the GUI's review list addresses marks by object
    /// id and navigates by the page index this reports. A wrong page index
    /// would send an operator to review a mark that is somewhere else.
    #[test]
    fn the_census_locates_each_mark_with_its_page_and_rect() {
        let marked = mark_and_save(&redactable_pdf());
        let doc = Document::from_bytes(marked).unwrap();
        let marks = redaction_marks(&doc);
        assert_eq!(marks.len(), 1);
        let mark = marks[0];
        assert_eq!(mark.page_index, 0);
        let [llx, lly, urx, ury] = mark.rect.expect("a search mark carries a /Rect");
        assert!(
            urx > llx && ury > lly,
            "the rect must be normalised to a positive-size box: {:?}",
            mark.rect
        );
        assert_eq!(marks.len(), count_redaction_marks(&doc));
    }

    /// A mark can be taken off BEFORE apply, as one undoable command — the
    /// per-mark reject that makes a bulk search-and-mark genuinely
    /// reviewable (rule 4). Three properties, all of which matter:
    ///
    /// 1. the mark disappears from the census;
    /// 2. undo puts it back (so an accidental reject costs nothing);
    /// 3. the page's CONTENT is unchanged in both directions — removing a
    ///    mark is the reverse of marking, never a reverse of redacting, and
    ///    a build that quietly touched content here would be doing something
    ///    nobody asked for on the most sensitive path in the app.
    #[test]
    fn a_mark_can_be_rejected_before_apply_and_the_content_is_untouched() {
        let marked = mark_and_save(&redactable_pdf());
        let doc = Document::from_bytes(marked).unwrap();
        let content_before = all_decoded_content(&doc);
        let mut session = EditSession::new(doc);

        let id = redaction_marks(&session.graph())[0].annot_id;
        session.delete_redaction_mark(id).expect("the mark exists");
        assert_eq!(
            count_redaction_marks(&session.graph()),
            0,
            "a rejected mark must leave the census"
        );

        // The page content is byte-identical: no redaction happened.
        let after = {
            let view = session.view();
            let pages = page_tree::pages_in(view.graph()).unwrap();
            let mut out = Vec::new();
            for page in &pages {
                if let Ok(cs) = ContentStream::from_page(&view, page) {
                    out.extend_from_slice(&cs.buf);
                }
            }
            out
        };
        assert_eq!(
            content_before, after,
            "removing a MARK must not change page content — nothing was ever applied"
        );
        assert!(
            contains(&after, b"SECRET"),
            "the covered text was always there and must still be"
        );

        session.undo().expect("one undoable command");
        assert_eq!(
            count_redaction_marks(&session.graph()),
            1,
            "undo must put a rejected mark back"
        );
    }

    /// A stale or wrong object id is refused **by name**, never treated as
    /// "delete whatever that is". The review list addresses marks by id, and
    /// an id can go stale (a mark already removed, an id from an undone
    /// command) — silently deleting some unrelated annotation that inherited
    /// the number is the failure this refusal exists to prevent.
    #[test]
    fn deleting_a_non_mark_is_refused_by_name() {
        let doc = Document::from_bytes(redactable_pdf()).unwrap();
        let mut session = EditSession::new(doc);
        // Object 3 is the page dictionary — a real object, not a mark.
        let err = session
            .delete_redaction_mark(ObjId::new(3, 0))
            .expect_err("a page is not a redaction mark");
        assert!(
            matches!(err, crate::edit::EditError::NotARedactionMark { .. }),
            "expected a named refusal, got {err:?}"
        );
        // And nothing was mutated by the attempt.
        assert!(!session.is_modified());
    }

    // -- THE HEADLINE GATE: absence proof --------------------------------

    #[test]
    fn apply_removes_redacted_text_from_the_whole_saved_file() {
        let marked = mark_and_save(&redactable_pdf());
        let doc = Document::from_bytes(marked).unwrap();
        assert_eq!(count_redaction_marks(&doc), 1);

        let (out, report) = apply_redactions(&doc, &SaveOptions::identity()).unwrap();

        // ABSENCE PROOF: "SECRET" appears nowhere in the raw saved bytes...
        assert!(
            !contains(&out, b"SECRET"),
            "redacted text 'SECRET' survived in the raw saved bytes"
        );
        // ...and nowhere in any decoded content stream.
        let back = Document::from_bytes(out.clone()).unwrap();
        let decoded = all_decoded_content(&back);
        assert!(
            !contains(&decoded, b"SECRET"),
            "redacted text 'SECRET' survived in a decoded content stream"
        );
        // The mark itself is gone, and the surviving text remains.
        assert_eq!(
            count_redaction_marks(&back),
            0,
            "the /Redact mark must be removed"
        );
        assert!(
            contains(&decoded, b"PUBLIC"),
            "un-redacted text 'PUBLIC' must survive"
        );
        assert!(report.glyphs_removed >= 6, "SECRET is 6 glyphs");
        assert!(report.marks_applied >= 1);
    }

    #[test]
    fn apply_forces_full_rewrite_dropping_prior_revisions() {
        // `marked` is an incremental save (base revision holds the
        // un-redacted content). Apply full-rewrites from it: the output
        // must carry NO /Prev (single revision) and NOT contain the text.
        let marked = mark_and_save(&redactable_pdf());
        let doc = Document::from_bytes(marked).unwrap();
        let (out, _) = apply_redactions(&doc, &SaveOptions::identity()).unwrap();
        let back = Document::from_bytes(out.clone()).unwrap();
        assert!(
            back.trailer().get(b"Prev").is_none(),
            "a redaction full rewrite must have no /Prev (no prior revision)"
        );
        assert!(!contains(&out, b"SECRET"));
    }

    // -- advance preservation --------------------------------------------

    #[test]
    fn surviving_text_stays_positioned_after_a_mid_string_redaction() {
        // Extract the original 'P' of PUBLIC, then the redacted 'P', and
        // assert it did not shift (the advance-preserving TJ compensates
        // for the removed SECRET run).
        let original = Document::from_bytes(redactable_pdf()).unwrap();
        let orig_x = first_glyph_x(&original, 'P').expect("original P");

        let marked = mark_and_save(&redactable_pdf());
        let doc = Document::from_bytes(marked).unwrap();
        let (out, _) = apply_redactions(&doc, &SaveOptions::identity()).unwrap();
        let back = Document::from_bytes(out).unwrap();
        let new_x = first_glyph_x(&back, 'P').expect("redacted P");

        assert!(
            (orig_x - new_x).abs() < 1.0,
            "PUBLIC's 'P' shifted from {orig_x} to {new_x} after redaction (advance not preserved)"
        );
    }

    /// The device-space x of the first glyph whose character is `ch`.
    fn first_glyph_x(doc: &Document, ch: char) -> Option<f32> {
        let text = text_extract::extract_document(doc, &ExtractOptions::default()).ok()?;
        for page in &text.pages {
            for run in &page.runs {
                for g in &run.glyphs {
                    let start = g.text_start as usize;
                    let seg = run.text.get(start..start + g.text_len as usize)?;
                    if seg.starts_with(ch) {
                        return Some(g.x);
                    }
                }
            }
        }
        None
    }

    // -- image destroy (§12.5.6.23) ----------------------------------------

    /// Assemble a classic PDF from BINARY bodies (an image stream cannot be
    /// a `&str`). Same shape as [`assemble`] otherwise.
    fn assemble_bytes(bodies: &[Vec<u8>]) -> Vec<u8> {
        let mut buf = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
        let mut offsets = Vec::new();
        for (i, body) in bodies.iter().enumerate() {
            offsets.push(buf.len());
            buf.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
            buf.extend_from_slice(body);
            buf.extend_from_slice(b"\nendobj\n");
        }
        let xref_at = buf.len();
        let n = bodies.len() + 1;
        buf.extend_from_slice(format!("xref\n0 {n}\n0000000000 65535 f \n").as_bytes());
        for off in &offsets {
            buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
        }
        buf.extend_from_slice(
            format!("trailer\n<< /Size {n} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n")
                .as_bytes(),
        );
        buf
    }

    fn stream_body(dict_entries: &str, data: &[u8]) -> Vec<u8> {
        let mut b = format!("<< {dict_entries} /Length {} >>\nstream\n", data.len()).into_bytes();
        b.extend_from_slice(data);
        b.extend_from_slice(b"\nendstream");
        b
    }

    /// A one-page document: `content` painted with `/Im1` bound to object 5
    /// (`image`) plus, optionally, a Helvetica `/F1` (object 6) and object 7.
    fn image_pdf(content: &[u8], image: Vec<u8>, extra: Vec<Vec<u8>>) -> Vec<u8> {
        let mut bodies = vec![
            b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] \
               /Resources << /XObject << /Im1 5 0 R >> /Font << /F1 6 0 R >> >> \
               /Contents 4 0 R >>"
                .to_vec(),
            stream_body("", content),
            image,
            b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
        ];
        bodies.extend(extra);
        assemble_bytes(&bodies)
    }

    /// A 4×4 8-bit DeviceGray image, every sample 0x20 (dark, so a clear to paper 0xFF shows), no filter.
    fn gray_4x4() -> Vec<u8> {
        stream_body(
            "/Type /XObject /Subtype /Image /Width 4 /Height 4 /BitsPerComponent 8 \
             /ColorSpace /DeviceGray",
            &[0x20; 16],
        )
    }

    /// Mark the given rectangles on page 0 and save incrementally.
    fn mark_rects(pdf: Vec<u8>, rects: &[[f64; 4]]) -> Vec<u8> {
        let doc = Document::from_bytes(pdf).unwrap();
        let mut session = EditSession::new(doc);
        for r in rects {
            let spec = crate::annot_author::RedactSpec {
                quads: vec![crate::annot_author::Quad::from_rect(Rect::from_corners(
                    r[0], r[1], r[2], r[3],
                ))],
                fill: None,
                overlay_text: None,
                quadding: crate::vartext::Quadding::Left,
            };
            session.add_redaction(0, &spec).unwrap();
        }
        let (marked, _) = session
            .to_incremental_bytes(&SaveOptions::identity())
            .unwrap();
        marked
    }

    /// The page's `/XObject` resources of the saved output, resolved, plus
    /// the page content bytes.
    fn output_page(bytes: &[u8]) -> (Document, Dict, Vec<u8>) {
        let doc = Document::from_bytes(bytes.to_vec()).unwrap();
        let pages = page_tree::pages(&doc).unwrap();
        let page = &pages[0];
        let xobjects = page
            .resources
            .get(b"XObject")
            .map(|o| doc.resolve(o))
            .and_then(Object::as_dict)
            .cloned()
            .unwrap_or_default();
        let content = ContentStream::from_page(&doc.view(), page).unwrap().buf;
        (doc, xobjects, content)
    }

    /// Decode the image object `id` of `doc` to its raw samples and size.
    fn decode_object(doc: &Document, id: ObjId) -> (Vec<u8>, u32, u32, Dict) {
        let Some(Object::Stream(s)) = doc.get(id).map(|io| &io.value) else {
            panic!("{id:?} is not a stream");
        };
        let view = doc.view();
        let raw = view.slice(s.data_span).unwrap();
        let coded = crate::image_codec::decode_image_view(&view, &s.dict, raw, false).unwrap();
        let w = s.dict.get(b"Width").and_then(Object::as_int).unwrap() as u32;
        let h = s.dict.get(b"Height").and_then(Object::as_int).unwrap() as u32;
        (coded.samples, w, h, s.dict.clone())
    }

    fn carrier_action(report: &RedactionReport, name: &str) -> CarrierAction {
        report
            .carriers
            .iter()
            .find(|c| c.carrier == name)
            .map(|c| c.action)
            .unwrap()
    }

    #[test]
    fn a_partially_covered_image_has_its_covered_cells_destroyed() {
        // Placement (50,100)-(150,150); region (60,110)-(120,140) covers
        // u 0.1..0.7 → cols 0..3 and v 0.2..0.8 → rows 0..4 of a 4×4 grid.
        let pdf = image_pdf(b"q 100 0 0 50 50 100 cm /Im1 Do Q", gray_4x4(), vec![]);
        let marked = mark_rects(pdf, &[[60.0, 110.0, 120.0, 140.0]]);
        let doc = Document::from_bytes(marked).unwrap();
        let (out, report) = apply_redactions(&doc, &SaveOptions::identity()).unwrap();

        assert_eq!(report.images_cleared, 1);
        assert_eq!(report.images_removed, 0);
        assert_eq!(report.images_cloned_shared, 0);
        assert_eq!(report.marks_retained, 0);
        assert_eq!(carrier_action(&report, "images"), CarrierAction::Scrubbed);
        assert!(!report.has_disclosed_residuals());

        let (out_doc, xobjects, content) = output_page(&out);
        // The Do now names the clone; the old name is no longer painted.
        assert!(
            !contains(&content, b"/Im1 Do"),
            "{}",
            String::from_utf8_lossy(&content)
        );
        assert!(contains(&content, b"/pdfceRd5_1 Do"));
        let clone_id = xobjects
            .get(b"pdfceRd5_1")
            .and_then(Object::as_reference)
            .unwrap();
        let (samples, w, h, dict) = decode_object(&out_doc, clone_id);
        assert_eq!((w, h), (4, 4));
        assert_eq!(
            dict.get(b"Filter")
                .and_then(Object::as_name)
                .map(|n| n.as_bytes()),
            Some(&b"FlateDecode"[..])
        );
        for row in 0..4 {
            assert_eq!(
                &samples[row * 4..row * 4 + 4],
                &[0xFF, 0xFF, 0xFF, 0x20],
                "row {row}"
            );
        }
        // The original (its only use was this placement) is a 1×1 blank.
        let (orig, w, h, _) = decode_object(&out_doc, ObjId::new(5, 0));
        assert_eq!((w, h), (1, 1));
        assert_eq!(orig, vec![0xFF]);
        // And no byte of the original's 0xFF grid survives in the file: the
        // absence proof, over the OUTPUT bytes.
        assert!(!contains(&out, &[0x20; 16]));
    }

    #[test]
    fn a_wholly_covered_image_is_removed_from_the_page_outright() {
        let pdf = image_pdf(b"q 100 0 0 50 50 100 cm /Im1 Do Q", gray_4x4(), vec![]);
        let marked = mark_rects(pdf, &[[0.0, 0.0, 300.0, 200.0]]);
        let doc = Document::from_bytes(marked).unwrap();
        let (out, report) = apply_redactions(&doc, &SaveOptions::identity()).unwrap();

        assert_eq!(report.images_removed, 1);
        assert_eq!(report.images_cleared, 0);
        assert!(
            report.notes.iter().any(|n| n.contains("REMOVED ENTIRELY")),
            "{:?}",
            report.notes
        );

        let (out_doc, _xobjects, content) = output_page(&out);
        assert!(
            !contains(&content, b"Do"),
            "{}",
            String::from_utf8_lossy(&content)
        );
        let (orig, w, h, _) = decode_object(&out_doc, ObjId::new(5, 0));
        assert_eq!((w, h), (1, 1));
        assert_eq!(orig, vec![0xFF]);
        assert!(!contains(&out, &[0x20; 16]));
    }

    #[test]
    fn a_shared_image_is_cloned_and_the_original_survives_for_its_other_placement() {
        // Two placements of /Im1; only the first is marked.
        let content = b"q 100 0 0 50 50 100 cm /Im1 Do Q q 100 0 0 50 150 20 cm /Im1 Do Q";
        let pdf = image_pdf(content, gray_4x4(), vec![]);
        let marked = mark_rects(pdf, &[[60.0, 110.0, 120.0, 140.0]]);
        let doc = Document::from_bytes(marked).unwrap();
        let (out, report) = apply_redactions(&doc, &SaveOptions::identity()).unwrap();

        assert_eq!(report.images_cleared, 1);
        assert_eq!(report.images_cloned_shared, 1);
        assert!(
            report.notes.iter().any(|n| n.contains("SHARED")),
            "{:?}",
            report.notes
        );

        let (out_doc, _x, content) = output_page(&out);
        assert!(contains(&content, b"/pdfceRd5_1 Do"));
        assert!(
            contains(&content, b"/Im1 Do"),
            "the unmarked placement keeps the original"
        );
        let (orig, w, _h, _) = decode_object(&out_doc, ObjId::new(5, 0));
        assert_eq!(w, 4);
        assert_eq!(orig, vec![0x20; 16], "the original is untouched");
    }

    /// An image whose samples pdfcer cannot decode: JBIG2 with garbage.
    fn undecodable_image() -> Vec<u8> {
        stream_body(
            "/Type /XObject /Subtype /Image /Width 4 /Height 4 /BitsPerComponent 1 \
             /ColorSpace /DeviceGray /Filter /JBIG2Decode",
            b"not a jbig2 stream at all",
        )
    }

    #[test]
    fn an_undestroyable_image_retains_its_mark_and_the_other_marks_apply() {
        let content =
            b"q 100 0 0 50 50 100 cm /Im1 Do Q BT /F1 24 Tf 20 20 Td (SECRET PUBLIC) Tj ET";
        let pdf = image_pdf(content, undecodable_image(), vec![]);
        // One mark over the image, one over "SECRET" (by search).
        let marked = mark_rects(pdf, &[[60.0, 110.0, 120.0, 140.0]]);
        let marked = mark_and_save(&marked);
        let doc = Document::from_bytes(marked).unwrap();
        let (out, report) = apply_redactions(&doc, &SaveOptions::identity()).unwrap();

        assert_eq!(report.marks_retained, 1);
        assert_eq!(report.marks_applied, 1);
        assert!(report.glyphs_removed >= 6, "{report:?}");
        assert_eq!(
            carrier_action(&report, "images"),
            CarrierAction::DisclosedNotScrubbed
        );
        assert!(report.has_disclosed_residuals());
        assert!(
            report
                .notes
                .iter()
                .any(|n| n.contains("RETAINED") && n.contains("JBIG2")),
            "{:?}",
            report.notes
        );

        let out_doc = Document::from_bytes(out.clone()).unwrap();
        // The retained mark is still IN the output as an unapplied /Redact.
        assert_eq!(count_redaction_marks(&out_doc.view()), 1);
        // The image is still painted (nothing was masked over it).
        let (_d, _x, content) = output_page(&out);
        assert!(contains(&content, b"/Im1 Do"));
        assert!(!contains(&all_decoded_content(&out_doc), b"SECRET"));
    }

    #[test]
    fn every_mark_undestroyable_is_refused_by_name() {
        let pdf = image_pdf(
            b"q 100 0 0 50 50 100 cm /Im1 Do Q",
            undecodable_image(),
            vec![],
        );
        let marked = mark_rects(pdf, &[[60.0, 110.0, 120.0, 140.0]]);
        let doc = Document::from_bytes(marked).unwrap();
        let err = apply_redactions(&doc, &SaveOptions::identity()).unwrap_err();
        assert!(
            matches!(&err, RedactError::ImageUndestroyable { page: 1, reason } if reason.contains("decoded")),
            "expected a named refusal, got {err:?}"
        );
    }

    #[test]
    fn a_touch_of_the_bounding_box_destroys_nothing() {
        // Region abuts the placement's right edge at x = 150 exactly.
        let pdf = image_pdf(b"q 100 0 0 50 50 100 cm /Im1 Do Q", gray_4x4(), vec![]);
        let marked = mark_rects(pdf, &[[150.0, 100.0, 200.0, 150.0]]);
        let doc = Document::from_bytes(marked).unwrap();
        let (out, report) = apply_redactions(&doc, &SaveOptions::identity()).unwrap();
        assert_eq!(report.images_cleared, 0);
        assert_eq!(report.images_removed, 0);
        let (out_doc, _x, content) = output_page(&out);
        assert!(contains(&content, b"/Im1 Do"));
        let (orig, _, _, _) = decode_object(&out_doc, ObjId::new(5, 0));
        assert_eq!(orig, vec![0x20; 16]);
    }

    #[test]
    fn an_inline_image_is_re_encoded_with_its_covered_cells_destroyed() {
        // 2×2 gray inline image, all 0xFF, placed (50,100)-(150,150).
        let mut content = b"q 100 0 0 50 50 100 cm BI /W 2 /H 2 /CS /G /BPC 8 ID ".to_vec();
        content.extend_from_slice(&[0x20; 4]);
        content.extend_from_slice(b" EI Q");
        let pdf = image_pdf(&content, gray_4x4(), vec![]);
        // Left half of the placement → column 0 of both rows.
        let marked = mark_rects(pdf, &[[50.0, 100.0, 100.0, 150.0]]);
        let doc = Document::from_bytes(marked).unwrap();
        let (out, report) = apply_redactions(&doc, &SaveOptions::identity()).unwrap();
        assert_eq!(report.images_cleared, 1);

        let (out_doc, _x, content) = output_page(&out);
        let cs = ContentStream::parse(content).unwrap();
        let inline = cs
            .operations()
            .find_map(|op| match &op.operator.kind {
                ContentTokenKind::InlineImage { params, data } => Some((params.clone(), *data)),
                _ => None,
            })
            .expect("the inline image survives, re-encoded");
        let raw = inline.1.slice(&cs.buf).unwrap();
        assert_eq!(
            inline
                .0
                .get(b"Filter")
                .and_then(Object::as_name)
                .map(|n| n.as_bytes()),
            Some(&b"FlateDecode"[..])
        );
        let coded =
            crate::image_codec::decode_image_view(&out_doc.view(), &inline.0, raw, true).unwrap();
        assert_eq!(coded.samples, vec![0xFF, 0x20, 0xFF, 0x20]);
    }

    #[test]
    fn a_wholly_covered_inline_image_is_removed() {
        let mut content = b"q 100 0 0 50 50 100 cm BI /W 2 /H 2 /CS /G /BPC 8 ID ".to_vec();
        content.extend_from_slice(&[0xFF; 4]);
        content.extend_from_slice(b" EI Q");
        let pdf = image_pdf(&content, gray_4x4(), vec![]);
        let marked = mark_rects(pdf, &[[0.0, 0.0, 300.0, 200.0]]);
        let doc = Document::from_bytes(marked).unwrap();
        let (out, report) = apply_redactions(&doc, &SaveOptions::identity()).unwrap();
        assert_eq!(report.images_removed, 1);
        let (_d, _x, content) = output_page(&out);
        assert!(
            !contains(&content, b"BI"),
            "{}",
            String::from_utf8_lossy(&content)
        );
        assert!(!contains(&content, b"EI"));
    }

    #[test]
    fn a_jpeg_is_decoded_cleared_and_re_encoded_losslessly() {
        let jpeg = crate::image_codec::fixtures::GRAY_2X2;
        let image = stream_body(
            "/Type /XObject /Subtype /Image /Width 2 /Height 2 /BitsPerComponent 8 \
             /ColorSpace /DeviceGray /Filter /DCTDecode",
            jpeg,
        );
        // What the codec yields for the untouched right column.
        let probe = Document::from_bytes(image_pdf(b"", image.clone(), vec![])).unwrap();
        let (before, _, _, _) = decode_object(&probe, ObjId::new(5, 0));

        let pdf = image_pdf(b"q 100 0 0 50 50 100 cm /Im1 Do Q", image, vec![]);
        let marked = mark_rects(pdf, &[[50.0, 100.0, 100.0, 150.0]]);
        let doc = Document::from_bytes(marked).unwrap();
        let (out, report) = apply_redactions(&doc, &SaveOptions::identity()).unwrap();
        assert_eq!(report.images_cleared, 1);

        let (out_doc, xobjects, _c) = output_page(&out);
        let clone_id = xobjects
            .get(b"pdfceRd5_1")
            .and_then(Object::as_reference)
            .unwrap();
        let (samples, w, h, dict) = decode_object(&out_doc, clone_id);
        assert_eq!((w, h), (2, 2));
        assert_eq!(
            dict.get(b"Filter")
                .and_then(Object::as_name)
                .map(|n| n.as_bytes()),
            Some(&b"FlateDecode"[..])
        );
        assert_eq!(samples, vec![0xFF, before[1], 0xFF, before[3]]);
        // The JPEG codestream itself is gone from the file.
        assert!(!contains(&out, jpeg));
    }

    #[test]
    fn a_soft_mask_is_destroyed_with_its_image() {
        // /Im1 (object 5) carries /SMask 7 0 R, a 4×4 gray alpha of 0x80.
        let image = stream_body(
            "/Type /XObject /Subtype /Image /Width 4 /Height 4 /BitsPerComponent 8 \
             /ColorSpace /DeviceGray /SMask 7 0 R",
            &[0x20; 16],
        );
        let smask = stream_body(
            "/Type /XObject /Subtype /Image /Width 4 /Height 4 /BitsPerComponent 8 \
             /ColorSpace /DeviceGray",
            &[0x80; 16],
        );
        let pdf = image_pdf(b"q 100 0 0 50 50 100 cm /Im1 Do Q", image, vec![smask]);
        let marked = mark_rects(pdf, &[[60.0, 110.0, 120.0, 140.0]]);
        let doc = Document::from_bytes(marked).unwrap();
        let (out, _report) = apply_redactions(&doc, &SaveOptions::identity()).unwrap();

        let (out_doc, xobjects, _c) = output_page(&out);
        let clone_id = xobjects
            .get(b"pdfceRd5_1")
            .and_then(Object::as_reference)
            .unwrap();
        let (_s, _w, _h, dict) = decode_object(&out_doc, clone_id);
        let sm_id = dict.get(b"SMask").and_then(Object::as_reference).unwrap();
        assert_ne!(sm_id, ObjId::new(7, 0), "the clone has its own mask");
        let (alpha, w, h, _) = decode_object(&out_doc, sm_id);
        assert_eq!((w, h), (4, 4));
        for row in 0..4 {
            assert_eq!(
                &alpha[row * 4..row * 4 + 4],
                &[0x00, 0x00, 0x00, 0x80],
                "row {row}"
            );
        }
        // The original mask is tombstoned with the original image.
        let (om, w, h, _) = decode_object(&out_doc, ObjId::new(7, 0));
        assert_eq!((w, h), (1, 1));
        assert_eq!(om, vec![0]);
        assert!(!contains(&out, &[0x80; 16]));
    }

    // -- vector paths: cut at the region boundary (Pass 246.0) ---------------

    #[test]
    fn a_vector_path_crossing_a_region_is_cut_and_one_inside_is_deleted() {
        // A line through the region, a filled square wholly inside it, a
        // rectangle far away, and a clip-only path (`W n`) inside it that
        // must be left alone.
        let content =
            b"0 100 m 300 150 l S 70 120 10 10 re f 200 10 20 20 re f 65 115 20 20 re W n";
        let pdf = image_pdf(content, gray_4x4(), vec![]);
        let marked = mark_rects(pdf, &[[60.0, 110.0, 120.0, 140.0]]);
        let doc = Document::from_bytes(marked).unwrap();
        let (out, report) = apply_redactions(&doc, &SaveOptions::identity()).unwrap();
        assert_eq!(report.vector_paths_cut, 2, "{report:?}");
        assert_eq!(report.vector_paths_dropped, 1);
        assert_eq!(report.vector_paths_intersecting, 0);
        assert_eq!(
            carrier_action(&report, "vector_paths"),
            CarrierAction::Scrubbed
        );
        assert!(!report.has_disclosed_residuals());
        assert!(
            report.notes.iter().any(|n| n.contains("were CUT")),
            "{:?}",
            report.notes
        );

        let (_d, _x, content) = output_page(&out);
        let text = String::from_utf8_lossy(&content);
        assert!(
            !text.contains("70 120 10 10 re"),
            "the inside square is gone: {text}"
        );
        assert!(
            text.contains("200 10 20 20 re"),
            "the far rectangle is untouched: {text}"
        );
        assert!(
            text.contains("65 115 20 20 re W n"),
            "the clip is untouched: {text}"
        );
        assert!(
            !text.contains("0 100 m 300 150 l S"),
            "the line was rewritten: {text}"
        );
        // Two stroke pieces: the line enters the (expanded) region and leaves it.
        assert_eq!(text.matches(" l\n").count(), 2, "{text}");
    }

    #[test]
    fn a_shading_whose_clip_meets_the_region_is_disclosed_and_one_clipped_away_is_not() {
        // An axial shading resource; `sh` under a clip over the region, and
        // `sh` under a clip far from it.
        let content = b"q 50 100 100 50 re W n /Sh1 sh Q q 200 10 20 20 re W n /Sh1 sh Q";
        let shading = b"<< /ShadingType 2 /ColorSpace /DeviceGray /Coords [0 0 300 0] \
                        /Function << /FunctionType 2 /Domain [0 1] /C0 [0] /C1 [1] /N 1 >> >>"
            .to_vec();
        let mut bodies = vec![
            b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] \
               /Resources << /Shading << /Sh1 5 0 R >> >> /Contents 4 0 R >>"
                .to_vec(),
            stream_body("", content),
            shading,
        ];
        bodies.push(b"<< >>".to_vec());
        let pdf = assemble_bytes(&bodies);
        let marked = mark_rects(pdf, &[[60.0, 110.0, 120.0, 140.0]]);
        let doc = Document::from_bytes(marked).unwrap();
        let (_out, report) = apply_redactions(&doc, &SaveOptions::identity()).unwrap();
        assert_eq!(report.shadings_intersecting, 1, "{report:?}");
        assert_eq!(
            carrier_action(&report, "shadings"),
            CarrierAction::DisclosedNotScrubbed
        );
        assert!(report.has_disclosed_residuals());
    }

    #[test]
    fn a_vector_path_outside_every_region_is_left_byte_identical() {
        let content = b"200 10 m 250 30 l S";
        let pdf = image_pdf(content, gray_4x4(), vec![]);
        let marked = mark_rects(pdf, &[[60.0, 110.0, 120.0, 140.0]]);
        let doc = Document::from_bytes(marked).unwrap();
        let (out, report) = apply_redactions(&doc, &SaveOptions::identity()).unwrap();
        assert_eq!(report.vector_paths_cut, 0);
        assert_eq!(report.vector_paths_intersecting, 0);
        assert_eq!(
            carrier_action(&report, "vector_paths"),
            CarrierAction::Absent
        );
        let (_d, _x, out_content) = output_page(&out);
        assert!(contains(&out_content, content));
    }

    #[test]
    fn a_malformed_path_object_is_a_disclosed_residual_not_a_cut() {
        // A `cm` between construction and paint: §8.2 forbids it, and the
        // bytes cannot be replaced as a unit.
        let content = b"0 100 m 300 150 l 1 0 0 1 0 0 cm S";
        let pdf = image_pdf(content, gray_4x4(), vec![]);
        let marked = mark_rects(pdf, &[[60.0, 110.0, 120.0, 140.0]]);
        let doc = Document::from_bytes(marked).unwrap();
        let (_out, report) = apply_redactions(&doc, &SaveOptions::identity()).unwrap();
        assert_eq!(report.vector_paths_cut, 0);
        assert_eq!(report.vector_paths_intersecting, 1);
        assert_eq!(
            carrier_action(&report, "vector_paths"),
            CarrierAction::DisclosedNotScrubbed
        );
        assert!(report.has_disclosed_residuals());
    }

    #[test]
    fn a_clip_marked_painted_path_keeps_its_clip_and_cuts_its_paint() {
        // `re W f`: the fill is cut, the clip survives as the ORIGINAL rect.
        let content = b"50 100 100 50 re W f 0 0 m 10 10 l S";
        let pdf = image_pdf(content, gray_4x4(), vec![]);
        let marked = mark_rects(pdf, &[[60.0, 110.0, 120.0, 140.0]]);
        let doc = Document::from_bytes(marked).unwrap();
        let (out, report) = apply_redactions(&doc, &SaveOptions::identity()).unwrap();
        assert_eq!(report.vector_paths_cut, 1);
        assert_eq!(report.vector_clips_kept, 1);
        let (_d, _x, content) = output_page(&out);
        let text = String::from_utf8_lossy(&content);
        assert!(text.contains("50 100 100 50 re W n"), "{text}");
        // The paint comes first, as four strips, then the clip.
        let paint_at = text.find(" f\n").or_else(|| text.find("\nf\n")).unwrap();
        let clip_at = text.find("re W n").unwrap();
        assert!(paint_at < clip_at, "{text}");
        assert!(
            text.contains("0 0 m 10 10 l S"),
            "the later stroke is untouched: {text}"
        );
    }

    // -- carrier scrub (/Info) -------------------------------------------

    #[test]
    fn info_dictionary_strings_are_scrubbed_and_disclosed() {
        let content = b"BT /F1 24 Tf 20 100 Td (SECRET PUBLIC) Tj ET";
        let stream = format!(
            "<< /Length {} >>\nstream\n{}\nendstream",
            content.len(),
            std::str::from_utf8(content).unwrap()
        );
        let pdf = assemble(
            &[
                "<< /Type /Catalog /Pages 2 0 R >>",
                "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 200] \
                 /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>",
                &stream,
                "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
                "<< /Title (SECRET dossier) /Author (Nobody) >>",
            ],
            "/Info 6 0 R",
        );
        let marked = mark_and_save(&pdf);
        let doc = Document::from_bytes(marked).unwrap();
        let (out, report) = apply_redactions(&doc, &SaveOptions::identity()).unwrap();
        assert!(!contains(&out, b"SECRET"), "SECRET survived in /Info");
        assert!(report.info_strings_scrubbed >= 1);
        // The non-matching /Author entry survives.
        assert!(contains(&out, b"Nobody"));
        // The carrier is reported as scrubbed.
        assert!(
            report
                .carriers
                .iter()
                .any(|c| c.carrier == "info" && c.action == CarrierAction::Scrubbed)
        );
    }

    #[test]
    fn a_structure_tree_is_disclosed_as_an_unscrubbed_residual() {
        // A tagged document's /ActualText//Alt//E can duplicate redacted
        // glyphs; this build detects and DISCLOSES it (never silently
        // leaves it), triggering the refusal-acknowledgement gate.
        let content = b"BT /F1 24 Tf 20 100 Td (SECRET PUBLIC) Tj ET";
        let stream = format!(
            "<< /Length {} >>\nstream\n{}\nendstream",
            content.len(),
            std::str::from_utf8(content).unwrap()
        );
        let pdf = assemble(
            &[
                "<< /Type /Catalog /Pages 2 0 R /StructTreeRoot 6 0 R >>",
                "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 200] \
                 /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>",
                &stream,
                "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
                "<< /Type /StructTreeRoot /K [] >>",
            ],
            "",
        );
        let marked = mark_and_save(&pdf);
        let doc = Document::from_bytes(marked).unwrap();
        let (out, report) = apply_redactions(&doc, &SaveOptions::identity()).unwrap();
        // The visible content is still removed...
        assert!(!contains(&out, b"SECRET"));
        // ...but the structure tree is disclosed, not silently scrubbed.
        assert!(
            report.has_disclosed_residuals(),
            "a present structure tree must be disclosed as a residual"
        );
        assert!(report.carriers.iter().any(|c| {
            c.carrier == "struct_tree" && c.action == CarrierAction::DisclosedNotScrubbed
        }));
    }

    #[test]
    fn nothing_to_apply_is_a_named_error() {
        let doc = Document::from_bytes(redactable_pdf()).unwrap();
        let err = apply_redactions(&doc, &SaveOptions::identity()).unwrap_err();
        assert!(matches!(err, RedactError::NothingToApply));
    }

    // -- container decomposition (§7.5.7 Strategy B) ---------------------

    /// A big-endian byte slice of `v` in `width` bytes (xref-stream field).
    fn be(v: u64, width: usize) -> Vec<u8> {
        v.to_be_bytes().get(8 - width..).unwrap_or(&[]).to_vec()
    }

    /// The body of an object stream holding `objects` (§7.5.7 layout: the
    /// `N` pairs, then the values at `/First`).
    fn objstm_body_local(objects: &[(u32, &str)]) -> String {
        let mut header = String::new();
        let mut body = String::new();
        for (num, text) in objects {
            header.push_str(&format!("{num} {} ", body.len()));
            body.push_str(text);
            body.push(' ');
        }
        let first = header.len();
        let data = format!("{header}{body}");
        format!(
            "<< /Type /ObjStm /N {} /First {first} /Length {} >>\nstream\n{data}\nendstream",
            objects.len(),
            data.len(),
        )
    }

    /// A PDF whose page tree, page dict and `/Info` dict live **compressed
    /// inside an object stream** (obj 6), reached via a cross-reference
    /// stream (obj 7). The `/Info` carries "SECRET" — the vector that would
    /// survive verbatim inside the untouched container if decomposition
    /// were not performed.
    fn build_objstm_pdf() -> Vec<u8> {
        let content = b"BT /F1 24 Tf 20 100 Td (SECRET PUBLIC) Tj ET";
        let cstream = format!(
            "<< /Length {} >>\nstream\n{}\nendstream",
            content.len(),
            std::str::from_utf8(content).unwrap()
        );
        let objstm = objstm_body_local(&[
            (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
            (
                3,
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 200] \
                 /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>",
            ),
            (8, "<< /Title (SECRET dossier) /Author (Nobody) >>"),
        ]);
        let file_objs: Vec<(u32, String)> = vec![
            (1, "<< /Type /Catalog /Pages 2 0 R >>".to_string()),
            (4, cstream),
            (
                5,
                "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
            ),
            (6, objstm),
        ];
        let mut buf = b"%PDF-1.5\n".to_vec();
        let mut offsets: Vec<(u32, usize)> = Vec::new();
        for (num, body) in &file_objs {
            offsets.push((*num, buf.len()));
            buf.extend_from_slice(format!("{num} 0 obj\n{body}\nendobj\n").as_bytes());
        }
        let xref_num = 7u32;
        let xref_at = buf.len();
        offsets.push((xref_num, xref_at));
        let size = 9u32;
        let mut data = Vec::new();
        for num in 0..size {
            let (t, f2, f3): (u64, u64, u64) = if num == 0 {
                (0, 0, 65535)
            } else if let Some((_, off)) = offsets.iter().find(|(n, _)| *n == num) {
                (1, *off as u64, 0)
            } else {
                match num {
                    2 => (2, 6, 0),
                    3 => (2, 6, 1),
                    8 => (2, 6, 2),
                    _ => (0, 0, 0),
                }
            };
            data.extend(be(t, 1));
            data.extend(be(f2, 4));
            data.extend(be(f3, 2));
        }
        let dict = format!(
            "<< /Type /XRef /Size {size} /W [1 4 2] /Root 1 0 R /Info 8 0 R /Length {} >>",
            data.len()
        );
        buf.extend_from_slice(format!("{xref_num} 0 obj\n{dict}\nstream\n").as_bytes());
        buf.extend_from_slice(&data);
        buf.extend_from_slice(b"\nendstream\nendobj\n");
        buf.extend_from_slice(format!("startxref\n{xref_at}\n%%EOF\n").as_bytes());
        buf
    }

    #[test]
    fn redacting_content_with_an_objstm_info_decomposes_the_container() {
        // Sanity: the fixture loads and /Info came from the object stream.
        let pdf = build_objstm_pdf();
        let d0 = Document::from_bytes(pdf.clone()).unwrap();
        let info_id = d0
            .trailer()
            .get(b"Info")
            .and_then(Object::as_reference)
            .unwrap();
        assert_eq!(
            d0.get(info_id).unwrap().provenance.container(),
            Some(ObjId::new(6, 0)),
            "the /Info dict must start life compressed in object stream 6"
        );

        let marked = mark_and_save(&pdf);
        let doc = Document::from_bytes(marked).unwrap();
        let (out, report) = apply_redactions(&doc, &SaveOptions::identity()).unwrap();

        // If the container were re-emitted verbatim, the compressed /Info's
        // "SECRET dossier" would survive. It must not.
        assert!(
            !contains(&out, b"SECRET"),
            "SECRET survived — the object stream was not decomposed"
        );
        let back = Document::from_bytes(out.clone()).unwrap();
        assert!(!contains(&all_decoded_content(&back), b"SECRET"));
        assert!(
            report.containers_decomposed >= 1,
            "the /Info's object stream must be decomposed"
        );
        assert!(report.info_strings_scrubbed >= 1);
        // The unrelated /Author survives the scrub + decomposition.
        assert!(contains(&out, b"Nobody"));
    }

    // -- the Table 192 overlay-marking ladder (§12.5.6.23) --------------

    /// A one-page document carrying a hand-built `/Redact` mark over the
    /// text, with `extra` spliced into the annotation dictionary.
    ///
    /// Hand-built rather than authored through `build_redact_mark` on
    /// purpose: these tests are about what APPLY does with the entries it
    /// finds in a file, including combinations pdfcer's own authoring
    /// cannot currently produce (`/RO`, `/Repeat`, `/OverlayText` with no
    /// `/DA`). Driving the reader from the writer would make the pair
    /// agree with each other while both disagreed with Table 192.
    fn pdf_with_redact_mark(extra: &str) -> Vec<u8> {
        let content = b"BT /F1 24 Tf 20 100 Td (SECRET PUBLIC) Tj ET";
        let stream = format!(
            "<< /Length {} >>\nstream\n{}\nendstream",
            content.len(),
            std::str::from_utf8(content).unwrap()
        );
        assemble(
            &[
                "<< /Type /Catalog /Pages 2 0 R >>",
                "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 200] \
                 /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R \
                 /Annots [6 0 R] >>",
                &stream,
                "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
                &format!(
                    "<< /Type /Annot /Subtype /Redact /Rect [20 95 200 125] \
                     /QuadPoints [20 125 200 125 20 95 200 95] {extra} >>"
                ),
            ],
            "",
        )
    }

    fn apply_marked(extra: &str) -> (Vec<u8>, RedactionReport) {
        let doc = Document::from_bytes(pdf_with_redact_mark(extra)).unwrap();
        apply_redactions(&doc, &SaveOptions::identity()).unwrap()
    }

    /// THE REPORTED DEFECT. `/OverlayText` reached the file and nothing
    /// ever drew it: the text was authored into the annotation, the
    /// annotation was deleted by apply, and the operator's words were gone
    /// with no runtime word about it.
    ///
    /// Asserts BOTH halves, because either alone can pass while the
    /// feature is broken — the glyphs can be drawn with no disclosure
    /// (rule 4), or the disclosure can be emitted with nothing drawn.
    #[test]
    fn overlay_text_is_burnt_into_the_page_and_disclosed() {
        let (out, report) =
            apply_marked("/IC [1 0 0] /OverlayText (CLASSIFIED) /Q 1 /DA (/Helv 10 Tf 0 g)");
        let doc = Document::from_bytes(out).unwrap();
        let content = all_decoded_content(&doc);
        assert!(
            contains(&content, b"CLASSIFIED"),
            "overlay text must be drawn into the page content"
        );
        assert_eq!(report.overlay_text_burned, 1);
        assert!(
            report
                .notes
                .iter()
                .any(|n| n.contains("overlay text burnt")),
            "the burn-in must be disclosed; notes were {:?}",
            report.notes
        );
        // The redaction itself still happened.
        assert!(!contains(&content, b"SECRET"));
    }

    /// Table 192's `/IC` row: "if this entry is absent, the interior of
    /// the redaction region is left transparent". pdfcer painted BLACK.
    ///
    /// The same shape of defect as painting a `/Separation /None` image
    /// (§8.6.6.4): a box that looks like what everyone expects, that the
    /// standard says not to paint.
    #[test]
    fn absent_ic_leaves_the_region_transparent() {
        let (out, report) = apply_marked("");
        let doc = Document::from_bytes(out).unwrap();
        let content = all_decoded_content(&doc);
        assert_eq!(report.overlay_transparent, 1);
        assert_eq!(report.overlay_text_burned, 0);
        assert!(
            !contains(&content, b" re\n"),
            "no /IC means no filled box: content was {:?}",
            String::from_utf8_lossy(&content)
        );
        assert!(
            report.notes.iter().any(|n| n.contains("TRANSPARENT")),
            "leaving the region unmarked must be disclosed; notes were {:?}",
            report.notes
        );
    }

    /// The rung that already worked, pinned so the transparency fix above
    /// cannot be over-applied into "never fill anything".
    #[test]
    fn present_ic_still_fills_the_region() {
        let (out, report) = apply_marked("/IC [0 0 1]");
        let doc = Document::from_bytes(out).unwrap();
        let content = all_decoded_content(&doc);
        assert_eq!(report.overlay_transparent, 0);
        assert!(contains(&content, b"0 0 1 rg"), "the /IC colour is used");
        assert!(contains(&content, b" re\n"), "a box is filled");
    }

    /// `/RO` takes precedence over everything and pdfcer cannot draw it.
    /// The requirement is that this is DISCLOSED and that the region is
    /// still visibly marked — an undrawn overlay on a region whose content
    /// is already destroyed is the one outcome this feature must not have.
    #[test]
    fn ro_is_disclosed_and_falls_back_to_a_visible_box() {
        let (out, report) = apply_marked("/RO 7 0 R /IC [0 1 0]");
        let doc = Document::from_bytes(out).unwrap();
        let content = all_decoded_content(&doc);
        assert_eq!(report.overlay_ro_not_drawn, 1);
        assert!(contains(&content, b"0 1 0 rg"), "falls back to the /IC box");
        assert!(
            report.notes.iter().any(|n| n.contains("/RO")),
            "an undrawn /RO must be disclosed; notes were {:?}",
            report.notes
        );
    }

    /// `/OverlayText` present with no `/DA` is malformed — Table 192 makes
    /// `/DA` required whenever `/OverlayText` is. Refusing is not an option
    /// (the content is already destroyed by the time the overlay is
    /// built), so pdfcer substitutes and says so.
    #[test]
    fn overlay_text_without_da_substitutes_and_discloses() {
        let (out, report) = apply_marked("/OverlayText (REDACTED)");
        let doc = Document::from_bytes(out).unwrap();
        assert!(contains(&all_decoded_content(&doc), b"REDACTED"));
        assert_eq!(report.overlay_text_burned, 1);
        assert!(
            report.notes.iter().any(|n| n.contains("no /DA")),
            "the substituted /DA must be disclosed; notes were {:?}",
            report.notes
        );
    }

    /// `/Repeat true` is not implemented. Silence here would be a claim
    /// that the region was tiled when it was not.
    #[test]
    fn repeat_is_ignored_and_disclosed() {
        let (_, report) = apply_marked("/OverlayText (X) /Repeat true /DA (/Helv 8 Tf 0 g)");
        assert!(
            report.notes.iter().any(|n| n.contains("/Repeat")),
            "an ignored /Repeat must be disclosed; notes were {:?}",
            report.notes
        );
    }

    /// The burnt-in text names a font resource, so that name must resolve
    /// from the page AFTER the annotation carrying the `/DA` is deleted.
    /// Without the merge the overlay draws with an unresolvable font and a
    /// viewer shows nothing — the exact "looks like it worked" failure.
    #[test]
    fn overlay_font_is_merged_into_the_page_resources() {
        let (out, _) = apply_marked("/OverlayText (HI) /DA (/Helv 9 Tf 0 g)");
        let doc = Document::from_bytes(out).unwrap();
        let pages = page_tree::pages(&doc).unwrap();
        let fonts = pages[0]
            .resources
            .get(b"Font")
            .map(|o| doc.resolve(o))
            .and_then(Object::as_dict)
            .expect("page must still have a /Font dict");
        assert!(fonts.get(b"Helv").is_some(), "the /DA font must resolve");
        // The page's pre-existing font is untouched.
        assert!(fonts.get(b"F1").is_some(), "existing resources survive");
    }

    /// THE SECOND REPORTED DEFECT: the find-and-mark path built its
    /// `RedactSpec` internally with `fill: None, overlay_text: None`, so
    /// every appearance choice an operator made was discarded before it
    /// reached a file — silently, because a mark with no `/IC` is a
    /// perfectly valid mark.
    ///
    /// Asserted END TO END (search → mark → save → reload → apply → the
    /// bytes) rather than by inspecting the authored annotation, because
    /// the annotation is DELETED by apply. A test that stopped at the mark
    /// would prove the appearance was written to an object that does not
    /// survive, which is exactly the gap that let this ship.
    #[test]
    fn marks_from_search_carry_the_operators_chosen_appearance() {
        use crate::annot_author::{Color, RedactAppearance};

        let doc = Document::from_bytes(redactable_pdf()).unwrap();
        let mut session = EditSession::new(doc);
        let appearance = RedactAppearance {
            fill: Some(Color::Rgb(0.0, 0.5, 1.0)),
            overlay_text: Some("GONE".to_string()),
            quadding: Quadding::Center,
        };
        let ids = session
            .mark_redactions_by_search_styled(
                "SECRET",
                &crate::edit::TextSearchOptions::default(),
                &appearance,
            )
            .unwrap();
        assert!(!ids.is_empty(), "search should have found SECRET");
        let (marked, _) = session
            .to_incremental_bytes(&SaveOptions::identity())
            .unwrap();

        let doc = Document::from_bytes(marked).unwrap();
        let (out, report) = apply_redactions(&doc, &SaveOptions::identity()).unwrap();
        let content = all_decoded_content(&Document::from_bytes(out).unwrap());
        assert_eq!(report.overlay_text_burned, 1, "the caption must be drawn");
        assert_eq!(report.overlay_transparent, 0, "a fill WAS chosen");
        assert!(contains(&content, b"GONE"), "the operator's caption");
        assert!(
            contains(&content, b"0 0.5 1 rg"),
            "the operator's fill colour; content was {:?}",
            String::from_utf8_lossy(&content)
        );
    }

    /// The unstyled verb keeps its historical meaning, so adding the
    /// styled sibling cannot have quietly changed every existing caller.
    #[test]
    fn unstyled_search_marks_still_default_to_no_appearance() {
        let doc = Document::from_bytes(mark_and_save(&redactable_pdf())).unwrap();
        let (_, report) = apply_redactions(&doc, &SaveOptions::identity()).unwrap();
        assert_eq!(report.overlay_text_burned, 0);
        assert_eq!(report.overlay_transparent, 1);
    }

    /// `/OverlayText` is a PDF text string (§7.9.2.2), so a UTF-16BE one
    /// must decode rather than being burnt in as mojibake — permanently,
    /// on the one operation with no undo.
    #[test]
    fn utf16be_overlay_text_decodes_before_layout() {
        // FEFF "NO" in UTF-16BE, as a hex string.
        let (out, report) = apply_marked("/OverlayText <FEFF004E004F> /DA (/Helv 9 Tf 0 g)");
        let doc = Document::from_bytes(out).unwrap();
        let content = all_decoded_content(&doc);
        assert_eq!(report.overlay_text_burned, 1);
        assert!(contains(&content, b"NO"), "UTF-16BE must be decoded");
        assert!(
            !contains(&content, b"\x00N"),
            "the raw UTF-16 code units must not reach the page"
        );
    }
}
