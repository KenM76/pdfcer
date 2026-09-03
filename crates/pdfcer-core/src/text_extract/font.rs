//! # The §9.10.2 ladder — a font dictionary resolved for EXTRACTION
//!
//! This module is the extraction-direction counterpart of
//! `pdfcer-render`'s `text::load`, and it is deliberately a **separate
//! implementation over the same font dictionary**, not a shared one.
//! §9.10.1 states the reason in one sentence: "Unicode values identify
//! characters, not glyphs." The rendering direction asks *which glyph*
//! and needs the font program; the extraction direction asks *which
//! character* and — for three of the four rungs — needs no font program
//! at all. Merging them would force `pdfcer-core` to depend on a font
//! rasterization stack it must not have (R21: `skrifa` lives in
//! `pdfcer-render`).
//!
//! Spec sources in the PDF-spec RAG: `iso32000__s__9.10.md` (the
//! ladder), `iso32000__s__9.10.3.md` (rung 1's data format),
//! `iso32000__s__9.6.6.md` (rung 2's encoding resolution),
//! `iso32000__annex__d.md` + `font__agl.md` (rung 2's tables),
//! `iso32000__s__9.7.md` (composite fonts, `CIDSystemInfo`).
//!
//! ## The ladder, verbatim, in priority order (§9.10.2)
//!
//! | Rung | Precondition | pdfcer status |
//! |---|---|---|
//! | **1** `/ToUnicode` | the entry is present in the font dictionary — **any** `Subtype`, no other test | **complete** ([`super::cmap`]) |
//! | **2** simple + standard names | `/Encoding` is `MacRomanEncoding`/`MacExpertEncoding`/`WinAnsiEncoding`, **or** `/Differences` contains only Adobe-standard-Latin ∪ `Symbol` names | **complete** (Annex D.2 + AGL, both already in `pdfcer-core`) |
//! | **3** composite + known collection | a Table 118 predefined CMap **except `Identity-H`/`Identity-V`**, **or** a descendant CIDFont in `Adobe-GB1`/`-CNS1`/`-Japan1`/`-Korea1` | **structural only** — the `registry-ordering-UCS2` CMaps are Adobe resource files pdfcer does not bundle. Diagnosed by name, never silently skipped. |
//! | **4** failure | everything above failed | U+FFFD **counted**, never a "code of our choosing" |
//!
//! ## The one fact that shapes everything: the `Identity-H` dead end
//!
//! Rung 3 excludes `Identity-H`/`Identity-V` **by name** from its first
//! disjunct, and a font whose descendant declares `Adobe-Identity-0`
//! satisfies neither disjunct of the second. That is exactly what every
//! modern subsetting producer emits. So for the common modern document:
//!
//! > **`/ToUnicode` or nothing.**
//!
//! §9.10.3's EXAMPLE 1 says it from the other side: "In the absence of a
//! `ToUnicode` entry, no information would be available about what the
//! glyphs mean." When pdfcer meets such a font it reports
//! [`Rung3Gap::IdentityNoToUnicode`] and emits U+FFFD per code — it does
//! **not** reconstruct text from glyph indices, because a GID is a
//! position in a subset font's glyph table and carries no character
//! identity whatsoever. That counter is this Pass's headline honesty
//! metric.
//!
//! ## Two documented deviations from a literal reading
//!
//! Both are *additive* (they recover text a literal reading would drop),
//! both are counted under their own rung so nothing is disguised as
//! sourced, and both are recorded here because §9.10.2 states them
//! nowhere.
//!
//! 1. **Per-code fallthrough.** §9.10.2 method 1's precondition is the
//!    *presence* of `/ToUnicode`, not its completeness, and §9.10.3 N4
//!    records that the standard says nothing about a code the CMap does
//!    not cover — there is no `notdef` analogue and no stated
//!    fallthrough. pdfcer falls through to rung 2 per code. Every real
//!    reader does; none of them can cite a clause for it.
//! 2. **Glyph-name extension.** §9.10.2 method 2's `Differences` test is
//!    a *whole-array* test — "an encoding whose `Differences` array
//!    includes only character names taken from…" — so one vendor-private
//!    name anywhere in the array disqualifies the entire font, by the
//!    letter of the clause. pdfcer still resolves the codes whose names
//!    *do* map through the AGL, and reports them as
//!    [`LadderRung::GlyphNameExtension`] rather than as rung 2. The
//!    strict precondition still decides what counts as rung 2.
//!
//! ## What `pdfcer-core` cannot see, and says so
//!
//! §9.6.6.1's implicit-base decision table sends an **embedded**
//! symbolic font to "the font program's built-in encoding" — a table
//! that lives inside the font file, reachable only through a font
//! parser. R21 keeps that parser in `pdfcer-render`, so extraction cannot
//! read it. pdfcer substitutes `StandardEncoding` as the base table,
//! reports [`FontNote::BuiltinEncodingUnreadable`], and lets rung 2's
//! strict precondition fail (such a font has no named encoding), so the
//! recovered characters surface as the extension of deviation 2 above,
//! never as sourced rung-2 output. **This is the one case the brief
//! predicted might be unreachable without font-program access, and it
//! is: it is unreachable, it is named, and it is counted.**

use crate::filters;
use crate::fontdata::{self, BaseEncoding, Std14};
use crate::graph::ObjectGraph;
use crate::object::{Dict, Object};
use crate::settings::UnmappableCode;
use crate::view::DocumentView;

use super::cmap::ToUnicodeCMap;

/// Which rung of the §9.10.2 ladder produced a character.
///
/// Recorded per glyph so that [`super::ExtractedText`] can answer "how
/// much of this page's text is *sourced*?" — the provenance that makes
/// rule 4 (fuzzy, never sneaky) enforceable rather than aspirational.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum LadderRung {
    /// Rung 1: the font's `/ToUnicode` CMap (§9.10.3). Fully sourced.
    ToUnicode,
    /// Rung 2: a simple font meeting method 2's precondition, resolved
    /// through Annex D.2 and the Adobe Glyph List. Fully sourced.
    EncodingAgl,
    /// Rung 3: a composite font in a known Adobe character collection.
    /// **Never produced by this Pass** — the `registry-ordering-UCS2`
    /// CMaps are not bundled. The variant exists so the rung is a
    /// visible hole in the ladder rather than an absent idea.
    CidCollection,
    /// pdfcer's documented glyph-name extension: the font failed method
    /// 2's whole-array precondition, but this code's resolved glyph name
    /// mapped through the AGL anyway. Recovered text, **not** sourced
    /// under §9.10.2.
    GlyphNameExtension,
    /// Rung 4: the ladder failed. §9.10.2 concedes "there is no way to
    /// determine what the character code represents". pdfcer emits
    /// U+FFFD and counts it.
    Failed,
}

impl LadderRung {
    /// Whether ISO 32000-1 §9.10.2 itself sanctions this result.
    ///
    /// [`LadderRung::GlyphNameExtension`] and [`LadderRung::Failed`] are
    /// `false`: the first is pdfcer's own recovery, the second is the
    /// standard's stated defeat.
    #[must_use]
    pub const fn is_sourced(self) -> bool {
        matches!(
            self,
            Self::ToUnicode | Self::EncodingAgl | Self::CidCollection
        )
    }

