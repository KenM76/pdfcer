//! # `ToUnicode` CMap parsing — the §9.10.2 ladder's rung 1
//!
//! Parses the CMap-syntax subset ISO 32000-1 §9.10.3 defines for the
//! `/ToUnicode` entry of a font dictionary: `begincodespacerange`,
//! `beginbfchar`/`beginbfrange` and their `end…` partners, plus
//! `usecmap` for the inheritance form. Spec sources in the PDF-spec
//! RAG: `iso32000__s__9.10.3.md` (this clause), `iso32000__s__9.7.5.md`
//! (the generic CMap file format the syntax is a subset of).
//!
//! **This is the only rung of the ladder that needs no external data**,
//! which is why §9.10.2's rung 3 stays diagnosed-not-implemented while
//! this one ships complete: a `ToUnicode` CMap is self-contained inside
//! the PDF, so one tokenizer plus a `bfchar`/`bfrange` consumer is the
//! whole feature. For a modern subsetted `Identity-H` font it is also
//! the *only* route to Unicode at all (§9.10.2 N5).
//!
//! ## The four mapping forms (§9.10.3)
//!
//! | Form | Syntax | Semantics |
//! |---|---|---|
//! | **A** | `n beginbfchar` / `src dst` … / `endbfchar` | one code → one destination string |
//! | **B** | `n beginbfrange` / `lo hi dst` … / `endbfrange` | contiguous range → `dst` with its **last BYTE** incremented per code |
//! | **C** | `n beginbfrange` / `lo hi [dst1 … dstm]` … / `endbfrange` | contiguous range → an explicit array, `m = hi − lo + 1` |
//! | **D** | a *name* destination | **not described by §9.10.3** (N2) — see below |
//!
//! ## The three traps this module exists to not fall into
//!
//! 1. **One code may map to MANY code points.** The standard's own
//!    EXAMPLE 2 decomposes the `ff`/`fi`/`ffl` ligatures into two, two
//!    and three code points from one code each. The mapping value is
//!    therefore a *string*, never a `char`. A `HashMap<u16, char>` is
//!    wrong at the first ligature in the first real document.
//! 2. **Destinations are UTF-16BE byte strings, so surrogate pairs are
//!    normal.** EXAMPLE 2's own `<3A51> → <D840DC3E>` is U+2003E. A
//!    UCS-2 decoder truncates it silently. Decoding goes through
//!    [`crate::textstring::decode_utf16be_bytes`], which pairs
//!    surrogates and counts what it cannot.
//! 3. **Form B increments the LAST BYTE, not the code point.** Verbatim:
//!    "the last byte of the string shall be incremented for each
//!    consecutive code in the source code range", with the constraint
//!    "the value of the last byte in the string shall be less than or
//!    equal to `255 − (srcCode2 − srcCode1)` … otherwise, the result of
//!    mapping is undefined". The two only coincide when no carry
//!    occurs; the bound exists precisely because the standard declines
//!    to define the carry. pdfcer refuses past the bound and counts it
//!    ([`CMapStats::range_overflows`]) rather than inventing a carry.
//!
//! ## What this module deliberately does NOT do
//!
//! - **It does not decide code width.** §9.10.3's codespace ranges are
//!   parsed and exposed ([`ToUnicodeCMap::codespace_widths`]) for
//!   diagnostics, but segmentation of a show string into codes comes
//!   from the font's own `/Encoding` (1 byte for a simple font, 2 for
//!   `Identity-H`), never from here. §9.10.3 N1: the clause says only
//!   that the two "shall be consistent", with no precedence and no
//!   recovery — so pdfcer fixes the precedence itself and records the
//!   disagreement as a diagnostic.
//! - **It does not validate Table 120 entries.** `/CMapName`,
//!   `/CIDSystemInfo`, `/CMapType`, `/WMode` are explicitly "not
//!   pertinent" here; `/CMapType 2` is not even described anywhere in
//!   ISO 32000-1 (it comes from Adobe TN #5014). A `ToUnicode` stream is
//!   never rejected for a missing or nonsensical `CIDSystemInfo`.
//! - **It does not reject `cidrange`/`notdefrange`.** §9.7.5.4
//!   constraint (c) makes those illegal in a `ToUnicode` CMap, but the
//!   clause states no recovery — pdfcer skips them and counts
//!   ([`CMapStats::foreign_operators`]) rather than discarding a CMap
//!   whose `bfchar` entries are perfectly usable.
//!
//! ## Name destinations (form D) — extension territory
//!
//! §9.10.3 describes only *string* destinations, but the underlying
//! PostScript CMap format permits a name, and real producers emit them
//! (§9.10.3 N2). pdfcer accepts a name destination and resolves it
//! through the Adobe Glyph List, exactly as §9.10.2 rung 2 step (b)
//! does, counting it in [`CMapStats::name_destinations`]. This is a
//! documented extension, **not** conformance — the count is what makes
//! it visible rather than a silent divergence.
//!
//! ## Guards (`ARCHITECTURE.md` §10, R25)
//!
//! Every ceiling here is an explicit pdfcer constant with a stated
//! derivation, not a vendor default and not an intuition:
//! [`MAX_BF_ENTRIES`], [`MAX_BF_RANGES`], [`MAX_DST_BYTES`],
//! [`MAX_CMAP_TOKENS`]. Form-B ranges are stored **lazily** (three
//! integers plus the destination bytes) precisely so that a hostile
//! `<0000> <FFFF>` range costs one entry rather than 65,536.

use std::collections::{BTreeMap, BTreeSet};

use crate::fontdata;
use crate::lexer::{Lexer, TokenKind};
use crate::textstring::decode_utf16be_bytes;

/// Maximum number of materialized single-code mappings (`bfchar`
/// entries plus form-C array elements).
///
/// Both sources are bounded by the input's own length — each entry costs
/// at least a `<..> <..>` pair of tokens — so this ceiling is a
/// belt-and-braces bound on *memory*, not the primary defence. 500,000
/// entries is roughly 8× the largest plausible CMap (a full 2-byte
/// codespace fully enumerated by `bfchar` would be 65,536) and still
/// only tens of megabytes.
pub const MAX_BF_ENTRIES: usize = 500_000;

/// Maximum number of lazily-stored form-B ranges.
///
/// Ranges are the entries an attacker can make cheap to write and
/// expensive to expand, which is exactly why they are never expanded
/// (see the module docs). 100,000 bounds the lookup scan.
pub const MAX_BF_RANGES: usize = 100_000;

