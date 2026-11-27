//! JBIG2 (T.88) peak, by walking segment headers the way hayro-jbig2
//! decodes them: the page bitmap, each region's own bitmap unless it can
//! be decoded straight into the page, stored intermediate regions and
//! pattern dictionaries, a halftone's gray-scale planes, and finally the
//! packed output grown byte by byte while the page is still held.

use super::{be, byte_pushed_peak, len};

/// Header prefix of the JBIG2 file organisation (T.88 D.4.1).
const FILE_HEADER_ID: [u8; 8] = [0x97, 0x4A, 0x42, 0x32, 0x0D, 0x0A, 0x1A, 0x0A];

/// Working bytes above `data` for an embedded JBIG2 stream and its
/// decoded `/JBIG2Globals`. `None` for the file organisation, a symbol
/// dictionary (its bitmap sizes are coded per height class, not in the
/// header), more than one page, a striped page of unknown height, a
/// segment of unknown length, or a truncated header.
pub(super) fn working_bytes(data: &[u8], globals: Option<&[u8]>) -> Option<u64> {
    if data.starts_with(&FILE_HEADER_ID) {
        return None;
    }
    let mut walk = Walk::default();
    if let Some(g) = globals {
        walk.segments(g)?;
        walk.held = walk.held.saturating_add(len(g));
    }
    walk.segments(data)?;
    let page = walk.page?;
    let output = byte_pushed_peak(page.width.div_ceil(8).saturating_mul(page.height));
    let held = walk.held.saturating_add(page.bytes());
    Some(walk.peak.max(held.saturating_add(output)))
}

#[derive(Debug, Clone, Copy)]
struct Page {
    width: u64,
    height: u64,
    default_pixel_zero: bool,
}

impl Page {
    fn bytes(self) -> u64 {
        bitmap(self.width, self.height)
    }
}

#[derive(Default)]
struct Walk {
    page: Option<Page>,
    /// Whether no region has been drawn on the page yet.
    painted: bool,
    /// Bytes that stay live to the end: stored regions and dictionaries.
    held: u64,
    /// Largest transient total seen while decoding a region.
    peak: u64,
}

/// A hayro `Bitmap`: rows of 32-bit words.
fn bitmap(width: u64, height: u64) -> u64 {
    width.div_ceil(32).saturating_mul(4).saturating_mul(height)
}

impl Walk {
    /// Segment headers in sequential organisation (T.88 7.2).
    fn segments(&mut self, d: &[u8]) -> Option<()> {
        let mut at = 0usize;
        while at < d.len() {
            let number = be::<4>(d, at)?;
            let flags = *d.get(at + 4)?;
            let count_byte = *d.get(at + 5)?;
            let (refs, mut pos) = if count_byte >> 5 == 7 {
                let r = be::<4>(d, at + 5)? & 0x1FFF_FFFF;
                (r, at + 9 + usize::try_from((r + 1).div_ceil(8)).ok()?)
            } else {
                (u64::from(count_byte >> 5), at + 6)
            };
            let ref_size = match number {
                0..=256 => 1,
                257..=65536 => 2,
                _ => 4,
            };
            pos = pos.checked_add(usize::try_from(refs.checked_mul(ref_size)?).ok()?)?;
            pos += if flags & 0x40 != 0 { 4 } else { 1 };
            let length = be::<4>(d, pos)?;
            if length == 0xFFFF_FFFF {
                return None;
            }
            pos += 4;
            let end = pos.checked_add(usize::try_from(length).ok()?)?;
            self.segment(flags & 0x3F, d.get(pos..end)?)?;
            at = end;
        }
        Some(())
    }

    fn segment(&mut self, kind: u8, body: &[u8]) -> Option<()> {
        match kind {
            0 => return None,
            48 => {
                if self.page.is_some() {
                    return None;
                }
                let height = be::<4>(body, 4)?;
                if height == 0xFFFF_FFFF {
                    return None;
                }
                self.page = Some(Page {
                    width: be::<4>(body, 0)?,
                    height,
                    default_pixel_zero: body.get(16)? & 0x04 == 0,
                });
            }
            16 => self.held = self.held.saturating_add(pattern_dictionary(body)?),
            4 | 20 | 36 | 40 => {
                let extra = if kind == 20 { halftone_extra(body)? } else { 0 };
                let region = region(body)?.bytes();
                self.transient(region.saturating_add(extra));
                self.held = self.held.saturating_add(region);
            }
            6 | 7 | 22 | 23 | 38 | 39 | 42 | 43 => self.immediate(kind, body)?,
            _ => {}
        }
        Some(())
    }

