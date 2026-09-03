//! # Vector-object content-stream SURGERY (Pass 9c-min, decision 011 §2.5)
//!
//! The **write half** of the vector object model: the three, and only
//! three, basic-editing operations decision 011 §2.5 scopes for the first
//! beta —
//!
//! 1. **move object** — translate every path-construction operand of a
//!    selected object by a page-space `(dx, dy)`, CTM-aware
//!    ([`plan_move`]);
//! 2. **delete object** — remove an object's construction **and** painting
//!    operators from the content stream ([`plan_delete`]);
//! 3. **drag node** — rewrite ONE anchor's coordinate pair in an `m`/`l`/
//!    `c`/`v`/`y` operand list ([`plan_move_node`]).
//!
//! All three are **content-stream surgery** through the same
//! advance-preserving interpreter Pass 8.0 (redaction) and Pass 14.x
//! (text edit) are built on — the **R46 named exception, ISO 32000-1 §5.7**
//! (`docs/ARCHITECTURE.md` §5.7): the object's operator byte range is
//! located from the read-only Pass 9a decomposition
//! ([`super::decompose`]), the numeric operands are rewritten (or the whole
//! run removed), and ONLY the edited content stream is re-emitted; every
//! other object in the file stays **byte-verbatim**. This module is the
//! geometric mirror of [`crate::redact`]'s operator removal.
//!
//! ## What this module does and does not do (crate placement)
//!
//! It is a set of **pure planners**: each takes a tokenized
//! [`ContentStream`] plus the target object (from the SAME decomposition)
//! and returns the **new decoded content buffer** ([`PlannedEdit::content`])
//! — it never touches a [`Document`](crate::document::Document), a writer,
//! or the undo stack. The session-integrated, one-undoable-command wrappers
//! that stage the new bytes and re-emit exactly the edited stream live in
//! [`crate::edit::EditSession`] (`move_object`/`delete_object`/`move_node`),
//! mirroring how [`crate::text_edit::edit::plan_edit`] feeds
//! [`crate::edit::EditSession::edit_text`]. The whole module is GUI-free
//! (`pdfcer-core`, no egui/eframe/winit/wgpu — the load-bearing invariant),
//! so the eventual WASM fork inherits the surgery unchanged; the GUI owns
//! only the drag gesture that produces the `(dx, dy)` / node index.
//!
//! ## Coordinate spaces (the CTM round-trip, §8.3.4)
//!
//! An object's construction operands live in the **user space** its
//! captured CTM ([`PathObject::ctm`]) maps *from*; the operator's drag and
//! the snap target are in **page space** (default user space, §8.3.2.3).
//! So [`plan_move`] converts the page-space displacement to a user-space
//! displacement with the CTM's **linear inverse** ([`Matrix::map_vector`]
//! ∘ [`Matrix::inverse`] — the delta transform, translation excluded), and
//! [`plan_move_node`] converts the page-space target *point* with the full
//! affine inverse ([`Matrix::map_point`] ∘ [`Matrix::inverse`]). A singular
//! CTM (an object flattened to a line) has no unambiguous pre-image and is
//! refused by name ([`VectorEditError::DegenerateCtm`]) — never fabricated
//! (rule 4, fuzzy-never-sneaky).
//!
//! ## Agreement with Pass 9a (node ordering, decision 011 Z2)
//!
//! [`plan_move_node`]'s `node_index` is the index into the object's anchors
//! in **decomposition order** — the flattening of
//! `obj.subpaths.flat_map(Subpath::anchors)` the snap engine and GUI node
//! hit-test already present. This module reproduces the EXACT subpath /
//! empty-subpath / `h`-reopen bookkeeping [`super::decompose`] uses, so the
//! nth anchor a caller sees and the nth anchor this surgery rewrites are the
//! same anchor by construction (the geometry analogue of the R49/R60 "one
//! pipeline" discipline), not by two hand-derived orderings kept in sync.
//!
//! ## Anchors whose coordinates are written NOWHERE (Pass 30.0)
//!
//! Two anchor kinds have no operand of their own to overwrite, and both were
//! refused for that reason until Pass 30.0:
//!
//! - an **`re` rectangle corner**. `re` carries an origin and a *size*, so
//!   only corner 0 appears literally, and even it cannot move alone — editing
//!   `x y` slides all four. Worse, the shape a dragged corner produces is in
//!   general NOT a box, and `re` has no spelling for that shape at all.
//! - the **implicit reused start** of a subpath reopened after `h`: the
//!   segment inherits the closed subpath's start point (§8.5.2.1) rather than
//!   naming it.
//!
//! Both are now edited by *materializing the missing operand* rather than by
//! refusing. The rectangle is expanded to the spec's own stated equivalent —
//! `x y m`, `x+w y l`, `x+w y+h l`, `x y+h l`, `h` (§8.5.2.1, Table 59) —
//! whose trailing `h` is load-bearing: a stroked subpath left open takes two
//! line caps where the closed one takes a corner join, so dropping it would
//! change the picture. The implicit start gets the `m` the file omitted,
//! inserted immediately before the segment that inherited it, which no earlier
//! geometry can observe because `h` has already terminated the subpath before
//! it.
//!
//! Both rewrites leave the anchor COUNT and ORDER unchanged, which is what
//! lets a front end hold a node index across the drag it just performed.
//! Both are [disclosed](PlannedEdit::disclosures): the drawing is identical,
//! the bytes are not, and dragging back does not restore the original form.
//!
//! ## Panic-free / adversarial input (ARCHITECTURE.md §10)
//!
//! Every operand access is checked; a construction operator whose operand
//! arity does not match the spec (§8.5.2.1, Table 59) is left byte-verbatim
//! for a node-drag (only the one edited operator is re-emitted) and is a
//! by-name refusal ([`VectorEditError::MalformedOperand`]) for a whole-object
//! move (a partially-moved shape would be torn — refused, never silently
//! half-applied). Degenerate coordinates (`NaN`, `±∞`, huge magnitudes) are
//! re-emitted through [`emit_number`], which is total. The fuzz target
//! `fuzz/fuzz_targets/vector_edit.rs` drives exactly these shapes.

use std::collections::{BTreeMap, BTreeSet};

use crate::content::{ContentStream, ContentToken, ContentTokenKind};
use crate::text_edit::edit::splice;
use crate::writer::content::emit_number;

use super::decompose::{PathObject, RunPositioning, TextObject, VectorObject};
use super::geometry::{Matrix, Point, Rgb, rect_corners};

/// Why a vector-edit surgery could not be planned.
///
/// Every variant names a condition the operator (or the calling front end)
/// can act on; there is deliberately no catch-all "edit failed" (mirrors
/// [`crate::edit::EditError`]'s discipline). Surgery that cannot be
/// performed cleanly is refused **before** any byte is produced — the
/// caller's session is never left half-edited (rule 4).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum VectorEditError {
    /// The object index is past the end of the page's decomposition.
    #[error("object index {index} is out of range (the page decomposes to {count} object(s))")]
    ObjectOutOfRange {
        /// The 0-based index that was asked for.
        index: usize,
        /// How many selectable objects the page actually has.
        count: usize,
    },
    /// A move or node-drag was requested on an object that is not a
    /// **path** (a text or image/form object). Text and image objects are
    /// selectable-for-move/delete in the model, but node editing and
    /// operand-translation are path-only in the beta (decision 011 §2.1:
    /// text/image are "not node-editable"). Move of a text/image object is
    /// a named fast-follow (it needs `Tm`/`cm`-operand surgery, a different
    /// operator family); 9c-min moves **paths**.
    #[error(
        "object {index} is not a path object (it is {kind}), which 9c-min move/node editing does not cover"
    )]
    NotAPath {
        /// The object's index.
        index: usize,
        /// A short kind label (`"text"` / `"image"`), for the diagnostic.
        kind: &'static str,
    },
    /// The object's captured CTM is singular (non-invertible), so a
    /// page-space drag has no unambiguous user-space pre-image. Refused
    /// rather than fabricated (rule 4).
    #[error(
        "the object's transform is singular (non-invertible), so a page-space drag cannot be mapped to its user space"
    )]
    DegenerateCtm,
    /// A whole-object move hit a construction operator whose operand arity
    /// does not match the spec (Table 59), so the object cannot be moved
    /// **as a whole** without tearing it. Refused by name; the object is
    /// left untouched.
    #[error(
        "the object contains a malformed construction operator (unexpected operand count), so it cannot be moved without tearing it"
    )]
    MalformedOperand,
    /// The **requested** transform is singular — it maps area to zero, so the
    /// object would collapse to a line or a point (`Pass 113.0`).
    ///
    /// # Why this is refused by default rather than applied
    ///
    /// A singular transform is **irrecoverable**. There is no inverse to apply
    /// later, so no subsequent gesture restores the object: it is data loss
    /// under a drag the operator almost certainly did not mean. The consuming
    /// shell asked for this case to be distinguishable from
    /// [`Self::DegenerateCtm`] — *"this object cannot be transformed at all"*
    /// — because the two produce different UI: one means do not offer a
    /// handle, this one means offer the handle and refuse on release.
    ///
    /// ★ **A negative scale is NOT this.** `scale(-1.0, 1.0)` is a mirror and
    /// is perfectly invertible, so dragging a resize grip through the opposite
    /// edge is an ordinary transform. Only *exactly* zero area is degenerate,
    /// which a commit-on-release gesture makes nearly unreachable — the
    /// default therefore costs a well-behaved shell nothing while still
    /// catching a gesture that forgets to clamp.
    ///
    /// [`TransformOptions::singular`] selects the clamp-and-disclose
    /// alternative (`R206`: both behaviours ship, the default is picked from
    /// what an ordinary operator would expect).
    #[error(
        "the requested transform is singular (it maps area to zero), so the selection would collapse to a line or a point with no inverse to undo it"
    )]
    SingularTransform,
    /// A clamp was requested for a singular transform that has no
    /// axis-aligned reading, so there is nothing well-defined to clamp
    /// (`Pass 113.0`).
    ///
    /// [`SingularPolicy::Clamp`] replaces a zero scale factor with a minimum,
    /// which is exactly what a resize gesture produces (`b == 0 && c == 0`).
    /// A singular matrix that also shears has no single "the scale that went
    /// to zero" — its degeneracy is a direction, not an axis — and inventing
    /// one would be pdfcer choosing a shape the operator did not draw. Refused
    /// by name, naming the option rather than the gesture, so the caller can
    /// tell "you asked for a clamp I cannot express" from "I refuse singular
    /// transforms".
    #[error(
        "a clamp was requested but this singular transform is not axis-aligned (it shears), so there is no single scale factor to clamp -- refusing rather than inventing a shape"
    )]
    ClampNotExpressible,
    /// The selection holds objects of more than one kind and the caller asked
    /// for single-kind semantics (`Pass 113.0`,
    /// [`MixedSelection::RefuseHeterogeneous`]).
    ///
    /// **Not the default.** A marquee-then-drag over a drawing selects
    /// whatever is under it, and refusing on kind would reopen the `NotAPath`
    /// complaint this verb exists to close.
    #[error(
        "the selection holds more than one kind of object ({first} and {second}) and single-kind semantics were requested"
    )]
    HeterogeneousSelection {
        /// The first kind seen, in selection order.
        first: &'static str,
        /// The first kind that differed from it.
        second: &'static str,
    },
    /// Two objects selected for deletion have **partially overlapping** byte
    /// spans — each starts inside the other and runs past its end.
    ///
    /// Impossible in a well-formed decomposition, where objects are emitted
    /// in paint order and therefore nest or sit apart. Its appearance means
    /// the object model and the content buffer disagree, and splicing anyway
    /// would emit half of one operator's operands followed by the tail of
    /// another's — a content stream that still parses, still renders
    /// something, and is wrong in a way no diff explains. Refused by name.
    #[error(
        "two selected objects have partially overlapping byte spans (one starts at {start} inside another that ends after {end}), which a paint-order decomposition cannot produce — refusing rather than splicing a torn content stream"
    )]
    OverlappingObjectSpans {
        /// The later span's start offset.
        start: usize,
        /// The later span's end offset.
        end: usize,
    },
    /// Deleting this subpath would silently MOVE the next one.
    ///
    /// The following subpath was started implicitly — by a segment operator
    /// after `h`, which reopens at the closed subpath's start point
    /// (§8.5.2.1). Its start is INHERITED and carried by no operand, so
    /// excising the operators before it changes where it begins: a
    /// byte-minimal edit that passes `--verify-undo` and every content-identity
    /// check, and is still wrong.
    #[error(
        "deleting part {index} would move the part after it, which starts where this one ends rather than at coordinates of its own"
    )]
    DeleteWouldMoveNextSubpath {
        /// The subpath whose deletion was refused.
        index: usize,
    },
    /// Deleting this point would leave the part with fewer than two points —
    /// which is not a shorter line, it is not a line (Pass 36.1).
    ///
    /// Refused rather than silently promoted to a whole-part delete. Removing
    /// a part is an operation the operator can name and already has
    /// ([`plan_delete_subpath`]); doing it *for* them, under a keystroke that
    /// said "remove this point", is the class of surprise rule 4 exists to
    /// forbid — and it is the same mistake, one rung down, that the GUI made
    /// before Pass 36.0 when Delete on a point deleted the part.
    #[error(
        "removing this point would leave part {subpath} with {remaining} point(s), which no longer draws anything — delete the whole part instead"
    )]
    NodeDeleteWouldEmptySubpath {
        /// The subpath the point belongs to.
        subpath: usize,
        /// How many points would remain.
        remaining: usize,
    },
    /// The point belongs to an `re` rectangle, which has no operand naming it
    /// (Pass 36.1).
    ///
    /// `re x y w h` writes an origin and a *size*, so three of the four
    /// corners appear nowhere in the bytes. [`plan_move_node`] handles a
    /// rectangle corner by EXPANDING the operator to its §8.5.2.1
    /// `m`/`l`/`l`/`l` equivalent and disclosing the change of form — a
    /// rectangle whose corner moved is still a closed quadrilateral, so the
    /// expansion preserves what the operator sees.
    ///
    /// Deleting a corner does not have that property: the result is a
    /// triangle. That is not a change of *form*, it is a change of *shape*,
    /// and pdfcer will not make one under a request to remove a point. Named
    /// rather than silently expanded.
    #[error(
        "this point is a corner of a rectangle, which is written as an origin and a size rather than as four corners — removing one corner would turn it into a triangle, so it is not done automatically"
    )]
    NodeDeleteRectangleCorner,
    /// The point is the INHERITED start of an `h`-reopened subpath
    /// (§8.5.2.1), carried by no operand of its own (Pass 36.1).
    ///
    /// [`plan_move_node`] materializes the missing `m` and moves it, which is
    /// a pure addition. Deletion cannot borrow that trick: the coordinates
    /// being removed are *the previous subpath's* start point, so honouring
    /// the request means reaching into a part the operator did not select.
    #[error(
        "this point is inherited from the part before it rather than written down, so removing it would change that other part instead"
    )]
    NodeDeleteImplicitStart,
    /// A subpath delete named an index past the object's subpath count.
    #[error("subpath index {index} is out of range (the object has {count} subpath(s))")]
    SubpathOutOfRange {
        /// The 0-based subpath index that was asked for.
        index: usize,
        /// How many subpaths the object has, in decomposition order.
        count: usize,
    },
    /// A subpath delete was asked for on a path that establishes a **clipping
    /// region** (`W` / `W*`, §8.5.4) rather than painting marks.
    ///
    /// Refused because the visible effect would be somewhere the operator was
    /// not looking: removing one subpath of a clip changes which OTHER content
    /// shows through. Rule 4 (fuzzy, never sneaky) forbids exactly that.
    #[error(
        "this path defines a clipping region, so deleting part of it would change what other content is visible rather than removing a mark"
    )]
    ClippingPath,
    /// A subpath's recorded token range names no operator in this stream — an
    /// internal inconsistency between the decomposition and the content it was
    /// derived from.
    ///
    /// **This used to mean something else.** Before Pass 28.0, subpaths carried
    /// no token range, so an edit re-walked the operators and raised this
    /// whenever its walk disagreed in COUNT with the geometry — which happened
    /// for any object containing an implicit `h`-reopen, and refused every
    /// subpath in it. That case now has its own precise variant
    /// ([`VectorEditError::DeleteWouldMoveNextSubpath`]) and the count can no
    /// longer disagree, because one walk records both.
    ///
    /// What remains is a should-never-happen path kept as an error rather than
    /// a panic: the crate's policy is that adversarial or corrupt input is
    /// refused by name, never unwrapped (ARCHITECTURE.md §10).
    #[error(
        "this path's structure cannot be edited by subpath index ({from_operators} subpath(s) found in its operators, {from_decomposition} in the geometry), so the wrong one might be removed"
    )]
    SubpathStructureMismatch {
        /// Subpaths found by walking the object's construction operators.
        from_operators: usize,
        /// Subpaths the geometric decomposition reports.
        from_decomposition: usize,
    },
    /// A handle drag named a node with no Bézier control point on that side.
    ///
    /// Either the neighbouring segment is straight (`l`, or a rectangle edge),
    /// or there is no segment there at all (the end of an open subpath, or the
    /// far side of a subpath boundary). Refused rather than converted: turning
    /// a straight segment into a curve is a different operation with a
    /// different name, and inferring it from a drag on a handle that was never
    /// drawn is the silent reinterpretation rule 4 forbids.
    #[error(
        "node {index} has no {handle:?} curve handle — the segment on that side is straight or absent, and pdfcer will not turn a straight line into a curve without being asked"
    )]
    NoHandleHere {
        /// The node whose handle was asked for.
        index: usize,
        /// Which side was asked for.
        handle: Handle,
    },
    /// A node-drag named an anchor index past the object's anchor count.
    #[error("node index {index} is out of range (the object has {count} anchor(s))")]
    NodeOutOfRange {
        /// The 0-based node index that was asked for.
        index: usize,
        /// How many anchors the object has, in decomposition order.
        count: usize,
    },
    /// A **multi-node** drag named the same anchor twice.
    ///
    /// # Refused rather than resolved, because either resolution is a guess
    ///
    /// The two available rules — last-one-wins, and first-one-wins — are
    /// equally defensible and produce different geometry, so picking one
    /// silently would move a node to a position the caller did not
    /// unambiguously ask for. A selection set cannot legitimately contain a
    /// duplicate (it is a set), so a duplicate here means the caller built
    /// its request wrongly, and saying so is more useful than absorbing it.
    ///
    /// Checked **before** any planning, so a rejected request has changed
    /// nothing (rule 4).
    #[error("node index {index} was named more than once in one multi-node move")]
    DuplicateNodeInMove {
        /// The 0-based anchor index that appeared twice.
        index: usize,
    },
    /// A per-run text edit named a run index the text object does not have.
    #[error("text run index {index} is out of range (the object has {count} run(s))")]
    TextRunOutOfRange {
        /// The 0-based run index that was asked for.
        index: usize,
        /// How many show operators the text object has, in content order.
        count: usize,
    },
    /// Deleting this text run would silently MOVE the next one.
    ///
    /// The **exact twin** of [`Self::DeleteWouldMoveNextSubpath`], one
    /// object kind over. §9.4.2 leaves the text matrix advanced past the
    /// string a show operator drew, so a following show operator with no
    /// positioning operator between them starts *wherever this one left the
    /// pen*. Its origin is INHERITED and written nowhere, so excising this
    /// run slides the next one back — a byte-minimal edit that passes
    /// `--verify-undo` and every content-identity check, and is still
    /// wrong.
    ///
    /// # Why refused rather than repaired
    ///
    /// Materialising the next run's origin is possible in principle — emit
    /// the `Td` the producer omitted — but it requires knowing the advance
    /// the deleted string produced, to the precision the font's own metrics
    /// give, and being wrong by a fraction of a point moves a label the
    /// operator never selected. Decision 027's posture applies: refuse what
    /// has no good reading rather than guess at it. The remedy is available
    /// and is stated in the message — delete the runs in the other order,
    /// which never inherits.
    #[error(
        "deleting run {index} would move the run after it, which starts where this one ends \
         rather than at a position of its own; delete the later run first"
    )]
    DeleteWouldMoveNextRun {
        /// The run whose deletion was refused.
        index: usize,
    },
    /// A **multi-node** drag named no nodes at all.
    ///
    /// Refused rather than treated as a successful no-op: an empty move
    /// that reported success would put an entry on the undo stack that
    /// undoes nothing, and a front end looping over an empty selection
    /// would get "moved" back rather than discovering its selection was
    /// empty.
    #[error("a multi-node move must name at least one node")]
    EmptyMove,
}

