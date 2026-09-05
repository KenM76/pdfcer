//! # The writer — save paths for `ARCHITECTURE.md` §5
//!
//! Two save modes, two **different** correctness contracts, and a
//! deliberate absence of any third thing.
//!
//! | Mode | Contract | Assertion shape |
//! |---|---|---|
//! | [`save_incremental`] with an empty dirty set | output is **byte-identical to the input**, whole file | `output == input` |
//! | [`save_incremental`] with a dirty set | prior bytes untouched; only an update revision appended (§7.5.6) | `output.starts_with(input)` |
//! | [`save_full`] | every `Provenance::File` object's **definition bytes** re-emitted verbatim; offsets, xref and trailer regenerated | **per object**, never per file |
//!
//! Decision 007 W1 names conflating the last two rows *"the single
//! likeliest source of a false green or a false red in this Pass"*. A
//! full rewrite **cannot** be byte-identical file-wide — object offsets
//! move, so the cross-reference section must differ. A test that asserts
//! file-level identity for [`save_full`] fails universally; a test that
//! asserts only reloadability passes vacuously. Hence R32: two
//! assertions, never one.
//!
//! ## Pass 3.0 scope: an *identity* writer, on purpose
//!
//! Pass 3.0 shipped **no editing capability of any kind** (decision 007
//! `explicit_non_goals`). What it built is:
//!
//! - the serializer ([`serialize`]), complete for every `Object`
//!   variant;
//! - both cross-reference forms ([`xref_out`]), selected by the input,
//!   never normalized (R33);
//! - the encryption seam ([`encoder`]), identity implementation (R37);
//! - the §7.5.6 append machinery, exercised through
//!   [`DirtySet::identity_reemission`] — a **verification** entry point
//!   that re-emits chosen objects *unchanged*, so the append path was
//!   corpus-proven before Pass 3.1 put real edits through it.
//!
//! ## Pass 3.1 scope: the mutation writer
//!
//! Pass 3.1 adds exactly one thing to the writer: a [`DirtySet`] may now
//! carry **replacement values** and **trailer-entry patches** as well as
//! bare re-emission requests. Three consequences, each of which was
//! unreachable before and is now a live code path:
//!
//! 1. **`/ID[1]` regeneration becomes reachable** (§14.4, R39). It fires
//!    exactly when a save writes at least one *changed* object — see
//!    [`DirtySet::changes_content`] and [`fileid`]. An identity
//!    re-emission is not a change, so the Pass 3.0 corpus gate is
//!    unperturbed by construction.
//! 2. **R38 promotion becomes reachable** (decision 007 W3). An edited
//!    object whose provenance is `ObjectStream` has no verbatim bytes to
//!    re-emit and cannot be patched in place, so it is *promoted* to an
//!    uncompressed file-level object; the old container is left
//!    untouched and the new type-1 cross-reference entry supersedes the
//!    type-2 one (§7.5.6 requirement: most recent copy wins; and for a
//!    hybrid file §7.5.8.4's search order puts the newest classic
//!    section ahead of `/XRefStm`). Every promotion is counted **and
//!    named** in [`SaveReport::promoted`] — it is a byte-level
//!    divergence the operator did not ask for, so it is disclosed.
//! 3. **New objects can be appended.** A `DirtySet` replacement whose id
//!    is absent from the base document is a *created* object (Pass 3.1's
//!    only user is "the operator set metadata on a file that had no
//!    `/Info` dictionary"). Creating one is not a fingerprint — see
//!    [`ProducerPolicy`]'s contrast note.
//!
//! Still deliberately absent, each for a stated reason: object deletion
//! and free-list writing (they need a real mutation to be tested
//! against — decision 007 W9; Pass 3.2), generation increments,
//! structural page operations, content-stream re-emission, cross-document
//! object renumbering, linearization writing, optimization, object-stream
//! authoring, and encryption.
//!
//! ## The three things a save must never do
//!
//! 1. **Never normalize** (R33, W4). Not the cross-reference form, not
//!    the header version, not number formatting, not object-stream
//!    membership. §7.5.6 does not *require* an appended section to
//!    match the base file's form — that is a recorded spec silence —
//!    which is precisely why pdfcer has to impose the rule itself.
//! 2. **Never leave a fingerprint** (R41, decision 001 §6.1
//!    obligation 6). [`save_incremental`] does not rewrite `/Info`, and
//!    does not regenerate `/ID`, on a save that changed nothing. See
//!    [`ProducerPolicy`] for the full-rewrite rule and the reasoning.
//! 3. **Never re-serialize a signature dictionary** (§12.8). A
//!    signature's `Contents` is a fixed-width placeholder covered by a
//!    `/ByteRange`; re-emitting it *even identically* is a byte-offset
//!    hazard. pdfcer's structural answer is that signed objects are, by
//!    construction, `Provenance::File` objects on the verbatim path —
//!    they are copied, never decomposed.
//!
//! ## `/ID` on save (§14.4, R39, W6)
//!
//! §14.4 says `ID[0]` *"shall not change when the file is incrementally
//! updated"* and `ID[1]` is *"a changing identifier based on the file's
//! contents at the time it was last updated."* Taken naively that
//! conflicts head-on with byte-identical round-tripping.
//!
//! It does not actually conflict, and the reasoning is worth stating
//! because it will be re-litigated: **if nothing changed, nothing was
//! "updated"**, so §14.4's trigger never fired. `/ID` is
//! `should`-strength for unencrypted files and **no `shall` anywhere
//! requires regeneration** — §14.4 states what `ID[1]` *is*, not when a
//! writer must recompute it. So pdfcer regenerates `ID[1]` exactly when
//! a save writes at least one changed object, and never otherwise; and
//! `ID[0]` changes only when pdfcer creates a document it regards as
//! new, which no save mode in this Pass does. A gratuitously
//! regenerated `/ID` is an observable "pdfcer touched this file" signal
//! on a file pdfcer did not change — R41 territory.
//!
//! Pass 3.1 makes that rule executable. [`DirtySet::changes_content`] is
//! the trigger; [`fileid::changing_identifier`] is the value; and three
//! sub-rules are enforced, each recorded because each is a judgement
//! call the spec does not make for us:
//!
//! - **A base file with no `/ID` gets none.** §14.4 is `should`-strength
//!   and the spec RAG's own analysis calls synthesizing one on an append
//!   *"a judgement call … recommend matching the base file (add
//!   nothing)"*. Adding `/ID` to a file that never had one changes the
//!   document's identity semantics on behalf of an operator who asked
//!   for a metadata edit. pdfcer adds nothing, in **both** save modes.
//!   (The RAG does recommend synthesizing on a full rewrite; pdfcer
//!   declines even there for now, because a full rewrite of an unchanged
//!   document must stay fingerprint-free under R41 and the two rules
//!   would then have to disagree by save mode. Revisit when a genuine
//!   from-scratch / "Save As new document" path exists — that is the
//!   context in which `ID[0]` legitimately changes too.)
//! - **A malformed `/ID` is left exactly as found.** §14.4 states no
//!   recovery rule for a 1- or 3-element array or a non-string element.
//!   Rewriting one would be pdfcer inventing document identity; passing
//!   it through preserves the operator's file.
//! - **`ID[0]` is never touched by either mode.** It is an input to
//!   §7.6.3.3 Algorithm 2 step (e), so an error here surfaces in Pass 5
//!   as a decryption failure that presents as a crypto bug.
//!
//! ## Redaction forbids incremental save (R35, W2)
//!
//! Recorded here because this module is where it will be enforced, and
//! because it is trust-critical and was undocumented before Pass 3.0.
//! Incremental save **structurally preserves superseded content**: the
//! old bytes of every edited object remain in the file by construction
//! (§7.5.6: *"changes shall be appended … leaving its original contents
//! intact"*). A redaction saved incrementally therefore leaves the
//! redacted content trivially recoverable — and incremental is the
//! **default** save mode. Any operation whose contract is *removal*
//! must force [`save_full`] and refuse [`save_incremental`]. See
//! `ARCHITECTURE.md` §5.
//!
//! ## Signatures interact with both modes, in opposite directions (R36, W7)
//!
//! §12.8.1 NOTE 1: *"If a signed document is modified and saved by
//! incremental update, the data corresponding to the byte range of the
//! original signature is preserved."* A full rewrite destroys every
//! signature. So signature presence forces incremental — which collides
//! head-on with R35's redaction rule. "Redact a signed document" is a
//! genuine either/or that must be surfaced to the operator, never
//! resolved silently. It belongs to the Redaction and Signatures
//! Passes; it is named here so neither Pass can claim surprise.

