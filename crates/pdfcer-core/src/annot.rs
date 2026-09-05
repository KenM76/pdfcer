//! # Annotation model — walk `/Annots`, model each annotation, select its
//! appearance (ISO 32000-1 §12.5)
//!
//! This is the **read/model half** of Pass 6.0 (docs/decisions/008,
//! `first_pass_scope`). It sits on decision 005's *"core decodes and
//! models, render paints"* axis (R26): this module walks a page's
//! `/Annots`, resolves each annotation dictionary, decodes its flags, and
//! **selects** the normal appearance stream a conforming reader would
//! paint — but it **never paints, never decides colour, and never
//! synthesises a look** (R43). `pdfcer-render`'s `annot` module consumes
//! [`Annotation`] and performs the §12.5.5 placement + painting.
//!
//! ## Scope — deliberately read-only (R43, decision 008 non-goals)
//!
//! Pass 6.0 introduces **no authoring capability**. This module has no
//! writer, produces no `/AP`, and synthesises nothing from `/MK`, `/IC`,
//! `/C`, `/DA`, `/L`, `/QuadPoints`, `/InkList`, or icon names. An
//! annotation without a usable appearance stream is **classified and
//! counted, not drawn** — that counter is the measured demand signal for
//! the later appearance-*generation* Passes (6.1/6.2/7).
//!
//! **That scope is unchanged by `Pass 38.5`, which is worth saying
//! because the Pass added four things here.** This module still only
//! reads;
//! [`EditSession::delete_annotation`](crate::edit::EditSession::delete_annotation)
//! does the writing and lives in `edit`. What arrived here is the model
//! that verb needs, and each piece is a key this file previously skipped
//! for being display-irrelevant:
//!
//! | Added | Clause | Why a DELETION verb needs it |
//! |---|---|---|
//! | [`Annotation::popup`] | §12.5.6.14, Table 170 | A pop-up *"shall not appear alone"*; deleting a markup annotation must take its window, or §12.5.6.2 NOTE 2 makes the orphan start displaying the deleted comment's own text. |
//! | [`Annotation::in_reply_to`] | §12.5.6.2, Table 170 | Who refers to the annotation being removed. |
//! | [`Annotation::reply_type`] + [`ReplyType`] | §12.5.6.2, Table 170 | `/RT /R` and `/RT /Group` are the SAME key with different deletion consequences — and Table 170's default is `R`, so an absent key is a reply. |
//! | [`AnnotFlags::LOCKED`] | §12.5.3, Table 165 bit 8 | *"do not allow the annotation to be deleted"* — the only clause in ISO 32000-1 that gates this operation. Explicitly **not** bit 10 `LockedContents`, which *"does not restrict deletion"*. |
//!
//! `LOCKED` is a deliberate exception to [`AnnotFlags`]'s own
//! display-flags-only rule, stated at the type. It must never gate
//! rendering.
//!
//! ## Spec sources (PDF-spec RAG, ISO 32000-1:2008)
//!
//! - `iso32000__s__12.5.2.md` — §12.5.1–.2, Table 164 (entries common to
//!   all annotation dictionaries): `/Subtype` (Required), `/Rect`
//!   (Required), `/F` flags, `/AP`, `/AS`. **`/Annots` is Optional and
//!   NOT inheritable** (a flat per-page array; §7.7.3.4 lists exactly
//!   four inheritable attributes and this is not one). A given annotation
//!   dictionary *"shall be referenced from the `Annots` array of only one
//!   page"*.
//! - `iso32000__s__12.5.3.md` — §12.5.3, Table 165 (the 10 annotation
//!   flags). Bit *N* has integer value `2^(N-1)`. Hidden (bit 2) and
//!   NoView (bit 6) are the display-suppression flags; the rest have no
//!   Pass-6.0 display consequence.
//! - `iso32000__s__12.5.5.md` — §12.5.5, Table 168 (the appearance
//!   dictionary `/AP`: `/N` normal, `/R` rollover, `/D` down). `/N` may be
//!   a single stream **or** a subdictionary keyed by appearance state,
//!   with `/AS` selecting. The placement algorithm (BBox→Matrix→Rect)
//!   lives in `pdfcer-render`; this module only *selects* the `/N` stream.
//! - `iso32000__s__12.5.6.md` — §12.5.6, per-subtype map. Every geometry
//!   subtype defines a fallback look AND says *"`/AP` takes precedence"*;
//!   R43 makes `/AP` the **only** thing pdfcer draws. `/Popup`
//!   (§12.5.6.14) *"shall have no appearance stream"* and is **never**
//!   painted as page content — a structural rule, stronger than R43.
//!
//! ## What this module deliberately does NOT model yet
//!
//! - **`/OC` optional content (§8.11)** is a GAP: the clause is not in the
//!   RAG and pdfcer implements no optional-content state anywhere (the
//!   content interpreter defers `BDC`/`EMC` marked content too). An
//!   annotation in an OFF optional-content group would be *"skipped as if
//!   not in the document"* by a full reader; pdfcer paints it (consistent
//!   with the rest of the renderer ignoring OC). Recorded as a known,
//!   consistent deferral rather than a silent divergence.
//! - **`/R` and `/D`** (rollover/down) are recognised but never selected —
//!   they are interaction states no static display drives (§12.5.5); this
//!   module models only `/N`. **(`Pass 38.5` note: the deletion verb
//!   nevertheless collects `/R` and `/D` streams when it removes an
//!   annotation. "Never selected for painting" and "never cleaned up" are
//!   different claims, and leaving them would orphan a stream in every
//!   subsequent save.)**
//! - **The pop-up's own `/Parent` back-reference** (Table 183) is not
//!   modelled, and [`Annotation::popup`] is. Deliberate, not an
//!   asymmetry to be tidied: the authoritative direction is the parent's
//!   `/Popup`, and finding a parent by scanning for whoever names the
//!   pop-up also copes with the malformed-but-real case of a `/Parent`
//!   that disagrees with its claimed parent's `/Popup`. Modelling both
//!   would mean two answers to one question.
//! - **Reply-thread RESOLUTION.** [`Annotation::in_reply_to`] is a link,
//!   not a walk: this module never follows it, never assembles a thread,
//!   and never applies §12.5.6.2's group-attribute rule (a subordinate's
//!   `Contents`/`T`/`M`/`C` *"shall be ignored"* in favour of its
//!   primary's). Applying it here would make [`Annotation::contents`]
//!   disagree with the dictionary it came from — see that field's own
//!   note. A consumer that wants the resolved view builds it from the
//!   links.

use std::collections::{BTreeMap, BTreeSet};

use crate::graph::ObjectGraph;
use crate::object::{Dict, Name, ObjId, Object};
use crate::outline::{Destination, DestinationReader};
use crate::page_tree::Rect;
use crate::settings::MissingAppearanceState;

/// Maximum annotations modelled from one page's `/Annots` array
/// (pdfcer policy, ARCHITECTURE.md §10.1 adversarial-input posture).
///
/// **No spec limit exists to inherit.** Annex C (informative) lists no
/// annotation-count bound, and PDF/A §6.1.12 positively forbids a reader
/// from imposing Annex C's implementation limits — so this is pure pdfcer
/// policy and must clear any conformant corpus. It bounds only the linear
/// allocation a hostile `/Annots` array (millions of tiny dictionaries)
/// could pin; a page carrying more than this many real annotations is
/// beyond any measured document. Chosen far above the corpus maximum
/// (see `tools/annot-corpus-check.py`) so the veraPDF §6.1.12
/// implementation-limits suite reports comfortable headroom, in the same
/// spirit as [`crate::page_tree::MAX_PAGES`].
pub const MAX_ANNOTS_PER_PAGE: usize = 1_000_000;

/// Decoded `/F` annotation flags (ISO 32000-1 §12.5.3, Table 165).
///
/// Bit positions are numbered from the low-order bit as **bit 1**, so
/// bit *N* has integer value `2^(N-1)` (§12.5.3 verbatim). Getting this
/// off by one silently mis-reads every flag, so the bit constants below
/// are named against Table 165 and pinned by a test. Default `/F` is `0`
/// (no flags; Table 164).
///
/// Only the display-relevant flags get accessors here — Pass 6.0 is a
/// display Pass. ReadOnly/Locked/ToggleNoView/LockedContents have **no
/// display consequence** (they govern interaction/editing) and are
/// deliberately not surfaced, so they cannot accidentally gate rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AnnotFlags(pub u32);

impl AnnotFlags {
    /// Bit 1 (value 1) — Invisible: suppress an *unknown* subtype that
    /// has no handler and no `/AP`. A near-noop for pdfcer (R43 paints
    /// strictly from `/AP`), so an unknown subtype with an `/AP` is
    /// painted regardless.
    pub const INVISIBLE: u32 = 1 << 0;
    /// Bit 2 (value 2) — Hidden: *"do not display or print … regardless
    /// of annotation type or handler."* The strongest suppression —
    /// gone from both screen and print.
    pub const HIDDEN: u32 = 1 << 1;
    /// Bit 3 (value 4) — Print: print when the page is printed; clear
    /// means a screen-only annotation. No on-screen consequence.
    pub const PRINT: u32 = 1 << 2;
    /// Bit 4 (value 8) — NoZoom: do not scale the appearance to the page
    /// magnification. Feeds the §12.5.5 post-placement transform.
    pub const NO_ZOOM: u32 = 1 << 3;
    /// Bit 5 (value 16) — NoRotate: do not rotate the appearance to the
    /// page rotation. Feeds the §12.5.5 post-placement transform.
    pub const NO_ROTATE: u32 = 1 << 4;
    /// Bit 6 (value 32) — NoView: suppress on **screen** but allow
    /// **print** (if Print is set). The inverse of a screen-only
    /// annotation, and a document-forensics vector when paired with
    /// Print.
    pub const NO_VIEW: u32 = 1 << 5;
    /// Bit 8 (value 128) — **Locked**: *"If set, do not allow the
    /// annotation to be **deleted** or its properties (including position
    /// and size) to be modified by the user."*
    ///
    /// # The only editing gate the standard puts on an annotation
    ///
    /// Added by `Pass 38.5`, and it is the exception to this type's own
    /// "display flags only" scope note directly above — deliberately, and
    /// with the reason: `Locked` is the one Table 165 bit that constrains
    /// an operation pdfcer performs. Every other non-display flag governs
    /// interaction pdfcer has no surface for.
    ///
    /// **It has NO display consequence and must never gate rendering.**
    /// A locked annotation paints exactly like an unlocked one; the flag
    /// is about who may change it.
    ///
    /// **Not to be confused with bit 10, `LockedContents`**, whose own
    /// Table 165 text says it *"does not restrict deletion"* — it locks
    /// the *contents* of the annotation, not its existence. Treating the
    /// two as one would refuse deletions the standard permits; treating
    /// `Locked` as cosmetic would perform deletions it forbids. Hence a
    /// named constant for exactly one of them.
    pub const LOCKED: u32 = 1 << 7;
    /// Table 165 bit 10, `LockedContents` (PDF 1.7): *"do not allow the
    /// contents of the annotation to be modified by the user"*. The
    /// standard's 1-based, low-order-first bit numbering makes bit 10
    /// the value **512** (`1 << 9`) — not 1024, which a reader counting
    /// from bit 1 = value 1 and doubling *ten* times arrives at by
    /// mistake.
    ///
    /// **This is the flag reshape does NOT check** (`Pass 255.0`). A
    /// vertex move, insert or remove changes the annotation's
    /// *position/size*, which is `Locked`'s domain; `LockedContents`
    /// guards the *contents* — the `/Contents` / `/RC` text — and its
    /// own Table 165 row says it *"does not restrict deletion or
    /// modification of other annotation properties"*. Reading it as a
    /// blanket lock would refuse a reshape the standard permits; ignoring
    /// `Locked` would perform one it forbids. Two flags, two gates.
    pub const LOCKED_CONTENTS: u32 = 1 << 9;

    /// Whether the Hidden flag (Table 165 bit 2) is set.
    #[must_use]
    pub const fn hidden(self) -> bool {
        self.0 & Self::HIDDEN != 0
    }

    /// Whether the NoView flag (Table 165 bit 6) is set.
    #[must_use]
    pub const fn no_view(self) -> bool {
        self.0 & Self::NO_VIEW != 0
    }

    /// Whether the Print flag (Table 165 bit 3) is set (for the future
    /// print path; no screen consequence).
    #[must_use]
    pub const fn print(self) -> bool {
        self.0 & Self::PRINT != 0
    }

    /// Whether the Invisible flag (Table 165 bit 1) is set.
    #[must_use]
    pub const fn invisible(self) -> bool {
        self.0 & Self::INVISIBLE != 0
    }

    /// Whether the NoZoom flag (Table 165 bit 4) is set.
    #[must_use]
    pub const fn no_zoom(self) -> bool {
        self.0 & Self::NO_ZOOM != 0
    }

    /// Whether the NoRotate flag (Table 165 bit 5) is set.
    #[must_use]
    pub const fn no_rotate(self) -> bool {
        self.0 & Self::NO_ROTATE != 0
    }

    /// Whether the **Locked** flag (Table 165 bit 8) is set — the
    /// annotation may not be deleted, moved or resized by the user.
    ///
    /// See [`Self::LOCKED`] for why this one non-display flag has an
    /// accessor when the other three do not, and for the
    /// `LockedContents` trap.
    #[must_use]
    pub const fn locked(self) -> bool {
        self.0 & Self::LOCKED != 0
    }

    /// Whether the LockedContents flag (Table 165 bit 10, value 512) is
    /// set. See [`Self::LOCKED_CONTENTS`] for why it is a separate gate
    /// from [`Self::locked`] and why geometry edits do not consult it.
    #[must_use]
    pub const fn locked_contents(self) -> bool {
        self.0 & Self::LOCKED_CONTENTS != 0
    }

    /// Whether this annotation is suppressed from **on-screen** display,
    /// i.e. Hidden **or** NoView (§12.5.3, Table 165).
    ///
    /// This is the render path's screen-suppression predicate. Per R50 a
    /// suppressed annotation is *honoured AND counted* — never silently
    /// dropped — because *"a page carrying content the operator cannot
    /// see is a fact they are entitled to know"* (hidden annotations are
    /// a recognised document-forensics vector).
    #[must_use]
    pub const fn suppressed_on_screen(self) -> bool {
        self.hidden() || self.no_view()
    }
}

/// The outcome of §12.5.5 **normal-appearance** (`/AP` `/N`) selection for
/// one annotation.
///
/// Core *selects*; `pdfcer-render` *places and paints*. The variants are
/// the full negative-result taxonomy the §12.5.5 RAG enumerates, because
/// under R43 *how* an annotation fails to yield an appearance is exactly
/// the diagnostic the operator is entitled to (R20/R27) and the demand
/// signal the later generation Passes measure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Appearance {
    /// A normal appearance stream (a form XObject, §8.10) resolved and
    /// ready to place. `stream_id` is its object identity — present in
    /// every well-formed file because *"all streams shall be indirect
    /// objects"* (§7.3.8.1), and it is the §8.10 cycle-guard key the
    /// render path needs. `None` only in the pathological case of a
    /// stream reached without an indirect reference.
    Normal {
        /// Identity of the appearance form XObject (the cycle-guard key).
        stream_id: Option<ObjId>,
    },
    /// No usable normal appearance: `/AP` is absent, `/AP` is not a
    /// dictionary, `/N` is absent, `/N` resolves to null (dangling), or
    /// `/N` is neither a stream nor a subdictionary. Under R43 this is
    /// **named-not-painted, counted by subtype** — never synthesised.
    None,
    /// `/N` is an appearance-state **subdictionary** but the state could
    /// not be selected: `/AS` is missing against a multi-entry
    /// subdictionary, or `/AS` names a state the subdictionary does not
    /// define (§12.5.5 NOTE 3: *"reasonable behaviour such as displaying
    /// nothing"*). pdfcer displays nothing and **does not guess** a
    /// first/`On`/`Off` key (the RAG's explicit negative result — real
    /// readers vary, so guessing would show a state no other reader
    /// picks). Counted separately from [`Appearance::None`] because the
    /// annotation *does* carry appearances; only selection failed.
    StateUnresolved,
}

