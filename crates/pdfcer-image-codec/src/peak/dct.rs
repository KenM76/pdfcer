//! DCT (T.81) peak: the remux grid when pdfcer-jpeg-remux interleaves a
//! one-scan-per-component baseline, then zune-jpeg's output, row buffers
//! and, for multi-scan decodes, its i16 coefficient planes.

use super::{be, len};

/// Frame and first-scan header fields the prediction and the evidence
/// bound ([`crate::evidence`]) need.
pub(crate) struct Header {
    /// SOF marker low byte: 0/1 sequential Huffman, 2 progressive.
    pub(crate) process: u8,
    pub(crate) precision: u8,
    pub(crate) width: u64,
    pub(crate) height: u64,
    /// `(H, V)` per frame component, in frame order (T.81 B.2.2).
    pub(crate) sampling: Vec<(u64, u64)>,
    /// Component identifier Ci per frame component, in frame order.
    pub(crate) ids: Vec<u8>,
    /// Ns of the first scan (T.81 B.2.3).
    pub(crate) first_scan_components: u64,
    /// Scan component selectors Csj of the first scan (T.81 B.2.3).
    pub(crate) first_scan_selectors: Vec<u8>,
    /// Ss of the first scan; 0 is a DC scan (T.81 B.2.3, G.1.1.1.1).
    pub(crate) first_scan_ss: u8,
}

/// Working bytes above `data` for a DCT decode, or `None` for a process
/// zune refuses before allocating (lossless, hierarchical, arithmetic,
/// 12-bit), a missing frame or scan, DNL-deferred height, or a component
/// count with no output colour space.
pub(super) fn working_bytes(data: &[u8]) -> Option<u64> {
    let h = header(data)?;
    let nc = len(&h.sampling);
    if !matches!(h.process, 0..=2) || h.precision != 8 || !matches!(nc, 1 | 3 | 4) {
        return None;
    }
    if h.width == 0 || h.height == 0 {
        return None;
    }
    let geometry = Geometry::new(&h);
    let output = h.width.saturating_mul(h.height).saturating_mul(nc);
    let rows = geometry.row_buffers(&h);
    let multi_scan = h.process == 2 || h.first_scan_components < nc;
    let planes = if multi_scan { geometry.planes(&h) } else { 0 };

    let Some(blocks) = geometry.remux_blocks(&h) else {
        return Some(output.saturating_add(rows).saturating_add(planes));
    };
    // The remux re-encodes every block with the stream's own tables plus
    // one interleaved scan header. Its length is not in any header; 5/4 of
    // the input covers the measured 1.06 (YCbCr) and 1.14 (CMYK) ratios.
    let remuxed = len(data).saturating_mul(5) / 4 + 1024;
    // Grid, plus the bit writer and the output it is copied into, which
    // then doubles once for the trailing EOI.
    let remux = blocks
        .saturating_mul(128)
        .saturating_add(remuxed.saturating_mul(3));
    // zune then decodes the remuxed copy (capacity 2n). A remux that bails
    // after parsing leaves zune the multi-scan stream, so take the larger.
    let zune = output
        .saturating_add(rows)
        .saturating_add(remuxed.saturating_mul(2).max(geometry.planes(&h)));
    Some(remux.max(zune))
}

/// MCU geometry (T.81 A.1.1, A.2.4).
struct Geometry {
    hmax: u64,
    vmax: u64,
    mcu_x: u64,
    mcu_y: u64,
    /// zune's interleaved layout: more than one component and a sampling
    /// factor other than 1x1.
    interleaved: bool,
}

impl Geometry {
    fn new(h: &Header) -> Self {
        let hmax = h.sampling.iter().map(|s| s.0).max().unwrap_or(1).max(1);
        let vmax = h.sampling.iter().map(|s| s.1).max().unwrap_or(1).max(1);
        Self {
            hmax,
            vmax,
            mcu_x: h.width.div_ceil(8 * hmax),
            mcu_y: h.height.div_ceil(8 * vmax),
            interleaved: h.sampling.len() > 1 && (hmax != 1 || vmax != 1),
        }
    }

    /// Blocks in the remux grid when pdfcer-jpeg-remux would interleave
    /// this stream, mirroring its acceptance test; `None` when it leaves the
    /// stream to zune unchanged.
    fn remux_blocks(&self, h: &Header) -> Option<u64> {
        let nc = len(&h.sampling);
        let uniform = h.sampling.windows(2).all(|w| w.first() == w.last());
        let mcu_blocks: u64 = h.sampling.iter().map(|&(a, b)| a * b).sum();
        let blocks = self
            .mcu_x
            .saturating_mul(self.mcu_y)
            .saturating_mul(mcu_blocks);
        let accepted = matches!(h.process, 0 | 1)
            && (2..=4).contains(&nc)
            && !uniform
            && mcu_blocks <= 10
            && h.first_scan_components < nc
            && blocks <= u64::try_from(crate::dct::MAX_REMUX_BLOCKS).unwrap_or(u64::MAX);
        accepted.then_some(blocks)
    }