pub mod content;
pub mod encoder;
pub mod fileid;
mod save;
pub mod serialize;
pub mod xref_out;

use std::collections::BTreeMap;

pub use encoder::{EncryptingEncoder, IdentityEncoder, ObjectEncoder};
pub use save::{
    EncryptParams, SaveReport, save_full, save_full_decrypted, save_full_encrypted,
    save_incremental,
};
pub use xref_out::XrefOutError;

use crate::object::{Dict, Name, ObjId, Object};
use crate::settings::{TrailingEol, XrefEntryEol};

/// The set of objects a save must write into the new revision.
///
/// ## The bug this type exists to make impossible
///
/// `ARCHITECTURE.md` §11.1 and decision 007: the dirty set is a **diff
/// against the base revision, computed at save time** — never the union
/// of every command run during the session. The difference is not
/// academic. An object edited and then *undone* is in the union but not
/// in the diff; including it appends a revision that silently re-states
/// the object's original value, bloating every save after any undo and
/// breaking the "objects pdfcer didn't logically touch are omitted"
/// half of the §5 invariant. §7.5.6 requirement 1 is the spec-side
/// reason: *"A cross-reference section for an incremental update shall
/// contain entries **only for** objects that have been changed,
/// replaced, or deleted."* Read the modality carefully — "only for" is
/// a restriction, not merely permission to omit.
///
/// ## Two kinds of entry, and why both live in one type
///
/// - A **re-emission** ([`DirtySet::identity_reemission`]) asks the
///   writer to write an object's *existing* value into the new
///   revision. It is a verification device, not an edit.
/// - A **replacement** ([`DirtySet::replace`]) carries a new value.
///   This is what an actual operator edit produces, and it is the only
///   entry kind that counts as "changed" for
///   [`DirtySet::changes_content`] — the `/ID[1]` trigger (§14.4).
///
/// Plus a **trailer patch** ([`DirtySet::patch_trailer`]): entries the
/// new revision's trailer must carry with a different value than the
/// base trailer had. The trailer is part of a revision, not an object,
/// so it cannot be expressed as an entry — but it *is* part of the same
/// save-time diff, and threading a second parameter through every save
/// signature would only hide that.
///
/// ## Building one
///
/// Nothing here computes the diff; [`crate::edit::EditSession`] does,
/// and it is the only thing that should. This type is the *result* of
/// that computation and the writer's input. Constructing one by hand is
/// legitimate for tests and for the verification modes, and is exactly
/// how the "union of every command ever run" bug would be reintroduced
/// if an edit path ever built one directly — so don't.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DirtySet {
    entries: BTreeMap<ObjId, Change>,
    trailer_patch: Dict,
    /// Authored stream bytes the session staged (R45, Pass 6.1). Empty
    /// for every pre-6.1 edit, which keeps their save path byte-for-byte
    /// unchanged (the combined-source construction below is skipped when
    /// this is empty).
    ///
    /// ## The combined coordinate system (R45, X5)
    ///
    /// A `Change::Replace` value may be — for the first time as of
    /// Pass 6.1 — a [`Stream`](crate::object::Stream) whose `data_span`
    /// points not into the base file but into *this* buffer: an authored
    /// appearance stream. To keep [`crate::object::Stream`]'s span model
    /// (R45 forbids an owned-bytes `Stream` variant), authored spans are
    /// expressed in a **single combined coordinate system** — an absolute
    /// offset of `base.len() + local`, where `local` is the byte offset
    /// within this staging buffer. At save time the writer serializes
    /// replacement objects against `base ++ staging`, in which a base span
    /// (`< base.len()`) resolves in the prefix and an authored span
    /// resolves in the suffix. See [`DirtySet::combined_source`].
    staging: Vec<u8>,
}

