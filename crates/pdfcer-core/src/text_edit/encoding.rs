//! # Inverse encoding — Unicode -> code, the Pass 14.1 correctness core
//!
//! This module is the **write-side inverse** of the §9.6.6 encoding
//! resolution Pass 4 runs forward (`text_extract/font.rs`). To REPLACE the
//! operand of a `Tj`/`TJ` show operator with new text
//! (`crate::text_edit::edit`), pdfcer must convert each target Unicode
//! character back into the **single-byte code** the run's own font uses to
//! show it. ISO 32000-1 specifies the forward map (code -> glyph name ->
//! glyph / Unicode, §9.6.6 / §9.10.2) and **no inverse**, and imposes no
//! obligation that the inverse be well-defined. Everything here is
//! engineering procedure grounded in those forward clauses and in the
//! derived spec-RAG consolidator `iso32000__ref__inverse_encoding.md`.
//!
//! ## §0 — the one load-bearing correctness point
//!
//! Re-encode by inverting the font's **own resolved `/Encoding`**
//! (base encoding + `/Differences`, via the AGL) — **NEVER** by inverting
//! `/ToUnicode`. `/ToUnicode` is one-way and **lossy** and cannot be
//! inverted to recover a code (inverse-encoding RAG §0):
//!
//! 1. **Not injective** — two codes may map to the *same* Unicode value
//!    (glyph variants, a subset reusing a scalar). Inverting picks one
//!    arbitrarily -> the wrong glyph.
//! 2. **Not single-valued forward either** — one code may map to a *string*
//!    of scalars (a ligature code -> `"ffi"`), so a target `"i"` has no code
//!    that produces *only* `"i"`.
//! 3. **May be partial** — `/ToUnicode` presence is not coverage; absence in
//!    the reverse map is indistinguishable from "not in this font".
//! 4. **Carries no rendering authority** — it is a semantic annotation for
//!    *extraction* (§9.10.1). The authoritative code<->glyph relation for a
//!    simple font is the font's `/Encoding` (§9.6.6).
//!
//! So the inverse is driven by inverting the resolved `/Encoding` table `E`
//! that [`crate::text_extract::font::ExtractFont`] already built forward.
//! A font that relates codes to characters **only** through `/ToUnicode`
//! (no usable `/Encoding`) has no well-defined inverse => REFUSE — that
//! classification (R-INV-2 / R-INV-3 / R-INV-4) is made by the caller
//! ([`crate::text_edit::edit`]), which can see the font's `/Subtype` and
//! descriptor; this module handles the per-character triggers on a font
//! that DID resolve a simple `/Encoding`: R-INV-1, 5, 6, 7, 8.
//!
//! ## The gate is fuzzy-never-sneaky (project rule 4, R71)
//!
//! A missing or ambiguous glyph returns a **named [`Refusal`]** the UI/CLI
//! surfaces verbatim. It must NEVER reach the writer, NEVER be faked, NEVER
//! be silently substituted. Ambiguity (R-INV-5) and a present-but-ligature
//! glyph (R-INV-6) are *disclosed* choices/notes, not silent behaviour.

use std::collections::{BTreeMap, BTreeSet};

use crate::fontdata::{self, BaseEncoding};
use crate::text_extract::cmap::{NotInjective, ToUnicodeCMap};

/// The exhaustive refuse/disclose triggers of the inverse-encoding gate
/// (inverse-encoding RAG §2). Hard refusals are 1, 2, 3, 4, 7, 8; 5 and 6
/// are soft (disclosed choice / note), surfaced through
/// [`CharEncoding::Chosen`] and a [`Refusal`] respectively.
///
/// R-INV-2/3/4 are raised by the caller's font classification
/// ([`crate::text_edit::edit`]); this enum is the shared vocabulary so a
/// diagnostic anywhere names the same trigger.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RInvTrigger {
    /// R-INV-1 — target `U` absent from the font's resolved encoding.
    TargetAbsent,
    /// R-INV-2 — symbolic font with a built-in/custom cmap and no usable
    /// `/Encoding` (§9.6.6.4 Branch B ignores `/Encoding`).
    SymbolicNoEncoding,
    /// R-INV-3 — `/ToUnicode` is the only code<->char relation; one-way and
    /// lossy (§0), so not invertible.
    ToUnicodeOnly,
    /// R-INV-4 — composite (Type 0 / CIDFont) run; deferred to FF-C.
    Composite,
    /// R-INV-5 — ambiguous inverse: multiple codes map to `U` (soft).
    Ambiguous,
    /// R-INV-6 — `U`'s glyph is present only inside a multi-scalar ligature
    /// name; single-char substitution not attempted in the first cut (soft).
    LigatureOnly,
    /// R-INV-7 — `U`'s glyph name has no code, and the code it would use is
    /// already assigned to a different glyph by `/Differences`; in-place
    /// REPLACE does not mutate `/Encoding` (14.1 invariant).
    CodeOccupied,
    /// R-INV-8 — target beyond the simple-font repertoire (outside the BMP,
    /// a surrogate, or a combining sequence) in a single-byte simple font.
    BeyondRepertoire,
}

