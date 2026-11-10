//! Marker walk (T.81 B.2) and the coefficient store the scans decode into.

use crate::RemuxError;
use crate::huffman::{Decoder, parse_dht};
use crate::scan::{self, ScanComp};

/// One frame component and its quantized coefficients, zig-zag order.
pub(crate) struct Component {
    pub(crate) id: u8,
    pub(crate) h: usize,
    pub(crate) v: usize,
    /// The block grid of an interleaved scan: whole MCUs (T.81 A.2.3).
    pub(crate) grid_w: usize,
    pub(crate) grid_h: usize,
    /// The blocks a non-interleaved scan codes: `ceil(xi / 8)` by
    /// `ceil(yi / 8)` with `xi = ceil(X * Hi / Hmax)` (T.81 A.1.1, A.2.2).
    pub(crate) own_w: usize,
    pub(crate) own_h: usize,
    pub(crate) blocks: Vec<[i16; 64]>,
    /// Extent the component's scan filled, once it has been decoded.
    pub(crate) filled: Option<(usize, usize)>,
}

/// A stream ready to re-encode.
pub(crate) struct Parsed<'a> {
    /// Marker segments to keep, each with its `0xFF xx` prefix.
    pub(crate) kept: Vec<&'a [u8]>,
    pub(crate) mcux: usize,
    pub(crate) mcuy: usize,
    pub(crate) comps: Vec<Component>,
}

#[derive(Default)]
struct Walk<'a> {
    kept: Vec<&'a [u8]>,
    moved: Vec<&'a [u8]>,
    dc: [Option<Decoder>; 4],
    ac: [Option<Decoder>; 4],
    restart: usize,
    frame: Option<Parsed<'a>>,
    scans: usize,
}

/// Walks `data` and decodes every scan; `Ok(None)` when the stream is not
/// one [`crate::interleave`] rewrites.
pub(crate) fn read(data: &[u8], max_blocks: usize) -> Result<Option<Parsed<'_>>, RemuxError> {
    if data.get(..2) != Some(&[0xFF, 0xD8]) {
        return Ok(None);
    }
    let mut w = Walk::default();
    let mut i = 2;
    loop {
        while data.get(i) == Some(&0xFF) && data.get(i + 1) == Some(&0xFF) {
            i += 1;
        }
        let (Some(&0xFF), Some(&marker)) = (data.get(i), data.get(i + 1)) else {
            break;
        };
        i += 2;
        match marker {
            0xD9 => break,
            0x01 | 0xD0..=0xD8 => continue,
            _ => {}
        }
        let len = match (data.get(i), data.get(i + 1)) {
            (Some(&a), Some(&b)) => usize::from(u16::from_be_bytes([a, b])),
            _ => return Err(RemuxError::Malformed("truncated segment length")),
        };
        let payload = len
            .checked_sub(2)
            .and_then(|n| data.get(i + 2..i + 2 + n))
            .ok_or(RemuxError::Malformed("segment runs past the data"))?;
        let segment = data
            .get(i - 2..i + len)
            .ok_or(RemuxError::Malformed("segment runs past the data"))?;
        i += len;
        let step = if marker == 0xDA {
            let (step, end) = w.scan(data, i, payload)?;
            i = end;
            step
        } else {
            w.segment(marker, segment, payload, max_blocks)?
        };
        if !step {
            return Ok(None);
        }
    }
    w.finish()
}

