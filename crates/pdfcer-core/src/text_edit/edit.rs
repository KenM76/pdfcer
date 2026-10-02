//! # In-place text-edit surgery — REPLACE a show operand (Pass 14.1)
//!
//! This module extends Pass 8.0's advance-preserving **REMOVE** surgery
//! (`crate::redact`) to **REPLACE**: it rewrites the operand of a `Tj`/`TJ`
//! show operator with new text, re-encodes that text in the run's own font
//! encoding (`crate::text_edit::encoding`), preserves the §9.4.4 advance so
//! un-edited text stays put, and saves the change **incrementally**
//! (decision 014 §3; R34/R47/R70/R72).
//!
//! ## What REPLACE inherits from REMOVE, and what is new
//!
//! REMOVE is the special case of REPLACE where the new run is **empty**
//! (`A_new = 0`). The advance bookkeeping is identical
//! (`iso32000__ref__text_edit_surgery.md` §0): map a run to bytes by
//! decoding character **codes**, never slicing raw bytes; each code's width
//! `w0` comes from the **same** `/Widths`/AFM the render path uses. What is
//! new:
//!
//! - The replacement run has its own advance `A_new`, so the delta
//!   `ΔA = A_new − A_old` has either sign.
//! - Editing's default posture is the **opposite** of redaction's: it
//!   **REFLOWS** — a longer word pushes the rest of the line right, a
//!   shorter word pulls it left — so the default is to let the line shift by
//!   `ΔA`, not to pin survivors (surgery ref §3). A `PIN` posture (the
//!   Pass-8.0 compensating-`TJ` path) stays available for a run whose tail
//!   is absolutely positioned and must not move.
//! - The new codes come from the inverse-encoding builder; a character the
//!   run's font cannot provide is **REFUSED by name**, never faked
//!   (`crate::text_edit::encoding`, rule 4 / R71).
//!
//! ## The advance-delta formula (§9.4.4)
//!
//! Horizontal writing mode. The advance applied to `Tm` after painting one
//! glyph is, in text-space units,
//! `tx = ((w0/1000 − Tj/1000)·Tfs + Tc + Tw)·Th`, where `Tw` contributes
//! **only** when the code is the single byte `0x20` (§9.3.3). A run's total
//! advance folds this over its codes; `ΔA = A_new − A_old` drives the edit.
//! `Tfs`, `Tc`, `Tw`, `Th` are unchanged by the edit (same text state), so
//! only the width sum and the `0x20`-count differ.
//!
//! ## Disposition of "the rest of the line" (surgery ref §3)
//!
//! "The rest of the line" is the run of subsequent operators up to the next
//! `Tm`/`Td`/`TD`/`T*`/`'`/`"` re-anchor. Under `REFLOW` (default):
//! advance-relative followers auto-shift by `ΔA` for free (nothing to do); a
//! follower re-anchored by an **absolute `Tm`** does NOT auto-shift (§9.4.2:
//! `Tm` REPLACES, not concatenates), so its `e` operand gets `ΔA` added; a
//! `Td`/`TD`/`T*` marks the line boundary and is left alone. The edited line
//! MAY overflow the original right margin — that is **DISCLOSED**, not
//! reflowed; block re-wrap is deferred (FF-A). Under `PIN`: a compensating
//! `TJ` number consumes `ΔA` so survivors do not move.
//!
//! ## Marked content / tagged PDFs (§14.6/§14.7, T-disclose, R72)
//!
//! The edit rewrites ONLY the show operator(s) (and, under reflow, one or
//! more following `Tm`s). The enclosing `BDC …/MCID n… EMC` wrapper and the
//! `/MCID` value are therefore preserved **by construction** — the structure
//! tree's `(Pg, MCID)` reference stays valid. What goes stale is the
//! `/ActualText`/reading-order the tree records; 14.1 **DISCLOSES** that
//! (a stale `/ActualText` would win on extraction, §14.9.4) and does not
//! regenerate it (FF-H).
//!
//! ## Save mode (R34/R36/R70)
//!
//! Editing is NOT redaction: it uses the **default incremental save**. Prior
//! text survives in the document's history by design, and this is DISCLOSED
//! — truly removing text is REDACTION (Pass 8, R35), a different operation.
//! Only the edited content stream object (+ any collapsed extra content
//! objects on a multi-stream page) is re-emitted; everything else is the
//! original file bytes verbatim (incremental append ⇒ the original is a
//! byte-prefix of the output, R32/R46).
//!
//! ## Scope of the first cut (decision 014 §5.2 "13.1")
//!
//! Simple (`Type1`/`TrueType`/`MMType1`) fonts only; `Tj`/`TJ` anchors only.
//! NO reflow/block re-wrap (line overflow is disclosed), NO family-change
//! formatting (Pass 14.2), NO font subsetting (FF-C), NO composite/CJK/RTL
//! editing (R-INV-4), NO add-new-text (FF-D). The `'`/`"` show operators are a
//! named non-goal of this cut.
//!
//! ## `Pass 119.0` — the target is no longer assumed to be the page
//!
//! **Form-XObject content was a named non-goal of the 14.1 cut, and that
//! sentence stood here until 2026-08-20.** It is now false, and the correction
//! is worth more than the deletion would be: on a CAD-exported drawing the
//! page's own `/Contents` holds the producer's watermark and a **form
//! XObject** holds every label and the whole title block, so "form content is
//! out of scope" and "text editing does nothing on my drawings" were the same
//! sentence. The operator's estimate: *"99 % of the text I will want to
//! edit."*
//!
//! What changed is small and deliberately confined to the *addressing*:
//! [`EditPlanTarget`] names the stream object, its resource dictionary and its
//! sibling-collapse count, where those three used to be read straight off the
//! [`Page`]. **The surgery itself is untouched** — the §9.4.4 advance
//! arithmetic, the inverse encoding, the follower disposition and every
//! refusal operate on operators, and an operator does not know which stream it
//! was parsed from. See [`crate::text_edit::forms`] for the discovery half and
//! for the shared-invocation problem it exists to disclose.

use crate::crypto::PermissionBit;
use crate::text_edit::cause::{NotFoundReason, UnsupportedCause};
use crate::text_edit::cross_object;
use crate::text_edit::fallback::{self, Fallback, FallbackFace, FallbackUse, PreviewFallback};
use crate::text_edit::sibling;
use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::content::{ContentError, ContentStream, ContentTokenKind, Operation};
use crate::document::Document;
use crate::graph::ObjectGraph;
use crate::object::{Dict, Name, ObjId, Object, Stream};
use crate::page_tree::{self, Page, PageTreeError};
use crate::settings::UnmappableCode;
use crate::span::ByteSpan;
use crate::text_edit::code_alloc::{Allocation, allocate};
use crate::text_edit::encoding::{CompositeEncoding, InverseEncoding, RInvTrigger, Refusal};
use crate::text_edit::font_extend::{Blocked, FontExtension};
use crate::text_edit::program_glyphs::EmbeddedGlyphs;
use crate::text_edit::subset_augment::SubsetAugment;
use crate::text_extract::cmap::ToUnicodeCMap;
use crate::text_extract::font::ExtractFont;
use crate::text_state::{AmbientTextState, TextStateParam};
use crate::view::DocumentView;
use crate::writer::content::{emit_literal_string, emit_number};
use crate::writer::{DirtySet, SaveOptions, WriteError, save_incremental};

// ===================================================================
// Fill-colour graphics state (§8.6.8) — recorded by the walk for Pass
// 14.2's formatting surgery (`crate::text_edit::format`)
// ===================================================================
//
// Pass 14.1 (this module) never reads a run's fill colour: a REPLACE
// rewrites only the show operator's *codes*, so the colour operator that
// precedes the run is left byte-verbatim and no restore is needed. Pass
// 14.2 DOES need it — a localized size/colour/font change re-emits the
// show operator wrapped in state-set/state-restore operators, and the
// restore must reinstate the exact prior fill colour so every following
// operator is byte-for-byte unaffected. The walk therefore records the
// current fill colour on every show operator; because 14.1's `edit_text`
// ignores the new field, its output bytes are unchanged (verified by the
// unaltered 14.1 fixtures/tests).

/// The three *device* fill-colour operators (§8.6.4.2/.3/.4). Each names
/// its own colour space inline, so a device colour is fully modelled by
/// its operator and components — the space pdfcer can both classify and,
/// for 14.2, restore by re-emitting the recorded operator bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DeviceSpace {
    /// `g` — DeviceGray, one component.
    Gray,
    /// `rg` — DeviceRGB, three components.
    Rgb,
    /// `k` — DeviceCMYK, four components.
    Cmyk,
}

/// The fill colour in effect at a show operator, recorded so Pass 14.2 can
/// **restore** it byte-faithfully after a wrapped edit.
///
/// - [`Self::Default`] — no fill-colour operator has run; the §8.6.8
///   default (black `DeviceGray 0`) is in effect. Restored by emitting
///   `0 g`.
/// - [`Self::Device`] — a `g`/`rg`/`k` operator set it; both the classified
///   space+components (for the narrowing decision) and the operator's raw
///   bytes (for a byte-identical restore) are kept.
/// - [`Self::Other`] — a colour set through `sc`/`scn` in a resource-named
///   space (ICCBased, Separation/spot, DeviceN, Indexed, …): present but
///   NOT decoded (the [`crate::text_extract::TextColor::Other`] analog).
///   The raw operator byte sequence (the `cs` that set the space plus the
///   `sc`/`scn` that set the value) is kept so a tail can be restored
///   verbatim even though pdfcer cannot interpret the colour.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum FillState {
    /// §8.6.8 default: black DeviceGray 0. No operator set the fill colour.
    Default,
    /// A device fill colour, with its classified space, components, and the
    /// raw operator bytes for a faithful restore.
    Device {
        /// Which device operator set it.
        space: DeviceSpace,
        /// The colour components as written (0.0..=1.0 by §8.6.4).
        comps: Vec<f64>,
        /// The raw bytes of the setting operator, e.g. `1 0 0 rg`.
        raw: Vec<u8>,
    },
    /// A non-device fill colour pdfcer does not decode; the raw operator
    /// byte sequence to re-emit for a verbatim restore.
    Other {
        /// The `cs`(space) + `sc`/`scn`(value) operator bytes, space-joined.
        raw: Vec<u8>,
    },
}

impl FillState {
    /// Whether this is a non-device (`Other`) fill colour — the space a
    /// 14.2 colour edit cannot preserve and must DISCLOSE narrowing for
    /// (rule 4).
    pub(crate) const fn is_other(&self) -> bool {
        matches!(self, Self::Other { .. })
    }

    /// The operator bytes that reinstate this fill colour, for the
    /// state-restore half of a 14.2 wrapped edit. `Default` restores with
    /// `0 g` (the §8.6.8 default made explicit); `Device`/`Other` re-emit
    /// their recorded raw bytes verbatim (minimal-diff restore).
    pub(crate) fn restore_bytes(&self) -> Vec<u8> {
        match self {
            Self::Default => b"0 g".to_vec(),
            Self::Device { raw, .. } | Self::Other { raw } => raw.clone(),
        }
    }

    /// The operator bytes that reinstate this colour as the **stroking**
    /// colour (Pass 19.2).
    ///
    /// The only difference from [`Self::restore_bytes`] is the `Default`
    /// arm, and it is a difference that matters: §8.6.8 gives the stroking
    /// and non-stroking colours *separate* graphics-state entries with the
    /// same initial value (black `DeviceGray 0`), and the operators that
    /// set them are spelled in different cases — `G`/`RG`/`K`/`SC`/`SCN`
    /// stroking, `g`/`rg`/`k`/`sc`/`scn` non-stroking. Restoring an unset
    /// stroking colour with `0 g` would put the *fill* colour back to black
    /// while leaving the stroking colour wherever synthetic bold left it —
    /// a silent corruption of two parameters at once.
    ///
    /// The `Device`/`Other` arms re-emit their own recorded bytes, which
    /// were already captured from an uppercase operator by the walk, so no
    /// case conversion is performed (or possible — an `Other` restore is an
    /// opaque `CS … SCN` sequence).
    pub(crate) fn restore_bytes_stroking(&self) -> Vec<u8> {
        match self {
            Self::Default => b"0 G".to_vec(),
            Self::Device { raw, .. } | Self::Other { raw } => raw.clone(),
        }
    }
}

/// The line width (§8.4.3.2 `w`) in effect at a show operator, recorded so
/// Pass 19.2's synthetic bold can restore it (Pass 19.2).
///
/// ## Why this is tracked at all, and why it is not a `f64`
///
/// Synthetic bold emits text rendering mode 2 (fill-then-stroke) plus a
/// line width, and §9.3.6 interprets that width **in user space** — it is
/// the ordinary graphics-state line width, the same one a later `S` on a
/// *path* would use. So a synthetic-bold run that does not put the width
/// back does not merely leave stale text state: it changes the weight of
/// every subsequent stroked path in the content stream. That is a
/// minimal-diff violation in content pdfcer never claimed to touch.
///
/// It is an enum rather than a bare number for the same reason
/// [`crate::text_state::AmbientOrigin`] is: the restore must know whether
/// the value is *provably* Table 52's initial (in which case `1 w` is
/// correct and byte-faithful in spirit) or was set by an operator whose
/// exact spelling should come back unchanged.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum LineWidth {
    /// No `w` operator has run; Table 52's initial line width of **1.0** is
    /// in force.
    Initial,
    /// A `w` operator set it; its raw bytes are kept for a byte-faithful
    /// restore (`0.5000 w` comes back as `0.5000 w`, not `0.5 w`).
    Observed {
        /// The width operand as parsed — used to decide whether an emitted
        /// width is a no-op, never to spell the restore.
        value: f64,
        /// The whole operator as written, e.g. `0.5000 w`.
        raw: Vec<u8>,
    },
}

impl LineWidth {
    /// The width in force, for the no-op comparison.
    pub(crate) const fn value(&self) -> f64 {
        match self {
            // §8.4.3.2 / Table 52: the initial line width is 1.0.
            Self::Initial => 1.0,
            Self::Observed { value, .. } => *value,
        }
    }

    /// The operator bytes that reinstate this line width.
    pub(crate) fn restore_bytes(&self) -> Vec<u8> {
        match self {
            Self::Initial => b"1 w".to_vec(),
            Self::Observed { raw, .. } => raw.clone(),
        }
    }
}

/// Multiply two PDF 3×2 matrices, `m` applied **first** (§8.3.3).
///
/// A PDF matrix `[a b c d e f]` denotes
///
/// ```text
/// | a  b  0 |
/// | c  d  0 |
/// | e  f  1 |
/// ```
///
/// and a point is a **row** vector multiplied on the left, so composing
/// "apply `m`, then apply `n`" is the product `m × n` in that order. Getting
/// the order backwards produces a transform that is right for every
/// symmetric case (pure scale, pure translation applied to the origin) and
/// wrong for exactly the asymmetric ones this Pass introduces — a shear, and
/// a translation under a shear. That is why this is a named function with a
/// test rather than six lines inlined at the two call sites.
pub(crate) fn mat_mul(m: [f64; 6], n: [f64; 6]) -> [f64; 6] {
    [
        m[0] * n[0] + m[1] * n[2],
        m[0] * n[1] + m[1] * n[3],
        m[2] * n[0] + m[3] * n[2],
        m[2] * n[1] + m[3] * n[3],
        m[4] * n[0] + m[5] * n[2] + n[4],
        m[4] * n[1] + m[5] * n[3] + n[5],
    ]
}

/// The identity matrix — `BT`'s reset value for both `Tm` and `Tlm`
/// (§9.4.1 Table 107).
pub(crate) const IDENTITY: [f64; 6] = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

/// How the rest of the edited line is treated after a REPLACE (surgery ref
/// §3). The default is [`Self::Reflow`] — in-place editing intends the line
/// to grow/shrink.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum FollowerDisposition {
    /// Let the line shift by `ΔA`: advance-relative followers move for free;
    /// an absolute-`Tm` follower gets `ΔA` added to its `e` (the default).
    #[default]
    Reflow,
    /// Pin survivors in place with a compensating `TJ` number (the Pass-8.0
    /// path), for a justified / right-aligned tail that must not move.
    Pin,
}

/// One in-place text-edit request against a page.
///
/// The anchor operator is located by finding [`Self::find`] in a single
/// show operator's decoded text — either the first such operator, or (when
/// [`Self::pinned_span`] is set from a Pass-14.0
/// [`GlyphProvenance::operator_span`](crate::text_extract::GlyphProvenance))
/// exactly the operator that provenance points at. That `operator_span` is
/// how Pass 14.0's model LOCATES the run to rewrite; this surgery re-tokenizes
/// the same content buffer and matches the same span.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct EditRequest {
    /// 0-based page index.
    pub page_index: usize,
    /// The text to locate — within one show operator's decoded run, or
    /// (`Pass 256.0`) across consecutive spannable operators of one text
    /// object when no single operator holds it (see `find_anchor_span`).
    pub find: String,
    /// The replacement text (re-encoded into the run's font).
    pub replace: String,
    /// When set, only consider the show operator that this byte span in the
    /// decoded content buffer NAMES — the provenance-pinned path.
    ///
    /// "Names", not "equals": two spans identify the same operator here, and
    /// both are accepted (see [`pin_names_operator`]). A span covering the
    /// operator token alone (`Tj`) is what
    /// [`GlyphProvenance::operator_span`](crate::text_extract::GlyphProvenance::operator_span)
    /// publishes; a span covering the operands too (`(hello) Tj`) is what this
    /// module's own walk records. Requiring exact equality against the second
    /// form silently broke every provenance-pinned request — fixed in Pass
    /// 19.3, with the history in `pin_names_operator`'s documentation.
    pub pinned_span: Option<ByteSpan>,
    /// **Which content stream to edit** (`Pass 119.0`). Defaults to
    /// [`EditTarget::Auto`], which is what every pre-119.0 caller gets by
    /// construction and what a shell should keep using unless it has a
    /// specific reason not to.
    pub target: EditTarget,
    /// Let the match **begin** at [`Self::pinned_span`] and run on across
    /// following spannable operators, instead of being confined to the
    /// pinned one (`Pass 272.0`).
    ///
    /// # What problem this exists for
    ///
    /// A pin and a `find` answer two different questions, and until this flag
    /// a caller could only ask one of them at a time:
    ///
    /// - **`find` alone** says *what* to edit and lets pdfcer pick an
    ///   occurrence. And **not** "the first one": `find_anchor` tries a
    ///   single-operator match across the *whole page* before the spanning
    ///   search runs, so a single-operator occurrence anywhere beats a
    ///   spanning one above it — which makes a spanning run **unreachable**
    ///   by `find` alone whenever a single-operator twin exists at all.
    /// - **a pin alone** says *where*, exactly — but confines the match to
    ///   that one operator, and a producer that emits one glyph per operator
    ///   will not have the whole run in any single one.
    ///
    /// A click-driven shell has exactly the information the second lacks. It
    /// knows which operator the operator touched; it just had no way to say
    /// *"start here, and keep going"*.
    ///
    /// **THIS PASS HAS NO KNOWN FILE ON WHICH IT BITES, AND THAT IS
    /// RECORDED RATHER THAN QUIETLY DROPPED.**
    ///
    /// It was requested citing a bill-of-materials sheet an operator could not
    /// edit. The requester **retracted that motivation the same day**, before
    /// this shipped: the real cause was that his drawing's fonts are
    /// subset-embedded and carry **46 of 95 printable ASCII characters with
    /// every lowercase letter absent** — he was typing letters the font does
    /// not have, and `UnsupportedFont` was correct.
    ///
    /// They then measured the population this verb addresses across all four
    /// of his sheets. A run must **both** repeat on the page **and** span more
    /// than one show operator: 57–133 of the first, 4–11 of the second, and
    /// **the intersection is ZERO**.
    ///
    /// So the gap is real and precisely located — it is a property of the API,
    /// not of one drawing — but the numbers that motivated it did not describe
    /// it. Kept because someone will hit it; documented this way because a
    /// motivating measurement that turned out to be empty is exactly the thing
    /// a later reader would otherwise cite as evidence.
    ///
    /// # Deliberately opt-in
    ///
    /// Setting a pin **without** this flag still means what it always meant:
    /// the match must lie inside the pinned operator. Widening that silently
    /// would change what every existing caller's refusal means, and the
    /// consuming shell asked for it to be explicit for exactly that reason.
    ///
    /// See [`Self::spanning_from`].
    pub span_from_pin: bool,
}

impl EditRequest {
    /// A find/replace request on `page_index` (no span pin), targeting
    /// [`EditTarget::Auto`].
    #[must_use]
    pub fn find_replace(page_index: usize, find: &str, replace: &str) -> Self {
        Self {
            page_index,
            find: find.to_owned(),
            replace: replace.to_owned(),
            pinned_span: None,
            target: EditTarget::Auto,
            // Meaningless without a pin, and `false` is what every caller
            // written before `Pass 272.0` gets by construction.
            span_from_pin: false,
        }
    }

    /// An edit request replacing **the whole show operator at `span`**
    /// (`Pass 152.0`) — the twin of
    /// [`FormatRequest::whole_operator`](crate::text_edit::FormatRequest::whole_operator).
    ///
    /// # Why this exists when the behaviour already did
    ///
    /// It is a **discoverability** fix, and the cost of not having it is
    /// measured rather than assumed. The mechanism — an empty `find` with a
    /// pin — has worked on this verb since `Pass 145.0`, is tested
    /// (`whole_operator_pin.rs::edit_text_gets_the_same_affordance`),
    /// disclosed, and documented. It was documented in **one trailing
    /// sentence at the end of the `FormatRequest` section**, with no example
    /// and no symbol to grep for.
    ///
    /// On 2026-08-28 `pdfcer-gui` filed a defect against this exact gap. Their
    /// report cites `Pass 145.0` and `FormatRequest::whole_operator` **by
    /// name** — they had read the very section that contains the sentence —
    /// and still concluded the edit verb could only be addressed by `find`.
    /// They then listed three ways they had tried to *describe* an operator
    /// they had already *located*.
    ///
    /// A capability nobody can find is not shipped, and no gate in this
    /// project can detect that: the code is correct, the test is green, the
    /// sentence is true. The only symptom is somebody asking for what they
    /// already have.
    ///
    /// # What it targets
    ///
    /// The pinned operator, entire. **Not** the text run it belongs to — one
    /// [`TextRun`](crate::text_extract::TextRun) can carry glyphs from several
    /// show operators (**2,420 of 18,559 runs, 13 %**, over pdfcer's corpus;
    /// `crates/pdfcer-core/tests/operator_span_invariant.rs`), because
    /// extraction closes a run on *geometry* and a producer closes an operator
    /// wherever its writer felt like. The report discloses the extent taken.
    ///
    /// # Why a caller must not rebuild `find` instead
    ///
    /// A run's `text` is **not** in 1:1 correspondence with its glyphs, and on
    /// a CAD drawing it is not even close: `text_extract` synthesises
    /// inter-glyph spacing so a line broken across several show operators
    /// reads as one string. Those spaces are **not in the content stream**, so
    /// a `find` rebuilt from extracted text cannot match. It fails invisibly
    /// on simple test text and routinely on the drawings this project exists
    /// for — which is the worst combination a locator API can have.
    ///
    /// # Equivalent to
    ///
    /// `EditRequest::find_replace(page_index, "", replace).pinned(span)`. Both
    /// spellings work and are pinned equal by a test; this one says what it
    /// means. An empty `find` with **no** pin is still refused, so a caller
    /// who forgot to pin gets a refusal rather than silent whole-operator
    /// behaviour on an operator pdfcer chose for them.
    ///
    /// ```no_run
    /// # use pdfcer_core::text_edit::EditRequest;
    /// # fn f(span: pdfcer_core::span::ByteSpan) {
    /// let req = EditRequest::whole_operator(0, span, "Rev B");
    /// assert!(req.find.is_empty());
    /// assert_eq!(req.pinned_span, Some(span));
    /// # }
    /// ```
    #[must_use]
    pub fn whole_operator(page_index: usize, span: ByteSpan, replace: &str) -> Self {
        Self::find_replace(page_index, "", replace).pinned(span)
    }

    /// Replace `find` in a run that **begins at** the operator `span` names
    /// and may continue across following operators (`Pass 272.0`).
    ///
    /// The disambiguating form of [`Self::find_replace`]: `find` says *what*,
    /// the pin says *which one*. Use it whenever the text may repeat on the
    /// page and the caller already knows which occurrence it means — a click,
    /// a hit test, a
    /// [`GlyphProvenance::operator_span`](crate::text_extract::GlyphProvenance::operator_span).
    ///
    /// # Why it is not just `find_replace` with a pin
    ///
    /// Because that combination already means something else, and quietly
    /// changing it would rewrite what every existing caller's refusal means.
    /// A plain pin **confines** the match to one operator. This one lets it
    /// **start** there.
    ///
    /// # What is unchanged, and this is the whole safety argument
    ///
    /// The span search itself is the same one [`Self::find_replace`] already
    /// uses, with the same guards — same `spannable` test, same `same_line`
    /// tolerance for `Td`/`Tm`, same trim-to-the-operators-the-match-touches
    /// rule, and the same requirement that the match **start inside the
    /// anchor operator's own text**. The only thing this changes is *where
    /// the search starts*: at the pinned operator instead of at the first
    /// operator on the page.
    ///
    /// So a span is still never longer than it needs to be, and a pinned
    /// request still names exactly one operator — it names where the run
    /// *begins*, which is what a click knows.
    ///
    /// # Errors, when used
    ///
    /// [`EditError::PinnedSpanNotFound`] if the span names no operator, and
    /// [`EditError::NoMatch`] if `find` does not begin inside it. Note the
    /// second is a **real** refusal here rather than a fallback: without this
    /// constructor, the same request resolved to byte 0 of the pinned
    /// operator and failed further downstream with a less useful message.
    ///
    /// # Examples
    ///
    /// ```
    /// use pdfcer_core::text_edit::EditRequest;
    /// # fn f(span: pdfcer_core::span::ByteSpan) {
    /// let req = EditRequest::spanning_from(0, span, "12345", "67890");
    /// assert_eq!(req.pinned_span, Some(span));
    /// assert!(req.span_from_pin);
    /// # }
    /// ```
    #[must_use]
    pub fn spanning_from(page_index: usize, span: ByteSpan, find: &str, replace: &str) -> Self {
        let mut req = Self::find_replace(page_index, find, replace).pinned(span);
        req.span_from_pin = true;
        req
    }

    /// Pin the target show operator by byte span, returning `self`.
    ///
    /// The twin of
    /// [`FormatRequest::pinned`](crate::text_edit::FormatRequest::pinned), and
    /// added with it in mind: the field was public and settable, but only the
    /// format verb had a builder, so the two siblings read differently for the
    /// same idea.
    ///
    /// Either byte-span convention for "the show operator" is accepted — the
    /// operator token alone (what
    /// [`GlyphProvenance::operator_span`](crate::text_extract::GlyphProvenance::operator_span)
    /// publishes) or the operand-inclusive extent (what the authoring walk
    /// records). See `pin_names_operator` for why neither side was made to
    /// adopt the other's spelling.
    ///
    /// With a non-empty `find` the pin narrows *which operator* and the find
    /// narrows *which characters within it*. With an empty `find` the whole
    /// operator is the target — see [`Self::whole_operator`].
    #[must_use]
    pub const fn pinned(mut self, span: ByteSpan) -> Self {
        self.pinned_span = Some(span);
        self
    }

    /// Set the [`EditTarget`], returning `self`.
    #[must_use]
    pub const fn with_target(mut self, target: EditTarget) -> Self {
        self.target = target;
        self
    }
}

/// Which content stream an edit is aimed at (`Pass 119.0`).
///
/// # Why this exists
///
/// A page's visible text does not all live in the page's own `/Contents`.
/// Anything drawn by a form XObject (§8.10.1) lives in **that stream object**,
/// and the surgery rewrites one buffer at a time. Before `Pass 119.0` the
/// buffer was always the page's, so form text was unreachable — the asymmetry
/// [`TextRun::editability`](crate::text_extract::TextRun::editability)
/// published. This names the buffer instead of assuming it.
///
/// # Why [`Self::Auto`] is the default rather than an explicit choice
///
/// A caller that types "replace *Rev A* with *Rev B*" does not know, and
/// should not have to know, which of a page's content streams holds those
/// glyphs — that is a fact about the producer's export settings, not about the
/// operator's intent. `Auto` searches the page's own content first and then
/// each reachable form in `Do` order, so the common case needs no decision.
/// The explicit variants exist for a shell that already knows (it has a
/// [`GlyphProvenance`](crate::text_extract::GlyphProvenance) in hand, or the
/// operator picked a target from a list) and for a batch caller that wants a
/// hard failure rather than a search.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum EditTarget {
    /// Search the page's own `/Contents` first, then every reachable form
    /// XObject in `Do` order, and edit the first stream that yields a match.
    ///
    /// A refusal that is *not* "no match here" (a font-coverage refusal, an
    /// unsupported run) stops the search and is reported: those describe the
    /// run the caller found, and retrying elsewhere would replace a precise
    /// diagnosis with a vague one.
    #[default]
    Auto,
    /// Edit the page's own `/Contents` only. A match inside a form is not
    /// considered, and its absence reports as a normal no-match.
    PageContents,
    /// Edit this form XObject's own content stream.
    Form {
        /// The form stream's object number, as reported by
        /// [`Editability::InsideForm`](crate::text_extract::Editability::InsideForm)
        /// or [`FormRef::id`](crate::text_edit::forms::FormRef::id).
        object: u32,
    },
}

/// Per-edit options.
#[derive(Debug, Clone, Copy, Default)]
#[non_exhaustive]
pub struct EditOptions {
    /// How the rest of the line is disposed (default [`FollowerDisposition::Reflow`]).
    pub disposition: FollowerDisposition,
    /// Reads embedded font programs so a character an embedded subset
    /// outlines but the page never shows can be typed (decision 172). `None`
    /// keeps the embedded-subset floor as it was: such characters refuse.
    pub embedded_glyphs: Option<&'static dyn EmbeddedGlyphs>,
    /// Extends an embedded TrueType subset from its installed face when the
    /// program has no outline for a character (decision 173). `None` keeps
    /// such characters refused.
    pub subset_augment: Option<SubsetAugment>,
    /// Sets characters the run's font cannot carry in another `/Font`
    /// resource on the page naming the same face, switching to it with `Tf`
    /// for the replacement only (decision 174). Off by default.
    pub sibling_fonts: bool,
    /// Sets each character the run's font cannot encode in this face, tried
    /// only after every route that keeps the run's font refuses (decision
    /// 172, 173 and 174 included). The match must lie in one `Tj`/`TJ`.
    /// `None` keeps such characters refused.
    ///
    /// A reference rather than a value so the options stay `Copy`.
    pub fallback: Option<&'static FallbackFace>,
}

impl EditOptions {
    /// Set the follower [`FollowerDisposition`], returning `self` — the
    /// out-of-crate constructor, since [`EditOptions`] is `#[non_exhaustive]`
    /// (a struct literal is not usable from `pdfcer`).
    #[must_use]
    pub fn with_disposition(mut self, disposition: FollowerDisposition) -> Self {
        self.disposition = disposition;
        self
    }

    /// Install the embedded-program reader, returning `self`.
    ///
    /// # Examples
    ///
    /// ```
    /// use pdfcer_core::text_edit::{EditOptions, EmbeddedGlyphs, ProgramGlyph};
    ///
    /// #[derive(Debug)]
    /// struct NoGlyphs;
    /// impl EmbeddedGlyphs for NoGlyphs {
    ///     fn unicode_glyph(&self, _: &[u8], _: char) -> Option<ProgramGlyph> {
    ///         None
    ///     }
    /// }
    /// let opts = EditOptions::default().with_embedded_glyphs(&NoGlyphs);
    /// assert!(opts.embedded_glyphs.is_some());
    /// ```
    #[must_use]
    pub fn with_embedded_glyphs(mut self, glyphs: &'static dyn EmbeddedGlyphs) -> Self {
        self.embedded_glyphs = Some(glyphs);
        self
    }

