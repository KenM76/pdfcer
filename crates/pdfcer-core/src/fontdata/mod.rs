//! # Standard-14 font data: metrics, descriptors, encodings, and glyph→Unicode
//!
//! Compiled-in data for the **14 standard Type 1 fonts** that every
//! conforming PDF reader must support (ISO 32000-1 §9.6.2.2): the
//! Helvetica, Times and Courier families (4 faces each), plus the two
//! symbolic fonts `Symbol` and `ZapfDingbats`.
//!
//! ## Why this module exists
//!
//! §9.6.2.2 permits a standard-14 font dictionary to be **just**
//! `/Type /Font /Subtype /Type1 /BaseFont /Helvetica` — no `/Widths`, no
//! `/FirstChar`/`/LastChar`, no `/FontDescriptor`, no `/Encoding`, no
//! `/ToUnicode`. Every number a consumer then needs (glyph advances for
//! `Tj` positioning per §9.4.4, ascent/descent for line boxes, the
//! code→glyph-name mapping, the glyph-name→Unicode mapping for text
//! extraction per §9.10.2) must come from *outside* the file. This module
//! is that outside source, compiled in so the engine is deterministic
//! across machines (no system-font dependency — see
//! `font__std14_afm_licensing.md`'s rejection of runtime metrics).
//!
//! ## Data provenance (the docs are the logic — read these to rebuild)
//!
//! Everything in the generated companion module [`tables`] (file
//! `tables.rs`) is extracted from the staged PDF-spec RAG at
//! `D:\Dev\Rag-Specialized\PDF_Spec\` by `tools/gen-fontdata/generate.py`:
//!
//! | Data | RAG source | Ultimate origin |
//! |---|---|---|
//! | Latin widths (315 glyphs × 6 columns) | `fonts/font__std14_widths__helvetica.md`, `__times.md`, `__courier.md` | Adobe Core 14 AFM `WX` values (TN #5004 §4.4), APAFML license |
//! | Symbol / ZapfDingbats widths + built-in encodings + Unicode | `fonts/font__std14_widths__symbol.md`, `__zapfdingbats.md` | AFM `C`/`WX` (`EncodingScheme FontSpecific` ⇒ the `C` codes ARE the built-in encoding, §9.6.6.1); Unicode via AGL |
//! | Descriptors (FontBBox/Ascent/…/StemV/Flags) | `fonts/font__std14_descriptors.md` | AFM global keys mapped onto §9.8.2 Table 122; `Flags` derived per Table 123 |
//! | Standard/MacRoman/WinAnsi encodings | `iso32000/iso32000__annex__d.md` | ISO 32000-1 Annex D.2 Table D.2 + its footnotes |
//! | Glyph-name → Unicode subset | `fonts/font__agl.md` (+ the symbolic fonts' Unicode columns) | Adobe Glyph List (`glyphlist.txt` / `zapfdingbats.txt`), BSD-3-Clause |
//!
//! Regeneration: `python tools/gen-fontdata/generate.py` (deterministic —
//! sorted output, no timestamps; the generated items carry
//! `#[rustfmt::skip]` so `cargo fmt` leaves them canonical).
//!
//! ## Contracts and invariants
//!
//! - **Units.** All widths are AFM `WX` values in **glyph space, 1000
//!   units/em** (§9.2.4). An entry here *is* a PDF `/Widths` value — no
//!   conversion. Advance in text space = `w / 1000 * Tfs` before
//!   `Tc`/`Tw`/`Th` (§9.4.4).
//! - **The file's own `/Widths` wins.** §9.6.2.2's omission is a
//!   permission, not a requirement. If a std-14 font dictionary carries
//!   `/Widths`, use those; consult this module only when the file is
//!   silent (or a code falls outside `/FirstChar`..`/LastChar`, where
//!   §9.8.2 `MissingWidth` applies). Same for an existing
//!   `/FontDescriptor` — never "correct" a file's own numbers against
//!   these tables (round-trip invariant, `docs/ARCHITECTURE.md` §5).
//! - **Key widths by glyph NAME, never by byte code.** The code your
//!   content stream carries is whatever `/Encoding` resolves to; the
//!   chain is `code → (Annex D.2 or /Differences) → glyph name → (this
//!   module) → width`. The AFM `C` codes of the 12 Latin fonts are
//!   AdobeStandardEncoding and cover only 149 of the 315 glyphs.
//! - **Oblique faces share widths with their uprights** (Helvetica and
//!   Courier families): obliquing is a shear that changes outlines and
//!   `FontBBox` but not advances — verified 0/315 differ per the RAG.
//!   Times' italics are **separate designs** (114/315 differ from Roman);
//!   all four Times faces have their own column.
//! - **Courier is uniformly 600.** Every glyph in all four Courier AFMs
//!   has `WX 600` (verified: the set of distinct widths is exactly
//!   `{600}`). [`std14_width`] therefore answers 600 for any glyph in the
//!   shared 315-name Latin repertoire and `None` for names the font does
//!   not have — presence still matters even though the value never varies.
//! - **Exact `BaseFont` names only.** §9.6.2.2 fixes all 14 spellings
//!   (`Times-Roman` not `Times`; `-Oblique` for Helvetica/Courier,
//!   `-Italic` for Times). [`std14_by_base_font`] matches exactly;
//!   empirical aliasing (`/Arial`, `/Helv`, `/CourierNew`, …) is a
//!   product decision that lives in `pdfcer-render::font::select`, NOT
//!   here.
//!
//! ## Encodings ([`encoding_glyph_name`])
//!
//! The three predefined **font** encodings of Annex D.2 (`Standard`,
//! `MacRoman`, `WinAnsi`) plus the two symbolic fonts' **built-in**
//! encodings (§9.6.6.1; the spec documents them as Annex D.5/D.6, which
//! the RAG has not extracted — the tables here come from the fonts' own
//! AFM `C` codes, flagged in the RAG as not yet cross-checked against the
//! annex). `PDFDocEncoding` is deliberately absent: it is a *string*
//! encoding (§7.9.2.3), not a font encoding.
//!
//! Annex D.2's footnotes are honoured (they add entries that are not
//! table rows):
//!
//! - WinAnsi 0o240 (160) → `space` (nonbreaking) and 0o255 (173) →
//!   `hyphen` (soft hyphen) — typographically identical duplicates
//!   (footnotes 5, 6).
//! - MacRoman 0o312 (202) → `space` (nonbreaking) (footnote 6).
//! - WinAnsi: **every otherwise-unused code ≥ 0o40 maps to `bullet`**
//!   (footnote 3 — this is why WinAnsi here is NOT byte-identical to
//!   CP1252, which leaves 0x81/0x8D/0x8F/0x90/0x9D undefined). Only
//!   0o225 is *specifically* assigned to `bullet`; the fallback codes are
//!   "subject to future reassignment" but a conforming reader maps them
//!   to `bullet` today.
//! - PDF 1.3 additions (`Euro` at WinAnsi 0o200; `Zcaron`/`zcaron` at
//!   0o216/0o236) are included; MacRoman 0o333 stays `currency`, NOT
//!   `Euro` (Apple's change was never adopted by PDF's MacRomanEncoding).
//!
//! `/Differences` still applies **on top of** whatever base table is
//! selected (§9.6.6.1) — that resolution belongs to the caller; this
//! module only supplies the base tables.
//!
//! ## Glyph-name → Unicode ([`glyph_name_to_unicode`])
//!
//! Implements the Adobe Glyph List Specification v2.9 §2 algorithm over a
//! compiled **subset** of AGL: the 315-name Latin std-14 repertoire, the
//! 189 encoded `Symbol` names, and the 202 `ZapfDingbats` names (all
//! verified single-code-point). The algorithm, per `font__agl.md`:
//!
//! 1. Truncate at the FIRST `.` — everything after is a variant suffix
//!    (`a.sc` → `a`; `.notdef` → empty → `None`).
//! 2. Table lookup **precedes** the `uni`/`u` forms — a name literally in
//!    the list wins even if it also parses as a `uni` name.
//! 3. `uni` + 4 UPPERCASE hex digits, value in `0000`–`D7FF` or
//!    `E000`–`FFFF` (surrogates rejected by construction; lowercase hex
//!    rejected — `uni20ac` maps to nothing, per the spec's own example).
//! 4. `u` + 4/5/6 UPPERCASE hex digits, value in `0000`–`D7FF` or
//!    `E000`–`10FFFF` (supplementary planes use this form, never `uni`
//!    surrogate pairs — `uniD801DC0C` is invalid, `u1040C` is right).
//!
//! **Deliberate deviations, forced by the `Option<char>` signature**
//! (recorded so nobody "fixes" them into bugs):
//!
//! - Multi-component ligature names (`f_i`) and multi-group `uni` names
//!   (`uni20AC0308`) map to multi-code-point strings in full AGL; a
//!   single-`char` API cannot represent them, so they return `None`.
//!   A future string-returning extraction API can lift this.
//! - Full AGL keys the `zapfdingbats.txt` list on
//!   `FontName == ZapfDingbats`; this subset merges the `aNN` names into
//!   the one table (the generator verifies no name collides with a
//!   different code point). Consequence: `a1` resolves to U+2701 even
//!   without ZapfDingbats context. Harmless in practice — the `aNN` names
//!   occur nowhere in Annex D and in no Latin AFM.
//! - AGL results in a Private Use Area (`commaaccent` → U+F6C3, Symbol's
//!   `registerserif` → U+F6DA, …) are returned as-is. Per the
//!   fuzzy-never-sneaky rule the *extraction* layer should mark PUA
//!   output low-confidence; this data layer does not editorialize.
//!
//! ## Descriptors ([`std14_descriptor`])
//!
//! The §9.8.2 Table 122 payload for synthesizing a `/FontDescriptor` a
//! file legally omitted. Sourced values come verbatim from the AFM
//! headers (`StemV` is AFM `StdVW` — a *sourced* number, not the guessed
//! constant most implementations emit). Derived values, flagged:
//!
//! - `flags` is derived per Table 123 (AFM has no flags concept):
//!   Helvetica 32/96, Times 34/98, Courier 35/99, Symbol/ZapfDingbats 4.
//!   `Symbolic` on the last two is *forced* (Table 123: `Symbolic` and
//!   `Nonsymbolic` shall not both be set nor both clear). `ForceBold` is
//!   deliberately NOT set on bold faces (it is a small-size rendering
//!   hint, not a weight bit).
//! - `Symbol`/`ZapfDingbats` AFMs have **no** `Ascender`/`Descender`/
//!   `CapHeight`/`XHeight` keys at all; `ascender`/`descender` here are
//!   bbox-derived (`ury`/`lly` — Symbol 1010/-293, ZapfDingbats
//!   820/-143) per the RAG's preferred derivation, and
//!   `cap_height`/`x_height` are 0 (§9.8.2 requires `CapHeight` only
//!   "for fonts that have Latin characters"; `XHeight` defaults to 0).
//! - `italic_angle` is `f32` because `Times-Italic` is **-15.5** (and
//!   `Times-BoldItalic` -15 — they differ); an integer field would
//!   silently truncate.
//!
//! Known data limitation: `Symbol`'s 190th glyph `apple` (the Apple logo,
//! AFM `C -1`, unencoded, reachable only via `/Differences`) is absent
//! from the RAG width table, so `std14_width(Std14::Symbol, "apple")` is
//! `None` and it has no Unicode entry (AGL maps it to PUA U+F8FF).

