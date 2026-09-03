//! Font **embedding of a program the document references but does not carry**
//! — planning it, and stating every inference before any of it becomes
//! document state.
//!
//! The constructive mirror of [`crate::font_unembed`]. That module removes a
//! font program and leaves a reference a reader satisfies by substitution;
//! this one takes a reference a reader is *already* satisfying by
//! substitution and makes the substitution permanent, inside the file, where
//! every consumer sees the same letterforms.
//!
//! Like its mirror it **consumes [`crate::fontinfo`]'s inventory rather than
//! re-deriving one**. There is exactly one font classifier in pdfcer, so the
//! panel that says "Not embedded" and the command that fixes it cannot
//! disagree about which fonts are missing.
//!
//! Nothing here mutates a document. It produces an [`EmbedPlan`] — a
//! description of what *would* happen — and
//! [`EditSession::embed_fonts`](crate::edit::EditSession::embed_fonts)
//! executes it through the command log. Rule 4 (fuzzy, never sneaky) requires
//! the operator to see the consequences first, and a preview that ran
//! different code from the commit would be a disclosure that could lie.
//!
//! # ★ Why this exists, and why the original request was backwards
//!
//! The Pass began as *"someone needs embedded fonts removed."* It was the
//! wrong way round. The end user was uploading a book to **Barnes & Noble
//! Press**; like every print-on-demand service it **requires** fonts to be
//! embedded and rejects a file with missing ones. `list-fonts` already
//! diagnoses that (`not-embedded=N`); this module is the fix.
//!
//! That origin sets the acceptance bar: the number an operator is trying to
//! drive to zero is the **count of non-embedded fonts in the output**, not
//! the number of fonts pdfcer found something to do with.
//!
//! # ★ The crate boundary: the shell resolves a NAME, core takes BYTES
//!
//! `pdfcer-render::FontEnvironment` — which knows what faces exist on disk —
//! lives in `pdfcer-render`, and `pdfcer-core` must never depend on it
//! (project rule 2, `ARCHITECTURE.md` §3; CI enforces it). So the seam runs
//! one way only: a shell resolves the document's `/BaseFont` to a font file,
//! reads the bytes, and hands them here as a [`SuppliedFont`] together with
//! the **provenance** of the match. This module never touches a filesystem
//! and never asks what fonts a machine has.
//!
//! It does, however, **sniff the program's format itself** (see
//! [`ProgramFormat::sniff`]) rather than trusting the shell to describe it.
//! The choice of `/FontFile` key is a conformance decision governed by
//! §9.9 Table 126, and a conformance decision made from a caller's
//! *assertion* about bytes is a conformance decision that can be wrong
//! without anyone noticing. The sniff reuses the same bounds-checked sfnt
//! table walk [`crate::fontinfo::read_fs_type`] already performs, so no font
//! parser enters `pdfcer-core` (R21).
//!
//! # ★ Layout CANNOT shift. Only the shapes change.
//!
//! This is the fact that makes the whole operation safe, and it is the
//! opposite of the intuition ("a near-match font will reflow my book").
//!
//! A PDF positions text from the **`/Widths` array in the file**, never from
//! the font program (§9.6.2.1 Table 111; decision 004 §3.6, which is also
//! why `--font-dir` improves *shapes only*). Both of this module's shapes
//! preserve that exactly:
//!
//! - [`EmbedShape::Attach`] does not touch `/Widths` at all. It adds one key
//!   to one `/FontDescriptor`.
//! - [`EmbedShape::Synthesise`] writes `/Widths` for a standard-14 font that
//!   had none — **from pdfcer's own compiled Adobe Core-14 AFM tables**
//!   ([`crate::fontdata::std14_width`]), which are the very metrics a
//!   conforming reader was already using for that font. The numbers written
//!   are the numbers already in effect.
//!
//! So no glyph moves. What changes is which face draws inside those
//! advances — and that is disclosed as a certainty, not a risk, exactly as
//! the mirror module discloses its own appearance change.
//!
//! # The two structural shapes, and why there are exactly two
//!
//! Measured over the corpus (3,912 loadable real-world files, 1,534
//! non-embedded fonts) the population is not evenly spread; it is two
//! clusters and a tail:
//!
//! | Shape in the file | Count | Share | Handled as |
//! |---|---|---|---|
//! | standard-14 name, no `/FontDescriptor`, no `/Widths` | 1,272 | 82.9 % | [`EmbedShape::Synthesise`] |
//! | simple font with both a descriptor and `/Widths` | 180 | 11.7 % | [`EmbedShape::Attach`] |
//! | standard-14 name that *does* carry both | 8 | 0.5 % | [`EmbedShape::Attach`] |
//! | Type 3 | 49 | 3.2 % | refused — nothing is missing |
//! | composite (`Type0`/CID) | 11 | 0.7 % | refused by name |
//! | simple font with neither descriptor nor `/Widths`, not standard-14 | 14 | 0.9 % | refused — no metric source |
//!
//! Two shapes cover 95 %. The tail is refused **by name, with a reason** —
//! the same posture as [`crate::font_unembed`], and for the same reason: a
//! font that vanishes from both the "done" and the "refused" lists is the
//! silence this family of features exists to break.
//!
//! ## Shape 1 — `Attach`: add one key, change nothing else
//!
//! The `/FontDescriptor` and `/Widths` already exist, so the only thing
//! missing is the program. One `/FontFile`, `/FontFile2` or `/FontFile3`
//! entry is added to the descriptor (§9.9 Table 126) and one stream object
//! is created. `/Widths`, `/FirstChar`, `/LastChar`, `/Encoding`,
//! `/Differences`, `/Flags`, `/FontBBox`, `/ToUnicode` and every content
//! stream are untouched. The incremental update section therefore carries
//! exactly **two objects**: the rewritten descriptor and the new stream
//! (plus the font dictionary only if a subset tag is being dropped).
//!
//! ## Shape 2 — `Synthesise`: build what §9.6.2.2 permitted to be absent
//!
//! §9.6.2.2 lets the 14 standard fonts omit `/FontDescriptor`, `/Widths`,
//! `/FirstChar` and `/LastChar` entirely, because a conforming reader is
//! required to know those fonts. A file that leans on that permission has
//! no metrics *in* it — which is exactly why 83 % of the corpus's
//! non-embedded fonts are of this shape, and why refusing it would be
//! refusing the problem.
//!
//! So pdfcer writes them, from data it already compiles in for text
//! extraction and appearance generation:
//!
//! | Entry | Source | Why it is faithful |
//! |---|---|---|
//! | `/Widths`, `/FirstChar`, `/LastChar` | [`crate::fontdata::std14_width`] over the font's **resolved** code→glyph-name table | The Adobe Core-14 AFM advances — the numbers a reader was already applying |
//! | `/FontDescriptor` | [`crate::fontdata::std14_descriptor`] | The AFM header metrics, plus Table 123 `/Flags` |
//! | `/Encoding` `/Differences` | [`crate::fontdata::encoding_glyph_name`] over [`crate::fontdata::std14_builtin_encoding`] | Written **only when the dictionary has none**, and it spells out the encoding the file was relying on implicitly |
//!
//! The code→glyph-name table comes from
//! [`crate::text_extract::font::ExtractFont`] — the one resolver in pdfcer
//! that implements §9.6.6's base-encoding-plus-`/Differences` chain (R171:
//! one owner, never restated). If extraction and embedding resolved codes
//! through different tables, a document could be searched for text that is
//! not what was painted.
//!
//! ### ★ Why `/Encoding` is pinned, and why that is the honest move
//!
//! When a standard-14 dictionary carries no `/Encoding`, §9.6.6.2 sends the
//! reader to *the font program's own built-in encoding* — and there is no
//! font program, so the reader falls back to the standard-14 face's
//! encoding. The moment pdfcer embeds a program, that clause starts pointing
//! at **the program pdfcer chose**, whose internal encoding is a property of
//! a face the document never named.
//!
//! Leaving `/Encoding` absent would therefore make the document's *text*
//! depend on pdfcer's donor choice. Writing the encoding out — from pdfcer's
//! compiled Annex D tables, which is what the reader was using — keeps the
//! text identical and confines the change to the shapes, which is the whole
//! contract of this operation.
//!
//! `/StandardEncoding` is **not** a legal `/Encoding` name in a file (Table
//! 114 admits only `MacRomanEncoding`, `MacExpertEncoding` and
//! `WinAnsiEncoding`), so it cannot be named — it has to be spelled out as a
//! `/Differences` array. Substituting `/WinAnsiEncoding` because it *is*
//! nameable would silently change which glyph a dozen codes draw
//! (`0o47` is `quoteright` under Standard and `quotesingle` under WinAnsi),
//! so the verbose form is the only correct one.
//!
//! ### ★ Re-declaring `/Subtype` — the one shape-changing move, and its guard
//!
//! §9.9 Table 126 binds the `/FontFile*` key to the **font dictionary's
//! subtype**, not to the operator's preference:
//!
//! | Dictionary | Admissible program |
//! |---|---|
//! | `Type1` / `MMType1` | Type 1 (`/FontFile`), bare CFF (`/FontFile3 /Type1C`), OpenType-with-`CFF `-and-`cmap` (`/FontFile3 /OpenType`, case OT-3) |
//! | `TrueType` | TrueType (`/FontFile2`), OpenType-with-`glyf` (`/FontFile3 /OpenType`, case OT-1) |
//!
//! The 14 standard fonts are `/Type1`. A Windows machine's font folder is
//! almost entirely `glyf` TrueType. So the commonest real pairing —
//! `Helvetica` in the document, `arial.ttf` on disk — has **no admissible
//! key at all** while the dictionary says `Type1`.
//!
//! pdfcer resolves that by re-declaring the font dictionary as `/TrueType`,
//! and only inside `Synthesise`, where it is already writing the descriptor,
//! the widths and the encoding from scratch. Three conditions gate it, and
//! all three are checked:
//!
//! 1. The font is **nonsymbolic** — §9.6.6.4 Branch A (code → glyph name →
//!    Unicode → `(3,1)` cmap) is defined only for a nonsymbolic TrueType
//!    font, and Branch B's symbolic path goes through the program's own
//!    `(3,0)` cmap, which a face the document never named cannot be trusted
//!    to carry.
//! 2. The effective encoding is **spellable as glyph names an AGL lookup can
//!    resolve** — Standard/WinAnsi/MacRoman. `Symbol` and `ZapfDingbats`
//!    name glyphs (`a1`, `Alpha`) the AGL subset does not carry, so Branch A
//!    cannot complete and the re-declaration is refused
//!    ([`EmbedBlocker::EncodingNotSpellable`]).
//! 3. The descriptor pdfcer writes carries the Table 123 `Nonsymbolic` flag,
//!    which is what puts the reader on Branch A in the first place.
//!
//! For a bare-CFF donor (including pdfcer's own bundled Base-14 faces) none
//! of this arises: `/FontFile3 /Type1C` is admissible under the existing
//! `/Type1` dictionary and the subtype is left alone.
//!
//! # ★ What is REFUSED, and why each refusal is principled rather than lazy
//!
//! ## Composite (`Type0` / CIDFont) — refused
//!
//! Under `Identity-H` the character codes in the content stream **are glyph
//! indices into the absent program** (§9.7.4.2). A different face has a
//! different glyph order, so embedding a substitute would not draw
//! different-looking letters — it would draw **the wrong letters**. Under a
//! predefined CMap the `/CIDSystemInfo` registry/ordering names a specific
//! character collection the donor must implement, and pdfcer cannot verify
//! from the bytes that it does. Both are cases where a plausible-looking
//! result would be silently wrong, which is worse than a refusal.
//!
//! ## Type 3 — refused, because nothing is missing
//!
//! A Type 3 font's glyphs are `/CharProcs` content streams **already inside
//! the document** (§9.6.5). There is no font program to supply and no
//! print-service check it can fail. Reported by name so an operator reading
//! "49 fonts are not embedded" is not left wondering which lever applies.
//!
//! ## A simple font with no `/Widths` that is not one of the standard 14
//!
//! There is no source for the advances the file assumes. Taking them from
//! the donor would move every glyph on the page — the single thing this
//! operation guarantees it will not do.
//!
//! ## A donor whose `fsType` forbids embedding
//!
//! §9.9's opening paragraph: a font program whose licence forbids embedding
//! *"should not be incorporated into a PDF file."* pdfcer reads the OpenType
//! `OS/2` `fsType` field with the classifier it already has
//! ([`crate::fontinfo::read_fs_type`]) and refuses
//! [`crate::fontinfo::EmbeddingPermission::Restricted`] by name. Note the
//! modality carefully: §9.9 E1 is a **`should`**, not a `shall` — the hard
//! prohibition comes from the font's licence, not from ISO 32000-1. pdfcer
//! refuses anyway, because the alternative is writing somebody's
//! no-embedding font into a document on their behalf.
//!
//! ## A symbolic font matched by anything other than its own name
//!
//! A symbolic font's codes mean whatever its program says they mean. A
//! stand-in chosen by family resemblance draws a different repertoire, not a
//! different style. So a symbolic font accepts an
//! [`FontMatch::Exact`] donor and refuses an inferred one
//! ([`EmbedBlocker::SymbolicSubstitute`]).
//!
//! # Hazards this module exists to get right
//!
//! **A shared `/FontDescriptor` must not be edited.** Two font dictionaries
//! can legally point at one descriptor. Attaching a program there would
//! embed a font the operator did not select, with a face pdfcer chose for a
//! different `/BaseFont`. Blocked by name
//! ([`EmbedBlocker::DescriptorShared`]), exactly as the mirror module blocks
//! the same shape.
//!
//! **A direct font dictionary cannot be edited** — it has no object identity
//! for the overlay to write, so it is blocked by name rather than skipped.
//!
//! **Partial success is normal and is reported per font.** The commonest
//! real outcome is a document with several missing fonts of which some
//! resolve and some do not. An all-or-nothing result would be wrong, and a
//! silent partial one would be worse.
//!
//! # PDF/A and signatures
//!
//! Embedding moves a file **toward** ISO 19005 conformance — every part
//! requires fonts to be embedded — which is the opposite of what unembedding
//! does. [`EmbedPlan::pdfa`] reports the document's self-identification for
//! context, and nothing here claims conformance: pdfcer does not validate
//! PDF/A and a file can fail it for a hundred reasons that have nothing to
//! do with fonts.
//!
//! Any modification invalidates a signature over the changed bytes; that is
//! disclosed by the shells through
//! [`EditSession::signature_impact_of_save`](crate::edit::EditSession::signature_impact_of_save),
//! the same channel every other editing operation uses.
//!
//! # Spec sources
//!
//! - ISO 32000-1 §9.9 Tables 126/127 — which `/FontFile*` key is admissible
//!   for which font dictionary; `/Length1` is the **decoded** byte count;
//!   `/FontFile3` *shall not* carry `Length1/2/3`; the embedding-permission
//!   paragraph (E1/E2/E3)
//!   - `D:\Dev\Rag-Specialized\PDF_Spec\iso32000\iso32000__s__9.9.md`
//! - ISO 32000-1 §9.8.1 Table 122 — descriptor entries; `/FontName` *shall*
//!   equal `/BaseFont` — `iso32000__s__9.8.md`
//! - ISO 32000-1 §9.6.2.2 — the standard 14 may omit descriptor and widths
//! - ISO 32000-1 §9.6.6.1/.2/.4 — encodings for Type 1 and TrueType fonts
//! - ISO 32000-1 §9.6.4 — subset tags: "exactly six uppercase letters"
//!   - `iso32000__s__9.6.md`
//! - ISO 32000-1 §9.7.4.2 — CIDFont glyph selection (the composite refusal)
//! - ISO 32000-1 §7.5.6 — an update section carries changed objects only