    /// Install the decision 173 subset augmenter, returning `self`. It acts
    /// only alongside [`Self::with_embedded_glyphs`], after route A finds no
    /// outline in the embedded program.
    #[must_use]
    pub fn with_subset_augment(mut self, augment: SubsetAugment) -> Self {
        self.subset_augment = Some(augment);
        self
    }

    /// Allow the decision 174 same-face sibling fallback, returning `self`.
    ///
    /// # Examples
    ///
    /// ```
    /// use pdfcer_core::text_edit::EditOptions;
    ///
    /// assert!(EditOptions::default().with_sibling_fonts(true).sibling_fonts);
    /// ```
    #[must_use]
    pub fn with_sibling_fonts(mut self, allow: bool) -> Self {
        self.sibling_fonts = allow;
        self
    }

    /// Install a [`FallbackFace`] for characters the run's font cannot
    /// encode, returning `self`.
    ///
    /// `EditOptions` is `Copy`, so the face is borrowed for `'static`: leak
    /// each distinct face once and reuse the reference across edits.
    ///
    /// # Examples
    ///
    /// ```
    /// use pdfcer_core::text_edit::{EditOptions, FallbackFace};
    ///
    /// let face: &'static FallbackFace =
    ///     Box::leak(Box::new(FallbackFace::Named("Helvetica".to_owned())));
    /// assert!(EditOptions::default().with_fallback(face).fallback.is_some());
    /// ```
    #[must_use]
    pub fn with_fallback(mut self, face: &'static FallbackFace) -> Self {
        self.fallback = Some(face);
        self
    }
}

/// Which trust level the edited run's glyphs come from, as far as
/// `pdfcer-core` can determine WITHOUT a font rasterizer (R21). The shell
/// refines [`Self::NonEmbedded`] into decision-012 `Bundled` vs `Supplied`
/// by consulting its own `FontEnvironment`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum EditGlyphSource {
    /// The run's font carries an embedded program (`/FontFile`/`2`/`3`).
    Embedded,
    /// The run's font is non-embedded — a bundled Base-14 or an
    /// operator-supplied face renders it (decision 012).
    NonEmbedded,
}

/// The outcome of a successful edit.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct EditOutcome {
    /// The saved (incrementally-appended) PDF bytes.
    pub bytes: Vec<u8>,
    /// The disclosure/diagnostic report.
    pub report: EditReport,
}

/// What the edit did and what it disclosed (fuzzy-never-sneaky, rule 4).
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct EditReport {
    /// The `/BaseFont` of the edited run (subset tag included).
    pub base_font: String,
    /// The core-visible glyph source (see [`EditGlyphSource`]).
    pub glyph_source: EditGlyphSource,
    /// Whether the run's font is an embedded **subset** (a 6-letter `+`
    /// tag) — the one refusal case in decision 014's four-case table.
    pub subset: bool,
    /// `ΔA = A_new − A_old` in text-space units.
    pub advance_delta: f64,
    /// The follower disposition actually used.
    pub disposition: FollowerDisposition,
    /// How many following absolute `Tm`s were repositioned by `ΔA` (reflow).
    pub followers_repositioned: u64,
    /// How many consecutive show operators the match spanned (`Pass 256.0`).
    ///
    /// **1** for the ordinary case — the match lay inside one operator, as
    /// every edit did before `Pass 256.0`. Greater than 1 when the producer
    /// wrote the matched text across several operators (one glyph per `Tj`
    /// with a `Td` between, the shape the operator's own document had) and
    /// pdfcer edited them as one run: the replacement went into the
    /// operator holding the match's END, the matched glyphs were removed
    /// from the earlier ones (an operator emptied that way is left as an
    /// empty `() Tj`, its `Td` step re-spaced), and every following
    /// operator on the line moved by the net advance. Never absent, so a
    /// shell can always show it; rule 4 says a multi-operator edit is
    /// disclosed, and this is the number that discloses it.
    pub operators_spanned: u64,
    /// The `/MCID` of the enclosing marked-content sequence, if the edit was
    /// inside a Tagged-PDF sequence (its wrapper is preserved; §14.7).
    pub tagged_mcid: Option<i64>,
    /// The content-stream object number that was rewritten. For a form-XObject
    /// edit (`Pass 119.0`) this is the **form stream's** object number, not the
    /// page's — the page's `/Contents` is untouched in that case.
    pub content_object: u32,
    /// Extra content objects collapsed/emptied on a multi-stream page. Always
    /// `0` for a form edit: a form XObject is exactly one stream (§8.10.1), so
    /// there is nothing to collapse.
    pub extra_objects_emptied: u64,
    /// The form XObject the edit went into, or `None` when the edit rewrote
    /// the page's own `/Contents` (`Pass 119.0`).
    pub form_object: Option<u32>,
    /// **How many places in the document paint the edited form** — the
    /// fan-out of the edit, counted document-wide and transitively through
    /// nesting. `1` for the ordinary case; `0` when the edit was not in a form.
    ///
    /// Greater than `1` means the edit is visible somewhere the operator was
    /// not looking, which the standard explicitly permits and provides no way
    /// to prevent: a form XObject "may be painted multiple times — either on
    /// several pages or at several locations on the same page" (§8.10.1) and
    /// **no clause anywhere binds one to a page** (`FX-N1`). A caller that
    /// drops this field is a caller that changes six drawing sheets while
    /// showing one.
    pub form_invocations: u64,
    /// The zero-based page indices the edited form appears on, ascending.
    /// Empty when the edit was not in a form.
    pub form_pages: Vec<usize>,
    /// Every operator-facing disclosure, verbatim (surfaced by the UI/CLI).
    pub disclosures: Vec<String>,
    /// The characters set in [`EditOptions::fallback`]'s face, and that face;
    /// `None` when the run's own font (or a decision 174 sibling) took the
    /// whole replacement.
    pub fallback: Option<FallbackUse>,
}

/// A failure to edit — every variant is a clean, named outcome, never a
/// crash (rule 4). A [`Self::Refused`] is the inverse-encoding gate saying
/// no by name.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum EditError {
    /// The inverse-encoding / font-on-edit gate refused, by name.
    #[error(transparent)]
    Refused(Refusal),
    /// No page at the requested index.
    #[error("no page at index {0}")]
    PageIndex(usize),
    /// The find text was not present in any editable run; `reason` says
    /// whether it is absent or only reachable by joining text objects.
    #[error("text to edit ({find:?}) was not found in an editable run on the page{reason}")]
    NoMatch {
        /// The text searched for.
        find: String,
        /// Why nothing matched.
        reason: NotFoundReason,
    },
    /// A **pinned** request named a byte span that matches no show operator in
    /// the buffer being edited (`Pass 118.0`).
    ///
    /// # Why this is not [`Self::NoMatch`], and the cost of it having been
    ///
    /// `find_anchor` short-circuits on a pinned request: if the pin is
    /// present, the text search is **never attempted**. So a pin that names
    /// nothing used to report `NoMatch(find)` — *"text to edit (`"p"`) was not
    /// found in an editable run on the page"* — which **names the operator's
    /// own text as the problem when the text is not the problem at all.**
    ///
    /// Those are different diagnoses with different fixes. `NoMatch` means
    /// *the text is not on this page*; this means *the text is there and the
    /// caller pointed at the wrong buffer* — which, in practice, means the run
    /// lives inside a form XObject while the surgery is looking at the page
    /// stream. A shell cannot word its own message without being able to tell
    /// them apart.
    ///
    /// **This message has now misled twice.**
    /// [`pin_names_operator`]'s own doc comment records the `Pass 19.3`
    /// incident with the identical symptom — a perfectly ordinary page
    /// refusing with "was not found" — and the consuming shell spent an
    /// investigation on the same sentence again on 2026-08-20. Their words:
    /// *"`EditError::PinnedSpanNotFound { .. }` would have made today's
    /// investigation a two-minute one."*
    ///
    /// The span is carried because the caller's next question is always
    /// *which* pin, and because a pin that is plausible-but-wrong (the wrong
    /// span convention) looks identical to one that is absent.
    #[error(
        "the pinned span {start}..{end} names no show operator in this content stream -- the text is not the problem; the pin is pointing at a different buffer (a form XObject's content is not the page's)"
    )]
    PinnedSpanNotFound {
        /// First byte of the pin, as supplied.
        start: usize,
        /// One past the last byte of the pin, as supplied.
        end: usize,
    },
    /// The run is real but this cut cannot edit it (composite font, a
    /// `'`/`"` anchor, a cross-element `TJ` match, …).
    #[error("this run cannot be edited: {0}")]
    Unsupported(UnsupportedCause),
    /// The document is encrypted and this edit is not permitted: see
    /// [`EditError::DocumentEncrypted`](crate::edit::EditError::DocumentEncrypted).
    #[error("{}", crate::edit::ENCRYPTED_EDIT_REFUSED)]
    Encrypted,
    /// The page's content stream could not be parsed.
    #[error("content stream parse failed: {0}")]
    Content(#[from] ContentError),
    /// The page tree could not be walked.
    #[error("page tree error: {0}")]
    PageTree(#[from] PageTreeError),
    /// The incremental save failed.
    #[error("save failed: {0}")]
    Write(#[from] WriteError),
}

impl EditError {
    /// A [`Self::NoMatch`] whose text is simply absent.
    pub(crate) fn no_match(find: impl Into<String>) -> Self {
        Self::NoMatch {
            find: find.into(),
            reason: NotFoundReason::NoSuchText,
        }
    }
}

// ===================================================================
// The walk — one pass over the page content, recording every operator
// ===================================================================

/// One element of a decoded show operator's operand list.
///
/// `pub(crate)` because Pass 14.2's formatting surgery
/// (`crate::text_edit::format`) reconstructs an anchor operator's element
/// list — splitting it at the match into pre/mid/post segments — and
/// re-emits each segment with [`emit_show`].
#[derive(Debug, Clone)]
pub(crate) enum ShowElem {
    /// A show string (its raw code bytes).
    Str(Vec<u8>),
    /// A `TJ` kerning number (thousandths of text space, §9.4.3).
    Num(f64),
}

/// Which show operator an anchor is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShowOp {
    Tj,
    TJ,
    Quote,
    DoubleQuote,
}

/// One code decoded off a show operator, with everything needed to map a
/// text match back to the bytes to splice.
#[derive(Debug, Clone)]
pub(crate) struct ShowSlot {
    /// The character code this slot shows.
    ///
    /// `u32`, not `u8`, since Pass 21.1. A simple font's code IS one byte
    /// and always will be (§9.4.3), so this is wider than that case needs —
    /// but a composite font addresses glyphs by a multi-byte code, and the
    /// `u8` here is the specific thing that made composite runs
    /// unrepresentable rather than merely unimplemented. Widening it is
    /// behaviour-preserving on its own; it is the substrate the rest of
    /// 21.1 needs, landed separately so that the step which DOES change
    /// behaviour is not tangled with a mechanical type change.
    ///
    /// Pair it with [`Self::width`]: a code's value does not tell you how
    /// many bytes it occupied, and every byte-range calculation needs that.
    pub(crate) code: u32,
    /// How many bytes this code occupied in the operand string.
    ///
    /// 1 for a simple font, 2 for `Identity-H`. Carried per slot rather
    /// than per run because it is what the byte-range arithmetic in
    /// [`match_run`] consumes, and deriving it there from the font would
    /// mean re-answering a question the decode already answered — the kind
    /// of duplicated predicate that drifts (R92).
    pub(crate) width: u8,
    /// Index into the operator's [`ShowElem`] list.
    pub(crate) elem: usize,
    /// Byte offset of this code within that element's string.
    pub(crate) byte_in_elem: usize,
    /// Byte range of this code's characters within the decoded `text`.
    pub(crate) t0: usize,
    pub(crate) t1: usize,
}

/// A recorded show operator with its full text state.
///
/// `pub(crate)` so the Pass 14.2 formatting surgery can read the same
/// recorded text-state (font resource, size, spacing, MCID) the Pass 14.1
/// REPLACE surgery reads, plus the fill colour 14.2 additionally needs.
#[derive(Debug, Clone)]
pub(crate) struct ShowData {
    pub(crate) font_name: Vec<u8>,
    pub(crate) tf_size: f64,
    /// The ambient §9.3 text state at this operator, **with each
    /// parameter's restore provenance** (Pass 19.0).
    ///
    /// This replaced three bare `f64`s (`tc`/`tw`/`th`). The values are
    /// still reachable through [`Self::tc`]/[`Self::tw`]/[`Self::th`] for
    /// the §9.4.4 advance arithmetic, but the struct now additionally
    /// knows how to *put each one back* — which is what a formatting
    /// surgery that emits `Tc`/`Tz`/`Ts` for one run needs, and what R88's
    /// three-tier ladder is expressed in. It also covers `Ts` and `Tr`,
    /// which this walk did not track at all before.
    pub(crate) text_state: AmbientTextState,
    pub(crate) mcid: Option<i64>,
    pub(crate) op: ShowOp,
    pub(crate) elems: Vec<ShowElem>,
    pub(crate) text: String,
    pub(crate) slots: Vec<ShowSlot>,
    /// The fill colour in effect (§8.6.8) — recorded for Pass 14.2's
    /// colour-restore; unused by Pass 14.1's REPLACE.
    pub(crate) fill_color: FillState,
    /// The **stroking** colour in effect (§8.6.8) — recorded for Pass
    /// 19.2's synthetic bold, which paints in text rendering mode 2 and
    /// must therefore both *set* the stroking colour (to match the fill,
    /// §9.3.6) and put the previous one back.
    pub(crate) stroke_color: FillState,
    /// The line width in effect (§8.4.3.2) — recorded for the same reason:
    /// a stroked-text width is the ordinary user-space line width, shared
    /// with path stroking, so it must be restored.
    pub(crate) line_width: LineWidth,
    /// The text matrix `Tm` in force at the **start** of this show operator
    /// (§9.4.2), i.e. before any of its own glyphs have advanced it.
    ///
    /// Pass 19.2 needs this because synthetic italic is a **shear
    /// premultiplied into `Tm`**, and a shear can only be premultiplied
    /// into a matrix that is known. Nothing before 19.2 read the text
    /// matrix in the authoring path at all: 14.1's relayout works in
    /// *deltas* (add ΔA to a follower's `e`), which never requires knowing
    /// the absolute matrix.
    pub(crate) text_matrix: [f64; 6],
    /// Whether [`Self::text_matrix`] is trustworthy.
    ///
    /// The walk advances `Tm` across each show operator by the §9.4.4
    /// displacement of its glyphs, which requires resolving the font and
    /// its widths. When that is not possible — an unresolvable font
    /// resource, or a composite run this walk does not decode — the
    /// accumulated matrix silently stops tracking reality. Rather than
    /// publish a plausible-looking wrong matrix, the walk marks it
    /// **unknown** and any consumer that needs an absolute position
    /// refuses (rule 4: fuzzy, never sneaky). A `Tm`/`Td`/`TD`/`T*`
    /// operator re-establishes the matrix absolutely and clears the flag.
    pub(crate) matrix_known: bool,
    /// The CTM in force at the operator (§8.3.2).
    pub(crate) ctm: [f64; 6],
}

impl ShowData {
    /// `Tc` — character spacing in effect at this operator (§9.3.2).
    pub(crate) fn tc(&self) -> f64 {
        self.text_state.char_spacing.value
    }

    /// `Tw` — word spacing in effect at this operator (§9.3.3).
    pub(crate) fn tw(&self) -> f64 {
        self.text_state.word_spacing.value
    }

    /// `Th` — horizontal scaling as a **ratio** (`Tz` ÷ 100, §9.3.4), the
    /// form §9.4.4's displacement formula multiplies by.
    pub(crate) fn th(&self) -> f64 {
        self.text_state.h_scale.value / 100.0
    }
}

/// What one operator contributes to the relayout scan.
#[derive(Debug, Clone)]
pub(crate) enum Rec {
    Show(Box<ShowData>),
    /// An absolute `Tm` with its six operands.
    Tm([f64; 6]),
    /// A `Td` / `TD` next-line operator with its operands (`Pass 256.0`).
    ///
    /// Recorded rather than folded into [`Rec::Boundary`] because a producer
    /// that writes ONE glyph per show operator advances between them with
    /// an x-only `Td`, and both the cross-operator match and the follower
    /// re-spacing need to see (and rewrite) that step. Every consumer that
    /// treated `Boundary` as "the line ends here" still does — an x-only
    /// `Td` is a boundary for a single-operator match too — but the span
    /// path may look through it when `ty == 0`.
    Td {
        /// The horizontal operand.
        tx: f64,
        /// The vertical operand; non-zero means a new line.
        ty: f64,
        /// `true` for `TD` (which also sets the leading), `false` for `Td`.
        leading: bool,
    },
    /// A `T*`/`'`/`"` (or a malformed `Td`/`TD`) — a line boundary reflow
    /// does not cross.
    Boundary,
    /// `ET` — the end of a text object (§9.4.1).
    ///
    /// Recorded from Pass 19.2 onward because synthetic italic must know
    /// **where the current text object stops**. The shear is emitted as an
    /// injected `Tm`, and any `Tm` overwrites `Tlm` as well (§9.4.2 Table
    /// 108), so a later `Td`/`TD`/`T*` *in the same text object* would
    /// derive its line from pdfcer's injected matrix instead of the
    /// producer's. Past `ET` the question is moot: the next `BT` resets
    /// both matrices to the identity (Table 107), so nothing carries over.
    EndText,
    /// Anything else.
    Ignore,
}

/// One recorded operator: its byte span in the decoded buffer + its role.
pub(crate) struct OpRec {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) rec: Rec,
}

/// The text-state machine of the walk (a focused sibling of
/// `redact::Surgeon`, reusing the same `content` tokenizer and §9.4.4
/// advance model rather than a second interpreter).
pub(crate) struct Walk<'a> {
    doc: &'a DocumentView<'a>,
    resources: &'a Dict,
    font_cache: HashMap<Vec<u8>, Option<ExtractFont>>,
    /// The graphics-state subset this walk models. One struct rather than
    /// loose fields because `q`/`Q` save and restore **all** of it at once
    /// (§8.4.2), and a stack of loose fields is how a `Q` comes to restore
    /// four of five things.
    gs: GState,
    /// The `q` save stack (§8.4.4). Bounded like the extraction walk's, so
    /// a hostile stream of `q`s cannot grow it without limit.
    gs_stack: Vec<GState>,
    // marked-content stack (§14.6/§14.7) — the current /MCID is the top.
    mc_stack: Vec<Option<i64>>,
    /// The text matrix `Tm` (§9.4.2), maintained across the whole walk.
    ///
    /// Unlike the six §9.3 parameters this is **not** graphics state — it
    /// is not saved by `q` or restored by `Q` (Table 52 lists no text
    /// matrix), and it exists only between `BT` and `ET`. So it lives on
    /// the walk rather than inside [`GState`], and that distinction is
    /// load-bearing: putting it in `GState` would have made a `Q` restore a
    /// text matrix, which no conforming reader does.
    tm: [f64; 6],
    /// The line matrix `Tlm` (§9.4.2) — what `Td`/`TD`/`T*` translate from.
    tlm: [f64; 6],
    /// Whether [`Self::tm`] still reflects reality; see
    /// [`ShowData::matrix_known`].
    tm_known: bool,
    pub(crate) recs: Vec<OpRec>,
}

/// The graphics state this authoring walk tracks, as `q`/`Q` see it.
///
/// # Pass 19.0: why this became a struct with a stack behind it
///
/// The walk previously kept `font_name`/`tf_size`/`tc`/`tw`/`th`/`fill`/
/// `last_cs` as loose fields and had **no `q` or `Q` arm at all**. Every
/// one of those is graphics state (§8.4.2, Table 52 + §9.3), so a stream
/// that wrote
///
/// ```text
/// q  1 0 0 rg  0.5 Tc  BT (a) Tj ET  Q   BT (b) Tj ET
/// ```
///
/// left the model believing `(b)` was red with `0.5 Tc`, when every
/// conforming reader discards both at the `Q`. That mis-modelled ambient
/// is exactly what a formatting restore would have re-emitted — writing a
/// wrong `0.5 Tc` into a stream that did not have one. The fix is
/// structural: one struct, one stack, one place a `Q` can go wrong.
#[derive(Debug, Clone)]
struct GState {
    /// The `/Fn` resource name selected by the most recent `Tf` (§9.3.1).
    font_name: Vec<u8>,
    /// `Tfs` — the `Tf` size operand (§9.3.1).
    tf_size: f64,
    /// The six shared §9.3 parameters, with restore provenance.
    ambient: AmbientTextState,
    /// The fill colour (§8.6.8) — recorded for Pass 14.2's restore.
    fill: FillState,
    /// The raw bytes of the most recent `cs` (set-fill-colour-space)
    /// operator, so a following `sc`/`scn` can record a re-emittable
    /// `cs … scn` sequence for the `Other` case.
    last_cs: Option<Vec<u8>>,
    /// The **stroking** colour (§8.6.8) — Pass 19.2. A separate
    /// graphics-state entry from `fill`, set by the uppercase operators.
    stroke: FillState,
    /// The stroking analogue of `last_cs`: the most recent `CS`
    /// (set-stroking-colour-space) operator's bytes.
    last_cs_stroking: Option<Vec<u8>>,
    /// The line width (§8.4.3.2) — Pass 19.2, for the synthetic-bold
    /// stroke width restore.
    line_width: LineWidth,
    /// The current transformation matrix (§8.3.2), for placing a run on the
    /// page. The surgery never needs it; the typing preview does.
    ctm: [f64; 6],
}

impl GState {
    /// The state at the start of a content stream: no font selected, and
    /// every §9.3 parameter at its Table 105 initial value.
    fn initial() -> Self {
        Self {
            ctm: IDENTITY,
            font_name: Vec::new(),
            tf_size: 0.0,
            ambient: AmbientTextState::initial(),
            fill: FillState::Default,
            last_cs: None,
            stroke: FillState::Default,
            last_cs_stroking: None,
            line_width: LineWidth::Initial,
        }
    }
}

impl<'a> Walk<'a> {
    pub(crate) fn new(doc: &'a DocumentView<'a>, resources: &'a Dict) -> Self {
        Self {
            doc,
            resources,
            font_cache: HashMap::new(),
            gs: GState::initial(),
            gs_stack: Vec::new(),
            mc_stack: Vec::new(),
            tm: IDENTITY,
            tlm: IDENTITY,
            tm_known: true,
            recs: Vec::new(),
        }
    }

    /// Resolve a `/Font /<name>` resource to an [`ExtractFont`] (cached).
    fn font(&mut self, name: &[u8]) -> Option<ExtractFont> {
        if let Some(hit) = self.font_cache.get(name) {
            return hit.clone();
        }
        let resolved = resolve_font(self.doc, self.resources, name);
        self.font_cache.insert(name.to_vec(), resolved.clone());
        resolved
    }

    fn nums(op: &Operation<'_>) -> Vec<f64> {
        op.operands
            .iter()
            .filter_map(|t| match &t.kind {
                ContentTokenKind::Operand(o) => o.as_number(),
                _ => None,
            })
            .collect()
    }

    fn current_mcid(&self) -> Option<i64> {
        self.mc_stack.iter().rev().find_map(|m| *m)
    }

    /// Apply §9.4.2 Table 108's next-line rule: `Tlm_new = translate(tx, ty)
    /// × Tlm_old`, and `Tm = Tlm_new`.
    ///
    /// Note that the translation composes with the **line** matrix, not with
    /// the current text matrix — which is the whole reason a `Td` after a
    /// long run returns to the left margin instead of continuing from where
    /// the glyphs stopped. It is also why an injected `Tm` is dangerous:
    /// `Tm` overwrites `Tlm` too, so the *next* `Td` would translate from
    /// pdfcer's matrix rather than the producer's line origin.
    ///
    /// The matrix becomes known again here for the same reason it does at a
    /// `Tm`: the new value is derived from `Tlm`, which is only ever set
    /// absolutely (by `BT`, by `Tm`, or by a previous next-line), and never
    /// drifts with glyph advances.
    fn next_line(&mut self, tx: f64, ty: f64) {
        self.tlm = mat_mul([1.0, 0.0, 0.0, 1.0, tx, ty], self.tlm);
        self.tm = self.tlm;
        self.tm_known = true;
    }

    /// Advance `Tm` by one show operator's total horizontal displacement
    /// (§9.4.4), in the **unrotated text space** the displacement is defined
    /// in: `Tm_new = translate(tx, 0) × Tm_old`.
    ///
    /// `tx` is the sum over the operator's elements of
    /// `((w0 − Tj/1000)·Tfs + Tc + Tw)·Th` for each shown glyph, and
    /// `(−Tj/1000)·Tfs·Th` for each standalone `TJ` number ("since no glyph
    /// was painted", §9.4.3's implementation note — the `Tc`/`Tw` terms do
    /// **not** apply to a bare adjustment, and adding them is the classic
    /// way to make justified text drift).
    ///
    /// If the font could not be resolved, or the run is composite (this walk
    /// decodes only simple fonts), the displacement is unknowable here and
    /// the matrix is marked **unknown** rather than left silently stale.
    fn advance_matrix(&mut self, font: Option<&ExtractFont>, elems: &[ShowElem]) {
        let Some(font) = font.filter(|f| f.is_simple()) else {
            self.tm_known = false;
            return;
        };
        let p = self.gs.ambient.params();
        let mut tx = 0.0;
        for e in elems {
            match e {
                ShowElem::Str(bytes) => {
                    // This walker is byte-wise, so it is single-byte by
                    // construction — a composite run's advance is computed on
                    // the decoded-slot path, not here.
                    for &code in bytes {
                        tx += glyph_advance_with(
                            font,
                            u32::from(code),
                            self.gs.tf_size,
                            p.char_spacing,
                            p.word_spacing,
                            p.h_scale,
                            true,
                        );
                    }
                }
                ShowElem::Num(v) => {
                    tx += (-v / 1000.0) * self.gs.tf_size * p.h_scale;
                }
            }
        }
        self.tm = mat_mul([1.0, 0.0, 0.0, 1.0, tx, 0.0], self.tm);
    }

    /// Build a [`FillState::Device`] from a device fill operator's numeric
    /// operands and its raw byte span. The raw bytes (`buf[start..end]`) are
    /// kept for a byte-faithful restore (Pass 14.2); the components feed the
    /// narrowing decision only.
    fn device_fill(
        space: DeviceSpace,
        n: &[f64],
        buf: &[u8],
        start: usize,
        end: usize,
    ) -> FillState {
        FillState::Device {
            space,
            comps: n.to_vec(),
            raw: buf.get(start..end).map(<[u8]>::to_vec).unwrap_or_default(),
        }
    }

    /// Process one operation, updating text state and recording it.
    pub(crate) fn operation(&mut self, op: &Operation<'_>, buf: &[u8]) {
        let (start, end) = op_span(op);
        let Some(name) = op.operator_name(buf) else {
            self.recs.push(OpRec {
                start,
                end,
                rec: Rec::Ignore,
            });
            return;
        };
        let n = Self::nums(op);
        let rec = match name {
            // --- special graphics state (§8.4.4) ---
            //
            // Pass 19.0. These arms did not exist, which meant text state
            // and fill colour set inside a `q … Q` bracket leaked past the
            // `Q` in the model. See [`GState`]'s doc comment for the
            // worked example and why it matters to a restore.
            b"q" => {
                self.gs_stack.push(self.gs.clone());
                // A hostile stream of `q`s must not grow the stack without
                // bound; 256 is far past any real nesting and matches the
                // extraction walk's guard.
                if self.gs_stack.len() > 256 {
                    self.gs_stack.remove(0);
                }
                Rec::Ignore
            }
            b"Q" => {
                if let Some(prev) = self.gs_stack.pop() {
                    self.gs = prev;
                }
                Rec::Ignore
            }
            // §8.4.4 Table 56: `a b c d e f cm` sets CTM' = M × CTM.
            b"cm" => {
                if let [a, b, c, d, e, f] = Self::nums(op)[..] {
                    self.gs.ctm = mat_mul([a, b, c, d, e, f], self.gs.ctm);
                }
                Rec::Ignore
            }
            b"Tf" => {
                if let Some(fname) = op.operands.iter().find_map(|t| match &t.kind {
                    ContentTokenKind::Operand(Object::Name(nm)) => Some(nm.as_bytes().to_vec()),
                    _ => None,
                }) {
                    self.gs.font_name = fname;
                }
                if let Some(size) = n.last() {
                    self.gs.tf_size = *size;
                }
                Rec::Ignore
            }
            // --- text state (§9.3 Table 105) ---
            //
            // Pass 19.0: six operators, ONE update rule, shared with the
            // extraction and vector walks. `Ts` and `Tr` are new here —
            // this walk tracked neither, which is why pdfcer could not
            // restore an ambient rise or rendering mode it had never
            // observed (decision 019 §1.2). The raw operator bytes are
            // captured for the R88 tier-2 restore.
            b"Tc" | b"Tw" | b"Tz" | b"TL" | b"Ts" | b"Tr" => {
                let raw = buf.get(start..end).unwrap_or_default();
                self.gs.ambient.apply_operator(name, &n, raw);
                Rec::Ignore
            }
            // --- text object delimiters (§9.4.1 Table 107) ---
            //
            // Pass 19.2. `BT` "shall initialize the text matrix Tm and the
            // text line matrix Tlm to the identity matrix" — and NOTHING
            // else: it does not reset text state (§9.3's retention rule),
            // which is why the ambient ladder exists at all.
            b"BT" => {
                self.tm = IDENTITY;
                self.tlm = IDENTITY;
                self.tm_known = true;
                Rec::Ignore
            }
            b"ET" => Rec::EndText,
            b"Tm" => match n.as_slice() {
                [a, b, c, d, e, f] => {
                    let m = [*a, *b, *c, *d, *e, *f];
                    // "Tm shall set the text matrix AND the text line
                    // matrix" (Table 108) — both, absolutely, which also
                    // makes the matrix known again after any drift.
                    self.tm = m;
                    self.tlm = m;
                    self.tm_known = true;
                    Rec::Tm(m)
                }
                _ => Rec::Ignore,
            },
            // `TD` additionally "sets the leading parameter to -ty"
            // (§9.4.2 Table 108). Tracked so the ambient `TL` this walk
            // publishes is the value actually in force — but as
            // ObservedIndirect, because re-emitting the `TD` to restore it
            // would also move the line.
            b"TD" => {
                if let [tx, ty] = n.as_slice() {
                    self.gs
                        .ambient
                        .set_indirect(TextStateParam::Leading, -*ty, "TD");
                    self.next_line(*tx, *ty);
                    Rec::Td {
                        tx: *tx,
                        ty: *ty,
                        leading: true,
                    }
                } else {
                    Rec::Boundary
                }
            }
            b"Td" => {
                if let [tx, ty] = n.as_slice() {
                    self.next_line(*tx, *ty);
                    Rec::Td {
                        tx: *tx,
                        ty: *ty,
                        leading: false,
                    }
                } else {
                    Rec::Boundary
                }
            }
            // `T*` is "0 −TL Td" (Table 108). `TL` comes from the shared
            // ambient state, which is exactly why 19.0 had to track it.
            b"T*" => {
                self.next_line(0.0, -self.gs.ambient.leading.value);
                Rec::Boundary
            }
            // --- general graphics state (§8.4.3.2) ---
            //
            // Pass 19.2: the line width is not text state, but synthetic
            // bold sets it (stroked text takes its width from here, in
            // USER space, §9.3.6), so it must be restorable.
            b"w" => {
                if let Some(v) = n.first() {
                    self.gs.line_width = LineWidth::Observed {
                        value: *v,
                        raw: buf.get(start..end).unwrap_or_default().to_vec(),
                    };
                }
                Rec::Ignore
            }
            // --- STROKING colour (§8.6.8) ---
            //
            // The uppercase twins of the fill arms below. Text painting
            // ignored these before Pass 19.2 because rendering mode 0 does
            // not stroke; synthetic bold uses mode 2, which does.
            b"G" => {
                self.gs.stroke = Self::device_fill(DeviceSpace::Gray, &n, buf, start, end);
                self.gs.last_cs_stroking = None;
                Rec::Ignore
            }
            b"RG" => {
                self.gs.stroke = Self::device_fill(DeviceSpace::Rgb, &n, buf, start, end);
                self.gs.last_cs_stroking = None;
                Rec::Ignore
            }
            b"K" => {
                self.gs.stroke = Self::device_fill(DeviceSpace::Cmyk, &n, buf, start, end);
                self.gs.last_cs_stroking = None;
                Rec::Ignore
            }
            b"CS" => {
                self.gs.last_cs_stroking = buf.get(start..end).map(<[u8]>::to_vec);
                Rec::Ignore
            }
            b"SC" | b"SCN" => {
                let mut raw = Vec::new();
                if let Some(cs) = &self.gs.last_cs_stroking {
                    raw.extend_from_slice(cs);
                    raw.push(b' ');
                }
                if let Some(here) = buf.get(start..end) {
                    raw.extend_from_slice(here);
                }
                self.gs.stroke = FillState::Other { raw };
                Rec::Ignore
            }
            // Fill-colour graphics state (§8.6.8). Recorded so Pass 14.2's
            // formatting surgery can classify (device vs Other) and RESTORE
            // the prior colour byte-faithfully after a wrapped edit. Only
            // the lowercase (fill) operators matter here; `G`/`RG`/`K`/`SC`/
            // `SCN` set the STROKE colour, which text painting does not use
            // by default (§9.3.1 render mode 0). Recording these does not
            // change Pass 14.1's REPLACE output — it never reads the field.
            b"g" => {
                self.gs.fill = Self::device_fill(DeviceSpace::Gray, &n, buf, start, end);
                self.gs.last_cs = None;
                Rec::Ignore
            }
            b"rg" => {
                self.gs.fill = Self::device_fill(DeviceSpace::Rgb, &n, buf, start, end);
                self.gs.last_cs = None;
                Rec::Ignore
            }
            b"k" => {
                self.gs.fill = Self::device_fill(DeviceSpace::Cmyk, &n, buf, start, end);
                self.gs.last_cs = None;
                Rec::Ignore
            }
            b"cs" => {
                // Set-fill-colour-space: remember the raw bytes so a
                // following `sc`/`scn` records a re-emittable `cs … scn`
                // restore sequence.
                self.gs.last_cs = buf.get(start..end).map(<[u8]>::to_vec);
                Rec::Ignore
            }
            b"sc" | b"scn" => {
                // A fill colour set in a resource-named space — pdfcer does
                // not decode it (TextColor::Other). Keep the raw operator
                // bytes (with the preceding `cs`, if any) to restore verbatim.
                let mut raw = Vec::new();
                if let Some(cs) = &self.gs.last_cs {
                    raw.extend_from_slice(cs);
                    raw.push(b' ');
                }
                if let Some(here) = buf.get(start..end) {
                    raw.extend_from_slice(here);
                }
                self.gs.fill = FillState::Other { raw };
                Rec::Ignore
            }
            b"BDC" | b"BMC" => {
                self.mc_stack.push(mcid_of(self.doc, op));
                Rec::Ignore
            }
            b"EMC" => {
                self.mc_stack.pop();
                Rec::Ignore
            }
            b"Tj" => self.record_show(op, ShowOp::Tj),
            b"TJ" => self.record_show(op, ShowOp::TJ),
            // `'` is "T* then Tj" (Table 109), so it moves to the next line
            // BEFORE showing — the matrix recorded on the show must be the
            // post-move one.
            b"'" => {
                self.next_line(0.0, -self.gs.ambient.leading.value);
                self.record_show(op, ShowOp::Quote)
            }
            b"\"" => {
                // Table 109: `"` sets `Tw` and `Tc` before showing. Routed
                // through the shared update rule so both are recorded with
                // the same provenance discipline as a standalone operator.
                let raw = buf.get(start..end).unwrap_or_default();
                self.gs.ambient.apply_operator(name, &n, raw);
                // `"` is `aw ac string "` ≡ set Tw/Tc, then `'` — so it too
                // moves to the next line before showing. The leading read
                // here is the value AFTER the Tw/Tc update, which is
                // correct: `"` does not touch `TL`.
                self.next_line(0.0, -self.gs.ambient.leading.value);
                self.record_show(op, ShowOp::DoubleQuote)
            }
            _ => Rec::Ignore,
        };
        self.recs.push(OpRec { start, end, rec });
    }