/// The result of a successful surgery plan: the **new decoded content
/// buffer** plus how many operators it rewrote/removed (a magnitude a front
/// end or a report can quote; the writer counts objects, this counts
/// operators).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedEdit {
    /// The rewritten, decoded content-stream bytes — the buffer the session
    /// wrapper stages and re-emits as ONE raw (unfiltered) stream (R46
    /// named exception; every other object byte-verbatim).
    pub content: Vec<u8>,
    /// How many construction/painting operators the surgery rewrote (move:
    /// every construction operator; node-drag: exactly one; delete: the
    /// object's whole operator run counts as one removal).
    pub operators_touched: usize,
    /// What the operator must be told about HOW the edit was expressed, in
    /// operator-facing prose — empty for the common case.
    ///
    /// Populated when the surgery had to change the *form* of an operator to
    /// express the requested change, because some shapes in PDF cannot say
    /// what the operator just asked for. Dragging one corner of an `re`
    /// rectangle is the canonical case: `re` carries an origin and a size, so
    /// a four-sided shape that is not a box has no `re` spelling at all and
    /// the operator must be expanded to four lines (§8.5.2.1). The drawing is
    /// unchanged — but the bytes are not recoverable by dragging back, and an
    /// operator who cares about minimal diffs (R46) is owed that fact rather
    /// than left to find it in a diff.
    ///
    /// This is rule 4 (fuzzy, never sneaky) applied to *representation*: pdfcer
    /// may reshape how a thing is written in order to do what was asked, and
    /// says so when it does.
    pub disclosures: Vec<String>,
}

// ---------------------------------------------------------------------------
// Public planners
// ---------------------------------------------------------------------------

/// Plan a **move**: translate every path-construction operand of `obj` by
/// the page-space displacement `(dx_page, dy_page)`, CTM-aware.
///
/// The displacement is converted to user space with the CTM's linear
/// inverse (module docs), then added to each operand point: `m`/`l` shift
/// their single point, `c` all three points, `v`/`y` both explicit points,
/// and `re` its **origin only** (`x, y` — the width/height `w, h` are a
/// size, not a point, and must not move). `h` and the painting operator are
/// re-emitted byte-verbatim. The whole object's operator run is re-emitted
/// at the new coordinates; every other stream object is untouched.
///
/// # Errors
///
/// [`VectorEditError::DegenerateCtm`] (singular CTM) or
/// [`VectorEditError::MalformedOperand`] (a construction operator with a
/// spec-violating operand count — refused rather than tear the shape). Both
/// are raised before any content byte is produced.
///
/// # Examples
///
/// ```
/// use pdfcer_core::content::ContentStream;
/// use pdfcer_core::vector::{decompose, NoXObjects, Matrix, VectorObject};
/// use pdfcer_core::vector::edit::plan_move;
///
/// let cs = ContentStream::parse(b"10 20 m 100 200 l S".to_vec()).unwrap();
/// let model = decompose(&cs, Matrix::IDENTITY, &NoXObjects);
/// let VectorObject::Path(path) = &model.objects[0] else { unreachable!() };
/// // Move +5 in x, −3 in y (identity CTM ⇒ page delta == user delta).
/// let plan = plan_move(&cs, path, 5.0, -3.0).unwrap();
/// assert_eq!(plan.content, b"15 17 m 105 197 l S");
/// ```
pub fn plan_move(
    content: &ContentStream,
    obj: &PathObject,
    dx_page: f64,
    dy_page: f64,
) -> Result<PlannedEdit, VectorEditError> {
    // Page-space drag → user-space delta via the CTM's linear inverse
    // (translation excluded — a displacement, not a point).
    let inv = obj.ctm.inverse().ok_or(VectorEditError::DegenerateCtm)?;
    let d = inv.map_vector(Point::new(dx_page, dy_page));
    let (du, dv) = (d.x, d.y);

    let mut edits = move_edits(content, obj, du, dv)?;
    let touched = edits.len();

    Ok(PlannedEdit {
        content: splice(&content.buf, &mut edits),
        operators_touched: touched,
        disclosures: clip_disclosure(content, obj),
    })
}

/// The operand rewrites a whole-object move produces, as absolute offsets
/// into `content.buf` — **without** splicing them.
///
/// Factored out of [`plan_move`] so [`plan_move_many`] can collect the edits
/// of several objects and splice them **once**. That is not a tidiness
/// refactor: splicing per object would invalidate every later object's byte
/// offsets, so a multi-object move built on repeated `plan_move` calls would
/// rewrite the wrong operands from the second object onward.
///
/// `du`/`dv` are already in the object's **user space** — the caller does the
/// CTM inverse, because each object has its own CTM and the conversion is
/// therefore per-object, not per-gesture.
///
/// # Errors
///
/// [`VectorEditError::MalformedOperand`] when a construction operator carries
/// an operand count Table 59 does not allow, which would tear the object.
fn move_edits(
    content: &ContentStream,
    obj: &PathObject,
    du: f64,
    dv: f64,
) -> Result<Vec<(usize, usize, Vec<u8>)>, VectorEditError> {
    let mut edits: Vec<(usize, usize, Vec<u8>)> = Vec::new();
    for item in ops_in_range(content, obj.tokens.start, obj.tokens.end) {
        let Some(keyword) = item.keyword(&content.buf) else {
            continue; // an inline image inside a path run: not a construction op
        };
        let nums = item.nums();
        // Rewrite only path-construction operators; everything else
        // (painting, `h`, state ops that slipped into the range) is left
        // byte-verbatim.
        let rewritten = match keyword {
            b"m" | b"l" => translate_points(&nums, &[true], du, dv),
            b"c" => translate_points(&nums, &[true, true, true], du, dv),
            b"v" | b"y" => translate_points(&nums, &[true, true], du, dv),
            // `re x y w h`: only the origin (x, y) moves; (w, h) is a size.
            b"re" => translate_rect(&nums, du, dv),
            // `h` and any painting operator: unchanged.
            _ => continue,
        };
        let Some(new_nums) = rewritten else {
            // A construction operator with the wrong operand arity: moving
            // the object as a whole would tear it. Refuse by name.
            return Err(VectorEditError::MalformedOperand);
        };
        edits.push((
            item.byte_start(),
            item.byte_end(),
            emit_op(&new_nums, keyword),
        ));
    }
    Ok(edits)
}

/// Plan a move of **several path objects by the same page-space delta** — one
/// splice, one rewritten content stream, one undoable command (Pass 47.0,
/// R168).
///
/// # The per-object CTM is why this is not one delta applied N times
///
/// The gesture supplies **one page-space** displacement, but each object's
/// construction operands live in **its own user space**. Two objects under
/// different `cm` transforms need different operand deltas to move the same
/// visible distance, so the CTM inverse is taken per object
/// ([`plan_move`] does the same for one). An implementation that converted
/// once and reused the result would move objects by visibly different amounts
/// on any page whose producer nested transforms — which is most CAD output.
///
/// # Every object must be a path, and the refusal is total
///
/// Operand translation is path-only (text and image objects need `Tm`/`cm`
/// surgery, a different operator family — see
/// [`VectorEditError::NotAPath`]). If any selected object is not a path the
/// whole move refuses: moving the paths and silently leaving the text behind
/// is precisely the partial application R168 forbids, and it would look like
/// a rendering bug rather than a refusal.
///
/// # Errors
///
/// [`VectorEditError::DegenerateCtm`], [`VectorEditError::MalformedOperand`]
/// and [`VectorEditError::OverlappingObjectSpans`] as for the single-object
/// planners. An empty `objs` plans an unchanged buffer.
pub fn plan_move_many(
    content: &ContentStream,
    objs: &[&PathObject],
    dx_page: f64,
    dy_page: f64,
) -> Result<PlannedEdit, VectorEditError> {
    let mut edits: Vec<(usize, usize, Vec<u8>)> = Vec::new();
    let mut disclosures: Vec<String> = Vec::new();
    for obj in objs {
        // Per-object CTM inverse — see the doc comment above for why this
        // cannot be hoisted out of the loop.
        let inv = obj.ctm.inverse().ok_or(VectorEditError::DegenerateCtm)?;
        let d = inv.map_vector(Point::new(dx_page, dy_page));
        edits.extend(move_edits(content, obj, d.x, d.y)?);
        for note in clip_disclosure(content, obj) {
            if !disclosures.contains(&note) {
                disclosures.push(note);
            }
        }
    }
    // Two objects cannot rewrite the same operator: each edit is keyed on an
    // operator's own byte range, and an operator belongs to exactly one
    // object's token run. A shared offset would mean the decomposition
    // assigned one operator to two objects, which is the same torn-model
    // condition `plan_delete_many` refuses — so it is checked, not assumed.
    edits.sort_by_key(|e| e.0);
    // A running scan rather than `windows(2)` + indexing: this crate DENIES
    // `clippy::indexing_slicing` (lib.rs's panic-free policy — pdfcer-core
    // parses untrusted input, so a reachable panic is a denial-of-service
    // bug), and `w[0]`/`w[1]` are exactly the pattern it forbids even though
    // `windows(2)` makes them provably in-bounds.
    let mut prev_end = 0usize;
    for (start, end, _) in &edits {
        if *start < prev_end {
            return Err(VectorEditError::OverlappingObjectSpans {
                start: *start,
                end: *end,
            });
        }
        prev_end = *end;
    }
    let touched = edits.len();
    Ok(PlannedEdit {
        content: splice(&content.buf, &mut edits),
        operators_touched: touched,
        disclosures,
    })
}

/// Plan a **delete**: remove `obj`'s construction **and** painting
/// operators from the content stream.
///
/// The object's byte span ([`VectorObject::bytes`]) covers exactly its
/// first construction operator through its painting operator (or the
/// `BT`→`ET` / `Do` run for a text/image object), captured by Pass 9a; the
/// span is spliced out and every other byte stays verbatim. Preceding
/// graphics-state operators (colour, line width, `q`/`cm`) are **not**
/// removed — they set state the object happened to use but that the
/// operator did not select for deletion (decision 011 §2.5: "remove the
/// object's construction + painting operators"), and a leftover `q…Q`
/// around nothing is inert. This deletes **any** object kind (path, text,
/// image), since it is a pure byte-span removal.
///
/// # Errors
///
/// Never — a delete is a total operation over a valid Pass 9a byte span.
/// (Returns `Result` for signature symmetry with the other two planners and
/// to stay forward-compatible if a future kind grows a refusal.)
#[allow(clippy::unnecessary_wraps)]
pub fn plan_delete(
    content: &ContentStream,
    obj: &VectorObject,
) -> Result<PlannedEdit, VectorEditError> {
    let span = obj.bytes();
    // Remove [start, end): splice with an empty replacement. `splice`
    // copies the gap before, inserts nothing, and resumes after the span,
    // leaving the surrounding whitespace/operators intact and separated.
    let mut edits = vec![(span.start, span.end(), Vec::new())];
    Ok(PlannedEdit {
        content: splice(&content.buf, &mut edits),
        operators_touched: 1,
        disclosures: Vec::new(),
    })
}

/// Plan a delete of **several objects at once** — one splice, one rewritten
/// content stream, and therefore one undoable command.
///
/// # Why this exists rather than calling [`plan_delete`] in a loop
///
/// Two reasons, and the second is the load-bearing one.
///
/// **Indices go stale.** Each object's byte span is an offset into the
/// content buffer this call was handed. Splicing one object out shifts every
/// later span, so a second `plan_delete` against the same indices would cut
/// the wrong bytes. A caller looping would have to re-decompose between every
/// deletion — N decompositions of a CAD page that measured 129,515 paths.
///
/// **N commands is N undos.** The project's standing shape is *one gesture,
/// one undo entry* (`move_nodes` refuses an N-call loop for exactly this
/// reason). An operator who marquee-selects six strays and presses Delete
/// once must get them back with Ctrl+Z once, not six times — and a half-undone
/// deletion is a document state they never chose.
///
/// # Overlap, and why containment is DROPPED while a partial overlap REFUSES
///
/// Spans are sorted and de-duplicated before splicing. A span **fully
/// contained** in an earlier one is dropped and counted: deleting a container
/// removes its contents anyway, so the request is already satisfied and there
/// is nothing to tell the operator. A **partial** overlap — two spans that
/// straddle each other — cannot happen in a well-formed decomposition, where
/// objects are laid out in paint order and nest or sit apart. If one appears,
/// it means the model and the buffer disagree, and splicing would emit
/// plausible-looking garbage: half of one operator's operands followed by the
/// tail of another's. That refuses by name
/// ([`VectorEditError::OverlappingObjectSpans`]) rather than producing a
/// content stream nobody can diff.
///
/// # Errors
///
/// [`VectorEditError::OverlappingObjectSpans`] for the partial-overlap case
/// above. An empty `objs` is **not** an error — it plans an unchanged buffer,
/// so a caller need not special-case the empty selection.
pub fn plan_delete_many(
    content: &ContentStream,
    objs: &[&VectorObject],
) -> Result<PlannedEdit, VectorEditError> {
    let mut spans: Vec<(usize, usize)> = objs
        .iter()
        .map(|o| {
            let s = o.bytes();
            (s.start, s.end())
        })
        .collect();
    // Sort by start, then by DESCENDING end, so that when two spans share a
    // start the container is seen first and the contained one is recognised
    // as redundant rather than the other way round.
    spans.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));

    let mut kept: Vec<(usize, usize)> = Vec::with_capacity(spans.len());
    for (start, end) in spans {
        match kept.last() {
            // Fully contained in the span already accepted: redundant.
            Some(&(_, prev_end)) if end <= prev_end => continue,
            // Starts inside the previous span but runs past its end — the
            // straddle that cannot occur in a well-formed decomposition.
            Some(&(_, prev_end)) if start < prev_end => {
                return Err(VectorEditError::OverlappingObjectSpans { start, end });
            }
            _ => kept.push((start, end)),
        }
    }

    let touched = kept.len();
    let mut edits: Vec<(usize, usize, Vec<u8>)> =
        kept.into_iter().map(|(s, e)| (s, e, Vec::new())).collect();
    Ok(PlannedEdit {
        content: splice(&content.buf, &mut edits),
        operators_touched: touched,
        disclosures: Vec::new(),
    })
}