    /// A short stable identifier for machine-readable output
    /// (`pdfcer extract-text --json`).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ToUnicode => "to_unicode",
            Self::EncodingAgl => "encoding_agl",
            Self::CidCollection => "cid_collection",
            Self::GlyphNameExtension => "glyph_name_extension",
            Self::Failed => "failed",
        }
    }
}

/// Why rung 3 could not run — each variant is a *named* diagnostic, per
/// R27: a rung pdfcer cannot climb is reported by name, never silently
/// skipped.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Rung3Gap {
    /// The font uses `Identity-H`/`Identity-V` (or an `Adobe-Identity-0`
    /// descendant) and carries no `/ToUnicode`. **No Unicode is
    /// recoverable at all** — this is §9.10.2's sourced answer, not a
    /// pdfcer limitation.
    IdentityNoToUnicode,
    /// The descendant CIDFont declares one of the four Adobe collections
    /// rung 3 names, but the `registry-ordering-UCS2` CMap that maps its
    /// CIDs to Unicode is an Adobe resource file pdfcer does not bundle.
    Ucs2NotBundled {
        /// The constructed CMap name, ASCII hyphens (`Adobe-Japan1-UCS2`).
        /// Note the source renders the separator as an en-dash; the
        /// actual Adobe resource filenames use hyphen-minus.
        cmap_name: String,
    },
    /// A Table 118 predefined CMap other than the Identity pair. pdfcer
    /// bundles neither the code→CID CMap nor the CID→Unicode one.
    PredefinedCmapNotBundled {
        /// The `/Encoding` name as written in the font dictionary.
        cmap_name: String,
    },
}

/// A per-font observation worth disclosing to the operator.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum FontNote {
    /// The font's encoding lives inside its embedded program, which
    /// `pdfcer-core` cannot read (R21 — see the module docs).
    /// `StandardEncoding` was substituted as the base table.
    BuiltinEncodingUnreadable,
    /// `/Subtype` was absent or unrecognized (Table 110). Treated as a
    /// simple font, which is the only guess that recovers anything.
    UnknownSubtype,
    /// The `/ToUnicode` CMap's declared codespace width disagrees with
    /// the width the font's own encoding implies. §9.10.3 makes
    /// consistency a `shall` and states no recovery (N1); pdfcer
    /// segments by the **font's** encoding and reports the conflict.
    CodespaceWidthConflict {
        /// Width implied by the font's `/Encoding`, in bytes.
        font: u8,
        /// Width the `/ToUnicode` CMap declared, in bytes.
        to_unicode: u8,
    },
    /// A `/ToUnicode` stream was present but could not be decoded
    /// (filter failure) or produced no mappings at all.
    ToUnicodeUnusable,
    /// The rung-3 hole, by name.
    Rung3(Rung3Gap),
    /// A **Type 3** font carrying no usable `/ToUnicode` CMap — the
    /// Type 3 dead end, and the exact analogue of
    /// [`Rung3Gap::IdentityNoToUnicode`] for a simple font.
    ///
    /// § 9.6.5 defines a Type 3 glyph as a **content stream** named by an
    /// arbitrary key in `/CharProcs`. That name is private to the one
    /// document: `/g13` or `/square` carries no Unicode meaning, and
    /// §9.10.2 method 2's precondition (the resolved code→name table
    /// drawing only from Adobe standard Latin ∪ Symbol) is therefore
    /// false for a Type 3 font **by construction** — see
    /// `resolve_encoding`'s closing line, which sets it false
    /// unconditionally.
    ///
    /// So rung 1 (`/ToUnicode`) is the only rung a Type 3 font can climb.
    /// Without it, extraction, search and copy of text set in that font
    /// are not merely degraded — they are impossible from the file's own
    /// contents. **This is Acrobat's limit too**, not a pdfcer shortfall:
    /// Acrobat's extract/search/copy pipeline for Type 3 is gated
    /// entirely on `/ToUnicode` (Acrobat_Features
    /// `type3fonts__extraction_editing_and_tagging.md`, verified
    /// 2026-08-25), and its Accessibility Checker fails such a document
    /// for the same reason.
    ///
    /// pdfcer's one **counted extension** beyond that: if a Type 3
    /// producer happened to name its glyphs with standard names, the
    /// Adobe Glyph List still resolves them, and those codes are counted
    /// as [`TextDiagnostics::via_glyph_name_extension`] — *not sourced*,
    /// and never as rung 2. So this note means "the only sourced route is
    /// closed", not "nothing came out".
    ///
    /// [`TextDiagnostics::via_glyph_name_extension`]: super::TextDiagnostics::via_glyph_name_extension
    Type3NoToUnicode,
    /// No width information was available for this font, so advances are
    /// estimated. Positions and derived word spacing downstream of this
    /// font are correspondingly less reliable.
    WidthsEstimated,
}

/// How a show string splits into character codes (§9.4.3, §9.7.6.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CodeWidth {
    /// One byte per code — every simple font (§9.6.1).
    One,
    /// Two bytes per code, high-order first — `Identity-H`/`Identity-V`
    /// and every 2-byte codespace.
    Two,
}

/// A font dictionary resolved into everything extraction needs.
///
/// Built once per distinct font resource per page and shared by
/// reference; the interpreter caches by object id so a font referenced
/// from ten pages is resolved once.
#[derive(Debug, Clone)]
pub struct ExtractFont {
    /// `/BaseFont` verbatim, subset tag included — the name a diagnostic
    /// names.
    pub base_font: String,
    /// Rung 1's data, if the font carries `/ToUnicode`.
    to_unicode: Option<ToUnicodeCMap>,
    /// Rung 2's data for a simple font: the resolved code→glyph-name
    /// table (base encoding with `/Differences` applied).
    glyph_names: Option<Box<[Option<String>; 256]>>,
    /// Whether §9.10.2 method 2's precondition holds for this font, as
    /// literally stated. Drives rung classification, not availability —
    /// see the module docs' deviation 2.
    rung2_precondition: bool,
    /// Code segmentation.
    width: CodeWidth,
    /// Advance widths, glyph space (1000/em unless `width_scale` says
    /// otherwise).
    widths: Widths,
    /// Glyph-space → text-space scale. `0.001` for every font except
    /// Type 3, where §9.6.5's `/FontMatrix` sets it.
    width_scale: f32,
    /// The font's vertical extent, text space (§9.8 Table 122).
    vertical: Vertical,
    /// Everything worth disclosing about this font.
    pub notes: Vec<FontNote>,
}

/// A font's vertical extent in **text space** (already divided by the
/// glyph-space scale), plus where it came from.
///
/// ISO 32000-1 §9.8 Table 122 defines `/Ascent` ("the maximum height above
/// the baseline reached by glyphs in this font", accents excluded) and
/// `/Descent` ("the maximum depth below the baseline… shall be a negative
/// number"), both **required** for every font descriptor except a Type 3's,
/// and both "units in glyph space" (§9.8.1). Together they are the only
/// dictionary-sourced answer to *how tall is a line of this font* — the
/// vertical half of a text object's bounding box, exactly as the widths are
/// its horizontal half.
///
/// The `nominal` flag is the honesty half: when neither the descriptor, its
/// `/FontBBox`, nor a compiled-in standard-14 descriptor could supply the
/// pair, the numbers below are a **guess of pdfcer's own** and every consumer
/// that reports a box built from them must say so rather than present the
/// guess as the font's designed metrics (rule 4).
#[derive(Debug, Clone, Copy)]
struct Vertical {
    /// Maximum height above the baseline, text space, positive.
    ascent: f32,
    /// Maximum depth below the baseline, text space, **negative**.
    descent: f32,
    /// `true` when [`NOMINAL_ASCENT`]/[`NOMINAL_DESCENT`] were substituted
    /// because the font supplied nothing usable.
    nominal: bool,
}