/// One page annotation, modelled read-only (ISO 32000-1 §12.5, Table 164).
///
/// Carries exactly what the render path needs to place-and-paint plus what
/// the diagnostics need to count, plus — since `Pass 255.0` — the
/// **point geometry a shell needs to draw reshape anchors from**
/// ([`Self::vertices`], [`Self::line`], [`Self::ink_list`]). It is still
/// **not** a faithful echo of the annotation dictionary: `/QuadPoints`,
/// `/IC`, `/MK` and icon `/Name` are deliberately *not* modelled (under
/// R43 they are neither painted nor generated from here).
///
/// # Why the point geometry joined the model (`Pass 255.0`)
///
/// `pdfcer-gui` reported (2026-09-05) that a polygon's own shape was not
/// a fact `pdfcer-core` exposed about a polygon: `add_markup` could author
/// a thirty-click cloud whose vertices could never be read back, so a
/// shell could not draw a single anchor to drag — the capability was not
/// merely unavailable, it was invisible. Re-parsing `/Vertices` in the
/// shell was refused (decision 058: a second geometry implementation
/// drifts), so the engine owns the read. The three fields are read for
/// **every** subtype that carries the key, on the same reasoning as `/T`:
/// an absent key is `None`, which is the truth, and a subtype-gated reader
/// would make the model silently disagree with a file that carries the
/// key anyway.
#[derive(Debug, Clone, PartialEq)]
pub struct Annotation {
    /// The annotation object's identity, if it was reached by an indirect
    /// reference from `/Annots` (it always is in a well-formed file —
    /// Table 164's dictionaries are indirect objects). Used only for
    /// diagnostics/dedup; the render path does not need it.
    pub id: Option<ObjId>,
    /// The `/Subtype` name bytes (Table 164, Required). Empty when the
    /// entry is absent — a malformed annotation, surfaced not repaired.
    pub subtype: Vec<u8>,
    /// The `/Rect` in default user space, normalised per §7.9.5 (corners
    /// may be given in either order). `None` when `/Rect` is absent or
    /// malformed — the §12.5.5 placement target is then missing and the
    /// render path refuses placement by name.
    pub rect: Option<Rect>,
    /// Decoded `/F` flags (§12.5.3, Table 165). Default `0`.
    pub flags: AnnotFlags,
    /// `/Vertices` — the ordered vertex list of a `/Polygon` or
    /// `/PolyLine` (ISO 32000-1 §12.5.6.9, Table 178), as `(x, y)` pairs
    /// in default user space. `None` when the key is absent or is not an
    /// array; an odd trailing coordinate is dropped rather than invented.
    ///
    /// A revision cloud is a `/Polygon` with `/BE << /S /C >>`, so its
    /// vertices are the **pre-bulge** polygon — the scallops are baked
    /// into `/AP` from these, and `/Rect` bounds the bulged outline
    /// (there is no `/RD` on a Polygon to record the difference). A shell
    /// drawing anchors draws them here, not on the cloud's outline.
    ///
    /// Reshaped through [`crate::edit::EditSession::reshape_annotation`]
    /// and its three vertex wrappers.
    pub vertices: Option<Vec<(f64, f64)>>,
    /// `/L` — the two endpoints of a `/Line` (§12.5.6.7, Table 175),
    /// `[start, end]`. `None` when absent or not exactly four numbers.
    ///
    /// A ce dimension (rule 15) is also a `/Line` and carries `/L`, so
    /// this is populated for it too — but its geometry is edited through
    /// the dimension verbs, and the annotation reshape verbs refuse it by
    /// name.
    pub line: Option<[(f64, f64); 2]>,
    /// `/InkList` — the strokes of an `/Ink` annotation (§12.5.6.13,
    /// Table 182), one inner vector per stroke. `None` when absent or not
    /// an array; a stroke that is not itself an array of numbers reads as
    /// empty rather than being skipped, so stroke indices stay aligned
    /// with the file's.
    ///
    /// **Read-only geometry by design.** pdfcer refuses per-point ink
    /// editing by name (Acrobat has never offered it at any version —
    /// whole-stroke move/resize/delete only); the field exists so a shell
    /// can *show* the strokes, and so `move_annotation` /
    /// `resize_annotation` have something to report about.
    pub ink_list: Option<Vec<Vec<(f64, f64)>>>,
    /// `/CA` — the annotation's **constant opacity** (§12.5.2, Table 164),
    /// `0.0`–`1.0`. `None` when the key is absent, which §12.5.2 defines as
    /// fully opaque.
    ///
    /// # Why `Option` rather than defaulting to `1.0` here
    ///
    /// "Absent" and "explicitly 1.0" are different facts about the file, and
    /// collapsing them here would make a writer unable to round-trip the
    /// difference. The *render* default is 1.0; the *model* keeps what the
    /// document said.
    ///
    /// # It applies to the annotation AS COMPOSITED, not inside its appearance
    ///
    /// §12.5.2: the value is the constant opacity used when painting the
    /// annotation onto the page. An appearance stream that also sets `/ca` in
    /// its own `ExtGState` therefore **compounds** with this — 0.5 twice reads
    /// as 0.25. That is why pdfcer's markup writer sets `/CA` alone and leaves
    /// the appearance's graphics state at 1.0.
    pub constant_alpha: Option<f64>,
    /// The selected normal (`/N`) appearance, per §12.5.5.
    pub appearance: Appearance,
    /// Whether `/Subtype` is `Popup` (§12.5.6.14). A `/Popup` is a reader
    /// UI window, **never** page content — a structural non-paint rule
    /// stronger than R43, checked before flags or appearance (risk X4).
    pub is_popup: bool,
    /// `/Contents` — the annotation's text, decoded per §7.9.2 (Table 164,
    /// Optional, PDF 1.0). `None` when the key is absent.
    ///
    /// # It is DUAL-PURPOSE, and a consumer must not assume "comment"
    ///
    /// §12.5.2: this is *"text displayed for the annotation, **or** (if the
    /// type does not display text) an alternate human-readable description"*
    /// for accessibility (§14.9.3). Which one it is depends on the subtype
    /// (§12.5.6.2): a `FreeText` DISPLAYS it, most markup types put it in the
    /// pop-up, and `Link`/`Movie`/`Widget`/`PrinterMark`/`TrapNet` use it
    /// purely as an accessibility alternate. So a UI labelling this "comment"
    /// is right for markup and wrong for a Link — modelled here without that
    /// interpretation, which belongs to whoever displays it.
    ///
    /// **Not resolved here:** §12.5.6.2 NOTE 2 says a markup annotation with
    /// a parent (`/IRT` reply) has its own `Contents` "shall be ignored".
    /// The `/IRT` link IS now modelled ([`Self::in_reply_to`],
    /// [`Self::reply_type`], added by Pass 38.5 so annotation *deletion*
    /// could reason about reply chains) — but the rule is still deliberately
    /// **not applied here**: this field stays the raw value the dictionary
    /// carries, and any consumer that wants the group-attribute resolution
    /// walks the link itself. Silently substituting a primary's `/Contents`
    /// for a subordinate's would make the model disagree with the file,
    /// which is the one thing `pdfcer-core`'s read half must never do.
    pub contents: Option<String>,
    /// `/T` — the annotation's title, conventionally the AUTHOR (Table 170,
    /// markup annotations only). `None` when absent.
    ///
    /// # Table 170, NOT Table 164 — this is not a common key
    ///
    /// `/T` is a **markup-annotation** key (§12.5.6), so it is legitimately
    /// absent on a `Link`, a `Widget` or a `PrinterMark`. Reading it here for
    /// every subtype is deliberate and harmless — an absent key is `None`,
    /// which is exactly the truth — but a consumer must not read `None` as
    /// "anonymous"; on a non-markup annotation it means "this subtype has no
    /// such concept".
    pub title: Option<String>,
    /// `/M` — the modification date, **raw and unparsed** (Table 164,
    /// Optional, PDF 1.1). `None` when absent.
    ///
    /// # Stored raw because the standard requires accepting anything
    ///
    /// §12.5.2 gives its type as "date **or text string**" and says a
    /// conforming reader *"shall accept and display a string in any format"*.
    /// Parsing to a date type would therefore have to either reject or
    /// silently mangle values the standard explicitly requires be accepted —
    /// so this is a `String`, and any future sort-by-date feature owns the
    /// decision about what to do with a value that is not a §7.9.4 date.
    pub mod_date: Option<String>,
    /// The `/OC` optional-content group/membership reference (§8.11.3.3), if
    /// the annotation carries one. Its default visibility is resolved against
    /// the catalog `/OCProperties /D` config: the annotation is visible only
    /// if the flags permit AND its OCG is ON (Pass 12.M2 authored-layer `/OC`
    /// honouring — decision 011 §2.4; full content-stream BDC/EMC `/OC` stays
    /// deferred). `None` when the annotation is on no layer.
    pub oc: Option<ObjId>,
    /// `/Popup` — this markup annotation's pop-up window companion
    /// (Table 170, Optional, PDF 1.3). `None` when the annotation has no
    /// pop-up, which includes every non-markup subtype.
    ///
    /// # Why the READ half models it: the popup cannot outlive its parent
    ///
    /// §12.5.6.14 is a `shall`: a pop-up annotation *"**shall not appear
    /// alone** but is associated with a markup annotation, its parent
    /// annotation"*. So this is not a decorative back-link — it is a
    /// **structural dependency**, and any operation that removes the
    /// parent must remove the pop-up in the same breath or leave the
    /// document violating that clause. Modelling it here rather than
    /// re-reading the dictionary at each call site is what lets
    /// [`crate::edit::EditSession::delete_annotation`] and any future
    /// mover/copier agree about the pair.
    ///
    /// The reference is surfaced **unresolved and unvalidated**: a
    /// `/Popup` pointing at a missing object, or at something that is not
    /// a `/Popup` annotation, is modelled as-is. Repairing it here would
    /// be the read half inventing structure (R27).
    pub popup: Option<ObjId>,
    /// `/IRT` — the annotation this one is *in reply to* (Table 170,
    /// Optional, PDF 1.5). `None` for an annotation that is not part of a
    /// thread or a group.
    ///
    /// # A reference with TWO meanings, disambiguated by [`Self::reply_type`]
    ///
    /// The same key builds two different structures (§12.5.6.2), and they
    /// behave differently under deletion, which is why both are modelled
    /// and neither is collapsed into the other:
    ///
    /// - **`/RT /R`** (a *reply*, and the default): an ordinary threaded
    ///   comment. It keeps its own author and text and is readable on its
    ///   own; losing its target costs the thread's shape, not its content.
    /// - **`/RT /Group`**: the target is the group **primary**, and
    ///   §12.5.6.2 says the subordinate's own `Contents`/`RC`+`DS`, `M`,
    ///   `C`, `T`, `Popup`, `CreationDate`, `Subj` and `Open` *"shall be
    ///   ignored"* in favour of the primary's. A subordinate whose primary
    ///   is gone therefore carries text a conforming reader is instructed
    ///   not to use.
    ///
    /// Surfaced unresolved, same as [`Self::popup`]: a dangling `/IRT` is
    /// modelled, not repaired.
    pub in_reply_to: Option<ObjId>,
    /// `/RT` — how [`Self::in_reply_to`] should be read (Table 170,
    /// Optional, PDF 1.6).
    ///
    /// `None` when the key is absent. **`None` is NOT the same as
    /// [`ReplyType::Reply`]** at the model level even though Table 170's
    /// default value is `R`: the absent case is a document fact this
    /// struct reports, and a consumer that wants the default applies it
    /// with [`Self::effective_reply_type`] rather than being unable to
    /// tell "the file said `R`" from "the file said nothing".
    pub reply_type: Option<ReplyType>,
    /// `/A` — the **action performed when this annotation is activated**
    /// (§12.5.2 Table 164), as its `/S` type name. `None` when the
    /// annotation carries no `/A`.
    ///
    /// # Why the read model carries it, when R43 keeps most of Table 164 out
    ///
    /// Everything else pdfcer declines to model here is *cosmetic* — `/MK`'s
    /// colours, `/BE`'s border effect — and the argument for leaving it out
    /// is that nothing consumes it. `/A` is the opposite: it is the only
    /// entry in the annotation dictionary that says **what happens to the
    /// operator**, and until `Pass 133.0` it was the one thing
    /// `list-annotations` could not tell them. A widget that submits a form
    /// to a web server printed identically to a widget that does nothing.
    ///
    /// # The `/S` NAME only, deliberately — not the action dictionary
    ///
    /// The type is the whole disclosure: *`SubmitForm`* answers "what does
    /// this do to me", and the action's own parameters (`/F`, `/URI`,
    /// `/Flags`) do not change the answer. Modelling them would mean
    /// modelling twelve action types' worth of dictionaries into a reader
    /// whose one consumer prints a token — and would put a URL into a
    /// structure that gets logged, which is a different decision needing its
    /// own reason.
    ///
    /// **This does not follow `/Next`.** A chain is a document-level property
    /// and is counted by [`crate::forms::scan_javascript`]; making a
    /// per-annotation field mean "this one plus everything it leads to" would
    /// give one name to a set. What it does instead is say that the chain is
    /// there — see [`Self::action_chains`].
    pub action_type: Option<Vec<u8>>,
    /// Whether this annotation's `/A` carries a **`/Next`** (§12.6.1) — that
    /// is, whether activating it performs more than the one action
    /// [`Self::action_type`] names.
    ///
    /// # ★ Why one bool, and why it is not optional polish
    ///
    /// Without it, `action_type` is a disclosure that can MISLEAD, which is
    /// worse than one that is absent. The worked case is in this project's
    /// own action fixture: a link whose `/A` is `/S /GoTo` — utterly benign,
    /// the most ordinary thing in a PDF — with a `/SubmitForm` hanging off
    /// its `/Next`. Reported as `action=GoTo` and nothing else, an operator
    /// reads *"this link goes to a page"* and is wrong.
    ///
    /// That is the same *"a check that under-reports reads as a clean bill of
    /// health"* shape as the `/AA`-only scan this field was added alongside,
    /// so fixing one while shipping the other would have been no fix at all.
    ///
    /// A bool rather than the chain's contents, deliberately: this says
    /// **"there is more here than the name above"** and sends the reader to
    /// [`crate::forms::scan_javascript`] for what it is. Summarising a whole
    /// chain into one annotation's line is the set-of-names problem again.
    pub action_chains: bool,
}

/// `/RT` — the relationship [`Annotation::in_reply_to`] expresses
/// (ISO 32000-1 §12.5.6.2, Table 170, PDF 1.6).
///
/// Two values are defined by the standard; anything else is preserved
/// rather than coerced, because a name pdfcer does not recognise is a
/// document fact and flattening it to the default would make the model
/// claim the file said something it did not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplyType {
    /// `/R` — a threaded **reply**. Table 170's default value.
    Reply,
    /// `/Group` — this annotation is a **subordinate** of the primary named
    /// by `/IRT`, and §12.5.6.2's group-attribute rule applies: its own
    /// `Contents` (or `RC`+`DS`), `M`, `C`, `T`, `Popup`, `CreationDate`,
    /// `Subj` and `Open` *"shall be ignored"* in favour of the primary's.
    Group,
    /// A `/RT` name that is neither `/R` nor `/Group`. Carried verbatim.
    Other(Vec<u8>),
}

impl Annotation {
    /// Whether this annotation is a form-field widget (`/Subtype`
    /// `Widget`, §12.5.6.19). A widget *is* an annotation first (R49);
    /// this is a census convenience — 87.8 % of organic annotations are
    /// widgets, so the count is a load-bearing demand signal.
    #[must_use]
    pub fn is_widget(&self) -> bool {
        self.subtype == b"Widget"
    }

    /// Whether this annotation is a `/RT /Group` **subordinate** — it has
    /// an `/IRT` target and that link is a grouping link, not a reply link
    /// (§12.5.6.2).
    ///
    /// The distinction matters to anything that removes or moves the
    /// primary: a subordinate's own `Contents`/`T`/`M`/`C`/`Popup`/
    /// `CreationDate`/`Subj`/`Open` *"shall be ignored"* in favour of the
    /// primary's, so losing the primary costs a subordinate its readable
    /// identity in a way losing a reply's target does not.
    ///
    /// Note the `&&`: an `/RT /Group` with **no** `/IRT` is not a
    /// subordinate of anything. Table 170 makes `/RT` meaningful only
    /// alongside `/IRT`, so the pair is checked, never `/RT` alone.
    #[must_use]
    pub fn is_group_subordinate(&self) -> bool {
        self.in_reply_to.is_some() && self.effective_reply_type() == Some(ReplyType::Group)
    }

    /// [`Self::reply_type`] with Table 170's **default value `R`** applied
    /// when the key is absent but `/IRT` is present.
    ///
    /// Returns `None` only when there is no `/IRT` at all — because `/RT`
    /// is meaningful only alongside `/IRT`, so "what relationship is this"
    /// has no answer for an annotation that is in reply to nothing.
    ///
    /// This exists so callers stop re-deriving the default. Table 170's
    /// defaults are permissive (§12.8.2.2's `/P` is the same trap), and a
    /// call site that treats an absent `/RT` as "not a reply" is wrong in
    /// the ordinary case: the ordinary case is a threaded comment that
    /// simply relies on the default.
    #[must_use]
    pub fn effective_reply_type(&self) -> Option<ReplyType> {
        self.in_reply_to?;
        Some(self.reply_type.clone().unwrap_or(ReplyType::Reply))
    }

    /// A stable, human/diagnostic label for the subtype: the `/Subtype`
    /// name bytes decoded lossily, or `"(no Subtype)"` when absent. Used
    /// as the by-subtype key of the `annotations_without_ap` counter, so
    /// it must be deterministic (it is — a pure function of the bytes).
    #[must_use]
    pub fn subtype_label(&self) -> String {
        if self.subtype.is_empty() {
            "(no Subtype)".to_owned()
        } else {
            String::from_utf8_lossy(&self.subtype).into_owned()
        }
    }

    /// Where this annotation goes when it is activated — the fully
    /// resolved [`Destination`] behind its `/Dest` or `/A` (§12.5.6.5
    /// Table 173 for a `/Link`; Table 188's `/A` for a `/Widget`).
    ///
    /// ## Why this is not a field on [`Annotation`]
    ///
    /// [`Self::action_type`] carries the action's `/S` name and nothing
    /// else, deliberately — that is the whole disclosure a *listing*
    /// needs, and it costs nothing to read. Resolving where the action
    /// *points* is a different and far more expensive question: it needs
    /// the page-object map and both named-destination namespaces, each
    /// **O(document)** to build. Putting it in the model would make
    /// every [`page_annotations`] call on every page pay for a walk that
    /// almost no caller wants.
    ///
    /// So the cost is moved to a [`DestinationReader`] the caller builds
    /// **once per document** and hands in here. See that type for why it
    /// is a snapshot and when it must be rebuilt.
    ///
    /// ## ★ It needs [`Self::id`], and one shape of file has none
    ///
    /// This re-reads the annotation's dictionary through `graph`, which
    /// requires the annotation to have been reached by an **indirect
    /// reference** from `/Annots`. Table 164's dictionaries are indirect
    /// objects, so that is the case in every well-formed file — but a
    /// dictionary written *directly* into the `/Annots` array is
    /// tolerated by [`page_annotations`] and arrives here with
    /// [`Self::id`] `None`, and this returns `None` for it.
    ///
    /// **That is a `None` meaning "could not read", sitting in the same
    /// slot as a `None` meaning "carries no destination".** The two are
    /// not distinguishable through this method, which is precisely why
    /// [`page_link_destinations`] exists: it walks `/Annots` and resolves
    /// each dictionary in place, never consulting `id`, and reports the
    /// unresolvable ones in
    /// [`PageLinks::links_without_destination`] rather than dropping
    /// them. Prefer it for anything that must be complete; use this one
    /// for the single annotation an operator just clicked, where the
    /// annotation demonstrably came from a real object.
    ///
    /// # Returns
    ///
    /// See [`DestinationReader::destination`] for the full variant list
    /// and what a viewer should do with each. In short: only
    /// [`Destination::Page`] is directly navigable; the other four are
    /// disclosures, not jumps, and must not be collapsed into "no link".
    #[must_use]
    pub fn destination<G: ObjectGraph + ?Sized>(
        &self,
        graph: &G,
        reader: &DestinationReader,
    ) -> Option<Destination> {
        let dict = graph.resolved(self.id?);
        reader.destination(graph, dict.as_dict()?)
    }
}

