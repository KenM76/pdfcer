//! Font **unembedding** — planning the removal of an embedded font program,
//! and stating everything the removal changes before it changes anything.
//!
//! This is the *action* half of the font-cleanup story. [`crate::fontinfo`]
//! is the *report* half, and this module **consumes its
//! [`Removability`](crate::fontinfo::Removability) verdict rather than
//! deriving one of its own**. There is exactly one classifier in pdfcer, so
//! the panel that says "this font can go" and the command that makes it go
//! cannot disagree. Nothing here re-reads an `/Encoding`, re-checks a
//! symbolic flag, or re-decides what `Identity-H` means; if a verdict is
//! wrong it is wrong in one place.
//!
//! Nothing in this module mutates a document. It produces an
//! [`UnembedPlan`] — a description of what *would* happen — and
//! [`EditSession::unembed_fonts`](crate::edit::EditSession::unembed_fonts)
//! executes it through the command log. The split exists because rule 4
//! (fuzzy, never sneaky) requires the operator to see the consequences
//! before they become document state, and a preview that ran different code
//! from the commit would be a disclosure that could lie.
//!
//! # What unembedding actually is
//!
//! Remove `/FontFile`, `/FontFile2` or `/FontFile3` from the
//! `/FontDescriptor` (§9.9 Table 126), leaving a font reference the reader
//! satisfies by **substitution**. §9.8.1 states that this is what the rest
//! of the descriptor is *for*:
//!
//! > "These font metrics provide information that enables a conforming
//! > reader to **synthesize a substitute font or select a similar font when
//! > the font program is unavailable**."
//!
//! So the operation is not a mutilation of the descriptor — it moves the
//! descriptor from its secondary job (carrying a program) back to its
//! primary one (describing a face well enough to stand one in). Everything
//! §9.8.1 names as substitution input — `/Flags`, `/FontBBox`,
//! `/ItalicAngle`, `/Ascent`, `/Descent`, `/CapHeight`, `/StemV`,
//! `/MissingWidth` — is therefore **kept**, and so is the font dictionary's
//! `/Widths` array. Removing any of them would break the substitution the
//! operation depends on.
//!
//! # The rule that decides everything else
//!
//! **Only a font whose verdict is
//! [`Removability::Removable`](crate::fontinfo::Removability::Removable) may
//! be unembedded.** Every other verdict refuses **by name, with its reason
//! shown** — never silently, never merely absent from a list.
//!
//! That is a deliberate divergence from Acrobat, which refuses the same
//! fonts *silently*: the font simply does not appear in the Optimizer's
//! unembed list, with no reason given anywhere on screen (sourced to a
//! former Adobe Principal Scientist in
//! `Acrobat_Features/optimize__font_unembedding.md`, and independently
//! corroborated by a user whose largest font was absent from the list with
//! no explanation).
//!
//! Refusal is not the edge case, and the shape of it was measured rather
//! than assumed. `tools/unembed-sweep` over **4,023 real-world files**
//! (pdfbox, pdfium, qpdf, pdf20examples, veraPDF-corpus) examined 3,097
//! distinct fonts, of which 1,560 carried a readable embedded program:
//!
//! | Verdict | Share of the 1,560 embedded |
//! |---|---|
//! | `unknown-symbolic-builtin` | 53.6 % |
//! | `removable` | 28.8 % |
//! | `blocked-identity` | 12.9 % |
//! | `unknown-embedded-cmap` | 4.4 % |
//! | `unknown-predefined-cmap` | 0.4 % |
//!
//! **Seven embedded fonts in ten refuse.** A refusal path that was an
//! afterthought would therefore be the path most operators actually take.
//!
//! The other two denominators the same sweep reports, because a table
//! headed "refusal reasons" invites a reader to supply whichever one they
//! had in mind — and one of them was mislabelled once, in a commit message,
//! before anyone re-derived it. Of the **2,648 fonts that refused**,
//! `not-embedded` is 56.1 % and `unknown-symbolic-builtin` is 31.6 %; the
//! same 836 symbolic fonts are 53.6 % of the *embedded* set above. Both
//! figures are correct and they answer different questions. The 1,560
//! denominator is `3,097 − 1,485 not-embedded − 49 Type 3 − 3 unreadable`:
//! every font that actually carries a readable program.
//!
//! ★ **This differs from the smaller measurement the Pass brief carried**,
//! and both are kept because they are different measurements rather than a
//! correction. A 400-file sample recorded 466 fonts / 117 embedded and put
//! `removable` at 48 % with `blocked-identity` at 34 %. Over ten times the
//! corpus the dominant blocker is not glyph-index encoding at all — it is a
//! **symbolic font with no `/Encoding`**, where §9.6.6.1 sends the codes
//! through the program's own `cmap` and the answer is genuinely unknown
//! rather than knowably unsafe. The design consequence is the same either
//! way (refuse, and say why), but anyone sizing the *value* of a future
//! re-encoding Pass should size it against `unknown-symbolic-builtin`.
//!
//! # ★ Two decisions this module makes, and Acrobat does not answer
//!
//! ## 1. The subset tag is STRIPPED (default), and the descriptor follows
//!
//! 87 % of embedded fonts in the corpus are subsets, named `ABCDEF+Arial`
//! per §9.6.4 ("a tag followed by a plus sign… exactly six uppercase
//! letters"). Whether Acrobat strips that tag from the finished file's
//! `/BaseFont` when it unembeds is an explicit **GAP** in the parity
//! research — no source found either way — so this is pdfcer's own decision,
//! made from the standard rather than inferred from a product.
//!
//! **`ABCDEF+Arial` names a subset that no longer exists.** §9.6.4 gives
//! the tag exactly one job: it lets a reader *"recognize font subsets and
//! merge documents containing different subsets of the same font."* After
//! the program is gone there is no subset in the file to recognise, and the
//! name has silently changed role — it is now a **substitution key**, the
//! string a reader matches against installed faces. No clause of ISO 32000
//! requires a reader to strip the tag before matching, so `ABCDEF+Arial` is
//! a face name that matches nothing on any system, and what a viewer does
//! next is undefined.
//!
//! Acrobat's own Optimizer batch syntax accepts `+Helvetica` to mean "any
//! subset of Helvetica", which shows Acrobat treats the tag as noise **for
//! matching**; it says nothing about what it writes. That is corroboration
//! for the direction, not a source for the behaviour.
//!
//! **`/FontName` moves with `/BaseFont`, and that part is a `shall`.**
//! §9.8.1 Table 122: *"`FontName` … **shall be the same as the value of
//! `BaseFont`** in the font or CIDFont dictionary that refers to this font
//! descriptor."* So the two are renamed together or not at all; a stripped
//! `/BaseFont` beside a tagged `/FontName` would be a conformance defect
//! introduced by the cleanup.
//!
//! **A name that carries no tag is not renamed**, and neither is one whose
//! `+` is not a §9.6.4 tag — [`split_subset_tag`] is strict in both
//! directions (five letters, seven letters, any lowercase, or nothing after
//! the `+` all mean "no tag, this is the name"), and pdfcer mangling a font
//! genuinely called `AB+Condensed` would be pdfcer inventing data.
//!
//! The policy is overridable ([`SubsetTagPolicy::Keep`]) because a caller
//! comparing pdfcer's output against another tool's byte-for-byte has a
//! legitimate reason to want the name untouched — but the default is the
//! decision above, and the rename is always disclosed.
//!
//! ## 2. `/CIDSet` and `/CharSet` go with the program
//!
//! Both are optional descriptor entries that describe **which glyphs the
//! font program contains**. §9.9: subsetting *"may be indicated by the
//! presence of a `CharSet` or `CIDSet` entry in the font descriptor **that
//! refers to the font file**"* — a `may`, and one whose subject is the font
//! file that is being deleted.
//!
//! `/CIDSet`'s Table 124 wording settles it: *"A stream identifying which
//! CIDs are present in **the CIDFont file**. **If this entry is present,
//! the CIDFont shall contain only a subset** of the glyphs…"* That `shall`
//! is conditioned on the entry's presence, and after unembedding the
//! CIDFont contains no glyphs at all — so leaving the entry leaves a
//! `shall` the file cannot satisfy. It is also a **stream**, so removing it
//! frees an object and reclaims bytes.
//!
//! `/CharSet` (Table 122, Type 1 only) is the simple-font analogue: *"A
//! string listing the character names **defined in a font subset**."* Same
//! subject, same conclusion; it is a string rather than a stream, so it
//! costs the descriptor a key and frees no object.
//!
//! Neither is required for substitution — §9.8.1's list of what a reader
//! uses to synthesize a face does not include them — so removing them costs
//! the operation nothing and removes two assertions that have become false.
//!
//! **`/CIDSet` is unreachable in practice today**, and is handled anyway.
//! `Removable` is currently only ever produced for a *simple* font
//! (`Type1`/`MMType1`/`TrueType`), and `/CIDSet` belongs on a CIDFont
//! descriptor — so a conforming document cannot present the combination.
//! It is removed when found because a descriptor carrying it is malformed
//! in a way that says "this used to be a subset", and leaving it would
//! preserve exactly the false claim the paragraph above rejects.
//!
//! # What is NOT changed, and why each stays
//!
//! | Entry | Kept because |
//! |---|---|
//! | `/Widths`, `/FirstChar`, `/LastChar` | Table 111: the reader positions text from `/Widths`, not from the program. They are what keeps every glyph's *advance* identical after substitution. Removing them would move the text. |
//! | `/Flags`, `/FontBBox`, `/ItalicAngle`, `/StemV`, `/Ascent`, `/Descent`, `/CapHeight` | §9.8.1 — this is the substitution input. They describe the subset rather than the whole face, which is imprecise but is strictly better than nothing. |
//! | `/Encoding`, `/Differences` | The verdict `Removable` **means** these define the codes independently of the program. They are the reason removal is safe. |
//! | `/ToUnicode` | Text extraction and search are unaffected by unembedding and must stay that way. |
//! | The content streams | Not touched at all. No glyph moves; no operator changes. |
//!
//! # ★ Appearance WILL change, and the disclosure says so as a fact
//!
//! `/Widths` are preserved, so each glyph occupies exactly the same advance
//! it did. The **glyph itself** is drawn by whatever face the reader
//! substitutes, whose own advances are not those widths. Table 111 requires
//! `/Widths` to be *"consistent with the actual widths given in the font
//! program"* — a requirement that was true of the program just deleted and
//! is not true of the substitute. The visible result is letters that sit
//! too loosely or too tightly inside correctly-placed cells, with different
//! shapes and stroke weights.
//!
//! This is stated as a certainty rather than a risk. It is not a
//! probabilistic warning; it is what the operation does.
//!
//! # Hazards this module exists to get right
//!
//! **A shared font program must not be freed.** Two font dictionaries can
//! legally point at one `/FontFile2` stream, or at one `/FontDescriptor`.
//! Unembedding one of them and freeing the shared object would silently
//! break the other. [`UnembedPlan`] resolves sharing across the **whole
//! inventory** before planning anything: a program object is freed only
//! when every font that reaches it is in the same operation, and a
//! descriptor shared with a font that is *not* in the operation blocks that
//! font outright ([`UnembedBlocker::DescriptorShared`]) rather than
//! quietly editing an object someone else owns.
//!
//! **Stated limit, in the same shape as
//! `EditSession::appearance_streams_owned_by`'s:** the sharing census is
//! over the font inventory, not over the whole object graph. A `/FontFile2`
//! stream referenced from somewhere that is not a font descriptor — which
//! no producer in the corpus emits — would not be seen. Closing that needs
//! a whole-document reference count, which pdfcer does not have; the bound
//! is stated rather than hidden.
//!
//! **A direct font dictionary cannot be edited.** A resource entry may hold
//! a font dictionary inline rather than by reference. It has no object
//! identity, so the overlay has nothing to write, and it is blocked by name
//! ([`UnembedBlocker::FontNotIndirect`]) rather than skipped.
//!
//! **Incremental save reclaims nothing.** This is the module's most
//! counter-intuitive fact and it is carried in the plan itself
//! ([`UnembedPlan::bytes_reclaimable`]'s docs). An incremental update
//! *appends* a revision; the deleted program's bytes remain in the prior
//! revision and the file gets **larger**. Only a full rewrite drops them.
//! An operator whose entire goal is a smaller file must be told which save
//! mode delivers it.
//!
//! # Spec sources
//!
//! - ISO 32000-1 §9.8.1 Table 122 — descriptor entries; `/FontName` *shall*
//!   equal `/BaseFont`; the substitution-metrics rationale; `/CharSet`
//!   - `D:\Dev\Rag-Specialized\PDF_Spec\iso32000\iso32000__s__9.8.md`
//! - ISO 32000-1 §9.8.3 Table 124 — `/CIDSet` (same file)
//! - ISO 32000-1 §9.9 Table 126/127 — `/FontFile*`; subset indication is a
//!   `may` — `iso32000__s__9.9.md`
//! - ISO 32000-1 §9.6.4 — subset tags: "exactly six uppercase letters"
//!   - `iso32000__s__9.6.md`
//! - ISO 32000-1 §9.6.2.1 Table 111 — `/Widths` and its consistency
//!   requirement (same file)
//! - ISO 32000-1 §7.5.6 — an update section carries changed objects only
//! - ISO 19005 (PDF/A, all parts) — fonts *shall* be embedded; see
//!   [`PdfaClaim`]