/// The ascent pdfcer substitutes when a font supplies no usable vertical
/// metrics — one full em above the baseline.
///
/// Deliberately the same number `redact`'s glyph box uses
/// (`GLYPH_BOX_ASCENT`), and for the same reason: with nothing to measure,
/// **over-covering is the safe direction**. A hit target that is slightly
/// too tall costs a click that selects text the operator was aiming just
/// above; one that is too short costs a click on the letters themselves
/// doing nothing, which is the failure this whole mechanism exists to
/// remove. One em is above every real font's `/Ascent` (Helvetica's is
/// 0.718 em) without being absurd.
const NOMINAL_ASCENT: f32 = 1.0;

/// The descent pdfcer substitutes alongside [`NOMINAL_ASCENT`] — a quarter
/// em below the baseline, negative per §9.8 Table 122's sign convention,
/// and again `redact`'s `GLYPH_BOX_DESCENT` value.
const NOMINAL_DESCENT: f32 = -0.25;

/// **The one advance formula.** ISO 32000-1 §9.4.4's horizontal glyph
/// displacement, in unscaled text-space units.
///
/// > `tx = ((w0 − Tj/1000) × Tfs + Tc + Tw) × Th`
///
/// This function takes `w0` with the `Tj` term **already folded in** (the
/// callers that support `TJ` apply the array's kerning offsets to the text
/// matrix as they meet them, so the offset never reaches here), leaving:
///
/// > `tx = (w0 × Tfs + Tc + Tw) × Th`
///
/// where `w0` is the glyph's width already in text space (what
/// [`ExtractFont::width`] returns), `Tfs` the `Tf` size, `Tc` the character
/// spacing (§9.3.2), `Tw` the word spacing (§9.3.3 — the caller decides
/// whether it applies, since it fires only for the single-byte code 32),
/// and `Th` the horizontal scaling `Tz/100` (§9.3.4).
///
/// **Why it is a function at all.** `pdfcer-core` had this arithmetic
/// written out three times (extraction's `show_code`, redaction's `glyph`,
/// and the text-edit layout path) before a fourth consumer — the vector
/// object model's text bounding box — needed it. Three copies that agree
/// today are three copies that can disagree tomorrow, which decision 011
/// §Z2 names as this project's recurring failure shape. Every consumer now
/// calls this; `pdfcer-render`'s `TextState::advance_for` is the same
/// formula stated in the rendering crate, which cannot depend on core's
/// internals, and is cross-checked by the render-parity gate rather than
/// shared.
///
/// **The same failure shape, one level up, is also closed (Pass 19.0).**
/// The *arguments* to this function came from three private, independently
/// maintained text-state trackers — `text_extract::page::TextState`,
/// `text_edit::edit::Walk` (+ `reflow_apply::BlockTextState`) and
/// `vector::decompose::GState` — and they had **already diverged**: the
/// authoring one tracked neither `Ts` nor `Tr`, and none of them handled
/// `q`/`Q`. Consolidating the formula while leaving three copies of its
/// inputs was half the job. All three now compose
/// [`crate::text_state`]'s single model. `pdfcer-render`'s own tracker is
/// deliberately still separate, for the same crate-boundary reason as the
/// formula itself, and under the same render-parity cross-check.
///
/// `f64` throughout because two of the three callers already work in `f64`
/// and the third (extraction) is narrowed back to `f32` at its own call
/// site; doing the arithmetic in the wider type never loses precision the
/// narrower one had.
pub(crate) fn advance_tx(w0: f64, tfs: f64, tc: f64, tw: f64, th: f64) -> f64 {
    (w0 * tfs + tc + tw) * th
}

/// Advance-width tables, in the two shapes the two font models use.
#[derive(Debug, Clone)]
enum Widths {
    /// §9.6.2.1: `/FirstChar` + `/Widths`, else the standard-14 AFM
    /// tables, else `/MissingWidth`.
    Simple(Box<[f32; 256]>),
    /// §9.7.4.3: `/W` ranges over `/DW`. Deliberately not materialized —
    /// CIDs run to 65,535 and `7080 8032 1000` is the point of the
    /// format.
    Composite {
        default: f32,
        ranges: Vec<(u32, u32, f32)>,
    },
}

impl ExtractFont {
    /// This font's `/ToUnicode` CMap, if it declared one.
    ///
    /// Exposed for standing rule R110: whether a COMPOSITE run is editable
    /// turns on whether its CMap is injective, and that is a property of the
    /// CMap rather than of anything else `ExtractFont` publishes. Read-only
    /// borrow — the CMap is built once per font per extraction and shared.
    #[must_use]
    pub fn to_unicode_cmap(&self) -> Option<&ToUnicodeCMap> {
        self.to_unicode.as_ref()
    }
    /// Resolve a font dictionary for extraction.
    ///
    /// Infallible: unlike the rendering side there is no "this font is
    /// out of scope" outcome. A Type 3 font has no glyph program worth
    /// parsing here and extracts perfectly well through its
    /// `/Differences` names; a composite font with no embedded program
    /// (which §9.7.5.2 forbids for `Identity-H`, and which rendering
    /// must refuse) still has a `/ToUnicode` that maps its codes. Every
    /// degradation becomes a [`FontNote`], never a dropped font.
    pub fn resolve(doc: &DocumentView<'_>, font_dict: &Dict) -> Self {
        let mut notes = Vec::new();
        let subtype = name_of(doc, font_dict, b"Subtype").unwrap_or_default();
        let base_font = name_of(doc, font_dict, b"BaseFont").unwrap_or_default();

        let to_unicode = load_to_unicode(doc, font_dict, &mut notes);

        match subtype.as_str() {
            "Type0" => Self::resolve_composite(doc, font_dict, base_font, to_unicode, notes),
            "Type1" | "MMType1" | "TrueType" | "Type3" => {
                let is_type3 = subtype == "Type3";
                Self::resolve_simple(doc, font_dict, base_font, to_unicode, notes, is_type3)
            }
            _ => {
                notes.push(FontNote::UnknownSubtype);
                Self::resolve_simple(doc, font_dict, base_font, to_unicode, notes, false)
            }
        }
    }

