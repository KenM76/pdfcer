//! # `attachments` — reading a document's embedded files, both kinds
//!
//! A PDF can carry another file inside it in **two structurally unrelated
//! ways**, and this module's whole reason to exist is that conflating them
//! is easy, tempting, and wrong:
//!
//! 1. **Document-level embedded files.** The catalog's `/Names` dictionary
//!    (§7.7.4, Table 31) has an `/EmbeddedFiles` entry holding a **name
//!    tree** (§7.9.6) whose keys are file names and whose values are
//!    **file specification dictionaries** (§7.11.3, Table 44). Each
//!    filespec's `/EF` entry points at an **embedded file stream**
//!    (§7.11.4, Table 45) that holds the actual bytes. These belong to the
//!    document as a whole. They are not on any page, no page displays
//!    them, and deleting every page in the document leaves all of them
//!    intact. A "PDF Portfolio" is this mechanism plus a `/Collection`
//!    dictionary — so a portfolio's contents show up here as ordinary
//!    document-level attachments, with nothing special about them at this
//!    layer.
//!
//! 2. **Page-level file-attachment annotations.** A `/FileAttachment`
//!    annotation (§12.5.6.15) is a markup annotation pinned to a rectangle
//!    on one specific page. Its `/FS` entry is a file specification — the
//!    same `/Type /Filespec` shape as above — and reaches an embedded file
//!    stream the same way. But it lives in that page's `/Annots` array, it
//!    draws an icon (`/Paperclip`, `/PushPin`, `/Graph`, `/Tag`) at a
//!    location, and **deleting the page deletes the attachment with it**.
//!
//! They differ in lifetime, in what a page operation does to them, in what
//! `/P`-bit tier an Acrobat-like permission model puts them behind, and in
//! where a save has to write them back. A caller that cannot tell them
//! apart will eventually delete one thinking it deleted the other. So
//! [`Attachment::kind`] is not decoration: it is the field that makes the
//! rest of the struct safe to act on.
//!
//! ## The contract
//!
//! [`list_attachments`] returns **both kinds in one list**, each labelled.
//! There is deliberately no "list document-level attachments" function and
//! no "list page attachments" function, because a caller answering *"what
//! does this document contain?"* that had to make two calls would sooner or
//! later make one. The single call is the safe default; the discrimination
//! is carried in the data.
//!
//! [`attachment_bytes`] / [`extract_attachment`] fetch the payload. They
//! need a [`DocumentView`] rather than a bare [`ObjectGraph`] because a
//! stream in pdfcer stores a [`ByteSpan`](crate::span::ByteSpan) into a
//! buffer, not the bytes themselves ([`crate::view`] explains why), and an
//! object graph alone has no buffer to slice.
//!
//! ## ⚠️ pdfcer never RUNS an attachment
//!
//! This module reads. It does not open, launch, execute, interpret,
//! shell-out-to, or hand to the operating system anything it finds. An
//! embedded file stream is an opaque byte payload here and stays one. That
//! is not an accident of scope — an attachment is fully attacker-controlled
//! content that arrived inside a document the operator merely opened, and
//! the `.exe`/`.bat`/`.js` payload delivered by PDF attachment is one of
//! the oldest live malware vectors there is. Acrobat's own posture is a
//! maintained whitelist plus a blacklist of never-openable types with an
//! explicit warning dialog on the boundary
//! (`Acrobat_Features/attachments__file_level_and_annotation_level.md`);
//! pdfcer's posture at *this* layer is simpler and stricter — there is no
//! open path at all. If a future Pass adds "open with the system handler",
//! it owns that gate in full, and it does not get to reach it through this
//! module.
//!
//! ## ⚠️ Attachment names are attacker-controlled text
//!
//! See [`Attachment::name`] and [`sanitize_attachment_name`]. The short
//! form: `name` is reported **exactly as the document spells it**,
//! including `..\..\Windows\System32\evil.exe`, an embedded NUL, or
//! `CON.txt`. Anything writing a file must go through
//! [`sanitize_attachment_name`] and must show the operator what changed.
//!
//! ## The listing is best-effort BY THE STANDARD'S OWN ADMISSION
//!
//! This is not a pdfcer limitation and cannot be engineered away. §7.11.7
//! NOTE 1 says outright that it is *"not possible, in general, to find all
//! file specification strings … there is no way to determine whether a
//! given string is a file specification string"*, and NOTE 3 adds that a
//! filespec written as a **direct** object *"may not be possible to
//! locate … neither self-typed nor necessarily reachable by any standard
//! path of object references"*. Related: §7.11.4.1's two association
//! routes are alternatives ("may"), and no `shall` requires an embedded
//! file to appear in `/EmbeddedFiles` at all.
//!
//! Three consequences, all load-bearing:
//!
//! - **Both roots must be walked, and neither is a superset of the other.**
//!   That is why [`list_attachments`] does both rather than offering a
//!   choice.
//! - A listing means *"the attachments reachable by the two standard
//!   paths"*, not *"every byte of embedded file in this document"*. A UI
//!   should not promise the latter.
//! - A cross-reference sweep for `/Type /Filespec` objects (blessed, but
//!   only conditionally, by §7.11.7 NOTE 2) would find some of the rest.
//!   pdfcer does not do it, and if it ever does it should be opt-in — it
//!   would surface filespecs that are unreferenced garbage as readily as
//!   real ones.
//!
//! ## Duplicate names are legal and are NOT a de-duplication key
//!
//! §7.11.7 NOTE 7: *"The same file name, such as `readme.txt`, may be
//! associated with different embedded files in distinct file
//! specifications."* Two rows with the same [`Attachment::name`] are
//! therefore ordinary, not a bug and not a reason to collapse them.
//! Identity, when a caller needs it, is [`Attachment::stream_id`] (or
//! [`Attachment::filespec_id`]) — never the name.
//!
//! ## `/Params /Size` is a DECLARED size, not a measured one
//!
//! §7.11.4's embedded-file parameter dictionary carries `/Size`, defined
//! as *"the size of the **uncompressed** embedded file, in bytes"*. It is
//! **Optional**, and — the part that matters — the standard attaches no
//! `shall` requiring it to agree with the stream and states **no reader
//! behaviour on a mismatch**. So a disagreement is a *fact pdfcer measured*,
//! not a conformance verdict pdfcer is entitled to pronounce. A file may
//! declare 4 GB and carry ten bytes. pdfcer reports it as
//! [`Attachment::declared_size`] — named *declared* so a caller cannot
//! mistake it for a fact — and separately reports
//! [`Attachment::size_check`], which says whether the declaration could be
//! checked cheaply and whether it held. The cheap check is exact for an
//! **unfiltered** stream (raw length == decoded length, so a disagreement
//! is proof) and is honestly `Unverified` for a filtered one, where
//! deciding would mean decoding. [`extract_attachment`] upgrades the
//! verdict once the bytes exist.
//!
//! Surfacing that disagreement rather than smoothing it over is the "fuzzy,
//! never sneaky" rule (project rule 4) applied to a *document's* claim
//! rather than to one of pdfcer's own: a file that lies about its
//! attachment's size is a fact the operator is entitled to.
//!
//! ## Never panics; degrades and counts
//!
//! Everything here is written against hostile input, per the crate-level
//! panic-free policy and `ARCHITECTURE.md` §10:
//!
//! - Name-tree traversal is **cycle-safe** (a visited set of node object
//!   ids), **depth-bounded**
//!   ([`MAX_NAME_TREE_DEPTH`](crate::pageops::references::MAX_NAME_TREE_DEPTH))
//!   and **node-budgeted**
//!   ([`MAX_NAME_TREE_NODES`](crate::pageops::references::MAX_NAME_TREE_NODES)).
//!   Those constants are borrowed from [`crate::pageops::references`]
//!   rather than redeclared, so pdfcer has one answer to "how big may a
//!   name tree be", not two that can drift.
//! - The result list is capped at [`MAX_ATTACHMENTS`].
//! - A malformed entry is **skipped and counted**, never fatal. A document
//!   whose page tree cannot be walked still yields its document-level
//!   attachments; a name tree that cycles still yields the entries reached
//!   before the cycle.
//! - Every one of those degradations increments a field of
//!   [`AttachmentNotes`], available from [`list_attachments_with_notes`].
//!   An omission a caller cannot see is indistinguishable from an absence,
//!   which is the same "sneaky" failure the disclosure rule forbids — so
//!   the notes are the omission's disclosure channel.
//!
//! ## What this module deliberately does NOT model
//!
//! Recorded so a future reader knows these were considered and left out,
//! not overlooked:
//!
//! - **`/RF` (related files arrays)** — a filespec may carry files
//!   *related to* the main one, keyed by the same `/F`/`/UF`/… slots as
//!   `/EF`. pdfcer reads only `/EF`. A document using `/RF` will have those
//!   related files silently absent from the listing. This is a **known
//!   gap**, not a decision that they do not matter.
//! - **`/CI` (collection item data)** — a portfolio's per-file sort/display
//!   metadata. Portfolio *presentation* is a separate feature; the files
//!   themselves list here regardless.
//! - **`/ID`, `/V`, `/FS`** on the filespec — the referenced file's §14.4
//!   file identifier, the "shall not cache" volatility flag, and the file
//!   system name (only standard value: `URL`). None affects what the
//!   attachment *is*; a future `/FS /URL` feature would need `/FS`.
//! - **`/Params /Mac`** (Table 47) — the Mac OS `Subtype`/`Creator`/
//!   `ResFork` triple. A resource fork is a second payload this module
//!   does not surface.
//! - **`/CheckSum` verification.** The value is reported, never computed
//!   or compared — see [`Attachment::checksum`], where the reason is not
//!   just cost.
//! - **Decryption.** See [`AttachmentNotes::may_be_encrypted`]; this is the
//!   sharpest trap in the module, because since PDF 1.5 an *otherwise
//!   unencrypted* document can carry encrypted embedded files.
//! - **Writing anything.** No add, no replace, no delete, no extract-to-disk.
//!
//! ## Spec sources
//!
//! Sourced by `pdfcer-spec-librarian` on 2026-08-10 directly from
//! ISO 32000-1:2008; see `iso32000__ref__embedded_files.md`,
//! `iso32000__s__7.11.md`, `iso32000__s__7.7.4.md`,
//! `iso32000__s__7.9.6.md` and `iso32000__s__12.5.6.15.md` in
//! `D:\Dev\Rag-Specialized\PDF_Spec\`. Every clause number below is the
//! **ISO 32000-1** numbering; ISO 32000-2 renumbers several of these
//! subclauses and tables, so a bare "Table 44" is edition-dependent.
//!
//! - §7.11.1–7.11.3, Table 44 — file specifications and the `/Type
//!   /Filespec` dictionary. Note what is **not** there: no `/UF`-over-`/F`
//!   precedence rule (see [`NAME_SLOT_ORDER`]).
//! - §7.11.4, Tables 45/46 — embedded file streams (`/Type /EmbeddedFile`,
//!   `/Subtype` as a `#`-escaped MIME type) and the `/Params` dictionary
//!   (`/Size`, `/CreationDate`, `/ModDate`, `/CheckSum`, `/Mac`).
//! - §7.11.7 NOTES 1/2/3/7 — why a complete enumeration is impossible, and
//!   why duplicate filenames are legal.
//! - §7.7.4 Table 31 — the catalog's `/Names` dictionary and its
//!   `/EmbeddedFiles` entry (which maps keys to **file specifications**,
//!   not to streams: the stream is one `/EF` hop further).
//! - §7.9.6 Table 36 — name trees. The traversal contract lives here, not
//!   in §7.7.4.
//! - §12.5.6.15 Table 184 — file attachment annotations: `/Subtype` and
//!   `/FS` required, `/Name` optional with four standard icon names
//!   (`Graph`, `PushPin`, `Paperclip`, `Tag`) and a `PushPin` default, and
//!   the `shall` that `/Contents` is used **rather than** the filespec's
//!   `/Desc`.
//! - §12.3.5 — a PDF Portfolio's contents *are* the `/EmbeddedFiles` tree:
//!   *"All attachments in that tree are in the collection; any attachments
//!   not in that tree are not."*
//! - §7.6.1 / §7.6.5 — embedded streams are encrypted like any other, and
//!   `/EFF` + `DefEmbeddedFile` can encrypt them inside an otherwise
//!   unencrypted document.
//! - §7.3.10 — a dangling indirect reference is `null` and "shall not be
//!   considered an error", which is why every unresolvable thing here
//!   degrades rather than failing.
//! - §7.9.2 — text strings, for `/UF` and `/Desc`.
//! - §7.3.5 — name `#xx` escapes, which is how a `/` gets into a MIME type.

use std::collections::HashSet;

use crate::annot::MAX_ANNOTS_PER_PAGE;
use crate::filters::{self, FilterError};
use crate::graph::ObjectGraph;
use crate::object::{Dict, ObjId, Object};
use crate::pageops::references::{MAX_NAME_TREE_DEPTH, MAX_NAME_TREE_NODES};
use crate::textstring::decode_text_string;
use crate::view::DocumentView;

