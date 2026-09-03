//! Font **inventory** — what fonts a document carries, where they are
//! reached from, how many bytes each embedded program costs, and whether
//! that program could be removed without destroying the text it draws.
//!
//! Read-only. Nothing in this module mutates a document, stages a byte, or
//! decides anything on the operator's behalf. It is the *report* half of
//! the font-cleanup story; the removal half is
//! [`crate::font_unembed`], which **shipped in `Pass 67.0` phase B** and
//! consumes [`Removability`] as given rather than deriving a second
//! classifier. That is what makes "the report and the action cannot
//! disagree" a structural fact rather than a promise: there is one
//! classifier, and if a verdict is wrong it is wrong in one place.
//!
//! # Why the report exists at all, and why it ships before removal
//!
//! The operator's request was *"someone needs embedded fonts removed from a
//! PDF."* The obvious implementation — find `/FontFile*`, delete it — is
//! wrong for most real files, and wrong **silently**: the text keeps its
//! character codes, the page keeps its operators, and the glyphs turn into
//! rubbish or vanish.
//!
//! ISO 32000-1 §9.9 is the reason, and it is a `shall` on the *writer*:
//!
//! > "Since this process sometimes produces ambiguous results, conforming
//! > writers, instead of using a simple font, **shall** use a Type 0 font
//! > with an `Identity-H` encoding and **use the glyph indices as
//! > character codes**."
//!
//! For such a font the bytes in the content stream are not characters. They
//! are positions inside *that exact embedded program*. Delete the program
//! and there is nothing left that says what code `0x0027` meant — unless a
//! `/ToUnicode` CMap happens to be present, and even then it recovers the
//! *text*, not the ability of any substitute face to draw it, because a
//! different build of "the same" font has a different glyph order.
//!
//! A 64-file survey of the PDFBox corpus (recorded in the Pass 67.0 brief)
//! measured the shape of the problem: of the 30 files that embed fonts,
//! **87 % embed subsets**, **40 % use `Identity-H`**, and only **50 %**
//! carry `/ToUnicode`. So the majority case for "remove the embedded fonts"
//! is a case where removal destroys the document.
//!
//! Acrobat reaches the same conclusion and acts on it — a former Adobe
//! Principal Scientist, quoted in
//! `Acrobat_Features/optimize__font_unembedding.md`, describes the refusal
//! as avoiding "a useless PDF file". But **Acrobat refuses silently**: the
//! font simply does not appear in its unembed list, with no reason given
//! anywhere on screen. pdfcer says why. That is a deliberate divergence, and
//! it is project rule 4 ("fuzzy, never sneaky") applied to a refusal rather
//! than to a suggestion: a shorter list is not actionable, and "this font's
//! text is stored as glyph indices into this exact program" is.
//!
//! # The three questions this module answers
//!
//! 1. **What fonts are here?** [`FontRecord`] — `/BaseFont` verbatim, the
//!    de-prefixed family name when the `/BaseFont` carries an ISO 32000-1
//!    §9.6.4 subset tag, `/Subtype` (and the descendant's for a `Type0`),
//!    `/Encoding`, whether `/ToUnicode` is present.
//! 2. **What do they cost?** [`EmbeddedProgram::stored_bytes`] — the
//!    embedded font program's size **in this file**. ★ Acrobat exposes this
//!    nowhere: not Document Properties → Fonts (which gives type/encoding/
//!    embedded status and no size at all), and not Audit Space Usage (which
//!    gives one *aggregate* "Fonts" bucket for the whole document, with no
//!    per-font attribution). The number is directly computable from data
//!    pdfcer has already parsed, and it is the number an operator optimising
//!    a file actually wants.
//! 3. **Can they be removed?** [`Removability`] — an enum that carries its
//!    reason, not a boolean. The later unembedding Pass consumes exactly
//!    this value, so the report and the action cannot disagree.
//!
//! # ★ Coverage is part of the answer, not a footnote
//!
//! Fonts are reachable from more than one place, and a font inventory that
//! quietly misses one and then prints a confident list is this project's
//! most-repeated defect shape (R186: a check that confirms the marker
//! rather than the thing). So [`SurfaceCoverage`] is returned alongside the
//! list and **names the surfaces that were not walked** as explicitly as
//! the ones that were. Acrobat's own coverage here is recorded as an
//! unconfirmed GAP in `optimize__font_reporting.md` — one community source
//! says form-field fonts are excluded from the *Optimizer's* unembed list,
//! and nothing speaks to the read-only Fonts tab — so pdfcer states its own
//! scope rather than assuming parity with a behaviour nobody has measured.
//!
//! Walked (see [`Surface`]):
//!
//! - page `/Resources /Font`, with §7.7.3.4 inheritance already resolved by
//!   [`crate::page_tree::pages_in`];
//! - form XObjects' own `/Resources`, recursively and without depth limit
//!   other than the node budget;
//! - tiling patterns' `/Resources` (a pattern is a content stream);
//! - soft-mask group XObjects reached through `/ExtGState /SMask /G`;
//! - a Type 3 font's own `/Resources` (its `/CharProcs` are content streams
//!   and may name further fonts);
//! - the AcroForm's `/DR /Font` default-resource dictionary;
//! - every annotation appearance stream's own `/Resources`, through all of
//!   `/AP /N`, `/AP /R`, `/AP /D` including their appearance-state
//!   subdictionaries.
//!
//! **Not walked**, and said so: [`Surface::UnreferencedObjects`] — font
//! dictionaries that exist in the file but are reachable from none of the
//! above. They still occupy bytes, so this is a real omission for an
//! optimisation report, not a pedantic one. It is out of scope here because
//! [`ObjectGraph`] is a *resolution* interface with no enumeration
//! primitive, and adding one to serve a report would widen the trait every
//! view must implement.
//!
//! # Hazards this module exists to get right
//!
//! **Size comes from [`Stream::data_span`], never from `/Length`.** On an
//! encrypted document the two disagree by design: [`crate::document`]'s
//! decryption walk writes the plaintext back at `data_span.start` and
//! *shortens `data_span.len`*, leaving the dictionary's `/Length` at the
//! ciphertext length (an `/AESV2` stream carries a 16-byte IV plus padding,
//! so `/Length` overstates by at least 17 bytes). `data_span` is what every
//! reader in this crate slices; a size report built on `/Length` would be
//! wrong for every font in every encrypted file, and plausibly wrong, which
//! is worse.
//!
//! **A `Type0` font's descriptor hangs off its descendant.** §9.8.1: a font
//! descriptor *shall not* be used with a Type 0 font. `/FontFile2` for a
//! composite font lives on the `/DescendantFonts [0] /FontDescriptor`, and
//! code that looks for it on the parent finds nothing and reports "not
//! embedded" about a font that is embedded.
//!
//! **"Not embedded" and "embedded but unreadable" are different facts.**
//! The first is a document that relies on viewer-side substitution and has
//! nothing to remove; the second is damage. They lead to different operator
//! actions, so [`Program`] distinguishes them rather than folding both into
//! an `Option`.
//!
//! **`fsType` is never guessed.** [`FsType::Unreadable`] and
//! [`FsType::NotApplicable`] are distinct states from any permission value,
//! and in particular from `0`, which genuinely *means* Installable — the
//! most permissive value there is. Modelling "absent" as `0` would silently
//! grant the broadest right the field can express. See
//! `PDF_Spec/fonts/font__opentype_os2_fstype.md` N1.
//!
//! # Spec sources
//!
//! - ISO 32000-1 §9.5–9.10 — font dictionaries, simple and composite fonts,
//!   descriptors, embedded font programs, `/ToUnicode`
//! - ISO 32000-1 §9.6.4 — subset tags: "exactly six uppercase letters"
//!   followed by `+`
//! - ISO 32000-1 §9.8.2 Table 123 — the descriptor `/Flags` Symbolic bit
//! - ISO 32000-1 §9.9 Tables 126/127 — which descriptor key carries which
//!   program format, and `/FontFile3`'s `/Subtype`
//! - ISO 32000-1 §7.7.3.4, §7.8.3 — resource inheritance
//! - ISO 32000-1 §12.5.5 — annotation appearance streams and `/AS`
//! - OpenType 1.9.1, `OS/2` — the `fsType` field, its version differences,
//!   and the table directory used to find it
//! - `PDF_Spec/iso32000/iso32000__ref__font_embedding.md`
//! - `PDF_Spec/fonts/font__opentype_os2_fstype.md`
//! - `Acrobat_Features/optimize__font_reporting.md`,
//!   `optimize__font_unembedding.md`

use std::collections::{BTreeMap, BTreeSet};

use crate::filters;
use crate::graph::ObjectGraph;
use crate::object::{Dict, Name, ObjId, Object, Stream};
use crate::page_tree::pages_in;
use crate::view::DocumentView;

/// Maximum resource dictionaries entered during the sweep.
///
/// Form XObjects carry their own `/Resources`, which may hold further form
/// XObjects; a hostile or merely damaged file can nest that arbitrarily and
/// can make two dictionaries reference each other. The `visited` set makes
/// a cycle terminate; this budget makes an unbounded *tree* terminate too.
/// Matches [`crate::layers::MAX_RESOURCE_NODES`] deliberately — the two
/// walks visit the same graph and there is no reason for one to give up
/// before the other.
///
/// Exceeding it sets [`FontDiagnostics::resource_scan_truncated`], which is
/// reported rather than swallowed: a truncated inventory that looks
/// complete is the failure this module's coverage discipline exists to
/// prevent.
pub const MAX_RESOURCE_NODES: usize = 8192;

/// Maximum distinct fonts recorded.
///
/// A real document has tens; a merged monster has hundreds. Four thousand
/// is far past any legitimate file and still trivially cheap to hold.
/// Exceeding it sets [`FontDiagnostics::font_limit_reached`].
pub const MAX_FONTS: usize = 4096;

/// Maximum resource names recorded per font.
///
/// A font referenced from 500 pages is usually bound to the same one or two
/// names (`/F1`), but nothing requires that. The names are a debugging
/// convenience, not the answer, so they are capped and the cap is visible
/// through [`FontRecord::resource_names_truncated`].
pub const MAX_RESOURCE_NAMES_PER_FONT: usize = 16;

/// Maximum sfnt table-directory entries read when looking for `OS/2`.
///
/// `numTables` is a `uint16`, so a malformed font can claim 65 535 tables
/// and cost 1 MiB of directory reads before failing. Every legitimate font
/// has well under a hundred. The read is bounded by the buffer as well, so
/// this is belt-and-braces on an untrusted-input path.
pub const MAX_SFNT_TABLES: usize = 512;

// ---------------------------------------------------------------------------
// Font type and encoding
// ---------------------------------------------------------------------------

/// A font dictionary's `/Subtype` (§9.5, Table 110).
///
/// `Type0` is *composite*: its character codes are decoded by a CMap into
/// CIDs, and the actual glyph source is a descendant CIDFont. Everything
/// else here is *simple*: single-byte codes indexed through an encoding.
/// The distinction is the top of every branch in this module.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum FontSubtype {
    /// Type 1 (§9.6.2) — PostScript outlines, `/FontFile`.
    Type1,
    /// Multiple Master Type 1 (§9.6.2 NOTE) — treated as Type 1 by every
    /// structural rule that matters here.
    MmType1,
    /// TrueType (§9.6.3) — `glyf` outlines, `/FontFile2`.
    TrueType,
    /// Type 3 (§9.6.5) — glyphs are **content streams** in this document's
    /// `/CharProcs`, not an outline program. See [`Removability::BlockedType3`].
    Type3,
    /// Type 0 (§9.7) — composite. The glyph source is the descendant.
    Type0,
    /// A CIDFont with `glyf` outlines (§9.7.4). Only ever a *descendant*.
    CidFontType2,
    /// A CIDFont with CFF outlines (§9.7.4). Only ever a *descendant*.
    CidFontType0,
    /// A `/Subtype` name pdfcer does not model, carried verbatim.
    Other(String),
    /// The dictionary has no `/Subtype`. Required by Table 110, so this is
    /// a genuine malformation and is reported rather than guessed at.
    Absent,
}

impl FontSubtype {
    /// Classify a `/Subtype` name's bytes.
    fn from_name(name: &Name) -> Self {
        match name.as_bytes() {
            b"Type1" => Self::Type1,
            b"MMType1" => Self::MmType1,
            b"TrueType" => Self::TrueType,
            b"Type3" => Self::Type3,
            b"Type0" => Self::Type0,
            b"CIDFontType2" => Self::CidFontType2,
            b"CIDFontType0" => Self::CidFontType0,
            other => Self::Other(String::from_utf8_lossy(other).into_owned()),
        }
    }

    /// The name as it appears in the file, for display.
    ///
    /// # Examples
    ///
    /// ```
    /// use pdfcer_core::fontinfo::FontSubtype;
    ///
    /// assert_eq!(FontSubtype::Type0.label(), "Type0");
    /// assert_eq!(FontSubtype::Absent.label(), "(no /Subtype)");
    /// ```
    #[must_use]
    pub fn label(&self) -> &str {
        match self {
            Self::Type1 => "Type1",
            Self::MmType1 => "MMType1",
            Self::TrueType => "TrueType",
            Self::Type3 => "Type3",
            Self::Type0 => "Type0",
            Self::CidFontType2 => "CIDFontType2",
            Self::CidFontType0 => "CIDFontType0",
            Self::Other(s) => s.as_str(),
            Self::Absent => "(no /Subtype)",
        }
    }

    /// Whether this is a composite (Type 0) font — the branch that decides
    /// nearly everything else.
    #[must_use]
    pub const fn is_composite(&self) -> bool {
        matches!(self, Self::Type0)
    }
}

/// What a font's `/Encoding` entry is (§9.6.6, §9.7.5).
///
/// The important question this answers is **not** "what does code 65
/// mean" — it is "does the meaning of the codes survive the removal of the
/// embedded program". A predefined `Identity-H` says no; a
/// `/WinAnsiEncoding` says yes; an absent entry on a symbolic TrueType font
/// says "the answer is inside the program you are about to delete".
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Encoding {
    /// No `/Encoding` key. For a simple font the program's **built-in**
    /// encoding governs (§9.6.6.1); for a composite font this is
    /// malformed (Table 121 makes `/Encoding` required).
    Absent,
    /// A name: a predefined CMap for a composite font (`Identity-H`,
    /// `UniJIS-UCS2-H`, …) or one of the base encodings for a simple font
    /// (`WinAnsiEncoding`, `MacRomanEncoding`, `MacExpertEncoding`).
    Predefined(String),
    /// An encoding *dictionary* (§9.6.6.1, Table 114) — an optional
    /// `/BaseEncoding` plus `/Differences`.
    Dictionary {
        /// `/BaseEncoding`, when present.
        base: Option<String>,
        /// Whether `/Differences` is present and non-empty.
        has_differences: bool,
    },
    /// An **embedded CMap stream** (§9.7.5.3). pdfcer does not parse it
    /// here, so what the codes mean is not known from this report alone.
    CMapStream {
        /// `/CMapName`, when the stream declares one.
        name: Option<String>,
    },
    /// `/Encoding` held something that is neither a name, a dictionary nor
    /// a stream.
    Malformed,
}