    /// §9.6 simple font (and Type 3, which extracts the same way).
    fn resolve_simple(
        doc: &DocumentView<'_>,
        font_dict: &Dict,
        base_font: String,
        to_unicode: Option<ToUnicodeCMap>,
        mut notes: Vec<FontNote>,
        is_type3: bool,
    ) -> Self {
        let std14 = fontdata::std14_by_base_font(strip_subset_tag(&base_font));

        // The Type 3 dead end, recorded where the evidence lives.
        //
        // Deliberately BEFORE the encoding is resolved, and deliberately
        // not conditioned on anything the encoding produces: whether a
        // Type 3 font has a sourced route to Unicode is a property of the
        // FONT DICTIONARY alone (§9.6.5 + §9.10.2), decided the moment
        // `/ToUnicode` is absent. Conditioning it on "did any code
        // actually fail" would make the disclosure depend on which page
        // happened to be extracted, and a document whose Type 3 text sits
        // on page 40 would report nothing for the first 39 — the exact
        // shape of silence rule 4 forbids.
        if is_type3 && to_unicode.is_none() {
            notes.push(FontNote::Type3NoToUnicode);
        }

        let (names, rung2_precondition) =
            resolve_encoding(doc, font_dict, std14, is_type3, &mut notes);
        let widths = simple_widths(doc, font_dict, &names, std14, &mut notes);

        // §9.6.5: a Type 3 font's glyph space is defined by /FontMatrix,
        // so its /Widths are in THAT space, not the 1000/em space every
        // other font uses. Reading a Type 3's widths as thousandths
        // scales every advance on the page by a random factor.
        let width_scale = if is_type3 {
            type3_width_scale(doc, font_dict)
        } else {
            0.001
        };

        if let Some(cmap) = &to_unicode
            && let Some(&w) = cmap.codespace_widths().first()
            && w != 1
        {
            notes.push(FontNote::CodespaceWidthConflict {
                font: 1,
                to_unicode: w,
            });
        }

        // §9.8.1: "Beginning with PDF 1.5, font descriptors may be used
        // with Type 3 fonts" — but a Type 3's descriptor numbers are in
        // ITS glyph space, which `/FontMatrix` defines with six elements,
        // not the single horizontal scale `width_scale` carries. Rather
        // than scale a vertical extent by a horizontal factor, a Type 3
        // takes the nominal fallback and SAYS it is nominal.
        let vertical = if is_type3 {
            Vertical {
                ascent: NOMINAL_ASCENT,
                descent: NOMINAL_DESCENT,
                nominal: true,
            }
        } else {
            let descriptor = doc
                .resolve(font_dict.get(b"FontDescriptor").unwrap_or(&Object::Null))
                .as_dict();
            resolve_vertical(doc, descriptor, std14)
        };

        Self {
            base_font,
            to_unicode,
            glyph_names: Some(names),
            rung2_precondition,
            width: CodeWidth::One,
            widths,
            width_scale,
            vertical,
            notes,
        }
    }

    /// §9.7 composite font.
    fn resolve_composite(
        doc: &DocumentView<'_>,
        font_dict: &Dict,
        base_font: String,
        to_unicode: Option<ToUnicodeCMap>,
        mut notes: Vec<FontNote>,
    ) -> Self {
        let encoding = name_of(doc, font_dict, b"Encoding").unwrap_or_default();
        // "PDF supports only a single descendant" (§9.7.1) — but it is
        // still an ARRAY (Table 121).
        let descendant = doc
            .resolve(font_dict.get(b"DescendantFonts").unwrap_or(&Object::Null))
            .as_array()
            .and_then(<[Object]>::first)
            .map(|o| doc.resolve(o))
            .and_then(Object::as_dict);

        let collection = descendant
            .and_then(|d| doc.resolve(d.get(b"CIDSystemInfo")?).as_dict())
            .map(|info| {
                (
                    string_of(doc, info, b"Registry"),
                    string_of(doc, info, b"Ordering"),
                )
            });

        // Rung 3's two disjuncts, evaluated exactly as §9.10.2 states
        // them — and the exclusion that decides the modern case.
        let identity = matches!(encoding.as_str(), "Identity-H" | "Identity-V");
        let known_collection = collection.as_ref().is_some_and(|(r, o)| {
            r == "Adobe" && matches!(o.as_str(), "GB1" | "CNS1" | "Japan1" | "Korea1")
        });

        if to_unicode.is_none() {
            let gap = if known_collection {
                let (registry, ordering) = collection.clone().unwrap_or_default();
                Rung3Gap::Ucs2NotBundled {
                    // ASCII hyphen-minus: the source renders the
                    // separator as an en-dash, but the Adobe resource
                    // filenames do not.
                    cmap_name: format!("{registry}-{ordering}-UCS2"),
                }
            } else if identity || encoding.is_empty() {
                Rung3Gap::IdentityNoToUnicode
            } else {
                Rung3Gap::PredefinedCmapNotBundled {
                    cmap_name: encoding.clone(),
                }
            };
            notes.push(FontNote::Rung3(gap));
        }

        // Code segmentation. The Identity CMaps are 2-byte by
        // definition (§9.7.5.2). An embedded CMap stream declares its
        // own codespace, which the same parser already reads. A
        // predefined CJK CMap's codespace lives in an Adobe resource
        // file pdfcer does not bundle: 2 bytes is the right guess for
        // every Table 118 CMap's *dominant* codespace, and the
        // already-emitted rung-3 note is what discloses the guess.
        let width = embedded_cmap_width(doc, font_dict).unwrap_or(CodeWidth::Two);

        if let Some(cmap) = &to_unicode
            && let Some(&w) = cmap.codespace_widths().first()
            && w != 2
            && width == CodeWidth::Two
        {
            notes.push(FontNote::CodespaceWidthConflict {
                font: 2,
                to_unicode: w,
            });
        }

        let widths = descendant.map_or(
            Widths::Composite {
                default: 1000.0,
                ranges: Vec::new(),
            },
            |d| composite_widths(doc, d),
        );

        // §9.8.1: "Font descriptors shall not be used with Type 0 fonts."
        // The descriptor belongs to the DESCENDANT CIDFont, so that is
        // where the vertical extent is read from. A Type 0 has no
        // standard-14 identity, so there is no AFM rung here — either the
        // descendant supplies metrics or the extent is nominal.
        let vertical = resolve_vertical(
            doc,
            descendant.and_then(|d| doc.resolve(d.get(b"FontDescriptor")?).as_dict()),
            None,
        );

        Self {
            base_font,
            to_unicode,
            glyph_names: None,
            rung2_precondition: false,
            width,
            widths,
            width_scale: 0.001,
            vertical,
            notes,
        }
    }

    /// Split a show string into character codes (§9.4.3).
    ///
    /// A trailing odd byte under 2-byte segmentation is malformed and
    /// §9.7.6.3's recovery does not cleanly apply (the Identity
    /// codespace is a single 2-byte range). pdfcer consumes it as a high
    /// byte with a zero low byte — a documented choice, not spec text —
    /// so the string is fully consumed and the loop cannot stall.
    pub(crate) fn codes(&self, string: &[u8]) -> Vec<Code> {
        match self.width {
            CodeWidth::One => string
                .iter()
                .map(|&b| Code {
                    value: u32::from(b),
                    // §9.3.3: word spacing applies to "occurrences of
                    // the single-byte character code 32", and "shall not
                    // apply to occurrences of the byte value 32 in
                    // multiple-byte codes".
                    word_spacing_applies: b == 32,
                })
                .collect(),
            CodeWidth::Two => string
                .chunks(2)
                .map(|pair| {
                    let hi = u32::from(pair.first().copied().unwrap_or(0));
                    let lo = u32::from(pair.get(1).copied().unwrap_or(0));
                    Code {
                        value: (hi << 8) | lo,
                        word_spacing_applies: false,
                    }
                })
                .collect(),
        }
    }

