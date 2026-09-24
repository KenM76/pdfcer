//! Character-pair log-probabilities, with a category backoff.
//!
//! # Contract
//!
//! [`Bigrams::parse`] validates the `bigrams` table once at load; after that
//! [`Bigrams::logp`] is total — every pair of in-range symbols has an answer,
//! and no answer is negative infinity. That totality is the point: a bigram
//! the table never saw must be *discouraged* and never *forbidden*, or the
//! decoder could not spell a part number (`CLAUDE.md` rule 6 applied to the
//! bigram term).
//!
//! Symbols are class indices. [`Bigrams::boundary`] is the extra symbol
//! standing for the edge of a word: as a `prev` it means the word starts
//! here, as a `next` that it ends here.
//!
//! Values are **log base two**, so the decoder adds them. No logarithm is
//! taken at runtime — every value was computed by the builder and stored —
//! which is what keeps the decoder's arithmetic identical on x86 and wasm32.

/// The magic the builder writes.
const MAGIC: &[u8; 4] = b"BGRM";
/// The layout this reader understands.
const VERSION: u16 = 1;
const HEADER: usize = 16;

/// The value a pair with no probability takes. Matches the builder's floor.
pub const LOG2_FLOOR: f32 = -40.0;

/// A loaded bigram table.
#[derive(Debug, Clone)]
pub struct Bigrams {
    row_off: Vec<u32>,
    col: Vec<u16>,
    logp: Vec<f32>,
    row_backoff: Vec<f32>,
    cat_logp: Vec<f32>,
    cat_of: Vec<u8>,
    n_cats: usize,
}

/// Why a `bigrams` table could not be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Malformed {
    Magic,
    Version,
    Truncated,
    /// A row's entry range is not ascending, or runs past the entry arrays.
    RowRange,
    /// A row names a category the table does not have.
    Category,
    /// The row count does not match the charset.
    RowCount,
}

