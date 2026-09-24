//! Two-pass union-find connected-component labelling.
//!
//! General-purpose: used by the feature extractor's hole count and, from
//! chunk 2 onward, by page-level component detection. Per `CLAUDE.md` rule
//! 4 this is the only labeller in the crate.

/// Which neighbours count as adjacent when grouping same-value pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Connectivity {
    /// North, south, east, west.
    Four,
    /// The four plus the diagonals.
    Eight,
}

/// Labels connected components of the nonzero pixels in `mask`.
///
/// `mask` is row-major, `width * height` bytes; any nonzero byte counts as
/// foreground, `0` as background. `connectivity` selects which neighbours
/// count as adjacent for foreground pixels.
///
/// Returns one label per pixel (background pixels hold `0`; each foreground
/// component holds a distinct label in `1..=count`, assigned in
/// first-encounter row-major order) and `count`, the number of components.
/// A `width == 0 || height == 0` mask returns an empty label vector and a
/// count of `0`.
///
/// # Panics
/// Panics if `mask.len() != width as usize * height as usize`.
pub fn label(mask: &[u8], width: u32, height: u32, connectivity: Connectivity) -> (Vec<u32>, u32) {
    assert_eq!(
        mask.len(),
        width as usize * height as usize,
        "mask length must equal width*height"
    );
    let w = width as usize;
    let h = height as usize;
    let mut labels = vec![0u32; w * h];
    if w == 0 || h == 0 {
        return (labels, 0);
    }

    // parent[0] is an unused sentinel; real provisional labels start at 1.
    let mut parent: Vec<u32> = vec![0];

    for y in 0..h {
        for x in 0..w {
            let idx = y * w + x;
            if mask[idx] == 0 {
                continue;
            }
            let mut neighbors: [u32; 4] = [0; 4];
            let mut n = 0;
            if x > 0 && labels[idx - 1] != 0 {
                neighbors[n] = labels[idx - 1];
                n += 1;
            }
            if y > 0 && labels[idx - w] != 0 {
                neighbors[n] = labels[idx - w];
                n += 1;
            }
            if connectivity == Connectivity::Eight {
                if y > 0 && x > 0 && labels[idx - w - 1] != 0 {
                    neighbors[n] = labels[idx - w - 1];
                    n += 1;
                }
                if y > 0 && x + 1 < w && labels[idx - w + 1] != 0 {
                    neighbors[n] = labels[idx - w + 1];
                    n += 1;
                }
            }
            if n == 0 {
                let new_label = parent.len() as u32;
                parent.push(new_label);
                labels[idx] = new_label;
            } else {
                let min_label = neighbors[..n].iter().copied().min().unwrap();
                labels[idx] = min_label;
                for &l in &neighbors[..n] {
                    union(&mut parent, l, min_label);
                }
            }
        }
    }

    for l in labels.iter_mut() {
        if *l != 0 {
            *l = find(&mut parent, *l);
        }
    }

    let mut remap: Vec<u32> = vec![0; parent.len()];
    let mut next_id = 1u32;
    for l in labels.iter_mut() {
        let root = *l;
        if root != 0 {
            if remap[root as usize] == 0 {
                remap[root as usize] = next_id;
                next_id += 1;
            }
            *l = remap[root as usize];
        }
    }
    (labels, next_id - 1)
}

/// One connected component: its label, its bounding box, how much ink it
/// holds, and how solid each of its four bounding-box sides is.
///
/// `x1`/`y1` are **exclusive**, so `x1 - x0` is the width. Every consumer of
/// this downstream does width arithmetic and none does an inclusive-range
/// walk, so exclusive is the form that never needs a `+ 1`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Component {
    pub label: u32,
    pub x0: u32,
    pub y0: u32,
    pub x1: u32,
    pub y1: u32,
    /// Ink pixels in the component, which is not `width * height` for
    /// anything but a solid rectangle.
    pub area: u32,
    /// For each of the four bounding-box sides, the fraction of that side's
    /// pixel positions this component inks within a band of
    /// `t = max(1, round(height/12))` px from the edge. Order: top, bottom,
    /// left, right (see [`Component::border_top`] etc).
    ///
    /// `ARCHITECTURE.md` section 11, 2026-09-23 ("Checkbox drop, first
    /// detector: falsified at screening; a border-coverage signal is added
    /// to `Component`"): a drawn rectangle's sides are (near-)fully inked,
    /// while a rounded glyph (`o`, `0`, `O`, `D`) misses its corners and
    /// scores clearly lower on at least one side, which a bounding box and
    /// an ink count alone cannot tell apart. Computed once, here, during
    /// labelling; it is an internal `Component` field, not a feature-vector
    /// dimension, and it does not touch the charset or the `.ocrw` format.
    pub border_coverage: [f32; 4],
}

