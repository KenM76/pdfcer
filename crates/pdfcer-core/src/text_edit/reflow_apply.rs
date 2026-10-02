//! # `reflow_apply` — within-block reflow SURGERY (FF-A, Pass 15.1)
//!
//! Pass 15.1 of pdfcer
//! (`docs/decisions/015-ffa-within-block-offline-reflow.md` §6, slice
//! **15.1**). Where Pass 15.0 ([`super::reflow`]) computes a **read-only**
//! [`ReflowPreview`] — new line breaks, new per-line origins, alignment
//! placement, justified slack, a new block box and page-overflow disclosure
//! — this module **applies** an accepted preview: it re-emits the block's
//! own show operators at the preview's new line origins and breaks, as ONE
//! incremental-save-safe change to only the block's content-stream object,
//! surfaced to the operator as one undo-able
//! [`CommandKind::ReflowBlock`](crate::edit::CommandKind::ReflowBlock) on an
//! [`EditSession`](crate::edit::EditSession).
//!
//! ## What it reuses (nothing new is invented)
//!
//! - **15.0's preview** ([`ReflowPreview`]) is the input: the greedy re-wrap,
//!   the auto-detected/overridden alignment, each line's origin, and each
//!   full justified line's slack are already computed. 15.1 only *emits*
//!   them.
//! - **14.1's advance-preserving machinery** ([`super::edit`]): the
//!   `emit_tm` / `splice` / `write_incremental` / `make_raw_stream` helpers
//!   and the §9.4.4 advance model. A justified line's per-gap `TJ` number is
//!   the **sign-mirror** of 14.1's compensating-`TJ` pin
//!   (`iso32000__ref__reflow_emission.md` §1: 14.1 REMOVES advance with a
//!   positive `N`; justify ADDS slack with a negative `N`).
//! - **14.3's `EditSession` command-log** integration: the reflow lands via
//!   the same `text_edit_command` seam `edit_text`/`format_text` use, so it
//!   is one undo entry whose `before` restores the byte-identical pre-reflow
//!   stream.
//! - **The shared tokeniser** ([`super::reflow::tokenise_block`]): the SAME
//!   words the preview's break points index, now carrying their source codes
//!   so the emission re-shows the **original bytes** — a reflow re-wraps, it
//!   never re-encodes (decision 015 §3.7, minimal-diff).
//!
//! ## The load-bearing justify emission (§9.4.3 / §9.3.3)
//!
//! A full (non-last) justified line distributes its slack
//! `S = wrap_width − natural_width` (in default user space, already computed
//! by 15.0 as [`ReflowLine::justified_slack`]) across its `G = words − 1`
//! inter-word gaps, as one negative `TJ` number per gap
//! (`iso32000__ref__reflow_emission.md` §1):
//!
//! ```text
//!   Δuser_x  =  −(N / 1000) · emit_scale           (§9.4.4, axis-aligned)
//!   emit_scale = Tfs · Th · a · ca                 (Tm x-scale × CTM x-scale)
//!   ⇒  N_gap  =  −(S / G) · 1000 / emit_scale       (negative ⇒ opens up)
//! ```
//!
//! Sign (§9.4.3 Table 109): a `TJ` number is **subtracted** in thousandths
//! and scaled by size × horizontal-scaling, so a **positive** number closes
//! the gap and a **negative** one opens it — justify adds space ⇒ negative
//! `N`. The line is emitted as ONE `[ (w0 SP) N (w1 SP) N (w2) ] TJ`: the
//! code-32 space glyphs stay inside the strings (so extraction still sees
//! the word breaks, §14.8.2.4) and the `N` numbers add the extra slack on
//! top. Word spacing is set to zero (`0 Tw`) so the kept spaces are not
//! double-stretched (`iso32000__ref__reflow_emission.md` §2/§4.1). **The
//! last line of the paragraph is never stretched** — 15.0 records
//! `justified_slack = None` for it (and for a single-word line, which falls
//! back to flush-left + a disclosure, decision 015 §3.1); 15.1 emits those
//! as a plain `Tj`. `TJ` is the general path (works for any font model, no
//! last-line state leak); the `Tw`-only alternative is a named non-goal here
//! because it cannot serve composite fonts (§9.3.3) and leaks into the last
//! line.
//!
//! ## The line-origin path — absolute `Tm` per line (recipe C)
//!
//! 15.1 re-emits the block as ONE fresh `BT … ET` text object with an
//! **absolute `Tm` per line** (`iso32000__ref__reflow_emission.md` §3.2
//! recipe **C**): `a b c d e f Tm` where `(a,b,c,d)` is the block's own
//! text-matrix linear part (carried from provenance, so size/orientation are
//! preserved) and `(e,f)` places the preview's user-space line origin. Recipe
//! C is chosen over the compact `Td`/`T*` recipe (A/B) because it is
//! **immune to the relative-delta bug** (each `Td` re-bases `Tlm`, so a
//! centre/right/justified block that used absolute deltas would drift —
//! §3.1), it handles left/centre/right/justified uniformly, and the whole
//! block is re-emitted anyway so the extra `Tm` per line costs nothing in
//! minimal-diff terms (the change is bounded to the block's own content
//! object regardless). The block grows **top-anchored** downward as lines are
//! added (decision 015 §3.5): line *i* sits at `baseline_y = first − i·L`,
//! straight from the preview.
//!
//! ## Scope — simple, axis-aligned, LTR, one block (everything else refused)
//!
//! Simple fonts only: a composite (Type0/CIDFont) block is **refused by
//! name** (R-INV-4) — `Tw` cannot justify a multi-byte code and the word
//! tokeniser assumes one byte per glyph (decision 015 §8, FF-E). The block's
//! text/CTM must be **axis-aligned** (no rotation/skew: `b = c = 0` in both
//! the text matrix and the CTM) and uniform across the block; a rotated,
//! skewed, or multi-transform block is refused (recipe C's rotation support
//! is a documented future extension, not a claimed-but-untested path).
//! Justify additionally requires zero `Tc`/`Tw` on the block (else the slack
//! arithmetic, which assumes the kept spaces carry only `w0`, would be
//! wrong) — refused-and-disclosed otherwise. The block's show operators must
//! occupy their own text object(s) (no sharing with another paragraph) and be
//! contiguous in the stream. All refusals are clean, named
//! [`ReflowApplyError`]s — never a crash, never a silent mangle (rule 4).
//!
//! ## Page overflow — disclose and allow, never clip (§3.5 / R76)
//!
//! If the re-wrap grows the block past the page cropbox, every line is still
//! emitted at its true (possibly off-page, negative-baseline) position — the
//! content is real and recoverable, never clipped-to-invisible or dropped.
//! The 15.0-computed [`PageOverflow`] disclosure is surfaced on the report.
//! This is the deliberate divergence from Acrobat's documented silent
//! "disappear".
//!
//! ## Tagged blocks (§14.6/§14.7 / R72)
//!
//! The block's `BDC …/MCID n… EMC` wrapper sits OUTSIDE the re-emitted region
//! (the region spans the block's own `BT … ET` text objects, which sit
//! *inside* the marked-content sequence), so the `/MCID` and the structure
//! tree's `(Pg, MCID)` reference are preserved by construction. What goes
//! stale is the `/ActualText`/reading-order the tree records; 15.1
//! **discloses** that and does not regenerate it (identical posture to 14.1).
//!
//! ## Save mode — incremental by default (R34/R36)
//!
//! Only the block's own content-stream object (+ any collapsed extra content
//! streams) is re-emitted, in the incremental update section; everything
//! outside is the original file bytes verbatim (the original is a byte-prefix
//! of the output). This is NOT redaction's forced full rewrite (R35). The
//! prior text survives in the document's revision history by design, and that
//! is disclosed.

use crate::content::{ContentError, ContentStream};
use crate::document::Document;
use crate::page_tree::{self, Page, PageTreeError};
use crate::span::ByteSpan;
use crate::text_edit::cause::UnsupportedCause;
use crate::text_extract::{self, ContentStreamRef, ExtractError, ExtractOptions, GlyphProvenance};
use crate::text_state::{AmbientRestoreError, AmbientTextState, TextStateParam};
use crate::view::DocumentView;

