//! Lexicon membership and prefix lookup over the compiled word graph.
//!
//! # Contract
//!
//! [`Lexicon::parse`] validates the `lexicon` table's bytes once at load and
//! then never fails: every offset in the graph is range-checked up front, so
//! traversal is index arithmetic with no error path. A malformed table is a
//! load-time refusal, not a wrong answer later.
//!
//! Traversal is by [`Cursor`], which advances one class index at a time and
//! reports, at each step, whether the path so far is a word ([`Cursor::tier`])
//! and whether any word continues it ([`Cursor::alive`]). Those are the two
//! questions the decoder asks per lattice edge, and both are a binary search
//! inside one node.
//!
//! **Symbols are case-folded class indices**, folded by the builder through
//! the charset's `case_twin` column. A caller holding an uppercase class must
//! fold it with [`Lexicon::fold`] before stepping, or `INVOICE` will not be
//! found in a graph that stores `invoice`.
//!
//! # What this is not
//!
//! It is not a spell checker and it cannot rewrite anything. Its entire
//! output is a tier, and the decoder turns a tier into a *bonus* (`CLAUDE.md`
//! rule 6). Absence from the graph has no representation here at all — there
//! is no penalty to return — which is the property that keeps `M8x1.25`
//! intact.

/// The magic the builder writes.
const MAGIC: &[u8; 4] = b"LXDW";
/// The graph layout this reader understands.
const VERSION: u16 = 1;
/// Fixed fields before `node_off`.
const HEADER: usize = 16;

/// A compiled word graph, owning its bytes.
#[derive(Debug, Clone)]
pub struct Lexicon {
    node_off: Vec<u32>,
    edge_target: Vec<u32>,
    edge_symbol: Vec<u16>,
    node_flags: Vec<u8>,
    /// Class index to case-folded class index, indexed by class.
    fold: Vec<u16>,
}

/// Why a `lexicon` table could not be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Malformed {
    Magic,
    Version,
    Truncated,
    /// An edge range is not ascending, or runs past the edge arrays.
    NodeRange,
    /// An edge names a node that does not exist.
    EdgeTarget,
    /// Edges inside a node are not sorted ascending by symbol, so a binary
    /// search over them would miss.
    EdgeOrder,
}