    /// Decode a show operator into text + slots under the current font.
    fn record_show(&mut self, op: &Operation<'_>, kind: ShowOp) -> Rec {
        let font = self.font(&self.gs.font_name.clone());
        let mut elems: Vec<ShowElem> = Vec::new();
        let mut text = String::new();
        let mut slots: Vec<ShowSlot> = Vec::new();

        // Collect operand elements (a string, or a TJ array of strings and
        // kerning numbers).
        let mut raw: Vec<ShowElem> = Vec::new();
        for t in op.operands {
            match &t.kind {
                ContentTokenKind::Operand(Object::String(s)) => raw.push(ShowElem::Str(s.clone())),
                ContentTokenKind::Operand(Object::Array(a)) => {
                    for item in a {
                        match item {
                            Object::String(s) => raw.push(ShowElem::Str(s.clone())),
                            other => {
                                if let Some(v) = other.as_number() {
                                    raw.push(ShowElem::Num(v));
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        // Decode each string element into codes (simple font = 1 byte/code).
        //
        // A composite font is NOT decoded here.
        //
        // This comment used to say "edit is refused later, R-INV-4". That is
        // FALSE, and the falsehood has an operator-facing cost. Because the
        // run is never decoded, the text is never located; because it is
        // never located, `classify_font` is never reached for it; so
        // R-INV-4's carefully-worded composite refusal NEVER FIRES from
        // `edit-text`. What the operator actually sees is "text to edit was
        // not found in an editable run on the page" — which reads as "your
        // text isn't there" when the truth is "it is there, in a font pdfcer
        // declines to edit". Those two lead to completely different next
        // actions, and only one of them is available.
        //
        // Found by trying to reach the R-INV-4 message through the CLI with
        // a composite fixture built for the purpose, and failing. Recorded
        // here rather than silently corrected because the fix is a change to
        // the LOCATION path — decode composite runs far enough to match, so
        // the right refusal can fire — and that belongs in its own slice
        // with its own tests, not bolted onto a comment repair.
        //
        // OWED (Pass 21.1): make a composite run locatable-but-refused
        // rather than invisible, so the specific refusal reaches the person
        // who has to act on it.
        let simple = font.as_ref().is_some_and(ExtractFont::is_simple);
        for (ei, elem) in raw.iter().enumerate() {
            match elem {
                ShowElem::Str(bytes) => {
                    if simple && let Some(f) = font.as_ref() {
                        for (bi, &byte) in bytes.iter().enumerate() {
                            // `TX-A1` PINNED — see the composite arm
                            // below for the reasoning; it applies
                            // identically to simple fonts.
                            let (chars, _) =
                                f.to_unicode(u32::from(byte), UnmappableCode::ReplacementChar);
                            let t0 = text.len();
                            text.push_str(&chars);
                            let t1 = text.len();
                            slots.push(ShowSlot {
                                code: u32::from(byte),
                                width: 1,
                                elem: ei,
                                byte_in_elem: bi,
                                t0,
                                t1,
                            });
                        }
                    } else if let Some(f) = font.as_ref() {
                        // COMPOSITE: decoded AND slotted (Pass 29.0).
                        //
                        // Slots were withheld here for two stated reasons.
                        // The first — "the re-encode and splice paths
                        // downstream are still single-byte, so a slot would be
                        // a handle nothing can use" — is now false: they take
                        // `u32` codes and splice raw bytes.
                        //
                        // The second was subtler and is worth recording,
                        // because it is why this could not simply be switched
                        // on. Giving composite runs slots WEAKENS the
                        // regression test that pins the classification
                        // ordering (`tests/composite_refusal_reachable.rs`).
                        // That test worked by asserting a composite edit does
                        // not surface `NoMatch` — which it could only do while
                        // `match_run` was guaranteed to fail for want of
                        // slots. With slots, `match_run` succeeds, so a broken
                        // ordering would still produce the refusal from
                        // `classify_font` and the test would pass on the bug
                        // it exists to catch.
                        //
                        // So that test is rewritten in this same change to use
                        // a font that is genuinely uneditable (no `/ToUnicode`
                        // at all), where the refusal is real rather than
                        // incidental — and the slots arrive here.
                        //
                        // Two bytes per code assumes `Identity-H`, which is
                        // what real-world composite text overwhelmingly uses
                        // and what pdfcer itself writes (Pass 21.0). A
                        // composite font on some other CMap decodes to nothing
                        // and stays unslotted, which is the old behaviour
                        // rather than a new regression.
                        //
                        for (pi, pair) in bytes.chunks_exact(2).enumerate() {
                            let [hi, lo] = pair else { continue };
                            let code = u32::from(*hi) << 8 | u32::from(*lo);
                            // `TX-A1` PINNED to the length-preserving
                            // sentinel, not the operator's extraction
                            // setting. This table maps CHARACTER offsets
                            // in `text` onto byte positions in the content
                            // stream, and `UnmappableCode::Omit` would
                            // give an unmappable code a zero-length span —
                            // a glyph the operator can see on the page and
                            // literally cannot address. Length
                            // preservation is load-bearing here in a way
                            // it is not in extraction output.
                            let (chars, _) = f.to_unicode(code, UnmappableCode::ReplacementChar);
                            let t0 = text.len();
                            text.push_str(&chars);
                            let t1 = text.len();
                            slots.push(ShowSlot {
                                code,
                                // TWO, and this is what makes `b_lo`/`b_hi`
                                // land on code boundaries rather than mid-CID.
                                width: 2,
                                elem: ei,
                                byte_in_elem: pi * 2,
                                t0,
                                t1,
                            });
                        }
                    }
                    elems.push(ShowElem::Str(bytes.clone()));
                }
                ShowElem::Num(v) => elems.push(ShowElem::Num(*v)),
            }
        }

        // Snapshot the matrices BEFORE this operator's own glyphs advance
        // them: `text_matrix` is defined as the matrix in force at the start
        // of the operator, which is what a shear must be premultiplied into.
        let at_start = ShowData {
            font_name: self.gs.font_name.clone(),
            tf_size: self.gs.tf_size,
            text_state: self.gs.ambient.clone(),
            mcid: self.current_mcid(),
            op: kind,
            elems,
            text,
            slots,
            fill_color: self.gs.fill.clone(),
            stroke_color: self.gs.stroke.clone(),
            line_width: self.gs.line_width.clone(),
            text_matrix: self.tm,
            matrix_known: self.tm_known,
            ctm: self.gs.ctm,
        };
        self.advance_matrix(font.as_ref(), &at_start.elems);
        Rec::Show(Box::new(at_start))
    }
}

// ===================================================================
// The public entry point
// ===================================================================

/// Edit the page's own text in place: locate the run, re-encode the new
/// text, relayout the line, and save incrementally.
///
/// Returns the appended PDF bytes plus an [`EditReport`] carrying every
/// disclosure. A character the run's font cannot provide yields
/// [`EditError::Refused`] BEFORE any save — the refusal never reaches the
/// writer (rule 4 / R71).
///
/// # Errors
///
/// See [`EditError`]: a named refusal, no match, an unsupported run, an
/// encrypted document, a parse/page-tree failure, or a save failure.
pub fn edit_text(
    doc: &Document,
    req: &EditRequest,
    opts: &EditOptions,
) -> Result<EditOutcome, EditError> {
    if crate::encryption_gate::forbids(doc, &[PermissionBit::ModifyContents]) {
        return Err(EditError::Encrypted);
    }
    let pages = page_tree::pages(doc)?;
    let page = pages
        .get(req.page_index)
        .ok_or(EditError::PageIndex(req.page_index))?;
    // BASE READ (decision 018 caller audit): `edit_text` is the one-shot
    // `&Document` entry point — it plans against the file as loaded (its
    // own view; there is no overlay here) and hands the plan to an
    // incremental save. The GUI's accumulating multi-edit path is
    // `EditSession::edit_text`, which plans against the session view.
    let (mut plan, target) = plan_edit_anywhere(&doc.view(), page, req, opts)?;
    // Incremental save (R34/R70). Which object gets rewritten now depends on
    // where the text was found (`Pass 119.0`): the page's first content
    // object, or the form XObject's own stream. Both are one-object rewrites
    // and both leave every other byte of the file verbatim.
    let mut next = doc.next_object_number();
    plan.font_writes.assign(|| {
        let n = next.ok_or(EditError::Unsupported(
            UnsupportedCause::ObjectNumbersExhausted,
        ))?;
        next = n.checked_add(1);
        Ok(ObjId::new(n, 0))
    })?;
    let (mut font_objects, mut staged) = plan.font_writes.staged(doc);
    let mut form_dict = target.form.as_ref().map(|f| f.dict.clone());
    if let Some(created) = &plan.created_font {
        let owner = target.form.as_ref().map_or(page.id, |f| f.id);
        let at = (owner, form_dict.as_mut());
        if fallback::bind_one_shot(doc, created, at, &mut next, &mut staged, &mut font_objects)? {
            let note = fallback::SHARED_RESOURCES_NOTE.to_owned();
            plan.report.disclosures.push(note);
        }
    }
    let bytes = match target.form.as_ref().zip(form_dict.as_ref()) {
        Some((form, form_dict)) => write_incremental_form_with(
            doc,
            form.id,
            form_dict,
            &plan.new_content,
            &font_objects,
            staged,
        )?,
        // The plan derives content_object / extra_objects_emptied from
        // `page.contents`; a decoupled write went elsewhere, so it overrides.
        None => {
            let (bytes, content_object, emptied, decoupled) =
                write_incremental_with(doc, page, &plan.new_content, &font_objects, staged)?;
            if decoupled {
                plan.report.content_object = content_object;
                plan.report.extra_objects_emptied = emptied;
                plan.report
                    .disclosures
                    .push(SHARED_CONTENT_DISCLOSURE.to_owned());
            }
            bytes
        }
    };
    Ok(EditOutcome {
        bytes,
        report: plan.report,
    })
}

/// The result of planning an edit WITHOUT committing it: the fully-spliced
/// replacement content-stream buffer plus the complete [`EditReport`].
///
/// This is the seam (Pass 14.3 UI spec §0.2) that lets the interactive
/// [`EditSession::edit_text`](crate::edit::EditSession::edit_text) reuse the
/// EXACT locate/re-encode/relayout/gate logic of the free-function
/// [`edit_text`] while landing the mutation as one undo-able command against
/// the session's in-memory object graph, instead of producing already-saved
/// bytes. `content_object` and `extra_objects_emptied` are derived from
/// `page.contents` (not from a save), so the report is complete before any
/// write happens — both the free function and the session path then perform
/// their own write step (`write_incremental` vs. session staging + command).
pub(crate) struct EditPlan {
    /// The spliced, decoded replacement content for the page's first content
    /// object (the whole page content, edited).
    pub(crate) new_content: Vec<u8>,
    /// The complete disclosure/diagnostic report.
    pub(crate) report: EditReport,
    /// Where the replacement's glyphs land.
    pub(crate) layout: EditLayout,
    /// Objects the edit adds or revises besides the content stream (a font
    /// dictionary extended under decision 172), written in the same revision.
    pub(crate) font_writes: FontWrites,
    /// [`narrow_span`]'s trim: the byte range of the request's `find`
    /// replaced and its replacement text; `None` when not narrowed.
    pub(crate) rewritten: Option<(std::ops::Range<usize>, String)>,
    /// Decision 173's new program, decoded, when the edit replaces the font's.
    pub(crate) font_program: Option<Vec<u8>>,
    /// Pass 431.0: the fallback face's `/Font` resource, when the edit must
    /// create it; bound into the target's resources in the same revision.
    pub(crate) created_font: Option<crate::text_edit::format::CreatedFont>,
}

/// A decision 172 extension's writes: replaced objects, and streams rewritten
/// in place as `(id, dictionary, unfiltered bytes)`.
#[derive(Debug, Clone, Default)]
pub(crate) struct FontWrites {
    pub(crate) objects: Vec<(ObjId, Object)>,
    pub(crate) streams: Vec<(ObjId, Dict, Vec<u8>)>,
    /// Decision 173: the writes use placeholder ids until [`Self::assign`].
    fresh: bool,
    /// The descriptor and program the augmented font used before.
    pub(crate) superseded: Option<(ObjId, ObjId)>,
}

impl FontWrites {
    fn of(extension: Option<crate::text_edit::font_extend::FontExtension>) -> Self {
        let Some(e) = extension else {
            return Self::default();
        };
        let mut w = Self {
            objects: e.write().into_iter().collect(),
            streams: e.to_unicode.clone().into_iter().collect(),
            ..Self::default()
        };
        if let Some(a) = e.augmented {
            use crate::text_edit::augment_route::{FRESH_DESCRIPTOR, FRESH_PROGRAM};
            w.objects
                .push((FRESH_DESCRIPTOR, Object::Dict(a.descriptor)));
            w.streams.push((FRESH_PROGRAM, a.stream_dict, a.encoded));
            w.fresh = true;
            w.superseded = a.superseded;
        }
        w
    }

    /// Give decision 173's new descriptor and program real numbers, the
    /// descriptor's first.
    ///
    /// # Errors
    ///
    /// `alloc`'s error when it has no number left.
    pub(crate) fn assign(
        &mut self,
        mut alloc: impl FnMut() -> Result<ObjId, EditError>,
    ) -> Result<(), EditError> {
        use crate::text_edit::augment_route::{FRESH_DESCRIPTOR, FRESH_PROGRAM, renumber};
        if !std::mem::take(&mut self.fresh) {
            return Ok(());
        }
        let map = [(FRESH_DESCRIPTOR, alloc()?), (FRESH_PROGRAM, alloc()?)];
        renumber(&mut self.objects, &map);
        for (id, _, _) in &mut self.streams {
            if let Some(&(_, n)) = map.iter().find(|(p, _)| p == id) {
                *id = n;
            }
        }
        Ok(())
    }

    /// The objects with every stream staged after the base file, and the
    /// staging buffer to hand [`write_incremental_with`].
    fn staged(&self, doc: &Document) -> (Vec<(ObjId, Object)>, Vec<u8>) {
        let (base_len, mut staging) = (doc.bytes().len(), Vec::new());
        let mut objects = self.objects.clone();
        for (id, dict, bytes) in &self.streams {
            let span = stage(&mut staging, base_len, bytes);
            let dict = dict.clone();
            objects.push((
                *id,
                Object::Stream(Stream {
                    dict,
                    data_span: span,
                }),
            ));
        }
        (objects, staging)
    }
}

/// A replacement laid out exactly as `crate::EditSession::edit_text` would
/// commit it, with nothing written (`Pass 366.0`).
///
/// Returned by `crate::EditSession::edit_text_preview`. The glyph codes,
/// positions and refusals come from the same plan the commit runs, so a
/// shell drawing these glyphs while the operator types shows what Enter
/// will produce. `pdfcer_render::edit_preview` turns it into outlines.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct TextEditPreview {
    /// The page the run is on.
    pub page_index: usize,
    /// The font resource name (the `Tf` operand) the replacement is set in:
    /// the run's own, or a same-face sibling under
    /// [`EditOptions::sibling_fonts`].
    pub font_resource: Vec<u8>,
    /// That font's dictionary, as the session sees it.
    pub font: Dict,
    /// `/BaseFont`, verbatim.
    pub base_font: String,
    /// One entry per code of the replacement, in order.
    pub glyphs: Vec<PreviewGlyph>,
    /// The replacement's advance box `[x0, y0, x1, y1]` in page user space
    /// (points, y up): the width the commit will occupy, and the font's
    /// ascent to descent (§9.8 Table 122).
    pub bbox: [f64; 4],
    /// The run's non-stroking colour.
    pub fill: PreviewColour,
    /// The run's stroking colour (text render modes 1, 2, 5, 6).
    pub stroke: PreviewColour,
    /// The run's text rendering mode `Tr` (§9.3.6 Table 106).
    pub render_mode: i64,
    /// What the commit's [`EditReport::disclosures`] would say.
    pub disclosures: Vec<String>,
    /// The part of the request laid out, when the edit was narrowed to it:
    /// the byte range of the request's `find` that is replaced and the
    /// replacement text for it. `glyphs` and `bbox` cover that text only.
    /// `None` when the whole `replace` was laid out.
    ///
    /// A match spanning several show operators is trimmed to what differs:
    /// the common prefix and suffix of `find` and `replace` are left as the
    /// producer placed them, keeping at least one `find` character (the
    /// `span:` disclosure). Appending `_` to `"ab "` gives `Some((2..3, " _"))`.
    pub rewritten: Option<(std::ops::Range<usize>, String)>,
    /// The decoded font program the commit would embed in place of the
    /// document's, when the edit augments a subset from an installed face
    /// (decision 173, [`EditOptions::with_subset_augment`]). `font`'s
    /// `/FontDescriptor` is then the new descriptor, inline, whose
    /// `/FontFile2` does not resolve until the commit; draw from these bytes.
    /// `None` when the document's own program is used.
    pub font_program: Option<Vec<u8>>,
    /// The face the characters the run's font cannot take are set in, under
    /// [`EditOptions::fallback`]; the glyphs with
    /// [`PreviewGlyph::fallback`] set are drawn from it. `None` otherwise.
    pub fallback: Option<PreviewFallback>,
}

/// One glyph of a [`TextEditPreview`].
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct PreviewGlyph {
    /// The character typed, when the code maps back to one.
    pub ch: Option<char>,
    /// The character code the commit will write (§9.4.3).
    pub code: u32,
    /// Glyph space in text-space units (one unit = one em at size 1) to page
    /// user space: `[Tfs×Th 0 0 Tfs x Trise] × Tm × CTM` (§9.4.4). A font
    /// program's outline, scaled by `1/unitsPerEm`, lands on the page
    /// through this matrix.
    pub matrix: [f64; 6],
    /// Whether the glyph is set in [`TextEditPreview::fallback`]'s face
    /// rather than the run's font; `code` is then that face's code.
    pub fallback: bool,
}

/// A run's colour as its operators set it (§8.6.4).
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum PreviewColour {
    /// DeviceGray, one component in `0..=1`. Also the initial colour.
    Gray(f64),
    /// DeviceRGB.
    Rgb([f64; 3]),
    /// DeviceCMYK.
    Cmyk([f64; 4]),
    /// Any other space (ICC-based, Separation, a pattern…); the operator
    /// bytes that set it.
    Other(Vec<u8>),
}

impl PreviewColour {
    fn from_state(state: &FillState) -> Self {
        match state {
            FillState::Default => Self::Gray(0.0),
            FillState::Device { space, comps, raw } => match (space, comps.as_slice()) {
                (DeviceSpace::Gray, [g]) => Self::Gray(*g),
                (DeviceSpace::Rgb, [r, g, b]) => Self::Rgb([*r, *g, *b]),
                (DeviceSpace::Cmyk, [c, m, y, k]) => Self::Cmyk([*c, *m, *y, *k]),
                _ => Self::Other(raw.clone()),
            },
            FillState::Other { raw } => Self::Other(raw.clone()),
        }
    }
}

impl TextEditPreview {
    pub(crate) fn new(
        page_index: usize,
        layout: EditLayout,
        disclosures: Vec<String>,
        rewritten: Option<(std::ops::Range<usize>, String)>,
        font_program: Option<Vec<u8>>,
    ) -> Self {
        let (fallback, flags) = layout.fallback.unzip();
        let flags = flags.unwrap_or_default();
        Self {
            page_index,
            font_resource: layout.font_name,
            font: layout.font_dict,
            base_font: layout.base_font,
            glyphs: layout
                .glyphs
                .into_iter()
                .enumerate()
                .map(|(i, (ch, code, matrix))| PreviewGlyph {
                    ch,
                    code,
                    matrix,
                    fallback: flags.get(i).copied().unwrap_or(false),
                })
                .collect(),
            bbox: layout.bbox,
            fill: PreviewColour::from_state(&layout.fill),
            stroke: PreviewColour::from_state(&layout.stroke),
            // `Tr` is an integer operand (Table 106); 0 when unset.
            #[allow(clippy::cast_possible_truncation)] // 0..=7 by §9.3.6
            render_mode: layout.render_mode as i64,
            disclosures,
            rewritten,
            font_program,
            fallback,
        }
    }
}

/// The replacement text as the edit lays it out: one entry per code, in the
/// anchor run's own font, text state and matrices.
#[derive(Debug, Clone)]
pub(crate) struct EditLayout {
    pub(crate) font_name: Vec<u8>,
    pub(crate) font_dict: Dict,
    pub(crate) base_font: String,
    /// `(char, code, glyph matrix)`; the matrix maps glyph space in text-space
    /// units (one unit = one em at size 1) to page user space.
    pub(crate) glyphs: Vec<(Option<char>, u32, [f64; 6])>,
    /// The advance box `[x0, y0, x1, y1]` in page user space.
    pub(crate) bbox: [f64; 4],
    pub(crate) fill: FillState,
    pub(crate) stroke: FillState,
    pub(crate) render_mode: f64,
    /// Pass 431.0: the fallback face, and per glyph whether it is set in it.
    pub(crate) fallback: Option<(PreviewFallback, Vec<bool>)>,
}

impl EditLayout {
    fn new(
        anchor: &ShowData,
        font_dict: &Dict,
        font: &ExtractFont,
        replace: &str,
        codes: &[u32],
        origin_x: f64,
    ) -> Self {
        let mut chars = replace.chars();
        let items = codes
            .iter()
            .map(|&code| (chars.next(), code, glyph_advance(font, code, anchor)));
        let metrics = (f64::from(font.ascent()), f64::from(font.descent()));
        Self::placed(anchor, font_dict, &font.base_font, items, origin_x, metrics)
    }

    /// Glyphs `(char, code, advance)` placed from `origin_x` along the
    /// anchor's line, boxed by `(ascent, descent)` per unit size.
    pub(crate) fn placed(
        anchor: &ShowData,
        font_dict: &Dict,
        base_font: &str,
        items: impl Iterator<Item = (Option<char>, u32, f64)>,
        origin_x: f64,
        (asc, desc): (f64, f64),
    ) -> Self {
        // §9.4.4: Trm = [Tfs×Th 0 0 Tfs 0 Trise] × Tm × CTM, with Tm advanced
        // by each glyph's displacement along the line.
        let tfs = anchor.tf_size;
        let th = anchor.th();
        let rise = anchor.text_state.rise.value;
        let line = mat_mul(anchor.text_matrix, anchor.ctm);
        let mut x = origin_x;
        let mut glyphs = Vec::new();
        for (ch, code, advance) in items {
            let m = mat_mul([tfs * th, 0.0, 0.0, tfs, x, rise], line);
            glyphs.push((ch, code, m));
            x += advance;
        }
        // The box spans the advance along the line and the font's
        // ascent/descent (§9.8 Table 122, text space per unit size).
        let corners = [
            (origin_x, desc * tfs + rise),
            (x, desc * tfs + rise),
            (origin_x, asc * tfs + rise),
            (x, asc * tfs + rise),
        ];
        let mut bbox = [
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ];
        for (cx, cy) in corners {
            let px = cx * line[0] + cy * line[2] + line[4];
            let py = cx * line[1] + cy * line[3] + line[5];
            bbox = [
                bbox[0].min(px),
                bbox[1].min(py),
                bbox[2].max(px),
                bbox[3].max(py),
            ];
        }
        Self {
            font_name: anchor.font_name.clone(),
            font_dict: font_dict.clone(),
            base_font: base_font.to_owned(),
            glyphs,
            bbox,
            fill: anchor.fill_color.clone(),
            stroke: anchor.stroke_color.clone(),
            render_mode: anchor.text_state.render_mode.value,
            fallback: None,
        }
    }
}

/// The stream an edit is being planned against, with everything the planner
/// needs that used to come straight off the [`Page`] (`Pass 119.0`).
///
/// # Why the planner stopped taking a `&Page`
///
/// Every use of the page inside `plan_edit` was one of three things: *which
/// object do I rewrite*, *how many sibling content streams get collapsed*, and
/// *which resource dictionary do names resolve in*. None of those is
/// page-specific — they are properties of **the buffer being edited** — and
/// hard-coding them to the page is precisely what made form content
/// unreachable. Naming them explicitly makes the page the *default* target
/// rather than the *only* one, and leaves the surgery itself completely
/// unchanged: the advance arithmetic, the re-encode, the follower disposition
/// and every refusal work on operators, not on pages.
pub(crate) struct EditPlanTarget {
    /// The stream object the spliced buffer replaces.
    pub(crate) content_id: ObjId,
    /// Sibling `/Contents` streams to empty (page target only; a form is one
    /// stream by construction).
    pub(crate) extra_emptied: u64,
    /// The effective resource dictionary for names inside this buffer.
    pub(crate) resources: Dict,
    /// The form being edited, when this is a form target. Carries the form's
    /// dictionary so the planner can apply the form-specific refusals without
    /// re-resolving anything.
    pub(crate) form: Option<crate::text_edit::forms::FormRef>,
    /// The form's document-wide invocation set, when this is a form target.
    pub(crate) invocations: Option<crate::text_edit::forms::InvocationSet>,
}

impl EditPlanTarget {
    /// The page's own `/Contents` — the pre-`Pass 119.0` behaviour, unchanged.
    ///
    /// # Errors
    ///
    /// [`EditError::Unsupported`] when the page has no `/Contents` at all.
    /// Checked here rather than at save time so the refusal precedes any
    /// mutation, matching `write_incremental`'s own first check.
    pub(crate) fn page(page: &Page) -> Result<Self, EditError> {
        let content_id = *page
            .contents
            .first()
            .ok_or(EditError::Unsupported(UnsupportedCause::NoContents))?;
        Ok(Self {
            content_id,
            extra_emptied: page.contents.len().saturating_sub(1) as u64,
            resources: page.resources.clone(),
            form: None,
            invocations: None,
        })
    }

    /// A form XObject's own content stream.
    pub(crate) fn form(
        form: crate::text_edit::forms::FormRef,
        invocations: crate::text_edit::forms::InvocationSet,
    ) -> Self {
        Self {
            content_id: form.id,
            extra_emptied: 0,
            resources: form.resources.clone(),
            form: Some(form),
            invocations: Some(invocations),
        }
    }
}

/// Plan a REPLACE edit against an explicitly-named target stream.
///
/// The body is `plan_edit`'s, with the three page-derived values taken from
/// [`EditPlanTarget`] and the form-specific refusal/disclosure block added.
///
/// # Errors
///
/// See [`EditError`].
pub(crate) fn plan_edit_target(
    doc: &DocumentView<'_>,
    target: &EditPlanTarget,
    stream: &ContentStream,
    req: &EditRequest,
    opts: &EditOptions,
) -> Result<EditPlan, EditError> {
    let recs = walk_records(doc, &target.resources, stream);
    plan_edit_with_records(doc, target, stream, &recs, req, opts, PlanMode::Commit)
}

/// Pass 1 of the planner: every operator of `stream` with its text state.
///
/// Separate so the session can cache the records per page: on a large CAD
/// page this walk is ~200 ms, the rest of the plan a few.
pub(crate) fn walk_records(
    doc: &DocumentView<'_>,
    resources: &Dict,
    stream: &ContentStream,
) -> Vec<OpRec> {
    let mut walk = Walk::new(doc, resources);
    for op in stream.operations() {
        walk.operation(&op, &stream.buf);
    }
    walk.recs
}

/// Whether [`plan_edit_with_records`] builds the new content or only the
/// layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlanMode {
    /// Splice the edit into a new content buffer.
    Commit,
    /// Everything but the splice: the gates, the report and the layout.
    /// `EditPlan::new_content` comes back empty.
    Preview,
}

/// [`plan_edit_target`] over already-walked `recs` (from [`walk_records`]
/// over the same `stream` and `target.resources`).
///
/// # Errors
///
/// See [`EditError`].
pub(crate) fn plan_edit_with_records(
    doc: &DocumentView<'_>,
    target: &EditPlanTarget,
    stream: &ContentStream,
    recs: &[OpRec],
    req: &EditRequest,
    opts: &EditOptions,
    mode: PlanMode,
) -> Result<EditPlan, EditError> {
    // Form refusals come before any surgery, so a refused edit costs nothing.
    if let Some(form) = target.form.as_ref() {
        refuse_unsuitable_form(form)?;
    }
    let span = find_anchor_span(recs, req)?;
    // A span edit touches only the part of the match that actually changes,
    // so the producer's own positioning of the unchanged glyphs survives.
    let narrow = narrow_span(recs, span, req);
    let (span, req) = narrow.as_ref().map_or((span, req), |(s, r, _)| (*s, r));
    let rewritten = narrow
        .as_ref()
        .map(|(_, r, range)| (range.clone(), r.replace.clone()));
    let at = locate(recs, span, &req.find)?;
    let anchor = at.anchor;
    let (font_dict, font, class) = anchor_font(doc, target, anchor)?;
    // An empty `find` on a PINNED request means the whole operator.
    let find = effective_find(anchor, &req.find, req.pinned_span);
    let (m, leading_matches) = match_anchor(&at, span, find)?;
    let single = !at.crossed && leading_matches.is_empty();
    let enc = encode_with_sibling(
        doc, target, recs, font, &class, font_dict, anchor, req, opts, single,
    )?;
    let font_dict = enc.extension.as_ref().map_or(enc.dict, FontExtension::view);
    let run_font = enc.sibling.as_ref().map_or(&enc.font, |s| &s.1);
    // Advance delta (§9.4.4): the anchor's matched glyphs plus any TJ kerns
    // the match swallowed, against the whole replacement, which lands there.
    let a_new = enc.advance(anchor);
    let laying = Laying {
        recs,
        font: run_font,
        glyph_font: &enc.font,
        switch: enc.sibling.as_ref().map(|s| s.0.as_slice()),
        fallback: enc.fallback.as_ref(),
        font_dict,
        anchor,
        anchor_index: at.anchor_index,
        anchor_bytes: at.anchor_bytes,
        m: &m,
        others: &leading_matches,
        replace: &req.replace,
        encoded: &enc.encoded,
        disposition: opts.disposition,
        a_new,
        a_old_last: codes_advance(run_font, &m.old_codes, anchor) + m.kern_advance,
    };
    let mut laid = lay(&laying, &at);
    let new_content = match mode {
        PlanMode::Commit => splice(&stream.buf, &mut laid.edits),
        PlanMode::Preview => Vec::new(),
    };
    // The report only; the caller performs its own write step.
    let mut disclosures = laid_notes(enc.encoded.disclosures, &mut laid, rewritten.is_some());
    disclosures.extend(general_disclosures(
        req, opts, anchor, find, &enc.font, &class,
    ));
    target_disclosures(doc, target, anchor, &mut disclosures);
    // Show operators only; the `Td` steps between them were not written across.
    let moved = (laid.delta, laid.followers, leading_matches.len() as u64 + 1);
    let mut report = edit_report(target, &enc.font, &class, opts, moved, anchor, disclosures);
    report.fallback = enc.fallback.as_ref().map(|f| f.used.clone());
    Ok(EditPlan {
        new_content,
        report,
        layout: laid.layout,
        font_program: FontExtension::program_of(enc.extension.as_ref()),
        font_writes: FontWrites::of(enc.extension),
        rewritten,
        created_font: enc.fallback.and_then(Fallback::into_created),
    })
}

/// The encoder's disclosures, then the layout's and the narrowing note.
fn laid_notes(mut notes: Vec<String>, laid: &mut Laid, narrowed: bool) -> Vec<String> {
    let narrowed = narrowed.then(|| NARROWED_NOTE.to_owned());
    notes.extend(
        [laid.span_note.take(), narrowed, laid.td_note.take()]
            .into_iter()
            .flatten(),
    );
    notes
}

impl Encoding<'_> {
    /// The replacement's advance (§9.4.4), each glyph in the font it is set in.
    fn advance(&self, anchor: &ShowData) -> f64 {
        self.fallback.as_ref().map_or_else(
            || codes_advance(&self.font, &self.encoded.codes, anchor),
            |fb| fb.advance(&self.font, anchor),
        )
    }
}

/// The summed advance of `codes` in `f` (§9.4.4).
fn codes_advance(f: &ExtractFont, codes: &[u32], anchor: &ShowData) -> f64 {
    codes.iter().map(|&c| glyph_advance(f, c, anchor)).sum()
}

/// Resolve and classify the anchor's font.
///
/// Classification comes BEFORE matching: a font-level refusal (R-INV-2/3/4)
/// is a property of the run, and reporting `NoMatch` for text present in a
/// refused font would tell the operator it is absent. The font resolves
/// against `target.resources`: inside a form XObject `/F1` can name a
/// different font dictionary (§8.10.1).
///
/// # Errors
///
/// [`UnsupportedCause::FontUnresolvable`], or a [`classify_font`] refusal.
fn anchor_font<'a>(
    doc: &'a DocumentView<'a>,
    target: &'a EditPlanTarget,
    anchor: &ShowData,
) -> Result<(&'a Dict, ExtractFont, FontClass), EditError> {
    let font_dict = resolve_font_dict(doc, &target.resources, &anchor.font_name)
        .ok_or(EditError::Unsupported(UnsupportedCause::FontUnresolvable))?;
    let font = ExtractFont::resolve(doc, font_dict);
    let class = classify_font(doc, font_dict, &font)?;
    Ok((font_dict, font, class))
}

/// The disclosure for an edit narrowed to the part of the find text that
/// differs from the replacement.
const NARROWED_NOTE: &str = "span: the start and end of the find text matched the replacement, so only the part that differs was rewritten — the unchanged glyphs keep the producer's own spacing.";

/// The anchor's match and the span's other operators' matches.
type Matched<'a> = (MatchRun, Vec<(usize, &'a ShowData, MatchRun)>);

/// The anchor operator of a match and the span's other operators.
struct Located<'a> {
    /// Record index of the operator that receives the replacement: the FIRST
    /// of a match crossing text objects (`cross_object`), otherwise the last.
    anchor_index: usize,
    /// The anchor's byte range in the content buffer.
    anchor_bytes: (usize, usize),
    anchor: &'a ShowData,
    /// Whether the span was joined across `ET`.
    crossed: bool,
    /// The span's operators other than the anchor, each with the character
    /// range of the match inside it (in its own text). Empty for a
    /// single-operator match. Every operator of a span shares the font
    /// resource by the grouping rule, so the anchor's classification holds
    /// for all of them.
    leading_ops: Vec<(usize, &'a ShowData, usize, usize)>,
    /// Where the anchor's text starts within the span's joined text.
    last_offset: usize,
}

/// Find the anchor operator of `span` and split the match across its
/// operators.
///
/// # Errors
///
/// [`EditError::NoMatch`] when the anchor record is not a show operator;
/// [`UnsupportedCause::QuoteOperator`] for a `'`/`"` anchor.
fn locate<'a>(recs: &'a [OpRec], span: Anchor, find: &str) -> Result<Located<'a>, EditError> {
    let crossed = cross_object::crosses(recs, span.first, span.last);
    let anchor_index = if crossed { span.first } else { span.last };
    let Some(OpRec {
        start,
        end,
        rec: Rec::Show(anchor),
    }) = recs.get(anchor_index)
    else {
        return Err(EditError::no_match(find.to_owned()));
    };
    let show = |k: usize| match recs.get(k) {
        Some(OpRec {
            rec: Rec::Show(s), ..
        }) => Some(&**s),
        _ => None,
    };
    let mut leading_ops = Vec::new();
    if span.first != span.last {
        let mut offset = 0usize;
        for k in span.first..=span.last {
            let Some(s) = show(k) else { continue };
            let lo = span.pos.saturating_sub(offset).min(s.text.len());
            let hi = span.end.saturating_sub(offset).min(s.text.len());
            if k != anchor_index && hi > lo {
                leading_ops.push((k, s, lo, hi));
            }
            offset += s.text.len();
        }
    }
    let last_offset = (span.first..anchor_index)
        .filter_map(show)
        .map(|s| s.text.len())
        .sum();
    if matches!(anchor.op, ShowOp::Quote | ShowOp::DoubleQuote) {
        return Err(EditError::Unsupported(UnsupportedCause::QuoteOperator));
    }
    Ok(Located {
        anchor_index,
        anchor_bytes: (*start, *end),
        anchor,
        crossed,
        leading_ops,
        last_offset,
    })
}