/// Walk one page's `/Annots` array and model every annotation on it
/// (ISO 32000-1 §12.5.2).
///
/// `page_id` is the page object's identity (from
/// [`crate::page_tree::Page::id`]). `/Annots` is read off the page
/// dictionary directly — it is **not inheritable** (§7.7.3.4), so there is
/// no page-tree walk: a page with no `/Annots` has no annotations, full
/// stop.
///
/// Generic over [`ObjectGraph`] so it works over both the loaded
/// [`Document`](crate::document::Document) and an
/// [`EditSession`](crate::edit::EditSession) overlay, exactly like
/// [`crate::page_tree::pages_in`]. Every malformed shape is tolerated by
/// skipping and modelling what is there — never a panic, never an abort
/// (the crate's adversarial-input policy):
///
/// - `/Annots` absent, null, or not an array → no annotations.
/// - An array entry that is not a dictionary (a null from a dangling
///   reference, a stray number) → skipped.
/// - `/Annots` may be a **shared indirect array** referenced by more than
///   one page (malformed per *"referenced from only one page"*, but seen
///   in the wild). This read-only walk simply reads it for each page; it
///   never mutates, so sharing is harmless here (the copy-on-write concern
///   is a Pass 6.1 authoring problem, risk X7).
///
/// The result is bounded by [`MAX_ANNOTS_PER_PAGE`].
#[must_use]
pub fn page_annotations<G: ObjectGraph + ?Sized>(graph: &G, page_id: ObjId) -> Vec<Annotation> {
    page_annotations_with(graph, page_id, MissingAppearanceState::default())
}

/// [`page_annotations`] with an explicit `AS-A1` policy (R169).
///
/// ## What `missing_as` decides, and what it does not
///
/// **Only** the malformed configuration §12.5.5 leaves undefined: an
/// `/AP` `/N` subdictionary of **two or more** entries with **no `/AS`**.
/// Table 164 makes `/AS` *required* there, and NOTE 3 covers only the
/// neighbouring case (`/AS` present, naming an absent state), so the
/// standard states no recovery at all. Every other path through
/// [`select_normal_appearance`] is spec-determined and this parameter
/// cannot reach it — a `/N` stream still wins outright, a present `/AS`
/// still selects, an absent named state is still
/// [`Appearance::StateUnresolved`], and a **single**-entry subdictionary
/// with no `/AS` is still painted (there are no alternatives to choose
/// between, so painting it is not a guess).
///
/// The default is [`MissingAppearanceState::PaintNothing`] — the shipped
/// behaviour, **evidence tier (d)**, a reasoned guess and deliberately the
/// conservative one. The spec RAG's row is explicit that "paint the first"
/// and "paint `/Off`" are *empirical* guesses belonging to
/// `C:\personal_rag\pdf\`, and installing one as the default would put a
/// plausible appearance on screen with nothing to say pdfcer chose it.
///
/// ## A separate function rather than a changed signature
///
/// [`page_annotations`] has callers in `pdfce-gui`, `pdfcer` and four
/// test crates, none of which have an opinion about this. Following the
/// crate's existing `*_with` convention (`pageops::extract_with`,
/// `EditSession::delete_pages_with`) keeps the policy explicit at the one
/// call site that carries the operator's setting — the renderer — and
/// keeps it out of the way everywhere else.
#[must_use]
pub fn page_annotations_with<G: ObjectGraph + ?Sized>(
    graph: &G,
    page_id: ObjId,
    missing_as: MissingAppearanceState,
) -> Vec<Annotation> {
    let page = graph.resolved(page_id);
    let Some(page_dict) = page.as_dict() else {
        return Vec::new();
    };
    let Some(annots_obj) = page_dict.get(b"Annots") else {
        return Vec::new();
    };
    let Some(array) = graph.resolve(annots_obj).as_array() else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for entry in array {
        if out.len() >= MAX_ANNOTS_PER_PAGE {
            break;
        }
        let id = entry.as_reference();
        let Some(dict) = graph.resolve(entry).as_dict() else {
            // A null (dangling reference) or non-dictionary entry is not
            // an annotation. §7.3.10 makes a dangling reference null, not
            // an error; skip it.
            continue;
        };
        out.push(model_annotation(graph, id, dict, missing_as));
    }
    out
}

/// One `/Link` annotation that has somewhere to go: its clickable box
/// and its fully resolved destination, together.
///
/// The pair is the point. Hit-testing needs the [`rect`](Self::rect) and
/// navigating needs the [`destination`](Self::destination), and a viewer
/// that had to correlate two separately-ordered lists to get both would
/// have an off-by-one waiting in it.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct LinkDestination {
    /// This link's 0-based position in the page's `/Annots` array — the
    /// same `index=` the CLI's `list-annotations` prints, and the same
    /// numbering `delete-annotation` takes.
    ///
    /// **Not** an index into the [`Vec`] this arrives in: entries with no
    /// destination are omitted, so the two disagree on any page that has
    /// a malformed link. Address the annotation by this, never by its
    /// position in the result.
    pub annots_index: usize,
    /// The annotation object's identity, when it was reached by an
    /// indirect reference (§7.3.10 — it always is in a well-formed
    /// file). `None` for a dictionary written directly into `/Annots`,
    /// which this function resolves anyway; see
    /// [`Annotation::destination`] for why that case is the reason this
    /// function exists.
    pub id: Option<ObjId>,
    /// The clickable box in default user space, normalised per §7.9.5.
    ///
    /// `None` is a real and reportable state, not a filter: §12.5.2
    /// makes `/Rect` **required**, so a link without one has a
    /// destination it can never be clicked to reach. It is kept rather
    /// than dropped so a repair tool can see it — a viewer should skip
    /// it for hit-testing and is free to say why.
    pub rect: Option<Rect>,
    /// Where activating it goes, fully resolved through both
    /// §12.3.2.3 namespaces and any `<< /D … >>` wrappers.
    ///
    /// Only [`Destination::Page`] is directly navigable. The other
    /// variants are disclosures — an unresolvable name, a page that is
    /// not in this document's tree, another file, or an action that is
    /// not a navigation at all — and collapsing them into "nothing here"
    /// is the failure mode this enum exists to prevent.
    pub destination: Destination,
}

/// Every navigable `/Link` on one page, plus what could not be read.
#[derive(Debug, Clone, Default, PartialEq)]
#[non_exhaustive]
pub struct PageLinks {
    /// The links that resolved, in `/Annots` order.
    pub links: Vec<LinkDestination>,
    /// How many `/Link` annotations on the page carried **neither**
    /// `/Dest` nor `/A`.
    ///
    /// Table 173 gives a link no other way to do anything, so each of
    /// these is a link the operator can see a border around and can
    /// never follow — a malformed annotation, usually the residue of an
    /// action stripped by a sanitiser.
    ///
    /// ★ It is counted rather than silently skipped because **a caller
    /// that only sees [`Self::links`] cannot distinguish a page with no
    /// links from a page whose links are all broken**, and those call
    /// for opposite operator messages.
    pub links_without_destination: usize,
}

/// Read every `/Link` annotation on one page and resolve where each one
/// goes (§12.5.6.5, Table 173).
///
/// This is the *complete* answer to "what is clickable on this page and
/// where does it lead", and the one to build a clickable table of
/// contents on. It walks `/Annots` and resolves each link's dictionary
/// **in place**, so unlike [`Annotation::destination`] it does not depend
/// on the annotation having an object id, and unlike a filter over
/// [`page_annotations`] it reports the links it could not resolve
/// instead of dropping them.
///
/// ## `/Link` only, deliberately
///
/// A `/Widget` pushbutton may also carry a `/GoTo` in its `/A` (Table
/// 188) and is genuinely navigable. It is **not** included here, because
/// this function's name is a promise about what it returns and a widget
/// is a form control first — activating one has form-side consequences
/// (`/AA` triggers, field focus) that a link does not, and a viewer
/// should not treat the two the same by accident. Resolve a widget
/// through [`Annotation::destination`], with the same
/// [`DestinationReader`].
///
/// ## Cost
///
/// `reader` is taken by reference rather than built here so that the two
/// **O(document)** tables behind it are built once and reused across
/// every page. Building one per call would make paging through a
/// 900-page document walk the page tree 900 times.
///
/// Bounded by [`MAX_ANNOTS_PER_PAGE`], like [`page_annotations`].
///
/// # Examples
///
/// ```no_run
/// use pdfcer_core::annot::page_link_destinations;
/// use pdfcer_core::document::Document;
/// use pdfcer_core::outline::{Destination, DestinationReader};
/// use pdfcer_core::page_tree::pages;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let doc = Document::load(std::path::Path::new("input.pdf"))?;
/// let reader = DestinationReader::new(&doc);
///
/// for page in pages(&doc)? {
///     let found = page_link_destinations(&doc, page.id, &reader);
///     if found.links_without_destination > 0 {
///         eprintln!("{} link(s) on this page lead nowhere", found.links_without_destination);
///     }
///     for link in found.links {
///         if let Destination::Page { page_index, .. } = link.destination {
///             println!("{:?} -> page {}", link.rect, page_index + 1);
///         }
///     }
/// }
/// # Ok(())
/// # }
/// ```
#[must_use]
pub fn page_link_destinations<G: ObjectGraph + ?Sized>(
    graph: &G,
    page_id: ObjId,
    reader: &DestinationReader,
) -> PageLinks {
    let mut out = PageLinks::default();

    let page = graph.resolved(page_id);
    // §7.7.3.4: `/Annots` is NOT inheritable, so there is no page-tree
    // walk here — a page without the key has no annotations, full stop.
    let Some(array) = page
        .as_dict()
        .and_then(|dict| dict.get(b"Annots"))
        .map(|value| graph.resolve(value))
        .and_then(Object::as_array)
    else {
        return out;
    };

    for (annots_index, entry) in array.iter().enumerate() {
        if annots_index >= MAX_ANNOTS_PER_PAGE {
            break;
        }
        let id = entry.as_reference();
        // A dangling reference resolves to null (§7.3.10) and a stray
        // number is not an annotation; both are skipped rather than
        // counted, because neither is a *link* that failed — they are
        // not links at all.
        let Some(dict) = graph.resolve(entry).as_dict() else {
            continue;
        };
        let is_link = graph
            .resolve(dict.get(b"Subtype").unwrap_or(&Object::Null))
            .as_name()
            .is_some_and(|name| name.as_bytes() == b"Link");
        if !is_link {
            continue;
        }
        match reader.destination(graph, dict) {
            Some(destination) => out.links.push(LinkDestination {
                annots_index,
                id,
                rect: dict.get(b"Rect").and_then(|value| read_rect(graph, value)),
                destination,
            }),
            None => out.links_without_destination += 1,
        }
    }
    out
}

/// Model one annotation dictionary into an [`Annotation`] (Table 164 +
/// §12.5.5 appearance selection).
fn model_annotation<G: ObjectGraph + ?Sized>(
    graph: &G,
    id: Option<ObjId>,
    dict: &Dict,
    missing_as: MissingAppearanceState,
) -> Annotation {
    let subtype = graph
        .resolve(dict.get(b"Subtype").unwrap_or(&Object::Null))
        .as_name()
        .map(|n| n.as_bytes().to_vec())
        .unwrap_or_default();
    let is_popup = subtype == b"Popup";

    let rect = dict.get(b"Rect").and_then(|o| read_rect(graph, o));

    // §12.5.3: /F is an integer bitfield; default 0. A non-integer /F is
    // malformed — treated as 0 (no flags) rather than rejected.
    let flags = AnnotFlags(
        dict.get(b"F")
            .map(|o| graph.resolve(o))
            .and_then(Object::as_int)
            .and_then(|v| u32::try_from(v).ok())
            .unwrap_or(0),
    );

    let appearance = select_normal_appearance(graph, dict, missing_as);

    // `Pass 255.0` point geometry. Each is read whenever its key is present
    // and array-shaped, regardless of `/Subtype` — see the struct docs.
    // `/L` is the one with a fixed arity: four numbers or nothing. A
    // six-number `/L` is malformed and reads as `None` rather than as
    // "the first two points", which would be the model repairing the file.
    let vertices = dict
        .get(b"Vertices")
        .map(|o| graph.resolve(o))
        .filter(|o| o.as_array().is_some())
        .map(|o| crate::annot_author::pairs_of(o, graph));
    let line = dict.get(b"L").and_then(|o| {
        let o = graph.resolve(o);
        let arity_ok = o.as_array().is_some_and(|items| items.len() == 4);
        match (arity_ok, crate::annot_author::pairs_of(o, graph).as_slice()) {
            (true, &[a, b]) => Some([a, b]),
            _ => None,
        }
    });
    let ink_list = dict.get(b"InkList").and_then(|o| {
        let strokes = graph.resolve(o).as_array()?;
        Some(
            strokes
                .iter()
                .map(|stroke| crate::annot_author::pairs_of(graph.resolve(stroke), graph))
                .collect::<Vec<_>>(),
        )
    });

    // §8.11.3.3 annotation /OC entry — an OCG or OCMD indirect reference. Only
    // the reference is modelled here; the render path resolves its default
    // visibility against /OCProperties /D (Pass 12.M2).
    let oc = dict.get(b"OC").and_then(Object::as_reference);

    // §7.9.2 text strings: `/Contents` and `/T` are text strings and may be
    // UTF-16BE with a BOM, so they go through the same decoder every other
    // text-string consumer in this crate uses rather than a second, private
    // lossy conversion that would disagree with it on non-Latin input.
    //
    // `/M` deliberately does NOT: it is "date or text string" and the
    // standard requires accepting any format, so it is surfaced verbatim.
    let text_of = |key: &[u8]| -> Option<String> {
        match graph.resolve(dict.get(key)?) {
            Object::String(bytes) => Some(crate::edit::decode_text_string(bytes).text),
            _ => None,
        }
    };
    // §12.5.2 Table 164: a number, 0.0-1.0. Out-of-range values are CLAMPED
    // rather than refused -- a producer writing 1.5 means "opaque", and
    // refusing to place the annotation over it would lose content to defend a
    // range check. Non-numeric is treated as absent.
    let constant_alpha = dict
        .get(b"CA")
        .map(|o| graph.resolve(o))
        .and_then(Object::as_number)
        .map(|v| v.clamp(0.0, 1.0));
    let contents = text_of(b"Contents");
    let title = text_of(b"T");
    let mod_date = text_of(b"M");

    // §12.5.6.2 Table 170 reply/grouping structure. Read for EVERY subtype,
    // not only markup ones, for the same reason `/T` is: an absent key is
    // `None`, which is exactly the truth, and a type-gated reader would make
    // the model silently disagree with a file that carries the key anyway.
    //
    // All three are taken as REFERENCES rather than resolved dictionaries.
    // Deletion needs identity (is THIS the object my `/IRT` names?), and a
    // resolved copy cannot answer that; a `/Popup` written as a direct
    // dictionary — illegal, since Table 164 dictionaries are indirect
    // objects — yields `None` and is therefore reported as "no companion"
    // rather than as a companion nothing can address.
    let popup = dict.get(b"Popup").and_then(Object::as_reference);
    let in_reply_to = dict.get(b"IRT").and_then(Object::as_reference);
    let reply_type = graph
        .resolve(dict.get(b"RT").unwrap_or(&Object::Null))
        .as_name()
        .map(|n| match n.as_bytes() {
            b"R" => ReplyType::Reply,
            b"Group" => ReplyType::Group,
            other => ReplyType::Other(other.to_vec()),
        });

    // `/A`'s `/S` type name (§12.5.2 Table 164). Resolved through the graph
    // because an action dictionary is routinely indirect, and taken as the
    // NAME only — see the field's documentation for why the parameters stay
    // out of the model.
    let action = dict
        .get(b"A")
        .map(|o| graph.resolve(o))
        .and_then(Object::as_dict);
    let action_type = action
        .and_then(|a| a.get(b"S"))
        .and_then(Object::as_name)
        .map(|n| n.as_bytes().to_vec());
    // Presence only — the chain is walked document-wide by
    // `forms::scan_javascript`, never here. `/Next` may be one action or an
    // array of them; both spellings mean the same thing to this bool.
    let action_chains = action.is_some_and(|a| a.get(b"Next").is_some());

    Annotation {
        id,
        subtype,
        rect,
        flags,
        vertices,
        line,
        ink_list,
        constant_alpha,
        appearance,
        is_popup,
        oc,
        contents,
        title,
        mod_date,
        popup,
        in_reply_to,
        reply_type,
        action_type,
        action_chains,
    }
}

/// The set of optional-content groups that are **OFF by default** per the
/// catalog `/OCProperties /D` configuration (ISO 32000-1 §8.11.4.3, Table
/// 101). Pass 12.M2 render-visibility input: an annotation whose `/OC`
/// resolves to (or through an OCMD to) an OFF group is hidden.
///
/// Follows the spec initialisation order: `/BaseState` (default `ON`) sets
/// all groups, then `/ON`/`/OFF` override. If `/OCProperties` or `/D` is
/// absent, the set is empty (nothing hidden by default). A missing
/// `/OCProperties` means optional content is ignored entirely (§8.11.4.2) —
/// returning an empty OFF set realises exactly that.
#[must_use]
pub fn optional_content_default_off<G: ObjectGraph + ?Sized>(graph: &G) -> BTreeSet<ObjId> {
    let mut off = BTreeSet::new();
    let Some(catalog) = graph.catalog_dict() else {
        return off;
    };
    let Some(ocp) = graph
        .resolve(catalog.get(b"OCProperties").unwrap_or(&Object::Null))
        .as_dict()
    else {
        return off;
    };
    let Some(d) = graph
        .resolve(ocp.get(b"D").unwrap_or(&Object::Null))
        .as_dict()
    else {
        return off;
    };
    let base_off = graph
        .resolve(d.get(b"BaseState").unwrap_or(&Object::Null))
        .as_name()
        .is_some_and(|n| n.as_bytes() == b"OFF");
    if base_off {
        // All OCGs start OFF; /ON re-enables.
        off.extend(oc_refs(graph, ocp.get(b"OCGs")));
        for on in oc_refs(graph, d.get(b"ON")) {
            off.remove(&on);
        }
    } else {
        // Default BaseState ON; /OFF disables specific groups.
        off.extend(oc_refs(graph, d.get(b"OFF")));
    }

    // §8.11.2.3 INTENT — which groups participate in visibility at all.
    //
    // ★ Not consulted until 2026-08-10, so a `Design`-only group hid
    // content in a `View` render. `/Design` is the author's structural
    // organisation of artwork — scaffolding a consumer is not supposed
    // to be affected by — and pdfcer was letting it blank out content for
    // a reader that had never asked to see design layers.
    //
    // The rule is a SET INTERSECTION, not a name comparison: both the
    // configuration and each group carry an intent that may be a single
    // name or an array, and a group participates when the two sets meet.
    // Table 101 additionally allows `All` on the configuration, which
    // matches everything.
    //
    // Applied as a FILTER over the already-computed set rather than
    // woven into the two branches above, because it applies identically
    // to both and the `/BaseState /OFF` branch is subtle enough already.
    let config_intent = intent_set(graph, d.get(b"Intent"));
    if config_intent.is_empty() {
        // §8.11.2.3: an empty intent array means no group participates,
        // which the clause states as "all content visible". An empty set
        // here, not the unfiltered one — this is the one case where
        // fewer intents means MORE visible content, and reading it as
        // "no filter" would inverte it.
        return BTreeSet::new();
    }
    if !config_intent.iter().any(|i| i == b"All") {
        off.retain(|id| {
            let group_intent = graph.resolved(*id).as_dict().map_or_else(
                || intent_set(graph, None),
                |g| intent_set(graph, g.get(b"Intent")),
            );
            group_intent.iter().any(|g| config_intent.contains(g))
        });
    }
    off
}