impl Encoding {
    /// Whether this is one of the two identity CMaps.
    ///
    /// `Identity-H`/`Identity-V` map a 2-byte code to the same 2-byte CID
    /// (Table 118), which — combined with `/CIDToGIDMap /Identity`, the
    /// overwhelmingly common pairing — makes **code = CID = glyph index in
    /// this program**. That equality is what makes the program
    /// unremovable.
    ///
    /// A `/CMapName` of `Identity-H` on an *embedded* CMap stream counts
    /// too: the encoding is identity regardless of how it was spelled.
    ///
    /// # Examples
    ///
    /// ```
    /// use pdfcer_core::fontinfo::Encoding;
    ///
    /// assert!(Encoding::Predefined("Identity-H".to_owned()).is_identity());
    /// assert!(!Encoding::Predefined("WinAnsiEncoding".to_owned()).is_identity());
    /// assert!(!Encoding::Absent.is_identity());
    /// ```
    #[must_use]
    pub fn is_identity(&self) -> bool {
        let is_identity_name = |s: &str| s == "Identity-H" || s == "Identity-V";
        match self {
            Self::Predefined(name) => is_identity_name(name),
            Self::CMapStream { name: Some(name) } => is_identity_name(name),
            _ => false,
        }
    }

    /// Whether this names one of the base encodings ISO 32000-1 defines in
    /// Annex D — the ones whose code→character meaning is written in the
    /// *standard* rather than in the embedded program.
    ///
    /// This is the property that makes a simple font's text survive
    /// unembedding: any substitute face with the same character repertoire
    /// draws the same characters for the same codes.
    #[must_use]
    pub fn is_standard_base(&self) -> bool {
        let known = |s: &str| {
            matches!(
                s,
                "WinAnsiEncoding" | "MacRomanEncoding" | "MacExpertEncoding" | "StandardEncoding"
            )
        };
        match self {
            Self::Predefined(name) => known(name),
            Self::Dictionary { base: Some(b), .. } => known(b),
            _ => false,
        }
    }

    /// A stable one-line label for a listing.
    #[must_use]
    pub fn label(&self) -> String {
        match self {
            Self::Absent => "(built-in)".to_owned(),
            Self::Predefined(name) => name.clone(),
            Self::Dictionary {
                base,
                has_differences,
            } => {
                let base = base.as_deref().unwrap_or("(built-in)");
                if *has_differences {
                    format!("{base}+Differences")
                } else {
                    base.to_owned()
                }
            }
            Self::CMapStream { name } => match name {
                Some(n) => format!("CMap({n})"),
                None => "CMap(embedded)".to_owned(),
            },
            Self::Malformed => "(malformed)".to_owned(),
        }
    }
}

// ---------------------------------------------------------------------------
// The embedded program
// ---------------------------------------------------------------------------

/// Which descriptor key carries the embedded font program (§9.9, Table 126).
///
/// The key is not decoration — it declares the program's format, and the
/// format decides whether an `OS/2` table can exist at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum ProgramKey {
    /// `/FontFile` — a Type 1 font program (PFB-style, `/Length1`,
    /// `/Length2`, `/Length3`). Has no `OS/2` table by construction.
    FontFile,
    /// `/FontFile2` — a TrueType/sfnt program with `glyf` outlines.
    FontFile2,
    /// `/FontFile3` — format declared by the stream's own `/Subtype`
    /// (`Type1C`, `CIDFontType0C`, `OpenType`).
    FontFile3,
}

impl ProgramKey {
    /// The dictionary key's spelling.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::FontFile => "FontFile",
            Self::FontFile2 => "FontFile2",
            Self::FontFile3 => "FontFile3",
        }
    }
}

/// Why an embedded font program's bytes could not be reached.
///
/// Distinct from "not embedded", because the operator's next move differs:
/// a font that is not embedded is a document relying on substitution, while
/// a font whose program cannot be read is a **damaged** document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ProgramUnreadable {
    /// The descriptor names a font-program object that is not in the file.
    /// §7.3.10 makes that a `null`, not an error — so the document is not
    /// rejected, but the program is gone.
    #[error("the font-program object referenced by the descriptor is not in this file")]
    DanglingReference,
    /// The `/FontFile*` entry resolved to something that is not a stream.
    #[error("the /FontFile entry is not a stream")]
    NotAStream,
    /// The stream's byte span does not lie inside this view's source — a
    /// truncated file, or a `/Length` that ran past the end of the buffer.
    #[error("the font-program stream's bytes lie outside the file")]
    SpanOutsideFile,
}

/// An embedded font program, measured.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct EmbeddedProgram {
    /// Which descriptor key carried it.
    pub key: ProgramKey,
    /// `/FontFile3`'s `/Subtype` (`Type1C`, `CIDFontType0C`, `OpenType`),
    /// when present. Always `None` for `/FontFile` and `/FontFile2`, which
    /// Table 127 gives no `/Subtype`.
    pub subtype: Option<String>,
    /// **The bytes this program occupies in the file**, as stored — after
    /// decryption, before filter decoding.
    ///
    /// ★ This is the exceed-Acrobat number, and it is deliberately the
    /// *stored* size rather than the decoded size, because it answers the
    /// question an operator optimising a file is asking: *how much smaller
    /// does the file get if this program goes away.* The decoded size (see
    /// [`Self::decoded_bytes`]) answers a different question — how big the
    /// font actually is — and both are reported.
    ///
    /// Taken from [`Stream::data_span`], **never** from the dictionary's
    /// `/Length`: on an encrypted document the decryption walk shortens the
    /// span and leaves `/Length` at the ciphertext length. See the module
    /// docs.
    pub stored_bytes: usize,
    /// The program's size after its `/Filter` chain has been decoded, or
    /// `None` when decoding failed (which is itself reported through
    /// [`FontDiagnostics::programs_undecodable`]).
    pub decoded_bytes: Option<usize>,
    /// The OpenType embedding-permission bits, read from the program's own
    /// `OS/2` table — or an explicit statement that they could not be read.
    pub fs_type: FsType,
}

/// Whether a font's glyph program is embedded, and in what state.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Program {
    /// No embedded program: either the font has no `/FontDescriptor` at all
    /// (legitimate for the standard 14 fonts, §9.6.2.2) or the descriptor
    /// carries none of `/FontFile`, `/FontFile2`, `/FontFile3`.
    ///
    /// There is nothing to remove. A "remove embedded fonts" action must
    /// not report success for these — it did nothing.
    NotEmbedded,
    /// A `/FontFile*` key is present but its bytes could not be reached.
    Unreadable {
        /// Which key made the claim.
        key: ProgramKey,
        /// Why the bytes are not available.
        why: ProgramUnreadable,
    },
    /// The program is present and was measured.
    Embedded(EmbeddedProgram),
}

impl Program {
    /// The program's stored size in this file, or `0` when there is none.
    ///
    /// Convenience for totalling a document; `0` is the honest answer for
    /// both [`Self::NotEmbedded`] and [`Self::Unreadable`], since neither
    /// contributes a measurable, removable payload.
    #[must_use]
    pub const fn stored_bytes(&self) -> usize {
        match self {
            Self::Embedded(p) => p.stored_bytes,
            Self::NotEmbedded | Self::Unreadable { .. } => 0,
        }
    }

    /// Whether a font program is actually present and measurable.
    #[must_use]
    pub const fn is_embedded(&self) -> bool {
        matches!(self, Self::Embedded(_))
    }
}

// ---------------------------------------------------------------------------
// fsType — OpenType OS/2 embedding permissions
// ---------------------------------------------------------------------------

/// Why an `fsType` read failed.
///
/// Every variant is a *measurement failure*, never a permission. See
/// [`FsType`] for why that distinction is load-bearing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum FsTypeError {
    /// The bytes do not start with a recognised sfnt magic
    /// (`0x00010000`, `OTTO`, `true`). A bare CFF program lands here, and
    /// legitimately: CFF has no `OS/2` table.
    #[error("the font program is not an sfnt (TrueType/OpenType) container")]
    NotSfnt,
    /// A TrueType **collection** (`ttcf`). Which face's `OS/2` applies is
    /// not determinable from the PDF alone, so no answer is invented.
    #[error("the font program is a TrueType collection; which face applies is not stated")]
    Collection,
    /// The table directory is truncated, or a table record points outside
    /// the buffer.
    #[error("the sfnt table directory is malformed or truncated")]
    BadTableDirectory,
    /// The sfnt has no `OS/2` table. Required by OpenType, so this is a
    /// non-conforming font — but the specification states **no** default
    /// permission for the absent case, so none is assumed (N1).
    #[error("the font program has no OS/2 table, and no default permission is defined")]
    NoOs2Table,
    /// The `OS/2` table is shorter than the ten bytes needed to reach
    /// `fsType` at offset 8.
    #[error("the OS/2 table is too short to contain fsType")]
    Os2Truncated,
}

/// The `fsType` usage sub-field (`fsType & 0x000F`), OpenType 1.9.1 `OS/2`.
///
/// **This is a value, not a bitmask.** A valid font sets at most one of
/// bits 1–3, and the legal values are exactly 0, 2, 4 and 8. Testing
/// `fsType != 0` as "restricted" is wrong; so is testing `fsType == 0` as
/// "no data".
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum EmbeddingPermission {
    /// `0` — Installable embedding. **The most permissive value**, and the
    /// reason an absent `OS/2` must never be modelled as zero.
    Installable,
    /// `2` — Restricted License embedding: the bits assert the font "must
    /// not be modified, embedded or exchanged in any manner without first
    /// obtaining explicit permission of the legal owner."
    Restricted,
    /// `4` — Preview & Print embedding. Carries a **document-level**
    /// assertion: documents containing such fonts "must be opened
    /// 'read-only'; no edits may be applied to the document."
    PreviewPrint,
    /// `8` — Editable embedding.
    Editable,
    /// More than one of bits 1–3 is set. For `OS/2` versions 0–2 the
    /// specification permits reading this as the least-restrictive of the
    /// set bits; from version 3 the bits are required to be mutually
    /// exclusive, so this is a non-conforming font and the standard gives
    /// only an observation about what "some applications could" do (N3).
    /// pdfcer reports the ambiguity rather than resolving it.
    Ambiguous,
    /// Only bit 0 is set (`fsType == 1`). Bit 0 is permanently reserved and
    /// its use is deprecated; some early fonts really did set it. The
    /// specification does **not** say what a reader should infer (N2), so
    /// pdfcer infers nothing.
    Unspecified,
}

impl EmbeddingPermission {
    /// A short stable label.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Installable => "Installable",
            Self::Restricted => "Restricted",
            Self::PreviewPrint => "PreviewPrint",
            Self::Editable => "Editable",
            Self::Ambiguous => "Ambiguous",
            Self::Unspecified => "Unspecified",
        }
    }
}

/// A successfully-read `fsType`, with the version-difference rules applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct FsTypeBits {
    /// The raw `uint16`, exactly as stored. Kept so a report can show the
    /// value that was actually read rather than only pdfcer's reading of it.
    pub raw: u16,
    /// The `OS/2` table's own `version` field (0–5 are defined).
    pub os2_version: u16,
    /// The usage sub-field, decoded.
    pub permission: EmbeddingPermission,
    /// Bit 8 — "the font must not be subsetted prior to embedding".
    ///
    /// **Forced to `false` for `OS/2` versions 0 and 1**, where the
    /// specification says applications *must ignore* bits 4–15. Reporting
    /// it as set there would refuse a font on the strength of bytes that
    /// never meant anything. [`Self::version_gated_bits_ignored`] records
    /// that the gate fired.
    pub no_subsetting: bool,
    /// Bit 9 — "only bitmaps contained in the font may be embedded. No
    /// outline data may be embedded." Same version gate as bit 8.
    pub bitmap_only: bool,
    /// Whether bits 8/9 were suppressed because the table is version 0 or 1.
    pub version_gated_bits_ignored: bool,
    /// Whether the deprecated reserved bit 0 is set.
    pub reserved_bit0: bool,
}

/// The `fsType` state of an embedded program.
///
/// **Four states, and the distinctions are the point.** `fsType == 0` means
/// *Installable* — maximally permissive. So "we could not read it" and
/// "this format has no such field" must never render like a permission,
/// and in particular must never render like zero. The OpenType
/// specification states no default for the absent case (N1); pdfcer
/// therefore states no default either.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum FsType {
    /// The program format has no `OS/2` table by construction: a
    /// `/FontFile` Type 1 program, or a `/FontFile3` bare CFF
    /// (`/Type1C`, `/CIDFontType0C`). Nothing failed; there is simply no
    /// field.
    NotApplicable,
    /// The program's filter chain did not decode, so no read was attempted.
    ProgramNotDecoded,
    /// A read was attempted and failed. **Never** interpreted as a
    /// permission.
    Unreadable(FsTypeError),
    /// The bits were read.
    Known(FsTypeBits),
}

impl FsType {
    /// The permission value, when one was actually read.
    ///
    /// Returns `None` for every non-`Known` state, which is the API-level
    /// expression of "pdfcer does not guess a permission bit".
    #[must_use]
    pub const fn permission(&self) -> Option<EmbeddingPermission> {
        match self {
            Self::Known(bits) => Some(bits.permission),
            Self::NotApplicable | Self::ProgramNotDecoded | Self::Unreadable(_) => None,
        }
    }
}

