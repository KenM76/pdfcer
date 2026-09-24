//! The character-segmentation lattice: every way a word's ink might divide
//! into characters, as a DAG the decoder searches.
//!
//! # Contract
//!
//! [`build`] turns one word into a [`Lattice`] whose nodes are candidate cut
//! positions across the word and whose edges are candidate character images —
//! the plain single-component cuts, merge candidates for broken glyphs and
//! multi-part glyphs (`i`, `j`, `=`, `:`, `%`, accented letters), and split
//! candidates at vertical-projection minima inside wide components
//! (`ARCHITECTURE.md` section 5). [`crop`] turns one edge into the bitmap the
//! feature extractor takes.
//!
//! This stage proposes; it does not decide. Deciding is the decoder's, which
//! is why an edge carries no score here and why the lattice is built wide:
//! an edge the matcher can reject is cheap, and a cut that was never offered
//! is unrecoverable.
//!
//! Deterministic: cut positions are integers, candidate ordering is by
//! `(from, to)`, and the valley search breaks ties to the earliest cut — the
//! rule `ARCHITECTURE.md` section 8.2 states for segmentation.

use crate::feature::GlyphInput;
use crate::image::components::Component;
use crate::layout::lines::TextLine;
use crate::layout::words::WordSpan;

/// Lattice-construction parameters.
#[derive(Debug, Clone, Copy)]
pub struct Params {
    /// How many adjacent atoms a merge candidate may span. Three: `%` is
    /// three pieces, and a glyph broken by a thin scan is rarely more.
    pub max_merge: usize,
    /// A merge candidate wider than this many x-heights is not one
    /// character and is not offered.
    ///
    /// **A guess, on chunk 8's tuning list.** A wide capital `M` or `W` runs
    /// to about 1.4 x-heights in the faces in the bank; the margin above
    /// that is for the accented and multi-part cases.
    pub max_merge_x_heights: f32,
    /// An atom narrower than this many x-heights is one character and is not
    /// searched for interior cuts.
    ///
    /// **A guess, on chunk 8's tuning list**, and the cheaper direction to
    /// err is low: offering a split that the matcher rejects costs time,
    /// while not offering one costs the word.
    pub split_min_x_heights: f32,
    /// Most interior cuts offered inside one atom.
    pub max_splits: usize,
    /// How far below the atom's mean ink-column height a column must fall to
    /// count as a valley.
    ///
    /// **A guess, on chunk 8's tuning list.** Touching characters in print
    /// meet at a ligature-thin bridge, so the true cut column carries a small
    /// fraction of the mean.
    pub valley_fraction: f32,
    /// Narrowest piece an interior cut may leave, in x-heights, measured from
    /// either end of the atom.
    ///
    /// **A guess, on chunk 8's tuning list.** A period is about a fifth of an
    /// x-height wide and is the narrowest thing in the charset.
    pub min_piece_x_heights: f32,
    /// How much of the narrower participant's own width two components'
    /// column overlap must cover before [`atoms`] glues them into one atom,
    /// beyond the always-merge case of one being fully inside the other's
    /// column range.
    ///
    /// **A guess, on chunk 8's tuning list.** `0` reproduces the legacy
    /// any-overlap rule exactly (`ARCHITECTURE.md` section 11, 2026-09-23,
    /// "Worst page named": serif crossbar/base-serif strokes chain
    /// pixel-disjoint letters at this setting).
    pub merge_overlap_frac: f32,
}

/// The shipped values, taken from the parameter block rather than restated.
///
/// The block in [`crate::params`] is the single definition of every knob, and
/// it is what `model/params.tsv` is asserted against at build time. A second
/// copy here would be a second definition that nothing compares.
impl Default for Params {
    fn default() -> Self {
        crate::params::Params::DEFAULT.segment()
    }
}

/// What kind of hypothesis an edge is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeKind {
    /// One atom, taken whole: the ordinary case.
    Single,
    /// Several adjacent atoms taken as one character.
    Merge,
    /// Part of one atom, bounded by at least one interior cut.
    Split,
}