use std::collections::{BTreeMap, BTreeSet};

use crate::font_unembed::PdfaClaim;
use crate::fontdata::{self, BaseEncoding, Std14};
use crate::fontinfo::{
    EmbeddingPermission, FontInventory, FontRecord, FontSubtype, FsTypeError, Program, ProgramKey,
    split_subset_tag,
};
use crate::graph::ObjectGraph;
use crate::object::{Dict, Name, ObjId, Object};
use crate::view::DocumentView;

/// The largest donor program pdfcer will embed, in bytes.
///
/// `ARCHITECTURE.md` §10.1 requires a ceiling on anything that consumes
/// caller-supplied bytes, and the plan holds every donor in memory at once.
/// 32 MiB clears the largest real faces by a wide margin — a full CJK
/// OpenType face is 15–20 MiB, a Latin face 0.1–1 MiB — while keeping a
/// pathological input from becoming an allocation the operator cannot
/// interrupt.
pub const MAX_DONOR_BYTES: usize = 32 * 1024 * 1024;

// ---------------------------------------------------------------------------
// The supplied program
// ---------------------------------------------------------------------------

/// How the shell arrived at this donor for this `/BaseFont`.
///
/// **This is the inference rule 4 governs.** pdfcer is choosing a font
/// program the document did not carry, and the three rungs are three
/// materially different acts: honouring a name the file already spells,
/// applying a well-known family equivalence, or falling back to a face pdfcer
/// ships. The operator sees which one before it becomes document state.
///
/// Ordered from most to least specific; [`Self::is_substitute`] is the line
/// that matters for the symbolic guard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum FontMatch {
    /// The donor advertises the name the document asks for (after a §9.6.4
    /// subset tag is stripped). The document named a face and that face was
    /// found.
    Exact,
    /// The donor was reached through a documented family equivalence —
    /// `Helvetica` → `Arial`, `Times-Roman` → `Times New Roman` — rather
    /// than by name. Metric-compatible by design, and the advances come from
    /// `/Widths` regardless, but the letterforms are a different designer's.
    Alias,
    /// The donor is one of the faces pdfcer itself ships as a standard-14
    /// substitute. The most inferred rung: nothing on the operator's machine
    /// was consulted.
    Bundled,
}

impl FontMatch {
    /// Whether this donor is something other than the face the document
    /// named.
    ///
    /// The symbolic guard turns on exactly this: a symbolic font's codes
    /// mean what its own program says they mean, so a stand-in draws a
    /// different repertoire rather than a different style.
    #[must_use]
    pub const fn is_substitute(self) -> bool {
        !matches!(self, Self::Exact)
    }

    /// A stable, locale-invariant token for a machine-readable report.
    #[must_use]
    pub const fn token(self) -> &'static str {
        match self {
            Self::Exact => "exact",
            Self::Alias => "alias",
            Self::Bundled => "bundled",
        }
    }
}

/// One font program a shell resolved, with the provenance of the match.
///
/// Deliberately free of any font-crate type and of any filesystem type: the
/// seam is plain data, so `pdfcer-core` acquires neither a font parser nor an
/// opinion about where fonts live (project rule 2).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct SuppliedFont {
    /// The program bytes exactly as they will be embedded, **undecoded and
    /// unmodified**. pdfcer does not subset, re-order tables, or strip
    /// anything: what the operator supplied is what the document gets, so
    /// the face in the file is the face they can verify on disk.
    pub program: Vec<u8>,
    /// The name the donor advertises, or the file stem it was registered
    /// under — the string an operator recognises as "which font is this".
    pub face_name: String,
    /// Where it came from, for disclosure. A path, or a description such as
    /// `"bundled: FoxitSans"`. Never parsed; only reported.
    pub source: String,
    /// How the shell got from the document's `/BaseFont` to this face.
    pub matched: FontMatch,
}

impl SuppliedFont {
    /// A donor with the given provenance.
    #[must_use]
    pub fn new(
        program: Vec<u8>,
        face_name: impl Into<String>,
        source: impl Into<String>,
        matched: FontMatch,
    ) -> Self {
        Self {
            program,
            face_name: face_name.into(),
            source: source.into(),
            matched,
        }
    }
}

/// The framing of a donor program, sniffed from its own bytes.
///
/// Determined here rather than taken from the caller because the choice of
/// `/FontFile*` key is a §9.9 Table 126 conformance decision, and a
/// conformance decision made from an assertion about bytes can be wrong
/// without anyone noticing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ProgramFormat {
    /// An sfnt carrying `glyf` outlines — an ordinary `.ttf`, or an
    /// OpenType face whose outlines are TrueType.
    TrueTypeGlyf,
    /// An sfnt carrying a `CFF ` table — an `.otf`.
    OpenTypeCff,
    /// A bare CFF program (`Type1C` / `CIDFontType0C` framing). pdfcer's own
    /// bundled Base-14 faces are this.
    BareCff,
    /// A bare PostScript Type 1 program, PFB-framed or PFA text.
    Type1,
    /// A TrueType/OpenType **collection** (`ttcf`). Never embeddable: a
    /// collection holds several faces and the PDF names one font.
    Collection,
    /// The bytes match no font framing pdfcer recognises.
    Unrecognised,
}

impl ProgramFormat {
    /// Classify `program` by its framing.
    ///
    /// # The algorithm, and why it is written out rather than delegated
    ///
    /// Four magics decide the container (§9.9's own list plus the Apple
    /// legacy `true`), and for an sfnt the table directory decides the
    /// outline flavour — `glyf` present means TrueType outlines, `CFF `
    /// (note the **trailing space**) means CFF outlines. The directory walk
    /// is the same bounds-checked shape
    /// [`crate::fontinfo::read_fs_type`] already uses, for the same reason:
    /// `pdfcer-core` must not gain a font-parsing dependency, and thirty
    /// lines of checked arithmetic is a far smaller cost than a dependency
    /// edge in the wrong direction.
    ///
    /// Every slice is bounds-tested and every offset is checked. This runs
    /// on bytes an operator pointed at, which is untrusted input.
    ///
    /// # Examples
    ///
    /// ```
    /// use pdfcer_core::font_embed_missing::ProgramFormat;
    ///
    /// assert_eq!(ProgramFormat::sniff(b"%PDF-1.7\n"), ProgramFormat::Unrecognised);
    /// assert_eq!(ProgramFormat::sniff(&[0x01, 0x00, 0x04, 0x01]), ProgramFormat::BareCff);
    /// assert_eq!(ProgramFormat::sniff(b"ttcf\0\x01\0\0"), ProgramFormat::Collection);
    /// ```
    #[must_use]
    pub fn sniff(program: &[u8]) -> Self {
        match program {
            [0x80, 0x01, ..] => return Self::Type1,
            [b'%', b'!', ..] => return Self::Type1,
            [0x01, 0x00, ..] => return Self::BareCff,
            [b't', b't', b'c', b'f', ..] => return Self::Collection,
            [0x00, 0x01, 0x00, 0x00, ..]
            | [b'O', b'T', b'T', b'O', ..]
            | [b't', b'r', b'u', b'e', ..] => {}
            _ => return Self::Unrecognised,
        }
        // An sfnt. Which outline table does it carry?
        let be16 = |at: usize| -> Option<u16> {
            let b: [u8; 2] = program.get(at..at.checked_add(2)?)?.try_into().ok()?;
            Some(u16::from_be_bytes(b))
        };
        let Some(num_tables) = be16(4).map(usize::from) else {
            return Self::Unrecognised;
        };
        if num_tables > crate::fontinfo::MAX_SFNT_TABLES {
            return Self::Unrecognised;
        }
        let mut has_glyf = false;
        let mut has_cff = false;
        for i in 0..num_tables {
            let Some(rec) = i.checked_mul(16).and_then(|o| o.checked_add(12)) else {
                return Self::Unrecognised;
            };
            let Some(tag) = rec.checked_add(4).and_then(|end| program.get(rec..end)) else {
                return Self::Unrecognised;
            };
            if tag == b"glyf" {
                has_glyf = true;
            } else if tag == b"CFF " {
                has_cff = true;
            }
        }
        // `glyf` wins a tie. An sfnt carrying both is malformed; the `glyf`
        // reading is the one Table 126's OT-1 case can express, and OT-1
        // (unlike OT-2/OT-3) does not additionally require a `cmap`.
        if has_glyf {
            Self::TrueTypeGlyf
        } else if has_cff {
            Self::OpenTypeCff
        } else {
            Self::Unrecognised
        }
    }

    /// A stable, locale-invariant token for a machine-readable report.
    #[must_use]
    pub const fn token(self) -> &'static str {
        match self {
            Self::TrueTypeGlyf => "truetype",
            Self::OpenTypeCff => "opentype-cff",
            Self::BareCff => "cff",
            Self::Type1 => "type1",
            Self::Collection => "collection",
            Self::Unrecognised => "unrecognised",
        }
    }

    /// The operator-facing name of this format.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::TrueTypeGlyf => "TrueType",
            Self::OpenTypeCff => "OpenType (CFF outlines)",
            Self::BareCff => "Compact Font Format",
            Self::Type1 => "PostScript Type 1",
            Self::Collection => "font collection",
            Self::Unrecognised => "unrecognised",
        }
    }
}

/// Whether an sfnt donor carries a `cmap` table.
///
/// §9.9 Table 126's OT-3 case — a `CFF `-outline OpenType program under a
/// `Type1` font dictionary — requires one ("In addition to the `CFF `
/// table, the font program must include the `cmap` table"), and §9.9's T2
/// requires one for any TrueType program used with a **simple** font
/// dictionary. Both are checked rather than assumed, because the failure is
/// a non-conformant file rather than an error anybody sees.
fn sfnt_has_cmap(program: &[u8]) -> bool {
    let be16 = |at: usize| -> Option<u16> {
        let b: [u8; 2] = program.get(at..at.checked_add(2)?)?.try_into().ok()?;
        Some(u16::from_be_bytes(b))
    };
    let Some(num_tables) = be16(4).map(usize::from) else {
        return false;
    };
    if num_tables > crate::fontinfo::MAX_SFNT_TABLES {
        return false;
    }
    (0..num_tables).any(|i| {
        i.checked_mul(16)
            .and_then(|o| o.checked_add(12))
            .and_then(|rec| rec.checked_add(4).and_then(|end| program.get(rec..end)))
            == Some(b"cmap".as_slice())
    })
}

// ---------------------------------------------------------------------------
// The request
// ---------------------------------------------------------------------------

/// Which fonts to embed into.
///
/// Three shapes for the same three callers [`crate::font_unembed`] has: a
/// GUI holds an object identity, a CLI holds a name the operator typed, and
/// "every font that is missing" is the batch case the feature exists for.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum EmbedSelection {
    /// Every font whose program is absent.
    AllMissing,
    /// Fonts named by `/BaseFont` **or** by the de-prefixed family name, so
    /// both `ABCDEF+Arial` and `Arial` select. A name that matches nothing
    /// is reported in [`EmbedPlan::unmatched`], never silently ignored.
    Named(Vec<String>),
    /// Fonts by font-dictionary object identity — the shape a GUI row has.
    Objects(Vec<ObjId>),
}

/// One embed request: which fonts, and what programs are available for them.
///
/// `supplied` is keyed by the document's `/BaseFont` **exactly as the file
/// spells it**, subset tag included. The shell owns resolution; this module
/// owns what may be done with the result.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct EmbedRequest {
    /// Which fonts.
    pub selection: EmbedSelection,
    /// `/BaseFont` → the donor the shell resolved for it. A selected font
    /// with no entry here is reported as
    /// [`EmbedBlocker::NoSourceFont`] — by name, with what would satisfy it.
    pub supplied: BTreeMap<String, SuppliedFont>,
}

impl EmbedRequest {
    /// Embed into every font whose program is absent.
    #[must_use]
    pub fn all_missing() -> Self {
        Self {
            selection: EmbedSelection::AllMissing,
            supplied: BTreeMap::new(),
        }
    }

    /// Embed into the fonts named by `/BaseFont` or family name.
    #[must_use]
    pub fn named<I, S>(names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            selection: EmbedSelection::Named(names.into_iter().map(Into::into).collect()),
            supplied: BTreeMap::new(),
        }
    }

    /// Embed into the fonts with these font-dictionary object identities.
    #[must_use]
    pub fn objects<I: IntoIterator<Item = ObjId>>(ids: I) -> Self {
        Self {
            selection: EmbedSelection::Objects(ids.into_iter().collect()),
            supplied: BTreeMap::new(),
        }
    }

    /// Offer `donor` for the document's `base_font` name.
    #[must_use]
    pub fn with_font(mut self, base_font: impl Into<String>, donor: SuppliedFont) -> Self {
        self.supplied.insert(base_font.into(), donor);
        self
    }
}

// ---------------------------------------------------------------------------
// The plan
// ---------------------------------------------------------------------------

/// Which structural shape one embed takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum EmbedShape {
    /// The `/FontDescriptor` and `/Widths` already exist; one `/FontFile*`
    /// key is added and **nothing else changes**.
    Attach,
    /// A standard-14 font that §9.6.2.2 permitted to omit its descriptor and
    /// metrics: pdfcer writes `/FirstChar`, `/LastChar`, `/Widths`, a
    /// `/FontDescriptor`, and — when the dictionary carries none — an
    /// explicit `/Encoding`, all from its compiled Adobe Core-14 data.
    Synthesise,
}

