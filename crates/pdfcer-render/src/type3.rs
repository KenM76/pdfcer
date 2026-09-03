//! # Type 3 fonts — glyphs that are content streams (ISO 32000-1 §9.6.5)
//!
//! The one font subtype in clause 9 that carries **no font program**. A
//! Type 3 font *defines* its glyphs; every other font dictionary merely
//! *describes* a program that lives elsewhere. §9.6.5, verbatim:
//!
//! > "In Type 3 fonts, glyphs shall be defined by streams of PDF graphics
//! > operators. These streams shall be associated with glyph names. A
//! > separate encoding entry shall map character codes to the appropriate
//! > glyph names."
//!
//! Sourced throughout from
//! `D:\Dev\Rag-Specialized\PDF_Spec\iso32000\iso32000__s__9.6.5.md`
//! (§9.6.5, §9.6.6.3, Tables 112 and 113), and from the Acrobat-parity
//! corpus at
//! `D:\Dev\Rag-Specialized\Acrobat_Features\type3fonts__rendering_and_color_semantics.md`.
//! Nothing here is written from recall.
//!
//! ## Why this is cheap, and why that is worth saying out loud
//!
//! A glyph procedure is an **ordinary content stream**, so the existing
//! interpreter runs it unchanged: no font-format parser, no rasteriser,
//! no hinting, no substitution. The consequence is unusual and worth
//! stating in the terms the spec corpus uses — a reader that supports
//! Type 3 renders those documents **pixel-correct**, whereas Type 1 or
//! TrueType *without* an embedded program can only ever substitute a
//! plausible shape. Type 3 is the cheapest exact glyph rendering in the
//! whole of clause 9.
//!
//! The machinery it needs already exists for a different reason.
//! §8.10's form XObject is the same shape — *save the graphics state,
//! run a nested stream, restore* — and [`crate::interpret`] already
//! carries the re-entrancy, the depth counter and the cycle set that
//! shape requires.
//!
//! ## The four things that are easy to get wrong
//!
//! Each of these renders *something* when got wrong, which is why they
//! are called out rather than left to the code to imply.
//!
//! 1. **★ WIDTHS ARE IN `FontMatrix` UNITS, NOT THOUSANDTHS.** Table
//!    112: *"These widths shall be interpreted in glyph space as
//!    specified by `FontMatrix` (**unlike** the widths of a Type 1 font,
//!    which are in thousandths of a unit of text space)."* Every other
//!    simple font in this crate divides its width by 1000. Doing that
//!    here **and** applying a typical `[0.001 0 0 0.001 0 0]` matrix
//!    scales the advance by `1e-6`, and the whole line collapses onto
//!    one point. The spec corpus names this the number-one Type 3 bug.
//!    See [`Type3Font::advance_text_space`], which is the only place a
//!    Type 3 width is ever converted.
//! 2. **`d1` glyphs take their colour from the graphics state, and any
//!    colour operator inside the procedure is IGNORED.** Table 113:
//!    *"A glyph description that begins with the `d1` operator should
//!    not execute any operators that set the colour … any use of such
//!    operators **shall be ignored**."* `d0` is the opposite — it
//!    *declares* that the procedure specifies its own colour.
//!    ★ **Measured against Acrobat Reader on 2026-08-25 rather than
//!    taken on trust**: a four-row probe with the page colour blue and a
//!    red-setting operator inside the procedure rendered `d1` **blue**
//!    and `d0` **red**. Acrobat honours the clause, so following the
//!    clause here is simultaneously Acrobat parity. The probe's
//!    generator is `tools/gen-type3-fixtures.py`.
//! 3. **The encoding is TOTAL, and there is no fallback.** §9.6.6.3:
//!    *"A Type 3 font's mapping from character codes to glyph names
//!    shall be **entirely** defined by its `Encoding` entry, which is
//!    required in this case."* There is no built-in encoding and no
//!    implicit `StandardEncoding` base, because `CharProcs` keys are
//!    arbitrary names with no standard meaning. A code with no
//!    `/Differences` entry has no glyph name, hence no procedure, hence
//!    paints nothing.
//! 4. **A missing glyph still ADVANCES.** §9.6.5 step (b) says *"if the
//!    name is not present as a key in `CharProcs`, no glyph shall be
//!    painted"* — and says nothing about the width, which `Widths`
//!    supplies independently. A reader that skips the whole code
//!    mis-positions every remaining glyph on the line, which looks like
//!    a layout bug rather than a missing-glyph one.
//!
//! ## `Resources`, and the fallback that is easy to miss
//!
//! Table 112's `Resources` entry is *optional*, and when it is absent
//! the names a glyph procedure uses **shall be looked up in the
//! resource dictionary of the PAGE on which the font is used**. Old
//! files routinely omit it. A reader that does not implement the
//! fallback reports "resource not found" on documents that are perfectly
//! well-formed. See [`Type3Font::resources`].
//!
//! ## What this module does NOT do
//!
//! * **No text extraction.** A Type 3 glyph name carries no intrinsic
//!   Unicode meaning, so extraction is gated entirely on `/ToUnicode`
//!   (§9.10) — a separate capability from rendering, deliberately not
//!   bundled in here.
//! * **No editing.** Acrobat has no in-place edit path for Type 3 text
//!   either; the parity corpus records that independently.
//! * **No `/FontBBox` culling.** Table 112 makes `[0 0 0 0]` a
//!   *sentinel* meaning "make no assumptions about glyph sizes", not an
//!   empty box, and a nonzero box that is wrong makes the result
//!   "unpredictable" rather than clipped. See [`Type3Font::font_bbox`].