/// Map `find` to a contiguous code range in the anchor, and in each of the
/// span's other operators.
///
/// `match_range` rather than `match_run`: inside one operator a match may
/// cross TJ elements (`[(cli) -20 (en)] TJ`).
///
/// # Errors
///
/// [`UnsupportedCause::EmptyFind`] for an empty single-operator find;
/// [`EditError::NoMatch`] when the text is not at the span's position.
fn match_anchor<'a>(at: &Located<'a>, span: Anchor, find: &str) -> Result<Matched<'a>, EditError> {
    let anchor = at.anchor;
    let m = if span.first == span.last {
        if find.is_empty() {
            return Err(EditError::Unsupported(UnsupportedCause::EmptyFind));
        }
        // `span.pos`, not a fresh search: a narrowed span names ONE
        // occurrence, and the first one in the operator may be another.
        if !anchor
            .text
            .get(span.pos..)
            .is_some_and(|t| t.starts_with(find))
        {
            return Err(EditError::no_match(find.to_owned()));
        }
        match_range(anchor, span.pos, span.pos + find.len(), find)?
    } else if at.crossed {
        // The match runs from `span.pos` to the end of the first operator.
        match_range(anchor, span.pos, anchor.text.len(), find)?
    } else {
        // In the last operator the match starts at character 0 and ends
        // where the span says.
        match_range(anchor, 0, span.end.saturating_sub(at.last_offset), find)?
    };
    let leading = at
        .leading_ops
        .iter()
        .map(|(k, s, lo, hi)| match_range(s, *lo, *hi, find).map(|mr| (*k, *s, mr)))
        .collect::<Result<_, _>>()?;
    Ok((m, leading))
}

/// Encode the replacement in the anchor's font.
///
/// # Errors
///
/// [`EditError::Refused`] for a character the font cannot produce;
/// [`EditError::Unsupported`] when the font's encoding cannot be inverted.
fn encode_replacement(
    font: &ExtractFont,
    anchor: &ShowData,
    replace: &str,
) -> Result<EncodedReplacement, EditError> {
    // The R-INV-5 tie-break seed: codes already used in this run.
    let prefer: BTreeSet<u8> = anchor
        .slots
        .iter()
        .filter_map(|s| u8::try_from(s.code).ok())
        .collect();
    encode_in(font, &prefer, replace)
}

/// [`encode_replacement`] with an explicit R-INV-5 tie-break seed.
///
/// # Errors
///
/// As [`encode_replacement`].
pub(crate) fn encode_in(
    font: &ExtractFont,
    prefer: &BTreeSet<u8>,
    replace: &str,
) -> Result<EncodedReplacement, EditError> {
    if !font.is_simple() {
        return encode_composite(font, replace);
    }
    let glyph_names = font.glyph_names().ok_or(EditError::Unsupported(
        UnsupportedCause::EncodingNotInvertible,
    ))?;
    let inverse = InverseEncoding::build(&font.base_font, glyph_names);
    let e = inverse
        .encode_str(replace, prefer)
        .map_err(EditError::Refused)?;
    Ok(EncodedReplacement {
        codes: e.codes.iter().map(|&c| u32::from(c)).collect(),
        bytes: e.codes,
        disclosures: e.disclosures,
    })
}

/// What [`encode_with_sibling`] settled on.
pub(crate) struct Encoding<'a> {
    pub(crate) encoded: EncodedReplacement,
    /// The font the replacement is set in.
    pub(crate) font: ExtractFont,
    pub(crate) dict: &'a Dict,
    pub(crate) extension: Option<FontExtension>,
    /// Decision 174: the sibling resource name, and the run's own font.
    pub(crate) sibling: Option<(Vec<u8>, ExtractFont)>,
    /// Pass 431.0: the characters set in [`EditOptions::fallback`]'s face.
    /// `encoded` then holds only the characters the run's font kept.
    pub(crate) fallback: Option<Fallback>,
}

/// [`encode_and_extend`], then — when that refuses and one operator holds
/// the match — each same-face sibling resource under `opts.sibling_fonts`
/// (decision 174), then `opts.fallback`'s face for the characters the run's
/// font still refuses (Pass 431.0). The run's own refusal stands when none
/// of them carries the replacement.
#[allow(clippy::too_many_arguments)] // the planner's own locals, passed through once
fn encode_with_sibling<'a>(
    doc: &'a DocumentView<'a>,
    target: &'a EditPlanTarget,
    recs: &[OpRec],
    font: ExtractFont,
    class: &FontClass,
    font_dict: &'a Dict,
    anchor: &ShowData,
    req: &EditRequest,
    opts: &EditOptions,
    single: bool,
) -> Result<Encoding<'a>, EditError> {
    let base_font = font.base_font.clone();
    let own = opts.fallback.map(|_| font.clone());
    let refused =
        match encode_and_extend(doc, target, recs, font, class, font_dict, anchor, req, opts) {
            Ok((encoded, font, extension)) => {
                return Ok(Encoding {
                    encoded,
                    font,
                    dict: font_dict,
                    extension,
                    sibling: None,
                    fallback: None,
                });
            }
            Err(e) => e,
        };
    if !sibling::splittable(anchor, single) {
        return Err(refused);
    }
    if opts.sibling_fonts
        && let Some(enc) = try_sibling(doc, target, recs, font_dict, anchor, req, opts, &base_font)
    {
        return Ok(enc);
    }
    let (Some(face), Some(own)) = (opts.fallback, own) else {
        return Err(refused);
    };
    let encode_own = |text: &str| {
        let mut part = req.clone();
        text.clone_into(&mut part.replace);
        encode_and_extend(
            doc,
            target,
            recs,
            own.clone(),
            class,
            font_dict,
            anchor,
            &part,
            opts,
        )
    };
    let at = fallback::RunAt {
        doc,
        resources: &target.resources,
        recs,
        own_dict: font_dict,
        anchor,
    };
    let (encoded, font, extension, fb) =
        fallback::encode(&at, &req.replace, face, &own, refused, encode_own)?;
    Ok(Encoding {
        encoded,
        font,
        dict: font_dict,
        extension,
        sibling: None,
        fallback: Some(fb),
    })
}

/// The first same-face sibling resource that carries the whole replacement
/// (decision 174), in key order.
#[allow(clippy::too_many_arguments)] // the planner's own locals, passed through once
fn try_sibling<'a>(
    doc: &'a DocumentView<'a>,
    target: &'a EditPlanTarget,
    recs: &[OpRec],
    font_dict: &'a Dict,
    anchor: &ShowData,
    req: &EditRequest,
    opts: &EditOptions,
    base_font: &str,
) -> Option<Encoding<'a>> {
    let vertical = writes_vertically(doc, font_dict);
    for (name, dict) in sibling::candidates(doc, &target.resources, font_dict, base_font) {
        let sib_font = ExtractFont::resolve(doc, dict);
        let Ok(sib_class) = classify_font(doc, dict, &sib_font) else {
            continue;
        };
        if writes_vertically(doc, dict) != vertical {
            continue;
        }
        let mut sib_anchor = anchor.clone();
        sib_anchor.font_name.clone_from(&name);
        let Ok((mut encoded, font, extension)) = encode_and_extend(
            doc,
            target,
            recs,
            sib_font,
            &sib_class,
            dict,
            &sib_anchor,
            req,
            opts,
        ) else {
            continue;
        };
        let note = sibling_note(base_font, &font.base_font, &name);
        encoded.disclosures.push(note);
        return Some(Encoding {
            encoded,
            font,
            dict,
            extension,
            sibling: Some((name, ExtractFont::resolve(doc, font_dict))),
            fallback: None,
        });
    }
    None
}

/// Decision 174's disclosure: the sibling is taken to be the same face from
/// its name alone.
fn sibling_note(own: &str, sibling: &str, resource: &[u8]) -> String {
    format!(
        "'{own}' cannot carry the replacement, so it is set in '{sibling}' (font resource \
         /{}), another font on this page taken to be the same face from its name; the \
         text around it stays in '{own}'",
        String::from_utf8_lossy(resource)
    )
}

/// The replacement's codes and the font extension that makes them showable:
/// a character the encoding cannot address gets an unused code
/// (`code_alloc`), and an uncarried code gets its width and `/ToUnicode`
/// entry (`font_extend`).
#[allow(clippy::too_many_arguments)] // the planner's own locals, passed through once
pub(crate) fn encode_and_extend(
    doc: &DocumentView<'_>,
    target: &EditPlanTarget,
    recs: &[OpRec],
    font: ExtractFont,
    class: &FontClass,
    font_dict: &Dict,
    anchor: &ShowData,
    req: &EditRequest,
    opts: &EditOptions,
) -> Result<(EncodedReplacement, ExtractFont, Option<FontExtension>), EditError> {
    let (mut encoded, font, alloc) = match encode_replacement(&font, anchor, &req.replace) {
        Ok(e) => (e, font, None),
        Err(refused) if !font.is_simple() => {
            let cmap = allocate_cids(doc, target, &font, class, font_dict, anchor, req, opts)
                .map_err(|blocked| with_allocation_reasons(refused, &blocked))?;
            (
                encode_composite_with(&font, &cmap, &req.replace)?,
                font,
                None,
            )
        }
        Err(refused) => {
            let alloc = allocate_codes(doc, target, &font, class, font_dict, anchor, req, opts)
                .map_err(|blocked| with_allocation_reasons(refused, &blocked))?;
            let font = ExtractFont::resolve(doc, &alloc.dict);
            (
                encode_replacement(&font, anchor, &req.replace)?,
                font,
                Some(alloc),
            )
        }
    };
    let dict = alloc.as_ref().map_or(font_dict, |a| &a.dict);
    let mut extension = extend_subset(
        doc, target, recs, &font, class, dict, anchor, req, &encoded, opts,
    )?;
    if let Some(a) = &alloc {
        encoded.disclosures.extend(a.disclosures(&font.base_font));
    }
    let Some(ext) = extension.as_mut() else {
        return Ok((encoded, font, None));
    };
    ext.reencoded = alloc.is_some();
    let font = ExtractFont::resolve(doc, ext.view());
    encoded.disclosures.extend(ext.disclosures(&font.base_font));
    Ok((encoded, font, extension))
}

/// Unused codes for the replacement's characters the encoding cannot
/// address. `Err(vec![])` when allocation does not apply here, so the
/// encoder's own refusal stands unchanged.
#[allow(clippy::too_many_arguments)] // the planner's own locals, passed through once
fn allocate_codes(
    doc: &DocumentView<'_>,
    target: &EditPlanTarget,
    font: &ExtractFont,
    class: &FontClass,
    font_dict: &Dict,
    anchor: &ShowData,
    req: &EditRequest,
    opts: &EditOptions,
) -> Result<Allocation, Vec<Blocked>> {
    let applies = class.embedded && class.subset && font.is_simple();
    let (Some(glyphs), Some(names), true) = (opts.embedded_glyphs, font.glyph_names(), applies)
    else {
        return Err(Vec::new());
    };
    let inverse = InverseEncoding::build(&font.base_font, names);
    let mut absent: Vec<char> = Vec::new();
    for ch in req.replace.chars() {
        // A single-byte code cannot carry a character beyond the BMP (R-INV-8).
        if !inverse.has_char(ch) && u32::from(ch) <= 0xFFFF && !absent.contains(&ch) {
            absent.push(ch);
        }
    }
    if absent.is_empty() {
        return Err(Vec::new());
    }
    allocate(
        doc,
        &target.resources,
        &anchor.font_name,
        font_dict,
        &absent,
        glyphs,
    )
}

/// The composite twin of [`allocate_codes`]: the font's `/ToUnicode` as it
/// reads once each replacement character it lacks is given a CID.
/// `Err(vec![])` when allocation does not apply.
#[allow(clippy::too_many_arguments)] // the planner's own locals, passed through once
fn allocate_cids(
    doc: &DocumentView<'_>,
    target: &EditPlanTarget,
    font: &ExtractFont,
    class: &FontClass,
    font_dict: &Dict,
    anchor: &ShowData,
    req: &EditRequest,
    opts: &EditOptions,
) -> Result<ToUnicodeCMap, Vec<Blocked>> {
    let (Some(glyphs), Some(cmap), true) = (
        opts.embedded_glyphs,
        font.to_unicode_cmap(),
        class.embedded && class.subset,
    ) else {
        return Err(Vec::new());
    };
    let composite = CompositeEncoding::build(&font.base_font, cmap).map_err(|_| Vec::new())?;
    let mut absent: Vec<char> = Vec::new();
    for ch in req.replace.chars() {
        let ambiguous = composite.ambiguous_chars().contains_key(&ch);
        if !composite.covers(ch) && !ambiguous && !absent.contains(&ch) {
            absent.push(ch);
        }
    }
    if absent.is_empty() {
        return Err(Vec::new());
    }
    crate::text_edit::cid_extend::allocate(
        doc,
        &target.resources,
        &anchor.font_name,
        font_dict,
        &absent,
        glyphs,
    )
}

/// `refused` with why each character could not be given a code.
fn with_allocation_reasons(refused: EditError, blocked: &[Blocked]) -> EditError {
    let EditError::Refused(mut r) = refused else {
        return refused;
    };
    if blocked.is_empty() {
        return EditError::Refused(r);
    }
    let why: Vec<String> = blocked
        .iter()
        .map(|b| format!("U+{:04X} '{}': {}", u32::from(b.ch), b.ch, b.reason))
        .collect();
    r.message.push_str(&format!(
        " It could not be given an unused code: {}.",
        why.join("; ")
    ));
    EditError::Refused(r)
}

/// [`encode_replacement`] for a composite font. `classify_font` has already
/// refused a map that is not invertible, so `build` re-derives a known-good
/// inversion.
fn encode_composite(font: &ExtractFont, replace: &str) -> Result<EncodedReplacement, EditError> {
    let cmap = font.to_unicode_cmap().ok_or(EditError::Unsupported(
        UnsupportedCause::CompositeWithoutToUnicode,
    ))?;
    encode_composite_with(font, cmap, replace)
}

/// [`encode_composite`] through `cmap` rather than the font's own map.
fn encode_composite_with(
    font: &ExtractFont,
    cmap: &ToUnicodeCMap,
    replace: &str,
) -> Result<EncodedReplacement, EditError> {
    let composite = CompositeEncoding::build(&font.base_font, cmap).map_err(|e| {
        EditError::Unsupported(UnsupportedCause::FontMapNotInvertible {
            detail: e.to_string(),
        })
    })?;
    let e = composite.encode_str(replace).map_err(EditError::Refused)?;
    Ok(EncodedReplacement {
        codes: e.cids.iter().map(|&c| u32::from(c)).collect(),
        bytes: e.to_bytes(),
        disclosures: ambiguity_note(composite.ambiguous_chars())
            .into_iter()
            .collect(),
    })
}

/// The replacement avoided every ambiguous character, but the font has some,
/// and the operator's next edit deserves to know which will be refused.
fn ambiguity_note(ambiguous: &BTreeMap<char, Vec<u32>>) -> Option<String> {
    if ambiguous.is_empty() {
        return None;
    }
    let list: Vec<String> = ambiguous
        .iter()
        .take(8)
        .map(|(ch, codes)| {
            let codes: Vec<String> = codes.iter().map(u32::to_string).collect();
            format!("{ch:?} (codes {})", codes.join("/"))
        })
        .collect();
    Some(format!(
        "font map: {} character(s) of this font are produced by more than one code and are REFUSED if a replacement needs them — {}{}; this replacement used none of them.",
        ambiguous.len(),
        list.join(", "),
        if ambiguous.len() > 8 { ", …" } else { "" }
    ))
}

/// The embedded-subset floor (`R-INV-1`), with decision 172's route A in
/// front of it: a character whose code no show of this font on the page uses
/// is added to the font dictionary when [`EditOptions::embedded_glyphs`] finds
/// its outline in the program, and refused otherwise.
#[allow(clippy::too_many_arguments)] // the planner's own locals, passed through once
fn extend_subset(
    doc: &DocumentView<'_>,
    target: &EditPlanTarget,
    recs: &[OpRec],
    font: &ExtractFont,
    class: &FontClass,
    font_dict: &Dict,
    anchor: &ShowData,
    req: &EditRequest,
    encoded: &EncodedReplacement,
    opts: &EditOptions,
) -> Result<Option<crate::text_edit::font_extend::FontExtension>, EditError> {
    if !(class.embedded && class.subset) {
        return Ok(None);
    }
    let carried = carried_codes(recs, &anchor.font_name);
    let mut missing: Vec<(char, u32)> = Vec::new();
    for (u, code) in req.replace.chars().zip(encoded.codes.iter().copied()) {
        if !carried.contains(&code) && !missing.iter().any(|&(_, c)| c == code) {
            missing.push((u, code));
        }
    }
    if missing.is_empty() {
        return Ok(None);
    }
    let Some(glyphs) = opts.embedded_glyphs else {
        let unread: Vec<Blocked> = missing
            .iter()
            .map(|&(ch, code)| Blocked {
                ch,
                code,
                reason: String::new(),
            })
            .collect();
        return Err(subset_floor(doc, target, recs, font, &unread));
    };
    let planned = crate::text_edit::font_extend::plan(
        doc,
        &target.resources,
        &anchor.font_name,
        font_dict,
        &missing,
        glyphs,
    );
    let planned = match (planned, &opts.subset_augment) {
        (Err(blocked), Some(settings)) => {
            let at = crate::text_edit::augment_route::FontAt {
                resources: &target.resources,
                font_name: &anchor.font_name,
                font_dict,
            };
            crate::text_edit::augment_route::plan(doc, &at, &missing, glyphs, settings, blocked)
        }
        (planned, _) => planned,
    };
    planned
        .map(Some)
        .map_err(|b| subset_floor(doc, target, recs, font, &b))
}

/// The embedded-subset floor: a code the subset does not already carry on
/// this page is refused by name.
///
/// The remedy names the standard-14 faces `set_font` would actually reach on
/// THIS page (`std14_faces_reachable`): asking for "Helvetica" on a page whose
/// font is `ABCDEF+Helvetica` re-selects that very subset. It is computed
/// once and spent on both the structured field and the sentence, and is the
/// same remedy the absent-glyph refusal names, because an operator
/// experiences the two as one thing.
///
/// Returns [`EditError::Refused`] with [`RInvTrigger::TargetAbsent`] naming
/// the first of `blocked`; the message lists every one, each with the reason
/// route A could not add it (empty when no program reader was supplied).
fn subset_floor(
    doc: &DocumentView<'_>,
    target: &EditPlanTarget,
    recs: &[OpRec],
    font: &ExtractFont,
    blocked: &[Blocked],
) -> EditError {
    let (u, code) = blocked.first().map_or((char::MIN, 0), |b| (b.ch, b.code));
    let reachable =
        crate::text_edit::format::std14_faces_reachable(doc, &target.resources, recs, u);
    let remedy = match crate::text_edit::encoding::faces_clause(&reachable).as_str() {
        "" => String::new(),
        clause => format!(
            " To make this edit now, switch the run to a font that carries '{u}' -- `format_text` with `set_font` will add one, and {clause}"
        ),
    };
    let why = refusal_reasons(blocked);
    EditError::Refused(Refusal {
        trigger: RInvTrigger::TargetAbsent,
        character: Some(u),
        base_font: font.base_font.clone(),
        remedy_faces: reachable.iter().map(|&f| f.to_owned()).collect(),
        message: format!(
            "R-INV-1 (embedded-subset floor): character U+{:04X} '{}' maps to code {} \
             which font '{}' (an embedded SUBSET) does not already carry on this page; \
             embedding a new glyph into that subset is deferred to FF-C (font \
             subsetting). This is exactly Acrobat's 'embedded-but-not-local' floor.{}{}",
            u as u32, u, code, font.base_font, why, remedy
        ),
    })
}

/// The per-character tail of the subset-floor message: the first character's
/// reason, then every other refused character with its own.
fn refusal_reasons(blocked: &[Blocked]) -> String {
    let mut out = String::new();
    if let Some(r) = blocked.first().map(|b| &b.reason).filter(|r| !r.is_empty()) {
        out.push_str(&format!(" It could not be added to the font: {r}."));
    }
    let rest: Vec<String> = blocked
        .iter()
        .skip(1)
        .map(|b| match b.reason.as_str() {
            "" => format!("U+{:04X} '{}' (code {})", b.ch as u32, b.ch, b.code),
            r => format!("U+{:04X} '{}' (code {}): {r}", b.ch as u32, b.ch, b.code),
        })
        .collect();
    if !rest.is_empty() {
        out.push_str(&format!(" Also refused: {}.", rest.join("; ")));
    }
    out
}

/// The disclosures every edit carries, whatever it was written into.
fn general_disclosures(
    req: &EditRequest,
    opts: &EditOptions,
    anchor: &ShowData,
    find: &str,
    font: &ExtractFont,
    class: &FontClass,
) -> Vec<String> {
    let mut out = Vec::new();
    if req.find.is_empty() {
        // Reached only on a pinned request. See
        // `format::disclosure_whole_operator` for the multi-operator sentence.
        out.push(format!(
            "whole operator: no find text was given and a byte span was pinned, so the ENTIRE \
             pinned show operator was replaced — {} character(s). One text run in pdfcer's \
             extraction model can carry glyphs from several show operators (13% of runs over \
             pdfcer's corpus do), so if the selection this pin came from spanned more than one \
             operator, only the PINNED one changed.",
            find.chars().count()
        ));
    }
    out.push(trust_disclosure(class.embedded, &font.base_font));
    out.push(
        "save: this edit was written INCREMENTALLY (R34/R70); the prior text survives in the \
         document's revision history by design. To truly remove text, use redaction (Pass 8) — a \
         distinct, security operation."
            .to_owned(),
    );
    if matches!(opts.disposition, FollowerDisposition::Reflow) {
        out.push(
            "relayout: the edited line was shifted by the advance delta and MAY now overflow the \
             original right margin; block re-wrap (reflow) is deferred (FF-A) — enable reflow to \
             re-wrap."
                .to_owned(),
        );
    }
    if let Some(mcid) = anchor.mcid {
        out.push(format!(
            "tagged PDF: the edit is inside a marked-content sequence (/MCID {mcid}); its \
             BDC/EMC+MCID wrapper was PRESERVED (structure references stay valid), but the \
             structure tree's /ActualText and reading order were NOT updated and are now STALE \
             (a stale /ActualText wins on extraction, §14.9.4). pdfcer discloses this rather than \
             silently corrupting the accessibility tree (R72)."
        ));
    }
    out
}

/// The disclosures owed to where the edit was written: a collapsed
/// multi-stream page, or a form XObject.
fn target_disclosures(
    doc: &DocumentView<'_>,
    target: &EditPlanTarget,
    anchor: &ShowData,
    out: &mut Vec<String>,
) {
    let extra_emptied = target.extra_emptied;
    if extra_emptied > 0 {
        out.push(format!(
            "multi-stream page: {extra_emptied} additional /Contents stream(s) were collapsed \
             into the first and emptied so the edit's byte offsets stay coherent."
        ));
    }
    if let Some(form) = target.form.as_ref() {
        disclose_form_edit(doc, form, target.invocations.as_ref(), anchor, out);
    }
}

/// Assemble the [`EditReport`]. `moved` is (advance delta, followers
/// repositioned, show operators spanned).
fn edit_report(
    target: &EditPlanTarget,
    font: &ExtractFont,
    class: &FontClass,
    opts: &EditOptions,
    (advance_delta, followers_repositioned, operators_spanned): (f64, u64, u64),
    anchor: &ShowData,
    disclosures: Vec<String>,
) -> EditReport {
    let invocations = target.invocations.as_ref();
    EditReport {
        base_font: font.base_font.clone(),
        glyph_source: if class.embedded {
            EditGlyphSource::Embedded
        } else {
            EditGlyphSource::NonEmbedded
        },
        subset: class.subset,
        advance_delta,
        disposition: opts.disposition,
        followers_repositioned,
        operators_spanned,
        tagged_mcid: anchor.mcid,
        content_object: target.content_id.num,
        extra_objects_emptied: target.extra_emptied,
        form_object: target.form.as_ref().map(|f| f.id.num),
        form_invocations: invocations.map_or(0, |set| set.count() as u64),
        form_pages: invocations
            .map(|set| set.pages.iter().copied().collect())
            .unwrap_or_default(),
        disclosures,
        fallback: None,
    }
}