impl EmbedShape {
    /// A stable, locale-invariant token for a machine-readable report.
    #[must_use]
    pub const fn token(self) -> &'static str {
        match self {
            Self::Attach => "attach",
            Self::Synthesise => "synthesise",
        }
    }
}

/// Why one font will not have a program embedded.
///
/// Every variant is a statement about **this document and this donor**, and
/// every one is reported by name. No font is ever silently absent from a
/// result — the same contract [`crate::font_unembed::UnembedBlocker`] holds,
/// and for the same reason.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum EmbedBlocker {
    /// The font already carries a program. Not a fault — it is the answer
    /// to "why is this font not in the list of things that changed".
    AlreadyEmbedded,
    /// The font declares a program that could not be read. Replacing it
    /// would be a repair rather than an embed, and pdfcer does not silently
    /// overwrite bytes it failed to parse.
    ProgramDeclaredButUnreadable,
    /// No donor was offered for this `/BaseFont`. **The commonest outcome of
    /// a headless run**, and the one whose message has to say what would fix
    /// it.
    NoSourceFont,
    /// The font is composite (`Type0` with a CIDFont descendant).
    Composite {
        /// Whether the encoding is `Identity-H`/`Identity-V`, which makes
        /// the character codes glyph indices into the absent program.
        identity: bool,
    },
    /// A Type 3 font. Its glyphs are `/CharProcs` content streams already in
    /// the document; there is no program to supply.
    Type3,
    /// A simple font with no `/Widths` that is not one of the standard 14,
    /// so pdfcer has no source for the advances the file assumes.
    NoMetricSource,
    /// The donor bytes match no font framing pdfcer recognises.
    ProgramUnrecognised,
    /// The donor is a font **collection** (`ttcf`). A collection holds
    /// several faces; the PDF names one font, and picking a face would be
    /// pdfcer guessing which.
    ProgramIsCollection,
    /// §9.9 Table 126 admits no `/FontFile*` key for this pairing of font
    /// dictionary and program format, and the re-declaration path is not
    /// available.
    FormatNotAdmissible {
        /// What the font dictionary declares itself to be.
        subtype: FontSubtype,
        /// What the donor actually is.
        format: ProgramFormat,
    },
    /// An OpenType donor that Table 126's OT-3 case requires to carry a
    /// `cmap` table, and does not.
    OpenTypeMissingCmap,
    /// The donor's OpenType `OS/2` `fsType` says the font may not be
    /// embedded (§9.9's opening paragraph).
    EmbeddingForbidden {
        /// The permission read from the donor.
        permission: EmbeddingPermission,
        /// The raw `fsType` value, so the disclosure can quote it.
        raw: u16,
    },
    /// A **symbolic** font offered a donor pdfcer inferred rather than one
    /// the document named. A symbolic font's codes mean what its own program
    /// says; a stand-in draws a different repertoire.
    SymbolicSubstitute {
        /// How the donor was reached.
        matched: FontMatch,
    },
    /// The re-declaration to `/TrueType` needs an encoding spellable as
    /// glyph names §9.6.6.4 Branch A can resolve through the AGL, and this
    /// font's built-in encoding is not one.
    EncodingNotSpellable,
    /// The font dictionary is a **direct** object inside a resource
    /// dictionary, so it has no identity for the overlay to write.
    FontNotIndirect,
    /// The `/FontDescriptor` object is shared with at least one font that is
    /// **not** part of this operation, so attaching a program there would
    /// embed that font too — with a face chosen for a different name.
    DescriptorShared {
        /// The other font dictionaries reaching the same descriptor.
        with: Vec<ObjId>,
    },
    /// The `/FontDescriptor` could not be reached as a dictionary.
    DescriptorUnreadable,
    /// The donor exceeds [`MAX_DONOR_BYTES`].
    ProgramTooLarge {
        /// The donor's size.
        bytes: usize,
    },
}

impl EmbedBlocker {
    /// A stable, locale-invariant token for a machine-readable report.
    #[must_use]
    pub const fn token(&self) -> &'static str {
        match self {
            Self::AlreadyEmbedded => "already-embedded",
            Self::ProgramDeclaredButUnreadable => "program-unreadable",
            Self::NoSourceFont => "no-source-font",
            Self::Composite { .. } => "composite",
            Self::Type3 => "type3",
            Self::NoMetricSource => "no-metric-source",
            Self::ProgramUnrecognised => "donor-unrecognised",
            Self::ProgramIsCollection => "donor-collection",
            Self::FormatNotAdmissible { .. } => "format-not-admissible",
            Self::OpenTypeMissingCmap => "opentype-missing-cmap",
            Self::EmbeddingForbidden { .. } => "embedding-forbidden",
            Self::SymbolicSubstitute { .. } => "symbolic-substitute",
            Self::EncodingNotSpellable => "encoding-not-spellable",
            Self::FontNotIndirect => "font-not-indirect",
            Self::DescriptorShared { .. } => "descriptor-shared",
            Self::DescriptorUnreadable => "descriptor-unreadable",
            Self::ProgramTooLarge { .. } => "donor-too-large",
        }
    }

    /// The sentence an operator reads.
    ///
    /// Every one of them names what would satisfy the refusal where anything
    /// would, because a refusal that only says "no" is a dead end (R27).
    #[must_use]
    pub const fn reason(&self) -> &'static str {
        match self {
            Self::AlreadyEmbedded => {
                "This font already carries its own program inside the document, so there is \
                 nothing to add."
            }
            Self::ProgramDeclaredButUnreadable => {
                "This font says it carries a program, but those bytes could not be read. pdfcer \
                 will not overwrite a program it failed to parse — that would be repairing the \
                 file, not embedding a font."
            }
            Self::NoSourceFont => {
                "No font file for this face was found. Point pdfcer at a folder containing it — \
                 on Windows the system font folder is C:\\Windows\\Fonts — and run this again."
            }
            Self::Composite { .. } => {
                "This font's character codes are positions inside the specific program the \
                 document is missing, so no other font file can stand in for it: a substitute \
                 would draw the wrong characters, not merely different-looking ones. The \
                 original font file this document was made with is the only thing that would \
                 satisfy it."
            }
            Self::Type3 => {
                "This font's glyphs are drawn by instructions already inside the document. There \
                 is no font file missing, and nothing to embed."
            }
            Self::NoMetricSource => {
                "This font carries no character-width table and is not one of the 14 fonts every \
                 reader knows, so pdfcer has no record of the spacing this document assumes. \
                 Embedding a font here would move the text on the page."
            }
            Self::ProgramUnrecognised => {
                "The file offered for this font is not in a format pdfcer recognises as a font \
                 program. TrueType (.ttf), OpenType (.otf) and Compact Font Format files work."
            }
            Self::ProgramIsCollection => {
                "The file offered for this font is a font collection holding several faces, and \
                 the document names one font. Supply a single-face font file instead."
            }
            Self::FormatNotAdmissible { .. } => {
                "The PDF specification does not allow a font program of this kind to be attached \
                 to a font of this kind. Supply a face in the matching format — an OpenType or \
                 Compact Font Format face for a PostScript font, a TrueType face for a TrueType \
                 font."
            }
            Self::OpenTypeMissingCmap => {
                "The OpenType face offered here has no character map table, which the PDF \
                 specification requires when an OpenType face stands in for a PostScript font. \
                 Supply a face that carries one."
            }
            Self::EmbeddingForbidden { .. } => {
                "The font file offered here says in its own licensing field that it may not be \
                 embedded in a document. pdfcer will not embed it. Use a face whose licence \
                 permits embedding."
            }
            Self::SymbolicSubstitute { .. } => {
                "This is a symbol font: its character codes mean whatever its own font file says \
                 they mean, so a stand-in chosen by family resemblance would draw a different \
                 set of symbols rather than a different style. Supply the face this font \
                 actually names."
            }
            Self::EncodingNotSpellable => {
                "This font would have to be re-declared as a TrueType font to accept the face \
                 offered, and its character mapping cannot be written out in a form a TrueType \
                 font can use. Supply an OpenType or Compact Font Format face instead."
            }
            Self::FontNotIndirect => {
                "This font dictionary is written directly into a page's resources rather than as \
                 its own numbered object, so there is no object to rewrite without re-emitting \
                 the page."
            }
            Self::DescriptorShared { .. } => {
                "This font shares its description with another font that is not being changed. \
                 Attaching a program here would embed it into that font as well, using a face \
                 chosen for a different name."
            }
            Self::DescriptorUnreadable => {
                "This font's description could not be read as a dictionary, so there is nowhere \
                 to record the embedded program."
            }
            Self::ProgramTooLarge { .. } => {
                "The font file offered here is larger than pdfcer will embed. Supply a smaller \
                 face."
            }
        }
    }
}

/// A font that will not gain a program, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct EmbedBlocked {
    /// The font dictionary's identity, or `None` for a direct dictionary.
    pub id: Option<ObjId>,
    /// `/BaseFont` exactly as the file spells it.
    pub base_font: Option<String>,
    /// Why.
    pub blocker: EmbedBlocker,
    /// Whether this font is one of the ones [`EmbedPlan::missing_before`]
    /// counts — i.e. it carries no program *and still will not*.
    ///
    /// # Why a field rather than a test on [`Self::blocker`]
    ///
    /// The two are not the same question and a shell that conflates them
    /// reports a false number. A row blocked as
    /// [`EmbedBlocker::AlreadyEmbedded`] is not missing anything, and a row
    /// blocked as [`EmbedBlocker::ProgramDeclaredButUnreadable`] declares a
    /// program that merely could not be decoded — neither is counted by
    /// `missing_before`, so neither is part of the number the operator is
    /// driving to zero. Every *other* blocker names a font that has no
    /// program and is not getting one.
    ///
    /// This is what lets [`EmbedPlan::unexplained_missing`] be computed at
    /// all, and therefore what lets a report say "every one is listed above"
    /// only when that is true.
    pub missing_program: bool,
}

/// A font that will gain a program, and everything that changes about it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct EmbedTarget {
    /// The font dictionary's identity.
    pub id: ObjId,
    /// `/BaseFont` exactly as the file spells it, before any rename.
    pub base_font: Option<String>,
    /// Which structural shape this embed takes.
    pub shape: EmbedShape,
    /// The existing `/FontDescriptor` object, when one exists and is
    /// reached by reference. `None` when [`Self::shape`] is
    /// [`EmbedShape::Synthesise`] and a descriptor is being created, or when
    /// an existing descriptor is a direct dictionary inside the font
    /// dictionary (in which case the font dictionary itself carries the
    /// edit).
    pub descriptor_id: Option<ObjId>,
    /// Which `/FontFile*` key the program is recorded under (§9.9 Table
    /// 126).
    pub program_key: ProgramKey,
    /// The `/Subtype` the program stream dictionary carries, for
    /// `/FontFile3` (Table 127 makes it **required** there and forbids it
    /// elsewhere).
    pub stream_subtype: Option<&'static str>,
    /// The donor's framing.
    pub format: ProgramFormat,
    /// The donor's advertised face name.
    pub face_name: String,
    /// Where the donor came from.
    pub source: String,
    /// How the shell reached the donor. **The disclosure rule 4 requires.**
    pub matched: FontMatch,
    /// The program's size in bytes, as it will be stored **before** any
    /// stream compression.
    pub program_bytes: usize,
    /// The donor's embedding permission, when the format carries one.
    /// Reported even when it permits embedding — a licence field an operator
    /// never sees is a licence field they cannot honour.
    pub permission: Option<EmbeddingPermission>,
    /// The name the font will carry afterwards, when a §9.6.4 subset tag is
    /// being dropped. `None` when there is no tag.
    ///
    /// A full face is being embedded, so a name asserting the file holds a
    /// *subset* of it would be false. `/FontName` moves with it — Table 122
    /// makes the two equal by `shall`.
    pub rename: Option<String>,
    /// Whether `/Subtype` is being re-declared from `Type1` to `TrueType` so
    /// a `glyf` donor can be attached (see the module docs).
    pub redeclared_truetype: bool,
    /// How many `/Widths` entries are being written, `0` for
    /// [`EmbedShape::Attach`].
    pub widths_written: usize,
    /// Whether an explicit `/Encoding` is being written because the
    /// dictionary carried none.
    pub encoding_written: bool,
    /// Whether a `/FontDescriptor` is being created.
    pub descriptor_written: bool,
    /// 1-based page numbers the font is reachable from, from the inventory.
    pub pages: Vec<u32>,
}

/// Everything embedding would do, computed before anything changes.
///
/// Both a preview and the record of what happened: the same value is
/// returned by the preview query and by the committing call, produced by the
/// same function, so a front end cannot show one thing and do another.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct EmbedPlan {
    /// The fonts that will gain a program.
    pub targets: Vec<EmbedTarget>,
    /// The fonts that will not, each with its reason.
    ///
    /// For [`EmbedSelection::AllMissing`] this is **every** other font in
    /// the document, including the ones that are simply already embedded.
    pub blocked: Vec<EmbedBlocked>,
    /// Names from [`EmbedSelection::Named`] that matched no font.
    pub unmatched: Vec<String>,
    /// Whether the document identifies itself as PDF/A. Reported for
    /// context only: embedding moves a file **toward** ISO 19005
    /// conformance, and pdfcer validates none of it.
    pub pdfa: PdfaClaim,
    /// Which font-bearing surfaces the inventory searched, carried through
    /// unchanged from [`crate::fontinfo::inventory`].
    pub coverage: crate::fontinfo::SurfaceCoverage,
    /// How many fonts in the whole document have no program, counted from
    /// the inventory rather than from this plan.
    ///
    /// ★ **The number the operator is actually trying to drive to zero.** It
    /// is deliberately independent of the selection: a plan that embeds
    /// three of seven has done real work and still leaves a file a
    /// print-on-demand service will reject, and a report that showed only
    /// the three would read as success.
    pub missing_before: usize,
}