/// Hard ceiling on how many attachments one listing will report.
///
/// A pdfcer guard, not a spec limit — §7.11 puts no bound on the size of
/// the `/EmbeddedFiles` name tree, so a hostile document can declare as
/// many entries as it has bytes to spell them with. 65,536 is far past any
/// legitimate document (a large PDF Portfolio is dozens of files, not tens
/// of thousands) while keeping a listing's memory cost bounded and its
/// construction fast enough to run on the UI thread.
///
/// Hitting it sets [`AttachmentNotes::truncated`], so a caller can say
/// "showing the first 65,536" rather than presenting a truncated list as
/// complete.
pub const MAX_ATTACHMENTS: usize = 65_536;

/// The longest sanitised filename [`sanitize_attachment_name`] will emit,
/// in `char`s.
///
/// Chosen well under the 255-*byte* component limit that NTFS, ext4 and
/// APFS all share, because a `char` can cost up to four bytes and because
/// a caller that must de-duplicate (`report.txt`, `report (2).txt`) needs
/// headroom to append without re-truncating.
pub const MAX_SAFE_NAME_CHARS: usize = 200;

/// The name [`sanitize_attachment_name`] falls back to when nothing usable
/// survives sanitisation (an empty name, a name that was only separators,
/// a name that was only dots).
pub const FALLBACK_SAFE_NAME: &str = "attachment";

// ---------------------------------------------------------------------------
// The model
// ---------------------------------------------------------------------------

/// Which of the two mechanisms carries this attachment.
///
/// This is the discrimination the module exists for. Read the module docs
/// before treating the two as interchangeable — they are not, and the
/// difference bites hardest at save time and at page-delete time.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum AttachmentKind {
    /// Reached through the catalog's `/Names /EmbeddedFiles` name tree
    /// (§7.7.4 Table 31 + §7.9.6). Belongs to the document, not to any
    /// page; survives deletion of every page.
    DocumentLevel {
        /// The raw name-tree **key** bytes, exactly as the tree spells
        /// them.
        ///
        /// Kept separately from [`Attachment::name`] because the key and
        /// the filespec's `/F`/`/UF` are three independently-authored
        /// strings that a real document is free to disagree on, and
        /// because a future write path needs the key to find the entry
        /// again. §7.9.6 makes tree keys **byte strings**, so this is
        /// `Vec<u8>` and not `String`.
        tree_key: Vec<u8>,
    },
    /// A `/FileAttachment` annotation (§12.5.6.15) in one page's `/Annots`
    /// array. Pinned to a rectangle on that page; **destroyed when the
    /// page is deleted**.
    PageAnnotation {
        /// Zero-based index of the page in document order, as
        /// [`crate::page_tree::pages_in`] enumerates it.
        page_index: usize,
        /// The page object's identity — stable across a reorder, unlike
        /// `page_index`, which is why both are carried.
        page_id: ObjId,
        /// The annotation object's identity, when the `/Annots` entry was
        /// an indirect reference (it always is in a well-formed file).
        annot_id: Option<ObjId>,
        /// The annotation's `/Name` icon, raw name bytes. `None` when the
        /// entry is absent.
        ///
        /// Table 184 names **four** standard icons a conforming reader
        /// "shall provide predefined icon appearances" for — `Graph`,
        /// `PushPin`, `Paperclip`, `Tag` — permits additional names, and
        /// gives a **default of `PushPin`** when the entry is absent.
        ///
        /// The default is deliberately **not applied here**. `None` means
        /// "the document did not say", which is the truth; a UI that wants
        /// the spec default can apply it, and one that wants to show that
        /// the producer omitted it still can. Substituting silently would
        /// be the read half inventing structure. Two further facts a
        /// renderer needs and this field does not carry: the standard
        /// specifies **no artwork** for the four names (no geometry, size
        /// or colour), and Table 184 says the annotation's `/AP`, if
        /// present, "shall take precedence over the `Name` entry".
        icon: Option<Vec<u8>>,
    },
}

/// Where [`Attachment::name`] came from.
///
/// Disclosed rather than hidden because the three candidate spellings can
/// disagree, and a caller showing a name the operator cannot find in the
/// document has no way to explain itself without this.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum NameSource {
    /// The filespec's `/UF` — a §7.9.2 **text string** (PDF 1.7), the only
    /// filename slot with a defined character encoding. pdfcer's first
    /// choice; see [`NAME_SLOT_ORDER`] for why that is policy and not a
    /// spec rule.
    Uf,
    /// The filespec's `/F` — a §7.11.2 file-specification string. §7.11.2.1
    /// says its bytes "shall be passed to the operating system without
    /// interpretation or conversion of any sort", so it has **no declared
    /// encoding**; pdfcer decodes it as a §7.9.2 text string anyway in order
    /// to have something displayable, and that is a guess.
    F,
    /// The filespec's `/DOS` slot. Table 44 calls these "obsolescent and
    /// should not be used by conforming writers", but `/F` is only
    /// *required* when all three platform slots are absent, so a
    /// conforming file can supply nothing else.
    Dos,
    /// The filespec's `/Mac` slot — see [`NameSource::Dos`].
    Mac,
    /// The filespec's `/Unix` slot — see [`NameSource::Dos`].
    Unix,
    /// No filename slot was usable, so the **name-tree key** was used.
    /// Only reachable for [`AttachmentKind::DocumentLevel`] — an
    /// annotation has no key to fall back on.
    ///
    /// # ⚠️ A tree key is NOT a filename and has no declared encoding
    ///
    /// Table 31 describes `/EmbeddedFiles` as mapping "name strings to file
    /// specifications" and stops there — it states **no** encoding
    /// requirement, and the sibling `/Renditions` row in the same table
    /// *does* ("which shall have Unicode encoding"), so the omission is
    /// deliberate. §7.9.6 then says outright that "any encoding of the keys
    /// may be used as long as it is self-consistent" and that keys "shall
    /// be compared for equality on a simple byte-by-byte basis". Two other
    /// tables (155 `/D`, 202 `/N`) type the same key a **byte** string.
    ///
    /// So when pdfcer falls back to the key it is (a) using an index key as
    /// a display name, which producers routinely mangle with numeric
    /// suffixes and portfolio folder prefixes, and (b) decoding bytes that
    /// have no declared encoding. Both are guesses, this variant is how
    /// they are disclosed, and
    /// [`AttachmentKind::DocumentLevel::tree_key`] keeps the raw bytes so a
    /// caller can disagree.
    TreeKey,
    /// Nothing named this attachment at all. [`Attachment::name`] is
    /// empty; a caller needing a filename must synthesise one (see
    /// [`sanitize_attachment_name`], which turns an empty name into
    /// [`FALLBACK_SAFE_NAME`]).
    None,
}

impl NameSource {
    /// Map a filespec filename-slot key to its variant.
    ///
    /// Total, with `None` as the catch-all, because the alternative is an
    /// `unwrap` on a lookup that is only correct as long as this function
    /// and [`NAME_SLOT_ORDER`] stay in sync — and the crate forbids that
    /// `unwrap` for exactly this class of reason.
    const fn from_slot(slot: &[u8]) -> Self {
        match slot {
            b"UF" => Self::Uf,
            b"F" => Self::F,
            b"DOS" => Self::Dos,
            b"Mac" => Self::Mac,
            b"Unix" => Self::Unix,
            _ => Self::None,
        }
    }
}

/// Whether the document's declared `/Params /Size` could be checked, and
/// whether it held.
///
/// Four of the five variants are reachable from a listing alone; the
/// `Unverified` one is the honest answer for a filtered stream, and
/// [`extract_attachment`] replaces it with `Agrees`/`Disagrees` once the
/// bytes have been decoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum DeclaredSizeCheck {
    /// No `/Params /Size` was declared. §7.11.4 makes it optional, so this
    /// is ordinary and not a defect.
    NotDeclared,
    /// There is no embedded file stream to compare against — an external
    /// file reference (a filespec with no `/EF`), or an `/EF` that does
    /// not resolve to a stream.
    NoStream,
    /// A size was declared and a stream exists, but the stream is
    /// **filtered**, so its raw byte count is not its decoded byte count
    /// and comparing them would manufacture a false verdict in both
    /// directions. Decode it ([`extract_attachment`]) to find out.
    Unverified,
    /// Declared size matched the measured byte count exactly.
    Agrees {
        /// The agreed size, in bytes.
        bytes: u64,
    },
    /// **The declaration and the bytes disagree.** Both numbers are given
    /// so a caller can state the discrepancy rather than merely flag it.
    ///
    /// Word it as a measurement, not a verdict: §7.11.4 attaches no `shall`
    /// to `/Size` and prescribes no reader behaviour on a mismatch
    /// (ambiguity **EF-A2**), so this is "the document says 999999 and
    /// pdfcer counted 10", not "this document is non-conforming".
    Disagrees {
        /// What `/Params /Size` claimed.
        declared: u64,
        /// What the bytes actually measure.
        actual: u64,
    },
}

impl DeclaredSizeCheck {
    /// `true` only for [`DeclaredSizeCheck::Disagrees`] — i.e. only when
    /// pdfcer actually counted the bytes and they did not match.
    ///
    /// Deliberately not true for `Unverified`: "we did not check" and "we
    /// checked and they differ" are different claims, and collapsing them
    /// would put an accusation on screen that pdfcer cannot support.
    ///
    /// The name says *contradicted*, not *non-conforming*, on purpose —
    /// see [`DeclaredSizeCheck::Disagrees`].
    #[must_use]
    pub const fn is_contradicted(self) -> bool {
        matches!(self, Self::Disagrees { .. })
    }
}