impl RInvTrigger {
    /// The stable trigger id, e.g. `"R-INV-1"`, for a machine-readable
    /// diagnostic and for the CLI/tests to key on.
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::TargetAbsent => "R-INV-1",
            Self::SymbolicNoEncoding => "R-INV-2",
            Self::ToUnicodeOnly => "R-INV-3",
            Self::Composite => "R-INV-4",
            Self::Ambiguous => "R-INV-5",
            Self::LigatureOnly => "R-INV-6",
            Self::CodeOccupied => "R-INV-7",
            Self::BeyondRepertoire => "R-INV-8",
        }
    }

    /// Whether this trigger is a **hard** refusal (1, 2, 3, 4, 7, 8) rather
    /// than a soft/disclosed outcome (5, 6). A hard trigger stops the edit;
    /// a soft one records a disclosure and proceeds.
    #[must_use]
    pub const fn is_hard(self) -> bool {
        matches!(
            self,
            Self::TargetAbsent
                | Self::SymbolicNoEncoding
                | Self::ToUnicodeOnly
                | Self::Composite
                | Self::CodeOccupied
                | Self::BeyondRepertoire
        )
    }
}

/// A named refusal the operator sees verbatim.
///
/// It never reaches the writer (rule 4 / R71): the surgery
/// ([`crate::text_edit::edit`]) turns a `Refusal` into a clean, named
/// error, never a faked byte. Carries the trigger, the offending character
/// (when a single character caused it), the font it was refused against,
/// and a full operator-facing message.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Refusal {
    /// Which inverse-encoding trigger fired.
    pub trigger: RInvTrigger,
    /// The character that could not be encoded, if a single one caused it.
    pub character: Option<char>,
    /// The `/BaseFont` the edit was refused against.
    pub base_font: String,
    /// The full, operator-facing message (surfaced verbatim by UI/CLI).
    pub message: String,
}

impl Refusal {
    /// Build a refusal for one character against one font.
    fn char_refusal(trigger: RInvTrigger, u: char, base_font: &str, why: &str) -> Self {
        Self {
            trigger,
            character: Some(u),
            base_font: base_font.to_owned(),
            message: format!(
                "{}: character U+{:04X} '{}' {} in font '{}'",
                trigger.id(),
                u as u32,
                u,
                why,
                base_font
            ),
        }
    }
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Refusal {}

/// The outcome of resolving one target character against an
/// [`InverseEncoding`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum CharEncoding {
    /// A single, unambiguous code.
    Code(u8),
    /// An ambiguous inverse (R-INV-5): the chosen code plus the disclosure
    /// to surface. The choice is fuzzy-never-sneaky, not an error.
    Chosen {
        /// The code chosen (reused-in-run if possible, else lowest).
        code: u8,
        /// The R-INV-5 disclosure, surfaced verbatim.
        disclosure: String,
    },
    /// A hard refusal — the edit of this run stops.
    Refuse(Refusal),
}

/// The result of encoding a whole target string.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct EncodeResult {
    /// The new code bytes, one per code (simple font).
    pub codes: Vec<u8>,
    /// Soft disclosures accumulated (R-INV-5 choices), verbatim.
    pub disclosures: Vec<String>,
}