    /// **The ladder.** Map one character code to Unicode, trying
    /// §9.10.2's methods in the priority the clause gives.
    ///
    /// Returns the characters produced (possibly several — one code to
    /// many code points is normal, see [`super::cmap`]) and the rung
    /// that produced them.
    ///
    /// ## The rung-4 sentinel is the operator's choice (`TX-A1`, R169)
    ///
    /// §9.10.2 N3 records that the standard names **no** sentinel — not
    /// U+FFFD, not omission, not a placeholder — and its failure sentence
    /// is grammatically broken besides (*"may choose a character code of
    /// their choosing"* where a **Unicode value** is what is produced).
    /// So this is disclosed pdfcer policy rather than conformance, and
    /// under R169 a genuine spec silence becomes a setting: `sentinel`
    /// carries [`UnmappableCode`], defaulting to U+FFFD — the shipped
    /// behaviour, **evidence tier (d)**, a reasoned guess.
    ///
    /// A **parameter, not a field on `ExtractFont`**: the same resolved
    /// font can legitimately be walked twice under two different
    /// extraction options within one session (a redaction preview versus
    /// a clipboard copy), and baking the sentinel into the font would make
    /// which one you got depend on which walk loaded the font first.
    ///
    /// The rung is returned unchanged in every case. Whatever the sentinel
    /// is, [`LadderRung::Failed`] is still reported and still counted —
    /// the setting chooses what the failure *looks like*, never whether
    /// the failure is admitted.
    pub(crate) fn to_unicode(&self, code: u32, sentinel: UnmappableCode) -> (String, LadderRung) {
        // Rung 1 — presence of /ToUnicode is the entire precondition,
        // for every Subtype including Type 3.
        if let Some(cmap) = &self.to_unicode
            && let Some(text) = cmap.lookup(code)
        {
            return (text, LadderRung::ToUnicode);
        }

        // Rung 2 — simple font, standard names. (Reached per code, not
        // per font: see the module docs' deviation 1.)
        if let Some(names) = &self.glyph_names
            && let Some(Some(name)) = usize::try_from(code).ok().and_then(|i| names.get(i))
            && let Some(text) = fontdata::glyph_name_to_unicode_string(name)
        {
            let rung = if self.rung2_precondition {
                LadderRung::EncodingAgl
            } else {
                // Deviation 2: the font failed method 2's whole-array
                // precondition, so this is pdfcer's extension, not the
                // standard's rung 2.
                LadderRung::GlyphNameExtension
            };
            return (text, rung);
        }

        // Rung 3 would go here. It cannot: see `Rung3Gap`.

        // Rung 4 — "there is no way to determine what the character code
        // represents". What that looks like is `TX-A1`, above.
        let text = match sentinel {
            UnmappableCode::ReplacementChar => char::REPLACEMENT_CHARACTER.to_string(),
            UnmappableCode::QuestionMark => "?".to_owned(),
            // Deliberately an empty string rather than an early return
            // with a different rung: the code still HAPPENED, it still
            // advances the text matrix, and it is still a rung-4 failure
            // in the diagnostics. Only the text is empty.
            //
            // Downstream consequence, documented on the variant and
            // pinned by a test: `layout::Builder::close_run` drops a run
            // whose text is empty, glyph records and all — so a run of
            // wholly-unmappable codes vanishes under this setting rather
            // than surviving as positioned, character-less glyphs. The
            // COUNT survives regardless, which is what keeps `omit`
            // honest; the positions do not.
            UnmappableCode::Omit => String::new(),
        };
        (text, LadderRung::Failed)
    }

    /// Advance width for a code, in **text space** (already divided by
    /// the glyph-space scale, so the caller multiplies by the font size
    /// directly).
    pub(crate) fn width(&self, code: u32) -> f32 {
        let glyph_space = match &self.widths {
            Widths::Simple(table) => usize::try_from(code)
                .ok()
                .and_then(|i| table.get(i))
                .copied()
                .unwrap_or(0.0),
            Widths::Composite { default, ranges } => ranges
                .iter()
                .find(|&&(first, last, _)| code >= first && code <= last)
                .map_or(*default, |&(_, _, w)| w),
        };
        glyph_space * self.width_scale
    }

    /// The font's maximum height above the baseline, **text space**
    /// (§9.8 Table 122 `/Ascent`), so a caller multiplies by the `Tf` size
    /// directly — the vertical twin of [`Self::width`].
    ///
    /// Always positive. See [`Vertical`] for the resolution ladder and for
    /// what [`Self::vertical_is_nominal`] discloses.
    pub(crate) fn ascent(&self) -> f32 {
        self.vertical.ascent
    }

    /// The font's maximum depth below the baseline, **text space**
    /// (§9.8 Table 122 `/Descent`). **Negative**, per the clause's own
    /// stated sign convention.
    pub(crate) fn descent(&self) -> f32 {
        self.vertical.descent
    }

    /// Whether [`Self::ascent`]/[`Self::descent`] are pdfcer's nominal
    /// fallback rather than the font's own numbers.
    ///
    /// A consumer that draws a box from them must disclose this: a box
    /// built on a guess and a box built on the file's declared metrics are
    /// two different claims, and presenting the first as the second is the
    /// exact shape rule 4 forbids.
    pub(crate) fn vertical_is_nominal(&self) -> bool {
        self.vertical.nominal
    }

    /// How many bytes one character code occupies in a show string: one
    /// for every simple font (§9.6.1), two for a composite font's 2-byte
    /// codespace (`Identity-H`/`Identity-V`, §9.7.6.2).
    ///
    /// Exposed for the redaction content-surgery interpreter
    /// ([`crate::redact`]), which must slice a show string on **code**
    /// boundaries — never in the middle of a multi-byte CID — when it
    /// removes an in-region run and re-emits the surviving segments.
    #[must_use]
    pub(crate) fn bytes_per_code(&self) -> usize {
        match self.width {
            CodeWidth::One => 1,
            CodeWidth::Two => 2,
        }
    }

    /// Whether this font's advance widths had to be estimated (no
    /// `/Widths`, not a standard-14 face). Redaction discloses this: an
    /// estimated width degrades only the *cosmetic* quality of advance
    /// preservation, never the removal itself.
    #[must_use]
    pub(crate) fn width_estimated(&self) -> bool {
        self.notes
            .iter()
            .any(|n| matches!(n, FontNote::WidthsEstimated))
    }

    /// The `/BaseFont` name, for disclosure de-duplication.
    #[must_use]
    pub(crate) fn base_font_name(&self) -> String {
        self.base_font.clone()
    }

    /// The resolved code->glyph-name table `E` (§9.6.6: base encoding with
    /// `/Differences` applied), or `None` for a composite font (which has
    /// no simple-font `/Encoding` to invert).
    ///
    /// This is the **forward** encoding table Pass 4 built for extraction;
    /// the Pass 14.1 inverse-encoding builder
    /// ([`crate::text_edit::encoding`]) inverts it -- deliberately the
    /// font's OWN resolved chain, never `/ToUnicode` (which is one-way and
    /// lossy, `iso32000__ref__inverse_encoding.md` §0). Exposed
    /// `pub(crate)` for that one caller; extraction never reads it back.
    #[must_use]
    pub(crate) fn glyph_names(&self) -> Option<&[Option<String>; 256]> {
        self.glyph_names.as_deref()
    }