/// One attachment, of either kind.
///
/// Everything here is **read from the document and reported as found**.
/// Nothing is repaired, normalised, or invented: a filespec with a
/// nonsense `/Subtype` reports that nonsense, an absent `/Desc` is `None`
/// rather than a synthesised description, and a name containing a path
/// traversal is reported containing a path traversal (see
/// [`Attachment::name`]).
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Attachment {
    /// The attachment's name, decoded to text.
    ///
    /// # ⚠️ This is attacker-controlled text. Do not pass it to the filesystem.
    ///
    /// It comes from a byte string inside a document that arrived from
    /// somewhere, and **nothing in ISO 32000-1 constrains its content**.
    /// §7.11.2's file-specification-string format describes how a
    /// *conforming producer* should spell a path, and says nothing that
    /// stops a hostile one from writing `..\..\..\Windows\System32\evil.exe`,
    /// `/etc/cron.d/pwn`, `report.pdf\0.exe`, or `CON.txt`. §7.9.6 says
    /// even less about a name-tree key: it is a byte string.
    ///
    /// pdfcer reports it **raw** anyway, and that is a deliberate design
    /// choice with a rationale — see [`sanitize_attachment_name`], which
    /// is the function to use before this string touches a path. In one
    /// line: the raw name is what the document *says*, and a reader whose
    /// job is to tell the operator what the document says must not quietly
    /// substitute a different string; the sanitiser is a separate,
    /// explicit step that reports what it changed.
    ///
    /// Empty when [`Attachment::name_source`] is [`NameSource::None`].
    pub name: String,
    /// The undecoded bytes behind [`Attachment::name`].
    ///
    /// Present because §7.9.2 decoding is lossy in one direction (an
    /// undefined PDFDocEncoding code becomes U+FFFD) and because a write
    /// path must be able to reproduce the original spelling byte for byte.
    pub name_bytes: Vec<u8>,
    /// Which document entry [`Attachment::name`] came from.
    pub name_source: NameSource,
    /// `false` when decoding [`Attachment::name_bytes`] to text needed at
    /// least one U+FFFD substitution — an undefined PDFDocEncoding code,
    /// an odd trailing byte after a UTF-16BE BOM, or an unpaired
    /// surrogate (see [`crate::textstring::DecodedText::exact`]).
    ///
    /// A GUI showing a name with `false` here should say the name is
    /// approximate. This is disclosure of pdfcer's *own* lossiness, which
    /// rule 4 requires just as much as disclosure of an inference.
    pub name_exact: bool,
    /// A human-authored description of the file — **not** a filename.
    ///
    /// # It comes from a DIFFERENT entry depending on the kind
    ///
    /// This is a `shall`, and it is easy to get wrong because the same
    /// filespec can be reached both ways:
    ///
    /// - [`AttachmentKind::DocumentLevel`] → the filespec's **`/Desc`**
    ///   (§7.11.3, PDF 1.6). Table 44's row for it says it "shall be used
    ///   for files in the `EmbeddedFiles` name tree", which is exactly
    ///   this route.
    /// - [`AttachmentKind::PageAnnotation`] → the annotation's
    ///   **`/Contents`**. §12.5.6.15: *"Conforming readers shall use this
    ///   entry rather than the optional `Desc` entry (PDF 1.6) in the file
    ///   specification dictionary."*
    ///
    /// So one filespec shared between a name-tree entry and an annotation
    /// legitimately shows **two different descriptions**, and neither may
    /// be cached for the other. `None` on the annotation route means the
    /// annotation had no `/Contents`; it does **not** fall through to
    /// `/Desc`, because the `shall` says "rather than", not "in preference
    /// to when present".
    ///
    /// Decoded per §7.9.2. On the annotation route the value is markup
    /// pop-up body text, whose paragraph separator per §12.5.6.2 is
    /// `CR (0Dh)` rather than LF — worth knowing before rendering it.
    pub description: Option<String>,
    /// Which of the two mechanisms carries this attachment, plus the
    /// mechanism-specific coordinates needed to find it again.
    pub kind: AttachmentKind,
    /// `/Params /Size` (§7.11.4) — **as declared by the document**, not as
    /// measured. See the module docs; see [`Attachment::size_check`] for
    /// whether the declaration survives contact with the stream.
    ///
    /// `None` when not declared (it is optional) or when the declared
    /// value was negative or not an integer, which no size can be.
    pub declared_size: Option<u64>,
    /// What pdfcer could cheaply determine about [`Attachment::declared_size`].
    pub size_check: DeclaredSizeCheck,
    /// The embedded file stream's `/Subtype` (§7.11.4) — a **MIME media
    /// type** written as a PDF name, so `text/plain` appears in the file
    /// as `/text#2Fplain` and arrives here already `#`-decoded (§7.3.5).
    ///
    /// Lossy-UTF-8 decoded, because §7.3.5 says a name used as text is
    /// UTF-8 but does not make non-UTF-8 bytes illegal. `None` when
    /// absent — which is common and not an error; the type is optional and
    /// most producers omit it.
    ///
    /// **A claim by the document about its own payload, never a
    /// measurement.** pdfcer does not sniff the bytes to check it, and a
    /// caller must not treat it as a safety signal: `/text#2Fplain` on a
    /// PE executable is trivially authorable.
    ///
    /// Because `/Subtype` is **optional**, `None` is common and means
    /// simply "the document declared no media type" — not "unknown type,
    /// go and work it out". Nothing in §7.11 authorises extension-sniffing
    /// or content-sniffing to fill the gap, so if pdfcer ever guesses a type
    /// it must be disclosed as a guess (gap **EF-N3**).
    pub mime: Option<String>,
    /// `/Params /CreationDate` (§7.11.4), **raw and unparsed**.
    ///
    /// Stored as the decoded text of the string rather than a date type
    /// for the same reason [`crate::annot::Annotation::mod_date`] is: this
    /// crate has no shared §7.9.4 date type yet, and inventing a private
    /// one here would guarantee two parsers that disagree the day a second
    /// caller wants dates. Whoever adds the shared parser owns migrating
    /// this field.
    pub created: Option<String>,
    /// `/Params /ModDate` (§7.11.4), raw and unparsed — see
    /// [`Attachment::created`].
    pub modified: Option<String>,
    /// `/Params /CheckSum` (§7.11.4) — the raw 16 bytes of a declared MD5
    /// digest.
    ///
    /// # Reported, never verified — and that is not merely a cost decision
    ///
    /// **The standard contradicts itself about which bytes are digested,
    /// inside a single table cell.** Table 46's `/CheckSum` row says it is
    /// the checksum "of the bytes of the *uncompressed* embedded file" and
    /// then, in the next sentence, describes "applying … MD5 … to the bytes
    /// of *the embedded file stream*". Those are different byte sequences
    /// whenever `/Filter` is present, which is most of the time. The
    /// spec-RAG sourcing pass checked ISO 32000-2's change record: 2.0
    /// retypes the entry a byte string and adds that "this is strictly a
    /// checksum, and is not used for security purposes", but changes
    /// **neither sentence** — so the contradiction is permanent
    /// (ambiguity **EF-A1**).
    ///
    /// A verifier would therefore have to pick an interpretation and could
    /// report "corrupt" for a perfectly conforming file that chose the
    /// other one. pdfcer reports the declared value and stops. If
    /// verification is ever added it must try both readings and report
    /// *which* matched, never a bare pass/fail.
    ///
    /// Independently: MD5, and explicitly non-security in PDF 2.0. A caller
    /// must not present this as an integrity or authenticity guarantee, and
    /// §7.11 provides no other one — `/ID` identifies the *referenced*
    /// file, and there is no per-attachment signature.
    pub checksum: Option<Vec<u8>>,
    /// The object id of the embedded file stream — the handle a caller
    /// needs to fetch bytes ([`attachment_bytes`]) or to reason about
    /// sharing.
    ///
    /// `None` for an external file reference (a filespec with no `/EF`),
    /// or when `/EF`'s entry did not resolve to a stream. Both are listed
    /// rather than dropped: an attachment the document *names* but cannot
    /// deliver is information the operator wants, and hiding it would make
    /// a damaged document look merely smaller.
    pub stream_id: Option<ObjId>,
    /// Which `/EF` sub-key supplied [`Attachment::stream_id`] — `b"UF"` or
    /// `b"F"`. `None` when there is no stream.
    pub ef_key: Option<Vec<u8>>,
    /// The file specification dictionary's own object id, when it was
    /// reached by reference.
    ///
    /// # This is how you detect the SAME file listed twice
    ///
    /// A `/FileAttachment` annotation's `/FS` may point at the very same
    /// filespec object that a `/Names /EmbeddedFiles` entry points at.
    /// Nothing forbids it, and when it happens the document has **one**
    /// file surfaced through **two** mechanisms with two different
    /// lifetimes. pdfcer reports both entries — collapsing them would hide
    /// the fact that deleting the page removes one route and not the other
    /// — and equal `filespec_id`s are the signal that lets a caller say
    /// "these are the same file".
    pub filespec_id: Option<ObjId>,
}

impl Attachment {
    /// This attachment's name, made safe to use as one filename component.
    ///
    /// Sugar for `sanitize_attachment_name(&self.name)`, and it exists for
    /// a reason that is not sugar: it makes the **safe** call the short one.
    /// Every extraction path should reach for this, and the raw
    /// [`Attachment::name`] only when it genuinely wants to display what
    /// the document says. Read [`sanitize_attachment_name`] before using
    /// the result — in particular, it is a name, not a location, and the
    /// caller still owns collision handling and destination containment.
    #[must_use]
    pub fn safe_name(&self) -> SafeName {
        sanitize_attachment_name(&self.name)
    }
}

/// Everything a listing had to skip, bound, or degrade, counted.
///
/// The disclosure channel for omissions. A caller that ignores this is
/// presenting a possibly-incomplete list as complete, which is exactly the
/// silence the "fuzzy, never sneaky" rule (project rule 4) forbids — the
/// rule is about pdfcer not quietly deciding things, and quietly dropping a
/// malformed entry is a decision.
///
/// All-zero/false means the listing is complete and everything parsed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct AttachmentNotes {
    /// The page tree could not be walked ([`crate::page_tree::pages_in`]
    /// returned an error), so **no page-level attachments were looked
    /// for**. Document-level ones are still listed and still complete.
    pub page_tree_unwalkable: bool,
    /// The listing stopped at [`MAX_ATTACHMENTS`]. There are more.
    pub truncated: bool,
    /// Name-tree traversal ran out of its
    /// [`MAX_NAME_TREE_NODES`](crate::pageops::references::MAX_NAME_TREE_NODES)
    /// budget or hit
    /// [`MAX_NAME_TREE_DEPTH`](crate::pageops::references::MAX_NAME_TREE_DEPTH).
    /// Entries beyond that point were not reached.
    pub name_tree_budget_exhausted: bool,
    /// `/Kids` entries that pointed back at a node already visited on this
    /// walk — a cycle, skipped. Non-zero means the document is malformed.
    pub name_tree_cycles: usize,
    /// Name-tree `/Names` entries that could not be read as a
    /// key/filespec pair: a trailing key with no value (an odd-length
    /// array), a key that was not a string or name, or a value that did
    /// not resolve to a dictionary.
    pub malformed_tree_entries: usize,
    /// `/FileAttachment` annotations whose `/FS` was missing or did not
    /// resolve to a dictionary. §12.5.6.15 makes `/FS` required, so each
    /// of these is an annotation that claims to carry a file and does not
    /// name one.
    pub annotations_without_filespec: usize,
    /// Filespecs with no `/EF` at all. **Not necessarily a defect** —
    /// §7.11.3 file specifications also describe *external* files, which
    /// legitimately have no embedded bytes. Counted so a caller can
    /// explain why some rows offer nothing to extract.
    pub filespecs_without_stream: usize,
    /// Filespecs whose `/EF` entry existed but did not resolve to a stream
    /// object — a dangling reference (§7.3.10: null, not an error) or a
    /// value of the wrong type. **This one is always a defect**: the
    /// document promised bytes and cannot produce them.
    pub unresolvable_streams: usize,
    /// The trailer carries an `/Encrypt` dictionary, so **any bytes
    /// [`extract_attachment`] returns may be ciphertext**.
    ///
    /// # Why this flag exists rather than being left to the caller to infer
    ///
    /// §7.6.1 says embedded file stream contents "shall be encrypted like
    /// any other stream", and — the part that makes this a trap rather than
    /// an inconvenience — since PDF 1.5 embedded files "can be encrypted in
    /// an otherwise unencrypted document" via the encryption dictionary's
    /// `/EFF` entry naming a `DefEmbeddedFile` crypt filter (§7.6.5). So
    /// the intuitive guard — *"is the document encrypted? no? then the
    /// bytes are plaintext"* — is wrong for a whole class of real file, and
    /// wrong **silently**: the `/Filter` chain runs, produces bytes, and
    /// those bytes are garbage that looks like a successful extraction.
    ///
    /// `pdfcer-core` does not implement decryption on this path yet, so the
    /// only honest thing this module can do is say when the question
    /// applies. It is set from the presence of `/Encrypt` alone, which is
    /// cheap and deliberately over-broad: a document with `/Encrypt` and
    /// only `/StrF` set may well have plaintext attachments, and this flag
    /// will still be `true`. Over-warning is the correct error here.
    pub may_be_encrypted: bool,
}

/// Why fetching an attachment's bytes failed.
///
/// Separate from a bare `Option` on [`extract_attachment`] because the
/// causes are operator-distinguishable and lead to different sentences:
/// "this attachment is a link to an external file, there is nothing to
/// extract" is not the same message as "this document is damaged" or "this
/// payload uses a compression pdfcer cannot decode".
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[non_exhaustive]
pub enum AttachmentError {
    /// The filespec carried no `/EF`, so there are no embedded bytes. An
    /// external file reference (§7.11.3) — legal, just not extractable.
    #[error("attachment has no embedded file stream (external file reference)")]
    NoEmbeddedStream,
    /// [`Attachment::stream_id`] did not resolve to a stream in this view.
    /// Either the object is missing (§7.3.10 null) or it is not a stream.
    ///
    /// Reachable even for an [`Attachment`] whose `stream_id` was `Some`
    /// at listing time, because the caller may pass a view of a
    /// *different* document — see [`extract_attachment`]'s docs.
    #[error("attachment stream {0} is missing or is not a stream")]
    StreamUnresolvable(ObjId),
    /// The stream's [`ByteSpan`](crate::span::ByteSpan) could not be
    /// served by this view's byte source. See
    /// [`crate::view::StreamSource::slice`] — it means the span is out of
    /// bounds, straddles the base/staging boundary, or belongs to another
    /// document's buffer.
    #[error("attachment stream {0} has a byte span this view cannot serve")]
    SpanUnservable(ObjId),
    /// The `/Filter` chain failed or is unsupported.
    #[error("attachment stream could not be decoded: {0}")]
    Decode(#[from] FilterError),
}

/// An extracted attachment: its bytes, plus the verdict on the document's
/// size claim now that the bytes exist to check it against.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ExtractedAttachment {
    /// The decoded payload. Opaque bytes — pdfcer does not interpret,
    /// validate, or execute them (module docs).
    pub data: Vec<u8>,
    /// `/Params /Size` as the document declared it, repeated here so a
    /// caller holding only this struct can still state the discrepancy.
    pub declared_size: Option<u64>,
    /// The **final** verdict. Never [`DeclaredSizeCheck::Unverified`]:
    /// once the bytes are decoded the question is answerable, and
    /// answering it is the point of doing the work.
    pub size_check: DeclaredSizeCheck,
}

// ---------------------------------------------------------------------------
// Listing
// ---------------------------------------------------------------------------