/// Read `fsType` out of an sfnt font program.
///
/// `program` is the **decoded** font program — the bytes after the PDF
/// stream's `/Filter` chain has been applied.
///
/// # The read, byte by byte
///
/// 1. `sfntVersion` (`uint32` big-endian at offset 0) must be `0x00010000`
///    (TrueType outlines), `OTTO` (CFF outlines), or the legacy Apple
///    `true`. `ttcf` is a collection and is refused by name rather than
///    silently reading face 0.
/// 2. `numTables` (`uint16` at offset 4); the table directory starts at
///    offset 12 with 16-byte records of `tag`/`checkSum`/`offset`/`length`.
/// 3. Find the record whose tag is `OS/2` — four bytes `4F 53 2F 32`, note
///    the **slash**; the tag is not `OS2`.
/// 4. Inside that table, `version` is a `uint16` at offset 0 and **`fsType`
///    is a `uint16` at offset 8**, in every `OS/2` version 0–5, because the
///    four fields before it are identical across all of them. So `fsType`
///    is readable without knowing the version — the version is read only to
///    apply the difference rules to bits 8 and 9.
///
/// All arithmetic is checked and every slice is bounds-tested: this runs on
/// bytes lifted straight out of an untrusted document.
///
/// # Why this is hand-written rather than delegated
///
/// `pdfcer-render` already reads `fsType`, via `skrifa`, in
/// `font::subset::check_embedding_permission`. That crate boundary is
/// load-bearing (project rule 2 / `ARCHITECTURE.md` §3): `pdfcer-core` must
/// not gain a font-parsing dependency, because `pdfcer-core` is what the
/// eventual WASM shell keeps. Forty lines of bounds-checked table-directory
/// walk is a much smaller cost than a dependency edge in the wrong
/// direction. The two implementations are held to agreement by a test over
/// the shared `fixtures/synthetic/text/subset-fstype-*.ttf` corpus.
///
/// # Errors
///
/// [`FsTypeError`] — every variant is a measurement failure and **none of
/// them may be read as a permission**. In particular, a font with no `OS/2`
/// table is *not* `Installable`: the specification defines no default for
/// the absent case.
///
/// # Examples
///
/// ```
/// use pdfcer_core::fontinfo::{read_fs_type, EmbeddingPermission, FsTypeError};
///
/// // Not a font at all.
/// assert_eq!(read_fs_type(b"%PDF-1.7\n"), Err(FsTypeError::NotSfnt));
///
/// // A real font program reads a real value.
/// let ttf = include_bytes!(
///     "../../../fixtures/synthetic/text/subset-fstype-restricted.ttf"
/// );
/// let bits = read_fs_type(ttf).expect("fixture has an OS/2 table");
/// assert_eq!(bits.permission, EmbeddingPermission::Restricted);
/// ```
pub fn read_fs_type(program: &[u8]) -> Result<FsTypeBits, FsTypeError> {
    let be16 = |at: usize| -> Option<u16> {
        let bytes: [u8; 2] = program.get(at..at.checked_add(2)?)?.try_into().ok()?;
        Some(u16::from_be_bytes(bytes))
    };
    let be32 = |at: usize| -> Option<u32> {
        let bytes: [u8; 4] = program.get(at..at.checked_add(4)?)?.try_into().ok()?;
        Some(u32::from_be_bytes(bytes))
    };

    // Step 1 — the container.
    let magic = be32(0).ok_or(FsTypeError::NotSfnt)?;
    match magic {
        0x0001_0000 | 0x4F54_544F | 0x7472_7565 => {}
        // 'ttcf'. Reading face 0's OS/2 would be a guess about which face
        // the PDF meant, and the PDF does not say.
        0x7474_6366 => return Err(FsTypeError::Collection),
        _ => return Err(FsTypeError::NotSfnt),
    }

    // Step 2 — the table directory.
    let num_tables = usize::from(be16(4).ok_or(FsTypeError::BadTableDirectory)?);
    if num_tables > MAX_SFNT_TABLES {
        return Err(FsTypeError::BadTableDirectory);
    }

    // Step 3 — locate `OS/2`.
    let mut os2: Option<(usize, usize)> = None;
    for i in 0..num_tables {
        let rec = 12usize
            .checked_add(i.checked_mul(16).ok_or(FsTypeError::BadTableDirectory)?)
            .ok_or(FsTypeError::BadTableDirectory)?;
        let tag = program
            .get(rec..rec.checked_add(4).ok_or(FsTypeError::BadTableDirectory)?)
            .ok_or(FsTypeError::BadTableDirectory)?;
        if tag == b"OS/2" {
            let offset = be32(rec + 8).ok_or(FsTypeError::BadTableDirectory)? as usize;
            let length = be32(rec + 12).ok_or(FsTypeError::BadTableDirectory)? as usize;
            os2 = Some((offset, length));
            break;
        }
    }
    let (offset, length) = os2.ok_or(FsTypeError::NoOs2Table)?;

    // Step 4 — read `version` @0 and `fsType` @8.
    //
    // The length check is against the DECLARED table length as well as the
    // buffer: a record claiming a 4-byte OS/2 table is malformed even if
    // the file happens to have ten readable bytes there.
    if length < 10 {
        return Err(FsTypeError::Os2Truncated);
    }
    let end = offset
        .checked_add(length)
        .ok_or(FsTypeError::Os2Truncated)?;
    if end > program.len() {
        return Err(FsTypeError::Os2Truncated);
    }
    let os2_version = be16(offset).ok_or(FsTypeError::Os2Truncated)?;
    let raw = be16(offset.checked_add(8).ok_or(FsTypeError::Os2Truncated)?)
        .ok_or(FsTypeError::Os2Truncated)?;

    Ok(decode_fs_type(raw, os2_version))
}

/// Apply the OpenType version-difference rules to a raw `fsType`.
///
/// Split out from [`read_fs_type`] so the decoding rules can be tested
/// exhaustively over synthetic `(raw, version)` pairs without having to
/// build a font file for each one — there are 65 536 × 6 combinations and
/// the interesting ones are all edge cases.
fn decode_fs_type(raw: u16, os2_version: u16) -> FsTypeBits {
    let sub = raw & 0x000F;
    let reserved_bit0 = sub & 0x0001 != 0;
    // Bit 0 is reserved; the usage value lives in bits 1–3.
    let usage = sub & 0x000E;
    let permission = match usage {
        0 => {
            if reserved_bit0 {
                // `fsType == 1` exactly. The spec documents the historical
                // mistake and states no reader-side interpretation (N2).
                EmbeddingPermission::Unspecified
            } else {
                EmbeddingPermission::Installable
            }
        }
        2 => EmbeddingPermission::Restricted,
        4 => EmbeddingPermission::PreviewPrint,
        8 => EmbeddingPermission::Editable,
        // More than one of bits 1–3.
        _ => EmbeddingPermission::Ambiguous,
    };

    // "Versions 0 to 1: only bits 0 to 3 were assigned. Applications must
    // ignore bits 4 to 15 when reading a version 0 or version 1 table."
    let gated = os2_version <= 1;
    FsTypeBits {
        raw,
        os2_version,
        permission,
        no_subsetting: !gated && raw & 0x0100 != 0,
        bitmap_only: !gated && raw & 0x0200 != 0,
        version_gated_bits_ignored: gated && raw & 0xFF00 != 0,
        reserved_bit0,
    }
}

// ---------------------------------------------------------------------------
// Removability
// ---------------------------------------------------------------------------

/// Whether this font's embedded program could be removed without
/// destroying the text it draws.
///
/// **An enum carrying its reason, not a boolean.** The reason is the whole
/// deliverable: Acrobat reaches the same refusals and communicates them by
/// omitting the font from a list, which tells an operator nothing about
/// why their largest font is not on it. The unembedding Pass consumes this
/// exact value, so a font pdfcer shows as blocked and a font pdfcer declines
/// to unembed are the same set by construction.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Removability {
    /// There is no embedded program. Removal is a no-op, not a success.
    NotEmbedded,
    /// The program can be removed: the character codes' meaning is defined
    /// by the *standard* (a base encoding from Annex D, possibly with
    /// `/Differences` naming standard glyph names), so a substitute face
    /// draws the same characters for the same codes.
    Removable,
    /// **Blocked.** The codes are glyph indices into this exact program
    /// (§9.9's `Identity-H` writer directive), so nothing outside the
    /// program says what they mean. Removing it produces unrenderable or
    /// garbled text.
    BlockedIdentityEncoded {
        /// Whether a `/ToUnicode` CMap is present.
        ///
        /// Present, it means the *text* is recoverable — extraction and
        /// search keep working, and a future re-encoding Pass would have
        /// something to work from. It does **not** make removal safe: a
        /// substitute face's glyph order differs, so the drawing is still
        /// destroyed. Carried here because it changes what an operator can
        /// do next, not because it changes the verdict.
        to_unicode: bool,
    },
    /// **Blocked.** A Type 3 font's glyphs *are* content streams in this
    /// document (`/CharProcs`, §9.6.5). There is no font program to strip,
    /// and no installed face can substitute for procedures that draw
    /// arbitrary graphics.
    BlockedType3,
    /// pdfcer cannot tell. Reported as its own state rather than folded into
    /// either answer — see [`RemovabilityUnknown`].
    Unknown(RemovabilityUnknown),
}

/// Why a removability verdict could not be reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum RemovabilityUnknown {
    /// A simple font with no `/Encoding` (or an encoding dictionary with no
    /// `/BaseEncoding`) whose descriptor flags it **symbolic** (§9.8.2 bit
    /// 3). §9.6.6.1: the built-in encoding governs, and for a symbolic
    /// TrueType font the codes go through the program's own `cmap` — which
    /// is inside the program about to be deleted.
    SymbolicBuiltinEncoding,
    /// A composite font whose CMap is a *predefined* one other than
    /// Identity (e.g. `UniJIS-UCS2-H`). The codes map to CIDs in a named
    /// character collection, so a substitute font *of that collection*
    /// would work — but whether one is available is not knowable from the
    /// document.
    PredefinedCMap,
    /// A composite font with an **embedded** CMap stream. pdfcer does not
    /// parse it here, so what the codes mean is not established.
    EmbeddedCMap,
    /// The `/FontFile*` bytes could not be read, so the font could not be
    /// classified.
    ProgramUnreadable,
    /// A composite font whose `/DescendantFonts` entry is missing or
    /// malformed, so there is no descendant to classify.
    NoDescendant,
    /// The `/Subtype` is absent or one pdfcer does not model.
    UnknownSubtype,
}

impl Removability {
    /// Whether the verdict permits removal.
    ///
    /// `false` for everything except [`Self::Removable`] — including
    /// [`Self::NotEmbedded`], where removal is meaningless rather than
    /// permitted, and [`Self::Unknown`], where the conservative answer is
    /// the honest one.
    #[must_use]
    pub const fn is_removable(&self) -> bool {
        matches!(self, Self::Removable)
    }

    /// A stable, locale-invariant token for a machine-readable listing.
    ///
    /// # Examples
    ///
    /// ```
    /// use pdfcer_core::fontinfo::Removability;
    ///
    /// assert_eq!(Removability::Removable.token(), "removable");
    /// assert_eq!(Removability::BlockedType3.token(), "blocked-type3");
    /// ```
    #[must_use]
    pub const fn token(&self) -> &'static str {
        match self {
            Self::NotEmbedded => "not-embedded",
            Self::Removable => "removable",
            Self::BlockedIdentityEncoded { .. } => "blocked-identity",
            Self::BlockedType3 => "blocked-type3",
            Self::Unknown(why) => match why {
                RemovabilityUnknown::SymbolicBuiltinEncoding => "unknown-symbolic-builtin",
                RemovabilityUnknown::PredefinedCMap => "unknown-predefined-cmap",
                RemovabilityUnknown::EmbeddedCMap => "unknown-embedded-cmap",
                RemovabilityUnknown::ProgramUnreadable => "unknown-program-unreadable",
                RemovabilityUnknown::NoDescendant => "unknown-no-descendant",
                RemovabilityUnknown::UnknownSubtype => "unknown-subtype",
            },
        }
    }

    /// The sentence an operator reads — the whole reason this is an enum.
    ///
    /// Written as a statement about **the file**, never about pdfcer's
    /// abilities: "this font's text is stored as glyph indices" is
    /// actionable; "pdfcer cannot remove this" is not.
    #[must_use]
    pub const fn reason(&self) -> &'static str {
        match self {
            Self::NotEmbedded => {
                "No font program is embedded for this font, so there is nothing to remove. \
                 The document already relies on a viewer-side substitute."
            }
            Self::Removable => {
                "The character codes mean the same thing to any font, because the encoding \
                 is defined by the PDF standard rather than by the embedded program."
            }
            Self::BlockedIdentityEncoded { to_unicode: true } => {
                "This font's text is stored as glyph indices into this exact embedded \
                 program. A /ToUnicode map means the text can still be extracted, but no \
                 substitute font would draw it correctly — a different build of the same \
                 typeface numbers its glyphs differently."
            }
            Self::BlockedIdentityEncoded { to_unicode: false } => {
                "This font's text is stored as glyph indices into this exact embedded \
                 program, and there is no /ToUnicode map. Removing the program would make \
                 the text neither renderable nor recoverable."
            }
            Self::BlockedType3 => {
                "A Type 3 font's glyphs are drawing procedures inside this document, not an \
                 embedded font program. There is no program to remove, and no installed \
                 font could stand in for them."
            }
            Self::Unknown(RemovabilityUnknown::SymbolicBuiltinEncoding) => {
                "This font declares no standard encoding and is flagged symbolic, so what \
                 its character codes mean is defined inside the embedded program itself."
            }
            Self::Unknown(RemovabilityUnknown::PredefinedCMap) => {
                "This font uses a predefined CMap for a named character collection. A \
                 substitute font built for the same collection would work; whether one is \
                 installed is not something this document can say."
            }
            Self::Unknown(RemovabilityUnknown::EmbeddedCMap) => {
                "This font's encoding is an embedded CMap stream, which pdfcer does not \
                 interpret here, so what the character codes mean has not been established."
            }
            Self::Unknown(RemovabilityUnknown::ProgramUnreadable) => {
                "A font program is declared but its bytes could not be read, so this font \
                 could not be classified. The document is damaged in this respect."
            }
            Self::Unknown(RemovabilityUnknown::NoDescendant) => {
                "This is a composite font whose descendant CIDFont is missing or malformed, \
                 so there is no glyph source to classify."
            }
            Self::Unknown(RemovabilityUnknown::UnknownSubtype) => {
                "This font dictionary declares no /Subtype, or one pdfcer does not model, so \
                 how its character codes reach glyphs is not established."
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Coverage
// ---------------------------------------------------------------------------

/// A place a font can be reached from.
///
/// Exists so [`SurfaceCoverage`] can *name* what was and was not searched.
/// A font inventory is only as good as its reachability story, and a
/// confident list built on a partial walk is worse than an admitted one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum Surface {
    /// A page's `/Resources /Font`, with §7.7.3.4 inheritance resolved.
    PageResources,
    /// A form XObject's own `/Resources`, at any nesting depth.
    FormXObject,
    /// A tiling pattern's `/Resources` (a pattern is a content stream).
    Pattern,
    /// A soft-mask group XObject reached through `/ExtGState /SMask /G`.
    SoftMaskGroup,
    /// A Type 3 font's own `/Resources`, which its `/CharProcs` draw
    /// against and which may name further fonts.
    Type3CharProcs,
    /// The AcroForm's `/DR /Font` default resources (§12.7.2, Table 218) —
    /// the fonts a field's `/DA` string names.
    AcroFormDefaultResources,
    /// An annotation appearance stream's own `/Resources`, through `/AP`
    /// `/N`, `/R` and `/D` including appearance-state subdictionaries.
    AnnotationAppearance,
    /// **Not walked.** Font dictionaries present in the file but reachable
    /// from none of the surfaces above. They still occupy bytes, so their
    /// absence from the report is a real limitation — stated rather than
    /// left to be discovered.
    UnreferencedObjects,
}

impl Surface {
    /// Every surface, in report order.
    pub const ALL: [Self; 8] = [
        Self::PageResources,
        Self::FormXObject,
        Self::Pattern,
        Self::SoftMaskGroup,
        Self::Type3CharProcs,
        Self::AcroFormDefaultResources,
        Self::AnnotationAppearance,
        Self::UnreferencedObjects,
    ];

    /// A stable, locale-invariant token for machine-readable output.
    #[must_use]
    pub const fn token(self) -> &'static str {
        match self {
            Self::PageResources => "page",
            Self::FormXObject => "form-xobject",
            Self::Pattern => "pattern",
            Self::SoftMaskGroup => "softmask",
            Self::Type3CharProcs => "type3-charprocs",
            Self::AcroFormDefaultResources => "acroform-dr",
            Self::AnnotationAppearance => "annotation-ap",
            Self::UnreferencedObjects => "unreferenced",
        }
    }
}