/// One candidate character image, as a span of the word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Edge {
    pub from: usize,
    pub to: usize,
    /// Left cut position, page x, inclusive.
    pub x0: u32,
    /// Right cut position, page x, exclusive.
    pub x1: u32,
    pub kind: EdgeKind,
}

/// The DAG for one word.
///
/// `positions` is ascending and `positions[0]`/`positions[last]` are the
/// word's edges, so node `0` is the start and node `positions.len() - 1` is
/// the end. Every edge goes strictly left to right, so the graph is acyclic
/// by construction and needs no cycle check.
#[derive(Debug, Clone)]
pub struct Lattice {
    pub positions: Vec<u32>,
    pub edges: Vec<Edge>,
    /// The line band edges are cropped to, so a neighbouring line's descender
    /// cannot leak into a glyph.
    pub y0: u32,
    pub y1: u32,
    /// The labels belonging to this word. Ink from anything else in the
    /// cropping window is not this word's.
    pub labels: Vec<u32>,
    /// Node index bracketing each atom: atom `k` occupies
    /// `atom_boundary[k]..atom_boundary[k + 1]`. Length `atoms.len() + 1`,
    /// empty for an empty word. [`crop`] uses this to find which atoms an
    /// edge spans.
    atom_boundary: Vec<usize>,
    /// Each atom's own member labels, sorted, aligned with `atom_boundary`.
    /// Kept separate from `labels` (the whole word) so [`crop`] can restrict
    /// a piece to its own members rather than the whole word's ink — see
    /// `crop`'s doc comment.
    atom_labels: Vec<Vec<u32>>,
}

impl Lattice {
    pub fn start(&self) -> usize {
        0
    }
    pub fn end(&self) -> usize {
        self.positions.len() - 1
    }
    /// Edges leaving `node`, in `(from, to)` order.
    pub fn from(&self, node: usize) -> impl Iterator<Item = &Edge> {
        self.edges.iter().filter(move |e| e.from == node)
    }
}

/// A cropped candidate glyph: tight ink plus where it sat on the page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Glyph {
    pub ink: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub x: u32,
    pub y: u32,
}

impl Glyph {
    /// This glyph as the feature extractor's input, on `line`.
    pub fn input<'a>(&'a self, line: &TextLine) -> GlyphInput<'a> {
        GlyphInput {
            ink: &self.ink,
            width: self.width,
            height: self.height,
            baseline_dy: line.baseline_dy(self.y),
            x_height: line.x_height,
        }
    }
}

/// Builds the lattice for one word with the default parameters.
pub fn build(
    word: &WordSpan,
    components: &[Component],
    labels: &[u32],
    page_width: u32,
    line: &TextLine,
) -> Lattice {
    build_with(word, components, labels, page_width, line, &Params::default())
}