mod tables;

/// The 14 standard Type 1 fonts of ISO 32000-1 §9.6.2.2.
///
/// Variant order groups the families (Helvetica, Times, Courier, then the
/// two symbolic fonts); the spelling of each `BaseFont` name is fixed by
/// the spec — see [`std14_by_base_font`]. Note the asymmetry the spec
/// bakes in: Helvetica/Courier slanted faces are `-Oblique`, Times' are
/// `-Italic`, and the upright Times face is `Times-Roman` (never `Times`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Std14 {
    /// `Helvetica`
    Helvetica,
    /// `Helvetica-Bold`
    HelveticaBold,
    /// `Helvetica-Oblique` (widths identical to `Helvetica`, 315/315)
    HelveticaOblique,
    /// `Helvetica-BoldOblique` (widths identical to `Helvetica-Bold`)
    HelveticaBoldOblique,
    /// `Times-Roman`
    TimesRoman,
    /// `Times-Bold`
    TimesBold,
    /// `Times-Italic` (a separate design — NOT Roman's widths)
    TimesItalic,
    /// `Times-BoldItalic`
    TimesBoldItalic,
    /// `Courier` (monospace: every glyph is width 600)
    Courier,
    /// `Courier-Bold`
    CourierBold,
    /// `Courier-Oblique`
    CourierOblique,
    /// `Courier-BoldOblique`
    CourierBoldOblique,
    /// `Symbol` (symbolic; built-in `FontSpecific` encoding)
    Symbol,
    /// `ZapfDingbats` (symbolic; built-in `FontSpecific` encoding)
    ZapfDingbats,
}

impl Std14 {
    /// The 14 canonical faces in enum/family order — a spec-frozen constant
    /// (ISO 32000-1 §9.6.2.2 defines exactly 14 and has never revised the set).
    ///
    /// Provided so a caller that must enumerate every face (e.g. a font picker
    /// listing the choices for [`crate::text_edit::add_text`]) has one ordered
    /// source of truth instead of re-typing the 14-arm list. Order matches the
    /// enum declaration, so `ALL[0]` is [`Self::Helvetica`] and the two symbolic
    /// faces come last (Pass 16.2 spec §0.4 — a P2 convenience that lets the GUI
    /// stop hardcoding the same list).
    ///
    /// # Examples
    ///
    /// ```
    /// use pdfcer_core::fontdata::{Std14, std14_base_font_name};
    ///
    /// assert_eq!(Std14::ALL.len(), 14);
    /// assert_eq!(Std14::ALL[0], Std14::Helvetica);
    /// assert_eq!(std14_base_font_name(*Std14::ALL.last().unwrap()), "ZapfDingbats");
    /// ```
    pub const ALL: [Std14; 14] = [
        Self::Helvetica,
        Self::HelveticaBold,
        Self::HelveticaOblique,
        Self::HelveticaBoldOblique,
        Self::TimesRoman,
        Self::TimesBold,
        Self::TimesItalic,
        Self::TimesBoldItalic,
        Self::Courier,
        Self::CourierBold,
        Self::CourierOblique,
        Self::CourierBoldOblique,
        Self::Symbol,
        Self::ZapfDingbats,
    ];
}

/// Map an exact §9.6.2.2 `BaseFont` name to its [`Std14`] variant.
///
/// **Exact std-14 names only** — no aliases, no case-folding, no subset
/// tags. `"Times"`, `"Arial"`, `"Helv"`, `"CourierNew"` and the like all
/// return `None`; empirical alias resolution is a rendering-policy
/// decision that lives in `pdfcer-render::font::select`, deliberately not
/// in this data module (the spec defines exactly 14 names and no aliasing
/// rule).
///
/// # Examples
///
/// ```
/// use pdfcer_core::fontdata::{std14_by_base_font, Std14};
///
/// assert_eq!(std14_by_base_font("Times-Roman"), Some(Std14::TimesRoman));
/// assert_eq!(
///     std14_by_base_font("Helvetica-BoldOblique"),
///     Some(Std14::HelveticaBoldOblique)
/// );
/// assert_eq!(std14_by_base_font("Times"), None); // not a std-14 spelling
/// assert_eq!(std14_by_base_font("Arial"), None); // aliasing lives elsewhere
/// ```
#[must_use]
pub fn std14_by_base_font(name: &str) -> Option<Std14> {
    // A 14-arm string match compiles to a decision tree; no table needed.
    // Ordering mirrors the enum for reviewability.
    Some(match name {
        "Helvetica" => Std14::Helvetica,
        "Helvetica-Bold" => Std14::HelveticaBold,
        "Helvetica-Oblique" => Std14::HelveticaOblique,
        "Helvetica-BoldOblique" => Std14::HelveticaBoldOblique,
        "Times-Roman" => Std14::TimesRoman,
        "Times-Bold" => Std14::TimesBold,
        "Times-Italic" => Std14::TimesItalic,
        "Times-BoldItalic" => Std14::TimesBoldItalic,
        "Courier" => Std14::Courier,
        "Courier-Bold" => Std14::CourierBold,
        "Courier-Oblique" => Std14::CourierOblique,
        "Courier-BoldOblique" => Std14::CourierBoldOblique,
        "Symbol" => Std14::Symbol,
        "ZapfDingbats" => Std14::ZapfDingbats,
        _ => return None,
    })
}