    /// Whether this is a **simple** font (one byte per code, an invertible
    /// `/Encoding` table) rather than a composite Type 0 / CIDFont. The
    /// Pass 14.1 gate refuses composite editing (R-INV-4), so it asks this
    /// first.
    ///
    /// # Why this is `pub` (Pass 19.0)
    ///
    /// It was `pub(crate)` over a private `CodeWidth` enum, which meant the
    /// only way for a caller outside the crate to learn whether a run was
    /// composite was to **attempt an edit and read the refusal**. Two
    /// behaviours downstream need the answer *before* acting: `Tw` is
    /// spec-void for multi-byte codes (§9.3.3), and R83 ("no affordance
    /// without the capability") requires the shell to be able to ask
    /// whether a control would do anything before it draws one. The
    /// answer is now also published per-run as
    /// [`GlyphProvenance::composite`](super::GlyphProvenance::composite);
    /// this accessor is the same fact reachable from a resolved font.
    ///
    /// `CodeWidth` itself stays private: it is a segmentation detail with
    /// exactly two states, and a `bool` at the boundary is the smaller
    /// public surface (Rust API Guidelines C-STRUCT-PRIVATE).
    #[must_use]
    pub fn is_simple(&self) -> bool {
        matches!(self.width, CodeWidth::One)
    }
}

/// One character code taken off a shown string (§9.4.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Code {
    /// The code value: one byte for a simple font, two big-endian bytes
    /// for a composite one.
    pub value: u32,
    /// Whether §9.3.3's word-spacing rule applies — true **only** for a
    /// single-byte code whose value is 32.
    ///
    /// This flag is why `Tw` is useless as a word-break heuristic in
    /// modern documents: under `Identity-H` every code is multi-byte, so
    /// `Tw` is inert (`iso32000__s__14.8.md` S6).
    pub word_spacing_applies: bool,
}

// ---------------------------------------------------------------------------
// Rung 1 — /ToUnicode
// ---------------------------------------------------------------------------

/// Load and parse the `/ToUnicode` stream, if present.
///
/// A present-but-broken `/ToUnicode` is reported
/// ([`FontNote::ToUnicodeUnusable`]) rather than silently ignored: the
/// difference between "this font has no Unicode information" and "this
/// font has Unicode information pdfcer could not read" is exactly the
/// difference an operator needs to act on.
fn load_to_unicode(
    doc: &DocumentView<'_>,
    font_dict: &Dict,
    notes: &mut Vec<FontNote>,
) -> Option<ToUnicodeCMap> {
    let entry = font_dict.get(b"ToUnicode")?;
    let Object::Stream(stream) = doc.resolve(entry) else {
        // Table 122 requires a stream; anything else is malformed.
        notes.push(FontNote::ToUnicodeUnusable);
        return None;
    };
    // `view.slice(span)` (Pass 17.1): a `/ToUnicode` CMap this session
    // authored lives in the R45 staging buffer, whose spans start past the
    // end of the base file. `None` keeps its existing meaning — an
    // unresolvable stream is an unusable CMap, counted, never fatal.
    let Some(raw) = doc.slice(stream.data_span) else {
        notes.push(FontNote::ToUnicodeUnusable);
        return None;
    };
    let Ok(decoded) = filters::decode_stream(&stream.dict, raw) else {
        notes.push(FontNote::ToUnicodeUnusable);
        return None;
    };
    let cmap = ToUnicodeCMap::parse(&decoded);
    if cmap.is_empty() {
        notes.push(FontNote::ToUnicodeUnusable);
        return None;
    }
    Some(cmap)
}

// ---------------------------------------------------------------------------
// Rung 2 — encoding resolution (§9.6.6) and its precondition (§9.10.2)
// ---------------------------------------------------------------------------

/// Resolve a simple font's code→glyph-name table and evaluate §9.10.2
/// method 2's precondition.
///
/// Returns `(table, precondition_holds)`. The table is built even when
/// the precondition fails — see the module docs' deviation 2.
fn resolve_encoding(
    doc: &DocumentView<'_>,
    font_dict: &Dict,
    std14: Option<Std14>,
    is_type3: bool,
    notes: &mut Vec<FontNote>,
) -> (Box<[Option<String>; 256]>, bool) {
    let encoding = doc.resolve(font_dict.get(b"Encoding").unwrap_or(&Object::Null));

    // --- step 1: the base table (§9.6.6.1 / Table 114) ---
    let named_base: Option<&[u8]> = match encoding {
        Object::Name(n) => Some(n.as_bytes()),
        Object::Dict(d) => match d.get(b"BaseEncoding").map(|o| doc.resolve(o)) {
            Some(Object::Name(n)) => Some(n.as_bytes()),
            _ => None,
        },
        _ => None,
    };

    // §9.10.2 method 2's FIRST disjunct, literally: the three named
    // encodings. `StandardEncoding` is conspicuously absent from the
    // list (§9.10.2 N6) — and so it is absent here.
    let named_qualifies = matches!(
        named_base,
        Some(b"MacRomanEncoding" | b"MacExpertEncoding" | b"WinAnsiEncoding")
    );

    let base = match named_base {
        Some(b"WinAnsiEncoding") => Some(BaseEncoding::WinAnsi),
        Some(b"MacRomanEncoding") => Some(BaseEncoding::MacRoman),
        // §9.6.6.1 says a reader "shall NOT have a predefined encoding
        // named StandardEncoding", so it is not a legal value — but real
        // producers write it, and reading it as Annex D.2's STD column
        // is the only sensible recovery. MacExpertEncoding is legal and
        // its table is not in the corpus yet.
        Some(b"StandardEncoding") => Some(BaseEncoding::Standard),
        _ => std14.map(fontdata::std14_builtin_encoding),
    };

    let base = match base {
        Some(b) => b,
        None => {
            // A Type 3 font legitimately has no base encoding — its
            // /Differences array IS the encoding (§9.6.5), so there is
            // nothing unreadable and nothing to report.
            if !is_type3 {
                notes.push(FontNote::BuiltinEncodingUnreadable);
            }
            BaseEncoding::Standard
        }
    };

    let mut table: Box<[Option<String>; 256]> = Box::new(std::array::from_fn(|code| {
        u8::try_from(code)
            .ok()
            .and_then(|c| fontdata::encoding_glyph_name(base, c))
            .map(str::to_owned)
    }));

    // --- step 2: /Differences over the base (§9.6.6.1) ---
    // "an integer sets the current code, each following name assigns and
    // increments." A leading name with no integer is malformed; the
    // spec RAG says diagnose rather than guess a start of 0, and
    // skipping is that diagnosis here.
    let mut differences_seen = false;
    let mut differences_all_standard = true;
    if let Object::Dict(d) = encoding
        && let Some(items) = d.get(b"Differences").map(|o| doc.resolve(o))
        && let Some(items) = items.as_array()
    {
        differences_seen = true;
        let mut cur: Option<usize> = None;
        for item in items {
            match doc.resolve(item) {
                Object::Integer(v) => cur = usize::try_from(*v).ok(),
                Object::Real(v) => cur = usize::try_from(*v as i64).ok(),
                Object::Name(n) => {
                    let name = String::from_utf8_lossy(n.as_bytes()).into_owned();
                    if !fontdata::is_standard_latin_or_symbol_name(&name) {
                        differences_all_standard = false;
                    }
                    if let Some(code) = cur {
                        if let Some(slot) = table.get_mut(code) {
                            *slot = Some(name);
                        }
                        cur = code.checked_add(1);
                    }
                }
                _ => {}
            }
        }
    }

    // §9.10.2 method 2's precondition, as a disjunction:
    //   (a) the named-encoding test, or
    //   (b) the WHOLE-ARRAY /Differences test.
    // Plus the resolution the spec RAG recommends for the genuine
    // ambiguity it records: a font with no /Encoding at all satisfies
    // neither disjunct as written (it uses no *named* encoding from the
    // list and has no Differences array to inspect), which would make
    // the standard-14 Helvetica un-extractable by method 2 — certainly
    // not the intent. The recommended reading is "the resolved code→name
    // table draws only from Adobe standard Latin ∪ Symbol", which
    // Annex D.2's STD column satisfies by construction.
    let precondition = if named_qualifies {
        true
    } else if differences_seen {
        differences_all_standard && matches!(named_base, None | Some(b"StandardEncoding"))
            || (named_qualifies && differences_all_standard)
    } else {
        // No Differences: the resolved table is a pure Annex D.2 column.
        !matches!(base, BaseEncoding::ZapfDingbats) || std14.is_some()
    };
    // A Type 3 font's names are arbitrary by definition; it never
    // satisfies method 2's precondition, though its names may still
    // resolve through the AGL (deviation 2).
    let precondition = precondition && !is_type3;

    (table, precondition)
}