use pdfcer_core::graph::ObjectGraph;
use pdfcer_core::object::{Dict, Name, Object};
use pdfcer_core::view::DocumentView;

/// Codes in a simple font's encoding (§9.6): one byte.
const CODES: usize = 256;

/// A resolved Type 3 font dictionary (Table 112).
///
/// Everything here is **owned**, not borrowed from the document, because
/// a [`crate::text::LoadedFont`] is `Arc`-shared across `q`/`Q` clones
/// and outlives any single resolve. The glyph procedures themselves are
/// deliberately **not** decoded at load time — see [`Self::char_procs`].
#[derive(Debug, Clone)]
pub struct Type3Font {
    /// `/FontMatrix` — the six numbers mapping **glyph space to text
    /// space** (§9.2.4). Required by Table 112.
    ///
    /// ★ A **general** matrix, not a scale. Rotated and skewed Type 3
    /// fonts exist — they are the mechanism behind rotated stamp and
    /// watermark glyph sets — and Table 112 legislates the rotation case
    /// for widths explicitly. Shortcutting to `font_matrix[0]` as a
    /// scale factor renders every such font wrong while rendering the
    /// common case right, which is the worst combination for noticing.
    pub font_matrix: [f32; 6],
    /// `/FontBBox`, in the glyph coordinate system. Stored for
    /// disclosure only.
    ///
    /// **`[0 0 0 0]` is a sentinel**, not an empty rectangle: Table 112
    /// says a reader "shall make no assumptions about glyph sizes based
    /// on the font bounding box" when all four are zero. Culling against
    /// it would erase every glyph of such a font.
    pub font_bbox: [f32; 4],
    /// `/Widths`, expanded to all 256 codes, **in glyph space**.
    ///
    /// Codes outside `FirstChar..=LastChar` get **0**, which Table 112
    /// states outright — note it is 0 and *not* `/MissingWidth`, which
    /// is the rule for every other simple font.
    pub widths: [f32; CODES],
    /// Code → glyph name, from `/Encoding`'s `/Differences` (§9.6.6.3).
    ///
    /// `None` means the code has no name at all, which for a Type 3 font
    /// means no glyph — there is nothing to fall back to.
    pub glyph_names: Vec<Option<Name>>,
    /// `/CharProcs` — glyph name → procedure stream, kept as the
    /// dictionary rather than as decoded bytes.
    ///
    /// # Why lazily, when every other font flattens at load
    ///
    /// [`crate::text::SimpleFont`] resolves all 256 codes up front
    /// because that work is a table lookup. A glyph procedure is a
    /// *stream*: resolving all 256 means a slice and a filter chain per
    /// entry, for glyphs a page may never show. A font with 256 large
    /// procedures would pay that in full at `Tf` time, on a page that
    /// shows one character.
    pub char_procs: Dict,
    /// `/Resources` (PDF 1.2, Table 112) — the named resources the glyph
    /// procedures use.
    ///
    /// `None` is **not** an error and must not be treated as one: Table
    /// 112 says that when this is absent, "the names shall be looked up
    /// in the resource dictionary of the PAGE on which the font is
    /// used". The caller supplies that fallback; this field only records
    /// whether the font brought its own.
    pub resources: Option<Dict>,
}