/// Which font-bearing surfaces this inventory searched.
///
/// Returned with every [`FontInventory`] and intended to be *displayed*,
/// not merely available. The one surface pdfcer does not walk
/// ([`Surface::UnreferencedObjects`]) is reported through
/// [`Self::not_walked`] for exactly the same reason the walked ones are
/// reported: an operator deciding what to delete needs to know the shape of
/// the evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct SurfaceCoverage {
    /// Page `/Resources /Font`, inheritance resolved.
    pub page_resources: bool,
    /// Form XObjects' nested `/Resources`.
    pub form_xobjects: bool,
    /// Tiling patterns' `/Resources`.
    pub patterns: bool,
    /// `/ExtGState /SMask /G` group XObjects' `/Resources`.
    pub soft_mask_groups: bool,
    /// Type 3 fonts' own `/Resources`.
    pub type3_charprocs: bool,
    /// The AcroForm `/DR /Font`.
    pub acroform_default_resources: bool,
    /// Annotation appearance streams' `/Resources`.
    pub annotation_appearances: bool,
    /// Font objects reachable from nothing. **Always `false`** — see
    /// [`Surface::UnreferencedObjects`].
    pub unreferenced_objects: bool,
}

impl SurfaceCoverage {
    /// What this build of pdfcer actually walks.
    const WALKED: Self = Self {
        page_resources: true,
        form_xobjects: true,
        patterns: true,
        soft_mask_groups: true,
        type3_charprocs: true,
        acroform_default_resources: true,
        annotation_appearances: true,
        unreferenced_objects: false,
    };

    /// Whether `surface` was searched.
    #[must_use]
    pub const fn includes(&self, surface: Surface) -> bool {
        match surface {
            Surface::PageResources => self.page_resources,
            Surface::FormXObject => self.form_xobjects,
            Surface::Pattern => self.patterns,
            Surface::SoftMaskGroup => self.soft_mask_groups,
            Surface::Type3CharProcs => self.type3_charprocs,
            Surface::AcroFormDefaultResources => self.acroform_default_resources,
            Surface::AnnotationAppearance => self.annotation_appearances,
            Surface::UnreferencedObjects => self.unreferenced_objects,
        }
    }

    /// The surfaces that were searched, in report order.
    #[must_use]
    pub fn walked(&self) -> Vec<Surface> {
        Surface::ALL
            .into_iter()
            .filter(|s| self.includes(*s))
            .collect()
    }

    /// The surfaces that were **not** searched, in report order.
    ///
    /// Never empty for the shipped configuration, and that is deliberate:
    /// a report that could claim total coverage would be claiming something
    /// no PDF reader can honestly claim.
    ///
    /// # Examples
    ///
    /// ```
    /// use pdfcer_core::fontinfo::{Surface, SurfaceCoverage};
    /// use pdfcer_core::document::Document;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let doc = Document::from_bytes(
    ///     include_bytes!("../../../fixtures/synthetic/hello.pdf").to_vec(),
    /// )?;
    /// let inv = pdfcer_core::fontinfo::inventory(&doc.view());
    /// assert_eq!(inv.coverage.not_walked(), vec![Surface::UnreferencedObjects]);
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn not_walked(&self) -> Vec<Surface> {
        Surface::ALL
            .into_iter()
            .filter(|s| !self.includes(*s))
            .collect()
    }
}

impl Default for SurfaceCoverage {
    fn default() -> Self {
        Self::WALKED
    }
}

// ---------------------------------------------------------------------------
// The record and the inventory
// ---------------------------------------------------------------------------

/// One distinct font, however many places reach it.
///
/// Deduplicated by object identity: a font object referenced from forty
/// pages is **one** record listing forty pages, not forty records. That is
/// not cosmetic — an operator sizing a cleanup needs the program's bytes
/// counted once.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct FontRecord {
    /// The font dictionary's object identity, or `None` when the resource
    /// entry held a **direct** dictionary rather than a reference. Direct
    /// font dictionaries cannot be deduplicated (they have no identity), so
    /// each occurrence becomes its own record and
    /// [`FontDiagnostics::direct_font_dicts`] counts them.
    pub id: Option<ObjId>,
    /// `/BaseFont` exactly as the file spells it, including any subset tag.
    /// `None` when the key is absent — required for every subtype except
    /// Type 3, so its absence is a malformation, not a default.
    pub base_font: Option<String>,
    /// The six-uppercase-letter subset tag (§9.6.4), without the `+`, when
    /// `/BaseFont` carries one.
    pub subset_tag: Option<String>,
    /// The font dictionary's `/Subtype`.
    pub subtype: FontSubtype,
    /// For a `Type0` font, the descendant CIDFont's `/Subtype`.
    pub descendant_subtype: Option<FontSubtype>,
    /// The `/Encoding` entry, classified.
    pub encoding: Encoding,
    /// Whether a `/FontDescriptor` was found — on the descendant for a
    /// composite font, on the font dictionary itself otherwise.
    ///
    /// Absent is legitimate for the standard 14 fonts (§9.6.2.2) and is a
    /// malformation for anything else.
    pub descriptor_present: bool,
    /// The descriptor's `/Flags` Symbolic bit (§9.8.2, bit 3, value 4), or
    /// `None` when there is no descriptor to read it from.
    pub symbolic: Option<bool>,
    /// Whether the font is embedded, and how big it is.
    pub program: Program,
    /// Whether the font dictionary carries a `/ToUnicode` CMap (§9.10.3).
    pub has_to_unicode: bool,
    /// Whether `/BaseFont` names one of the standard 14 fonts — see
    /// [`is_standard_14`]. An absent descriptor is legitimate for these and
    /// a malformation for anything else, and they are the class the parity
    /// reference treats as always-safe to unembed.
    pub standard_14: bool,
    /// The verdict the removal Pass will consume.
    pub removability: Removability,
    /// 1-based page numbers this font is reachable from, sorted and
    /// deduplicated. Empty for a font reached only from the AcroForm `/DR`.
    pub pages: Vec<u32>,
    /// Which surfaces reached it, sorted and deduplicated.
    pub surfaces: Vec<Surface>,
    /// The resource names it was bound to (`/F1`, `/Helv`), sorted,
    /// deduplicated and capped at [`MAX_RESOURCE_NAMES_PER_FONT`].
    pub resource_names: Vec<String>,
    /// Whether [`Self::resource_names`] hit its cap.
    pub resource_names_truncated: bool,
}

impl FontRecord {
    /// Whether `/BaseFont` carries an ISO 32000-1 §9.6.4 subset tag.
    #[must_use]
    pub const fn is_subset(&self) -> bool {
        self.subset_tag.is_some()
    }

    /// `/BaseFont` with any subset tag removed — the name an operator
    /// recognises, and the name a substitution would match against.
    ///
    /// # Examples
    ///
    /// ```
    /// use pdfcer_core::fontinfo::split_subset_tag;
    ///
    /// assert_eq!(split_subset_tag("ABCDEF+Arial"), (Some("ABCDEF"), "Arial"));
    /// assert_eq!(split_subset_tag("Arial"), (None, "Arial"));
    /// // Exactly six UPPERCASE letters, or it is part of the name.
    /// assert_eq!(split_subset_tag("ABCDE+Arial"), (None, "ABCDE+Arial"));
    /// assert_eq!(split_subset_tag("AbCdEf+Arial"), (None, "AbCdEf+Arial"));
    /// ```
    #[must_use]
    pub fn family_name(&self) -> Option<&str> {
        self.base_font
            .as_deref()
            .map(|full| split_subset_tag(full).1)
    }

    /// The stored bytes this font's program occupies, or `0`.
    #[must_use]
    pub const fn stored_bytes(&self) -> usize {
        self.program.stored_bytes()
    }
}

/// Split a `/BaseFont` name into its subset tag and its family name.
///
/// ISO 32000-1 §9.6.4: the name "shall begin with a tag followed by a plus
/// sign (`+`). The tag shall consist of **exactly six uppercase letters**;
/// the choice of letters is arbitrary."
///
/// The rule is applied strictly, in both directions. Five letters, seven
/// letters, any lowercase letter, or a digit means there is **no** tag and
/// the whole string is the name — because a font genuinely called
/// `AB+Condensed` exists as a possibility and mangling it would be pdfcer
/// inventing data. Equally, `ABCDEF+` with an empty remainder yields an
/// empty family name rather than a tag-less fallback, because the file
/// really does say the family part is empty.
///
/// # Examples
///
/// ```
/// use pdfcer_core::fontinfo::split_subset_tag;
///
/// assert_eq!(split_subset_tag("QWERTY+TimesNewRoman"), (Some("QWERTY"), "TimesNewRoman"));
/// assert_eq!(split_subset_tag("Helvetica"), (None, "Helvetica"));
/// assert_eq!(split_subset_tag("ABCDEF+Noto Sans JP"), (Some("ABCDEF"), "Noto Sans JP"));
/// ```
#[must_use]
pub fn split_subset_tag(base_font: &str) -> (Option<&str>, &str) {
    // Every access is fallible: the input is a `/BaseFont` name lifted out
    // of an untrusted document, and it may be empty, shorter than the tag,
    // or non-ASCII. `split_at_checked` also guarantees the split lands on a
    // char boundary, which raw slicing would not — a name whose seventh
    // byte falls inside a multi-byte UTF-8 sequence would otherwise panic.
    let bytes = base_font.as_bytes();
    let is_tagged = bytes.len() > 7
        && bytes.get(6) == Some(&b'+')
        && bytes
            .get(..6)
            .is_some_and(|tag| tag.iter().all(u8::is_ascii_uppercase));
    if is_tagged
        && let Some((tag, rest)) = base_font.split_at_checked(6)
        && let Some(family) = rest.strip_prefix('+')
    {
        (Some(tag), family)
    } else {
        (None, base_font)
    }
}

/// Whether `base_font` names one of the **standard 14** fonts (§9.6.2.2,
/// Table 109) — the faces "any conforming reader shall support", for which
/// an absent `/FontDescriptor` is legitimate rather than a malformation.
///
/// Matched against the de-prefixed name, so a (nonsensical but legal)
/// `ABCDEF+Helvetica` still counts. The comparison is exact and
/// case-sensitive: these are PostScript names, and `helvetica` is not one of
/// them.
///
/// Why this is public: the standard 14 are the one font class the parity
/// reference treats as *always* safe to unembed, on the reasoning that every
/// conforming viewer ships equivalent metrics
/// (`Acrobat_Features/optimize__font_unembedding.md`). The removal Pass will
/// want this predicate, and having two spellings of "is this Helvetica"
/// would be exactly the drift this module's shared-classification design
/// exists to prevent.
///
/// # Examples
///
/// ```
/// use pdfcer_core::fontinfo::is_standard_14;
///
/// assert!(is_standard_14("Helvetica-BoldOblique"));
/// assert!(is_standard_14("ZapfDingbats"));
/// assert!(!is_standard_14("Arial"));
/// assert!(!is_standard_14("helvetica"));
/// ```
#[must_use]
pub fn is_standard_14(base_font: &str) -> bool {
    matches!(
        split_subset_tag(base_font).1,
        "Courier"
            | "Courier-Bold"
            | "Courier-Oblique"
            | "Courier-BoldOblique"
            | "Helvetica"
            | "Helvetica-Bold"
            | "Helvetica-Oblique"
            | "Helvetica-BoldOblique"
            | "Times-Roman"
            | "Times-Bold"
            | "Times-Italic"
            | "Times-BoldItalic"
            | "Symbol"
            | "ZapfDingbats"
    )
}

/// Collapse a sorted, unique page list into `1-4,9,12-14`.
///
/// A font in a real document is used on *every* page, and four hundred
/// comma-separated numbers on one line is not a report — it buries every
/// field beside it. Ranges keep the answer complete (nothing is elided)
/// while staying scannable.
///
/// Lives here rather than in either shell so `pdfcer`'s listing and
/// `pdfce-gui`'s panel cannot come to disagree about what "pages 1-4" means.
/// The output is locale-invariant and stable, which is what a batch sweep
/// parsing the CLI's lines depends on.
///
/// Input is assumed sorted and deduplicated — which is what
/// [`FontRecord::pages`] guarantees. An unsorted input is not an error and
/// does not panic; it simply produces more groups.
///
/// # Examples
///
/// ```
/// use pdfcer_core::fontinfo::format_page_ranges;
///
/// assert_eq!(format_page_ranges(&[1, 2, 3, 4, 9, 12, 13, 14]), "1-4,9,12-14");
/// assert_eq!(format_page_ranges(&[7]), "7");
/// assert_eq!(format_page_ranges(&[]), "-");
/// ```
#[must_use]
pub fn format_page_ranges(pages: &[u32]) -> String {
    let Some(first) = pages.first() else {
        return "-".to_owned();
    };
    let mut out = String::new();
    let mut start = *first;
    let mut end = *first;
    // Zipped against its own tail rather than indexed: every access is
    // proven in bounds by the iterators, which is the crate's standing
    // preference on a path fed by document data. (`windows(2)` would need
    // `pair[0]`/`pair[1]`, which clippy's `indexing_slicing` denies here for
    // exactly that reason, even though the slice length is known.)
    for (prev, next) in pages.iter().zip(pages.iter().skip(1)) {
        if *next == prev.saturating_add(1) {
            end = *next;
            continue;
        }
        push_range(&mut out, start, end);
        start = *next;
        end = *next;
    }
    push_range(&mut out, start, end);
    out
}

/// Append one `n` or `n-m` group to a comma-separated list.
fn push_range(out: &mut String, start: u32, end: u32) {
    if !out.is_empty() {
        out.push(',');
    }
    if start == end {
        out.push_str(&start.to_string());
    } else {
        out.push_str(&format!("{start}-{end}"));
    }
}

/// Things that went wrong, or were cut short, while building an inventory.
///
/// Every field is a fact about the *document* or about a pdfcer ceiling that
/// fired — never a silent degradation. A truncated sweep that printed a
/// tidy list would be the exact defect this module's coverage discipline
/// exists to prevent.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct FontDiagnostics {
    /// [`MAX_RESOURCE_NODES`] was exhausted; some resource dictionaries
    /// were not entered and fonts reachable only through them are missing.
    pub resource_scan_truncated: bool,
    /// [`MAX_FONTS`] was reached; the list is incomplete.
    pub font_limit_reached: bool,
    /// The page tree would not walk, so no page-reachable font was found.
    /// The AcroForm `/DR` sweep still ran.
    pub page_scan_failed: bool,
    /// Resource entries that held a **direct** font dictionary rather than
    /// a reference. Legal, but they have no identity, so they cannot be
    /// deduplicated across pages.
    pub direct_font_dicts: usize,
    /// `/Font` entries whose reference resolved to nothing (§7.3.10 makes
    /// that a `null`, not an error).
    pub dangling_font_references: usize,
    /// Fonts with no `/FontDescriptor` **that needed one**. The standard 14
    /// (§9.6.2.2) and Type 3 fonts are exempt and are not counted, so a
    /// non-zero value here is always a real malformation.
    pub descriptors_missing: usize,
    /// Declared `/FontFile*` programs whose bytes could not be reached.
    pub programs_unreadable: usize,
    /// Declared `/FontFile*` programs whose filter chain did not decode.
    /// Their stored size is still known; their `fsType` is not.
    pub programs_undecodable: usize,
    /// Composite fonts whose `/DescendantFonts` was missing or malformed.
    pub descendants_missing: usize,
}

impl FontDiagnostics {
    /// Whether nothing at all went wrong.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        *self == Self::default()
    }
}