/// An `/Intent` entry as a set of names, defaulting to `[View]`
/// (§8.11.2.3, Table 98 and Table 101 both give `View` as the default).
///
/// Accepts a single name or an array, the same tolerance
/// [`oc_refs`] applies to `/OCGs`. A present-but-empty array is returned
/// EMPTY rather than defaulted, because §8.11.2.3 gives an empty array
/// its own meaning ("all content visible") — defaulting it to `View`
/// would silently discard the one intent value that changes the answer.
fn intent_set<G: ObjectGraph + ?Sized>(graph: &G, obj: Option<&Object>) -> Vec<Vec<u8>> {
    match obj.map(|o| graph.resolve(o)) {
        Some(Object::Name(n)) => vec![n.as_bytes().to_vec()],
        Some(Object::Array(items)) => items
            .iter()
            .map(|o| graph.resolve(o))
            .filter_map(Object::as_name)
            .map(|n| n.as_bytes().to_vec())
            .collect(),
        // Absent, null, or a type Table 98 does not allow: the default.
        _ => vec![b"View".to_vec()],
    }
}

/// How deep a `/VE` visibility expression may nest before pdfcer stops
/// descending (§8.11.2.2).
///
/// **This is pdfcer POLICY, not a spec requirement** — §8.11.2.2 sets no
/// depth limit and Annex C's architectural limits never mention
/// visibility expressions (`DA-N18`). Cite it as a choice, not a clause.
///
/// The standard sets no limit, and a boolean expression tree is a shape
/// a hostile or broken file can nest arbitrarily. 32 matches
/// [`crate::layers::MAX_ORDER_DEPTH`] deliberately — both are
/// optional-content trees walked from the same document, and two
/// different caps would mean a file that renders but cannot be listed,
/// or the reverse.
///
/// A real expression is two or three levels deep; anything past 32 is
/// not an expression an author wrote.
pub const MAX_VE_DEPTH: usize = 32;

/// Evaluate a `/VE` visibility expression (§8.11.2.2), or `None` if it
/// is not one pdfcer can evaluate.
///
/// # Returning `None` rather than a default is the whole design
///
/// `None` means *"this is not an expression I can evaluate"*, and the
/// caller responds by falling back to `/OCGs` + `/P`. That fallback is
/// not a guess: §8.11.2.2 NOTE 2 tells authors to supply `/OCGs` and
/// `/P` **alongside** `/VE` precisely so a reader without visibility-
/// expression support has something correct to use. Falling back is
/// therefore the behaviour the standard designed for, not a repair
/// pdfcer invented.
///
/// The alternative — treating a malformed `/VE` as "visible" — would
/// discard the author's `/P` for no reason, and treating it as "hidden"
/// would remove content because pdfcer could not read a *hint*.
///
/// # Grammar
///
/// An array whose first element is the name `And`, `Or` or `Not`.
/// Remaining elements are operands: either an OCG (ON = true) or a
/// nested expression array. `Not` takes exactly one operand; `And` and
/// `Or` take one or more.
///
/// `visited` carries the object ids of indirect arrays already entered,
/// because an expression may reference itself through an indirect
/// reference — legal syntax describing an infinite tree, the same hazard
/// `/Order` has and the same guard.
///
/// The cycle guard is **pdfcer policy too** (`DA-N19` — §8.11.2.2 states
/// no cycle rule), though it is the one policy here that is arguably
/// forced: the grammar permits arbitrary indirect nesting, so without it
/// a conforming-looking file ends the stack.
///
/// # `/VE` is preferred by a `should`, not a `shall`
///
/// NOTE 2 recommends supporting `/VE` in preference to `/OCGs` + `/P`;
/// there is no `shall` anywhere requiring it (`DA-A16`). A reader that
/// ignored visibility expressions entirely would violate nothing. So the
/// fallback below is not a concession — the whole arrangement is
/// NOTE 2's design, and a conforming file may legitimately carry `/VE`
/// with no `/OCGs` at all.
fn eval_ve<G: ObjectGraph + ?Sized>(
    graph: &G,
    obj: &Object,
    off: &BTreeSet<ObjId>,
    depth: usize,
    visited: &mut Vec<ObjId>,
) -> Option<bool> {
    if depth > MAX_VE_DEPTH {
        return None;
    }
    // Capture identity BEFORE resolving: the cycle guard's key exists
    // only on the reference.
    let id = obj.as_reference();
    if let Some(id) = id {
        if visited.contains(&id) {
            return None;
        }
        visited.push(id);
    }
    let result = eval_ve_resolved(graph, graph.resolve(obj), off, depth, visited);
    if id.is_some() {
        visited.pop();
    }
    result
}

/// [`eval_ve`] once the reference (if any) has been resolved and the
/// cycle guard has recorded it.
fn eval_ve_resolved<G: ObjectGraph + ?Sized>(
    graph: &G,
    resolved: &Object,
    off: &BTreeSet<ObjId>,
    depth: usize,
    visited: &mut Vec<ObjId>,
) -> Option<bool> {
    let Object::Array(items) = resolved else {
        return None;
    };
    let op = graph
        .resolve(items.first()?)
        .as_name()
        .map(|n| n.as_bytes().to_vec())?;
    // `get(1..)` rather than `&items[1..]`: `first()?` above proves
    // the array is non-empty, but the slicing lint is right that the
    // proof lives in another expression and a later edit could move it.
    let operands = items.get(1..)?;
    if operands.is_empty() {
        // `And`/`Or` take "one or more"; an operator with none is not an
        // expression, and inventing an identity element (`And` of
        // nothing = true) would be pdfcer deciding what the author meant.
        return None;
    }
    match op.as_slice() {
        b"Not" => {
            if operands.len() != 1 {
                // §8.11.2.2: `Not` takes EXACTLY one operand. A `Not`
                // with two is ambiguous — it could be read as
                // `Not(And(a, b))` or as a typo for `Or` — so it is not
                // evaluated rather than resolved by a house rule.
                return None;
            }
            Some(!eval_ve_operand(
                graph,
                operands.first()?,
                off,
                depth,
                visited,
            )?)
        }
        b"And" => {
            // Every operand must be evaluable: one unreadable operand
            // makes the whole conjunction unknown, because it is the
            // operand that could have been the false one.
            let mut all = true;
            for o in operands {
                all &= eval_ve_operand(graph, o, off, depth, visited)?;
            }
            Some(all)
        }
        b"Or" => {
            let mut any = false;
            for o in operands {
                any |= eval_ve_operand(graph, o, off, depth, visited)?;
            }
            Some(any)
        }
        // §8.11.2.2 names three operators. A fourth is not an expression
        // pdfcer can evaluate, and guessing at it is how a reader shows
        // content an author hid.
        _ => None,
    }
}

/// One operand of a `/VE` array: a nested expression, or an OCG whose
/// state is its truth value (ON = true).
fn eval_ve_operand<G: ObjectGraph + ?Sized>(
    graph: &G,
    obj: &Object,
    off: &BTreeSet<ObjId>,
    depth: usize,
    visited: &mut Vec<ObjId>,
) -> Option<bool> {
    // An array is a nested expression whatever it contains; try that
    // first, because an OCG is never an array.
    if matches!(graph.resolve(obj), Object::Array(_)) {
        return eval_ve(graph, obj, off, depth + 1, visited);
    }
    // Otherwise it must be an OCG, and it must be an indirect reference
    // — visibility is keyed on object identity, so a direct dictionary
    // has nothing to look up in `off`.
    let id = obj.as_reference()?;
    // §8.11.2.2: "Subsequent elements shall be either optional content
    // groups or other visibility expressions." An **OCMD is not a legal
    // operand** (`DA-N17`), and accepting one would silently treat a
    // membership dictionary as though it were a group — testing the
    // wrong object's state and producing a confident wrong answer rather
    // than an abstention.
    if graph
        .resolved(id)
        .as_dict()
        .and_then(|d| {
            graph
                .resolve(d.get(b"Type").unwrap_or(&Object::Null))
                .as_name()
        })
        .is_some_and(|n| n.as_bytes() == b"OCMD")
    {
        return None;
    }
    Some(!off.contains(&id))
}

/// Whether an annotation's `/OC` reference resolves to a hidden state, given
/// the default-OFF set from [`optional_content_default_off`] (§8.11.3.3).
///
/// A direct `/OCG` is hidden iff it is in `off`. An `/OCMD` is evaluated with
/// its default `AnyOn` policy (§8.11.2.2): hidden iff **all** its member OCGs
/// are OFF (an empty/undetermined membership is visible — the spec's "no
/// effect" rule). An unresolvable or non-optional-content target is treated as
/// visible (never hide by guessing).
#[must_use]
pub fn oc_is_hidden<G: ObjectGraph + ?Sized>(graph: &G, oc: ObjId, off: &BTreeSet<ObjId>) -> bool {
    let Some(d) = graph.resolved(oc).as_dict() else {
        return false;
    };
    let is_ocmd = graph
        .resolve(d.get(b"Type").unwrap_or(&Object::Null))
        .as_name()
        .is_some_and(|n| n.as_bytes() == b"OCMD");
    if is_ocmd {
        // §8.11.2.2: `/VE` is a full boolean expression and OVERRIDES
        // `/OCGs` + `/P` where a reader supports it. Tried first for
        // that reason, and `None` — "not an expression pdfcer can
        // evaluate" — falls through to the `/P` path below, which NOTE 2
        // tells authors to supply for exactly this reader.
        if let Some(visible) = eval_ve(
            graph,
            d.get(b"VE").unwrap_or(&Object::Null),
            off,
            0,
            &mut Vec::new(),
        ) {
            return !visible;
        }
        let members = oc_refs(graph, d.get(b"OCGs"));
        if members.is_empty() {
            // Table 99: with no `/OCGs` there is nothing to test, and
            // §8.11.3.3 makes visibility the default. `/VE` could still
            // decide it, but pdfcer does not evaluate visibility
            // expressions (see the fn docs), and refusing to guess shows
            // the content rather than hiding it.
            return false;
        }
        // Table 99 `/P` — the visibility POLICY, default `AnyOn`.
        //
        // ★ This was not read at all until 2026-08-10, so every
        // membership dictionary was evaluated as `AnyOn`. The divergence
        // is not a rounding error: under `/P /AllOff` with every member
        // group OFF the standard says the content is VISIBLE, and pdfcer
        // hid it — the exact inverse of the answer, on the policy whose
        // whole purpose is "show this when the layers are off".
        //
        // Note that each arm is written as the VISIBILITY test from the
        // table and then negated once, rather than as four hand-derived
        // hidden-tests. Deriving them by hand is how `AnyOff` and
        // `AllOff` get swapped: the negation of "any member is off" is
        // "every member is on", which is not any of the other three
        // policies and is easy to write as one by mistake.
        let on = |g: &ObjId| !off.contains(g);
        let visible = match graph
            .resolve(d.get(b"P").unwrap_or(&Object::Null))
            .as_name()
            .map(|n| n.as_bytes().to_vec())
            .as_deref()
        {
            Some(b"AllOn") => members.iter().all(on),
            Some(b"AnyOff") => members.iter().any(|g| !on(g)),
            Some(b"AllOff") => members.iter().all(|g| !on(g)),
            // `AnyOn` explicitly, and every other value: Table 99 names
            // four policies and gives `AnyOn` as the default, so an
            // unrecognised name falls back to the default rather than
            // inventing a fifth behaviour.
            _ => members.iter().any(on),
        };
        !visible
    } else {
        // Treat the reference itself as the OCG (Type /OCG or an untyped
        // group-shaped dict — the authored-layer case, §8.11 NOTE 3).
        off.contains(&oc)
    }
}

/// What a viewer's `/AS` usage application left unapplied, and why.
///
/// Returned alongside the state so a shell can say what it did not do.
/// A layer whose visibility is auto-managed by a category pdfcer cannot
/// evaluate is a layer showing the `/D` answer while the document asked
/// for a computed one — indistinguishable, from outside, from pdfcer
/// ignoring `/AS` entirely.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UsageNotes {
    /// `View`-event usage application dictionaries examined.
    pub applications: usize,
    /// Groups whose state a category actually decided.
    pub groups_managed: usize,
    /// Categories named in a `/Category` array that pdfcer cannot
    /// evaluate: `Language` (needs a system locale), `User` (needs an
    /// identity), and `CreatorInfo`/`PageElement` (§8.11.4.4 defines no
    /// behaviour for them at all — `DA-N13`).
    pub categories_unevaluable: usize,
    /// `/AS` entries whose `/Event` is not `View`. Counted, not applied:
    /// §8.11.4.5 scopes the viewer's examination to `Event` `View`, and
    /// `Print`/`Export` apply only for the duration of that operation.
    pub non_view_events: usize,
}

/// A single category's recommendation for one group, or `None` for
/// "this category yielded no recommendation".
///
/// # `None` is not `OFF`, and that distinction is the whole clause
///
/// §8.11.4.4's aggregation sentence reads *"If all the entries yield a
/// recommended state of `ON`, the group's state shall be set to `ON`;
/// otherwise, its state shall be set to `OFF`"*, which taken alone makes
/// a missing sub-dictionary an `OFF`. The standard then refutes that
/// twice in its own text: the `Print` bullet says an absent `PrintState`
/// leaves the state *"unchanged"*, and the §8.11.4.4 EXAMPLE says a
/// group with no `/Zoom` *"shall not be affected by zoom level
/// changes"*. `Language` goes the other way and explicitly assigns `OFF`
/// to non-matching groups.
///
/// Three loci, three answers. The reading that makes all four sentences
/// simultaneously true — and the one implemented here — is that an
/// absent category yields NO recommendation and is excluded from the
/// conjunction, rather than contributing a false.
///
/// Corroborated by the standard's own stated rationale for permitting
/// multiple same-`Event` entries: *"to allow documents with incompatible
/// usage application dictionaries to be combined into larger documents
/// and have their behaviour preserved"*. Under absent-means-`OFF`,
/// merging two documents blacks out every layer lacking a merged
/// category — precisely what that sentence exists to prevent.
///
/// Recorded in the spec corpus as `DA.16.4`, and registered as setting
/// candidate `DA-A13` (*absent usage category ⇒ skip | force OFF*). No
/// knob is offered: the alternative reading contradicts the standard's
/// own assembly rationale, which makes this a defended default rather
/// than a fork. See `ARCHITECTURE.md` for the difference.
type Recommendation = Option<bool>;

/// Evaluate one usage category for one group (§8.11.4.4, Table 102).
///
/// `magnification` is a SCALE FACTOR where `1.0` is 100 %.
/// §8.11.4.4 never defines the quantity it compares against — it says
/// only "the current magnification level of the document" — so the unit
/// is sourced from §12.3.2.2 (`/XYZ`: "magnified by the **factor**
/// `zoom`") and Annex C.2 ("between approximately 8 percent and 6400
/// percent"), plus the clause's own EXAMPLE, whose `[0,1) [1,2) [2,20)`
/// bands are captioned "20000 foot view" through "1000 foot view".
fn usage_recommendation<G: ObjectGraph + ?Sized>(
    graph: &G,
    usage: &Dict,
    category: &[u8],
    magnification: f32,
) -> Recommendation {
    let sub = graph
        .resolve(usage.get(category).unwrap_or(&Object::Null))
        .as_dict()?;
    match category {
        // Table 102: "A dictionary that shall have a single entry,
        // ViewState, a name that shall have a value of either ON or
        // OFF". A missing or unrecognised value is not repaired — it
        // yields no recommendation, which leaves the `/D` state standing.
        b"View" => state_name(graph, sub, b"ViewState"),
        // Table 102's `Print` is the loose one: "may contain the
        // following optional entries", both optional. §8.11.4.4 states
        // the absent case outright — "If PrintState is not present, the
        // state of the optional content group shall be left unchanged" —
        // which is `None`, and is where the excluded-from-the-conjunction
        // reading is sourced rather than inferred.
        b"Print" => state_name(graph, sub, b"PrintState"),
        b"Export" => state_name(graph, sub, b"ExportState"),
        // §8.11.4.4: "If the current magnification level of the document
        // is greater than or equal to `min` and less than `max`, the ON
        // state shall be used; otherwise, OFF shall be used."
        //
        // HALF-OPEN, `[min, max)`, and deliberately implemented with no
        // epsilon: `/max 1.0` is OFF at exactly 100 % and ON at 99.99 %.
        // That is the specified boundary and softening it would make a
        // layer appear at a magnification the document excluded.
        //
        // Defaults are Table 102's own: `min` 0, `max` infinity — so an
        // absent bound is unbounded on that side. An inverted or empty
        // range (`min >= max`) is NOT repaired: the standard imposes no
        // `min <= max` constraint and states no recovery, and the
        // predicate degrades to permanently-OFF, which is the honest
        // reading of a range that admits nothing.
        b"Zoom" => {
            let min = number(graph, sub.get(b"min")).unwrap_or(0.0);
            let max = number(graph, sub.get(b"max")).unwrap_or(f32::INFINITY);
            Some(magnification >= min && magnification < max)
        }
        // `Language` and `User` need a system locale and an identity
        // that pdfcer has no concept of; `CreatorInfo` and `PageElement`
        // have no defined effect on state at all (`DA-N13`). All four
        // are counted as unevaluable by the caller rather than guessed.
        _ => None,
    }
}

