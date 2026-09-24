//! Stripping table rules and underlines out of over-wide components before
//! line grouping runs.
//!
//! # Contract
//!
//! [`strip_underlines`] takes a binarized mask and [`Params`] and returns the
//! mask's components after erasing straight rule/underline bands in place,
//! plus a [`RuleSegment`] for every band it erased.

use crate::image::components::Component;
use crate::layout::lines::Params;

/// One erased rule or underline band, in the page's own pixel coordinates
/// (`x1`/`y1` exclusive, the same convention [`Component`] uses).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuleSegment {
    pub x0: u32,
    pub x1: u32,
    pub y0: u32,
    pub y1: u32,
}

/// The result of [`strip_underlines`].
pub struct StripOutput {
    /// The modified mask's component labels.
    pub labels: Vec<u32>,
    /// The modified mask's components.
    pub components: Vec<Component>,
    /// Every rule or underline band this pass erased, for an output stage
    /// that wants to record underline formatting (`ARCHITECTURE.md` section
    /// 11, 2026-09-23, "Keep what was stripped"). Nothing consumes this yet.
    pub rule_segments: Vec<RuleSegment>,
}

/// Strips a table rule or an underline out of over-wide components, in
/// place, and re-labels what remains.
///
/// `ARCHITECTURE.md` section 11, 2026-09-23 ("Underline strip, second rule:
/// straight bands only, keep crossing strokes, drop strip debris, and keep
/// what was stripped"):
///
/// 1. Take the page's median height of glyphish components, `h`, and the run
///    floor `L = p.rule_run_heights * h`. Only a component whose width is at
///    least `L` is examined, same as before -- this is what keeps every
///    hyphen, minus sign, em dash and ordinary word out of this pass
///    entirely (see [`Params::rule_run_heights`]).
/// 2. **Bands, not rows.** Within an examined component, only a band of at
///    least 2 consecutive rows is erased, each row holding exactly one run
///    of the component's own ink at least `L` long, with the run's `x0`/`x1`
///    agreeing within 2px of the previous row in the band. A single
///    qualifying row, or a row holding more than one qualifying run, breaks
///    the band instead of joining it and is never erased.
/// 3. **Keep crossing strokes.** Within a band's x-range, a column's ink is
///    erased only where that column's own vertical run of ink -- the run
///    overlapping the band's rows, which may reach above or below them --
///    is no taller than the band's row count plus 1. A stroke that crosses
///    the rule (a `)`'s bowl, a box's vertical side) keeps every pixel.
/// 4. **Drop strip debris.** After the remaining pixels of every examined
///    component are re-labelled into sub-components (horizontal only: there
///    is no evidence yet of vertical rules touching text), a piece produced
///    by re-labelling a component this pass actually erased ink from --
///    entirely inside that component's original box -- is discarded if its
///    height exceeds `p.debris_heights * h`. A piece is also discarded, even
///    under that floor, when its width is `<= 0.5 * h` **and** its height
///    exceeds `p.thin_debris_heights * h` -- a box's left/right side left
///    behind by an erased top or bottom rule is exactly this shape
///    (`ARCHITECTURE.md` section 11, 2026-09-23, "Two mechanisms named").
///    This never touches an ordinary component that was never stripped.
///
/// Each accepted band from step 2 is recorded as a [`RuleSegment`] whether
/// or not step 3 kept some of its columns.
///
/// `mask` is modified in place; the returned labels and components describe
/// the modified mask, replacing what a caller would otherwise have labelled
/// directly. A no-op (returns the labels/components of the untouched mask,
/// no rule segments) when `p.rule_run_heights <= 0.0` or no component is
/// glyphish.
pub fn strip_underlines(mask: &mut [u8], width: u32, height: u32, p: &Params) -> StripOutput {
    let (labels0, count0) =
        crate::image::components::label(mask, width, height, crate::image::components::Connectivity::Eight);
    let comps0 = crate::image::components::components(&labels0, width, height, count0);

    let mut rule_segments: Vec<RuleSegment> = Vec::new();
    let mut stripped_boxes: Vec<(u32, u32, u32, u32)> = Vec::new();
    let mut h = 0.0f64;

    if p.rule_run_heights > 0.0 {
        let mut heights: Vec<u32> = comps0
            .iter()
            .filter(|c| super::lines::is_glyphish(c, width, height, p))
            .map(Component::height)
            .collect();
        if !heights.is_empty() {
            heights.sort_unstable();
            h = f64::from(heights[heights.len() / 2]);
            let run_floor = f64::from(p.rule_run_heights) * h;
            if run_floor > 0.0 {
                for c in &comps0 {
                    if f64::from(c.width()) >= run_floor {
                        let erased =
                            strip_component_bands(mask, &labels0, width, c, run_floor, &mut rule_segments);
                        if erased {
                            stripped_boxes.push((c.x0, c.y0, c.x1, c.y1));
                        }
                    }
                }
            }
        }
    }

    let (mut labels, mut comps) = {
        let (l, count) =
            crate::image::components::label(mask, width, height, crate::image::components::Connectivity::Eight);
        let c = crate::image::components::components(&l, width, height, count);
        (l, c)
    };

    if (p.debris_heights > 0.0 || p.thin_debris_heights > 0.0) && h > 0.0 && !stripped_boxes.is_empty() {
        let debris_floor = f64::from(p.debris_heights) * h;
        let thin_debris_floor = f64::from(p.thin_debris_heights) * h;
        let thin_width_ceiling = 0.5 * h;
        let mut any_dropped = false;
        for c in &comps {
            let from_strip = stripped_boxes
                .iter()
                .any(|&(sx0, sy0, sx1, sy1)| c.x0 >= sx0 && c.x1 <= sx1 && c.y0 >= sy0 && c.y1 <= sy1);
            if !from_strip {
                continue;
            }
            let height = f64::from(c.height());
            let is_debris = (p.debris_heights > 0.0 && height > debris_floor)
                || (p.thin_debris_heights > 0.0
                    && f64::from(c.width()) <= thin_width_ceiling
                    && height > thin_debris_floor);
            if is_debris {
                erase_component(mask, &labels, width, c);
                any_dropped = true;
            }
        }
        if any_dropped {
            let (l, count) =
                crate::image::components::label(mask, width, height, crate::image::components::Connectivity::Eight);
            comps = crate::image::components::components(&l, width, height, count);
            labels = l;
        }
    }

    StripOutput { labels, components: comps, rule_segments }
}