impl Component {
    pub fn width(&self) -> u32 {
        self.x1 - self.x0
    }
    pub fn height(&self) -> u32 {
        self.y1 - self.y0
    }
    /// Horizontal centre, in half-pixel units doubled to stay integral.
    pub fn cx2(&self) -> u32 {
        self.x0 + self.x1
    }
    pub fn overlaps_x(&self, other: &Component) -> bool {
        self.x0 < other.x1 && other.x0 < self.x1
    }
    /// Height of the vertical overlap between two components, zero when they
    /// do not overlap.
    pub fn overlap_y(&self, other: &Component) -> u32 {
        self.y1.min(other.y1).saturating_sub(self.y0.max(other.y0))
    }

    pub fn border_top(&self) -> f32 {
        self.border_coverage[0]
    }
    pub fn border_bottom(&self) -> f32 {
        self.border_coverage[1]
    }
    pub fn border_left(&self) -> f32 {
        self.border_coverage[2]
    }
    pub fn border_right(&self) -> f32 {
        self.border_coverage[3]
    }
    /// The least-covered of the four sides -- what a "drawn box on every
    /// side" test reads.
    pub fn border_min(&self) -> f32 {
        self.border_coverage.iter().copied().fold(f32::INFINITY, f32::min)
    }
}

/// Bounding boxes, ink counts and border coverage for every component,
/// indexed by `label - 1`, so `components(...)[i].label == i as u32 + 1`.
///
/// Labels are assigned in first-encounter row-major order, so this vector is
/// in a fixed order for a given mask and no sort is needed to make a fixture
/// reproducible.
///
/// Two passes over the mask: the first finds each component's bounding box
/// (needed before border coverage can be measured, since the band width `t`
/// in [`Component::border_coverage`]'s doc is itself a function of a
/// component's own height); the second walks every ink pixel again and, for
/// each of the four sides, records which position along that side (a column
/// for top/bottom, a row for left/right) the pixel falls under. Both passes
/// are `O(width * height)` and touch no pixel outside the mask, so this
/// stays the integer, single-threaded-friendly cost `ARCHITECTURE.md`
/// section 4.1 budgets for.
pub fn components(labels: &[u32], width: u32, height: u32, count: u32) -> Vec<Component> {
    let w = width as usize;
    let h = height as usize;
    let mut out: Vec<Component> = (1..=count)
        .map(|label| Component {
            label,
            x0: u32::MAX,
            y0: u32::MAX,
            x1: 0,
            y1: 0,
            area: 0,
            border_coverage: [0.0; 4],
        })
        .collect();
    for y in 0..h {
        for x in 0..w {
            let l = labels[y * w + x];
            if l == 0 {
                continue;
            }
            let c = &mut out[(l - 1) as usize];
            c.x0 = c.x0.min(x as u32);
            c.y0 = c.y0.min(y as u32);
            c.x1 = c.x1.max(x as u32 + 1);
            c.y1 = c.y1.max(y as u32 + 1);
            c.area += 1;
        }
    }
    if out.is_empty() {
        return out;
    }

    // `top_seen`/`bottom_seen` are indexed by column offset from `x0` (size
    // = width); `left_seen`/`right_seen` by row offset from `y0` (size =
    // height). A position is "seen" once any of this component's own pixels
    // falls in that side's band, so coverage is the count of `true`s over
    // the side's own length.
    let mut top_seen: Vec<Vec<bool>> = out.iter().map(|c| vec![false; c.width() as usize]).collect();
    let mut bottom_seen: Vec<Vec<bool>> =
        out.iter().map(|c| vec![false; c.width() as usize]).collect();
    let mut left_seen: Vec<Vec<bool>> =
        out.iter().map(|c| vec![false; c.height() as usize]).collect();
    let mut right_seen: Vec<Vec<bool>> =
        out.iter().map(|c| vec![false; c.height() as usize]).collect();

    for y in 0..h {
        for x in 0..w {
            let l = labels[y * w + x];
            if l == 0 {
                continue;
            }
            let idx = (l - 1) as usize;
            let c = &out[idx];
            let t = ((c.height() as f32) / 12.0).round().max(1.0) as u32;
            let (xu, yu) = (x as u32, y as u32);
            if yu - c.y0 < t {
                top_seen[idx][(xu - c.x0) as usize] = true;
            }
            if c.y1 - 1 - yu < t {
                bottom_seen[idx][(xu - c.x0) as usize] = true;
            }
            if xu - c.x0 < t {
                left_seen[idx][(yu - c.y0) as usize] = true;
            }
            if c.x1 - 1 - xu < t {
                right_seen[idx][(yu - c.y0) as usize] = true;
            }
        }
    }

    let count_true = |v: &[bool]| v.iter().filter(|&&b| b).count() as f32;
    for (idx, c) in out.iter_mut().enumerate() {
        let w_f = c.width().max(1) as f32;
        let h_f = c.height().max(1) as f32;
        c.border_coverage = [
            count_true(&top_seen[idx]) / w_f,
            count_true(&bottom_seen[idx]) / w_f,
            count_true(&left_seen[idx]) / h_f,
            count_true(&right_seen[idx]) / h_f,
        ];
    }
    out
}