use super::edit::{EditError, EditGlyphSource, splice, trust_disclosure, write_incremental};
use super::encoding::Refusal;
use super::model::{Block, EditableTextModel, GlyphRef};
use super::reflow::{
    BlockAlignment, PageOverflow, ReflowEngine, ReflowPreview, ReflowRequest, tokenise_block,
};
use super::reflow_fit::CellOverflow;
use super::reflow_spacing::JustifySpacing;
use super::reflow_style::{BlockFonts, EmitCtx, Emitted, emit_block, resolve_fonts};
use super::reflow_walk::locate_block_region;

/// Axis-alignment / near-zero tolerance for matrix entries and scales,
/// points. Below this a matrix off-diagonal is treated as zero (upright) and
/// a scale as degenerate.
const MTX_EPS: f64 = 1e-6;

/// The outcome of a successful reflow-apply (free-function path).
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ReflowOutcome {
    /// The saved (incrementally-appended) PDF bytes.
    pub bytes: Vec<u8>,
    /// The disclosure/diagnostic report.
    pub report: ReflowApplyReport,
}

/// What a reflow-apply did and disclosed (fuzzy-never-sneaky, rule 4).
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ReflowApplyReport {
    /// The 0-based block index that was reflowed.
    pub block_index: usize,
    /// Line count before the re-wrap.
    pub lines_before: usize,
    /// Line count after the re-wrap.
    pub lines_after: usize,
    /// The alignment used (auto-detected or overridden).
    pub alignment: BlockAlignment,
    /// How many full lines received justified slack (0 unless justified).
    pub justified_lines: usize,
    /// The `/BaseFont` of the reflowed block's font (subset tag included).
    pub base_font: String,
    /// The core-visible glyph source of the block's font.
    pub glyph_source: EditGlyphSource,
    /// The `/MCID` of the enclosing marked-content sequence, if the block was
    /// inside a Tagged-PDF sequence (its wrapper is preserved; §14.7).
    pub tagged_mcid: Option<i64>,
    /// The signed change in block-box height (new − old), points. Positive =
    /// the block grew taller.
    pub height_delta: f64,
    /// A disclosed page-overflow condition, if the re-wrap grew the block
    /// past the page cropbox (§3.5 / R76). Content is still emitted.
    pub overflow: Option<PageOverflow>,
    /// A disclosed table-cell overflow, if the block is a table cell and the
    /// re-wrapped text does not fit it. Content is still emitted; the cell
    /// and the rows below are not moved.
    pub cell_overflow: Option<CellOverflow>,
    /// The content-stream object number that was rewritten.
    pub content_object: u32,
    /// Extra content objects collapsed/emptied on a multi-stream page.
    pub extra_objects_emptied: u64,
    /// Every operator-facing disclosure, verbatim (surfaced by the UI/CLI).
    pub disclosures: Vec<String>,
}