/// Builds the lattice for one word.
pub fn build_with(
    word: &WordSpan,
    components: &[Component],
    labels: &[u32],
    page_width: u32,
    line: &TextLine,
    p: &Params,
) -> Lattice {
    let mut word_labels: Vec<u32> = word.members.iter().map(|&i| components[i].label).collect();
    word_labels.sort_unstable();

    let atoms = atoms(word, components, p.merge_overlap_frac);
    if atoms.is_empty() {
        return Lattice {
            positions: vec![word.x0, word.x1.max(word.x0 + 1)],
            edges: Vec::new(),
            y0: line.y0,
            y1: line.y1,
            labels: word_labels,
            atom_boundary: Vec::new(),
            atom_labels: Vec::new(),
        };
    }

    // Node positions: the word's left edge, then for each atom its interior
    // cuts followed by its right edge. `boundary[k]` is atom k's left node
    // and `boundary[k + 1]` its right node.
    let mut positions = vec![atoms[0].0];
    let mut boundary = vec![0usize];
    let mut atom_labels: Vec<Vec<u32>> = Vec::with_capacity(atoms.len());
    for &(ax0, ax1, ref members) in &atoms {
        for x in interior_cuts(ax0, ax1, members, components, labels, page_width, line, p) {
            positions.push(x);
        }
        positions.push(ax1);
        boundary.push(positions.len() - 1);
        let mut labs: Vec<u32> = members.iter().map(|&i| components[i].label).collect();
        labs.sort_unstable();
        atom_labels.push(labs);
    }

    let max_merge_w = f64::from(line.x_height) * f64::from(p.max_merge_x_heights);
    let mut edges: Vec<Edge> = Vec::new();

    // Whole atoms, and runs of them.
    for i in 0..atoms.len() {
        for j in i + 1..=(i + p.max_merge).min(atoms.len()) {
            let (x0, x1) = (atoms[i].0, atoms[j - 1].1);
            let kind = if j == i + 1 { EdgeKind::Single } else { EdgeKind::Merge };
            if kind == EdgeKind::Merge && f64::from(x1 - x0) > max_merge_w {
                continue;
            }
            edges.push(Edge { from: boundary[i], to: boundary[j], x0, x1, kind });
        }
    }

    // Pieces of one atom, across its interior cuts.
    for k in 0..atoms.len() {
        let (lo, hi) = (boundary[k], boundary[k + 1]);
        for a in lo..hi {
            for b in a + 1..=hi {
                if a == lo && b == hi {
                    continue; // already offered as the whole atom
                }
                edges.push(Edge {
                    from: a,
                    to: b,
                    x0: positions[a],
                    x1: positions[b],
                    kind: EdgeKind::Split,
                });
            }
        }
    }

    edges.sort_by(|a, b| a.from.cmp(&b.from).then(a.to.cmp(&b.to)).then(a.x0.cmp(&b.x0)));
    edges.dedup_by(|a, b| a.from == b.from && a.to == b.to);

    Lattice {
        positions,
        edges,
        y0: line.y0,
        y1: line.y1,
        labels: word_labels,
        atom_boundary: boundary,
        atom_labels,
    }
}

/// Groups a word's members into x-disjoint atoms.
///
/// Components whose boxes overlap horizontally cannot be separated by a
/// vertical cut, so a real overlap has to become one atom -- an `i` and its
/// dot, a kerned pair whose boxes interleave. But gluing on *any* overlap
/// also chains pixel-disjoint neighbouring letters in a serif face whose
/// crossbar and base-serif strokes cross another letter's column range by a
/// few pixels while never touching in ink (`ARCHITECTURE.md` section 11,
/// 2026-09-23, "Worst page named": `t`'s crossbar and `h`'s base serif
/// overlap by 2 columns while sitting 12 rows apart). `merge_overlap_frac`
/// narrows that to a fractional test -- see [`should_merge`] -- while an
/// always-merge case stays: either component fully inside the other's
/// column range (an `i`'s dot inside its stem, a `:` or `;`'s two dots, and
/// `=`, whose two equal-width bars fully overlap). Returns
/// `(x0, x1, members)` left to right.
fn atoms(
    word: &WordSpan,
    components: &[Component],
    merge_overlap_frac: f32,
) -> Vec<(u32, u32, Vec<usize>)> {
    let mut out: Vec<(u32, u32, Vec<usize>)> = Vec::new();
    for &i in &word.members {
        let c = &components[i];
        let merge = match out.last() {
            Some(a) => should_merge(a.0, a.1, c.x0, c.x1, merge_overlap_frac),
            None => false,
        };
        if merge {
            let a = out.last_mut().expect("just matched Some(a) above");
            a.1 = a.1.max(c.x1);
            a.2.push(i);
        } else {
            out.push((c.x0, c.x1, vec![i]));
        }
    }
    out
}