// ---------------------------------------------------------------------------
// Widths (geometry, not the ladder)
// ---------------------------------------------------------------------------

/// §9.6.2.1 simple-font widths: `/FirstChar` + `/Widths`, else the
/// standard-14 AFM tables, else `/MissingWidth`.
fn simple_widths(
    doc: &DocumentView<'_>,
    font_dict: &Dict,
    names: &[Option<String>; 256],
    std14: Option<Std14>,
    notes: &mut Vec<FontNote>,
) -> Widths {
    let descriptor = doc
        .resolve(font_dict.get(b"FontDescriptor").unwrap_or(&Object::Null))
        .as_dict();
    let missing_width = descriptor
        .and_then(|d| doc.resolve(d.get(b"MissingWidth")?).as_number())
        .unwrap_or(0.0) as f32;

    let mut table = Box::new([missing_width; 256]);
    let first_char = doc
        .resolve(font_dict.get(b"FirstChar").unwrap_or(&Object::Null))
        .as_int()
        .unwrap_or(0);
    let widths = doc
        .resolve(font_dict.get(b"Widths").unwrap_or(&Object::Null))
        .as_array()
        .map(<[Object]>::to_vec);

    if let Some(widths) = widths {
        for (offset, entry) in widths.iter().enumerate() {
            let Ok(offset) = i64::try_from(offset) else {
                break;
            };
            let Ok(code) = usize::try_from(first_char.saturating_add(offset)) else {
                continue;
            };
            if let Some(slot) = table.get_mut(code)
                && let Some(w) = doc.resolve(entry).as_number()
            {
                *slot = w as f32;
            }
        }
        return Widths::Simple(table);
    }

    // No /Widths. §9.6.2.2 permits this only for the standard 14, whose
    // metrics pdfcer carries.
    if let Some(std14) = std14 {
        for (code, slot) in table.iter_mut().enumerate() {
            if let Some(Some(name)) = names.get(code)
                && let Some(w) = fontdata::std14_width(std14, name)
            {
                *slot = f32::from(w);
            }
        }
        return Widths::Simple(table);
    }

    // Neither: the true advances live in the font program, which
    // `pdfcer-core` cannot read (R21). A table of zeros would collapse
    // every glyph on a line to one x-coordinate and destroy the derived
    // word/line segmentation entirely, so pdfcer estimates from the
    // metrically-similar Helvetica and SAYS SO. The estimate never
    // affects extracted characters — only positions and derived
    // whitespace.
    notes.push(FontNote::WidthsEstimated);
    for (code, slot) in table.iter_mut().enumerate() {
        let estimate = names
            .get(code)
            .and_then(Option::as_ref)
            .and_then(|n| fontdata::std14_width(Std14::Helvetica, n))
            .map_or(500.0, f32::from);
        *slot = estimate;
    }
    Widths::Simple(table)
}

/// Resolve a font's vertical extent from dictionary data alone (§9.8
/// Table 122), in text space, with a four-rung ladder and an honest
/// bottom rung.
///
/// | Rung | Source | Why it is preferred to the next |
/// |---|---|---|
/// | 1 | `/Ascent` + `/Descent` on the descriptor | The two entries §9.8 defines for exactly this question, and **required** on every non-Type-3 descriptor. Accents are excluded by the clause's own wording, which is the small, named residual inaccuracy of any box built from them. |
/// | 2 | `/FontBBox` `ury` / `lly` | Also required (Table 122), and it *includes* accents, so it over-covers rather than under-covers — the safe direction for a hit target. Second because it is the box enclosing **every** glyph placed at a common origin, which for a font with one tall outlier is materially looser than `/Ascent`. |
/// | 3 | The compiled-in standard-14 descriptor | §9.6.2.2 permits a standard-14 font dictionary to carry no descriptor at all; `pdfcer-core::fontdata` holds the real AFM-derived numbers for those 14 faces, so this is still the FONT's metrics, not a guess. |
/// | 4 | [`NOMINAL_ASCENT`]/[`NOMINAL_DESCENT`] | Nothing usable was found. Flagged `nominal: true` so every consumer discloses the guess. |
///
/// Both rungs 1 and 2 require BOTH numbers before they are accepted: half
/// a measurement silently completed from a different source would be a
/// composite no clause describes.
///
/// The `/1000` is §9.8.1's "all integer values shall be units in glyph
/// space", with §9.2.4's glyph→text conversion; the sole exception (a
/// Type 3's `/FontMatrix` glyph space) never reaches here, see
/// [`ExtractFont::resolve_simple`].
fn resolve_vertical(
    doc: &DocumentView<'_>,
    descriptor: Option<&Dict>,
    std14: Option<Std14>,
) -> Vertical {
    const GLYPH_SPACE: f32 = 0.001;

    if let Some(d) = descriptor {
        let num = |key: &[u8]| doc.resolve(d.get(key)?).as_number();
        // Rung 1. `/Descent` "shall be a negative number" — a producer
        // that writes it positive is corrected here rather than trusted,
        // since a positive descent would fold the box's bottom edge above
        // the baseline and make the box shorter than the ink.
        if let (Some(a), Some(dsc)) = (num(b"Ascent"), num(b"Descent"))
            && a > 0.0
            && dsc != 0.0
        {
            return Vertical {
                ascent: a as f32 * GLYPH_SPACE,
                descent: -(dsc.abs() as f32) * GLYPH_SPACE,
                nominal: false,
            };
        }
        // Rung 2: `/FontBBox` is `[llx lly urx ury]` (§7.9.5).
        if let Some(items) = doc
            .resolve(d.get(b"FontBBox").unwrap_or(&Object::Null))
            .as_array()
            && let Some(n) = <[f64; 4]>::try_from(
                items
                    .iter()
                    .filter_map(|o| doc.resolve(o).as_number())
                    .collect::<Vec<f64>>(),
            )
            .ok()
            .filter(|n| n[3] > n[1])
        {
            return Vertical {
                ascent: n[3] as f32 * GLYPH_SPACE,
                descent: n[1].min(0.0) as f32 * GLYPH_SPACE,
                nominal: false,
            };
        }
    }

    // Rung 3.
    if let Some(std14) = std14 {
        let d = fontdata::std14_descriptor(std14);
        return Vertical {
            ascent: f32::from(d.ascender) * GLYPH_SPACE,
            descent: -(f32::from(d.descender).abs()) * GLYPH_SPACE,
            nominal: false,
        };
    }

    // Rung 4.
    Vertical {
        ascent: NOMINAL_ASCENT,
        descent: NOMINAL_DESCENT,
        nominal: true,
    }
}