/// A `/ViewState`-style name as a boolean, `None` for absent or
/// unrecognised.
fn state_name<G: ObjectGraph + ?Sized>(graph: &G, dict: &Dict, key: &[u8]) -> Recommendation {
    match graph
        .resolve(dict.get(key).unwrap_or(&Object::Null))
        .as_name()?
        .as_bytes()
    {
        b"ON" => Some(true),
        b"OFF" => Some(false),
        _ => None,
    }
}

/// A number entry as `f32`, `None` if absent or not numeric.
fn number<G: ObjectGraph + ?Sized>(graph: &G, obj: Option<&Object>) -> Option<f32> {
    match graph.resolve(obj?) {
        Object::Integer(i) => Some(*i as f32),
        Object::Real(r) => Some(*r as f32),
        _ => None,
    }
}

/// Apply the `View`-event `/AS` usage applications on top of the
/// `/D`-initial state (§8.11.4.4 and §8.11.4.5).
///
/// # ★ This function must never be reachable from a print or export path
///
/// That is a `shall not`, not a preference. §8.11.4.5, of the
/// `/D`-initial state: *"This state shall be the state used by printing
/// and aggregating application. **Such applications shall not apply the
/// changes based on usage application dictionaries described below.**"*
/// Only then: *"The remaining discussion in this sub-clause applies only
/// to viewer applications. Such applications shall examine the `AS`
/// array…"*
///
/// So [`optional_content_default_off`] is not merely a first step that
/// this refines — it is the complete and correct answer for printing and
/// for aggregation, and calling this on the way to a printed page would
/// violate the standard rather than merely differ from it. The two are
/// separate functions so that the print path cannot acquire this one by
/// accident.
///
/// # It must be re-run when the magnification changes
///
/// §8.11.4.5: *"Whenever there is a change to a factor that the usage
/// application dictionaries with event type `View` depend on (such as
/// zoom level), the corresponding dictionaries shall be reapplied."*
/// Without the re-run the `Zoom` category does not work at all — a
/// layer banded to `[2.0, 20.0)` would keep whatever state it had when
/// the document opened.
///
/// # The operator's manual overrides sit ABOVE this, and stay
///
/// §8.11.4.5: *"Manual changes shall override the states that were set
/// automatically. The states of these groups remain overridden and shall
/// not be readjusted based on usage application dictionaries with event
/// type `View` as long as the document is open (or until the user
/// reverts the document to its original state)."*
///
/// That is why `pdfcer_render::LayerVisibility` REPLACES the document's
/// answer rather than merging with it — a merge would let a zoom change
/// re-decide a layer the operator had toggled, which the sentence above
/// forbids. The contract was argued for on its own terms before this
/// clause was read; the clause turns out to require it.
///
/// Two details of that sentence are worth carrying: the stickiness is
/// **per group** (*"the states of these groups"*), so toggling layer A
/// must not freeze layer B's zoom behaviour; and the standard names the
/// release — *"until the user reverts the document to its original
/// state"* — which is exactly the Layers panel's **Reset**.
///
/// # Aggregation across several applications
///
/// §8.11.4.4: *"If a given optional content group appears in more than
/// one `OCGs` array, its state shall be `ON` only if all categories in
/// all the usage application dictionaries it appears in shall have a
/// state of `ON`."*
///
/// A global conjunction: `OFF` dominates, and the result is
/// **order-independent**. Note that this is the opposite algebra to the
/// `/D` `/ON`//`/OFF` arrays in the same clause, where the order is
/// load-bearing (decision 038) — do not carry last-writer-wins thinking
/// across that boundary.
#[must_use]
pub fn apply_view_usage<G: ObjectGraph + ?Sized>(
    graph: &G,
    off: &mut BTreeSet<ObjId>,
    magnification: f32,
) -> UsageNotes {
    let mut notes = UsageNotes::default();
    let Some(catalog) = graph.catalog_dict() else {
        return notes;
    };
    let Some(ocp) = graph
        .resolve(catalog.get(b"OCProperties").unwrap_or(&Object::Null))
        .as_dict()
    else {
        return notes;
    };
    let Some(d) = graph
        .resolve(ocp.get(b"D").unwrap_or(&Object::Null))
        .as_dict()
    else {
        return notes;
    };
    let Some(Object::Array(applications)) = d.get(b"AS").map(|o| graph.resolve(o)) else {
        return notes;
    };

    // Accumulated across EVERY View-event application before anything is
    // written back, because the rule is a conjunction over all of them —
    // writing per-application would make the outcome depend on order.
    let mut verdicts: BTreeMap<ObjId, bool> = BTreeMap::new();

    for app in applications {
        let Some(app) = graph.resolve(app).as_dict() else {
            continue;
        };
        // Table 103: `/Event` is Required and shall be View, Print or
        // Export. §8.11.4.5 scopes the viewer to View.
        let event = graph
            .resolve(app.get(b"Event").unwrap_or(&Object::Null))
            .as_name()
            .map(|n| n.as_bytes().to_vec());
        if event.as_deref() != Some(b"View".as_slice()) {
            notes.non_view_events += 1;
            continue;
        }
        notes.applications += 1;

        // NOTE 3's trap: `Event` and `Category` share the names View,
        // Print and Export and are INDEPENDENT. `<< /Event /View
        // /Category [/Zoom] >>` is legal and common, so the categories
        // are read from `/Category` and never inferred from the event.
        let categories: Vec<Vec<u8>> = match app.get(b"Category").map(|o| graph.resolve(o)) {
            Some(Object::Array(items)) => items
                .iter()
                .map(|o| graph.resolve(o))
                .filter_map(Object::as_name)
                .map(|n| n.as_bytes().to_vec())
                .collect(),
            _ => Vec::new(),
        };

        // Table 103: `/OCGs` default is an empty array, "indicating that
        // no groups shall be affected".
        for group in oc_refs(graph, app.get(b"OCGs")) {
            let Some(usage) = graph.resolved(group).as_dict().and_then(|g| {
                graph
                    .resolve(g.get(b"Usage").unwrap_or(&Object::Null))
                    .as_dict()
            }) else {
                continue;
            };
            for category in &categories {
                if matches!(
                    category.as_slice(),
                    b"Language" | b"User" | b"CreatorInfo" | b"PageElement"
                ) {
                    notes.categories_unevaluable += 1;
                    continue;
                }
                if let Some(recommended) =
                    usage_recommendation(graph, usage, category, magnification)
                {
                    // Conjunction: once any category anywhere says OFF,
                    // no later ON can revive it.
                    let slot = verdicts.entry(group).or_insert(true);
                    *slot &= recommended;
                }
            }
        }
    }

    notes.groups_managed = verdicts.len();
    for (group, on) in verdicts {
        if on {
            off.remove(&group);
        } else {
            off.insert(group);
        }
    }
    notes
}

/// Collect the OCG references an `/OCGs`/`/ON`/`/OFF` entry names — either a
/// single indirect reference or an array of them (§8.11 Table 99/100/101).
///
/// # `pub(crate)` on purpose, and it must stay that way
///
/// [`crate::layers`] calls this. It briefly carried a byte-for-byte copy,
/// because this was private — and its own doc comment said so and asked
/// for the copy to be deleted if this ever became reachable. It has, and
/// it was.
///
/// The duplication mattered for a specific reason worth keeping: the
/// layers panel and the renderer must agree about **which groups a `/D`
/// array names**, or `locked` and `off` end up computed over different
/// sets of the same document — and the visible symptom is a panel that
/// says "shown" about content the page hides. Two functions cannot
/// guarantee that; one can.
///
/// Note the tolerance being shared as well as the behaviour: a single
/// reference is accepted where Table 101 says array. Both callers must
/// tolerate exactly the same malformed shapes, not merely the same
/// well-formed ones.
pub(crate) fn oc_refs<G: ObjectGraph + ?Sized>(graph: &G, obj: Option<&Object>) -> Vec<ObjId> {
    match obj.map(|o| graph.resolve(o)) {
        Some(Object::Reference(r)) => vec![*r],
        Some(Object::Array(items)) => items.iter().filter_map(Object::as_reference).collect(),
        _ => obj.and_then(Object::as_reference).into_iter().collect(),
    }
}

/// Select the normal (`/N`) appearance per ISO 32000-1 §12.5.5 (Table 168
/// + the `/AS` state-selection rule).
///
/// Returns the full negative-result taxonomy ([`Appearance`]); it never
/// guesses and never synthesises (R43).
fn select_normal_appearance<G: ObjectGraph + ?Sized>(
    graph: &G,
    annot: &Dict,
    missing_as: MissingAppearanceState,
) -> Appearance {
    // /AP (Table 164) — a dictionary. Absent or non-dictionary ⇒ nothing
    // to paint.
    let Some(ap) = annot
        .get(b"AP")
        .map(|o| graph.resolve(o))
        .and_then(Object::as_dict)
    else {
        return Appearance::None;
    };
    // /N (Table 168, Required). Absent ⇒ no normal appearance (R43
    // named-not-painted).
    let Some(n) = ap.get(b"N") else {
        return Appearance::None;
    };

    match graph.resolve(n) {
        // Form 1 — /N is a stream: that stream IS the normal appearance;
        // /AS is ignored (§12.5.5). Streams are indirect (§7.3.8.1), so
        // `n` is a reference and carries the cycle-guard identity.
        Object::Stream(_) => Appearance::Normal {
            stream_id: n.as_reference(),
        },
        // Form 2 — /N is a subdictionary keyed by appearance state; /AS
        // selects.
        Object::Dict(subdict) => {
            let state = annot
                .get(b"AS")
                .map(|o| graph.resolve(o))
                .and_then(Object::as_name);
            select_state(graph, subdict, state, missing_as)
        }
        // /N present but neither stream nor dictionary (malformed). Under
        // R43 there is no usable appearance; named-not-painted.
        _ => Appearance::None,
    }
}

/// Select one stream from a `/N` appearance-state subdictionary using
/// `/AS` (§12.5.5, Table 164 `/AS` + NOTE 3).
fn select_state<G: ObjectGraph + ?Sized>(
    graph: &G,
    subdict: &Dict,
    state: Option<&Name>,
    missing_as: MissingAppearanceState,
) -> Appearance {
    match state {
        // /AS present: paint the sub-entry it names, or display nothing if
        // that state is absent (§12.5.5 NOTE 3).
        Some(state) => match subdict.get(state.as_bytes()) {
            Some(entry) => classify_state_entry(graph, entry),
            None => Appearance::StateUnresolved,
        },
        // /AS absent. §12.5.5: /AS is Required when /AP holds
        // subdictionaries, so this is malformed. The RAG's negative
        // result: the spec gives NO rule for choosing among entries, so
        // "display nothing" is the conservative extension of NOTE 3 —
        // pdfcer must NOT guess a first/On/Off key *by default*. Under
        // R169 the guesses are available, named, and opt-in (`AS-A1`).
        None => {
            let mut present = subdict.iter().filter(|(_, v)| !matches!(v, Object::Null));
            match (present.next(), present.next()) {
                // Empty subdictionary ⇒ nothing to paint.
                (None, _) => Appearance::None,
                // Exactly one entry ⇒ unambiguous: there is only one
                // possible appearance, so painting it is not "guessing
                // among alternatives" that the RAG forbids — there are no
                // alternatives. (The forbidden case is a *multi-entry*
                // subdictionary with no /AS.) The setting does NOT reach
                // this arm: there is nothing here to have a policy about.
                (Some((_, only)), None) => classify_state_entry(graph, only),
                // Two or more entries, no /AS ⇒ the one genuinely
                // undefined case, and the only one `missing_as` governs.
                // Whichever way it goes the annotation is still surfaced
                // as state-unresolved when nothing is painted, so the
                // count never depends on the setting.
                (Some((_, first)), Some(_)) => match missing_as {
                    MissingAppearanceState::PaintNothing => Appearance::StateUnresolved,
                    // "First" is the dictionary's own iteration order,
                    // which `Dict` preserves from the file — so this is
                    // the PRODUCER's first entry, not an alphabetical
                    // invention of pdfcer's.
                    MissingAppearanceState::FirstEntry => classify_state_entry(graph, first),
                    // The checkbox-shaped guess. `/Off` is Table 164's own
                    // conventional name for an unset widget state, and it
                    // is the state that misleads least if the guess is
                    // wrong. Absent ⇒ back to painting nothing rather than
                    // falling through to a second guess.
                    MissingAppearanceState::OffElseNothing => subdict
                        .get(b"Off")
                        .filter(|v| !matches!(v, Object::Null))
                        .map_or(Appearance::StateUnresolved, |entry| {
                            classify_state_entry(graph, entry)
                        }),
                },
            }
        }
    }
}

/// Classify one appearance-subdictionary entry: a stream is paintable, a
/// dangling/non-stream entry is not (R43 named-not-painted).
fn classify_state_entry<G: ObjectGraph + ?Sized>(graph: &G, entry: &Object) -> Appearance {
    match graph.resolve(entry) {
        Object::Stream(_) => Appearance::Normal {
            stream_id: entry.as_reference(),
        },
        _ => Appearance::None,
    }
}

/// Read a `/Rect`-shaped array (four numbers, each possibly an indirect
/// reference per §7.3.10) and normalise it per §7.9.5.
///
/// Returns `None` when the value is not an array of four resolvable
/// numbers — a malformed `/Rect`, surfaced by the caller as a missing
/// placement target rather than repaired.
fn read_rect<G: ObjectGraph + ?Sized>(graph: &G, obj: &Object) -> Option<Rect> {
    let array = graph.resolve(obj).as_array()?;
    let nums: Vec<f64> = array
        .iter()
        .filter_map(|o| graph.resolve(o).as_number())
        .collect();
    match nums.as_slice() {
        &[x1, y1, x2, y2] => Some(Rect::from_corners(x1, y1, x2, y2)),
        _ => None,
    }
}