/// What the planner hands to the code that lays out the replacement.
struct Laying<'a> {
    recs: &'a [OpRec],
    /// The run's own font, which measures the glyphs already on the page.
    font: &'a ExtractFont,
    /// The font the replacement is set in, with `font_dict` its dictionary.
    glyph_font: &'a ExtractFont,
    /// The decision 174 sibling resource the replacement switches to.
    switch: Option<&'a [u8]>,
    /// Pass 431.0: the characters set in a fallback face.
    fallback: Option<&'a Fallback>,
    font_dict: &'a Dict,
    anchor: &'a ShowData,
    anchor_index: usize,
    /// The anchor operator's byte range in the content buffer.
    anchor_bytes: (usize, usize),
    m: &'a MatchRun,
    /// The span's other operators with their matched parts.
    others: &'a [(usize, &'a ShowData, MatchRun)],
    replace: &'a str,
    encoded: &'a EncodedReplacement,
    disposition: FollowerDisposition,
    /// The replacement's advance (§9.4.4).
    a_new: f64,
    /// The advance of the anchor's matched part, kerns included.
    a_old_last: f64,
}

/// The laid-out replacement: where its glyphs land, the byte rewrites, the
/// advance change, followers moved and the span disclosures.
struct Laid {
    layout: EditLayout,
    edits: Vec<(usize, usize, Vec<u8>)>,
    delta: f64,
    followers: u64,
    span_note: Option<String>,
    td_note: Option<String>,
}

/// Lay out a match within one text object or across several.
fn lay(c: &Laying<'_>, at: &Located<'_>) -> Laid {
    if at.crossed {
        lay_across(c, &at.leading_ops)
    } else {
        lay_in_object(c)
    }
}

/// Lay out a match inside one text object: the replacement goes into the
/// last operator and is moved back to where the match began.
fn lay_in_object(c: &Laying<'_>) -> Laid {
    let Laying {
        recs,
        font,
        anchor,
        anchor_index,
        anchor_bytes: (a_start, a_end),
        m,
        others: leading_matches,
        disposition,
        a_new,
        a_old_last,
        ..
    } = *c;
    let (lead_shift, mut op_deltas, origin_x) = span_shifts(font, anchor, m, leading_matches);
    let delta: f64 = round4(lead_shift + a_new - a_old_last);
    // Reflow: everything after the anchor moves by the net change. Pin: the
    // compensating number absorbs it inside the anchor, and the anchor's own
    // move is undone for the operators after it.
    let (anchor_walk, pin_num) = match disposition {
        FollowerDisposition::Pin => (
            -lead_shift,
            compensating_tj(delta, anchor.tf_size, anchor.th()),
        ),
        FollowerDisposition::Reflow => (a_new - a_old_last, None),
    };
    op_deltas.push((anchor_index, anchor_walk));
    let (layout, new_op_bytes) = laid_operator(c, origin_x, pin_num);
    let mut edits: Vec<(usize, usize, Vec<u8>)> = vec![(a_start, a_end, new_op_bytes)];
    let emptied = clear_leading(recs, leading_matches, &mut edits);

    let walk_needed = match disposition {
        FollowerDisposition::Reflow => delta != 0.0 || lead_shift != 0.0,
        FollowerDisposition::Pin => lead_shift != 0.0,
    };
    let mut reflowed = if walk_needed {
        reposition_followers(recs, anchor, &op_deltas)
    } else {
        Reflowed::default()
    };
    let followers = reflowed.followers;
    let td_note = reflowed.note.take();
    edits.append(&mut reflowed.edits);
    let span_note = in_object_span_note(
        leading_matches.len() as u64 + 1,
        emptied,
        disposition,
        delta,
    );
    Laid {
        layout,
        edits,
        delta,
        followers,
        span_note,
        td_note,
    }
}

/// The anchor operator rewritten with the replacement, and where the
/// replacement's glyphs land: in the run's font, switched to a decision 174
/// sibling, or split between the run's font and a fallback face.
fn laid_operator(c: &Laying<'_>, origin_x: f64, pin_num: Option<f64>) -> (EditLayout, Vec<u8>) {
    let (anchor, m, bytes) = (c.anchor, c.m, &c.encoded.bytes);
    if let Some(fb) = c.fallback {
        let layout = fb.layout(anchor, c.font_dict, c.glyph_font, origin_x);
        return (layout, fb.emit(anchor, m, pin_num));
    }
    let mut layout = EditLayout::new(
        anchor,
        c.font_dict,
        c.glyph_font,
        c.replace,
        &c.encoded.codes,
        origin_x,
    );
    let op = match c.switch {
        Some(name) => {
            layout.font_name = name.to_vec();
            sibling::emit_switched_operator(anchor, m, bytes, pin_num, name)
        }
        None => emit_edited_operator(anchor, m, bytes, pin_num),
    };
    (layout, op)
}

/// Remove each leading operator's matched part; answers how many were left
/// empty.
fn clear_leading(
    recs: &[OpRec],
    leading_matches: &[(usize, &ShowData, MatchRun)],
    edits: &mut Vec<(usize, usize, Vec<u8>)>,
) -> u64 {
    let mut emptied = 0u64;
    for (k, s, mr) in leading_matches {
        let bytes = emit_edited_operator(s, mr, &[], None);
        if bytes.starts_with(b"() Tj") || bytes == b"[()] TJ" {
            emptied += 1;
        }
        if let Some(r) = recs.get(*k) {
            edits.push((r.start, r.end, bytes));
        }
    }
    emptied
}

/// Per-operator shifts for a match spanning show operators inside one text
/// object, in record order and text-space units along the line, plus the
/// last leading operator's shift and where the replacement lands.
///
/// Each operator after the first is moved so its match starts where the
/// WHOLE match started (`p0`), measured from the operators' real origins, so
/// the producer's inter-operator gaps are removed with the glyphs rather than
/// left as a hole before the replacement. Shifts snap to the writer's
/// 4-decimal precision: re-measured geometry leaves ~1e-6 of float noise
/// where the true shift is zero, and a nonzero shift walks every follower.
fn span_shifts(
    font: &ExtractFont,
    anchor: &ShowData,
    m: &MatchRun,
    leading: &[(usize, &ShowData, MatchRun)],
) -> (f64, Vec<(usize, f64)>, f64) {
    let reference = anchor.text_matrix;
    let mut lead_shift = 0.0f64;
    let mut op_deltas: Vec<(usize, f64)> = Vec::with_capacity(leading.len() + 1);
    let Some((_, s0, mr0)) = leading.first() else {
        return (0.0, op_deltas, advance_before(font, anchor, m.elem, m.b_lo));
    };
    let p0 = line_x(&reference, &s0.text_matrix) + advance_before(font, s0, mr0.elem, mr0.b_lo);
    let next_shifts = leading
        .iter()
        .skip(1)
        .map(|(_, s, mr)| (*s, mr.elem, mr.b_lo))
        .chain(std::iter::once((anchor, m.elem, m.b_lo)))
        .map(|(s, elem, byte)| {
            round4(p0 - advance_before(font, s, elem, byte) - line_x(&reference, &s.text_matrix))
        });
    for ((k, _, _), shift) in leading.iter().zip(next_shifts) {
        op_deltas.push((*k, shift - lead_shift));
        lead_shift = shift;
    }
    (lead_shift, op_deltas, p0)
}

/// The disclosure for a match spanning `operators_spanned` show operators in
/// one text object; `None` for a single operator.
fn in_object_span_note(
    operators_spanned: u64,
    emptied: u64,
    disposition: FollowerDisposition,
    delta: f64,
) -> Option<String> {
    (operators_spanned > 1).then(|| {
        format!(
            "span: the text was written across {operators_spanned} consecutive show operators (one glyph per operator is a common producer shape) and was edited as ONE run — the replacement went into the operator holding the match's end and was moved back to where the match began, the matched glyphs were removed from the {} earlier one(s){}, and {}.",
            operators_spanned - 1,
            if emptied > 0 {
                format!(" ({emptied} left as an empty `() Tj` so the producer's own positioning chain stays intact)")
            } else {
                String::new()
            },
            match disposition {
                FollowerDisposition::Reflow => format!("the text after it on the line moved by the net change ({delta:.3} text-space units)"),
                FollowerDisposition::Pin => "the text after it on the line kept its position".to_owned(),
            }
        )
    })
}

/// Lay out a match that crosses text objects ([`cross_object`]): the
/// replacement goes into the first operator in place, and the later ones
/// lose their matched glyphs.
fn lay_across(c: &Laying<'_>, others: &[(usize, &ShowData, usize, usize)]) -> Laid {
    let later: Vec<cross_object::Later<'_>> = c
        .others
        .iter()
        .zip(others)
        .map(
            |((index, show, matched), (_, _, _, hi))| cross_object::Later {
                index: *index,
                show,
                matched,
                has_tail: *hi < show.text.len(),
            },
        )
        .collect();
    let origin_x = advance_before(c.font, c.anchor, c.m.elem, c.m.b_lo);
    let layout = EditLayout::new(
        c.anchor,
        c.font_dict,
        c.glyph_font,
        c.replace,
        &c.encoded.codes,
        origin_x,
    );
    let mut emptied = cross_object::empty_later(
        c.recs,
        c.font,
        (c.anchor_index, c.anchor),
        &later,
        origin_x + c.a_new,
        c.disposition,
    );
    let mut edits = vec![(
        c.anchor_bytes.0,
        c.anchor_bytes.1,
        emit_edited_operator(c.anchor, c.m, &c.encoded.bytes, None),
    )];
    edits.append(&mut emptied.edits);
    Laid {
        layout,
        edits,
        delta: emptied.delta,
        followers: 0,
        span_note: Some(cross_object::disclosure(&emptied, c.disposition)),
        td_note: None,
    }
}

/// Build the ordered list of candidate targets an [`EditRequest`] may be
/// planned against (`Pass 119.0`).
///
/// # The order, and why it is not negotiable
///
/// The page's own `/Contents` comes first, then each reachable form in `Do`
/// order — i.e. **paint order**. Two reasons, and the second is the one that
/// matters:
///
/// 1. The page stream is the cheap case: no scan, no invocation map. A
///    document with no forms pays nothing at all for this Pass existing.
/// 2. `Do` order is the order the marks appear on the page, so when two
///    streams both contain the sought text, the one chosen is the one drawn
///    first — a rule the operator can predict without knowing anything about
///    PDF structure. Choosing by object number, or by whichever stream happens
///    to be smaller, would be arbitrary in a way that shows up as *"it edited
///    the wrong one"*.
///
/// # Errors
///
/// [`EditError::Unsupported`] when an explicitly-named
/// [`EditTarget::Form`] is not reachable from this page, or when the page has
/// no `/Contents` and the target needs one. A *named* target that cannot be
/// found is an error rather than an empty candidate list on purpose: the
/// caller asserted a fact about the document, and silently searching somewhere
/// else would hide that the assertion was wrong.
pub(crate) fn edit_candidates(
    doc: &DocumentView<'_>,
    page: &Page,
    req: &EditRequest,
) -> Result<Vec<(EditPlanTarget, ContentStream)>, EditError> {
    use crate::text_edit::forms;

    let mut out: Vec<(EditPlanTarget, ContentStream)> = Vec::new();
    if matches!(req.target, EditTarget::Auto | EditTarget::PageContents) {
        // A page with no `/Contents` is not an error when forms are still on
        // the table — it is simply not a candidate. The refusal only fires
        // when the caller asked for the page stream by name (below).
        if let Ok(target) = EditPlanTarget::page(page) {
            match ContentStream::from_page(doc, page) {
                Ok(stream) => out.push((target, stream)),
                Err(e) => return Err(EditError::Content(e)),
            }
        } else if matches!(req.target, EditTarget::PageContents) {
            return Err(EditError::Unsupported(UnsupportedCause::NoContents));
        }
    }
    if matches!(req.target, EditTarget::PageContents) {
        return Ok(out);
    }

    let scan = forms::scan_page_forms(doc, page);
    if scan.forms.is_empty() {
        if let EditTarget::Form { object } = req.target {
            return Err(EditError::Unsupported(UnsupportedCause::FormNotOnPage {
                object,
            }));
        }
        return Ok(out);
    }
    // ONE document walk for every candidate (see `forms::invocation_map`).
    let mut map = forms::invocation_map(doc);
    let mut seen: BTreeSet<u32> = BTreeSet::new();
    for form in scan.forms {
        if let EditTarget::Form { object } = req.target
            && form.id.num != object
        {
            continue;
        }
        // The same form painted twice on this page is ONE editable stream, so
        // it is one candidate. Its fan-out is reported by the invocation set,
        // which counts both sites.
        if !seen.insert(form.id.num) {
            continue;
        }
        let Ok(stream) = ContentStream::from_form(doc, form.id) else {
            // An undecodable form is skipped rather than fatal: every other
            // form on the page is still editable, and refusing the whole
            // request would cost the operator content that is fine (§10).
            continue;
        };
        let invocations = map.remove(&form.id.num).unwrap_or_default();
        out.push((EditPlanTarget::form(form, invocations), stream));
    }
    if out.is_empty()
        && let EditTarget::Form { object } = req.target
    {
        return Err(EditError::Unsupported(UnsupportedCause::FormUndecodable {
            object,
        }));
    }
    Ok(out)
}

/// Plan an edit against the first candidate stream that yields one
/// (`Pass 119.0`).
///
/// # Which error survives when every candidate refuses
///
/// The candidates are tried in order and the **first non-locational** error
/// wins — a font-coverage refusal, an unsupported run, a form-level refusal —
/// because those describe a run the planner actually found and are what the
/// operator needs to hear. [`EditError::NoMatch`] and
/// [`EditError::PinnedSpanNotFound`] are *locational*: they mean "not in this
/// buffer", which is uninformative while other buffers remain untried. Only if
/// every candidate is locational does the first one propagate.
///
/// This ordering is the whole reason the pre-119.0 failure was so
/// misleading. A pinned edit into a form used to report *"text to edit was not
/// found in an editable run on the page"* — naming the operator's text as the
/// problem when the text was present and the surgery was looking at the wrong
/// stream. Now the search reaches the other stream; and when it genuinely
/// cannot, `PinnedSpanNotFound` says so in those words.
///
/// # Errors
///
/// See [`EditError`].
pub(crate) fn plan_edit_anywhere(
    doc: &DocumentView<'_>,
    page: &Page,
    req: &EditRequest,
    opts: &EditOptions,
) -> Result<(EditPlan, EditPlanTarget), EditError> {
    let candidates = edit_candidates(doc, page, req)?;
    if candidates.is_empty() {
        return Err(EditError::no_match(req.find.clone()));
    }
    let mut first_locational: Option<EditError> = None;
    for (target, stream) in candidates {
        match plan_edit_target(doc, &target, &stream, req, opts) {
            Ok(plan) => return Ok((plan, target)),
            Err(e) if is_locational_error(&e) => {
                if first_locational.is_none() {
                    first_locational = Some(e);
                }
            }
            Err(e) => return Err(e),
        }
    }
    Err(first_locational.unwrap_or_else(|| EditError::no_match(req.find.clone())))
}

/// How far left of the anchor's own origin a same-baseline re-anchor may sit
/// and still count as this line's tail: **nothing**, up to a rounding guard.
///
/// The unit is text space, the same unit `Tm`'s `e` is written in, so this is
/// a tenth of one text-space unit — under a twentieth of a point at the 5 pt
/// title-block type this guard was measured on, and far below any real
/// leftward re-anchor (the SolidWorks note bullet below jumps `-5.66931`).
/// It exists only so a producer that re-states the *same* origin with f32
/// round-trip noise is not read as jumping backwards.
pub(crate) const FOLLOWER_ORIGIN_EPSILON: f64 = 0.1;

/// Whether the positioning operator at `index` lands the pen **before**
/// `left_bound` on the line — in which case it is not this line's tail, and
/// the follower walk must stop rather than shift it.
///
/// `left_bound` is the origin of the **first** edited show operator, not the
/// anchor's. On a `Pass 256.0` span the anchor is the **last** operator of the
/// run, so measuring from it puts the span's own interior `Td` steps "behind"
/// the edit and stops the walk before it starts. Caught by
/// `text_edit_span.rs::a_growing_replacement_respaces_the_followers_and_keeps_the_next_line_put`
/// on the first cut of this guard — a per-glyph producer's three-operator span
/// respaced nothing at all. The first operator is also the semantically right
/// reference: *the rest of the line* is what lies right of where the edited
/// text STARTS.
///
/// # The defect this closes, measured on a real file
///
/// [`same_line`] answers *"same baseline?"*, and the walk in
/// [`reposition_followers`] was treating that as *"the rest of the line?"*.
/// Those come apart the moment a producer writes a line's pieces out of
/// visual order, and SolidWorks does exactly that for a numbered note: the
/// note's **text** is emitted first and its **bullet** second, to the LEFT,
/// on the same baseline —
///
/// ```text
/// 100.00423 Tz 5.66931 -1 Td  <TOLERANCE :->Tj     ← the anchor
///  99.94655 Tz -5.66931 0 Td  <3.>Tj               ← same row, 28 pt LEFT
///  99.82585 Tz 5.66931 -1 Td  <X/XX: …>Tj          ← the next line
/// ```
///
/// Shortening `TOLERANCE :-` by `ΔA` then moved the `3.` bullet by `ΔA` as
/// well (and compensated the line after it, so the damage was confined to the
/// one glyph pair the operator had not touched). Reported as *"when I edit
/// line #3, after I am done the whole line shifts position"* — the bullet is
/// the part of the line the eye tracks.
///
/// # Why the origin is read off the show operator, not computed here
///
/// A `Td`'s operands are relative to the line matrix, which this walk does not
/// track — it rewrites operands by delta precisely so it never has to. The
/// [`Walk`] already resolved an absolute [`ShowData::text_matrix`] for every
/// show operator, so the honest answer is one lookup forward rather than a
/// second matrix machine. When the matrix could not be tracked
/// ([`ShowData::matrix_known`] false) this answers `false` — *do not stop* —
/// which leaves the pre-existing behaviour in place for that case rather than
/// letting a new guard silently suppress reflow on files it was never measured
/// against.
fn re_anchors_before_anchor(recs: &[OpRec], index: usize, left_bound: f64) -> bool {
    for r in recs.iter().skip(index + 1) {
        match &r.rec {
            // Nothing between the step and the string it positions.
            Rec::Show(s) => {
                return s.matrix_known && s.text_matrix[4] < left_bound - FOLLOWER_ORIGIN_EPSILON;
            }
            // A second positioning operator before any string: this step's
            // landing point is overwritten and was never shown at, so it
            // cannot be judged. The next step gets asked on its own turn.
            Rec::Tm(_) | Rec::Td { .. } => return false,
            Rec::EndText | Rec::Boundary => return false,
            Rec::Ignore => {}
        }
    }
    false
}

/// Re-space the operators that follow an edit on the same line
/// (`Pass 256.0` generalisation of the `Tm`-only follower loop).
///
/// `op_deltas` are the edited show operators (record index, advance
/// change) in record order. Walking forward from the first of them, two
/// running quantities are kept: `cum`, the total change so far, and
/// `absorbed`, how much of it the positioning chain has already realised.
///
/// - An absolute **`Tm`** on the same row is rewritten with `e + cum·a`,
///   `f + cum·b` (`cum` is along the line, in text-space units); that
///   realises everything, so `absorbed = cum`.
/// - An x-only **`Td`** (`|ty|` within [`SPAN_LINE_DRIFT_TOLERANCE`]) is RELATIVE to the line matrix it also replaces, so
///   shifting it by `cum − absorbed` moves that operator and every later
///   relative step with it — the cumulative effect falls out of the
///   operator's own semantics (§9.4.2), and `absorbed = cum`.
/// - A **`Td` whose `|ty|` exceeds [`SPAN_LINE_DRIFT_TOLERANCE`]** starts a new line, still relative to the
///   shifted chain: it is rewritten with `tx − absorbed` so the next line
///   lands exactly where the producer put it, and the walk stops.
/// - **`T*`**, `'`, `"` and `ET` stop the walk. `T*` cannot be compensated
///   (it has no operands), so when one — or a `'`/`"` — lies ahead in the
///   same text object, `Td` steps are NOT rewritten at all (the pre-256.0
///   behaviour: only `Tm` followers move) and the report says why. That
///   keeps every edit that succeeded before this Pass producing the bytes
///   it produced then, and confines the new re-spacing to text objects
///   whose remaining lines are positioned by `Td`/`Tm`.
fn reposition_followers(recs: &[OpRec], anchor: &ShowData, op_deltas: &[(usize, f64)]) -> Reflowed {
    let Some(&(first_idx, _)) = op_deltas.first() else {
        return Reflowed::default();
    };
    // Look ahead: is there an uncompensatable next-line operator before ET?
    let td_safe = !recs.iter().skip(first_idx + 1).any(|r| match &r.rec {
        Rec::EndText => false,
        Rec::Boundary => true,
        Rec::Show(s) => matches!(s.op, ShowOp::Quote | ShowOp::DoubleQuote),
        _ => false,
    }) || !recs
        .iter()
        .skip(first_idx + 1)
        .take_while(|r| !matches!(r.rec, Rec::EndText))
        .any(|r| matches!(r.rec, Rec::Boundary) || matches!(&r.rec, Rec::Show(s) if matches!(s.op, ShowOp::Quote | ShowOp::DoubleQuote)));
    let has_td = recs
        .iter()
        .skip(first_idx + 1)
        .take_while(|r| !matches!(r.rec, Rec::EndText))
        .any(|r| matches!(r.rec, Rec::Td { .. }));

    // Where the EDIT starts on the line — the ordering reference for
    // `re_anchors_before_anchor`. The first edited operator, not the anchor:
    // on a span the anchor is the LAST of them. Falls back to the anchor when
    // the first record is not a show operator with a tracked matrix, which
    // keeps the guard inert rather than letting it stop a walk it cannot
    // judge.
    let left_bound = match recs.get(first_idx).map(|r| &r.rec) {
        Some(Rec::Show(s)) if s.matrix_known => s.text_matrix[4],
        _ => anchor.text_matrix[4],
    };

    let mut edits = Vec::new();
    let mut followers = 0u64;
    let mut cum = 0.0f64;
    let mut absorbed = 0.0f64;
    for (i, r) in recs.iter().enumerate().skip(first_idx) {
        if let Some((_, d)) = op_deltas.iter().find(|(k, _)| *k == i) {
            cum += d;
            continue;
        }
        match &r.rec {
            Rec::EndText | Rec::Boundary => break,
            Rec::Show(s) if matches!(s.op, ShowOp::Quote | ShowOp::DoubleQuote) => break,
            Rec::Tm(m) => {
                if !same_line(anchor, m) {
                    break;
                }
                // Same baseline is NOT the same as "after this on the line".
                // A producer may re-anchor BACKWARDS on the same row; that is
                // a different piece of text, not this line's tail. See
                // `re_anchors_before_anchor`.
                if m[4] < left_bound - FOLLOWER_ORIGIN_EPSILON {
                    break;
                }
                // `cum` is along the text line; a scaled or rotated `Tm`
                // turns it into this much `e` and `f`.
                edits.push((
                    r.start,
                    r.end,
                    emit_tm([m[0], m[1], m[2], m[3], m[4] + cum * m[0], m[5] + cum * m[1]]),
                ));
                absorbed = cum;
                followers += 1;
            }
            Rec::Td { tx, ty, leading } if td_safe => {
                // Same guard as the `Tm` arm, asked of the position this step
                // actually lands the pen at. A `Td`'s operands are relative to
                // a line matrix this loop does not track, so the origin is read
                // off the show operator the step positions — which the walk
                // already resolved into an absolute `Tm`.
                let op: &[u8] = if *leading { b" TD" } else { b" Td" };
                // A new line (beyond the span search's drift tolerance —
                // SolidWorks steps along ONE line with `tx ±0.00057 Td`), or
                // a step back to text before the edit: either
                // way the walk stops here. `Td` is RELATIVE, so the step
                // still carries the chain's shift and must undo it, or the
                // next line (which normally starts left of a mid-line edit)
                // moves with the edited one.
                if ty.abs() > SPAN_LINE_DRIFT_TOLERANCE
                    || re_anchors_before_anchor(recs, i, left_bound)
                {
                    if round4(absorbed) != 0.0 {
                        let mut out = Vec::new();
                        emit_number(&mut out, round4(tx - absorbed));
                        out.push(b' ');
                        emit_number(&mut out, *ty);
                        out.extend_from_slice(op);
                        edits.push((r.start, r.end, out));
                    }
                    break;
                }
                // Glyph widths arrive as f32, so a delta of "one 28.8 pt
                // glyph" is 28.80000114…; rounding the rewritten operand
                // to 1/10 000 pt (a 720 000th of an inch) keeps the
                // producer's own numbers clean instead of smearing f32
                // noise across the line. The absolute-`Tm` path is left
                // as it always was (pre-256.0 bytes stay identical).
                let shift = round4(cum - absorbed);
                if shift != 0.0 {
                    let mut out = Vec::new();
                    emit_number(&mut out, round4(tx + shift));
                    out.push(b' ');
                    emit_number(&mut out, *ty);
                    out.extend_from_slice(op);
                    edits.push((r.start, r.end, out));
                    followers += 1;
                }
                absorbed = cum;
            }
            Rec::Td { .. } => break,
            _ => {}
        }
    }
    let note = (has_td && !td_safe).then(|| {
        "relayout: the operators after this edit are positioned by `Td` steps, but a later line in the same text object uses `T*`, `'` or `\"` (which pdfcer cannot re-anchor), so those `Td` steps were NOT re-spaced — the text after the edit keeps the producer's original positions and may crowd or gap the edited glyphs."
            .to_owned()
    });
    Reflowed {
        edits,
        followers,
        note,
    }
}

/// What [`reposition_followers`] decided: the byte-range rewrites, how many
/// positioning operators moved, and the disclosure when `Td` steps were
/// deliberately left alone.
#[derive(Default)]
struct Reflowed {
    edits: Vec<(usize, usize, Vec<u8>)>,
    followers: u64,
    note: Option<String>,
}

/// Round to 1/10 000 pt — see `reposition_followers`.
pub(crate) fn round4(v: f64) -> f64 {
    let r = (v * 10_000.0).round() / 10_000.0;
    if r == 0.0 { 0.0 } else { r }
}

/// Where `m`'s origin sits along `reference`'s text line, in text-space units
/// — the unit `Td` operands and glyph advances share (§9.4.2, §9.4.4).
pub(crate) fn line_x(reference: &[f64; 6], m: &[f64; 6]) -> f64 {
    let (a, b) = (reference[0], reference[1]);
    let norm = a * a + b * b;
    if norm < f64::EPSILON {
        return 0.0;
    }
    ((m[4] - reference[4]) * a + (m[5] - reference[5]) * b) / norm
}

/// The pen advance from `s`'s origin to the code at byte `byte` of element
/// `elem`: every glyph and `TJ` number before it (§9.4.3, §9.4.4).
fn advance_before(font: &ExtractFont, s: &ShowData, elem: usize, byte: usize) -> f64 {
    let kerns: f64 = s
        .elems
        .iter()
        .take(elem)
        .map(|e| match e {
            ShowElem::Num(n) => -n / 1000.0 * s.tf_size * s.th(),
            ShowElem::Str(_) => 0.0,
        })
        .sum();
    let glyphs: f64 = s
        .slots
        .iter()
        .filter(|sl| sl.elem < elem || (sl.elem == elem && sl.byte_in_elem < byte))
        .map(|sl| glyph_advance(font, sl.code, s))
        .sum();
    kerns + glyphs
}

/// Shrink a multi-operator match to the part of it the replacement changes.
///
/// The producer spaced the operators of a span itself (SolidWorks adds a
/// gap between every word). Rewriting the whole match collapses that
/// spacing; trimming the common prefix and suffix of find and replace keeps
/// every unchanged glyph where the producer put it. At least one find
/// character is kept, because an empty find is the whole-operator pin.
///
/// `None` when nothing trims or the match is in one operator — single
/// operators are left exactly as they were edited before. Otherwise the
/// narrowed anchor and request, and the byte range of `req.find` the
/// narrowed find covers.
fn narrow_span(
    recs: &[OpRec],
    span: Anchor,
    req: &EditRequest,
) -> Option<(Anchor, EditRequest, std::ops::Range<usize>)> {
    if span.first == span.last || req.find.is_empty() {
        return None;
    }
    let (f, r) = (req.find.as_str(), req.replace.as_str());
    let mut pre: usize = f
        .chars()
        .zip(r.chars())
        .take_while(|(a, b)| a == b)
        .map(|(a, _)| a.len_utf8())
        .sum();
    let mut suf: usize = f[pre..]
        .chars()
        .rev()
        .zip(r[pre..].chars().rev())
        .take_while(|(a, b)| a == b)
        .map(|(a, _)| a.len_utf8())
        .sum();
    if pre + suf == f.len() {
        if let Some(c) = f[..pre].chars().next_back() {
            pre -= c.len_utf8();
        } else if let Some(c) = f[f.len() - suf..].chars().next() {
            suf -= c.len_utf8();
        }
    }
    if pre == 0 && suf == 0 {
        return None;
    }
    let (lo, hi) = (span.pos + pre, span.end - suf);
    // Re-derive which operators the narrowed range falls in.
    let mut offset = 0usize;
    let (mut first, mut first_offset, mut last) = (None, 0usize, None);
    for k in span.first..=span.last {
        let Some(OpRec {
            rec: Rec::Show(s), ..
        }) = recs.get(k)
        else {
            continue;
        };
        let len = s.text.len();
        if first.is_none() && lo < offset + len {
            first = Some(k);
            first_offset = offset;
        }
        if first.is_some() && hi <= offset + len {
            last = Some(k);
            break;
        }
        offset += len;
    }
    let (first, last) = (first?, last?);
    let mut narrowed = req.clone();
    narrowed.find = f[pre..f.len() - suf].to_owned();
    narrowed.replace = r[pre..r.len() - suf].to_owned();
    Some((
        Anchor {
            first,
            last,
            pos: lo - first_offset,
            end: hi - first_offset,
        },
        narrowed,
        pre..f.len() - suf,
    ))
}