/// Whether an incoming component at columns `(c0, c1)` joins the running
/// atom whose extent so far is `(a0, a1)`.
///
/// Compared against the atom's whole running extent, not only its most
/// recently added member: that is what the legacy any-overlap rule compared
/// `c.x0` against, so `frac = 0` reproduces it exactly (below, `overlap`
/// reduces to `c.x0 < a.1` once `word.members` is x0-sorted, which every
/// caller here guarantees -- `a.x0 <= c.x0` always holds, so the max in the
/// overlap computation is always `c.x0`). It also does not need to be a
/// last-member comparison in practice: the overlap is bounded by the
/// extent's right edge and the new component's left edge, so widening the
/// atom leftward (from an earlier merge) never changes the overlap or the
/// narrower-of-the-two-widths figure the fraction is measured against.
fn should_merge(a0: u32, a1: u32, c0: u32, c1: u32, frac: f32) -> bool {
    if (c0 <= a0 && c1 >= a1) || (a0 <= c0 && a1 >= c1) {
        return true; // one fully inside the other's column range
    }
    let overlap = a1.min(c1).saturating_sub(a0.max(c0));
    if overlap == 0 {
        return false;
    }
    let narrow = (a1 - a0).min(c1 - c0);
    if narrow == 0 {
        return true; // degenerate zero-width box; already excluded above otherwise
    }
    f64::from(overlap) >= f64::from(frac) * f64::from(narrow)
}

/// Interior cut positions inside one atom, ascending.
///
/// Empty when the atom is narrow enough to be one character. Otherwise the
/// deepest valleys in the atom's ink-column profile, subject to leaving a
/// piece of usable width at either end.
#[allow(clippy::too_many_arguments)]
fn interior_cuts(
    ax0: u32,
    ax1: u32,
    members: &[usize],
    components: &[Component],
    labels: &[u32],
    page_width: u32,
    line: &TextLine,
    p: &Params,
) -> Vec<u32> {
    let width = ax1 - ax0;
    let split_min = f64::from(line.x_height) * f64::from(p.split_min_x_heights);
    if f64::from(width) < split_min {
        return Vec::new();
    }
    let mut allowed: Vec<u32> = members.iter().map(|&i| components[i].label).collect();
    allowed.sort_unstable();

    let profile = column_profile(labels, page_width, &allowed, ax0, ax1, line.y0, line.y1);
    let total: u64 = profile.iter().map(|&v| u64::from(v)).sum();
    if total == 0 {
        return Vec::new();
    }
    let mean = total as f64 / profile.len() as f64;
    let ceiling = mean * f64::from(p.valley_fraction);

    let margin = (f64::from(line.x_height) * f64::from(p.min_piece_x_heights)).max(1.0) as u32;
    if width <= margin * 2 {
        return Vec::new();
    }

    // A candidate is a column at or below the ceiling that no neighbour
    // beats. `<=` on the left and `<` on the right makes the leftmost column
    // of a flat valley floor the candidate, which is the "earliest cut" tie
    // rule stated in section 8.2.
    let mut cands: Vec<(u32, u32)> = Vec::new(); // (height, x)
    for i in 1..profile.len() - 1 {
        let x = ax0 + i as u32;
        if x < ax0 + margin || x + margin > ax1 {
            continue;
        }
        let v = profile[i];
        if f64::from(v) <= ceiling && v <= profile[i - 1] && v < profile[i + 1] {
            cands.push((v, x));
        }
    }
    cands.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    cands.truncate(p.max_splits);
    let mut out: Vec<u32> = cands.into_iter().map(|(_, x)| x).collect();
    out.sort_unstable();
    out
}

/// Ink pixels per column in `x0..x1`, counting only pixels whose label is in
/// `allowed` and whose row is in `y0..y1`.
fn column_profile(
    labels: &[u32],
    page_width: u32,
    allowed: &[u32],
    x0: u32,
    x1: u32,
    y0: u32,
    y1: u32,
) -> Vec<u32> {
    let w = page_width as usize;
    let mut out = vec![0u32; (x1 - x0) as usize];
    for y in y0..y1 {
        let row = y as usize * w;
        for x in x0..x1 {
            let l = labels[row + x as usize];
            if l != 0 && allowed.binary_search(&l).is_ok() {
                out[(x - x0) as usize] += 1;
            }
        }
    }
    out
}

