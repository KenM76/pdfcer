//! Standard paper sizes, as PDF default-user-space rectangles.
//!
//! # Purpose
//!
//! A shared, shell-independent table of the sheet sizes an operator names
//! by letter ("A1", "ANSI D", "Letter") rather than by four numbers, so
//! that [`crate::edit::EditSession::set_media_box`] has a companion that
//! answers *"how big is A1, in points?"* exactly once for the whole
//! project.
//!
//! ## Why this lives in `pdfcer-core` and not in a shell
//!
//! It looks like presentation data, and it is not. `pdfce-gui`'s page-size
//! chooser, `pdfcer`'s `set-page-size --size`, and any future web shell
//! all need the identical numbers, and a table duplicated across three
//! shells is a table that will disagree across three shells — with the
//! disagreement showing up as a drawing sheet that is 0.4 pt short in one
//! of them and nowhere else. The GUI-core separation invariant says
//! `pdfcer-core` may not depend on a GUI crate; it says nothing against
//! core owning data every shell needs, and this is exactly that case.
//!
//! It also has a second, engine-side consumer that is not a shell at all:
//! a "what size is this page?" report wants to say *"A3 landscape"*, not
//! *"1190.55 × 841.89"*, and [`PaperSize::classify`] is where that lives.
//!
//! # Units, and the one arithmetic decision in this file
//!
//! Every rectangle is in **default user space units** — 1/72 inch
//! (ISO 32000-1 §8.3.2.3), the unit `/MediaBox` is defined in
//! (§7.7.3.3, Table 30). Sizes are therefore stored in their **defining**
//! unit — millimetres for the ISO 216 A series, inches for the US and
//! ANSI series — and converted here, rather than stored as pre-rounded
//! point values.
//!
//! That is deliberate and is worth the extra line of arithmetic. A1 is
//! *defined* as 594 × 841 mm; written as points it is
//! 1683.7795275590551 × 2383.937007874016, and any hand-rounded form of
//! that ("1683.78") is a number that is **not** A1 and that will not
//! compare equal to a file produced by a CAD exporter doing the same
//! conversion at full precision. Doing the division here means pdfcer's A1
//! and SolidWorks' A1 are the same `f64`.
//!
//! Note what is NOT here: PDF 1.6's per-page `/UserUnit` (Table 30) can
//! rescale user space to physical space, so a page whose media box is
//! "A1 in points" is only physically A1 at the default unit. This module
//! deals in default user space and says nothing about `/UserUnit`; a
//! caller that cares must read it off the page.
//!
//! # Provenance of the numbers
//!
//! - **A series** — ISO 216, which defines A0 as 841 × 1189 mm and each
//!   subsequent size as the previous one halved across its longer
//!   dimension, rounded down to the millimetre. The rounding is part of
//!   the standard, which is why each size is listed explicitly rather
//!   than computed by halving A0 in a loop: a loop would produce
//!   594.5 mm for A1's short edge, and A1 is 594.
//! - **US sizes** — the customary Letter/Legal/Tabloid/Executive
//!   dimensions in whole or half inches, all of which land on exact
//!   integer point values (8.5 in × 72 = 612).
//! - **ANSI series** — ASME Y14.1 engineering drawing sheet sizes A–E,
//!   each an exact doubling of the previous, all integer points. These
//!   are the ones a CAD exporter emits, and the reason this table exists
//!   at all: the corpus pdfcer is measured against is drawing sheets.
//!
//! # Failure modes
//!
//! There are none. Every function here is total: an infallible lookup
//! into a fixed table, plus [`PaperSize::classify`], which returns
//! `Option` and cannot fail. Nothing here parses, allocates, or touches a
//! document.

use crate::page_tree::Rect;