/// How a heterogeneous selection is treated by [`plan_transform_many`]
/// (`Pass 113.0`).
///
/// Both behaviours ship and the default is picked from what an ordinary
/// operator would expect, per standing rule **R206** — the operator ruled on
/// this by name: *"make things work both ways as options. default it to your
/// best guess as to what would be normally expected."*
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum MixedSelection {
    /// **Default.** Transform the whole selection, whatever kinds it holds —
    /// one command, one undo entry.
    ///
    /// That is what a marquee-then-drag gesture *is*. Refusing on kind would
    /// reopen the `NotAPath` complaint this verb exists to close: the
    /// requesting shell's words were *"a placed image and a placed text run
    /// are the same shape"*, and under `q…cm…Q` wrapping they genuinely are —
    /// the wrap never looks at an operand, so it is kind-agnostic by
    /// construction rather than by a match arm per kind.
    #[default]
    TransformWhole,
    /// Refuse a selection holding more than one kind, by name, for a shell
    /// that wants single-kind semantics.
    RefuseHeterogeneous,
}

/// What [`plan_transform_many`] does with a **singular** requested transform
/// (`Pass 113.0`). Same `R206` shape as [`MixedSelection`].
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[non_exhaustive]
pub enum SingularPolicy {
    /// **Default.** Refuse by name
    /// ([`VectorEditError::SingularTransform`]) — a singular transform is
    /// irrecoverable, and collapsing an object to a line under a drag is data
    /// loss nobody asked for.
    #[default]
    Refuse,
    /// Clamp the degenerate axis to `min` (keeping its sign where it has one)
    /// and **disclose** that it happened.
    ///
    /// Expressible only for an axis-aligned matrix (`b == 0 && c == 0`), which
    /// is what a resize gesture produces. Anything else is
    /// [`VectorEditError::ClampNotExpressible`].
    Clamp {
        /// The minimum absolute scale factor to substitute for a zero.
        min: f64,
    },
}

/// Per-call options for [`plan_transform_many`] (`Pass 113.0`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[non_exhaustive]
pub struct TransformOptions {
    /// How a multi-kind selection is treated. Default
    /// [`MixedSelection::TransformWhole`].
    pub mixed: MixedSelection,
    /// What a singular requested transform does. Default
    /// [`SingularPolicy::Refuse`].
    pub singular: SingularPolicy,
}

impl TransformOptions {
    /// Set [`Self::mixed`], returning `self` — the out-of-crate constructor,
    /// since this struct is `#[non_exhaustive]`.
    #[must_use]
    pub const fn with_mixed(mut self, mixed: MixedSelection) -> Self {
        self.mixed = mixed;
        self
    }

    /// Set [`Self::singular`], returning `self`.
    #[must_use]
    pub const fn with_singular(mut self, singular: SingularPolicy) -> Self {
        self.singular = singular;
        self
    }
}

/// Plan a **transform**: wrap each selected object's operator run in
/// `q <cm> … Q`, so the whole selection is scaled, rotated, sheared or moved
/// by one page-space matrix (`Pass 113.0`).
///
/// # ★ Why this is not `plan_move_many` with a matrix
///
/// The requesting shell assumed it would be, and it cannot be. `plan_move_many`
/// **rewrites numeric operands in place**, and operand rewriting can express
/// translation and nothing else:
///
/// - **A rotated rectangle has no `re` spelling.** `re` carries an origin and a
///   size (§8.5.2.1); there is no operand arrangement that makes it a
///   parallelogram, so a rotate would have to expand every rectangle to four
///   lines — changing the file's shape to express a gesture that changed
///   nothing about what is drawn.
/// - **`line_width` is a user-space scalar** (§8.4.3.2). Scaling coordinates
///   leaves it behind, so a scaled path would keep its original stroke weight
///   and look wrong in a way no operand carries.
/// - **Text and images have no coordinate operands at all.** A placed image is
///   a unit square under the CTM (§8.9.5) and a text run is positioned by `Tm`.
///   Operand rewriting never touches either, which is precisely why
///   `move_objects` refuses them with `NotAPath`.
///
/// Wrapping in `q…cm…Q` has none of those problems **because it never looks at
/// an operand**. It is therefore kind-agnostic by construction — a path, a
/// text object, an image XObject, a form XObject and an inline image are all
/// just a byte span with a CTM — which is the requesting shell's own argument
/// that *"a placed image and a placed text run are the same shape"*, granted
/// by the mechanism rather than by a match arm per kind.
///
/// # ★★ The matrix that gets emitted is NOT the one that was asked for
///
/// `page_matrix` is in **page space**, because that is the space the operator
/// gestures in. The `cm` operator composes into the CTM *at that point in the
/// stream* (§8.3.4: `CTM′ = M × CTM`), which is the object's **user** space.
/// Emitting `page_matrix` directly would apply it in whatever local space the
/// producer happened to leave in force — correct only when the object's CTM is
/// the identity, and silently wrong at a slant or a scale everywhere else.
///
/// The object's marks land at `p × CTM`; the operator wants them at
/// `p × CTM × M`. Inserting `cm X` puts them at `p × (X × CTM)`. Equating the
/// two gives
///
/// ```text
///     X = CTM × M × CTM⁻¹
/// ```
///
/// which is what is emitted, per object, using **that object's own** captured
/// CTM. A selection spanning two different local spaces therefore gets two
/// different `cm` operands for one gesture, and both land in the same place on
/// the page. An object whose CTM is not invertible has no such `X` and is
/// refused ([`VectorEditError::DegenerateCtm`]) — **refusing the whole call**,
/// per `R168`, rather than transforming the part of the selection that
/// happened to qualify.
///
/// # Balance, and why wrapping cannot tear the stream
///
/// `q` and `Q` are inserted at the object's own span boundaries, which begin
/// at its first defining operator and end after its painting operator. A
/// decomposed object is a complete graphics object, so the inserted pair
/// encloses a balanced region and every byte between them is re-emitted
/// verbatim. Nothing outside the span is touched, and the graphics state is
/// restored exactly — an object drawn after the selection sees the state the
/// producer left it, not the transform.
///
/// # Errors
///
/// [`VectorEditError::SingularTransform`] (default policy),
/// [`VectorEditError::ClampNotExpressible`] (clamp requested but not
/// axis-aligned), [`VectorEditError::HeterogeneousSelection`] (opt-in),
/// [`VectorEditError::DegenerateCtm`], and
/// [`VectorEditError::OverlappingObjectSpans`] for a torn model. An empty
/// selection is **not** an error — it plans an unchanged buffer.
pub fn plan_transform_many(
    content: &ContentStream,
    objs: &[&VectorObject],
    page_matrix: Matrix,
    options: TransformOptions,
) -> Result<PlannedEdit, VectorEditError> {
    if objs.is_empty() {
        return Ok(PlannedEdit {
            content: content.buf.clone(),
            operators_touched: 0,
            disclosures: Vec::new(),
        });
    }

    if matches!(options.mixed, MixedSelection::RefuseHeterogeneous)
        && let Some(&head) = objs.first()
    {
        let first = object_kind(head);
        if let Some(other) = objs.iter().map(|o| object_kind(o)).find(|k| *k != first) {
            return Err(VectorEditError::HeterogeneousSelection {
                first,
                second: other,
            });
        }
    }

    let mut disclosures = Vec::new();
    let matrix = resolve_singular(page_matrix, options.singular, &mut disclosures)?;

    // Collect (span, ctm) and apply the same containment/overlap discipline
    // `plan_delete_many` uses — one shared reading of what a decomposition can
    // and cannot produce, so the two verbs cannot disagree about a torn model.
    let mut spans: Vec<(usize, usize, Matrix)> = objs
        .iter()
        .map(|o| {
            let s = o.bytes();
            (s.start, s.end(), object_ctm(o))
        })
        .collect();
    spans.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));

    let mut kept: Vec<(usize, usize, Matrix)> = Vec::with_capacity(spans.len());
    for (start, end, ctm) in spans {
        match kept.last() {
            // Fully contained in a span already accepted: wrapping it again
            // would apply the transform TWICE to the same marks.
            Some(&(_, prev_end, _)) if end <= prev_end => continue,
            Some(&(_, prev_end, _)) if start < prev_end => {
                return Err(VectorEditError::OverlappingObjectSpans { start, end });
            }
            _ => kept.push((start, end, ctm)),
        }
    }

    let touched = kept.len();
    let mut edits: Vec<(usize, usize, Vec<u8>)> = Vec::with_capacity(kept.len() * 2);
    for (start, end, ctm) in kept {
        let local = local_matrix(ctm, matrix)?;
        // Zero-length insertions at the span boundaries: `splice` copies the
        // gap between edits verbatim, so the object's own bytes pass through
        // untouched and only the wrapper is new.
        edits.push((start, start, emit_q_cm(local)));
        edits.push((end, end, b" Q".to_vec()));
    }

    Ok(PlannedEdit {
        content: splice(&content.buf, &mut edits),
        operators_touched: touched,
        disclosures,
    })
}

/// The `cm` matrix to emit for an object whose captured CTM is `ctm`, so that
/// `page_matrix` takes effect in **page** space. See
/// [`plan_transform_many`]'s "the matrix that gets emitted is not the one that
/// was asked for".
fn local_matrix(ctm: Matrix, page_matrix: Matrix) -> Result<Matrix, VectorEditError> {
    let inverse = ctm.inverse().ok_or(VectorEditError::DegenerateCtm)?;
    Ok(ctm.post_concat(page_matrix).post_concat(inverse))
}

/// Apply [`SingularPolicy`] to the requested matrix, returning the matrix to
/// use and pushing any disclosure the clamp owes.
fn resolve_singular(
    m: Matrix,
    policy: SingularPolicy,
    disclosures: &mut Vec<String>,
) -> Result<Matrix, VectorEditError> {
    if m.is_invertible() {
        return Ok(m);
    }
    match policy {
        SingularPolicy::Refuse => Err(VectorEditError::SingularTransform),
        SingularPolicy::Clamp { min } => {
            // Axis-aligned only: with a shear present the degeneracy is a
            // DIRECTION, not an axis, and there is no single factor to clamp.
            if m.b != 0.0 || m.c != 0.0 || !min.is_finite() || min <= 0.0 {
                return Err(VectorEditError::ClampNotExpressible);
            }
            // Sign-preserving: a scale that arrived as -0.0 was heading
            // negative, and clamping it to +min would flip the object as well
            // as rescue it.
            let clamp = |v: f64| {
                if v == 0.0 {
                    if v.is_sign_negative() { -min } else { min }
                } else {
                    v
                }
            };
            let clamped = Matrix {
                a: clamp(m.a),
                d: clamp(m.d),
                ..m
            };
            if !clamped.is_invertible() {
                return Err(VectorEditError::ClampNotExpressible);
            }
            disclosures.push(format!(
                "transform: the requested scale collapsed the selection to zero area, so it was CLAMPED to a minimum of {min} rather than applied -- a singular transform has no inverse and could not have been undone by dragging back."
            ));
            Ok(clamped)
        }
    }
}

/// Plan a **recolour** of `objs`: wrap each object's own bytes in `q … Q` with
/// the requested colour operators in front (`Pass 219.0`).
///
/// # Why a wrap and not an operand rewrite
///
/// Colour is GRAPHICS STATE, not a property of a path. The operators that set
/// it — `rg`, `k`, `scn` … — sit *before* the object's construction and are
/// routinely SHARED: one `0 0 1 RG` commonly governs every stroke on a CAD
/// sheet. Rewriting the operand an object happens to inherit would recolour
/// every other object that inherits the same one, silently, and the operator
/// would have selected one line and changed a thousand.
///
/// So nothing existing is rewritten. Each object's span is wrapped, exactly as
/// [`plan_transform_many`] wraps a `cm`, and the `Q` confines the change to
/// the object the operator picked. That also makes the edit trivially
/// invertible and keeps every other byte on the page verbatim (the
/// minimal-diff invariant, `ARCHITECTURE.md` §5).
///
/// ★ The wrap is safe against the object's own bytes overriding it because a
/// `PathObject`'s span begins at its first CONSTRUCTION operator — colour
/// operators are outside it by construction. If that ever changes, this
/// becomes a silent no-op, which is why the caller counts what it touched.
///
/// # This planner does no policy
///
/// It does not decide whether an object MAY be recoloured — refusing a spot
/// ink is the session's job, because the session is what holds the
/// [`crate::vector::PathPaint`] and what must report the refusal by name. A
/// planner that silently skipped objects would make "nine of twelve changed"
/// unreportable.
pub fn plan_recolour(
    content: &ContentStream,
    objs: &[&VectorObject],
    fill: Option<Rgb>,
    stroke: Option<Rgb>,
) -> Result<PlannedEdit, VectorEditError> {
    if objs.is_empty() || (fill.is_none() && stroke.is_none()) {
        return Ok(PlannedEdit {
            content: content.buf.clone(),
            operators_touched: 0,
            disclosures: Vec::new(),
        });
    }

    let mut prefix = b"q ".to_vec();
    if let Some(c) = fill {
        emit_rgb_op(&mut prefix, c, false);
    }
    if let Some(c) = stroke {
        emit_rgb_op(&mut prefix, c, true);
    }

    let mut spans: Vec<(usize, usize)> = objs
        .iter()
        .map(|o| {
            let s = o.bytes();
            (s.start, s.end())
        })
        .collect();
    spans.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));

    let mut kept: Vec<(usize, usize)> = Vec::with_capacity(spans.len());
    for (start, end) in spans {
        match kept.last() {
            Some(&(_, prev_end)) if end <= prev_end => continue,
            Some(&(_, prev_end)) if start < prev_end => {
                return Err(VectorEditError::OverlappingObjectSpans { start, end });
            }
            _ => kept.push((start, end)),
        }
    }

    let touched = kept.len();
    let mut edits: Vec<(usize, usize, Vec<u8>)> = kept
        .into_iter()
        .map(|(s, e)| {
            let mut body = prefix.clone();
            // `get` rather than a slice index: the spans come from the
            // decomposition of THIS buffer so they are in range, but a panic
            // on untrusted input is never the right failure mode
            // (`ARCHITECTURE.md` §10). An out-of-range span degrades to
            // wrapping nothing, which the caller's `operators_touched` count
            // still reports.
            if let Some(original) = content.buf.get(s..e) {
                body.extend_from_slice(original);
            }
            body.extend_from_slice(b" Q");
            (s, e, body)
        })
        .collect();

    Ok(PlannedEdit {
        content: splice(&content.buf, &mut edits),
        operators_touched: touched,
        disclosures: Vec::new(),
    })
}

/// `r g b rg ` (or `RG ` when stroking), with the round-trip-safe emitter every
/// other planner uses.
fn emit_rgb_op(out: &mut Vec<u8>, c: Rgb, stroking: bool) {
    for v in [c.r, c.g, c.b] {
        emit_number(out, f64::from(v));
        out.push(b' ');
    }
    out.extend_from_slice(if stroking { b"RG " } else { b"rg " });
}

/// `q a b c d e f cm ` as bytes, with `emit_number`'s round-trip-safe
/// formatting — the same emitter every other planner uses, so a transformed
/// object's operands are spelled exactly as a moved one's are.
fn emit_q_cm(m: Matrix) -> Vec<u8> {
    let mut out = b"q ".to_vec();
    for v in [m.a, m.b, m.c, m.d, m.e, m.f] {
        emit_number(&mut out, v);
        out.push(b' ');
    }
    out.extend_from_slice(b"cm ");
    out
}

/// A short kind label for a diagnostic — the same vocabulary
/// [`VectorEditError::NotAPath`] uses.
const fn object_kind(o: &VectorObject) -> &'static str {
    match o {
        VectorObject::Path(_) => "path",
        VectorObject::Text(_) => "text",
        VectorObject::Image(_) => "image",
    }
}

/// The object's captured CTM, whatever its kind.
const fn object_ctm(o: &VectorObject) -> Matrix {
    match o {
        VectorObject::Path(p) => p.ctm,
        VectorObject::Text(t) => t.ctm,
        VectorObject::Image(i) => i.ctm,
    }
}