use std::collections::{BTreeMap, BTreeSet};

use crate::fontinfo::{FontInventory, FontRecord, ProgramKey, Removability, split_subset_tag};
use crate::graph::ObjectGraph;
use crate::object::{Dict, Name, ObjId, Object};
use crate::view::DocumentView;

/// How many bytes of a `/Metadata` packet are scanned for a PDF/A claim.
///
/// The identifier lives in the packet's header region — `pdfaid:part` is
/// emitted inside the first `rdf:Description` by every producer that emits
/// it at all — so a bounded scan finds it. The bound exists because
/// `/Metadata` is attacker-influenced bytes and `ARCHITECTURE.md` §10.1
/// requires a ceiling on anything that consumes them.
///
/// A packet larger than this is scanned to the limit and, if no claim is
/// found there, reported as [`PdfaClaim::None`] — not as "unreadable",
/// because the scan succeeded; it simply did not find a claim in the region
/// where one is written.
pub const MAX_METADATA_SCAN_BYTES: usize = 512 * 1024;

/// Whether the document identifies itself as PDF/A.
///
/// **A detection, never a validation.** The presence of `pdfaid:part` is a
/// *claim the file makes about itself*; pdfcer does not check conformance
/// and this type must never be rendered as "this file is valid PDF/A".
/// Equally, [`Self::None`] is not proof the file is not PDF/A — although it
/// is strong evidence, because every part of ISO 19005 requires the
/// identification metadata to be present.
///
/// It matters here because **PDF/A requires fonts to be embedded, in every
/// part**. Unembedding therefore breaks conformance, with certainty, and
/// the operator is told before it happens. Whether Acrobat warns about the
/// same thing is an unresolved GAP in the parity research
/// (`optimize__font_unembedding.md`), so pdfcer is not matching a behaviour
/// here — it is choosing one.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PdfaClaim {
    /// No PDF/A identification found in `/Metadata`, and no PDF/A output
    /// intent.
    None,
    /// The XMP packet carries `pdfaid:part`.
    Identified {
        /// The part number as written (`"1"`, `"2"`, `"3"`, `"4"`), when it
        /// could be read as a short token. `None` when the key was found
        /// but its value was not in a shape worth quoting back.
        part: Option<String>,
        /// The conformance level as written (`"A"`, `"B"`, `"U"`), when
        /// present.
        conformance: Option<String>,
    },
    /// No `pdfaid:part`, but the catalog carries an `/OutputIntent` whose
    /// `/S` is `GTS_PDFA1`.
    ///
    /// Its own state rather than folded into [`Self::Identified`], because
    /// an output intent alone is **not** a PDF/A claim — a plain PDF may
    /// legitimately carry one for colour management. It is reported so an
    /// operator who knows their workflow can recognise the file; it is not
    /// treated as identification.
    OutputIntentOnly,
    /// A `/Metadata` stream is present but its bytes could not be reached
    /// or decoded, so the question was not answered.
    ///
    /// Distinct from [`Self::None`] for the same reason
    /// [`crate::fontinfo::Program::Unreadable`] is distinct from
    /// `NotEmbedded`: "we looked and there is none" and "we could not look"
    /// lead to different operator decisions.
    MetadataUnreadable,
}

impl PdfaClaim {
    /// Whether unembedding would break a conformance claim this document
    /// makes.
    ///
    /// `true` only for [`Self::Identified`]. An output intent alone is not
    /// a claim, and an unreadable packet is not evidence of one — both are
    /// reported to the operator and neither is treated as identification.
    ///
    /// # Examples
    ///
    /// ```
    /// use pdfcer_core::font_unembed::PdfaClaim;
    ///
    /// assert!(!PdfaClaim::None.breaks_conformance());
    /// assert!(!PdfaClaim::OutputIntentOnly.breaks_conformance());
    /// assert!(
    ///     PdfaClaim::Identified { part: Some("2".into()), conformance: Some("B".into()) }
    ///         .breaks_conformance()
    /// );
    /// ```
    #[must_use]
    pub const fn breaks_conformance(&self) -> bool {
        matches!(self, Self::Identified { .. })
    }

    /// A stable, locale-invariant token for a machine-readable report.
    #[must_use]
    pub const fn token(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Identified { .. } => "pdfa-identified",
            Self::OutputIntentOnly => "output-intent-only",
            Self::MetadataUnreadable => "metadata-unreadable",
        }
    }
}