impl Lexicon {
    /// Reads and fully validates the table.
    ///
    /// `n_classes` is the charset size; a symbol at or above it is refused
    /// here rather than range-checked on every traversal step.
    pub fn parse(bytes: &[u8], n_classes: usize) -> Result<Lexicon, Malformed> {
        if bytes.len() < HEADER {
            return Err(Malformed::Truncated);
        }
        if &bytes[..4] != MAGIC {
            return Err(Malformed::Magic);
        }
        if u16::from_le_bytes([bytes[4], bytes[5]]) != VERSION {
            return Err(Malformed::Version);
        }
        let n_nodes = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]) as usize;
        let n_edges = u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]) as usize;
        if n_nodes == 0 {
            return Err(Malformed::Truncated);
        }

        let off_end = HEADER + 4 * (n_nodes + 1);
        let target_end = off_end + 4 * n_edges;
        let symbol_end = target_end + 2 * n_edges;
        let flags_end = symbol_end + n_nodes;
        if bytes.len() < flags_end {
            return Err(Malformed::Truncated);
        }

        let node_off: Vec<u32> = bytes[HEADER..off_end]
            .chunks_exact(4)
            .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect();
        let edge_target: Vec<u32> = bytes[off_end..target_end]
            .chunks_exact(4)
            .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect();
        let edge_symbol: Vec<u16> = bytes[target_end..symbol_end]
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        let node_flags = bytes[symbol_end..flags_end].to_vec();

        if node_off[n_nodes] as usize != n_edges {
            return Err(Malformed::NodeRange);
        }
        for i in 0..n_nodes {
            let (lo, hi) = (node_off[i] as usize, node_off[i + 1] as usize);
            if lo > hi || hi > n_edges {
                return Err(Malformed::NodeRange);
            }
            for e in lo..hi {
                if edge_target[e] as usize >= n_nodes {
                    return Err(Malformed::EdgeTarget);
                }
                if edge_symbol[e] as usize >= n_classes {
                    return Err(Malformed::EdgeTarget);
                }
                if e > lo && edge_symbol[e] <= edge_symbol[e - 1] {
                    return Err(Malformed::EdgeOrder);
                }
            }
        }

        let mut fold = Vec::new();
        fold.resize(n_classes, 0u16);
        for (i, f) in fold.iter_mut().enumerate() {
            *f = i as u16;
        }
        Ok(Lexicon { node_off, edge_target, edge_symbol, node_flags, fold })
    }

    /// Installs the case-fold map, one entry per class.
    ///
    /// Separate from [`parse`](Lexicon::parse) because the map comes from the
    /// charset in `meta` and the graph comes from a table; the loader has
    /// both and this reader has neither.
    pub fn set_fold(&mut self, fold: Vec<u16>) {
        if fold.len() == self.fold.len() {
            self.fold = fold;
        }
    }

    /// The case-folded form of a class index. Out-of-range classes fold to
    /// themselves.
    pub fn fold(&self, class: u16) -> u16 {
        self.fold.get(class as usize).copied().unwrap_or(class)
    }

    /// How many nodes the graph holds. A diagnostic, not a word count.
    pub fn nodes(&self) -> usize {
        self.node_flags.len()
    }

    /// A cursor at the root, before any symbol.
    pub fn root(&self) -> Cursor<'_> {
        Cursor { lex: self, node: Some(0) }
    }

    /// The node a cursor at the root would be at, for a caller that wants to
    /// carry the traversal as a plain value.
    ///
    /// A beam search holds thousands of partial paths and copies them between
    /// lattice nodes; a borrowed [`Cursor`] would tie every one of them to the
    /// graph's lifetime for no benefit, since the node index is the whole of
    /// the state. [`Lexicon::step_node`] and [`Lexicon::tier_at`] are the same
    /// walk over that plain value.
    pub fn root_node(&self) -> Option<u32> {
        Some(0)
    }

    /// Advances a plain node by one class index, case-folding it first.
    ///
    /// `None` in gives `None` out: a path that has already left the graph
    /// stays out, so a caller extending a dead path needs no check.
    pub fn step_node(&self, node: Option<u32>, class: u16) -> Option<u32> {
        let folded = self.fold(class);
        node.and_then(|n| self.step(n, folded))
    }

    /// The tier of the word a plain node spells, or `None` when it spells no
    /// whole word.
    pub fn tier_at(&self, node: Option<u32>) -> Option<u8> {
        let n = node?;
        let flags = *self.node_flags.get(n as usize)?;
        if flags & 1 == 0 {
            None
        } else {
            Some(((flags >> 1) & 0b111) + 1)
        }
    }

    fn step(&self, node: u32, symbol: u16) -> Option<u32> {
        let (mut lo, mut hi) = (self.node_off[node as usize], self.node_off[node as usize + 1]);
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            let s = self.edge_symbol[mid as usize];
            if s < symbol {
                lo = mid + 1;
            } else if s > symbol {
                hi = mid;
            } else {
                return Some(self.edge_target[mid as usize]);
            }
        }
        None
    }
}

/// A position in the graph.
#[derive(Debug, Clone, Copy)]
pub struct Cursor<'a> {
    lex: &'a Lexicon,
    /// `None` once a symbol left the graph. A dead cursor stays dead, so a
    /// caller extending a path that already failed does not have to check.
    node: Option<u32>,
}

impl<'a> Cursor<'a> {
    /// Advances by one class index, case-folding it first.
    pub fn step(self, class: u16) -> Cursor<'a> {
        let folded = self.lex.fold(class);
        Cursor { lex: self.lex, node: self.node.and_then(|n| self.lex.step(n, folded)) }
    }

    /// Whether any word in the graph begins with the path walked so far.
    pub fn alive(&self) -> bool {
        self.node.is_some()
    }

    /// The tier of the word the path spells, or `None` when the path is not
    /// itself a word.
    ///
    /// Tier 1 is the closed-class core and 5 is a rare domain term; the
    /// decoder scales its bonus by this, so the number says how much evidence
    /// membership provides rather than whether the string is allowed.
    pub fn tier(&self) -> Option<u8> {
        let n = self.node?;
        let flags = self.lex.node_flags[n as usize];
        if flags & 1 == 0 {
            None
        } else {
            Some(((flags >> 1) & 0b111) + 1)
        }
    }
}