impl Type3Font {
    /// Resolve a `/Subtype /Type3` font dictionary.
    ///
    /// Returns `None` only when the dictionary is missing something
    /// Table 112 makes **required** and for which no recovery exists —
    /// `/CharProcs` (there would be no glyphs at all) or `/FontMatrix`
    /// (there would be no mapping from glyph space to text space, and
    /// guessing the common `[0.001 …]` would render a nonstandard font
    /// at a thousand times the wrong size).
    ///
    /// A missing `/Encoding` is **not** fatal here even though Table 112
    /// makes it required: the result is a font every code of which
    /// paints nothing, which is exactly §9.6.5 step (b)'s outcome and is
    /// reported by the caller rather than by refusing the whole font.
    /// Refusing would skip the text *and its advances*, moving the rest
    /// of the line.
    #[must_use]
    pub fn load(doc: &DocumentView<'_>, dict: &Dict) -> Option<Self> {
        let char_procs = dict
            .get(b"CharProcs")
            .map(|o| doc.resolve(o))
            .and_then(Object::as_dict)
            .cloned()?;

        let font_matrix = numbers::<6>(doc, dict, b"FontMatrix")?;
        // Table 112 makes `/FontBBox` required, but a missing one is
        // recoverable in a way `/FontMatrix` is not: the all-zero
        // sentinel means "assume nothing", which is precisely the
        // posture for a box that is not there.
        let font_bbox = numbers::<4>(doc, dict, b"FontBBox").unwrap_or([0.0; 4]);

        Some(Self {
            font_matrix,
            font_bbox,
            widths: load_widths(doc, dict),
            glyph_names: load_encoding(doc, dict),
            char_procs,
            resources: dict
                .get(b"Resources")
                .map(|o| doc.resolve(o))
                .and_then(Object::as_dict)
                .cloned(),
        })
    }

    /// The glyph procedure for `code`, or `None` if there is none.
    ///
    /// Two distinct ways to get `None`, and §9.6.5 gives them the same
    /// outcome — paint nothing, still advance:
    ///
    /// * the code has no `/Differences` entry, so it has no glyph name
    ///   (§9.6.6.3 leaves nothing to fall back on); or
    /// * the name is not a key in `/CharProcs`, which step (b) names
    ///   explicitly.
    ///
    /// The caller cannot tell them apart from the return value and does
    /// not need to; both are counted together as a glyph that was asked
    /// for and does not exist.
    #[must_use]
    pub fn proc_for(&self, code: u32) -> Option<&Object> {
        let name = self
            .glyph_names
            .get(usize::try_from(code).ok()?)?
            .as_ref()?;
        self.char_procs.get(name.as_bytes())
    }

    /// The advance for `code`, **in text space**.
    ///
    /// ★ THIS FUNCTION IS THE WHOLE OF THE WIDTH RULE, and it exists as
    /// a function so that no call site can apply the simple-font `/1000`
    /// by habit. Table 112:
    ///
    /// > "These widths shall be interpreted in glyph space as specified
    /// > by `FontMatrix` (unlike the widths of a Type 1 font, which are
    /// > in thousandths of a unit of text space). **If `FontMatrix`
    /// > specifies a rotation, only the horizontal component of the
    /// > transformed width shall be used**; that is, the resulting
    /// > displacement shall be horizontal in text space, as is the case
    /// > for all simple fonts."
    ///
    /// So the width is a **vector** `(w, 0)` transformed by the matrix,
    /// of which only the `x` component survives — and the translation
    /// components `e`/`f` are deliberately **not** applied, because a
    /// displacement is a direction and not a position.
    #[must_use]
    pub fn advance_text_space(&self, code: u32) -> f32 {
        let w = usize::try_from(code)
            .ok()
            .and_then(|c| self.widths.get(c))
            .copied()
            .unwrap_or(0.0);
        // [a b c d e f]: x' = a·x + c·y + e. With y = 0 and no
        // translation, the horizontal component is a·w.
        self.font_matrix[0] * w
    }

    /// Whether `/FontBBox` is the all-zero "assume nothing" sentinel.
    ///
    /// Exposed so a caller that ever wants to cull has to ask, rather
    /// than discovering the sentinel by culling every glyph of a font
    /// that used it.
    #[must_use]
    pub fn bbox_is_sentinel(&self) -> bool {
        self.font_bbox.iter().all(|v| *v == 0.0)
    }
}

/// What the first operator of a glyph procedure declared (Table 113).
///
/// The distinction is **not** bookkeeping: it decides where the glyph's
/// colour comes from, and getting it backwards paints every glyph of a
/// colour-setting font in the wrong colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GlyphColorSource {
    /// `d1 wx wy llx lly urx ury` — "the glyph description specifies
    /// **only shape, not colour**". Colour operators inside the
    /// procedure are ignored and the glyph is painted in the colour that
    /// was current at the text-showing operator.
    ///
    /// **The default**, and deliberately so: a procedure with no `d0` or
    /// `d1` at all is non-conformant ("shall include as its first
    /// operator either `d0` or `d1`"), and §9.6.5 does not say what to
    /// do with one. Defaulting to shape-only means such a glyph inherits
    /// the text colour, which is what a reader expects of text; the
    /// alternative default would paint it in whatever colour the
    /// procedure happened to leave in the graphics state.
    #[default]
    ShapeOnly,
    /// `d0 wx wy` — "the glyph description specifies **both its shape
    /// and its colour**". Colour operators inside the procedure are
    /// honoured.
    ShapeAndColor,
}