/// The inverse encoding map for one **composite** (`/Type0`) font.
///
/// The multi-byte sibling of [`InverseEncoding`]. Where that one inverts a
/// simple font's code→glyph-name table, this inverts a `/ToUnicode` CMap —
/// and only where doing so is sound, which is standing rule R110's whole
/// subject.
///
/// # Why this is a separate type rather than a mode on the simple one
///
/// They answer the same question and share almost nothing to answer it with.
/// The simple encoder reasons about glyph NAMES, `/Differences`, ligature
/// components and code occupancy — none of which exist for a CIDFont, whose
/// codes are opaque indices with no names attached. Folding the two together
/// would mean a type where half the fields are meaningless depending on a
/// flag, and every method would have to say which half it was in.
///
/// # What it cannot do, and will not pretend to
///
/// * **Add a character the font lacks.** A composite subset carries the CIDs
///   it carries; there is no code for a glyph that is not there. That is the
///   composite form of R-INV-1 and it refuses by name.
/// * **Work at all without an injective `/ToUnicode`.** Construction goes
///   through [`ToUnicodeCMap::injective_inverse`], so a font whose map is a
///   ligature table or has colliding entries never produces one of these.
///   The refusal happens where the evidence is, not here.
#[derive(Debug, Clone)]
pub struct CompositeEncoding {
    base_font: String,
    /// Unicode scalar → the CID that shows it. One CID, because the map it
    /// was built from was verified injective.
    reverse: BTreeMap<char, u32>,
}

/// The result of encoding text into a composite font's codes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompositeEncodeResult {
    /// The CIDs, in text order.
    pub cids: Vec<u16>,
}

impl CompositeEncodeResult {
    /// The operand bytes for a `Tj`/`TJ` string, big-endian per §9.7.6.2.
    ///
    /// `Identity-H` codes are two bytes, most significant first. Producing
    /// the bytes here rather than at the splice keeps the byte order in one
    /// place — the failure mode for getting it wrong is not a crash but a
    /// page of plausible, entirely different glyphs.
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.cids.len() * 2);
        for cid in &self.cids {
            out.extend_from_slice(&cid.to_be_bytes());
        }
        out
    }
}

impl CompositeEncoding {
    /// Build from a font's `/ToUnicode`, or refuse with the reason.
    ///
    /// # Errors
    ///
    /// Returns the [`NotInjective`] obstruction verbatim — a ligature
    /// destination, a collision naming both codes, an empty map, or a map too
    /// large to materialise. The caller surfaces it; this type does not
    /// paraphrase it into something vaguer.
    pub fn build(base_font: &str, cmap: &ToUnicodeCMap) -> Result<Self, NotInjective> {
        Ok(Self {
            base_font: base_font.to_owned(),
            reverse: cmap.injective_inverse()?,
        })
    }

    /// Encode `target` into this font's CIDs.
    ///
    /// # Errors
    ///
    /// Refuses by name on the first character the font has no CID for. The
    /// composite form of R-INV-1: a subset carries what it carries, and there
    /// is no code that means a glyph the program does not contain.
    pub fn encode_str(&self, target: &str) -> Result<CompositeEncodeResult, Refusal> {
        let mut cids = Vec::with_capacity(target.chars().count());
        for ch in target.chars() {
            let Some(&code) = self.reverse.get(&ch) else {
                return Err(Refusal {
                    trigger: RInvTrigger::TargetAbsent,
                    character: Some(ch),
                    base_font: self.base_font.clone(),
                    message: format!(
                        "this font has no glyph for {ch:?}, and pdfcer cannot add one to a font \
                         that is already embedded. Keep this edit to characters the font \
                         already uses, or choose a font that covers it."
                    ),
                });
            };
            // A CID wider than 16 bits cannot be written as an `Identity-H`
            // code. Refused rather than truncated: a truncated CID is a
            // DIFFERENT, VALID glyph, so the page would render confidently
            // wrong text with nothing to indicate it.
            let Ok(cid) = u16::try_from(code) else {
                return Err(Refusal {
                    trigger: RInvTrigger::TargetAbsent,
                    character: Some(ch),
                    base_font: self.base_font.clone(),
                    message: format!(
                        "this font maps {ch:?} to a glyph index pdfcer cannot write in this \
                         encoding."
                    ),
                });
            };
            cids.push(cid);
        }
        Ok(CompositeEncodeResult { cids })
    }