/// What the writer must do with one named object. Private: the public
/// surface is [`DirtySet::replacement`] / [`DirtySet::contains`] /
/// [`DirtySet::is_deleted`], which is exactly why adding the `Delete`
/// variant in Pass 3.2 was not a breaking change — the note that
/// predicted it is left above as evidence the encapsulation paid for
/// itself.
#[derive(Debug, Clone, PartialEq)]
enum Change {
    /// Write the object's existing value into the new revision.
    Reemit,
    /// Write this value instead of the object's existing one. Also used
    /// for an object that does not exist in the base document at all.
    Replace(Object),
    /// **Delete** the object: emit no body at all, and give it a type-0
    /// (free) cross-reference entry (§7.5.4) so every later reference to
    /// it resolves to `null` (§7.3.10).
    ///
    /// Pass 3.2's addition, and the one that decision 007 W9 warns about:
    /// *"A malformed type-0 free chain produces files Acrobat tolerates
    /// and stricter readers reject — the worst failure shape, because
    /// the obvious test passes."* See [`save`]'s free-list section for
    /// the discipline this variant obliges.
    Delete,
}

impl DirtySet {
    /// The empty dirty set — "this document has no unsaved changes".
    ///
    /// With this, [`save_incremental`] produces a file **byte-identical
    /// to the input**: zero edits means zero bytes, not "the input plus
    /// an empty revision".
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// Build a dirty set that asks the writer to **re-emit the named
    /// objects unchanged**.
    ///
    /// This is a **verification** constructor, not an editing API, and
    /// the distinction is worth being precise about. It changes no
    /// object's value; it drives the §7.5.6 append machinery — object
    /// re-emission, update-section construction, `/Prev` chaining,
    /// trailer copying, `startxref` placement — over real files so that
    /// path is corpus-proven *before* genuine edits go through it. The
    /// resulting file is semantically identical to the input by
    /// construction, which gives the round-trip harness a strong oracle:
    /// it must reload to the same object graph and re-render to the same
    /// pixels.
    ///
    /// Without this, Pass 3.0's headline gate (an empty dirty set
    /// producing a byte-identical file) would exercise a `memcpy` and
    /// nothing else, and the append writer would have shipped untested.
    ///
    /// Because it changes nothing, a set built this way answers `false`
    /// to [`DirtySet::changes_content`] and therefore does **not**
    /// regenerate `/ID[1]`.
    #[must_use]
    pub fn identity_reemission(ids: impl IntoIterator<Item = ObjId>) -> Self {
        Self {
            entries: ids.into_iter().map(|id| (id, Change::Reemit)).collect(),
            trailer_patch: Dict::new(),
            staging: Vec::new(),
        }
    }