/// Maximum destination-string length in bytes.
///
/// **This one is spec-stated**, twice: §9.10.3 says "The value of
/// `dstString` can be a string of up to 512 bytes" for form A and again
/// for form C's array elements. 512 bytes = 256 UTF-16 code units. A
/// longer destination is non-conforming; pdfcer truncates nothing and
/// rejects the entry, counting it.
pub const MAX_DST_BYTES: usize = 512;

/// Maximum number of lexer tokens consumed from one CMap stream.
///
/// Bounds parse *time* independently of the entry ceilings above, since
/// a stream can be mostly comments, junk keywords, or `endcmap`-less
/// filler that produces no entries at all. Ten million tokens is far
/// beyond any real `ToUnicode` CMap (the largest CJK ones are in the
/// low hundreds of thousands).
pub const MAX_CMAP_TOKENS: usize = 10_000_000;

/// Why a `/ToUnicode` CMap could not be inverted (standing rule R110).
///
/// Each variant names a specific obstruction. "This font is unsupported"
/// would leave the operator with nothing to act on and no way to tell a
/// ligature from a genuinely broken map (R27).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum NotInjective {
    /// A code maps to more than one character — a ligature or a decomposed
    /// sequence.
    #[error(
        "code {code} maps to {text:?}, which is more than one character, so there is no single code that means just one of them"
    )]
    MultiCharDestination { code: u32, text: String },
    /// Two codes map to the same character.
    #[error(
        "codes {first} and {second} both map to {ch:?}, so mapping that character back to a code has no single answer"
    )]
    Collision { ch: char, first: u32, second: u32 },
    /// A code maps to the empty string.
    #[error("code {code} maps to nothing at all")]
    EmptyDestination { code: u32 },
    /// The CMap has no usable entries.
    #[error("this font's character map is empty")]
    Empty,
    /// The CMap declares more entries than pdfcer will materialise.
    #[error(
        "this font's character map declares more than {entries} entries; pdfcer will not expand it"
    )]
    TooLarge { entries: usize },
}

/// A parsed `ToUnicode` CMap: character code → Unicode **string**.
///
/// Cheap to clone is deliberately *not* claimed — a CMap is built once
/// per font per extraction and shared by reference.
#[derive(Debug, Clone, Default)]
pub struct ToUnicodeCMap {
    /// Form-A `bfchar` entries and form-C array elements, materialized.
    /// A `BTreeMap` rather than a `HashMap` so iteration (used by the
    /// diagnostics surface and by tests) is deterministic.
    singles: BTreeMap<u32, Box<str>>,
    /// Form-B ranges, stored lazily (see the module docs).
    ranges: Vec<BfRange>,
    /// Codespace byte widths declared by `begincodespacerange`, in
    /// declaration order. Diagnostics only — see the module docs.
    codespace_widths: Vec<u8>,
    /// What the parse had to tolerate.
    stats: CMapStats,
}

/// One form-B `bfrange`, unexpanded.
#[derive(Debug, Clone)]
struct BfRange {
    /// First source code (inclusive).
    lo: u32,
    /// Last source code (inclusive).
    hi: u32,
    /// The destination string's UTF-16BE bytes for code `lo`. Codes
    /// above `lo` increment the **last byte** of a copy of this.
    dst: Vec<u8>,
}

/// What a `ToUnicode` CMap parse had to tolerate, so the caller can
/// disclose it instead of pdfcer quietly absorbing it.
///
/// Every field corresponds to a NEGATIVE RESULT in
/// `iso32000__s__9.10.3.md` — a place where the standard states a
/// `shall` and then states no recovery. Counting them is what keeps
/// pdfcer's leniency honest.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct CMapStats {
    /// Form-A/form-C entries materialized.
    pub singles: usize,
    /// Form-B ranges stored.
    pub ranges: usize,
    /// Destination strings longer than [`MAX_DST_BYTES`] (§9.10.3's own
    /// 512-byte cap), rejected.
    pub oversize_destinations: usize,
    /// Form-B ranges whose last destination byte would pass 255 — the
    /// case §9.10.3 calls "undefined". Codes past the overflow point
    /// resolve to nothing rather than to a fabricated carry.
    pub range_overflows: usize,
    /// Form-C arrays whose length disagreed with `hi − lo + 1`. The
    /// overlap is used; the surplus (either side) is dropped.
    pub array_length_mismatches: usize,
    /// Destinations that were *names* rather than strings — the
    /// undocumented form D (N2), resolved through the AGL.
    pub name_destinations: usize,
    /// `cidrange`/`cidchar`/`notdefrange` blocks, which §9.7.5.4
    /// constraint (c) forbids in a `ToUnicode` CMap. Skipped.
    pub foreign_operators: usize,
    /// Destination byte strings that decoded imperfectly (odd length,
    /// unpaired surrogate — §9.10.3 N3).
    pub malformed_destinations: usize,
    /// `true` if a guard ceiling stopped the parse early, so the map is
    /// known-incomplete.
    pub truncated: bool,
    /// `usecmap` references, which pdfcer recognizes but does not follow
    /// (a `ToUnicode` CMap based on another one is legal per §9.10.3 but
    /// requires resolving a name to a bundled resource pdfcer does not
    /// carry).
    pub usecmap_references: usize,
}