impl Bigrams {
    /// Reads and fully validates the table.
    ///
    /// `n_classes` is the charset size; the table must hold exactly one row
    /// more than that, the extra one being the boundary.
    pub fn parse(bytes: &[u8], n_classes: usize) -> Result<Bigrams, Malformed> {
        if bytes.len() < HEADER {
            return Err(Malformed::Truncated);
        }
        if &bytes[..4] != MAGIC {
            return Err(Malformed::Magic);
        }
        if u16::from_le_bytes([bytes[4], bytes[5]]) != VERSION {
            return Err(Malformed::Version);
        }
        let n_cats = u16::from_le_bytes([bytes[6], bytes[7]]) as usize;
        let rows = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]) as usize;
        let entries = u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]) as usize;
        if rows != n_classes + 1 {
            return Err(Malformed::RowCount);
        }
        if n_cats == 0 || n_cats > 64 {
            return Err(Malformed::Category);
        }

        let off_end = HEADER + 4 * (rows + 1);
        let col_end = off_end + 2 * entries;
        let logp_end = col_end + 4 * entries;
        let backoff_end = logp_end + 4 * rows;
        let catlog_end = backoff_end + 4 * n_cats * n_cats;
        let catof_end = catlog_end + rows;
        if bytes.len() < catof_end {
            return Err(Malformed::Truncated);
        }

        let u32s = |r: core::ops::Range<usize>| -> Vec<u32> {
            bytes[r].chunks_exact(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect()
        };
        let f32s = |r: core::ops::Range<usize>| -> Vec<f32> {
            bytes[r].chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect()
        };

        let row_off = u32s(HEADER..off_end);
        let col: Vec<u16> = bytes[off_end..col_end]
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        let logp = f32s(col_end..logp_end);
        let row_backoff = f32s(logp_end..backoff_end);
        let cat_logp = f32s(backoff_end..catlog_end);
        let cat_of = bytes[catlog_end..catof_end].to_vec();

        if row_off[rows] as usize != entries {
            return Err(Malformed::RowRange);
        }
        for r in 0..rows {
            let (lo, hi) = (row_off[r] as usize, row_off[r + 1] as usize);
            if lo > hi || hi > entries {
                return Err(Malformed::RowRange);
            }
            for e in lo..hi {
                if col[e] as usize >= rows {
                    return Err(Malformed::RowRange);
                }
                if e > lo && col[e] <= col[e - 1] {
                    return Err(Malformed::RowRange);
                }
            }
            if cat_of[r] as usize >= n_cats {
                return Err(Malformed::Category);
            }
        }
        for v in row_backoff.iter().chain(cat_logp.iter()).chain(logp.iter()) {
            if !v.is_finite() {
                return Err(Malformed::RowRange);
            }
        }

        Ok(Bigrams { row_off, col, logp, row_backoff, cat_logp, cat_of, n_cats })
    }

    /// The symbol standing for the edge of a word.
    pub fn boundary(&self) -> u16 {
        (self.row_off.len() - 2) as u16
    }

    /// How many rows, which is the charset size plus the boundary.
    pub fn rows(&self) -> usize {
        self.cat_of.len()
    }

    /// `log2 P(next | prev)`.
    ///
    /// Out-of-range symbols return the floor rather than panicking: a caller
    /// holding a class the table does not cover is a bug worth reporting, but
    /// not one worth taking a page down for.
    pub fn logp(&self, prev: u16, next: u16) -> f32 {
        let rows = self.rows();
        if prev as usize >= rows || next as usize >= rows {
            return LOG2_FLOOR;
        }
        let (mut lo, mut hi) = (self.row_off[prev as usize], self.row_off[prev as usize + 1]);
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            let c = self.col[mid as usize];
            if c < next {
                lo = mid + 1;
            } else if c > next {
                hi = mid;
            } else {
                return self.logp[mid as usize];
            }
        }
        let cp = self.cat_of[prev as usize] as usize;
        let cn = self.cat_of[next as usize] as usize;
        self.row_backoff[prev as usize] + self.cat_logp[cp * self.n_cats + cn]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two classes and a boundary, one category each, with a single observed
    /// pair — small enough to check the arithmetic by hand.
    fn table() -> Vec<u8> {
        let rows = 3usize; // classes 0 and 1, boundary 2
        let n_cats = 2usize;
        let row_off: [u32; 4] = [0, 1, 1, 1];
        let col: [u16; 1] = [1];
        let logp: [f32; 1] = [-0.5];
        let row_backoff: [f32; 3] = [-3.0, 0.0, 0.0];
        let cat_logp: [f32; 4] = [-1.0, -2.0, -4.0, -8.0];
        let cat_of: [u8; 3] = [0, 1, 1];

        let mut b = Vec::new();
        b.extend_from_slice(MAGIC);
        b.extend_from_slice(&VERSION.to_le_bytes());
        b.extend_from_slice(&(n_cats as u16).to_le_bytes());
        b.extend_from_slice(&(rows as u32).to_le_bytes());
        b.extend_from_slice(&(col.len() as u32).to_le_bytes());
        for v in row_off {
            b.extend_from_slice(&v.to_le_bytes());
        }
        for v in col {
            b.extend_from_slice(&v.to_le_bytes());
        }
        for v in logp {
            b.extend_from_slice(&v.to_le_bytes());
        }
        for v in row_backoff {
            b.extend_from_slice(&v.to_le_bytes());
        }
        for v in cat_logp {
            b.extend_from_slice(&v.to_le_bytes());
        }
        b.extend_from_slice(&cat_of);
        b
    }

    #[test]
    fn an_observed_pair_reads_its_stored_value() {
        let t = Bigrams::parse(&table(), 2).expect("parses");
        assert_eq!(t.logp(0, 1), -0.5);
        assert_eq!(t.boundary(), 2);
    }

    #[test]
    fn an_unobserved_pair_takes_the_row_backoff_plus_its_category_cell() {
        let t = Bigrams::parse(&table(), 2).expect("parses");
        // prev 0 is category 0, next 0 is category 0: -3.0 + -1.0.
        assert_eq!(t.logp(0, 0), -4.0);
        // prev 1 is category 1, next 0 is category 0: 0.0 + -4.0.
        assert_eq!(t.logp(1, 0), -4.0);
    }

    /// The property the decoder depends on: nothing is impossible.
    #[test]
    fn no_pair_is_forbidden() {
        let t = Bigrams::parse(&table(), 2).expect("parses");
        for p in 0..3u16 {
            for n in 0..3u16 {
                assert!(t.logp(p, n).is_finite(), "{p} -> {n} was not finite");
            }
        }
        // And a symbol the table does not cover reports the floor rather
        // than panicking.
        assert_eq!(t.logp(9, 0), LOG2_FLOOR);
    }

    #[test]
    fn a_damaged_table_is_refused_rather_than_misread() {
        let good = table();
        let mut bad = good.clone();
        bad[0] = b'X';
        assert_eq!(Bigrams::parse(&bad, 2).err(), Some(Malformed::Magic));
        let mut bad = good.clone();
        bad[4] = 7;
        assert_eq!(Bigrams::parse(&bad, 2).err(), Some(Malformed::Version));
        assert_eq!(Bigrams::parse(&good, 5).err(), Some(Malformed::RowCount));
        assert!(Bigrams::parse(&good[..20], 2).is_err());
    }
}