/// Detect the document's PDF/A self-identification.
///
/// Reads the catalog's `/Metadata` stream (§14.3.2), decodes its filter
/// chain, and looks for the `pdfaid:part` property in either of the two
/// shapes XMP permits — an attribute (`pdfaid:part="2"`) or an element
/// (`<pdfaid:part>2</pdfaid:part>`). Falls back to `/OutputIntents` when no
/// claim is found.
///
/// # Why a byte scan and not an XMP parser
///
/// pdfcer has no XMP parser and this is not the Pass to acquire one. The
/// property name is a fixed ASCII string that cannot appear by accident in
/// a packet that is not making the claim, and the alternative — treating
/// "we have no parser" as "no claim" — would silently omit the one warning
/// this detection exists to produce. The limitation is real and bounded:
/// a claim written with an unusual namespace prefix (XMP allows any prefix
/// bound to the `pdfaid` namespace URI) would be missed. That is why the
/// operator-facing text says a claim was *found*, never that none exists.
///
/// # Never fails
///
/// Every structural fault degrades into a [`PdfaClaim`] variant. A document
/// with no catalog, no `/Metadata`, or an undecodable packet still yields
/// an answer, because refusing to plan an unembed over a metadata question
/// would cost the operator the operation for a disclosure's sake.
#[must_use]
pub fn detect_pdfa(view: &DocumentView<'_>) -> PdfaClaim {
    let graph = view;
    let catalog = graph
        .trailer_entry(b"Root")
        .map(|o| graph.resolve(o))
        .and_then(Object::as_dict)
        .cloned();
    let Some(catalog) = catalog else {
        return PdfaClaim::None;
    };

    if let Some(entry) = catalog.get(b"Metadata") {
        match graph.resolve(entry) {
            Object::Stream(stream) => {
                let decoded = view
                    .slice(stream.data_span)
                    .and_then(|raw| crate::filters::decode_stream(&stream.dict, raw).ok());
                match decoded {
                    Some(bytes) => {
                        let window = bytes
                            .get(..bytes.len().min(MAX_METADATA_SCAN_BYTES))
                            .unwrap_or(&bytes);
                        if let Some(claim) = pdfa_claim_in(window) {
                            return claim;
                        }
                    }
                    None => return PdfaClaim::MetadataUnreadable,
                }
            }
            // §7.3.10 makes a dangling reference a `null`, not an error.
            // A `/Metadata` that resolves to a non-stream is a malformation
            // and, like a stream that will not decode, means the question
            // was not answered rather than answered "no".
            Object::Null => {}
            _ => return PdfaClaim::MetadataUnreadable,
        }
    }

    if has_pdfa_output_intent(view, &catalog) {
        return PdfaClaim::OutputIntentOnly;
    }
    PdfaClaim::None
}

/// Look for `pdfaid:part` in a decoded XMP packet.
///
/// Returns `None` when the property is absent, so the caller can fall
/// through to the output-intent check rather than committing to "no claim".
fn pdfa_claim_in(packet: &[u8]) -> Option<PdfaClaim> {
    find_bytes(packet, b"pdfaid:part")?;
    Some(PdfaClaim::Identified {
        part: xmp_short_value(packet, b"pdfaid:part"),
        conformance: xmp_short_value(packet, b"pdfaid:conformance"),
    })
}

/// Pull a short scalar value for `key` out of an XMP packet.
///
/// Handles the attribute form (`key="value"`, either quote character) and
/// the element form (`<key>value</key>`). Anything longer than a handful of
/// characters is rejected rather than quoted back: a PDF/A part is one
/// digit and a conformance level is one letter, so a long match means the
/// scan found something that is not the value and reporting it would put
/// arbitrary document bytes into an operator-facing sentence.
fn xmp_short_value(packet: &[u8], key: &[u8]) -> Option<String> {
    /// The longest value worth quoting back. `"1".."4"` and `"A"/"B"/"U"`
    /// are the whole legitimate domain; the slack covers whitespace.
    const MAX_VALUE: usize = 8;

    let start = find_bytes(packet, key)? + key.len();
    let rest = packet.get(start..)?;
    // Attribute form: skip `=` and optional whitespace, then read to the
    // matching quote.
    let mut idx = 0usize;
    while matches!(rest.get(idx), Some(b' ' | b'\t' | b'\r' | b'\n')) {
        idx += 1;
    }
    match rest.get(idx) {
        Some(b'=') => {
            idx += 1;
            while matches!(rest.get(idx), Some(b' ' | b'\t' | b'\r' | b'\n')) {
                idx += 1;
            }
            let quote = *rest.get(idx)?;
            if quote != b'"' && quote != b'\'' {
                return None;
            }
            idx += 1;
            let tail = rest.get(idx..)?;
            let end = tail.iter().position(|b| *b == quote)?;
            short_ascii(tail.get(..end)?, MAX_VALUE)
        }
        Some(b'>') => {
            let tail = rest.get(idx + 1..)?;
            let end = tail.iter().position(|b| *b == b'<')?;
            short_ascii(tail.get(..end)?, MAX_VALUE)
        }
        _ => None,
    }
}

/// Accept a candidate value only if it is short, printable ASCII.
fn short_ascii(bytes: &[u8], max: usize) -> Option<String> {
    let text = std::str::from_utf8(bytes).ok()?.trim();
    if text.is_empty() || text.len() > max || !text.bytes().all(|b| b.is_ascii_graphic()) {
        return None;
    }
    Some(text.to_owned())
}

/// Whether the catalog carries an `/OutputIntent` with `/S /GTS_PDFA1`.
fn has_pdfa_output_intent(graph: &DocumentView<'_>, catalog: &Dict) -> bool {
    let Some(entry) = catalog.get(b"OutputIntents") else {
        return false;
    };
    let Some(items) = graph.resolve(entry).as_array().map(<[Object]>::to_vec) else {
        return false;
    };
    items.iter().any(|item| {
        graph
            .resolve(item)
            .as_dict()
            .and_then(|d| d.get(b"S"))
            .map(|s| graph.resolve(s))
            .and_then(Object::as_name)
            .is_some_and(|n| n.as_bytes() == b"GTS_PDFA1")
    })
}

/// First index of `needle` in `haystack`.
///
/// `slice::windows` rather than a crate: one call site, no dependency, and
/// the packet is bounded by [`MAX_METADATA_SCAN_BYTES`].
fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

// ---------------------------------------------------------------------------
// The request
// ---------------------------------------------------------------------------

/// What to unembed.
///
/// Three shapes rather than one, because the three callers ask genuinely
/// different questions: a GUI has an object identity in hand, a CLI has a
/// name the operator typed, and "everything that can go" is the batch case
/// the whole feature exists for.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum UnembedSelection {
    /// Every font whose verdict is
    /// [`Removability::Removable`].
    AllRemovable,
    /// Fonts named by `/BaseFont` **or** by the de-prefixed family name.
    ///
    /// Both spellings match, because an operator reading a font list sees
    /// the family name (`Arial`) while the file spells it `ABCDEF+Arial`,
    /// and requiring the tag would make the obvious command fail. A name
    /// that matches nothing is reported in [`UnembedPlan::unmatched`] —
    /// never silently ignored, which is how a typo becomes "the tool did
    /// nothing and said it succeeded".
    Named(Vec<String>),
    /// Fonts by font-dictionary object identity. The shape a GUI row has.
    Objects(Vec<ObjId>),
}

/// Whether the §9.6.4 subset tag is stripped from the resulting name.
///
/// See the module docs for the reasoning; the default is [`Self::Strip`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum SubsetTagPolicy {
    /// Remove the six-letter tag and the `+` from `/BaseFont` and
    /// `/FontName` together. The default: the name's job after unembedding
    /// is to match an installed face, and a tagged name matches none.
    #[default]
    Strip,
    /// Leave both names exactly as the file spells them.
    ///
    /// For a caller comparing pdfcer's output against another tool
    /// byte-for-byte, or one that has already established the reader on the
    /// far end strips tags itself.
    Keep,
}

/// One unembed request: what to remove, and how to name what is left.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct UnembedRequest {
    /// Which fonts.
    pub selection: UnembedSelection,
    /// What happens to a subset tag on the fonts that go.
    pub subset_tag: SubsetTagPolicy,
}

impl UnembedRequest {
    /// Every font the report says can go.
    #[must_use]
    pub fn all_removable() -> Self {
        Self {
            selection: UnembedSelection::AllRemovable,
            subset_tag: SubsetTagPolicy::Strip,
        }
    }

    /// Fonts named by `/BaseFont` or family name.
    #[must_use]
    pub fn named<I, S>(names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            selection: UnembedSelection::Named(names.into_iter().map(Into::into).collect()),
            subset_tag: SubsetTagPolicy::Strip,
        }
    }

    /// Fonts by font-dictionary object identity.
    #[must_use]
    pub fn objects<I: IntoIterator<Item = ObjId>>(ids: I) -> Self {
        Self {
            selection: UnembedSelection::Objects(ids.into_iter().collect()),
            subset_tag: SubsetTagPolicy::Strip,
        }
    }

    /// Keep the §9.6.4 subset tag on the resulting names.
    #[must_use]
    pub const fn keeping_subset_tag(mut self) -> Self {
        self.subset_tag = SubsetTagPolicy::Keep;
        self
    }
}

// ---------------------------------------------------------------------------
// The plan
// ---------------------------------------------------------------------------