/// Plan a **subpath delete**: remove ONE subpath's construction operators
/// from a path object, leaving the object's other subpaths byte-verbatim.
///
/// # Why this operation exists at all
///
/// A CAD producer routinely emits an entire drawing view as one path object.
/// Measured on a real SolidWorks export: one stroked path with **1194
/// subpaths** covering a 550×500 pt isometric view. [`plan_delete`] can only
/// remove the whole view. This removes one line of it — which is what an
/// operator asking to "delete this line" almost always means on such a file.
///
/// # The index, and the guard that makes it safe
///
/// `subpath_index` is into `obj.subpaths` — the SAME ordering
/// [`super::hit_test_subpaths`] returns and the GUI selects with. That
/// agreement is not assumed: this re-derives the subpaths from the operator
/// bytes and **refuses if the two counts disagree**
/// ([`VectorEditError::SubpathStructureMismatch`]). A silent disagreement
/// would delete a different line from the one the operator picked, which is
/// the single worst outcome available here and is not detectable afterwards
/// by looking at the file.
///
/// Only subpaths begun by an explicit `m` or `re` are counted. A subpath that
/// PDF starts implicitly — a segment operator after `h`, which reopens at the
/// closed subpath's start point (§8.5.2.1) — has no operator of its own to
/// remove cleanly, so its presence trips the count guard and the whole edit is
/// refused rather than approximated. Note the asymmetry with MOVING such a
/// subpath (and with node-dragging its start), both of which now succeed by
/// materializing the `m` the file omitted: an insertion can supply a missing
/// coordinate, but a DELETION has nowhere to put one — removing the operators
/// before an implicit start changes where it begins, and there is no operand
/// to pin it with because the subpath being deleted is the one that would
/// have carried it.
///
/// # Clipping paths are refused
///
/// If the object's operators include `W` or `W*`, its subpaths define a
/// **clipping region** (§8.5.4), not marks on the page. Deleting one would
/// change which OTHER content is visible — an edit whose visible effect is
/// somewhere the operator was not looking, and the definition of sneaky (rule
/// 4). Refused by name.
///
/// # Deleting the last subpath deletes the object
///
/// A path object with no construction operators left is not a smaller object;
/// it is a painting operator with no path, which is meaningless. So when
/// `obj` has exactly one subpath this removes the object's whole byte span —
/// identical to [`plan_delete`]. Callers that need to distinguish the two
/// outcomes should check `obj.subpaths.len() == 1` before calling.
///
/// # Errors
///
/// [`VectorEditError::SubpathOutOfRange`], [`VectorEditError::ClippingPath`],
/// or [`VectorEditError::SubpathStructureMismatch`]. Each is raised before any
/// content byte is produced, so a refusal leaves the document untouched.
///
/// # Examples
///
/// ```
/// use pdfcer_core::content::ContentStream;
/// use pdfcer_core::vector::{decompose, NoXObjects, Matrix, VectorObject};
/// use pdfcer_core::vector::edit::plan_delete_subpath;
///
/// // Three separate lines painted by ONE `S` — one object, three subpaths.
/// let cs = ContentStream::parse(b"0 0 m 10 0 l 0 5 m 10 5 l 0 9 m 10 9 l S".to_vec()).unwrap();
/// let model = decompose(&cs, Matrix::IDENTITY, &NoXObjects);
/// let VectorObject::Path(path) = &model.objects[0] else { unreachable!() };
/// assert_eq!(path.subpaths.len(), 3);
///
/// // Remove the middle one; the other two keep their exact bytes.
/// let plan = plan_delete_subpath(&cs, path, 1).unwrap();
/// assert_eq!(plan.content, b"0 0 m 10 0 l 0 9 m 10 9 l S");
/// ```
pub fn plan_delete_subpath(
    content: &ContentStream,
    obj: &PathObject,
    subpath_index: usize,
) -> Result<PlannedEdit, VectorEditError> {
    if is_clipping_path(content, obj.tokens.start, obj.tokens.end) {
        return Err(VectorEditError::ClippingPath);
    }

    let declared = obj.subpaths.len();
    if subpath_index >= declared {
        return Err(VectorEditError::SubpathOutOfRange {
            index: subpath_index,
            count: declared,
        });
    }

    // The last one standing: an empty path is not an object.
    if declared == 1 {
        let span = obj.bytes;
        let mut edits = vec![(span.start, span.end(), Vec::new())];
        return Ok(PlannedEdit {
            content: splice(&content.buf, &mut edits),
            operators_touched: 1,
            disclosures: Vec::new(),
        });
    }

    // The subpath's OWN recorded token range (Pass 28.0), converted to bytes.
    //
    // This replaces a second walk over the operators plus a count guard that
    // refused the whole object whenever the two walks disagreed. The range is
    // now recorded by the decomposition that produced the subpath, so the
    // index and the bytes cannot describe different things — the agreement is
    // structural rather than checked.
    let subpath = obj
        .subpaths
        .get(subpath_index)
        .ok_or(VectorEditError::SubpathOutOfRange {
            index: subpath_index,
            count: declared,
        })?;

    // The precise form of decision 026's `DeleteWouldMoveNextSubpath`. A
    // subpath started implicitly by a segment after `h` inherits its start
    // point from whatever precedes it, carried by no operand — so excising the
    // subpath BEFORE it silently moves a line the operator never touched, in a
    // byte-minimal edit that passes every round-trip check. Only that one
    // deletion is refused now; previously any `h`-reopen anywhere in the object
    // made every subpath in it undeletable.
    if obj
        .subpaths
        .get(subpath_index + 1)
        .is_some_and(|next| next.starts_implicitly)
    {
        return Err(VectorEditError::DeleteWouldMoveNextSubpath {
            index: subpath_index,
        });
    }

    let site = span_of_tokens(content, subpath.tokens).ok_or(
        VectorEditError::SubpathStructureMismatch {
            from_operators: 0,
            from_decomposition: declared,
        },
    )?;

    // Swallow the whitespace that FOLLOWED the removed operators, so the
    // separator before them becomes the separator between their neighbours.
    // Without this every delete leaves a widening gap behind — cosmetically
    // untidy on one edit, and on a 1194-subpath drawing an operator could
    // remove hundreds of lines and leave hundreds of orphaned runs of spaces.
    //
    // Trailing rather than leading, and bounded by the object's own span, so
    // this can never reach across into a neighbouring object's bytes or run
    // two tokens together.
    let end = extend_over_whitespace(&content.buf, site.1, obj.bytes.end());
    let mut edits = vec![(site.0, end, Vec::new())];
    Ok(PlannedEdit {
        content: splice(&content.buf, &mut edits),
        operators_touched: 1,
        disclosures: Vec::new(),
    })
}

/// Plan the deletion of **one show operator** out of a text object
/// (`Pass 32.0`, ISO 32000-1 §9.4).
///
/// # The defect this closes, measured on the operator's own drawing
///
/// Deletion has been object-granular since Pass 9c-min, and the operator's
/// mental unit is the **run**. A SolidWorks export puts *every* label on a
/// sheet inside **one** `BT`…`ET`, so on `cad-drawing-a.pdf` deleting "a
/// dimension label" removes **all 237 of them at once**.
///
/// This is the text-side twin of [`plan_delete_subpath`] (Pass 25.2), and
/// deliberately the same shape: the hit-test half has been per-run since
/// Pass 18.5, so a run could already be *selected* and could not be
/// *removed*.
///
/// # What it does
///
/// Removes exactly that run's operator bytes — the span
/// [`TextRun::bytes`] recorded at decomposition, so the index and the bytes
/// cannot describe different things — and re-emits everything else
/// verbatim. Trailing whitespace goes with it, bounded by the text
/// object's own span, so removing 237 labels does not leave 237 orphaned
/// runs of spaces.
///
/// **Deleting the only run deletes the text object**, exactly as deleting
/// the only subpath deletes the path object: a `BT`…`ET` that shows
/// nothing is not an object.
///
/// # The refusal that earns its code
///
/// [`VectorEditError::DeleteWouldMoveNextRun`] when the **following** run's
/// position is [`RunPositioning::Inherited`] — §9.4.2 leaves the pen
/// advanced past the string just drawn, so that run starts wherever this
/// one ends and has no coordinates of its own. Excising this run slides it.
/// The edit would be byte-minimal, would round-trip, would pass
/// `--verify-undo`, and would be wrong — which is precisely the class
/// decision 027 says to refuse rather than guess at.
///
/// The message names the remedy, because there is one and it always works:
/// **delete the later run first.** A run that inherits from a run that is
/// already gone is not a case this can produce.
///
/// # Errors
///
/// [`VectorEditError::TextRunOutOfRange`],
/// [`VectorEditError::DeleteWouldMoveNextRun`]. Both are raised before any
/// content byte is produced.
///
/// # Examples
///
/// ```
/// use pdfcer_core::content::ContentStream;
/// use pdfcer_core::vector::{decompose, NoXObjects, Matrix, VectorObject};
/// use pdfcer_core::vector::edit::plan_delete_text_run;
///
/// // Two runs, each placed by its own `Tm`, so neither inherits.
/// let src = b"BT /F1 10 Tf 1 0 0 1 10 700 Tm (A) Tj 1 0 0 1 10 680 Tm (B) Tj ET".to_vec();
/// let cs = ContentStream::parse(src).unwrap();
/// let model = decompose(&cs, Matrix::IDENTITY, &NoXObjects);
/// # if let Some(VectorObject::Text(t)) = model.objects.first() {
/// # if t.runs.len() == 2 {
/// let VectorObject::Text(text) = &model.objects[0] else { unreachable!() };
/// let plan = plan_delete_text_run(&cs, text, 0).unwrap();
/// assert!(!String::from_utf8_lossy(&plan.content).contains("(A)"));
/// assert!(String::from_utf8_lossy(&plan.content).contains("(B)"));
/// # }}
/// ```
pub fn plan_delete_text_run(
    content: &ContentStream,
    obj: &TextObject,
    run_index: usize,
) -> Result<PlannedEdit, VectorEditError> {
    let count = obj.runs.len();
    let Some(run) = obj.runs.get(run_index) else {
        return Err(VectorEditError::TextRunOutOfRange {
            index: run_index,
            count,
        });
    };

    // The last one standing: a text object that shows nothing is not an
    // object. Same rule, same reason, as `plan_delete_subpath`'s.
    if count == 1 {
        let span = obj.bytes;
        let mut edits = vec![(span.start, span.end(), Vec::new())];
        return Ok(PlannedEdit {
            content: splice(&content.buf, &mut edits),
            operators_touched: 1,
            disclosures: Vec::new(),
        });
    }

    // The §9.4.2 guard — see `DeleteWouldMoveNextRun`. Checked against the
    // NEXT run only: a run that inherits from something further back is not
    // a shape the model can produce, since inheritance is always from the
    // immediately preceding show operator.
    if obj
        .runs
        .get(run_index + 1)
        .is_some_and(|next| next.positioned_by == RunPositioning::Inherited)
    {
        return Err(VectorEditError::DeleteWouldMoveNextRun { index: run_index });
    }

    let end = extend_over_whitespace(&content.buf, run.bytes.end(), obj.bytes.end());
    let mut edits = vec![(run.bytes.start, end, Vec::new())];
    Ok(PlannedEdit {
        content: splice(&content.buf, &mut edits),
        operators_touched: 1,
        disclosures: Vec::new(),
    })
}

/// Advance `end` past any PDF white-space characters (§7.2.2 Table 1), never
/// beyond `limit` and never beyond the buffer.
fn extend_over_whitespace(buf: &[u8], mut end: usize, limit: usize) -> usize {
    let limit = limit.min(buf.len());
    while end < limit
        && matches!(
            buf.get(end),
            Some(b' ' | b'\t' | b'\r' | b'\n' | b'\x00' | b'\x0c')
        )
    {
        end += 1;
    }
    end
}

/// The byte range covered by a token range, as `(start, end)`.
///
/// `None` when the range names no operator — a subpath the decomposition
/// recorded but whose tokens fall outside this stream, which should be
/// impossible and is refused rather than assumed.
fn span_of_tokens(
    content: &ContentStream,
    tokens: super::decompose::TokenRange,
) -> Option<(usize, usize)> {
    let items = ops_in_range(content, tokens.start, tokens.end.saturating_add(1));
    let first = items.first()?;
    let last = items.last()?;
    Some((first.byte_start(), last.byte_end()))
}

/// Whether the object's operators establish a clipping region (`W` / `W*`,
/// §8.5.4).
///
/// Checked from the OPERATORS rather than from [`PaintStyle`]: `is_invisible`
/// is true for a bare `n` as well as for `W n`, and a bare `n` path clips
/// nothing. Refusing both would be over-broad, and over-broad refusals teach
/// an operator that the tool says no for no reason.
fn is_clipping_path(content: &ContentStream, start: usize, end: usize) -> bool {
    ops_in_range(content, start, end).iter().any(|item| {
        matches!(
            item.keyword(&content.buf),
            Some(b"W") | Some(b"W*") // ui-text-exempt: PDF operator keywords, §8.5.4
        )
    })
}

/// The disclosure a *move* of a clipping path owes the operator, if the object
/// is one — otherwise nothing.
///
/// # Why this discloses where subpath-DELETE refuses
///
/// Both edits change what OTHER content is visible rather than changing a mark
/// the operator can see, which is the condition rule 4 exists for. They differ
/// in whether a legitimate intent exists. Deleting one subpath of a clip has
/// none worth guessing at — it changes the region's topology, and the operator
/// asking to "delete this line" cannot have meant "reveal whatever is under
/// that part of the page." Moving one does: resizing a crop region is a real
/// task, and refusing it would leave clip geometry permanently uneditable.
///
/// So: refuse the one with no good reading, disclose the one that has one.
///
/// # Why this was easy to miss
///
/// Until Pass 30.0 a clip rectangle's corners were unreachable — clips are
/// overwhelmingly `re` rectangles (§8.5.4's canonical `re W n` idiom), and
/// `re` corners were refused as un-draggable. Making them draggable removed
/// that accidental cover, so the gap had to be closed in the same change.
/// Found by running the new node drag against a real file rather than a
/// fixture: the first closed 4-anchor object on its first page was a
/// full-page clip.
fn clip_disclosure(content: &ContentStream, obj: &PathObject) -> Vec<String> {
    if is_clipping_path(content, obj.tokens.start, obj.tokens.end) {
        vec![
            "This shape is a clipping region: it draws nothing itself, it controls \
             which OTHER content on the page is visible. Moving it changes what shows \
             through elsewhere on the page, not here."
                .to_owned(),
        ]
    } else {
        Vec::new()
    }
}

/// Plan a **subpath move**: translate ONE subpath's construction operands by a
/// page-space `(dx, dy)`, leaving the object's other subpaths byte-verbatim
/// (Pass 28.0).
///
/// # Why this could not be written before
///
/// `Subpath` carried no byte range. `plan_delete_subpath` worked around that by
/// re-walking the operators and refusing whenever its walk disagreed with the
/// geometry about how many subpaths there were — enough to EXCISE a span, but
/// not enough to rewrite operands inside one, because a move has to know which
/// operator each coordinate pair belongs to. Recording the token range on the
/// decomposition walk that already knew it is what makes this expressible.
///
/// # What is refused, and why each
///
/// - A subpath that **starts implicitly** (a segment after `h`, §8.5.2.1): its
///   start point is inherited and carried by no operand, so translating the
///   operands that ARE written would move the rest of the subpath away from a
///   start that stayed put — tearing it. Since Pass 30.0 this is HANDLED, not
///   refused: an explicit `m` at the moved start is inserted ahead of the
///   segment that inherited it, and the translation is then uniform. Disclosed
///   via [`PlannedEdit::disclosures`].
/// - A **malformed operand run**, for the same reason `plan_move` refuses one:
///   a partially-moved subpath is worse than an unmoved one.
/// - A **singular CTM**, which has no unambiguous user-space pre-image.
///
/// # Errors
///
/// [`VectorEditError::SubpathOutOfRange`],
/// [`VectorEditError::MalformedOperand`], [`VectorEditError::DegenerateCtm`].
/// Every refusal happens before any byte is produced.
///
/// # Examples
///
/// ```
/// use pdfcer_core::content::ContentStream;
/// use pdfcer_core::vector::{decompose, NoXObjects, Matrix, VectorObject};
/// use pdfcer_core::vector::edit::plan_move_subpath;
///
/// let cs = ContentStream::parse(b"0 0 m 10 0 l 0 5 m 10 5 l S".to_vec()).unwrap();
/// let model = decompose(&cs, Matrix::IDENTITY, &NoXObjects);
/// let VectorObject::Path(path) = &model.objects[0] else { unreachable!() };
/// // Move only the SECOND line up by 20.
/// let plan = plan_move_subpath(&cs, path, 1, 0.0, 20.0).unwrap();
/// assert_eq!(plan.content, b"0 0 m 10 0 l 0 25 m 10 25 l S");
/// ```
pub fn plan_move_subpath(
    content: &ContentStream,
    obj: &PathObject,
    subpath_index: usize,
    dx: f64,
    dy: f64,
) -> Result<PlannedEdit, VectorEditError> {
    let count = obj.subpaths.len();
    let subpath = obj
        .subpaths
        .get(subpath_index)
        .ok_or(VectorEditError::SubpathOutOfRange {
            index: subpath_index,
            count,
        })?;
    // Page-space delta to user-space delta: the LINEAR inverse, translation
    // excluded — the same conversion `plan_move` makes, for the same reason.
    let inv = obj.ctm.inverse().ok_or(VectorEditError::DegenerateCtm)?;
    let d = inv.map_vector(Point::new(dx, dy));

    let mut edits: Vec<(usize, usize, Vec<u8>)> = Vec::new();
    let mut disclosures: Vec<String> = clip_disclosure(content, obj);
    let mut touched = 0usize;

    // An implicitly-started subpath (§8.5.2.1: a segment after `h` with no `m`
    // of its own) INHERITS its start from the closed subpath before it. Its
    // segment operands can be translated like any other, but its start point
    // is written nowhere, so translating the operands alone would move every
    // point of the subpath EXCEPT the first — shearing the shape rather than
    // moving it.
    //
    // This used to refuse for that reason. Materializing the `m` the file
    // omitted, at the inherited start plus the delta, removes the cause: after
    // it the subpath's start is its own, and the translation is uniform. The
    // insertion touches nothing before it, because `h` has already terminated
    // the previous subpath.
    // Prepended to the FIRST rewritten operator's bytes rather than pushed as
    // its own zero-width edit at the same offset: `splice` silently skips an
    // edit that starts before its cursor, so two edits sharing a start offset
    // would drop one of the pair, chosen by sort order. Prepending has no such
    // race and produces the identical bytes.
    let mut lead_insert: Option<Vec<u8>> = None;
    if subpath.starts_implicitly {
        // `subpath.start` is in USER space (the decomposer records operands
        // before the CTM), so the user-space delta applies to it directly.
        let moved = Point::new(subpath.start.x + d.x, subpath.start.y + d.y);
        let mut lead = emit_op(&[moved.x, moved.y], b"m");
        lead.push(b' ');
        lead_insert = Some(lead);
        disclosures.push(
            "This shape had no starting point of its own — it re-used the start of the \
             shape before it. A move instruction naming its start has been added so it \
             can be moved independently."
                .to_owned(),
        );
    }
    for item in ops_in_range(content, subpath.tokens.start, subpath.tokens.end + 1) {
        let Some(keyword) = item.keyword(&content.buf) else {
            continue;
        };
        let nums = item.nums();
        // Coordinate arity per Table 59. `h` carries none and needs no edit.
        let pairs = match keyword {
            b"m" | b"l" => 1,
            b"v" | b"y" => 2,
            b"c" => 3,
            b"re" => {
                // Only the ORIGIN moves; width and height are a size, not a
                // position, so translating all four would resize the rectangle.
                let &[x, y, w, h] = nums.as_slice() else {
                    return Err(VectorEditError::MalformedOperand);
                };
                let mut out = Vec::new();
                emit_number(&mut out, x + d.x);
                out.push(b' ');
                emit_number(&mut out, y + d.y);
                out.push(b' ');
                emit_number(&mut out, w);
                out.push(b' ');
                emit_number(&mut out, h);
                out.extend_from_slice(b" re");
                push_edit(&mut edits, &mut lead_insert, &item, out);
                touched += 1;
                continue;
            }
            b"h" => continue,
            _ => continue,
        };
        if nums.len() != pairs * 2 {
            return Err(VectorEditError::MalformedOperand);
        }
        let mut out = Vec::new();
        for (i, chunk) in nums.chunks_exact(2).enumerate() {
            // `chunks_exact(2)` guarantees the pair, but destructuring proves
            // it to the compiler rather than to a reader — the crate forbids
            // indexing that could panic even where the invariant holds.
            let &[cx, cy] = chunk else {
                return Err(VectorEditError::MalformedOperand);
            };
            if i > 0 {
                out.push(b' ');
            }
            emit_number(&mut out, cx + d.x);
            out.push(b' ');
            emit_number(&mut out, cy + d.y);
        }
        out.push(b' ');
        out.extend_from_slice(keyword);
        push_edit(&mut edits, &mut lead_insert, &item, out);
        touched += 1;
    }

    Ok(PlannedEdit {
        content: splice(&content.buf, &mut edits),
        operators_touched: touched,
        disclosures,
    })
}