/// Every attachment in `graph`, of both kinds, each labelled by
/// [`AttachmentKind`].
///
/// Document-level entries come first, in name-tree traversal order (which
/// is ascending key order for a conforming tree — §7.9.6 requires the keys
/// to be sorted, and pdfcer walks `/Kids` in array order rather than
/// re-sorting, so a *non*-conforming tree is reported in the order it is
/// written rather than being silently tidied). Page-level entries follow,
/// in page order and then `/Annots` order.
///
/// Never fails and never panics: a document with no attachments, a
/// document with a shredded name tree, and a document that is not really a
/// document all return an empty or partial list. Use
/// [`list_attachments_with_notes`] when the difference matters — and it
/// usually does, because "no attachments" and "attachments pdfcer could not
/// read" look identical here.
///
/// # Examples
///
/// ```
/// use pdfcer_core::attachments::{list_attachments, AttachmentKind};
/// use pdfcer_core::document::Document;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let doc = Document::from_bytes(
///     include_bytes!("../../../fixtures/synthetic/attachments/both-kinds.pdf").to_vec(),
/// )?;
/// let found = list_attachments(&doc);
/// assert_eq!(found.len(), 2);
///
/// // One of each kind, and the page-level one knows which page it is on.
/// assert!(matches!(found[0].kind, AttachmentKind::DocumentLevel { .. }));
/// assert!(matches!(
///     found[1].kind,
///     AttachmentKind::PageAnnotation { page_index: 1, .. }
/// ));
/// # Ok(())
/// # }
/// ```
#[must_use]
/// An embedded file on the clipboard (`Pass 173.0`).
///
/// # Why the DECODED bytes, and why that is the whole type
///
/// The clip carries what `extract-attachment` would have written to disk, not
/// the raw embedded-file stream with its `/Filter` chain. Two reasons:
/// [`EditSession::attach_file`](crate::edit::EditSession::attach_file) takes
/// decoded bytes on the way back in, so carrying the raw form would mean
/// re-deriving the same answer at paste time; and a clip whose payload is the
/// file itself is one a shell can hand to anything.
///
/// So there is no serialisation method here. **The clip's payload IS the
/// file** — write [`Self::bytes`] out under [`Self::name`] and you have the
/// attachment; hand the pair back to
/// [`paste_attachment`](crate::edit::EditSession::paste_attachment) and it
/// goes into another document.
///
/// # ⚠️ [`Self::name`] is attacker-controlled text
///
/// It came out of a document that arrived from somewhere, and **nothing in
/// ISO 32000-1 constrains its content** — see
/// [`Attachment::name`](crate::attachments::Attachment::name) for the full
/// warning and the shapes to expect. **Do not pass it to the filesystem
/// without sanitising it**, and note that `paste_attachment` writing it back
/// into another PDF is safe precisely because it never touches a path.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct AttachmentClip {
    /// The attachment's name, decoded. See the type's own warning.
    pub name: String,
    /// The file's decoded bytes — the payload itself.
    pub bytes: Vec<u8>,
    /// `/Desc`, the human-readable description, when the source carried one.
    pub description: Option<String>,
}

impl AttachmentClip {
    /// Build a clip from a file a shell already has in hand.
    ///
    /// Exists for the same reason
    /// [`PageClip::from_bytes`](crate::pageops::PageClip::from_bytes) does:
    /// the type is `#[non_exhaustive]`, so a consumer cannot write the struct
    /// literal, and a clipboard a shell can read and never fill is half a
    /// clipboard. This is also the honest way to implement *"attach the file
    /// the operator just dropped on the window"* — it is a paste.
    pub fn new(name: impl Into<String>, bytes: Vec<u8>, description: Option<String>) -> Self {
        Self {
            name: name.into(),
            bytes,
            description,
        }
    }

    /// How many bytes the attached file holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    /// Whether the attached file is empty.
    ///
    /// An empty attachment is legal and occasionally meant — a zero-byte
    /// marker file — so this is a question, not a refusal.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }
}

pub fn list_attachments<G: ObjectGraph + ?Sized>(graph: &G) -> Vec<Attachment> {
    list_attachments_with_notes(graph).0
}

/// [`list_attachments`], keeping the [`AttachmentNotes`].
///
/// The `*_with_notes` shape follows [`crate::filters::decode_stream_with_notes`]:
/// the plain function is what most callers want, and the noted one exists
/// for the caller that has somewhere to put a diagnostic. A GUI attachment
/// panel is exactly that caller.
///
/// # Examples
///
/// ```
/// use pdfcer_core::attachments::list_attachments_with_notes;
/// use pdfcer_core::document::Document;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let doc = Document::from_bytes(
///     include_bytes!("../../../fixtures/synthetic/attachments/degenerate.pdf").to_vec(),
/// )?;
/// let (found, notes) = list_attachments_with_notes(&doc);
///
/// // A cycle and several broken entries did not stop the good one.
/// assert!(found.iter().any(|a| a.name == "good.txt"));
/// assert!(notes.name_tree_cycles > 0);
/// assert!(notes.malformed_tree_entries > 0);
/// # Ok(())
/// # }
/// ```
#[must_use]
pub fn list_attachments_with_notes<G: ObjectGraph + ?Sized>(
    graph: &G,
) -> (Vec<Attachment>, AttachmentNotes) {
    let mut out = Vec::new();
    // Set at construction rather than after the collectors run, so the flag
    // is true even if both bail out — "this document's attachments may be
    // encrypted" holds regardless of whether pdfcer enumerated any. See
    // `AttachmentNotes::may_be_encrypted`.
    let mut notes = AttachmentNotes {
        may_be_encrypted: graph.trailer_entry(b"Encrypt").is_some(),
        ..AttachmentNotes::default()
    };

    collect_document_level(graph, &mut out, &mut notes);
    collect_page_level(graph, &mut out, &mut notes);

    (out, notes)
}

/// Walk the catalog's `/Names /EmbeddedFiles` name tree (§7.7.4 Table 31 +
/// §7.9.6).
///
/// An absent `/Names`, an absent `/EmbeddedFiles`, or either one resolving
/// to a non-dictionary means "this document has no document-level
/// attachments", which is the overwhelmingly common case and is not a
/// defect — so none of them touches [`AttachmentNotes`].
fn collect_document_level<G: ObjectGraph + ?Sized>(
    graph: &G,
    out: &mut Vec<Attachment>,
    notes: &mut AttachmentNotes,
) {
    let Some(catalog) = graph.catalog_dict() else {
        return;
    };
    let Some(names_dict) = catalog
        .get(b"Names")
        .map(|o| graph.resolve(o))
        .and_then(Object::as_dict)
    else {
        return;
    };
    let root_obj = names_dict.get(b"EmbeddedFiles");
    let Some(root) = root_obj.map(|o| graph.resolve(o)).and_then(Object::as_dict) else {
        return;
    };

    // The root node's own id, so a `/Kids` pointing straight back at it is
    // caught by the same visited-set as any deeper cycle.
    let mut visited: HashSet<ObjId> = root_obj
        .and_then(Object::as_reference)
        .into_iter()
        .collect();
    let mut budget = MAX_NAME_TREE_NODES;

    walk_name_tree(graph, root, 0, &mut budget, &mut visited, out, notes);
}

/// Recursive half of [`collect_document_level`].
///
/// Reads `/Names` **and** `/Kids` wherever it finds them rather than
/// dispatching on which node type this is. §7.9.6 says a node has one or
/// the other (root nodes may have either, intermediate nodes have `/Kids`,
/// leaves have `/Names`), but a malformed file carrying both is readable,
/// and refusing to read half of it would lose entries for no benefit. This
/// mirrors [`crate::pageops::references`]'s own name-tree walk, deliberately
/// — two name-tree readers in one crate that disagree about malformed input
/// is a bug generator.
///
/// `/Limits` is **not** consulted. It is an optimisation for *searching* a
/// tree by key (§7.9.6: it bounds the keys in a subtree), and this walk
/// enumerates everything; honouring it could only ever cause pdfcer to skip
/// entries that a mis-authored `/Limits` excluded — entries that are
/// genuinely in the file.
fn walk_name_tree<G: ObjectGraph + ?Sized>(
    graph: &G,
    node: &Dict,
    depth: usize,
    budget: &mut usize,
    visited: &mut HashSet<ObjId>,
    out: &mut Vec<Attachment>,
    notes: &mut AttachmentNotes,
) {
    if depth > MAX_NAME_TREE_DEPTH || *budget == 0 {
        notes.name_tree_budget_exhausted = true;
        return;
    }
    *budget -= 1;

    if let Some(pairs) = node
        .get(b"Names")
        .map(|o| graph.resolve(o))
        .and_then(Object::as_array)
    {
        // §7.9.6: `[key1 value1 key2 value2 …]`. An odd-length array has a
        // trailing key with no value — malformed, counted, and dropped
        // rather than paired with the next node's first value.
        if pairs.len() % 2 == 1 {
            notes.malformed_tree_entries += 1;
        }
        for pair in pairs.chunks_exact(2) {
            let (Some(key), Some(value)) = (pair.first(), pair.get(1)) else {
                continue;
            };
            // §7.9.6 says keys "shall be strings". A file using names is
            // malformed but unambiguous; both are read, matching
            // `pageops::references`' tolerance.
            let key_bytes = match graph.resolve(key) {
                Object::String(bytes) => bytes.clone(),
                Object::Name(name) => name.as_bytes().to_vec(),
                _ => {
                    notes.malformed_tree_entries += 1;
                    continue;
                }
            };
            let filespec_id = value.as_reference();
            let Some(filespec) = graph.resolve(value).as_dict() else {
                // The key exists but names nothing usable (a dangling
                // reference, an integer, …).
                notes.malformed_tree_entries += 1;
                continue;
            };
            if out.len() >= MAX_ATTACHMENTS {
                notes.truncated = true;
                return;
            }
            out.push(model_attachment(
                graph,
                filespec,
                filespec_id,
                AttachmentKind::DocumentLevel {
                    tree_key: key_bytes.clone(),
                },
                Some(&key_bytes),
                // Not the annotation route: `/Desc` applies here, and
                // §7.11.3's Table 44 row for `/Desc` says it "shall be used
                // for files in the EmbeddedFiles name tree" — i.e. this is
                // precisely where it belongs.
                None,
                notes,
            ));
        }
    }

    if let Some(kids) = node
        .get(b"Kids")
        .map(|o| graph.resolve(o))
        .and_then(Object::as_array)
    {
        for kid in kids {
            if out.len() >= MAX_ATTACHMENTS {
                notes.truncated = true;
                return;
            }
            // Cycle guard. Without it a `/Kids` pointing at an ancestor
            // does not merely recurse forever — the depth guard would stop
            // that — it makes the walk re-expand the whole subtree at every
            // level, which is exponential in the depth limit, not linear.
            if let Some(id) = kid.as_reference()
                && !visited.insert(id)
            {
                notes.name_tree_cycles += 1;
                continue;
            }
            if let Some(dict) = graph.resolve(kid).as_dict() {
                walk_name_tree(graph, dict, depth + 1, budget, visited, out, notes);
            } else {
                notes.malformed_tree_entries += 1;
            }
        }
    }
}

/// Walk every page's `/Annots` for `/FileAttachment` annotations
/// (§12.5.6.15).
///
/// A page tree that will not walk costs the page-level attachments and
/// nothing else — the document-level ones are already collected, and
/// throwing them away because a *different* part of the file is damaged
/// would be the "a damaged part must not cost the operator the whole file"
/// posture failing in the small.
fn collect_page_level<G: ObjectGraph + ?Sized>(
    graph: &G,
    out: &mut Vec<Attachment>,
    notes: &mut AttachmentNotes,
) {
    let Ok(pages) = crate::page_tree::pages_in(graph) else {
        notes.page_tree_unwalkable = true;
        return;
    };

    for (page_index, page) in pages.iter().enumerate() {
        let Some(page_dict) = graph.resolved(page.id).as_dict() else {
            continue;
        };
        let Some(annots) = page_dict
            .get(b"Annots")
            .map(|o| graph.resolve(o))
            .and_then(Object::as_array)
        else {
            continue;
        };

        for (seen, entry) in annots.iter().enumerate() {
            if seen >= MAX_ANNOTS_PER_PAGE {
                notes.truncated = true;
                break;
            }
            let annot_id = entry.as_reference();
            let Some(annot) = graph.resolve(entry).as_dict() else {
                // A dangling or non-dictionary `/Annots` element is not an
                // annotation at all (§7.3.10) — not an attachment defect.
                continue;
            };
            let is_file_attachment = annot
                .get(b"Subtype")
                .map(|o| graph.resolve(o))
                .and_then(Object::as_name)
                .is_some_and(|n| n.as_bytes() == b"FileAttachment");
            if !is_file_attachment {
                continue;
            }

            let icon = annot
                .get(b"Name")
                .map(|o| graph.resolve(o))
                .and_then(Object::as_name)
                .map(|n| n.as_bytes().to_vec());

            // §12.5.6.15 Table 184 makes `/FS` **required**. An annotation
            // without a usable one is a promise the document cannot keep,
            // so it is counted rather than silently ignored — otherwise a
            // paperclip visible on the page would correspond to no row in
            // the listing and nothing would explain the gap.
            let fs_obj = annot.get(b"FS");
            let filespec_id = fs_obj.and_then(Object::as_reference);
            let Some(filespec) = fs_obj.map(|o| graph.resolve(o)).and_then(Object::as_dict) else {
                notes.annotations_without_filespec += 1;
                continue;
            };

            // §12.5.6.15's one clause-unique `shall`: "Conforming readers
            // shall use this entry rather than the optional `Desc` entry
            // (PDF 1.6) in the file specification dictionary" — where
            // "this entry" is the ANNOTATION's `/Contents`. So the very
            // same filespec yields a different description depending on
            // which route reached it, and a reader that read `/Desc` here
            // would be violating a `shall` while looking correct.
            //
            // `/T` is deliberately NOT consulted. Table 170 makes it the
            // pop-up window's title bar text and says it "shall identify
            // the user who added the annotation" — it is the AUTHOR, never
            // the filename, and mistaking it for one is an easy and
            // plausible-looking bug.
            let contents = read_text(graph, annot, b"Contents").map(|(_, d, _)| d.text);

            if out.len() >= MAX_ATTACHMENTS {
                notes.truncated = true;
                return;
            }
            out.push(model_attachment(
                graph,
                filespec,
                filespec_id,
                AttachmentKind::PageAnnotation {
                    page_index,
                    page_id: page.id,
                    annot_id,
                    icon,
                },
                None,
                Some(contents),
                notes,
            ));
        }
    }
}