/// `/Widths` expanded to all 256 codes, in glyph space.
///
/// Table 112: "(`LastChar` − `FirstChar` + 1) widths… For character
/// codes outside the range `FirstChar` to `LastChar`, **the width shall
/// be 0**." Note that 0 — not `/MissingWidth`, which is Table 111's rule
/// for the other simple fonts and does not apply here.
fn load_widths(doc: &DocumentView<'_>, dict: &Dict) -> [f32; CODES] {
    let mut widths = [0.0f32; CODES];
    let first = dict
        .get(b"FirstChar")
        .and_then(|o| doc.resolve(o).as_number())
        .unwrap_or(0.0);
    let Some(list) = dict
        .get(b"Widths")
        .map(|o| doc.resolve(o))
        .and_then(Object::as_array)
    else {
        return widths;
    };
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let first = if (0.0..=255.0).contains(&first) {
        first as usize
    } else {
        return widths;
    };
    for (i, item) in list.iter().enumerate() {
        let Some(slot) = first.checked_add(i).and_then(|c| widths.get_mut(c)) else {
            // Past code 255. `/LastChar` is not consulted: the array's
            // own length is the authority, and a `/LastChar` that
            // disagrees with it is a malformed file whose widths are
            // still readable.
            break;
        };
        if let Some(w) = doc.resolve(item).as_number() {
            #[allow(clippy::cast_possible_truncation)]
            {
                *slot = w as f32;
            }
        }
    }
    widths
}

/// Code → glyph name, from `/Encoding` (§9.6.6.3).
///
/// **Differences-only, with no base encoding**, and that is the whole
/// point of this function existing separately from
/// [`crate::text`]'s general encoding ladder. §9.6.6.3 makes a Type 3
/// font's mapping "entirely defined by its `Encoding` entry"; there is
/// no built-in encoding in a font that has no program, and
/// `StandardEncoding`'s names are meaningless against arbitrary
/// `CharProcs` keys.
///
/// ★ Table 112 types `/Encoding` as "**name** or dictionary" while its
/// own description and §9.6.6.3 both require a dictionary with a
/// `Differences` array. The spec corpus flags that as a **genuine
/// inconsistency in the standard**, not a reading error. A bare
/// predefined-encoding name is treated here as an empty encoding: every
/// code then has no glyph name, paints nothing and still advances, which
/// is the same outcome §9.6.5 step (b) prescribes and is the least
/// inventive of the available answers.
fn load_encoding(doc: &DocumentView<'_>, dict: &Dict) -> Vec<Option<Name>> {
    let mut table: Vec<Option<Name>> = vec![None; CODES];
    let Some(enc) = dict.get(b"Encoding").map(|o| doc.resolve(o)) else {
        return table;
    };
    let Some(enc) = enc.as_dict() else {
        return table;
    };
    let Some(diffs) = enc
        .get(b"Differences")
        .map(|o| doc.resolve(o))
        .and_then(Object::as_array)
    else {
        return table;
    };

    // §9.6.6.1's parse, verbatim in shape: an integer sets the current
    // code; each following name assigns at that code and increments.
    let mut cur: Option<usize> = None;
    for item in diffs {
        match doc.resolve(item) {
            Object::Integer(v) => cur = usize::try_from(*v).ok(),
            #[allow(clippy::cast_possible_truncation)]
            Object::Real(v) => cur = usize::try_from(*v as i64).ok(),
            Object::Name(n) => {
                let Some(code) = cur else {
                    // A leading name with no integer before it is
                    // malformed. Skipped rather than assumed to start at
                    // 0 — guessing a start point silently shifts every
                    // glyph in the array.
                    continue;
                };
                if let Some(slot) = table.get_mut(code) {
                    *slot = Some(n.clone());
                }
                cur = code.checked_add(1);
            }
            _ => {}
        }
    }
    table
}