/// A failure to apply a reflow — every variant is a clean, named outcome,
/// never a crash (rule 4).
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ReflowApplyError {
    /// The 15.0 preview could not be computed (bad index / width / empty
    /// block). Carries the underlying [`super::ReflowError`].
    #[error("reflow preview failed: {0}")]
    Preview(#[from] super::ReflowError),
    /// The font-on-edit gate refused, by name (composite/CJK — R-INV-4).
    #[error(transparent)]
    Refused(Refusal),
    /// The page was extracted without provenance, so the block's glyphs
    /// cannot be located back to their show operators. Reflow-apply requires
    /// [`ExtractOptions::capture_provenance`].
    #[error(
        "reflow-apply requires provenance capture: extract the page with \
         ExtractOptions::with_provenance(true)"
    )]
    NoProvenance,
    /// No page at the requested index.
    #[error("no page at index {0}")]
    PageIndex(usize),
    /// The block is real but this cut cannot reflow-apply it: a named
    /// condition (rotated/skewed text, a shared or non-contiguous text
    /// object, a form-XObject block, non-zero Tc/Tw under justify, …).
    ///
    /// **Every one of these is permanent for this document as it stands** —
    /// see [`Self::PageEditedThisSession`], which was carved out of this
    /// variant precisely because it is *not*.
    #[error("this block cannot be reflow-applied: {0}")]
    Unsupported(UnsupportedCause),
    /// Text was added to this page **in this session**, in a new content
    /// stream, and reflow re-emits the page's first content stream only —
    /// so committing would drop the added run (`Pass 251.0`'s guard).
    ///
    /// # This is the ONLY reflow refusal the operator can act on
    ///
    /// **Save the document and reopen it, then reflow — it works.** Every
    /// other reason reflow declines is a property of how the page was drawn
    /// and no operator action changes it.
    ///
    /// Carved out of [`Self::Unsupported`] at `pdfcer-gui`'s request
    /// (2026-09-07). Their finding: `Unsupported(String)` carried **ten**
    /// distinct refusals, **one** of them recoverable and **the commonest**
    /// — it fires whenever text has been added to the page this session,
    /// which during live editing is most of the time. A shell with no
    /// discriminant can only print the weakest sentence true of all ten, so
    /// the operator was being denied a remedy that existed.
    ///
    /// **They refused to match on the prose, and were right to.** A shell
    /// branching on `starts_with("text was added")` would break silently on
    /// the next typo fix and would be re-deriving pdfcer's control flow from
    /// pdfcer's sentences. A sentence is not an API; this variant is.
    #[error(
        "text was added to this page this session (in a new content stream); reflow re-emits \
         the page's first content stream only and committing would drop the added run, so save \
         and reopen before reflowing this page"
    )]
    /// ⚠ **NO LONGER PRODUCED BY ANY PATH, as of `G015` (2026-09-14), and
    /// kept rather than removed so the decision to drop it is the consuming
    /// project's rather than a side effect of this fix.**
    ///
    /// `reflow_block`'s guard returned this whenever the page carried a
    /// non-empty extra `/Contents` stream. That was correct while the planner
    /// read the BASE document — a run appended this session lived in an extra
    /// the plan could not see, and the commit would have emptied it.
    /// `Pass 257.0` made the planner read the SESSION's graph, from which
    /// point the appended run is in the plan's source and survives; the guard
    /// was removed once that was measured.
    ///
    /// **It was also firing on pages nobody had edited.** The condition tested
    /// a structural property — *does `contents[1..]` hold a non-empty stream* —
    /// which ISO 32000-1 §7.8.2 explicitly permits a producer to author, and
    /// CAD exporters do routinely. A 36-sheet drawing set whose title sheet
    /// carried eight producer-authored streams was refused with *"text was
    /// added to this page this session"* on a freshly opened session, offering
    /// a remedy ("save and reopen") that could not work because the streams are
    /// in the file.
    ///
    /// ⇒ It is therefore still the variant [`ReflowApplyError::is_recoverable`]
    /// reports as recoverable, and nothing constructs it.
    ///
    /// **KEEPING IT IS A DECISION, NOT AN OMISSION** — `pdfcer-gui`'s, made
    /// 2026-09-14 when asked directly whether it should go. Their reasons, so
    /// nobody re-derives them:
    ///
    /// 1. Their `match` over the decline type is compiler-proved exhaustive.
    ///    An `unreachable!()` in that arm would turn *a future engine
    ///    reinstating the guard* into **a panic on a refusal path** — the one
    ///    path whose whole purpose is to fail safely in front of an operator.
    /// 2. The removal's only benefit is a compile break saying something they
    ///    already re-measure every commit, and a break fires only on removal
    ///    where their gate fires on any change.
    /// 3. `G013` took the opposite trade because that symbol was one their
    ///    shell **constructs**; this one it only **receives**.
    ///
    /// ⇒ That third clause is the general rule worth carrying: **a loud
    /// compile break is worth it for a symbol a consumer constructs, and is
    /// worse than a gate for one it only receives.** If the engine later wants
    /// this gone for its own reasons, they will delete the arm in the same
    /// commit that moves the pin.
    PageEditedThisSession,
    /// The document is encrypted and this edit is not permitted: see
    /// [`EditError::DocumentEncrypted`](crate::edit::EditError::DocumentEncrypted).
    #[error("{}", crate::edit::ENCRYPTED_EDIT_REFUSED)]
    Encrypted,
    /// Extracting the page failed.
    #[error("page extraction failed: {0}")]
    Extract(#[from] ExtractError),
    /// The page's content stream could not be parsed.
    #[error("content stream parse failed: {0}")]
    Content(#[from] ContentError),
    /// The page tree could not be walked.
    #[error("page tree error: {0}")]
    PageTree(#[from] PageTreeError),
    /// The incremental save failed.
    #[error("save failed: {0}")]
    Write(#[from] EditError),
}

/// **What an operator can do about a declined reflow** — the stable
/// discriminant a shell switches on instead of reading pdfcer's prose.
///
/// # Why this exists rather than a fifth [`super::RefusalKind`]
///
/// `pdfcer-gui` asked for a discriminant and named
/// [`super::RefusalClass`]/[`super::RefusalKind`] as the precedent, since
/// pdfcer had already built exactly that for `EditError` and `AddTextError`
/// (`Pass 249.0`). The shape is right; **the vocabulary is not.** Those four
/// buckets answer *"what kind of thing went wrong"* — font, structure,
/// not-found, other — and **every reflow refusal lands in one bucket under
/// them**, so the question the shell actually needs answered would still be
/// unanswerable.
///
/// **And `RefusalKind` could not simply grow a fifth variant**, because
/// the reply that shipped it made a written promise on the request channel:
/// it is deliberately **not** `#[non_exhaustive]` so a consumer's `match` is
/// compiler-proved complete, and *"growing the enum is a deliberate breaking
/// change with a note on this channel — **it will not happen because we
/// learned a new refusal**."* Adding a variant here because reflow turned
/// out to have a recoverable case is precisely the thing that promise
/// forbids. So reflow gets its own discriminant, and the older one keeps its
/// guarantee.
///
/// # The distinction it draws, which is the only one that matters
///
/// **Can the operator do something, or not.** Nine of reflow's ten refusals
/// are properties of how the page was drawn — no action changes them. One is
/// a session artefact that a save-and-reopen clears.
///
/// # Exhaustive on purpose, same as `RefusalKind`
///
/// Not `#[non_exhaustive]`: a caller's `match` is compiler-proved complete,
/// and a future refusal joins an existing arm rather than appearing as a
/// silent fall-through. Growing it would be a deliberate breaking change
/// announced on the channel — the same contract, made the same way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReflowDecline {
    /// **The operator can fix this: save and reopen, then reflow.**
    /// [`ReflowApplyError::PageEditedThisSession`] and nothing else.
    RetryAfterSaveAndReopen,
    /// The document's structure forbids it — encryption, or a save that the
    /// edit gates refused. No reflow of this document until that changes.
    StructureForbids,
    /// The request named something absent: a page index out of range, or a
    /// page extracted without provenance so the glyphs cannot be traced back
    /// to their show operators. A **caller** error, not an operator one.
    NotFound,
    /// This block cannot be reflowed as this page is drawn — rotated or
    /// skewed text, a shared or non-contiguous text object, a form-XObject
    /// block, a missing or unresolvable font resource, a degenerate CTM, a
    /// page with no `/Contents`. **Permanent for this document**; the
    /// operator did nothing wrong and can do nothing about it.
    NotReflowable,
}

impl ReflowApplyError {
    /// Which [`ReflowDecline`] this refusal is — **the seam to switch on**.
    ///
    /// Match this, never [`std::fmt::Display`] output. pdfcer's sentences are
    /// implementer-voiced diagnostics and are reworded whenever they are
    /// wrong; this value is a contract.
    ///
    /// # Example
    ///
    /// ```
    /// use pdfcer_core::text_edit::{ReflowApplyError, ReflowDecline};
    ///
    /// let e = ReflowApplyError::PageEditedThisSession;
    /// assert_eq!(e.decline(), ReflowDecline::RetryAfterSaveAndReopen);
    /// assert!(e.is_recoverable());
    ///
    /// let e = ReflowApplyError::Encrypted;
    /// assert!(!e.is_recoverable());
    /// ```
    #[must_use]
    pub fn decline(&self) -> ReflowDecline {
        match self {
            Self::PageEditedThisSession => ReflowDecline::RetryAfterSaveAndReopen,
            Self::Encrypted | Self::Write(_) => ReflowDecline::StructureForbids,
            Self::PageIndex(_) | Self::NoProvenance => ReflowDecline::NotFound,
            // Everything else is a property of the page as drawn. `Refused`
            // is the font-invariant gate, `Unsupported` the nine merged
            // conditions, and the three parse/walk failures describe a
            // document this cut cannot process either way.
            Self::Unsupported(_)
            | Self::Refused(_)
            | Self::Preview(_)
            | Self::Extract(_)
            | Self::Content(_)
            | Self::PageTree(_) => ReflowDecline::NotReflowable,
        }
    }

    /// Whether the operator can clear this by saving and reopening.
    ///
    /// Derived from [`Self::decline`] rather than matched independently —
    /// two readers of one fact is the `R243` shape, and a shell that trusted
    /// a `is_recoverable()` disagreeing with `decline()` would offer a remedy
    /// that does not work.
    #[must_use]
    pub fn is_recoverable(&self) -> bool {
        matches!(self.decline(), ReflowDecline::RetryAfterSaveAndReopen)
    }
}

/// The result of planning a reflow WITHOUT committing it: the fully-spliced
/// replacement content-stream buffer plus the complete report and the
/// content-object identity.
///
/// This is the seam (mirroring [`super::edit::EditPlan`]) that lets the
/// interactive
/// [`EditSession::reflow_block`](crate::edit::EditSession::reflow_block)
/// reuse the EXACT extract → recognise → preview → re-emit → splice logic of
/// the free-function [`apply_reflow`] while landing the mutation as one
/// undo-able command against the session's in-memory object graph, instead of
/// producing already-saved bytes.
pub(crate) struct ReflowPlan {
    /// The spliced, decoded replacement content for the page's first content
    /// object (the whole page content, with the block re-emitted).
    pub(crate) new_content: Vec<u8>,
    /// The complete disclosure/diagnostic report.
    pub(crate) report: ReflowApplyReport,
}

/// Apply an accepted within-block reflow and save **incrementally** — the
/// free-function entry point (mirroring
/// [`edit_text`](super::edit::edit_text)).
///
/// Extracts page `page_index` WITH provenance, recognises the block model
/// (with first-line-indent splitting relaxed, so a right/centre/justified
/// paragraph stays one block — matching the 15.0 preview path), computes the
/// [`ReflowPreview`] for `block_index` under `req`, re-emits that block's show
/// operators at the new origins/breaks via the 14.1 advance-preserving
/// machinery, and writes the change incrementally. Only the block's own
/// content-stream object changes; everything else is byte-verbatim.
///
/// # Errors
///
/// See [`ReflowApplyError`]: a named composite refusal, a missing-provenance
/// error, an unsupported block (rotated/shared/non-contiguous/…), a bad page
/// index, an encrypted document, or an extraction/parse/save failure.
pub fn apply_reflow(
    doc: &Document,
    page_index: usize,
    block_index: usize,
    req: &ReflowRequest,
) -> Result<ReflowOutcome, ReflowApplyError> {
    if crate::encryption_gate::forbids(doc, &[crate::crypto::PermissionBit::ModifyContents]) {
        return Err(ReflowApplyError::Encrypted);
    }
    let mut plan = plan_reflow_from_doc(&doc.view(), page_index, block_index, req)?;
    let pages = page_tree::pages(doc)?;
    let page = pages
        .get(page_index)
        .ok_or(ReflowApplyError::PageIndex(page_index))?;
    let (bytes, content_object, emptied, decoupled) =
        write_incremental(doc, page, &plan.new_content)?;
    if decoupled {
        plan.report.content_object = content_object;
        plan.report.extra_objects_emptied = emptied;
        plan.report
            .disclosures
            .push(crate::text_edit::edit::SHARED_CONTENT_DISCLOSURE.to_owned());
    }
    Ok(ReflowOutcome {
        bytes,
        report: plan.report,
    })
}

/// Extract, recognise, preview and plan a reflow over a whole document —
/// the shared core of the free function and the session path.
///
/// Returns the spliced replacement content + report WITHOUT saving; the
/// caller performs its own write step (`write_incremental` for the free
/// function, `text_edit_command` for the session).
///
/// # Errors
///
/// See [`ReflowApplyError`].
pub(crate) fn plan_reflow_from_doc(
    doc: &DocumentView<'_>,
    page_index: usize,
    block_index: usize,
    req: &ReflowRequest,
) -> Result<ReflowPlan, ReflowApplyError> {
    let pages = page_tree::pages_in(doc)?;
    let page = pages
        .get(page_index)
        .ok_or(ReflowApplyError::PageIndex(page_index))?;

    // Extract WITH provenance (so the block's glyphs carry their show
    // operators' spans + matrices) and enable the overflow disclosure by
    // supplying the page cropbox unless the caller overrode it.
    let options = ExtractOptions::default().with_provenance(true);
    let extracted = text_extract::extract_page_view(doc, page, page_index, &options)?;
    let cells = super::detect_cell_regions(doc, page_index).map_err(table_error)?;
    let model = EditableTextModel::recognize_with_cells(
        &extracted,
        &super::reflow::reflow_recognition_options(),
        &cells,
    );

    let req = ReflowRequest {
        page_cropbox: req.page_cropbox.or(Some(page.crop_box)),
        ..*req
    };
    let engine = ReflowEngine::new(&model);
    let preview = engine.preview(block_index, &req)?;

    // SAME GRAPH as the extraction a few lines above (decision 018 caller
    // audit, re-based on the caller's view in `Pass 257.0`): the block model
    // and the stream it is spliced into must come from one graph, or the
    // plan's byte offsets describe a different stream than the one edited.
    let stream = ContentStream::from_page(doc, page)?;
    plan_reflow(doc, page, &stream, &model, block_index, &preview)
}

fn table_error(e: crate::table_detect::TableError) -> ReflowApplyError {
    use crate::table_detect::TableError;
    match e {
        TableError::Extract(e) => ReflowApplyError::Extract(e),
        TableError::PageTree(e) => ReflowApplyError::PageTree(e),
    }
}

/// Plan a reflow-apply over an already-decoded content `stream`, given a
/// computed `preview`: locate the block's text objects, re-encode-free
/// re-emit its lines at the preview's origins/breaks (justified lines get
/// their `TJ` slack), splice — returning the new content buffer and report,
/// but performing NO save.
///
/// Factored so the interactive session path reuses the identical surgery
/// (mirroring [`super::edit::plan_edit`]). Takes the `preview` directly so a
/// future GUI (Pass 15.2) can pass an operator-adjusted preview.
///
/// # Errors
///
/// See [`ReflowApplyError`]: a composite refusal, missing provenance, or an
/// unsupported (rotated/shared/non-contiguous/…) block.
pub(crate) fn plan_reflow(
    doc: &DocumentView<'_>,
    page: &Page,
    stream: &ContentStream,
    model: &EditableTextModel<'_>,
    block_index: usize,
    preview: &ReflowPreview,
) -> Result<ReflowPlan, ReflowApplyError> {
    let content_id = *page
        .contents
        .first()
        .ok_or(ReflowApplyError::Unsupported(UnsupportedCause::NoContents))?;
    let extra_emptied = page.contents.len().saturating_sub(1) as u64;

    let block = model
        .blocks()
        .get(block_index)
        .ok_or(ReflowApplyError::Preview(
            super::ReflowError::BlockIndexOutOfRange(block_index, model.blocks().len()),
        ))?;
    let prov = block_provenance(model, block)?;
    let mut region = locate_block_region(stream, &prov.op_spans)?;
    let spacing = JustifySpacing::detect(model, block);
    spacing.apply_to(&mut region);
    let (words, _) = tokenise_block(model, model.sourced_view(), block, &spacing);
    let fonts = resolve_fonts(doc, &page.resources, model, &region, &words)?;
    let ctx = EmitCtx {
        model,
        region: &region,
        fonts: &fonts,
        prov: &prov,
        spacing,
        spaces: words.iter().filter_map(|w| w.space_after).collect(),
        synthetic_space: preview.diagnostics.space_width_pt,
    };
    let emitted = emit_block(&ctx, &words, preview)?;

    let mut disclosures = apply_disclosures(block_index, preview, &fonts, &emitted);
    carried_disclosures(&mut disclosures, preview, prov.mcid, extra_emptied);
    let mut edits: Vec<(usize, usize, Vec<u8>)> = vec![(region.start, region.end, emitted.body)];
    let new_content = splice(&stream.buf, &mut edits);
    let first_font = fonts.order.first().and_then(|n| fonts.info.get(n));
    let all_embedded = fonts.info.values().all(|f| f.embedded);
    let report = ReflowApplyReport {
        block_index,
        lines_before: preview.lines_before,
        lines_after: preview.lines_after,
        alignment: preview.alignment.alignment,
        justified_lines: emitted.justified_lines,
        base_font: first_font.map(|f| f.base_font.clone()).unwrap_or_default(),
        glyph_source: if all_embedded {
            EditGlyphSource::Embedded
        } else {
            EditGlyphSource::NonEmbedded
        },
        tagged_mcid: prov.mcid,
        height_delta: preview.height_delta(),
        overflow: preview.overflow,
        cell_overflow: preview.cell_overflow,
        content_object: content_id.num,
        extra_objects_emptied: extra_emptied,
        disclosures,
    };
    Ok(ReflowPlan {
        new_content,
        report,
    })
}

/// The disclosures of what the apply stage itself did.
fn apply_disclosures(
    block_index: usize,
    preview: &ReflowPreview,
    fonts: &BlockFonts,
    emitted: &Emitted,
) -> Vec<String> {
    let mut disclosures: Vec<String> = fonts
        .order
        .iter()
        .filter_map(|n| fonts.info.get(n))
        .map(|f| trust_disclosure(f.embedded, &f.base_font))
        .collect();
    disclosures.push(
        "save: this reflow was written INCREMENTALLY (R34/R70); the prior text survives in the \
         document's revision history by design. To truly remove text, use redaction (Pass 8) — a \
         distinct, security operation."
            .to_owned(),
    );
    disclosures.push(format!(
        "reflow: block {block_index} re-wrapped from {} to {} line(s) (alignment {}); only the \
         block's own content-stream object was re-emitted — everything else is byte-verbatim \
         (R32/R46, minimal-diff)",
        preview.lines_before,
        preview.lines_after,
        preview.alignment.alignment.as_str(),
    ));
    disclosures.push(
        "reflow: the new line breaks, per-line origins and block box are DERIVED layout the file \
         never stated (ISO 32000-1 §14.8 S1-S9) — this was an explicit, reviewable re-wrap \
         (decision 015 §3.3/R75), not a silent re-layout"
            .to_owned(),
    );
    disclosures.push(
        "reflow: every glyph kept its own code, font, size, text state and colour, and the \
         source's positioning inside each word (TJ kerning, §9.4.3) was kept; no new pair \
         kerning was applied"
            .to_owned(),
    );
    if emitted.synthetic_spaces > 0 {
        disclosures.push(format!(
            "reflow: {} word gap(s) were written with a code-32 space the block never showed, \
             at an assumed width of {:.2}pt",
            emitted.synthetic_spaces, preview.diagnostics.space_width_pt,
        ));
    }
    if emitted.leak_closed {
        disclosures.push(
            "reflow: the re-emitted text object leaves text state, colour or font different \
             from what followed the block, so an explicit restore was appended inside the text \
             object (R88 restore-by-value — q/Q are not permitted inside BT … ET, §8.2 Table \
             51). Content following the block is therefore unaffected."
                .to_owned(),
        );
    }
    if emitted.justified_lines > 0 {
        disclosures.push(format!(
            "reflow: {} full line(s) were JUSTIFIED — inter-word slack distributed as per-gap TJ \
             numbers (§9.4.3); the last line of the paragraph is left un-stretched (decision 015 \
             §3.1)",
            emitted.justified_lines,
        ));
    }
    disclosures
}

/// The preview's still-true disclosures, and the overflow, tagging and
/// multi-stream notes.
fn carried_disclosures(
    disclosures: &mut Vec<String>,
    preview: &ReflowPreview,
    mcid: Option<i64>,
    extra_emptied: u64,
) {
    // Preview notes about the READ-ONLY stage would be false here.
    for note in &preview.diagnostics.disclosures {
        let preview_only = note.contains("READ-ONLY")
            || note.contains("nothing is written")
            || note.contains("not applied");
        if !preview_only && !disclosures.contains(note) {
            disclosures.push(note.clone());
        }
    }
    if let Some(ov) = preview.overflow {
        if ov.past_bottom_pt > 0.0 {
            disclosures.push(format!(
                "reflow: the re-wrap grows the block {:.1}pt past the page bottom (cropbox); {} \
                 line(s) fall outside the visible page — the content was EMITTED at its true \
                 off-page position, NOT clipped or dropped (decision 015 §3.5, R76)",
                ov.past_bottom_pt, ov.lines_outside,
            ));
        }
        if ov.past_right_pt > 0.0 {
            disclosures.push(format!(
                "reflow: the wrap width put the block {:.1}pt past the page RIGHT edge (cropbox), \
                 so the re-wrapped text runs off the page — EMITTED at its true off-page \
                 position, NOT clipped. The width was measured from the block's own box, which \
                 an earlier edit may have widened past the margin; re-run with an explicit width \
                 to wrap to the original margin (R148, R76)",
                ov.past_right_pt,
            ));
        }
    }
    if let Some(mcid) = mcid {
        disclosures.push(format!(
            "tagged PDF: the block is inside a marked-content sequence (/MCID {mcid}); its \
             BDC/EMC+MCID wrapper was PRESERVED (structure references stay valid), but the \
             structure tree's /ActualText and reading order were NOT updated and are now STALE \
             (a stale /ActualText wins on extraction, §14.9.4). pdfcer discloses this rather than \
             silently corrupting the accessibility tree (R72)."
        ));
    }
    if extra_emptied > 0 {
        disclosures.push(format!(
            "multi-stream page: {extra_emptied} additional /Contents stream(s) were collapsed \
             into the first and emptied so the reflow's byte offsets stay coherent."
        ));
    }
}

// ===================================================================
// Block provenance (font + matrices + show-operator spans)
// ===================================================================

/// The block-level provenance the surgery needs: one axis-aligned text/CTM
/// linear part, one `/MCID`, and the ordered set of show-operator byte spans
/// (in the page content buffer) that produced the block's glyphs.
pub(super) struct BlockProvenance {
    mcid: Option<i64>,
    /// Text-matrix linear part (must be uniform + axis-aligned).
    pub(super) tm_a: f64,
    pub(super) tm_b: f64,
    pub(super) tm_c: f64,
    pub(super) tm_d: f64,
    /// CTM linear + translation (axis-aligned; translation used to map an
    /// origin back to `Tm` operands).
    pub(super) ctm_a: f64,
    ctm_d: f64,
    ctm_e: f64,
    ctm_f: f64,
    /// The distinct show-operator keyword spans of the block, in first-seen
    /// order (matches [`GlyphProvenance::operator_span`]).
    op_spans: Vec<ByteSpan>,
}

/// Collect the block's provenance, refusing (by name) a block that carries no
/// provenance, spans a form XObject, or is not uniform + axis-aligned.
fn block_provenance(
    model: &EditableTextModel<'_>,
    block: &Block,
) -> Result<BlockProvenance, ReflowApplyError> {
    let mut mcid: Option<i64> = None;
    let mut tm: Option<[f64; 6]> = None;
    let mut ctm: Option<[f64; 6]> = None;
    let mut op_spans: Vec<ByteSpan> = Vec::new();
    let mut saw_glyph = false;

    for &li in &block.line_indices {
        let Some(line) = model.lines().get(li) else {
            continue;
        };
        for &gref in &line.glyphs {
            let p = model
                .provenance(gref)
                .ok_or(ReflowApplyError::NoProvenance)?;
            saw_glyph = true;
            // Only page-buffer spans are handled; a form XObject is a
            // separate decoded buffer this cut does not re-emit.
            match p.content_stream {
                ContentStreamRef::Page => {}
                ContentStreamRef::Form { object } => {
                    return Err(ReflowApplyError::Unsupported(
                        UnsupportedCause::InsideFormXObject {
                            object: Some(object),
                        },
                    ));
                }
            }
            if p.font_resource.is_none() {
                return Err(ReflowApplyError::Unsupported(
                    UnsupportedCause::ShowWithoutFont,
                ));
            }
            // Matrices must be uniform (linear part) + axis-aligned.
            check_uniform_axis_aligned(&mut tm, p.text_matrix, "text matrix (Tm)")?;
            check_uniform_axis_aligned(&mut ctm, p.ctm, "CTM")?;
            record_prov(&mut mcid, &mut op_spans, model, gref, p);
        }
    }

    if !saw_glyph {
        return Err(ReflowApplyError::Preview(super::ReflowError::EmptyBlock(0)));
    }
    let tm = tm.unwrap_or([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
    let ctm = ctm.unwrap_or([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
    if op_spans.is_empty() {
        return Err(ReflowApplyError::Unsupported(
            UnsupportedCause::NoShowOperators,
        ));
    }
    Ok(BlockProvenance {
        mcid,
        tm_a: tm[0],
        tm_b: tm[1],
        tm_c: tm[2],
        tm_d: tm[3],
        ctm_a: ctm[0],
        ctm_d: ctm[3],
        ctm_e: ctm[4],
        ctm_f: ctm[5],
        op_spans,
    })
}

/// Record a glyph's `/MCID` (first-seen wins; a block that mixes MCIDs is
/// tolerated — the wrapper is preserved regardless) and its distinct
/// show-operator span.
fn record_prov(
    mcid: &mut Option<i64>,
    op_spans: &mut Vec<ByteSpan>,
    model: &EditableTextModel<'_>,
    gref: GlyphRef,
    p: &GlyphProvenance,
) {
    if mcid.is_none()
        && let Some(run) = model.sourced_view().runs.get(gref.run)
    {
        *mcid = run.mcid.map(i64::from);
    }
    let span = p.operator_span;
    if !op_spans
        .iter()
        .any(|s| s.start == span.start && s.len == span.len)
    {
        op_spans.push(span);
    }
}

/// Ensure a matrix is uniform across the block and axis-aligned (no
/// rotation/skew: `b = c = 0`), refusing by name otherwise.
fn check_uniform_axis_aligned(
    seen: &mut Option<[f64; 6]>,
    m: [f32; 6],
    label: &'static str,
) -> Result<(), ReflowApplyError> {
    let m = [
        f64::from(m[0]),
        f64::from(m[1]),
        f64::from(m[2]),
        f64::from(m[3]),
        f64::from(m[4]),
        f64::from(m[5]),
    ];
    if m[1].abs() > MTX_EPS || m[2].abs() > MTX_EPS {
        return Err(ReflowApplyError::Unsupported(
            UnsupportedCause::RotatedOrSkewed { matrix: label },
        ));
    }
    match seen {
        None => *seen = Some(m),
        // Only the LINEAR part must be uniform; the translation (e,f)
        // legitimately differs per glyph/line.
        Some(prev) => {
            if (prev[0] - m[0]).abs() > MTX_EPS || (prev[3] - m[3]).abs() > MTX_EPS {
                return Err(ReflowApplyError::Unsupported(
                    UnsupportedCause::MixedScale { matrix: label },
                ));
            }
        }
    }
    Ok(())
}

/// Map a user-space line origin to `Tm` translation operands `(e, f)` under
/// the axis-aligned CTM (§9.4.2): `user = (ca·e + ce, cd·f + cf)` ⇒
/// `e = (x − ce)/ca`, `f = (y − cf)/cd`. Refuses a degenerate CTM scale.
pub(super) fn origin_to_tm(
    x: f64,
    y: f64,
    prov: &BlockProvenance,
) -> Result<(f64, f64), ReflowApplyError> {
    if prov.ctm_a.abs() < MTX_EPS || prov.ctm_d.abs() < MTX_EPS {
        return Err(ReflowApplyError::Unsupported(
            UnsupportedCause::DegenerateCtm,
        ));
    }
    Ok(((x - prov.ctm_e) / prov.ctm_a, (y - prov.ctm_f) / prov.ctm_d))
}

/// The operator bytes that put the §9.3 text state back the way the
/// reflowed region left it — the symmetric half of the preamble.
///
/// # The obligation (decision 019 §3.4, standing rule R88)
///
/// A reflow replaces `[region.start, region.end)` with a fresh
/// `BT … ET`. Text state is graphics state and **survives `ET`** (§9.3's
/// scope rule: "retained across text objects in a single content stream"),
/// so whatever the new body leaves in force is what the *next* operator in
/// the stream sees. If that differs from what the next operator saw before
/// the reflow, pdfcer has silently changed content it did not logically
/// touch — a direct R32/R46 minimal-diff violation, and exactly the
/// rule-4 failure mode this project exists not to commit.
///
/// # The arithmetic
///
/// For each of the six modelled parameters, the value in force after the
/// new body is:
///
/// ```text
/// after(p) = emitted(p)          if the preamble emitted p
///          = entry.get(p).value  otherwise   (the operators that used to
///                                             set it were inside the
///                                             replaced region, and are gone)
/// ```
///
/// and the obligation is `after(p) == exit.get(p).value` for every `p`.
/// Where it does not hold, the restore emits `exit`'s bytes for that
/// parameter — the R88 ladder: the spec default when the parameter was
/// provably never set, the **observed raw operand bytes** when it was (so
/// `0.5000 Tc` goes back as `0.5000 Tc`), and a refusal when the value is
/// unobservable.
///
/// # Why this usually emits nothing, and why that is the point
///
/// Under the justify gate above (`|Tc| ≤ ε` and `|Tw| ≤ ε` whenever `TJ`
/// slack is used) and the non-justify path's habit of re-emitting values
/// **equal** to the ambient, `after(p) == exit(p)` already holds for every
/// parameter on every fixture in the corpus — which is precisely why the
/// missing restore was benign rather than a shipped bug. It is written now,
/// while it is a no-op, so that the slice which relaxes the gate (19.1) is
/// relaxing a gate rather than opening a hole. An empty return is the
/// correct, expected result today; the value of the function is that it
/// stops being empty the moment the emitted state and the ambient state
/// diverge, without anyone having to remember to add it.
///
/// # Errors
///
/// [`AmbientRestoreError`] when a restore is required but the ambient value
/// is [`AmbientOrigin::Unobservable`](crate::text_state::AmbientOrigin) —
/// refuse and disclose, never guess the Table 105 default.
pub(super) fn restore_ops(
    emitted: &[(TextStateParam, f64)],
    entry: &AmbientTextState,
    exit: &AmbientTextState,
) -> Result<Vec<u8>, AmbientRestoreError> {
    let mut out = Vec::new();
    for param in TextStateParam::ALL {
        let after = emitted
            .iter()
            .find(|(p, _)| *p == param)
            .map_or_else(|| entry.get(param).value, |(_, v)| *v);
        let wanted = exit.get(param).value;
        // Bit-for-bit comparison, deliberately. An epsilon here would let a
        // tiny-but-real divergence through silently, and the whole point of
        // this function is that a divergence is never silent. The values
        // being compared are both `f64`s that travelled the same parse
        // path, so an exact match is the normal case rather than a lucky
        // one.
        if after == wanted {
            continue;
        }
        out.extend_from_slice(&exit.restore_bytes(param)?);
        out.push(b'\n');
    }
    Ok(out)
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
    use crate::text_edit::reflow_walk::{operand_numbers, text_op_span};
    use crate::text_extract::ExtractOptions;

    /// A `/Font` subdictionary entry: (resource key bytes, font-dict body).
    type FontEntry = (Vec<u8>, Vec<u8>);
    /// An extra indirect object: (object number, body bytes).
    type ObjEntry = (u32, Vec<u8>);

    // -- test PDF builders ---------------------------------------------

    /// A one-page PDF with the given content stream and one `/Font`
    /// subdictionary entry (object 5), mirroring the 14.1/14.2 helpers.
    fn build_pdf(
        content: &str,
        fonts: &[(Vec<u8>, Vec<u8>)],
        extra_objs: &[(u32, Vec<u8>)],
    ) -> Vec<u8> {
        let mut objects: Vec<(u32, Vec<u8>)> = Vec::new();
        let mut font_dict = Vec::new();
        font_dict.extend_from_slice(b"<< ");
        for (i, (key, _)) in fonts.iter().enumerate() {
            let num = 5 + i as u32;
            font_dict.push(b'/');
            font_dict.extend_from_slice(key);
            font_dict.extend_from_slice(format!(" {num} 0 R ").as_bytes());
        }
        font_dict.extend_from_slice(b">>");

        objects.push((1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()));
        let mut pages = b"<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 612 792] \
              /Resources << /Font "
            .to_vec();
        pages.extend_from_slice(&font_dict);
        pages.extend_from_slice(b" >> >>");
        objects.push((2, pages));
        objects.push((
            3,
            b"<< /Type /Page /Parent 2 0 R /Contents 4 0 R >>".to_vec(),
        ));
        let body = content.as_bytes();
        let mut s = format!("<< /Length {} >>\nstream\n", body.len()).into_bytes();
        s.extend_from_slice(body);
        s.extend_from_slice(b"\nendstream");
        objects.push((4, s));
        for (i, (_, obj)) in fonts.iter().enumerate() {
            objects.push((5 + i as u32, obj.clone()));
        }
        for (num, obj) in extra_objs {
            objects.push((*num, obj.clone()));
        }
        objects.sort_by_key(|(n, _)| *n);

        let highest = objects.iter().map(|(n, _)| *n).max().unwrap_or(4);
        let mut out = b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n".to_vec();
        let mut offsets = std::collections::BTreeMap::new();
        for (num, obj) in &objects {
            offsets.insert(*num, out.len());
            out.extend_from_slice(format!("{num} 0 obj\n").as_bytes());
            out.extend_from_slice(obj);
            out.extend_from_slice(b"\nendobj\n");
        }
        let xref_at = out.len();
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

    /// Courier metrics: every glyph (incl. space) is 600/1000 em, so at size
    /// 10 each advances exactly 6.0 pt — an exact, hand-computable geometry.
    fn courier() -> (Vec<u8>, Vec<u8>) {
        (
            b"F1".to_vec(),
            b"<< /Type /Font /Subtype /Type1 /BaseFont /Courier /Encoding /WinAnsiEncoding >>"
                .to_vec(),
        )
    }

    /// A Courier-metric Type1 with a /FontDescriptor carrying a /FontFile —
    /// an EMBEDDED, non-subset simple font (classified Embedded; the dummy
    /// program is never parsed for widths — /Widths would be, but Courier's
    /// std-14 metrics serve). Object 5 = font, 6 = descriptor, 7 = FontFile.
    fn embedded_courier() -> (FontEntry, Vec<ObjEntry>) {
        let font = b"<< /Type /Font /Subtype /Type1 /BaseFont /Courier \
                     /Encoding /WinAnsiEncoding /FontDescriptor 6 0 R >>"
            .to_vec();
        let descriptor =
            b"<< /Type /FontDescriptor /FontName /Courier /Flags 33 /FontFile 7 0 R >>".to_vec();
        let mut ff = b"<< /Length 5 /Length1 5 >>\nstream\n".to_vec();
        ff.extend_from_slice(b"dummy");
        ff.extend_from_slice(b"\nendstream");
        ((b"F1".to_vec(), font), vec![(6, descriptor), (7, ff)])
    }

    fn load(bytes: &[u8]) -> Document {
        Document::from_bytes(bytes.to_vec()).unwrap()
    }

    /// Extract page 0 WITH provenance and return the model-independent glyphs
    /// for position assertions.
    fn reextract(bytes: &[u8]) -> crate::text_extract::PageText {
        let doc = load(bytes);
        let pages = page_tree::pages(&doc).unwrap();
        let opts = ExtractOptions::default().with_provenance(true);
        text_extract::extract_page(&doc, &pages[0], 0, &opts).unwrap()
    }

    // -- non-embedded block re-wrap, minimal-diff ----------------------

    #[test]
    fn rewrap_non_embedded_block_changes_only_content_and_is_incremental() {
        // Three source lines, one left block; re-wrap to a wide width so it
        // collapses to fewer lines. Non-embedded Courier.
        let src = build_pdf(
            "BT /F1 10 Tf 72 740 Td (alpha beta) Tj ET\n\
             BT /F1 10 Tf 72 726 Td (gamma delta) Tj ET\n\
             BT /F1 10 Tf 72 712 Td (epsilon) Tj ET\n",
            &[courier()],
            &[],
        );
        let doc = load(&src);
        let out = apply_reflow(&doc, 0, 0, &ReflowRequest::new().with_wrap_width(400.0)).unwrap();
        // Incremental: the original file is a byte-prefix of the output.
        assert_eq!(out.bytes.get(..src.len()), Some(src.as_slice()));
        assert!(out.bytes.len() > src.len());
        // Only the content object (4) changed — re-extract shows the same
        // words, re-wrapped onto one line (all five words fit in 400pt).
        let page = reextract(&out.bytes);
        let text = page.sourced_text();
        assert!(
            text.contains("alpha beta gamma delta epsilon"),
            "got {text:?}"
        );
        assert_eq!(out.report.glyph_source, EditGlyphSource::NonEmbedded);
        assert_eq!(out.report.lines_after, 1);
    }

    // -- embedded-full block re-wrap -----------------------------------

    #[test]
    fn rewrap_embedded_block_is_reported_embedded() {
        let (font, extra) = embedded_courier();
        let src = build_pdf(
            "BT /F1 10 Tf 72 740 Td (alpha beta) Tj ET\n\
             BT /F1 10 Tf 72 726 Td (gamma) Tj ET\n",
            &[font],
            &extra,
        );
        let doc = load(&src);
        let out = apply_reflow(&doc, 0, 0, &ReflowRequest::new().with_wrap_width(400.0)).unwrap();
        assert_eq!(out.report.glyph_source, EditGlyphSource::Embedded);
        assert_eq!(out.bytes.get(..src.len()), Some(src.as_slice()));
        let text = reextract(&out.bytes).sourced_text();
        assert!(text.contains("alpha beta gamma"), "got {text:?}");
    }

    // -- justified: slack distributed, right edge flush, last line not ----

    #[test]
    fn justified_line_lands_flush_at_the_box_right_edge() {
        // Four words on the first line, then a short last line. Force
        // justified; wrap width chosen so the first line is full and its
        // right edge must land at llx + width. Courier @10: each glyph 6pt.
        // "aa bb cc dd" natural = 2*4 words *6 + 3 spaces*6 ... compute:
        // each word 2 glyphs = 12; 4 words = 48; 3 gaps *6 = 18; nat=66.
        let src = build_pdf(
            "BT /F1 10 Tf 72 740 Td (aa bb cc dd ee ff) Tj ET\n\
             BT /F1 10 Tf 72 726 Td (end) Tj ET\n",
            &[courier()],
            &[],
        );
        let doc = load(&src);
        // Width 120: greedy packs as many words as fit. Each word 12, gap 6:
        // running width 12,30,48,66,84,102 (6 words = 102 <=120) -> all six
        // on line 1, then "end" on line 2. Justify line 1 to flush at 72+120.
        let out = apply_reflow(
            &doc,
            0,
            0,
            &ReflowRequest::new()
                .with_wrap_width(120.0)
                .with_alignment(BlockAlignment::Justified),
        )
        .unwrap();
        assert!(out.report.justified_lines >= 1, "a full line was justified");
        let page = reextract(&out.bytes);
        // Find the glyphs of the first (justified) line: baseline y≈740.
        let mut max_right = f32::MIN;
        for run in &page.runs {
            for g in &run.glyphs {
                if (g.y - 740.0).abs() < 0.5 {
                    max_right = max_right.max(g.x + g.advance);
                }
            }
        }
        // The right edge of the last glyph on the justified line lands flush
        // at the box right margin 72 + 120 = 192 (within a small tolerance —
        // exact for Courier's integer metrics).
        assert!(
            (max_right - 192.0).abs() < 0.5,
            "justified right edge = {max_right}, expected 192"
        );
        // The LAST line ("end") is NOT justified: its glyphs sit at the left
        // origin 72, natural width, not stretched.
        let mut last_line_left = f32::MAX;
        for run in &page.runs {
            for g in &run.glyphs {
                if (g.y - 726.0).abs() < 0.5 {
                    last_line_left = last_line_left.min(g.x);
                }
            }
        }
        assert!((last_line_left - 72.0).abs() < 0.5, "last line at left 72");
    }

    // -- page overflow: content EMITTED off-page, not clipped -----------

    /// **A wrap width wider than the page is disclosed** (R148).
    ///
    /// The composition this catches: `edit-text` pushes a replacement past the
    /// right margin — honestly disclosing that it may have — which WIDENS the
    /// block's bounding box. Reflow's wrap width defaults to that box, so the
    /// next re-wrap faithfully honours a width the operator never chose and
    /// puts text off the page, while reporting a successful re-wrap.
    ///
    /// Measured on a real run before the fix: a 156 pt block became 930 pt on
    /// a 612 pt page, and reflow reported "re-wrapped from 4 to 2 lines" with
    /// no mention that the result was off the page.
    ///
    /// Nothing caught it because the only overflow check was
    /// `crop.lly - new_bbox.lly` — purely vertical. That reads as complete,
    /// because a re-wrap grows DOWNWARD; the horizontal axis is threatened by
    /// the wrap width instead, which is a different mechanism entirely.
    #[test]
    fn a_wrap_width_past_the_page_right_edge_is_disclosed() {
        let src = build_pdf(
            "BT /F1 10 Tf 20 40 Td (aa bb cc dd ee) Tj ET
",
            &[courier()],
            &[(
                3,
                b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 60] /Contents 4 0 R >>".to_vec(),
            )],
        );
        let doc = load(&src);
        // Wrap to 400 pt on a 100 pt-wide page: the block starts at x=20, so
        // it ends 320 pt past the right edge.
        let out = apply_reflow(&doc, 0, 0, &ReflowRequest::new().with_wrap_width(400.0)).unwrap();

        let ov = out.report.overflow.expect("overflow computed");
        assert!(
            (ov.past_right_pt - 320.0).abs() < 0.5,
            "expected ~320pt past the right edge, got {}",
            ov.past_right_pt
        );
        assert!(
            out.report
                .disclosures
                .iter()
                .any(|d| d.contains("RIGHT edge") && d.contains("EMITTED")),
            "the horizontal overflow must be disclosed as emitted, not clipped: {:?}",
            out.report.disclosures
        );
    }

    /// **And the vertical note must not fire at 0.0pt.**
    ///
    /// `overflow` being `Some` no longer implies the BOTTOM overflowed — it is
    /// `Some` when EITHER axis does. An unguarded note therefore reported
    /// "grows the block 0.0pt past the page bottom" for a block that only ran
    /// off the right, which is a disclosure false in its letter while the true
    /// one went unsaid. Caught by reading the CLI output of the very run this
    /// fix was written for, not by a test.
    #[test]
    fn a_right_only_overflow_does_not_claim_a_bottom_overflow() {
        let src = build_pdf(
            "BT /F1 10 Tf 20 40 Td (aa bb) Tj ET
",
            &[courier()],
            &[(
                3,
                b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 200] /Contents 4 0 R >>".to_vec(),
            )],
        );
        let doc = load(&src);
        // Tall page (no vertical overflow), very wide wrap (horizontal only).
        let out = apply_reflow(&doc, 0, 0, &ReflowRequest::new().with_wrap_width(400.0)).unwrap();
        let ov = out.report.overflow.expect("overflow computed");
        assert_eq!(ov.past_bottom_pt, 0.0, "nothing overflowed the bottom");
        assert!(
            !out.report
                .disclosures
                .iter()
                .any(|d| d.contains("past the page bottom")),
            "must not claim a bottom overflow that did not happen: {:?}",
            out.report.disclosures
        );
    }

    #[test]
    fn overflow_emits_all_lines_below_the_page_and_discloses() {
        // A small page; narrow the wrap so the block grows past the bottom.
        let src = build_pdf(
            "BT /F1 10 Tf 20 40 Td (aa bb cc dd ee) Tj ET\n\
             BT /F1 10 Tf 20 26 Td (ff gg hh) Tj ET\n",
            &[courier()],
            &[(
                // page overrides its MediaBox to a tiny 100x60 box
                3,
                b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 60] /Contents 4 0 R >>".to_vec(),
            )],
        );
        let doc = load(&src);
        let out = apply_reflow(&doc, 0, 0, &ReflowRequest::new().with_wrap_width(12.0)).unwrap();
        // Overflow disclosed.
        assert!(out.report.overflow.is_some(), "overflow computed");
        assert!(
            out.report
                .disclosures
                .iter()
                .any(|d| d.contains("past the page bottom") && d.contains("EMITTED")),
            "overflow disclosed as emitted, not clipped: {:?}",
            out.report.disclosures
        );
        // Every word is still present in the re-extracted content (nothing
        // dropped), and some glyphs sit at a NEGATIVE baseline (off-page).
        let page = reextract(&out.bytes);
        let text = page.sourced_text().replace(['\n', ' '], "");
        for w in ["aa", "bb", "cc", "dd", "ee", "ff", "gg", "hh"] {
            assert!(text.contains(w), "word {w} survived: {text:?}");
        }
        let has_off_page = page.runs.iter().flat_map(|r| &r.glyphs).any(|g| g.y < 0.0);
        assert!(has_off_page, "some content emitted below the page (y<0)");
    }

    // -- composite with no space glyph ----------------------------------

    #[test]
    fn composite_block_joining_lines_without_a_space_glyph_is_refused() {
        // A Type0/Identity-H font that never shows a space: joining its two
        // lines needs a word gap no code in the block supplies.
        let font = b"<< /Type /Font /Subtype /Type0 /BaseFont /F+Sub /Encoding /Identity-H \
                     /DescendantFonts [7 0 R] >>"
            .to_vec();
        let descendant = b"<< /Type /Font /Subtype /CIDFontType2 /BaseFont /F+Sub \
                           /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> \
                           /FontDescriptor 8 0 R /DW 1000 >>"
            .to_vec();
        let descriptor = b"<< /Type /FontDescriptor /FontName /F+Sub /Flags 4 >>".to_vec();
        let src = build_pdf(
            "BT /F1 10 Tf 72 740 Td <00410042> Tj ET\n\
             BT /F1 10 Tf 72 726 Td <00430044> Tj ET\n",
            &[(b"F1".to_vec(), font)],
            &[(7, descendant), (8, descriptor)],
        );
        let doc = load(&src);
        let err =
            apply_reflow(&doc, 0, 0, &ReflowRequest::new().with_wrap_width(400.0)).unwrap_err();
        assert!(
            matches!(
                err,
                ReflowApplyError::Unsupported(UnsupportedCause::NoSpaceGlyph { .. })
            ),
            "got {err:?}"
        );
    }

    // -- rotated text refused ------------------------------------------

    #[test]
    fn rotated_block_is_refused() {
        // A 90° rotation folded into Tm: [0 1 -1 0 x y].
        let src = build_pdf(
            "BT /F1 10 Tf 0 1 -1 0 72 740 Tm (alpha beta) Tj ET\n\
             BT /F1 10 Tf 0 1 -1 0 86 740 Tm (gamma) Tj ET\n",
            &[courier()],
            &[],
        );
        let doc = load(&src);
        let err = apply_reflow(&doc, 0, 0, &ReflowRequest::new()).unwrap_err();
        assert!(
            matches!(
                err,
                ReflowApplyError::Unsupported(UnsupportedCause::RotatedOrSkewed { .. })
            ),
            "rotated text refused by name"
        );
    }

    // -- Pass 19.0: the ET state leak, closed --------------------------

    /// The text state left in force by the whole content stream, walked
    /// with the same shared rule the surgery uses.
    ///
    /// This is the *observable* form of "nothing bled past `ET`": if the
    /// re-emitted body leaks, the state at end of stream changes, and every
    /// operator after the block sees a different world.
    fn end_of_stream_state(bytes: &[u8]) -> AmbientTextState {
        let doc = load(bytes);
        let pages = page_tree::pages(&doc).unwrap();
        let stream = ContentStream::from_page(&doc.view(), &pages[0]).unwrap();
        let mut ambient = AmbientTextState::initial();
        let mut stack: Vec<AmbientTextState> = Vec::new();
        for op in stream.operations() {
            let Some(name) = op.operator_name(&stream.buf) else {
                continue;
            };
            match name {
                b"q" => stack.push(ambient.clone()),
                b"Q" => {
                    if let Some(prev) = stack.pop() {
                        ambient = prev;
                    }
                }
                _ => {
                    let (s, e) = text_op_span(&op);
                    let raw = stream.buf.get(s..e).unwrap_or_default();
                    ambient.apply_operator(name, &operand_numbers(&op), raw);
                }
            }
        }
        ambient
    }

    /// The acceptance test named in decision 019 §19.0: reflow a block that
    /// is followed by unrelated text in the same stream, and prove the
    /// following text's state is untouched.
    ///
    /// Note this passes *today* — the leak was latent, masked by the
    /// justify gate (which is deliberately left in place by this slice).
    /// The test is a tripwire: it fails the moment a later slice relaxes
    /// the gate without the restore, which is exactly the sequence
    /// decision 019 §3.4 exists to prevent.
    #[test]
    fn reflow_leaves_the_following_text_state_unchanged() {
        let src = build_pdf(
            "0.75 Tc 95 Tz\n\
             BT /F1 10 Tf 72 740 Td (alpha beta) Tj ET\n\
             BT /F1 10 Tf 72 726 Td (gamma delta) Tj ET\n\
             BT /F1 10 Tf 72 660 Td (unrelated tail) Tj ET\n",
            &[courier()],
            &[],
        );
        let before = end_of_stream_state(&src);
        let doc = load(&src);
        let out = apply_reflow(&doc, 0, 0, &ReflowRequest::new().with_wrap_width(400.0)).unwrap();
        let after = end_of_stream_state(&out.bytes);
        assert_eq!(
            before.params(),
            after.params(),
            "the reflowed text object leaked text state past its ET"
        );
    }

    /// `restore_ops` is the mechanism; drive it directly with an emitted
    /// state that DIVERGES from the ambient, which the justify gate makes
    /// unreachable through `apply_reflow` today. Tiers 1 and 2 both.
    #[test]
    fn restore_ops_emits_the_ambient_when_the_preamble_diverges() {
        let mut entry = AmbientTextState::initial();
        entry.apply_operator(b"Tc", &[0.25], b"0.2500 Tc");
        let exit = entry.clone();

        // The preamble emitted `0 Tc` (as the justify path would) over an
        // ambient of 0.25 — a real divergence.
        let ops = restore_ops(&[(TextStateParam::CharSpacing, 0.0)], &entry, &exit).unwrap();
        assert_eq!(
            ops, b"0.2500 Tc\n",
            "tier 2: restore the observed operand bytes, not a renormalized 0.25"
        );

        // A parameter the preamble did NOT emit, whose ambient before the
        // region differs from the ambient after it (the region itself set
        // it, and the region is being replaced): the restore must reinstate
        // the AFTER value, here the never-set spec default.
        let mut exit_unset = AmbientTextState::initial();
        exit_unset.apply_operator(b"Ts", &[0.0], b"0 Ts");
        let mut entry_set = AmbientTextState::initial();
        entry_set.apply_operator(b"Ts", &[4.0], b"4 Ts");
        let ops = restore_ops(&[], &entry_set, &exit_unset).unwrap();
        assert_eq!(ops, b"0 Ts\n");
    }

    /// The no-op case, asserted rather than assumed: when the emitted state
    /// already equals the ambient (every fixture in the corpus today), the
    /// restore is empty and the output bytes are unchanged. This is what
    /// keeps Pass 19.0 a correctness slice with no output movement.
    #[test]
    fn restore_ops_emits_nothing_when_nothing_diverges() {
        let mut ts = AmbientTextState::initial();
        ts.apply_operator(b"Tc", &[0.75], b"0.75 Tc");
        ts.apply_operator(b"Tz", &[95.0], b"95 Tz");
        let ops = restore_ops(
            &[
                (TextStateParam::CharSpacing, 0.75),
                (TextStateParam::HorizScale, 95.0),
            ],
            &ts,
            &ts,
        )
        .unwrap();
        assert!(ops.is_empty(), "got {:?}", String::from_utf8_lossy(&ops));
    }

    /// R88 tier 3 propagates: an unobservable ambient makes the restore a
    /// **refusal**, never a guessed default.
    #[test]
    fn restore_ops_refuses_an_unobservable_ambient() {
        let mut entry = AmbientTextState::initial();
        entry.apply_operator(b"Tc", &[0.25], b"0.25 Tc");
        let mut exit = entry.clone();
        exit.enter_form(Some(9));

        let err = restore_ops(&[(TextStateParam::CharSpacing, 0.0)], &entry, &exit)
            .expect_err("an inherited ambient cannot be restored");
        let msg = err.to_string();
        assert!(msg.contains("character spacing"), "{msg}");
        assert!(msg.contains("refuses"), "{msg}");
    }
}