impl<'a> Walk<'a> {
    /// Handles a non-scan segment; `false` means "not a stream to rewrite".
    fn segment(
        &mut self,
        marker: u8,
        segment: &'a [u8],
        payload: &[u8],
        max_blocks: usize,
    ) -> Result<bool, RemuxError> {
        match marker {
            0xC0 | 0xC1 if self.frame.is_none() => match frame(payload, max_blocks)? {
                Some(f) => {
                    self.kept.push(segment);
                    self.frame = Some(f);
                }
                None => return Ok(false),
            },
            // A second frame, any other coding process, DNL, or a table
            // change after the first scan (B.2.2: a component's table is
            // fixed at its first scan, so moving it could re-quantize).
            0xC0..=0xCF if marker != 0xC4 && marker != 0xC8 && marker != 0xCC => return Ok(false),
            0xDC => return Ok(false),
            0xDB if self.scans > 0 => return Ok(false),
            0xC4 => {
                for (class, dest, spec) in parse_dht(payload)? {
                    let table = Some(Decoder::new(&spec)?);
                    let set = if class == 0 {
                        &mut self.dc
                    } else {
                        &mut self.ac
                    };
                    if let Some(slot) = set.get_mut(usize::from(dest)) {
                        *slot = table;
                    }
                }
            }
            0xDD => match payload {
                [a, b] => self.restart = usize::from(u16::from_be_bytes([*a, *b])),
                _ => return Err(RemuxError::Malformed("DRI length is not 4")),
            },
            _ if self.scans > 0 => self.moved.push(segment),
            _ => self.kept.push(segment),
        }
        Ok(true)
    }

    /// Decodes one scan; returns `(continue, index after its data)`.
    fn scan(
        &mut self,
        data: &[u8],
        start: usize,
        header: &[u8],
    ) -> Result<(bool, usize), RemuxError> {
        let frame = self
            .frame
            .as_mut()
            .ok_or(RemuxError::Malformed("scan before any frame"))?;
        let comps = scan_header(header, frame, &self.dc, &self.ac)?;
        if self.scans == 0 && comps.len() == frame.comps.len() {
            return Ok((false, start));
        }
        let end = entropy_end(data, start);
        let ecs = data
            .get(start..end)
            .ok_or(RemuxError::Malformed("scan runs past the data"))?;
        scan::decode(ecs, &comps, self.restart, frame)?;
        self.scans += 1;
        Ok((true, end))
    }

    fn finish(self) -> Result<Option<Parsed<'a>>, RemuxError> {
        let Some(mut p) = self.frame else {
            return Ok(None);
        };
        if self.scans == 0 {
            return Ok(None);
        }
        for c in &mut p.comps {
            let (fw, fh) = c
                .filled
                .ok_or(RemuxError::Malformed("a component is in no scan"))?;
            pad(c, fw, fh);
        }
        p.kept = self.kept;
        p.kept.extend(self.moved);
        Ok(Some(p))
    }
}

/// Parses `SOF0`/`SOF1` (T.81 B.2.2); `None` for a frame not rewritten.
fn frame<'a>(p: &[u8], max_blocks: usize) -> Result<Option<Parsed<'a>>, RemuxError> {
    let bad = RemuxError::Malformed("short frame header");
    let (&precision, rest) = p.split_first().ok_or(bad.clone())?;
    let [y0, y1, x0, x1, nf, ..] = *rest else {
        return Err(bad);
    };
    let (height, width) = (
        usize::from(u16::from_be_bytes([y0, y1])),
        usize::from(u16::from_be_bytes([x0, x1])),
    );
    let nf = usize::from(nf);
    let specs = rest.get(5..5 + 3 * nf).ok_or(bad)?;
    let sampling: Vec<(u8, usize, usize)> = specs
        .as_chunks::<3>()
        .0
        .iter()
        .map(|&[id, hv, _]| (id, usize::from(hv >> 4), usize::from(hv & 0x0F)))
        .collect();
    if sampling
        .iter()
        .any(|&(_, h, v)| !(1..=4).contains(&h) || !(1..=4).contains(&v))
    {
        return Err(RemuxError::Malformed("sampling factor outside 1-4"));
    }
    let first = sampling.first().map(|&(_, h, v)| (h, v));
    let uniform = sampling.iter().all(|&(_, h, v)| Some((h, v)) == first);
    let mcu_blocks: usize = sampling.iter().map(|&(_, h, v)| h * v).sum();
    // B.2.3: an interleaved MCU holds at most 10 blocks, so a frame over
    // that cannot be written as one scan.
    if precision != 8
        || !(2..=4).contains(&nf)
        || height == 0
        || width == 0
        || uniform
        || mcu_blocks > 10
    {
        return Ok(None);
    }
    let hmax = sampling.iter().map(|s| s.1).max().unwrap_or(1);
    let vmax = sampling.iter().map(|s| s.2).max().unwrap_or(1);
    let (mcux, mcuy) = (width.div_ceil(8 * hmax), height.div_ceil(8 * vmax));
    let needed = mcux.saturating_mul(mcuy).saturating_mul(mcu_blocks);
    if needed > max_blocks {
        return Err(RemuxError::TooLarge {
            needed,
            ceiling: max_blocks,
        });
    }
    let comps = sampling
        .iter()
        .map(|&(id, h, v)| Component {
            id,
            h,
            v,
            grid_w: mcux * h,
            grid_h: mcuy * v,
            own_w: (width * h).div_ceil(hmax).div_ceil(8),
            own_h: (height * v).div_ceil(vmax).div_ceil(8),
            blocks: vec![[0; 64]; mcux * h * mcuy * v],
            filled: None,
        })
        .collect();
    Ok(Some(Parsed {
        kept: Vec::new(),
        mcux,
        mcuy,
        comps,
    }))
}