/// Plan a **node delete**: remove ONE anchor from a path object, leaving every
/// other object and every other subpath byte-verbatim (Pass 36.1).
///
/// `node_index` is object-scoped, in decomposition order — the same numbering
/// [`plan_move_node`] takes, [`anchor_count`](super::anchor_count) reports and
/// `pdfcer node-move --node` addresses. One numbering for a point, whatever
/// is being done to it.
///
/// # What removing an anchor actually means in a content stream
///
/// A path is a sequence of *segment operators*, each of which contributes its
/// endpoint as an anchor: `l x y` contributes one, `c x1 y1 x2 y2 x3 y3`
/// contributes its endpoint `(x3, y3)` and carries two control points that
/// belong to the segment, not to the endpoint. So deleting an anchor is
/// deleting **the operator that produced it** — which also removes the segment
/// arriving at it, and joins its neighbours directly. That is what an operator
/// means by "delete this point": the line should now run from the point before
/// to the point after.
///
/// The subpath's FIRST anchor is the exception, because it is carried by `m`
/// rather than by a segment. Removing the `m` alone would leave the subpath
/// with no start, so the operator that follows is rewritten INTO the new `m`
/// at its own endpoint: `l x y` becomes `m x y`, and `c x1 y1 x2 y2 x3 y3`
/// becomes `m x3 y3`. Both are exact — the segment being discarded is the one
/// that arrived at the deleted point — but the curve case drops two control
/// points with it, so it is disclosed rather than left to be discovered.
///
/// # What is refused, and why each
///
/// - A **clipping path** (`W`/`W*`, §8.5.4), for the identical reason
///   [`plan_delete_subpath`] refuses one: the visible change would be to
///   *other* content showing through, somewhere the operator was not looking.
/// - A subpath that would be left with **fewer than two anchors**
///   ([`VectorEditError::NodeDeleteWouldEmptySubpath`]) — the remainder draws
///   nothing, and quietly promoting the request to a whole-part delete is the
///   exact surprise Pass 36.0 removed one rung up.
/// - An **`re` rectangle corner**
///   ([`VectorEditError::NodeDeleteRectangleCorner`]) — no operand names it,
///   and the honest result would be a triangle.
/// - The **inherited start of an `h`-reopened subpath**
///   ([`VectorEditError::NodeDeleteImplicitStart`]) — the coordinates live in
///   the previous subpath, which the operator did not select.
///
/// Every refusal happens before any byte is produced (rule 4).
///
/// # Errors
///
/// [`VectorEditError::NodeOutOfRange`],
/// [`VectorEditError::NodeDeleteWouldEmptySubpath`],
/// [`VectorEditError::NodeDeleteRectangleCorner`],
/// [`VectorEditError::NodeDeleteImplicitStart`],
/// [`VectorEditError::ClippingPath`], [`VectorEditError::MalformedOperand`].
///
/// # Examples
///
/// ```
/// use pdfcer_core::content::ContentStream;
/// use pdfcer_core::vector::{decompose, NoXObjects, Matrix, VectorObject};
/// use pdfcer_core::vector::edit::plan_delete_node;
///
/// // A three-point polyline; remove the middle point.
/// let cs = ContentStream::parse(b"0 0 m 10 0 l 20 0 l S".to_vec()).unwrap();
/// let model = decompose(&cs, Matrix::IDENTITY, &NoXObjects);
/// let VectorObject::Path(path) = &model.objects[0] else { unreachable!() };
/// let plan = plan_delete_node(&cs, path, 1).unwrap();
/// assert_eq!(plan.content, b"0 0 m 20 0 l S");
///
/// // Removing the FIRST point promotes the next operator to the new start.
/// let plan = plan_delete_node(&cs, path, 0).unwrap();
/// assert_eq!(plan.content, b"10 0 m 20 0 l S");
/// ```
pub fn plan_delete_node(
    content: &ContentStream,
    obj: &PathObject,
    node_index: usize,
) -> Result<PlannedEdit, VectorEditError> {
    if is_clipping_path(content, obj.tokens.start, obj.tokens.end) {
        return Err(VectorEditError::ClippingPath);
    }

    let anchors = enumerate_anchors(content, obj.tokens.start, obj.tokens.end);
    let count = anchors.len();
    let site = anchors
        .get(node_index)
        .ok_or(VectorEditError::NodeOutOfRange {
            index: node_index,
            count,
        })?;

    // Which subpath holds this anchor, and how many anchors that subpath has.
    // Derived by walking `obj.subpaths` and accumulating, because the geometry
    // and `enumerate_anchors` flatten in the SAME order by construction (the
    // module docs' one-numbering rule) — so the running offset is the
    // object-scoped index of each subpath's first anchor.
    let mut offset = 0usize;
    let mut found: Option<(usize, usize)> = None; // (subpath index, its anchor count)
    for (i, sp) in obj.subpaths.iter().enumerate() {
        let n = sp.anchors().count();
        if node_index < offset + n {
            found = Some((i, n));
            break;
        }
        offset += n;
    }
    let (subpath_index, subpath_anchors) = found.ok_or(VectorEditError::NodeOutOfRange {
        index: node_index,
        count,
    })?;

    if subpath_anchors < 3 {
        return Err(VectorEditError::NodeDeleteWouldEmptySubpath {
            subpath: subpath_index,
            remaining: subpath_anchors.saturating_sub(1),
        });
    }

    match site.kind {
        AnchorKind::Rectangle { .. } => return Err(VectorEditError::NodeDeleteRectangleCorner),
        AnchorKind::Implicit => return Err(VectorEditError::NodeDeleteImplicitStart),
        AnchorKind::Editable => {}
    }

    let mut disclosures = clip_disclosure(content, obj);

    // ---- (a) Not the subpath's start: excise the segment operator. -------
    if !site.is_start {
        let end = extend_over_whitespace(&content.buf, site.byte_end, obj.bytes.end());
        let mut edits = vec![(site.byte_start, end, Vec::new())];
        if is_curve_keyword(&site.keyword) {
            disclosures.push(
                "The curve that ran into this point was removed along with it, so the shape now \
                 goes straight from the point before to the point after."
                    .to_owned(),
            );
        }
        return Ok(PlannedEdit {
            content: splice(&content.buf, &mut edits),
            operators_touched: 1,
            disclosures,
        });
    }

    // ---- (b) The subpath's start: promote the NEXT operator to `m`. ------
    //
    // Guaranteed to exist: the subpath has at least three anchors (checked
    // above), so the anchor after the start is in the same subpath.
    let next = anchors
        .get(node_index + 1)
        .ok_or(VectorEditError::MalformedOperand)?;
    // A rectangle or an implicit start immediately after an `m` would mean the
    // walk disagreed with itself about subpath boundaries; refuse rather than
    // rewrite an operator whose operand layout is not the one assumed below.
    if !matches!(next.kind, AnchorKind::Editable) {
        return Err(VectorEditError::MalformedOperand);
    }
    let x_slot = next.pair_index * 2;
    let (Some(&nx), Some(&ny)) = (next.operands.get(x_slot), next.operands.get(x_slot + 1)) else {
        return Err(VectorEditError::MalformedOperand);
    };

    // Two edits, applied by one `splice`: drop the old `m` (with its trailing
    // whitespace, so no widening gap is left behind — the same reasoning
    // `plan_delete_subpath` documents), and rewrite the follower as the new
    // `m`. They never overlap: `site` ends before `next` begins.
    let m_end = extend_over_whitespace(&content.buf, site.byte_end, next.byte_start);
    let mut edits = vec![
        (site.byte_start, m_end, Vec::new()),
        (next.byte_start, next.byte_end, emit_op(&[nx, ny], b"m")),
    ];
    if is_curve_keyword(&next.keyword) {
        disclosures.push(
            "This was the part's starting point, so the curve that led away from it was removed \
             too and the part now starts at the next point."
                .to_owned(),
        );
    }
    Ok(PlannedEdit {
        content: splice(&content.buf, &mut edits),
        operators_touched: 2,
        disclosures,
    })
}

/// Whether a path-construction keyword draws a Bézier segment (§8.5.2.2).
///
/// Its own function because "did removing this operator also remove curvature
/// the operator can see" is a disclosure question asked from two arms of
/// [`plan_delete_node`], and a missed arm would be a silent shape change.
fn is_curve_keyword(keyword: &[u8]) -> bool {
    matches!(keyword, b"c" | b"v" | b"y")
}

/// Record one operator rewrite, prepending any pending lead-in bytes (an
/// explicit `m` materialized for an implicitly-started subpath) to the FIRST
/// rewrite only.
///
/// Exists so the insertion never becomes a second edit sharing a start offset
/// with the rewrite: [`splice`] skips an edit starting before its cursor, so
/// such a pair silently loses one member depending on sort order.
fn push_edit(
    edits: &mut Vec<(usize, usize, Vec<u8>)>,
    lead_insert: &mut Option<Vec<u8>>,
    item: &OpItem<'_>,
    body: Vec<u8>,
) {
    let bytes = match lead_insert.take() {
        Some(mut lead) => {
            lead.extend_from_slice(&body);
            lead
        }
        None => body,
    };
    edits.push((item.byte_start(), item.byte_end(), bytes));
}

/// What an operator is told when an `re` rectangle had to be expanded into
/// four lines to place a corner independently (§8.5.2.1 Table 59).
///
/// # A constant because TWO functions owe the same sentence
///
/// [`plan_move_node`] and [`plan_move_nodes`] must word this identically —
/// a one-node batch and a single-node call describe the same event, and an
/// operator who saw the wording change would reasonably conclude something
/// else had changed too. Held as a constant so the two cannot drift, and
/// pinned by `node_multi_move.rs`'s
/// `a_single_element_batch_matches_the_single_node_verb`, which compares
/// the disclosure text and not just the bytes.
///
/// # ★ The wording is deliberately COUNT-AGNOSTIC — do not re-add a number
///
/// It read *"Moving **one corner** on its own"* until a multi-node drag was
/// driven in the running application and moved **two** corners of one
/// rectangle, at which point the sentence was simply false about what the
/// operator had just done. Nothing caught it: every test asserted the
/// constant against itself, so the text was self-consistent and wrong.
///
/// The singular/plural axis this constant sits on is *how many SHAPES were
/// rewritten* ([`RECT_EXPANDED_DISCLOSURE_PLURAL`]), **not** how many
/// corners moved. Mixing the two axes into one sentence is what made it
/// wrong, and re-introducing a corner count would do it again — the shape
/// is rewritten for the same reason whether one corner moved or four, and
/// the number is not something the operator can act on.
const RECT_EXPANDED_DISCLOSURE: &str = "This shape was stored as a rectangle, which can only describe a box with square \
     corners. Moving a corner independently makes it a four-sided shape that is no longer \
     a box, so it has been rewritten as four lines. It draws identically; dragging the \
     corner back will not restore the original rectangle form.";

/// [`RECT_EXPANDED_DISCLOSURE`] for a multi-node drag that expanded **more
/// than one** rectangle — said once, not once per shape.
const RECT_EXPANDED_DISCLOSURE_PLURAL: &str = "More than one of these shapes was stored as a rectangle, which can only describe a \
     box with square corners. Moving a corner on its own makes a shape that is no longer \
     a box, so each has been rewritten as four lines. They draw identically; dragging the \
     corners back will not restore the original rectangle form.";

/// What an operator is told when a subpath's reused start had no
/// coordinates of its own and an `m` was materialised for it (§8.5.2.1).
///
/// A constant for the same reason as [`RECT_EXPANDED_DISCLOSURE`].
const IMPLICIT_START_DISCLOSURE: &str = "This point had no coordinates of its own — the file re-used the start of the shape \
     before it. A move instruction naming the point has been added so it can be placed \
     independently.";

/// [`IMPLICIT_START_DISCLOSURE`] for a multi-node drag that materialised
/// more than one such start.
const IMPLICIT_START_DISCLOSURE_PLURAL: &str = "More than one of these points had no coordinates of its own — the file re-used the \
     start of an earlier shape for each. A move instruction naming each point has been \
     added so they can be placed independently.";

/// Plan a **node drag**: rewrite the single anchor `node_index` of `obj` to
/// the page-space point `to_page` (anchor/corner move only — adjacent
/// Bézier control-point "handle" editing is a named fast-follow, decision
/// 011 §2.5).
///
/// `node_index` is into the object's anchors in **decomposition order**
/// (module docs). The target point is mapped from page space to the
/// object's user space with the full affine inverse, and only the ONE
/// operator that defines that anchor is re-emitted with its anchor pair
/// replaced — every other operator of the object, and every other object,
/// stays byte-verbatim (so exactly one operator's bytes change).
///
/// # Errors
///
/// [`VectorEditError::NodeOutOfRange`], [`VectorEditError::DegenerateCtm`]
/// (singular CTM), or [`VectorEditError::MalformedOperand`] (an operator whose
/// operand count contradicts Table 59). Each is raised before any content byte
/// is produced.
///
/// # Examples
///
/// ```
/// use pdfcer_core::content::ContentStream;
/// use pdfcer_core::vector::{decompose, NoXObjects, Matrix, Point, VectorObject};
/// use pdfcer_core::vector::edit::plan_move_node;
///
/// let cs = ContentStream::parse(b"10 20 m 100 200 l S".to_vec()).unwrap();
/// let model = decompose(&cs, Matrix::IDENTITY, &NoXObjects);
/// let VectorObject::Path(path) = &model.objects[0] else { unreachable!() };
/// // Drag node 1 (the `l` endpoint) to (120, 250).
/// let plan = plan_move_node(&cs, path, 1, Point::new(120.0, 250.0)).unwrap();
/// assert_eq!(plan.content, b"10 20 m 120 250 l S");
/// ```
pub fn plan_move_node(
    content: &ContentStream,
    obj: &PathObject,
    node_index: usize,
    to_page: Point,
) -> Result<PlannedEdit, VectorEditError> {
    let inv = obj.ctm.inverse().ok_or(VectorEditError::DegenerateCtm)?;
    let to_user = inv.map_point(to_page);
    // Owed regardless of which of the three rewrites runs below, so it is
    // computed once here rather than in each arm — where the next arm added
    // would forget it.
    let clip = clip_disclosure(content, obj);

    let anchors = enumerate_anchors(content, obj.tokens.start, obj.tokens.end);
    let count = anchors.len();
    let site = anchors
        .get(node_index)
        .ok_or(VectorEditError::NodeOutOfRange {
            index: node_index,
            count,
        })?;

    match site.kind {
        // ---- (1) The operand exists: overwrite it in place. -------------
        AnchorKind::Editable => {
            // Replace the anchor's coordinate pair (operand indices 2k, 2k+1)
            // with the user-space target, then re-emit that one operator.
            // Everything else is byte-verbatim.
            let mut new_nums = site.operands.clone();
            let x_slot = site.pair_index * 2;
            let y_slot = x_slot + 1;
            // The anchor bookkeeping only marks an operator Editable when its
            // arity matched, so `y_slot` is in range; the guard degrades an
            // impossible out-of-range to a by-name refusal rather than an
            // index-panic (crate panic-free policy).
            if x_slot >= new_nums.len() || y_slot >= new_nums.len() {
                return Err(VectorEditError::MalformedOperand);
            }
            if let Some(x) = new_nums.get_mut(x_slot) {
                *x = to_user.x;
            }
            if let Some(y) = new_nums.get_mut(y_slot) {
                *y = to_user.y;
            }
            let mut edits = vec![(
                site.byte_start,
                site.byte_end,
                emit_op(&new_nums, &site.keyword),
            )];
            Ok(PlannedEdit {
                content: splice(&content.buf, &mut edits),
                operators_touched: 1,
                disclosures: clip,
            })
        }

        // ---- (2) `re` names a size, not four corners: expand it. --------
        AnchorKind::Rectangle { corner } => {
            let [x, y, w, h] = site.operands[..] else {
                return Err(VectorEditError::MalformedOperand);
            };
            // The spec's own equivalence (§8.5.2.1, Table 59): `x y w h re`
            // IS `x y m / x+w y l / x+w y+h l / x y+h l / h`. Emitting that
            // form changes no pixel — the trailing `h` is load-bearing and
            // must not be dropped, because a stroked subpath left OPEN gets
            // two line caps where the closed one gets a corner join.
            let mut pts = rect_corners(x, y, w, h);
            let Some(dragged) = pts.get_mut(corner) else {
                return Err(VectorEditError::NodeOutOfRange {
                    index: node_index,
                    count,
                });
            };
            *dragged = to_user;

            let mut out = Vec::new();
            for (i, p) in pts.iter().enumerate() {
                if i > 0 {
                    out.push(b' ');
                }
                out.extend_from_slice(&emit_op(&[p.x, p.y], if i == 0 { b"m" } else { b"l" }));
            }
            out.extend_from_slice(b" h");

            let mut edits = vec![(site.byte_start, site.byte_end, out)];
            Ok(PlannedEdit {
                content: splice(&content.buf, &mut edits),
                operators_touched: 1,
                disclosures: [clip, vec![RECT_EXPANDED_DISCLOSURE.to_owned()]].concat(),
            })
        }

        // ---- (3) Nothing names this point: write the `m` that was left
        //          implicit, immediately before the segment that inherits it.
        AnchorKind::Implicit => {
            // After `h` (or `re`) the current point is the closed subpath's
            // start, and the next segment operator reopens there with no `m`
            // of its own (§8.5.2.1). An explicit `m` at the target overrides
            // exactly that inheritance and nothing else: the closed subpath
            // is already terminated, so no earlier geometry can see it.
            let mut m = emit_op(&[to_user.x, to_user.y], b"m");
            m.push(b' ');
            m.extend_from_slice(content.buf.get(site.byte_start..site.byte_end).ok_or(
                VectorEditError::NodeOutOfRange {
                    index: node_index,
                    count,
                },
            )?);
            let mut edits = vec![(site.byte_start, site.byte_end, m)];
            Ok(PlannedEdit {
                content: splice(&content.buf, &mut edits),
                operators_touched: 1,
                disclosures: [clip, vec![IMPLICIT_START_DISCLOSURE.to_owned()]].concat(),
            })
        }
    }
}