/// The exact §9.6.2.2 `BaseFont` PostScript name of a [`Std14`] variant —
/// the inverse of [`std14_by_base_font`].
///
/// Total (every variant has one fixed spelling) and `const`. This is the
/// name a **writer** must emit in a Standard-14 font dictionary's
/// `/BaseFont` (Pass 16.0 add-new-text, `crate::text_edit::addtext`): the
/// value `shall` be one of these 14 exact strings (§9.6.2.2), so keeping the
/// canonical spelling in one place stops the writer from hand-rolling a name
/// a strict reader would reject.
///
/// # Examples
///
/// ```
/// use pdfcer_core::fontdata::{std14_base_font_name, std14_by_base_font, Std14};
///
/// assert_eq!(std14_base_font_name(Std14::TimesRoman), "Times-Roman");
/// assert_eq!(std14_base_font_name(Std14::Symbol), "Symbol");
/// // Round-trips with the name -> variant parser.
/// for name in ["Helvetica-BoldOblique", "Courier", "ZapfDingbats"] {
///     let v = std14_by_base_font(name).unwrap();
///     assert_eq!(std14_base_font_name(v), name);
/// }
/// ```
#[must_use]
pub const fn std14_base_font_name(font: Std14) -> &'static str {
    match font {
        Std14::Helvetica => "Helvetica",
        Std14::HelveticaBold => "Helvetica-Bold",
        Std14::HelveticaOblique => "Helvetica-Oblique",
        Std14::HelveticaBoldOblique => "Helvetica-BoldOblique",
        Std14::TimesRoman => "Times-Roman",
        Std14::TimesBold => "Times-Bold",
        Std14::TimesItalic => "Times-Italic",
        Std14::TimesBoldItalic => "Times-BoldItalic",
        Std14::Courier => "Courier",
        Std14::CourierBold => "Courier-Bold",
        Std14::CourierOblique => "Courier-Oblique",
        Std14::CourierBoldOblique => "Courier-BoldOblique",
        Std14::Symbol => "Symbol",
        Std14::ZapfDingbats => "ZapfDingbats",
    }
}

/// Which Latin width column a face reads, when it is not one of the two
/// symbolic fonts. Private plumbing for [`std14_width`]: collapsing the
/// 12 Latin faces onto 7 outcomes (6 columns + the Courier constant)
/// keeps the width lookup a single match with no unreachable arms.
enum LatinColumn {
    Helvetica,
    HelveticaBold,
    TimesRoman,
    TimesBold,
    TimesItalic,
    TimesBoldItalic,
    /// All four Courier faces: width is the constant 600 for every glyph
    /// the repertoire contains (presence check still required).
    Courier600,
}

/// Advance width of `glyph_name` in `font`, in AFM `WX` units (glyph
/// space, 1000/em — §9.2.4). `None` means the font has no such glyph
/// (the caller falls through to §9.8.2 `MissingWidth` handling).
///
/// Keying is by **glyph name** (post-`/Encoding` resolution), never by
/// byte code — see the module docs. The value is what belongs in a
/// `/Widths` array entry; only consult this when the file's own
/// `/Widths` is absent (round-trip invariant).
///
/// Face folding baked into the data (verified by the RAG's AFM diffs):
/// the Helvetica/Courier oblique faces answer their upright's column;
/// every Courier face answers 600 for any of the 315 shared Latin glyph
/// names. `Symbol` and `ZapfDingbats` have their own tables and
/// repertoires (glyph names collide across fonts — `Delta` exists in
/// both the Latin set and Symbol — so resolution is always per-font,
/// never through one global width table).
///
/// # Examples
///
/// ```
/// use pdfcer_core::fontdata::{std14_width, Std14};
///
/// assert_eq!(std14_width(Std14::Helvetica, "space"), Some(278));
/// assert_eq!(std14_width(Std14::Helvetica, "A"), Some(667));
/// assert_eq!(std14_width(Std14::TimesRoman, "A"), Some(722));
/// assert_eq!(std14_width(Std14::CourierBold, "A"), Some(600)); // any glyph
/// assert_eq!(std14_width(Std14::Symbol, "Alpha"), Some(722));
/// assert_eq!(std14_width(Std14::Helvetica, "Alpha"), None); // Symbol-only
/// ```
#[must_use]
pub fn std14_width(font: Std14, glyph_name: &str) -> Option<u16> {
    let column = match font {
        // The symbolic fonts have their own repertoires and tables.
        Std14::Symbol => return pair_lookup(tables::SYMBOL_WIDTHS, glyph_name),
        Std14::ZapfDingbats => return pair_lookup(tables::ZAPF_DINGBATS_WIDTHS, glyph_name),
        // Oblique == upright for Helvetica (0/315 differ per the RAG).
        Std14::Helvetica | Std14::HelveticaOblique => LatinColumn::Helvetica,
        Std14::HelveticaBold | Std14::HelveticaBoldOblique => LatinColumn::HelveticaBold,
        // Times: four distinct designs, four distinct columns.
        Std14::TimesRoman => LatinColumn::TimesRoman,
        Std14::TimesBold => LatinColumn::TimesBold,
        Std14::TimesItalic => LatinColumn::TimesItalic,
        Std14::TimesBoldItalic => LatinColumn::TimesBoldItalic,
        // Courier: every glyph in all four faces is 600 (verified), but the
        // glyph must still exist in the shared 315-name repertoire.
        Std14::Courier | Std14::CourierBold | Std14::CourierOblique | Std14::CourierBoldOblique => {
            LatinColumn::Courier600
        }
    };
    let idx = tables::LATIN_WIDTHS
        .binary_search_by(|row| row.name.cmp(glyph_name))
        .ok()?;
    let row = tables::LATIN_WIDTHS.get(idx)?;
    Some(match column {
        LatinColumn::Helvetica => row.helvetica,
        LatinColumn::HelveticaBold => row.helvetica_bold,
        LatinColumn::TimesRoman => row.times_roman,
        LatinColumn::TimesBold => row.times_bold,
        LatinColumn::TimesItalic => row.times_italic,
        LatinColumn::TimesBoldItalic => row.times_bold_italic,
        LatinColumn::Courier600 => 600,
    })
}

/// Binary-search a sorted `(name, width)` table. Shared by the two
/// symbolic fonts' lookups; the tables are emitted sorted by the
/// generator (and the generator aborts if they are not).
fn pair_lookup(table: &[(&str, u16)], glyph_name: &str) -> Option<u16> {
    let idx = table
        .binary_search_by(|&(name, _)| name.cmp(glyph_name))
        .ok()?;
    table.get(idx).map(|&(_, width)| width)
}