/// Which way round a sheet is used.
///
/// Separate from [`PaperSize`] rather than doubled into it (`A1`,
/// `A1Landscape`, …) because orientation is orthogonal to size: every
/// size supports both, a doubled enum has twice the variants and twice
/// the match arms, and a front end almost always wants the two as
/// separate controls — a size list and an orientation toggle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Orientation {
    /// Taller than it is wide — the defining orientation of every size in
    /// this table.
    #[default]
    Portrait,
    /// Wider than it is tall. **The normal orientation for a drawing
    /// sheet**, which is why this type exists at all: a CAD sheet named
    /// "A1" is A1 landscape in every practical case.
    Landscape,
}

/// A named standard sheet size.
///
/// Sizes are listed in their **portrait** orientation; use
/// [`PaperSize::rect_with`] to get either.
///
/// `#[non_exhaustive]`: this table will grow (ARCH sizes, JIS B, ISO B/C
/// envelope series are all plausible additions), and a downstream `match`
/// that would break on each addition is a cost paid by every consumer for
/// no benefit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PaperSize {
    /// ISO 216 A0 — 841 × 1189 mm.
    A0,
    /// ISO 216 A1 — 594 × 841 mm.
    A1,
    /// ISO 216 A2 — 420 × 594 mm.
    A2,
    /// ISO 216 A3 — 297 × 420 mm.
    A3,
    /// ISO 216 A4 — 210 × 297 mm.
    A4,
    /// ISO 216 A5 — 148 × 210 mm.
    A5,
    /// ISO 216 A6 — 105 × 148 mm.
    A6,
    /// US Letter — 8.5 × 11 in (612 × 792 pt exactly).
    Letter,
    /// US Legal — 8.5 × 14 in (612 × 1008 pt exactly).
    Legal,
    /// US Tabloid / Ledger — 11 × 17 in (792 × 1224 pt exactly).
    Tabloid,
    /// US Executive — 7.25 × 10.5 in (522 × 756 pt exactly).
    Executive,
    /// ASME Y14.1 ANSI A — 8.5 × 11 in. Dimensionally identical to
    /// [`Self::Letter`]; kept distinct because a drawing titled "ANSI A"
    /// and a memo on Letter are different documents to their author, and
    /// [`Self::classify`] has to pick one name for the shared rectangle
    /// (it picks `Letter` — see that method).
    AnsiA,
    /// ASME Y14.1 ANSI B — 11 × 17 in. Dimensionally identical to
    /// [`Self::Tabloid`].
    AnsiB,
    /// ASME Y14.1 ANSI C — 17 × 22 in (1224 × 1584 pt exactly).
    AnsiC,
    /// ASME Y14.1 ANSI D — 22 × 34 in (1584 × 2448 pt exactly).
    AnsiD,
    /// ASME Y14.1 ANSI E — 34 × 44 in (2448 × 3168 pt exactly).
    AnsiE,
}

/// Points per millimetre: 72 points per inch ÷ 25.4 mm per inch.
const PT_PER_MM: f64 = 72.0 / 25.4;
/// Points per inch (ISO 32000-1 §8.3.2.3 — the default user space unit is
/// 1/72 inch).
const PT_PER_IN: f64 = 72.0;