impl ToUnicodeCMap {
    /// Parse a `ToUnicode` CMap from a decoded stream's bytes.
    ///
    /// Infallible by design: §9.10.3 is a chain of `shall`s with no
    /// stated recovery for any violation, and a CMap that is 99%
    /// well-formed still maps 99% of a document's text. Everything the
    /// parse had to tolerate lands in [`ToUnicodeCMap::stats`] instead
    /// of an error the caller would have to discard the whole map over.
    ///
    /// # Examples
    ///
    /// ```
    /// use pdfcer_core::text_extract::cmap::ToUnicodeCMap;
    ///
    /// // The §9.10.3 EXAMPLE 2 mapping blocks, verbatim.
    /// let cmap = ToUnicodeCMap::parse(
    ///     b"1 begincodespacerange
    ///       <0000> <FFFF>
    ///       endcodespacerange
    ///       2 beginbfrange
    ///       <0000> <005E> <0020>
    ///       <005F> <0061> [<00660066> <00660069> <00660066006C>]
    ///       endbfrange
    ///       1 beginbfchar
    ///       <3A51> <D840DC3E>
    ///       endbfchar",
    /// );
    /// // Form B: the LAST BYTE increments, 0x0000 -> U+0020 .. 0x005E -> U+007E.
    /// assert_eq!(cmap.lookup(0x0000).as_deref(), Some(" "));
    /// assert_eq!(cmap.lookup(0x0041).as_deref(), Some("a"));
    /// // Form C: one-to-many ligature decomposition.
    /// assert_eq!(cmap.lookup(0x0061).as_deref(), Some("ffl"));
    /// // Form A: a surrogate pair is a supplementary-plane character.
    /// assert_eq!(cmap.lookup(0x3A51).as_deref(), Some("\u{2003E}"));
    /// ```
    #[must_use]
    pub fn parse(bytes: &[u8]) -> Self {
        let mut out = Self::default();
        let mut lexer = Lexer::new(bytes);
        // Operand window: CMap block syntax is `… operands OPERATOR`, so
        // a small ring of recent operands is all the parser needs. Three
        // is the widest any form uses (form B's `lo hi dst`).
        let mut pending: Vec<Operand> = Vec::new();
        let mut tokens = 0usize;

        loop {
            if tokens >= MAX_CMAP_TOKENS {
                out.stats.truncated = true;
                break;
            }
            tokens += 1;
            let token = match lexer.next_token() {
                Ok(Some(t)) => t,
                // A lexical failure ends the parse: the remainder of the
                // stream cannot be resynchronized without guessing, and
                // whatever was already collected stays valid.
                Ok(None) | Err(_) => break,
            };
            match token.kind {
                TokenKind::String(s) => pending.push(Operand::Str(s)),
                TokenKind::Name(n) => pending.push(Operand::Name(n)),
                TokenKind::Integer(_) => pending.push(Operand::Int),
                TokenKind::ArrayOpen => {
                    let (items, consumed) = collect_array(&mut lexer, &mut out.stats);
                    tokens = tokens.saturating_add(consumed);
                    pending.push(Operand::Array(items));
                }
                TokenKind::Keyword => {
                    let word = token.lexeme(bytes).unwrap_or(b"");
                    out.apply_operator(word, bytes, &mut lexer, &mut tokens);
                    pending.clear();
                }
                // Dictionaries (the CMap stream's own Table 120 entries),
                // reals, braces and stray closers are all inert here.
                _ => {}
            }
            // Bound the operand window so a stream of a million hex
            // strings with no operator cannot grow it without limit.
            if pending.len() > 8 {
                pending.remove(0);
            }
            if out.stats.truncated {
                break;
            }
        }
        out.stats.singles = out.singles.len();
        out.stats.ranges = out.ranges.len();
        out
    }

    /// Handle one CMap operator. Block operators (`beginbfchar` …)
    /// consume their own body from the lexer, which keeps the outer loop
    /// free of block state.
    fn apply_operator(
        &mut self,
        word: &[u8],
        buf: &[u8],
        lexer: &mut Lexer<'_>,
        tokens: &mut usize,
    ) {
        match word {
            b"beginbfchar" => self.read_bfchar(buf, lexer, tokens),
            b"beginbfrange" => self.read_bfrange(buf, lexer, tokens),
            b"begincodespacerange" => self.read_codespace(buf, lexer, tokens),
            b"usecmap" => self.stats.usecmap_references += 1,
            // §9.7.5.4 constraint (c): these belong to an `Encoding`
            // CMap and `shall not` appear here. Skip the block, count it.
            b"begincidrange" | b"begincidchar" | b"beginnotdefrange" => {
                self.stats.foreign_operators += 1;
                skip_to_end(buf, lexer, tokens);
            }
            _ => {}
        }
    }

    /// Form A: `n beginbfchar` / `srcCode dstString` … / `endbfchar`.
    fn read_bfchar(&mut self, buf: &[u8], lexer: &mut Lexer<'_>, tokens: &mut usize) {
        let mut operands: Vec<Operand> = Vec::new();
        while let Some(op) = next_operand(buf, lexer, tokens, &mut self.stats) {
            match op {
                Operand::End => break,
                other => {
                    operands.push(other);
                    if operands.len() == 2 {
                        let dst = operands.pop();
                        let src = operands.pop();
                        if let (Some(Operand::Str(src)), Some(dst)) = (src, dst)
                            && let Some(code) = code_from_bytes(&src)
                            && let Some(text) = self.destination(&dst)
                        {
                            self.insert_single(code, text);
                        }
                        operands.clear();
                    }
                }
            }
            if self.stats.truncated {
                return;
            }
        }
    }

    /// Forms B and C: `n beginbfrange` / `lo hi dst-or-array` … /
    /// `endbfrange`.
    fn read_bfrange(&mut self, buf: &[u8], lexer: &mut Lexer<'_>, tokens: &mut usize) {
        let mut operands: Vec<Operand> = Vec::new();
        while let Some(op) = next_operand(buf, lexer, tokens, &mut self.stats) {
            match op {
                Operand::End => break,
                other => {
                    operands.push(other);
                    if operands.len() == 3 {
                        let dst = operands.pop();
                        let hi = operands.pop();
                        let lo = operands.pop();
                        if let (Some(Operand::Str(lo)), Some(Operand::Str(hi)), Some(dst)) =
                            (lo, hi, dst)
                            && let (Some(lo), Some(hi)) =
                                (code_from_bytes(&lo), code_from_bytes(&hi))
                            && lo <= hi
                        {
                            self.insert_range(lo, hi, dst);
                        }
                        operands.clear();
                    }
                }
            }
            if self.stats.truncated {
                return;
            }
        }
    }

    /// `n begincodespacerange` / `lo hi` … / `endcodespacerange`.
    ///
    /// Only the byte WIDTH is retained. §9.10.3 makes the codespace a
    /// consistency statement about the font's encoding, not a
    /// segmentation authority for extraction (module docs / N1).
    fn read_codespace(&mut self, buf: &[u8], lexer: &mut Lexer<'_>, tokens: &mut usize) {
        while let Some(op) = next_operand(buf, lexer, tokens, &mut self.stats) {
            match op {
                Operand::End => break,
                Operand::Str(s) => {
                    if let Ok(width) = u8::try_from(s.len())
                        && width > 0
                        && !self.codespace_widths.contains(&width)
                    {
                        self.codespace_widths.push(width);
                    }
                }
                _ => {}
            }
            if self.stats.truncated {
                return;
            }
        }
    }

    /// Resolve one destination operand to its Unicode string.
    ///
    /// Handles both the documented string form and the undocumented
    /// name form (module docs, N2).
    fn destination(&mut self, operand: &Operand) -> Option<Box<str>> {
        match operand {
            Operand::Str(bytes) => {
                if bytes.len() > MAX_DST_BYTES {
                    self.stats.oversize_destinations += 1;
                    return None;
                }
                let (text, replacements) = decode_utf16be_bytes(bytes);
                if replacements > 0 {
                    self.stats.malformed_destinations += 1;
                }
                (!text.is_empty()).then(|| text.into_boxed_str())
            }
            Operand::Name(name) => {
                self.stats.name_destinations += 1;
                let name = std::str::from_utf8(name).ok()?;
                let text = fontdata::glyph_name_to_unicode_string(name)?;
                (!text.is_empty()).then(|| text.into_boxed_str())
            }
            _ => None,
        }
    }