/// The §9.8.2 Table 122 payload for one standard-14 font, for
/// synthesizing a `/FontDescriptor` the file legally omitted (§9.6.2.2).
///
/// Field-by-field sourcing (see the module docs' "Descriptors" section
/// for the full derivation notes):
///
/// - `font_bbox` — AFM `FontBBox`, already normalized `[llx lly urx ury]`
///   in all 14 fonts (no §7.9.5 reordering needed).
/// - `ascender`/`descender` — AFM `Ascender`/`Descender` for the 12 Latin
///   fonts (`descender` is already negative, as Table 122 requires);
///   **bbox-derived** (`ury`/`lly`) for `Symbol`/`ZapfDingbats`, whose
///   AFMs omit the keys entirely.
/// - `cap_height`/`x_height` — AFM values; **0** for the two symbolic
///   fonts (legitimately omitted per §9.8.2 / Table 122 default).
/// - `italic_angle` — AFM `ItalicAngle`; `f32` because `Times-Italic` is
///   -15.5 (non-integer, and ≠ `Times-BoldItalic`'s -15).
/// - `stem_v` — AFM `StdVW` (the *sourced* dominant-vertical-stem width,
///   exactly Table 122's `StemV` definition — not a guess).
/// - `flags` — **derived** per Table 123; see [`std14_descriptor`].
///
/// When *reading* a file that omitted its descriptor, prefer omission
/// (§9.6.2.2 permits it); this struct is for when pdfcer must *write* a
/// full font dictionary. Never overwrite an existing descriptor's numbers
/// with these (round-trip invariant).
#[derive(Debug, Clone, Copy)]
pub struct Std14Descriptor {
    /// `/FontBBox` — `[llx, lly, urx, ury]`, glyph space.
    pub font_bbox: [i16; 4],
    /// `/Ascent` (bbox-derived for the two symbolic fonts).
    pub ascender: i16,
    /// `/Descent` — negative, per §9.8.2.
    pub descender: i16,
    /// `/CapHeight` — 0 for the symbolic fonts (no Latin capitals).
    pub cap_height: i16,
    /// `/XHeight` — 0 for the symbolic fonts (Table 122 default).
    pub x_height: i16,
    /// `/ItalicAngle` — degrees counterclockwise from vertical; negative
    /// for fonts sloping right.
    pub italic_angle: f32,
    /// `/StemV` — dominant vertical stem width (AFM `StdVW`).
    pub stem_v: u16,
    /// `/Flags` — Table 123 bit set (FixedPitch 1, Serif 2, Symbolic 4,
    /// Nonsymbolic 32, Italic 64).
    pub flags: u32,
}

/// The [`Std14Descriptor`] for `font`. Total — every standard-14 font has
/// one (the data is compiled in; see the generated `tables.rs`).
///
/// # Examples
///
/// ```
/// use pdfcer_core::fontdata::{std14_descriptor, Std14};
///
/// let d = std14_descriptor(Std14::Helvetica);
/// assert_eq!(d.ascender, 718);
/// assert_eq!(d.flags, 32); // Nonsymbolic
///
/// let d = std14_descriptor(Std14::TimesItalic);
/// assert_eq!(d.italic_angle, -15.5); // non-integer — why the field is f32
/// assert_eq!(d.flags, 34 | 64); // Serif + Nonsymbolic + Italic = 98
/// ```
#[must_use]
pub fn std14_descriptor(font: Std14) -> Std14Descriptor {
    tables::descriptor(font)
}

/// A predefined simple-font base encoding: the three Latin encodings of
/// Annex D.2 plus the two symbolic fonts' built-in encodings (§9.6.6.1).
///
/// `StandardEncoding` is data a reader must hold even though it is not a
/// legal `/Encoding` *name* in a file — it is the implicit base encoding
/// for a non-embedded nonsymbolic font and the fill-in for the TrueType
/// §9.6.6.4 Branch A path. `PDFDocEncoding` is deliberately absent (a
/// string encoding, §7.9.2.3, never a font encoding).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BaseEncoding {
    /// Annex D.2 `StandardEncoding` (149 assigned codes).
    Standard,
    /// Annex D.2 `WinAnsiEncoding` + footnote entries (soft hyphen,
    /// nonbreaking space, PDF 1.3 Euro/Zcaron/zcaron) + the "unused codes
    /// ≥ 0o40 map to `bullet`" fallback rule. NOT identical to CP1252.
    WinAnsi,
    /// Annex D.2 `MacRomanEncoding` + the nonbreaking-space footnote
    /// entry (207+1 assigned codes). Code 0o333 is `currency`, not
    /// `Euro` — PDF never adopted Apple's change.
    MacRoman,
    /// `Symbol`'s built-in `FontSpecific` encoding (189 assigned codes,
    /// from the font's own AFM `C` codes; spec Annex D.5).
    Symbol,
    /// `ZapfDingbats`' built-in `FontSpecific` encoding (202 assigned
    /// codes; spec Annex D.6). Note the `aNN` glyph-name number is NOT
    /// the character code.
    ZapfDingbats,
}

/// Resolve a byte code to a glyph name under `enc`. `None` means the code
/// is unencoded there (the caller's `/Differences` layer, or `.notdef`
/// handling, takes over — `.notdef` is never *encoded*, so it never
/// appears as a result here).
///
/// This is the `code → glyph name` half of the width/extraction chain;
/// pair it with [`std14_width`] / [`glyph_name_to_unicode`].
///
/// # Examples
///
/// ```
/// use pdfcer_core::fontdata::{encoding_glyph_name, BaseEncoding};
///
/// assert_eq!(encoding_glyph_name(BaseEncoding::WinAnsi, 0x41), Some("A"));
/// // StandardEncoding places typographic quotes where WinAnsi has ASCII ones:
/// assert_eq!(encoding_glyph_name(BaseEncoding::Standard, 0o47), Some("quoteright"));
/// assert_eq!(encoding_glyph_name(BaseEncoding::WinAnsi, 0o47), Some("quotesingle"));
/// // Footnote entries and the WinAnsi bullet fallback:
/// assert_eq!(encoding_glyph_name(BaseEncoding::WinAnsi, 160), Some("space"));
/// assert_eq!(encoding_glyph_name(BaseEncoding::WinAnsi, 0x90), Some("bullet"));
/// // Symbol's built-in encoding:
/// assert_eq!(encoding_glyph_name(BaseEncoding::Symbol, 0x41), Some("Alpha"));
/// ```
#[must_use]
pub fn encoding_glyph_name(enc: BaseEncoding, code: u8) -> Option<&'static str> {
    let table: &[Option<&'static str>; 256] = match enc {
        BaseEncoding::Standard => &tables::STANDARD_ENCODING,
        BaseEncoding::WinAnsi => &tables::WIN_ANSI_ENCODING,
        BaseEncoding::MacRoman => &tables::MAC_ROMAN_ENCODING,
        BaseEncoding::Symbol => &tables::SYMBOL_ENCODING,
        BaseEncoding::ZapfDingbats => &tables::ZAPF_DINGBATS_ENCODING,
    };
    // Checked access per the crate indexing_slicing policy; a u8 index into
    // a 256-entry array cannot actually miss.
    table.get(usize::from(code)).copied().flatten()
}

/// Map a glyph name to a Unicode scalar per the AGL Specification v2.9 §2
/// algorithm, over the compiled std-14 subset. See the module docs'
/// "Glyph-name → Unicode" section for the algorithm, the precedence
/// rules, and the two documented deviations (multi-code-point results
/// return `None`; the ZapfDingbats `aNN` names are merged rather than
/// font-keyed).
///
/// `None` corresponds to AGL's "empty string" outcome: the glyph
/// contributes no extractable text. Do NOT substitute U+FFFD or the raw
/// byte code — emit nothing (per `font__agl.md`).
///
/// # Examples
///
/// ```
/// use pdfcer_core::fontdata::glyph_name_to_unicode;
///
/// assert_eq!(glyph_name_to_unicode("A"), Some('A'));
/// assert_eq!(glyph_name_to_unicode("germandbls"), Some('ß'));
/// assert_eq!(glyph_name_to_unicode("uni0041"), Some('A'));
/// assert_eq!(glyph_name_to_unicode("u1040C"), Some('\u{1040C}'));
/// assert_eq!(glyph_name_to_unicode("a.sc"), Some('a')); // suffix stripped
/// assert_eq!(glyph_name_to_unicode(".notdef"), None); // empty after strip
/// assert_eq!(glyph_name_to_unicode("uni20ac"), None); // lowercase hex
/// assert_eq!(glyph_name_to_unicode("uniD801"), None); // surrogate
/// ```
#[must_use]
pub fn glyph_name_to_unicode(glyph_name: &str) -> Option<char> {
    // Step 1: truncate at the FIRST '.' (suffixes may themselves contain
    // periods — "a.alt.01" truncates to "a"). ".notdef" truncates to the
    // empty string, which resolves to nothing — by design (AGL §2 step 1 +
    // rule 5), not an error case.
    let base = match glyph_name.split_once('.') {
        Some((before, _suffix)) => before,
        None => glyph_name,
    };
    if base.is_empty() {
        return None;
    }
    // Ligature names ("f_i") map each '_'-separated component independently
    // and concatenate — a multi-code-point result this char-level API cannot
    // represent. Documented deviation: return None rather than a wrong
    // single char.
    if base.contains('_') {
        return None;
    }
    // Step 2: list lookup FIRST — a name literally present wins even if it
    // also parses as a uni/u form (AGL precedence rule).
    if let Ok(idx) = tables::GLYPH_TO_UNICODE.binary_search_by(|&(name, _)| name.cmp(base)) {
        return tables::GLYPH_TO_UNICODE.get(idx).map(|&(_, ch)| ch);
    }
    // Step 3: "uni" + 4·n UPPERCASE hex digits. Only n == 1 is
    // representable as one char (n > 1 is a code-point *sequence*, e.g.
    // uni20AC0308 → U+20AC U+0308 — documented deviation: None). Each
    // group must lie outside the surrogate range: uniD801DC0C is invalid
    // by rule, never decoded as a UTF-16 pair. A failed "uni..." candidate
    // still falls through to the "u" form below — it can never actually
    // match there ('n' is not a hex digit), but the `or_else` keeps the
    // rule ordering honest rather than encoding that reasoning.
    base.strip_prefix("uni")
        .filter(|hex| hex.len() == 4)
        .and_then(|hex| hex_scalar(hex, 0xFFFF))
        .or_else(|| {
            // Step 4: "u" + 4/5/6 UPPERCASE hex digits, any plane except
            // surrogates. The only spelling for supplementary-plane glyphs.
            base.strip_prefix('u')
                .filter(|hex| (4..=6).contains(&hex.len()))
                .and_then(|hex| hex_scalar(hex, 0x0010_FFFF))
        })
    // Step 5 (either chain yielding None): no mapping — the glyph
    // contributes no text.
}