impl EmbedPlan {
    /// Whether this plan would change anything.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.targets.is_empty()
    }

    /// The uncompressed size of every program this plan would add.
    ///
    /// A **ceiling, not a prediction**: the writer deflates each program
    /// stream (§7.4.4), and a font typically halves. The uncompressed figure
    /// is reported because it is the one that is knowable before the save
    /// and cannot mislead in the direction that matters — the file will not
    /// grow by more than this.
    #[must_use]
    pub fn bytes_added_uncompressed(&self) -> u64 {
        self.targets.iter().map(|t| t.program_bytes as u64).sum()
    }

    /// How many fonts would still have no program after this plan ran.
    ///
    /// [`Self::missing_before`] minus the targets. **Zero is the end state
    /// the whole feature exists to reach**; any other number is what an
    /// operator still has to solve, and it is on the report rather than
    /// implied by an absence.
    #[must_use]
    pub fn missing_after(&self) -> usize {
        self.missing_before.saturating_sub(self.targets.len())
    }

    /// How many of [`Self::missing_after`]'s fonts appear in
    /// [`Self::blocked`] with a reason attached.
    ///
    /// Pairs with [`Self::unexplained_missing`]; the two always sum to
    /// `missing_after()`.
    #[must_use]
    pub fn explained_missing(&self) -> usize {
        self.blocked.iter().filter(|b| b.missing_program).count()
    }

    /// How many of [`Self::missing_after`]'s fonts this plan says **nothing**
    /// about.
    ///
    /// # ★ Why this exists, and what it is guarding against
    ///
    /// A report that prints `missing_after` and then asserts *"every one is
    /// listed above with its reason"* is making a claim it cannot keep under
    /// [`EmbedSelection::Named`]. A font the operator did not name is neither
    /// a target nor a refusal — `plan` deliberately does not list it, because
    /// under an explicit selection a font nobody asked about is not a
    /// refusal, and listing it as one would bury the fonts that are. So the
    /// count is right and the sentence is wrong: the operator is told to look
    /// for reasons that were never printed.
    ///
    /// That is exactly the failure project rule 4 exists to prevent — the
    /// tool describing its own output inaccurately — and it is invisible to
    /// any test that only ever passes `--all-missing`, where this number is
    /// always zero. A shell must gate the "listed above" wording on this
    /// being zero and account for the remainder separately.
    ///
    /// Zero under [`EmbedSelection::AllMissing`], by construction: every
    /// `NotEmbedded` font is selected, so it becomes a target or a blocked
    /// row.
    #[must_use]
    pub fn unexplained_missing(&self) -> usize {
        self.missing_after()
            .saturating_sub(self.explained_missing())
    }

    /// How many blocked fonts carry each reason, keyed by
    /// [`EmbedBlocker::token`].
    #[must_use]
    pub fn blocker_counts(&self) -> BTreeMap<&'static str, usize> {
        let mut out = BTreeMap::new();
        for b in &self.blocked {
            *out.entry(b.blocker.token()).or_insert(0) += 1;
        }
        out
    }

    /// Whether any target's donor was reached by inference rather than by
    /// the name the document spells.
    ///
    /// The gate a shell uses to decide whether the exact-vs-substitute
    /// disclosure needs prominence rather than a footnote.
    #[must_use]
    pub fn substitutes_any(&self) -> bool {
        self.targets.iter().any(|t| t.matched.is_substitute())
    }

    /// Whether any target re-declares its `/Subtype`.
    #[must_use]
    pub fn redeclares_any(&self) -> bool {
        self.targets.iter().any(|t| t.redeclared_truetype)
    }
}

// ---------------------------------------------------------------------------
// Planning
// ---------------------------------------------------------------------------

/// One font's resolved object identities and dictionary facts.
struct Located<'a> {
    record: &'a FontRecord,
    /// The `/FontDescriptor` object, when reached by reference.
    descriptor_id: Option<ObjId>,
    /// True when a descriptor exists as a **direct** dictionary inside the
    /// font dictionary, so writing the font dictionary writes it.
    descriptor_inline: bool,
    /// Whether any descriptor could be resolved to a dictionary at all.
    descriptor_readable: bool,
    /// Whether the font dictionary carries `/Widths`.
    has_widths: bool,
    /// Whether the font dictionary carries `/Encoding`.
    has_encoding: bool,
    /// The standard-14 face this font's name denotes, if any.
    std14: Option<Std14>,
}

/// Resolve one font record's dictionary facts.
///
/// **Locates, never classifies.** Which fonts are missing a program was
/// decided by [`crate::fontinfo`] and is used as given; this only answers
/// *which objects would have to be written and what is already in them*.
fn locate<'a>(graph: &DocumentView<'_>, record: &'a FontRecord) -> Located<'a> {
    let mut out = Located {
        record,
        descriptor_id: None,
        descriptor_inline: false,
        descriptor_readable: false,
        has_widths: false,
        has_encoding: false,
        std14: record
            .base_font
            .as_deref()
            .map(|b| split_subset_tag(b).1)
            .and_then(fontdata::std14_by_base_font),
    };
    let Some(font_id) = record.id else {
        return out;
    };
    let Some(font_dict) = graph.value(font_id).and_then(Object::as_dict).cloned() else {
        return out;
    };
    out.has_widths = font_dict.contains_key(b"Widths");
    out.has_encoding = font_dict.contains_key(b"Encoding");

    // §9.8.1 — a composite font's descriptor hangs off its descendant. The
    // lookup is correct rather than convenient even though a composite font
    // is never a target, so a future classifier change cannot silently start
    // reading the wrong dictionary.
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
    out.descriptor_inline = out.descriptor_id.is_none() && !record.subtype.is_composite();
    out.descriptor_readable = graph.resolve(entry).as_dict().is_some();
    out
}

/// The descendant CIDFont dictionary of a `Type0` font (§9.7.6 Table 121).
///
/// Accepts both the conforming one-element array and the bare dictionary
/// some producers write, for the same reason [`crate::fontinfo`]'s
/// equivalent does.
fn descendant_dict(graph: &DocumentView<'_>, font_dict: &Dict) -> Option<Dict> {
    let entry = font_dict.get(b"DescendantFonts")?;
    match graph.resolve(entry) {
        Object::Array(items) => graph.resolve(items.first()?).as_dict().cloned(),
        Object::Dict(d) => Some(d.clone()),
        _ => None,
    }
}

/// Whether `name` selects `record` — `/BaseFont` verbatim or the de-prefixed
/// family name, so both `ABCDEF+Arial` and `Arial` work.
///
/// Case-**sensitive**: a PostScript font name is case-significant and
/// case-folding `Arial` onto `ARIAL` would be pdfcer deciding two different
/// names are one.
fn matches_name(record: &FontRecord, name: &str) -> bool {
    let Some(base) = record.base_font.as_deref() else {
        return false;
    };
    base == name || split_subset_tag(base).1 == name
}