/// Whether a following absolute `Tm` sits on the anchor's line (`Pass 121.1`).
///
/// True only when the two matrices differ in `e` — the horizontal translation
/// — **and nothing else**. Same `a`/`b`/`c`/`d` means same orientation and
/// scale; same `f` means same baseline. A `Tm` that changes any of those is
/// re-anchoring somewhere new, which ends the line.
///
/// **Same line is not the same question as "after this on the line"**, and
/// reading it as the second was a shipped defect —
/// [`re_anchors_before_anchor`] is the ordering half, and
/// [`reposition_followers`] must ask both. This predicate is deliberately
/// left as the cheap geometric one: the span search
/// ([`find_anchor_span`]) wants *same row* with no ordering claim attached.
///
/// # Why an unknown anchor matrix answers `false`
///
/// [`ShowData::matrix_known`] is false when the walk could not track `Tm`
/// across the operators before the anchor. Without it there is no way to tell
/// a same-line follower from a new line, and the two answers are not equally
/// wrong: leaving a follower where the producer put it shows up as text that
/// may overlap — visible, and recoverable by undo — while shifting one that
/// should not move silently relocates content the operator was not editing.
/// So an unknown matrix shifts nothing, and `FollowerDisposition::Pin`
/// remains available for a caller that wants the tail explicitly held.
pub(crate) fn same_line(anchor: &ShowData, follower: &[f64; 6]) -> bool {
    if !anchor.matrix_known {
        return false;
    }
    let a = &anchor.text_matrix;
    // SCALE AND ROTATION STAY EXACT; THE BASELINE GETS A TOLERANCE.
    //
    // This whole comparison used to be exact, and the reasoning written here
    // was: *"both sides are the producer's own operands, parsed from the same
    // file and never arithmetic'd on … a near-miss means the producer wrote a
    // different number, which is a different line by the only evidence
    // available."*
    //
    // That is sound for `a[0..3]` and it is kept for them — a different scale
    // or rotation IS different text, whatever the magnitude.
    //
    // ⚠ It is FALSE for the baseline, `a[5]`, and a real CAD file is the
    // evidence. SolidWorks advances along one visual line with a `Td` whose
    // vertical is a float round-trip rather than zero:
    //
    // ```text
    // 100.03006 Tz 8.95675 -0.00057 Td
    // ```
    //
    // `Td` translates the line matrix, so `a[5]` moves by that 0.00057 and the
    // exact compare says "different line" — for a displacement ~3,000× smaller
    // than the same note's real line break (`0 -1.72646 Td`). The consequence
    // was not cosmetic: a nine-fragment note could not be edited across its
    // fragments by ANY route, so a balloon-reference list like `8 9 10 11`
    // was uneditable in a drawing whose whole purpose is to be revised
    // (operator-reported, 2026-09-14).
    //
    // The tolerance is the same one the span walk uses for `Td` itself, for
    // the same reason and from the same measurement.
    a[0] == follower[0] && a[1] == follower[1] && a[2] == follower[2] && a[3] == follower[3] && {
        // THE TOLERANCE IS SCALED, AND GETTING THAT WRONG IS WHY THE
        // FIRST CUT FIXED ONE NOTE AND NOT THE ONE BESIDE IT.
        //
        // `SPAN_LINE_DRIFT_TOLERANCE` is in UNSCALED text units, the units
        // `Td` takes. `a[5]` is not: `Td` translates the line matrix, so
        // the drift arrives here multiplied by the matrix's y scale.
        // Measured on the same drawing, two notes apart:
        //
        // ```text
        // Td ty      y scale   a[5] drift
        // -0.00057   13.228    0.0075     note #2 — passed a flat 0.01
        // -0.00661   13.228    0.0870     note #3 — did NOT
        // ```
        //
        // Both are the same producer artefact and both must span. A flat
        // threshold could only have covered both by being loose enough to
        // start swallowing real leading.
        //
        // `hypot(a[2], a[3])` rather than `a[3]` so rotated text — a title
        // block's vertical annotation — keeps a meaningful scale instead of
        // a near-zero one. A degenerate matrix falls back to 1.0 rather
        // than collapsing the tolerance to nothing.
        let y_scale = a[2].hypot(a[3]);
        let scale = if y_scale.is_finite() && y_scale > f64::EPSILON {
            y_scale
        } else {
            1.0
        };
        (a[5] - follower[5]).abs() <= SPAN_LINE_DRIFT_TOLERANCE * scale
    }
}

/// Whether an error means "not in *this* buffer" rather than "not editable".
///
/// See [`plan_edit_anywhere`] for why the distinction decides which error a
/// multi-candidate search reports.
///
/// `pub(crate)` because the session-integrated
/// [`EditSession::edit_text`](crate::edit::EditSession::edit_text) runs the
/// same search over the same candidates and must classify errors the same way.
/// One predicate, two callers -- the `FormatRequest::is_empty` lesson (Pass
/// 19.1), where a re-listed copy of a condition learned about new cases and
/// the original did not.
pub(crate) const fn is_locational_error(e: &EditError) -> bool {
    matches!(
        e,
        EditError::NoMatch { .. } | EditError::PinnedSpanNotFound { .. }
    )
}

/// The form-XObject HARD refusals, applied before any surgery.
///
/// Two triggers, both from the spec corpus's `Pass 119.0` refuse table
/// (`iso32000__ref__form_xobject_text_edit.md` §11):
///
/// - **`R-FX-2` — `/Ref` or `/OPI`.** The stream is a **proxy**: a reference
///   XObject (§8.10.4) stands in for content in *another PDF file*, and an OPI
///   proxy stands in for a high-resolution image held by a prepress system. In
///   both cases the bytes pdfcer can see are a low-fidelity placeholder that a
///   conforming consumer is entitled to replace wholesale with the real thing.
///   Editing text in a proxy is therefore editing something that may never be
///   printed, while the printed article keeps the old text — an edit that
///   *appears* to work and silently does not, which is the exact class rule 4
///   forbids. Refused by name instead.
///
/// - **`R-FX-9` — the nesting guard.** Reported as pdfcer's limit, never as the
///   file's defect: neither ISO edition states any nesting limit for form
///   XObjects (`FX-N9`), so a document deeper than
///   [`MAX_FORM_DEPTH`](crate::text_edit::forms::MAX_FORM_DEPTH) is conforming
///   and pdfcer is the one saying no. (Detected upstream, in the scan; this
///   function refuses the *targeted* form only when it is itself at the
///   guard's edge.)
///
/// # Errors
///
/// [`EditError::Unsupported`] with the trigger named.
pub(crate) fn refuse_unsuitable_form(
    form: &crate::text_edit::forms::FormRef,
) -> Result<(), EditError> {
    if form.dict.contains_key(b"Ref") {
        return Err(EditError::Unsupported(UnsupportedCause::ReferenceXObject));
    }
    if form.dict.contains_key(b"OPI") {
        return Err(EditError::Unsupported(UnsupportedCause::OpiProxy));
    }
    Ok(())
}

/// Every disclosure a form-XObject edit owes the operator (`Pass 119.0`,
/// rule 4).
///
/// **The first one is the reason this whole Pass has a design question.**
/// Nothing in either ISO edition binds a form XObject to a page (`FX-N1`), and
/// §8.10.1 states multi-invocation as the *purpose* of the feature, so an
/// in-place edit of a shared form changes content on pages the operator never
/// opened. That is not a bug to be fixed — it is the file's own structure —
/// and the only honest response is to **say so**, off-canvas, in the report
/// (rule 4 as narrowed by decision 059: render normally, report separately).
pub(crate) fn disclose_form_edit(
    doc: &DocumentView<'_>,
    form: &crate::text_edit::forms::FormRef,
    invocations: Option<&crate::text_edit::forms::InvocationSet>,
    anchor: &ShowData,
    disclosures: &mut Vec<String>,
) {
    let object = form.id.num;
    disclosures.push(format!(
        "the edited text lives inside form XObject {object}, not in the page's own /Contents -- that stream object is what changed; the page dictionary and its content streams are byte-identical to the original."
    ));

    if let Some(set) = invocations {
        if set.is_shared() {
            disclosures.push(format!(
                "SHARED CONTENT: {} -- the edit changed all of them, because the standard binds a form XObject to no page at all (ISO 32000-1 8.10.1) and there is exactly one stream holding these glyphs.",
                set.describe()
            ));
        }
        if set.is_lower_bound() {
            disclosures.push(
                "the shared-content count above is a LOWER BOUND: at least one page could not be scanned completely (a nesting-depth guard or an unreadable form), so this text may appear in more places than reported.".to_owned(),
            );
        }
    }

    if !form.owns_font(doc, &anchor.font_name) {
        disclosures.push(format!(
            "the font this run selects (/{}) is NOT declared in the form's own /Resources -- pdfcer resolved it by inheritance from the page, which ISO 32000-1 7.8.3's fourth bullet requires a reader to do ('All resources that are referenced from those forms ... shall be inherited from the resource dictionary of the page on which they are used'), and which PDF 2.0 no longer calls obsolete. The re-encoding used that inherited font.",
            String::from_utf8_lossy(&anchor.font_name)
        ));
    }

    // `FX-N6` / `R-FX-4`: a form may show text with NO `Tf` of its own and
    // inherit the caller's font, because the nine text state parameters ARE
    // graphics state (Table 52) and §8.10.1 inherits the graphics state at the
    // `Do`. Invoked from two sites with different current fonts, the same
    // bytes render in two typefaces and the run has no single font -- so a
    // re-encode has no defined target. pdfcer detects the *inherited* case and
    // discloses it whenever it is shared; the hard refusal case is the
    // intersection, and is handled by the caller that knows both halves.
    if anchor.font_name.is_empty() {
        disclosures.push(
            "this run selected no font of its own (no Tf inside the form): it inherits the font in force where the form is painted, so the glyphs it renders with depend on the invoking content stream (ISO 32000-1 8.10.1, 8.4.1 Table 52).".to_owned(),
        );
    }

    if form.dict.contains_key(b"OC") {
        disclosures.push(
            "this form is optional content (/OC): it is skipped entirely when its optional-content group is OFF, so the edit may not be visible in every configuration of this document.".to_owned(),
        );
    }
    if form.dict.contains_key(b"StructParent") || form.dict.contains_key(b"StructParents") {
        disclosures.push(
            "this form is tagged content (/StructParent or /StructParents): the operand replacement leaves the marked-content structure intact, but any /ActualText or reading order the structure tree records for it is now STALE.".to_owned(),
        );
    }
}

// ===================================================================
// Locating + matching
// ===================================================================

/// Where a match lives: one show operator, or — `Pass 256.0` — a run of
/// consecutive ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Anchor {
    /// Index of the first show operator of the span in the walk's records.
    pub(crate) first: usize,
    /// Index of the last (`== first` for a single-operator match).
    pub(crate) last: usize,
    /// Character offset of the match within the concatenated text of the
    /// span (`first.text ++ … ++ last.text`).
    pub(crate) pos: usize,
    /// One past the match's last character, in the same coordinate.
    pub(crate) end: usize,
}

/// How far two show operators' horizontal scaling (`Tz`, §9.3.4) may differ
/// and still count as the same run — **as a ratio**, so `0.001` is 0.1 %.
///
/// # Why this is not exact equality, and what it was measured against
///
/// `spannable` compared `th()` with `==`, which is right in principle: a
/// producer that meant a different scale wrote a different number. Real CAD
/// output does not honour that. SolidWorks re-states `Tz` before every
/// fragment of one visual line, with a rounding wobble in the fifth decimal:
///
/// ```text
/// 100.00154 Tz   …SPACERS
/// 100.03006 Tz   8
/// 100.03026 Tz   (space)
/// ```
///
/// That is **0.03 %**, and its visual effect is 0.03 % of an advance — far
/// below a device pixel at any sane zoom. Meanwhile a deliberate `Tz` is
/// condensed or expanded type: 80, 90, 120. The gap between the noise and any
/// real intent is three orders of magnitude, which is what makes a threshold
/// defensible rather than arbitrary.
///
/// `0.001` sits two orders above the observed wobble and two below the
/// smallest deliberate change anyone writes.
pub(crate) const SPAN_H_SCALE_TOLERANCE: f64 = 0.001;

/// How far a `Td`'s vertical displacement may be from zero and still count as
/// staying on the same line, in unscaled text-space units.
///
/// Same producer, same cause: `8.95675 -0.00057 Td` moves the pen **0.00057**
/// down while advancing 8.96 across — a float round-trip, not a line break.
/// The real line break in the same note is `0 -1.72646 Td`, three orders of
/// magnitude larger.
///
/// `0.01` is below any leading a producer would write and above the noise.
const SPAN_LINE_DRIFT_TOLERANCE: f64 = 0.01;

/// Whether two show operators may be edited as ONE run (`Pass 256.0`'s
/// grouping rule — pdfcer's own, documented here because Acrobat's is
/// unpublished): same font RESOURCE NAME and size, same character/word
/// spacing and marked-content sequence, horizontal scale within
/// [`SPAN_H_SCALE_TOLERANCE`], both `Tj`
/// or `TJ`, and the same text-space row — every text-matrix component but
/// the x translation equal, which is what "same baseline" means once a
/// producer's `Td` steps have been folded into the matrix.
fn spannable(a: &ShowData, b: &ShowData) -> bool {
    a.matrix_known
        && b.matrix_known
        && a.font_name == b.font_name
        && a.tf_size == b.tf_size
        && a.mcid == b.mcid
        && a.tc() == b.tc()
        && a.tw() == b.tw()
        // `Tz` within tolerance rather than `==` — see
        // `SPAN_H_SCALE_TOLERANCE`. Every other comparison here stays exact:
        // a different font, size, MCID, char- or word-spacing is a different
        // run by anyone's reading, and only the horizontal scale was measured
        // carrying producer noise.
        && (a.th() - b.th()).abs() <= SPAN_H_SCALE_TOLERANCE
        && matches!(a.op, ShowOp::Tj | ShowOp::TJ)
        && matches!(b.op, ShowOp::Tj | ShowOp::TJ)
        && same_line(a, &b.text_matrix)
}

/// Locate `req.find` as a single operator ([`find_anchor`]) or, failing
/// that, as a span of consecutive spannable operators whose concatenated
/// text contains it (`Pass 256.0`).
///
/// The scan is left to right: from each show operator `i`, operators are
/// appended while [`spannable`] holds and the records between them are
/// only x-only `Td` steps, same-row `Tm`s, or ignorable operators. The
/// first span whose text contains `find` with the match STARTING inside
/// operator `i`'s own text wins — a match that starts later would be found
/// from that later `i`, so spans are never longer than they need to be. A
/// pinned request never spans: the pin names one operator.
pub(crate) fn find_anchor_span(recs: &[OpRec], req: &EditRequest) -> Result<Anchor, EditError> {
    // Where the span search starts. `None` means "scan the page", which is
    // what an unpinned request has always done.
    let span_from = match pinned_start(recs, req)? {
        Start::Found(anchor) => return Ok(anchor),
        Start::From(i) => Some(i),
        Start::Scan => None,
    };
    for i in 0..recs.len() {
        // A pinned spanning search considers exactly one starting operator:
        // the one the caller pointed at. Only it may cross text objects
        // (`cross_object`); an unpinned span stays inside one.
        if span_from.is_some_and(|from| from != i) {
            continue;
        }
        if let Some(anchor) = span_at(recs, i, &req.find, span_from.is_some()) {
            return Ok(anchor);
        }
    }
    Err(not_found(recs, &req.find))
}

/// What the single-operator locator decided before any span search.
enum Start {
    /// The match lies inside one operator.
    Found(Anchor),
    /// A pinned spanning request: search from this operator only.
    From(usize),
    /// Unpinned and not inside one operator: scan the page.
    Scan,
}

fn pinned_start(recs: &[OpRec], req: &EditRequest) -> Result<Start, EditError> {
    match find_anchor(recs, req) {
        Ok(i) => {
            let Some(OpRec {
                rec: Rec::Show(s), ..
            }) = recs.get(i)
            else {
                return Err(EditError::no_match(req.find.clone()));
            };
            let find = effective_find(s, &req.find, req.pinned_span);
            // `find_anchor` returns `Ok(i)` for a resolvable pin WITHOUT
            // consulting `find`, so a pinned `find` that spans operators is
            // absent from this operator's text. Without `spanning_from` that
            // is refused by name rather than resolved to an invented position.
            match s.text.find(find) {
                Some(pos) => Ok(Start::Found(Anchor {
                    first: i,
                    last: i,
                    pos,
                    end: pos + find.len(),
                })),
                None if req.span_from_pin => Ok(Start::From(i)),
                None => Err(EditError::no_match(req.find.clone())),
            }
        }
        Err(e) if req.pinned_span.is_some() || req.find.is_empty() => Err(e),
        Err(_) => Ok(Start::Scan),
    }
}

/// The span of consecutive operators from `i` whose joined text contains
/// `find` with the match starting inside operator `i`, trimmed to the
/// operators the match touches. With `cross`, the span may continue past
/// `ET` into a later text object that [`cross_object::continues_line`].
fn span_at(recs: &[OpRec], i: usize, find: &str, cross: bool) -> Option<Anchor> {
    let Some(OpRec {
        rec: Rec::Show(head),
        ..
    }) = recs.get(i)
    else {
        return None;
    };
    if !matches!(head.op, ShowOp::Tj | ShowOp::TJ) {
        return None;
    }
    let (text, last) = grow_span(recs, i, head, cross);
    if last == i {
        return None;
    }
    let pos = text.find(find).filter(|&pos| pos < head.text.len())?;
    let end = pos + find.len();
    // Trim the span to the operators the match actually touches.
    let mut acc = head.text.len();
    let mut last_used = i;
    for k in (i + 1)..=last {
        if acc >= end {
            break;
        }
        if let Some(OpRec {
            rec: Rec::Show(s), ..
        }) = recs.get(k)
        {
            acc += s.text.len();
            last_used = k;
        }
    }
    // Fits in one operator after all; `find_anchor` would have said so.
    (last_used != i).then_some(Anchor {
        first: i,
        last: last_used,
        pos,
        end,
    })
}

/// Join show operators after `head` while they continue its run: the joined
/// text and the index of the last operator joined.
fn grow_span<'a>(recs: &'a [OpRec], i: usize, head: &'a ShowData, cross: bool) -> (String, usize) {
    let mut text = head.text.clone();
    let mut last = i;
    let mut prev = head;
    // Between `ET` and the next show operator: positioning is re-established
    // absolutely, and the next show operator is judged on its own geometry.
    let mut gap = false;
    for (j, next) in recs.iter().enumerate().skip(i + 1) {
        match &next.rec {
            Rec::EndText if cross => gap = true,
            Rec::Ignore => {}
            Rec::Tm(_) | Rec::Td { .. } if gap => {}
            // `|ty|` within tolerance rather than exactly zero — see
            // `SPAN_LINE_DRIFT_TOLERANCE`.
            Rec::Td { ty, .. } if ty.abs() <= SPAN_LINE_DRIFT_TOLERANCE => {}
            Rec::Tm(m) if same_line(prev, m) => {}
            Rec::Show(s) if gap && cross_object::continues_line(prev, s) => {
                gap = false;
                text.push_str(&s.text);
                (last, prev) = (j, s);
            }
            // Inside the first text object the run is judged against its
            // head, as it always was; after a crossing, against the
            // operator before it, whose MCID is the later object's own.
            Rec::Show(s) if !gap && spannable(if last == i { head } else { prev }, s) => {
                text.push_str(&s.text);
                (last, prev) = (j, s);
            }
            _ => break,
        }
    }
    (text, last)
}

/// The `NoMatch` for a find no single text object contains: when the show
/// operators' text, joined across `ET` in content order, does contain it,
/// the reason names how many text objects the match touches.
fn not_found(recs: &[OpRec], find: &str) -> EditError {
    let mut joined = String::new();
    let mut object_of: Vec<usize> = Vec::new();
    let mut object = 0usize;
    for r in recs {
        match &r.rec {
            Rec::Show(s) => {
                joined.push_str(&s.text);
                object_of.resize(joined.len(), object);
            }
            Rec::EndText => object += 1,
            _ => {}
        }
    }
    let span = joined
        .find(find)
        .filter(|_| !find.is_empty())
        .and_then(|pos| Some((*object_of.get(pos)?, *object_of.get(pos + find.len() - 1)?)));
    let reason = match span {
        Some((first, last)) if last > first => NotFoundReason::SpansTextObjects {
            objects: last - first + 1,
        },
        _ => NotFoundReason::NoSuchText,
    };
    EditError::NoMatch {
        find: find.to_owned(),
        reason,
    }
}

/// Find the anchor operator: the pinned span if given, else the first show
/// operator whose decoded text contains `find`. The single-operator locator
/// every route used before `Pass 256.0`; [`find_anchor_span`] tries it first
/// and only then looks across operators.
pub(crate) fn find_anchor(recs: &[OpRec], req: &EditRequest) -> Result<usize, EditError> {
    for (i, r) in recs.iter().enumerate() {
        let Rec::Show(s) = &r.rec else { continue };
        if let Some(pin) = req.pinned_span {
            if pin_names_operator(r, pin) {
                return Ok(i);
            }
            continue;
        }
        if s.text.contains(&req.find) {
            return Ok(i);
        }
    }
    // The two failures are told apart (`Pass 118.0`). A pinned request never
    // reaches the text search above -- the `continue` skips it -- so reporting
    // `NoMatch(find)` here blamed the operator's own text for a pin that named
    // nothing. See `EditError::PinnedSpanNotFound` for why that sentence has
    // now misled twice.
    if let Some(pin) = req.pinned_span {
        return Err(EditError::PinnedSpanNotFound {
            start: pin.start,
            end: pin.start.saturating_add(pin.len),
        });
    }
    Err(EditError::no_match(req.find.clone()))
}

/// Whether `pin` names the operation `r` — under **either** of the two byte-
/// span conventions this codebase publishes for "the show operator".
///
/// # The defect this function exists to close (found Pass 19.3, live)
///
/// There are two conventions, they disagree, and until this was written the
/// pinned path silently required the one that no caller actually produces:
///
/// | Producer | Span of `(hello) Tj` at offset 23 |
/// |---|---|
/// | [`op_span`] — this module's own walk, recorded into [`OpRec`] | `23..39` (first operand → operator end) |
/// | [`GlyphProvenance::operator_span`](crate::text_extract::GlyphProvenance::operator_span) — the extraction walk (`page.rs`, `self.cur_op_span = op.operator.span`) | `37..39` (the `Tj` token alone) |
///
/// The old test for equality against the FIRST form meant that every request
/// pinned from provenance — which is every request the GUI's Edit Text tool
/// builds, since it pins from `model.provenance(...).operator_span` — failed
/// with [`EditError::NoMatch`] before it ever reached the surgery. Observed
/// in the running application: the shipped property bar's "Apply size"
/// refused with *"text to format (…) was not found in an editable run on the
/// page"* on a perfectly ordinary one-`Tj` page. Two doc comments asserted
/// the opposite — `EditRequest::pinned_span`'s "this surgery … matches the
/// same span", and `page.rs`'s "the surgery locates the operator by exactly
/// this span" — which is presumably why it went unnoticed: the claim was
/// written down, so it was believed.
///
/// # Why accept both rather than pick one
///
/// The two spans are not rival encodings of the same idea; each is correct
/// for its own reader. Extraction publishes the operator *token* because that
/// is what identifies the operator to a consumer that never re-tokenizes the
/// operands. The authoring walk records the operand-inclusive extent because
/// that is the byte range it is about to splice. Forcing either side to adopt
/// the other's convention would change a published field's meaning (and, for
/// provenance, one that is already in operator-visible CLI output) to fix a
/// comparison — so the comparison is what gets fixed.
///
/// # The rule, and why it cannot alias the wrong operator
///
/// A pin names `r` when it **ends where `r` ends** and **starts at or after
/// `r` starts**. Both conventions satisfy that for the operator they mean.
/// Nothing else can: two distinct operations in one stream have distinct end
/// offsets (an operator token is at least one byte and they do not overlap),
/// so `pin.end() == r.end` already identifies the operation uniquely; the
/// start bound is kept as a cheap sanity check that the pin lies inside the
/// operation rather than reaching back over an earlier one.
fn pin_names_operator(r: &OpRec, pin: ByteSpan) -> bool {
    pin.end() == r.end && pin.start >= r.start
}

/// A resolved match within one show operator.
///
/// `pub(crate)` so Pass 14.2's formatting surgery can reuse the identical
/// single-element, contiguous-code-range match the REPLACE surgery uses.
pub(crate) struct MatchRun {
    /// The TJ element the match ENDS in (`== elem` for a single-element
    /// match, which every match was before `Pass 256.0`). When it differs,
    /// `b_hi` is a byte offset within THIS element, and the elements
    /// strictly between `elem` and `elem_hi` — strings and kern numbers
    /// alike — are consumed by the edit.
    pub(crate) elem_hi: usize,
    /// The horizontal displacement, in text-space units already scaled by
    /// `Tfs`/`Th` (i.e. glyph-advance units), contributed by the `TJ` kern
    /// numbers strictly between `elem` and `elem_hi`. Zero for a
    /// single-element match. Part of the OLD advance the replacement
    /// replaces, because those numbers are dropped with the glyphs around
    /// them.
    pub(crate) kern_advance: f64,
    /// Which element the matched codes live in.
    pub(crate) elem: usize,
    /// Byte range within that element's string.
    pub(crate) b_lo: usize,
    pub(crate) b_hi: usize,
    /// The old codes being replaced (for `A_old`).
    ///
    /// `u32`, not `u8` (Pass 29.0): a composite run's CIDs are two bytes, and
    /// narrowing them here dropped every one — which would have made `A_old`
    /// zero and the advance compensation wrong by the whole matched run.
    pub(crate) old_codes: Vec<u32>,
}

/// A replacement encoded for whichever font family the run uses (Pass 29.0).
///
/// The seam that lets one `plan_edit` serve simple and composite runs. Both
/// encoders answer the same two questions — what are the per-code values (for
/// the advance sum) and what bytes go into the show string — and differ only
/// in how they answer them. Keeping the difference here rather than in
/// branches further down is what stopped composite support from being "a
/// change to the whole encoding seam".
#[derive(Default)]
pub(crate) struct EncodedReplacement {
    /// Per-code values, for the §9.4.4 advance sum. `u32` covers a
    /// single-byte code and a 2-byte CID alike.
    pub(crate) codes: Vec<u32>,
    /// The exact bytes to splice into the show string — single bytes for a
    /// simple font, big-endian pairs for `Identity-H`.
    pub(crate) bytes: Vec<u8>,
    /// Encoder-level disclosures (the simple path's R-INV-5 substitutions).
    pub(crate) disclosures: Vec<String>,
}

/// The text a request is **actually** about, resolving *"the whole pinned
/// operator"* (`Pass 145.0`).
///
/// # The problem it removes
///
/// A caller that has already **located** a show operator — by walking the
/// text model and pinning `provenance(..).operator_span` — still had to
/// **describe** it, by handing back a `find` string that pdfcer would then
/// search for inside the very operator the pin had already identified. That
/// is not a formality; a consuming project got it wrong three times in a row,
/// each attempt looking right and each failing differently:
///
/// | attempt | outcome |
/// |---|---|
/// | `find: ""` with a pin | refused — *"empty find text"* |
/// | `find` = the run's `text` | `NoMatch` |
/// | `find` = the glyph-covered bytes | `NoMatch` on some runs |
///
/// The middle row is the one worth understanding, because it is invisible in
/// test data. A `TextRun`'s `text` is **not** in 1:1 correspondence with its
/// glyphs: `/ToUnicode` may map one glyph to **several** characters (ISO
/// 32000-1 §9.10.3) — an `ffl` ligature is one glyph and three characters, a
/// surrogate pair is one glyph and two `char`s. So a `find` rebuilt from a
/// run's text can fail to match the operator's own decoded text even though
/// the pin names that exact operator. Unligatured synthetic fixtures never
/// show it; real typeset copy does.
///
/// # The rule
///
/// An **empty** `find` means *"the whole pinned operator"* — **only** when a
/// `pinned_span` is present. With no pin it stays
/// [`EditError::Unsupported`]`("empty find text")`, because a caller who
/// forgot to pin must get a refusal rather than silent whole-operator
/// behaviour on an operator pdfcer chose for them.
///
/// Returning `&anchor.text` rather than a distinct "whole operator" match
/// path is deliberate: everything downstream — the code-range match, the
/// font-coverage gate, the synthesis gate, the disclosure counts — then sees
/// one string and cannot disagree about what was edited.
///
/// # Scope
///
/// It restyles **the pinned operator only**. A `TextRun` from the text model
/// closes on *geometry* and a producer closes a show operator on whatever its
/// writer felt like, so one run can correspond to more than one operator.
/// Whether that actually occurs is measured by
/// `crates/pdfcer-core/tests/operator_span_invariant.rs`, and the answer is
/// carried in that file rather than asserted here.
pub(crate) fn effective_find<'a>(
    anchor: &'a ShowData,
    find: &'a str,
    pinned_span: Option<ByteSpan>,
) -> &'a str {
    if find.is_empty() && pinned_span.is_some() {
        &anchor.text
    } else {
        find
    }
}

/// Map `find` (a substring of the operator's decoded text) to a contiguous
/// code range within a single string element.
pub(crate) fn match_run(anchor: &ShowData, find: &str) -> Result<MatchRun, EditError> {
    if find.is_empty() {
        return Err(EditError::Unsupported(UnsupportedCause::EmptyFind));
    }
    let pos = anchor
        .text
        .find(find)
        .ok_or_else(|| EditError::no_match(find.to_owned()))?;
    let m = match_range(anchor, pos, pos + find.len(), find)?;
    if m.elem_hi != m.elem {
        // The single-operator, single-element contract `format_text` and
        // every pre-256.0 caller rely on. The cross-element form is reached
        // only through `match_range` by the span path in `plan_edit_target`.
        return Err(EditError::Unsupported(UnsupportedCause::CrossElementTj));
    }
    Ok(m)
}

/// Map a character range `[pos, end)` of `anchor.text` onto the operator's
/// operands — the slots it covers, the element(s) they sit in, and the
/// byte extents within the first and last of those elements (`Pass 256.0`
/// generalisation of the single-element `match_run`).
///
/// A match may cross `TJ` element boundaries here: `elem..=elem_hi` is the
/// element range, `b_lo` is a byte offset in `elem`, `b_hi` in `elem_hi`,
/// and `kern_advance` totals the kern numbers strictly between them (as an
/// advance, so it can be subtracted with the glyphs it separated). `find`
/// is only for the error message.
pub(crate) fn match_range(
    anchor: &ShowData,
    pos: usize,
    end: usize,
    find: &str,
) -> Result<MatchRun, EditError> {
    let matched: Vec<&ShowSlot> = anchor
        .slots
        .iter()
        .filter(|s| s.t0 < end && s.t1 > pos)
        .collect();
    let first = matched
        .first()
        .ok_or_else(|| EditError::no_match(find.to_owned()))?;
    let elem = first.elem;
    let elem_hi = matched.iter().map(|s| s.elem).max().unwrap_or(elem);
    let b_lo = matched
        .iter()
        .filter(|s| s.elem == elem)
        .map(|s| s.byte_in_elem)
        .min()
        .unwrap_or(0);
    let b_hi = matched
        .iter()
        .filter(|s| s.elem == elem_hi)
        .map(|s| s.byte_in_elem + usize::from(s.width))
        .max()
        .unwrap_or(0);
    // TJ numbers between the first and last matched element: each shifts
    // the pen by -n/1000 text-space units, scaled by Tfs and Th (§9.4.3).
    let kern_advance: f64 = anchor
        .elems
        .iter()
        .enumerate()
        .filter(|(i, _)| *i > elem && *i < elem_hi)
        .map(|(_, e)| match e {
            ShowElem::Num(n) => -n / 1000.0 * anchor.tf_size * anchor.th(),
            ShowElem::Str(_) => 0.0,
        })
        .sum();
    let old_codes = matched.iter().map(|s| s.code).collect();
    Ok(MatchRun {
        elem,
        b_lo,
        b_hi,
        old_codes,
        elem_hi,
        kern_advance,
    })
}

/// Every code shown under `font_name` anywhere on the page — the "already
/// carries" set for the embedded-subset floor (a code in use ⇒ its glyph is
/// physically present in the subset program).
pub(crate) fn carried_codes(recs: &[OpRec], font_name: &[u8]) -> BTreeSet<u32> {
    let mut set = BTreeSet::new();
    for r in recs {
        if let Rec::Show(s) = &r.rec
            && s.font_name == font_name
        {
            for slot in &s.slots {
                // Same narrowing rationale as `prefer`: this set answers the
                // SIMPLE-font embedded-subset floor ("is this one-byte code
                // already carried on the page"), so a multi-byte code is
                // dropped rather than truncated. Truncating would add a
                // code the page does not actually carry, which would let
                // R-INV-1 pass on a glyph that is not there. Recorded at full
                // width (Pass 29.0): a composite subset carries CIDs, and
                // narrowing them to `u8` made every composite code look
                // absent — which would refuse every composite edit under the
                // embedded-subset floor, for glyphs that are demonstrably on
                // the page.
                set.insert(slot.code);
            }
        }
    }
    set
}

// ===================================================================
// Font classification (R-INV-2/3/4)
// ===================================================================

/// The classification the font-on-edit gate needs.
///
/// `pub(crate)` so Pass 14.2's family-change surgery can classify the
/// TARGET font with the identical R-INV-2/3/4 refuse triggers before
/// re-encoding the run into it.
pub(crate) struct FontClass {
    pub(crate) embedded: bool,
    pub(crate) subset: bool,
}

/// Whether a Type 0 font writes vertically: its `/Encoding` CMap is a stream
/// whose dictionary has `/WMode 1`, or a predefined CMap whose name ends in
/// `V` (ISO 32000-2 §9.7.5.2, Table 120).
pub(crate) fn writes_vertically(doc: &DocumentView<'_>, font_dict: &Dict) -> bool {
    match font_dict.get(b"Encoding").map(|o| doc.resolve(o)) {
        Some(Object::Name(n)) => n.as_bytes().ends_with(b"V"),
        Some(Object::Stream(s)) => {
            s.dict
                .get(b"WMode")
                .map(|o| doc.resolve(o))
                .and_then(Object::as_int)
                == Some(1)
        }
        _ => false,
    }
}