    /// Insert one materialized mapping, honouring [`MAX_BF_ENTRIES`].
    ///
    /// Overlapping entries are **last-wins**: §9.10.3 N5 records that no
    /// non-overlap rule exists for `bf` mappings (the `shall` about
    /// non-overlap governs *codespace* ranges only) and states no
    /// precedence, so pdfcer picks the rule a sequential reader falls
    /// into naturally and documents it here.
    fn insert_single(&mut self, code: u32, text: Box<str>) {
        if self.singles.len() >= MAX_BF_ENTRIES {
            self.stats.truncated = true;
            return;
        }
        self.singles.insert(code, text);
    }

    /// Insert a `bfrange`, choosing form B or form C by destination type.
    fn insert_range(&mut self, lo: u32, hi: u32, dst: Operand) {
        match dst {
            // Form C: an explicit array, one destination per code.
            Operand::Array(items) => {
                let span = (hi - lo).saturating_add(1) as usize;
                if items.len() != span {
                    self.stats.array_length_mismatches += 1;
                }
                for (offset, item) in items.iter().enumerate() {
                    let Ok(offset) = u32::try_from(offset) else {
                        break;
                    };
                    let Some(code) = lo.checked_add(offset) else {
                        break;
                    };
                    if code > hi {
                        break;
                    }
                    if let Some(text) = self.destination(item) {
                        self.insert_single(code, text);
                    }
                    if self.stats.truncated {
                        return;
                    }
                }
            }
            // Form B: increment the last byte per successive code.
            Operand::Str(bytes) => {
                if bytes.len() > MAX_DST_BYTES {
                    self.stats.oversize_destinations += 1;
                    return;
                }
                if bytes.is_empty() {
                    return;
                }
                if self.ranges.len() >= MAX_BF_RANGES {
                    self.stats.truncated = true;
                    return;
                }
                // §9.10.3's own bound: last byte ≤ 255 − (hi − lo).
                // Beyond it "the result of mapping is undefined" — count
                // now so the diagnostic exists even if no code in the
                // overflowing tail is ever looked up.
                let last = bytes.last().copied().unwrap_or(0);
                if u32::from(last).saturating_add(hi - lo) > 255 {
                    self.stats.range_overflows += 1;
                }
                self.ranges.push(BfRange { lo, hi, dst: bytes });
            }
            // A single name over a range has no defined increment rule;
            // apply it to the first code only and count it as the
            // extension it is.
            Operand::Name(_) => {
                if let Some(text) = self.destination(&dst) {
                    self.insert_single(lo, text);
                }
            }
            _ => {}
        }
    }

    /// Map one character code to its Unicode string.
    ///
    /// `None` means this CMap does not cover the code. §9.10.3 N4
    /// records that the standard says nothing about an uncovered code —
    /// there is no `notdef` analogue for `ToUnicode` and no stated
    /// fallthrough to §9.10.2's other rungs. The per-code fallthrough
    /// every real reader implements is therefore pdfcer *policy*,
    /// implemented one level up in
    /// [`super::font::ExtractFont::to_unicode`], not conformance.
    ///
    /// Materialized entries (forms A and C) win over form-B ranges,
    /// because a `bfchar` naming a specific code is the more specific
    /// statement. The standard does not address the conflict.
    #[must_use]
    pub fn lookup(&self, code: u32) -> Option<String> {
        if let Some(text) = self.singles.get(&code) {
            return Some(text.to_string());
        }
        // Reverse scan: last-wins among overlapping ranges, matching
        // `insert_single`'s rule (N5 leaves both undefined).
        for range in self.ranges.iter().rev() {
            if code < range.lo || code > range.hi {
                continue;
            }
            let delta = code - range.lo;
            let mut bytes = range.dst.clone();
            let Some(last) = bytes.last_mut() else {
                continue;
            };
            // Refuse rather than carry: §9.10.3 declares the result
            // undefined past 255, and a fabricated carry would be a
            // confident wrong character.
            let Some(incremented) = last.checked_add(u8::try_from(delta).ok()?) else {
                continue;
            };
            *last = incremented;
            let (text, _) = decode_utf16be_bytes(&bytes);
            if !text.is_empty() {
                return Some(text);
            }
        }
        None
    }