/// Whether a whole class sequence is a word, and at what tier.
pub fn lookup(lex: &Lexicon, classes: &[u16]) -> Option<u8> {
    let mut cur = lex.root();
    for &c in classes {
        cur = cur.step(c);
        if !cur.alive() {
            return None;
        }
    }
    cur.tier()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds the same byte layout `ocrcer-build`'s `lexicon::build` writes,
    /// from a plain trie — small enough to check by hand, and the point is to
    /// exercise the reader rather than to re-implement the minimiser.
    fn graph(words: &[(&[u16], u8)]) -> Vec<u8> {
        #[derive(Default, Clone)]
        struct N {
            terminal: bool,
            tier: u8,
            edges: Vec<(u16, u32)>,
        }
        let mut pool: Vec<N> = vec![N::default()];
        for (syms, tier) in words {
            let mut at = 0usize;
            for &s in *syms {
                match pool[at].edges.iter().find(|e| e.0 == s) {
                    Some(&(_, t)) => at = t as usize,
                    None => {
                        let id = pool.len() as u32;
                        pool.push(N::default());
                        pool[at].edges.push((s, id));
                        at = id as usize;
                    }
                }
            }
            pool[at].terminal = true;
            pool[at].tier = *tier;
        }
        for n in pool.iter_mut() {
            n.edges.sort();
        }

        let n_edges: usize = pool.iter().map(|n| n.edges.len()).sum();
        let mut b = Vec::new();
        b.extend_from_slice(MAGIC);
        b.extend_from_slice(&VERSION.to_le_bytes());
        b.extend_from_slice(&0u16.to_le_bytes());
        b.extend_from_slice(&(pool.len() as u32).to_le_bytes());
        b.extend_from_slice(&(n_edges as u32).to_le_bytes());
        let mut off = 0u32;
        for n in &pool {
            b.extend_from_slice(&off.to_le_bytes());
            off += n.edges.len() as u32;
        }
        b.extend_from_slice(&off.to_le_bytes());
        for n in &pool {
            for &(_, t) in &n.edges {
                b.extend_from_slice(&t.to_le_bytes());
            }
        }
        for n in &pool {
            for &(s, _) in &n.edges {
                b.extend_from_slice(&s.to_le_bytes());
            }
        }
        for n in &pool {
            b.push(u8::from(n.terminal) | (n.tier.saturating_sub(1) << 1));
        }
        b
    }

    fn sample() -> Lexicon {
        // 1,2,3 = "cat"; 1,2,3,4 = "cats"; 1,5 = "co"
        let bytes = graph(&[(&[1, 2, 3], 2), (&[1, 2, 3, 4], 3), (&[1, 5], 1)]);
        Lexicon::parse(&bytes, 16).expect("hand-built graph parses")
    }

    #[test]
    fn a_word_in_the_graph_reports_its_tier() {
        let lex = sample();
        assert_eq!(lookup(&lex, &[1, 2, 3]), Some(2));
        assert_eq!(lookup(&lex, &[1, 2, 3, 4]), Some(3));
        assert_eq!(lookup(&lex, &[1, 5]), Some(1));
    }

    #[test]
    fn a_prefix_is_alive_without_being_a_word() {
        let lex = sample();
        let cur = lex.root().step(1).step(2);
        assert!(cur.alive());
        assert_eq!(cur.tier(), None);
    }

    /// The property the decoder leans on: once a path leaves the graph it
    /// stays out, so extending it needs no extra check.
    #[test]
    fn a_dead_cursor_stays_dead() {
        let lex = sample();
        let cur = lex.root().step(9);
        assert!(!cur.alive());
        let cur = cur.step(1).step(2).step(3);
        assert!(!cur.alive());
        assert_eq!(cur.tier(), None);
    }

    #[test]
    fn a_word_not_in_the_graph_reports_nothing_rather_than_a_penalty() {
        let lex = sample();
        assert_eq!(lookup(&lex, &[1, 2, 9]), None);
        assert_eq!(lookup(&lex, &[7]), None);
        // And the empty path is not a word.
        assert_eq!(lookup(&lex, &[]), None);
    }

    #[test]
    fn case_folding_is_applied_on_every_step() {
        let mut lex = sample();
        let mut fold: Vec<u16> = (0..16u16).collect();
        fold[11] = 1; // class 11 is the uppercase twin of class 1
        lex.set_fold(fold);
        assert_eq!(lookup(&lex, &[11, 2, 3]), Some(2));
    }

    #[test]
    fn a_damaged_table_is_refused_rather_than_misread() {
        let good = graph(&[(&[1, 2], 1)]);
        assert!(Lexicon::parse(&good[..8], 16).is_err());
        let mut bad = good.clone();
        bad[0] = b'X';
        assert_eq!(Lexicon::parse(&bad, 16).err(), Some(Malformed::Magic));
        let mut bad = good.clone();
        bad[4] = 9;
        assert_eq!(Lexicon::parse(&bad, 16).err(), Some(Malformed::Version));
        // A symbol outside the charset would index a class that does not
        // exist, so it is caught here and not on a traversal step.
        assert!(Lexicon::parse(&good, 2).is_err());
    }
}