/// Classify the anchor font and apply the font-level refuse triggers
/// R-INV-2/3/4 (the per-character triggers are the inverse map's job), and
/// refuse vertical writing, whose advances run down rather than across.
pub(crate) fn classify_font(
    doc: &DocumentView<'_>,
    font_dict: &Dict,
    font: &ExtractFont,
) -> Result<FontClass, EditError> {
    let subtype = font_dict
        .get(b"Subtype")
        .map(|o| doc.resolve(o))
        .and_then(Object::as_name)
        .map(|n| n.as_bytes().to_vec())
        .unwrap_or_default();

    // R-INV-4: composite (Type 0 / CIDFont) — now refused ONLY when the
    // font's own map makes editing impossible (Pass 29.0).
    //
    // This used to refuse every composite run, on the stated grounds that
    // "the re-encoding path is single-byte end to end ... making composite
    // runs editable is a change to the whole encoding seam". Surveyed again
    // before starting, that turned out to be substantially untrue, and the
    // parts that were true were already built:
    //
    //  - `ExtractFont::width` already read `Widths::Composite` (`/W`//`/DW`,
    //    §9.7.4.3), so advances needed no new table;
    //  - `emit_literal_string` already escaped arbitrary bytes as three-digit
    //    octal, so a two-byte code needs no hex-string emitter — a literal
    //    string is legal for any bytes (§7.3.4.2);
    //  - `CompositeEncoding` was already written and tested, and nothing
    //    called it;
    //  - `ShowSlot` already carried `code: u32` + `width`, and `b_lo`/`b_hi`
    //    were already computed from that width.
    //
    // What genuinely remained was widening three narrowings that only existed
    // BECAUSE composite runs were refused here — `MatchRun::old_codes`,
    // `glyph_advance`, and `carried_codes` — and choosing an encoder. The
    // refusal had become self-justifying: it was cited as the reason those
    // types could stay single-byte, and their being single-byte was cited as
    // the reason the refusal had to stay.
    //
    // What is still refused, by name and for real reasons: a font whose
    // `/ToUnicode` is absent (nothing says which code produces a character),
    // or non-injective (a ligature or a collision, so the inverse is not a
    // function). Those are properties of the FONT and no amount of pdfcer work
    // fixes them — standing rule R110's distinction, now load-bearing rather
    // than descriptive.
    if subtype.as_slice() == b"Type0" && writes_vertically(doc, font_dict) {
        return Err(EditError::Unsupported(UnsupportedCause::VerticalWriting));
    }
    if subtype.as_slice() == b"Type0" || !font.is_simple() {
        let refuse = |why: String| {
            EditError::Refused(Refusal {
                trigger: RInvTrigger::Composite,
                character: None,
                base_font: font.base_font.clone(),
                remedy_faces: Vec::new(),
                message: format!(
                    "R-INV-4: font '{}' is a composite (Type 0 / CIDFont) run that pdfcer cannot edit in place. {why}",
                    font.base_font
                ),
            })
        };
        match font.to_unicode_cmap() {
            None => {
                return Err(refuse(
                    "This font declares no /ToUnicode character map, so pdfcer cannot tell which code produces a given character. Nothing pdfcer can do makes this editable — the information is not in the file."
                        .to_owned(),
                ));
            }
            Some(cmap) => {
                // `Pass 256.1`: only a map with NOTHING invertible (empty, or
                // over the size ceiling) refuses the font; a collision on some
                // characters is refused per character in `encode_str`.
                if let Err(e) = cmap.partial_inverse() {
                    return Err(refuse(format!(
                        "This font's character map cannot be inverted at all, so pdfcer could not know which code to write back: {e}"
                    )));
                }
                // Invertible: fall through. The run is editable.
            }
        }
    }

    // The descriptor lives on the SIMPLE font dictionary — or, for a Type 0
    // font, on its descendant CIDFont (§9.7.4.1; Table 117 makes it a
    // required entry of the CIDFont dictionary, and it is never on the
    // parent). Reading it from the parent made every composite run
    // "non-embedded" — a false disclosure on most real documents, and it
    // silenced the embedded-subset floor for them (pdfcer-gui, 2026-09-05).
    let descriptor_holder: Dict = if subtype.as_slice() == b"Type0" {
        font_dict
            .get(b"DescendantFonts")
            .map(|o| doc.resolve(o))
            .and_then(Object::as_array)
            .and_then(|a| a.first())
            .map(|o| doc.resolve(o))
            .and_then(Object::as_dict)
            .cloned()
            .unwrap_or_default()
    } else {
        font_dict.clone()
    };
    let descriptor = descriptor_holder
        .get(b"FontDescriptor")
        .map(|o| doc.resolve(o))
        .and_then(Object::as_dict);
    let embedded = descriptor.is_some_and(|d| {
        d.contains_key(b"FontFile") || d.contains_key(b"FontFile2") || d.contains_key(b"FontFile3")
    });
    let flags = descriptor
        .and_then(|d| {
            doc.resolve(d.get(b"Flags").unwrap_or(&Object::Null))
                .as_int()
        })
        .unwrap_or(0);
    // §9.8.2 Table 123: bit 3 (value 4) Symbolic, bit 6 (value 32) Nonsymbolic.
    let symbolic = (flags & 0x4) != 0 && (flags & 0x20) == 0;

    // Is /Encoding a usable (invertible) Name or Dict, vs absent / a stream?
    let encoding_usable = matches!(
        font_dict.get(b"Encoding").map(|o| doc.resolve(o)),
        Some(Object::Name(_) | Object::Dict(_))
    );

    // R-INV-2: symbolic embedded font whose code→glyph relation lives in the
    // font program's built-in cmap; /Encoding is ignored (§9.6.6.4 Branch B),
    // so pdfcer cannot build an invertible table from PDF objects.
    if symbolic && embedded && !encoding_usable {
        return Err(EditError::Refused(Refusal {
            trigger: RInvTrigger::SymbolicNoEncoding,
            character: None,
            base_font: font.base_font.clone(),
            remedy_faces: Vec::new(),
            message: format!(
                "R-INV-2: font '{}' is symbolic with a built-in/custom cmap and no usable \
                 /Encoding (§9.6.6.4 Branch B ignores /Encoding); its code↔glyph relation lives \
                 inside the embedded program, which pdfcer-core does not parse (R21). Editing is \
                 refused.",
                font.base_font
            ),
        }));
    }

    // R-INV-3: the only code↔char relation is /ToUnicode (one-way/lossy, §0),
    // and there is no authoritative /Encoding to invert — the base table was
    // unreadable and a /ToUnicode is present.
    let builtin_unreadable = font.notes.iter().any(|n| {
        matches!(
            n,
            crate::text_extract::font::FontNote::BuiltinEncodingUnreadable
        )
    });
    let has_to_unicode = font_dict.contains_key(b"ToUnicode");
    if !encoding_usable && builtin_unreadable && has_to_unicode {
        return Err(EditError::Refused(Refusal {
            trigger: RInvTrigger::ToUnicodeOnly,
            character: None,
            base_font: font.base_font.clone(),
            remedy_faces: Vec::new(),
            message: format!(
                "R-INV-3: font '{}' relates codes to characters only through /ToUnicode, which is \
                 one-way and lossy (§0) and cannot be inverted; it has no authoritative /Encoding \
                 to invert instead. Editing is refused.",
                font.base_font
            ),
        }));
    }

    Ok(FontClass {
        embedded,
        subset: is_subset_tag(&font.base_font),
    })
}

// ===================================================================
// Geometry + emission
// ===================================================================

/// The §9.4.4 horizontal advance `tx` for one code, in text-space units,
/// under the run's text state. `Tw` applies only to the single byte `0x20`
/// (§9.3.3). Width `w0` comes from the SAME `/Widths`/AFM the render path
/// uses ([`ExtractFont::width`] already scales it to text space).
pub(crate) fn glyph_advance(font: &ExtractFont, code: u32, s: &ShowData) -> f64 {
    glyph_advance_with(
        font,
        code,
        s.tf_size,
        s.tc(),
        s.tw(),
        s.th(),
        font.is_simple(),
    )
}

/// The §9.4.4 advance for one code with **explicit** text-state scalars —
/// the size- and font-independent form Pass 14.2 needs, since a formatting
/// edit varies the font (`font`) and/or the size (`tf_size`) from the run's
/// recorded state while `Tc`/`Tw`/`Th` stay put. [`glyph_advance`] is the
/// Pass-14.1 wrapper that passes the run's own recorded state, so its
/// numbers are byte-for-byte what Pass 14.1 always computed.
pub(crate) fn glyph_advance_with(
    font: &ExtractFont,
    code: u32,
    tf_size: f64,
    tc: f64,
    tw: f64,
    th: f64,
    single_byte: bool,
) -> f64 {
    let w0 = f64::from(font.width(code));
    // The word-spacing rule is SINGLE-BYTE code 32 only (§9.3.3): "Word
    // spacing shall be applied to every occurrence of the single-byte
    // character code 32 ... It shall not apply to occurrences of the byte
    // value 32 in multiple-byte codes." So a composite CID of 32 must NOT
    // take `Tw`, and testing the numeric code alone would wrongly give it one.
    let tw = if code == 0x20 && single_byte { tw } else { 0.0 };
    (w0 * tf_size + tc + tw) * th
}

/// The compensating `TJ` number that consumes `ΔA` so survivors do not move
/// (surgery ref §2): `N = ΔA·1000/(Tfs·Th)`. `None` when the scale is ~0
/// (invisible text advances nothing — pinning is a no-op).
pub(crate) fn compensating_tj(delta: f64, tfs: f64, th: f64) -> Option<f64> {
    let scale = tfs * th;
    if scale.abs() < f64::EPSILON {
        None
    } else {
        Some(delta * 1000.0 / scale)
    }
}

/// Re-emit the anchor operator with the matched codes replaced by
/// `new_codes` and, for PIN, a trailing compensating number.
pub(crate) fn emit_edited_operator(
    anchor: &ShowData,
    m: &MatchRun,
    new_codes: &[u8],
    pin_num: Option<f64>,
) -> Vec<u8> {
    // Build the final element list: the matched element's string has its
    // [b_lo, b_hi) byte range replaced by the new codes.
    let mut elems: Vec<ShowElem> = Vec::new();
    for (i, e) in anchor.elems.iter().enumerate() {
        match e {
            // The element the match starts in: its prefix, the replacement,
            // and — from the element the match ENDS in — the suffix. For a
            // single-element match (`elem_hi == elem`) that is the one
            // element's own suffix, byte for byte what it always was.
            ShowElem::Str(bytes) if i == m.elem => {
                let mut out = Vec::new();
                out.extend_from_slice(bytes.get(..m.b_lo).unwrap_or(&[]));
                out.extend_from_slice(new_codes);
                let tail_src = if m.elem_hi == m.elem {
                    Some(bytes)
                } else {
                    match anchor.elems.get(m.elem_hi) {
                        Some(ShowElem::Str(t)) => Some(t),
                        _ => None,
                    }
                };
                if let Some(t) = tail_src {
                    out.extend_from_slice(t.get(m.b_hi..).unwrap_or(&[]));
                }
                elems.push(ShowElem::Str(out));
            }
            // Strings and kern numbers strictly inside the matched element
            // range, and the end element itself, are consumed (their glyphs
            // were part of the match; their kerning separated glyphs that no
            // longer exist).
            _ if i > m.elem && i <= m.elem_hi => {}
            ShowElem::Str(bytes) => elems.push(ShowElem::Str(bytes.clone())),
            ShowElem::Num(v) => elems.push(ShowElem::Num(*v)),
        }
    }

    let single_str = matches!(elems.as_slice(), [ShowElem::Str(_)]);
    if anchor.op == ShowOp::Tj && single_str && pin_num.is_none() {
        let mut out = Vec::new();
        if let Some(ShowElem::Str(s)) = elems.first() {
            emit_literal_string(&mut out, s);
        }
        out.extend_from_slice(b" Tj");
        return out;
    }

    let mut out = Vec::new();
    out.push(b'[');
    let mut first = true;
    for e in &elems {
        if !first {
            out.push(b' ');
        }
        first = false;
        match e {
            ShowElem::Str(s) => emit_literal_string(&mut out, s),
            ShowElem::Num(v) => emit_number(&mut out, *v),
        }
    }
    if let Some(n) = pin_num {
        out.push(b' ');
        emit_number(&mut out, n);
    }
    out.extend_from_slice(b"] TJ");
    out
}

/// Emit a list of [`ShowElem`]s as one show operator: a lone string as
/// `(s) Tj`; anything else (multiple strings, or strings with `TJ` kerning
/// numbers) as `[ … ] TJ`. This is Pass 14.2's segment emitter — the
/// formatting surgery splits an anchor operator into pre/mid/post element
/// lists and emits each with this. An empty list yields empty bytes; the
/// caller is expected to skip an empty segment rather than emit `[] TJ`.
pub(crate) fn emit_show(elems: &[ShowElem]) -> Vec<u8> {
    if elems.is_empty() {
        return Vec::new();
    }
    if let [ShowElem::Str(s)] = elems {
        let mut out = Vec::new();
        emit_literal_string(&mut out, s);
        out.extend_from_slice(b" Tj");
        return out;
    }
    let mut out = Vec::new();
    out.push(b'[');
    let mut first = true;
    for e in elems {
        if !first {
            out.push(b' ');
        }
        first = false;
        match e {
            ShowElem::Str(s) => emit_literal_string(&mut out, s),
            ShowElem::Num(v) => emit_number(&mut out, *v),
        }
    }
    out.extend_from_slice(b"] TJ");
    out
}

/// Re-emit a `Tm` operator from its six operands.
pub(crate) fn emit_tm(m: [f64; 6]) -> Vec<u8> {
    let mut out = Vec::new();
    for (i, v) in m.iter().enumerate() {
        if i > 0 {
            out.push(b' ');
        }
        emit_number(&mut out, *v);
    }
    out.extend_from_slice(b" Tm");
    out
}

/// Splice sorted, non-overlapping `(start, end, bytes)` edits into `buf`.
pub(crate) fn splice(buf: &[u8], edits: &mut [(usize, usize, Vec<u8>)]) -> Vec<u8> {
    edits.sort_by_key(|e| e.0);
    let mut out = Vec::with_capacity(buf.len());
    let mut cursor = 0usize;
    for (start, end, bytes) in edits.iter() {
        if *start < cursor {
            continue; // defensive: skip an overlapping edit
        }
        if let Some(gap) = buf.get(cursor..*start) {
            out.extend_from_slice(gap);
        }
        out.extend_from_slice(bytes);
        cursor = *end;
    }
    if let Some(tail) = buf.get(cursor..) {
        out.extend_from_slice(tail);
    }
    out
}

// ===================================================================
// Save (incremental, R34/R70)
// ===================================================================

/// The disclosure every edit route adds when the edited page shared a content
/// stream with another page and was given its own copy.
pub(crate) const SHARED_CONTENT_DISCLOSURE: &str = "content: this page shared a content stream \
    with another page, so the edit went into a copy this page alone draws; the other page \
    renders unchanged";

/// Replace the page's first content object with `new_content` (emptying any
/// extra content objects) and save **incrementally**. Returns the appended
/// bytes, the rewritten content object number, how many extras were
/// emptied, and whether the page was decoupled from a content stream another
/// page also draws (see [`write_incremental_with`]).
pub(crate) fn write_incremental(
    doc: &Document,
    page: &Page,
    new_content: &[u8],
) -> Result<(Vec<u8>, u32, u64, bool), EditError> {
    write_incremental_with(doc, page, new_content, &[], Vec::new())
}

/// [`write_incremental`] plus objects the plan requires to exist
/// (`Pass 162.0`).
///
/// `extra` is written into the SAME incremental revision as the content
/// change, which is what makes the result atomic: a revision carrying a
/// content stream that names `/pdfceF1` and no `/pdfceF1` is a document whose
/// text does not render, and a reader has no way to tell that from a corrupt
/// file. An id `extra` names that the base does not define is a **created**
/// object — `DirtySet::replace` appends it and gives it a fresh
/// cross-reference entry (§7.5.6).
///
/// A content stream another page also draws is never rewritten or emptied:
/// the new content goes into a fresh stream (or `contents[0]` when that one is
/// this page's alone), the page's `/Contents` is pointed at it, and the shared
/// streams are left for the pages still drawing them. The returned `bool` is
/// `true` when that happened, so the caller can disclose it.
///
/// `staged` is the start of the update's staging buffer: bytes the caller
/// already placed for `extra_objects`' streams, at spans of
/// `doc.bytes().len() + offset`.
pub(crate) fn write_incremental_with(
    doc: &Document,
    page: &Page,
    new_content: &[u8],
    extra_objects: &[(ObjId, Object)],
    staged: Vec<u8>,
) -> Result<(Vec<u8>, u32, u64, bool), EditError> {
    let first = *page
        .contents
        .first()
        .ok_or(EditError::Unsupported(UnsupportedCause::NoContents))?;
    let mine: BTreeSet<ObjId> = page.contents.iter().copied().collect();
    let mut shared: BTreeSet<ObjId> = BTreeSet::new();
    for other in page_tree::pages(doc)?.iter().filter(|p| p.id != page.id) {
        shared.extend(other.contents.iter().filter(|id| mine.contains(id)));
    }

    let mut dirty = DirtySet::empty();
    let base_len = doc.bytes().len();
    let mut staging: Vec<u8> = staged;

    let content_id = if shared.contains(&first) {
        let taken = extra_objects
            .iter()
            .map(|(id, _)| id.num)
            .max()
            .unwrap_or(0);
        let next = doc.next_object_number().ok_or(EditError::Unsupported(
            UnsupportedCause::ObjectNumbersExhausted,
        ))?;
        ObjId::new(next.max(taken.saturating_add(1)), 0)
    } else {
        first
    };
    let span = stage(&mut staging, base_len, new_content);
    dirty.replace(content_id, make_raw_stream(span, new_content.len()));

    let mut extra = 0u64;
    for id in &mine {
        if *id == content_id || shared.contains(id) {
            continue;
        }
        let empty = stage(&mut staging, base_len, &[]);
        dirty.replace(*id, make_raw_stream(empty, 0));
        extra += 1;
    }

    let mut page_dict = None;
    for (id, value) in extra_objects {
        if *id == page.id && !shared.is_empty() {
            page_dict = Some(value.clone());
        } else {
            dirty.replace(*id, value.clone());
        }
    }
    if !shared.is_empty() {
        let page_dict = page_dict.or_else(|| doc.get(page.id).map(|o| o.value.clone()));
        let Some(Object::Dict(mut dict)) = page_dict else {
            return Err(EditError::Unsupported(UnsupportedCause::PageNotDictionary));
        };
        dict.insert(Name::from(b"Contents"), Object::Reference(content_id));
        dirty.replace(page.id, Object::Dict(dict));
    }

    dirty.set_staging(staging);
    let (bytes, _report) = save_incremental(doc, &dirty, &SaveOptions::identity())?;
    Ok((bytes, content_id.num, extra, !shared.is_empty()))
}

/// Replace one **form XObject's** content stream with `new_content` and save
/// **incrementally** (`Pass 119.0`). Returns the appended bytes.
///
/// # Why this is not `write_incremental` with a different id
///
/// A page content stream's dictionary carries nothing but `/Length` and
/// filtering, so [`make_raw_stream`] can build a whole replacement from
/// scratch. **A form XObject's dictionary is the object's identity**:
/// `/Subtype /Form` and `/BBox` are required (Table 95 in ISO 32000-1, Table
/// 93 in ISO 32000-2), and `/Matrix`, `/Resources`, `/Group`, `/OC`,
/// `/StructParent`, `/PieceInfo` and `/Metadata` all carry meaning that
/// nothing in the content stream reproduces. Rebuilding the dictionary would
/// turn an edited title block into an unclipped, unpositioned, resource-less
/// stream — a file that opens and draws nothing.
///
/// So the dictionary is **preserved key-for-key**, with exactly three changes,
/// each of which has to happen:
///
/// - `/Length` becomes the new byte count (§7.3.8.2 — required, and a wrong
///   one truncates or over-reads the stream).
/// - `/Filter` and `/DecodeParms` are **removed**, because the replacement
///   content is emitted verbatim. Leaving a stale `/FlateDecode` on plain
///   bytes is the one mistake here that produces a file every reader rejects.
/// - `/LastModified` is bumped when the form carries `/PieceInfo`. See
///   [`bump_last_modified`] for why that is conditional.
///
/// Also writes `extra`, objects the plan requires to exist — the
/// form twin of [`write_incremental_with`] (`Pass 162.0`). See that function
/// for why `extra` must land in the same revision.
///
/// # Errors
///
/// [`EditError::Unsupported`] when the object is not a stream, or a write
/// failure from the incremental save.
///
/// `extra` must NOT contain `form_id`: the form is a stream rebuilt here from
/// `form_dict`, and a second write for the same id in one revision means the
/// later silently wins. The caller feeds a patched form dictionary in through
/// `form_dict` instead. `staged` is as for [`write_incremental_with`].
pub(crate) fn write_incremental_form_with(
    doc: &Document,
    form_id: ObjId,
    form_dict: &Dict,
    new_content: &[u8],
    extra: &[(ObjId, Object)],
    staged: Vec<u8>,
) -> Result<Vec<u8>, EditError> {
    debug_assert!(
        !extra.iter().any(|(id, _)| *id == form_id),
        "the form's own object must arrive via `form_dict`, not `extra`"
    );
    let mut dirty = DirtySet::empty();
    let base_len = doc.bytes().len();
    let mut staging: Vec<u8> = staged;
    let span = stage(&mut staging, base_len, new_content);
    dirty.replace(
        form_id,
        make_form_stream(form_dict, span, new_content.len()),
    );
    for (id, value) in extra {
        dirty.replace(*id, value.clone());
    }
    dirty.set_staging(staging);
    let (bytes, _report) = save_incremental(doc, &dirty, &SaveOptions::identity())?;
    Ok(bytes)
}

/// Build the replacement [`Object::Stream`] for an edited form XObject: the
/// original dictionary, `/Length` corrected, filtering dropped, timestamp
/// bumped. See [`write_incremental_form_with`] for why each of those is required.
///
/// `pub(crate)` so the session-integrated path builds the identical object the
/// one-shot path builds — one definition, no drift, the same reason
/// [`make_raw_stream`] is shared.
pub(crate) fn make_form_stream(form_dict: &Dict, span: ByteSpan, len: usize) -> Object {
    let mut dict = form_dict.clone();
    dict.remove(b"Filter");
    dict.remove(b"DecodeParms");
    dict.insert(
        Name::from(b"Length"),
        Object::Integer(i64::try_from(len).unwrap_or(i64::MAX)),
    );
    bump_last_modified(&mut dict);
    Object::Stream(Stream {
        dict,
        data_span: span,
    })
}

/// Mark a form's `/PieceInfo` private data as stale by bumping the form
/// dictionary's `/LastModified` (§14.5).
///
/// # Why this is conditional rather than always
///
/// `FX-N7`: **no `shall` obliges a writer to update `/LastModified` after
/// editing a form's content.** But §14.5 defines the staleness protocol for
/// page-piece dictionaries as an *equality comparison* — a consuming
/// application compares the `/LastModified` in its own `/PieceInfo` entry with
/// the one on the containing dictionary, and treats its private data as valid
/// when they match. So a form whose content pdfcer changed while its
/// `/LastModified` stayed put tells every other application *"nothing has
/// happened here"*, and their cached private data — an Illustrator editable
/// layer, a CAD tool's own model of this block — silently outlives the content
/// it describes. That is the same class of defect as a silent edit, one layer
/// down.
///
/// A form with **no** `/PieceInfo` has no such protocol to break, and adding a
/// `/LastModified` to it would be pdfcer inventing a key nobody asked for,
/// against the minimal-diff invariant. Hence: bump it if the protocol exists,
/// leave the dictionary alone if it does not.
///
/// # Why the timestamp can be absent
///
/// The clock is read only where the platform has one. On `wasm32` targets —
/// the web fork's target, which `pdfcer-core` must keep compiling for —
/// `SystemTime::now` is not implemented and **panics at runtime**, so a
/// conditional here is the difference between a portable engine and one that
/// aborts the browser tab on its first form edit. Where no clock exists the
/// key is left untouched and the edit still succeeds; the staleness protocol
/// is not repaired, which is worse than repairing it and much better than a
/// panic.
fn bump_last_modified(dict: &mut Dict) {
    if !dict.contains_key(b"PieceInfo") {
        return;
    }
    let Some(now) = wall_clock_pdf_date() else {
        return;
    };
    dict.insert(
        Name::from(b"LastModified"),
        Object::String(now.into_bytes()),
    );
}

/// The current time as a §7.9.4 date string (`D:YYYYMMDDHHmmSSZ`), or `None`
/// on a target with no clock. See [`bump_last_modified`] for why the `None`
/// arm exists and must not be turned into a panic or a fabricated constant.
fn wall_clock_pdf_date() -> Option<String> {
    #[cfg(target_family = "wasm")]
    {
        None
    }
    #[cfg(not(target_family = "wasm"))]
    {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_secs();
        let secs = i64::try_from(secs).ok()?;
        // `format_rfc3339_utc` yields `YYYY-MM-DDTHH:MM:SSZ`; §7.9.4 wants the same
        // fields with no separators behind a `D:` prefix. Reusing the crate's
        // one calendar implementation rather than writing a second one is the
        // point — see `civil_time`'s own header on why that file exists.
        let iso = crate::civil_time::format_rfc3339_utc(secs);
        let digits: String = iso.chars().filter(char::is_ascii_digit).collect();
        Some(format!("D:{digits}Z"))
    }
}

/// Append `bytes` to staging and return their combined-space span.
fn stage(staging: &mut Vec<u8>, base_len: usize, bytes: &[u8]) -> ByteSpan {
    let start = base_len + staging.len();
    staging.extend_from_slice(bytes);
    ByteSpan::new(start, bytes.len())
}

/// A raw (unfiltered) content stream object with the given data span and
/// length — the edited content is emitted verbatim, no `/Filter`.
///
/// `pub(crate)` so the session-integrated
/// [`EditSession::edit_text`](crate::edit::EditSession::edit_text) (Pass 14.3
/// §0.2) builds the identical replacement Stream object the free-function
/// `write_incremental` path builds — one definition, no drift.
pub(crate) fn make_raw_stream(span: ByteSpan, len: usize) -> Object {
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

// ===================================================================
// Small helpers
// ===================================================================

/// The byte span (operands + operator) of a whole operation.
fn op_span(op: &Operation<'_>) -> (usize, usize) {
    let start = op
        .operands
        .first()
        .map_or(op.operator.span.start, |t| t.span.start);
    (start, op.operator.span.end())
}

/// The `/MCID` integer of a `BDC`/`BMC` operator, if its property operand is
/// an inline dict carrying one (§14.7.4.2). A named property resource is not
/// resolved in the first cut — its MCID is treated as absent.
fn mcid_of(doc: &DocumentView<'_>, op: &Operation<'_>) -> Option<i64> {
    for t in op.operands {
        if let ContentTokenKind::Operand(Object::Dict(d)) = &t.kind {
            return doc.resolve(d.get(b"MCID")?).as_int();
        }
    }
    None
}

/// Resolve a `/Font /<name>` resource to its font dictionary.
pub(crate) fn resolve_font_dict<'a>(
    doc: &'a DocumentView<'a>,
    resources: &'a Dict,
    name: &[u8],
) -> Option<&'a Dict> {
    let fonts = resources
        .get(b"Font")
        .map(|o| doc.resolve(o))
        .and_then(Object::as_dict)?;
    fonts
        .get(name)
        .map(|o| doc.resolve(o))
        .and_then(Object::as_dict)
}

/// Resolve a `/Font /<name>` resource to an [`ExtractFont`].
fn resolve_font(doc: &DocumentView<'_>, resources: &Dict, name: &[u8]) -> Option<ExtractFont> {
    resolve_font_dict(doc, resources, name).map(|d| ExtractFont::resolve(doc, d))
}

/// Whether a `/BaseFont` name carries a §9.6.4 subset tag (`ABCDEF+…`):
/// exactly six uppercase letters then `+`.
pub(crate) fn is_subset_tag(base_font: &str) -> bool {
    matches!(base_font.split_once('+'), Some((tag, _))
        if tag.len() == 6 && tag.bytes().all(|b| b.is_ascii_uppercase()))
}