    /// Build the code←character inverse, but only if this CMap is INJECTIVE.
    ///
    /// # Why editing a composite run turns on exactly this
    ///
    /// R-INV-4 refuses to edit `/Type0` runs because there is no
    /// Unicode→CID map: the PDF carries CID→Unicode, and inverting it is
    /// *lossy in general*. "In general" is doing a lot of work in that
    /// sentence — inversion is lossy precisely when the forward map is not
    /// injective, and injectivity is a **checkable property of the data**
    /// rather than something to assume either way (standing rule R110).
    ///
    /// A pdfcer-authored CMap is injective by construction. It is checked
    /// anyway, because authorship is a claim and the check is evidence
    /// (R93) — and because plenty of third-party CMaps are injective too, so
    /// checking rather than trusting is what makes the lift general instead
    /// of self-serving.
    ///
    /// # What disqualifies a CMap, and why each one genuinely does
    ///
    /// * **A multi-character destination.** A code mapping to `"ffi"` is a
    ///   ligature. Editing one character of that run has no single answer —
    ///   there is no code meaning "just the f" — so the whole run stays
    ///   refused rather than pdfcer picking an interpretation.
    /// * **Two codes mapping to the same character.** The inverse is then a
    ///   relation, not a function: pdfcer would have to choose which code to
    ///   write, and either choice silently changes which glyph appears.
    /// * **An empty destination.** Nothing to invert.
    ///
    /// # Errors
    ///
    /// Returns [`NotInjective`] naming the specific obstruction, so the
    /// refusal can tell the operator which font and which character rather
    /// than "this font is unsupported".
    pub fn injective_inverse(&self) -> Result<BTreeMap<char, u32>, NotInjective> {
        let mut inverse: BTreeMap<char, u32> = BTreeMap::new();
        let mut seen = 0usize;

        // Ranges are expanded here, unlike `lookup`, which resolves them
        // lazily. Injectivity is a property of the WHOLE map — a collision
        // between a range member and a single is invisible to any
        // point lookup — so there is no way to answer this question without
        // materialising. The ceiling below is what keeps that safe.
        //
        // ★★ THE CODE SET IS DEDUPLICATED, AND THAT IS A BUG FIX, NOT A
        // TIDY-UP (`Pass 121.0`). This loop used to push the singles into a
        // `Vec` and then push `lookup(code)` for every code of every range —
        // but `lookup` CONSULTS THE SINGLES FIRST (that is its documented
        // precedence), so a code present as a `bfchar` AND inside a `bfrange`
        // was pushed TWICE, with identical text both times. The injectivity
        // check downstream then saw the same character arrive under the same
        // code and reported a collision **of a code with itself**:
        //
        //     codes 361 and 361 both map to 'Ʃ'
        //
        // A nonsense sentence, and worse, a FALSE REFUSAL — the font is
        // perfectly invertible. It fired on the operator's own benchmark CAD
        // drawing (`.SFNS-Regular`), where it read as "pdfcer cannot edit this
        // text" for a reason that did not exist. Two overlapping RANGES
        // produced the same false positive by the same route, since `lookup`
        // resolves overlaps last-wins and would return one range's answer
        // twice.
        //
        // Iterating DISTINCT codes and asking `lookup` once per code makes the
        // materialised map agree with the lazy one by construction: one code,
        // one answer, whichever tier supplies it. A genuine collision — two
        // DIFFERENT codes, one character — is unaffected and still refused.
        let mut codes: BTreeSet<u32> = self.singles.keys().copied().collect();
        seen = seen.max(codes.len());
        if seen > MAX_BF_ENTRIES {
            return Err(NotInjective::TooLarge {
                entries: MAX_BF_ENTRIES,
            });
        }
        for range in &self.ranges {
            // `lo..=hi` is attacker-controlled and may span the whole 32-bit
            // space. Bounded per range AND in total.
            let span = u64::from(range.hi) - u64::from(range.lo) + 1;
            if span > MAX_BF_ENTRIES as u64 {
                return Err(NotInjective::TooLarge {
                    entries: MAX_BF_ENTRIES,
                });
            }
            for code in range.lo..=range.hi {
                codes.insert(code);
                // Counted per code CONSIDERED, not per code stored, so a
                // document that spends the budget on overlapping ranges is
                // bounded by the same ceiling as one that spends it on
                // distinct codes. Deduplication must not become a way to make
                // the guard cheaper to defeat.
                seen += 1;
                if seen > MAX_BF_ENTRIES {
                    return Err(NotInjective::TooLarge {
                        entries: MAX_BF_ENTRIES,
                    });
                }
            }
        }
        let all: Vec<(u32, String)> = codes
            .into_iter()
            .filter_map(|code| self.lookup(code).map(|text| (code, text)))
            .collect();

        for (code, text) in all {
            let mut chars = text.chars();
            let (Some(ch), None) = (chars.next(), chars.next()) else {
                if text.is_empty() {
                    return Err(NotInjective::EmptyDestination { code });
                }
                return Err(NotInjective::MultiCharDestination { code, text });
            };
            if let Some(&first) = inverse.get(&ch) {
                // Two codes, one character. Report BOTH codes: knowing only
                // the character leaves the operator unable to find the
                // problem in the font.
                return Err(NotInjective::Collision {
                    ch,
                    first,
                    second: code,
                });
            }
            inverse.insert(ch, code);
        }

        if inverse.is_empty() {
            return Err(NotInjective::Empty);
        }
        Ok(inverse)
    }

    /// Whether this CMap contains no usable mapping at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.singles.is_empty() && self.ranges.is_empty()
    }

    /// The distinct code widths (in bytes) declared by
    /// `begincodespacerange`, in declaration order.
    ///
    /// Diagnostics only: a simple font's `ToUnicode` codespace `shall`
    /// be one byte long (§9.10.3), and a disagreement with the font's
    /// own encoding is worth surfacing — but the font's `/Encoding`, not
    /// this, decides how a show string is split into codes (N1).
    #[must_use]
    pub fn codespace_widths(&self) -> &[u8] {
        &self.codespace_widths
    }

    /// What the parse had to tolerate.
    #[must_use]
    pub fn stats(&self) -> CMapStats {
        self.stats
    }
}

/// One CMap operand, in the narrow set §9.10.3's forms use.
#[derive(Debug, Clone)]
enum Operand {
    /// A literal or hex string — the source code or destination.
    Str(Vec<u8>),
    /// A name destination (the undocumented form D).
    Name(Vec<u8>),
    /// An integer — a block's entry count, or (in malformed producer
    /// output) a source code written as a number rather than a hex
    /// string. The value is never used: §9.10.3's own forms all take
    /// hex-string sources, and the block counts are advisory (the
    /// `end…` operator is what actually terminates a block).
    Int,
    /// A form-C destination array.
    Array(Vec<Operand>),
    /// An `end…` operator closing the current block.
    End,
}

/// Pull the next operand inside a `begin…`/`end…` block.
///
/// Returns `None` at end of input (an unterminated block, which
/// §9.10.3 does not address) and [`Operand::End`] at any `end…`
/// keyword.
fn next_operand(
    buf: &[u8],
    lexer: &mut Lexer<'_>,
    tokens: &mut usize,
    stats: &mut CMapStats,
) -> Option<Operand> {
    loop {
        if *tokens >= MAX_CMAP_TOKENS {
            stats.truncated = true;
            return None;
        }
        *tokens += 1;
        let token = match lexer.next_token() {
            Ok(Some(t)) => t,
            Ok(None) | Err(_) => return None,
        };
        match token.kind {
            TokenKind::String(s) => return Some(Operand::Str(s)),
            TokenKind::Name(n) => return Some(Operand::Name(n)),
            TokenKind::Integer(_) => return Some(Operand::Int),
            TokenKind::ArrayOpen => {
                let (items, consumed) = collect_array(lexer, stats);
                *tokens = tokens.saturating_add(consumed);
                return Some(Operand::Array(items));
            }
            TokenKind::Keyword => {
                // Any `end…` closes the block. Being liberal about
                // *which* `end…` is deliberate: a producer that writes
                // `endbfchar` to close a `beginbfrange` has made a
                // typing error, not a semantic one, and the block is
                // over either way.
                let is_end = token.lexeme(buf).is_some_and(|w| w.starts_with(b"end"));
                if is_end {
                    return Some(Operand::End);
                }
            }
            _ => {}
        }
    }
}