/// Plan a **multi-node drag**: move every anchor named in `moves` to its own
/// target, as ONE surgery over one content stream.
///
/// # Why this exists rather than a loop over [`plan_move_node`]
///
/// **One gesture must be one undo.** The GUI has had a multi-node selection
/// set since `Pass 41.0` and could not move it, because N calls to
/// `plan_move_node` would put N entries on the undo stack for one drag —
/// the operator presses Ctrl+Z once and half their nodes stay moved.
///
/// **And N calls owe the same disclosure N times.** Expanding two `re`
/// rectangles in one drag would tell the operator the same paragraph twice;
/// this verb de-duplicates, and switches to singular wording when exactly
/// one shape provoked it.
///
/// ## What is NOT a reason, having been checked
///
/// It is tempting to argue that a loop would **corrupt** an `re`: all four
/// corners are described by the same four operands of one operator, and
/// [`plan_move_node`] handles a single corner by expanding that operator
/// into its §8.5.2.1 `m`/`l`/`l`/`l`/`h` equivalent, so a second call would
/// be planning against bytes the first already replaced.
///
/// **That argument is wrong, and the test suite proves it rather than
/// asserting it** (`node_multi_move.rs`). A caller that re-decomposes
/// between calls — which it must, since `plan_move_node` takes a
/// `ContentStream` — gets fresh offsets, and the expansion preserves both
/// the anchor **count** and the anchor **order**, so index *k* still names
/// the same geometric point afterwards. The loop produces byte-identical
/// output to this function for that case.
///
/// The reason to record the refutation instead of quietly dropping it: it
/// is the argument a future reader will reconstruct when they wonder
/// whether this verb could be deleted, and they should find it already
/// tested and answered.
///
/// # The mechanism: group by the OPERATOR, not by the node
///
/// Each anchor knows the byte range of the operator that defines it. Anchors
/// are therefore bucketed by `byte_start`, and **each bucket produces exactly
/// one replacement** covering that range:
///
/// - four `re` corners in one bucket → one expansion, with all four
///   requested corners applied to it before it is emitted;
/// - an `m`/`l`/`c`/`v`/`y` endpoint → its own operand pair rewritten, every
///   other operand byte-verbatim;
/// - an [implicit](AnchorKind::Implicit) reused subpath start → the omitted
///   `m` written in front of the segment that inherits it, **and if that same
///   segment also carries a moved endpoint, both changes land in the one
///   replacement** rather than as two overlapping edits (which `splice`
///   would silently drop).
///
/// The buckets are disjoint by construction, so the splice is
/// non-overlapping without needing to be checked for it.
///
/// # Errors
///
/// [`VectorEditError::EmptyMove`] for an empty request;
/// [`VectorEditError::DuplicateNodeInMove`] when one anchor is named twice
/// (refused rather than resolved — see that variant);
/// [`VectorEditError::NodeOutOfRange`] for an index past the anchor count;
/// [`VectorEditError::DegenerateCtm`]; [`VectorEditError::MalformedOperand`].
/// **Every refusal happens before any byte is planned**, so a rejected
/// request leaves the caller exactly where it was (rule 4).
///
/// # Returns
///
/// The [disclosures](PlannedEdit::disclosures) the surgery owes, **each at
/// most once however many nodes provoked it** — a five-corner drag across
/// two rectangles should not tell the operator the same paragraph five
/// times. [`PlannedEdit::operators_touched`] counts **operators**, not
/// nodes, so moving four corners of one rectangle reports 1.
///
/// # Examples
///
/// Two ordinary anchors, two operators rewritten, everything else verbatim:
///
/// ```
/// use pdfcer_core::content::ContentStream;
/// use pdfcer_core::vector::{decompose, NoXObjects, Matrix, Point, VectorObject};
/// use pdfcer_core::vector::edit::plan_move_nodes;
///
/// let cs = ContentStream::parse(b"10 20 m 100 200 l 50 60 l S".to_vec()).unwrap();
/// let model = decompose(&cs, Matrix::IDENTITY, &NoXObjects);
/// let VectorObject::Path(path) = &model.objects[0] else { unreachable!() };
/// let plan = plan_move_nodes(
///     &cs,
///     path,
///     &[(0, Point::new(11.0, 21.0)), (2, Point::new(51.0, 61.0))],
/// )
/// .unwrap();
/// assert_eq!(plan.content, b"11 21 m 100 200 l 51 61 l S");
/// assert_eq!(plan.operators_touched, 2);
/// ```
///
/// Two corners of ONE rectangle — the case an N-call loop cannot do,
/// because both are the same four operands of the same operator:
///
/// ```
/// use pdfcer_core::content::ContentStream;
/// use pdfcer_core::vector::{decompose, NoXObjects, Matrix, Point, VectorObject};
/// use pdfcer_core::vector::edit::plan_move_nodes;
///
/// let cs = ContentStream::parse(b"0 0 10 10 re S".to_vec()).unwrap();
/// let model = decompose(&cs, Matrix::IDENTITY, &NoXObjects);
/// let VectorObject::Path(path) = &model.objects[0] else { unreachable!() };
/// // Corner 0 is (0,0); corner 2 is (10,10). Drag both outwards.
/// let plan = plan_move_nodes(
///     &cs,
///     path,
///     &[(0, Point::new(-1.0, -1.0)), (2, Point::new(12.0, 12.0))],
/// )
/// .unwrap();
/// // ONE expansion carrying BOTH moves — corners 1 and 3 keep the
/// // coordinates the original `re` implied.
/// assert_eq!(plan.content, b"-1 -1 m 10 0 l 12 12 l 0 10 l h S");
/// assert_eq!(plan.operators_touched, 1);
/// assert_eq!(plan.disclosures.len(), 1);
/// ```
pub fn plan_move_nodes(
    content: &ContentStream,
    obj: &PathObject,
    moves: &[(usize, Point)],
) -> Result<PlannedEdit, VectorEditError> {
    if moves.is_empty() {
        return Err(VectorEditError::EmptyMove);
    }
    // Duplicate check FIRST, before any lookup: a request that names node 3
    // twice is malformed whether or not node 3 exists, and reporting the
    // out-of-range error for it would send the caller after the wrong bug.
    let mut seen = BTreeSet::new();
    for (index, _) in moves {
        if !seen.insert(*index) {
            return Err(VectorEditError::DuplicateNodeInMove { index: *index });
        }
    }

    let inv = obj.ctm.inverse().ok_or(VectorEditError::DegenerateCtm)?;
    let anchors = enumerate_anchors(content, obj.tokens.start, obj.tokens.end);
    let count = anchors.len();

    // Resolve every request before planning anything, so an out-of-range
    // index in position 7 does not leave positions 0..6 already spliced.
    let mut resolved: Vec<(usize, Point)> = Vec::with_capacity(moves.len());
    for (index, to_page) in moves {
        if *index >= count {
            return Err(VectorEditError::NodeOutOfRange {
                index: *index,
                count,
            });
        }
        resolved.push((*index, inv.map_point(*to_page)));
    }

    // Bucket by the operator each anchor lives in. `BTreeMap` so the buckets
    // come out in byte order, which makes the emitted edit list already
    // sorted the way `splice` wants it.
    let mut buckets: BTreeMap<usize, Vec<(usize, Point)>> = BTreeMap::new();
    for (index, to_user) in resolved {
        let Some(site) = anchors.get(index) else {
            return Err(VectorEditError::NodeOutOfRange { index, count });
        };
        buckets
            .entry(site.byte_start)
            .or_default()
            .push((index, to_user));
    }

    let mut edits: Vec<(usize, usize, Vec<u8>)> = Vec::with_capacity(buckets.len());
    // COUNTED, not flagged, so the disclosure can be singular when exactly
    // one shape provoked it — which is what makes a one-element batch say
    // precisely what `plan_move_node` says for the same node, and lets a
    // front end use one verb for both cases without the wording changing
    // under it. Pinned by `a_single_element_batch_matches_the_single_node_verb`.
    let mut rects_expanded = 0usize;
    let mut implicits_written = 0usize;

    for (_, group) in buckets {
        // Every anchor in a bucket shares one operator, so any of them
        // supplies its byte range and its operands.
        let Some(first) = group.first().and_then(|(i, _)| anchors.get(*i)) else {
            return Err(VectorEditError::MalformedOperand);
        };
        let (byte_start, byte_end) = (first.byte_start, first.byte_end);

        // --- (2) The `re` case: one expansion carrying every moved corner.
        if group
            .iter()
            .filter_map(|(i, _)| anchors.get(*i))
            .any(|s| matches!(s.kind, AnchorKind::Rectangle { .. }))
        {
            let [x, y, w, h] = first.operands[..] else {
                return Err(VectorEditError::MalformedOperand);
            };
            let mut pts = rect_corners(x, y, w, h);
            for (index, to_user) in &group {
                let Some(site) = anchors.get(*index) else {
                    return Err(VectorEditError::MalformedOperand);
                };
                // A bucket holding an `re` corner cannot also hold an
                // Editable or Implicit anchor — `re` opens and closes its own
                // subpath, so nothing else is defined by those bytes. Refused
                // rather than assumed, because silently ignoring the other
                // kind would drop a node the caller asked to move.
                let AnchorKind::Rectangle { corner } = site.kind else {
                    return Err(VectorEditError::MalformedOperand);
                };
                let Some(p) = pts.get_mut(corner) else {
                    return Err(VectorEditError::NodeOutOfRange {
                        index: *index,
                        count,
                    });
                };
                *p = *to_user;
            }
            let mut out = Vec::new();
            for (i, p) in pts.iter().enumerate() {
                if i > 0 {
                    out.push(b' ');
                }
                out.extend_from_slice(&emit_op(&[p.x, p.y], if i == 0 { b"m" } else { b"l" }));
            }
            out.extend_from_slice(b" h");
            edits.push((byte_start, byte_end, out));
            rects_expanded += 1;
            continue;
        }

        // --- (1)+(3) Editable pairs rewritten in place, and an implicit
        // start written in front of the same operator when both land here.
        //
        // The operands come from the bucket's EDITABLE site, not from
        // `first`. An `Implicit` anchor is not defined by operands of its
        // own — it borrows the byte range of the segment that inherits it —
        // so if the implicit anchor happened to sort first, `first.operands`
        // would be the wrong list (or empty) and every `Editable` slot check
        // below would fail with `MalformedOperand`. Found by
        // `an_implicit_start_and_its_segments_endpoint_share_one_replacement`,
        // which is the only shape where a bucket holds both kinds.
        let editable_site = group
            .iter()
            .filter_map(|(i, _)| anchors.get(*i))
            .find(|s| matches!(s.kind, AnchorKind::Editable));
        let mut operands = editable_site.map_or_else(Vec::new, |s| s.operands.clone());
        let keyword = editable_site.map_or_else(Vec::new, |s| s.keyword.clone());
        let mut editable_touched = false;
        let mut implicit_at: Option<Point> = None;
        for (index, to_user) in &group {
            let Some(site) = anchors.get(*index) else {
                return Err(VectorEditError::MalformedOperand);
            };
            match site.kind {
                AnchorKind::Editable => {
                    let x_slot = site.pair_index * 2;
                    let y_slot = x_slot + 1;
                    if x_slot >= operands.len() || y_slot >= operands.len() {
                        return Err(VectorEditError::MalformedOperand);
                    }
                    if let Some(x) = operands.get_mut(x_slot) {
                        *x = to_user.x;
                    }
                    if let Some(y) = operands.get_mut(y_slot) {
                        *y = to_user.y;
                    }
                    editable_touched = true;
                }
                AnchorKind::Implicit => implicit_at = Some(*to_user),
                AnchorKind::Rectangle { .. } => {
                    // Unreachable: the `re` branch above claimed any bucket
                    // containing one. Guarded rather than assumed.
                    return Err(VectorEditError::MalformedOperand);
                }
            }
        }

        // The operator itself: re-emitted when an endpoint moved, otherwise
        // the ORIGINAL bytes, so an implicit-only move leaves the segment it
        // prefixes byte-verbatim (rule 3 inside one operator).
        let body: Vec<u8> = if editable_touched {
            emit_op(&operands, &keyword)
        } else {
            content
                .buf
                .get(byte_start..byte_end)
                .ok_or(VectorEditError::MalformedOperand)?
                .to_vec()
        };
        let out = if let Some(p) = implicit_at {
            implicits_written += 1;
            let mut m = emit_op(&[p.x, p.y], b"m");
            m.push(b' ');
            m.extend_from_slice(&body);
            m
        } else {
            body
        };
        edits.push((byte_start, byte_end, out));
    }

    // Disclosures, de-duplicated: each shape change is described ONCE
    // however many nodes provoked it, and in the SINGULAR when exactly one
    // did — which is what makes a one-element batch word its result exactly
    // as `plan_move_node` words the same node's.
    let mut disclosures = clip_disclosure(content, obj);
    match rects_expanded {
        0 => {}
        1 => disclosures.push(RECT_EXPANDED_DISCLOSURE.to_owned()),
        _ => disclosures.push(RECT_EXPANDED_DISCLOSURE_PLURAL.to_owned()),
    }
    match implicits_written {
        0 => {}
        1 => disclosures.push(IMPLICIT_START_DISCLOSURE.to_owned()),
        _ => disclosures.push(IMPLICIT_START_DISCLOSURE_PLURAL.to_owned()),
    }

    let operators_touched = edits.len();
    Ok(PlannedEdit {
        content: splice(&content.buf, &mut edits),
        operators_touched,
        disclosures,
    })
}

/// Which of a node's two Bézier control points ("handles") to move.
///
/// An on-curve anchor sits between at most two segments, and each contributes
/// one control point to it: the segment arriving contributes its SECOND
/// control point, the segment leaving its FIRST. They are the two levers that
/// shape the curve on either side of the point without moving the point.
///
/// Named for direction of travel along the path rather than "first/second",
/// because first-and-second are properties of an *operator* while an operator
/// says nothing about which node a front end has selected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Handle {
    /// The control point governing the curve as it ARRIVES at the node — the
    /// second control point of the segment that ends here.
    Incoming,
    /// The control point governing the curve as it LEAVES the node — the
    /// first control point of the segment that starts here.
    Outgoing,
}

