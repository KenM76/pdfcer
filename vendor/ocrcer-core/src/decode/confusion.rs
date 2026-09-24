//! Context priors for the pairs the matcher cannot separate on shape.
//!
//! # Contract
//!
//! [`Confusions::parse`] validates the `confusions` table once at load, and
//! refuses a table whose context list differs from [`CONTEXTS`] — a
//! reordered list would silently turn one test into another, which is the
//! same failure mode a positional charset has.
//!
//! After that, [`Confusions::adjust`] is total and cheap: given a class and a
//! bitmask of the contexts holding at this position, it returns the log2
//! adjustment to add to that class's score. A class with no rules, or a
//! position with no contexts, returns exactly zero.
//!
//! # What the numbers mean, and what they may not do
//!
//! Every value here came from an authored pair rule that was split evenly
//! between the two members, so the adjustments for any single context sum to
//! zero across all classes. A rule can therefore say "in this context, prefer
//! this one *of the two*" and can never say "prefer this one generally". It
//! is added to a score the matcher already produced, so a confident match
//! outvotes it — the same relationship the lexicon bonus has to the match
//! term under `CLAUDE.md` rule 6.
//!
//! Nothing here is derived at runtime and no logarithm is taken; the values
//! are stored and added, which is what keeps the decoder identical on x86 and
//! wasm32.

/// The magic the builder writes.
const MAGIC: &[u8; 4] = b"CNFS";
/// The layout this reader understands.
const VERSION: u16 = 1;
const HEADER: usize = 12;
/// Bytes per stored adjustment.
const ENTRY: usize = 8;

/// The contexts, in bit order. Bit `i` of a context mask is `CONTEXTS[i]`.
///
/// Mirrored in `ocrcer_build::confusions::CONTEXTS`. The duplication is made
/// safe by [`Confusions::parse`] comparing the two, which is the only way two
/// copies of an ordered list can be allowed to exist (`CLAUDE.md` rule 4).
pub const CONTEXTS: [&str; 7] = [
    "digit_neighbour",
    "letter_neighbour",
    "upper_run",
    "word_start",
    "word_end",
    "identifier",
    "lexicon_word",
];

/// Bit for `digit_neighbour`: the adjacent character reads as a digit.
pub const CTX_DIGIT_NEIGHBOUR: u8 = 1 << 0;
/// Bit for `letter_neighbour`: the adjacent character reads as a letter.
pub const CTX_LETTER_NEIGHBOUR: u8 = 1 << 1;
/// Bit for `upper_run`: the preceding character reads as an uppercase letter.
pub const CTX_UPPER_RUN: u8 = 1 << 2;
/// Bit for `word_start`: this is the word's first character.
pub const CTX_WORD_START: u8 = 1 << 3;
/// Bit for `word_end`: this is the word's last character.
pub const CTX_WORD_END: u8 = 1 << 4;
/// Bit for `identifier`: the word is identifier-shaped, so the lexicon term is
/// suppressed inside it.
pub const CTX_IDENTIFIER: u8 = 1 << 5;
/// Bit for `lexicon_word`: the path so far is a live prefix in the lexicon.
pub const CTX_LEXICON_WORD: u8 = 1 << 6;

/// A loaded confusion table.
#[derive(Debug, Clone, Default)]
pub struct Confusions {
    /// Sorted by `(class, context)`; parallel to `adjust`.
    key: Vec<u32>,
    adjust: Vec<f32>,
}

/// Why a `confusions` table could not be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Malformed {
    Magic,
    Version,
    Truncated,
    /// The table's context list is not this build's.
    Contexts,
    /// Entries are not in ascending `(class, context)` order, or name a
    /// context bit this build does not have.
    Order,
    /// An adjustment is not a finite number.
    Value,
}

impl Confusions {
    /// An empty table. Every lookup returns zero, which is what a model built
    /// before this table existed should behave like.
    pub fn empty() -> Confusions {
        Confusions::default()
    }