/// The order in which the filespec's filename slots are consulted for
/// [`Attachment::name`].
///
/// # ⚠️ The spec states NO precedence between `/UF` and `/F`. This is policy.
///
/// It is tempting to write "§7.11.3 prefers `/UF`", and an earlier draft of
/// this module did. It is **not true**, and the spec-RAG sourcing pass
/// (`iso32000__s__7.11.md`, ambiguity **EF-A3**) checked every sentence on
/// the subject. All of them are `should`s about including *both*:
///
/// > "Regardless of the platform, conforming readers should use the `F` and
/// > `UF` (beginning with PDF 1.7) entries to specify files. The `UF` entry
/// > is optional, but should be included because it enables cross-platform
/// > and cross-language compatibility." (§7.11.3)
///
/// > "The `UF` entry should be used **in addition to** the `F` entry. The
/// > `UF` entry provides cross-platform and cross-language compatibility
/// > and the `F` entry provides backwards compatibility." (Table 44, `F`)
///
/// The precedence the standard *does* state is a different one: `F`/`UF`
/// over `DOS`/`Mac`/`Unix`, which Table 44 calls "obsolescent and should
/// not be used by conforming writers". That part is sourced and is
/// implemented below.
///
/// So `/UF` first is **pdfcer's choice**, and here is its justification,
/// offered as a reading rather than as the standard's word: `/UF` is a
/// §7.9.2 *text string* with a defined character encoding, while §7.11.2.1
/// says `/F`'s bytes "shall be passed to the operating system without
/// interpretation or conversion of any sort" — i.e. `/F` has no encoding at
/// all, and any decoding of it into displayable text is already a guess.
/// Preferring the one entry that can be decoded correctly is the only
/// choice that is not arbitrary.
///
/// **This belongs in `crate::settings` as `filename_source`, and is not
/// there yet** — a choice the standard leaves open should be the
/// operator's, not a constant. Recorded here so it is a known debt rather
/// than an invisible hard-code.
const NAME_SLOT_ORDER: [&[u8]; 5] = [b"UF", b"F", b"DOS", b"Mac", b"Unix"];

/// The order in which `/EF`'s sub-keys are consulted for the payload.
///
/// # Also unspecified, and differently so (ambiguity EF-A4)
///
/// §7.11.4 says `/EF` holds "a subset of the keys `F`, `UF`, `DOS`, `Mac`,
/// `Unix`" and stops there. It gives **no ordering**, does not require
/// `/EF`'s keys to match the filespec's own, and does not forbid different
/// keys pointing at *different streams*. The clause's own worked example
/// has an `/EF` with `DOS`, `Mac` and `Unix` and **no `F` at all**, so a
/// reader that only looked at `F`/`UF` would find nothing in a document the
/// standard itself printed.
///
/// `F` leads here even though `UF` leads for the *name*, and the asymmetry
/// is deliberate: for the name the question is "which spelling can be
/// decoded correctly", and for the stream it is "which slot is most likely
/// to be populated" — `/F` predates `/UF` by four versions and remains the
/// one every producer writes.
///
/// [`Attachment::ef_key`] records which slot actually supplied the bytes,
/// so the choice is disclosed rather than silent. Like [`NAME_SLOT_ORDER`]
/// this should become a setting.
const EF_SLOT_ORDER: [&[u8]; 5] = [b"F", b"UF", b"DOS", b"Mac", b"Unix"];

/// Turn one §7.11.3 file specification dictionary into an [`Attachment`].
///
/// The two routes into this function differ in exactly two places, and both
/// are parameters rather than branches inside the body, because a filespec
/// is a filespec regardless of who points at it:
///
/// - `tree_key` — the name-tree key, for a document-level entry only.
/// - `annot_contents` — the annotation's `/Contents`, for a page-level
///   entry only. §12.5.6.15 makes this **override** the filespec's `/Desc`,
///   which is why the description cannot simply be read here.
fn model_attachment<G: ObjectGraph + ?Sized>(
    graph: &G,
    filespec: &Dict,
    filespec_id: Option<ObjId>,
    kind: AttachmentKind,
    tree_key: Option<&[u8]>,
    annot_contents: Option<Option<String>>,
    notes: &mut AttachmentNotes,
) -> Attachment {
    // --- the name -------------------------------------------------------
    //
    // Slot order and its (un)sourcing: see NAME_SLOT_ORDER.
    //
    // The name-tree key is the LAST resort, and reaching it is itself worth
    // disclosing — see NameSource::TreeKey. §7.9.6 makes a tree key an
    // opaque byte string with NO defined encoding ("any encoding of the
    // keys may be used as long as it is self-consistent; keys shall be
    // compared for equality on a simple byte-by-byte basis"), so decoding
    // one as if it were a §7.9.2 text string is pdfcer guessing, and the
    // guess is labelled.
    let (name_bytes, name, name_exact, name_source) = match NAME_SLOT_ORDER
        .into_iter()
        .find_map(|slot| read_text(graph, filespec, slot))
    {
        Some((bytes, decoded, slot)) => (
            bytes,
            decoded.text,
            decoded.exact,
            NameSource::from_slot(slot),
        ),
        None => match tree_key {
            Some(key) => {
                let decoded = decode_text_string(key);
                (
                    key.to_vec(),
                    decoded.text,
                    decoded.exact,
                    NameSource::TreeKey,
                )
            }
            None => (Vec::new(), String::new(), true, NameSource::None),
        },
    };

    // §12.5.6.15: an annotation's `/Contents` is used **rather than** the
    // filespec's `/Desc` — a `shall`, and it applies even when `/Contents`
    // is absent, so `Some(None)` must mean "no description" and must NOT
    // fall through to `/Desc`. That is why this is `Option<Option<String>>`
    // rather than a plain `Option<String>` with `.or_else()`: the
    // distinction between "the annotation route, which said nothing" and
    // "the name-tree route, which has not been asked yet" is the whole
    // rule.
    let description = match annot_contents {
        Some(contents) => contents,
        None => read_text(graph, filespec, b"Desc").map(|(_, d, _)| d.text),
    };

    // --- the embedded stream --------------------------------------------
    //
    // Slot order and its (un)sourcing: see EF_SLOT_ORDER.
    let mut stream_id = None;
    let mut ef_key = None;
    match filespec
        .get(b"EF")
        .map(|o| graph.resolve(o))
        .and_then(Object::as_dict)
    {
        None => notes.filespecs_without_stream += 1,
        Some(ef) => {
            let picked = EF_SLOT_ORDER
                .into_iter()
                .find_map(|key| ef.get(key).map(|obj| (key, obj)));
            match picked {
                None => notes.filespecs_without_stream += 1,
                Some((key, obj)) => {
                    let id = obj.as_reference();
                    if matches!(graph.resolve(obj), Object::Stream(_)) {
                        stream_id = id;
                        ef_key = Some(key.to_vec());
                    }
                    if stream_id.is_none() {
                        // §7.11.4 requires an embedded file STREAM here.
                        // Present-but-unusable is a real defect, distinct
                        // from absent-and-therefore-external.
                        notes.unresolvable_streams += 1;
                    }
                }
            }
        }
    }

    // --- /Params (§7.11.4) ----------------------------------------------
    let params = stream_id
        .map(|id| graph.resolved(id))
        .and_then(Object::as_dict)
        .and_then(|d| d.get(b"Params").map(|o| graph.resolve(o)))
        .and_then(Object::as_dict);

    let declared_size = params
        .and_then(|p| p.get(b"Size").map(|o| graph.resolve(o)))
        .and_then(Object::as_int)
        .and_then(|v| u64::try_from(v).ok());
    let created = params
        .and_then(|p| read_text(graph, p, b"CreationDate"))
        .map(|(_, d, _)| d.text);
    let modified = params
        .and_then(|p| read_text(graph, p, b"ModDate"))
        .map(|(_, d, _)| d.text);
    let checksum = params
        .and_then(|p| p.get(b"CheckSum").map(|o| graph.resolve(o)))
        .and_then(|o| match o {
            Object::String(bytes) => Some(bytes.clone()),
            _ => None,
        });

    let mime = stream_id
        .map(|id| graph.resolved(id))
        .and_then(Object::as_dict)
        .and_then(|d| d.get(b"Subtype").map(|o| graph.resolve(o)))
        .and_then(Object::as_name)
        .map(|n| String::from_utf8_lossy(n.as_bytes()).into_owned());

    let size_check = cheap_size_check(graph, stream_id, declared_size);

    Attachment {
        name,
        name_bytes,
        name_source,
        name_exact,
        description,
        kind,
        declared_size,
        size_check,
        mime,
        created,
        modified,
        checksum,
        stream_id,
        ef_key,
        filespec_id,
    }
}

/// Check `/Params /Size` against the stream **without decoding anything**.
///
/// The trick is that an unfiltered stream's raw byte count *is* its decoded
/// byte count, so for that (very common — most producers store attachments
/// with `/FlateDecode`, but plenty do not) case the check is exact and
/// costs a dictionary lookup. For a filtered stream the raw length says
/// nothing about the decoded length in either direction, and guessing
/// would be worse than admitting ignorance: a compressed 4 GB payload with
/// a 3 KB raw span is perfectly normal, and reporting that as a
/// disagreement would train the operator to ignore the warning.
///
/// An empty `/Filter` array counts as unfiltered — Table 5 permits it, and
/// it means the identity transform.
fn cheap_size_check<G: ObjectGraph + ?Sized>(
    graph: &G,
    stream_id: Option<ObjId>,
    declared: Option<u64>,
) -> DeclaredSizeCheck {
    let Some(declared) = declared else {
        return DeclaredSizeCheck::NotDeclared;
    };
    let Some(id) = stream_id else {
        return DeclaredSizeCheck::NoStream;
    };
    let Object::Stream(stream) = graph.resolved(id) else {
        return DeclaredSizeCheck::NoStream;
    };

    let filtered = match stream.dict.get(b"Filter").map(|o| graph.resolve(o)) {
        None | Some(Object::Null) => false,
        Some(Object::Array(items)) => !items.is_empty(),
        Some(_) => true,
    };
    if filtered {
        return DeclaredSizeCheck::Unverified;
    }

    let actual = stream.data_span.len as u64;
    if actual == declared {
        DeclaredSizeCheck::Agrees { bytes: actual }
    } else {
        DeclaredSizeCheck::Disagrees { declared, actual }
    }
}