    /// Record that `id` must be written with `value` instead of whatever
    /// it holds in the base revision.
    ///
    /// `id` need not exist in the base document: an id the base does not
    /// define is a **created** object, which the writer appends and
    /// gives a fresh cross-reference entry. That is how a metadata edit
    /// on a file with no `/Info` dictionary reaches the file.
    ///
    /// Replacing supersedes a previously recorded re-emission for the
    /// same id, and a second replacement supersedes the first — so N
    /// edits to one object produce exactly **one** entry in the update
    /// section, never N. That coalescing is structural (a map keyed by
    /// id), not a special case.
    pub fn replace(&mut self, id: ObjId, value: Object) {
        self.entries.insert(id, Change::Replace(value));
    }

    /// Record that `id` must be **deleted**: no body in the new
    /// revision, and a type-0 (free) cross-reference entry that makes
    /// every reference to it resolve to `null` (§7.3.10, §7.5.4).
    ///
    /// ## Deleting is not redacting, and this method is not a shortcut
    ///
    /// A deletion recorded here is a *structural* removal — the object
    /// leaves the document graph. Under [`save_incremental`] its
    /// previous bytes remain in the file by construction (§7.5.6 appends,
    /// it does not erase), and under [`save_full`] they are simply not
    /// re-emitted. Neither is redaction, and `ARCHITECTURE.md` §5.7
    /// records why full rewrite is not sufficient there either
    /// (object-stream containers carry through verbatim in both modes).
    /// Front ends must say so — pdfce-gui's delete tooltip does.
    ///
    /// ## Generation numbers are the writer's business, not the caller's
    ///
    /// §7.5.4 makes a free entry's generation *"the generation number to
    /// be used if the object … is reused"*, i.e. one more than the
    /// deleted object's. The caller supplies only the id it wants gone;
    /// the increment (and its 65,535 saturation, which marks a number
    /// permanently unreusable) happens in [`save`], once, where the
    /// free-list chain is built. Letting callers pass a generation would
    /// be an invitation to get that arithmetic wrong in three places.
    ///
    /// A delete supersedes any previously recorded change for the same
    /// id, and vice versa — the map is keyed by id, so the last word
    /// wins, which is what an editing session's save-time diff means.
    pub fn delete(&mut self, id: ObjId) {
        self.entries.insert(id, Change::Delete);
    }

    /// Whether `id` is recorded for deletion.
    #[must_use]
    pub fn is_deleted(&self, id: ObjId) -> bool {
        matches!(self.entries.get(&id), Some(Change::Delete))
    }