    /// Reads and fully validates the table.
    pub fn parse(bytes: &[u8]) -> Result<Confusions, Malformed> {
        if bytes.len() < HEADER {
            return Err(Malformed::Truncated);
        }
        if &bytes[..4] != MAGIC {
            return Err(Malformed::Magic);
        }
        if u16::from_le_bytes([bytes[4], bytes[5]]) != VERSION {
            return Err(Malformed::Version);
        }
        let n_contexts = u16::from_le_bytes([bytes[6], bytes[7]]) as usize;
        let count = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]) as usize;
        if n_contexts != CONTEXTS.len() {
            return Err(Malformed::Contexts);
        }

        let mut p = HEADER;
        for want in CONTEXTS {
            if p >= bytes.len() {
                return Err(Malformed::Truncated);
            }
            let len = bytes[p] as usize;
            p += 1;
            if p + len > bytes.len() {
                return Err(Malformed::Truncated);
            }
            if &bytes[p..p + len] != want.as_bytes() {
                return Err(Malformed::Contexts);
            }
            p += len;
        }
        if bytes.len() < p + count * ENTRY {
            return Err(Malformed::Truncated);
        }

        let mut key = Vec::with_capacity(count);
        let mut adjust = Vec::with_capacity(count);
        for i in 0..count {
            let e = &bytes[p + i * ENTRY..p + (i + 1) * ENTRY];
            let class = u16::from_le_bytes([e[0], e[1]]);
            let context = e[2] as usize;
            if context >= CONTEXTS.len() {
                return Err(Malformed::Order);
            }
            let v = f32::from_le_bytes([e[4], e[5], e[6], e[7]]);
            if !v.is_finite() {
                return Err(Malformed::Value);
            }
            let k = (u32::from(class) << 8) | context as u32;
            if i > 0 && k <= key[i - 1] {
                return Err(Malformed::Order);
            }
            key.push(k);
            adjust.push(v);
        }
        Ok(Confusions { key, adjust })
    }

    /// How many `(class, context)` adjustments the table holds.
    pub fn len(&self) -> usize {
        self.key.len()
    }

    /// Whether the table carries nothing.
    pub fn is_empty(&self) -> bool {
        self.key.is_empty()
    }

    /// The total log2 adjustment for `class` under the contexts in `mask`.
    ///
    /// Contexts are additive: a glyph that is both between digits and inside
    /// an identifier collects both arguments. That is deliberate — they are
    /// separate pieces of evidence, and a rule that only fired for the
    /// strongest one would make the table's total effect depend on which
    /// other contexts happened to be present.
    pub fn adjust(&self, class: u16, mask: u8) -> f32 {
        if mask == 0 || self.key.is_empty() {
            return 0.0;
        }
        let lo_key = u32::from(class) << 8;
        let mut lo = 0usize;
        let mut hi = self.key.len();
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            if self.key[mid] < lo_key {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        let mut total = 0.0f32;
        let mut i = lo;
        while i < self.key.len() && self.key[i] >> 8 == u32::from(class) {
            let bit = 1u8 << (self.key[i] & 0xff) as u8;
            if mask & bit != 0 {
                total += self.adjust[i];
            }
            i += 1;
        }
        total
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two classes, one context rule each, split zero-sum by hand.
    fn table(entries: &[(u16, u8, f32)]) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(MAGIC);
        b.extend_from_slice(&VERSION.to_le_bytes());
        b.extend_from_slice(&(CONTEXTS.len() as u16).to_le_bytes());
        b.extend_from_slice(&(entries.len() as u32).to_le_bytes());
        for name in CONTEXTS {
            b.push(name.len() as u8);
            b.extend_from_slice(name.as_bytes());
        }
        for (class, ctx, v) in entries {
            b.extend_from_slice(&class.to_le_bytes());
            b.push(*ctx);
            b.push(0);
            b.extend_from_slice(&v.to_le_bytes());
        }
        b
    }

    #[test]
    fn a_class_collects_only_the_contexts_that_hold() {
        let t = table(&[(3, 0, 0.8), (3, 6, -0.7), (9, 0, -0.8)]);
        let c = Confusions::parse(&t).expect("parses");
        assert_eq!(c.adjust(3, CTX_DIGIT_NEIGHBOUR), 0.8);
        assert_eq!(c.adjust(3, CTX_LEXICON_WORD), -0.7);
        assert_eq!(c.adjust(3, CTX_DIGIT_NEIGHBOUR | CTX_LEXICON_WORD), 0.8 - 0.7);
        assert_eq!(c.adjust(3, CTX_WORD_START), 0.0);
        assert_eq!(c.adjust(9, CTX_DIGIT_NEIGHBOUR), -0.8);
        // A class with no rules at all.
        assert_eq!(c.adjust(4, 0xff), 0.0);
    }

    #[test]
    fn no_context_means_no_adjustment() {
        let t = table(&[(3, 0, 0.8), (9, 0, -0.8)]);
        let c = Confusions::parse(&t).expect("parses");
        assert_eq!(c.adjust(3, 0), 0.0);
        assert_eq!(Confusions::empty().adjust(3, 0xff), 0.0);
    }

    #[test]
    fn a_damaged_or_mismatched_table_is_refused_rather_than_misread() {
        let good = table(&[(3, 0, 0.8), (9, 0, -0.8)]);
        let mut bad = good.clone();
        bad[0] = b'X';
        assert_eq!(Confusions::parse(&bad).err(), Some(Malformed::Magic));
        let mut bad = good.clone();
        bad[4] = 9;
        assert_eq!(Confusions::parse(&bad).err(), Some(Malformed::Version));
        // A context list of a different length.
        let mut bad = good.clone();
        bad[6] = 3;
        assert_eq!(Confusions::parse(&bad).err(), Some(Malformed::Contexts));
        // A context list of the right length but a different order: corrupt
        // the first stored name.
        let mut bad = good.clone();
        bad[HEADER + 1] = b'X';
        assert_eq!(Confusions::parse(&bad).err(), Some(Malformed::Contexts));
        assert_eq!(Confusions::parse(&good[..HEADER + 2]).err(), Some(Malformed::Truncated));
    }

    #[test]
    fn entries_out_of_order_are_refused() {
        let t = table(&[(9, 0, -0.8), (3, 0, 0.8)]);
        assert_eq!(Confusions::parse(&t).err(), Some(Malformed::Order));
    }
}