    /// Whether this font can show `ch` at all.
    #[must_use]
    pub fn covers(&self, ch: char) -> bool {
        self.reverse.contains_key(&ch)
    }
}

/// The inverse encoding map for one **simple** font.
///
/// Built by [`Self::build`] from the font's resolved code->glyph-name table
/// `E` (the same table Pass 4 built forward). Inversion is per-FONT, exactly
/// like `/ToUnicode` (§9.10 Gotchas): two fonts in one edited run invert
/// independently.
#[derive(Debug, Clone)]
pub struct InverseEncoding {
    base_font: String,
    /// Unicode scalar -> the codes (ascending) that show it in `E`.
    reverse: BTreeMap<char, Vec<u8>>,
    /// Characters that appear ONLY inside a multi-scalar (ligature) glyph
    /// name — present in the font, but not addressable as a single char
    /// (R-INV-6).
    ligature_chars: BTreeSet<char>,
    /// Codes assigned a non-`.notdef` glyph in `E` — the occupancy test for
    /// R-INV-7.
    occupied: BTreeSet<u8>,
}

impl InverseEncoding {
    /// Build the inverse map by inverting the font's **own** forward chain.
    ///
    /// For each code in `E`, `AGL_forward(name)` (the existing
    /// [`fontdata::glyph_name_to_unicode_string`], name -> Unicode) gives the
    /// scalar(s). A single-scalar name populates `reverse[U]`; a
    /// multi-scalar (ligature) name records its scalars in `ligature_chars`
    /// and is skipped for the single-char reverse map (inverse-encoding RAG
    /// §1). A target code need not be *currently used* on the page — being
    /// present in `E` is sufficient; in-place REPLACE may emit any code
    /// already in `E`, and never adds codes.
    #[must_use]
    pub fn build(base_font: &str, glyph_names: &[Option<String>; 256]) -> Self {
        let mut reverse: BTreeMap<char, Vec<u8>> = BTreeMap::new();
        let mut ligature_chars: BTreeSet<char> = BTreeSet::new();
        let mut occupied: BTreeSet<u8> = BTreeSet::new();

        for (code, slot) in glyph_names.iter().enumerate() {
            let Some(name) = slot else { continue };
            if name == ".notdef" {
                continue;
            }
            let Ok(code) = u8::try_from(code) else {
                continue;
            };
            occupied.insert(code);
            let Some(text) = fontdata::glyph_name_to_unicode_string(name) else {
                continue;
            };
            let mut chars = text.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) => reverse.entry(c).or_default().push(code),
                (Some(_), Some(_)) => {
                    for c in text.chars() {
                        ligature_chars.insert(c);
                    }
                }
                _ => {}
            }
        }
        for codes in reverse.values_mut() {
            codes.sort_unstable();
            codes.dedup();
        }
        Self {
            base_font: base_font.to_owned(),
            reverse,
            ligature_chars,
            occupied,
        }
    }

    /// The `/BaseFont` this map inverts.
    #[must_use]
    pub fn base_font(&self) -> &str {
        &self.base_font
    }

    /// Whether a code shows character `U` in this font's encoding — the
    /// caller's "already carries" hint uses this together with its own
    /// subset-usage set.
    #[must_use]
    pub fn has_char(&self, u: char) -> bool {
        self.reverse.contains_key(&u)
    }

    /// Resolve one target character to a code (or a named refusal).
    ///
    /// `prefer` is the set of codes already used elsewhere in the same run /
    /// line (the R-INV-5 tie-break: prefer a reused code, else the lowest).
    /// The per-character triggers implemented here are R-INV-1, 5, 6, 7, 8;
    /// the font-classification triggers R-INV-2/3/4 are the caller's.
    #[must_use]
    pub fn encode_char(&self, u: char, prefer: &BTreeSet<u8>) -> CharEncoding {
        // R-INV-8: a simple font addresses <=256 single-byte codes, so a
        // scalar outside the BMP has no single-code representation. (A Rust
        // `char` can never be a lone surrogate, so the surrogate half of the
        // trigger cannot arise here; the astral half is what we test.)
        if (u as u32) > 0xFFFF {
            return CharEncoding::Refuse(Refusal::char_refusal(
                RInvTrigger::BeyondRepertoire,
                u,
                &self.base_font,
                "is outside the Basic Multilingual Plane and has no single-byte code",
            ));
        }

        match self.reverse.get(&u) {
            Some(codes) => match codes.as_slice() {
                [only] => CharEncoding::Code(*only),
                _ => {
                    // R-INV-5: many-to-one at `U`. Prefer a code already
                    // used in the run/line, else the lowest code.
                    let chosen = codes
                        .iter()
                        .copied()
                        .find(|c| prefer.contains(c))
                        .or_else(|| codes.iter().copied().min());
                    match chosen {
                        Some(code) => CharEncoding::Chosen {
                            code,
                            disclosure: format!(
                                "R-INV-5: character U+{:04X} '{}' is shown by {} codes in font \
                                 '{}'; chose code {} ({}) — the inverse is ambiguous, review",
                                u as u32,
                                u,
                                codes.len(),
                                self.base_font,
                                code,
                                if prefer.contains(&code) {
                                    "reused in this run"
                                } else {
                                    "lowest code"
                                },
                            ),
                        },
                        None => CharEncoding::Refuse(Refusal::char_refusal(
                            RInvTrigger::TargetAbsent,
                            u,
                            &self.base_font,
                            "resolved to no code",
                        )),
                    }
                }
            },
            None => {
                // Present only as a ligature component? (R-INV-6.)
                if self.ligature_chars.contains(&u) {
                    return CharEncoding::Refuse(Refusal::char_refusal(
                        RInvTrigger::LigatureOnly,
                        u,
                        &self.base_font,
                        "is present only inside a ligature glyph; single-character substitution \
                         is not performed in this first cut",
                    ));
                }
                // R-INV-7 vs R-INV-1: is the code this char would naturally
                // use already occupied by a DIFFERENT glyph (a `/Differences`
                // override)?
                if let Some(canon) = canonical_code(u)
                    && self.occupied.contains(&canon)
                {
                    return CharEncoding::Refuse(Refusal {
                        trigger: RInvTrigger::CodeOccupied,
                        character: Some(u),
                        base_font: self.base_font.clone(),
                        message: format!(
                            "R-INV-7: character U+{:04X} '{}' has no code in font '{}'s encoding, \
                             and code {} (its standard slot) is already assigned to a different \
                             glyph by /Differences; in-place edit does not mutate /Encoding \
                             (unblock path: add a /Differences entry in a later pass)",
                            u as u32, u, self.base_font, canon
                        ),
                    });
                }
                CharEncoding::Refuse(Refusal::char_refusal(
                    RInvTrigger::TargetAbsent,
                    u,
                    &self.base_font,
                    "is not in the font's resolved encoding",
                ))
            }
        }
    }

    /// Encode a whole target string, or return the first hard [`Refusal`].
    ///
    /// A single un-invertible character REFUSES the whole edit of that run
    /// (inverse-encoding RAG §2 gate order) — the surgery never emits a
    /// partial run. Soft R-INV-5 choices are accumulated into
    /// [`EncodeResult::disclosures`]. `prefer` seeds the R-INV-5 tie-break
    /// and grows as codes are chosen, so a character shown twice reuses one
    /// code.
    ///
    /// # Errors
    ///
    /// The first hard [`Refusal`] encountered (R-INV-1/6/7/8 here; the
    /// caller adds R-INV-2/3/4 and the embedded-subset floor).
    pub fn encode_str(&self, target: &str, prefer: &BTreeSet<u8>) -> Result<EncodeResult, Refusal> {
        let mut codes = Vec::new();
        let mut disclosures = Vec::new();
        let mut prefer = prefer.clone();
        for u in target.chars() {
            match self.encode_char(u, &prefer) {
                CharEncoding::Code(c) => {
                    codes.push(c);
                    prefer.insert(c);
                }
                CharEncoding::Chosen { code, disclosure } => {
                    codes.push(code);
                    prefer.insert(code);
                    disclosures.push(disclosure);
                }
                CharEncoding::Refuse(r) => return Err(r),
            }
        }
        Ok(EncodeResult { codes, disclosures })
    }
}