/// Whether the document's interactive form asserts its field appearances
/// are stale (`/AcroForm` `/NeedAppearances` true, ISO 32000-1 §12.7.2).
///
/// This is **document-scoped**, not per-page or per-annotation, so it is a
/// separate query rather than a field of [`Annotation`]. Pass 6.0 only
/// **counts** the documents that set it (R51): a document setting
/// `/NeedAppearances` true is asserting its widget appearances need
/// regenerating, and pdfcer reports that condition but **never** silently
/// regenerates on load — doing so would rewrite objects the operator
/// never touched (a §5 minimal-diff violation dressed as helpfulness) and
/// pick appearances for them (fuzzy-never-sneaky). Regeneration is a
/// Pass 7 operator-requested action.
///
/// A widget whose `/AP` `/N` is present is still painted from it at
/// display time regardless of `/NeedAppearances`; this flag only governs
/// the stale-appearance *disclosure*, not per-widget painting.
#[must_use]
pub fn need_appearances<G: ObjectGraph + ?Sized>(graph: &G) -> bool {
    let Some(catalog) = graph.catalog_dict() else {
        return false;
    };
    matches!(
        catalog
            .get(b"AcroForm")
            .map(|o| graph.resolve(o))
            .and_then(Object::as_dict)
            .and_then(|af| af.get(b"NeedAppearances"))
            .map(|o| graph.resolve(o)),
        Some(Object::Boolean(true))
    )
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
    use crate::document::Document;
    use crate::outline::DestView;

    /// Assemble a classic-xref PDF from numbered object bodies (raw bytes,
    /// so stream objects can be built by the same helper). Object 1 is the
    /// catalog; the xref is generated from contiguous numbering.
    fn build_pdf(objects: &[(u32, Vec<u8>)]) -> Document {
        let mut buf = b"%PDF-1.7\n".to_vec();
        let mut offsets: Vec<(u32, usize)> = Vec::new();
        for (num, body) in objects {
            offsets.push((*num, buf.len()));
            buf.extend_from_slice(format!("{num} 0 obj\n").as_bytes());
            buf.extend_from_slice(body);
            buf.extend_from_slice(b"\nendobj\n");
        }
        let xref_at = buf.len();
        // Object numbers may be non-contiguous (annotation fixtures skip
        // ids for readability), so the xref spans 0..=max and any gap is a
        // free entry — mirroring `page_tree::tests::build_pdf`.
        let max_num = objects.iter().map(|(n, _)| *n).max().unwrap_or(0);
        let size = max_num + 1;
        buf.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f\r\n").as_bytes());
        for num in 1..=max_num {
            match offsets.iter().find(|(n, _)| *n == num) {
                Some((_, off)) => {
                    buf.extend_from_slice(format!("{off:010} 00000 n\r\n").as_bytes());
                }
                None => buf.extend_from_slice(b"0000000000 65535 f\r\n"),
            }
        }
        buf.extend_from_slice(
            format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n")
                .as_bytes(),
        );
        Document::from_bytes(buf).unwrap()
    }

    /// A stream object body.
    fn stream_object(dict_extra: &str, data: &[u8]) -> Vec<u8> {
        let mut out = format!("<< {dict_extra} /Length {} >>\nstream\n", data.len()).into_bytes();
        out.extend_from_slice(data);
        out.extend_from_slice(b"\nendstream");
        out
    }

    /// A one-page document whose single page carries the given raw
    /// `/Annots` array text and the given extra objects (numbered from 5).
    /// The page is object 3; its id is `ObjId::new(3, 0)`.
    fn doc_with_annots(annots: &str, extra: &[(u32, Vec<u8>)]) -> Document {
        let mut objects: Vec<(u32, Vec<u8>)> = vec![
            (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
            (
                2,
                b"<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 200 200] \
                  /Resources << >> >>"
                    .to_vec(),
            ),
            (
                3,
                format!("<< /Type /Page /Parent 2 0 R /Annots {annots} >>").into_bytes(),
            ),
        ];
        objects.extend_from_slice(extra);
        build_pdf(&objects)
    }

    const PAGE_ID: ObjId = ObjId::new(3, 0);

    /// A form-XObject appearance stream body (a valid `/N` target).
    fn ap_stream(extra: &str) -> Vec<u8> {
        stream_object(
            &format!("/Type /XObject /Subtype /Form /BBox [0 0 20 20] {extra}"),
            b"0 0 0 rg 0 0 20 20 re f",
        )
    }

    #[test]
    fn flag_bit_values_match_table_165() {
        // Off-by-one here silently mis-reads every flag: Hidden is bit 2 =
        // value 2, NOT 1<<2.
        assert_eq!(AnnotFlags::INVISIBLE, 1);
        assert_eq!(AnnotFlags::HIDDEN, 2);
        assert_eq!(AnnotFlags::PRINT, 4);
        assert_eq!(AnnotFlags::NO_ZOOM, 8);
        assert_eq!(AnnotFlags::NO_ROTATE, 16);
        assert_eq!(AnnotFlags::NO_VIEW, 32);
        assert!(AnnotFlags(2).hidden() && AnnotFlags(2).suppressed_on_screen());
        assert!(AnnotFlags(32).no_view() && AnnotFlags(32).suppressed_on_screen());
        assert!(
            !AnnotFlags(4).suppressed_on_screen(),
            "Print is screen-neutral"
        );
    }

    #[test]
    fn absent_annots_yields_nothing() {
        let doc = build_pdf(&[
            (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
            (
                2,
                b"<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 10 10] \
                  /Resources << >> >>"
                    .to_vec(),
            ),
            (3, b"<< /Type /Page /Parent 2 0 R >>".to_vec()),
        ]);
        assert!(page_annotations(&doc, PAGE_ID).is_empty());
    }

    #[test]
    fn stream_n_is_selected_and_rect_normalized() {
        let doc = doc_with_annots(
            "[5 0 R]",
            &[
                (
                    5,
                    b"<< /Type /Annot /Subtype /Square /Rect [30 40 10 20] /AP << /N 6 0 R >> >>"
                        .to_vec(),
                ),
                (6, ap_stream("")),
            ],
        );
        let annots = page_annotations(&doc, PAGE_ID);
        assert_eq!(annots.len(), 1);
        let a = &annots[0];
        assert_eq!(a.subtype, b"Square");
        // §7.9.5: corners normalised min→max.
        let r = a.rect.unwrap();
        assert_eq!((r.llx, r.lly, r.urx, r.ury), (10.0, 20.0, 30.0, 40.0));
        assert_eq!(
            a.appearance,
            Appearance::Normal {
                stream_id: Some(ObjId::new(6, 0))
            }
        );
    }

    #[test]
    fn no_ap_is_none_by_subtype() {
        let doc = doc_with_annots(
            "[5 0 R]",
            &[(
                5,
                b"<< /Subtype /Circle /Rect [0 0 10 10] /IC [1 0 0] >>".to_vec(),
            )],
        );
        let a = &page_annotations(&doc, PAGE_ID)[0];
        // R43: an /IC-only Circle synthesises nothing — named-not-painted.
        assert_eq!(a.appearance, Appearance::None);
        assert_eq!(a.subtype_label(), "Circle");
    }

    #[test]
    fn as_selects_from_state_subdictionary() {
        // Checkbox: /N subdictionary keyed On/Off, /AS picks On.
        let doc = doc_with_annots(
            "[5 0 R]",
            &[
                (
                    5,
                    b"<< /Subtype /Widget /Rect [0 0 10 10] /AS /On \
                      /AP << /N << /On 6 0 R /Off 7 0 R >> >> >>"
                        .to_vec(),
                ),
                (6, ap_stream("")),
                (7, ap_stream("")),
            ],
        );
        let a = &page_annotations(&doc, PAGE_ID)[0];
        assert!(a.is_widget());
        assert_eq!(
            a.appearance,
            Appearance::Normal {
                stream_id: Some(ObjId::new(6, 0))
            }
        );
    }

    #[test]
    fn as_naming_absent_state_displays_nothing() {
        let doc = doc_with_annots(
            "[5 0 R]",
            &[
                (
                    5,
                    b"<< /Subtype /Widget /Rect [0 0 10 10] /AS /Maybe \
                      /AP << /N << /On 6 0 R /Off 7 0 R >> >> >>"
                        .to_vec(),
                ),
                (6, ap_stream("")),
                (7, ap_stream("")),
            ],
        );
        // §12.5.5 NOTE 3: state not found ⇒ display nothing.
        assert_eq!(
            page_annotations(&doc, PAGE_ID)[0].appearance,
            Appearance::StateUnresolved
        );
    }

    #[test]
    fn missing_as_multi_entry_displays_nothing_not_a_guess() {
        let doc = doc_with_annots(
            "[5 0 R]",
            &[
                (
                    5,
                    b"<< /Subtype /Widget /Rect [0 0 10 10] \
                      /AP << /N << /On 6 0 R /Off 7 0 R >> >> >>"
                        .to_vec(),
                ),
                (6, ap_stream("")),
                (7, ap_stream("")),
            ],
        );
        // No /AS against a multi-entry subdictionary: the RAG's negative
        // result — display nothing, never guess On/Off.
        assert_eq!(
            page_annotations(&doc, PAGE_ID)[0].appearance,
            Appearance::StateUnresolved
        );
    }

    #[test]
    fn missing_as_policy_offers_the_two_empirical_guesses_as_opt_ins() {
        // `AS-A1` (R169). The default above stays "paint nothing"; these
        // are the guesses the spec RAG explicitly forbids INSTALLING but
        // does not forbid OFFERING. `/On` is written first, so the
        // producer's first entry is object 6.
        let doc = doc_with_annots(
            "[5 0 R]",
            &[
                (
                    5,
                    b"<< /Subtype /Widget /Rect [0 0 10 10] \
                      /AP << /N << /On 6 0 R /Off 7 0 R >> >> >>"
                        .to_vec(),
                ),
                (6, ap_stream("")),
                (7, ap_stream("")),
            ],
        );
        assert_eq!(
            page_annotations_with(&doc, PAGE_ID, MissingAppearanceState::FirstEntry)[0].appearance,
            Appearance::Normal {
                stream_id: Some(ObjId::new(6, 0))
            },
            "`first_entry` must take the FILE's first key, not an \
             alphabetical one"
        );
        assert_eq!(
            page_annotations_with(&doc, PAGE_ID, MissingAppearanceState::OffElseNothing)[0]
                .appearance,
            Appearance::Normal {
                stream_id: Some(ObjId::new(7, 0))
            }
        );
        assert_eq!(
            page_annotations_with(&doc, PAGE_ID, MissingAppearanceState::PaintNothing)[0]
                .appearance,
            Appearance::StateUnresolved,
            "the default must be unchanged by the setting existing"
        );
        assert_eq!(
            page_annotations(&doc, PAGE_ID)[0].appearance,
            page_annotations_with(&doc, PAGE_ID, MissingAppearanceState::default())[0].appearance,
            "the convenience wrapper must be the default policy"
        );
    }

    #[test]
    fn off_else_nothing_falls_back_rather_than_guessing_twice() {
        // The guess is specifically "/Off", not "some entry". A
        // subdictionary with no /Off must go back to painting nothing —
        // falling through to the first entry would be a second, unnamed
        // guess stacked on the operator's chosen one.
        let doc = doc_with_annots(
            "[5 0 R]",
            &[
                (
                    5,
                    b"<< /Subtype /Widget /Rect [0 0 10 10] \
                      /AP << /N << /Yes 6 0 R /No 7 0 R >> >> >>"
                        .to_vec(),
                ),
                (6, ap_stream("")),
                (7, ap_stream("")),
            ],
        );
        assert_eq!(
            page_annotations_with(&doc, PAGE_ID, MissingAppearanceState::OffElseNothing)[0]
                .appearance,
            Appearance::StateUnresolved
        );
    }

    #[test]
    fn the_missing_as_policy_cannot_reach_a_well_formed_annotation() {
        // Blast-radius containment. The setting governs ONE malformed
        // configuration; a present /AS and a single-entry subdictionary
        // are both spec-determined and must be identical under all three
        // values, or the knob is wider than its documentation claims.
        let with_as = doc_with_annots(
            "[5 0 R]",
            &[
                (
                    5,
                    b"<< /Subtype /Widget /Rect [0 0 10 10] /AS /On \
                      /AP << /N << /On 6 0 R /Off 7 0 R >> >> >>"
                        .to_vec(),
                ),
                (6, ap_stream("")),
                (7, ap_stream("")),
            ],
        );
        let single = doc_with_annots(
            "[5 0 R]",
            &[
                (
                    5,
                    b"<< /Subtype /Widget /Rect [0 0 10 10] \
                      /AP << /N << /Only 6 0 R >> >> >>"
                        .to_vec(),
                ),
                (6, ap_stream("")),
            ],
        );
        for policy in [
            MissingAppearanceState::PaintNothing,
            MissingAppearanceState::FirstEntry,
            MissingAppearanceState::OffElseNothing,
        ] {
            assert_eq!(
                page_annotations_with(&with_as, PAGE_ID, policy)[0].appearance,
                Appearance::Normal {
                    stream_id: Some(ObjId::new(6, 0))
                },
                "{policy:?} disturbed a present /AS"
            );
            assert_eq!(
                page_annotations_with(&single, PAGE_ID, policy)[0].appearance,
                Appearance::Normal {
                    stream_id: Some(ObjId::new(6, 0))
                },
                "{policy:?} disturbed a single-entry subdictionary"
            );
        }
    }

    #[test]
    fn missing_as_single_entry_is_unambiguous() {
        let doc = doc_with_annots(
            "[5 0 R]",
            &[
                (
                    5,
                    b"<< /Subtype /Widget /Rect [0 0 10 10] \
                      /AP << /N << /Only 6 0 R >> >> >>"
                        .to_vec(),
                ),
                (6, ap_stream("")),
            ],
        );
        // One entry, no /AS: there are no alternatives to guess among, so
        // painting the sole appearance is unambiguous (not the forbidden
        // multi-entry guess).
        assert_eq!(
            page_annotations(&doc, PAGE_ID)[0].appearance,
            Appearance::Normal {
                stream_id: Some(ObjId::new(6, 0))
            }
        );
    }

    #[test]
    fn popup_is_flagged_structurally() {
        let doc = doc_with_annots(
            "[5 0 R]",
            &[(
                5,
                b"<< /Subtype /Popup /Rect [0 0 10 10] /Open true >>".to_vec(),
            )],
        );
        let a = &page_annotations(&doc, PAGE_ID)[0];
        assert!(
            a.is_popup,
            "/Popup must be flagged for the never-paint rule"
        );
    }

    #[test]
    fn non_dictionary_annots_entries_are_skipped() {
        // A dangling reference (null) and a bare number are not
        // annotations; the real one survives.
        let doc = doc_with_annots(
            "[99 0 R 42 5 0 R]",
            &[(5, b"<< /Subtype /Link /Rect [0 0 10 10] >>".to_vec())],
        );
        let annots = page_annotations(&doc, PAGE_ID);
        assert_eq!(annots.len(), 1);
        assert_eq!(annots[0].subtype, b"Link");
    }

    #[test]
    fn flags_decoded_from_f_integer() {
        let doc = doc_with_annots(
            "[5 0 R]",
            &[(
                5,
                // Hidden|Print = 2|4 = 6.
                b"<< /Subtype /Text /Rect [0 0 10 10] /F 6 >>".to_vec(),
            )],
        );
        let a = &page_annotations(&doc, PAGE_ID)[0];
        assert!(a.flags.hidden());
        assert!(a.flags.print());
        assert!(a.flags.suppressed_on_screen());
    }

    #[test]
    fn need_appearances_reads_acroform() {
        let doc = build_pdf(&[
            (
                1,
                b"<< /Type /Catalog /Pages 2 0 R /AcroForm << /NeedAppearances true >> >>".to_vec(),
            ),
            (
                2,
                b"<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 10 10] \
                  /Resources << >> >>"
                    .to_vec(),
            ),
            (3, b"<< /Type /Page /Parent 2 0 R >>".to_vec()),
        ]);
        assert!(need_appearances(&doc));
    }

    /// `/Contents`, `/T` and `/M` are modelled, and each is `None` when absent
    /// rather than an empty string — "no note" and "an empty note" are
    /// different facts and a UI captions them differently.
    #[test]
    fn contents_title_and_mod_date_are_modelled_and_absent_means_none() {
        let doc = doc_with_annots(
            "[4 0 R 5 0 R]",
            &[
                (
                    4,
                    b"<< /Type /Annot /Subtype /Square /Rect [0 0 10 10]                        /Contents (Check this dimension) /T (Ken) /M (D:20260806120000Z) >>"
                        .to_vec(),
                ),
                // No /Contents, no /T, no /M — the common case for a shape
                // pdfcer itself authored (Pass 6.1 sets none of them).
                (
                    5,
                    b"<< /Type /Annot /Subtype /Circle /Rect [0 0 10 10] >>".to_vec(),
                ),
            ],
        );
        let annots = page_annotations(&doc, PAGE_ID);
        assert_eq!(annots.len(), 2);

        assert_eq!(annots[0].contents.as_deref(), Some("Check this dimension"));
        assert_eq!(annots[0].title.as_deref(), Some("Ken"));
        assert_eq!(annots[0].mod_date.as_deref(), Some("D:20260806120000Z"));

        assert_eq!(annots[1].contents, None, "absent /Contents is None");
        assert_eq!(annots[1].title, None, "absent /T is None");
        assert_eq!(annots[1].mod_date, None, "absent /M is None");
    }

    /// A UTF-16BE `/Contents` decodes through the SAME §7.9.2 decoder every
    /// other text-string consumer uses.
    ///
    /// Non-vacuous by construction: the assertion is on a non-ASCII character
    /// that a naive byte-to-char conversion would mangle, so a second private
    /// lossy decoder could not pass this.
    #[test]
    fn a_utf16_contents_decodes_rather_than_mojibake() {
        // UTF-16BE BOM + "Ré" — 0xFEFF, 'R', 0x00E9.
        let doc = doc_with_annots(
            "[4 0 R]",
            &[(
                4,
                b"<< /Type /Annot /Subtype /Text /Rect [0 0 10 10]                    /Contents <FEFF005200E9> >>"
                    .to_vec(),
            )],
        );
        let annots = page_annotations(&doc, PAGE_ID);
        assert_eq!(annots[0].contents.as_deref(), Some("Ré"));
    }

    /// `/M` is stored VERBATIM, including a value that is not a §7.9.4 date.
    ///
    /// §12.5.2 gives its type as "date or text string" and requires a reader
    /// to "accept and display a string in any format" — so a parser that
    /// rejected or normalised this would violate the standard, and this test
    /// is what stops one being added later.
    #[test]
    fn a_non_date_mod_date_is_kept_verbatim_because_the_standard_demands_it() {
        let doc = doc_with_annots(
            "[4 0 R]",
            &[(
                4,
                b"<< /Type /Annot /Subtype /Square /Rect [0 0 10 10]                    /M (last Tuesday) >>"
                    .to_vec(),
            )],
        );
        let annots = page_annotations(&doc, PAGE_ID);
        assert_eq!(annots[0].mod_date.as_deref(), Some("last Tuesday"));
    }
    // ---- Table 99 `/P`, the OCMD visibility policy ----

    /// A document with one OCMD over two OCGs, `off` naming which of the
    /// two the default configuration hides, and `policy` the `/P` value
    /// (empty for "absent", which must behave as `AnyOn`).
    fn ocmd_is_hidden(policy: &str, first_off: bool, second_off: bool) -> bool {
        let p = if policy.is_empty() {
            String::new()
        } else {
            format!("/P /{policy}")
        };
        let mut off = String::from("/OFF [");
        if first_off {
            off.push_str("5 0 R ");
        }
        if second_off {
            off.push_str("6 0 R");
        }
        off.push(']');
        let objects: Vec<(u32, Vec<u8>)> = vec![
            (
                1,
                format!(
                    "<< /Type /Catalog /Pages 2 0 R /OCProperties \
                     << /OCGs [5 0 R 6 0 R] /D << {off} >> >> >>"
                )
                .into_bytes(),
            ),
            (
                2,
                b"<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 10 10] >>".to_vec(),
            ),
            (3, b"<< /Type /Page /Parent 2 0 R >>".to_vec()),
            (
                4,
                format!("<< /Type /OCMD /OCGs [5 0 R 6 0 R] {p} >>").into_bytes(),
            ),
            (5, b"<< /Type /OCG /Name (A) >>".to_vec()),
            (6, b"<< /Type /OCG /Name (B) >>".to_vec()),
        ];
        let doc = build_pdf(&objects);
        let graph = doc.view();
        let off_set = optional_content_default_off(&graph);
        oc_is_hidden(&graph, ObjId::new(4, 0), &off_set)
    }

    /// ★ **`/P /AllOff` with every member off is VISIBLE.**
    ///
    /// The case that was inverted. Until `/P` was read, every OCMD was
    /// evaluated as `AnyOn`, so "show this when the layers are off" —
    /// which is the entire purpose of `AllOff` — produced exactly the
    /// opposite answer. A "no layers shown" placeholder would have been
    /// hidden precisely when it was meant to appear.
    #[test]
    fn an_all_off_membership_is_visible_when_every_member_is_off() {
        assert!(
            !ocmd_is_hidden("AllOff", true, true),
            "Table 99 /P /AllOff: visible iff all members are OFF"
        );
        assert!(
            ocmd_is_hidden("AllOff", true, false),
            "one member still on means not all off, so hidden"
        );
    }

    /// The default, and an absent `/P`, are both `AnyOn` — the behaviour
    /// every OCMD had before `/P` was read, pinned so implementing the
    /// other three policies cannot have moved it.
    #[test]
    fn an_absent_p_behaves_as_any_on() {
        for policy in ["", "AnyOn"] {
            assert!(ocmd_is_hidden(policy, true, true), "{policy:?}: none on");
            assert!(!ocmd_is_hidden(policy, true, false), "{policy:?}: one on");
            assert!(!ocmd_is_hidden(policy, false, false), "{policy:?}: both on");
        }
    }

    /// `AllOn` and `AnyOff` are each other's inverse over the same two
    /// groups, which is the property that catches the likeliest mistake:
    /// deriving the hidden-test by hand and swapping them.
    #[test]
    fn all_on_and_any_off_are_complementary() {
        for (a, b) in [(false, false), (true, false), (false, true), (true, true)] {
            assert_ne!(
                ocmd_is_hidden("AllOn", a, b),
                ocmd_is_hidden("AnyOff", a, b),
                "AllOn and AnyOff must disagree for off=({a}, {b})"
            );
        }
    }

    /// An unrecognised `/P` falls back to the default rather than
    /// inventing a fifth policy — Table 99 names exactly four.
    #[test]
    fn an_unknown_p_falls_back_to_the_default() {
        assert_eq!(
            ocmd_is_hidden("Sometimes", true, false),
            ocmd_is_hidden("AnyOn", true, false)
        );
    }

    /// An OCMD with no `/OCGs` is visible: there is nothing to test, and
    /// hiding content because a membership dictionary was empty would
    /// remove marks no clause asks to remove.
    #[test]
    fn an_empty_membership_is_visible() {
        let objects: Vec<(u32, Vec<u8>)> = vec![
            (
                1,
                b"<< /Type /Catalog /Pages 2 0 R /OCProperties \
                  << /OCGs [5 0 R] /D << /OFF [5 0 R] >> >> >>"
                    .to_vec(),
            ),
            (
                2,
                b"<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 10 10] >>".to_vec(),
            ),
            (3, b"<< /Type /Page /Parent 2 0 R >>".to_vec()),
            (4, b"<< /Type /OCMD /P /AllOn >>".to_vec()),
            (5, b"<< /Type /OCG /Name (A) >>".to_vec()),
        ];
        let doc = build_pdf(&objects);
        let graph = doc.view();
        let off_set = optional_content_default_off(&graph);
        assert!(!oc_is_hidden(&graph, ObjId::new(4, 0), &off_set));
    }
    // ---- §8.11.2.3 `/Intent` ----

    /// A doc whose single OCG is in `/D /OFF`, with `group_intent` on the
    /// group and `config_intent` on the configuration (empty string =
    /// the entry is absent).
    fn design_intent_off_set(group_intent: &str, config_intent: &str) -> usize {
        let g = if group_intent.is_empty() {
            String::new()
        } else {
            format!("/Intent {group_intent}")
        };
        let c = if config_intent.is_empty() {
            String::new()
        } else {
            format!("/Intent {config_intent}")
        };
        let objects: Vec<(u32, Vec<u8>)> = vec![
            (
                1,
                format!(
                    "<< /Type /Catalog /Pages 2 0 R /OCProperties \
                     << /OCGs [5 0 R] /D << /OFF [5 0 R] {c} >> >> >>"
                )
                .into_bytes(),
            ),
            (
                2,
                b"<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 10 10] >>".to_vec(),
            ),
            (3, b"<< /Type /Page /Parent 2 0 R >>".to_vec()),
            (4, b"<< /Type /OCG /Name (unused) >>".to_vec()),
            (5, format!("<< /Type /OCG /Name (A) {g} >>").into_bytes()),
        ];
        let doc = build_pdf(&objects);
        optional_content_default_off(&doc.view()).len()
    }

    /// ★ **A `Design`-only group does not hide content in a `View`
    /// render.**
    ///
    /// §8.11.2.3: `/Design` is the author's structural organisation of
    /// artwork, and a configuration's intent set selects which groups
    /// participate in visibility at all. Until `/Intent` was read, a
    /// group marked `Design` and listed in `/OFF` blanked out content for
    /// a reader that had never asked to see design layers.
    #[test]
    fn a_design_only_group_does_not_hide_in_a_view_configuration() {
        assert_eq!(
            design_intent_off_set("/Design", ""),
            0,
            "the configuration defaults to /View, which /Design does not meet"
        );
        assert_eq!(
            design_intent_off_set("/Design", "/Design"),
            1,
            "a Design configuration DOES consider it"
        );
    }

    /// The defaults on both sides are `View`, so an ordinary document
    /// with no `/Intent` anywhere is unaffected — the case that must not
    /// have moved.
    #[test]
    fn absent_intent_on_both_sides_still_hides() {
        assert_eq!(design_intent_off_set("", ""), 1);
        assert_eq!(design_intent_off_set("/View", ""), 1);
    }

    /// Intent is a SET intersection, so a group naming both intents
    /// participates in either configuration.
    #[test]
    fn an_array_intent_intersects_rather_than_compares() {
        assert_eq!(design_intent_off_set("[/View /Design]", ""), 1);
        assert_eq!(design_intent_off_set("[/View /Design]", "/Design"), 1);
        assert_eq!(design_intent_off_set("[/Design]", ""), 0);
    }

    /// Table 101's `All` on the configuration considers every group
    /// whatever its intent.
    #[test]
    fn a_config_intent_of_all_considers_every_group() {
        assert_eq!(design_intent_off_set("/Design", "/All"), 1);
        assert_eq!(design_intent_off_set("/Anything", "/All"), 1);
    }

    /// ★ **An EMPTY configuration intent array makes everything visible.**
    ///
    /// The one case where fewer intents means MORE visible content.
    /// §8.11.2.3 states it outright, and it is exactly the shape a
    /// "treat an empty array as no filter" reading gets backwards — that
    /// reading would hide the group instead.
    #[test]
    fn an_empty_config_intent_array_shows_everything() {
        assert_eq!(design_intent_off_set("/View", "[]"), 0);
        assert_eq!(design_intent_off_set("", "[]"), 0);
    }
    // ---- §8.11.2.2 `/VE` visibility expressions ----

    /// An OCMD carrying `ve` (raw PDF for the `/VE` value) plus an
    /// `/OCGs` + `/P` pair that would answer DIFFERENTLY, so every test
    /// below shows which of the two was actually consulted.
    ///
    /// Groups 5 and 6 exist; `off` says which are hidden. `/P /AllOn`
    /// with both listed means the fallback answers "visible only when
    /// both are on".
    fn ocmd_ve_hidden(ve: &str, five_off: bool, six_off: bool) -> bool {
        let mut off = String::from("/OFF [");
        if five_off {
            off.push_str("5 0 R ");
        }
        if six_off {
            off.push_str("6 0 R");
        }
        off.push(']');
        let ve_entry = if ve.is_empty() {
            String::new()
        } else {
            format!("/VE {ve}")
        };
        let objects: Vec<(u32, Vec<u8>)> = vec![
            (
                1,
                format!(
                    "<< /Type /Catalog /Pages 2 0 R /OCProperties \
                     << /OCGs [5 0 R 6 0 R] /D << {off} >> >> >>"
                )
                .into_bytes(),
            ),
            (
                2,
                b"<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 10 10] >>".to_vec(),
            ),
            (3, b"<< /Type /Page /Parent 2 0 R >>".to_vec()),
            (
                4,
                format!("<< /Type /OCMD /OCGs [5 0 R 6 0 R] /P /AllOn {ve_entry} >>").into_bytes(),
            ),
            (5, b"<< /Type /OCG /Name (A) >>".to_vec()),
            (6, b"<< /Type /OCG /Name (B) >>".to_vec()),
        ];
        let doc = build_pdf(&objects);
        let graph = doc.view();
        let off_set = optional_content_default_off(&graph);
        oc_is_hidden(&graph, ObjId::new(4, 0), &off_set)
    }

    /// ★ **`/VE` overrides `/OCGs` + `/P`.**
    ///
    /// The fixture's `/P /AllOn` says "visible only when BOTH groups are
    /// on". The expression says `Or`, which is satisfied by one. With
    /// group B off, the two answers differ — so this test says which one
    /// pdfcer used, and §8.11.2.2 requires the expression.
    #[test]
    fn a_visibility_expression_overrides_the_p_policy() {
        assert!(
            !ocmd_ve_hidden("[/Or 5 0 R 6 0 R]", false, true),
            "Or is satisfied by A alone; /P /AllOn would have hidden it"
        );
        assert!(
            ocmd_ve_hidden("[/Or 5 0 R 6 0 R]", true, true),
            "with both off, Or is false and the content is hidden"
        );
    }

    /// `And` and `Not`, including the nesting that makes `/VE` worth
    /// having at all — no `/P` policy can express "A but not B".
    #[test]
    fn and_or_and_not_compose() {
        assert!(!ocmd_ve_hidden("[/And 5 0 R 6 0 R]", false, false));
        assert!(ocmd_ve_hidden("[/And 5 0 R 6 0 R]", false, true));
        assert!(ocmd_ve_hidden("[/Not 5 0 R]", false, false));
        assert!(!ocmd_ve_hidden("[/Not 5 0 R]", true, false));
        // A but not B — the case /P cannot express.
        let expr = "[/And 5 0 R [/Not 6 0 R]]";
        assert!(!ocmd_ve_hidden(expr, false, true), "A on, B off => visible");
        assert!(ocmd_ve_hidden(expr, false, false), "B on => hidden");
        assert!(ocmd_ve_hidden(expr, true, true), "A off => hidden");
    }

    /// ★ **An expression pdfcer cannot evaluate falls back to `/P`,
    /// rather than defaulting to visible or hidden.**
    ///
    /// §8.11.2.2 NOTE 2 tells authors to supply `/OCGs` + `/P` alongside
    /// `/VE` precisely so a reader without expression support has
    /// something correct to use. So the fallback is the behaviour the
    /// standard designed for, not a repair pdfcer invented — and it is
    /// strictly better than the alternatives, which would either discard
    /// the author's `/P` or remove content because a *hint* was
    /// unreadable.
    ///
    /// Each malformation here is checked against the `/P /AllOn` answer
    /// with one group off — which is "hidden" — and against the `Or`
    /// answer, which would be "visible". Getting `/P`'s answer is the
    /// proof that the fallback ran.
    #[test]
    fn an_unevaluable_expression_falls_back_to_the_p_policy() {
        for bad in [
            "[/Xor 5 0 R 6 0 R]", // not one of the three operators
            "[/Not 5 0 R 6 0 R]", // Not takes exactly one operand
            "[/And]",             // an operator with no operands
            "[5 0 R 6 0 R]",      // no operator at all
            "42",                 // not an array
            "[/Or (text)]",       // an operand that is neither OCG nor expression
        ] {
            assert!(
                ocmd_ve_hidden(bad, false, true),
                "{bad} must fall back to /P /AllOn, which hides when B is off"
            );
        }
        // And the control: a WELL-FORMED Or over the same state is
        // visible, so the assertions above are detecting the fallback
        // rather than an evaluator that hides everything.
        assert!(!ocmd_ve_hidden("[/Or 5 0 R 6 0 R]", false, true));
    }

    /// An absent `/VE` is the ordinary case and must reach `/P`
    /// untouched.
    #[test]
    fn an_absent_ve_leaves_the_p_policy_alone() {
        assert!(ocmd_ve_hidden("", false, true), "/P /AllOn with B off");
        assert!(!ocmd_ve_hidden("", false, false), "/P /AllOn with both on");
    }

    /// **A self-referential expression terminates.**
    ///
    /// `/VE` operands may be indirect, so an array can reference itself
    /// — legal syntax describing an infinite tree, the same hazard
    /// `/Order` carries. Without the guard this recurses until the stack
    /// ends; with it the expression is unevaluable and `/P` answers.
    #[test]
    fn a_self_referential_expression_does_not_recurse_forever() {
        let objects: Vec<(u32, Vec<u8>)> = vec![
            (
                1,
                b"<< /Type /Catalog /Pages 2 0 R /OCProperties \
                  << /OCGs [5 0 R] /D << /OFF [5 0 R] >> >> >>"
                    .to_vec(),
            ),
            (
                2,
                b"<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 10 10] >>".to_vec(),
            ),
            (3, b"<< /Type /Page /Parent 2 0 R >>".to_vec()),
            (
                4,
                b"<< /Type /OCMD /OCGs [5 0 R] /P /AnyOn /VE 6 0 R >>".to_vec(),
            ),
            (5, b"<< /Type /OCG /Name (A) >>".to_vec()),
            // The expression's only operand is the expression.
            (6, b"[/Or 6 0 R]".to_vec()),
        ];
        let doc = build_pdf(&objects);
        let graph = doc.view();
        let off_set = optional_content_default_off(&graph);
        assert!(
            oc_is_hidden(&graph, ObjId::new(4, 0), &off_set),
            "the cycle makes the expression unevaluable, so /P /AnyOn answers: A is off, so hidden"
        );
    }
    // ---- §8.11.4.4 `/AS` + `/Usage` auto-state ----

    /// A doc whose `/D` carries one `View`-event usage application over
    /// group 5, with `categories` and the group's `/Usage` supplied raw.
    /// Group 5 starts ON (nothing in `/OFF`).
    fn usage_off_set(categories: &str, usage: &str, magnification: f32) -> BTreeSet<ObjId> {
        let objects: Vec<(u32, Vec<u8>)> = vec![
            (
                1,
                format!(
                    "<< /Type /Catalog /Pages 2 0 R /OCProperties << /OCGs [5 0 R] \
                     /D << /AS [ << /Event /View /Category {categories} /OCGs [5 0 R] >> ] >> >> >>"
                )
                .into_bytes(),
            ),
            (
                2,
                b"<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 10 10] >>".to_vec(),
            ),
            (3, b"<< /Type /Page /Parent 2 0 R >>".to_vec()),
            (4, b"<< /Type /OCG /Name (unused) >>".to_vec()),
            (
                5,
                format!("<< /Type /OCG /Name (A) /Usage {usage} >>").into_bytes(),
            ),
        ];
        let doc = build_pdf(&objects);
        let graph = doc.view();
        let mut off = optional_content_default_off(&graph);
        let _ = apply_view_usage(&graph, &mut off, magnification);
        off
    }

    /// **`/View /ViewState /OFF` hides a group the `/D` state showed.**
    ///
    /// The base case: `/D` says nothing about group 5, so it is ON, and
    /// the usage application turns it off.
    #[test]
    fn a_view_state_of_off_hides_a_group_the_default_config_shows() {
        assert!(
            usage_off_set("[/View]", "<< /View << /ViewState /OFF >> >>", 1.0)
                .contains(&ObjId::new(5, 0))
        );
        assert!(
            !usage_off_set("[/View]", "<< /View << /ViewState /ON >> >>", 1.0)
                .contains(&ObjId::new(5, 0))
        );
    }

    /// ★ **`Zoom` is half-open: `min` inclusive, `max` EXCLUSIVE.**
    ///
    /// §8.11.4.4: *"If the current magnification level of the document
    /// is greater than or equal to `min` and less than `max`, the ON
    /// state shall be used; otherwise, OFF shall be used."*
    ///
    /// The exact-boundary pair is the whole point of this test. A layer
    /// banded `[1.0, 2.0)` is ON at exactly 100 % and OFF at exactly
    /// 200 %, and an implementation that used `<=` on the upper bound
    /// would differ from the standard at precisely one magnification —
    /// which nobody discovers by accident, because nobody zooms to
    /// exactly 200 % on purpose.
    #[test]
    fn zoom_bounds_are_half_open_at_both_ends() {
        let band = "<< /Zoom << /min 1.0 /max 2.0 >> >>";
        let hidden = |m| usage_off_set("[/Zoom]", band, m).contains(&ObjId::new(5, 0));
        assert!(hidden(0.999), "below min is OFF");
        assert!(!hidden(1.0), "AT min is ON — the bound is inclusive");
        assert!(!hidden(1.999), "inside the band is ON");
        assert!(hidden(2.0), "AT max is OFF — the bound is exclusive");
        assert!(hidden(2.001), "above max is OFF");
    }

    /// An absent bound is unbounded on that side (Table 102: `min`
    /// defaults to 0, `max` to infinity).
    #[test]
    fn an_absent_zoom_bound_is_unbounded() {
        let only_min = "<< /Zoom << /min 2.0 >> >>";
        assert!(!usage_off_set("[/Zoom]", only_min, 1000.0).contains(&ObjId::new(5, 0)));
        assert!(usage_off_set("[/Zoom]", only_min, 1.0).contains(&ObjId::new(5, 0)));
        let only_max = "<< /Zoom << /max 2.0 >> >>";
        assert!(!usage_off_set("[/Zoom]", only_max, 0.01).contains(&ObjId::new(5, 0)));
        assert!(usage_off_set("[/Zoom]", only_max, 2.0).contains(&ObjId::new(5, 0)));
    }

    /// An empty or inverted range is left to degrade to permanently-OFF
    /// rather than repaired.
    ///
    /// The standard imposes no `min <= max` constraint and states no
    /// recovery. Silently swapping the bounds would show content at
    /// magnifications the document's own numbers exclude — inventing an
    /// intent from a malformation.
    #[test]
    fn an_inverted_zoom_range_is_not_repaired() {
        let inverted = "<< /Zoom << /min 5.0 /max 2.0 >> >>";
        for m in [1.0, 3.0, 10.0] {
            assert!(
                usage_off_set("[/Zoom]", inverted, m).contains(&ObjId::new(5, 0)),
                "an empty range admits nothing, at any magnification"
            );
        }
    }

    /// ★ **A category the group does not carry yields NO recommendation
    /// — it does not vote `OFF`.**
    ///
    /// §8.11.4.4's aggregation sentence, read alone, makes a missing
    /// sub-dictionary an `OFF`. Its own `Print` bullet ("left
    /// unchanged") and its own EXAMPLE ("shall not be affected by zoom
    /// level changes") both say otherwise, and the clause's stated
    /// rationale for multiple applications — combining documents "and
    /// have their behaviour preserved" — is impossible under
    /// absent-means-OFF, because merging would black out every layer
    /// lacking a merged category.
    ///
    /// Here the group has a `/View` saying ON and no `/Zoom` at all,
    /// while both categories are requested. Under absent-means-OFF the
    /// group would be hidden; under the implemented reading the `/Zoom`
    /// simply abstains.
    #[test]
    fn a_category_the_group_lacks_abstains_rather_than_voting_off() {
        let off = usage_off_set("[/View /Zoom]", "<< /View << /ViewState /ON >> >>", 1.0);
        assert!(
            !off.contains(&ObjId::new(5, 0)),
            "the absent /Zoom must not veto the present /View"
        );
    }

    /// **A group with no `/Usage` at all is left exactly as `/D` had
    /// it** — in both directions, which is what "left unchanged" means
    /// and what a blanket `OFF` would break.
    #[test]
    fn a_group_with_no_usage_dictionary_keeps_its_default_state() {
        let objects: Vec<(u32, Vec<u8>)> = vec![
            (
                1,
                b"<< /Type /Catalog /Pages 2 0 R /OCProperties << /OCGs [5 0 R 6 0 R] \
                  /D << /OFF [6 0 R] /AS [ << /Event /View /Category [/View] \
                  /OCGs [5 0 R 6 0 R] >> ] >> >> >>"
                    .to_vec(),
            ),
            (
                2,
                b"<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 10 10] >>".to_vec(),
            ),
            (3, b"<< /Type /Page /Parent 2 0 R >>".to_vec()),
            (4, b"<< /Type /OCG /Name (unused) >>".to_vec()),
            (5, b"<< /Type /OCG /Name (on, no usage) >>".to_vec()),
            (6, b"<< /Type /OCG /Name (off, no usage) >>".to_vec()),
        ];
        let doc = build_pdf(&objects);
        let graph = doc.view();
        let mut off = optional_content_default_off(&graph);
        let notes = apply_view_usage(&graph, &mut off, 1.0);
        assert!(!off.contains(&ObjId::new(5, 0)), "was ON, stays ON");
        assert!(off.contains(&ObjId::new(6, 0)), "was OFF, stays OFF");
        assert_eq!(notes.groups_managed, 0, "no category decided anything");
        assert_eq!(notes.applications, 1, "the application was still examined");
    }

    /// ★ **The conjunction is global and order-independent: `OFF`
    /// dominates.**
    ///
    /// §8.11.4.4: *"If a given optional content group appears in more
    /// than one `OCGs` array, its state shall be ON only if all
    /// categories in all the usage application dictionaries it appears
    /// in shall have a state of ON."*
    ///
    /// This is the OPPOSITE algebra to the `/D` `/ON`//`/OFF` arrays in
    /// the same clause, where order decides (decision 038). Both orders
    /// are tested so a last-writer-wins implementation fails one of
    /// them, whichever way round it was written.
    #[test]
    fn two_applications_conjoin_with_off_dominating_in_either_order() {
        for (first, second) in [("/ON", "/OFF"), ("/OFF", "/ON")] {
            let objects: Vec<(u32, Vec<u8>)> = vec![
                (
                    1,
                    b"<< /Type /Catalog /Pages 2 0 R /OCProperties << /OCGs [5 0 R] /D << /AS [ \
                     << /Event /View /Category [/View] /OCGs [5 0 R] >> \
                     << /Event /View /Category [/Print] /OCGs [5 0 R] >> ] >> >> >>"
                        .to_vec(),
                ),
                (
                    2,
                    b"<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 10 10] >>".to_vec(),
                ),
                (3, b"<< /Type /Page /Parent 2 0 R >>".to_vec()),
                (4, b"<< /Type /OCG /Name (unused) >>".to_vec()),
                (
                    5,
                    format!(
                        "<< /Type /OCG /Name (A) /Usage << /View << /ViewState {first} >> \
                         /Print << /PrintState {second} >> >> >>"
                    )
                    .into_bytes(),
                ),
            ];
            let doc = build_pdf(&objects);
            let graph = doc.view();
            let mut off = optional_content_default_off(&graph);
            let _ = apply_view_usage(&graph, &mut off, 1.0);
            assert!(
                off.contains(&ObjId::new(5, 0)),
                "one OFF anywhere hides the group, whichever application carried it ({first}/{second})"
            );
        }
    }

    /// **A non-`View` event is counted and not applied.**
    ///
    /// §8.11.4.5 scopes a viewer's examination to `Event` `View`;
    /// `Print` and `Export` apply only for the duration of that
    /// operation and then revert. Applying a `/Print` dictionary at
    /// viewing time would hide content on screen that the document only
    /// asked to hide on paper.
    #[test]
    fn a_print_event_application_is_not_applied_when_viewing() {
        let objects: Vec<(u32, Vec<u8>)> = vec![
            (
                1,
                b"<< /Type /Catalog /Pages 2 0 R /OCProperties << /OCGs [5 0 R] /D << /AS [ \
                  << /Event /Print /Category [/View] /OCGs [5 0 R] >> ] >> >> >>"
                    .to_vec(),
            ),
            (
                2,
                b"<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 10 10] >>".to_vec(),
            ),
            (3, b"<< /Type /Page /Parent 2 0 R >>".to_vec()),
            (4, b"<< /Type /OCG /Name (unused) >>".to_vec()),
            (
                5,
                b"<< /Type /OCG /Name (A) /Usage << /View << /ViewState /OFF >> >> >>".to_vec(),
            ),
        ];
        let doc = build_pdf(&objects);
        let graph = doc.view();
        let mut off = optional_content_default_off(&graph);
        let notes = apply_view_usage(&graph, &mut off, 1.0);
        assert!(
            !off.contains(&ObjId::new(5, 0)),
            "a Print event does not act on screen"
        );
        assert_eq!(
            notes.non_view_events, 1,
            "and it is counted, not ignored silently"
        );
        assert_eq!(notes.applications, 0);
    }

    /// **`Event` and `Category` sharing names does not conflate them**
    /// (§8.11.4.4 NOTE 3).
    ///
    /// `<< /Event /View /Category [/Zoom] >>` is legal: the event says
    /// WHEN to apply, the category says WHICH usage entry to read. An
    /// implementation that keyed the category off the event name would
    /// read `/View` here and find nothing.
    #[test]
    fn a_view_event_can_request_a_zoom_category() {
        let off = usage_off_set("[/Zoom]", "<< /Zoom << /min 4.0 >> >>", 1.0);
        assert!(
            off.contains(&ObjId::new(5, 0)),
            "the Zoom category was read under a View event"
        );
    }

    /// **Categories pdfcer cannot evaluate are counted, not guessed.**
    ///
    /// `Language` and `User` need a locale and an identity pdfcer has no
    /// concept of; `CreatorInfo` and `PageElement` have no defined
    /// effect on state at all. Guessing at any of them would move
    /// content on the page for a reason pdfcer could not explain.
    #[test]
    fn unevaluable_categories_are_disclosed_rather_than_guessed() {
        let objects: Vec<(u32, Vec<u8>)> = vec![
            (
                1,
                b"<< /Type /Catalog /Pages 2 0 R /OCProperties << /OCGs [5 0 R] /D << /AS [ \
                  << /Event /View /Category [/Language /User] /OCGs [5 0 R] >> ] >> >> >>"
                    .to_vec(),
            ),
            (
                2,
                b"<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 10 10] >>".to_vec(),
            ),
            (3, b"<< /Type /Page /Parent 2 0 R >>".to_vec()),
            (4, b"<< /Type /OCG /Name (unused) >>".to_vec()),
            (
                5,
                b"<< /Type /OCG /Name (A) /Usage << /Language << /Lang (de-DE) >> \
                  /User << /Type /Ind /Name (someone) >> >> >>"
                    .to_vec(),
            ),
        ];
        let doc = build_pdf(&objects);
        let graph = doc.view();
        let mut off = optional_content_default_off(&graph);
        let notes = apply_view_usage(&graph, &mut off, 1.0);
        assert!(!off.contains(&ObjId::new(5, 0)), "state untouched");
        assert_eq!(notes.categories_unevaluable, 2);
        assert_eq!(notes.groups_managed, 0);
    }

    // -----------------------------------------------------------------
    // `/Link` destinations (§12.5.6.5, Table 173) — `Pass 222.0`
    // -----------------------------------------------------------------

    /// A two-page document whose page 1 carries the given `/Annots` text
    /// and whose catalog carries the given extra text (a `/Names` tree,
    /// typically). Page 1 is object 3, page 2 is object 4.
    ///
    /// Deliberately TWO pages: a one-page fixture cannot tell a correct
    /// `page_index` from a hard-coded `0`, which is the single most
    /// likely defect in a destination resolver and the reason a
    /// default-valued fixture cannot falsify a carry.
    fn two_page_doc_with_links(
        annots: &str,
        catalog_extra: &str,
        extra: &[(u32, Vec<u8>)],
    ) -> Document {
        let mut objects: Vec<(u32, Vec<u8>)> = vec![
            (
                1,
                format!("<< /Type /Catalog /Pages 2 0 R {catalog_extra} >>").into_bytes(),
            ),
            (
                2,
                b"<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 /MediaBox [0 0 612 792] \
                  /Resources << >> >>"
                    .to_vec(),
            ),
            (
                3,
                format!("<< /Type /Page /Parent 2 0 R /Annots {annots} >>").into_bytes(),
            ),
            (4, b"<< /Type /Page /Parent 2 0 R >>".to_vec()),
        ];
        objects.extend_from_slice(extra);
        build_pdf(&objects)
    }

    /// Would catch: a `/GoTo` action's `/D` array not being followed at
    /// all — the exact gap pdfcer-gui reported, where `action_type` said
    /// `GoTo` and nothing could say where.
    ///
    /// Also pins the `/FitR` view through, because a viewer that lands on
    /// the right page at the wrong zoom is a visible defect, and pins the
    /// page index to **1** (the second page) so a hard-coded zero fails.
    #[test]
    fn a_goto_action_on_a_link_resolves_to_a_page_and_a_view() {
        let doc = two_page_doc_with_links(
            "[5 0 R]",
            "",
            &[(
                5,
                b"<< /Type /Annot /Subtype /Link /Rect [10 20 110 40] \
                  /A << /S /GoTo /D [4 0 R /FitR 76 119 687 558] >> >>"
                    .to_vec(),
            )],
        );
        let graph = doc.view();
        let reader = DestinationReader::new(&graph);
        let found = page_link_destinations(&graph, ObjId::new(3, 0), &reader);

        assert_eq!(found.links_without_destination, 0);
        assert_eq!(found.links.len(), 1);
        let link = &found.links[0];
        assert_eq!(link.annots_index, 0);
        assert_eq!(link.id, Some(ObjId::new(5, 0)));
        let rect = link.rect.expect("Table 173 makes /Rect required");
        assert!((rect.llx - 10.0).abs() < 1e-9 && (rect.ury - 40.0).abs() < 1e-9);
        match &link.destination {
            Destination::Page { page_index, view } => {
                assert_eq!(*page_index, 1, "the SECOND page, not a defaulted zero");
                match view {
                    DestView::FitR {
                        left,
                        bottom,
                        right,
                        top,
                    } => {
                        // Table 151 lets any of the four be null, so
                        // each is an Option — `Some` here is half the
                        // assertion, the value is the other half.
                        assert_eq!(*left, Some(76.0));
                        assert_eq!(*bottom, Some(119.0));
                        assert_eq!(*right, Some(687.0));
                        assert_eq!(*top, Some(558.0));
                    }
                    other => panic!("expected FitR, got {other:?}"),
                }
            }
            other => panic!("expected a resolved page, got {other:?}"),
        }
    }

    /// Would catch: the `/Names → /Dests` name-tree path not being walked
    /// for a LINK, which is the half a bookmark-only resolver would have
    /// silently lacked — every by-name link would look broken.
    #[test]
    fn a_link_by_name_resolves_through_the_names_tree() {
        let doc = two_page_doc_with_links(
            "[5 0 R]",
            "/Names << /Dests 6 0 R >>",
            &[
                (
                    5,
                    b"<< /Type /Annot /Subtype /Link /Rect [0 0 50 50] \
                      /A << /S /GoTo /D (chapter-two) >> >>"
                        .to_vec(),
                ),
                (
                    6,
                    b"<< /Names [(chapter-two) [4 0 R /XYZ 0 792 null]] >>".to_vec(),
                ),
            ],
        );
        let graph = doc.view();
        let reader = DestinationReader::new(&graph);
        assert_eq!(reader.named_destination_count(), 1);
        let found = page_link_destinations(&graph, ObjId::new(3, 0), &reader);

        assert_eq!(found.links.len(), 1);
        match &found.links[0].destination {
            Destination::Page { page_index, view } => {
                assert_eq!(*page_index, 1);
                // Table 151: a NULL zoom means "retain the current
                // magnification". A viewer that read it as 0 would zoom
                // the page out of existence.
                assert!(view.zoom_is_retain(), "null zoom must mean retain");
            }
            other => panic!("expected a resolved page, got {other:?}"),
        }
    }

    /// Would catch: a link's direct `/Dest` (rather than `/A`) being
    /// ignored. Table 173 permits either, and the two are mutually
    /// exclusive — a resolver that only read `/A` would break every link
    /// written the older way.
    #[test]
    fn a_link_may_carry_dest_directly_and_it_wins_over_an_action() {
        let doc = two_page_doc_with_links(
            "[5 0 R]",
            "",
            &[(
                5,
                // Malformed on purpose: Table 173 says /Dest "shall not
                // be present if an A entry is present". pdfcer takes
                // /Dest, matching the outline path, and COUNTS the
                // conflict rather than silently picking.
                b"<< /Type /Annot /Subtype /Link /Rect [0 0 50 50] \
                  /Dest [4 0 R /Fit] \
                  /A << /S /GoTo /D [3 0 R /Fit] >> >>"
                    .to_vec(),
            )],
        );
        let graph = doc.view();
        let reader = DestinationReader::new(&graph);
        let dict_obj = graph.resolved(ObjId::new(5, 0));
        let dict = dict_obj.as_dict().expect("annotation dictionary");
        let (resolved, diagnostics) = reader.destination_with_diagnostics(&graph, dict);

        assert_eq!(diagnostics.dest_and_action_both_present, 1);
        match resolved {
            Some(Destination::Page { page_index, .. }) => {
                assert_eq!(page_index, 1, "/Dest wins, so page 2 not page 1");
            }
            other => panic!("expected /Dest to win, got {other:?}"),
        }
    }

    /// Would catch: a `/URI` link being reported as a page jump, or as
    /// nothing at all. Both are wrong in opposite directions — the first
    /// navigates the operator somewhere arbitrary, the second hides that
    /// the link exists.
    #[test]
    fn a_uri_link_is_disclosed_as_a_non_navigation_never_as_a_page() {
        let doc = two_page_doc_with_links(
            "[5 0 R]",
            "",
            &[(
                5,
                b"<< /Type /Annot /Subtype /Link /Rect [0 0 50 50] \
                  /A << /S /URI /URI (https://example.invalid/) >> >>"
                    .to_vec(),
            )],
        );
        let graph = doc.view();
        let reader = DestinationReader::new(&graph);
        let found = page_link_destinations(&graph, ObjId::new(3, 0), &reader);

        assert_eq!(found.links.len(), 1);
        assert_eq!(found.links_without_destination, 0);
        match &found.links[0].destination {
            Destination::NonNavigation { action } => {
                assert_eq!(
                    action.as_ref().map(Name::as_bytes),
                    Some(&b"URI"[..]),
                    "the action type is the disclosure"
                );
            }
            other => panic!("a /URI is not a page jump, got {other:?}"),
        }
    }

    /// Would catch: a link with neither `/Dest` nor `/A` being dropped
    /// silently, which would make a page of wholly-broken links
    /// indistinguishable from a page with no links.
    ///
    /// Sabotage note: deleting the `links_without_destination` increment
    /// makes this test fail on the counter alone — the `links` vector is
    /// empty either way, which is exactly why the counter had to exist.
    #[test]
    fn a_link_that_goes_nowhere_is_counted_not_dropped() {
        let doc = two_page_doc_with_links(
            "[5 0 R 6 0 R]",
            "",
            &[
                (
                    5,
                    b"<< /Type /Annot /Subtype /Link /Rect [0 0 50 50] >>".to_vec(),
                ),
                // A non-link annotation on the same page must not be
                // counted by either field.
                (
                    6,
                    b"<< /Type /Annot /Subtype /Square /Rect [0 0 9 9] >>".to_vec(),
                ),
            ],
        );
        let graph = doc.view();
        let reader = DestinationReader::new(&graph);
        let found = page_link_destinations(&graph, ObjId::new(3, 0), &reader);

        assert!(found.links.is_empty());
        assert_eq!(
            found.links_without_destination, 1,
            "the /Square must not be counted; the broken /Link must be"
        );
    }

    /// Would catch: `annots_index` being the position in the RESULT
    /// rather than in `/Annots`, which would make every
    /// `delete-annotation` built on it address the wrong object once a
    /// page held anything other than links.
    #[test]
    fn annots_index_is_the_array_position_not_the_result_position() {
        let doc = two_page_doc_with_links(
            "[5 0 R 6 0 R 7 0 R]",
            "",
            &[
                (
                    5,
                    b"<< /Type /Annot /Subtype /Square /Rect [0 0 9 9] >>".to_vec(),
                ),
                (
                    6,
                    b"<< /Type /Annot /Subtype /Widget /Rect [0 0 9 9] \
                      /A << /S /GoTo /D [4 0 R /Fit] >> >>"
                        .to_vec(),
                ),
                (
                    7,
                    b"<< /Type /Annot /Subtype /Link /Rect [0 0 9 9] \
                      /A << /S /GoTo /D [4 0 R /Fit] >> >>"
                        .to_vec(),
                ),
            ],
        );
        let graph = doc.view();
        let reader = DestinationReader::new(&graph);
        let found = page_link_destinations(&graph, ObjId::new(3, 0), &reader);

        assert_eq!(found.links.len(), 1, "the /Widget is deliberately excluded");
        assert_eq!(
            found.links[0].annots_index, 2,
            "third in /Annots, first in the result"
        );

        // …but the same widget IS reachable through the per-annotation
        // route, which is the documented division of labour.
        let widget = page_annotations(&graph, ObjId::new(3, 0))
            .into_iter()
            .find(Annotation::is_widget)
            .expect("the widget is on the page");
        assert!(
            matches!(
                widget.destination(&graph, &reader),
                Some(Destination::Page { page_index: 1, .. })
            ),
            "a pushbutton's /A is resolvable, just not by page_link_destinations"
        );
    }

    /// Would catch: a destination naming an object that is not a page
    /// being reported as `Page { page_index: 0 }` — the residue of a page
    /// delete, presented as a working link to the front of the document.
    #[test]
    fn a_link_to_a_deleted_page_is_unmapped_never_page_zero() {
        let doc = two_page_doc_with_links(
            "[5 0 R]",
            "",
            &[
                (
                    5,
                    // Object 6 is a plain dictionary, not a page in the tree.
                    b"<< /Type /Annot /Subtype /Link /Rect [0 0 50 50] \
                  /A << /S /GoTo /D [6 0 R /Fit] >> >>"
                        .to_vec(),
                ),
                (6, b"<< /Type /Whatever >>".to_vec()),
            ],
        );
        let graph = doc.view();
        let reader = DestinationReader::new(&graph);
        assert!(
            reader.page_tree_error().is_none(),
            "the tree itself is fine"
        );
        let found = page_link_destinations(&graph, ObjId::new(3, 0), &reader);

        match &found.links[0].destination {
            Destination::UnmappedPage { page, .. } => {
                assert_eq!(*page, Some(ObjId::new(6, 0)), "the failed id is kept");
            }
            other => panic!("expected UnmappedPage, got {other:?}"),
        }
    }
}