    /// zune's per-component i16 row buffers (raw coefficients, the two
    /// context rows and the upsampler's destinations) and its scratch row.
    fn row_buffers(&self, h: &Header) -> u64 {
        if !self.interleaved {
            let stride = h.width.div_ceil(8) * 8;
            return 2 * len(&h.sampling) * stride * 8 + 2 * stride * 8;
        }
        let luma = h.sampling.first().copied().unwrap_or((1, 1));
        let chroma_sub = h.sampling.iter().skip(1).any(|&s| s != (1, 1));
        let doubled = if (luma.0 == 2 || luma.1 == 2) && chroma_sub {
            2
        } else {
            1
        };
        let mut total = 0u64;
        let mut widest = 0u64;
        for &(hc, vc) in &h.sampling {
            let (hc, vc) = (hc.max(1), vc.max(1));
            let stride = hc * self.mcu_x * 8;
            let ratio = (self.hmax / hc).max(1) * (self.vmax / vc).max(1);
            let raw = stride * vc * 8;
            let upsample = (stride * ratio * doubled * 8).max(raw * 2);
            total = total
                .saturating_add(raw + 2 * stride * vc + vc * stride * ratio)
                .saturating_add(upsample);
            widest = widest.max(stride * 8);
        }
        total.saturating_add(widest).saturating_mul(2)
    }

    /// zune's whole-image i16 coefficient planes for a multi-scan decode.
    fn planes(&self, h: &Header) -> u64 {
        let elements: u64 = if self.interleaved {
            h.sampling
                .iter()
                .map(|&(hc, vc)| (self.mcu_x * 64 * hc).saturating_mul(self.mcu_y * vc))
                .sum()
        } else {
            len(&h.sampling)
                .saturating_mul(h.width.div_ceil(8) * 64)
                .saturating_mul(h.height.div_ceil(8))
        };
        elements.saturating_mul(2)
    }
}

/// Walks marker segments from SOI to the first SOS (T.81 B.1.1.2, B.2).
/// Each step consumes at least two bytes, so the walk is bounded by
/// `data.len()`.
pub(crate) fn header(data: &[u8]) -> Option<Header> {
    if data.get(..2)? != [0xFF, 0xD8] {
        return None;
    }
    let mut frame: Option<Header> = None;
    let mut at = 2usize;
    loop {
        while data.get(at) == Some(&0xFF) && data.get(at + 1) == Some(&0xFF) {
            at += 1;
        }
        if *data.get(at)? != 0xFF {
            return None;
        }
        let marker = *data.get(at + 1)?;
        at += 2;
        if matches!(marker, 0x01 | 0xD0..=0xD7) {
            continue;
        }
        let length = usize::try_from(be::<2>(data, at)?).ok()?;
        let payload = data.get(at + 2..at.checked_add(length)?)?;
        match marker {
            0xC0..=0xCF if !matches!(marker, 0xC4 | 0xC8 | 0xCC) => {
                frame.get_or_insert(sof(marker, payload)?);
            }
            0xDA => {
                let mut f = frame?;
                let ns = usize::from(*payload.first()?);
                f.first_scan_components = u64::from(*payload.first()?);
                // A short scan header leaves the selectors empty, which the
                // evidence bound reads as "no opinion".
                if let Some(ss) = payload.get(1 + 2 * ns) {
                    f.first_scan_ss = *ss;
                    f.first_scan_selectors = payload
                        .get(1..1 + 2 * ns)
                        .unwrap_or_default()
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|&[cs, _]| cs)
                        .collect();
                }
                return Some(f);
            }
            0xD9 => return None,
            _ => {}
        }
        at = at.checked_add(length)?;
    }
}

/// SOF payload (T.81 B.2.2): P, Y, X, Nf, then (C, H|V, Tq) per component.
fn sof(marker: u8, p: &[u8]) -> Option<Header> {
    let nf = usize::from(*p.get(5)?);
    let specs = p.get(6..6 + 3 * nf)?;
    Some(Header {
        process: marker & 0x0F,
        precision: *p.first()?,
        height: be::<2>(p, 1)?,
        width: be::<2>(p, 3)?,
        sampling: specs
            .as_chunks::<3>()
            .0
            .iter()
            .map(|&[_, hv, _]| (u64::from(hv >> 4), u64::from(hv & 0x0F)))
            .collect(),
        ids: specs
            .as_chunks::<3>()
            .0
            .iter()
            .map(|&[c, _, _]| c)
            .collect(),
        first_scan_components: 0,
        first_scan_selectors: Vec::new(),
        first_scan_ss: 0,
    })
}