/// Collect a form-C destination array after its `[` has been consumed.
/// Returns the items and the number of tokens consumed.
fn collect_array(lexer: &mut Lexer<'_>, stats: &mut CMapStats) -> (Vec<Operand>, usize) {
    let mut items = Vec::new();
    let mut consumed = 0usize;
    loop {
        consumed += 1;
        // An array cannot legitimately be larger than the range it
        // covers, and a 2-byte codespace caps that at 65,536.
        if items.len() > 65_536 || consumed > MAX_CMAP_TOKENS {
            stats.truncated = true;
            return (items, consumed);
        }
        match lexer.next_token() {
            Ok(Some(t)) => match t.kind {
                TokenKind::ArrayClose => return (items, consumed),
                TokenKind::String(s) => items.push(Operand::Str(s)),
                TokenKind::Name(n) => items.push(Operand::Name(n)),
                _ => {}
            },
            Ok(None) | Err(_) => return (items, consumed),
        }
    }
}

/// Skip a `begin…`/`end…` block whose contents pdfcer does not consume.
fn skip_to_end(buf: &[u8], lexer: &mut Lexer<'_>, tokens: &mut usize) {
    let mut stats = CMapStats::default();
    while let Some(op) = next_operand(buf, lexer, tokens, &mut stats) {
        if matches!(op, Operand::End) {
            return;
        }
    }
}