/// The code a character would *naturally* occupy in a standard simple-font
/// encoding — used only for the R-INV-1 vs R-INV-7 distinction.
///
/// Searches `WinAnsiEncoding` then `StandardEncoding` for a code whose glyph
/// name maps (single-scalar) to `u`. `None` when the character has no slot
/// in either standard encoding (then the absence is a plain R-INV-1).
fn canonical_code(u: char) -> Option<u8> {
    for base in [BaseEncoding::WinAnsi, BaseEncoding::Standard] {
        for code in 0u16..=255 {
            let Ok(code) = u8::try_from(code) else {
                continue;
            };
            if let Some(name) = fontdata::encoding_glyph_name(base, code)
                && fontdata::glyph_name_to_unicode(name) == Some(u)
            {
                return Some(code);
            }
        }
    }
    None
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

    /// Build a composite encoder from `beginbfchar` pairs.
    fn composite(pairs: &[(u16, &str)]) -> Result<CompositeEncoding, NotInjective> {
        let mut body =
            String::from("begincmap\n1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n");
        body.push_str(&format!("{} beginbfchar\n", pairs.len()));
        for (code, dst) in pairs {
            let hex: String = dst.encode_utf16().map(|u| format!("{u:04X}")).collect();
            body.push_str(&format!("<{code:04X}> <{hex}>\n"));
        }
        body.push_str("endbfchar\nendcmap\n");
        CompositeEncoding::build("TEST+Composite", &ToUnicodeCMap::parse(body.as_bytes()))
    }

    #[test]
    fn encodes_text_into_the_fonts_own_cids() {
        let enc = composite(&[(1, "A"), (2, "B"), (3, "C")]).expect("injective");
        let out = enc.encode_str("CAB").expect("all three are covered");
        assert_eq!(out.cids, vec![3, 1, 2]);
    }

    /// Byte order is the whole ballgame for a composite operand: writing
    /// the CID little-endian would still produce a valid two-byte code,
    /// pointing at a DIFFERENT glyph. Nothing would error; the page would
    /// simply say something else.
    #[test]
    fn operand_bytes_are_big_endian_two_per_cid() {
        let enc = composite(&[(0x0102, "A")]).expect("injective");
        let out = enc.encode_str("A").expect("covered");
        assert_eq!(out.cids, vec![0x0102]);
        assert_eq!(
            out.to_bytes(),
            vec![0x01, 0x02],
            "Identity-H codes are big-endian (§9.7.6.2); reversed, this addresses glyph 0x0201"
        );
    }

    /// The composite form of R-INV-1: a subset carries what it carries,
    /// and no code means a glyph the program does not contain.
    #[test]
    fn a_character_the_font_lacks_is_refused_by_name() {
        let enc = composite(&[(1, "A")]).expect("injective");
        let err = enc.encode_str("AZ").expect_err("Z is not in this font");
        assert_eq!(err.character, Some('Z'));
        assert_eq!(err.trigger, RInvTrigger::TargetAbsent);
        assert!(
            err.message.contains("no glyph"),
            "the refusal must say what is missing: {}",
            err.message
        );
    }

    /// A non-injective CMap must never yield an encoder at all — the
    /// refusal belongs where the evidence is (R110), not at encode time
    /// when the caller has already committed to an edit.
    #[test]
    fn a_non_injective_cmap_yields_no_encoder() {
        let err = composite(&[(1, "A"), (7, "A")]).expect_err("two codes, one char");
        assert!(matches!(err, NotInjective::Collision { .. }), "{err:?}");
    }

    /// Empty input is not an error — replacing text with nothing is a
    /// deletion, and the encoder has no opinion about that.
    #[test]
    fn empty_text_encodes_to_no_cids() {
        let enc = composite(&[(1, "A")]).expect("injective");
        let out = enc.encode_str("").expect("empty is not a refusal");
        assert!(out.cids.is_empty());
        assert!(out.to_bytes().is_empty());
    }

    /// A full `WinAnsiEncoding` code->name table `E`.
    fn winansi_table() -> Box<[Option<String>; 256]> {
        Box::new(std::array::from_fn(|code| {
            u8::try_from(code)
                .ok()
                .and_then(|c| fontdata::encoding_glyph_name(BaseEncoding::WinAnsi, c))
                .map(str::to_owned)
        }))
    }

    /// An otherwise-empty `E` with the given (code, name) assignments.
    fn table_with(pairs: &[(u8, &str)]) -> Box<[Option<String>; 256]> {
        let mut t: Box<[Option<String>; 256]> = Box::new(std::array::from_fn(|_| None));
        for &(code, name) in pairs {
            t[code as usize] = Some(name.to_string());
        }
        t
    }

    #[test]
    fn happy_path_winansi_roundtrips_latin() {
        let inv = InverseEncoding::build("Helvetica", &winansi_table());
        let out = inv.encode_str("the", &BTreeSet::new()).unwrap();
        assert_eq!(out.codes, vec![b't', b'h', b'e']);
        assert!(out.disclosures.is_empty());
    }

    #[test]
    fn r_inv_1_absent_char_refuses() {
        let inv = InverseEncoding::build("Sub", &table_with(&[(0x41, "A")]));
        match inv.encode_char('B', &BTreeSet::new()) {
            CharEncoding::Refuse(r) => assert_eq!(r.trigger, RInvTrigger::TargetAbsent),
            other => panic!("expected R-INV-1, got {other:?}"),
        }
    }

    #[test]
    fn r_inv_5_ambiguous_chooses_and_discloses() {
        let inv = InverseEncoding::build("Sub", &table_with(&[(0x41, "A"), (0x61, "A")]));
        match inv.encode_char('A', &BTreeSet::new()) {
            CharEncoding::Chosen { code, .. } => assert_eq!(code, 0x41),
            other => panic!("expected R-INV-5, got {other:?}"),
        }
        let mut prefer = BTreeSet::new();
        prefer.insert(0x61u8);
        match inv.encode_char('A', &prefer) {
            CharEncoding::Chosen { code, .. } => assert_eq!(code, 0x61),
            other => panic!("expected reused code, got {other:?}"),
        }
    }

    #[test]
    fn r_inv_6_ligature_only_char_refuses_named() {
        let inv = InverseEncoding::build("Sub", &table_with(&[(0x01, "f_i")]));
        match inv.encode_char('i', &BTreeSet::new()) {
            CharEncoding::Refuse(r) => assert_eq!(r.trigger, RInvTrigger::LigatureOnly),
            other => panic!("expected R-INV-6, got {other:?}"),
        }
    }

    #[test]
    fn r_inv_7_occupied_slot_refuses() {
        let inv = InverseEncoding::build("Sub", &table_with(&[(0x41, "bullet")]));
        match inv.encode_char('A', &BTreeSet::new()) {
            CharEncoding::Refuse(r) => assert_eq!(r.trigger, RInvTrigger::CodeOccupied),
            other => panic!("expected R-INV-7, got {other:?}"),
        }
    }

    #[test]
    fn r_inv_8_astral_char_refuses() {
        let inv = InverseEncoding::build("Helvetica", &winansi_table());
        match inv.encode_char('\u{1D54F}', &BTreeSet::new()) {
            CharEncoding::Refuse(r) => assert_eq!(r.trigger, RInvTrigger::BeyondRepertoire),
            other => panic!("expected R-INV-8, got {other:?}"),
        }
    }

    #[test]
    fn encode_str_refuses_whole_run_on_one_bad_char() {
        let inv = InverseEncoding::build("Sub", &table_with(&[(0x74, "t"), (0x68, "h")]));
        let err = inv.encode_str("the", &BTreeSet::new()).unwrap_err();
        assert_eq!(err.trigger, RInvTrigger::TargetAbsent);
        assert_eq!(err.character, Some('e'));
    }

    #[test]
    fn trigger_hardness_matches_the_table() {
        for t in [
            RInvTrigger::TargetAbsent,
            RInvTrigger::SymbolicNoEncoding,
            RInvTrigger::ToUnicodeOnly,
            RInvTrigger::Composite,
            RInvTrigger::CodeOccupied,
            RInvTrigger::BeyondRepertoire,
        ] {
            assert!(t.is_hard(), "{} must be hard", t.id());
        }
        assert!(!RInvTrigger::Ambiguous.is_hard());
        assert!(!RInvTrigger::LigatureOnly.is_hard());
    }
}