/// The three-trust-level disclosure the core can produce (Embedded vs
/// NonEmbedded); the shell refines NonEmbedded into Bundled/Supplied.
pub(crate) fn trust_disclosure(embedded: bool, base_font: &str) -> String {
    if embedded {
        format!(
            "font: '{base_font}' has an embedded program; the edit renders with the document's \
             own glyphs (GlyphSource::Embedded)."
        )
    } else {
        format!(
            "font: '{base_font}' is NON-embedded; a bundled Base-14 substitute or an \
             operator-supplied face (--font-dir, decision 012) renders the edited glyphs \
             (shapes only — positions come from /Widths). The shell reports Bundled vs Supplied."
        )
    }
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

    /// A note split across show operators the way a real CAD exporter splits
    /// one — re-stating `Tz` and nudging `Td`'s vertical by a float
    /// round-trip between every fragment.
    ///
    /// # Why synthetic, and what the numbers are
    ///
    /// The evidence is the operator's own 36-sheet SOLIDWORKS drawing, which
    /// cannot be committed (rule 7 — synthetic or rights-cleared only). The
    /// NUMBERS are that file's, measured: `Tz` wobbling in the fifth decimal
    /// around 100, and a `Td` vertical of `-0.00661` against a 13.22835 text
    /// scale — the larger of the two drifts on that page, and the one a flat
    /// tolerance missed.
    fn wobbling_note() -> Vec<u8> {
        helvetica_pdf(concat!(
            "BT /F1 1 Tf 13.22835 0 0 13.22835 72 700 Tm\n",
            "99.99553 Tz (#3 BOLT ITEM ) Tj\n",
            "100.03006 Tz 7.15247 -0.00661 Td (14) Tj\n",
            "99.97365 Tz 1.70167 0.00661 Td (  IN PLACE.) Tj\n",
            "ET\n",
        ))
    }

    /// A BALLOON REFERENCE SPLIT ACROSS SHOW OPERATORS IS EDITABLE
    /// (operator-reported, 2026-09-14).
    ///
    /// `ITEM 14` spans two fragments. Before this, NO route reached it — the
    /// plain find refused and so did the pinned-span route — because
    /// `spannable` compared `Tz` exactly and `same_line` compared the baseline
    /// exactly, and this producer perturbs both between every fragment of one
    /// visual line.
    ///
    /// An operator renumbering a balloon on a revision saw a note he could
    /// read and could not change, refused with *"not found in an editable
    /// run"* about text plainly on the page.
    #[test]
    fn a_reference_split_across_wobbling_fragments_is_editable() {
        let doc = crate::document::Document::from_bytes(wobbling_note()).expect("loads");
        let mut s = crate::edit::EditSession::new(doc);
        // The change must straddle the fragment boundary: `ITEM 14` ->
        // `ITEM 16` narrows to the `4` alone, which sits in one operator.
        let report = s
            .edit_text(
                &EditRequest::find_replace(0, "ITEM 14", "ITEMS 24"),
                &EditOptions::default(),
            )
            .expect("a reference split across fragments must be editable");
        assert!(
            report.operators_spanned >= 2,
            "the edit must have SPANNED fragments — matching inside one means \
             the fixture stopped reproducing the defect: {report:?}"
        );
    }

    /// THE SCALING IS THE HALF THAT WAS GOT WRONG FIRST, so it is pinned
    /// separately.
    ///
    /// `SPAN_LINE_DRIFT_TOLERANCE` is in unscaled text units; `a[5]` carries
    /// the drift already multiplied by the matrix's y scale. The first cut
    /// compared them directly, which fixed a note whose drift was
    /// `0.00057 × 13.2 = 0.0075` and left the one beside it —
    /// `0.00661 × 13.2 = 0.087` — still refusing. Same producer, same page,
    /// one under a flat `0.01` and one over.
    ///
    /// The same content at a 1.0 text scale must behave identically, which it
    /// cannot if the comparison is unscaled.
    #[test]
    fn the_drift_tolerance_scales_with_the_text_matrix() {
        let bytes = helvetica_pdf(concat!(
            "BT /F1 1 Tf 1 0 0 1 72 700 Tm\n",
            "99.99553 Tz (#3 BOLT ITEM ) Tj\n",
            "100.03006 Tz 7.15247 -0.00661 Td (14) Tj\n",
            "ET\n",
        ));
        let doc = crate::document::Document::from_bytes(bytes).expect("loads");
        let mut s = crate::edit::EditSession::new(doc);
        s.edit_text(
            &EditRequest::find_replace(0, "ITEM 14", "ITEM 16"),
            &EditOptions::default(),
        )
        .expect("the same wobble at a 1.0 text scale must span too");
    }

    /// AND THE TOLERANCE MUST NOT SWALLOW A REAL LINE BREAK — the assertion
    /// that stops the two above from being bought with a loosened guard.
    ///
    /// The same note's genuine leading is `0 -1.72646 Td`, three orders of
    /// magnitude beyond the drift. Two lines must stay two lines.
    #[test]
    fn a_real_line_break_still_separates_two_lines() {
        let bytes = helvetica_pdf(concat!(
            "BT /F1 1 Tf 13.22835 0 0 13.22835 72 700 Tm\n",
            "99.99553 Tz (FIRST LINE) Tj\n",
            "99.99553 Tz 0 -1.72646 Td (SECOND LINE) Tj\n",
            "ET\n",
        ));
        let doc = crate::document::Document::from_bytes(bytes).expect("loads");
        let mut s = crate::edit::EditSession::new(doc);
        assert!(
            s.edit_text(
                &EditRequest::find_replace(0, "FIRST LINESECOND", "X"),
                &EditOptions::default(),
            )
            .is_err(),
            "a real line break must still end a span — if this succeeds the \
             tolerance has grown past the leading it must never reach"
        );
    }

    /// A minimal one-page PDF with a Helvetica (WinAnsi, non-embedded) run.
    /// `content` is the page content stream; the font is object 5.
    fn helvetica_pdf(content: &str) -> Vec<u8> {
        let mut objects: Vec<(u32, Vec<u8>)> = Vec::new();
        objects.push((1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()));
        objects.push((
            2,
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 612 792] \
              /Resources << /Font << /F1 5 0 R >> >> >>"
                .to_vec(),
        ));
        objects.push((
            3,
            b"<< /Type /Page /Parent 2 0 R /Contents 4 0 R >>".to_vec(),
        ));
        let body = content.as_bytes();
        let mut s = format!("<< /Length {} >>\nstream\n", body.len()).into_bytes();
        s.extend_from_slice(body);
        s.extend_from_slice(b"\nendstream");
        objects.push((4, s));
        objects.push((
            5,
            b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"
                .to_vec(),
        ));

        let mut out = b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n".to_vec();
        let mut offsets = std::collections::BTreeMap::new();
        for (num, obj) in &objects {
            offsets.insert(*num, out.len());
            out.extend_from_slice(format!("{num} 0 obj\n").as_bytes());
            out.extend_from_slice(obj);
            out.extend_from_slice(b"\nendobj\n");
        }
        let xref_at = out.len();
        let highest = 5u32;
        out.extend_from_slice(format!("xref\n0 {}\n", highest + 1).as_bytes());
        out.extend_from_slice(b"0000000000 65535 f \n");
        for num in 1..=highest {
            match offsets.get(&num) {
                Some(off) => out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes()),
                None => out.extend_from_slice(b"0000000000 65535 f \n"),
            }
        }
        out.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n",
                highest + 1
            )
            .as_bytes(),
        );
        out
    }

    fn extract_first_page_text(bytes: &[u8]) -> String {
        let doc = Document::from_bytes(bytes.to_vec()).unwrap();
        let pages = page_tree::pages(&doc).unwrap();
        let page =
            crate::text_extract::extract_page(&doc, &pages[0], 0, &Default::default()).unwrap();
        page.sourced_text()
    }

    #[test]
    fn reflow_edit_only_changes_edited_stream_and_reextracts() {
        let src = helvetica_pdf("BT /F1 12 Tf 72 700 Td (teh cat) Tj ET\n");
        let doc = Document::from_bytes(src.clone()).unwrap();
        let out = edit_text(
            &doc,
            &EditRequest::find_replace(0, "teh", "the"),
            &EditOptions::default(),
        )
        .unwrap();
        // Incremental save ⇒ the original file is a byte-prefix of the output
        // (every untouched object is verbatim).
        assert_eq!(out.bytes.get(..src.len()), Some(src.as_slice()));
        assert!(out.bytes.len() > src.len());
        // The edit round-trips: the corrected text extracts, the typo is gone.
        let text = extract_first_page_text(&out.bytes);
        assert!(text.contains("the cat"), "got {text:?}");
        assert!(!text.contains("teh"));
        assert_eq!(out.report.glyph_source, EditGlyphSource::NonEmbedded);
        assert!(!out.report.subset);
    }

    #[test]
    fn tm_follower_is_repositioned_by_delta() {
        // "Hello " (Td) then "World" re-anchored by an absolute Tm on the
        // same line. Editing "Hello"->"Hi" shortens the run, so the follower
        // Tm's e must decrease by |ΔA|.
        let src =
            helvetica_pdf("BT /F1 12 Tf 100 700 Td (Hello ) Tj 1 0 0 1 240 700 Tm (World) Tj ET\n");
        let doc = Document::from_bytes(src).unwrap();
        let out = edit_text(
            &doc,
            &EditRequest::find_replace(0, "Hello", "Hi"),
            &EditOptions::default(),
        )
        .unwrap();
        assert_eq!(out.report.followers_repositioned, 1);
        assert!(out.report.advance_delta < 0.0, "shorter run ⇒ negative ΔA");
        let text = extract_first_page_text(&out.bytes);
        assert!(text.contains("Hi"));
        assert!(text.contains("World"));
    }

    /// A RE-ANCHOR **BACKWARDS** ON THE SAME BASELINE IS NOT THIS LINE'S
    /// TAIL, AND MUST NOT SHIFT (`Pass 306.0`).
    ///
    /// `same_line` answers *"same baseline?"* and the follower walk was
    /// reading that as *"the rest of the line?"*. Those come apart the moment
    /// a producer writes a line's pieces out of visual order, and SolidWorks
    /// does exactly that for a numbered note — the note's TEXT first, its
    /// BULLET second, to the left, on the same baseline:
    ///
    /// ```text
    /// 100.00423 Tz 5.66931 -1 Td  <TOLERANCE :->Tj     ← the anchor
    ///  99.94655 Tz -5.66931 0 Td  <3.>Tj               ← same row, 28 pt LEFT
    ///  99.82585 Tz 5.66931 -1 Td  <X/XX: …>Tj          ← the next line
    /// ```
    ///
    /// Shortening the anchor moved the bullet by `ΔA` — content the operator
    /// had not touched — and compensated the line below it, so the file
    /// round-tripped perfectly and the page was wrong. Operator-reported
    /// 2026-09-15: *"when I edit the line #3, after I am done the whole line
    /// shifts position"*; the bullet is the part of the line the eye tracks.
    ///
    /// The fixture below is that shape in miniature, and the assertion is the
    /// strong one — the backwards-anchored run's operands survive **verbatim**,
    /// not merely "close".
    #[test]
    fn a_backwards_re_anchor_on_the_same_baseline_is_not_a_follower() {
        let src = helvetica_pdf(
            "BT /F1 12 Tf 200 700 Td (TOLERANCE) Tj -60 0 Td (3.) Tj 60 -14 Td (X/XX) Tj ET\n",
        );
        let doc = Document::from_bytes(src).unwrap();
        let out = edit_text(
            &doc,
            &EditRequest::find_replace(0, "TOLERANCE", "TOL"),
            &EditOptions::default(),
        )
        .unwrap();
        assert!(out.report.advance_delta < 0.0, "shorter run ⇒ negative ΔA");
        assert_eq!(
            out.report.followers_repositioned, 0,
            "nothing on this line follows the anchor"
        );
        // The bullet's own step, and the next line's, are byte-verbatim. This
        // is the assertion that would have caught the defect: a geometry check
        // on the bullet would ALSO have caught it, but a check on the rendered
        // page would not have — the compensation kept everything downstream in
        // place, so only the one glyph pair moved.
        let body = String::from_utf8_lossy(&out.bytes).into_owned();
        assert!(body.contains("-60 0 Td"), "bullet step unchanged: {body}");
        assert!(body.contains("60 -14 Td"), "next line unchanged");
    }

    /// The same guard must not suppress a REAL tail — the forward case still
    /// reflows, which is what stops the fix above from being an over-broad
    /// "never move a `Td` follower".
    #[test]
    fn a_forward_re_anchor_on_the_same_baseline_still_reflows() {
        let src = helvetica_pdf("BT /F1 12 Tf 100 700 Td (Hello ) Tj 60 0 Td (World) Tj ET\n");
        let doc = Document::from_bytes(src).unwrap();
        let out = edit_text(
            &doc,
            &EditRequest::find_replace(0, "Hello", "Hi"),
            &EditOptions::default(),
        )
        .unwrap();
        assert_eq!(
            out.report.followers_repositioned, 1,
            "the tail of the line is a follower and moves"
        );
    }

    /// A `Tm` ON A DIFFERENT BASELINE IS A NEW LINE, AND MUST NOT SHIFT
    /// (`Pass 121.1`).
    ///
    /// The reflow used to shift every following `Tm` until a
    /// `Td`/`TD`/`T*`/`'`/`"` boundary — and a content stream that positions
    /// every run with `Tm` and never emits `Td` has **no boundary at all**, so
    /// one edit slid the entire rest of the text object sideways.
    ///
    /// Measured on the operator's own benchmark CAD drawing: a four-character
    /// edit reported **1,676 followers repositioned**, and a render diff put
    /// **34,059 changed pixels across the whole page**. After the fix: 0
    /// followers, **42 changed pixels** in a 20x7 box — one label. An edit
    /// that wrecks a drawing is worse than the "editing does nothing" it
    /// replaced, and this is the assertion that keeps it fixed.
    #[test]
    fn a_tm_on_another_baseline_is_a_new_line_and_does_not_shift() {
        // Two runs, each absolutely placed, on DIFFERENT baselines -- the
        // shape of every CAD label set. Nothing follows "Hello" on its line.
        let src = helvetica_pdf(
            "BT /F1 12 Tf 1 0 0 1 100 700 Tm (Hello ) Tj 1 0 0 1 240 400 Tm (World) Tj ET
",
        );
        let doc = Document::from_bytes(src).unwrap();
        let out = edit_text(
            &doc,
            &EditRequest::find_replace(0, "Hello", "Hi"),
            &EditOptions::default(),
        )
        .unwrap();
        assert_eq!(
            out.report.followers_repositioned, 0,
            "a run on another baseline is not part of the edited line"
        );
        // The follower's own operands survive verbatim -- the strongest form
        // of "it did not move", since a shifted `Tm` is re-emitted and a
        // left-alone one is spliced past.
        let bytes = String::from_utf8_lossy(&out.bytes).into_owned();
        assert!(
            bytes.contains("240") && bytes.contains("400"),
            "the untouched Tm must be re-emitted byte-verbatim"
        );
        let text = extract_first_page_text(&out.bytes);
        assert!(text.contains("Hi") && text.contains("World"));
    }

    /// The same-line case is unaffected by the fix above, and a scale change
    /// ends the line.
    ///
    /// Written as ONE test over two streams because the property is a
    /// boundary: `same_line` must say yes to the first and no to the second,
    /// and asserting only the "yes" half would pass against a predicate that
    /// always says yes -- which is exactly the pre-fix behaviour.
    #[test]
    fn same_line_shifts_and_a_scale_change_ends_the_line() {
        let same = helvetica_pdf(
            "BT /F1 12 Tf 1 0 0 1 100 700 Tm (Hello ) Tj 1 0 0 1 240 700 Tm (World) Tj ET
",
        );
        let out = edit_text(
            &Document::from_bytes(same).unwrap(),
            &EditRequest::find_replace(0, "Hello", "Hi"),
            &EditOptions::default(),
        )
        .unwrap();
        assert_eq!(out.report.followers_repositioned, 1, "same baseline shifts");

        let scaled = helvetica_pdf(
            "BT /F1 12 Tf 1 0 0 1 100 700 Tm (Hello ) Tj 2 0 0 2 240 700 Tm (World) Tj ET
",
        );
        let out = edit_text(
            &Document::from_bytes(scaled).unwrap(),
            &EditRequest::find_replace(0, "Hello", "Hi"),
            &EditOptions::default(),
        )
        .unwrap();
        assert_eq!(
            out.report.followers_repositioned, 0,
            "a scale change re-anchors: treated as a new line, deliberately conservative"
        );
    }

    #[test]
    fn missing_glyph_char_is_refused_never_written() {
        // WinAnsi Helvetica has no code for an astral char ⇒ R-INV-8 refusal.
        let src = helvetica_pdf("BT /F1 12 Tf 72 700 Td (hi) Tj ET\n");
        let doc = Document::from_bytes(src).unwrap();
        let err = edit_text(
            &doc,
            &EditRequest::find_replace(0, "hi", "h\u{1D54F}"),
            &EditOptions::default(),
        )
        .unwrap_err();
        match err {
            EditError::Refused(r) => assert_eq!(r.trigger, RInvTrigger::BeyondRepertoire),
            other => panic!("expected a refusal, got {other:?}"),
        }
    }

    #[test]
    fn pinned_span_locates_the_exact_operator() {
        // Two identical runs; pin the SECOND by its operator span.
        let content = "BT /F1 12 Tf 72 700 Td (cat) Tj 72 680 Td (cat) Tj ET\n";
        let src = helvetica_pdf(content);
        // The second "(cat) Tj" span in the decoded buffer.
        let start = content.rfind("(cat) Tj").unwrap();
        let span = ByteSpan::new(start, "(cat) Tj".len());
        let doc = Document::from_bytes(src).unwrap();
        let mut req = EditRequest::find_replace(0, "cat", "dog");
        req.pinned_span = Some(span);
        let out = edit_text(&doc, &req, &EditOptions::default()).unwrap();
        let text = extract_first_page_text(&out.bytes);
        // First run unchanged, second edited.
        assert!(text.contains("cat"));
        assert!(text.contains("dog"));
    }

    /// **The Pass-19.3 regression fixture.** A pin taken from
    /// `GlyphProvenance::operator_span` — the operator TOKEN alone, which is
    /// what every GUI request carries — must locate the same operator as the
    /// operand-inclusive span the test above uses.
    ///
    /// Before `pin_names_operator`, this returned `NoMatch`, which meant the
    /// shipped Edit Text property bar could not apply anything at all. The
    /// test pins the SECOND of two identical runs, so "it accidentally found
    /// the right one" is not a way to pass.
    #[test]
    fn a_pin_taken_from_provenance_locates_the_same_operator() {
        let content = "BT /F1 12 Tf 72 700 Td (cat) Tj 72 680 Td (cat) Tj ET\n";
        let src = helvetica_pdf(content);
        let doc = Document::from_bytes(src).unwrap();

        // Exactly what the extraction walk publishes: `op.operator.span`.
        let tj = content.rfind("Tj").unwrap();
        let mut req = EditRequest::find_replace(0, "cat", "dog");
        req.pinned_span = Some(ByteSpan::new(tj, "Tj".len()));

        let out = edit_text(&doc, &req, &EditOptions::default()).unwrap();
        let text = extract_first_page_text(&out.bytes);
        assert!(text.contains("cat"), "the FIRST run is untouched: {text}");
        assert!(text.contains("dog"), "the SECOND run was edited: {text}");
    }

    /// …and the pin still discriminates. A span that ends inside a different
    /// operator must not silently match a neighbour — otherwise the fix would
    /// have traded a refusal for the far worse failure of editing the wrong
    /// run.
    ///
    /// # THIS TEST ASSERTED THE DEFECT, and that is worth recording
    ///
    /// It used to assert [`EditError::NoMatch`] — *"text to edit (`"cat"`) was
    /// not found in an editable run on the page"* — for a request whose text
    /// is plainly on the page twice. The refusal was **correct**; the
    /// *sentence* blamed the operator's own text for a pin that named nothing,
    /// and this test pinned that sentence in place as expected behaviour.
    ///
    /// **A test that codifies a diagnosis nobody checked is how a misleading
    /// message survives a refactor.** This one did, through `Pass 19.3`'s
    /// investigation of the identical symptom, and the consuming shell spent a
    /// second investigation on it on 2026-08-20 — which is what finally split
    /// the variant (`Pass 118.0`).
    ///
    /// The behaviour under test is unchanged: the pin still refuses to match a
    /// neighbour. Only the name of the refusal moved, and now it is the name
    /// that is asserted.
    #[test]
    fn a_pin_that_names_no_operator_still_refuses() {
        let content = "BT /F1 12 Tf 72 700 Td (cat) Tj 72 680 Td (cat) Tj ET\n";
        let src = helvetica_pdf(content);
        let doc = Document::from_bytes(src).unwrap();
        let mut req = EditRequest::find_replace(0, "cat", "dog");
        // Ends one byte short of the first `Tj`, so it names nothing.
        let tj = content.find("Tj").unwrap();
        req.pinned_span = Some(ByteSpan::new(tj, 1));
        let err = edit_text(&doc, &req, &EditOptions::default()).unwrap_err();
        assert!(
            matches!(err, EditError::PinnedSpanNotFound { .. }),
            "the pin named nothing -- the TEXT is not the problem and the message must not say it is: {err}"
        );
        // And the message must not contain the find text, which is exactly
        // what made the old one mislead.
        let rendered = err.to_string();
        assert!(
            !rendered.contains("cat"),
            "the refusal must not name the operator's own text: {rendered}"
        );
    }

    // -- Pass 19.0: the authoring walk's text-state model ---------------

    /// Run the authoring [`Walk`] over a page's content and return the
    /// recorded show operators, in stream order.
    fn show_records(content: &str) -> Vec<ShowData> {
        let src = helvetica_pdf(content);
        let doc = Document::from_bytes(src).unwrap();
        let pages = crate::page_tree::pages(&doc).unwrap();
        let page = &pages[0];
        let view = doc.view();
        let stream = ContentStream::from_page(&view, page).unwrap();
        let mut walk = Walk::new(&view, &page.resources);
        for op in stream.operations() {
            walk.operation(&op, &stream.buf);
        }
        walk.recs
            .into_iter()
            .filter_map(|r| match r.rec {
                Rec::Show(s) => Some(*s),
                _ => None,
            })
            .collect()
    }

    /// The regression this slice exists to fix. Before Pass 19.0 the
    /// authoring walk had **no `b"Ts"` arm and no `b"Tr"` arm**, so an
    /// ambient rise or rendering mode was invisible to every formatting
    /// surgery — which is why pdfcer could not restore one.
    #[test]
    fn the_authoring_walk_tracks_rise_and_render_mode() {
        let recs = show_records("BT /F1 12 Tf 3 Ts 2 Tr 72 700 Td (hi) Tj ET\n");
        assert_eq!(recs.len(), 1);
        let ts = &recs[0].text_state;
        assert_eq!(ts.rise.value, 3.0, "Ts was not tracked");
        assert_eq!(ts.render_mode.value, 2.0, "Tr was not tracked");
        assert_eq!(ts.restore_bytes(TextStateParam::Rise).unwrap(), b"3 Ts");
        assert_eq!(
            ts.restore_bytes(TextStateParam::RenderMode).unwrap(),
            b"2 Tr"
        );
    }

    /// §8.4.2/§9.3: text state is graphics state, so `Q` discards whatever
    /// was set since the matching `q`. The walk had no `q`/`Q` arms at all
    /// before Pass 19.0, so the second run below inherited the first run's
    /// state — and a restore built on that would have written a `3 Ts` into
    /// a stream that did not have one.
    #[test]
    fn q_and_q_restore_every_text_state_parameter() {
        let recs = show_records(
            "q 0.5 Tc 3 Ts 2 Tr 90 Tz 1 Tw BT /F1 12 Tf 72 700 Td (in) Tj ET Q \
             BT /F1 12 Tf 72 680 Td (out) Tj ET\n",
        );
        assert_eq!(recs.len(), 2);

        let inside = &recs[0].text_state;
        assert_eq!(inside.char_spacing.value, 0.5);
        assert_eq!(inside.rise.value, 3.0);
        assert_eq!(inside.render_mode.value, 2.0);
        assert_eq!(inside.h_scale.value, 90.0);
        assert_eq!(inside.word_spacing.value, 1.0);

        let outside = &recs[1].text_state;
        assert_eq!(
            *outside,
            AmbientTextState::initial(),
            "everything set inside the q … Q bracket must be discarded by the Q"
        );
        // …and therefore restores to the Table 105 defaults, not to the
        // bracket's values.
        assert_eq!(
            outside.restore_bytes(TextStateParam::Rise).unwrap(),
            b"0 Ts"
        );
        assert_eq!(
            outside.restore_bytes(TextStateParam::HorizScale).unwrap(),
            b"100 Tz"
        );
    }

    /// `Q` also restores the fill colour, which is likewise graphics state
    /// (§8.6.8) and was likewise leaking past the bracket.
    #[test]
    fn q_and_q_restore_the_fill_colour_too() {
        let recs = show_records(
            "q 1 0 0 rg BT /F1 12 Tf 72 700 Td (red) Tj ET Q \
             BT /F1 12 Tf 72 680 Td (black) Tj ET\n",
        );
        assert_eq!(recs.len(), 2);
        assert!(matches!(recs[0].fill_color, FillState::Device { .. }));
        assert_eq!(
            recs[1].fill_color,
            FillState::Default,
            "the Q must put the fill colour back to the §8.6.8 default"
        );
    }

    /// R88 tier 2, on the authoring path: the restore re-emits the operand
    /// **as written**, so a producer's `0.5000` does not come back as
    /// `0.5`. A renormalized number is a diff in bytes pdfcer claims not to
    /// have logically touched.
    #[test]
    fn the_authoring_walk_keeps_raw_operand_bytes_for_restore() {
        let recs = show_records("BT /F1 12 Tf 0.5000 Tc 72 700 Td (hi) Tj ET\n");
        let ts = &recs[0].text_state;
        assert_eq!(ts.char_spacing.value, 0.5);
        assert_eq!(
            ts.restore_bytes(TextStateParam::CharSpacing).unwrap(),
            b"0.5000 Tc"
        );
    }

    /// An unbalanced `Q` must not pop state the stream never pushed, and
    /// must not panic — §7.8.2's recovery posture, and the same guard the
    /// extraction walk has always had.
    #[test]
    fn an_unbalanced_q_is_survivable() {
        let recs = show_records("Q Q 0.5 Tc BT /F1 12 Tf 72 700 Td (hi) Tj ET\n");
        assert_eq!(recs.len(), 1);
        assert_eq!(recs[0].text_state.char_spacing.value, 0.5);
    }
    /// One visual line the way SolidWorks writes it: an operator per word,
    /// each placed by `Td` with a producer gap on top of the space glyph and
    /// a float-noise vertical of ±0.00057 (G028, measured on the operator's
    /// drawing; synthetic per rule 7).
    fn solidworks_line() -> Vec<u8> {
        helvetica_pdf(concat!(
            "BT /F1 1 Tf 10 0 0 10 100 700 Tm\n",
            "100 Tz (USE ) Tj\n",
            "100.03 Tz 3 -0.00057 Td (8) Tj\n",
            "100.03 Tz 1.5 0.00057 Td ( ) Tj\n",
            "100.03 Tz 1.2 -0.00057 Td (9) Tj\n",
            "100.03 Tz 1.5 0.00057 Td ( IF.) Tj\n",
            "100 Tz -7.2 -2.3 Td (NEXT) Tj\n",
            "ET\n",
        ))
    }

    /// Every glyph on page 1 as (char, origin x, advance), in content order.
    fn glyph_xs(bytes: &[u8]) -> Vec<(char, f32, f32)> {
        let doc = Document::from_bytes(bytes.to_vec()).unwrap();
        let pages = page_tree::pages(&doc).unwrap();
        let page =
            crate::text_extract::extract_page(&doc, &pages[0], 0, &Default::default()).unwrap();
        page.runs
            .iter()
            .flat_map(|r| {
                r.glyphs.iter().filter_map(|g| {
                    let s = g.text_start as usize;
                    let c = r.text.get(s..s + g.text_len as usize)?.chars().next()?;
                    Some((c, g.x, g.advance))
                })
            })
            .collect()
    }

    /// The `n`th glyph (0-based) showing `c`.
    fn nth(glyphs: &[(char, f32, f32)], c: char, n: usize) -> (f32, f32) {
        let (_, x, adv) = glyphs
            .iter()
            .filter(|(g, _, _)| *g == c)
            .nth(n)
            .unwrap_or_else(|| panic!("no glyph #{n} {c:?} in {glyphs:?}"));
        (*x, *adv)
    }

    fn assert_near(got: f32, want: f32, what: &str) {
        assert!((got - want).abs() < 0.05, "{what}: got {got}, want {want}");
    }

    /// G028: a SolidWorks drift `Td` is the same line, so a one-operator
    /// edit re-spaces the words after it. It used to stop at the first
    /// `-0.00057` and leave `8` sitting on top of the lengthened word.
    #[test]
    fn a_drift_td_is_the_same_line_for_reflow() {
        let src = solidworks_line();
        let before = glyph_xs(&src);
        let out = edit_text(
            &Document::from_bytes(src).unwrap(),
            &EditRequest::find_replace(0, "USE", "USED"),
            &EditOptions::default(),
        )
        .unwrap();
        assert!(out.report.followers_repositioned >= 1, "{:?}", out.report);
        let after = glyph_xs(&out.bytes);
        let (d_x, d_adv) = nth(&after, 'D', 0);
        let (e_x, _) = nth(&before, 'E', 0);
        assert!(d_x > e_x);
        assert_near(
            nth(&after, '8', 0).0,
            nth(&before, '8', 0).0 + d_adv,
            "8 moves by D",
        );
        assert_near(
            nth(&after, 'N', 0).0,
            nth(&before, 'N', 0).0,
            "next line holds",
        );
    }

    /// G028: a change spanning operators lands where the match began, and
    /// the producer's gaps inside the match go with it. Before, the
    /// replacement stayed in the last operator and the line kept a hole
    /// the width of every removed gap.
    #[test]
    fn a_span_edit_lands_where_the_match_began() {
        let src = solidworks_line();
        let before = glyph_xs(&src);
        let out = edit_text(
            &Document::from_bytes(src).unwrap(),
            &EditRequest::find_replace(0, "USE 8 9 IF.", "USE 7 IF."),
            &EditOptions::default(),
        )
        .unwrap();
        assert_eq!(out.report.operators_spanned, 3, "{:?}", out.report);
        let after = glyph_xs(&out.bytes);
        let (x7, adv7) = nth(&after, '7', 0);
        assert_near(x7, nth(&before, '8', 0).0, "7 lands on 8");
        // The gap between the match's end and ` IF.` is the producer's, kept.
        let (x9, adv9) = nth(&before, '9', 0);
        let gap = nth(&before, 'I', 0).0 - (x9 + adv9);
        assert_near(
            nth(&after, 'I', 0).0 - (x7 + adv7),
            gap,
            "gap after the match",
        );
        assert_near(
            nth(&after, 'U', 0).0,
            nth(&before, 'U', 0).0,
            "prefix holds",
        );
        assert_near(
            nth(&after, 'N', 0).0,
            nth(&before, 'N', 0).0,
            "next line holds",
        );
    }

    /// G028, Pin: the replacement lands where the match began AND the
    /// text after it does not move. Pin used to skip the walk entirely.
    #[test]
    fn a_pinned_span_edit_keeps_the_tail_and_lands_at_the_start() {
        let src = solidworks_line();
        let before = glyph_xs(&src);
        let out = edit_text(
            &Document::from_bytes(src).unwrap(),
            &EditRequest::find_replace(0, "USE 8 9 IF.", "USE 7 IF."),
            &EditOptions::default().with_disposition(FollowerDisposition::Pin),
        )
        .unwrap();
        let after = glyph_xs(&out.bytes);
        assert_near(
            nth(&after, '7', 0).0,
            nth(&before, '8', 0).0,
            "7 lands on 8",
        );
        assert_near(nth(&after, 'I', 0).0, nth(&before, 'I', 0).0, "tail pinned");
        assert_near(
            nth(&after, 'N', 0).0,
            nth(&before, 'N', 0).0,
            "next line holds",
        );
    }

    /// Narrowing keeps the unchanged words where the producer put them:
    /// inserting one letter touches one operator, not the whole line.
    #[test]
    fn a_span_edit_rewrites_only_the_part_that_changes() {
        let src = solidworks_line();
        let before = glyph_xs(&src);
        let out = edit_text(
            &Document::from_bytes(src).unwrap(),
            &EditRequest::find_replace(0, "USE 8 9 IF.", "USES 8 9 IF."),
            &EditOptions::default().with_disposition(FollowerDisposition::Pin),
        )
        .unwrap();
        assert_eq!(out.report.operators_spanned, 1, "{:?}", out.report);
        let after = glyph_xs(&out.bytes);
        for (c, n) in [('8', 0), ('9', 0), ('I', 0)] {
            assert_near(
                nth(&after, c, n).0,
                nth(&before, c, n).0,
                "untouched words hold",
            );
        }
    }
    /// A `Tm` follower under a scaled matrix moves by the change in USER
    /// space: the text-space delta times `a`. It used to be added to `e`
    /// raw, so with the size baked into `Tm` the follower barely moved.
    #[test]
    fn a_scaled_tm_follower_moves_in_user_space() {
        let src = helvetica_pdf(
            "BT /F1 1 Tf 10 0 0 10 100 700 Tm (Hello ) Tj 10 0 0 10 240 700 Tm (World) Tj ET\n",
        );
        let before = glyph_xs(&src);
        let out = edit_text(
            &Document::from_bytes(src).unwrap(),
            &EditRequest::find_replace(0, "Hello", "Hi"),
            &EditOptions::default(),
        )
        .unwrap();
        let after = glyph_xs(&out.bytes);
        let (x_i, adv_i) = nth(&after, 'i', 0);
        let (x_o, adv_o) = nth(&before, 'o', 0);
        // The gap from the edited word's end to `World` is the producer's.
        let gap = nth(&before, 'W', 0).0 - (x_o + adv_o);
        assert_near(nth(&after, 'W', 0).0 - (x_i + adv_i), gap, "gap to World");
    }

    /// G082: a preview of a narrowed match names the part it laid out, and
    /// that part is exactly [`narrow_span`]'s trim. One font, one line, two
    /// text objects, one character appended.
    #[test]
    fn a_narrowed_preview_names_the_rewritten_part() {
        let src = helvetica_pdf(concat!(
            "BT /F1 12 Tf 72 700 Td (Date Premises ) Tj ET\n",
            "BT /F1 12 Tf 160 700 Td (Required____ ) Tj ET\n",
        ));
        let find = "Date Premises Required____ ";
        // `(Date Premises ) Tj`: a pinned request may cross `ET`.
        let first = crate::span::ByteSpan { start: 23, len: 19 };
        let req = EditRequest::spanning_from(0, first, find, &format!("{find}_"));
        let doc = Document::from_bytes(src).unwrap();
        let narrowed = {
            let pages = crate::page_tree::pages(&doc).unwrap();
            let view = doc.view();
            let stream = ContentStream::from_page(&view, &pages[0]).unwrap();
            let recs = walk_records(&view, &pages[0].resources, &stream);
            let span = find_anchor_span(&recs, &req).unwrap();
            assert_ne!(span.first, span.last, "the match must span operators");
            narrow_span(&recs, span, &req).map(|(_, r, range)| (range, r.replace))
        };
        let s = crate::edit::EditSession::new(doc);
        let preview = s.edit_text_preview(&req, &EditOptions::default()).unwrap();
        assert_eq!(preview.rewritten, narrowed);
        assert_eq!(preview.rewritten, Some((26..27, " _".to_owned())));
        assert_eq!(preview.glyphs.len(), 2);
    }
}