/// Whether `glyph_name` belongs to the **Adobe standard Latin character
/// set ∪ the set of named characters in the `Symbol` font** — the
/// repertoire ISO 32000-1 §9.10.2 method 2 names in its second
/// precondition disjunct.
///
/// The clause's own words are: "an encoding whose `Differences` array
/// includes only character names taken from the Adobe standard Latin
/// character set and the set of named characters in the `Symbol` font
/// (see Annex D)". It is a **whole-array** test — one name outside the
/// repertoire disqualifies the entire font from method 2 — which is why
/// this is a predicate a caller runs over every entry rather than a
/// per-code lookup.
///
/// ## What backs the answer, precisely
///
/// - The **Symbol** half is exact: `Symbol`'s own AFM repertoire.
/// - The **Latin** half is the 315-name shared Latin repertoire of the
///   standard-14 AFMs, of which Annex D.2's `PDF` column assigns 229.
///   The extra ~86 names are reachable only through `/Differences` and
///   are all ordinary Latin-script glyph names, so accepting them is
///   *marginally* more permissive than the strictest possible reading of
///   "the Adobe standard Latin character set".
///
/// That widening is deliberate and recorded: the alternative is a
/// separate 229-name table that duplicates data already present and
/// disagrees with it at the edges. The consequence is bounded — the
/// predicate can only cause a font to be classified as *sourced* rung 2
/// rather than as pdfcer's counted glyph-name extension, and only for a
/// name that is a genuine Adobe Latin glyph name either way.
///
/// # Examples
///
/// ```
/// use pdfcer_core::fontdata::is_standard_latin_or_symbol_name;
///
/// assert!(is_standard_latin_or_symbol_name("A"));
/// assert!(is_standard_latin_or_symbol_name("germandbls"));
/// assert!(is_standard_latin_or_symbol_name("Alpha")); // Symbol
/// // A vendor-private or synthetic name is NOT in the repertoire —
/// // one of these anywhere in a /Differences array disqualifies the
/// // whole font from §9.10.2 method 2.
/// assert!(!is_standard_latin_or_symbol_name("uni4E2D"));
/// assert!(!is_standard_latin_or_symbol_name("g123"));
/// ```
#[must_use]
pub fn is_standard_latin_or_symbol_name(glyph_name: &str) -> bool {
    tables::LATIN_WIDTHS
        .binary_search_by(|e| e.name.cmp(glyph_name))
        .is_ok()
        || tables::SYMBOL_WIDTHS
            .binary_search_by(|&(name, _)| name.cmp(glyph_name))
            .is_ok()
}

/// Map a glyph name to the Unicode **string** it denotes, per the full
/// AGL Specification v2.9 §2 algorithm — including the two cases
/// [`glyph_name_to_unicode`] cannot represent.
///
/// [`glyph_name_to_unicode`] is the `char`-valued convenience used by
/// the *rendering* side, where one glyph maps to one glyph index and a
/// multi-code-point result has nowhere to go. **Text extraction needs
/// the string form**: §9.10.2 rung 2's job is to produce the characters
/// a reader would see, and AGL's own algorithm produces sequences for
/// two constructs that appear in real fonts:
///
/// - **Ligature names** — `f_i` maps each `_`-separated component
///   independently and concatenates: `fi`. Subsetting tools emit these
///   routinely, and they are the same problem `ToUnicode`'s one-to-many
///   `bfchar` destinations solve from the other direction
///   (`iso32000__s__9.10.3.md`).
/// - **Multi-group `uni` names** — `uni0041030A` is the *sequence*
///   U+0041 U+030A (`A` plus COMBINING RING ABOVE), not one character.
///   AGL's rule is "`uni` followed by 4·n uppercase hex digits", n ≥ 1.
///
/// Everything else — the list lookup, the `u` form, the `.suffix`
/// truncation, the surrogate exclusion, and the case-sensitivity of the
/// hex digits (AGL's own example maps `uni20ac` to nothing because of
/// the lowercase letters) — is delegated to [`glyph_name_to_unicode`],
/// so the two functions can never disagree about a single-character
/// name.
///
/// `None` is AGL's "empty string" outcome: the glyph contributes **no**
/// extractable text. Do not substitute U+FFFD or the raw code — a name
/// with no mapping is a *sourced silence*, and inventing a character for
/// it is exactly the fabrication the ladder's failure clause forbids
/// pdfcer from doing quietly.
///
/// # Examples
///
/// ```
/// use pdfcer_core::fontdata::glyph_name_to_unicode_string;
///
/// assert_eq!(glyph_name_to_unicode_string("A").as_deref(), Some("A"));
/// // Ligature names decompose — the char-valued API returns None here.
/// assert_eq!(glyph_name_to_unicode_string("f_i").as_deref(), Some("fi"));
/// assert_eq!(glyph_name_to_unicode_string("f_f_l").as_deref(), Some("ffl"));
/// // Multi-group `uni` names are sequences.
/// assert_eq!(
///     glyph_name_to_unicode_string("uni0041030A").as_deref(),
///     Some("A\u{030A}")
/// );
/// // Suffixes are still truncated, and `.notdef` still maps to nothing.
/// assert_eq!(glyph_name_to_unicode_string("a.sc").as_deref(), Some("a"));
/// assert_eq!(glyph_name_to_unicode_string(".notdef"), None);
/// // Case-sensitivity is normative and unchanged.
/// assert_eq!(glyph_name_to_unicode_string("uni20ac"), None);
/// ```
#[must_use]
pub fn glyph_name_to_unicode_string(glyph_name: &str) -> Option<String> {
    // Step 1 (AGL §2): truncate at the FIRST '.'. Done here as well as
    // in the char-valued function because the ligature split below must
    // see the already-truncated name ("f_i.alt" is the `fi` ligature).
    let base = match glyph_name.split_once('.') {
        Some((before, _suffix)) => before,
        None => glyph_name,
    };
    if base.is_empty() {
        return None;
    }

    // Ligature form: split on '_' and map each component independently.
    // A component that maps to nothing makes the WHOLE name unmapped —
    // AGL concatenates component results, and a partial ligature would
    // be a silently wrong character sequence rather than a known gap.
    if base.contains('_') {
        let mut out = String::new();
        for component in base.split('_') {
            out.push_str(&glyph_name_to_unicode_string(component)?);
        }
        return (!out.is_empty()).then_some(out);
    }

    // Single-character forms (list lookup, single-group `uni`, `u`).
    if let Some(ch) = glyph_name_to_unicode(base) {
        return Some(ch.to_string());
    }

    // Multi-group `uni` form: 4·n uppercase hex digits, n ≥ 2 (n == 1
    // was already handled above). Each group is an independent scalar
    // and each is independently subject to the surrogate exclusion.
    let hex = base.strip_prefix("uni")?;
    if hex.len() < 8 || !hex.len().is_multiple_of(4) {
        return None;
    }
    let mut out = String::new();
    let bytes = hex.as_bytes();
    let mut i = 0usize;
    while i < hex.len() {
        let group = bytes
            .get(i..i + 4)
            .and_then(|g| std::str::from_utf8(g).ok())?;
        out.push(hex_scalar(group, 0xFFFF)?);
        i += 4;
    }
    Some(out)
}