/// The label set an edge may draw ink from: the union of the member labels
/// of every atom the edge spans, found from [`Lattice::atom_boundary`] by
/// interval overlap against `(from, to)`.
///
/// Restricted to the spanning atoms rather than the whole word's `labels`
/// because, once atoms are allowed to keep overlapping column ranges without
/// merging (`merge_overlap_frac > 0`), a plain column crop against every
/// label in the word would still pick up a neighbouring un-merged atom's
/// pixels that happen to fall in this edge's `x0..x1` window -- exactly the
/// serif-kerning contamination named in `ARCHITECTURE.md` section 11,
/// 2026-09-23 ("Worst page named"): a separated `t` cropped by column range
/// alone would still carry `h`'s base-serif columns. At `merge_overlap_frac
/// = 0`, atoms are x-disjoint by construction (as the legacy code already
/// relied on), so this is provably identical to the old whole-word filter:
/// no other atom's component can have ink inside this edge's `x0..x1`.
fn edge_labels(lat: &Lattice, from: usize, to: usize) -> Vec<u32> {
    if lat.atom_boundary.len() < 2 {
        return lat.labels.clone();
    }
    let mut out: Vec<u32> = Vec::new();
    for m in 0..lat.atom_boundary.len() - 1 {
        let (b0, b1) = (lat.atom_boundary[m], lat.atom_boundary[m + 1]);
        if b0 < to && b1 > from {
            out.extend_from_slice(&lat.atom_labels[m]);
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// Crops one edge into a tight glyph bitmap.
///
/// `None` when the cut window holds no ink of this word — an edge that spans
/// only a gap. The bitmap is `1` for ink and `0` for background, which is
/// what [`crate::feature::extract`] takes.
pub fn crop(lat: &Lattice, labels: &[u32], page_width: u32, e: &Edge) -> Option<Glyph> {
    let allowed = edge_labels(lat, e.from, e.to);
    let w = page_width as usize;
    let (mut gx0, mut gy0, mut gx1, mut gy1) = (u32::MAX, u32::MAX, 0u32, 0u32);
    for y in lat.y0..lat.y1 {
        let row = y as usize * w;
        for x in e.x0..e.x1 {
            let l = labels[row + x as usize];
            if l != 0 && allowed.binary_search(&l).is_ok() {
                gx0 = gx0.min(x);
                gy0 = gy0.min(y);
                gx1 = gx1.max(x + 1);
                gy1 = gy1.max(y + 1);
            }
        }
    }
    if gx1 == 0 && gy1 == 0 {
        return None;
    }

    let (gw, gh) = ((gx1 - gx0) as usize, (gy1 - gy0) as usize);
    let mut ink = vec![0u8; gw * gh];
    for y in gy0..gy1 {
        let row = y as usize * w;
        for x in gx0..gx1 {
            let l = labels[row + x as usize];
            if l != 0 && allowed.binary_search(&l).is_ok() {
                ink[(y - gy0) as usize * gw + (x - gx0) as usize] = 1;
            }
        }
    }
    Some(Glyph { ink, width: gw as u32, height: gh as u32, x: gx0, y: gy0 })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::components::{find_components, Connectivity};
    use crate::layout::{lines, words};

    /// Renders a mask from a row of `#`/`.` strings, one char per pixel.
    fn mask_of(rows: &[&str]) -> (Vec<u8>, u32, u32) {
        let h = rows.len() as u32;
        let w = rows[0].len() as u32;
        let mut m = Vec::with_capacity((w * h) as usize);
        for r in rows {
            assert_eq!(r.len() as u32, w);
            for ch in r.chars() {
                m.push(u8::from(ch == '#'));
            }
        }
        (m, w, h)
    }

    /// The whole stage end to end on a tiny page: label, group, split, build.
    fn lattice_of(rows: &[&str]) -> (Lattice, Vec<u32>, u32, lines::TextLine) {
        let (mask, w, h) = mask_of(rows);
        let (labels, count) = crate::image::components::label(&mask, w, h, Connectivity::Eight);
        let comps = crate::image::components::components(&labels, w, h, count);
        let mut ls = lines::group_with(
            &comps,
            w,
            h,
            // These fixtures are a few pixels tall, so the page-furniture
            // rule — which is about proportions of a real page — would throw
            // every one of them away. Checkbox dropping is a lines-layer
            // concern unrelated to what this helper tests (edge cropping,
            // lattice connectivity); some fixtures draw a small hollow
            // rectangle to exercise ink cropping and it must not be evicted
            // as a false-positive checkbox.
            &lines::Params {
                min_area: 1,
                furniture_fraction: 1.0,
                checkbox_drop: false,
                ..lines::Params::default()
            },
        );
        assert_eq!(ls.len(), 1, "fixture must be one line");
        let line = ls.remove(0);
        // Pinned for the same reason the line params above are: these
        // fixtures have a 3px x-height, so a one-pixel inter-letter gap is
        // already a third of one and the shipped ratios call it a space.
        // Both the ceiling and the no-valley fallback have to be pinned:
        // either one alone still splits these boxes.
        // That is an artefact of the fixture's scale, not a claim about
        // printed text -- and a test of edge cropping must not be steerable
        // by a word-splitting sweep.
        let ws = words::split_with(
            &line,
            &comps,
            &words::Params {
                lone_gap_x_heights: 2.0,
                no_valley_x_heights: 2.0,
                ..words::Params::default()
            },
        );
        assert_eq!(ws.len(), 1, "fixture must be one word");
        let lat = build(&ws[0], &comps, &labels, w, &line);
        (lat, labels, w, line)
    }

    /// Two separated letters: two atoms, so a single edge each plus the
    /// merge that spans them.
    #[test]
    fn separated_letters_are_single_edges_plus_a_merge() {
        let (lat, _, _, _) = lattice_of(&[
            "#..#..#..#",
            "#..#..#..#",
            "#..#..#..#",
            "#..#..#..#",
            "#..#..#..#",
        ]);
        let singles = lat.edges.iter().filter(|e| e.kind == EdgeKind::Single).count();
        assert_eq!(singles, 4, "one whole-atom edge per atom");
        assert!(lat.edges.iter().any(|e| e.kind == EdgeKind::Merge));
        assert!(lat.edges.iter().all(|e| e.from < e.to), "the lattice must be acyclic");
    }

    /// The case merges exist for: a glyph broken into two pieces by a thin
    /// scan. The decoder needs an edge that spans both.
    #[test]
    fn a_broken_glyph_has_an_edge_that_spans_its_pieces() {
        let (lat, _, _, _) = lattice_of(&["##.##", "##.##", "#####", "##.##", "##.##"]);
        let spans_all = lat
            .edges
            .iter()
            .any(|e| e.x0 == 0 && e.x1 == 5 && matches!(e.kind, EdgeKind::Single | EdgeKind::Merge));
        assert!(spans_all, "no edge covers the whole broken glyph");
    }

    /// Touching characters are one component, so only an interior cut can
    /// separate them. The bridge column is the valley the cut has to find.
    #[test]
    fn touching_characters_get_an_interior_cut_at_the_bridge() {
        // Two 5-wide blocks joined by a one-pixel bridge on the middle row.
        let (lat, _, _, line) = lattice_of(&[
            "#####.......#####",
            "#####.......#####",
            "#################",
            "#####.......#####",
            "#####.......#####",
        ]);
        assert!(line.x_height > 0.0);
        assert!(
            lat.positions.len() > 2,
            "a wide atom with a valley must offer an interior cut, got {:?}",
            lat.positions
        );
        assert!(lat.edges.iter().any(|e| e.kind == EdgeKind::Split));
    }

    /// A cut may not shave a sliver off the end: every piece it leaves must
    /// be wide enough to be a character.
    #[test]
    fn interior_cuts_leave_usable_pieces_at_both_ends() {
        let (lat, _, _, line) = lattice_of(&[
            "#####.......#####",
            "#####.......#####",
            "#################",
            "#####.......#####",
            "#####.......#####",
        ]);
        let margin = (f64::from(line.x_height) * 0.2).max(1.0) as u32;
        let (lo, hi) = (lat.positions[0], *lat.positions.last().unwrap());
        for &x in &lat.positions[1..lat.positions.len() - 1] {
            assert!(x >= lo + margin && x + margin <= hi, "cut at {x} leaves a sliver");
        }
    }

    /// The cropped bitmap is tight, holds only this word's ink, and lands
    /// where the page says it does.
    #[test]
    fn cropping_an_edge_gives_tight_ink_at_the_right_place() {
        let (lat, labels, w, line) = lattice_of(&[
            "..........",
            "..###..##.",
            "..#.#..##.",
            "..###..##.",
            "..........",
        ]);
        let e = lat.edges.iter().find(|e| e.kind == EdgeKind::Single).unwrap();
        let g = crop(&lat, &labels, w, e).unwrap();
        assert_eq!((g.width, g.height), (3, 3));
        assert_eq!((g.x, g.y), (2, 1));
        assert_eq!(g.ink, vec![1, 1, 1, 1, 0, 1, 1, 1, 1]);
        // And it presents to the extractor with the line's own metrics.
        let gi = g.input(&line);
        assert_eq!(gi.baseline_dy, line.baseline - 1.0);
        assert_eq!(gi.x_height, line.x_height);
    }

    /// Every edge must be reachable from the start and reach the end, or the
    /// decoder would have paths that dead-end mid-word.
    #[test]
    fn the_lattice_is_connected_end_to_end() {
        let (lat, _, _, _) = lattice_of(&[
            "#..#..#####.......#####",
            "#..#..#####.......#####",
            "#..#..#################",
            "#..#..#####.......#####",
            "#..#..#####.......#####",
        ]);
        let n = lat.positions.len();
        let mut reach = vec![false; n];
        reach[0] = true;
        for i in 0..n {
            if reach[i] {
                for e in lat.from(i) {
                    reach[e.to] = true;
                }
            }
        }
        assert!(reach[lat.end()], "no path from the start of the word to its end");
    }

    #[test]
    fn an_empty_word_gives_an_empty_lattice() {
        let comps: Vec<Component> = Vec::new();
        let word = WordSpan { members: Vec::new(), x0: 0, y0: 0, x1: 0, y1: 0 };
        let line = lines::TextLine {
            members: Vec::new(),
            x0: 0,
            y0: 0,
            x1: 0,
            y1: 0,
            baseline: 0.0,
            x_height: 1.0,
            cap_height: 1.0,
            x_height_source: lines::XHeightSource::FromCapHeight,
            median_height: 1,
        };
        let lat = build(&word, &comps, &[], 0, &line);
        assert!(lat.edges.is_empty());
    }

    #[test]
    fn find_components_agrees_with_labelling_by_hand() {
        let (mask, w, h) = mask_of(&["#.#", "#.#"]);
        let cs = find_components(&mask, w, h, Connectivity::Eight);
        assert_eq!(cs.len(), 2);
        assert_eq!((cs[0].x0, cs[0].x1, cs[0].area), (0, 1, 2));
        assert_eq!((cs[1].x0, cs[1].x1, cs[1].area), (2, 3, 2));
    }

    // -- segment.merge_overlap_frac (`ARCHITECTURE.md` section 11,
    // 2026-09-23, "Worst page named") ------------------------------------

    /// An `i`'s dot inside its stem's column range merges regardless of the
    /// overlap fraction -- even one no ordinary two-letter overlap could
    /// ever satisfy.
    #[test]
    fn i_dot_merges_by_full_containment_regardless_of_overlap_fraction() {
        assert!(should_merge(3, 8, 4, 6, 1.0), "dot inside stem");
        assert!(should_merge(4, 6, 3, 8, 1.0), "stem inside dot, order-independent");
    }

    /// `=`'s two bars share the same column range exactly, which is "fully
    /// inside" in both directions at once -- checked explicitly per the
    /// task, since it is the one case where containment and equality
    /// coincide.
    #[test]
    fn equals_sign_bars_merge_by_full_containment() {
        assert!(should_merge(10, 20, 10, 20, 1.0));
    }

    /// The worked example from the worst-page diagnostic: `t` (43..53) and
    /// `h` (51..67) overlap by 2 of `t`'s 10 columns (a 0.2 fraction).
    /// Above that fraction they must not merge; at or below it, they still
    /// do.
    #[test]
    fn serif_kerning_overlap_below_threshold_does_not_merge() {
        assert!(!should_merge(43, 53, 51, 67, 0.5), "0.2 overlap fraction must not clear 0.5");
        // 0.15 rather than the fraction's own 0.2, to stay clear of f32
        // rounding at the exact boundary -- this asserts "still merges
        // comfortably below the threshold", not exact-equality inclusion.
        assert!(should_merge(43, 53, 51, 67, 0.15), "0.2 overlap fraction clears a lower threshold");
    }

    /// `merge_overlap_frac = 0` is the legacy any-overlap rule: any positive
    /// overlap merges, and no overlap at all never does, whatever the
    /// fraction.
    #[test]
    fn merge_overlap_frac_zero_reproduces_the_legacy_any_overlap_rule() {
        assert!(should_merge(43, 53, 51, 67, 0.0), "the same t/h pair merges at the legacy setting");
        assert!(!should_merge(0, 10, 10, 20, 0.0), "touching but not overlapping never merges");
    }

    /// The same `t`/`h` shape end to end: two components whose bounding
    /// boxes overlap by one column while their ink never touches (a
    /// crossbar row and a base-serif row, 3 rows apart), same as
    /// `ARCHITECTURE.md` section 11, 2026-09-23 ("Worst page named"). Above
    /// the overlap-fraction threshold this must produce two atoms, not one
    /// -- and critically, cropping each atom's own edge must carry only its
    /// own component's ink, never the neighbour's, even though the edges'
    /// `x0..x1` ranges still overlap by that one column.
    #[test]
    fn serif_kerning_atoms_do_not_merge_and_their_crops_do_not_cross_contaminate() {
        // Column 0..4 is a `t`-like stem-and-crossbar shape; column 3..7 is
        // an `h`-like stem-and-base-serif shape. Column 3 is shared by both
        // boxes but never by both letters' ink at the same row.
        let (mask, w, h) = mask_of(&[
            ".#....#...",
            "####..#...",
            ".#....#...",
            ".#....#...",
            ".#.####...",
        ]);
        let (labels, count) = crate::image::components::label(&mask, w, h, Connectivity::Eight);
        let comps = crate::image::components::components(&labels, w, h, count);
        assert_eq!(comps.len(), 2, "fixture must be two pixel-disjoint components");
        let (a, b) = (0usize, 1usize);
        assert_eq!((comps[a].x0, comps[a].x1), (0, 4), "the t-like shape");
        assert_eq!((comps[b].x0, comps[b].x1), (3, 7), "the h-like shape");

        let word = WordSpan { members: vec![a, b], x0: 0, y0: 0, x1: 7, y1: h };
        let line = lines::TextLine {
            members: vec![a, b],
            x0: 0,
            y0: 0,
            x1: 7,
            y1: h,
            baseline: (h - 1) as f32,
            x_height: h as f32,
            cap_height: h as f32,
            x_height_source: lines::XHeightSource::FromCapHeight,
            median_height: h,
        };

        let mut p = Params::default();
        p.merge_overlap_frac = 0.5; // above this fixture's 1/4 = 0.25 overlap
        let lat = build_with(&word, &comps, &labels, w, &line, &p);
        let singles: Vec<&Edge> = lat.edges.iter().filter(|e| e.kind == EdgeKind::Single).collect();
        assert_eq!(singles.len(), 2, "the overlap is below threshold: two atoms, not one");

        let mut total = 0u32;
        for e in &singles {
            let owner = if (e.x0, e.x1) == (comps[a].x0, comps[a].x1) {
                a
            } else if (e.x0, e.x1) == (comps[b].x0, comps[b].x1) {
                b
            } else {
                panic!("edge {e:?} does not match either component's own box");
            };
            let g = crop(&lat, &labels, w, e).expect("each atom has ink");
            let ink: u32 = g.ink.iter().map(|&v| u32::from(v)).sum();
            assert_eq!(
                ink, comps[owner].area,
                "atom {owner}'s crop must hold exactly its own component's pixels, \
                 not a neighbour's ink that happens to share its column range"
            );
            total += ink;
        }
        assert_eq!(total, comps[a].area + comps[b].area, "no pixel double-counted or dropped");
    }
}