/// Read a §7.9.2 text string entry, returning its raw bytes, its decoding,
/// and the key it was found under.
///
/// Returns `None` for an absent entry **and** for a present entry whose
/// value is not a string. The latter is malformed, and substituting some
/// other rendering of a non-string (a name's bytes, a number's digits)
/// would put text on screen that is not in the document.
fn read_text<'a, G: ObjectGraph + ?Sized>(
    graph: &'a G,
    dict: &'a Dict,
    key: &'static [u8],
) -> Option<(Vec<u8>, crate::textstring::DecodedText, &'static [u8])> {
    match graph.resolve(dict.get(key)?) {
        Object::String(bytes) => Some((bytes.clone(), decode_text_string(bytes), key)),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Extraction
// ---------------------------------------------------------------------------

/// The decoded bytes of `attachment`, or `None` if they cannot be produced.
///
/// The convenience form of [`extract_attachment`], for a caller that has
/// nowhere to put a reason. Prefer the full form anywhere the operator will
/// see the outcome — "nothing happened" is a poor answer to "extract this".
///
/// # Examples
///
/// ```
/// use pdfcer_core::attachments::{attachment_bytes, list_attachments};
/// use pdfcer_core::document::Document;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let doc = Document::from_bytes(
///     include_bytes!("../../../fixtures/synthetic/attachments/doc-level-simple.pdf").to_vec(),
/// )?;
/// let view = doc.view();
/// let found = list_attachments(&doc);
/// assert_eq!(
///     attachment_bytes(&view, &found[0]).as_deref(),
///     Some(&b"hello attachment\n"[..])
/// );
/// # Ok(())
/// # }
/// ```
#[must_use]
pub fn attachment_bytes(view: &DocumentView<'_>, attachment: &Attachment) -> Option<Vec<u8>> {
    extract_attachment(view, attachment).ok().map(|e| e.data)
}

/// Decode `attachment`'s embedded file stream and settle the declared-size
/// question.
///
/// # ⚠️ The view must be the one the [`Attachment`] was listed from
///
/// An [`Attachment`] carries object *ids*, and an id only means something
/// relative to a document. Passing a view of a different document will
/// either fail with [`AttachmentError::StreamUnresolvable`] /
/// [`AttachmentError::SpanUnservable`] — the likely outcome, and a safe one
/// — or, if the other document happens to have a stream at the same id,
/// return **that** document's bytes. pdfcer cannot detect the confusion
/// (there is no document identity on either side), so the obligation is the
/// caller's. Listing from `doc` and extracting through `doc.view()` in the
/// same breath, as the examples do, makes it a non-issue.
///
/// # ⚠️ The returned bytes are untrusted
///
/// They came from a file inside a file. pdfcer does not execute, open, or
/// interpret them, and neither should a caller without its own gate (module
/// docs).
///
/// # Errors
///
/// - [`AttachmentError::NoEmbeddedStream`] — an external file reference,
///   nothing to extract.
/// - [`AttachmentError::StreamUnresolvable`] — the id names no stream in
///   this view.
/// - [`AttachmentError::SpanUnservable`] — the view's byte source cannot
///   serve the stream's span.
/// - [`AttachmentError::Decode`] — the `/Filter` chain failed, is
///   unsupported, or blew the [`crate::filters::MAX_DECODED_LEN`] ceiling
///   (the decompression-bomb guard: an attachment is exactly the place
///   someone would hide one).
///
/// # Examples
///
/// ```
/// use pdfcer_core::attachments::{extract_attachment, list_attachments, DeclaredSizeCheck};
/// use pdfcer_core::document::Document;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let doc = Document::from_bytes(
///     include_bytes!("../../../fixtures/synthetic/attachments/size-lies.pdf").to_vec(),
/// )?;
/// let view = doc.view();
/// let found = list_attachments(&doc);
/// let got = extract_attachment(&view, &found[0])?;
///
/// // The document claimed 999999 bytes and delivered ten. pdfcer says so.
/// assert_eq!(
///     got.size_check,
///     DeclaredSizeCheck::Disagrees { declared: 999_999, actual: 10 }
/// );
/// assert!(got.size_check.is_contradicted());
/// # Ok(())
/// # }
/// ```
pub fn extract_attachment(
    view: &DocumentView<'_>,
    attachment: &Attachment,
) -> Result<ExtractedAttachment, AttachmentError> {
    let id = attachment
        .stream_id
        .ok_or(AttachmentError::NoEmbeddedStream)?;
    let Object::Stream(stream) = view.resolved(id) else {
        return Err(AttachmentError::StreamUnresolvable(id));
    };
    let raw = view
        .slice(stream.data_span)
        .ok_or(AttachmentError::SpanUnservable(id))?;
    let data = filters::decode_stream(&stream.dict, raw)?;

    let size_check = match attachment.declared_size {
        None => DeclaredSizeCheck::NotDeclared,
        Some(declared) => {
            let actual = data.len() as u64;
            if actual == declared {
                DeclaredSizeCheck::Agrees { bytes: actual }
            } else {
                DeclaredSizeCheck::Disagrees { declared, actual }
            }
        }
    };

    Ok(ExtractedAttachment {
        data,
        declared_size: attachment.declared_size,
        size_check,
    })
}

// ---------------------------------------------------------------------------
// Filename safety
// ---------------------------------------------------------------------------

/// One way an attachment name was unsafe to use as a filename.
///
/// Reported rather than merely fixed, because a caller that cannot say
/// *what* it changed cannot honour the disclosure obligation (project rule
/// 4) — "pdfcer renamed this file" with no reason is exactly the sneaky
/// behaviour the rule forbids.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum NameHazard {
    /// Contained `/` or `\`. The name is a **path**, not a filename;
    /// everything up to the last separator is discarded.
    PathSeparator,
    /// A component was `..`, which climbs out of the destination
    /// directory. The classic zip-slip / path-traversal shape.
    ParentTraversal,
    /// Began with a Windows drive designator (`C:`) or contained a colon,
    /// which on Windows also selects an NTFS alternate data stream
    /// (`file.txt:hidden`).
    DriveOrStream,
    /// Contained a NUL byte or another C0 control character. NUL is the
    /// extension-spoofing classic: `invoice.pdf\0.exe` displays as a PDF
    /// in anything that stops at the NUL and executes as a program in
    /// anything that does not.
    ControlCharacter,
    /// Contained U+FFFD REPLACEMENT CHARACTER.
    ///
    /// # This is not a cosmetic rule, and it is load-bearing
    ///
    /// U+FFFD in an attachment name almost never came from the document —
    /// it came from **pdfcer's own §7.9.2 decode failing**
    /// ([`crate::textstring::DecodedText::exact`]), and the single most
    /// likely byte behind it is `0x00`, which PDFDocEncoding leaves
    /// undefined. So a NUL extension-spoof (`invoice.pdf\0.exe`) arrives at
    /// the sanitiser already wearing a different face, and a sanitiser that
    /// only looked for `char::is_control` would wave it through.
    ///
    /// This was found by a test rather than by reasoning: the
    /// `hostile-names.pdf` fixture's NUL entry passed sanitisation
    /// unchanged on the first implementation. Treating an undecodable byte
    /// as exactly as untrustworthy as the control character it probably was
    /// is the only reading that closes the hole, and it costs nothing —
    /// a name pdfcer could not decode is one the operator should be told
    /// about regardless of what the byte turns out to have been.
    UndecodableBytes,
    /// Contained a character Windows forbids in a filename
    /// (`< > : " | ? *`).
    ReservedCharacter,
    /// Contained a Unicode bidirectional-override or isolate control
    /// (U+200E, U+200F, U+202A–U+202E, U+2066–U+2069).
    ///
    /// # A display spoof, not a filesystem hazard
    ///
    /// These characters are legal in a filename on every mainstream
    /// filesystem and are harmless to *write*. What they do is reorder how
    /// the name **renders**: `"\u{202E}gnp.exe"` displays as `exe.png` in
    /// any conforming Unicode renderer, including pdfcer's own attachment
    /// list and the operator's file manager. The operator sees a PNG,
    /// double-clicks a program.
    ///
    /// This is the same trick as the NUL spoof
    /// ([`NameHazard::ControlCharacter`]) aimed at the human instead of the
    /// parser, and an attachment name is a first-class delivery vehicle for
    /// it — the string comes from inside a document the operator merely
    /// opened. The characters are stripped rather than replaced with `_`,
    /// because they are zero-width by intent and an underscore where a
    /// reader expected nothing is its own small lie.
    BidiOverride,
    /// The stem is a reserved Windows device name — `CON`, `PRN`, `AUX`,
    /// `NUL`, `COM0`–`COM9`, `LPT0`–`LPT9`. Opening `CON.txt` for writing
    /// on Windows talks to the console device, not to a file, and the
    /// reservation applies **whatever the extension is**.
    ReservedDeviceName,
    /// Ended in a `.` or a space. Windows silently strips these when
    /// creating a file, so `evil.exe.` and `evil.exe` become the same
    /// file — a mismatch between the name the operator was shown and the
    /// file that appeared.
    TrailingDotOrSpace,
    /// Nothing usable was left, so [`FALLBACK_SAFE_NAME`] was substituted.
    Empty,
    /// Longer than [`MAX_SAFE_NAME_CHARS`] and truncated.
    TooLong,
}

/// An attachment name made safe to use as a single filename component,
/// together with a full account of what had to change.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct SafeName {
    /// The sanitised name. Always non-empty, always a single path
    /// component, never a reserved device name, never longer than
    /// [`MAX_SAFE_NAME_CHARS`] `char`s.
    ///
    /// **Still not a path.** It is safe to *join* to a directory the
    /// caller chose; it is not a location. The caller still owns checking
    /// that the join stays inside the destination and that it is not
    /// overwriting something.
    pub value: String,
    /// `false` when [`SafeName::value`] is byte-for-byte the input — i.e.
    /// the document's name was already safe and nothing was substituted.
    pub changed: bool,
    /// Every distinct hazard found, sorted and deduplicated so a message
    /// can list them deterministically.
    pub hazards: Vec<NameHazard>,
}

/// Make an attachment name safe to use as **one filename component**.
///
/// # Why pdfcer exposes the raw name AND a sanitiser, rather than only one
///
/// This was a real design choice, so here is the reasoning rather than the
/// outcome alone.
///
/// **Returning only a sanitised name was rejected.** This module's job is
/// to report what a document contains. If [`Attachment::name`] silently
/// held `evil.exe` for a document that actually says
/// `..\..\Windows\System32\evil.exe`, then pdfcer would be *lying about the
/// document* — and the operator investigating a suspicious file would be
/// looking at pdfcer's cleaned-up version of the evidence, with no way to
/// see the traversal that made it suspicious. A forensic reader that
/// quietly repairs its input is not a reader.
///
/// **Returning only the raw name was also rejected**, even with a loud
/// comment. It makes the *unsafe* thing the default and the *safe* thing an
/// extra step somebody has to know about, which is backwards: the failure
/// mode is silent, remote, and severe (a file written outside the
/// destination directory), and "we documented the hazard" has never
/// prevented that class of bug. The `serde`/`zip` ecosystem's long history
/// of zip-slip CVEs is the evidence.
///
/// **So: both, with the correction disclosed.** [`Attachment::name`] is the
/// truth about the document. `sanitize_attachment_name` is the truth about
/// what pdfcer would write to disk, *plus* a [`SafeName::hazards`] list
/// explaining every difference. That satisfies project rule 4 exactly as
/// written — the correction pdfcer made is visible before it becomes state,
/// and the operator can reject it (by choosing their own name) without
/// undoing anything else.
///
/// # What it does, in order
///
/// 1. Take everything after the **last** `/` or `\`, so a path becomes its
///    final component ([`NameHazard::PathSeparator`]).
/// 2. Replace every C0 control byte, every character Windows forbids
///    (`< > : " | ? * \` and `/`), NUL, and **U+FFFD** with `_`
///    ([`NameHazard::ControlCharacter`], [`NameHazard::ReservedCharacter`],
///    [`NameHazard::DriveOrStream`], [`NameHazard::UndecodableBytes`]).
///    The U+FFFD rule is the one that is easy to omit and must not be —
///    see [`NameHazard::UndecodableBytes`] for why a NUL reaches this
///    function disguised as a replacement character.
/// 3. **Drop** every Unicode bidi override/isolate control
///    ([`NameHazard::BidiOverride`]). Dropped rather than replaced,
///    because they are zero-width and substituting a visible character
///    would misrepresent the name in the other direction.
/// 4. Refuse the pure-traversal components `.` and `..`
///    ([`NameHazard::ParentTraversal`]).
/// 5. Strip trailing dots and spaces ([`NameHazard::TrailingDotOrSpace`]).
/// 6. If the stem (the part before the first `.`) is a reserved Windows
///    device name, prefix an `_` ([`NameHazard::ReservedDeviceName`]).
/// 7. Truncate to [`MAX_SAFE_NAME_CHARS`] `char`s ([`NameHazard::TooLong`]).
/// 8. If nothing is left, use [`FALLBACK_SAFE_NAME`]
///    ([`NameHazard::Empty`]).
///
/// The Windows rules are applied **on every platform**, deliberately. A
/// name that is safe on Linux and catastrophic on Windows is still a bug;
/// pdfcer is a portable application and its output should not depend on
/// which machine extracted the file. The cost is a Linux user occasionally
/// seeing an underscore where a `:` would have been legal, which is
/// visible, disclosed, and trivially overridable.
///
/// # What it does NOT do
///
/// It does not resolve collisions (two attachments legitimately named
/// `notes.txt`), it does not check that the destination directory contains
/// the result, and it does not decide whether the file is safe to *open*.
/// Those are the caller's, and the last one is not this module's business
/// at all (module docs: pdfcer never runs an attachment).
///
/// # Examples
///
/// ```
/// use pdfcer_core::attachments::{sanitize_attachment_name, NameHazard};
///
/// // Ordinary names pass through untouched, and say so.
/// let ok = sanitize_attachment_name("quarterly-report.pdf");
/// assert_eq!(ok.value, "quarterly-report.pdf");
/// assert!(!ok.changed);
/// assert!(ok.hazards.is_empty());
///
/// // A traversal is reduced to its last component, and the reason is kept.
/// let bad = sanitize_attachment_name(r"..\..\..\Windows\System32\evil.exe");
/// assert_eq!(bad.value, "evil.exe");
/// assert!(bad.changed);
/// assert!(bad.hazards.contains(&NameHazard::PathSeparator));
///
/// // A reserved device name is defused without losing what it was.
/// assert_eq!(sanitize_attachment_name("CON.txt").value, "_CON.txt");
///
/// // A NUL extension-spoof cannot survive.
/// let spoof = sanitize_attachment_name("invoice.pdf\u{0}.exe");
/// assert_eq!(spoof.value, "invoice.pdf_.exe");
/// assert!(spoof.hazards.contains(&NameHazard::ControlCharacter));
///
/// // A right-to-left override that made `gnp.exe` render as `exe.png`
/// // is removed, and the removal is reported.
/// let bidi = sanitize_attachment_name("\u{202E}gnp.exe");
/// assert_eq!(bidi.value, "gnp.exe");
/// assert!(bidi.hazards.contains(&NameHazard::BidiOverride));
///
/// // Nothing usable at all still yields a usable name.
/// assert_eq!(sanitize_attachment_name("../").value, "attachment");
/// ```
#[must_use]
pub fn sanitize_attachment_name(raw: &str) -> SafeName {
    let mut hazards: Vec<NameHazard> = Vec::new();

    // 1. Last path component only.
    let mut work = raw;
    if let Some(pos) = raw.rfind(['/', '\\']) {
        hazards.push(NameHazard::PathSeparator);
        // `pos` is a char boundary returned by `rfind`, and `+ 1` lands on
        // the next boundary because both separators are one byte in UTF-8.
        work = raw.get(pos + 1..).unwrap_or("");
    }
    // A traversal component is worth naming even when the separator split
    // already removed it — the operator wants to know it was a traversal,
    // not merely that it had slashes in it.
    if raw
        .split(['/', '\\'])
        .any(|part| part == ".." || part == ".")
    {
        hazards.push(NameHazard::ParentTraversal);
    }
    if work == ".." || work == "." {
        hazards.push(NameHazard::ParentTraversal);
        work = "";
    }

    // 2. Character-level scrub.
    let mut scrubbed = String::with_capacity(work.len());
    for ch in work.chars() {
        match ch {
            '\0' => {
                hazards.push(NameHazard::ControlCharacter);
                scrubbed.push('_');
            }
            c if c.is_control() => {
                hazards.push(NameHazard::ControlCharacter);
                scrubbed.push('_');
            }
            '\u{FFFD}' => {
                // See NameHazard::UndecodableBytes — this is where a NUL
                // that PDFDocEncoding already turned into U+FFFD gets
                // caught.
                hazards.push(NameHazard::UndecodableBytes);
                scrubbed.push('_');
            }
            ':' => {
                // Both a drive designator and an NTFS alternate-data-stream
                // selector, which is why it gets its own hazard rather than
                // being lumped in with `<>"|?*`.
                hazards.push(NameHazard::DriveOrStream);
                scrubbed.push('_');
            }
            '<' | '>' | '"' | '|' | '?' | '*' => {
                hazards.push(NameHazard::ReservedCharacter);
                scrubbed.push('_');
            }
            // Bidi controls: DROPPED, not substituted. See
            // NameHazard::BidiOverride — they are zero-width by design and
            // exist only to lie about the rendered order of the name.
            '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' => {
                hazards.push(NameHazard::BidiOverride);
            }
            other => scrubbed.push(other),
        }
    }

    // 4. Trailing dots and spaces — Windows strips them at creation time,
    //    so leaving them would mean the file on disk has a different name
    //    than the one pdfcer showed.
    let trimmed = scrubbed.trim_end_matches(['.', ' ']);
    if trimmed.len() != scrubbed.len() {
        hazards.push(NameHazard::TrailingDotOrSpace);
    }
    let mut value = trimmed.to_owned();

    // 5. Reserved device names, checked on the stem and case-insensitively.
    if is_reserved_device_name(&value) {
        hazards.push(NameHazard::ReservedDeviceName);
        value.insert(0, '_');
    }

    // 6. Length.
    if value.chars().count() > MAX_SAFE_NAME_CHARS {
        hazards.push(NameHazard::TooLong);
        value = value.chars().take(MAX_SAFE_NAME_CHARS).collect();
        // Truncation can re-expose a trailing dot/space; re-trim, but do
        // not report the hazard twice — the truncation is the story.
        value = value.trim_end_matches(['.', ' ']).to_owned();
    }

    // 7. Fallback.
    if value.is_empty() {
        hazards.push(NameHazard::Empty);
        value = FALLBACK_SAFE_NAME.to_owned();
    }

    hazards.sort_unstable();
    hazards.dedup();
    let changed = value != raw;
    SafeName {
        value,
        changed,
        hazards,
    }
}