/// Parses a scan header (T.81 B.2.3) and resolves its tables.
fn scan_header(
    p: &[u8],
    frame: &Parsed<'_>,
    dc: &[Option<Decoder>; 4],
    ac: &[Option<Decoder>; 4],
) -> Result<Vec<ScanComp>, RemuxError> {
    let bad = RemuxError::Malformed("short scan header");
    let ns = usize::from(*p.first().ok_or(bad.clone())?);
    let specs = p.get(1..1 + 2 * ns).ok_or(bad.clone())?;
    let tail = p.get(1 + 2 * ns..4 + 2 * ns).ok_or(bad)?;
    // Sequential DCT: Ss = 0, Se = 63, Ah = Al = 0 (Table B.3).
    if ns == 0 || ns > 4 || tail != [0, 63, 0] {
        return Err(RemuxError::Malformed("not a sequential scan header"));
    }
    specs
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&[id, tables]| {
            let index = frame
                .comps
                .iter()
                .position(|c| c.id == id)
                .ok_or(RemuxError::Malformed("scan names an unknown component"))?;
            let table = |set: &[Option<Decoder>; 4], t: u8| {
                set.get(usize::from(t))
                    .and_then(Option::clone)
                    .ok_or(RemuxError::Malformed("scan uses an undefined table"))
            };
            Ok(ScanComp {
                index,
                dc: table(dc, tables >> 4)?,
                ac: table(ac, tables & 0x0F)?,
            })
        })
        .collect()
}

/// Index of the first marker after `start` that is neither stuffing nor
/// `RSTm` — where the scan's entropy-coded data ends (T.81 B.1.1.5).
fn entropy_end(data: &[u8], start: usize) -> usize {
    data.get(start..)
        .and_then(|tail| {
            tail.windows(2)
                .position(|w| matches!(w, [0xFF, b] if !matches!(b, 0x00 | 0xD0..=0xD7 | 0xFF)))
        })
        .map_or(data.len(), |k| start + k)
}

/// Fills the blocks past a component's coded extent with copies of the
/// nearest coded block, so an upsampler reading across the image edge sees
/// the edge continued rather than a hard step.
fn pad(c: &mut Component, fw: usize, fh: usize) {
    for by in 0..c.grid_h {
        for bx in 0..c.grid_w {
            if bx < fw && by < fh {
                continue;
            }
            let src = by.min(fh.saturating_sub(1)) * c.grid_w + bx.min(fw.saturating_sub(1));
            if let Some(&block) = c.blocks.get(src)
                && let Some(dst) = c.blocks.get_mut(by * c.grid_w + bx)
            {
                *dst = block;
            }
        }
    }
}