/// §9.7.4.3 composite-font widths: `/DW` (Table 117 default 1000) and
/// `/W`'s two forms.
///
/// `/W` is an array mixing two shapes:
/// `c [w1 w2 …]` (consecutive CIDs from `c`) and `c_first c_last w` (a
/// range at one width). Both are flattened to `(first, last, width)`
/// triples in file order; the ranges themselves are never expanded.
fn composite_widths(doc: &DocumentView<'_>, descendant: &Dict) -> Widths {
    let default = doc
        .resolve(descendant.get(b"DW").unwrap_or(&Object::Null))
        .as_number()
        .unwrap_or(1000.0) as f32;

    let mut ranges = Vec::new();
    if let Some(items) = doc
        .resolve(descendant.get(b"W").unwrap_or(&Object::Null))
        .as_array()
    {
        let items: Vec<&Object> = items.iter().map(|o| doc.resolve(o)).collect();
        let mut i = 0usize;
        while i < items.len() {
            let Some(first) = items.get(i).and_then(|o| o.as_int()) else {
                break;
            };
            let Ok(first) = u32::try_from(first) else {
                i += 1;
                continue;
            };
            match items.get(i + 1) {
                Some(Object::Array(list)) => {
                    for (offset, w) in list.iter().enumerate() {
                        let Ok(offset) = u32::try_from(offset) else {
                            break;
                        };
                        let Some(cid) = first.checked_add(offset) else {
                            break;
                        };
                        if let Some(w) = doc.resolve(w).as_number() {
                            ranges.push((cid, cid, w as f32));
                        }
                    }
                    i += 2;
                }
                Some(o) if o.as_int().is_some() => {
                    let last = o.as_int().and_then(|v| u32::try_from(v).ok());
                    let w = items.get(i + 2).and_then(|o| o.as_number());
                    if let (Some(last), Some(w)) = (last, w) {
                        ranges.push((first, last, w as f32));
                    }
                    i += 3;
                }
                _ => break,
            }
            // A pathological /W cannot be allowed to grow without
            // bound; 2^20 triples is far past any real CJK font.
            if ranges.len() > 1_048_576 {
                break;
            }
        }
    }
    Widths::Composite { default, ranges }
}

/// §9.6.5: a Type 3 font's glyph space is defined by `/FontMatrix`, so
/// its `/Widths` are in that space rather than the 1000/em space. The
/// `a` element is the horizontal scale; the Table 112 conventional value
/// is `[0.001 0 0 0.001 0 0]`, which reproduces the ordinary scale.
fn type3_width_scale(doc: &DocumentView<'_>, font_dict: &Dict) -> f32 {
    doc.resolve(font_dict.get(b"FontMatrix").unwrap_or(&Object::Null))
        .as_array()
        .and_then(<[Object]>::first)
        .and_then(|o| doc.resolve(o).as_number())
        .map_or(0.001, |v| v as f32)
}

// ---------------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------------

/// Read the code width declared by an embedded CMap stream's
/// `begincodespacerange`.
///
/// Reuses the `ToUnicode` parser, which reads the same
/// `begincodespacerange` syntax — this is the one place where an
/// embedded `Encoding` CMap and a `ToUnicode` CMap genuinely share
/// grammar (§9.7.5 defines the format both subset).
fn embedded_cmap_width(doc: &DocumentView<'_>, font_dict: &Dict) -> Option<CodeWidth> {
    let Object::Stream(stream) = doc.resolve(font_dict.get(b"Encoding")?) else {
        return None;
    };
    // `view.slice(span)` (Pass 17.1) — see `load_to_unicode` above.
    let raw = doc.slice(stream.data_span)?;
    let decoded = filters::decode_stream(&stream.dict, raw).ok()?;
    let cmap = ToUnicodeCMap::parse(&decoded);
    match cmap.codespace_widths().first().copied()? {
        1 => Some(CodeWidth::One),
        _ => Some(CodeWidth::Two),
    }
}

/// A resolved name entry as a `String`.
fn name_of(doc: &DocumentView<'_>, dict: &Dict, key: &[u8]) -> Option<String> {
    let name = doc.resolve(dict.get(key)?).as_name()?;
    Some(String::from_utf8_lossy(name.as_bytes()).into_owned())
}

/// A resolved string entry as a `String`. `/Registry` and `/Ordering`
/// are declared as ASCII strings (Table 116), so a lossy decode is
/// exact in practice and honest when it is not.
fn string_of(doc: &DocumentView<'_>, dict: &Dict, key: &[u8]) -> String {
    match dict.get(key).map(|o| doc.resolve(o)) {
        Some(Object::String(bytes)) => String::from_utf8_lossy(bytes).into_owned(),
        Some(Object::Name(n)) => String::from_utf8_lossy(n.as_bytes()).into_owned(),
        _ => String::new(),
    }
}

/// Strip a §9.6.4 subset tag (`ABCDEF+Helvetica` → `Helvetica`).
///
/// The tag is exactly six uppercase letters followed by `+`. Anything
/// else keeps the name intact — a `+` in a font name is not automatically
/// a subset tag.
fn strip_subset_tag(base_font: &str) -> &str {
    match base_font.split_once('+') {
        Some((tag, rest)) if tag.len() == 6 && tag.bytes().all(|b| b.is_ascii_uppercase()) => rest,
        _ => base_font,
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

    #[test]
    fn subset_tag_stripping_is_exact() {
        assert_eq!(strip_subset_tag("ABCDEF+Helvetica"), "Helvetica");
        assert_eq!(strip_subset_tag("Helvetica"), "Helvetica");
        // Not six letters: not a subset tag.
        assert_eq!(strip_subset_tag("ABC+Foo"), "ABC+Foo");
        assert_eq!(strip_subset_tag("abcdef+Foo"), "abcdef+Foo");
    }

    #[test]
    fn rung_sourcing_classification() {
        assert!(LadderRung::ToUnicode.is_sourced());
        assert!(LadderRung::EncodingAgl.is_sourced());
        assert!(LadderRung::CidCollection.is_sourced());
        // pdfcer's own recovery and the standard's own defeat are NOT
        // sourced results, and the API must not pretend otherwise.
        assert!(!LadderRung::GlyphNameExtension.is_sourced());
        assert!(!LadderRung::Failed.is_sourced());
    }
}