/// Sets every pixel `labels` attributes to `c` back to background.
fn erase_component(mask: &mut [u8], labels: &[u32], width: u32, c: &Component) {
    let w = width as usize;
    for y in c.y0..c.y1 {
        let row = y as usize * w;
        for x in c.x0..c.x1 {
            if labels[row + x as usize] == c.label {
                mask[row + x as usize] = 0;
            }
        }
    }
}

/// Erases the straight rule/underline bands within `c`, keeping any column
/// whose ink reaches past the band. Returns whether any pixel was erased.
///
/// Checked against `labels`, not `mask`, so a neighbouring component that
/// happens to share a row or an x-range inside this box is left alone: only
/// pixels this component's own label owns are candidates.
fn strip_component_bands(
    mask: &mut [u8],
    labels: &[u32],
    width: u32,
    c: &Component,
    run_floor: f64,
    rule_segments: &mut Vec<RuleSegment>,
) -> bool {
    let w = width as usize;

    // Group each row's single qualifying run (if any) into bands of >= 2
    // consecutive rows whose runs agree within 2px of the row before them.
    let mut bands: Vec<Vec<(u32, u32, u32)>> = Vec::new();
    let mut current: Vec<(u32, u32, u32)> = Vec::new();
    let flush = |current: &mut Vec<(u32, u32, u32)>, bands: &mut Vec<Vec<(u32, u32, u32)>>| {
        if current.len() >= 2 {
            bands.push(std::mem::take(current));
        } else {
            current.clear();
        }
    };
    for y in c.y0..c.y1 {
        match single_qualifying_run(labels, width, c, y, run_floor) {
            Some((x0, x1)) => {
                let joins = current
                    .last()
                    .is_some_and(|&(_, px0, px1)| (x0 as i64 - px0 as i64).abs() <= 2 && (x1 as i64 - px1 as i64).abs() <= 2);
                if !joins {
                    flush(&mut current, &mut bands);
                }
                current.push((y, x0, x1));
            }
            None => flush(&mut current, &mut bands),
        }
    }
    flush(&mut current, &mut bands);

    let mut erased = false;
    for band in &bands {
        let y0 = band.first().expect("flush only keeps bands with >= 2 rows").0;
        let y1 = band.last().expect("flush only keeps bands with >= 2 rows").0 + 1;
        let x0 = band.iter().map(|&(_, bx0, _)| bx0).min().expect("band is non-empty");
        let x1 = band.iter().map(|&(_, _, bx1)| bx1).max().expect("band is non-empty");
        rule_segments.push(RuleSegment { x0, x1, y0, y1 });

        let thickness = band.len() as u32;
        for x in x0..x1 {
            if let Some((ry0, ry1)) = column_ink_run(labels, width, c, x, y0, y1 - 1) {
                if ry1 - ry0 <= thickness + 1 {
                    for ry in ry0..ry1 {
                        mask[ry as usize * w + x as usize] = 0;
                    }
                    erased = true;
                }
            }
        }
    }
    erased
}