/// Why one font will not be unembedded.
///
/// Every variant is a statement about **this document**, and every one is
/// reported by name. The whole point of the type is that no font is ever
/// silently absent from a result.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum UnembedBlocker {
    /// The report's verdict is not
    /// [`Removability::Removable`]. Carries the verdict so the reason
    /// sentence comes from the one classifier rather than being restated
    /// here — [`Removability::reason`] is the disclosure.
    NotRemovable(Removability),
    /// The font dictionary is a **direct** object inside a resource
    /// dictionary, so it has no identity for the overlay to write.
    ///
    /// Legal (§7.3.7 permits a direct dictionary anywhere a dictionary is
    /// allowed) and rare. Blocked rather than skipped, because a font that
    /// vanished from both the "removed" and the "refused" lists is exactly
    /// the silence this module exists to prevent.
    FontNotIndirect,
    /// The `/FontDescriptor` object is shared with at least one font that
    /// is **not** part of this operation, so removing the program from it
    /// would unembed that font too.
    DescriptorShared {
        /// The other font dictionaries reaching the same descriptor.
        with: Vec<ObjId>,
    },
    /// The font's `/FontDescriptor` could not be reached as a dictionary,
    /// so there is nothing to edit.
    ///
    /// A font classified `Removable` always had a readable program (the
    /// classifier requires it), so this is a structure that changed between
    /// the inventory and the plan, or a malformation the inventory tolerated.
    DescriptorUnreadable,
    /// The `/FontDescriptor` is a **direct** dictionary somewhere pdfcer
    /// cannot address — inside a composite font's descendant rather than
    /// inside the font dictionary itself.
    ///
    /// Legal, and unreachable in practice today: a composite font is never
    /// classified `Removable`. Blocked by name rather than by a silently
    /// wrong write, because the alternative is to re-emit a chain of
    /// containing objects the operator did not touch (rule 3).
    DescriptorNotAddressable,
}

impl UnembedBlocker {
    /// A stable, locale-invariant token for a machine-readable report.
    #[must_use]
    pub fn token(&self) -> &'static str {
        match self {
            Self::NotRemovable(verdict) => verdict.token(),
            Self::FontNotIndirect => "font-not-indirect",
            Self::DescriptorShared { .. } => "descriptor-shared",
            Self::DescriptorUnreadable => "descriptor-unreadable",
            Self::DescriptorNotAddressable => "descriptor-not-addressable",
        }
    }

    /// The sentence an operator reads.
    ///
    /// For [`Self::NotRemovable`] this delegates to
    /// [`Removability::reason`] — the same words the Fonts panel and
    /// `list-fonts` already show, because a font that refused in the report
    /// and refuses here must refuse for the *same stated reason*.
    #[must_use]
    pub const fn reason(&self) -> &'static str {
        match self {
            Self::NotRemovable(verdict) => verdict.reason(),
            Self::FontNotIndirect => {
                "This font dictionary is written directly into a page's resources rather than as \
                 its own numbered object, so there is no object to rewrite without re-emitting \
                 the page."
            }
            Self::DescriptorShared { .. } => {
                "This font shares its descriptor with another font that is not being changed. \
                 Removing the program here would remove it from that font as well."
            }
            Self::DescriptorUnreadable => {
                "This font's descriptor could not be read as a dictionary, so the entry naming \
                 the embedded program cannot be removed."
            }
            Self::DescriptorNotAddressable => {
                "This font's descriptor is written inline inside another object rather than as \
                 its own numbered object, so it cannot be rewritten without re-emitting objects \
                 the operation did not touch."
            }
        }
    }
}

/// A font that will not be unembedded, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct UnembedBlocked {
    /// The font dictionary's identity, or `None` for a direct dictionary.
    pub id: Option<ObjId>,
    /// `/BaseFont` exactly as the file spells it.
    pub base_font: Option<String>,
    /// The bytes its embedded program occupies, or `0` when it has none.
    /// Reported so a summary can say what refusing *cost*.
    pub stored_bytes: usize,
    /// Why.
    pub blocker: UnembedBlocker,
}

/// A font that will be unembedded, and everything that changes about it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct UnembedTarget {
    /// The font dictionary's identity.
    pub id: ObjId,
    /// `/BaseFont` exactly as the file spells it, before any rename.
    pub base_font: Option<String>,
    /// The descriptor object, or `None` when the descriptor is a direct
    /// dictionary inside the font dictionary (in which case the font
    /// dictionary itself carries the edit).
    pub descriptor_id: Option<ObjId>,
    /// Which key carries the program.
    pub program_key: ProgramKey,
    /// The program stream's object, when it is reached by reference.
    ///
    /// `None` for a program written as a direct stream — not legal
    /// (§7.3.8: a stream *shall* be an indirect object) but tolerated by
    /// the reader, in which case removing the key removes the bytes with it.
    pub program_id: Option<ObjId>,
    /// The bytes the program occupies in this file, from the inventory's
    /// `data_span` measurement.
    pub stored_bytes: usize,
    /// Whether the program object is freed by this operation.
    ///
    /// `false` when another font that is **not** in this operation also
    /// reaches the same stream — the key still comes out of this
    /// descriptor, but the bytes stay, and
    /// [`UnembedPlan::bytes_reclaimable`] does not count them.
    pub program_freed: bool,
    /// Other font dictionaries reaching the same program stream and
    /// remaining embedded. Non-empty exactly when `program_freed` is false.
    pub program_shared_with: Vec<ObjId>,
    /// The name the font will carry afterwards, when the subset tag is
    /// being stripped. `None` when there is no tag, when the policy is
    /// [`SubsetTagPolicy::Keep`], or when stripping would leave an empty
    /// name.
    pub rename: Option<String>,
    /// Whether a `/CIDSet` entry is removed with the program.
    pub cid_set_removed: bool,
    /// The `/CIDSet` stream's object, when it is freed. Its bytes are
    /// counted in [`UnembedPlan::bytes_reclaimable`].
    pub cid_set_id: Option<ObjId>,
    /// Whether a `/CharSet` entry is removed with the program.
    pub char_set_removed: bool,
    /// 1-based page numbers the font is reachable from, from the inventory.
    pub pages: Vec<u32>,
}

impl UnembedTarget {
    /// The bytes this target actually reclaims on a **full rewrite**.
    ///
    /// Zero when the program is shared and therefore not freed. `/CIDSet`
    /// bytes are not included: they are counted by the plan, which has the
    /// stream sizes, whereas this type carries only the program's.
    #[must_use]
    pub const fn reclaimed_bytes(&self) -> usize {
        if self.program_freed {
            self.stored_bytes
        } else {
            0
        }
    }
}

/// Everything unembedding would do, computed before anything changes.
///
/// Both a preview and the record of what happened: the same value is
/// returned by the preview query and by the committing call, so a front end
/// cannot show one thing and do another.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct UnembedPlan {
    /// The fonts that will lose their programs.
    pub targets: Vec<UnembedTarget>,
    /// The fonts that will not, each with its reason.
    ///
    /// For [`UnembedSelection::AllRemovable`] this is **every** other font
    /// in the document, including the ones that are simply not embedded —
    /// a shorter list is not actionable, which is the divergence from
    /// Acrobat this whole feature is built around.
    pub blocked: Vec<UnembedBlocked>,
    /// Names from [`UnembedSelection::Named`] that matched no font.
    pub unmatched: Vec<String>,
    /// Whether the document identifies itself as PDF/A.
    pub pdfa: PdfaClaim,
    /// Which font-bearing surfaces the inventory searched, and which it did
    /// not — carried through unchanged from [`crate::fontinfo::inventory`]
    /// so a plan states the shape of its own evidence.
    pub coverage: crate::fontinfo::SurfaceCoverage,
}

impl UnembedPlan {
    /// Whether this plan would change anything.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.targets.is_empty()
    }

    /// The bytes a **full rewrite** would drop from the file.
    ///
    /// Every freed program plus every freed `/CIDSet` stream. A shared
    /// program that stays behind contributes nothing.
    ///
    /// # ★ An incremental save reclaims NONE of this
    ///
    /// §7.5.6's update section is *appended*: the deleted objects get free
    /// cross-reference entries in the new section, and their bytes remain
    /// in the prior revision, which is still in the file. An incremental
    /// save after an unembed therefore produces a **larger** file. Only
    /// [`EditSession::to_full_bytes`](crate::edit::EditSession::to_full_bytes)
    /// drops the bytes.
    ///
    /// This number is what the operator is trying to recover, so it is
    /// reported — but it must never be reported without the save mode that
    /// delivers it.
    #[must_use]
    pub fn bytes_reclaimable(&self) -> u64 {
        self.targets
            .iter()
            .map(|t| t.reclaimed_bytes() as u64)
            .sum()
    }

    /// How many blocked fonts carry each reason, keyed by
    /// [`UnembedBlocker::token`].
    #[must_use]
    pub fn blocker_counts(&self) -> BTreeMap<&'static str, usize> {
        let mut out = BTreeMap::new();
        for b in &self.blocked {
            *out.entry(b.blocker.token()).or_insert(0) += 1;
        }
        out
    }

    /// Whether any target's `/BaseFont` is being renamed.
    #[must_use]
    pub fn renames_any(&self) -> bool {
        self.targets.iter().any(|t| t.rename.is_some())
    }
}

// ---------------------------------------------------------------------------
// Planning
// ---------------------------------------------------------------------------