/// Parse an UPPERCASE-hex Unicode scalar in `0..=0xD7FF` or
/// `0xE000..=max` (the AGL `uni`/`u` range rule — surrogates are always
/// excluded; `max` is 0xFFFF for the BMP-only `uni` form, 0x10FFFF for
/// `u`). Case-sensitivity is normative: AGL's own example maps `uni20ac`
/// to nothing because of the lowercase `a`/`c`. Do not case-fold.
fn hex_scalar(hex: &str, max: u32) -> Option<char> {
    let uppercase_hex = !hex.is_empty()
        && hex
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'A'..=b'F').contains(&b));
    if !uppercase_hex {
        return None;
    }
    let value = u32::from_str_radix(hex, 16).ok()?;
    if value <= 0xD7FF || (0xE000..=max).contains(&value) {
        char::from_u32(value)
    } else {
        None
    }
}

/// The built-in encoding a standard-14 font carries when its dictionary
/// has no `/Encoding` (§9.6.2.2 / §9.6.6.1): the two symbolic fonts use
/// their own `FontSpecific` encodings; the 12 Latin faces use
/// `StandardEncoding` (their AFMs declare `EncodingScheme
/// AdobeStandardEncoding`, and §9.6.6.2's built-in-encoding rule for
/// non-symbolic Type 1 fonts resolves to it).
///
/// `/Differences` — and, for the Latin faces, a named `/BaseEncoding` —
/// layer on top of this at resolution time; a named base encoding on
/// `Symbol`/`ZapfDingbats` is a conformance error that real files
/// nevertheless contain (prefer the built-in — see the RAG gotcha).
///
/// # Examples
///
/// ```
/// use pdfcer_core::fontdata::{std14_builtin_encoding, BaseEncoding, Std14};
///
/// assert_eq!(std14_builtin_encoding(Std14::Symbol), BaseEncoding::Symbol);
/// assert_eq!(std14_builtin_encoding(Std14::Helvetica), BaseEncoding::Standard);
/// ```
#[must_use]
pub fn std14_builtin_encoding(font: Std14) -> BaseEncoding {
    match font {
        Std14::Symbol => BaseEncoding::Symbol,
        Std14::ZapfDingbats => BaseEncoding::ZapfDingbats,
        _ => BaseEncoding::Standard,
    }
}

/// Map a `/BaseFont` name to the standard-14 face it denotes, for resolving
/// a `/DA` font name against a resource dictionary (§12.7.3.3).
///
/// Lives here rather than in `edit` because it is pure font-name data
/// with no session, document or graph dependency, and it now has two
/// callers in different layers: form-field appearance regeneration
/// (`EditSession::resolve_dr_fonts`, against the AcroForm `/DR`) and
/// redaction overlay text (`redact::overlay_font_resources`, against the
/// page's own `/Resources`). A second copy would be two answers to
/// "which face is `/Helv`?" in one binary.
///
/// Handles the canonical §9.6.2.2 spellings and the common producer
/// shorthands (`Helv`, `HeBo`, `Cour`, `TiRo`, `Symb`, `ZaDb`) Acrobat's
/// default `/DR` uses. A subset-prefixed name (`ABCDEF+Helvetica`) is
/// matched on the suffix. `None` for anything not a standard-14 face — the
/// caller then falls back to Helvetica (the Base-14 generator cannot lay out
/// an embedded/CID font).
pub(crate) fn basefont_to_std14(name: &[u8]) -> Option<Std14> {
    // Strip a subset prefix `ABCDEF+`.
    let bare = match name.iter().position(|&b| b == b'+') {
        Some(i) if i == 6 => name.get(i + 1..).unwrap_or(name),
        _ => name,
    };
    Some(match bare {
        b"Helvetica" | b"Helv" | b"Arial" | b"ArialMT" => Std14::Helvetica,
        b"Helvetica-Bold" | b"HeBo" | b"Arial-Bold" | b"Arial-BoldMT" => Std14::HelveticaBold,
        b"Helvetica-Oblique" | b"Arial-Italic" | b"Arial-ItalicMT" => Std14::HelveticaOblique,
        b"Helvetica-BoldOblique" | b"Arial-BoldItalic" => Std14::HelveticaBoldOblique,
        b"Times-Roman" | b"TiRo" | b"TimesNewRoman" | b"TimesNewRomanPSMT" => Std14::TimesRoman,
        b"Times-Bold" | b"TimesNewRomanPS-BoldMT" => Std14::TimesBold,
        b"Times-Italic" | b"TimesNewRomanPS-ItalicMT" => Std14::TimesItalic,
        b"Times-BoldItalic" | b"TimesNewRomanPS-BoldItalicMT" => Std14::TimesBoldItalic,
        b"Courier" | b"Cour" | b"CourierNew" | b"CourierNewPSMT" => Std14::Courier,
        b"Courier-Bold" => Std14::CourierBold,
        b"Courier-Oblique" => Std14::CourierOblique,
        b"Courier-BoldOblique" => Std14::CourierBoldOblique,
        b"Symbol" | b"Symb" => Std14::Symbol,
        b"ZapfDingbats" | b"ZaDb" => Std14::ZapfDingbats,
        _ => return None,
    })
}