/// A document's complete font inventory.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct FontInventory {
    /// Every distinct font found, ordered by first discovery so the listing
    /// is stable across runs.
    pub fonts: Vec<FontRecord>,
    /// Which font-bearing surfaces were searched — and which were not.
    pub coverage: SurfaceCoverage,
    /// What went wrong or was cut short.
    pub diagnostics: FontDiagnostics,
}

impl FontInventory {
    /// How many fonts have a measurable embedded program.
    #[must_use]
    pub fn embedded_count(&self) -> usize {
        self.fonts
            .iter()
            .filter(|f| f.program.is_embedded())
            .count()
    }

    /// Total stored bytes of every embedded program, counted **once per
    /// distinct font object**.
    ///
    /// ★ The document-level figure Acrobat's Audit Space Usage gives as one
    /// aggregate bucket, computed here from the same per-font numbers the
    /// listing shows — so the total and the rows cannot disagree.
    #[must_use]
    pub fn embedded_bytes(&self) -> u64 {
        self.fonts.iter().map(|f| f.stored_bytes() as u64).sum()
    }

    /// How many fonts carry each removability verdict, keyed by
    /// [`Removability::token`].
    ///
    /// The shape a corpus sweep or a summary line wants.
    #[must_use]
    pub fn verdict_counts(&self) -> BTreeMap<&'static str, usize> {
        let mut out = BTreeMap::new();
        for f in &self.fonts {
            *out.entry(f.removability.token()).or_insert(0) += 1;
        }
        out
    }
}

// ---------------------------------------------------------------------------
// The sweep
// ---------------------------------------------------------------------------

/// One resource dictionary waiting to be entered, with where it came from.
struct Node {
    resources: Dict,
    /// 1-based page number, when this node is reachable from a page.
    page: Option<u32>,
    surface: Surface,
}