/// Plan a **handle drag**: move one Bézier control point of `node_index`,
/// leaving the on-curve node itself exactly where it is.
///
/// This is the operation that changes a curve's SHAPE. Without it
/// [`plan_move_node`] can only move the points a curve passes through, so a
/// curve's curvature was not editable at all.
///
/// # Implicit control points, and why a `v`/`y` drag rewrites the operator
///
/// Table 59 (§8.5.2.1) gives cubic segments three spellings, and two of them
/// omit a control point by making it equal to a point they already have:
///
/// | operator | operands | first control | second control |
/// |---|---|---|---|
/// | `c` | `x1 y1 x2 y2 x3 y3` | `(x1,y1)` | `(x2,y2)` |
/// | `v` | `x2 y2 x3 y3` | **the current point** | `(x2,y2)` |
/// | `y` | `x1 y1 x3 y3` | `(x1,y1)` | **the endpoint** |
///
/// So dragging `v`'s incoming-side handle or `y`'s outgoing-side handle is
/// an in-place operand rewrite, while dragging the OTHER one asks to move a
/// point whose whole definition is "equal to that other point". It cannot
/// stay implicit and also move, so the segment is re-spelled as the `c` that
/// states both control points — the same materialize-rather-than-refuse move
/// Pass 30.0 makes for `re` corners, and disclosed for the same reason.
///
/// # What is refused, and why not silently converted
///
/// A straight segment (`l`, or a rectangle edge) has no handles. Dragging one
/// could only mean "turn this line into a curve", which is a different
/// operation with a different name: it changes the object's shape vocabulary
/// rather than adjusting a curve that already exists. Guessing it from a drag
/// on a control point that was never drawn would be exactly the silent
/// reinterpretation rule 4 forbids, so it is refused by name and a caller that
/// wants the conversion asks for it.
///
/// # Errors
///
/// [`VectorEditError::NodeOutOfRange`], [`VectorEditError::NoHandleHere`]
/// (the neighbouring segment is straight, absent, or across a subpath
/// boundary), [`VectorEditError::DegenerateCtm`], or
/// [`VectorEditError::MalformedOperand`].
///
/// # Examples
///
/// ```
/// use pdfcer_core::content::ContentStream;
/// use pdfcer_core::vector::{decompose, NoXObjects, Matrix, Point, VectorObject};
/// use pdfcer_core::vector::edit::{plan_move_handle, Handle};
///
/// let cs = ContentStream::parse(b"0 0 m 10 40 60 40 70 0 c S".to_vec()).unwrap();
/// let model = decompose(&cs, Matrix::IDENTITY, &NoXObjects);
/// let VectorObject::Path(path) = &model.objects[0] else { unreachable!() };
/// // Node 0 is the `m`; its OUTGOING handle is the `c`'s first control point.
/// let plan = plan_move_handle(&cs, path, 0, Handle::Outgoing, Point::new(5.0, 90.0)).unwrap();
/// assert_eq!(plan.content, b"0 0 m 5 90 60 40 70 0 c S");
/// ```
pub fn plan_move_handle(
    content: &ContentStream,
    obj: &PathObject,
    node_index: usize,
    handle: Handle,
    to_page: Point,
) -> Result<PlannedEdit, VectorEditError> {
    let inv = obj.ctm.inverse().ok_or(VectorEditError::DegenerateCtm)?;
    let to_user = inv.map_point(to_page);
    let clip = clip_disclosure(content, obj);

    let anchors = enumerate_anchors(content, obj.tokens.start, obj.tokens.end);
    let count = anchors.len();
    if node_index >= count {
        return Err(VectorEditError::NodeOutOfRange {
            index: node_index,
            count,
        });
    }

    // WHICH operator carries the handle:
    //
    // - Incoming: the operator that ENDS at this node — the node's own site.
    // - Outgoing: the operator that ends at the NEXT node, since that is the
    //   segment leaving here.
    //
    // Anchor indices are object-scoped and run straight across subpath
    // boundaries, so "the next anchor" after a subpath's last node is the NEXT
    // SUBPATH's first node — and reshaping its segment would edit geometry the
    // operator never selected.
    //
    // What actually prevents that is the KEYWORD match below: every anchor
    // that opens a subpath carries `m`, `re`, or (for an `h`-reopen) no
    // keyword at all, none of which is a curve, so all three fall through to
    // `NoHandleHere`. The `is_start` filter here is a second line of defence
    // that cannot fire today. It is kept because it states the intent
    // structurally rather than as a consequence of which keywords happen to
    // exist — but it is NOT the thing to rely on when touching the keyword
    // match, and the test that covers this boundary passes with this filter
    // deleted. That was verified, not assumed.
    let site = match handle {
        Handle::Incoming => anchors.get(node_index),
        Handle::Outgoing => anchors.get(node_index + 1).filter(|next| !next.is_start),
    }
    .ok_or(VectorEditError::NoHandleHere {
        index: node_index,
        handle,
    })?;

    // Which operand pair holds the requested control point, per Table 59 —
    // `None` where the spelling leaves it implicit and the operator has to be
    // promoted to `c`.
    let pair: Option<usize> = match (site.keyword.as_slice(), handle) {
        (b"c", Handle::Outgoing) => Some(0),
        (b"c", Handle::Incoming) => Some(1),
        // `v`'s second control point is explicit; its first IS the current
        // point, so it can only move by becoming a `c`.
        (b"v", Handle::Incoming) => Some(0),
        (b"v", Handle::Outgoing) => None,
        // `y` mirrors it: first explicit, second equals the endpoint.
        (b"y", Handle::Outgoing) => Some(0),
        (b"y", Handle::Incoming) => None,
        // `m`, `l`, `re`, or an implicit start: no curve on that side.
        _ => {
            return Err(VectorEditError::NoHandleHere {
                index: node_index,
                handle,
            });
        }
    };

    let (bytes, disclosures) = match pair {
        // In-place operand rewrite: the control point is already written down.
        Some(p) => {
            let mut nums = site.operands.clone();
            let (xs, ys) = (p * 2, p * 2 + 1);
            if ys >= nums.len() {
                return Err(VectorEditError::MalformedOperand);
            }
            if let Some(x) = nums.get_mut(xs) {
                *x = to_user.x;
            }
            if let Some(y) = nums.get_mut(ys) {
                *y = to_user.y;
            }
            (emit_op(&nums, &site.keyword), clip)
        }
        // Promotion: re-spell the segment as the `c` that states BOTH control
        // points, so the one that was implicit can hold its own value.
        None => {
            let promoted = promote_to_cubic(site, handle, to_user)?;
            (
                promoted,
                [
                    clip,
                    vec![
                        "This curve was written in a short form that left one of its \
                         shaping handles implied by another point, so it could not be \
                         moved on its own. The curve has been rewritten in the long \
                         form that states both handles. It draws identically."
                            .to_owned(),
                    ],
                ]
                .concat(),
            )
        }
    };

    let mut edits = vec![(site.byte_start, site.byte_end, bytes)];
    Ok(PlannedEdit {
        content: splice(&content.buf, &mut edits),
        operators_touched: 1,
        disclosures,
    })
}

/// Re-spell a `v` or `y` segment as the equivalent `c`, with the previously
/// implicit control point set to `to_user`.
///
/// The two spellings are shorthands for a cubic whose omitted control point
/// duplicates a point the operator already carries (Table 59), so the `c` form
/// is exactly equivalent when it repeats that point — which is what makes this
/// a re-spelling rather than a change of shape.
///
/// `v`'s implicit FIRST control point is the current point, which is not in
/// the operator's own operands; it is the previous node, which the caller
/// resolves. Here the value being written is the operator's new position, so
/// the old one is not needed at all — the point is being replaced, not copied.
fn promote_to_cubic(
    site: &AnchorSite,
    handle: Handle,
    to_user: Point,
) -> Result<Vec<u8>, VectorEditError> {
    let &[a, b, c, d] = site.operands.as_slice() else {
        return Err(VectorEditError::MalformedOperand);
    };
    let nums = match (site.keyword.as_slice(), handle) {
        // `x2 y2 x3 y3 v` → `NEW x2 y2 x3 y3 c`: the dragged first control
        // point takes the lead, the explicit second and the endpoint follow.
        (b"v", Handle::Outgoing) => [to_user.x, to_user.y, a, b, c, d],
        // `x1 y1 x3 y3 y` → `x1 y1 NEW x3 y3 c`: the explicit first control
        // point stays, the dragged second takes the middle, endpoint last.
        (b"y", Handle::Incoming) => [a, b, to_user.x, to_user.y, c, d],
        _ => return Err(VectorEditError::MalformedOperand),
    };
    Ok(emit_op(&nums, b"c"))
}

/// The number of node-draggable and non-draggable anchors an object
/// exposes, in decomposition order — the count a front end validates a node
/// index against, and the value [`VectorEditError::NodeOutOfRange`] reports.
///
/// Equal to `obj.subpaths.iter().map(|s| s.anchors().count()).sum()` by
/// construction (this walk mirrors [`super::decompose`]); provided so the
/// CLI/GUI need not re-derive the flattening.
#[must_use]
pub fn anchor_count(content: &ContentStream, obj: &PathObject) -> usize {
    enumerate_anchors(content, obj.tokens.start, obj.tokens.end).len()
}

// ---------------------------------------------------------------------------
// Operand arithmetic
// ---------------------------------------------------------------------------

/// Translate the point operands of a construction operator by `(du, dv)`.
/// `translate` has one flag per **point pair** (`m`/`l` = 1, `v`/`y` = 2,
/// `c` = 3); a `true` pair is shifted. Returns `None` if `nums` does not
/// hold exactly `2 × translate.len()` operands (a malformed operator).
fn translate_points(nums: &[f64], translate: &[bool], du: f64, dv: f64) -> Option<Vec<f64>> {
    if nums.len() != translate.len() * 2 {
        return None;
    }
    let mut out = nums.to_vec();
    for (pair, &shift) in translate.iter().enumerate() {
        if shift {
            // Indices are in range by the length check above; checked access
            // keeps the crate's panic-free policy (clippy::indexing_slicing).
            if let Some(x) = out.get_mut(pair * 2) {
                *x += du;
            }
            if let Some(y) = out.get_mut(pair * 2 + 1) {
                *y += dv;
            }
        }
    }
    Some(out)
}

/// Translate an `re x y w h` operator: shift the origin `(x, y)` by
/// `(du, dv)`, leave the size `(w, h)` unchanged. `None` if the arity is
/// not 4.
fn translate_rect(nums: &[f64], du: f64, dv: f64) -> Option<Vec<f64>> {
    match *nums {
        [x, y, w, h] => Some(vec![x + du, y + dv, w, h]),
        _ => None,
    }
}

/// Emit an operation as `operand operand … keyword` bytes (the `emit_tm`
/// pattern), the numbers formatted by the writer's total
/// [`emit_number`] and the keyword copied verbatim (so `f*`, `B*` survive).
fn emit_op(operands: &[f64], keyword: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for (i, v) in operands.iter().enumerate() {
        if i > 0 {
            out.push(b' ');
        }
        emit_number(&mut out, *v);
    }
    out.push(b' ');
    out.extend_from_slice(keyword);
    out
}

// ---------------------------------------------------------------------------
// Operation iteration over a token range
// ---------------------------------------------------------------------------

/// One operation (operand run + operator) inside a token range — the same
/// segmentation [`ContentStream::operations`] performs, but bounded to a
/// half-open token range and exposing the byte bounds the splice needs.
struct OpItem<'a> {
    operands: &'a [ContentToken],
    operator: &'a ContentToken,
}

impl OpItem<'_> {
    /// The operator keyword bytes, or `None` for an inline image (which is
    /// its own indivisible "operator" token but has no keyword).
    fn keyword<'b>(&self, buf: &'b [u8]) -> Option<&'b [u8]> {
        match self.operator.kind {
            ContentTokenKind::Operator => self.operator.span.slice(buf),
            _ => None,
        }
    }

    /// The numeric operands, in order (non-numeric operands skipped, matching
    /// the decomposer's tolerance).
    fn nums(&self) -> Vec<f64> {
        self.operands
            .iter()
            .filter_map(|t| match &t.kind {
                ContentTokenKind::Operand(o) => o.as_number(),
                _ => None,
            })
            .collect()
    }

    /// Byte offset of the operation's first operand (or the operator, when
    /// there are none) — the splice start.
    fn byte_start(&self) -> usize {
        self.operands
            .first()
            .map_or(self.operator.span.start, |t| t.span.start)
    }

    /// Byte offset one past the operator — the splice end.
    fn byte_end(&self) -> usize {
        self.operator.span.end()
    }
}

/// The operations whose operator token index lies in `[start, end)`.
///
/// `end` is the exclusive one-past-the-painting-operator bound Pass 9a
/// captures ([`super::decompose::TokenRange`]), so the painting operator at
/// `end - 1` is included. Operand runs are grouped exactly as
/// [`ContentStream::operations`] groups them.
fn ops_in_range(content: &ContentStream, start: usize, end: usize) -> Vec<OpItem<'_>> {
    let mut out = Vec::new();
    let end = end.min(content.tokens.len());
    let start = start.min(end);
    let mut run_start = start;
    for i in start..end {
        let Some(tok) = content.tokens.get(i) else {
            break;
        };
        if matches!(tok.kind, ContentTokenKind::Operand(_)) {
            continue;
        }
        let operands = content.tokens.get(run_start..i).unwrap_or(&[]);
        out.push(OpItem {
            operands,
            operator: tok,
        });
        run_start = i + 1;
    }
    out
}

// ---------------------------------------------------------------------------
// Anchor enumeration (mirrors decompose's subpath bookkeeping)
// ---------------------------------------------------------------------------

/// How an anchor's coordinates are carried in the content stream — which
/// decides HOW a node drag rewrites it, not WHETHER it can.
///
/// All three are draggable as of Pass 30.0. The distinction survives because
/// the three need three different rewrites: one replaces an operand pair, one
/// expands an operator, one inserts a new operator. See [`plan_move_node`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AnchorKind {
    /// A real operand pair (`m`/`l`/`c`/`v`/`y`) — rewritable in place.
    Editable,
    /// A corner of an `re` rectangle. `re` carries an origin and a *size*, so
    /// no operand names this corner (only corner 0 appears literally, and
    /// even it cannot move alone without dragging the other three with it).
    /// Dragged by expanding the operator to its §8.5.2.1 equivalent.
    Rectangle {
        /// Which corner, in [`super::geometry::rect_corners`] order —
        /// `(x, y)`, `(x+w, y)`, `(x+w, y+h)`, `(x, y+h)` — which is also the
        /// order of the spec's equivalent `m`/`l`/`l`/`l` sequence, so the
        /// index means the same thing before and after the expansion.
        corner: usize,
    },
    /// The reused start of an `h`-reopened subpath (§8.5.2.1): its
    /// coordinates are *inherited* from the closed subpath's start rather
    /// than written anywhere. Dragged by inserting the `m` the file omitted.
    Implicit,
}

/// One anchor, with the operator that defines it (for the editable case).
struct AnchorSite {
    kind: AnchorKind,
    byte_start: usize,
    byte_end: usize,
    operands: Vec<f64>,
    keyword: Vec<u8>,
    /// Which operand **pair** carries the anchor's coordinates (`m`/`l` → 0,
    /// `v`/`y` → 1, `c` → 2). Ignored for non-editable anchors.
    pair_index: usize,
    /// Whether this anchor OPENS its subpath (an `m`, an `re` corner, or an
    /// implicit `h`-reopen) rather than ending a segment.
    ///
    /// Needed by handle editing to tell "the next anchor is the far end of my
    /// outgoing segment" from "the next anchor belongs to the next subpath
    /// entirely" — indices are object-scoped and run straight across the
    /// boundary, so without this a handle drag on a subpath's last node would
    /// silently reshape the following subpath's first segment.
    is_start: bool,
}

/// Enumerate the object's anchors in the SAME order
/// `obj.subpaths.flat_map(Subpath::anchors)` produces (module docs), by
/// replaying [`super::decompose`]'s subpath / empty-subpath / `h`-reopen
/// state machine over the token range.
fn enumerate_anchors(content: &ContentStream, start: usize, end: usize) -> Vec<AnchorSite> {
    let mut w = AnchorWalk::default();
    for item in ops_in_range(content, start, end) {
        let Some(keyword) = item.keyword(&content.buf) else {
            continue;
        };
        let nums = item.nums();
        let bs = item.byte_start();
        let be = item.byte_end();
        match keyword {
            b"m" => {
                if nums.len() == 2 {
                    // A new subpath: finalize the previous open one, then open
                    // at the `m` anchor.
                    w.finalize_open();
                    w.open_start = Some(AnchorSite {
                        kind: AnchorKind::Editable,
                        byte_start: bs,
                        byte_end: be,
                        operands: nums,
                        keyword: keyword.to_vec(),
                        pair_index: 0,
                        is_start: true,
                    });
                    w.open_ends.clear();
                    w.current = true;
                    w.needs_move = false;
                }
            }
            b"l" => w.segment(&nums, 1, 0, bs, be, keyword),
            b"c" => w.segment(&nums, 3, 2, bs, be, keyword),
            b"v" | b"y" => w.segment(&nums, 2, 1, bs, be, keyword),
            b"re" => {
                if nums.len() == 4 {
                    // A complete closed subpath of four corners, one operator.
                    w.finalize_open();
                    for corner in 0..4 {
                        w.committed.push(AnchorSite {
                            kind: AnchorKind::Rectangle { corner },
                            byte_start: bs,
                            byte_end: be,
                            operands: nums.clone(),
                            keyword: keyword.to_vec(),
                            pair_index: 0,
                            is_start: true,
                        });
                    }
                    w.current = true;
                    w.needs_move = true;
                }
            }
            b"h" => {
                // Close: finalize the open subpath; the current point becomes
                // the subpath start and the next segment reopens there.
                w.finalize_open();
                w.needs_move = true;
            }
            b"S" | b"s" | b"f" | b"F" | b"f*" | b"B" | b"B*" | b"b" | b"b*" | b"n" => {
                w.finalize_open();
            }
            _ => {}
        }
    }
    // A trailing open path with no painting operator is dropped (matches the
    // decomposer discarding an unpainted path); its anchors are not counted.
    w.committed
}