/// Labels a mask and returns its components in one call, the form every
/// page-level caller wants.
pub fn find_components(mask: &[u8], width: u32, height: u32, connectivity: Connectivity) -> Vec<Component> {
    let (labels, count) = label(mask, width, height, connectivity);
    components(&labels, width, height, count)
}

fn find(parent: &mut [u32], mut x: u32) -> u32 {
    while parent[x as usize] != x {
        parent[x as usize] = parent[parent[x as usize] as usize];
        x = parent[x as usize];
    }
    x
}

fn union(parent: &mut [u32], a: u32, b: u32) {
    let ra = find(parent, a);
    let rb = find(parent, b);
    if ra != rb {
        if ra < rb {
            parent[rb as usize] = ra;
        } else {
            parent[ra as usize] = rb;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_mask_has_no_components() {
        let (labels, count) = label(&[], 0, 0, Connectivity::Eight);
        assert!(labels.is_empty());
        assert_eq!(count, 0);
    }

    #[test]
    fn solid_block_is_one_component() {
        let mask = vec![1u8; 9];
        let (labels, count) = label(&mask, 3, 3, Connectivity::Four);
        assert_eq!(count, 1);
        assert!(labels.iter().all(|&l| l == 1));
    }

    #[test]
    fn two_separate_blocks_get_two_labels() {
        // 5x1: 1 1 0 1 1
        let mask = [1u8, 1, 0, 1, 1];
        let (labels, count) = label(&mask, 5, 1, Connectivity::Four);
        assert_eq!(count, 2);
        assert_eq!(labels[0], labels[1]);
        assert_eq!(labels[3], labels[4]);
        assert_ne!(labels[0], labels[3]);
    }

    #[test]
    #[should_panic(expected = "mask length must equal width*height")]
    fn mismatched_length_panics() {
        let _ = label(&[1, 0, 1], 2, 2, Connectivity::Four);
    }

    /// Diagonal-touching pixels merge under 8-connectivity and stay separate
    /// under 4-connectivity, which is the whole reason the hole count in
    /// `feature.rs` must fix background at 4-connected: mixing it up with a
    /// uniform choice changes which background region counts as enclosed.
    #[test]
    fn diagonal_pixels_differ_by_connectivity() {
        #[rustfmt::skip]
        let mask = [
            1u8, 0,
            0,   1,
        ];
        let (_, count_four) = label(&mask, 2, 2, Connectivity::Four);
        let (_, count_eight) = label(&mask, 2, 2, Connectivity::Eight);
        assert_eq!(count_four, 2);
        assert_eq!(count_eight, 1);
    }

    /// The concrete case that matters for hole counting: a 1-thick ring
    /// with its top-left wall corner notched out so the interior touches
    /// the border-adjacent corner pixel only diagonally. Background must be
    /// labelled 4-connected (per ARCHITECTURE.md section 3.1) so the
    /// interior stays a separate, non-border-touching component; the
    /// "naive" uniform 8-connected choice merges it into the border
    /// component and loses the hole entirely.
    #[test]
    fn connectivity_choice_changes_hole_topology() {
        #[rustfmt::skip]
        let ink = [
            0u8, 1, 1, 1,
            1,   0, 0, 1,
            1,   0, 0, 1,
            1,   1, 1, 1,
        ];
        let bg: Vec<u8> = ink.iter().map(|&v| if v == 0 { 1 } else { 0 }).collect();

        let (labels_four, count_four) = label(&bg, 4, 4, Connectivity::Four);
        assert_eq!(count_four, 2, "isolated corner pixel + interior 2x2");
        // (0,0) is background, index 0; interior top-left is (1,1), index 5.
        assert_ne!(labels_four[0], labels_four[5]);

        let (labels_eight, count_eight) = label(&bg, 4, 4, Connectivity::Eight);
        assert_eq!(
            count_eight, 1,
            "8-connected background wrongly merges the corner into the interior"
        );
        assert_eq!(labels_eight[0], labels_eight[5]);
    }

    /// A solid rectangle inks every position on every side: border coverage
    /// is `1.0` on all four sides regardless of `t`.
    #[test]
    fn solid_rectangle_has_full_border_coverage_on_every_side() {
        let mask = vec![1u8; 8 * 12];
        let (labels, count) = label(&mask, 8, 12, Connectivity::Eight);
        assert_eq!(count, 1);
        let comps = components(&labels, 8, 12, count);
        assert_eq!(comps[0].border_coverage, [1.0, 1.0, 1.0, 1.0]);
    }

    /// A hollow ring with its four corners notched out -- the signature a
    /// rounded glyph (`o`, `0`) leaves at the pixel level -- inks every
    /// column/row of its 1px-wide sides except the two corner positions, so
    /// every side scores below full coverage. Confirms
    /// `ARCHITECTURE.md` section 11, 2026-09-23 ("Checkbox drop, first
    /// detector..."): a drawn box's straight sides and a ring's rounded
    /// ones are distinguishable at the `Component` level once border
    /// coverage exists, which bounding box and ink count alone could not
    /// do.
    #[test]
    fn a_ring_with_notched_corners_misses_full_coverage_on_every_side() {
        // 12x12: a 1px ring (row/col 0 and 11 inked, interior empty), with
        // all four corner pixels forced to background. `t = max(1,
        // round(12/12)) = 1`, so the band is exactly that outer ring.
        let n = 12usize;
        let mut mask = vec![0u8; n * n];
        for i in 0..n {
            mask[i] = 1; // top row
            mask[(n - 1) * n + i] = 1; // bottom row
            mask[i * n] = 1; // left col
            mask[i * n + (n - 1)] = 1; // right col
        }
        for &(x, y) in &[(0usize, 0usize), (n - 1, 0), (0, n - 1), (n - 1, n - 1)] {
            mask[y * n + x] = 0;
        }
        let (labels, count) = label(&mask, n as u32, n as u32, Connectivity::Eight);
        assert_eq!(count, 1, "the notched ring must still be one component");
        let comps = components(&labels, n as u32, n as u32, count);
        let want = (n - 2) as f32 / n as f32; // 10 of 12 positions inked
        for (side, cov) in ["top", "bottom", "left", "right"].iter().zip(comps[0].border_coverage) {
            assert_eq!(cov, want, "{side} side");
        }
        assert!(comps[0].border_min() < 0.85, "must miss a checkbox-shaped threshold");
    }
}