/// Whether `name`'s stem is one of Windows' reserved device names.
///
/// The reservation is on the part before the **first** `.`, is
/// case-insensitive, and holds regardless of extension: `CON`, `con.txt`
/// and `Con.tar.gz` are all the console device. Trailing spaces in the
/// stem are also ignored by Windows, so they are trimmed before the
/// comparison — otherwise `"CON .txt"` would slip through.
fn is_reserved_device_name(name: &str) -> bool {
    const RESERVED: [&str; 4] = ["CON", "PRN", "AUX", "NUL"];

    let stem = name
        .split('.')
        .next()
        .unwrap_or("")
        .trim_end_matches(' ')
        .to_ascii_uppercase();
    if RESERVED.contains(&stem.as_str()) {
        return true;
    }
    // COM0–COM9 and LPT0–LPT9. (COM0/LPT0 are not documented as devices by
    // every Microsoft page, but they are reserved by the same naming rule
    // and cost nothing to include.)
    for prefix in ["COM", "LPT"] {
        if let Some(rest) = stem.strip_prefix(prefix)
            && rest.len() == 1
            && rest.chars().all(|c| c.is_ascii_digit())
        {
            return true;
        }
    }
    false
}

#[cfg(test)]
// Tests are exempt from the crate's panic-free policy: a panicking
// assertion IS the test-failure mechanism (see `lib.rs`'s lint rationale).
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;
    use crate::document::Document;

    fn load(name: &str) -> Document {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/synthetic/attachments")
            .join(name);
        let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        Document::from_bytes(bytes).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()))
    }

    // -- document-level --------------------------------------------------

    /// Catches: the `/Names /EmbeddedFiles` walk not happening at all, and
    /// every §7.11.3/§7.11.4 optional key being dropped on the floor.
    #[test]
    fn document_level_entry_is_read_whole() {
        let doc = load("doc-level-simple.pdf");
        let (found, notes) = list_attachments_with_notes(&doc);
        assert_eq!(found.len(), 1);
        let a = &found[0];

        assert_eq!(a.name, "notes.txt");
        assert_eq!(a.name_source, NameSource::Uf);
        assert!(a.name_exact);
        assert_eq!(a.description.as_deref(), Some("A plain-text note"));
        assert!(matches!(a.kind, AttachmentKind::DocumentLevel { .. }));
        assert_eq!(a.declared_size, Some(17));
        // §7.3.5: `/text#2Fplain` is the NAME `text/plain`. A reader that
        // does not apply `#` escapes reports the MIME type wrong.
        assert_eq!(a.mime.as_deref(), Some("text/plain"));
        assert_eq!(a.created.as_deref(), Some("D:20260101000000Z"));
        assert_eq!(a.modified.as_deref(), Some("D:20260810123000Z"));
        assert_eq!(a.checksum.as_deref(), Some(&b"\x01\x02\x03\x04"[..]));
        assert!(a.stream_id.is_some());
        assert_eq!(a.ef_key.as_deref(), Some(&b"F"[..]));
        assert_eq!(notes, AttachmentNotes::default());
    }

    /// Catches: falling back to the name-tree key when `/UF` exists, and
    /// treating UTF-16BE `/UF` bytes as Latin-1. The fixture makes the
    /// key, `/F` and `/UF` three different strings so only one answer is
    /// possible.
    #[test]
    fn uf_wins_over_f_and_over_the_tree_key() {
        let doc = load("doc-level-unicode-name.pdf");
        let found = list_attachments(&doc);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "résumé-Σ.txt");
        assert_eq!(found[0].name_source, NameSource::Uf);
        // The key is still recoverable — a write path needs it.
        match &found[0].kind {
            AttachmentKind::DocumentLevel { tree_key } => {
                assert_eq!(tree_key.as_slice(), b"tree-key-differs.txt");
            }
            other => panic!("expected a document-level entry, got {other:?}"),
        }
    }

    /// Catches: reading only the root node's `/Names` and reporting a
    /// three-file document as empty. §7.9.6 lets the root be pure `/Kids`.
    #[test]
    fn a_name_tree_with_interior_nodes_is_fully_walked() {
        let doc = load("doc-level-kids-tree.pdf");
        let (found, notes) = list_attachments_with_notes(&doc);
        let names: Vec<&str> = found.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, ["alpha.txt", "mike.txt", "zulu.txt"]);
        assert_eq!(notes, AttachmentNotes::default());
    }

    // -- page-level ------------------------------------------------------

    /// Catches: the whole page-level mechanism being missed. This document
    /// has NO `/Names` at all, so a document-level-only reader reports it
    /// as having no attachments — which is the conflation this module
    /// exists to prevent.
    #[test]
    fn a_file_attachment_annotation_is_found_without_any_name_tree() {
        let doc = load("page-level-annot.pdf");
        let found = list_attachments(&doc);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "pinned.txt");
        match &found[0].kind {
            AttachmentKind::PageAnnotation {
                page_index, icon, ..
            } => {
                assert_eq!(*page_index, 0);
                assert_eq!(icon.as_deref(), Some(&b"Paperclip"[..]));
            }
            other => panic!("expected a page annotation, got {other:?}"),
        }
    }

    /// Catches: returning both kinds but labelling them the same, and
    /// hard-coding page 0. The fixture's annotation is on the SECOND page.
    #[test]
    fn both_kinds_are_returned_in_one_list_and_told_apart() {
        let doc = load("both-kinds.pdf");
        let found = list_attachments(&doc);
        assert_eq!(found.len(), 2);

        match &found[0].kind {
            AttachmentKind::DocumentLevel { .. } => {}
            other => panic!("document-level entries come first; got {other:?}"),
        }
        assert_eq!(found[0].name, "whole-document.txt");

        match &found[1].kind {
            AttachmentKind::PageAnnotation {
                page_index,
                page_id,
                annot_id,
                ..
            } => {
                assert_eq!(*page_index, 1, "the annotation is on the second page");
                assert!(annot_id.is_some());
                // The page id must be the page the annotation is actually
                // on, not merely the first page's.
                let pages = crate::page_tree::pages(&doc).unwrap();
                assert_eq!(*page_id, pages[1].id);
            }
            other => panic!("expected a page annotation, got {other:?}"),
        }
        assert_eq!(found[1].name, "on-page-two.txt");
    }

    /// Catches a `shall` that is easy to violate while looking correct:
    /// §12.5.6.15 requires an annotation's `/Contents` to be used **rather
    /// than** the filespec's `/Desc`. The fixture reaches ONE filespec both
    /// ways, so the two rows must show two different descriptions off the
    /// same dictionary — and must not be de-duplicated into one.
    ///
    /// The first implementation of this module read `/Desc` for both
    /// routes; the defect was invisible until the spec was actually
    /// sourced.
    #[test]
    fn an_annotation_description_comes_from_contents_not_desc() {
        let doc = load("annot-contents-beats-desc.pdf");
        let found = list_attachments(&doc);
        assert_eq!(found.len(), 2);

        let doc_level = found
            .iter()
            .find(|a| matches!(a.kind, AttachmentKind::DocumentLevel { .. }))
            .unwrap();
        let annot = found
            .iter()
            .find(|a| matches!(a.kind, AttachmentKind::PageAnnotation { .. }))
            .unwrap();

        assert_eq!(
            doc_level.description.as_deref(),
            Some("DESC from the file specification")
        );
        assert_eq!(
            annot.description.as_deref(),
            Some("CONTENTS from the annotation"),
            "§12.5.6.15: the annotation's /Contents is used RATHER THAN /Desc"
        );

        // Same file, two routes, two lifetimes. Equal `filespec_id` is the
        // documented signal for that — and the rows stay separate.
        assert_eq!(doc_level.filespec_id, annot.filespec_id);
        assert!(doc_level.filespec_id.is_some());
    }

    /// Catches: consulting only `/F` and `/UF`. Table 44 makes `/F`
    /// required *only* when `/DOS`, `/Mac` and `/Unix` are all absent, so a
    /// platform-slots-only filespec is conforming — and it is the shape the
    /// standard's own §7.11.4 example uses.
    ///
    /// A reader without the fallback reports this legal document as
    /// containing a nameless attachment with no bytes.
    #[test]
    fn platform_only_filename_and_ef_slots_are_still_read() {
        let doc = load("ef-platform-slots.pdf");
        let (found, notes) = list_attachments_with_notes(&doc);
        assert_eq!(found.len(), 1);
        let a = &found[0];

        // NAME_SLOT_ORDER puts /DOS ahead of /Mac and /Unix.
        assert_eq!(a.name, "DOSNAME.TXT");
        assert_eq!(a.name_source, NameSource::Dos);
        // The name-tree key is NOT what got used, even though it is the
        // most filename-looking string in the document.
        assert_ne!(a.name_source, NameSource::TreeKey);

        // The payload came from /EF /Unix, and pdfcer says which slot.
        assert!(a.stream_id.is_some());
        assert_eq!(a.ef_key.as_deref(), Some(&b"Unix"[..]));
        assert_eq!(notes.filespecs_without_stream, 0);
        assert_eq!(notes.unresolvable_streams, 0);

        let view = doc.view();
        assert_eq!(
            attachment_bytes(&view, a).as_deref(),
            Some(&b"platform payload\n"[..])
        );
    }

    /// Catches: the encryption trap going unflagged. §7.6.5 lets an
    /// *otherwise unencrypted* document carry encrypted embedded files via
    /// `/EFF`, so a caller cannot infer plaintext from the absence of a
    /// password prompt — the flag is the only channel that says so.
    #[test]
    fn an_unencrypted_document_is_not_flagged_as_maybe_encrypted() {
        for name in ["doc-level-simple.pdf", "both-kinds.pdf"] {
            let doc = load(name);
            let (_, notes) = list_attachments_with_notes(&doc);
            assert!(!notes.may_be_encrypted, "{name}");
        }
    }

    // -- declared size ---------------------------------------------------

    /// Catches: presenting `/Params /Size` as a measurement. The fixture
    /// declares 999999 over a ten-byte unfiltered stream, so the lie is
    /// provable without decoding anything.
    #[test]
    fn a_lying_size_is_caught_cheaply_at_listing_time() {
        let doc = load("size-lies.pdf");
        let found = list_attachments(&doc);
        assert_eq!(found[0].declared_size, Some(999_999));
        assert_eq!(
            found[0].size_check,
            DeclaredSizeCheck::Disagrees {
                declared: 999_999,
                actual: 10
            }
        );
        assert!(found[0].size_check.is_contradicted());
    }

    /// Catches: the *opposite* defect — validating `/Size` against
    /// `/Length` (the ENCODED length) and crying wolf on every compressed
    /// attachment. Here `/Size` is honest and the raw span is much
    /// smaller, so the only correct listing-time verdict is "unverified".
    #[test]
    fn a_compressed_attachment_is_unverified_not_accused() {
        let doc = load("flate-size-truth.pdf");
        let found = list_attachments(&doc);
        assert_eq!(found[0].declared_size, Some(4096));
        assert_eq!(found[0].size_check, DeclaredSizeCheck::Unverified);
        assert!(!found[0].size_check.is_contradicted());

        // And decoding settles it in the document's favour.
        let view = doc.view();
        let got = extract_attachment(&view, &found[0]).unwrap();
        assert_eq!(got.data.len(), 4096);
        assert_eq!(got.size_check, DeclaredSizeCheck::Agrees { bytes: 4096 });
    }

    /// Catches: extraction returning the raw (still-Flate-encoded) span.
    #[test]
    fn extraction_runs_the_filter_chain() {
        let doc = load("flate-size-truth.pdf");
        let view = doc.view();
        let found = list_attachments(&doc);
        assert_eq!(attachment_bytes(&view, &found[0]), Some(vec![b'A'; 4096]));
    }

    // -- degradation -----------------------------------------------------

    /// Catches: any of the five malformations in `degenerate.pdf` being
    /// fatal, silent, or an infinite loop. The one good entry must survive
    /// all of them, and every degradation must be counted.
    #[test]
    fn malformed_entries_degrade_and_are_counted() {
        let doc = load("degenerate.pdf");
        let (found, notes) = list_attachments_with_notes(&doc);

        let names: Vec<&str> = found.iter().map(|a| a.name.as_str()).collect();
        assert!(names.contains(&"good.txt"), "got {names:?}");
        // The external reference and the dangling stream are LISTED — an
        // attachment the document names but cannot deliver is information,
        // not noise.
        assert!(names.contains(&"elsewhere.txt"), "got {names:?}");
        assert!(names.contains(&"vanished.txt"), "got {names:?}");

        let external = found.iter().find(|a| a.name == "elsewhere.txt").unwrap();
        assert_eq!(external.stream_id, None);
        assert_eq!(external.size_check, DeclaredSizeCheck::NotDeclared);

        let dangling = found.iter().find(|a| a.name == "vanished.txt").unwrap();
        assert_eq!(dangling.stream_id, None);

        assert!(notes.name_tree_cycles > 0, "the /Kids cycle must be seen");
        assert!(
            notes.malformed_tree_entries >= 2,
            "the odd-length array and the integer value must both count"
        );
        assert!(notes.filespecs_without_stream >= 1);
        assert!(notes.unresolvable_streams >= 1);
        assert!(!notes.truncated);
    }

    /// Catches: extraction panicking, or inventing bytes, for an
    /// attachment that has none.
    #[test]
    fn extracting_an_external_reference_names_the_refusal() {
        let doc = load("degenerate.pdf");
        let view = doc.view();
        let found = list_attachments(&doc);
        let external = found.iter().find(|a| a.name == "elsewhere.txt").unwrap();
        assert_eq!(
            extract_attachment(&view, external).unwrap_err(),
            AttachmentError::NoEmbeddedStream
        );
        assert_eq!(attachment_bytes(&view, external), None);
    }

    /// Catches: a document with no attachments at all producing anything
    /// other than a clean empty answer.
    #[test]
    fn a_document_without_attachments_is_empty_and_unremarkable() {
        let bytes = include_bytes!("../../../fixtures/synthetic/hello.pdf").to_vec();
        let doc = Document::from_bytes(bytes).unwrap();
        let (found, notes) = list_attachments_with_notes(&doc);
        assert!(found.is_empty());
        assert_eq!(notes, AttachmentNotes::default());
    }

    // -- filename safety -------------------------------------------------

    /// Catches: the raw name being quietly sanitised. The document says
    /// what it says, and the listing must repeat it — the sanitiser is a
    /// separate, opt-in step.
    #[test]
    fn hostile_names_are_reported_verbatim_by_the_listing() {
        let doc = load("hostile-names.pdf");
        let found = list_attachments(&doc);
        let names: Vec<&[u8]> = found.iter().map(|a| a.name_bytes.as_slice()).collect();
        assert!(names.contains(&&b"..\\..\\..\\Windows\\System32\\evil.exe"[..]));
        assert!(names.contains(&&b"/etc/cron.d/pwn"[..]));
        assert!(names.contains(&&b"ok.txt\x00.exe"[..]));
        assert!(names.contains(&&b"CON.txt"[..]));
    }

    /// Catches: the sanitiser missing any of the four hazard shapes the
    /// fixture carries, and catches it through the real read path rather
    /// than on hand-written strings.
    #[test]
    fn every_hostile_fixture_name_survives_sanitisation_safely() {
        let doc = load("hostile-names.pdf");
        for a in list_attachments(&doc) {
            // Through the convenience method, so that path is covered too.
            let safe = a.safe_name();
            assert_eq!(safe, sanitize_attachment_name(&a.name));
            assert!(safe.changed, "{:?} should have been changed", a.name);
            assert!(!safe.hazards.is_empty());
            assert!(!safe.value.is_empty());
            assert!(!safe.value.contains(['/', '\\', '\0', ':']));
            assert_ne!(safe.value, "..");
            assert!(!is_reserved_device_name(&safe.value));
        }
    }

    /// Catches: a safe name being needlessly rewritten, which would make
    /// `changed` useless as a "tell the operator" trigger.
    #[test]
    fn an_already_safe_name_is_untouched_and_says_so() {
        for name in [
            "notes.txt",
            "Quarterly Report 2026.pdf",
            "résumé-Σ.txt",
            "a.tar.gz",
            "-leading-dash.txt",
        ] {
            let safe = sanitize_attachment_name(name);
            assert_eq!(safe.value, name);
            assert!(!safe.changed, "{name} was rewritten");
            assert!(safe.hazards.is_empty(), "{name} -> {:?}", safe.hazards);
        }
    }

    /// Catches: path traversal surviving in any of its shapes.
    #[test]
    fn traversals_are_reduced_to_a_single_component() {
        let cases = [
            (r"..\..\..\Windows\System32\evil.exe", "evil.exe"),
            ("/etc/cron.d/pwn", "pwn"),
            ("../../secret", "secret"),
            ("a/b/c/d.txt", "d.txt"),
        ];
        for (raw, want) in cases {
            let safe = sanitize_attachment_name(raw);
            assert_eq!(safe.value, want, "{raw}");
            assert!(safe.hazards.contains(&NameHazard::PathSeparator));
        }
        // A name that is ONLY a traversal has nothing left.
        let nothing = sanitize_attachment_name("..");
        assert_eq!(nothing.value, FALLBACK_SAFE_NAME);
        assert!(nothing.hazards.contains(&NameHazard::ParentTraversal));
        assert!(nothing.hazards.contains(&NameHazard::Empty));
    }

    /// Catches: the reserved-device-name rule being applied to the whole
    /// name instead of the stem, or case-sensitively.
    #[test]
    fn reserved_device_names_are_defused_in_every_spelling() {
        for raw in [
            "CON",
            "con.txt",
            "Con.tar.gz",
            "NUL.dat",
            "COM1.txt",
            "lpt9",
        ] {
            let safe = sanitize_attachment_name(raw);
            assert!(
                safe.value.starts_with('_'),
                "{raw} -> {} was not defused",
                safe.value
            );
            assert!(safe.hazards.contains(&NameHazard::ReservedDeviceName));
        }
        // Not reserved: a longer word that merely starts the same way.
        for raw in ["CONTRACT.txt", "console.log", "COM10.txt", "COMMS"] {
            let safe = sanitize_attachment_name(raw);
            assert_eq!(safe.value, raw, "{raw} was wrongly treated as a device");
        }
    }

    /// Catches: NUL-byte extension spoofing surviving, which is the
    /// highest-severity single item on the hazard list.
    #[test]
    fn nul_and_control_bytes_cannot_survive() {
        let safe = sanitize_attachment_name("invoice.pdf\u{0}.exe");
        assert_eq!(safe.value, "invoice.pdf_.exe");
        assert!(safe.hazards.contains(&NameHazard::ControlCharacter));

        let tabbed = sanitize_attachment_name("a\tb\r\n.txt");
        assert_eq!(tabbed.value, "a_b__.txt");
        assert!(tabbed.hazards.contains(&NameHazard::ControlCharacter));
    }

    /// Catches the hole that the first implementation actually had, and
    /// that reasoning alone had missed: a NUL inside a `/F` byte string is
    /// **not** a NUL by the time it reaches the sanitiser. PDFDocEncoding
    /// leaves `0x00` undefined, so [`crate::textstring::decode_text_string`]
    /// has already turned it into U+FFFD — and a sanitiser checking only
    /// `char::is_control` waves the extension-spoof straight through.
    ///
    /// This test goes through the real fixture rather than a hand-written
    /// string precisely because that is what exposed it.
    #[test]
    fn a_nul_spoof_arriving_as_u_fffd_is_still_caught() {
        let doc = load("hostile-names.pdf");
        let found = list_attachments(&doc);
        let spoof = found
            .iter()
            .find(|a| a.name_bytes.contains(&0))
            .expect("the fixture carries a NUL-bearing name");

        // The decode was lossy, and pdfcer says so rather than pretending.
        assert!(!spoof.name_exact);
        assert!(spoof.name.contains('\u{FFFD}'));
        assert!(!spoof.name.contains('\0'));

        let safe = spoof.safe_name();
        assert_eq!(safe.value, "ok.txt_.exe");
        assert!(safe.changed);
        assert!(safe.hazards.contains(&NameHazard::UndecodableBytes));
        assert!(!safe.value.contains('\u{FFFD}'));
    }

    /// Catches: a bidirectional-override spoof surviving into a filename.
    ///
    /// `"\u{202E}gnp.exe"` renders as `exe.png` — this is a lie aimed at
    /// the operator's eyes rather than at a parser, and an attachment name
    /// is an ideal delivery vehicle for it.
    #[test]
    fn bidi_overrides_that_reverse_a_displayed_extension_are_removed() {
        let spoof = sanitize_attachment_name("\u{202E}gnp.exe");
        assert_eq!(spoof.value, "gnp.exe");
        assert!(spoof.changed);
        assert!(spoof.hazards.contains(&NameHazard::BidiOverride));

        // Every control in the family, and the isolates too.
        for ch in [
            '\u{200E}', '\u{200F}', '\u{202A}', '\u{202B}', '\u{202C}', '\u{202D}', '\u{202E}',
            '\u{2066}', '\u{2067}', '\u{2068}', '\u{2069}',
        ] {
            let safe = sanitize_attachment_name(&format!("a{ch}b.txt"));
            assert_eq!(safe.value, "ab.txt", "U+{:04X} survived", ch as u32);
            assert!(safe.hazards.contains(&NameHazard::BidiOverride));
        }
    }

    /// Catches: Windows silently eating a trailing dot so that the file on
    /// disk has a different name than the one pdfcer displayed.
    #[test]
    fn trailing_dots_and_spaces_are_stripped() {
        let safe = sanitize_attachment_name("evil.exe. ");
        assert_eq!(safe.value, "evil.exe");
        assert!(safe.hazards.contains(&NameHazard::TrailingDotOrSpace));
    }

    /// Catches: an unbounded name reaching a filesystem call, and catches
    /// a truncation that re-introduces a trailing dot.
    #[test]
    fn absurdly_long_names_are_truncated() {
        let raw = "x".repeat(MAX_SAFE_NAME_CHARS * 3);
        let safe = sanitize_attachment_name(&raw);
        assert_eq!(safe.value.chars().count(), MAX_SAFE_NAME_CHARS);
        assert!(safe.hazards.contains(&NameHazard::TooLong));

        let dotty = format!("{}...", "y".repeat(MAX_SAFE_NAME_CHARS));
        let safe = sanitize_attachment_name(&dotty);
        assert!(!safe.value.ends_with('.'));
    }

    /// Catches: the `:` case being lumped in with the other forbidden
    /// characters and losing the alternate-data-stream explanation, and
    /// catches a Windows drive prefix surviving.
    #[test]
    fn colons_are_neutralised_as_drive_or_stream_selectors() {
        let ads = sanitize_attachment_name("report.txt:hidden");
        assert_eq!(ads.value, "report.txt_hidden");
        assert!(ads.hazards.contains(&NameHazard::DriveOrStream));

        let drive = sanitize_attachment_name(r"C:\Users\x\evil.exe");
        assert_eq!(drive.value, "evil.exe");
    }

    /// Catches: hazards being reported in a nondeterministic order or
    /// duplicated, either of which would make a UI message unstable.
    #[test]
    fn hazards_are_sorted_and_deduplicated() {
        let safe = sanitize_attachment_name("a<b<c>d.txt");
        assert_eq!(safe.hazards, vec![NameHazard::ReservedCharacter]);

        let many = sanitize_attachment_name("../CON\0.txt");
        let mut sorted = many.hazards.clone();
        sorted.sort_unstable();
        assert_eq!(many.hazards, sorted);
        assert!(many.hazards.windows(2).all(|w| w[0] != w[1]));
    }
}