    /// Every id recorded for deletion, ascending.
    ///
    /// Ascending because the free-list chain the writer builds from this
    /// runs in increasing object-number order — not a `shall` (§7.5.4
    /// only requires a well-formed linked list) but the form Annex H's
    /// examples use, and the one that keeps output deterministic and
    /// therefore byte-comparison-testable.
    pub fn deletions(&self) -> impl Iterator<Item = ObjId> + '_ {
        self.entries
            .iter()
            .filter(|(_, change)| matches!(change, Change::Delete))
            .map(|(id, _)| *id)
    }

    /// Record that the new revision's trailer must carry `key` with
    /// `value`.
    ///
    /// §7.5.6 requirement 3 makes an update trailer carry *"all the
    /// entries except the `Prev` entry … from the previous trailer,
    /// whether modified or not"*, so the writer starts from the base
    /// trailer and applies this patch over it. Only genuinely changed
    /// keys belong here — a patch that restates a base value is a
    /// no-op that would still flip [`DirtySet::changes_content`], and
    /// therefore would regenerate `/ID[1]` on a document nothing
    /// changed in.
    pub fn patch_trailer(&mut self, key: Name, value: Object) {
        self.trailer_patch.insert(key, value);
    }

    /// Whether the set names nothing at all — the condition under which
    /// [`save_incremental`] guarantees whole-file byte identity.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty() && self.trailer_patch.is_empty()
    }

    /// How many object definitions the next revision will carry.
    ///
    /// Deliberately excludes the trailer patch: the trailer is not an
    /// object and does not get a cross-reference entry.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether this save writes at least one **changed** thing — the
    /// §14.4 / R39 trigger for regenerating `/ID[1]`.
    ///
    /// A pure identity re-emission answers `false`: it writes objects,
    /// but it changes none of them, so §14.4's *"at the time it was last
    /// updated"* trigger never fired. Keeping these two questions
    /// separate is what lets Pass 3.0's `append-identity` corpus mode
    /// stay byte-stable while a real edit correctly refreshes `/ID[1]`.
    #[must_use]
    pub fn changes_content(&self) -> bool {
        !self.trailer_patch.is_empty()
            || self
                .entries
                .values()
                .any(|c| matches!(c, Change::Replace(_) | Change::Delete))
    }

    /// The replacement value recorded for `id`, or `None` if this id is
    /// absent or is a bare re-emission.
    #[must_use]
    pub fn replacement(&self, id: ObjId) -> Option<&Object> {
        match self.entries.get(&id) {
            Some(Change::Replace(value)) => Some(value),
            _ => None,
        }
    }

    /// Whether `id` is named at all (as a replacement or a re-emission).
    #[must_use]
    pub fn contains(&self, id: ObjId) -> bool {
        self.entries.contains_key(&id)
    }

    /// The trailer entries the new revision must carry with a value
    /// different from the base trailer's.
    #[must_use]
    pub const fn trailer_patch(&self) -> &Dict {
        &self.trailer_patch
    }

    /// Iterate the named objects in ascending `(num, generation)`
    /// order.
    ///
    /// Ascending order is not cosmetic: it is what lets the
    /// cross-reference emitters group entries into runs, which
    /// §7.5.8.2 turns into a `shall` for cross-reference streams
    /// (*"The array shall be sorted in ascending order by object
    /// number"*).
    pub fn iter(&self) -> impl Iterator<Item = ObjId> + '_ {
        self.entries.keys().copied()
    }

    /// Attach the session's authored-stream staging buffer (R45).
    ///
    /// Called by [`crate::edit::EditSession::dirty_set`] when the session
    /// carries authored streams. The buffer's bytes are the raw
    /// (still-filter-encoded, but Pass 6.1 authors raw appearances)
    /// payloads that replacement `Stream` values' spans index into, in the
    /// `base.len() + local` combined coordinate system documented on the
    /// field.
    pub fn set_staging(&mut self, staging: Vec<u8>) {
        self.staging = staging;
    }

    /// The authored-stream staging buffer (empty for a pre-6.1 edit).
    #[must_use]
    pub fn staging(&self) -> &[u8] {
        &self.staging
    }

    /// The serialization source a replacement object's stream spans index
    /// into (R45): `base` alone when nothing was authored (zero-copy, the
    /// unchanged pre-6.1 path), or `base ++ staging` when authored streams
    /// are present.
    ///
    /// The combined buffer is sound because a base span (`< base.len()`)
    /// resolves in the prefix — identical to `base` — while an authored
    /// span (`>= base.len()`, i.e. `base.len() + local`) resolves in the
    /// appended suffix. `base` **must** be the same buffer the session's
    /// authored offsets were computed against, i.e. the document passed to
    /// the save function; the writer guarantees this by passing
    /// `doc.bytes()`.
    #[must_use]
    pub fn combined_source<'a>(&'a self, base: &'a [u8]) -> std::borrow::Cow<'a, [u8]> {
        if self.staging.is_empty() {
            std::borrow::Cow::Borrowed(base)
        } else {
            let mut combined = Vec::with_capacity(base.len() + self.staging.len());
            combined.extend_from_slice(base);
            combined.extend_from_slice(&self.staging);
            std::borrow::Cow::Owned(combined)
        }
    }
}

/// What [`save_full`] does with `/Producer` (§14.3.3 Table 317).
///
/// ## Why this is a policy knob and not a constant
///
/// Decision 001 §6.1 obligation 6 — restated as decision 007's R41 at
/// its actual enforcement point, which is this module — requires that
/// pdfcer leave **no non-suppressible output fingerprint**. No build
/// hash, no edition marker, no producer id the operator cannot turn
/// off. This is the structural prevention of the specific behavior that
/// disqualified `oxidize-pdf` as a foundation for pdfcer.
///
/// [`save_incremental`] has no such knob **by construction**: it never
/// rewrites `/Info` at all, because doing so would mean appending a
/// revision to a document the operator did not change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum ProducerPolicy {
    /// Emit `/Producer (pdfcer <version>)` into an existing document
    /// information dictionary. The default for a full rewrite, which is
    /// an operator-initiated act of authorship rather than a
    /// passthrough.
    ///
    /// **Only rewrites an `/Info` that already exists.** If the base
    /// document has no information dictionary, none is created. That is
    /// a deliberate narrowing of "sets `/Producer`": manufacturing an
    /// `/Info` object on a file that had none is exactly the
    /// stamp-our-name-on-everything behavior R41 exists to prevent, in
    /// miniature. The narrowing is recorded rather than silent.
    ///
    /// ## Contrast with an operator metadata edit (Pass 3.1)
    ///
    /// [`crate::edit::EditSession::set_info_field`] **does** create an
    /// `/Info` dictionary when the base file has none, and that is not
    /// an inconsistency. R41 forbids pdfcer writing *its own* identity
    /// into a file the operator did not ask it to mark. An operator who
    /// types a document title has asked, explicitly, for exactly that
    /// dictionary to exist and to say exactly that. The distinction the
    /// rule actually draws is **who authored the value**, not whether
    /// the object is new:
    ///
    /// | | writes `/Info` | creates `/Info` | why |
    /// |---|---|---|---|
    /// | `ProducerPolicy::Set` | yes | **no** | pdfcer authored it; creating a whole object to hold a producer id is a fingerprint |
    /// | operator metadata edit | yes | **yes** | the operator authored it and asked for it by name |
    /// | `save_incremental`, no edit | **no** | no | nothing changed, so nothing is written |
    #[default]
    Set,
    /// Leave `/Info` byte-untouched. Required for any byte-comparison
    /// harness, and available to operators who want a rewrite that is
    /// as close to a passthrough as a rewrite can be.
    Preserve,
}