    /// An immediate region (T.88 7.4.1): its own bitmap unless hayro
    /// decodes it straight into a pristine page, plus a halftone's planes.
    fn immediate(&mut self, kind: u8, body: &[u8]) -> Option<()> {
        let info = region(body)?;
        let (default_pixel, extra) = match kind {
            6 | 7 => (be::<2>(body, 17)? & 0x0200 != 0, 0),
            22 | 23 => (body.get(17)? & 0x80 != 0, halftone_extra(body)?),
            _ => (false, 0),
        };
        let direct = self.page.is_some_and(|p| info.direct(p, default_pixel)) && !self.painted;
        self.painted = true;
        let own = if direct { 0 } else { info.bytes() };
        self.transient(own.saturating_add(extra));
        Some(())
    }

    fn transient(&mut self, bytes: u64) {
        let page = self.page.map_or(0, Page::bytes);
        let now = self.held.saturating_add(page).saturating_add(bytes);
        self.peak = self.peak.max(now);
    }
}

/// Region segment information field (T.88 7.4.1).
struct Region {
    width: u64,
    height: u64,
    x: u64,
    y: u64,
    /// External combination operator, flags bits 0-2.
    op: u8,
}

fn region(body: &[u8]) -> Option<Region> {
    Some(Region {
        width: be::<4>(body, 0)?,
        height: be::<4>(body, 4)?,
        x: be::<4>(body, 8)?,
        y: be::<4>(body, 12)?,
        op: body.get(16)? & 0x07,
    })
}

impl Region {
    fn bytes(&self) -> u64 {
        bitmap(self.width, self.height)
    }

    /// hayro-jbig2's direct-decode test: the region covers the page from
    /// the origin, its initial pixel differs from "page default is zero",
    /// and the operator leaves a fresh page unchanged where the region is
    /// blank (OR/XOR on a 0 page, AND/XNOR on a 1 page, REPLACE always).
    fn direct(&self, page: Page, default_pixel: bool) -> bool {
        let covers =
            self.x == 0 && self.y == 0 && self.width == page.width && self.height == page.height;
        covers
            && default_pixel != page.default_pixel_zero
            && match self.op {
                4 => true,
                0 | 2 => page.default_pixel_zero,
                _ => !page.default_pixel_zero,
            }
    }
}

/// Pattern dictionary (T.88 7.4.4, 6.7.5): the collective bitmap, one
/// bitmap per pattern, and for patterns at most 32 px square hayro's
/// pre-shifted copies (32 shifts of two words per row).
fn pattern_dictionary(body: &[u8]) -> Option<u64> {
    let (pw, ph) = (u64::from(*body.get(1)?), u64::from(*body.get(2)?));
    let patterns = be::<4>(body, 3)?.saturating_add(1);
    let collective = bitmap(patterns.saturating_mul(pw), ph);
    let each = patterns.saturating_mul(bitmap(pw, ph));
    let shifted = if pw <= 32 && ph <= 32 {
        patterns.saturating_mul(32 * ph * 2 * 4)
    } else {
        0
    };
    Some(collective.saturating_add(each).saturating_add(shifted))
}

/// Halftone gray-scale decoding (T.88 7.4.5, C.5): the HGW x HGH grid of
/// 32-bit gray values, the current and previous bitplane, and a skip
/// bitmap when HENABLESKIP is set.
fn halftone_extra(body: &[u8]) -> Option<u64> {
    let flags = *body.get(17)?;
    let (gw, gh) = (be::<4>(body, 18)?, be::<4>(body, 22)?);
    let plane = bitmap(gw, gh);
    let planes = if flags & 0x08 != 0 { 3 } else { 2 };
    Some(
        gw.saturating_mul(gh)
            .saturating_mul(4)
            .saturating_add(plane.saturating_mul(planes)),
    )
}