/// Read a fixed-length numeric array entry.
fn numbers<const N: usize>(doc: &DocumentView<'_>, dict: &Dict, key: &[u8]) -> Option<[f32; N]> {
    let list = doc.resolve(dict.get(key)?).as_array()?;
    if list.len() < N {
        return None;
    }
    let mut out = [0.0f32; N];
    for (slot, item) in out.iter_mut().zip(list) {
        #[allow(clippy::cast_possible_truncation)]
        {
            *slot = doc.resolve(item).as_number()? as f32;
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ★ The number-one Type 3 bug, pinned.
    ///
    /// A width of 750 in a font with the conventional
    /// `[0.001 0 0 0.001 0 0]` matrix is **0.75 text-space units**. The
    /// simple-font habit — divide by 1000 *and* apply the matrix — gives
    /// `0.00075`, a factor of 1000 out, and collapses a line of text
    /// onto a point.
    #[test]
    fn a_width_goes_through_the_font_matrix_not_through_a_thousand() {
        let mut f = font_with_matrix([0.001, 0.0, 0.0, 0.001, 0.0, 0.0]);
        f.widths[65] = 750.0;
        let got = f.advance_text_space(65);
        assert!(
            (got - 0.75).abs() < 1e-6,
            "750 glyph units under a 0.001 matrix is 0.75 text units, got {got}"
        );
    }

    /// A non-conventional matrix is the case that separates "apply the
    /// matrix" from "divide by 1000", which agree at 0.001 and only
    /// there. A 1-unit em is a real shape: it is what a font whose
    /// glyphs are authored in text space uses.
    #[test]
    fn a_unit_font_matrix_leaves_the_width_alone() {
        let mut f = font_with_matrix([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
        f.widths[65] = 0.5;
        assert!((f.advance_text_space(65) - 0.5).abs() < 1e-6);
    }

    /// Table 112's rotation rule: only the horizontal component of the
    /// transformed width survives, and the translation is not applied.
    ///
    /// Under a 90-degree rotation `[0 1 -1 0 0 0]` the `a` term is 0, so
    /// a rotated Type 3 font advances by **nothing** horizontally — which
    /// is what the clause requires, however odd it looks. The
    /// translation terms are set non-zero here precisely to prove they
    /// are ignored: a displacement is a direction, not a position.
    #[test]
    fn a_rotation_keeps_only_the_horizontal_component_and_ignores_translation() {
        let mut f = font_with_matrix([0.0, 0.001, -0.001, 0.0, 99.0, 99.0]);
        f.widths[65] = 750.0;
        assert!((f.advance_text_space(65) - 0.0).abs() < 1e-6);
    }

    /// Codes outside `/Widths` are width **0** (Table 112), not
    /// `/MissingWidth` — the opposite of every other simple font.
    #[test]
    fn a_code_outside_the_widths_array_has_width_zero() {
        let f = font_with_matrix([0.001, 0.0, 0.0, 0.001, 0.0, 0.0]);
        assert!((f.advance_text_space(200) - 0.0).abs() < 1e-9);
        // And a code past the one-byte range does not panic.
        assert!((f.advance_text_space(99_999) - 0.0).abs() < 1e-9);
    }

    /// `[0 0 0 0]` is a sentinel meaning "assume nothing", not an empty
    /// box. Culling against it would erase every glyph.
    #[test]
    fn an_all_zero_font_bbox_is_recognised_as_the_sentinel() {
        let f = font_with_matrix([0.001, 0.0, 0.0, 0.001, 0.0, 0.0]);
        assert!(
            f.bbox_is_sentinel(),
            "an unset box must read as the sentinel"
        );
        let mut g = f.clone();
        g.font_bbox = [0.0, 0.0, 700.0, 700.0];
        assert!(!g.bbox_is_sentinel());
    }

    /// A code with no `/Differences` entry has no glyph, because
    /// §9.6.6.3 leaves a Type 3 font nothing to fall back on.
    #[test]
    fn a_code_with_no_encoding_entry_has_no_glyph_procedure() {
        let f = font_with_matrix([0.001, 0.0, 0.0, 0.001, 0.0, 0.0]);
        assert!(f.proc_for(65).is_none());
    }

    /// The default colour source is shape-only, so a malformed
    /// procedure with neither `d0` nor `d1` inherits the text colour
    /// rather than whatever the procedure last set.
    #[test]
    fn the_default_colour_source_is_shape_only() {
        assert_eq!(GlyphColorSource::default(), GlyphColorSource::ShapeOnly);
    }

    fn font_with_matrix(m: [f32; 6]) -> Type3Font {
        Type3Font {
            font_matrix: m,
            font_bbox: [0.0; 4],
            widths: [0.0; CODES],
            glyph_names: vec![None; CODES],
            char_procs: Dict::new(),
            resources: None,
        }
    }
}