#[cfg(test)]
// Tests are exempt from the panic-free policy: a panicking assertion IS the
// test-failure mechanism (see the crate-level lint rationale in lib.rs).
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;

    /// All 14 variants with their exact §9.6.2.2 `BaseFont` spellings —
    /// the fixture for the round-trip and totality tests below.
    const ALL_14: [(Std14, &str); 14] = [
        (Std14::Helvetica, "Helvetica"),
        (Std14::HelveticaBold, "Helvetica-Bold"),
        (Std14::HelveticaOblique, "Helvetica-Oblique"),
        (Std14::HelveticaBoldOblique, "Helvetica-BoldOblique"),
        (Std14::TimesRoman, "Times-Roman"),
        (Std14::TimesBold, "Times-Bold"),
        (Std14::TimesItalic, "Times-Italic"),
        (Std14::TimesBoldItalic, "Times-BoldItalic"),
        (Std14::Courier, "Courier"),
        (Std14::CourierBold, "Courier-Bold"),
        (Std14::CourierOblique, "Courier-Oblique"),
        (Std14::CourierBoldOblique, "Courier-BoldOblique"),
        (Std14::Symbol, "Symbol"),
        (Std14::ZapfDingbats, "ZapfDingbats"),
    ];

    // ----- std14_by_base_font -----

    #[test]
    fn base_font_names_round_trip_for_all_14() {
        for (font, name) in ALL_14 {
            assert_eq!(std14_by_base_font(name), Some(font), "{name}");
        }
    }

    #[test]
    fn base_font_rejects_aliases_and_near_misses() {
        for bad in [
            "Times",          // §9.6.2.2 spells it Times-Roman
            "Times-Oblique",  // Times uses -Italic
            "Courier-Italic", // Courier uses -Oblique
            "Helvetica-Italic",
            "helvetica", // case-sensitive
            "Arial",     // aliasing lives in pdfcer-render, not here
            "CourierNew",
            "Helv",
            "",
        ] {
            assert_eq!(std14_by_base_font(bad), None, "{bad:?}");
        }
    }

    // ----- widths (spot values verified against the RAG width files) -----

    #[test]
    fn helvetica_spot_widths_match_afm() {
        assert_eq!(std14_width(Std14::Helvetica, "space"), Some(278));
        assert_eq!(std14_width(Std14::Helvetica, "A"), Some(667));
        assert_eq!(std14_width(Std14::Helvetica, "at"), Some(1015));
        assert_eq!(std14_width(Std14::HelveticaBold, "A"), Some(722));
        assert_eq!(std14_width(Std14::HelveticaBold, "at"), Some(975));
    }

    #[test]
    fn oblique_faces_share_upright_widths() {
        for name in ["A", "at", "space", "quotesingle", "lcaron"] {
            assert_eq!(
                std14_width(Std14::HelveticaOblique, name),
                std14_width(Std14::Helvetica, name),
                "{name}"
            );
            assert_eq!(
                std14_width(Std14::HelveticaBoldOblique, name),
                std14_width(Std14::HelveticaBold, name),
                "{name}"
            );
        }
    }

    #[test]
    fn times_faces_are_distinct_designs() {
        // Times-Roman A=722 per the RAG; italic is 611 — substituting the
        // Roman column for Italic would be visibly wrong.
        assert_eq!(std14_width(Std14::TimesRoman, "A"), Some(722));
        assert_eq!(std14_width(Std14::TimesItalic, "A"), Some(611));
        assert_eq!(std14_width(Std14::TimesBold, "A"), Some(722));
        assert_eq!(std14_width(Std14::TimesBoldItalic, "A"), Some(667));
        assert_eq!(std14_width(Std14::TimesRoman, "space"), Some(250));
        assert_eq!(std14_width(Std14::TimesRoman, "quotesingle"), Some(180));
        assert_eq!(std14_width(Std14::TimesItalic, "quotesingle"), Some(214));
    }

    #[test]
    fn courier_is_600_for_every_repertoire_glyph_and_none_otherwise() {
        for font in [
            Std14::Courier,
            Std14::CourierBold,
            Std14::CourierOblique,
            Std14::CourierBoldOblique,
        ] {
            for name in ["A", "space", "at", "Zdotaccent", "lslash", "fi"] {
                assert_eq!(std14_width(font, name), Some(600), "{font:?} {name}");
            }
            // Symbol-only and nonsense names are NOT in Courier.
            assert_eq!(std14_width(font, "universal"), None);
            assert_eq!(std14_width(font, "nosuchglyph"), None);
        }
    }

    #[test]
    fn symbol_and_zapf_have_their_own_repertoires() {
        // Symbol code 0o101 = 0x41 is Alpha, width 722 per the RAG.
        assert_eq!(std14_width(Std14::Symbol, "Alpha"), Some(722));
        assert_eq!(std14_width(Std14::Symbol, "universal"), Some(713));
        assert_eq!(std14_width(Std14::Symbol, "arrowboth"), Some(1042));
        // `Delta` exists in BOTH repertoires (same width here, but the
        // lookup must be per-font, never a merged table).
        assert_eq!(std14_width(Std14::Symbol, "Delta"), Some(612));
        assert_eq!(std14_width(Std14::Helvetica, "Delta"), Some(612));
        // Latin-only names are not in Symbol.
        assert_eq!(std14_width(Std14::Symbol, "germandbls"), None);
        // ZapfDingbats: the aNN number is not the code; a1 is code 33.
        assert_eq!(std14_width(Std14::ZapfDingbats, "a1"), Some(974));
        assert_eq!(std14_width(Std14::ZapfDingbats, "space"), Some(278));
        assert_eq!(std14_width(Std14::ZapfDingbats, "a191"), Some(918));
        assert_eq!(std14_width(Std14::ZapfDingbats, "A"), None);
    }

    #[test]
    fn every_encoded_glyph_of_every_face_has_a_width() {
        // Chain check: for each face, every code its builtin encoding
        // assigns must resolve to a glyph the width table knows. This
        // catches name-set drift between the encoding and width tables.
        for (font, name) in ALL_14 {
            let enc = std14_builtin_encoding(font);
            for code in 0..=255u8 {
                if let Some(glyph) = encoding_glyph_name(enc, code) {
                    assert!(
                        std14_width(font, glyph).is_some(),
                        "{name}: code {code} -> {glyph} has no width"
                    );
                }
            }
        }
    }

    // ----- descriptors -----

    #[test]
    fn every_font_has_a_sane_descriptor() {
        for (font, name) in ALL_14 {
            let d = std14_descriptor(font);
            let [llx, lly, urx, ury] = d.font_bbox;
            assert!(llx < urx, "{name}: bbox x order");
            assert!(lly < ury, "{name}: bbox y order");
            assert!(d.ascender > 0, "{name}: ascender");
            assert!(d.descender < 0, "{name}: descender must be negative");
            assert!(d.stem_v > 0, "{name}: stem_v");
            assert_ne!(d.flags, 0, "{name}: flags");
            // Table 123: Symbolic (4) and Nonsymbolic (32) are mutually
            // exclusive and one must be set.
            let symbolic = d.flags & 4 != 0;
            let nonsymbolic = d.flags & 32 != 0;
            assert!(symbolic ^ nonsymbolic, "{name}: symbolic xor nonsymbolic");
        }
    }

    #[test]
    fn descriptor_spot_values_match_the_rag() {
        let helv = std14_descriptor(Std14::Helvetica);
        assert_eq!(helv.font_bbox, [-166, -225, 1000, 931]);
        assert_eq!(helv.ascender, 718);
        assert_eq!(helv.descender, -207);
        assert_eq!(helv.cap_height, 718);
        assert_eq!(helv.x_height, 523);
        assert_eq!(helv.stem_v, 88); // sourced StdVW, not a guess
        assert_eq!(helv.flags, 32);

        // Times-Italic's -15.5 is the reason italic_angle is f32.
        let ti = std14_descriptor(Std14::TimesItalic);
        assert_eq!(ti.italic_angle, -15.5);
        assert_eq!(ti.flags, 98); // Serif + Nonsymbolic + Italic
        let tbi = std14_descriptor(Std14::TimesBoldItalic);
        assert_eq!(tbi.italic_angle, -15.0); // differs from Times-Italic

        let courier = std14_descriptor(Std14::Courier);
        assert_eq!(courier.flags, 35); // FixedPitch + Serif + Nonsymbolic
        assert_eq!(courier.stem_v, 51);

        // Symbolic fonts: bbox-derived ascent/descent, flags forced to 4.
        let sym = std14_descriptor(Std14::Symbol);
        assert_eq!(sym.font_bbox, [-180, -293, 1090, 1010]);
        assert_eq!(sym.ascender, 1010);
        assert_eq!(sym.descender, -293);
        assert_eq!(sym.cap_height, 0);
        assert_eq!(sym.flags, 4);
        let zapf = std14_descriptor(Std14::ZapfDingbats);
        assert_eq!(zapf.ascender, 820);
        assert_eq!(zapf.descender, -143);
        assert_eq!(zapf.flags, 4);
    }

    // ----- encodings -----

    #[test]
    fn winansi_spot_codes_match_annex_d() {
        assert_eq!(encoding_glyph_name(BaseEncoding::WinAnsi, 0x41), Some("A"));
        assert_eq!(
            encoding_glyph_name(BaseEncoding::WinAnsi, 32),
            Some("space")
        );
        assert_eq!(
            encoding_glyph_name(BaseEncoding::WinAnsi, 128),
            Some("Euro")
        ); // PDF 1.3
        assert_eq!(
            encoding_glyph_name(BaseEncoding::WinAnsi, 142),
            Some("Zcaron")
        ); // PDF 1.3
        assert_eq!(
            encoding_glyph_name(BaseEncoding::WinAnsi, 149),
            Some("bullet")
        ); // 0o225
        assert_eq!(
            encoding_glyph_name(BaseEncoding::WinAnsi, 223),
            Some("germandbls")
        );
    }

    #[test]
    fn winansi_footnote_entries_and_bullet_fallback() {
        // Footnote 6: nonbreaking space at 0o240; footnote 5: soft hyphen
        // at 0o255 — typographically identical duplicates.
        assert_eq!(
            encoding_glyph_name(BaseEncoding::WinAnsi, 160),
            Some("space")
        );
        assert_eq!(
            encoding_glyph_name(BaseEncoding::WinAnsi, 173),
            Some("hyphen")
        );
        // Footnote 3: unused codes >= 0o40 map to bullet (these five are
        // exactly where PDF's WinAnsi diverges from CP1252, plus DEL).
        for code in [0x7F, 0x81, 0x8D, 0x8F, 0x90, 0x9D] {
            assert_eq!(
                encoding_glyph_name(BaseEncoding::WinAnsi, code),
                Some("bullet"),
                "{code:#x}"
            );
        }
        // ...but codes below 0o40 stay unencoded.
        for code in 0..32u8 {
            assert_eq!(encoding_glyph_name(BaseEncoding::WinAnsi, code), None);
        }
    }

    #[test]
    fn standard_encoding_spot_codes_match_annex_d() {
        assert_eq!(encoding_glyph_name(BaseEncoding::Standard, 0x41), Some("A"));
        assert_eq!(
            encoding_glyph_name(BaseEncoding::Standard, 0o341),
            Some("AE")
        );
        // The classic Standard-vs-WinAnsi quote divergence:
        assert_eq!(
            encoding_glyph_name(BaseEncoding::Standard, 0o47),
            Some("quoteright")
        );
        assert_eq!(
            encoding_glyph_name(BaseEncoding::WinAnsi, 0o47),
            Some("quotesingle")
        );
        assert_eq!(
            encoding_glyph_name(BaseEncoding::Standard, 0o140),
            Some("quoteleft")
        );
        assert_eq!(
            encoding_glyph_name(BaseEncoding::WinAnsi, 0o140),
            Some("grave")
        );
        // Standard has no bullet fallback: unused codes are None.
        assert_eq!(encoding_glyph_name(BaseEncoding::Standard, 0o200), None);
    }

    #[test]
    fn macroman_spot_codes_match_annex_d() {
        assert_eq!(encoding_glyph_name(BaseEncoding::MacRoman, 0x41), Some("A"));
        assert_eq!(
            encoding_glyph_name(BaseEncoding::MacRoman, 167),
            Some("germandbls")
        ); // 0o247
        // Footnote 6: nonbreaking space at 0o312.
        assert_eq!(
            encoding_glyph_name(BaseEncoding::MacRoman, 202),
            Some("space")
        );
        // Footnote 1: code 0o333 stays currency — PDF never adopted
        // Apple's Euro reassignment.
        assert_eq!(
            encoding_glyph_name(BaseEncoding::MacRoman, 0o333),
            Some("currency")
        );
    }

    #[test]
    fn symbol_and_zapf_builtin_encodings() {
        assert_eq!(
            encoding_glyph_name(BaseEncoding::Symbol, 0x41),
            Some("Alpha")
        );
        assert_eq!(
            encoding_glyph_name(BaseEncoding::Symbol, 0o42),
            Some("universal")
        );
        assert_eq!(
            encoding_glyph_name(BaseEncoding::Symbol, 0o376),
            Some("bracerightbt")
        );
        assert_eq!(encoding_glyph_name(BaseEncoding::Symbol, 240), None); // 0o360 gap
        // ZapfDingbats: the aNN number is not the code (code 97 is a60).
        assert_eq!(
            encoding_glyph_name(BaseEncoding::ZapfDingbats, 33),
            Some("a1")
        );
        assert_eq!(
            encoding_glyph_name(BaseEncoding::ZapfDingbats, 97),
            Some("a60")
        );
        assert_eq!(
            encoding_glyph_name(BaseEncoding::ZapfDingbats, 32),
            Some("space")
        );
        assert_eq!(encoding_glyph_name(BaseEncoding::ZapfDingbats, 127), None);
        assert_eq!(encoding_glyph_name(BaseEncoding::ZapfDingbats, 255), None);
    }

    #[test]
    fn builtin_encoding_selection() {
        for (font, _) in ALL_14 {
            let expected = match font {
                Std14::Symbol => BaseEncoding::Symbol,
                Std14::ZapfDingbats => BaseEncoding::ZapfDingbats,
                _ => BaseEncoding::Standard,
            };
            assert_eq!(std14_builtin_encoding(font), expected);
        }
    }

    // ----- glyph_name_to_unicode -----

    #[test]
    fn agl_plain_lookups() {
        assert_eq!(glyph_name_to_unicode("A"), Some('A'));
        assert_eq!(glyph_name_to_unicode("space"), Some(' '));
        assert_eq!(glyph_name_to_unicode("germandbls"), Some('\u{00DF}'));
        assert_eq!(glyph_name_to_unicode("quotesinglbase"), Some('\u{201A}'));
        assert_eq!(glyph_name_to_unicode("Lcommaaccent"), Some('\u{013B}'));
        // Symbol names (Omega maps to OHM SIGN per AGL, not U+03A9):
        assert_eq!(glyph_name_to_unicode("Alpha"), Some('\u{0391}'));
        assert_eq!(glyph_name_to_unicode("Omega"), Some('\u{2126}'));
        // ZapfDingbats aNN names (merged-table deviation, documented):
        assert_eq!(glyph_name_to_unicode("a1"), Some('\u{2701}'));
        // PUA results are returned as-is (extraction layer flags them):
        assert_eq!(glyph_name_to_unicode("commaaccent"), Some('\u{F6C3}'));
        // Unknown names contribute no text:
        assert_eq!(glyph_name_to_unicode("foo"), None);
    }

    #[test]
    fn agl_suffix_stripping() {
        assert_eq!(glyph_name_to_unicode("a.sc"), Some('a'));
        assert_eq!(glyph_name_to_unicode("one.oldstyle"), Some('1'));
        assert_eq!(glyph_name_to_unicode("a.alt.01"), Some('a')); // FIRST period
        assert_eq!(glyph_name_to_unicode("uni0041.alt"), Some('A'));
        assert_eq!(glyph_name_to_unicode(".notdef"), None); // empty base
        assert_eq!(glyph_name_to_unicode("."), None);
    }

    #[test]
    fn agl_uni_form() {
        assert_eq!(glyph_name_to_unicode("uni0041"), Some('A'));
        assert_eq!(glyph_name_to_unicode("uni20AC"), Some('\u{20AC}'));
        assert_eq!(glyph_name_to_unicode("uni20ac"), None); // lowercase hex
        assert_eq!(glyph_name_to_unicode("uniD801"), None); // surrogate
        assert_eq!(glyph_name_to_unicode("uniD801DC0C"), None); // never a UTF-16 pair
        assert_eq!(glyph_name_to_unicode("uni004"), None); // not 4 digits
        assert_eq!(glyph_name_to_unicode("uni20AC0308"), None); // multi-scalar: API deviation
        assert_eq!(glyph_name_to_unicode("united"), None); // not hex at all
    }

    #[test]
    fn agl_u_form() {
        assert_eq!(glyph_name_to_unicode("u0041"), Some('A'));
        assert_eq!(glyph_name_to_unicode("u1040C"), Some('\u{1040C}')); // 5 digits
        assert_eq!(glyph_name_to_unicode("u10FFFF"), Some('\u{10FFFF}')); // 6 digits
        assert_eq!(glyph_name_to_unicode("u110000"), None); // beyond Unicode
        assert_eq!(glyph_name_to_unicode("uD800"), None); // surrogate
        assert_eq!(glyph_name_to_unicode("u041"), None); // 3 digits
        assert_eq!(glyph_name_to_unicode("u0000041"), None); // 7 digits
        assert_eq!(glyph_name_to_unicode("u1040c"), None); // lowercase
    }

    #[test]
    fn agl_ligature_names_are_unrepresentable() {
        // f_i maps to "fi" (two scalars) in full AGL — documented deviation:
        // the char-level API declines rather than guessing.
        assert_eq!(glyph_name_to_unicode("f_i"), None);
        // NB: the single-glyph ligature NAME "fi" (U+FB01) is a plain table
        // hit, distinct from the two-component f_i.
        assert_eq!(glyph_name_to_unicode("fi"), Some('\u{FB01}'));
    }

    #[test]
    fn every_annex_d_encoded_name_resolves_to_unicode() {
        // Chain check: any glyph name reachable through a predefined Latin
        // encoding must yield extractable text.
        for enc in [
            BaseEncoding::Standard,
            BaseEncoding::WinAnsi,
            BaseEncoding::MacRoman,
        ] {
            for code in 0..=255u8 {
                if let Some(name) = encoding_glyph_name(enc, code) {
                    assert!(
                        glyph_name_to_unicode(name).is_some(),
                        "{enc:?} code {code} -> {name} has no Unicode"
                    );
                }
            }
        }
    }
}