/// Build the plan for `request` over `inventory`, resolving object
/// identities against `graph`.
///
/// Pure: reads the graph, allocates a plan, changes nothing. The committing
/// side ([`EditSession::unembed_fonts`](crate::edit::EditSession::unembed_fonts))
/// calls exactly this function and then executes what it returns, so the
/// preview and the commit cannot diverge.
///
/// # The order of the work, which is the argument
///
/// 1. **Resolve every font's descriptor and program object** — for all
///    fonts, not only the selected ones, because sharing can only be
///    detected across the whole set.
/// 2. **Select** by name / identity / verdict.
/// 3. **Block** everything not selected-and-removable, by name and reason.
/// 4. **Decide freeing** — a program object is freed only when every font
///    reaching it is a target.
///
/// Steps 1 and 4 are why this is not simply "for each selected font, delete
/// its `/FontFile`": two fonts sharing one program is legal, and the naive
/// loop would free a stream another font still needs.
///
/// # Never fails
///
/// Every fault is data in the plan. A malformed descriptor becomes an
/// [`UnembedBlocker`], not an error, for the same reason
/// [`crate::fontinfo::inventory`] is infallible: refusing the whole
/// operation over one damaged font would cost the operator every undamaged
/// one.
#[must_use]
pub fn plan(
    view: &DocumentView<'_>,
    inventory: &FontInventory,
    request: &UnembedRequest,
) -> UnembedPlan {
    // ---- 1. locate ------------------------------------------------------
    let located: Vec<Located> = inventory
        .fonts
        .iter()
        .map(|record| locate(view, record))
        .collect();

    // How many fonts reach each descriptor / program object. Counted over
    // the WHOLE inventory, which is the only scope in which sharing is
    // visible.
    let mut descriptor_users: BTreeMap<ObjId, Vec<ObjId>> = BTreeMap::new();
    let mut program_users: BTreeMap<ObjId, Vec<ObjId>> = BTreeMap::new();
    for l in &located {
        let Some(font_id) = l.record.id else { continue };
        if let Some(d) = l.descriptor_id {
            descriptor_users.entry(d).or_default().push(font_id);
        }
        if let Some(p) = l.program_id {
            program_users.entry(p).or_default().push(font_id);
        }
    }

    // ---- 2. select ------------------------------------------------------
    let mut unmatched: Vec<String> = Vec::new();
    let selected: BTreeSet<usize> = match &request.selection {
        UnembedSelection::AllRemovable => located
            .iter()
            .enumerate()
            .filter(|(_, l)| l.record.removability.is_removable())
            .map(|(i, _)| i)
            .collect(),
        UnembedSelection::Objects(ids) => {
            let wanted: BTreeSet<ObjId> = ids.iter().copied().collect();
            let hit: BTreeSet<usize> = located
                .iter()
                .enumerate()
                .filter(|(_, l)| l.record.id.is_some_and(|id| wanted.contains(&id)))
                .map(|(i, _)| i)
                .collect();
            // An object id that named no font is reported the same way an
            // unmatched name is: a GUI whose row went stale must not be
            // told the operation succeeded.
            let found: BTreeSet<ObjId> = hit
                .iter()
                .filter_map(|i| located.get(*i).and_then(|l| l.record.id))
                .collect();
            for id in &wanted {
                if !found.contains(id) {
                    unmatched.push(format!("{id}"));
                }
            }
            hit
        }
        UnembedSelection::Named(names) => {
            let mut hit = BTreeSet::new();
            for name in names {
                let mut any = false;
                for (i, l) in located.iter().enumerate() {
                    if matches_name(l.record, name) {
                        hit.insert(i);
                        any = true;
                    }
                }
                if !any {
                    unmatched.push(name.clone());
                }
            }
            hit
        }
    };

    // ---- 3. split into targets and blocked ------------------------------
    let mut targets: Vec<UnembedTarget> = Vec::new();
    let mut blocked: Vec<UnembedBlocked> = Vec::new();
    // Font ids that will be unembedded, needed by step 4 and computed here
    // so the freeing decision sees the final target set.
    let mut target_ids: BTreeSet<ObjId> = BTreeSet::new();

    for (i, l) in located.iter().enumerate() {
        let record = l.record;
        if !selected.contains(&i) {
            // Not asked for. Only reported as blocked when the caller asked
            // for "everything removable" — under an explicit selection a
            // font the operator did not name is not a refusal, and listing
            // it as one would bury the fonts that ARE refusals.
            if matches!(request.selection, UnembedSelection::AllRemovable) {
                blocked.push(UnembedBlocked {
                    id: record.id,
                    base_font: record.base_font.clone(),
                    stored_bytes: record.stored_bytes(),
                    blocker: UnembedBlocker::NotRemovable(record.removability.clone()),
                });
            }
            continue;
        }

        if !record.removability.is_removable() {
            blocked.push(UnembedBlocked {
                id: record.id,
                base_font: record.base_font.clone(),
                stored_bytes: record.stored_bytes(),
                blocker: UnembedBlocker::NotRemovable(record.removability.clone()),
            });
            continue;
        }
        let Some(font_id) = record.id else {
            blocked.push(UnembedBlocked {
                id: None,
                base_font: record.base_font.clone(),
                stored_bytes: record.stored_bytes(),
                blocker: UnembedBlocker::FontNotIndirect,
            });
            continue;
        };
        let Some(program_key) = l.program_key else {
            blocked.push(UnembedBlocked {
                id: record.id,
                base_font: record.base_font.clone(),
                stored_bytes: record.stored_bytes(),
                blocker: UnembedBlocker::DescriptorUnreadable,
            });
            continue;
        };
        if l.descriptor_id.is_none() && !l.descriptor_on_font_dict {
            blocked.push(UnembedBlocked {
                id: record.id,
                base_font: record.base_font.clone(),
                stored_bytes: record.stored_bytes(),
                blocker: UnembedBlocker::DescriptorNotAddressable,
            });
            continue;
        }
        target_ids.insert(font_id);
        targets.push(UnembedTarget {
            id: font_id,
            base_font: record.base_font.clone(),
            descriptor_id: l.descriptor_id,
            program_key,
            program_id: l.program_id,
            stored_bytes: record.stored_bytes(),
            // Provisional; step 4 settles both.
            program_freed: true,
            program_shared_with: Vec::new(),
            rename: rename_for(record, request.subset_tag),
            cid_set_removed: l.cid_set,
            cid_set_id: l.cid_set_id,
            char_set_removed: l.char_set,
            pages: record.pages.clone(),
        });
    }

    // ---- 4. sharing ------------------------------------------------------
    // A descriptor shared with a font NOT in the operation blocks that
    // target outright: editing the descriptor would unembed the other font
    // as a side effect, which is the definition of sneaky.
    let mut i = 0usize;
    while i < targets.len() {
        let Some(target) = targets.get(i) else { break };
        let outsiders: Vec<ObjId> = target
            .descriptor_id
            .and_then(|d| descriptor_users.get(&d))
            .map(|users| {
                users
                    .iter()
                    .copied()
                    .filter(|u| *u != target.id && !target_ids.contains(u))
                    .collect()
            })
            .unwrap_or_default();
        if outsiders.is_empty() {
            i += 1;
            continue;
        }
        let t = targets.remove(i);
        target_ids.remove(&t.id);
        blocked.push(UnembedBlocked {
            id: Some(t.id),
            base_font: t.base_font,
            stored_bytes: t.stored_bytes,
            blocker: UnembedBlocker::DescriptorShared { with: outsiders },
        });
    }

    // A shared PROGRAM is not a blocker — the key still comes out of this
    // descriptor and this font really is unembedded. Only the bytes stay,
    // and the plan says whose they are.
    for t in &mut targets {
        let Some(program_id) = t.program_id else {
            // A direct stream has no object to free; its bytes leave with
            // the descriptor entry, so a full rewrite does recover them.
            continue;
        };
        let holders: Vec<ObjId> = program_users
            .get(&program_id)
            .map(|users| {
                users
                    .iter()
                    .copied()
                    .filter(|u| *u != t.id && !target_ids.contains(u))
                    .collect()
            })
            .unwrap_or_default();
        if !holders.is_empty() {
            t.program_freed = false;
            t.program_shared_with = holders;
        }
    }

    // Stable output order: by object number, so two runs over one document
    // produce identical reports and a diff of two reports is meaningful.
    targets.sort_by_key(|t| (t.id.num, t.id.generation));
    blocked.sort_by_key(|b| b.id.map_or((u32::MAX, 0), |id| (id.num, id.generation)));

    UnembedPlan {
        targets,
        blocked,
        unmatched,
        pdfa: detect_pdfa(view),
        coverage: inventory.coverage,
    }
}

/// One font's resolved object identities.
struct Located<'a> {
    record: &'a FontRecord,
    /// The `/FontDescriptor` object, when it is reached by reference.
    descriptor_id: Option<ObjId>,
    /// Which `/FontFile*` key carries the program, when one is present and
    /// the descriptor could be read.
    program_key: Option<ProgramKey>,
    /// The program stream's object, when reached by reference.
    program_id: Option<ObjId>,
    /// Whether the descriptor carries `/CIDSet`, and its object if it is a
    /// reference.
    cid_set: bool,
    cid_set_id: Option<ObjId>,
    /// Whether the descriptor carries `/CharSet`.
    char_set: bool,
    /// True when the descriptor is a **direct** dictionary sitting in the
    /// font dictionary itself, so writing the font dictionary writes it.
    /// Meaningful only when `descriptor_id` is `None`.
    descriptor_on_font_dict: bool,
}