/// The one run of `c`'s own ink on row `y` that reaches `run_floor` pixels,
/// or `None` if there is no such run or more than one.
fn single_qualifying_run(labels: &[u32], width: u32, c: &Component, y: u32, run_floor: f64) -> Option<(u32, u32)> {
    let w = width as usize;
    let row = y as usize * w;
    let mut found = None;
    let mut x = c.x0;
    while x < c.x1 {
        if labels[row + x as usize] == c.label {
            let start = x;
            while x < c.x1 && labels[row + x as usize] == c.label {
                x += 1;
            }
            if f64::from(x - start) >= run_floor {
                if found.is_some() {
                    return None;
                }
                found = Some((start, x));
            }
        } else {
            x += 1;
        }
    }
    found
}

/// The run of `c`'s own ink in column `x`, within `c`'s own bounding box,
/// that overlaps rows `band_y0..=band_y1_incl` -- `None` if column `x` has
/// no such ink.
fn column_ink_run(
    labels: &[u32],
    width: u32,
    c: &Component,
    x: u32,
    band_y0: u32,
    band_y1_incl: u32,
) -> Option<(u32, u32)> {
    let w = width as usize;
    let mut y = c.y0;
    while y < c.y1 {
        if labels[y as usize * w + x as usize] == c.label {
            let start = y;
            while y < c.y1 && labels[y as usize * w + x as usize] == c.label {
                y += 1;
            }
            if start <= band_y1_incl && y > band_y0 {
                return Some((start, y));
            }
        } else {
            y += 1;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- `strip_underlines` -----------------------------------------------

    /// Turns `#`/`.` rows into a row-major mask, `(mask, width, height)`.
    fn mask_of(rows: &[&str]) -> (Vec<u8>, u32, u32) {
        let w = rows[0].len() as u32;
        let h = rows.len() as u32;
        let mut m = Vec::with_capacity((w * h) as usize);
        for r in rows {
            assert_eq!(r.len() as u32, w, "every row must be the same width");
            m.extend(r.chars().map(|ch| u8::from(ch == '#')));
        }
        (m, w, h)
    }

    fn strip_params(rule_run_heights: f32) -> Params {
        Params { min_area: 1, furniture_fraction: 1.0, rule_run_heights, underline_strip: true, ..Params::default() }
    }

    /// An underlined word: three one-pixel-wide strokes fused into one
    /// component by a full-width rule touching their feet. The rule is
    /// erased everywhere except where a stroke touches it directly.
    ///
    /// The rule is drawn 2 rows deep, not 1, per the 2026-09-23 second rule
    /// ("Bands, not rows"): a single-row run is never erased, so a 1-row
    /// rule fixture would no longer exercise this pass at all.
    ///
    /// Premise changed from the first rule's version of this test: item 3
    /// ("Keep crossing strokes") measures each touching column's own
    /// vertical ink run *through* the band, which may reach above or below
    /// it -- and a letter stroke that touches the rule with no vertical gap
    /// produces one continuous run for its whole height plus the rule's, far
    /// taller than the band's own thickness, so that column is preserved
    /// whole rather than split at the band boundary. The three letters no
    /// longer recover their original 8px height; each keeps its own 8 rows
    /// plus the 2 rule rows fused to its foot (10), and only the columns
    /// between the letters -- where the rule's ink stands alone -- are
    /// erased. This is the same mechanism, not a special case, that
    /// preserves a `)`'s bowl in
    /// `a_crossing_stroke_keeps_its_pixels_through_the_band` below.
    #[test]
    fn an_underlined_word_splits_into_letters_with_the_line_gone() {
        let (mut mask, w, h) = mask_of(&[
            "..............................",
            "..............................",
            ".....#.......#.......#........",
            ".....#.......#.......#........",
            ".....#.......#.......#........",
            ".....#.......#.......#........",
            ".....#.......#.......#........",
            ".....#.......#.......#........",
            ".....#.......#.......#........",
            ".....#.......#.......#........",
            "...####################.......",
            "...####################.......",
            "..............................",
        ]);
        // Single component on the page, so the median glyphish height this
        // measures against is that component's own height (10): run_floor =
        // 1.0 * 10 = 10, comfortably below the rule's 20px run and
        // comfortably above every 1px-wide letter stroke.
        let stripped = strip_underlines(&mut mask, w, h, &strip_params(1.0));
        let comps = stripped.components;
        assert_eq!(comps.len(), 3, "the rule must be gone and the three strokes separate");
        let mut x0s: Vec<u32> = comps.iter().map(|c| c.x0).collect();
        x0s.sort_unstable();
        assert_eq!(x0s, vec![5, 13, 21]);
        for c in &comps {
            assert_eq!(c.height(), 10, "a touching stroke keeps the rule rows fused to its own foot");
        }
        // Rows 10 and 11: erased everywhere except the three columns where a
        // letter's own stroke touches the rule directly.
        for y in [10u32, 11] {
            let row = &mask[(y * w) as usize..((y + 1) * w) as usize];
            for (x, &b) in row.iter().enumerate() {
                if [5usize, 13, 21].contains(&x) {
                    assert_ne!(b, 0, "a touching stroke's own column keeps its ink at row {y}, col {x}");
                } else {
                    assert_eq!(b, 0, "the rule alone must be gone at row {y}, col {x}");
                }
            }
        }
        assert_eq!(stripped.rule_segments.len(), 1, "one band erased");
        let seg = stripped.rule_segments[0];
        assert_eq!((seg.x0, seg.x1, seg.y0, seg.y1), (3, 23, 10, 12), "the band's own extent is recorded");
    }

    /// A long em dash and a separate row of hyphens, on a page whose other
    /// components establish a realistic median height. Both are narrower
    /// than the run floor and must never be examined at all.
    #[test]
    fn an_em_dash_and_a_row_of_hyphens_are_untouched() {
        let mut rows: Vec<String> = vec![".".repeat(80); 14];
        // Five 8px-tall letter blocks establish the page's median glyphish
        // height.
        for k in 0..5u32 {
            let x0 = (2 + k * 10) as usize;
            for y in 2..10usize {
                rows[y].replace_range(x0..x0 + 6, "######");
            }
        }
        // The em dash: one 20px-wide, 2px-tall component, well under
        // run_floor = 5.4209 * 8 = 43.4.
        for y in 12..14usize {
            rows[y].replace_range(2..22, &"#".repeat(20));
        }
        let rows_ref: Vec<&str> = rows.iter().map(String::as_str).collect();
        let (mut mask, w, h) = mask_of(&rows_ref);
        let before = mask.clone();
        let p = strip_params(Params::default().rule_run_heights.max(5.4209));
        let stripped = strip_underlines(&mut mask, w, h, &p);
        assert_eq!(mask, before, "nothing under the run floor may be touched");
        // 5 letter blocks + 1 dash, none merged or altered.
        assert_eq!(stripped.components.len(), 6);
        assert!(
            stripped.components.iter().any(|c| c.width() == 20 && c.height() == 2),
            "the dash must survive whole"
        );
        assert!(stripped.rule_segments.is_empty(), "nothing examined, nothing erased");
    }

    /// A table rule crossing a row of digit-like blocks: the rule is erased
    /// between the digits, and each digit still ends up its own component at
    /// its original x-position. Drawn 2 rows deep, same reason as the
    /// underlined-word fixture above.
    ///
    /// Premise changed from the first rule's version of this test, the same
    /// way and for the same reason as
    /// `an_underlined_word_splits_into_letters_with_the_line_gone` above:
    /// these digit blocks are solid rectangles touching the rule with no
    /// vertical gap, so every one of a digit's own columns reads as a
    /// crossing stroke under item 3 and keeps the two rule rows fused to its
    /// foot. Each digit comes back 10px tall, not 8; only the rule pixels
    /// strictly between digits, where no glyph column touches them, are
    /// erased.
    #[test]
    fn a_table_rule_touching_digits_recovers_them() {
        let mut rows: Vec<String> = vec![".".repeat(60); 12];
        for k in 0..4u32 {
            let x0 = (4 + k * 12) as usize;
            for y in 1..9usize {
                rows[y].replace_range(x0..x0 + 4, "####");
            }
        }
        // A rule spanning every digit, touching their feet at rows 9-10.
        rows[9] = format!(".{}.", "#".repeat(58));
        rows[10] = rows[9].clone();
        let rows_ref: Vec<&str> = rows.iter().map(String::as_str).collect();
        let (mut mask, w, h) = mask_of(&rows_ref);
        let stripped = strip_underlines(&mut mask, w, h, &strip_params(1.0));
        let comps = stripped.components;
        assert_eq!(comps.len(), 4, "all four digits recovered, the rule gone between them");
        let mut x0s: Vec<u32> = comps.iter().map(|c| c.x0).collect();
        x0s.sort_unstable();
        assert_eq!(x0s, vec![4, 16, 28, 40]);
        for c in &comps {
            assert_eq!((c.width(), c.height()), (4, 10), "each digit keeps its own columns fused to the rule rows beneath them");
        }
        assert_eq!(stripped.rule_segments.len(), 1, "one band erased");
        let seg = stripped.rule_segments[0];
        assert_eq!((seg.x0, seg.x1, seg.y0, seg.y1), (1, 59, 9, 11), "the band's own extent is recorded");
    }

    /// A lone row that matches a real rule's own x-range, but never gets a
    /// second consecutive qualifying row of its own, must not be erased --
    /// even though a genuine 2-row band right next to it is.
    /// `docs/measurements/2026-09-23_underline_strip_damage.md` section 3
    /// found exactly this shape: a single problematic row sitting between
    /// the two lines of a double-underline convention.
    #[test]
    fn a_single_row_between_a_double_rule_is_not_erased() {
        let mut rows: Vec<String> = vec![".".repeat(50); 5];
        // The real band: two matching rows.
        for y in [0usize, 1] {
            rows[y].replace_range(5..45, &"#".repeat(40));
        }
        // Row 2 stays blank, disconnecting the lone row below from the band
        // above -- the same effect the observed 5-disjoint-run row had:
        // whatever comes after it cannot extend the band above.
        // A lone row matching the band's own x-range, with nothing before
        // or after it to make a second row.
        rows[3].replace_range(5..45, &"#".repeat(40));
        let rows_ref: Vec<&str> = rows.iter().map(String::as_str).collect();
        let (mut mask, w, h) = mask_of(&rows_ref);
        let before_row3 = mask[(3 * w) as usize..(4 * w) as usize].to_vec();
        let stripped = strip_underlines(&mut mask, w, h, &strip_params(1.0));
        let after_row3 = mask[(3 * w) as usize..(4 * w) as usize].to_vec();
        assert_eq!(after_row3, before_row3, "a lone qualifying row is never erased");
        for y in [0u32, 1] {
            let row = &mask[(y * w) as usize..((y + 1) * w) as usize];
            assert!(row.iter().all(|&b| b == 0), "the genuine 2-row band must still be erased");
        }
        assert_eq!(stripped.rule_segments.len(), 1, "only the real band is recorded");
        let seg = stripped.rule_segments[0];
        assert_eq!((seg.x0, seg.x1, seg.y0, seg.y1), (5, 45, 0, 2));
    }

    /// A vertical stroke crossing a rule -- a `)`'s bowl running the whole
    /// height of the examined component -- must keep every pixel where it
    /// crosses the band, even though the band is erased everywhere else
    /// (`ARCHITECTURE.md` section 11, 2026-09-23, "Keep crossing strokes";
    /// the `$(83)`/`$(328)` finding in
    /// `docs/measurements/2026-09-23_underline_strip_damage.md` section 3).
    #[test]
    fn a_crossing_stroke_keeps_its_pixels_through_the_band() {
        let w = 30usize;
        let hgt = 9usize;
        let mut rows: Vec<String> = vec![".".repeat(w); hgt];
        // The rule: two full-width rows.
        for y in [3usize, 4] {
            rows[y] = "#".repeat(w);
        }
        // A stroke crossing the whole component, 3px wide, columns 10..13.
        for row in rows.iter_mut() {
            row.replace_range(10..13, "###");
        }
        let rows_ref: Vec<&str> = rows.iter().map(String::as_str).collect();
        let (mut mask, wu, hu) = mask_of(&rows_ref);
        let stripped = strip_underlines(&mut mask, wu, hu, &strip_params(1.0));
        for y in [3u32, 4] {
            let row = &mask[(y * wu) as usize..((y + 1) * wu) as usize];
            for x in 0..wu as usize {
                let ink = row[x] != 0;
                if (10..13).contains(&x) {
                    assert!(ink, "the crossing stroke keeps its pixels at row {y}, col {x}");
                } else {
                    assert!(!ink, "the rule itself must be erased at row {y}, col {x}");
                }
            }
        }
        assert_eq!(stripped.rule_segments.len(), 1, "the band is recorded even though a stroke crosses it");
    }

    /// A box whose top edge is a real 2-row band: erasing it leaves the
    /// box's own vertical sides behind as new tall/narrow pieces, because
    /// each side is a crossing stroke far taller than the band's own
    /// thickness. With `debris_heights` set, no such piece may survive
    /// (`ARCHITECTURE.md` section 11, 2026-09-23, "Drop strip debris";
    /// `docs/measurements/2026-09-23_underline_strip_damage.md` sections 3-4,
    /// the `POST-STRIP-LARGEST` finding).
    #[test]
    fn a_box_top_edge_leaves_no_debris_taller_than_the_bound() {
        let w = 140usize;
        let hgt = 95usize;
        let mut rows: Vec<String> = vec![".".repeat(w); hgt];

        // Five 8px-tall letter blocks set the page's median glyphish height.
        for k in 0..5u32 {
            let x0 = (2 + k * 10) as usize;
            for y in 2..10usize {
                rows[y].replace_range(x0..x0 + 6, "######");
            }
        }

        // A box: a 2-row top edge and two full-height vertical sides, far
        // taller than the letters.
        for y in [2usize, 3] {
            rows[y].replace_range(100..130, &"#".repeat(30));
        }
        for y in 2..90usize {
            rows[y].replace_range(100..101, "#");
            rows[y].replace_range(129..130, "#");
        }

        let rows_ref: Vec<&str> = rows.iter().map(String::as_str).collect();
        let (mut mask, wu, hu) = mask_of(&rows_ref);
        let p = Params {
            min_area: 1,
            furniture_fraction: 1.0,
            rule_run_heights: 1.0,
            debris_heights: 5.0,
            underline_strip: true,
            ..Params::default()
        };
        let stripped = strip_underlines(&mut mask, wu, hu, &p);

        let debris_floor = 5.0 * 8.0; // debris_heights * the median height h = 8
        assert!(
            stripped.components.iter().all(|c| f64::from(c.height()) <= debris_floor),
            "no post-strip piece may survive taller than the debris bound"
        );
        assert_eq!(stripped.components.len(), 5, "only the five letters survive; the box is fully consumed");
        assert!(stripped.components.iter().all(|c| c.x0 < 100), "nothing from the box's region survives");
        assert_eq!(stripped.rule_segments.len(), 1, "the box's top edge is recorded as one band");
        let seg = stripped.rule_segments[0];
        assert_eq!((seg.x0, seg.x1, seg.y0, seg.y1), (100, 130, 2, 4));
    }

    /// A box's top-rule strip leaves its narrow vertical sides behind as a
    /// tall/narrow sliver that survives `debris_heights` (too short) but not
    /// `thin_debris_heights` -- the exact shape (3px wide, 51px tall)
    /// observed on `filing__r000055`/`r000044` (`ARCHITECTURE.md` section
    /// 11, 2026-09-23, "Two mechanisms named: the strip leaves a box-side
    /// sliver that merges lines").
    #[test]
    fn a_narrow_box_side_survives_debris_heights_but_not_thin_debris_heights() {
        let w = 60usize;
        let hgt = 60usize;
        let mut rows: Vec<String> = vec![".".repeat(w); hgt];

        // Five 13px-tall letter blocks, clear of the box's x-range, set the
        // page's median glyphish height -- matching the real h=13.000
        // measured on filing__r000055.
        for k in 0..5u32 {
            let x0 = (2 + k * 5) as usize;
            for y in 2..15usize {
                rows[y].replace_range(x0..x0 + 4, "####");
            }
        }

        // A box: a 2-row top edge 20px wide, and two 3px-wide vertical
        // sides running 51 rows -- 3x51, the observed sliver shape.
        for y in [2usize, 3] {
            rows[y].replace_range(30..50, &"#".repeat(20));
        }
        for y in 2..53usize {
            rows[y].replace_range(30..33, "###");
            rows[y].replace_range(47..50, "###");
        }

        let rows_ref: Vec<&str> = rows.iter().map(String::as_str).collect();

        // Without thin_debris_heights (0.0, the shipped-off value), the
        // sliver survives: too narrow for furniture_fraction, too short for
        // debris_heights (51 < 5.0 x 13 = 65).
        let (mut mask_off, wu, hu) = mask_of(&rows_ref);
        let p_off = Params {
            min_area: 1,
            furniture_fraction: 1.0,
            rule_run_heights: 1.0,
            debris_heights: 5.0,
            thin_debris_heights: 0.0,
            underline_strip: true,
            ..Params::default()
        };
        let stripped_off = strip_underlines(&mut mask_off, wu, hu, &p_off);
        assert!(
            stripped_off.components.iter().any(|c| c.width() == 3 && c.height() == 51),
            "without thin_debris_heights the 3x51 sliver survives"
        );

        // With thin_debris_heights set, the same sliver -- width 3 <= 0.5 x
        // h (6.5) and height 51 > thin_debris_heights x h (3.5 x 13 = 45.5)
        // -- is dropped, even though debris_heights alone would not catch
        // it.
        let (mut mask_on, _, _) = mask_of(&rows_ref);
        let p_on = Params { thin_debris_heights: 3.5, ..p_off };
        let stripped_on = strip_underlines(&mut mask_on, wu, hu, &p_on);
        assert!(
            stripped_on.components.iter().all(|c| !(c.width() <= 6 && c.height() > 45)),
            "thin_debris_heights drops the narrow box-side sliver"
        );
        assert_eq!(
            stripped_on.components.len(),
            5,
            "only the five letters survive; both box-side slivers are dropped"
        );
    }

    /// Every erased band is recorded as a rule segment on the strip's
    /// output, independent of whatever downstream consumes it
    /// (`ARCHITECTURE.md` section 11, 2026-09-23, "Keep what was stripped").
    /// Two independent bands on the same page must each produce their own
    /// segment.
    #[test]
    fn rule_segments_are_recorded() {
        let mut rows: Vec<String> = vec![".".repeat(60); 20];
        // Five 8px-tall letter blocks establish the page's median glyphish
        // height.
        for k in 0..5u32 {
            let x0 = (2 + k * 10) as usize;
            for y in 2..10usize {
                rows[y].replace_range(x0..x0 + 6, "######");
            }
        }
        // Two independent 2-row rule bands, in different components, at
        // different rows and x-ranges.
        for y in [12usize, 13] {
            rows[y].replace_range(0..40, &"#".repeat(40));
        }
        for y in [16usize, 17] {
            rows[y].replace_range(10..58, &"#".repeat(48));
        }
        let rows_ref: Vec<&str> = rows.iter().map(String::as_str).collect();
        let (mut mask, w, h) = mask_of(&rows_ref);
        let stripped = strip_underlines(&mut mask, w, h, &strip_params(1.0));
        let mut segs = stripped.rule_segments.clone();
        segs.sort_by_key(|s| s.y0);
        assert_eq!(segs.len(), 2, "two independent bands must each be recorded");
        assert_eq!((segs[0].x0, segs[0].x1, segs[0].y0, segs[0].y1), (0, 40, 12, 14));
        assert_eq!((segs[1].x0, segs[1].x1, segs[1].y0, segs[1].y1), (10, 58, 16, 18));
    }

    /// The shipped default runs the strip (`ARCHITECTURE.md` section 11,
    /// 2026-09-23, "Underline strip ships"); `lines.underline_strip=0` is the
    /// off switch the pipeline branch honours.
    #[test]
    fn the_strip_ships_on_by_default() {
        assert!(Params::default().underline_strip);
    }
}