impl PaperSize {
    /// Every size in this table, in the order a size picker should list
    /// them: the A series largest-first, then the US sizes, then the ANSI
    /// engineering series.
    ///
    /// Largest-first for the A series because that is how a drafting
    /// sheet list reads, and because the operator's own corpus is A1/A3 —
    /// burying them under A4 would make the common case the hard one.
    pub const ALL: &'static [Self] = &[
        Self::A0,
        Self::A1,
        Self::A2,
        Self::A3,
        Self::A4,
        Self::A5,
        Self::A6,
        Self::Letter,
        Self::Legal,
        Self::Tabloid,
        Self::Executive,
        Self::AnsiA,
        Self::AnsiB,
        Self::AnsiC,
        Self::AnsiD,
        Self::AnsiE,
    ];

    /// The size's **portrait** width and height, in default user space
    /// units (points).
    ///
    /// Kept private-ish in spirit (it is `pub` only because
    /// [`Self::rect_with`] and [`Self::classify`] both need it and hiding
    /// it would mean duplicating the table) — prefer [`Self::rect_with`],
    /// which returns the rectangle callers actually want.
    #[must_use]
    pub fn size_pt(self) -> (f64, f64) {
        match self {
            Self::A0 => (841.0 * PT_PER_MM, 1189.0 * PT_PER_MM),
            Self::A1 => (594.0 * PT_PER_MM, 841.0 * PT_PER_MM),
            Self::A2 => (420.0 * PT_PER_MM, 594.0 * PT_PER_MM),
            Self::A3 => (297.0 * PT_PER_MM, 420.0 * PT_PER_MM),
            Self::A4 => (210.0 * PT_PER_MM, 297.0 * PT_PER_MM),
            Self::A5 => (148.0 * PT_PER_MM, 210.0 * PT_PER_MM),
            Self::A6 => (105.0 * PT_PER_MM, 148.0 * PT_PER_MM),
            Self::Letter | Self::AnsiA => (8.5 * PT_PER_IN, 11.0 * PT_PER_IN),
            Self::Legal => (8.5 * PT_PER_IN, 14.0 * PT_PER_IN),
            Self::Tabloid | Self::AnsiB => (11.0 * PT_PER_IN, 17.0 * PT_PER_IN),
            Self::Executive => (7.25 * PT_PER_IN, 10.5 * PT_PER_IN),
            Self::AnsiC => (17.0 * PT_PER_IN, 22.0 * PT_PER_IN),
            Self::AnsiD => (22.0 * PT_PER_IN, 34.0 * PT_PER_IN),
            Self::AnsiE => (34.0 * PT_PER_IN, 44.0 * PT_PER_IN),
        }
    }

    /// The `/MediaBox` rectangle for this size in the given orientation,
    /// with its lower-left corner at the origin.
    ///
    /// **The origin is `(0, 0)` and that is a choice, not a law.**
    /// §7.7.3.3 does not require a media box to start at the origin, and
    /// real files (imposition output, cropped scans) do carry offset
    /// ones. A *named* size, though, is a request for a fresh sheet of
    /// that size, and a fresh sheet at the origin is what every producer
    /// emits and what every downstream coordinate assumption expects. A
    /// caller who needs an offset sheet builds the [`Rect`] directly and
    /// does not come through here.
    #[must_use]
    pub fn rect_with(self, orientation: Orientation) -> Rect {
        let (w, h) = self.size_pt();
        let (w, h) = match orientation {
            Orientation::Portrait => (w, h),
            Orientation::Landscape => (h, w),
        };
        Rect::from_corners(0.0, 0.0, w, h)
    }

    /// The portrait rectangle for this size — [`Self::rect_with`] with
    /// [`Orientation::Portrait`].
    #[must_use]
    pub fn rect(self) -> Rect {
        self.rect_with(Orientation::Portrait)
    }

    /// A stable, lowercase, machine-facing identifier: `"a1"`,
    /// `"letter"`, `"ansi-d"`.
    ///
    /// **Not a display string** — decision 002 R1 keeps user-facing text
    /// in the shells' own catalogs. This is what a CLI flag value and a
    /// settings file spell, so it is ASCII, lowercase, hyphenated, and
    /// must not change once shipped.
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::A0 => "a0",
            Self::A1 => "a1",
            Self::A2 => "a2",
            Self::A3 => "a3",
            Self::A4 => "a4",
            Self::A5 => "a5",
            Self::A6 => "a6",
            Self::Letter => "letter",
            Self::Legal => "legal",
            Self::Tabloid => "tabloid",
            Self::Executive => "executive",
            Self::AnsiA => "ansi-a",
            Self::AnsiB => "ansi-b",
            Self::AnsiC => "ansi-c",
            Self::AnsiD => "ansi-d",
            Self::AnsiE => "ansi-e",
        }
    }

    /// Look a size up by its [`Self::id`], case-insensitively.
    ///
    /// Returns `None` rather than a guess: a mistyped `--size a11` must
    /// become a named refusal, never the nearest match. Silently
    /// resolving a typo to a *plausible* sheet size is precisely the
    /// "fuzzy, never sneaky" failure — the operator would get a working
    /// file of the wrong size and no signal at all.
    #[must_use]
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|size| size.id().eq_ignore_ascii_case(id))
    }

    /// Name the standard size a rectangle matches, and in which
    /// orientation — or `None` if it matches none.
    ///
    /// Matching is to within `tolerance` points on **both** dimensions.
    /// A tolerance is required rather than optional: A4 in points is
    /// 595.2755905511811 × 841.8897637795276, producers round it
    /// differently (595.276, 595.28, 595.32 all occur in the wild), and
    /// an exact-equality classifier would answer `None` for essentially
    /// every real A4 page. [`Self::CLASSIFY_TOLERANCE`] is the value to
    /// pass when there is no reason to pick another.
    ///
    /// The lower-left corner is **ignored**: a sheet offset from the
    /// origin is still an A1 sheet, and this classifies *size*, not
    /// placement.
    ///
    /// Where two entries share a rectangle exactly ([`Self::Letter`] /
    /// [`Self::AnsiA`], [`Self::Tabloid`] / [`Self::AnsiB`]) the first in
    /// [`Self::ALL`] wins, i.e. the US name. That is a coin toss made
    /// once and written down, not a claim about the document's intent —
    /// nothing in the bytes distinguishes them, so no classifier could
    /// do better.
    #[must_use]
    pub fn classify(rect: &Rect, tolerance: f64) -> Option<(Self, Orientation)> {
        let (w, h) = (rect.width(), rect.height());
        Self::ALL.iter().copied().find_map(|size| {
            let (pw, ph) = size.size_pt();
            if (w - pw).abs() <= tolerance && (h - ph).abs() <= tolerance {
                Some((size, Orientation::Portrait))
            } else if (w - ph).abs() <= tolerance && (h - pw).abs() <= tolerance {
                Some((size, Orientation::Landscape))
            } else {
                None
            }
        })
    }

    /// The tolerance [`Self::classify`] should be given absent a reason
    /// to choose another: **1 point**, i.e. 1/72 inch.
    ///
    /// Chosen from the gap it has to bridge versus the gap it must not
    /// cross. Producer rounding of the A series is sub-point (the largest
    /// disagreement between "595.276" and the exact 595.2755… is 0.0005
    /// pt; a producer truncating to whole points is off by 0.28). The
    /// nearest two *distinct* sizes in this table differ by far more —
    /// A4 and Executive, the closest pair, are 73 pt apart on the long
    /// edge. So 1 pt absorbs every rounding artefact seen in practice
    /// with three orders of magnitude of headroom before it could
    /// mis-name a sheet.
    pub const CLASSIFY_TOLERANCE: f64 = 1.0;
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn a4_is_the_size_every_producer_writes() {
        // The canonical check: A4 in points is what the whole ecosystem
        // spells 595.276 x 841.89. If the mm->pt conversion here were
        // wrong, this is the number that would move.
        let (w, h) = PaperSize::A4.size_pt();
        assert!((w - 595.275_590_551_181_1).abs() < 1e-9, "A4 width {w}");
        assert!((h - 841.889_763_779_527_6).abs() < 1e-9, "A4 height {h}");
    }

    #[test]
    fn the_us_and_ansi_sizes_are_exact_integers() {
        // Inches x 72 lands on whole points for every one of these, so a
        // fractional result means the table is wrong.
        for size in [
            PaperSize::Letter,
            PaperSize::Legal,
            PaperSize::Tabloid,
            PaperSize::Executive,
            PaperSize::AnsiA,
            PaperSize::AnsiB,
            PaperSize::AnsiC,
            PaperSize::AnsiD,
            PaperSize::AnsiE,
        ] {
            let (w, h) = size.size_pt();
            assert_eq!(w.fract(), 0.0, "{} width {w}", size.id());
            assert_eq!(h.fract(), 0.0, "{} height {h}", size.id());
        }
        assert_eq!(PaperSize::Letter.size_pt(), (612.0, 792.0));
        assert_eq!(PaperSize::AnsiD.size_pt(), (1584.0, 2448.0));
        assert_eq!(PaperSize::AnsiE.size_pt(), (2448.0, 3168.0));
    }

    #[test]
    fn the_a_series_halves_with_iso_216s_own_rounding() {
        // Each A(n+1) is A(n) halved across the long edge, ROUNDED DOWN
        // to the millimetre. A1's short edge is 594 mm, not 594.5 — which
        // is exactly why the table is explicit rather than a halving
        // loop. This test is what would catch someone "simplifying" it
        // into one.
        let a1 = PaperSize::A1.size_pt();
        assert!(
            (a1.0 - 594.0 * PT_PER_MM).abs() < 1e-9,
            "A1 short edge must be 594 mm, not half of 1189"
        );
        // Every A size is taller than it is wide, and each is smaller
        // than its predecessor.
        let mut prev = f64::INFINITY;
        for size in [
            PaperSize::A0,
            PaperSize::A1,
            PaperSize::A2,
            PaperSize::A3,
            PaperSize::A4,
            PaperSize::A5,
            PaperSize::A6,
        ] {
            let (w, h) = size.size_pt();
            assert!(h > w, "{} must be portrait", size.id());
            assert!(
                h < prev,
                "{} must be smaller than its predecessor",
                size.id()
            );
            prev = h;
        }
    }

    #[test]
    fn landscape_swaps_the_edges_and_keeps_the_origin() {
        let p = PaperSize::A1.rect_with(Orientation::Portrait);
        let l = PaperSize::A1.rect_with(Orientation::Landscape);
        assert_eq!((p.llx, p.lly), (0.0, 0.0));
        assert_eq!((l.llx, l.lly), (0.0, 0.0));
        assert_eq!(p.width(), l.height());
        assert_eq!(p.height(), l.width());
        assert!(l.width() > l.height(), "landscape A1 is wider than tall");
    }

    #[test]
    fn every_id_round_trips_and_is_unique() {
        let mut seen = Vec::new();
        for size in PaperSize::ALL {
            assert_eq!(PaperSize::from_id(size.id()), Some(*size));
            assert!(!seen.contains(&size.id()), "duplicate id {}", size.id());
            seen.push(size.id());
        }
        assert_eq!(seen.len(), PaperSize::ALL.len());
        // Case-insensitive, because a script author will type A1.
        assert_eq!(PaperSize::from_id("A1"), Some(PaperSize::A1));
        assert_eq!(PaperSize::from_id("ANSI-D"), Some(PaperSize::AnsiD));
    }

    #[test]
    fn an_unknown_id_is_none_not_a_near_miss() {
        // Silently resolving `a11` to A1 would hand the operator a
        // working file of the wrong size with no signal.
        assert_eq!(PaperSize::from_id("a11"), None);
        assert_eq!(PaperSize::from_id(""), None);
        assert_eq!(PaperSize::from_id("a"), None);
    }

    #[test]
    fn classify_names_a_producer_rounded_sheet() {
        // The real-world case: a file that says 595.276 x 841.89, which
        // is A4 rounded to 3 decimals, must classify as A4. Exact
        // equality would answer None here.
        let rounded = Rect::from_corners(0.0, 0.0, 595.276, 841.89);
        assert_eq!(
            PaperSize::classify(&rounded, PaperSize::CLASSIFY_TOLERANCE),
            Some((PaperSize::A4, Orientation::Portrait))
        );
        // And a whole-point truncation, which is off by 0.28 pt.
        let truncated = Rect::from_corners(0.0, 0.0, 595.0, 842.0);
        assert_eq!(
            PaperSize::classify(&truncated, PaperSize::CLASSIFY_TOLERANCE),
            Some((PaperSize::A4, Orientation::Portrait))
        );
    }

    #[test]
    fn classify_recognizes_landscape_and_ignores_the_origin() {
        // A drawing sheet: A1 landscape, and offset from the origin for
        // good measure — placement is not size.
        let sheet = Rect::from_corners(100.0, 50.0, 100.0 + 2383.937, 50.0 + 1683.78);
        assert_eq!(
            PaperSize::classify(&sheet, PaperSize::CLASSIFY_TOLERANCE),
            Some((PaperSize::A1, Orientation::Landscape))
        );
    }

    #[test]
    fn classify_declines_a_custom_size() {
        // 900 x 600 pt is nothing standard, and must not be forced onto
        // the nearest entry.
        let custom = Rect::from_corners(0.0, 0.0, 900.0, 600.0);
        assert_eq!(
            PaperSize::classify(&custom, PaperSize::CLASSIFY_TOLERANCE),
            None
        );
    }

    #[test]
    fn the_tolerance_cannot_reach_a_neighbouring_size() {
        // The guard on CLASSIFY_TOLERANCE's docs: the closest two
        // distinct sizes must be far more than one tolerance apart, or
        // the classifier could name the wrong sheet.
        let mut closest = f64::INFINITY;
        for (i, a) in PaperSize::ALL.iter().enumerate() {
            for b in PaperSize::ALL.iter().skip(i + 1) {
                let (aw, ah) = a.size_pt();
                let (bw, bh) = b.size_pt();
                // Distinct only if they are not the same rectangle
                // (Letter/AnsiA and Tabloid/AnsiB deliberately coincide).
                let gap = (aw - bw).abs().max((ah - bh).abs());
                if gap > 0.0 {
                    closest = closest.min(gap);
                }
            }
        }
        assert!(
            closest > PaperSize::CLASSIFY_TOLERANCE * 10.0,
            "closest distinct sizes are {closest} pt apart; tolerance is \
             {} pt and needs an order of magnitude of headroom",
            PaperSize::CLASSIFY_TOLERANCE
        );
    }

    #[test]
    fn the_coincident_pairs_classify_to_the_us_name() {
        // Nothing in the bytes distinguishes Letter from ANSI A. The
        // choice is written down rather than left to ALL's order
        // accidentally changing it.
        assert_eq!(
            PaperSize::classify(&PaperSize::AnsiA.rect(), PaperSize::CLASSIFY_TOLERANCE),
            Some((PaperSize::Letter, Orientation::Portrait))
        );
        assert_eq!(
            PaperSize::classify(&PaperSize::AnsiB.rect(), PaperSize::CLASSIFY_TOLERANCE),
            Some((PaperSize::Tabloid, Orientation::Portrait))
        );
    }

    #[test]
    fn every_size_classifies_as_itself_in_both_orientations() {
        // Round-trip over the whole table: rect_with -> classify must
        // return a rectangle-equal size in the orientation asked for.
        for size in PaperSize::ALL {
            for orientation in [Orientation::Portrait, Orientation::Landscape] {
                let rect = size.rect_with(orientation);
                let (got, got_orientation) =
                    PaperSize::classify(&rect, PaperSize::CLASSIFY_TOLERANCE)
                        .unwrap_or_else(|| panic!("{} did not classify", size.id()));
                assert_eq!(
                    got.size_pt(),
                    size.size_pt(),
                    "{} classified as {}",
                    size.id(),
                    got.id()
                );
                // A square sheet would make orientation ambiguous; none
                // of these are square, so the orientation must match.
                assert_eq!(got_orientation, orientation, "{}", size.id());
            }
        }
    }
}