/// Build the plan for `request` over `inventory`.
///
/// Pure: reads the graph, allocates a plan, changes nothing. The committing
/// side ([`EditSession::embed_fonts`](crate::edit::EditSession::embed_fonts))
/// calls exactly this function and then executes what it returns, so the
/// preview and the commit cannot diverge.
///
/// # The order of the work, which is the argument
///
/// 1. **Locate** every font's descriptor and dictionary facts — for all
///    fonts, not only the selected ones, because descriptor sharing is only
///    visible across the whole set.
/// 2. **Select** by verdict / name / identity.
/// 3. **Classify** each selection into a target or a named refusal. This is
///    where §9.9 Table 126's admissibility, the `fsType` licence check, the
///    symbolic guard and the metric-source question are all decided.
/// 4. **Sharing** — a descriptor reached by a font outside the operation
///    blocks that target outright.
///
/// # Never fails
///
/// Every fault is data in the plan, for the same reason
/// [`crate::fontinfo::inventory`] is infallible: refusing the whole
/// operation over one damaged font would cost the operator every undamaged
/// one.
#[must_use]
pub fn plan(
    view: &DocumentView<'_>,
    inventory: &FontInventory,
    request: &EmbedRequest,
) -> EmbedPlan {
    // ---- 1. locate ------------------------------------------------------
    let located: Vec<Located<'_>> = inventory
        .fonts
        .iter()
        .map(|record| locate(view, record))
        .collect();

    let mut descriptor_users: BTreeMap<ObjId, Vec<ObjId>> = BTreeMap::new();
    for l in &located {
        let Some(font_id) = l.record.id else { continue };
        if let Some(d) = l.descriptor_id {
            descriptor_users.entry(d).or_default().push(font_id);
        }
    }

    let missing_before = inventory
        .fonts
        .iter()
        .filter(|f| matches!(f.program, Program::NotEmbedded))
        .count();

    // ---- 2. select ------------------------------------------------------
    let mut unmatched: Vec<String> = Vec::new();
    let selected: BTreeSet<usize> = match &request.selection {
        EmbedSelection::AllMissing => located
            .iter()
            .enumerate()
            .filter(|(_, l)| matches!(l.record.program, Program::NotEmbedded))
            .map(|(i, _)| i)
            .collect(),
        EmbedSelection::Objects(ids) => {
            let wanted: BTreeSet<ObjId> = ids.iter().copied().collect();
            let hit: BTreeSet<usize> = located
                .iter()
                .enumerate()
                .filter(|(_, l)| l.record.id.is_some_and(|id| wanted.contains(&id)))
                .map(|(i, _)| i)
                .collect();
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
        EmbedSelection::Named(names) => {
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

    // ---- 3. classify ----------------------------------------------------
    let mut targets: Vec<EmbedTarget> = Vec::new();
    let mut blocked: Vec<EmbedBlocked> = Vec::new();
    let mut target_ids: BTreeSet<ObjId> = BTreeSet::new();

    for (i, l) in located.iter().enumerate() {
        let record = l.record;
        if !selected.contains(&i) {
            // Not asked for. Reported as blocked only under "everything
            // missing" — under an explicit selection a font the operator did
            // not name is not a refusal, and listing it as one would bury the
            // fonts that ARE refusals.
            if matches!(request.selection, EmbedSelection::AllMissing) {
                blocked.push(EmbedBlocked {
                    id: record.id,
                    base_font: record.base_font.clone(),
                    blocker: blocker_for_unselected(record),
                    // Unreachable in practice — under `AllMissing` every
                    // `NotEmbedded` font IS selected — but derived from the
                    // record rather than hard-coded `false`, so the field
                    // stays correct if the selection rule ever changes.
                    missing_program: matches!(record.program, Program::NotEmbedded),
                });
            }
            continue;
        }
        match classify(view, l, request) {
            Ok(target) => {
                target_ids.insert(target.id);
                targets.push(target);
            }
            Err(blocker) => blocked.push(EmbedBlocked {
                id: record.id,
                base_font: record.base_font.clone(),
                blocker,
                missing_program: matches!(record.program, Program::NotEmbedded),
            }),
        }
    }

    // ---- 4. descriptor sharing ------------------------------------------
    // ★ ANY other font reaching the descriptor blocks the target — not only
    // one outside the operation, which is where this diverges from
    // [`crate::font_unembed`]'s otherwise identical census.
    //
    // Unembedding two fonts that share a descriptor is idempotent: both
    // remove the same `/FontFile*` key and the second finds it gone.
    // Embedding is not. Two font dictionaries with different `/BaseFont`
    // values resolve to different donors, and writing both into one shared
    // descriptor would leave whichever ran last, silently giving one font
    // the other's letterforms. So sharing is blocked outright here, and the
    // asymmetry is deliberate rather than an oversight in the mirror.
    let mut i = 0usize;
    while i < targets.len() {
        let Some(target) = targets.get(i) else { break };
        let outsiders: Vec<ObjId> = target
            .descriptor_id
            .and_then(|d| descriptor_users.get(&d))
            .map(|users| users.iter().copied().filter(|u| *u != target.id).collect())
            .unwrap_or_default();
        if outsiders.is_empty() {
            i += 1;
            continue;
        }
        let t = targets.remove(i);
        target_ids.remove(&t.id);
        blocked.push(EmbedBlocked {
            id: Some(t.id),
            base_font: t.base_font,
            blocker: EmbedBlocker::DescriptorShared { with: outsiders },
            // It was a target a moment ago, and `classify` only ever
            // produces one from a `NotEmbedded` font.
            missing_program: true,
        });
    }

    // Stable output order: by object number, so two runs over one document
    // produce identical reports and a diff of two reports is meaningful.
    targets.sort_by_key(|t| (t.id.num, t.id.generation));
    blocked.sort_by_key(|b| b.id.map_or((u32::MAX, 0), |id| (id.num, id.generation)));

    EmbedPlan {
        targets,
        blocked,
        unmatched,
        pdfa: crate::font_unembed::detect_pdfa(view),
        coverage: inventory.coverage,
        missing_before,
    }
}

/// Why a font that was not selected is nevertheless listed.
///
/// Under [`EmbedSelection::AllMissing`] every other font in the document is
/// reported, so the list answers "and what about this one?" for every row a
/// Fonts panel shows.
fn blocker_for_unselected(record: &FontRecord) -> EmbedBlocker {
    match &record.program {
        Program::Embedded(_) => EmbedBlocker::AlreadyEmbedded,
        Program::Unreadable { .. } => EmbedBlocker::ProgramDeclaredButUnreadable,
        // `Program` is `#[non_exhaustive]`; a state this build does not know
        // is not silently treated as embeddable.
        _ => EmbedBlocker::ProgramDeclaredButUnreadable,
    }
}

/// Decide whether one selected font can gain a program, and how.
///
/// The whole conformance argument lives here, in the order the checks have
/// to run: what the FONT is, then whether a DONOR exists, then whether the
/// donor may be embedded at all, then whether §9.9 Table 126 admits the
/// pairing, and only then how much of the dictionary has to be written.
fn classify(
    view: &DocumentView<'_>,
    l: &Located<'_>,
    request: &EmbedRequest,
) -> Result<EmbedTarget, EmbedBlocker> {
    let record = l.record;

    // -- what the font is -------------------------------------------------
    if !matches!(record.program, Program::NotEmbedded) {
        return Err(blocker_for_unselected(record));
    }
    if matches!(record.subtype, FontSubtype::Type3) {
        return Err(EmbedBlocker::Type3);
    }
    if record.subtype.is_composite() {
        return Err(EmbedBlocker::Composite {
            identity: record.encoding.is_identity(),
        });
    }
    let Some(font_id) = record.id else {
        return Err(EmbedBlocker::FontNotIndirect);
    };
    let Some(base_font) = record.base_font.clone() else {
        // No `/BaseFont` at all: nothing to resolve a donor against, and no
        // standard-14 identity. §9.6.2.1 Table 111 makes `/BaseFont`
        // required, so this is a malformation.
        return Err(EmbedBlocker::NoSourceFont);
    };

    // -- is there a donor -------------------------------------------------
    let Some(donor) = request.supplied.get(&base_font) else {
        return Err(EmbedBlocker::NoSourceFont);
    };
    if donor.program.len() > MAX_DONOR_BYTES {
        return Err(EmbedBlocker::ProgramTooLarge {
            bytes: donor.program.len(),
        });
    }
    let format = ProgramFormat::sniff(&donor.program);
    match format {
        ProgramFormat::Collection => return Err(EmbedBlocker::ProgramIsCollection),
        ProgramFormat::Unrecognised => return Err(EmbedBlocker::ProgramUnrecognised),
        ProgramFormat::Type1 => {
            // A bare Type 1 program needs Table 127's three-part
            // `Length1`/`Length2`/`Length3` split, which means finding the
            // `eexec` boundary and the fixed-content tail. pdfcer does not
            // compute that yet, and writing the key with wrong lengths
            // produces a font no reader can decrypt. Refused by name rather
            // than emitted wrong.
            return Err(EmbedBlocker::FormatNotAdmissible {
                subtype: record.subtype.clone(),
                format,
            });
        }
        _ => {}
    }

    // -- may this donor be embedded at all (§9.9's opening paragraph) -----
    let permission = match crate::fontinfo::read_fs_type(&donor.program) {
        Ok(bits) => match bits.permission {
            EmbeddingPermission::Restricted | EmbeddingPermission::Ambiguous => {
                return Err(EmbedBlocker::EmbeddingForbidden {
                    permission: bits.permission,
                    raw: bits.raw,
                });
            }
            p => Some(p),
        },
        // A bare CFF or a Type 1 program has no `OS/2` table and therefore no
        // `fsType`; that is "this format carries no such field", not
        // "permission denied". Every other read failure is a measurement
        // failure and, like the absent case, is not a permission — but it is
        // also not a prohibition, and §9.9 E1 is a `should` rather than a
        // `shall`, so it does not gate.
        Err(FsTypeError::Collection) => return Err(EmbedBlocker::ProgramIsCollection),
        Err(_) => None,
    };

    // `symbolic` is Table 123 bit 3 as the descriptor declares it; the two
    // standard-14 symbolic faces declare it through their compiled
    // descriptor rather than through a descriptor in the file. The guard it
    // feeds fires further down, once it is known HOW codes will be mapped.
    let symbolic = record.symbolic.unwrap_or(false)
        || matches!(l.std14, Some(Std14::Symbol | Std14::ZapfDingbats));

    // -- which shape -------------------------------------------------------
    let shape = if l.has_widths && l.descriptor_readable {
        EmbedShape::Attach
    } else if l.std14.is_some() {
        EmbedShape::Synthesise
    } else {
        return Err(EmbedBlocker::NoMetricSource);
    };
    if shape == EmbedShape::Attach && l.descriptor_id.is_none() && !l.descriptor_inline {
        return Err(EmbedBlocker::DescriptorUnreadable);
    }

    // -- §9.9 Table 126: which key, and is a re-declaration needed --------
    let (program_key, stream_subtype, redeclared_truetype) =
        admissible_key(&record.subtype, format, shape, &donor.program)?;

    // The re-declaration to `/TrueType` needs §9.6.6.4 Branch A, which needs
    // an encoding writable as AGL-resolvable glyph names.
    if redeclared_truetype && !l.has_encoding {
        let enc = l
            .std14
            .map(fontdata::std14_builtin_encoding)
            .unwrap_or(BaseEncoding::Standard);
        if !encoding_is_spellable(enc) {
            return Err(EmbedBlocker::EncodingNotSpellable);
        }
    }

    // -- how much of the dictionary is written ----------------------------
    let (widths_written, encoding_written, descriptor_written) = match shape {
        EmbedShape::Attach => (0usize, false, false),
        EmbedShape::Synthesise => {
            let std14 = l.std14.ok_or(EmbedBlocker::NoMetricSource)?;
            let widths = synth_widths(view, font_id, std14).ok_or(EmbedBlocker::NoMetricSource)?;
            (widths.1.len(), !l.has_encoding, !l.descriptor_readable)
        }
    };

    // -- ★ the symbolic guard, and why it is HERE rather than earlier -----
    //
    // The hazard a symbolic font poses is not "the letters look different".
    // It is that under §9.6.6.4 Branch B the character codes are looked up in
    // **the program's own `(3,0)` cmap**, so a face the document never named
    // draws whatever glyph happens to sit at that code — the wrong symbol,
    // silently, with nothing on the page to say so.
    //
    // That hazard exists exactly when the mapping runs through the program.
    // It does not exist when the mapping runs through **glyph names**: §9.6.6.2
    // sends a Type 1-flavour font's codes to an `/Encoding` table and then to
    // the program's charset BY NAME, so a donor that lacks the named glyph
    // draws `.notdef` — a visible hole, not a plausible-looking wrong symbol.
    //
    // So the guard is written against the mapping pdfcer is about to produce,
    // and it can only be evaluated once the key and the encoding are decided:
    //
    // - the dictionary stays Type 1-flavour (no `/TrueType` re-declaration),
    //   so glyph selection is by name, AND
    // - an explicit name table exists — either the file already carried one,
    //   or pdfcer is writing one out of its compiled Annex D.5/D.6 tables.
    //
    // This is what lets the two standard-14 symbolic faces be embedded at
    // all. Measured: `Symbol` and `ZapfDingbats` are **248 of the corpus's
    // 1,534 non-embedded fonts (16 %)**, essentially all of them bare
    // dictionaries with no `/Encoding` — precisely the case pdfcer pins from
    // Annex D and can therefore serve.
    let name_mapped = !redeclared_truetype
        && matches!(record.subtype, FontSubtype::Type1 | FontSubtype::MmType1)
        && (l.has_encoding || encoding_written);
    if symbolic && donor.matched.is_substitute() && !name_mapped {
        return Err(EmbedBlocker::SymbolicSubstitute {
            matched: donor.matched,
        });
    }

    let (tag, family) = split_subset_tag(&base_font);
    let rename = tag
        .and(Some(family))
        .filter(|f| !f.is_empty())
        .map(str::to_owned);

    Ok(EmbedTarget {
        id: font_id,
        base_font: Some(base_font),
        shape,
        descriptor_id: l.descriptor_id,
        program_key,
        stream_subtype,
        format,
        face_name: donor.face_name.clone(),
        source: donor.source.clone(),
        matched: donor.matched,
        program_bytes: donor.program.len(),
        permission,
        rename,
        redeclared_truetype,
        widths_written,
        encoding_written,
        descriptor_written,
        pages: record.pages.clone(),
    })
}

/// Which `/FontFile*` key §9.9 Table 126 admits for this font dictionary and
/// this program format, and whether reaching it needs a `/Subtype`
/// re-declaration.
///
/// Returns `(key, stream /Subtype, redeclared)`. Table 127 makes the stream
/// `/Subtype` **required** for `/FontFile3` and forbids `Length1/2/3` there;
/// `/FontFile2` is the mirror (a `/Length1` and no `/Subtype`).
///
/// # The re-declaration, restated where it is enforced
///
/// A `glyf` donor under a `Type1` dictionary has no admissible key. It is
/// reachable only by re-declaring the dictionary as `/TrueType`, and only
/// inside [`EmbedShape::Synthesise`], where pdfcer is already authoring the
/// descriptor, the widths and the encoding from its own data — so the
/// re-declared font is one pdfcer fully describes rather than one it has
/// half-rewritten. Under [`EmbedShape::Attach`] the promise is "one key
/// added, nothing else changed", and a subtype change would break it.
fn admissible_key(
    subtype: &FontSubtype,
    format: ProgramFormat,
    shape: EmbedShape,
    program: &[u8],
) -> Result<(ProgramKey, Option<&'static str>, bool), EmbedBlocker> {
    let not_admissible = || {
        Err(EmbedBlocker::FormatNotAdmissible {
            subtype: subtype.clone(),
            format,
        })
    };
    match (subtype, format) {
        // A TrueType dictionary takes a `glyf` program directly (Table 126,
        // `FontFile2`). §9.9 T2 additionally requires the `cmap` a simple
        // font's §9.6.6.4 mapping goes through.
        (&FontSubtype::TrueType, ProgramFormat::TrueTypeGlyf) => {
            if !sfnt_has_cmap(program) {
                return Err(EmbedBlocker::OpenTypeMissingCmap);
            }
            Ok((ProgramKey::FontFile2, None, false))
        }
        // A Type 1 dictionary takes a bare CFF (Table 126, `Type1C`).
        (&FontSubtype::Type1 | &FontSubtype::MmType1, ProgramFormat::BareCff) => {
            Ok((ProgramKey::FontFile3, Some("Type1C"), false))
        }
        // Table 126 case OT-3: an OpenType program whose outlines are CFF,
        // under a Type 1 dictionary. "In addition to the CFF table, the font
        // program must include the cmap table."
        (&FontSubtype::Type1 | &FontSubtype::MmType1, ProgramFormat::OpenTypeCff) => {
            if !sfnt_has_cmap(program) {
                return Err(EmbedBlocker::OpenTypeMissingCmap);
            }
            Ok((ProgramKey::FontFile3, Some("OpenType"), false))
        }
        // The re-declaration. See this function's docs and the module docs.
        (&FontSubtype::Type1 | &FontSubtype::MmType1, ProgramFormat::TrueTypeGlyf) => {
            if shape != EmbedShape::Synthesise {
                return not_admissible();
            }
            if !sfnt_has_cmap(program) {
                return Err(EmbedBlocker::OpenTypeMissingCmap);
            }
            Ok((ProgramKey::FontFile2, None, true))
        }
        _ => not_admissible(),
    }
}

/// Whether `enc`'s glyph names can be written into a `/Differences` array a
/// §9.6.6.4 Branch A reader can resolve.
///
/// Branch A goes glyph name → Unicode (through the Adobe Glyph List) → GID
/// (through the `(3,1)` cmap). The three Latin encodings name glyphs the AGL
/// carries. `Symbol` and `ZapfDingbats` name glyphs (`Alpha`, `a1`) it does
/// not, so a TrueType font could not complete the lookup — which is why the
/// re-declaration refuses them by name rather than writing an array no
/// reader can use.
const fn encoding_is_spellable(enc: BaseEncoding) -> bool {
    matches!(
        enc,
        BaseEncoding::Standard | BaseEncoding::WinAnsi | BaseEncoding::MacRoman
    )
}

/// Compute `/FirstChar`, `/LastChar` and `/Widths` for a standard-14 font
/// that carries none.
///
/// Returns `(first_char, widths)`, or `None` when not one code in the
/// font's resolved encoding has a width — which would mean the compiled AFM
/// table and the resolved encoding disagree completely, and writing an empty
/// `/Widths` would be worse than refusing.
///
/// # Why the encoding comes from the extraction resolver
///
/// [`crate::text_extract::font::ExtractFont`] is the one implementation in
/// pdfcer of §9.6.6's base-encoding-plus-`/Differences` chain, including the
/// standard-14 built-in fallback (R171: one owner, never restated). Widths
/// computed against a *different* code→name table than extraction uses would
/// let a document be searched for text that is not what was painted.
///
/// # Why unencoded codes inside the range are written as zero
///
/// `/Widths` is a dense array from `/FirstChar` to `/LastChar` (§9.6.2.1
/// Table 111), so a gap has to hold something. A code the encoding does not
/// assign draws `.notdef`, whose advance is `/MissingWidth` — which the
/// synthesised descriptor sets to 0, matching what the standard-14 AFMs
/// imply for an unencoded code. Zero is therefore the value that agrees with
/// what a reader was already doing, not a filler.
fn synth_widths(view: &DocumentView<'_>, font_id: ObjId, std14: Std14) -> Option<(u8, Vec<i64>)> {
    let font_dict = view.value(font_id).and_then(Object::as_dict)?.clone();
    let resolved = crate::text_extract::font::ExtractFont::resolve(view, &font_dict);
    let names = resolved.glyph_names()?;

    let width_at = |code: usize| -> Option<u16> {
        names
            .get(code)
            .and_then(Option::as_ref)
            .and_then(|n| fontdata::std14_width(std14, n))
    };
    let first = (0usize..=255).find(|c| width_at(*c).is_some())?;
    let last = (0usize..=255).rev().find(|c| width_at(*c).is_some())?;
    let widths: Vec<i64> = (first..=last)
        .map(|c| i64::from(width_at(c).unwrap_or(0)))
        .collect();
    Some((u8::try_from(first).ok()?, widths))
}

/// Build the `/Encoding` dictionary that spells out `enc` as a
/// `/Differences` array.
///
/// Written in the compact run form §9.6.6.1 defines — a starting code
/// followed by consecutive glyph names — so a 149-code encoding costs one
/// integer per run rather than one per code.
///
/// No `/BaseEncoding` is written. Naming one would only be possible for
/// WinAnsi/MacRoman/MacExpert (Table 114 lists no `StandardEncoding`), and
/// naming a *different* base than the one being spelled out would leave two
/// disagreeing statements in the same dictionary.
fn encoding_dict(enc: BaseEncoding) -> Dict {
    let mut differences: Vec<Object> = Vec::new();
    let mut expect_next: Option<u16> = None;
    for code in 0u16..=255 {
        let Some(name) = u8::try_from(code)
            .ok()
            .and_then(|c| fontdata::encoding_glyph_name(enc, c))
        else {
            expect_next = None;
            continue;
        };
        if expect_next != Some(code) {
            differences.push(Object::Integer(i64::from(code)));
        }
        differences.push(Object::Name(Name(name.as_bytes().to_vec())));
        expect_next = Some(code.saturating_add(1));
    }
    let mut dict = Dict::new();
    dict.insert(Name::from(b"Type"), Object::Name(Name::from(b"Encoding")));
    dict.insert(Name::from(b"Differences"), Object::Array(differences));
    dict
}

/// Build the `/FontDescriptor` for a standard-14 face pdfcer is describing
/// from its own compiled data (§9.8.1 Table 122).
///
/// `flags` comes from [`crate::fontdata::std14_descriptor`], which derives
/// Table 123's bits rather than reading them from an AFM — including the
/// `Nonsymbolic` bit that puts a re-declared TrueType font on §9.6.6.4
/// Branch A.
///
/// `/MissingWidth` is written explicitly as 0 so the zeros
/// [`synth_widths`] writes for unencoded codes agree with a stated default
/// rather than with Table 122's implicit one.
fn synth_descriptor(name: &str, std14: Std14) -> Dict {
    let m = fontdata::std14_descriptor(std14);
    let mut d = Dict::new();
    d.insert(
        Name::from(b"Type"),
        Object::Name(Name::from(b"FontDescriptor")),
    );
    d.insert(
        Name::from(b"FontName"),
        Object::Name(Name(name.as_bytes().to_vec())),
    );
    d.insert(Name::from(b"Flags"), Object::Integer(i64::from(m.flags)));
    d.insert(
        Name::from(b"FontBBox"),
        Object::Array(
            m.font_bbox
                .iter()
                .map(|v| Object::Integer(i64::from(*v)))
                .collect(),
        ),
    );
    d.insert(
        Name::from(b"ItalicAngle"),
        Object::Real(f64::from(m.italic_angle)),
    );
    d.insert(
        Name::from(b"Ascent"),
        Object::Integer(i64::from(m.ascender)),
    );
    d.insert(
        Name::from(b"Descent"),
        Object::Integer(i64::from(m.descender)),
    );
    d.insert(
        Name::from(b"CapHeight"),
        Object::Integer(i64::from(m.cap_height)),
    );
    d.insert(Name::from(b"StemV"), Object::Integer(i64::from(m.stem_v)));
    if m.x_height != 0 {
        d.insert(
            Name::from(b"XHeight"),
            Object::Integer(i64::from(m.x_height)),
        );
    }
    d.insert(Name::from(b"MissingWidth"), Object::Integer(0));
    d
}

// ---------------------------------------------------------------------------
// The edits, written once so the session and the tests share them
// ---------------------------------------------------------------------------

/// Everything one target writes, computed from the plan and the document.
///
/// Separated from the session so the exact set of keys that move is written
/// once and can be tested without driving a save.
pub(crate) struct TargetEdits {
    /// The font dictionary as it will be written, or `None` when it does not
    /// change (the [`EmbedShape::Attach`] case with no rename).
    pub(crate) font_dict: Option<Dict>,
    /// The descriptor as it will be written, and whether it is a NEW object.
    /// `None` when the descriptor is inline in the font dictionary (in which
    /// case `font_dict` already carries it).
    pub(crate) descriptor: Option<Dict>,
    /// The program stream's dictionary; the caller stages the bytes and adds
    /// `/Length`.
    pub(crate) stream_dict: Dict,
}

/// Compute the dictionary edits for one target.
///
/// `program_id` is the object number the caller has reserved for the font
/// program stream, and `descriptor_id` the one reserved for a synthesised
/// descriptor (unused when the target already has one).
///
/// Returns `None` when the font dictionary cannot be read, which the plan's
/// own resolution makes unreachable in practice but which is handled rather
/// than unwrapped.
pub(crate) fn target_edits(
    view: &DocumentView<'_>,
    current_font: &Dict,
    current_descriptor: Option<&Dict>,
    target: &EmbedTarget,
    program_id: ObjId,
    new_descriptor_id: ObjId,
    compressed_len: usize,
) -> Option<TargetEdits> {
    let mut font = current_font.clone();
    let mut font_changed = false;

    // The name the font ends up with. A full face is being embedded, so a
    // §9.6.4 tag asserting a subset would be false — and Table 122 makes
    // `/FontName` follow `/BaseFont` by `shall`.
    let final_name = match &target.rename {
        Some(new_name) => {
            font.insert(
                Name::from(b"BaseFont"),
                Object::Name(Name(new_name.as_bytes().to_vec())),
            );
            font_changed = true;
            new_name.clone()
        }
        None => target.base_font.clone().unwrap_or_default(),
    };

    if target.redeclared_truetype {
        font.insert(
            Name::from(b"Subtype"),
            Object::Name(Name::from(b"TrueType")),
        );
        font_changed = true;
    }

    // Synthesised metrics.
    if target.shape == EmbedShape::Synthesise {
        let std14 = target
            .base_font
            .as_deref()
            .map(|b| split_subset_tag(b).1)
            .and_then(fontdata::std14_by_base_font)?;
        let (first, widths) = synth_widths(view, target.id, std14)?;
        let last = usize::from(first).checked_add(widths.len().checked_sub(1)?)?;
        font.insert(Name::from(b"FirstChar"), Object::Integer(i64::from(first)));
        font.insert(
            Name::from(b"LastChar"),
            Object::Integer(i64::try_from(last).ok()?),
        );
        font.insert(
            Name::from(b"Widths"),
            Object::Array(widths.into_iter().map(Object::Integer).collect()),
        );
        if target.encoding_written {
            font.insert(
                Name::from(b"Encoding"),
                Object::Dict(encoding_dict(fontdata::std14_builtin_encoding(std14))),
            );
        }
        font_changed = true;
    }

    // The descriptor: an existing one gains a key, or a new one is authored.
    let mut descriptor = match current_descriptor {
        Some(d) => d.clone(),
        None => {
            let std14 = target
                .base_font
                .as_deref()
                .map(|b| split_subset_tag(b).1)
                .and_then(fontdata::std14_by_base_font)?;
            synth_descriptor(&final_name, std14)
        }
    };
    // Table 122's `shall`: `/FontName` equals `/BaseFont`. A rename moves
    // both or the cleanup introduces a conformance defect.
    if target.rename.is_some() && descriptor.contains_key(b"FontName") {
        descriptor.insert(
            Name::from(b"FontName"),
            Object::Name(Name(final_name.as_bytes().to_vec())),
        );
    }
    descriptor.insert(
        Name::from(target.program_key.label().as_bytes()),
        Object::Reference(program_id),
    );

    let descriptor_inline = current_descriptor.is_some() && target.descriptor_id.is_none();
    if descriptor_inline {
        font.insert(
            Name::from(b"FontDescriptor"),
            Object::Dict(descriptor.clone()),
        );
        font_changed = true;
    } else if target.descriptor_written {
        font.insert(
            Name::from(b"FontDescriptor"),
            Object::Reference(new_descriptor_id),
        );
        font_changed = true;
    }

    // The program stream dictionary (§9.9 Table 127).
    let mut stream_dict = Dict::new();
    stream_dict.insert(
        Name::from(b"Length"),
        Object::Integer(i64::try_from(compressed_len).unwrap_or(i64::MAX)),
    );
    stream_dict.insert(
        Name::from(b"Filter"),
        Object::Name(Name::from(b"FlateDecode")),
    );
    match target.stream_subtype {
        // `/FontFile3`: `/Subtype` is REQUIRED and `Length1/2/3` "shall not
        // be present".
        Some(subtype) => {
            stream_dict.insert(
                Name::from(b"Subtype"),
                Object::Name(Name(subtype.as_bytes().to_vec())),
            );
        }
        // `/FontFile2`: `/Length1` is "the entire TrueType font program,
        // after it has been decoded using the filters specified by the
        // stream's Filter entry" — the DECODED length, not `/Length`.
        None => {
            stream_dict.insert(
                Name::from(b"Length1"),
                Object::Integer(i64::try_from(target.program_bytes).unwrap_or(i64::MAX)),
            );
        }
    }

    Some(TargetEdits {
        font_dict: font_changed.then_some(font),
        descriptor: (!descriptor_inline).then_some(descriptor),
        stream_dict,
    })
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
    fn sniff_recognises_every_framing_it_claims_to() {
        assert_eq!(ProgramFormat::sniff(&[]), ProgramFormat::Unrecognised);
        assert_eq!(
            ProgramFormat::sniff(b"%PDF-1.7"),
            ProgramFormat::Unrecognised
        );
        assert_eq!(
            ProgramFormat::sniff(&[0x01, 0x00, 4, 1]),
            ProgramFormat::BareCff
        );
        assert_eq!(
            ProgramFormat::sniff(&[0x80, 0x01, 0, 0]),
            ProgramFormat::Type1
        );
        assert_eq!(
            ProgramFormat::sniff(b"%!PS-AdobeFont"),
            ProgramFormat::Type1
        );
        assert_eq!(
            ProgramFormat::sniff(b"ttcf\0\x01\0\0"),
            ProgramFormat::Collection
        );
    }

    /// A truncated sfnt must not panic and must not be called a font. The
    /// donor is operator-supplied and therefore untrusted input.
    #[test]
    fn a_truncated_sfnt_is_unrecognised_rather_than_a_panic() {
        for len in 0..40usize {
            let mut bytes = vec![0x00, 0x01, 0x00, 0x00, 0x00, 0x40];
            bytes.resize(len, 0u8);
            let _ = ProgramFormat::sniff(&bytes);
            let _ = sfnt_has_cmap(&bytes);
        }
        // A directory claiming 64 tables in a 6-byte file.
        assert_eq!(
            ProgramFormat::sniff(&[0x00, 0x01, 0x00, 0x00, 0x00, 0x40]),
            ProgramFormat::Unrecognised
        );
    }

    /// §9.9 Table 126, exhaustively, in both directions: every admissible
    /// pairing yields the key the table names, and every other pairing
    /// refuses.
    #[test]
    fn table_126_admissibility_is_exact() {
        // A minimal sfnt directory with `glyf` and `cmap`, enough for the
        // table walk. Two records: `cmap` then `glyf`.
        let mut ttf = vec![0x00, 0x01, 0x00, 0x00, 0x00, 0x02, 0, 0, 0, 0, 0, 0];
        ttf.extend_from_slice(b"cmap");
        ttf.extend_from_slice(&[0; 12]);
        ttf.extend_from_slice(b"glyf");
        ttf.extend_from_slice(&[0; 12]);
        assert_eq!(ProgramFormat::sniff(&ttf), ProgramFormat::TrueTypeGlyf);
        assert!(sfnt_has_cmap(&ttf));

        let cff = [0x01u8, 0x00, 0x04, 0x01];

        // Admissible.
        assert_eq!(
            admissible_key(
                &FontSubtype::TrueType,
                ProgramFormat::TrueTypeGlyf,
                EmbedShape::Attach,
                &ttf
            )
            .unwrap(),
            (ProgramKey::FontFile2, None, false)
        );
        assert_eq!(
            admissible_key(
                &FontSubtype::Type1,
                ProgramFormat::BareCff,
                EmbedShape::Attach,
                &cff
            )
            .unwrap(),
            (ProgramKey::FontFile3, Some("Type1C"), false)
        );
        // The re-declaration: allowed ONLY under Synthesise.
        assert_eq!(
            admissible_key(
                &FontSubtype::Type1,
                ProgramFormat::TrueTypeGlyf,
                EmbedShape::Synthesise,
                &ttf
            )
            .unwrap(),
            (ProgramKey::FontFile2, None, true)
        );
        assert!(
            admissible_key(
                &FontSubtype::Type1,
                ProgramFormat::TrueTypeGlyf,
                EmbedShape::Attach,
                &ttf
            )
            .is_err(),
            "Attach promises 'one key added, nothing else changed'; a subtype re-declaration \
             breaks that promise and must not be reachable from it"
        );
        // Not admissible: a CFF face under a TrueType dictionary. Table
        // 126's OT-1 case is `glyf`-only.
        assert!(
            admissible_key(
                &FontSubtype::TrueType,
                ProgramFormat::OpenTypeCff,
                EmbedShape::Synthesise,
                &cff
            )
            .is_err()
        );
    }

    /// A TrueType program with no `cmap` cannot serve a SIMPLE font: §9.9's
    /// T2 puts a `shall` on it, because §9.6.6.4's whole mapping goes
    /// through it.
    #[test]
    fn a_truetype_donor_without_a_cmap_is_refused() {
        let mut ttf = vec![0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0, 0, 0, 0, 0, 0];
        ttf.extend_from_slice(b"glyf");
        ttf.extend_from_slice(&[0; 12]);
        assert_eq!(ProgramFormat::sniff(&ttf), ProgramFormat::TrueTypeGlyf);
        assert!(!sfnt_has_cmap(&ttf));
        assert_eq!(
            admissible_key(
                &FontSubtype::TrueType,
                ProgramFormat::TrueTypeGlyf,
                EmbedShape::Attach,
                &ttf
            )
            .unwrap_err(),
            EmbedBlocker::OpenTypeMissingCmap
        );
    }

    /// The two symbolic standard-14 faces cannot be re-declared as TrueType:
    /// their glyph names are not in the AGL, so §9.6.6.4 Branch A could not
    /// complete.
    #[test]
    fn only_the_latin_encodings_are_spellable() {
        assert!(encoding_is_spellable(BaseEncoding::Standard));
        assert!(encoding_is_spellable(BaseEncoding::WinAnsi));
        assert!(encoding_is_spellable(BaseEncoding::MacRoman));
        assert!(!encoding_is_spellable(BaseEncoding::Symbol));
        assert!(!encoding_is_spellable(BaseEncoding::ZapfDingbats));
    }

    /// The `/Differences` array must reproduce the encoding exactly — every
    /// assigned code, no unassigned one, and the run form must not shift a
    /// name onto the wrong code.
    #[test]
    fn the_differences_array_reproduces_the_encoding_code_for_code() {
        for enc in [
            BaseEncoding::Standard,
            BaseEncoding::WinAnsi,
            BaseEncoding::MacRoman,
            BaseEncoding::Symbol,
            BaseEncoding::ZapfDingbats,
        ] {
            let dict = encoding_dict(enc);
            let Some(Object::Array(items)) = dict.get(b"Differences") else {
                panic!("no /Differences");
            };
            // Replay the array the way a reader does and compare.
            let mut seen: Vec<Option<String>> = vec![None; 256];
            let mut code = 0usize;
            for item in items {
                match item {
                    Object::Integer(n) => code = usize::try_from(*n).unwrap(),
                    Object::Name(n) => {
                        seen[code] = Some(String::from_utf8_lossy(&n.0).into_owned());
                        code += 1;
                    }
                    other => panic!("unexpected {other:?}"),
                }
            }
            for c in 0u16..=255 {
                let want = fontdata::encoding_glyph_name(enc, u8::try_from(c).unwrap());
                assert_eq!(
                    seen[usize::from(c)].as_deref(),
                    want,
                    "{enc:?} code {c} disagrees"
                );
            }
        }
    }

    /// A refusal that only says "no" is a dead end (R27). Every blocker has
    /// to name what would satisfy it, or say plainly that nothing would.
    #[test]
    fn every_blocker_has_a_distinct_token_and_a_non_empty_reason() {
        let all = [
            EmbedBlocker::AlreadyEmbedded,
            EmbedBlocker::ProgramDeclaredButUnreadable,
            EmbedBlocker::NoSourceFont,
            EmbedBlocker::Composite { identity: true },
            EmbedBlocker::Type3,
            EmbedBlocker::NoMetricSource,
            EmbedBlocker::ProgramUnrecognised,
            EmbedBlocker::ProgramIsCollection,
            EmbedBlocker::FormatNotAdmissible {
                subtype: FontSubtype::Type1,
                format: ProgramFormat::TrueTypeGlyf,
            },
            EmbedBlocker::OpenTypeMissingCmap,
            EmbedBlocker::EmbeddingForbidden {
                permission: EmbeddingPermission::Restricted,
                raw: 2,
            },
            EmbedBlocker::SymbolicSubstitute {
                matched: FontMatch::Bundled,
            },
            EmbedBlocker::EncodingNotSpellable,
            EmbedBlocker::FontNotIndirect,
            EmbedBlocker::DescriptorShared { with: Vec::new() },
            EmbedBlocker::DescriptorUnreadable,
            EmbedBlocker::ProgramTooLarge { bytes: 1 },
        ];
        let mut tokens: Vec<&str> = all.iter().map(EmbedBlocker::token).collect();
        let count = tokens.len();
        tokens.sort_unstable();
        tokens.dedup();
        assert_eq!(tokens.len(), count, "two blockers share a token");
        for b in &all {
            assert!(!b.reason().is_empty(), "{b:?} has no reason");
            assert!(
                b.reason().len() > 40,
                "{b:?}'s reason is too short to say what would fix it"
            );
        }
    }

    #[test]
    fn only_an_exact_match_is_not_a_substitute() {
        assert!(!FontMatch::Exact.is_substitute());
        assert!(FontMatch::Alias.is_substitute());
        assert!(FontMatch::Bundled.is_substitute());
    }

    // -- planning and committing, over real fixtures ----------------------

    use crate::document::Document;
    use crate::edit::EditSession;
    use crate::writer::SaveOptions;

    fn session(bytes: &[u8]) -> EditSession {
        EditSession::new(Document::from_bytes(bytes.to_vec()).expect("fixture parses"))
    }

    /// The project's own synthetic TrueType donor, `fsType` = Editable.
    const DONOR: &[u8] =
        include_bytes!("../../../fixtures/synthetic/text/subset-fstype-editable.ttf");
    /// The same shape with `fsType` = Restricted (bits 1–3 == 2), which
    /// §9.9's opening paragraph says should not be embedded.
    const DONOR_RESTRICTED: &[u8] =
        include_bytes!("../../../fixtures/synthetic/text/subset-fstype-restricted.ttf");

    fn donor(matched: FontMatch) -> SuppliedFont {
        SuppliedFont::new(DONOR.to_vec(), "pdfceDonor", "fixtures/donor.ttf", matched)
    }

    /// The 83 % case, end to end at the planning level: a bare standard-14
    /// font gains a descriptor, metrics and an encoding, and is re-declared
    /// so a `glyf` donor can lawfully attach.
    #[test]
    fn a_bare_standard_14_font_is_synthesised() {
        let s = session(include_bytes!(
            "../../../fixtures/synthetic/embed/embed-std14-bare.pdf"
        ));
        let req = EmbedRequest::all_missing().with_font("Helvetica", donor(FontMatch::Alias));
        let plan = s.embed_preview(&req);
        assert_eq!(plan.missing_before, 1);
        assert_eq!(plan.targets.len(), 1, "blocked: {:?}", plan.blocked);
        let t = &plan.targets[0];
        assert_eq!(t.shape, EmbedShape::Synthesise);
        assert!(t.descriptor_written, "§9.6.2.2 let the file omit it");
        assert!(t.encoding_written, "the dictionary carried none");
        assert!(t.redeclared_truetype, "a glyf donor under a Type1 dict");
        assert_eq!(t.program_key, ProgramKey::FontFile2);
        assert_eq!(
            t.stream_subtype, None,
            "/FontFile2 carries /Length1, not /Subtype"
        );
        assert!(t.widths_written > 90, "StandardEncoding assigns 149 codes");
        assert_eq!(t.matched, FontMatch::Alias);
        assert!(plan.substitutes_any());
        assert_eq!(
            plan.missing_after(),
            0,
            "the number the operator cares about"
        );
    }

    /// ★ The widths follow the dictionary's OWN encoding, not the
    /// standard-14 built-in one.
    ///
    /// The two fixtures differ in exactly one entry — `/Encoding
    /// /WinAnsiEncoding` — and Standard and WinAnsi disagree about more than
    /// a dozen codes. If the synthesiser read the built-in encoding
    /// unconditionally, both would produce identical arrays and the page
    /// would be mis-spaced in a way nothing else here would catch.
    #[test]
    fn an_explicit_encoding_decides_the_widths_and_is_not_overwritten() {
        let bare = session(include_bytes!(
            "../../../fixtures/synthetic/embed/embed-std14-bare.pdf"
        ));
        let mut enc = session(include_bytes!(
            "../../../fixtures/synthetic/embed/embed-std14-encoded.pdf"
        ));
        let req = EmbedRequest::all_missing().with_font("Helvetica", donor(FontMatch::Alias));

        let bare_plan = bare.embed_preview(&req);
        let enc_plan = enc.embed_preview(&req);
        assert!(bare_plan.targets[0].encoding_written);
        assert!(
            !enc_plan.targets[0].encoding_written,
            "an /Encoding already in the file is never overwritten"
        );

        // Commit the WinAnsi one and read the widths back.
        enc.embed_fonts(&req).expect("embeds");
        let font = enc
            .value(enc_plan.targets[0].id)
            .and_then(Object::as_dict)
            .expect("font dict")
            .clone();
        assert_eq!(
            font.get(b"Encoding")
                .and_then(Object::as_name)
                .map(Name::as_bytes),
            Some(b"WinAnsiEncoding".as_slice()),
            "the file's own encoding survived untouched"
        );
        let first = usize::try_from(font.get(b"FirstChar").and_then(Object::as_int).unwrap())
            .expect("in range");
        let Some(Object::Array(widths)) = font.get(b"Widths") else {
            panic!("no /Widths");
        };

        // ★ Every entry checked against the AFM table under WinAnsi — not a
        // single hand-picked code. A spot check would be hostage to whichever
        // code the author happened to remember, and the two encodings differ
        // in more than a dozen places.
        let mut differs_from_standard = 0usize;
        for (i, w) in widths.iter().enumerate() {
            let code = u8::try_from(first + i).expect("range fits a byte");
            let want = fontdata::encoding_glyph_name(BaseEncoding::WinAnsi, code)
                .and_then(|n| fontdata::std14_width(Std14::Helvetica, n))
                .map_or(0, i64::from);
            assert_eq!(
                w.as_int(),
                Some(want),
                "code {code:#04x} must carry its WinAnsi width"
            );
            let under_standard = fontdata::encoding_glyph_name(BaseEncoding::Standard, code)
                .and_then(|n| fontdata::std14_width(Std14::Helvetica, n))
                .map_or(0, i64::from);
            if under_standard != want {
                differs_from_standard += 1;
            }
        }
        // The assertion above is only meaningful if the two encodings
        // actually disagree over this range — otherwise it would pass for a
        // synthesiser that read the wrong table.
        assert!(
            differs_from_standard >= 10,
            "the two encodings must genuinely disagree here, or this test proves nothing \
             (differences found: {differs_from_standard})"
        );
    }

    /// The drop-in case, and the sharpest round-trip claim in the Pass:
    /// exactly ONE key is added to ONE descriptor, one stream object is
    /// created, and **nothing else in the file changes**.
    #[test]
    fn attaching_writes_two_objects_and_touches_nothing_else() {
        let source = include_bytes!("../../../fixtures/synthetic/embed/embed-attach.pdf");
        let mut s = session(source);
        let req = EmbedRequest::all_missing().with_font("pdfceMissing", donor(FontMatch::Exact));
        let plan = s.embed_fonts(&req).expect("embeds");
        assert_eq!(plan.targets.len(), 1);
        let t = &plan.targets[0];
        assert_eq!(t.shape, EmbedShape::Attach);
        assert!(!t.descriptor_written);
        assert!(!t.encoding_written);
        assert!(!t.redeclared_truetype);
        assert_eq!(t.widths_written, 0);

        let (bytes, report) = s
            .to_incremental_bytes(&SaveOptions::identity())
            .expect("saves");
        assert_eq!(
            report.objects_written, 2,
            "the descriptor and the new program stream — nothing else"
        );
        // ★ The round-trip claim, checked over the BYTES rather than over
        // the object model: an incremental save appends, so every byte of
        // the input must still be there, in place, unaltered.
        assert!(
            bytes.starts_with(source),
            "an incremental save must leave the original revision byte-identical"
        );

        // And the result really is embedded, from a fresh parse.
        let reopened = Document::from_bytes(bytes).expect("output parses");
        let inv = crate::fontinfo::inventory(&reopened.view());
        assert_eq!(
            inv.fonts
                .iter()
                .filter(|f| matches!(f.program, Program::NotEmbedded))
                .count(),
            0,
            "not-embedded must reach zero — the operator's actual goal"
        );
    }

    /// Undo restores the document exactly, including the staged program.
    #[test]
    fn undo_restores_the_original_document() {
        let source = include_bytes!("../../../fixtures/synthetic/embed/embed-attach.pdf");
        let mut s = session(source);
        let req = EmbedRequest::all_missing().with_font("pdfceMissing", donor(FontMatch::Exact));
        s.embed_fonts(&req).expect("embeds");
        assert!(s.is_modified());
        s.undo();
        let (bytes, report) = s
            .to_incremental_bytes(&SaveOptions::identity())
            .expect("saves");
        assert_eq!(
            report.objects_written, 0,
            "after undo the dirty set is empty — §11.1's save-time diff, not a session counter"
        );
        assert_eq!(
            bytes.as_slice(),
            source.as_slice(),
            "undo must reproduce the input byte for byte"
        );
    }

    /// ★ The commonest headless outcome. Every missing font is named, with a
    /// reason that says what would satisfy it.
    #[test]
    fn with_no_donor_every_font_is_refused_by_name() {
        let s = session(include_bytes!(
            "../../../fixtures/synthetic/embed/embed-std14-bare.pdf"
        ));
        let plan = s.embed_preview(&EmbedRequest::all_missing());
        assert!(plan.targets.is_empty());
        assert_eq!(plan.blocked.len(), 1);
        assert_eq!(plan.blocked[0].blocker, EmbedBlocker::NoSourceFont);
        assert_eq!(plan.blocked[0].base_font.as_deref(), Some("Helvetica"));
        assert_eq!(
            plan.missing_after(),
            1,
            "nothing was solved, and it says so"
        );
    }

    /// Partial success: some fonts resolve, some refuse, and the report
    /// distinguishes every one of them. An all-or-nothing result would be
    /// wrong, and a silent partial one would be worse.
    #[test]
    fn partial_success_is_reported_per_font() {
        let mut s = session(include_bytes!(
            "../../../fixtures/synthetic/embed/embed-mixed.pdf"
        ));
        let req = EmbedRequest::all_missing()
            .with_font("pdfceAttach", donor(FontMatch::Exact))
            .with_font("Helvetica", donor(FontMatch::Alias));
        let plan = s.embed_preview(&req);
        assert_eq!(plan.missing_before, 4, "attach, std14, composite, type3");
        assert_eq!(plan.targets.len(), 2);
        let tokens = plan.blocker_counts();
        assert_eq!(tokens.get("composite"), Some(&1));
        assert_eq!(tokens.get("type3"), Some(&1));
        assert_eq!(
            tokens.get("already-embedded"),
            Some(&1),
            "the embedded font is listed too, so no row is unaccounted for"
        );
        assert_eq!(plan.missing_after(), 2);

        // The composite refusal must name Identity — the reason it can never
        // be satisfied by a substitute.
        let composite = plan
            .blocked
            .iter()
            .find(|b| b.blocker.token() == "composite")
            .expect("the composite font is refused BY NAME");
        assert_eq!(
            composite.blocker,
            EmbedBlocker::Composite { identity: true },
            "Identity-H is what makes the codes glyph indices"
        );

        let applied = s.embed_fonts(&req).expect("embeds the two that resolve");
        assert_eq!(applied.targets.len(), 2);
        let (bytes, _) = s
            .to_incremental_bytes(&SaveOptions::identity())
            .expect("saves");
        let reopened = Document::from_bytes(bytes).expect("output parses");
        let inv = crate::fontinfo::inventory(&reopened.view());
        assert_eq!(
            inv.fonts
                .iter()
                .filter(|f| matches!(f.program, Program::NotEmbedded))
                .count(),
            2,
            "two remain, exactly the two the plan said it could not solve"
        );
    }

    /// ★ The divergence from the mirror module. Two differently-named fonts
    /// through one descriptor block BOTH — unlike unembedding, where the
    /// same shape is idempotent.
    #[test]
    fn a_shared_descriptor_blocks_both_fonts() {
        let s = session(include_bytes!(
            "../../../fixtures/synthetic/embed/embed-shared-descriptor.pdf"
        ));
        let req = EmbedRequest::all_missing()
            .with_font("pdfceSharedA", donor(FontMatch::Exact))
            .with_font("pdfceSharedB", donor(FontMatch::Exact));
        let plan = s.embed_preview(&req);
        assert!(
            plan.targets.is_empty(),
            "one descriptor cannot name two different programs"
        );
        assert_eq!(
            plan.blocked
                .iter()
                .filter(|b| b.blocker.token() == "descriptor-shared")
                .count(),
            2,
            "both are blocked, both by name"
        );
    }

    /// ★ The symbolic guard, in BOTH directions, over two fixtures that
    /// differ in how their codes are mapped.
    ///
    /// A rule that refused every symbolic font would pass a
    /// refusal-only test on its own, and a rule that allowed every one
    /// would pass an acceptance-only test. Neither half is evidence
    /// without the other.
    ///
    /// - `embed-symbolic-truetype.pdf` maps codes through **the program's
    ///   own `(3,0)` cmap** (§9.6.6.4 Branch B, no `/Encoding`, symbolic
    ///   flag). An inferred donor there draws the wrong symbols silently, so
    ///   it is refused.
    /// - `embed-std14-dingbats.pdf` is a bare standard-14 symbolic face, so
    ///   pdfcer writes the Annex D.6 name table itself and glyph selection
    ///   runs **by name** through a Type 1-flavour dictionary. An inferred
    ///   donor there produces `.notdef` for a name it lacks — a visible
    ///   hole, not a plausible wrong symbol — so it proceeds.
    ///
    /// The second case uses a stub bare-CFF donor: `pdfcer-core` never parses
    /// a donor, so the bytes only have to carry the framing. Whether a real
    /// face actually contains `a1`…`a191` is a question for `embed-sweep`,
    /// which runs the pixel-identity oracle over real Base-14 faces.
    #[test]
    fn the_symbolic_guard_fires_on_cmap_mapping_and_stands_aside_for_name_mapping() {
        // -- refused: the mapping runs through the program's own cmap.
        let cmap_mapped =
            include_bytes!("../../../fixtures/synthetic/embed/embed-symbolic-truetype.pdf");
        for inferred in [FontMatch::Alias, FontMatch::Bundled] {
            let s = session(cmap_mapped);
            let req = EmbedRequest::all_missing().with_font("pdfceSymbolic", donor(inferred));
            let plan = s.embed_preview(&req);
            assert!(plan.targets.is_empty());
            assert_eq!(
                plan.blocked[0].blocker,
                EmbedBlocker::SymbolicSubstitute { matched: inferred },
                "a symbolic font mapped through its own cmap must refuse a stand-in"
            );
        }
        // The positive control on the SAME fixture: the face it names is
        // accepted, so the guard is about provenance and not about symbolic
        // fonts being unembeddable.
        let s = session(cmap_mapped);
        let req = EmbedRequest::all_missing().with_font("pdfceSymbolic", donor(FontMatch::Exact));
        assert_eq!(s.embed_preview(&req).targets.len(), 1);

        // -- allowed: pdfcer writes the name table, so selection is by name.
        // A bare-CFF donor keeps the dictionary Type 1-flavour (Table 126's
        // `Type1C` row) rather than forcing the /TrueType re-declaration.
        let name_mapped =
            include_bytes!("../../../fixtures/synthetic/embed/embed-std14-dingbats.pdf");
        let cff = SuppliedFont::new(
            vec![0x01, 0x00, 0x04, 0x01],
            "FoxitDingbats",
            "bundled: FoxitDingbats",
            FontMatch::Bundled,
        );
        let s = session(name_mapped);
        let req = EmbedRequest::all_missing().with_font("ZapfDingbats", cff);
        let plan = s.embed_preview(&req);
        assert_eq!(plan.targets.len(), 1, "blocked: {:?}", plan.blocked);
        let t = &plan.targets[0];
        assert_eq!(t.program_key, ProgramKey::FontFile3);
        assert_eq!(t.stream_subtype, Some("Type1C"));
        assert!(!t.redeclared_truetype);
        assert!(t.encoding_written, "the Annex D.6 name table is pdfcer's");

        // -- and the OTHER guard still stands: a `glyf` donor for the same
        // font forces the /TrueType re-declaration, whose §9.6.6.4 Branch A
        // lookup cannot resolve `a1`…`a191` through the Adobe Glyph List.
        let s = session(name_mapped);
        let req = EmbedRequest::all_missing().with_font("ZapfDingbats", donor(FontMatch::Exact));
        assert_eq!(
            s.embed_preview(&req).blocked[0].blocker,
            EmbedBlocker::EncodingNotSpellable
        );
    }

    /// ★ §9.9's opening paragraph, enforced. A donor whose own `fsType`
    /// says it may not be embedded is refused by name.
    ///
    /// Paired with a permissive donor over the SAME fixture, so the test
    /// cannot pass by refusing everything — which is exactly how this guard
    /// would look correct while doing nothing.
    #[test]
    fn a_donor_whose_licence_forbids_embedding_is_refused() {
        let fixture = include_bytes!("../../../fixtures/synthetic/embed/embed-attach.pdf");
        let s = session(fixture);
        let req = EmbedRequest::all_missing().with_font(
            "pdfceMissing",
            SuppliedFont::new(
                DONOR_RESTRICTED.to_vec(),
                "pdfceRestricted",
                "fixtures/restricted.ttf",
                FontMatch::Exact,
            ),
        );
        let plan = s.embed_preview(&req);
        assert!(plan.targets.is_empty());
        assert_eq!(
            plan.blocked[0].blocker,
            EmbedBlocker::EmbeddingForbidden {
                permission: EmbeddingPermission::Restricted,
                raw: 2,
            }
        );
        // The positive control.
        let s = session(fixture);
        let ok = EmbedRequest::all_missing().with_font("pdfceMissing", donor(FontMatch::Exact));
        assert_eq!(s.embed_preview(&ok).targets.len(), 1);
    }

    /// A font with no width table and no standard-14 identity is refused:
    /// taking the advances from the donor would move every glyph on the
    /// page, which is the one thing this operation promises not to do.
    #[test]
    fn a_font_with_no_metric_source_is_refused() {
        let s = session(include_bytes!(
            "../../../fixtures/synthetic/embed/embed-nometrics.pdf"
        ));
        let req = EmbedRequest::all_missing().with_font("pdfceNoMetrics", donor(FontMatch::Exact));
        let plan = s.embed_preview(&req);
        assert!(plan.targets.is_empty());
        assert_eq!(plan.blocked[0].blocker, EmbedBlocker::NoMetricSource);
    }

    /// A donor that is not a font, is a collection, or is oversized: each
    /// refused by its own name rather than through one generic failure.
    #[test]
    fn a_donor_that_is_not_an_embeddable_program_is_refused_by_kind() {
        let fixture = include_bytes!("../../../fixtures/synthetic/embed/embed-attach.pdf");
        let cases: [(Vec<u8>, EmbedBlocker); 3] = [
            (
                b"this is not a font".to_vec(),
                EmbedBlocker::ProgramUnrecognised,
            ),
            (
                b"ttcf\0\x01\0\0".to_vec(),
                EmbedBlocker::ProgramIsCollection,
            ),
            (
                vec![0u8; MAX_DONOR_BYTES + 1],
                EmbedBlocker::ProgramTooLarge {
                    bytes: MAX_DONOR_BYTES + 1,
                },
            ),
        ];
        for (bytes, want) in cases {
            let s = session(fixture);
            let req = EmbedRequest::all_missing().with_font(
                "pdfceMissing",
                SuppliedFont::new(bytes, "x", "x", FontMatch::Exact),
            );
            assert_eq!(s.embed_preview(&req).blocked[0].blocker, want);
        }
    }

    /// The synthesised descriptor and stream dictionary carry exactly what
    /// §9.8.1 Table 122 and §9.9 Table 127 require — checked on the written
    /// objects, not on the plan, because the plan is what pdfcer *intended*.
    #[test]
    fn the_written_objects_match_tables_122_and_127() {
        let mut s = session(include_bytes!(
            "../../../fixtures/synthetic/embed/embed-std14-bare.pdf"
        ));
        let req = EmbedRequest::all_missing().with_font("Helvetica", donor(FontMatch::Alias));
        let plan = s.embed_fonts(&req).expect("embeds");
        let font = s
            .value(plan.targets[0].id)
            .and_then(Object::as_dict)
            .expect("font dict")
            .clone();
        assert_eq!(
            font.get(b"Subtype")
                .and_then(Object::as_name)
                .map(Name::as_bytes),
            Some(b"TrueType".as_slice()),
            "re-declared so the glyf donor can lawfully attach"
        );
        let descriptor_id = font
            .get(b"FontDescriptor")
            .and_then(Object::as_reference)
            .expect("a NEW indirect descriptor");
        let d = s
            .value(descriptor_id)
            .and_then(Object::as_dict)
            .expect("descriptor")
            .clone();
        // Table 122's required entries for a non-Type-3 descriptor.
        for key in [
            b"Type".as_slice(),
            b"FontName",
            b"Flags",
            b"FontBBox",
            b"ItalicAngle",
            b"Ascent",
            b"Descent",
            b"CapHeight",
            b"StemV",
        ] {
            assert!(d.contains_key(key), "Table 122 requires {:?}", key);
        }
        // §9.6.6.4 Branch A only runs for a NONSYMBOLIC font.
        assert_eq!(
            d.get(b"Flags").and_then(Object::as_int),
            Some(32),
            "Helvetica is Nonsymbolic (Table 123 bit 6)"
        );
        // Table 122: /FontName shall equal /BaseFont.
        assert_eq!(
            d.get(b"FontName")
                .and_then(Object::as_name)
                .map(Name::as_bytes),
            font.get(b"BaseFont")
                .and_then(Object::as_name)
                .map(Name::as_bytes),
        );

        let program_id = d
            .get(b"FontFile2")
            .and_then(Object::as_reference)
            .expect("the program");
        let Some(Object::Stream(stream)) = s.value(program_id) else {
            panic!("the program is not a stream");
        };
        // Table 127: /FontFile2 carries /Length1 (the DECODED length) and
        // no /Subtype. Getting /Length1 from the compressed buffer is the
        // single easiest mistake here and produces a font no reader loads.
        assert_eq!(
            stream.dict.get(b"Length1").and_then(Object::as_int),
            Some(i64::try_from(DONOR.len()).unwrap()),
            "/Length1 is the length AFTER the filters are applied"
        );
        assert!(!stream.dict.contains_key(b"Subtype"));
        assert_eq!(
            stream
                .dict
                .get(b"Filter")
                .and_then(Object::as_name)
                .map(Name::as_bytes),
            Some(b"FlateDecode".as_slice())
        );
        // And the compressed bytes really do decode back to the donor.
        let raw = s
            .view()
            .slice(stream.data_span)
            .expect("staged bytes are reachable")
            .to_vec();
        let decoded = crate::filters::decode_stream(&stream.dict, &raw).expect("decodes");
        assert_eq!(decoded, DONOR, "what was embedded is what was supplied");
    }

    /// Embedding is offered on a PDF/A document without a gate: it moves the
    /// file TOWARD conformance. The exact opposite of the mirror module,
    /// and worth an explicit test so a later "consistency" edit does not
    /// quietly add a refusal that would be backwards.
    #[test]
    fn a_pdfa_document_is_not_gated() {
        let s = session(include_bytes!(
            "../../../fixtures/synthetic/embed/embed-attach.pdf"
        ));
        assert!(s.embed_refusal().is_none());
        assert_eq!(
            s.embed_preview(&EmbedRequest::all_missing()).pdfa,
            PdfaClaim::None
        );
    }

    /// ★ REGRESSION: a created object must not land on the object number the
    /// writer reuses for its own cross-reference stream.
    ///
    /// The fixture's xref stream is object 6 while its `/Size` is 6 and its
    /// `/Index` omits it, so every source
    /// [`Document::next_object_number`](crate::document::Document::next_object_number)
    /// consulted before this was fixed answered 5 and it handed out **6**.
    /// The session wrote the font program there; the writer then wrote its
    /// own section over the top, later in the file. The result parsed,
    /// opened and rendered — with a 44-byte cross-reference stream sitting
    /// where a 16 KB font program should have been, and the text it drew
    /// silently skipped.
    ///
    /// The assertion is over the REOPENED bytes, decoded, and compares them
    /// to the donor. Checking the object number alone would pass the moment
    /// somebody changed the number and left the collision.
    ///
    /// The bug was never specific to embedding — any object-creating command
    /// hits it on a file shaped this way — but this is where it was found,
    /// so this is where the guard lives until a more general home exists.
    #[test]
    fn a_created_object_never_collides_with_the_cross_reference_stream() {
        let mut s = session(include_bytes!(
            "../../../fixtures/synthetic/embed/embed-xrefstream-outside-size.pdf"
        ));
        let req = EmbedRequest::all_missing().with_font("Helvetica", donor(FontMatch::Alias));
        let plan = s.embed_fonts(&req).expect("embeds");
        assert_eq!(plan.targets.len(), 1);
        let (bytes, _) = s
            .to_incremental_bytes(&SaveOptions::identity())
            .expect("saves");

        let reopened = Document::from_bytes(bytes).expect("output parses");
        let view = reopened.view();
        let inv = crate::fontinfo::inventory(&view);
        let font = inv.fonts.first().expect("the font is still there");
        let Program::Embedded(program) = &font.program else {
            panic!(
                "the font program did not survive the save: {:?}",
                font.program
            );
        };
        assert_eq!(
            program.decoded_bytes,
            Some(DONOR.len()),
            "the embedded program must be the donor, not whatever object took its number"
        );
    }

    /// Selecting by name matches both spellings, and a name that matches
    /// nothing is reported rather than silently doing nothing.
    #[test]
    fn a_name_that_matches_nothing_is_reported() {
        let s = session(include_bytes!(
            "../../../fixtures/synthetic/embed/embed-attach.pdf"
        ));
        let mut req = EmbedRequest::named(["pdfceMissing", "NoSuchFont"]);
        req.supplied
            .insert("pdfceMissing".to_owned(), donor(FontMatch::Exact));
        let plan = s.embed_preview(&req);
        assert_eq!(plan.targets.len(), 1);
        assert_eq!(plan.unmatched, vec!["NoSuchFont".to_owned()]);
    }

    /// ★ A font left missing by an explicit selection is counted and
    /// **not** explained, and the plan says which of the two it is.
    ///
    /// `missing_after()` counts the whole document; `blocked` lists only what
    /// the operation considered. Under [`EmbedSelection::Named`] those two
    /// diverge, and a shell that assumed they agreed printed
    /// "every one is listed above with its reason" over an empty list. The
    /// hazard is made to occur here rather than asserted about in the
    /// abstract (R187): three fonts missing, one named and embedded, and the
    /// other two reported by NEITHER list.
    #[test]
    fn fonts_outside_an_explicit_selection_are_counted_but_unexplained() {
        let s = session(include_bytes!(
            "../../../fixtures/synthetic/embed/embed-mixed.pdf"
        ));
        let req =
            EmbedRequest::named(["pdfceAttach"]).with_font("pdfceAttach", donor(FontMatch::Exact));
        let plan = s.embed_preview(&req);

        assert_eq!(plan.targets.len(), 1, "the one font named is embedded");
        assert!(
            plan.blocked.is_empty(),
            "an unnamed font is not a refusal, so nothing is listed: {:?}",
            plan.blocked
        );
        assert!(
            plan.missing_after() > 0,
            "the hazard has to occur or the guard is untested"
        );
        assert_eq!(
            plan.explained_missing(),
            0,
            "nothing was listed, so nothing is explained"
        );
        assert_eq!(
            plan.unexplained_missing(),
            plan.missing_after(),
            "every still-missing font here is one the report says nothing about"
        );
    }

    /// The counterpart: under `AllMissing` every still-missing font DOES get
    /// a row, so `unexplained_missing()` is zero and the "listed above"
    /// wording is owed.
    ///
    /// Pins the invariant the shell branches on, in the mode the sweep
    /// harness runs — the mode in which the bug above was invisible.
    #[test]
    fn all_missing_leaves_nothing_unexplained() {
        let s = session(include_bytes!(
            "../../../fixtures/synthetic/embed/embed-mixed.pdf"
        ));
        // No donors at all: every missing font is refused, which is the
        // strongest form of "each one got a reason".
        let plan = s.embed_preview(&EmbedRequest::all_missing());
        assert!(plan.missing_after() > 0, "nothing can be embedded");
        assert_eq!(plan.unexplained_missing(), 0);
        assert_eq!(plan.explained_missing(), plan.missing_after());
        assert!(
            plan.blocked.iter().any(|b| b.missing_program),
            "and the rows carrying that count are marked as such"
        );
    }
}