/// A source code as written in a CMap: a big-endian byte string, one to
/// four bytes.
///
/// Returns `None` for an empty or over-wide string. Four bytes is the
/// CMap format's own maximum codespace width (§9.7.5.2); a wider one
/// could not be a character code in any font.
fn code_from_bytes(bytes: &[u8]) -> Option<u32> {
    if bytes.is_empty() || bytes.len() > 4 {
        return None;
    }
    let mut value = 0u32;
    for &b in bytes {
        value = (value << 8) | u32::from(b);
    }
    Some(value)
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

    /// Build a CMap from `beginbfchar` entries, for the R110 inverse tests.
    fn bfchar_cmap(pairs: &[(u16, &str)]) -> ToUnicodeCMap {
        let mut body =
            String::from("begincmap\n1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n");
        body.push_str(&format!("{} beginbfchar\n", pairs.len()));
        for (code, dst) in pairs {
            let hex: String = dst.encode_utf16().map(|u| format!("{u:04X}")).collect();
            body.push_str(&format!("<{code:04X}> <{hex}>\n"));
        }
        body.push_str("endbfchar\nendcmap\n");
        ToUnicodeCMap::parse(body.as_bytes())
    }

    /// The ordinary case R110 exists to allow: a one-to-one CMap inverts.
    #[test]
    fn a_one_to_one_cmap_inverts() {
        let hiragana_a = '\u{3042}';
        let cmap = bfchar_cmap(&[(1, "A"), (2, "B"), (3, "\u{3042}")]);
        let inv = cmap.injective_inverse().expect("this CMap is injective");
        assert_eq!(inv.get(&'A'), Some(&1));
        assert_eq!(inv.get(&'B'), Some(&2));
        assert_eq!(inv.get(&hiragana_a), Some(&3), "non-Latin must invert too");
        assert_eq!(inv.len(), 3);
    }

    /// Two codes, one character: the inverse is a relation, not a function.
    ///
    /// pdfcer would have to CHOOSE which code to write back, and either
    /// choice silently changes which glyph appears. Both codes are named in
    /// the error, because knowing only the character leaves the operator
    /// unable to find the problem in the font.
    #[test]
    fn two_codes_mapping_to_one_character_is_refused_with_both_codes_named() {
        let cmap = bfchar_cmap(&[(1, "A"), (7, "A")]);
        let err = cmap.injective_inverse().unwrap_err();
        match err {
            NotInjective::Collision { ch, first, second } => {
                assert_eq!(ch, 'A');
                assert_eq!((first, second), (1, 7));
            }
            other => panic!("expected Collision, got {other:?}"),
        }
    }

    /// ★★ A CODE COVERED BY BOTH A `bfchar` AND A `bfrange` IS ONE CODE, NOT
    /// A COLLISION (`Pass 121.0`).
    ///
    /// The materialising loop used to push the singles and then push
    /// `lookup(code)` for every code of every range — but `lookup` consults
    /// the singles FIRST, so a code present in both tiers was pushed twice
    /// with identical text. The injectivity check then reported a collision
    /// **of a code with itself**:
    ///
    /// ```text
    /// codes 361 and 361 both map to 'Ʃ'
    /// ```
    ///
    /// A nonsense sentence and a **false refusal** — the map is perfectly
    /// invertible. It fired on the operator's own benchmark CAD drawing, where
    /// it read as "pdfcer cannot edit this text" for a reason that did not
    /// exist. Note the shape: the message was *visibly* absurd (the same
    /// number twice) and had been shipping regardless, because nothing reads a
    /// refusal message it never expects to see.
    #[test]
    fn a_code_in_both_a_bfchar_and_a_bfrange_is_not_a_collision_with_itself() {
        let body = concat!(
            "begincmap
1 begincodespacerange
<0000> <FFFF>
endcodespacerange
",
            "1 beginbfchar
<0005> <0041>
endbfchar
",
            "1 beginbfrange
<0005> <0007> <0041>
endbfrange
",
            "endcmap
"
        );
        let cmap = ToUnicodeCMap::parse(body.as_bytes());
        let inverse = cmap
            .injective_inverse()
            .expect("one code in two tiers is one code, not two");
        // And the answer agrees with `lookup`'s own precedence: the single
        // wins, so 'A' inverts to 5 rather than to a range-derived code.
        assert_eq!(inverse.get(&'A'), Some(&5));
    }

    /// Two OVERLAPPING RANGES produced the same false collision by the same
    /// route — `lookup` resolves an overlap last-wins and returned one
    /// range's answer for both iterations. Pinned separately because the two
    /// share a fix but not a trigger, and a fix verified on one of two
    /// triggers is a fix verified on one of two triggers.
    #[test]
    fn overlapping_bfranges_are_not_a_collision_with_themselves() {
        let body = concat!(
            "begincmap
1 begincodespacerange
<0000> <FFFF>
endcodespacerange
",
            "2 beginbfrange
<0010> <0012> <0041>
<0011> <0013> <0042>
endbfrange
",
            "endcmap
"
        );
        let cmap = ToUnicodeCMap::parse(body.as_bytes());
        // ★ Asserted as a POSITIVE, not as "no self-collision". The first
        // draft of this test was `if let Err(Collision) { assert_ne!(first,
        // second) }` — which passes vacuously whenever the call succeeds AND
        // whenever it fails for any other reason, and it duly passed against
        // a deliberately re-broken build. A conditional assertion about an
        // error that may not occur tests nothing.
        //
        // The map's own answer is determinate: `lookup` resolves overlaps
        // last-wins, so 0x11..0x13 come from the SECOND range and 0x10 from
        // the first — codes 16..19 mapping to 'A'..'D', which is injective.
        let inverse = cmap
            .injective_inverse()
            .expect("overlapping ranges resolve to one answer per code");
        assert_eq!(inverse.get(&'A'), Some(&0x10));
        assert_eq!(inverse.get(&'B'), Some(&0x11));
        assert_eq!(inverse.get(&'C'), Some(&0x12));
        assert_eq!(inverse.get(&'D'), Some(&0x13));
    }

    /// A ligature — one code, several characters — has no single-character
    /// inverse, so the run stays refused rather than pdfcer picking an
    /// interpretation of "edit the f in ffi".
    #[test]
    fn a_ligature_destination_is_refused_by_name() {
        let cmap = bfchar_cmap(&[(1, "A"), (2, "ffi")]);
        let err = cmap.injective_inverse().unwrap_err();
        match err {
            NotInjective::MultiCharDestination { code, text } => {
                assert_eq!(code, 2);
                assert_eq!(text, "ffi");
            }
            other => panic!("expected MultiCharDestination, got {other:?}"),
        }
    }

    /// A CMap pdfcer did NOT author must be evaluated on its merits, or R110
    /// is a rule that only ever says yes to pdfcer's own output — which
    /// would make the whole lift self-serving rather than general.
    ///
    /// The §9.10.3 EXAMPLE 2 body is the least pdfcer-shaped CMap available:
    /// it is the standard's own, written years before this project.
    #[test]
    fn the_standards_own_example_cmap_is_evaluated_on_its_merits() {
        let cmap = ToUnicodeCMap::parse(EXAMPLE_2);
        // Deliberately NOT asserting "it inverts". Whether the standard's
        // example happens to be injective is a fact about that example, not
        // about pdfcer. What matters is that the check RUNS on a foreign
        // CMap and reaches a decision with a stated reason, rather than
        // special-casing provenance.
        match cmap.injective_inverse() {
            Ok(inv) => assert!(!inv.is_empty(), "an Ok inverse must not be empty"),
            Err(e) => assert!(!e.to_string().is_empty(), "a refusal must state its reason"),
        }
    }

    /// An empty CMap has nothing to invert, and must say so rather than
    /// returning an empty map a caller would read as "everything is
    /// editable".
    #[test]
    fn an_empty_cmap_is_refused_rather_than_inverting_to_nothing() {
        let cmap = ToUnicodeCMap::parse(b"begincmap\nendcmap\n");
        assert_eq!(cmap.injective_inverse().unwrap_err(), NotInjective::Empty);
    }

    /// The §9.10.3 EXAMPLE 2 body, verbatim from the standard.
    const EXAMPLE_2: &[u8] = b"/CIDInit /ProcSet findresource begin
12 dict begin
begincmap
/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def
/CMapName /Adobe-Identity-UCS def
/CMapType 2 def
1 begincodespacerange
<0000> <FFFF>
endcodespacerange
2 beginbfrange
<0000> <005E> <0020>
<005F> <0061> [<00660066> <00660069> <00660066006C>]
endbfrange
1 beginbfchar
<3A51> <D840DC3E>
endbfchar
endcmap
CMapName currentdict /CMap defineresource pop
end
end";

    #[test]
    fn example_2_form_b_increments_the_last_byte() {
        let cmap = ToUnicodeCMap::parse(EXAMPLE_2);
        // The spec's own gloss: "<0000> to <005E> are mapped to the
        // Unicode values U+0020 to U+007E".
        assert_eq!(cmap.lookup(0x0000).as_deref(), Some(" "));
        assert_eq!(cmap.lookup(0x0001).as_deref(), Some("!"));
        assert_eq!(cmap.lookup(0x005E).as_deref(), Some("~"));
        assert!(cmap.lookup(0x005F).is_some(), "form C takes over");
    }

    #[test]
    fn example_2_form_c_is_one_to_many() {
        let cmap = ToUnicodeCMap::parse(EXAMPLE_2);
        // The three ligature decompositions, from the standard's own
        // example. A `code -> char` model cannot represent these.
        assert_eq!(cmap.lookup(0x005F).as_deref(), Some("ff"));
        assert_eq!(cmap.lookup(0x0060).as_deref(), Some("fi"));
        assert_eq!(cmap.lookup(0x0061).as_deref(), Some("ffl"));
    }

    #[test]
    fn example_2_form_a_surrogate_pair() {
        let cmap = ToUnicodeCMap::parse(EXAMPLE_2);
        // <D840DC3E> is U+2003E in UTF-16BE. A UCS-2 decoder truncates
        // this silently; this assertion is the guard against that.
        assert_eq!(cmap.lookup(0x3A51).as_deref(), Some("\u{2003E}"));
        assert_eq!(cmap.lookup(0x3A51).unwrap().chars().count(), 1);
    }

    #[test]
    fn example_2_ignores_table_120_entries_entirely() {
        // /CIDSystemInfo, /CMapName and the undocumented /CMapType 2 are
        // all "not pertinent" (§9.10.3) — the parse must neither reject
        // nor be confused by them, and `def`/`begin`/`end` must not be
        // mistaken for block terminators in a way that loses entries.
        let cmap = ToUnicodeCMap::parse(EXAMPLE_2);
        assert_eq!(cmap.stats().singles, 4, "3 ligatures + 1 bfchar");
        assert_eq!(cmap.stats().ranges, 1);
        assert_eq!(cmap.codespace_widths(), &[2]);
    }

    #[test]
    fn simple_font_one_byte_codespace() {
        let cmap = ToUnicodeCMap::parse(
            b"1 begincodespacerange <00> <FF> endcodespacerange
              2 beginbfchar <41> <0041> <42> <00C4> endbfchar",
        );
        assert_eq!(cmap.codespace_widths(), &[1]);
        assert_eq!(cmap.lookup(0x41).as_deref(), Some("A"));
        assert_eq!(cmap.lookup(0x42).as_deref(), Some("\u{00C4}"));
        assert_eq!(cmap.lookup(0x43), None, "uncovered code maps to nothing");
    }

    #[test]
    fn form_b_overflow_past_255_is_refused_not_carried() {
        // §9.10.3: "the value of the last byte in the string shall be
        // less than or equal to 255 - (srcCode2 - srcCode1) … otherwise
        // the result of mapping is undefined." dst 0x00FE over a range
        // of 4 overflows at the third code.
        let cmap = ToUnicodeCMap::parse(b"1 beginbfrange <0010> <0014> <00FE> endbfrange");
        assert_eq!(cmap.lookup(0x0010).as_deref(), Some("\u{00FE}"));
        assert_eq!(cmap.lookup(0x0011).as_deref(), Some("\u{00FF}"));
        assert_eq!(
            cmap.lookup(0x0012),
            None,
            "past 255 the standard declares the result undefined"
        );
        assert_eq!(cmap.stats().range_overflows, 1);
    }

    #[test]
    fn form_b_increments_bytes_not_code_points() {
        // The two coincide only when no carry occurs. A destination
        // whose low byte is 0xFF proves the difference: a code-point
        // increment would give U+0100, a byte increment overflows.
        let cmap = ToUnicodeCMap::parse(b"1 beginbfrange <0000> <0001> <00FF> endbfrange");
        assert_eq!(cmap.lookup(0x0000).as_deref(), Some("\u{00FF}"));
        assert_eq!(cmap.lookup(0x0001), None);
    }

    #[test]
    fn bfchar_wins_over_an_overlapping_bfrange() {
        let cmap = ToUnicodeCMap::parse(
            b"1 beginbfrange <0000> <00FF> <0041> endbfrange
              1 beginbfchar <0005> <005A> endbfchar",
        );
        assert_eq!(cmap.lookup(0x0004).as_deref(), Some("E"));
        assert_eq!(
            cmap.lookup(0x0005).as_deref(),
            Some("Z"),
            "bfchar is more specific"
        );
    }

    #[test]
    fn overlapping_ranges_are_last_wins() {
        // §9.10.3 N5: no non-overlap rule exists for bf mappings and no
        // precedence is stated. pdfcer documents last-wins; this pins it.
        let cmap = ToUnicodeCMap::parse(
            b"2 beginbfrange <0000> <000F> <0041> <0000> <000F> <0061> endbfrange",
        );
        assert_eq!(cmap.lookup(0x0000).as_deref(), Some("a"));
    }

    #[test]
    fn oversize_destination_is_rejected_and_counted() {
        // §9.10.3's own 512-byte cap. 600 bytes of hex = 300 code units.
        let mut src = b"1 beginbfchar <0001> <".to_vec();
        src.extend(std::iter::repeat_n(b'0', 1200));
        src.extend_from_slice(b"> endbfchar");
        let cmap = ToUnicodeCMap::parse(&src);
        assert_eq!(cmap.lookup(0x0001), None);
        assert_eq!(cmap.stats().oversize_destinations, 1);
    }

    #[test]
    fn form_c_array_length_mismatch_is_counted_not_fatal() {
        // m must equal hi - lo + 1 (= 3 here); the array has 2.
        let cmap = ToUnicodeCMap::parse(b"1 beginbfrange <0000> <0002> [<0041> <0042>] endbfrange");
        assert_eq!(cmap.lookup(0x0000).as_deref(), Some("A"));
        assert_eq!(cmap.lookup(0x0001).as_deref(), Some("B"));
        assert_eq!(cmap.lookup(0x0002), None);
        assert_eq!(cmap.stats().array_length_mismatches, 1);
    }

    #[test]
    fn name_destination_is_the_documented_extension() {
        // §9.10.3 N2: name destinations are NOT described by the clause,
        // but real producers emit them. Accepted via the AGL, counted.
        let cmap = ToUnicodeCMap::parse(b"1 beginbfchar <41> /Adieresis endbfchar");
        assert_eq!(cmap.lookup(0x41).as_deref(), Some("\u{00C4}"));
        assert_eq!(cmap.stats().name_destinations, 1);
    }

    #[test]
    fn ligature_name_destination_resolves_to_many_code_points() {
        let cmap = ToUnicodeCMap::parse(b"1 beginbfchar <41> /f_i endbfchar");
        assert_eq!(cmap.lookup(0x41).as_deref(), Some("fi"));
    }

    #[test]
    fn cid_operators_are_skipped_and_counted() {
        // §9.7.5.4 constraint (c) forbids cidrange in a ToUnicode CMap.
        // The usable bfchar entries must survive the violation.
        let cmap = ToUnicodeCMap::parse(
            b"1 begincidrange <0000> <00FF> 0 endcidrange
              1 beginbfchar <41> <0041> endbfchar",
        );
        assert_eq!(cmap.stats().foreign_operators, 1);
        assert_eq!(cmap.lookup(0x41).as_deref(), Some("A"));
    }

    #[test]
    fn usecmap_is_recognized_but_not_followed() {
        let cmap = ToUnicodeCMap::parse(
            b"/Adobe-Identity-UCS usecmap 1 beginbfchar <41> <0041> endbfchar",
        );
        assert_eq!(cmap.stats().usecmap_references, 1);
        assert_eq!(cmap.lookup(0x41).as_deref(), Some("A"));
    }

    #[test]
    fn unterminated_block_keeps_what_it_parsed() {
        // §9.10.3 states no recovery for a missing `endbfchar`.
        let cmap = ToUnicodeCMap::parse(b"2 beginbfchar <41> <0041> <42> <0042>");
        assert_eq!(cmap.lookup(0x41).as_deref(), Some("A"));
        assert_eq!(cmap.lookup(0x42).as_deref(), Some("B"));
    }

    #[test]
    fn empty_input_is_an_empty_map() {
        let cmap = ToUnicodeCMap::parse(b"");
        assert!(cmap.is_empty());
        assert_eq!(cmap.lookup(0), None);
    }

    #[test]
    fn garbage_input_does_not_panic_or_hang() {
        for junk in [
            &b"beginbfchar beginbfrange endbfchar endbfrange"[..],
            b"<> <> beginbfchar <> <> endbfchar",
            b"1 beginbfrange <FFFFFFFFFF> <00> <00> endbfrange",
            b"[[[[[[[[[[",
            b"1 beginbfchar",
            b"\x00\x01\x02\xFF\xFE",
        ] {
            let _ = ToUnicodeCMap::parse(junk);
        }
    }

    #[test]
    fn odd_length_destination_is_counted() {
        // §9.10.3 N3: no validity rule exists for the UTF-16BE bytes.
        // <004> is an odd-digit hex string, which §7.3.4.3 pads to
        // <0040> — so use an explicitly odd BYTE count instead.
        let cmap = ToUnicodeCMap::parse(b"1 beginbfchar <41> <414243> endbfchar");
        assert_eq!(cmap.stats().malformed_destinations, 1);
        assert!(cmap.lookup(0x41).is_some(), "the decodable prefix survives");
    }

    #[test]
    fn code_width_is_taken_from_the_source_string_length() {
        // <41> is code 0x41; <0041> is code 0x0041 — the SAME numeric
        // value, but a one-byte and a two-byte code respectively. The
        // map is keyed on the value; width belongs to the font.
        let cmap = ToUnicodeCMap::parse(b"1 beginbfchar <0041> <0058> endbfchar");
        assert_eq!(cmap.lookup(0x41).as_deref(), Some("X"));
    }
}