/// Resolve one font record's descriptor and program object identities.
///
/// **Locates, never classifies.** The verdict on the record was decided by
/// [`crate::fontinfo`] and is used as given; this function only answers
/// *which objects would have to be written*. Keeping the two apart is what
/// makes "the report and the action cannot disagree" true rather than
/// aspirational.
///
/// The descriptor is looked for on the descendant CIDFont for a composite
/// font (§9.8.1: a descriptor *shall not* be used with a Type 0 font) even
/// though no composite font is currently ever classified `Removable` — the
/// lookup is correct rather than convenient, so a future classifier change
/// does not silently start editing the wrong dictionary.
fn locate<'a>(graph: &DocumentView<'_>, record: &'a FontRecord) -> Located<'a> {
    let mut out = Located {
        record,
        descriptor_id: None,
        program_key: None,
        program_id: None,
        cid_set: false,
        cid_set_id: None,
        char_set: false,
        descriptor_on_font_dict: false,
    };
    let Some(font_id) = record.id else {
        return out;
    };
    let Some(font_dict) = graph.value(font_id).and_then(Object::as_dict).cloned() else {
        return out;
    };

    // §9.8.1 — a composite font's descriptor hangs off its descendant.
    let glyph_source = if record.subtype.is_composite() {
        descendant_dict(graph, &font_dict)
    } else {
        Some(font_dict.clone())
    };
    let Some(glyph_source) = glyph_source else {
        return out;
    };
    let Some(entry) = glyph_source.get(b"FontDescriptor") else {
        return out;
    };
    out.descriptor_id = entry.as_reference();
    out.descriptor_on_font_dict = out.descriptor_id.is_none() && !record.subtype.is_composite();
    let Some(descriptor) = graph.resolve(entry).as_dict().cloned() else {
        return out;
    };

    // §9.9 Table 126 order, matching `fontinfo::model_program` exactly so
    // the key the report measured is the key the plan removes.
    for (bytes, key) in [
        (b"FontFile".as_slice(), ProgramKey::FontFile),
        (b"FontFile2".as_slice(), ProgramKey::FontFile2),
        (b"FontFile3".as_slice(), ProgramKey::FontFile3),
    ] {
        let Some(program) = descriptor.get(bytes) else {
            continue;
        };
        out.program_key = Some(key);
        out.program_id = program.as_reference();
        break;
    }

    if let Some(cid_set) = descriptor.get(b"CIDSet") {
        out.cid_set = true;
        out.cid_set_id = cid_set.as_reference();
    }
    out.char_set = descriptor.contains_key(b"CharSet");
    out
}

/// The descendant CIDFont dictionary of a `Type0` font (§9.7.6 Table 121).
///
/// Accepts both the conforming one-element array and the bare dictionary
/// some producers write, for the same reason
/// [`crate::fontinfo`]'s equivalent does: refusing the unwrapped form would
/// misreport a font over a wrapper that carries no information.
fn descendant_dict(graph: &DocumentView<'_>, font_dict: &Dict) -> Option<Dict> {
    let entry = font_dict.get(b"DescendantFonts")?;
    match graph.resolve(entry) {
        Object::Array(items) => graph.resolve(items.first()?).as_dict().cloned(),
        Object::Dict(d) => Some(d.clone()),
        _ => None,
    }
}

/// Whether `name` selects `record`.
///
/// Matches `/BaseFont` verbatim or the de-prefixed family name, so both
/// `ABCDEF+Arial` and `Arial` work. Comparison is exact and
/// case-**sensitive**: a PostScript font name is case-significant, and
/// case-folding `Arial` onto `ARIAL` would be pdfcer deciding two different
/// names are one.
fn matches_name(record: &FontRecord, name: &str) -> bool {
    let Some(base) = record.base_font.as_deref() else {
        return false;
    };
    base == name || split_subset_tag(base).1 == name
}

/// The post-unembed `/BaseFont` value, or `None` when the name does not
/// change.
///
/// See the module docs for why stripping is the default.
///
/// # The empty-family guard, and why it is unreachable
///
/// It declines when the family part is empty — and **it cannot fire**.
/// [`split_subset_tag`] requires `len() > 7` before it will call anything a
/// tag, so `ABCDEF+` (exactly seven bytes) carries no tag at all and the
/// whole string stays the name; the shortest string it *will* split is
/// `ABCDEF+X`, leaving one character. The guard was written for a case the
/// splitter forbids, and that was found by building a fixture to reach it
/// and watching it classify the other way.
///
/// It stays, in one line, for two reasons rather than out of caution. An
/// empty `/BaseFont` is the single output worse than a tagged one — a name
/// that matches nothing *and* says nothing — so this is the one postcondition
/// worth asserting in code. And the invariant it depends on lives in another
/// function, which is precisely the kind of coupling that a later relaxation
/// of §9.6.4 strictness would break silently.
fn rename_for(record: &FontRecord, policy: SubsetTagPolicy) -> Option<String> {
    if policy != SubsetTagPolicy::Strip {
        return None;
    }
    let base = record.base_font.as_deref()?;
    let (tag, family) = split_subset_tag(base);
    tag?;
    if family.is_empty() {
        return None;
    }
    Some(family.to_owned())
}