/// The producer string [`ProducerPolicy::Set`] writes.
///
/// Deliberately just the crate name and version: **no build hash, no
/// timestamp, no edition marker, no host identifier** (R41). Anything
/// that varies between two builds of the same version would make
/// pdfcer's output non-reproducible and would be a fingerprint.
#[must_use]
pub fn producer_string() -> String {
    format!("pdfcer {}", env!("CARGO_PKG_VERSION"))
}

/// Options controlling a save.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct SaveOptions {
    /// `/Producer` handling for [`save_full`]. Ignored by
    /// [`save_incremental`], which never touches `/Info`.
    pub producer: ProducerPolicy,
    /// Which of §7.5.4's three permitted two-byte terminators ends a
    /// classic cross-reference **entry** (spec ambiguity `EOL-A1`, R169).
    ///
    /// Default [`XrefEntryEol::SpaceLf`] — the form pdfcer has always
    /// emitted. **Evidence tier (c), downgrade pending**: the spec RAG
    /// calls `SP LF` *"the common choice"* but the register's §11.3 flags
    /// that the claim carries no citation and should either gain one or
    /// drop to tier (d).
    ///
    /// Applies to **newly written** cross-reference entries only, so an
    /// incremental save changes nothing about the base revision's bytes.
    pub xref_entry_eol: XrefEntryEol,
    /// Whether an end-of-line byte follows the final `%%EOF` (spec
    /// ambiguity `EOL-A2`, R169).
    ///
    /// Default [`TrailingEol::Lf`] — what pdfcer has always emitted.
    /// **Evidence tier (d)**, a reasoned guess and the safe side of one:
    /// §7.2.3 needs an EOL before a following `N G obj` on the append
    /// path anyway, and a trailing EOL never breaks a backward `%%EOF`
    /// scan.
    pub trailing_eol: TrailingEol,
}

impl SaveOptions {
    /// Options that leave every byte pdfcer did not have to change
    /// exactly as it was — the settings the round-trip harness and the
    /// per-object byte-identity gate run under.
    ///
    /// The two §7.5 end-of-line knobs stay at their **defaults** here
    /// rather than tracking the operator's persisted choice, and that is
    /// deliberate: `identity()` names a byte-comparison posture, and a
    /// harness whose expected bytes depend on a settings file is a harness
    /// that fails on one machine and passes on another. An operator-facing
    /// save path applies the persisted values explicitly; this one does
    /// not, and says so.
    #[must_use]
    pub fn identity() -> Self {
        Self {
            producer: ProducerPolicy::Preserve,
            xref_entry_eol: XrefEntryEol::default(),
            trailing_eol: TrailingEol::default(),
        }
    }

    /// Set the classic cross-reference entry terminator (`EOL-A1`),
    /// consuming and returning `self`.
    ///
    /// **BYTES blast radius.** Every value §7.5.4 permits is conforming,
    /// so this changes bytes and nothing else; it exists so an operator
    /// matching another tool's output byte-for-byte can.
    #[must_use]
    pub const fn with_xref_entry_eol(mut self, eol: XrefEntryEol) -> Self {
        self.xref_entry_eol = eol;
        self
    }

    /// Set whether a trailing end-of-line follows `%%EOF` (`EOL-A2`),
    /// consuming and returning `self`.
    #[must_use]
    pub const fn with_trailing_eol(mut self, eol: TrailingEol) -> Self {
        self.trailing_eol = eol;
        self
    }

    /// Set the `/Producer` policy, consuming and returning `self`.
    ///
    /// A setter rather than a struct-literal field because
    /// `SaveOptions` is `#[non_exhaustive]`: downstream crates
    /// (`pdfcer`, `pdfce-gui`, the round-trip harness) cannot use a
    /// struct expression at all, and adding an option in a later Pass
    /// must not be a breaking change for them.
    #[must_use]
    pub const fn with_producer(mut self, producer: ProducerPolicy) -> Self {
        self.producer = producer;
        self
    }
}