/// Build a document's font inventory.
///
/// Takes a [`DocumentView`] rather than a `&Document` so it works
/// identically over a loaded file and over an
/// [`EditSession`](crate::edit::EditSession)'s overlay — and, critically,
/// so stream spans resolve against the right byte source in both cases. A
/// session's staged payloads live past the end of the base buffer, and
/// slicing them against `document().bytes()` would silently measure the
/// wrong bytes.
///
/// # Never fails
///
/// Deliberately infallible. Every structural fault a document can present
/// — an unwalkable page tree, a dangling `/Font` reference, a `/FontFile2`
/// pointing at a non-stream, a `/DescendantFonts` that is not an array —
/// degrades into a [`FontDiagnostics`] counter or a
/// [`Removability::Unknown`] verdict, because refusing the whole inventory
/// over one damaged font would cost the operator every *undamaged* one.
/// That is the `ARCHITECTURE.md` §10 fail-clean posture applied to a
/// report.
///
/// # Examples
///
/// ```
/// use pdfcer_core::document::Document;
/// use pdfcer_core::fontinfo::{inventory, Removability};
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let doc = Document::from_bytes(
///     include_bytes!("../../../fixtures/synthetic/text/cidfonttype2-nocmap-embedded.pdf")
///         .to_vec(),
/// )?;
/// let inv = inventory(&doc.view());
/// // An Identity-H composite font with no /ToUnicode is the case the whole
/// // report exists for: removing its program destroys the text irrecoverably.
/// assert!(inv.fonts.iter().any(|f| matches!(
///     f.removability,
///     Removability::BlockedIdentityEncoded { to_unicode: false }
/// )));
/// # Ok(())
/// # }
/// ```
#[must_use]
pub fn inventory(view: &DocumentView<'_>) -> FontInventory {
    let mut sweep = Sweep {
        view,
        records: Vec::new(),
        by_id: BTreeMap::new(),
        diagnostics: FontDiagnostics::default(),
        visited: BTreeSet::new(),
        budget: MAX_RESOURCE_NODES,
        queue: Vec::new(),
    };

    sweep.seed_pages();
    sweep.seed_acroform_dr();
    sweep.drain();

    FontInventory {
        fonts: sweep.records,
        coverage: SurfaceCoverage::WALKED,
        diagnostics: sweep.diagnostics,
    }
}

/// The mutable state of one inventory sweep.
struct Sweep<'a, 'v> {
    view: &'a DocumentView<'v>,
    records: Vec<FontRecord>,
    /// Index into `records` by font-object identity, so a font reached from
    /// many places is merged rather than repeated.
    by_id: BTreeMap<ObjId, usize>,
    diagnostics: FontDiagnostics,
    /// Resource/XObject dictionaries already entered, so a cyclic
    /// `/Resources` graph terminates.
    visited: BTreeSet<ObjId>,
    budget: usize,
    queue: Vec<Node>,
}

impl Sweep<'_, '_> {
    /// Queue every page's resolved `/Resources`, and every annotation
    /// appearance stream's own `/Resources`.
    fn seed_pages(&mut self) {
        let Ok(pages) = pages_in(self.view) else {
            self.diagnostics.page_scan_failed = true;
            return;
        };
        for (index, page) in pages.iter().enumerate() {
            // `usize` → `u32`: a document with more than 4 billion pages is
            // not reachable in this address space; saturating keeps the
            // arithmetic total rather than relying on that.
            let number = u32::try_from(index.saturating_add(1)).unwrap_or(u32::MAX);
            self.queue.push(Node {
                resources: page.resources.clone(),
                page: Some(number),
                surface: Surface::PageResources,
            });
            self.seed_annotation_appearances(page.id, number);
        }
    }

    /// Queue the `/Resources` of every appearance stream on one page.
    ///
    /// §12.5.5: `/AP` has up to three entries (`/N` normal, `/R` rollover,
    /// `/D` down), and each may be **either** a stream **or** a
    /// subdictionary keyed by appearance state. All three are walked, and
    /// both shapes, because a font that only ever appears in a rollover
    /// appearance is still a font in the file — and because "which
    /// appearance is currently selected" is a rendering question, not an
    /// inventory one.
    fn seed_annotation_appearances(&mut self, page_id: ObjId, page: u32) {
        let Some(page_dict) = self.view.resolved(page_id).as_dict() else {
            return;
        };
        let Some(annots) = page_dict
            .get(b"Annots")
            .map(|o| self.view.resolve(o))
            .and_then(Object::as_array)
        else {
            return;
        };
        // Cloned so the borrow of `self.view` ends before `self` is mutated
        // below. The alternative — restructuring every accessor to take
        // `&Dict` — would spread this concern across the module.
        let entries: Vec<Object> = annots.to_vec();
        for entry in entries {
            let Some(annot) = self.view.resolve(&entry).as_dict() else {
                continue;
            };
            let Some(ap) = annot
                .get(b"AP")
                .map(|o| self.view.resolve(o))
                .and_then(Object::as_dict)
            else {
                continue;
            };
            let mut streams: Vec<Object> = Vec::new();
            for key in [b"N".as_slice(), b"R", b"D"] {
                let Some(value) = ap.get(key) else { continue };
                match self.view.resolve(value) {
                    // A single appearance stream.
                    Object::Stream(_) => streams.push(value.clone()),
                    // A subdictionary of appearance states.
                    Object::Dict(states) => {
                        for (_, state) in &states.0 {
                            streams.push(state.clone());
                        }
                    }
                    _ => {}
                }
            }
            for s in streams {
                let id = s.as_reference();
                let Object::Stream(stream) = self.view.resolve(&s) else {
                    continue;
                };
                // Appearance streams are form XObjects and nest like any
                // other, so mark them visited by identity to stop a shared
                // one being entered once per annotation.
                if let Some(id) = id
                    && !self.visited.insert(id)
                {
                    continue;
                }
                if let Some(res) = stream
                    .dict
                    .get(b"Resources")
                    .map(|o| self.view.resolve(o))
                    .and_then(Object::as_dict)
                {
                    self.queue.push(Node {
                        resources: res.clone(),
                        page: Some(page),
                        surface: Surface::AnnotationAppearance,
                    });
                }
            }
        }
    }

    /// Queue the AcroForm's `/DR` default-resource dictionary.
    ///
    /// §12.7.2 Table 218: `/DR` is "a resource dictionary containing
    /// default resources (such as fonts, patterns, or colour spaces) that
    /// shall be used by form field appearance streams." A field's `/DA`
    /// string names a font from here, so a form's fonts can live *only*
    /// here — reachable from no page at all until an appearance is
    /// generated. Missing this surface is the specific gap
    /// `optimize__font_reporting.md` flags as unconfirmed in Acrobat.
    fn seed_acroform_dr(&mut self) {
        let Some(catalog) = self.view.catalog_dict() else {
            return;
        };
        let Some(acroform) = catalog
            .get(b"AcroForm")
            .map(|o| self.view.resolve(o))
            .and_then(Object::as_dict)
        else {
            return;
        };
        let Some(dr) = acroform
            .get(b"DR")
            .map(|o| self.view.resolve(o))
            .and_then(Object::as_dict)
        else {
            return;
        };
        self.queue.push(Node {
            resources: dr.clone(),
            page: None,
            surface: Surface::AcroFormDefaultResources,
        });
    }

    /// Work the queue until it is empty or the node budget is exhausted.
    fn drain(&mut self) {
        while let Some(node) = self.queue.pop() {
            if self.budget == 0 {
                self.diagnostics.resource_scan_truncated = true;
                break;
            }
            self.budget -= 1;
            self.enter(&node);
        }
    }

    /// Record every font in one resource dictionary and queue its nested
    /// resource-bearing children.
    fn enter(&mut self, node: &Node) {
        self.collect_fonts(node);
        self.queue_children(node);
    }

    /// Record the `/Font` subdictionary's entries.
    fn collect_fonts(&mut self, node: &Node) {
        let Some(fonts) = node
            .resources
            .get(b"Font")
            .map(|o| self.view.resolve(o))
            .and_then(Object::as_dict)
        else {
            return;
        };
        let entries: Vec<(Name, Object)> = fonts.0.clone();
        for (name, value) in entries {
            let resource_name = String::from_utf8_lossy(name.as_bytes()).into_owned();
            let id = value.as_reference();
            let resolved = self.view.resolve(&value);
            let Some(dict) = resolved.as_dict() else {
                if id.is_some() {
                    self.diagnostics.dangling_font_references += 1;
                }
                continue;
            };
            let dict = dict.clone();
            self.record(id, &dict, node, &resource_name);
        }
    }

    /// Merge one font into the records, or create a new one.
    fn record(&mut self, id: Option<ObjId>, dict: &Dict, node: &Node, resource_name: &str) {
        if let Some(id) = id
            && let Some(&index) = self.by_id.get(&id)
            && let Some(existing) = self.records.get_mut(index)
        {
            merge_use(existing, node, resource_name);
            return;
        }
        if self.records.len() >= MAX_FONTS {
            self.diagnostics.font_limit_reached = true;
            return;
        }
        if id.is_none() {
            self.diagnostics.direct_font_dicts += 1;
        }
        let mut record = self.model(id, dict);
        merge_use(&mut record, node, resource_name);
        if let Some(id) = id {
            self.by_id.insert(id, self.records.len());
        }
        // A Type 3 font's own `/Resources` is a further font-bearing
        // surface; queue it now that the font is known to be Type 3.
        if record.subtype == FontSubtype::Type3
            && let Some(res) = dict
                .get(b"Resources")
                .map(|o| self.view.resolve(o))
                .and_then(Object::as_dict)
        {
            self.queue.push(Node {
                resources: res.clone(),
                page: node.page,
                surface: Surface::Type3CharProcs,
            });
        }
        self.records.push(record);
    }

    /// Build a [`FontRecord`] from one font dictionary.
    fn model(&mut self, id: Option<ObjId>, dict: &Dict) -> FontRecord {
        let base_font = dict
            .get(b"BaseFont")
            .map(|o| self.view.resolve(o))
            .and_then(Object::as_name)
            .map(|n| String::from_utf8_lossy(n.as_bytes()).into_owned());
        let subset_tag = base_font
            .as_deref()
            .and_then(|b| split_subset_tag(b).0)
            .map(str::to_owned);

        let subtype = dict
            .get(b"Subtype")
            .map(|o| self.view.resolve(o))
            .and_then(Object::as_name)
            .map_or(FontSubtype::Absent, FontSubtype::from_name);

        let encoding = self.model_encoding(dict);
        let has_to_unicode = dict.contains_key(b"ToUnicode");

        // §9.8.1: a font descriptor SHALL NOT be used with a Type 0 font.
        // For a composite font the descriptor — and therefore the embedded
        // program — hangs off the descendant CIDFont. Looking on the parent
        // finds nothing and would report "not embedded" about an embedded
        // font.
        let (glyph_source, descendant_subtype, descendant_missing) = if subtype.is_composite() {
            match self.descendant(dict) {
                Some((d_dict, d_subtype)) => (Some(d_dict), Some(d_subtype), false),
                None => (None, None, true),
            }
        } else {
            (Some(dict.clone()), None, false)
        };
        if descendant_missing {
            self.diagnostics.descendants_missing += 1;
        }

        let descriptor = glyph_source.as_ref().and_then(|d| {
            d.get(b"FontDescriptor")
                .map(|o| self.view.resolve(o))
                .and_then(Object::as_dict)
                .cloned()
        });
        let descriptor_present = descriptor.is_some();
        let standard_14 = base_font.as_deref().is_some_and(is_standard_14);
        // §9.6.2.2: the standard 14 fonts need no descriptor, so their
        // absence there is the *conforming* case and counting it as a fault
        // would make the diagnostics fire on every ordinary Helvetica
        // document. Type 3 fonts are exempt for the same structural reason
        // (Table 111 does not require one).
        if !descriptor_present
            && !descendant_missing
            && !standard_14
            && subtype != FontSubtype::Type3
        {
            self.diagnostics.descriptors_missing += 1;
        }
        // §9.8.2 Table 123: bit 3 (value 4) is Symbolic.
        let symbolic = descriptor.as_ref().and_then(|d| {
            d.get(b"Flags")
                .map(|o| self.view.resolve(o))
                .and_then(Object::as_int)
                .map(|flags| flags & 4 != 0)
        });

        let program = descriptor
            .as_ref()
            .map_or(Program::NotEmbedded, |d| self.model_program(d));
        match &program {
            Program::Unreadable { .. } => self.diagnostics.programs_unreadable += 1,
            Program::Embedded(p) if p.decoded_bytes.is_none() => {
                self.diagnostics.programs_undecodable += 1;
            }
            _ => {}
        }

        let removability = classify(
            &subtype,
            descendant_subtype.as_ref(),
            &encoding,
            &program,
            symbolic,
            has_to_unicode,
            descendant_missing,
        );

        FontRecord {
            id,
            base_font,
            subset_tag,
            subtype,
            descendant_subtype,
            encoding,
            descriptor_present,
            symbolic,
            program,
            has_to_unicode,
            standard_14,
            removability,
            pages: Vec::new(),
            surfaces: Vec::new(),
            resource_names: Vec::new(),
            resource_names_truncated: false,
        }
    }

    /// The descendant CIDFont of a `Type0` font (§9.7.6, Table 121).
    ///
    /// `/DescendantFonts` is "an array specifying **one** CIDFont
    /// dictionary" — a one-element array, always. Files in the wild
    /// occasionally write the dictionary directly instead of wrapping it;
    /// both shapes are accepted here, because refusing the unwrapped form
    /// would report an embedded font as having no glyph source over a
    /// wrapper that carries no information.
    fn descendant(&self, dict: &Dict) -> Option<(Dict, FontSubtype)> {
        let entry = dict.get(b"DescendantFonts")?;
        let resolved = self.view.resolve(entry);
        let d = match resolved {
            Object::Array(items) => self.view.resolve(items.first()?).as_dict()?,
            Object::Dict(d) => d,
            _ => return None,
        };
        let subtype = d
            .get(b"Subtype")
            .map(|o| self.view.resolve(o))
            .and_then(Object::as_name)
            .map_or(FontSubtype::Absent, FontSubtype::from_name);
        Some((d.clone(), subtype))
    }

    /// Classify a font dictionary's `/Encoding`.
    fn model_encoding(&self, dict: &Dict) -> Encoding {
        let Some(entry) = dict.get(b"Encoding") else {
            return Encoding::Absent;
        };
        match self.view.resolve(entry) {
            Object::Name(n) => {
                Encoding::Predefined(String::from_utf8_lossy(n.as_bytes()).into_owned())
            }
            Object::Dict(d) => Encoding::Dictionary {
                base: d
                    .get(b"BaseEncoding")
                    .map(|o| self.view.resolve(o))
                    .and_then(Object::as_name)
                    .map(|n| String::from_utf8_lossy(n.as_bytes()).into_owned()),
                has_differences: d
                    .get(b"Differences")
                    .map(|o| self.view.resolve(o))
                    .and_then(Object::as_array)
                    .is_some_and(|a| !a.is_empty()),
            },
            Object::Stream(s) => Encoding::CMapStream {
                name: s
                    .dict
                    .get(b"CMapName")
                    .map(|o| self.view.resolve(o))
                    .and_then(Object::as_name)
                    .map(|n| String::from_utf8_lossy(n.as_bytes()).into_owned()),
            },
            Object::Null => Encoding::Absent,
            _ => Encoding::Malformed,
        }
    }

    /// Measure the embedded program named by a `/FontDescriptor`.
    ///
    /// §9.9 Table 126 fixes the key→format mapping, and at most one of the
    /// three keys is present in a conforming descriptor. They are checked
    /// in `/FontFile`, `/FontFile2`, `/FontFile3` order and the first found
    /// wins; a descriptor carrying two is malformed, and picking one
    /// deterministically is better than reporting a size that varies with
    /// dictionary iteration order.
    fn model_program(&self, descriptor: &Dict) -> Program {
        let keys = [
            (b"FontFile".as_slice(), ProgramKey::FontFile),
            (b"FontFile2".as_slice(), ProgramKey::FontFile2),
            (b"FontFile3".as_slice(), ProgramKey::FontFile3),
        ];
        for (key_bytes, key) in keys {
            let Some(entry) = descriptor.get(key_bytes) else {
                continue;
            };
            let resolved = self.view.resolve(entry);
            let Object::Stream(stream) = resolved else {
                // A reference that resolved to null is a dangling one;
                // anything else present-but-not-a-stream is a type error.
                // Both mean the program is gone, and the operator's next
                // move differs from "not embedded", so they are named.
                let why = if matches!(resolved, Object::Null) {
                    ProgramUnreadable::DanglingReference
                } else {
                    ProgramUnreadable::NotAStream
                };
                return Program::Unreadable { key, why };
            };
            return self.measure(key, stream);
        }
        Program::NotEmbedded
    }

    /// Measure one font-program stream.
    fn measure(&self, key: ProgramKey, stream: &Stream) -> Program {
        // ★ `data_span`, never `/Length`. On a decrypted document the two
        // disagree by design — see the module docs.
        let Some(raw) = self.view.slice(stream.data_span) else {
            return Program::Unreadable {
                key,
                why: ProgramUnreadable::SpanOutsideFile,
            };
        };
        let stored_bytes = raw.len();

        let subtype = stream
            .dict
            .get(b"Subtype")
            .map(|o| self.view.resolve(o))
            .and_then(Object::as_name)
            .map(|n| String::from_utf8_lossy(n.as_bytes()).into_owned());

        let decoded = filters::decode_stream(&stream.dict, raw).ok();
        let decoded_bytes = decoded.as_ref().map(Vec::len);

        let fs_type = fs_type_for(key, subtype.as_deref(), decoded.as_deref());

        Program::Embedded(EmbeddedProgram {
            key,
            subtype,
            stored_bytes,
            decoded_bytes,
            fs_type,
        })
    }

    /// Queue the resource-bearing children of one resource dictionary.
    ///
    /// Form XObjects and tiling patterns carry their own `/Resources`;
    /// `/ExtGState /SMask /G` names a transparency-group form XObject that
    /// carries its own too. All three nest, and a file may make two of them
    /// reference each other, so `visited` gates by identity and `budget`
    /// bounds the total.
    fn queue_children(&mut self, node: &Node) {
        for (key, surface) in [
            (b"XObject".as_slice(), Surface::FormXObject),
            (b"Pattern".as_slice(), Surface::Pattern),
        ] {
            let Some(entries) = node
                .resources
                .get(key)
                .map(|o| self.view.resolve(o))
                .and_then(Object::as_dict)
            else {
                continue;
            };
            let values: Vec<Object> = entries.0.iter().map(|(_, v)| v.clone()).collect();
            for value in values {
                self.queue_nested_resources(&value, node.page, surface);
            }
        }

        // `/ExtGState` entries may carry `/SMask << /G <form XObject> >>`
        // (§11.6.5.2). The group is a content stream with its own
        // resources, and nothing else in this walk reaches it.
        let Some(gs_entries) = node
            .resources
            .get(b"ExtGState")
            .map(|o| self.view.resolve(o))
            .and_then(Object::as_dict)
        else {
            return;
        };
        let values: Vec<Object> = gs_entries.0.iter().map(|(_, v)| v.clone()).collect();
        for value in values {
            let Some(gs) = self.view.resolve(&value).as_dict() else {
                continue;
            };
            // `/SMask` may also be the name `/None`, which is not a group.
            let Some(smask) = gs
                .get(b"SMask")
                .map(|o| self.view.resolve(o))
                .and_then(Object::as_dict)
            else {
                continue;
            };
            let Some(g) = smask.get(b"G").cloned() else {
                continue;
            };
            self.queue_nested_resources(&g, node.page, Surface::SoftMaskGroup);
        }
    }

    /// Queue the `/Resources` of one XObject/pattern-shaped object.
    fn queue_nested_resources(&mut self, value: &Object, page: Option<u32>, surface: Surface) {
        // Identity gating happens before resolution so a shared XObject is
        // entered once regardless of how many resource dictionaries name
        // it. A direct (non-indirect) nested stream cannot be gated and is
        // simply entered — the budget still bounds the walk.
        if let Some(id) = value.as_reference()
            && !self.visited.insert(id)
        {
            return;
        }
        let nested = match self.view.resolve(value) {
            Object::Stream(s) => s.dict.get(b"Resources"),
            Object::Dict(d) => d.get(b"Resources"),
            _ => None,
        };
        let Some(nested) = nested else { return };
        let Some(res) = self.view.resolve(nested).as_dict() else {
            return;
        };
        self.queue.push(Node {
            resources: res.clone(),
            page,
            surface,
        });
    }
}

/// Add one discovery site to a record, keeping every list sorted and unique.
fn merge_use(record: &mut FontRecord, node: &Node, resource_name: &str) {
    if let Some(page) = node.page
        && let Err(at) = record.pages.binary_search(&page)
    {
        record.pages.insert(at, page);
    }
    if let Err(at) = record.surfaces.binary_search(&node.surface) {
        record.surfaces.insert(at, node.surface);
    }
    if record
        .resource_names
        .binary_search_by(|n| n.as_str().cmp(resource_name))
        .is_err()
    {
        if record.resource_names.len() >= MAX_RESOURCE_NAMES_PER_FONT {
            record.resource_names_truncated = true;
        } else {
            let at = record
                .resource_names
                .partition_point(|n| n.as_str() < resource_name);
            record.resource_names.insert(at, resource_name.to_owned());
        }
    }
}

/// Decide whether a program's `fsType` is even a question, then answer it.
///
/// `/FontFile` is a Type 1 program and `/FontFile3` with `/Type1C` or
/// `/CIDFontType0C` is a bare CFF — neither format has an `OS/2` table, so
/// the honest answer is [`FsType::NotApplicable`] rather than a failed
/// read. Everything else is attempted, and any failure is reported as a
/// failure.
fn fs_type_for(key: ProgramKey, subtype: Option<&str>, decoded: Option<&[u8]>) -> FsType {
    let has_no_os2_by_construction = match key {
        ProgramKey::FontFile => true,
        ProgramKey::FontFile2 => false,
        ProgramKey::FontFile3 => matches!(subtype, Some("Type1C" | "CIDFontType0C")),
    };
    if has_no_os2_by_construction {
        return FsType::NotApplicable;
    }
    let Some(bytes) = decoded else {
        return FsType::ProgramNotDecoded;
    };
    match read_fs_type(bytes) {
        Ok(bits) => FsType::Known(bits),
        Err(err) => FsType::Unreadable(err),
    }
}

/// Decide a font's removability.
///
/// The order of the branches is the argument, so it is written out rather
/// than compressed:
///
/// 1. **Type 3 first**, before embedding is even considered. Its glyphs are
///    content streams in this document, so "is a program embedded" is the
///    wrong question about it.
/// 2. **Unreadable program** next, because a font pdfcer could not read is a
///    font pdfcer cannot classify — and saying "removable" about one would
///    be a guess dressed as a verdict.
/// 3. **Not embedded** next: no program, nothing to remove. Reported as its
///    own verdict rather than as "removable", because an unembed action
///    that reported success here would have done nothing.
/// 4. **Composite fonts** by their CMap. Identity is the blocking case and
///    the common one; a named non-Identity CMap and an embedded CMap stream
///    are both `Unknown`, for different stated reasons.
/// 5. **Simple fonts** by whether the *standard* defines their codes. A
///    base encoding from Annex D does; a built-in encoding on a symbolic
///    font does not.
fn classify(
    subtype: &FontSubtype,
    descendant: Option<&FontSubtype>,
    encoding: &Encoding,
    program: &Program,
    symbolic: Option<bool>,
    has_to_unicode: bool,
    descendant_missing: bool,
) -> Removability {
    if *subtype == FontSubtype::Type3 {
        return Removability::BlockedType3;
    }
    if matches!(program, Program::Unreadable { .. }) {
        return Removability::Unknown(RemovabilityUnknown::ProgramUnreadable);
    }
    if !program.is_embedded() {
        return Removability::NotEmbedded;
    }

    if subtype.is_composite() {
        if descendant_missing || descendant.is_none() {
            return Removability::Unknown(RemovabilityUnknown::NoDescendant);
        }
        if encoding.is_identity() {
            return Removability::BlockedIdentityEncoded {
                to_unicode: has_to_unicode,
            };
        }
        return match encoding {
            Encoding::CMapStream { .. } => Removability::Unknown(RemovabilityUnknown::EmbeddedCMap),
            Encoding::Predefined(_) => Removability::Unknown(RemovabilityUnknown::PredefinedCMap),
            // Table 121 makes `/Encoding` required on a Type 0 font; an
            // absent or malformed one leaves the code→CID mapping
            // undefined, which is exactly "not established".
            _ => Removability::Unknown(RemovabilityUnknown::EmbeddedCMap),
        };
    }

    match subtype {
        FontSubtype::Type1 | FontSubtype::MmType1 | FontSubtype::TrueType => {
            if encoding.is_standard_base() {
                Removability::Removable
            } else if symbolic == Some(true) {
                // §9.6.6.1: with no base encoding the program's built-in
                // encoding governs, and for a symbolic font that is the
                // program's own `cmap` — inside the bytes being removed.
                Removability::Unknown(RemovabilityUnknown::SymbolicBuiltinEncoding)
            } else {
                // Non-symbolic with no explicit base encoding: §9.6.6.1
                // resolves this to the standard encoding for the font's
                // type, which every substitute face shares.
                Removability::Removable
            }
        }
        // CIDFontType0/2 appearing as a top-level font is malformed; an
        // absent or unmodelled `/Subtype` is too.
        _ => Removability::Unknown(RemovabilityUnknown::UnknownSubtype),
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
    use crate::document::Document;

    fn open(bytes: &[u8]) -> Document {
        Document::from_bytes(bytes.to_vec()).expect("fixture parses")
    }

    fn inv_of(doc: &Document) -> FontInventory {
        inventory(&doc.view())
    }

    // -- subset tags ------------------------------------------------------

    /// §9.6.4: "exactly six uppercase letters" then `+`. The strictness is
    /// the point in both directions — a five-letter prefix is part of the
    /// name, not a malformed tag, and pdfcer must not invent a family name
    /// by trimming one.
    #[test]
    fn subset_tag_requires_exactly_six_uppercase_letters() {
        assert_eq!(split_subset_tag("ABCDEF+Arial"), (Some("ABCDEF"), "Arial"));
        assert_eq!(split_subset_tag("ABCDE+Arial"), (None, "ABCDE+Arial"));
        assert_eq!(split_subset_tag("ABCDEFG+Arial"), (None, "ABCDEFG+Arial"));
        assert_eq!(split_subset_tag("ABCDE1+Arial"), (None, "ABCDE1+Arial"));
        assert_eq!(split_subset_tag("abcdef+Arial"), (None, "abcdef+Arial"));
        assert_eq!(split_subset_tag("Arial"), (None, "Arial"));
        // Exactly the tag and the plus, with nothing after it: there is no
        // family name, and saying so beats pretending the tag is one.
        assert_eq!(split_subset_tag("ABCDEF+"), (None, "ABCDEF+"));
        // Non-ASCII after the tag must not panic on the byte slice.
        assert_eq!(split_subset_tag("ABCDEF+Ærial"), (Some("ABCDEF"), "Ærial"));
    }

    // -- fsType -----------------------------------------------------------

    /// The four usage values, read from real font programs.
    #[test]
    fn fs_type_usage_values_read_from_fixtures() {
        for (file, expected) in [
            (
                &include_bytes!("../../../fixtures/synthetic/text/subset-fstype-installable.ttf")[..],
                EmbeddingPermission::Installable,
            ),
            (
                &include_bytes!("../../../fixtures/synthetic/text/subset-fstype-restricted.ttf")[..],
                EmbeddingPermission::Restricted,
            ),
            (
                &include_bytes!("../../../fixtures/synthetic/text/subset-fstype-preview-print.ttf")
                    [..],
                EmbeddingPermission::PreviewPrint,
            ),
            (
                &include_bytes!("../../../fixtures/synthetic/text/subset-fstype-editable.ttf")[..],
                EmbeddingPermission::Editable,
            ),
        ] {
            let bits = read_fs_type(file).expect("fixture has an OS/2 table");
            assert_eq!(bits.permission, expected, "raw={:#06x}", bits.raw);
        }
    }

    /// Bit 8 is honoured at `OS/2` version 2+ and **ignored** at version 0
    /// or 1, where the specification says applications must ignore bits
    /// 4–15. The two fixtures differ only in the table version.
    #[test]
    fn bit_8_is_version_gated() {
        let modern = include_bytes!("../../../fixtures/synthetic/text/subset-fstype-nosubset.ttf");
        let legacy =
            include_bytes!("../../../fixtures/synthetic/text/subset-fstype-nosubset-v1.ttf");

        let modern = read_fs_type(modern).expect("has OS/2");
        assert!(modern.no_subsetting, "bit 8 must be honoured at v2+");
        assert!(!modern.version_gated_bits_ignored);

        let legacy = read_fs_type(legacy).expect("has OS/2");
        assert!(
            !legacy.no_subsetting,
            "bit 8 must be IGNORED at OS/2 v0-v1 — the bits had no assigned meaning there"
        );
        assert!(
            legacy.version_gated_bits_ignored,
            "and the report must be able to say the gate fired, not just show a cleared bit"
        );
    }

    /// Bit 9 — outlines may not be embedded.
    #[test]
    fn bit_9_bitmap_only_is_read() {
        let bytes = include_bytes!("../../../fixtures/synthetic/text/subset-fstype-bitmaponly.ttf");
        let bits = read_fs_type(bytes).expect("has OS/2");
        assert!(bits.bitmap_only);
    }

    /// The decoding rules, exhaustively over the interesting `(raw,
    /// version)` pairs. Building a font file per case is not practical;
    /// the split between [`read_fs_type`] and `decode_fs_type` exists so
    /// this can be tested directly.
    #[test]
    fn fs_type_decoding_edge_cases() {
        // 0 is Installable — the most permissive value, NOT "no data".
        assert_eq!(
            decode_fs_type(0, 4).permission,
            EmbeddingPermission::Installable
        );
        // `fsType == 1` is bit 0 only: reserved, deprecated, and the spec
        // states no reader-side interpretation (N2).
        assert_eq!(
            decode_fs_type(1, 4).permission,
            EmbeddingPermission::Unspecified
        );
        assert!(decode_fs_type(1, 4).reserved_bit0);
        // Two usage bits at once: non-conforming from v3, and pdfcer reports
        // the ambiguity rather than silently picking least-restrictive.
        assert_eq!(
            decode_fs_type(0x000C, 4).permission,
            EmbeddingPermission::Ambiguous
        );
        assert_eq!(
            decode_fs_type(0x000C, 2).permission,
            EmbeddingPermission::Ambiguous
        );
        // Editable + no-subsetting is a real, valid combination: the face
        // permits embedding and forbids subsetting.
        let bits = decode_fs_type(0x0108, 4);
        assert_eq!(bits.permission, EmbeddingPermission::Editable);
        assert!(bits.no_subsetting);
        // The same value at v1 keeps the permission and drops the bit.
        let legacy = decode_fs_type(0x0108, 1);
        assert_eq!(legacy.permission, EmbeddingPermission::Editable);
        assert!(!legacy.no_subsetting);
        assert!(legacy.version_gated_bits_ignored);
        // A v0 table with nothing set above bit 3 has no gate to report.
        assert!(!decode_fs_type(0x0008, 0).version_gated_bits_ignored);
    }

    /// Every rejection path names its cause, and none of them can be
    /// mistaken for a permission.
    #[test]
    fn fs_type_refuses_rather_than_guessing() {
        assert_eq!(read_fs_type(b""), Err(FsTypeError::NotSfnt));
        assert_eq!(read_fs_type(b"%PDF-1.7"), Err(FsTypeError::NotSfnt));
        // A bare CFF program starts with its own header, not an sfnt magic.
        assert_eq!(read_fs_type(&[1, 0, 4, 4, 0, 0]), Err(FsTypeError::NotSfnt));
        // 'ttcf' — a collection.
        assert_eq!(
            read_fs_type(b"ttcf\x00\x01\x00\x00"),
            Err(FsTypeError::Collection)
        );
        // A valid magic with a directory that runs off the end.
        let truncated = [0x00, 0x01, 0x00, 0x00, 0x00, 0x09];
        assert_eq!(
            read_fs_type(&truncated),
            Err(FsTypeError::BadTableDirectory)
        );
        // An absurd numTables is refused before a megabyte of reads.
        let absurd = [0x00, 0x01, 0x00, 0x00, 0xFF, 0xFF, 0, 0, 0, 0, 0, 0];
        assert_eq!(read_fs_type(&absurd), Err(FsTypeError::BadTableDirectory));
        // A well-formed directory with no OS/2 table.
        let mut no_os2 = vec![0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0, 0, 0, 0, 0, 0];
        no_os2.extend_from_slice(b"head");
        no_os2.extend_from_slice(&[0; 12]);
        assert_eq!(read_fs_type(&no_os2), Err(FsTypeError::NoOs2Table));
    }

    /// An `OS/2` record that declares a table too short to hold `fsType`
    /// is refused rather than read out of whatever follows it.
    #[test]
    fn fs_type_refuses_a_short_os2_table() {
        let mut f = vec![0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0, 0, 0, 0, 0, 0];
        f.extend_from_slice(b"OS/2");
        f.extend_from_slice(&[0, 0, 0, 0]); // checkSum
        f.extend_from_slice(&28u32.to_be_bytes()); // offset
        f.extend_from_slice(&4u32.to_be_bytes()); // length — too short
        f.extend_from_slice(&[0; 32]);
        assert_eq!(read_fs_type(&f), Err(FsTypeError::Os2Truncated));
    }

    /// The hand-written reader must agree with `pdfcer-render`'s
    /// `skrifa`-based one. They exist separately only because `pdfcer-core`
    /// may not take a font-parsing dependency (project rule 2); if they
    /// ever disagreed, a font could be refused for embedding and reported
    /// as unrestricted in the same session.
    ///
    /// The cross-check itself lives in `pdfcer-render`'s test suite, where
    /// both readers are in scope. This test pins the half that is
    /// checkable here: every fsType fixture reads a value, and the value is
    /// the one the fixture's name claims.
    #[test]
    fn every_fs_type_fixture_reads_the_value_its_name_claims() {
        for (file, expect_no_subset, expect_bitmap_only) in [
            (
                &include_bytes!("../../../fixtures/synthetic/text/subset-fstype-nosubset.ttf")[..],
                true,
                false,
            ),
            (
                &include_bytes!("../../../fixtures/synthetic/text/subset-fstype-bitmaponly.ttf")[..],
                false,
                true,
            ),
            (
                &include_bytes!("../../../fixtures/synthetic/text/subset-fstype-installable.ttf")[..],
                false,
                false,
            ),
        ] {
            let bits = read_fs_type(file).expect("has OS/2");
            assert_eq!(
                bits.no_subsetting, expect_no_subset,
                "raw={:#06x}",
                bits.raw
            );
            assert_eq!(
                bits.bitmap_only, expect_bitmap_only,
                "raw={:#06x}",
                bits.raw
            );
        }
    }

    // -- inventory --------------------------------------------------------

    /// A non-embedded font reports exactly that — and reports it as its own
    /// verdict rather than as "removable". An unembed action here would do
    /// nothing, and reporting success for nothing is the failure this
    /// distinction prevents.
    #[test]
    fn non_embedded_font_has_nothing_to_remove() {
        let doc = open(include_bytes!(
            "../../../fixtures/synthetic/nonembedded_calibri.pdf"
        ));
        let inv = inv_of(&doc);
        assert!(!inv.fonts.is_empty(), "the fixture has a font");
        let f = &inv.fonts[0];
        assert_eq!(f.program, Program::NotEmbedded);
        assert_eq!(f.removability, Removability::NotEmbedded);
        assert_eq!(f.stored_bytes(), 0);
        assert_eq!(inv.embedded_bytes(), 0);
        assert!(!f.removability.is_removable());
    }

    /// The standard 14 fonts carry no descriptor **legitimately** (§9.6.2.2),
    /// so a document whose only font is `/Helvetica` must come back clean.
    /// Counting that as a missing descriptor would fire the diagnostics on
    /// essentially every ordinary document and train an operator to ignore
    /// them.
    #[test]
    fn a_standard_14_font_without_a_descriptor_is_not_a_fault() {
        let doc = open(include_bytes!(
            "../../../fixtures/synthetic/text/simple-winansi.pdf"
        ));
        let inv = inv_of(&doc);
        let f = inv
            .fonts
            .iter()
            .find(|f| f.base_font.as_deref() == Some("Helvetica"))
            .expect("fixture has Helvetica");
        assert!(f.standard_14);
        assert!(!f.descriptor_present);
        assert_eq!(
            f.encoding,
            Encoding::Predefined("WinAnsiEncoding".to_owned())
        );
        assert!(f.encoding.is_standard_base());
        // Nothing is embedded, so the verdict is NotEmbedded — not
        // Removable, even though the encoding would permit removal.
        assert_eq!(f.removability, Removability::NotEmbedded);
        assert_eq!(inv.diagnostics.descriptors_missing, 0);
        assert!(inv.diagnostics.is_clean());
    }

    /// An embedded simple font with a standard base encoding is the
    /// removable case: the standard defines what its codes mean, so a
    /// substitute draws the same characters.
    #[test]
    fn embedded_simple_font_with_standard_encoding_is_removable() {
        let doc = open(include_bytes!(
            "../../../fixtures/synthetic/text/subset-simple-embedded.pdf"
        ));
        let inv = inv_of(&doc);
        let f = inv
            .fonts
            .iter()
            .find(|f| f.program.is_embedded())
            .expect("fixture embeds a font");
        assert_eq!(f.subtype, FontSubtype::TrueType);
        assert!(f.encoding.is_standard_base());
        assert_eq!(f.removability, Removability::Removable);
        assert!(f.removability.is_removable());
    }

    /// ★ The case the whole report exists for. `Identity-H` over an embedded
    /// `CIDFontType2` with no `/ToUnicode`: the codes are glyph indices into
    /// this program, and nothing else in the file says what they mean.
    #[test]
    fn identity_h_without_tounicode_is_blocked_and_says_why() {
        let doc = open(include_bytes!(
            "../../../fixtures/synthetic/text/cidfonttype2-nocmap-embedded.pdf"
        ));
        let inv = inv_of(&doc);
        let f = inv
            .fonts
            .iter()
            .find(|f| f.subtype == FontSubtype::Type0)
            .expect("fixture has a Type0 font");
        assert!(f.encoding.is_identity());
        assert!(!f.has_to_unicode);
        assert_eq!(
            f.removability,
            Removability::BlockedIdentityEncoded { to_unicode: false }
        );
        assert!(!f.removability.is_removable());
        assert!(
            f.removability.reason().contains("glyph indices"),
            "the verdict must state the mechanism, not merely refuse"
        );
        // §9.8.1: the descriptor hangs off the DESCENDANT for a composite
        // font. Looking on the parent would report "not embedded" about an
        // embedded font.
        assert_eq!(f.descendant_subtype, Some(FontSubtype::CidFontType2));
        assert!(f.descriptor_present);
        assert!(f.program.is_embedded());
    }

    /// The same shape **with** `/ToUnicode` is still blocked, and the verdict
    /// carries the difference: the text is recoverable even though the
    /// drawing is not.
    #[test]
    fn identity_h_with_tounicode_is_still_blocked_but_recoverable() {
        let doc = open(include_bytes!(
            "../../../fixtures/synthetic/text/cidfonttype2-with-tounicode.pdf"
        ));
        let inv = inv_of(&doc);
        let f = inv
            .fonts
            .iter()
            .find(|f| f.subtype == FontSubtype::Type0)
            .expect("fixture has a Type0 font");
        assert!(f.has_to_unicode);
        assert_eq!(
            f.removability,
            Removability::BlockedIdentityEncoded { to_unicode: true }
        );
        assert!(f.removability.reason().contains("/ToUnicode"));
        // Same token, different reason — the verdict is identical and only
        // the recovery story differs.
        assert_eq!(f.removability.token(), "blocked-identity");
    }

    /// A predefined, non-Identity CMap is `Unknown`, not blocked. The codes
    /// are CIDs in a **public** character collection, so a substitute built
    /// for that collection would work — whether one exists is not something
    /// the document can say. Collapsing this into `BlockedIdentityEncoded`
    /// would be pdfcer asserting something it has not established.
    #[test]
    fn predefined_non_identity_cmap_is_unknown_not_blocked() {
        let doc = open(include_bytes!(
            "../../../fixtures/synthetic/fontinfo/predefined-cmap.pdf"
        ));
        let inv = inv_of(&doc);
        let f = inv
            .fonts
            .iter()
            .find(|f| f.subtype == FontSubtype::Type0)
            .expect("fixture has a Type0 font");
        assert!(!f.encoding.is_identity());
        assert_eq!(
            f.removability,
            Removability::Unknown(RemovabilityUnknown::PredefinedCMap)
        );
        assert!(f.removability.reason().contains("character collection"));
    }

    /// A Type 3 font is blocked, and blocked for a reason that has nothing to
    /// do with embedding: its glyphs are content streams in this document.
    /// The same fixture pins the `Type3CharProcs` surface — the inner
    /// `/Courier` is reachable from nowhere else in the file.
    #[test]
    fn type3_is_blocked_and_its_own_resources_are_walked() {
        let doc = open(include_bytes!(
            "../../../fixtures/synthetic/fontinfo/type3-charprocs.pdf"
        ));
        let inv = inv_of(&doc);
        let t3 = inv
            .fonts
            .iter()
            .find(|f| f.subtype == FontSubtype::Type3)
            .expect("fixture has a Type3 font");
        assert_eq!(t3.removability, Removability::BlockedType3);
        assert_eq!(t3.program, Program::NotEmbedded);
        assert!(t3.removability.reason().contains("Type 3"));

        let inner = inv
            .fonts
            .iter()
            .find(|f| f.base_font.as_deref() == Some("Courier"))
            .expect("the font inside the Type 3 font's own /Resources");
        assert_eq!(inner.surfaces, vec![Surface::Type3CharProcs]);
        assert!(inner.standard_14);
    }

    /// A symbolic simple font with no `/Encoding` is `Unknown`, and the
    /// reason names the mechanism: §9.6.6.1 sends the codes through the
    /// program's own built-in encoding, which is inside the bytes an unembed
    /// would delete. The same fixture pins an `fsType` read through
    /// `/FontFile2`.
    #[test]
    fn symbolic_builtin_encoding_is_unknown_and_reads_fs_type() {
        let doc = open(include_bytes!(
            "../../../fixtures/synthetic/fontinfo/symbolic-builtin-encoding.pdf"
        ));
        let inv = inv_of(&doc);
        let f = &inv.fonts[0];
        assert_eq!(f.encoding, Encoding::Absent);
        assert_eq!(f.symbolic, Some(true));
        assert_eq!(
            f.removability,
            Removability::Unknown(RemovabilityUnknown::SymbolicBuiltinEncoding)
        );
        let Program::Embedded(p) = &f.program else {
            panic!("expected an embedded program, got {:?}", f.program);
        };
        assert_eq!(p.key, ProgramKey::FontFile2);
        assert_eq!(
            p.fs_type.permission(),
            Some(EmbeddingPermission::Editable),
            "the donor's OS/2 says Editable and the report must say so too"
        );
    }

    /// A `/FontFile` Type 1 program has **no `OS/2` table by construction**,
    /// which is a different fact from "we could not read it" — and both must
    /// be different from `0`, which genuinely means Installable.
    ///
    /// The same fixture pins stored-vs-decoded size: the payload is Flate
    /// compressed, so the bytes removing it recovers and the size the program
    /// actually is are different numbers, and both are reported.
    #[test]
    fn type1_fontfile_has_no_fs_type_field_and_reports_both_sizes() {
        let doc = open(include_bytes!(
            "../../../fixtures/synthetic/fontinfo/fontfile-type1.pdf"
        ));
        let inv = inv_of(&doc);
        let f = &inv.fonts[0];
        let Program::Embedded(p) = &f.program else {
            panic!("expected an embedded program, got {:?}", f.program);
        };
        assert_eq!(p.key, ProgramKey::FontFile);
        assert_eq!(p.fs_type, FsType::NotApplicable);
        assert_eq!(
            p.fs_type.permission(),
            None,
            "an inapplicable field must never surface as a permission"
        );
        let decoded = p.decoded_bytes.expect("the filler payload inflates");
        assert!(
            p.stored_bytes < decoded,
            "a FlateDecode program stores fewer bytes than it decodes: {} vs {decoded}",
            p.stored_bytes
        );
        // A standard base encoding on a non-symbolic simple font.
        assert_eq!(f.removability, Removability::Removable);
        assert_eq!(f.subset_tag.as_deref(), Some("QWERTY"));
        assert_eq!(f.family_name(), Some("pdfceType1"));
    }

    /// "Declared but unreadable" is not "not embedded". The first is damage,
    /// the second is a document relying on substitution, and they lead to
    /// different operator actions.
    #[test]
    fn a_dangling_font_program_is_unreadable_not_absent() {
        let doc = open(include_bytes!(
            "../../../fixtures/synthetic/fontinfo/unreadable-program.pdf"
        ));
        let inv = inv_of(&doc);
        let f = &inv.fonts[0];
        assert_eq!(
            f.program,
            Program::Unreadable {
                key: ProgramKey::FontFile2,
                why: ProgramUnreadable::DanglingReference,
            }
        );
        assert_ne!(f.program, Program::NotEmbedded);
        assert_eq!(
            f.removability,
            Removability::Unknown(RemovabilityUnknown::ProgramUnreadable)
        );
        assert_eq!(inv.diagnostics.programs_unreadable, 1);
        // The verdict must NOT be Removable: classifying an unreadable font
        // as safe would be a guess wearing a verdict's clothes.
        assert!(!f.removability.is_removable());
    }

    /// ★ The coverage claim's falsifier. A font reachable ONLY through a form
    /// XObject nested inside an annotation appearance stream — two hops a
    /// naive page-resources sweep misses. Without this file the coverage
    /// declaration would be a marker confirming itself (R186).
    #[test]
    fn a_font_behind_an_annotation_appearance_and_a_nested_form_is_found() {
        let doc = open(include_bytes!(
            "../../../fixtures/synthetic/fontinfo/nested-ap-xobject.pdf"
        ));
        let inv = inv_of(&doc);
        let f = inv
            .fonts
            .iter()
            .find(|f| f.base_font.as_deref() == Some("pdfceHiddenInAppearance"))
            .expect("a font two hops deep must still be found");
        assert_eq!(f.surfaces, vec![Surface::FormXObject]);
        assert_eq!(f.pages, vec![1], "and must still be attributed to its page");
        assert_eq!(f.resource_names, vec!["Hidden".to_owned()]);
    }

    /// A font reachable only through `/ExtGState /SMask /G`. Nothing in the
    /// `/XObject` or `/Pattern` walk reaches a soft-mask group, so a sweep
    /// that covers the obvious two still misses this one.
    #[test]
    fn a_font_inside_a_soft_mask_group_is_found() {
        let doc = open(include_bytes!(
            "../../../fixtures/synthetic/fontinfo/softmask-group.pdf"
        ));
        let inv = inv_of(&doc);
        let f = inv
            .fonts
            .iter()
            .find(|f| f.base_font.as_deref() == Some("pdfceInSoftMask"))
            .expect("a font inside a soft-mask group must be found");
        assert_eq!(f.surfaces, vec![Surface::SoftMaskGroup]);
    }

    /// A font in the AcroForm `/DR` is found. This surface is reachable from
    /// **no page**, and one community source says Acrobat's own Optimizer
    /// excludes it — so covering it is a divergence pdfcer states rather than
    /// assumes.
    #[test]
    fn acroform_default_resources_are_walked() {
        let doc = open(include_bytes!(
            "../../../fixtures/synthetic/forms/demo-form.pdf"
        ));
        let inv = inv_of(&doc);
        assert!(
            inv.fonts
                .iter()
                .any(|f| f.surfaces.contains(&Surface::AcroFormDefaultResources)),
            "the form's /DR fonts must appear: {:?}",
            inv.fonts
                .iter()
                .map(|f| (&f.base_font, &f.surfaces))
                .collect::<Vec<_>>()
        );
    }

    /// An annotation appearance stream carrying its own `/Resources /Font` is
    /// walked. This is the surface an inventory silently misses.
    #[test]
    fn annotation_appearance_fonts_are_walked() {
        let doc = open(include_bytes!(
            "../../../fixtures/synthetic/annot/ap-resources-own-font.pdf"
        ));
        let inv = inv_of(&doc);
        assert!(
            inv.fonts
                .iter()
                .any(|f| f.surfaces.contains(&Surface::AnnotationAppearance)),
            "the appearance stream's own font must appear: {:?}",
            inv.fonts
                .iter()
                .map(|f| (&f.base_font, &f.surfaces))
                .collect::<Vec<_>>()
        );
    }

    /// Font records are deduplicated by object identity, and every list on
    /// them is sorted and unique. Counting one program's bytes twice would
    /// overstate what a cleanup recovers.
    #[test]
    fn font_records_are_deduplicated_by_object_identity() {
        let doc = open(include_bytes!(
            "../../../fixtures/synthetic/pageops/two-pages.pdf"
        ));
        let inv = inv_of(&doc);
        for f in &inv.fonts {
            assert!(
                f.pages.windows(2).all(|w| w[0] < w[1]),
                "pages must be sorted and unique: {:?}",
                f.pages
            );
            assert!(f.surfaces.windows(2).all(|w| w[0] < w[1]));
            assert!(f.resource_names.windows(2).all(|w| w[0] < w[1]));
        }
        let ids: Vec<_> = inv.fonts.iter().filter_map(|f| f.id).collect();
        let unique: BTreeSet<_> = ids.iter().copied().collect();
        assert_eq!(ids.len(), unique.len(), "font records must be deduplicated");
    }

    /// Inherited page resources (§7.7.3.4) are resolved before the sweep sees
    /// them, so a font declared only on `/Pages` is found on the leaf.
    #[test]
    fn inherited_page_resources_are_covered() {
        let doc = open(include_bytes!(
            "../../../fixtures/synthetic/addtext/inherited-resources.pdf"
        ));
        let inv = inv_of(&doc);
        assert!(
            !inv.fonts.is_empty(),
            "a font inherited from /Pages must still be found"
        );
        assert!(inv.fonts.iter().all(|f| !f.pages.is_empty()));
    }

    /// ★ The `data_span` hazard, with a fixture that actually reaches it.
    ///
    /// On an encrypted document the decryption walk writes the plaintext back
    /// at `data_span.start` and **shortens the span**, leaving the
    /// dictionary's `/Length` at the ciphertext length — an `/AESV2` stream
    /// carries a 16-byte IV plus padding, so `/Length` overstates by at least
    /// 17 bytes. A size report built on `/Length` is therefore wrong for
    /// every font in every encrypted file.
    ///
    /// The assertion is not "the number looks plausible": the program is
    /// decoded through its filter chain, which only succeeds if the bytes
    /// measured are the plaintext, and the result is compared against the
    /// same font in the **unencrypted** source document.
    #[test]
    fn encrypted_document_measures_plaintext_not_ciphertext() {
        let plain = open(include_bytes!(
            "../../../fixtures/synthetic/text/subset-simple-embedded.pdf"
        ));
        let plain_inv = inv_of(&plain);
        let plain_program = plain_inv
            .fonts
            .iter()
            .find_map(|f| match &f.program {
                Program::Embedded(p) => Some(p.clone()),
                _ => None,
            })
            .expect("the source document embeds a font");

        let doc = Document::from_bytes_with_password(
            include_bytes!("../../../fixtures/synthetic/fontinfo/enc-aes-128-embedded-font.pdf")
                .to_vec(),
            Some(b"userpw"),
        )
        .expect("the AES-128 fixture opens with the corpus user password");
        let inv = inv_of(&doc);
        let program = inv
            .fonts
            .iter()
            .map(|f| f.program.clone())
            .find(Program::is_embedded)
            .expect("the encrypted document embeds the same font");
        let Program::Embedded(p) = program else {
            unreachable!("filtered on is_embedded")
        };

        assert!(
            p.decoded_bytes.is_some(),
            "an AES-decrypted font program must still decode through its filters; \
             a failure here means the ciphertext length was measured"
        );
        assert_eq!(
            p.decoded_bytes, plain_program.decoded_bytes,
            "the decoded program must be exactly the size it is in the unencrypted \
             source; a mismatch means the ciphertext was measured"
        );
        assert_eq!(
            p.fs_type.permission(),
            plain_program.fs_type.permission(),
            "and the OS/2 read must land on the same bits"
        );
    }

    /// Coverage is reported, and the surface pdfcer does NOT walk is named.
    #[test]
    fn coverage_names_what_was_not_searched() {
        let doc = open(include_bytes!("../../../fixtures/synthetic/hello.pdf"));
        let inv = inv_of(&doc);
        assert!(inv.coverage.walked().contains(&Surface::PageResources));
        assert!(
            inv.coverage
                .walked()
                .contains(&Surface::AcroFormDefaultResources)
        );
        assert_eq!(
            inv.coverage.not_walked(),
            vec![Surface::UnreferencedObjects],
            "the one surface pdfcer does not walk must be named, not omitted"
        );
        // Every surface is accounted for exactly once, in one list or the
        // other — so a surface added to the enum and forgotten in the
        // coverage struct fails here rather than vanishing from the report.
        assert_eq!(
            inv.coverage.walked().len() + inv.coverage.not_walked().len(),
            Surface::ALL.len()
        );
        assert!(inv.diagnostics.is_clean());
    }

    /// Page-range collapsing is exact in both directions: nothing is
    /// elided and nothing is invented.
    #[test]
    fn page_ranges_collapse_without_losing_a_page() {
        assert_eq!(format_page_ranges(&[]), "-");
        assert_eq!(format_page_ranges(&[1]), "1");
        assert_eq!(format_page_ranges(&[1, 2]), "1-2");
        assert_eq!(format_page_ranges(&[1, 3]), "1,3");
        assert_eq!(
            format_page_ranges(&[1, 2, 3, 4, 9, 12, 13, 14]),
            "1-4,9,12-14"
        );
        // A long run is one group, which is the whole point on a document
        // where one font is used on every page.
        let all: Vec<u32> = (1..=400).collect();
        assert_eq!(format_page_ranges(&all), "1-400");
        // Saturating arithmetic keeps the last page total even at the top of
        // the range, where `prev + 1` would overflow.
        assert_eq!(format_page_ranges(&[u32::MAX]), u32::MAX.to_string());
    }

    /// Surface tokens are stable and distinct — a listing buckets on them.
    #[test]
    fn surface_tokens_are_distinct() {
        let tokens: BTreeSet<_> = Surface::ALL.iter().map(|s| s.token()).collect();
        assert_eq!(tokens.len(), Surface::ALL.len());
    }

    /// A document with no fonts at all produces an empty, clean inventory
    /// rather than an error.
    #[test]
    fn a_document_with_no_fonts_is_not_an_error() {
        let doc = open(include_bytes!(
            "../../../fixtures/synthetic/vector/paths.pdf"
        ));
        let inv = inv_of(&doc);
        assert!(inv.fonts.is_empty());
        assert_eq!(inv.embedded_count(), 0);
        assert_eq!(inv.embedded_bytes(), 0);
        assert!(inv.verdict_counts().is_empty());
        assert!(inv.diagnostics.is_clean(), "{:?}", inv.diagnostics);
    }

    /// ★ An empty list and an unsearchable document must not look alike.
    ///
    /// `minimal.pdf`'s page has no `/Resources`, which Table 30 marks
    /// Required, so [`crate::page_tree::pages_in`] refuses the whole walk.
    /// The inventory still returns — refusing the report over one damaged
    /// page tree would cost the operator the AcroForm sweep too — but it
    /// **says** the page scan failed. Without that flag, "this document has
    /// no fonts" and "pdfcer could not look" would print identically, which
    /// is the exact shape of confident-but-blind reporting this module's
    /// coverage discipline exists to prevent.
    #[test]
    fn an_unwalkable_page_tree_is_reported_not_rendered_as_no_fonts() {
        let doc = open(include_bytes!("../../../fixtures/synthetic/minimal.pdf"));
        let inv = inv_of(&doc);
        assert!(inv.fonts.is_empty());
        assert!(
            inv.diagnostics.page_scan_failed,
            "an empty list from an unwalkable page tree must be flagged, not presented as an answer"
        );
        assert!(!inv.diagnostics.is_clean());
    }

    /// Verdict tokens are stable and distinct — a corpus sweep buckets on
    /// them, and two verdicts sharing a token would silently merge.
    #[test]
    fn verdict_tokens_are_distinct_and_every_reason_is_a_sentence() {
        let all = [
            Removability::NotEmbedded,
            Removability::Removable,
            Removability::BlockedIdentityEncoded { to_unicode: true },
            Removability::BlockedType3,
            Removability::Unknown(RemovabilityUnknown::SymbolicBuiltinEncoding),
            Removability::Unknown(RemovabilityUnknown::PredefinedCMap),
            Removability::Unknown(RemovabilityUnknown::EmbeddedCMap),
            Removability::Unknown(RemovabilityUnknown::ProgramUnreadable),
            Removability::Unknown(RemovabilityUnknown::NoDescendant),
            Removability::Unknown(RemovabilityUnknown::UnknownSubtype),
        ];
        let tokens: BTreeSet<_> = all.iter().map(Removability::token).collect();
        assert_eq!(tokens.len(), all.len());
        // `BlockedIdentityEncoded` shares one token across both /ToUnicode
        // states deliberately — the verdict is the same and only the recovery
        // story differs — but its REASON must not.
        assert_ne!(
            Removability::BlockedIdentityEncoded { to_unicode: true }.reason(),
            Removability::BlockedIdentityEncoded { to_unicode: false }.reason()
        );
        for r in &all {
            assert!(r.reason().len() > 40, "{:?} needs a real reason", r.token());
        }
    }

    /// The inventory runs over an editing session's overlay as well as over a
    /// loaded file, and resolves stream spans against the session's split
    /// byte source. A `&Document`-only signature would have made this
    /// impossible without a second walk.
    #[test]
    fn inventory_runs_over_an_edit_session_view() {
        let doc = open(include_bytes!(
            "../../../fixtures/synthetic/text/subset-simple-embedded.pdf"
        ));
        let from_document = inventory(&doc.view());
        let session = crate::edit::EditSession::new(doc);
        let from_session = inventory(&session.view());
        assert_eq!(from_document, from_session);
    }
}