/// Apply one target's edits to a descriptor dictionary.
///
/// Separated from the session so the exact set of keys that move is written
/// once and is testable without a document: the program key, `/CIDSet`,
/// `/CharSet`, and — when the descriptor is the one carrying it —
/// `/FontName`.
///
/// Returns `true` when anything changed. A descriptor that changed nothing
/// must not be written, because §7.5.6 restricts an update section to
/// objects that *"have been changed, replaced, or deleted"* and a no-op
/// write would put an unchanged object in it.
pub(crate) fn strip_descriptor(descriptor: &mut Dict, target: &UnembedTarget) -> bool {
    let mut changed = false;
    if descriptor
        .remove(target.program_key.label().as_bytes())
        .is_some()
    {
        changed = true;
    }
    if descriptor.remove(b"CIDSet").is_some() {
        changed = true;
    }
    if descriptor.remove(b"CharSet").is_some() {
        changed = true;
    }
    // Table 122: `/FontName` **shall** equal `/BaseFont`. The two move
    // together or the cleanup introduces a conformance defect.
    if let Some(new_name) = &target.rename
        && descriptor.contains_key(b"FontName")
    {
        descriptor.insert(
            Name::from(b"FontName"),
            Object::Name(Name(new_name.as_bytes().to_vec())),
        );
        changed = true;
    }
    changed
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

    #[test]
    fn pdfa_claim_reads_attribute_form() {
        let packet = br#"<rdf:Description pdfaid:part="2" pdfaid:conformance="B"/>"#;
        assert_eq!(
            pdfa_claim_in(packet),
            Some(PdfaClaim::Identified {
                part: Some("2".to_owned()),
                conformance: Some("B".to_owned()),
            })
        );
    }

    #[test]
    fn pdfa_claim_reads_element_form() {
        let packet = b"<pdfaid:part>1</pdfaid:part><pdfaid:conformance>A</pdfaid:conformance>";
        assert_eq!(
            pdfa_claim_in(packet),
            Some(PdfaClaim::Identified {
                part: Some("1".to_owned()),
                conformance: Some("A".to_owned()),
            })
        );
    }

    #[test]
    fn pdfa_claim_absent_is_none() {
        assert_eq!(
            pdfa_claim_in(b"<x:xmpmeta><dc:title>hi</dc:title></x:xmpmeta>"),
            None
        );
    }

    #[test]
    fn a_long_value_is_not_quoted_back() {
        // Guards against putting arbitrary document bytes into an
        // operator-facing sentence.
        let packet = br#"<rdf:Description pdfaid:part="this is not a part number"/>"#;
        assert_eq!(
            pdfa_claim_in(packet),
            Some(PdfaClaim::Identified {
                part: None,
                conformance: None
            })
        );
    }

    #[test]
    fn only_identification_breaks_conformance() {
        assert!(!PdfaClaim::None.breaks_conformance());
        assert!(!PdfaClaim::OutputIntentOnly.breaks_conformance());
        assert!(!PdfaClaim::MetadataUnreadable.breaks_conformance());
        assert!(
            PdfaClaim::Identified {
                part: None,
                conformance: None
            }
            .breaks_conformance()
        );
    }

    #[test]
    fn requests_default_to_stripping_the_tag() {
        assert_eq!(
            UnembedRequest::all_removable().subset_tag,
            SubsetTagPolicy::Strip
        );
        assert_eq!(
            UnembedRequest::all_removable()
                .keeping_subset_tag()
                .subset_tag,
            SubsetTagPolicy::Keep
        );
    }

    // -- planning, over real fixtures -------------------------------------

    use crate::document::Document;
    use crate::edit::EditSession;
    use crate::fontinfo::RemovabilityUnknown;

    fn session(bytes: &[u8]) -> EditSession {
        EditSession::new(Document::from_bytes(bytes.to_vec()).expect("fixture parses"))
    }

    /// A plain removable subset: one target, the tag stripped, the program
    /// freed. The baseline every other test is a deviation from.
    #[test]
    fn a_removable_subset_is_a_target_and_is_renamed() {
        let s = session(include_bytes!(
            "../../../fixtures/synthetic/text/subset-simple-embedded.pdf"
        ));
        let plan = s.unembed_preview(&UnembedRequest::all_removable());
        assert_eq!(plan.targets.len(), 1);
        let t = &plan.targets[0];
        assert_eq!(t.base_font.as_deref(), Some("SUBSET+pdfceSubsetDemo"));
        assert_eq!(t.rename.as_deref(), Some("pdfceSubsetDemo"));
        assert_eq!(t.program_key, ProgramKey::FontFile2);
        assert!(t.program_freed);
        assert!(t.program_shared_with.is_empty());
        assert!(plan.bytes_reclaimable() > 0);
        assert_eq!(plan.pdfa, PdfaClaim::None);
    }

    /// The core half of `--keep-subset-tag`: the same plan with the name
    /// left exactly as the file spells it.
    #[test]
    fn keeping_the_subset_tag_leaves_the_name_alone() {
        let s = session(include_bytes!(
            "../../../fixtures/synthetic/text/subset-simple-embedded.pdf"
        ));
        let plan = s.unembed_preview(&UnembedRequest::all_removable().keeping_subset_tag());
        assert_eq!(plan.targets.len(), 1);
        assert_eq!(plan.targets[0].rename, None);
        assert!(!plan.renames_any());
    }

    /// The whole reason this Pass diverges from Acrobat. A blocked font is
    /// NAMED, with the same reason sentence the report shows — never merely
    /// absent from the list.
    #[test]
    fn a_blocked_font_is_named_with_its_reason() {
        let s = session(include_bytes!(
            "../../../fixtures/synthetic/text/cidfonttype2-nocmap-embedded.pdf"
        ));
        let plan = s.unembed_preview(&UnembedRequest::all_removable());
        assert!(plan.targets.is_empty());
        let identity = plan
            .blocked
            .iter()
            .find(|b| b.blocker.token() == "blocked-identity")
            .expect("the Identity-H font is refused BY NAME");
        assert!(identity.base_font.is_some());
        assert_eq!(
            identity.blocker.reason(),
            Removability::BlockedIdentityEncoded { to_unicode: false }.reason(),
            "the refusal must quote the classifier, not restate it"
        );
    }

    /// Every one of phase A's nine verdict tokens gates correctly: exactly
    /// one proceeds, and the rest each carry a distinct reason sentence.
    ///
    /// A table rather than nine fixtures because the gate under test is
    /// `Removability::is_removable`, a pure function of the verdict — a
    /// document per verdict would be testing the classifier again, which
    /// `fontinfo`'s own tests already do.
    #[test]
    fn exactly_one_of_the_nine_verdicts_proceeds() {
        let all = [
            Removability::NotEmbedded,
            Removability::Removable,
            Removability::BlockedIdentityEncoded { to_unicode: true },
            Removability::BlockedIdentityEncoded { to_unicode: false },
            Removability::BlockedType3,
            Removability::Unknown(RemovabilityUnknown::SymbolicBuiltinEncoding),
            Removability::Unknown(RemovabilityUnknown::PredefinedCMap),
            Removability::Unknown(RemovabilityUnknown::EmbeddedCMap),
            Removability::Unknown(RemovabilityUnknown::ProgramUnreadable),
            Removability::Unknown(RemovabilityUnknown::NoDescendant),
            Removability::Unknown(RemovabilityUnknown::UnknownSubtype),
        ];
        let proceeds: Vec<_> = all.iter().filter(|v| v.is_removable()).collect();
        assert_eq!(proceeds.len(), 1);
        assert_eq!(*proceeds[0], Removability::Removable);
        for v in &all {
            if v.is_removable() {
                continue;
            }
            let blocker = UnembedBlocker::NotRemovable(v.clone());
            assert_eq!(blocker.token(), v.token());
            assert_eq!(blocker.reason(), v.reason());
            assert!(!blocker.reason().is_empty());
        }
    }

    /// Two descriptors, one program stream. The removable font unembeds and
    /// the stream SURVIVES, because a blocked font still reaches it —
    /// freeing it would blank the font pdfcer refused to touch.
    #[test]
    fn a_shared_program_is_not_freed() {
        let s = session(include_bytes!(
            "../../../fixtures/synthetic/unembed/unembed-shared-program.pdf"
        ));
        let plan = s.unembed_preview(&UnembedRequest::all_removable());
        assert_eq!(plan.targets.len(), 1);
        let t = &plan.targets[0];
        assert_eq!(t.base_font.as_deref(), Some("AAAAAA+pdfceShared"));
        assert!(!t.program_freed, "the other font still needs the program");
        assert_eq!(t.program_shared_with.len(), 1);
        // The font IS unembedded; only the bytes stay. Claiming a saving
        // here would be claiming bytes that do not go away.
        assert_eq!(plan.bytes_reclaimable(), 0);
    }

    /// Two font dictionaries, one descriptor. The removable font is blocked
    /// outright, because editing that descriptor would unembed a font whose
    /// verdict said no.
    #[test]
    fn a_shared_descriptor_blocks_the_removable_font() {
        let s = session(include_bytes!(
            "../../../fixtures/synthetic/unembed/unembed-shared-descriptor.pdf"
        ));
        let plan = s.unembed_preview(&UnembedRequest::all_removable());
        assert!(plan.targets.is_empty());
        let shared = plan
            .blocked
            .iter()
            .find(|b| b.blocker.token() == "descriptor-shared")
            .expect("the removable font is blocked by its shared descriptor");
        assert!(matches!(
            shared.blocker,
            UnembedBlocker::DescriptorShared { ref with } if with.len() == 1
        ));
    }

    /// One font object on five pages is ONE target listing five pages — not
    /// five targets, and not five times the byte saving.
    #[test]
    fn a_font_on_many_pages_is_one_target() {
        let s = session(include_bytes!(
            "../../../fixtures/synthetic/unembed/unembed-many-pages.pdf"
        ));
        let plan = s.unembed_preview(&UnembedRequest::all_removable());
        assert_eq!(plan.targets.len(), 1);
        assert_eq!(plan.targets[0].pages, vec![1, 2, 3, 4, 5]);
        assert_eq!(
            plan.bytes_reclaimable(),
            plan.targets[0].stored_bytes as u64
        );
    }

    /// A font reachable only from the AcroForm `/DR` has no page at all, and
    /// must still be reachable by the operation.
    #[test]
    fn a_form_default_resource_font_unembeds() {
        let mut s = session(include_bytes!(
            "../../../fixtures/synthetic/unembed/unembed-acroform-dr.pdf"
        ));
        let plan = s.unembed_preview(&UnembedRequest::all_removable());
        assert_eq!(plan.targets.len(), 1);
        assert!(plan.targets[0].pages.is_empty(), "no page names it");
        let done = s
            .unembed_fonts(&UnembedRequest::all_removable())
            .expect("unembeds");
        assert_eq!(done.targets.len(), 1);
        assert!(s.is_modified());
    }

    /// `/CharSet` and `/CIDSet` describe the program's glyph coverage, so
    /// both go with it — and the `/CIDSet` STREAM is freed. Everything
    /// §9.8.1 names as substitution input stays.
    #[test]
    fn charset_and_cidset_go_with_the_program() {
        let mut s = session(include_bytes!(
            "../../../fixtures/synthetic/unembed/unembed-charset-cidset.pdf"
        ));
        let plan = s
            .unembed_fonts(&UnembedRequest::all_removable())
            .expect("unembeds");
        let t = &plan.targets[0];
        assert!(t.char_set_removed);
        assert!(t.cid_set_removed);
        assert!(t.cid_set_id.is_some());
        let descriptor_id = t.descriptor_id.expect("indirect descriptor");
        let d = s
            .value(descriptor_id)
            .and_then(Object::as_dict)
            .expect("descriptor is a dict");
        assert!(!d.contains_key(b"FontFile2"));
        assert!(!d.contains_key(b"CharSet"));
        assert!(!d.contains_key(b"CIDSet"));
        for kept in [
            b"Flags".as_slice(),
            b"FontBBox",
            b"ItalicAngle",
            b"Ascent",
            b"Descent",
            b"CapHeight",
            b"StemV",
        ] {
            assert!(d.contains_key(kept), "the descriptor keeps its metrics");
        }
        // Table 122: /FontName tracks /BaseFont, and both lose the tag.
        assert_eq!(
            d.get(b"FontName")
                .and_then(Object::as_name)
                .map(Name::as_bytes),
            Some(b"pdfceCoverage".as_slice())
        );
        let font = s
            .value(t.id)
            .and_then(Object::as_dict)
            .expect("font dict is a dict");
        assert_eq!(
            font.get(b"BaseFont")
                .and_then(Object::as_name)
                .map(Name::as_bytes),
            Some(b"pdfceCoverage".as_slice())
        );
        // The reader positions text from /Widths, so they must not move.
        assert!(font.contains_key(b"Widths"));
        assert!(font.contains_key(b"FirstChar"));
        assert!(font.contains_key(b"LastChar"));
    }

    /// A direct font dictionary has no identity for the overlay to write, so
    /// it is blocked BY NAME rather than silently missing from both lists.
    #[test]
    fn a_direct_font_dictionary_is_blocked_by_name() {
        let s = session(include_bytes!(
            "../../../fixtures/synthetic/unembed/unembed-direct-fontdict.pdf"
        ));
        let plan = s.unembed_preview(&UnembedRequest::all_removable());
        assert!(plan.targets.is_empty());
        assert!(
            plan.blocked
                .iter()
                .any(|b| b.blocker == UnembedBlocker::FontNotIndirect)
        );
    }

    /// A direct DESCRIPTOR inside an indirect font dictionary is addressable
    /// — writing the font dictionary writes it — so it unembeds. The pair
    /// with the test above proves the two "direct" cases are not confused.
    #[test]
    fn an_inline_descriptor_unembeds_through_the_font_dictionary() {
        let mut s = session(include_bytes!(
            "../../../fixtures/synthetic/unembed/unembed-inline-descriptor.pdf"
        ));
        let plan = s
            .unembed_fonts(&UnembedRequest::all_removable())
            .expect("unembeds");
        assert_eq!(plan.targets.len(), 1);
        assert_eq!(plan.targets[0].descriptor_id, None);
        let font = s
            .value(plan.targets[0].id)
            .and_then(Object::as_dict)
            .expect("font dict");
        let inline = font
            .get(b"FontDescriptor")
            .and_then(Object::as_dict)
            .expect("descriptor stayed inline");
        assert!(!inline.contains_key(b"FontFile2"));
    }

    /// PDF/A conformance is detected and reported BEFORE anything happens —
    /// and disclosed rather than refused. The core reports; the shells gate.
    #[test]
    fn a_pdfa_document_is_disclosed_not_refused() {
        let mut s = session(include_bytes!(
            "../../../fixtures/synthetic/unembed/unembed-pdfa.pdf"
        ));
        let plan = s.unembed_preview(&UnembedRequest::all_removable());
        assert_eq!(
            plan.pdfa,
            PdfaClaim::Identified {
                part: Some("2".to_owned()),
                conformance: Some("B".to_owned()),
            }
        );
        assert!(plan.pdfa.breaks_conformance());
        assert!(s.unembed_refusal().is_none());
        assert!(s.unembed_fonts(&UnembedRequest::all_removable()).is_ok());
    }

    /// The shortest name §9.6.4 can tag still renames, to one character.
    #[test]
    fn the_shortest_tagged_name_still_renames() {
        let s = session(include_bytes!(
            "../../../fixtures/synthetic/unembed/unembed-shortest-subset-tag.pdf"
        ));
        let plan = s.unembed_preview(&UnembedRequest::all_removable());
        assert_eq!(plan.targets[0].rename.as_deref(), Some("X"));
    }

    /// Selection by name accepts BOTH spellings, and a name that matched
    /// nothing is reported rather than silently doing nothing.
    #[test]
    fn selection_by_name_takes_either_spelling_and_reports_a_miss() {
        let s = session(include_bytes!(
            "../../../fixtures/synthetic/text/subset-simple-embedded.pdf"
        ));
        for spelling in ["SUBSET+pdfceSubsetDemo", "pdfceSubsetDemo"] {
            let plan = s.unembed_preview(&UnembedRequest::named([spelling]));
            assert_eq!(plan.targets.len(), 1, "{spelling} selects the font");
            assert!(plan.unmatched.is_empty());
        }
        let miss = s.unembed_preview(&UnembedRequest::named(["Arial"]));
        assert!(miss.targets.is_empty());
        assert_eq!(miss.unmatched, vec!["Arial".to_owned()]);
    }

    /// An explicit selection does NOT list every other font as refused —
    /// only "everything removable" does. A font the operator did not name is
    /// not a refusal, and listing it as one buries the ones that are.
    #[test]
    fn an_explicit_selection_does_not_report_unrelated_fonts() {
        let s = session(include_bytes!(
            "../../../fixtures/synthetic/unembed/unembed-shared-program.pdf"
        ));
        let named = s.unembed_preview(&UnembedRequest::named(["AAAAAA+pdfceShared"]));
        assert!(named.blocked.is_empty());
        let all = s.unembed_preview(&UnembedRequest::all_removable());
        assert_eq!(all.blocked.len(), 1, "AllRemovable names the other font");
    }

    /// Selecting a font that cannot go refuses it BY NAME through the
    /// explicit path too — the caller asked, so the caller is answered.
    #[test]
    fn naming_a_blocked_font_refuses_it_by_name() {
        let s = session(include_bytes!(
            "../../../fixtures/synthetic/unembed/unembed-shared-program.pdf"
        ));
        let plan = s.unembed_preview(&UnembedRequest::named(["BBBBBB+pdfceShared"]));
        assert!(plan.targets.is_empty());
        assert_eq!(plan.blocked.len(), 1);
        assert_eq!(plan.blocked[0].blocker.token(), "unknown-symbolic-builtin");
    }

    /// "Nothing happened" and "it worked" must not share an exit path.
    #[test]
    fn a_plan_with_no_target_is_an_error_not_a_silent_success() {
        let mut s = session(include_bytes!(
            "../../../fixtures/synthetic/text/cidfonttype2-nocmap-embedded.pdf"
        ));
        let err = s
            .unembed_fonts(&UnembedRequest::all_removable())
            .expect_err("nothing could go");
        assert!(matches!(
            err,
            crate::edit::EditError::NoFontsToUnembed { blocked } if blocked > 0
        ));
        assert!(!s.is_modified(), "a refusal changes nothing");
    }

    /// Undo restores the document exactly, and an unembed-then-undo saves
    /// byte-identically — §11.1's diff-not-replay contract applied to the
    /// first destructive font operation.
    #[test]
    fn undo_restores_the_document_byte_for_byte() {
        let source = include_bytes!("../../../fixtures/synthetic/text/subset-simple-embedded.pdf");
        let mut s = session(source);
        s.unembed_fonts(&UnembedRequest::all_removable())
            .expect("unembeds");
        assert!(s.is_modified());
        assert!(s.undo().is_some());
        assert!(!s.is_modified(), "undo puts the document back");
        let (bytes, _) = s
            .to_incremental_bytes(&crate::writer::SaveOptions::identity())
            .expect("saves");
        assert_eq!(bytes, source, "edit -> undo -> save is byte-identical");
    }

    /// Round-trip / minimal diff (rule 3): the update section carries the
    /// descriptor and the font dictionary and NOTHING else, and the base
    /// revision's bytes are untouched.
    #[test]
    fn only_the_touched_objects_are_written() {
        let source = include_bytes!("../../../fixtures/synthetic/text/subset-simple-embedded.pdf");
        let mut s = session(source);
        let plan = s
            .unembed_fonts(&UnembedRequest::all_removable())
            .expect("unembeds");
        // Two rewrites (font dict for /BaseFont, descriptor for the rest)
        // and one free entry (the program).
        assert_eq!(
            s.dirty_set().len(),
            3,
            "font dict + descriptor + freed program"
        );
        let (bytes, report) = s
            .to_incremental_bytes(&crate::writer::SaveOptions::identity())
            .expect("saves");
        assert!(
            bytes.starts_with(source),
            "an incremental update APPENDS; the base revision is untouched"
        );
        assert_eq!(report.objects_written, 2);
        // And the bytes are NOT recovered by this save — they are still in
        // the prior revision. The plan's figure is a full-rewrite figure.
        assert!(bytes.len() > source.len());
        assert!(plan.bytes_reclaimable() > 0);
    }

    /// A full rewrite is where the bytes actually go.
    #[test]
    fn a_full_rewrite_is_smaller_by_roughly_the_program() {
        let source = include_bytes!("../../../fixtures/synthetic/text/subset-simple-embedded.pdf");
        let mut s = session(source);
        let plan = s
            .unembed_fonts(&UnembedRequest::all_removable())
            .expect("unembeds");
        let (bytes, _) = s
            .to_full_bytes(&crate::writer::SaveOptions::default())
            .expect("saves");
        assert!(
            bytes.len() < source.len(),
            "a full rewrite drops the freed program"
        );
        let saved = source.len() - bytes.len();
        let claimed = plan.bytes_reclaimable() as usize;
        // Not exact: the cross-reference table loses an entry too, and the
        // producer string may differ. Within a few hundred bytes is the
        // honest assertion.
        assert!(
            saved + 400 >= claimed && claimed + 400 >= saved,
            "claimed {claimed}, actually saved {saved}"
        );
    }

    /// The encryption refusal fires BEFORE any mutation (rule 4). Reading is
    /// not the refused act, so the report still works.
    #[test]
    fn an_encrypted_document_is_refused_before_anything_changes() {
        let doc = Document::from_bytes_with_password(
            include_bytes!("../../../fixtures/synthetic/fontinfo/enc-aes-128-embedded-font.pdf")
                .to_vec(),
            Some(b"userpw"),
        )
        .expect("fixture opens with the corpus password");
        let mut s = EditSession::new(doc);
        assert!(
            !s.unembed_preview(&UnembedRequest::all_removable())
                .targets
                .is_empty()
        );
        assert!(matches!(
            s.unembed_refusal(),
            Some(crate::edit::EditError::DocumentEncrypted)
        ));
        assert!(matches!(
            s.unembed_fonts(&UnembedRequest::all_removable()),
            Err(crate::edit::EditError::DocumentEncrypted)
        ));
        assert!(!s.is_modified(), "nothing was mutated before the refusal");
    }
}