/// Why a save could not be completed.
///
/// Every variant is a **named, counted refusal** rather than a
/// best-effort degradation — the R27 fail-clean posture applied to the
/// write side. A writer that guesses produces a plausible, working,
/// wrong file, which is the worst possible failure shape for this
/// subsystem (decision 007 W4).
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum WriteError {
    /// File I/O failed.
    #[error("I/O error writing PDF: {0}")]
    Io(#[from] std::io::Error),
    /// A cross-reference section could not be emitted.
    #[error(transparent)]
    Xref(#[from] XrefOutError),
    /// An object's retained `ByteSpan` does not lie inside the
    /// document's buffer — a provenance bug, and one that must never be
    /// papered over by emitting a truncated object.
    #[error("object {id}: retained byte span {span} lies outside the source buffer")]
    SpanOutOfRange {
        /// The object whose provenance is broken.
        id: ObjId,
        /// The span that could not be sliced.
        span: crate::span::ByteSpan,
    },
    /// A save was requested on a session that has a **deferred redaction
    /// pending** ([`crate::edit::EditSession::apply_redactions_deferred`],
    /// `Pass 250.2`).
    ///
    /// A deferred redaction leaves the un-redacted content in the live
    /// session so the operator's undo history is preserved; the removal is
    /// carried out only by
    /// [`crate::edit::EditSession::save_applying_redaction`], which runs the
    /// surgery over the current state and emits clean bytes. Both ordinary
    /// save modes are refused meanwhile: an **incremental** save would append
    /// a delta over the un-redacted base and leak the content via `/Prev`
    /// (`ARCHITECTURE.md` §5.2), and a **full** rewrite would emit the marks
    /// with the content still present (an unapplied redaction — a footgun, not
    /// a leak). Refusing by name, and pointing at the verb that is safe, is the
    /// honest outcome.
    #[error(
        "a deferred redaction is pending; ordinary saves are refused to avoid \
         leaking (incremental) or silently not applying it (full rewrite) — \
         use EditSession::save_applying_redaction, or cancel_pending_redaction first"
    )]
    RedactionPending,
    /// A full rewrite was requested for a **hybrid-reference** file
    /// (§7.5.8.4).
    ///
    /// A hybrid file is a three-part unit — main classic table, update
    /// classic table, and a cross-reference stream carrying the hidden
    /// objects — that §7.5.8.4 says a writer *"creates … at the same
    /// time"*. Rebuilding that unit from a merged view requires
    /// re-deriving which objects were hidden and re-checking §7.5.8.4's
    /// recursive visibility rule, which is Pass 3.2 work at the
    /// earliest. Normalizing the file to a single non-hybrid section
    /// instead would destroy its pre-1.5 readability, which R33 forbids
    /// outright.
    ///
    /// So: refuse, by name, and count it. Incremental save of a hybrid
    /// file **is** supported — as a classic update section carrying
    /// `/XRefStm` forward (§7.5.8.4 form A).
    #[error(
        "full rewrite of a hybrid-reference file (§7.5.8.4) is not supported; \
         use incremental save, which appends a conforming classic update section"
    )]
    HybridFullRewrite,
    /// A full rewrite was asked to build a cross-reference table up to an
    /// object number beyond [`save::MAX_REWRITE_OBJECT_NUMBER`].
    ///
    /// §7.5.4 requires a single-section full rewrite to carry one entry
    /// per object number from 0 to the highest defined in the file, so
    /// the writer's cost is set by the largest object NUMBER rather than
    /// by how many objects the file contains. A small file naming one
    /// enormous number therefore asks for an enormous table: pdfium's
    /// `bug_455199.pdf` is 1.2 KB, names `2147483648 0 obj`, and would
    /// require 2,147,483,649 entries — measured at ~27 MB/s of steady
    /// allocation with the CPU pinned, i.e. about an hour of apparent
    /// progress before the allocator gives up.
    ///
    /// Refusing by name is the honest outcome, and the alternatives are
    /// both worse: grinding is an unrecoverable freeze in the GUI, and
    /// emitting a sparse table instead would break §7.5.4's completeness
    /// requirement — trading a hang for a malformed file.
    ///
    /// **Reading such a file is unaffected**; this bounds the writer
    /// only. `inspect` and `extract-text` both succeed on the file above.
    #[error(
        "object number {num} exceeds the largest a full rewrite will build a \
         cross-reference table up to ({max}, ISO 32000-1 Annex C Table C.1 \
         maximum indirect objects); §7.5.4 requires one entry per number from \
         0, so this file would need a table of {} entries",
        u64::from(*num) + 1
    )]
    ObjectNumberTooLarge {
        /// The highest object number the document defines.
        num: u32,
        /// The bound that was exceeded.
        max: u32,
    },
    /// The document's cross-reference table names an object the loader
    /// did not parse — the table and the object map disagree.
    #[error("cross-reference entry for object {num} has no corresponding parsed object")]
    MissingObject {
        /// The object number with no parsed definition.
        num: u32,
    },
    /// An object named in the dirty set is not present in the document.
    #[error("object {id} is named in the dirty set but is not present in the document")]
    UnknownDirtyObject {
        /// The object that could not be re-emitted.
        id: ObjId,
    },
    /// [`save_incremental`] was requested for a document loaded via
    /// **cross-reference recovery** (decision 013).
    ///
    /// A recovered document had a **broken** base cross-reference table, so
    /// an incremental append is not merely undesirable but structurally
    /// impossible: the appended section's `/Prev` would have to point at a
    /// cross-reference section that does not correctly exist, and the
    /// preserved base bytes are not a valid revision to build on. Its save
    /// is therefore a mandatory **full rewrite** ([`save_full`], which
    /// emits a fresh valid classic cross-reference); incremental is refused
    /// by name here.
    ///
    /// This is the recovered-base-forces-full-rewrite standing rule
    /// **R67** (decision 013 §9; librarian-assigned — "R59" was proposed
    /// but R59/R60/R61 were already taken, so R67 is the assigned number).
    /// It is a sibling of R35 (redaction forces full rewrite) and R58
    /// (removal/scrub forces full rewrite): §5.6's "never normalize" does
    /// not bind a recovered file, because its base was already invalid.
    #[error(
        "incremental save of a recovered document is refused; its base cross-reference \
         was invalid, so its save must be a full rewrite (save_full)"
    )]
    RecoveredBaseForbidsIncremental,
    /// A save was requested for a document loaded from an **encrypted** file
    /// (7.6). pdfcer can decrypt such a file but cannot yet write one.
    ///
    /// # Why this is a refusal and not a best effort
    ///
    /// After [`Document`](crate::document::Document) decrypts a file, its two
    /// halves deliberately disagree. Stream data was decrypted **in the
    /// retained buffer** (RC4 preserves length, so the plaintext fits exactly
    /// where the ciphertext was and every span stays true); strings were
    /// decrypted **in the parsed objects**, because a decrypted string cannot
    /// generally be re-escaped into the same number of source bytes.
    ///
    /// Both save modes re-emit untouched objects verbatim from their source
    /// span (R32). Doing that here would produce a file whose `/Encrypt`
    /// dictionary still claims the document is encrypted, whose streams are
    /// plaintext, and whose strings are ciphertext. That is not a partly-saved
    /// document — it is one that **no reader can open, including pdfcer**, and
    /// it would look like a successful save.
    ///
    /// The alternatives were considered and rejected for this increment.
    /// Re-encrypting on save needs the file key, which the document
    /// deliberately does not retain, and would emit RC4 — which pdfcer never
    /// writes (**W14**). Stripping `/Encrypt` and saving plaintext would
    /// silently remove protection the author applied, which is precisely the
    /// kind of decision rule 4 forbids taking on the operator's behalf.
    ///
    /// So: reading encrypted documents works, and writing them is named,
    /// scoped, unfinished work rather than a surprise at save time.
    #[error(
        "this document was loaded from an encrypted file; pdfcer can read encrypted \
         documents but cannot yet write them, so saving is refused rather than \
         producing a file that claims encryption it does not have"
    )]
    EncryptedSaveUnsupported,
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn empty_dirty_set_is_the_default() {
        assert!(DirtySet::default().is_empty());
        assert!(DirtySet::empty().is_empty());
        assert_eq!(DirtySet::empty().len(), 0);
    }

    #[test]
    fn dirty_set_iterates_in_ascending_order() {
        // §7.5.8.2 makes ascending order a `shall` for xref-stream
        // /Index; sourcing it from a BTreeSet makes it structural.
        let d =
            DirtySet::identity_reemission([ObjId::new(9, 0), ObjId::new(2, 1), ObjId::new(2, 0)]);
        let got: Vec<ObjId> = d.iter().collect();
        assert_eq!(
            got,
            vec![ObjId::new(2, 0), ObjId::new(2, 1), ObjId::new(9, 0)]
        );
        assert_eq!(d.len(), 3);
    }

    #[test]
    fn producer_string_carries_no_build_fingerprint() {
        // R41: no hash, no timestamp, no host. Two builds of the same
        // version must produce identical bytes.
        let p = producer_string();
        assert!(p.starts_with("pdfcer "));
        assert!(!p.contains('+'), "build metadata leaked: {p}");
        assert!(
            p.chars().all(|c| c.is_ascii_graphic() || c == ' '),
            "non-ASCII in producer string: {p}"
        );
    }

    #[test]
    fn identity_options_preserve_the_information_dictionary() {
        assert_eq!(SaveOptions::identity().producer, ProducerPolicy::Preserve);
        assert_eq!(SaveOptions::default().producer, ProducerPolicy::Set);
    }
}