/// The anchor-walk state (mirrors `decompose::PathAccum`'s `open`/`current`/
/// `needs_move`, minus geometry).
#[derive(Default)]
struct AnchorWalk {
    committed: Vec<AnchorSite>,
    open_start: Option<AnchorSite>,
    open_ends: Vec<AnchorSite>,
    current: bool,
    needs_move: bool,
}

impl AnchorWalk {
    /// Commit the open subpath's anchors iff it has at least one segment end
    /// (a lone `m` produces no contour — the decomposer's `finalize_open_pa`
    /// drop of an empty open subpath).
    fn finalize_open(&mut self) {
        if self.open_ends.is_empty() {
            self.open_start = None;
            self.open_ends.clear();
        } else if let Some(startsite) = self.open_start.take() {
            self.committed.push(startsite);
            self.committed.append(&mut self.open_ends);
        } else {
            // A segment without a start (reopened implicit) — the implicit
            // start was already pushed as open_start in `segment`; if it is
            // None here the ends stand alone (defensive).
            self.committed.append(&mut self.open_ends);
        }
    }

    /// Handle a segment operator (`l`/`c`/`v`/`y`): `pairs` = point-pair
    /// count, `anchor_pair` = which pair is the segment's on-curve endpoint.
    fn segment(
        &mut self,
        nums: &[f64],
        pairs: usize,
        anchor_pair: usize,
        bs: usize,
        be: usize,
        keyword: &[u8],
    ) {
        if nums.len() != pairs * 2 {
            return; // malformed arity: no anchor (decomposer skips it)
        }
        if !self.current {
            return; // §8.5.2.1 segment with no current point: skipped
        }
        if self.needs_move {
            // Reopen a subpath at the current point: an implicit start anchor
            // with no operand of its own.
            self.open_start = Some(AnchorSite {
                kind: AnchorKind::Implicit,
                byte_start: bs,
                byte_end: be,
                operands: Vec::new(),
                keyword: Vec::new(),
                pair_index: 0,
                is_start: true,
            });
            self.open_ends.clear();
            self.needs_move = false;
        }
        self.open_ends.push(AnchorSite {
            kind: AnchorKind::Editable,
            byte_start: bs,
            byte_end: be,
            operands: nums.to_vec(),
            keyword: keyword.to_vec(),
            pair_index: anchor_pair,
            is_start: false,
        });
    }
}

// ---------------------------------------------------------------------------
// Selection survival across a delete (Pass 75.0)
// ---------------------------------------------------------------------------

/// Re-point a paint-order object index across a delete, or report that the
/// object it named is gone.
///
/// # Why this exists at all — it is three lines a caller could write
///
/// Because getting it wrong is **silent**, and the wrong answer is worse than
/// an error. `decompose_page` mints paint-order **positions**, not identities,
/// and a position is only an identity while nothing moves. A shell holding a
/// selection across `EditSession::delete_objects` has three possible outcomes:
///
/// | outcome | what the operator sees |
/// |---|---|
/// | the index resolves to the same object | correct |
/// | the index resolves to nothing | correct — the selection clears |
/// | **the index resolves to a DIFFERENT object** | **the outline redraws around the wrong thing, and the next Delete removes it** |
///
/// The third is unreportable after the fact: nothing errors, and the shell has
/// no way to notice. It was raised by the `pdfcer-gui` session
/// (`request_stable_object_identity.md`, 2026-08-13), which correctly declined
/// to build move/resize until it was answered:
///
/// > *"given a token taken before an edit, either resolve it to the same
/// > object afterwards, or tell me it is gone. 'Resolves to a different
/// > object' is the one answer that is worse than an error."*
///
/// A caller **does** have the information — it supplied `deleted` itself — so
/// this could be re-derived at every call site. `R151`'s sibling argument
/// applies: a formula every consumer re-implements is a defect waiting for one
/// of them to get the `<` versus `<=` backwards, and that particular slip
/// produces an off-by-one that only manifests when the deleted object sits
/// exactly at the selection boundary.
///
/// # Which verbs need this
///
/// **Only the `delete_*` family.** `crates/pdfcer-core/tests/object_identity_across_edits.rs`
/// proves empirically that `move_*` does **not** renumber — it rewrites
/// operator operands in place, so no operator is added or removed and the
/// decomposition yields the same objects in the same order. Indices are
/// therefore stable across moves, resizes and node edits, and no remapping is
/// needed for them.
///
/// # Returns
///
/// `Some(new_index)` if the object survived, `None` if `index` was itself
/// deleted. `deleted` need not be sorted and may contain duplicates.
///
/// # Examples
///
/// ```
/// use pdfcer_core::vector::remap_index_after_delete;
///
/// // Deleting object 1 from a page of five.
/// assert_eq!(remap_index_after_delete(0, &[1]), Some(0)); // before the hole
/// assert_eq!(remap_index_after_delete(1, &[1]), None);    // it was the hole
/// assert_eq!(remap_index_after_delete(2, &[1]), Some(1)); // shifted down
/// assert_eq!(remap_index_after_delete(4, &[1, 3]), Some(2)); // two holes below
/// ```
#[must_use]
pub fn remap_index_after_delete(index: usize, deleted: &[usize]) -> Option<usize> {
    if deleted.contains(&index) {
        return None;
    }
    // Count DISTINCT deleted indices strictly below `index`. Distinct, because
    // `deleted` is caller-supplied and a duplicate would otherwise shift the
    // survivor twice — a plausible input (a shell unioning two selections)
    // producing a silently wrong answer, which is the whole failure class this
    // function exists to close.
    let mut below: Vec<usize> = deleted.iter().copied().filter(|&d| d < index).collect();
    below.sort_unstable();
    below.dedup();
    Some(index - below.len())
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
    use crate::vector::{Matrix, NoXObjects, decompose};

    /// Decompose a source content stream and return `(stream, first path)`.
    fn path_of(src: &[u8]) -> (ContentStream, PathObject) {
        let cs = ContentStream::parse(src.to_vec()).unwrap();
        let model = decompose(&cs, Matrix::IDENTITY, &NoXObjects);
        let path = model
            .objects
            .iter()
            .find_map(|o| match o {
                VectorObject::Path(p) => Some(p.clone()),
                _ => None,
            })
            .expect("a path object");
        (cs, path)
    }

    #[test]
    fn move_translates_every_construction_operand() {
        let (cs, path) = path_of(b"10 20 m 100 200 l 40 50 60 70 80 90 c S");
        let plan = plan_move(&cs, &path, 5.0, -3.0).unwrap();
        assert_eq!(plan.content, b"15 17 m 105 197 l 45 47 65 67 85 87 c S");
        assert_eq!(plan.operators_touched, 3); // m, l, c
    }

    #[test]
    fn move_shifts_re_origin_but_not_size() {
        let (cs, path) = path_of(b"10 10 80 40 re f");
        let plan = plan_move(&cs, &path, 100.0, 0.0).unwrap();
        // x,y move by 100; w,h unchanged.
        assert_eq!(plan.content, b"110 10 80 40 re f");
    }

    #[test]
    fn move_is_ctm_aware() {
        // Object drawn under a 2× scale: a page-space drag of (10,0) is a
        // user-space drag of (5,0), so the user-space operands shift by 5.
        let (cs, path) = path_of(b"2 0 0 2 0 0 cm 0 0 m 10 0 l S");
        let plan = plan_move(&cs, &path, 10.0, 0.0).unwrap();
        // Only the object's operators are rewritten; the `cm` stays verbatim.
        assert_eq!(plan.content, b"2 0 0 2 0 0 cm 5 0 m 15 0 l S");
    }

    #[test]
    fn move_refuses_a_singular_ctm() {
        // A CTM scaled flat to a line (determinant 0).
        let (cs, path) = path_of(b"1 0 0 0 0 0 cm 0 0 m 10 0 l S");
        assert_eq!(
            plan_move(&cs, &path, 1.0, 1.0),
            Err(VectorEditError::DegenerateCtm)
        );
    }

    // ---- Pass 36.1: node deletion ------------------------------------

    /// The ordinary case: an interior anchor's segment operator is excised and
    /// its neighbours join directly. Every other byte is verbatim.
    #[test]
    fn node_delete_excises_the_interior_segment_operator() {
        let (cs, path) = path_of(b"0 0 m 10 0 l 20 0 l 30 0 l S");
        let plan = plan_delete_node(&cs, &path, 2).unwrap();
        assert_eq!(plan.content, b"0 0 m 10 0 l 30 0 l S");
        assert_eq!(plan.operators_touched, 1);
        assert!(
            plan.disclosures.is_empty(),
            "a straight segment owes nothing"
        );
    }

    /// The terminal anchor is the same excision — nothing special about being
    /// last, which is worth pinning because the FIRST is special.
    #[test]
    fn node_delete_handles_the_last_anchor() {
        let (cs, path) = path_of(b"0 0 m 10 0 l 20 0 l S");
        let plan = plan_delete_node(&cs, &path, 2).unwrap();
        assert_eq!(plan.content, b"0 0 m 10 0 l S");
    }

    /// Deleting a subpath's FIRST anchor promotes the follower to the new `m`.
    #[test]
    fn node_delete_promotes_the_follower_to_the_new_start() {
        let (cs, path) = path_of(b"0 0 m 10 0 l 20 0 l S");
        let plan = plan_delete_node(&cs, &path, 0).unwrap();
        assert_eq!(plan.content, b"10 0 m 20 0 l S");
        assert_eq!(plan.operators_touched, 2);
    }

    /// Promotion across a CURVE keeps the endpoint and drops the control
    /// points — correct, and disclosed, because re-adding a point would not
    /// bring the curvature back.
    #[test]
    fn node_delete_promoting_a_curve_discloses_the_lost_curvature() {
        let (cs, path) = path_of(b"0 0 m 1 1 2 2 10 0 c 20 0 l S");
        let plan = plan_delete_node(&cs, &path, 0).unwrap();
        assert_eq!(plan.content, b"10 0 m 20 0 l S");
        assert_eq!(
            plan.disclosures.len(),
            1,
            "the discarded curve is disclosed"
        );
    }

    /// Excising an interior CURVE discloses it too — same obligation, other arm.
    /// This is the arm `is_curve_keyword` exists to keep in step.
    #[test]
    fn node_delete_of_an_interior_curve_discloses() {
        let (cs, path) = path_of(b"0 0 m 1 1 2 2 10 0 c 20 0 l S");
        let plan = plan_delete_node(&cs, &path, 1).unwrap();
        assert_eq!(plan.content, b"0 0 m 20 0 l S");
        assert_eq!(plan.disclosures.len(), 1);
    }

    /// A two-anchor subpath has no shorter form: refused BY NAME rather than
    /// silently promoted to a whole-part delete.
    #[test]
    fn node_delete_refuses_to_empty_a_subpath() {
        let (cs, path) = path_of(b"0 0 m 10 0 l S");
        assert_eq!(
            plan_delete_node(&cs, &path, 1),
            Err(VectorEditError::NodeDeleteWouldEmptySubpath {
                subpath: 0,
                remaining: 1,
            })
        );
    }

    /// An `re` corner is named by no operand, and the honest result would be a
    /// triangle. Refused by name.
    #[test]
    fn node_delete_refuses_a_rectangle_corner() {
        let (cs, path) = path_of(b"10 10 80 40 re f");
        assert_eq!(
            plan_delete_node(&cs, &path, 0),
            Err(VectorEditError::NodeDeleteRectangleCorner)
        );
    }

    /// A clipping path is refused for the same reason `plan_delete_subpath`
    /// refuses one: the visible change would be to OTHER content.
    #[test]
    fn node_delete_refuses_a_clipping_path() {
        let (cs, path) = path_of(b"0 0 m 10 0 l 20 0 l W n");
        assert_eq!(
            plan_delete_node(&cs, &path, 1),
            Err(VectorEditError::ClippingPath)
        );
    }

    /// An index past the object's anchor count reports the count, so a caller
    /// can say how many there actually are.
    #[test]
    fn node_delete_reports_an_out_of_range_index() {
        let (cs, path) = path_of(b"0 0 m 10 0 l 20 0 l S");
        assert_eq!(
            plan_delete_node(&cs, &path, 99),
            Err(VectorEditError::NodeOutOfRange {
                index: 99,
                count: 3,
            })
        );
    }

    /// Deleting inside ONE subpath leaves its siblings byte-verbatim — the
    /// property that makes this usable on a CAD export where a single object
    /// holds hundreds of parts.
    #[test]
    fn node_delete_leaves_sibling_subpaths_verbatim() {
        let (cs, path) = path_of(b"0 0 m 10 0 l 20 0 l 0 5 m 10 5 l 20 5 l S");
        let plan = plan_delete_node(&cs, &path, 4).unwrap();
        assert_eq!(plan.content, b"0 0 m 10 0 l 20 0 l 0 5 m 20 5 l S");
    }

    #[test]
    fn delete_removes_exactly_the_object_span() {
        let cs =
            ContentStream::parse(b"1 0 0 RG 10 20 m 100 200 l S 0 0 m 5 5 l S".to_vec()).unwrap();
        let model = decompose(&cs, Matrix::IDENTITY, &NoXObjects);
        // Delete the FIRST path (indices are paint order).
        let plan = plan_delete(&cs, &model.objects[0]).unwrap();
        // The `1 0 0 RG` state op and the second path survive; only the first
        // path's operators are gone.
        let text = String::from_utf8(plan.content).unwrap();
        assert!(text.contains("1 0 0 RG"), "preceding state op kept: {text}");
        assert!(!text.contains("100 200 l"), "first path removed: {text}");
        assert!(text.contains("5 5 l"), "second path kept: {text}");
    }

    #[test]
    fn node_drag_rewrites_one_anchor_only() {
        let (cs, path) = path_of(b"10 20 m 100 200 l 300 400 l S");
        // Node 0 = m start, 1 = first l, 2 = second l.
        let plan = plan_move_node(&cs, &path, 1, Point::new(120.0, 250.0)).unwrap();
        assert_eq!(plan.content, b"10 20 m 120 250 l 300 400 l S");
        // Node 0 moves the start.
        let (cs2, path2) = path_of(b"10 20 m 100 200 l S");
        let plan0 = plan_move_node(&cs2, &path2, 0, Point::new(0.0, 0.0)).unwrap();
        assert_eq!(plan0.content, b"0 0 m 100 200 l S");
    }

    #[test]
    fn node_drag_targets_the_curve_endpoint_not_a_handle() {
        // `c x1 y1 x2 y2 x3 y3`: the anchor is (x3,y3); the handles stay put.
        let (cs, path) = path_of(b"10 10 m 20 30 40 50 60 70 c S");
        // Node 1 is the `c` endpoint (60,70).
        let plan = plan_move_node(&cs, &path, 1, Point::new(99.0, 88.0)).unwrap();
        assert_eq!(plan.content, b"10 10 m 20 30 40 50 99 88 c S");
    }

    #[test]
    fn node_count_matches_the_decomposition() {
        let (cs, path) = path_of(b"10 20 m 100 200 l 300 400 l S");
        assert_eq!(anchor_count(&cs, &path), 3);
        // Equal to the flattened subpath anchors.
        let flat: usize = path.subpaths.iter().map(|s| s.anchors().count()).sum();
        assert_eq!(anchor_count(&cs, &path), flat);
    }

    #[test]
    fn node_drag_expands_a_rectangle_corner() {
        let (cs, path) = path_of(b"10 10 80 40 re f");
        // A rectangle has four anchors, all `re` corners.
        assert_eq!(anchor_count(&cs, &path), 4);
        let plan = plan_move_node(&cs, &path, 0, Point::new(0.0, 0.0)).unwrap();
        // The spec's own equivalence (Table 59) with corner 0 relocated. The
        // trailing `h` must survive: without it a stroked box gets caps, not
        // a corner join.
        assert_eq!(plan.content, b"0 0 m 90 10 l 90 50 l 10 50 l h f");
        assert_eq!(plan.disclosures.len(), 1);
    }

    #[test]
    fn node_drag_out_of_range_is_named() {
        let (cs, path) = path_of(b"10 20 m 100 200 l S");
        assert_eq!(
            plan_move_node(&cs, &path, 9, Point::new(0.0, 0.0)),
            Err(VectorEditError::NodeOutOfRange { index: 9, count: 2 })
        );
    }

    #[test]
    fn a_lone_move_contributes_no_anchor() {
        // `10 10 m` opens an empty subpath that the paint drops; the real
        // subpath starts at the second `m`. Anchor order must match.
        let (cs, path) = path_of(b"10 10 m 20 20 m 30 30 l S");
        // One committed subpath: start (20,20) + end (30,30) = 2 anchors.
        assert_eq!(anchor_count(&cs, &path), 2);
        assert_eq!(
            path.subpaths
                .iter()
                .map(|s| s.anchors().count())
                .sum::<usize>(),
            2
        );
    }

    #[test]
    fn degenerate_coordinates_do_not_panic() {
        let (cs, path) = path_of(b"0 0 m 10 10 l S");
        // A huge, non-finite-ish drag: the surgery must produce bytes, not panic.
        let _ = plan_move(&cs, &path, 1e308, -1e308);
        let _ = plan_move_node(&cs, &path, 1, Point::new(f64::MAX, f64::MIN));
    }
}
